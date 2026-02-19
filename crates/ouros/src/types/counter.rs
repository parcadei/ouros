//! Implementation of `collections.Counter`.
//!
//! The Counter type stores element counts in a dict-like structure and provides
//! Counter-specific methods like `most_common`, `elements`, `update`, `subtract`,
//! `total`, and arithmetic operations.
//!
//! This is a lightweight wrapper around `Dict` that preserves Python semantics:
//! - Missing keys return 0
//! - `update` adds counts
//! - `subtract` subtracts counts (allowing negatives)
//! - Arithmetic operations return new Counters with only positive counts

use std::fmt::Write;

use ahash::AHashSet;
use smallvec::SmallVec;

use crate::{
    args::ArgValues,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunError, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::{ResourceError, ResourceTracker},
    types::{Dict, List, OurosIter, PyTrait, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// A Counter mapping hashable keys to integer counts.
///
/// Counter behaves like a dict but returns 0 for missing keys and provides
/// Counter-specific aggregation helpers for element counting.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Counter {
    /// Underlying dict storing counts as `Value::Int` values.
    dict: Dict,
}

impl Counter {
    /// Creates an empty Counter.
    #[must_use]
    pub fn new() -> Self {
        Self { dict: Dict::new() }
    }

    /// Creates a Counter from an existing dict of counts.
    #[must_use]
    pub fn from_dict(dict: Dict) -> Self {
        Self { dict }
    }

    /// Returns a reference to the underlying dict.
    #[must_use]
    pub fn dict(&self) -> &Dict {
        &self.dict
    }

    /// Returns a mutable reference to the underlying dict.
    #[must_use]
    pub fn dict_mut(&mut self) -> &mut Dict {
        &mut self.dict
    }

    /// Returns the number of stored entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.dict.len()
    }

    /// Returns true if the counter contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dict.is_empty()
    }

    /// Returns the count for a key, defaulting to 0 if missing or non-integer.
    fn get_count(&self, key: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<i64> {
        match self.dict.get(key, heap, interns)? {
            Some(Value::Int(n)) => Ok(*n),
            Some(_) | None => Ok(0),
        }
    }

    /// Sets the count for a key, replacing any existing value.
    fn set_count(
        &mut self,
        key: Value,
        count: i64,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        if let Some(old_value) = self.dict.set(key, Value::Int(count), heap, interns)? {
            old_value.drop_with_heap(heap);
        }
        Ok(())
    }

    /// Adds a delta to the existing count for a key.
    fn add_count(
        &mut self,
        key: Value,
        delta: i64,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let current = self.get_count(&key, heap, interns)?;
        self.set_count(key, current + delta, heap, interns)
    }

    /// Collects `(key, count)` pairs from a Counter or mapping.
    #[expect(clippy::unnecessary_wraps, reason = "consistent API with counts_from_iterable")]
    fn counts_from_mapping(
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<Vec<(Value, i64)>>> {
        let Value::Ref(id) = &value else {
            return Ok(None);
        };

        let counts = heap.with_entry_mut(*id, |heap_inner, data| match data {
            HeapData::Counter(counter) => counter
                .dict
                .items(heap_inner)
                .into_iter()
                .map(|(k, v)| {
                    let count = if let Value::Int(n) = &v { *n } else { 0 };
                    v.drop_with_heap(heap_inner);
                    (k, count)
                })
                .collect(),
            HeapData::Dict(dict) => dict
                .items(heap_inner)
                .into_iter()
                .map(|(k, v)| {
                    let count = if let Value::Int(n) = &v { *n } else { 0 };
                    v.drop_with_heap(heap_inner);
                    (k, count)
                })
                .collect(),
            HeapData::OrderedDict(od) => od
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

        let result = if counts.is_empty() { None } else { Some(counts) };
        value.drop_with_heap(heap);
        Ok(result)
    }

    /// Collects counts by iterating over an iterable and counting occurrences.
    fn counts_from_iterable(
        iterable: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Vec<(Value, i64)>> {
        let mut dict = Dict::new();
        let iter = OurosIter::new(iterable, heap, interns)?;
        defer_drop_mut!(iter, heap);

        loop {
            match iter.for_next(heap, interns) {
                Ok(Some(item)) => {
                    let current = match dict.get(&item, heap, interns) {
                        Ok(Some(Value::Int(n))) => *n,
                        Ok(_) => 0,
                        Err(e) => {
                            item.drop_with_heap(heap);
                            dict.drop_all_entries(heap);
                            return Err(e);
                        }
                    };

                    let key = item.clone_with_heap(heap);
                    if let Err(e) = dict.set(key, Value::Int(current + 1), heap, interns) {
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

        let result = dict
            .items(heap)
            .into_iter()
            .map(|(k, v)| {
                let count = if let Value::Int(n) = &v { *n } else { 0 };
                v.drop_with_heap(heap);
                (k, count)
            })
            .collect();

        dict.drop_all_entries(heap);
        Ok(result)
    }

    /// Extracts counts from a mapping or iterable value.
    fn counts_from_value(
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Vec<(Value, i64)>> {
        defer_drop!(value, heap);
        if let Some(counts) = Self::counts_from_mapping(value.clone_with_heap(heap), heap, interns)? {
            return Ok(counts);
        }
        Self::counts_from_iterable(value.clone_with_heap(heap), heap, interns)
    }

    /// Updates this Counter by adding counts from another value.
    fn update_from_value(
        &mut self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let counts = Self::counts_from_value(other, heap, interns)?;
        for (key, count) in counts {
            self.add_count(key, count, heap, interns)?;
        }
        Ok(())
    }

    /// Updates this Counter by subtracting counts from another value.
    fn subtract_from_value(
        &mut self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let counts = Self::counts_from_value(other, heap, interns)?;
        for (key, count) in counts {
            self.add_count(key, -count, heap, interns)?;
        }
        Ok(())
    }

    /// Returns counts as a vector of `(key, count)` pairs.
    fn iter_counts(&self, heap: &mut Heap<impl ResourceTracker>) -> Vec<(Value, i64)> {
        self.dict
            .items(heap)
            .into_iter()
            .map(|(k, v)| {
                let count = if let Value::Int(n) = &v { *n } else { 0 };
                v.drop_with_heap(heap);
                (k, count)
            })
            .collect()
    }

    /// Builds a Counter from counts, optionally filtering non-positive counts.
    fn from_counts(
        counts: Vec<(Value, i64)>,
        keep_non_positive: bool,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let mut dict = Dict::new();
        for (key, count) in counts {
            if keep_non_positive || count > 0 {
                if let Some(old) = dict.set(key, Value::Int(count), heap, interns)? {
                    old.drop_with_heap(heap);
                }
            } else {
                key.drop_with_heap(heap);
            }
        }
        Ok(Self::from_dict(dict))
    }

    /// Returns a list of (element, count) tuples sorted by count descending.
    fn most_common_list(&self, n: Option<i64>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let mut items = self.iter_counts(heap);
        items.sort_by_key(|b| std::cmp::Reverse(b.1));

        if let Some(n) = n {
            if n >= 0 {
                #[expect(clippy::cast_sign_loss, reason = "already validated n >= 0")]
                #[expect(clippy::cast_possible_truncation, reason = "truncating to usize for slice operation")]
                items.truncate(n as usize);
            } else {
                items.clear();
            }
        }

        let mut result_items = Vec::with_capacity(items.len());
        for (key, count) in items {
            let tuple_items: SmallVec<[Value; 3]> = SmallVec::from_vec(vec![key, Value::Int(count)]);
            let tuple_val = allocate_tuple(tuple_items, heap)?;
            result_items.push(tuple_val);
        }

        let list_id = heap.allocate(HeapData::List(List::new(result_items)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns a list of elements repeated by their positive counts.
    fn elements_list(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let items = self.iter_counts(heap);
        let mut result = Vec::new();
        for (key, count) in items {
            if count > 0 {
                #[expect(clippy::cast_sign_loss, reason = "already validated count > 0")]
                #[expect(clippy::cast_possible_truncation, reason = "truncating to usize for loop iteration")]
                for _ in 0..count as usize {
                    result.push(key.clone_with_heap(heap));
                }
            }
            key.drop_with_heap(heap);
        }

        let list_id = heap.allocate(HeapData::List(List::new(result)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns the total of all counts (including negatives).
    fn total_count(&self, heap: &mut Heap<impl ResourceTracker>) -> i64 {
        self.iter_counts(heap)
            .into_iter()
            .map(|(k, v)| {
                k.drop_with_heap(heap);
                v
            })
            .sum()
    }

    /// Builds a Counter result for addition.
    fn add_result(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut counts = self.iter_counts(heap);
        for (key, count) in other.iter_counts(heap) {
            counts.push((key, count));
        }

        let mut dict = Dict::new();
        for (key, count) in counts {
            let current = match dict.get(&key, heap, interns) {
                Ok(Some(Value::Int(n))) => *n,
                Ok(_) => 0,
                Err(e) => {
                    key.drop_with_heap(heap);
                    dict.drop_all_entries(heap);
                    return Err(e);
                }
            };

            let new_count = current + count;
            let key_clone = key.clone_with_heap(heap);
            if let Err(e) = dict.set(key_clone, Value::Int(new_count), heap, interns) {
                key.drop_with_heap(heap);
                dict.drop_all_entries(heap);
                return Err(e);
            }
            key.drop_with_heap(heap);
        }

        let result = Self::from_dict(dict);
        Self::from_counts(result.iter_counts(heap), false, heap, interns)
    }

    /// Allocates a Counter value on the heap from a computed result.
    fn allocate_result_value(result: Self, heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
        Ok(Value::Ref(heap.allocate(HeapData::Counter(result))?))
    }

    /// Builds a Counter result for subtraction.
    fn sub_result(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut counts = self.iter_counts(heap);
        for (key, count) in other.iter_counts(heap) {
            counts.push((key, -count));
        }

        let mut dict = Dict::new();
        for (key, delta) in counts {
            let current = match dict.get(&key, heap, interns) {
                Ok(Some(Value::Int(n))) => *n,
                Ok(_) => 0,
                Err(e) => {
                    key.drop_with_heap(heap);
                    dict.drop_all_entries(heap);
                    return Err(e);
                }
            };

            let new_count = current + delta;
            let key_clone = key.clone_with_heap(heap);
            if let Err(e) = dict.set(key_clone, Value::Int(new_count), heap, interns) {
                key.drop_with_heap(heap);
                dict.drop_all_entries(heap);
                return Err(e);
            }
            key.drop_with_heap(heap);
        }

        let result = Self::from_dict(dict);
        Self::from_counts(result.iter_counts(heap), false, heap, interns)
    }

    /// Builds a Counter result for intersection (`&`).
    fn and_result(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut dict = Dict::new();
        for (key, count) in self.iter_counts(heap) {
            let other_count = other.get_count(&key, heap, interns)?;
            let new_count = std::cmp::min(count, other_count);
            if new_count > 0 {
                if let Some(old) = dict.set(key, Value::Int(new_count), heap, interns)? {
                    old.drop_with_heap(heap);
                }
            } else {
                key.drop_with_heap(heap);
            }
        }
        Ok(Self::from_dict(dict))
    }

    /// Builds a Counter result for union (`|`).
    fn or_result(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut dict = Dict::new();
        for (key, count) in self.iter_counts(heap) {
            if let Some(old) = dict.set(key, Value::Int(count), heap, interns)? {
                old.drop_with_heap(heap);
            }
        }

        for (key, count) in other.iter_counts(heap) {
            let current = match dict.get(&key, heap, interns) {
                Ok(Some(Value::Int(n))) => *n,
                Ok(_) => 0,
                Err(e) => {
                    key.drop_with_heap(heap);
                    dict.drop_all_entries(heap);
                    return Err(e);
                }
            };
            let new_count = std::cmp::max(current, count);
            if let Some(old) = dict.set(key, Value::Int(new_count), heap, interns)? {
                old.drop_with_heap(heap);
            }
        }

        let result = Self::from_dict(dict);
        Self::from_counts(result.iter_counts(heap), false, heap, interns)
    }

    /// Builds a Counter result for unary plus (`+counter`) by keeping only positive counts.
    fn pos_result(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        Self::from_counts(self.iter_counts(heap), false, heap, interns)
    }

    /// Builds a Counter result for unary minus (`-counter`) by negating negative counts.
    fn neg_result(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut negated = Vec::new();
        for (key, count) in self.iter_counts(heap) {
            if count < 0 {
                negated.push((key, -count));
            } else {
                key.drop_with_heap(heap);
            }
        }
        Self::from_counts(negated, false, heap, interns)
    }

    /// Returns `self + other` as a heap-allocated Counter value.
    pub(crate) fn binary_add_value(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, ResourceError> {
        let result = self
            .add_result(other, heap, interns)
            .map_err(run_error_to_resource_error)?;
        Self::allocate_result_value(result, heap)
    }

    /// Returns `self - other` as a heap-allocated Counter value.
    pub(crate) fn binary_sub_value(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, ResourceError> {
        let result = self
            .sub_result(other, heap, interns)
            .map_err(run_error_to_resource_error)?;
        Self::allocate_result_value(result, heap)
    }

    /// Returns `self & other` as a heap-allocated Counter value.
    pub(crate) fn binary_and_value(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, ResourceError> {
        let result = self
            .and_result(other, heap, interns)
            .map_err(run_error_to_resource_error)?;
        Self::allocate_result_value(result, heap)
    }

    /// Returns `self | other` as a heap-allocated Counter value.
    pub(crate) fn binary_or_value(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, ResourceError> {
        let result = self
            .or_result(other, heap, interns)
            .map_err(run_error_to_resource_error)?;
        Self::allocate_result_value(result, heap)
    }

    /// Returns unary `+self` as a heap-allocated Counter value.
    pub(crate) fn unary_pos_value(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, ResourceError> {
        let result = self.pos_result(heap, interns).map_err(run_error_to_resource_error)?;
        Self::allocate_result_value(result, heap)
    }

    /// Returns unary `-self` as a heap-allocated Counter value.
    pub(crate) fn unary_neg_value(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, ResourceError> {
        let result = self.neg_result(heap, interns).map_err(run_error_to_resource_error)?;
        Self::allocate_result_value(result, heap)
    }
}

impl PyTrait for Counter {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Counter
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.dict.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        self.dict.py_eq(&other.dict, heap, interns)
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.dict.py_dec_ref_ids(stack);
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.dict.is_empty()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("Counter(")?;
        self.dict.py_repr_fmt(f, heap, heap_ids, interns)?;
        f.write_char(')')
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.dict.py_estimate_size()
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Value> {
        let count = self.get_count(key, heap, interns)?;
        Ok(Value::Int(count))
    }

    fn py_setitem(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let count = match value {
            Value::Int(n) => n,
            Value::Bool(b) => i64::from(b),
            Value::Ref(_) => value.as_int(heap)?,
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("Counter values must be integers"));
            }
        };
        self.set_count(key, count, heap, interns)
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let method = attr.static_string();
        if method.is_none() && attr.as_str(interns) == "__missing__" {
            let key = args.get_one_arg("Counter.__missing__", heap)?;
            key.drop_with_heap(heap);
            return Ok(Value::Int(0));
        }
        let Some(method) = method else {
            return Err(ExcType::attribute_error(Type::Counter, attr.as_str(interns)));
        };

        match method {
            StaticStrings::MostCommon => {
                let (n_val, _) = args.get_zero_one_two_args("Counter.most_common", heap)?;
                let n = if let Some(n_val) = n_val {
                    let n = n_val.as_int(heap)?;
                    n_val.drop_with_heap(heap);
                    Some(n)
                } else {
                    None
                };
                self.most_common_list(n, heap)
            }
            StaticStrings::Elements => {
                args.check_zero_args("Counter.elements", heap)?;
                self.elements_list(heap)
            }
            StaticStrings::Update => {
                let (mut positional, kwargs) = args.into_parts();
                let positional_count = positional.len();
                if positional_count > 1 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error_at_most("Counter.update", 1, positional_count));
                }

                if let Some(other) = positional.next() {
                    self.update_from_value(other, heap, interns)?;
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
                        return Err(ExcType::type_error_kwargs_nonstring_key());
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
                            return Err(err);
                        }
                    };
                    value.drop_with_heap(heap);

                    if let Err(err) = self.add_count(key.clone_with_heap(heap), count, heap, interns) {
                        key.drop_with_heap(heap);
                        for (rest_key, rest_value) in kwargs_iter {
                            rest_key.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        return Err(err);
                    }
                    key.drop_with_heap(heap);
                }
                Ok(Value::None)
            }
            StaticStrings::Subtract => {
                let other = args.get_one_arg("Counter.subtract", heap)?;
                self.subtract_from_value(other, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::Total => {
                args.check_zero_args("Counter.total", heap)?;
                Ok(Value::Int(self.total_count(heap)))
            }
            // Delegate dict methods (keys, values, items, get, pop, etc.) to the
            // underlying dict.  Forward `self_id` so that view-creating methods
            // (keys/values/items) can look up this Counter on the heap and extract
            // the inner dict.
            _ => self.dict.py_call_attr(heap, attr, args, interns, self_id),
        }
    }

    fn py_add(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Option<Value>, ResourceError> {
        Ok(Some(self.binary_add_value(other, heap, interns)?))
    }
}

/// Converts a `RunError` to a `ResourceError` for use in trait methods that
/// can only return `ResourceError`.
///
/// Internal dict operations in Counter arithmetic can produce `RunError` variants,
/// but the `PyTrait::py_add` signature requires `ResourceError`. This extracts
/// the underlying exception as a `ResourceError::Exception`.
fn run_error_to_resource_error(err: RunError) -> ResourceError {
    match err {
        RunError::UncatchableExc(exc) | RunError::Exc(exc) => ResourceError::Exception(
            crate::exception_public::Exception::new(exc.exc.exc_type(), exc.exc.arg().map(ToString::to_string)),
        ),
        RunError::Internal(msg) => ResourceError::Exception(crate::exception_public::Exception::new(
            ExcType::RuntimeError,
            Some(msg.to_string()),
        )),
    }
}
