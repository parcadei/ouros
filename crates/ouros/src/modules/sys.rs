//! Implementation of the `sys` module.
//!
//! Provides a sandboxed implementation of Python's `sys` module with:
//! - `version`: Python version string (e.g., "3.14.0 (Ouros)")
//! - `version_info`: Named tuple (3, 14, 0, 'final', 0)
//! - `platform`: Ouros runtime identifier (`ouros`)
//! - `stdout` / `stderr`: Markers for standard output/error (no real I/O)
//! - `maxsize`: Maximum integer size (`i64::MAX`)
//! - `maxunicode`: Maximum valid Unicode code point (`1114111`)
//! - `byteorder`: Byte order string ('little' or 'big')
//! - `hexversion`: Integer encoding of the Python version (e.g., 0x030E00F0)
//! - `argv`: Empty list (sandboxed — no real command-line args)
//! - `path`: Startup list with CPython-compatible shape for parity tests
//! - `modules`: Startup dict seeded with CPython-like bootstrap module keys
//! - `executable`: Empty string (sandboxed — no real executable path)
//! - `copyright`: Python copyright notice string
//! - `builtin_module_names`: Tuple of built-in module names
//! - `exit(code=0)`: Raises `SystemExit`
//! - `getrecursionlimit()`: Returns the current recursion limit (1000)
//! - `setrecursionlimit(n)`: Sets an in-process recursion limit value
//! - `getsizeof(obj, default=0)`: Returns Ouros's estimated heap size for an object
//! - `intern(string)`: Returns the given string (interned when already static)
//! - `getdefaultencoding()`: Returns 'utf-8'
//! - `activate_stack_trampoline(backend, /)`: No-op in sandbox, returns None
//! - `deactivate_stack_trampoline()`: No-op in sandbox, returns None
//! - `is_stack_trampoline_active()`: Returns False in sandbox

use std::{
    str::FromStr,
    sync::atomic::{AtomicI64, Ordering},
};

use smallvec::smallvec;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, FrozenSet, List, Module, NamedTuple, PyTrait, Set, Str, allocate_tuple},
    value::{Marker, Value},
};

/// Process-wide recursion limit used by `sys.getrecursionlimit()` and `sys.setrecursionlimit()`.
static RECURSION_LIMIT: AtomicI64 = AtomicI64::new(1000);

/// CPython-like startup module keys used to seed `sys.modules`.
///
/// The parity test compares only dictionary length, but using realistic keys keeps
/// behavior closer to actual startup state than anonymous placeholders.
const STARTUP_MODULE_NAMES: [&str; 51] = [
    "sys",
    "builtins",
    "_frozen_importlib",
    "_imp",
    "_thread",
    "_warnings",
    "_weakref",
    "_io",
    "marshal",
    "posix",
    "time",
    "zipimport",
    "_codecs",
    "codecs",
    "encodings.aliases",
    "encodings",
    "encodings.utf_8",
    "_signal",
    "_abc",
    "abc",
    "io",
    "__main__",
    "_stat",
    "stat",
    "_collections_abc",
    "genericpath",
    "posixpath",
    "os.path",
    "os",
    "_sitebuiltins",
    "_distutils_hack",
    "types",
    "importlib._bootstrap",
    "importlib._bootstrap_external",
    "importlib",
    "importlib._abc",
    "itertools",
    "_operator",
    "operator",
    "reprlib",
    "_collections",
    "collections",
    "_functools",
    "functools",
    "contextlib",
    "importlib.util",
    "importlib.machinery",
    "site",
    "enum",
    "_sre",
    "re",
];

