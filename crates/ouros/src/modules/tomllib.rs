//! Implementation of the `tomllib` module.
//!
//! This module provides `tomllib.load()` and `tomllib.loads()` using a TOML 1.0
//! parser (`toml_edit`) and converts decoded values into Ouros runtime values.
//! It exposes a dedicated `TOMLDecodeError` type and enforces CPython-compatible
//! `parse_float` behavior for supported callback shapes.

use toml_edit::{Array, ArrayOfTables, DocumentMut, Formatted, InlineTable, Item, Table, Value as TomlValue};

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Date, Datetime, Dict, List, Module, PyTrait, Str, Time, Timedelta, Timezone, Type},
    value::{EitherStr, Value},
};

/// `tomllib` module call targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum TomllibFunctions {
    /// Implements `tomllib.load`.
    Load,
    /// Implements `tomllib.loads`.
    Loads,
}

/// Parsed `tomllib` keyword arguments.
///
/// `parse_float` defaults to `float` exactly like CPython.
#[derive(Debug)]
struct TomllibKwargs {
    parse_float: Value,
}

impl Default for TomllibKwargs {
    fn default() -> Self {
        Self {
            parse_float: Value::Builtin(Builtins::Type(Type::Float)),
        }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for TomllibKwargs {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.parse_float.drop_with_heap(heap);
    }
}

/// Creates the `tomllib` module instance.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Tomllib);
    module.set_attr_text(
        "load",
        Value::ModuleFunction(ModuleFunctions::Tomllib(TomllibFunctions::Load)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "loads",
        Value::ModuleFunction(ModuleFunctions::Tomllib(TomllibFunctions::Loads)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "TOMLDecodeError",
        Value::Builtin(Builtins::ExcType(ExcType::TOMLDecodeError)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches `tomllib` module calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TomllibFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        TomllibFunctions::Load => load(heap, interns, args)?,
        TomllibFunctions::Loads => loads(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Implements `tomllib.load(fp, /, *, parse_float=float)`.
fn load(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let fp = positional.next();
    let positional_count = usize::from(fp.is_some()) + positional.len();
    if positional_count > 1 {
        fp.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("load", 1, positional_count, 0));
    }

    let kwargs = match extract_tomllib_kwargs(kwargs, "load", "fp", heap, interns) {
        Ok(kwargs) => kwargs,
        Err(err) => {
            fp.drop_with_heap(heap);
            return Err(err);
        }
    };
    let mut kwargs_guard = HeapGuard::new(kwargs, heap);
    let (kwargs, heap) = kwargs_guard.as_parts_mut();

    let Some(fp) = fp else {
        return Err(ExcType::type_error_missing_positional_with_names("load", &["fp"]));
    };

    let mut fp_guard = HeapGuard::new(fp, heap);
    let (fp, heap) = fp_guard.as_parts_mut();
    let source = read_toml_source_from_file_like(fp, heap, interns)?;
    parse_toml_source(&source, &kwargs.parse_float, heap, interns)
}

/// Implements `tomllib.loads(s, /, *, parse_float=float)`.
fn loads(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let src = positional.next();
    let positional_count = usize::from(src.is_some()) + positional.len();
    if positional_count > 1 {
        src.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("loads", 1, positional_count, 0));
    }

    let kwargs = match extract_tomllib_kwargs(kwargs, "loads", "s", heap, interns) {
        Ok(kwargs) => kwargs,
        Err(err) => {
            src.drop_with_heap(heap);
            return Err(err);
        }
    };
    let mut kwargs_guard = HeapGuard::new(kwargs, heap);
    let (kwargs, heap) = kwargs_guard.as_parts_mut();

    let Some(src) = src else {
        return Err(ExcType::type_error_missing_positional_with_names("loads", &["s"]));
    };

    let mut src_guard = HeapGuard::new(src, heap);
    let (src, heap) = src_guard.as_parts_mut();
    let source = extract_loads_source_string(src, heap, interns)?;
    parse_toml_source(&source, &kwargs.parse_float, heap, interns)
}

/// Parses keyword arguments shared by `tomllib.load` and `tomllib.loads`.
fn extract_tomllib_kwargs(
    kwargs: KwargsValues,
    function_name: &str,
    positional_only_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<TomllibKwargs> {
    let mut parsed = TomllibKwargs::default();

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let keyword_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if keyword_name.as_str() == "parse_float" {
            parsed.parse_float.drop_with_heap(heap);
            parsed.parse_float = value;
        } else {
            value.drop_with_heap(heap);
            if keyword_name == positional_only_name {
                return Err(ExcType::type_error_positional_only(function_name, positional_only_name));
            }
            return Err(ExcType::type_error_unexpected_keyword(function_name, &keyword_name));
        }
    }

    Ok(parsed)
}

/// Reads TOML source text from a file-like object's `read()` result.
///
/// CPython requires `load()` readers to provide bytes/bytearray and raises a
/// binary-mode `TypeError` for other return types.
fn read_toml_source_from_file_like(
    fp: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let Value::Ref(fp_id) = fp else {
        return Err(ExcType::attribute_error(fp.py_type(heap), "read"));
    };

    let result = heap.call_attr_raw(*fp_id, &EitherStr::Heap("read".to_owned()), ArgValues::Empty, interns)?;
    let bytes = match result {
        AttrCallResult::Value(value) => {
            defer_drop!(value, heap);
            extract_binary_mode_bytes(value, heap, interns)?
        }
        other => {
            super::json::drop_non_value_attr_result(other, heap);
            return Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "tomllib.load() expected fp.read() to return a value immediately",
            )
            .into());
        }
    };

    decode_utf8_source(&bytes)
}

/// Extracts `bytes`/`bytearray` payload for `tomllib.load`.
fn extract_binary_mode_bytes(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(bytes_id) => Ok(interns.get_bytes(*bytes_id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            _ => Err(tomllib_binary_mode_type_error()),
        },
        _ => Err(tomllib_binary_mode_type_error()),
    }
}

/// Decodes UTF-8 TOML source bytes.
fn decode_utf8_source(bytes: &[u8]) -> RunResult<String> {
    std::str::from_utf8(bytes)
        .map(std::borrow::ToOwned::to_owned)
        .map_err(|_| ExcType::unicode_decode_error_invalid_utf8())
}

/// Returns CPython-compatible `tomllib.load` binary-mode type error text.
fn tomllib_binary_mode_type_error() -> crate::exception_private::RunError {
    ExcType::type_error("File must be opened in binary mode, e.g. use `open('foo.toml', 'rb')`".to_string())
}

/// Validates and extracts the source string for `tomllib.loads`.
fn extract_loads_source_string(src: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let Some(text) = src.as_either_str(heap) else {
        return Err(ExcType::type_error(format!(
            "Expected str object, not '{}'",
            src.py_type(heap)
        )));
    };
    Ok(text.as_str(interns).to_owned())
}

/// Parses TOML source and converts to Ouros values.
fn parse_toml_source(
    source: &str,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let document = source
        .parse::<DocumentMut>()
        .map_err(|error| toml_decode_error_from_parse_error(&error, source))?;
    convert_toml_table(document.as_table(), parse_float, heap, interns)
}

/// Converts a `toml_edit` parse error to `tomllib.TOMLDecodeError`.
fn toml_decode_error_from_parse_error(
    error: &toml_edit::TomlError,
    source: &str,
) -> crate::exception_private::RunError {
    let mut message = normalize_toml_error_message(error.message());
    if let Some(span) = error.span() {
        let (line, column) = line_and_column_from_offset(source, span.start);
        message = format!("{message} (at line {}, column {})", line + 1, column + 1);
    }
    SimpleException::new_msg(ExcType::TOMLDecodeError, message).into()
}

/// Normalizes parser messages into one-line `TOMLDecodeError` text.
fn normalize_toml_error_message(message: &str) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "Invalid TOML document".to_string();
    }
    let mut chars = normalized.chars();
    let Some(first) = chars.next() else {
        return normalized;
    };
    if first.is_ascii_lowercase() {
        let mut capitalized = String::with_capacity(normalized.len());
        capitalized.push(first.to_ascii_uppercase());
        capitalized.push_str(chars.as_str());
        return capitalized;
    }
    normalized
}

