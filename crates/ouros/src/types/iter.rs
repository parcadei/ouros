//! Iterator support for Python for loops and the `iter()` type constructor.
//!
//! This module provides the `OurosIter` struct which encapsulates iteration state
//! for different iterable types. It uses index-based iteration internally to avoid
//! borrow conflicts when accessing the heap during iteration.
//!
//! The design stores iteration state (indices) rather than Rust iterators, allowing
//! `for_next()` to take `&mut Heap` for cloning values and allocating strings.
//!
//! For constructors like `list()` and `tuple()`, use `OurosIter::new()` followed
//! by `collect()` to materialize all items into a Vec.
//!
//! ## Efficient Iteration with `IterState`
//!
//! For the VM's `ForIter` opcode, `advance_on_heap()` uses two strategies:
//!
//! **Fast path** for simple iterators (Range, InternBytes, ASCII IterStr):
//! - Single `get_mut()` call to compute value and advance index
//! - No additional heap access needed during iteration
//!
//! **Multi-phase approach** for complex iterators (IterStr, HeapRef):
//! 1. `iter_state()` - reads current state without mutation, returns `Option<IterState>`
//! 2. Get the value (may access other heap objects like strings or containers)
//! 3. `advance()` - updates the index after the caller has done its work
//!
//! This allows `advance_on_heap()` to coordinate access without extracting
//! the iterator from the heap (avoiding `std::mem::replace` overhead).
//!
//! ## Builtin Support
//!
//! The `iterator_next()` helper implements the `next()` builtin.

use std::{collections::VecDeque, mem};

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{BytesId, Interns, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, List, PyTrait, Range, StdlibObject, Str, Type, str::allocate_char},
    value::{EitherStr, Value},
};

/// Iterator state for Python for loops.
///
/// Contains the current iteration index and the type-specific iteration data.
/// Uses index-based iteration to avoid borrow conflicts when accessing the heap.
///
/// For strings, stores the string content with a byte offset for O(1) UTF-8 iteration.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OurosIter {
    /// Current iteration index, shared across all iterator types.
    index: usize,
    /// Type-specific iteration data.
    iter_value: IterValue,
    /// the actual Value being iterated over.
    value: Value,
}

impl OurosIter {
    /// Creates an iterator from the `iter()` constructor call.
    ///
    /// - `iter(iterable)` - Returns an iterator for the iterable. If the argument is
    ///   already an iterator, returns the same object.
    /// - `iter(callable, sentinel)` - Not yet supported.
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (iterable, sentinel) = args.get_one_two_args("iter", heap)?;

        if let Some(s) = sentinel {
            // Two-argument form: iter(callable, sentinel)
            // This is the sentinel iteration protocol, not yet supported
            iterable.drop_with_heap(heap);
            s.drop_with_heap(heap);
            return Err(ExcType::type_error("iter(callable, sentinel) is not yet supported"));
        }

        // Check if already an iterator - return self.
        // Generators implement the iterator protocol directly (`__iter__` returns self).
        if let Value::Ref(id) = &iterable
            && matches!(heap.get(*id), HeapData::Iter(_) | HeapData::Generator(_))
        {
            // Already an iterator - return it (refcount already correct from caller)
            return Ok(iterable);
        }

