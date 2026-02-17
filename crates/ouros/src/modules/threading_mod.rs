//! Minimal single-threaded `threading` module.
//!
//! The sandbox is single-threaded, so `Thread.start()` executes targets synchronously.

use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, Instance, List, Module, OurosIter, PyTrait, Str, Type, allocate_tuple,
        compute_c3_mro,
    },
    value::{EitherStr, Value},
};

const THREAD_TARGET_ATTR: &str = "_ouros_thread_target";
const THREAD_ARGS_ATTR: &str = "_ouros_thread_args";
const THREAD_KWARGS_ATTR: &str = "_ouros_thread_kwargs";
const THREAD_STARTED_ATTR: &str = "_ouros_thread_started";
const THREAD_ALIVE_ATTR: &str = "_ouros_thread_alive";
const THREAD_DAEMON_ATTR: &str = "daemon";
const THREAD_NAME_ATTR: &str = "name";
const THREAD_IDENT_ATTR: &str = "ident";

const LOCK_LOCKED_ATTR: &str = "_ouros_lock_locked";
const RLOCK_COUNT_ATTR: &str = "_ouros_rlock_count";
const EVENT_FLAG_ATTR: &str = "_ouros_event_flag";

/// `threading` module functions and helper-class methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ThreadingFunctions {
    CurrentThread,
    MainThread,
    ActiveCount,
    Enumerate,

    ThreadInit,
    ThreadStart,
    ThreadJoin,
    ThreadIsAlive,

    LockInit,
    LockAcquire,
    LockRelease,
    LockLocked,
    LockEnter,
    LockExit,

    RLockInit,
    RLockAcquire,
    RLockRelease,
    RLockLocked,
    RLockEnter,
    RLockExit,

    EventInit,
    EventSet,
    EventClear,
    EventWait,
    EventIsSet,
}

