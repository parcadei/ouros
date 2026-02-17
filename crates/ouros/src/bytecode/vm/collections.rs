//! Collection building and unpacking helpers for the VM.

use smallvec::SmallVec;

use super::{CallResult, PendingUnpack, VM};
use crate::{
    exception_private::{ExcType, RunError, SimpleException},
    heap::{DropWithHeap, HeapData, HeapId},
    intern::StringId,
    io::PrintWriter,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{
        Dict, List, OurosIter, PyTrait, Set, Slice, Type, allocate_tuple, iter::advance_on_heap,
        slice::value_to_option_i64, str::allocate_char,
    },
    value::Value,
};

impl<T: ResourceTracker, P: PrintWriter, Tr: VmTracer> VM<'_, T, P, Tr> {
    /// Builds a list from the top n stack values.
    pub(super) fn build_list(&mut self, count: usize) -> Result<(), RunError> {
        let items = self.pop_n(count);
        let list = List::new(items);
        let heap_id = self.heap.allocate(HeapData::List(list))?;
        self.push(Value::Ref(heap_id));
        Ok(())
    }

    /// Builds an empty list with a pre-allocation hint.
    ///
    /// This is used by list-comprehension setup when the compiler can infer an
    /// exact iteration count from literal `range(...)` generators.
    pub(super) fn build_list_with_hint(&mut self, hint: &Value) -> Result<(), RunError> {
        let capacity = match hint {
            Value::Int(i) => usize::try_from(*i).unwrap_or_default(),
            _ => 0,
        };
        let list = List::new(Vec::with_capacity(capacity));
        let heap_id = self.heap.allocate(HeapData::List(list))?;
        self.push(Value::Ref(heap_id));
        Ok(())
    }

    /// Builds a tuple from the top n stack values.
    ///
    /// Uses the empty tuple singleton when count is 0, and SmallVec
    /// optimization for small tuples (≤2 elements).
    pub(super) fn build_tuple(&mut self, count: usize) -> Result<(), RunError> {
        match count {
            0 => {
                // Avoid pop_n(0) allocation in tight empty-tuple loops.
                let heap_id = self.heap.get_or_create_empty_tuple()?;
                self.push(Value::Ref(heap_id));
                Ok(())
            }
            1 => {
                let item = self.pop();
                let mut items = SmallVec::with_capacity(1);
                items.push(item);
                let value = allocate_tuple(items, self.heap)?;
                self.push(value);
                Ok(())
            }
            2 => {
                // Preserve stack order: [..., a, b] -> (a, b)
                let second = self.pop();
                let first = self.pop();
                let mut items = SmallVec::with_capacity(2);
                items.push(first);
                items.push(second);
                let value = allocate_tuple(items, self.heap)?;
                self.push(value);
                Ok(())
            }
            _ => {
                let items = self.pop_n(count);
                let value = allocate_tuple(items.into(), self.heap)?;
                self.push(value);
                Ok(())
            }
        }
    }

    /// Builds a dict from the top 2n stack values (key/value pairs).
    pub(super) fn build_dict(&mut self, count: usize) -> Result<(), RunError> {
        let items = self.pop_n(count * 2);
        let mut dict = Dict::new();
        // Use into_iter to consume items by value, avoiding clone and proper ownership transfer
        let mut iter = items.into_iter();
        while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
            dict.set(key, value, self.heap, self.interns)?;
        }
        let heap_id = self.heap.allocate(HeapData::Dict(dict))?;
        self.push(Value::Ref(heap_id));
        Ok(())
    }

    /// Builds a set from the top n stack values.
    pub(super) fn build_set(&mut self, count: usize) -> Result<(), RunError> {
        let items = self.pop_n(count);
        let mut set = Set::new();
        for item in items {
            set.add(item, self.heap, self.interns)?;
        }
        let heap_id = self.heap.allocate(HeapData::Set(set))?;
        self.push(Value::Ref(heap_id));
        Ok(())
    }

    /// Builds a slice object from the top 3 stack values.
    ///
    /// Stack: [start, stop, step] -> [slice]
    /// Each value can be None (for default) or an integer.
    pub(super) fn build_slice(&mut self) -> Result<(), RunError> {
        let step_val = self.pop();
        let stop_val = self.pop();
        let start_val = self.pop();

        // Store results before dropping to avoid refcount leak on error
        let start = value_to_option_i64(&start_val);
        let stop = value_to_option_i64(&stop_val);
        let step = value_to_option_i64(&step_val);

        // Drop the values after extracting their integer content
        start_val.drop_with_heap(self.heap);
        stop_val.drop_with_heap(self.heap);
        step_val.drop_with_heap(self.heap);

        let slice = Slice::new(start?, stop?, step?);
        let heap_id = self.heap.allocate(HeapData::Slice(slice))?;
        self.push(Value::Ref(heap_id));
        Ok(())
    }

    /// Extends a list with items from an iterable.
    ///
    /// Stack: [list, iterable] -> [list]
    /// Pops the iterable, extends the list in place, leaves list on stack.
    pub(super) fn list_extend(&mut self) -> Result<(), RunError> {
        let iterable = self.pop();
        let list_ref = self.pop();

        // Two-phase approach to avoid borrow conflicts:
        // Phase 1: Copy items without refcount changes
        let copied_items: Vec<Value> = match &iterable {
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::List(list) => list.as_vec().iter().map(Value::copy_for_extend).collect(),
                HeapData::Tuple(tuple) => tuple.as_vec().iter().map(Value::copy_for_extend).collect(),
                HeapData::NamedTuple(tuple) => tuple.as_vec().iter().map(Value::copy_for_extend).collect(),
                HeapData::Set(set) => set.storage().iter().map(Value::copy_for_extend).collect(),
                HeapData::Dict(dict) => dict.iter().map(|(k, _)| Value::copy_for_extend(k)).collect(),
                HeapData::Counter(counter) => counter.dict().iter().map(|(k, _)| Value::copy_for_extend(k)).collect(),
                HeapData::OrderedDict(od) => od.dict().iter().map(|(k, _)| Value::copy_for_extend(k)).collect(),
                HeapData::Str(s) => {
                    // Need to allocate strings for each character
                    let chars: Vec<char> = s.as_str().chars().collect();
                    let mut items = Vec::with_capacity(chars.len());
                    for c in chars {
                        items.push(allocate_char(c, self.heap)?);
                    }
                    items
                }
                _ => {
                    let type_ = iterable.py_type(self.heap);
                    iterable.drop_with_heap(self.heap);
                    list_ref.drop_with_heap(self.heap);
                    return Err(ExcType::type_error_not_iterable(type_));
                }
            },
            Value::InternString(id) => {
                let s = self.interns.get_str(*id);
                let chars: Vec<char> = s.chars().collect();
                let mut items = Vec::with_capacity(chars.len());
                for c in chars {
                    items.push(allocate_char(c, self.heap)?);
                }
                items
            }
            _ => {
                let type_ = iterable.py_type(self.heap);
                iterable.drop_with_heap(self.heap);
                list_ref.drop_with_heap(self.heap);
                return Err(ExcType::type_error_not_iterable(type_));
            }
        };

        // Phase 2: Increment refcounts now that the borrow has ended
        for item in &copied_items {
            if let Value::Ref(id) = item {
                self.heap.inc_ref(*id);
            }
        }

        // Check if any copied items are refs (for updating contains_refs)
        let has_refs = copied_items.iter().any(|v| matches!(v, Value::Ref(_)));

        // Extend the list
        if let Value::Ref(id) = &list_ref
            && let HeapData::List(list) = self.heap.get_mut(*id)
        {
            // Update contains_refs before extending
            if has_refs {
                list.set_contains_refs();
            }
            list.as_vec_mut().extend(copied_items);
        }

        // Mark potential cycle after the mutable borrow ends
        if has_refs {
            self.heap.mark_potential_cycle();
        }

        iterable.drop_with_heap(self.heap);
        self.push(list_ref);
        Ok(())
    }

    /// Converts a list to a tuple.
    ///
    /// Stack: [list] -> [tuple]
    pub(super) fn list_to_tuple(&mut self) -> Result<(), RunError> {
        let list_ref = self.pop();

        // Phase 1: Copy items without refcount changes
        let copied_items: SmallVec<_> = if let Value::Ref(id) = &list_ref {
            if let HeapData::List(list) = self.heap.get(*id) {
                list.as_vec().iter().map(Value::copy_for_extend).collect()
            } else {
                return Err(RunError::internal("ListToTuple: expected list"));
            }
        } else {
            return Err(RunError::internal("ListToTuple: expected list ref"));
        };

        // Phase 2: Increment refcounts now that the borrow has ended
        for item in &copied_items {
            if let Value::Ref(id) = item {
                self.heap.inc_ref(*id);
            }
        }

        list_ref.drop_with_heap(self.heap);

        let value = allocate_tuple(copied_items, self.heap)?;
        self.push(value);
        Ok(())
    }

    /// Merges a mapping into a dict for **kwargs unpacking.
    ///
    /// Stack: [dict, mapping] -> [dict]
    /// Validates that mapping is a dict and that keys are strings.
    pub(super) fn dict_merge(&mut self, func_name_id: u16) -> Result<(), RunError> {
        let mapping = self.pop();
        let dict_ref = self.pop();

        // Get function name for error messages
        let func_name = if func_name_id == 0xFFFF {
            "<unknown>".to_string()
        } else {
            self.interns.get_str(StringId::from_index(func_name_id)).to_string()
        };

        // Two-phase approach: copy items first, then inc refcounts
        // Phase 1: Copy key-value pairs without refcount changes
        // Check that mapping is a dict (Ref pointing to Dict)
        let copied_items: Vec<(Value, Value)> = if let Value::Ref(id) = &mapping {
            if let HeapData::Dict(dict) = self.heap.get(*id) {
                dict.iter()
                    .map(|(k, v)| (Value::copy_for_extend(k), Value::copy_for_extend(v)))
                    .collect()
            } else {
                let type_name = mapping.py_type(self.heap).to_string();
                mapping.drop_with_heap(self.heap);
                dict_ref.drop_with_heap(self.heap);
                return Err(ExcType::type_error_kwargs_not_mapping(&func_name, &type_name));
            }
        } else {
            let type_name = mapping.py_type(self.heap).to_string();
            mapping.drop_with_heap(self.heap);
            dict_ref.drop_with_heap(self.heap);
            return Err(ExcType::type_error_kwargs_not_mapping(&func_name, &type_name));
        };

        // Phase 2: Increment refcounts now that the borrow has ended
        for (key, value) in &copied_items {
            if let Value::Ref(id) = key {
                self.heap.inc_ref(*id);
            }
            if let Value::Ref(id) = value {
                self.heap.inc_ref(*id);
            }
        }

        // Merge into the dict, validating string keys
        let dict_id = if let Value::Ref(id) = &dict_ref {
            *id
        } else {
            mapping.drop_with_heap(self.heap);
            dict_ref.drop_with_heap(self.heap);
            return Err(RunError::internal("DictMerge: expected dict ref"));
        };

        for (key, value) in copied_items {
            // Validate key is a string (InternString or heap-allocated Str)
            let is_string = match &key {
                Value::InternString(_) => true,
                Value::Ref(id) => matches!(self.heap.get(*id), HeapData::Str(_)),
                _ => false,
            };
            if !is_string {
                key.drop_with_heap(self.heap);
                value.drop_with_heap(self.heap);
                mapping.drop_with_heap(self.heap);
                dict_ref.drop_with_heap(self.heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            }

            // Get the string key for error messages (needed before moving key into closure)
            let key_str = match &key {
                Value::InternString(id) => self.interns.get_str(*id).to_string(),
                Value::Ref(id) => {
                    if let HeapData::Str(s) = self.heap.get(*id) {
                        s.as_str().to_string()
                    } else {
                        "<unknown>".to_string()
                    }
                }
                _ => "<unknown>".to_string(),
            };

            // Use with_entry_mut to avoid borrow conflict: takes data out temporarily
            let result = self.heap.with_entry_mut(dict_id, |heap, data| {
                if let HeapData::Dict(dict) = data {
                    dict.set(key, value, heap, self.interns)
                } else {
                    Err(RunError::internal("DictMerge: entry is not a Dict"))
                }
            });

            // If set returned Some, the key already existed (duplicate kwarg)
            if let Some(old_value) = result? {
                old_value.drop_with_heap(self.heap);
                mapping.drop_with_heap(self.heap);
                dict_ref.drop_with_heap(self.heap);
                return Err(ExcType::type_error_multiple_values(&func_name, &key_str));
            }
        }

        mapping.drop_with_heap(self.heap);
        self.push(dict_ref);
        Ok(())
    }

    /// Merges a mapping into a dict for dict-literal unpacking (`{**mapping}`).
    ///
    /// Stack: [dict, mapping] -> [dict]
    /// Unlike `dict_merge()` (used for `**kwargs`), this accepts arbitrary hashable
    /// keys and silently overwrites duplicates, matching Python dict literal behavior.
    pub(super) fn dict_update(&mut self) -> Result<(), RunError> {
        let mapping = self.pop();
        let dict_ref = self.pop();

        // Two-phase approach: copy items first, then inc refcounts.
        let copied_items: Vec<(Value, Value)> = if let Value::Ref(id) = &mapping {
            if let HeapData::Dict(dict) = self.heap.get(*id) {
                dict.iter()
                    .map(|(k, v)| (Value::copy_for_extend(k), Value::copy_for_extend(v)))
                    .collect()
            } else {
                let type_name = mapping.py_type(self.heap);
                mapping.drop_with_heap(self.heap);
                dict_ref.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!("'{type_name}' object is not a mapping")));
            }
        } else {
            let type_name = mapping.py_type(self.heap);
            mapping.drop_with_heap(self.heap);
            dict_ref.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!("'{type_name}' object is not a mapping")));
        };

        // Increment refcounts now that immutable borrows have ended.
        for (key, value) in &copied_items {
            if let Value::Ref(id) = key {
                self.heap.inc_ref(*id);
            }
            if let Value::Ref(id) = value {
                self.heap.inc_ref(*id);
            }
        }

        let dict_id = if let Value::Ref(id) = &dict_ref {
            *id
        } else {
            mapping.drop_with_heap(self.heap);
            dict_ref.drop_with_heap(self.heap);
            return Err(RunError::internal("DictUpdate: expected dict ref"));
        };

        for (key, value) in copied_items {
            let old_value = self.heap.with_entry_mut(dict_id, |heap, data| {
                if let HeapData::Dict(dict) = data {
                    dict.set(key, value, heap, self.interns)
                } else {
                    Err(RunError::internal("DictUpdate: entry is not a Dict"))
                }
            })?;
            if let Some(old) = old_value {
                old.drop_with_heap(self.heap);
            }
        }

        mapping.drop_with_heap(self.heap);
        self.push(dict_ref);
        Ok(())
    }

    // ========================================================================
    // Comprehension Building
    // ========================================================================

    /// Appends TOS to list for comprehension.
    ///
    /// Stack: [..., list, iter1, ..., iterN, value] -> [..., list, iter1, ..., iterN]
    /// The `depth` parameter is the number of iterators between the list and the value.
    /// List is at stack position: len - 2 - depth (0-indexed from bottom).
    pub(super) fn list_append(&mut self, depth: usize) -> Result<(), RunError> {
        let value = self.pop();
        let stack_len = self.stack.len();
        let list_pos = stack_len - 1 - depth;

        // Get the list reference
        let Value::Ref(list_id) = self.stack[list_pos] else {
            value.drop_with_heap(self.heap);
            return Err(RunError::internal("ListAppend: expected list ref on stack"));
        };

        if let Err(err) = self.heap.on_container_insert() {
            value.drop_with_heap(self.heap);
            return Err(err.into());
        }

        let value_is_ref = matches!(value, Value::Ref(_));
        {
            let HeapData::List(list) = self.heap.get_mut(list_id) else {
                value.drop_with_heap(self.heap);
                return Err(RunError::internal("ListAppend: expected list on heap"));
            };
            if value_is_ref {
                list.set_contains_refs();
            }
            list.as_vec_mut().push(value);
        }
        if value_is_ref {
            self.heap.mark_potential_cycle();
        }
        Ok(())
    }

    /// Adds TOS to set for comprehension.
    ///
    /// Stack: [..., set, iter1, ..., iterN, value] -> [..., set, iter1, ..., iterN]
    /// The `depth` parameter is the number of iterators between the set and the value.
    /// May raise TypeError if value is unhashable.
    pub(super) fn set_add(&mut self, depth: usize) -> Result<(), RunError> {
        let value = self.pop();
        let stack_len = self.stack.len();
        let set_pos = stack_len - 1 - depth;

        // Get the set reference
        let Value::Ref(set_id) = self.stack[set_pos] else {
            value.drop_with_heap(self.heap);
            return Err(RunError::internal("SetAdd: expected set ref on stack"));
        };

        if let Err(err) = self.heap.on_container_insert() {
            value.drop_with_heap(self.heap);
            return Err(err.into());
        }

        // Add to the set using with_entry_mut to avoid borrow conflicts
        self.heap.with_entry_mut(set_id, |heap, data| {
            if let HeapData::Set(set) = data {
                set.add(value, heap, self.interns)
            } else {
                value.drop_with_heap(heap);
                Err(RunError::internal("SetAdd: expected set on heap"))
            }
        })?;

        Ok(())
    }

    /// Sets dict[key] = value for comprehension.
    ///
    /// Stack: [..., dict, iter1, ..., iterN, key, value] -> [..., dict, iter1, ..., iterN]
    /// The `depth` parameter is the number of iterators between the dict and the key-value pair.
    /// May raise TypeError if key is unhashable.
    pub(super) fn dict_set_item(&mut self, depth: usize) -> Result<(), RunError> {
        let value = self.pop();
        let key = self.pop();
        let stack_len = self.stack.len();
        let dict_pos = stack_len - 1 - depth;

        // Get the dict reference
        let Value::Ref(dict_id) = self.stack[dict_pos] else {
            key.drop_with_heap(self.heap);
            value.drop_with_heap(self.heap);
            return Err(RunError::internal("DictSetItem: expected dict ref on stack"));
        };

        if let Err(err) = self.heap.on_container_insert() {
            key.drop_with_heap(self.heap);
            value.drop_with_heap(self.heap);
            return Err(err.into());
        }

        // Set item in the dict using with_entry_mut to avoid borrow conflicts
        let old_value = self.heap.with_entry_mut(dict_id, |heap, data| {
            if let HeapData::Dict(dict) = data {
                dict.set(key, value, heap, self.interns)
            } else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                Err(RunError::internal("DictSetItem: expected dict on heap"))
            }
        })?;

        // Drop old value if key already existed
        if let Some(old) = old_value {
            old.drop_with_heap(self.heap);
        }

        Ok(())
    }

    // ========================================================================
    // Unpacking
    // ========================================================================

    /// Unpacks an iterable into exactly `count` values on the stack.
    ///
    /// Returns `true` when execution suspended because generator-backed iteration
    /// pushed a frame and unpacking was deferred, `false` when unpacking completed
    /// synchronously.
    pub(super) fn unpack_sequence(&mut self, count: usize) -> Result<bool, RunError> {
        let value = self.pop();
        if let Value::Ref(generator_id) = &value
            && matches!(self.heap.get(*generator_id), HeapData::Generator(_))
        {
            return self.unpack_from_generator(value, PendingUnpack::Sequence { count });
        }

        self.unpack_sequence_value(value, count)?;
        Ok(false)
    }

    /// Unpacks exactly `count` items from a provided iterable value.
    ///
    /// The input value is consumed (and dropped on all paths).
    fn unpack_sequence_value(&mut self, value: Value, count: usize) -> Result<(), RunError> {
        if let Value::Ref(iter_id) = &value
            && matches!(self.heap.get(*iter_id), HeapData::Iter(_))
        {
            return self.unpack_sequence_from_heap_iter(*iter_id, value, count);
        }

        let mut iter = self.unpack_iter_from_value(value)?;
        let mut items = Vec::with_capacity(count);

        for _ in 0..count {
            match iter.for_next(self.heap, self.interns) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => {
                    let actual = items.len();
                    iter.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    return Err(unpack_size_error(count, actual));
                }
                Err(err) => {
                    iter.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    return Err(err);
                }
            }
        }

        // Probe one extra element to detect "too many values" without fully exhausting
        // potentially-infinite iterables.
        match iter.for_next(self.heap, self.interns) {
            Ok(Some(extra)) => {
                extra.drop_with_heap(self.heap);
                let remaining = iter.size_hint(self.heap);
                let actual = if remaining == usize::MAX {
                    count + 1
                } else {
                    count.saturating_add(1).saturating_add(remaining)
                };
                iter.drop_with_heap(self.heap);
                for item in items {
                    item.drop_with_heap(self.heap);
                }
                return Err(unpack_size_error(count, actual));
            }
            Ok(None) => {}
            Err(err) => {
                iter.drop_with_heap(self.heap);
                for item in items {
                    item.drop_with_heap(self.heap);
                }
                return Err(err);
            }
        }
        iter.drop_with_heap(self.heap);

        // Push items in reverse order so first item is on top
        for item in items.into_iter().rev() {
            self.push(item);
        }
        Ok(())
    }

    /// Unpacks an iterable with a starred target.
    ///
    /// `before` is the number of targets before the star, `after` is the number after.
    /// The starred target collects all middle items into a list.
    ///
    /// For example, `first, *rest, last = [1, 2, 3, 4, 5]` has before=1, after=1.
    /// After execution, the stack has: first (top), rest_list, last.
    pub(super) fn unpack_ex(&mut self, before: usize, after: usize) -> Result<bool, RunError> {
        let value = self.pop();
        if let Value::Ref(generator_id) = &value
            && matches!(self.heap.get(*generator_id), HeapData::Generator(_))
        {
            return self.unpack_from_generator(value, PendingUnpack::Extended { before, after });
        }

        self.unpack_ex_value(value, before, after)?;
        Ok(false)
    }

    /// Unpacks a value for `UNPACK_EX` semantics (`before`, `*rest`, `after`).
    ///
    /// The input value is consumed (and dropped on all paths).
    fn unpack_ex_value(&mut self, value: Value, before: usize, after: usize) -> Result<(), RunError> {
        if let Value::Ref(iter_id) = &value
            && matches!(self.heap.get(*iter_id), HeapData::Iter(_))
        {
            return self.unpack_ex_from_heap_iter(*iter_id, value, before, after);
        }

        let min_items = before + after;
        let mut iter = self.unpack_iter_from_value(value)?;
        let mut items = Vec::new();

        loop {
            match iter.for_next(self.heap, self.interns) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => break,
                Err(err) => {
                    iter.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    return Err(err);
                }
            }
        }
        iter.drop_with_heap(self.heap);

        if items.len() < min_items {
            let actual = items.len();
            for item in items {
                item.drop_with_heap(self.heap);
            }
            return Err(unpack_ex_too_few_error(min_items, actual));
        }

        self.push_unpack_ex_results(&items, before, after)?;
        Ok(())
    }

    /// Defers unpacking for a generator by materializing it with list-building logic.
    ///
    /// This reuses VM-managed generator iteration (`list_build_from_iterator`) so
    /// frame suspension/resume continues to work exactly like other generator consumers.
    fn unpack_from_generator(&mut self, generator: Value, pending: PendingUnpack) -> Result<bool, RunError> {
        match self.list_build_from_iterator(generator)? {
            CallResult::Push(list_value) => {
                self.resume_pending_unpack_with_list_value(pending, list_value)?;
                Ok(false)
            }
            CallResult::FramePushed => {
                self.pending_unpack = Some(pending);
                Ok(true)
            }
            other => Err(RunError::internal(format!(
                "list_build_from_iterator returned unsupported result for unpacking: {other:?}"
            ))),
        }
    }

    /// Resumes a deferred generator unpack once list materialization completed.
    ///
    /// The list value is consumed and unpacked according to the pending mode.
    pub(super) fn resume_pending_unpack(&mut self, list_value: Value) -> Result<(), RunError> {
        let Some(pending) = self.pending_unpack.take() else {
            list_value.drop_with_heap(self.heap);
            return Err(RunError::internal(
                "resume_pending_unpack called without pending unpack state",
            ));
        };
        self.resume_pending_unpack_with_list_value(pending, list_value)
    }

    /// Applies unpacking mode to an already-materialized list-like value.
    fn resume_pending_unpack_with_list_value(
        &mut self,
        pending: PendingUnpack,
        list_value: Value,
    ) -> Result<(), RunError> {
        match pending {
            PendingUnpack::Sequence { count } => self.unpack_sequence_value(list_value, count),
            PendingUnpack::Extended { before, after } => self.unpack_ex_value(list_value, before, after),
        }
    }

    /// Creates an iterator for unpacking and remaps non-iterable errors to unpack wording.
    fn unpack_iter_from_value(&mut self, value: Value) -> Result<OurosIter, RunError> {
        let type_name = value.py_type(self.heap);
        match OurosIter::new(value, self.heap, self.interns) {
            Ok(iter) => Ok(iter),
            Err(err) if is_not_iterable_type_error(&err, type_name) => Err(unpack_type_error(type_name)),
            Err(err) => Err(err),
        }
    }

    /// Unpacks from an existing heap iterator (`HeapData::Iter`) in-place.
    ///
    /// This is required for values produced by `iter(...)` and `itertools.tee(...)`,
    /// which should be consumed directly during unpacking.
    fn unpack_sequence_from_heap_iter(
        &mut self,
        iter_id: HeapId,
        iter_value: Value,
        count: usize,
    ) -> Result<(), RunError> {
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            match advance_on_heap(self.heap, iter_id, self.interns) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => {
                    let actual = items.len();
                    iter_value.drop_with_heap(self.heap);
                    items.drop_with_heap(self.heap);
                    return Err(unpack_size_error(count, actual));
                }
                Err(err) => {
                    iter_value.drop_with_heap(self.heap);
                    items.drop_with_heap(self.heap);
                    return Err(err);
                }
            }
        }

        match advance_on_heap(self.heap, iter_id, self.interns) {
            Ok(Some(extra)) => {
                extra.drop_with_heap(self.heap);
                let remaining = match self.heap.get(iter_id) {
                    HeapData::Iter(iter) => iter.size_hint(self.heap),
                    _ => usize::MAX,
                };
                let actual = if remaining == usize::MAX {
                    count + 1
                } else {
                    count.saturating_add(1).saturating_add(remaining)
                };
                iter_value.drop_with_heap(self.heap);
                items.drop_with_heap(self.heap);
                return Err(unpack_size_error(count, actual));
            }
            Ok(None) => {}
            Err(err) => {
                iter_value.drop_with_heap(self.heap);
                items.drop_with_heap(self.heap);
                return Err(err);
            }
        }

        iter_value.drop_with_heap(self.heap);
        for item in items.into_iter().rev() {
            self.push(item);
        }
        Ok(())
    }

    /// Star-unpacks from an existing heap iterator (`HeapData::Iter`) in-place.
    fn unpack_ex_from_heap_iter(
        &mut self,
        iter_id: HeapId,
        iter_value: Value,
        before: usize,
        after: usize,
    ) -> Result<(), RunError> {
        let min_items = before + after;
        let mut items = Vec::new();
        loop {
            match advance_on_heap(self.heap, iter_id, self.interns) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => break,
                Err(err) => {
                    iter_value.drop_with_heap(self.heap);
                    items.drop_with_heap(self.heap);
                    return Err(err);
                }
            }
        }
        iter_value.drop_with_heap(self.heap);

        if items.len() < min_items {
            let actual = items.len();
            items.drop_with_heap(self.heap);
            return Err(unpack_ex_too_few_error(min_items, actual));
        }

        self.push_unpack_ex_results(&items, before, after)?;
        Ok(())
    }

    /// Helper to push unpacked items with starred target onto the stack.
    ///
    /// Takes a slice of items and creates the middle list.
    fn push_unpack_ex_results(&mut self, items: &[Value], before: usize, after: usize) -> Result<(), RunError> {
        let total = items.len();

        // Collect results: before items, middle list, after items
        let mut results = Vec::with_capacity(before + 1 + after);

        // Before items
        for item in items.iter().take(before) {
            results.push(item.copy_for_extend());
        }

        // Middle items as a list (starred target).
        // Items produced by `OurosIter` already carry an owned reference (or are immediate
        // values), so no additional refcount bump is needed here.
        let middle_start = before;
        let middle_end = total - after;
        let mut middle = Vec::with_capacity(middle_end - middle_start);
        for item in &items[middle_start..middle_end] {
            middle.push(item.copy_for_extend());
        }
        let middle_list = List::new(middle);
        let list_id = self.heap.allocate(HeapData::List(middle_list))?;
        results.push(Value::Ref(list_id));

        // After items
        for item in items.iter().skip(total - after) {
            results.push(item.copy_for_extend());
        }

        // Push in reverse order so first item is on top
        for item in results.into_iter().rev() {
            self.push(item);
        }

        Ok(())
    }
}