/// Converts a byte offset into zero-based `(line, column)` coordinates.
fn line_and_column_from_offset(source: &str, offset: usize) -> (usize, usize) {
    let mut clamped = offset.min(source.len());
    while clamped > 0 && !source.is_char_boundary(clamped) {
        clamped -= 1;
    }
    let prefix = &source[..clamped];
    let line = prefix.chars().filter(|&ch| ch == '\n').count();
    let column = prefix.rsplit('\n').next().map_or(0, |segment| segment.chars().count());
    (line, column)
}

/// Converts a TOML `Table` into a Python `dict`.
fn convert_toml_table(
    table: &Table,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut dict_guard = HeapGuard::new(Dict::new(), heap);
    let (dict, heap) = dict_guard.as_parts_mut();

    for (key, item) in table {
        let value = convert_toml_item(item, parse_float, heap, interns)?;
        let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
        if let Some(old_value) = dict.set(Value::Ref(key_id), value, heap, interns)? {
            old_value.drop_with_heap(heap);
        }
    }

    let (dict, heap) = dict_guard.into_parts();
    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(dict_id))
}

/// Converts a TOML `Item` into a Python value.
fn convert_toml_item(
    item: &Item,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match item {
        Item::Value(value) => convert_toml_value(value, parse_float, heap, interns),
        Item::Table(table) => convert_toml_table(table, parse_float, heap, interns),
        Item::ArrayOfTables(array) => convert_toml_array_of_tables(array, parse_float, heap, interns),
        Item::None => Ok(Value::None),
    }
}

