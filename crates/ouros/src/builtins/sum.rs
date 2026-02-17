//! Implementation of the sum() builtin function.

use num_bigint::BigInt;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData},
    intern::Interns,
    resource::ResourceTracker,
    types::{LongInt, OurosIter, PyTrait, Type},
    value::Value,
};

/// Implementation of the sum() builtin function.
///
/// Sums the items of an iterable from left to right with an optional start value.
/// The default start value is 0. String start values are explicitly rejected
/// (use `''.join(seq)` instead for string concatenation).
pub fn builtin_sum(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (iterable, start) = args.get_one_two_args("sum", heap)?;

    // Get the start value, defaulting to 0
    let mut accumulator = match start {
        Some(v) => {
            // Reject string start values - Python explicitly forbids this
            let is_str = matches!(v.py_type(heap), Type::Str);
            if is_str {
                iterable.drop_with_heap(heap);
                v.drop_with_heap(heap);
                return Err(SimpleException::new_msg(
                    ExcType::TypeError,
                    "sum() can't sum strings [use ''.join(seq) instead]",
                )
                .into());
            }
            v
        }
        None => Value::Int(0),
    };

    // Fast path for the common benchmark shape: sum(list_of_ints[, int_start]).
    // This avoids iterator construction and repeated dynamic binary-op dispatch.
    if let Some(fast_result) = try_sum_int_sequence(&iterable, &accumulator, heap) {
        iterable.drop_with_heap(heap);
        accumulator.drop_with_heap(heap);
        return fast_result;
    }

    // Create iterator from the iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;

    // Sum all items
    while let Some(item) = iter.for_next(heap, interns)? {
        // Get item type before any operations (needed for error messages)
        let item_type = item.py_type(heap);

        // Try to add the item to accumulator
        let add_result = accumulator.py_add(&item, heap, interns);
        item.drop_with_heap(heap);

        match add_result {
            Ok(Some(new_value)) => {
                accumulator.drop_with_heap(heap);
                accumulator = new_value;
            }
            Ok(None) => {
                // Types don't support addition - use binary_type_error for consistent messages
                let acc_type = accumulator.py_type(heap);
                accumulator.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(ExcType::binary_type_error("+", acc_type, item_type));
            }
            Err(e) => {
                accumulator.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(e.into());
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(accumulator)
}

/// Attempts a specialized `sum()` for list/tuple/namedtuple integer sequences.
///
/// Supports:
/// - `start` of `int`, `bool`, or `LongInt`
/// - items of `int` or `bool`
///
/// Returns `None` when the call shape is not supported, so the caller can fall
/// back to the full generic `sum()` implementation.
fn try_sum_int_sequence(
    iterable: &Value,
    start: &Value,
    heap: &mut Heap<impl ResourceTracker>,
) -> Option<RunResult<Value>> {
    enum Accumulator {
        Int(i64),
        Big(BigInt),
    }

    let mut acc = match start {
        Value::Int(i) => Accumulator::Int(*i),
        Value::Bool(b) => Accumulator::Int(i64::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => Accumulator::Big(li.inner().clone()),
            _ => return None,
        },
        _ => return None,
    };

    let sum_result = {
        let items = match iterable {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::List(list) => list.as_vec().as_slice(),
                HeapData::Tuple(tuple) => tuple.as_vec().as_slice(),
                HeapData::NamedTuple(tuple) => tuple.as_vec().as_slice(),
                _ => return None,
            },
            _ => return None,
        };

        for item in items {
            let item_i64 = match item {
                Value::Int(i) => *i,
                Value::Bool(b) => i64::from(*b),
                _ => return None,
            };

            let mut promote_to_big: Option<BigInt> = None;
            match &mut acc {
                Accumulator::Int(total) => {
                    if let Some(next) = total.checked_add(item_i64) {
                        *total = next;
                    } else {
                        promote_to_big = Some(BigInt::from(*total) + BigInt::from(item_i64));
                    }
                }
                Accumulator::Big(total) => {
                    *total += item_i64;
                }
            }
            if let Some(promoted) = promote_to_big {
                acc = Accumulator::Big(promoted);
            }
        }

        Ok(())
    };

    Some(match sum_result {
        Ok(()) => match acc {
            Accumulator::Int(total) => Ok(Value::Int(total)),
            Accumulator::Big(total) => LongInt::new(total).into_value(heap).map_err(Into::into),
        },
        Err(e) => Err(e),
    })
}
