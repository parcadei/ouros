//! Implementation of the `csv` module.
//!
//! This module provides a sandbox-safe subset of CPython's `csv` API with
//! object-based readers and writers:
//! - `reader(iterable, dialect='excel', **fmtparams)` -> iterator object
//! - `writer(fileobj, dialect='excel', **fmtparams)` -> writer object
//! - `DictReader(...)` / `DictWriter(...)`
//! - Dialect registry helpers and built-in dialect objects
//! - `Sniffer()` constructor with `.sniff()` / `.has_header()`
//! - `field_size_limit([limit])`
//! - Quoting constants and `Error` / `Dialect` classes
//!
//! The implementation intentionally targets behavior used by Ouros's parity
//! suite while remaining fully sandboxed (no filesystem access).

use std::sync::{Mutex, OnceLock};

use crate::{
    args::{ArgPosIter, ArgValues},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, List, Module, OurosIter, PyTrait, StdlibObject, Str, Type, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// CSV module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum CsvFunctions {
    Reader,
    Writer,
    #[strum(serialize = "field_size_limit")]
    FieldSizeLimit,
    #[strum(serialize = "get_dialect")]
    GetDialect,
    #[strum(serialize = "list_dialects")]
    ListDialects,
    #[strum(serialize = "register_dialect")]
    RegisterDialect,
    #[strum(serialize = "unregister_dialect")]
    UnregisterDialect,
    #[strum(serialize = "DictReader")]
    DictReader,
    #[strum(serialize = "DictWriter")]
    DictWriter,
    #[strum(serialize = "Sniffer")]
    Sniffer,
}

/// Default maximum field size, matching CPython's csv module.
const DEFAULT_FIELD_SIZE_LIMIT: i64 = 131_072;

/// `csv.QUOTE_MINIMAL`.
pub(crate) const QUOTE_MINIMAL: i64 = 0;
/// `csv.QUOTE_ALL`.
pub(crate) const QUOTE_ALL: i64 = 1;
/// `csv.QUOTE_NONNUMERIC`.
pub(crate) const QUOTE_NONNUMERIC: i64 = 2;
/// `csv.QUOTE_NONE`.
pub(crate) const QUOTE_NONE: i64 = 3;
/// `csv.QUOTE_STRINGS`.
pub(crate) const QUOTE_STRINGS: i64 = 4;
/// `csv.QUOTE_NOTNULL`.
pub(crate) const QUOTE_NOTNULL: i64 = 5;

/// Parsed CSV field with quote metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CsvParsedField {
    /// Parsed field text (without surrounding quotes).
    pub text: String,
    /// Whether the source token was a quoted field.
    pub quoted: bool,
}

/// Dialect options used by readers and writers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CsvDialect {
    /// Field delimiter.
    pub delimiter: char,
    /// Quote character.
    pub quotechar: char,
    /// Escape character used when `QUOTE_NONE` or `doublequote=False`.
    pub escapechar: Option<char>,
    /// Whether doubled quote characters escape quotes inside quoted fields.
    pub doublequote: bool,
    /// Whether spaces after delimiters are skipped.
    pub skipinitialspace: bool,
    /// Row terminator used by writer objects.
    pub lineterminator: String,
    /// Quoting mode.
    pub quoting: i64,
}

/// Dialect registry that preserves insertion order like CPython.
#[derive(Debug, Clone)]
struct CsvDialectRegistry {
    entries: Vec<(String, CsvDialect)>,
}

impl CsvDialectRegistry {
    /// Creates a registry with CPython-compatible default dialect names.
    fn new() -> Self {
        Self {
            entries: vec![
                ("excel".to_owned(), excel_dialect()),
                ("excel-tab".to_owned(), excel_tab_dialect()),
                ("unix".to_owned(), unix_dialect()),
            ],
        }
    }

    /// Gets a dialect by name.
    fn get(&self, name: &str) -> Option<CsvDialect> {
        self.entries
            .iter()
            .find_map(|(dialect_name, dialect)| (dialect_name == name).then_some(dialect.clone()))
    }

