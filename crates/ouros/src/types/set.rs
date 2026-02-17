use std::fmt::Write;

use ahash::AHashSet;
use hashbrown::HashTable;

use super::{OurosIter, PyTrait};
use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    py_hash::{cpython_hash_bytes_seed0_i64, cpython_hash_str_seed0},
    resource::ResourceTracker,
    types::Type,
    value::{EitherStr, Value},
};

/// Entry in the set storage, containing a value and its cached hash.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SetEntry {
    pub(crate) value: Value,
    /// Cached hash for efficient lookup and reinsertion.
    pub(crate) hash: u64,
}

/// Internal storage shared between Set and FrozenSet.
///
/// Uses a `HashTable<usize>` for O(1) lookups combined with a dense `Vec<SetEntry>`
/// to preserve insertion order (consistent with Python 3.7+ dict behavior).
/// The hash table maps value hashes to indices in the entries vector.
#[derive(Debug, Default)]
pub(crate) struct SetStorage {
    /// Maps hash to index in entries vector.
    indices: HashTable<usize>,
    /// Dense vector of entries maintaining insertion order.
    entries: Vec<SetEntry>,
}

impl SetStorage {
    /// Number of perturb bits shifted each probe step in CPython set lookup.
    const PERTURB_SHIFT: usize = 5;
    /// Number of adjacent slots CPython linearly probes before perturb stepping.
    ///
    /// This small linear scan materially affects final slot placement and therefore
    /// iteration/repr order.
    const LINEAR_PROBES: usize = 9;

