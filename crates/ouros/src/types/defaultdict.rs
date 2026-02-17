//! Implementation of Python's `collections.defaultdict` type.
//!
//! A defaultdict is a dict subclass that calls a factory function to supply
//! missing values. When a key is accessed that doesn't exist, the default
//! factory is called to create a new value, which is then inserted into the
//! dict and returned.

use std::{fmt::Write, sync::RwLock};

use ahash::AHashSet;

use crate::{
    ResourceTracker,
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    types::{AttrCallResult, Dict, Type},
    value::{EitherStr, Value},
};

/// A defaultdict implementation with automatic default value creation.
///
/// Wraps a `Dict` and stores a `default_factory` callable that is invoked
/// when a missing key is accessed. Uses `RwLock` for interior mutability
/// to allow mutation during `py_getitem` which takes `&self`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DefaultDict {
    /// The underlying dictionary storing key-value pairs.
    /// Uses RwLock to allow mutation during get operations.
    #[serde(skip)]
    dict: RwLock<Dict>,
    /// The factory function called to create default values for missing keys.
    /// If None, behaves like a regular dict (raises KeyError for missing keys).
    default_factory: Option<Value>,
}

impl DefaultDict {
    /// Creates a new defaultdict with the given default factory.
    #[inline]
    pub fn new(default_factory: Option<Value>) -> Self {
        Self {
            dict: RwLock::new(Dict::new()),
            default_factory,
        }
    }

    /// Creates a defaultdict from an existing dict.
    #[inline]
    pub fn from_dict(dict: Dict, default_factory: Option<Value>) -> Self {
        Self {
            dict: RwLock::new(dict),
            default_factory,
        }
    }

    /// Returns the default factory, if any.
    #[inline]
    pub fn default_factory(&self) -> Option<&Value> {
        self.default_factory.as_ref()
    }

    /// Sets the default factory.
    #[inline]
    #[expect(dead_code, reason = "reserved for future defaultdict API")]
    pub fn set_default_factory(&mut self, factory: Option<Value>) {
        self.default_factory = factory;
    }

    /// Replaces `default_factory`, returning the previous value if present.
    pub fn replace_default_factory(&mut self, factory: Option<Value>) -> Option<Value> {
        std::mem::replace(&mut self.default_factory, factory)
    }

    /// Returns the number of items in the defaultdict.
    #[inline]
    pub fn len(&self) -> usize {
        self.dict.read().unwrap().len()
    }

    /// Returns whether the defaultdict is empty.
    #[inline]
    #[expect(dead_code, reason = "reserved for future defaultdict API")]
    pub fn is_empty(&self) -> bool {
        self.dict.read().unwrap().is_empty()
    }

    /// Returns a reference to the underlying dict.
    #[inline]
    pub fn dict(&self) -> std::sync::RwLockReadGuard<'_, Dict> {
        self.dict.read().unwrap()
    }

    /// Returns an existing value for `key`, cloned for the caller.
    ///
    /// Unlike `py_getitem`, this never triggers `default_factory`; it is used by
    /// VM-level missing-key handling that may need to call arbitrary Python
    /// callables and resume after frame pushes.
    pub fn get_existing(
        &self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        let dict_ref = self.dict.read().unwrap();
        match dict_ref.get(key, heap, interns)? {
            Some(value) => Ok(Some(value.clone_with_heap(heap))),
            None => Ok(None),
        }
    }

    /// Returns a cloned `default_factory` value, if present.
    pub fn default_factory_cloned(&self, heap: &Heap<impl ResourceTracker>) -> Option<Value> {
        self.default_factory
            .as_ref()
            .map(|factory| factory.clone_with_heap(heap))
    }

    /// Inserts a computed default value for `key`.
    ///
    /// Drops any replaced old value and mirrors dict assignment semantics.
    pub fn insert_default(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        if let Some(old_value) = self.dict.write().unwrap().set(key, value, heap, interns)? {
            old_value.drop_with_heap(heap);
        }
        Ok(())
    }

    /// Returns whether the defaultdict contains any heap references.
    #[inline]
    pub fn has_refs(&self) -> bool {
        self.dict.read().unwrap().has_refs()
            || self
                .default_factory
                .as_ref()
                .is_some_and(|v| matches!(v, Value::Ref(_)))
    }

    /// Creates a shallow copy of this defaultdict with proper refcount handling.
    pub fn clone_with_heap(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        let new_dict = self.dict.read().unwrap().clone_with_heap(heap, interns)?;
        let new_factory = self.default_factory.as_ref().map(|f| f.clone_with_heap(heap));
        Ok(Self::from_dict(new_dict, new_factory))
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for DefaultDict {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        // Dict doesn't implement DropWithHeap, but has drop_all_entries
        // Since we're consuming self, we need to drop entries manually
        let dict = self.dict;
        dict.into_inner().unwrap().drop_all_entries(heap);
        if let Some(factory) = self.default_factory {
            factory.drop_with_heap(heap);
        }
    }
}

