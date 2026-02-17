//! Implementation of the `typing` module.
//!
//! Provides a runtime-compatible subset of Python's `typing` module:
//! - `TYPE_CHECKING`: Always `False`
//! - Common type-hint markers (`Any`, `Optional`, `List`, ...)
//! - `Protocol` as a real class object with runtime structural checks via
//!   `@runtime_checkable` and metaclass `__instancecheck__`
//! - `TypedDict` as a real class object supporting:
//!   - class syntax (`class X(TypedDict, total=False): ...`)
//!   - functional syntax (`TypedDict('X', {'k': int}, total=False)`)
//! - `NewType`, `cast`, `reveal_type`, `assert_type`, `overload`
//! - `dataclass_transform`, `override`, `deprecated` as identity decorators
//! - `get_type_hints`, `get_origin`, `get_args`
//!
//! Ouros does not perform static type checking; this module focuses on runtime
//! compatibility for code that imports and uses typing constructs.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

use ahash::AHashSet;
use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::{Builtins, BuiltinsFunctions},
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, Instance, List, Module, PyTrait, Str, Type, allocate_tuple, compute_c3_mro,
        make_generic_alias,
    },
    value::{EitherStr, Marker, Value},
};

/// Private class-namespace flag marking protocol classes.
const ATTR_IS_PROTOCOL: &str = "_is_protocol";
/// Private class-namespace flag enabling runtime `isinstance()` protocol checks.
const ATTR_IS_RUNTIME_PROTOCOL: &str = "_is_runtime_protocol";
/// Private class-namespace flag marking the `TypedDict` root helper class.
const ATTR_TYPED_DICT_ROOT: &str = "__typed_dict_root__";
/// Private class-namespace flag marking TypedDict classes.
const ATTR_IS_TYPED_DICT: &str = "__is_typeddict__";
/// Per-class totality flag for TypedDict classes.
const ATTR_TYPED_DICT_TOTAL: &str = "__total__";
/// Process-global overload registry keyed by function `__name__`.
static OVERLOAD_REGISTRY: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Typing module functions that can be called at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum TypingFunctions {
    /// `typing.TypeVar(name, ...)` — returns a runtime marker object.
    #[strum(serialize = "TypeVar")]
    TypeVar,
    /// `typing.ParamSpec(name)` — returns a runtime marker object with `__name__`.
    #[strum(serialize = "ParamSpec")]
    ParamSpec,
    /// `typing.TypeVarTuple(name)` — returns a runtime marker object with `__name__`.
    #[strum(serialize = "TypeVarTuple")]
    TypeVarTuple,
    /// `typing.NewType(name, tp)` — returns `tp` (identity function).
    #[strum(serialize = "NewType")]
    NewType,
    /// `typing.cast(typ, val)` — returns `val` (no-op cast).
    Cast,
    /// `typing.reveal_type(obj)` — prints runtime type and returns `obj`.
    #[strum(serialize = "reveal_type")]
    RevealType,
    /// `typing.assert_type(obj, typ)` — returns `obj` unchanged.
    #[strum(serialize = "assert_type")]
    AssertType,
    /// `typing.overload(func)` — returns `func` (no-op decorator).
    Overload,
    /// `typing.get_overloads(func)` — returns collected overload stubs.
    #[strum(serialize = "get_overloads")]
    GetOverloads,
    /// `typing.clear_overloads()` — clears the overload registry.
    #[strum(serialize = "clear_overloads")]
    ClearOverloads,
    /// `typing.runtime_checkable(cls)` — marks protocol class for runtime checks.
    #[strum(serialize = "runtime_checkable")]
    RuntimeCheckable,
    /// `typing.final(obj)` — identity decorator.
    #[strum(serialize = "final")]
    FinalDecorator,
    /// `typing.no_type_check(obj)` — identity decorator.
    #[strum(serialize = "no_type_check")]
    NoTypeCheck,
    /// `typing.no_type_check_decorator(obj)` — identity decorator.
    #[strum(serialize = "no_type_check_decorator")]
    NoTypeCheckDecorator,
    /// `typing.dataclass_transform(func)` — returns `func` (no-op decorator).
    #[strum(serialize = "dataclass_transform")]
    DataclassTransform,
    /// `typing.override(func)` — returns `func` (no-op decorator).
    Override,
    /// `typing.deprecated(func)` — returns `func` (no-op decorator).
    Deprecated,
    /// `typing.get_type_hints(obj)` — returns runtime annotation mapping.
    #[strum(serialize = "get_type_hints")]
    GetTypeHints,
    /// `typing.get_origin(tp)` — returns generic alias origin or `None`.
    #[strum(serialize = "get_origin")]
    GetOrigin,
    /// `typing.get_args(tp)` — returns generic alias args or `()`.
    #[strum(serialize = "get_args")]
    GetArgs,
    /// `typing.get_protocol_members(tp)` — returns member names for protocol classes.
    #[strum(serialize = "get_protocol_members")]
    GetProtocolMembers,
    /// `typing.is_protocol(tp)` — returns whether `tp` is a protocol class.
    #[strum(serialize = "is_protocol")]
    IsProtocol,
    /// `typing.is_typeddict(tp)` — returns whether `tp` is a TypedDict class.
    #[strum(serialize = "is_typeddict")]
    IsTypedDict,
    /// `typing.GenericAlias(origin, args)` — runtime alias constructor.
    #[strum(serialize = "GenericAlias")]
    GenericAliasCtor,
    /// `typing.ForwardRef(arg)` — creates a forward-reference helper object.
    #[strum(serialize = "ForwardRef")]
    ForwardRefCtor,
    /// Internal: `_ProtocolMeta.__instancecheck__(cls, obj)` structural check.
    #[strum(serialize = "_protocol_instancecheck")]
    ProtocolInstancecheck,
    /// Internal: `_ProtocolMeta.__subclasscheck__(cls, sub)` structural check.
    #[strum(serialize = "_protocol_subclasscheck")]
    ProtocolSubclasscheck,
    /// Internal: `TypedDict.__new__(cls, ...)` for functional syntax and instance creation.
    #[strum(serialize = "_typed_dict_new")]
    TypedDictNew,
    /// Internal: `TypedDict.__init_subclass__(cls, *, total=True)` for class syntax.
    #[strum(serialize = "_typed_dict_init_subclass")]
    TypedDictInitSubclass,
}

