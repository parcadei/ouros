//! Implementation of the `queue` module.
//!
//! This module provides CPython-compatible queue container classes for
//! single-threaded execution:
//! - `Queue` (FIFO)
//! - `LifoQueue` (LIFO/stack)
//! - `PriorityQueue` (min-heap priority queue)
//! - `SimpleQueue` (unbounded FIFO)
//!
//! Ouros does not provide thread scheduling, so `put()`/`get()` never block and
//! `task_done()`/`join()` are no-ops kept for API parity.

use std::cmp::Ordering;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, ClassObject, Dict, List, PyTrait, Str, Type, compute_c3_mro},
    value::{EitherStr, Value},
};

/// Hidden instance attribute storing the backing list object.
const QUEUE_ITEMS_ATTR: &str = "__queue_items__";
/// Hidden instance attribute storing maxsize for bounded queues.
const QUEUE_MAXSIZE_ATTR: &str = "__queue_maxsize__";
/// Hidden instance attribute storing queue strategy.
const QUEUE_MODE_ATTR: &str = "__queue_mode__";

/// Queue mode: FIFO queue.
const QUEUE_MODE_FIFO: i64 = 0;
/// Queue mode: LIFO queue.
const QUEUE_MODE_LIFO: i64 = 1;
/// Queue mode: priority queue backed by a min-heap.
const QUEUE_MODE_PRIORITY: i64 = 2;
/// Queue mode: simple unbounded FIFO queue.
const QUEUE_MODE_SIMPLE: i64 = 3;

/// Module-callable functions for `queue` class methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum QueueFunctions {
    #[strum(serialize = "Queue.__init__")]
    QueueInit,
    #[strum(serialize = "LifoQueue.__init__")]
    LifoQueueInit,
    #[strum(serialize = "PriorityQueue.__init__")]
    PriorityQueueInit,
    #[strum(serialize = "SimpleQueue.__init__")]
    SimpleQueueInit,
    #[strum(serialize = "put")]
    Put,
    #[strum(serialize = "get")]
    Get,
    #[strum(serialize = "put_nowait")]
    PutNowait,
    #[strum(serialize = "get_nowait")]
    GetNowait,
    #[strum(serialize = "qsize")]
    Qsize,
    #[strum(serialize = "empty")]
    Empty,
    #[strum(serialize = "full")]
    Full,
    #[strum(serialize = "task_done")]
    TaskDone,
    #[strum(serialize = "join")]
    Join,
}

/// Parsed queue mode for dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueMode {
    Fifo,
    Lifo,
    Priority,
    Simple,
}

impl QueueMode {
    fn from_i64(mode: i64) -> Option<Self> {
        match mode {
            QUEUE_MODE_FIFO => Some(Self::Fifo),
            QUEUE_MODE_LIFO => Some(Self::Lifo),
            QUEUE_MODE_PRIORITY => Some(Self::Priority),
            QUEUE_MODE_SIMPLE => Some(Self::Simple),
            _ => None,
        }
    }
}

/// Internal state extracted from a queue instance.
#[derive(Debug, Clone, Copy)]
struct QueueState {
    items_id: HeapId,
    maxsize: i64,
    mode: QueueMode,
}

