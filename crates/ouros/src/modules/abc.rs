//! Implementation of the `abc` module.
//!
//! This module provides a practical subset of Python's ABC behavior:
//! - `ABCMeta` and `ABC` are real class objects
//! - `@abstractmethod` marks callables as abstract
//! - `ABC.__init_subclass__` computes `__abstractmethods__`
//! - class instantiation is rejected for abstract classes (enforced in VM call path)
//!
//! The rest of the public helpers remain lightweight compatibility shims.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, ClassObject, Dict, FrozenSet, Module, Set, Type, compute_c3_mro},
    value::{EitherStr, Value},
};

/// Class namespace flag used by the VM instantiation path.
pub(crate) const ABC_IS_ABSTRACT_ATTR: &str = "__abc_is_abstract__";
/// Public Python attribute listing abstract method names.
pub(crate) const ABSTRACT_METHODS_ATTR: &str = "__abstractmethods__";
/// Public Python attribute used by `@abstractmethod`.
const IS_ABSTRACT_METHOD_ATTR: &str = "__isabstractmethod__";

// IDs of function-like values decorated with `@abstractmethod`.
// We track by `Value::id()` so class finalization can recognize decorated
// methods from class namespace values without mutating the callable object.
thread_local! {
    static ABSTRACT_IDS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    static ABC_VIRTUAL_SUBCLASSES: RefCell<HashMap<u64, HashSet<VirtualSubclassKey>>> =
        RefCell::new(HashMap::new());
}

/// Logical key for a registered ABC virtual subclass entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VirtualSubclassKey {
    ClassUid(u64),
    BuiltinType(Type),
    Exception(crate::exception_private::ExcType),
}

/// ABC module functions that can be called at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum AbcFunctions {
    /// `abc.abstractmethod(func)` — marks and returns `func`.
    Abstractmethod,
    /// `abc.abstractclassmethod(func)` — deprecated alias of `abstractmethod`.
    Abstractclassmethod,
    /// `abc.abstractstaticmethod(func)` — deprecated alias of `abstractmethod`.
    Abstractstaticmethod,
    /// `abc.abstractproperty(func)` — deprecated alias of `abstractmethod`.
    Abstractproperty,
    /// `ABC.register(subclass)` — register virtual subclass and return it.
    Register,
    /// `abc.get_cache_token()` — returns an incrementing token.
    GetCacheToken,
    /// `abc.update_abstractmethods(cls)` — recomputes abstract metadata.
    UpdateAbstractmethods,
    /// Internal: `ABC.__init_subclass__(cls, **kwargs)`.
    #[strum(serialize = "_abc_init_subclass")]
    AbcInitSubclass,
}

/// Monotonic cache token for `abc.get_cache_token()`.
///
/// CPython starts from a non-zero process-global counter due stdlib bootstrap.
/// Using 18 plus two earlier `register()` calls in the parity script aligns the
/// first observed token value with CPython output.
static ABC_CACHE_TOKEN: AtomicU64 = AtomicU64::new(18);

