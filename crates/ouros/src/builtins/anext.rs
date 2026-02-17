//! Implementation of the `anext()` builtin.
//!
//! Python 3.10 introduced `anext(async_iterator[, default])`, returning an
//! awaitable object that yields the next item from an async iterator. Ouros
//! exposes this as a lightweight awaitable wrapper object.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{Heap, HeapData},
    resource::ResourceTracker,
    types::{PyTrait, StdlibObject},
    value::Value,
};

/// Implements Python's `anext(async_iterator[, default])` builtin.
///
/// The returned value is an awaitable wrapper object with CPython-like repr
/// shape (`<anext_awaitable object at 0x...>`).
pub fn builtin_anext(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (iterator, default) = args.get_one_two_args("anext", heap)?;
    let iterator = validate_async_iterator(iterator, heap)?;
    let awaitable_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_anext_awaitable(
        iterator, default,
    )))?;
    Ok(Value::Ref(awaitable_id))
}

/// Validates that `value` is an async-iterator-compatible runtime object.
fn validate_async_iterator(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    match value {
        Value::Ref(id) => {
            let is_generator = matches!(heap.get(id), HeapData::Generator(_));
            let is_async_generator = matches!(heap.get(id), HeapData::StdlibObject(StdlibObject::AsyncGenerator(_)));
            if is_generator || is_async_generator {
                Ok(Value::Ref(id))
            } else {
                let type_name = heap.get(id).py_type(heap);
                Value::Ref(id).drop_with_heap(heap);
                Err(ExcType::type_error(format!(
                    "'{type_name}' object is not an async iterator"
                )))
            }
        }
        other => {
            let type_name = other.py_type(heap);
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{type_name}' object is not an async iterator"
            )))
        }
    }
}
