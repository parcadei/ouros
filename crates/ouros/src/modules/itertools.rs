//! Implementation of the `itertools` module.
//!
//! Provides iterator utilities from Python's `itertools` module.
//! Since Ouros has limited iterator protocol support, most functions
//! eagerly return lists instead of lazy iterators.
//!
//! # Implemented Functions
//! - `chain(*iterables)` - Concatenate multiple iterables into one list
//! - `chain.from_iterable(iterable_of_iterables)` - Flatten one level of nesting
//! - `tee(iterable, n=2)` - Clone an iterator into n independent iterators
//! - `islice(iterable, stop)` / `islice(iterable, start, stop)` - Return a sliced list
//! - `zip_longest(*iterables, fillvalue=None)` - Zip iterables, padding shorter ones
//! - `product(*iterables)` - Cartesian product as list of tuples
//! - `permutations(iterable, r=None)` - Return list of tuples of permutations
//! - `combinations(iterable, r)` - Return list of tuples of combinations
//! - `combinations_with_replacement(iterable, r)` - Combinations allowing repeated elements
//! - `repeat(elem, times)` - Return list of elem repeated times times
//! - `count(start=0, step=1)` - Infinite counter (returns lazy iterator)
//! - `cycle(iterable)` - Infinite repeater (returns lazy iterator)
//! - `accumulate(iterable, func=operator.add)` - Running total / accumulation
//! - `starmap(function, iterable)` - Map with unpacked tuple arguments
//! - `filterfalse(predicate, iterable)` - Keep elements where predicate is false
//! - `takewhile(predicate, iterable)` - Take while predicate is true
//! - `dropwhile(predicate, iterable)` - Drop leading elements while predicate is true
//! - `compress(data, selectors)` - Filter data by boolean selectors
//! - `pairwise(iterable)` - Successive overlapping pairs
//! - `batched(iterable, n)` - Batch elements into tuples of size n
//! - `groupby(iterable, key=None)` - Group consecutive equal elements

use std::{borrow::Cow, mem};

use smallvec::SmallVec;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    exception_public::Exception,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::PrintWriter,
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, List, OurosIter, PyTrait, TeeState, allocate_tuple},
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

/// Inline capacity for small argument lists.
const INLINE_CAPACITY: usize = 3;

/// Itertools module functions.
///
/// Each variant maps to a Python `itertools` function. Functions that return
/// infinite iterators (`Count`, `Cycle`) produce lazy `HeapData::Iter` values;
/// all others eagerly return lists or lists of tuples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum ItertoolsFunctions {
    Chain,
    #[strum(serialize = "chain.from_iterable")]
    ChainFromIterable,
    Tee,
    Islice,
    #[strum(serialize = "zip_longest")]
    ZipLongest,
    Product,
    Permutations,
    Combinations,
    #[strum(serialize = "combinations_with_replacement")]
    CombinationsWithReplacement,
    Repeat,
    Count,
    Cycle,
    Accumulate,
    Starmap,
    Filterfalse,
    Takewhile,
    Dropwhile,
    Compress,
    Pairwise,
    Batched,
    Groupby,
}