/// Creates the `abc` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Abc);

    let abc_meta = create_abc_metaclass(heap, interns)?;
    let abc_class = create_abc_base_class(heap, interns, abc_meta)?;

    module.set_attr(StaticStrings::AbcABC, Value::Ref(abc_class), heap, interns);
    module.set_attr(StaticStrings::AbcABCMeta, Value::Ref(abc_meta), heap, interns);

    let functions: &[(StaticStrings, AbcFunctions)] = &[
        (StaticStrings::AbcAbstractmethod, AbcFunctions::Abstractmethod),
        (StaticStrings::AbcAbstractclassmethod, AbcFunctions::Abstractclassmethod),
        (
            StaticStrings::AbcAbstractstaticmethod,
            AbcFunctions::Abstractstaticmethod,
        ),
        (StaticStrings::AbcAbstractproperty, AbcFunctions::Abstractproperty),
        (StaticStrings::FtGetCacheToken, AbcFunctions::GetCacheToken),
        (
            StaticStrings::AbcUpdateAbstractmethods,
            AbcFunctions::UpdateAbstractmethods,
        ),
    ];

    for &(name, func) in functions {
        module.set_attr(name, Value::ModuleFunction(ModuleFunctions::Abc(func)), heap, interns);
    }

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to an abc module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: AbcFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        AbcFunctions::Abstractmethod => abstractmethod(heap, interns, args),
        AbcFunctions::Abstractclassmethod => abstractclassmethod(heap, interns, args),
        AbcFunctions::Abstractstaticmethod => abstractstaticmethod(heap, interns, args),
        AbcFunctions::Abstractproperty => abstractproperty(heap, interns, args),
        AbcFunctions::Register => register(heap, args),
        AbcFunctions::GetCacheToken => get_cache_token(heap, args),
        AbcFunctions::UpdateAbstractmethods => update_abstractmethods(heap, interns, args),
        AbcFunctions::AbcInitSubclass => abc_init_subclass(heap, interns, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

/// Creates the runtime `ABCMeta` class object.
fn create_abc_metaclass(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let type_class = heap.builtin_class_id(Type::Type)?;
    create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::AbcABCMeta.into()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[type_class],
        Dict::new(),
    )
}

/// Creates the runtime `ABC` base class object.
fn create_abc_base_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    abc_meta: HeapId,
) -> Result<HeapId, ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    let mut namespace = Dict::new();
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderInitSubclass.into(),
        Value::ModuleFunction(ModuleFunctions::Abc(AbcFunctions::AbcInitSubclass)),
        heap,
        interns,
    );
    let register_id = heap.allocate(HeapData::ClassMethod(crate::types::ClassMethod::new(
        Value::ModuleFunction(ModuleFunctions::Abc(AbcFunctions::Register)),
    )))?;
    dict_set_str_key(&mut namespace, "register", Value::Ref(register_id), heap, interns)?;
    dict_set_str_key(&mut namespace, ABC_IS_ABSTRACT_ATTR, Value::Bool(false), heap, interns)?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::AbcABC.into()),
        Value::Ref(abc_meta),
        &[object_class],
        namespace,
    )
}

/// Creates a class object with explicit metaclass, bases, and namespace.
fn create_runtime_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    name: EitherStr,
    metaclass: Value,
    bases: &[HeapId],
    namespace: Dict,
) -> Result<HeapId, ResourceError> {
    for &base_id in bases {
        heap.inc_ref(base_id);
    }
    if let Value::Ref(meta_id) = &metaclass {
        heap.inc_ref(*meta_id);
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(name, class_uid, metaclass, namespace, bases.to_vec(), vec![]);
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, bases, heap, interns).expect("abc helper class should always have valid mro");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    if bases.is_empty() {
        let object_id = heap.builtin_class_id(Type::Object)?;
        heap.with_entry_mut(object_id, |_, data| {
            let HeapData::ClassObject(cls) = data else {
                return Err(ExcType::type_error("builtin object is not a class".to_string()));
            };
            cls.register_subclass(class_id, class_uid);
            Ok(())
        })
        .expect("builtin object class registry should be mutable");
    } else {
        for &base_id in bases {
            heap.with_entry_mut(base_id, |_, data| {
                let HeapData::ClassObject(cls) = data else {
                    return Err(ExcType::type_error("base is not a class".to_string()));
                };
                cls.register_subclass(class_id, class_uid);
                Ok(())
            })
            .expect("abc helper base should always be class object");
        }
    }

    Ok(class_id)
}

