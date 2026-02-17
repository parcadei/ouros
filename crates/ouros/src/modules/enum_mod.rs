//! Implementation of the `enum` module.
//!
//! This module provides a pragmatic runtime `Enum` model built on Ouros's class
//! machinery:
//! - `EnumMeta` and `Enum` are real class objects
//! - `Enum.__init_subclass__` materializes declared members as instances
//! - `EnumMeta.__call__` performs value lookup (`Color(1) -> Color.RED`)
//! - `EnumMeta.__iter__` returns members in declaration order
//!
//! The implementation intentionally remains narrower than CPython's full `enum`
//! package but supports the core semantics needed for stdlib parity work.

use std::cell::Cell;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, ClassObject, Dict, Instance, List, PyTrait, Str, Type, compute_c3_mro},
    value::{EitherStr, Value},
};

/// Name of the class attribute storing enum members by declaration order.
const ENUM_MEMBER_VALUES_ATTR: &str = "__member_values__";
/// Name of the class attribute storing enum value -> member mapping.
const ENUM_VALUE_MAP_ATTR: &str = "__value2member_map__";
/// Name of the class attribute marking `Flag`/`IntFlag`-style enums.
const ENUM_IS_FLAG_ATTR: &str = "__enum_is_flag__";
/// Name of the class attribute forcing enum member `str()` to use the raw value.
const ENUM_STR_VALUE_ATTR: &str = "__enum_str_value__";

/// Temporary container used while materializing enum members in `__init_subclass__`.
struct MemberSpec {
    key: Value,
    member: Value,
    raw_value: Value,
}

// Thread-local counter for `auto()` — each call returns the next integer starting at 1.
thread_local! {
    static AUTO_COUNTER: Cell<i64> = const { Cell::new(0) };
}

/// Resets the `auto()` counter back to zero.
///
/// Called before each execution to ensure `auto()` starts fresh at 1 for every
/// program run.
pub fn reset_auto_counter() {
    AUTO_COUNTER.with(|c| c.set(0));
}

/// Enum module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum EnumFunctions {
    /// `auto()` — returns the next auto-incrementing integer (starting at 1).
    Auto,
    /// `unique(cls)` — validates duplicate values in `cls.__members__`.
    Unique,
    /// `member(obj)` — identity function.
    Member,
    /// `nonmember(obj)` — identity function.
    Nonmember,
    /// `property(func)` — no-op decorator.
    Property,
    /// `verify(*checks)` — returns an enum validator decorator.
    Verify,
    /// `pickle_by_enum_name(enum_member)` compatibility helper.
    PickleByEnumName,
    /// `pickle_by_global_name(enum_member)` compatibility helper.
    PickleByGlobalName,
    /// `show_flag_values(flag_member)` — returns set bit values.
    ShowFlagValues,
    /// `global_enum(obj)` compatibility helper.
    GlobalEnum,
    /// `global_enum_repr(obj)` compatibility helper.
    GlobalEnumRepr,
    /// `global_flag_repr(obj)` compatibility helper.
    GlobalFlagRepr,
    /// `global_str(obj)` compatibility helper.
    GlobalStr,
    /// Internal: `Enum.__init_subclass__(cls, **kwargs)` member materialization hook.
    #[strum(serialize = "_enum_init_subclass")]
    EnumInitSubclass,
    /// Internal: `EnumMeta.__iter__(cls)`.
    #[strum(serialize = "_enum_meta_iter")]
    EnumMetaIter,
    /// Internal: `EnumMeta.__call__(cls, value)` lookup by value.
    #[strum(serialize = "_enum_meta_call")]
    EnumMetaCall,
    /// Internal: class subscription lookup (`Color['RED']`).
    #[strum(serialize = "_enum_class_getitem")]
    EnumClassGetitem,
    /// Internal: `Flag.__or__`.
    #[strum(serialize = "_enum_flag_or")]
    FlagOr,
    /// Internal: `Flag.__and__`.
    #[strum(serialize = "_enum_flag_and")]
    FlagAnd,
    /// Internal: `Flag.__xor__`.
    #[strum(serialize = "_enum_flag_xor")]
    FlagXor,
    /// Internal: `Flag.__invert__`.
    #[strum(serialize = "_enum_flag_invert")]
    FlagInvert,
}

