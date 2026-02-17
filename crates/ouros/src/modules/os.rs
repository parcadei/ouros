//! Implementation of the `os` module.
//!
//! Provides a sandboxed implementation of Python's `os` module with:
//! - `getenv(key, default=None)`: Get a single environment variable (yields to host)
//! - `environ`: Property that returns the entire environment as a dict (yields to host)
//! - `getcwd()`: Returns `'/'` (sandboxed — no real filesystem)
//! - `cpu_count()`: Returns `None` (sandboxed — no real hardware info)
//! - `urandom(n)`: Returns `n` random bytes (safe — uses in-process PRNG)
//! - `name`: OS name constant (`'posix'` on Unix, `'nt'` on Windows)
//! - `sep`, `pathsep`, `linesep`, `altsep`, `curdir`, `pardir`, `extsep`, `devnull`: Path constants
//! - `path`: Sub-module with pure string-manipulation path functions (join, dirname, etc.)
//!
//! # Security
//!
//! All functions are sandboxed. `getenv` and `environ` yield to the host via `OsFunction`
//! callbacks — the host decides what (if anything) to expose. `getcwd` always returns `'/'`,
//! `cpu_count` always returns `None`, and `urandom` uses an in-process PRNG rather than
//! reading from `/dev/urandom`.

use std::{cell::RefCell, env};

use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    os::OsFunction,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Bytes, Module, OurosIter, Property, PyTrait, Str},
    value::Value,
};

thread_local! {
    /// Thread-local RNG for `os.urandom()`.
    ///
    /// Seeded from entropy at startup. This avoids accessing the real `/dev/urandom`
    /// or any OS random device, keeping the sandbox hermetic.
    static OS_RNG: RefCell<StdRng> = RefCell::new(StdRng::from_entropy());
    /// Thread-local virtual cwd stack used by sandboxed `os.getcwd()`/`contextlib.chdir()`.
    static OS_CWD_STACK: RefCell<Vec<String>> = RefCell::new(vec!["/".to_owned()]);
}

/// Returns the sandbox virtual current working directory.
#[must_use]
pub(crate) fn current_working_dir() -> String {
    OS_CWD_STACK.with(|stack| stack.borrow().last().cloned().unwrap_or_else(|| "/".to_owned()))
}

/// Pushes a new virtual current working directory for the current thread.
pub(crate) fn push_working_dir(path: String) {
    OS_CWD_STACK.with(|stack| stack.borrow_mut().push(path));
}

/// Restores the previous virtual current working directory if present.
///
/// The root entry is always retained to keep the stack non-empty.
pub(crate) fn pop_working_dir() {
    OS_CWD_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.len() > 1 {
            let _ = stack.pop();
        }
    });
}

/// OS module functions.
///
/// Each variant corresponds to a callable function exposed on the `os` module.
/// Functions that need host involvement (like `getenv`) return `AttrCallResult::OsCall`.
/// Pure sandboxed functions (like `getcwd`) return values directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum OsFunctions {
    /// `os.getenv(key, default=None)` — yields to host
    Getenv,
    /// `os.getcwd()` — returns `'/'` (sandboxed)
    Getcwd,
    /// `os.fspath(path)` — returns filesystem path representation for path-like objects.
    Fspath,
    /// `os.cpu_count()` — returns `None` (sandboxed)
    #[strum(serialize = "cpu_count")]
    CpuCount,
    /// `os.urandom(n)` — returns n random bytes
    Urandom,
    /// `os.strerror(code)` — returns the platform error description
    Strerror,
}

/// `os.path` sub-module functions.
///
/// All of these are pure string-manipulation functions — they never touch the
/// real filesystem and are safe for sandboxed execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum OsPathFunctions {
    /// `os.path.join(a, *p)` — join path components
    Join,
    /// `os.path.exists(p)` — always returns `False` in sandbox
    Exists,
    /// `os.path.dirname(p)` — directory component of path
    Dirname,
    /// `os.path.basename(p)` — final component of path
    Basename,
    /// `os.path.split(p)` — split into `(dirname, basename)`
    Split,
    /// `os.path.splitext(p)` — split into `(root, ext)`
    Splitext,
    /// `os.path.splitdrive(p)` — split into `(drive, path)`
    Splitdrive,
    /// `os.path.splitroot(p)` — split into `(drive, root, tail)`
    Splitroot,
    /// `os.path.isabs(p)` — check if path is absolute
    Isabs,
    /// `os.path.abspath(p)` — return normalized absolutized version
    Abspath,
    /// `os.path.normpath(p)` — normalize path (collapse `.`, `..`, double slashes)
    Normpath,
    /// `os.path.commonpath(paths)` — longest common path prefix
    Commonpath,
    /// `os.path.commonprefix(list)` — longest common string prefix
    Commonprefix,
    /// `os.path.relpath(path, start='.')` — relative path between two locations
    Relpath,
    /// `os.path.lexists(p)` — existence check without following symlinks
    Lexists,
    /// `os.path.isfile(p)` — file check (always `False` in sandbox)
    Isfile,
    /// `os.path.isdir(p)` — directory check (always `False` in sandbox)
    Isdir,
    /// `os.path.islink(p)` — symlink check (always `False` in sandbox)
    Islink,
    /// `os.path.ismount(p)` — mount-point check
    Ismount,
    /// `os.path.isjunction(p)` — Windows junction check (always `False` on POSIX)
    Isjunction,
    /// `os.path.isdevdrive(p)` — Windows Dev Drive check (always `False` on POSIX)
    Isdevdrive,
    /// `os.path.normcase(s)` — case normalization (identity on POSIX)
    Normcase,
    /// `os.path.realpath(p)` — canonicalized absolute path
    Realpath,
    /// `os.path.expanduser(path)` — expand leading `~` user home marker
    Expanduser,
    /// `os.path.expandvars(path)` — expand `$VAR` and `${VAR}` markers
    Expandvars,
    /// `os.path.getsize(path)` — file size (returns `int` in sandbox)
    Getsize,
    /// `os.path.getmtime(path)` — file modified time (returns `float` in sandbox)
    Getmtime,
    /// `os.path.getatime(path)` — file access time (returns `float` in sandbox)
    Getatime,
    /// `os.path.getctime(path)` — file metadata change time (returns `float` in sandbox)
    Getctime,
}

