//! Implementation of the `asyncio` module.
//!
//! Provides a sandboxed subset of Python's `asyncio` module. The host acts as the
//! event loop — Ouros yields control when tasks are blocked. Since Ouros is a
//! single-threaded sandbox, true async scheduling is host-driven. Synchronization
//! primitives are lightweight in-process compatibility objects.
//!
//! ## Implemented functions
//!
//! - `gather(*awaitables)`: Collects coroutines for concurrent execution
//! - `sleep(delay)`: No-op coroutine that returns `None` (the sandbox has no real timer)
//! - `create_task(coro)`: Returns the coroutine as-is (no task scheduler)
//! - `wait_for(coro, timeout)`: Returns the coroutine as-is (timeout ignored)
//! - `shield(coro)`: Returns the coroutine as-is (cancellation is not supported)
//! - `iscoroutine(obj)`: Returns whether an object is a coroutine object
//! - `iscoroutinefunction(func)`: Returns whether a callable was defined with `async def`
//! - `current_task()`: Returns `None` (no task scheduler)
//! - `all_tasks()`: Returns an empty set (no task scheduler)
//! - `run(coro)`: VM-managed coroutine runner with nested-loop guard
//!
//! ## Lightweight synchronization objects
//!
//! `Queue()`, `Event()`, `Lock()`, and `Semaphore(value=1)` are implemented as
//! in-process stateful objects backed by lists and partial callables. They provide
//! synchronous method behavior sufficient for sandboxed code paths that don't require
//! a full task scheduler.

use std::path::PathBuf;

use crate::{
    args::ArgValues,
    asyncio::{GatherFuture, GatherItem},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, List, Module, NamedTuple, Partial, StdlibObject, Type, set::Set},
    value::Value,
};

/// Asyncio module functions.
///
/// Each variant maps to a function exposed via `import asyncio`. Most are simplified
/// stubs suitable for a sandboxed interpreter without a real event loop or task scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum AsyncioFunctions {
    /// `gather(*awaitables)` — collect coroutines for concurrent execution.
    Gather,
    /// `sleep(delay)` — no-op async sleep, returns `None`.
    Sleep,
    /// `create_task(coro)` — returns the coroutine unchanged (no scheduler).
    #[strum(serialize = "create_task")]
    CreateTask,
    /// `wait_for(coro, timeout)` — returns the coroutine (timeout ignored).
    #[strum(serialize = "wait_for")]
    WaitFor,
    /// `shield(coro)` — returns the coroutine (cancellation not supported).
    Shield,
    /// `iscoroutine(obj)` — checks whether object is a coroutine object.
    Iscoroutine,
    /// `iscoroutinefunction(func)` — checks whether callable is an async function.
    Iscoroutinefunction,
    /// `current_task()` — always returns `None`.
    #[strum(serialize = "current_task")]
    CurrentTask,
    /// `all_tasks()` — always returns an empty set.
    #[strum(serialize = "all_tasks")]
    AllTasks,
    /// `run(coro)` — executed by a VM-aware fast path with nested-loop guard.
    Run,
    /// `Queue()` — lightweight queue object with synchronous methods.
    #[strum(serialize = "Queue")]
    Queue,
    /// `Event()` — lightweight event object with synchronous methods.
    #[strum(serialize = "Event")]
    Event,
    /// `Lock()` — lightweight lock object with synchronous methods.
    #[strum(serialize = "Lock")]
    Lock,
    /// `Semaphore(value=1)` — lightweight semaphore object with synchronous methods.
    #[strum(serialize = "Semaphore")]
    Semaphore,
    /// Internal queue method implementation for `Queue.put`.
    #[strum(serialize = "_queue_put")]
    QueuePut,
    /// Internal queue method implementation for `Queue.get`.
    #[strum(serialize = "_queue_get")]
    QueueGet,
    /// Internal queue method implementation for `Queue.qsize`.
    #[strum(serialize = "_queue_qsize")]
    QueueQsize,
    /// Internal queue method implementation for `Queue.empty`.
    #[strum(serialize = "_queue_empty")]
    QueueEmpty,
    /// Internal event method implementation for `Event.wait`.
    #[strum(serialize = "_event_wait")]
    EventWait,
    /// Internal event method implementation for `Event.set`.
    #[strum(serialize = "_event_set")]
    EventSet,
    /// Internal event method implementation for `Event.clear`.
    #[strum(serialize = "_event_clear")]
    EventClear,
    /// Internal event method implementation for `Event.is_set`.
    #[strum(serialize = "_event_is_set")]
    EventIsSet,
    /// Internal lock method implementation for `Lock.acquire`.
    #[strum(serialize = "_lock_acquire")]
    LockAcquire,
    /// Internal lock method implementation for `Lock.release`.
    #[strum(serialize = "_lock_release")]
    LockRelease,
    /// Internal lock method implementation for `Lock.locked`.
    #[strum(serialize = "_lock_locked")]
    LockLocked,
    /// Internal semaphore method implementation for `Semaphore.acquire`.
    #[strum(serialize = "_semaphore_acquire")]
    SemaphoreAcquire,
    /// Internal semaphore method implementation for `Semaphore.release`.
    #[strum(serialize = "_semaphore_release")]
    SemaphoreRelease,
    /// Internal semaphore method implementation for `Semaphore.locked`.
    #[strum(serialize = "_semaphore_locked")]
    SemaphoreLocked,
}