/// Creates the `threading` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let module_name = interns
        .try_get_str_id("threading")
        .unwrap_or_else(|| StaticStrings::EmptyString.into());
    let mut module = Module::new(module_name);

    module.set_attr_text(
        "current_thread",
        Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::CurrentThread)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "main_thread",
        Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::MainThread)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "active_count",
        Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::ActiveCount)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "enumerate",
        Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::Enumerate)),
        heap,
        interns,
    )?;

    let thread_class = create_thread_class(heap, interns)?;
    module.set_attr_text("Thread", Value::Ref(thread_class), heap, interns)?;

    let lock_class = create_lock_class(heap, interns)?;
    module.set_attr_text("Lock", Value::Ref(lock_class), heap, interns)?;

    let rlock_class = create_rlock_class(heap, interns)?;
    module.set_attr_text("RLock", Value::Ref(rlock_class), heap, interns)?;

    let event_class = create_event_class(heap, interns)?;
    module.set_attr_text("Event", Value::Ref(event_class), heap, interns)?;

    let local_class = create_local_class(heap, interns)?;
    module.set_attr_text("local", Value::Ref(local_class), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ThreadingFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        ThreadingFunctions::CurrentThread | ThreadingFunctions::MainThread => {
            current_or_main_thread(heap, interns, args)?
        }
        ThreadingFunctions::ActiveCount => active_count(heap, args)?,
        ThreadingFunctions::Enumerate => enumerate(heap, interns, args)?,

        ThreadingFunctions::ThreadInit => thread_init(heap, interns, args)?,
        ThreadingFunctions::ThreadStart => return thread_start(heap, interns, args),
        ThreadingFunctions::ThreadJoin => thread_join(heap, args)?,
        ThreadingFunctions::ThreadIsAlive => thread_is_alive(heap, interns, args)?,

        ThreadingFunctions::LockInit => lock_init(heap, interns, args)?,
        ThreadingFunctions::LockAcquire => lock_acquire(heap, interns, args)?,
        ThreadingFunctions::LockRelease => lock_release(heap, interns, args)?,
        ThreadingFunctions::LockLocked => lock_locked(heap, interns, args)?,
        ThreadingFunctions::LockEnter => lock_enter(heap, interns, args)?,
        ThreadingFunctions::LockExit => lock_exit(heap, interns, args)?,

        ThreadingFunctions::RLockInit => rlock_init(heap, interns, args)?,
        ThreadingFunctions::RLockAcquire => rlock_acquire(heap, interns, args)?,
        ThreadingFunctions::RLockRelease => rlock_release(heap, interns, args)?,
        ThreadingFunctions::RLockLocked => rlock_locked(heap, interns, args)?,
        ThreadingFunctions::RLockEnter => rlock_enter(heap, interns, args)?,
        ThreadingFunctions::RLockExit => rlock_exit(heap, interns, args)?,

        ThreadingFunctions::EventInit => event_init(heap, interns, args)?,
        ThreadingFunctions::EventSet => event_set(heap, interns, args)?,
        ThreadingFunctions::EventClear => event_clear(heap, interns, args)?,
        ThreadingFunctions::EventWait => event_wait(heap, interns, args)?,
        ThreadingFunctions::EventIsSet => event_is_set(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

fn active_count(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("active_count", heap)?;
    Ok(Value::Int(1))
}

fn enumerate(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("enumerate", heap)?;
    let main = create_main_thread_like_instance(heap, interns)?;
    let list_id = heap.allocate(HeapData::List(List::new(vec![Value::Ref(main)])))?;
    Ok(Value::Ref(list_id))
}

fn current_or_main_thread(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    args.check_zero_args("current_thread", heap)?;
    Ok(Value::Ref(create_main_thread_like_instance(heap, interns)?))
}

fn thread_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Thread.__init__")?;

    let (pos_iter, kwargs_values) = call_args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    let kwargs = parse_keyword_args(kwargs_values, heap, interns)?;

    if positional.len() > 5 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("Thread.__init__", 5, count));
    }

    let mut target = None;
    let mut thread_args = None;
    let mut thread_kwargs = None;
    let mut daemon = None;
    let mut name = None;

    for (key, value) in kwargs {
        match key.as_str() {
            "target" => {
                if target.is_some() {
                    value.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    target.drop_with_heap(heap);
                    thread_args.drop_with_heap(heap);
                    thread_kwargs.drop_with_heap(heap);
                    daemon.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("Thread.__init__", "target"));
                }
                target = Some(value);
            }
            "args" => {
                if thread_args.is_some() {
                    value.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    target.drop_with_heap(heap);
                    thread_args.drop_with_heap(heap);
                    thread_kwargs.drop_with_heap(heap);
                    daemon.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("Thread.__init__", "args"));
                }
                thread_args = Some(value);
            }
            "kwargs" => {
                if thread_kwargs.is_some() {
                    value.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    target.drop_with_heap(heap);
                    thread_args.drop_with_heap(heap);
                    thread_kwargs.drop_with_heap(heap);
                    daemon.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("Thread.__init__", "kwargs"));
                }
                thread_kwargs = Some(value);
            }
            "daemon" => {
                if daemon.is_some() {
                    value.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    target.drop_with_heap(heap);
                    thread_args.drop_with_heap(heap);
                    thread_kwargs.drop_with_heap(heap);
                    daemon.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("Thread.__init__", "daemon"));
                }
                daemon = Some(value);
            }
            "name" => {
                if name.is_some() {
                    value.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    target.drop_with_heap(heap);
                    thread_args.drop_with_heap(heap);
                    thread_kwargs.drop_with_heap(heap);
                    daemon.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("Thread.__init__", "name"));
                }
                name = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                positional.drop_with_heap(heap);
                target.drop_with_heap(heap);
                thread_args.drop_with_heap(heap);
                thread_kwargs.drop_with_heap(heap);
                daemon.drop_with_heap(heap);
                name.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("Thread.__init__", &key));
            }
        }
    }

    let mut positional_iter = positional.into_iter();
    if target.is_none() {
        target = positional_iter.next();
    }
    if thread_args.is_none() {
        thread_args = positional_iter.next();
    }
    if thread_kwargs.is_none() {
        thread_kwargs = positional_iter.next();
    }
    if daemon.is_none() {
        daemon = positional_iter.next();
    }
    if name.is_none() {
        name = positional_iter.next();
    }

    let target = target.unwrap_or(Value::None);
    let thread_args = match thread_args {
        Some(value) => value,
        None => empty_tuple_value(heap)?,
    };
    let thread_kwargs = match thread_kwargs {
        Some(value) => value,
        None => Value::Ref(heap.allocate(HeapData::Dict(Dict::new()))?),
    };
    let daemon = daemon.unwrap_or(Value::Bool(false));
    let name = match name {
        Some(value) => value,
        None => Value::Ref(heap.allocate(HeapData::Str(Str::from("Thread")))?),
    };

    set_instance_attr_by_name(self_id, THREAD_TARGET_ATTR, target, heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_ARGS_ATTR, thread_args, heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_KWARGS_ATTR, thread_kwargs, heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_STARTED_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_ALIVE_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_DAEMON_ATTR, daemon, heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_NAME_ATTR, name, heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_IDENT_ATTR, Value::None, heap, interns)?;

    Ok(Value::None)
}

