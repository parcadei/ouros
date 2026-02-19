//! Implementation of the `collections` module.
//!
//! Provides Python's `collections` module with:
//! - `Counter(iterable)`: Count elements from an iterable, return a dict
//! - `counter_most_common(counter, n=None)`: Return elements sorted by count
//! - `counter_elements(counter)`: Return list of elements repeated by count
//! - `counter_subtract(counter, other)`: Subtract counts from another iterable/dict
//! - `counter_update(counter, other)`: Add counts from another iterable/dict
//! - `counter_total(counter)`: Sum of all counts
//! - `namedtuple(typename, field_names)`: Create a namedtuple factory
//! - `defaultdict(default_factory, *args)`: Create a dict (simplified)
//! - `OrderedDict(*args)`: Create a real ordered dict object
//! - `ordereddict_move_to_end(dict, key, last=True)`: Move key to end or start
//! - `ordereddict_popitem(dict, last=True)`: Pop last or first item
//! - `deque(iterable)`: Create a list from an iterable (simplified)
//! - `deque_appendleft(deque, x)`: Append to left end
//! - `deque_popleft(deque)`: Pop from left end
//! - `deque_extendleft(deque, iterable)`: Extend from left
//! - `deque_rotate(deque, n=1)`: Rotate elements by n steps
//! - `ChainMap(*maps)`: Create a `ChainMap` mapping view over multiple dicts
//! - `UserDict([mapping_or_pairs])`: Thin wrapper returning a dict
//! - `UserList([iterable])`: Thin wrapper returning a list
//! - `UserString([object])`: Thin wrapper returning a str
//!
//! This module includes a mixture of full wrapper types (`Counter`,
//! `OrderedDict`, `deque`, `defaultdict`, `ChainMap`, `namedtuple`) and
//! compatibility constructors (`UserDict`, `UserList`, `UserString`) used by
//! stdlib parity tests.

use smallvec::SmallVec;

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{
        AttrCallResult, ChainMap as ChainMapType, Counter as CounterType, DefaultDict, Deque, Dict, List,
        NamedTupleFactory, OrderedDict as OrderedDictType, OurosIter, PyTrait, Str, allocate_tuple,
    },
    value::{EitherStr, Value},
};

/// Collections module functions.
///
/// Each variant maps to a constructor or utility function in the `collections` module.
/// Constructors like `Counter` and `deque` create dict/list objects from iterables.
/// Utility functions like `counter_most_common` operate on those objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum CollectionsFunctions {
    /// `Counter(iterable)` — count elements from an iterable into a dict.
    Counter,
    /// `namedtuple(typename, field_names, *, rename=False, defaults=None, module=None)`.
    #[strum(serialize = "namedtuple")]
    Namedtuple,
    /// `defaultdict(default_factory)` — create a dict (simplified, no __missing__).
    Defaultdict,
    /// `OrderedDict(*args, **kwargs)` — create an ordered dict preserving insertion order.
    Ordereddict,
    /// `deque(iterable)` — create a list from an iterable (simplified deque).
    #[strum(serialize = "deque")]
    Deque,
    /// `ChainMap(*maps)` — merge multiple dicts, first match wins.
    #[strum(serialize = "ChainMap")]
    ChainMap,
    /// `UserDict([mapping_or_pairs])` — thin wrapper returning a dict.
    #[strum(serialize = "UserDict")]
    UserDict,
    /// `UserList([iterable])` — thin wrapper returning a list.
    #[strum(serialize = "UserList")]
    UserList,
    /// `UserString([object])` — thin wrapper returning a str.
    #[strum(serialize = "UserString")]
    UserString,
    /// `counter_most_common(counter, n=None)` — return (elem, count) pairs sorted by count.
    #[strum(serialize = "counter_most_common")]
    CounterMostCommon,
    /// `counter_elements(counter)` — return list of elements repeated by their count.
    #[strum(serialize = "counter_elements")]
    CounterElements,
    /// `counter_subtract(counter, other)` — subtract counts in-place.
    #[strum(serialize = "counter_subtract")]
    CounterSubtract,
    /// `counter_update(counter, other)` — add counts in-place from another iterable/dict.
    #[strum(serialize = "counter_update")]
    CounterUpdate,
    /// `counter_total(counter)` — sum of all count values.
    #[strum(serialize = "counter_total")]
    CounterTotal,
    /// `deque_appendleft(deque, x)` — append to the left end.
    #[strum(serialize = "deque_appendleft")]
    DequeAppendleft,
    /// `deque_popleft(deque)` — pop from the left end.
    #[strum(serialize = "deque_popleft")]
    DequePopleft,
    /// `deque_extendleft(deque, iterable)` — extend from the left.
    #[strum(serialize = "deque_extendleft")]
    DequeExtendleft,
    /// `deque_rotate(deque, n=1)` — rotate elements n steps to the right.
    #[strum(serialize = "deque_rotate")]
    DequeRotate,
    /// `ordereddict_move_to_end(od, key, last=True)` — move key to end or start.
    #[strum(serialize = "ordereddict_move_to_end")]
    OdMoveToEnd,
    /// `ordereddict_popitem(od, last=True)` — pop last or first item.
    #[strum(serialize = "ordereddict_popitem")]
    OdPopitem,
}