    // Returns the hash value used for CPython-style repr slot simulation.
    //
    // Ouros hashes many values with Rust's `DefaultHasher` for runtime lookups,
    // while CPython set iteration order is derived from CPython hash values and
    // probe sequence. For repr/scan-order simulation we use CPython-compatible
    // hashes for the subset we can compute without mutating heap state, and fall
    // back to the cached runtime hash for all other values.
    #[inline]
    fn repr_hash(entry: &SetEntry, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> u64 {
        match entry.value {
            Value::Int(i) => {
                let cpython_hash = if i == -1 { -2 } else { i };
                u64::from_ne_bytes(cpython_hash.to_ne_bytes())
            }
            Value::Bool(b) => u64::from_ne_bytes(i64::from(b).to_ne_bytes()),
            Value::Float(f) => u64::from_ne_bytes(Self::cpython_float_hash(f).to_ne_bytes()),
            Value::Ref(id) => match heap.get(id) {
                HeapData::Tuple(tuple) => Self::cpython_tuple_hash(tuple.as_vec(), heap, interns).unwrap_or(entry.hash),
                _ => entry.hash,
            },
            _ => entry.hash,
        }
    }

    /// Computes CPython-compatible hash for tuple values used in repr ordering.
    ///
    /// This uses the same xxHash-style tuple mixing algorithm as CPython so set
    /// slot simulation for tuple elements matches CPython scan order.
    fn cpython_tuple_hash(values: &[Value], heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<u64> {
        const MASK: u64 = u64::MAX;
        const XXPRIME_1: u64 = 11_400_714_785_074_694_791;
        const XXPRIME_2: u64 = 14_029_467_366_897_019_727;
        const XXPRIME_5: u64 = 2_870_177_450_012_600_261;
        const TUPLE_HASH_SUFFIX: u64 = 3_527_539;

        let mut acc = XXPRIME_5;
        for value in values {
            let lane_signed = Self::cpython_scalar_hash(value, heap, interns)?;
            let lane = u64::from_ne_bytes(lane_signed.to_ne_bytes());
            acc = acc.wrapping_add(lane.wrapping_mul(XXPRIME_2)) & MASK;
            acc = acc.rotate_left(31);
            acc = acc.wrapping_mul(XXPRIME_1) & MASK;
        }

        acc = acc.wrapping_add((values.len() as u64) ^ (XXPRIME_5 ^ TUPLE_HASH_SUFFIX)) & MASK;
        if acc == MASK {
            acc = 1_546_275_796;
        }
        Some(acc)
    }

    /// Computes CPython-compatible scalar hashes for repr tuple hashing.
    fn cpython_scalar_hash(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<i64> {
        match value {
            Value::Int(i) => Some(if *i == -1 { -2 } else { *i }),
            Value::Bool(v) => Some(i64::from(*v)),
            Value::Float(v) => Some(Self::cpython_float_hash(*v)),
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Tuple(tuple) => {
                    let hash = Self::cpython_tuple_hash(tuple.as_vec(), heap, interns)?;
                    Some(i64::from_ne_bytes(hash.to_ne_bytes()))
                }
                HeapData::Str(s) => Some(cpython_hash_bytes_seed0_i64(s.as_str().as_bytes())),
                _ => None,
            },
            Value::InternString(string_id) => Some(i64::from_ne_bytes(
                cpython_hash_str_seed0(interns.get_str(*string_id)).to_ne_bytes(),
            )),
            _ => None,
        }
    }

    /// Computes CPython-compatible float hash.
    ///
    /// CPython's float hash is not raw-bit hashing. Matching this matters for
    /// tuple hash parity because tuple hashing mixes element hash values.
    fn cpython_float_hash(value: f64) -> i64 {
        const PY_HASH_BITS: u32 = 61;
        const PY_HASH_MODULUS: u64 = (1u64 << PY_HASH_BITS) - 1;
        const PY_HASH_INF: i64 = 314_159;
        const CHUNK_MULTIPLIER: f64 = 268_435_456.0;
        const CHUNK_BITS: i32 = 28;

        if value.is_infinite() {
            return if value.is_sign_negative() {
                -PY_HASH_INF
            } else {
                PY_HASH_INF
            };
        }
        if value.is_nan() {
            return 0;
        }
        if value == 0.0 {
            return 0;
        }

        let sign = if value.is_sign_negative() { -1 } else { 1 };
        let mut abs_value = value.abs();
        let mut exponent: i32 = 0;
        while abs_value >= 1.0 {
            abs_value /= CHUNK_MULTIPLIER;
            exponent += CHUNK_BITS;
        }
        while abs_value > 0.0 && abs_value < (1.0 / CHUNK_MULTIPLIER) {
            abs_value *= CHUNK_MULTIPLIER;
            exponent -= CHUNK_BITS;
        }

        let mut hash: u64 = 0;
        let mut mantissa = abs_value;
        for _ in 0..12 {
            if mantissa == 0.0 {
                break;
            }
            mantissa *= CHUNK_MULTIPLIER;
            let chunk = mantissa as u64;
            mantissa -= chunk as f64;
            hash = hash.wrapping_add(chunk);
            if hash >= PY_HASH_MODULUS {
                hash -= PY_HASH_MODULUS;
            }
        }

        let exp = if exponent >= 0 {
            exponent as u32 % PY_HASH_BITS
        } else {
            PY_HASH_BITS - 1 - (((-1 - exponent) as u32) % PY_HASH_BITS)
        };

        hash = ((hash << exp) & PY_HASH_MODULUS) | (hash >> (PY_HASH_BITS - exp));
        let signed = (hash as i64) * sign;
        if signed == -1 { -2 } else { signed }
    }

    /// Returns a table size suitable for CPython-style set probing.
    ///
    /// CPython set growth depends on insertion history, not just final length:
    /// the table resizes when load exceeds ~3/5 and may jump 4x for small sets.
    /// We simulate the same insertion-driven growth so scan order matches repr.
    fn repr_table_size(entry_count: usize) -> usize {
        let mut table_size = 8usize;
        let mut used = 0usize;

        for _ in 0..entry_count {
            used = used.saturating_add(1);
            let mask = table_size - 1;
            if used.saturating_mul(5) >= mask.saturating_mul(3) {
                let min_used = if used > 50_000 {
                    used.saturating_mul(2)
                } else {
                    used.saturating_mul(4)
                };
                let mut new_size = 8usize;
                while new_size <= min_used {
                    new_size = new_size.saturating_mul(2);
                }
                table_size = new_size;
            }
        }
        table_size
    }

    /// Computes CPython table-scan order by simulating set table placement.
    ///
    /// CPython iterates sets/frozensets by scanning internal table slots in
    /// index order. We don't persist that table shape, so we rebuild a
    /// lightweight slot map from insertion-ordered entries and cached hashes
    /// using the same open-addressing probe sequence.
    fn repr_order_indices(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Vec<usize> {
        let entry_count = self.entries.len();
        if entry_count <= 1 {
            return (0..entry_count).collect();
        }

        let table_size = Self::repr_table_size(entry_count);
        let mask = table_size - 1;
        let mut slots: Vec<Option<usize>> = vec![None; table_size];

        for (entry_idx, entry) in self.entries.iter().enumerate() {
            let repr_hash = Self::repr_hash(entry, heap, interns);
            let mut i = (repr_hash as usize) & mask;
            let mut perturb = repr_hash as usize;

            'place: loop {
                // CPython probes a short contiguous window before perturb stepping.
                let mut probes = if i + Self::LINEAR_PROBES <= mask {
                    Self::LINEAR_PROBES
                } else {
                    0
                };
                let mut j = i;
                loop {
                    if slots[j].is_none() {
                        slots[j] = Some(entry_idx);
                        break 'place;
                    }
                    if probes == 0 {
                        break;
                    }
                    probes -= 1;
                    j += 1;
                }
                perturb >>= Self::PERTURB_SHIFT;
                i = i.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb) & mask;
            }
        }

        slots.into_iter().flatten().collect()
    }

    /// Creates a new empty set storage.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Creates a new set storage with pre-allocated capacity.
    fn with_capacity(capacity: usize) -> Self {
        Self {
            indices: HashTable::with_capacity(capacity),
            entries: Vec::with_capacity(capacity),
        }
    }

    /// Creates a SetStorage from a vector of (value, hash) pairs.
    ///
    /// This is used to avoid borrow conflicts when we need to copy another set's
    /// contents and then perform operations requiring mutable heap access.
    /// The caller is responsible for handling reference counting.
    pub(crate) fn from_entries(entries: Vec<(Value, u64)>) -> Self {
        let mut storage = Self::with_capacity(entries.len());
        for (idx, (value, hash)) in entries.into_iter().enumerate() {
            storage.entries.push(SetEntry { value, hash });
            storage.indices.insert_unique(hash, idx, |&i| storage.entries[i].hash);
        }
        storage
    }

    /// Reorders entries into CPython-style set scan order and rebuilds indices.
    ///
    /// Set algebra results are materialized from insertion-ordered storage, but
    /// CPython iteration order is derived from hash-table slot scan order. This
    /// method applies the same probe simulation used by repr so list()/iteration
    /// and repr remain aligned with CPython for derived set results.
    pub(crate) fn sort_by_hash(&mut self, heap: &Heap<impl ResourceTracker>, interns: &Interns) {
        let order = self.repr_order_indices(heap, interns);
        let mut old_entries: Vec<Option<SetEntry>> = std::mem::take(&mut self.entries).into_iter().map(Some).collect();
        self.entries = order
            .into_iter()
            .map(|entry_idx| {
                old_entries[entry_idx]
                    .take()
                    .expect("repr_order_indices must include each entry exactly once")
            })
            .collect();

        self.indices.clear();
        for (idx, entry) in self.entries.iter().enumerate() {
            self.indices
                .insert_unique(entry.hash, idx, |&entry_idx| self.entries[entry_idx].hash);
        }
    }

    /// Drops all values in this storage, decrementing their reference counts.
    pub(crate) fn drop_all_values(self, heap: &mut Heap<impl ResourceTracker>) {
        for entry in self.entries {
            entry.value.drop_with_heap(heap);
        }
    }

    /// Copies entries without incrementing reference counts.
    ///
    /// Used to break borrow conflicts: copy entries first, then after the
    /// borrow ends, call `inc_refs_for_entries` to fix up refcounts.
    pub(crate) fn copy_entries(&self) -> Vec<(Value, u64)> {
        self.entries
            .iter()
            .map(|e| (e.value.copy_for_extend(), e.hash))
            .collect()
    }

    /// Increments reference counts for all Ref values in an entries vector.
    ///
    /// Call this after `copy_entries` once the original borrow is released.
    pub(crate) fn inc_refs_for_entries(entries: &[(Value, u64)], heap: &mut Heap<impl ResourceTracker>) {
        for (v, _) in entries {
            if let Value::Ref(id) = v {
                heap.inc_ref(*id);
            }
        }
    }

    /// Returns the number of elements in the set.
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the set is empty.
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns whether this set contains any heap references (`Value::Ref`).
    ///
    /// Used during allocation to determine if this container could create cycles.
    #[inline]
    fn has_refs(&self) -> bool {
        self.entries.iter().any(|e| matches!(e.value, Value::Ref(_)))
    }

    /// Adds an element to the set, transferring ownership.
    ///
    /// Returns `Ok(true)` if the element was added (not already present),
    /// `Ok(false)` if the element was already in the set.
    /// Returns `Err` if the element is unhashable.
    ///
    /// The caller transfers ownership of `value`. If the value is already in
    /// the set, it will be dropped.
    pub(crate) fn add(
        &mut self,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        let Some(hash) = value.py_hash(heap, interns) else {
            let err = ExcType::type_error_unhashable_set_element(value.py_type(heap));
            value.drop_with_heap(heap);
            return Err(err);
        };

        // Check if value already exists
        let existing = self
            .indices
            .find(hash, |&idx| value.py_eq(&self.entries[idx].value, heap, interns));

        if existing.is_some() {
            // Value already in set, drop the new value
            value.drop_with_heap(heap);
            Ok(false)
        } else {
            // Add new entry
            let index = self.entries.len();
            self.entries.push(SetEntry { value, hash });
            self.indices.insert_unique(hash, index, |&idx| self.entries[idx].hash);
            Ok(true)
        }
    }

    /// Removes an element from the set.
    ///
    /// Returns `Ok(true)` if the element was removed, `Ok(false)` if not found.
    /// Returns `Err` if the key is unhashable.
    fn remove(&mut self, value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        let hash = value
            .py_hash(heap, interns)
            .ok_or_else(|| ExcType::type_error_unhashable_set_element(value.py_type(heap)))?;

        let entry = self.indices.entry(
            hash,
            |&idx| value.py_eq(&self.entries[idx].value, heap, interns),
            |&idx| self.entries[idx].hash,
        );

        if let hashbrown::hash_table::Entry::Occupied(occ) = entry {
            let index = *occ.get();
            let removed_entry = self.entries.remove(index);
            occ.remove();

            // Update indices for entries that shifted down
            for idx in &mut self.indices {
                if *idx > index {
                    *idx -= 1;
                }
            }

            // Drop the removed value
            removed_entry.value.drop_with_heap(heap);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Removes an element from the set without raising an error if not found.
    ///
    /// Returns `Ok(())` always (unless the key is unhashable).
    fn discard(&mut self, value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
        self.remove(value, heap, interns)?;
        Ok(())
    }

    /// Removes and returns an arbitrary element from the set.
    ///
    /// Returns `Err(KeyError)` if the set is empty.
    fn pop(&mut self) -> RunResult<Value> {
        if self.entries.is_empty() {
            return Err(ExcType::key_error_pop_empty_set());
        }

        // Remove the last entry (most efficient)
        let entry = self.entries.pop().expect("checked non-empty");

        // Remove from hash table
        self.indices
            .find_entry(entry.hash, |&idx| idx == self.entries.len())
            .expect("entry must exist")
            .remove();

        Ok(entry.value)
    }

    /// Removes all elements from the set.
    fn clear(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        for entry in self.entries.drain(..) {
            entry.value.drop_with_heap(heap);
        }
        self.indices.clear();
    }

    /// Creates a deep clone with proper reference counting.
    fn clone_with_heap(&self, heap: &mut Heap<impl ResourceTracker>) -> Self {
        Self {
            indices: self.indices.clone(),
            entries: self
                .entries
                .iter()
                .map(|entry| SetEntry {
                    value: entry.value.clone_with_heap(heap),
                    hash: entry.hash,
                })
                .collect(),
        }
    }

    /// Checks if the set contains a value.
    pub fn contains(&self, value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        let hash = value
            .py_hash(heap, interns)
            .ok_or_else(|| ExcType::type_error_unhashable_set_element(value.py_type(heap)))?;

        Ok(self
            .indices
            .find(hash, |&idx| value.py_eq(&self.entries[idx].value, heap, interns))
            .is_some())
    }

    /// Returns an iterator over the values in the set.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|e| &e.value)
    }

    /// Returns the value at the given index, if valid.
    ///
    /// Used by OurosIter for index-based iteration.
    pub(crate) fn value_at(&self, index: usize) -> Option<&Value> {
        self.entries.get(index).map(|e| &e.value)
    }

    /// Collects heap IDs for reference counting cleanup.
    fn collect_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        for entry in &mut self.entries {
            if let Value::Ref(id) = &entry.value {
                stack.push(*id);
                #[cfg(feature = "ref-count-panic")]
                entry.value.dec_ref_forget();
            }
        }
    }

    /// Compares two sets for equality.
    pub(crate) fn eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        if self.len() != other.len() {
            return false;
        }

        // Check that every element in self is in other
        for entry in &self.entries {
            match other.contains(&entry.value, heap, interns) {
                Ok(true) => {}
                _ => return false,
            }
        }
        true
    }

