//! Lightweight runtime objects used by stdlib compatibility shims.
//!
//! These objects model class-like APIs exposed from modules such as
//! `contextlib`, `string`, and `json` without requiring full Python class
//! implementations for each helper.
#![expect(clippy::cast_possible_truncation, reason = "narrowing is runtime-checked")]
#![expect(clippy::cast_sign_loss, reason = "sign changes are intentional")]
#![expect(clippy::cast_possible_wrap, reason = "wrapping preserves parity behavior")]
#![expect(clippy::unnecessary_wraps, reason = "helpers share dispatch signatures")]
#![expect(clippy::type_complexity, reason = "tuple-heavy APIs mirror Python protocols")]
#![expect(clippy::fn_params_excessive_bools, reason = "bool flags mirror Python kwargs")]
#![expect(clippy::struct_excessive_bools, reason = "state mirrors Python flag fields")]
#![expect(dead_code, reason = "some stdlib shims are parity-only")]

use std::{borrow::Cow, fmt::Write, str::FromStr};

use ahash::AHashSet;
use fancy_regex::Regex;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    fstring::ascii_escape,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    io::RedirectTarget,
    modules::{
        ModuleFunctions,
        csv_mod::{
            CsvDialect, CsvParsedField, QUOTE_ALL, QUOTE_MINIMAL, QUOTE_NONE, QUOTE_NONNUMERIC, QUOTE_NOTNULL,
            QUOTE_STRINGS, detect_delimiter, is_numeric_value, parse_csv_row,
        },
        decimal_mod::{self, DecimalContextConfig},
        os as os_mod,
        pprint_mod::{self, PprintParams},
        struct_mod::StructFunctions,
    },
    resource::ResourceTracker,
    types::{AttrCallResult, Bytes, Dict, List, OurosIter, PyTrait, Str, Type, allocate_tuple},
    value::{EitherStr, Value},
};

const RE_FLAG_IGNORECASE: i64 = 2;
const RE_FLAG_LOCALE: i64 = 4;
const RE_FLAG_MULTILINE: i64 = 8;
const RE_FLAG_DOTALL: i64 = 16;
const RE_FLAG_UNICODE: i64 = 32;
const RE_FLAG_VERBOSE: i64 = 64;
const RE_FLAG_DEBUG: i64 = 128;
const RE_FLAG_ASCII: i64 = 256;

fn regex_flag_bits_by_name(name: &str) -> Option<i64> {
    match name {
        "IGNORECASE" | "I" => Some(RE_FLAG_IGNORECASE),
        "LOCALE" | "L" => Some(RE_FLAG_LOCALE),
        "MULTILINE" | "M" => Some(RE_FLAG_MULTILINE),
        "DOTALL" | "S" => Some(RE_FLAG_DOTALL),
        "UNICODE" | "U" => Some(RE_FLAG_UNICODE),
        "VERBOSE" | "X" => Some(RE_FLAG_VERBOSE),
        "DEBUG" => Some(RE_FLAG_DEBUG),
        "ASCII" | "A" => Some(RE_FLAG_ASCII),
        "NOFLAG" => Some(0),
        _ => None,
    }
}

fn regex_flag_repr(bits: i64) -> String {
    if bits == 0 {
        return "re.NOFLAG".to_string();
    }

    let ordered = [
        (RE_FLAG_IGNORECASE, "re.IGNORECASE"),
        (RE_FLAG_MULTILINE, "re.MULTILINE"),
        (RE_FLAG_DOTALL, "re.DOTALL"),
        (RE_FLAG_UNICODE, "re.UNICODE"),
        (RE_FLAG_VERBOSE, "re.VERBOSE"),
        (RE_FLAG_LOCALE, "re.LOCALE"),
        (RE_FLAG_DEBUG, "re.DEBUG"),
        (RE_FLAG_ASCII, "re.ASCII"),
    ];
    let mut remaining = bits;
    let mut names: Vec<&str> = Vec::new();
    for (flag, name) in ordered {
        if remaining & flag != 0 {
            names.push(name);
            remaining &= !flag;
        }
    }
    if !names.is_empty() && remaining == 0 {
        return names.join("|");
    }
    format!("re.RegexFlag({bits})")
}

/// Formats a float component for complex repr output.
///
/// CPython's complex repr omits trailing `.0` for integral components while
/// still using lowercase `inf`/`nan` spellings.
fn complex_component_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf".to_owned()
        } else {
            "inf".to_owned()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".to_owned()
        } else {
            "0".to_owned()
        };
    }

    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

/// Formats a complex value for `repr()`/`str()` parity.
fn complex_repr(real: f64, imag: f64) -> String {
    let imag_abs = complex_component_repr(imag.abs());
    if real == 0.0 && !real.is_sign_negative() {
        if imag.is_sign_negative() {
            format!("-{imag_abs}j")
        } else {
            format!("{imag_abs}j")
        }
    } else {
        let real_repr = complex_component_repr(real);
        let sign = if imag.is_sign_negative() { '-' } else { '+' };
        format!("({real_repr}{sign}{imag_abs}j)")
    }
}

/// Callback entries registered on an `ExitStack`-like object.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum ExitCallback {
    /// Context-manager object that will be invoked via `__exit__` / `__aexit__`.
    ExitMethod(Value),
    /// Exit callback callable that accepts `(exc_type, exc, tb)` (`ExitStack.push`).
    ExitFunc(Value),
    /// Simple callback with pre-bound positional and keyword args.
    Callback {
        func: Value,
        args: Vec<Value>,
        kwargs: Vec<(Value, Value)>,
    },
}

/// Runtime state for `contextlib.ExitStack` and `contextlib.AsyncExitStack`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ExitStackState {
    /// Whether this stack was constructed as `AsyncExitStack`.
    pub(crate) async_mode: bool,
    /// Heap identity used by `__enter__` to return `self`.
    pub(crate) self_id: Option<HeapId>,
    /// Registered callbacks in push order.
    pub(crate) callbacks: Vec<ExitCallback>,
}

/// Runtime state for simple context manager wrappers from `contextlib`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ContextManagerState {
    /// Human-readable context manager type name used in repr/errors.
    pub(crate) name: String,
    /// Value returned by `__enter__`.
    pub(crate) enter_value: Value,
    /// Exception types suppressed by `__exit__` (`contextlib.suppress` only).
    pub(crate) suppress_types: Vec<Value>,
}

/// Runtime state for objects returned by `contextlib.contextmanager(...)`.
///
/// This stores the original generator function so calling the returned object
/// can instantiate a fresh generator-backed context manager.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratorContextManagerFactoryState {
    /// Generator function wrapped by `@contextmanager`.
    pub(crate) func: Value,
    /// Whether this factory came from `@asynccontextmanager`.
    pub(crate) async_mode: bool,
}

/// Runtime state for one live generator-backed context manager instance.
///
/// The wrapped generator is advanced via `__enter__` and finalized via
/// `__exit__`/`__aexit__`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratorContextManagerState {
    /// Generator object created from the wrapped function.
    pub(crate) generator: Value,
    /// Whether this manager came from `@asynccontextmanager`.
    pub(crate) async_mode: bool,
}

/// Runtime state for a context-manager-backed decorator wrapper.
///
/// The wrapper is callable and runs `wrapped` inside the generator context.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratorContextDecoratorState {
    /// Context manager object driving setup/cleanup.
    pub(crate) generator: Value,
    /// Function being decorated.
    pub(crate) wrapped: Value,
    /// Whether wrapped results should be awaited before cleanup.
    pub(crate) async_mode: bool,
    /// Whether cleanup uses `__exit__`/`__aexit__` instead of generator close.
    pub(crate) close_with_exit: bool,
}

/// Runtime state for instance-based context decorator wrappers.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct InstanceContextDecoratorState {
    /// Context manager instance used around wrapped calls.
    pub(crate) manager: Value,
    /// Function being decorated.
    pub(crate) wrapped: Value,
    /// Whether this wrapper should use async context protocol.
    pub(crate) async_mode: bool,
}

/// Internal callable state that advances a generator via `__next__`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratorNextCallableState {
    /// Generator object to advance.
    pub(crate) generator: Value,
}

/// Internal callable state that finalizes a generator via `close()`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratorCloseCallableState {
    /// Generator object to close.
    pub(crate) generator: Value,
}

/// Internal callable state that advances a generator with default `None`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GeneratorNextDefaultCallableState {
    /// Generator object to advance.
    pub(crate) generator: Value,
}

/// Internal awaitable wrapper that resolves immediately to a stored value.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ImmediateAwaitableState {
    /// Value yielded when this awaitable is awaited.
    pub(crate) value: Value,
}

/// Wrapper object exposed from `aiter(...)` for async-generator parity.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AsyncGeneratorState {
    /// Wrapped iterator object used to preserve identity and lifetime.
    pub(crate) iterator: Value,
}

/// Awaitable object returned by `anext(...)`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AnextAwaitableState {
    /// Async iterator to advance when awaited.
    pub(crate) iterator: Value,
    /// Optional default value used when iterator is exhausted.
    pub(crate) default: Option<Value>,
}

/// Runtime state for `string.Template`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct TemplateState {
    /// Source template text.
    template: String,
    /// Placeholder delimiter character string.
    delimiter: String,
    /// Identifier validation pattern.
    idpattern: String,
}

/// Runtime state for `json.JSONEncoder`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct JsonEncoderState {
    /// Indentation width, `None` for compact output.
    indent: Option<usize>,
    /// Whether object keys should be sorted.
    sort_keys: bool,
    /// Whether non-ASCII characters should be escaped as `\uXXXX`.
    ensure_ascii: bool,
    /// Whether unsupported dictionary keys are skipped instead of raising `TypeError`.
    skipkeys: bool,
    /// Whether `NaN`/`Infinity`/`-Infinity` can be emitted.
    allow_nan: bool,
    /// Whether circular references are detected and rejected with `ValueError`.
    check_circular: bool,
}

/// Runtime state for `json.JSONDecoder`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct JsonDecoderState {
    /// Callback invoked for decoded dictionaries.
    object_hook: Option<Value>,
    /// Callback invoked for floating-point number tokens.
    parse_float: Option<Value>,
    /// Callback invoked for integer number tokens.
    parse_int: Option<Value>,
    /// Callback invoked for non-finite constants.
    parse_constant: Option<Value>,
    /// Callback invoked with key/value pair order for decoded objects.
    object_pairs_hook: Option<Value>,
    /// Stored strict mode flag for parity with CPython constructor signature.
    strict: bool,
}

/// Runtime state for `pprint.PrettyPrinter`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PrettyPrinterState {
    /// Printer formatting parameters.
    params: PprintParams,
    /// Class object used for `type(instance)` output.
    class_id: HeapId,
}

impl PrettyPrinterState {
    /// Returns the class object ID used for this instance.
    #[must_use]
    pub(crate) fn class_id(&self) -> HeapId {
        self.class_id
    }
}

/// Runtime state for `io.StringIO`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct StringIOState {
    /// The internal string buffer.
    buffer: String,
    /// Current position in the buffer.
    position: usize,
    /// Whether the stream is closed.
    closed: bool,
    /// Newline translation mode.
    newline: String,
    /// Line endings translated so far (for newlines property).
    newlines_translated: Option<Vec<String>>,
}

impl StringIOState {
    /// Reads the next line from the buffer, advancing the position.
    ///
    /// Used by the iteration protocol to implement line-by-line iteration
    /// over StringIO objects (e.g., `for line in stringio:`).
    /// Returns an empty string when the buffer is exhausted.
    pub(crate) fn readline(&mut self) -> String {
        string_io_readline(self, None)
    }
}

/// Runtime state for `io.BytesIO`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct BytesIOState {
    /// The internal byte buffer.
    buffer: Vec<u8>,
    /// Current position in the buffer.
    position: usize,
    /// Whether the stream is closed.
    closed: bool,
    /// Heap IDs returned by `getbuffer()` that still hold a BytesIO-owned tracking ref.
    ///
    /// Each exported buffer gets one extra `inc_ref` so `close()` can detect
    /// whether user code still holds a live reference (`refcount > 1`) and
    /// raise `BufferError` like CPython.
    #[serde(default)]
    exported_buffers: Vec<HeapId>,
}

/// Runtime state for `struct.Struct`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct StructState {
    /// Compiled format string.
    format: String,
    /// Cached struct size in bytes.
    size: usize,
}

/// Runtime state for `decimal.Context` objects.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DecimalContextState {
    /// Active precision.
    prec: i64,
    /// Active rounding mode string.
    rounding: String,
    /// Optional context snapshot to restore on `__exit__`.
    saved: Option<DecimalContextConfig>,
}

/// Runtime state for objects returned by `Decimal.as_tuple()`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DecimalTupleState {
    /// Sign bit: `0` for positive, `1` for negative.
    sign: i64,
    /// Decimal digits of the coefficient.
    digits: Vec<i64>,
    /// Base-10 exponent.
    exponent: i64,
}

/// Runtime state for `csv.reader`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CsvReaderState {
    /// Parsed rows.
    rows: Vec<Vec<CsvParsedField>>,
    /// Current cursor into `rows`.
    index: usize,
    /// Active dialect options.
    dialect: CsvDialect,
}

/// Runtime state for `csv.writer`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CsvWriterState {
    /// File-like object receiving serialized rows.
    file_obj: Value,
    /// Active dialect options.
    dialect: CsvDialect,
}

/// Runtime state for `csv.DictReader`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CsvDictReaderState {
    /// Parsed data rows (header already consumed when inferred).
    rows: Vec<Vec<String>>,
    /// Field names used as output dictionary keys.
    fieldnames: Vec<String>,
    /// Optional key for excess row values.
    restkey: Option<String>,
    /// Value used for missing row columns.
    restval: Value,
    /// Current cursor into `rows`.
    index: usize,
}

/// Runtime state for `csv.DictWriter`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CsvDictWriterState {
    /// File-like object receiving serialized rows.
    file_obj: Value,
    /// Field names used to order dictionary values.
    fieldnames: Vec<String>,
    /// Value used for missing keys in row dictionaries.
    restval: Value,
    /// Either `raise` or `ignore`.
    extrasaction: String,
    /// Active dialect options.
    dialect: CsvDialect,
}

/// One lexing rule for `re.Scanner`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReScannerRule {
    /// Pattern used to match at the current cursor position.
    pub(crate) pattern: String,
    /// Optional token label to emit. `None` means "skip token".
    pub(crate) tag: Option<String>,
}

/// Runtime state for scanner objects returned by `re.Scanner(...)`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReScannerState {
    /// Scanner rules in declaration order.
    rules: Vec<ReScannerRule>,
}

/// Small tagged object used for stdlib class-like APIs.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum StdlibObject {
    /// `contextlib.ExitStack` / `contextlib.AsyncExitStack`.
    ExitStack(ExitStackState),
    /// Lightweight context manager wrappers (`suppress`, `redirect_*`, etc.).
    ContextManager(ContextManagerState),
    /// Callable wrapper returned by `contextlib.contextmanager`.
    GeneratorContextManagerFactory(GeneratorContextManagerFactoryState),
    /// Live context manager instance created by a generator context factory.
    GeneratorContextManager(GeneratorContextManagerState),
    /// Callable returned when a generator context manager is used as a decorator.
    GeneratorContextDecorator(GeneratorContextDecoratorState),
    /// Callable wrapper for `ContextDecorator` / `AsyncContextDecorator` instances.
    InstanceContextDecorator(InstanceContextDecoratorState),
    /// Internal callable that dispatches to `generator.__next__()`.
    GeneratorNextCallable(GeneratorNextCallableState),
    /// Internal callable that dispatches to `next(generator, None)`.
    GeneratorNextDefaultCallable(GeneratorNextDefaultCallableState),
    /// Internal callable that dispatches to `generator.close()`.
    GeneratorCloseCallable(GeneratorCloseCallableState),
    /// Internal awaitable that resolves immediately to a stored value.
    ImmediateAwaitable(ImmediateAwaitableState),
    /// Async-generator facade returned by `aiter(...)`.
    AsyncGenerator(AsyncGeneratorState),
    /// Awaitable returned by `anext(...)`.
    AnextAwaitable(AnextAwaitableState),
    /// `string.Formatter`.
    Formatter,
    /// `string.Template`.
    Template(TemplateState),
    /// `json.JSONEncoder`.
    JsonEncoder(JsonEncoderState),
    /// `json.JSONDecoder`.
    JsonDecoder(JsonDecoderState),
    /// `pprint.PrettyPrinter`.
    PrettyPrinter(PrettyPrinterState),
    /// `io.StringIO`.
    StringIO(StringIOState),
    /// `io.BytesIO`.
    BytesIO(BytesIOState),
    /// `struct.Struct`.
    Struct(StructState),
    /// `decimal.Context`.
    DecimalContext(DecimalContextState),
    /// Return object for `Decimal.as_tuple()`.
    DecimalTuple(DecimalTupleState),
    /// A runtime complex number value (`complex`).
    Complex { real: f64, imag: f64 },
    /// `csv.reader`.
    CsvReader(CsvReaderState),
    /// `csv.writer`.
    CsvWriter(CsvWriterState),
    /// `csv.DictReader`.
    CsvDictReader(CsvDictReaderState),
    /// `csv.DictWriter`.
    CsvDictWriter(CsvDictWriterState),
    /// Dialect object returned by `csv.get_dialect()` and `csv.Sniffer.sniff()`.
    CsvDialect(CsvDialect),
    /// `csv.Sniffer`.
    CsvSniffer,
    /// A `re.RegexFlag` value.
    RegexFlagValue(i64),
    /// Namespace object exposed as `re.RegexFlag`.
    RegexFlagEnum,
    /// Scanner object returned by `re.Scanner` / `re.Pattern.scanner`.
    ReScanner(ReScannerState),
}

impl StdlibObject {
    /// Creates an ExitStack-like object.
    #[must_use]
    pub fn new_exit_stack(async_mode: bool) -> Self {
        Self::ExitStack(ExitStackState {
            async_mode,
            self_id: None,
            callbacks: Vec::new(),
        })
    }

    /// Records the heap identity for an `ExitStack` object.
    ///
    /// This allows `ExitStack.__enter__` to return `self` with correct identity.
    pub fn set_exit_stack_self_id(&mut self, id: HeapId) {
        if let Self::ExitStack(state) = self {
            state.self_id = Some(id);
        }
    }

    /// Creates a formatter object.
    #[must_use]
    pub fn new_formatter() -> Self {
        Self::Formatter
    }

    /// Creates a simple context manager wrapper.
    #[must_use]
    pub fn new_context_manager(name: impl Into<String>, enter_value: Value) -> Self {
        Self::ContextManager(ContextManagerState {
            name: name.into(),
            enter_value,
            suppress_types: Vec::new(),
        })
    }

    /// Creates a callable `@contextmanager` wrapper around a generator function.
    #[must_use]
    pub fn new_generator_context_manager_factory(func: Value, async_mode: bool) -> Self {
        Self::GeneratorContextManagerFactory(GeneratorContextManagerFactoryState { func, async_mode })
    }

    /// Creates a live generator-backed context manager instance.
    #[must_use]
    pub fn new_generator_context_manager(generator: Value, async_mode: bool) -> Self {
        Self::GeneratorContextManager(GeneratorContextManagerState { generator, async_mode })
    }

