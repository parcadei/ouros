//! Implementation of `collections.OrderedDict`.
//!
//! OrderedDict preserves insertion order and provides `move_to_end` and
//! `popitem(last=...)` methods. It is a thin wrapper around `Dict`, which
//! already preserves insertion order in Ouros.

use std::fmt::Write;

use ahash::AHashSet;
use smallvec::SmallVec;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::{Dict, List, PyTrait, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// An ordered dictionary preserving insertion order with ordered-specific methods.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct OrderedDict {
    /// Underlying dict storing key-value pairs in insertion order.
    dict: Dict,
}

impl OrderedDict {
    /// Creates a new empty OrderedDict.
    #[must_use]
    pub fn new() -> Self {
        Self { dict: Dict::new() }
    }

    /// Creates an OrderedDict from an existing dict.
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

    /// Moves an existing key to the beginning or end of the ordered dict.
    fn move_to_end(
        &mut self,
        key: Value,
        last: bool,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let popped = self.dict.pop(&key, heap, interns)?;
        let Some((old_key, value)) = popped else {
            key.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::KeyError, "key not found").into());
        };
        key.drop_with_heap(heap);

        if last {
            let _ = self.dict.set(old_key, value, heap, interns)?;
            return Ok(());
        }

        let existing_items = self.dict.items(heap);
        self.dict.drop_all_entries(heap);

        let _ = self.dict.set(old_key, value, heap, interns)?;
        for (k, v) in existing_items {
            let _ = self.dict.set(k, v, heap, interns)?;
        }
        Ok(())
    }

    /// Pops and returns a (key, value) tuple from either end of the ordered dict.
    fn popitem(&mut self, last: bool, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
        if self.dict.is_empty() {
            return Err(SimpleException::new_msg(ExcType::KeyError, "dictionary is empty").into());
        }

        let target_key = if last {
            self.dict.key_at(self.dict.len() - 1).map(|k| k.clone_with_heap(heap))
        } else {
            self.dict.key_at(0).map(|k| k.clone_with_heap(heap))
        };

        let Some(target_key) = target_key else {
            return Err(SimpleException::new_msg(ExcType::KeyError, "dictionary is empty").into());
        };

        let popped = self.dict.pop(&target_key, heap, interns)?;
        target_key.drop_with_heap(heap);

        if let Some((key, value)) = popped {
            let tuple_items: SmallVec<[Value; 3]> = SmallVec::from_vec(vec![key, value]);
            let tuple_val = allocate_tuple(tuple_items, heap)?;
            Ok(tuple_val)
        } else {
            Err(SimpleException::new_msg(ExcType::KeyError, "key not found").into())
        }
    }

    /// Returns keys in insertion order as an iterable list result.
    ///
    /// This avoids delegating to dict view machinery that requires a heap id for
    /// the embedded dict storage.
    fn keys_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let keys = self.dict.keys(heap);
        let list_id = heap.allocate(HeapData::List(List::new(keys)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns values in insertion order as an iterable list result.
    ///
    /// This preserves OrderedDict ordering semantics for `list(od.values())`.
    fn values_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let values = self.dict.values(heap);
        let list_id = heap.allocate(HeapData::List(List::new(values)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns `(key, value)` pairs in insertion order as an iterable list result.
    ///
    /// Pairs are materialized as tuple objects to preserve item iteration behavior.
    fn items_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let items = self.dict.items(heap);
        let mut result_items = Vec::with_capacity(items.len());
        for (key, value) in items {
            let tuple_items: SmallVec<[Value; 3]> = SmallVec::from_vec(vec![key, value]);
            let tuple_val = allocate_tuple(tuple_items, heap)?;
            result_items.push(tuple_val);
        }
        let list_id = heap.allocate(HeapData::List(List::new(result_items)))?;
        Ok(Value::Ref(list_id))
    }

    /// Returns keys in reverse insertion order for `OrderedDict.__reversed__()`.
    fn reversed_keys_result(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let mut keys = self.dict.keys(heap);
        keys.reverse();
        let list_id = heap.allocate(HeapData::List(List::new(keys)))?;
        Ok(Value::Ref(list_id))
    }
}

impl PyTrait for OrderedDict {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::OrderedDict
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.dict.len())
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        if self.dict.len() != other.dict.len() {
            return false;
        }
        self.dict
            .iter()
            .zip(other.dict.iter())
            .all(|((left_key, left_value), (right_key, right_value))| {
                left_key.py_eq(right_key, heap, interns) && left_value.py_eq(right_value, heap, interns)
            })
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
        f.write_str("OrderedDict(")?;
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
        self.dict.py_getitem(key, heap, interns)
    }

    fn py_setitem(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        self.dict.py_setitem(key, value, heap, interns)
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
        if method.is_none() && attr.as_str(interns) == "__reversed__" {
            args.check_zero_args("OrderedDict.__reversed__", heap)?;
            return self.reversed_keys_result(heap);
        }
        let Some(method) = method else {
            return Err(ExcType::attribute_error(Type::OrderedDict, attr.as_str(interns)));
        };

        match method {
            StaticStrings::Keys => {
                args.check_zero_args("OrderedDict.keys", heap)?;
                self.keys_result(heap)
            }
            StaticStrings::Values => {
                args.check_zero_args("OrderedDict.values", heap)?;
                self.values_result(heap)
            }
            StaticStrings::Items => {
                args.check_zero_args("OrderedDict.items", heap)?;
                self.items_result(heap)
            }
            StaticStrings::MoveToEnd => {
                let (mut positional, kwargs) = args.into_parts();
                let positional_count = positional.len();
                if positional_count == 0 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error_at_least("OrderedDict.move_to_end", 1, 0));
                }
                if positional_count > 2 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error_at_most(
                        "OrderedDict.move_to_end",
                        2,
                        positional_count,
                    ));
                }

                let key = positional.next().expect("count checked above");
                let mut last = positional.next();
                positional.drop_with_heap(heap);

                let mut kwargs_iter = kwargs.into_iter();
                while let Some((kw, value)) = kwargs_iter.next() {
                    let Some(kw_name) = kw.as_either_str(heap) else {
                        kw.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        for (rest_kw, rest_value) in kwargs_iter {
                            rest_kw.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        key.drop_with_heap(heap);
                        last.drop_with_heap(heap);
                        return Err(ExcType::type_error_kwargs_nonstring_key());
                    };
                    if kw_name.as_str(interns) != "last" {
                        kw.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        for (rest_kw, rest_value) in kwargs_iter {
                            rest_kw.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        key.drop_with_heap(heap);
                        last.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "OrderedDict.move_to_end() got an unexpected keyword argument",
                        ));
                    }
                    if last.is_some() {
                        kw.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        for (rest_kw, rest_value) in kwargs_iter {
                            rest_kw.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        key.drop_with_heap(heap);
                        last.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "OrderedDict.move_to_end() got multiple values for argument 'last'",
                        ));
                    }
                    kw.drop_with_heap(heap);
                    last = Some(value);
                }

                let move_last = if let Some(last) = last {
                    let flag = last.py_bool(heap, interns);
                    last.drop_with_heap(heap);
                    flag
                } else {
                    true
                };
                self.move_to_end(key, move_last, heap, interns)?;
                Ok(Value::None)
            }
            StaticStrings::Popitem => {
                let (positional, kwargs) = args.into_parts();
                let positional_count = positional.len();
                if positional_count > 1 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error_at_most("OrderedDict.popitem", 1, positional_count));
                }

                let mut positional = positional;
                let mut last = positional.next();
                positional.drop_with_heap(heap);

                let mut kwargs_iter = kwargs.into_iter();
                while let Some((kw, value)) = kwargs_iter.next() {
                    let Some(kw_name) = kw.as_either_str(heap) else {
                        kw.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        for (rest_kw, rest_value) in kwargs_iter {
                            rest_kw.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        last.drop_with_heap(heap);
                        return Err(ExcType::type_error_kwargs_nonstring_key());
                    };
                    if kw_name.as_str(interns) != "last" {
                        kw.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        for (rest_kw, rest_value) in kwargs_iter {
                            rest_kw.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        last.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "OrderedDict.popitem() got an unexpected keyword argument",
                        ));
                    }
                    if last.is_some() {
                        kw.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        for (rest_kw, rest_value) in kwargs_iter {
                            rest_kw.drop_with_heap(heap);
                            rest_value.drop_with_heap(heap);
                        }
                        last.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "OrderedDict.popitem() got multiple values for argument 'last'",
                        ));
                    }
                    kw.drop_with_heap(heap);
                    last = Some(value);
                }

                let pop_last = if let Some(last) = last {
                    let flag = last.py_bool(heap, interns);
                    last.drop_with_heap(heap);
                    flag
                } else {
                    true
                };
                self.popitem(pop_last, heap, interns)
            }
            // Delegate dict methods (keys, values, items, get, pop, etc.) to the
            // underlying dict.  Forward `self_id` so that view-creating methods
            // can look up this OrderedDict on the heap and extract the inner dict.
            _ => self.dict.py_call_attr(heap, attr, args, interns, self_id),
        }
    }
}