/// Creates the `itertools` module and allocates it on the heap.
///
/// Sets up all itertools functions as module attributes.
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

    let mut module = Module::new(StaticStrings::Itertools);

    // chain(*iterables) - concatenate iterables
    module.set_attr(
        StaticStrings::ItChain,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Chain)),
        heap,
        interns,
    );

    // tee(iterable, n=2) - clone iterator into n independent iterators
    module.set_attr(
        StaticStrings::ItTee,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Tee)),
        heap,
        interns,
    );

    // islice(iterable, stop) or islice(iterable, start, stop)
    module.set_attr(
        StaticStrings::Islice,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Islice)),
        heap,
        interns,
    );

    // zip_longest(*iterables, fillvalue=None)
    module.set_attr(
        StaticStrings::ZipLongest,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::ZipLongest)),
        heap,
        interns,
    );

    // product(*iterables)
    module.set_attr(
        StaticStrings::ItProduct,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Product)),
        heap,
        interns,
    );

    // permutations(iterable, r=None)
    module.set_attr(
        StaticStrings::Permutations,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Permutations)),
        heap,
        interns,
    );

    // combinations(iterable, r)
    module.set_attr(
        StaticStrings::Combinations,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Combinations)),
        heap,
        interns,
    );

    // combinations_with_replacement(iterable, r)
    module.set_attr(
        StaticStrings::CombinationsWithReplacement,
        Value::ModuleFunction(ModuleFunctions::Itertools(
            ItertoolsFunctions::CombinationsWithReplacement,
        )),
        heap,
        interns,
    );

    // repeat(elem, times)
    module.set_attr(
        StaticStrings::ItRepeat,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Repeat)),
        heap,
        interns,
    );

    // count(start=0, step=1) - infinite counter
    module.set_attr(
        StaticStrings::Count,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Count)),
        heap,
        interns,
    );

    // cycle(iterable) - infinite repeater
    module.set_attr(
        StaticStrings::ItCycle,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Cycle)),
        heap,
        interns,
    );

    // accumulate(iterable, func=operator.add)
    module.set_attr(
        StaticStrings::Accumulate,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Accumulate)),
        heap,
        interns,
    );

    // starmap(function, iterable)
    module.set_attr(
        StaticStrings::Starmap,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Starmap)),
        heap,
        interns,
    );

    // filterfalse(predicate, iterable)
    module.set_attr(
        StaticStrings::Filterfalse,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Filterfalse)),
        heap,
        interns,
    );

    // takewhile(predicate, iterable)
    module.set_attr(
        StaticStrings::Takewhile,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Takewhile)),
        heap,
        interns,
    );

    // dropwhile(predicate, iterable)
    module.set_attr(
        StaticStrings::Dropwhile,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Dropwhile)),
        heap,
        interns,
    );

    // compress(data, selectors)
    module.set_attr(
        StaticStrings::Compress,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Compress)),
        heap,
        interns,
    );

    // pairwise(iterable)
    module.set_attr(
        StaticStrings::Pairwise,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Pairwise)),
        heap,
        interns,
    );

    // batched(iterable, n)
    module.set_attr(
        StaticStrings::Batched,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Batched)),
        heap,
        interns,
    );

    // groupby(iterable, key=None)
    module.set_attr(
        StaticStrings::Groupby,
        Value::ModuleFunction(ModuleFunctions::Itertools(ItertoolsFunctions::Groupby)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to an itertools module function.
///
/// All itertools functions return immediate values (no host involvement needed).
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ItertoolsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    if matches!(
        function,
        ItertoolsFunctions::Groupby
            | ItertoolsFunctions::Starmap
            | ItertoolsFunctions::Filterfalse
            | ItertoolsFunctions::Takewhile
            | ItertoolsFunctions::Dropwhile
    ) {
        return match function {
            ItertoolsFunctions::Groupby => itertools_groupby(heap, interns, args),
            ItertoolsFunctions::Starmap => itertools_starmap(heap, interns, args),
            ItertoolsFunctions::Filterfalse => itertools_filterfalse(heap, interns, args),
            ItertoolsFunctions::Takewhile => itertools_takewhile(heap, interns, args),
            ItertoolsFunctions::Dropwhile => itertools_dropwhile(heap, interns, args),
            _ => unreachable!("handled by match guard"),
        };
    }

    let result = match function {
        ItertoolsFunctions::Chain => itertools_chain(heap, interns, args),
        ItertoolsFunctions::ChainFromIterable => itertools_chain_from_iterable(heap, interns, args),
        ItertoolsFunctions::Tee => itertools_tee(heap, interns, args),
        ItertoolsFunctions::Islice => itertools_islice(heap, interns, args),
        ItertoolsFunctions::ZipLongest => itertools_zip_longest(heap, interns, args),
        ItertoolsFunctions::Product => itertools_product(heap, interns, args),
        ItertoolsFunctions::Permutations => itertools_permutations(heap, interns, args),
        ItertoolsFunctions::Combinations => itertools_combinations(heap, interns, args),
        ItertoolsFunctions::CombinationsWithReplacement => itertools_combinations_with_replacement(heap, interns, args),
        ItertoolsFunctions::Repeat => itertools_repeat(heap, interns, args),
        ItertoolsFunctions::Count => itertools_count(heap, interns, args),
        ItertoolsFunctions::Cycle => itertools_cycle(heap, interns, args),
        ItertoolsFunctions::Accumulate => itertools_accumulate(heap, interns, args),
        ItertoolsFunctions::Starmap
        | ItertoolsFunctions::Filterfalse
        | ItertoolsFunctions::Takewhile
        | ItertoolsFunctions::Dropwhile => unreachable!("handled in early return"),
        ItertoolsFunctions::Compress => itertools_compress(heap, interns, args),
        ItertoolsFunctions::Pairwise => itertools_pairwise(heap, interns, args),
        ItertoolsFunctions::Batched => itertools_batched(heap, interns, args),
        ItertoolsFunctions::Groupby => unreachable!("groupby handled in early return"),
    }?;
    Ok(AttrCallResult::Value(result))
}

// ============================================================
// Original functions
// ============================================================

/// Implementation of `itertools.chain(*iterables)`.
///
/// Concatenates multiple iterables into a single list.
fn itertools_chain(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);

    // Collect all items from all iterables
    let mut result: Vec<Value> = Vec::new();

    for iterable in pos {
        let mut iter = OurosIter::new(iterable, heap, interns)?;
        // Collect items - these are newly created values with correct refcounts
        let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
        // iter holds the original iterable, need to drop it
        iter.drop_with_heap(heap);

        // items contains owned values - transfer ownership to result
        result.extend(items.into_iter());
    }
    // pos is fully consumed above

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Implementation of `itertools.chain.from_iterable(iterable_of_iterables)`.
///
/// Flattens one level of nesting from a single outer iterable.
fn itertools_chain_from_iterable(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let iterable_of_iterables = args.get_one_arg("chain.from_iterable", heap)?;

    let mut outer_iter = OurosIter::new(iterable_of_iterables, heap, interns)?;
    let outer_items: SmallVec<[Value; INLINE_CAPACITY]> = outer_iter.collect(heap, interns)?;
    outer_iter.drop_with_heap(heap);

    let chain_args = vec_to_arg_values(outer_items.into_iter().collect());
    itertools_chain(heap, interns, chain_args)
}

/// Implementation of `itertools.tee(iterable, n=2)`.
///
/// Returns a tuple of `n` independent iterators that share a buffer.
fn itertools_tee(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (iterable, n_val) = args.get_one_two_args("tee", heap)?;

    let n = match n_val {
        Some(v) => {
            let n_i64 = match v.as_int(heap) {
                Ok(value) => value,
                Err(err) => {
                    v.drop_with_heap(heap);
                    iterable.drop_with_heap(heap);
                    return Err(err);
                }
            };
            v.drop_with_heap(heap);
            if n_i64 < 0 {
                iterable.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "n must be >= 0").into());
            }
            usize::try_from(n_i64).unwrap_or(usize::MAX)
        }
        None => 2,
    };

    if n == 0 {
        iterable.drop_with_heap(heap);
        return Ok(allocate_tuple(SmallVec::new(), heap)?);
    }

    let source_iter = iter_from_value(iterable, heap, interns)?;
    let tee_state = TeeState::new(source_iter, n);
    let tee_id = heap.allocate(HeapData::Tee(tee_state))?;

    let mut iterators: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::with_capacity(n);
    for slot in 0..n {
        if slot > 0 {
            heap.inc_ref(tee_id);
        }
        let tee_ref = Value::Ref(tee_id);
        let iter = OurosIter::new_tee(tee_ref, tee_id, slot);
        let iter_id = heap.allocate(HeapData::Iter(iter))?;
        iterators.push(Value::Ref(iter_id));
    }

    Ok(allocate_tuple(iterators, heap)?)
}

/// Convert a Vec<Value> to ArgValues.
fn vec_to_arg_values(args: Vec<Value>) -> ArgValues {
    match args.len() {
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
    }
}