/// Creates the `collections` module and allocates it on the heap.
///
/// The module provides constructors for specialized container types and
/// utility functions that operate on the returned dicts/lists.
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

    let mut module = Module::new(StaticStrings::Collections);
    let abc_module_id = super::collections_abc::create_module(heap, interns)?;
    module.set_attr_text("abc", Value::Ref(abc_module_id), heap, interns)?;

    // --- Constructors ---
    register(
        &mut module,
        StaticStrings::Counter,
        CollectionsFunctions::Counter,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollNamedtuple,
        CollectionsFunctions::Namedtuple,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::DefaultDict,
        CollectionsFunctions::Defaultdict,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollOrderedDict,
        CollectionsFunctions::Ordereddict,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::Deque,
        CollectionsFunctions::Deque,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::ChainMap,
        CollectionsFunctions::ChainMap,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollUserDict,
        CollectionsFunctions::UserDict,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollUserList,
        CollectionsFunctions::UserList,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollUserString,
        CollectionsFunctions::UserString,
        heap,
        interns,
    );

    // --- Counter utilities ---
    register(
        &mut module,
        StaticStrings::CollCounterMostCommon,
        CollectionsFunctions::CounterMostCommon,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollCounterElements,
        CollectionsFunctions::CounterElements,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollCounterSubtract,
        CollectionsFunctions::CounterSubtract,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollCounterUpdate,
        CollectionsFunctions::CounterUpdate,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollCounterTotal,
        CollectionsFunctions::CounterTotal,
        heap,
        interns,
    );

    // --- Deque utilities ---
    register(
        &mut module,
        StaticStrings::CollDequeAppendleft,
        CollectionsFunctions::DequeAppendleft,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollDequePopleft,
        CollectionsFunctions::DequePopleft,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollDequeExtendleft,
        CollectionsFunctions::DequeExtendleft,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollDequeRotate,
        CollectionsFunctions::DequeRotate,
        heap,
        interns,
    );

    // --- OrderedDict utilities ---
    register(
        &mut module,
        StaticStrings::CollOdMoveToEnd,
        CollectionsFunctions::OdMoveToEnd,
        heap,
        interns,
    );
    register(
        &mut module,
        StaticStrings::CollOdPopitem,
        CollectionsFunctions::OdPopitem,
        heap,
        interns,
    );

    // __all__ — public API matching CPython's collections namespace.
    // Excludes internal dispatch functions (counter_elements, deque_appendleft, etc.)
    // that are exposed as module-level functions for method dispatch but are not part
    // of the public API.
    let public_names = [
        "ChainMap",
        "Counter",
        "OrderedDict",
        "UserDict",
        "UserList",
        "UserString",
        "defaultdict",
        "deque",
        "namedtuple",
    ];
    let mut all_values = Vec::with_capacity(public_names.len());
    for name in public_names {
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        all_values.push(Value::Ref(name_id));
    }
    let all_id = heap.allocate(HeapData::List(List::new(all_values)))?;
    module.set_attr_str("__all__", Value::Ref(all_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a collections module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: CollectionsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        CollectionsFunctions::Counter => counter(heap, interns, args),
        CollectionsFunctions::Namedtuple => namedtuple(heap, interns, args),
        CollectionsFunctions::Defaultdict => defaultdict(heap, interns, args),
        CollectionsFunctions::Ordereddict => ordereddict(heap, interns, args),
        CollectionsFunctions::Deque => deque(heap, interns, args),
        CollectionsFunctions::ChainMap => chain_map(heap, interns, args),
        CollectionsFunctions::UserDict => userdict(heap, interns, args),
        CollectionsFunctions::UserList => userlist(heap, interns, args),
        CollectionsFunctions::UserString => userstring(heap, interns, args),
        CollectionsFunctions::CounterMostCommon => counter_most_common(heap, interns, args),
        CollectionsFunctions::CounterElements => counter_elements(heap, interns, args),
        CollectionsFunctions::CounterSubtract => counter_subtract(heap, interns, args),
        CollectionsFunctions::CounterUpdate => counter_update(heap, interns, args),
        CollectionsFunctions::CounterTotal => counter_total(heap, interns, args),
        CollectionsFunctions::DequeAppendleft => deque_appendleft(heap, interns, args),
        CollectionsFunctions::DequePopleft => deque_popleft(heap, args),
        CollectionsFunctions::DequeExtendleft => deque_extendleft(heap, interns, args),
        CollectionsFunctions::DequeRotate => deque_rotate(heap, args),
        CollectionsFunctions::OdMoveToEnd => od_move_to_end(heap, interns, args),
        CollectionsFunctions::OdPopitem => od_popitem(heap, interns, args),
    }
}

// ============================================================
// Constructors
// ============================================================

/// Implementation of `collections.Counter(iterable)`.
///
/// Counts elements from an iterable and returns a Counter mapping elements to counts.
/// When called with no arguments, returns an empty counter.
/// Counter returns 0 for missing keys instead of raising KeyError.
fn counter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("collections.Counter", 1, positional_count));
    }

    let mut dict = Dict::new();

    if let Some(iterable) = positional.next() {
        let counts = extract_counts(iterable, heap, interns)?;
        let mut counts_iter = counts.into_iter();
        while let Some((key, count)) = counts_iter.next() {
            let current_count = match dict.get(&key, heap, interns) {
                Ok(Some(Value::Int(n))) => *n,
                Ok(_) => 0,
                Err(err) => {
                    key.drop_with_heap(heap);
                    for (rest_key, _) in counts_iter {
                        rest_key.drop_with_heap(heap);
                    }
                    dict.drop_all_entries(heap);
                    return Err(err);
                }
            };

            let key_clone = key.clone_with_heap(heap);
            let old = match dict.set(key_clone, Value::Int(current_count + count), heap, interns) {
                Ok(old) => old,
                Err(err) => {
                    key.drop_with_heap(heap);
                    for (rest_key, _) in counts_iter {
                        rest_key.drop_with_heap(heap);
                    }
                    dict.drop_all_entries(heap);
                    return Err(err);
                }
            };
            old.drop_with_heap(heap);
            key.drop_with_heap(heap);
        }
    }
    positional.drop_with_heap(heap);

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        if key.as_either_str(heap).is_none() {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            dict.drop_all_entries(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        }

        let count = match value.as_int(heap) {
            Ok(count) => count,
            Err(err) => {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                dict.drop_all_entries(heap);
                return Err(err);
            }
        };
        value.drop_with_heap(heap);

        let current_count = match dict.get(&key, heap, interns) {
            Ok(Some(Value::Int(n))) => *n,
            Ok(_) => 0,
            Err(err) => {
                key.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                dict.drop_all_entries(heap);
                return Err(err);
            }
        };

        let key_clone = key.clone_with_heap(heap);
        let old = match dict.set(key_clone, Value::Int(current_count + count), heap, interns) {
            Ok(old) => old,
            Err(err) => {
                key.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                dict.drop_all_entries(heap);
                return Err(err);
            }
        };
        old.drop_with_heap(heap);
        key.drop_with_heap(heap);
    }

    let counter = CounterType::from_dict(dict);
    let counter_id = heap.allocate(HeapData::Counter(counter))?;
    Ok(AttrCallResult::Value(Value::Ref(counter_id)))
}