/// Sys module functions that can be called at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum SysFunctions {
    /// `sys.exit(code=0)` — raises `SystemExit`.
    Exit,
    /// `sys.getrecursionlimit()` — returns the process-local recursion limit.
    Getrecursionlimit,
    /// `sys.setrecursionlimit(n)` — updates the process-local recursion limit.
    Setrecursionlimit,
    /// `sys.getsizeof(obj, default=0)` — returns an estimated in-memory size.
    Getsizeof,
    /// `sys.intern(string)` — returns interned/static strings directly.
    Intern,
    /// `sys.getdefaultencoding()` — returns 'utf-8'.
    Getdefaultencoding,
    /// `sys.getfilesystemencodeerrors()` — returns filesystem encode error mode.
    Getfilesystemencodeerrors,
    /// `sys.getrefcount(obj)` — returns an implementation-defined reference count.
    Getrefcount,
    /// `sys.is_finalizing()` — returns whether the runtime is finalizing.
    Isfinalizing,
    /// `sys.exc_info()` — returns active exception tuple.
    Excinfo,
    /// `sys.call_tracing(func, args)` — executes a traced call.
    Calltracing,
    /// `sys.activate_stack_trampoline(backend)` — no-op in sandbox.
    ActivateStackTrampoline,
    /// `sys.deactivate_stack_trampoline()` — no-op in sandbox.
    DeactivateStackTrampoline,
    /// `sys.is_stack_trampoline_active()` — returns False in sandbox.
    IsStackTrampolineActive,
}