/// Implementation of `itertools.islice(iterable, stop)` or `islice(iterable, start, stop)`.
///
/// Returns a sliced list from the iterable. Uses lazy `for_next()` iteration
/// so it works correctly with infinite iterators like `count()` and `cycle()`.
fn itertools_islice(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("islice() takes no keyword arguments"));
    }
    kwargs.drop_with_heap(heap);

    // Get the iterable
    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("islice", 2, 0));
    };

    // Get start/stop/step arguments
    let first = pos.next();
    let second = pos.next();
    let third = pos.next();

    // Check for extra arguments
    if let Some(extra) = pos.next() {
        extra.drop_with_heap(heap);
        for v in pos {
            v.drop_with_heap(heap);
        }
        iterable.drop_with_heap(heap);
        if let Some(v) = first {
            v.drop_with_heap(heap);
        }
        if let Some(v) = second {
            v.drop_with_heap(heap);
        }
        if let Some(v) = third {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("islice", 4, 5));
    }
    pos.drop_with_heap(heap); // Drop remaining iterator

    // Parse start, stop, and step.
    let (start, stop, step) = match (first, second, third) {
        (Some(stop), None, None) => {
            // islice(iterable, stop) - start is 0
            let stop_opt = match stop {
                Value::None => None,
                value => {
                    let stop_i64 = value.as_int(heap)?;
                    if stop_i64 < 0 {
                        value.drop_with_heap(heap);
                        iterable.drop_with_heap(heap);
                        return Err(
                            SimpleException::new_msg(ExcType::ValueError, "stop index must be non-negative").into(),
                        );
                    }
                    let converted = usize::try_from(stop_i64).unwrap_or(usize::MAX);
                    value.drop_with_heap(heap);
                    Some(converted)
                }
            };
            (0usize, stop_opt, 1usize)
        }
        (Some(start), Some(stop), step_value) => {
            let start = match start {
                Value::None => 0usize,
                value => {
                    let start_i64 = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    if start_i64 < 0 {
                        iterable.drop_with_heap(heap);
                        if let Some(step_value) = step_value {
                            step_value.drop_with_heap(heap);
                        }
                        stop.drop_with_heap(heap);
                        return Err(
                            SimpleException::new_msg(ExcType::ValueError, "start index must be non-negative").into(),
                        );
                    }
                    usize::try_from(start_i64).unwrap_or(usize::MAX)
                }
            };

            let stop = match stop {
                Value::None => None,
                value => {
                    let stop_i64 = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    if stop_i64 < 0 {
                        iterable.drop_with_heap(heap);
                        if let Some(step_value) = step_value {
                            step_value.drop_with_heap(heap);
                        }
                        return Err(
                            SimpleException::new_msg(ExcType::ValueError, "stop index must be non-negative").into(),
                        );
                    }
                    Some(usize::try_from(stop_i64).unwrap_or(usize::MAX))
                }
            };

            let step = match step_value {
                None | Some(Value::None) => 1usize,
                Some(value) => {
                    let step_i64 = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    if step_i64 <= 0 {
                        iterable.drop_with_heap(heap);
                        return Err(SimpleException::new_msg(
                            ExcType::ValueError,
                            "step index must be a positive integer",
                        )
                        .into());
                    }
                    usize::try_from(step_i64).unwrap_or(usize::MAX)
                }
            };

            (start, stop, step)
        }
        (None, _, _) => {
            // Should not happen due to the check above
            iterable.drop_with_heap(heap);
            return Err(ExcType::type_error_at_least("islice", 2, 1));
        }
        (Some(stop), None, Some(step)) => {
            stop.drop_with_heap(heap);
            step.drop_with_heap(heap);
            iterable.drop_with_heap(heap);
            return Err(ExcType::type_error_at_least("islice", 3, 2));
        }
    };

    // Use lazy for_next() iteration so infinite iterators work correctly.
    // Supports step slicing by tracking the current element index.
    let mut iter = iter_from_value(iterable, heap, interns)?;

    let mut result: Vec<Value> = Vec::new();
    let mut index = 0usize;
    loop {
        if stop.is_some_and(|stop_index| index >= stop_index) {
            break;
        }
        match iter.for_next(heap, interns)? {
            Some(v) => {
                if index >= start && (index - start).is_multiple_of(step) {
                    result.push(v);
                } else {
                    v.drop_with_heap(heap);
                }
                index = index.saturating_add(1);
            }
            None => break,
        }
    }

    iter.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Implementation of `itertools.zip_longest(*iterables, fillvalue=None)`.
///
/// Zips iterables together, padding shorter ones with fillvalue.
fn itertools_zip_longest(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (pos, kwargs) = args.into_parts();

    // Extract fillvalue from kwargs (default None)
    let fillvalue = extract_fillvalue(kwargs, heap, interns)?;

    // Collect all iterables
    let iterables: SmallVec<[Value; INLINE_CAPACITY]> = pos.collect();
    // pos is consumed by collect

    if iterables.is_empty() {
        fillvalue.drop_with_heap(heap);
        let list = List::new(Vec::new());
        let heap_id = heap.allocate(HeapData::List(list))?;
        return Ok(Value::Ref(heap_id));
    }

    // Collect items from all iterables
    let mut all_items: Vec<Vec<Value>> = Vec::with_capacity(iterables.len());
    for iterable in iterables {
        let mut iter = OurosIter::new(iterable, heap, interns)?;
        let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
        iter.drop_with_heap(heap);

        // Transfer ownership to vec
        let items_vec: Vec<Value> = items.into_iter().collect();
        all_items.push(items_vec);
    }

    // Find the maximum length
    let max_len = all_items.iter().map(Vec::len).max().unwrap_or(0);

    // Create tuples for each position
    let mut result: Vec<Value> = Vec::with_capacity(max_len);
    for i in 0..max_len {
        let mut tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::with_capacity(all_items.len());
        for items in &all_items {
            if i < items.len() {
                tuple_items.push(items[i].clone_with_heap(heap));
            } else {
                tuple_items.push(fillvalue.clone_with_heap(heap));
            }
        }
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
    }

    // Drop all_items and fillvalue
    for items in all_items {
        for item in items {
            item.drop_with_heap(heap);
        }
    }
    fillvalue.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Extracts the fillvalue from kwargs.
///
/// Returns Value::None if not provided or if fillvalue=None.
fn extract_fillvalue(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    use crate::args::KwargsValues;

    let mut fillvalue = Value::None;

    match kwargs {
        KwargsValues::Empty => {}
        KwargsValues::Inline(kvs) => {
            for (key_id, value) in kvs {
                let key = interns.get_str(key_id);
                if key == "fillvalue" {
                    fillvalue = value;
                } else {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "zip_longest() got an unexpected keyword argument '{key}'"
                    )));
                }
            }
        }
        KwargsValues::Dict(dict) => {
            let kv_pairs: Vec<(Value, Value)> = dict.into_iter().collect();
            for (key, value) in kv_pairs {
                let key_str = key
                    .as_either_str(heap)
                    .ok_or_else(|| ExcType::type_error("keywords must be strings"))?;
                let key_name = key_str.as_str(interns);
                key.drop_with_heap(heap);
                if key_name == "fillvalue" {
                    fillvalue = value;
                } else {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "zip_longest() got an unexpected keyword argument '{key_name}'"
                    )));
                }
            }
        }
    }

    Ok(fillvalue)
}