    /// Returns true if this set is a subset of other.
    fn is_subset(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        for entry in &self.entries {
            if !other.contains(&entry.value, heap, interns)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Returns true if this set is a superset of other.
    fn is_superset(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        other.is_subset(self, heap, interns)
    }

    /// Returns true if this set has no elements in common with other.
    fn is_disjoint(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        // Iterate over the smaller set for efficiency
        let (smaller, larger) = if self.len() <= other.len() {
            (self, other)
        } else {
            (other, self)
        };

        for entry in &smaller.entries {
            if larger.contains(&entry.value, heap, interns)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Returns a new set containing elements in either set (union).
    pub(crate) fn union(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let mut result = self.clone_with_heap(heap);
        for entry in &other.entries {
            let value = entry.value.clone_with_heap(heap);
            result.add(value, heap, interns)?;
        }
        Ok(result)
    }

    /// Returns a new set containing elements in both sets (intersection).
    pub(crate) fn intersection(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let mut result = Self::new();
        // Iterate over the smaller set for efficiency
        let (smaller, larger) = if self.len() <= other.len() {
            (self, other)
        } else {
            (other, self)
        };

        for entry in &smaller.entries {
            if larger.contains(&entry.value, heap, interns)? {
                let value = entry.value.clone_with_heap(heap);
                result.add(value, heap, interns)?;
            }
        }
        Ok(result)
    }

    /// Returns a new set containing elements in self but not in other (difference).
    pub(crate) fn difference(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let mut result = Self::new();
        for entry in &self.entries {
            if !other.contains(&entry.value, heap, interns)? {
                let value = entry.value.clone_with_heap(heap);
                result.add(value, heap, interns)?;
            }
        }
        Ok(result)
    }

    /// Returns a new set containing elements in either set but not both (symmetric difference).
    pub(crate) fn symmetric_difference(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let mut result = Self::new();

        // Add elements in self but not in other
        for entry in &self.entries {
            if !other.contains(&entry.value, heap, interns)? {
                let value = entry.value.clone_with_heap(heap);
                result.add(value, heap, interns)?;
            }
        }

        // Add elements in other but not in self
        for entry in &other.entries {
            if !self.contains(&entry.value, heap, interns)? {
                let value = entry.value.clone_with_heap(heap);
                result.add(value, heap, interns)?;
            }
        }

        Ok(result)
    }

    /// Adds all elements from other to this set (in-place union).
    fn update(&mut self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
        for entry in &other.entries {
            let value = entry.value.clone_with_heap(heap);
            self.add(value, heap, interns)?;
        }
        Ok(())
    }

    /// Writes the repr format to a formatter.
    ///
    /// For sets, outputs `{elem1, elem2, ...}` (no type prefix).
    /// For frozensets, outputs `frozenset({elem1, elem2, ...})`.
    fn repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
        type_name: &str,
    ) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "{type_name}()");
        }

        // frozenset needs type prefix: frozenset({...}), but set doesn't: {...}
        let needs_prefix = type_name != "set";
        if needs_prefix {
            write!(f, "{type_name}(")?;
        }

        f.write_char('{')?;
        for (idx, entry_idx) in self.repr_order_indices(heap, interns).into_iter().enumerate() {
            if idx > 0 {
                f.write_str(", ")?;
            }
            let entry = &self.entries[entry_idx];
            entry.value.py_repr_fmt(f, heap, heap_ids, interns)?;
        }
        f.write_char('}')?;

        if needs_prefix {
            f.write_char(')')?;
        }
        Ok(())
    }

