//! Python class object and instance types.
//!
//! `ClassObject` represents the class itself (created by `class Foo: ...`).
//! `Instance` represents instances created by calling the class (`Foo()`).
//!
//! # Scoping
//!
//! Python class body scope is special:
//! - Class body variables are NOT visible to methods
//! - Methods must use `self.x` or `ClassName.x` to access class-level variables
//! - The class body CAN capture variables from enclosing function scopes
//!
//! # Attribute Access
//!
//! - Instance attributes are checked first (`self.x`), then class attributes
//! - Class attributes are shared across all instances
//! - Setting an attribute on an instance creates an instance-level attribute

use std::{borrow::Cow, fmt::Write};

use ahash::{AHashMap, AHashSet};

use super::{Dict, PyTrait, Type};
use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::{MAX_INHERITANCE_DEPTH, ResourceTracker},
    types::AttrCallResult,
    value::{EitherStr, Value},
};

/// Weakly tracked subclass entry for `type.__subclasses__()`.
///
/// Stores the heap ID plus a unique class UID so we can detect stale entries
/// after heap slot reuse without holding a strong reference.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct SubclassEntry {
    /// Heap ID of the subclass (may be stale after GC/free).
    class_id: HeapId,
    /// Unique class UID captured at registration time.
    class_uid: u64,
}

impl SubclassEntry {
    /// Creates a new subclass registry entry.
    #[must_use]
    pub fn new(class_id: HeapId, class_uid: u64) -> Self {
        Self { class_id, class_uid }
    }

    /// Returns the registered class heap ID.
    #[must_use]
    pub fn class_id(self) -> HeapId {
        self.class_id
    }

    /// Returns the registered class UID.
    #[must_use]
    pub fn class_uid(self) -> u64 {
        self.class_uid
    }
}

/// A Python class object, created by executing a `class` statement.
///
/// Contains the class name, base classes, and a namespace dict holding
/// class attributes and method definitions. Methods are stored as
/// `Value::DefFunction` or `Value::Ref` (for closures) entries.
///
/// When called (instantiated), creates an `Instance` with a fresh attrs dict,
/// then calls `__init__` if defined.
#[derive(Debug)]
pub(crate) struct ClassObject {
    /// The class name (e.g., "Foo", "MyClass").
    name: EitherStr,
    /// Unique ID for this class, used to validate subclass registry entries.
    class_uid: u64,
    /// Metaclass for this class (usually `type`).
    ///
    /// Stored as a Value so it can be either a builtin type or a user-defined
    /// class object. If this is a heap reference, the caller must have incremented
    /// its refcount before constructing the ClassObject.
    metaclass: Value,
    /// Class namespace containing class attributes and method definitions.
    /// Keys are interned string names, values are the attribute/method values.
    namespace: Dict,
    /// Direct base classes (the classes listed in `class Foo(Base1, Base2): ...`).
    /// Empty for classes with no explicit bases (implicitly inherit from object).
    bases: Vec<HeapId>,
    /// Method Resolution Order computed by C3 linearization.
    /// Includes this class itself as the first entry, followed by bases in MRO order.
    /// Does NOT include a sentinel for `object` - that is handled as a special case.
    mro: Vec<HeapId>,
    /// `__slots__` defined on this class, if any.
    ///
    /// Stored as plain strings to allow runtime mangling without relying on the interner.
    /// Only contains slots defined directly on *this* class, not inherited slots.
    slots: Option<Vec<String>>,
    /// Full slot layout (including inherited slots, excluding `__dict__`/`__weakref__`).
    slot_layout: Vec<String>,
    /// Slot name -> index in `slot_layout`.
    slot_indices: AHashMap<String, usize>,
    /// Whether instances of this class have an instance `__dict__`.
    instance_has_dict: bool,
    /// Whether instances of this class have a `__weakref__` slot.
    instance_has_weakref: bool,
    /// Direct subclasses registered for `type.__subclasses__()`.
    subclasses: Vec<SubclassEntry>,
}

impl ClassObject {
    /// Creates a new class object with base classes and MRO.
    ///
    /// # Arguments
    /// * `name` - The class name
    /// * `namespace` - Dict of class attributes and methods
    /// * `bases` - Direct base class HeapIds
    /// * `mro` - Full MRO (computed by C3 linearization), including self as first element
    #[must_use]
    pub fn new(
        name: impl Into<EitherStr>,
        class_uid: u64,
        metaclass: Value,
        namespace: Dict,
        bases: Vec<HeapId>,
        mro: Vec<HeapId>,
    ) -> Self {
        Self {
            name: name.into(),
            class_uid,
            metaclass,
            namespace,
            bases,
            mro,
            slots: None,
            slot_layout: Vec::new(),
            slot_indices: AHashMap::new(),
            instance_has_dict: true,
            instance_has_weakref: true,
            subclasses: Vec::new(),
        }
    }

    /// Returns the class name.
    #[must_use]
    pub fn name<'a>(&'a self, interns: &'a Interns) -> &'a str {
        self.name.as_str(interns)
    }

    /// Returns the unique class UID.
    #[must_use]
    pub fn class_uid(&self) -> u64 {
        self.class_uid
    }

    /// Returns the metaclass value for this class.
    #[must_use]
    pub fn metaclass(&self) -> &Value {
        &self.metaclass
    }

    /// Returns a reference to the class namespace dict.
    #[must_use]
    pub fn namespace(&self) -> &Dict {
        &self.namespace
    }

    /// Returns the direct base class HeapIds.
    #[must_use]
    pub fn bases(&self) -> &[HeapId] {
        &self.bases
    }

    /// Returns the Method Resolution Order (MRO) as a slice of HeapIds.
    /// The first element is always this class itself.
    #[must_use]
    pub fn mro(&self) -> &[HeapId] {
        &self.mro
    }

    /// Sets the MRO after initial allocation.
    ///
    /// Called by `finalize_class_body` after the class HeapId is known,
    /// since the MRO includes the class itself as the first entry.
    pub fn set_mro(&mut self, mro: Vec<HeapId>) {
        self.mro = mro;
    }

    /// Sets the `__slots__` for this class.
    pub fn set_slots(&mut self, slots: Vec<String>) {
        self.slots = Some(slots);
    }

    /// Returns the `__slots__` defined on this class, if any.
    ///
    /// Retained for feature checks and error reporting.
    #[must_use]
    #[expect(dead_code)]
    pub fn slots(&self) -> Option<&[String]> {
        self.slots.as_deref()
    }

    /// Checks whether this class defines `__slots__` directly.
    ///
    /// Used to determine whether instances should restrict attribute access.
    #[expect(dead_code)]
    pub fn has_slots(&self) -> bool {
        self.slots.is_some()
    }

    /// Returns the full slot layout for instances (including inherited slots).
    #[must_use]
    pub fn slot_layout(&self) -> &[String] {
        &self.slot_layout
    }

    /// Returns the slot index for a name, if it is a slot on this class.
    #[must_use]
    pub fn slot_index(&self, name: &str) -> Option<usize> {
        self.slot_indices.get(name).copied()
    }

    /// Returns whether instances of this class have a `__dict__`.
    #[must_use]
    pub fn instance_has_dict(&self) -> bool {
        self.instance_has_dict
    }

    /// Returns whether instances of this class have a `__weakref__` slot.
    #[must_use]
    pub fn instance_has_weakref(&self) -> bool {
        self.instance_has_weakref
    }

    /// Registers a direct subclass in the weak registry.
    pub fn register_subclass(&mut self, class_id: HeapId, class_uid: u64) {
        self.subclasses.push(SubclassEntry::new(class_id, class_uid));
    }

    /// Returns the direct subclass registry entries.
    #[must_use]
    pub fn subclasses(&self) -> &[SubclassEntry] {
        &self.subclasses
    }

    /// Replaces the subclass registry entries.
    ///
    /// Used to prune stale entries after heap slot reuse.
    pub fn set_subclasses(&mut self, subclasses: Vec<SubclassEntry>) {
        self.subclasses = subclasses;
    }

    /// Sets the finalized slot layout and instance flags.
    pub fn set_slot_layout(
        &mut self,
        slot_layout: Vec<String>,
        slot_indices: AHashMap<String, usize>,
        instance_has_dict: bool,
        instance_has_weakref: bool,
    ) {
        self.slot_layout = slot_layout;
        self.slot_indices = slot_indices;
        self.instance_has_dict = instance_has_dict;
        self.instance_has_weakref = instance_has_weakref;
    }

    /// Overrides instance dict/weakref flags (used for builtin class wrappers).
    pub fn set_instance_flags(&mut self, instance_has_dict: bool, instance_has_weakref: bool) {
        self.instance_has_dict = instance_has_dict;
        self.instance_has_weakref = instance_has_weakref;
    }