        // Create new iterator
        let iter = Self::new(iterable, heap, interns)?;
        let id = heap.allocate(HeapData::Iter(iter))?;
        Ok(Value::Ref(id))
    }

    /// Creates a new OurosIter from a Value.
    ///
    /// Returns an error if the value is not iterable.
    /// For strings, copies the string content for byte-offset based iteration.
    /// For ranges, the data is copied so the heap reference is dropped immediately.
    pub fn new(mut value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        if let Value::Ref(id) = &value
            && matches!(heap.get(*id), HeapData::Generator(_))
        {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("generator requires VM context for iteration"));
        }

        if let Some(iter_value) = IterValue::new(&value, heap, interns) {
            // For Range, we copy next/step/len into ForIterValue::Range, so we don't need
            // to keep the heap object alive during iteration. Drop it immediately to avoid
            // GC issues (the Range isn't in any namespace slot, so GC wouldn't see it).
            // Same for IterStr which copies the string content.
            if matches!(iter_value, IterValue::Range { .. } | IterValue::IterStr { .. }) {
                value.drop_with_heap(heap);
                value = Value::None;
            }
            Ok(Self {
                index: 0,
                iter_value,
                value,
            })
        } else {
            let err = ExcType::type_error_not_iterable(value.py_type(heap));
            value.drop_with_heap(heap);
            Err(err)
        }
    }

    /// Creates an infinite counter iterator for `itertools.count(start, step)`.
    ///
    /// Yields `start`, `start + step`, `start + 2*step`, ... indefinitely.
    /// The runtime stores count operands as immediate numeric `Value`s (`int`/`float`)
    /// to match Python behavior for mixed integer/float stepping.
    pub fn new_count(start: Value, step: Value) -> Self {
        Self {
            index: 0,
            iter_value: IterValue::Count { current: start, step },
            value: Value::None,
        }
    }

    /// Creates an infinite cycling iterator for `itertools.cycle(iterable)`.
    ///
    /// Takes ownership of the item snapshots. Yields them in round-robin order indefinitely.
    /// If `items` is empty, the iterator is immediately exhausted.
    pub fn new_cycle(items: Vec<Value>) -> Self {
        Self {
            index: 0,
            iter_value: IterValue::Cycle { items },
            value: Value::None,
        }
    }

    /// Creates a tee iterator backed by a shared tee state object.
    ///
    /// The iterator holds a reference to the shared tee state so it stays alive
    /// while any clones are in use.
    #[must_use]
    pub fn new_tee(tee_ref: Value, tee_id: HeapId, slot: usize) -> Self {
        Self {
            index: 0,
            iter_value: IterValue::Tee { tee_id, slot },
            value: tee_ref,
        }
    }

    /// Drops the iterator and its held value properly.
    ///
    /// For `Cycle` iterators, also drops all stored item snapshots.
    pub fn drop_with_heap(self, heap: &mut Heap<impl ResourceTracker>) {
        self.value.drop_with_heap(heap);
        // Cycle stores owned item copies that need cleanup
        if let IterValue::Cycle { items } = self.iter_value {
            for item in items {
                item.drop_with_heap(heap);
            }
        }
    }

    /// Collects HeapIds from this iterator for reference counting cleanup.
    pub fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.value.py_dec_ref_ids(stack);
        // Cycle items may hold heap references
        if let IterValue::Cycle { ref mut items } = self.iter_value {
            for item in items {
                item.py_dec_ref_ids(stack);
            }
        }
    }

    /// Returns whether this iterator holds a heap reference (`Value::Ref`).
    ///
    /// Used during allocation to determine if this container could create cycles.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        matches!(self.value, Value::Ref(_))
    }

    /// Returns true if this iterator requires extraction from the heap for advancement.
    ///
    /// Some iterators must run through `for_next()` and cannot use the in-place
    /// fast path or multi-phase `iter_state()` flow. These iterators either need
    /// internal mutable state handling that is already centralized in `for_next()`
    /// (e.g. `Cycle`, `Tee`) or call into heap-backed stdlib iterator objects
    /// (`StringIO`, CSV readers), so we temporarily extract them via `mem::replace`.
    #[must_use]
    fn needs_extraction(&self) -> bool {
        matches!(
            self.iter_value,
            IterValue::Cycle { .. }
                | IterValue::Tee { .. }
                | IterValue::StringIORef { .. }
                | IterValue::BytesIORef { .. }
                | IterValue::CsvReaderRef { .. }
                | IterValue::CsvDictReaderRef { .. }
        )
    }

    /// Returns a reference to the underlying value being iterated.
    ///
    /// Used by GC to traverse heap references held by the iterator.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Returns the current iterator state without mutation.
    ///
    /// This is used by the multi-phase approach in `advance_on_heap()` for complex
    /// iterator types (IterStr, HeapRef). Simple types (Range, InternBytes, ASCII
    /// IterStr) are handled by the fast path and should not call this method.
    ///
    /// Returns `None` if the iterator is exhausted.
    fn iter_state(&self) -> Option<IterState> {
        match &self.iter_value {
            // Range, InternBytes, and ASCII IterStr are handled by try_advance_simple() fast path
            IterValue::Range { .. } | IterValue::InternBytes { .. } => {
                unreachable!("Range and InternBytes use fast path, not iter_state")
            }
            IterValue::IterStr {
                string,
                byte_offset,
                len,
                ..
            } => {
                if self.index >= *len {
                    None
                } else {
                    // Get the next character at current byte offset
                    let c = string[*byte_offset..]
                        .chars()
                        .next()
                        .expect("index < len implies char exists");
                    Some(IterState::IterStr {
                        char: c,
                        char_len: c.len_utf8(),
                    })
                }
            }
            IterValue::HeapRef {
                heap_id,
                len,
                checks_mutation,
            } => {
                // For types with captured len, check exhaustion here.
                // For List (len=None), exhaustion is checked in advance_on_heap().
                if let Some(l) = len
                    && self.index >= *l
                {
                    return None;
                }
                Some(IterState::HeapIndex {
                    heap_id: *heap_id,
                    index: self.index,
                    expected_len: if *checks_mutation { *len } else { None },
                })
            }
            // Count uses the fast path (try_advance_simple), so iter_state is not called
            IterValue::Count { .. } => {
                unreachable!("Count uses fast path, not iter_state")
            }
            // Cycle, Tee, StringIO, and CSV readers are handled via for_next() and a special-case in
            // advance_on_heap(), so iter_state is not called.
            IterValue::Cycle { .. }
            | IterValue::Tee { .. }
            | IterValue::StringIORef { .. }
            | IterValue::BytesIORef { .. }
            | IterValue::CsvReaderRef { .. }
            | IterValue::CsvDictReaderRef { .. } => {
                unreachable!("Cycle/Tee/StringIO/CSV iterators are used via for_next(), not iter_state")
            }
        }
    }

    /// Advances the iterator by one step.
    ///
    /// This is phase 2 of the two-phase iteration approach. Call this after
    /// successfully retrieving the value using the data from `iter_state()`.
    ///
    /// For string iterators, `string_char_len` must be provided (the UTF-8 byte
    /// length of the character that was just yielded) to update the byte offset.
    /// For other iterator types, pass `None`.
    #[inline]
    pub fn advance(&mut self, string_char_len: Option<usize>) {
        self.index += 1;
        if let Some(char_len) = string_char_len
            && let IterValue::IterStr { byte_offset, .. } = &mut self.iter_value
        {
            *byte_offset += char_len;
        }
    }

    /// Attempts to advance simple iterator types that don't need additional heap access.
    ///
    /// Returns `Some(result)` if handled (Range, InternBytes, ASCII IterStr),
    /// `None` if caller should use the multi-phase approach (non-ASCII IterStr, HeapRef).
    ///
    /// This optimization avoids two heap lookups for iterator types that can compute
    /// their next value without accessing other heap objects.
    #[inline]
    fn try_advance_simple(&mut self, interns: &Interns) -> Option<RunResult<Option<Value>>> {
        match &mut self.iter_value {
            IterValue::Range { next, step, len } => {
                if self.index >= *len {
                    Some(Ok(None))
                } else {
                    let value = *next;
                    *next += *step;
                    self.index += 1;
                    Some(Ok(Some(Value::Int(value))))
                }
            }
            IterValue::IterStr {
                string,
                byte_offset,
                len,
                is_ascii,
            } => {
                if !*is_ascii {
                    None
                } else if self.index >= *len {
                    Some(Ok(None))
                } else {
                    let byte = string.as_bytes()[*byte_offset];
                    *byte_offset += 1;
                    self.index += 1;
                    Some(Ok(Some(Value::InternString(StringId::from_ascii(byte)))))
                }
            }
            IterValue::InternBytes { bytes_id, len } => {
                if self.index >= *len {
                    Some(Ok(None))
                } else {
                    let i = self.index;
                    self.index += 1;
                    let bytes = interns.get_bytes(*bytes_id);
                    Some(Ok(Some(Value::Int(i64::from(bytes[i])))))
                }
            }
            IterValue::HeapRef { .. } => None,
            IterValue::Count { current, step } => match advance_count_value(current, step) {
                Ok(value) => {
                    self.index += 1;
                    Some(Ok(Some(value)))
                }
                Err(err) => Some(Err(err)),
            },
            // Extraction-routed iterators fall through to for_next()
            IterValue::Cycle { .. }
            | IterValue::Tee { .. }
            | IterValue::StringIORef { .. }
            | IterValue::BytesIORef { .. }
            | IterValue::CsvReaderRef { .. }
            | IterValue::CsvDictReaderRef { .. } => None,
        }
    }

    /// Returns the next item from the iterator, advancing the internal index.
    ///
    /// Returns `Ok(None)` when the iterator is exhausted.
    /// Returns `Err` if allocation fails (for string character iteration) or if
    /// a dict/set changes size during iteration (RuntimeError).
    pub fn for_next(&mut self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Option<Value>> {
        match &mut self.iter_value {
            IterValue::Range { next, step, len } => {
                if self.index >= *len {
                    return Ok(None);
                }
                let value = *next;
                *next += *step;
                self.index += 1;
                Ok(Some(Value::Int(value)))
            }
            IterValue::IterStr {
                string,
                byte_offset,
                len,
                is_ascii,
            } => {
                if self.index >= *len {
                    Ok(None)
                } else if *is_ascii {
                    let byte = string.as_bytes()[*byte_offset];
                    *byte_offset += 1;
                    self.index += 1;
                    Ok(Some(Value::InternString(StringId::from_ascii(byte))))
                } else {
                    // Get next char at current byte offset
                    let c = string[*byte_offset..]
                        .chars()
                        .next()
                        .expect("index < len implies char exists");
                    *byte_offset += c.len_utf8();
                    self.index += 1;
                    Ok(Some(allocate_char(c, heap)?))
                }
            }
            IterValue::InternBytes { bytes_id, len } => {
                if self.index >= *len {
                    return Ok(None);
                }
                let i = self.index;
                self.index += 1;
                let bytes = interns.get_bytes(*bytes_id);
                Ok(Some(Value::Int(i64::from(bytes[i]))))
            }
            IterValue::HeapRef {
                heap_id,
                len,
                checks_mutation,
            } => {
                // Check exhaustion for types with captured len
                if let Some(l) = len
                    && self.index >= *l
                {
                    return Ok(None);
                }
                let i = self.index;
                let expected_len = if *checks_mutation { *len } else { None };
                let item = get_heap_item(heap, *heap_id, i, expected_len)?;
                // Check for list exhaustion (list can shrink during iteration)
                let Some(item) = item else {
                    return Ok(None);
                };
                self.index += 1;
                Ok(Some(clone_and_inc_ref(item, heap)))
            }
            IterValue::Count { current, step } => {
                let value = advance_count_value(current, step)?;
                self.index += 1;
                Ok(Some(value))
            }
            IterValue::Cycle { items } => {
                if items.is_empty() {
                    return Ok(None);
                }
                let i = self.index % items.len();
                self.index += 1;
                Ok(Some(items[i].clone_with_heap(heap)))
            }
            IterValue::Tee { tee_id, slot } => {
                let index = self.index;
                let heap_data = mem::replace(heap.get_mut(*tee_id), HeapData::List(List::new(Vec::new())));
                let HeapData::Tee(mut tee_state) = heap_data else {
                    unreachable!("tee iterator must reference tee state")
                };
                let result = tee_state.fetch_value(index, heap, interns);
                if let Ok(Some(_)) = result {
                    self.index += 1;
                    tee_state.advance_slot(*slot, self.index, heap);
                }
                *heap.get_mut(*tee_id) = HeapData::Tee(tee_state);
                result
            }
            IterValue::StringIORef { heap_id } => {
                // Temporarily swap out the StringIO state to call readline().
                // We use mem::replace to avoid borrow conflicts (we need &mut Heap for allocating
                // the result string, but also need to mutate the StringIO's position).
                let placeholder = HeapData::List(List::new(Vec::new()));
                let heap_data = mem::replace(heap.get_mut(*heap_id), placeholder);
                let HeapData::StdlibObject(StdlibObject::StringIO(mut state)) = heap_data else {
                    // Not a StringIO (shouldn't happen) -- restore and signal exhaustion
                    *heap.get_mut(*heap_id) = heap_data;
                    return Ok(None);
                };
                let line = state.readline();
                *heap.get_mut(*heap_id) = HeapData::StdlibObject(StdlibObject::StringIO(state));
                if line.is_empty() {
                    Ok(None)
                } else {
                    self.index += 1;
                    let id = heap.allocate(HeapData::Str(Str::from(line)))?;
                    Ok(Some(Value::Ref(id)))
                }
            }
            IterValue::BytesIORef { heap_id } => match heap.call_attr_raw(
                *heap_id,
                &EitherStr::Heap("__next__".to_owned()),
                ArgValues::Empty,
                interns,
            ) {
                Ok(AttrCallResult::Value(value)) => {
                    self.index += 1;
                    Ok(Some(value))
                }
                Ok(_) => {
                    Err(SimpleException::new_msg(ExcType::RuntimeError, "iterator __next__ returned non-value").into())
                }
                Err(err) if err.is_stop_iteration() => Ok(None),
                Err(err) => Err(err),
            },
            IterValue::CsvReaderRef { heap_id } | IterValue::CsvDictReaderRef { heap_id } => match heap.call_attr_raw(
                *heap_id,
                &EitherStr::Heap("__next__".to_owned()),
                ArgValues::Empty,
                interns,
            ) {
                Ok(AttrCallResult::Value(value)) => {
                    self.index += 1;
                    Ok(Some(value))
                }
                Ok(_) => {
                    Err(SimpleException::new_msg(ExcType::RuntimeError, "iterator __next__ returned non-value").into())
                }
                Err(err) if err.is_stop_iteration() => Ok(None),
                Err(err) => Err(err),
            },
        }
    }

    /// Returns the remaining size for iterables based on current state.
    ///
    /// For immutable types (Range, Tuple, Str, Bytes, FrozenSet), returns the exact remaining count.
    /// For List, returns current length minus index (may change if list is mutated).
    /// For Dict and Set, returns the captured length minus index (used for size-change detection).
    pub fn size_hint(&self, heap: &Heap<impl ResourceTracker>) -> usize {
        let len = match &self.iter_value {
            IterValue::Range { len, .. } | IterValue::IterStr { len, .. } | IterValue::InternBytes { len, .. } => *len,
            IterValue::HeapRef { heap_id, len, .. } => {
                // For List/Deque (len=None), check current length dynamically.
                len.unwrap_or_else(|| match heap.get(*heap_id) {
                    HeapData::List(list) => list.len(),
                    HeapData::Deque(deque) => deque.len(),
                    _ => 0,
                })
            }
            IterValue::Tee { tee_id, .. } => {
                let HeapData::Tee(state) = heap.get(*tee_id) else {
                    return 0;
                };
                state.remaining_from(self.index, heap)
            }
            // Infinite iterators return a large hint
            IterValue::Count { .. } | IterValue::Cycle { .. } => usize::MAX,
            // StringIO: unknown remaining lines, return 0 as a conservative hint
            IterValue::StringIORef { .. }
            | IterValue::BytesIORef { .. }
            | IterValue::CsvReaderRef { .. }
            | IterValue::CsvDictReaderRef { .. } => {
                return 0;
            }
        };
        len.saturating_sub(self.index)
    }

    /// Collects all remaining items from the iterator into a Vec.
    ///
    /// Consumes the iterator and returns all items. Used by `list()`, `tuple()`,
    /// and similar constructors that need to materialize all items.
    ///
    /// Pre-allocates capacity based on `size_hint()` for better performance.
    pub fn collect<T: FromIterator<Value>>(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<T> {
        HeapedOurosIter(self, heap, interns).collect()
    }
}

struct HeapedOurosIter<'a, T: ResourceTracker>(&'a mut OurosIter, &'a mut Heap<T>, &'a Interns);

