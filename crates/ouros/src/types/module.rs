//! Python module type for representing imported modules.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StringId},
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, PyTrait, Str},
    value::{EitherStr, Value},
};

/// A Python module with a name and attribute dictionary.
///
/// Modules in Ouros are simplified compared to CPython - they just have a name
/// and a dictionary of attributes. This is sufficient for built-in modules like
/// `sys` and `typing` where we control the available attributes.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Module {
    /// The module name (e.g., "sys", "typing").
    name: StringId,
    /// The module's attributes (e.g., `version`, `platform` for `sys`).
    attrs: Dict,
}

impl Module {
    /// Creates a new module with an empty attributes dictionary.
    ///
    /// The module name must be pre-interned during the prepare phase.
    ///
    /// # Panics
    ///
    /// Panics if the module name string has not been pre-interned.
    pub fn new(name: impl Into<StringId>) -> Self {
        Self {
            name: name.into(),
            attrs: Dict::new(),
        }
    }

    /// Returns the module's name StringId.
    pub fn name(&self) -> StringId {
        self.name
    }

    /// Returns a reference to the module's attribute dictionary.
    pub fn attrs(&self) -> &Dict {
        &self.attrs
    }

    /// Sets an attribute in the module's dictionary.
    ///
    /// The attribute name must be pre-interned during the prepare phase.
    ///
    /// # Panics
    ///
    /// Panics if the attribute name string has not been pre-interned.
    pub fn set_attr(
        &mut self,
        name: impl Into<StringId>,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) {
        let key = Value::InternString(name.into());
        // Unwrap is safe because InternString keys are always hashable.
        // If the key already exists, Dict::set transfers ownership of the replaced
        // value back to us, so we must decrement it explicitly.
        if let Some(old_value) = self.attrs.set(key, value, heap, interns).unwrap() {
            old_value.drop_with_heap(heap);
        }
    }

    /// Sets an attribute in the module's dictionary using a heap string key.
    ///
    /// This is useful for dynamic attribute names that are not represented by
    /// `StaticStrings`. Also available as `set_attr_text` for compatibility.
    pub fn set_attr_str(
        &mut self,
        name: &str,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<(), ResourceError> {
        let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        if let Some(old) = self
            .attrs
            .set(Value::Ref(key_id), value, heap, interns)
            .expect("string keys are always hashable")
        {
            old.drop_with_heap(heap);
        }
        Ok(())
    }

    /// Alias for `set_attr_str` for backwards compatibility.
    pub fn set_attr_text(
        &mut self,
        name: &str,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<(), ResourceError> {
        self.set_attr_str(name, value, heap, interns)
    }

    /// Looks up an attribute by name in the module's attribute dictionary.
    ///
    /// Returns `Some(value)` if the attribute exists, `None` otherwise.
    /// The returned value is cloned with correct ownership semantics for callers.
    pub fn get_attr(
        &self,
        attr_value: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<Value> {
        // Dict::get returns Result because of hash computation, but InternString keys
        // are always hashable, so unwrap is safe here.
        self.attrs
            .get(attr_value, heap, interns)
            .ok()
            .flatten()
            .map(|value| value.clone_with_heap(heap))
    }

    /// Returns whether this module has any heap references in its attributes.
    pub fn has_refs(&self) -> bool {
        self.attrs.has_refs()
    }

    /// Drops this module's heap-backed attributes with proper refcount bookkeeping.
    ///
    /// Use this when a partially constructed module must be discarded before it is
    /// allocated into the heap, so all attribute keys/values release their refs.
    pub(crate) fn drop_with_heap(mut self, heap: &mut Heap<impl ResourceTracker>) {
        self.attrs.drop_all_entries(heap);
    }

    /// Collects child HeapIds for reference counting.
    pub fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.attrs.py_dec_ref_ids(stack);
    }

    /// Gets an attribute by string ID for the `py_getattr` trait method.
    ///
    /// Returns the attribute value if found, or `None` if the attribute doesn't exist.
    /// For `Property` values, invokes the property getter rather than returning
    /// the Property itself - this implements Python's descriptor protocol.
    pub fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<AttrCallResult> {
        let value = self.attrs.get_by_str(interns.get_str(attr_id), heap, interns)?;

        // If the value is a Property, invoke its getter to compute the actual value
        if let Value::Property(prop) = *value {
            Some(prop.get())
        } else {
            Some(AttrCallResult::Value(value.clone_with_heap(heap)))
        }
    }

    /// Calls an attribute as a function on this module.
    ///
    /// Modules don't have methods - they have callable attributes. This looks up
    /// the attribute and calls it if it's a `ModuleFunction`.
    ///
    /// Returns `AttrCallResult` because module functions may need OS operations
    /// (e.g., `os.getenv()`) that require host involvement.
    pub fn py_call_attr_raw(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<AttrCallResult> {
        let mut args_guard = HeapGuard::new(args, heap);

        let attr_key = match attr {
            EitherStr::Interned(id) => Value::InternString(*id),
            EitherStr::Heap(s) => {
                // Module attributes are always interned, so owned strings won't match
                return Err(ExcType::attribute_error_module(interns.get_str(self.name), s));
            }
        };

        match self.get_attr(&attr_key, args_guard.heap(), interns) {
            Some(Value::ModuleFunction(mf)) => {
                let (args, _heap) = args_guard.into_parts();
                Ok(AttrCallResult::CallFunction(Value::ModuleFunction(mf), args))
            }
            Some(func) => {
                // Defer callable validation/execution to the VM so modules can expose class objects too.
                if let Value::Ref(id) = &func {
                    args_guard.heap().inc_ref(*id);
                }
                let (args, _heap) = args_guard.into_parts();
                Ok(AttrCallResult::CallFunction(func, args))
            }
            None => Err(ExcType::attribute_error_module(
                interns.get_str(self.name),
                attr.as_str(interns),
            )),
        }
    }
}

/// Returns whether a module attribute value is callable.
fn module_attr_is_callable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::ClassSubclasses(_)
            | HeapData::ClassGetItem(_)
            | HeapData::FunctionGet(_)
            | HeapData::WeakRef(_)
            | HeapData::ClassObject(_)
            | HeapData::BoundMethod(_)
            | HeapData::Partial(_)
            | HeapData::SingleDispatch(_)
            | HeapData::SingleDispatchRegister(_)
            | HeapData::SingleDispatchMethod(_)
            | HeapData::CmpToKey(_)
            | HeapData::ItemGetter(_)
            | HeapData::AttrGetter(_)
            | HeapData::MethodCaller(_)
            | HeapData::PropertyAccessor(_)
            | HeapData::Closure(_, _, _)
            | HeapData::FunctionDefaults(_, _)
            | HeapData::ObjectNewImpl(_) => true,
            HeapData::Instance(instance) => {
                let class_id = instance.class_id();
                let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
                    return false;
                };
                class_obj.mro_has_attr("__call__", class_id, heap, interns)
            }
            _ => false,
        },
        _ => false,
    }
}