    /// Returns whether this class object contains any heap references.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        !self.bases.is_empty() || !self.mro.is_empty() || self.namespace.has_refs()
    }

    /// Looks up an attribute in the class namespace and returns a cloned value.
    ///
    /// Not currently called — existing attribute lookups use `namespace.get_by_str()`
    /// directly (with descriptor unwrapping). Retained as
    /// class infrastructure for cases needing a simple owned-value lookup.
    #[expect(dead_code)]
    pub fn get_attr(&self, attr_name: &str, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<Value> {
        self.namespace
            .get_by_str(attr_name, heap, interns)
            .map(|v| v.clone_with_heap(heap))
    }

    /// Sets an attribute in the class namespace.
    ///
    /// Returns the old value if the attribute existed (caller must drop it).
    pub fn set_attr(
        &mut self,
        name: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        self.namespace.set(name, value, heap, interns)
    }

    /// Looks up an attribute by walking the MRO (this class first, then bases in MRO order).
    ///
    /// Returns the cloned value (with refcounts updated) and the HeapId of the class
    /// where it was found.
    pub fn mro_lookup_attr(
        &self,
        attr_name: &str,
        self_id: HeapId,
        heap: &Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<(Value, HeapId)> {
        // Check own namespace first
        if let Some(value) = self.namespace.get_by_str(attr_name, heap, interns) {
            return Some((value.clone_with_heap(heap), self_id));
        }
        if let Some(value) = builtin_exception_method_for_attr(self_id, attr_name, heap) {
            return Some((value, self_id));
        }
        // Walk the MRO (skip self which is mro[0])
        for &base_id in &self.mro[1..] {
            if let HeapData::ClassObject(base_cls) = heap.get(base_id)
                && let Some(value) = base_cls.namespace.get_by_str(attr_name, heap, interns)
            {
                return Some((value.clone_with_heap(heap), base_id));
            }
            if let Some(value) = builtin_exception_method_for_attr(base_id, attr_name, heap) {
                return Some((value, base_id));
            }
        }
        None
    }

    /// Checks if an attribute exists in this class's namespace or MRO.
    ///
    /// Unlike `mro_lookup_attr`, this does not return the value. Use this when you
    /// only need to check for the existence of an attribute (e.g., `__get__`, `__set__`).
    pub fn mro_has_attr(
        &self,
        attr_name: &str,
        self_id: HeapId,
        heap: &Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> bool {
        if self.namespace.get_by_str(attr_name, heap, interns).is_some() {
            return true;
        }
        if builtin_exception_method_for_attr(self_id, attr_name, heap).is_some() {
            return true;
        }
        for &base_id in &self.mro[1..] {
            if let HeapData::ClassObject(base_cls) = heap.get(base_id)
                && base_cls.namespace.get_by_str(attr_name, heap, interns).is_some()
            {
                return true;
            }
            if builtin_exception_method_for_attr(base_id, attr_name, heap).is_some() {
                return true;
            }
        }
        false
    }

    /// Checks if this class (identified by `self_id`) is a subclass of `other_id`.
    ///
    /// A class is considered a subclass of itself.
    pub fn is_subclass_of(&self, self_id: HeapId, other_id: HeapId) -> bool {
        if self_id == other_id {
            return true;
        }
        self.mro.contains(&other_id)
    }
}

impl PyTrait for ClassObject {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Type
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.py_estimate_size()
            + std::mem::size_of::<Value>()
            + self.namespace.py_estimate_size()
            + self.bases.len() * std::mem::size_of::<HeapId>()
            + self.mro.len() * std::mem::size_of::<HeapId>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        self.name == other.name && self.namespace.py_eq(&other.namespace, heap, interns)
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        if let Value::Ref(id) = &self.metaclass {
            stack.push(*id);
            #[cfg(feature = "ref-count-panic")]
            self.metaclass.dec_ref_forget();
        }
        self.namespace.py_dec_ref_ids(stack);
        // Decrement refs for base classes and MRO entries
        for &base_id in &self.bases {
            stack.push(base_id);
        }
        for &mro_id in &self.mro {
            stack.push(mro_id);
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
        interns: &Interns,
    ) -> std::fmt::Result {
        write!(f, "<class '{}'>", self.name(interns))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);

        // Handle special class attributes: __class__, __name__, __bases__, __mro__
        if attr_name == "__class__" {
            return Ok(Some(AttrCallResult::Value(self.metaclass.clone_with_heap(heap))));
        }
        if attr_name == "__name__" {
            let name_val = match &self.name {
                EitherStr::Interned(id) => Value::InternString(*id),
                EitherStr::Heap(s) => {
                    let heap_id = heap.allocate(HeapData::Str(crate::types::Str::from(s.as_str())))?;
                    Value::Ref(heap_id)
                }
            };
            return Ok(Some(AttrCallResult::Value(name_val)));
        }
        if attr_name == "__bases__" {
            // Return a tuple of base class refs
            if self.bases.is_empty() {
                // No explicit bases: implicit (object,)
                let base_values: smallvec::SmallVec<[Value; 3]> =
                    smallvec::smallvec![Value::Builtin(Builtins::Type(crate::types::Type::Object))];
                let tuple_val = crate::types::allocate_tuple(base_values, heap)?;
                return Ok(Some(AttrCallResult::Value(tuple_val)));
            }
            let base_values: smallvec::SmallVec<[Value; 3]> = self
                .bases
                .iter()
                .map(|&id| {
                    if let Some(t) = heap.builtin_type_for_class_id(id) {
                        Value::Builtin(Builtins::Type(t))
                    } else {
                        heap.inc_ref(id);
                        Value::Ref(id)
                    }
                })
                .collect();
            let tuple_val = crate::types::allocate_tuple(base_values, heap)?;
            return Ok(Some(AttrCallResult::Value(tuple_val)));
        }
        if attr_name == "__mro__" {
            // Return a tuple of class refs in MRO order, with `object` appended
            let mut mro_values: smallvec::SmallVec<[Value; 3]> = self
                .mro
                .iter()
                .map(|&id| {
                    if let Some(t) = heap.builtin_type_for_class_id(id) {
                        Value::Builtin(Builtins::Type(t))
                    } else {
                        heap.inc_ref(id);
                        Value::Ref(id)
                    }
                })
                .collect();
            // All classes implicitly end with `object`
            if !self
                .mro
                .iter()
                .any(|&id| heap.builtin_type_for_class_id(id) == Some(crate::types::Type::Object))
            {
                mro_values.push(Value::Builtin(Builtins::Type(crate::types::Type::Object)));
            }
            let tuple_val = crate::types::allocate_tuple(mro_values, heap)?;
            return Ok(Some(AttrCallResult::Value(tuple_val)));
        }

        // __dict__ is handled in Value::py_getattr to return a mappingproxy
        // that reflects the live class namespace.

        // Check own namespace first
        if let Some(value) = self.namespace.get_by_str(attr_name, heap, interns) {
            let unwrapped = unwrap_descriptor_for_class(value, heap);
            return Ok(Some(AttrCallResult::Value(unwrapped)));
        }

        // Walk MRO for attribute lookup (skip self which is mro[0])
        for &base_id in &self.mro[1..] {
            let found = match heap.get(base_id) {
                HeapData::ClassObject(base_cls) => base_cls.namespace.get_by_str(attr_name, heap, interns),
                _ => None,
            };
            if let Some(value) = found {
                let unwrapped = unwrap_descriptor_for_class(value, heap);
                return Ok(Some(AttrCallResult::Value(unwrapped)));
            }
        }

        // Special case: if looking for __new__, return the default object.__new__
        if attr_name == "__new__" {
            return Ok(Some(AttrCallResult::ObjectNew));
        }

        Err(ExcType::attribute_error(
            format!("type object '{}'", self.name(interns)),
            attr_name,
        ))
    }
}

/// Custom serde for ClassObject.
impl serde::Serialize for ClassObject {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ClassObject", 12)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("class_uid", &self.class_uid)?;
        state.serialize_field("metaclass", &self.metaclass)?;
        state.serialize_field("namespace", &self.namespace)?;
        state.serialize_field("bases", &self.bases)?;
        state.serialize_field("mro", &self.mro)?;
        state.serialize_field("slots", &self.slots)?;
        state.serialize_field("slot_layout", &self.slot_layout)?;
        state.serialize_field("slot_indices", &self.slot_indices)?;
        state.serialize_field("instance_has_dict", &self.instance_has_dict)?;
        state.serialize_field("instance_has_weakref", &self.instance_has_weakref)?;
        state.serialize_field("subclasses", &self.subclasses)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for ClassObject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Data {
            name: EitherStr,
            #[serde(default = "default_class_uid")]
            class_uid: u64,
            #[serde(default = "default_metaclass_value")]
            metaclass: Value,
            namespace: Dict,
            #[serde(default)]
            bases: Vec<HeapId>,
            #[serde(default)]
            mro: Vec<HeapId>,
            #[serde(default)]
            slots: Option<Vec<String>>,
            #[serde(default)]
            slot_layout: Vec<String>,
            #[serde(default)]
            slot_indices: AHashMap<String, usize>,
            #[serde(default = "default_instance_has_dict")]
            instance_has_dict: bool,
            #[serde(default = "default_instance_has_weakref")]
            instance_has_weakref: bool,
            #[serde(default)]
            subclasses: Vec<SubclassEntry>,
        }
        let data = Data::deserialize(deserializer)?;
        Ok(Self {
            name: data.name,
            class_uid: data.class_uid,
            metaclass: data.metaclass,
            namespace: data.namespace,
            bases: data.bases,
            mro: data.mro,
            slots: data.slots,
            slot_layout: data.slot_layout,
            slot_indices: data.slot_indices,
            instance_has_dict: data.instance_has_dict,
            instance_has_weakref: data.instance_has_weakref,
            subclasses: data.subclasses,
        })
    }
}

/// Default metaclass for deserialization (builtin `type`).
fn default_metaclass_value() -> Value {
    Value::Builtin(Builtins::Type(Type::Type))
}

/// Default for `instance_has_dict` when deserializing older snapshots.
fn default_instance_has_dict() -> bool {
    true
}

/// Default for `instance_has_weakref` when deserializing older snapshots.
fn default_instance_has_weakref() -> bool {
    true
}

fn default_class_uid() -> u64 {
    0
}

