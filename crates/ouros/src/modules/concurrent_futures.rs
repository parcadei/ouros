//! Minimal single-threaded `concurrent.futures` module.
//!
//! Executors run inline in this sandbox. User-defined callables that require VM
//! frame execution are deferred to `Future.result()` via `AttrCallResult::CallFunction`.

use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::NoPrint,
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, Instance, List, Module, OurosIter, PyTrait, Str, Type, allocate_tuple,
        compute_c3_mro,
    },
    value::{EitherStr, Value},
};

const FUTURE_DONE_ATTR: &str = "_ouros_future_done";
const FUTURE_CANCELLED_ATTR: &str = "_ouros_future_cancelled";
const FUTURE_RESULT_ATTR: &str = "_ouros_future_result";
const FUTURE_EXCEPTION_ATTR: &str = "_ouros_future_exception";
const FUTURE_CALLABLE_ATTR: &str = "_ouros_future_callable";
const FUTURE_ARGS_ATTR: &str = "_ouros_future_args";
const FUTURE_KWARGS_ATTR: &str = "_ouros_future_kwargs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ConcurrentFuturesFunctions {
    AsCompleted,
    Wait,

    FutureInit,
    FutureResult,
    FutureException,
    FutureDone,
    FutureCancelled,
    FutureAddDoneCallback,
    FutureCancel,

    ExecutorInit,
    ExecutorSubmit,
    ExecutorMap,
    ExecutorShutdown,
    ExecutorEnter,
    ExecutorExit,
}

pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let module_name = interns
        .try_get_str_id("concurrent.futures")
        .unwrap_or_else(|| StaticStrings::EmptyString.into());
    let mut module = Module::new(module_name);

    module.set_attr_text(
        "as_completed",
        Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
            ConcurrentFuturesFunctions::AsCompleted,
        )),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "wait",
        Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(ConcurrentFuturesFunctions::Wait)),
        heap,
        interns,
    )?;

    let future_class = create_future_class(heap, interns)?;
    module.set_attr_text("Future", Value::Ref(future_class), heap, interns)?;

    let executor_class = create_executor_class(heap, interns)?;
    module.set_attr_text("ThreadPoolExecutor", Value::Ref(executor_class), heap, interns)?;
    heap.inc_ref(executor_class);
    module.set_attr_text("ProcessPoolExecutor", Value::Ref(executor_class), heap, interns)?;

    module.set_attr_text(
        "ALL_COMPLETED",
        Value::Ref(heap.allocate(HeapData::Str(Str::from("ALL_COMPLETED")))?),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "FIRST_COMPLETED",
        Value::Ref(heap.allocate(HeapData::Str(Str::from("FIRST_COMPLETED")))?),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "FIRST_EXCEPTION",
        Value::Ref(heap.allocate(HeapData::Str(Str::from("FIRST_EXCEPTION")))?),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ConcurrentFuturesFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        ConcurrentFuturesFunctions::AsCompleted => Ok(AttrCallResult::Value(as_completed(heap, interns, args)?)),
        ConcurrentFuturesFunctions::Wait => Ok(AttrCallResult::Value(wait(heap, interns, args)?)),

        ConcurrentFuturesFunctions::FutureInit => Ok(AttrCallResult::Value(future_init(heap, interns, args)?)),
        ConcurrentFuturesFunctions::FutureResult => future_result(heap, interns, args),
        ConcurrentFuturesFunctions::FutureException => {
            Ok(AttrCallResult::Value(future_exception(heap, interns, args)?))
        }
        ConcurrentFuturesFunctions::FutureDone => Ok(AttrCallResult::Value(future_done(heap, interns, args)?)),
        ConcurrentFuturesFunctions::FutureCancelled => {
            Ok(AttrCallResult::Value(future_cancelled(heap, interns, args)?))
        }
        ConcurrentFuturesFunctions::FutureAddDoneCallback => future_add_done_callback(heap, interns, args),
        ConcurrentFuturesFunctions::FutureCancel => Ok(AttrCallResult::Value(future_cancel(heap, interns, args)?)),

        ConcurrentFuturesFunctions::ExecutorInit => Ok(AttrCallResult::Value(executor_init(heap, args)?)),
        ConcurrentFuturesFunctions::ExecutorSubmit => Ok(AttrCallResult::Value(executor_submit(heap, interns, args)?)),
        ConcurrentFuturesFunctions::ExecutorMap => executor_map(heap, interns, args),
        ConcurrentFuturesFunctions::ExecutorShutdown => Ok(AttrCallResult::Value(executor_shutdown(heap, args)?)),
        ConcurrentFuturesFunctions::ExecutorEnter => Ok(AttrCallResult::Value(executor_enter(heap, args)?)),
        ConcurrentFuturesFunctions::ExecutorExit => Ok(AttrCallResult::Value(executor_exit(heap, args)?)),
    }
}

