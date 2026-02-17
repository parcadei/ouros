//! Implementation of the `copy` module.
//!
//! Provides shallow and deep copy operations from Python's `copy` module:
//! - `copy(x)`: Creates a shallow copy of x
//! - `deepcopy(x, memo=None)`: Creates a deep copy of x, with optional memo dict for circular reference handling
//! - `Error`: Exception raised for copy-specific errors
//!
//! Shallow copies create new containers but share references to the original items.
//! Deep copies recursively copy all objects, handling circular references via a memo dictionary.

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Dict, Instance, List, Module, Set, Str, allocate_tuple},
    value::Value,
};

/// Copy module functions.
///
/// Each variant corresponds to a Python `copy` module function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum CopyFunctions {
    Copy,
    Deepcopy,
    Replace,
}

/// Creates the `copy` module and allocates it on the heap.
///
/// Sets up `copy()`, `deepcopy()`, and `Error` exception.
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
    let mut module = Module::new(StaticStrings::Copy);

    // copy(x) - shallow copy function
    module.set_attr(
        StaticStrings::Copy,
        Value::ModuleFunction(ModuleFunctions::Copy(CopyFunctions::Copy)),
        heap,
        interns,
    );

    // deepcopy(x, memo=None) - deep copy function
    module.set_attr(
        StaticStrings::Deepcopy,
        Value::ModuleFunction(ModuleFunctions::Copy(CopyFunctions::Deepcopy)),
        heap,
        interns,
    );

    // replace(obj, /, **changes) - copy.replace helper
    module.set_attr(
        StaticStrings::Replace,
        Value::ModuleFunction(ModuleFunctions::Copy(CopyFunctions::Replace)),
        heap,
        interns,
    );

    // Error exception type (subclass of Exception)
    module.set_attr(
        StaticStrings::CopyError,
        Value::Builtin(crate::builtins::Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    );

    // copy.error is an alias for copy.Error
    module.set_attr_str(
        "error",
        Value::Builtin(crate::builtins::Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    )?;

    // copy.dispatch_table exists in CPython and is mutable.
    let dispatch_table = Dict::new();
    let dispatch_table_id = heap.allocate(HeapData::Dict(dispatch_table))?;
    module.set_attr_str("dispatch_table", Value::Ref(dispatch_table_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a copy module function.
///
/// All copy functions return immediate values (no host involvement needed).
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: CopyFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        CopyFunctions::Copy => copy_shallow(heap, interns, args),
        CopyFunctions::Deepcopy => copy_deep(heap, interns, args),
        CopyFunctions::Replace => copy_replace(heap, interns, args).map(AttrCallResult::Value),
    }
}

/// Implementation of `copy.copy(x)` - shallow copy.
///
/// Creates a shallow copy of the object. For compound objects (lists, dicts, sets),
/// creates a new container with references to the original items.
/// For immutable types and non-container types, returns the original object.
fn copy_shallow(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("copy.copy", heap)?;

    if let Value::Ref(_) = obj {
        let dunder_id: crate::intern::StringId = StaticStrings::DunderCopy.into();
        match obj.py_getattr(dunder_id, heap, interns) {
            Ok(AttrCallResult::Value(callable)) => {
                obj.drop_with_heap(heap);
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::Empty));
            }
            Ok(other) => {
                obj.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {}
        }
    }

    // For immediate values (non-heap), return as-is (they're immutable)
    let result: RunResult<Value> = match obj {
        // Immutable/immediate values - return as-is
        Value::Undefined
        | Value::Ellipsis
        | Value::None
        | Value::NotImplemented
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::InternString(_)
        | Value::InternBytes(_)
        | Value::InternLongInt(_)
        | Value::Builtin(_)
        | Value::ModuleFunction(_)
        | Value::DefFunction(_)
        | Value::ExtFunction(_)
        | Value::Proxy(_)
        | Value::Marker(_)
        | Value::Property(_)
        | Value::ExternalFuture(_) => Ok(obj),

        // Heap-allocated values - need to copy the container
        Value::Ref(heap_id) => {
            // Keep the original argument alive during copy and release it
            // through heap-aware cleanup on all paths.
            defer_drop!(obj, heap);
            shallow_copy_heap(heap_id, heap, interns)
        }

        #[cfg(feature = "ref-count-panic")]
        Value::Dereferenced => {
            Err(SimpleException::new_msg(ExcType::RuntimeError, "cannot copy dereferenced object").into())
        }
    };
    let result = result?;
    Ok(AttrCallResult::Value(result))
}

/// Data extracted for shallow copying.
enum ShallowCopySource {
    List(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Set(Vec<Value>),
    Tuple(Vec<Value>),
    NamedTuple {
        name: String,
        field_names: Vec<crate::value::EitherStr>,
        items: Vec<Value>,
    },
    Immutable,
}

/// Creates a shallow copy of a heap-allocated object.
fn shallow_copy_heap(heap_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    if matches!(heap.get(heap_id), HeapData::Instance(_)) {
        return shallow_copy_instance(heap_id, heap, interns);
    }

    // `re.Pattern`/`re.Match` objects are immutable from Python's perspective,
    // but returning the same heap id would transfer ownership and can free the
    // original object when the copy is dropped. Always allocate a fresh clone.
    match heap.get(heap_id) {
        HeapData::RePattern(pattern) => {
            let new_id = heap.allocate(HeapData::RePattern(pattern.clone()))?;
            return Ok(Value::Ref(new_id));
        }
        HeapData::ReMatch(re_match) => {
            let new_id = heap.allocate(HeapData::ReMatch(re_match.clone()))?;
            return Ok(Value::Ref(new_id));
        }
        _ => {}
    }

    // Extract data first without holding borrows
    let source = match heap.get(heap_id) {
        HeapData::List(list) => {
            ShallowCopySource::List(list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect())
        }
        HeapData::Dict(dict) => ShallowCopySource::Dict(
            dict.iter()
                .map(|(k, v)| (k.clone_with_heap(heap), v.clone_with_heap(heap)))
                .collect(),
        ),
        HeapData::Set(set) => {
            ShallowCopySource::Set(set.storage().copy_entries().into_iter().map(|(v, _)| v).collect())
        }
        // Tuples are immutable: copy.copy returns the original object.
        HeapData::Tuple(_) => ShallowCopySource::Immutable,
        HeapData::NamedTuple(nt) => ShallowCopySource::NamedTuple {
            name: nt.name(interns).to_string(),
            field_names: nt.field_names().to_vec(),
            items: nt.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        },
        // Immutable types - return as-is
        HeapData::FrozenSet(_)
        | HeapData::Str(_)
        | HeapData::Bytes(_)
        | HeapData::LongInt(_)
        | HeapData::Range(_)
        | HeapData::Slice(_)
        | HeapData::Closure(_, _, _)
        | HeapData::FunctionDefaults(_, _)
        | HeapData::Coroutine(_)
        | HeapData::Module(_)
        | HeapData::ClassObject(_)
        | HeapData::Instance(_)
        | HeapData::Dataclass(_)
        | HeapData::Iter(_)
        | HeapData::Tee(_)
        | HeapData::GatherFuture(_) => ShallowCopySource::Immutable,
        _ => ShallowCopySource::Immutable,
    };

    // Now create the new object without any borrows
    match source {
        ShallowCopySource::List(items) => {
            let new_list = List::new(items);
            let new_id = heap.allocate(HeapData::List(new_list))?;
            Ok(Value::Ref(new_id))
        }
        ShallowCopySource::Dict(pairs) => {
            let new_dict = Dict::from_pairs(pairs, heap, interns)?;
            let new_id = heap.allocate(HeapData::Dict(new_dict))?;
            Ok(Value::Ref(new_id))
        }
        ShallowCopySource::Set(items) => {
            let mut new_set = Set::new();
            for item in items {
                new_set.add(item, heap, interns)?;
            }
            let new_id = heap.allocate(HeapData::Set(new_set))?;
            Ok(Value::Ref(new_id))
        }
        ShallowCopySource::Tuple(items) => {
            let items_sv: smallvec::SmallVec<[Value; 3]> = items.into_iter().collect();
            let result =
                allocate_tuple(items_sv, heap).map_err(|e: crate::resource::ResourceError| RunError::from(e))?;
            Ok(result)
        }
        ShallowCopySource::NamedTuple {
            name,
            field_names,
            items,
        } => {
            let new_nt = crate::types::NamedTuple::new(name, field_names, items);
            let new_id = heap.allocate(HeapData::NamedTuple(new_nt))?;
            Ok(Value::Ref(new_id))
        }
        ShallowCopySource::Immutable => {
            // copy.copy() returns the original object identity for immutable
            // heap objects (e.g. tuple, str). This call returns a new owning
            // Value, so we must add a ref for the returned handle.
            heap.inc_ref(heap_id);
            Ok(Value::Ref(heap_id))
        }
    }
}

/// Implementation of `copy.deepcopy(x, memo=None)` - deep copy.
///
/// Creates a deep copy of the object, recursively copying all nested objects.
/// Uses an optional memo dictionary to handle circular references.
fn copy_deep(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (obj, memo) = args.get_one_two_args("copy.deepcopy", heap)?;

    // Create memo dict if not provided
    let memo_value = if let Some(m) = memo {
        m
    } else {
        let new_dict = Dict::new();
        let dict_id = heap.allocate(HeapData::Dict(new_dict))?;
        Value::Ref(dict_id)
    };

    if let Value::Ref(_) = obj {
        let dunder_id: crate::intern::StringId = StaticStrings::DunderDeepcopy.into();
        match obj.py_getattr(dunder_id, heap, interns) {
            Ok(AttrCallResult::Value(callable)) => {
                obj.drop_with_heap(heap);
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::One(memo_value)));
            }
            Ok(other) => {
                obj.drop_with_heap(heap);
                memo_value.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {}
        }
    }

    // Check that memo is a dict
    let memo_id = match memo_value {
        Value::Ref(id) => {
            if !matches!(heap.get(id), HeapData::Dict(_)) {
                return Err(SimpleException::new_msg(ExcType::TypeError, "memo must be a dictionary").into());
            }
            id
        }
        _ => {
            return Err(SimpleException::new_msg(ExcType::TypeError, "memo must be a dictionary").into());
        }
    };

    // Keep memo alive for the entire deepcopy call. Dropping it here can free
    // the dict before `check_memo`/`store_in_memo` uses `memo_id`.
    defer_drop!(memo_value, heap);

    // Handle immediate values (non-heap)
    let result: RunResult<Value> = match obj {
        // Immutable/immediate values - return as-is without memo check
        Value::Undefined
        | Value::Ellipsis
        | Value::None
        | Value::NotImplemented
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::InternString(_)
        | Value::InternBytes(_)
        | Value::InternLongInt(_)
        | Value::Builtin(_)
        | Value::ModuleFunction(_)
        | Value::DefFunction(_)
        | Value::ExtFunction(_)
        | Value::Proxy(_)
        | Value::Marker(_)
        | Value::Property(_)
        | Value::ExternalFuture(_) => Ok(obj),

        // Heap-allocated values - need to check memo and copy recursively
        Value::Ref(heap_id) => {
            // Check memo first to handle circular references
            if let Some(copied) = check_memo(heap_id, memo_id, heap, interns)? {
                // Drop the original since we're returning the memoized copy
                obj.drop_with_heap(heap);
                Ok(copied)
            } else {
                // Perform default deep copy
                let copied = deep_copy_heap(heap_id, memo_id, heap, interns)?;

                // Clone for memo before potentially moving
                let copied_for_memo = copied.clone_with_heap(heap);

                // Store in memo before returning (to handle self-references)
                store_in_memo(heap_id, copied_for_memo, memo_id, heap, interns)?;

                // Drop the original
                obj.drop_with_heap(heap);

                Ok(copied)
            }
        }

        #[cfg(feature = "ref-count-panic")]
        Value::Dereferenced => {
            obj.drop_with_heap(heap);
            Err(SimpleException::new_msg(ExcType::RuntimeError, "cannot copy dereferenced object").into())
        }
    };
    let result = result?;
    Ok(AttrCallResult::Value(result))
}

/// Check if object is already in memo dict.
fn check_memo(
    heap_id: HeapId,
    memo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let key = memo_key(heap_id);
    let mut result = None;

    // Use with_entry_mut to avoid borrow conflicts
    heap.with_entry_mut(memo_id, |heap_inner, data| {
        if let HeapData::Dict(memo) = data {
            // Use id() as key - in Python, memo uses id(obj)
            if let Ok(Some(value)) = memo.get(&key, heap_inner, interns) {
                result = Some(value.clone_with_heap(heap_inner));
            }
        }
    });

    Ok(result)
}

/// Store copied value in memo dict.
fn store_in_memo(
    original_id: HeapId,
    copied: Value,
    memo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut copied = Some(copied);
    let mut set_error = None;

    // Use with_entry_mut to avoid borrow conflicts.
    heap.with_entry_mut(memo_id, |heap_inner, data| {
        if let HeapData::Dict(memo) = data {
            let value = copied
                .take()
                .expect("store_in_memo value already moved before dict insertion");
            match memo.set(memo_key(original_id), value, heap_inner, interns) {
                Ok(Some(old_value)) => old_value.drop_with_heap(heap_inner),
                Ok(None) => {}
                Err(err) => set_error = Some(err),
            }
        }
    });

    if let Some(value) = copied {
        value.drop_with_heap(heap);
    }
    if let Some(err) = set_error {
        return Err(err);
    }
    Ok(())
}

/// Returns the memo dictionary key for a heap object id.
///
/// CPython uses `id(obj)` (an integer) as the memo key. Using integer keys is
/// required because many objects copied by `deepcopy` (e.g. lists, dicts, sets)
/// are themselves unhashable.
fn memo_key(heap_id: HeapId) -> Value {
    let key = i64::try_from(heap_id.index()).unwrap_or(i64::MAX);
    Value::Int(key)
}

/// Allocates a placeholder object and stores it in `memo` before recursively
/// copying child values.
///
/// This ordering is required for circular references: recursive visits must be
/// able to find the in-progress copy in `memo` and reuse its heap id.
fn allocate_memoized_placeholder(
    original_id: HeapId,
    placeholder: HeapData,
    memo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<HeapId> {
    let copied_id = heap.allocate(placeholder)?;
    heap.inc_ref(copied_id);
    store_in_memo(original_id, Value::Ref(copied_id), memo_id, heap, interns)?;
    Ok(copied_id)
}

/// Data extracted from heap for deep copying.
enum DeepCopySource {
    List(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Set(Vec<Value>),
    FrozenSet(Vec<Value>),
    Tuple(Vec<Value>),
    NamedTuple {
        name: String,
        field_names: Vec<crate::value::EitherStr>,
        items: Vec<Value>,
    },
    Cell(Value),
    Immutable, // For types that don't need deep copying
}

/// Creates a deep copy of a heap-allocated object.
fn deep_copy_heap(
    heap_id: HeapId,
    memo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if matches!(heap.get(heap_id), HeapData::Instance(_)) {
        return deep_copy_instance(heap_id, memo_id, heap, interns);
    }

    // Keep deepcopy semantics for regex objects aligned with copy.copy(): they
    // should produce an equivalent standalone object, not alias the same id.
    match heap.get(heap_id) {
        HeapData::RePattern(pattern) => {
            let new_id = heap.allocate(HeapData::RePattern(pattern.clone()))?;
            return Ok(Value::Ref(new_id));
        }
        HeapData::ReMatch(re_match) => {
            let new_id = heap.allocate(HeapData::ReMatch(re_match.clone()))?;
            return Ok(Value::Ref(new_id));
        }
        _ => {}
    }

    // First, extract all the data we need from the heap
    let source = match heap.get(heap_id) {
        HeapData::List(list) => DeepCopySource::List(list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect()),
        HeapData::Dict(dict) => DeepCopySource::Dict(
            dict.iter()
                .map(|(k, v)| (k.clone_with_heap(heap), v.clone_with_heap(heap)))
                .collect(),
        ),
        HeapData::Set(set) => DeepCopySource::Set(set.storage().copy_entries().into_iter().map(|(v, _)| v).collect()),
        HeapData::FrozenSet(_) => {
            let copied = heap.with_entry_mut(heap_id, |heap_inner, data| {
                if let HeapData::FrozenSet(set) = data {
                    set.copy(heap_inner)
                } else {
                    unreachable!("checked frozen set variant above");
                }
            });
            let new_id = heap.allocate(HeapData::FrozenSet(copied))?;
            return Ok(Value::Ref(new_id));
        }
        HeapData::Tuple(tuple) => {
            DeepCopySource::Tuple(tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect())
        }
        HeapData::NamedTuple(nt) => DeepCopySource::NamedTuple {
            name: nt.name(interns).to_string(),
            field_names: nt.field_names().to_vec(),
            items: nt.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        },
        HeapData::Cell(cell_value) => DeepCopySource::Cell(cell_value.clone_with_heap(heap)),
        // Immutable types - return as-is
        HeapData::Str(_)
        | HeapData::Bytes(_)
        | HeapData::LongInt(_)
        | HeapData::Range(_)
        | HeapData::Slice(_)
        | HeapData::Closure(_, _, _)
        | HeapData::FunctionDefaults(_, _)
        | HeapData::Coroutine(_)
        | HeapData::Module(_)
        | HeapData::ClassObject(_)
        | HeapData::Instance(_)
        | HeapData::Dataclass(_)
        | HeapData::Iter(_)
        | HeapData::Tee(_)
        | HeapData::GatherFuture(_) => DeepCopySource::Immutable,
        _ => DeepCopySource::Immutable,
    };

    // Now process the data without holding any borrows
    match source {
        DeepCopySource::List(items) => {
            let copied_id =
                allocate_memoized_placeholder(heap_id, HeapData::List(List::new(Vec::new())), memo_id, heap, interns)?;
            let mut new_items = Vec::with_capacity(items.len());
            for item in items {
                let deep_copied = deep_copy_value(item, memo_id, heap, interns)?;
                new_items.push(deep_copied);
            }
            heap.with_entry_mut(copied_id, |_heap_inner, data| {
                if let HeapData::List(list) = data {
                    *list = List::new(new_items);
                } else {
                    unreachable!("deepcopy list placeholder replaced with non-list data");
                }
            });
            Ok(Value::Ref(copied_id))
        }

        DeepCopySource::Dict(pairs) => {
            let copied_id =
                allocate_memoized_placeholder(heap_id, HeapData::Dict(Dict::new()), memo_id, heap, interns)?;
            let mut new_dict = Dict::with_capacity(pairs.len());
            for (key, value) in pairs {
                let key_copied = deep_copy_value(key, memo_id, heap, interns)?;
                let value_copied = deep_copy_value(value, memo_id, heap, interns)?;
                let _ = new_dict.set(key_copied, value_copied, heap, interns)?;
            }
            heap.with_entry_mut(copied_id, |_heap_inner, data| {
                if let HeapData::Dict(dict) = data {
                    *dict = new_dict;
                } else {
                    unreachable!("deepcopy dict placeholder replaced with non-dict data");
                }
            });
            Ok(Value::Ref(copied_id))
        }

        DeepCopySource::Set(items) => {
            let copied_id = allocate_memoized_placeholder(heap_id, HeapData::Set(Set::new()), memo_id, heap, interns)?;
            let mut new_set = Set::new();
            for item in items {
                let deep_copied = deep_copy_value(item, memo_id, heap, interns)?;
                new_set.add(deep_copied, heap, interns)?;
            }
            heap.with_entry_mut(copied_id, |_heap_inner, data| {
                if let HeapData::Set(set) = data {
                    *set = new_set;
                } else {
                    unreachable!("deepcopy set placeholder replaced with non-set data");
                }
            });
            Ok(Value::Ref(copied_id))
        }

        DeepCopySource::FrozenSet(_) => {
            // Should not reach here - handled above
            shallow_copy_heap(heap_id, heap, interns)
        }

        DeepCopySource::Tuple(items) => {
            if !items.iter().any(|value| matches!(value, Value::Ref(_))) {
                heap.inc_ref(heap_id);
                return Ok(Value::Ref(heap_id));
            }
            let mut copied_items: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::with_capacity(items.len());
            for item in items {
                let deep_copied = deep_copy_value(item, memo_id, heap, interns)?;
                copied_items.push(deep_copied);
            }
            let result =
                allocate_tuple(copied_items, heap).map_err(|e: crate::resource::ResourceError| RunError::from(e))?;
            Ok(result)
        }

        DeepCopySource::NamedTuple {
            name,
            field_names,
            items,
        } => {
            let copied_id = allocate_memoized_placeholder(
                heap_id,
                HeapData::NamedTuple(crate::types::NamedTuple::new(
                    name.clone(),
                    field_names.clone(),
                    Vec::new(),
                )),
                memo_id,
                heap,
                interns,
            )?;
            let mut copied_items = Vec::with_capacity(items.len());
            for item in items {
                let deep_copied = deep_copy_value(item, memo_id, heap, interns)?;
                copied_items.push(deep_copied);
            }
            heap.with_entry_mut(copied_id, |_heap_inner, data| {
                if let HeapData::NamedTuple(nt) = data {
                    *nt = crate::types::NamedTuple::new(name, field_names, copied_items);
                } else {
                    unreachable!("deepcopy namedtuple placeholder replaced with non-namedtuple data");
                }
            });
            Ok(Value::Ref(copied_id))
        }

        DeepCopySource::Cell(cell_value) => {
            let copied_id =
                allocate_memoized_placeholder(heap_id, HeapData::Cell(Value::None), memo_id, heap, interns)?;
            let deep_copied = deep_copy_value(cell_value, memo_id, heap, interns)?;
            heap.with_entry_mut(copied_id, |_heap_inner, data| {
                if let HeapData::Cell(cell) = data {
                    *cell = deep_copied;
                } else {
                    unreachable!("deepcopy cell placeholder replaced with non-cell data");
                }
            });
            Ok(Value::Ref(copied_id))
        }

        DeepCopySource::Immutable => {
            heap.inc_ref(heap_id);
            Ok(Value::Ref(heap_id))
        }
    }
}

/// Returns whether an instance class defines a specific method name.
fn class_has_method(class_id: HeapId, method_name: &str, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.mro_has_attr(method_name, class_id, heap, interns),
        _ => false,
    }
}

/// Sets a boolean attribute in an instance dict, creating the key if needed.
fn set_dict_bool_attr(
    dict: &mut Dict,
    attr_name: &str,
    value: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(attr_name.to_owned())))?;
    if let Some(old_value) = dict.set(Value::Ref(key_id), Value::Bool(value), heap, interns)? {
        old_value.drop_with_heap(heap);
    }
    Ok(())
}