/// Creates the `enum` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::EnumMod);

    let enum_meta_id = create_enum_metaclass(heap, interns)?;
    let enum_class_id = create_enum_base_class(heap, interns, enum_meta_id)?;
    let flag_class_id = create_flag_base_class(heap, interns, enum_meta_id, enum_class_id)?;
    let int_flag_class_id = create_int_flag_base_class(heap, interns, enum_meta_id, flag_class_id)?;
    let enum_check_class_id = create_enum_check_class(heap, interns, enum_meta_id)?;
    let flag_boundary_class_id = create_flag_boundary_class(heap, interns, enum_meta_id, enum_class_id)?;

    module.set_attr(StaticStrings::EnEnum, Value::Ref(enum_class_id), heap, interns);
    module.set_attr(
        StaticStrings::EnIntEnum,
        Value::Builtin(Builtins::Type(Type::Int)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::EnStrEnum,
        Value::Builtin(Builtins::Type(Type::Str)),
        heap,
        interns,
    );

    // Expose metaclass under both names used by CPython.
    for name in [StaticStrings::EnEnumType, StaticStrings::EnEnumMeta] {
        heap.inc_ref(enum_meta_id);
        module.set_attr(name, Value::Ref(enum_meta_id), heap, interns);
    }

    module.set_attr(StaticStrings::EnFlag, Value::Ref(flag_class_id), heap, interns);
    module.set_attr(StaticStrings::EnIntFlag, Value::Ref(int_flag_class_id), heap, interns);
    module.set_attr_str("FlagBoundary", Value::Ref(flag_boundary_class_id), heap, interns)?;
    heap.inc_ref(enum_class_id);
    module.set_attr_str("ReprEnum", Value::Ref(enum_class_id), heap, interns)?;
    module.set_attr_str("EnumCheck", Value::Ref(enum_check_class_id), heap, interns)?;
    module.set_attr_str("EnumDict", Value::Builtin(Builtins::Type(Type::Dict)), heap, interns)?;

    let enum_checks: &[(&str, i64)] = &[("CONTINUOUS", 1), ("NAMED_FLAGS", 2), ("UNIQUE", 3)];
    for &(name, raw_value) in enum_checks {
        let member = create_enum_symbol_member(enum_check_class_id, name, Value::Int(raw_value), heap, interns)?;
        set_class_attr(enum_check_class_id, name, member.clone_with_heap(heap), heap, interns)
            .expect("EnumCheck member setup should not fail");
        module.set_attr_str(name, member, heap, interns)?;
    }

    let boundaries: &[(StaticStrings, &str, &str)] = &[
        (StaticStrings::EnConform, "CONFORM", "conform"),
        (StaticStrings::EnEject, "EJECT", "eject"),
        (StaticStrings::EnKeep, "KEEP", "keep"),
        (StaticStrings::EnStrict, "STRICT", "strict"),
    ];
    for &(name, label, raw_value) in boundaries {
        let value_id = heap.allocate(HeapData::Str(Str::from(raw_value)))?;
        let member = create_enum_symbol_member(flag_boundary_class_id, label, Value::Ref(value_id), heap, interns)?;
        set_class_attr(
            flag_boundary_class_id,
            label,
            member.clone_with_heap(heap),
            heap,
            interns,
        )
        .expect("FlagBoundary member setup should not fail");
        module.set_attr(name, member, heap, interns);
    }

    let functions: &[(StaticStrings, EnumFunctions)] = &[
        (StaticStrings::EnAuto, EnumFunctions::Auto),
        (StaticStrings::EnUnique, EnumFunctions::Unique),
        (StaticStrings::EnMember, EnumFunctions::Member),
        (StaticStrings::EnNonmember, EnumFunctions::Nonmember),
        (StaticStrings::EnProperty, EnumFunctions::Property),
    ];

    for &(name, func) in functions {
        module.set_attr(name, Value::ModuleFunction(ModuleFunctions::Enum(func)), heap, interns);
    }
    module.set_attr_str(
        "verify",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::Verify)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "pickle_by_enum_name",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::PickleByEnumName)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "pickle_by_global_name",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::PickleByGlobalName)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "show_flag_values",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::ShowFlagValues)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "global_enum",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::GlobalEnum)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "global_enum_repr",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::GlobalEnumRepr)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "global_flag_repr",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::GlobalFlagRepr)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "global_str",
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::GlobalStr)),
        heap,
        interns,
    )?;
    let public_names = [
        "EnumType",
        "EnumMeta",
        "EnumDict",
        "Enum",
        "IntEnum",
        "StrEnum",
        "Flag",
        "IntFlag",
        "ReprEnum",
        "auto",
        "unique",
        "property",
        "verify",
        "member",
        "nonmember",
        "FlagBoundary",
        "STRICT",
        "CONFORM",
        "EJECT",
        "KEEP",
        "global_flag_repr",
        "global_enum_repr",
        "global_str",
        "global_enum",
        "EnumCheck",
        "CONTINUOUS",
        "NAMED_FLAGS",
        "UNIQUE",
        "pickle_by_global_name",
        "pickle_by_enum_name",
    ];
    let mut all_values = Vec::with_capacity(public_names.len());
    for name in public_names {
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        all_values.push(Value::Ref(name_id));
    }
    let all_id = heap.allocate(HeapData::List(List::new(all_values)))?;
    module.set_attr_str("__all__", Value::Ref(all_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to an enum module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: EnumFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        EnumFunctions::Auto => auto(heap, args),
        EnumFunctions::Unique => unique(heap, interns, args),
        EnumFunctions::Member => member(heap, args),
        EnumFunctions::Nonmember => nonmember(heap, args),
        EnumFunctions::Property => property(heap, args),
        EnumFunctions::Verify => verify(heap, args),
        EnumFunctions::PickleByEnumName => pickle_by_enum_name(heap, args),
        EnumFunctions::PickleByGlobalName => pickle_by_global_name(heap, args),
        EnumFunctions::ShowFlagValues => show_flag_values(heap, interns, args),
        EnumFunctions::GlobalEnum => identity_one_arg(heap, args, "enum.global_enum"),
        EnumFunctions::GlobalEnumRepr => identity_one_arg(heap, args, "enum.global_enum_repr"),
        EnumFunctions::GlobalFlagRepr => identity_one_arg(heap, args, "enum.global_flag_repr"),
        EnumFunctions::GlobalStr => identity_one_arg(heap, args, "enum.global_str"),
        EnumFunctions::EnumInitSubclass => enum_init_subclass(heap, interns, args),
        EnumFunctions::EnumMetaIter => enum_meta_iter(heap, interns, args),
        EnumFunctions::EnumMetaCall => enum_meta_call(heap, interns, args),
        EnumFunctions::EnumClassGetitem => enum_class_getitem(heap, interns, args),
        EnumFunctions::FlagOr => flag_binary_op(heap, interns, args, '|'),
        EnumFunctions::FlagAnd => flag_binary_op(heap, interns, args, '&'),
        EnumFunctions::FlagXor => flag_binary_op(heap, interns, args, '^'),
        EnumFunctions::FlagInvert => flag_invert(heap, interns, args),
    }
}

