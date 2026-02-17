//! Minimal weak reference support.
//!
//! This module provides a `WeakRef` type that mirrors `weakref.ref` objects.
//! Weak references do not keep targets alive and return `None` when the target
//! has been cleared.

use std::fmt::Write;

use ahash::AHashSet;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, BoundMethod, PyTrait, Type},
    value::{EitherStr, Value, heap_tagged_id, public_id_from_internal_id},
};

/// Weak-reference flavor used to distinguish `weakref.ref` from proxy handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum WeakRefKind {
    /// Standard `weakref.ref` semantics.
    Reference,
    /// Proxy object semantics (`weakref.proxy`).
    Proxy,
    /// `weakref.WeakMethod` semantics.
    Method,
}

/// A weak reference to a heap-allocated object.
///
/// Weak references do not participate in reference counting for the target.
/// When the target is freed, the weak reference is cleared and returns `None`
/// when called.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct WeakRef {
    /// The referenced heap ID, or `None` if the target has been cleared.
    target: Option<HeapId>,
    /// Optional direct target for non-heap referents (e.g. `DefFunction`).
    direct_target: Option<Value>,
    /// Optional callback invoked when the target dies.
    callback: Option<Value>,
    /// Stored function for `WeakMethod` reconstruction.
    method_func: Option<Value>,
    /// The weakref behavior flavor.
    kind: WeakRefKind,
    /// Cached hash value for `weakref.ref` objects.
    ///
    /// CPython preserves weakref hashes after the target dies once the hash has
    /// been computed at least once while alive.
    cached_hash: Option<u64>,
}

impl WeakRef {
    /// Creates a new weak reference to the given heap ID.
    #[must_use]
    pub fn new(target: HeapId) -> Self {
        Self::new_with(target, None, WeakRefKind::Reference)
    }

    /// Creates a new weak reference with callback and flavor.
    #[must_use]
    pub fn new_with(target: HeapId, callback: Option<Value>, kind: WeakRefKind) -> Self {
        Self {
            target: Some(target),
            direct_target: None,
            callback,
            method_func: None,
            kind,
            cached_hash: None,
        }
    }

    /// Creates a new proxy-style weak reference.
    #[must_use]
    pub fn new_proxy(target: HeapId, callback: Option<Value>) -> Self {
        Self::new_with(target, callback, WeakRefKind::Proxy)
    }

    /// Creates a weak reference that targets a non-heap value.
    #[must_use]
    pub fn new_direct(target: Value, callback: Option<Value>) -> Self {
        Self {
            target: None,
            direct_target: Some(target),
            callback,
            method_func: None,
            kind: WeakRefKind::Reference,
            cached_hash: None,
        }
    }

    /// Creates a new `WeakMethod`-style weak reference.
    #[must_use]
    pub fn new_method(target: HeapId, method_func: Value, callback: Option<Value>) -> Self {
        Self {
            target: Some(target),
            direct_target: None,
            callback,
            method_func: Some(method_func),
            kind: WeakRefKind::Method,
            cached_hash: None,
        }
    }

    /// Returns the target heap ID if still alive.
    #[must_use]
    pub fn target(&self) -> Option<HeapId> {
        self.target
    }

    /// Returns true when this weak reference is a proxy.
    #[must_use]
    pub fn is_proxy(&self) -> bool {
        self.kind == WeakRefKind::Proxy
    }

    /// Returns true when this weak reference is a `WeakMethod`.
    #[must_use]
    pub fn is_method(&self) -> bool {
        self.kind == WeakRefKind::Method
    }

    /// Returns the direct non-heap target, if present.
    #[must_use]
    pub fn direct_target(&self) -> Option<&Value> {
        self.direct_target.as_ref()
    }

    /// Returns the stored bound function for `WeakMethod`.
    #[must_use]
    pub fn method_func(&self) -> Option<&Value> {
        self.method_func.as_ref()
    }

    /// Returns the callback, if set.
    #[must_use]
    pub fn callback(&self) -> Option<&Value> {
        self.callback.as_ref()
    }

    /// Returns true when a callback is currently registered.
    #[must_use]
    pub fn has_callback(&self) -> bool {
        self.callback.is_some()
    }

    /// Returns the cached hash, if already computed.
    #[must_use]
    pub fn cached_hash(&self) -> Option<u64> {
        self.cached_hash
    }

    /// Stores a computed hash for later reuse.
    pub fn set_cached_hash(&mut self, hash: u64) {
        self.cached_hash = Some(hash);
    }

    /// Clears the weak reference (target has been freed).
    pub fn clear(&mut self) {
        self.target = None;
    }

    /// Removes and returns the callback, if present.
    pub fn take_callback(&mut self) -> Option<Value> {
        self.callback.take()
    }
}

