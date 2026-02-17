//! Implementation of the `aiter()` builtin.
//!
//! Python 3.10 introduced `aiter(async_iterable)` as the async counterpart to
//! `iter(...)`. Ouros currently models async-generator behavior with lightweight
//! runtime wrappers, so this builtin normalizes supported async iterables into
//! an async-generator facade.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{Heap, HeapData},
    resource::ResourceTracker,
    types::{PyTrait, StdlibObject},
    value::Value,
};

/// Implements Python's `aiter(async_iterable)` builtin.
///
/// For generator-backed async iterables, Ouros returns a dedicated facade object
/// whose runtime type is `async_generator` for parity with CPython's
/// `type(aiter(...))` behavior in parity tests.
pub fn builtin_aiter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let iterable = args.get_one_arg("aiter", heap)?;
    match iterable {
        Value::Ref(id) => {
            let is_generator = matches!(heap.get(id), HeapData::Generator(_));
            let is_async_generator = matches!(heap.get(id), HeapData::StdlibObject(StdlibObject::AsyncGenerator(_)));
            if is_generator {
                let facade_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_async_generator(
                    Value::Ref(id),
                )))?;
                return Ok(Value::Ref(facade_id));
            }
            if is_async_generator {
                return Ok(Value::Ref(id));
            }
            let type_name = heap.get(id).py_type(heap);
            Value::Ref(id).drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{type_name}' object is not an async iterable"
            )))
        }
        value => {
            let type_name = value.py_type(heap);
            value.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{type_name}' object is not an async iterable"
            )))
        }
    }
}