/// Creates a shallow copy of an instance object.
fn shallow_copy_instance(
    heap_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (class_id, attrs_pairs, slot_values, has_custom_copy) = if let HeapData::Instance(inst) = heap.get(heap_id) {
        let attrs_pairs = if let Some(attrs_id) = inst.attrs_id() {
            if let HeapData::Dict(dict) = heap.get(attrs_id) {
                Some(
                    dict.iter()
                        .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        } else {
            None
        };
        let slot_values: Vec<Value> = inst
            .slot_values()
            .iter()
            .map(|value| value.clone_with_heap(heap))
            .collect();
        (
            inst.class_id(),
            attrs_pairs,
            slot_values,
            class_has_method(inst.class_id(), "__copy__", heap, interns),
        )
    } else {
        heap.inc_ref(heap_id);
        return Ok(Value::Ref(heap_id));
    };

    heap.inc_ref(class_id);
    let attrs_id = if let Some(pairs) = attrs_pairs {
        let mut attrs = Dict::from_pairs(pairs, heap, interns)?;
        if has_custom_copy {
            set_dict_bool_attr(&mut attrs, "copy_called", true, heap, interns)?;
        }
        Some(heap.allocate(HeapData::Dict(attrs))?)
    } else {
        None
    };

    let instance = Instance::new(class_id, attrs_id, slot_values, Vec::new());
    let new_id = heap.allocate(HeapData::Instance(instance))?;
    Ok(Value::Ref(new_id))
}

/// Creates a deep copy of an instance object.
fn deep_copy_instance(
    heap_id: HeapId,
    memo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (class_id, attrs_pairs, slot_values, has_custom_deepcopy) = if let HeapData::Instance(inst) = heap.get(heap_id)
    {
        let attrs_pairs = if let Some(attrs_id) = inst.attrs_id() {
            if let HeapData::Dict(dict) = heap.get(attrs_id) {
                Some(
                    dict.iter()
                        .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        } else {
            None
        };
        let slot_values: Vec<Value> = inst
            .slot_values()
            .iter()
            .map(|value| value.clone_with_heap(heap))
            .collect();
        (
            inst.class_id(),
            attrs_pairs,
            slot_values,
            class_has_method(inst.class_id(), "__deepcopy__", heap, interns),
        )
    } else {
        heap.inc_ref(heap_id);
        return Ok(Value::Ref(heap_id));
    };

    heap.inc_ref(class_id);
    let placeholder_attrs_id = if attrs_pairs.is_some() {
        Some(heap.allocate(HeapData::Dict(Dict::new()))?)
    } else {
        None
    };
    let mut placeholder_slots = Vec::with_capacity(slot_values.len());
    placeholder_slots.resize_with(slot_values.len(), || Value::Undefined);
    let copied_id = allocate_memoized_placeholder(
        heap_id,
        HeapData::Instance(Instance::new(
            class_id,
            placeholder_attrs_id,
            placeholder_slots,
            Vec::new(),
        )),
        memo_id,
        heap,
        interns,
    )?;

    let mut copied_slots = Vec::with_capacity(slot_values.len());
    for slot_value in slot_values {
        if matches!(slot_value, Value::Undefined) {
            copied_slots.push(Value::Undefined);
        } else {
            copied_slots.push(deep_copy_value(slot_value, memo_id, heap, interns)?);
        }
    }

    if let Some(pairs) = attrs_pairs {
        let mut copied_pairs = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let copied_value = deep_copy_value(value, memo_id, heap, interns)?;
            copied_pairs.push((key, copied_value));
        }
        let mut copied_attrs = Dict::from_pairs(copied_pairs, heap, interns)?;
        if has_custom_deepcopy {
            set_dict_bool_attr(&mut copied_attrs, "deepcopy_called", true, heap, interns)?;
        }
        if let Some(attrs_id) = placeholder_attrs_id {
            heap.with_entry_mut(attrs_id, |_heap_inner, data| {
                if let HeapData::Dict(dict) = data {
                    *dict = copied_attrs;
                } else {
                    unreachable!("deepcopy instance attrs placeholder replaced with non-dict data");
                }
            });
        }
    }

    heap.with_entry_mut(copied_id, |_heap_inner, data| {
        if let HeapData::Instance(instance) = data {
            *instance = Instance::new(class_id, placeholder_attrs_id, copied_slots, Vec::new());
        } else {
            unreachable!("deepcopy instance placeholder replaced with non-instance data");
        }
    });

    Ok(Value::Ref(copied_id))
}

/// Implementation of `copy.replace(obj, /, **changes)`.
fn copy_replace(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(obj) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "replace expected at least 1 argument, got 0".to_string(),
        ));
    };
    if positional.len() > 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "replace expected exactly 1 positional argument".to_string(),
        ));
    }
    positional.drop_with_heap(heap);

    if let Value::Ref(id) = &obj
        && matches!(heap.get(*id), HeapData::NamedTuple(_))
    {
        return replace_namedtuple(*id, kwargs, heap, interns);
    }

    let result = super::dataclasses::call(
        heap,
        interns,
        super::dataclasses::DataclassesFunctions::Replace,
        ArgValues::ArgsKargs {
            args: vec![obj],
            kwargs,
        },
    )?;
    match result {
        AttrCallResult::Value(value) => Ok(value),
        AttrCallResult::OsCall(_, os_args) => {
            os_args.drop_with_heap(heap);
            Err(ExcType::type_error("copy.replace returned unsupported os call result"))
        }
        AttrCallResult::ExternalCall(_, ext_args) => {
            ext_args.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported external call result",
            ))
        }
        AttrCallResult::PropertyCall(getter, instance) => {
            getter.drop_with_heap(heap);
            instance.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported property call result",
            ))
        }
        AttrCallResult::DescriptorGet(descriptor) => {
            descriptor.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported descriptor call result",
            ))
        }
        AttrCallResult::ReduceCall(function, accumulator, items) => {
            function.drop_with_heap(heap);
            accumulator.drop_with_heap(heap);
            items.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported reduce call result",
            ))
        }
        AttrCallResult::MapCall(function, iterators) => {
            function.drop_with_heap(heap);
            for values in iterators {
                values.drop_with_heap(heap);
            }
            Err(ExcType::type_error("copy.replace returned unsupported map call result"))
        }
        AttrCallResult::FilterCall(function, items) => {
            function.drop_with_heap(heap);
            items.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported filter call result",
            ))
        }
        AttrCallResult::GroupByCall(function, items) => {
            function.drop_with_heap(heap);
            items.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported groupby call result",
            ))
        }
        AttrCallResult::TextwrapIndentCall(predicate, _, _) => {
            predicate.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported textwrap call result",
            ))
        }
        AttrCallResult::CallFunction(callable, call_args) => {
            callable.drop_with_heap(heap);
            call_args.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported deferred call result",
            ))
        }
        AttrCallResult::ObjectNew => Err(ExcType::type_error(
            "copy.replace returned unsupported object-new result",
        )),
        AttrCallResult::FilterFalseCall(function, items) => {
            function.drop_with_heap(heap);
            items.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported filterfalse call result",
            ))
        }
        AttrCallResult::TakeWhileCall(function, items) => {
            function.drop_with_heap(heap);
            items.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported takewhile call result",
            ))
        }
        AttrCallResult::DropWhileCall(function, items) => {
            function.drop_with_heap(heap);
            items.drop_with_heap(heap);
            Err(ExcType::type_error(
                "copy.replace returned unsupported dropwhile call result",
            ))
        }
        AttrCallResult::ReSubCall(callable, matches, _string, _is_bytes, _return_count) => {
            callable.drop_with_heap(heap);
            for (_start, _end, match_val) in matches {
                match_val.drop_with_heap(heap);
            }
            Err(ExcType::type_error(
                "copy.replace returned unsupported re.sub call result",
            ))
        }
    }
}

