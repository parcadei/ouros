//! Implementation of the `callable()` builtin.
//!
//! Python defines `callable(obj)` as a fast capability check: it reports whether
//! the interpreter can invoke `obj(...)` without immediately raising
//! `'... object is not callable'`.

use crate::{
    args::ArgValues,
    exception_private::RunResult,
    heap::{Heap, HeapData, HeapId},
    intern::Interns,
    resource::ResourceTracker,
    value::Value,
};

/// Implementation of Python's `callable(obj)` builtin.
///
/// This check mirrors Ouros's runtime call dispatch:
/// - immediate callable value variants (`Builtin`, `DefFunction`, etc.) are callable
/// - callable heap wrappers (`BoundMethod`, `Partial`, `WeakRef`, etc.) are callable
/// - class objects are callable (instantiation)
/// - instances are callable only when their class MRO defines `__call__`
pub fn builtin_callable(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let value = args.get_one_arg("callable", heap)?;
    let callable = is_value_callable(&value, heap, interns);
    value.drop_with_heap(heap);
    Ok(Value::Bool(callable))
}

/// Returns whether a runtime `Value` is callable in Ouros's VM dispatch model.
fn is_value_callable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Marker(marker) => marker.is_callable(),
        Value::Ref(heap_id) => is_heap_value_callable(*heap_id, heap, interns),
        _ => false,
    }
}

/// Returns whether a specific heap object can be called.
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

/// Returns true if instances of `class_id` expose a `__call__` method in their MRO.
fn instance_is_callable(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
        return false;
    };
    class_obj.mro_has_attr("__call__", class_id, heap, interns)
}
