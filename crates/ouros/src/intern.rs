//! String, bytes, and long integer interning for efficient storage of literals and identifiers.
//!
//! This module provides interners that store unique strings, bytes, and long integers in vectors
//! and return indices (`StringId`, `BytesId`, `LongIntId`) for efficient storage and comparison.
//! This avoids the overhead of cloning strings or using atomic reference counting.
//!
//! The interners are populated during parsing and preparation, then owned by the `Executor`.
//! During execution, lookups are needed only for error messages and repr output.
//!
//! StringIds are laid out as follows:
//! * 0 to 128 - single character strings for all 128 ASCII characters
//! * 1000 to count(StaticStrings) - strings StaticStrings
//! * 10_000+ - strings interned per executor

use std::{str::FromStr, sync::LazyLock};

use ahash::AHashMap;
use num_bigint::BigInt;
use strum::{EnumString, FromRepr, IntoStaticStr};

use crate::{function::Function, value::Value};

/// Index into the string interner's storage.
///
/// Uses `u32` to save space (4 bytes vs 8 bytes for `usize`). This limits us to
/// ~4 billion unique interns, which is more than sufficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct StringId(u32);

impl StringId {
    /// Creates a StringId from a raw index value.
    ///
    /// Used by the bytecode VM to reconstruct StringIds from operands stored
    /// in bytecode. The caller is responsible for ensuring the index is valid.
    #[inline]
    pub fn from_index(index: u16) -> Self {
        Self(u32::from(index))
    }

    /// Returns the raw index value.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns the StringId for an ASCII byte.
    #[must_use]
    pub fn from_ascii(byte: u8) -> Self {
        Self(u32::from(byte))
    }
}

/// StringId offsets
const STATIC_STRING_ID_OFFSET: u32 = 1000;
const INTERN_STRING_ID_OFFSET: usize = 10_000;

/// Static strings for all 128 ASCII characters, built once on first access.
///
/// Uses `LazyLock` to build the array at runtime (once), leaking the strings to get
/// `'static` lifetime. The leak is intentional and bounded (128 single-byte strings).
static ASCII_STRS: LazyLock<[&'static str; 128]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        // Safe: i is always 0-127 for a 128-element array
        let s = char::from(u8::try_from(i).expect("index out of u8 range")).to_string();
        // Leak to get 'static lifetime - this is intentional and bounded (128 bytes total)
        // Reborrow as immutable since we won't mutate
        &*Box::leak(s.into_boxed_str())
    })
});

/// Static string values which are known at compile time and don't need to be interned.
#[repr(u16)]
#[derive(
    Debug, Clone, Copy, FromRepr, EnumString, IntoStaticStr, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
pub enum StaticStrings {
    #[strum(serialize = "")]
    EmptyString,
    #[strum(serialize = "<module>")]
    Module,
    // ==========================
    // List methods
    // Also uses shared: POP, CLEAR, COPY, REMOVE
    // Also uses string-shared: INDEX, COUNT
    Append,
    Insert,
    Extend,
    Reverse,
    Sort,

    // ==========================
    // Dict methods
    // Also uses shared: POP, CLEAR, COPY, UPDATE
    Get,
    Keys,
    Values,
    Value,
    Items,
    Setdefault,
    Popitem,
    Fromkeys,
    // OrderedDict methods
    #[strum(serialize = "move_to_end")]
    MoveToEnd,

    // ==========================
    // NamedTuple helpers
    #[strum(serialize = "_fields")]
    NamedTupleFields,
    #[strum(serialize = "_make")]
    NamedTupleMake,
    #[strum(serialize = "_asdict")]
    NamedTupleAsDict,
    #[strum(serialize = "_replace")]
    NamedTupleReplace,

    // ==========================
    // Deque methods
    #[strum(serialize = "appendleft")]
    Appendleft,
    #[strum(serialize = "popleft")]
    Popleft,
    #[strum(serialize = "extendleft")]
    Extendleft,
    #[strum(serialize = "rotate")]
    Rotate,
    #[strum(serialize = "maxlen")]
    Maxlen,

    // ==========================
    // Shared methods
    // Used by multiple container types: list, dict, set
    Pop,
    Clear,
    Copy,
    Send,
    Throw,
    Close,

    // ==========================
    // Set methods
    // Also uses shared: POP, CLEAR, COPY
    Add,
    Remove,
    Discard,
    Update,
    #[strum(serialize = "intersection_update")]
    IntersectionUpdate,
    #[strum(serialize = "difference_update")]
    DifferenceUpdate,
    #[strum(serialize = "symmetric_difference_update")]
    SymmetricDifferenceUpdate,
    Union,
    Intersection,
    Difference,
    #[strum(serialize = "symmetric_difference")]
    SymmetricDifference,
    Issubset,
    Issuperset,
    Isdisjoint,

    // ==========================
    // Counter methods
    #[strum(serialize = "most_common")]
    MostCommon,
    #[strum(serialize = "elements")]
    Elements,
    #[strum(serialize = "subtract")]
    Subtract,
    #[strum(serialize = "total")]
    Total,

    // ==========================
    // String methods
    // Some methods shared with bytes: FIND, INDEX, COUNT, STARTSWITH, ENDSWITH
    // Some methods shared with list/tuple: INDEX, COUNT
    Join,
    // Simple transformations
    Lower,
    Upper,
    Capitalize,
    Title,
    Swapcase,
    Casefold,
    // Predicate methods
    Isalpha,
    Isdigit,
    Isalnum,
    Isnumeric,
    Isspace,
    Islower,
    Isupper,
    Isascii,
    Isdecimal,
    Isprintable,
    // Search methods (some shared with bytes, list, tuple)
    Find,
    Rfind,
    Index,
    Rindex,
    Count,
    Startswith,
    Endswith,
    // Strip/trim methods
    Strip,
    Lstrip,
    Rstrip,
    Removeprefix,
    Removesuffix,
    // Split methods
    Split,
    Rsplit,
    Splitlines,
    Partition,
    Rpartition,
    // Replace/padding methods
    Replace,
    Center,
    Ljust,
    Rjust,
    Zfill,
    // Additional string methods
    Encode,
    Isidentifier,
    Istitle,
    Expandtabs,
    FormatMap,
    Translate,

    // ==========================
    // Bytes methods
    // Also uses string-shared: FIND, INDEX, COUNT, STARTSWITH, ENDSWITH
    // Also uses most string methods: LOWER, UPPER, CAPITALIZE, TITLE, SWAPCASE,
    // ISALPHA, ISDIGIT, ISALNUM, ISSPACE, ISLOWER, ISUPPER, ISASCII, ISTITLE,
    // RFIND, RINDEX, STRIP, LSTRIP, RSTRIP, REMOVEPREFIX, REMOVESUFFIX,
    // SPLIT, RSPLIT, SPLITLINES, PARTITION, RPARTITION, REPLACE,
    // CENTER, LJUST, RJUST, ZFILL, JOIN
    Decode,
    Hex,
    Fromhex,

    // ==========================
    // sys module strings
    #[strum(serialize = "sys")]
    Sys,
    #[strum(serialize = "sys.version_info")]
    SysVersionInfo,
    #[strum(serialize = "version")]
    Version,
    #[strum(serialize = "version_info")]
    VersionInfo,
    #[strum(serialize = "platform")]
    Platform,
    #[strum(serialize = "stdout")]
    Stdout,
    #[strum(serialize = "stderr")]
    Stderr,
    #[strum(serialize = "major")]
    Major,
    #[strum(serialize = "minor")]
    Minor,
    #[strum(serialize = "micro")]
    Micro,
    #[strum(serialize = "releaselevel")]
    Releaselevel,
    #[strum(serialize = "serial")]
    Serial,
    #[strum(serialize = "final")]
    Final,
    #[strum(serialize = "3.14.0 (Ouros)")]
    OurosVersionString,
    #[strum(serialize = "ouros")]
    Ouros,
    #[strum(serialize = "maxsize")]
    SysMaxsize,
    #[strum(serialize = "byteorder")]
    SysByteorder,
    #[strum(serialize = "little")]
    SysLittle,
    #[strum(serialize = "big")]
    SysBig,
    #[strum(serialize = "exit")]
    SysExit,
    #[strum(serialize = "getrecursionlimit")]
    SysGetrecursionlimit,
    #[strum(serialize = "setrecursionlimit")]
    SysSetrecursionlimit,
    #[strum(serialize = "getsizeof")]
    SysGetsizeof,
    #[strum(serialize = "intern")]
    SysIntern,
    #[strum(serialize = "argv")]
    SysArgv,
    #[strum(serialize = "path")]
    SysPath,
    #[strum(serialize = "modules")]
    SysModules,
    #[strum(serialize = "implementation")]
    Implementation,
    #[strum(serialize = "executable")]
    SysExecutable,
    #[strum(serialize = "getdefaultencoding")]
    SysGetdefaultencoding,
    #[strum(serialize = "utf-8")]
    SysUtf8,
    #[strum(serialize = "hexversion")]
    SysHexversion,
    #[strum(serialize = "maxunicode")]
    SysMaxunicode,
    #[strum(serialize = "copyright")]
    SysCopyright,
    #[strum(serialize = "builtin_module_names")]
    SysBuiltinModuleNames,
    #[strum(serialize = "builtins")]
    BuiltinsMod,

    // ==========================
    // sys module - additional attributes
    #[strum(serialize = "cpython")]
    Cpython,
    #[strum(serialize = "darwin")]
    Darwin,
    #[strum(serialize = "linux")]
    Linux,
    #[strum(serialize = "win32")]
    Win32,
    #[strum(serialize = "stdlib_module_names")]
    SysStdlibModuleNames,
    #[strum(serialize = "meta_path")]
    SysMetaPath,
    #[strum(serialize = "path_hooks")]
    SysPathHooks,
    #[strum(serialize = "path_importer_cache")]
    SysPathImporterCache,

    // ==========================
    // weakref module strings
    #[strum(serialize = "weakref")]
    Weakref,
    #[strum(serialize = "atexit")]
    Atexit,
    #[strum(serialize = "gc")]
    Gc,
    #[strum(serialize = "ref")]
    Ref,
    #[strum(serialize = "proxy")]
    WrProxy,
    #[strum(serialize = "getweakrefcount")]
    WrGetweakrefcount,
    #[strum(serialize = "getweakrefs")]
    WrGetweakrefs,
    #[strum(serialize = "finalize")]
    WrFinalize,
    #[strum(serialize = "ProxyTypes")]
    WrProxyTypes,
    #[strum(serialize = "WeakSet")]
    WrWeakSet,
    #[strum(serialize = "WeakValueDictionary")]
    WrWeakValueDictionary,
    #[strum(serialize = "WeakKeyDictionary")]
    WrWeakKeyDictionary,
    #[strum(serialize = "WeakMethod")]
    WrWeakMethod,