// ===========================================================================
// Module creation
// ===========================================================================

/// Creates the `os` module and allocates it on the heap.
///
/// Sets up:
/// - Constants: `name`, `sep`, `pathsep`, `linesep`, `altsep`, `curdir`, `pardir`, `extsep`, `devnull`
/// - Functions: `getenv`, `getcwd`, `cpu_count`, `urandom`
/// - Property: `environ`
/// - Sub-module: `path` (with `join`, `exists`, `dirname`, `basename`, `split`, `splitext`,
///   `isabs`, `abspath`, `normpath`)
///
/// # Returns
/// A `HeapId` pointing to the newly allocated module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Os);

    // --- Constants ---
    set_os_constants(&mut module, heap, interns)?;

    // --- Functions ---
    module.set_attr(
        StaticStrings::Getenv,
        Value::ModuleFunction(ModuleFunctions::Os(OsFunctions::Getenv)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::OsGetcwd,
        Value::ModuleFunction(ModuleFunctions::Os(OsFunctions::Getcwd)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::OsFspath,
        Value::ModuleFunction(ModuleFunctions::Os(OsFunctions::Fspath)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::OsCpuCount,
        Value::ModuleFunction(ModuleFunctions::Os(OsFunctions::CpuCount)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::OsUrandom,
        Value::ModuleFunction(ModuleFunctions::Os(OsFunctions::Urandom)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::OsStrerror,
        Value::ModuleFunction(ModuleFunctions::Os(OsFunctions::Strerror)),
        heap,
        interns,
    );

    // --- Property ---
    module.set_attr(
        StaticStrings::Environ,
        Value::Property(Property::Os(OsFunction::GetEnviron)),
        heap,
        interns,
    );

    // --- os.path sub-module ---
    let path_mod_id = create_os_path_module(heap, interns)?;
    module.set_attr(StaticStrings::SysPath, Value::Ref(path_mod_id), heap, interns);

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to an os module function.
///
/// Returns `AttrCallResult::OsCall` for functions that need host involvement,
/// or `AttrCallResult::Value` for pure sandboxed functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    functions: OsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match functions {
        OsFunctions::Getenv => getenv(heap, args),
        OsFunctions::Getcwd => getcwd(heap, args),
        OsFunctions::Fspath => fspath(heap, interns, args),
        OsFunctions::CpuCount => cpu_count(heap, args),
        OsFunctions::Urandom => urandom(heap, args),
        OsFunctions::Strerror => strerror(heap, args),
    }
}

/// Dispatches a call to an `os.path` sub-module function.
///
/// All os.path functions are pure string manipulation — no I/O or host callbacks needed.
pub(super) fn call_path(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: OsPathFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        OsPathFunctions::Join => os_path_join(heap, interns, args),
        OsPathFunctions::Exists => os_path_exists(heap, args),
        OsPathFunctions::Dirname => os_path_dirname(heap, interns, args),
        OsPathFunctions::Basename => os_path_basename(heap, interns, args),
        OsPathFunctions::Split => os_path_split(heap, interns, args),
        OsPathFunctions::Splitext => os_path_splitext(heap, interns, args),
        OsPathFunctions::Splitdrive => os_path_splitdrive(heap, interns, args),
        OsPathFunctions::Splitroot => os_path_splitroot(heap, interns, args),
        OsPathFunctions::Isabs => os_path_isabs(heap, interns, args),
        OsPathFunctions::Abspath => os_path_abspath(heap, interns, args),
        OsPathFunctions::Normpath => os_path_normpath(heap, interns, args),
        OsPathFunctions::Commonpath => os_path_commonpath(heap, interns, args),
        OsPathFunctions::Commonprefix => os_path_commonprefix(heap, interns, args),
        OsPathFunctions::Relpath => os_path_relpath(heap, interns, args),
        OsPathFunctions::Lexists => os_path_lexists(heap, args),
        OsPathFunctions::Isfile => os_path_isfile(heap, args),
        OsPathFunctions::Isdir => os_path_isdir(heap, args),
        OsPathFunctions::Islink => os_path_islink(heap, args),
        OsPathFunctions::Ismount => os_path_ismount(heap, interns, args),
        OsPathFunctions::Isjunction => os_path_isjunction(heap, args),
        OsPathFunctions::Isdevdrive => os_path_isdevdrive(heap, args),
        OsPathFunctions::Normcase => os_path_normcase(heap, interns, args),
        OsPathFunctions::Realpath => os_path_realpath(heap, interns, args),
        OsPathFunctions::Expanduser => os_path_expanduser(heap, interns, args),
        OsPathFunctions::Expandvars => os_path_expandvars(heap, interns, args),
        OsPathFunctions::Getsize => os_path_getsize(heap, interns, args),
        OsPathFunctions::Getmtime => os_path_getmtime(heap, interns, args),
        OsPathFunctions::Getatime => os_path_getatime(heap, interns, args),
        OsPathFunctions::Getctime => os_path_getctime(heap, interns, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// os module functions
// ===========================================================================

/// Implementation of `os.getenv(key, default=None)`.
///
/// Yields to the host to perform the actual environment lookup.
/// The host decides what environment variables (if any) to expose.
fn getenv(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (key, default) = args.get_one_two_args("os.getenv", heap)?;

    if key.is_str(heap) {
        let final_default = default.unwrap_or(Value::None);
        let args = ArgValues::Two(key, final_default);
        Ok(AttrCallResult::OsCall(OsFunction::Getenv, args))
    } else {
        let type_name = key.py_type(heap);
        key.drop_with_heap(heap);
        if let Some(d) = default {
            d.drop_with_heap(heap);
        }
        Err(ExcType::type_error(format!("str expected, not {type_name}")))
    }
}

/// Implementation of `os.getcwd()`.
///
/// Returns the sandbox virtual current working directory.
fn getcwd(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("os.getcwd", heap)?;
    let s = heap.allocate(HeapData::Str(Str::new(current_working_dir())))?;
    Ok(AttrCallResult::Value(Value::Ref(s)))
}

/// Implementation of `os.fspath(path)`.
///
/// Accepts string/bytes values and pathlib path values, returning the native
/// filesystem path representation.
fn fspath(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("os.fspath", heap)?;
    let result = match &value {
        Value::InternString(id) => Value::Ref(heap.allocate(HeapData::Str(Str::new(interns.get_str(*id).to_owned())))?),
        Value::InternBytes(id) => {
            Value::Ref(heap.allocate(HeapData::Bytes(Bytes::new(interns.get_bytes(*id).to_vec())))?)
        }
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Value::Ref(heap.allocate(HeapData::Str(Str::new(s.as_str().to_owned())))?),
            HeapData::Bytes(b) => Value::Ref(heap.allocate(HeapData::Bytes(Bytes::new(b.as_slice().to_vec())))?),
            HeapData::Path(p) => Value::Ref(heap.allocate(HeapData::Str(Str::new(p.display_path())))?),
            _ => {
                let type_name = value.py_type(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "expected str, bytes or os.PathLike object, not {type_name}",
                )));
            }
        },
        _ => {
            let type_name = value.py_type(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "expected str, bytes or os.PathLike object, not {type_name}",
            )));
        }
    };

    value.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `os.cpu_count()`.
///
/// Always returns `None` in the sandbox — hardware information is not exposed.
fn cpu_count(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("os.cpu_count", heap)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `os.urandom(n)`.
///
/// Returns `n` random bytes using an in-process PRNG. Does not access
/// the real `/dev/urandom` or any OS random device.
fn urandom(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let n_val = args.get_one_arg("os.urandom", heap)?;
    let n = n_val.as_int(heap)?;
    n_val.drop_with_heap(heap);

    if n < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "negative argument not allowed").into());
    }

    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let n = n as usize;

    let bytes = OS_RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        let mut buf = vec![0u8; n];
        rng.fill(buf.as_mut_slice());
        buf
    });

    let id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `os.strerror(code)`.