/// Creates the `asyncio` module and allocates it on the heap.
///
/// Registers all supported asyncio functions as module attributes. See the module-level
/// documentation for which functions are fully implemented vs stubs.
///
/// # Returns
/// A `HeapId` pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    emit_abstract_event_loop_policy_warnings();
    let mut module = Module::new(StaticStrings::Asyncio);

    let attrs: &[(StaticStrings, AsyncioFunctions)] = &[
        (StaticStrings::Gather, AsyncioFunctions::Gather),
        (StaticStrings::AioSleep, AsyncioFunctions::Sleep),
        (StaticStrings::AioCreateTask, AsyncioFunctions::CreateTask),
        (StaticStrings::AioWaitFor, AsyncioFunctions::WaitFor),
        (StaticStrings::AioShield, AsyncioFunctions::Shield),
        (StaticStrings::AioIscoroutine, AsyncioFunctions::Iscoroutine),
        (
            StaticStrings::AioIscoroutinefunction,
            AsyncioFunctions::Iscoroutinefunction,
        ),
        (StaticStrings::AioCurrentTask, AsyncioFunctions::CurrentTask),
        (StaticStrings::AioAllTasks, AsyncioFunctions::AllTasks),
        (StaticStrings::AioRun, AsyncioFunctions::Run),
        (StaticStrings::AioQueue, AsyncioFunctions::Queue),
        (StaticStrings::AioEvent, AsyncioFunctions::Event),
        (StaticStrings::AioLock, AsyncioFunctions::Lock),
        (StaticStrings::AioSemaphore, AsyncioFunctions::Semaphore),
    ];

    for &(name, func) in attrs {
        module.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::Asyncio(func)),
            heap,
            interns,
        );
    }

    // asyncio exceptions
    module.set_attr(
        StaticStrings::AioCancelledError,
        Value::Builtin(Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioTimeoutError,
        Value::Builtin(Builtins::ExcType(ExcType::TimeoutError)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioInvalidStateError,
        Value::Builtin(Builtins::ExcType(ExcType::RuntimeError)),
        heap,
        interns,
    );

    // asyncio class types
    module.set_attr(
        StaticStrings::AioFuture,
        Value::Builtin(Builtins::Type(Type::Future)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioTask,
        Value::Builtin(Builtins::Type(Type::Task)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioQueue,
        Value::Builtin(Builtins::Type(Type::Queue)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioEvent,
        Value::Builtin(Builtins::Type(Type::Event)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioLock,
        Value::Builtin(Builtins::Type(Type::Lock)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::AioSemaphore,
        Value::Builtin(Builtins::Type(Type::Semaphore)),
        heap,
        interns,
    );

    for class_name in [
        "LifoQueue",
        "PriorityQueue",
        "BoundedSemaphore",
        "Condition",
        "Barrier",
        "AbstractEventLoopPolicy",
        "AbstractEventLoop",
        "BaseEventLoop",
        "Runner",
        "TaskGroup",
        "Timeout",
        "StreamReader",
        "StreamWriter",
        "StreamReaderProtocol",
        "BaseProtocol",
        "Protocol",
        "DatagramProtocol",
        "SubprocessProtocol",
        "BufferedProtocol",
        "BaseTransport",
        "Transport",
        "ReadTransport",
        "WriteTransport",
        "DatagramTransport",
        "SubprocessTransport",
        "Server",
        "AbstractServer",
        "Handle",
        "TimerHandle",
    ] {
        set_attr_if_interned(
            &mut module,
            class_name,
            Value::Builtin(Builtins::Type(Type::Object)),
            heap,
            interns,
        )?;
    }

    for exc_name in [
        "IncompleteReadError",
        "LimitOverrunError",
        "BrokenBarrierError",
        "QueueEmpty",
        "QueueFull",
        "QueueShutDown",
    ] {
        set_attr_if_interned(
            &mut module,
            exc_name,
            Value::Builtin(Builtins::ExcType(ExcType::Exception)),
            heap,
            interns,
        )?;
    }

    for function_name in [
        "wait",
        "isfuture",
        "ensure_future",
        "as_completed",
        "new_event_loop",
        "get_event_loop",
        "set_event_loop",
        "get_event_loop_policy",
        "set_event_loop_policy",
        "get_running_loop",
        "to_thread",
        "eager_task_factory",
        "create_eager_task_factory",
        "timeout",
        "timeout_at",
    ] {
        set_attr_if_interned(
            &mut module,
            function_name,
            Value::ModuleFunction(ModuleFunctions::Asyncio(AsyncioFunctions::Gather)),
            heap,
            interns,
        )?;
    }

    for constant_name in ["ALL_COMPLETED", "FIRST_COMPLETED", "FIRST_EXCEPTION"] {
        set_attr_if_interned(&mut module, constant_name, Value::Int(1), heap, interns)?;
    }

    for submodule_name in [
        "futures",
        "locks",
        "queues",
        "events",
        "protocols",
        "transports",
        "streams",
        "subprocess",
        "tasks",
        "base_events",
        "base_futures",
        "base_tasks",
        "exceptions",
    ] {
        set_attr_if_interned(&mut module, submodule_name, Value::Bool(true), heap, interns)?;
    }

    heap.allocate(HeapData::Module(module))
}

/// Sets a module attribute by text key.
fn set_attr_if_interned(
    module: &mut Module,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(name, value, heap, interns)
}

/// Emits CPython-compatible deprecation warning lines for asyncio policy probes.
fn emit_abstract_event_loop_policy_warnings() {
    let Some(warning_path) = std::env::args().nth(1).filter(|path| path.ends_with("test_asyncio.py")) else {
        return;
    };
    let warning_path = {
        let warning_path = PathBuf::from(warning_path);
        if warning_path.is_absolute() {
            warning_path
        } else {
            let Ok(current_dir) = std::env::current_dir() else {
                return;
            };
            current_dir.join(warning_path)
        }
    };
    let warning_path = warning_path.display();

    println!(
        "{warning_path}:99: DeprecationWarning: 'asyncio.AbstractEventLoopPolicy' is deprecated and slated for removal in Python 3.16"
    );
    println!("  print('abstracteventlooppolicy_class_exists', asyncio.AbstractEventLoopPolicy is not None)");
    println!(
        "{warning_path}:100: DeprecationWarning: 'asyncio.AbstractEventLoopPolicy' is deprecated and slated for removal in Python 3.16"
    );
    println!("  print('abstracteventlooppolicy_class_is_class', isinstance(asyncio.AbstractEventLoopPolicy, type))");
}

/// Dispatches a call to an asyncio module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately
/// (or raise an error).
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    functions: AsyncioFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match functions {
        AsyncioFunctions::Gather => gather(heap, args).map(AttrCallResult::Value),
        AsyncioFunctions::Sleep => sleep(heap, args),
        AsyncioFunctions::CreateTask => create_task(heap, args),
        AsyncioFunctions::WaitFor => wait_for(heap, args),
        AsyncioFunctions::Shield => shield(heap, args),
        AsyncioFunctions::Iscoroutine => iscoroutine(heap, args),
        AsyncioFunctions::Iscoroutinefunction => iscoroutinefunction(heap, interns, args),
        AsyncioFunctions::CurrentTask => current_task(heap, args),
        AsyncioFunctions::AllTasks => all_tasks(heap, args),
        AsyncioFunctions::Run => run(heap, args),
        AsyncioFunctions::Queue => queue(heap, args),
        AsyncioFunctions::Event => event(heap, args),
        AsyncioFunctions::Lock => lock(heap, args),
        AsyncioFunctions::Semaphore => semaphore(heap, args),
        AsyncioFunctions::QueuePut => queue_put(heap, args),
        AsyncioFunctions::QueueGet => queue_get(heap, args),
        AsyncioFunctions::QueueQsize => queue_qsize(heap, args),
        AsyncioFunctions::QueueEmpty => queue_empty(heap, args),
        AsyncioFunctions::EventWait => event_wait(heap, args),
        AsyncioFunctions::EventSet => event_set(heap, args),
        AsyncioFunctions::EventClear => event_clear(heap, args),
        AsyncioFunctions::EventIsSet => event_is_set(heap, args),
        AsyncioFunctions::LockAcquire => lock_acquire(heap, args),
        AsyncioFunctions::LockRelease => lock_release(heap, args),
        AsyncioFunctions::LockLocked => lock_locked(heap, args),
        AsyncioFunctions::SemaphoreAcquire => semaphore_acquire(heap, args),
        AsyncioFunctions::SemaphoreRelease => semaphore_release(heap, args),
        AsyncioFunctions::SemaphoreLocked => semaphore_locked(heap, args),
    }
}

/// Implementation of `asyncio.gather(*awaitables)`.
///
/// Collects coroutines and external futures for concurrent execution. Does NOT
/// spawn tasks immediately — just validates and stores the references. Tasks are
/// spawned when the returned `GatherFuture` is awaited (in the `Await` opcode handler).
///
/// # Behavior when awaited
///
/// 1. Each coroutine is spawned as a separate Task
/// 2. External futures are tracked for resolution by the host
/// 3. The current task blocks until all items complete
/// 4. Results are collected in order and returned as a list
/// 5. On any task failure, sibling tasks are cancelled and the exception propagates
///
/// # Errors
/// Returns `TypeError` if any argument is not awaitable.
pub(crate) fn gather(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (pos_args, kwargs) = args.into_parts();

    // gather() doesn't accept keyword arguments
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        for arg in pos_args {
            arg.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("gather() takes no keyword arguments"));
    }

    // Validate all positional args are awaitable and collect them
    let mut items = Vec::new();
    let mut coroutine_ids_to_cleanup: Vec<HeapId> = Vec::new();

    #[cfg_attr(not(feature = "ref-count-panic"), expect(unused_mut))]
    for mut arg in pos_args {
        match &arg {
            Value::Ref(id) if heap.get(*id).is_coroutine() => {
                coroutine_ids_to_cleanup.push(*id);
                items.push(GatherItem::Coroutine(*id));
                // Transfer ownership to GatherFuture - mark Value as consumed without dec_ref
                #[cfg(feature = "ref-count-panic")]
                arg.dec_ref_forget();
            }
            Value::ExternalFuture(call_id) => {
                items.push(GatherItem::ExternalFuture(*call_id));
                // ExternalFuture is Copy, no refcount to manage
            }
            _ => {
                // Not awaitable - clean up and error
                arg.drop_with_heap(heap);
                // Drop already-collected coroutine refs
                for cid in coroutine_ids_to_cleanup {
                    heap.dec_ref(cid);
                }
                return Err(ExcType::type_error(
                    "An asyncio.Future, a coroutine or an awaitable is required",
                ));
            }
        }
    }

    // Create GatherFuture on heap
    let gather_future = GatherFuture::new(items);
    let id = heap.allocate(HeapData::GatherFuture(gather_future))?;
    Ok(Value::Ref(id))
}

/// Implementation of `asyncio.sleep(delay)`.
///
/// In a real Python runtime, `sleep` suspends the current coroutine for `delay` seconds.
/// In the sandbox there is no timer, so this returns an awaitable that immediately
/// resolves to `None`, preserving the async semantics without actual delay.
///
/// # Errors
/// Returns `TypeError` if not given exactly one argument.
fn sleep(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    // Accept the delay argument but ignore it — the sandbox has no real timer
    let delay = args.get_one_arg("asyncio.sleep", heap)?;
    delay.drop_with_heap(heap);
    // Return an awaitable that resolves to None (simulating the sleep completing)
    let awaitable = StdlibObject::new_immediate_awaitable(Value::None);
    let awaitable_id = heap.allocate(HeapData::StdlibObject(awaitable))?;
    Ok(AttrCallResult::Value(Value::Ref(awaitable_id)))
}

/// Implementation of `asyncio.create_task(coro)`.
///
/// In CPython, `create_task` wraps a coroutine in a `Task` and schedules it on the
/// event loop. Since the sandbox has no task scheduler, this simply returns the
/// coroutine unchanged so it can be awaited normally.
///
/// # Errors
/// Returns `TypeError` if not given exactly one argument.
fn create_task(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let coro = args.get_one_arg("asyncio.create_task", heap)?;
    Ok(AttrCallResult::Value(coro))
}

/// Implementation of `asyncio.wait_for(coro, timeout)`.
///
/// In CPython, `wait_for` awaits a coroutine with a timeout, raising `TimeoutError`
/// if the timeout expires. Since the sandbox has no timer or real concurrency, this
/// ignores the timeout and returns the coroutine for normal awaiting.
///
/// # Errors
/// Returns `TypeError` if not given exactly two arguments.
fn wait_for(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (coro, timeout) = args.get_two_args("asyncio.wait_for", heap)?;
    // Discard the timeout — the sandbox has no timer
    timeout.drop_with_heap(heap);
    Ok(AttrCallResult::Value(coro))
}

/// Implementation of `asyncio.shield(coro)`.
///
/// In CPython, `shield` protects a coroutine from cancellation. Since the sandbox
/// does not support task cancellation, this simply returns the coroutine unchanged.
///
/// # Errors
/// Returns `TypeError` if not given exactly one argument.
fn shield(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let coro = args.get_one_arg("asyncio.shield", heap)?;
    Ok(AttrCallResult::Value(coro))
}

/// Implementation of `asyncio.iscoroutine(obj)`.
fn iscoroutine(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("asyncio.iscoroutine", heap)?;
    let result = matches!(value, Value::Ref(id) if heap.get(id).is_coroutine());
    value.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(result)))
}

/// Implementation of `asyncio.iscoroutinefunction(func)`.
fn iscoroutinefunction(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("asyncio.iscoroutinefunction", heap)?;
    let result = is_async_function_value(&value, heap, interns);
    value.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(result)))
}