/// Namedtuple-specific support for `copy.replace`.
fn replace_namedtuple(
    tuple_id: HeapId,
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (name, field_names, mut items) = match heap.get(tuple_id) {
        HeapData::NamedTuple(nt) => (
            nt.name(interns).to_string(),
            nt.field_names().to_vec(),
            nt.as_vec()
                .iter()
                .map(|value| value.clone_with_heap(heap))
                .collect::<Vec<_>>(),
        ),
        _ => unreachable!("replace_namedtuple called on non-namedtuple"),
    };

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap).map(|name| name.as_str(interns).to_owned()) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for item in items {
                item.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings".to_string()));
        };
        let Some(index) = field_names
            .iter()
            .position(|field_name| field_name.as_str(interns) == key_name)
        else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for item in items {
                item.drop_with_heap(heap);
            }
            return Err(ExcType::type_error(format!(
                "got an unexpected keyword argument '{key_name}'"
            )));
        };
        key.drop_with_heap(heap);
        let old_value = std::mem::replace(&mut items[index], value);
        old_value.drop_with_heap(heap);
    }

    let replaced = crate::types::NamedTuple::new(name, field_names, items);
    let replaced_id = heap.allocate(HeapData::NamedTuple(replaced))?;
    Ok(Value::Ref(replaced_id))
}

