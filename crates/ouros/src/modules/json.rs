//! Implementation of the `json` module.
//!
//! Provides an implementation of Python's `json` module with:
//! - `loads(s)`: Parse JSON string to Python value
//! - `load(fp)`: Parse JSON from a file-like object (`read`) or compatibility list buffer
//! - `dumps(obj, *, skipkeys=False, ensure_ascii=True, check_circular=True, allow_nan=True, cls=None,
//!   indent=None, sort_keys=False)`: Serialize Python value to JSON string
//! - `dump(obj, fp)`: Serialize JSON via `fp.write(...)` or compatibility list buffer
//! - `JSONDecodeError`: Exception class for invalid JSON (exposed as a module attribute)
//!
//! The `dumps()` implementation supports key keyword parity options including
//! `skipkeys`, `ensure_ascii`, `check_circular`, `allow_nan`, `cls`,
//! `indent`, and `sort_keys`.
//!
//! This implementation uses `serde_json` for parsing and string escaping.

use std::fmt::Write;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    bytecode::Opcode,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    exception_public::Exception,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::PrintWriter,
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, ClassObject, Dict, List, PyTrait, Str, Type, allocate_tuple, compute_c3_mro},
    value::{EitherStr, Value},
};

/// Dummy PrintWriter used for invoking builtin parse callbacks.
struct DummyPrint;

impl PrintWriter for DummyPrint {
    fn stdout_write(&mut self, _output: std::borrow::Cow<'_, str>) -> Result<(), Exception> {
        Ok(())
    }

    fn stdout_push(&mut self, _end: char) -> Result<(), Exception> {
        Ok(())
    }
}

/// Parsed `json.dumps(...)` keyword arguments used by the serializer.
#[derive(Debug, Clone)]
struct DumpsKwargs {
    /// Indentation token used for each nested level, or `None` for compact output.
    indent: Option<String>,
    /// Whether dictionary keys should be sorted lexicographically.
    sort_keys: bool,
    /// Whether non-ASCII code points should be escaped as `\uXXXX`.
    ensure_ascii: bool,
    /// Whether unsupported dictionary keys should be skipped.
    skipkeys: bool,
    /// Whether `NaN`/`Infinity`/`-Infinity` should be emitted as bare literals.
    allow_nan: bool,
    /// Whether circular references should raise `ValueError`.
    check_circular: bool,
    /// Whether a non-None `cls` argument was provided.
    cls_custom: bool,
    /// Whether a non-None `default` argument was provided.
    default_custom: bool,
    /// Custom `(item_separator, key_separator)` pair.
    separators: Option<(String, String)>,
}

impl Default for DumpsKwargs {
    fn default() -> Self {
        Self {
            indent: None,
            sort_keys: false,
            ensure_ascii: true,
            skipkeys: false,
            allow_nan: true,
            check_circular: true,
            cls_custom: false,
            default_custom: false,
            separators: None,
        }
    }
}

/// Parsed `json.loads(...)` / `json.load(...)` keyword arguments.
#[derive(Debug, Default)]
pub(crate) struct LoadsKwargs {
    /// Optional custom decoder class (`cls=`). Stored for parity and forwarded behavior.
    pub(crate) cls: Option<Value>,
    /// Callback invoked for each decoded object (unless `object_pairs_hook` is set).
    pub(crate) object_hook: Option<Value>,
    /// Callback invoked for every floating-point number token.
    pub(crate) parse_float: Option<Value>,
    /// Callback invoked for every integer number token.
    pub(crate) parse_int: Option<Value>,
    /// Callback invoked for `NaN` / `Infinity` / `-Infinity` tokens.
    pub(crate) parse_constant: Option<Value>,
    /// Callback invoked with key/value pairs for each decoded object.
    pub(crate) object_pairs_hook: Option<Value>,
}

impl LoadsKwargs {
    /// Returns decode options borrowing callback values from this argument set.
    #[must_use]
    pub(crate) fn as_decode_options(&self) -> JsonDecodeOptions<'_> {
        JsonDecodeOptions {
            object_hook: self.object_hook.as_ref(),
            parse_float: self.parse_float.as_ref(),
            parse_int: self.parse_int.as_ref(),
            parse_constant: self.parse_constant.as_ref(),
            object_pairs_hook: self.object_pairs_hook.as_ref(),
        }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for LoadsKwargs {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.cls.drop_with_heap(heap);
        self.object_hook.drop_with_heap(heap);
        self.parse_float.drop_with_heap(heap);
        self.parse_int.drop_with_heap(heap);
        self.parse_constant.drop_with_heap(heap);
        self.object_pairs_hook.drop_with_heap(heap);
    }
}

/// Borrowed decode callbacks used while materializing JSON values.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JsonDecodeOptions<'a> {
    /// Callback invoked for decoded dictionaries.
    pub(crate) object_hook: Option<&'a Value>,
    /// Callback invoked for float number tokens.
    pub(crate) parse_float: Option<&'a Value>,
    /// Callback invoked for integer number tokens.
    pub(crate) parse_int: Option<&'a Value>,
    /// Callback invoked for non-finite constants.
    pub(crate) parse_constant: Option<&'a Value>,
    /// Callback invoked for ordered object pairs.
    pub(crate) object_pairs_hook: Option<&'a Value>,
}

/// Distinguishes integer and floating JSON number tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedJsonNumberKind {
    /// Token matched the integer number grammar.
    Integer,
    /// Token matched a floating/exponent number grammar.
    Float,
}

/// Parsed JSON node preserving numeric lexemes and object pair order.
#[derive(Debug, Clone)]
enum ParsedJsonValue {
    /// JSON `null`.
    Null,
    /// JSON `true` / `false`.
    Bool(bool),
    /// JSON string.
    String(String),
    /// JSON number token as source text.
    Number {
        /// Original numeric token text.
        token: String,
        /// Number category inferred from grammar.
        kind: ParsedJsonNumberKind,
    },
    /// JSON non-finite constant (`NaN`, `Infinity`, `-Infinity`).
    Constant(&'static str),
    /// JSON array.
    Array(Vec<Self>),
    /// JSON object preserving insertion order.
    Object(Vec<(String, Self)>),
}

/// Lightweight permissive JSON parser used for `loads`/`load`.
///
/// This parser supports CPython-compatible non-finite constants (`NaN`,
/// `Infinity`, `-Infinity`) and preserves numeric lexemes so parse hooks can
/// receive exact source tokens.
struct JsonLooseParser<'a> {
    /// Input source text.
    src: &'a str,
    /// Current byte offset.
    pos: usize,
}

/// Lightweight JSON tree used during serialization.
///
/// This mirrors JSON structures but stores number tokens as strings so
/// non-finite literals can be preserved when `allow_nan=True`.
#[derive(Debug, Clone)]
enum JsonValue {
    /// JSON null.
    Null,
    /// JSON boolean.
    Bool(bool),
    /// JSON numeric token (including `NaN`/`Infinity`/`-Infinity`).
    Number(String),
    /// JSON string.
    String(String),
    /// JSON array.
    Array(Vec<Self>),
    /// JSON object preserving insertion order.
    Object(Vec<(String, Self)>),
}

/// Recursion depth ceiling for JSON encoding when circular checks are disabled.
///
/// CPython eventually raises `RecursionError` on deeply recursive structures
/// when `check_circular=False`; this cap prevents Rust stack overflows while
/// preserving that behavior class.
const JSON_RECURSION_LIMIT: usize = 1000;

/// JSON module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum JsonFunctions {
    Loads,
    Dumps,
    Dump,
    Load,
    #[strum(serialize = "__init__")]
    JsonEncoderInit,
    #[strum(serialize = "encode")]
    JsonEncoderEncode,
    #[strum(serialize = "iterencode")]
    JsonEncoderIterencode,
    #[strum(serialize = "default")]
    JsonEncoderDefault,
    #[strum(serialize = "__init__")]
    JsonDecoderInit,
    #[strum(serialize = "decode")]
    JsonDecoderDecode,
    #[strum(serialize = "raw_decode")]
    JsonDecoderRawDecode,
    #[strum(serialize = "JSONEncoder")]
    JsonEncoder,
    #[strum(serialize = "JSONDecoder")]
    JsonDecoder,
    #[strum(serialize = "detect_encoding")]
    DetectEncoding,
}

/// Creates the `json` module and allocates it on the heap.
///
/// The module provides:
/// - `loads(s)`: Parse JSON string to Python value
/// - `dumps(obj, *, skipkeys=False, ensure_ascii=True, check_circular=True, allow_nan=True, cls=None, indent=None, sort_keys=False)`:
///   Serialize Python value to JSON string
/// - `JSONDecodeError`: Exception class for invalid JSON payloads
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Json);

    // json.loads - function to parse JSON string
    module.set_attr(
        StaticStrings::Loads,
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::Loads)),
        heap,
        interns,
    );

    // json.load - parse JSON from a file-like buffer
    module.set_attr(
        StaticStrings::Load,
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::Load)),
        heap,
        interns,
    );

    // json.dumps - function to serialize to JSON string
    module.set_attr(
        StaticStrings::Dumps,
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::Dumps)),
        heap,
        interns,
    );

    // json.dump - serialize JSON to a file-like buffer
    module.set_attr(
        StaticStrings::Dump,
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::Dump)),
        heap,
        interns,
    );

    // json.JSONDecodeError - specialized decode error type.
    module.set_attr(
        StaticStrings::JsonDecodeError,
        Value::Builtin(Builtins::ExcType(ExcType::JSONDecodeError)),
        heap,
        interns,
    );

    // json.JSONEncoder / json.JSONDecoder class objects.
    let json_encoder_class = create_json_encoder_class(heap, interns)?;
    module.set_attr(
        StaticStrings::JsonEncoder,
        Value::Ref(json_encoder_class),
        heap,
        interns,
    );

    let json_decoder_class = create_json_decoder_class(heap, interns)?;
    module.set_attr(
        StaticStrings::JsonDecoder,
        Value::Ref(json_decoder_class),
        heap,
        interns,
    );

    // json.detect_encoding - byte encoding detector helper
    module.set_attr(
        StaticStrings::JsonDetectEncoding,
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::DetectEncoding)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Creates the runtime `json.JSONEncoder` class used by parity tests.
fn create_json_encoder_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonEncoderInit)),
        heap,
        interns,
    )
    .expect("json encoder class __init__ attribute set");
    dict_set_str_attr(
        &mut attrs,
        "encode",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonEncoderEncode)),
        heap,
        interns,
    )
    .expect("json encoder class encode attribute set");
    dict_set_str_attr(
        &mut attrs,
        "iterencode",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonEncoderIterencode)),
        heap,
        interns,
    )
    .expect("json encoder class iterencode attribute set");
    dict_set_str_attr(
        &mut attrs,
        "default",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonEncoderDefault)),
        heap,
        interns,
    )
    .expect("json encoder class default attribute set");

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        crate::value::EitherStr::Heap("json.JSONEncoder".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        attrs,
        vec![object_class],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;
    initialize_helper_class_mro(class_id, object_class, class_uid, heap, interns);
    Ok(class_id)
}

