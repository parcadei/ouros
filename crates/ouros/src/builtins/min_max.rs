//! Implementation of the min() and max() builtin functions.

use std::{borrow::Cow, cmp::Ordering};

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    exception_public::Exception,
    heap::{Heap, HeapData, HeapGuard},
    intern::Interns,
    io::PrintWriter,
    resource::ResourceTracker,
    types::{OurosIter, PyTrait},
    value::Value,
};

/// Dummy PrintWriter for calling builtin key functions that don't print.
struct DummyPrint;

impl PrintWriter for DummyPrint {
    fn stdout_write(&mut self, _output: Cow<'_, str>) -> Result<(), Exception> {
        Ok(())
    }

    fn stdout_push(&mut self, _end: char) -> Result<(), Exception> {
        Ok(())
    }
}

/// Implementation of the min() builtin function.
///
/// Returns the smallest item in an iterable or the smallest of two or more arguments.
/// Supports two forms:
/// - `min(iterable)` - returns smallest item from iterable
/// - `min(arg1, arg2, ...)` - returns smallest of the arguments
pub fn builtin_min(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    builtin_min_max(heap, args, interns, true)
}

/// Implementation of the max() builtin function.
///
/// Returns the largest item in an iterable or the largest of two or more arguments.
/// Supports two forms:
/// - `max(iterable)` - returns largest item from iterable
/// - `max(arg1, arg2, ...)` - returns largest of the arguments
pub fn builtin_max(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    builtin_min_max(heap, args, interns, false)
}

/// Shared implementation for min() and max().
///
/// When `is_min` is true, returns the minimum; otherwise returns the maximum.
fn builtin_min_max(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    interns: &Interns,
    is_min: bool,
) -> RunResult<Value> {
    let func_name = if is_min { "min" } else { "max" };
    let (positional, kwargs) = args.into_parts();
    defer_drop_mut!(positional, heap);
    let mut key_fn: Option<Value> = None;
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(key_name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns);
        if key_name != "key" {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(func_name, key_name));
        }
        if key_fn.is_some() {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_multiple_values(func_name, "key"));
        }
        key_fn = Some(value);
    }
    let key_fn = match key_fn {
        Some(value) if matches!(value, Value::None) => {
            value.drop_with_heap(heap);
            None
        }
        other => other,
    };
    let mut key_fn_guard = HeapGuard::new(key_fn, heap);
    let (key_fn, heap) = key_fn_guard.as_parts_mut();

    let Some(first_arg) = positional.next() else {
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("{func_name}() expected at least 1 argument, got 0"),
        )
        .into());
    };

    // decide what to do based on remaining arguments
    if positional.len() == 0 {
        // Fast path for homogeneous primitive sequences without key=.
        if key_fn.is_none()
            && let Some(result) = try_min_max_homogeneous_sequence(&first_arg, heap, interns, is_min, func_name)
        {
            first_arg.drop_with_heap(heap);
            return result;
        }

        // Single argument: iterate over it
        let iter = OurosIter::new(first_arg, heap, interns)?;
        defer_drop_mut!(iter, heap);

        let Some(result) = iter.for_next(heap, interns)? else {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("{func_name}() iterable argument is empty"),
            )
            .into());
        };

        let mut result_guard = HeapGuard::new(result, heap);
        let (result, heap) = result_guard.as_parts_mut();

        if let Some(key_fn) = key_fn.as_ref() {
            let result_key = apply_key_function(key_fn, result.clone_with_heap(heap), heap, interns, func_name)?;
            let mut result_key_guard = HeapGuard::new(result_key, heap);
            let (result_key, heap) = result_key_guard.as_parts_mut();

            while let Some(item) = iter.for_next(heap, interns)? {
                defer_drop_mut!(item, heap);
                let item_key = apply_key_function(key_fn, item.clone_with_heap(heap), heap, interns, func_name)?;
                defer_drop_mut!(item_key, heap);

                let Some(ordering) = result_key.py_cmp(item_key, heap, interns) else {
                    return Err(ord_not_supported(result_key, item_key, heap));
                };

                if (is_min && ordering == Ordering::Greater) || (!is_min && ordering == Ordering::Less) {
                    std::mem::swap(result, item);
                    std::mem::swap(result_key, item_key);
                }
            }
        } else {
            while let Some(item) = iter.for_next(heap, interns)? {
                defer_drop_mut!(item, heap);

                let Some(ordering) = result.py_cmp(item, heap, interns) else {
                    return Err(ord_not_supported(result, item, heap));
                };

                if (is_min && ordering == Ordering::Greater) || (!is_min && ordering == Ordering::Less) {
                    std::mem::swap(result, item);
                }
            }
        }

        Ok(result_guard.into_inner())
    } else {
        // Multiple arguments: compare them directly
        let mut result_guard = HeapGuard::new(first_arg, heap);
        let (result, heap) = result_guard.as_parts_mut();

        if let Some(key_fn) = key_fn.as_ref() {
            let result_key = apply_key_function(key_fn, result.clone_with_heap(heap), heap, interns, func_name)?;
            let mut result_key_guard = HeapGuard::new(result_key, heap);
            let (result_key, heap) = result_key_guard.as_parts_mut();

            for item in positional {
                defer_drop_mut!(item, heap);
                let item_key = apply_key_function(key_fn, item.clone_with_heap(heap), heap, interns, func_name)?;
                defer_drop_mut!(item_key, heap);

                let Some(ordering) = result_key.py_cmp(item_key, heap, interns) else {
                    return Err(ord_not_supported(result_key, item_key, heap));
                };

                if (is_min && ordering == Ordering::Greater) || (!is_min && ordering == Ordering::Less) {
                    std::mem::swap(result, item);
                    std::mem::swap(result_key, item_key);
                }
            }
        } else {
            for item in positional {
                defer_drop_mut!(item, heap);

                let Some(ordering) = result.py_cmp(item, heap, interns) else {
                    return Err(ord_not_supported(result, item, heap));
                };

                if (is_min && ordering == Ordering::Greater) || (!is_min && ordering == Ordering::Less) {
                    std::mem::swap(result, item);
                }
            }
        }

        Ok(result_guard.into_inner())
    }
}