impl<T: ResourceTracker> Iterator for HeapedOurosIter<'_, T> {
    type Item = RunResult<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.for_next(self.1, self.2).transpose()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.0.size_hint(self.1);
        (remaining, Some(remaining))
    }
}

/// Advances an iterator stored on the heap and returns the next value.
///
/// Uses a fast path for simple iterators (Range, InternBytes, ASCII IterStr) that don't need
/// additional heap access - these are handled with a single mutable borrow.
///
/// For complex iterators (IterStr, HeapRef), uses a multi-phase approach:
/// 1. Read iterator state (immutable borrow ends)
/// 2. Based on state, get the value (may access other heap objects)
/// 3. Update iterator index (mutable borrow)
///
/// This is more efficient than `std::mem::replace` with a placeholder because
/// it avoids creating and moving placeholder objects on every iteration.
/// Iterators marked by `needs_extraction()` (`Cycle`, `Tee`, `StringIO`, CSV readers)
/// are explicitly routed through extraction + `for_next()`.
///
/// Returns `Ok(None)` when the iterator is exhausted.
/// Returns `Err` for dict/set size changes or allocation failures.
pub(crate) fn advance_on_heap(
    heap: &mut Heap<impl ResourceTracker>,
    iter_id: HeapId,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    // Iterators marked as extraction-required (Cycle/Tee/StringIO/CSV) are handled
    // by temporarily extracting the iterator from the heap and delegating to for_next().
    if let HeapData::Iter(iter) = heap.get(iter_id)
        && iter.needs_extraction()
    {
        let heap_data = mem::replace(heap.get_mut(iter_id), HeapData::List(List::new(Vec::new())));
        let HeapData::Iter(mut iter) = heap_data else {
            unreachable!("advance_on_heap: expected Iterator on heap")
        };
        let result = iter.for_next(heap, interns);
        *heap.get_mut(iter_id) = HeapData::Iter(iter);
        return result;
    }

    if !matches!(heap.get(iter_id), HeapData::Iter(_)) {
        return Err(ExcType::type_error_not_iterable(heap.get(iter_id).py_type(heap)));
    }

    // Fast path: Range and InternBytes don't need additional heap access,
    // so we can handle them with a single mutable borrow.
    {
        let HeapData::Iter(iter) = heap.get_mut(iter_id) else {
            return Err(
                SimpleException::new_msg(ExcType::RuntimeError, "advance_on_heap: expected Iterator on heap").into(),
            );
        };
        if let Some(result) = iter.try_advance_simple(interns) {
            return result;
        }
    }
    // Mutable borrow ends here, allowing the multi-phase approach below

    // Multi-phase approach for IterStr and HeapRef (need heap access during value retrieval)
    // Phase 1: Get iterator state (immutable borrow ends after this block)
    let HeapData::Iter(iter) = heap.get(iter_id) else {
        return Err(
            SimpleException::new_msg(ExcType::RuntimeError, "advance_on_heap: expected Iterator on heap").into(),
        );
    };
    let Some(state) = iter.iter_state() else {
        return Ok(None); // Iterator exhausted
    };

    // Phase 2: Based on state, get the value and determine char_len for strings
    let (value, string_char_len) = match state {
        IterState::IterStr { char, char_len } => {
            let value = allocate_char(char, heap)?;
            (value, Some(char_len))
        }
        IterState::HeapIndex {
            heap_id,
            index,
            expected_len,
        } => {
            let item = get_heap_item(heap, heap_id, index, expected_len)?;
            // Check for list exhaustion (list can shrink during iteration)
            let Some(item) = item else {
                return Ok(None);
            };
            // Inc refcount after borrow ends
            if let Value::Ref(id) = &item {
                heap.inc_ref(*id);
            }
            (item, None)
        }
    };

    // Phase 3: Advance the iterator
    let HeapData::Iter(iter) = heap.get_mut(iter_id) else {
        return Err(
            SimpleException::new_msg(ExcType::RuntimeError, "advance_on_heap: expected Iterator on heap").into(),
        );
    };
    iter.advance(string_char_len);

    Ok(Some(value))
}