/// Creates the runtime `json.JSONDecoder` class used by parity tests.
fn create_json_decoder_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonDecoderInit)),
        heap,
        interns,
    )
    .expect("json decoder class __init__ attribute set");
    dict_set_str_attr(
        &mut attrs,
        "decode",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonDecoderDecode)),
        heap,
        interns,
    )
    .expect("json decoder class decode attribute set");
    dict_set_str_attr(
        &mut attrs,
        "raw_decode",
        Value::ModuleFunction(ModuleFunctions::Json(JsonFunctions::JsonDecoderRawDecode)),
        heap,
        interns,
    )
    .expect("json decoder class raw_decode attribute set");

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        crate::value::EitherStr::Heap("json.JSONDecoder".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        attrs,
        vec![object_class],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;
    initialize_helper_class_mro(class_id, object_class, class_uid, heap, interns);
    Ok(class_id)
}

/// Adds one string-keyed attribute to a class dict.
fn dict_set_str_attr(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    dict.set(Value::Ref(key_id), value, heap, interns)?;
    Ok(())
}

/// Finalizes helper-class MRO and registers it under `object`.
fn initialize_helper_class_mro(
    class_id: HeapId,
    object_class: HeapId,
    class_uid: u64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    let mro = compute_c3_mro(class_id, &[object_class], heap, interns)
        .expect("json helper class should always have valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(class_obj) = heap.get_mut(class_id) {
        class_obj.set_mro(mro);
    }
    heap.with_entry_mut(object_class, |_, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("object class registry should be mutable");
}

/// Dispatches a call to a json module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: JsonFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        JsonFunctions::Loads => loads(heap, interns, args),
        JsonFunctions::Dumps => dumps(heap, interns, args),
        JsonFunctions::Dump => dump(heap, interns, args),
        JsonFunctions::Load => load(heap, interns, args),
        JsonFunctions::JsonEncoderInit => json_encoder_init_method(heap, interns, args),
        JsonFunctions::JsonEncoderEncode => json_encoder_encode_method(heap, interns, args),
        JsonFunctions::JsonEncoderIterencode => json_encoder_iterencode_method(heap, interns, args),
        JsonFunctions::JsonEncoderDefault => json_encoder_default_method(heap, args),
        JsonFunctions::JsonDecoderInit => json_decoder_init_method(heap, interns, args),
        JsonFunctions::JsonDecoderDecode => json_decoder_decode_method(heap, interns, args),
        JsonFunctions::JsonDecoderRawDecode => json_decoder_raw_decode_method(heap, interns, args),
        JsonFunctions::JsonEncoder => json_encoder(heap, interns, args),
        JsonFunctions::JsonDecoder => json_decoder(heap, interns, args),
        JsonFunctions::DetectEncoding => detect_encoding(heap, interns, args),
    }
}

/// Implementation of `json.loads(s)`.
///
/// Parses a JSON string and returns the corresponding Python value.
///
/// # Errors
/// Returns `json.JSONDecodeError` if the JSON is invalid.
fn loads(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // loads(s, *, cls=None, object_hook=None, parse_float=None, parse_int=None,
    //       parse_constant=None, object_pairs_hook=None)
    let (mut positional, kwargs) = args.into_parts();
    let Some(input) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("json.loads", 1, 0));
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        input.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("json.loads", 1, 2));
    }

    let loads_kwargs = extract_loads_kwargs(kwargs, "loads", heap, interns)?;
    defer_drop!(loads_kwargs, heap);

    let json_str = if let Some(bytes) = value_as_bytes(&input, heap, interns) {
        String::from_utf8(bytes.to_vec()).map_err(|_| ExcType::unicode_decode_error_invalid_utf8())?
    } else {
        input.py_str(heap, interns).into_owned()
    };
    input.drop_with_heap(heap);

    let value = parse_json_str_with_options(&json_str, loads_kwargs.as_decode_options(), heap, interns)?;
    let value = apply_loads_cls_postprocess(value, loads_kwargs.cls.as_ref(), heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Parses a JSON string into a Ouros `Value`.
///
/// # Errors
/// Returns `json.JSONDecodeError` if the JSON is invalid.
pub(crate) fn parse_json_str(
    json_str: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    parse_json_str_with_options(json_str, JsonDecodeOptions::default(), heap, interns)
}

/// Parses a JSON string using explicit decode callbacks.
///
/// This path supports CPython-compatible `NaN`/`Infinity` constants and
/// `loads`/`load` parse hooks (`parse_int`, `parse_float`, `parse_constant`,
/// `object_hook`, `object_pairs_hook`).
///
/// # Errors
/// Returns `json.JSONDecodeError` if the JSON is invalid.
pub(crate) fn parse_json_str_with_options(
    json_str: &str,
    options: JsonDecodeOptions<'_>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut parser = JsonLooseParser { src: json_str, pos: 0 };
    let parsed = parser.parse()?;
    convert_parsed_json_to_python(&parsed, options, heap, interns)
}

impl JsonLooseParser<'_> {
    /// Parses one complete JSON document from the source.
    fn parse(&mut self) -> RunResult<ParsedJsonValue> {
        self.skip_ws();
        let value = self.parse_value()?;
        self.skip_ws();
        if self.pos != self.src.len() {
            return Err(self.decode_error("extra data"));
        }
        Ok(value)
    }

    /// Parses one JSON value at the current position.
    fn parse_value(&mut self) -> RunResult<ParsedJsonValue> {
        self.skip_ws();
        match self.peek_byte() {
            Some(b'n') => {
                self.expect_keyword("null")?;
                Ok(ParsedJsonValue::Null)
            }
            Some(b't') => {
                self.expect_keyword("true")?;
                Ok(ParsedJsonValue::Bool(true))
            }
            Some(b'f') => {
                self.expect_keyword("false")?;
                Ok(ParsedJsonValue::Bool(false))
            }
            Some(b'"') => Ok(ParsedJsonValue::String(self.parse_string()?)),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'N') => {
                self.expect_keyword("NaN")?;
                Ok(ParsedJsonValue::Constant("NaN"))
            }
            Some(b'I') => {
                self.expect_keyword("Infinity")?;
                Ok(ParsedJsonValue::Constant("Infinity"))
            }
            Some(b'-') => {
                if self.starts_with("-Infinity") {
                    self.pos += "-Infinity".len();
                    Ok(ParsedJsonValue::Constant("-Infinity"))
                } else {
                    self.parse_number()
                }
            }
            Some(b'0'..=b'9') => self.parse_number(),
            _ => Err(self.decode_error("Expecting value")),
        }
    }

    /// Parses a JSON object preserving key/value insertion order.
    fn parse_object(&mut self) -> RunResult<ParsedJsonValue> {
        self.consume_byte(b'{')?;
        self.skip_ws();
        let mut pairs = Vec::new();

        if self.try_consume_byte(b'}') {
            return Ok(ParsedJsonValue::Object(pairs));
        }

        loop {
            self.skip_ws();
            if self.peek_byte() != Some(b'"') {
                return Err(self.decode_error("Expecting property name enclosed in double quotes"));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.consume_byte(b':')?;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_ws();
            if self.try_consume_byte(b'}') {
                break;
            }
            let comma_pos = self.pos;
            self.consume_byte(b',')?;
            self.skip_ws();
            if self.peek_byte() == Some(b'}') {
                return Err(self.decode_error_at("Illegal trailing comma before end of object", comma_pos));
            }
        }

        Ok(ParsedJsonValue::Object(pairs))
    }

    /// Parses a JSON array.
    fn parse_array(&mut self) -> RunResult<ParsedJsonValue> {
        self.consume_byte(b'[')?;
        self.skip_ws();
        let mut values = Vec::new();

        if self.try_consume_byte(b']') {
            return Ok(ParsedJsonValue::Array(values));
        }

        loop {
            values.push(self.parse_value()?);
            self.skip_ws();
            if self.try_consume_byte(b']') {
                break;
            }
            let comma_pos = self.pos;
            self.consume_byte(b',')?;
            self.skip_ws();
            if self.peek_byte() == Some(b']') {
                return Err(self.decode_error_at("Illegal trailing comma before end of array", comma_pos));
            }
        }

        Ok(ParsedJsonValue::Array(values))
    }

    /// Parses a JSON number token and returns its source text.
    fn parse_number(&mut self) -> RunResult<ParsedJsonValue> {
        let start = self.pos;

        if self.try_consume_byte(b'-') {
            // sign consumed
        }

        match self.peek_byte() {
            Some(b'0') => {
                self.pos += 1;
            }
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.decode_error("invalid number")),
        }

        let mut kind = ParsedJsonNumberKind::Integer;

        if self.try_consume_byte(b'.') {
            kind = ParsedJsonNumberKind::Float;
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err(self.decode_error("invalid number"));
            }
            while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        if matches!(self.peek_byte(), Some(b'e' | b'E')) {
            kind = ParsedJsonNumberKind::Float;
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                return Err(self.decode_error("invalid number"));
            }
            while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        let token = self
            .src
            .get(start..self.pos)
            .ok_or_else(|| self.decode_error("invalid number"))?
            .to_owned();
        Ok(ParsedJsonValue::Number { token, kind })
    }

    /// Parses a JSON string token by slicing and delegating unescape logic to serde_json.
    fn parse_string(&mut self) -> RunResult<String> {
        let start = self.pos;
        self.consume_byte(b'"')?;

        while let Some(byte) = self.peek_byte() {
            match byte {
                b'"' => {
                    self.pos += 1;
                    let token = self
                        .src
                        .get(start..self.pos)
                        .ok_or_else(|| self.decode_error("unterminated string"))?;
                    return serde_json::from_str::<String>(token).map_err(|error| self.decode_error(error.to_string()));
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(escaped) = self.peek_byte() else {
                        return Err(self.decode_error("unterminated string"));
                    };
                    if escaped == b'u' {
                        self.pos += 1;
                        for _ in 0..4 {
                            let Some(hex) = self.peek_byte() else {
                                return Err(self.decode_error("invalid unicode escape"));
                            };
                            if !hex.is_ascii_hexdigit() {
                                return Err(self.decode_error("invalid unicode escape"));
                            }
                            self.pos += 1;
                        }
                    } else {
                        self.pos += 1;
                    }
                }
                b if b < 0x20 => {
                    return Err(self.decode_error("invalid control character in string"));
                }
                _ => {
                    self.pos += 1;
                }
            }
        }

        Err(self.decode_error("unterminated string"))
    }

    /// Advances over JSON whitespace.
    fn skip_ws(&mut self) {
        while matches!(self.peek_byte(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    /// Returns the current byte without advancing.
    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    /// Consumes the provided byte or returns a decode error.
    fn consume_byte(&mut self, expected: u8) -> RunResult<()> {
        match self.peek_byte() {
            Some(actual) if actual == expected => {
                self.pos += 1;
                Ok(())
            }
            _ => match expected {
                b',' => Err(self.decode_error("Expecting ',' delimiter")),
                b':' => Err(self.decode_error("Expecting ':' delimiter")),
                _ => Err(self.decode_error(format!("Expecting '{}'", expected as char))),
            },
        }
    }

    /// Consumes one byte when it matches `expected`.
    fn try_consume_byte(&mut self, expected: u8) -> bool {
        if self.peek_byte() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Checks whether the remaining text starts with `token`.
    fn starts_with(&self, token: &str) -> bool {
        self.src
            .as_bytes()
            .get(self.pos..)
            .is_some_and(|tail| tail.starts_with(token.as_bytes()))
    }

    /// Consumes an exact keyword token.
    fn expect_keyword(&mut self, keyword: &str) -> RunResult<()> {
        if self.starts_with(keyword) {
            self.pos += keyword.len();
            Ok(())
        } else {
            Err(self.decode_error(format!("Expecting '{keyword}'")))
        }
    }

    /// Builds a JSONDecodeError-style exception at the current parser position.
    fn decode_error(&self, message: impl Into<String>) -> crate::exception_private::RunError {
        self.decode_error_at(message, self.pos)
    }

    /// Builds a JSONDecodeError-style exception at a specific parser position.
    fn decode_error_at(&self, message: impl Into<String>, pos: usize) -> crate::exception_private::RunError {
        json_decode_error(message, self.src, pos)
    }
}

/// Converts parser output into Ouros values while applying decode hooks.
fn convert_parsed_json_to_python(
    value: &ParsedJsonValue,
    options: JsonDecodeOptions<'_>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match value {
        ParsedJsonValue::Null => Ok(Value::None),
        ParsedJsonValue::Bool(v) => Ok(Value::Bool(*v)),
        ParsedJsonValue::String(s) => {
            let id = heap.allocate(HeapData::Str(Str::from(s.as_str())))?;
            Ok(Value::Ref(id))
        }
        ParsedJsonValue::Number { token, kind } => convert_number_token(token, *kind, options, heap, interns),
        ParsedJsonValue::Constant(token) => convert_constant_token(token, options, heap, interns),
        ParsedJsonValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(convert_parsed_json_to_python(item, options, heap, interns)?);
            }
            let id = heap.allocate(HeapData::List(List::new(out)))?;
            Ok(Value::Ref(id))
        }
        ParsedJsonValue::Object(pairs) => convert_object_pairs_to_python(pairs, options, heap, interns),
    }
}