    /// Registers or updates a dialect.
    fn insert(&mut self, name: String, dialect: CsvDialect) {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(dialect_name, _)| *dialect_name == name) {
            *existing = dialect;
        } else {
            self.entries.push((name, dialect));
        }
    }

    /// Removes a dialect by name.
    fn remove(&mut self, name: &str) -> bool {
        if let Some(index) = self.entries.iter().position(|(dialect_name, _)| dialect_name == name) {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    /// Returns the dialect names in insertion order.
    fn list_names(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }
}

/// Registry of named CSV dialects.
static CSV_DIALECTS: OnceLock<Mutex<CsvDialectRegistry>> = OnceLock::new();
/// Configurable CSV field size limit.
static CSV_FIELD_SIZE_LIMIT: OnceLock<Mutex<i64>> = OnceLock::new();

/// Creates the `csv` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Csv);

    module.set_attr(
        StaticStrings::CsvReader,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::Reader)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvWriter,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::Writer)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvFieldSizeLimit,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::FieldSizeLimit)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvRegisterDialect,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::RegisterDialect)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvGetDialect,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::GetDialect)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvListDialects,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::ListDialects)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvUnregisterDialect,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::UnregisterDialect)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvDictReader,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::DictReader)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::CsvDictWriter,
        Value::ModuleFunction(ModuleFunctions::Csv(CsvFunctions::DictWriter)),
        heap,
        interns,
    );
    // In Ouros, `csv.Sniffer` is a pre-created object rather than a constructor,
    // so `csv.Sniffer.sniff(...)` works without needing `csv.Sniffer().sniff(...)`.
    let sniffer_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_csv_sniffer()))?;
    module.set_attr(StaticStrings::CsvSniffer, Value::Ref(sniffer_id), heap, interns);

    // csv.Error - class named _csv.Error.
    let error_class_id = create_csv_class(heap, interns, "_csv.Error", &[Type::Exception(ExcType::Exception)])?;
    let error_class = Value::Ref(error_class_id);
    module.set_attr_str("Error", error_class.clone_with_heap(heap), heap, interns)?;
    module.set_attr(StaticStrings::CsvError, error_class, heap, interns);

    // csv.Dialect - base class used for user-defined dialect subclasses.
    let dialect_class_id = create_csv_class(heap, interns, "Dialect", &[Type::Object])?;
    module.set_attr_str("Dialect", Value::Ref(dialect_class_id), heap, interns)?;

    // Quoting constants.
    module.set_attr(StaticStrings::CsvQuoteMinimal, Value::Int(QUOTE_MINIMAL), heap, interns);
    module.set_attr(StaticStrings::CsvQuoteAll, Value::Int(QUOTE_ALL), heap, interns);
    module.set_attr(
        StaticStrings::CsvQuoteNonnumeric,
        Value::Int(QUOTE_NONNUMERIC),
        heap,
        interns,
    );
    module.set_attr(StaticStrings::CsvQuoteNone, Value::Int(QUOTE_NONE), heap, interns);
    module.set_attr(StaticStrings::CsvQuoteStrings, Value::Int(QUOTE_STRINGS), heap, interns);
    module.set_attr(StaticStrings::CsvQuoteNotnull, Value::Int(QUOTE_NOTNULL), heap, interns);

    // Built-in dialect classes (excel, excel_tab, unix_dialect).
    // These are classes that inherit from Dialect and have class attributes
    // set to their respective dialect configuration.
    let excel_class_id = create_dialect_class(heap, interns, "excel", dialect_class_id, excel_dialect())?;
    module.set_attr(StaticStrings::CsvExcel, Value::Ref(excel_class_id), heap, interns);

    let excel_tab_class_id = create_dialect_class(heap, interns, "excel_tab", dialect_class_id, excel_tab_dialect())?;
    module.set_attr(
        StaticStrings::CsvExcelTab,
        Value::Ref(excel_tab_class_id),
        heap,
        interns,
    );

    let unix_class_id = create_dialect_class(heap, interns, "unix_dialect", dialect_class_id, unix_dialect())?;
    module.set_attr(StaticStrings::CsvUnixDialect, Value::Ref(unix_class_id), heap, interns);

    heap.allocate(HeapData::Module(module))
}

/// Dispatches csv module function calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: CsvFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        CsvFunctions::Reader => reader(heap, interns, args),
        CsvFunctions::Writer => writer(heap, interns, args),
        CsvFunctions::FieldSizeLimit => field_size_limit(heap, args),
        CsvFunctions::GetDialect => get_dialect(heap, interns, args),
        CsvFunctions::ListDialects => list_dialects(heap),
        CsvFunctions::RegisterDialect => register_dialect(heap, interns, args),
        CsvFunctions::UnregisterDialect => unregister_dialect(heap, interns, args),
        CsvFunctions::DictReader => dict_reader(heap, interns, args),
        CsvFunctions::DictWriter => dict_writer(heap, interns, args),
        CsvFunctions::Sniffer => sniffer(heap, args),
    }
}

/// Implements `csv.reader(iterable, dialect='excel', **fmtparams)`.
fn reader(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let iterable = next_positional(&mut positional, "csv.reader", 1)?;
    let mut dialect_arg = next_optional_positional(&mut positional);
    if let Some(extra) = next_optional_positional(&mut positional) {
        iterable.drop_with_heap(heap);
        dialect_arg.drop_with_heap(heap);
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("csv.reader", 2, 3));
    }
    positional.drop_with_heap(heap);

    let mut kw = kwargs_to_pairs(kwargs, heap, interns, "csv.reader")?;
    let dialect_from_kw = take_kwarg(&mut kw, "dialect");
    if dialect_arg.is_some() && dialect_from_kw.is_some() {
        iterable.drop_with_heap(heap);
        dialect_arg.drop_with_heap(heap);
        if let Some(value) = dialect_from_kw {
            value.drop_with_heap(heap);
        }
        drop_kwarg_pairs(kw, heap);
        return Err(ExcType::type_error(
            "csv.reader() got multiple values for argument 'dialect'".to_string(),
        ));
    }
    if let Some(value) = dialect_from_kw {
        dialect_arg = Some(value);
    }

    let mut dialect = match dialect_arg {
        Some(value) => {
            let parsed = parse_dialect_value(&value, heap, interns)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => lookup_dialect("excel")?,
    };
    apply_fmtparams(&mut dialect, &mut kw, heap, interns, "csv.reader")?;
    validate_no_kwargs(&mut kw, heap, "csv.reader")?;

    let rows = collect_parsed_rows_from_iterable(iterable, &dialect, heap, interns, "csv.reader")?;
    let reader_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_csv_reader(rows, dialect)))?;
    Ok(AttrCallResult::Value(Value::Ref(reader_id)))
}