fn thread_start(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Thread.start")?;
    call_args.check_zero_args("Thread.start", heap)?;

    let already_started = get_instance_bool_attr(self_id, THREAD_STARTED_ATTR, heap, interns).unwrap_or(false);
    if already_started {
        return Err(SimpleException::new_msg(ExcType::RuntimeError, "threads can only be started once").into());
    }

    set_instance_attr_by_name(self_id, THREAD_STARTED_ATTR, Value::Bool(true), heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_ALIVE_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(self_id, THREAD_IDENT_ATTR, Value::Int(2), heap, interns)?;

    let target = get_instance_attr_by_name(self_id, THREAD_TARGET_ATTR, heap, interns).unwrap_or(Value::None);
    let mut target_guard = HeapGuard::new(target, heap);
    let (target, heap) = target_guard.as_parts_mut();
    if matches!(target, Value::None) {
        return Ok(AttrCallResult::Value(Value::None));
    }

    let args_value = get_instance_attr_by_name(self_id, THREAD_ARGS_ATTR, heap, interns)
        .unwrap_or_else(|| empty_tuple_value(heap).unwrap_or(Value::None));
    let kwargs_value = get_instance_attr_by_name(self_id, THREAD_KWARGS_ATTR, heap, interns).unwrap_or_else(|| {
        Value::Ref(
            heap.allocate(HeapData::Dict(Dict::new()))
                .expect("dict alloc should succeed"),
        )
    });

    let call_args = build_call_args_from_values(args_value, kwargs_value, heap, interns)?;
    let (target, _heap) = target_guard.into_parts();
    Ok(AttrCallResult::CallFunction(target, call_args))
}

fn thread_join(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (_self_id, call_args) = extract_instance_self_and_args(args, heap, "Thread.join")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    for value in positional {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    Ok(Value::None)
}

fn thread_is_alive(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Thread.is_alive")?;
    call_args.check_zero_args("Thread.is_alive", heap)?;
    Ok(Value::Bool(
        get_instance_bool_attr(self_id, THREAD_ALIVE_ATTR, heap, interns).unwrap_or(false),
    ))
}

fn lock_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Lock.__init__")?;
    call_args.check_zero_args("Lock.__init__", heap)?;
    set_instance_attr_by_name(self_id, LOCK_LOCKED_ATTR, Value::Bool(false), heap, interns)?;
    Ok(Value::None)
}

fn lock_acquire(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Lock.acquire")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();

    let mut blocking = true;
    if let Some(value) = positional.get_mut(0) {
        blocking = value.py_bool(heap, interns);
    }
    for value in positional {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        let key_name = key
            .as_either_str(heap)
            .map(|s| s.as_str(interns).to_string())
            .unwrap_or_default();
        key.drop_with_heap(heap);
        if key_name == "blocking" {
            blocking = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
        } else {
            value.drop_with_heap(heap);
        }
    }

    let currently_locked = get_instance_bool_attr(self_id, LOCK_LOCKED_ATTR, heap, interns).unwrap_or(false);
    if currently_locked && !blocking {
        return Ok(Value::Bool(false));
    }

    set_instance_attr_by_name(self_id, LOCK_LOCKED_ATTR, Value::Bool(true), heap, interns)?;
    Ok(Value::Bool(true))
}

fn lock_release(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Lock.release")?;
    call_args.check_zero_args("Lock.release", heap)?;

    let currently_locked = get_instance_bool_attr(self_id, LOCK_LOCKED_ATTR, heap, interns).unwrap_or(false);
    if !currently_locked {
        return Err(SimpleException::new_msg(ExcType::RuntimeError, "release unlocked lock").into());
    }

    set_instance_attr_by_name(self_id, LOCK_LOCKED_ATTR, Value::Bool(false), heap, interns)?;
    Ok(Value::None)
}

fn lock_locked(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Lock.locked")?;
    call_args.check_zero_args("Lock.locked", heap)?;

    let locked = get_instance_attr_by_name(self_id, LOCK_LOCKED_ATTR, heap, interns)
        .is_some_and(|value| matches!(value, Value::Bool(true)));
    Ok(Value::Bool(locked))
}

fn lock_enter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Lock.__enter__")?;
    call_args.check_zero_args("Lock.__enter__", heap)?;
    set_instance_attr_by_name(self_id, LOCK_LOCKED_ATTR, Value::Bool(true), heap, interns)?;
    Ok(Value::Bool(true))
}

