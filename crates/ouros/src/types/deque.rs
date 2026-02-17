//! Implementation of Python's `collections.deque` type.
//!
//! A deque (double-ended queue) is a sequence type that supports efficient
//! append and pop operations from both ends. It's implemented using a VecDeque
//! to provide O(1) operations at both ends.
//!
//! This implementation provides the core deque operations:
//! - `append(x)`: Add x to the right side
//! - `appendleft(x)`: Add x to the left side
//! - `pop()`: Remove and return item from the right side
//! - `popleft()`: Remove and return item from the left side
//! - `clear()`: Remove all items
//! - `extend(iterable)`: Extend right side with elements from iterable
//! - `__len__()`, `__iter__()`, `__getitem__()`, `__setitem__()`

use std::{cmp::Ordering, collections::VecDeque, fmt::Write};

use ahash::AHashSet;

use crate::{
    ResourceError, ResourceTracker,
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    types::{AttrCallResult, PyTrait, Type},
    value::{EitherStr, Value},
};

/// A double-ended queue implementation.
///
/// Wraps a `VecDeque<Value>` to provide Python-compatible deque semantics.
/// The deque supports efficient O(1) append and pop operations from both ends.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Deque {
    items: VecDeque<Value>,
    /// Maximum size for bounded deques.
    ///
    /// When set, appends automatically discard items from the opposite end.
    maxlen: Option<usize>,
    /// Tracks whether this deque contains any `Value::Ref` items.
    /// Used for GC cycle detection.
    contains_refs: bool,
}

impl Deque {
    /// Creates a new empty deque.
    #[inline]
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
            maxlen: None,
            contains_refs: false,
        }
    }

    /// Creates a deque from an existing VecDeque.
    #[inline]
    pub fn from_vec_deque(items: VecDeque<Value>) -> Self {
        Self::from_vec_deque_with_maxlen(items, None)
    }

    /// Creates a deque from an existing VecDeque with an optional maxlen.
    #[inline]
    pub fn from_vec_deque_with_maxlen(items: VecDeque<Value>, maxlen: Option<usize>) -> Self {
        let contains_refs = items.iter().any(|v| matches!(v, Value::Ref(_)));
        Self {
            items,
            maxlen,
            contains_refs,
        }
    }

    /// Returns the number of elements in the deque.
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns the configured maximum length, if this deque is bounded.
    #[inline]
    pub fn maxlen(&self) -> Option<usize> {
        self.maxlen
    }

    /// Returns whether the deque is empty.
    #[inline]
    #[expect(dead_code, reason = "reserved for future deque API")]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Appends an item to the right side of the deque.
    #[inline]
    pub fn append(&mut self, item: Value) {
        if matches!(item, Value::Ref(_)) {
            self.contains_refs = true;
        }
        self.items.push_back(item);
    }

    /// Appends an item to the left side of the deque.
    #[inline]
    pub fn appendleft(&mut self, item: Value) {
        if matches!(item, Value::Ref(_)) {
            self.contains_refs = true;
        }
        self.items.push_front(item);
    }

    /// Removes and returns an item from the right side of the deque.
    ///
    /// Returns `None` if the deque is empty.
    #[inline]
    pub fn pop(&mut self) -> Option<Value> {
        self.items.pop_back()
    }

    /// Removes and returns an item from the left side of the deque.
    ///
    /// Returns `None` if the deque is empty.
    #[inline]
    pub fn popleft(&mut self) -> Option<Value> {
        self.items.pop_front()
    }

    /// Clears all items from the deque.
    #[inline]
    #[expect(dead_code, reason = "reserved for future deque API")]
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Extends the deque with items from an iterator.
    #[expect(dead_code, reason = "reserved for future deque API")]
    pub fn extend(&mut self, items: impl Iterator<Item = Value>) {
        for item in items {
            self.append(item);
        }
    }

    /// Returns a reference to the item at the given index.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.items.get(index)
    }

    /// Removes and returns the item at the given index.
    #[inline]
    pub fn remove_index(&mut self, index: usize) -> Option<Value> {
        self.items.remove(index)
    }

    /// Sets the item at the given index.
    #[inline]
    #[expect(dead_code, reason = "reserved for future deque API")]
    pub fn set(&mut self, index: usize, value: Value) {
        if let Some(old) = self.items.get_mut(index) {
            *old = value;
        }
    }

    /// Returns whether the deque contains any heap references.
    #[inline]
    pub fn contains_refs(&self) -> bool {
        self.contains_refs
    }

    /// Returns an iterator over the deque's items.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        self.items.iter()
    }
}

