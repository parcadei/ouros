//! Implementation of the `bisect` module.
//!
//! Provides binary search functions for sorted lists:
//! - `bisect_left(a, x, lo=0, hi=len(a), *, key=None)`: Return leftmost insertion index for x in sorted list a
//! - `bisect_right(a, x, lo=0, hi=len(a), *, key=None)`: Return rightmost insertion index for x in sorted list a
//! - `bisect(a, x, lo=0, hi=len(a), *, key=None)`: Alias for `bisect_right`
//! - `insort_left(a, x, lo=0, hi=len(a), *, key=None)`: Insert x into sorted list a at leftmost position
//! - `insort_right(a, x, lo=0, hi=len(a), *, key=None)`: Insert x into sorted list a at rightmost position
//! - `insort(a, x, lo=0, hi=len(a), *, key=None)`: Alias for `insort_right`
//!
//! All functions accept optional `lo` and `hi` integer parameters to restrict the
//! search range. If omitted, `lo` defaults to 0 and `hi` defaults to `len(a)`.
//! All functions accept optional `key` parameter for custom comparison.

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    exception_public::Exception,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::PrintWriter,
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Module, PyTrait},
    value::Value,
};

/// Dummy PrintWriter for calling builtin key functions.
struct DummyPrint;

impl PrintWriter for DummyPrint {
    fn stdout_write(&mut self, _output: std::borrow::Cow<'_, str>) -> Result<(), Exception> {
        Ok(())
    }

    fn stdout_push(&mut self, _end: char) -> Result<(), Exception> {
        Ok(())
    }
}

/// Bisect module functions.
///
/// Each variant maps to a function in Python's `bisect` module for binary search
/// on sorted lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum BisectFunctions {
    BisectLeft,
    BisectRight,
    InsortLeft,
    InsortRight,
}