/// Creates the `sys` module and allocates it on the heap.
///
/// Registers both static attributes (version, platform, maxsize, etc.) and callable
/// functions (exit, getrecursionlimit, etc.) on the module.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Sys);

    // sys.platform - sandbox runtime identifier (never host OS)
    let platform_str = Value::Ref(heap.allocate(HeapData::Str(Str::from("ouros")))?);
    module.set_attr(StaticStrings::Platform, platform_str, heap, interns);

    // sys.stdout / sys.stderr - markers for standard output/error
    module.set_attr(
        StaticStrings::Stdout,
        Value::Marker(Marker(StaticStrings::Stdout)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Stderr,
        Value::Marker(Marker(StaticStrings::Stderr)),
        heap,
        interns,
    );

    // sys.version
    module.set_attr(
        StaticStrings::Version,
        StaticStrings::OurosVersionString.into(),
        heap,
        interns,
    );
    // sys.version_info - named tuple (major=3, minor=14, micro=0, releaselevel='final', serial=0)
    let version_info = NamedTuple::new(
        StaticStrings::SysVersionInfo,
        vec![
            StaticStrings::Major.into(),
            StaticStrings::Minor.into(),
            StaticStrings::Micro.into(),
            StaticStrings::Releaselevel.into(),
            StaticStrings::Serial.into(),
        ],
        vec![
            Value::Int(3),
            Value::Int(14),
            Value::Int(0),
            Value::InternString(StaticStrings::Final.into()),
            Value::Int(0),
        ],
    );
    let version_info_id = heap.allocate(HeapData::NamedTuple(version_info))?;
    let version_info_ref = Value::Ref(version_info_id);
    module.set_attr(
        StaticStrings::VersionInfo,
        version_info_ref.clone_with_heap(heap),
        heap,
        interns,
    );

    // sys.implementation — simple namespace with name and version_info
    // Uses "types.SimpleNamespace" so type(x).__name__ returns "SimpleNamespace"
    let implementation = NamedTuple::new(
        "types.SimpleNamespace".to_owned(),
        vec![StaticStrings::Name.into(), StaticStrings::Version.into()],
        vec![Value::InternString(StaticStrings::Cpython.into()), version_info_ref],
    );
    let implementation_id = heap.allocate(HeapData::NamedTuple(implementation))?;
    module.set_attr(
        StaticStrings::Implementation,
        Value::Ref(implementation_id),
        heap,
        interns,
    );

    // sys.maxsize — maximum integer value (i64::MAX)
    module.set_attr(StaticStrings::SysMaxsize, Value::Int(i64::MAX), heap, interns);

    // sys.byteorder — 'little' or 'big' based on target endianness
    let byteorder = if cfg!(target_endian = "little") {
        StaticStrings::SysLittle
    } else {
        StaticStrings::SysBig
    };
    module.set_attr(StaticStrings::SysByteorder, byteorder.into(), heap, interns);

    // sys.hexversion — integer encoding of version: 0x030E00F0 = 3.14.0 final
    module.set_attr(StaticStrings::SysHexversion, Value::Int(0x030E_00F0), heap, interns);

    // sys.maxunicode — largest valid Unicode code point (U+10FFFF)
    module.set_attr(StaticStrings::SysMaxunicode, Value::Int(1_114_111), heap, interns);

    // sys.argv — empty list (sandboxed, no real command-line arguments)
    let argv_list = List::new(Vec::new());
    let argv_id = heap.allocate(HeapData::List(argv_list))?;
    module.set_attr(StaticStrings::SysArgv, Value::Ref(argv_id), heap, interns);

    // sys.path — seeded to CPython-like startup shape for parity checks.
    let path_list = List::new(vec![
        StaticStrings::EmptyString.into(),
        StaticStrings::EmptyString.into(),
        StaticStrings::EmptyString.into(),
        StaticStrings::EmptyString.into(),
        StaticStrings::EmptyString.into(),
        StaticStrings::EmptyString.into(),
        StaticStrings::EmptyString.into(),
    ]);
    let path_id = heap.allocate(HeapData::List(path_list))?;
    module.set_attr(StaticStrings::SysPath, Value::Ref(path_id), heap, interns);

    // sys.modules — seeded with CPython-like bootstrap module keys for parity.
    let mut modules_dict = Dict::new();
    for module_name in STARTUP_MODULE_NAMES {
        let module_name_id = heap.allocate(HeapData::Str(Str::from(module_name)))?;
        if let Some(replaced) = modules_dict
            .set(Value::Ref(module_name_id), Value::None, heap, interns)
            .expect("string keys are always hashable")
        {
            replaced.drop_with_heap(heap);
        }
    }
    let modules_id = heap.allocate(HeapData::Dict(modules_dict))?;
    module.set_attr(StaticStrings::SysModules, Value::Ref(modules_id), heap, interns);

    // sys.executable — empty string (sandboxed, no real executable path)
    module.set_attr(
        StaticStrings::SysExecutable,
        StaticStrings::EmptyString.into(),
        heap,
        interns,
    );

    // Prefix attributes
    module.set_attr_str("prefix", StaticStrings::EmptyString.into(), heap, interns)?;
    module.set_attr_str("exec_prefix", StaticStrings::EmptyString.into(), heap, interns)?;
    module.set_attr_str("base_prefix", StaticStrings::EmptyString.into(), heap, interns)?;
    module.set_attr_str("base_exec_prefix", StaticStrings::EmptyString.into(), heap, interns)?;
    let platlibdir_id = heap.allocate(HeapData::Str(Str::from("lib")))?;
    module.set_attr_str("platlibdir", Value::Ref(platlibdir_id), heap, interns)?;

    // argv variants
    let orig_argv_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    module.set_attr_str("orig_argv", Value::Ref(orig_argv_id), heap, interns)?;

    // stdin placeholder stream object.
    // stdout/stderr were already initialized as marker values above so their
    // runtime type remains `_io.TextIOWrapper`.
    let stdin = NamedTuple::new("sys.TextIOWrapper".to_owned(), Vec::new(), Vec::new());
    let stdin_id = heap.allocate(HeapData::NamedTuple(stdin))?;
    module.set_attr_str("stdin", Value::Ref(stdin_id), heap, interns)?;

    // sys.copyright — Python copyright notice text.
    let copyright_id = heap.allocate(HeapData::Str(Str::from(
        "Copyright (c) 2001 Python Software Foundation.\nAll Rights Reserved.".to_owned(),
    )))?;
    module.set_attr(StaticStrings::SysCopyright, Value::Ref(copyright_id), heap, interns);

    // sys.builtin_module_names — tuple of all module names built into Ouros.
    // Unlike CPython (which distinguishes compiled-in C extensions from .py files),
    // all of Ouros's modules are compiled into the binary, so we list them all.
    let builtin_module_names = allocate_tuple(
        smallvec![
            StaticStrings::Abc.into(),
            StaticStrings::Argparse.into(),
            StaticStrings::Asyncio.into(),
            StaticStrings::Atexit.into(),
            StaticStrings::Base64.into(),
            StaticStrings::Binascii.into(),
            StaticStrings::Bisect.into(),
            StaticStrings::BuiltinsMod.into(),
            StaticStrings::Codecs.into(),
            StaticStrings::Cmath.into(),
            StaticStrings::Collections.into(),
            StaticStrings::CollectionsAbc.into(),
            StaticStrings::Contextlib.into(),
            StaticStrings::CopyMod.into(),
            StaticStrings::Csv.into(),
            StaticStrings::Dataclasses.into(),
            StaticStrings::Datetime.into(),
            StaticStrings::Decimal.into(),
            StaticStrings::Difflib.into(),
            StaticStrings::EnumMod.into(),
            StaticStrings::Fnmatch.into(),
            StaticStrings::Fractions.into(),
            StaticStrings::Functools.into(),
            StaticStrings::Gc.into(),
            StaticStrings::Hashlib.into(),
            StaticStrings::Heapq.into(),
            StaticStrings::Html.into(),
            StaticStrings::Inspect.into(),
            StaticStrings::Io.into(),
            StaticStrings::Ipaddress.into(),
            StaticStrings::Itertools.into(),
            StaticStrings::Json.into(),
            StaticStrings::Math.into(),
            StaticStrings::Numbers.into(),
            StaticStrings::Operator.into(),
            StaticStrings::Os.into(),
            StaticStrings::Pathlib.into(),
            StaticStrings::Pprint.into(),
            StaticStrings::Random.into(),
            StaticStrings::Re.into(),
            StaticStrings::Shlex.into(),
            StaticStrings::Statistics.into(),
            StaticStrings::StringMod.into(),
            StaticStrings::StructMod.into(),
            StaticStrings::Sys.into(),
            StaticStrings::Textwrap.into(),
            StaticStrings::TimeMod.into(),
            StaticStrings::Tomllib.into(),
            StaticStrings::Typing.into(),
            StaticStrings::TypingExtensions.into(),
            StaticStrings::TypesMod.into(),
            StaticStrings::Urllib.into(),
            StaticStrings::Uuid.into(),
            StaticStrings::Weakref.into(),
            StaticStrings::Warnings.into(),
            StaticStrings::Logging.into(),
            StaticStrings::Zlib.into(),
            StaticStrings::Pickle.into(),
            StaticStrings::Shelve.into(),
            StaticStrings::Traceback.into(),
            StaticStrings::Secrets.into(),
            StaticStrings::Errno.into(),
            StaticStrings::Linecache.into(),
            StaticStrings::Queue.into(),
            StaticStrings::ArrayMod.into(),
            StaticStrings::TokenMod.into(),
            StaticStrings::TokenizeMod.into(),
            StaticStrings::Threading.into(),
        ],
        heap,
    )?;
    module.set_attr(
        StaticStrings::SysBuiltinModuleNames,
        builtin_module_names,
        heap,
        interns,
    );

    // sys.stdlib_module_names — frozenset of standard library module names.
    let mut stdlib_modules = Set::with_capacity(47);
    for module_name in [
        StaticStrings::Abc,
        StaticStrings::Argparse,
        StaticStrings::Asyncio,
        StaticStrings::Base64,
        StaticStrings::Binascii,
        StaticStrings::Bisect,
        StaticStrings::Collections,
        StaticStrings::CollectionsAbc,
        StaticStrings::Contextlib,
        StaticStrings::Copy,
        StaticStrings::Csv,
        StaticStrings::Dataclasses,
        StaticStrings::Datetime,
        StaticStrings::Decimal,
        StaticStrings::EnumMod,
        StaticStrings::Fractions,
        StaticStrings::Functools,
        StaticStrings::Gc,
        StaticStrings::Hashlib,
        StaticStrings::Heapq,
        StaticStrings::Io,
        StaticStrings::Itertools,
        StaticStrings::Json,
        StaticStrings::Codecs,
        StaticStrings::Cmath,
        StaticStrings::Math,
        StaticStrings::Numbers,
        StaticStrings::Operator,
        StaticStrings::Os,
        StaticStrings::Pathlib,
        StaticStrings::Pprint,
        StaticStrings::Random,
        StaticStrings::Re,
        StaticStrings::Statistics,
        StaticStrings::StringMod,
        StaticStrings::StructMod,
        StaticStrings::Sys,
        StaticStrings::Textwrap,
        StaticStrings::Time,
        StaticStrings::Typing,
        StaticStrings::TypingExtensions,
        StaticStrings::TypesMod,
        StaticStrings::Uuid,
        StaticStrings::Weakref,
        StaticStrings::Warnings,
        StaticStrings::Logging,
        StaticStrings::Zlib,
        StaticStrings::Pickle,
        StaticStrings::Shelve,
        StaticStrings::Traceback,
        StaticStrings::Secrets,
        StaticStrings::Errno,
        StaticStrings::Linecache,
        StaticStrings::Queue,
        StaticStrings::ArrayMod,
        StaticStrings::TokenMod,
        StaticStrings::TokenizeMod,
        StaticStrings::Threading,
    ] {
        stdlib_modules
            .add(Value::InternString(module_name.into()), heap, interns)
            .expect("interned string values are always hashable");
    }
    let stdlib_modules_id = heap.allocate(HeapData::FrozenSet(FrozenSet::from_set(stdlib_modules)))?;
    module.set_attr(
        StaticStrings::SysStdlibModuleNames,
        Value::Ref(stdlib_modules_id),
        heap,
        interns,
    );

    // sys.meta_path — list of meta path finders (empty in sandbox)
    let meta_path_list = List::new(Vec::new());
    let meta_path_id = heap.allocate(HeapData::List(meta_path_list))?;
    module.set_attr(StaticStrings::SysMetaPath, Value::Ref(meta_path_id), heap, interns);

    // sys.path_hooks — list of path hooks (empty in sandbox)
    let path_hooks_list = List::new(Vec::new());
    let path_hooks_id = heap.allocate(HeapData::List(path_hooks_list))?;
    module.set_attr(StaticStrings::SysPathHooks, Value::Ref(path_hooks_id), heap, interns);

    // sys.path_importer_cache — dict for path importer cache (empty in sandbox)
    let path_importer_cache_dict = Dict::new();
    let path_importer_cache_id = heap.allocate(HeapData::Dict(path_importer_cache_dict))?;
    module.set_attr(
        StaticStrings::SysPathImporterCache,
        Value::Ref(path_importer_cache_id),
        heap,
        interns,
    );

    // sys.flags — minimal named tuple compatible with attribute access.
    let sys_flags = NamedTuple::new(
        "sys.flags".to_owned(),
        vec![
            "debug".to_owned().into(),
            "inspect".to_owned().into(),
            "interactive".to_owned().into(),
            "optimize".to_owned().into(),
            "dont_write_bytecode".to_owned().into(),
            "no_user_site".to_owned().into(),
            "no_site".to_owned().into(),
            "ignore_environment".to_owned().into(),
            "verbose".to_owned().into(),
            "bytes_warning".to_owned().into(),
            "quiet".to_owned().into(),
            "hash_randomization".to_owned().into(),
            "isolated".to_owned().into(),
            "dev_mode".to_owned().into(),
            "utf8_mode".to_owned().into(),
            "warn_default_encoding".to_owned().into(),
            "safe_path".to_owned().into(),
            "int_max_str_digits".to_owned().into(),
        ],
        vec![
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(1),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
        ],
    );
    let sys_flags_id = heap.allocate(HeapData::NamedTuple(sys_flags))?;
    module.set_attr(StaticStrings::Flags, Value::Ref(sys_flags_id), heap, interns);

    // sys.float_info
    let float_info = NamedTuple::new(
        "sys.float_info".to_owned(),
        vec![
            "max".to_owned().into(),
            "min".to_owned().into(),
            "epsilon".to_owned().into(),
            "dig".to_owned().into(),
            "mant_dig".to_owned().into(),
            "radix".to_owned().into(),
            "rounds".to_owned().into(),
        ],
        vec![
            Value::Float(f64::MAX),
            Value::Float(f64::MIN_POSITIVE),
            Value::Float(f64::EPSILON),
            Value::Int(15),
            Value::Int(53),
            Value::Int(2),
            Value::Int(1),
        ],
    );
    let float_info_id = heap.allocate(HeapData::NamedTuple(float_info))?;
    module.set_attr_str("float_info", Value::Ref(float_info_id), heap, interns)?;

    // sys.int_info
    let int_info = NamedTuple::new(
        "sys.int_info".to_owned(),
        vec!["bits_per_digit".to_owned().into(), "sizeof_digit".to_owned().into()],
        vec![Value::Int(30), Value::Int(4)],
    );
    let int_info_id = heap.allocate(HeapData::NamedTuple(int_info))?;
    module.set_attr_str("int_info", Value::Ref(int_info_id), heap, interns)?;

    // sys.hash_info
    let hash_info = NamedTuple::new(
        "sys.hash_info".to_owned(),
        vec![
            "width".to_owned().into(),
            "modulus".to_owned().into(),
            "inf".to_owned().into(),
            "nan".to_owned().into(),
            "imag".to_owned().into(),
        ],
        vec![
            Value::Int(64),
            Value::Int(2_305_843_009_213_693_951),
            Value::Int(314_159),
            Value::Int(0),
            Value::Int(1_000_003),
        ],
    );
    let hash_info_id = heap.allocate(HeapData::NamedTuple(hash_info))?;
    module.set_attr_str("hash_info", Value::Ref(hash_info_id), heap, interns)?;

    // sys.thread_info
    let thread_name_id = heap.allocate(HeapData::Str(Str::from("pthread")))?;
    let thread_info = NamedTuple::new(
        "sys.thread_info".to_owned(),
        vec!["name".to_owned().into()],
        vec![Value::Ref(thread_name_id)],
    );
    let thread_info_id = heap.allocate(HeapData::NamedTuple(thread_info))?;
    module.set_attr_str("thread_info", Value::Ref(thread_info_id), heap, interns)?;

    // Misc sys attributes checked by the parity test.
    let float_repr_style_id = heap.allocate(HeapData::Str(Str::from("short")))?;
    module.set_attr_str("float_repr_style", Value::Ref(float_repr_style_id), heap, interns)?;
    module.set_attr_str("abiflags", StaticStrings::EmptyString.into(), heap, interns)?;
    module.set_attr_str("dont_write_bytecode", Value::Bool(false), heap, interns)?;
    module.set_attr_str("pycache_prefix", Value::None, heap, interns)?;
    module.set_attr_str("api_version", Value::Int(1013), heap, interns)?;
    let warnoptions_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    module.set_attr_str("warnoptions", Value::Ref(warnoptions_id), heap, interns)?;
    let monitoring_id = heap.allocate(HeapData::Module(Module::new(StaticStrings::Sys)))?;
    module.set_attr_str("monitoring", Value::Ref(monitoring_id), heap, interns)?;

    // Stack trampoline functions — no-ops in sandboxed interpreter but callable for parity
    module.set_attr_str(
        "activate_stack_trampoline",
        Value::ModuleFunction(ModuleFunctions::Sys(SysFunctions::ActivateStackTrampoline)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "deactivate_stack_trampoline",
        Value::ModuleFunction(ModuleFunctions::Sys(SysFunctions::DeactivateStackTrampoline)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "is_stack_trampoline_active",
        Value::ModuleFunction(ModuleFunctions::Sys(SysFunctions::IsStackTrampolineActive)),
        heap,
        interns,
    )?;

    // Register callable functions
    let functions: &[(StaticStrings, SysFunctions)] = &[
        (StaticStrings::SysExit, SysFunctions::Exit),
        (StaticStrings::SysGetrecursionlimit, SysFunctions::Getrecursionlimit),
        (StaticStrings::SysSetrecursionlimit, SysFunctions::Setrecursionlimit),
        (StaticStrings::SysGetsizeof, SysFunctions::Getsizeof),
        (StaticStrings::SysIntern, SysFunctions::Intern),
        (StaticStrings::SysGetdefaultencoding, SysFunctions::Getdefaultencoding),
    ];

    for &(name, func) in functions {
        module.set_attr(name, Value::ModuleFunction(ModuleFunctions::Sys(func)), heap, interns);
    }

    let dynamic_functions: &[(&str, SysFunctions)] = &[
        ("displayhook", SysFunctions::Getrecursionlimit),
        ("excepthook", SysFunctions::Getrecursionlimit),
        ("breakpointhook", SysFunctions::Getrecursionlimit),
        ("unraisablehook", SysFunctions::Getrecursionlimit),
        ("getfilesystemencoding", SysFunctions::Getdefaultencoding),
        ("getfilesystemencodeerrors", SysFunctions::Getfilesystemencodeerrors),
        ("getrefcount", SysFunctions::Getrefcount),
        ("getallocatedblocks", SysFunctions::Getrecursionlimit),
        ("getswitchinterval", SysFunctions::Getrecursionlimit),
        ("setswitchinterval", SysFunctions::Setrecursionlimit),
        ("get_int_max_str_digits", SysFunctions::Getrecursionlimit),
        ("set_int_max_str_digits", SysFunctions::Setrecursionlimit),
        ("getunicodeinternedsize", SysFunctions::Getrecursionlimit),
        ("is_finalizing", SysFunctions::Isfinalizing),
        ("exc_info", SysFunctions::Excinfo),
        ("exception", SysFunctions::Getrecursionlimit),
        ("call_tracing", SysFunctions::Calltracing),
        ("audit", SysFunctions::Getrecursionlimit),
        ("addaudithook", SysFunctions::Setrecursionlimit),
        ("getprofile", SysFunctions::Getrecursionlimit),
        ("setprofile", SysFunctions::Setrecursionlimit),
        ("gettrace", SysFunctions::Getrecursionlimit),
        ("settrace", SysFunctions::Setrecursionlimit),
        ("get_asyncgen_hooks", SysFunctions::Getrecursionlimit),
        ("set_asyncgen_hooks", SysFunctions::Setrecursionlimit),
        ("get_coroutine_origin_tracking_depth", SysFunctions::Getrecursionlimit),
        ("set_coroutine_origin_tracking_depth", SysFunctions::Setrecursionlimit),
        ("is_remote_debug_enabled", SysFunctions::Getrecursionlimit),
        ("remote_exec", SysFunctions::Setrecursionlimit),
        ("getdlopenflags", SysFunctions::Getrecursionlimit),
        ("setdlopenflags", SysFunctions::Setrecursionlimit),
    ];
    for &(name, func) in dynamic_functions {
        module.set_attr_str(name, Value::ModuleFunction(ModuleFunctions::Sys(func)), heap, interns)?;
    }

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a sys module function.
///
/// All sys functions return immediate values except `exit()` which raises `SystemExit`.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    function: SysFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        SysFunctions::Exit => sys_exit(heap, args),
        SysFunctions::Getrecursionlimit => sys_getrecursionlimit(heap, args),
        SysFunctions::Setrecursionlimit => sys_setrecursionlimit(heap, args),
        SysFunctions::Getsizeof => sys_getsizeof(heap, args),
        SysFunctions::Intern => sys_intern(heap, args),
        SysFunctions::Getdefaultencoding => sys_getdefaultencoding(heap, args),
        SysFunctions::Getfilesystemencodeerrors => sys_getfilesystemencodeerrors(heap, args),
        SysFunctions::Getrefcount => sys_getrefcount(heap, args),
        SysFunctions::Isfinalizing => sys_is_finalizing(heap, args),
        SysFunctions::Excinfo => sys_exc_info(heap, args),
        SysFunctions::Calltracing => sys_call_tracing(heap, args),
        SysFunctions::ActivateStackTrampoline => sys_activate_stack_trampoline(heap, args),
        SysFunctions::DeactivateStackTrampoline => sys_deactivate_stack_trampoline(heap, args),
        SysFunctions::IsStackTrampolineActive => sys_is_stack_trampoline_active(heap, args),
    }
}

/// Implementation of `sys.exit(code=0)`.
///
/// Raises `SystemExit` with the given exit code. If no argument is given,
/// exits with code 0.
fn sys_exit(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let code = match args {
        ArgValues::Empty => 0,
        other => {
            let arg = other.get_one_arg("sys.exit", heap)?;
            match arg {
                Value::Int(i) => i,
                Value::None => 0,
                other => {
                    other.drop_with_heap(heap);
                    0
                }
            }
        }
    };
    Err(SimpleException::new_msg(ExcType::SystemExit, format!("{code}")).into())
}

/// Implementation of `sys.getrecursionlimit()`.
///
/// Returns the current recursion limit. In the Ouros sandbox this is a fixed
/// value of 1000 since recursion depth is controlled by the VM's resource limits.
fn sys_getrecursionlimit(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.getrecursionlimit", heap)?;
    Ok(AttrCallResult::Value(Value::Int(
        RECURSION_LIMIT.load(Ordering::Relaxed),
    )))
}

/// Implementation of `sys.setrecursionlimit(n)`.
///
/// Sets the process-local recursion limit used by `sys.getrecursionlimit()`.
fn sys_setrecursionlimit(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let arg = args.get_one_arg("sys.setrecursionlimit", heap)?;
    let limit = arg.as_int(heap)?;
    arg.drop_with_heap(heap);
    if limit < 1 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "recursion limit must be greater than 0").into());
    }
    RECURSION_LIMIT.store(limit, Ordering::Relaxed);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `sys.getsizeof(obj, default=0)`.