/// Creates the ValueError for star unpacking when there are too few values.
fn unpack_ex_too_few_error(min_needed: usize, actual: usize) -> RunError {
    let message = format!("not enough values to unpack (expected at least {min_needed}, got {actual})");
    SimpleException::new_msg(ExcType::ValueError, message).into()
}

/// Creates the appropriate ValueError for unpacking size mismatches.
///
/// Python uses different messages depending on whether there are too few or too many values:
/// - Too few: "not enough values to unpack (expected X, got Y)"
/// - Too many: "too many values to unpack (expected X, got Y)"
fn unpack_size_error(expected: usize, actual: usize) -> RunError {
    let message = if actual < expected {
        format!("not enough values to unpack (expected {expected}, got {actual})")
    } else {
        format!("too many values to unpack (expected {expected}, got {actual})")
    };
    SimpleException::new_msg(ExcType::ValueError, message).into()
}

/// Creates a TypeError for attempting to unpack a non-iterable type.
fn unpack_type_error(type_name: Type) -> RunError {
    SimpleException::new_msg(
        ExcType::TypeError,
        format!("cannot unpack non-iterable {type_name} object"),
    )
    .into()
}

/// Returns true if the error is the generic `'{type}' object is not iterable` TypeError.
fn is_not_iterable_type_error(error: &RunError, type_name: Type) -> bool {
    let expected = format!("'{type_name}' object is not iterable");
    matches!(
        error,
        RunError::Exc(exc)
            if exc.exc.exc_type() == ExcType::TypeError
                && exc.exc.arg().is_some_and(|arg| arg == &expected)
    )
}