    // ==========================
    // json module strings
    #[strum(serialize = "json")]
    Json,
    #[strum(serialize = "pickle")]
    Pickle,
    #[strum(serialize = "load")]
    Load,
    #[strum(serialize = "loads")]
    Loads,
    #[strum(serialize = "dump")]
    Dump,
    #[strum(serialize = "dumps")]
    Dumps,
    #[strum(serialize = "collect")]
    Collect,
    #[strum(serialize = "JSONEncoder")]
    JsonEncoder,
    #[strum(serialize = "JSONDecoder")]
    JsonDecoder,
    #[strum(serialize = "detect_encoding")]
    JsonDetectEncoding,
    #[strum(serialize = "JSONDecodeError")]
    JsonDecodeError,

    // ==========================
    // os.stat_result fields
    #[strum(serialize = "StatResult")]
    OsStatResult,
    #[strum(serialize = "st_mode")]
    StMode,
    #[strum(serialize = "st_ino")]
    StIno,
    #[strum(serialize = "st_dev")]
    StDev,
    #[strum(serialize = "st_nlink")]
    StNlink,
    #[strum(serialize = "st_uid")]
    StUid,
    #[strum(serialize = "st_gid")]
    StGid,
    #[strum(serialize = "st_size")]
    StSize,
    #[strum(serialize = "st_atime")]
    StAtime,
    #[strum(serialize = "st_mtime")]
    StMtime,
    #[strum(serialize = "st_ctime")]
    StCtime,

    // ==========================
    // typing module strings
    #[strum(serialize = "typing")]
    Typing,
    #[strum(serialize = "TYPE_CHECKING")]
    TypeChecking,
    #[strum(serialize = "Any")]
    Any,
    #[strum(serialize = "Optional")]
    Optional,
    #[strum(serialize = "Union")]
    UnionType,
    #[strum(serialize = "List")]
    ListType,
    #[strum(serialize = "Dict")]
    DictType,
    #[strum(serialize = "Tuple")]
    TupleType,
    #[strum(serialize = "Set")]
    SetType,
    #[strum(serialize = "FrozenSet")]
    FrozenSet,
    #[strum(serialize = "Callable")]
    Callable,
    #[strum(serialize = "Type")]
    Type,
    #[strum(serialize = "Sequence")]
    Sequence,
    #[strum(serialize = "Mapping")]
    Mapping,
    #[strum(serialize = "Iterable")]
    Iterable,
    #[strum(serialize = "Iterator")]
    IteratorType,
    #[strum(serialize = "Generator")]
    Generator,
    #[strum(serialize = "ClassVar")]
    ClassVar,
    #[strum(serialize = "Final")]
    FinalType,
    #[strum(serialize = "Literal")]
    Literal,
    #[strum(serialize = "TypeVar")]
    TypeVar,
    #[strum(serialize = "Generic")]
    Generic,
    #[strum(serialize = "Protocol")]
    Protocol,
    #[strum(serialize = "Annotated")]
    Annotated,
    #[strum(serialize = "Self")]
    SelfType,
    #[strum(serialize = "Never")]
    Never,
    #[strum(serialize = "NoReturn")]
    NoReturn,
    #[strum(serialize = "AnyStr")]
    AnyStr,
    #[strum(serialize = "Awaitable")]
    Awaitable,
    #[strum(serialize = "Coroutine")]
    Coroutine,
    #[strum(serialize = "AsyncIterator")]
    AsyncIterator,
    #[strum(serialize = "AsyncIterable")]
    AsyncIterable,
    #[strum(serialize = "AsyncGenerator")]
    AsyncGenerator,
    #[strum(serialize = "MutableMapping")]
    MutableMapping,
    #[strum(serialize = "MutableSequence")]
    MutableSequence,
    #[strum(serialize = "MutableSet")]
    MutableSet,
    #[strum(serialize = "DefaultDict")]
    TypingDefaultDict,
    #[strum(serialize = "Deque")]
    TypingDeque,
    #[strum(serialize = "Pattern")]
    TypingPattern,
    #[strum(serialize = "Match")]
    TypingMatch,
    #[strum(serialize = "IO")]
    TypingIO,
    #[strum(serialize = "TextIO")]
    TypingTextIO,
    #[strum(serialize = "BinaryIO")]
    TypingBinaryIO,
    #[strum(serialize = "TypeGuard")]
    TypeGuard,
    #[strum(serialize = "TypeIs")]
    TypeIs,
    #[strum(serialize = "Unpack")]
    Unpack,
    #[strum(serialize = "ParamSpec")]
    ParamSpec,
    #[strum(serialize = "ParamSpecArgs")]
    ParamSpecArgs,
    #[strum(serialize = "ParamSpecKwargs")]
    ParamSpecKwargs,
    #[strum(serialize = "Concatenate")]
    Concatenate,
    #[strum(serialize = "TypeVarTuple")]
    TypeVarTuple,
    #[strum(serialize = "TypeAlias")]
    TypeAlias,
    #[strum(serialize = "Required")]
    Required,
    #[strum(serialize = "NotRequired")]
    NotRequired,
    #[strum(serialize = "SupportsInt")]
    SupportsInt,
    #[strum(serialize = "SupportsFloat")]
    SupportsFloat,
    #[strum(serialize = "SupportsComplex")]
    SupportsComplex,
    #[strum(serialize = "SupportsBytes")]
    SupportsBytes,
    #[strum(serialize = "SupportsAbs")]
    SupportsAbs,
    #[strum(serialize = "SupportsRound")]
    SupportsRound,
    #[strum(serialize = "NewType")]
    TypingNewType,
    #[strum(serialize = "cast")]
    TypingCast,
    #[strum(serialize = "reveal_type")]
    TypingRevealType,
    #[strum(serialize = "assert_type")]
    TypingAssertType,
    #[strum(serialize = "overload")]
    TypingOverload,
    #[strum(serialize = "runtime_checkable")]
    TypingRuntimeCheckable,
    #[strum(serialize = "dataclass_transform")]
    TypingDataclassTransform,
    #[strum(serialize = "override")]
    TypingOverride,
    #[strum(serialize = "deprecated")]
    TypingDeprecated,
    #[strum(serialize = "get_type_hints")]
    TypingGetTypeHints,
    #[strum(serialize = "get_origin")]
    TypingGetOrigin,
    #[strum(serialize = "get_args")]
    TypingGetArgs,
    #[strum(serialize = "NamedTuple")]
    TypingNamedTuple,
    #[strum(serialize = "TypedDict")]
    TypingTypedDict,

    // ==========================
    // asyncio module strings
    #[strum(serialize = "asyncio")]
    Asyncio,
    #[strum(serialize = "gather")]
    Gather,
    #[strum(serialize = "sleep")]
    AioSleep,
    #[strum(serialize = "create_task")]
    AioCreateTask,
    #[strum(serialize = "wait_for")]
    AioWaitFor,
    #[strum(serialize = "shield")]
    AioShield,
    #[strum(serialize = "current_task")]
    AioCurrentTask,
    #[strum(serialize = "all_tasks")]
    AioAllTasks,
    #[strum(serialize = "run")]
    AioRun,
    #[strum(serialize = "iscoroutine")]
    AioIscoroutine,
    #[strum(serialize = "iscoroutinefunction")]
    AioIscoroutinefunction,
    #[strum(serialize = "CancelledError")]
    AioCancelledError,
    #[strum(serialize = "TimeoutError")]
    AioTimeoutError,
    #[strum(serialize = "InvalidStateError")]
    AioInvalidStateError,
    #[strum(serialize = "Future")]
    AioFuture,
    #[strum(serialize = "Task")]
    AioTask,
    #[strum(serialize = "Queue")]
    AioQueue,
    #[strum(serialize = "Event")]
    AioEvent,
    #[strum(serialize = "Lock")]
    AioLock,
    #[strum(serialize = "Semaphore")]
    AioSemaphore,
    #[strum(serialize = "ALL_COMPLETED")]
    AioAllCompleted,
    #[strum(serialize = "FIRST_COMPLETED")]
    AioFirstCompleted,
    #[strum(serialize = "FIRST_EXCEPTION")]
    AioFirstException,
    #[strum(serialize = "__main__")]
    MainModule,

    // ==========================
    // os module strings
    #[strum(serialize = "os")]
    Os,
    #[strum(serialize = "getenv")]
    Getenv,
    #[strum(serialize = "environ")]
    Environ,
    #[strum(serialize = "default")]
    Default,
    #[strum(serialize = "getcwd")]
    OsGetcwd,
    #[strum(serialize = "fspath")]
    OsFspath,
    #[strum(serialize = "cpu_count")]
    OsCpuCount,
    #[strum(serialize = "urandom")]
    OsUrandom,
    #[strum(serialize = "strerror")]
    OsStrerror,
    #[strum(serialize = "sep")]
    OsSep,
    #[strum(serialize = "pathsep")]
    OsPathsep,
    #[strum(serialize = "linesep")]
    OsLinesep,
    #[strum(serialize = "altsep")]
    OsAltsep,
    #[strum(serialize = "curdir")]
    OsCurdir,
    #[strum(serialize = "pardir")]
    OsPardir,
    #[strum(serialize = "extsep")]
    OsExtsep,
    #[strum(serialize = "devnull")]
    OsDevnull,
    #[strum(serialize = "os.path")]
    OsPathMod,

    // os.path module functions (pure string manipulation)
    // NOTE: os.path.join reuses `Join`, os.path.split reuses `Split`,
    // os.path.exists reuses `Exists` from pathlib. Only functions with
    // names unique to os.path need new variants here.
    #[strum(serialize = "dirname")]
    OsPathDirname,
    #[strum(serialize = "basename")]
    OsPathBasename,
    #[strum(serialize = "splitext")]
    OsPathSplitext,
    #[strum(serialize = "splitdrive")]
    OsPathSplitdrive,
    #[strum(serialize = "splitroot")]
    OsPathSplitroot,
    #[strum(serialize = "isabs")]
    OsPathIsabs,
    #[strum(serialize = "abspath")]
    OsPathAbspath,
    #[strum(serialize = "normpath")]
    OsPathNormpath,
    #[strum(serialize = "commonpath")]
    OsPathCommonpath,
    #[strum(serialize = "commonprefix")]
    OsPathCommonprefix,
    #[strum(serialize = "relpath")]
    OsPathRelpath,