/// Creates the `queue` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Queue);

    // Exception classes. Ouros currently maps raising to builtin exception
    // categories, but exposing dedicated classes keeps the module API shape.
    let empty_class_id = create_exception_class(heap, interns, "queue.Empty", Type::Exception(ExcType::IndexError))?;
    let full_class_id = create_exception_class(heap, interns, "queue.Full", Type::Exception(ExcType::RuntimeError))?;
    module.set_attr_str("Empty", Value::Ref(empty_class_id), heap, interns)?;
    module.set_attr_str("Full", Value::Ref(full_class_id), heap, interns)?;

    let queue_class_id = create_queue_class(heap, interns)?;
    module.set_attr_str("Queue", Value::Ref(queue_class_id), heap, interns)?;

    let lifo_class_id = create_lifo_queue_class(heap, interns, queue_class_id)?;
    module.set_attr_str("LifoQueue", Value::Ref(lifo_class_id), heap, interns)?;

    let priority_class_id = create_priority_queue_class(heap, interns, queue_class_id)?;
    module.set_attr_str("PriorityQueue", Value::Ref(priority_class_id), heap, interns)?;

    let simple_queue_class_id = create_simple_queue_class(heap, interns)?;
    module.set_attr_str("SimpleQueue", Value::Ref(simple_queue_class_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `queue` class methods.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: QueueFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        QueueFunctions::QueueInit => queue_init(heap, interns, args, "Queue.__init__", QueueMode::Fifo),
        QueueFunctions::LifoQueueInit => queue_init(heap, interns, args, "LifoQueue.__init__", QueueMode::Lifo),
        QueueFunctions::PriorityQueueInit => {
            queue_init(heap, interns, args, "PriorityQueue.__init__", QueueMode::Priority)
        }
        QueueFunctions::SimpleQueueInit => simple_queue_init(heap, interns, args),
        QueueFunctions::Put => queue_put(heap, interns, args),
        QueueFunctions::Get => queue_get(heap, interns, args),
        QueueFunctions::PutNowait => queue_put_nowait(heap, interns, args),
        QueueFunctions::GetNowait => queue_get_nowait(heap, interns, args),
        QueueFunctions::Qsize => queue_qsize(heap, interns, args),
        QueueFunctions::Empty => queue_empty(heap, interns, args),
        QueueFunctions::Full => queue_full(heap, interns, args),
        QueueFunctions::TaskDone => queue_task_done(heap, args),
        QueueFunctions::Join => queue_join(heap, args),
    }
}

/// Creates a class object representing an exception-like type.
fn create_exception_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    class_name: &str,
    base: Type,
) -> Result<HeapId, ResourceError> {
    let base_id = heap.builtin_class_id(base)?;
    let namespace = Dict::new();
    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap(class_name.to_owned()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[base_id],
        namespace,
    )
}

/// Creates the runtime `queue.Queue` class.
fn create_queue_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let object_id = heap.builtin_class_id(Type::Object)?;
    let mut namespace = Dict::new();
    dict_set_str_attr(
        &mut namespace,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::QueueInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "put",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Put)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "get",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Get)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "put_nowait",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::PutNowait)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "get_nowait",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::GetNowait)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "qsize",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Qsize)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "empty",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Empty)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "full",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Full)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "task_done",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::TaskDone)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "join",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Join)),
        heap,
        interns,
    )?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("queue.Queue".to_owned()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[object_id],
        namespace,
    )
}

/// Creates the runtime `queue.LifoQueue` class.
fn create_lifo_queue_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    queue_class_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    dict_set_str_attr(
        &mut namespace,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::LifoQueueInit)),
        heap,
        interns,
    )?;
    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("queue.LifoQueue".to_owned()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[queue_class_id],
        namespace,
    )
}

/// Creates the runtime `queue.PriorityQueue` class.
fn create_priority_queue_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    queue_class_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    dict_set_str_attr(
        &mut namespace,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::PriorityQueueInit)),
        heap,
        interns,
    )?;
    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("queue.PriorityQueue".to_owned()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[queue_class_id],
        namespace,
    )
}

/// Creates the runtime `queue.SimpleQueue` class.
fn create_simple_queue_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let object_id = heap.builtin_class_id(Type::Object)?;
    let mut namespace = Dict::new();
    dict_set_str_attr(
        &mut namespace,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::SimpleQueueInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "put",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Put)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "get",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Get)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "put_nowait",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::PutNowait)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "get_nowait",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::GetNowait)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "qsize",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Qsize)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut namespace,
        "empty",
        Value::ModuleFunction(ModuleFunctions::Queue(QueueFunctions::Empty)),
        heap,
        interns,
    )?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("queue.SimpleQueue".to_owned()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[object_id],
        namespace,
    )
}

