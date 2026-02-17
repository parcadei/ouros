//! Implementation of the `ascii()` builtin.
//!
//! `ascii(obj)` is equivalent to `repr(obj)` except non-ASCII code points are
//! escaped using `\\x`, `\\u`, or `\\U` escape sequences.

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::RunResult,
    fstring::ascii_escape,
    heap::{Heap, HeapData},
    intern::Interns,
    resource::ResourceTracker,
    types::{PyTrait, Str},
    value::Value,
};

/// Implements Python's `ascii(obj)` builtin.
pub fn builtin_ascii(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let value = args.get_one_arg("ascii", heap)?;
    defer_drop!(value, heap);
    let escaped = ascii_escape(&value.py_repr(heap, interns));
    let heap_id = heap.allocate(HeapData::Str(Str::from(escaped)))?;
    Ok(Value::Ref(heap_id))
}
