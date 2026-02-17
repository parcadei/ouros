//! Implementation of the `heapq` module.
//!
//! Provides heap queue (priority queue) operations on lists.
//! This is a min-heap implementation where `heap[0]` is always the smallest element.
//!
//! # Functions
//! - `heappush(heap, item)`: Push item onto heap, maintaining heap property
//! - `heappop(heap)`: Pop and return the smallest item from the heap
//! - `heapify(x)`: Transform list into a heap in-place (in O(n) time)
//! - `heappush_max(heap, item)`: Push item onto max-heap, maintaining heap property
//! - `heappop_max(heap)`: Pop and return the largest item from the max-heap
//! - `heapify_max(x)`: Transform list into a max-heap in-place
//! - `nlargest(n, iterable)`: Return the n largest elements
//! - `nsmallest(n, iterable)`: Return the n smallest elements
//! - `merge(*iterables, key=None, reverse=False)`: Merge sorted iterables (returns list)

use std::cmp::Ordering;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    exception_public::Exception,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::PrintWriter,
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, List, OurosIter, PyTrait},
    value::Value,
};

/// Dummy PrintWriter for calling builtin key functions in `heapq.merge()`.
struct DummyPrint;

impl PrintWriter for DummyPrint {
    fn stdout_write(&mut self, _output: std::borrow::Cow<'_, str>) -> Result<(), Exception> {
        Ok(())
    }

    fn stdout_push(&mut self, _end: char) -> Result<(), Exception> {
        Ok(())
    }
}

/// Heapq module functions.
///
/// Each variant maps to a function in Python's `heapq` module for heap queue operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum HeapqFunctions {
    Heappush,
    Heappop,
    Heapify,
    HeappushMax,
    HeappopMax,
    HeapifyMax,
    HeappushpopMax,
    HeapreplaceMax,
    Nlargest,
    Nsmallest,
    Heappushpop,
    Heapreplace,
    Merge,
}

/// Creates the `heapq` module and allocates it on the heap.
///
/// The module provides heap queue (priority queue) operations on lists.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Heapq);

    module.set_attr(
        StaticStrings::HqHeappush,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Heappush)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeappop,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Heappop)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeapify,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Heapify)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeappushMax,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::HeappushMax)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeappopMax,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::HeappopMax)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeapifyMax,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::HeapifyMax)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqNlargest,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Nlargest)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqNsmallest,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Nsmallest)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeappushpop,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Heappushpop)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeappushpopMax,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::HeappushpopMax)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeapreplace,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Heapreplace)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqHeapreplaceMax,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::HeapreplaceMax)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::HqMerge,
        Value::ModuleFunction(ModuleFunctions::Heapq(HeapqFunctions::Merge)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a heapq module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: HeapqFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        HeapqFunctions::Heappush => heappush(heap, interns, args),
        HeapqFunctions::Heappop => heappop(heap, interns, args),
        HeapqFunctions::Heapify => heapify(heap, interns, args),
        HeapqFunctions::HeappushMax => heappush_max(heap, interns, args),
        HeapqFunctions::HeappopMax => heappop_max(heap, interns, args),
        HeapqFunctions::HeapifyMax => heapify_max(heap, interns, args),
        HeapqFunctions::HeappushpopMax => heappushpop_max(heap, interns, args),
        HeapqFunctions::HeapreplaceMax => heapreplace_max(heap, interns, args),
        HeapqFunctions::Nlargest => nlargest(heap, interns, args),
        HeapqFunctions::Nsmallest => nsmallest(heap, interns, args),
        HeapqFunctions::Heappushpop => heappushpop(heap, interns, args),
        HeapqFunctions::Heapreplace => heapreplace(heap, interns, args),
        HeapqFunctions::Merge => merge(heap, interns, args),
    }
}