/// Creates the `bisect` module and allocates it on the heap.
///
/// Sets up all bisect functions and aliases.
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
    let mut module = Module::new(StaticStrings::Bisect);

    // bisect_left - find leftmost insertion point
    module.set_attr(
        StaticStrings::BisectLeft,
        Value::ModuleFunction(ModuleFunctions::Bisect(BisectFunctions::BisectLeft)),
        heap,
        interns,
    );

    // bisect_right - find rightmost insertion point
    module.set_attr(
        StaticStrings::BisectRight,
        Value::ModuleFunction(ModuleFunctions::Bisect(BisectFunctions::BisectRight)),
        heap,
        interns,
    );

    // bisect - alias for bisect_right
    module.set_attr(
        StaticStrings::Bisect,
        Value::ModuleFunction(ModuleFunctions::Bisect(BisectFunctions::BisectRight)),
        heap,
        interns,
    );

    // insort_left - insert at leftmost position
    module.set_attr(
        StaticStrings::InsortLeft,
        Value::ModuleFunction(ModuleFunctions::Bisect(BisectFunctions::InsortLeft)),
        heap,
        interns,
    );

    // insort_right - insert at rightmost position
    module.set_attr(
        StaticStrings::InsortRight,
        Value::ModuleFunction(ModuleFunctions::Bisect(BisectFunctions::InsortRight)),
        heap,
        interns,
    );

    // insort - alias for insort_right
    module.set_attr(
        StaticStrings::Insort,
        Value::ModuleFunction(ModuleFunctions::Bisect(BisectFunctions::InsortRight)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a bisect module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: BisectFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        BisectFunctions::BisectLeft => bisect_left(heap, interns, args),
        BisectFunctions::BisectRight => bisect_right(heap, interns, args),
        BisectFunctions::InsortLeft => insort_left(heap, interns, args),
        BisectFunctions::InsortRight => insort_right(heap, interns, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `bisect.bisect_left(a, x, lo=0, hi=len(a), *, key=None)`.
///
/// Returns the leftmost insertion index for x in sorted list a.
/// This is the index where x would be inserted to maintain sort order,
/// to the left of any existing entries of x.
///
/// Optional `lo` and `hi` parameters restrict the search range.
/// Optional `key` parameter provides custom comparison.
fn bisect_left(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (list_val, x, lo_val, hi_val, key_fn) = extract_bisect_args("bisect_left", args, heap, interns)?;
    defer_drop!(list_val, heap);
    defer_drop!(x, heap);

    let list_id = get_list_id(list_val, heap)?;
    let (lo, hi) = resolve_lo_hi(list_id, lo_val, hi_val, heap)?;
    let index = bisect_search(list_id, x, heap, interns, true, lo, hi, key_fn)?;
    Ok(Value::Int(index))
}

/// Implementation of `bisect.bisect_right(a, x, lo=0, hi=len(a), *, key=None)`.
///
/// Returns the rightmost insertion index for x in sorted list a.
/// This is the index where x would be inserted to maintain sort order,
/// to the right of any existing entries of x.
///
/// Optional `lo` and `hi` parameters restrict the search range.
/// Optional `key` parameter provides custom comparison.
fn bisect_right(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (list_val, x, lo_val, hi_val, key_fn) = extract_bisect_args("bisect_right", args, heap, interns)?;
    defer_drop!(list_val, heap);
    defer_drop!(x, heap);

    let list_id = get_list_id(list_val, heap)?;
    let (lo, hi) = resolve_lo_hi(list_id, lo_val, hi_val, heap)?;
    let index = bisect_search(list_id, x, heap, interns, false, lo, hi, key_fn)?;
    Ok(Value::Int(index))
}

/// Implementation of `bisect.insort_left(a, x, lo=0, hi=len(a), *, key=None)`.
///
/// Inserts x into sorted list a at the leftmost position to maintain sort order.
/// Modifies the list in-place and returns None.
///
/// Optional `lo` and `hi` parameters restrict the search range.
/// Optional `key` parameter provides custom comparison.
fn insort_left(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (list_val, x, lo_val, hi_val, key_fn) = extract_bisect_args("insort_left", args, heap, interns)?;

    let list_id = get_list_id(&list_val, heap)?;
    let (lo, hi) = resolve_lo_hi(list_id, lo_val, hi_val, heap)?;

    // Perform binary search (needs mutable heap for comparisons)
    let index = bisect_search(list_id, &x, heap, interns, true, lo, hi, key_fn)?;

    insert_at_index(list_id, index, x, heap)?;

    // Drop the list reference
    list_val.drop_with_heap(heap);

    Ok(Value::None)
}

/// Implementation of `bisect.insort_right(a, x, lo=0, hi=len(a), *, key=None)`.
///
/// Inserts x into sorted list a at the rightmost position to maintain sort order.
/// Modifies the list in-place and returns None.
///
/// Optional `lo` and `hi` parameters restrict the search range.
/// Optional `key` parameter provides custom comparison.
fn insort_right(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (list_val, x, lo_val, hi_val, key_fn) = extract_bisect_args("insort_right", args, heap, interns)?;

    let list_id = get_list_id(&list_val, heap)?;
    let (lo, hi) = resolve_lo_hi(list_id, lo_val, hi_val, heap)?;

    // Perform binary search (needs mutable heap for comparisons)
    let index = bisect_search(list_id, &x, heap, interns, false, lo, hi, key_fn)?;

    insert_at_index(list_id, index, x, heap)?;

    // Drop the list reference
    list_val.drop_with_heap(heap);

    Ok(Value::None)
}

// ===== Helper functions =====

/// Extracts the arguments for bisect functions.
///
/// Returns `(list, x, optional_lo, optional_hi, optional_key)`.
/// All bisect functions accept `(a, x)` or `(a, x, lo)` or `(a, x, lo, hi)`
/// plus keyword-only `lo`, `hi`, `key` parameters.
fn extract_bisect_args(
    name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Value, Option<Value>, Option<Value>, Option<Value>)> {
    match args {
        ArgValues::Two(a, x) => Ok((a, x, None, None, None)),
        ArgValues::ArgsKargs { args, kwargs } => {
            let count = args.len();
            if count < 2 {
                // Drop all arguments and return error
                for v in args {
                    v.drop_with_heap(heap);
                }
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "bisect.{name}() missing required argument: 'a' or 'x'"
                )));
            }

            let mut iter = args.into_iter();
            let a = iter.next().unwrap();
            let x = iter.next().unwrap();

            // After a and x, we have lo and/or hi as positional args
            // We need to figure out how to interpret remaining positional args
            let remaining: Vec<Value> = iter.collect();
            let mut lo_pos: Option<Value> = None;
            let mut hi_pos: Option<Value> = None;

            match remaining.len() {
                0 => {} // No lo or hi
                1 => {
                    // One extra arg: this is lo
                    lo_pos = Some(remaining.into_iter().next().unwrap());
                }
                2 => {
                    // Two extra args: these are lo and hi
                    let mut rem_iter = remaining.into_iter();
                    lo_pos = Some(rem_iter.next().unwrap());
                    hi_pos = Some(rem_iter.next().unwrap());
                }
                _ => {
                    // Too many positional arguments
                    for v in remaining {
                        v.drop_with_heap(heap);
                    }
                    kwargs.drop_with_heap(heap);
                    a.drop_with_heap(heap);
                    x.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "bisect.{name}() takes at most 4 positional arguments but {count} were given"
                    )));
                }
            }

            // Parse keyword arguments to find lo, hi, key functions
            let mut lo_kw: Option<Value> = None;
            let mut hi_kw: Option<Value> = None;
            let mut key_fn: Option<Value> = None;

            for (kw_key, kw_value) in kwargs {
                defer_drop!(kw_key, heap);

                let Some(keyword_name) = kw_key.as_either_str(heap) else {
                    // Clean up all values before returning error
                    kw_value.drop_with_heap(heap);
                    if let Some(v) = lo_pos {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = hi_pos {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = lo_kw {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = hi_kw {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = key_fn {
                        v.drop_with_heap(heap);
                    }
                    a.drop_with_heap(heap);
                    x.drop_with_heap(heap);
                    return Err(ExcType::type_error("keywords must be strings"));
                };

                let key_str = keyword_name.as_str(interns);
                match key_str {
                    "lo" => {
                        lo_kw = Some(kw_value);
                    }
                    "hi" => {
                        hi_kw = Some(kw_value);
                    }
                    "key" => {
                        if matches!(kw_value, Value::None) {
                            // None means no key function
                            kw_value.drop_with_heap(heap);
                        } else {
                            key_fn = Some(kw_value);
                        }
                    }
                    _ => {
                        // Invalid keyword argument - clean up everything
                        kw_value.drop_with_heap(heap);
                        if let Some(v) = lo_pos {
                            v.drop_with_heap(heap);
                        }
                        if let Some(v) = hi_pos {
                            v.drop_with_heap(heap);
                        }
                        if let Some(v) = lo_kw {
                            v.drop_with_heap(heap);
                        }
                        if let Some(v) = hi_kw {
                            v.drop_with_heap(heap);
                        }
                        if let Some(v) = key_fn {
                            v.drop_with_heap(heap);
                        }
                        a.drop_with_heap(heap);
                        x.drop_with_heap(heap);
                        return Err(ExcType::type_error(format!(
                            "'{key_str}' is an invalid keyword argument for bisect.{name}()"
                        )));
                    }
                }
            }

            // Merge positional and keyword args: keyword args take precedence
            let lo = lo_kw.or(lo_pos);
            let hi = hi_kw.or(hi_pos);

            Ok((a, x, lo, hi, key_fn))
        }
        other => {
            let count = match &other {
                ArgValues::Empty => 0,
                ArgValues::One(_) => 1,
                _ => 0,
            };
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "bisect.{name}() takes 2 to 4 positional arguments but {count} were given"
            )))
        }
    }
}