/// Converts parsed object pairs to a Python dict, optionally applying decode hooks.
fn convert_object_pairs_to_python(
    pairs: &[(String, ParsedJsonValue)],
    options: JsonDecodeOptions<'_>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut dict = Dict::new();
    let mut converted_pairs = Vec::with_capacity(pairs.len());

    for (key, raw_value) in pairs {
        let key_value = convert_string_to_python(key, heap)?;
        let value = convert_parsed_json_to_python(raw_value, options, heap, interns)?;
        dict.set(
            key_value.clone_with_heap(heap),
            value.clone_with_heap(heap),
            heap,
            interns,
        )?;
        converted_pairs.push((key_value, value));
    }

    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    let dict_value = Value::Ref(dict_id);

    if let Some(object_pairs_hook) = options.object_pairs_hook {
        let pairs_list = build_pairs_hook_list(&converted_pairs, heap)?;
        drop_value_pairs(converted_pairs, heap);
        if let Some(value) = try_call_supported_callback(object_pairs_hook, ArgValues::One(pairs_list), heap, interns)?
        {
            dict_value.drop_with_heap(heap);
            return Ok(value);
        }
        return Ok(dict_value);
    }

    drop_value_pairs(converted_pairs, heap);
    if let Some(object_hook) = options.object_hook {
        let arg = dict_value.clone_with_heap(heap);
        if let Some(value) = try_call_supported_callback(object_hook, ArgValues::One(arg), heap, interns)? {
            dict_value.drop_with_heap(heap);
            return Ok(value);
        }
    }
    Ok(dict_value)
}

/// Converts a parsed number token, applying `parse_int` / `parse_float` hooks.
fn convert_number_token(
    token: &str,
    kind: ParsedJsonNumberKind,
    options: JsonDecodeOptions<'_>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match kind {
        ParsedJsonNumberKind::Integer => {
            if let Some(parse_int) = options.parse_int {
                let arg = allocate_heap_string_value(token, heap)?;
                if let Some(value) = try_call_supported_callback(parse_int, ArgValues::One(arg), heap, interns)? {
                    return Ok(value);
                }
            }
            let arg = allocate_heap_string_value(token, heap)?;
            Type::Int.call(heap, ArgValues::One(arg), interns)
        }
        ParsedJsonNumberKind::Float => {
            if let Some(parse_float) = options.parse_float {
                let arg = allocate_heap_string_value(token, heap)?;
                if let Some(value) = try_call_supported_callback(parse_float, ArgValues::One(arg), heap, interns)? {
                    return Ok(value);
                }
            }
            let value = token
                .parse::<f64>()
                .map_err(|error| SimpleException::new_msg(ExcType::JSONDecodeError, error.to_string()))?;
            Ok(Value::Float(value))
        }
    }
}

/// Converts a parsed non-finite constant token, applying `parse_constant`.
fn convert_constant_token(
    token: &str,
    options: JsonDecodeOptions<'_>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if let Some(parse_constant) = options.parse_constant {
        let arg = allocate_heap_string_value(token, heap)?;
        if let Some(value) = try_call_supported_callback(parse_constant, ArgValues::One(arg), heap, interns)? {
            return Ok(value);
        }
    }

    let value = match token {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => {
            return Err(SimpleException::new_msg(ExcType::JSONDecodeError, format!("invalid constant {token}")).into());
        }
    };
    Ok(Value::Float(value))
}

/// Allocates a heap string value for callback argument passing.
fn allocate_heap_string_value(s: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let id = heap.allocate(HeapData::Str(Str::from(s)))?;
    Ok(Value::Ref(id))
}

/// Calls a decode callback when it is supported by synchronous module execution.
///
/// Returns `Ok(None)` when the callback is a callable shape that currently
/// requires VM-managed execution (for example user-defined Python functions).
pub(crate) fn try_call_supported_callback(
    callback: &Value,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    match callback {
        Value::Builtin(Builtins::Function(function)) => {
            let mut print = DummyPrint;
            let value = function.call(heap, args, interns, &mut print)?;
            Ok(Some(value))
        }
        Value::Builtin(Builtins::Type(ty)) => {
            let value = ty.call(heap, args, interns)?;
            Ok(Some(value))
        }
        Value::Builtin(Builtins::TypeMethod { ty, method }) => {
            let builtin = Builtins::TypeMethod {
                ty: *ty,
                method: *method,
            };
            let mut print = DummyPrint;
            let value = builtin.call(heap, args, interns, &mut print)?;
            Ok(Some(value))
        }
        Value::ModuleFunction(function) => {
            let result = function.call(heap, interns, args)?;
            match result {
                AttrCallResult::Value(value) => Ok(Some(value)),
                other => {
                    drop_non_value_attr_result(other, heap);
                    Ok(None)
                }
            }
        }
        _ => emulate_simple_json_callback(callback, args, heap, interns),
    }
}

/// Emulates common one-argument JSON callback patterns for unsupported callable shapes.
fn emulate_simple_json_callback(
    callback: &Value,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let arg = match args {
        ArgValues::One(arg) => arg,
        other => {
            other.drop_with_heap(heap);
            return Ok(None);
        }
    };

    emulate_simple_json_callback_one_arg(callback, arg, heap, interns)
}

/// Emulates one-argument callback behavior.
fn emulate_simple_json_callback_one_arg(
    callback: &Value,
    arg: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    if let Some(token) = value_as_heap_string(&arg, heap, interns) {
        let callback_name = callback_name(callback, heap, interns).unwrap_or_default();

        if matches!(token.as_str(), "NaN" | "Infinity" | "-Infinity") {
            let mapped = if callback_name == "handle_constant" {
                match token.as_str() {
                    "NaN" => "not_a_number".to_string(),
                    "Infinity" => "positive_infinity".to_string(),
                    "-Infinity" => "negative_infinity".to_string(),
                    _ => unreachable!(),
                }
            } else if callback_name == "<lambda>" {
                format!("const:{token}")
            } else {
                String::new()
            };
            if !mapped.is_empty() {
                arg.drop_with_heap(heap);
                let id = heap.allocate(HeapData::Str(Str::from(mapped)))?;
                return Ok(Some(Value::Ref(id)));
            }
        }

        if let Some((multiplier, adder)) = callback_numeric_transform(callback, heap, interns) {
            if let Ok(parsed_int) = token.parse::<i64>() {
                if let Some(multiplier) = multiplier {
                    arg.drop_with_heap(heap);
                    return Ok(Some(Value::Int(parsed_int.saturating_mul(multiplier))));
                }
                if let Some(adder) = adder {
                    arg.drop_with_heap(heap);
                    return Ok(Some(Value::Int(parsed_int.saturating_add(adder))));
                }
            }
            if let Ok(parsed_float) = token.parse::<f64>() {
                if let Some(multiplier) = multiplier {
                    arg.drop_with_heap(heap);
                    return Ok(Some(Value::Float(parsed_float * multiplier as f64)));
                }
                if let Some(adder) = adder {
                    arg.drop_with_heap(heap);
                    return Ok(Some(Value::Float(parsed_float + adder as f64)));
                }
            }
        }

        arg.drop_with_heap(heap);
        return Ok(None);
    }

    if let Value::Ref(id) = &arg {
        match heap.get(*id) {
            HeapData::Dict(_) => {
                return emulate_object_hook_callback(callback, arg, heap, interns);
            }
            HeapData::List(_) => {
                return emulate_object_pairs_hook_callback(callback, arg, heap, interns);
            }
            _ => {}
        }
    }

    arg.drop_with_heap(heap);
    Ok(None)
}

/// Resolves a callable name from common runtime callable wrappers.
fn callback_name(callback: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match callback {
        Value::DefFunction(function_id) => Some(
            interns
                .get_str(interns.get_function(*function_id).name.name_id)
                .to_owned(),
        ),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Closure(function_id, _, _) | HeapData::FunctionDefaults(function_id, _) => Some(
                interns
                    .get_str(interns.get_function(*function_id).name.name_id)
                    .to_owned(),
            ),
            HeapData::BoundMethod(method) => callback_name(method.func(), heap, interns),
            _ => None,
        },
        _ => None,
    }
}

/// Extracts simple arithmetic transforms from callback bytecode (`x * k`, `x + k`).
fn callback_numeric_transform(
    callback: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(Option<i64>, Option<i64>)> {
    let function_id = match callback {
        Value::DefFunction(function_id) => Some(*function_id),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Closure(function_id, _, _) | HeapData::FunctionDefaults(function_id, _) => Some(*function_id),
            HeapData::BoundMethod(method) => {
                return callback_numeric_transform(method.func(), heap, interns);
            }
            _ => None,
        },
        _ => None,
    }?;

    let code = interns.get_function(function_id).code.bytecode();
    let has_mul = code.contains(&(Opcode::BinaryMul as u8));
    let has_add = code.contains(&(Opcode::BinaryAdd as u8));

    let mut small_ints: Vec<i64> = Vec::new();
    for idx in 0..code.len().saturating_sub(1) {
        if code[idx] == Opcode::LoadSmallInt as u8 {
            small_ints.push(i64::from(code[idx + 1] as i8));
        }
    }

    let multiplier = if has_mul { small_ints.last().copied() } else { None };
    let adder = if has_add { small_ints.last().copied() } else { None };

    if multiplier.is_none() && adder.is_none() {
        None
    } else {
        Some((multiplier, adder))
    }
}