/// Gets an item from a heap-allocated container at the given index.
///
/// Returns `Ok(None)` if the index is out of bounds (for lists that shrunk during iteration).
/// Returns `Err` if a dict/set changed size during iteration (RuntimeError).
fn get_heap_item(
    heap: &mut Heap<impl ResourceTracker>,
    heap_id: HeapId,
    index: usize,
    expected_len: Option<usize>,
) -> RunResult<Option<Value>> {
    match heap.get(heap_id) {
        HeapData::List(list) => {
            // Check if list shrunk during iteration
            if index >= list.len() {
                return Ok(None);
            }
            Ok(Some(list.as_vec()[index].copy_for_extend()))
        }
        HeapData::Deque(deque) => {
            // Check if deque shrunk during iteration
            if index >= deque.len() {
                return Ok(None);
            }
            Ok(Some(deque.get(index).expect("index should be valid").copy_for_extend()))
        }
        HeapData::Tuple(tuple) => Ok(Some(tuple.as_vec()[index].copy_for_extend())),
        HeapData::NamedTuple(namedtuple) => Ok(Some(namedtuple.as_vec()[index].copy_for_extend())),
        HeapData::Dict(dict) => {
            // Check for dict mutation
            if let Some(expected) = expected_len
                && dict.len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            Ok(Some(
                dict.key_at(index).expect("index should be valid").copy_for_extend(),
            ))
        }
        HeapData::DefaultDict(dd) => {
            // Check for dict mutation
            if let Some(expected) = expected_len
                && dd.len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            Ok(Some(
                dd.dict()
                    .key_at(index)
                    .expect("index should be valid")
                    .copy_for_extend(),
            ))
        }
        HeapData::Counter(counter) => {
            // Counter iterates over keys like dict and uses the same mutation error.
            if let Some(expected) = expected_len
                && counter.len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            Ok(Some(
                counter
                    .dict()
                    .key_at(index)
                    .expect("index should be valid")
                    .copy_for_extend(),
            ))
        }
        HeapData::OrderedDict(od) => {
            // OrderedDict iterates over keys in insertion order, like dict.
            if let Some(expected) = expected_len
                && od.dict().len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            Ok(Some(
                od.dict()
                    .key_at(index)
                    .expect("index should be valid")
                    .copy_for_extend(),
            ))
        }
        HeapData::ChainMap(chain_map) => {
            // ChainMap iteration yields unique keys from the merged mapping view.
            if let Some(expected) = expected_len
                && chain_map.flat().len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            Ok(Some(
                chain_map
                    .flat()
                    .key_at(index)
                    .expect("index should be valid")
                    .copy_for_extend(),
            ))
        }
        HeapData::Bytes(bytes) => Ok(Some(Value::Int(i64::from(bytes.as_slice()[index])))),
        HeapData::Bytearray(bytes) => Ok(Some(Value::Int(i64::from(bytes.as_slice()[index])))),
        HeapData::Set(set) => {
            // Check for set mutation
            if let Some(expected) = expected_len
                && set.len() != expected
            {
                return Err(ExcType::runtime_error_set_changed_size());
            }
            Ok(Some(
                set.storage()
                    .value_at(index)
                    .expect("index should be valid")
                    .copy_for_extend(),
            ))
        }
        HeapData::FrozenSet(frozenset) => Ok(Some(
            frozenset
                .storage()
                .value_at(index)
                .expect("index should be valid")
                .copy_for_extend(),
        )),
        // DictKeys iterates over keys
        HeapData::DictKeys(dk) => {
            let Some(dict) = dk.get_dict(heap) else {
                return Ok(None); // Dict was deleted
            };
            if let Some(expected) = expected_len
                && dict.len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            Ok(Some(
                dict.key_at(index).expect("index should be valid").copy_for_extend(),
            ))
        }
        // DictValues iterates over values
        HeapData::DictValues(dv) => {
            let Some(dict) = dv.get_dict(heap) else {
                return Ok(None); // Dict was deleted
            };
            if let Some(expected) = expected_len
                && dict.len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            let (_, value): (&Value, &Value) = dict.iter().nth(index).expect("index should be valid");
            Ok(Some(value.copy_for_extend()))
        }
        // DictItems iterates over (key, value) tuples
        HeapData::DictItems(di) => {
            let Some(dict) = di.get_dict(heap) else {
                return Ok(None); // Dict was deleted
            };
            if let Some(expected) = expected_len
                && dict.len() != expected
            {
                return Err(ExcType::runtime_error_dict_changed_size());
            }
            let (key, value): (&Value, &Value) = dict.iter().nth(index).expect("index should be valid");
            let key_copy: Value = key.copy_for_extend();
            let value_copy: Value = value.copy_for_extend();
            // Increment refcounts after borrowing the dict
            if let Value::Ref(key_id) = &key_copy {
                heap.inc_ref(*key_id);
            }
            if let Value::Ref(val_id) = &value_copy {
                heap.inc_ref(*val_id);
            }
            let tuple_val = crate::types::allocate_tuple(smallvec::smallvec![key_copy, value_copy], heap)?;
            Ok(Some(tuple_val))
        }
        other => Err(ExcType::type_error_not_iterable(other.py_type(heap))),
    }
}