/// Resolves optional `lo` and `hi` arguments into concrete bounds.
///
/// Defaults: `lo=0`, `hi=len(list)`. Validates that lo and hi are non-negative
/// integers and clamps them to the list length.
#[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn resolve_lo_hi(
    list_id: HeapId,
    lo_val: Option<Value>,
    hi_val: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<(usize, usize)> {
    let len = match heap.get(list_id) {
        HeapData::List(list) => list.len(),
        _ => 0,
    };

    let lo = match lo_val {
        Some(val) => {
            let lo_int = val.as_int(heap)?;
            val.drop_with_heap(heap);
            if lo_int < 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "lo must be non-negative").into());
            }
            (lo_int as usize).min(len)
        }
        None => 0,
    };

    let hi = match hi_val {
        Some(val) => {
            let hi_int = val.as_int(heap)?;
            val.drop_with_heap(heap);
            if hi_int < 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "hi must be non-negative").into());
            }
            (hi_int as usize).min(len)
        }
        None => len,
    };

    Ok((lo, hi))
}

/// Inserts a value at the given index in a list.
///
/// Handles reference tracking and cycle detection for heap-allocated values.
fn insert_at_index(list_id: HeapId, index: i64, x: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<()> {
    // Clone x for insertion
    let x_clone = x.clone_with_heap(heap);
    let index_usize = usize::try_from(index).unwrap_or(0);

    // Mark potential cycle if needed (must be done before getting list mut ref)
    let is_ref = matches!(x_clone, Value::Ref(_));
    if is_ref {
        heap.mark_potential_cycle();
    }

    // Get the list for mutation and insert directly
    let HeapData::List(list) = heap.get_mut(list_id) else {
        x.drop_with_heap(heap);
        x_clone.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "bisect requires a list as first argument".to_string(),
        ));
    };

    // Track if we're adding a reference
    if is_ref {
        list.set_contains_refs();
    }

    // Insert directly into the items vector
    let items = list.as_vec_mut();
    if index_usize >= items.len() {
        items.push(x_clone);
    } else {
        items.insert(index_usize, x_clone);
    }

    // Drop the original x (we used x_clone for insertion)
    x.drop_with_heap(heap);

    Ok(())
}

