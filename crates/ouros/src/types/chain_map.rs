//! Implementation of `collections.ChainMap`.
//!
//! `ChainMap` groups multiple dictionaries into a single mapping view with
//! "first mapping wins" lookup semantics. Writes and deletes affect only the
//! first mapping.

use std::fmt::Write;

use ahash::AHashSet;
use smallvec::SmallVec;

use crate::{
    args::ArgValues,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunError, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, Dict, List, PyTrait, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// A `collections.ChainMap` mapping view.
///
/// `maps` stores the underlying dictionaries in precedence order (index `0`
/// has highest precedence). `flat` is a merged view used for iteration and
/// mapping-copy operations.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ChainMap {
    /// The underlying mappings in lookup order.
    maps: Vec<Value>,
    /// Cached merged dictionary where earlier maps override later maps.
    flat: Dict,
}

impl ChainMap {
    /// Creates a ChainMap from validated dict references.
    ///
    /// `maps` must contain only `Value::Ref` values pointing to `HeapData::Dict`.
    pub fn new(maps: Vec<Value>, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let mut result = Self {
            maps,
            flat: Dict::new(),
        };
        if let Err(err) = result.rebuild_flat(heap, interns) {
            result.maps.drop_with_heap(heap);
            return Err(err);
        }
        Ok(result)
    }

    /// Returns a borrowed view of the underlying map references.
    #[must_use]
    pub fn maps(&self) -> &[Value] {
        &self.maps
    }

    /// Returns a borrowed view of the merged dictionary cache.
    #[must_use]
    pub fn flat(&self) -> &Dict {
        &self.flat
    }

    /// Returns cloned `(key, value)` pairs from the merged dictionary cache.
    pub fn flat_items(&self, heap: &mut Heap<impl ResourceTracker>) -> Vec<(Value, Value)> {
        self.flat.items(heap)
    }

    /// Returns whether this chain map contains any heap references.
    #[must_use]
    pub fn has_refs(&self) -> bool {
        self.flat.has_refs() || self.maps.iter().any(|map| matches!(map, Value::Ref(_)))
    }

    /// Rebuilds the merged dictionary cache from `maps`.
    ///
    /// Later maps are loaded first and earlier maps are applied last so that
    /// earlier maps take precedence.
    pub(crate) fn rebuild_flat(&mut self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
        let mut merged = Dict::new();
        for map in self.maps.iter().rev() {
            let Value::Ref(map_id) = map else {
                merged.drop_all_entries(heap);
                return Err(RunError::internal("ChainMap map entry was not a heap reference"));
            };
            let map_items = heap.with_entry_mut(*map_id, |heap_inner, data| match data {
                HeapData::Dict(dict) => Ok(dict.items(heap_inner)),
                _ => Err(RunError::internal("ChainMap map entry was not a dict")),
            })?;
            for (key, value) in map_items {
                if let Some(old) = merged.set(key, value, heap, interns)? {
                    old.drop_with_heap(heap);
                }
            }
        }

        self.flat.drop_all_entries(heap);
        self.flat = merged;
        Ok(())
    }

    /// Materializes the `.maps` attribute value as a list.
    fn maps_value(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let mut maps = Vec::with_capacity(self.maps.len());
        for map in &self.maps {
            maps.push(map.clone_with_heap(heap));
        }
        let list_id = heap.allocate(HeapData::List(List::new(maps)))?;
        Ok(Value::Ref(list_id))
    }

    /// Materializes the `.parents` attribute value.
    ///
    /// `parents` drops the first mapping. If that would produce an empty map
    /// chain, CPython returns `ChainMap({})`.
    fn parents_value(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
        let mut maps = if self.maps.len() > 1 {
            let mut parent_maps = Vec::with_capacity(self.maps.len() - 1);
            for map in self.maps.iter().skip(1) {
                parent_maps.push(map.clone_with_heap(heap));
            }
            parent_maps
        } else {
            let dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
            vec![Value::Ref(dict_id)]
        };

        let chain = match Self::new(std::mem::take(&mut maps), heap, interns) {
            Ok(chain) => chain,
            Err(err) => {
                maps.drop_with_heap(heap);
                return Err(err);
            }
        };

        let chain_id = heap.allocate(HeapData::ChainMap(chain))?;
        Ok(Value::Ref(chain_id))
    }