/// Gets the next item from an iterator.
///
/// If the iterator is exhausted:
/// - If `default` is `Some`, returns the default value
/// - If `default` is `None`, raises `StopIteration`
///
/// This implements Python's `next()` builtin semantics.
///
/// # Arguments
/// * `iter_value` - Must be an iterator (heap-allocated OurosIter)
/// * `default` - Optional default value to return when exhausted
/// * `heap` - The heap for memory operations
/// * `interns` - String interning table
///
/// # Errors
/// Returns `StopIteration` if exhausted with no default, or propagates errors from iteration.
pub fn iterator_next(
    iter_value: &Value,
    default: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let Value::Ref(iter_id) = iter_value else {
        // Not a heap value - can't be an iterator
        if let Some(d) = default {
            d.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_not_iterable(iter_value.py_type(heap)));
    };

    if matches!(heap.get(*iter_id), HeapData::Iter(_)) {
        // Get next item using the OurosIter::advance_on_heap method.
        return match advance_on_heap(heap, *iter_id, interns)? {
            Some(item) => {
                if let Some(d) = default {
                    d.drop_with_heap(heap);
                }
                Ok(item)
            }
            None => match default {
                Some(d) => Ok(d),
                None => Err(ExcType::stop_iteration()),
            },
        };
    }

    // Support stdlib iterator-like objects used by csv/io.
    if matches!(
        heap.get(*iter_id),
        HeapData::StdlibObject(
            StdlibObject::StringIO(_)
                | StdlibObject::BytesIO(_)
                | StdlibObject::CsvReader(_)
                | StdlibObject::CsvDictReader(_)
        )
    ) {
        match heap.call_attr_raw(
            *iter_id,
            &EitherStr::Heap("__next__".to_owned()),
            ArgValues::Empty,
            interns,
        ) {
            Ok(AttrCallResult::Value(value)) => {
                if let Some(d) = default {
                    d.drop_with_heap(heap);
                }
                return Ok(value);
            }
            Ok(_) => {
                if let Some(d) = default {
                    d.drop_with_heap(heap);
                }
                return Err(
                    SimpleException::new_msg(ExcType::RuntimeError, "iterator __next__ returned non-value").into(),
                );
            }
            Err(err) if err.is_stop_iteration() => {
                return match default {
                    Some(d) => Ok(d),
                    None => Err(ExcType::stop_iteration()),
                };
            }
            Err(err) => {
                if let Some(d) = default {
                    d.drop_with_heap(heap);
                }
                return Err(err);
            }
        }
    }

    if let Some(d) = default {
        d.drop_with_heap(heap);
    }
    let data_type = heap.get(*iter_id).py_type(heap);
    Err(ExcType::type_error(format!("'{data_type}' object is not an iterator")))
}