/// Creates the runtime `EnumMeta` class object.
fn create_enum_metaclass(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let type_class = heap.builtin_class_id(Type::Type)?;

    let mut namespace = Dict::new();
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderCall.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::EnumMetaCall)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderIter.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::EnumMetaIter)),
        heap,
        interns,
    );

    create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::EnEnumMeta.into()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[type_class],
        namespace,
    )
}

/// Creates the runtime `Enum` base class object.
fn create_enum_base_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    enum_meta_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;

    let mut namespace = Dict::new();
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderInitSubclass.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::EnumInitSubclass)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderClassGetitem.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::EnumClassGetitem)),
        heap,
        interns,
    );

    let class_id = create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::EnEnum.into()),
        Value::Ref(enum_meta_id),
        &[object_class],
        namespace,
    )?;
    set_class_attr(class_id, ENUM_IS_FLAG_ATTR, Value::Bool(false), heap, interns)
        .expect("enum marker setup should not fail");
    Ok(class_id)
}

/// Creates the runtime `Flag` base class object with bitwise dunder methods.
fn create_flag_base_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    enum_meta_id: HeapId,
    enum_class_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderOr.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::FlagOr)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderAnd.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::FlagAnd)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderXor.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::FlagXor)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderInvert.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::FlagInvert)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderClassGetitem.into(),
        Value::ModuleFunction(ModuleFunctions::Enum(EnumFunctions::EnumClassGetitem)),
        heap,
        interns,
    );

    let class_id = create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::EnFlag.into()),
        Value::Ref(enum_meta_id),
        &[enum_class_id],
        namespace,
    )?;
    set_class_attr(class_id, ENUM_IS_FLAG_ATTR, Value::Bool(true), heap, interns)
        .expect("flag marker setup should not fail");
    Ok(class_id)
}

/// Creates the lightweight `EnumCheck` class used by `verify(...)`.
fn create_enum_check_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    enum_meta_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("EnumCheck".to_string()),
        Value::Ref(enum_meta_id),
        &[object_class],
        Dict::new(),
    )
}

/// Creates the `FlagBoundary` enum class used for boundary mode constants.
fn create_flag_boundary_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    enum_meta_id: HeapId,
    enum_class_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let class_id = create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("FlagBoundary".to_string()),
        Value::Ref(enum_meta_id),
        &[enum_class_id],
        Dict::new(),
    )?;
    set_class_attr(class_id, ENUM_STR_VALUE_ATTR, Value::Bool(true), heap, interns)
        .expect("FlagBoundary str marker setup should not fail");
    Ok(class_id)
}