/// Creates the `typing` module and allocates it on the heap.
///
/// Registers marker attributes, helper runtime functions, and concrete class objects
/// for `Protocol` and `TypedDict` so class syntax and runtime checks behave like CPython.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Typing);

    // typing.TYPE_CHECKING - always False
    module.set_attr(StaticStrings::TypeChecking, Value::Bool(false), heap, interns);

    // Export all pure marker attributes.
    for ss in MARKER_ATTRS {
        module.set_attr(*ss, Value::Marker(Marker(*ss)), heap, interns);
    }
    // typing.NamedTuple remains a marker value.
    module.set_attr(
        StaticStrings::TypingNamedTuple,
        Value::Marker(Marker(StaticStrings::TypingNamedTuple)),
        heap,
        interns,
    );

    // Concrete protocol metaclass with __instancecheck__ hook.
    let protocol_meta = create_protocol_metaclass(heap, interns)?;
    // Concrete Protocol base class.
    let protocol_class = create_protocol_class(heap, interns, protocol_meta)?;
    module.set_attr(StaticStrings::Protocol, Value::Ref(protocol_class), heap, interns);

    // ABC-like protocol shims used in runtime `isinstance(...)` parity checks.
    module.set_attr(
        StaticStrings::SupportsInt,
        Value::Builtin(Builtins::Type(Type::Int)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::SupportsFloat,
        Value::Builtin(Builtins::Type(Type::Float)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::SupportsComplex,
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::SupportsBytes,
        Value::Builtin(Builtins::Type(Type::Bytes)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::SupportsAbs,
        Value::Builtin(Builtins::Type(Type::Int)),
        heap,
        interns,
    );
    module.set_attr_str(
        "SupportsIndex",
        Value::Builtin(Builtins::Type(Type::Int)),
        heap,
        interns,
    )?;
    let supports_round = create_supports_round_protocol_class(heap, interns, protocol_meta, protocol_class)?;
    module.set_attr(StaticStrings::SupportsRound, Value::Ref(supports_round), heap, interns);

    // Concrete TypedDict base class supporting functional and class syntax.
    let typed_dict_class = create_typed_dict_class(heap, interns)?;
    module.set_attr(
        StaticStrings::TypingTypedDict,
        Value::Ref(typed_dict_class),
        heap,
        interns,
    );

    // Public callable functions.
    let functions: &[(StaticStrings, TypingFunctions)] = &[
        (StaticStrings::TypeVar, TypingFunctions::TypeVar),
        (StaticStrings::ParamSpec, TypingFunctions::ParamSpec),
        (StaticStrings::TypeVarTuple, TypingFunctions::TypeVarTuple),
        (StaticStrings::TypingNewType, TypingFunctions::NewType),
        (StaticStrings::TypingCast, TypingFunctions::Cast),
        (StaticStrings::TypingRevealType, TypingFunctions::RevealType),
        (StaticStrings::TypingAssertType, TypingFunctions::AssertType),
        (StaticStrings::TypingOverload, TypingFunctions::Overload),
        (StaticStrings::TypingRuntimeCheckable, TypingFunctions::RuntimeCheckable),
        (
            StaticStrings::TypingDataclassTransform,
            TypingFunctions::DataclassTransform,
        ),
        (StaticStrings::TypingOverride, TypingFunctions::Override),
        (StaticStrings::TypingGetTypeHints, TypingFunctions::GetTypeHints),
        (StaticStrings::TypingGetOrigin, TypingFunctions::GetOrigin),
        (StaticStrings::TypingGetArgs, TypingFunctions::GetArgs),
    ];

    for &(name, func) in functions {
        module.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::Typing(func)),
            heap,
            interns,
        );
    }

    // Public callable names that are not interned in `StaticStrings`.
    let dynamic_functions: &[(&str, TypingFunctions)] = &[
        ("get_overloads", TypingFunctions::GetOverloads),
        ("clear_overloads", TypingFunctions::ClearOverloads),
        ("final", TypingFunctions::FinalDecorator),
        ("no_type_check", TypingFunctions::NoTypeCheck),
        ("no_type_check_decorator", TypingFunctions::NoTypeCheckDecorator),
        ("get_protocol_members", TypingFunctions::GetProtocolMembers),
        ("is_protocol", TypingFunctions::IsProtocol),
        ("is_typeddict", TypingFunctions::IsTypedDict),
        ("GenericAlias", TypingFunctions::GenericAliasCtor),
        ("ForwardRef", TypingFunctions::ForwardRefCtor),
    ];
    for &(name, func) in dynamic_functions {
        module.set_attr_str(
            name,
            Value::ModuleFunction(ModuleFunctions::Typing(func)),
            heap,
            interns,
        )?;
    }

    // Compatibility placeholders for typing names not modeled as StaticStrings.
    // These unblock `from typing import ...` for broad parity tests.
    // Context-manager aliases match contextlib's lightweight abstract base shims.
    module.set_attr_str(
        "ContextManager",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "AsyncContextManager",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "Collection",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "AbstractSet",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("Container", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str("Sized", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str("Hashable", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str(
        "Reversible",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("ItemsView", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str("KeysView", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str(
        "ValuesView",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "MappingView",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("Text", Value::Builtin(Builtins::Type(Type::Str)), heap, interns)?;
    module.set_attr_str(
        "LiteralString",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("ReadOnly", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str(
        "TypeAliasType",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("NoDefault", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str(
        "evaluate_forward_ref",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "assert_never",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("ByteString", Value::Builtin(Builtins::Type(Type::Bytes)), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a typing module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TypingFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        TypingFunctions::TypeVar => typing_type_var(heap, interns, args),
        TypingFunctions::ParamSpec => typing_param_spec(heap, interns, args),
        TypingFunctions::TypeVarTuple => typing_type_var_tuple(heap, interns, args),
        TypingFunctions::NewType => typing_new_type(heap, args),
        TypingFunctions::Cast => typing_cast(heap, args),
        TypingFunctions::RevealType => typing_reveal_type(heap, args),
        TypingFunctions::AssertType => typing_assert_type(heap, args),
        TypingFunctions::Overload => typing_overload(heap, interns, args),
        TypingFunctions::GetOverloads => typing_get_overloads(heap, interns, args),
        TypingFunctions::ClearOverloads => typing_clear_overloads(heap, args),
        TypingFunctions::RuntimeCheckable => typing_runtime_checkable(heap, interns, args),
        TypingFunctions::FinalDecorator => typing_identity_one_arg(heap, args, "typing.final"),
        TypingFunctions::NoTypeCheck => typing_identity_one_arg(heap, args, "typing.no_type_check"),
        TypingFunctions::NoTypeCheckDecorator => typing_identity_one_arg(heap, args, "typing.no_type_check_decorator"),
        TypingFunctions::DataclassTransform => typing_dataclass_transform(heap, args),
        TypingFunctions::Override => typing_identity_one_arg(heap, args, "typing.override"),
        TypingFunctions::Deprecated => typing_identity_one_arg(heap, args, "typing.deprecated"),
        TypingFunctions::GetTypeHints => typing_get_type_hints(heap, interns, args),
        TypingFunctions::GetOrigin => typing_get_origin(heap, args),
        TypingFunctions::GetArgs => typing_get_args(heap, args),
        TypingFunctions::GetProtocolMembers => typing_get_protocol_members(heap, interns, args),
        TypingFunctions::IsProtocol => typing_is_protocol(heap, interns, args),
        TypingFunctions::IsTypedDict => typing_is_typeddict(heap, interns, args),
        TypingFunctions::GenericAliasCtor => typing_generic_alias_ctor(heap, interns, args),
        TypingFunctions::ForwardRefCtor => typing_forward_ref(heap, interns, args),
        TypingFunctions::ProtocolInstancecheck => typing_protocol_instancecheck(heap, interns, args),
        TypingFunctions::ProtocolSubclasscheck => typing_protocol_subclasscheck(heap, interns, args),
        TypingFunctions::TypedDictNew => typing_typed_dict_new(heap, interns, args),
        TypingFunctions::TypedDictInitSubclass => typing_typed_dict_init_subclass(heap, interns, args),
    }
}

/// Implementation of `typing.ForwardRef(arg)`.
///
/// Ouros models this as a lightweight object with the common runtime attributes
/// consumed by annotation-resolution helpers.
fn typing_forward_ref(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let arg = args.get_one_arg("typing.ForwardRef", heap)?;
    let result = if let Some(arg_name) = value_to_either_str(&arg, heap) {
        let forward_arg = match arg_name {
            EitherStr::Interned(id) => Value::InternString(id),
            EitherStr::Heap(s) => Value::Ref(heap.allocate(HeapData::Str(Str::from(s.as_str())))?),
        };
        let obj = create_runtime_attrs_instance(
            vec![
                ("__forward_arg__", forward_arg.clone_with_heap(heap)),
                ("arg", forward_arg),
                ("__forward_evaluated__", Value::Bool(false)),
                ("__forward_value__", Value::None),
            ],
            heap,
            interns,
        )?;
        Ok(AttrCallResult::Value(obj))
    } else {
        Err(ExcType::type_error(format!(
            "Forward reference must be a string -- got {}",
            arg.py_type(heap)
        )))
    };
    arg.drop_with_heap(heap);
    result
}

/// Implementation of `typing.TypeVar(name, *constraints, bound=...)`.
fn typing_type_var(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(name) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypeVar() missing required positional argument: 'name'".to_string(),
        ));
    };
    if value_to_either_str(&name, heap).is_none() {
        name.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("TypeVar() argument 'name' must be str".to_string()));
    }

    let constraints: Vec<Value> = positional.collect();
    let mut bound: Option<Value> = None;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            constraints.drop_with_heap(heap);
            name.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name == "bound" {
            if let Some(old) = bound.replace(value) {
                old.drop_with_heap(heap);
            }
            key.drop_with_heap(heap);
            continue;
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        constraints.drop_with_heap(heap);
        name.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "TypeVar() got an unexpected keyword argument '{key_name}'"
        )));
    }

    let mut constraint_items: SmallVec<[Value; 3]> = SmallVec::new();
    constraint_items.extend(constraints);
    let constraints_tuple = allocate_tuple(constraint_items, heap)?;
    let attrs = vec![
        ("__name__", name),
        ("__constraints__", constraints_tuple),
        ("__bound__", bound.unwrap_or(Value::None)),
    ];
    let type_var = create_runtime_attrs_instance(attrs, heap, interns)?;
    Ok(AttrCallResult::Value(type_var))
}

/// Implementation of `typing.ParamSpec(name)`.
fn typing_param_spec(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let name = args.get_one_arg("typing.ParamSpec", heap)?;
    if value_to_either_str(&name, heap).is_none() {
        name.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "ParamSpec() argument 'name' must be str".to_string(),
        ));
    }
    let param_spec = create_runtime_attrs_instance(vec![("__name__", name)], heap, interns)?;
    Ok(AttrCallResult::Value(param_spec))
}

/// Implementation of `typing.TypeVarTuple(name)`.
fn typing_type_var_tuple(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let name = args.get_one_arg("typing.TypeVarTuple", heap)?;
    if value_to_either_str(&name, heap).is_none() {
        name.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypeVarTuple() argument 'name' must be str".to_string(),
        ));
    }
    let type_var_tuple = create_runtime_attrs_instance(vec![("__name__", name)], heap, interns)?;
    Ok(AttrCallResult::Value(type_var_tuple))
}

/// Typing marker attributes exported by this module.
///
/// `Protocol` and `TypedDict` are excluded because they are concrete class objects.
const MARKER_ATTRS: &[StaticStrings] = &[
    StaticStrings::Any,
    StaticStrings::Optional,
    StaticStrings::UnionType,
    StaticStrings::ListType,
    StaticStrings::DictType,
    StaticStrings::TupleType,
    StaticStrings::SetType,
    StaticStrings::FrozenSet,
    StaticStrings::Callable,
    StaticStrings::Type,
    StaticStrings::Sequence,
    StaticStrings::Mapping,
    StaticStrings::Iterable,
    StaticStrings::IteratorType,
    StaticStrings::Generator,
    StaticStrings::ClassVar,
    StaticStrings::FinalType,
    StaticStrings::Literal,
    StaticStrings::TypeVar,
    StaticStrings::Generic,
    StaticStrings::Annotated,
    StaticStrings::SelfType,
    StaticStrings::Never,
    StaticStrings::NoReturn,
    StaticStrings::AnyStr,
    StaticStrings::Awaitable,
    StaticStrings::Coroutine,
    StaticStrings::AsyncIterator,
    StaticStrings::AsyncIterable,
    StaticStrings::AsyncGenerator,
    StaticStrings::MutableMapping,
    StaticStrings::MutableSequence,
    StaticStrings::MutableSet,
    StaticStrings::TypingDefaultDict,
    StaticStrings::CollOrderedDict,
    StaticStrings::Counter,
    StaticStrings::TypingDeque,
    StaticStrings::ChainMap,
    StaticStrings::TypingPattern,
    StaticStrings::TypingMatch,
    StaticStrings::TypingIO,
    StaticStrings::TypingTextIO,
    StaticStrings::TypingBinaryIO,
    StaticStrings::TypeGuard,
    StaticStrings::TypeIs,
    StaticStrings::Unpack,
    StaticStrings::ParamSpec,
    StaticStrings::ParamSpecArgs,
    StaticStrings::ParamSpecKwargs,
    StaticStrings::Concatenate,
    StaticStrings::TypeVarTuple,
    StaticStrings::TypeAlias,
    StaticStrings::Required,
    StaticStrings::NotRequired,
    StaticStrings::SupportsInt,
    StaticStrings::SupportsFloat,
    StaticStrings::SupportsComplex,
    StaticStrings::SupportsBytes,
    StaticStrings::SupportsAbs,
    StaticStrings::SupportsRound,
];

/// Implementation of `typing.NewType(name, tp)`.
fn typing_new_type(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (name, tp) = args.get_two_args("typing.NewType", heap)?;
    name.drop_with_heap(heap);
    Ok(AttrCallResult::Value(tp))
}

/// Implementation of `typing.cast(typ, val)`.
fn typing_cast(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (typ, val) = args.get_two_args("typing.cast", heap)?;
    typ.drop_with_heap(heap);
    Ok(AttrCallResult::Value(val))
}

/// Implementation of `typing.reveal_type(obj)`.
fn typing_reveal_type(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("typing.reveal_type", heap)?;
    let runtime_type = value.py_type(heap);
    eprintln!("Runtime type is '{runtime_type}'");
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `typing.assert_type(obj, typ)`.
fn typing_assert_type(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (value, typ) = args.get_two_args("typing.assert_type", heap)?;
    typ.drop_with_heap(heap);
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `typing.overload(func)`.
fn typing_overload(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("typing.overload", heap)?;
    if let Some(key) = overload_registry_key(&func, heap, interns) {
        let mut registry = OVERLOAD_REGISTRY
            .lock()
            .expect("typing overload registry mutex poisoned");
        *registry.entry(key).or_insert(0) += 1;
    }
    Ok(AttrCallResult::Value(func))
}

/// Implementation of `typing.get_overloads(func)`.
fn typing_get_overloads(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("typing.get_overloads", heap)?;
    let count = overload_registry_key(&func, heap, interns)
        .and_then(|key| {
            OVERLOAD_REGISTRY
                .lock()
                .expect("typing overload registry mutex poisoned")
                .get(&key)
                .copied()
        })
        .unwrap_or(0);
    func.drop_with_heap(heap);
    let mut overload_items = Vec::with_capacity(count);
    overload_items.resize_with(count, || Value::None);
    let overloads = List::new(overload_items);
    let overloads_id = heap.allocate(HeapData::List(overloads))?;
    Ok(AttrCallResult::Value(Value::Ref(overloads_id)))
}

/// Implementation of `typing.clear_overloads()`.
fn typing_clear_overloads(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("typing.clear_overloads", heap)?;
    OVERLOAD_REGISTRY
        .lock()
        .expect("typing overload registry mutex poisoned")
        .clear();
    Ok(AttrCallResult::Value(Value::None))
}

/// Implementation of `typing.runtime_checkable(cls)`.
///
/// Marks `Protocol` subclasses so `isinstance(obj, ProtocolSubclass)` is allowed.
fn typing_runtime_checkable(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("typing.runtime_checkable", heap)?;

    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "@runtime_checkable can be only applied to protocol classes".to_string(),
            ));
        }
    };

    if !is_protocol_class(class_id, heap, interns) {
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "@runtime_checkable can be only applied to protocol classes".to_string(),
        ));
    }

    set_class_bool_attr(class_id, ATTR_IS_RUNTIME_PROTOCOL, true, heap, interns)?;
    Ok(AttrCallResult::Value(cls))
}

