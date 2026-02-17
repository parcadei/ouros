//! Built-in module implementations.
//!
//! This module provides implementations for Python built-in modules like `sys`, `typing`,
//! and `asyncio`. These are created on-demand when import statements are executed.

use std::fmt::{self, Write};

use strum::FromRepr;

use crate::{
    args::ArgValues,
    exception_private::RunResult,
    heap::{Heap, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::{ResourceError, ResourceTracker},
    types::AttrCallResult,
};

pub(crate) mod abc;
pub(crate) mod argparse_mod;
pub(crate) mod array_mod;
pub(crate) mod asyncio;
pub(crate) mod atexit_mod;
pub(crate) mod base64_mod;
pub(crate) mod binascii_mod;
pub(crate) mod bisect;
pub(crate) mod builtins_mod;
pub(crate) mod cmath_mod;
pub(crate) mod codecs_mod;
pub(crate) mod collections;
pub(crate) mod collections_abc;
pub(crate) mod concurrent_futures;
pub(crate) mod concurrent_mod;
pub(crate) mod contextlib;
pub(crate) mod copy_mod;
pub(crate) mod csv_mod;
pub(crate) mod dataclasses;
pub(crate) mod datetime_mod;
pub(crate) mod decimal_mod;
pub(crate) mod difflib;
pub(crate) mod enum_mod;
pub(crate) mod errno_mod;
pub(crate) mod fnmatch;
pub(crate) mod fractions_mod;
pub(crate) mod functools;
pub(crate) mod gc_mod;
pub(crate) mod hashlib;
pub(crate) mod heapq;
pub(crate) mod html;
pub(crate) mod inspect_mod;
pub(crate) mod io_mod;
pub(crate) mod ipaddress;
pub(crate) mod itertools;
pub(crate) mod json;
pub(crate) mod linecache_mod;
pub(crate) mod logging_mod;
pub(crate) mod math;
pub(crate) mod numbers_mod;
pub(crate) mod operator;
pub(crate) mod os;
pub(crate) mod pathlib;
pub(crate) mod pickle_mod;
pub(crate) mod pprint_mod;
pub(crate) mod queue_mod;
pub(crate) mod random_mod;
pub(crate) mod re;
pub(crate) mod secrets_mod;
pub(crate) mod shelve_mod;
pub(crate) mod shlex;
pub(crate) mod statistics;
pub(crate) mod string_mod;
pub(crate) mod struct_mod;
pub(crate) mod sys;
pub(crate) mod textwrap;
pub(crate) mod threading_mod;
pub(crate) mod time_mod;
pub(crate) mod token_mod;
pub(crate) mod tokenize_mod;
pub(crate) mod tomllib;
pub(crate) mod traceback_mod;
pub(crate) mod types_mod;
pub(crate) mod typing;
pub(crate) mod typing_extensions;
pub(crate) mod urllib_mod;
pub(crate) mod urllib_parse;
pub(crate) mod uuid_mod;
pub(crate) mod warnings_mod;
pub(crate) mod weakref;
pub(crate) mod zlib_mod;

/// Built-in modules that can be imported.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
pub(crate) enum BuiltinModule {
    /// The `sys` module providing system-specific parameters and functions.
    Sys,
    /// The `typing` module providing type hints support.
    Typing,
    /// The `asyncio` module providing async/await support (`gather`, `sleep`, `create_task`, etc.).
    Asyncio,
    /// The `pathlib` module providing object-oriented filesystem paths.
    Pathlib,
    /// The `os` module providing operating system interface (only `getenv()` implemented).
    Os,
    /// The `os.path` module providing pure string path helpers.
    OsPath,
    /// The `weakref` module providing weak reference helpers.
    Weakref,
    /// The `gc` module providing garbage-collection compatibility helpers.
    Gc,
    /// The `copy` module providing shallow and deep copy operations.
    Copy,
    /// The `json` module providing JSON encoding/decoding.
    Json,
    /// The `math` module providing mathematical functions and constants.
    Math,
    /// The `re` module providing regular expression support.
    Re,
    /// The `collections` module providing specialized container types.
    Collections,
    /// The `itertools` module providing iterator utilities.
    Itertools,
    /// The `functools` module providing higher-order functions.
    Functools,
    /// The `dataclasses` module providing dataclass decorator.
    Dataclasses,
    /// The `abc` module providing abstract base class stubs.
    Abc,
    /// The `argparse` module.
    Argparse,
    /// The `atexit` module for exit callback registration.
    Atexit,
    /// The `hashlib` module providing hash functions.
    Hashlib,
    /// The `contextlib` module providing context manager utilities.
    Contextlib,
    /// The `statistics` module providing basic statistical functions.
    Statistics,
    /// The `textwrap` module providing text wrapping and indentation.
    Textwrap,
    /// The `uuid` module providing UUID generation.
    Uuid,
    /// The `base64` module providing base64 encoding/decoding.
    Base64,
    /// The `binascii` module providing binary/ASCII transforms.
    Binascii,
    /// The `random` module providing pseudo-random number generation.
    Random,
    /// The `csv` module providing CSV parsing.
    Csv,
    /// The `codecs` module providing text and bytes codec helpers.
    Codecs,
    /// The `cmath` module providing complex-number math helpers.
    Cmath,
    /// The `operator` module providing function equivalents of built-in operators.
    Operator,
    /// The `bisect` module providing binary search functions for sorted lists.
    Bisect,
    /// The `heapq` module providing heap queue operations.
    Heapq,
    /// The `string` module providing string constants.
    StringMod,
    /// The `struct` module providing binary data packing/unpacking.
    Struct,
    /// The `enum` module providing enum compatibility stubs.
    Enum,
    /// The `io` module providing in-memory stream classes.
    Io,
    /// The `datetime` module providing date and time types.
    Datetime,
    /// The `decimal` module providing arbitrary precision decimal arithmetic.
    Decimal,
    /// The `fractions` module providing rational number arithmetic.
    Fractions,
    /// The `pprint` module providing pretty-printing functionality.
    Pprint,
    /// The `time` module providing sandboxed time functions.
    Time,
    /// The `warnings` module providing warning APIs.
    Warnings,
    /// The `logging` module providing logging APIs.
    Logging,
    /// The `numbers` module providing numeric abstract base classes.
    Numbers,
    /// The `types` module providing runtime type helpers.
    Types,
    /// The `typing_extensions` module providing typing compatibility helpers.
    TypingExtensions,
    /// The `collections.abc` submodule.
    CollectionsAbc,
    /// The `zlib` module providing checksum/compression helpers.
    Zlib,
    /// The `inspect` module providing runtime introspection helpers.
    Inspect,
    /// The `html` module.
    Html,
    /// The `shlex` module.
    Shlex,
    /// The `fnmatch` module.
    Fnmatch,
    /// The `tomllib` module.
    Tomllib,
    /// The `urllib` package.
    Urllib,
    /// The `urllib.parse` submodule.
    UrllibParse,
    /// The `difflib` module.
    Difflib,
    /// The `ipaddress` module.
    Ipaddress,
    /// The `builtins` module exposing built-in runtime names.
    BuiltinsMod,
    /// The `threading` module.
    Threading,
    /// The `concurrent` package.
    Concurrent,
    /// The `concurrent.futures` submodule.
    ConcurrentFutures,
    /// The `pickle` module providing object serialization.
    Pickle,
    /// The `shelve` module providing dict-like persistent storage.
    Shelve,
    /// The `traceback` module providing traceback helpers.
    Traceback,
    /// The `secrets` module providing cryptographic token helpers.
    Secrets,
    /// The `errno` module exposing POSIX errno constants and `errorcode`.
    Errno,
    /// The `linecache` module providing source-line caching helpers.
    Linecache,
    /// The `queue` module providing synchronized queue APIs.
    Queue,
    /// The `array` module providing typed array sequences.
    ArrayMod,
    /// The `token` module.
    Token,
    /// The `tokenize` module.
    Tokenize,
}

impl BuiltinModule {
    /// Get the module from a string ID.
    pub fn from_string_id(string_id: StringId) -> Option<Self> {
        match StaticStrings::from_string_id(string_id)? {
            StaticStrings::Sys => Some(Self::Sys),
            StaticStrings::Typing => Some(Self::Typing),
            StaticStrings::Asyncio => Some(Self::Asyncio),
            StaticStrings::Pathlib => Some(Self::Pathlib),
            StaticStrings::Os => Some(Self::Os),
            StaticStrings::OsPathMod => Some(Self::OsPath),
            StaticStrings::Weakref => Some(Self::Weakref),
            StaticStrings::Gc => Some(Self::Gc),
            StaticStrings::CopyMod | StaticStrings::Copy => Some(Self::Copy),
            StaticStrings::Json => Some(Self::Json),
            StaticStrings::Math => Some(Self::Math),
            StaticStrings::Re => Some(Self::Re),
            StaticStrings::Collections => Some(Self::Collections),
            StaticStrings::Itertools => Some(Self::Itertools),
            StaticStrings::Functools => Some(Self::Functools),
            StaticStrings::Dataclasses => Some(Self::Dataclasses),
            StaticStrings::Abc => Some(Self::Abc),
            StaticStrings::Argparse => Some(Self::Argparse),
            StaticStrings::Atexit => Some(Self::Atexit),
            StaticStrings::Hashlib => Some(Self::Hashlib),
            StaticStrings::Contextlib => Some(Self::Contextlib),
            StaticStrings::Statistics => Some(Self::Statistics),
            StaticStrings::Textwrap => Some(Self::Textwrap),
            StaticStrings::Uuid => Some(Self::Uuid),
            StaticStrings::Base64 => Some(Self::Base64),
            StaticStrings::Binascii => Some(Self::Binascii),
            StaticStrings::Random => Some(Self::Random),
            StaticStrings::Csv => Some(Self::Csv),
            StaticStrings::Codecs => Some(Self::Codecs),
            StaticStrings::Cmath => Some(Self::Cmath),
            StaticStrings::Operator => Some(Self::Operator),
            StaticStrings::Bisect => Some(Self::Bisect),
            StaticStrings::Heapq => Some(Self::Heapq),
            StaticStrings::Inspect => Some(Self::Inspect),
            StaticStrings::BuiltinsMod => Some(Self::BuiltinsMod),
            StaticStrings::StringMod => Some(Self::StringMod),
            StaticStrings::StructMod => Some(Self::Struct),
            StaticStrings::EnumMod => Some(Self::Enum),
            StaticStrings::Io => Some(Self::Io),
            StaticStrings::Datetime => Some(Self::Datetime),
            StaticStrings::Decimal => Some(Self::Decimal),
            StaticStrings::Fractions => Some(Self::Fractions),
            StaticStrings::Pprint => Some(Self::Pprint),
            StaticStrings::TimeMod | StaticStrings::Time => Some(Self::Time),
            StaticStrings::Warnings => Some(Self::Warnings),
            StaticStrings::Logging => Some(Self::Logging),
            StaticStrings::Numbers => Some(Self::Numbers),
            StaticStrings::TypesMod => Some(Self::Types),
            StaticStrings::TypingExtensions => Some(Self::TypingExtensions),
            StaticStrings::CollectionsAbc => Some(Self::CollectionsAbc),
            StaticStrings::Zlib => Some(Self::Zlib),
            StaticStrings::Html => Some(Self::Html),
            StaticStrings::Shlex => Some(Self::Shlex),
            StaticStrings::Fnmatch => Some(Self::Fnmatch),
            StaticStrings::Tomllib => Some(Self::Tomllib),
            StaticStrings::Urllib => Some(Self::Urllib),
            StaticStrings::UrllibParse => Some(Self::UrllibParse),
            StaticStrings::Difflib => Some(Self::Difflib),
            StaticStrings::Ipaddress => Some(Self::Ipaddress),
            StaticStrings::Pickle => Some(Self::Pickle),
            StaticStrings::Shelve => Some(Self::Shelve),
            StaticStrings::Traceback => Some(Self::Traceback),
            StaticStrings::Secrets => Some(Self::Secrets),
            StaticStrings::Errno => Some(Self::Errno),
            StaticStrings::Linecache => Some(Self::Linecache),
            StaticStrings::Queue => Some(Self::Queue),
            StaticStrings::ArrayMod => Some(Self::ArrayMod),
            StaticStrings::TokenMod => Some(Self::Token),
            StaticStrings::TokenizeMod => Some(Self::Tokenize),
            _ => None,
        }
    }

    /// Creates a new instance of this module on the heap.
    ///
    /// Returns a HeapId pointing to the newly allocated module.
    ///
    /// # Panics
    ///
    /// Panics if the required strings have not been pre-interned during prepare phase.
    pub fn create(self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
        match self {
            Self::Sys => sys::create_module(heap, interns),
            Self::Typing => typing::create_module(heap, interns),
            Self::Asyncio => asyncio::create_module(heap, interns),
            Self::Pathlib => pathlib::create_module(heap, interns),
            Self::Os => os::create_module(heap, interns),
            Self::OsPath => os::create_os_path_module(heap, interns),
            Self::Weakref => weakref::create_module(heap, interns),
            Self::Gc => gc_mod::create_module(heap, interns),
            Self::Copy => copy_mod::create_module(heap, interns),
            Self::Json => json::create_module(heap, interns),
            Self::Math => math::create_module(heap, interns),
            Self::Re => re::create_module(heap, interns),
            Self::Collections => collections::create_module(heap, interns),
            Self::Itertools => itertools::create_module(heap, interns),
            Self::Functools => functools::create_module(heap, interns),
            Self::Dataclasses => dataclasses::create_module(heap, interns),
            Self::Abc => abc::create_module(heap, interns),
            Self::Argparse => argparse_mod::create_module(heap, interns),
            Self::Atexit => atexit_mod::create_module(heap, interns),
            Self::Hashlib => hashlib::create_module(heap, interns),
            Self::Contextlib => contextlib::create_module(heap, interns),
            Self::Statistics => statistics::create_module(heap, interns),
            Self::Textwrap => textwrap::create_module(heap, interns),
            Self::Uuid => uuid_mod::create_module(heap, interns),
            Self::Base64 => base64_mod::create_module(heap, interns),
            Self::Binascii => binascii_mod::create_module(heap, interns),
            Self::Random => random_mod::create_module(heap, interns),
            Self::Csv => csv_mod::create_module(heap, interns),
            Self::Codecs => codecs_mod::create_module(heap, interns),
            Self::Cmath => cmath_mod::create_module(heap, interns),
            Self::Operator => operator::create_module(heap, interns),
            Self::Bisect => bisect::create_module(heap, interns),
            Self::Heapq => heapq::create_module(heap, interns),
            Self::Inspect => inspect_mod::create_module(heap, interns),
            Self::BuiltinsMod => builtins_mod::create_module(heap, interns),
            Self::StringMod => string_mod::create_module(heap, interns),
            Self::Struct => struct_mod::create_module(heap, interns),
            Self::Enum => enum_mod::create_module(heap, interns),
            Self::Io => io_mod::create_module(heap, interns),
            Self::Datetime => datetime_mod::create_module(heap, interns),
            Self::Decimal => decimal_mod::create_module(heap, interns),
            Self::Fractions => fractions_mod::create_module(heap, interns),
            Self::Pprint => pprint_mod::create_module(heap, interns),
            Self::Time => time_mod::create_module(heap, interns),
            Self::Warnings => warnings_mod::create_module(heap, interns),
            Self::Logging => logging_mod::create_module(heap, interns),
            Self::Numbers => numbers_mod::create_module(heap, interns),
            Self::Types => types_mod::create_module(heap, interns),
            Self::TypingExtensions => typing_extensions::create_module(heap, interns),
            Self::CollectionsAbc => collections_abc::create_module(heap, interns),
            Self::Zlib => zlib_mod::create_module(heap, interns),
            Self::Html => html::create_module(heap, interns),
            Self::Shlex => shlex::create_module(heap, interns),
            Self::Fnmatch => fnmatch::create_module(heap, interns),
            Self::Tomllib => tomllib::create_module(heap, interns),
            Self::Urllib => urllib_mod::create_module(heap, interns),
            Self::UrllibParse => urllib_parse::create_module(heap, interns),
            Self::Difflib => difflib::create_module(heap, interns),
            Self::Ipaddress => ipaddress::create_module(heap, interns),
            Self::Threading => threading_mod::create_module(heap, interns),
            Self::Concurrent => concurrent_mod::create_module(heap, interns),
            Self::ConcurrentFutures => concurrent_futures::create_module(heap, interns),
            Self::Pickle => pickle_mod::create_module(heap, interns),
            Self::Shelve => shelve_mod::create_module(heap, interns),
            Self::Traceback => traceback_mod::create_module(heap, interns),
            Self::Secrets => secrets_mod::create_module(heap, interns),
            Self::Errno => errno_mod::create_module(heap, interns),
            Self::Linecache => linecache_mod::create_module(heap, interns),
            Self::Queue => queue_mod::create_module(heap, interns),
            Self::ArrayMod => array_mod::create_module(heap, interns),
            Self::Token => token_mod::create_module(heap, interns),
            Self::Tokenize => tokenize_mod::create_module(heap, interns),
        }
    }
}

/// All stdlib module function (but not builtins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum ModuleFunctions {
    Asyncio(asyncio::AsyncioFunctions),
    Atexit(atexit_mod::AtexitFunctions),
    Collections(collections::CollectionsFunctions),
    Copy(copy_mod::CopyFunctions),
    Dataclasses(dataclasses::DataclassesFunctions),
    Functools(functools::FunctoolsFunctions),
    Itertools(itertools::ItertoolsFunctions),
    Json(json::JsonFunctions),
    Gc(gc_mod::GcFunctions),
    Math(math::MathFunctions),
    Os(os::OsFunctions),
    OsPath(os::OsPathFunctions),
    Re(re::ReFunctions),
    Weakref(weakref::WeakrefFunctions),
    Abc(abc::AbcFunctions),
    Argparse(argparse_mod::ArgparseFunctions),
    Hashlib(hashlib::HashlibFunctions),
    Contextlib(contextlib::ContextlibFunctions),
    Statistics(statistics::StatisticsFunctions),
    Textwrap(textwrap::TextwrapFunctions),
    Uuid(uuid_mod::UuidFunctions),
    Base64(base64_mod::Base64Functions),
    Binascii(binascii_mod::BinasciiFunctions),
    Random(random_mod::RandomFunctions),
    Csv(csv_mod::CsvFunctions),
    Codecs(codecs_mod::CodecsFunctions),
    Cmath(cmath_mod::CmathFunctions),
    Operator(operator::OperatorFunctions),
    Bisect(bisect::BisectFunctions),
    Heapq(heapq::HeapqFunctions),
    Inspect(inspect_mod::InspectFunctions),
    StringMod(string_mod::StringModFunctions),
    Struct(struct_mod::StructFunctions),
    Sys(sys::SysFunctions),
    Typing(typing::TypingFunctions),
    TypingExtensions(typing_extensions::TypingExtensionsFunctions),
    Time(time_mod::TimeFunctions),
    Warnings(warnings_mod::WarningsFunctions),
    Logging(logging_mod::LoggingFunctions),
    Numbers(numbers_mod::NumbersFunctions),
    Types(types_mod::TypesFunctions),
    Zlib(zlib_mod::ZlibFunctions),
    Fnmatch(fnmatch::FnmatchFunctions),
    Difflib(difflib::DifflibFunctions),
    Html(html::HtmlFunctions),
    Shlex(shlex::ShlexFunctions),
    Tomllib(tomllib::TomllibFunctions),
    Ipaddress(ipaddress::IpaddressFunctions),
    UrllibParse(urllib_parse::UrllibParseFunctions),
    Enum(enum_mod::EnumFunctions),
    Io(io_mod::IoFunctions),
    Decimal(decimal_mod::DecimalFunctions),
    Pprint(pprint_mod::PprintFunctions),
    Threading(threading_mod::ThreadingFunctions),
    ConcurrentFutures(concurrent_futures::ConcurrentFuturesFunctions),
    Pickle(pickle_mod::PickleFunctions),
    Shelve(shelve_mod::ShelveFunctions),
    Traceback(traceback_mod::TracebackFunctions),
    Secrets(secrets_mod::SecretsFunctions),
    Linecache(linecache_mod::LinecacheFunctions),
    Queue(queue_mod::QueueFunctions),
    ArrayMod(array_mod::ArrayFunctions),
    Token(token_mod::TokenFunctions),
    Tokenize(tokenize_mod::TokenizeFunctions),
}