/// Creates a runtime class object and wires base/subclass links.
fn create_runtime_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    name: EitherStr,
    metaclass: Value,
    bases: &[HeapId],
    namespace: Dict,
) -> Result<HeapId, ResourceError> {
    for &base_id in bases {
        heap.inc_ref(base_id);
    }
    if let Value::Ref(meta_id) = metaclass {
        heap.inc_ref(meta_id);
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(name, class_uid, metaclass, namespace, bases.to_vec(), vec![]);
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, bases, heap, interns).expect("queue helper class should have a valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    if bases.is_empty() {
        let object_id = heap.builtin_class_id(Type::Object)?;
        heap.with_entry_mut(object_id, |_, data| {
            let HeapData::ClassObject(base_cls) = data else {
                return Err(ExcType::type_error("builtin object is not a class".to_string()));
            };
            base_cls.register_subclass(class_id, class_uid);
            Ok(())
        })
        .expect("object class registry should be mutable");
    } else {
        for &base_id in bases {
            heap.with_entry_mut(base_id, |_, data| {
                let HeapData::ClassObject(base_cls) = data else {
                    return Err(ExcType::type_error("base is not a class".to_string()));
                };
                base_cls.register_subclass(class_id, class_uid);
                Ok(())
            })
            .expect("base class registry should be mutable");
        }
    }

    Ok(class_id)
}

/// Sets a string-keyed class namespace entry and drops any replaced value.
fn dict_set_str_attr(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string class key must be hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Rebuilds an `ArgValues` from raw positional/keyword parts.
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

/// Extracts `self` from an instance method call and returns the remaining args.
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

/// Sets an instance attribute by string key and drops replaced values.
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
            return Err(ExcType::type_error("queue helper expected instance"));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

/// Gets an instance attribute by string key and clones the value.
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

/// Raises `queue.Empty`-equivalent runtime error.
fn queue_empty_error() -> RunError {
    SimpleException::new(ExcType::IndexError, None).into()
}

/// Raises `queue.Full`-equivalent runtime error.
fn queue_full_error() -> RunError {
    SimpleException::new(ExcType::RuntimeError, None).into()
}

/// Raises timeout validation error matching CPython text.
fn timeout_non_negative_error() -> RunError {
    SimpleException::new_msg(ExcType::ValueError, "'timeout' must be a non-negative number").into()
}

/// Reads queue state fields from an initialized instance.
fn queue_state(instance_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<QueueState> {
    let items_value = get_instance_attr_by_name(instance_id, QUEUE_ITEMS_ATTR, heap, interns)
        .ok_or_else(|| ExcType::type_error("queue instance is not initialized"))?;
    let maxsize_value = get_instance_attr_by_name(instance_id, QUEUE_MAXSIZE_ATTR, heap, interns)
        .ok_or_else(|| ExcType::type_error("queue instance is not initialized"))?;
    let mode_value = get_instance_attr_by_name(instance_id, QUEUE_MODE_ATTR, heap, interns)
        .ok_or_else(|| ExcType::type_error("queue instance is not initialized"))?;

    let items_id = match &items_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::List(_)) => *id,
        _ => {
            items_value.drop_with_heap(heap);
            maxsize_value.drop_with_heap(heap);
            mode_value.drop_with_heap(heap);
            return Err(ExcType::type_error("queue instance has invalid state"));
        }
    };
    items_value.drop_with_heap(heap);

    let maxsize = value_to_i64(&maxsize_value, heap)?;
    maxsize_value.drop_with_heap(heap);

    let mode_i64 = value_to_i64(&mode_value, heap)?;
    mode_value.drop_with_heap(heap);
    let mode = QueueMode::from_i64(mode_i64).ok_or_else(|| ExcType::type_error("queue mode is invalid"))?;

    Ok(QueueState {
        items_id,
        maxsize,
        mode,
    })
}

/// Converts integer-like values (`int`, `bool`, small long-int) to `i64`.
fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Bool(flag) => Ok(i64::from(*flag)),
        _ => value.as_int(heap),
    }
}