impl super::PyTrait for DefaultDict {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::DefaultDict
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.dict.read().unwrap().len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        self.dict
            .read()
            .unwrap()
            .py_eq(&other.dict.read().unwrap(), heap, interns)
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.dict.write().unwrap().py_dec_ref_ids(stack);
        if let Some(Value::Ref(id)) = &self.default_factory {
            stack.push(*id);
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.dict.read().unwrap().is_empty()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("defaultdict(")?;
        match &self.default_factory {
            Some(factory) => {
                factory.py_repr_fmt(f, heap, heap_ids, interns)?;
            }
            None => f.write_str("None")?,
        }
        f.write_str(", ")?;
        self.dict.read().unwrap().py_repr_fmt(f, heap, heap_ids, interns)?;
        f.write_char(')')
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.dict.read().unwrap().py_estimate_size()
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Value> {
        // Read under a scoped lock, then release before any potential write path.
        let existing_value = {
            let dict_ref = self.dict.read().unwrap();
            dict_ref
                .get(key, heap, interns)?
                .map(|value| value.clone_with_heap(heap))
        };
        if let Some(value) = existing_value {
            return Ok(value);
        }

        // Key not found - check if we have a default factory
        match &self.default_factory {
            Some(factory) => {
                // Call the factory to get default value
                // Handle the common case of builtin types like int, list, etc.
                let default_value = match factory {
                    Value::Builtin(Builtins::Type(t)) => {
                        // Builtin type constructor - call directly
                        t.call(heap, ArgValues::Empty, interns)?
                    }
                    _ => {
                        // For other callables (classes, functions), we would need VM access
                        // For now, return an error indicating this is not supported
                        return Err(ExcType::type_error(
                            "defaultdict currently only supports builtin types as default_factory",
                        ));
                    }
                };

                // Clone the key for insertion only after factory succeeds so early errors
                // do not leak a key clone.
                let key_clone = key.clone_with_heap(heap);

                // Insert the default value into the dict.
                if let Err(e) =
                    self.dict
                        .write()
                        .unwrap()
                        .set(key_clone, default_value.clone_with_heap(heap), heap, interns)
                {
                    default_value.drop_with_heap(heap);
                    return Err(e);
                }

                // Return the default value
                Ok(default_value)
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
        if let Some(old_value) = self.dict.write().unwrap().set(key, value, heap, interns)? {
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
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let method = attr.static_string();
        if method.is_none() && attr.as_str(interns) == "__missing__" {
            let key = args.get_one_arg("defaultdict.__missing__", heap)?;
            let result = self.py_getitem(&key, heap, interns);
            key.drop_with_heap(heap);
            return result;
        }
        let Some(method) = method else {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(Type::DefaultDict, attr.as_str(interns)));
        };

        match method {
            // Delegate dict methods to the underlying dict
            StaticStrings::Get => {
                let (key, default) = args.get_one_two_args("get", heap)?;
                let default = default.unwrap_or(Value::None);
                let dict_ref = self.dict.read().unwrap();
                let result = match dict_ref.get(&key, heap, interns) {
                    Ok(r) => r,
                    Err(e) => {
                        drop(dict_ref);
                        key.drop_with_heap(heap);
                        default.drop_with_heap(heap);
                        return Err(e);
                    }
                };
                let value = match result {
                    Some(v) => v.clone_with_heap(heap),
                    None => default.clone_with_heap(heap),
                };
                drop(dict_ref);
                key.drop_with_heap(heap);
                default.drop_with_heap(heap);
                Ok(value)
            }
            StaticStrings::Keys => {
                args.check_zero_args("keys", heap)?;
                let keys = self.dict.read().unwrap().keys(heap);
                let list_id = heap.allocate(HeapData::List(super::List::new(keys)))?;
                Ok(Value::Ref(list_id))
            }
            StaticStrings::Values => {
                args.check_zero_args("values", heap)?;
                let values = self.dict.read().unwrap().values(heap);
                let list_id = heap.allocate(HeapData::List(super::List::new(values)))?;
                Ok(Value::Ref(list_id))
            }
            StaticStrings::Items => {
                args.check_zero_args("items", heap)?;
                let items = self.dict.read().unwrap().items(heap);
                let mut tuples: Vec<Value> = Vec::with_capacity(items.len());
                for (k, v) in items {
                    let tuple_val = super::allocate_tuple(smallvec::smallvec![k, v], heap)?;
                    tuples.push(tuple_val);
                }
                let list_id = heap.allocate(HeapData::List(super::List::new(tuples)))?;
                Ok(Value::Ref(list_id))
            }
            StaticStrings::Pop => {
                let (key, default) = args.get_one_two_args("pop", heap)?;
                let result = match self.dict.write().unwrap().pop(&key, heap, interns) {
                    Ok(r) => r,
                    Err(e) => {
                        key.drop_with_heap(heap);
                        if let Some(d) = default {
                            d.drop_with_heap(heap);
                        }
                        return Err(e);
                    }
                };
                match result {
                    Some((k, v)) => {
                        k.drop_with_heap(heap);
                        key.drop_with_heap(heap);
                        if let Some(d) = default {
                            d.drop_with_heap(heap);
                        }
                        Ok(v)
                    }
                    None => match default {
                        Some(d) => {
                            key.drop_with_heap(heap);
                            Ok(d)
                        }
                        None => Err(ExcType::key_error(&key, heap, interns)),
                    },
                }
            }
            StaticStrings::Clear => {
                args.check_zero_args("clear", heap)?;
                self.dict.write().unwrap().drop_all_entries(heap);
                *self.dict.write().unwrap() = Dict::new();
                Ok(Value::None)
            }
            StaticStrings::Copy => {
                args.check_zero_args("copy", heap)?;
                let new_defaultdict = self.clone_with_heap(heap, interns)?;
                let id = heap.allocate(HeapData::DefaultDict(new_defaultdict))?;
                Ok(Value::Ref(id))
            }
            StaticStrings::Update => {
                // Get the iterable/dict to update from
                let other = args.get_one_arg("update", heap)?;

                // Try to iterate over the other object
                // Note: OurosIter::new takes ownership of `other`, so we can't drop it on error
                let mut iter = super::OurosIter::new(other, heap, interns)?;

                // Process each item
                loop {
                    match iter.for_next(heap, interns) {
                        Ok(Some(item)) => {
                            // Each item should be a (key, value) pair
                            // Use a separate scope to handle the pair iterator
                            let (key, value) = {
                                let mut pair_iter = super::OurosIter::new(item, heap, interns)?;

                                // Get the key
                                let key = match pair_iter.for_next(heap, interns) {
                                    Ok(Some(k)) => k,
                                    Ok(None) => {
                                        pair_iter.drop_with_heap(heap);
                                        iter.drop_with_heap(heap);
                                        return Err(ExcType::type_error(
                                            "cannot convert dictionary update sequence element #0 to a sequence"
                                                .to_string(),
                                        ));
                                    }
                                    Err(e) => {
                                        pair_iter.drop_with_heap(heap);
                                        iter.drop_with_heap(heap);
                                        return Err(e);
                                    }
                                };

                                // Get the value
                                let value = match pair_iter.for_next(heap, interns) {
                                    Ok(Some(v)) => v,
                                    Ok(None) => {
                                        key.drop_with_heap(heap);
                                        pair_iter.drop_with_heap(heap);
                                        iter.drop_with_heap(heap);
                                        return Err(ExcType::type_error(
                                            "dictionary update sequence element #0 has length 1; 2 is required"
                                                .to_string(),
                                        ));
                                    }
                                    Err(e) => {
                                        key.drop_with_heap(heap);
                                        pair_iter.drop_with_heap(heap);
                                        iter.drop_with_heap(heap);
                                        return Err(e);
                                    }
                                };

                                // Check there's no extra element
                                match pair_iter.for_next(heap, interns) {
                                    Ok(Some(extra)) => {
                                        extra.drop_with_heap(heap);
                                        key.drop_with_heap(heap);
                                        value.drop_with_heap(heap);
                                        pair_iter.drop_with_heap(heap);
                                        iter.drop_with_heap(heap);
                                        return Err(ExcType::type_error(
                                            "dictionary update sequence element #0 has length 3; 2 is required"
                                                .to_string(),
                                        ));
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        key.drop_with_heap(heap);
                                        value.drop_with_heap(heap);
                                        pair_iter.drop_with_heap(heap);
                                        iter.drop_with_heap(heap);
                                        return Err(e);
                                    }
                                }

                                pair_iter.drop_with_heap(heap);
                                (key, value)
                            };

                            // Insert into the dict
                            // Note: set() takes ownership of key and value. On error, it drops them internally.
                            match self.dict.write().unwrap().set(key, value, heap, interns) {
                                Ok(_) => {}
                                Err(e) => {
                                    iter.drop_with_heap(heap);
                                    return Err(e);
                                }
                            }
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
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(Type::DefaultDict, method.into()))
            }
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        if interns.get_str(attr_id) == "default_factory" {
            let value = match &self.default_factory {
                Some(factory) => factory.clone_with_heap(heap),
                None => Value::None,
            };
            return Ok(Some(AttrCallResult::Value(value)));
        }
        Ok(None)
    }
}