impl fmt::Display for ModuleFunctions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asyncio(func) => write!(f, "{func}"),
            Self::Atexit(func) => write!(f, "{func}"),
            Self::Collections(func) => write!(f, "{func}"),
            Self::Dataclasses(func) => write!(f, "{func}"),
            Self::Functools(func) => write!(f, "{func}"),
            Self::Itertools(func) => write!(f, "{func}"),
            Self::Json(func) => write!(f, "{func}"),
            Self::Gc(func) => write!(f, "{func}"),
            Self::Math(func) => write!(f, "{func}"),
            Self::Os(func) => write!(f, "{func}"),
            Self::OsPath(func) => write!(f, "{func}"),
            Self::Re(func) => write!(f, "{func}"),
            Self::Weakref(func) => write!(f, "{func}"),
            Self::Abc(func) => write!(f, "{func}"),
            Self::Argparse(func) => write!(f, "{func}"),
            Self::Hashlib(func) => write!(f, "{func}"),
            Self::Contextlib(func) => write!(f, "{func}"),
            Self::Statistics(func) => write!(f, "{func}"),
            Self::Textwrap(func) => write!(f, "{func}"),
            Self::Uuid(func) => write!(f, "{func}"),
            Self::Base64(func) => write!(f, "{func}"),
            Self::Binascii(func) => write!(f, "{func}"),
            Self::Random(func) => write!(f, "{func}"),
            Self::Csv(func) => write!(f, "{func}"),
            Self::Codecs(func) => write!(f, "{func}"),
            Self::Cmath(func) => write!(f, "{func}"),
            Self::Operator(func) => write!(f, "{func}"),
            Self::Bisect(func) => write!(f, "{func}"),
            Self::Heapq(func) => write!(f, "{func}"),
            Self::Inspect(func) => write!(f, "{func}"),
            Self::StringMod(func) => write!(f, "{func}"),
            Self::Struct(func) => write!(f, "{func}"),
            Self::Sys(func) => write!(f, "{func}"),
            Self::Typing(func) => write!(f, "{func}"),
            Self::TypingExtensions(func) => write!(f, "{func}"),
            Self::Time(func) => write!(f, "{func}"),
            Self::Warnings(func) => write!(f, "{func}"),
            Self::Logging(func) => write!(f, "{func}"),
            Self::Numbers(func) => write!(f, "{func}"),
            Self::Types(func) => write!(f, "{func}"),
            Self::Zlib(func) => write!(f, "{func}"),
            Self::Fnmatch(func) => write!(f, "{func}"),
            Self::Difflib(func) => write!(f, "{func}"),
            Self::Html(func) => write!(f, "{func}"),
            Self::Shlex(func) => write!(f, "{func}"),
            Self::Tomllib(func) => write!(f, "{func}"),
            Self::Ipaddress(func) => write!(f, "{func}"),
            Self::UrllibParse(func) => write!(f, "{func}"),
            Self::Enum(func) => write!(f, "{func}"),
            Self::Io(func) => write!(f, "{func}"),
            Self::Copy(func) => write!(f, "{func}"),
            Self::Decimal(func) => write!(f, "{func}"),
            Self::Pprint(func) => write!(f, "{func}"),
            Self::Threading(func) => write!(f, "{func}"),
            Self::ConcurrentFutures(func) => write!(f, "{func}"),
            Self::Pickle(func) => write!(f, "{func}"),
            Self::Shelve(func) => write!(f, "{func}"),
            Self::Traceback(func) => write!(f, "{func}"),
            Self::Secrets(func) => write!(f, "{func}"),
            Self::Linecache(func) => write!(f, "{func}"),
            Self::Queue(func) => write!(f, "{func}"),
            Self::ArrayMod(func) => write!(f, "{func}"),
            Self::Token(func) => write!(f, "{func}"),
            Self::Tokenize(func) => write!(f, "{func}"),
        }
    }
}