/// Implementation of `collections.namedtuple(...)`.
fn namedtuple(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();
    if positional.len() < 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("namedtuple", 2, 0));
    }
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("namedtuple", 2, count));
    }

    let mut positional_iter = positional.into_iter();
    let typename_value = positional_iter.next().expect("validated len");
    let fields_value = positional_iter.next().expect("validated len");

    let typename = typename_value.py_str(heap, interns).into_owned();
    typename_value.drop_with_heap(heap);
    let field_names = parse_namedtuple_field_names(fields_value, heap, interns)?;

    let mut defaults_arg: Option<Value> = None;
    let mut module_arg: Option<Value> = None;
    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            defaults_arg.drop_with_heap(heap);
            module_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "rename" => {
                // We currently accept and ignore rename for valid-field parity coverage.
                value.drop_with_heap(heap);
            }
            "defaults" => {
                if defaults_arg.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    module_arg.drop_with_heap(heap);
                    defaults_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("namedtuple", "defaults"));
                }
                defaults_arg = Some(value);
            }
            "module" => {
                if module_arg.is_some() {
                    value.drop_with_heap(heap);
                    for (rest_key, rest_value) in kwargs_iter {
                        rest_key.drop_with_heap(heap);
                        rest_value.drop_with_heap(heap);
                    }
                    defaults_arg.drop_with_heap(heap);
                    module_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("namedtuple", "module"));
                }
                module_arg = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                for (rest_key, rest_value) in kwargs_iter {
                    rest_key.drop_with_heap(heap);
                    rest_value.drop_with_heap(heap);
                }
                defaults_arg.drop_with_heap(heap);
                module_arg.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("namedtuple", &key_name));
            }
        }
    }

    let defaults = if let Some(defaults_value) = defaults_arg {
        collect_iterable(defaults_value, heap, interns)?
    } else {
        Vec::new()
    };
    if defaults.len() > field_names.len() {
        defaults.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "Got more default values than field names").into());
    }
    let module = if let Some(module_value) = module_arg {
        let module = module_value.py_str(heap, interns).into_owned();
        module_value.drop_with_heap(heap);
        module
    } else {
        "__main__".to_owned()
    };

    let factory = NamedTupleFactory::new_with_options(typename, field_names, defaults, module);
    let factory_id = heap.allocate(HeapData::NamedTupleFactory(factory))?;
    Ok(AttrCallResult::Value(Value::Ref(factory_id)))
}

/// Implementation of `collections.defaultdict(default_factory, *args)`.
///
/// Creates a defaultdict with the given default_factory. The factory is called
/// to provide default values for missing keys.
fn defaultdict(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let factory = args.get_zero_one_arg("collections.defaultdict", heap)?;

    let dd = DefaultDict::new(factory);
    let dd_id = heap.allocate(HeapData::DefaultDict(dd))?;
    Ok(AttrCallResult::Value(Value::Ref(dd_id)))
}

/// Implementation of `collections.OrderedDict(*args)`.
///
/// Creates an `OrderedDict` from mapping/pairs/kwargs constructor inputs.
fn ordereddict(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let dict_value = Dict::init(heap, args, interns)?;
    defer_drop!(dict_value, heap);

    let &Value::Ref(dict_id) = dict_value else {
        unreachable!("dict constructor always returns a heap reference");
    };

    let dict = heap.with_entry_mut(dict_id, |heap_inner, data| match data {
        HeapData::Dict(dict) => dict.clone_with_heap(heap_inner, interns),
        _ => unreachable!("dict constructor must return HeapData::Dict"),
    })?;

    let ordered_dict = OrderedDictType::from_dict(dict);
    let ordered_dict_id = heap.allocate(HeapData::OrderedDict(ordered_dict))?;
    Ok(AttrCallResult::Value(Value::Ref(ordered_dict_id)))
}

/// Implementation of `collections.deque(iterable)`.
///
/// Creates a list from an iterable. This is a simplified implementation
/// that uses a list instead of a proper deque type.
fn deque(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("collections.deque", 1, positional_count));
    }
    let iterable = positional.next();
    positional.drop_with_heap(heap);

    let mut maxlen_arg: Option<Value> = None;
    let mut kwargs_iter = kwargs.into_iter();
    while let Some((kw, value)) = kwargs_iter.next() {
        let Some(kw_name) = kw.as_either_str(heap) else {
            kw.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_kw, rest_value) in kwargs_iter {
                rest_kw.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            maxlen_arg.drop_with_heap(heap);
            if let Some(iterable) = iterable {
                iterable.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };

        if kw_name.as_str(interns) != "maxlen" {
            kw.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_kw, rest_value) in kwargs_iter {
                rest_kw.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            maxlen_arg.drop_with_heap(heap);
            if let Some(iterable) = iterable {
                iterable.drop_with_heap(heap);
            }
            return Err(ExcType::type_error(
                "collections.deque() got an unexpected keyword argument",
            ));
        }
        if maxlen_arg.is_some() {
            kw.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_kw, rest_value) in kwargs_iter {
                rest_kw.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            maxlen_arg.drop_with_heap(heap);
            if let Some(iterable) = iterable {
                iterable.drop_with_heap(heap);
            }
            return Err(ExcType::type_error(
                "collections.deque() got multiple values for argument 'maxlen'",
            ));
        }
        kw.drop_with_heap(heap);
        maxlen_arg = Some(value);
    }

    let maxlen = match maxlen_arg {
        None => None,
        Some(value) => {
            if matches!(value, Value::None) {
                value.drop_with_heap(heap);
                None
            } else {
                let n = value.as_int(heap)?;
                value.drop_with_heap(heap);
                if n < 0 {
                    if let Some(iterable) = iterable {
                        iterable.drop_with_heap(heap);
                    }
                    return Err(SimpleException::new_msg(ExcType::ValueError, "maxlen must be non-negative").into());
                }
                #[expect(clippy::cast_sign_loss, reason = "n is non-negative above")]
                Some(n as usize)
            }
        }
    };

    let mut items = if let Some(iterable) = iterable {
        use std::collections::VecDeque;
        let vec_items = collect_iterable(iterable, heap, interns)?;
        VecDeque::from(vec_items)
    } else {
        std::collections::VecDeque::new()
    };

    if let Some(maxlen) = maxlen {
        while items.len() > maxlen {
            if let Some(removed) = items.pop_front() {
                removed.drop_with_heap(heap);
            }
        }
    }

    let deque = Deque::from_vec_deque_with_maxlen(items, maxlen);
    let deque_id = heap.allocate(HeapData::Deque(deque))?;
    Ok(AttrCallResult::Value(Value::Ref(deque_id)))
}

