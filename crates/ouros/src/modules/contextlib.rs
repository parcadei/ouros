//! Implementation of the `contextlib` module.
//!
//! Provides context manager utilities as simplified stubs suitable for a sandboxed
//! Python interpreter:
//!
//! - `suppress(*exceptions)`: Returns a context manager that suppresses matching
//!   exception types from `with` blocks.
//! - `contextmanager(func)`: Decorator that wraps generator functions into
//!   context manager factories.
//! - `nullcontext(enter_result=None)`: Returns a context manager whose `__enter__`
//!   yields `enter_result` and whose `__exit__` is a no-op.
//! - `closing(thing)`: Returns a context manager whose `__enter__` yields `thing`.
//! - `aclosing(thing)`: Async-compatible variant of `closing`.
//! - `redirect_stdout(new_target)`: Returns a context manager whose `__enter__`
//!   yields `new_target` (I/O redirection is not performed in sandbox mode).
//! - `redirect_stderr(new_target)`: Same as `redirect_stdout` for stderr.

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::RunResult,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, PyTrait, Str, Type},
    value::Value,
};

/// contextlib module functions.
///
/// Each variant maps to a function in Python's `contextlib` module. Most are simplified
/// stubs since the sandbox does not support the full context manager protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum ContextlibFunctions {
    /// `suppress(*exceptions)` — returns a suppressing context manager.
    Suppress,
    /// `contextmanager(func)` — wraps a generator function as a context manager factory.
    Contextmanager,
    /// `asynccontextmanager(func)` — wraps async generator functions as context manager factories.
    #[strum(serialize = "asynccontextmanager")]
    Asynccontextmanager,
    /// `nullcontext(enter_result=None)` — returns a no-op context manager.
    Nullcontext,
    /// `closing(thing)` — returns a context manager yielding `thing`.
    Closing,
    /// `aclosing(thing)` — async closing helper context manager.
    Aclosing,
    /// `redirect_stdout(new_target)` — returns a redirect context manager.
    #[strum(serialize = "redirect_stdout")]
    RedirectStdout,
    /// `redirect_stderr(new_target)` — returns a redirect context manager.
    #[strum(serialize = "redirect_stderr")]
    RedirectStderr,
    /// `ExitStack()` — creates a dynamic context manager stack.
    #[strum(serialize = "ExitStack")]
    ExitStack,
    /// `AsyncExitStack()` — async-flavored exit stack.
    #[strum(serialize = "AsyncExitStack")]
    AsyncExitStack,
    /// `chdir(path)` — temporarily overrides sandbox cwd.
    Chdir,
}