    // ==========================
    // math module strings
    #[strum(serialize = "math")]
    Math,
    #[strum(serialize = "ceil")]
    Ceil,
    #[strum(serialize = "floor")]
    Floor,
    #[strum(serialize = "sqrt")]
    Sqrt,
    #[strum(serialize = "cbrt")]
    Cbrt,
    #[strum(serialize = "log")]
    Log,
    #[strum(serialize = "log1p")]
    Log1p,
    #[strum(serialize = "log2")]
    Log2,
    #[strum(serialize = "log10")]
    Log10,
    #[strum(serialize = "pow")]
    MathPow,
    #[strum(serialize = "fabs")]
    Fabs,
    #[strum(serialize = "isnan")]
    Isnan,
    #[strum(serialize = "isinf")]
    Isinf,
    #[strum(serialize = "isfinite")]
    Isfinite,
    #[strum(serialize = "pi")]
    Pi,
    #[strum(serialize = "e")]
    MathE,
    #[strum(serialize = "inf")]
    MathInf,
    #[strum(serialize = "nan")]
    MathNan,
    #[strum(serialize = "exp")]
    Exp,
    #[strum(serialize = "exp2")]
    Exp2,
    #[strum(serialize = "expm1")]
    Expm1,
    #[strum(serialize = "sin")]
    Sin,
    #[strum(serialize = "cos")]
    Cos,
    #[strum(serialize = "tan")]
    Tan,
    #[strum(serialize = "asin")]
    Asin,
    #[strum(serialize = "acos")]
    Acos,
    #[strum(serialize = "atan")]
    Atan,
    #[strum(serialize = "atan2")]
    Atan2,
    #[strum(serialize = "degrees")]
    MathDegrees,
    #[strum(serialize = "radians")]
    MathRadians,
    #[strum(serialize = "factorial")]
    Factorial,
    #[strum(serialize = "isqrt")]
    Isqrt,
    #[strum(serialize = "gcd")]
    Gcd,
    #[strum(serialize = "trunc")]
    Trunc,
    #[strum(serialize = "copysign")]
    Copysign,
    #[strum(serialize = "isclose")]
    Isclose,
    #[strum(serialize = "tau")]
    Tau,
    #[strum(serialize = "sinh")]
    Sinh,
    #[strum(serialize = "cosh")]
    Cosh,
    #[strum(serialize = "tanh")]
    Tanh,
    #[strum(serialize = "asinh")]
    Asinh,
    #[strum(serialize = "acosh")]
    Acosh,
    #[strum(serialize = "atanh")]
    Atanh,
    #[strum(serialize = "erf")]
    Erf,
    #[strum(serialize = "erfc")]
    Erfc,
    #[strum(serialize = "gamma")]
    MathGamma,
    #[strum(serialize = "lgamma")]
    Lgamma,
    #[strum(serialize = "hypot")]
    Hypot,
    #[strum(serialize = "fmod")]
    Fmod,
    #[strum(serialize = "remainder")]
    MathRemainder,
    #[strum(serialize = "fsum")]
    Fsum,
    #[strum(serialize = "fma")]
    Fma,
    #[strum(serialize = "nextafter")]
    Nextafter,
    #[strum(serialize = "ulp")]
    Ulp,
    #[strum(serialize = "comb")]
    Comb,
    #[strum(serialize = "perm")]
    MathPerm,
    #[strum(serialize = "lcm")]
    Lcm,
    #[strum(serialize = "prod")]
    MathProd,
    #[strum(serialize = "sumprod")]
    Sumprod,
    #[strum(serialize = "dist")]
    MathDist,
    #[strum(serialize = "modf")]
    Modf,
    #[strum(serialize = "frexp")]
    Frexp,
    #[strum(serialize = "ldexp")]
    Ldexp,
    #[strum(serialize = "ceil_div")]
    MathCeilDiv,
    #[strum(serialize = "floor_div")]
    MathFloorDiv,
    #[strum(serialize = "sum_of_squares")]
    MathSumOfSquares,
    #[strum(serialize = "dot")]
    MathDot,
    #[strum(serialize = "cross")]
    MathCross,

    // ==========================
    // Exception attributes
    Args,
    Func,
    Keywords,
    Alive,
    Detach,
    Peek,
    #[strum(serialize = "GeneratorExit")]
    GeneratorExit,

    // ==========================
    // Type / class dunder attributes
    #[strum(serialize = "__name__")]
    DunderName,
    #[strum(serialize = "__module__")]
    DunderModule,
    #[strum(serialize = "__qualname__")]
    DunderQualname,
    #[strum(serialize = "__doc__")]
    DunderDoc,
    #[strum(serialize = "__annotations__")]
    DunderAnnotations,
    #[strum(serialize = "__annotate__")]
    DunderAnnotate,
    #[strum(serialize = "__defaults__")]
    DunderDefaults,
    #[strum(serialize = "__kwdefaults__")]
    DunderKwdefaults,
    #[strum(serialize = "__isabstractmethod__")]
    DunderIsabstractmethod,
    #[strum(serialize = "__init__")]
    DunderInit,
    #[strum(serialize = "__class__")]
    DunderClass,
    #[strum(serialize = "__bases__")]
    DunderBases,
    #[strum(serialize = "__mro__")]
    DunderMro,
    #[strum(serialize = "__subclasses__")]
    DunderSubclasses,
    #[strum(serialize = "__self__")]
    DunderSelf,
    #[strum(serialize = "__func__")]
    DunderFunc,
    #[strum(serialize = "__wrapped__")]
    DunderWrapped,

    // ==========================
    // Core protocol dunders
    #[strum(serialize = "__str__")]
    DunderStr,
    #[strum(serialize = "__repr__")]
    DunderRepr,
    #[strum(serialize = "__eq__")]
    DunderEq,
    #[strum(serialize = "__ne__")]
    DunderNe,
    #[strum(serialize = "__lt__")]
    DunderLt,
    #[strum(serialize = "__le__")]
    DunderLe,
    #[strum(serialize = "__gt__")]
    DunderGt,
    #[strum(serialize = "__ge__")]
    DunderGe,
    #[strum(serialize = "__hash__")]
    DunderHash,
    #[strum(serialize = "__bool__")]
    DunderBool,
    #[strum(serialize = "__len__")]
    DunderLen,
    #[strum(serialize = "__contains__")]
    DunderContains,

    // ==========================
    // Arithmetic dunders
    #[strum(serialize = "__add__")]
    DunderAdd,
    #[strum(serialize = "__radd__")]
    DunderRadd,
    #[strum(serialize = "__iadd__")]
    DunderIadd,
    #[strum(serialize = "__sub__")]
    DunderSub,
    #[strum(serialize = "__rsub__")]
    DunderRsub,
    #[strum(serialize = "__isub__")]
    DunderIsub,
    #[strum(serialize = "__mul__")]
    DunderMul,
    #[strum(serialize = "__rmul__")]
    DunderRmul,
    #[strum(serialize = "__imul__")]
    DunderImul,
    #[strum(serialize = "__truediv__")]
    DunderTruediv,
    #[strum(serialize = "__rtruediv__")]
    DunderRtruediv,
    #[strum(serialize = "__itruediv__")]
    DunderItruediv,
    #[strum(serialize = "__floordiv__")]
    DunderFloordiv,
    #[strum(serialize = "__rfloordiv__")]
    DunderRfloordiv,
    #[strum(serialize = "__ifloordiv__")]
    DunderIfloordiv,
    #[strum(serialize = "__mod__")]
    DunderMod,
    #[strum(serialize = "__rmod__")]
    DunderRmod,
    #[strum(serialize = "__imod__")]
    DunderImod,
    #[strum(serialize = "__pow__")]
    DunderPow,
    #[strum(serialize = "__rpow__")]
    DunderRpow,
    #[strum(serialize = "__ipow__")]
    DunderIpow,
    #[strum(serialize = "__neg__")]
    DunderNeg,
    #[strum(serialize = "__pos__")]
    DunderPos,
    #[strum(serialize = "__abs__")]
    DunderAbs,
    #[strum(serialize = "__invert__")]
    DunderInvert,

    // ==========================
    // Bitwise dunders
    #[strum(serialize = "__and__")]
    DunderAnd,
    #[strum(serialize = "__rand__")]
    DunderRand,
    #[strum(serialize = "__iand__")]
    DunderIand,
    #[strum(serialize = "__or__")]
    DunderOr,
    #[strum(serialize = "__ror__")]
    DunderRor,
    #[strum(serialize = "__ior__")]
    DunderIor,
    #[strum(serialize = "__xor__")]
    DunderXor,
    #[strum(serialize = "__rxor__")]
    DunderRxor,
    #[strum(serialize = "__ixor__")]
    DunderIxor,
    #[strum(serialize = "__lshift__")]
    DunderLshift,
    #[strum(serialize = "__rlshift__")]
    DunderRlshift,
    #[strum(serialize = "__ilshift__")]
    DunderIlshift,
    #[strum(serialize = "__rshift__")]
    DunderRshift,
    #[strum(serialize = "__rrshift__")]
    DunderRrshift,
    #[strum(serialize = "__irshift__")]
    DunderIrshift,
    #[strum(serialize = "__matmul__")]
    DunderMatmul,
    #[strum(serialize = "__rmatmul__")]
    DunderRmatmul,
    #[strum(serialize = "__imatmul__")]
    DunderImatmul,

    // ==========================
    // Copy dunders
    #[strum(serialize = "__copy__")]
    DunderCopy,
    #[strum(serialize = "__deepcopy__")]
    DunderDeepcopy,

    // ==========================
    // Conversion dunders
    #[strum(serialize = "__int__")]
    DunderInt,
    #[strum(serialize = "__float__")]
    DunderFloat,
    #[strum(serialize = "__complex__")]
    DunderComplex,
    #[strum(serialize = "__index__")]
    DunderIndex,
    #[strum(serialize = "__length_hint__")]
    DunderLengthHint,

    // ==========================
    // Container dunders
    #[strum(serialize = "__getitem__")]
    DunderGetitem,
    #[strum(serialize = "__setitem__")]
    DunderSetitem,
    #[strum(serialize = "__delitem__")]
    DunderDelitem,

    // ==========================
    // Iterator dunders
    #[strum(serialize = "__iter__")]
    DunderIter,
    #[strum(serialize = "__next__")]
    DunderNext,

    // ==========================
    // Callable dunder
    #[strum(serialize = "__call__")]
    DunderCall,

    // ==========================
    // Context manager dunders
    #[strum(serialize = "__enter__")]
    DunderEnter,
    #[strum(serialize = "__exit__")]
    DunderExit,
    #[strum(serialize = "__aenter__")]
    DunderAenter,
    #[strum(serialize = "__aexit__")]
    DunderAexit,

    // ==========================
    // Format dunder
    #[strum(serialize = "__format__")]
    DunderFormat,

    // ==========================
    // Attribute access dunders
    #[strum(serialize = "__getattr__")]
    DunderGetattr,
    #[strum(serialize = "__getattribute__")]
    DunderGetattribute,
    #[strum(serialize = "__setattr__")]
    DunderSetattr,
    #[strum(serialize = "__delattr__")]
    DunderDelattr,

    // ==========================
    // Descriptor protocol dunders
    #[strum(serialize = "__get__")]
    DunderDescGet,
    #[strum(serialize = "__set__")]
    DunderDescSet,
    #[strum(serialize = "__delete__")]
    DunderDescDelete,
    #[strum(serialize = "__set_name__")]
    DunderSetName,