/// Snapshot of iterator state needed to produce the next value.
///
/// This enum captures state for complex iterator types (IterStr, HeapRef) that
/// require the multi-phase approach in `advance_on_heap()`. Simple types (Range,
/// InternBytes, ASCII IterStr) are handled by the fast path and don't use this enum.
///
/// The multi-phase approach avoids borrow conflicts:
/// 1. Read `Option<IterState>` from iterator (immutable borrow ends, `None` means exhausted)
/// 2. Use the state to get the value (may access other heap objects)
/// 3. Call `advance()` to update the iterator index
#[derive(Debug, Clone, Copy)]
enum IterState {
    /// String iterator yields this character; char_len is UTF-8 byte length for advance().
    IterStr { char: char, char_len: usize },
    /// Heap-based iterator (List, Tuple, NamedTuple, Dict, Bytes, Set, FrozenSet).
    /// The expected_len is Some for types that check for mutation (Dict, Set).
    HeapIndex {
        heap_id: HeapId,
        index: usize,
        expected_len: Option<usize>,
    },
}

/// Increments the reference count for a value copied via `copy_for_extend()`.
///
/// This is the second half of the two-phase clone pattern: first copy the value
/// without incrementing refcount (to avoid borrow conflicts), then increment
/// the refcount once the heap borrow is released.
fn clone_and_inc_ref(value: Value, heap: &mut Heap<impl ResourceTracker>) -> Value {
    if let Value::Ref(ref_id) = &value {
        heap.inc_ref(*ref_id);
    }
    value
}

/// Advances a `count()` accumulator and returns the yielded value.
///
/// `current` and `step` are normalized to immediate numeric values (`int`/`float`)
/// by the itertools module before iterator creation.
fn advance_count_value(current: &mut Value, step: &Value) -> RunResult<Value> {
    let yielded = current.clone_immediate();
    let next = match (&*current, step) {
        (Value::Int(c), Value::Int(s)) => Value::Int(c + s),
        (Value::Int(c), Value::Float(s)) => Value::Float(*c as f64 + s),
        (Value::Float(c), Value::Int(s)) => Value::Float(c + *s as f64),
        (Value::Float(c), Value::Float(s)) => Value::Float(c + s),
        _ => {
            return Err(ExcType::type_error(
                "count() start and step must be int or float in this runtime",
            ));
        }
    };
    *current = next;
    Ok(yielded)
}

/// Shared state for `itertools.tee` iterators.
///
/// Stores the underlying source iterator, a shared buffer of yielded items, and
/// per-iterator positions so clones can advance independently.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct TeeState {
    /// Underlying source iterator.
    source: OurosIter,
    /// Shared buffer of items already pulled from the source.
    buffer: VecDeque<Value>,
    /// Absolute index of the first item in the buffer.
    buffer_start: usize,
    /// Per-iterator absolute positions (next index to read).
    positions: Vec<usize>,
    /// Whether the source iterator is exhausted.
    exhausted: bool,
}

impl TeeState {
    /// Creates a new tee shared state for the given number of clones.
    #[must_use]
    pub fn new(source: OurosIter, clones: usize) -> Self {
        Self {
            source,
            buffer: VecDeque::new(),
            buffer_start: 0,
            positions: vec![0; clones],
            exhausted: false,
        }
    }

    /// Returns the next value at the given absolute index, pulling from the source if needed.
    fn fetch_value(
        &mut self,
        index: usize,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        if index < self.buffer_start {
            return Ok(None);
        }

        let offset = index - self.buffer_start;
        while offset >= self.buffer.len() {
            if self.exhausted {
                return Ok(None);
            }
            if let Some(value) = self.source.for_next(heap, interns)? {
                self.buffer.push_back(value);
            } else {
                self.exhausted = true;
                return Ok(None);
            }
        }

        Ok(Some(
            self.buffer
                .get(offset)
                .expect("offset checked against buffer length")
                .clone_with_heap(heap),
        ))
    }

    /// Updates an iterator's position and drops buffered items no longer needed.
    fn advance_slot(&mut self, slot: usize, new_index: usize, heap: &mut Heap<impl ResourceTracker>) {
        if let Some(pos) = self.positions.get_mut(slot) {
            *pos = new_index;
        }
        self.prune_buffer(heap);
    }

    /// Returns an upper-bound size hint from the given iterator index.
    #[must_use]
    pub fn remaining_from(&self, index: usize, heap: &Heap<impl ResourceTracker>) -> usize {
        let buffer_end = self.buffer_start.saturating_add(self.buffer.len());
        let buffer_remaining = buffer_end.saturating_sub(index);
        let source_remaining = self.source.size_hint(heap);
        buffer_remaining.saturating_add(source_remaining)
    }

    /// Returns whether the tee state contains any heap references.
    #[must_use]
    pub fn has_refs(&self) -> bool {
        if self.source.has_refs() {
            return true;
        }
        self.buffer.iter().any(|value| matches!(value, Value::Ref(_)))
    }

    /// Collects heap references from the source iterator and buffer for GC traversal.
    pub fn collect_child_ids(&self, work_list: &mut Vec<HeapId>) {
        if let Value::Ref(id) = self.source.value() {
            work_list.push(*id);
        }
        for value in &self.buffer {
            if let Value::Ref(id) = value {
                work_list.push(*id);
            }
        }
    }

    /// Drops buffered items that are no longer needed by any iterator.
    fn prune_buffer(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        let Some(min_pos) = self.positions.iter().copied().min() else {
            return;
        };
        while self.buffer_start < min_pos {
            if let Some(value) = self.buffer.pop_front() {
                value.drop_with_heap(heap);
            }
            self.buffer_start += 1;
        }
    }
}