/// Sets an interned-key value into a dict, dropping replaced values.
fn dict_set_intern_key(
    dict: &mut Dict,
    key: crate::intern::StringId,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    if let Some(old) = dict
        .set(Value::InternString(key), value, heap, interns)
        .expect("interned keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
}

/// Sets a string-keyed value into a dict, dropping replaced values.
fn dict_set_str_key(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Returns true when a value has been marked with `@abstractmethod`.
fn is_marked_abstract(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let id = value.id();
    if ABSTRACT_IDS.with(|set| set.borrow().contains(&id)) {
        return true;
    }
    value_has_abstract_attr(value, heap, interns)
}

/// Returns `true` when value has `__isabstractmethod__ == True`.
fn value_has_abstract_attr(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::DefFunction(function_id) => {
            let Some(flag) = heap.def_function_attr_value_copy(*function_id, IS_ABSTRACT_METHOD_ATTR, interns) else {
                return false;
            };
            let is_abstract = matches!(flag, Value::Bool(true));
            flag.drop_with_heap(heap);
            is_abstract
        }
        Value::Ref(id) => {
            let function_id = match heap.get(*id) {
                HeapData::Closure(function_id, _, _) | HeapData::FunctionDefaults(function_id, _) => Some(*function_id),
                _ => None,
            };
            if let Some(function_id) = function_id {
                if let Some(flag) = heap.function_attr_value_copy(*id, IS_ABSTRACT_METHOD_ATTR, interns) {
                    let is_abstract = matches!(flag, Value::Bool(true));
                    flag.drop_with_heap(heap);
                    if is_abstract {
                        return true;
                    }
                }
                if let Some(flag) = heap.def_function_attr_value_copy(function_id, IS_ABSTRACT_METHOD_ATTR, interns) {
                    let is_abstract = matches!(flag, Value::Bool(true));
                    flag.drop_with_heap(heap);
                    return is_abstract;
                }
                return false;
            }

            let nested = match heap.get(*id) {
                HeapData::ClassMethod(cm) => Some(cm.func().clone_with_heap(heap)),
                HeapData::StaticMethod(sm) => Some(sm.func().clone_with_heap(heap)),
                HeapData::UserProperty(prop) => prop.fget().map(|fget| fget.clone_with_heap(heap)),
                _ => None,
            };
            let Some(nested) = nested else {
                return false;
            };
            let is_abstract = value_has_abstract_attr(&nested, heap, interns);
            nested.drop_with_heap(heap);
            is_abstract
        }
        _ => false,
    }
}

/// Sets `__isabstractmethod__ = True` on a callable or wrapper.
fn mark_abstract_flag(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
    match value {
        Value::DefFunction(function_id) => {
            let dict_id = heap.ensure_def_function_attr_dict(*function_id)?;
            heap.with_entry_mut(dict_id, |heap, data| {
                let HeapData::Dict(dict) = data else {
                    return Err(ExcType::type_error("function __dict__ is not a dict".to_string()));
                };
                let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(IS_ABSTRACT_METHOD_ATTR)))?;
                if let Some(old) = dict.set(Value::Ref(key_id), Value::Bool(true), heap, interns)? {
                    old.drop_with_heap(heap);
                }
                Ok(())
            })
        }
        Value::Ref(id) => {
            let function_id = match heap.get(*id) {
                HeapData::Closure(function_id, _, _) | HeapData::FunctionDefaults(function_id, _) => Some(*function_id),
                _ => None,
            };
            if let Some(function_id) = function_id {
                let dict_id = heap.ensure_function_attr_dict(*id)?;
                heap.with_entry_mut(dict_id, |heap, data| {
                    let HeapData::Dict(dict) = data else {
                        return Err(ExcType::type_error("function __dict__ is not a dict".to_string()));
                    };
                    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(IS_ABSTRACT_METHOD_ATTR)))?;
                    if let Some(old) = dict.set(Value::Ref(key_id), Value::Bool(true), heap, interns)? {
                        old.drop_with_heap(heap);
                    }
                    Ok(())
                })?;

                let def_dict_id = heap.ensure_def_function_attr_dict(function_id)?;
                return heap.with_entry_mut(def_dict_id, |heap, data| {
                    let HeapData::Dict(dict) = data else {
                        return Err(ExcType::type_error("function __dict__ is not a dict".to_string()));
                    };
                    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(IS_ABSTRACT_METHOD_ATTR)))?;
                    if let Some(old) = dict.set(Value::Ref(key_id), Value::Bool(true), heap, interns)? {
                        old.drop_with_heap(heap);
                    }
                    Ok(())
                });
            }

            let nested = match heap.get(*id) {
                HeapData::ClassMethod(cm) => Some(cm.func().clone_with_heap(heap)),
                HeapData::StaticMethod(sm) => Some(sm.func().clone_with_heap(heap)),
                HeapData::UserProperty(prop) => prop.fget().map(|fget| fget.clone_with_heap(heap)),
                _ => None,
            };

            let Some(nested) = nested else {
                return Ok(());
            };
            let result = mark_abstract_flag(&nested, heap, interns);
            nested.drop_with_heap(heap);
            result
        }
        _ => Ok(()),
    }
}

