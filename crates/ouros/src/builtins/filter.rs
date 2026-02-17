//! Implementation of the filter() builtin function.

use std::borrow::Cow;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    exception_public::Exception,
    heap::{DropWithHeap, Heap, HeapData},
    intern::Interns,
    io::PrintWriter,
    resource::ResourceTracker,
    types::{List, OurosIter, PyTrait},
    value::Value,
};

/// Dummy PrintWriter for calling builtins that don't actually print.
struct DummyPrint;

impl PrintWriter for DummyPrint {
    fn stdout_write(&mut self, _output: Cow<'_, str>) -> Result<(), Exception> {
        Ok(())
    }
    fn stdout_push(&mut self, _end: char) -> Result<(), Exception> {
        Ok(())
    }
}

/// Implementation of the filter() builtin function.
///
/// Returns a list with items from iterable where func(item) is truthy.
/// Note: In Python this returns a lazy iterator, but we return a list for simplicity.
/// This matches the behavior of map(), enumerate() and zip() in this implementation.
///
/// Supports:
/// - filter(func, iterable) - keep items where func(item) is truthy
/// - filter(None, iterable) - keep items that are truthy (identity filter)
///
/// # Note
/// User-defined functions (lambdas, def functions, closures) are handled at the VM level
/// in `call_filter_builtin` which delegates to `filter_continue`/`handle_filter_return`.
pub fn builtin_filter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();

    // Check for unsupported kwargs
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "filter() does not support keyword arguments").into());
    }

    // Check for correct number of arguments
    let pos_len = positional.len();
    if pos_len != 2 {
        positional.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("filter() expected 2 arguments, got {pos_len}"),
        )
        .into());
    }

    // Get the function (first argument) and iterable (second argument)
    let func = positional.next().expect("len check ensures at least one value");
    let iterable = positional.next().expect("len check ensures two values");

    // Create iterator for the iterable
    let mut iterator = match OurosIter::new(iterable, heap, interns) {
        Ok(iter) => iter,
        Err(e) => {
            func.drop_with_heap(heap);
            return Err(e);
        }
    };

    let mut result: Vec<Value> = Vec::new();

    // Determine if we're using a function or identity (None)
    let use_identity = matches!(func, Value::None);

    // Process items from the iterator
    if use_identity {
        // filter(None, iterable) - keep truthy values
        while let Some(item) = iterator.for_next(heap, interns)? {
            if item.py_bool(heap, interns) {
                result.push(item);
            } else {
                item.drop_with_heap(heap);
            }
        }
        func.drop_with_heap(heap);
    } else {
        // filter(func, iterable) - apply func and keep where result is truthy
        while let Some(item) = iterator.for_next(heap, interns)? {
            // Apply the function to the item
            let func_result = apply_filter_function(&func, item.clone_with_heap(heap), heap, interns);

            match func_result {
                Ok(keep) => {
                    if keep.py_bool(heap, interns) {
                        result.push(item);
                    } else {
                        item.drop_with_heap(heap);
                    }
                    keep.drop_with_heap(heap);
                }
                Err(e) => {
                    item.drop_with_heap(heap);
                    func.drop_with_heap(heap);
                    return Err(e);
                }
            }
        }
        func.drop_with_heap(heap);
    }

    // Clean up the iterator
    iterator.drop_with_heap(heap);

    let heap_id = heap.allocate(HeapData::List(List::new(result)))?;
    Ok(Value::Ref(heap_id))
}

/// Apply a builtin filter function to an argument.
///
/// This handles builtin functions directly. User-defined functions are
/// handled at the VM level via `call_filter_builtin`.
fn apply_filter_function(
    func: &Value,
    arg: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    // Builtin functions can be called directly
    if let Value::Builtin(builtin) = func {
        let arg_values = ArgValues::One(arg);
        builtin.call(heap, arg_values, interns, &mut DummyPrint)
    } else {
        // For other types (user functions, closures), we'd need VM frame support
        arg.drop_with_heap(heap);
        let type_name = func.py_type(heap);
        Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("'{type_name}' object is not callable from filter()"),
        )
        .into())
    }
}
