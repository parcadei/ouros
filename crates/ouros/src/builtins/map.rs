//! Implementation of the map() builtin function.

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

/// Implementation of the map() builtin function.
///
/// Returns a list with func applied to each item of iterable(s).
/// Note: In Python this returns a lazy iterator, but we return a list for simplicity.
/// This matches the behavior of enumerate() and zip() in this implementation.
///
/// Supports:
/// - map(func, iterable) - single iterable
/// - map(func, iter1, iter2, ...) - multiple iterables (zip-like)
///
/// # Note
/// User-defined functions (lambdas, def functions, closures) are handled at the VM level
/// in `call_map_builtin` which delegates to `map_continue`/`handle_map_return`.
pub fn builtin_map(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();

    // Check for unsupported kwargs
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "map() does not support keyword arguments").into());
    }

    // Check for at least one argument (the function)
    let pos_len = positional.len();
    if pos_len == 0 {
        positional.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "map() must have at least one argument (the function)",
        )
        .into());
    }

    // Get the function (first argument)
    let func = positional.next().expect("len check ensures at least one value");

    // If only function provided (no iterables), return empty list
    if pos_len == 1 {
        func.drop_with_heap(heap);
        let heap_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
        return Ok(Value::Ref(heap_id));
    }

    // Create iterators for each iterable
    let mut iterators: Vec<OurosIter> = Vec::with_capacity(pos_len - 1);
    for iterable in positional {
        match OurosIter::new(iterable, heap, interns) {
            Ok(iter) => iterators.push(iter),
            Err(e) => {
                // Clean up already-created iterators
                for iter in iterators {
                    iter.drop_with_heap(heap);
                }
                func.drop_with_heap(heap);
                return Err(e);
            }
        }
    }

    let mut result: Vec<Value> = Vec::new();

    // Process items from all iterators in parallel
    'outer: loop {
        let mut call_args: Vec<Value> = Vec::with_capacity(iterators.len());

        for iter in &mut iterators {
            if let Some(item) = iter.for_next(heap, interns)? {
                call_args.push(item);
            } else {
                // This iterator is exhausted - drop partial args and stop
                for item in call_args {
                    item.drop_with_heap(heap);
                }
                break 'outer;
            }
        }

        // Apply the function to the arguments
        // For now, we only support builtin functions directly
        let applied_result = apply_function(&func, call_args, heap, interns);

        match applied_result {
            Ok(val) => result.push(val),
            Err(e) => {
                for iter in iterators {
                    iter.drop_with_heap(heap);
                }
                func.drop_with_heap(heap);
                return Err(e);
            }
        }
    }

    // Clean up iterators and function
    for iter in iterators {
        iter.drop_with_heap(heap);
    }
    func.drop_with_heap(heap);

    let heap_id = heap.allocate(HeapData::List(List::new(result)))?;
    Ok(Value::Ref(heap_id))
}

/// Apply a builtin function to arguments.
///
/// This handles builtin functions directly. User-defined functions are
/// handled at the VM level via `call_map_builtin`.
fn apply_function(
    func: &Value,
    args: Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    // Builtin functions can be called directly
    if let Value::Builtin(builtin) = func {
        // Convert Vec<Value> to appropriate ArgValues variant
        let arg_values = match args.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(args.into_iter().next().unwrap()),
            2 => {
                let mut iter = args.into_iter();
                ArgValues::Two(iter.next().unwrap(), iter.next().unwrap())
            }
            _ => ArgValues::ArgsKargs {
                args,
                kwargs: crate::args::KwargsValues::Empty,
            },
        };
        builtin.call(heap, arg_values, interns, &mut DummyPrint)
    } else {
        // For other types (user functions, closures), we'd need VM frame support
        // Clean up args
        for arg in args {
            arg.drop_with_heap(heap);
        }
        let type_name = func.py_type(heap);
        Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("'{type_name}' object is not callable from map()"),
        )
        .into())
    }
}