    // ==========================
    // Object creation / class dunders
    #[strum(serialize = "__new__")]
    DunderNew,
    #[strum(serialize = "__prepare__")]
    DunderPrepare,
    #[strum(serialize = "__mro_entries__")]
    DunderMroEntries,
    #[strum(serialize = "__orig_bases__")]
    DunderOrigBases,
    #[strum(serialize = "__type_params__")]
    DunderTypeParams,
    #[strum(serialize = "__origin__")]
    DunderOrigin,
    #[strum(serialize = "__args__")]
    DunderArgs,
    #[strum(serialize = "__parameters__")]
    DunderParameters,
    #[strum(serialize = "__match_args__")]
    DunderMatchArgs,
    #[strum(serialize = "__instancecheck__")]
    DunderInstancecheck,
    #[strum(serialize = "__subclasscheck__")]
    DunderSubclasscheck,
    #[strum(serialize = "__dict__")]
    DunderDictAttr,
    #[strum(serialize = "__slots__")]
    DunderSlots,
    #[strum(serialize = "__init_subclass__")]
    DunderInitSubclass,
    #[strum(serialize = "__class_getitem__")]
    DunderClassGetitem,

    // ==========================
    // pathlib module strings
    #[strum(serialize = "pathlib")]
    Pathlib,
    #[strum(serialize = "Path")]
    PathClass,
    #[strum(serialize = "PurePath")]
    PurePathClass,
    #[strum(serialize = "PurePosixPath")]
    PurePosixPathClass,
    #[strum(serialize = "PureWindowsPath")]
    PureWindowsPathClass,

    // Path properties (pure - no I/O)
    #[strum(serialize = "name")]
    Name,
    #[strum(serialize = "parent")]
    Parent,
    #[strum(serialize = "parents")]
    Parents,
    #[strum(serialize = "stem")]
    Stem,
    #[strum(serialize = "suffix")]
    Suffix,
    #[strum(serialize = "suffixes")]
    Suffixes,
    #[strum(serialize = "parts")]
    Parts,
    #[strum(serialize = "root")]
    Root,
    #[strum(serialize = "anchor")]
    Anchor,
    #[strum(serialize = "drive")]
    Drive,

    // Path pure methods (no I/O)
    #[strum(serialize = "is_absolute")]
    IsAbsolute,
    #[strum(serialize = "is_relative_to")]
    IsRelativeTo,
    #[strum(serialize = "joinpath")]
    Joinpath,
    #[strum(serialize = "with_name")]
    WithName,
    #[strum(serialize = "with_stem")]
    WithStem,
    #[strum(serialize = "with_suffix")]
    WithSuffix,
    #[strum(serialize = "as_posix")]
    AsPosix,
    #[strum(serialize = "as_uri")]
    AsUri,
    #[strum(serialize = "is_reserved")]
    IsReserved,
    #[strum(serialize = "full_match")]
    FullMatch,
    #[strum(serialize = "with_segments")]
    WithSegments,
    #[strum(serialize = "parser")]
    Parser,
    #[strum(serialize = "__fspath__")]
    Fspath,
    #[strum(serialize = "relative_to")]
    RelativeTo,

    // Path filesystem methods (require OsAccess - yield external calls)
    #[strum(serialize = "exists")]
    Exists,
    #[strum(serialize = "is_file")]
    IsFile,
    #[strum(serialize = "is_dir")]
    IsDir,
    #[strum(serialize = "is_symlink")]
    IsSymlink,
    #[strum(serialize = "stat")]
    StatMethod,
    #[strum(serialize = "read_bytes")]
    ReadBytes,
    #[strum(serialize = "read_text")]
    ReadText,
    #[strum(serialize = "iterdir")]
    Iterdir,
    #[strum(serialize = "resolve")]
    Resolve,
    #[strum(serialize = "absolute")]
    Absolute,

    // Path write methods (require OsAccess - yield external calls)
    #[strum(serialize = "write_text")]
    WriteText,
    #[strum(serialize = "write_bytes")]
    WriteBytes,
    #[strum(serialize = "mkdir")]
    Mkdir,
    #[strum(serialize = "unlink")]
    Unlink,
    #[strum(serialize = "rmdir")]
    Rmdir,
    #[strum(serialize = "rename")]
    Rename,

    // Slice attributes
    Start,
    Stop,
    Step,

    // ==========================
    // re module strings
    #[strum(serialize = "re")]
    Re,
    #[strum(serialize = "search")]
    ReSearch,
    #[strum(serialize = "match")]
    ReMatch,
    #[strum(serialize = "fullmatch")]
    ReFullmatch,
    #[strum(serialize = "findall")]
    ReFindall,
    #[strum(serialize = "sub")]
    ReSub,

    #[strum(serialize = "compile")]
    ReCompile,
    #[strum(serialize = "finditer")]
    ReFinditer,
    #[strum(serialize = "subn")]
    ReSubn,
    #[strum(serialize = "scanner")]
    ReScanner,
    #[strum(serialize = "escape")]
    ReEscape,
    #[strum(serialize = "purge")]
    RePurge,
    #[strum(serialize = "IGNORECASE")]
    ReIgnorecase,
    #[strum(serialize = "MULTILINE")]
    ReMultiline,
    #[strum(serialize = "DOTALL")]
    ReDotall,
    #[strum(serialize = "VERBOSE")]
    ReVerbose,
    #[strum(serialize = "ASCII")]
    ReAscii,
    #[strum(serialize = "NOFLAG")]
    ReNoflag,
    #[strum(serialize = "UNICODE")]
    ReUnicode,
    #[strum(serialize = "U")]
    ReUnicodeShort,
    #[strum(serialize = "error")]
    ReError,

    // re.Match / re.Pattern attribute and method names
    #[strum(serialize = "group")]
    Group,
    #[strum(serialize = "groups")]
    Groups,
    #[strum(serialize = "span")]
    Span,
    #[strum(serialize = "end")]
    ReEnd,
    #[strum(serialize = "endpos")]
    ReEndpos,
    #[strum(serialize = "lineno")]
    ReLineno,
    #[strum(serialize = "colno")]
    ReColno,
    #[strum(serialize = "pattern")]
    Pattern,
    #[strum(serialize = "flags")]
    Flags,

    // ==========================
    // collections module strings
    #[strum(serialize = "collections")]
    Collections,
    #[strum(serialize = "Counter")]
    Counter,
    #[strum(serialize = "namedtuple")]
    CollNamedtuple,
    #[strum(serialize = "defaultdict")]
    DefaultDict,
    #[strum(serialize = "OrderedDict")]
    CollOrderedDict,
    #[strum(serialize = "deque")]
    Deque,
    #[strum(serialize = "ChainMap")]
    ChainMap,
    #[strum(serialize = "UserDict")]
    CollUserDict,
    #[strum(serialize = "UserList")]
    CollUserList,
    #[strum(serialize = "UserString")]
    CollUserString,
    #[strum(serialize = "counter_most_common")]
    CollCounterMostCommon,
    #[strum(serialize = "counter_elements")]
    CollCounterElements,
    #[strum(serialize = "counter_subtract")]
    CollCounterSubtract,
    #[strum(serialize = "counter_update")]
    CollCounterUpdate,
    #[strum(serialize = "counter_total")]
    CollCounterTotal,
    #[strum(serialize = "deque_appendleft")]
    CollDequeAppendleft,
    #[strum(serialize = "deque_popleft")]
    CollDequePopleft,
    #[strum(serialize = "deque_extendleft")]
    CollDequeExtendleft,
    #[strum(serialize = "deque_rotate")]
    CollDequeRotate,
    #[strum(serialize = "ordereddict_move_to_end")]
    CollOdMoveToEnd,
    #[strum(serialize = "ordereddict_popitem")]
    CollOdPopitem,

    // ==========================
    // itertools module strings
    #[strum(serialize = "itertools")]
    Itertools,
    #[strum(serialize = "chain")]
    ItChain,
    #[strum(serialize = "tee")]
    ItTee,
    #[strum(serialize = "islice")]
    Islice,
    #[strum(serialize = "zip_longest")]
    ZipLongest,
    #[strum(serialize = "fillvalue")]
    Fillvalue,
    #[strum(serialize = "product")]
    ItProduct,
    #[strum(serialize = "permutations")]
    Permutations,
    #[strum(serialize = "combinations")]
    Combinations,
    #[strum(serialize = "repeat")]
    ItRepeat,
    #[strum(serialize = "cycle")]
    ItCycle,
    #[strum(serialize = "accumulate")]
    Accumulate,
    #[strum(serialize = "starmap")]
    Starmap,
    #[strum(serialize = "filterfalse")]
    Filterfalse,
    #[strum(serialize = "takewhile")]
    Takewhile,
    #[strum(serialize = "dropwhile")]
    Dropwhile,
    #[strum(serialize = "compress")]
    Compress,
    #[strum(serialize = "pairwise")]
    Pairwise,
    #[strum(serialize = "batched")]
    Batched,
    #[strum(serialize = "groupby")]
    Groupby,
    #[strum(serialize = "combinations_with_replacement")]
    CombinationsWithReplacement,

    // ==========================
    // functools module strings
    #[strum(serialize = "functools")]
    Functools,
    #[strum(serialize = "reduce")]
    FtReduce,
    #[strum(serialize = "partial")]
    FtPartial,
    #[strum(serialize = "cmp_to_key")]
    FtCmpToKey,
    #[strum(serialize = "lru_cache")]
    FtLruCache,
    #[strum(serialize = "cache")]
    FtCache,
    #[strum(serialize = "cached_property")]
    FtCachedProperty,
    #[strum(serialize = "singledispatch")]
    FtSingledispatch,
    #[strum(serialize = "singledispatchmethod")]
    FtSingledispatchmethod,
    #[strum(serialize = "partialmethod")]
    FtPartialmethod,
    #[strum(serialize = "Placeholder")]
    FtPlaceholder,
    #[strum(serialize = "wraps")]
    FtWraps,
    #[strum(serialize = "update_wrapper")]
    FtUpdateWrapper,
    #[strum(serialize = "get_cache_token")]
    FtGetCacheToken,
    #[strum(serialize = "recursive_repr")]
    FtRecursiveRepr,
    #[strum(serialize = "total_ordering")]
    FtTotalOrdering,
    #[strum(serialize = "WRAPPER_ASSIGNMENTS")]
    FtWrapperAssignments,
    #[strum(serialize = "WRAPPER_UPDATES")]
    FtWrapperUpdates,

    // ==========================
    // inspect module strings
    #[strum(serialize = "inspect")]
    Inspect,
    #[strum(serialize = "signature")]
    InspectSignature,
    #[strum(serialize = "parameters")]
    Parameters,
    #[strum(serialize = "return")]
    ReturnWord,

