//! Implementation of the `atexit` module.
//!
//! This provides a sandbox-safe callback registry used by stdlib code that
//! expects `atexit.register()` / `atexit.unregister()`. Callback execution is
//! driven by `_run_exitfuncs()` which yields one callback invocation at a time
//! through normal VM call flow.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, ExitCallback, Module},
    value::Value,
};

/// Upper bound for callbacks to prevent unbounded memory/CPU abuse.
const MAX_ATEXIT_CALLBACKS: usize = 4096;

/// `atexit` module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum AtexitFunctions {
    Register,
    Unregister,
    #[strum(serialize = "_run_exitfuncs")]
    RunExitfuncs,
    #[strum(serialize = "_clear")]
    Clear,
    #[strum(serialize = "_ncallbacks")]
    Ncallbacks,
}

/// Creates the `atexit` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Atexit);
    module.set_attr_text(
        "register",
        Value::ModuleFunction(ModuleFunctions::Atexit(AtexitFunctions::Register)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "unregister",
        Value::ModuleFunction(ModuleFunctions::Atexit(AtexitFunctions::Unregister)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "_run_exitfuncs",
        Value::ModuleFunction(ModuleFunctions::Atexit(AtexitFunctions::RunExitfuncs)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "_clear",
        Value::ModuleFunction(ModuleFunctions::Atexit(AtexitFunctions::Clear)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "_ncallbacks",
        Value::ModuleFunction(ModuleFunctions::Atexit(AtexitFunctions::Ncallbacks)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to one of the `atexit` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: AtexitFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        AtexitFunctions::Register => register(heap, interns, args),
        AtexitFunctions::Unregister => unregister(heap, interns, args),
        AtexitFunctions::RunExitfuncs => run_exitfuncs(heap, interns, args),
        AtexitFunctions::Clear => clear(heap, args),
        AtexitFunctions::Ncallbacks => ncallbacks(heap, args),
    }
}

/// Implements `atexit.register(func, *args, **kwargs)`.
fn register(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let pos_len = positional.len();
    if pos_len < 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("register() takes at least 1 argument (0 given)"));
    }

    let func = positional.next().expect("len checked");
    if !is_value_callable(&func, heap, interns) {
        func.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("the first argument must be callable"));
    }

    let callback_args = positional.collect::<Vec<_>>();
    let callback_kwargs = kwargs.into_iter().collect::<Vec<_>>();
    if heap.atexit_callback_count() >= MAX_ATEXIT_CALLBACKS {
        func.drop_with_heap(heap);
        callback_args.drop_with_heap(heap);
        for (key, value) in callback_kwargs {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
        return Err(SimpleException::new_msg(
            ExcType::MemoryError,
            format!("atexit callback registry limit ({MAX_ATEXIT_CALLBACKS}) exceeded"),
        )
        .into());
    }

    let return_value = func.clone_with_heap(heap);
    heap.register_atexit_callback(ExitCallback::Callback {
        func,
        args: callback_args,
        kwargs: callback_kwargs,
    });
    Ok(AttrCallResult::Value(return_value))
}

/// Implements `atexit.unregister(func)`.
fn unregister(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let target = args.get_one_arg("atexit.unregister", heap)?;
    heap.unregister_atexit_callbacks(&target, interns);
    target.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `atexit._run_exitfuncs()`.
///
/// Each call executes one pending callback (LIFO). Repeated calls continue
/// draining until the registry is empty, then return `None`.
fn run_exitfuncs(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    args.check_zero_args("atexit._run_exitfuncs", heap)?;
    if let Some((func, callback_args)) = heap.take_pending_atexit_callback(interns)? {
        return Ok(AttrCallResult::CallFunction(func, callback_args));
    }
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `atexit._clear()`.
fn clear(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("atexit._clear", heap)?;
    heap.clear_atexit_callbacks();
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `atexit._ncallbacks()`.
fn ncallbacks(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("atexit._ncallbacks", heap)?;
    Ok(AttrCallResult::Value(Value::Int(
        i64::try_from(heap.atexit_callback_count()).expect("callback count should fit i64"),
    )))
}

/// Returns whether a runtime value can be invoked as a callable.
fn is_value_callable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Ref(heap_id) => is_heap_value_callable(*heap_id, heap, interns),
        _ => false,
    }
}

/// Returns whether a specific heap object is callable.
fn is_heap_value_callable(heap_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match heap.get(heap_id) {
        HeapData::ClassSubclasses(_)
        | HeapData::ClassGetItem(_)
        | HeapData::GenericAlias(_)
        | HeapData::FunctionGet(_)
        | HeapData::WeakRef(_)
        | HeapData::ClassObject(_)
        | HeapData::BoundMethod(_)
        | HeapData::Partial(_)
        | HeapData::SingleDispatch(_)
        | HeapData::SingleDispatchRegister(_)
        | HeapData::SingleDispatchMethod(_)
        | HeapData::CmpToKey(_)
        | HeapData::ItemGetter(_)
        | HeapData::AttrGetter(_)
        | HeapData::MethodCaller(_)
        | HeapData::PropertyAccessor(_)
        | HeapData::Closure(_, _, _)
        | HeapData::FunctionDefaults(_, _)
        | HeapData::ObjectNewImpl(_) => true,
        HeapData::Instance(instance) => instance_is_callable(instance.class_id(), heap, interns),
        _ => false,
    }
}

/// Returns whether instances of `class_id` expose `__call__` in MRO.
fn instance_is_callable(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
        return false;
    };
    class_obj.mro_has_attr("__call__", class_id, heap, interns)
}