/// A Python class instance, created by calling a `ClassObject`.
///
/// Contains a reference to the class (as a HeapId) and instance-specific
/// attributes. Attribute lookup checks instance attrs first, then class attrs.
///
/// # Attribute Lookup Order
/// 1. Instance attributes (`self.__dict__`)
/// 2. Class attributes (from the ClassObject's namespace)
///
/// # Method Binding
/// When a function is looked up through instance attribute access, the VM
/// binds it as a method by passing the instance as the first argument (`self`).
#[derive(Debug)]
pub(crate) struct Instance {
    /// HeapId of the ClassObject this instance belongs to.
    class_id: HeapId,
    /// HeapId of the instance attribute dictionary, if present.
    ///
    /// Stored on the heap to allow `instance.__dict__` to return the live dict.
    attrs_id: Option<HeapId>,
    /// Storage for slot values (excluding `__dict__` and `__weakref__`).
    slot_values: Vec<Value>,
    /// Weak reference handles registered for this instance.
    ///
    /// Stored without incrementing refcounts to avoid keeping weakrefs alive.
    weakref_ids: Vec<HeapId>,
}

impl Instance {
    /// Creates a new instance with the provided attribute dict and slot storage.
    #[must_use]
    pub fn new(class_id: HeapId, attrs_id: Option<HeapId>, slot_values: Vec<Value>, weakref_ids: Vec<HeapId>) -> Self {
        Self {
            class_id,
            attrs_id,
            slot_values,
            weakref_ids,
        }
    }

    /// Returns the HeapId of the class this instance belongs to.
    #[must_use]
    pub fn class_id(&self) -> HeapId {
        self.class_id
    }

    /// Returns the HeapId of the instance attribute dictionary.
    #[must_use]
    pub fn attrs_id(&self) -> Option<HeapId> {
        self.attrs_id
    }

    /// Returns the first registered weakref id, if any.
    #[must_use]
    pub fn weakref_id(&self) -> Option<HeapId> {
        self.weakref_ids.first().copied()
    }

    /// Returns all weakref ids registered for this instance.
    #[must_use]
    pub fn weakref_ids(&self) -> &[HeapId] {
        &self.weakref_ids
    }

    /// Registers a weakref handle for this instance.
    pub fn register_weakref(&mut self, weakref_id: HeapId) {
        if !self.weakref_ids.contains(&weakref_id) {
            self.weakref_ids.push(weakref_id);
        }
    }

    /// Returns whether this instance contains any heap references.
    #[inline]
    #[must_use]
    #[expect(clippy::unused_self)]
    pub fn has_refs(&self) -> bool {
        // Always has at least class_id ref
        true
    }

    /// Sets an attribute on this instance.
    ///
    /// Returns the old value if the attribute existed (caller must drop it).
    pub fn set_attr(
        &mut self,
        name: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        let attr_name = match &name {
            Value::InternString(id) => interns.get_str(*id).to_string(),
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Str(s) => s.as_str().to_string(),
                _ => String::new(),
            },
            _ => String::new(),
        };

        if attr_name == "__dict__" {
            let has_dict = match heap.get(self.class_id) {
                HeapData::ClassObject(cls) => cls.instance_has_dict(),
                _ => false,
            };
            if !has_dict {
                value.drop_with_heap(heap);
                let class_name = self.class_name(heap, interns);
                return Err(ExcType::attribute_error_no_dict_for_setting(
                    &class_name,
                    attr_name.as_str(),
                ));
            }

            let Value::Ref(dict_id) = value else {
                let type_name = value.py_type(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "__dict__ must be set to a dictionary, not a '{type_name}'"
                )));
            };
            if !matches!(heap.get(dict_id), HeapData::Dict(_)) {
                let type_name = heap.get(dict_id).py_type(heap);
                Value::Ref(dict_id).drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "__dict__ must be set to a dictionary, not a '{type_name}'"
                )));
            }

            let old = self.attrs_id.replace(dict_id);
            return Ok(old.map(Value::Ref));
        }

        if attr_name == "__weakref__" {
            value.drop_with_heap(heap);
            let class_name = self.class_name(heap, interns);
            return Err(ExcType::attribute_error_weakref_not_writable(&class_name));
        }

        if let Some(slot_idx) = self.slot_index(attr_name.as_str(), heap) {
            let old = std::mem::replace(&mut self.slot_values[slot_idx], value);
            if matches!(old, Value::Undefined) {
                Ok(None)
            } else {
                Ok(Some(old))
            }
        } else if let Some(attrs_id) = self.attrs_id {
            let class_name = self.class_name(heap, interns);
            if !matches!(heap.get_if_live(attrs_id), Some(HeapData::Dict(_))) {
                value.drop_with_heap(heap);
                return Err(ExcType::attribute_error_no_dict_for_setting(
                    &class_name,
                    attr_name.as_str(),
                ));
            }
            heap.with_entry_mut(attrs_id, |heap, data| {
                let HeapData::Dict(dict) = data else {
                    value.drop_with_heap(heap);
                    return Err(ExcType::attribute_error_no_dict_for_setting(
                        &class_name,
                        attr_name.as_str(),
                    ));
                };
                dict.set(name, value, heap, interns)
            })
        } else {
            value.drop_with_heap(heap);
            let class_name = self.class_name(heap, interns);
            Err(ExcType::attribute_error_no_dict_for_setting(
                &class_name,
                attr_name.as_str(),
            ))
        }
    }

    /// Deletes an attribute from this instance.
    ///
    /// Returns the old value if the attribute existed (caller must drop it).
    /// Returns Ok(None) if the attribute didn't exist.
    pub fn del_attr(
        &mut self,
        name: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<(Value, Value)>> {
        let attr_name = match name {
            Value::InternString(id) => interns.get_str(*id).to_string(),
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Str(s) => s.as_str().to_string(),
                _ => String::new(),
            },
            _ => String::new(),
        };

        if attr_name == "__dict__" {
            let has_dict = match heap.get(self.class_id) {
                HeapData::ClassObject(cls) => cls.instance_has_dict(),
                _ => false,
            };
            if !has_dict {
                let class_name = self.class_name(heap, interns);
                return Err(ExcType::attribute_error_no_dict_for_setting(
                    &class_name,
                    attr_name.as_str(),
                ));
            }
            let old_id = self.attrs_id;
            let new_id = heap.allocate(HeapData::Dict(Dict::new()))?;
            self.attrs_id = Some(new_id);
            let old_value = old_id.map(Value::Ref).unwrap_or(Value::Ref(new_id));
            return Ok(Some((name.clone_with_heap(heap), old_value)));
        }

        if attr_name == "__weakref__" {
            let class_name = self.class_name(heap, interns);
            return Err(ExcType::attribute_error_weakref_not_writable(&class_name));
        }

        if let Some(slot_idx) = self.slot_index(attr_name.as_str(), heap) {
            let old = std::mem::replace(&mut self.slot_values[slot_idx], Value::Undefined);
            if matches!(old, Value::Undefined) {
                let class_name = self.class_name(heap, interns);
                return Err(ExcType::attribute_error(class_name, attr_name.as_str()));
            }
            Ok(Some((name.clone_with_heap(heap), old)))
        } else if let Some(attrs_id) = self.attrs_id {
            let class_name = self.class_name(heap, interns);
            if !matches!(heap.get_if_live(attrs_id), Some(HeapData::Dict(_))) {
                return Err(ExcType::attribute_error_no_dict_for_setting(
                    &class_name,
                    attr_name.as_str(),
                ));
            }
            heap.with_entry_mut(attrs_id, |heap, data| {
                let HeapData::Dict(dict) = data else {
                    return Err(ExcType::attribute_error_no_dict_for_setting(
                        &class_name,
                        attr_name.as_str(),
                    ));
                };
                dict.pop(name, heap, interns)
            })
            .and_then(|opt| {
                if opt.is_some() {
                    Ok(opt)
                } else {
                    Err(ExcType::attribute_error(class_name, attr_name.as_str()))
                }
            })
        } else {
            let class_name = self.class_name(heap, interns);
            Err(ExcType::attribute_error_no_dict_for_setting(
                &class_name,
                attr_name.as_str(),
            ))
        }
    }

    /// Returns a reference to the instance attrs dict, if present.
    #[must_use]
    pub fn attrs<'a>(&self, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a Dict> {
        let attrs_id = self.attrs_id?;
        match heap.get(attrs_id) {
            HeapData::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    /// Returns a mutable reference to the instance attrs dict, if present.
    #[must_use]
    #[expect(dead_code)]
    pub fn attrs_mut<'a>(&'a mut self, heap: &'a mut Heap<impl ResourceTracker>) -> Option<&'a mut Dict> {
        let attrs_id = self.attrs_id?;
        match heap.get_mut(attrs_id) {
            HeapData::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    /// Returns the class name string for error messages.
    fn class_name(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> String {
        match heap.get(self.class_id) {
            HeapData::ClassObject(cls) => cls.name(interns).to_string(),
            _ => "<unknown>".to_string(),
        }
    }

    /// Returns the slot index for a name, if it is a slot on this instance's class.
    #[must_use]
    pub fn slot_index(&self, name: &str, heap: &Heap<impl ResourceTracker>) -> Option<usize> {
        match heap.get(self.class_id) {
            HeapData::ClassObject(cls) => cls.slot_index(name),
            _ => None,
        }
    }

    /// Returns the current slot value for a named slot, if present and initialized.
    #[must_use]
    pub fn slot_value<'a>(&'a self, name: &str, heap: &Heap<impl ResourceTracker>) -> Option<&'a Value> {
        let idx = self.slot_index(name, heap)?;
        let value = self.slot_values.get(idx)?;
        if matches!(value, Value::Undefined) {
            None
        } else {
            Some(value)
        }
    }

    /// Returns all slot values for this instance.
    #[must_use]
    pub fn slot_values(&self) -> &[Value] {
        &self.slot_values
    }

    /// Sets a named slot value, returning the old value if one existed.
    pub fn set_slot_value(
        &mut self,
        name: &str,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        let Some(idx) = self.slot_index(name, heap) else {
            value.drop_with_heap(heap);
            let class_name = self.class_name(heap, interns);
            return Err(ExcType::attribute_error(format!("'{class_name}' object"), name));
        };

        let old = std::mem::replace(&mut self.slot_values[idx], value);
        if matches!(old, Value::Undefined) {
            Ok(None)
        } else {
            Ok(Some(old))
        }
    }

    /// Deletes a named slot value, returning the old value if one existed.
    pub fn delete_slot_value(
        &mut self,
        name: &str,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        let Some(idx) = self.slot_index(name, heap) else {
            let class_name = self.class_name(heap, interns);
            return Err(ExcType::attribute_error(format!("'{class_name}' object"), name));
        };

        let old = std::mem::replace(&mut self.slot_values[idx], Value::Undefined);
        if matches!(old, Value::Undefined) {
            Ok(None)
        } else {
            Ok(Some(old))
        }
    }

    /// Writes the instance repr while including the Python-visible object id.
    ///
    /// This is used by `Value::py_repr_fmt` for instance values so default
    /// object repr output can match CPython's `<module.Class object at 0x...>`
    /// shape while preserving existing custom/dataclass repr fallbacks.
    pub(crate) fn py_repr_fmt_with_id(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
        py_id: usize,
    ) -> std::fmt::Result {
        instance_repr_fmt(self, f, heap, heap_ids, interns, Some(py_id))
    }

    /// Returns `str(instance)` while carrying the Python-visible object id.
    ///
    /// Mirrors `Instance::py_str` (enum / exception early-returns) but when the
    /// default repr fallback is reached the id is forwarded to
    /// `py_repr_fmt_with_id` so the output includes the memory address
    /// (`<__main__.C object at 0x...>`), matching CPython behaviour.
    pub(crate) fn py_str_with_id(
        &self,
        heap: &Heap<impl ResourceTracker>,
        interns: &Interns,
        py_id: usize,
    ) -> Cow<'static, str> {
        if enum_member_str_uses_value(self.class_id(), heap, interns)
            && let Some(attrs) = self.attrs(heap)
            && let Some(member_value) = attrs.get_by_str("value", heap, interns)
        {
            return member_value.py_str(heap, interns);
        }
        if let Some((class_name, member_name, member_value)) = enum_member_display(self, heap, interns) {
            if enum_member_str_uses_value(self.class_id(), heap, interns) {
                return member_value.py_str(heap, interns);
            }
            return Cow::Owned(format!("{class_name}.{member_name}"));
        }
        if let Some(exception_str) = exception_instance_str(self, heap, interns) {
            return exception_str;
        }
        // Default: id-aware repr so the address is included.
        let mut s = String::new();
        let mut heap_ids = AHashSet::new();
        let _ = self.py_repr_fmt_with_id(&mut s, heap, &mut heap_ids, interns, py_id);
        Cow::Owned(s)
    }
}