/// Implementation of `itertools.product(*iterables)`.
///
/// Returns the cartesian product of iterables as a list of tuples.
fn itertools_product(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos, kwargs) = args.into_parts();
    let repeat = extract_product_repeat(kwargs, heap, interns)?;

    // Collect all iterables
    let iterables: SmallVec<[Value; INLINE_CAPACITY]> = pos.collect();
    // pos is consumed by collect

    if repeat == 0 {
        for iterable in iterables {
            iterable.drop_with_heap(heap);
        }
        let empty_tuple = allocate_tuple(SmallVec::new(), heap)?;
        let list = List::new(vec![empty_tuple]);
        let heap_id = heap.allocate(HeapData::List(list))?;
        return Ok(Value::Ref(heap_id));
    }

    if iterables.is_empty() {
        // product() with no arguments returns a list containing an empty tuple
        let empty_tuple = allocate_tuple(SmallVec::new(), heap)?;
        let list = List::new(vec![empty_tuple]);
        let heap_id = heap.allocate(HeapData::List(list))?;
        return Ok(Value::Ref(heap_id));
    }

    // Collect items from all iterables
    let mut all_items: Vec<Vec<Value>> = Vec::with_capacity(iterables.len());
    for iterable in iterables {
        let mut iter = OurosIter::new(iterable, heap, interns)?;
        let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
        iter.drop_with_heap(heap);

        // Transfer ownership to vec
        let items_vec: Vec<Value> = items.into_iter().collect();
        all_items.push(items_vec);
    }

    if repeat > 1 {
        let mut repeated_items: Vec<Vec<Value>> = Vec::with_capacity(all_items.len() * repeat);
        for _ in 0..repeat {
            for pool in &all_items {
                repeated_items.push(pool.iter().map(|value| value.clone_with_heap(heap)).collect());
            }
        }
        for pool in all_items {
            for value in pool {
                value.drop_with_heap(heap);
            }
        }
        all_items = repeated_items;
    }

    // Compute cartesian product
    let result = cartesian_product(&all_items, heap)?;

    // Drop all_items
    for items in all_items {
        for item in items {
            item.drop_with_heap(heap);
        }
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Extracts the optional `repeat` keyword for `itertools.product`.
fn extract_product_repeat(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<usize> {
    let mut repeat = 1usize;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap).map(|name| name.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        if key_name != "repeat" {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("product", &key_name));
        }

        let repeat_i64 = value.as_int(heap)?;
        value.drop_with_heap(heap);
        if repeat_i64 < 0 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "repeat argument cannot be negative").into());
        }
        repeat = usize::try_from(repeat_i64).unwrap_or(usize::MAX);
    }
    Ok(repeat)
}

/// Computes the cartesian product of a list of item lists.
///
/// Returns a vector of tuple Values.
fn cartesian_product(
    all_items: &[Vec<Value>],
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Vec<Value>, crate::resource::ResourceError> {
    if all_items.is_empty() {
        return Ok(vec![]);
    }

    // Calculate total product size
    let total: usize = all_items.iter().map(Vec::len).product();
    let mut result: Vec<Value> = Vec::with_capacity(total);

    // Generate all combinations
    generate_product_recursive(all_items, 0, &mut SmallVec::new(), &mut result, heap)?;

    Ok(result)
}

/// Recursively generates the cartesian product.
fn generate_product_recursive(
    all_items: &[Vec<Value>],
    depth: usize,
    current: &mut SmallVec<[Value; INLINE_CAPACITY]>,
    result: &mut Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<(), crate::resource::ResourceError> {
    if depth == all_items.len() {
        // Create tuple from current combination
        let tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = current.iter().map(|v| v.clone_with_heap(heap)).collect();
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
        return Ok(());
    }

    for item in &all_items[depth] {
        current.push(item.clone_with_heap(heap));
        generate_product_recursive(all_items, depth + 1, current, result, heap)?;
        current.pop().drop_with_heap(heap);
    }

    Ok(())
}

/// Implementation of `itertools.permutations(iterable, r=None)`.
///
/// Returns all r-length permutations of elements from the iterable.
fn itertools_permutations(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);

    // Get the iterable
    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("permutations", 1, 0));
    };

    // Get optional r argument
    let r_val = pos.next();

    // Check for extra arguments
    if let Some(extra) = pos.next() {
        extra.drop_with_heap(heap);
        for v in pos {
            v.drop_with_heap(heap);
        }
        iterable.drop_with_heap(heap);
        if let Some(v) = r_val {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("permutations", 2, 3));
    }
    pos.drop_with_heap(heap);

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    // Transfer ownership to pool
    let pool: Vec<Value> = items.into_iter().collect();

    let n = pool.len();

    // Parse r argument
    let r = match r_val {
        Some(v) => {
            let r_i64 = v.as_int(heap)?;
            v.drop_with_heap(heap);
            if r_i64 < 0 {
                // Drop pool before returning error
                for item in pool {
                    item.drop_with_heap(heap);
                }
                return Err(SimpleException::new_msg(ExcType::ValueError, "r must be non-negative").into());
            }
            usize::try_from(r_i64).unwrap_or(usize::MAX)
        }
        None => n,
    };

    // Generate permutations
    let result = generate_permutations(&pool, r, heap)?;

    // Drop pool
    for item in pool {
        item.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Generates all r-length permutations of the pool.
fn generate_permutations(
    pool: &[Value],
    r: usize,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Vec<Value>, crate::resource::ResourceError> {
    if r > pool.len() {
        return Ok(vec![]);
    }

    let mut result: Vec<Value> = Vec::new();
    let mut used = vec![false; pool.len()];
    let mut current: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::with_capacity(r);

    permutations_recursive(pool, r, &mut used, &mut current, &mut result, heap)?;

    Ok(result)
}

/// Recursive helper for generating permutations.
fn permutations_recursive(
    pool: &[Value],
    r: usize,
    used: &mut [bool],
    current: &mut SmallVec<[Value; INLINE_CAPACITY]>,
    result: &mut Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<(), crate::resource::ResourceError> {
    if current.len() == r {
        let tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = current.iter().map(|v| v.clone_with_heap(heap)).collect();
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
        return Ok(());
    }

    for i in 0..pool.len() {
        if !used[i] {
            used[i] = true;
            current.push(pool[i].clone_with_heap(heap));
            permutations_recursive(pool, r, used, current, result, heap)?;
            current.pop().drop_with_heap(heap);
            used[i] = false;
        }
    }

    Ok(())
}

/// Implementation of `itertools.combinations(iterable, r)`.
///
/// Returns all r-length combinations of elements from the iterable.
fn itertools_combinations(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (iterable, r_val) = args.get_one_two_args("combinations", heap)?;

    let Some(r_val) = r_val else {
        iterable.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("combinations", 2, 1));
    };

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    // Transfer ownership to pool
    let pool: Vec<Value> = items.into_iter().collect();

    // Parse r argument
    let r_i64 = r_val.as_int(heap)?;
    r_val.drop_with_heap(heap);
    if r_i64 < 0 {
        for item in pool {
            item.drop_with_heap(heap);
        }
        return Err(SimpleException::new_msg(ExcType::ValueError, "r must be non-negative").into());
    }
    let r = usize::try_from(r_i64).unwrap_or(usize::MAX);

    // Generate combinations
    let result = generate_combinations(&pool, r, heap)?;

    // Drop pool
    for item in pool {
        item.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Generates all r-length combinations of the pool.
fn generate_combinations(
    pool: &[Value],
    r: usize,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Vec<Value>, crate::resource::ResourceError> {
    if r > pool.len() {
        return Ok(vec![]);
    }

    let mut result: Vec<Value> = Vec::new();
    let mut current: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::with_capacity(r);

    combinations_recursive(pool, r, 0, &mut current, &mut result, heap)?;

    Ok(result)
}

/// Recursive helper for generating combinations.
fn combinations_recursive(
    pool: &[Value],
    r: usize,
    start: usize,
    current: &mut SmallVec<[Value; INLINE_CAPACITY]>,
    result: &mut Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<(), crate::resource::ResourceError> {
    if current.len() == r {
        let tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = current.iter().map(|v| v.clone_with_heap(heap)).collect();
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
        return Ok(());
    }

    for i in start..pool.len() {
        current.push(pool[i].clone_with_heap(heap));
        combinations_recursive(pool, r, i + 1, current, result, heap)?;
        current.pop().drop_with_heap(heap);
    }

    Ok(())
}

/// Implementation of `itertools.repeat(elem, times)`.
///
/// Returns a list with elem repeated times times.
fn itertools_repeat(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (elem, times_val) = args.get_one_two_args("repeat", heap)?;

    let Some(times_val) = times_val else {
        elem.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("repeat", 2, 1));
    };

    // Parse times argument
    let times_i64 = times_val.as_int(heap)?;
    times_val.drop_with_heap(heap);
    if times_i64 < 0 {
        elem.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "times must be non-negative").into());
    }
    let times = usize::try_from(times_i64).unwrap_or(usize::MAX);

    // Create repeated list
    let mut result: Vec<Value> = Vec::with_capacity(times);
    for _ in 0..times {
        result.push(elem.clone_with_heap(heap));
    }

    elem.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

// ============================================================
// New functions
// ============================================================

/// Implementation of `itertools.count(start=0, step=1)`.
///
/// Returns an infinite iterator that yields `start`, `start + step`, `start + 2*step`, ...
/// Must be consumed with `islice` or similar to avoid infinite loops.
fn itertools_count(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    let mut start_arg = pos.next();
    let mut step_arg = pos.next();
    let positional_count = usize::from(start_arg.is_some()) + usize::from(step_arg.is_some()) + pos.len();
    if positional_count > 2 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        start_arg.drop_with_heap(heap);
        step_arg.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("count", 2, positional_count));
    }
    pos.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap).map(|name| name.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            start_arg.drop_with_heap(heap);
            step_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "start" => {
                if start_arg.is_some() {
                    value.drop_with_heap(heap);
                    start_arg.drop_with_heap(heap);
                    step_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("count", "start"));
                }
                start_arg = Some(value);
            }
            "step" => {
                if step_arg.is_some() {
                    value.drop_with_heap(heap);
                    start_arg.drop_with_heap(heap);
                    step_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("count", "step"));
                }
                step_arg = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                start_arg.drop_with_heap(heap);
                step_arg.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("count", &key_name));
            }
        }
    }

    let start = match start_arg {
        Some(value) => match normalize_count_numeric(value, heap) {
            Ok(v) => v,
            Err(err) => {
                step_arg.drop_with_heap(heap);
                return Err(err);
            }
        },
        None => Value::Int(0),
    };
    let step = match step_arg {
        Some(value) => match normalize_count_numeric(value, heap) {
            Ok(v) => v,
            Err(err) => {
                start.drop_with_heap(heap);
                return Err(err);
            }
        },
        None => Value::Int(1),
    };

    let iter = OurosIter::new_count(start, step);
    let heap_id = heap.allocate(HeapData::Iter(iter))?;
    Ok(Value::Ref(heap_id))
}