/// Implementation of `collections.ChainMap(*maps)`.
///
/// Creates a `ChainMap` object storing one or more underlying dict mappings.
fn chain_map(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut maps_iter, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        maps_iter.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("ChainMap() arguments must be dicts".to_string()));
    }

    let mut maps: Vec<Value> = maps_iter.by_ref().collect();
    maps_iter.drop_with_heap(heap);

    if maps.is_empty() {
        let empty_dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
        maps.push(Value::Ref(empty_dict_id));
    }

    for map in &maps {
        let Value::Ref(map_id) = map else {
            maps.drop_with_heap(heap);
            return Err(ExcType::type_error("ChainMap() arguments must be dicts".to_string()));
        };
        if !matches!(heap.get(*map_id), HeapData::Dict(_)) {
            maps.drop_with_heap(heap);
            return Err(ExcType::type_error("ChainMap() arguments must be dicts".to_string()));
        }
    }

    let chain_map = ChainMapType::new(maps, heap, interns)?;
    let chain_map_id = heap.allocate(HeapData::ChainMap(chain_map))?;
    Ok(AttrCallResult::Value(Value::Ref(chain_map_id)))
}

/// Implementation of `collections.UserDict([mapping_or_pairs])`.
///
/// Returns a plain dict for compatibility with code that expects the constructor
/// to exist. If an argument is provided, it can be either a dict or an iterable
/// of key/value pairs.
fn userdict(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("collections.UserDict", 1, positional_count));
    }

    let initial = positional.next();
    positional.drop_with_heap(heap);
    let mut dict = Dict::new();
    dict.set_user_data_attr();

    if let Some(initial) = initial {
        if let Value::Ref(id) = &initial
            && matches!(heap.get(*id), HeapData::Dict(_))
        {
            let pairs = heap.with_entry_mut(*id, |heap_inner, data| {
                if let HeapData::Dict(existing) = data {
                    existing.items(heap_inner)
                } else {
                    Vec::new()
                }
            });
            for (key, value) in pairs {
                if let Some(old) = dict.set(key, value, heap, interns)? {
                    old.drop_with_heap(heap);
                }
            }
            initial.drop_with_heap(heap);
        } else {
            consume_pairs_into_dict(&mut dict, initial, heap, interns)?;
        }
    }

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(_key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            dict.drop_all_entries(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        if let Some(old) = dict.set(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }

    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(AttrCallResult::Value(Value::Ref(dict_id)))
}

/// Implementation of `collections.UserList([iterable])`.
///
/// Returns a plain list for compatibility with code that expects the constructor
/// to exist.
fn userlist(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let iterable = args.get_zero_one_arg("collections.UserList", heap)?;
    let items = if let Some(iterable) = iterable {
        collect_iterable(iterable, heap, interns)?
    } else {
        Vec::new()
    };
    let list_id = heap.allocate(HeapData::List(List::new_user(items)))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Implementation of `collections.UserString([object])`.
///
/// Returns a plain string object containing `str(object)` (or empty string if no
/// argument is provided), matching `UserString` constructor behavior closely enough
/// for stdlib compatibility.
fn userstring(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_zero_one_arg("collections.UserString", heap)?;
    let text = if let Some(value) = value {
        let text = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
        text
    } else {
        String::new()
    };
    let string_id = heap.allocate(HeapData::Str(Str::new_user(text)))?;
    Ok(AttrCallResult::Value(Value::Ref(string_id)))
}

// ============================================================
// Counter utilities
// ============================================================

/// Implementation of `collections.counter_most_common(counter, n=None)`.
///
/// Returns a list of `(element, count)` tuples, sorted from most common to least.
/// If `n` is provided, only the `n` most common elements are returned.
/// If `n` is `None` (or omitted), all elements are returned.
#[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn counter_most_common(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (counter_val, n_val) = extract_one_or_two_args("counter_most_common", args, heap)?;

    let counter_id = require_dict_ref(&counter_val, "counter_most_common", heap)?;

    // Collect items from the dict or counter
    // Use with_entry_mut to avoid borrow conflict: items() needs &mut Heap
    // but heap.get() returns an immutable borrow
    let mut items: Vec<(Value, i64)> = heap.with_entry_mut(counter_id, |heap_inner, data| match data {
        HeapData::Dict(dict) => dict
            .items(heap_inner)
            .into_iter()
            .map(|(k, v)| {
                let count = if let Value::Int(n) = &v { *n } else { 0 };
                v.drop_with_heap(heap_inner);
                (k, count)
            })
            .collect(),
        HeapData::Counter(counter) => counter
            .dict()
            .items(heap_inner)
            .into_iter()
            .map(|(k, v)| {
                let count = if let Value::Int(n) = &v { *n } else { 0 };
                v.drop_with_heap(heap_inner);
                (k, count)
            })
            .collect(),
        _ => Vec::new(),
    });

    // Sort by count descending, then by insertion order for stability
    items.sort_by_key(|b| std::cmp::Reverse(b.1));

    // Truncate if n provided
    if let Some(n_val) = n_val {
        let n = n_val.as_int(heap)?;
        n_val.drop_with_heap(heap);
        if n >= 0 {
            items.truncate(n as usize);
        }
    }

    // Build list of (elem, count) tuples
    let mut result_items = Vec::with_capacity(items.len());
    for (key, count) in items {
        let tuple_items: SmallVec<[Value; 3]> = SmallVec::from_vec(vec![key, Value::Int(count)]);
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        result_items.push(tuple_val);
    }

    counter_val.drop_with_heap(heap);

    let result_list = List::new(result_items);
    let list_id = heap.allocate(HeapData::List(result_list))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Implementation of `collections.counter_elements(counter)`.
///
/// Returns a list of elements repeated according to their counts. Elements with
/// zero or negative counts are omitted. The order matches iteration order of the dict.
#[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn counter_elements(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let counter_val = args.get_one_arg("counter_elements", heap)?;

    let counter_id = require_dict_ref(&counter_val, "counter_elements", heap)?;

    // Collect items via with_entry_mut to avoid borrow conflicts
    let items: Vec<(Value, i64)> = heap.with_entry_mut(counter_id, |heap_inner, data| match data {
        HeapData::Dict(dict) => dict
            .items(heap_inner)
            .into_iter()
            .map(|(k, v)| {
                let count = if let Value::Int(n) = &v { *n } else { 0 };
                v.drop_with_heap(heap_inner);
                (k, count)
            })
            .collect(),
        HeapData::Counter(counter) => counter
            .dict()
            .items(heap_inner)
            .into_iter()
            .map(|(k, v)| {
                let count = if let Value::Int(n) = &v { *n } else { 0 };
                v.drop_with_heap(heap_inner);
                (k, count)
            })
            .collect(),
        _ => Vec::new(),
    });

    let mut result = Vec::new();
    for (key, count) in items {
        if count > 0 {
            for _ in 0..count as usize {
                let cloned = key.clone_with_heap(heap);
                result.push(cloned);
            }
        }
        key.drop_with_heap(heap);
    }

    counter_val.drop_with_heap(heap);

    let result_list = List::new(result);
    let list_id = heap.allocate(HeapData::List(result_list))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Implementation of `collections.counter_subtract(counter, other)`.
///
/// Subtracts counts from `other` (dict or iterable) from `counter` in-place.
/// Counts can go below zero. Returns `None`.
fn counter_subtract(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (counter_val, other_val) = args.get_two_args("counter_subtract", heap)?;

    let counter_id = require_dict_ref(&counter_val, "counter_subtract", heap)?;

    // Determine if other is a dict or an iterable
    let other_counts = extract_counts(other_val, heap, interns)?;

    // Subtract counts
    for (key, subtract_count) in other_counts {
        let current = get_dict_int(counter_id, &key, heap, interns);
        let new_count = current - subtract_count;
        let key_clone = key.clone_with_heap(heap);
        heap.with_entry_mut(counter_id, |heap_inner, data| {
            match data {
                HeapData::Dict(dict) => {
                    // Ignore errors here since we've already validated
                    let _ = dict.set(key_clone, Value::Int(new_count), heap_inner, interns);
                }
                HeapData::Counter(counter) => {
                    let _ = counter
                        .dict_mut()
                        .set(key_clone, Value::Int(new_count), heap_inner, interns);
                }
                _ => {}
            }
        });
        key.drop_with_heap(heap);
    }

    counter_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `collections.counter_update(counter, other)`.
///
/// Adds counts from `other` (dict or iterable) to `counter` in-place.
/// Returns `None`.
fn counter_update(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (counter_val, other_val) = args.get_two_args("counter_update", heap)?;

    let counter_id = require_dict_ref(&counter_val, "counter_update", heap)?;

    let other_counts = extract_counts(other_val, heap, interns)?;

    // Add counts
    for (key, add_count) in other_counts {
        let current = get_dict_int(counter_id, &key, heap, interns);
        let new_count = current + add_count;
        let key_clone = key.clone_with_heap(heap);
        heap.with_entry_mut(counter_id, |heap_inner, data| match data {
            HeapData::Dict(dict) => {
                let _ = dict.set(key_clone, Value::Int(new_count), heap_inner, interns);
            }
            HeapData::Counter(counter) => {
                let _ = counter
                    .dict_mut()
                    .set(key_clone, Value::Int(new_count), heap_inner, interns);
            }
            _ => {}
        });
        key.drop_with_heap(heap);
    }

    counter_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `collections.counter_total(counter)`.
///
/// Returns the sum of all count values in the counter dict.
fn counter_total(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let counter_val = args.get_one_arg("counter_total", heap)?;

    let counter_id = require_dict_ref(&counter_val, "counter_total", heap)?;

    // Use with_entry_mut to avoid borrow conflict when reading dict values
    let total: i64 = heap.with_entry_mut(counter_id, |_, data| match data {
        HeapData::Dict(dict) => dict
            .iter()
            .map(|(_, v)| if let Value::Int(n) = v { *n } else { 0 })
            .sum(),
        HeapData::Counter(counter) => counter
            .dict()
            .iter()
            .map(|(_, v)| if let Value::Int(n) = v { *n } else { 0 })
            .sum(),
        _ => 0,
    });

    counter_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Int(total)))
}

// ============================================================
// Deque utilities
// ============================================================

/// Implementation of `collections.deque_appendleft(deque, x)`.
///
/// Inserts `x` at the beginning of the deque (list). Returns `None`.
fn deque_appendleft(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (deque_val, item) = args.get_two_args("deque_appendleft", heap)?;

    let list_id = require_list_ref(&deque_val, "deque_appendleft", heap)?;

    // Track if we're adding a reference
    let is_ref = matches!(item, Value::Ref(_));
    if is_ref {
        heap.mark_potential_cycle();
    }

    heap.with_entry_mut(list_id, |_, data| {
        if let HeapData::List(list) = data {
            if is_ref {
                list.set_contains_refs();
            }
            let vec = list.as_vec_mut();
            vec.insert(0, item);
        }
    });

    deque_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `collections.deque_popleft(deque)`.
///
/// Removes and returns the leftmost element from the deque (list).
/// Raises `IndexError` if the deque is empty.
fn deque_popleft(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let deque_val = args.get_one_arg("deque_popleft", heap)?;

    let list_id = require_list_ref(&deque_val, "deque_popleft", heap)?;

    // Check if empty
    let is_empty = matches!(heap.get(list_id), HeapData::List(list) if list.len() == 0);
    if is_empty {
        deque_val.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::IndexError, "pop from an empty deque").into());
    }

    let result = heap.with_entry_mut(list_id, |_, data| {
        if let HeapData::List(list) = data {
            let vec = list.as_vec_mut();
            vec.remove(0)
        } else {
            Value::None
        }
    });

    deque_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `collections.deque_extendleft(deque, iterable)`.
///
/// Extends the deque by prepending elements from the iterable.
/// Elements are added one at a time to the left, so the iterable's
/// elements appear in reverse order in the deque (matching CPython behavior).
fn deque_extendleft(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (deque_val, iterable) = args.get_two_args("deque_extendleft", heap)?;

    let list_id = require_list_ref(&deque_val, "deque_extendleft", heap)?;

    let items = collect_iterable(iterable, heap, interns)?;

    // Insert each item at position 0, which reverses the order (matching CPython)
    for item in items {
        let is_ref = matches!(item, Value::Ref(_));
        if is_ref {
            heap.mark_potential_cycle();
        }

        heap.with_entry_mut(list_id, |_, data| {
            if let HeapData::List(list) = data {
                if is_ref {
                    list.set_contains_refs();
                }
                let vec = list.as_vec_mut();
                vec.insert(0, item);
            }
        });
    }

    deque_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `collections.deque_rotate(deque, n=1)`.
///
/// Rotates the deque `n` steps to the right. If `n` is negative, rotates left.
/// When the deque is not empty, rotating one step to the right is equivalent
/// to `d.appendleft(d.pop())`.
fn deque_rotate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (deque_val, n_val) = extract_one_or_two_args("deque_rotate", args, heap)?;

    let list_id = require_list_ref(&deque_val, "deque_rotate", heap)?;

    let n = if let Some(n_val) = n_val {
        let n = n_val.as_int(heap)?;
        n_val.drop_with_heap(heap);
        n
    } else {
        1
    };

    heap.with_entry_mut(list_id, |_, data| {
        if let HeapData::List(list) = data {
            let vec = list.as_vec_mut();
            let len = vec.len();
            if len > 1 {
                // Normalize rotation amount
                #[expect(clippy::cast_possible_wrap, reason = "deque length fits in i64")]
                let len_i64 = len as i64;
                #[expect(
                    clippy::cast_sign_loss,
                    clippy::cast_possible_truncation,
                    reason = "modulo result is non-negative and fits in usize"
                )]
                let n_mod = ((n % len_i64) + len_i64) as usize % len;
                if n_mod > 0 {
                    vec.rotate_right(n_mod);
                }
            }
        }
    });

    deque_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

// ============================================================
// OrderedDict utilities
// ============================================================

/// Implementation of `collections.ordereddict_move_to_end(od, key, last=True)`.
///
/// Moves an existing key to either end of the ordered dict.
/// If `last` is true (default), moves to the end; if false, moves to the beginning.
/// Raises `KeyError` if the key is not found.
fn od_move_to_end(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (dict_val, key_val, last_val) = extract_two_or_three_args("ordereddict_move_to_end", args, heap)?;

    let dict_id = require_dict_ref(&dict_val, "ordereddict_move_to_end", heap)?;

    let move_to_last = if let Some(last_val) = last_val {
        let b = last_val.py_bool(heap, interns);
        last_val.drop_with_heap(heap);
        b
    } else {
        true
    };

    // Pop the key-value pair and re-insert at the appropriate end
    let popped = heap.with_entry_mut(dict_id, |heap_inner, data| {
        if let HeapData::Dict(dict) = data {
            dict.pop(&key_val, heap_inner, interns)
        } else {
            Ok(None)
        }
    })?;

    if let Some((key, value)) = popped {
        if move_to_last {
            // Re-insert at the end (default dict behavior)
            heap.with_entry_mut(dict_id, |heap_inner, data| {
                if let HeapData::Dict(dict) = data {
                    let _ = dict.set(key, value, heap_inner, interns);
                }
            });
        } else {
            // To move to the beginning: collect all items, clear, insert key first, then rest
            let existing_items = heap.with_entry_mut(dict_id, |heap_inner, data| {
                if let HeapData::Dict(dict) = data {
                    dict.items(heap_inner)
                } else {
                    Vec::new()
                }
            });

            // Clear the dict and rebuild with key first
            heap.with_entry_mut(dict_id, |heap_inner, data| {
                if let HeapData::Dict(dict) = data {
                    dict.drop_all_entries(heap_inner);
                    let _ = dict.set(key, value, heap_inner, interns);
                    for (k, v) in existing_items {
                        let _ = dict.set(k, v, heap_inner, interns);
                    }
                }
            });
        }
    } else {
        key_val.drop_with_heap(heap);
        dict_val.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::KeyError, "key not found").into());
    }

    key_val.drop_with_heap(heap);
    dict_val.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `collections.ordereddict_popitem(od, last=True)`.