///
/// Converts a platform errno value to a human-readable message.
fn strerror(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let code = args.get_one_arg("os.strerror", heap)?;
    let code_i64 = code.as_int(heap)?;
    code.drop_with_heap(heap);

    #[expect(clippy::cast_possible_truncation)]
    let code_i32 = code_i64 as i32;
    let message = std::io::Error::from_raw_os_error(code_i32).to_string();
    let id = heap.allocate(HeapData::Str(Str::new(message)))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

// ===========================================================================
// os module helpers
// ===========================================================================

/// Sets platform-dependent constants on the os module.
///
/// Uses sandboxed POSIX-style values for path constants:
/// - `sep`: `'/'`
/// - `pathsep`: `':'`
/// - `linesep`: `'\n'`
/// - `altsep`: `None`
/// - `curdir`, `pardir`, `extsep`: `'.'`, `'..'`, `'.'`
/// - `devnull`: `'/dev/null'`
fn set_os_constants(
    module: &mut Module,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    // os.name
    let name_str = if cfg!(windows) { "nt" } else { "posix" };
    let name_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(name_str.to_owned())))?);
    module.set_attr(StaticStrings::Name, name_val, heap, interns);

    // os.sep
    let sep_val = Value::Ref(heap.allocate(HeapData::Str(Str::new("/".to_owned())))?);
    module.set_attr(StaticStrings::OsSep, sep_val, heap, interns);

    // os.pathsep
    let pathsep_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(":".to_owned())))?);
    module.set_attr(StaticStrings::OsPathsep, pathsep_val, heap, interns);

    // os.linesep
    let linesep_val = Value::Ref(heap.allocate(HeapData::Str(Str::new("\n".to_owned())))?);
    module.set_attr(StaticStrings::OsLinesep, linesep_val, heap, interns);

    // os.altsep
    module.set_attr(StaticStrings::OsAltsep, Value::None, heap, interns);

    // os.curdir
    let curdir_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(".".to_owned())))?);
    module.set_attr(StaticStrings::OsCurdir, curdir_val, heap, interns);

    // os.pardir
    let pardir_val = Value::Ref(heap.allocate(HeapData::Str(Str::new("..".to_owned())))?);
    module.set_attr(StaticStrings::OsPardir, pardir_val, heap, interns);

    // os.extsep
    let extsep_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(".".to_owned())))?);
    module.set_attr(StaticStrings::OsExtsep, extsep_val, heap, interns);

    // os.devnull
    let devnull_val = Value::Ref(heap.allocate(HeapData::Str(Str::new("/dev/null".to_owned())))?);
    module.set_attr(StaticStrings::OsDevnull, devnull_val, heap, interns);

    Ok(())
}

