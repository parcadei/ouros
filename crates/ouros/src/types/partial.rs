//! Implementation of `functools.partial` and `functools.cmp_to_key` wrapper types.
//!
//! `Partial` stores a callable together with pre-applied positional and keyword arguments.
//! When called, the VM prepends the stored positional args and merges stored kwargs
//! before forwarding to the wrapped function.
//!
//! `CmpToKey` wraps a comparison function. When used as a key function (e.g., in
//! `sorted()`), the VM calls the comparison function with two values and uses
//! the return value to determine ordering.

use std::fmt::Write;

use ahash::AHashSet;
use smallvec::smallvec;

use crate::{
    args::ArgValues,
    exception_private::RunResult,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StringId},
    modules::{ModuleFunctions, statistics::StatisticsFunctions},
    resource::ResourceTracker,
    types::{AttrCallResult, Dict, PyTrait, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// A `functools.partial` object that stores a callable and pre-applied arguments.
///
/// When the partial object is called, the stored positional arguments are prepended
/// to the call-site arguments, and stored keyword arguments are merged (call-site
/// kwargs take precedence over stored kwargs).
///
/// # Example (Python)
/// ```python
/// from functools import partial
/// add = partial(lambda a, b: a + b, 10)
/// add(5)  # returns 15
/// ```
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Partial {
    /// The wrapped callable (function, lambda, builtin, etc.).
    pub(crate) func: Value,
    /// Pre-applied positional arguments, prepended on every call.
    pub(crate) args: Vec<Value>,
    /// Pre-applied keyword arguments as (key, value) pairs, merged on every call.
    pub(crate) kwargs: Vec<(Value, Value)>,
    /// Optional weakref-finalizer state when this partial is created by `weakref.finalize()`.
    ///
    /// Normal `functools.partial` values keep this as `None`.
    #[serde(default)]
    finalizer: Option<WeakFinalizeState>,
}

/// Runtime state attached to `weakref.finalize()` handles.
///
/// We model finalizers as specialized `Partial` values so they remain callable and
/// can reuse argument-merging code. The state tracks the target weakref and whether
/// the callback is still pending.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct WeakFinalizeState {
    /// Heap ID of an internal weakref object pointing at the target.
    ///
    /// The finalizer holds this weakref strongly so it can check liveness later
    /// without keeping the target alive.
    target_ref: HeapId,
    /// Whether the callback may still run.
    ///
    /// Set to `false` by `detach()`, manual invocation, or GC-driven callback dispatch.
    pending: bool,
}

impl Partial {
    /// Creates a new `Partial` wrapping the given function with pre-applied args.
    pub fn new(func: Value, args: Vec<Value>, kwargs: Vec<(Value, Value)>) -> Self {
        Self {
            func,
            args,
            kwargs,
            finalizer: None,
        }
    }

    /// Creates a `Partial` configured as a `weakref.finalize()` handle.
    ///
    /// The handle remains callable, but also exposes `alive`, `detach()`, and `peek()`.
    pub fn new_weakref_finalize(
        func: Value,
        args: Vec<Value>,
        kwargs: Vec<(Value, Value)>,
        target_ref: HeapId,
    ) -> Self {
        Self {
            func,
            args,
            kwargs,
            finalizer: Some(WeakFinalizeState {
                target_ref,
                pending: true,
            }),
        }
    }

    /// Returns a reference to the wrapped function.
    pub fn func(&self) -> &Value {
        &self.func
    }

    /// Returns a reference to the pre-applied positional arguments.
    pub fn args(&self) -> &[Value] {
        &self.args
    }

    /// Returns a reference to the pre-applied keyword arguments.
    pub fn kwargs(&self) -> &[(Value, Value)] {
        &self.kwargs
    }

    /// Returns true if this partial is a `weakref.finalize()` handle.
    #[must_use]
    pub fn is_weakref_finalize(&self) -> bool {
        self.finalizer.is_some()
    }

    /// Returns true if a weakref finalizer callback is still pending.
    #[must_use]
    pub fn weak_finalize_pending(&self) -> bool {
        self.finalizer.is_some_and(|state| state.pending)
    }

    /// Marks the weakref finalizer callback as no longer pending.
    pub fn mark_weak_finalize_complete(&mut self) {
        if let Some(state) = &mut self.finalizer {
            state.pending = false;
        }
    }

    /// Returns the target weakref id for `weakref.finalize()` handles.
    #[must_use]
    pub fn weak_finalize_target_ref(&self) -> Option<HeapId> {
        self.finalizer.map(|state| state.target_ref)
    }