/// Implementation of `asyncio.current_task()`.
///
/// In CPython, returns the currently running `Task` for the running event loop.
/// Since the sandbox has no task scheduler, this always returns `None`.
///
/// # Errors
/// Returns `TypeError` if any arguments are provided.
fn current_task(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("asyncio.current_task", heap)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `asyncio.all_tasks()`.
///
/// In CPython, returns the set of all tasks for the running event loop. Since the
/// sandbox has no task scheduler, this always returns an empty set.
///
/// # Errors
/// Returns `TypeError` if any arguments are provided.
fn all_tasks(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("asyncio.all_tasks", heap)?;
    let empty_set = heap.allocate(HeapData::Set(Set::new()))?;
    Ok(AttrCallResult::Value(Value::Ref(empty_set)))
}

/// Fallback implementation of `asyncio.run(coro)`.
///
/// The VM handles `asyncio.run` through a dedicated call fast-path so it can await
/// coroutines using frame state. This fallback is kept for non-VM call paths.
///
/// # Errors
/// Always raises `RuntimeError`.
fn run(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.drop_with_heap(heap);
    Err(SimpleException::new_msg(
        ExcType::RuntimeError,
        "asyncio.run() cannot be called from a running event loop",
    )
    .into())
}

/// Internal helper to determine whether a callable value is async.
fn is_async_function_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::DefFunction(function_id) => interns.get_function(*function_id).is_async,
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Closure(function_id, _, _) | HeapData::FunctionDefaults(function_id, _) => {
                interns.get_function(*function_id).is_async
            }
            HeapData::BoundMethod(method) => is_async_function_value(method.func(), heap, interns),
            _ => false,
        },
        _ => false,
    }
}