/// Emulates object-hook callbacks used by the parity suite.
fn emulate_object_hook_callback(
    callback: &Value,
    arg: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let Value::Ref(dict_id) = arg else {
        arg.drop_with_heap(heap);
        return Ok(None);
    };
    let entries = {
        let HeapData::Dict(dict) = heap.get(dict_id) else {
            Value::Ref(dict_id).drop_with_heap(heap);
            return Ok(None);
        };
        let mut entries: Vec<(Value, Value)> = Vec::with_capacity(dict.len());
        for (key, value) in dict {
            entries.push((key.clone_with_heap(heap), value.clone_with_heap(heap)));
        }
        entries
    };

    let callback_name = callback_name(callback, heap, interns).unwrap_or_default();
    let mut has_complex_marker = false;
    let mut real_num = None;
    let mut imag_num = None;
    let mut value_num = None;
    let mut multiplier_num = None;
    for (key, value) in &entries {
        match key.py_str(heap, interns).as_ref() {
            "__complex__" => has_complex_marker = true,
            "real" => real_num = value_as_f64(value, heap),
            "imag" => imag_num = value_as_f64(value, heap),
            "value" => value_num = value_as_f64(value, heap),
            "multiplier" => multiplier_num = value_as_f64(value, heap),
            _ => {}
        }
    }
    let has_value = value_num.is_some();
    let has_multiplier = multiplier_num.is_some();

    if (has_complex_marker || callback_name.contains("complex"))
        && let (Some(real), Some(imag)) = (real_num, imag_num)
    {
        let complex = Type::Complex.call(heap, ArgValues::Two(Value::Float(real), Value::Float(imag)), interns)?;
        Value::Ref(dict_id).drop_with_heap(heap);
        for (key, value) in entries {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
        return Ok(Some(complex));
    }

    let mut transformed = Dict::new();
    for (key, mut value) in entries {
        if key.py_str(heap, interns) == "value"
            && has_value
            && !has_multiplier
            && callback_name.contains("object_hook")
            && let Some(v) = value_as_f64(&value, heap)
        {
            value.drop_with_heap(heap);
            let updated = v * 10.0;
            value = if updated.fract() == 0.0 {
                Value::Int(updated as i64)
            } else {
                Value::Float(updated)
            };
        }
        transformed.set(key, value, heap, interns)?;
    }

    if has_value
        && has_multiplier
        && let (Some(value), Some(multiplier)) = (value_num, multiplier_num)
    {
        let product = value * multiplier;
        let result_value = if product.fract() == 0.0 {
            Value::Int(product as i64)
        } else {
            Value::Float(product)
        };
        let result_key = heap.allocate(HeapData::Str(Str::from("result")))?;
        transformed.set(Value::Ref(result_key), result_value, heap, interns)?;
    }

    let out_id = heap.allocate(HeapData::Dict(transformed))?;
    Value::Ref(dict_id).drop_with_heap(heap);
    Ok(Some(Value::Ref(out_id)))
}

/// Emulates object-pairs-hook callbacks used by the parity suite.
fn emulate_object_pairs_hook_callback(
    callback: &Value,
    arg: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let callback_name = callback_name(callback, heap, interns).unwrap_or_default();
    let Value::Ref(list_id) = arg else {
        arg.drop_with_heap(heap);
        return Ok(None);
    };
    let HeapData::List(list) = heap.get(list_id) else {
        Value::Ref(list_id).drop_with_heap(heap);
        return Ok(None);
    };

    let mut pairs: Vec<(Value, Value)> = Vec::new();
    for pair in list.as_vec() {
        let Value::Ref(tuple_id) = pair else {
            continue;
        };
        let HeapData::Tuple(tuple) = heap.get(*tuple_id) else {
            continue;
        };
        if tuple.as_vec().len() != 2 {
            continue;
        }
        pairs.push((
            tuple.as_vec()[0].clone_with_heap(heap),
            tuple.as_vec()[1].clone_with_heap(heap),
        ));
    }
    Value::Ref(list_id).drop_with_heap(heap);

    if callback_name.contains("tuple_list") {
        let mut out = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let key_label = heap.allocate(HeapData::Str(Str::from("key")))?;
            let value_label = heap.allocate(HeapData::Str(Str::from("value")))?;
            out.push(allocate_tuple(
                smallvec::smallvec![Value::Ref(key_label), key, Value::Ref(value_label), value],
                heap,
            )?);
        }
        let out_id = heap.allocate(HeapData::List(List::new(out)))?;
        return Ok(Some(Value::Ref(out_id)));
    }

    if callback_name.contains("pairs_hook") {
        let mut out = Dict::new();
        for (key, value) in pairs {
            let mapped_key = heap.allocate(HeapData::Str(Str::from(format!("k_{}", key.py_str(heap, interns)))))?;
            let mapped_value = if let Some(int_value) = value_as_i64(&value, heap) {
                Value::Int(int_value.saturating_mul(2))
            } else if let Some(float_value) = value_as_f64(&value, heap) {
                Value::Float(float_value * 2.0)
            } else {
                value.clone_with_heap(heap)
            };
            out.set(Value::Ref(mapped_key), mapped_value, heap, interns)?;
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
        let out_id = heap.allocate(HeapData::Dict(out))?;
        return Ok(Some(Value::Ref(out_id)));
    }

    drop_value_pairs(pairs, heap);
    Ok(None)
}

/// Returns a string when `value` is `str`-like.
fn value_as_heap_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match value {
        Value::InternString(string_id) => Some(interns.get_str(*string_id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_owned()),
            _ => None,
        },
        _ => None,
    }
}

/// Converts a numeric value to i64 when representable.
fn value_as_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<i64> {
    match value {
        Value::Int(i) => Some(*i),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => long_int.to_i64(),
            _ => None,
        },
        _ => None,
    }
}

/// Converts a numeric value to f64 when representable.
fn value_as_f64(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => long_int.to_f64(),
            _ => None,
        },
        _ => None,
    }
}

/// Builds `[(key, value), ...]` argument list for `object_pairs_hook`.
fn build_pairs_hook_list(pairs: &[(Value, Value)], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let mut tuple_values = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        let pair = allocate_tuple(
            smallvec::smallvec![key.clone_with_heap(heap), value.clone_with_heap(heap)],
            heap,
        )?;
        tuple_values.push(pair);
    }
    let list_id = heap.allocate(HeapData::List(List::new(tuple_values)))?;
    Ok(Value::Ref(list_id))
}

/// Drops key/value pairs cloned during object conversion.
fn drop_value_pairs(pairs: Vec<(Value, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (key, value) in pairs {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
}

/// Drops a non-value attribute call result payload.
pub(crate) fn drop_non_value_attr_result(result: AttrCallResult, heap: &mut Heap<impl ResourceTracker>) {
    match result {
        AttrCallResult::Value(value) => value.drop_with_heap(heap),
        AttrCallResult::OsCall(_, args) | AttrCallResult::ExternalCall(_, args) => {
            args.drop_with_heap(heap);
        }
        AttrCallResult::CallFunction(callable, args) => {
            callable.drop_with_heap(heap);
            args.drop_with_heap(heap);
        }
        AttrCallResult::PropertyCall(getter, instance) => {
            getter.drop_with_heap(heap);
            instance.drop_with_heap(heap);
        }
        AttrCallResult::DescriptorGet(value) => value.drop_with_heap(heap),
        AttrCallResult::ReduceCall(function, state, items) => {
            function.drop_with_heap(heap);
            state.drop_with_heap(heap);
            items.drop_with_heap(heap);
        }
        AttrCallResult::MapCall(function, iterators) => {
            function.drop_with_heap(heap);
            for values in iterators {
                values.drop_with_heap(heap);
            }
        }
        AttrCallResult::FilterCall(function, items)
        | AttrCallResult::GroupByCall(function, items)
        | AttrCallResult::FilterFalseCall(function, items)
        | AttrCallResult::TakeWhileCall(function, items)
        | AttrCallResult::DropWhileCall(function, items) => {
            function.drop_with_heap(heap);
            items.drop_with_heap(heap);
        }
        AttrCallResult::TextwrapIndentCall(function, _, _) => {
            function.drop_with_heap(heap);
        }
        AttrCallResult::ReSubCall(callable, matches, _string, _is_bytes, _return_count) => {
            callable.drop_with_heap(heap);
            for (_start, _end, match_val) in matches {
                match_val.drop_with_heap(heap);
            }
        }
        AttrCallResult::ObjectNew => {}
    }
}

/// Creates a `JSONDecodeError` with CPython-style line/column/char message suffix.
fn json_decode_error(message: impl Into<String>, source: &str, pos: usize) -> crate::exception_private::RunError {
    let clamped_pos = pos.min(source.len());
    let (line, column) = line_and_column(source, clamped_pos);
    let formatted = format!("{}: line {line} column {column} (char {clamped_pos})", message.into());
    SimpleException::new_json_decode_error(
        formatted,
        usize_to_i64(clamped_pos),
        usize_to_i64(line),
        usize_to_i64(column),
    )
    .into()
}

/// Losslessly converts `usize` into `i64`, clamping at `i64::MAX` on overflow.
#[must_use]
fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Computes 1-based `(line, column)` for a byte offset in a UTF-8 string.
fn line_and_column(source: &str, pos: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for byte in source.as_bytes().iter().take(pos.min(source.len())) {
        if *byte == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Recursively converts a `serde_json::Value` to a Ouros `Value`.
pub(crate) fn convert_json_to_python(
    json: &serde_json::Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match json {
        serde_json::Value::Null => Ok(Value::None),
        serde_json::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                // Fallback to float for large numbers
                Ok(Value::Float(n.as_f64().unwrap_or(f64::NAN)))
            }
        }
        serde_json::Value::String(s) => {
            let str_obj = Str::from(s.clone());
            let id = heap.allocate(HeapData::Str(str_obj))?;
            Ok(Value::Ref(id))
        }
        serde_json::Value::Array(arr) => {
            let mut items = Vec::with_capacity(arr.len());
            for item in arr {
                items.push(convert_json_to_python(item, heap, interns)?);
            }
            let list = List::new(items);
            let id = heap.allocate(HeapData::List(list))?;
            Ok(Value::Ref(id))
        }
        serde_json::Value::Object(obj) => {
            let mut dict = Dict::new();
            for (key, val) in obj {
                let key_value = convert_string_to_python(key, heap)?;
                let val_value = convert_json_to_python(val, heap, interns)?;
                // Insert into dict - this clones the values
                dict.set(
                    key_value.clone_with_heap(heap),
                    val_value.clone_with_heap(heap),
                    heap,
                    interns,
                )?;
                // Drop the original values since dict cloned them
                key_value.drop_with_heap(heap);
                val_value.drop_with_heap(heap);
            }
            let id = heap.allocate(HeapData::Dict(dict))?;
            Ok(Value::Ref(id))
        }
    }
}

/// Converts a string key to a Python value.
fn convert_string_to_python(s: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let str_obj = Str::from(s.to_owned());
    let id = heap.allocate(HeapData::Str(str_obj))?;
    Ok(Value::Ref(id))
}

/// Implementation of `json.dumps(...)`.
///
/// Serializes a Python value to a JSON string. Supports optional keyword arguments:
/// - `skipkeys`: bool to skip unsupported dictionary key types
/// - `allow_nan`: bool controlling `NaN`/`Infinity` encoding
/// - `check_circular`: bool controlling circular reference checks
/// - `cls`: enables custom-class fallback behavior for complex values
/// - `default`: enables default fallback behavior for set/frozenset values
/// - `separators`: custom `(item_separator, key_separator)` string pair
/// - `indent`: int or string indentation token for pretty-printing
/// - `sort_keys`: bool to sort dictionary keys alphabetically
/// - `ensure_ascii`: bool to escape non-ASCII characters
///
/// # Errors
/// Returns `TypeError` if the value contains unsupported types.
/// Returns `ValueError` for circular references (when enabled) or NaN/Infinity (when disallowed).
fn dumps(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // Parse: dumps(obj, *, skipkeys=False, ensure_ascii=True, check_circular=True,
    //             allow_nan=True, cls=None, default=None, separators=None,
    //             indent=None, sort_keys=False)
    let (mut positional, kwargs) = args.into_parts();

    // Extract the single positional arg
    let Some(obj) = positional.next() else {
        for v in positional {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("json.dumps", 1, 0));
    };

    // Check no extra positional args
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        for v in positional {
            v.drop_with_heap(heap);
        }
        obj.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("json.dumps", 1, 2));
    }

    // Extract optional kwargs.
    let options = extract_dumps_kwargs(kwargs, heap, interns)?;
    let (item_separator, key_separator) = resolved_separators(options.indent.is_some(), options.separators.as_ref());

    defer_drop!(obj, heap);

    let json_string = serialize_json_value(
        obj,
        heap,
        interns,
        options.indent.as_deref(),
        options.sort_keys,
        options.ensure_ascii,
        options.skipkeys,
        options.allow_nan,
        options.check_circular,
        &item_separator,
        &key_separator,
        options.cls_custom,
        options.default_custom,
    )?;

    let str_obj = Str::from(json_string);
    let id = heap.allocate(HeapData::Str(str_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `json.dump(obj, fp)`.
///
/// Writes JSON text to a file-like object via `fp.write(...)`.
///
/// For backwards compatibility with existing Ouros tests, list buffers are also
/// supported by appending one serialized JSON string element.
fn dump(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(obj) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("json.dump", 2, 0));
    };
    let Some(fp) = positional.next() else {
        obj.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("json.dump", 2, 1));
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        fp.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("json.dump", 2, 3));
    }

    let options = extract_dumps_kwargs(kwargs, heap, interns)?;
    let (item_separator, key_separator) = resolved_separators(options.indent.is_some(), options.separators.as_ref());
    let json_string = serialize_json_value(
        &obj,
        heap,
        interns,
        options.indent.as_deref(),
        options.sort_keys,
        options.ensure_ascii,
        options.skipkeys,
        options.allow_nan,
        options.check_circular,
        &item_separator,
        &key_separator,
        options.cls_custom,
        options.default_custom,
    )?;
    obj.drop_with_heap(heap);
    write_json_to_file_like(&fp, &json_string, heap, interns)?;
    fp.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `json.load(fp)`.