/// Normalizes `count()` operands to immediate numeric values used by the iterator.
///
/// Supports ints and floats directly, bools as ints, and LongInt values that fit in i64.
fn normalize_count_numeric(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    match value {
        Value::Int(_) | Value::Float(_) => Ok(value),
        Value::Bool(b) => Ok(Value::Int(i64::from(b))),
        other => {
            let parsed = other.as_int(heap);
            other.drop_with_heap(heap);
            parsed.map(Value::Int)
        }
    }
}

/// Implementation of `itertools.cycle(iterable)`.
///
/// Returns an infinite iterator that repeats the items from `iterable` indefinitely.
/// Must be consumed with `islice` or similar to avoid infinite loops.
fn itertools_cycle(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);

    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("cycle", 1, 0));
    };

    // Drop any extra args
    for v in pos {
        v.drop_with_heap(heap);
    }

    // Eagerly collect items from the iterable as a snapshot
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    let items_vec: Vec<Value> = items.into_iter().collect();
    let cycle_iter = OurosIter::new_cycle(items_vec);
    let heap_id = heap.allocate(HeapData::Iter(cycle_iter))?;
    Ok(Value::Ref(heap_id))
}

/// Implementation of `itertools.accumulate(iterable, func=operator.add, *, initial=None)`.
///
/// Returns a list of running totals (or running results of `func`).
/// When `func` is `None`, defaults to addition (`operator.add`).
/// If `initial` is provided, it is yielded first and used as the initial accumulator.
///
/// Supports `Builtin` and `ModuleFunction` callables (e.g. `max`, `operator.add`).
/// User-defined functions are not supported in this eager implementation.
fn itertools_accumulate(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();

    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("accumulate", 1, 0));
    };

    // Optional func argument (default: addition).
    let func = pos.next();

    let positional_count = 1 + usize::from(func.is_some()) + pos.len();
    if positional_count > 2 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        iterable.drop_with_heap(heap);
        func.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("accumulate", 2, positional_count));
    }
    pos.drop_with_heap(heap);

    let initial = extract_accumulate_initial(kwargs, heap, interns)?;

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    let mut result: Vec<Value> = Vec::new();
    let mut acc = initial;
    if let Some(initial_value) = acc.as_ref() {
        result.push(initial_value.clone_with_heap(heap));
    }

    let mut items_iter = items.into_iter().collect::<Vec<Value>>().into_iter();
    while let Some(item) = items_iter.next() {
        if let Some(previous) = acc.take() {
            match call_binary_func(func.as_ref(), previous, item, heap, interns) {
                Ok(next) => {
                    result.push(next.clone_with_heap(heap));
                    acc = Some(next);
                }
                Err(err) => {
                    items_iter.drop_with_heap(heap);
                    result.drop_with_heap(heap);
                    acc.drop_with_heap(heap);
                    func.drop_with_heap(heap);
                    return Err(err);
                }
            }
        } else {
            result.push(item.clone_with_heap(heap));
            acc = Some(item);
        }
    }

    acc.drop_with_heap(heap);
    func.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Extracts the optional `initial` keyword argument for `accumulate()`.
fn extract_accumulate_initial(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let mut initial = None;

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap).map(|name| name.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            initial.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        if key_name == "initial" {
            initial.drop_with_heap(heap);
            initial = Some(value);
        } else {
            value.drop_with_heap(heap);
            initial.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("accumulate", &key_name));
        }
    }

    Ok(initial)
}