/// Validates optional timeout value and rejects negative numbers.
fn validate_timeout(timeout: Option<&Value>, heap: &Heap<impl ResourceTracker>) -> RunResult<()> {
    let Some(timeout) = timeout else {
        return Ok(());
    };
    if matches!(timeout, Value::None) {
        return Ok(());
    }
    match timeout {
        Value::Int(i) => {
            if *i < 0 {
                return Err(timeout_non_negative_error());
            }
        }
        Value::Bool(_) => {}
        Value::Float(f) => {
            if *f < 0.0 {
                return Err(timeout_non_negative_error());
            }
        }
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id)
                && let Some(i) = li.to_i64()
                && i < 0
            {
                return Err(timeout_non_negative_error());
            }
        }
        _ => return Err(timeout_non_negative_error()),
    }
    Ok(())
}

/// Returns queue length.
fn queue_len(items_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<usize> {
    heap.with_entry_mut(items_id, |_heap, data| match data {
        HeapData::List(list) => Ok(list.len()),
        _ => Err(ExcType::type_error("queue storage must be a list")),
    })
}

/// Pushes one item according to queue mode.
fn queue_push(
    state: QueueState,
    item: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    heap.with_entry_mut(state.items_id, |heap_inner, data| {
        if let HeapData::List(list) = data {
            list.as_vec_mut().push(item);
            Ok(())
        } else {
            item.drop_with_heap(heap_inner);
            Err(ExcType::type_error("queue storage must be a list"))
        }
    })?;
    if matches!(state.mode, QueueMode::Priority) {
        priority_sift_up(heap, interns, state.items_id);
    }
    Ok(())
}

/// Pops one item according to queue mode.
fn queue_pop(state: QueueState, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    if queue_len(state.items_id, heap)? == 0 {
        return Err(queue_empty_error());
    }
    match state.mode {
        QueueMode::Fifo | QueueMode::Simple => heap.with_entry_mut(state.items_id, |_heap, data| match data {
            HeapData::List(list) => Ok(list.as_vec_mut().remove(0)),
            _ => Err(ExcType::type_error("queue storage must be a list")),
        }),
        QueueMode::Lifo => heap.with_entry_mut(state.items_id, |_heap, data| match data {
            HeapData::List(list) => Ok(list.as_vec_mut().pop().unwrap_or(Value::None)),
            _ => Err(ExcType::type_error("queue storage must be a list")),
        }),
        QueueMode::Priority => priority_pop(heap, interns, state.items_id),
    }
}

/// Sifts up one value in the priority queue list.
fn priority_sift_up(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId) {
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return,
    };
    if len == 0 {
        return;
    }
    let mut idx = len - 1;
    while idx > 0 {
        let parent_idx = (idx - 1) / 2;
        let (current, parent) = {
            let HeapData::List(list) = heap.get(list_id) else {
                return;
            };
            let vec = list.as_vec();
            (vec[idx].clone_with_heap(heap), vec[parent_idx].clone_with_heap(heap))
        };
        let should_swap = {
            let cmp = current.py_cmp(&parent, heap, interns);
            current.drop_with_heap(heap);
            parent.drop_with_heap(heap);
            matches!(cmp, Some(Ordering::Less))
        };
        if !should_swap {
            break;
        }
        heap.with_entry_mut(list_id, |_heap, data| {
            if let HeapData::List(list) = data {
                list.as_vec_mut().swap(idx, parent_idx);
            }
        });
        idx = parent_idx;
    }
}

/// Sifts down one value in the priority queue list.
fn priority_sift_down(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId, start_idx: usize) {
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return,
    };
    if len == 0 {
        return;
    }

    let mut idx = start_idx;
    loop {
        let left = 2 * idx + 1;
        let right = 2 * idx + 2;
        if left >= len {
            break;
        }

        let smallest_child = if right < len {
            let (left_val, right_val) = {
                let HeapData::List(list) = heap.get(list_id) else {
                    return;
                };
                let vec = list.as_vec();
                (vec[left].clone_with_heap(heap), vec[right].clone_with_heap(heap))
            };
            let is_right_smaller = {
                let cmp = left_val.py_cmp(&right_val, heap, interns);
                left_val.drop_with_heap(heap);
                right_val.drop_with_heap(heap);
                matches!(cmp, Some(Ordering::Greater))
            };
            if is_right_smaller { right } else { left }
        } else {
            left
        };

        let (current, child) = {
            let HeapData::List(list) = heap.get(list_id) else {
                return;
            };
            let vec = list.as_vec();
            (
                vec[idx].clone_with_heap(heap),
                vec[smallest_child].clone_with_heap(heap),
            )
        };
        let should_swap = {
            let cmp = current.py_cmp(&child, heap, interns);
            current.drop_with_heap(heap);
            child.drop_with_heap(heap);
            matches!(cmp, Some(Ordering::Greater))
        };
        if !should_swap {
            break;
        }
        heap.with_entry_mut(list_id, |_heap, data| {
            if let HeapData::List(list) = data {
                list.as_vec_mut().swap(idx, smallest_child);
            }
        });
        idx = smallest_child;
    }
}