/// Creates the `contextlib` module and allocates it on the heap.
///
/// The module provides:
/// - `suppress(*exceptions)`: Context manager that suppresses matching exceptions
/// - `contextmanager(func)`: Generator context manager decorator
/// - `nullcontext(enter_result=None)`: Context manager yielding `enter_result`
/// - `closing(thing)`: Context manager yielding `thing`
/// - `aclosing(thing)`: Async-compatible context manager yielding `thing`
/// - `redirect_stdout(new_target)`: Context manager yielding `new_target`
/// - `redirect_stderr(new_target)`: Context manager yielding `new_target`
///
/// # Returns
/// A `HeapId` pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Contextlib);

    // contextlib.suppress — accepts exception types, returns None
    module.set_attr(
        StaticStrings::ClSuppress,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Suppress)),
        heap,
        interns,
    );

    // contextlib.contextmanager — generator context manager decorator
    module.set_attr(
        StaticStrings::ClContextmanager,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Contextmanager)),
        heap,
        interns,
    );
    module.set_attr_str(
        "asynccontextmanager",
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Asynccontextmanager)),
        heap,
        interns,
    )?;

    // Lightweight aliases so class inheritance works for parity tests.
    module.set_attr_str(
        "AbstractContextManager",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "AbstractAsyncContextManager",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "ContextDecorator",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "AsyncContextDecorator",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;

    // contextlib.nullcontext — returns enter_result (or None)
    module.set_attr(
        StaticStrings::ClNullcontext,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Nullcontext)),
        heap,
        interns,
    );

    // contextlib.closing — returns thing directly
    module.set_attr(
        StaticStrings::ClClosing,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Closing)),
        heap,
        interns,
    );

    // contextlib.aclosing — returns thing directly
    module.set_attr(
        StaticStrings::ClAclosing,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Aclosing)),
        heap,
        interns,
    );

    // contextlib.redirect_stdout — no-op in sandbox
    module.set_attr(
        StaticStrings::ClRedirectStdout,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::RedirectStdout)),
        heap,
        interns,
    );

    // contextlib.redirect_stderr — no-op in sandbox
    module.set_attr(
        StaticStrings::ClRedirectStderr,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::RedirectStderr)),
        heap,
        interns,
    );

    // contextlib.ExitStack — stack object constructor
    module.set_attr(
        StaticStrings::ClExitStack,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::ExitStack)),
        heap,
        interns,
    );

    // contextlib.AsyncExitStack — async stack object constructor
    module.set_attr(
        StaticStrings::ClAsyncExitStack,
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::AsyncExitStack)),
        heap,
        interns,
    );
    module.set_attr_str(
        "chdir",
        Value::ModuleFunction(ModuleFunctions::Contextlib(ContextlibFunctions::Chdir)),
        heap,
        interns,
    )?;

    heap.allocate(crate::heap::HeapData::Module(module))
}

/// Dispatches a call to a contextlib module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ContextlibFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        ContextlibFunctions::Suppress => suppress(heap, args),
        ContextlibFunctions::Contextmanager => contextmanager(heap, args),
        ContextlibFunctions::Asynccontextmanager => asynccontextmanager(heap, args),
        ContextlibFunctions::Nullcontext => nullcontext(heap, args),
        ContextlibFunctions::Closing => closing(heap, args),
        ContextlibFunctions::Aclosing => aclosing(heap, args),
        ContextlibFunctions::RedirectStdout => redirect_stdout(heap, args),
        ContextlibFunctions::RedirectStderr => redirect_stderr(heap, args),
        ContextlibFunctions::ExitStack => exit_stack(heap, args),
        ContextlibFunctions::AsyncExitStack => async_exit_stack(heap, args),
        ContextlibFunctions::Chdir => chdir(heap, interns, args),
    }
}