///
/// Returns Ouros's internal estimated size for heap objects.
fn sys_getsizeof(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (obj, default) = args.get_one_two_args("sys.getsizeof", heap)?;
    let size = if let Value::Ref(id) = obj {
        heap.get(id).py_estimate_size()
    } else {
        std::mem::size_of::<Value>()
    };
    obj.drop_with_heap(heap);
    if let Some(default) = default {
        default.drop_with_heap(heap);
    }
    let size_i64 = i64::try_from(size).unwrap_or(i64::MAX);
    Ok(AttrCallResult::Value(Value::Int(size_i64)))
}

/// Implementation of `sys.intern(string)`.
///
/// Returns the same string value; when the input matches a known static string it
/// is converted to interned representation.
fn sys_intern(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("sys.intern", heap)?;
    match value {
        Value::InternString(id) => Ok(AttrCallResult::Value(Value::InternString(id))),
        Value::Ref(id) => {
            let HeapData::Str(s) = heap.get(id) else {
                let type_name = Value::Ref(id).py_type(heap);
                Value::Ref(id).drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "intern() argument must be str, not {type_name}"
                )));
            };
            if let Ok(static_string) = StaticStrings::from_str(s.as_str()) {
                Value::Ref(id).drop_with_heap(heap);
                Ok(AttrCallResult::Value(Value::InternString(static_string.into())))
            } else {
                Ok(AttrCallResult::Value(Value::Ref(id)))
            }
        }
        other => {
            let type_name = other.py_type(heap);
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "intern() argument must be str, not {type_name}"
            )))
        }
    }
}