/// Implementation of `heapq.heappush(heap, item)`.
///
/// Push item onto heap, maintaining the heap invariant.
/// The heap is a list that is modified in-place.
///
/// # Arguments
/// * `heap` - The heap for any allocations
/// * `interns` - The interner for string lookups
/// * `args` - Function arguments: `heap` (list), `item` (value)
///
/// # Returns
/// `AttrCallResult::Value` containing `None`.
///
/// # Errors
/// Returns `TypeError` if:
/// - The wrong number of arguments is provided
/// - The first argument is not a list
fn heappush(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (heap_arg, item) = args.get_two_args("heappush", heap)?;

    // Get the list from the heap argument
    let Value::Ref(list_id) = heap_arg else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "heappush() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    // Verify it's a list and append the item
    if let HeapData::List(_) = heap.get(list_id) {
        // Append the item to the list
        heap.with_entry_mut(list_id, |heap_inner, data| {
            if let HeapData::List(list) = data {
                list.append(heap_inner, item);
            }
        });
        // Sift up to maintain heap invariant
        sift_up(heap, interns, list_id);
        // Drop the heap_arg (the list reference)
        heap_arg.drop_with_heap(heap);
        Ok(AttrCallResult::Value(Value::None))
    } else {
        item.drop_with_heap(heap);
        Err(ExcType::type_error(format!(
            "heappush() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )))
    }
}

/// Sifts a value up the heap to maintain the heap invariant.
///
/// This is called after appending a new item to the heap.
/// The item at the end of the list is moved up until the heap property
/// is restored (parent <= children for min-heap).
fn sift_up(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId) {
    // Get the list length
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return,
    };

    if len == 0 {
        return;
    }

    // Start from the last element and move it up
    let mut idx = len - 1;

    while idx > 0 {
        let parent_idx = (idx - 1) / 2;

        // Get the values at current index and parent
        let (current_val, parent_val) = {
            let HeapData::List(list) = heap.get(list_id) else {
                return;
            };
            let vec = list.as_vec();
            if idx >= vec.len() || parent_idx >= vec.len() {
                return;
            }
            (vec[idx].clone_with_heap(heap), vec[parent_idx].clone_with_heap(heap))
        };

        // Use defer_drop! to ensure values are properly cleaned up
        crate::defer_drop!(current_val, heap);
        crate::defer_drop!(parent_val, heap);

        // Compare: if current < parent, swap them
        match current_val.py_cmp(parent_val, heap, interns) {
            Some(std::cmp::Ordering::Less) => {
                // Swap the values
                heap.with_entry_mut(list_id, |_, data| {
                    if let HeapData::List(list) = data {
                        let vec = list.as_vec_mut();
                        vec.swap(idx, parent_idx);
                    }
                });
                idx = parent_idx;
            }
            _ => break,
        }
    }
}

/// Implementation of `heapq.heappop(heap)`.
///
/// Pop and return the smallest item from the heap, maintaining the heap invariant.
///
/// # Arguments
/// * `heap` - The heap for any allocations
/// * `interns` - The interner for string lookups
/// * `args` - Function arguments: `heap` (list)
///
/// # Returns
/// `AttrCallResult::Value` containing the smallest item.
///
/// # Errors
/// Returns `TypeError` if the argument is not a list.
/// Returns `IndexError` if the heap is empty.
fn heappop(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let heap_arg = args.get_one_arg("heappop", heap)?;

    // Get the list from the heap argument
    let Value::Ref(list_id) = heap_arg else {
        return Err(ExcType::type_error(format!(
            "heappop() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    // Verify it's a list and pop the smallest item
    match heap.get(list_id) {
        HeapData::List(list) => {
            if list.len() == 0 {
                return Err(SimpleException::new_msg(ExcType::IndexError, "index out of range").into());
            }
            let result = pop_heap(heap, interns, list_id);
            // Drop the heap_arg (the list reference)
            heap_arg.drop_with_heap(heap);
            Ok(AttrCallResult::Value(result))
        }
        _ => Err(ExcType::type_error(format!(
            "heappop() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        ))),
    }
}

/// Pops the smallest item from the heap and restores the heap invariant.
///
/// This swaps the first and last elements, removes the last (which was the smallest),
/// and then sifts down the new root to restore the heap property.
fn pop_heap(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId) -> Value {
    // Get the list length
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return Value::None,
    };

    if len == 0 {
        return Value::None;
    }

    if len == 1 {
        // Just remove and return the only element
        return heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                let vec = list.as_vec_mut();
                vec.pop().unwrap_or(Value::None)
            } else {
                Value::None
            }
        });
    }

    // Swap first and last, then pop
    let last_val = heap.with_entry_mut(list_id, |_, data| {
        if let HeapData::List(list) = data {
            let vec = list.as_vec_mut();
            vec.swap(0, len - 1);
            vec.pop()
        } else {
            None
        }
    });

    // Sift down from root to restore heap property
    sift_down(heap, interns, list_id, 0);

    last_val.unwrap_or(Value::None)
}

/// Sifts a value down the heap to maintain the heap invariant.
///
/// This is called after popping an item from the heap.
/// The item at the root is moved down until the heap property
/// is restored (parent <= children for min-heap).
fn sift_down(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId, start_idx: usize) {
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return,
    };

    if len == 0 {
        return;
    }

    let mut idx = start_idx;

    loop {
        let left_child = 2 * idx + 1;
        let right_child = 2 * idx + 2;

        if left_child >= len {
            break; // No children
        }

        // Find the smallest child
        let smallest_child = if right_child < len {
            let (left_val, right_val) = {
                let HeapData::List(list) = heap.get(list_id) else {
                    return;
                };
                let vec = list.as_vec();
                (
                    vec[left_child].clone_with_heap(heap),
                    vec[right_child].clone_with_heap(heap),
                )
            };
            crate::defer_drop!(left_val, heap);
            crate::defer_drop!(right_val, heap);

            match left_val.py_cmp(right_val, heap, interns) {
                Some(std::cmp::Ordering::Greater) => right_child,
                _ => left_child,
            }
        } else {
            left_child
        };

        // Compare current with smallest child
        let (current_val, child_val) = {
            let HeapData::List(list) = heap.get(list_id) else {
                return;
            };
            let vec = list.as_vec();
            (
                vec[idx].clone_with_heap(heap),
                vec[smallest_child].clone_with_heap(heap),
            )
        };
        crate::defer_drop!(current_val, heap);
        crate::defer_drop!(child_val, heap);

        match current_val.py_cmp(child_val, heap, interns) {
            Some(std::cmp::Ordering::Greater) => {
                // Swap with smallest child
                heap.with_entry_mut(list_id, |_, data| {
                    if let HeapData::List(list) = data {
                        let vec = list.as_vec_mut();
                        vec.swap(idx, smallest_child);
                    }
                });
                idx = smallest_child;
            }
            _ => break,
        }
    }
}

/// Implementation of `heapq.heapify(x)`.
///
/// Transform list into a heap in-place, in O(n) time.
///
/// # Arguments
/// * `heap` - The heap for any allocations
/// * `interns` - The interner for string lookups
/// * `args` - Function arguments: `x` (list)
///
/// # Returns
/// `AttrCallResult::Value` containing `None`.
///
/// # Errors
/// Returns `TypeError` if the argument is not a list.
fn heapify(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let heap_arg = args.get_one_arg("heapify", heap)?;

    // Get the list from the heap argument
    let Value::Ref(list_id) = heap_arg else {
        return Err(ExcType::type_error(format!(
            "heapify() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    // Verify it's a list
    match heap.get(list_id) {
        HeapData::List(list) => {
            let len = list.len();
            if len > 1 {
                // Start from the last non-leaf node and sift down
                // Last non-leaf node is at (len - 2) / 2
                for i in (0..=(len - 2) / 2).rev() {
                    sift_down(heap, interns, list_id, i);
                }
            }
            // Drop the heap_arg (the list reference)
            heap_arg.drop_with_heap(heap);
            Ok(AttrCallResult::Value(Value::None))
        }
        _ => Err(ExcType::type_error(format!(
            "heapify() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        ))),
    }
}

/// Implementation of `heapq.heappush_max(heap, item)`.
///
/// Push item onto max-heap, maintaining the max-heap invariant.
fn heappush_max(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (heap_arg, item) = args.get_two_args("heappush_max", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "heappush_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    if let HeapData::List(_) = heap.get(list_id) {
        heap.with_entry_mut(list_id, |heap_inner, data| {
            if let HeapData::List(list) = data {
                list.append(heap_inner, item);
            }
        });
        sift_up_max(heap, interns, list_id);
        heap_arg.drop_with_heap(heap);
        Ok(AttrCallResult::Value(Value::None))
    } else {
        item.drop_with_heap(heap);
        Err(ExcType::type_error(format!(
            "heappush_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )))
    }
}

/// Implementation of `heapq.heappop_max(heap)`.
///
/// Pop and return the largest item from the max-heap.
fn heappop_max(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let heap_arg = args.get_one_arg("heappop_max", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        return Err(ExcType::type_error(format!(
            "heappop_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    match heap.get(list_id) {
        HeapData::List(list) => {
            if list.len() == 0 {
                return Err(SimpleException::new_msg(ExcType::IndexError, "index out of range").into());
            }
            let result = pop_heap_max(heap, interns, list_id);
            heap_arg.drop_with_heap(heap);
            Ok(AttrCallResult::Value(result))
        }
        _ => Err(ExcType::type_error(format!(
            "heappop_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        ))),
    }
}

/// Implementation of `heapq.heapify_max(x)`.
///
/// Transform list into a max-heap in-place, in O(n) time.
fn heapify_max(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let heap_arg = args.get_one_arg("heapify_max", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        return Err(ExcType::type_error(format!(
            "heapify_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    match heap.get(list_id) {
        HeapData::List(list) => {
            let len = list.len();
            if len > 1 {
                for i in (0..=(len - 2) / 2).rev() {
                    sift_down_max(heap, interns, list_id, i);
                }
            }
            heap_arg.drop_with_heap(heap);
            Ok(AttrCallResult::Value(Value::None))
        }
        _ => Err(ExcType::type_error(format!(
            "heapify_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        ))),
    }
}

/// Implementation of `heapq.heappushpop_max(heap, item)`.
///
/// Push item on max-heap, then pop and return the largest item.
fn heappushpop_max(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (heap_arg, item) = args.get_two_args("heappushpop_max", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "heappushpop_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    if let HeapData::List(list) = heap.get(list_id) {
        if list.len() == 0 {
            heap_arg.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(item));
        }

        let largest = list.as_vec()[0].clone_with_heap(heap);
        let should_push = {
            crate::defer_drop!(largest, heap);
            matches!(item.py_cmp(largest, heap, interns), Some(std::cmp::Ordering::Less))
        };

        if !should_push {
            heap_arg.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(item));
        }

        let old_largest = heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                let vec = list.as_vec_mut();
                let old = vec[0].copy_for_extend();
                vec[0] = item;
                old
            } else {
                Value::None
            }
        });

        sift_down_max(heap, interns, list_id, 0);
        heap_arg.drop_with_heap(heap);
        Ok(AttrCallResult::Value(old_largest))
    } else {
        item.drop_with_heap(heap);
        Err(ExcType::type_error(format!(
            "heappushpop_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )))
    }
}

/// Implementation of `heapq.heapreplace_max(heap, item)`.
///
/// Pop and return the largest item from max-heap, then push the new item.
fn heapreplace_max(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (heap_arg, item) = args.get_two_args("heapreplace_max", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "heapreplace_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    if let HeapData::List(list) = heap.get(list_id) {
        if list.len() == 0 {
            item.drop_with_heap(heap);
            heap_arg.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::IndexError, "index out of range").into());
        }

        let old_largest = heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                let vec = list.as_vec_mut();
                let old = vec[0].copy_for_extend();
                vec[0] = item;
                old
            } else {
                Value::None
            }
        });

        sift_down_max(heap, interns, list_id, 0);
        heap_arg.drop_with_heap(heap);
        Ok(AttrCallResult::Value(old_largest))
    } else {
        item.drop_with_heap(heap);
        Err(ExcType::type_error(format!(
            "heapreplace_max() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )))
    }
}

/// Sifts a value up the max-heap to maintain heap invariant.
fn sift_up_max(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId) {
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

        let (current_val, parent_val) = {
            let HeapData::List(list) = heap.get(list_id) else {
                return;
            };
            let vec = list.as_vec();
            if idx >= vec.len() || parent_idx >= vec.len() {
                return;
            }
            (vec[idx].clone_with_heap(heap), vec[parent_idx].clone_with_heap(heap))
        };

        crate::defer_drop!(current_val, heap);
        crate::defer_drop!(parent_val, heap);

        match current_val.py_cmp(parent_val, heap, interns) {
            Some(std::cmp::Ordering::Greater) => {
                heap.with_entry_mut(list_id, |_, data| {
                    if let HeapData::List(list) = data {
                        let vec = list.as_vec_mut();
                        vec.swap(idx, parent_idx);
                    }
                });
                idx = parent_idx;
            }
            _ => break,
        }
    }
}

/// Pops the largest item from a max-heap and restores heap invariant.
fn pop_heap_max(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId) -> Value {
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return Value::None,
    };

    if len == 0 {
        return Value::None;
    }

    if len == 1 {
        return heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                let vec = list.as_vec_mut();
                vec.pop().unwrap_or(Value::None)
            } else {
                Value::None
            }
        });
    }

    let last_val = heap.with_entry_mut(list_id, |_, data| {
        if let HeapData::List(list) = data {
            let vec = list.as_vec_mut();
            vec.swap(0, len - 1);
            vec.pop()
        } else {
            None
        }
    });

    sift_down_max(heap, interns, list_id, 0);

    last_val.unwrap_or(Value::None)
}

/// Sifts a value down the max-heap to maintain heap invariant.
fn sift_down_max(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, list_id: HeapId, start_idx: usize) {
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => return,
    };

    if len == 0 {
        return;
    }

    let mut idx = start_idx;

    loop {
        let left_child = 2 * idx + 1;
        let right_child = 2 * idx + 2;

        if left_child >= len {
            break;
        }

        let largest_child = if right_child < len {
            let (left_val, right_val) = {
                let HeapData::List(list) = heap.get(list_id) else {
                    return;
                };
                let vec = list.as_vec();
                (
                    vec[left_child].clone_with_heap(heap),
                    vec[right_child].clone_with_heap(heap),
                )
            };
            crate::defer_drop!(left_val, heap);
            crate::defer_drop!(right_val, heap);

            match left_val.py_cmp(right_val, heap, interns) {
                Some(std::cmp::Ordering::Less) => right_child,
                _ => left_child,
            }
        } else {
            left_child
        };

        let (current_val, child_val) = {
            let HeapData::List(list) = heap.get(list_id) else {
                return;
            };
            let vec = list.as_vec();
            (vec[idx].clone_with_heap(heap), vec[largest_child].clone_with_heap(heap))
        };
        crate::defer_drop!(current_val, heap);
        crate::defer_drop!(child_val, heap);

        match current_val.py_cmp(child_val, heap, interns) {
            Some(std::cmp::Ordering::Less) => {
                heap.with_entry_mut(list_id, |_, data| {
                    if let HeapData::List(list) = data {
                        let vec = list.as_vec_mut();
                        vec.swap(idx, largest_child);
                    }
                });
                idx = largest_child;
            }
            _ => break,
        }
    }
}

/// Implementation of `heapq.nlargest(n, iterable, key=None)`.
///
/// Return a list with the n largest elements from the dataset defined by iterable.
/// The returned list is sorted in descending order.
///
/// # Arguments
/// * `heap` - The heap for any allocations
/// * `interns` - The interner for string lookups
/// * `args` - Function arguments: `n` (int), `iterable` (list or other iterable), `key` (optional callable)
///
/// # Returns
/// `AttrCallResult::Value` containing the list of n largest elements.
///
/// # Errors
/// Returns `TypeError` if arguments are invalid.
#[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn nlargest(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // Handle both Two(a, b) and ArgsKargs cases for positional args
    let (n_arg, iterable_arg, kwargs) = match args {
        ArgValues::Two(a, b) => (a, b, KwargsValues::Empty),
        ArgValues::ArgsKargs { args, kwargs } => {
            let mut iter = args.into_iter();
            let a = iter
                .next()
                .ok_or_else(|| ExcType::type_error("nlargest() missing required argument: 'n'"))?;
            let b = iter
                .next()
                .ok_or_else(|| ExcType::type_error("nlargest() missing required argument: 'iterable'"))?;
            // Drop any extra positional arguments
            for extra in iter {
                extra.drop_with_heap(heap);
            }
            (a, b, kwargs)
        }
        other => {
            other.drop_with_heap(heap);
            return Err(ExcType::type_error("nlargest() takes exactly 2 positional arguments"));
        }
    };
    defer_drop!(n_arg, heap);

    // Parse key keyword argument
    let mut key_fn: Option<Value> = None;
    for (kw_key, kw_value) in kwargs {
        defer_drop!(kw_key, heap);

        let Some(keyword_name) = kw_key.as_either_str(heap) else {
            kw_value.drop_with_heap(heap);
            iterable_arg.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);
        if key_str == "key" {
            if matches!(kw_value, Value::None) {
                // None means no key function
                kw_value.drop_with_heap(heap);
            } else {
                key_fn = Some(kw_value);
            }
        } else {
            kw_value.drop_with_heap(heap);
            iterable_arg.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_str}' is an invalid keyword argument for nlargest()"
            )));
        }
    }

    // Get n
    let n = n_arg.as_int(heap)?;

    if n < 0 {
        iterable_arg.drop_with_heap(heap);
        if let Some(k) = key_fn {
            k.drop_with_heap(heap);
        }
        return Ok(AttrCallResult::Value(create_empty_list(heap)?));
    }

    let n = n as usize;

    // Collect all items from the iterable
    let items = collect_iterable(heap, interns, iterable_arg)?;

    // Compute key values if key function provided
    let keys = if let Some(ref key) = key_fn {
        let mut keys_vec = Vec::with_capacity(items.len());
        for item in &items {
            let elem = item.clone_with_heap(heap);
            match call_nsmallest_nlargest_key(key, elem, heap, interns) {
                Ok(key_value) => keys_vec.push(key_value),
                Err(e) => {
                    // Clean up on error
                    for k in keys_vec {
                        k.drop_with_heap(heap);
                    }
                    if let Some(k) = key_fn {
                        k.drop_with_heap(heap);
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    return Err(e);
                }
            }
        }
        if let Some(k) = key_fn {
            k.drop_with_heap(heap);
        }
        Some(keys_vec)
    } else {
        None
    };

    // Sort in descending order and take n, using keys for comparison
    let mut sorted_items = items;
    if let Some(k) = keys {
        // Sort using keys: sort indices first
        let mut indices: Vec<usize> = (0..sorted_items.len()).collect();
        indices.sort_by(|&a, &b| compare_values(&k[b], &k[a], heap, interns));
        indices.truncate(n);

        // Reorder items based on sorted indices
        let mut result: Vec<Value> = Vec::with_capacity(n);
        for &idx in &indices {
            // Use copy_for_extend because we're keeping the original item
            result.push(sorted_items[idx].copy_for_extend());
        }

        // Drop keys and original items
        for key in k {
            key.drop_with_heap(heap);
        }
        for item in sorted_items {
            item.drop_with_heap(heap);
        }

        return Ok(AttrCallResult::Value(create_list_from_items(heap, result)?));
    }

    sorted_items.sort_by(|a, b| compare_values(b, a, heap, interns));
    sorted_items.truncate(n);

    Ok(AttrCallResult::Value(create_list_from_items(heap, sorted_items)?))
}

/// Implementation of `heapq.nsmallest(n, iterable, key=None)`.
///
/// Return a list with the n smallest elements from the dataset defined by iterable.
/// The returned list is sorted in ascending order.
///
/// # Arguments
/// * `heap` - The heap for any allocations
/// * `interns` - The interner for string lookups
/// * `args` - Function arguments: `n` (int), `iterable` (list or other iterable), `key` (optional callable)
///
/// # Returns
/// `AttrCallResult::Value` containing the list of n smallest elements.
///
/// # Errors
/// Returns `TypeError` if arguments are invalid.
#[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn nsmallest(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // Handle both Two(a, b) and ArgsKargs cases for positional args
    let (n_arg, iterable_arg, kwargs) = match args {
        ArgValues::Two(a, b) => (a, b, KwargsValues::Empty),
        ArgValues::ArgsKargs { args, kwargs } => {
            let mut iter = args.into_iter();
            let a = iter
                .next()
                .ok_or_else(|| ExcType::type_error("nsmallest() missing required argument: 'n'"))?;
            let b = iter
                .next()
                .ok_or_else(|| ExcType::type_error("nsmallest() missing required argument: 'iterable'"))?;
            // Drop any extra positional arguments
            for extra in iter {
                extra.drop_with_heap(heap);
            }
            (a, b, kwargs)
        }
        other => {
            other.drop_with_heap(heap);
            return Err(ExcType::type_error("nsmallest() takes exactly 2 positional arguments"));
        }
    };
    defer_drop!(n_arg, heap);

    // Parse key keyword argument
    let mut key_fn: Option<Value> = None;
    for (kw_key, kw_value) in kwargs {
        defer_drop!(kw_key, heap);

        let Some(keyword_name) = kw_key.as_either_str(heap) else {
            kw_value.drop_with_heap(heap);
            iterable_arg.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);
        if key_str == "key" {
            if matches!(kw_value, Value::None) {
                // None means no key function
                kw_value.drop_with_heap(heap);
            } else {
                key_fn = Some(kw_value);
            }
        } else {
            kw_value.drop_with_heap(heap);
            iterable_arg.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_str}' is an invalid keyword argument for nsmallest()"
            )));
        }
    }

    // Get n
    let n = n_arg.as_int(heap)?;

    if n < 0 {
        iterable_arg.drop_with_heap(heap);
        if let Some(k) = key_fn {
            k.drop_with_heap(heap);
        }
        return Ok(AttrCallResult::Value(create_empty_list(heap)?));
    }

    let n = n as usize;

    // Collect all items from the iterable
    let items = collect_iterable(heap, interns, iterable_arg)?;

    // Compute key values if key function provided
    let keys = if let Some(ref key) = key_fn {
        let mut keys_vec = Vec::with_capacity(items.len());
        for item in &items {
            let elem = item.clone_with_heap(heap);
            match call_nsmallest_nlargest_key(key, elem, heap, interns) {
                Ok(key_value) => keys_vec.push(key_value),
                Err(e) => {
                    // Clean up on error
                    for k in keys_vec {
                        k.drop_with_heap(heap);
                    }
                    if let Some(k) = key_fn {
                        k.drop_with_heap(heap);
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    return Err(e);
                }
            }
        }
        if let Some(k) = key_fn {
            k.drop_with_heap(heap);
        }
        Some(keys_vec)
    } else {
        None
    };

    // Sort and take n smallest, using keys for comparison
    let mut sorted_items = items;
    if let Some(k) = keys {
        // Sort using keys: sort indices first
        let mut indices: Vec<usize> = (0..sorted_items.len()).collect();
        indices.sort_by(|&a, &b| compare_values(&k[a], &k[b], heap, interns));
        indices.truncate(n);

        // Reorder items based on sorted indices
        let mut result: Vec<Value> = Vec::with_capacity(n);
        for &idx in &indices {
            // Use copy_for_extend because we're keeping the original item
            result.push(sorted_items[idx].copy_for_extend());
        }

        // Drop keys and original items
        for key in k {
            key.drop_with_heap(heap);
        }
        for item in sorted_items {
            item.drop_with_heap(heap);
        }

        return Ok(AttrCallResult::Value(create_list_from_items(heap, result)?));
    }

    sorted_items.sort_by(|a, b| compare_values(a, b, heap, interns));
    sorted_items.truncate(n);

    Ok(AttrCallResult::Value(create_list_from_items(heap, sorted_items)?))
}

/// Implementation of `heapq.heappushpop(heap, item)`.
///
/// Push item onto the heap, then pop and return the smallest item.
/// This is more efficient than calling `heappush()` followed by `heappop()`,
/// and can be appropriate when the heap size is fixed.
///
/// The returned value may be larger than the item pushed. If that's not desired,
/// consider using `heapreplace()` instead.
///
/// # Errors
/// Returns `TypeError` if the first argument is not a list, or if the wrong number
/// of arguments is provided.
fn heappushpop(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (heap_arg, item) = args.get_two_args("heappushpop", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "heappushpop() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    if let HeapData::List(list) = heap.get(list_id) {
        if list.len() == 0 {
            // Empty heap: just return the item without modifying the heap
            heap_arg.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(item));
        }

        // Compare item with current smallest (heap[0])
        let smallest = list.as_vec()[0].clone_with_heap(heap);
        let should_push = {
            crate::defer_drop!(smallest, heap);
            // If item <= heap[0], just return item without modifying heap
            !matches!(
                item.py_cmp(smallest, heap, interns),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
        };

        if !should_push {
            heap_arg.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(item));
        }

        // Replace heap[0] with item and sift down.
        // Use copy_for_extend() because we're taking the value out of the vec
        // and replacing it — the old value keeps its existing refcount.
        let old_smallest = heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                let vec = list.as_vec_mut();
                let old = vec[0].copy_for_extend();
                vec[0] = item;
                old
            } else {
                Value::None
            }
        });

        sift_down(heap, interns, list_id, 0);
        heap_arg.drop_with_heap(heap);
        Ok(AttrCallResult::Value(old_smallest))
    } else {
        item.drop_with_heap(heap);
        Err(ExcType::type_error(format!(
            "heappushpop() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )))
    }
}

/// Implementation of `heapq.heapreplace(heap, item)`.
///
/// Pop and return the smallest item from the heap, then push the new item.
/// The heap size is unchanged. If the heap is empty, raises `IndexError`.
///
/// This is more efficient than a `heappop()` followed by `heappush()`, and
/// is appropriate when the heap size is fixed. Note the returned value may
/// be larger than the item added. If that's not desired, consider using
/// `heappushpop()` instead.
///
/// # Errors
/// Returns `TypeError` if the first argument is not a list.
/// Returns `IndexError` if the heap is empty.
fn heapreplace(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (heap_arg, item) = args.get_two_args("heapreplace", heap)?;

    let Value::Ref(list_id) = heap_arg else {
        item.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "heapreplace() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )));
    };

    if let HeapData::List(list) = heap.get(list_id) {
        if list.len() == 0 {
            item.drop_with_heap(heap);
            heap_arg.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::IndexError, "index out of range").into());
        }

        // Replace heap[0] with item and sift down.
        // Use copy_for_extend() because we're taking the value out of the vec
        // and replacing it — the old value keeps its existing refcount.
        let old_smallest = heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                let vec = list.as_vec_mut();
                let old = vec[0].copy_for_extend();
                vec[0] = item;
                old
            } else {
                Value::None
            }
        });

        sift_down(heap, interns, list_id, 0);
        heap_arg.drop_with_heap(heap);
        Ok(AttrCallResult::Value(old_smallest))
    } else {
        item.drop_with_heap(heap);
        Err(ExcType::type_error(format!(
            "heapreplace() arg 1 must be a list, not '{}'",
            heap_arg.py_type(heap)
        )))
    }
}

/// Entry stored in the merge heap for `heapq.merge()`.
///
/// Holds the key value used for ordering, the original item to return, and
/// the index of the iterator that produced it.
struct MergeEntry {
    key: Value,
    value: Value,
    iter_index: usize,
}

/// Implementation of `heapq.merge(*iterables, key=None, reverse=False)`.
///
/// Returns a list of merged items since Ouros does not yet support lazy iterators.
fn merge(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();
    let (key_arg, reverse_arg) = extract_merge_kwargs(kwargs, heap, interns)?;

    let reverse = if let Some(v) = reverse_arg {
        let result = v.py_bool(heap, interns);
        v.drop_with_heap(heap);
        result
    } else {
        false
    };

    let mut key_fn = match key_arg {
        Some(v) if matches!(v, Value::None) => {
            v.drop_with_heap(heap);
            None
        }
        other => other,
    };

    let mut iterators: Vec<OurosIter> = Vec::new();
    while let Some(iterable) = pos.next() {
        match OurosIter::new(iterable, heap, interns) {
            Ok(iter) => iterators.push(iter),
            Err(e) => {
                pos.drop_with_heap(heap);
                drop_iterators(iterators, heap);
                if let Some(k) = key_fn {
                    k.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    }

    if iterators.is_empty() {
        if let Some(k) = key_fn {
            k.drop_with_heap(heap);
        }
        return Ok(AttrCallResult::Value(create_empty_list(heap)?));
    }

    let mut entries: Vec<MergeEntry> = Vec::with_capacity(iterators.len());
    let mut output: Vec<Value> = Vec::new();

    for (index, iter) in iterators.iter_mut().enumerate() {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let key_value = match compute_merge_key(key_fn.as_ref(), &item, heap, interns) {
                    Ok(value) => value,
                    Err(e) => {
                        item.drop_with_heap(heap);
                        drop_values(output, heap);
                        drop_merge_entries(entries, heap);
                        drop_iterators(iterators, heap);
                        if let Some(k) = key_fn {
                            k.drop_with_heap(heap);
                        }
                        return Err(e);
                    }
                };

                entries.push(MergeEntry {
                    key: key_value,
                    value: item,
                    iter_index: index,
                });
                let last_index = entries.len() - 1;
                merge_sift_up(&mut entries, last_index, reverse, heap, interns);
            }
            Ok(None) => {}
            Err(e) => {
                drop_values(output, heap);
                drop_merge_entries(entries, heap);
                drop_iterators(iterators, heap);
                if let Some(k) = key_fn {
                    k.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    }

    while !entries.is_empty() {
        let entry = merge_pop_entry(&mut entries, reverse, heap, interns);
        entry.key.drop_with_heap(heap);
        output.push(entry.value);

        match iterators[entry.iter_index].for_next(heap, interns) {
            Ok(Some(item)) => {
                let key_value = match compute_merge_key(key_fn.as_ref(), &item, heap, interns) {
                    Ok(value) => value,
                    Err(e) => {
                        item.drop_with_heap(heap);
                        drop_values(output, heap);
                        drop_merge_entries(entries, heap);
                        drop_iterators(iterators, heap);
                        if let Some(k) = key_fn {
                            k.drop_with_heap(heap);
                        }
                        return Err(e);
                    }
                };

                entries.push(MergeEntry {
                    key: key_value,
                    value: item,
                    iter_index: entry.iter_index,
                });
                let last_index = entries.len() - 1;
                merge_sift_up(&mut entries, last_index, reverse, heap, interns);
            }
            Ok(None) => {}
            Err(e) => {
                drop_values(output, heap);
                drop_merge_entries(entries, heap);
                drop_iterators(iterators, heap);
                if let Some(k) = key_fn {
                    k.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    }

    drop_iterators(iterators, heap);
    if let Some(k) = key_fn.take() {
        k.drop_with_heap(heap);
    }

    Ok(AttrCallResult::Value(create_list_from_items(heap, output)?))
}

/// Extracts `key` and `reverse` keyword arguments for `heapq.merge()`.
fn extract_merge_kwargs(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<Value>, Option<Value>)> {
    let mut key_value: Option<Value> = None;
    let mut reverse_value: Option<Value> = None;

    for (key, value) in kwargs {
        crate::defer_drop!(key, heap);
        let Some(keyword) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            if let Some(v) = key_value {
                v.drop_with_heap(heap);
            }
            if let Some(v) = reverse_value {
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword.as_str(interns);
        match key_str {
            "key" => {
                if let Some(old) = key_value.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            "reverse" => {
                if let Some(old) = reverse_value.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            _ => {
                value.drop_with_heap(heap);
                if let Some(v) = key_value {
                    v.drop_with_heap(heap);
                }
                if let Some(v) = reverse_value {
                    v.drop_with_heap(heap);
                }
                return Err(ExcType::type_error(format!(
                    "'{key_str}' is an invalid keyword argument for heapq.merge()"
                )));
            }
        }
    }

    Ok((key_value, reverse_value))
}

/// Calls a key function for `heapq.merge()` on a single element.
fn call_merge_key(
    key_fn: &Value,
    elem: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut print = DummyPrint;
    match key_fn {
        Value::Builtin(Builtins::Function(builtin)) => builtin.call(heap, ArgValues::One(elem), interns, &mut print),
        Value::Builtin(Builtins::Type(t)) => t.call(heap, ArgValues::One(elem), interns),
        Value::Builtin(Builtins::TypeMethod { ty, method }) => {
            let builtin = Builtins::TypeMethod {
                ty: *ty,
                method: *method,
            };
            builtin.call(heap, ArgValues::One(elem), interns, &mut print)
        }
        Value::DefFunction(_) | Value::ExtFunction(_) | Value::Ref(_) => {
            elem.drop_with_heap(heap);
            Err(ExcType::type_error(
                "heapq.merge() key argument must be a builtin function (user-defined functions not yet supported)",
            ))
        }
        _ => {
            elem.drop_with_heap(heap);
            Err(ExcType::type_error("heapq.merge() key must be callable or None"))
        }
    }
}

/// Calls a key function on a single element for nsmallest/nlargest.
///
/// Currently supports builtin functions directly. User-defined functions return
/// an error since they would require VM frame management for proper execution.
fn call_nsmallest_nlargest_key(
    key_fn: &Value,
    elem: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut print = DummyPrint;
    match key_fn {
        Value::Builtin(Builtins::Function(builtin)) => builtin.call(heap, ArgValues::One(elem), interns, &mut print),
        Value::Builtin(Builtins::Type(t)) => {
            // Type constructors (int, str, float, etc.) are callable key functions
            let args = ArgValues::One(elem);
            t.call(heap, args, interns)
        }
        Value::Builtin(Builtins::TypeMethod { ty, method }) => {
            let builtin = Builtins::TypeMethod {
                ty: *ty,
                method: *method,
            };
            builtin.call(heap, ArgValues::One(elem), interns, &mut print)
        }
        Value::DefFunction(_) | Value::ExtFunction(_) | Value::Ref(_) => {
            // User-defined or external functions require VM frame management
            elem.drop_with_heap(heap);
            Err(ExcType::type_error(
                "nsmallest()/nlargest() key argument must be a builtin function (user-defined functions not yet supported)",
            ))
        }
        _ => {
            elem.drop_with_heap(heap);
            Err(ExcType::type_error(
                "nsmallest()/nlargest() key must be callable or None",
            ))
        }
    }
}

/// Computes the merge key for a single item, using the optional key function.
fn compute_merge_key(
    key_fn: Option<&Value>,
    item: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if let Some(key) = key_fn {
        let elem = item.clone_with_heap(heap);
        call_merge_key(key, elem, heap, interns)
    } else {
        Ok(item.clone_with_heap(heap))
    }
}

/// Compares two merge entries using key values and stable iterator ordering.
fn merge_entry_cmp(
    left: &MergeEntry,
    right: &MergeEntry,
    reverse: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Ordering {
    let mut cmp = compare_values(&left.key, &right.key, heap, interns);
    if reverse {
        cmp = cmp.reverse();
    }
    if cmp == Ordering::Equal {
        return left.iter_index.cmp(&right.iter_index);
    }
    cmp
}

/// Sifts a merge entry up to restore the heap invariant.
fn merge_sift_up(
    entries: &mut [MergeEntry],
    mut index: usize,
    reverse: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    while index > 0 {
        let parent = (index - 1) / 2;
        if merge_entry_cmp(&entries[index], &entries[parent], reverse, heap, interns) == Ordering::Less {
            entries.swap(index, parent);
            index = parent;
        } else {
            break;
        }
    }
}

/// Sifts a merge entry down to restore the heap invariant.
fn merge_sift_down(
    entries: &mut [MergeEntry],
    mut index: usize,
    reverse: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    let len = entries.len();
    loop {
        let left = 2 * index + 1;
        if left >= len {
            break;
        }
        let right = left + 1;
        let mut smallest = left;
        if right < len && merge_entry_cmp(&entries[right], &entries[left], reverse, heap, interns) == Ordering::Less {
            smallest = right;
        }
        if merge_entry_cmp(&entries[smallest], &entries[index], reverse, heap, interns) == Ordering::Less {
            entries.swap(index, smallest);
            index = smallest;
        } else {
            break;
        }
    }
}

/// Pops the smallest merge entry from the heap.
fn merge_pop_entry(
    entries: &mut Vec<MergeEntry>,
    reverse: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> MergeEntry {
    let last = entries.pop().expect("merge heap not empty");
    if entries.is_empty() {
        return last;
    }

    let root = std::mem::replace(&mut entries[0], last);
    merge_sift_down(entries, 0, reverse, heap, interns);
    root
}

/// Drops all merge entries, releasing their key and value references.
fn drop_merge_entries(entries: Vec<MergeEntry>, heap: &mut Heap<impl ResourceTracker>) {
    for entry in entries {
        entry.key.drop_with_heap(heap);
        entry.value.drop_with_heap(heap);
    }
}

/// Drops all iterators used by `heapq.merge()`.
fn drop_iterators(iterators: Vec<OurosIter>, heap: &mut Heap<impl ResourceTracker>) {
    for iter in iterators {
        iter.drop_with_heap(heap);
    }
}

/// Drops all values accumulated so far during `heapq.merge()`.
fn drop_values(values: Vec<Value>, heap: &mut Heap<impl ResourceTracker>) {
    for value in values {
        value.drop_with_heap(heap);
    }
}

/// Collects all items from an iterable into a Vec.
fn collect_iterable(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    iterable: Value,
) -> RunResult<Vec<Value>> {
    // For a list, we can just clone the items
    if let Value::Ref(id) = &iterable
        && let HeapData::List(list) = heap.get(*id)
    {
        let items: Vec<Value> = list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
        iterable.drop_with_heap(heap);
        return Ok(items);
    }

    // For other iterables, use ContyIter
    let mut iter = crate::types::OurosIter::new(iterable, heap, interns)?;
    let items = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);
    Ok(items)
}

/// Creates an empty list on the heap.
fn create_empty_list(heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
    let list = List::new(Vec::new());
    let id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(id))
}

/// Creates a list on the heap from a Vec of Values.
fn create_list_from_items(
    heap: &mut Heap<impl ResourceTracker>,
    items: Vec<Value>,
) -> Result<Value, crate::resource::ResourceError> {
    let list = List::new(items);
    let id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(id))
}

/// Compares two values using Python comparison semantics.
/// Returns an Ordering that can be used with sort functions.
fn compare_values(
    a: &Value,
    b: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> std::cmp::Ordering {
    if let Some(ord) = a.py_cmp(b, heap, interns) {
        ord
    } else {
        // If comparison fails, use type names as tiebreaker for stable sort
        let type_a = a.py_type(heap);
        let type_b = b.py_type(heap);
        type_a.to_string().cmp(&type_b.to_string())
    }
}