/// Implements `csv.writer(fileobj, dialect='excel', **fmtparams)`.
fn writer(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let file_obj = next_positional(&mut positional, "csv.writer", 1)?;
    let mut dialect_arg = next_optional_positional(&mut positional);
    if let Some(extra) = next_optional_positional(&mut positional) {
        file_obj.drop_with_heap(heap);
        dialect_arg.drop_with_heap(heap);
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("csv.writer", 2, 3));
    }
    positional.drop_with_heap(heap);

    if !matches!(file_obj, Value::Ref(_)) {
        file_obj.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "csv.writer() argument 1 must be a file-like object",
        ));
    }

    let mut kw = kwargs_to_pairs(kwargs, heap, interns, "csv.writer")?;
    let dialect_from_kw = take_kwarg(&mut kw, "dialect");
    if dialect_arg.is_some() && dialect_from_kw.is_some() {
        file_obj.drop_with_heap(heap);
        dialect_arg.drop_with_heap(heap);
        if let Some(value) = dialect_from_kw {
            value.drop_with_heap(heap);
        }
        drop_kwarg_pairs(kw, heap);
        return Err(ExcType::type_error(
            "csv.writer() got multiple values for argument 'dialect'".to_string(),
        ));
    }
    if let Some(value) = dialect_from_kw {
        dialect_arg = Some(value);
    }

    let mut dialect = match dialect_arg {
        Some(value) => {
            let parsed = parse_dialect_value(&value, heap, interns)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => lookup_dialect("excel")?,
    };
    apply_fmtparams(&mut dialect, &mut kw, heap, interns, "csv.writer")?;
    validate_no_kwargs(&mut kw, heap, "csv.writer")?;

    let writer_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_csv_writer(file_obj, dialect)))?;
    Ok(AttrCallResult::Value(Value::Ref(writer_id)))
}

/// Implements `csv.DictReader(...)`.
fn dict_reader(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let iterable = next_positional(&mut positional, "csv.DictReader", 1)?;
    let fieldnames_pos = next_optional_positional(&mut positional);
    if let Some(extra) = next_optional_positional(&mut positional) {
        iterable.drop_with_heap(heap);
        fieldnames_pos.drop_with_heap(heap);
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("csv.DictReader", 2, 3));
    }
    positional.drop_with_heap(heap);

    let mut kw = kwargs_to_pairs(kwargs, heap, interns, "csv.DictReader")?;
    let fieldnames_kw = take_kwarg(&mut kw, "fieldnames");
    if fieldnames_pos.is_some() && fieldnames_kw.is_some() {
        iterable.drop_with_heap(heap);
        fieldnames_pos.drop_with_heap(heap);
        if let Some(value) = fieldnames_kw {
            value.drop_with_heap(heap);
        }
        drop_kwarg_pairs(kw, heap);
        return Err(ExcType::type_error(
            "csv.DictReader() got multiple values for argument 'fieldnames'".to_string(),
        ));
    }
    let fieldnames_value = fieldnames_kw.or(fieldnames_pos);

    let restkey = if let Some(value) = take_kwarg(&mut kw, "restkey") {
        let key = value_to_optional_string(&value, heap, interns, "csv.DictReader restkey")?;
        value.drop_with_heap(heap);
        key
    } else {
        None
    };

    let restval = take_kwarg(&mut kw, "restval").unwrap_or(Value::None);

    let mut dialect_arg = take_kwarg(&mut kw, "dialect");
    if matches!(dialect_arg, Some(Value::None))
        && let Some(value) = dialect_arg.take()
    {
        value.drop_with_heap(heap);
    }
    let mut dialect = match dialect_arg {
        Some(value) => {
            let parsed = parse_dialect_value(&value, heap, interns)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => lookup_dialect("excel")?,
    };
    apply_fmtparams(&mut dialect, &mut kw, heap, interns, "csv.DictReader")?;
    validate_no_kwargs(&mut kw, heap, "csv.DictReader")?;

    let parsed_rows = collect_parsed_rows_from_iterable(iterable, &dialect, heap, interns, "csv.DictReader")?;
    let mut string_rows: Vec<Vec<String>> = Vec::with_capacity(parsed_rows.len());
    for row in parsed_rows {
        string_rows.push(row.into_iter().map(|field| field.text).collect());
    }

    let (fieldnames, rows) = match fieldnames_value {
        Some(value) if !matches!(value, Value::None) => {
            let names = iterable_to_string_vec(&value, heap, interns, "csv.DictReader fieldnames")?;
            value.drop_with_heap(heap);
            (names, string_rows)
        }
        Some(value) => {
            value.drop_with_heap(heap);
            if let Some(first_row) = string_rows.first() {
                (first_row.clone(), string_rows.into_iter().skip(1).collect())
            } else {
                (Vec::new(), Vec::new())
            }
        }
        None => {
            if let Some(first_row) = string_rows.first() {
                (first_row.clone(), string_rows.into_iter().skip(1).collect())
            } else {
                (Vec::new(), Vec::new())
            }
        }
    };

    let object_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_csv_dict_reader(
        rows, fieldnames, restkey, restval,
    )))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Implements `csv.DictWriter(rows, fieldnames)`.