/// Converts a TOML scalar/container value into a Python value.
fn convert_toml_value(
    value: &TomlValue,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match value {
        TomlValue::String(value) => {
            let id = heap.allocate(HeapData::Str(Str::from(value.value().as_str())))?;
            Ok(Value::Ref(id))
        }
        TomlValue::Integer(value) => Ok(Value::Int(*value.value())),
        TomlValue::Float(value) => convert_toml_float(value, parse_float, heap, interns),
        TomlValue::Boolean(value) => Ok(Value::Bool(*value.value())),
        TomlValue::Datetime(value) => convert_toml_datetime(value.value(), heap),
        TomlValue::Array(array) => convert_toml_array(array, parse_float, heap, interns),
        TomlValue::InlineTable(table) => convert_toml_inline_table(table, parse_float, heap, interns),
    }
}

/// Converts a TOML float while applying `parse_float`.
fn convert_toml_float(
    value: &Formatted<f64>,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if !is_value_callable(parse_float, heap, interns) {
        return Err(ExcType::type_error_not_callable(parse_float.py_type(heap)));
    }

    let token = value
        .as_repr()
        .and_then(|repr| repr.as_raw().as_str())
        .unwrap_or("")
        .to_owned();
    let token = if token.is_empty() {
        value.value().to_string()
    } else {
        token
    };

    let arg_id = heap.allocate(HeapData::Str(Str::from(token.as_str())))?;
    let callback_value =
        super::json::try_call_supported_callback(parse_float, ArgValues::One(Value::Ref(arg_id)), heap, interns)?;
    if let Some(callback_value) = callback_value {
        if parse_float_result_is_disallowed(&callback_value, heap) {
            callback_value.drop_with_heap(heap);
            return Err(
                SimpleException::new_msg(ExcType::ValueError, "parse_float must not return dicts or lists").into(),
            );
        }
        return Ok(callback_value);
    }

    // Fallback for callable shapes not synchronously executable in module context.
    Ok(Value::Float(*value.value()))
}

/// Returns true when a `parse_float` callback result is forbidden by CPython.
fn parse_float_result_is_disallowed(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    let Value::Ref(id) = value else {
        return false;
    };
    matches!(heap.get(*id), HeapData::Dict(_) | HeapData::List(_))
}

/// Converts a TOML array into a Python list.
fn convert_toml_array(
    array: &Array,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut items_guard = HeapGuard::new(Vec::<Value>::new(), heap);
    let (items, heap) = items_guard.as_parts_mut();

    for item in array {
        items.push(convert_toml_value(item, parse_float, heap, interns)?);
    }

    let (items, heap) = items_guard.into_parts();
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Converts a TOML inline table into a Python dict.
fn convert_toml_inline_table(
    table: &InlineTable,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut dict_guard = HeapGuard::new(Dict::new(), heap);
    let (dict, heap) = dict_guard.as_parts_mut();

    for (key, value) in table {
        let converted = convert_toml_value(value, parse_float, heap, interns)?;
        let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
        if let Some(old_value) = dict.set(Value::Ref(key_id), converted, heap, interns)? {
            old_value.drop_with_heap(heap);
        }
    }

    let (dict, heap) = dict_guard.into_parts();
    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(dict_id))
}