/// Pops the smallest priority item and restores heap invariant.
fn priority_pop(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId) -> RunResult<Value> {
    let len = queue_len(list_id, heap)?;
    if len == 0 {
        return Err(queue_empty_error());
    }
    if len == 1 {
        return heap.with_entry_mut(list_id, |_heap, data| match data {
            HeapData::List(list) => Ok(list.as_vec_mut().pop().unwrap_or(Value::None)),
            _ => Err(ExcType::type_error("queue storage must be a list")),
        });
    }

    let popped = heap.with_entry_mut(list_id, |_heap, data| match data {
        HeapData::List(list) => {
            let vec = list.as_vec_mut();
            vec.swap(0, len - 1);
            Ok(vec.pop().unwrap_or(Value::None))
        }
        _ => Err(ExcType::type_error("queue storage must be a list")),
    })?;
    priority_sift_down(heap, interns, list_id, 0);
    Ok(popped)
}

/// Parses `Queue.__init__(self, maxsize=0)`-style arguments.
fn parse_init_maxsize(
    method_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(HeapId, i64)> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, method_name)?;
    let (mut positional, kwargs) = method_args.into_parts();

    let mut maxsize = positional.next();
    let positional_count = usize::from(maxsize.is_some()) + positional.len();
    if positional_count > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        maxsize.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(method_name, 2, positional_count + 1));
    }
    positional.drop_with_heap(heap);

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            maxsize.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name == "maxsize" {
            if maxsize.is_some() {
                value.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                maxsize.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(method_name, "maxsize"));
            }
            maxsize = Some(value);
            continue;
        }

        value.drop_with_heap(heap);
        for (rest_key, rest_value) in kwargs_iter {
            rest_key.drop_with_heap(heap);
            rest_value.drop_with_heap(heap);
        }
        maxsize.drop_with_heap(heap);
        return Err(ExcType::type_error_unexpected_keyword(method_name, &key_name));
    }

    let maxsize_i64 = match maxsize {
        Some(value) => {
            let parsed = value_to_i64(&value, heap)?;
            value.drop_with_heap(heap);
            parsed
        }
        None => 0,
    };
    Ok((instance_id, maxsize_i64))
}

/// Parses `put(self, item, block=True, timeout=None)`-style arguments.
fn parse_put_args(
    method_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(HeapId, Value, Option<Value>, Option<Value>)> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, method_name)?;
    let (mut positional, kwargs) = method_args.into_parts();

    let mut item = positional.next();
    let mut block = positional.next();
    let mut timeout = positional.next();

    let positional_count = usize::from(item.is_some()) + usize::from(block.is_some()) + usize::from(timeout.is_some());
    if positional.next().is_some() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        item.drop_with_heap(heap);
        block.drop_with_heap(heap);
        timeout.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(method_name, 4, positional_count + 2));
    }
    positional.drop_with_heap(heap);

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            item.drop_with_heap(heap);
            block.drop_with_heap(heap);
            timeout.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "item" => {
                if item.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    item.drop_with_heap(heap);
                    block.drop_with_heap(heap);
                    timeout.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(method_name, "item"));
                }
                item = Some(value);
            }
            "block" => {
                if block.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    item.drop_with_heap(heap);
                    block.drop_with_heap(heap);
                    timeout.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(method_name, "block"));
                }
                block = Some(value);
            }
            "timeout" => {
                if timeout.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    item.drop_with_heap(heap);
                    block.drop_with_heap(heap);
                    timeout.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(method_name, "timeout"));
                }
                timeout = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                item.drop_with_heap(heap);
                block.drop_with_heap(heap);
                timeout.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(method_name, &key_name));
            }
        }
    }

    let Some(item) = item else {
        block.drop_with_heap(heap);
        timeout.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 2, 1));
    };
    Ok((instance_id, item, block, timeout))
}