    /// Creates a callable decorator wrapper that executes `wrapped` in `generator`.
    #[must_use]
    pub fn new_generator_context_decorator(generator: Value, wrapped: Value, async_mode: bool) -> Self {
        Self::GeneratorContextDecorator(GeneratorContextDecoratorState {
            generator,
            wrapped,
            async_mode,
            close_with_exit: false,
        })
    }

    /// Creates a callable decorator wrapper for context-manager instances.
    #[must_use]
    pub fn new_instance_context_decorator(manager: Value, wrapped: Value, async_mode: bool) -> Self {
        Self::InstanceContextDecorator(InstanceContextDecoratorState {
            manager,
            wrapped,
            async_mode,
        })
    }

    /// Creates an internal callable that advances a generator one step.
    #[must_use]
    pub fn new_generator_next_callable(generator: Value) -> Self {
        Self::GeneratorNextCallable(GeneratorNextCallableState { generator })
    }

    /// Creates an internal callable that advances a generator and maps
    /// `StopIteration` to `None`.
    #[must_use]
    pub fn new_generator_next_default_callable(generator: Value) -> Self {
        Self::GeneratorNextDefaultCallable(GeneratorNextDefaultCallableState { generator })
    }

    /// Creates an internal callable that closes a generator.
    #[must_use]
    pub fn new_generator_close_callable(generator: Value) -> Self {
        Self::GeneratorCloseCallable(GeneratorCloseCallableState { generator })
    }

    /// Creates an awaitable wrapper that resolves immediately to `value`.
    #[must_use]
    pub fn new_immediate_awaitable(value: Value) -> Self {
        Self::ImmediateAwaitable(ImmediateAwaitableState { value })
    }

    /// Creates an async-generator facade for `aiter(...)`.
    #[must_use]
    pub fn new_async_generator(iterator: Value) -> Self {
        Self::AsyncGenerator(AsyncGeneratorState { iterator })
    }

    /// Creates an awaitable wrapper for `anext(...)`.
    #[must_use]
    pub fn new_anext_awaitable(iterator: Value, default: Option<Value>) -> Self {
        Self::AnextAwaitable(AnextAwaitableState { iterator, default })
    }

    /// Creates a `decimal.Context` object.
    #[must_use]
    pub fn new_decimal_context(prec: i64, rounding: String, saved: Option<DecimalContextConfig>) -> Self {
        Self::DecimalContext(DecimalContextState { prec, rounding, saved })
    }

    /// Creates an object returned by `Decimal.as_tuple()`.
    #[must_use]
    pub fn new_decimal_tuple(sign: i64, digits: Vec<i64>, exponent: i64) -> Self {
        Self::DecimalTuple(DecimalTupleState { sign, digits, exponent })
    }

    /// Creates a runtime complex number object.
    #[must_use]
    pub fn new_complex(real: f64, imag: f64) -> Self {
        Self::Complex { real, imag }
    }

    /// Returns the context fields when this object is a `decimal.Context`.
    #[must_use]
    pub fn decimal_context_config(&self) -> Option<DecimalContextConfig> {
        let Self::DecimalContext(state) = self else {
            return None;
        };
        Some(DecimalContextConfig {
            prec: state.prec,
            rounding: state.rounding.clone(),
        })
    }