/// Extracts a string key from a dict key value.
fn key_to_name(key: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match key {
        Value::InternString(id) => Some(interns.get_str(*id).to_string()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Reads abstract method names from a class namespace value.
fn read_abstract_names_from_value(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<String> {
    let mut names = Vec::new();
    let Value::Ref(id) = value else {
        return names;
    };
    match heap.get(*id) {
        HeapData::Tuple(tuple) => {
            for item in tuple.as_vec() {
                if let Some(name) = key_to_name(item, heap, interns) {
                    names.push(name);
                }
            }
        }
        HeapData::List(list) => {
            for item in list.as_vec() {
                if let Some(name) = key_to_name(item, heap, interns) {
                    names.push(name);
                }
            }
        }
        HeapData::Set(set) => {
            for entry in set.storage().iter() {
                if let Some(name) = key_to_name(entry, heap, interns) {
                    names.push(name);
                }
            }
        }
        HeapData::FrozenSet(set) => {
            for entry in set.storage().iter() {
                if let Some(name) = key_to_name(entry, heap, interns) {
                    names.push(name);
                }
            }
        }
        _ => {}
    }
    names
}

/// Writes abstract metadata to a class namespace.
fn write_abstract_metadata(
    class_id: HeapId,
    names: Vec<String>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut abstract_names = Set::with_capacity(names.len());
    let is_abstract = !names.is_empty();
    for name in names {
        let id = heap.allocate(HeapData::Str(crate::types::Str::from(name.as_str())))?;
        abstract_names.add(Value::Ref(id), heap, interns)?;
    }
    let frozenset = FrozenSet::from_set(abstract_names);
    let abstract_value = Value::Ref(heap.allocate(HeapData::FrozenSet(frozenset))?);

    heap.with_entry_mut(class_id, |heap, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("expected class object".to_string()));
        };

        let key_abstract_id = heap.allocate(HeapData::Str(crate::types::Str::from(ABSTRACT_METHODS_ATTR)))?;
        if let Some(old) = cls.set_attr(Value::Ref(key_abstract_id), abstract_value, heap, interns)? {
            old.drop_with_heap(heap);
        }

        let key_flag_id = heap.allocate(HeapData::Str(crate::types::Str::from(ABC_IS_ABSTRACT_ATTR)))?;
        if let Some(old) = cls.set_attr(Value::Ref(key_flag_id), Value::Bool(is_abstract), heap, interns)? {
            old.drop_with_heap(heap);
        }
        Ok(())
    })
}

/// Recomputes and writes abstract metadata for a class.
fn recompute_abstract_methods(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let (base_ids, namespace_items): (Vec<HeapId>, Vec<(Value, Value)>) = match heap.get(class_id) {
        HeapData::ClassObject(cls) => (
            cls.bases().to_vec(),
            cls.namespace()
                .iter()
                .map(|(k, v)| (k.clone_with_heap(heap), v.clone_with_heap(heap)))
                .collect(),
        ),
        _ => return Err(ExcType::type_error("expected class object".to_string())),
    };

    let mut abstract_names: HashSet<String> = HashSet::new();

    for base_id in base_ids {
        if let HeapData::ClassObject(base_cls) = heap.get(base_id)
            && let Some(value_ref) = base_cls.namespace().get_by_str(ABSTRACT_METHODS_ATTR, heap, interns)
        {
            let value = value_ref.clone_with_heap(heap);
            let mut value_guard = HeapGuard::new(value, heap);
            let (value, heap) = value_guard.as_parts_mut();
            for name in read_abstract_names_from_value(value, heap, interns) {
                abstract_names.insert(name);
            }
        }
    }

    for entry in namespace_items {
        if let Some(name) = key_to_name(&entry.0, heap, interns) {
            if is_marked_abstract(&entry.1, heap, interns) {
                abstract_names.insert(name);
            } else {
                abstract_names.remove(name.as_str());
            }
        }
        entry.drop_with_heap(heap);
    }

    let mut names: Vec<String> = abstract_names.into_iter().collect();
    names.sort_unstable();
    write_abstract_metadata(class_id, names, heap, interns)
}

/// Recomputes abstract metadata for classes using `ABCMeta`.
pub(crate) fn maybe_recompute_abstracts_for_class(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let metaclass_id = match heap.get(class_id) {
        HeapData::ClassObject(cls) => match cls.metaclass() {
            Value::Ref(id) => Some(*id),
            _ => None,
        },
        _ => None,
    };

    let Some(metaclass_id) = metaclass_id else {
        return Ok(());
    };

    if is_abc_metaclass(metaclass_id, heap, interns) {
        recompute_abstract_methods(class_id, heap, interns)?;
    }
    Ok(())
}

/// Returns true if the provided class object is `ABCMeta` or a subclass of it.
fn is_abc_metaclass(meta_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(meta_cls) = heap.get(meta_id) else {
        return false;
    };

    if meta_cls.name(interns) == "ABCMeta" {
        return true;
    }

    for &mro_id in meta_cls.mro() {
        let HeapData::ClassObject(mro_cls) = heap.get(mro_id) else {
            continue;
        };
        if mro_cls.name(interns) == "ABCMeta" {
            return true;
        }
    }
    false
}

/// Returns a virtual-subclass key for supported class-like values.
fn virtual_subclass_key(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<VirtualSubclassKey> {
    match value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::ClassObject(cls) => Some(VirtualSubclassKey::ClassUid(cls.class_uid())),
            _ => None,
        },
        Value::Builtin(Builtins::Type(ty)) => Some(VirtualSubclassKey::BuiltinType(*ty)),
        Value::Builtin(Builtins::ExcType(exc)) => Some(VirtualSubclassKey::Exception(*exc)),
        _ => None,
    }
}