fn lock_exit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Lock.__exit__")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    set_instance_attr_by_name(self_id, LOCK_LOCKED_ATTR, Value::Bool(false), heap, interns)?;
    Ok(Value::Bool(false))
}

fn rlock_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "RLock.__init__")?;
    call_args.check_zero_args("RLock.__init__", heap)?;
    set_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, Value::Int(0), heap, interns)?;
    Ok(Value::None)
}

fn rlock_acquire(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "RLock.acquire")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let current_count = get_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, heap, interns)
        .and_then(|v| match v {
            Value::Int(i) => Some(i),
            _ => None,
        })
        .unwrap_or(0);
    set_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, Value::Int(current_count + 1), heap, interns)?;
    Ok(Value::Bool(true))
}

fn rlock_release(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "RLock.release")?;
    call_args.check_zero_args("RLock.release", heap)?;

    let current_count = get_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, heap, interns)
        .and_then(|v| match v {
            Value::Int(i) => Some(i),
            _ => None,
        })
        .unwrap_or(0);
    if current_count <= 0 {
        return Err(SimpleException::new_msg(ExcType::RuntimeError, "cannot release un-acquired lock").into());
    }

    set_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, Value::Int(current_count - 1), heap, interns)?;
    Ok(Value::None)
}

fn rlock_locked(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "RLock.locked")?;
    call_args.check_zero_args("RLock.locked", heap)?;

    let count = get_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, heap, interns)
        .and_then(|v| match v {
            Value::Int(i) => Some(i),
            _ => None,
        })
        .unwrap_or(0);
    Ok(Value::Bool(count > 0))
}

fn rlock_enter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "RLock.__enter__")?;
    call_args.check_zero_args("RLock.__enter__", heap)?;

    let count = get_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, heap, interns)
        .and_then(|v| match v {
            Value::Int(i) => Some(i),
            _ => None,
        })
        .unwrap_or(0);
    set_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, Value::Int(count + 1), heap, interns)?;
    Ok(Value::Bool(true))
}

fn rlock_exit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "RLock.__exit__")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let count = get_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, heap, interns)
        .and_then(|v| match v {
            Value::Int(i) => Some(i),
            _ => None,
        })
        .unwrap_or(0);
    if count > 0 {
        set_instance_attr_by_name(self_id, RLOCK_COUNT_ATTR, Value::Int(count - 1), heap, interns)?;
    }
    Ok(Value::Bool(false))
}

fn event_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Event.__init__")?;
    call_args.check_zero_args("Event.__init__", heap)?;
    set_instance_attr_by_name(self_id, EVENT_FLAG_ATTR, Value::Bool(false), heap, interns)?;
    Ok(Value::None)
}

fn event_set(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Event.set")?;
    call_args.check_zero_args("Event.set", heap)?;
    set_instance_attr_by_name(self_id, EVENT_FLAG_ATTR, Value::Bool(true), heap, interns)?;
    Ok(Value::None)
}

fn event_clear(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Event.clear")?;
    call_args.check_zero_args("Event.clear", heap)?;
    set_instance_attr_by_name(self_id, EVENT_FLAG_ATTR, Value::Bool(false), heap, interns)?;
    Ok(Value::None)
}

fn event_wait(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Event.wait")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    Ok(Value::Bool(
        get_instance_bool_attr(self_id, EVENT_FLAG_ATTR, heap, interns).unwrap_or(false),
    ))
}

fn event_is_set(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Event.is_set")?;
    call_args.check_zero_args("Event.is_set", heap)?;
    Ok(Value::Bool(
        get_instance_attr_by_name(self_id, EVENT_FLAG_ATTR, heap, interns)
            .is_some_and(|value| matches!(value, Value::Bool(true))),
    ))
}

fn create_thread_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    create_helper_class(
        "threading.Thread",
        &[
            (
                "__init__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::ThreadInit)),
            ),
            (
                "start",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::ThreadStart)),
            ),
            (
                "join",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::ThreadJoin)),
            ),
            (
                "is_alive",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::ThreadIsAlive)),
            ),
        ],
        heap,
        interns,
    )
}