/// Returns enum-display components for an instance when it behaves like an enum member.
///
/// Enum members in Ouros are regular instances with `name` and `value` attributes,
/// and their class namespace contains `__members__`. This helper centralizes
/// detection so `py_str` and `py_repr_fmt` can render CPython-compatible output.
fn enum_member_display<'a>(
    instance: &'a Instance,
    heap: &'a Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(String, String, &'a Value)> {
    let HeapData::ClassObject(class_obj) = heap.get(instance.class_id()) else {
        return None;
    };
    class_obj.namespace().get_by_str("__members__", heap, interns)?;
    let attrs = instance.attrs(heap)?;
    let name_value = attrs.get_by_str("name", heap, interns)?;
    let value = attrs.get_by_str("value", heap, interns)?;

    let member_name = match name_value {
        Value::InternString(id) => interns.get_str(*id).to_string(),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => s.as_str().to_string(),
            _ => return None,
        },
        _ => return None,
    };

    Some((class_obj.name(interns).to_string(), member_name, value))
}

/// Returns whether enum member `str()` for `class_id` should render the raw value.
fn enum_member_str_uses_value(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
        return false;
    };
    if let Some(Value::Bool(use_value)) = class_obj.namespace().get_by_str("__enum_str_value__", heap, interns) {
        return *use_value;
    }
    for &base_id in class_obj.mro().iter().skip(1) {
        let HeapData::ClassObject(base_cls) = heap.get(base_id) else {
            continue;
        };
        if let Some(Value::Bool(use_value)) = base_cls.namespace().get_by_str("__enum_str_value__", heap, interns) {
            return *use_value;
        }
    }
    false
}

/// Returns exception-instance metadata for classes derived from `BaseException`.
///
/// The returned tuple contains:
/// - class name for display
/// - whether `__str__` is defined on the class MRO
/// - whether `__repr__` is defined on the class MRO
/// - the current `args` attribute value, if present
///
/// This supports CPython-compatible fallback formatting for exception instances
/// that use builtin exception behavior via `super().__init__(...)`.
fn exception_instance_info<'a>(
    instance: &'a Instance,
    heap: &'a Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(String, bool, bool, Option<&'a Value>)> {
    let HeapData::ClassObject(class_obj) = heap.get(instance.class_id()) else {
        return None;
    };
    if !class_obj
        .mro()
        .iter()
        .any(|&mro_id| matches!(heap.builtin_type_for_class_id(mro_id), Some(Type::Exception(_))))
    {
        return None;
    }
    let args_value = instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str("args", heap, interns))
        .or_else(|| instance.slot_value("args", heap));
    Some((
        class_obj.name(interns).to_string(),
        class_obj.mro_has_attr("__str__", instance.class_id(), heap, interns),
        class_obj.mro_has_attr("__repr__", instance.class_id(), heap, interns),
        args_value,
    ))
}

/// Returns sequence items when an exception `args` value is tuple/list-like.
fn exception_args_items<'a>(args_value: &'a Value, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a [Value]> {
    let Value::Ref(args_id) = args_value else {
        return None;
    };
    match heap.get(*args_id) {
        HeapData::Tuple(tuple) => Some(tuple.as_vec()),
        HeapData::List(list) => Some(list.as_vec()),
        _ => None,
    }
}

/// Formats `str(exc)` for exception instances using CPython argument rules.
///
/// CPython rules:
/// - zero args -> `''`
/// - one arg -> `str(arg0)`
/// - multiple args -> `str(args)`
fn exception_instance_str(
    instance: &Instance,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Cow<'static, str>> {
    let (_, has_custom_str, _, args_value) = exception_instance_info(instance, heap, interns)?;
    if has_custom_str {
        return None;
    }
    let Some(args_value) = args_value else {
        return Some(Cow::Borrowed(""));
    };
    if let Some(items) = exception_args_items(args_value, heap) {
        return Some(match items {
            [] => Cow::Borrowed(""),
            [item] => item.py_str(heap, interns),
            _ => args_value.py_str(heap, interns),
        });
    }
    Some(args_value.py_str(heap, interns))
}

/// Formats `repr(exc)` for exception instances using CPython argument rules.
///
/// CPython rules:
/// - zero args -> `ClassName()`
/// - one arg -> `ClassName(repr(arg0))`
/// - multiple args -> `ClassName` + `repr(args)`
fn exception_instance_repr_fmt(
    instance: &Instance,
    f: &mut impl Write,
    heap: &Heap<impl ResourceTracker>,
    heap_ids: &mut AHashSet<HeapId>,
    interns: &Interns,
) -> Option<std::fmt::Result> {
    let (class_name, _, has_custom_repr, args_value) = exception_instance_info(instance, heap, interns)?;
    if has_custom_repr {
        return None;
    }
    Some((|| match args_value {
        None => write!(f, "{class_name}()"),
        Some(args_value) => {
            if let Some(items) = exception_args_items(args_value, heap) {
                match items {
                    [] => write!(f, "{class_name}()"),
                    [item] => {
                        write!(f, "{class_name}(")?;
                        item.py_repr_fmt(f, heap, heap_ids, interns)?;
                        f.write_char(')')
                    }
                    _ => {
                        f.write_str(&class_name)?;
                        args_value.py_repr_fmt(f, heap, heap_ids, interns)
                    }
                }
            } else {
                write!(f, "{class_name}(")?;
                args_value.py_repr_fmt(f, heap, heap_ids, interns)?;
                f.write_char(')')
            }
        }
    })())
}