/// Creates an enum-style symbol instance for classes like `EnumCheck` and `FlagBoundary`.
fn create_enum_symbol_member(
    class_id: HeapId,
    name: &str,
    raw_value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<Value, ResourceError> {
    let member_id = allocate_enum_member_instance(class_id, heap).expect("enum symbol class id should be valid");
    let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    set_instance_attr(member_id, StaticStrings::Name, Value::Ref(name_id), heap, interns)
        .expect("enum symbol name setup should not fail");
    set_instance_attr(member_id, StaticStrings::Value, raw_value, heap, interns)
        .expect("enum symbol value setup should not fail");
    Ok(Value::Ref(member_id))
}

/// Creates the runtime `IntFlag` class as a true enum/int hybrid.
fn create_int_flag_base_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    enum_meta_id: HeapId,
    flag_class_id: HeapId,
) -> Result<HeapId, ResourceError> {
    let int_class = heap.builtin_class_id(Type::Int)?;
    let class_id = create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::EnIntFlag.into()),
        Value::Ref(enum_meta_id),
        &[flag_class_id, int_class],
        Dict::new(),
    )?;
    set_class_attr(class_id, ENUM_IS_FLAG_ATTR, Value::Bool(true), heap, interns)
        .expect("int flag marker setup should not fail");
    set_class_attr(class_id, ENUM_STR_VALUE_ATTR, Value::Bool(true), heap, interns)
        .expect("int flag str marker setup should not fail");
    Ok(class_id)
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

    let mro = compute_c3_mro(class_id, bases, heap, interns).expect("enum helper class should always have a valid MRO");

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
            .expect("enum helper base should always be a class object");
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
        .expect("interned string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
}

/// Sets a string-keyed value on a class namespace, dropping replaced values.
fn set_class_attr(
    class_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    set_class_attr_value(class_id, Value::Ref(key_id), value, heap, interns)
}

/// Sets a raw key/value pair on a class namespace, dropping replaced values.
fn set_class_attr_value(
    class_id: HeapId,
    key: Value,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    heap.with_entry_mut(class_id, |heap, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("expected class object".to_string()));
        };
        if let Some(old) = cls.set_attr(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
        Ok(())
    })
}

/// Allocates a new instance of an enum class.
fn allocate_enum_member_instance(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let (slot_len, has_dict) = match heap.get(class_id) {
        HeapData::ClassObject(cls) => (cls.slot_layout().len(), cls.instance_has_dict()),
        _ => return Err(ExcType::type_error("enum class id is not a class object".to_string())),
    };

    let attrs_id = if has_dict {
        Some(heap.allocate(HeapData::Dict(Dict::new()))?)
    } else {
        None
    };

    let mut slot_values = Vec::with_capacity(slot_len);
    for _ in 0..slot_len {
        slot_values.push(Value::Undefined);
    }

    heap.inc_ref(class_id);
    let instance = Instance::new(class_id, attrs_id, slot_values, Vec::new());
    Ok(heap.allocate(HeapData::Instance(instance))?)
}

/// Sets an instance attribute by interned name, dropping replaced values.
fn set_instance_attr(
    instance_id: HeapId,
    name: StaticStrings,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    heap.with_entry_mut(instance_id, |heap, data| {
        let HeapData::Instance(instance) = data else {
            return Err(ExcType::type_error("expected enum member instance".to_string()));
        };
        if let Some(old) = instance.set_attr(Value::InternString(name.into()), value, heap, interns)? {
            old.drop_with_heap(heap);
        }
        Ok(())
    })
}

/// Returns true when a class namespace entry should become an enum member.
fn is_enum_member_candidate(name: &str, value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    if name.starts_with('_') {
        return false;
    }

    if matches!(
        value,
        Value::DefFunction(_) | Value::ExtFunction(_) | Value::ModuleFunction(_) | Value::Property(_)
    ) {
        return false;
    }

    if let Value::Ref(id) = value {
        return !matches!(
            heap.get(*id),
            HeapData::ClassObject(_)
                | HeapData::Closure(_, _, _)
                | HeapData::FunctionDefaults(_, _)
                | HeapData::Module(_)
                | HeapData::StaticMethod(_)
                | HeapData::ClassMethod(_)
                | HeapData::UserProperty(_)
                | HeapData::PropertyAccessor(_)
                | HeapData::BoundMethod(_)
                | HeapData::ClassSubclasses(_)
                | HeapData::ClassGetItem(_)
                | HeapData::FunctionGet(_)
        );
    }

    true
}

/// Returns whether a class has a truthy boolean attribute in its MRO.
fn class_has_true_attr(
    class_id: HeapId,
    attr_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
        return false;
    };
    if let Some((value, _)) = class_obj.mro_lookup_attr(attr_name, class_id, heap, interns) {
        let result = matches!(value, Value::Bool(true));
        value.drop_with_heap(heap);
        result
    } else {
        false
    }
}