impl PyTrait for TeeState {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Iterator
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.source.py_dec_ref_ids(stack);
        for value in &mut self.buffer {
            value.py_dec_ref_ids(stack);
        }
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl std::fmt::Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut ahash::AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<itertools.tee>")
    }

    fn py_estimate_size(&self) -> usize {
        let buffer_bytes = self.buffer.capacity() * std::mem::size_of::<Value>();
        let positions_bytes = self.positions.len() * std::mem::size_of::<usize>();
        std::mem::size_of::<Self>() + buffer_bytes + positions_bytes
    }
}

/// Type-specific iteration data for different Python iterable types.
///
/// Each variant stores the data needed to iterate over a specific type,
/// excluding the index which is stored in the parent `OurosIter` struct.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum IterValue {
    /// Iterating over a Range, yields `Value::Int`.
    Range {
        /// Next value to yield.
        next: i64,
        /// Step between values.
        step: i64,
        /// Total number of elements.
        len: usize,
    },
    /// Iterating over a string (heap or interned), yields single-char Str values.
    ///
    /// Stores a copy of the string content plus a byte offset for O(1) UTF-8 character access.
    /// We store the string rather than referencing the heap because `for_next()` needs mutable
    /// heap access to allocate the returned character strings, which would conflict with
    /// borrowing the source string from the heap.
    IterStr {
        /// Copy of the string content for iteration.
        string: String,
        /// Current byte offset into the string (points to next char to yield).
        byte_offset: usize,
        /// Total number of characters in the string.
        len: usize,
        /// Whether the string is ASCII (enables fast-path iteration).
        is_ascii: bool,
    },
    /// Iterating over interned bytes, yields `Value::Int` for each byte.
    InternBytes { bytes_id: BytesId, len: usize },
    /// Iterating over a heap-allocated container (List, Tuple, NamedTuple, Dict, Bytes, Set, FrozenSet).
    ///
    /// - `len`: `None` for List (checked dynamically since lists can mutate during iteration),
    ///   `Some(n)` for other types (captured at construction for exhaustion checking).
    /// - `checks_mutation`: `true` for Dict/Set (raises RuntimeError if size changes),
    ///   `false` for other types.
    HeapRef {
        heap_id: HeapId,
        len: Option<usize>,
        checks_mutation: bool,
    },
    /// Infinite counter from `itertools.count(start, step)`.
    ///
    /// Yields numeric values (`int`/`float`) starting at `current` and incrementing
    /// by `step` on each call. Never exhausts — must be used with `islice` or
    /// similar to limit output.
    Count {
        /// Next value to yield.
        current: Value,
        /// Increment between values.
        step: Value,
    },
    /// Infinite cycler from `itertools.cycle(iterable)`.
    ///
    /// Stores a snapshot of the iterable's items and yields them in a round-robin loop.
    /// Never exhausts — must be used with `islice` or similar to limit output.
    Cycle {
        /// Snapshot of the iterable's items (owned copies with correct refcounts).
        items: Vec<Value>,
    },
    /// Tee iterator from `itertools.tee` sharing a buffer with sibling iterators.
    ///
    /// Stores the shared tee state ID and this iterator's slot index.
    Tee {
        /// Heap ID of the shared tee state.
        tee_id: HeapId,
        /// Slot index for this iterator in the shared tee state.
        slot: usize,
    },
    /// Line-by-line iterator over a `io.StringIO` object.
    ///
    /// On each `for_next`, calls `readline()` on the StringIO and returns the line.
    /// When `readline()` returns an empty string, the iterator is exhausted.
    StringIORef {
        /// Heap ID of the `StdlibObject::StringIO` on the heap.
        heap_id: HeapId,
    },
    /// Line-by-line iterator over an `io.BytesIO` object.
    ///
    /// Delegates to `BytesIO.__next__()` so iteration semantics and errors match
    /// the object's own methods.
    BytesIORef {
        /// Heap ID of the `StdlibObject::BytesIO` on the heap.
        heap_id: HeapId,
    },
    /// Iterator over a `csv.reader` object.
    CsvReaderRef {
        /// Heap ID of `StdlibObject::CsvReader`.
        heap_id: HeapId,
    },
    /// Iterator over a `csv.DictReader` object.
    CsvDictReaderRef {
        /// Heap ID of `StdlibObject::CsvDictReader`.
        heap_id: HeapId,
    },
}

