//! Callable wrapper types returned by the `operator` module.
//!
//! These wrapper types are heap-allocated so they can store pre-applied
//! arguments and be reused across calls, mirroring CPython's `operator`
//! callables like `itemgetter`, `attrgetter`, and `methodcaller`.

use crate::{
    heap::{DropWithHeap, Heap},
    resource::ResourceTracker,
    value::Value,
};

/// A callable that returns one or more items from its argument.
///
/// This is the heap representation of `operator.itemgetter(*items)`.
/// The getter stores the requested item keys and, when called,
/// indexes the provided object with each key.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ItemGetter {
    pub(crate) items: Vec<Value>,
}

impl ItemGetter {
    /// Creates a new `ItemGetter` with the provided item keys.
    pub fn new(items: Vec<Value>) -> Self {
        Self { items }
    }

    /// Returns the stored item keys.
    pub fn items(&self) -> &[Value] {
        &self.items
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for ItemGetter {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        for item in self.items {
            item.drop_with_heap(heap);
        }
    }
}

/// A callable that returns one or more attributes from its argument.
///
/// This is the heap representation of `operator.attrgetter(*attrs)`.
/// The getter stores attribute name values and, when called, retrieves
/// each attribute (supporting dotted names like "a.b").
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttrGetter {
    pub(crate) attrs: Vec<Value>,
}

impl AttrGetter {
    /// Creates a new `AttrGetter` with the provided attribute name values.
    pub fn new(attrs: Vec<Value>) -> Self {
        Self { attrs }
    }

    /// Returns the stored attribute name values.
    pub fn attrs(&self) -> &[Value] {
        &self.attrs
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for AttrGetter {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        for attr in self.attrs {
            attr.drop_with_heap(heap);
        }
    }
}

/// A callable that invokes a named method with pre-applied arguments.
///
/// This is the heap representation of `operator.methodcaller(name, *args, **kwargs)`.
/// The wrapper stores the method name and arguments, and when called,
/// looks up the method on the provided object and invokes it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct MethodCaller {
    pub(crate) name: Value,
    pub(crate) args: Vec<Value>,
    pub(crate) kwargs: Vec<(Value, Value)>,
}

impl MethodCaller {
    /// Creates a new `MethodCaller` with the provided name, args, and kwargs.
    pub fn new(name: Value, args: Vec<Value>, kwargs: Vec<(Value, Value)>) -> Self {
        Self { name, args, kwargs }
    }

    /// Returns the stored method name value.
    pub fn name(&self) -> &Value {
        &self.name
    }

    /// Returns the stored positional arguments.
    pub fn args(&self) -> &[Value] {
        &self.args
    }

    /// Returns the stored keyword arguments.
    pub fn kwargs(&self) -> &[(Value, Value)] {
        &self.kwargs
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for MethodCaller {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.name.drop_with_heap(heap);
        for arg in self.args {
            arg.drop_with_heap(heap);
        }
        for (key, value) in self.kwargs {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
    }
}