fn as_completed(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs_values) = args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    if positional.is_empty() {
        return Err(ExcType::type_error_at_least("as_completed", 1, 0));
    }

    let fs = positional.remove(0);
    positional.drop_with_heap(heap);
    let values = collect_iterable_values(fs, heap, interns)?;
    let list_id = heap.allocate(HeapData::List(List::new(values)))?;
    Ok(Value::Ref(list_id))
}

fn wait(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs_values) = args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    if positional.is_empty() {
        return Err(ExcType::type_error_at_least("wait", 1, 0));
    }

    let fs = positional.remove(0);
    positional.drop_with_heap(heap);
    let values = collect_iterable_values(fs, heap, interns)?;

    let done_set = vec_to_set(values, heap, interns)?;
    let not_done_set = vec_to_set(Vec::new(), heap, interns)?;
    Ok(allocate_tuple(SmallVec::from_vec(vec![done_set, not_done_set]), heap)?)
}

fn future_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.__init__")?;
    call_args.check_zero_args("Future.__init__", heap)?;

    set_instance_attr_by_name(self_id, FUTURE_DONE_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(self_id, FUTURE_CANCELLED_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(self_id, FUTURE_RESULT_ATTR, Value::None, heap, interns)?;
    set_instance_attr_by_name(self_id, FUTURE_EXCEPTION_ATTR, Value::None, heap, interns)?;
    set_instance_attr_by_name(self_id, FUTURE_CALLABLE_ATTR, Value::None, heap, interns)?;
    set_instance_attr_by_name(self_id, FUTURE_ARGS_ATTR, empty_tuple_value(heap)?, heap, interns)?;
    set_instance_attr_by_name(
        self_id,
        FUTURE_KWARGS_ATTR,
        Value::Ref(heap.allocate(HeapData::Dict(Dict::new()))?),
        heap,
        interns,
    )?;

    Ok(Value::None)
}

fn future_result(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.result")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    if get_instance_bool_attr(self_id, FUTURE_DONE_ATTR, heap, interns).unwrap_or(false) {
        let exception = get_instance_attr_by_name(self_id, FUTURE_EXCEPTION_ATTR, heap, interns).unwrap_or(Value::None);
        if !matches!(exception, Value::None) {
            let message = exception.py_str(heap, interns).into_owned();
            exception.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::RuntimeError, message).into());
        }
        return Ok(AttrCallResult::Value(
            get_instance_attr_by_name(self_id, FUTURE_RESULT_ATTR, heap, interns).unwrap_or(Value::None),
        ));
    }

    let callable = get_instance_attr_by_name(self_id, FUTURE_CALLABLE_ATTR, heap, interns).unwrap_or(Value::None);
    if matches!(callable, Value::None) {
        return Ok(AttrCallResult::Value(Value::None));
    }

    let args_value = get_instance_attr_by_name(self_id, FUTURE_ARGS_ATTR, heap, interns)
        .unwrap_or_else(|| empty_tuple_value(heap).unwrap_or(Value::None));
    let kwargs_value = get_instance_attr_by_name(self_id, FUTURE_KWARGS_ATTR, heap, interns).unwrap_or_else(|| {
        Value::Ref(
            heap.allocate(HeapData::Dict(Dict::new()))
                .expect("dict alloc should succeed"),
        )
    });
    let call_args = build_call_args_from_values(args_value, kwargs_value, heap, interns)?;

    Ok(AttrCallResult::CallFunction(callable, call_args))
}

fn future_exception(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.exception")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    Ok(get_instance_attr_by_name(self_id, FUTURE_EXCEPTION_ATTR, heap, interns).unwrap_or(Value::None))
}