/// Implementation of `contextlib.suppress(*exceptions)`.
///
/// Accepts any number of exception types and returns a context manager that
/// suppresses matching exceptions during `__exit__`.
fn suppress(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(crate::exception_private::ExcType::type_error_no_kwargs(
            "contextlib.suppress",
        ));
    }
    let suppress_types: Vec<Value> = positional.collect();
    let object = crate::types::StdlibObject::new_suppress_context_manager(suppress_types);
    let id = heap.allocate(crate::heap::HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `contextlib.contextmanager(func)`.
///
/// Wraps a generator function in a callable factory that produces
/// generator-backed context manager objects.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn contextmanager(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("contextlib.contextmanager", heap)?;
    let object = crate::types::StdlibObject::new_generator_context_manager_factory(func, false);
    let id = heap.allocate(crate::heap::HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `contextlib.asynccontextmanager(func)`.
///
/// In Ouros, async generator functions currently follow the same runtime path
/// as generator-backed context managers, so this reuses `contextmanager`.
fn asynccontextmanager(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("contextlib.asynccontextmanager", heap)?;
    let object = crate::types::StdlibObject::new_generator_context_manager_factory(func, true);
    let id = heap.allocate(crate::heap::HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `contextlib.nullcontext(enter_result=None)`.
///
/// Returns a context manager whose `__enter__` yields `enter_result` and whose
/// `__exit__` is a no-op.
///
/// # Errors
/// Returns `TypeError` if more than one argument is provided.
fn nullcontext(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    match args {
        ArgValues::Empty => allocate_context_manager("contextlib.nullcontext", Value::None, heap),
        ArgValues::One(val) => allocate_context_manager("contextlib.nullcontext", val, heap),
        other => {
            other.drop_with_heap(heap);
            Err(crate::exception_private::ExcType::type_error(
                "nullcontext() takes 0 or 1 positional arguments".to_string(),
            ))
        }
    }
}

/// Implementation of `contextlib.closing(thing)`.
///
/// Returns a context manager yielding `thing`. Automatic `.close()` invocation
/// is not performed in sandbox mode.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn closing(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let thing = args.get_one_arg("contextlib.closing", heap)?;
    allocate_context_manager("contextlib.closing", thing, heap)
}

/// Implementation of `contextlib.aclosing(thing)`.
///
/// Returns an async-compatible context manager yielding `thing`.
fn aclosing(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let thing = args.get_one_arg("contextlib.aclosing", heap)?;
    allocate_context_manager("contextlib.aclosing", thing, heap)
}

/// Implementation of `contextlib.redirect_stdout(new_target)`.
///
/// Returns a context manager yielding `new_target`. There is no real stdout
/// redirection in sandbox mode.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn redirect_stdout(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let target = args.get_one_arg("contextlib.redirect_stdout", heap)?;
    allocate_context_manager("contextlib.redirect_stdout", target, heap)
}

/// Implementation of `contextlib.redirect_stderr(new_target)`.
///
/// Returns a context manager yielding `new_target`. There is no real stderr
/// redirection in sandbox mode.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn redirect_stderr(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let target = args.get_one_arg("contextlib.redirect_stderr", heap)?;
    allocate_context_manager("contextlib.redirect_stderr", target, heap)
}

/// Implementation of `contextlib.ExitStack()`.
///
/// Creates an object supporting `push`, `enter_context`, `callback`, and `close`.
fn exit_stack(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("contextlib.ExitStack", heap)?;
    let object = crate::types::StdlibObject::new_exit_stack(false);
    let id = heap.allocate(crate::heap::HeapData::StdlibObject(object))?;
    let crate::heap::HeapData::StdlibObject(object) = heap.get_mut(id) else {
        unreachable!("allocated ExitStack must be a StdlibObject");
    };
    object.set_exit_stack_self_id(id);
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `contextlib.AsyncExitStack()`.
///
/// Creates an object supporting `push`, `enter_context`, `callback`, and `aclose`.
fn async_exit_stack(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("contextlib.AsyncExitStack", heap)?;
    let object = crate::types::StdlibObject::new_exit_stack(true);
    let id = heap.allocate(crate::heap::HeapData::StdlibObject(object))?;
    let crate::heap::HeapData::StdlibObject(object) = heap.get_mut(id) else {
        unreachable!("allocated AsyncExitStack must be a StdlibObject");
    };
    object.set_exit_stack_self_id(id);
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `contextlib.chdir(path)`.
///
/// Creates a context manager that updates the sandbox virtual cwd while active.
fn chdir(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let path = args.get_one_arg("contextlib.chdir", heap)?;
    let path_text = path.py_str(heap, interns).into_owned();
    path.drop_with_heap(heap);

    let canonical = std::fs::canonicalize(&path_text)
        .ok()
        .and_then(|path_buf| path_buf.to_str().map(ToOwned::to_owned))
        .unwrap_or(path_text);
    let path_id = heap.allocate(HeapData::Str(Str::new(canonical)))?;
    allocate_context_manager("contextlib.chdir", Value::Ref(path_id), heap)
}

/// Allocates a simple context manager wrapper and returns it.
fn allocate_context_manager(
    name: &str,
    enter_value: Value,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<AttrCallResult> {
    let object = crate::types::StdlibObject::new_context_manager(name, enter_value);
    let id = heap.allocate(crate::heap::HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}