    // ==========================
    // dataclasses module strings
    #[strum(serialize = "dataclasses")]
    Dataclasses,
    #[strum(serialize = "dataclass")]
    Dataclass,
    #[strum(serialize = "field")]
    DcField,
    #[strum(serialize = "fields")]
    DcFields,
    #[strum(serialize = "asdict")]
    DcAsdict,
    #[strum(serialize = "astuple")]
    DcAstuple,
    #[strum(serialize = "is_dataclass")]
    DcIsDataclass,
    #[strum(serialize = "KW_ONLY")]
    DcKwOnly,
    #[strum(serialize = "MISSING")]
    DcMissing,
    #[strum(serialize = "InitVar")]
    DcInitVar,
    #[strum(serialize = "FrozenInstanceError")]
    DcFrozenInstanceError,
    #[strum(serialize = "make_dataclass")]
    DcMakeDataclass,
    // abc module strings
    #[strum(serialize = "abc")]
    Abc,
    #[strum(serialize = "ABC")]
    AbcABC,
    #[strum(serialize = "abstractmethod")]
    AbcAbstractmethod,
    #[strum(serialize = "abstractclassmethod")]
    AbcAbstractclassmethod,
    #[strum(serialize = "abstractstaticmethod")]
    AbcAbstractstaticmethod,
    #[strum(serialize = "abstractproperty")]
    AbcAbstractproperty,
    #[strum(serialize = "abc_get_cache_token")]
    AbcGetCacheToken,
    #[strum(serialize = "update_abstractmethods")]
    AbcUpdateAbstractmethods,
    #[strum(serialize = "ABCMeta")]
    AbcABCMeta,

    // ==========================
    // hashlib module strings
    #[strum(serialize = "hashlib")]
    Hashlib,
    #[strum(serialize = "md5")]
    HlMd5,
    #[strum(serialize = "new")]
    HlNew,
    #[strum(serialize = "sha1")]
    HlSha1,
    #[strum(serialize = "sha224")]
    HlSha224,
    #[strum(serialize = "sha256")]
    HlSha256,
    #[strum(serialize = "hexdigest")]
    HlHexdigest,
    #[strum(serialize = "digest")]
    HlDigest,
    #[strum(serialize = "sha384")]
    HlSha384,
    #[strum(serialize = "sha512")]
    HlSha512,
    #[strum(serialize = "sha512_224")]
    HlSha512_224,
    #[strum(serialize = "sha512_256")]
    HlSha512_256,
    #[strum(serialize = "sha3_224")]
    HlSha3_224,
    #[strum(serialize = "sha3_256")]
    HlSha3_256,
    #[strum(serialize = "sha3_384")]
    HlSha3_384,
    #[strum(serialize = "sha3_512")]
    HlSha3_512,
    #[strum(serialize = "blake2b")]
    HlBlake2b,
    #[strum(serialize = "blake2s")]
    HlBlake2s,
    #[strum(serialize = "shake_128")]
    HlShake128,
    #[strum(serialize = "shake_256")]
    HlShake256,
    #[strum(serialize = "scrypt")]
    HlScrypt,
    #[strum(serialize = "pbkdf2_hmac")]
    HlPbkdf2Hmac,
    #[strum(serialize = "algorithms_available")]
    HlAlgorithmsAvailable,
    #[strum(serialize = "algorithms_guaranteed")]
    HlAlgorithmsGuaranteed,
    #[strum(serialize = "file_digest")]
    HlFileDigest,

    // ==========================
    // contextlib module strings
    #[strum(serialize = "contextlib")]
    Contextlib,
    #[strum(serialize = "suppress")]
    ClSuppress,
    #[strum(serialize = "contextmanager")]
    ClContextmanager,
    #[strum(serialize = "nullcontext")]
    ClNullcontext,
    #[strum(serialize = "closing")]
    ClClosing,
    #[strum(serialize = "aclosing")]
    ClAclosing,
    #[strum(serialize = "redirect_stdout")]
    ClRedirectStdout,
    #[strum(serialize = "redirect_stderr")]
    ClRedirectStderr,
    #[strum(serialize = "ExitStack")]
    ClExitStack,
    #[strum(serialize = "AsyncExitStack")]
    ClAsyncExitStack,

    // ==========================
    // statistics module strings
    #[strum(serialize = "statistics")]
    Statistics,
    #[strum(serialize = "mean")]
    StatMean,
    #[strum(serialize = "median")]
    StatMedian,
    #[strum(serialize = "mode")]
    StatMode,
    #[strum(serialize = "stdev")]
    StatStdev,
    #[strum(serialize = "variance")]
    StatVariance,
    #[strum(serialize = "harmonic_mean")]
    StatHarmonicMean,
    #[strum(serialize = "geometric_mean")]
    StatGeometricMean,
    #[strum(serialize = "median_low")]
    StatMedianLow,
    #[strum(serialize = "median_high")]
    StatMedianHigh,
    #[strum(serialize = "multimode")]
    StatMultimode,
    #[strum(serialize = "pstdev")]
    StatPstdev,
    #[strum(serialize = "pvariance")]
    StatPvariance,
    #[strum(serialize = "fmean")]
    StatFmean,
    #[strum(serialize = "median_grouped")]
    StatMedianGrouped,
    #[strum(serialize = "kde")]
    StatKde,
    #[strum(serialize = "kde_random")]
    StatKdeRandom,
    #[strum(serialize = "quantiles")]
    StatQuantiles,
    #[strum(serialize = "correlation")]
    StatCorrelation,
    #[strum(serialize = "covariance")]
    StatCovariance,
    #[strum(serialize = "linear_regression")]
    StatLinearRegression,
    #[strum(serialize = "StatisticsError")]
    StatStatisticsError,
    #[strum(serialize = "NormalDist")]
    StatNormalDist,

    // ==========================
    // string module strings
    #[strum(serialize = "string")]
    StringMod,
    #[strum(serialize = "ascii_lowercase")]
    StrAsciiLowercase,
    #[strum(serialize = "ascii_uppercase")]
    StrAsciiUppercase,
    #[strum(serialize = "ascii_letters")]
    StrAsciiLetters,
    #[strum(serialize = "digits")]
    StrDigits,
    #[strum(serialize = "hexdigits")]
    StrHexdigits,
    #[strum(serialize = "octdigits")]
    StrOctdigits,
    #[strum(serialize = "punctuation")]
    StrPunctuation,
    #[strum(serialize = "whitespace")]
    StrWhitespace,
    #[strum(serialize = "printable")]
    StrPrintable,
    #[strum(serialize = "capwords")]
    StrCapwords,
    #[strum(serialize = "Formatter")]
    StrFormatter,
    #[strum(serialize = "Template")]
    StrTemplate,

    // ==========================
    // textwrap module strings
    #[strum(serialize = "textwrap")]
    Textwrap,
    #[strum(serialize = "dedent")]
    TwDedent,
    #[strum(serialize = "indent")]
    TwIndent,
    #[strum(serialize = "fill")]
    TwFill,
    #[strum(serialize = "wrap")]
    TwWrap,
    #[strum(serialize = "shorten")]
    TwShorten,
    #[strum(serialize = "TextWrapper")]
    TwTextWrapper,
    #[strum(serialize = "width")]
    TwWidth,
    #[strum(serialize = "initial_indent")]
    TwInitialIndent,
    #[strum(serialize = "subsequent_indent")]
    TwSubsequentIndent,
    #[strum(serialize = "placeholder")]
    TwPlaceholder,
    #[strum(serialize = "max_lines")]
    TwMaxLines,
    #[strum(serialize = "break_long_words")]
    TwBreakLongWords,
    #[strum(serialize = "expand_tabs")]
    TwExpandTabs,
    #[strum(serialize = "replace_whitespace")]
    TwReplaceWhitespace,
    #[strum(serialize = "fix_sentence_endings")]
    TwFixSentenceEndings,
    #[strum(serialize = "drop_whitespace")]
    TwDropWhitespace,
    #[strum(serialize = "break_on_hyphens")]
    TwBreakOnHyphens,
    #[strum(serialize = "tabsize")]
    TwTabsize,

    // ==========================
    // Additional stdlib module names
    #[strum(serialize = "html")]
    Html,
    #[strum(serialize = "argparse")]
    Argparse,
    #[strum(serialize = "shlex")]
    Shlex,
    #[strum(serialize = "fnmatch")]
    Fnmatch,
    #[strum(serialize = "tomllib")]
    Tomllib,
    #[strum(serialize = "typing_extensions")]
    TypingExtensions,
    #[strum(serialize = "collections.abc")]
    CollectionsAbc,
    #[strum(serialize = "numbers")]
    Numbers,
    #[strum(serialize = "types")]
    TypesMod,
    #[strum(serialize = "binascii")]
    Binascii,
    #[strum(serialize = "codecs")]
    Codecs,
    #[strum(serialize = "cmath")]
    Cmath,
    #[strum(serialize = "zlib")]
    Zlib,
    #[strum(serialize = "warnings")]
    Warnings,
    #[strum(serialize = "logging")]
    Logging,
    #[strum(serialize = "urllib")]
    Urllib,
    #[strum(serialize = "urllib.parse")]
    UrllibParse,
    #[strum(serialize = "ast")]
    AstMod,
    #[strum(serialize = "tokenize")]
    TokenizeMod,
    #[strum(serialize = "difflib")]
    Difflib,
    #[strum(serialize = "ipaddress")]
    Ipaddress,
    #[strum(serialize = "keyword")]
    KeywordMod,

    // ==========================
    // uuid module strings
    #[strum(serialize = "uuid")]
    Uuid,
    #[strum(serialize = "uuid1")]
    UuidUuid1,
    #[strum(serialize = "uuid4")]
    UuidUuid4,
    #[strum(serialize = "uuid3")]
    UuidUuid3,
    #[strum(serialize = "uuid5")]
    UuidUuid5,
    #[strum(serialize = "uuid6")]
    UuidUuid6,
    #[strum(serialize = "uuid7")]
    UuidUuid7,
    #[strum(serialize = "uuid8")]
    UuidUuid8,
    #[strum(serialize = "UUID")]
    UuidClass,
    #[strum(serialize = "SafeUUID")]
    UuidSafeClass,
    #[strum(serialize = "getnode")]
    UuidGetnode,
    #[strum(serialize = "NAMESPACE_DNS")]
    UuidNamespaceDns,
    #[strum(serialize = "NAMESPACE_URL")]
    UuidNamespaceUrl,
    #[strum(serialize = "NAMESPACE_OID")]
    UuidNamespaceOid,
    #[strum(serialize = "NAMESPACE_X500")]
    UuidNamespaceX500,
    #[strum(serialize = "NIL")]
    UuidNil,
    #[strum(serialize = "MAX")]
    UuidMax,
    #[strum(serialize = "RESERVED_NCS")]
    UuidReservedNcs,
    #[strum(serialize = "RFC_4122")]
    UuidRfc4122,
    #[strum(serialize = "RESERVED_MICROSOFT")]
    UuidReservedMicrosoft,
    #[strum(serialize = "RESERVED_FUTURE")]
    UuidReservedFuture,