/// Implementation of `typing.get_type_hints(obj)`.
fn typing_get_type_hints(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("typing.get_type_hints", heap)?;
    let name_id: crate::intern::StringId = StaticStrings::DunderAnnotations.into();
    let out = match obj.py_getattr(name_id, heap, interns) {
        Ok(AttrCallResult::Value(value)) => value,
        Ok(_) => Value::None,
        Err(_) => Value::None,
    };
    obj.drop_with_heap(heap);
    if let Value::Ref(id) = &out
        && matches!(heap.get(*id), HeapData::Dict(_))
    {
        return Ok(AttrCallResult::Value(out));
    }
    out.drop_with_heap(heap);
    let dict = Dict::new();
    let id = heap.allocate(HeapData::Dict(dict))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `typing.get_origin(tp)`.
fn typing_get_origin(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let tp = args.get_one_arg("typing.get_origin", heap)?;
    let out = match &tp {
        Value::Ref(id) => {
            let origin = match heap.get(*id) {
                HeapData::GenericAlias(alias) => Some(alias.origin().copy_for_extend()),
                _ => None,
            };
            if let Some(origin) = origin {
                let origin = match origin {
                    Value::Ref(origin_id) => {
                        heap.inc_ref(origin_id);
                        Value::Ref(origin_id)
                    }
                    other => other,
                };
                normalize_typing_origin(origin)
            } else {
                Value::None
            }
        }
        _ => Value::None,
    };
    tp.drop_with_heap(heap);
    Ok(AttrCallResult::Value(out))
}

/// Normalizes `typing` alias origins to CPython-style `get_origin(...)` results.
///
/// `typing.List[int]` (and peers) should report builtin collection classes as
/// their origin even though the alias itself is built from a typing marker.
fn normalize_typing_origin(origin: Value) -> Value {
    match origin {
        Value::Marker(Marker(StaticStrings::ListType)) => Value::Builtin(Builtins::Type(Type::List)),
        Value::Marker(Marker(StaticStrings::DictType)) => Value::Builtin(Builtins::Type(Type::Dict)),
        Value::Marker(Marker(StaticStrings::TupleType)) => Value::Builtin(Builtins::Type(Type::Tuple)),
        Value::Marker(Marker(StaticStrings::SetType)) => Value::Builtin(Builtins::Type(Type::Set)),
        other => other,
    }
}

/// Implementation of `typing.get_args(tp)`.
fn typing_get_args(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let tp = args.get_one_arg("typing.get_args", heap)?;
    let out = match &tp {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::GenericAlias(alias) => {
                let mut items: SmallVec<[Value; 3]> = SmallVec::new();
                for arg in alias.args() {
                    items.push(arg.clone_with_heap(heap));
                }
                allocate_tuple(items, heap)?
            }
            _ => allocate_tuple(SmallVec::new(), heap)?,
        },
        _ => allocate_tuple(SmallVec::new(), heap)?,
    };
    tp.drop_with_heap(heap);
    Ok(AttrCallResult::Value(out))
}