/// Parses `put_nowait(self, item)` arguments.
fn parse_put_nowait_args(
    method_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(HeapId, Value)> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, method_name)?;
    let (mut positional, kwargs) = method_args.into_parts();
    let mut item = positional.next();
    let positional_count = usize::from(item.is_some()) + positional.len();
    if positional_count > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        item.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(method_name, 2, positional_count + 1));
    }
    positional.drop_with_heap(heap);

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            item.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name == "item" {
            if item.is_some() {
                value.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                item.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(method_name, "item"));
            }
            item = Some(value);
            continue;
        }

        value.drop_with_heap(heap);
        for (rest_key, rest_value) in kwargs_iter {
            rest_key.drop_with_heap(heap);
            rest_value.drop_with_heap(heap);
        }
        item.drop_with_heap(heap);
        return Err(ExcType::type_error_unexpected_keyword(method_name, &key_name));
    }

    let Some(item) = item else {
        return Err(ExcType::type_error_at_least(method_name, 2, 1));
    };
    Ok((instance_id, item))
}

/// Parses `get(self, block=True, timeout=None)`-style arguments.
fn parse_get_args(
    method_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(HeapId, Option<Value>, Option<Value>)> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, method_name)?;
    let (mut positional, kwargs) = method_args.into_parts();

    let mut block = positional.next();
    let mut timeout = positional.next();
    let positional_count = usize::from(block.is_some()) + usize::from(timeout.is_some()) + positional.len();
    if positional_count > 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        block.drop_with_heap(heap);
        timeout.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(method_name, 3, positional_count + 1));
    }
    positional.drop_with_heap(heap);

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            block.drop_with_heap(heap);
            timeout.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "block" => {
                if block.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    block.drop_with_heap(heap);
                    timeout.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(method_name, "block"));
                }
                block = Some(value);
            }
            "timeout" => {
                if timeout.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    block.drop_with_heap(heap);
                    timeout.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(method_name, "timeout"));
                }
                timeout = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                block.drop_with_heap(heap);
                timeout.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(method_name, &key_name));
            }
        }
    }

    Ok((instance_id, block, timeout))
}

/// Validates there are no arguments besides `self`.
fn parse_noarg_method(method_name: &str, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, method_name)?;
    method_args.check_zero_args(method_name, heap)?;
    Ok(instance_id)
}