impl PyTrait for WeakRef {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::WeakRef
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        if let (Some(lhs), Some(rhs)) = (&self.direct_target, &other.direct_target) {
            return matches!((lhs, rhs), (Value::DefFunction(left), Value::DefFunction(right)) if left == right);
        }
        match (self.target, other.target) {
            (Some(lhs), Some(rhs)) => lhs == rhs,
            _ => false,
        }
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        // Weak references do not own the target.
        if let Some(callback) = &mut self.callback {
            callback.py_dec_ref_ids(stack);
        }
        if let Some(target) = &mut self.direct_target {
            target.py_dec_ref_ids(stack);
        }
        if let Some(method_func) = &mut self.method_func {
            method_func.py_dec_ref_ids(stack);
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        let self_ptr = std::ptr::from_ref::<Self>(self) as usize;
        if let Some(target) = &self.direct_target {
            let target_type = target.py_type(heap);
            return write!(f, "<weakref at 0x{self_ptr:x}; to '{target_type}' at 0x0>",);
        }
        if let Some(target_id) = self.target {
            let target_type = heap
                .get_if_live(target_id)
                .map_or(Type::Object, |data| data.py_type(heap));
            let target_address = public_id_from_internal_id(heap_tagged_id(target_id));
            if self.is_proxy() {
                write!(
                    f,
                    "<weakproxy at 0x{self_ptr:x}; to '{target_type}' at 0x{target_address:x}>",
                )
            } else {
                write!(
                    f,
                    "<weakref at 0x{self_ptr:x}; to '{target_type}' at 0x{target_address:x}>",
                )
            }
        } else if self.is_proxy() {
            write!(f, "<weakproxy at 0x{self_ptr:x}; dead>")
        } else {
            write!(f, "<weakref at 0x{self_ptr:x}; dead>")
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);

        if attr_name == "__callback__" {
            let value = self
                .callback
                .as_ref()
                .map_or(Value::None, |callback| callback.clone_with_heap(heap));
            return Ok(Some(AttrCallResult::Value(value)));
        }

        if self.is_proxy() {
            let Some(target_id) = self.target else {
                return Err(proxy_reference_error());
            };
            if heap.get_if_live(target_id).is_none() {
                return Err(proxy_reference_error());
            }
            let attr_name = interns.get_str(attr_id);

            if let HeapData::Instance(inst) = heap.get(target_id) {
                let class_id = inst.class_id();
                let attrs_id = inst.attrs_id();

                // Instance dictionary lookup first.
                if let Some(attrs_id) = attrs_id
                    && let HeapData::Dict(dict) = heap.get(attrs_id)
                    && let Some(value) = dict.get_by_str(attr_name, heap, interns)
                {
                    return Ok(Some(AttrCallResult::Value(value.clone_with_heap(heap))));
                }

                // Then class/MRO lookup.
                let class_attr = match heap.get(class_id) {
                    HeapData::ClassObject(cls) => cls.mro_lookup_attr(attr_name, class_id, heap, interns),
                    _ => None,
                };
                if let Some((value, _)) = class_attr {
                    let bind_method = match &value {
                        Value::DefFunction(_) => true,
                        Value::Ref(desc_id) => {
                            matches!(
                                heap.get(*desc_id),
                                HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)
                            )
                        }
                        _ => false,
                    };
                    if bind_method {
                        heap.inc_ref(target_id);
                        let instance = Value::Ref(target_id);
                        let bound_id = heap.allocate(HeapData::BoundMethod(BoundMethod::new(value, instance)))?;
                        return Ok(Some(AttrCallResult::Value(Value::Ref(bound_id))));
                    }
                    return Ok(Some(AttrCallResult::Value(value)));
                }
            }

            let result = Value::Ref(target_id).py_getattr(attr_id, heap, interns)?;
            if let AttrCallResult::DescriptorGet(descriptor) = result {
                heap.inc_ref(target_id);
                let instance = Value::Ref(target_id);
                let bound_id = heap.allocate(HeapData::BoundMethod(BoundMethod::new(descriptor, instance)))?;
                return Ok(Some(AttrCallResult::Value(Value::Ref(bound_id))));
            }
            return Ok(Some(result));
        }

        Ok(None)
    }

    fn py_call_attr_raw(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<AttrCallResult> {
        if !self.is_proxy() {
            return Err(ExcType::attribute_error(self.py_type(heap), attr.as_str(interns)));
        }

        let Some(target_id) = self.target else {
            args.drop_with_heap(heap);
            return Err(proxy_reference_error());
        };
        if heap.get_if_live(target_id).is_none() {
            self.clear();
            args.drop_with_heap(heap);
            return Err(proxy_reference_error());
        }

        heap.call_attr_raw(target_id, attr, args, interns)
    }
}

/// Creates the `ReferenceError` used when a weak proxy's target is dead.
fn proxy_reference_error() -> crate::exception_private::RunError {
    SimpleException::new_msg(ExcType::ReferenceError, "weakly-referenced object no longer exists").into()
}