///
/// In Ouros's sandboxed environment (no file I/O), `DictWriter` takes a list of
/// dicts and a list of fieldnames, then returns a list of CSV-formatted strings
/// directly -- one string per row, values ordered by fieldnames.
///
/// Missing keys in a row dict produce an empty field (or the `restval` value).
fn dict_writer(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let rows_value = next_positional(&mut positional, "csv.DictWriter", 1)?;
    let fieldnames_pos = next_optional_positional(&mut positional);
    if let Some(extra) = next_optional_positional(&mut positional) {
        rows_value.drop_with_heap(heap);
        fieldnames_pos.drop_with_heap(heap);
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("csv.DictWriter", 2, 3));
    }
    positional.drop_with_heap(heap);

    let mut kw = kwargs_to_pairs(kwargs, heap, interns, "csv.DictWriter")?;
    let fieldnames_kw = take_kwarg(&mut kw, "fieldnames");
    if fieldnames_pos.is_some() && fieldnames_kw.is_some() {
        rows_value.drop_with_heap(heap);
        fieldnames_pos.drop_with_heap(heap);
        if let Some(value) = fieldnames_kw {
            value.drop_with_heap(heap);
        }
        drop_kwarg_pairs(kw, heap);
        return Err(ExcType::type_error(
            "csv.DictWriter() got multiple values for argument 'fieldnames'".to_string(),
        ));
    }
    let Some(fieldnames_value) = fieldnames_kw.or(fieldnames_pos) else {
        rows_value.drop_with_heap(heap);
        drop_kwarg_pairs(kw, heap);
        return Err(ExcType::type_error(
            "csv.DictWriter() missing required argument 'fieldnames'",
        ));
    };

    let fieldnames = iterable_to_string_vec(&fieldnames_value, heap, interns, "csv.DictWriter fieldnames")?;
    fieldnames_value.drop_with_heap(heap);

    let restval =
        take_kwarg(&mut kw, "restval").unwrap_or_else(|| Value::InternString(StaticStrings::EmptyString.into()));

    let mut dialect_arg = take_kwarg(&mut kw, "dialect");
    if matches!(dialect_arg, Some(Value::None))
        && let Some(value) = dialect_arg.take()
    {
        value.drop_with_heap(heap);
    }
    let mut dialect = match dialect_arg {
        Some(value) => {
            let parsed = parse_dialect_value(&value, heap, interns)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => lookup_dialect("excel")?,
    };
    apply_fmtparams(&mut dialect, &mut kw, heap, interns, "csv.DictWriter")?;
    validate_no_kwargs(&mut kw, heap, "csv.DictWriter")?;

    // Iterate over the input rows and format each dict as a CSV line.
    let mut iter = OurosIter::new(rows_value, heap, interns)?;
    let mut result_items: Vec<Value> = Vec::new();
    while let Some(row) = iter.for_next(heap, interns)? {
        let line = dict_writer_format_row(&row, &fieldnames, &restval, &dialect, heap, interns)?;
        row.drop_with_heap(heap);
        let str_id = heap.allocate(HeapData::Str(Str::from(line)))?;
        result_items.push(Value::Ref(str_id));
    }
    iter.drop_with_heap(heap);
    restval.drop_with_heap(heap);

    let list_id = heap.allocate(HeapData::List(List::new(result_items)))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Formats a single dict row as a CSV string using the given fieldnames and dialect.
///
/// For each fieldname, looks up the value in the dict. Missing keys use `restval`
/// (defaulting to empty string). The result is a delimiter-joined string without
/// a trailing line terminator.
fn dict_writer_format_row(
    row: &Value,
    fieldnames: &[String],
    restval: &Value,
    dialect: &CsvDialect,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let dict = match row {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Dict(dict) => dict,
            _ => return Err(ExcType::type_error("DictWriter row must be a dict")),
        },
        _ => return Err(ExcType::type_error("DictWriter row must be a dict")),
    };

    let mut out = String::new();
    for (index, name) in fieldnames.iter().enumerate() {
        if index > 0 {
            out.push(dialect.delimiter);
        }
        let field_text = if let Some(value) = dict.get_by_str(name, heap, interns) {
            if matches!(value, Value::None) {
                String::new()
            } else {
                value.py_str(heap, interns).into_owned()
            }
        } else if matches!(restval, Value::None) {
            String::new()
        } else {
            restval.py_str(heap, interns).into_owned()
        };
        // Apply quoting rules for special characters.
        let needs_quoting = field_text.contains(dialect.delimiter)
            || field_text.contains(dialect.quotechar)
            || field_text.contains('\n')
            || field_text.contains('\r');
        if needs_quoting
            || dialect.quoting == QUOTE_ALL
            || dialect.quoting == QUOTE_NOTNULL
            || dialect.quoting == QUOTE_STRINGS
        {
            let escaped = if dialect.doublequote {
                field_text.replace(dialect.quotechar, &format!("{0}{0}", dialect.quotechar))
            } else {
                field_text
            };
            out.push(dialect.quotechar);
            out.push_str(&escaped);
            out.push(dialect.quotechar);
        } else {
            out.push_str(&field_text);
        }
    }
    Ok(out)
}

/// Implements `csv.Sniffer()`.
fn sniffer(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("csv.Sniffer", heap)?;
    let object_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_csv_sniffer()))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Implements `csv.field_size_limit([limit])`.
fn field_size_limit(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let limit = args.get_zero_one_arg("csv.field_size_limit", heap)?;
    let cell = csv_field_size_limit();
    let mut guard = cell.lock().expect("csv field size limit mutex poisoned");
    let old = *guard;

    if let Some(value) = limit {
        let new_limit = value_to_i64(&value, heap, "csv.field_size_limit")?;
        value.drop_with_heap(heap);
        if new_limit < 0 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "field_size_limit must be >= 0").into());
        }
        *guard = new_limit;
    }

    Ok(AttrCallResult::Value(Value::Int(old)))
}

