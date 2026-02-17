//! Helper callable types used by the `functools` module.
//!
//! This module provides heap-allocated callable wrappers for:
//! - `lru_cache` decorator factories and cache wrappers
//! - `wraps`/`update_wrapper` attribute-copying wrappers
//! - `total_ordering` generated comparison methods

use std::fmt::Write;

use ahash::AHashSet;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, Dict, NamedTuple, PyTrait, Type},
    value::{EitherStr, Value},
};

/// A callable LRU cache wrapper or decorator factory.
///
/// When `func` is `None`, the object acts as a decorator factory and expects
/// a single callable argument, returning a new cache wrapper.
/// When `func` is `Some`, calls are cached based on argument keys.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct LruCache {
    /// Wrapped callable, if this is an active cache wrapper.
    pub(crate) func: Option<Value>,
    /// Cache mapping from argument keys to results.
    pub(crate) cache: Dict,
    /// LRU order list (oldest first) storing cache keys.
    pub(crate) order: Vec<Value>,
    /// Maximum size of the cache; `None` for unbounded.
    pub(crate) maxsize: Option<usize>,
    /// Whether argument types are part of the cache key.
    pub(crate) typed: bool,
    /// Number of cache hits.
    pub(crate) hits: usize,
    /// Number of cache misses.
    pub(crate) misses: usize,
}

impl LruCache {
    /// Creates a new cache wrapper or factory with the given maxsize.
    #[must_use]
    pub fn new(maxsize: Option<usize>, typed: bool, func: Option<Value>) -> Self {
        Self {
            func,
            cache: Dict::new(),
            order: Vec::new(),
            maxsize,
            typed,
            hits: 0,
            misses: 0,
        }
    }

    /// Returns the wrapped function, if any.
    #[must_use]
    #[expect(dead_code, reason = "part of public API for LruCache type")]
    pub fn func(&self) -> Option<&Value> {
        self.func.as_ref()
    }

    /// Returns the cache size limit.
    #[must_use]
    #[expect(dead_code, reason = "part of public API for LruCache type")]
    pub fn maxsize(&self) -> Option<usize> {
        self.maxsize
    }

    /// Returns whether the cache is typed.
    #[must_use]
    pub fn typed(&self) -> bool {
        self.typed
    }