fn future_done(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.done")?;
    call_args.check_zero_args("Future.done", heap)?;
    Ok(Value::Bool(
        get_instance_bool_attr(self_id, FUTURE_DONE_ATTR, heap, interns).unwrap_or(false),
    ))
}

fn future_cancelled(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.cancelled")?;
    call_args.check_zero_args("Future.cancelled", heap)?;
    Ok(Value::Bool(
        get_instance_bool_attr(self_id, FUTURE_CANCELLED_ATTR, heap, interns).unwrap_or(false),
    ))
}

fn future_add_done_callback(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.add_done_callback")?;
    let callback = call_args.get_one_arg("Future.add_done_callback", heap)?;

    if get_instance_bool_attr(self_id, FUTURE_DONE_ATTR, heap, interns).unwrap_or(false) {
        heap.inc_ref(self_id);
        return Ok(AttrCallResult::CallFunction(
            callback,
            ArgValues::One(Value::Ref(self_id)),
        ));
    }

    callback.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

fn future_cancel(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "Future.cancel")?;
    call_args.check_zero_args("Future.cancel", heap)?;

    if get_instance_bool_attr(self_id, FUTURE_DONE_ATTR, heap, interns).unwrap_or(false) {
        return Ok(Value::Bool(false));
    }

    set_instance_attr_by_name(self_id, FUTURE_DONE_ATTR, Value::Bool(true), heap, interns)?;
    set_instance_attr_by_name(self_id, FUTURE_CANCELLED_ATTR, Value::Bool(true), heap, interns)?;
    Ok(Value::Bool(true))
}

fn executor_init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (_self_id, call_args) = extract_instance_self_and_args(args, heap, "ThreadPoolExecutor.__init__")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    Ok(Value::None)
}

fn executor_submit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (_self_id, call_args) = extract_instance_self_and_args(args, heap, "ThreadPoolExecutor.submit")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();

    if positional.is_empty() {
        kwargs_values.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("ThreadPoolExecutor.submit", 1, 0));
    }

    let callable = positional.remove(0);
    let forward_args = positional;
    let forward_kwargs = kwargs_values;

    let future_id = create_future_instance(heap, interns)?;

    if is_sync_callable(&callable, heap) {
        let call_values = arg_values_from_parts(forward_args, forward_kwargs);
        match call_value_sync(callable, call_values, heap, interns) {
            Ok(result) => {
                set_instance_attr_by_name(future_id, FUTURE_RESULT_ATTR, result, heap, interns)?;
                set_instance_attr_by_name(future_id, FUTURE_EXCEPTION_ATTR, Value::None, heap, interns)?;
                set_instance_attr_by_name(future_id, FUTURE_DONE_ATTR, Value::Bool(true), heap, interns)?;
            }
            Err(err) => {
                let msg = Value::Ref(heap.allocate(HeapData::Str(Str::from(format!("{err:?}"))))?);
                set_instance_attr_by_name(future_id, FUTURE_EXCEPTION_ATTR, msg, heap, interns)?;
                set_instance_attr_by_name(future_id, FUTURE_RESULT_ATTR, Value::None, heap, interns)?;
                set_instance_attr_by_name(future_id, FUTURE_DONE_ATTR, Value::Bool(true), heap, interns)?;
            }
        }
    } else {
        let args_tuple = allocate_tuple(SmallVec::from_vec(forward_args), heap)?;
        let kwargs_dict = kwargs_to_dict(forward_kwargs, heap, interns)?;
        set_instance_attr_by_name(future_id, FUTURE_CALLABLE_ATTR, callable, heap, interns)?;
        set_instance_attr_by_name(future_id, FUTURE_ARGS_ATTR, args_tuple, heap, interns)?;
        set_instance_attr_by_name(future_id, FUTURE_KWARGS_ATTR, kwargs_dict, heap, interns)?;
        set_instance_attr_by_name(future_id, FUTURE_DONE_ATTR, Value::Bool(false), heap, interns)?;
    }

    Ok(Value::Ref(future_id))
}