/// Formats an instance repr with optional Python-visible id for default object repr.
fn instance_repr_fmt(
    instance: &Instance,
    f: &mut impl Write,
    heap: &Heap<impl ResourceTracker>,
    heap_ids: &mut AHashSet<HeapId>,
    interns: &Interns,
    py_id: Option<usize>,
) -> std::fmt::Result {
    if let Some((class_name, member_name, member_value)) = enum_member_display(instance, heap, interns) {
        write!(f, "<{class_name}.{member_name}: ")?;
        member_value.py_repr_fmt(f, heap, heap_ids, interns)?;
        return f.write_char('>');
    }
    if let Some(result) = exception_instance_repr_fmt(instance, f, heap, heap_ids, interns) {
        return result;
    }

    let (class_name, default_repr_name, has_custom_repr, dataclass_fields, dataclass_repr_enabled) =
        match heap.get(instance.class_id) {
            crate::heap::HeapData::ClassObject(cls) => {
                let dataclass_fields = cls
                    .namespace()
                    .get_by_str("__ouros_dataclass_repr_fields__", heap, interns)
                    .or_else(|| cls.namespace().get_by_str("__ouros_dataclass_fields__", heap, interns))
                    .and_then(|value| match value {
                        Value::Ref(id) => match heap.get(*id) {
                            HeapData::Tuple(tuple) => Some(
                                tuple
                                    .as_vec()
                                    .iter()
                                    .filter_map(|value| {
                                        value.as_either_str(heap).map(|value| value.as_str(interns).to_string())
                                    })
                                    .collect::<Vec<String>>(),
                            ),
                            HeapData::List(list) => Some(
                                list.as_vec()
                                    .iter()
                                    .filter_map(|value| {
                                        value.as_either_str(heap).map(|value| value.as_str(interns).to_string())
                                    })
                                    .collect::<Vec<String>>(),
                            ),
                            _ => None,
                        },
                        _ => None,
                    })
                    .unwrap_or_default();
                let dataclass_repr_enabled = cls
                    .namespace()
                    .get_by_str("__ouros_dataclass_repr_enabled__", heap, interns)
                    .and_then(|value| match value {
                        Value::Bool(enabled) => Some(*enabled),
                        _ => None,
                    })
                    .unwrap_or(true);
                let class_name = cls.name(interns).to_string();
                let default_repr_name = cls
                    .namespace()
                    .get_by_str("__module__", heap, interns)
                    .and_then(|value| value.as_either_str(heap).map(|text| text.as_str(interns).to_string()))
                    .filter(|module| !module.is_empty())
                    .map_or_else(|| class_name.clone(), |module| format!("{module}.{class_name}"));
                (
                    class_name,
                    default_repr_name,
                    cls.mro_has_attr("__repr__", instance.class_id, heap, interns),
                    dataclass_fields,
                    dataclass_repr_enabled,
                )
            }
            _ => (
                "<unknown>".to_string(),
                "<unknown>".to_string(),
                false,
                Vec::new(),
                true,
            ),
        };

    if dataclass_repr_enabled && !dataclass_fields.is_empty() {
        write!(f, "{class_name}(")?;
        for (index, field_name) in dataclass_fields.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{field_name}=")?;
            let value = instance
                .attrs(heap)
                .and_then(|attrs| attrs.get_by_str(field_name.as_str(), heap, interns))
                .or_else(|| instance.slot_value(field_name.as_str(), heap));
            if let Some(value) = value {
                value.py_repr_fmt(f, heap, heap_ids, interns)?;
            } else {
                f.write_str("<?>")?;
            }
        }
        return f.write_char(')');
    }

    // The runtime formatter cannot execute user bytecode, so when a class
    // defines `__repr__` we provide a structured fallback for the common
    // one-field case (`ClassName(<field_repr>)`) instead of generic object repr.
    if has_custom_repr && let Some(attrs) = instance.attrs(heap) {
        let mut items = attrs.iter();
        if let Some((_, value)) = items.next()
            && items.next().is_none()
        {
            write!(f, "{class_name}(")?;
            value.py_repr_fmt(f, heap, heap_ids, interns)?;
            return f.write_char(')');
        }
    }

    if let Some(py_id) = py_id {
        write!(f, "<{default_repr_name} object at 0x{py_id:x}>")
    } else {
        write!(f, "<{default_repr_name} object>")
    }
}

impl PyTrait for Instance {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Instance
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.slot_values.len() * std::mem::size_of::<Value>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        // Default Python behavior: instances are equal only if they are the same object
        // (identity check happens at the Value level, not here).
        // If we reach here, check class and attrs.
        if self.class_id != other.class_id {
            return false;
        }
        if let HeapData::ClassObject(cls) = heap.get(self.class_id)
            && let Some(Value::Bool(false)) =
                cls.namespace()
                    .get_by_str("__ouros_dataclass_eq_enabled__", heap, interns)
        {
            // eq=False dataclasses should keep identity semantics.
            return false;
        }
        if self.slot_values.len() != other.slot_values.len() {
            return false;
        }
        for (left, right) in self.slot_values.iter().zip(&other.slot_values) {
            if !left.py_eq(right, heap, interns) {
                return false;
            }
        }

        match (self.attrs_id, other.attrs_id) {
            (Some(left_id), Some(right_id)) => {
                heap.with_two(left_id, right_id, |heap, left, right| match (left, right) {
                    (HeapData::Dict(a), HeapData::Dict(b)) => a.py_eq(b, heap, interns),
                    _ => false,
                })
            }
            (None, None) => true,
            _ => false,
        }
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        // Decrement ref for the class reference
        stack.push(self.class_id);
        // Decrement refs for all instance attributes
        if let Some(attrs_id) = self.attrs_id {
            stack.push(attrs_id);
        }
        for value in &mut self.slot_values {
            if let Value::Ref(id) = value {
                stack.push(*id);
                #[cfg(feature = "ref-count-panic")]
                value.dec_ref_forget();
            }
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_str(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Cow<'static, str> {
        if enum_member_str_uses_value(self.class_id(), heap, interns)
            && let Some(attrs) = self.attrs(heap)
            && let Some(member_value) = attrs.get_by_str("value", heap, interns)
        {
            return member_value.py_str(heap, interns);
        }
        if let Some((class_name, member_name, member_value)) = enum_member_display(self, heap, interns) {
            if enum_member_str_uses_value(self.class_id(), heap, interns) {
                return member_value.py_str(heap, interns);
            }
            return Cow::Owned(format!("{class_name}.{member_name}"));
        }
        if let Some(exception_str) = exception_instance_str(self, heap, interns) {
            return exception_str;
        }
        self.py_repr(heap, interns)
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        instance_repr_fmt(self, f, heap, heap_ids, interns, None)
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let method_name = attr.as_str(interns);
        defer_drop!(args, heap);
        Err(ExcType::attribute_error(self.py_type(heap), method_name))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);

        // Handle __class__ special attribute
        if attr_name == "__class__" {
            heap.inc_ref(self.class_id);
            return Ok(Some(AttrCallResult::Value(Value::Ref(self.class_id))));
        }

        // Handle __dict__ special attribute: return the live instance dict when present.
        if attr_name == "__dict__" {
            let has_dict = match heap.get(self.class_id) {
                HeapData::ClassObject(cls) => cls.instance_has_dict(),
                _ => false,
            };
            if !has_dict {
                let class_name = match heap.get(self.class_id) {
                    HeapData::ClassObject(cls) => cls.name(interns).to_string(),
                    _ => "<unknown>".to_string(),
                };
                return Err(ExcType::attribute_error(format!("'{class_name}' object"), "__dict__"));
            }
            let Some(attrs_id) = self.attrs_id else {
                return Err(ExcType::attribute_error("instance", "__dict__"));
            };
            heap.inc_ref(attrs_id);
            return Ok(Some(AttrCallResult::Value(Value::Ref(attrs_id))));
        }

        // Handle __weakref__ special attribute.
        if attr_name == "__weakref__" {
            let has_weakref = match heap.get(self.class_id) {
                HeapData::ClassObject(cls) => cls.instance_has_weakref(),
                _ => false,
            };
            if !has_weakref {
                let class_name = match heap.get(self.class_id) {
                    HeapData::ClassObject(cls) => cls.name(interns).to_string(),
                    _ => "<unknown>".to_string(),
                };
                return Err(ExcType::attribute_error(
                    format!("'{class_name}' object"),
                    "__weakref__",
                ));
            }
            let value = if let Some(weakref_id) = self.weakref_id()
                && heap.get_if_live(weakref_id).is_some()
            {
                heap.inc_ref(weakref_id);
                Value::Ref(weakref_id)
            } else {
                Value::None
            };
            return Ok(Some(AttrCallResult::Value(value)));
        }

        // Full descriptor protocol for attribute lookup:
        // 1. Check class MRO for data descriptors (has __set__ or __delete__) with __get__
        // 2. Check instance __dict__
        // 3. Check class MRO for non-data descriptors (has __get__ only)
        // 4. Check class attributes (non-descriptor)
        // 5. AttributeError
        //
        // Note: Custom descriptor __get__ calls are signaled via `DescriptorGet` which the
        // VM's load_attr handles. UserProperty is handled via PropertyCall.

        // Phase 1: Look up in class MRO (immutable borrow).
        // The returned value is already cloned with correct refcounts.
        let mut class_attr = match heap.get(self.class_id) {
            HeapData::ClassObject(cls) => cls.mro_lookup_attr(attr_name, self.class_id, heap, interns),
            _ => None,
        };

        // Extract ref_id without borrowing class_attr (avoids borrow conflicts with .take())
        let class_attr_ref_id = match &class_attr {
            Some((Value::Ref(id), _)) => Some(*id),
            _ => None,
        };

        // Phase 2: Check for data descriptor in class attr
        if let Some(ref_id) = class_attr_ref_id
            && is_data_descriptor(ref_id, heap, interns)
        {
            if matches!(heap.get(ref_id), HeapData::UserProperty(_)) {
                // UserProperty: falls through to Phase 4 where it's returned
                // as a plain class attr for the VM to handle via PropertyCall
            } else if has_descriptor_get(ref_id, heap, interns) {
                // Custom data descriptor with __get__
                let (value, _) = class_attr.take().expect("class attr should be present for descriptor");
                return Ok(Some(AttrCallResult::DescriptorGet(value)));
            }
        }

        // Phase 3: Check instance attributes
        let inst_value = if let Some(dict) = self.attrs(heap) {
            dict.get_by_str(attr_name, heap, interns)
                .map(|v| v.clone_with_heap(heap))
        } else {
            None
        };
        if let Some(inst_value) = inst_value {
            if let Some((value, _)) = class_attr.take() {
                value.drop_with_heap(heap);
            }
            return Ok(Some(AttrCallResult::Value(inst_value)));
        }

        // Phase 4: Non-data descriptor or plain class attr
        if let Some((value, _found_in)) = class_attr {
            if let Value::Ref(ref_id) = &value {
                let ref_id = *ref_id;
                // Check for non-data descriptor with __get__ (but not data descriptor)
                if !is_data_descriptor(ref_id, heap, interns) && has_descriptor_get(ref_id, heap, interns) {
                    return Ok(Some(AttrCallResult::DescriptorGet(value)));
                }
            }
            // Plain class attribute: value already owns its refcount
            return Ok(Some(AttrCallResult::Value(value)));
        }

        // Phase 5: Not found
        let class_name = match heap.get(self.class_id) {
            HeapData::ClassObject(cls) => cls.name(interns).to_string(),
            _ => "<unknown>".to_string(),
        };
        Err(ExcType::attribute_error(format!("'{class_name}' object"), attr_name))
    }
}