    /// Estimates the memory size of this storage.
    fn estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.len() * std::mem::size_of::<SetEntry>()
    }
}

/// Python set type - mutable, unordered collection of unique hashable elements.
///
/// Sets support standard operations like add, remove, discard, pop, clear, as well
/// as set algebra operations like union, intersection, difference, and symmetric
/// difference.
///
/// # Reference Counting
/// When values are added, their reference counts are NOT incremented by the set -
/// the caller transfers ownership. When values are removed or the set is cleared,
/// their reference counts are decremented.
#[derive(Debug, Default)]
pub(crate) struct Set(SetStorage);

impl Set {
    /// Creates a new empty set.
    #[must_use]
    pub fn new() -> Self {
        Self(SetStorage::new())
    }

    /// Creates a set with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(SetStorage::with_capacity(capacity))
    }

    /// Creates a set from existing storage.
    #[must_use]
    pub(crate) fn from_storage(storage: SetStorage) -> Self {
        Self(storage)
    }

    /// Returns the number of elements in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns whether this set contains any heap references (`Value::Ref`).
    ///
    /// Used during allocation to determine if this container could create cycles.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        self.0.has_refs()
    }

    /// Adds an element to the set, transferring ownership.
    ///
    /// Returns `Ok(true)` if added, `Ok(false)` if already present.
    pub fn add(&mut self, value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        self.0.add(value, heap, interns)
    }

    /// Removes an element from the set.
    ///
    /// Returns `Err(KeyError)` if the element is not present.
    pub fn remove(&mut self, value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
        if self.0.remove(value, heap, interns)? {
            Ok(())
        } else {
            Err(ExcType::key_error(value, heap, interns))
        }
    }

    /// Removes an element from the set if present.
    ///
    /// Does not raise an error if the element is not found.
    pub fn discard(
        &mut self,
        value: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        self.0.discard(value, heap, interns)
    }

    /// Removes and returns an arbitrary element from the set.
    ///
    /// Returns `Err(KeyError)` if the set is empty.
    pub fn pop(&mut self) -> RunResult<Value> {
        self.0.pop()
    }

    /// Removes all elements from the set.
    pub fn clear(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        self.0.clear(heap);
    }

    /// Returns a shallow copy of the set.
    #[must_use]
    pub fn copy(&self, heap: &mut Heap<impl ResourceTracker>) -> Self {
        Self(self.0.clone_with_heap(heap))
    }

    /// Checks if the set contains a value.
    pub fn contains(&self, value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        self.0.contains(value, heap, interns)
    }

    /// Returns the internal storage (for set operations between Set and FrozenSet).
    pub(crate) fn storage(&self) -> &SetStorage {
        &self.0
    }

    /// Creates a set from the `set()` constructor call.
    ///
    /// - `set()` with no args returns an empty set
    /// - `set(iterable)` creates a set from any iterable (list, tuple, set, dict, range, str, bytes)
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let value = args.get_zero_one_arg("set", heap)?;
        let set = match value {
            None => Self::new(),
            Some(v) => Self::from_iterable(v, heap, interns)?,
        };
        let heap_id = heap.allocate(HeapData::Set(set))?;
        Ok(Value::Ref(heap_id))
    }

    /// Creates a set from a OurosIter, adding elements one by one.
    ///
    /// Unlike list/tuple which can just collect into a Vec, sets need to add
    /// each element individually to handle duplicates and compute hashes.
    fn from_iterator(mut iter: OurosIter, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut set = Self::with_capacity(iter.size_hint(heap));
        while let Some(item) = iter.for_next(heap, interns)? {
            set.add(item, heap, interns)?;
        }
        iter.drop_with_heap(heap);
        Ok(set)
    }

    /// Creates a set from an iterable value.
    ///
    /// This is a convenience method used by helper methods that need to convert
    /// arbitrary iterables to sets. It uses `OurosIter` internally.
    fn from_iterable(iterable: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let iter = OurosIter::new(iterable, heap, interns)?;
        let set = Self::from_iterator(iter, heap, interns)?;
        Ok(set)
    }
}