/// Creates the `os.path` sub-module with pure string-manipulation path functions.
///
/// All functions here operate on path strings — they never access the filesystem.
pub(crate) fn create_os_path_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut path_mod = Module::new(StaticStrings::OsPathMod);

    let attrs: &[(StaticStrings, OsPathFunctions)] = &[
        (StaticStrings::Join, OsPathFunctions::Join),
        (StaticStrings::Exists, OsPathFunctions::Exists),
        (StaticStrings::OsPathDirname, OsPathFunctions::Dirname),
        (StaticStrings::OsPathBasename, OsPathFunctions::Basename),
        (StaticStrings::Split, OsPathFunctions::Split),
        (StaticStrings::OsPathSplitext, OsPathFunctions::Splitext),
        (StaticStrings::OsPathIsabs, OsPathFunctions::Isabs),
        (StaticStrings::OsPathAbspath, OsPathFunctions::Abspath),
        (StaticStrings::OsPathNormpath, OsPathFunctions::Normpath),
        (StaticStrings::OsPathCommonpath, OsPathFunctions::Commonpath),
        (StaticStrings::OsPathCommonprefix, OsPathFunctions::Commonprefix),
        (StaticStrings::OsPathRelpath, OsPathFunctions::Relpath),
    ];

    for &(name, func) in attrs {
        path_mod.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::OsPath(func)),
            heap,
            interns,
        );
    }

    let dynamic_attrs: &[(&str, OsPathFunctions)] = &[
        ("splitdrive", OsPathFunctions::Splitdrive),
        ("splitroot", OsPathFunctions::Splitroot),
        ("lexists", OsPathFunctions::Lexists),
        ("isfile", OsPathFunctions::Isfile),
        ("isdir", OsPathFunctions::Isdir),
        ("islink", OsPathFunctions::Islink),
        ("ismount", OsPathFunctions::Ismount),
        ("isjunction", OsPathFunctions::Isjunction),
        ("isdevdrive", OsPathFunctions::Isdevdrive),
        ("normcase", OsPathFunctions::Normcase),
        ("realpath", OsPathFunctions::Realpath),
        ("expanduser", OsPathFunctions::Expanduser),
        ("expandvars", OsPathFunctions::Expandvars),
        ("getsize", OsPathFunctions::Getsize),
        ("getmtime", OsPathFunctions::Getmtime),
        ("getatime", OsPathFunctions::Getatime),
        ("getctime", OsPathFunctions::Getctime),
    ];

    for &(name, func) in dynamic_attrs {
        path_mod.set_attr_str(
            name,
            Value::ModuleFunction(ModuleFunctions::OsPath(func)),
            heap,
            interns,
        )?;
    }

    path_mod.set_attr_str("supports_unicode_filenames", Value::Bool(true), heap, interns)?;
    path_mod.set_attr_str(
        "sep",
        Value::Ref(heap.allocate(HeapData::Str(Str::new("/".to_owned())))?),
        heap,
        interns,
    )?;
    path_mod.set_attr_str("altsep", Value::None, heap, interns)?;
    path_mod.set_attr_str(
        "pathsep",
        Value::Ref(heap.allocate(HeapData::Str(Str::new(":".to_owned())))?),
        heap,
        interns,
    )?;
    path_mod.set_attr_str(
        "extsep",
        Value::Ref(heap.allocate(HeapData::Str(Str::new(".".to_owned())))?),
        heap,
        interns,
    )?;
    path_mod.set_attr_str(
        "curdir",
        Value::Ref(heap.allocate(HeapData::Str(Str::new(".".to_owned())))?),
        heap,
        interns,
    )?;
    path_mod.set_attr_str(
        "pardir",
        Value::Ref(heap.allocate(HeapData::Str(Str::new("..".to_owned())))?),
        heap,
        interns,
    )?;
    path_mod.set_attr_str(
        "devnull",
        Value::Ref(heap.allocate(HeapData::Str(Str::new("/dev/null".to_owned())))?),
        heap,
        interns,
    )?;
    path_mod.set_attr_str(
        "defpath",
        Value::Ref(heap.allocate(HeapData::Str(Str::new("/bin:/usr/bin".to_owned())))?),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(path_mod))
}