/// Calls a key function on a single element for bisect.
///
/// Currently supports builtin functions and type constructors directly.
fn call_bisect_key(
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
            // Type methods (e.g., str.lower) are callable key functions
            // We need to call the method with elem as the first argument (instance)
            let ty = *ty;
            let method = *method;
            // Use the builtin's call_type_method via the Builtins::call method
            let builtin = Builtins::TypeMethod { ty, method };
            builtin.call(heap, ArgValues::One(elem), interns, &mut print)
        }
        Value::DefFunction(_) | Value::ExtFunction(_) | Value::Ref(_) => {
            // User-defined or external functions require VM frame management
            elem.drop_with_heap(heap);
            Err(ExcType::type_error(
                "bisect() key argument must be a builtin function (user-defined functions not yet supported)",
            ))
        }
        _ => {
            elem.drop_with_heap(heap);
            Err(ExcType::type_error("bisect() key must be callable or None"))
        }
    }
}

/// Binary search for the insertion point of x in a sorted list.
///
/// # Arguments
/// * `list_id` - The heap ID of the sorted list to search
/// * `x` - The value to insert
/// * `heap` - The heap for value comparisons
/// * `interns` - The interns table for string comparisons
/// * `left` - If true, find leftmost insertion point; if false, find rightmost
/// * `lo` - Start index of the search range (inclusive)
/// * `hi` - End index of the search range (exclusive)
/// * `key_fn` - Optional key function for custom comparison
///
/// # Returns
/// The insertion index as an i64.
fn bisect_search(
    list_id: HeapId,
    x: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    left: bool,
    lo: usize,
    hi: usize,
    key_fn: Option<Value>,
) -> RunResult<i64> {
    let mut lo = lo;
    let mut hi = hi;

    while lo < hi {
        let mid = lo + (hi - lo) / 2;

        // Get the mid value (this clones it with proper refcount)
        let mid_val = {
            let HeapData::List(list) = heap.get(list_id) else {
                key_fn.drop_with_heap(heap);
                return Ok(i64::try_from(lo).expect("list index exceeds i64::MAX"));
            };
            let items = list.as_vec();
            if mid >= items.len() {
                break;
            }
            items[mid].clone_with_heap(heap)
        };

        // Apply key function to mid_val if provided
        // Note: When key is provided, we compare key(mid_val) with x directly
        // (not with key(x)), matching CPython behavior.
        let cmp = if let Some(ref key) = key_fn {
            let mid_key_result = call_bisect_key(key, mid_val.clone_with_heap(heap), heap, interns);
            match mid_key_result {
                Ok(mk) => {
                    let cmp_result = compare_values(&mk, x, heap, interns);
                    mk.drop_with_heap(heap);
                    cmp_result
                }
                Err(e) => {
                    mid_val.drop_with_heap(heap);
                    key_fn.drop_with_heap(heap);
                    return Err(e);
                }
            }
        } else {
            compare_values(&mid_val, x, heap, interns)
        };

        if cmp == Some(std::cmp::Ordering::Less) {
            // mid_val < x, search right half
            lo = mid + 1;
        } else if cmp == Some(std::cmp::Ordering::Greater) {
            // mid_val > x, search left half
            hi = mid;
        } else {
            // mid_val == x or not comparable
            if left {
                // For bisect_left, go left to find first occurrence
                hi = mid;
            } else {
                // For bisect_right, go right to find after last occurrence
                lo = mid + 1;
            }
        }
    }

    key_fn.drop_with_heap(heap);

    Ok(i64::try_from(lo).expect("list index exceeds i64::MAX"))
}

/// Compare two values and return their ordering if comparable.
///
/// Returns None if the values cannot be compared.
fn compare_values(
    a: &Value,
    b: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<std::cmp::Ordering> {
    a.py_cmp(b, heap, interns)
}

/// Extract a list HeapId from a Value.
///
/// Returns the HeapId if the value is a Ref pointing to a List.
/// Returns a TypeError if the value is not a list.
fn get_list_id(value: &Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    match value {
        Value::Ref(id) => {
            if let HeapData::List(_) = heap.get(*id) {
                Ok(*id)
            } else {
                Err(ExcType::type_error(
                    "bisect requires a list as first argument".to_string(),
                ))
            }
        }
        _ => Err(ExcType::type_error(
            "bisect requires a list as first argument".to_string(),
        )),
    }
}