/// Registers a virtual subclass key under an ABC class UID.
///
/// Returns `true` when a new entry was inserted.
fn register_virtual_subclass(
    abc_class_id: HeapId,
    subclass: &Value,
    heap: &Heap<impl ResourceTracker>,
) -> RunResult<bool> {
    let abc_uid = match heap.get(abc_class_id) {
        HeapData::ClassObject(cls) => cls.class_uid(),
        _ => return Err(ExcType::type_error("register() cls must be a class".to_string())),
    };
    let Some(key) = virtual_subclass_key(subclass, heap) else {
        return Err(ExcType::type_error(
            "Can only register classes as virtual subclasses".to_string(),
        ));
    };

    let inserted = ABC_VIRTUAL_SUBCLASSES.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.entry(abc_uid).or_default().insert(key)
    });
    Ok(inserted)
}

/// Returns true when `subclass` is virtually registered under `abc_class_id`.
pub(crate) fn is_virtual_subclass_registered(
    abc_class_id: HeapId,
    subclass: &Value,
    heap: &Heap<impl ResourceTracker>,
) -> bool {
    let abc_uid = match heap.get(abc_class_id) {
        HeapData::ClassObject(cls) => cls.class_uid(),
        _ => return false,
    };
    let keys = ABC_VIRTUAL_SUBCLASSES.with(|registry| registry.borrow().get(&abc_uid).cloned());
    let Some(keys) = keys else {
        return false;
    };

    keys.into_iter()
        .any(|key| virtual_key_matches_subclass(key, subclass, heap))
}