/// Custom serde for Instance.
impl serde::Serialize for Instance {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Instance", 4)?;
        state.serialize_field("class_id", &self.class_id)?;
        state.serialize_field("attrs_id", &self.attrs_id)?;
        state.serialize_field("slot_values", &self.slot_values)?;
        state.serialize_field("weakref_ids", &self.weakref_ids)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for Instance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Data {
            class_id: HeapId,
            attrs_id: Option<HeapId>,
            slot_values: Vec<Value>,
            #[serde(default)]
            weakref_ids: Vec<HeapId>,
        }
        let data = Data::deserialize(deserializer)?;
        Ok(Self {
            class_id: data.class_id,
            attrs_id: data.attrs_id,
            slot_values: data.slot_values,
            weakref_ids: data.weakref_ids,
        })
    }
}

// ============================================================================
// C3 Linearization
// ============================================================================

/// Computes the C3 linearization (MRO) for a class with the given base classes.
///
/// The C3 algorithm merges the MROs of all base classes with the list of bases
/// to produce a consistent method resolution order. This is the same algorithm
/// used by CPython since Python 2.3.
///
/// # Arguments
/// * `self_id` - HeapId of the class being defined
/// * `bases` - Direct base class HeapIds
/// * `heap` - Heap to look up base class MROs
///
/// # Returns
/// The full MRO starting with `self_id`, or an error if the hierarchy is
/// inconsistent (would produce an ambiguous ordering).
pub(crate) fn compute_c3_mro(
    self_id: HeapId,
    bases: &[HeapId],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<HeapId>> {
    if bases.is_empty() {
        // No bases - implicit (object,)
        let object_id = heap.builtin_class_id(Type::Object)?;
        return Ok(vec![self_id, object_id]);
    }

    // Check for circular inheritance - a class cannot inherit from itself
    if bases.contains(&self_id) {
        return Err(ExcType::type_error("a class cannot inherit from itself".to_string()));
    }

    // Collect the MROs of all base classes
    let mut linearizations: Vec<Vec<HeapId>> = Vec::with_capacity(bases.len() + 1);
    for &base_id in bases {
        match heap.get(base_id) {
            HeapData::ClassObject(cls) => {
                linearizations.push(cls.mro().to_vec());
            }
            _ => {
                return Err(ExcType::type_error("bases must be classes".to_string()));
            }
        }
    }
    // Check inheritance depth — reject chains deeper than MAX_INHERITANCE_DEPTH
    for lin in &linearizations {
        if lin.len() > MAX_INHERITANCE_DEPTH {
            return Err(ExcType::type_error(format!(
                "inheritance chain too deep (maximum depth {MAX_INHERITANCE_DEPTH})"
            )));
        }
    }

    // Add the list of bases itself as the last sequence to merge
    linearizations.push(bases.to_vec());

    // C3 merge
    let mut result = vec![self_id];
    loop {
        // Remove empty lists
        linearizations.retain(|l| !l.is_empty());
        if linearizations.is_empty() {
            break;
        }

        // Find a good head: a class that does not appear in the tail of any list
        let mut found = None;
        for lin in &linearizations {
            let candidate = lin[0];
            let in_tail = linearizations.iter().any(|other| other[1..].contains(&candidate));
            if !in_tail {
                found = Some(candidate);
                break;
            }
        }

        if let Some(next) = found {
            result.push(next);
            // Remove `next` from the head of all lists where it appears
            for lin in &mut linearizations {
                if !lin.is_empty() && lin[0] == next {
                    lin.remove(0);
                }
            }
        } else {
            // Build error message with base class names
            let base_names: Vec<String> = bases
                .iter()
                .map(|&id| match heap.get(id) {
                    HeapData::ClassObject(cls) => cls.name(interns).to_string(),
                    _ => "?".to_string(),
                })
                .collect();
            return Err(ExcType::type_error(format!(
                "Cannot create a consistent method resolution order (MRO) for bases {}",
                base_names.join(", ")
            )));
        }

        // Safety check for MRO length
        if result.len() > crate::resource::MAX_MRO_LENGTH {
            return Err(ExcType::type_error("MRO exceeds maximum length".to_string()));
        }
    }

    Ok(result)
}

// ============================================================================
// SuperProxy
// ============================================================================

/// A proxy object returned by `super()` that delegates attribute lookup
/// to the next class in the MRO after the current class.
///
/// In Python, `super()` inside a method of class C returns a proxy that,
/// when accessed for attributes, starts looking from the class after C
/// in the instance's MRO.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SuperProxy {
    /// The instance that `super()` was called on (the `self` of the method).
    instance_id: HeapId,
    /// The class whose method contains the `super()` call.
    /// Attribute lookup starts AFTER this class in the MRO.
    current_class_id: HeapId,
}

impl SuperProxy {
    /// Creates a new SuperProxy.
    #[must_use]
    pub fn new(instance_id: HeapId, current_class_id: HeapId) -> Self {
        Self {
            instance_id,
            current_class_id,
        }
    }

    /// Returns the instance HeapId.
    #[must_use]
    pub fn instance_id(&self) -> HeapId {
        self.instance_id
    }

    /// Returns the current class HeapId.
    #[must_use]
    pub fn current_class_id(&self) -> HeapId {
        self.current_class_id
    }

    /// Returns whether this proxy contains heap references.
    #[inline]
    #[must_use]
    #[expect(clippy::unused_self)]
    pub fn has_refs(&self) -> bool {
        true
    }
}

impl PyTrait for SuperProxy {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Instance // super proxies are treated as instance-like
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.instance_id == other.instance_id && self.current_class_id == other.current_class_id
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        stack.push(self.instance_id);
        stack.push(self.current_class_id);
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
        write!(f, "<super>")
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);

        // Get the instance's class to find the MRO
        let instance_class_id = match heap.get(self.instance_id) {
            HeapData::Instance(inst) => inst.class_id(),
            _ => return Err(ExcType::type_error("super(): __self__ is not an instance".to_string())),
        };

        // Get the MRO from the instance's class
        let mro = match heap.get(instance_class_id) {
            HeapData::ClassObject(cls) => cls.mro().to_vec(),
            _ => {
                return Err(ExcType::type_error(
                    "super(): instance's class is not a ClassObject".to_string(),
                ));
            }
        };

        // Find current_class_id in the MRO and start searching from the next class
        let start_idx = mro
            .iter()
            .position(|&id| id == self.current_class_id)
            .map_or(0, |i| i + 1);

        // Search classes after current_class_id in the MRO
        for &class_id in &mro[start_idx..] {
            let found = match heap.get(class_id) {
                HeapData::ClassObject(cls) => cls.namespace().get_by_str(attr_name, heap, interns),
                _ => None,
            };
            if let Some(value) = found {
                // Check if this is a UserProperty - if so, return PropertyCall
                // so the VM invokes the getter with the instance
                if let Value::Ref(prop_id) = value
                    && let HeapData::UserProperty(prop) = heap.get(*prop_id)
                    && let Some(fget) = prop.fget()
                {
                    let getter = fget.clone_with_heap(heap);
                    let instance = Value::Ref(self.instance_id);
                    heap.inc_ref(self.instance_id);
                    return Ok(Some(AttrCallResult::PropertyCall(getter, instance)));
                }
                return Ok(Some(AttrCallResult::Value(value.clone_with_heap(heap))));
            }
            if let Some(value) = builtin_exception_method_for_attr(class_id, attr_name, heap) {
                return Ok(Some(AttrCallResult::Value(value)));
            }
        }

        Err(ExcType::attribute_error("super", attr_name))
    }
}

/// Returns synthesized methods exposed by builtin exception class wrappers.
///
/// Builtin exception classes are represented by lightweight `ClassObject` wrappers
/// with empty namespaces. We synthesize `__init__` so user exception subclasses can
/// call `super().__init__(...)` and class instantiation can inherit BaseException
/// initialization semantics.
fn builtin_exception_method_for_attr(
    class_id: HeapId,
    attr_name: &str,
    heap: &Heap<impl ResourceTracker>,
) -> Option<Value> {
    let Type::Exception(exc_type) = heap.builtin_type_for_class_id(class_id)? else {
        return None;
    };
    match attr_name {
        "__init__" => Some(Value::Builtin(Builtins::TypeMethod {
            ty: Type::Exception(exc_type),
            method: StaticStrings::DunderInit,
        })),
        _ => None,
    }
}