impl Default for Deque {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for Deque {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        for item in self.items {
            item.drop_with_heap(heap);
        }
    }
}

impl super::PyTrait for Deque {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Deque
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.items.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        if self.items.len() != other.items.len() {
            return false;
        }
        self.items
            .iter()
            .zip(other.items.iter())
            .all(|(a, b)| a.py_eq(b, heap, interns))
    }

    fn py_cmp(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<Ordering> {
        let min_len = self.items.len().min(other.items.len());
        for idx in 0..min_len {
            let left = &self.items[idx];
            let right = &other.items[idx];
            if left.py_eq(right, heap, interns) {
                continue;
            }
            return left.py_cmp(right, heap, interns);
        }
        self.items.len().partial_cmp(&other.items.len())
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        for item in &self.items {
            if let Value::Ref(id) = item {
                stack.push(*id);
            }
        }
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("deque([")?;
        let mut first = true;
        for item in &self.items {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            item.py_repr_fmt(f, heap, heap_ids, interns)?;
        }
        if let Some(maxlen) = self.maxlen {
            write!(f, "], maxlen={maxlen})")
        } else {
            f.write_str("])")
        }
    }

    fn py_add(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Result<Option<Value>, ResourceError> {
        let mut new_items = VecDeque::with_capacity(self.items.len() + other.items.len());
        for item in &self.items {
            new_items.push_back(item.clone_with_heap(heap));
        }
        for item in &other.items {
            new_items.push_back(item.clone_with_heap(heap));
        }
        let maxlen = self.maxlen;
        if let Some(maxlen) = maxlen {
            while new_items.len() > maxlen {
                if let Some(removed) = new_items.pop_front() {
                    removed.drop_with_heap(heap);
                }
            }
        }
        let deque = Self::from_vec_deque_with_maxlen(new_items, maxlen);
        let id = heap.allocate(HeapData::Deque(deque))?;
        Ok(Some(Value::Ref(id)))
    }

    fn py_iadd(
        &mut self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        _self_id: Option<HeapId>,
        interns: &Interns,
    ) -> RunResult<bool> {
        let mut iter = crate::types::OurosIter::new(other, heap, interns)?;
        loop {
            match iter.for_next(heap, interns) {
                Ok(Some(item)) => {
                    self.append(item);
                    enforce_maxlen_after_right_push(self, heap);
                }
                Ok(None) => break,
                Err(err) => {
                    iter.drop_with_heap(heap);
                    return Err(err);
                }
            }
        }
        iter.drop_with_heap(heap);
        Ok(true)
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.items.capacity() * std::mem::size_of::<Value>()
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Value> {
        // Support integer indexing
        let idx = match key {
            Value::Int(i) => *i,
            _ => return Err(ExcType::type_error("deque indices must be integers".to_string())),
        };

        let len = self.items.len();
        if len == 0 {
            return Err(ExcType::index_error_pop_empty_list());
        }

        let len_i64 = i64::try_from(len).expect("deque length exceeds i64::MAX");
        let normalized = if idx < 0 { idx + len_i64 } else { idx };

        if normalized < 0 || normalized >= len_i64 {
            return Err(ExcType::index_error_pop_out_of_range());
        }

        let idx_usize = usize::try_from(normalized).expect("index validated non-negative");
        Ok(self.items[idx_usize].clone_with_heap(heap))
    }

    fn py_setitem(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<()> {
        // Support integer indexing
        let idx = if let Value::Int(i) = &key {
            *i
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("deque indices must be integers".to_string()));
        };

        let len = self.items.len();
        if len == 0 {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::index_error_pop_empty_list());
        }

        let len_i64 = i64::try_from(len).expect("deque length exceeds i64::MAX");
        let normalized = if idx < 0 { idx + len_i64 } else { idx };

        if normalized < 0 || normalized >= len_i64 {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::index_error_pop_out_of_range());
        }

        let idx_usize = usize::try_from(normalized).expect("index validated non-negative");
        if idx_usize < self.items.len() {
            let old = std::mem::replace(&mut self.items[idx_usize], value);
            old.drop_with_heap(heap);
        }

        key.drop_with_heap(heap);
        Ok(())
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let method = attr.static_string();
        if method.is_none() && attr.as_str(interns) == "__reversed__" {
            args.check_zero_args("deque.__reversed__", heap)?;
            let mut items: Vec<Value> = self.items.iter().map(|value| value.clone_with_heap(heap)).collect();
            items.reverse();
            let list_id = heap.allocate(HeapData::List(crate::types::List::new(items)))?;
            return Ok(Value::Ref(list_id));
        }
        let Some(method) = method else {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(Type::Deque, attr.as_str(interns)));
        };

        call_deque_method(self, method, args, heap, interns)
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        if attr_id == StringId::from(StaticStrings::Maxlen) {
            let value = match self.maxlen() {
                Some(maxlen) =>
                {
                    #[expect(clippy::cast_possible_wrap, reason = "usize maxlen fits in i64 for Python int")]
                    Value::Int(maxlen as i64)
                }
                None => Value::None,
            };
            return Ok(Some(AttrCallResult::Value(value)));
        }
        Ok(None)
    }
}