    /// Sets a `decimal.Context` attribute when supported.
    ///
    /// Returns `Some(result)` when this object is a decimal context and the
    /// assignment was handled. Returns `None` for non-context objects.
    pub fn set_decimal_context_attr(
        &mut self,
        attr_name: &str,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<RunResult<()>> {
        let Self::DecimalContext(state) = self else {
            return None;
        };

        let result = match attr_name {
            "prec" => {
                let parsed = value.as_int(heap);
                value.drop_with_heap(heap);
                match parsed {
                    Ok(prec) => {
                        state.prec = prec;
                        decimal_mod::set_current_context_config(DecimalContextConfig {
                            prec: state.prec,
                            rounding: state.rounding.clone(),
                        });
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            }
            "rounding" => {
                state.rounding = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
                decimal_mod::set_current_context_config(DecimalContextConfig {
                    prec: state.prec,
                    rounding: state.rounding.clone(),
                });
                Ok(())
            }
            _ => {
                value.drop_with_heap(heap);
                Err(ExcType::attribute_error_no_setattr(Type::DecimalContext, attr_name))
            }
        };
        Some(result)
    }

    /// Creates a `contextlib.suppress`-style context manager.
    #[must_use]
    pub fn new_suppress_context_manager(suppress_types: Vec<Value>) -> Self {
        Self::ContextManager(ContextManagerState {
            name: "contextlib.suppress".to_string(),
            enter_value: Value::None,
            suppress_types,
        })
    }

    /// Creates a template object.
    #[must_use]
    pub fn new_template(template: String, delimiter: String, idpattern: String) -> Self {
        Self::Template(TemplateState {
            template,
            delimiter,
            idpattern,
        })
    }

    /// Creates a JSON encoder object.
    #[must_use]
    pub fn new_json_encoder(
        indent: Option<usize>,
        sort_keys: bool,
        ensure_ascii: bool,
        skipkeys: bool,
        allow_nan: bool,
        check_circular: bool,
    ) -> Self {
        Self::JsonEncoder(JsonEncoderState {
            indent,
            sort_keys,
            ensure_ascii,
            skipkeys,
            allow_nan,
            check_circular,
        })
    }

    /// Creates a JSON decoder object.
    #[must_use]
    pub fn new_json_decoder(kwargs: crate::modules::json::LoadsKwargs) -> Self {
        let crate::modules::json::LoadsKwargs {
            cls: _cls,
            object_hook,
            parse_float,
            parse_int,
            parse_constant,
            object_pairs_hook,
        } = kwargs;
        Self::JsonDecoder(JsonDecoderState {
            object_hook,
            parse_float,
            parse_int,
            parse_constant,
            object_pairs_hook,
            strict: false,
        })
    }

    /// Creates a `pprint.PrettyPrinter` object.
    #[must_use]
    pub fn new_pretty_printer(params: PprintParams, class_id: HeapId) -> Self {
        Self::PrettyPrinter(PrettyPrinterState { params, class_id })
    }

    /// Creates a `re.RegexFlag` value object.
    #[must_use]
    pub fn new_regex_flag(bits: i64) -> Self {
        Self::RegexFlagValue(bits)
    }

    /// Creates the `re.RegexFlag` namespace object.
    #[must_use]
    pub fn new_regex_flag_enum() -> Self {
        Self::RegexFlagEnum
    }

    /// Creates a scanner with no rules (used by `Pattern.scanner` type parity).
    #[must_use]
    pub fn new_empty_re_scanner() -> Self {
        Self::ReScanner(ReScannerState { rules: Vec::new() })
    }

    /// Creates a scanner with explicit rules.
    #[must_use]
    pub fn new_re_scanner(rules: Vec<ReScannerRule>) -> Self {
        Self::ReScanner(ReScannerState { rules })
    }

    /// Calls methods on a regex scanner object.
    fn call_re_scanner_method(
        state: &mut ReScannerState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        name: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        if name != "scan" {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(Type::SreScanner, name));
        }

        let input = args.get_one_arg("re.Scanner.scan", heap)?;
        let text = input.py_str(heap, interns).into_owned();
        input.drop_with_heap(heap);

        let mut index = 0usize;
        let mut out_items: Vec<Value> = Vec::new();

        while index < text.len() {
            let slice = &text[index..];
            let mut matched_rule = false;

            for rule in &state.rules {
                let Ok(regex) = Regex::new(&format!("^(?:{})", rule.pattern)) else {
                    continue;
                };
                let Ok(captures_opt) = regex.captures(slice) else {
                    continue;
                };
                let Some(captures) = captures_opt else {
                    continue;
                };
                let Some(matched) = captures.get(0) else {
                    continue;
                };
                if matched.start() != 0 {
                    continue;
                }

                let token = matched.as_str();
                matched_rule = true;

                if let Some(tag) = &rule.tag {
                    let tag_id = heap.allocate(HeapData::Str(Str::from(tag.as_str())))?;
                    let token_id = heap.allocate(HeapData::Str(Str::from(token)))?;
                    let tuple = allocate_tuple(smallvec::smallvec![Value::Ref(tag_id), Value::Ref(token_id)], heap)?;
                    out_items.push(tuple);
                }

                if token.is_empty() {
                    // Avoid infinite loops on zero-width scanner rules.
                    let next = slice.char_indices().nth(1).map_or(text.len(), |(off, _)| index + off);
                    index = next;
                } else {
                    index += token.len();
                }
                break;
            }

            if !matched_rule {
                break;
            }
        }

        let remainder_id = heap.allocate(HeapData::Str(Str::from(&text[index..])))?;
        let list_id = heap.allocate(HeapData::List(List::new(out_items)))?;
        Ok(allocate_tuple(
            smallvec::smallvec![Value::Ref(list_id), Value::Ref(remainder_id)],
            heap,
        )?)
    }

    /// Creates a StringIO object.
    #[must_use]
    pub fn new_string_io(initial_value: String, newline: String) -> Self {
        Self::StringIO(StringIOState {
            buffer: initial_value,
            position: 0,
            closed: false,
            newline,
            newlines_translated: None,
        })
    }

    /// Creates a BytesIO object.
    #[must_use]
    pub fn new_bytes_io(initial_bytes: Vec<u8>) -> Self {
        Self::BytesIO(BytesIOState {
            buffer: initial_bytes,
            position: 0,
            closed: false,
            exported_buffers: Vec::new(),
        })
    }

    /// Creates a `struct.Struct` object.
    #[must_use]
    pub fn new_struct(format: String, size: usize) -> Self {
        Self::Struct(StructState { format, size })
    }

    /// Creates a `csv.reader` object.
    #[must_use]
    pub fn new_csv_reader(rows: Vec<Vec<CsvParsedField>>, dialect: CsvDialect) -> Self {
        Self::CsvReader(CsvReaderState {
            rows,
            index: 0,
            dialect,
        })
    }

    /// Creates a `csv.writer` object.
    #[must_use]
    pub fn new_csv_writer(file_obj: Value, dialect: CsvDialect) -> Self {
        Self::CsvWriter(CsvWriterState { file_obj, dialect })
    }

    /// Creates a `csv.DictReader` object.
    #[must_use]
    pub fn new_csv_dict_reader(
        rows: Vec<Vec<String>>,
        fieldnames: Vec<String>,
        restkey: Option<String>,
        restval: Value,
    ) -> Self {
        Self::CsvDictReader(CsvDictReaderState {
            rows,
            fieldnames,
            restkey,
            restval,
            index: 0,
        })
    }

    /// Creates a `csv.DictWriter` object.
    #[must_use]
    pub fn new_csv_dict_writer(
        file_obj: Value,
        fieldnames: Vec<String>,
        restval: Value,
        extrasaction: String,
        dialect: CsvDialect,
    ) -> Self {
        Self::CsvDictWriter(CsvDictWriterState {
            file_obj,
            fieldnames,
            restval,
            extrasaction,
            dialect,
        })
    }

    /// Creates a CSV dialect object.
    #[must_use]
    pub fn new_csv_dialect(dialect: CsvDialect) -> Self {
        Self::CsvDialect(dialect)
    }

    /// Creates a `csv.Sniffer` object.
    #[must_use]
    pub fn new_csv_sniffer() -> Self {
        Self::CsvSniffer
    }

    /// Returns whether this object currently stores heap references.
    #[must_use]
    pub fn has_refs(&self) -> bool {
        match self {
            Self::ExitStack(state) => state.callbacks.iter().any(|callback| match callback {
                ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => matches!(value, Value::Ref(_)),
                ExitCallback::Callback { func, args, kwargs } => {
                    matches!(func, Value::Ref(_))
                        || args.iter().any(|arg| matches!(arg, Value::Ref(_)))
                        || kwargs
                            .iter()
                            .any(|(key, value)| matches!(key, Value::Ref(_)) || matches!(value, Value::Ref(_)))
                }
            }),
            Self::ContextManager(state) => {
                matches!(&state.enter_value, Value::Ref(_))
                    || state.suppress_types.iter().any(|value| matches!(value, Value::Ref(_)))
            }
            Self::GeneratorContextManagerFactory(state) => matches!(state.func, Value::Ref(_)),
            Self::GeneratorContextManager(state) => matches!(state.generator, Value::Ref(_)),
            Self::GeneratorContextDecorator(state) => {
                matches!(state.generator, Value::Ref(_)) || matches!(state.wrapped, Value::Ref(_))
            }
            Self::InstanceContextDecorator(state) => {
                matches!(state.manager, Value::Ref(_)) || matches!(state.wrapped, Value::Ref(_))
            }
            Self::GeneratorNextCallable(state) => matches!(state.generator, Value::Ref(_)),
            Self::GeneratorNextDefaultCallable(state) => matches!(state.generator, Value::Ref(_)),
            Self::GeneratorCloseCallable(state) => matches!(state.generator, Value::Ref(_)),
            Self::ImmediateAwaitable(state) => matches!(state.value, Value::Ref(_)),
            Self::AsyncGenerator(state) => matches!(state.iterator, Value::Ref(_)),
            Self::AnextAwaitable(state) => {
                matches!(state.iterator, Value::Ref(_))
                    || state
                        .default
                        .as_ref()
                        .is_some_and(|value| matches!(value, Value::Ref(_)))
            }
            Self::Formatter
            | Self::Template(_)
            | Self::JsonEncoder(_)
            | Self::JsonDecoder(_)
            | Self::StringIO(_)
            | Self::BytesIO(_)
            | Self::Struct(_)
            | Self::DecimalContext(_)
            | Self::DecimalTuple(_)
            | Self::Complex { .. }
            | Self::CsvReader(_)
            | Self::CsvDialect(_)
            | Self::CsvSniffer
            | Self::RegexFlagValue(_)
            | Self::RegexFlagEnum
            | Self::ReScanner(_) => false,
            Self::CsvWriter(state) => matches!(state.file_obj, Value::Ref(_)),
            Self::CsvDictReader(state) => matches!(state.restval, Value::Ref(_)),
            Self::CsvDictWriter(state) => {
                matches!(state.file_obj, Value::Ref(_)) || matches!(state.restval, Value::Ref(_))
            }
            Self::PrettyPrinter(_) => true,
        }
    }

    /// Appends referenced heap IDs to a work list for GC traversal.
    pub fn collect_ref_ids(&self, work_list: &mut Vec<HeapId>) {
        if let Self::ExitStack(state) = self {
            for callback in &state.callbacks {
                match callback {
                    ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => {
                        if let Value::Ref(id) = value {
                            work_list.push(*id);
                        }
                    }
                    ExitCallback::Callback { func, args, kwargs } => {
                        if let Value::Ref(id) = func {
                            work_list.push(*id);
                        }
                        for arg in args {
                            if let Value::Ref(id) = arg {
                                work_list.push(*id);
                            }
                        }
                        for (key, value) in kwargs {
                            if let Value::Ref(id) = key {
                                work_list.push(*id);
                            }
                            if let Value::Ref(id) = value {
                                work_list.push(*id);
                            }
                        }
                    }
                }
            }
        }
        if let Self::ContextManager(state) = self {
            if let Value::Ref(id) = &state.enter_value {
                work_list.push(*id);
            }
            for value in &state.suppress_types {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        if let Self::GeneratorContextManagerFactory(state) = self
            && let Value::Ref(id) = state.func
        {
            work_list.push(id);
        }
        if let Self::GeneratorContextManager(state) = self
            && let Value::Ref(id) = state.generator
        {
            work_list.push(id);
        }
        if let Self::GeneratorContextDecorator(state) = self {
            if let Value::Ref(id) = state.generator {
                work_list.push(id);
            }
            if let Value::Ref(id) = state.wrapped {
                work_list.push(id);
            }
        }
        if let Self::InstanceContextDecorator(state) = self {
            if let Value::Ref(id) = state.manager {
                work_list.push(id);
            }
            if let Value::Ref(id) = state.wrapped {
                work_list.push(id);
            }
        }
        if let Self::GeneratorNextCallable(state) = self
            && let Value::Ref(id) = state.generator
        {
            work_list.push(id);
        }
        if let Self::GeneratorNextDefaultCallable(state) = self
            && let Value::Ref(id) = state.generator
        {
            work_list.push(id);
        }
        if let Self::GeneratorCloseCallable(state) = self
            && let Value::Ref(id) = state.generator
        {
            work_list.push(id);
        }
        if let Self::ImmediateAwaitable(state) = self
            && let Value::Ref(id) = state.value
        {
            work_list.push(id);
        }
        if let Self::AsyncGenerator(state) = self
            && let Value::Ref(id) = state.iterator
        {
            work_list.push(id);
        }
        if let Self::AnextAwaitable(state) = self {
            if let Value::Ref(id) = state.iterator {
                work_list.push(id);
            }
            if let Some(Value::Ref(id)) = &state.default {
                work_list.push(*id);
            }
        }
        if let Self::PrettyPrinter(state) = self {
            work_list.push(state.class_id);
        }
        if let Self::CsvWriter(state) = self
            && let Value::Ref(id) = state.file_obj
        {
            work_list.push(id);
        }
        if let Self::CsvDictReader(state) = self
            && let Value::Ref(id) = state.restval
        {
            work_list.push(id);
        }
        if let Self::CsvDictWriter(state) = self {
            if let Value::Ref(id) = state.file_obj {
                work_list.push(id);
            }
            if let Value::Ref(id) = state.restval {
                work_list.push(id);
            }
        }
    }

    /// Handles `contextlib.ExitStack` and `contextlib.AsyncExitStack` methods.
    fn call_exit_stack_method(
        state: &mut ExitStackState,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "push" => {
                let callback = args.get_one_arg("ExitStack.push", heap)?;
                state
                    .callbacks
                    .push(ExitCallback::ExitFunc(callback.clone_with_heap(heap)));
                Ok(callback)
            }
            "enter_context" => {
                let manager = args.get_one_arg("ExitStack.enter_context", heap)?;
                state
                    .callbacks
                    .push(ExitCallback::ExitMethod(manager.clone_with_heap(heap)));
                Ok(manager)
            }
            "callback" => {
                let (mut positional, kwargs) = args.into_parts();
                let Some(func) = positional.next() else {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error_at_least("ExitStack.callback", 1, 0));
                };
                let args_vec: Vec<Value> = positional.collect();
                let kwargs_vec: Vec<(Value, Value)> = kwargs.into_iter().collect();
                state.callbacks.push(ExitCallback::Callback {
                    func: func.clone_with_heap(heap),
                    args: args_vec,
                    kwargs: kwargs_vec,
                });
                Ok(func)
            }
            "pop_all" => {
                args.check_zero_args("ExitStack.pop_all", heap)?;
                let object = Self::new_exit_stack(state.async_mode);
                let id = heap.allocate(HeapData::StdlibObject(object))?;
                let HeapData::StdlibObject(Self::ExitStack(new_state)) = heap.get_mut(id) else {
                    unreachable!("allocated ExitStack must be a StdlibObject::ExitStack");
                };
                new_state.self_id = Some(id);
                new_state.callbacks = std::mem::take(&mut state.callbacks);
                Ok(Value::Ref(id))
            }
            "close" => {
                for callback in state.callbacks.drain(..) {
                    match callback {
                        ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.drop_with_heap(heap),
                        ExitCallback::Callback { func, args, kwargs } => {
                            func.drop_with_heap(heap);
                            args.drop_with_heap(heap);
                            for (key, value) in kwargs {
                                key.drop_with_heap(heap);
                                value.drop_with_heap(heap);
                            }
                        }
                    }
                }
                Ok(Value::None)
            }
            "aclose" if state.async_mode => {
                for callback in state.callbacks.drain(..) {
                    match callback {
                        ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.drop_with_heap(heap),
                        ExitCallback::Callback { func, args, kwargs } => {
                            func.drop_with_heap(heap);
                            args.drop_with_heap(heap);
                            for (key, value) in kwargs {
                                key.drop_with_heap(heap);
                                value.drop_with_heap(heap);
                            }
                        }
                    }
                }
                let awaitable = Self::new_immediate_awaitable(Value::None);
                let awaitable_id = heap.allocate(HeapData::StdlibObject(awaitable))?;
                Ok(Value::Ref(awaitable_id))
            }
            "__enter__" => {
                args.check_zero_args("ExitStack.__enter__", heap)?;
                let Some(self_id) = state.self_id else {
                    return Err(
                        SimpleException::new_msg(ExcType::RuntimeError, "internal ExitStack state error").into(),
                    );
                };
                Ok(Value::Ref(self_id).clone_with_heap(heap))
            }
            "__aenter__" if state.async_mode => {
                args.check_zero_args("AsyncExitStack.__aenter__", heap)?;
                let Some(self_id) = state.self_id else {
                    return Err(
                        SimpleException::new_msg(ExcType::RuntimeError, "internal ExitStack state error").into(),
                    );
                };
                let awaitable = Self::new_immediate_awaitable(Value::Ref(self_id).clone_with_heap(heap));
                let awaitable_id = heap.allocate(HeapData::StdlibObject(awaitable))?;
                Ok(Value::Ref(awaitable_id))
            }
            "__exit__" => {
                let (positional, kwargs) = args.into_parts();
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                for callback in state.callbacks.drain(..) {
                    match callback {
                        ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.drop_with_heap(heap),
                        ExitCallback::Callback { func, args, kwargs } => {
                            func.drop_with_heap(heap);
                            args.drop_with_heap(heap);
                            for (key, value) in kwargs {
                                key.drop_with_heap(heap);
                                value.drop_with_heap(heap);
                            }
                        }
                    }
                }
                Ok(Value::Bool(false))
            }
            "__aexit__" if state.async_mode => {
                let (positional, kwargs) = args.into_parts();
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                for callback in state.callbacks.drain(..) {
                    match callback {
                        ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.drop_with_heap(heap),
                        ExitCallback::Callback { func, args, kwargs } => {
                            func.drop_with_heap(heap);
                            args.drop_with_heap(heap);
                            for (key, value) in kwargs {
                                key.drop_with_heap(heap);
                                value.drop_with_heap(heap);
                            }
                        }
                    }
                }
                let awaitable = Self::new_immediate_awaitable(Value::Bool(false));
                let awaitable_id = heap.allocate(HeapData::StdlibObject(awaitable))?;
                Ok(Value::Ref(awaitable_id))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("ExitStack", attr))
            }
        }
    }

    /// Handles simple context manager wrappers (`suppress`, `redirect_*`, etc.).
    fn call_context_manager_method(
        state: &mut ContextManagerState,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "__enter__" => {
                args.check_zero_args(attr, heap)?;
                if state.name == "contextlib.redirect_stdout" {
                    match &state.enter_value {
                        Value::None => crate::io::push_stdout_redirect(RedirectTarget::Sink),
                        Value::Ref(id) => {
                            heap.inc_ref(*id);
                            crate::io::push_stdout_redirect(RedirectTarget::Heap(*id));
                        }
                        _ => crate::io::push_stdout_redirect(RedirectTarget::Sink),
                    }
                    Ok(state.enter_value.clone_with_heap(heap))
                } else if state.name == "contextlib.redirect_stderr" {
                    match &state.enter_value {
                        Value::None => crate::io::push_stderr_redirect(RedirectTarget::Sink),
                        Value::Ref(id) => {
                            heap.inc_ref(*id);
                            crate::io::push_stderr_redirect(RedirectTarget::Heap(*id));
                        }
                        _ => crate::io::push_stderr_redirect(RedirectTarget::Sink),
                    }
                    Ok(state.enter_value.clone_with_heap(heap))
                } else if state.name == "contextlib.chdir" {
                    let cwd = match &state.enter_value {
                        Value::Ref(id) => match heap.get(*id) {
                            HeapData::Str(s) => s.as_str().to_owned(),
                            _ => "/".to_owned(),
                        },
                        _ => "/".to_owned(),
                    };
                    os_mod::push_working_dir(cwd);
                    Ok(Value::None)
                } else {
                    Ok(state.enter_value.clone_with_heap(heap))
                }
            }
            "__aenter__" => {
                args.check_zero_args(attr, heap)?;
                let enter_value = if state.name == "contextlib.redirect_stdout" {
                    match &state.enter_value {
                        Value::None => crate::io::push_stdout_redirect(RedirectTarget::Sink),
                        Value::Ref(id) => {
                            heap.inc_ref(*id);
                            crate::io::push_stdout_redirect(RedirectTarget::Heap(*id));
                        }
                        _ => crate::io::push_stdout_redirect(RedirectTarget::Sink),
                    }
                    state.enter_value.clone_with_heap(heap)
                } else if state.name == "contextlib.redirect_stderr" {
                    match &state.enter_value {
                        Value::None => crate::io::push_stderr_redirect(RedirectTarget::Sink),
                        Value::Ref(id) => {
                            heap.inc_ref(*id);
                            crate::io::push_stderr_redirect(RedirectTarget::Heap(*id));
                        }
                        _ => crate::io::push_stderr_redirect(RedirectTarget::Sink),
                    }
                    state.enter_value.clone_with_heap(heap)
                } else if state.name == "contextlib.chdir" {
                    let cwd = match &state.enter_value {
                        Value::Ref(id) => match heap.get(*id) {
                            HeapData::Str(s) => s.as_str().to_owned(),
                            _ => "/".to_owned(),
                        },
                        _ => "/".to_owned(),
                    };
                    os_mod::push_working_dir(cwd);
                    Value::None
                } else {
                    state.enter_value.clone_with_heap(heap)
                };
                let awaitable = Self::new_immediate_awaitable(enter_value);
                let awaitable_id = heap.allocate(HeapData::StdlibObject(awaitable))?;
                Ok(Value::Ref(awaitable_id))
            }
            "__exit__" | "__aexit__" => {
                let (mut positional, kwargs) = args.into_parts();
                let exc_type = positional.next();
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);

                let should_suppress = if state.suppress_types.is_empty() {
                    false
                } else if let Some(exc_type_value) = exc_type.as_ref().filter(|value| !matches!(value, Value::None)) {
                    Self::context_manager_should_suppress(exc_type_value, &state.suppress_types, heap)?
                } else {
                    false
                };

                if let Some(exc_type) = exc_type {
                    exc_type.drop_with_heap(heap);
                }
                if state.name == "contextlib.redirect_stdout" {
                    if let Some(RedirectTarget::Heap(id)) = crate::io::pop_stdout_redirect() {
                        heap.dec_ref(id);
                    }
                } else if state.name == "contextlib.redirect_stderr" {
                    if let Some(RedirectTarget::Heap(id)) = crate::io::pop_stderr_redirect() {
                        heap.dec_ref(id);
                    }
                } else if state.name == "contextlib.chdir" {
                    os_mod::pop_working_dir();
                }
                if attr == "__aexit__" {
                    let awaitable = Self::new_immediate_awaitable(Value::Bool(should_suppress));
                    let awaitable_id = heap.allocate(HeapData::StdlibObject(awaitable))?;
                    Ok(Value::Ref(awaitable_id))
                } else {
                    Ok(Value::Bool(should_suppress))
                }
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(state.name.as_str(), attr))
            }
        }
    }

    /// Handles generator-backed context manager methods from `@contextmanager`.
    fn call_generator_context_manager_method(
        state: &mut GeneratorContextManagerState,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<AttrCallResult> {
        match attr {
            "__enter__" | "__aenter__" => {
                args.check_zero_args(attr, heap)?;
                let next_callable = Self::new_generator_next_callable(state.generator.clone_with_heap(heap));
                let next_id = heap.allocate(HeapData::StdlibObject(next_callable))?;
                Ok(AttrCallResult::CallFunction(Value::Ref(next_id), ArgValues::Empty))
            }
            "__exit__" | "__aexit__" => {
                let (exc_type, exc, traceback) = args.get_three_args(attr, heap)?;
                let has_exception = !matches!(exc_type, Value::None);
                exc_type.drop_with_heap(heap);
                exc.drop_with_heap(heap);
                traceback.drop_with_heap(heap);
                if has_exception {
                    let close_callable = Self::new_generator_close_callable(state.generator.clone_with_heap(heap));
                    let close_id = heap.allocate(HeapData::StdlibObject(close_callable))?;
                    Ok(AttrCallResult::CallFunction(Value::Ref(close_id), ArgValues::Empty))
                } else {
                    Ok(AttrCallResult::CallFunction(
                        Value::Builtin(Builtins::Type(Type::List)),
                        ArgValues::One(state.generator.clone_with_heap(heap)),
                    ))
                }
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._GeneratorContextManager", attr))
            }
        }
    }

    /// Handles `decimal.Context` methods used by parity tests.
    fn call_decimal_context_method(
        state: &mut DecimalContextState,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
        attr: &str,
        args: ArgValues,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr {
            "copy" => {
                args.check_zero_args("Context.copy", heap)?;
                let context = Self::new_decimal_context(state.prec, state.rounding.clone(), None);
                let id = heap.allocate(HeapData::StdlibObject(context))?;
                Ok(Value::Ref(id))
            }
            "__enter__" => {
                args.check_zero_args("Context.__enter__", heap)?;
                if state.saved.is_none() {
                    state.saved = Some(decimal_mod::get_current_context_config());
                }
                decimal_mod::set_current_context_config(DecimalContextConfig {
                    prec: state.prec,
                    rounding: state.rounding.clone(),
                });
                if let Some(self_id) = self_id {
                    heap.inc_ref(self_id);
                    Ok(Value::Ref(self_id))
                } else {
                    let context = Self::new_decimal_context(state.prec, state.rounding.clone(), state.saved.clone());
                    let id = heap.allocate(HeapData::StdlibObject(context))?;
                    Ok(Value::Ref(id))
                }
            }
            "__exit__" => {
                let (exc_type, exc, traceback) = args.get_three_args("Context.__exit__", heap)?;
                exc_type.drop_with_heap(heap);
                exc.drop_with_heap(heap);
                traceback.drop_with_heap(heap);
                if let Some(saved) = state.saved.take() {
                    decimal_mod::set_current_context_config(saved);
                }
                Ok(Value::Bool(false))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(Type::DecimalContext, attr))
            }
        }
    }

    /// Handles methods on runtime `complex` values.
    fn call_complex_method(
        real: f64,
        imag: f64,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        if attr == "conjugate" {
            args.check_zero_args("complex.conjugate", heap)?;
            let id = heap.allocate(HeapData::StdlibObject(Self::new_complex(real, -imag)))?;
            Ok(Value::Ref(id))
        } else {
            args.drop_with_heap(heap);
            Err(ExcType::attribute_error(Type::Complex, attr))
        }
    }

    /// Returns whether `contextlib.suppress` should suppress `exc_type`.
    ///
    /// Supports handler tuples and the VM's exception class representation
    /// (`Builtins::ExcType`) used by `WITH_EXCEPT_SETUP`.
    fn context_manager_should_suppress(
        exc_type: &Value,
        handled_types: &[Value],
        heap: &Heap<impl ResourceTracker>,
    ) -> RunResult<bool> {
        for handled_type in handled_types {
            if Self::exception_type_matches(exc_type, handled_type, heap)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Checks one suppress handler entry against a raised exception class.
    ///
    /// Handler values follow `except`-style rules: an exception class or a
    /// tuple of exception classes. Invalid handler values raise a TypeError.
    fn exception_type_matches(
        exc_type: &Value,
        handled_type: &Value,
        heap: &Heap<impl ResourceTracker>,
    ) -> RunResult<bool> {
        match handled_type {
            Value::Builtin(Builtins::ExcType(handler_type)) => match exc_type {
                Value::Builtin(Builtins::ExcType(exc_type)) => Ok(exc_type.is_subclass_of(*handler_type)),
                Value::Builtin(Builtins::Type(Type::Exception(exc_type))) => Ok(exc_type.is_subclass_of(*handler_type)),
                _ => Ok(false),
            },
            Value::Ref(id) => {
                let HeapData::Tuple(tuple) = heap.get(*id) else {
                    return Err(ExcType::except_invalid_type_error());
                };
                for nested in tuple.as_vec() {
                    if Self::exception_type_matches(exc_type, nested, heap)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Err(ExcType::except_invalid_type_error()),
        }
    }

    /// Handles `string.Formatter` methods.
    fn call_formatter_method(
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "format" => formatter_format(heap, interns, args),
            "vformat" => formatter_vformat(heap, interns, args),
            "parse" => formatter_parse(heap, interns, args),
            "get_value" => formatter_get_value(heap, interns, args),
            "get_field" => formatter_get_field(heap, interns, args),
            "format_field" => formatter_format_field(heap, interns, args),
            "convert_field" => formatter_convert_field(heap, interns, args),
            "check_unused_args" => formatter_check_unused_args(heap, args),
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("Formatter", attr))
            }
        }
    }

    /// Handles `string.Template` methods.
    fn call_template_method(
        state: &TemplateState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "substitute" => template_substitute(state, heap, interns, args, false),
            "safe_substitute" => template_substitute(state, heap, interns, args, true),
            "get_identifiers" => template_get_identifiers(state, heap, args),
            "is_valid" => template_is_valid(state, heap, args),
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("Template", attr))
            }
        }
    }

    /// Handles `json.JSONEncoder` methods.
    fn call_json_encoder_method(
        state: &JsonEncoderState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "encode" => {
                let value = args.get_one_arg("JSONEncoder.encode", heap)?;
                let indent = state.indent.map(|level| " ".repeat(level));
                let (item_separator, key_separator) = if state.indent.is_some() {
                    (",".to_string(), ": ".to_string())
                } else {
                    (", ".to_string(), ": ".to_string())
                };
                let encoded = match crate::modules::json::serialize_json_value(
                    &value,
                    heap,
                    interns,
                    indent.as_deref(),
                    state.sort_keys,
                    state.ensure_ascii,
                    state.skipkeys,
                    state.allow_nan,
                    state.check_circular,
                    &item_separator,
                    &key_separator,
                    false,
                    false,
                ) {
                    Ok(encoded) => encoded,
                    Err(err) => {
                        value.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                value.drop_with_heap(heap);
                let id = heap.allocate(HeapData::Str(Str::from(encoded)))?;
                Ok(Value::Ref(id))
            }
            "iterencode" => {
                let value = args.get_one_arg("JSONEncoder.iterencode", heap)?;
                let indent = state.indent.map(|level| " ".repeat(level));
                let (item_separator, key_separator) = if state.indent.is_some() {
                    (",".to_string(), ": ".to_string())
                } else {
                    (", ".to_string(), ": ".to_string())
                };
                let encoded = match crate::modules::json::serialize_json_value(
                    &value,
                    heap,
                    interns,
                    indent.as_deref(),
                    state.sort_keys,
                    state.ensure_ascii,
                    state.skipkeys,
                    state.allow_nan,
                    state.check_circular,
                    &item_separator,
                    &key_separator,
                    false,
                    false,
                ) {
                    Ok(encoded) => encoded,
                    Err(err) => {
                        value.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                value.drop_with_heap(heap);
                let str_id = heap.allocate(HeapData::Str(Str::from(encoded)))?;
                let list_id = heap.allocate(HeapData::List(List::new(vec![Value::Ref(str_id)])))?;
                Ok(Value::Ref(list_id))
            }
            "default" => {
                let value = args.get_one_arg("JSONEncoder.default", heap)?;
                let type_name = value.py_type(heap).to_string();
                value.drop_with_heap(heap);
                Err(ExcType::type_error(format!(
                    "Object of type {type_name} is not JSON serializable"
                )))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("JSONEncoder", attr))
            }
        }
    }

    /// Handles `json.JSONDecoder` methods.
    fn call_json_decoder_method(
        state: &JsonDecoderState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "decode" => {
                let input = args.get_one_arg("JSONDecoder.decode", heap)?;
                let json_str = input.py_str(heap, interns).into_owned();
                input.drop_with_heap(heap);
                let options = crate::modules::json::JsonDecodeOptions {
                    object_hook: state.object_hook.as_ref(),
                    parse_float: state.parse_float.as_ref(),
                    parse_int: state.parse_int.as_ref(),
                    parse_constant: state.parse_constant.as_ref(),
                    object_pairs_hook: state.object_pairs_hook.as_ref(),
                };
                crate::modules::json::parse_json_str_with_options(&json_str, options, heap, interns)
            }
            "raw_decode" => {
                let input = args.get_one_arg("JSONDecoder.raw_decode", heap)?;
                let json_str = input.py_str(heap, interns).into_owned();
                input.drop_with_heap(heap);

                let deserializer = serde_json::Deserializer::from_str(&json_str);
                let mut stream = deserializer.into_iter::<serde_json::Value>();
                let json_value = stream
                    .next()
                    .transpose()
                    .map_err(|e| SimpleException::new_msg(ExcType::ValueError, format!("json.JSONDecodeError: {e}")))?
                    .ok_or_else(|| {
                        SimpleException::new_msg(ExcType::ValueError, "json.JSONDecodeError: empty input")
                    })?;
                let consumed = stream.byte_offset();

                let value = crate::modules::json::convert_json_to_python(&json_value, heap, interns)?;
                #[expect(clippy::cast_possible_wrap)]
                let tuple = allocate_tuple(smallvec::smallvec![value, Value::Int(consumed as i64)], heap)?;
                Ok(tuple)
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("JSONDecoder", attr))
            }
        }
    }

    /// Handles `pprint.PrettyPrinter` instance methods.
    fn call_pretty_printer_method(
        state: &PrettyPrinterState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "pformat" => {
                let obj = args.get_one_arg("PrettyPrinter.pformat", heap)?;
                let formatted = match pprint_mod::format_object(heap, interns, &obj, &state.params) {
                    Ok(formatted) => formatted,
                    Err(err) => {
                        obj.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                obj.drop_with_heap(heap);
                let id = heap.allocate(HeapData::Str(Str::from(formatted)))?;
                Ok(Value::Ref(id))
            }
            "pprint" => {
                let obj = args.get_one_arg("PrettyPrinter.pprint", heap)?;
                let _formatted = match pprint_mod::format_object(heap, interns, &obj, &state.params) {
                    Ok(formatted) => formatted,
                    Err(err) => {
                        obj.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                obj.drop_with_heap(heap);
                Ok(Value::None)
            }
            "isreadable" => {
                let obj = args.get_one_arg("PrettyPrinter.isreadable", heap)?;
                let readable = pprint_mod::object_is_readable(heap, &obj)?;
                obj.drop_with_heap(heap);
                Ok(Value::Bool(readable))
            }
            "isrecursive" => {
                let obj = args.get_one_arg("PrettyPrinter.isrecursive", heap)?;
                let recursive = pprint_mod::object_is_recursive(heap, &obj)?;
                obj.drop_with_heap(heap);
                Ok(Value::Bool(recursive))
            }
            "format" => {
                let (mut positional, kwargs) = args.into_parts();
                let arg_count = positional.len();
                let kwargs_len = kwargs.len();
                if arg_count != 4 || kwargs_len != 0 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    let total = arg_count + kwargs_len;
                    return Err(ExcType::type_error_arg_count("PrettyPrinter.format", 4, total));
                }

                let obj = positional.next().expect("length checked above");
                let context = positional.next().expect("length checked above");
                let maxlevels = positional.next().expect("length checked above");
                let level = positional.next().expect("length checked above");
                context.drop_with_heap(heap);
                maxlevels.drop_with_heap(heap);
                level.drop_with_heap(heap);

                let formatted = match pprint_mod::format_object(heap, interns, &obj, &state.params) {
                    Ok(formatted) => formatted,
                    Err(err) => {
                        obj.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                let readable = pprint_mod::object_is_readable(heap, &obj)?;
                let recursive = pprint_mod::object_is_recursive(heap, &obj)?;
                obj.drop_with_heap(heap);

                let str_id = heap.allocate(HeapData::Str(Str::from(formatted)))?;
                let tuple = allocate_tuple(
                    smallvec::smallvec![Value::Ref(str_id), Value::Bool(readable), Value::Bool(recursive)],
                    heap,
                )?;
                Ok(tuple)
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("PrettyPrinter", attr))
            }
        }
    }

    /// Handles `struct.Struct` methods by forwarding to module implementations.
    fn call_struct_method(
        state: &StructState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        let function = match attr {
            "pack" => StructFunctions::Pack,
            "unpack" => StructFunctions::Unpack,
            "iter_unpack" => StructFunctions::IterUnpack,
            "pack_into" => StructFunctions::PackInto,
            "unpack_from" => StructFunctions::UnpackFrom,
            _ => {
                args.drop_with_heap(heap);
                return Err(ExcType::attribute_error(Type::Struct, attr));
            }
        };
        let forwarded_args = Self::prepend_struct_format_arg(&state.format, args, heap)?;
        let result = ModuleFunctions::Struct(function).call(heap, interns, forwarded_args)?;
        match result {
            AttrCallResult::Value(value) => Ok(value),
            _ => Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "unexpected non-value result from struct method".to_string(),
            )
            .into()),
        }
    }

    /// Handles `csv.reader` iterator methods.
    fn call_csv_reader_method(
        state: &mut CsvReaderState,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &str,
        args: ArgValues,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr {
            "__iter__" => {
                args.check_zero_args("csv.reader.__iter__", heap)?;
                let Some(this_id) = self_id else {
                    return Err(SimpleException::new_msg(ExcType::RuntimeError, "missing csv.reader self").into());
                };
                Ok(Value::Ref(this_id).clone_with_heap(heap))
            }
            "__next__" => {
                args.check_zero_args("csv.reader.__next__", heap)?;
                let Some(row) = state.rows.get(state.index) else {
                    return Err(ExcType::stop_iteration());
                };
                state.index += 1;

                let mut values = Vec::with_capacity(row.len());
                for field in row {
                    if state.dialect.quoting == QUOTE_NONNUMERIC && !field.quoted && !field.text.is_empty() {
                        if let Ok(number) = field.text.parse::<f64>() {
                            values.push(Value::Float(number));
                        } else {
                            let id = heap.allocate(HeapData::Str(Str::from(field.text.as_str())))?;
                            values.push(Value::Ref(id));
                        }
                    } else {
                        let id = heap.allocate(HeapData::Str(Str::from(field.text.as_str())))?;
                        values.push(Value::Ref(id));
                    }
                }

                let list_id = heap.allocate(HeapData::List(List::new(values)))?;
                Ok(Value::Ref(list_id))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("reader", attr))
            }
        }
    }

    /// Handles `csv.writer` methods.
    fn call_csv_writer_method(
        state: &mut CsvWriterState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "writerow" => {
                let row = args.get_one_arg("writer.writerow", heap)?;
                let result = csv_writer_write_row(&state.file_obj, &state.dialect, &row, heap, interns);
                row.drop_with_heap(heap);
                result
            }
            "writerows" => {
                let rows = args.get_one_arg("writer.writerows", heap)?;
                let mut iter = OurosIter::new(rows, heap, interns)?;
                while let Some(row) = iter.for_next(heap, interns)? {
                    let result = csv_writer_write_row(&state.file_obj, &state.dialect, &row, heap, interns)?;
                    result.drop_with_heap(heap);
                    row.drop_with_heap(heap);
                }
                iter.drop_with_heap(heap);
                Ok(Value::None)
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("writer", attr))
            }
        }
    }

    /// Handles `csv.DictReader` iterator methods.
    fn call_csv_dict_reader_method(
        state: &mut CsvDictReaderState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr {
            "__iter__" => {
                args.check_zero_args("DictReader.__iter__", heap)?;
                let Some(this_id) = self_id else {
                    return Err(SimpleException::new_msg(ExcType::RuntimeError, "missing DictReader self").into());
                };
                Ok(Value::Ref(this_id).clone_with_heap(heap))
            }
            "__next__" => {
                args.check_zero_args("DictReader.__next__", heap)?;
                let Some(row) = state.rows.get(state.index) else {
                    return Err(ExcType::stop_iteration());
                };
                state.index += 1;

                let mut dict = Dict::new();
                for (idx, field_name) in state.fieldnames.iter().enumerate() {
                    let key_id = heap.allocate(HeapData::Str(Str::from(field_name.as_str())))?;
                    let value = if let Some(cell) = row.get(idx) {
                        let value_id = heap.allocate(HeapData::Str(Str::from(cell.as_str())))?;
                        Value::Ref(value_id)
                    } else {
                        state.restval.clone_with_heap(heap)
                    };
                    let old_value: Option<Value> = dict.set(Value::Ref(key_id), value, heap, interns)?;
                    if let Some(old) = old_value {
                        old.drop_with_heap(heap);
                    }
                }

                if row.len() > state.fieldnames.len()
                    && let Some(restkey) = &state.restkey
                {
                    let key_id = heap.allocate(HeapData::Str(Str::from(restkey.as_str())))?;
                    let mut extras = Vec::with_capacity(row.len() - state.fieldnames.len());
                    for cell in row.iter().skip(state.fieldnames.len()) {
                        let value_id = heap.allocate(HeapData::Str(Str::from(cell.as_str())))?;
                        extras.push(Value::Ref(value_id));
                    }
                    let list_id = heap.allocate(HeapData::List(List::new(extras)))?;
                    let old_value: Option<Value> = dict.set(Value::Ref(key_id), Value::Ref(list_id), heap, interns)?;
                    if let Some(old) = old_value {
                        old.drop_with_heap(heap);
                    }
                }

                let dict_id = heap.allocate(HeapData::Dict(dict))?;
                Ok(Value::Ref(dict_id))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("DictReader", attr))
            }
        }
    }

    /// Handles `csv.DictWriter` methods.
    fn call_csv_dict_writer_method(
        state: &mut CsvDictWriterState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "writeheader" => {
                args.check_zero_args("DictWriter.writeheader", heap)?;
                csv_write_string_values_to_file(&state.file_obj, &state.dialect, &state.fieldnames, heap, interns)
            }
            "writerow" => {
                let row = args.get_one_arg("DictWriter.writerow", heap)?;
                let result = csv_dict_writerow(state, &row, heap, interns);
                row.drop_with_heap(heap);
                result
            }
            "writerows" => {
                let rows = args.get_one_arg("DictWriter.writerows", heap)?;
                let mut iter = OurosIter::new(rows, heap, interns)?;
                while let Some(row) = iter.for_next(heap, interns)? {
                    let value = csv_dict_writerow(state, &row, heap, interns)?;
                    value.drop_with_heap(heap);
                    row.drop_with_heap(heap);
                }
                iter.drop_with_heap(heap);
                Ok(Value::None)
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("DictWriter", attr))
            }
        }
    }

    /// Handles `csv.Sniffer` methods.
    fn call_csv_sniffer_method(
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        match attr {
            "sniff" => {
                let sample = args.get_one_arg("Sniffer.sniff", heap)?;
                let sample_text = sample.py_str(heap, interns).into_owned();
                sample.drop_with_heap(heap);
                let delimiter = detect_delimiter(&sample_text);
                let mut dialect = crate::modules::csv_mod::excel_dialect();
                dialect.delimiter = delimiter;
                let id = heap.allocate(HeapData::StdlibObject(Self::new_csv_dialect(dialect)))?;
                Ok(Value::Ref(id))
            }
            "has_header" => {
                let sample = args.get_one_arg("Sniffer.has_header", heap)?;
                let sample_text = sample.py_str(heap, interns).into_owned();
                sample.drop_with_heap(heap);
                Ok(Value::Bool(csv_sniffer_has_header(&sample_text)))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("Sniffer", attr))
            }
        }
    }

    /// Prepends a struct format string to a positional argument list.
    fn prepend_struct_format_arg(
        format: &str,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<ArgValues, crate::resource::ResourceError> {
        let (positional, kwargs) = args.into_parts();
        let mut forwarded_args = Vec::new();
        let format_id = heap.allocate(HeapData::Str(Str::from(format)))?;
        forwarded_args.push(Value::Ref(format_id));
        forwarded_args.extend(positional);
        if kwargs.is_empty() {
            match forwarded_args.len() {
                0 => Ok(ArgValues::Empty),
                1 => {
                    let arg = forwarded_args.into_iter().next().expect("len checked");
                    Ok(ArgValues::One(arg))
                }
                2 => {
                    let mut iter = forwarded_args.into_iter();
                    let first = iter.next().expect("len checked");
                    let second = iter.next().expect("len checked");
                    Ok(ArgValues::Two(first, second))
                }
                _ => Ok(ArgValues::ArgsKargs {
                    args: forwarded_args,
                    kwargs,
                }),
            }
        } else {
            Ok(ArgValues::ArgsKargs {
                args: forwarded_args,
                kwargs,
            })
        }
    }

    /// Handles `io.StringIO` methods.
    fn call_string_io_method(
        state: &mut StringIOState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        // Check if stream is closed for most operations
        let check_closed = |state: &StringIOState, _op: &str| -> RunResult<()> {
            if state.closed {
                return Err(
                    SimpleException::new_msg(ExcType::ValueError, "I/O operation on closed file".to_string()).into(),
                );
            }
            Ok(())
        };

        match attr {
            "read" => {
                let size_arg = args.get_zero_one_arg("StringIO.read", heap)?;
                check_closed(state, "read")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None,
                };

                let result = string_io_read(state, size);
                let id = heap.allocate(HeapData::Str(Str::from(result)))?;
                Ok(Value::Ref(id))
            }
            "write" => {
                let value = args.get_one_arg("StringIO.write", heap)?;
                check_closed(state, "write")?;

                let s = match value_to_string(&value, heap, interns) {
                    Ok(s) => s,
                    Err(err) => {
                        value.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                value.drop_with_heap(heap);

                let count = string_io_write(state, &s);
                Ok(Value::Int(count))
            }
            "getvalue" => {
                args.check_zero_args("StringIO.getvalue", heap)?;
                if state.closed {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        "I/O operation on closed file".to_string(),
                    )
                    .into());
                }
                let id = heap.allocate(HeapData::Str(Str::from(state.buffer.clone())))?;
                Ok(Value::Ref(id))
            }
            "tell" => {
                args.check_zero_args("StringIO.tell", heap)?;
                check_closed(state, "tell")?;
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(state.position as i64))
            }
            "seek" => {
                let (pos, whence) = args.get_one_two_args("StringIO.seek", heap)?;
                check_closed(state, "seek")?;

                let pos_val = extract_seek_pos(&pos, heap)?;
                let whence_val = match whence {
                    Some(w) => extract_whence(&w, heap)?,
                    None => 0,
                };

                let new_pos = string_io_seek(state, pos_val, whence_val)?;
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(new_pos as i64))
            }
            "truncate" => {
                let size_arg = args.get_zero_one_arg("StringIO.truncate", heap)?;
                check_closed(state, "truncate")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None, // Defaults to current position
                };

                let new_size = string_io_truncate(state, size);
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(new_size as i64))
            }
            "close" => {
                args.check_zero_args("StringIO.close", heap)?;
                state.closed = true;
                Ok(Value::None)
            }
            "readable" => {
                args.check_zero_args("StringIO.readable", heap)?;
                check_closed(state, "readable")?;
                Ok(Value::Bool(true))
            }
            "writable" => {
                args.check_zero_args("StringIO.writable", heap)?;
                check_closed(state, "writable")?;
                Ok(Value::Bool(true))
            }
            "seekable" => {
                args.check_zero_args("StringIO.seekable", heap)?;
                check_closed(state, "seekable")?;
                Ok(Value::Bool(true))
            }
            "readline" => {
                let size_arg = args.get_zero_one_arg("StringIO.readline", heap)?;
                check_closed(state, "readline")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None,
                };

                let result = string_io_readline(state, size);
                let id = heap.allocate(HeapData::Str(Str::from(result)))?;
                Ok(Value::Ref(id))
            }
            "readlines" => {
                let size_hint = args.get_zero_one_arg("StringIO.readlines", heap)?;
                check_closed(state, "readlines")?;
                let hint = match &size_hint {
                    Some(val) => extract_size(val, heap)?,
                    None => None,
                };
                size_hint.drop_with_heap(heap);

                let lines = string_io_readlines(state, hint);
                let line_values: Vec<Value> = lines
                    .into_iter()
                    .map(|s| Value::Ref(heap.allocate(HeapData::Str(Str::from(s))).expect("allocation failed")))
                    .collect();
                let id = heap.allocate(HeapData::List(List::new(line_values)))?;
                Ok(Value::Ref(id))
            }
            "writelines" => {
                let lines = args.get_one_arg("StringIO.writelines", heap)?;
                check_closed(state, "writelines")?;

                let mut iter = OurosIter::new(lines, heap, interns)?;
                while let Some(item) = iter.for_next(heap, interns)? {
                    let s = match value_to_string(&item, heap, interns) {
                        Ok(s) => s,
                        Err(err) => {
                            item.drop_with_heap(heap);
                            iter.drop_with_heap(heap);
                            return Err(err);
                        }
                    };
                    item.drop_with_heap(heap);
                    string_io_write(state, &s);
                }
                iter.drop_with_heap(heap);
                Ok(Value::None)
            }
            "flush" => {
                args.check_zero_args("StringIO.flush", heap)?;
                check_closed(state, "flush")?;
                Ok(Value::None)
            }
            "__iter__" => {
                args.check_zero_args("StringIO.__iter__", heap)?;
                check_closed(state, "__iter__")?;
                // Return self as iterator
                Ok(Value::None) // Caller will use the object itself
            }
            "__next__" => {
                args.check_zero_args("StringIO.__next__", heap)?;
                check_closed(state, "__next__")?;

                let line = string_io_readline(state, None);
                if line.is_empty() {
                    Err(ExcType::stop_iteration())
                } else {
                    let id = heap.allocate(HeapData::Str(Str::from(line)))?;
                    Ok(Value::Ref(id))
                }
            }
            "__enter__" => {
                args.check_zero_args("StringIO.__enter__", heap)?;
                check_closed(state, "__enter__")?;
                // Return self - handled by caller
                Ok(Value::None)
            }
            "__exit__" => {
                let (positional, kwargs) = args.into_parts();
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                state.closed = true;
                Ok(Value::None)
            }
            "isatty" => {
                args.check_zero_args("StringIO.isatty", heap)?;
                check_closed(state, "isatty")?;
                Ok(Value::Bool(false))
            }
            "fileno" => {
                args.check_zero_args("StringIO.fileno", heap)?;
                check_closed(state, "fileno")?;
                Err(SimpleException::new_msg(ExcType::OSError, "StringIO.fileno() is not supported".to_string()).into())
            }
            "detach" => {
                args.check_zero_args("StringIO.detach", heap)?;
                check_closed(state, "detach")?;
                Err(SimpleException::new_msg(ExcType::OSError, "detach".to_string()).into())
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("StringIO", attr))
            }
        }
    }

    /// Handles `io.BytesIO` methods.
    fn call_bytes_io_method(
        state: &mut BytesIOState,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        attr: &str,
        args: ArgValues,
    ) -> RunResult<Value> {
        // Check if stream is closed for most operations
        let check_closed = |state: &BytesIOState, _op: &str| -> RunResult<()> {
            if state.closed {
                return Err(
                    SimpleException::new_msg(ExcType::ValueError, "I/O operation on closed file.".to_string()).into(),
                );
            }
            Ok(())
        };

        match attr {
            "read" => {
                let size_arg = args.get_zero_one_arg("BytesIO.read", heap)?;
                check_closed(state, "read")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None,
                };

                let result = bytes_io_read(state, size);
                let id = heap.allocate(HeapData::Bytes(Bytes::from(result)))?;
                Ok(Value::Ref(id))
            }
            "read1" => {
                let size_arg = args.get_zero_one_arg("BytesIO.read1", heap)?;
                check_closed(state, "read1")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None,
                };

                let result = bytes_io_read(state, size);
                let id = heap.allocate(HeapData::Bytes(Bytes::from(result)))?;
                Ok(Value::Ref(id))
            }
            "write" => {
                let value = args.get_one_arg("BytesIO.write", heap)?;
                check_closed(state, "write")?;

                let bytes = value_to_bytes(&value, heap, interns)?;
                value.drop_with_heap(heap);

                let count = bytes_io_write(state, &bytes);
                Ok(Value::Int(count))
            }
            "getvalue" => {
                args.check_zero_args("BytesIO.getvalue", heap)?;
                if state.closed {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        "I/O operation on closed file.".to_string(),
                    )
                    .into());
                }
                let id = heap.allocate(HeapData::Bytes(Bytes::from(state.buffer.clone())))?;
                Ok(Value::Ref(id))
            }
            "tell" => {
                args.check_zero_args("BytesIO.tell", heap)?;
                check_closed(state, "tell")?;
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(state.position as i64))
            }
            "seek" => {
                let (pos, whence) = args.get_one_two_args("BytesIO.seek", heap)?;
                check_closed(state, "seek")?;

                let pos_val = extract_seek_pos(&pos, heap)?;
                let whence_val = match whence {
                    Some(w) => extract_whence(&w, heap)?,
                    None => 0,
                };

                let new_pos = bytes_io_seek(state, pos_val, whence_val)?;
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(new_pos as i64))
            }
            "truncate" => {
                let size_arg = args.get_zero_one_arg("BytesIO.truncate", heap)?;
                check_closed(state, "truncate")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None, // Defaults to current position
                };

                let new_size = bytes_io_truncate(state, size);
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(new_size as i64))
            }
            "close" => {
                args.check_zero_args("BytesIO.close", heap)?;
                bytes_io_validate_close(state, heap)?;
                state.closed = true;
                Ok(Value::None)
            }
            "readable" => {
                args.check_zero_args("BytesIO.readable", heap)?;
                check_closed(state, "readable")?;
                Ok(Value::Bool(true))
            }
            "writable" => {
                args.check_zero_args("BytesIO.writable", heap)?;
                check_closed(state, "writable")?;
                Ok(Value::Bool(true))
            }
            "seekable" => {
                args.check_zero_args("BytesIO.seekable", heap)?;
                check_closed(state, "seekable")?;
                Ok(Value::Bool(true))
            }
            "readline" => {
                let size_arg = args.get_zero_one_arg("BytesIO.readline", heap)?;
                check_closed(state, "readline")?;

                let size = match size_arg {
                    Some(val) => extract_size(&val, heap)?,
                    None => None,
                };

                let result = bytes_io_readline(state, size);
                let id = heap.allocate(HeapData::Bytes(Bytes::from(result)))?;
                Ok(Value::Ref(id))
            }
            "readlines" => {
                let size_hint = args.get_zero_one_arg("BytesIO.readlines", heap)?;
                check_closed(state, "readlines")?;
                let hint = match &size_hint {
                    Some(val) => extract_size(val, heap)?,
                    None => None,
                };
                size_hint.drop_with_heap(heap);

                let lines = bytes_io_readlines(state, hint);
                let line_values: Vec<Value> = lines
                    .into_iter()
                    .map(|b| {
                        Value::Ref(
                            heap.allocate(HeapData::Bytes(Bytes::from(b)))
                                .expect("allocation failed"),
                        )
                    })
                    .collect();
                let id = heap.allocate(HeapData::List(List::new(line_values)))?;
                Ok(Value::Ref(id))
            }
            "writelines" => {
                let lines = args.get_one_arg("BytesIO.writelines", heap)?;
                check_closed(state, "writelines")?;

                let mut iter = OurosIter::new(lines, heap, interns)?;
                while let Some(item) = iter.for_next(heap, interns)? {
                    let bytes = match value_to_bytes(&item, heap, interns) {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            item.drop_with_heap(heap);
                            iter.drop_with_heap(heap);
                            return Err(err);
                        }
                    };
                    item.drop_with_heap(heap);
                    bytes_io_write(state, &bytes);
                }
                iter.drop_with_heap(heap);
                Ok(Value::None)
            }
            "flush" => {
                args.check_zero_args("BytesIO.flush", heap)?;
                check_closed(state, "flush")?;
                Ok(Value::None)
            }
            "__iter__" => {
                args.check_zero_args("BytesIO.__iter__", heap)?;
                check_closed(state, "__iter__")?;
                Ok(Value::None) // Return self
            }
            "__next__" => {
                args.check_zero_args("BytesIO.__next__", heap)?;
                check_closed(state, "__next__")?;

                let line = bytes_io_readline(state, None);
                if line.is_empty() {
                    Err(ExcType::stop_iteration())
                } else {
                    let id = heap.allocate(HeapData::Bytes(Bytes::from(line)))?;
                    Ok(Value::Ref(id))
                }
            }
            "__enter__" => {
                args.check_zero_args("BytesIO.__enter__", heap)?;
                check_closed(state, "__enter__")?;
                Ok(Value::None)
            }
            "__exit__" => {
                let (positional, kwargs) = args.into_parts();
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                bytes_io_validate_close(state, heap)?;
                state.closed = true;
                Ok(Value::None)
            }
            "isatty" => {
                args.check_zero_args("BytesIO.isatty", heap)?;
                Ok(Value::Bool(false))
            }
            "fileno" => {
                args.check_zero_args("BytesIO.fileno", heap)?;
                Err(SimpleException::new_msg(ExcType::OSError, "fileno".to_string()).into())
            }
            "detach" => {
                args.check_zero_args("BytesIO.detach", heap)?;
                check_closed(state, "detach")?;
                Err(SimpleException::new_msg(ExcType::OSError, "detach".to_string()).into())
            }
            "readinto" | "readinto1" => {
                let buffer = args.get_one_arg("BytesIO.readinto", heap)?;
                check_closed(state, "readinto")?;

                let bytes_read = if let Value::Ref(id) = &buffer {
                    let buffer_len = if let HeapData::Bytearray(bytearray) = heap.get(*id) {
                        bytearray.as_slice().len()
                    } else {
                        let buffer_type = buffer.py_type(heap);
                        buffer.drop_with_heap(heap);
                        return Err(ExcType::type_error(format!(
                            "readinto() argument must be read-write bytes-like object, not '{buffer_type}'"
                        )));
                    };

                    let data = bytes_io_read(state, Some(buffer_len));
                    heap.with_entry_mut(*id, |_, heap_data| {
                        let HeapData::Bytearray(bytearray) = heap_data else {
                            return Err(ExcType::type_error(
                                "readinto() argument must be read-write bytes-like object".to_string(),
                            ));
                        };
                        let target = bytearray.as_vec_mut();
                        let copy_len = data.len().min(target.len());
                        target[..copy_len].copy_from_slice(&data[..copy_len]);
                        Ok(copy_len)
                    })?
                } else {
                    let buffer_type = buffer.py_type(heap);
                    buffer.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "readinto() argument must be read-write bytes-like object, not '{buffer_type}'"
                    )));
                };
                buffer.drop_with_heap(heap);
                #[expect(clippy::cast_possible_wrap)]
                Ok(Value::Int(bytes_read as i64))
            }
            "getbuffer" => {
                args.check_zero_args("BytesIO.getbuffer", heap)?;
                check_closed(state, "getbuffer")?;
                let id = heap.allocate(HeapData::Bytes(Bytes::from(state.buffer.clone())))?;
                bytes_io_track_export(state, id, heap);
                Ok(Value::Ref(id))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("BytesIO", attr))
            }
        }
    }
}