/// Implements `csv.register_dialect(name, dialect='excel', **fmtparams)`.
fn register_dialect(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let name_value = next_positional(&mut positional, "csv.register_dialect", 1)?;
    let dialect_pos = next_optional_positional(&mut positional);
    if let Some(extra) = next_optional_positional(&mut positional) {
        name_value.drop_with_heap(heap);
        dialect_pos.drop_with_heap(heap);
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("csv.register_dialect", 2, 3));
    }
    positional.drop_with_heap(heap);

    let name = value_to_string(&name_value, heap, interns, "csv.register_dialect name")?;
    name_value.drop_with_heap(heap);

    let mut kw = kwargs_to_pairs(kwargs, heap, interns, "csv.register_dialect")?;
    let dialect_kw = take_kwarg(&mut kw, "dialect");
    if dialect_pos.is_some() && dialect_kw.is_some() {
        dialect_pos.drop_with_heap(heap);
        if let Some(value) = dialect_kw {
            value.drop_with_heap(heap);
        }
        drop_kwarg_pairs(kw, heap);
        return Err(ExcType::type_error(
            "csv.register_dialect() got multiple values for argument 'dialect'".to_string(),
        ));
    }

    let mut dialect = match dialect_kw.or(dialect_pos) {
        Some(value) => {
            let parsed = parse_dialect_value(&value, heap, interns)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => lookup_dialect("excel")?,
    };
    apply_fmtparams(&mut dialect, &mut kw, heap, interns, "csv.register_dialect")?;
    validate_no_kwargs(&mut kw, heap, "csv.register_dialect")?;

    let registry = csv_dialects();
    let mut guard = registry.lock().expect("csv dialect registry mutex poisoned");
    guard.insert(name, dialect);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `csv.get_dialect(name)`.
fn get_dialect(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let name_value = args.get_one_arg("csv.get_dialect", heap)?;
    defer_drop!(name_value, heap);

    let name = match name_value {
        Value::InternString(string_id) => interns.get_str(*string_id).to_owned(),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Str(s) => s.as_str().to_owned(),
            _ => return Err(ExcType::type_error("csv.get_dialect() argument must be a string")),
        },
        _ => return Err(ExcType::type_error("csv.get_dialect() argument must be a string")),
    };

    let dialect = lookup_dialect(&name)?;
    let object_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_csv_dialect(dialect)))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Implements `csv.list_dialects()`.
fn list_dialects(heap: &mut Heap<impl ResourceTracker>) -> RunResult<AttrCallResult> {
    let names = {
        let registry = csv_dialects();
        let guard = registry.lock().expect("csv dialect registry mutex poisoned");
        guard.list_names()
    };
    let mut items = Vec::with_capacity(names.len());
    for name in names {
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        items.push(Value::Ref(name_id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Implements `csv.unregister_dialect(name)`.
fn unregister_dialect(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let name_value = args.get_one_arg("csv.unregister_dialect", heap)?;
    let name = value_to_string(&name_value, heap, interns, "csv.unregister_dialect name")?;
    name_value.drop_with_heap(heap);

    let registry = csv_dialects();
    let mut guard = registry.lock().expect("csv dialect registry mutex poisoned");
    if !guard.remove(&name) {
        return Err(SimpleException::new_msg(ExcType::Exception, "unknown dialect".to_string()).into());
    }
    Ok(AttrCallResult::Value(Value::None))
}

/// Parses one CSV line using dialect options.
pub(crate) fn parse_csv_row(row: &str, dialect: &CsvDialect) -> Vec<CsvParsedField> {
    let mut chars = row.chars().peekable();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if Some(ch) == dialect.escapechar {
                if let Some(next) = chars.next() {
                    field.push(next);
                } else {
                    field.push(ch);
                }
                continue;
            }

            if ch == dialect.quotechar {
                if dialect.doublequote && chars.peek() == Some(&dialect.quotechar) {
                    chars.next();
                    field.push(dialect.quotechar);
                } else {
                    in_quotes = false;
                }
                continue;
            }

            field.push(ch);
            continue;
        }

        if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            break;
        }

        if ch == dialect.delimiter {
            fields.push(CsvParsedField {
                text: std::mem::take(&mut field),
                quoted,
            });
            quoted = false;
            if dialect.skipinitialspace {
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
            }
            continue;
        }

        if dialect.quoting != QUOTE_NONE && ch == dialect.quotechar && field.is_empty() {
            in_quotes = true;
            quoted = true;
            continue;
        }

        if dialect.quoting == QUOTE_NONE && Some(ch) == dialect.escapechar {
            if let Some(next) = chars.next() {
                field.push(next);
            } else {
                field.push(ch);
            }
            continue;
        }

        field.push(ch);
    }

    fields.push(CsvParsedField { text: field, quoted });
    fields
}

/// Returns an inferred delimiter used by `csv.Sniffer`.
pub(crate) fn detect_delimiter(sample: &str) -> char {
    let candidates = [',', ';', '\t', '|'];
    let mut best = (',', 0usize);
    for candidate in candidates {
        let count = sample.chars().filter(|ch| *ch == candidate).count();
        if count > best.1 {
            best = (candidate, count);
        }
    }
    best.0
}

/// Returns whether a value should be treated as numeric for `QUOTE_NONNUMERIC`.
pub(crate) fn is_numeric_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::InternLongInt(_) => true,
        Value::Ref(id) => matches!(heap.get(*id), HeapData::LongInt(_)),
        _ => false,
    }
}

/// Returns the default excel dialect.
pub(crate) fn excel_dialect() -> CsvDialect {
    CsvDialect {
        delimiter: ',',
        quotechar: '"',
        escapechar: None,
        doublequote: true,
        skipinitialspace: false,
        lineterminator: "\r\n".to_owned(),
        quoting: QUOTE_MINIMAL,
    }
}

/// Returns the default excel-tab dialect.
pub(crate) fn excel_tab_dialect() -> CsvDialect {
    CsvDialect {
        delimiter: '\t',
        ..excel_dialect()
    }
}

/// Returns the default unix dialect.
pub(crate) fn unix_dialect() -> CsvDialect {
    CsvDialect {
        lineterminator: "\n".to_owned(),
        quoting: QUOTE_ALL,
        ..excel_dialect()
    }
}