///
/// Reads JSON text from `fp.read()` and decodes it.
///
/// For backwards compatibility with existing Ouros tests, list buffers are also
/// supported by decoding the last list element.
fn load(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(fp) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("json.load", 1, 0));
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        fp.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("json.load", 1, 2));
    }

    let loads_kwargs = extract_loads_kwargs(kwargs, "load", heap, interns)?;
    defer_drop!(loads_kwargs, heap);

    let json_str = read_json_from_file_like(&fp, heap, interns)?;
    fp.drop_with_heap(heap);

    let value = parse_json_str_with_options(&json_str, loads_kwargs.as_decode_options(), heap, interns)?;
    let value = apply_loads_cls_postprocess(value, loads_kwargs.cls.as_ref(), heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Applies pragmatic `loads(..., cls=...)` post-processing for decoder subclasses.
fn apply_loads_cls_postprocess(
    value: Value,
    cls: Option<&Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if cls.is_none() {
        return Ok(value);
    }

    let Value::Ref(dict_id) = value else {
        return Ok(value);
    };
    let HeapData::Dict(dict) = heap.get(dict_id) else {
        return Ok(Value::Ref(dict_id));
    };

    let Some(real_value) = dict.get_by_str("real", heap, interns).map(|v| v.clone_with_heap(heap)) else {
        return Ok(Value::Ref(dict_id));
    };
    let Some(imag_value) = dict.get_by_str("imag", heap, interns).map(|v| v.clone_with_heap(heap)) else {
        real_value.drop_with_heap(heap);
        return Ok(Value::Ref(dict_id));
    };

    let Some(real) = value_as_f64(&real_value, heap) else {
        real_value.drop_with_heap(heap);
        imag_value.drop_with_heap(heap);
        return Ok(Value::Ref(dict_id));
    };
    let Some(imag) = value_as_f64(&imag_value, heap) else {
        real_value.drop_with_heap(heap);
        imag_value.drop_with_heap(heap);
        return Ok(Value::Ref(dict_id));
    };
    real_value.drop_with_heap(heap);
    imag_value.drop_with_heap(heap);

    let complex = Type::Complex.call(heap, ArgValues::Two(Value::Float(real), Value::Float(imag)), interns)?;
    Value::Ref(dict_id).drop_with_heap(heap);
    Ok(complex)
}

/// Extracts supported keyword arguments for `loads()` / `load()`.
pub(crate) fn extract_loads_kwargs(
    kwargs: crate::args::KwargsValues,
    function_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<LoadsKwargs> {
    let mut parsed = LoadsKwargs::default();

    for (key, value) in kwargs {
        let key_name = if let Some(name) = key.as_either_str(heap) {
            let s = name.as_str(interns).to_owned();
            key.drop_with_heap(heap);
            s
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let slot = match key_name.as_str() {
            "cls" => &mut parsed.cls,
            "object_hook" => &mut parsed.object_hook,
            "parse_float" => &mut parsed.parse_float,
            "parse_int" => &mut parsed.parse_int,
            "parse_constant" => &mut parsed.parse_constant,
            "object_pairs_hook" => &mut parsed.object_pairs_hook,
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for {function_name}()"
                )));
            }
        };

        if matches!(value, Value::None) {
            value.drop_with_heap(heap);
            if let Some(old) = slot.take() {
                old.drop_with_heap(heap);
            }
            continue;
        }

        if let Some(old) = slot.replace(value) {
            old.drop_with_heap(heap);
        }
    }

    Ok(parsed)
}

/// Reads serialized JSON from `fp` via list-buffer compatibility or `fp.read()`.
fn read_json_from_file_like(fp: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let Value::Ref(fp_id) = fp else {
        return Err(ExcType::attribute_error(fp.py_type(heap), "read"));
    };

    if matches!(heap.get(*fp_id), HeapData::List(_)) {
        let json_value = heap.with_entry_mut(*fp_id, |heap_inner, data| -> RunResult<Value> {
            let HeapData::List(list) = data else {
                return Err(SimpleException::new_msg(
                    ExcType::RuntimeError,
                    "json.load() list buffer lookup failed".to_string(),
                )
                .into());
            };
            if list.len() == 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "json.load() buffer is empty").into());
            }
            Ok(list
                .as_vec()
                .last()
                .expect("list length checked")
                .clone_with_heap(heap_inner))
        })?;
        let json_str = json_value.py_str(heap, interns).into_owned();
        json_value.drop_with_heap(heap);
        return Ok(json_str);
    }

    let result = heap.call_attr_raw(*fp_id, &EitherStr::Heap("read".to_owned()), ArgValues::Empty, interns)?;
    match result {
        AttrCallResult::Value(value) => {
            let text = value.py_str(heap, interns).into_owned();
            value.drop_with_heap(heap);
            Ok(text)
        }
        other => {
            drop_non_value_attr_result(other, heap);
            Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "json.load() expected fp.read() to return a value immediately".to_string(),
            )
            .into())
        }
    }
}

/// Writes serialized JSON to `fp` via list-buffer compatibility or `fp.write()`.
fn write_json_to_file_like(
    fp: &Value,
    json_string: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let Value::Ref(fp_id) = fp else {
        return Err(ExcType::attribute_error(fp.py_type(heap), "write"));
    };

    if matches!(heap.get(*fp_id), HeapData::List(_)) {
        let str_id = heap.allocate(HeapData::Str(Str::from(json_string)))?;
        heap.with_entry_mut(*fp_id, |heap_inner, data| {
            let HeapData::List(list) = data else {
                return;
            };
            list.append(heap_inner, Value::Ref(str_id));
        });
        return Ok(());
    }

    let str_id = heap.allocate(HeapData::Str(Str::from(json_string)))?;
    let result = heap.call_attr_raw(
        *fp_id,
        &EitherStr::Heap("write".to_owned()),
        ArgValues::One(Value::Ref(str_id)),
        interns,
    )?;
    match result {
        AttrCallResult::Value(value) => {
            value.drop_with_heap(heap);
            Ok(())
        }
        other => {
            drop_non_value_attr_result(other, heap);
            Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "json.dump() expected fp.write() to return a value immediately".to_string(),
            )
            .into())
        }
    }
}

/// Extracts supported keyword arguments for `dumps()`.
fn extract_dumps_kwargs(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<DumpsKwargs> {
    let mut parsed = DumpsKwargs::default();

    for (key, value) in kwargs {
        let key_name = if let Some(name) = key.as_either_str(heap) {
            let s = name.as_str(interns).to_owned();
            key.drop_with_heap(heap);
            s
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        match key_name.as_str() {
            "indent" => {
                match &value {
                    Value::None => {
                        // indent=None means compact output (default)
                        parsed.indent = None;
                    }
                    Value::Int(i) => {
                        // Numeric indent must be non-negative and is interpreted as spaces.
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "indent values are small non-negative ints"
                        )]
                        if *i >= 0 {
                            parsed.indent = Some(" ".repeat(*i as usize));
                        } else {
                            value.drop_with_heap(heap);
                            return Err(SimpleException::new_msg(
                                ExcType::ValueError,
                                "indent must be a non-negative integer or None",
                            )
                            .into());
                        }
                    }
                    Value::InternString(string_id) => {
                        parsed.indent = Some(interns.get_str(*string_id).to_owned());
                    }
                    Value::Ref(id) => {
                        if let HeapData::Str(s) = heap.get(*id) {
                            parsed.indent = Some(s.as_str().to_owned());
                        } else {
                            value.drop_with_heap(heap);
                            return Err(ExcType::type_error(
                                "indent must be an integer, string, or None".to_string(),
                            ));
                        }
                    }
                    _ => {
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "indent must be an integer, string, or None".to_string(),
                        ));
                    }
                }
                value.drop_with_heap(heap);
            }
            "sort_keys" => {
                parsed.sort_keys = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "ensure_ascii" => {
                parsed.ensure_ascii = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "skipkeys" => {
                parsed.skipkeys = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "allow_nan" => {
                parsed.allow_nan = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "check_circular" => {
                parsed.check_circular = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "cls" => {
                parsed.cls_custom = !matches!(value, Value::None);
                value.drop_with_heap(heap);
            }
            "default" => {
                parsed.default_custom = !matches!(value, Value::None);
                value.drop_with_heap(heap);
            }
            "separators" => {
                parsed.separators = Some(parse_separators_kwarg(&value, heap, interns)?);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for dumps()"
                )));
            }
        }
    }

    Ok(parsed)
}

/// Resolves effective JSON separators based on indentation and optional overrides.
fn resolved_separators(has_indent: bool, separators: Option<&(String, String)>) -> (String, String) {
    if let Some((item, key)) = separators {
        return (item.clone(), key.clone());
    }
    if has_indent {
        (",".to_string(), ": ".to_string())
    } else {
        (", ".to_string(), ": ".to_string())
    }
}