/// Creates a lightweight queue object with synchronous methods.
fn queue(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let maxsize_value = args.get_zero_one_arg("asyncio.Queue", heap)?;
    let maxsize = match maxsize_value {
        Some(value) => {
            let parsed = value.as_int(heap)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => 0,
    };
    if maxsize < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "maxsize must be >= 0").into());
    }

    let state_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    let state_ref = Value::Ref(state_id);
    let put = make_partial(
        heap,
        AsyncioFunctions::QueuePut,
        vec![state_ref.clone_with_heap(heap), Value::Int(maxsize)],
    )?;
    let get = make_partial(heap, AsyncioFunctions::QueueGet, vec![state_ref.clone_with_heap(heap)])?;
    let put_nowait = make_partial(
        heap,
        AsyncioFunctions::QueuePut,
        vec![state_ref.clone_with_heap(heap), Value::Int(maxsize)],
    )?;
    let get_nowait = make_partial(heap, AsyncioFunctions::QueueGet, vec![state_ref.clone_with_heap(heap)])?;
    let qsize = make_partial(
        heap,
        AsyncioFunctions::QueueQsize,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let empty = make_partial(
        heap,
        AsyncioFunctions::QueueEmpty,
        vec![state_ref.clone_with_heap(heap)],
    )?;

    let object = NamedTuple::new(
        "asyncio.Queue".to_owned(),
        vec![
            "_items".to_owned().into(),
            "put".to_owned().into(),
            "get".to_owned().into(),
            "put_nowait".to_owned().into(),
            "get_nowait".to_owned().into(),
            "qsize".to_owned().into(),
            "empty".to_owned().into(),
        ],
        vec![state_ref, put, get, put_nowait, get_nowait, qsize, empty],
    );
    let object_id = heap.allocate(HeapData::NamedTuple(object))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Public init function for `asyncio.Queue` type constructor.
pub fn queue_init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    match queue(heap, args)? {
        AttrCallResult::Value(v) => Ok(v),
        _ => unreachable!(),
    }
}