/// Calls a binary function (or defaults to addition if `func` is `None`).
///
/// Supports `Builtin` and `ModuleFunction` callables. Consumes both `a` and `b`.
fn call_binary_func(
    func: Option<&Value>,
    a: Value,
    b: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match func {
        None | Some(Value::None) => {
            // Default: addition
            let lhs_type = a.py_type(heap);
            let rhs_type = b.py_type(heap);
            let result = match a.py_add(&b, heap, interns) {
                Ok(Some(value)) => Ok(value),
                Ok(None) => Err(ExcType::binary_type_error("+", lhs_type, rhs_type)),
                Err(err) => Err(err.into()),
            };
            a.drop_with_heap(heap);
            b.drop_with_heap(heap);
            result
        }
        Some(Value::Builtin(builtin)) => {
            let call_args = ArgValues::Two(a, b);
            builtin.call(heap, call_args, interns, &mut DummyPrint)
        }
        Some(Value::ModuleFunction(mf)) => {
            let call_args = ArgValues::Two(a, b);
            let result = mf.call(heap, interns, call_args)?;
            match result {
                AttrCallResult::Value(v) => Ok(v),
                _ => Err(ExcType::type_error("accumulate() function returned unsupported result")),
            }
        }
        Some(_) => {
            let callable_type = func.expect("matched Some(_) above").py_type(heap);
            a.drop_with_heap(heap);
            b.drop_with_heap(heap);
            Err(ExcType::type_error(format!("'{callable_type}' object is not callable")))
        }
    }
}

/// Implementation of `itertools.starmap(function, iterable)`.
///
/// Implementation of `itertools.starmap(func, iterable)`.
///
/// Applies `function` to each element of `iterable`, where each element
/// is unpacked as positional arguments. Equivalent to `[f(*args) for args in iterable]`.
///
/// Supports builtins/module functions directly and user-defined callables via VM `MapCall`.
fn itertools_starmap(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (function, iterable) = args.get_two_args("starmap", heap)?;

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    if is_user_defined_callable(&function, heap) {
        let mut rows: Vec<Vec<Value>> = Vec::with_capacity(items.len());
        for item in items {
            let mut inner_iter = OurosIter::new(item, heap, interns)?;
            let inner_items: SmallVec<[Value; INLINE_CAPACITY]> = inner_iter.collect(heap, interns)?;
            inner_iter.drop_with_heap(heap);
            rows.push(inner_items.into_iter().collect());
        }

        if rows.is_empty() {
            function.drop_with_heap(heap);
            let list_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
            return Ok(AttrCallResult::Value(Value::Ref(list_id)));
        }

        let arg_count = rows[0].len();
        if rows.iter().any(|row| row.len() != arg_count) {
            for row in rows {
                row.drop_with_heap(heap);
            }
            function.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "starmap() argument tuples have inconsistent lengths",
            ));
        }
        if arg_count == 0 {
            for row in rows {
                row.drop_with_heap(heap);
            }
            function.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "starmap() with empty argument tuples is not supported",
            ));
        }

        let mut iterators: Vec<Vec<Value>> = (0..arg_count).map(|_| Vec::with_capacity(rows.len())).collect();
        for row in rows {
            for (index, value) in row.into_iter().enumerate() {
                iterators[index].push(value);
            }
        }
        return Ok(AttrCallResult::MapCall(function, iterators));
    }

    let mut result: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        // Each item should be a tuple/list - unpack as positional args
        let mut inner_iter = OurosIter::new(item, heap, interns)?;
        let inner_items: SmallVec<[Value; INLINE_CAPACITY]> = inner_iter.collect(heap, interns)?;
        inner_iter.drop_with_heap(heap);

        let call_args = vec_to_arg_values(inner_items.into_iter().collect());

        let val = match &function {
            Value::Builtin(builtin) => builtin.call(heap, call_args, interns, &mut DummyPrint)?,
            Value::ModuleFunction(mf) => {
                let call_result = mf.call(heap, interns, call_args)?;
                if let AttrCallResult::Value(v) = call_result {
                    v
                } else {
                    for v in result {
                        v.drop_with_heap(heap);
                    }
                    function.drop_with_heap(heap);
                    return Err(ExcType::type_error("starmap() function returned unsupported result"));
                }
            }
            other => {
                call_args.drop_with_heap(heap);
                for v in result {
                    v.drop_with_heap(heap);
                }
                let type_name = other.py_type(heap);
                function.drop_with_heap(heap);
                return Err(SimpleException::new_msg(
                    ExcType::TypeError,
                    format!("'{type_name}' object is not callable from starmap()"),
                )
                .into());
            }
        };
        result.push(val);
    }

    function.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Implementation of `itertools.filterfalse(predicate, iterable)`.