// ============================================================================
// Descriptor Detection Helpers
// ============================================================================

/// Checks if a heap object is a data descriptor (has `__set__` or `__delete__`).
///
/// A data descriptor is an object whose type defines `__set__` or `__delete__`.
/// Data descriptors take priority over instance `__dict__` in attribute lookup.
/// `UserProperty` is always a data descriptor (handled separately).
pub(crate) fn is_data_descriptor(ref_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    // UserProperty is always a data descriptor
    if matches!(heap.get(ref_id), HeapData::UserProperty(_)) {
        return true;
    }

    // Slot descriptors are always data descriptors
    if matches!(heap.get(ref_id), HeapData::SlotDescriptor(_)) {
        return true;
    }

    // Check if it's an Instance whose class has __set__ or __delete__
    if let HeapData::Instance(inst) = heap.get(ref_id) {
        let desc_class_id = inst.class_id();
        if let HeapData::ClassObject(desc_cls) = heap.get(desc_class_id) {
            let has_set = desc_cls.mro_has_attr("__set__", desc_class_id, heap, interns);
            let has_delete = desc_cls.mro_has_attr("__delete__", desc_class_id, heap, interns);
            return has_set || has_delete;
        }
    }

    false
}

/// Checks if a heap object has a `__get__` method on its type.
///
/// Used to detect descriptor objects (both data and non-data descriptors).
pub(crate) fn has_descriptor_get(ref_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    if matches!(
        heap.get(ref_id),
        HeapData::SlotDescriptor(_)
            | HeapData::CachedProperty(_)
            | HeapData::SingleDispatchMethod(_)
            | HeapData::PartialMethod(_)
    ) {
        return true;
    }
    if let HeapData::Instance(inst) = heap.get(ref_id) {
        let desc_class_id = inst.class_id();
        if let HeapData::ClassObject(desc_cls) = heap.get(desc_class_id) {
            return desc_cls.mro_has_attr("__get__", desc_class_id, heap, interns);
        }
    }
    false
}

// ============================================================================
// Descriptor Unwrapping Helpers
// ============================================================================

/// Unwraps descriptor wrappers when accessed on a class.
///
/// - `StaticMethod`: returns the inner function directly
/// - `ClassMethod`: returns the inner function (VM handles class binding)
/// - `UserProperty`: returns the property object itself (class-level access)
/// - Other values: cloned with proper reference counting
///
fn unwrap_descriptor_for_class(value: &Value, heap: &Heap<impl ResourceTracker>) -> Value {
    if let Value::Ref(id) = value {
        match heap.get(*id) {
            HeapData::StaticMethod(sm) => return sm.func().clone_with_heap(heap),
            HeapData::UserProperty(_) => return value.clone_with_heap(heap),
            _ => {}
        }
    }
    value.clone_with_heap(heap)
}

// ============================================================================
// Slot Descriptor
// ============================================================================

/// Slot descriptor kinds used for `__slots__` handling.
///
/// `Member` represents normal user slots, while `Dict` and `Weakref` represent
/// the special `__dict__` and `__weakref__` slots which use getset semantics.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) enum SlotDescriptorKind {
    Member,
    Dict,
    Weakref,
}

/// A built-in descriptor created for `__slots__` entries.
///
/// Slot descriptors are data descriptors that read/write instance storage
/// directly and bypass the normal instance dict. They are created at class
/// creation time based on `__slots__` contents.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SlotDescriptor {
    name: String,
    kind: SlotDescriptorKind,
}

impl SlotDescriptor {
    /// Creates a slot descriptor for a slot name and kind.
    #[must_use]
    pub fn new(name: String, kind: SlotDescriptorKind) -> Self {
        Self { name, kind }
    }

    /// Returns the slot name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the descriptor kind.
    #[must_use]
    pub fn kind(&self) -> SlotDescriptorKind {
        self.kind
    }
}

impl PyTrait for SlotDescriptor {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        match self.kind {
            SlotDescriptorKind::Member => Type::MemberDescriptor,
            SlotDescriptorKind::Dict | SlotDescriptorKind::Weakref => Type::GetSetDescriptor,
        }
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.name.len()
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
        match self.kind {
            SlotDescriptorKind::Member => write!(f, "<member '{}' of slots>", self.name),
            SlotDescriptorKind::Dict => write!(f, "<attribute '__dict__' of slots>"),
            SlotDescriptorKind::Weakref => write!(f, "<attribute '__weakref__' of slots>"),
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        match interns.get_str(attr_id) {
            "__name__" => {
                let name_id = heap.allocate(HeapData::Str(crate::types::Str::from(self.name.clone())))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(name_id))))
            }
            _ => Ok(None),
        }
    }
}

// ============================================================================
// StaticMethod Wrapper
// ============================================================================

/// A `@staticmethod` wrapper around a function.
///
/// When accessed on a class or instance, the wrapped function is returned
/// directly without binding `self` or `cls`. This differs from regular methods
/// which automatically inject `self` as the first argument.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct StaticMethod {
    /// The wrapped function value.
    func: Value,
}

impl StaticMethod {
    /// Creates a new StaticMethod wrapper.
    #[must_use]
    pub fn new(func: Value) -> Self {
        Self { func }
    }

    /// Returns a reference to the wrapped function.
    #[must_use]
    pub fn func(&self) -> &Value {
        &self.func
    }

    /// Returns whether this wrapper contains heap references.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        matches!(self.func, Value::Ref(_))
    }
}

impl PyTrait for StaticMethod {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::StaticMethod
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false // Identity-based, not value-based
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.func.py_dec_ref_ids(stack);
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
        write!(f, "<staticmethod object>")
    }
}

// ============================================================================
// ClassMethod Wrapper
// ============================================================================

/// A `@classmethod` wrapper around a function.
///
/// When accessed on a class, the class itself is injected as the first argument (`cls`).
/// When accessed on an instance, `type(instance)` is injected as `cls`.
/// This enables factory methods and class-level operations.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassMethod {
    /// The wrapped function value.
    func: Value,
}

impl ClassMethod {
    /// Creates a new ClassMethod wrapper.
    #[must_use]
    pub fn new(func: Value) -> Self {
        Self { func }
    }

    /// Returns a reference to the wrapped function.
    #[must_use]
    pub fn func(&self) -> &Value {
        &self.func
    }

    /// Returns whether this wrapper contains heap references.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        matches!(self.func, Value::Ref(_))
    }
}

impl PyTrait for ClassMethod {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::ClassMethod
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false // Identity-based, not value-based
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.func.py_dec_ref_ids(stack);
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
        write!(f, "<classmethod object>")
    }
}

// ============================================================================
// UserProperty Descriptor
// ============================================================================

/// A `@property` descriptor with optional getter, setter, and deleter.
///
/// Properties are data descriptors: when set as a class attribute, attribute
/// access on instances is intercepted:
/// - `obj.prop` calls the getter
/// - `obj.prop = val` calls the setter (if defined)
/// - `del obj.prop` calls the deleter (if defined)
///
/// A property without a setter is read-only and raises `AttributeError` on write.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct UserProperty {
    /// The getter function (called on attribute access).
    fget: Option<Value>,
    /// The setter function (called on attribute assignment).
    fset: Option<Value>,
    /// The deleter function (called on attribute deletion).
    fdel: Option<Value>,
    /// Explicit property docstring. When absent, `__doc__` falls back to `fget.__doc__`.
    doc: Option<Value>,
}

impl UserProperty {
    /// Creates a new UserProperty with only a getter.
    #[must_use]
    pub fn new(fget: Option<Value>) -> Self {
        Self {
            fget,
            fset: None,
            fdel: None,
            doc: None,
        }
    }

    /// Creates a new UserProperty with explicit getter/setter/deleter callables.
    #[must_use]
    pub fn new_full(fget: Option<Value>, fset: Option<Value>, fdel: Option<Value>, doc: Option<Value>) -> Self {
        Self { fget, fset, fdel, doc }
    }

    /// Returns the getter function, if any.
    #[must_use]
    pub fn fget(&self) -> Option<&Value> {
        self.fget.as_ref()
    }

    /// Returns the setter function, if any.
    #[must_use]
    pub fn fset(&self) -> Option<&Value> {
        self.fset.as_ref()
    }

    /// Returns the deleter function, if any.
    #[must_use]
    pub fn fdel(&self) -> Option<&Value> {
        self.fdel.as_ref()
    }

    /// Returns the explicit doc value, if provided.
    #[must_use]
    pub fn doc(&self) -> Option<&Value> {
        self.doc.as_ref()
    }

    /// Creates a new UserProperty with a setter added.
    ///
    /// Used by `@prop.setter` to create a new property that combines
    /// the original getter with a new setter.
    #[must_use]
    pub fn with_setter(fget: Option<Value>, fset: Value, doc: Option<Value>) -> Self {
        Self {
            fget,
            fset: Some(fset),
            fdel: None,
            doc,
        }
    }

    /// Creates a new UserProperty with a deleter added.
    ///
    /// Used by `@prop.deleter` to create a new property with a deleter.
    #[must_use]
    pub fn with_deleter(fget: Option<Value>, fset: Option<Value>, fdel: Value, doc: Option<Value>) -> Self {
        Self {
            fget,
            fset,
            fdel: Some(fdel),
            doc,
        }
    }