    // ==========================
    // base64 module strings
    #[strum(serialize = "base64")]
    Base64,
    #[strum(serialize = "b64encode")]
    B64Encode,
    #[strum(serialize = "b64decode")]
    B64Decode,
    #[strum(serialize = "standard_b64encode")]
    StandardB64Encode,
    #[strum(serialize = "standard_b64decode")]
    StandardB64Decode,
    #[strum(serialize = "urlsafe_b64encode")]
    UrlsafeB64Encode,
    #[strum(serialize = "urlsafe_b64decode")]
    UrlsafeB64Decode,
    #[strum(serialize = "encodebytes")]
    EncodeBytes,
    #[strum(serialize = "decodebytes")]
    DecodeBytes,
    #[strum(serialize = "b32encode")]
    B32Encode,
    #[strum(serialize = "b32decode")]
    B32Decode,
    #[strum(serialize = "b32hexencode")]
    B32HexEncode,
    #[strum(serialize = "b32hexdecode")]
    B32HexDecode,
    #[strum(serialize = "b16encode")]
    B16Encode,
    #[strum(serialize = "b16decode")]
    B16Decode,
    #[strum(serialize = "b85encode")]
    B85Encode,
    #[strum(serialize = "b85decode")]
    B85Decode,
    #[strum(serialize = "a85encode")]
    A85Encode,
    #[strum(serialize = "a85decode")]
    A85Decode,
    #[strum(serialize = "z85encode")]
    Z85Encode,
    #[strum(serialize = "z85decode")]
    Z85Decode,
    #[strum(serialize = "MAXBINSIZE")]
    Base64MaxBinSize,
    #[strum(serialize = "MAXLINESIZE")]
    Base64MaxLineSize,

    // ==========================
    // random module strings
    #[strum(serialize = "random")]
    Random,
    #[strum(serialize = "randint")]
    RdRandint,
    #[strum(serialize = "choice")]
    RdChoice,
    #[strum(serialize = "shuffle")]
    RdShuffle,
    #[strum(serialize = "seed")]
    RdSeed,
    #[strum(serialize = "getstate")]
    RdGetstate,
    #[strum(serialize = "setstate")]
    RdSetstate,
    #[strum(serialize = "uniform")]
    RdUniform,
    #[strum(serialize = "randrange")]
    RdRandrange,
    #[strum(serialize = "randbytes")]
    RdRandbytes,
    #[strum(serialize = "getrandbits")]
    RdGetrandbits,
    #[strum(serialize = "triangular")]
    RdTriangular,
    #[strum(serialize = "expovariate")]
    RdExpovariate,
    #[strum(serialize = "paretovariate")]
    RdParetovariate,
    #[strum(serialize = "weibullvariate")]
    RdWeibullvariate,
    #[strum(serialize = "binomialvariate")]
    RdBinomialvariate,
    #[strum(serialize = "gauss")]
    RdGauss,
    #[strum(serialize = "normalvariate")]
    RdNormalvariate,
    #[strum(serialize = "lognormvariate")]
    RdLognormvariate,
    #[strum(serialize = "gammavariate")]
    RdGammavariate,
    #[strum(serialize = "betavariate")]
    RdBetavariate,
    #[strum(serialize = "vonmisesvariate")]
    RdVonmisesvariate,
    #[strum(serialize = "choices")]
    RdChoices,
    #[strum(serialize = "sample")]
    RdSample,

    // ==========================
    // enum module strings
    #[strum(serialize = "enum")]
    EnumMod,
    #[strum(serialize = "Enum")]
    EnEnum,
    #[strum(serialize = "IntEnum")]
    EnIntEnum,
    #[strum(serialize = "Flag")]
    EnFlag,
    #[strum(serialize = "IntFlag")]
    EnIntFlag,
    #[strum(serialize = "StrEnum")]
    EnStrEnum,
    #[strum(serialize = "EnumType")]
    EnEnumType,
    #[strum(serialize = "EnumMeta")]
    EnEnumMeta,
    #[strum(serialize = "auto")]
    EnAuto,
    #[strum(serialize = "unique")]
    EnUnique,
    #[strum(serialize = "member")]
    EnMember,
    #[strum(serialize = "nonmember")]
    EnNonmember,
    #[strum(serialize = "property")]
    EnProperty,
    #[strum(serialize = "CONFORM")]
    EnConform,
    #[strum(serialize = "EJECT")]
    EnEject,
    #[strum(serialize = "KEEP")]
    EnKeep,
    #[strum(serialize = "STRICT")]
    EnStrict,

    // ==========================
    // copy module strings
    #[strum(serialize = "__copy_module__")]
    CopyMod,
    #[strum(serialize = "deepcopy")]
    Deepcopy,
    #[strum(serialize = "Error")]
    CopyError,

    // ==========================
    // csv module strings
    #[strum(serialize = "csv")]
    Csv,
    #[strum(serialize = "field_size_limit")]
    CsvFieldSizeLimit,
    #[strum(serialize = "get_dialect")]
    CsvGetDialect,
    #[strum(serialize = "list_dialects")]
    CsvListDialects,
    #[strum(serialize = "register_dialect")]
    CsvRegisterDialect,
    #[strum(serialize = "unregister_dialect")]
    CsvUnregisterDialect,
    #[strum(serialize = "DictReader")]
    CsvDictReader,
    #[strum(serialize = "DictWriter")]
    CsvDictWriter,
    #[strum(serialize = "Sniffer")]
    CsvSniffer,
    #[strum(serialize = "sniff")]
    CsvSniff,
    #[strum(serialize = "reader")]
    CsvReader,
    #[strum(serialize = "writer")]
    CsvWriter,
    #[strum(serialize = "__csv_error__")]
    CsvError,
    #[strum(serialize = "excel")]
    CsvExcel,
    #[strum(serialize = "excel_tab")]
    CsvExcelTab,
    #[strum(serialize = "unix_dialect")]
    CsvUnixDialect,
    #[strum(serialize = "QUOTE_ALL")]
    CsvQuoteAll,
    #[strum(serialize = "QUOTE_MINIMAL")]
    CsvQuoteMinimal,
    #[strum(serialize = "QUOTE_NONNUMERIC")]
    CsvQuoteNonnumeric,
    #[strum(serialize = "QUOTE_NONE")]
    CsvQuoteNone,
    #[strum(serialize = "QUOTE_STRINGS")]
    CsvQuoteStrings,
    #[strum(serialize = "QUOTE_NOTNULL")]
    CsvQuoteNotnull,

    // ==========================
    // io module strings
    #[strum(serialize = "io")]
    Io,
    #[strum(serialize = "StringIO")]
    StringIO,
    #[strum(serialize = "BytesIO")]
    BytesIO,
    #[strum(serialize = "DEFAULT_BUFFER_SIZE")]
    DefaultBufferSize,
    #[strum(serialize = "newline")]
    Newline,

    // ==========================
    // array module strings
    #[strum(serialize = "array")]
    ArrayMod,

    // ==========================
    // operator module strings
    // NOTE: We reuse existing variants for "add", "sub", "abs", "index" since they have
    // the same serialize strings. The operator module uses the same StaticStrings variants
    // for these function names.
    #[strum(serialize = "abs")]
    Abs,
    #[strum(serialize = "operator")]
    Operator,
    #[strum(serialize = "call")]
    OperatorCall,
    #[strum(serialize = "mul")]
    OperatorMul,
    #[strum(serialize = "truediv")]
    OperatorTruediv,
    #[strum(serialize = "floordiv")]
    OperatorFloordiv,
    #[strum(serialize = "mod")]
    OperatorMod,
    #[strum(serialize = "neg")]
    OperatorNeg,
    #[strum(serialize = "eq")]
    OperatorEq,
    #[strum(serialize = "ne")]
    OperatorNe,
    #[strum(serialize = "lt")]
    OperatorLt,
    #[strum(serialize = "le")]
    OperatorLe,
    #[strum(serialize = "gt")]
    OperatorGt,
    #[strum(serialize = "ge")]
    OperatorGe,
    #[strum(serialize = "is_")]
    OperatorIs,
    #[strum(serialize = "is_not")]
    OperatorIsNot,
    #[strum(serialize = "is_none")]
    OperatorIsNone,
    #[strum(serialize = "is_not_none")]
    OperatorIsNotNone,
    #[strum(serialize = "not_")]
    OperatorNot,
    #[strum(serialize = "truth")]
    OperatorTruth,
    #[strum(serialize = "getitem")]
    OperatorGetitem,
    #[strum(serialize = "contains")]
    OperatorContains,
    #[strum(serialize = "pos")]
    OperatorPos,
    #[strum(serialize = "and_")]
    OperatorAnd,
    #[strum(serialize = "or_")]
    OperatorOr,
    #[strum(serialize = "xor")]
    OperatorXor,
    #[strum(serialize = "inv")]
    OperatorInv,
    #[strum(serialize = "invert")]
    OperatorInvert,
    #[strum(serialize = "matmul")]
    OperatorMatmul,
    #[strum(serialize = "lshift")]
    OperatorLshift,
    #[strum(serialize = "rshift")]
    OperatorRshift,
    #[strum(serialize = "concat")]
    OperatorConcat,
    #[strum(serialize = "iconcat")]
    OperatorIconcat,
    #[strum(serialize = "countOf")]
    OperatorCountOf,
    #[strum(serialize = "indexOf")]
    OperatorIndexOf,
    #[strum(serialize = "setitem")]
    OperatorSetitem,
    #[strum(serialize = "delitem")]
    OperatorDelitem,
    #[strum(serialize = "length_hint")]
    OperatorLengthHint,
    #[strum(serialize = "itemgetter")]
    OperatorItemgetter,
    #[strum(serialize = "attrgetter")]
    OperatorAttrgetter,
    #[strum(serialize = "methodcaller")]
    OperatorMethodcaller,
    #[strum(serialize = "iadd")]
    OperatorIadd,
    #[strum(serialize = "isub")]
    OperatorIsub,
    #[strum(serialize = "imul")]
    OperatorImul,
    #[strum(serialize = "itruediv")]
    OperatorItruediv,
    #[strum(serialize = "ifloordiv")]
    OperatorIfloordiv,
    #[strum(serialize = "imod")]
    OperatorImod,
    #[strum(serialize = "iand")]
    OperatorIand,
    #[strum(serialize = "ior")]
    OperatorIor,
    #[strum(serialize = "ixor")]
    OperatorIxor,
    #[strum(serialize = "ilshift")]
    OperatorIlshift,
    #[strum(serialize = "irshift")]
    OperatorIrshift,
    #[strum(serialize = "ipow")]
    OperatorIpow,
    #[strum(serialize = "imatmul")]
    OperatorImatmul,

    // ==========================
    // bisect module strings
    #[strum(serialize = "bisect")]
    Bisect,
    #[strum(serialize = "bisect_left")]
    BisectLeft,
    #[strum(serialize = "bisect_right")]
    BisectRight,
    #[strum(serialize = "insort_left")]
    InsortLeft,
    #[strum(serialize = "insort_right")]
    InsortRight,
    #[strum(serialize = "insort")]
    Insort,