impl PyTrait for Set {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Set
    }

    fn py_estimate_size(&self) -> usize {
        self.0.estimate_size()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        self.0.eq(&other.0, heap, interns)
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.0.collect_dec_ref_ids(stack);
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
        self.0.repr_fmt(f, heap, heap_ids, interns, "set")
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let Some(method) = attr.static_string() else {
            return Err(ExcType::attribute_error(Type::Set, attr.as_str(interns)));
        };

        match method {
            StaticStrings::Add => {
                let value = args.get_one_arg("set.add", heap)?;
                self.add(value, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::Remove => {
                let value = args.get_one_arg("set.remove", heap)?;
                let result = self.remove(&value, heap, interns);
                value.drop_with_heap(heap);
                result?;
                Ok(Value::None)
            }
            StaticStrings::Discard => {
                let value = args.get_one_arg("set.discard", heap)?;
                let result = self.discard(&value, heap, interns);
                value.drop_with_heap(heap);
                result?;
                Ok(Value::None)
            }
            StaticStrings::Pop => {
                args.check_zero_args("set.pop", heap)?;
                self.pop()
            }
            StaticStrings::Clear => {
                args.check_zero_args("set.clear", heap)?;
                self.clear(heap);
                Ok(Value::None)
            }
            StaticStrings::Copy => {
                args.check_zero_args("set.copy", heap)?;
                let copy = self.copy(heap);
                let heap_id = heap.allocate(HeapData::Set(copy))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Update => {
                self.update_from_args(args, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::IntersectionUpdate => {
                self.intersection_update_from_args(args, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::DifferenceUpdate => {
                self.difference_update_from_args(args, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::SymmetricDifferenceUpdate => {
                let other = args.get_one_arg("set.symmetric_difference_update", heap)?;
                self.symmetric_difference_update_from_value(other, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::Union => {
                let (positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    kwargs.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs("set.union"));
                }

                // Start with a copy of this set, then union with each argument
                let mut result = self.copy(heap);
                for other in positional {
                    result = result.union_from_value(other, heap, interns)?;
                }
                let heap_id = heap.allocate(HeapData::Set(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Intersection => {
                let (positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    kwargs.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs("set.intersection"));
                }

                // Start with a copy of this set, then intersect with each argument
                let mut result = self.copy(heap);
                for other in positional {
                    let updated = result.intersection_from_value(other, heap, interns)?;
                    result.0 = updated.0;
                }
                let heap_id = heap.allocate(HeapData::Set(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Difference => {
                let (positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    kwargs.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs("set.difference"));
                }

                // Start with a copy of this set, then difference with each argument
                let mut result = self.copy(heap);
                for other in positional {
                    let updated = result.difference_from_value(other, heap, interns)?;
                    result.0 = updated.0;
                }
                let heap_id = heap.allocate(HeapData::Set(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::SymmetricDifference => {
                let other = args.get_one_arg("set.symmetric_difference", heap)?;
                let result = self.symmetric_difference_from_value(other, heap, interns)?;
                let heap_id = heap.allocate(HeapData::Set(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Issubset => {
                let other = args.get_one_arg("set.issubset", heap)?;
                let result = self.issubset_from_value(&other, heap, interns);
                other.drop_with_heap(heap);
                Ok(Value::Bool(result?))
            }
            StaticStrings::Issuperset => {
                let other = args.get_one_arg("set.issuperset", heap)?;
                let result = self.issuperset_from_value(&other, heap, interns);
                other.drop_with_heap(heap);
                Ok(Value::Bool(result?))
            }
            StaticStrings::Isdisjoint => {
                let other = args.get_one_arg("set.isdisjoint", heap)?;
                let result = self.isdisjoint_from_value(&other, heap, interns);
                other.drop_with_heap(heap);
                Ok(Value::Bool(result?))
            }
            _ => Err(ExcType::attribute_error(Type::Set, attr.as_str(interns))),
        }
    }

    fn py_sub(
        &self,
        _other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        // This is called from heap.rs with two Sets
        // We need interns for contains check, but py_sub doesn't have it
        // This is a limitation - we'll need to handle this differently
        // For now, return None to indicate not supported via this path
        Ok(None)
    }
}

/// Helper methods for set operations with arbitrary iterables.
impl Set {
    /// Updates this set with all values from positional arguments.
    ///
    /// Mirrors `set.update(*others)` and accepts zero or more iterables.
    fn update_from_args(
        &mut self,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let (positional, kwargs) = args.into_parts();
        if !kwargs.is_empty() {
            kwargs.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            return Err(ExcType::type_error_no_kwargs("set.update"));
        }

        for other in positional {
            self.update_from_value(other, heap, interns)?;
        }
        Ok(())
    }

    /// Intersects this set in-place with all values from positional arguments.
    ///
    /// Mirrors `set.intersection_update(*others)` and accepts zero or more iterables.
    fn intersection_update_from_args(
        &mut self,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let (positional, kwargs) = args.into_parts();
        if !kwargs.is_empty() {
            kwargs.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            return Err(ExcType::type_error_no_kwargs("set.intersection_update"));
        }

        for other in positional {
            let updated = self.intersection_from_value(other, heap, interns)?;
            let old = std::mem::replace(&mut self.0, updated.0);
            old.drop_all_values(heap);
        }
        Ok(())
    }

    /// Removes values present in any positional argument from this set.
    ///
    /// Mirrors `set.difference_update(*others)` and accepts zero or more iterables.
    fn difference_update_from_args(
        &mut self,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let (positional, kwargs) = args.into_parts();
        if !kwargs.is_empty() {
            kwargs.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            return Err(ExcType::type_error_no_kwargs("set.difference_update"));
        }

        for other in positional {
            let updated = self.difference_from_value(other, heap, interns)?;
            let old = std::mem::replace(&mut self.0, updated.0);
            old.drop_all_values(heap);
        }
        Ok(())
    }

    /// Replaces this set with its symmetric difference against `other`.
    ///
    /// Mirrors `set.symmetric_difference_update(other)`.
    fn symmetric_difference_update_from_value(
        &mut self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let updated = self.symmetric_difference_from_value(other, heap, interns)?;
        let old = std::mem::replace(&mut self.0, updated.0);
        old.drop_all_values(heap);
        Ok(())
    }

    /// Updates this set with elements from an iterable value.
    fn update_from_value(
        &mut self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match &other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, now we can use heap mutably
            // IMPORTANT: Inc refs BEFORE dropping the source to avoid use-after-free
            SetStorage::inc_refs_for_entries(&entries, heap);
            other.drop_with_heap(heap);
            for (value, _hash) in entries {
                self.add(value, heap, interns)?;
            }
            return Ok(());
        }

        // Fall back to creating a temporary set from the iterable
        let temp_set = Self::from_iterable(other, heap, interns)?;
        self.0.update(&temp_set.0, heap, interns)?;
        temp_set.0.drop_all_values(heap);
        Ok(())
    }

    /// Returns a new set with elements from both this set and an iterable.
    fn union_from_value(
        &self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let other_storage = Self::get_storage_from_value(other, heap, interns)?;
        let result_storage = self.0.union(&other_storage, heap, interns)?;
        // Clean up other_storage if it was created from a non-set
        for entry in other_storage.entries {
            entry.value.drop_with_heap(heap);
        }
        Ok(Self(result_storage))
    }

    /// Returns a new set with elements common to both this set and an iterable.
    fn intersection_from_value(
        &self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let other_storage = Self::get_storage_from_value(other, heap, interns)?;
        let result_storage = self.0.intersection(&other_storage, heap, interns)?;
        for entry in other_storage.entries {
            entry.value.drop_with_heap(heap);
        }
        Ok(Self(result_storage))
    }

    /// Returns a new set with elements in this set but not in an iterable.
    fn difference_from_value(
        &self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let other_storage = Self::get_storage_from_value(other, heap, interns)?;
        let result_storage = self.0.difference(&other_storage, heap, interns)?;
        for entry in other_storage.entries {
            entry.value.drop_with_heap(heap);
        }
        Ok(Self(result_storage))
    }

    /// Returns a new set with elements in either set but not both.
    fn symmetric_difference_from_value(
        &self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let other_storage = Self::get_storage_from_value(other, heap, interns)?;
        let result_storage = self.0.symmetric_difference(&other_storage, heap, interns)?;
        for entry in other_storage.entries {
            entry.value.drop_with_heap(heap);
        }
        Ok(Self(result_storage))
    }

    /// Checks if this set is a subset of an iterable.
    fn issubset_from_value(
        &self,
        other: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build temporary storage and check
            SetStorage::inc_refs_for_entries(&entries, heap);
            let other_storage = SetStorage::from_entries(entries);
            let result = self.0.is_subset(&other_storage, heap, interns);
            other_storage.drop_all_values(heap);
            return result;
        }

        // Handle all other iterables (list, tuple, range, str, bytes, dict, etc.)
        let temp = Self::from_iterable(other.clone_with_heap(heap), heap, interns)?;
        let result = self.0.is_subset(&temp.0, heap, interns);
        temp.0.drop_all_values(heap);
        result
    }

    /// Checks if this set is a superset of an iterable.
    fn issuperset_from_value(
        &self,
        other: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build temporary storage and check
            SetStorage::inc_refs_for_entries(&entries, heap);
            let other_storage = SetStorage::from_entries(entries);
            let result = self.0.is_superset(&other_storage, heap, interns);
            other_storage.drop_all_values(heap);
            return result;
        }

        // Handle all other iterables (list, tuple, range, str, bytes, dict, etc.)
        let temp = Self::from_iterable(other.clone_with_heap(heap), heap, interns)?;
        let result = self.0.is_superset(&temp.0, heap, interns);
        temp.0.drop_all_values(heap);
        result
    }

    /// Checks if this set has no elements in common with an iterable.
    fn isdisjoint_from_value(
        &self,
        other: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build temporary storage and check
            SetStorage::inc_refs_for_entries(&entries, heap);
            let other_storage = SetStorage::from_entries(entries);
            let result = self.0.is_disjoint(&other_storage, heap, interns);
            other_storage.drop_all_values(heap);
            return result;
        }

        // Handle all other iterables (list, tuple, range, str, bytes, dict, etc.)
        let temp = Self::from_iterable(other.clone_with_heap(heap), heap, interns)?;
        let result = self.0.is_disjoint(&temp.0, heap, interns);
        temp.0.drop_all_values(heap);
        result
    }

    /// Helper to get SetStorage from a Value (either directly or by conversion).
    fn get_storage_from_value(
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SetStorage> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match &value {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(set) => Some(set.0.copy_entries()),
                HeapData::FrozenSet(set) => Some(set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build storage with proper refcounts
            // IMPORTANT: Inc refs BEFORE dropping the source to avoid use-after-free
            SetStorage::inc_refs_for_entries(&entries, heap);
            value.drop_with_heap(heap);
            return Ok(SetStorage::from_entries(entries));
        }

        // Convert iterable to set
        let temp_set = Self::from_iterable(value, heap, interns)?;
        Ok(temp_set.0)
    }
}

/// Python frozenset type - immutable, unordered collection of unique hashable elements.
///
/// FrozenSets support the same set algebra operations as sets (union, intersection,
/// difference, symmetric difference) but are immutable and therefore hashable.
///
/// # Hashability
/// Unlike mutable sets, frozensets can be used as dict keys or set elements because
/// they are immutable. The hash is computed as the XOR of element hashes (order-independent).
#[derive(Debug, Default)]
pub(crate) struct FrozenSet(SetStorage);

impl FrozenSet {
    /// Creates a new empty frozenset.
    #[must_use]
    pub fn new() -> Self {
        Self(SetStorage::new())
    }

    /// Creates a frozenset from existing storage.
    #[must_use]
    pub(crate) fn from_storage(storage: SetStorage) -> Self {
        Self(storage)
    }

    /// Returns the number of elements in the frozenset.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the frozenset is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns whether this frozenset contains any heap references (`Value::Ref`).
    ///
    /// Used during allocation to determine if this container could create cycles.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        self.0.has_refs()
    }

    /// Returns a shallow copy of the frozenset.
    #[must_use]
    pub fn copy(&self, heap: &mut Heap<impl ResourceTracker>) -> Self {
        Self(self.0.clone_with_heap(heap))
    }

    /// Returns the internal storage.
    pub(crate) fn storage(&self) -> &SetStorage {
        &self.0
    }

    /// Checks if the frozenset contains a value.
    pub fn contains(&self, value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
        self.0.contains(value, heap, interns)
    }

    /// Computes the hash of this frozenset.
    ///
    /// The hash is the XOR of all element hashes, making it order-independent.
    pub fn compute_hash(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<u64> {
        let mut hash: u64 = 0;
        for entry in &self.0.entries {
            // All elements must be hashable (enforced at construction)
            let elem_hash = entry.value.py_hash(heap, interns)?;
            hash ^= elem_hash;
        }
        Some(hash)
    }

    /// Creates a frozenset from a Set, consuming the Set's storage.
    ///
    /// This is used when we need to convert a mutable set to an immutable frozenset
    /// without cloning.
    pub fn from_set(set: Set) -> Self {
        Self(set.0)
    }

    /// Creates a frozenset from the `frozenset()` constructor call.
    ///
    /// - `frozenset()` with no args returns an empty frozenset
    /// - `frozenset(iterable)` creates a frozenset from any iterable (list, tuple, set, dict, range, str, bytes)
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let value = args.get_zero_one_arg("frozenset", heap)?;
        let frozenset = match value {
            None => Self::new(),
            Some(v) => Self::from_set(Set::from_iterable(v, heap, interns)?),
        };
        let heap_id = heap.allocate(HeapData::FrozenSet(frozenset))?;
        Ok(Value::Ref(heap_id))
    }

    /// Returns a new frozenset with elements from both this and another set.
    pub(crate) fn union(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        Ok(Self(self.0.union(other, heap, interns)?))
    }

    /// Returns a new frozenset with elements common to both sets.
    pub(crate) fn intersection(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        Ok(Self(self.0.intersection(other, heap, interns)?))
    }

    /// Returns a new frozenset with elements in this set but not in other.
    pub(crate) fn difference(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        Ok(Self(self.0.difference(other, heap, interns)?))
    }

    /// Returns a new frozenset with elements in either set but not both.
    pub(crate) fn symmetric_difference(
        &self,
        other: &SetStorage,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        Ok(Self(self.0.symmetric_difference(other, heap, interns)?))
    }
}

impl PyTrait for FrozenSet {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::FrozenSet
    }

    fn py_estimate_size(&self) -> usize {
        self.0.estimate_size()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        self.0.eq(&other.0, heap, interns)
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.0.collect_dec_ref_ids(stack);
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
        self.0.repr_fmt(f, heap, heap_ids, interns, "frozenset")
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let Some(method) = attr.static_string() else {
            return Err(ExcType::attribute_error(Type::FrozenSet, attr.as_str(interns)));
        };

        match method {
            StaticStrings::Copy => {
                args.check_zero_args("frozenset.copy", heap)?;
                let copy = self.copy(heap);
                let heap_id = heap.allocate(HeapData::FrozenSet(copy))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Union => {
                let (positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    kwargs.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs("frozenset.union"));
                }

                // Start with a copy of this frozenset, then union with each argument
                let mut result = self.copy(heap);
                for other in positional {
                    let other_storage = Set::get_storage_from_value(other, heap, interns)?;
                    let new_result = result.union(&other_storage, heap, interns)?;
                    for entry in other_storage.entries {
                        entry.value.drop_with_heap(heap);
                    }
                    result = new_result;
                }
                let heap_id = heap.allocate(HeapData::FrozenSet(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Intersection => {
                let (positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    kwargs.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs("frozenset.intersection"));
                }

                // Start with a copy of this frozenset, then intersect with each argument
                let mut result = self.copy(heap);
                for other in positional {
                    let other_storage = Set::get_storage_from_value(other, heap, interns)?;
                    let new_result = result.intersection(&other_storage, heap, interns)?;
                    for entry in other_storage.entries {
                        entry.value.drop_with_heap(heap);
                    }
                    result.0 = new_result.0;
                }
                let heap_id = heap.allocate(HeapData::FrozenSet(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Difference => {
                let (positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    kwargs.drop_with_heap(heap);
                    positional.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs("frozenset.difference"));
                }

                // Start with a copy of this frozenset, then difference with each argument
                let mut result = self.copy(heap);
                for other in positional {
                    let other_storage = Set::get_storage_from_value(other, heap, interns)?;
                    let new_result = result.difference(&other_storage, heap, interns)?;
                    for entry in other_storage.entries {
                        entry.value.drop_with_heap(heap);
                    }
                    result.0 = new_result.0;
                }
                let heap_id = heap.allocate(HeapData::FrozenSet(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::SymmetricDifference => {
                let other = args.get_one_arg("frozenset.symmetric_difference", heap)?;
                let other_storage = Set::get_storage_from_value(other, heap, interns)?;
                let result = self.symmetric_difference(&other_storage, heap, interns)?;
                for entry in other_storage.entries {
                    entry.value.drop_with_heap(heap);
                }
                let heap_id = heap.allocate(HeapData::FrozenSet(result))?;
                Ok(Value::Ref(heap_id))
            }
            StaticStrings::Issubset => {
                let other = args.get_one_arg("frozenset.issubset", heap)?;
                let result = self.issubset_from_value(&other, heap, interns);
                other.drop_with_heap(heap);
                Ok(Value::Bool(result?))
            }
            StaticStrings::Issuperset => {
                let other = args.get_one_arg("frozenset.issuperset", heap)?;
                let result = self.issuperset_from_value(&other, heap, interns);
                other.drop_with_heap(heap);
                Ok(Value::Bool(result?))
            }
            StaticStrings::Isdisjoint => {
                let other = args.get_one_arg("frozenset.isdisjoint", heap)?;
                let result = self.isdisjoint_from_value(&other, heap, interns);
                other.drop_with_heap(heap);
                Ok(Value::Bool(result?))
            }
            _ => Err(ExcType::attribute_error(Type::FrozenSet, attr.as_str(interns))),
        }
    }

    fn py_sub(
        &self,
        _other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        // Same limitation as Set - needs interns
        Ok(None)
    }
}

/// Helper methods for frozenset operations with arbitrary iterables.
impl FrozenSet {
    /// Checks if this frozenset is a subset of an iterable.
    fn issubset_from_value(
        &self,
        other: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build temporary storage and check
            SetStorage::inc_refs_for_entries(&entries, heap);
            let other_storage = SetStorage::from_entries(entries);
            let result = self.0.is_subset(&other_storage, heap, interns);
            other_storage.drop_all_values(heap);
            return result;
        }

        // Handle all other iterables (list, tuple, range, str, bytes, dict, etc.)
        let temp = Set::from_iterable(other.clone_with_heap(heap), heap, interns)?;
        let result = self.0.is_subset(&temp.0, heap, interns);
        temp.0.drop_all_values(heap);
        result
    }

    /// Checks if this frozenset is a superset of an iterable.
    fn issuperset_from_value(
        &self,
        other: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build temporary storage and check
            SetStorage::inc_refs_for_entries(&entries, heap);
            let other_storage = SetStorage::from_entries(entries);
            let result = self.0.is_superset(&other_storage, heap, interns);
            other_storage.drop_all_values(heap);
            return result;
        }

        // Handle all other iterables (list, tuple, range, str, bytes, dict, etc.)
        let temp = Set::from_iterable(other.clone_with_heap(heap), heap, interns)?;
        let result = self.0.is_superset(&temp.0, heap, interns);
        temp.0.drop_all_values(heap);
        result
    }

    /// Checks if this frozenset has no elements in common with an iterable.
    fn isdisjoint_from_value(
        &self,
        other: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        // Try to get entries from a Set/FrozenSet directly
        let entries_opt = match other {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Set(other_set) => Some(other_set.0.copy_entries()),
                HeapData::FrozenSet(other_set) => Some(other_set.0.copy_entries()),
                _ => None,
            },
            _ => None,
        };

        if let Some(entries) = entries_opt {
            // Borrow released, build temporary storage and check
            SetStorage::inc_refs_for_entries(&entries, heap);
            let other_storage = SetStorage::from_entries(entries);
            let result = self.0.is_disjoint(&other_storage, heap, interns);
            other_storage.drop_all_values(heap);
            return result;
        }

        // Handle all other iterables (list, tuple, range, str, bytes, dict, etc.)
        let temp = Set::from_iterable(other.clone_with_heap(heap), heap, interns)?;
        let result = self.0.is_disjoint(&temp.0, heap, interns);
        temp.0.drop_all_values(heap);
        result
    }
}

// Custom serde implementations for SetStorage, Set, and FrozenSet.
// Only serialize entries; rebuild the indices hash table on deserialize.

impl serde::Serialize for SetStorage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.entries.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SetStorage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries: Vec<SetEntry> = serde::Deserialize::deserialize(deserializer)?;
        // Rebuild the indices hash table from the entries
        let mut indices = HashTable::with_capacity(entries.len());
        for (idx, entry) in entries.iter().enumerate() {
            indices.insert_unique(entry.hash, idx, |&i| entries[i].hash);
        }
        Ok(Self { indices, entries })
    }
}

impl serde::Serialize for Set {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Set {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(SetStorage::deserialize(deserializer)?))
    }
}

impl serde::Serialize for FrozenSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for FrozenSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(SetStorage::deserialize(deserializer)?))
    }
}