/// Converts a TOML array-of-tables into a Python list of dicts.
fn convert_toml_array_of_tables(
    array: &ArrayOfTables,
    parse_float: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut items_guard = HeapGuard::new(Vec::<Value>::new(), heap);
    let (items, heap) = items_guard.as_parts_mut();

    for table in array {
        items.push(convert_toml_table(table, parse_float, heap, interns)?);
    }

    let (items, heap) = items_guard.into_parts();
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Converts TOML datetime/date/time values to `datetime` module objects.
fn convert_toml_datetime(value: &toml_edit::Datetime, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    match (value.date, value.time, value.offset) {
        (Some(date), Some(time), offset) => {
            let tzinfo = convert_toml_offset(offset, heap)?;
            let microsecond =
                i32::try_from(time.nanosecond / 1_000).expect("nanoseconds divided by 1000 always fit in i32");
            let datetime = Datetime::new(
                i32::from(date.year),
                i32::from(date.month),
                i32::from(date.day),
                i32::from(time.hour),
                i32::from(time.minute),
                i32::from(time.second),
                microsecond,
                tzinfo,
                0,
            )?;
            let id = heap.allocate(HeapData::Datetime(datetime))?;
            Ok(Value::Ref(id))
        }
        (Some(date), None, None) => {
            let date = Date::new(i32::from(date.year), i32::from(date.month), i32::from(date.day))?;
            let id = heap.allocate(HeapData::Date(date))?;
            Ok(Value::Ref(id))
        }
        (None, Some(time), None) => {
            let microsecond =
                i32::try_from(time.nanosecond / 1_000).expect("nanoseconds divided by 1000 always fit in i32");
            let time = Time::new(
                i32::from(time.hour),
                i32::from(time.minute),
                i32::from(time.second),
                microsecond,
                None,
                0,
            )?;
            let id = heap.allocate(HeapData::Time(time))?;
            Ok(Value::Ref(id))
        }
        _ => Err(SimpleException::new_msg(ExcType::TOMLDecodeError, "Invalid datetime").into()),
    }
}

/// Converts TOML offsets to `datetime.timezone` heap objects.
fn convert_toml_offset(
    offset: Option<toml_edit::Offset>,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Option<HeapId>> {
    let Some(offset) = offset else {
        return Ok(None);
    };
    let timezone = match offset {
        toml_edit::Offset::Z => Timezone::utc(),
        toml_edit::Offset::Custom { minutes } => {
            let seconds = i64::from(minutes) * 60;
            let delta = Timedelta::new(0, seconds, 0)?;
            Timezone::new(delta, None)?
        }
    };
    let id = heap.allocate(HeapData::Timezone(timezone))?;
    Ok(Some(id))
}

/// Returns whether a runtime `Value` is callable by Ouros's dispatch model.
fn is_value_callable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Marker(marker) => marker.is_callable(),
        Value::Ref(heap_id) => is_heap_value_callable(*heap_id, heap, interns),
        _ => false,
    }
}

/// Returns whether the heap object represented by `heap_id` can be called.
fn is_heap_value_callable(heap_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match heap.get(heap_id) {
        HeapData::ClassSubclasses(_)
        | HeapData::ClassGetItem(_)
        | HeapData::GenericAlias(_)
        | HeapData::FunctionGet(_)
        | HeapData::WeakRef(_)
        | HeapData::ClassObject(_)
        | HeapData::BoundMethod(_)
        | HeapData::Partial(_)
        | HeapData::SingleDispatch(_)
        | HeapData::SingleDispatchRegister(_)
        | HeapData::SingleDispatchMethod(_)
        | HeapData::CmpToKey(_)
        | HeapData::ItemGetter(_)
        | HeapData::AttrGetter(_)
        | HeapData::MethodCaller(_)
        | HeapData::PropertyAccessor(_)
        | HeapData::Closure(_, _, _)
        | HeapData::FunctionDefaults(_, _)
        | HeapData::ObjectNewImpl(_) => true,
        HeapData::Instance(instance) => instance_is_callable(instance.class_id(), heap, interns),
        _ => false,
    }
}

/// Returns true when instances of `class_id` expose `__call__`.
fn instance_is_callable(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
        return false;
    };
    class_obj.mro_has_attr("__call__", class_id, heap, interns)
}