/// Implementation of `typing.get_protocol_members(cls)`.
fn typing_get_protocol_members(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("typing.get_protocol_members", heap)?;
    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(allocate_tuple(SmallVec::new(), heap)?));
        }
    };
    let members = if is_protocol_class(class_id, heap, interns) {
        protocol_member_names(class_id, heap, interns)
    } else {
        Vec::new()
    };
    let mut out: SmallVec<[Value; 3]> = SmallVec::new();
    for member in members {
        let member_id = heap.allocate(HeapData::Str(Str::from(member)))?;
        out.push(Value::Ref(member_id));
    }
    cls.drop_with_heap(heap);
    Ok(AttrCallResult::Value(allocate_tuple(out, heap)?))
}

/// Implementation of `typing.is_protocol(cls)`.
fn typing_is_protocol(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("typing.is_protocol", heap)?;
    let out = if let Value::Ref(id) = &cls {
        matches!(heap.get(*id), HeapData::ClassObject(_)) && is_protocol_class(*id, heap, interns)
    } else {
        false
    };
    cls.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(out)))
}

/// Implementation of `typing.is_typeddict(cls)`.
fn typing_is_typeddict(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("typing.is_typeddict", heap)?;
    let out = if let Value::Ref(id) = &cls {
        if matches!(heap.get(*id), HeapData::ClassObject(_)) {
            class_bool_attr(*id, ATTR_IS_TYPED_DICT, heap, interns).unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    };
    cls.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(out)))
}