    /// Returns whether this property has heap references.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        matches!(self.fget, Some(Value::Ref(_)))
            || matches!(self.fset, Some(Value::Ref(_)))
            || matches!(self.fdel, Some(Value::Ref(_)))
            || matches!(self.doc, Some(Value::Ref(_)))
    }
}

impl PyTrait for UserProperty {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Type
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        if let Some(ref mut v) = self.fget {
            v.py_dec_ref_ids(stack);
        }
        if let Some(ref mut v) = self.fset {
            v.py_dec_ref_ids(stack);
        }
        if let Some(ref mut v) = self.fdel {
            v.py_dec_ref_ids(stack);
        }
        if let Some(ref mut v) = self.doc {
            v.py_dec_ref_ids(stack);
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
        write!(f, "<property object>")
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);

        // Support @prop.setter and @prop.deleter syntax.
        // These return the property itself -- the actual wrapping happens at the call level
        // when the returned "property" is called with the new function.
        if attr_name == "setter" || attr_name == "deleter" || attr_name == "getter" {
            // Return a special callable that, when called with a function, creates a
            // new UserProperty with the given setter/deleter.
            // We need to clone the property's getter/setter/deleter refs and create
            // a PropertyAccessor on the heap.
            let fget = self.fget.as_ref().map(|value| value.clone_with_heap(heap));
            let fset = self.fset.as_ref().map(|value| value.clone_with_heap(heap));
            let fdel = self.fdel.as_ref().map(|value| value.clone_with_heap(heap));
            let doc = self.doc.as_ref().map(|value| value.clone_with_heap(heap));

            let kind = match attr_name {
                "setter" => PropertyAccessorKind::Setter,
                "deleter" => PropertyAccessorKind::Deleter,
                _ => PropertyAccessorKind::Getter,
            };

            let accessor = PropertyAccessor {
                fget,
                fset,
                fdel,
                doc,
                kind,
            };
            let heap_id = heap.allocate(HeapData::PropertyAccessor(accessor))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(heap_id))));
        }

        // Return fget/fset/fdel as attributes
        if attr_name == "fget" {
            if let Some(ref v) = self.fget {
                return Ok(Some(AttrCallResult::Value(v.clone_with_heap(heap))));
            }
            return Ok(Some(AttrCallResult::Value(Value::None)));
        }
        if attr_name == "fset" {
            if let Some(ref v) = self.fset {
                return Ok(Some(AttrCallResult::Value(v.clone_with_heap(heap))));
            }
            return Ok(Some(AttrCallResult::Value(Value::None)));
        }
        if attr_name == "fdel" {
            if let Some(ref v) = self.fdel {
                return Ok(Some(AttrCallResult::Value(v.clone_with_heap(heap))));
            }
            return Ok(Some(AttrCallResult::Value(Value::None)));
        }
        if attr_name == "__doc__" {
            if let Some(ref v) = self.doc {
                return Ok(Some(AttrCallResult::Value(v.clone_with_heap(heap))));
            }
            if let Some(fget) = self.fget.as_ref() {
                let doc_id: StringId = StaticStrings::DunderDoc.into();
                let result = fget.py_getattr(doc_id, heap, interns)?;
                return Ok(Some(result));
            }
            return Ok(Some(AttrCallResult::Value(Value::None)));
        }

        Err(ExcType::attribute_error("property", attr_name))
    }
}

// ============================================================================
// PropertyAccessor - helper for @prop.setter / @prop.deleter
// ============================================================================

/// The kind of property accessor being created.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) enum PropertyAccessorKind {
    /// `@prop.getter` - replaces the getter function.
    Getter,
    /// `@prop.setter` - adds/replaces the setter function.
    Setter,
    /// `@prop.deleter` - adds/replaces the deleter function.
    Deleter,
}

/// A callable returned by `property.setter` / `property.deleter` / `property.getter`.
///
/// When called with a function, creates a new `UserProperty` that inherits
/// the original property's getter/setter/deleter but replaces the one corresponding
/// to this accessor's kind.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct PropertyAccessor {
    /// The original property's getter.
    fget: Option<Value>,
    /// The original property's setter.
    fset: Option<Value>,
    /// The original property's deleter.
    fdel: Option<Value>,
    /// Explicit property docstring to preserve across setter/getter/deleter chains.
    doc: Option<Value>,
    /// Which function this accessor replaces.
    kind: PropertyAccessorKind,
}

impl PropertyAccessor {
    /// Returns the kind of accessor.
    #[must_use]
    pub fn kind(&self) -> PropertyAccessorKind {
        self.kind
    }

    /// Returns references to the original property functions.
    #[must_use]
    pub fn parts(&self) -> (Option<&Value>, Option<&Value>, Option<&Value>, Option<&Value>) {
        (
            self.fget.as_ref(),
            self.fset.as_ref(),
            self.fdel.as_ref(),
            self.doc.as_ref(),
        )
    }

    /// Returns whether this accessor contains heap references.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        matches!(self.fget, Some(Value::Ref(_)))
            || matches!(self.fset, Some(Value::Ref(_)))
            || matches!(self.fdel, Some(Value::Ref(_)))
            || matches!(self.doc, Some(Value::Ref(_)))
    }
}

impl PyTrait for PropertyAccessor {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Type
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        if let Some(ref mut v) = self.fget {
            v.py_dec_ref_ids(stack);
        }
        if let Some(ref mut v) = self.fset {
            v.py_dec_ref_ids(stack);
        }
        if let Some(ref mut v) = self.fdel {
            v.py_dec_ref_ids(stack);
        }
        if let Some(ref mut v) = self.doc {
            v.py_dec_ref_ids(stack);
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
        write!(f, "<property accessor>")
    }
}

// ============================================================================
// Bound Method
// ============================================================================

/// A bound method created from attribute access on an instance or classmethod.
///
/// Bound methods bundle the underlying function together with the bound `self`
/// (or `cls`) value. They are callable and expose `__self__` and `__func__`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct BoundMethod {
    /// The underlying function or callable.
    func: Value,
    /// The bound `self` (instance) or `cls` (class) value.
    self_arg: Value,
}

impl BoundMethod {
    /// Creates a new bound method from a function and bound argument.
    #[must_use]
    pub fn new(func: Value, self_arg: Value) -> Self {
        Self { func, self_arg }
    }

    /// Returns the underlying function value.
    #[must_use]
    pub fn func(&self) -> &Value {
        &self.func
    }

    /// Returns the bound `self`/`cls` value.
    #[must_use]
    pub fn self_arg(&self) -> &Value {
        &self.self_arg
    }

    /// Returns whether this bound method holds heap references.
    #[inline]
    #[must_use]
    pub fn has_refs(&self) -> bool {
        matches!(self.func, Value::Ref(_)) || matches!(self.self_arg, Value::Ref(_))
    }
}

impl PyTrait for BoundMethod {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Method
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.func.py_dec_ref_ids(stack);
        self.self_arg.py_dec_ref_ids(stack);
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
        write!(f, "<bound method>")
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        match interns.get_str(attr_id) {
            "__self__" => Ok(Some(AttrCallResult::Value(self.self_arg.clone_with_heap(heap)))),
            "__func__" => Ok(Some(AttrCallResult::Value(self.func.clone_with_heap(heap)))),
            _ => Ok(None),
        }
    }
}

// ============================================================================
// Built-in Class Callables
// ============================================================================

/// Callable returned by `type.__subclasses__` access on a class object.
///
/// This is a lightweight built-in method wrapper that is bound to a specific
/// class heap ID and returns the direct subclasses list when called.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassSubclasses {
    class_id: HeapId,
}

impl ClassSubclasses {
    /// Creates a new bound `__subclasses__` callable for the given class.
    #[must_use]
    pub fn new(class_id: HeapId) -> Self {
        Self { class_id }
    }

    /// Returns the bound class heap ID.
    #[must_use]
    pub fn class_id(&self) -> HeapId {
        self.class_id
    }
}

impl PyTrait for ClassSubclasses {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::BuiltinFunction
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        stack.push(self.class_id);
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
        f.write_str("<built-in method __subclasses__>")
    }

    fn py_getattr(
        &self,
        _attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        Ok(None)
    }
}

/// Callable returned by default `__class_getitem__` for PEP 695 classes.
///
/// When invoked, produces a `GenericAlias` for the bound class and arguments.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassGetItem {
    class_id: HeapId,
}

impl ClassGetItem {
    /// Creates a new bound `__class_getitem__` callable for the given class.
    #[must_use]
    pub fn new(class_id: HeapId) -> Self {
        Self { class_id }
    }

    /// Returns the bound class heap ID.
    #[must_use]
    pub fn class_id(&self) -> HeapId {
        self.class_id
    }
}

impl PyTrait for ClassGetItem {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::BuiltinFunction
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        stack.push(self.class_id);
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
        f.write_str("<built-in method __class_getitem__>")
    }

    fn py_getattr(
        &self,
        _attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        Ok(None)
    }
}

/// Callable returned by `function.__get__`.
///
/// Binds a function to an instance when called with (obj, cls).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct FunctionGet {
    func: Value,
}

impl FunctionGet {
    /// Creates a new bound `__get__` callable for the given function value.
    #[must_use]
    pub fn new(func: Value) -> Self {
        Self { func }
    }

    /// Returns the underlying function value.
    #[must_use]
    pub fn func(&self) -> &Value {
        &self.func
    }
}

impl PyTrait for FunctionGet {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::BuiltinFunction
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        self.func.py_dec_ref_ids(stack);
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
        f.write_str("<function __get__>")
    }

    fn py_getattr(
        &self,
        _attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        Ok(None)
    }
}