/// Formats and writes one csv row from an arbitrary iterable.
fn csv_writer_write_row(
    file_obj: &Value,
    dialect: &CsvDialect,
    row: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut iter = OurosIter::new(row.clone_with_heap(heap), heap, interns)?;
    let mut values = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        values.push(item);
    }
    iter.drop_with_heap(heap);

    let result = csv_write_values_to_file(file_obj, dialect, &values, heap, interns);
    values.drop_with_heap(heap);
    result
}

/// Writes already collected field values to the destination file object.
fn csv_write_values_to_file(
    file_obj: &Value,
    dialect: &CsvDialect,
    values: &[Value],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let line = csv_format_row(values, dialect, heap, interns)?;
    let line_id = heap.allocate(HeapData::Str(Str::from(line)))?;
    let write_arg = Value::Ref(line_id);

    let file_id = match file_obj {
        Value::Ref(id) => *id,
        _ => return Err(ExcType::type_error("csv writer expected a file object")),
    };

    let result = heap.call_attr_raw(
        file_id,
        &EitherStr::Heap("write".to_owned()),
        ArgValues::One(write_arg),
        interns,
    )?;
    match result {
        AttrCallResult::Value(value) => Ok(value),
        _ => Err(SimpleException::new_msg(ExcType::RuntimeError, "csv write failed".to_string()).into()),
    }
}