    // ==========================
    // struct module strings
    // ==========================
    // struct module strings
    #[strum(serialize = "struct")]
    StructMod,
    #[strum(serialize = "pack")]
    StructPack,
    #[strum(serialize = "unpack")]
    StructUnpack,
    #[strum(serialize = "calcsize")]
    StructCalcsize,
    #[strum(serialize = "iter_unpack")]
    StructIterUnpack,
    #[strum(serialize = "pack_into")]
    StructPackInto,
    #[strum(serialize = "unpack_from")]
    StructUnpackFrom,
    #[strum(serialize = "Struct")]
    StructType,
    #[strum(serialize = "__struct_format__")]
    StructFormatAttr,
    #[strum(serialize = "size")]
    Size,

    // ==========================
    // heapq module strings
    #[strum(serialize = "heapq")]
    Heapq,
    #[strum(serialize = "heappush")]
    HqHeappush,
    #[strum(serialize = "heappop")]
    HqHeappop,
    #[strum(serialize = "heapify")]
    HqHeapify,
    #[strum(serialize = "heapify_max")]
    HqHeapifyMax,
    #[strum(serialize = "nlargest")]
    HqNlargest,
    #[strum(serialize = "nsmallest")]
    HqNsmallest,
    #[strum(serialize = "heappushpop")]
    HqHeappushpop,
    #[strum(serialize = "heappush_max")]
    HqHeappushMax,
    #[strum(serialize = "heappop_max")]
    HqHeappopMax,
    #[strum(serialize = "heappushpop_max")]
    HqHeappushpopMax,
    #[strum(serialize = "heapreplace")]
    HqHeapreplace,
    #[strum(serialize = "heapreplace_max")]
    HqHeapreplaceMax,
    #[strum(serialize = "merge")]
    HqMerge,
    // Shared name used by `str.format(...)` method dispatch.
    #[strum(serialize = "format")]
    Format,

    // ==========================
    // datetime module strings
    #[strum(serialize = "datetime")]
    Datetime,
    #[strum(serialize = "timedelta")]
    Timedelta,
    #[strum(serialize = "date")]
    Date,
    #[strum(serialize = "time")]
    Time,
    #[strum(serialize = "timezone")]
    Timezone,
    #[strum(serialize = "tzinfo")]
    Tzinfo,
    #[strum(serialize = "UTC")]
    Utc,
    #[strum(serialize = "MINYEAR")]
    Minyear,
    #[strum(serialize = "MAXYEAR")]
    Maxyear,
    // timedelta attributes
    #[strum(serialize = "days")]
    Days,
    #[strum(serialize = "seconds")]
    Seconds,
    #[strum(serialize = "microseconds")]
    Microseconds,
    #[strum(serialize = "total_seconds")]
    TotalSeconds,
    // date/datetime attributes
    #[strum(serialize = "year")]
    Year,
    #[strum(serialize = "month")]
    Month,
    #[strum(serialize = "day")]
    Day,
    // time/datetime attributes
    #[strum(serialize = "hour")]
    Hour,
    #[strum(serialize = "minute")]
    Minute,
    #[strum(serialize = "second")]
    Second,
    // datetime methods
    #[strum(serialize = "today")]
    Today,
    #[strum(serialize = "now")]
    Now,
    #[strum(serialize = "utcnow")]
    Utcnow,
    #[strum(serialize = "fromtimestamp")]
    Fromtimestamp,
    #[strum(serialize = "utcfromtimestamp")]
    Utcfromtimestamp,
    #[strum(serialize = "fromordinal")]
    Fromordinal,
    #[strum(serialize = "toordinal")]
    Toordinal,
    #[strum(serialize = "fromisoformat")]
    Fromisoformat,
    #[strum(serialize = "fromisocalendar")]
    Fromisocalendar,
    #[strum(serialize = "isocalendar")]
    Isocalendar,
    #[strum(serialize = "isoweekday")]
    Isoweekday,
    #[strum(serialize = "weekday")]
    Weekday,
    #[strum(serialize = "ctime")]
    Ctime,
    #[strum(serialize = "timetuple")]
    Timetuple,
    #[strum(serialize = "utctimetuple")]
    Utctimetuple,
    #[strum(serialize = "strftime")]
    Strftime,
    #[strum(serialize = "strptime")]
    Strptime,
    #[strum(serialize = "isoformat")]
    Isoformat,
    #[strum(serialize = "combine")]
    Combine,
    #[strum(serialize = "timestamp")]
    Timestamp,
    DateFn,
    TimeFn,
    #[strum(serialize = "timetz")]
    Timetz,
    #[strum(serialize = "utcoffset")]
    Utcoffset,
    #[strum(serialize = "tzname")]
    Tzname,
    #[strum(serialize = "dst")]
    Dst,
    #[strum(serialize = "fold")]
    Fold,
    #[strum(serialize = "astimezone")]
    Astimezone,

    // ==========================
    // decimal module strings
    #[strum(serialize = "decimal")]
    Decimal,
    #[strum(serialize = "Decimal")]
    DecimalClass,
    #[strum(serialize = "quantize")]
    DecQuantize,
    #[strum(serialize = "to_eng_string")]
    DecToEngString,
    #[strum(serialize = "copy_abs")]
    DecCopyAbs,
    #[strum(serialize = "copy_negate")]
    DecCopyNegate,
    #[strum(serialize = "copy_sign")]
    DecCopySign,
    #[strum(serialize = "is_finite")]
    DecIsFinite,
    #[strum(serialize = "is_infinite")]
    DecIsInfinite,
    #[strum(serialize = "is_nan")]
    DecIsNan,
    #[strum(serialize = "is_zero")]
    DecIsZero,
    #[strum(serialize = "is_signed")]
    DecIsSigned,

    // ==========================
    // fractions module strings
    #[strum(serialize = "fractions")]
    Fractions,
    #[strum(serialize = "Fraction")]
    FractionClass,
    #[strum(serialize = "from_float")]
    FracFromFloat,
    #[strum(serialize = "from_decimal")]
    FracFromDecimal,
    #[strum(serialize = "limit_denominator")]
    FracLimitDenominator,
    #[strum(serialize = "as_integer_ratio")]
    FracAsIntegerRatio,
    #[strum(serialize = "numerator")]
    FracNumerator,
    #[strum(serialize = "denominator")]
    FracDenominator,

    // ==========================
    // pprint module strings
    #[strum(serialize = "pprint")]
    Pprint,
    #[strum(serialize = "pformat")]
    Pformat,
    #[strum(serialize = "pp")]
    Pp,
    #[strum(serialize = "isreadable")]
    Isreadable,
    #[strum(serialize = "isrecursive")]
    Isrecursive,
    #[strum(serialize = "saferepr")]
    Saferepr,
    #[strum(serialize = "PrettyPrinter")]
    PrettyPrinter,
    #[strum(serialize = "stream")]
    Stream,
    #[strum(serialize = "depth")]
    Depth,
    #[strum(serialize = "compact")]
    Compact,
    #[strum(serialize = "sort_dicts")]
    SortDicts,
    #[strum(serialize = "underscore_numbers")]
    UnderscoreNumbers,

    // ==========================
    // time module strings (sandboxed)
    #[strum(serialize = "__time_module__")]
    TimeMod,
    #[strum(serialize = "timeit")]
    Timeit,
    #[strum(serialize = "monotonic")]
    Monotonic,
    #[strum(serialize = "monotonic_ns")]
    MonotonicNs,
    #[strum(serialize = "time_ns")]
    TimeNs,
    #[strum(serialize = "perf_counter")]
    PerfCounter,
    #[strum(serialize = "perf_counter_ns")]
    PerfCounterNs,
    #[strum(serialize = "process_time")]
    ProcessTime,
    #[strum(serialize = "process_time_ns")]
    ProcessTimeNs,
    #[strum(serialize = "thread_time")]
    ThreadTime,
    #[strum(serialize = "thread_time_ns")]
    ThreadTimeNs,
    #[strum(serialize = "gmtime")]
    Gmtime,
    #[strum(serialize = "localtime")]
    Localtime,
    #[strum(serialize = "mktime")]
    Mktime,
    #[strum(serialize = "asctime")]
    Asctime,
    #[strum(serialize = "struct_time")]
    StructTime,
    #[strum(serialize = "get_clock_info")]
    GetClockInfo,
    #[strum(serialize = "altzone")]
    Altzone,
    #[strum(serialize = "daylight")]
    Daylight,
    #[strum(serialize = "adjustable")]
    Adjustable,
    #[strum(serialize = "resolution")]
    Resolution,
    #[strum(serialize = "CLOCK_MONOTONIC")]
    ClockMonotonic,
    #[strum(serialize = "CLOCK_MONOTONIC_RAW")]
    ClockMonotonicRaw,
    #[strum(serialize = "CLOCK_REALTIME")]
    ClockRealtime,
    #[strum(serialize = "CLOCK_PROCESS_CPUTIME_ID")]
    ClockProcessCputimeId,
    #[strum(serialize = "CLOCK_THREAD_CPUTIME_ID")]
    ClockThreadCputimeId,

    // ==========================
    // Additional module names (wave 2 stdlib)
    #[strum(serialize = "shelve")]
    Shelve,
    #[strum(serialize = "traceback")]
    Traceback,
    #[strum(serialize = "secrets")]
    Secrets,
    #[strum(serialize = "errno")]
    Errno,
    #[strum(serialize = "linecache")]
    Linecache,
    #[strum(serialize = "queue")]
    Queue,
    #[strum(serialize = "token")]
    TokenMod,
    #[strum(serialize = "threading")]
    Threading,
}

impl StaticStrings {
    /// Attempts to convert a `StringId` back to a `StaticStrings` variant.
    ///
    /// Returns `None` if the `StringId` doesn't correspond to a static string
    /// (e.g., it's an ASCII char or a dynamically interned string).
    pub fn from_string_id(id: StringId) -> Option<Self> {
        let enum_id = id.0.checked_sub(STATIC_STRING_ID_OFFSET)?;
        u16::try_from(enum_id).ok().and_then(Self::from_repr)
    }
}

/// Converts this static string variant to its corresponding `StringId`.
impl From<StaticStrings> for StringId {
    fn from(value: StaticStrings) -> Self {
        let string_id = value as u32;
        Self(string_id + STATIC_STRING_ID_OFFSET)
    }
}

impl From<StaticStrings> for Value {
    fn from(value: StaticStrings) -> Self {
        Self::InternString(value.into())
    }
}

impl PartialEq<StaticStrings> for StringId {
    fn eq(&self, other: &StaticStrings) -> bool {
        *self == Self::from(*other)
    }
}

impl PartialEq<StringId> for StaticStrings {
    fn eq(&self, other: &StringId) -> bool {
        StringId::from(*self) == *other
    }
}

/// Index into the bytes interner's storage.
///
/// Separate from `StringId` to distinguish string vs bytes literals at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BytesId(u32);