    /// Returns the current number of cached entries.
    #[must_use]
    pub fn currsize(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if the cache stores any heap references.
    #[must_use]
    pub fn has_refs(&self) -> bool {
        self.cache.has_refs()
            || self.order.iter().any(|v| matches!(v, Value::Ref(_)))
            || self.func.as_ref().is_some_and(|v| matches!(v, Value::Ref(_)))
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for LruCache {
    fn drop_with_heap(mut self, heap: &mut Heap<T>) {
        if let Some(func) = self.func {
            func.drop_with_heap(heap);
        }
        for key in self.order {
            key.drop_with_heap(heap);
        }
        self.cache.drop_all_entries(heap);
    }
}

impl PyTrait for LruCache {
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
        if let Some(Value::Ref(id)) = &self.func {
            stack.push(*id);
        }
        for key in &mut self.order {
            key.py_dec_ref_ids(stack);
        }
        self.cache.py_dec_ref_ids(stack);
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<functools.lru_cache object>")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.order.len() * std::mem::size_of::<Value>()
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr.as_str(interns) {
            "cache_info" => {
                args.check_zero_args("cache_info", heap)?;
                let maxsize = if let Some(maxsize) = self.maxsize {
                    Value::Int(i64::try_from(maxsize).unwrap_or(i64::MAX))
                } else {
                    Value::None
                };
                let info = NamedTuple::new(
                    "CacheInfo".to_owned(),
                    vec![
                        "hits".to_owned().into(),
                        "misses".to_owned().into(),
                        "maxsize".to_owned().into(),
                        "currsize".to_owned().into(),
                    ],
                    vec![
                        Value::Int(i64::try_from(self.hits).unwrap_or(i64::MAX)),
                        Value::Int(i64::try_from(self.misses).unwrap_or(i64::MAX)),
                        maxsize,
                        Value::Int(i64::try_from(self.currsize()).unwrap_or(i64::MAX)),
                    ],
                );
                let info_id = heap.allocate(HeapData::NamedTuple(info))?;
                Ok(Value::Ref(info_id))
            }
            "cache_clear" => {
                args.check_zero_args("cache_clear", heap)?;
                for key in self.order.drain(..) {
                    key.drop_with_heap(heap);
                }
                self.cache.drop_all_entries(heap);
                self.hits = 0;
                self.misses = 0;
                Ok(Value::None)
            }
            "cache_parameters" => {
                args.check_zero_args("cache_parameters", heap)?;
                let mut result = Dict::new();
                let maxsize_key = Value::Ref(heap.allocate(HeapData::Str("maxsize".into()))?);
                let maxsize_value = if let Some(maxsize) = self.maxsize {
                    Value::Int(i64::try_from(maxsize).unwrap_or(i64::MAX))
                } else {
                    Value::None
                };
                if let Some(old) = result.set(maxsize_key, maxsize_value, heap, interns)? {
                    old.drop_with_heap(heap);
                }

                let typed_key = Value::Ref(heap.allocate(HeapData::Str("typed".into()))?);
                if let Some(old) = result.set(typed_key, Value::Bool(self.typed), heap, interns)? {
                    old.drop_with_heap(heap);
                }

                let result_id = heap.allocate(HeapData::Dict(result))?;
                Ok(Value::Ref(result_id))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(self.py_type(heap), attr.as_str(interns)))
            }
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        if StaticStrings::from_string_id(attr_id) == Some(StaticStrings::DunderWrapped)
            && let Some(func) = &self.func
        {
            return Ok(Some(AttrCallResult::Value(func.clone_with_heap(heap))));
        }
        Ok(None)
    }
}

/// A callable wrapper that exposes updated metadata for a wrapped function.
///
/// This is the runtime object returned by `update_wrapper` and `wraps` to
/// provide `__name__`, `__module__`, `__qualname__`, `__doc__`, and
/// `__wrapped__` attributes while delegating calls to the wrapper function.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct FunctionWrapper {
    /// The wrapper function to call.
    pub(crate) wrapper: Value,
    /// The wrapped function (exposed via `__wrapped__`).
    pub(crate) wrapped: Value,
    /// Copied metadata values.
    pub(crate) name: Value,
    pub(crate) module: Value,
    pub(crate) qualname: Value,
    pub(crate) doc: Value,
}

impl FunctionWrapper {
    /// Constructs a new FunctionWrapper with explicitly provided metadata values.
    #[must_use]
    pub fn new(wrapper: Value, wrapped: Value, name: Value, module: Value, qualname: Value, doc: Value) -> Self {
        Self {
            wrapper,
            wrapped,
            name,
            module,
            qualname,
            doc,
        }
    }

    /// Returns a reference to the wrapper callable.
    #[must_use]
    #[expect(dead_code, reason = "part of public API for FunctionWrapper type")]
    pub fn wrapper(&self) -> &Value {
        &self.wrapper
    }

    /// Returns a reference to the wrapped callable.
    #[must_use]
    #[expect(dead_code, reason = "part of public API for FunctionWrapper type")]
    pub fn wrapped(&self) -> &Value {
        &self.wrapped
    }