/// Writes string field values directly to the destination file object.
fn csv_write_string_values_to_file(
    file_obj: &Value,
    dialect: &CsvDialect,
    values: &[String],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let line = csv_format_row_from_strings(values, dialect);
    let line_id = heap.allocate(HeapData::Str(Str::from(line)))?;
    let write_arg = Value::Ref(line_id);
    let file_id = match file_obj {
        Value::Ref(id) => *id,
        _ => return Err(ExcType::type_error("csv writer expected a file object")),
    };
    let result = heap.call_attr_raw(
        file_id,
        &EitherStr::Heap("write".to_owned()),
        ArgValues::One(write_arg),
        interns,
    )?;
    match result {
        AttrCallResult::Value(value) => Ok(value),
        _ => Err(SimpleException::new_msg(ExcType::RuntimeError, "csv write failed".to_string()).into()),
    }
}

/// Formats a row of values according to CSV dialect quoting rules.
fn csv_format_row(
    values: &[Value],
    dialect: &CsvDialect,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let mut out = String::new();
    let row_len = values.len();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(dialect.delimiter);
        }
        let field = csv_format_field(value, row_len, dialect, heap, interns)?;
        out.push_str(&field);
    }
    out.push_str(&dialect.lineterminator);
    Ok(out)
}

/// Formats a row of string values with `QUOTE_MINIMAL` semantics.
fn csv_format_row_from_strings(values: &[String], dialect: &CsvDialect) -> String {
    let mut out = String::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(dialect.delimiter);
        }
        let special = value.contains(dialect.delimiter)
            || value.contains(dialect.quotechar)
            || value.contains('\n')
            || value.contains('\r');
        if special
            || dialect.quoting == QUOTE_ALL
            || dialect.quoting == QUOTE_NOTNULL
            || dialect.quoting == QUOTE_STRINGS
        {
            let escaped = if dialect.doublequote {
                value.replace(
                    dialect.quotechar,
                    &format!("{}{}", dialect.quotechar, dialect.quotechar),
                )
            } else {
                value.to_owned()
            };
            out.push(dialect.quotechar);
            out.push_str(&escaped);
            out.push(dialect.quotechar);
        } else if dialect.quoting == QUOTE_NONE {
            let escaped = csv_escape_unquoted(value, dialect).unwrap_or_else(|_| value.clone());
            out.push_str(&escaped);
        } else {
            out.push_str(value);
        }
    }
    out.push_str(&dialect.lineterminator);
    out
}

/// Formats one field according to the active quoting mode.
fn csv_format_field(
    value: &Value,
    row_len: usize,
    dialect: &CsvDialect,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let is_none = matches!(value, Value::None);
    let is_string = is_string_value(value, heap);
    let numeric = is_numeric_value(value, heap);
    let text = if is_none {
        String::new()
    } else {
        value.py_str(heap, interns).into_owned()
    };

    let special = text.contains(dialect.delimiter)
        || text.contains(dialect.quotechar)
        || text.contains('\n')
        || text.contains('\r');

    let quote = match dialect.quoting {
        QUOTE_MINIMAL => special || (row_len == 1 && is_string && text.is_empty()),
        QUOTE_ALL => true,
        QUOTE_NONNUMERIC => !numeric && !is_none,
        QUOTE_NONE => false,
        QUOTE_STRINGS => is_string,
        QUOTE_NOTNULL => !is_none,
        _ => special,
    };

    if quote {
        let escaped = csv_escape_quoted(&text, dialect)?;
        Ok(format!("{}{}{}", dialect.quotechar, escaped, dialect.quotechar))
    } else if dialect.quoting == QUOTE_NONE {
        csv_escape_unquoted(&text, dialect)
    } else {
        Ok(text)
    }
}

/// Escapes a field that is being wrapped in quote characters.
fn csv_escape_quoted(text: &str, dialect: &CsvDialect) -> RunResult<String> {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == dialect.quotechar {
            if dialect.doublequote {
                out.push(ch);
                out.push(ch);
            } else if let Some(escapechar) = dialect.escapechar {
                out.push(escapechar);
                out.push(ch);
            } else {
                return Err(ExcType::type_error("need escapechar when doublequote is false"));
            }
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

/// Escapes a field in `QUOTE_NONE` mode.
fn csv_escape_unquoted(text: &str, dialect: &CsvDialect) -> RunResult<String> {
    let Some(escapechar) = dialect.escapechar else {
        if text.contains(dialect.delimiter)
            || text.contains(dialect.quotechar)
            || text.contains('\n')
            || text.contains('\r')
        {
            return Err(ExcType::type_error("need to escape, but no escapechar set"));
        }
        return Ok(text.to_owned());
    };

    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == dialect.delimiter || ch == dialect.quotechar || ch == '\n' || ch == '\r' {
            out.push(escapechar);
        }
        out.push(ch);
    }
    Ok(out)
}

/// Returns true when a value is a Python string.
fn is_string_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::InternString(_) => true,
        Value::Ref(id) => matches!(heap.get(*id), HeapData::Str(_)),
        _ => false,
    }
}

/// Writes one dictionary row with DictWriter semantics.
fn csv_dict_writerow(
    state: &CsvDictWriterState,
    row: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let dict = match row {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Dict(dict) => dict,
            _ => return Err(ExcType::type_error("DictWriter.writerow() argument must be a dict")),
        },
        _ => return Err(ExcType::type_error("DictWriter.writerow() argument must be a dict")),
    };

    if state.extrasaction == "raise" {
        for (key, _) in dict {
            let key_text = key.py_str(heap, interns).into_owned();
            if !state.fieldnames.iter().any(|field| field == &key_text) {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    format!("dict contains fields not in fieldnames: '{key_text}'"),
                )
                .into());
            }
        }
    }

    let mut values = Vec::with_capacity(state.fieldnames.len());
    for name in &state.fieldnames {
        if let Some(value) = dict.get_by_str(name, heap, interns) {
            if matches!(value, Value::None) {
                values.push(String::new());
            } else {
                values.push(value.py_str(heap, interns).into_owned());
            }
        } else if matches!(state.restval, Value::None) {
            values.push(String::new());
        } else {
            values.push(state.restval.py_str(heap, interns).into_owned());
        }
    }
    csv_write_string_values_to_file(&state.file_obj, &state.dialect, &values, heap, interns)
}

/// Heuristic used by `csv.Sniffer.has_header`.
fn csv_sniffer_has_header(sample: &str) -> bool {
    let delimiter = detect_delimiter(sample);
    let dialect = CsvDialect {
        delimiter,
        quotechar: '"',
        escapechar: None,
        doublequote: true,
        skipinitialspace: false,
        lineterminator: "\r\n".to_owned(),
        quoting: QUOTE_MINIMAL,
    };

    let mut lines = sample.lines().filter(|line| !line.is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    let Some(second) = lines.next() else {
        return false;
    };

    let header = parse_csv_row(first, &dialect);
    let data = parse_csv_row(second, &dialect);
    if header.is_empty() || data.is_empty() {
        return false;
    }

    let header_textual = header.iter().all(|cell| cell.text.parse::<f64>().is_err());
    let data_numeric = data.iter().any(|cell| cell.text.parse::<f64>().is_ok());
    header_textual && data_numeric
}

/// Helper function to extract size argument for read operations.
fn extract_size(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<Option<usize>> {
    match value {
        Value::None => Ok(None),
        Value::Int(i) => {
            if *i < 0 {
                Ok(None) // Negative size means read all
            } else {
                #[expect(clippy::cast_sign_loss)]
                Ok(Some(*i as usize))
            }
        }
        Value::Bool(b) => Ok(Some(usize::from(*b))),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => {
                if let Some(i) = li.to_i64() {
                    if i < 0 {
                        Ok(None)
                    } else {
                        #[expect(clippy::cast_sign_loss)]
                        Ok(Some(i as usize))
                    }
                } else {
                    Ok(None) // Large integer, treat as "read all"
                }
            }
            _ => Err(ExcType::type_error("integer argument expected, got float".to_string())),
        },
        _ => Err(ExcType::type_error("integer argument expected, got float".to_string())),
    }
}

/// Helper function to extract seek position.
fn extract_seek_pos(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => li
                .to_i64()
                .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "int too large".to_string()).into()),
            _ => Err(ExcType::type_error("integer argument expected, got float".to_string())),
        },
        _ => Err(ExcType::type_error("integer argument expected, got float".to_string())),
    }
}

/// Helper function to extract whence argument.
fn extract_whence(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i32> {
    match value {
        Value::Int(i) => {
            if *i < 0 || *i > 2 {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    format!("invalid whence ({i}, should be 0, 1 or 2)"),
                )
                .into());
            }
            Ok(*i as i32)
        }
        Value::Bool(b) => Ok(i32::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => {
                if let Some(i) = li.to_i64() {
                    if !(0..=2).contains(&i) {
                        return Err(SimpleException::new_msg(
                            ExcType::ValueError,
                            format!("invalid whence ({i}, should be 0, 1 or 2)"),
                        )
                        .into());
                    }
                    Ok(i as i32)
                } else {
                    Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        format!("invalid whence ({li}, should be 0, 1 or 2)"),
                    )
                    .into())
                }
            }
            _ => Err(ExcType::type_error("integer argument expected, got float".to_string())),
        },
        _ => Err(ExcType::type_error("integer argument expected, got float".to_string())),
    }
}

/// Helper to extract exact `str` values for StringIO methods.
fn value_to_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "string argument expected, got '{}'",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "string argument expected, got '{}'",
            value.py_type(heap)
        ))),
    }
}

/// Helper to convert value to bytes.
fn value_to_bytes(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::InternString(_) => Err(ExcType::type_error(
            "a bytes-like object is required, not 'str'".to_string(),
        )),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Bytearray(bytearray) => Ok(bytearray.as_slice().to_vec()),
            HeapData::Str(_) => Err(ExcType::type_error(
                "a bytes-like object is required, not 'str'".to_string(),
            )),
            _ => Err(ExcType::type_error(format!(
                "a bytes-like object is required, not '{}'",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "a bytes-like object is required, not '{}'",
            value.py_type(heap)
        ))),
    }
}

fn string_char_len(s: &str) -> usize {
    s.chars().count()
}

fn char_to_byte_index(s: &str, char_index: usize) -> usize {
    s.char_indices().nth(char_index).map_or(s.len(), |(idx, _)| idx)
}

// ===== StringIO implementation =====

/// Read from StringIO buffer.
fn string_io_read(state: &mut StringIOState, size: Option<usize>) -> String {
    let start = state.position;
    let total_chars = string_char_len(&state.buffer);
    if start >= total_chars {
        return String::new();
    }

    let end = match size {
        Some(n) => start.saturating_add(n).min(total_chars),
        None => total_chars,
    };

    let start_byte = char_to_byte_index(&state.buffer, start);
    let end_byte = char_to_byte_index(&state.buffer, end);
    let result = state.buffer[start_byte..end_byte].to_string();
    state.position = end;
    result
}

/// Write to StringIO buffer at current position.
fn string_io_write(state: &mut StringIOState, s: &str) -> i64 {
    let pos = state.position;
    let write_chars: Vec<char> = s.chars().collect();
    let write_len = write_chars.len();

    if write_len == 0 {
        return 0;
    }

    let mut buffer_chars: Vec<char> = state.buffer.chars().collect();
    if pos > buffer_chars.len() {
        buffer_chars.resize(pos, '\0');
    }

    let end_pos = pos + write_len;
    if end_pos > buffer_chars.len() {
        buffer_chars.resize(end_pos, '\0');
    }

    for (idx, ch) in write_chars.into_iter().enumerate() {
        buffer_chars[pos + idx] = ch;
    }

    state.buffer = buffer_chars.into_iter().collect();
    state.position = end_pos;
    write_len as i64
}

/// Seek to position in StringIO buffer.
fn string_io_seek(state: &mut StringIOState, pos: i64, whence: i32) -> RunResult<usize> {
    let end_pos = string_char_len(&state.buffer);
    let new_pos = match whence {
        0 => {
            if pos < 0 {
                return Err(
                    SimpleException::new_msg(ExcType::ValueError, format!("Negative seek position {pos}")).into(),
                );
            }
            usize::try_from(pos).expect("checked non-negative")
        }
        1 => {
            if pos != 0 {
                return Err(SimpleException::new_msg(
                    ExcType::OSError,
                    "Can't do nonzero cur-relative seeks".to_string(),
                )
                .into());
            }
            state.position
        }
        2 => {
            if pos != 0 {
                return Err(SimpleException::new_msg(
                    ExcType::OSError,
                    "Can't do nonzero end-relative seeks".to_string(),
                )
                .into());
            }
            end_pos
        }
        _ => {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("invalid whence ({whence}, should be 0, 1 or 2)"),
            )
            .into());
        }
    };

    state.position = new_pos;
    Ok(new_pos)
}