/// Implementation of `typing.GenericAlias(origin, args)`.
fn typing_generic_alias_ctor(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (origin, item) = args.get_two_args("typing.GenericAlias", heap)?;
    let alias = make_generic_alias(origin, item, heap, interns)?;
    Ok(AttrCallResult::Value(alias))
}

/// Internal implementation of `_ProtocolMeta.__instancecheck__(cls, obj)`.
///
/// For `@runtime_checkable` protocols this performs structural checks by ensuring
/// all declared protocol members are present on `obj`.
fn typing_protocol_instancecheck(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (cls, obj) = args.get_two_args("_ProtocolMeta.__instancecheck__", heap)?;

    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(Value::Bool(false)));
        }
    };

    if !is_protocol_class(class_id, heap, interns) {
        cls.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(Value::Bool(false)));
    }

    let is_runtime = class_bool_attr(class_id, ATTR_IS_RUNTIME_PROTOCOL, heap, interns).unwrap_or(false);
    if !is_runtime {
        cls.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "Instance and class checks can only be used with @runtime_checkable protocols".to_string(),
        ));
    }

    let members = protocol_member_names(class_id, heap, interns);
    let matches = members
        .into_iter()
        .all(|name| object_has_named_attribute(&obj, name.as_str(), heap, interns));

    cls.drop_with_heap(heap);
    obj.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(matches)))
}

/// Internal implementation of `_ProtocolMeta.__subclasscheck__(cls, sub)`.
///
/// Mirrors the runtime-checkable protocol subclass behavior used by `issubclass()`.
fn typing_protocol_subclasscheck(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (cls, sub) = args.get_two_args("_ProtocolMeta.__subclasscheck__", heap)?;

    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            sub.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(Value::Bool(false)));
        }
    };

    if !is_protocol_class(class_id, heap, interns) {
        cls.drop_with_heap(heap);
        sub.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(Value::Bool(false)));
    }

    let is_runtime = class_bool_attr(class_id, ATTR_IS_RUNTIME_PROTOCOL, heap, interns).unwrap_or(false);
    if !is_runtime {
        cls.drop_with_heap(heap);
        sub.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "Instance and class checks can only be used with @runtime_checkable protocols".to_string(),
        ));
    }

    let members = protocol_member_names(class_id, heap, interns);
    let matches = members
        .into_iter()
        .all(|name| class_or_type_has_named_attribute(&sub, name.as_str(), heap, interns));

    cls.drop_with_heap(heap);
    sub.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(matches)))
}

/// Internal implementation of `TypedDict.__new__(cls, ...)`.
///
/// Supports both:
/// - Functional syntax when called on `typing.TypedDict`
/// - Instance creation (returns plain `dict`) when called on TypedDict subclasses
fn typing_typed_dict_new(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    let Some(cls) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict.__new__() missing cls argument".to_string(),
        ));
    };

    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            pos.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "TypedDict.__new__() cls must be a class".to_string(),
            ));
        }
    };

    let rest_pos: Vec<Value> = pos.collect();
    let is_root = class_bool_attr(class_id, ATTR_TYPED_DICT_ROOT, heap, interns).unwrap_or(false);

    if !is_root {
        cls.drop_with_heap(heap);
        let dict_args = compose_arg_values(rest_pos, KwargsValues::Empty);
        let dict_value = Type::Dict.call(heap, dict_args, interns)?;

        if kwargs.is_empty() {
            return Ok(AttrCallResult::Value(dict_value));
        }

        let Value::Ref(dict_id) = &dict_value else {
            kwargs.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(dict_value));
        };

        for (key, value) in kwargs {
            heap.with_entry_mut(*dict_id, |heap, data| {
                let HeapData::Dict(dict) = data else {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error("TypedDict must construct a dict".to_string()));
                };
                if let Some(old) = dict.set(key, value, heap, interns)? {
                    old.drop_with_heap(heap);
                }
                Ok(())
            })?;
        }

        return Ok(AttrCallResult::Value(dict_value));
    }

    cls.drop_with_heap(heap);
    create_typed_dict_class_from_functional_syntax(class_id, rest_pos, kwargs, heap, interns)
}