/// Attempts a specialized `min()`/`max()` over homogeneous primitive sequences.
///
/// Supports exact list/tuple/namedtuple iterables where all elements are:
/// - `int`/`bool`, or
/// - `str` (interned or heap string)
///
/// Returns `None` when unsupported so the caller can fall back to the generic
/// iterator + dynamic comparison path.
fn try_min_max_homogeneous_sequence(
    iterable: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    is_min: bool,
    func_name: &str,
) -> Option<RunResult<Value>> {
    let items = match iterable {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::List(list) => list.as_vec().as_slice(),
            HeapData::Tuple(tuple) => tuple.as_vec().as_slice(),
            HeapData::NamedTuple(tuple) => tuple.as_vec().as_slice(),
            _ => return None,
        },
        _ => return None,
    };

    if items.is_empty() {
        return Some(Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("{func_name}() iterable argument is empty"),
        )
        .into()));
    }

    if items.iter().all(|item| matches!(item, Value::Int(_) | Value::Bool(_))) {
        let mut best_index = 0usize;
        let mut best_value = int_like_to_i64(&items[0]).expect("all() guarantees int-like");
        for (idx, item) in items.iter().enumerate().skip(1) {
            let current = int_like_to_i64(item).expect("all() guarantees int-like");
            let is_better = if is_min {
                current < best_value
            } else {
                current > best_value
            };
            if is_better {
                best_value = current;
                best_index = idx;
            }
        }
        return Some(Ok(items[best_index].clone_with_heap(heap)));
    }

    if items.iter().all(|item| as_exact_str(item, heap, interns).is_some()) {
        let mut best_index = 0usize;
        for idx in 1..items.len() {
            let best_str = as_exact_str(&items[best_index], heap, interns).expect("all() guarantees str-like");
            let current_str = as_exact_str(&items[idx], heap, interns).expect("all() guarantees str-like");
            let is_better = if is_min {
                current_str < best_str
            } else {
                current_str > best_str
            };
            if is_better {
                best_index = idx;
            }
        }
        return Some(Ok(items[best_index].clone_with_heap(heap)));
    }

    None
}

/// Converts an int-like immediate value to i64.
///
/// Returns `None` for non-int-like values.
fn int_like_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

/// Returns the underlying string slice when `value` is an exact string.
///
/// Accepts interned strings and heap `str` objects. Returns `None` for all
/// other value kinds.
fn as_exact_str<'a>(value: &'a Value, heap: &'a Heap<impl ResourceTracker>, interns: &'a Interns) -> Option<&'a str> {
    match value {
        Value::InternString(id) => Some(interns.get_str(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// Applies a `key=` function for `min()`/`max()` comparisons.
///
/// This currently supports builtin callables and builtin type constructors.
fn apply_key_function(
    key_fn: &Value,
    item: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<Value> {
    match key_fn {
        Value::Builtin(Builtins::Function(builtin_fn)) => {
            let args = ArgValues::One(item);
            builtin_fn.call(heap, args, interns, &mut DummyPrint)
        }
        Value::Builtin(Builtins::Type(t)) => {
            let args = ArgValues::One(item);
            t.call(heap, args, interns)
        }
        _ => {
            let item_type = key_fn.py_type(heap);
            item.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{item_type}' object is not callable as {func_name}() key"
            )))
        }
    }
}

#[cold]
fn ord_not_supported(left: &Value, right: &Value, heap: &Heap<impl ResourceTracker>) -> RunError {
    let left_type = left.py_type(heap);
    let right_type = right.py_type(heap);
    ExcType::type_error(format!(
        "'<' not supported between instances of '{left_type}' and '{right_type}'"
    ))
}