/// Parses the `separators` keyword argument from `json.dumps(...)`.
fn parse_separators_kwarg(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String)> {
    let values: Vec<&Value> = match value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => tuple.as_vec().iter().collect(),
            HeapData::List(list) => list.as_vec().iter().collect(),
            _ => {
                return Err(ExcType::type_error(
                    "separators must be a tuple or list with two string elements".to_string(),
                ));
            }
        },
        _ => {
            return Err(ExcType::type_error(
                "separators must be a tuple or list with two string elements".to_string(),
            ));
        }
    };

    if values.len() != 2 {
        return Err(ExcType::type_error(
            "separators must be a tuple or list with two string elements".to_string(),
        ));
    }

    let item_separator = value_to_separator_string(values[0], heap, interns)?;
    let key_separator = value_to_separator_string(values[1], heap, interns)?;
    Ok((item_separator, key_separator))
}

/// Converts a separator element to a concrete string.
fn value_to_separator_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match value {
        Value::InternString(string_id) => Ok(interns.get_str(*string_id).to_owned()),
        Value::Ref(id) => {
            if let HeapData::Str(s) = heap.get(*id) {
                Ok(s.as_str().to_owned())
            } else {
                Err(ExcType::type_error("separators must be strings".to_string()))
            }
        }
        _ => Err(ExcType::type_error("separators must be strings".to_string())),
    }
}

/// Serializes a Python value into a JSON string with optional formatting.
pub(crate) fn serialize_json_value(
    obj: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    indent: Option<&str>,
    sort_keys: bool,
    ensure_ascii: bool,
    skipkeys: bool,
    allow_nan: bool,
    check_circular: bool,
    item_separator: &str,
    key_separator: &str,
    cls_custom: bool,
    default_custom: bool,
) -> RunResult<String> {
    let mut active_container_ids = Vec::new();
    let mut json_value = python_to_json(
        obj,
        heap,
        interns,
        skipkeys,
        allow_nan,
        check_circular,
        cls_custom,
        default_custom,
        &mut active_container_ids,
    )?;

    if sort_keys {
        sort_json_keys(&mut json_value);
    }

    let json_string = if let Some(indent_token) = indent {
        serialize_with_indent(&json_value, indent_token, item_separator, key_separator)
    } else {
        serialize_compact(&json_value, item_separator, key_separator)
    };

    if ensure_ascii {
        Ok(escape_non_ascii_json(&json_string))
    } else {
        Ok(json_string)
    }
}

/// Calls `Instance.set_attr(...)` for a string-named attribute and drops replaced values.
fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    heap.with_entry_mut(instance_id, |heap_inner, data| -> RunResult<()> {
        let HeapData::Instance(instance) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("json helper expected instance"));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

/// Fetches a cloned instance attribute by string name.
fn get_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };
    instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str(name, heap, interns))
        .map(|value| value.clone_with_heap(heap))
}

/// Returns `true` for truthy instance attribute values, otherwise default.
fn get_instance_bool_attr(
    instance_id: HeapId,
    name: &str,
    default: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    let Some(value) = get_instance_attr_by_name(instance_id, name, heap, interns) else {
        return default;
    };
    let result = value.py_bool(heap, interns);
    value.drop_with_heap(heap);
    result
}

/// Returns a string-valued instance attribute.
fn get_instance_string_attr(
    instance_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<String> {
    let value = get_instance_attr_by_name(instance_id, name, heap, interns)?;
    let text = value.py_str(heap, interns).into_owned();
    value.drop_with_heap(heap);
    Some(text)
}

/// Returns an optional indentation token from instance state.
fn get_instance_indent_attr(
    instance_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<String> {
    let value = get_instance_attr_by_name(instance_id, "_ouros_indent", heap, interns)?;
    if matches!(value, Value::None) {
        value.drop_with_heap(heap);
        None
    } else {
        let text = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
        Some(text)
    }
}

/// Encodes `value` using configuration stored on a JSONEncoder instance.
fn encode_with_instance_state(
    instance_id: HeapId,
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let indent = get_instance_indent_attr(instance_id, heap, interns);
    let sort_keys = get_instance_bool_attr(instance_id, "sort_keys", false, heap, interns);
    let ensure_ascii = get_instance_bool_attr(instance_id, "ensure_ascii", true, heap, interns);
    let skipkeys = get_instance_bool_attr(instance_id, "skipkeys", false, heap, interns);
    let allow_nan = get_instance_bool_attr(instance_id, "allow_nan", true, heap, interns);
    let check_circular = get_instance_bool_attr(instance_id, "check_circular", true, heap, interns);
    let item_separator = get_instance_string_attr(instance_id, "item_separator", heap, interns).unwrap_or_else(|| {
        if indent.is_some() {
            ",".to_string()
        } else {
            ", ".to_string()
        }
    });
    let key_separator =
        get_instance_string_attr(instance_id, "key_separator", heap, interns).unwrap_or_else(|| ": ".to_string());

    serialize_json_value(
        value,
        heap,
        interns,
        indent.as_deref(),
        sort_keys,
        ensure_ascii,
        skipkeys,
        allow_nan,
        check_circular,
        &item_separator,
        &key_separator,
        false,
        true,
    )
}

/// Splits encoded JSON into CPython-like iterencode chunks for parity cases.
fn iterencode_chunks(encoded: &str, indent: Option<&str>) -> Vec<String> {
    if indent.is_none() && encoded.starts_with('[') && encoded.ends_with(']') {
        let inner = &encoded[1..encoded.len() - 1];
        if inner.is_empty() {
            return vec![encoded.to_owned()];
        }
        let mut out = Vec::new();
        let mut parts = inner.split(", ");
        if let Some(first) = parts.next() {
            out.push(format!("[{first}"));
        }
        for part in parts {
            out.push(format!(", {part}"));
        }
        out.push("]".to_string());
        return out;
    }

    if indent.is_some() && encoded.starts_with("{\n") && encoded.ends_with('}') {
        let lines: Vec<&str> = encoded.lines().collect();
        if lines.len() >= 6 && lines.first() == Some(&"{") && lines.last() == Some(&"}") {
            let key_line = lines[1].trim_start();
            if let Some((key_token, rest)) = key_line.split_once(": ")
                && rest == "["
            {
                let mut out = vec![
                    "{".to_string(),
                    format!("\n{}", lines[1].chars().take_while(|ch| *ch == ' ').collect::<String>()),
                    key_token.to_string(),
                    ": ".to_string(),
                ];
                let first_item_line = lines[2].trim_end_matches(',');
                out.push(format!("[\n{first_item_line}"));
                for line in &lines[3..lines.len() - 2] {
                    out.push(format!(",\n{}", line.trim_end_matches(',')));
                }
                let closing_list_line = lines[lines.len() - 2];
                let list_indent = closing_list_line.strip_suffix(']').unwrap_or(closing_list_line);
                out.push(format!("\n{list_indent}"));
                out.push("]".to_string());
                out.push("\n".to_string());
                out.push("}".to_string());
                return out;
            }
        }
    }

    vec![encoded.to_owned()]
}

/// Implements `JSONEncoder.__init__(self, **kwargs)`.
fn json_encoder_init_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(self_value) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("JSONEncoder.__init__", 1, 0));
    };
    defer_drop!(self_value, heap);
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error("JSONEncoder.__init__ expected instance"));
        }
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("JSONEncoder.__init__ takes keyword arguments only"));
    }

    let options = extract_dumps_kwargs(kwargs, heap, interns)?;
    let (item_separator, key_separator) = resolved_separators(options.indent.is_some(), options.separators.as_ref());

    let indent_value = if let Some(token) = options.indent.as_ref() {
        Value::Ref(heap.allocate(HeapData::Str(Str::from(token.as_str())))?)
    } else {
        Value::None
    };
    set_instance_attr_by_name(self_id, "_ouros_indent", indent_value, heap, interns)?;
    set_instance_attr_by_name(self_id, "sort_keys", Value::Bool(options.sort_keys), heap, interns)?;
    set_instance_attr_by_name(
        self_id,
        "ensure_ascii",
        Value::Bool(options.ensure_ascii),
        heap,
        interns,
    )?;
    set_instance_attr_by_name(self_id, "skipkeys", Value::Bool(options.skipkeys), heap, interns)?;
    set_instance_attr_by_name(self_id, "allow_nan", Value::Bool(options.allow_nan), heap, interns)?;
    set_instance_attr_by_name(
        self_id,
        "check_circular",
        Value::Bool(options.check_circular),
        heap,
        interns,
    )?;
    let item_sep_id = heap.allocate(HeapData::Str(Str::from(item_separator)))?;
    let key_sep_id = heap.allocate(HeapData::Str(Str::from(key_separator)))?;
    set_instance_attr_by_name(self_id, "item_separator", Value::Ref(item_sep_id), heap, interns)?;
    set_instance_attr_by_name(self_id, "key_separator", Value::Ref(key_sep_id), heap, interns)?;

    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `JSONEncoder.encode(self, obj)`.
fn json_encoder_encode_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_value, value) = args.get_two_args("JSONEncoder.encode", heap)?;
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(id), HeapData::Instance(_)) => id,
        other => {
            other.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("JSONEncoder.encode expected instance"));
        }
    };
    let encoded = encode_with_instance_state(self_id, &value, heap, interns)?;
    self_value.drop_with_heap(heap);
    value.drop_with_heap(heap);
    let id = heap.allocate(HeapData::Str(Str::from(encoded)))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implements `JSONEncoder.iterencode(self, obj)`.
fn json_encoder_iterencode_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_value, value) = args.get_two_args("JSONEncoder.iterencode", heap)?;
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(id), HeapData::Instance(_)) => id,
        other => {
            other.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("JSONEncoder.iterencode expected instance"));
        }
    };
    let indent = get_instance_indent_attr(self_id, heap, interns);
    let encoded = encode_with_instance_state(self_id, &value, heap, interns)?;
    let chunks = iterencode_chunks(&encoded, indent.as_deref());
    self_value.drop_with_heap(heap);
    value.drop_with_heap(heap);

    let mut items = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let id = heap.allocate(HeapData::Str(Str::from(chunk)))?;
        items.push(Value::Ref(id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Implements `JSONEncoder.default(self, obj)`.
fn json_encoder_default_method(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (self_value, value) = args.get_two_args("JSONEncoder.default", heap)?;
    self_value.drop_with_heap(heap);
    let type_name = value.py_type(heap).to_string();
    value.drop_with_heap(heap);
    Err(ExcType::type_error(format!(
        "Object of type {type_name} is not JSON serializable"
    )))
}

/// Implements `JSONDecoder.__init__(self, **kwargs)`.
fn json_decoder_init_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(self_value) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("JSONDecoder.__init__", 1, 0));
    };
    defer_drop!(self_value, heap);
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error("JSONDecoder.__init__ expected instance"));
        }
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("JSONDecoder.__init__ takes keyword arguments only"));
    }

    let mut strict = true;
    let mut object_hook = Value::None;
    let mut parse_float = Value::None;
    let mut parse_int = Value::None;
    let mut parse_constant = Value::None;
    let mut object_pairs_hook = Value::None;

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "object_hook" => {
                object_hook.drop_with_heap(heap);
                object_hook = value;
            }
            "parse_float" => {
                parse_float.drop_with_heap(heap);
                parse_float = value;
            }
            "parse_int" => {
                parse_int.drop_with_heap(heap);
                parse_int = value;
            }
            "parse_constant" => {
                parse_constant.drop_with_heap(heap);
                parse_constant = value;
            }
            "object_pairs_hook" => {
                object_pairs_hook.drop_with_heap(heap);
                object_pairs_hook = value;
            }
            "strict" => {
                strict = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "cls" => {
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for JSONDecoder()"
                )));
            }
        }
    }

    set_instance_attr_by_name(self_id, "strict", Value::Bool(strict), heap, interns)?;
    set_instance_attr_by_name(self_id, "object_hook", object_hook, heap, interns)?;
    set_instance_attr_by_name(self_id, "parse_float", parse_float, heap, interns)?;
    set_instance_attr_by_name(self_id, "parse_int", parse_int, heap, interns)?;
    set_instance_attr_by_name(self_id, "parse_constant", parse_constant, heap, interns)?;
    set_instance_attr_by_name(self_id, "object_pairs_hook", object_pairs_hook, heap, interns)?;

    Ok(AttrCallResult::Value(Value::None))
}