/// Initializes `Queue`/`LifoQueue`/`PriorityQueue`.
fn queue_init(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    method_name: &str,
    mode: QueueMode,
) -> RunResult<AttrCallResult> {
    let (instance_id, maxsize) = parse_init_maxsize(method_name, args, heap, interns)?;
    if maxsize < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "maxsize must be >= 0").into());
    }
    let items_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    set_instance_attr_by_name(instance_id, QUEUE_ITEMS_ATTR, Value::Ref(items_id), heap, interns)?;
    set_instance_attr_by_name(instance_id, QUEUE_MAXSIZE_ATTR, Value::Int(maxsize), heap, interns)?;
    let mode_int = match mode {
        QueueMode::Fifo => QUEUE_MODE_FIFO,
        QueueMode::Lifo => QUEUE_MODE_LIFO,
        QueueMode::Priority => QUEUE_MODE_PRIORITY,
        QueueMode::Simple => QUEUE_MODE_SIMPLE,
    };
    set_instance_attr_by_name(instance_id, QUEUE_MODE_ATTR, Value::Int(mode_int), heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Initializes `SimpleQueue`.
fn simple_queue_init(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "SimpleQueue.__init__")?;
    method_args.check_zero_args("SimpleQueue.__init__", heap)?;
    let items_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    set_instance_attr_by_name(instance_id, QUEUE_ITEMS_ATTR, Value::Ref(items_id), heap, interns)?;
    set_instance_attr_by_name(instance_id, QUEUE_MAXSIZE_ATTR, Value::Int(0), heap, interns)?;
    set_instance_attr_by_name(
        instance_id,
        QUEUE_MODE_ATTR,
        Value::Int(QUEUE_MODE_SIMPLE),
        heap,
        interns,
    )?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `put(self, item, block=True, timeout=None)`.
fn queue_put(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (instance_id, item, block, timeout) = parse_put_args("Queue.put", args, heap, interns)?;
    validate_timeout(timeout.as_ref(), heap)?;
    block.drop_with_heap(heap);
    timeout.drop_with_heap(heap);

    let state = queue_state(instance_id, heap, interns)?;
    if state.maxsize > 0 {
        let len = queue_len(state.items_id, heap)?;
        if i64::try_from(len).map(|n| n >= state.maxsize).unwrap_or(true) {
            item.drop_with_heap(heap);
            return Err(queue_full_error());
        }
    }

    queue_push(state, item, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `get(self, block=True, timeout=None)`.
fn queue_get(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (instance_id, block, timeout) = parse_get_args("Queue.get", args, heap, interns)?;
    validate_timeout(timeout.as_ref(), heap)?;
    block.drop_with_heap(heap);
    timeout.drop_with_heap(heap);

    let state = queue_state(instance_id, heap, interns)?;
    let value = queue_pop(state, heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Implements `put_nowait(self, item)`.
fn queue_put_nowait(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, item) = parse_put_nowait_args("Queue.put_nowait", args, heap, interns)?;
    let state = queue_state(instance_id, heap, interns)?;
    if state.maxsize > 0 {
        let len = queue_len(state.items_id, heap)?;
        if i64::try_from(len).map(|n| n >= state.maxsize).unwrap_or(true) {
            item.drop_with_heap(heap);
            return Err(queue_full_error());
        }
    }
    queue_push(state, item, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `get_nowait(self)`.
fn queue_get_nowait(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let instance_id = parse_noarg_method("Queue.get_nowait", args, heap)?;
    let state = queue_state(instance_id, heap, interns)?;
    let value = queue_pop(state, heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Implements `qsize(self)`.
fn queue_qsize(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let instance_id = parse_noarg_method("Queue.qsize", args, heap)?;
    let state = queue_state(instance_id, heap, interns)?;
    let len = queue_len(state.items_id, heap)?;
    Ok(AttrCallResult::Value(Value::Int(
        i64::try_from(len).unwrap_or(i64::MAX),
    )))
}

/// Implements `empty(self)`.
fn queue_empty(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let instance_id = parse_noarg_method("Queue.empty", args, heap)?;
    let state = queue_state(instance_id, heap, interns)?;
    Ok(AttrCallResult::Value(Value::Bool(
        queue_len(state.items_id, heap)? == 0,
    )))
}

/// Implements `full(self)` for bounded queues.
fn queue_full(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let instance_id = parse_noarg_method("Queue.full", args, heap)?;
    let state = queue_state(instance_id, heap, interns)?;
    if matches!(state.mode, QueueMode::Simple) {
        return Ok(AttrCallResult::Value(Value::Bool(false)));
    }
    if state.maxsize <= 0 {
        return Ok(AttrCallResult::Value(Value::Bool(false)));
    }
    let len = queue_len(state.items_id, heap)?;
    let is_full = i64::try_from(len).map(|n| n >= state.maxsize).unwrap_or(true);
    Ok(AttrCallResult::Value(Value::Bool(is_full)))
}

/// Implements `task_done(self)` as a no-op.
fn queue_task_done(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    parse_noarg_method("Queue.task_done", args, heap)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `join(self)` as a no-op.
fn queue_join(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    parse_noarg_method("Queue.join", args, heap)?;
    Ok(AttrCallResult::Value(Value::None))
}