/// Dispatches a method call on a deque value.
fn call_deque_method(
    deque: &mut Deque,
    method: StaticStrings,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match method {
        StaticStrings::Append => {
            let item = args.get_one_arg("deque.append", heap)?;
            deque.append(item);
            enforce_maxlen_after_right_push(deque, heap);
            Ok(Value::None)
        }
        StaticStrings::Appendleft => {
            let item = args.get_one_arg("deque.appendleft", heap)?;
            deque.appendleft(item);
            enforce_maxlen_after_left_push(deque, heap);
            Ok(Value::None)
        }
        StaticStrings::Pop => {
            args.check_zero_args("deque.pop", heap)?;
            match deque.pop() {
                Some(item) => Ok(item),
                None => Err(ExcType::index_error_pop_empty_list()),
            }
        }
        StaticStrings::Popleft => {
            args.check_zero_args("deque.popleft", heap)?;
            match deque.popleft() {
                Some(item) => Ok(item),
                None => Err(ExcType::index_error_pop_empty_list()),
            }
        }
        StaticStrings::Clear => {
            args.check_zero_args("deque.clear", heap)?;
            // Drop all items properly
            for item in deque.items.drain(..) {
                item.drop_with_heap(heap);
            }
            Ok(Value::None)
        }
        StaticStrings::Copy => {
            args.check_zero_args("deque.copy", heap)?;
            let items: VecDeque<Value> = deque.items.iter().map(|v| v.clone_with_heap(heap)).collect();
            let new_deque = Deque::from_vec_deque_with_maxlen(items, deque.maxlen());
            let id = heap.allocate(HeapData::Deque(new_deque))?;
            Ok(Value::Ref(id))
        }
        StaticStrings::Extend => {
            let iterable = args.get_one_arg("deque.extend", heap)?;
            let mut iter = crate::types::OurosIter::new(iterable, heap, interns)?;
            loop {
                match iter.for_next(heap, interns) {
                    Ok(Some(item)) => {
                        deque.append(item);
                        enforce_maxlen_after_right_push(deque, heap);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        iter.drop_with_heap(heap);
                        return Err(e);
                    }
                }
            }
            iter.drop_with_heap(heap);
            Ok(Value::None)
        }
        StaticStrings::Extendleft => {
            let iterable = args.get_one_arg("deque.extendleft", heap)?;
            let mut iter = crate::types::OurosIter::new(iterable, heap, interns)?;
            loop {
                match iter.for_next(heap, interns) {
                    Ok(Some(item)) => {
                        deque.appendleft(item);
                        enforce_maxlen_after_left_push(deque, heap);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        iter.drop_with_heap(heap);
                        return Err(e);
                    }
                }
            }
            iter.drop_with_heap(heap);
            Ok(Value::None)
        }
        StaticStrings::Rotate => {
            let amount = args.get_zero_one_arg("deque.rotate", heap)?;
            let n = if let Some(value) = amount {
                let n = value.as_int(heap)?;
                value.drop_with_heap(heap);
                n
            } else {
                1
            };
            let len = deque.items.len();
            if len > 1 {
                #[expect(clippy::cast_possible_wrap, reason = "deque length fits in i64")]
                let len_i64 = len as i64;
                #[expect(clippy::cast_sign_loss, reason = "normalized modulo is non-negative")]
                #[expect(clippy::cast_possible_truncation, reason = "normalized modulo fits in usize")]
                let n_mod = ((n % len_i64) + len_i64) as usize % len;
                if n_mod > 0 {
                    deque.items.rotate_right(n_mod);
                }
            }
            Ok(Value::None)
        }
        StaticStrings::Count => {
            let target = args.get_one_arg("deque.count", heap)?;
            let mut count: i64 = 0;
            for item in &deque.items {
                if item.py_eq(&target, heap, interns) {
                    count += 1;
                }
            }
            target.drop_with_heap(heap);
            Ok(Value::Int(count))
        }
        StaticStrings::Index => {
            let (target, start) = args.get_one_two_args("deque.index", heap)?;
            let start = if let Some(start) = start {
                let idx = start.as_int(heap)?;
                start.drop_with_heap(heap);
                idx
            } else {
                0
            };

            let len = deque.items.len();
            #[expect(clippy::cast_possible_wrap, reason = "deque length fits in i64")]
            let len_i64 = len as i64;
            let mut start_idx = if start < 0 { start + len_i64 } else { start };
            if start_idx < 0 {
                start_idx = 0;
            }
            if start_idx > len_i64 {
                start_idx = len_i64;
            }

            #[expect(clippy::cast_sign_loss, reason = "start index normalized non-negative")]
            let start_usize = start_idx as usize;
            for (idx, item) in deque.items.iter().enumerate().skip(start_usize) {
                if item.py_eq(&target, heap, interns) {
                    target.drop_with_heap(heap);
                    #[expect(clippy::cast_possible_wrap, reason = "usize index fits in i64 for Python int")]
                    return Ok(Value::Int(idx as i64));
                }
            }
            target.drop_with_heap(heap);
            Err(SimpleException::new_msg(ExcType::ValueError, "deque.index(x): x not in deque").into())
        }
        StaticStrings::Insert => {
            let (index, item) = args.get_two_args("deque.insert", heap)?;
            let index = index.as_int(heap)?;
            if let Some(maxlen) = deque.maxlen
                && deque.items.len() >= maxlen
            {
                item.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::IndexError, "deque already at its maximum size").into());
            }

            let len = deque.items.len();
            #[expect(clippy::cast_possible_wrap, reason = "deque length fits in i64")]
            let len_i64 = len as i64;
            let mut index = if index < 0 { index + len_i64 } else { index };
            if index < 0 {
                index = 0;
            }
            if index > len_i64 {
                index = len_i64;
            }
            #[expect(clippy::cast_sign_loss, reason = "normalized insert index is non-negative")]
            let index = index as usize;
            deque.items.insert(index, item);
            Ok(Value::None)
        }
        StaticStrings::Remove => {
            let target = args.get_one_arg("deque.remove", heap)?;
            let Some(remove_idx) = deque.items.iter().position(|item| item.py_eq(&target, heap, interns)) else {
                target.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "deque.remove(x): x not in deque").into());
            };
            let removed = deque.items.remove(remove_idx).expect("position returned valid index");
            removed.drop_with_heap(heap);
            target.drop_with_heap(heap);
            Ok(Value::None)
        }
        StaticStrings::Reverse => {
            args.check_zero_args("deque.reverse", heap)?;
            deque.items.make_contiguous().reverse();
            Ok(Value::None)
        }
        _ => {
            args.drop_with_heap(heap);
            Err(ExcType::attribute_error(Type::Deque, method.into()))
        }
    }
}

/// Enforces maxlen after pushing to the right by discarding from the left.
fn enforce_maxlen_after_right_push(deque: &mut Deque, heap: &mut Heap<impl ResourceTracker>) {
    if let Some(maxlen) = deque.maxlen {
        while deque.items.len() > maxlen {
            if let Some(removed) = deque.items.pop_front() {
                removed.drop_with_heap(heap);
            }
        }
    }
}

/// Enforces maxlen after pushing to the left by discarding from the right.
fn enforce_maxlen_after_left_push(deque: &mut Deque, heap: &mut Heap<impl ResourceTracker>) {
    if let Some(maxlen) = deque.maxlen {
        while deque.items.len() > maxlen {
            if let Some(removed) = deque.items.pop_back() {
                removed.drop_with_heap(heap);
            }
        }
    }
}