impl ModuleFunctions {
    /// Calls the module function with the given arguments.
    ///
    /// Returns `AttrCallResult` to support both immediate values and OS calls that
    /// require host involvement (e.g., `os.getenv()` needs the host to provide environment variables).
    pub fn call(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        args: ArgValues,
    ) -> RunResult<AttrCallResult> {
        match self {
            Self::Asyncio(functions) => asyncio::call(heap, interns, functions, args),
            Self::Atexit(functions) => atexit_mod::call(heap, interns, functions, args),
            Self::Collections(functions) => collections::call(heap, interns, functions, args),
            Self::Copy(functions) => copy_mod::call(heap, interns, functions, args),
            Self::Dataclasses(functions) => dataclasses::call(heap, interns, functions, args),
            Self::Functools(functions) => functools::call(heap, interns, functions, args),
            Self::Itertools(functions) => itertools::call(heap, interns, functions, args),
            Self::Json(functions) => json::call(heap, interns, functions, args),
            Self::Gc(functions) => gc_mod::call(heap, interns, functions, args),
            Self::Math(functions) => math::call(heap, interns, functions, args),
            Self::Os(functions) => os::call(heap, interns, functions, args),
            Self::OsPath(functions) => os::call_path(heap, interns, functions, args),
            Self::Re(functions) => re::call(heap, interns, functions, args),
            Self::Weakref(functions) => weakref::call(heap, interns, functions, args),
            Self::Abc(functions) => abc::call(heap, interns, functions, args),
            Self::Argparse(functions) => argparse_mod::call(heap, interns, functions, args),
            Self::Hashlib(functions) => hashlib::call(heap, interns, functions, args),
            Self::Contextlib(functions) => contextlib::call(heap, interns, functions, args),
            Self::Statistics(functions) => statistics::call(heap, interns, functions, args),
            Self::Textwrap(functions) => textwrap::call(heap, interns, functions, args),
            Self::Uuid(functions) => uuid_mod::call(heap, interns, functions, args),
            Self::Base64(functions) => base64_mod::call(heap, interns, functions, args),
            Self::Binascii(functions) => binascii_mod::call(heap, interns, functions, args),
            Self::Random(functions) => random_mod::call(heap, interns, functions, args),
            Self::Csv(functions) => csv_mod::call(heap, interns, functions, args),
            Self::Codecs(functions) => codecs_mod::call(heap, interns, functions, args),
            Self::Cmath(functions) => cmath_mod::call(heap, interns, functions, args),
            Self::Operator(functions) => operator::call(heap, interns, functions, args),
            Self::Bisect(functions) => bisect::call(heap, interns, functions, args),
            Self::Heapq(functions) => heapq::call(heap, interns, functions, args),
            Self::Inspect(functions) => inspect_mod::call(heap, interns, functions, args),
            Self::StringMod(functions) => string_mod::call(heap, interns, functions, args),
            Self::Struct(functions) => struct_mod::call(heap, interns, functions, args),
            Self::Sys(functions) => sys::call(heap, interns, functions, args),
            Self::Typing(functions) => typing::call(heap, interns, functions, args),
            Self::TypingExtensions(functions) => typing_extensions::call(heap, interns, functions, args),
            Self::Time(functions) => time_mod::call(heap, interns, functions, args),
            Self::Warnings(functions) => warnings_mod::call(heap, interns, functions, args),
            Self::Logging(functions) => logging_mod::call(heap, interns, functions, args),
            Self::Numbers(functions) => numbers_mod::call(heap, interns, functions, args),
            Self::Types(functions) => types_mod::call(heap, interns, functions, args),
            Self::Zlib(functions) => zlib_mod::call(heap, interns, functions, args),
            Self::Fnmatch(functions) => fnmatch::call(heap, interns, functions, args),
            Self::Difflib(functions) => difflib::call(heap, interns, functions, args),
            Self::Html(functions) => html::call(heap, interns, functions, args),
            Self::Shlex(functions) => shlex::call(heap, interns, functions, args),
            Self::Tomllib(functions) => tomllib::call(heap, interns, functions, args),
            Self::Ipaddress(functions) => ipaddress::call(heap, interns, functions, args),
            Self::UrllibParse(functions) => urllib_parse::call(heap, interns, functions, args),
            Self::Enum(functions) => enum_mod::call(heap, interns, functions, args),
            Self::Io(functions) => io_mod::call(heap, interns, functions, args),
            Self::Decimal(functions) => decimal_mod::call(heap, interns, functions, args),
            Self::Pprint(functions) => pprint_mod::call(heap, interns, functions, args),
            Self::Threading(functions) => threading_mod::call(heap, interns, functions, args),
            Self::ConcurrentFutures(functions) => concurrent_futures::call(heap, interns, functions, args),
            Self::Pickle(functions) => pickle_mod::call(heap, interns, functions, args),
            Self::Shelve(functions) => shelve_mod::call(heap, interns, functions, args),
            Self::Traceback(functions) => traceback_mod::call(heap, interns, functions, args),
            Self::Secrets(functions) => secrets_mod::call(heap, interns, functions, args),
            Self::Linecache(functions) => linecache_mod::call(heap, interns, functions, args),
            Self::Queue(functions) => queue_mod::call(heap, interns, functions, args),
            Self::ArrayMod(functions) => array_mod::call(heap, interns, functions, args),
            Self::Token(functions) => token_mod::call(heap, interns, functions, args),
            Self::Tokenize(functions) => tokenize_mod::call(heap, interns, functions, args),
        }
    }

    /// Writes the Python repr() string for this function to a formatter.
    pub fn py_repr_fmt<W: Write>(self, f: &mut W, py_id: usize) -> std::fmt::Result {
        write!(f, "<function {self} at 0x{py_id:x}>")
    }
}