///
/// Returns elements from iterable where `predicate(element)` is false.
/// If predicate is `None`, keeps falsy elements directly.
///
/// Builtins and module functions are evaluated eagerly in this module.
/// User-defined callables are delegated to the VM via `AttrCallResult::FilterFalseCall`.
fn itertools_filterfalse(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (predicate, iterable) = args.get_two_args("filterfalse", heap)?;

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    if is_user_defined_callable(&predicate, heap) {
        return Ok(AttrCallResult::FilterFalseCall(predicate, items.into_iter().collect()));
    }

    let mut result: Vec<Value> = Vec::new();

    for item in items {
        let is_true = call_predicate(&predicate, &item, heap, interns)?;
        if is_true {
            item.drop_with_heap(heap);
        } else {
            result.push(item);
        }
    }

    predicate.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Implementation of `itertools.takewhile(predicate, iterable)`.
///
/// Returns elements from iterable while `predicate(element)` is true.
/// Stops at the first element where the predicate is false.
///
/// Builtins and module functions are evaluated eagerly in this module.
/// User-defined callables are delegated to the VM via `AttrCallResult::TakeWhileCall`.
fn itertools_takewhile(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (predicate, iterable) = args.get_two_args("takewhile", heap)?;

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    if is_user_defined_callable(&predicate, heap) {
        return Ok(AttrCallResult::TakeWhileCall(predicate, items.into_iter().collect()));
    }

    let mut result: Vec<Value> = Vec::new();
    let mut stopped = false;

    for item in items {
        if stopped {
            item.drop_with_heap(heap);
            continue;
        }
        let is_true = call_predicate(&predicate, &item, heap, interns)?;
        if is_true {
            result.push(item);
        } else {
            item.drop_with_heap(heap);
            stopped = true;
        }
    }

    predicate.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Implementation of `itertools.dropwhile(predicate, iterable)`.
///
/// Drops elements from iterable while `predicate(element)` is true,
/// then returns all remaining elements.
///
/// Builtins and module functions are evaluated eagerly in this module.
/// User-defined callables are delegated to the VM via `AttrCallResult::DropWhileCall`.
fn itertools_dropwhile(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (predicate, iterable) = args.get_two_args("dropwhile", heap)?;

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    if is_user_defined_callable(&predicate, heap) {
        return Ok(AttrCallResult::DropWhileCall(predicate, items.into_iter().collect()));
    }

    let mut result: Vec<Value> = Vec::new();
    let mut dropping = true;

    for item in items {
        if dropping {
            let is_true = call_predicate(&predicate, &item, heap, interns)?;
            if is_true {
                item.drop_with_heap(heap);
                continue;
            }
            dropping = false;
        }
        result.push(item);
    }

    predicate.drop_with_heap(heap);

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Returns true when the callable needs VM frame management.
///
/// User-defined functions and closures can push frames and therefore must be
/// delegated to VM continuation machinery via `AttrCallResult` variants.
fn is_user_defined_callable(callable: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(callable, Value::DefFunction(_))
        || matches!(
            callable,
            Value::Ref(id)
                if matches!(heap.get(*id), HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _))
        )
}

/// Calls a predicate function on a value.
///
/// If predicate is `None`, returns the truthiness of the value.
/// If predicate is callable (builtin or module function), calls it with the
/// item as a single positional argument and returns the truthiness of the result.
fn call_predicate(
    predicate: &Value,
    item: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    match predicate {
        Value::None => Ok(item.py_bool(heap, interns)),
        Value::Builtin(builtin) => {
            let arg = item.clone_with_heap(heap);
            let call_args = ArgValues::One(arg);
            let value = builtin.call(heap, call_args, interns, &mut DummyPrint)?;
            let is_true = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
            Ok(is_true)
        }
        Value::ModuleFunction(mf) => {
            let arg = item.clone_with_heap(heap);
            let call_args = ArgValues::One(arg);
            let call_result = mf.call(heap, interns, call_args)?;
            match call_result {
                AttrCallResult::Value(v) => {
                    let b = v.py_bool(heap, interns);
                    v.drop_with_heap(heap);
                    Ok(b)
                }
                _ => Err(ExcType::type_error("predicate returned unsupported result")),
            }
        }
        _ => {
            let predicate_type = predicate.py_type(heap);
            Err(ExcType::type_error(format!(
                "'{predicate_type}' object is not callable"
            )))
        }
    }
}

/// Implementation of `itertools.compress(data, selectors)`.
///
/// Filters `data` elements based on corresponding `selectors` truthiness.
/// Stops when either `data` or `selectors` is exhausted.
fn itertools_compress(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_val, selectors_val) = args.get_two_args("compress", heap)?;

    // Collect data
    let mut data_iter = OurosIter::new(data_val, heap, interns)?;
    let data: SmallVec<[Value; INLINE_CAPACITY]> = data_iter.collect(heap, interns)?;
    data_iter.drop_with_heap(heap);

    // Collect selectors
    let mut sel_iter = OurosIter::new(selectors_val, heap, interns)?;
    let selectors: SmallVec<[Value; INLINE_CAPACITY]> = sel_iter.collect(heap, interns)?;
    sel_iter.drop_with_heap(heap);

    let mut result: Vec<Value> = Vec::new();
    let min_len = data.len().min(selectors.len());

    let mut data_iter = data.into_iter();
    let mut sel_iter = selectors.into_iter();

    for _ in 0..min_len {
        let d = data_iter.next().unwrap();
        let s = sel_iter.next().unwrap();
        let is_true = s.py_bool(heap, interns);
        s.drop_with_heap(heap);
        if is_true {
            result.push(d);
        } else {
            d.drop_with_heap(heap);
        }
    }

    // Drop remaining data and selectors
    for d in data_iter {
        d.drop_with_heap(heap);
    }
    for s in sel_iter {
        s.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Implementation of `itertools.pairwise(iterable)`.
///
/// Returns successive overlapping pairs from the iterable.
/// `pairwise([1,2,3,4])` → `[(1,2), (2,3), (3,4)]`
fn itertools_pairwise(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);

    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("pairwise", 1, 0));
    };

    for v in pos {
        v.drop_with_heap(heap);
    }

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    if items.len() < 2 {
        // Not enough items for any pairs
        for item in items {
            item.drop_with_heap(heap);
        }
        let list = List::new(Vec::new());
        let heap_id = heap.allocate(HeapData::List(list))?;
        return Ok(Value::Ref(heap_id));
    }

    let mut result: Vec<Value> = Vec::with_capacity(items.len() - 1);
    for window in items.windows(2) {
        let a = window[0].clone_with_heap(heap);
        let b = window[1].clone_with_heap(heap);
        let mut tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::new();
        tuple_items.push(a);
        tuple_items.push(b);
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
    }

    // Drop original items
    for item in items {
        item.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Implementation of `itertools.batched(iterable, n, *, strict=False)`.
///
/// Batches elements from `iterable` into tuples of size `n`.
/// The last batch may be shorter if the iterable is not evenly divisible by `n`.
/// When `strict=True`, a short final batch raises `ValueError`.
fn itertools_batched(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("batched", 2, 0));
    };
    let Some(n_val) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        iterable.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("batched", 2, 1));
    };

    let positional_count = 2 + pos.len();
    if positional_count > 2 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        iterable.drop_with_heap(heap);
        n_val.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("batched", 2, positional_count));
    }
    pos.drop_with_heap(heap);

    let strict = extract_batched_strict(kwargs, heap, interns)?;

    // Parse n
    let n_i64 = n_val.as_int(heap)?;
    n_val.drop_with_heap(heap);
    if n_i64 < 1 {
        iterable.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be at least one").into());
    }
    let n = usize::try_from(n_i64).unwrap_or(usize::MAX);

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    let items_vec: Vec<Value> = items.into_iter().collect();

    if strict && !items_vec.is_empty() && !items_vec.len().is_multiple_of(n) {
        items_vec.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "batched(): incomplete batch").into());
    }

    let mut result: Vec<Value> = Vec::new();
    for chunk in items_vec.chunks(n) {
        let tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = chunk.iter().map(|v| v.clone_with_heap(heap)).collect();
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
    }

    // Drop original items
    for item in items_vec {
        item.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Extracts `strict=` for `batched()` and rejects unknown kwargs.
fn extract_batched_strict(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    let mut strict = false;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap).map(|name| name.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        if key_name == "strict" {
            strict = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
        } else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("batched", &key_name));
        }
    }
    Ok(strict)
}