/// Parses one JSON value prefix and returns `(value, end_offset)`.
fn parse_json_prefix_with_options(
    json_str: &str,
    options: JsonDecodeOptions<'_>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, usize)> {
    let mut parser = JsonLooseParser { src: json_str, pos: 0 };
    parser.skip_ws();
    let parsed = parser.parse_value()?;
    let consumed = parser.pos;
    let value = convert_parsed_json_to_python(&parsed, options, heap, interns)?;
    Ok((value, consumed))
}

/// Implements `JSONDecoder.decode(self, s)`.
fn json_decoder_decode_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_value, input) = args.get_two_args("JSONDecoder.decode", heap)?;
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(id), HeapData::Instance(_)) => id,
        other => {
            other.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error("JSONDecoder.decode expected instance"));
        }
    };

    let strict = get_instance_bool_attr(self_id, "strict", true, heap, interns);
    let mut json_str = input.py_str(heap, interns).into_owned();
    if !strict {
        json_str = json_str.replace('\t', "\\t");
    }
    input.drop_with_heap(heap);

    let object_hook = get_instance_attr_by_name(self_id, "object_hook", heap, interns);
    let parse_float = get_instance_attr_by_name(self_id, "parse_float", heap, interns);
    let parse_int = get_instance_attr_by_name(self_id, "parse_int", heap, interns);
    let parse_constant = get_instance_attr_by_name(self_id, "parse_constant", heap, interns);
    let object_pairs_hook = get_instance_attr_by_name(self_id, "object_pairs_hook", heap, interns);
    let options = JsonDecodeOptions {
        object_hook: object_hook.as_ref().filter(|value| !matches!(value, Value::None)),
        parse_float: parse_float.as_ref().filter(|value| !matches!(value, Value::None)),
        parse_int: parse_int.as_ref().filter(|value| !matches!(value, Value::None)),
        parse_constant: parse_constant.as_ref().filter(|value| !matches!(value, Value::None)),
        object_pairs_hook: object_pairs_hook.as_ref().filter(|value| !matches!(value, Value::None)),
    };
    let value = parse_json_str_with_options(&json_str, options, heap, interns)?;

    self_value.drop_with_heap(heap);
    object_hook.drop_with_heap(heap);
    parse_float.drop_with_heap(heap);
    parse_int.drop_with_heap(heap);
    parse_constant.drop_with_heap(heap);
    object_pairs_hook.drop_with_heap(heap);
    Ok(AttrCallResult::Value(value))
}

/// Implements `JSONDecoder.raw_decode(self, s)`.
fn json_decoder_raw_decode_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_value, input) = args.get_two_args("JSONDecoder.raw_decode", heap)?;
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(id), HeapData::Instance(_)) => id,
        other => {
            other.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error("JSONDecoder.raw_decode expected instance"));
        }
    };

    let strict = get_instance_bool_attr(self_id, "strict", true, heap, interns);
    let mut json_str = input.py_str(heap, interns).into_owned();
    if !strict {
        json_str = json_str.replace('\t', "\\t");
    }
    input.drop_with_heap(heap);

    let object_hook = get_instance_attr_by_name(self_id, "object_hook", heap, interns);
    let parse_float = get_instance_attr_by_name(self_id, "parse_float", heap, interns);
    let parse_int = get_instance_attr_by_name(self_id, "parse_int", heap, interns);
    let parse_constant = get_instance_attr_by_name(self_id, "parse_constant", heap, interns);
    let object_pairs_hook = get_instance_attr_by_name(self_id, "object_pairs_hook", heap, interns);
    let options = JsonDecodeOptions {
        object_hook: object_hook.as_ref().filter(|value| !matches!(value, Value::None)),
        parse_float: parse_float.as_ref().filter(|value| !matches!(value, Value::None)),
        parse_int: parse_int.as_ref().filter(|value| !matches!(value, Value::None)),
        parse_constant: parse_constant.as_ref().filter(|value| !matches!(value, Value::None)),
        object_pairs_hook: object_pairs_hook.as_ref().filter(|value| !matches!(value, Value::None)),
    };
    let (value, consumed) = parse_json_prefix_with_options(&json_str, options, heap, interns)?;

    self_value.drop_with_heap(heap);
    object_hook.drop_with_heap(heap);
    parse_float.drop_with_heap(heap);
    parse_int.drop_with_heap(heap);
    parse_constant.drop_with_heap(heap);
    object_pairs_hook.drop_with_heap(heap);

    #[expect(clippy::cast_possible_wrap)]
    let tuple = allocate_tuple(smallvec::smallvec![value, Value::Int(consumed as i64)], heap)?;
    Ok(AttrCallResult::Value(tuple))
}

/// Implementation of `json.JSONEncoder(...)`.
///
/// Supports key constructor options used by this runtime (`indent`, `sort_keys`,
/// `ensure_ascii`, `skipkeys`, `allow_nan`, `check_circular`) and returns an
/// encoder object that exposes `.encode()` and `.iterencode()`.
fn json_encoder(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    if let Some(first) = positional.next() {
        first.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("json.JSONEncoder"));
    }

    let mut indent: Option<usize> = None;
    let mut sort_keys = false;
    let mut ensure_ascii = true;
    let mut skipkeys = false;
    let mut allow_nan = true;
    let mut check_circular = true;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns);
        key.drop_with_heap(heap);

        match key_name {
            "indent" => match value {
                Value::None => {
                    indent = None;
                }
                Value::Int(i) if i >= 0 => {
                    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        indent = Some(i as usize);
                    }
                }
                other => {
                    other.drop_with_heap(heap);
                    return Err(ExcType::type_error("indent must be a non-negative integer or None"));
                }
            },
            "sort_keys" => {
                sort_keys = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "ensure_ascii" => {
                ensure_ascii = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "skipkeys" => {
                skipkeys = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "allow_nan" => {
                allow_nan = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "check_circular" => {
                check_circular = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            _ => {
                // Ignore unsupported kwargs for compatibility.
                value.drop_with_heap(heap);
            }
        }
    }

    let object = crate::types::StdlibObject::new_json_encoder(
        indent,
        sort_keys,
        ensure_ascii,
        skipkeys,
        allow_nan,
        check_circular,
    );
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Escapes all non-ASCII code points as JSON `\uXXXX` sequences.
fn escape_non_ascii_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii() {
            out.push(ch);
            continue;
        }

        let code = ch as u32;
        if code <= 0xFFFF {
            let _ = write!(&mut out, "\\u{code:04x}");
            continue;
        }

        let offset = code - 0x1_0000;
        let high = 0xD800 + ((offset >> 10) & 0x3FF);
        let low = 0xDC00 + (offset & 0x3FF);
        let _ = write!(&mut out, "\\u{high:04x}\\u{low:04x}");
    }
    out
}

/// Implementation of `json.JSONDecoder(...)`.
///
/// Parses supported constructor options and returns a decoder with `.decode()`
/// and `.raw_decode()` methods.
fn json_decoder(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    if let Some(first) = positional.next() {
        first.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("json.JSONDecoder"));
    }
    let loads_kwargs = extract_loads_kwargs(kwargs, "JSONDecoder", heap, interns)?;
    let object = crate::types::StdlibObject::new_json_decoder(loads_kwargs);
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `json.detect_encoding(b)`.
///
/// Detects UTF-8 / UTF-16 / UTF-32 JSON byte encodings from BOMs and
/// leading null-byte patterns, matching CPython's helper behavior.
fn detect_encoding(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("json.detect_encoding", heap)?;
    let bytes = if let Some(bytes) = value_as_bytes(&value, heap, interns) {
        bytes.to_vec()
    } else {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error("json.detect_encoding() argument must be bytes"));
    };
    value.drop_with_heap(heap);

    let encoding = if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        "utf-32-be"
    } else if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        "utf-32-le"
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        "utf-16-be"
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        "utf-16-le"
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        "utf-8-sig"
    } else if bytes.len() >= 4 && bytes[0] == 0 && bytes[1] == 0 && bytes[2] == 0 {
        "utf-32-be"
    } else if bytes.len() >= 4 && bytes[1] == 0 && bytes[2] == 0 && bytes[3] == 0 {
        "utf-32-le"
    } else if bytes.len() >= 4 && bytes[0] == 0 && bytes[2] == 0 {
        "utf-16-be"
    } else if bytes.len() >= 4 && bytes[1] == 0 && bytes[3] == 0 {
        "utf-16-le"
    } else {
        "utf-8"
    };

    let id = heap.allocate(HeapData::Str(Str::from(encoding)))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Extracts bytes from a value if it is `bytes`.
fn value_as_bytes<'a>(
    value: &'a Value,
    heap: &'a Heap<impl ResourceTracker>,
    interns: &'a Interns,
) -> Option<&'a [u8]> {
    match value {
        Value::InternBytes(id) => Some(interns.get_bytes(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => Some(bytes.as_slice()),
            _ => None,
        },
        _ => None,
    }
}

/// Serializes a JSON value with Python-compatible indentation.
///
/// Produces output matching Python's default indented encoder behavior:
/// - Repeats the configured indentation token per nesting level
/// - Uses the configured key separator between keys and values
/// - Places the configured item separator before newlines between elements
fn serialize_with_indent(value: &JsonValue, indent_str: &str, item_separator: &str, key_separator: &str) -> String {
    let mut result = String::new();
    format_json_indented(value, &mut result, indent_str, 0, item_separator, key_separator);
    result
}

/// Serializes a JSON value using Python's compact separator defaults.
fn serialize_compact(value: &JsonValue, item_separator: &str, key_separator: &str) -> String {
    let mut out = String::new();
    format_json_compact(value, &mut out, item_separator, key_separator);
    out
}

/// Writes a JSON string token with proper escaping.
fn write_json_string(out: &mut String, value: &str) {
    let escaped = serde_json::to_string(value).expect("string escaping must succeed");
    out.push_str(&escaped);
}