/// Returns the synthetic power-of-two value for a `Flag` member index.
fn flag_member_value_for_index(index: usize) -> i64 {
    let shift = u32::try_from(index).unwrap_or(u32::MAX);
    1_i64.checked_shl(shift).unwrap_or(0)
}

/// `Enum.__init_subclass__(cls, **kwargs)` implementation.
///
/// Materializes class attributes into enum member instances and stores helper maps
/// used by metaclass `__iter__` and `__call__`.
fn enum_init_subclass(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();
    kwargs.drop_with_heap(heap);

    let Some(cls) = positional.first() else {
        for value in positional {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("Enum.__init_subclass__() missing cls".to_string()));
    };

    for value in positional.iter().skip(1) {
        value.clone_with_heap(heap).drop_with_heap(heap);
    }

    let class_id = match cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            for value in positional {
                value.drop_with_heap(heap);
            }
            return Err(ExcType::type_error(
                "Enum.__init_subclass__() cls must be a class".to_string(),
            ));
        }
    };
    let is_flag_class = class_has_true_attr(class_id, ENUM_IS_FLAG_ATTR, heap, interns);
    set_class_attr(class_id, ENUM_IS_FLAG_ATTR, Value::Bool(is_flag_class), heap, interns)?;

    let namespace_items: Vec<(Value, Value)> = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => class_obj
            .namespace()
            .iter()
            .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
            .collect(),
        _ => Vec::new(),
    };

    let mut members: Vec<MemberSpec> = Vec::new();

    for (key, value) in namespace_items {
        let Some(name) = enum_member_name(&key, heap, interns) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            continue;
        };

        if !is_enum_member_candidate(name.as_str(), &value, heap) {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            continue;
        }

        let member_id = allocate_enum_member_instance(class_id, heap)?;
        let member_value = Value::Ref(member_id);
        let raw_value = if is_flag_class {
            Value::Int(flag_member_value_for_index(members.len()))
        } else {
            value.clone_with_heap(heap)
        };

        set_instance_attr(member_id, StaticStrings::Name, key.clone_with_heap(heap), heap, interns)?;
        set_instance_attr(
            member_id,
            StaticStrings::Value,
            raw_value.clone_with_heap(heap),
            heap,
            interns,
        )?;

        set_class_attr_value(
            class_id,
            key.clone_with_heap(heap),
            member_value.clone_with_heap(heap),
            heap,
            interns,
        )?;

        members.push(MemberSpec {
            key,
            member: member_value,
            raw_value,
        });
        value.drop_with_heap(heap);
    }

    let mut members_dict = Dict::new();
    let mut member_values = Vec::with_capacity(members.len());
    let mut value_map = Dict::new();

    for member in &members {
        if let Some(old) = members_dict.set(
            member.key.clone_with_heap(heap),
            member.member.clone_with_heap(heap),
            heap,
            interns,
        )? {
            old.drop_with_heap(heap);
        }
        member_values.push(member.member.clone_with_heap(heap));
        if let Some(old) = value_map.set(
            member.raw_value.clone_with_heap(heap),
            member.member.clone_with_heap(heap),
            heap,
            interns,
        )? {
            old.drop_with_heap(heap);
        }
    }

    let members_dict_id = heap.allocate(HeapData::Dict(members_dict))?;
    set_class_attr(class_id, "__members__", Value::Ref(members_dict_id), heap, interns)?;

    let member_values_id = heap.allocate(HeapData::List(List::new(member_values)))?;
    set_class_attr(
        class_id,
        ENUM_MEMBER_VALUES_ATTR,
        Value::Ref(member_values_id),
        heap,
        interns,
    )?;

    let value_map_id = heap.allocate(HeapData::Dict(value_map))?;
    set_class_attr(class_id, ENUM_VALUE_MAP_ATTR, Value::Ref(value_map_id), heap, interns)?;

    for member in members {
        member.key.drop_with_heap(heap);
        member.member.drop_with_heap(heap);
        member.raw_value.drop_with_heap(heap);
    }
    for value in positional {
        value.drop_with_heap(heap);
    }

    Ok(AttrCallResult::Value(Value::None))
}

/// `EnumMeta.__iter__(cls)` implementation.
fn enum_meta_iter(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("EnumMeta.__iter__", heap)?;
    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "EnumMeta.__iter__() cls must be a class".to_string(),
            ));
        }
    };

    let members_value = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => class_obj
            .namespace()
            .get_by_str(ENUM_MEMBER_VALUES_ATTR, heap, interns)
            .map(|v| v.clone_with_heap(heap)),
        _ => None,
    };

    cls.drop_with_heap(heap);

    if let Some(value) = members_value {
        Ok(AttrCallResult::Value(value))
    } else {
        let list_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
        Ok(AttrCallResult::Value(Value::Ref(list_id)))
    }
}