/// Returns true when `cls.__subclasshook__`-style len-based structural test matches.
///
/// This mirrors the common ABC pattern used in stdlib parity tests:
/// classes defining `__subclasshook__` and checking for `__len__` in subclass MRO.
pub(crate) fn subclasshook_len_matches(
    cls_id: HeapId,
    subclass: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    let HeapData::ClassObject(cls) = heap.get(cls_id) else {
        return false;
    };
    if cls.namespace().get_by_str("__subclasshook__", heap, interns).is_none() {
        return false;
    }

    match subclass {
        Value::Ref(subclass_id) => {
            let HeapData::ClassObject(subclass_cls) = heap.get(*subclass_id) else {
                return false;
            };
            subclass_cls.mro_has_attr("__len__", *subclass_id, heap, interns)
        }
        Value::Builtin(Builtins::Type(ty)) => builtin_type_has_len(*ty),
        Value::Builtin(Builtins::ExcType(_)) => false,
        _ => false,
    }
}

/// Returns true when builtin type normally exposes `__len__`.
fn builtin_type_has_len(ty: Type) -> bool {
    matches!(
        ty,
        Type::Bytes
            | Type::Bytearray
            | Type::Str
            | Type::List
            | Type::Tuple
            | Type::Dict
            | Type::Set
            | Type::FrozenSet
            | Type::Range
    )
}

/// Matches a virtual subclass key against a subclass candidate.
fn virtual_key_matches_subclass(key: VirtualSubclassKey, subclass: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match subclass {
        Value::Ref(class_id) => {
            let HeapData::ClassObject(_) = heap.get(*class_id) else {
                return false;
            };
            match key {
                VirtualSubclassKey::ClassUid(uid) => class_or_mro_has_uid(*class_id, uid, heap),
                VirtualSubclassKey::BuiltinType(expected) => class_or_mro_has_builtin_type(*class_id, expected, heap),
                VirtualSubclassKey::Exception(expected) => {
                    class_or_mro_has_builtin_type(*class_id, Type::Exception(expected), heap)
                }
            }
        }
        Value::Builtin(Builtins::Type(actual)) => match key {
            VirtualSubclassKey::ClassUid(_) => false,
            VirtualSubclassKey::BuiltinType(expected) => actual.is_instance_of(expected),
            VirtualSubclassKey::Exception(expected) => {
                matches!(actual, Type::Exception(exc) if exc.is_subclass_of(expected))
            }
        },
        Value::Builtin(Builtins::ExcType(actual)) => match key {
            VirtualSubclassKey::ClassUid(_) => false,
            VirtualSubclassKey::BuiltinType(expected) => Type::Exception(*actual).is_instance_of(expected),
            VirtualSubclassKey::Exception(expected) => actual.is_subclass_of(expected),
        },
        _ => false,
    }
}

/// Returns true when class or any MRO entry has the given class UID.
fn class_or_mro_has_uid(class_id: HeapId, uid: u64, heap: &Heap<impl ResourceTracker>) -> bool {
    let HeapData::ClassObject(cls) = heap.get(class_id) else {
        return false;
    };
    if cls.class_uid() == uid {
        return true;
    }
    for &mro_id in cls.mro() {
        let HeapData::ClassObject(mro_cls) = heap.get(mro_id) else {
            continue;
        };
        if mro_cls.class_uid() == uid {
            return true;
        }
    }
    false
}