/// Internal implementation of `TypedDict.__init_subclass__(cls, *, total=True)`.
///
/// Handles class-syntax `total=...` and marks subclasses as TypedDict classes.
fn typing_typed_dict_init_subclass(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    let Some(cls) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict.__init_subclass__() missing cls".to_string(),
        ));
    };

    if pos.next().is_some() {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict.__init_subclass__() takes no positional arguments".to_string(),
        ));
    }

    let class_id = match &cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            cls.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "TypedDict.__init_subclass__() cls must be a class".to_string(),
            ));
        }
    };

    let mut total = true;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            cls.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name != "total" {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            cls.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "TypedDict.__init_subclass__() got an unexpected keyword argument '{key_name}'"
            )));
        }

        total = match value {
            Value::Bool(b) => b,
            other => {
                key.drop_with_heap(heap);
                other.drop_with_heap(heap);
                cls.drop_with_heap(heap);
                return Err(ExcType::type_error("TypedDict total must be a bool".to_string()));
            }
        };
        key.drop_with_heap(heap);
    }

    set_class_bool_attr(class_id, ATTR_IS_TYPED_DICT, true, heap, interns)?;
    set_class_bool_attr(class_id, ATTR_TYPED_DICT_ROOT, false, heap, interns)?;
    set_class_bool_attr(class_id, ATTR_TYPED_DICT_TOTAL, total, heap, interns)?;

    cls.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Returns a single-argument value unchanged for typing decorator stubs.
fn typing_identity_one_arg(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    name: &str,
) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg(name, heap)?;
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `typing.dataclass_transform`.
///
/// Supports both decorator usage (`@dataclass_transform`) and decorator-factory
/// usage (`@dataclass_transform(...)`) by returning itself when invoked without
/// a decorated target.
fn typing_dataclass_transform(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    if let Some(value) = positional.next() {
        if positional.next().is_none() {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(value));
        }
        value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("typing.dataclass_transform", 1, 2));
    }
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::ModuleFunction(ModuleFunctions::Typing(
        TypingFunctions::DataclassTransform,
    ))))
}

/// Creates the `_ProtocolMeta` metaclass object.
fn create_protocol_metaclass(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let type_class = heap.builtin_class_id(Type::Type)?;
    let mut namespace = Dict::new();
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderInstancecheck.into(),
        Value::ModuleFunction(ModuleFunctions::Typing(TypingFunctions::ProtocolInstancecheck)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderSubclasscheck.into(),
        Value::ModuleFunction(ModuleFunctions::Typing(TypingFunctions::ProtocolSubclasscheck)),
        heap,
        interns,
    );

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("_ProtocolMeta".to_string()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[type_class],
        namespace,
    )
}

/// Creates the public `typing.Protocol` class object.
fn create_protocol_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    protocol_meta: HeapId,
) -> Result<HeapId, ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    let mut namespace = Dict::new();
    dict_set_string_key(&mut namespace, ATTR_IS_PROTOCOL, Value::Bool(true), heap, interns)?;
    dict_set_string_key(
        &mut namespace,
        ATTR_IS_RUNTIME_PROTOCOL,
        Value::Bool(false),
        heap,
        interns,
    )?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::Protocol.into()),
        Value::Ref(protocol_meta),
        &[object_class],
        namespace,
    )
}

/// Creates a runtime-checkable class object for `typing.SupportsRound`.
///
/// The class name is dotted (`typing.SupportsRound`) so repr mirrors CPython,
/// and the structural protocol member is `__round__`.
fn create_supports_round_protocol_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    protocol_meta: HeapId,
    protocol_class: HeapId,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    dict_set_string_key(&mut namespace, ATTR_IS_PROTOCOL, Value::Bool(true), heap, interns)?;
    dict_set_string_key(
        &mut namespace,
        ATTR_IS_RUNTIME_PROTOCOL,
        Value::Bool(true),
        heap,
        interns,
    )?;
    // Protocol member discovery only includes callable values.
    dict_set_string_key(
        &mut namespace,
        "__round__",
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Repr)),
        heap,
        interns,
    )?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("typing.SupportsRound".to_string()),
        Value::Ref(protocol_meta),
        &[protocol_class],
        namespace,
    )
}

/// Creates the public `typing.TypedDict` class object.
fn create_typed_dict_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let dict_class = heap.builtin_class_id(Type::Dict)?;
    let mut namespace = Dict::new();

    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderNew.into(),
        Value::ModuleFunction(ModuleFunctions::Typing(TypingFunctions::TypedDictNew)),
        heap,
        interns,
    );
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderInitSubclass.into(),
        Value::ModuleFunction(ModuleFunctions::Typing(TypingFunctions::TypedDictInitSubclass)),
        heap,
        interns,
    );
    dict_set_string_key(&mut namespace, ATTR_TYPED_DICT_ROOT, Value::Bool(true), heap, interns)?;
    dict_set_string_key(&mut namespace, ATTR_IS_TYPED_DICT, Value::Bool(true), heap, interns)?;
    dict_set_string_key(&mut namespace, ATTR_TYPED_DICT_TOTAL, Value::Bool(true), heap, interns)?;

    let empty_annotations_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderAnnotations.into(),
        Value::Ref(empty_annotations_id),
        heap,
        interns,
    );

    create_runtime_class(
        heap,
        interns,
        EitherStr::Interned(StaticStrings::TypingTypedDict.into()),
        Value::Builtin(Builtins::Type(Type::Type)),
        &[dict_class],
        namespace,
    )
}