/// `EnumMeta.__call__(cls, value)` implementation.
fn enum_meta_call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional.collect();

    if !kwargs.is_empty() {
        for value in positional {
            value.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "EnumMeta.__call__() does not accept keyword arguments".to_string(),
        ));
    }
    kwargs.drop_with_heap(heap);

    if positional.len() != 2 {
        let got = positional.len().saturating_sub(1);
        for value in positional {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(format!(
            "EnumMeta.__call__() takes exactly one value argument ({got} given)"
        )));
    }

    let lookup_value = positional.pop().expect("checked len");
    let cls = positional.pop().expect("checked len");

    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            lookup_value.drop_with_heap(heap);
            cls.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "EnumMeta.__call__() cls must be a class".to_string(),
            ));
        }
    };

    let class_name = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => class_obj.name(interns).to_string(),
        _ => "Enum".to_string(),
    };

    let value_map_id = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => class_obj
            .namespace()
            .get_by_str(ENUM_VALUE_MAP_ATTR, heap, interns)
            .and_then(|v| match v {
                Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Some(*id),
                _ => None,
            }),
        _ => None,
    };

    let mut found_member: Option<Value> = None;
    if let Some(map_id) = value_map_id {
        let items = heap.with_entry_mut(map_id, |heap, data| {
            if let HeapData::Dict(dict) = data {
                dict.items(heap)
            } else {
                Vec::new()
            }
        });

        for (key, value) in items {
            if found_member.is_none() && lookup_value.py_eq(&key, heap, interns) {
                key.drop_with_heap(heap);
                found_member = Some(value);
                continue;
            }
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }
    }

    lookup_value.drop_with_heap(heap);
    cls.drop_with_heap(heap);

    if let Some(member) = found_member {
        return Ok(AttrCallResult::Value(member));
    }

    Err(SimpleException::new_msg(ExcType::ValueError, format!("value is not a valid {class_name}")).into())
}

/// Implementation of `enum.auto()`.
fn auto(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("enum.auto", heap)?;
    let value = AUTO_COUNTER.with(|c| {
        let next = c.get() + 1;
        c.set(next);
        next
    });
    Ok(AttrCallResult::Value(Value::Int(value)))
}

/// Implementation of `enum.unique(cls)`.
fn unique(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("enum.unique", heap)?;

    if let Value::Ref(class_id) = &cls
        && matches!(heap.get(*class_id), HeapData::ClassObject(_))
    {
        let (class_name, members_dict_id) = {
            let HeapData::ClassObject(class_obj) = heap.get(*class_id) else {
                return Ok(AttrCallResult::Value(cls));
            };
            let class_name = class_obj.name(interns).to_owned();
            let members_dict_id = class_obj
                .namespace()
                .get_by_str("__members__", heap, interns)
                .and_then(|value| match value {
                    Value::Ref(id) => Some(*id),
                    _ => None,
                });
            (class_name, members_dict_id)
        };

        let Some(members_dict_id) = members_dict_id else {
            return Ok(AttrCallResult::Value(cls));
        };

        let raw_members = heap.with_entry_mut(members_dict_id, |heap_inner, data| {
            if let HeapData::Dict(members_dict) = data {
                members_dict.items(heap_inner)
            } else {
                Vec::new()
            }
        });

        let mut members = Vec::with_capacity(raw_members.len());
        for (key, member) in raw_members {
            let Some(name) = enum_member_name(&key, heap, interns) else {
                key.drop_with_heap(heap);
                member.drop_with_heap(heap);
                continue;
            };
            // CPython's unique() inspects aliases via __members__, where aliases
            // point at the same member object. For enum-like classes that don't expose
            // a .value attribute, compare member objects directly.
            let value = enum_member_value(&member, heap, interns).unwrap_or_else(|| member.clone_with_heap(heap));
            members.push((name, value));
            key.drop_with_heap(heap);
            member.drop_with_heap(heap);
        }

        let duplicate = first_duplicate_member(&members, heap, interns);
        for (_, value) in members {
            value.drop_with_heap(heap);
        }

        if let Some((alias, original)) = duplicate {
            cls.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("duplicate values found in <enum '{class_name}'>: {alias} -> {original}"),
            )
            .into());
        }
    }

    Ok(AttrCallResult::Value(cls))
}

/// Extracts an enum member name from a class namespace key.
fn enum_member_name(key: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match key {
        Value::InternString(id) => Some(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_owned()),
            _ => None,
        },
        _ => None,
    }
}