fn create_lock_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    create_helper_class(
        "threading.Lock",
        &[
            (
                "__init__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::LockInit)),
            ),
            (
                "acquire",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::LockAcquire)),
            ),
            (
                "release",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::LockRelease)),
            ),
            (
                "locked",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::LockLocked)),
            ),
            (
                "__enter__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::LockEnter)),
            ),
            (
                "__exit__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::LockExit)),
            ),
        ],
        heap,
        interns,
    )
}

fn create_rlock_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    create_helper_class(
        "threading.RLock",
        &[
            (
                "__init__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::RLockInit)),
            ),
            (
                "acquire",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::RLockAcquire)),
            ),
            (
                "release",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::RLockRelease)),
            ),
            (
                "locked",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::RLockLocked)),
            ),
            (
                "__enter__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::RLockEnter)),
            ),
            (
                "__exit__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::RLockExit)),
            ),
        ],
        heap,
        interns,
    )
}

fn create_event_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    create_helper_class(
        "threading.Event",
        &[
            (
                "__init__",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::EventInit)),
            ),
            (
                "set",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::EventSet)),
            ),
            (
                "clear",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::EventClear)),
            ),
            (
                "wait",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::EventWait)),
            ),
            (
                "is_set",
                Value::ModuleFunction(ModuleFunctions::Threading(ThreadingFunctions::EventIsSet)),
            ),
        ],
        heap,
        interns,
    )
}

fn create_local_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap("threading.local".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        vec![object_class],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[object_class], heap, interns)
        .expect("local helper class should always have valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(class_obj) = heap.get_mut(class_id) {
        class_obj.set_mro(mro);
    }

    Ok(class_id)
}

fn create_main_thread_like_instance(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let thread_class = create_thread_class(heap, interns)?;
    let instance_id = allocate_instance(thread_class, heap)?;

    set_instance_attr_by_name(
        instance_id,
        THREAD_NAME_ATTR,
        Value::Ref(heap.allocate(HeapData::Str(Str::from("MainThread")))?),
        heap,
        interns,
    )
    .expect("main-thread name assignment should succeed");
    set_instance_attr_by_name(instance_id, THREAD_DAEMON_ATTR, Value::Bool(false), heap, interns)
        .expect("main-thread daemon assignment should succeed");
    set_instance_attr_by_name(instance_id, THREAD_IDENT_ATTR, Value::Int(1), heap, interns)
        .expect("main-thread ident assignment should succeed");
    set_instance_attr_by_name(instance_id, THREAD_STARTED_ATTR, Value::Bool(true), heap, interns)
        .expect("main-thread started assignment should succeed");
    set_instance_attr_by_name(instance_id, THREAD_ALIVE_ATTR, Value::Bool(true), heap, interns)
        .expect("main-thread alive assignment should succeed");
    set_instance_attr_by_name(instance_id, THREAD_TARGET_ATTR, Value::None, heap, interns)
        .expect("main-thread target assignment should succeed");
    set_instance_attr_by_name(instance_id, THREAD_ARGS_ATTR, empty_tuple_value(heap)?, heap, interns)
        .expect("main-thread args assignment should succeed");
    set_instance_attr_by_name(
        instance_id,
        THREAD_KWARGS_ATTR,
        Value::Ref(heap.allocate(HeapData::Dict(Dict::new()))?),
        heap,
        interns,
    )
    .expect("main-thread kwargs assignment should succeed");

    Ok(instance_id)
}