// ===========================================================================
// os.path functions (pure string manipulation)
// ===========================================================================

/// Implementation of `os.path.join(a, *p)`.
///
/// Joins path components. If a component is absolute, it replaces
/// everything before it (matching CPython behavior).
fn os_path_join(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let parts = match args {
        ArgValues::One(val) => {
            let s = extract_str(&val, heap, interns, "os.path.join")?;
            val.drop_with_heap(heap);
            return Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(s)))?));
        }
        ArgValues::Two(a, b) => {
            let a_str = extract_str(&a, heap, interns, "os.path.join")?;
            let b_str = extract_str(&b, heap, interns, "os.path.join")?;
            a.drop_with_heap(heap);
            b.drop_with_heap(heap);
            vec![a_str, b_str]
        }
        ArgValues::ArgsKargs { args: vals, kwargs } => {
            if !kwargs.is_empty() {
                for v in vals {
                    v.drop_with_heap(heap);
                }
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error_no_kwargs("os.path.join"));
            }
            if vals.is_empty() {
                return Err(ExcType::type_error(
                    "join() missing 1 required positional argument: 'a'".to_owned(),
                ));
            }
            let mut parts = Vec::with_capacity(vals.len());
            for v in &vals {
                parts.push(extract_str(v, heap, interns, "os.path.join")?);
            }
            for v in vals {
                v.drop_with_heap(heap);
            }
            parts
        }
        ArgValues::Empty => {
            return Err(ExcType::type_error(
                "join() missing 1 required positional argument: 'a'".to_owned(),
            ));
        }
        ArgValues::Kwargs(kwargs) => {
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error_no_kwargs("os.path.join"));
        }
    };

    let result = join_path_parts(&parts);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(result)))?))
}

/// Implementation of `os.path.exists(p)`.
///
/// Always returns `False` in the sandbox — there is no real filesystem.
fn os_path_exists(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.exists", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.lexists(p)`.
///
/// In the sandbox this mirrors `exists()` and always returns `False`.
fn os_path_lexists(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.lexists", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.dirname(p)`.
///
/// Returns the directory component of the path string.
fn os_path_dirname(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.dirname", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.dirname")?;
    val.drop_with_heap(heap);

    let (dirname, _) = split_path_impl(&s);

    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(dirname)))?))
}

/// Implementation of `os.path.basename(p)`.
///
/// Returns the final component of the path string.
fn os_path_basename(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.basename", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.basename")?;
    val.drop_with_heap(heap);

    let base = s.rsplit_once('/').map_or(s.as_str(), |(_, name)| name);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(base.to_owned())))?))
}

/// Implementation of `os.path.split(p)`.
///
/// Splits the path into `(dirname, basename)` tuple.
fn os_path_split(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.split", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.split")?;
    val.drop_with_heap(heap);

    let (head, tail) = split_path_impl(&s);

    let head_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(head)))?);
    let tail_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(tail)))?);
    crate::types::allocate_tuple(smallvec::smallvec![head_val, tail_val], heap).map(Ok)?
}

/// Implementation of `os.path.splitext(p)`.
///
/// Splits the path into `(root, ext)` where `ext` is the file extension
/// including the leading dot. Matches CPython's behavior for edge cases.
fn os_path_splitext(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.splitext", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.splitext")?;
    val.drop_with_heap(heap);

    // Find the basename first
    let sep_idx = s.rfind('/');
    let basename_start = sep_idx.map_or(0, |i| i + 1);
    let basename = &s[basename_start..];

    // Find the last dot in basename, but skip leading dots (hidden files)
    let (root, ext) = if let Some(dot_idx) = basename.rfind('.') {
        // Check this isn't a leading dot or all dots
        if dot_idx == 0 || basename[..dot_idx].chars().all(|c| c == '.') {
            (s.clone(), String::new())
        } else {
            let abs_dot_idx = basename_start + dot_idx;
            (s[..abs_dot_idx].to_owned(), s[abs_dot_idx..].to_owned())
        }
    } else {
        (s, String::new())
    };

    let root_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(root)))?);
    let ext_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(ext)))?);
    crate::types::allocate_tuple(smallvec::smallvec![root_val, ext_val], heap).map(Ok)?
}