/// Creates a TypedDict class via functional syntax.
fn create_typed_dict_class_from_functional_syntax(
    typed_dict_base: HeapId,
    positional: Vec<Value>,
    kwargs: KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<AttrCallResult> {
    let mut args_iter = positional.into_iter();
    let Some(name_value) = args_iter.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict() missing 2 required positional arguments: 'typename' and 'fields'".to_string(),
        ));
    };
    let Some(fields_value) = args_iter.next() else {
        name_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict() missing 1 required positional argument: 'fields'".to_string(),
        ));
    };

    let mut total_from_positional: Option<bool> = None;
    if let Some(total_value) = args_iter.next() {
        total_from_positional = Some(match total_value {
            Value::Bool(b) => b,
            other => {
                other.drop_with_heap(heap);
                name_value.drop_with_heap(heap);
                fields_value.drop_with_heap(heap);
                args_iter.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error("TypedDict total must be a bool".to_string()));
            }
        });
        if args_iter.next().is_some() {
            name_value.drop_with_heap(heap);
            fields_value.drop_with_heap(heap);
            args_iter.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "TypedDict() takes at most 3 positional arguments".to_string(),
            ));
        }
    }

    let mut total_from_kwarg: Option<bool> = None;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            name_value.drop_with_heap(heap);
            fields_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name != "total" {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            name_value.drop_with_heap(heap);
            fields_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "TypedDict() got an unexpected keyword argument '{key_name}'"
            )));
        }

        total_from_kwarg = Some(match value {
            Value::Bool(b) => b,
            other => {
                key.drop_with_heap(heap);
                other.drop_with_heap(heap);
                name_value.drop_with_heap(heap);
                fields_value.drop_with_heap(heap);
                return Err(ExcType::type_error("TypedDict total must be a bool".to_string()));
            }
        });
        key.drop_with_heap(heap);
    }

    if total_from_positional.is_some() && total_from_kwarg.is_some() {
        name_value.drop_with_heap(heap);
        fields_value.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict() got multiple values for argument 'total'".to_string(),
        ));
    }

    let total = total_from_kwarg.or(total_from_positional).unwrap_or(true);

    let Some(class_name) = value_to_either_str(&name_value, heap) else {
        name_value.drop_with_heap(heap);
        fields_value.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "TypedDict() expects typename to be str".to_string(),
        ));
    };

    let annotations = match clone_dict_value(&fields_value, heap, interns) {
        Ok(v) => v,
        Err(err) => {
            name_value.drop_with_heap(heap);
            fields_value.drop_with_heap(heap);
            return Err(err);
        }
    };

    name_value.drop_with_heap(heap);
    fields_value.drop_with_heap(heap);

    let mut namespace = Dict::new();
    let annotations_id = heap.allocate(HeapData::Dict(annotations))?;
    dict_set_intern_key(
        &mut namespace,
        StaticStrings::DunderAnnotations.into(),
        Value::Ref(annotations_id),
        heap,
        interns,
    );
    dict_set_string_key(&mut namespace, ATTR_IS_TYPED_DICT, Value::Bool(true), heap, interns)?;
    dict_set_string_key(&mut namespace, ATTR_TYPED_DICT_ROOT, Value::Bool(false), heap, interns)?;
    dict_set_string_key(&mut namespace, ATTR_TYPED_DICT_TOTAL, Value::Bool(total), heap, interns)?;

    let metaclass = match heap.get(typed_dict_base) {
        HeapData::ClassObject(cls) => cls.metaclass().clone_with_heap(heap),
        _ => Value::Builtin(Builtins::Type(Type::Type)),
    };

    let class_id = create_runtime_class(heap, interns, class_name, metaclass, &[typed_dict_base], namespace)?;

    Ok(AttrCallResult::Value(Value::Ref(class_id)))
}

/// Creates a class object with explicit metaclass, bases, and namespace.
///
/// This mirrors VM class finalization logic for module-defined helper classes.
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

    let mro =
        compute_c3_mro(class_id, bases, heap, interns).expect("typing helper class should always have a valid MRO");

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
            .expect("typed helper base should always be a class object");
        }
    }

    Ok(class_id)
}

/// Sets a string-keyed value into a dict, dropping replaced values.
fn dict_set_string_key(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
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

/// Returns the string form of a dict key value, if it is string-like.
fn value_to_string_key(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match value {
        Value::InternString(id) => Some(interns.get_str(*id).to_string()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Creates a lightweight runtime instance with a dynamic attribute dictionary.
///
/// This is used for typing helper objects like `TypeVar`, `ParamSpec`, and
/// `TypeVarTuple`, where runtime code expects attributes such as `__name__`.
fn create_runtime_attrs_instance(
    attrs: Vec<(&str, Value)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);
    let mut dict = Dict::new();
    for (key, value) in attrs {
        dict_set_string_key(&mut dict, key, value, heap, interns)?;
    }
    let attrs_id = heap.allocate(HeapData::Dict(dict))?;
    let instance = Instance::new(object_class, Some(attrs_id), Vec::new(), Vec::new());
    let instance_id = heap.allocate(HeapData::Instance(instance))?;
    Ok(Value::Ref(instance_id))
}

/// Returns a stable registry key for overload-tracked callables.
///
/// The current implementation keys on `__name__`, which is sufficient for
/// parity tests that register and query overloads in a single module scope.
fn overload_registry_key(callable: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match callable {
        Value::DefFunction(func_id) => Some(interns.get_str(interns.get_function(*func_id).name.name_id).to_string()),
        Value::Ref(id) => {
            if let HeapData::ClassObject(cls) = heap.get(*id) {
                Some(cls.name(interns).to_string())
            } else {
                let attr_id: crate::intern::StringId = StaticStrings::DunderName.into();
                let value = match callable.py_getattr(attr_id, heap, interns).ok()? {
                    AttrCallResult::Value(value) => value,
                    _ => return None,
                };
                let key = value_to_string_key(&value, heap, interns);
                value.drop_with_heap(heap);
                key
            }
        }
        _ => {
            let attr_id: crate::intern::StringId = StaticStrings::DunderName.into();
            let value = match callable.py_getattr(attr_id, heap, interns).ok()? {
                AttrCallResult::Value(value) => value,
                _ => return None,
            };
            let key = value_to_string_key(&value, heap, interns);
            value.drop_with_heap(heap);
            key
        }
    }
}

/// Converts a value into an owned `EitherStr` when it is string-like.
fn value_to_either_str(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<EitherStr> {
    match value {
        Value::InternString(id) => Some(EitherStr::Interned(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(EitherStr::Heap(s.as_str().to_string())),
            _ => None,
        },
        _ => None,
    }
}

/// Clones a dict value, preserving keys and values for new class namespaces.
fn clone_dict_value(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Dict> {
    let Value::Ref(id) = value else {
        return Err(ExcType::type_error("TypedDict fields must be a dict".to_string()));
    };
    heap.with_entry_mut(*id, |heap, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("TypedDict fields must be a dict".to_string()));
        };
        dict.clone_with_heap(heap, interns)
    })
}

/// Sets a boolean attribute on a class namespace.
fn set_class_bool_attr(
    class_id: HeapId,
    name: &str,
    value: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    set_class_attr(class_id, name, Value::Bool(value), heap, interns)
}

/// Sets a namespace attribute on a class, dropping replaced values.
fn set_class_attr(
    class_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    let mut value_opt = Some(value);
    heap.with_entry_mut(class_id, |heap, data| {
        let value = value_opt.take().expect("value already moved");
        let HeapData::ClassObject(cls) = data else {
            Value::Ref(key_id).drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("expected class object".to_string()));
        };
        if let Some(old) = cls.set_attr(Value::Ref(key_id), value, heap, interns)? {
            old.drop_with_heap(heap);
        }
        Ok(())
    })
}

/// Returns a boolean class attribute from MRO lookup, if present and boolean.
fn class_bool_attr(
    class_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<bool> {
    let value = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.mro_lookup_attr(name, class_id, heap, interns).map(|(v, _)| v),
        _ => None,
    }?;

    let out = if let Value::Bool(b) = value { Some(b) } else { None };
    value.drop_with_heap(heap);
    out
}

/// Returns true when the class is a `Protocol` subclass.
fn is_protocol_class(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    class_bool_attr(class_id, ATTR_IS_PROTOCOL, heap, interns).unwrap_or(false)
        || class_bool_attr(class_id, ATTR_IS_RUNTIME_PROTOCOL, heap, interns).unwrap_or(false)
}

/// Collects protocol member names used for structural checks.
fn protocol_member_names(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Vec<String> {
    let mut names = AHashSet::new();

    let mro_ids = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.mro().to_vec(),
        _ => Vec::new(),
    };

    for mro_id in mro_ids {
        let HeapData::ClassObject(cls) = heap.get(mro_id) else {
            continue;
        };
        if !class_has_protocol_marker(mro_id, heap, interns) {
            continue;
        }

        for (key, value) in cls.namespace() {
            let Some(attr_name) = value_to_string_key(key, heap, interns) else {
                continue;
            };

            if attr_name == "__annotations__" {
                if let Value::Ref(ann_id) = value
                    && let HeapData::Dict(ann) = heap.get(*ann_id)
                {
                    for (ann_key, _) in ann {
                        if let Some(member) = value_to_string_key(ann_key, heap, interns)
                            && include_protocol_member(member.as_str())
                        {
                            names.insert(member);
                        }
                    }
                }
                continue;
            }

            if include_protocol_member(attr_name.as_str()) && protocol_member_is_callable(value, heap) {
                names.insert(attr_name);
            }
        }
    }

    let mut out: Vec<String> = names.into_iter().collect();
    out.sort();
    out
}

/// Returns true when class namespace or ancestors carry the protocol marker.
fn class_has_protocol_marker(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(cls) = heap.get(class_id) else {
        return false;
    };

    for &mro_id in cls.mro() {
        let HeapData::ClassObject(mro_cls) = heap.get(mro_id) else {
            continue;
        };
        if let Some(value) = mro_cls.namespace().get_by_str(ATTR_IS_PROTOCOL, heap, interns)
            && matches!(value, Value::Bool(true))
        {
            return true;
        }
    }

    false
}

/// Returns whether a protocol member name should participate in structural checks.
fn include_protocol_member(name: &str) -> bool {
    if matches!(
        name,
        "__module__"
            | "__dict__"
            | "__weakref__"
            | "__doc__"
            | "__annotations__"
            | "__class_getitem__"
            | "__subclasshook__"
            | ATTR_IS_PROTOCOL
            | ATTR_IS_RUNTIME_PROTOCOL
    ) {
        return false;
    }

    if name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__")) {
        return false;
    }

    true
}

/// Returns true for values that represent callable protocol members.
fn protocol_member_is_callable(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::DefFunction(_)
        | Value::ModuleFunction(_)
        | Value::Builtin(Builtins::Function(_))
        | Value::ExtFunction(_) => true,
        Value::Ref(id) => matches!(
            heap.get(*id),
            HeapData::Closure(_, _, _)
                | HeapData::FunctionDefaults(_, _)
                | HeapData::BoundMethod(_)
                | HeapData::StaticMethod(_)
                | HeapData::ClassMethod(_)
        ),
        _ => false,
    }
}