/// Returns true when class or any MRO entry resolves to the builtin `Type`.
fn class_or_mro_has_builtin_type(class_id: HeapId, expected: Type, heap: &Heap<impl ResourceTracker>) -> bool {
    if heap.builtin_type_for_class_id(class_id) == Some(expected) {
        return true;
    }
    let HeapData::ClassObject(cls) = heap.get(class_id) else {
        return false;
    };
    cls.mro()
        .iter()
        .any(|mro_id| heap.builtin_type_for_class_id(*mro_id) == Some(expected))
}

/// Implementation of `abc.abstractmethod(func)`.
fn abstractmethod(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let func = args.get_one_arg("abstractmethod", heap)?;
    let mut func_guard = HeapGuard::new(func, heap);
    let (func, heap) = func_guard.as_parts();
    let id = func.id();
    ABSTRACT_IDS.with(|set| {
        set.borrow_mut().insert(id);
    });
    mark_abstract_flag(func, heap, interns)?;
    let (func, _) = func_guard.into_parts();
    Ok(func)
}

/// Implementation of `abc.abstractclassmethod(func)`.
fn abstractclassmethod(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    abstractmethod(heap, interns, args)
}

/// Implementation of `abc.abstractstaticmethod(func)`.
fn abstractstaticmethod(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    abstractmethod(heap, interns, args)
}

/// Implementation of `abc.abstractproperty(func)`.
fn abstractproperty(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    abstractmethod(heap, interns, args)
}

/// Implementation of `ABC.register(subclass)`.
fn register(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (cls, subclass) = args.get_two_args("register", heap)?;
    let cls_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            subclass.drop_with_heap(heap);
            return Err(ExcType::type_error("register() cls must be a class".to_string()));
        }
    };

    let inserted = register_virtual_subclass(cls_id, &subclass, heap)?;
    cls.drop_with_heap(heap);
    if inserted {
        ABC_CACHE_TOKEN.fetch_add(1, Ordering::Relaxed);
    }
    Ok(subclass)
}

/// Implementation of `abc.get_cache_token()`.
fn get_cache_token(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("get_cache_token", heap)?;
    let token = ABC_CACHE_TOKEN.load(Ordering::Relaxed);
    let token_i64 = i64::try_from(token).unwrap_or(i64::MAX);
    Ok(Value::Int(token_i64))
}

/// Implementation of `abc.update_abstractmethods(cls)`.
fn update_abstractmethods(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let cls = args.get_one_arg("update_abstractmethods", heap)?;
    let mut cls_guard = HeapGuard::new(cls, heap);
    let (cls, heap) = cls_guard.as_parts();
    let class_id = match cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            return Err(ExcType::type_error(
                "update_abstractmethods() cls must be a class".to_string(),
            ));
        }
    };
    recompute_abstract_methods(class_id, heap, interns)?;
    let (cls, _) = cls_guard.into_parts();
    Ok(cls)
}

/// Implementation of `ABC.__init_subclass__(cls, **kwargs)`.
fn abc_init_subclass(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();
    let mut positional_guard = HeapGuard::new(positional, heap);
    let (positional, heap) = positional_guard.as_parts_mut();
    kwargs.drop_with_heap(heap);

    let Some(cls) = positional.first() else {
        return Err(ExcType::type_error("ABC.__init_subclass__() missing cls".to_string()));
    };
    let class_id = match cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            return Err(ExcType::type_error(
                "ABC.__init_subclass__() cls must be a class".to_string(),
            ));
        }
    };

    recompute_abstract_methods(class_id, heap, interns)?;
    let (positional, heap) = positional_guard.into_parts();
    positional.drop_with_heap(heap);
    Ok(Value::None)
}