/// Implementation of `os.path.splitdrive(p)`.
///
/// On POSIX this always returns `('', p)`.
fn os_path_splitdrive(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.splitdrive", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.splitdrive")?;
    val.drop_with_heap(heap);

    let drive_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(String::new())))?);
    let path_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(s)))?);
    crate::types::allocate_tuple(smallvec::smallvec![drive_val, path_val], heap).map(Ok)?
}

/// Implementation of `os.path.splitroot(p)`.
///
/// On POSIX this returns `(drive, root, tail)` with empty drive.
fn os_path_splitroot(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.splitroot", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.splitroot")?;
    val.drop_with_heap(heap);

    let (root, tail) = if s.starts_with("//") && !s.starts_with("///") {
        ("//".to_owned(), s[2..].to_owned())
    } else if let Some(stripped) = s.strip_prefix('/') {
        ("/".to_owned(), stripped.to_owned())
    } else {
        (String::new(), s)
    };

    let drive_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(String::new())))?);
    let root_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(root)))?);
    let tail_val = Value::Ref(heap.allocate(HeapData::Str(Str::new(tail)))?);
    crate::types::allocate_tuple(smallvec::smallvec![drive_val, root_val, tail_val], heap).map(Ok)?
}

/// Implementation of `os.path.isabs(p)`.
///
/// Returns `True` if the path is absolute (starts with `/`).
fn os_path_isabs(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.isabs", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.isabs")?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(s.starts_with('/')))
}

/// Implementation of `os.path.isfile(p)`.
///
/// Always returns `False` in the sandbox.
fn os_path_isfile(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.isfile", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.isdir(p)`.
///
/// Always returns `False` in the sandbox.
fn os_path_isdir(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.isdir", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.islink(p)`.
///
/// Always returns `False` in the sandbox.
fn os_path_islink(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.islink", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.ismount(p)`.
///
/// Returns `True` for `'/'` and `False` otherwise.
fn os_path_ismount(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.ismount", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.ismount")?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(normpath_impl(&s) == "/"))
}

/// Implementation of `os.path.isjunction(p)`.
///
/// Always returns `False` on POSIX.
fn os_path_isjunction(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.isjunction", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.isdevdrive(p)`.
///
/// Always returns `False` on POSIX.
fn os_path_isdevdrive(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.isdevdrive", heap)?;
    val.drop_with_heap(heap);
    Ok(Value::Bool(false))
}

/// Implementation of `os.path.abspath(p)`.
///
/// Returns a normalized absolutized version of the path.
fn os_path_abspath(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.abspath", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.abspath")?;
    val.drop_with_heap(heap);

    let abs = to_absolute_normpath(&s);

    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(abs)))?))
}

/// Implementation of `os.path.realpath(p)`.
///
/// For this sandbox implementation, this mirrors `abspath`.
fn os_path_realpath(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.realpath", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.realpath")?;
    val.drop_with_heap(heap);
    let abs = to_absolute_normpath(&s);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(abs)))?))
}

/// Implementation of `os.path.normcase(s)`.
///
/// On POSIX this returns the input unchanged.
fn os_path_normcase(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.normcase", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.normcase")?;
    val.drop_with_heap(heap);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(s)))?))
}

/// Implementation of `os.path.normpath(p)`.
///
/// Normalizes the path by collapsing redundant separators, `.` and `..` components.
fn os_path_normpath(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.normpath", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.normpath")?;
    val.drop_with_heap(heap);

    let result = normpath_impl(&s);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(result)))?))
}

/// Implementation of `os.path.commonpath(paths)`.
///
/// Returns the longest common sub-path for the provided path sequence.
fn os_path_commonpath(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let paths_value = args.get_one_arg("os.path.commonpath", heap)?;
    let mut iter = OurosIter::new(paths_value, heap, interns)?;
    let mut all_paths = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        let path = extract_str(&item, heap, interns, "os.path.commonpath")?;
        item.drop_with_heap(heap);
        all_paths.push(path);
    }
    iter.drop_with_heap(heap);

    if all_paths.is_empty() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "commonpath() arg is an empty sequence").into());
    }

    let absolute = all_paths[0].starts_with('/');
    for path in &all_paths[1..] {
        if path.starts_with('/') != absolute {
            return Err(SimpleException::new_msg(ExcType::ValueError, "Can't mix absolute and relative paths").into());
        }
    }

    let mut common: Vec<&str> = path_components(&all_paths[0]);
    for path in &all_paths[1..] {
        let parts = path_components(path);
        let max = common.len().min(parts.len());
        let mut idx = 0;
        while idx < max && common[idx] == parts[idx] {
            idx += 1;
        }
        common.truncate(idx);
    }

    let result = if absolute {
        if common.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", common.join("/"))
        }
    } else if common.is_empty() {
        String::new()
    } else {
        common.join("/")
    };
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(result)))?))
}