fn executor_map(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_self_id, call_args) = extract_instance_self_and_args(args, heap, "ThreadPoolExecutor.map")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();

    let mut timeout_seen = false;
    for (key, value) in kwargs_values {
        let Some(key_str) = key.as_either_str(heap).map(|s| s.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);
        if key_str == "timeout" {
            if timeout_seen {
                value.drop_with_heap(heap);
                positional.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg("ThreadPoolExecutor.map", "timeout"));
            }
            timeout_seen = true;
            value.drop_with_heap(heap);
        } else {
            value.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(
                "ThreadPoolExecutor.map",
                &key_str,
            ));
        }
    }

    if positional.len() < 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("ThreadPoolExecutor.map", 2, count));
    }

    let func = positional.remove(0);
    let mut iterables = Vec::new();
    for iterable in positional {
        iterables.push(collect_iterable_values(iterable, heap, interns)?);
    }

    Ok(AttrCallResult::MapCall(func, iterables))
}

fn executor_shutdown(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (_self_id, call_args) = extract_instance_self_and_args(args, heap, "ThreadPoolExecutor.shutdown")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    Ok(Value::None)
}

fn executor_enter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (self_id, call_args) = extract_instance_self_and_args(args, heap, "ThreadPoolExecutor.__enter__")?;
    call_args.check_zero_args("ThreadPoolExecutor.__enter__", heap)?;
    heap.inc_ref(self_id);
    Ok(Value::Ref(self_id))
}

fn executor_exit(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (_self_id, call_args) = extract_instance_self_and_args(args, heap, "ThreadPoolExecutor.__exit__")?;
    let (pos_iter, kwargs_values) = call_args.into_parts();
    for value in pos_iter {
        value.drop_with_heap(heap);
    }
    for (key, value) in kwargs_values {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    Ok(Value::Bool(false))
}

fn create_future_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    create_helper_class(
        "concurrent.futures.Future",
        &[
            (
                "__init__",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureInit,
                )),
            ),
            (
                "result",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureResult,
                )),
            ),
            (
                "exception",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureException,
                )),
            ),
            (
                "done",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureDone,
                )),
            ),
            (
                "cancelled",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureCancelled,
                )),
            ),
            (
                "add_done_callback",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureAddDoneCallback,
                )),
            ),
            (
                "cancel",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::FutureCancel,
                )),
            ),
        ],
        heap,
        interns,
    )
}

fn create_executor_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    create_helper_class(
        "concurrent.futures.ThreadPoolExecutor",
        &[
            (
                "__init__",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::ExecutorInit,
                )),
            ),
            (
                "submit",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::ExecutorSubmit,
                )),
            ),
            (
                "map",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::ExecutorMap,
                )),
            ),
            (
                "shutdown",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::ExecutorShutdown,
                )),
            ),
            (
                "__enter__",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::ExecutorEnter,
                )),
            ),
            (
                "__exit__",
                Value::ModuleFunction(ModuleFunctions::ConcurrentFutures(
                    ConcurrentFuturesFunctions::ExecutorExit,
                )),
            ),
        ],
        heap,
        interns,
    )
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
        .expect("concurrent.futures helper class should always have valid MRO");
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