/// Returns the global CSV dialect registry.
fn csv_dialects() -> &'static Mutex<CsvDialectRegistry> {
    CSV_DIALECTS.get_or_init(|| Mutex::new(CsvDialectRegistry::new()))
}

/// Returns the global field-size-limit cell.
fn csv_field_size_limit() -> &'static Mutex<i64> {
    CSV_FIELD_SIZE_LIMIT.get_or_init(|| Mutex::new(DEFAULT_FIELD_SIZE_LIMIT))
}

/// Looks up a dialect by name from the registry.
fn lookup_dialect(name: &str) -> RunResult<CsvDialect> {
    let registry = csv_dialects();
    let guard = registry.lock().expect("csv dialect registry mutex poisoned");
    guard
        .get(name)
        .ok_or_else(|| SimpleException::new_msg(ExcType::Exception, "unknown dialect".to_string()).into())
}

/// Creates a simple class object for csv module compatibility.
fn create_csv_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    class_name: &str,
    bases: &[Type],
) -> Result<HeapId, ResourceError> {
    let mut base_ids = Vec::with_capacity(bases.len());
    for base in bases {
        base_ids.push(heap.builtin_class_id(*base)?);
    }
    for base_id in &base_ids {
        heap.inc_ref(*base_id);
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap(class_name.to_owned()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        base_ids.clone(),
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &base_ids, heap, interns).expect("csv helper class MRO must be valid");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    for &base_id in &base_ids {
        heap.with_entry_mut(base_id, |_, data| {
            let HeapData::ClassObject(base_cls) = data else {
                return Err(ExcType::type_error("csv base is not a class".to_string()));
            };
            base_cls.register_subclass(class_id, class_uid);
            Ok(())
        })
        .expect("csv class base mutation should succeed");
    }

    Ok(class_id)
}

/// Creates a dialect class (excel, excel_tab, unix_dialect) with class attributes.
///
/// The created class inherits from `Dialect` and has class attributes set to match
/// the dialect configuration. When called, it creates a `StdlibObject::CsvDialect`
/// instance with the appropriate default settings.
fn create_dialect_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    class_name: &str,
    dialect_base_id: HeapId,
    dialect: CsvDialect,
) -> Result<HeapId, ResourceError> {
    // Create the class with Dialect as its base
    heap.inc_ref(dialect_base_id);
    let class_uid = heap.next_class_uid();
    let mut namespace = Dict::new();

    // Add dialect configuration as class attributes
    let delimiter_key = heap.allocate(HeapData::Str(Str::from("delimiter")))?;
    let delimiter_value = heap.allocate(HeapData::Str(Str::from(dialect.delimiter.to_string())))?;
    let _ = namespace.set(Value::Ref(delimiter_key), Value::Ref(delimiter_value), heap, interns);

    let quotechar_key = heap.allocate(HeapData::Str(Str::from("quotechar")))?;
    let quotechar_value = heap.allocate(HeapData::Str(Str::from(dialect.quotechar.to_string())))?;
    let _ = namespace.set(Value::Ref(quotechar_key), Value::Ref(quotechar_value), heap, interns);

    let escapechar_key = heap.allocate(HeapData::Str(Str::from("escapechar")))?;
    let escapechar_value = match dialect.escapechar {
        Some(ch) => {
            let id = heap.allocate(HeapData::Str(Str::from(ch.to_string())))?;
            Value::Ref(id)
        }
        None => Value::None,
    };
    let _ = namespace.set(Value::Ref(escapechar_key), escapechar_value, heap, interns);

    let doublequote_key = heap.allocate(HeapData::Str(Str::from("doublequote")))?;
    let _ = namespace.set(
        Value::Ref(doublequote_key),
        Value::Bool(dialect.doublequote),
        heap,
        interns,
    );

    let skipinitialspace_key = heap.allocate(HeapData::Str(Str::from("skipinitialspace")))?;
    let _ = namespace.set(
        Value::Ref(skipinitialspace_key),
        Value::Bool(dialect.skipinitialspace),
        heap,
        interns,
    );

    let lineterminator_key = heap.allocate(HeapData::Str(Str::from("lineterminator")))?;
    let lineterminator_value = heap.allocate(HeapData::Str(Str::from(dialect.lineterminator.clone())))?;
    let _ = namespace.set(
        Value::Ref(lineterminator_key),
        Value::Ref(lineterminator_value),
        heap,
        interns,
    );

    let quoting_key = heap.allocate(HeapData::Str(Str::from("quoting")))?;
    let _ = namespace.set(Value::Ref(quoting_key), Value::Int(dialect.quoting), heap, interns);

    let class_obj = ClassObject::new(
        EitherStr::Heap(class_name.to_owned()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        namespace,
        vec![dialect_base_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    // Compute and set the MRO
    let mro = compute_c3_mro(class_id, &[dialect_base_id], heap, interns).expect("dialect class MRO must be valid");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    // Register this class as a subclass of Dialect
    heap.with_entry_mut(dialect_base_id, |_, data| {
        let HeapData::ClassObject(base_cls) = data else {
            return Err(ExcType::type_error("Dialect base is not a class".to_string()));
        };
        base_cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("dialect class base mutation should succeed");

    Ok(class_id)
}

/// Collects rows from a string iterable and parses each row.
fn collect_parsed_rows_from_iterable(
    iterable: Value,
    dialect: &CsvDialect,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    context: &str,
) -> RunResult<Vec<Vec<CsvParsedField>>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut rows = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        let row_text = value_to_string(&item, heap, interns, context)?;
        item.drop_with_heap(heap);
        let parsed = parse_csv_row(&row_text, dialect);
        check_field_size_limit(&parsed)?;
        rows.push(parsed);
    }
    iter.drop_with_heap(heap);
    Ok(rows)
}

/// Validates all parsed fields against the current `field_size_limit`.
fn check_field_size_limit(parsed: &[CsvParsedField]) -> RunResult<()> {
    let current_limit = {
        let guard = csv_field_size_limit()
            .lock()
            .expect("csv field size limit mutex poisoned");
        *guard
    };
    let limit_usize = current_limit as usize;
    for field in parsed {
        if field.text.len() > limit_usize {
            return Err(
                SimpleException::new_msg(ExcType::Exception, "field larger than field limit".to_string()).into(),
            );
        }
    }
    Ok(())
}

/// Parses a dialect from user input (`str` name, dialect object, class, or dict).
fn parse_dialect_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<CsvDialect> {
    match value {
        Value::InternString(string_id) => lookup_dialect(interns.get_str(*string_id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => lookup_dialect(s.as_str()),
            HeapData::Dict(dict) => {
                let mut dialect = excel_dialect();
                apply_dict_fmtparams(&mut dialect, dict, heap, interns)?;
                Ok(dialect)
            }
            HeapData::StdlibObject(StdlibObject::CsvDialect(dialect)) => Ok(dialect.clone()),
            HeapData::ClassObject(class_obj) => parse_dialect_from_class(class_obj, heap, interns),
            _ => Err(ExcType::type_error(
                "dialect must be a string name or dialect object".to_string(),
            )),
        },
        _ => Err(ExcType::type_error(
            "dialect must be a string name or dialect object".to_string(),
        )),
    }
}

/// Parses a dialect from class attributes.
fn parse_dialect_from_class(
    class_obj: &ClassObject,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<CsvDialect> {
    let mut dialect = excel_dialect();
    if let Some(value) = lookup_class_attr(class_obj, "delimiter", heap, interns) {
        dialect.delimiter = one_char_from_value(value, heap, interns, "delimiter")?;
    }
    if let Some(value) = lookup_class_attr(class_obj, "quotechar", heap, interns) {
        dialect.quotechar = one_char_from_value(value, heap, interns, "quotechar")?;
    }
    if let Some(value) = lookup_class_attr(class_obj, "escapechar", heap, interns) {
        dialect.escapechar = value_to_optional_char(value, heap, interns, "escapechar")?;
    }
    if let Some(value) = lookup_class_attr(class_obj, "doublequote", heap, interns) {
        dialect.doublequote = value.py_bool(heap, interns);
    }
    if let Some(value) = lookup_class_attr(class_obj, "skipinitialspace", heap, interns) {
        dialect.skipinitialspace = value.py_bool(heap, interns);
    }
    if let Some(value) = lookup_class_attr(class_obj, "lineterminator", heap, interns) {
        dialect.lineterminator = value_to_string(value, heap, interns, "lineterminator")?;
    }
    if let Some(value) = lookup_class_attr(class_obj, "quoting", heap, interns) {
        dialect.quoting = value_to_i64(value, heap, "quoting")?;
    }
    Ok(dialect)
}

/// Looks up a class attribute by name across the class MRO.
fn lookup_class_attr<'a>(
    class_obj: &'a ClassObject,
    attr_name: &str,
    heap: &'a Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<&'a Value> {
    if let Some(value) = class_obj.namespace().get_by_str(attr_name, heap, interns) {
        return Some(value);
    }
    for base_id in class_obj.mro().iter().skip(1) {
        let HeapData::ClassObject(base_cls) = heap.get(*base_id) else {
            continue;
        };
        if let Some(value) = base_cls.namespace().get_by_str(attr_name, heap, interns) {
            return Some(value);
        }
    }
    None
}

/// Applies dialect fmtparams from keyword args.
fn apply_fmtparams(
    dialect: &mut CsvDialect,
    kwargs: &mut Vec<(String, Value)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function_name: &str,
) -> RunResult<()> {
    if let Some(value) = take_kwarg(kwargs, "delimiter") {
        dialect.delimiter = one_char_from_value(&value, heap, interns, "delimiter")?;
        value.drop_with_heap(heap);
    }
    if let Some(value) = take_kwarg(kwargs, "quotechar") {
        dialect.quotechar = one_char_from_value(&value, heap, interns, "quotechar")?;
        value.drop_with_heap(heap);
    }
    if let Some(value) = take_kwarg(kwargs, "escapechar") {
        dialect.escapechar = value_to_optional_char(&value, heap, interns, "escapechar")?;
        value.drop_with_heap(heap);
    }
    if let Some(value) = take_kwarg(kwargs, "doublequote") {
        dialect.doublequote = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
    }
    if let Some(value) = take_kwarg(kwargs, "skipinitialspace") {
        dialect.skipinitialspace = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
    }
    if let Some(value) = take_kwarg(kwargs, "lineterminator") {
        dialect.lineterminator = value_to_string(&value, heap, interns, "lineterminator")?;
        value.drop_with_heap(heap);
    }
    if let Some(value) = take_kwarg(kwargs, "quoting") {
        dialect.quoting = value_to_i64(&value, heap, "quoting")?;
        value.drop_with_heap(heap);
    }

    if !matches!(
        dialect.quoting,
        QUOTE_MINIMAL | QUOTE_ALL | QUOTE_NONNUMERIC | QUOTE_NONE | QUOTE_STRINGS | QUOTE_NOTNULL
    ) {
        return Err(ExcType::type_error(format!("{function_name}() invalid quoting value")));
    }
    Ok(())
}

/// Applies fmtparams from a dictionary dialect object.
fn apply_dict_fmtparams(
    dialect: &mut CsvDialect,
    dict: &Dict,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    if let Some(value) = dict.get_by_str("delimiter", heap, interns) {
        dialect.delimiter = one_char_from_value(value, heap, interns, "delimiter")?;
    }
    if let Some(value) = dict.get_by_str("quotechar", heap, interns) {
        dialect.quotechar = one_char_from_value(value, heap, interns, "quotechar")?;
    }
    if let Some(value) = dict.get_by_str("escapechar", heap, interns) {
        dialect.escapechar = value_to_optional_char(value, heap, interns, "escapechar")?;
    }
    if let Some(value) = dict.get_by_str("doublequote", heap, interns) {
        dialect.doublequote = value.py_bool(heap, interns);
    }
    if let Some(value) = dict.get_by_str("skipinitialspace", heap, interns) {
        dialect.skipinitialspace = value.py_bool(heap, interns);
    }
    if let Some(value) = dict.get_by_str("lineterminator", heap, interns) {
        dialect.lineterminator = value_to_string(value, heap, interns, "lineterminator")?;
    }
    if let Some(value) = dict.get_by_str("quoting", heap, interns) {
        dialect.quoting = value_to_i64(value, heap, "quoting")?;
    }
    Ok(())
}

/// Converts kwargs into owned `(name, value)` pairs.
fn kwargs_to_pairs(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function_name: &str,
) -> RunResult<Vec<(String, Value)>> {
    let mut pairs = Vec::with_capacity(kwargs.len());
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        pairs.push((key_name.as_str(interns).to_owned(), value));
    }
    if pairs.is_empty() {
        return Ok(pairs);
    }
    // Keep behavior deterministic by preserving incoming order.
    let _ = function_name;
    Ok(pairs)
}

/// Removes and returns a keyword value by name.
fn take_kwarg(kwargs: &mut Vec<(String, Value)>, name: &str) -> Option<Value> {
    kwargs
        .iter()
        .position(|(key, _)| key == name)
        .map(|index| kwargs.remove(index).1)
}

/// Raises on unexpected remaining kwargs.
fn validate_no_kwargs(
    kwargs: &mut Vec<(String, Value)>,
    heap: &mut Heap<impl ResourceTracker>,
    function_name: &str,
) -> RunResult<()> {
    if let Some((key, value)) = kwargs.pop() {
        value.drop_with_heap(heap);
        drop_kwarg_pairs(std::mem::take(kwargs), heap);
        return Err(ExcType::type_error(format!(
            "'{key}' is an invalid keyword argument for {function_name}()"
        )));
    }
    Ok(())
}

/// Returns and removes the next required positional argument.
fn next_positional(positional: &mut ArgPosIter, function_name: &str, required: usize) -> RunResult<Value> {
    positional
        .next()
        .ok_or_else(|| ExcType::type_error_at_least(function_name, required, 0))
}

/// Returns and removes the next optional positional argument.
fn next_optional_positional(positional: &mut ArgPosIter) -> Option<Value> {
    positional.next()
}

/// Drops keyword argument value pairs.
fn drop_kwarg_pairs(kwargs: Vec<(String, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (_, value) in kwargs {
        value.drop_with_heap(heap);
    }
}

/// Converts an arbitrary iterable to a vector of strings.
fn iterable_to_string_vec(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    context: &str,
) -> RunResult<Vec<String>> {
    let mut iter = OurosIter::new(value.clone_with_heap(heap), heap, interns)?;
    let mut out = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        let text = value_to_string(&item, heap, interns, context)?;
        item.drop_with_heap(heap);
        out.push(text);
    }
    iter.drop_with_heap(heap);
    Ok(out)
}

/// Converts a value to string for csv APIs.
fn value_to_string(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    context: &str,
) -> RunResult<String> {
    match value {
        Value::InternString(string_id) => Ok(interns.get_str(*string_id).to_owned()),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!("{context} expects string values"))),
        },
        _ => Err(ExcType::type_error(format!("{context} expects string values"))),
    }
}

/// Converts a value to optional string (`None` -> `None`).
fn value_to_optional_string(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    context: &str,
) -> RunResult<Option<String>> {
    if matches!(value, Value::None) {
        return Ok(None);
    }
    Ok(Some(value_to_string(value, heap, interns, context)?))
}

/// Extracts a one-character string.
fn one_char_from_value(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    name: &str,
) -> RunResult<char> {
    let s = value_to_string(value, heap, interns, name)?;
    let mut chars = s.chars();
    let Some(ch) = chars.next() else {
        return Err(ExcType::type_error(format!("{name} must be a 1-character string")));
    };
    if chars.next().is_some() {
        return Err(ExcType::type_error(format!("{name} must be a 1-character string")));
    }
    Ok(ch)
}

/// Extracts an optional char where `None` is allowed.
fn value_to_optional_char(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    name: &str,
) -> RunResult<Option<char>> {
    if matches!(value, Value::None) {
        return Ok(None);
    }
    one_char_from_value(value, heap, interns, name).map(Some)
}

/// Converts a value to i64 for quoting and field-size APIs.
fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>, context: &str) -> RunResult<i64> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                li.to_i64().ok_or_else(|| {
                    SimpleException::new_msg(ExcType::OverflowError, format!("{context} argument too large")).into()
                })
            } else {
                Err(ExcType::type_error(format!("{context} argument must be an integer")))
            }
        }
        _ => Err(ExcType::type_error(format!("{context} argument must be an integer"))),
    }
}