/// Implementation of `itertools.groupby(iterable, key=None)`.
///
/// Groups consecutive elements by key. Returns a list of `(key, list)` tuples.
/// When `key` is `None`, elements are grouped by identity (equality).
///
/// Note: Unlike Python's `groupby` which returns lazy group iterators, this
/// implementation eagerly collects groups into lists for simplicity.
fn itertools_groupby(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    let Some(iterable) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("groupby", 1, 0));
    };

    // Optional key function (2nd positional).
    let mut key_func = pos.next();
    if pos.len() > 0 {
        for value in pos {
            value.drop_with_heap(heap);
        }
        iterable.drop_with_heap(heap);
        if let Some(key) = key_func {
            key.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("groupby", 2, 3));
    }

    // Optional key function via keyword argument.
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap).map(|name| name.as_str(interns).to_string()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            iterable.drop_with_heap(heap);
            if let Some(existing) = key_func {
                existing.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        if key_name != "key" {
            value.drop_with_heap(heap);
            iterable.drop_with_heap(heap);
            if let Some(existing) = key_func {
                existing.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_unexpected_keyword("groupby", &key_name));
        }

        if let Some(existing) = key_func.replace(value) {
            existing.drop_with_heap(heap);
            iterable.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg("groupby", "key"));
        }
    }

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: Vec<Value> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    if items.is_empty() {
        if let Some(f) = key_func {
            f.drop_with_heap(heap);
        }
        let list = List::new(Vec::new());
        let heap_id = heap.allocate(HeapData::List(list))?;
        return Ok(AttrCallResult::Value(Value::Ref(heap_id)));
    }

    // User-defined key functions require VM call machinery (frames/closures).
    let needs_vm_key_calls = matches!(
        key_func.as_ref(),
        Some(v) if !matches!(v, Value::None | Value::ModuleFunction(_))
    );
    if needs_vm_key_calls {
        if let Some(function) = key_func {
            return Ok(AttrCallResult::GroupByCall(function, items));
        }
        return Err(RunError::internal(
            "groupby key-callable path selected without callable key",
        ));
    }

    // Group consecutive elements for identity key or module-function key.
    let callable_key = key_func.as_ref().filter(|value| !matches!(value, Value::None));
    let mut result: Vec<Value> = Vec::new();
    let mut current_key: Option<Value> = None;
    let mut current_group: Vec<Value> = Vec::new();

    for item in items {
        let item_key = compute_key(callable_key, &item, heap, interns)?;
        let same_group = current_key
            .as_ref()
            .is_some_and(|existing| existing.py_eq(&item_key, heap, interns));

        if same_group {
            item_key.drop_with_heap(heap);
            current_group.push(item);
            continue;
        }

        if let Some(previous_key) = current_key.take() {
            flush_group(&mut result, previous_key, std::mem::take(&mut current_group), heap)?;
        }
        current_key = Some(item_key);
        current_group.push(item);
    }

    if let Some(last_key) = current_key {
        flush_group(&mut result, last_key, current_group, heap)?;
    }

    if let Some(f) = key_func {
        f.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Computes the key for a groupby element.
///
/// If key_func is None, returns a clone of the item.
/// If key_func is a `ModuleFunction`, calls it with the item.
fn compute_key(
    key_func: Option<&Value>,
    item: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match key_func {
        None => Ok(item.clone_with_heap(heap)),
        Some(Value::ModuleFunction(mf)) => {
            let arg = item.clone_with_heap(heap);
            let call_args = ArgValues::One(arg);
            let call_result = mf.call(heap, interns, call_args)?;
            match call_result {
                AttrCallResult::Value(v) => Ok(v),
                _ => Err(ExcType::type_error(
                    "groupby() key function returned unsupported result",
                )),
            }
        }
        Some(_) => Err(ExcType::type_error("groupby() key must be None or a built-in function")),
    }
}

/// Flushes a completed group into the result list as a `(key, [items])` tuple.
fn flush_group(
    result: &mut Vec<Value>,
    key: Value,
    group: Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<(), crate::resource::ResourceError> {
    let group_list = List::new(group);
    let group_heap_id = heap.allocate(HeapData::List(group_list))?;
    let group_val = Value::Ref(group_heap_id);

    let mut tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::new();
    tuple_items.push(key);
    tuple_items.push(group_val);
    let tuple_val = allocate_tuple(tuple_items, heap)?;
    result.push(tuple_val);
    Ok(())
}

/// Implementation of `itertools.combinations_with_replacement(iterable, r)`.
///
/// Returns all r-length combinations of elements from the iterable,
/// allowing individual elements to be repeated.
fn itertools_combinations_with_replacement(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (iterable, r_val) = args.get_two_args("combinations_with_replacement", heap)?;

    // Collect items from iterable
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let items: SmallVec<[Value; INLINE_CAPACITY]> = iter.collect(heap, interns)?;
    iter.drop_with_heap(heap);

    // Transfer ownership to pool
    let pool: Vec<Value> = items.into_iter().collect();

    // Parse r argument
    let r_i64 = r_val.as_int(heap)?;
    r_val.drop_with_heap(heap);
    if r_i64 < 0 {
        for item in pool {
            item.drop_with_heap(heap);
        }
        return Err(SimpleException::new_msg(ExcType::ValueError, "r must be non-negative").into());
    }
    let r = usize::try_from(r_i64).unwrap_or(usize::MAX);

    if pool.is_empty() && r > 0 {
        let list = List::new(Vec::new());
        let heap_id = heap.allocate(HeapData::List(list))?;
        return Ok(Value::Ref(heap_id));
    }

    // Generate combinations with replacement
    let mut result: Vec<Value> = Vec::new();
    let mut current: SmallVec<[Value; INLINE_CAPACITY]> = SmallVec::with_capacity(r);
    combinations_with_replacement_recursive(&pool, r, 0, &mut current, &mut result, heap)?;

    // Drop pool
    for item in pool {
        item.drop_with_heap(heap);
    }

    let list = List::new(result);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Recursive helper for generating combinations with replacement.
///
/// Same as combinations, but allows picking the same index again (start = i, not i + 1).
fn combinations_with_replacement_recursive(
    pool: &[Value],
    r: usize,
    start: usize,
    current: &mut SmallVec<[Value; INLINE_CAPACITY]>,
    result: &mut Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<(), crate::resource::ResourceError> {
    if current.len() == r {
        let tuple_items: SmallVec<[Value; INLINE_CAPACITY]> = current.iter().map(|v| v.clone_with_heap(heap)).collect();
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result.push(tuple_val);
        return Ok(());
    }

    for i in start..pool.len() {
        current.push(pool[i].clone_with_heap(heap));
        // Key difference from combinations: pass `i` instead of `i + 1` to allow repeats
        combinations_with_replacement_recursive(pool, r, i, current, result, heap)?;
        current.pop().drop_with_heap(heap);
    }

    Ok(())
}

// ============================================================
// Helpers
// ============================================================

/// Creates a `OurosIter` from a value, handling existing iterators specially.
///
/// If the value is already a `HeapData::Iter` (e.g. from `count()` or `cycle()`),
/// extracts the iterator from the heap so it can be consumed directly.
/// Otherwise, creates a new `OurosIter` from the value in the normal way.
///
/// This allows functions like `islice` to work with both regular iterables
/// (lists, ranges, strings) and lazy iterators (count, cycle).
fn iter_from_value(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<OurosIter> {
    if let Value::Ref(id) = &value
        && matches!(heap.get(*id), HeapData::Iter(_))
    {
        // Extract the OurosIter from the heap, replacing with a dummy list.
        // This effectively "consumes" the heap Iter so we can iterate it directly
        // without borrow conflicts between for_next() and heap access.
        let heap_data = mem::replace(heap.get_mut(*id), HeapData::List(List::new(Vec::new())));
        // Drop the Value::Ref which decrements refcount; the dummy list will be
        // collected when refcount reaches 0.
        value.drop_with_heap(heap);
        match heap_data {
            HeapData::Iter(iter) => return Ok(iter),
            _ => unreachable!("checked above"),
        }
    }
    OurosIter::new(value, heap, interns)
}