impl BytesId {
    /// Returns the raw index value.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index into the long integer interner's storage.
///
/// Used for integer literals that exceed i64 range. The actual `BigInt` values
/// are stored in the `Interns` table and looked up by index at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LongIntId(u32);

impl LongIntId {
    /// Returns the raw index value.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Unique identifier for functions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct FunctionId(u32);

impl FunctionId {
    /// Creates a FunctionId from a raw index value.
    ///
    /// Used by the bytecode VM to reconstruct FunctionIds from operands stored
    /// in bytecode. The caller is responsible for ensuring the index is valid.
    #[inline]
    pub fn from_index(index: u16) -> Self {
        Self(u32::from(index))
    }

    /// Returns the raw index value.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Unique identifier for external functions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ExtFunctionId(u32);

impl ExtFunctionId {
    pub fn new(index: usize) -> Self {
        Self(index.try_into().expect("Invalid external function id"))
    }

    /// Returns the raw index value.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A string, bytes, and long integer interner that stores unique values and returns indices for lookup.
///
/// Interns are deduplicated on insertion - interning the same string twice returns
/// the same `StringId`. Bytes and long integers are NOT deduplicated (rare enough that it's not worth it).
/// The interner owns all strings/bytes/long integers and provides lookup by index.
///
/// # Thread Safety
///
/// The interner is not thread-safe. It's designed to be used single-threaded during
/// parsing/preparation, then the values are accessed read-only during execution.
#[derive(Debug, Default, Clone)]
pub struct InternerBuilder {
    /// Maps strings to their indices for deduplication during interning.
    string_map: AHashMap<String, StringId>,
    /// Storage for interned interns, indexed by `StringId`.
    strings: Vec<String>,
    /// Storage for interned bytes literals, indexed by `BytesId`.
    /// Not deduplicated since bytes literals are rare.
    bytes: Vec<Vec<u8>>,
    /// Storage for interned long integer literals, indexed by `LongIntId`.
    /// Not deduplicated since long integer literals are rare.
    long_ints: Vec<BigInt>,
}

impl InternerBuilder {
    /// Creates a new string interner with pre-interned strings.
    ///
    /// Clones from a lazily-initialized base interner that contains all pre-interned
    /// strings (`<module>`, attribute names, ASCII characters). This avoids rebuilding
    /// the base set on every call.
    ///
    /// # Arguments
    /// * `code` - The code being parsed, used for a very rough guess at how many
    ///   additional strings will be interned beyond the base set.
    ///
    /// Pre-interns (via `BASE_INTERNER`):
    /// - Index 0: `"<module>"` for module-level code
    /// - Indices 1-MAX_ATTR_ID: Known attribute names (append, insert, get, join, etc.)
    /// - Indices MAX_ATTR_ID+1..: ASCII single-character strings
    pub fn new(code: &str) -> Self {
        // Reserve capacity for code-specific strings
        // Rough guess: count quotes and divide by 2 (open+close per string)
        let capacity = code.bytes().filter(|&b| b == b'"' || b == b'\'').count() >> 1;
        Self {
            string_map: AHashMap::with_capacity(capacity),
            strings: Vec::with_capacity(capacity),
            bytes: Vec::new(),
            long_ints: Vec::new(),
        }
    }

    /// Interns a string, returning its `StringId`.
    ///
    /// * If the string is ascii, return the pre-interned string id
    /// * If the string is a known static string, return the pre-interned string id
    /// * If the string was already interned, returns the existing string id
    /// * Otherwise, stores the string and returns a new string id
    pub fn intern(&mut self, s: &str) -> StringId {
        if s.len() == 1 {
            StringId::from_ascii(s.as_bytes()[0])
        } else if let Ok(ss) = StaticStrings::from_str(s) {
            ss.into()
        } else {
            *self.string_map.entry(s.to_owned()).or_insert_with(|| {
                let string_id = self.strings.len() + INTERN_STRING_ID_OFFSET;
                let id = StringId(string_id.try_into().expect("StringId overflow"));
                self.strings.push(s.to_owned());
                id
            })
        }
    }

    /// Interns bytes, returning its `BytesId`.
    ///
    /// Unlike interns, bytes are not deduplicated (bytes literals are rare).
    pub fn intern_bytes(&mut self, b: &[u8]) -> BytesId {
        let id = BytesId(self.bytes.len().try_into().expect("BytesId overflow"));
        self.bytes.push(b.to_vec());
        id
    }

    /// Interns a long integer, returning its `LongIntId`.
    ///
    /// Big integers are not deduplicated since literals exceeding i64 are rare.
    pub fn intern_long_int(&mut self, bi: BigInt) -> LongIntId {
        let id = LongIntId(self.long_ints.len().try_into().expect("LongIntId overflow"));
        self.long_ints.push(bi);
        id
    }

    /// Returns the number of dynamically interned strings.
    ///
    /// This counts only strings interned at runtime (not the base set of
    /// pre-interned attribute names and ASCII characters that are always present).
    #[must_use]
    pub fn interned_string_count(&self) -> usize {
        self.strings.len()
    }

    /// Looks up a string by its `StringId`.
    #[inline]
    pub fn get_str(&self, id: StringId) -> &str {
        get_str(&self.strings, id)
    }

    /// Looks up a `StringId` by its string value.
    ///
    /// Returns `Some(id)` if the string was previously interned, `None` otherwise.
    /// This is the inverse of `get_str()` for strings that have already been interned.
    #[must_use]
    pub fn try_get_str_id(&self, s: &str) -> Option<StringId> {
        // Check single ASCII character
        if s.len() == 1 {
            return Some(StringId::from_ascii(s.as_bytes()[0]));
        }
        // Check known static strings
        if let Ok(ss) = StaticStrings::from_str(s) {
            return Some(ss.into());
        }
        // Check user-interned strings
        self.string_map.get(s).copied()
    }

    /// Clones interned backing data for constructing a read-only [`Interns`] view.
    ///
    /// This is primarily used by REPL execution, where the session keeps owning the
    /// mutable `InternerBuilder` but needs temporary immutable intern tables for
    /// compilation and execution.
    pub(crate) fn clone_data(&self) -> (Vec<String>, Vec<Vec<u8>>, Vec<BigInt>) {
        (self.strings.clone(), self.bytes.clone(), self.long_ints.clone())
    }

    /// Reconstructs an `InternerBuilder` from its serialized components.
    ///
    /// This is the inverse of `clone_data()` and is used when loading a saved
    /// session from disk. The `string_map` is rebuilt by iterating over the
    /// strings and mapping each to its `StringId` based on position.
    pub(crate) fn from_parts(strings: Vec<String>, bytes: Vec<Vec<u8>>, long_ints: Vec<BigInt>) -> Self {
        let mut string_map = AHashMap::with_capacity(strings.len());
        for (index, s) in strings.iter().enumerate() {
            let string_id = index + INTERN_STRING_ID_OFFSET;
            let id = StringId(string_id.try_into().expect("StringId overflow"));
            string_map.insert(s.clone(), id);
        }
        Self {
            string_map,
            strings,
            bytes,
            long_ints,
        }
    }
}

/// Looks up a string by its `StringId`.
///
/// # Panics
///
/// Panics if the `StringId` is invalid - not from this interner or ascii chars or StaticStrings.
fn get_str(strings: &[String], id: StringId) -> &str {
    if let Ok(c) = u8::try_from(id.0) {
        ASCII_STRS[c as usize]
    } else if let Some(intern_index) = id.index().checked_sub(INTERN_STRING_ID_OFFSET) {
        &strings[intern_index]
    } else {
        let static_str = StaticStrings::from_string_id(id).expect("Invalid static string ID");
        static_str.into()
    }
}

/// Read-only storage for interned strings, bytes, and long integers.
///
/// This provides lookup by `StringId`, `BytesId`, `LongIntId` and `FunctionId` for interned literals and functions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Interns {
    strings: Vec<String>,
    bytes: Vec<Vec<u8>>,
    long_ints: Vec<BigInt>,
    functions: Vec<Function>,
    external_functions: Vec<String>,
}

impl Interns {
    pub fn new(interner: InternerBuilder, functions: Vec<Function>, external_functions: Vec<String>) -> Self {
        Self {
            strings: interner.strings,
            bytes: interner.bytes,
            long_ints: interner.long_ints,
            functions,
            external_functions,
        }
    }

    /// Creates an `Interns` instance for REPL compilation/execution from borrowed state.
    ///
    /// This keeps the session's `InternerBuilder` alive for future lines while
    /// materializing an owned `Interns` snapshot for a single run.
    pub(crate) fn new_for_repl(
        interner: &InternerBuilder,
        functions: Vec<Function>,
        external_functions: Vec<String>,
    ) -> Self {
        let (strings, bytes, long_ints) = interner.clone_data();
        Self {
            strings,
            bytes,
            long_ints,
            functions,
            external_functions,
        }
    }

    /// Looks up a string by its `StringId`.
    ///
    /// # Panics
    ///
    /// Panics if the `StringId` is invalid.
    #[inline]
    pub fn get_str(&self, id: StringId) -> &str {
        get_str(&self.strings, id)
    }

    /// Tries to find the `StringId` for a given string, returning `None` if not interned.
    #[must_use]
    pub fn try_get_str_id(&self, s: &str) -> Option<StringId> {
        if s.len() == 1 {
            return Some(StringId::from_ascii(s.as_bytes()[0]));
        }
        if let Ok(ss) = StaticStrings::from_str(s) {
            return Some(ss.into());
        }
        self.strings.iter().position(|existing| existing == s).map(|index| {
            let raw_index = index + INTERN_STRING_ID_OFFSET;
            StringId(raw_index.try_into().expect("StringId overflow"))
        })
    }

    /// Looks up bytes by their `BytesId`.
    ///
    /// # Panics
    ///
    /// Panics if the `BytesId` is invalid.
    #[inline]
    pub fn get_bytes(&self, id: BytesId) -> &[u8] {
        &self.bytes[id.index()]
    }

    /// Looks up a long integer by its `LongIntId`.
    ///
    /// # Panics
    ///
    /// Panics if the `LongIntId` is invalid.
    #[inline]
    pub fn get_long_int(&self, id: LongIntId) -> &BigInt {
        &self.long_ints[id.index()]
    }

    /// Lookup a function by its `FunctionId`
    ///
    /// # Panics
    ///
    /// Panics if the `FunctionId` is invalid.
    #[inline]
    pub fn get_function(&self, id: FunctionId) -> &Function {
        self.functions.get(id.index()).expect("Function not found")
    }

    /// Lookup an external function name by its `ExtFunctionId`
    ///
    /// # Panics
    ///
    /// Panics if the `ExtFunctionId` is invalid.
    #[inline]
    pub fn get_external_function_name(&self, id: ExtFunctionId) -> String {
        self.external_functions
            .get(id.index())
            .expect("External function not found")
            .clone()
    }

    /// Sets the compiled functions.
    ///
    /// This is called after compilation to populate the functions that were
    /// compiled from `PreparedFunctionDef` nodes.
    pub fn set_functions(&mut self, functions: Vec<Function>) {
        self.functions = functions;
    }
}