/// Truncate StringIO buffer.
fn string_io_truncate(state: &mut StringIOState, size: Option<usize>) -> usize {
    let new_size = size.unwrap_or(state.position);
    let char_len = string_char_len(&state.buffer);
    if new_size < char_len {
        let byte_end = char_to_byte_index(&state.buffer, new_size);
        state.buffer.truncate(byte_end);
    }
    new_size
}

/// Read a line from StringIO buffer.
fn string_io_readline(state: &mut StringIOState, size: Option<usize>) -> String {
    let start = state.position;
    let total_chars = string_char_len(&state.buffer);
    if start >= total_chars {
        return String::new();
    }

    let start_byte = char_to_byte_index(&state.buffer, start);
    let remaining = &state.buffer[start_byte..];
    let line_end = if let Some(pos) = remaining.chars().position(|ch| ch == '\n') {
        start + pos + 1
    } else {
        total_chars
    };

    let end = match size {
        Some(limit) => start.saturating_add(limit).min(line_end),
        None => line_end,
    };

    let end_byte = char_to_byte_index(&state.buffer, end);
    let result = state.buffer[start_byte..end_byte].to_string();
    state.position = end;
    result
}

/// Read all lines from StringIO buffer.
fn string_io_readlines(state: &mut StringIOState, hint: Option<usize>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut consumed = 0usize;
    while state.position < string_char_len(&state.buffer) {
        let line = string_io_readline(state, None);
        if line.is_empty() {
            break;
        }
        consumed = consumed.saturating_add(line.chars().count());
        lines.push(line);
        if let Some(limit) = hint
            && limit > 0
            && consumed > limit
        {
            break;
        }
    }
    lines
}

// ===== BytesIO implementation =====

/// Read from BytesIO buffer.
fn bytes_io_read(state: &mut BytesIOState, size: Option<usize>) -> Vec<u8> {
    let start = state.position;
    let end = match size {
        Some(n) => (start + n).min(state.buffer.len()),
        None => state.buffer.len(),
    };

    if start >= state.buffer.len() {
        return Vec::new();
    }

    let result = state.buffer[start..end].to_vec();
    state.position = end;
    result
}

/// Write to BytesIO buffer at current position.
fn bytes_io_write(state: &mut BytesIOState, bytes: &[u8]) -> i64 {
    let pos = state.position;
    let len = bytes.len();

    if len == 0 {
        return 0;
    }

    if pos > state.buffer.len() {
        state.buffer.resize(pos, 0);
    }

    let end = pos + len;
    state.buffer.resize(end, 0);
    state.buffer[pos..end].copy_from_slice(bytes);

    state.position = pos + len;
    len as i64
}

/// Seek to position in BytesIO buffer.
fn bytes_io_seek(state: &mut BytesIOState, pos: i64, whence: i32) -> RunResult<usize> {
    let new_pos: i64 = match whence {
        0 => pos,
        1 => state.position as i64 + pos,
        2 => state.buffer.len() as i64 + pos,
        _ => {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("invalid whence ({whence}, should be 0, 1 or 2)"),
            )
            .into());
        }
    };

    if new_pos < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, format!("Negative seek position {new_pos}")).into());
    }

    state.position = new_pos as usize;
    Ok(state.position)
}

/// Truncate BytesIO buffer.
fn bytes_io_truncate(state: &mut BytesIOState, size: Option<usize>) -> usize {
    let new_size = size.unwrap_or(state.position);
    if new_size < state.buffer.len() {
        state.buffer.truncate(new_size);
    }
    new_size
}

/// Read a line from BytesIO buffer (lines end with \n).
fn bytes_io_readline(state: &mut BytesIOState, size: Option<usize>) -> Vec<u8> {
    let start = state.position;
    if start >= state.buffer.len() {
        return Vec::new();
    }

    let remaining = &state.buffer[start..];

    // Find newline
    if let Some(pos) = remaining.iter().position(|&b| b == b'\n') {
        let line_end = start + pos + 1; // Include the newline
        let end = size.map_or(line_end, |s| (start + s).min(line_end));
        let result = state.buffer[start..end].to_vec();
        state.position = end;
        result
    } else {
        // No newline found, return rest or up to size
        let end = size.map_or(state.buffer.len(), |s| (start + s).min(state.buffer.len()));
        let result = state.buffer[start..end].to_vec();
        state.position = end;
        result
    }
}

/// Read all lines from BytesIO buffer.
fn bytes_io_readlines(state: &mut BytesIOState, hint: Option<usize>) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut consumed = 0usize;
    while state.position < state.buffer.len() {
        let line = bytes_io_readline(state, None);
        if line.is_empty() {
            break;
        }
        consumed = consumed.saturating_add(line.len());
        lines.push(line);
        if let Some(limit) = hint
            && limit > 0
            && consumed >= limit
        {
            break;
        }
    }
    lines
}

/// Tracks one exported buffer by adding a BytesIO-owned reference.
///
/// The additional ref keeps the exported value alive so close-time validation
/// can safely inspect whether user code still holds the buffer.
fn bytes_io_track_export(state: &mut BytesIOState, buffer_id: HeapId, heap: &Heap<impl ResourceTracker>) {
    heap.inc_ref(buffer_id);
    state.exported_buffers.push(buffer_id);
}

/// Validates that no exported `BytesIO` buffers remain alive at close time.
///
/// Stale exports (only held by the BytesIO tracking ref) are released eagerly.
/// Live exports trigger the CPython-compatible `BufferError`.
fn bytes_io_validate_close(
    state: &mut BytesIOState,
    heap: &mut Heap<impl ResourceTracker>,
) -> crate::exception_private::RunResult<()> {
    let mut still_exported = Vec::with_capacity(state.exported_buffers.len());
    let mut has_live_export = false;

    for buffer_id in state.exported_buffers.drain(..) {
        if heap.get_refcount(buffer_id) > 1 {
            has_live_export = true;
            still_exported.push(buffer_id);
        } else {
            heap.dec_ref(buffer_id);
        }
    }
    state.exported_buffers = still_exported;

    if has_live_export {
        return Err(SimpleException::new_msg(
            ExcType::BufferError,
            "Existing exports of data: object cannot be re-sized",
        )
        .into());
    }

    Ok(())
}

impl PyTrait for StdlibObject {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        match self {
            Self::Struct(_) => Type::Struct,
            Self::DecimalContext(_) => Type::DecimalContext,
            Self::Complex { .. } => Type::Complex,
            Self::RegexFlagValue(_) => Type::RegexFlag,
            Self::ReScanner(_) => Type::SreScanner,
            Self::AsyncGenerator(_) => Type::AsyncGenerator,
            _ => Type::Object,
        }
    }

    fn py_estimate_size(&self) -> usize {
        match self {
            Self::ExitStack(state) => {
                std::mem::size_of::<ExitStackState>() + state.callbacks.len() * std::mem::size_of::<ExitCallback>()
            }
            Self::ContextManager(state) => {
                std::mem::size_of::<ContextManagerState>()
                    + state.name.len()
                    + state.suppress_types.len() * std::mem::size_of::<Value>()
            }
            Self::GeneratorContextManagerFactory(_) => std::mem::size_of::<GeneratorContextManagerFactoryState>(),
            Self::GeneratorContextManager(_) => std::mem::size_of::<GeneratorContextManagerState>(),
            Self::GeneratorContextDecorator(_) => std::mem::size_of::<GeneratorContextDecoratorState>(),
            Self::InstanceContextDecorator(_) => std::mem::size_of::<InstanceContextDecoratorState>(),
            Self::GeneratorNextCallable(_) => std::mem::size_of::<GeneratorNextCallableState>(),
            Self::GeneratorNextDefaultCallable(_) => std::mem::size_of::<GeneratorNextDefaultCallableState>(),
            Self::GeneratorCloseCallable(_) => std::mem::size_of::<GeneratorCloseCallableState>(),
            Self::ImmediateAwaitable(_) => std::mem::size_of::<ImmediateAwaitableState>(),
            Self::AsyncGenerator(_) => std::mem::size_of::<AsyncGeneratorState>(),
            Self::AnextAwaitable(_) => std::mem::size_of::<AnextAwaitableState>(),
            Self::Formatter => std::mem::size_of::<Self>(),
            Self::Template(state) => {
                std::mem::size_of::<TemplateState>()
                    + state.template.len()
                    + state.delimiter.len()
                    + state.idpattern.len()
            }
            Self::JsonEncoder(_) => std::mem::size_of::<JsonEncoderState>(),
            Self::JsonDecoder(_) => std::mem::size_of::<JsonDecoderState>(),
            Self::PrettyPrinter(_) => std::mem::size_of::<PrettyPrinterState>(),
            Self::StringIO(state) => std::mem::size_of::<StringIOState>() + state.buffer.len() + state.newline.len(),
            Self::BytesIO(state) => {
                std::mem::size_of::<BytesIOState>()
                    + state.buffer.len()
                    + state.exported_buffers.capacity() * std::mem::size_of::<HeapId>()
            }
            Self::Struct(state) => std::mem::size_of::<StructState>() + state.format.len(),
            Self::DecimalContext(state) => std::mem::size_of::<DecimalContextState>() + state.rounding.len(),
            Self::DecimalTuple(state) => {
                std::mem::size_of::<DecimalTupleState>() + state.digits.len() * std::mem::size_of::<i64>()
            }
            Self::Complex { .. } => std::mem::size_of::<(f64, f64)>(),
            Self::CsvReader(state) => {
                let rows_bytes = state
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|field| field.text.len()).sum::<usize>())
                    .sum::<usize>();
                std::mem::size_of::<CsvReaderState>() + rows_bytes + state.dialect.lineterminator.len()
            }
            Self::CsvWriter(_) => std::mem::size_of::<CsvWriterState>(),
            Self::CsvDictReader(state) => {
                std::mem::size_of::<CsvDictReaderState>()
                    + state
                        .rows
                        .iter()
                        .map(|row| row.iter().map(String::len).sum::<usize>())
                        .sum::<usize>()
                    + state.fieldnames.iter().map(String::len).sum::<usize>()
            }
            Self::CsvDictWriter(state) => {
                std::mem::size_of::<CsvDictWriterState>()
                    + state.fieldnames.iter().map(String::len).sum::<usize>()
                    + state.extrasaction.len()
            }
            Self::CsvDialect(state) => std::mem::size_of::<CsvDialect>() + state.lineterminator.len(),
            Self::CsvSniffer => std::mem::size_of::<Self>(),
            Self::RegexFlagValue(_) => std::mem::size_of::<i64>(),
            Self::RegexFlagEnum => std::mem::size_of::<Self>(),
            Self::ReScanner(state) => {
                std::mem::size_of::<ReScannerState>()
                    + state
                        .rules
                        .iter()
                        .map(|rule| rule.pattern.len() + rule.tag.as_ref().map_or(0, String::len))
                        .sum::<usize>()
            }
        }
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        match (self, other) {
            (Self::RegexFlagValue(a), Self::RegexFlagValue(b)) => a == b,
            (
                Self::Complex {
                    real: a_real,
                    imag: a_imag,
                },
                Self::Complex {
                    real: b_real,
                    imag: b_imag,
                },
            ) => a_real == b_real && a_imag == b_imag,
            _ => false,
        }
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        if let Self::ExitStack(state) = self {
            for callback in &mut state.callbacks {
                match callback {
                    ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.py_dec_ref_ids(stack),
                    ExitCallback::Callback { func, args, kwargs } => {
                        func.py_dec_ref_ids(stack);
                        for arg in args {
                            arg.py_dec_ref_ids(stack);
                        }
                        for (key, value) in kwargs {
                            key.py_dec_ref_ids(stack);
                            value.py_dec_ref_ids(stack);
                        }
                    }
                }
            }
        }
        if let Self::ContextManager(state) = self {
            state.enter_value.py_dec_ref_ids(stack);
            for value in &mut state.suppress_types {
                value.py_dec_ref_ids(stack);
            }
        }
        if let Self::GeneratorContextManagerFactory(state) = self {
            state.func.py_dec_ref_ids(stack);
        }
        if let Self::GeneratorContextManager(state) = self {
            state.generator.py_dec_ref_ids(stack);
        }
        if let Self::GeneratorContextDecorator(state) = self {
            state.generator.py_dec_ref_ids(stack);
            state.wrapped.py_dec_ref_ids(stack);
        }
        if let Self::InstanceContextDecorator(state) = self {
            state.manager.py_dec_ref_ids(stack);
            state.wrapped.py_dec_ref_ids(stack);
        }
        if let Self::GeneratorNextCallable(state) = self {
            state.generator.py_dec_ref_ids(stack);
        }
        if let Self::GeneratorNextDefaultCallable(state) = self {
            state.generator.py_dec_ref_ids(stack);
        }
        if let Self::GeneratorCloseCallable(state) = self {
            state.generator.py_dec_ref_ids(stack);
        }
        if let Self::ImmediateAwaitable(state) = self {
            state.value.py_dec_ref_ids(stack);
        }
        if let Self::AsyncGenerator(state) = self {
            state.iterator.py_dec_ref_ids(stack);
        }
        if let Self::AnextAwaitable(state) = self {
            state.iterator.py_dec_ref_ids(stack);
            if let Some(default) = &mut state.default {
                default.py_dec_ref_ids(stack);
            }
        }
        if let Self::PrettyPrinter(state) = self {
            stack.push(state.class_id);
        }
        if let Self::JsonDecoder(state) = self {
            if let Some(value) = &mut state.object_hook {
                value.py_dec_ref_ids(stack);
            }
            if let Some(value) = &mut state.parse_float {
                value.py_dec_ref_ids(stack);
            }
            if let Some(value) = &mut state.parse_int {
                value.py_dec_ref_ids(stack);
            }
            if let Some(value) = &mut state.parse_constant {
                value.py_dec_ref_ids(stack);
            }
            if let Some(value) = &mut state.object_pairs_hook {
                value.py_dec_ref_ids(stack);
            }
        }
        if let Self::BytesIO(state) = self {
            stack.append(&mut state.exported_buffers);
        }
        if let Self::CsvWriter(state) = self {
            state.file_obj.py_dec_ref_ids(stack);
        }
        if let Self::CsvDictReader(state) = self {
            state.restval.py_dec_ref_ids(stack);
        }
        if let Self::CsvDictWriter(state) = self {
            state.file_obj.py_dec_ref_ids(stack);
            state.restval.py_dec_ref_ids(stack);
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        match self {
            Self::RegexFlagValue(bits) => *bits != 0,
            Self::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
            _ => true,
        }
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        match self {
            Self::ExitStack(state) if state.async_mode => f.write_str("<contextlib.AsyncExitStack object>"),
            Self::ExitStack(_) => f.write_str("<contextlib.ExitStack object>"),
            Self::ContextManager(state) => write!(f, "<{} object>", state.name),
            Self::GeneratorContextManagerFactory(_) => {
                f.write_str("<contextlib._GeneratorContextManagerFactory object>")
            }
            Self::GeneratorContextManager(_) => f.write_str("<contextlib._GeneratorContextManager object>"),
            Self::GeneratorContextDecorator(_) => f.write_str("<contextlib._GeneratorContextDecorator object>"),
            Self::InstanceContextDecorator(_) => f.write_str("<contextlib._InstanceContextDecorator object>"),
            Self::GeneratorNextCallable(_) => f.write_str("<contextlib._GeneratorNextCallable object>"),
            Self::GeneratorNextDefaultCallable(_) => f.write_str("<contextlib._GeneratorNextDefaultCallable object>"),
            Self::GeneratorCloseCallable(_) => f.write_str("<contextlib._GeneratorCloseCallable object>"),
            Self::ImmediateAwaitable(_) => f.write_str("<contextlib._ImmediateAwaitable object>"),
            Self::AsyncGenerator(_) => f.write_str("<async_generator object>"),
            Self::AnextAwaitable(_) => f.write_str("<anext_awaitable object>"),
            Self::Formatter => f.write_str("<string.Formatter object>"),
            Self::Template(_) => f.write_str("<string.Template object>"),
            Self::JsonEncoder(_) => f.write_str("<json.JSONEncoder object>"),
            Self::JsonDecoder(_) => f.write_str("<json.JSONDecoder object>"),
            Self::PrettyPrinter(_) => f.write_str("<pprint.PrettyPrinter object>"),
            Self::StringIO(_) => f.write_str("<_io.StringIO object>"),
            Self::BytesIO(_) => f.write_str("<_io.BytesIO object>"),
            Self::Struct(_) => f.write_str("<_struct.Struct object>"),
            Self::DecimalContext(_) => f.write_str("<decimal.Context object>"),
            Self::DecimalTuple(_) => f.write_str("<DecimalTuple object>"),
            Self::Complex { real, imag } => f.write_str(&complex_repr(*real, *imag)),
            Self::CsvReader(_) => f.write_str("<_csv.reader object>"),
            Self::CsvWriter(_) => f.write_str("<_csv.writer object>"),
            Self::CsvDictReader(_) => f.write_str("<_csv.DictReader object>"),
            Self::CsvDictWriter(_) => f.write_str("<_csv.DictWriter object>"),
            Self::CsvDialect(_) => f.write_str("<_csv.Dialect object>"),
            Self::CsvSniffer => f.write_str("<csv.Sniffer object>"),
            Self::RegexFlagValue(bits) => f.write_str(&regex_flag_repr(*bits)),
            Self::RegexFlagEnum => f.write_str("<flag 'RegexFlag'>"),
            Self::ReScanner(_) => f.write_str("<_sre.SRE_Scanner object>"),
        }
    }

    fn py_str(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Cow<'static, str> {
        self.py_repr(heap, interns)
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let name = attr.as_str(interns);
        match self {
            Self::ExitStack(state) => Self::call_exit_stack_method(state, heap, name, args),
            Self::ContextManager(state) => Self::call_context_manager_method(state, heap, name, args),
            Self::GeneratorContextManagerFactory(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(
                    "contextlib._GeneratorContextManagerFactory",
                    name,
                ))
            }
            Self::GeneratorContextManager(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._GeneratorContextManager", name))
            }
            Self::GeneratorContextDecorator(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._GeneratorContextDecorator", name))
            }
            Self::InstanceContextDecorator(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._InstanceContextDecorator", name))
            }
            Self::GeneratorNextCallable(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._GeneratorNextCallable", name))
            }
            Self::GeneratorNextDefaultCallable(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(
                    "contextlib._GeneratorNextDefaultCallable",
                    name,
                ))
            }
            Self::GeneratorCloseCallable(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._GeneratorCloseCallable", name))
            }
            Self::ImmediateAwaitable(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("contextlib._ImmediateAwaitable", name))
            }
            Self::AsyncGenerator(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("async_generator", name))
            }
            Self::AnextAwaitable(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("anext_awaitable", name))
            }
            Self::Formatter => Self::call_formatter_method(heap, interns, name, args),
            Self::Template(state) => Self::call_template_method(state, heap, interns, name, args),
            Self::JsonEncoder(state) => Self::call_json_encoder_method(state, heap, interns, name, args),
            Self::JsonDecoder(state) => Self::call_json_decoder_method(state, heap, interns, name, args),
            Self::PrettyPrinter(state) => Self::call_pretty_printer_method(state, heap, interns, name, args),
            Self::StringIO(state) => Self::call_string_io_method(state, heap, interns, name, args),
            Self::BytesIO(state) => Self::call_bytes_io_method(state, heap, interns, name, args),
            Self::Struct(state) => Self::call_struct_method(state, heap, interns, name, args),
            Self::DecimalContext(state) => Self::call_decimal_context_method(state, heap, interns, name, args, self_id),
            Self::DecimalTuple(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("DecimalTuple", name))
            }
            Self::Complex { real, imag } => Self::call_complex_method(*real, *imag, heap, name, args),
            Self::CsvReader(state) => Self::call_csv_reader_method(state, heap, name, args, self_id),
            Self::CsvWriter(state) => Self::call_csv_writer_method(state, heap, interns, name, args),
            Self::CsvDictReader(state) => Self::call_csv_dict_reader_method(state, heap, interns, name, args, self_id),
            Self::CsvDictWriter(state) => Self::call_csv_dict_writer_method(state, heap, interns, name, args),
            Self::CsvDialect(_) => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error("Dialect", name))
            }
            Self::CsvSniffer => Self::call_csv_sniffer_method(heap, interns, name, args),
            Self::RegexFlagValue(_) | Self::RegexFlagEnum => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(Type::RegexFlag, name))
            }
            Self::ReScanner(state) => Self::call_re_scanner_method(state, heap, interns, name, args),
        }
    }

    fn py_call_attr_raw(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<AttrCallResult> {
        let name = attr.as_str(interns);
        if let Self::GeneratorContextManager(state) = self {
            return Self::call_generator_context_manager_method(state, heap, interns, name, args);
        }
        if let Self::PrettyPrinter(state) = self
            && name == "pprint"
        {
            let obj = args.get_one_arg("PrettyPrinter.pprint", heap)?;
            defer_drop!(obj, heap);
            let formatted = pprint_mod::format_object(heap, interns, obj, &state.params)?;
            return pprint_mod::build_print_call_result(heap, formatted);
        }

        let value = self.py_call_attr(heap, attr, args, interns, self_id)?;
        Ok(AttrCallResult::Value(value))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        if let Self::RegexFlagEnum = self {
            if let Some(bits) = regex_flag_bits_by_name(attr_name) {
                let id = heap.allocate(HeapData::StdlibObject(Self::new_regex_flag(bits)))?;
                return Ok(Some(AttrCallResult::Value(Value::Ref(id))));
            }
            return Ok(None);
        }
        if let Self::Formatter = self
            && matches!(
                attr_name,
                "format"
                    | "vformat"
                    | "parse"
                    | "get_value"
                    | "get_field"
                    | "format_field"
                    | "convert_field"
                    | "check_unused_args"
            )
        {
            // Expose formatter methods for attribute existence checks (e.g., hasattr).
            return Ok(Some(AttrCallResult::Value(Value::InternString(attr_id))));
        }
        if let Self::Template(state) = self
            && attr_name == "template"
        {
            let id = heap.allocate(HeapData::Str(Str::from(state.template.as_str())))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(id))));
        }
        if let Self::Struct(state) = self {
            if attr_id == StaticStrings::Size {
                #[expect(clippy::cast_possible_wrap)]
                return Ok(Some(AttrCallResult::Value(Value::Int(state.size as i64))));
            }
            if attr_id == StaticStrings::Format {
                let id = heap.allocate(HeapData::Str(Str::from(state.format.as_str())))?;
                return Ok(Some(AttrCallResult::Value(Value::Ref(id))));
            }
        }
        if let Self::DecimalContext(state) = self {
            let value = match attr_name {
                "prec" => Some(Value::Int(state.prec)),
                "rounding" => {
                    let id = heap.allocate(HeapData::Str(Str::from(state.rounding.as_str())))?;
                    Some(Value::Ref(id))
                }
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Some(AttrCallResult::Value(value)));
            }
        }
        if let Self::DecimalTuple(state) = self {
            let value = match attr_name {
                "sign" => Some(Value::Int(state.sign)),
                "exponent" => Some(Value::Int(state.exponent)),
                "digits" => {
                    let mut items: smallvec::SmallVec<[Value; 3]> =
                        smallvec::SmallVec::with_capacity(state.digits.len());
                    for digit in &state.digits {
                        items.push(Value::Int(*digit));
                    }
                    Some(allocate_tuple(items, heap)?)
                }
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Some(AttrCallResult::Value(value)));
            }
        }
        if let Self::Complex { real, imag } = self {
            let value = match attr_name {
                "real" => Some(Value::Float(*real)),
                "imag" => Some(Value::Float(*imag)),
                "conjugate" => Some(Value::InternString(attr_id)),
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Some(AttrCallResult::Value(value)));
            }
        }
        if let Self::CsvDictReader(state) = self
            && attr_name == "fieldnames"
        {
            let mut values = Vec::with_capacity(state.fieldnames.len());
            for name in &state.fieldnames {
                let id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
                values.push(Value::Ref(id));
            }
            let list_id = heap.allocate(HeapData::List(List::new(values)))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(list_id))));
        }
        if let Self::CsvDictWriter(state) = self
            && attr_name == "fieldnames"
        {
            let mut values = Vec::with_capacity(state.fieldnames.len());
            for name in &state.fieldnames {
                let id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
                values.push(Value::Ref(id));
            }
            let list_id = heap.allocate(HeapData::List(List::new(values)))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(list_id))));
        }
        if let Self::CsvDialect(state) = self {
            let value = match attr_name {
                "delimiter" => {
                    let id = heap.allocate(HeapData::Str(Str::from(state.delimiter.to_string())))?;
                    Some(Value::Ref(id))
                }
                "quotechar" => {
                    let id = heap.allocate(HeapData::Str(Str::from(state.quotechar.to_string())))?;
                    Some(Value::Ref(id))
                }
                "lineterminator" => {
                    let id = heap.allocate(HeapData::Str(Str::from(state.lineterminator.as_str())))?;
                    Some(Value::Ref(id))
                }
                "quoting" => Some(Value::Int(state.quoting)),
                "doublequote" => Some(Value::Bool(state.doublequote)),
                "skipinitialspace" => Some(Value::Bool(state.skipinitialspace)),
                "escapechar" => match state.escapechar {
                    Some(ch) => {
                        let id = heap.allocate(HeapData::Str(Str::from(ch.to_string())))?;
                        Some(Value::Ref(id))
                    }
                    None => Some(Value::None),
                },
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Some(AttrCallResult::Value(value)));
            }
        }
        // Handle `closed` property for StringIO and BytesIO
        if attr_name == "closed" {
            match self {
                Self::StringIO(state) => {
                    return Ok(Some(AttrCallResult::Value(Value::Bool(state.closed))));
                }
                Self::BytesIO(state) => {
                    return Ok(Some(AttrCallResult::Value(Value::Bool(state.closed))));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

/// Renders a `string.Formatter.format()` call.
fn formatter_format(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(format_value) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("Formatter.format", 1, 0));
    };
    let format_string = format_value.py_str(heap, interns).into_owned();
    format_value.drop_with_heap(heap);

    let positional_values: Vec<Value> = positional.collect();
    let named_values = kwargs_to_named_values(kwargs, heap, interns)?;

    let rendered = match simple_format(&format_string, &positional_values, &named_values, heap, interns) {
        Ok(rendered) => rendered,
        Err(err) => {
            positional_values.drop_with_heap(heap);
            for (_, value) in named_values {
                value.drop_with_heap(heap);
            }
            return Err(err);
        }
    };

    positional_values.drop_with_heap(heap);
    for (_, value) in named_values {
        value.drop_with_heap(heap);
    }

    let id = heap.allocate(HeapData::Str(Str::from(rendered)))?;
    Ok(Value::Ref(id))
}

/// Renders a `string.Formatter.vformat()` call.
fn formatter_vformat(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (format_value, values_value, mapping_value) = args.get_three_args("Formatter.vformat", heap)?;
    let format_string = format_value.py_str(heap, interns).into_owned();
    format_value.drop_with_heap(heap);

    let positional_values = iterable_to_values(values_value, heap, interns)?;
    let named_values = mapping_to_named_values(mapping_value, heap, interns, "Formatter.vformat")?;

    let rendered = match simple_format(&format_string, &positional_values, &named_values, heap, interns) {
        Ok(rendered) => rendered,
        Err(err) => {
            positional_values.drop_with_heap(heap);
            for (_, value) in named_values {
                value.drop_with_heap(heap);
            }
            return Err(err);
        }
    };

    positional_values.drop_with_heap(heap);
    for (_, value) in named_values {
        value.drop_with_heap(heap);
    }

    let id = heap.allocate(HeapData::Str(Str::from(rendered)))?;
    Ok(Value::Ref(id))
}

/// Parses a format string and returns a list of tuples.
fn formatter_parse(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let format_value = args.get_one_arg("Formatter.parse", heap)?;
    let format_string = format_value.py_str(heap, interns).into_owned();
    format_value.drop_with_heap(heap);

    let tuples = parse_format_string(&format_string)?;

    let mut tuple_values = Vec::with_capacity(tuples.len());
    for (literal, field_name, format_spec, conversion) in tuples {
        let literal_val = Value::Ref(heap.allocate(HeapData::Str(Str::from(literal)))?);

        let field_val = match field_name {
            Some(name) => Value::Ref(heap.allocate(HeapData::Str(Str::from(name)))?),
            None => Value::None,
        };

        let spec_val = match format_spec {
            Some(spec) => Value::Ref(heap.allocate(HeapData::Str(Str::from(spec)))?),
            None => Value::None,
        };

        let conv_val = match conversion {
            Some(c) => Value::Ref(heap.allocate(HeapData::Str(Str::from(c.to_string())))?),
            None => Value::None,
        };

        let tuple = allocate_tuple(smallvec::smallvec![literal_val, field_val, spec_val, conv_val], heap)?;
        tuple_values.push(tuple);
    }

    let list_id = heap.allocate(HeapData::List(List::new(tuple_values)))?;
    Ok(Value::Ref(list_id))
}

/// Parses a format string into component tuples.
fn parse_format_string(format_string: &str) -> RunResult<Vec<(String, Option<String>, Option<String>, Option<char>)>> {
    let mut result = Vec::new();
    let bytes = format_string.as_bytes();
    let mut i = 0;
    let mut literal_start = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2;
                    continue;
                }

                let literal = format_string[literal_start..i].to_owned();

                let field_start = i + 1;
                let mut end = field_start;
                while end < bytes.len() && bytes[end] != b'}' {
                    end += 1;
                }

                if end >= bytes.len() {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        "Single '{' encountered in format string",
                    )
                    .into());
                }

                let field_content = &format_string[field_start..end];
                let (field_name, format_spec, conversion) = parse_field_spec(field_content);

                result.push((literal, field_name, format_spec, conversion));

                i = end + 1;
                literal_start = i;
            }
            b'}' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                    i += 2;
                    continue;
                }
                return Err(
                    SimpleException::new_msg(ExcType::ValueError, "Single '}' encountered in format string").into(),
                );
            }
            _ => {
                i += 1;
            }
        }
    }

    // Only add final literal if there's remaining content after the last field
    if literal_start < format_string.len() {
        let final_literal = format_string[literal_start..].to_owned();
        let final_literal = final_literal.replace("{{", "{").replace("}}", "}");
        result.push((final_literal, None, None, None));
    } else if result.is_empty() && !format_string.is_empty() {
        // No fields at all - return the entire string as one literal
        let unescaped = format_string.replace("{{", "{").replace("}}", "}");
        result.push((unescaped, None, None, None));
    }
    // If result is empty and format_string is empty, return empty vec

    Ok(result)
}