/// Deep copy a single value (handles both immediate and heap-allocated).
fn deep_copy_value(
    obj: Value,
    memo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match obj {
        // Immutable/immediate values - return as-is
        Value::Undefined
        | Value::Ellipsis
        | Value::None
        | Value::NotImplemented
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::InternString(_)
        | Value::InternBytes(_)
        | Value::InternLongInt(_)
        | Value::Builtin(_)
        | Value::ModuleFunction(_)
        | Value::DefFunction(_)
        | Value::ExtFunction(_)
        | Value::Proxy(_)
        | Value::Marker(_)
        | Value::Property(_)
        | Value::ExternalFuture(_) => Ok(obj),

        // Heap-allocated values - check memo and copy
        Value::Ref(heap_id) => {
            // Check memo first
            if let Some(copied) = check_memo(heap_id, memo_id, heap, interns)? {
                obj.drop_with_heap(heap);
                return Ok(copied);
            }

            // Perform deep copy
            let copied = deep_copy_heap(heap_id, memo_id, heap, interns)?;

            // Clone for memo
            let copied_for_memo = copied.clone_with_heap(heap);

            // Store in memo
            store_in_memo(heap_id, copied_for_memo, memo_id, heap, interns)?;

            // Drop original
            obj.drop_with_heap(heap);

            Ok(copied)
        }

        #[cfg(feature = "ref-count-panic")]
        Value::Dereferenced => {
            obj.drop_with_heap(heap);
            Err(SimpleException::new_msg(ExcType::RuntimeError, "cannot copy dereferenced object").into())
        }
    }
}