impl IterValue {
    fn new(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<Self> {
        match &value {
            Value::InternString(string_id) => Some(Self::from_str(interns.get_str(*string_id))),
            Value::InternBytes(bytes_id) => Some(Self::from_intern_bytes(*bytes_id, interns)),
            Value::Ref(heap_id) => Self::from_heap_data(*heap_id, heap),
            _ => None,
        }
    }

    /// Creates a Range iterator value.
    fn from_range(range: &Range) -> Self {
        Self::Range {
            next: range.start,
            step: range.step,
            len: range.len(),
        }
    }

    /// Creates an iterator value over a string.
    ///
    /// Copies the string content and counts characters for the length field.
    fn from_str(s: &str) -> Self {
        let is_ascii = s.is_ascii();
        let len = if is_ascii { s.len() } else { s.chars().count() };
        Self::IterStr {
            string: s.to_owned(),
            byte_offset: 0,
            len,
            is_ascii,
        }
    }

    /// Creates an iterator value over interned bytes.
    fn from_intern_bytes(bytes_id: BytesId, interns: &Interns) -> Self {
        let bytes = interns.get_bytes(bytes_id);
        Self::InternBytes {
            bytes_id,
            len: bytes.len(),
        }
    }

    /// Creates an iterator value from heap data.
    fn from_heap_data(heap_id: HeapId, heap: &Heap<impl ResourceTracker>) -> Option<Self> {
        match heap.get(heap_id) {
            // List: no captured len (checked dynamically), no mutation check
            HeapData::List(_) => Some(Self::HeapRef {
                heap_id,
                len: None,
                checks_mutation: false,
            }),
            // Deque: no captured len (checked dynamically), no mutation check (like List)
            HeapData::Deque(_) => Some(Self::HeapRef {
                heap_id,
                len: None,
                checks_mutation: false,
            }),
            // Tuple/NamedTuple/Bytes/FrozenSet: captured len, no mutation check
            HeapData::Tuple(tuple) => Some(Self::HeapRef {
                heap_id,
                len: Some(tuple.as_vec().len()),
                checks_mutation: false,
            }),
            HeapData::NamedTuple(namedtuple) => Some(Self::HeapRef {
                heap_id,
                len: Some(namedtuple.len()),
                checks_mutation: false,
            }),
            HeapData::Bytes(b) => Some(Self::HeapRef {
                heap_id,
                len: Some(b.len()),
                checks_mutation: false,
            }),
            // Bytearray: no captured len (can be mutated), no mutation check
            HeapData::Bytearray(b) => Some(Self::HeapRef {
                heap_id,
                len: Some(b.len()),
                checks_mutation: false,
            }),
            HeapData::FrozenSet(frozenset) => Some(Self::HeapRef {
                heap_id,
                len: Some(frozenset.len()),
                checks_mutation: false,
            }),
            // Dict/Set/DefaultDict/Counter/OrderedDict: captured len, WITH mutation check
            HeapData::Dict(dict) => Some(Self::HeapRef {
                heap_id,
                len: Some(dict.len()),
                checks_mutation: true,
            }),
            HeapData::DefaultDict(dd) => Some(Self::HeapRef {
                heap_id,
                len: Some(dd.len()),
                checks_mutation: true,
            }),
            HeapData::Counter(counter) => Some(Self::HeapRef {
                heap_id,
                len: Some(counter.len()),
                checks_mutation: true,
            }),
            HeapData::OrderedDict(od) => Some(Self::HeapRef {
                heap_id,
                len: Some(od.dict().len()),
                checks_mutation: true,
            }),
            HeapData::ChainMap(chain_map) => Some(Self::HeapRef {
                heap_id,
                len: Some(chain_map.flat().len()),
                checks_mutation: true,
            }),
            HeapData::Set(set) => Some(Self::HeapRef {
                heap_id,
                len: Some(set.len()),
                checks_mutation: true,
            }),
            // String: copy content for iteration
            HeapData::Str(s) => Some(Self::from_str(s.as_str())),
            // Range: copy values for iteration
            HeapData::Range(range) => Some(Self::from_range(range)),
            // StringIO: line-by-line iteration using readline()
            HeapData::StdlibObject(StdlibObject::StringIO(_)) => Some(Self::StringIORef { heap_id }),
            // BytesIO: line-by-line iteration using __next__()
            HeapData::StdlibObject(StdlibObject::BytesIO(_)) => Some(Self::BytesIORef { heap_id }),
            // csv.reader and csv.DictReader expose iterator semantics.
            HeapData::StdlibObject(StdlibObject::CsvReader(_)) => Some(Self::CsvReaderRef { heap_id }),
            HeapData::StdlibObject(StdlibObject::CsvDictReader(_)) => Some(Self::CsvDictReaderRef { heap_id }),
            // Remaining StdlibObjects are not iterable
            HeapData::StdlibObject(_) => None,
            // Dict views: iterate with mutation check (like the underlying dict)
            HeapData::DictKeys(dk) => {
                // Get the length from the underlying dict
                let len = dk.get_dict(heap).map(super::dict::Dict::len);
                Some(Self::HeapRef {
                    heap_id,
                    len,
                    checks_mutation: true,
                })
            }
            HeapData::DictValues(dv) => {
                let len = dv.get_dict(heap).map(super::dict::Dict::len);
                Some(Self::HeapRef {
                    heap_id,
                    len,
                    checks_mutation: true,
                })
            }
            HeapData::DictItems(di) => {
                let len = di.get_dict(heap).map(super::dict::Dict::len);
                Some(Self::HeapRef {
                    heap_id,
                    len,
                    checks_mutation: true,
                })
            }
            // Closures, FunctionDefaults, Cells, Exceptions, Dataclasses, Iterators, LongInts, Slices, Modules,
            // Paths, ClassObjects, Instances, and async types are not iterable
            HeapData::Closure(_, _, _)
            | HeapData::FunctionDefaults(_, _)
            | HeapData::Cell(_)
            | HeapData::Exception(_)
            | HeapData::Dataclass(_)
            | HeapData::Iter(_)
            | HeapData::LongInt(_)
            | HeapData::Slice(_)
            | HeapData::Module(_)
            | HeapData::Path(_)
            | HeapData::Coroutine(_)
            | HeapData::GatherFuture(_)
            | HeapData::ClassObject(_)
            | HeapData::Instance(_)
            | HeapData::SuperProxy(_)
            | HeapData::StaticMethod(_)
            | HeapData::ClassMethod(_)
            | HeapData::MappingProxy(_)
            | HeapData::SlotDescriptor(_)
            | HeapData::BoundMethod(_)
            | HeapData::UserProperty(_)
            | HeapData::PropertyAccessor(_)
            | HeapData::GenericAlias(_)
            | HeapData::WeakRef(_)
            | HeapData::ClassSubclasses(_)
            | HeapData::ClassGetItem(_)
            | HeapData::FunctionGet(_)
            | HeapData::Hash(_)
            | HeapData::ZlibCompress(_)
            | HeapData::ZlibDecompress(_)
            | HeapData::Partial(_)
            | HeapData::CmpToKey(_)
            | HeapData::ItemGetter(_)
            | HeapData::AttrGetter(_)
            | HeapData::MethodCaller(_)
            | HeapData::NamedTupleFactory(_)
            | HeapData::LruCache(_)
            | HeapData::FunctionWrapper(_)
            | HeapData::Wraps(_)
            | HeapData::TotalOrderingMethod(_)
            | HeapData::CachedProperty(_)
            | HeapData::SingleDispatch(_)
            | HeapData::SingleDispatchRegister(_)
            | HeapData::SingleDispatchMethod(_)
            | HeapData::PartialMethod(_)
            | HeapData::Placeholder(_)
            | HeapData::TextWrapper(_)
            | HeapData::ReMatch(_)
            | HeapData::RePattern(_)
            | HeapData::Tee(_)
            | HeapData::Generator(_)
            | HeapData::Timedelta(_)
            | HeapData::Date(_)
            | HeapData::Datetime(_)
            | HeapData::Time(_)
            | HeapData::Timezone(_)
            | HeapData::Decimal(_)
            | HeapData::Fraction(_)
            | HeapData::Uuid(_)
            | HeapData::SafeUuid(_)
            | HeapData::ObjectNewImpl(_) => None,
        }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for OurosIter {
    #[inline]
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        Self::drop_with_heap(self, heap);
    }
}