    /// Builds a FunctionWrapper by copying metadata from `wrapped`.
    pub fn from_wrapped(
        wrapper: Value,
        wrapped: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let name = get_attr_or_none(&wrapped, StaticStrings::DunderName, heap, interns)?;
        let module = get_attr_or_none(&wrapped, StaticStrings::DunderModule, heap, interns)?;
        let qualname = get_attr_or_none(&wrapped, StaticStrings::DunderQualname, heap, interns)?;
        let doc = get_attr_or_none(&wrapped, StaticStrings::DunderDoc, heap, interns)?;
        Ok(Self::new(wrapper, wrapped, name, module, qualname, doc))
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for FunctionWrapper {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.wrapper.drop_with_heap(heap);
        self.wrapped.drop_with_heap(heap);
        self.name.drop_with_heap(heap);
        self.module.drop_with_heap(heap);
        self.qualname.drop_with_heap(heap);
        self.doc.drop_with_heap(heap);
    }
}

impl PyTrait for FunctionWrapper {
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
        self.wrapper.py_dec_ref_ids(stack);
        self.wrapped.py_dec_ref_ids(stack);
        self.name.py_dec_ref_ids(stack);
        self.module.py_dec_ref_ids(stack);
        self.qualname.py_dec_ref_ids(stack);
        self.doc.py_dec_ref_ids(stack);
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
        self.wrapper.py_repr_fmt(f, heap, heap_ids, interns)
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_getattr(
        &self,
        attr_id: crate::intern::StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<crate::types::AttrCallResult>> {
        let attr = StaticStrings::from_string_id(attr_id);
        match attr {
            Some(StaticStrings::DunderName) => Ok(Some(crate::types::AttrCallResult::Value(
                self.name.clone_with_heap(heap),
            ))),
            Some(StaticStrings::DunderModule) => Ok(Some(crate::types::AttrCallResult::Value(
                self.module.clone_with_heap(heap),
            ))),
            Some(StaticStrings::DunderQualname) => Ok(Some(crate::types::AttrCallResult::Value(
                self.qualname.clone_with_heap(heap),
            ))),
            Some(StaticStrings::DunderDoc) => Ok(Some(crate::types::AttrCallResult::Value(
                self.doc.clone_with_heap(heap),
            ))),
            Some(StaticStrings::DunderWrapped) => Ok(Some(crate::types::AttrCallResult::Value(
                self.wrapped.clone_with_heap(heap),
            ))),
            _ => self.wrapper.py_getattr(attr_id, heap, interns).map(Some),
        }
    }
}

/// A decorator factory returned by `functools.wraps`.
///
/// The factory holds the wrapped callable and returns a FunctionWrapper
/// when invoked with a wrapper function.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Wraps {
    /// The wrapped callable whose metadata should be copied.
    pub(crate) wrapped: Value,
    /// Attributes copied from wrapped to wrapper.
    pub(crate) assigned: Vec<StringId>,
    /// Attributes updated on wrapper from wrapped.
    pub(crate) updated: Vec<StringId>,
}

impl Wraps {
    /// Creates a new wraps decorator factory.
    #[must_use]
    pub fn new(wrapped: Value, assigned: Vec<StringId>, updated: Vec<StringId>) -> Self {
        Self {
            wrapped,
            assigned,
            updated,
        }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for Wraps {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.wrapped.drop_with_heap(heap);
    }
}

impl PyTrait for Wraps {
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
        self.wrapped.py_dec_ref_ids(stack);
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<functools.wraps object>")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// A generated comparison method created by `functools.total_ordering`.
///
/// The method calls a base comparison and optionally swaps operands and/or
/// negates the boolean result to implement missing ordering dunders.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct TotalOrderingMethod {
    /// Base comparison dunder to call (`__lt__`, `__le__`, `__gt__`, `__ge__`).
    pub(crate) base: StaticStrings,
    /// Whether to call the base method on the other operand (swap operands).
    pub(crate) swap: bool,
    /// Whether to negate the boolean result.
    pub(crate) negate: bool,
}

impl TotalOrderingMethod {
    /// Creates a new generated ordering method.
    #[must_use]
    pub fn new(base: StaticStrings, swap: bool, negate: bool) -> Self {
        Self { base, swap, negate }
    }
}

impl PyTrait for TotalOrderingMethod {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Function
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {}

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<functools.total_ordering method>")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Sentinel object used by `functools.partial` argument templates.
///
/// This mirrors Python 3.14's `functools.Placeholder` and marks positional
/// slots that should be filled by call-site positional arguments.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct Placeholder;

/// Descriptor implementing `functools.cached_property`.
///
/// The descriptor stores the wrapped function and the attribute name used for
/// instance-dict caching. The VM handles descriptor calls and cache write-back.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CachedProperty {
    /// Wrapped function called on first instance access.
    pub(crate) func: Value,
    /// Attribute name used for caching in `instance.__dict__`.
    pub(crate) attr_name: Option<String>,
}

impl CachedProperty {
    /// Creates a new cached-property descriptor.
    #[must_use]
    pub fn new(func: Value, attr_name: Option<String>) -> Self {
        Self { func, attr_name }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for CachedProperty {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.func.drop_with_heap(heap);
    }
}

/// Callable object returned by `functools.singledispatch`.
///
/// Stores a default implementation and a registration table keyed by type-like
/// values (`int`, user classes, etc.). Dispatch behavior is implemented in the VM.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SingleDispatch {
    /// Default implementation used when no specialized registration matches.
    pub(crate) func: Value,
    /// Registration table entries as `(type_key, implementation)`.
    pub(crate) registry: Vec<(Value, Value)>,
    /// Positional index used for dispatch selection (0 for functions, 1 for methods).
    pub(crate) dispatch_index: usize,
}

impl SingleDispatch {
    /// Creates a new single-dispatch callable.
    #[must_use]
    pub fn new(func: Value, dispatch_index: usize) -> Self {
        Self {
            func,
            registry: Vec::new(),
            dispatch_index,
        }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for SingleDispatch {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.func.drop_with_heap(heap);
        for (cls, func) in self.registry {
            cls.drop_with_heap(heap);
            func.drop_with_heap(heap);
        }
    }
}

impl PyTrait for SingleDispatch {
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
        for (cls, func) in &mut self.registry {
            cls.py_dec_ref_ids(stack);
            func.py_dec_ref_ids(stack);
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<functools.singledispatch object>")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.registry.len() * std::mem::size_of::<(Value, Value)>()
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        if interns.get_str(attr_id) != "registry" {
            return Ok(None);
        }

        let mut registry = Dict::new();
        if let Some(old) = registry.set(
            Value::Builtin(Builtins::Type(Type::Object)),
            self.func.clone_with_heap(heap),
            heap,
            interns,
        )? {
            old.drop_with_heap(heap);
        }

        for (cls, func) in &self.registry {
            if let Some(old) = registry.set(cls.clone_with_heap(heap), func.clone_with_heap(heap), heap, interns)? {
                old.drop_with_heap(heap);
            }
        }

        let dict_id = heap.allocate(HeapData::Dict(registry))?;
        Ok(Some(AttrCallResult::Value(Value::Ref(dict_id))))
    }
}

/// Decorator callable returned by `singledispatch.register(cls)`.
///
/// Calling this object with a function registers it on the associated
/// dispatcher and returns the function unchanged.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SingleDispatchRegister {
    /// Owning `SingleDispatch` object.
    pub(crate) dispatcher: Value,
    /// Type-like key used for registration.
    pub(crate) cls: Value,
}

impl SingleDispatchRegister {
    /// Creates a registration decorator wrapper.
    #[must_use]
    pub fn new(dispatcher: Value, cls: Value) -> Self {
        Self { dispatcher, cls }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for SingleDispatchRegister {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.dispatcher.drop_with_heap(heap);
        self.cls.drop_with_heap(heap);
    }
}

/// Descriptor wrapper returned by `functools.singledispatchmethod`.
///
/// Holds an underlying `SingleDispatch` callable and relies on descriptor
/// handling in the VM to bind the receiver before invocation.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SingleDispatchMethod {
    /// Wrapped `SingleDispatch` callable.
    pub(crate) dispatcher: Value,
}

impl SingleDispatchMethod {
    /// Creates a new single-dispatch method descriptor.
    #[must_use]
    pub fn new(dispatcher: Value) -> Self {
        Self { dispatcher }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for SingleDispatchMethod {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.dispatcher.drop_with_heap(heap);
    }
}

/// Descriptor wrapper returned by `functools.partialmethod`.
///
/// At attribute access time this binds the underlying descriptor/callable to
/// the receiver and returns a `functools.partial` with stored arguments.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PartialMethod {
    /// Wrapped callable or descriptor.
    pub(crate) func: Value,
    /// Stored positional arguments for the produced partial.
    pub(crate) args: Vec<Value>,
    /// Stored keyword arguments for the produced partial.
    pub(crate) kwargs: Vec<(Value, Value)>,
}

impl PartialMethod {
    /// Creates a new partialmethod descriptor.
    #[must_use]
    pub fn new(func: Value, args: Vec<Value>, kwargs: Vec<(Value, Value)>) -> Self {
        Self { func, args, kwargs }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for PartialMethod {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.func.drop_with_heap(heap);
        for arg in self.args {
            arg.drop_with_heap(heap);
        }
        for (key, value) in self.kwargs {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
    }
}

/// Attempts to read an attribute from a value, returning None on AttributeError.
fn get_attr_or_none(
    value: &Value,
    attr: StaticStrings,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let attr_id = attr.into();
    match value.py_getattr(attr_id, heap, interns) {
        Ok(crate::types::AttrCallResult::Value(v)) => Ok(v),
        Ok(_) => Ok(Value::None),
        Err(crate::exception_private::RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::AttributeError => {
            Ok(Value::None)
        }
        Err(e) => Err(e),
    }
}