/// Implementation of `os.path.commonprefix(list)`.
///
/// Returns the longest common string prefix (character-based).
fn os_path_commonprefix(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let list_value = args.get_one_arg("os.path.commonprefix", heap)?;
    let mut iter = OurosIter::new(list_value, heap, interns)?;
    let mut values = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        let s = extract_str(&item, heap, interns, "os.path.commonprefix")?;
        item.drop_with_heap(heap);
        values.push(s);
    }
    iter.drop_with_heap(heap);

    if values.is_empty() {
        return Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(String::new())))?));
    }

    let mut prefix = values[0].clone();
    for value in &values[1..] {
        let mut next = String::new();
        for (a, b) in prefix.chars().zip(value.chars()) {
            if a != b {
                break;
            }
            next.push(a);
        }
        prefix = next;
        if prefix.is_empty() {
            break;
        }
    }

    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(prefix)))?))
}

/// Implementation of `os.path.relpath(path, start='.')`.
///
/// Computes a relative path from `start` to `path` using sandbox POSIX semantics.
fn os_path_relpath(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (path_value, start_value) = args.get_one_two_args("os.path.relpath", heap)?;
    let path = extract_str(&path_value, heap, interns, "os.path.relpath")?;
    path_value.drop_with_heap(heap);
    let start = if let Some(start) = start_value {
        let value = extract_str(&start, heap, interns, "os.path.relpath")?;
        start.drop_with_heap(heap);
        value
    } else {
        ".".to_owned()
    };

    let path_abs = to_absolute_normpath(&path);
    let start_abs = to_absolute_normpath(&start);
    let path_parts = path_components(&path_abs);
    let start_parts = path_components(&start_abs);

    let mut same = 0usize;
    while same < path_parts.len() && same < start_parts.len() && path_parts[same] == start_parts[same] {
        same += 1;
    }

    let mut out_parts: Vec<String> = Vec::new();
    for _ in same..start_parts.len() {
        out_parts.push("..".to_owned());
    }
    for part in &path_parts[same..] {
        out_parts.push((*part).to_owned());
    }

    let result = if out_parts.is_empty() {
        ".".to_owned()
    } else {
        out_parts.join("/")
    };
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(result)))?))
}

/// Implementation of `os.path.expanduser(path)`.
fn os_path_expanduser(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.expanduser", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.expanduser")?;
    val.drop_with_heap(heap);

    let expanded = expanduser_impl(&s);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(expanded)))?))
}

/// Implementation of `os.path.expandvars(path)`.
fn os_path_expandvars(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.expandvars", heap)?;
    let s = extract_str(&val, heap, interns, "os.path.expandvars")?;
    val.drop_with_heap(heap);

    let expanded = expandvars_impl(&s);
    Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(expanded)))?))
}

/// Implementation of `os.path.getsize(path)`.
///
/// Returns an integer sentinel while preserving CPython's return type.
fn os_path_getsize(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.getsize", heap)?;
    let _ = extract_str(&val, heap, interns, "os.path.getsize")?;
    val.drop_with_heap(heap);
    Ok(Value::Int(0))
}

/// Implementation of `os.path.getmtime(path)`.
///
/// Returns a float sentinel while preserving CPython's return type.
fn os_path_getmtime(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.getmtime", heap)?;
    let _ = extract_str(&val, heap, interns, "os.path.getmtime")?;
    val.drop_with_heap(heap);
    Ok(Value::Float(0.0))
}

/// Implementation of `os.path.getatime(path)`.
///
/// Returns a float sentinel while preserving CPython's return type.
fn os_path_getatime(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.getatime", heap)?;
    let _ = extract_str(&val, heap, interns, "os.path.getatime")?;
    val.drop_with_heap(heap);
    Ok(Value::Float(0.0))
}

/// Implementation of `os.path.getctime(path)`.
///
/// Returns a float sentinel while preserving CPython's return type.
fn os_path_getctime(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("os.path.getctime", heap)?;
    let _ = extract_str(&val, heap, interns, "os.path.getctime")?;
    val.drop_with_heap(heap);
    Ok(Value::Float(0.0))
}

// ===========================================================================
// String extraction and path helpers
// ===========================================================================

/// Extracts a string from a `Value`, returning an error if not a string type.
fn extract_str(
    val: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<String> {
    match val {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "{func_name}() argument must be str, not {}",
                val.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{func_name}() argument must be str, not {}",
            val.py_type(heap)
        ))),
    }
}

/// Joins multiple path parts using POSIX semantics.
///
/// If a part is absolute (starts with `/`), it replaces everything before it.
fn join_path_parts(parts: &[String]) -> String {
    let mut iter = parts.iter();
    let mut path = match iter.next() {
        Some(part) => part.clone(),
        None => return String::new(),
    };

    for part in iter {
        if part.starts_with('/') || path.is_empty() {
            path = part.clone();
        } else if path.ends_with('/') {
            path.push_str(part);
        } else {
            path.push('/');
            path.push_str(part);
        }
    }

    path
}