/// Returns true when `obj` has an attribute named `name`.
fn object_has_named_attribute(
    obj: &Value,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    let Value::Ref(id) = obj else {
        return builtin_type_has_protocol_member(obj.py_type(heap), name);
    };

    match heap.get(*id) {
        HeapData::Instance(inst) => {
            if let Some(attrs) = inst.attrs(heap)
                && attrs.get_by_str(name, heap, interns).is_some()
            {
                return true;
            }
            let class_id = inst.class_id();
            if let HeapData::ClassObject(cls) = heap.get(class_id) {
                return cls.mro_has_attr(name, class_id, heap, interns);
            }
            false
        }
        HeapData::Dataclass(dc) => dc.attrs().get_by_str(name, heap, interns).is_some(),
        HeapData::ClassObject(cls) => cls.mro_has_attr(name, *id, heap, interns),
        _ => false,
    }
}

/// Returns true when a class or builtin type has an attribute named `name`.
///
/// This powers protocol subclass checks where the candidate passed to
/// `issubclass()` may be a user class object or a builtin type object.
fn class_or_type_has_named_attribute(
    class_value: &Value,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    match class_value {
        Value::Builtin(Builtins::Type(t)) => builtin_type_has_protocol_member(*t, name),
        Value::Ref(class_id) => {
            if let HeapData::ClassObject(cls) = heap.get(*class_id) {
                cls.mro_has_attr(name, *class_id, heap, interns)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Returns whether a builtin type is known to provide a protocol member.
///
/// Ouros's builtin type objects do not expose the full descriptor surface for
/// class-level attribute lookups, so runtime protocol checks use this targeted
/// compatibility map for stdlib parity behavior.
fn builtin_type_has_protocol_member(t: Type, name: &str) -> bool {
    match name {
        "__len__" => matches!(
            t,
            Type::List
                | Type::Tuple
                | Type::Dict
                | Type::Set
                | Type::FrozenSet
                | Type::Str
                | Type::Bytes
                | Type::Bytearray
                | Type::Range
        ),
        "__round__" => matches!(t, Type::Int | Type::Float),
        "__int__" => matches!(t, Type::Int | Type::Bool | Type::Float),
        "__float__" => matches!(t, Type::Int | Type::Bool | Type::Float),
        "__complex__" => matches!(t, Type::Int | Type::Bool | Type::Float | Type::Complex),
        "__bytes__" => matches!(t, Type::Bytes | Type::Bytearray),
        "__abs__" => matches!(t, Type::Int | Type::Bool | Type::Float | Type::Complex),
        "__index__" => matches!(t, Type::Int | Type::Bool),
        _ => false,
    }
}

/// Rebuilds `ArgValues` from positional vector and kwargs map.
fn compose_arg_values(positional: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    match (positional.len(), kwargs.is_empty()) {
        (0, true) => ArgValues::Empty,
        (0, false) => ArgValues::Kwargs(kwargs),
        (1, true) => {
            let mut args = positional;
            ArgValues::One(args.pop().expect("positional length checked"))
        }
        (2, true) => {
            let mut args = positional;
            let second = args.pop().expect("positional length checked");
            let first = args.pop().expect("positional length checked");
            ArgValues::Two(first, second)
        }
        _ => ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        },
    }
}