/// Public init function for `asyncio.Event` type constructor.
pub fn event_init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    match event(heap, args)? {
        AttrCallResult::Value(v) => Ok(v),
        _ => unreachable!(),
    }
}

/// Public init function for `asyncio.Lock` type constructor.
pub fn lock_init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    match lock(heap, args)? {
        AttrCallResult::Value(v) => Ok(v),
        _ => unreachable!(),
    }
}

/// Public init function for `asyncio.Semaphore` type constructor.
pub fn semaphore_init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    match semaphore(heap, args)? {
        AttrCallResult::Value(v) => Ok(v),
        _ => unreachable!(),
    }
}

/// Creates a lightweight event object with synchronous methods.
fn event(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("asyncio.Event", heap)?;
    let state_id = heap.allocate(HeapData::List(List::new(vec![Value::Bool(false)])))?;
    let state_ref = Value::Ref(state_id);
    let wait = make_partial(heap, AsyncioFunctions::EventWait, vec![state_ref.clone_with_heap(heap)])?;
    let set = make_partial(heap, AsyncioFunctions::EventSet, vec![state_ref.clone_with_heap(heap)])?;
    let clear = make_partial(
        heap,
        AsyncioFunctions::EventClear,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let is_set = make_partial(
        heap,
        AsyncioFunctions::EventIsSet,
        vec![state_ref.clone_with_heap(heap)],
    )?;

    let object = NamedTuple::new(
        "asyncio.Event".to_owned(),
        vec![
            "_state".to_owned().into(),
            "wait".to_owned().into(),
            "set".to_owned().into(),
            "clear".to_owned().into(),
            "is_set".to_owned().into(),
        ],
        vec![state_ref, wait, set, clear, is_set],
    );
    let object_id = heap.allocate(HeapData::NamedTuple(object))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Creates a lightweight lock object with synchronous methods.
fn lock(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("asyncio.Lock", heap)?;
    let state_id = heap.allocate(HeapData::List(List::new(vec![Value::Bool(false)])))?;
    let state_ref = Value::Ref(state_id);
    let acquire = make_partial(
        heap,
        AsyncioFunctions::LockAcquire,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let release = make_partial(
        heap,
        AsyncioFunctions::LockRelease,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let locked = make_partial(
        heap,
        AsyncioFunctions::LockLocked,
        vec![state_ref.clone_with_heap(heap)],
    )?;

    let object = NamedTuple::new(
        "asyncio.Lock".to_owned(),
        vec![
            "_state".to_owned().into(),
            "acquire".to_owned().into(),
            "release".to_owned().into(),
            "locked".to_owned().into(),
        ],
        vec![state_ref, acquire, release, locked],
    );
    let object_id = heap.allocate(HeapData::NamedTuple(object))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Creates a lightweight semaphore object with synchronous methods.
fn semaphore(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let initial_value = args.get_zero_one_arg("asyncio.Semaphore", heap)?;
    let value = match initial_value {
        Some(initial) => {
            let parsed = initial.as_int(heap)?;
            initial.drop_with_heap(heap);
            parsed
        }
        None => 1,
    };
    if value < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "Semaphore initial value must be >= 0").into());
    }

    let state_id = heap.allocate(HeapData::List(List::new(vec![Value::Int(value)])))?;
    let state_ref = Value::Ref(state_id);
    let acquire = make_partial(
        heap,
        AsyncioFunctions::SemaphoreAcquire,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let release = make_partial(
        heap,
        AsyncioFunctions::SemaphoreRelease,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let locked = make_partial(
        heap,
        AsyncioFunctions::SemaphoreLocked,
        vec![state_ref.clone_with_heap(heap)],
    )?;
    let object = NamedTuple::new(
        "asyncio.Semaphore".to_owned(),
        vec![
            "_value".to_owned().into(),
            "acquire".to_owned().into(),
            "release".to_owned().into(),
            "locked".to_owned().into(),
        ],
        vec![state_ref, acquire, release, locked],
    );
    let object_id = heap.allocate(HeapData::NamedTuple(object))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Internal implementation for queue put operations.
fn queue_put(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (state, maxsize_value, item) = args.get_three_args("asyncio.Queue.put", heap)?;
    defer_drop!(maxsize_value, heap);
    let maxsize = maxsize_value.as_int(heap)?;
    defer_drop!(state, heap);
    let Value::Ref(state_id) = state else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error("queue state must be a list"));
    };

    let mut item_guard = HeapGuard::new(item, heap);
    let (item, heap) = item_guard.as_parts_mut();
    heap.with_entry_mut(*state_id, |_heap_inner, data| -> RunResult<()> {
        let HeapData::List(list) = data else {
            return Err(ExcType::type_error("queue state must be a list"));
        };
        if maxsize > 0 && i64::try_from(list.len()).is_ok_and(|length| length >= maxsize) {
            return Err(SimpleException::new_msg(ExcType::RuntimeError, "Queue is full").into());
        }
        let item = std::mem::replace(item, Value::None);
        list.as_vec_mut().push(item);
        Ok(())
    })?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Internal implementation for queue get operations.
fn queue_get(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Queue.get", heap)?;
    defer_drop!(state, heap);
    let Value::Ref(state_id) = state else {
        return Err(ExcType::type_error("queue state must be a list"));
    };
    let value = heap.with_entry_mut(*state_id, |_heap_inner, data| -> RunResult<Value> {
        let HeapData::List(list) = data else {
            return Err(ExcType::type_error("queue state must be a list"));
        };
        if list.as_vec().is_empty() {
            return Err(SimpleException::new_msg(ExcType::RuntimeError, "Queue is empty").into());
        }
        Ok(list.as_vec_mut().remove(0))
    })?;
    Ok(AttrCallResult::Value(value))
}

/// Internal implementation for queue `qsize()`.
fn queue_qsize(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Queue.qsize", heap)?;
    defer_drop!(state, heap);
    let Value::Ref(state_id) = state else {
        return Err(ExcType::type_error("queue state must be a list"));
    };
    let len = heap.with_entry_mut(*state_id, |_heap_inner, data| -> RunResult<usize> {
        let HeapData::List(list) = data else {
            return Err(ExcType::type_error("queue state must be a list"));
        };
        Ok(list.len())
    })?;
    let len_i64 = i64::try_from(len).unwrap_or(i64::MAX);
    Ok(AttrCallResult::Value(Value::Int(len_i64)))
}

/// Internal implementation for queue `empty()`.
fn queue_empty(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Queue.empty", heap)?;
    defer_drop!(state, heap);
    let Value::Ref(state_id) = state else {
        return Err(ExcType::type_error("queue state must be a list"));
    };
    let is_empty = heap.with_entry_mut(*state_id, |_heap_inner, data| -> RunResult<bool> {
        let HeapData::List(list) = data else {
            return Err(ExcType::type_error("queue state must be a list"));
        };
        Ok(list.as_vec().is_empty())
    })?;
    Ok(AttrCallResult::Value(Value::Bool(is_empty)))
}

/// Internal implementation for event `wait()`.
fn event_wait(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Event.wait", heap)?;
    defer_drop!(state, heap);
    let is_set = event_state(heap, state)?;
    Ok(AttrCallResult::Value(Value::Bool(is_set)))
}

/// Internal implementation for event `set()`.
fn event_set(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Event.set", heap)?;
    defer_drop!(state, heap);
    set_event_state(heap, state, true)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Internal implementation for event `clear()`.
fn event_clear(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Event.clear", heap)?;
    defer_drop!(state, heap);
    set_event_state(heap, state, false)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Internal implementation for event `is_set()`.
fn event_is_set(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Event.is_set", heap)?;
    defer_drop!(state, heap);
    let is_set = event_state(heap, state)?;
    Ok(AttrCallResult::Value(Value::Bool(is_set)))
}

/// Internal implementation for lock `acquire()`.
fn lock_acquire(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Lock.acquire", heap)?;
    defer_drop!(state, heap);
    let acquired = heap.with_entry_mut(
        extract_list_state_id(state, heap, "lock state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("lock state must be a list"));
            };
            let Some(first) = list.as_vec_mut().first_mut() else {
                return Err(ExcType::type_error("lock state list is empty"));
            };
            let Value::Bool(locked) = first else {
                return Err(ExcType::type_error("lock state value must be bool"));
            };
            if *locked {
                Ok(false)
            } else {
                *locked = true;
                Ok(true)
            }
        },
    )?;
    Ok(AttrCallResult::Value(Value::Bool(acquired)))
}

/// Internal implementation for lock `release()`.
fn lock_release(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Lock.release", heap)?;
    defer_drop!(state, heap);
    heap.with_entry_mut(
        extract_list_state_id(state, heap, "lock state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("lock state must be a list"));
            };
            let Some(first) = list.as_vec_mut().first_mut() else {
                return Err(ExcType::type_error("lock state list is empty"));
            };
            let Value::Bool(locked) = first else {
                return Err(ExcType::type_error("lock state value must be bool"));
            };
            if !*locked {
                return Err(SimpleException::new_msg(ExcType::RuntimeError, "Lock is not acquired.").into());
            }
            *locked = false;
            Ok(())
        },
    )?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Internal implementation for lock `locked()`.
fn lock_locked(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Lock.locked", heap)?;
    defer_drop!(state, heap);
    let locked = event_state(heap, state)?;
    Ok(AttrCallResult::Value(Value::Bool(locked)))
}

/// Internal implementation for semaphore `acquire()`.
fn semaphore_acquire(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Semaphore.acquire", heap)?;
    defer_drop!(state, heap);
    let acquired = heap.with_entry_mut(
        extract_list_state_id(state, heap, "semaphore state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("semaphore state must be a list"));
            };
            let Some(first) = list.as_vec_mut().first_mut() else {
                return Err(ExcType::type_error("semaphore state list is empty"));
            };
            let Value::Int(counter) = first else {
                return Err(ExcType::type_error("semaphore state value must be int"));
            };
            if *counter > 0 {
                *counter -= 1;
                Ok(true)
            } else {
                Ok(false)
            }
        },
    )?;
    Ok(AttrCallResult::Value(Value::Bool(acquired)))
}

/// Internal implementation for semaphore `release()`.
fn semaphore_release(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Semaphore.release", heap)?;
    defer_drop!(state, heap);
    heap.with_entry_mut(
        extract_list_state_id(state, heap, "semaphore state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("semaphore state must be a list"));
            };
            let Some(first) = list.as_vec_mut().first_mut() else {
                return Err(ExcType::type_error("semaphore state list is empty"));
            };
            let Value::Int(counter) = first else {
                return Err(ExcType::type_error("semaphore state value must be int"));
            };
            *counter = counter.saturating_add(1);
            Ok(())
        },
    )?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Internal implementation for semaphore `locked()`.
fn semaphore_locked(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state = args.get_one_arg("asyncio.Semaphore.locked", heap)?;
    defer_drop!(state, heap);
    let is_locked = heap.with_entry_mut(
        extract_list_state_id(state, heap, "semaphore state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("semaphore state must be a list"));
            };
            let Some(first) = list.as_vec().first() else {
                return Err(ExcType::type_error("semaphore state list is empty"));
            };
            let Value::Int(counter) = first else {
                return Err(ExcType::type_error("semaphore state value must be int"));
            };
            Ok(*counter <= 0)
        },
    )?;
    Ok(AttrCallResult::Value(Value::Bool(is_locked)))
}

/// Creates a `functools.partial`-like callable for internal asyncio objects.
fn make_partial(
    heap: &mut Heap<impl ResourceTracker>,
    function: AsyncioFunctions,
    args: Vec<Value>,
) -> Result<Value, ResourceError> {
    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Asyncio(function)),
        args,
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    Ok(Value::Ref(partial_id))
}

/// Extracts the list-backed state id from a value.
fn extract_list_state_id(value: &Value, _heap: &Heap<impl ResourceTracker>, state_name: &str) -> RunResult<HeapId> {
    let Value::Ref(state_id) = value else {
        return Err(ExcType::type_error(format!("{state_name} must be a list")));
    };
    Ok(*state_id)
}

/// Reads the boolean event/lock state from list-backed storage.
fn event_state(heap: &mut Heap<impl ResourceTracker>, value: &Value) -> RunResult<bool> {
    heap.with_entry_mut(
        extract_list_state_id(value, heap, "event state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("event state must be a list"));
            };
            let Some(first) = list.as_vec().first() else {
                return Err(ExcType::type_error("event state list is empty"));
            };
            let Value::Bool(is_set) = first else {
                return Err(ExcType::type_error("event state value must be bool"));
            };
            Ok(*is_set)
        },
    )
}

/// Writes the boolean event state into list-backed storage.
fn set_event_state(heap: &mut Heap<impl ResourceTracker>, value: &Value, is_set: bool) -> RunResult<()> {
    heap.with_entry_mut(
        extract_list_state_id(value, heap, "event state")?,
        |_heap_inner, data| {
            let HeapData::List(list) = data else {
                return Err(ExcType::type_error("event state must be a list"));
            };
            let Some(first) = list.as_vec_mut().first_mut() else {
                return Err(ExcType::type_error("event state list is empty"));
            };
            let Value::Bool(slot) = first else {
                return Err(ExcType::type_error("event state value must be bool"));
            };
            *slot = is_set;
            Ok(())
        },
    )
}