///
/// Removes and returns a `(key, value)` tuple. If `last` is true (default),
/// removes the last item (LIFO); if false, removes the first item (FIFO).
/// Raises `KeyError` if the dict is empty.
fn od_popitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (dict_val, last_val) = extract_one_or_two_args("ordereddict_popitem", args, heap)?;

    let dict_id = require_dict_ref(&dict_val, "ordereddict_popitem", heap)?;

    let pop_last = if let Some(last_val) = last_val {
        let b = last_val.py_bool(heap, interns);
        last_val.drop_with_heap(heap);
        b
    } else {
        true
    };

    // Get the key to pop
    let target_key = {
        let HeapData::Dict(dict) = heap.get(dict_id) else {
            dict_val.drop_with_heap(heap);
            return Err(ExcType::type_error("not a dict".to_string()));
        };

        if dict.is_empty() {
            dict_val.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::KeyError, "dictionary is empty").into());
        }

        if pop_last {
            // Get the last key
            dict.key_at(dict.len() - 1).map(|k| k.clone_with_heap(heap))
        } else {
            // Get the first key
            dict.key_at(0).map(|k| k.clone_with_heap(heap))
        }
    };

    let Some(target_key) = target_key else {
        dict_val.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::KeyError, "dictionary is empty").into());
    };

    // Pop the key-value pair
    let popped = heap.with_entry_mut(dict_id, |heap_inner, data| {
        if let HeapData::Dict(dict) = data {
            dict.pop(&target_key, heap_inner, interns)
        } else {
            Ok(None)
        }
    })?;

    target_key.drop_with_heap(heap);

    if let Some((key, value)) = popped {
        let tuple_items: SmallVec<[Value; 3]> = SmallVec::from_vec(vec![key, value]);
        let tuple_val = allocate_tuple(tuple_items, heap)?;
        dict_val.drop_with_heap(heap);
        Ok(AttrCallResult::Value(tuple_val))
    } else {
        dict_val.drop_with_heap(heap);
        Err(SimpleException::new_msg(ExcType::KeyError, "key not found").into())
    }
}