/// Extracts a cloned `.value` attribute from an enum member instance.
fn enum_member_value(member: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<Value> {
    let Value::Ref(member_id) = member else {
        return None;
    };
    let HeapData::Instance(instance) = heap.get(*member_id) else {
        return None;
    };
    instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str("value", heap, interns))
        .map(|value| value.clone_with_heap(heap))
}

/// Finds the first duplicate member value in declaration order.
fn first_duplicate_member(
    members: &[(String, Value)],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(String, String)> {
    for (idx, (name, value)) in members.iter().enumerate() {
        for (previous_name, previous_value) in &members[..idx] {
            if value.py_eq(previous_value, heap, interns) {
                return Some((name.clone(), previous_name.clone()));
            }
        }
    }
    None
}

/// Looks up a member by value in a class's `__value2member_map__`.
fn find_member_for_value(
    class_id: HeapId,
    lookup_value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let value_map_id = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => class_obj
            .namespace()
            .get_by_str(ENUM_VALUE_MAP_ATTR, heap, interns)
            .and_then(|value| match value {
                Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Some(*id),
                _ => None,
            }),
        _ => None,
    }?;

    let items = heap.with_entry_mut(value_map_id, |heap, data| {
        if let HeapData::Dict(dict) = data {
            dict.items(heap)
        } else {
            Vec::new()
        }
    });

    let mut found: Option<Value> = None;
    for (key, value) in items {
        if found.is_none() && lookup_value.py_eq(&key, heap, interns) {
            key.drop_with_heap(heap);
            found = Some(value);
            continue;
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    found
}

/// Extracts `(class_id, name, value)` from an enum member value.
fn enum_member_components(
    member: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(HeapId, String, i64)> {
    let Value::Ref(member_id) = member else {
        return None;
    };
    let HeapData::Instance(instance) = heap.get(*member_id) else {
        return None;
    };
    let class_id = instance.class_id();
    let attrs = instance.attrs(heap)?;
    let name = enum_member_name(attrs.get_by_str("name", heap, interns)?, heap, interns)?;
    let value = match attrs.get_by_str("value", heap, interns)? {
        Value::Int(i) => *i,
        _ => return None,
    };
    Some((class_id, name, value))
}

/// Returns the declaration-order members for an enum class.
fn class_member_list(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Vec<Value> {
    let members_id = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => class_obj
            .namespace()
            .get_by_str(ENUM_MEMBER_VALUES_ATTR, heap, interns)
            .and_then(|value| match value {
                Value::Ref(id) if matches!(heap.get(*id), HeapData::List(_)) => Some(*id),
                _ => None,
            }),
        _ => None,
    };
    let Some(list_id) = members_id else {
        return Vec::new();
    };
    heap.with_entry_mut(list_id, |heap, data| {
        if let HeapData::List(list) = data {
            list.as_vec()
                .iter()
                .map(|member| member.clone_with_heap(heap))
                .collect()
        } else {
            Vec::new()
        }
    })
}

/// Builds a display name for synthesized flag values.
fn synthesize_flag_name(
    class_id: HeapId,
    value: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> String {
    let mut names: Vec<String> = Vec::new();
    let members = class_member_list(class_id, heap, interns);
    for member in members {
        if let Some((_, name, member_value)) = enum_member_components(&member, heap, interns)
            && member_value > 0
            && (value & member_value) == member_value
        {
            names.push(name);
        }
        member.drop_with_heap(heap);
    }
    if names.is_empty() {
        "0".to_string()
    } else {
        names.join("|")
    }
}

/// Builds (or reuses) a flag member instance for the given class/value pair.
fn build_flag_result(
    class_id: HeapId,
    value: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if let Some(member) = find_member_for_value(class_id, &Value::Int(value), heap, interns) {
        return Ok(member);
    }

    let member_id = allocate_enum_member_instance(class_id, heap)?;
    let member_name = synthesize_flag_name(class_id, value, heap, interns);
    let name_id = heap.allocate(HeapData::Str(Str::from(member_name)))?;
    set_instance_attr(member_id, StaticStrings::Name, Value::Ref(name_id), heap, interns)?;
    set_instance_attr(member_id, StaticStrings::Value, Value::Int(value), heap, interns)?;
    Ok(Value::Ref(member_id))
}

/// Performs the shared `Flag` binary dunder implementation.
fn flag_binary_op(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    op: char,
) -> RunResult<AttrCallResult> {
    let (left, right) = args.get_two_args("Flag operation", heap)?;
    let Some((left_class_id, _, left_value)) = enum_member_components(&left, heap, interns) else {
        left.drop_with_heap(heap);
        right.drop_with_heap(heap);
        return Err(ExcType::type_error("unsupported flag operands".to_string()));
    };
    let Some((right_class_id, _, right_value)) = enum_member_components(&right, heap, interns) else {
        left.drop_with_heap(heap);
        right.drop_with_heap(heap);
        return Err(ExcType::type_error("unsupported flag operands".to_string()));
    };
    if left_class_id != right_class_id {
        left.drop_with_heap(heap);
        right.drop_with_heap(heap);
        return Err(ExcType::type_error("unsupported flag operands".to_string()));
    }

    let value = match op {
        '|' => left_value | right_value,
        '&' => left_value & right_value,
        '^' => left_value ^ right_value,
        _ => unreachable!("invalid flag binary operator"),
    };
    let result = build_flag_result(left_class_id, value, heap, interns)?;
    left.drop_with_heap(heap);
    right.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implements `Flag.__invert__`.
fn flag_invert(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("Flag.__invert__", heap)?;
    let Some((class_id, _, current_value)) = enum_member_components(&value, heap, interns) else {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error("unsupported flag operand".to_string()));
    };

    let mut mask = 0_i64;
    let members = class_member_list(class_id, heap, interns);
    for member in members {
        if let Some((_, _, member_value)) = enum_member_components(&member, heap, interns) {
            mask |= member_value;
        }
        member.drop_with_heap(heap);
    }
    let inverted = (!current_value) & mask;
    let result = build_flag_result(class_id, inverted, heap, interns)?;
    value.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implements enum class subscription lookup (`Color['RED']`).
fn enum_class_getitem(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (cls, key) = args.get_two_args("Enum.__class_getitem__", heap)?;
    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            key.drop_with_heap(heap);
            cls.drop_with_heap(heap);
            return Err(ExcType::type_error("Enum class lookup requires a class".to_string()));
        }
    };

    let members_dict_id =
        match heap.get(class_id) {
            HeapData::ClassObject(class_obj) => class_obj
                .namespace()
                .get_by_str("__members__", heap, interns)
                .and_then(|value| match value {
                    Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Some(*id),
                    _ => None,
                }),
            _ => None,
        };
    let Some(dict_id) = members_dict_id else {
        key.drop_with_heap(heap);
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error("Enum class has no members".to_string()));
    };

    let member = heap.with_entry_mut(dict_id, |heap, data| {
        let HeapData::Dict(dict) = data else {
            return Ok::<Option<Value>, crate::exception_private::RunError>(None);
        };
        Ok(dict.get(&key, heap, interns)?.map(|value| value.clone_with_heap(heap)))
    })?;
    key.drop_with_heap(heap);
    cls.drop_with_heap(heap);

    if let Some(value) = member {
        Ok(AttrCallResult::Value(value))
    } else {
        Err(SimpleException::new_msg(ExcType::KeyError, "unknown enum key".to_string()).into())
    }
}

/// Implementation of `enum.member(obj)`.
fn member(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("enum.member", heap)?;
    Ok(AttrCallResult::Value(obj))
}

/// Implementation of `enum.nonmember(obj)`.
fn nonmember(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("enum.nonmember", heap)?;
    Ok(AttrCallResult::Value(obj))
}

/// Implementation of `enum.property(func)`.
fn property(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("enum.property", heap)?;
    Ok(AttrCallResult::Value(func))
}

/// Implementation of `enum.verify(*checks)`.
fn verify(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    for value in positional {
        value.drop_with_heap(heap);
    }
    kwargs.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::ModuleFunction(ModuleFunctions::Enum(
        EnumFunctions::Unique,
    ))))
}

/// Implementation of `enum.pickle_by_enum_name(member)`.
fn pickle_by_enum_name(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("enum.pickle_by_enum_name", heap)?;
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `enum.pickle_by_global_name(member)`.
fn pickle_by_global_name(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("enum.pickle_by_global_name", heap)?;
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `enum.show_flag_values(flag)`.
fn show_flag_values(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let member = args.get_one_arg("enum.show_flag_values", heap)?;
    let value = if let Some((_, _, member_value)) = enum_member_components(&member, heap, interns) {
        member_value
    } else {
        member.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "show_flag_values() expects a Flag member".to_string(),
        ));
    };

    let mut parts = Vec::new();
    let mut remaining = value;
    while remaining > 0 {
        let bit = remaining & -remaining;
        parts.push(Value::Int(bit));
        remaining &= !bit;
    }

    member.drop_with_heap(heap);
    let list_id = heap.allocate(HeapData::List(List::new(parts)))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

/// Returns a single argument unchanged for enum decorator/helper compatibility shims.
fn identity_one_arg(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, name: &str) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg(name, heap)?;
    Ok(AttrCallResult::Value(value))
}