/// Parses a field specification like "name!r:>10"
fn parse_field_spec(content: &str) -> (Option<String>, Option<String>, Option<char>) {
    if content.is_empty() {
        return (Some(String::new()), Some(String::new()), None);
    }

    // First split on ':' to get format_spec
    let (before_spec, format_spec) = match content.find(':') {
        Some(pos) => {
            let spec = &content[pos + 1..];
            (&content[..pos], Some(spec.to_owned()))
        }
        None => (content, Some(String::new())),
    };

    // Then in the part before ':', split on '!' to get conversion
    let (field_name, conversion) = match before_spec.find('!') {
        Some(pos) => {
            let conv_char = before_spec.chars().nth(pos + 1);
            (&before_spec[..pos], conv_char)
        }
        None => (before_spec, None),
    };

    (Some(field_name.to_owned()), format_spec, conversion)
}

/// Gets a value from args or kwargs by key.
fn formatter_get_value(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (key_value, args_value, kwargs_value) = args.get_three_args("Formatter.get_value", heap)?;

    let key_str = key_value.py_str(heap, interns).into_owned();
    key_value.drop_with_heap(heap);

    let positional = iterable_to_values(args_value, heap, interns)?;
    defer_drop!(positional, heap);

    let named = mapping_to_named_values(kwargs_value, heap, interns, "Formatter.get_value")?;

    let result = if let Ok(idx) = key_str.parse::<i64>() {
        if idx < 0 {
            return Err(SimpleException::new_msg(ExcType::IndexError, "tuple index out of range").into());
        }
        let uidx = idx as usize;
        if uidx >= positional.len() {
            return Err(SimpleException::new_msg(ExcType::IndexError, "tuple index out of range").into());
        }
        positional[uidx].clone_with_heap(heap)
    } else {
        let found = named.iter().find(|(name, _)| name == &key_str);
        match found {
            Some((_, value)) => value.clone_with_heap(heap),
            None => {
                return Err(SimpleException::new_msg(ExcType::KeyError, key_str).into());
            }
        }
    };

    for (_, v) in named {
        v.drop_with_heap(heap);
    }

    Ok(result)
}

/// Parses a field name and gets the value from args/kwargs.
fn formatter_get_field(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (field_name_value, args_value, kwargs_value) = args.get_three_args("Formatter.get_field", heap)?;

    let field_name = field_name_value.py_str(heap, interns).into_owned();
    field_name_value.drop_with_heap(heap);

    let positional = iterable_to_values(args_value, heap, interns)?;
    defer_drop!(positional, heap);

    let named = mapping_to_named_values(kwargs_value, heap, interns, "Formatter.get_field")?;

    let (value, used_key) = parse_and_resolve_field(&field_name, positional, &named, heap, interns)?;

    for (_, v) in named {
        v.drop_with_heap(heap);
    }

    let key_value = match used_key {
        FieldKey::Int(i) => Value::Int(i64::from(i)),
        FieldKey::Str(s) => Value::Ref(heap.allocate(HeapData::Str(Str::from(s)))?),
    };

    let tuple = allocate_tuple(smallvec::smallvec![value, key_value], heap)?;
    Ok(tuple)
}

/// Represents a field key - either an integer index or a string name.
#[derive(Debug)]
enum FieldKey {
    Int(i32),
    Str(String),
}

/// Parses a field name and resolves it to a value.
fn parse_and_resolve_field(
    field_name: &str,
    positional: &[Value],
    named: &[(String, Value)],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, FieldKey)> {
    let (first_part, rest) = split_field_name(field_name);

    let mut value = if let Ok(index) = first_part.parse::<usize>() {
        positional
            .get(index)
            .ok_or_else(|| SimpleException::new_msg(ExcType::IndexError, "tuple index out of range"))?
            .clone_with_heap(heap)
    } else {
        let found = named
            .iter()
            .find(|(name, _)| name == first_part)
            .ok_or_else(|| SimpleException::new_msg(ExcType::KeyError, first_part.to_owned()))?;
        found.1.clone_with_heap(heap)
    };

    let used_key = if let Ok(index) = first_part.parse::<i32>() {
        FieldKey::Int(index)
    } else {
        FieldKey::Str(first_part.to_owned())
    };

    for accessor in parse_field_accessors(rest)? {
        let next = match accessor {
            FieldAccessor::Attr(attr_name) => {
                let Some(attr_id) = static_or_ascii_string_id(attr_name) else {
                    let type_name = value.py_type(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::attribute_error(type_name, attr_name));
                };
                match value.py_getattr(attr_id, heap, interns)? {
                    AttrCallResult::Value(next) => next,
                    AttrCallResult::DescriptorGet(descriptor) => descriptor,
                    _ => {
                        let type_name = value.py_type(heap);
                        value.drop_with_heap(heap);
                        return Err(ExcType::attribute_error(type_name, attr_name));
                    }
                }
            }
            FieldAccessor::Index(key_str) => {
                let key_value = if let Ok(index) = key_str.parse::<i64>() {
                    Value::Int(index)
                } else if let Some(key_id) = static_or_ascii_string_id(key_str) {
                    Value::InternString(key_id)
                } else {
                    let id = heap.allocate(HeapData::Str(Str::from(key_str)))?;
                    Value::Ref(id)
                };
                let mut value_for_getitem = value.clone_with_heap(heap);
                let next = value_for_getitem.py_getitem(&key_value, heap, interns);
                value_for_getitem.drop_with_heap(heap);
                key_value.drop_with_heap(heap);
                next?
            }
        };
        value.drop_with_heap(heap);
        value = next;
    }

    Ok((value, used_key))
}

/// Splits a field name at the first attribute or index accessor.
fn split_field_name(field_name: &str) -> (&str, &str) {
    for (i, c) in field_name.char_indices() {
        if c == '.' || c == '[' {
            return (&field_name[..i], &field_name[i..]);
        }
    }
    (field_name, "")
}

/// Accessor parsed from a formatter field tail (`.name` or `[key]`).
enum FieldAccessor<'a> {
    /// Attribute lookup.
    Attr(&'a str),
    /// Item lookup with numeric or string key text.
    Index(&'a str),
}

/// Parses chained accessors after a formatter field root.
fn parse_field_accessors(rest: &str) -> RunResult<Vec<FieldAccessor<'_>>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let bytes = rest.as_bytes();

    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'.' && bytes[end] != b'[' {
                    end += 1;
                }
                if end == start {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "Empty attribute in field name").into());
                }
                out.push(FieldAccessor::Attr(&rest[start..end]));
                i = end;
            }
            b'[' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b']' {
                    end += 1;
                }
                if end >= bytes.len() {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "Missing ']' in field name").into());
                }
                out.push(FieldAccessor::Index(&rest[start..end]));
                i = end + 1;
            }
            _ => {
                return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid character in field name").into());
            }
        }
    }

    Ok(out)
}

/// Returns a `StringId` for ASCII or static strings that do not require runtime interning.
fn static_or_ascii_string_id(name: &str) -> Option<StringId> {
    if name.len() == 1 {
        return Some(StringId::from_ascii(name.as_bytes()[0]));
    }
    StaticStrings::from_str(name).ok().map(Into::into)
}