fn create_helper_class(
    class_name: &str,
    methods: &[(&str, Value)],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let mut attrs = Dict::new();
    for (name, value) in methods {
        dict_set_str_attr(&mut attrs, name, value.clone_with_heap(heap), heap, interns)?;
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap(class_name.to_string()),
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

fn initialize_helper_class_mro(
    class_id: HeapId,
    object_class: HeapId,
    class_uid: u64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    let mro = compute_c3_mro(class_id, &[object_class], heap, interns)
        .expect("threading helper class should always have a valid MRO");
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

fn allocate_instance(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> Result<HeapId, ResourceError> {
    let (slot_len, has_dict) = match heap.get(class_id) {
        HeapData::ClassObject(cls) => (cls.slot_layout().len(), cls.instance_has_dict()),
        _ => (0, true),
    };

    let attrs_id = if has_dict {
        Some(heap.allocate(HeapData::Dict(Dict::new()))?)
    } else {
        None
    };

    let mut slot_values = Vec::with_capacity(slot_len);
    for _ in 0..slot_len {
        slot_values.push(Value::Undefined);
    }

    heap.inc_ref(class_id);
    heap.allocate(HeapData::Instance(Instance::new(
        class_id,
        attrs_id,
        slot_values,
        Vec::new(),
    )))
}

fn empty_tuple_value(heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
    allocate_tuple(SmallVec::new(), heap)
}

fn parse_keyword_args(
    kwargs_values: KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<(String, Value)>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (key, value) in kwargs_values {
        let Some(key_str) = key.as_either_str(heap).map(|s| s.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (_, v) in out {
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);
        out.push((key_str, value));
    }
    Ok(out)
}

fn build_call_args_from_values(
    args_value: Value,
    kwargs_value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<ArgValues> {
    let mut kwargs_value_guard = HeapGuard::new(kwargs_value, heap);
    let (kwargs_value, heap) = kwargs_value_guard.as_parts_mut();

    let positional = value_to_vec(args_value, heap, interns)?;
    let mut positional_guard = HeapGuard::new(positional, heap);
    let (_positional, heap) = positional_guard.as_parts_mut();
    let kwargs = dict_value_to_kwargs(kwargs_value.clone_with_heap(heap), heap, interns)?;
    let (positional, _heap) = positional_guard.into_parts();

    Ok(if kwargs.is_empty() {
        match positional.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(positional.into_iter().next().expect("checked")),
            2 => {
                let mut it = positional.into_iter();
                ArgValues::Two(it.next().expect("checked"), it.next().expect("checked"))
            }
            _ => ArgValues::ArgsKargs {
                args: positional,
                kwargs,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        }
    })
}

fn value_to_vec(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<Value>> {
    let mut value_guard = HeapGuard::new(value, heap);
    let (value, heap) = value_guard.as_parts_mut();

    match value {
        Value::None => Ok(Vec::new()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => Ok(tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect()),
            HeapData::List(list) => Ok(list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect()),
            _ => {
                let mut iter = OurosIter::new(value.clone_with_heap(heap), heap, interns)?;
                let out: Vec<Value> = iter.collect(heap, interns)?;
                Ok(out)
            }
        },
        _ => {
            let mut iter = OurosIter::new(value.clone_with_heap(heap), heap, interns)?;
            let out: Vec<Value> = iter.collect(heap, interns)?;
            Ok(out)
        }
    }
}

fn dict_value_to_kwargs(
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<KwargsValues> {
    let mut value_guard = HeapGuard::new(value, heap);
    let (value, heap) = value_guard.as_parts_mut();

    let dict_id = match value {
        Value::Ref(dict_id) => *dict_id,
        _ => return Ok(KwargsValues::Empty),
    };

    let cloned = clone_dict(dict_id, heap, interns)?;
    Ok(KwargsValues::Dict(cloned))
}

fn clone_dict(dict_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Dict> {
    let entries: Vec<(Value, Value)> = match heap.get(dict_id) {
        HeapData::Dict(dict) => dict
            .iter()
            .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
            .collect(),
        _ => return Err(ExcType::type_error("kwargs must be a mapping")),
    };

    let mut out = Dict::new();
    for (key, value) in entries {
        if let Some(old) = out.set(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }
    Ok(out)
}

fn extract_instance_self_and_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    method_name: &str,
) -> RunResult<(HeapId, ArgValues)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 1, 0));
    }
    let self_value = positional.remove(0);
    let self_id = match &self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            self_value.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("{method_name} expected instance")));
        }
    };
    self_value.drop_with_heap(heap);
    Ok((self_id, arg_values_from_parts(positional, kwargs)))
}

fn arg_values_from_parts(positional: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    if kwargs.is_empty() {
        match positional.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(positional.into_iter().next().expect("length checked")),
            2 => {
                let mut iter = positional.into_iter();
                ArgValues::Two(
                    iter.next().expect("length checked"),
                    iter.next().expect("length checked"),
                )
            }
            _ => ArgValues::ArgsKargs {
                args: positional,
                kwargs,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        }
    }
}

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
            return Err(ExcType::type_error("threading helper expected instance"));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

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

fn get_instance_bool_attr(
    instance_id: HeapId,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<bool> {
    get_instance_attr_by_name(instance_id, name, heap, interns).and_then(|value| match value {
        Value::Bool(flag) => Some(flag),
        _ => None,
    })
}

fn dict_set_str_attr(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), crate::resource::ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}