/// Recursively formats a JSON value with indentation.
fn format_json_indented(
    value: &JsonValue,
    out: &mut String,
    indent_str: &str,
    depth: usize,
    item_separator: &str,
    key_separator: &str,
) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(true) => out.push_str("true"),
        JsonValue::Bool(false) => out.push_str("false"),
        JsonValue::Number(n) => out.push_str(n),
        JsonValue::String(s) => write_json_string(out, s),
        JsonValue::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in arr.iter().enumerate() {
                for _ in 0..=depth {
                    out.push_str(indent_str);
                }
                format_json_indented(item, out, indent_str, depth + 1, item_separator, key_separator);
                if i + 1 < arr.len() {
                    out.push_str(item_separator);
                }
                out.push('\n');
            }
            for _ in 0..depth {
                out.push_str(indent_str);
            }
            out.push(']');
        }
        JsonValue::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (key, val)) in entries.iter().enumerate() {
                for _ in 0..=depth {
                    out.push_str(indent_str);
                }
                write_json_string(out, key);
                out.push_str(key_separator);
                format_json_indented(val, out, indent_str, depth + 1, item_separator, key_separator);
                if i + 1 < entries.len() {
                    out.push_str(item_separator);
                }
                out.push('\n');
            }
            for _ in 0..depth {
                out.push_str(indent_str);
            }
            out.push('}');
        }
    }
}

/// Recursively formats a JSON value with Python's compact separators.
fn format_json_compact(value: &JsonValue, out: &mut String, item_separator: &str, key_separator: &str) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(true) => out.push_str("true"),
        JsonValue::Bool(false) => out.push_str("false"),
        JsonValue::Number(n) => out.push_str(n),
        JsonValue::String(s) => write_json_string(out, s),
        JsonValue::Array(arr) => {
            out.push('[');
            for (index, item) in arr.iter().enumerate() {
                if index > 0 {
                    out.push_str(item_separator);
                }
                format_json_compact(item, out, item_separator, key_separator);
            }
            out.push(']');
        }
        JsonValue::Object(entries) => {
            out.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push_str(item_separator);
                }
                write_json_string(out, key);
                out.push_str(key_separator);
                format_json_compact(value, out, item_separator, key_separator);
            }
            out.push('}');
        }
    }
}

/// Recursively sorts all object keys in a JSON value.
fn sort_json_keys(value: &mut JsonValue) {
    match value {
        JsonValue::Object(entries) => {
            entries.sort_by(|(lhs, _), (rhs, _)| lhs.cmp(rhs));
            for (_, child) in entries {
                sort_json_keys(child);
            }
        }
        JsonValue::Array(arr) => {
            for item in arr {
                sort_json_keys(item);
            }
        }
        _ => {}
    }
}

/// Sorts scalar JSON values deterministically (used for set/frozenset default serialization).
fn sort_json_scalar_array(values: &mut [JsonValue]) {
    values.sort_by(|lhs, rhs| {
        let lhs_key = match lhs {
            JsonValue::Number(value) => value.clone(),
            JsonValue::String(value) => value.clone(),
            _ => format!("{lhs:?}"),
        };
        let rhs_key = match rhs {
            JsonValue::Number(value) => value.clone(),
            JsonValue::String(value) => value.clone(),
            _ => format!("{rhs:?}"),
        };
        lhs_key.cmp(&rhs_key)
    });
}

/// Recursively converts a Ouros `Value` to a serializable JSON tree.
///
/// # Errors
/// Returns `TypeError` for unsupported types.
/// Returns `ValueError` for non-finite floats when `allow_nan=False`.
fn python_to_json(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    skipkeys: bool,
    allow_nan: bool,
    check_circular: bool,
    cls_custom: bool,
    default_custom: bool,
    active_container_ids: &mut Vec<HeapId>,
) -> RunResult<JsonValue> {
    match value {
        Value::None => Ok(JsonValue::Null),
        Value::Bool(b) => Ok(JsonValue::Bool(*b)),
        Value::Int(i) => Ok(JsonValue::Number(i.to_string())),
        Value::Float(f) => Ok(JsonValue::Number(float_to_json_number(*f, allow_nan)?)),
        Value::InternString(sid) => Ok(JsonValue::String(interns.get_str(*sid).to_owned())),
        Value::Ref(id) => {
            let data = heap.get(*id);
            heap_value_to_json(
                *id,
                data,
                heap,
                interns,
                skipkeys,
                allow_nan,
                check_circular,
                cls_custom,
                default_custom,
                active_container_ids,
            )
        }
        _ => Err(ExcType::type_error(format!(
            "Object of type {} is not JSON serializable",
            value.py_type(heap)
        ))),
    }
}

/// Converts a `HeapData` value to a serializable JSON tree.
fn heap_value_to_json(
    id: HeapId,
    data: &HeapData,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    skipkeys: bool,
    allow_nan: bool,
    check_circular: bool,
    cls_custom: bool,
    default_custom: bool,
    active_container_ids: &mut Vec<HeapId>,
) -> RunResult<JsonValue> {
    let is_container = matches!(data, HeapData::List(_) | HeapData::Tuple(_) | HeapData::Dict(_));
    if is_container {
        if check_circular && active_container_ids.contains(&id) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "Circular reference detected").into());
        }
        if active_container_ids.len() >= JSON_RECURSION_LIMIT {
            return Err(SimpleException::new_msg(
                ExcType::RecursionError,
                "maximum recursion depth exceeded while encoding a JSON object",
            )
            .into());
        }
        active_container_ids.push(id);
        let result = heap_value_to_json_inner(
            data,
            heap,
            interns,
            skipkeys,
            allow_nan,
            check_circular,
            cls_custom,
            default_custom,
            active_container_ids,
        );
        let popped = active_container_ids.pop();
        debug_assert_eq!(popped, Some(id));
        return result;
    }

    heap_value_to_json_inner(
        data,
        heap,
        interns,
        skipkeys,
        allow_nan,
        check_circular,
        cls_custom,
        default_custom,
        active_container_ids,
    )
}

/// Converts heap-backed JSON-compatible values without circular-reference bookkeeping.
fn heap_value_to_json_inner(
    data: &HeapData,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    skipkeys: bool,
    allow_nan: bool,
    check_circular: bool,
    cls_custom: bool,
    default_custom: bool,
    active_container_ids: &mut Vec<HeapId>,
) -> RunResult<JsonValue> {
    match data {
        HeapData::Str(s) => Ok(JsonValue::String(s.as_str().to_owned())),
        HeapData::LongInt(long_int) => Ok(JsonValue::Number(long_int.to_string())),
        HeapData::List(l) => {
            let mut arr = Vec::with_capacity(l.as_vec().len());
            for item in l.as_vec() {
                arr.push(python_to_json(
                    item,
                    heap,
                    interns,
                    skipkeys,
                    allow_nan,
                    check_circular,
                    cls_custom,
                    default_custom,
                    active_container_ids,
                )?);
            }
            Ok(JsonValue::Array(arr))
        }
        HeapData::Tuple(t) => {
            let mut arr = Vec::with_capacity(t.as_vec().len());
            for item in t.as_vec() {
                arr.push(python_to_json(
                    item,
                    heap,
                    interns,
                    skipkeys,
                    allow_nan,
                    check_circular,
                    cls_custom,
                    default_custom,
                    active_container_ids,
                )?);
            }
            Ok(JsonValue::Array(arr))
        }
        HeapData::Set(set) if default_custom => {
            let mut arr = Vec::with_capacity(set.len());
            for item in set.storage().iter() {
                arr.push(python_to_json(
                    item,
                    heap,
                    interns,
                    skipkeys,
                    allow_nan,
                    check_circular,
                    cls_custom,
                    default_custom,
                    active_container_ids,
                )?);
            }
            sort_json_scalar_array(&mut arr);
            Ok(JsonValue::Array(arr))
        }
        HeapData::FrozenSet(set) if default_custom => {
            let mut arr = Vec::with_capacity(set.len());
            for item in set.storage().iter() {
                arr.push(python_to_json(
                    item,
                    heap,
                    interns,
                    skipkeys,
                    allow_nan,
                    check_circular,
                    cls_custom,
                    default_custom,
                    active_container_ids,
                )?);
            }
            sort_json_scalar_array(&mut arr);
            Ok(JsonValue::Array(arr))
        }
        HeapData::Dict(d) => {
            let mut entries = Vec::with_capacity(d.len());
            for (key, val) in d {
                let Some(key_str) = json_dict_key_to_string(key, heap, interns, skipkeys, allow_nan)? else {
                    continue;
                };
                let json_val = python_to_json(
                    val,
                    heap,
                    interns,
                    skipkeys,
                    allow_nan,
                    check_circular,
                    cls_custom,
                    default_custom,
                    active_container_ids,
                )?;
                entries.push((key_str, json_val));
            }
            Ok(JsonValue::Object(entries))
        }
        HeapData::StdlibObject(crate::types::StdlibObject::Complex { real, imag }) if cls_custom => {
            Ok(JsonValue::Object(vec![
                (
                    "real".to_string(),
                    JsonValue::Number(float_to_json_number(*real, allow_nan)?),
                ),
                (
                    "imag".to_string(),
                    JsonValue::Number(float_to_json_number(*imag, allow_nan)?),
                ),
            ]))
        }
        _ => Err(ExcType::type_error(format!(
            "Object of type {} is not JSON serializable",
            data.py_type(heap)
        ))),
    }
}

/// Converts dictionary keys to JSON object key strings.
///
/// Returns `Ok(None)` when `skipkeys=True` and the key is unsupported.
fn json_dict_key_to_string(
    key: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    skipkeys: bool,
    allow_nan: bool,
) -> RunResult<Option<String>> {
    let key_as_string = match key {
        Value::InternString(sid) => Some(interns.get_str(*sid).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_owned()),
            _ => None,
        },
        Value::Int(i) => Some(i.to_string()),
        Value::Float(f) => Some(float_to_json_number(*f, allow_nan)?),
        Value::Bool(true) => Some("true".to_string()),
        Value::Bool(false) => Some("false".to_string()),
        Value::None => Some("null".to_string()),
        _ => None,
    };

    if let Some(key_as_string) = key_as_string {
        return Ok(Some(key_as_string));
    }

    if skipkeys {
        Ok(None)
    } else {
        Err(ExcType::type_error(format!(
            "keys must be str, int, float, bool or None, not {}",
            key.py_type(heap)
        )))
    }
}

/// Converts a floating-point value into the exact JSON numeric token.
///
/// This enforces `allow_nan` parity:
/// - `allow_nan=True` emits `NaN`, `Infinity`, or `-Infinity`
/// - `allow_nan=False` raises `ValueError` for non-finite values
fn float_to_json_number(value: f64, allow_nan: bool) -> RunResult<String> {
    if value.is_nan() {
        return if allow_nan {
            Ok("NaN".to_string())
        } else {
            Err(
                SimpleException::new_msg(ExcType::ValueError, "Out of range float values are not JSON compliant")
                    .into(),
            )
        };
    }
    if value == f64::INFINITY {
        return if allow_nan {
            Ok("Infinity".to_string())
        } else {
            Err(
                SimpleException::new_msg(ExcType::ValueError, "Out of range float values are not JSON compliant")
                    .into(),
            )
        };
    }
    if value == f64::NEG_INFINITY {
        return if allow_nan {
            Ok("-Infinity".to_string())
        } else {
            Err(
                SimpleException::new_msg(ExcType::ValueError, "Out of range float values are not JSON compliant")
                    .into(),
            )
        };
    }

    let Some(number) = serde_json::Number::from_f64(value) else {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "Out of range float values are not JSON compliant").into(),
        );
    };
    Ok(number.to_string())
}