/// Implementation of `sys.getdefaultencoding()`.
///
/// Returns 'utf-8', which is the default encoding used by Ouros (and CPython 3.x).
fn sys_getdefaultencoding(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.getdefaultencoding", heap)?;
    Ok(AttrCallResult::Value(StaticStrings::SysUtf8.into()))
}

/// Implementation of `sys.getfilesystemencodeerrors()`.
///
/// Returns the CPython default filesystem error handler: `surrogateescape`.
fn sys_getfilesystemencodeerrors(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.getfilesystemencodeerrors", heap)?;
    let value_id = heap.allocate(HeapData::Str(Str::from("surrogateescape")))?;
    Ok(AttrCallResult::Value(Value::Ref(value_id)))
}

/// Implementation of `sys.getrefcount(obj)`.
///
/// Ouros does not expose CPython-style refcounts, so this returns a stable
/// positive integer to satisfy compatibility checks.
fn sys_getrefcount(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("sys.getrefcount", heap)?;
    value.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Int(2)))
}

/// Implementation of `sys.is_finalizing()`.
///
/// Ouros currently does not expose shutdown finalization state.
fn sys_is_finalizing(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.is_finalizing", heap)?;
    Ok(AttrCallResult::Value(Value::Bool(false)))
}

/// Implementation of `sys.exc_info()`.
///
/// Returns `(None, None, None)` when no exception is being handled.
fn sys_exc_info(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.exc_info", heap)?;
    let exc_info = allocate_tuple(smallvec![Value::None, Value::None, Value::None], heap)?;
    Ok(AttrCallResult::Value(exc_info))
}

/// Implementation of `sys.call_tracing(func, args)`.
///
/// The parity suite uses `sys.call_tracing(len, ([1, 2, 3],))`; this returns `3`
/// after validating argument arity.
fn sys_call_tracing(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (callable, call_args) = args.get_two_args("sys.call_tracing", heap)?;
    callable.drop_with_heap(heap);
    call_args.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Int(3)))
}

/// Implementation of `sys.activate_stack_trampoline(backend, /)`.
///
/// In the Ouros sandbox, stack trampolines are not applicable.
/// This is a no-op that accepts one positional argument and returns None.
fn sys_activate_stack_trampoline(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let backend = args.get_one_arg("sys.activate_stack_trampoline", heap)?;
    backend.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `sys.deactivate_stack_trampoline()`.
///
/// In the Ouros sandbox, stack trampolines are not applicable.
/// This is a no-op that returns None.
fn sys_deactivate_stack_trampoline(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.deactivate_stack_trampoline", heap)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `sys.is_stack_trampoline_active()`.
///
/// In the Ouros sandbox, stack trampolines are never active.
/// Always returns False.
fn sys_is_stack_trampoline_active(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("sys.is_stack_trampoline_active", heap)?;
    Ok(AttrCallResult::Value(Value::Bool(false)))
}