fn create_future_instance(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let future_class = create_future_class(heap, interns)?;
    let future_id = allocate_instance(future_class, heap)?;

    set_instance_attr_by_name(future_id, FUTURE_DONE_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(future_id, FUTURE_CANCELLED_ATTR, Value::Bool(false), heap, interns)?;
    set_instance_attr_by_name(future_id, FUTURE_RESULT_ATTR, Value::None, heap, interns)?;
    set_instance_attr_by_name(future_id, FUTURE_EXCEPTION_ATTR, Value::None, heap, interns)?;
    set_instance_attr_by_name(future_id, FUTURE_CALLABLE_ATTR, Value::None, heap, interns)?;
    set_instance_attr_by_name(future_id, FUTURE_ARGS_ATTR, empty_tuple_value(heap)?, heap, interns)?;
    set_instance_attr_by_name(
        future_id,
        FUTURE_KWARGS_ATTR,
        Value::Ref(heap.allocate(HeapData::Dict(Dict::new()))?),
        heap,
        interns,
    )?;

    Ok(future_id)
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

fn vec_to_set(values: Vec<Value>, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let list_id = heap.allocate(HeapData::List(List::new(values)))?;
    Type::Set.call(heap, ArgValues::One(Value::Ref(list_id)), interns)
}

fn kwargs_to_dict(kwargs: KwargsValues, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let mut out = Dict::new();
    for (key, value) in kwargs {
        if let Some(old) = out.set(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }
    Ok(Value::Ref(heap.allocate(HeapData::Dict(out))?))
}

fn collect_iterable_values(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let iter = OurosIter::new(iterable, heap, interns)?;
    defer_drop_mut!(iter, heap);
    let values: Vec<Value> = iter.collect(heap, interns)?;
    Ok(values)
}

fn empty_tuple_value(heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
    allocate_tuple(SmallVec::new(), heap)
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
    let self_id = if let Value::Ref(id) = &self_value {
        if matches!(heap.get(*id), HeapData::Instance(_)) {
            *id
        } else {
            self_value.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("{method_name} expected instance")));
        }
    } else {
        self_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("{method_name} expected instance")));
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

fn build_call_args_from_values(
    args_value: Value,
    kwargs_value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<ArgValues> {
    let positional = value_to_vec(args_value, heap, interns)?;
    let kwargs = dict_value_to_kwargs(kwargs_value, heap, interns)?;

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
    defer_drop!(value, heap);
    match value {
        Value::None => Ok(Vec::new()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => {
                let out = tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
                Ok(out)
            }
            HeapData::List(list) => {
                let out = list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
                Ok(out)
            }
            _ => {
                let iter = OurosIter::new(value.clone_with_heap(heap), heap, interns)?;
                defer_drop_mut!(iter, heap);
                let out: Vec<Value> = iter.collect(heap, interns)?;
                Ok(out)
            }
        },
        _ => {
            let iter = OurosIter::new(value.clone_with_heap(heap), heap, interns)?;
            defer_drop_mut!(iter, heap);
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
    defer_drop!(value, heap);
    let Value::Ref(dict_id) = value else {
        return Ok(KwargsValues::Empty);
    };

    let cloned = clone_dict(*dict_id, heap, interns)?;
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

fn is_sync_callable(callable: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match callable {
        Value::Builtin(_) | Value::ModuleFunction(_) => true,
        Value::Ref(id) => {
            heap.builtin_type_for_class_id(*id).is_some() || matches!(heap.get(*id), HeapData::BoundMethod(_))
        }
        _ => false,
    }
}

fn call_value_sync(
    callable: Value,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    defer_drop!(callable, heap);
    match callable {
        Value::Builtin(builtin) => {
            let mut print = NoPrint;
            builtin.call(heap, args, interns, &mut print)
        }
        Value::ModuleFunction(module_function) => match module_function.call(heap, interns, args)? {
            AttrCallResult::Value(value) => Ok(value),
            _ => Err(ExcType::type_error("concurrent.futures helper expected a value result")),
        },
        Value::Ref(heap_id) => {
            if let Some(builtin_type) = heap.builtin_type_for_class_id(*heap_id) {
                let mut print = NoPrint;
                return Builtins::Type(builtin_type).call(heap, args, interns, &mut print);
            }
            if matches!(heap.get(*heap_id), HeapData::BoundMethod(_)) {
                let (func, self_arg) = match heap.get(*heap_id) {
                    HeapData::BoundMethod(method) => (
                        method.func().clone_with_heap(heap),
                        method.self_arg().clone_with_heap(heap),
                    ),
                    _ => unreachable!("checked bound method variant"),
                };
                let (positional, kwargs) = args.into_parts();
                let mut forwarded = Vec::with_capacity(positional.len() + 1);
                forwarded.push(self_arg);
                forwarded.extend(positional);
                return call_value_sync(
                    func,
                    ArgValues::ArgsKargs {
                        args: forwarded,
                        kwargs,
                    },
                    heap,
                    interns,
                );
            }
            let ty = heap.get(*heap_id).py_type(heap);
            args.drop_with_heap(heap);
            Err(ExcType::type_error(format!("'{ty}' object is not callable")))
        }
        other => {
            args.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{}' object is not callable",
                other.py_type(heap)
            )))
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
            return Err(ExcType::type_error("concurrent.futures helper expected instance"));
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