/// Splits a path into `(head, tail)` using CPython `posixpath.split` semantics.
///
/// `head` has trailing separators removed unless it consists only of separators.
fn split_path_impl(path: &str) -> (String, String) {
    let idx = path.rfind('/').map_or(0, |i| i + 1);
    let mut head = path[..idx].to_owned();
    let tail = path[idx..].to_owned();

    if !head.is_empty() {
        let trimmed = head.trim_end_matches('/');
        if !trimmed.is_empty() {
            head = trimmed.to_owned();
        }
    }

    (head, tail)
}

/// Normalizes a POSIX path by collapsing `.`, `..`, and redundant separators.
///
/// This is a pure string operation matching CPython's `os.path.normpath` behavior:
/// - Collapses multiple slashes to one
/// - Resolves `.` (current dir) components
/// - Resolves `..` (parent dir) components
/// - Preserves absolute vs relative distinction
/// - Returns `'.'` for empty paths
fn normpath_impl(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }

    let is_absolute = path.starts_with('/');
    let mut components: Vec<&str> = Vec::new();

    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if is_absolute {
                // Can't go above root — just drop the component
                components.pop();
            } else if components.last().is_some_and(|last| *last != "..") {
                components.pop();
            } else {
                // Relative path, no parent to pop — keep the ..
                components.push(part);
            }
        } else {
            components.push(part);
        }
    }

    if is_absolute {
        if components.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", components.join("/"))
        }
    } else if components.is_empty() {
        ".".to_owned()
    } else {
        components.join("/")
    }
}

/// Splits a normalized POSIX path into path components without empty segments.
fn path_components(path: &str) -> Vec<&str> {
    path.split('/').filter(|component| !component.is_empty()).collect()
}

/// Converts a path to an absolute, normalized POSIX path in the sandbox.
fn to_absolute_normpath(path: &str) -> String {
    if path.starts_with('/') {
        normpath_impl(path)
    } else if path.is_empty() {
        current_working_dir_normpath()
    } else {
        let cwd = current_working_dir_normpath();
        if cwd == "/" {
            normpath_impl(&format!("/{path}"))
        } else {
            normpath_impl(&format!("{cwd}/{path}"))
        }
    }
}

/// Returns the process current working directory normalized as a POSIX path.
///
/// Falls back to `'/'` if the host path cannot be resolved.
fn current_working_dir_normpath() -> String {
    let cwd = env::current_dir()
        .ok()
        .map_or_else(|| "/".to_owned(), |p| p.to_string_lossy().into_owned());
    if cwd.is_empty() {
        "/".to_owned()
    } else {
        normpath_impl(&cwd)
    }
}

/// Expands a leading `~` marker similarly to CPython's POSIX `expanduser`.
fn expanduser_impl(path: &str) -> String {
    if !path.starts_with('~') {
        return path.to_owned();
    }

    let tail = &path[1..];
    let (user, suffix) = if let Some((u, rest)) = tail.split_once('/') {
        (u, Some(rest))
    } else {
        (tail, None)
    };

    let home = if user.is_empty() {
        env::var("HOME").ok()
    } else if user == "root" {
        Some("/var/root".to_owned())
    } else {
        None
    };

    match home {
        Some(home_dir) => match suffix {
            Some(rest) if !rest.is_empty() => format!("{home_dir}/{rest}"),
            Some(_) | None => home_dir,
        },
        None => path.to_owned(),
    }
}

/// Expands `$VAR` and `${VAR}` markers using host environment variables.
fn expandvars_impl(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = String::with_capacity(path.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        if i + 1 >= bytes.len() {
            out.push('$');
            i += 1;
            continue;
        }

        let next = bytes[i + 1];
        if next == b'$' {
            // Keep one '$' and continue scanning from the second '$',
            // so '$$HOME' becomes '$' + value(HOME).
            out.push('$');
            i += 1;
            continue;
        }

        if next == b'{' {
            let mut j = i + 2;
            while j < bytes.len() && is_env_var_char(bytes[j]) {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'}' && j > i + 2 {
                let key = &path[i + 2..j];
                if let Ok(value) = env::var(key) {
                    out.push_str(&value);
                } else {
                    out.push_str(&path[i..=j]);
                }
                i = j + 1;
                continue;
            }
            out.push('$');
            i += 1;
            continue;
        }

        if is_env_var_start(next) {
            let mut j = i + 1;
            while j < bytes.len() && is_env_var_char(bytes[j]) {
                j += 1;
            }
            let key = &path[i + 1..j];
            if let Ok(value) = env::var(key) {
                out.push_str(&value);
            } else {
                out.push('$');
                out.push_str(key);
            }
            i = j;
            continue;
        }

        out.push('$');
        i += 1;
    }

    out
}

/// Returns `true` if the byte is a valid first environment-variable character.
fn is_env_var_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Returns `true` if the byte is a valid subsequent environment-variable character.
fn is_env_var_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
