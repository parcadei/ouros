use std::fmt::Write;

use ahash::AHashSet;
use hashbrown::{HashTable, hash_table::Entry};
use serde::ser::SerializeStruct;
use smallvec::smallvec;

use super::{OurosIter, PyTrait, SetStorage, allocate_tuple};
use crate::{
    args::{ArgValues, KwargsValues},
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings, StringId},
    py_hash::cpython_hash_str_seed0,
    resource::ResourceTracker,
    types::Type,
    value::{EitherStr, Value},
};

/// Python dict type preserving insertion order.
///
/// This type provides Python dict semantics including dynamic key-value namespaces,
/// reference counting for heap values, and standard dict methods.
///
/// # Implemented Methods
/// - `get(key[, default])` - Get value or default
/// - `keys()` - Return view of keys
/// - `values()` - Return view of values
/// - `items()` - Return view of (key, value) pairs
/// - `pop(key[, default])` - Remove and return value
/// - `clear()` - Remove all items
/// - `copy()` - Shallow copy
/// - `update(other)` - Update from dict or iterable of pairs
/// - `setdefault(key[, default])` - Get or set default value
/// - `popitem()` - Remove and return last (key, value) pair
/// - `fromkeys(iterable[, value])` - Create dict from keys (classmethod)
///
/// All dict methods from Python's builtins are implemented.
///
/// # Storage Strategy
/// Uses a `HashTable<usize>` for hash lookups combined with a dense `Vec<DictEntry>`
/// to preserve insertion order (matching Python 3.7+ behavior). The hash table maps
/// key hashes to indices in the entries vector. This design provides O(1) lookups
/// while maintaining insertion order for iteration.
///
/// # Reference Counting
/// When values are added via `set()`, their reference counts are incremented.
/// When using `from_pairs()`, ownership is transferred without incrementing refcounts
/// (caller must ensure values' refcounts account for the dict's reference).
///
/// # GC Optimization
/// The `contains_refs` flag tracks whether the dict contains any `Value::Ref` items.
/// This allows `collect_child_ids` and `py_dec_ref_ids` to skip iteration when the
/// dict contains only primitive values (ints, bools, None, etc.), significantly
/// improving GC performance for dicts of primitives.
#[derive(Debug, Default)]
pub(crate) struct Dict {
    /// indices mapping from the entry hash to its index.
    indices: HashTable<usize>,
    /// entries is a dense vec maintaining entry order.
    entries: Vec<DictEntry>,
    /// True if any key or value in the dict is a `Value::Ref`. Used to skip iteration
    /// in `collect_child_ids` and `py_dec_ref_ids` when no refs are present.
    /// Only transitions from false to true (never back) since tracking removals would be O(n).
    contains_refs: bool,
    /// True when this dict was created via `collections.UserDict` and should expose `.data`.
    user_data_attr: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DictEntry {
    key: Value,
    value: Value,
    /// the hash is needed here for correct use of insert_unique
    hash: u64,
}

impl Dict {
    /// Creates a new empty dict.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            indices: HashTable::with_capacity(capacity),
            entries: Vec::with_capacity(capacity),
            contains_refs: false,
            user_data_attr: false,
        }
    }

    /// Returns whether this dict contains any heap references (`Value::Ref`).
    ///
    /// Used during allocation to determine if this container could create cycles,
    /// and in `collect_child_ids` and `py_dec_ref_ids` to skip iteration when no refs
    /// are present.
    ///
    /// Note: This flag only transitions from false to true (never back). When a ref is
    /// removed via `pop()`, we do NOT recompute the flag because that would be O(n).
    /// This is conservative - we may iterate unnecessarily if all refs were removed,
    /// but we'll never skip iteration when refs exist.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        self.contains_refs
    }

    /// Marks this dict as exposing a `data` attribute (UserDict compatibility).
    pub fn set_user_data_attr(&mut self) {
        self.user_data_attr = true;
    }

    /// Creates a dict from a vector of (key, value) pairs.
    ///
    /// Assumes the caller is transferring ownership of all keys and values in the pairs.
    /// Does NOT increment reference counts since ownership is being transferred.
    /// Returns Err if any key is unhashable (e.g., list, dict).
    pub fn from_pairs(
        pairs: Vec<(Value, Value)>,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let mut dict = Self::with_capacity(pairs.len());
        let mut pairs_iter = pairs.into_iter();
        for (key, value) in pairs_iter.by_ref() {
            if let Err(err) = dict.set_transfer_ownership(key, value, heap, interns) {
                for (k, v) in pairs_iter {
                    k.drop_with_heap(heap);
                    v.drop_with_heap(heap);
                }
                dict.drop_all_entries(heap);
                return Err(err);
            }
        }
        Ok(dict)
    }

    /// Internal method to set a key-value pair without incrementing refcounts.
    ///
    /// Used when ownership is being transferred (e.g., from_pairs) rather than shared.
    /// The caller must ensure the values' refcounts already account for this dict's reference.
    fn set_transfer_ownership(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        // Track if we're adding a reference for GC optimization
        if matches!(key, Value::Ref(_)) || matches!(value, Value::Ref(_)) {
            self.contains_refs = true;
        }

        let (opt_index, hash) = match self.find_index_hash(&key, heap, interns) {
            Ok((index, hash)) => (index, hash),
            Err(err) => {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(err);
            }
        };

        // Check if key already exists in bucket
        if let Some(index) = opt_index {
            // Key exists, replace in place to preserve insertion order.
            // The new duplicate key must be dropped since we keep the existing key.
            // The old value must also be dropped since we're replacing it.
            let existing_bucket = &mut self.entries[index];
            let old_value = std::mem::replace(&mut existing_bucket.value, value);
            old_value.drop_with_heap(heap);
            key.drop_with_heap(heap);
        } else {
            // Key doesn't exist, add new pair to indices and entries
            let index = self.entries.len();
            self.entries.push(DictEntry { key, value, hash });
            self.indices
                .insert_unique(hash, index, |index| self.entries[*index].hash);
        }
        Ok(())
    }

    pub(crate) fn drop_all_entries(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        for entry in self.entries.drain(..) {
            entry.key.drop_with_heap(heap);
            entry.value.drop_with_heap(heap);
        }
        self.indices.clear();
    }

    /// Gets a value from the dict by key.
    ///
    /// Returns Ok(Some(value)) if key exists, Ok(None) if key doesn't exist.
    /// Returns Err if key is unhashable.
    pub fn get(
        &self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<&Value>> {
        if let Some(index) = self.find_index_hash(key, heap, interns)?.0 {
            Ok(Some(&self.entries[index].value))
        } else {
            Ok(None)
        }
    }

    /// Gets a value from the dict by string key name (immutable lookup).
    ///
    /// This is an O(1) lookup that doesn't require mutable heap access.
    /// Only works for string keys - returns None if the key is not found.
    pub fn get_by_str(&self, key_str: &str, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<&Value> {
        // Compute hash for the string key
        let hash = cpython_hash_str_seed0(key_str);

        // Find entry with matching hash and key
        self.indices
            .find(hash, |&idx| {
                let entry_key = &self.entries[idx].key;
                match entry_key {
                    Value::InternString(id) => interns.get_str(*id) == key_str,
                    Value::Ref(id) => {
                        if let HeapData::Str(s) = heap.get(*id) {
                            s.as_str() == key_str
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            })
            .map(|&idx| &self.entries[idx].value)
    }

    /// Creates a shallow clone of this dict with proper refcount handling.
    ///
    /// Clones each key and value with `clone_with_heap` and builds a new Dict.
    /// This is useful when a caller needs an owned `Dict` (e.g., for kwargs)
    /// without mutating or consuming the original.
    pub fn clone_with_heap(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let pairs: Vec<(Value, Value)> = self
            .iter()
            .map(|(k, v)| (k.clone_with_heap(heap), v.clone_with_heap(heap)))
            .collect();
        Self::from_pairs(pairs, heap, interns)
    }

    /// Removes and returns a key-value pair by string key name.
    ///
    /// Matches both interned strings and heap-allocated strings.
    /// Returns Some((key, value)) if found, None if not found.
    /// Caller owns the returned key/value and must manage refcounts.
    pub fn pop_by_str(
        &mut self,
        key_str: &str,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<(Value, Value)> {
        let hash = cpython_hash_str_seed0(key_str);

        let entry = self.indices.find_entry(hash, |&idx| {
            let entry_key = &self.entries[idx].key;
            match entry_key {
                Value::InternString(id) => interns.get_str(*id) == key_str,
                Value::Ref(id) => matches!(heap.get(*id), HeapData::Str(s) if s.as_str() == key_str),
                _ => false,
            }
        });

        let Ok(entry) = entry else {
            return None;
        };

        let idx = *entry.get();
        entry.remove();
        let entry = self.entries.remove(idx);
        Some((entry.key, entry.value))
    }

    /// Sets a key-value pair in the dict.
    ///
    /// The caller transfers ownership of `key` and `value` to the dict. Their refcounts
    /// are NOT incremented here - the caller is responsible for ensuring the refcounts
    /// were already incremented (e.g., via `clone_with_heap` or `evaluate_use`).
    ///
    /// If the key already exists, replaces the old value and returns it (caller now
    /// owns the old value and is responsible for its refcount).
    /// Returns Err if key is unhashable.
    pub fn set(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        // Track if we're adding a reference for GC optimization
        if matches!(key, Value::Ref(_)) || matches!(value, Value::Ref(_)) {
            self.contains_refs = true;
        }

        // Handle hash computation errors explicitly so we can drop key/value properly
        let (opt_index, hash) = match self.find_index_hash(&key, heap, interns) {
            Ok(result) => result,
            Err(e) => {
                // Drop the key and value before returning the error
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(e);
            }
        };

        let entry = DictEntry { key, value, hash };
        if let Some(index) = opt_index {
            // Key exists, replace in place to preserve insertion order
            let old_entry = std::mem::replace(&mut self.entries[index], entry);

            // Decrement refcount for old key (we're discarding it)
            old_entry.key.drop_with_heap(heap);
            // Transfer ownership of the old value to caller (no clone needed)
            Ok(Some(old_entry.value))
        } else {
            // Key doesn't exist, add new pair to indices and entries
            let index = self.entries.len();
            self.entries.push(entry);
            self.indices
                .insert_unique(hash, index, |index| self.entries[*index].hash);
            Ok(None)
        }
    }

    /// Sets a key-value pair while preserving the original key object on equality matches.
    ///
    /// This is used by `weakref.WeakKeyDictionary`, which updates the value when an
    /// equal key already exists but keeps the first inserted key object.
    ///
    /// The caller transfers ownership of `key` and `value`.
    /// Returns `Some(old_value)` when an equal key existed.
    pub fn set_preserve_existing_equal_key(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        if matches!(key, Value::Ref(_)) || matches!(value, Value::Ref(_)) {
            self.contains_refs = true;
        }

        for entry in &mut self.entries {
            if weak_key_matches_existing(&key, &entry.key, heap, interns) {
                key.drop_with_heap(heap);
                let old_value = std::mem::replace(&mut entry.value, value);
                return Ok(Some(old_value));
            }
        }

        self.set(key, value, heap, interns)
    }

    /// Removes and returns a key-value pair from the dict.
    ///
    /// Returns Ok(Some((key, value))) if key exists, Ok(None) if key doesn't exist.
    /// Returns Err if key is unhashable.
    ///
    /// Reference counting: does not decrement refcounts for removed key and value;
    /// caller assumes ownership and is responsible for managing their refcounts.
    pub fn pop(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<(Value, Value)>> {
        let hash = key
            .py_hash(heap, interns)
            .ok_or_else(|| ExcType::type_error_unhashable_dict_key(key.py_type(heap)))?;

        let entry = self.indices.entry(
            hash,
            |v| key.py_eq(&self.entries[*v].key, heap, interns),
            |index| self.entries[*index].hash,
        );

        if let Entry::Occupied(occ_entry) = entry {
            let removed_index = *occ_entry.get();
            let entry = self.entries.remove(removed_index);
            occ_entry.remove();
            // Entries after the removed slot shift left by one, so stored indices
            // in the hash table must be updated to stay aligned with `entries`.
            for index in &mut self.indices {
                if *index > removed_index {
                    *index -= 1;
                }
            }
            // Don't decrement refcounts - caller now owns the values
            Ok(Some((entry.key, entry.value)))
        } else {
            Ok(None)
        }
    }

    /// Returns a vector of all keys in the dict with proper reference counting.
    ///
    /// Each key's reference count is incremented since the returned vector
    /// now holds additional references to these values.
    #[must_use]
    pub fn keys(&self, heap: &mut Heap<impl ResourceTracker>) -> Vec<Value> {
        self.entries
            .iter()
            .map(|entry| entry.key.clone_with_heap(heap))
            .collect()
    }

    /// Returns a vector of all values in the dict with proper reference counting.
    ///
    /// Each value's reference count is incremented since the returned vector
    /// now holds additional references to these values.
    #[must_use]
    pub fn values(&self, heap: &mut Heap<impl ResourceTracker>) -> Vec<Value> {
        self.entries
            .iter()
            .map(|entry| entry.value.clone_with_heap(heap))
            .collect()
    }

    /// Returns a vector of all (key, value) pairs in the dict with proper reference counting.
    ///
    /// Each key and value's reference count is incremented since the returned vector
    /// now holds additional references to these values.
    #[must_use]
    pub fn items(&self, heap: &mut Heap<impl ResourceTracker>) -> Vec<(Value, Value)> {
        self.entries
            .iter()
            .map(|entry| (entry.key.clone_with_heap(heap), entry.value.clone_with_heap(heap)))
            .collect()
    }

    /// Returns the number of key-value pairs in the dict.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the dict is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns an iterator over references to (key, value) pairs.
    pub fn iter(&self) -> DictIter<'_> {
        self.into_iter()
    }

    /// Returns the key at the given iteration index, or None if out of bounds.
    ///
    /// Used for index-based iteration in for loops. Returns a reference to
    /// the key at the given position in insertion order.
    pub fn key_at(&self, index: usize) -> Option<&Value> {
        self.entries.get(index).map(|e| &e.key)
    }

    /// Creates a dict from the `dict()` constructor call.
    ///
    /// - `dict()` with no args returns an empty dict
    /// - `dict(mapping)` where mapping is Dict/DefaultDict/Counter/OrderedDict
    ///   returns a shallow copy of key-value pairs
    /// - `dict(iterable)` expects iterable items to be `(key, value)` pairs
    /// - `dict(**kwargs)` adds keyword pairs
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (mut positional, kwargs) = args.into_parts();
        let positional_count = positional.len();
        if positional_count > 1 {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error_at_most("dict", 1, positional_count));
        }

        let other = positional.next();
        positional.drop_with_heap(heap);

        let mut dict = Self::new();
        if let Some(other) = other
            && let Err(err) = dict_update_from_value(&mut dict, other, heap, interns)
        {
            kwargs.drop_with_heap(heap);
            dict.drop_all_entries(heap);
            return Err(err);
        }

        if let Err(err) = dict_update_from_kwargs(&mut dict, kwargs, heap, interns) {
            dict.drop_all_entries(heap);
            return Err(err);
        }

        let heap_id = heap.allocate(HeapData::Dict(dict))?;
        Ok(Value::Ref(heap_id))
    }
}

/// Returns cloned `(key, value)` pairs when `value` is a supported mapping type.
///
/// This is used by `dict()` and `dict.update()` to implement shallow mapping-copy
/// semantics for the builtin dict and common collections wrappers.
fn mapping_items_for_dict_update(value: &Value, heap: &mut Heap<impl ResourceTracker>) -> Option<Vec<(Value, Value)>> {
    let Value::Ref(id) = value else {
        return None;
    };

    heap.with_entry_mut(*id, |heap_inner, data| match data {
        HeapData::Dict(dict) => Some(dict.items(heap_inner)),
        HeapData::DefaultDict(default_dict) => Some(default_dict.dict().items(heap_inner)),
        HeapData::Counter(counter) => Some(counter.dict().items(heap_inner)),
        HeapData::OrderedDict(ordered_dict) => Some(ordered_dict.dict().items(heap_inner)),
        HeapData::ChainMap(chain_map) => Some(chain_map.flat_items(heap_inner)),
        _ => None,
    })
}

/// Updates a dict from a single positional `dict()`/`dict.update()` value.
///
/// Handles supported mapping objects by copying their items, then falls back to
/// iterable-of-pairs semantics.
fn dict_update_from_value(
    dict: &mut Dict,
    other_value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    if let Some(mapping_items) = mapping_items_for_dict_update(&other_value, heap) {
        other_value.drop_with_heap(heap);

        let mut items_iter = mapping_items.into_iter();
        while let Some((key, value)) = items_iter.next() {
            match dict.set(key, value, heap, interns) {
                Ok(Some(old_value)) => old_value.drop_with_heap(heap),
                Ok(None) => {}
                Err(err) => {
                    for (remaining_key, remaining_value) in items_iter {
                        remaining_key.drop_with_heap(heap);
                        remaining_value.drop_with_heap(heap);
                    }
                    return Err(err);
                }
            }
        }

        return Ok(());
    }

    let mut iter = OurosIter::new(other_value, heap, interns)?;
    loop {
        let item = match iter.for_next(heap, interns) {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        let mut pair_iter = match OurosIter::new(item, heap, interns) {
            Ok(pair_iter) => pair_iter,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        let key = match pair_iter.for_next(heap, interns) {
            Ok(Some(key)) => key,
            Ok(None) => {
                pair_iter.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "dictionary update sequence element has length 0; 2 is required",
                ));
            }
            Err(err) => {
                pair_iter.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        let value = match pair_iter.for_next(heap, interns) {
            Ok(Some(value)) => value,
            Ok(None) => {
                key.drop_with_heap(heap);
                pair_iter.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "dictionary update sequence element has length 1; 2 is required",
                ));
            }
            Err(err) => {
                key.drop_with_heap(heap);
                pair_iter.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        match pair_iter.for_next(heap, interns) {
            Ok(Some(first_extra)) => {
                first_extra.drop_with_heap(heap);
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                loop {
                    match pair_iter.for_next(heap, interns) {
                        Ok(Some(extra)) => extra.drop_with_heap(heap),
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                pair_iter.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "dictionary update sequence element has length > 2; 2 is required",
                ));
            }
            Ok(None) => {}
            Err(err) => {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                pair_iter.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
        pair_iter.drop_with_heap(heap);

        match dict.set(key, value, heap, interns) {
            Ok(Some(old_value)) => old_value.drop_with_heap(heap),
            Ok(None) => {}
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(())
}

impl Dict {
    fn find_index_hash(
        &self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<(Option<usize>, u64)> {
        let hash = key
            .py_hash(heap, interns)
            .ok_or_else(|| ExcType::type_error_unhashable_dict_key(key.py_type(heap)))?;

        let opt_index = self
            .indices
            .find(hash, |v| key.py_eq(&self.entries[*v].key, heap, interns))
            .copied();
        Ok((opt_index, hash))
    }
}

/// Returns true when a weak-key dictionary should treat `candidate` as an existing key.
///
/// CPython `WeakKeyDictionary` keeps identity semantics for instances that do not
/// define their own `__eq__`, but uses equality semantics when `__eq__` is defined.
fn weak_key_matches_existing(
    candidate: &Value,
    existing: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    match (candidate, existing) {
        (Value::Ref(candidate_id), Value::Ref(existing_id))
            if matches!(heap.get(*candidate_id), HeapData::Instance(_))
                && matches!(heap.get(*existing_id), HeapData::Instance(_)) =>
        {
            if candidate_id == existing_id {
                return true;
            }
            if !instance_defines_eq(*candidate_id, heap, interns) {
                return false;
            }
            candidate.py_eq(existing, heap, interns)
        }
        _ => candidate.py_eq(existing, heap, interns),
    }
}

/// Returns true if an instance's class defines `__eq__` directly in its namespace.
fn instance_defines_eq(instance_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::Instance(inst) = heap.get(instance_id) else {
        return false;
    };
    let HeapData::ClassObject(cls) = heap.get(inst.class_id()) else {
        return false;
    };
    cls.namespace().get_by_str("__eq__", heap, interns).is_some()
}

/// Iterator over borrowed (key, value) pairs in a dict.
pub(crate) struct DictIter<'a>(std::slice::Iter<'a, DictEntry>);

impl<'a> Iterator for DictIter<'a> {
    type Item = (&'a Value, &'a Value);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|e| (&e.key, &e.value))
    }
}

impl<'a> IntoIterator for &'a Dict {
    type Item = (&'a Value, &'a Value);
    type IntoIter = DictIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        DictIter(self.entries.iter())
    }
}

/// Iterator over owned (key, value) pairs from a consumed dict.
pub(crate) struct DictIntoIter(std::vec::IntoIter<DictEntry>);

impl Iterator for DictIntoIter {
    type Item = (Value, Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|e| (e.key, e.value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for DictIntoIter {}

impl IntoIterator for Dict {
    type Item = (Value, Value);
    type IntoIter = DictIntoIter;
    fn into_iter(self) -> Self::IntoIter {
        DictIntoIter(self.entries.into_iter())
    }
}

impl PyTrait for Dict {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Dict
    }

    fn py_estimate_size(&self) -> usize {
        // Dict size: struct overhead + entries (2 Values per entry for key+value)
        std::mem::size_of::<Self>() + self.len() * 2 * std::mem::size_of::<Value>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        if self.len() != other.len() {
            return false;
        }

        // Check that all keys in self exist in other with equal values
        for entry in &self.entries {
            match other.get(&entry.key, heap, interns) {
                Ok(Some(other_v)) => {
                    if !entry.value.py_eq(other_v, heap, interns) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        // Skip iteration if no refs - major GC optimization for dicts of primitives
        if !self.contains_refs {
            return;
        }
        for entry in &mut self.entries {
            if let Value::Ref(id) = &entry.key {
                stack.push(*id);
                #[cfg(feature = "ref-count-panic")]
                entry.key.dec_ref_forget();
            }
            if let Value::Ref(id) = &entry.value {
                stack.push(*id);
                #[cfg(feature = "ref-count-panic")]
                entry.value.dec_ref_forget();
            }
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.is_empty()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        if self.is_empty() {
            return f.write_str("{}");
        }

        f.write_char('{')?;
        let mut first = true;
        for entry in &self.entries {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            entry.key.py_repr_fmt(f, heap, heap_ids, interns)?;
            f.write_str(": ")?;
            entry.value.py_repr_fmt(f, heap, heap_ids, interns)?;
        }
        f.write_char('}')
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Value> {
        // Use copy_for_extend to avoid borrow conflict, then increment refcount
        let result = self.get(key, heap, interns)?.map(Value::copy_for_extend);
        match result {
            Some(value) => {
                if let Value::Ref(id) = &value {
                    heap.inc_ref(*id);
                }
                Ok(value)
            }
            None => Err(ExcType::key_error(key, heap, interns)),
        }
    }

    fn py_setitem(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        // Drop the old value if one was replaced
        if let Some(old_value) = self.set(key, value, heap, interns)? {
            old_value.drop_with_heap(heap);
        }
        Ok(())
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let Some(method) = attr.static_string() else {
            return Err(ExcType::attribute_error(Type::Dict, attr.as_str(interns)));
        };

        match method {
            StaticStrings::Get => {
                // dict.get() accepts 1 or 2 arguments
                let (key, default) = args.get_one_two_args("get", heap)?;
                let default = default.unwrap_or(Value::None);
                // Handle the lookup - may fail for unhashable keys
                let result = match self.get(&key, heap, interns) {
                    Ok(r) => r,
                    Err(e) => {
                        // Drop key and default before returning error
                        key.drop_with_heap(heap);
                        default.drop_with_heap(heap);
                        return Err(e);
                    }
                };
                let value = match result {
                    Some(v) => v.clone_with_heap(heap),
                    None => default.clone_with_heap(heap),
                };
                // Drop the key and default arguments
                key.drop_with_heap(heap);
                default.drop_with_heap(heap);
                Ok(value)
            }
            StaticStrings::Keys => {
                args.check_zero_args("dict.keys", heap)?;
                let Some(dict_id) = self_id else {
                    return Err(ExcType::type_error("dict.keys() failed to get dict id"));
                };
                // Increment refcount of the source dict since the view holds a reference to it
                heap.inc_ref(dict_id);
                let view = DictKeys::new(dict_id);
                let view_id = heap.allocate(HeapData::DictKeys(view))?;
                Ok(Value::Ref(view_id))
            }
            StaticStrings::Values => {
                args.check_zero_args("dict.values", heap)?;
                let Some(dict_id) = self_id else {
                    return Err(ExcType::type_error("dict.values() failed to get dict id"));
                };
                // Increment refcount of the source dict since the view holds a reference to it
                heap.inc_ref(dict_id);
                let view = DictValues::new(dict_id);
                let view_id = heap.allocate(HeapData::DictValues(view))?;
                Ok(Value::Ref(view_id))
            }
            StaticStrings::Items => {
                args.check_zero_args("dict.items", heap)?;
                let Some(dict_id) = self_id else {
                    return Err(ExcType::type_error("dict.items() failed to get dict id"));
                };
                // Increment refcount of the source dict since the view holds a reference to it
                heap.inc_ref(dict_id);
                let view = DictItems::new(dict_id);
                let view_id = heap.allocate(HeapData::DictItems(view))?;
                Ok(Value::Ref(view_id))
            }
            StaticStrings::Pop => {
                // dict.pop() accepts 1 or 2 arguments (key, optional default)
                let (key, default) = args.get_one_two_args("pop", heap)?;
                let result = match self.pop(&key, heap, interns) {
                    Ok(r) => r,
                    Err(e) => {
                        // Clean up key and default before returning error
                        key.drop_with_heap(heap);
                        if let Some(d) = default {
                            d.drop_with_heap(heap);
                        }
                        return Err(e);
                    }
                };
                if let Some((old_key, value)) = result {
                    // Drop the old key - we don't need it
                    old_key.drop_with_heap(heap);
                    // Drop the lookup key and default arguments
                    key.drop_with_heap(heap);
                    if let Some(d) = default {
                        d.drop_with_heap(heap);
                    }
                    Ok(value)
                } else {
                    // No matching key - return default if provided, else KeyError
                    if let Some(d) = default {
                        key.drop_with_heap(heap);
                        Ok(d)
                    } else {
                        let err = ExcType::key_error(&key, heap, interns);
                        key.drop_with_heap(heap);
                        Err(err)
                    }
                }
            }
            StaticStrings::Clear => {
                args.check_zero_args("dict.clear", heap)?;
                dict_clear(self, heap);
                Ok(Value::None)
            }
            StaticStrings::Copy => {
                args.check_zero_args("dict.copy", heap)?;
                let copied = dict_copy(self, heap, interns)?;
                if let Some(source_id) = self_id
                    && let Value::Ref(copy_id) = &copied
                {
                    if heap.is_weak_value_dict(source_id) {
                        heap.mark_weak_value_dict(*copy_id);
                    }
                    if heap.is_weak_key_dict(source_id) {
                        heap.mark_weak_key_dict(*copy_id);
                    }
                }
                Ok(copied)
            }
            StaticStrings::Update => dict_update(self, args, heap, interns),
            StaticStrings::Setdefault => dict_setdefault(self, args, heap, interns),
            StaticStrings::DunderSetitem => {
                let (key, value) = args.get_two_args("__setitem__", heap)?;
                self.py_setitem(key, value, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::Popitem => {
                args.check_zero_args("dict.popitem", heap)?;
                dict_popitem(self, heap)
            }
            // fromkeys is a classmethod but also accessible on instances
            StaticStrings::Fromkeys => dict_fromkeys(args, heap, interns),
            _ => Err(ExcType::attribute_error(Type::Dict, attr.as_str(interns))),
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<super::AttrCallResult>> {
        if self.user_data_attr && interns.get_str(attr_id) == "data" {
            let mut copied = self.clone_with_heap(heap, interns)?;
            copied.set_user_data_attr();
            let id = heap.allocate(HeapData::Dict(copied))?;
            return Ok(Some(super::AttrCallResult::Value(Value::Ref(id))));
        }
        Ok(None)
    }
}

/// Implements Python's `dict.clear()` method.
///
/// Removes all items from the dict.
fn dict_clear(dict: &mut Dict, heap: &mut Heap<impl ResourceTracker>) {
    for entry in dict.entries.drain(..) {
        entry.key.drop_with_heap(heap);
        entry.value.drop_with_heap(heap);
    }
    dict.indices.clear();
    // Note: contains_refs stays true even if all refs removed, per conservative GC strategy
}

/// Implements Python's `dict.copy()` method.
///
/// Returns a shallow copy of the dict.
fn dict_copy(dict: &Dict, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    // Copy all key-value pairs (incrementing refcounts)
    let pairs: Vec<(Value, Value)> = dict
        .iter()
        .map(|(k, v)| (k.clone_with_heap(heap), v.clone_with_heap(heap)))
        .collect();

    let new_dict = Dict::from_pairs(pairs, heap, interns)?;
    let heap_id = heap.allocate(HeapData::Dict(new_dict))?;
    Ok(Value::Ref(heap_id))
}

/// Implements Python's `dict.update([other], **kwargs)` method.
///
/// Updates the dict with key-value pairs from `other` and/or `kwargs`.
/// If `other` is a supported mapping type, copies its key-value pairs.
/// If `other` is an iterable, expects pairs of (key, value).
/// Keyword arguments are also added to the dict.
fn dict_update(
    dict: &mut Dict,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (pos, kwargs) = args.into_parts();

    let mut pos_iter = pos;
    let other = pos_iter.next();

    // Check no extra positional arguments
    if let Some(extra) = pos_iter.next() {
        extra.drop_with_heap(heap);
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        if let Some(v) = other {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("dict.update", 1, 2));
    }

    // Process positional argument if present
    let Some(other_value) = other else {
        // No positional argument - just process kwargs
        return dict_update_from_kwargs(dict, kwargs, heap, interns);
    };

    if let Err(err) = dict_update_from_value(dict, other_value, heap, interns) {
        kwargs.drop_with_heap(heap);
        return Err(err);
    }

    dict_update_from_kwargs(dict, kwargs, heap, interns)
}

/// Helper to update a dict from keyword arguments.
fn dict_update_from_kwargs(
    dict: &mut Dict,
    kwargs: KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    // Use while let to allow draining on error
    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        // Drop key, value, and remaining kwargs before propagating error
        match dict.set(key, value, heap, interns) {
            Ok(Some(old_value)) => old_value.drop_with_heap(heap),
            Ok(None) => {}
            Err(e) => {
                for (k, v) in kwargs_iter {
                    k.drop_with_heap(heap);
                    v.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    }
    Ok(Value::None)
}

/// Implements Python's `dict.setdefault(key[, default])` method.
///
/// If key is in the dict, return its value.
/// If not, insert key with a value of default (or None) and return default.
fn dict_setdefault(
    dict: &mut Dict,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (key, default) = args.get_one_two_args("setdefault", heap)?;
    let default = default.unwrap_or(Value::None);

    // Check if key exists
    let result = match dict.get(&key, heap, interns) {
        Ok(r) => r,
        Err(e) => {
            key.drop_with_heap(heap);
            default.drop_with_heap(heap);
            return Err(e);
        }
    };

    if let Some(existing) = result {
        // Key exists - return its value (cloned)
        let value = existing.clone_with_heap(heap);
        key.drop_with_heap(heap);
        default.drop_with_heap(heap);
        Ok(value)
    } else {
        // Key doesn't exist - insert default and return it (cloned before insertion)
        let return_value = default.clone_with_heap(heap);
        let mut return_value_guard = HeapGuard::new(return_value, heap);
        let heap = return_value_guard.heap();
        if let Some(old_value) = dict.set(key, default, heap, interns)? {
            // This shouldn't happen since we checked, but handle it anyway
            old_value.drop_with_heap(heap);
        }
        Ok(return_value_guard.into_inner())
    }
}

/// Implements Python's `dict.popitem()` method.
///
/// Removes and returns the last inserted key-value pair as a tuple.
/// Raises KeyError if the dict is empty.
fn dict_popitem(dict: &mut Dict, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if dict.is_empty() {
        return Err(ExcType::key_error_popitem_empty_dict());
    }

    // Remove the last entry (LIFO order)
    let entry = dict.entries.pop().expect("dict is not empty");

    // Remove from indices - need to find the entry with this index
    // Since we removed the last entry, we need to clear and rebuild indices
    // (This is simpler than trying to find and remove the specific hash entry)
    // TODO: This O(n) rebuild could be optimized by finding and removing the
    // specific hash entry directly from the hashbrown table.
    dict.indices.clear();
    for (idx, e) in dict.entries.iter().enumerate() {
        dict.indices.insert_unique(e.hash, idx, |&i| dict.entries[i].hash);
    }

    // Create tuple (key, value)
    Ok(allocate_tuple(smallvec![entry.key, entry.value], heap)?)
}

// Custom serde implementation for Dict.
// Serializes entries and contains_refs; rebuilds the indices hash table on deserialize.
impl serde::Serialize for Dict {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Dict", 3)?;
        state.serialize_field("entries", &self.entries)?;
        state.serialize_field("contains_refs", &self.contains_refs)?;
        state.serialize_field("user_data_attr", &self.user_data_attr)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for Dict {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct DictFields {
            entries: Vec<DictEntry>,
            contains_refs: bool,
            #[serde(default)]
            user_data_attr: bool,
        }
        let fields = DictFields::deserialize(deserializer)?;
        // Rebuild the indices hash table from the entries
        let mut indices = HashTable::with_capacity(fields.entries.len());
        for (idx, entry) in fields.entries.iter().enumerate() {
            indices.insert_unique(entry.hash, idx, |&i| fields.entries[i].hash);
        }
        Ok(Self {
            indices,
            entries: fields.entries,
            contains_refs: fields.contains_refs,
            user_data_attr: fields.user_data_attr,
        })
    }
}

/// A dynamic view of a dict's keys.
///
/// This type implements Python's `dict_keys` view object. It holds a reference to
/// the source dict and reflects live updates to the dict. Views support iteration,
/// membership testing (`in` operator), and set operations (`|`, `&`, `-`, `^`).
///
/// The view holds a `HeapId` reference to the source dict. The dict's refcount
/// is not incremented by the view - in CPython, views keep the dict alive, but
/// for now we implement simpler semantics where views become invalid if the
/// underlying dict is deleted.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DictKeys {
    /// HeapId of the source dict
    dict_id: HeapId,
}

impl DictKeys {
    /// Creates a new DictKeys view referencing the given dict.
    #[must_use]
    pub fn new(dict_id: HeapId) -> Self {
        Self { dict_id }
    }

    /// Returns the HeapId of the source dict.
    #[must_use]
    pub(crate) fn dict_id(&self) -> HeapId {
        self.dict_id
    }

    /// Returns a reference to the source dict from the heap.
    ///
    /// Handles both plain `Dict` and wrapper types (`Counter`, `OrderedDict`)
    /// that store their data in an inner dict, so that dict views created from
    /// these types can correctly access the underlying entries.
    pub(crate) fn get_dict<'a>(&self, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a Dict> {
        match heap.get(self.dict_id) {
            HeapData::Dict(d) => Some(d),
            HeapData::Counter(c) => Some(c.dict()),
            HeapData::OrderedDict(od) => Some(od.dict()),
            _ => None,
        }
    }

    /// Returns a mutable reference to the source dict from the heap.
    ///
    /// Handles both plain `Dict` and wrapper types (`Counter`, `OrderedDict`)
    /// that store their data in an inner dict.
    pub(crate) fn get_dict_mut<'a>(&self, heap: &'a mut Heap<impl ResourceTracker>) -> Option<&'a mut Dict> {
        match heap.get_mut(self.dict_id) {
            HeapData::Dict(d) => Some(d),
            HeapData::Counter(c) => Some(c.dict_mut()),
            HeapData::OrderedDict(od) => Some(od.dict_mut()),
            _ => None,
        }
    }
}

impl PyTrait for DictKeys {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::DictKeys
    }

    fn py_len(&self, heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        self.get_dict(heap).map(Dict::len)
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        // Get both dict refs first
        let (d1_len, d2_len) = {
            let Some(d1) = self.get_dict(heap) else {
                return false;
            };
            let Some(d2) = other.get_dict(heap) else {
                return false;
            };
            (d1.len(), d2.len())
        };
        if d1_len != d2_len {
            return false;
        }
        // Collect keys from d1 first
        let keys: Vec<Value> = {
            let Some(d1) = self.get_dict(heap) else {
                return false;
            };
            d1.iter().map(|(k, _)| k.copy_for_extend()).collect()
        };
        // Check that all keys in d1 exist in d2
        // Use with_entry_mut to get mutable access to d2 without borrowing heap
        for key in keys {
            let exists = if let Ok(v) = heap.with_entry_mut(other.dict_id, |heap, d2_data| {
                let HeapData::Dict(d2) = d2_data else {
                    return Ok(false);
                };
                d2.get(&key, heap, interns).map(|opt| opt.is_some())
            }) {
                v
            } else {
                key.drop_with_heap(heap);
                return false;
            };
            key.drop_with_heap(heap);
            if !exists {
                return false;
            }
        }
        true
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        stack.push(self.dict_id);
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("dict_keys([")?;
        if let Some(dict) = self.get_dict(heap) {
            let mut first = true;
            for entry in dict {
                if !first {
                    f.write_str(", ")?;
                }
                first = false;
                entry.0.py_repr_fmt(f, heap, heap_ids, interns)?;
            }
        }
        f.write_str("])")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl DictKeys {
    /// Collects all keys into a SetStorage for set operations.
    pub(crate) fn to_set_storage(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        // Collect keys first to avoid borrow issues
        let keys: Vec<Value> = if let Some(dict) = self.get_dict(heap) {
            dict.iter().map(|(k, _)| k.copy_for_extend()).collect()
        } else {
            Vec::new()
        };

        let mut storage = SetStorage::new();
        for key in keys {
            // Increment refcount for the copied key
            if let Value::Ref(id) = key {
                heap.inc_ref(id);
            }
            storage.add(key, heap, interns)?;
        }
        Ok(storage)
    }

    /// Returns a new set with elements from both this and another set-like.
    pub(crate) fn union(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.union(other, heap, interns)
    }

    /// Returns a new set with elements common to both.
    pub(crate) fn intersection(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.intersection(other, heap, interns)
    }

    /// Returns a new set with elements in this but not in other.
    pub(crate) fn difference(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.difference(other, heap, interns)
    }

    /// Returns a new set with elements in either but not both.
    pub(crate) fn symmetric_difference(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.symmetric_difference(other, heap, interns)
    }
}

/// A dynamic view of a dict's values.
///
/// This type implements Python's `dict_values` view object. It holds a reference to
/// the source dict and reflects live updates to the dict. Views support iteration
/// and membership testing (`in` operator).
///
/// The view holds a `HeapId` reference to the source dict.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DictValues {
    /// HeapId of the source dict
    dict_id: HeapId,
}

impl DictValues {
    /// Creates a new DictValues view referencing the given dict.
    #[must_use]
    pub fn new(dict_id: HeapId) -> Self {
        Self { dict_id }
    }

    /// Returns the HeapId of the source dict.
    #[must_use]
    pub(crate) fn dict_id(&self) -> HeapId {
        self.dict_id
    }

    /// Returns a reference to the source dict from the heap.
    ///
    /// Handles both plain `Dict` and wrapper types (`Counter`, `OrderedDict`)
    /// that store their data in an inner dict, so that dict views created from
    /// these types can correctly access the underlying entries.
    pub(crate) fn get_dict<'a>(&self, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a Dict> {
        match heap.get(self.dict_id) {
            HeapData::Dict(d) => Some(d),
            HeapData::Counter(c) => Some(c.dict()),
            HeapData::OrderedDict(od) => Some(od.dict()),
            _ => None,
        }
    }
}

impl PyTrait for DictValues {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::DictValues
    }

    fn py_len(&self, heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        self.get_dict(heap).map(Dict::len)
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        let Some(d1) = self.get_dict(heap) else {
            return false;
        };
        let Some(d2) = other.get_dict(heap) else {
            return false;
        };
        if d1.len() != d2.len() {
            return false;
        }
        // Collect values from both dicts to avoid borrow issues
        let values1: Vec<Value> = d1.iter().map(|(_, v)| v.copy_for_extend()).collect();
        let values2: Vec<Value> = d2.iter().map(|(_, v)| v.copy_for_extend()).collect();
        // Values must be in the same order for equality
        let result = values1
            .iter()
            .zip(values2.iter())
            .all(|(v1, v2)| v1.py_eq(v2, heap, interns));
        // Drop the copied values
        for v in values1 {
            v.drop_with_heap(heap);
        }
        for v in values2 {
            v.drop_with_heap(heap);
        }
        result
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        stack.push(self.dict_id);
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("dict_values([")?;
        if let Some(dict) = self.get_dict(heap) {
            let mut first = true;
            for entry in dict {
                if !first {
                    f.write_str(", ")?;
                }
                first = false;
                entry.1.py_repr_fmt(f, heap, heap_ids, interns)?;
            }
        }
        f.write_str("])")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// A dynamic view of a dict's (key, value) pairs.
///
/// This type implements Python's `dict_items` view object. It holds a reference to
/// the source dict and reflects live updates to the dict. Views support iteration,
/// membership testing (`in` operator), and set operations (`|`, `&`, `-`, `^`).
///
/// The view holds a `HeapId` reference to the source dict.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DictItems {
    /// HeapId of the source dict
    dict_id: HeapId,
}

impl DictItems {
    /// Creates a new DictItems view referencing the given dict.
    #[must_use]
    pub fn new(dict_id: HeapId) -> Self {
        Self { dict_id }
    }

    /// Returns the HeapId of the source dict.
    #[must_use]
    pub(crate) fn dict_id(&self) -> HeapId {
        self.dict_id
    }

    /// Returns a reference to the source dict from the heap.
    ///
    /// Handles both plain `Dict` and wrapper types (`Counter`, `OrderedDict`)
    /// that store their data in an inner dict, so that dict views created from
    /// these types can correctly access the underlying entries.
    pub(crate) fn get_dict<'a>(&self, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a Dict> {
        match heap.get(self.dict_id) {
            HeapData::Dict(d) => Some(d),
            HeapData::Counter(c) => Some(c.dict()),
            HeapData::OrderedDict(od) => Some(od.dict()),
            _ => None,
        }
    }
}

impl PyTrait for DictItems {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::DictItems
    }

    fn py_len(&self, heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        self.get_dict(heap).map(Dict::len)
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        // Get the dict_ids and use Dict::py_eq via the heap
        let self_id = self.dict_id;
        let other_id = other.dict_id;
        // Use with_two to get both dicts simultaneously
        heap.with_two(self_id, other_id, |heap, d1_data, d2_data| {
            let HeapData::Dict(d1) = d1_data else {
                return false;
            };
            let HeapData::Dict(d2) = d2_data else {
                return false;
            };
            d1.py_eq(d2, heap, interns)
        })
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        stack.push(self.dict_id);
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("dict_items([")?;
        if let Some(dict) = self.get_dict(heap) {
            let mut first = true;
            for entry in dict {
                if !first {
                    f.write_str(", ")?;
                }
                first = false;
                f.write_char('(')?;
                entry.0.py_repr_fmt(f, heap, heap_ids, interns)?;
                f.write_str(", ")?;
                entry.1.py_repr_fmt(f, heap, heap_ids, interns)?;
                f.write_char(')')?;
            }
        }
        f.write_str("])")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl DictItems {
    /// Collects all items into a SetStorage for set operations.
    /// Items are converted to (key, value) tuples.
    pub(crate) fn to_set_storage(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        // Collect (key, value) pairs first to avoid borrow issues
        let pairs: Vec<(Value, Value)> = if let Some(dict) = self.get_dict(heap) {
            dict.iter()
                .map(|(k, v)| (k.copy_for_extend(), v.copy_for_extend()))
                .collect()
        } else {
            Vec::new()
        };

        // Increment refcounts for copied values after borrowing the dict
        for (k, v) in &pairs {
            if let Value::Ref(key_id) = k {
                heap.inc_ref(*key_id);
            }
            if let Value::Ref(val_id) = v {
                heap.inc_ref(*val_id);
            }
        }

        let mut storage = SetStorage::new();
        for (key, value) in pairs {
            // Create tuple (key, value)
            let tuple_value = allocate_tuple(smallvec![key, value], heap)?;
            storage.add(tuple_value, heap, interns)?;
        }
        Ok(storage)
    }

    /// Returns a new set with elements from both this and another set-like.
    pub(crate) fn union(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.union(other, heap, interns)
    }

    /// Returns a new set with elements common to both.
    pub(crate) fn intersection(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.intersection(other, heap, interns)
    }

    /// Returns a new set with elements in this but not in other.
    pub(crate) fn difference(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.difference(other, heap, interns)
    }

    /// Returns a new set with elements in either but not both.
    pub(crate) fn symmetric_difference(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        let self_storage = self.to_set_storage(heap, interns)?;
        self_storage.symmetric_difference(other, heap, interns)
    }
}

/// Implements Python's `dict.fromkeys(iterable[, value])` classmethod.
///
/// Creates a new dictionary with keys from `iterable` and all values set to `value`
/// (default: None).
///
/// This is a classmethod that can be called directly on the dict type:
/// ```python
/// dict.fromkeys(['a', 'b', 'c'])  # {'a': None, 'b': None, 'c': None}
/// dict.fromkeys(['a', 'b'], 0)    # {'a': 0, 'b': 0}
/// ```
pub fn dict_fromkeys(args: ArgValues, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let (iterable, default) = args.get_one_two_args("dict.fromkeys", heap)?;
    let default = default.unwrap_or(Value::None);

    // Iterate over the iterable to get keys
    // Drop default before propagating error to avoid refcount leak
    let iter_result = OurosIter::new(iterable, heap, interns);
    let mut iter = match iter_result {
        Ok(i) => i,
        Err(e) => {
            default.drop_with_heap(heap);
            return Err(e);
        }
    };

    let mut dict = Dict::new();

    loop {
        // Drop iter and default before propagating error to avoid refcount leak
        let next_result = iter.for_next(heap, interns);
        let key = match next_result {
            Ok(Some(k)) => k,
            Ok(None) => break,
            Err(e) => {
                iter.drop_with_heap(heap);
                default.drop_with_heap(heap);
                return Err(e);
            }
        };

        // Clone the default value for each key
        let value = default.clone_with_heap(heap);
        // Drop key, value, iter, default before propagating error
        let set_result = dict.set(key, value, heap, interns);
        match set_result {
            Ok(Some(old_value)) => old_value.drop_with_heap(heap),
            Ok(None) => {}
            Err(e) => {
                // Note: key and value are consumed by dict.set, so we only drop iter and default
                iter.drop_with_heap(heap);
                default.drop_with_heap(heap);
                return Err(e);
            }
        }
    }

    iter.drop_with_heap(heap);
    default.drop_with_heap(heap);

    let heap_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(heap_id))
}