    /// Implements `ChainMap.new_child(m=None, **kwargs)`.
    fn new_child(
        &mut self,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Value> {
        let (mut positional, kwargs) = args.into_parts();
        let positional_len = positional.len();
        if positional_len > 1 {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error_at_most("ChainMap.new_child", 1, positional_len));
        }

        let m_arg = positional.next();
        positional.drop_with_heap(heap);
        defer_drop_mut!(m_arg, heap);

        let updates: Vec<(Value, Value)> = Vec::new();
        defer_drop_mut!(updates, heap);
        let kwargs_iter = kwargs.into_iter();
        defer_drop_mut!(kwargs_iter, heap);
        for (key, value) in kwargs_iter.by_ref() {
            let Some(key_name) = key.as_either_str(heap) else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            if key_name.as_str(interns) == "m" {
                if m_arg.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "ChainMap.new_child() got multiple values for argument 'm'",
                    ));
                }
                key.drop_with_heap(heap);
                *m_arg = Some(value);
                continue;
            }
            updates.push((key, value));
        }

        let mut first_map = Dict::new();
        if let Some(m_value) = m_arg.take() {
            let m_id = if let Value::Ref(m_id) = &m_value {
                *m_id
            } else {
                m_value.drop_with_heap(heap);
                return Err(ExcType::type_error("ChainMap.new_child() argument 'm' must be dict"));
            };
            defer_drop!(m_value, heap);
            let source_items = heap.with_entry_mut(m_id, |heap_inner, data| match data {
                HeapData::Dict(dict) => Ok(dict.items(heap_inner)),
                _ => Err(ExcType::type_error("ChainMap.new_child() argument 'm' must be dict")),
            })?;
            let source_items_iter = source_items.into_iter();
            defer_drop_mut!(source_items_iter, heap);
            for (key, value) in source_items_iter.by_ref() {
                if let Some(old) = first_map.set(key, value, heap, interns)? {
                    old.drop_with_heap(heap);
                }
            }
        }

        let updates_iter = std::mem::take(updates).into_iter();
        defer_drop_mut!(updates_iter, heap);
        for (key, value) in updates_iter.by_ref() {
            if let Some(old) = first_map.set(key, value, heap, interns)? {
                old.drop_with_heap(heap);
            }
        }

        let first_map_id = heap.allocate(HeapData::Dict(first_map))?;
        let mut child_maps = Vec::with_capacity(self.maps.len() + 1);
        child_maps.push(Value::Ref(first_map_id));
        for map in &self.maps {
            child_maps.push(map.clone_with_heap(heap));
        }
        let child = Self::new(child_maps, heap, interns)?;
        let child_id = heap.allocate(HeapData::ChainMap(child))?;
        Ok(Value::Ref(child_id))
    }

    /// Returns a list for `.keys()` in ChainMap lookup order.
    fn keys_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let keys = self.flat.keys(heap);
        let list_id = heap.allocate(HeapData::List(List::new(keys)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns a list for `.values()` in ChainMap lookup order.
    fn values_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let values = self.flat.values(heap);
        let list_id = heap.allocate(HeapData::List(List::new(values)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns `(key, value)` tuples for `.items()` in ChainMap lookup order.
    fn items_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let items = self.flat.items(heap);
        let mut tuples = Vec::with_capacity(items.len());
        for (key, value) in items {
            let tuple = allocate_tuple(SmallVec::from_vec(vec![key, value]), heap)?;
            tuples.push(tuple);
        }
        let list_id = heap.allocate(HeapData::List(List::new(tuples)))?;
        Ok(Value::Ref(list_id))
    }
}

impl PyTrait for ChainMap {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::ChainMap
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.flat.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        self.flat.py_eq(&other.flat, heap, interns)
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        for map in &mut self.maps {
            map.py_dec_ref_ids(stack);
        }
        self.flat.py_dec_ref_ids(stack);
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.flat.is_empty()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("ChainMap(")?;
        for (idx, map) in self.maps.iter().enumerate() {
            if idx > 0 {
                f.write_str(", ")?;
            }
            map.py_repr_fmt(f, heap, heap_ids, interns)?;
        }
        f.write_char(')')
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.flat.py_estimate_size()
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Value> {
        match self.flat.get(key, heap, interns)? {
            Some(value) => Ok(value.clone_with_heap(heap)),
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
        let Some(Value::Ref(first_id)) = self.maps.first() else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(RunError::internal("ChainMap has no first mapping"));
        };

        heap.with_entry_mut(*first_id, |heap_inner, data| {
            if let HeapData::Dict(dict) = data {
                if let Some(old) = dict.set(key, value, heap_inner, interns)? {
                    old.drop_with_heap(heap_inner);
                }
                Ok(())
            } else {
                key.drop_with_heap(heap_inner);
                value.drop_with_heap(heap_inner);
                Err(RunError::internal("ChainMap first mapping is not a dict"))
            }
        })?;
        self.rebuild_flat(heap, interns)
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        if let Some(method) = attr.static_string() {
            return match method {
                StaticStrings::Keys => {
                    args.check_zero_args("ChainMap.keys", heap)?;
                    self.keys_result(heap)
                }
                StaticStrings::Values => {
                    args.check_zero_args("ChainMap.values", heap)?;
                    self.values_result(heap)
                }
                StaticStrings::Items => {
                    args.check_zero_args("ChainMap.items", heap)?;
                    self.items_result(heap)
                }
                StaticStrings::Get => {
                    let (key, default) = args.get_one_two_args("ChainMap.get", heap)?;
                    let fallback = default.unwrap_or(Value::None);
                    let result = match self.flat.get(&key, heap, interns)? {
                        Some(value) => value.clone_with_heap(heap),
                        None => fallback.clone_with_heap(heap),
                    };
                    key.drop_with_heap(heap);
                    fallback.drop_with_heap(heap);
                    Ok(result)
                }
                StaticStrings::Pop => {
                    let (key, default) = args.get_one_two_args("ChainMap.pop", heap)?;
                    let Some(Value::Ref(first_id)) = self.maps.first() else {
                        key.drop_with_heap(heap);
                        default.drop_with_heap(heap);
                        return Err(RunError::internal("ChainMap has no first mapping"));
                    };

                    let popped = heap.with_entry_mut(*first_id, |heap_inner, data| match data {
                        HeapData::Dict(dict) => dict.pop(&key, heap_inner, interns),
                        _ => Err(RunError::internal("ChainMap first mapping is not a dict")),
                    })?;

                    match popped {
                        Some((old_key, value)) => {
                            old_key.drop_with_heap(heap);
                            key.drop_with_heap(heap);
                            default.drop_with_heap(heap);
                            self.rebuild_flat(heap, interns)?;
                            Ok(value)
                        }
                        None => {
                            if let Some(default) = default {
                                key.drop_with_heap(heap);
                                Ok(default)
                            } else {
                                let err = ExcType::key_error(&key, heap, interns);
                                key.drop_with_heap(heap);
                                Err(err)
                            }
                        }
                    }
                }
                _ => {
                    args.drop_with_heap(heap);
                    Err(ExcType::attribute_error(Type::ChainMap, attr.as_str(interns)))
                }
            };
        }

        if attr.as_str(interns) == "new_child" {
            return self.new_child(args, heap, interns);
        }

        args.drop_with_heap(heap);
        Err(ExcType::attribute_error(Type::ChainMap, attr.as_str(interns)))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let name = interns.get_str(attr_id);
        if name == "maps" {
            return self.maps_value(heap).map(AttrCallResult::Value).map(Some);
        }
        if name == "parents" {
            return self.parents_value(heap, interns).map(AttrCallResult::Value).map(Some);
        }
        Ok(None)
    }
}