    /// Returns true if the finalizer is currently alive.
    ///
    /// A finalizer is alive only while pending and while its target weakref still
    /// points to a live referent.
    #[must_use]
    pub fn weak_finalize_alive(&self, heap: &Heap<impl ResourceTracker>) -> bool {
        let Some(state) = self.finalizer else {
            return false;
        };
        if !state.pending {
            return false;
        }
        weakref_referent_is_live(state.target_ref, heap)
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for Partial {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.func.drop_with_heap(heap);
        for arg in self.args {
            arg.drop_with_heap(heap);
        }
        for (key, value) in self.kwargs {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
        if let Some(state) = self.finalizer {
            heap.dec_ref(state.target_ref);
        }
    }
}

impl PyTrait for Partial {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Function
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.func.py_dec_ref_ids(stack);
        for arg in &mut self.args {
            arg.py_dec_ref_ids(stack);
        }
        for (key, value) in &mut self.kwargs {
            key.py_dec_ref_ids(stack);
            value.py_dec_ref_ids(stack);
        }
        if let Some(state) = self.finalizer {
            stack.push(state.target_ref);
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        if let Some(method_name) = normaldist_method_name(self.func()) {
            let args = self.args();
            if self.kwargs().is_empty() && args.len() == 2 {
                f.write_str("<bound method NormalDist.")?;
                f.write_str(method_name)?;
                f.write_str(" of NormalDist(mu=")?;
                args[0].py_repr_fmt(f, heap, heap_ids, interns)?;
                f.write_str(", sigma=")?;
                args[1].py_repr_fmt(f, heap, heap_ids, interns)?;
                f.write_str(")>")?;
                return Ok(());
            }
        }
        f.write_str("functools.partial(...)")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.args.len() * std::mem::size_of::<Value>()
            + self.kwargs.len() * std::mem::size_of::<(Value, Value)>()
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        match interns.get_str(attr_id) {
            "func" => Ok(Some(AttrCallResult::Value(self.func.clone_with_heap(heap)))),
            "args" => {
                let tuple = allocate_tuple(self.args.iter().map(|arg| arg.clone_with_heap(heap)).collect(), heap)?;
                Ok(Some(AttrCallResult::Value(tuple)))
            }
            "keywords" => {
                let pairs: Vec<(Value, Value)> = self
                    .kwargs
                    .iter()
                    .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
                    .collect();
                let dict = Dict::from_pairs(pairs, heap, interns)?;
                let dict_id = heap.allocate(HeapData::Dict(dict))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(dict_id))))
            }
            "alive" if self.is_weakref_finalize() => {
                Ok(Some(AttrCallResult::Value(Value::Bool(self.weak_finalize_alive(heap)))))
            }
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_name = attr.as_str(interns);
        if !self.is_weakref_finalize() {
            return Err(crate::exception_private::ExcType::attribute_error(
                self.py_type(heap),
                attr_name,
            ));
        }

        match attr_name {
            "detach" => {
                args.check_zero_args("finalize.detach", heap)?;
                self.mark_weak_finalize_complete();
                Ok(Value::None)
            }
            "peek" => {
                args.check_zero_args("finalize.peek", heap)?;
                Ok(finalize_peek_tuple(self, heap, interns)?)
            }
            _ => Err(crate::exception_private::ExcType::attribute_error(
                self.py_type(heap),
                attr_name,
            )),
        }
    }
}

/// Returns the user-facing NormalDist method name for synthetic statistics partials.
fn normaldist_method_name(func: &Value) -> Option<&'static str> {
    let Value::ModuleFunction(ModuleFunctions::Statistics(function)) = func else {
        return None;
    };

    match function {
        StatisticsFunctions::NormalDistPdf => Some("pdf"),
        StatisticsFunctions::NormalDistCdf => Some("cdf"),
        StatisticsFunctions::NormalDistInvCdf => Some("inv_cdf"),
        StatisticsFunctions::NormalDistOverlap => Some("overlap"),
        StatisticsFunctions::NormalDistSamples => Some("samples"),
        StatisticsFunctions::NormalDistQuantiles => Some("quantiles"),
        StatisticsFunctions::NormalDistZscore => Some("zscore"),
        _ => None,
    }
}

/// Returns true if the weakref at `weakref_id` still points at a live referent.
fn weakref_referent_is_live(weakref_id: HeapId, heap: &Heap<impl ResourceTracker>) -> bool {
    let Some(HeapData::WeakRef(weakref)) = heap.get_if_live(weakref_id) else {
        return false;
    };
    weakref
        .target()
        .is_some_and(|target_id| heap.get_if_live(target_id).is_some())
}

/// Builds the `finalize.peek()` tuple `(obj, func, args, kwargs)` or `None`.
fn finalize_peek_tuple(
    partial: &Partial,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if !partial.weak_finalize_pending() {
        return Ok(Value::None);
    }
    let Some(target_ref) = partial.weak_finalize_target_ref() else {
        return Ok(Value::None);
    };
    let Some(HeapData::WeakRef(weakref)) = heap.get_if_live(target_ref) else {
        return Ok(Value::None);
    };
    let Some(target_id) = weakref.target() else {
        return Ok(Value::None);
    };
    if heap.get_if_live(target_id).is_none() {
        return Ok(Value::None);
    }

    heap.inc_ref(target_id);
    let obj = Value::Ref(target_id);
    let func = partial.func().clone_with_heap(heap);
    let args_tuple = allocate_tuple(
        partial.args().iter().map(|arg| arg.clone_with_heap(heap)).collect(),
        heap,
    )?;
    let kwargs = Dict::from_pairs(
        partial
            .kwargs()
            .iter()
            .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
            .collect(),
        heap,
        interns,
    )?;
    let kwargs_id = heap.allocate(HeapData::Dict(kwargs))?;
    let kwargs_value = Value::Ref(kwargs_id);

    Ok(allocate_tuple(smallvec![obj, func, args_tuple, kwargs_value], heap)?)
}

/// A `functools.cmp_to_key` wrapper that adapts a comparison function into a key function.
///
/// The wrapper stores a comparison function. When used as a key function, the VM
/// creates `CmpToKeyValue` instances that use the comparison function to determine
/// ordering via `__lt__`.
///
/// # Example (Python)
/// ```python
/// from functools import cmp_to_key
/// sorted([3, 1, 2], key=cmp_to_key(lambda a, b: a - b))  # [1, 2, 3]
/// ```
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CmpToKey {
    /// The comparison function that takes two arguments and returns
    /// negative (a < b), zero (a == b), or positive (a > b).
    pub(crate) func: Value,
}

impl CmpToKey {
    /// Creates a new `CmpToKey` wrapping the given comparison function.
    pub fn new(func: Value) -> Self {
        Self { func }
    }

    /// Returns a reference to the wrapped comparison function.
    pub fn func(&self) -> &Value {
        &self.func
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for CmpToKey {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.func.drop_with_heap(heap);
    }
}