/// Formats a single field with the given format specification.
fn formatter_format_field(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (value, spec_value) = args.get_two_args("Formatter.format_field", heap)?;
    defer_drop!(value, heap);

    let spec = spec_value.py_str(heap, interns);

    let formatted = if spec.is_empty() {
        value.py_str(heap, interns).into_owned()
    } else {
        format_with_spec(value, &spec, heap, interns)?
    };

    spec_value.drop_with_heap(heap);

    let id = heap.allocate(HeapData::Str(Str::from(formatted)))?;
    Ok(Value::Ref(id))
}

/// Formats a value according to a format specification.
fn format_with_spec(
    value: &Value,
    spec: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let mut chars = spec.chars().peekable();

    let mut fill = ' ';
    let mut align = None::<char>;

    let first_char = chars.peek().copied();
    if let Some(c) = first_char {
        let spec_str: String = chars.clone().take(2).collect();
        if spec_str.len() == 2 && "<>=^".contains(spec_str.chars().nth(1).unwrap()) {
            fill = c;
            align = Some(spec_str.chars().nth(1).unwrap());
            chars.next();
            chars.next();
        } else if "<>=^".contains(c) {
            align = Some(c);
            chars.next();
        }
    }

    let mut _sign = None::<char>;
    if let Some(&c) = chars.peek()
        && (c == '+' || c == '-' || c == ' ')
    {
        _sign = Some(c);
        chars.next();
    }

    let mut _alternate = false;
    if let Some(&c) = chars.peek()
        && c == '#'
    {
        _alternate = true;
        chars.next();
    }

    let mut _zero_pad = false;
    if let Some(&c) = chars.peek()
        && c == '0'
        && align.is_none()
    {
        _zero_pad = true;
        fill = '0';
        align = Some('>');
        chars.next();
    }

    let mut width = None::<usize>;
    let mut width_str = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            width_str.push(c);
            chars.next();
        } else {
            break;
        }
    }
    if !width_str.is_empty() {
        width = width_str.parse().ok();
    }

    let mut precision = None::<usize>;
    if let Some(&c) = chars.peek()
        && c == '.'
    {
        chars.next();
        let mut prec_str = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                prec_str.push(c);
                chars.next();
            } else {
                break;
            }
        }
        if !prec_str.is_empty() {
            precision = prec_str.parse().ok();
        }
    }

    let type_char = chars.next();

    let mut result = match type_char {
        Some('d' | 'n') => {
            if let Value::Int(n) = value {
                format!("{n}")
            } else {
                value.py_str(heap, interns).into_owned()
            }
        }
        Some('f' | 'F') => {
            let prec = precision.unwrap_or(6);
            format_float_fixed(value, prec, heap, interns)?
        }
        Some('g' | 'G' | 'e' | 'E') => value.py_str(heap, interns).into_owned(),
        Some('s') => value.py_str(heap, interns).into_owned(),
        Some('r') => value.py_repr(heap, interns).into_owned(),
        Some('a') => {
            let s = value.py_repr(heap, interns);
            format_ascii(&s)
        }
        _ => value.py_str(heap, interns).into_owned(),
    };

    if (type_char.is_none() || type_char == Some('s'))
        && let Some(prec) = precision
        && result.chars().count() > prec
    {
        result = result.chars().take(prec).collect();
    }

    if let Some(w) = width {
        let result_len = result.chars().count();
        if result_len < w {
            let fill_char = fill;
            let alignment = align.unwrap_or('<');

            let padding = w - result_len;
            match alignment {
                '<' => {
                    for _ in 0..padding {
                        result.push(fill_char);
                    }
                }
                '>' | '=' => {
                    let mut new_result = String::with_capacity(w);
                    for _ in 0..padding {
                        new_result.push(fill_char);
                    }
                    new_result.push_str(&result);
                    result = new_result;
                }
                '^' => {
                    let left_pad = padding / 2;
                    let right_pad = padding - left_pad;
                    let mut new_result = String::with_capacity(w);
                    for _ in 0..left_pad {
                        new_result.push(fill_char);
                    }
                    new_result.push_str(&result);
                    for _ in 0..right_pad {
                        new_result.push(fill_char);
                    }
                    result = new_result;
                }
                _ => {}
            }
        }
    }

    Ok(result)
}

/// Formats a string as ASCII using Python-style escapes.
fn format_ascii(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            let byte = c as u8;
            if (0x20..=0x7e).contains(&byte) {
                result.push(c);
            } else {
                write!(result, "\\x{byte:02x}").expect("string write should be infallible");
            }
        } else {
            result.push_str(&ascii_escape(&c.to_string()));
        }
    }
    result
}

/// Applies a conversion to a value.
fn formatter_convert_field(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (value, conv_value) = args.get_two_args("Formatter.convert_field", heap)?;
    defer_drop!(value, heap);
    defer_drop!(conv_value, heap);

    let result = if matches!(conv_value, Value::None) {
        value.clone_with_heap(heap)
    } else {
        let conv_str = conv_value.py_str(heap, interns);
        match conv_str.as_ref() {
            "s" => {
                let s = value.py_str(heap, interns);
                Value::Ref(heap.allocate(HeapData::Str(Str::from(s.into_owned())))?)
            }
            "r" => {
                let s = value.py_repr(heap, interns);
                Value::Ref(heap.allocate(HeapData::Str(Str::from(s.into_owned())))?)
            }
            "a" => {
                let s = value.py_repr(heap, interns);
                let ascii_str = format_ascii(&s);
                Value::Ref(heap.allocate(HeapData::Str(Str::from(ascii_str)))?)
            }
            _ => {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    format!("Unknown conversion specifier {}", conv_str.as_ref()),
                )
                .into());
            }
        }
    };

    Ok(result)
}

/// Default implementation of check_unused_args - does nothing.
fn formatter_check_unused_args(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.drop_with_heap(heap);
    Ok(Value::None)
}

/// Converts kwargs to `(name, value)` pairs.
fn kwargs_to_named_values(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<(String, Value)>> {
    let mut values: Vec<(String, Value)> = Vec::new();
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (_, existing) in values {
                existing.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        values.push((name, value));
    }
    Ok(values)
}

/// Converts a mapping object into named `(key, value)` pairs.
fn mapping_to_named_values(
    mapping: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    method_name: &str,
) -> RunResult<Vec<(String, Value)>> {
    let Value::Ref(mapping_id) = mapping else {
        mapping.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("{method_name} mapping must be a dict")));
    };

    let HeapData::Dict(dict) = heap.get(mapping_id) else {
        Value::Ref(mapping_id).drop_with_heap(heap);
        return Err(ExcType::type_error(format!("{method_name} mapping must be a dict")));
    };

    let mut values: Vec<(String, Value)> = Vec::new();
    for (key, value) in dict {
        let key_name = match key {
            Value::InternString(id) => interns.get_str(*id).to_owned(),
            Value::Ref(id) => {
                if let HeapData::Str(s) = heap.get(*id) {
                    s.as_str().to_owned()
                } else {
                    for (_, existing) in values {
                        existing.drop_with_heap(heap);
                    }
                    Value::Ref(mapping_id).drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "{method_name} mapping keys must be strings"
                    )));
                }
            }
            _ => {
                for (_, existing) in values {
                    existing.drop_with_heap(heap);
                }
                Value::Ref(mapping_id).drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{method_name} mapping keys must be strings"
                )));
            }
        };
        values.push((key_name, value.clone_with_heap(heap)));
    }

    Value::Ref(mapping_id).drop_with_heap(heap);
    Ok(values)
}

/// Converts an iterable value into owned positional values.
fn iterable_to_values(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let values = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);
    Ok(values)
}

/// Performs lightweight brace-format substitution.
///
/// Supports:
/// - positional fields (`{}` / `{0}`), named fields (`{name}`), and escaped braces
/// - basic format specs for floats: `{:.2f}` for precision
pub(crate) fn simple_format(
    template: &str,
    positional: &[Value],
    named: &[(String, Value)],
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0usize;
    let mut auto_index = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    out.push('{');
                    i += 2;
                    continue;
                }
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'}' {
                    end += 1;
                }
                if end >= bytes.len() {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        "Single '{' encountered in format string",
                    )
                    .into());
                }
                let field_spec = &template[i + 1..end];

                // Parse conversion (after !) and format spec (after :) - accept but treat all as str()
                let (field, format_spec) = if let Some((head, tail)) = field_spec.split_once('!') {
                    (head, tail.split_once(':').map(|(_, b)| b))
                } else if let Some((head, tail)) = field_spec.split_once(':') {
                    (head, Some(tail))
                } else {
                    (field_spec, None)
                };

                let value = if field.is_empty() {
                    let Some(value) = positional.get(auto_index) else {
                        return Err(SimpleException::new_msg(
                            ExcType::IndexError,
                            "Replacement index out of range for positional args tuple",
                        )
                        .into());
                    };
                    auto_index += 1;
                    value
                } else if let Ok(index) = field.parse::<usize>() {
                    positional.get(index).ok_or_else(|| {
                        SimpleException::new_msg(
                            ExcType::IndexError,
                            "Replacement index out of range for positional args tuple",
                        )
                    })?
                } else {
                    named
                        .iter()
                        .find_map(|(name, value)| if name == field { Some(value) } else { None })
                        .ok_or_else(|| SimpleException::new_msg(ExcType::KeyError, field.to_owned()))?
                };

                // Apply format spec if present
                let formatted = if let Some(spec) = format_spec {
                    apply_format_spec(value, spec, heap, interns)?
                } else {
                    value.py_str(heap, interns).into_owned()
                };

                out.push_str(&formatted);
                i = end + 1;
            }
            b'}' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                    out.push('}');
                    i += 2;
                } else {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        "Single '}' encountered in format string",
                    )
                    .into());
                }
            }
            _ => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'{' && bytes[end] != b'}' {
                    end += 1;
                }
                out.push_str(&template[i..end]);
                i = end;
            }
        }
    }

    Ok(out)
}

/// Applies a format specification to a value.
///
/// Supports a subset of Python's format mini-language:
/// - Float precision: `.2f`, `.0f`, etc.
fn apply_format_spec(
    value: &Value,
    spec: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    // Parse the format spec for precision and type
    // Format: [[fill]align][sign][#][0][width][grouping_option][.precision][type]

    let mut remaining = spec;

    // Skip alignment, sign, alternate form, zero padding, width, grouping
    // Look for .precision or type at the end

    // Check for alignment (look for <, >, =, ^)
    if let Some(pos) = remaining.find(['<', '>', '=', '^']) {
        if pos > 0 {
            remaining = &remaining[pos + 1..];
        } else {
            remaining = &remaining[1..];
        }
    }

    // Check for sign
    if remaining.starts_with(['+', '-', ' ']) {
        remaining = &remaining[1..];
    }

    // Check for alternate form (#)
    if remaining.starts_with('#') {
        remaining = &remaining[1..];
    }

    // Check for zero padding
    if remaining.starts_with('0') {
        remaining = &remaining[1..];
    }

    // Check for width
    while remaining.starts_with(|c: char| c.is_ascii_digit()) {
        remaining = &remaining[1..];
    }

    // Check for grouping option
    if remaining.starts_with([',', '_']) {
        remaining = &remaining[1..];
    }

    // Check for precision
    let mut precision = None;
    if remaining.starts_with('.') {
        let rest = &remaining[1..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            precision = digits.parse::<usize>().ok();
            remaining = &rest[digits.len()..];
        }
    }

    // The remaining is the type
    let type_char = remaining;

    // Apply formatting based on type
    match type_char {
        "f" | "F" => {
            // Fixed-point float formatting with precision
            let prec = precision.unwrap_or(6);
            format_float_fixed(value, prec, heap, interns)
        }
        _ => {
            // For unknown types or empty type, just convert to string
            Ok(value.py_str(heap, interns).into_owned())
        }
    }
}

/// Formats a float in fixed-point notation with given precision.
fn format_float_fixed(
    value: &Value,
    precision: usize,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    match value {
        Value::Float(f) => Ok(format!("{f:.precision$}")),
        Value::Int(i) => Ok(format!("{:.prec$}", *i as f64, prec = precision)),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::LongInt(li) => {
                if let Some(f) = li.to_f64() {
                    Ok(format!("{f:.precision$}"))
                } else {
                    Ok(value.py_str(heap, interns).into_owned())
                }
            }
            _ => Ok(value.py_str(heap, interns).into_owned()),
        },
        _ => Ok(value.py_str(heap, interns).into_owned()),
    }
}

/// Implements `Template.substitute` and `Template.safe_substitute`.
fn template_substitute(
    state: &TemplateState,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    safe: bool,
) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let mapping = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        mapping.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("Template.substitute", 1, 2));
    }

    let mut bindings = Vec::new();
    if let Some(mapping) = mapping {
        let mut mapped = match mapping_to_named_values(mapping, heap, interns, "Template.substitute") {
            Ok(mapped) => mapped,
            Err(err) => {
                kwargs.drop_with_heap(heap);
                return Err(err);
            }
        };
        bindings.append(&mut mapped);
    }
    let mut kwargs_pairs = kwargs_to_named_values(kwargs, heap, interns)?;
    bindings.append(&mut kwargs_pairs);

    let rendered = match substitute_template(
        &state.template,
        &state.delimiter,
        &state.idpattern,
        &bindings,
        heap,
        interns,
        safe,
    ) {
        Ok(rendered) => rendered,
        Err(err) => {
            for (_, value) in bindings {
                value.drop_with_heap(heap);
            }
            return Err(err);
        }
    };
    for (_, value) in bindings {
        value.drop_with_heap(heap);
    }

    let id = heap.allocate(HeapData::Str(Str::from(rendered)))?;
    Ok(Value::Ref(id))
}

/// Returns unique valid placeholder identifiers in order of first appearance.
fn template_get_identifiers(
    state: &TemplateState,
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
) -> RunResult<Value> {
    args.check_zero_args("Template.get_identifiers", heap)?;

    let (identifiers, _is_valid) =
        template_identifiers_and_validity(&state.template, &state.delimiter, &state.idpattern);
    let mut items = Vec::with_capacity(identifiers.len());
    for identifier in identifiers {
        let id = heap.allocate(HeapData::Str(Str::from(identifier)))?;
        items.push(Value::Ref(id));
    }

    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Returns whether the template contains only valid placeholders.
fn template_is_valid(
    state: &TemplateState,
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
) -> RunResult<Value> {
    args.check_zero_args("Template.is_valid", heap)?;
    let (_identifiers, is_valid) =
        template_identifiers_and_validity(&state.template, &state.delimiter, &state.idpattern);
    Ok(Value::Bool(is_valid))
}

/// Parses template placeholders and reports both identifiers and validity.
fn template_identifiers_and_validity(template: &str, delimiter: &str, idpattern: &str) -> (Vec<String>, bool) {
    let bytes = template.as_bytes();
    let delimiter = delimiter.as_bytes().first().copied().unwrap_or(b'$');
    let mut i = 0usize;
    let mut valid = true;
    let mut seen = AHashSet::<String>::new();
    let mut identifiers = Vec::new();

    while i < bytes.len() {
        if bytes[i] != delimiter {
            i += 1;
            continue;
        }

        if i + 1 >= bytes.len() {
            valid = false;
            break;
        }

        match bytes[i + 1] {
            next if next == delimiter => {
                i += 2;
            }
            b'{' => {
                let mut end = i + 2;
                while end < bytes.len() && bytes[end] != b'}' {
                    end += 1;
                }
                if end >= bytes.len() {
                    valid = false;
                    break;
                }
                let key = &template[i + 2..end];
                if is_valid_template_identifier(key, idpattern) {
                    let key_owned = key.to_owned();
                    if seen.insert(key_owned.clone()) {
                        identifiers.push(key_owned);
                    }
                } else {
                    valid = false;
                }
                i = end + 1;
            }
            _ => {
                let mut end = i + 1;
                while end < bytes.len() && is_identifier_continue(bytes[end]) {
                    end += 1;
                }
                let key = &template[i + 1..end];
                if is_valid_template_identifier(key, idpattern) {
                    let key_owned = key.to_owned();
                    if seen.insert(key_owned.clone()) {
                        identifiers.push(key_owned);
                    }
                    i = end;
                } else {
                    valid = false;
                    i += 1;
                }
            }
        }
    }

    (identifiers, valid)
}

/// Resolves `$name` and `${name}` placeholders in a template.
fn substitute_template(
    template: &str,
    delimiter: &str,
    idpattern: &str,
    bindings: &[(String, Value)],
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    safe: bool,
) -> RunResult<String> {
    let bytes = template.as_bytes();
    let delimiter = delimiter.as_bytes().first().copied().unwrap_or(b'$');
    let mut i = 0usize;
    let mut out = String::with_capacity(template.len());

    while i < bytes.len() {
        if bytes[i] != delimiter {
            let mut end = i + 1;
            while end < bytes.len() && bytes[end] != delimiter {
                end += 1;
            }
            out.push_str(&template[i..end]);
            i = end;
            continue;
        }

        if i + 1 >= bytes.len() {
            out.push(delimiter as char);
            break;
        }

        match bytes[i + 1] {
            next if next == delimiter => {
                out.push(delimiter as char);
                i += 2;
            }
            b'{' => {
                let mut end = i + 2;
                while end < bytes.len() && bytes[end] != b'}' {
                    end += 1;
                }
                if end >= bytes.len() {
                    if safe {
                        out.push_str(&template[i..]);
                        break;
                    }
                    return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid placeholder in string").into());
                }
                let key = &template[i + 2..end];
                if !is_valid_template_identifier(key, idpattern) {
                    if safe {
                        out.push_str(&template[i..=end]);
                        i = end + 1;
                        continue;
                    }
                    return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid placeholder in string").into());
                }
                if let Some((_, value)) = bindings.iter().find(|(name, _)| name == key) {
                    out.push_str(value.py_str(heap, interns).as_ref());
                } else if safe {
                    out.push_str(&template[i..=end]);
                } else {
                    return Err(SimpleException::new_msg(ExcType::KeyError, key.to_owned()).into());
                }
                i = end + 1;
            }
            _ => {
                let mut end = i + 1;
                while end < bytes.len() && is_identifier_continue(bytes[end]) {
                    end += 1;
                }
                let key = &template[i + 1..end];
                if !is_valid_template_identifier(key, idpattern) {
                    if safe {
                        out.push(delimiter as char);
                        i += 1;
                        continue;
                    }
                    return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid placeholder in string").into());
                }
                if let Some((_, value)) = bindings.iter().find(|(name, _)| name == key) {
                    out.push_str(value.py_str(heap, interns).as_ref());
                } else if safe {
                    out.push_str(&template[i..end]);
                } else {
                    return Err(SimpleException::new_msg(ExcType::KeyError, key.to_owned()).into());
                }
                i = end;
            }
        }
    }

    Ok(out)
}

/// Returns whether the byte can continue an identifier.
fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Returns whether a string is a valid ASCII identifier.
fn is_valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && *first != b'_' {
        return false;
    }
    bytes.iter().skip(1).all(|byte| is_identifier_continue(*byte))
}

/// Returns whether a template identifier matches the configured id pattern.
fn is_valid_template_identifier(value: &str, idpattern: &str) -> bool {
    match idpattern {
        "(?a:[_a-z][_a-z0-9]*)" => is_valid_identifier(value),
        "[A-Z]+" => !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_uppercase()),
        _ => is_valid_identifier(value),
    }
}