// ============================================================
// Helper functions
// ============================================================

/// Registers a module function attribute on the module.
fn register(
    module: &mut crate::types::Module,
    name: StaticStrings,
    func: CollectionsFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    module.set_attr(
        name,
        Value::ModuleFunction(ModuleFunctions::Collections(func)),
        heap,
        interns,
    );
}

/// Counts elements from an iterable and adds them to a dict.
///
/// Each element becomes a key, and its count is the value.
/// If the element already exists, its count is incremented.
fn count_elements(
    dict: &mut Dict,
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;

    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let current_count = match dict.get(&item, heap, interns) {
                    Ok(Some(Value::Int(n))) => *n,
                    Ok(_) => 0,
                    Err(e) => {
                        item.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(e);
                    }
                };

                let key = item.clone_with_heap(heap);
                if let Err(e) = dict.set(key, Value::Int(current_count + 1), heap, interns) {
                    item.drop_with_heap(heap);
                    dict.drop_all_entries(heap);
                    return Err(e);
                }
                item.drop_with_heap(heap);
            }
            Ok(None) => break,
            Err(e) => {
                dict.drop_all_entries(heap);
                return Err(e);
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(())
}

/// Consumes an iterable of (key, value) pairs and inserts them into a dict.
fn consume_pairs_into_dict(
    dict: &mut Dict,
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;

    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(pair)) => {
                let mut pair_iter = OurosIter::new(pair, heap, interns)?;

                let key = match pair_iter.for_next(heap, interns) {
                    Ok(Some(k)) => k,
                    Ok(None) => {
                        pair_iter.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(ExcType::type_error(
                            "each element of the iterable must be a pair".to_string(),
                        ));
                    }
                    Err(e) => {
                        pair_iter.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(e);
                    }
                };

                let value = match pair_iter.for_next(heap, interns) {
                    Ok(Some(v)) => v,
                    Ok(None) => {
                        key.drop_with_heap(heap);
                        pair_iter.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(ExcType::type_error(
                            "each element of the iterable must be a pair".to_string(),
                        ));
                    }
                    Err(e) => {
                        key.drop_with_heap(heap);
                        pair_iter.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(e);
                    }
                };

                // Check no extra element
                match pair_iter.for_next(heap, interns) {
                    Ok(Some(extra)) => {
                        extra.drop_with_heap(heap);
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        pair_iter.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(ExcType::type_error(
                            "each element of the iterable must be a pair".to_string(),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        pair_iter.drop_with_heap(heap);
                        dict.drop_all_entries(heap);
                        return Err(e);
                    }
                }

                pair_iter.drop_with_heap(heap);

                if let Err(e) = dict.set(key, value, heap, interns) {
                    dict.drop_all_entries(heap);
                    return Err(e);
                }
            }
            Ok(None) => break,
            Err(e) => {
                dict.drop_all_entries(heap);
                return Err(e);
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(())
}

/// Collects all elements from an iterable into a Vec.
fn collect_iterable(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut items = Vec::new();

    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(e) => {
                for item in items {
                    item.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(items)
}

/// Parses `field_names` for `collections.namedtuple`.
fn parse_namedtuple_field_names(
    field_names_value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<EitherStr>> {
    let parsed = match &field_names_value {
        Value::InternString(id) => parse_namedtuple_field_name_str(interns.get_str(*id)),
        Value::Ref(id) => {
            if let HeapData::Str(s) = heap.get(*id) {
                parse_namedtuple_field_name_str(s.as_str())
            } else {
                let names = collect_iterable(field_names_value.clone_with_heap(heap), heap, interns)?;
                let mut parsed = Vec::with_capacity(names.len());
                for name in names {
                    parsed.push(EitherStr::Heap(name.py_str(heap, interns).into_owned()));
                    name.drop_with_heap(heap);
                }
                parsed
            }
        }
        _ => {
            let names = collect_iterable(field_names_value.clone_with_heap(heap), heap, interns)?;
            let mut parsed = Vec::with_capacity(names.len());
            for name in names {
                parsed.push(EitherStr::Heap(name.py_str(heap, interns).into_owned()));
                name.drop_with_heap(heap);
            }
            parsed
        }
    };
    field_names_value.drop_with_heap(heap);
    Ok(parsed)
}

/// Splits a namedtuple field declaration string into identifiers.
fn parse_namedtuple_field_name_str(value: &str) -> Vec<EitherStr> {
    value
        .split([',', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| EitherStr::Heap(part.to_owned()))
        .collect()
}

/// Extracts a `HeapId` from a `Value::Ref` that points to a `Dict`.
/// Returns a `TypeError` if the value is not a dict reference.
fn require_dict_ref(val: &Value, func_name: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let Value::Ref(id) = val else {
        return Err(ExcType::type_error(format!(
            "{func_name}() arg 1 must be a dict, not '{}'",
            val.py_type(heap)
        )));
    };
    if !matches!(heap.get(*id), HeapData::Dict(_)) {
        return Err(ExcType::type_error(format!(
            "{func_name}() arg 1 must be a dict, not '{}'",
            val.py_type(heap)
        )));
    }
    Ok(*id)
}

/// Extracts a `HeapId` from a `Value::Ref` that points to a `List`.
/// Returns a `TypeError` if the value is not a list reference.
fn require_list_ref(val: &Value, func_name: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let Value::Ref(id) = val else {
        return Err(ExcType::type_error(format!(
            "{func_name}() arg 1 must be a list, not '{}'",
            val.py_type(heap)
        )));
    };
    if !matches!(heap.get(*id), HeapData::List(_)) {
        return Err(ExcType::type_error(format!(
            "{func_name}() arg 1 must be a list, not '{}'",
            val.py_type(heap)
        )));
    }
    Ok(*id)
}

/// Gets the integer value for a key in a dict, defaulting to 0 if not found.
fn get_dict_int(dict_id: HeapId, key: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> i64 {
    // Clone the key to avoid holding a borrow across with_entry_mut
    let key_clone = key.clone_with_heap(heap);
    let result = heap.with_entry_mut(dict_id, |heap_inner, data| {
        if let HeapData::Dict(dict) = data {
            match dict.get(&key_clone, heap_inner, interns) {
                Ok(Some(Value::Int(n))) => *n,
                _ => 0,
            }
        } else {
            0
        }
    });
    key_clone.drop_with_heap(heap);
    result
}

/// Extracts counts from a value that is either a dict (key->count) or an iterable.
///
/// If the value is a dict, returns (key, count) pairs directly.
/// If it's an iterable, counts elements like Counter and returns (key, count) pairs.
fn extract_counts(
    val: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<(Value, i64)>> {
    // Check if it's a mapping first.
    if let Value::Ref(id) = &val {
        let mapping_items: Option<Vec<(Value, i64)>> = heap.with_entry_mut(*id, |heap_inner, data| {
            let raw_items = match data {
                HeapData::Dict(dict) => Some(dict.items(heap_inner)),
                HeapData::Counter(counter) => Some(counter.dict().items(heap_inner)),
                HeapData::OrderedDict(od) => Some(od.dict().items(heap_inner)),
                HeapData::DefaultDict(default_dict) => Some(default_dict.dict().items(heap_inner)),
                _ => None,
            }?;

            Some(
                raw_items
                    .into_iter()
                    .map(|(k, v)| {
                        let count = if let Value::Int(n) = &v { *n } else { 0 };
                        v.drop_with_heap(heap_inner);
                        (k, count)
                    })
                    .collect(),
            )
        });
        if let Some(items) = mapping_items {
            val.drop_with_heap(heap);
            return Ok(items);
        }
    }

    // Otherwise, count elements from the iterable
    let mut counts = Dict::new();
    count_elements(&mut counts, val, heap, interns)?;

    let result: Vec<(Value, i64)> = counts
        .items(heap)
        .into_iter()
        .map(|(k, v)| {
            let count = if let Value::Int(n) = &v { *n } else { 0 };
            v.drop_with_heap(heap);
            (k, count)
        })
        .collect();

    counts.drop_all_entries(heap);
    Ok(result)
}

/// Parses `collections.namedtuple` field names from either a string or iterable.
///
/// When a string is provided, commas are treated as separators and parsing
/// follows whitespace splitting semantics.
fn parse_namedtuple_fields(
    field_names: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<EitherStr>> {
    if let Some(field_names_str) = field_names.as_either_str(heap) {
        let parsed = field_names_str
            .as_str(interns)
            .replace(',', " ")
            .split_whitespace()
            .map(|field| EitherStr::Heap(field.to_owned()))
            .collect();
        field_names.drop_with_heap(heap);
        return Ok(parsed);
    }

    let mut iter = OurosIter::new(field_names, heap, interns)?;
    let mut parsed = Vec::new();

    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(field_value)) => {
                let Some(field_name) = field_value.as_either_str(heap) else {
                    field_value.drop_with_heap(heap);
                    iter.drop_with_heap(heap);
                    return Err(ExcType::type_error("Type names and field names must be strings"));
                };
                parsed.push(field_name);
                field_value.drop_with_heap(heap);
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(parsed)
}

/// Extracts 1 or 2 arguments from `ArgValues`.
///
/// Returns `(required_arg, optional_arg)`.
fn extract_one_or_two_args(
    name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<(Value, Option<Value>)> {
    match args {
        ArgValues::One(a) => Ok((a, None)),
        ArgValues::Two(a, b) => Ok((a, Some(b))),
        ArgValues::ArgsKargs { args, .. } if args.len() == 1 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), None))
        }
        ArgValues::ArgsKargs { args, .. } if args.len() == 2 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), Some(iter.next().unwrap())))
        }
        other => {
            let count = match &other {
                ArgValues::Empty => 0,
                _ => 3, // approximate
            };
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "{name}() takes 1 to 2 positional arguments but {count} were given"
            )))
        }
    }
}

/// Extracts 2 or 3 arguments from `ArgValues`.
///
/// Returns `(arg1, arg2, optional_arg3)`.
fn extract_two_or_three_args(
    name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<(Value, Value, Option<Value>)> {
    match args {
        ArgValues::Two(a, b) => Ok((a, b, None)),
        ArgValues::ArgsKargs { args, .. } if args.len() == 2 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), iter.next().unwrap(), None))
        }
        ArgValues::ArgsKargs { args, .. } if args.len() == 3 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), iter.next().unwrap(), Some(iter.next().unwrap())))
        }
        other => {
            let count = match &other {
                ArgValues::Empty => 0,
                ArgValues::One(_) => 1,
                _ => 4, // approximate
            };
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "{name}() takes 2 to 3 positional arguments but {count} were given"
            )))
        }
    }
}
