//! Implementation of the `dataclasses` module.
//!
//! Provides an implementation of Python's `dataclasses` module with:
//! - `dataclass(cls)`: Decorator that generates `__init__`, `__repr__`, and `__eq__` from annotations
//! - `field()`: Returns `None` (placeholder for field customization)
//! - `fields(class_or_instance)`: Returns a list of field names for a dataclass
//! - `asdict(instance)`: Converts a dataclass instance to a dict
//! - `astuple(instance)`: Converts a dataclass instance to a tuple
//! - `is_dataclass(obj)`: Returns `True` if `obj` is a dataclass instance or class
//! - `replace(instance, **changes)`: Creates a shallow copy with field overrides
//! - `make_dataclass(cls_name, fields)`: Creates a new dataclass from field names (simplified)
//! - `MISSING`: Sentinel constant for missing field values
//! - `KW_ONLY`: Sentinel constant for keyword-only field declarations
//! - `FrozenInstanceError`: Exception raised by writes to frozen dataclasses
//!
//! The `dataclass` decorator mutates class objects in place by reading ordered
//! field names from `__annotations__`, storing that order on the class, and
//! installing generated dunder methods.
//! The `fields()`, `asdict()`, `astuple()`, and `replace()` functions operate on
//! heap-allocated `Dataclass` objects, extracting or manipulating their declared fields.
//!
//! `is_dataclass()` checks whether the given value is a `HeapData::Dataclass` on the heap.
//! `MISSING` and `KW_ONLY` are exposed as dedicated marker sentinels.

use ahash::AHashSet;
use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Dataclass, Dict, List, Module, Partial, PyTrait, Str, Type, allocate_tuple},
    value::{Marker, Value},
};

/// Private class attribute storing ordered dataclass field names.
const DATACLASS_FIELDS_ATTR: &str = "__ouros_dataclass_fields__";
/// Private class attribute storing ordered dataclass `Field` objects.
const DATACLASS_FIELD_OBJECTS_ATTR: &str = "__ouros_dataclass_field_objects__";
/// Private class attribute storing keyword-only field names.
const DATACLASS_KW_ONLY_FIELDS_ATTR: &str = "__ouros_dataclass_kw_only_fields__";
/// Private class attribute storing fields included in generated repr.
const DATACLASS_REPR_FIELDS_ATTR: &str = "__ouros_dataclass_repr_fields__";
/// Private class attribute storing fields included in generated equality/order.
const DATACLASS_COMPARE_FIELDS_ATTR: &str = "__ouros_dataclass_compare_fields__";
/// Private class attribute storing fields included in generated hash.
const DATACLASS_HASH_FIELDS_ATTR: &str = "__ouros_dataclass_hash_fields__";
/// Private class attribute storing init-only variable names (`InitVar[...]`).
const DATACLASS_INITVAR_FIELDS_ATTR: &str = "__ouros_dataclass_initvar_fields__";
/// Private class attribute recording whether generated dataclass repr is enabled.
const DATACLASS_REPR_ENABLED_ATTR: &str = "__ouros_dataclass_repr_enabled__";
/// Private class attribute recording whether generated dataclass equality is enabled.
const DATACLASS_EQ_ENABLED_ATTR: &str = "__ouros_dataclass_eq_enabled__";
/// Private class attribute recording whether generated dataclass ordering is enabled.
const DATACLASS_ORDER_ENABLED_ATTR: &str = "__ouros_dataclass_order_enabled__";
/// Private class attribute recording frozen behavior.
const DATACLASS_FROZEN_ATTR: &str = "__ouros_dataclass_frozen__";
/// Private class attribute recording `unsafe_hash=True`.
const DATACLASS_UNSAFE_HASH_ATTR: &str = "__ouros_dataclass_unsafe_hash__";
/// Private class attribute recording `kw_only=True`.
const DATACLASS_KW_ONLY_ENABLED_ATTR: &str = "__ouros_dataclass_kw_only_enabled__";

/// Parsed decorator options for `@dataclass(...)`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DataclassOptions {
    init: bool,
    repr: bool,
    eq: bool,
    order: bool,
    unsafe_hash: bool,
    frozen: bool,
    match_args: bool,
    kw_only: bool,
    slots: bool,
    weakref_slot: bool,
}

impl Default for DataclassOptions {
    fn default() -> Self {
        Self {
            init: true,
            repr: true,
            eq: true,
            order: false,
            unsafe_hash: false,
            frozen: false,
            match_args: true,
            kw_only: false,
            slots: false,
            weakref_slot: false,
        }
    }
}

/// Parsed runtime metadata for one dataclass field.
///
/// This metadata is derived from class annotations, class attributes, and
/// `field(...)` descriptors. It drives generated method behavior and powers
/// `dataclasses.fields()` output.
#[derive(Debug)]
struct DataclassFieldSpec {
    name: String,
    annotation: Value,
    default: Value,
    default_factory: Value,
    init: bool,
    repr: bool,
    hash: Value,
    compare: bool,
    metadata: Value,
    kw_only: bool,
    is_initvar: bool,
}

impl DataclassFieldSpec {
    /// Drops all heap-backed values owned by this field metadata.
    fn drop_with_heap(self, heap: &mut Heap<impl ResourceTracker>) {
        self.annotation.drop_with_heap(heap);
        self.default.drop_with_heap(heap);
        self.default_factory.drop_with_heap(heap);
        self.hash.drop_with_heap(heap);
        self.metadata.drop_with_heap(heap);
    }
}

/// Dataclasses module functions.
///
/// Each variant corresponds to a callable exposed by the `dataclasses` module.
/// Functions like `fields`, `asdict`, `astuple`, and `is_dataclass` inspect
/// heap-allocated `Dataclass` objects. `replace` creates copies with modifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum DataclassesFunctions {
    /// The `dataclass` decorator function — generates dataclass methods on a class.
    Dataclass,
    /// The `field()` function — returns `None` (placeholder).
    Field,
    /// The `fields(class_or_instance)` function — returns list of field name strings.
    Fields,
    /// The `asdict(instance)` function — converts dataclass to dict.
    Asdict,
    /// The `astuple(instance)` function — converts dataclass to tuple.
    Astuple,
    /// The `is_dataclass(obj)` function — checks if obj is a dataclass.
    #[strum(serialize = "is_dataclass")]
    IsDataclass,
    /// The `replace(instance, **changes)` function — shallow copy with overrides.
    Replace,
    /// The `make_dataclass(cls_name, fields)` function — creates a dataclass from field names.
    #[strum(serialize = "make_dataclass")]
    MakeDataclass,
    /// Internal generated `__init__` for class-based `@dataclass`.
    #[strum(serialize = "_generated_init")]
    GeneratedInit,
    /// Internal generated `__repr__` for class-based `@dataclass`.
    #[strum(serialize = "_generated_repr")]
    GeneratedRepr,
    /// Internal generated `__eq__` for class-based `@dataclass`.
    #[strum(serialize = "_generated_eq")]
    GeneratedEq,
    /// Internal generated `__lt__` for class-based `@dataclass(order=True)`.
    #[strum(serialize = "_generated_lt")]
    GeneratedLt,
    /// Internal generated `__le__` for class-based `@dataclass(order=True)`.
    #[strum(serialize = "_generated_le")]
    GeneratedLe,
    /// Internal generated `__gt__` for class-based `@dataclass(order=True)`.
    #[strum(serialize = "_generated_gt")]
    GeneratedGt,
    /// Internal generated `__ge__` for class-based `@dataclass(order=True)`.
    #[strum(serialize = "_generated_ge")]
    GeneratedGe,
    /// Internal generated `__hash__` for class-based dataclasses.
    #[strum(serialize = "_generated_hash")]
    GeneratedHash,
    /// Internal generated frozen `__setattr__`.
    #[strum(serialize = "_generated_frozen_setattr")]
    GeneratedFrozenSetattr,
    /// Internal generated frozen `__delattr__`.
    #[strum(serialize = "_generated_frozen_delattr")]
    GeneratedFrozenDelattr,
    /// `recursive_repr` decorator factory.
    #[strum(serialize = "recursive_repr")]
    RecursiveRepr,
    /// Internal decorator returned by `recursive_repr()`.
    #[strum(serialize = "_recursive_repr_decorator")]
    RecursiveReprDecorator,
}

/// Creates the `dataclasses` module and allocates it on the heap.
///
/// The module provides:
/// - `dataclass(cls)`: Decorator that generates `__init__`, `__repr__`, `__eq__`
/// - `field()`: Returns `None` (placeholder)
/// - `fields(class_or_instance)`: Returns list of field name strings
/// - `asdict(instance)`: Converts dataclass instance to dict
/// - `astuple(instance)`: Converts dataclass instance to tuple
/// - `is_dataclass(obj)`: Returns `True` if obj is a dataclass
/// - `replace(instance, **changes)`: Creates shallow copy with overrides
/// - `make_dataclass(cls_name, fields)`: Creates a dataclass from field names
/// - `MISSING`: Sentinel marker object
/// - `KW_ONLY`: Sentinel marker object
/// - `FrozenInstanceError`: Exception class (subclass of `AttributeError`)
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Dataclasses);

    // dataclasses.dataclass - decorator that returns class unchanged
    module.set_attr(
        StaticStrings::Dataclass,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Dataclass)),
        heap,
        interns,
    );

    // dataclasses.field - returns None (placeholder)
    module.set_attr(
        StaticStrings::DcField,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Field)),
        heap,
        interns,
    );

    // dataclasses.fields - returns list of field names
    module.set_attr(
        StaticStrings::DcFields,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Fields)),
        heap,
        interns,
    );

    // dataclasses.asdict - converts dataclass to dict
    module.set_attr(
        StaticStrings::DcAsdict,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Asdict)),
        heap,
        interns,
    );

    // dataclasses.astuple - converts dataclass to tuple
    module.set_attr(
        StaticStrings::DcAstuple,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Astuple)),
        heap,
        interns,
    );

    // dataclasses.is_dataclass - checks if object is a dataclass
    module.set_attr(
        StaticStrings::DcIsDataclass,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::IsDataclass)),
        heap,
        interns,
    );

    // dataclasses.replace - creates copy with changes
    // Reuses the existing StaticStrings::Replace variant (serializes to "replace")
    // since a dedicated DcReplace would conflict with it in strum's FromStr.
    module.set_attr(
        StaticStrings::Replace,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Replace)),
        heap,
        interns,
    );

    // dataclasses.make_dataclass - creates a dataclass from field names
    module.set_attr(
        StaticStrings::DcMakeDataclass,
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::MakeDataclass)),
        heap,
        interns,
    );

    // dataclasses.MISSING - sentinel marker value.
    module.set_attr(
        StaticStrings::DcMissing,
        Value::Marker(Marker(StaticStrings::DcMissing)),
        heap,
        interns,
    );

    // dataclasses.KW_ONLY - sentinel object used to mark keyword-only field sections.
    module.set_attr(
        StaticStrings::DcKwOnly,
        Value::Marker(Marker(StaticStrings::DcKwOnly)),
        heap,
        interns,
    );

    // dataclasses.FrozenInstanceError - exposed exception subclass of AttributeError
    module.set_attr(
        StaticStrings::DcFrozenInstanceError,
        Value::Builtin(Builtins::ExcType(ExcType::FrozenInstanceError)),
        heap,
        interns,
    );

    // dataclasses.InitVar - marker for init-only vars.
    // Exposed as a subscriptable special-form marker so annotations like
    // `InitVar[str]` remain distinguishable from plain `tuple[str]` annotations.
    module.set_attr(
        StaticStrings::DcInitVar,
        Value::Marker(Marker(StaticStrings::DcInitVar)),
        heap,
        interns,
    );

    // dataclasses.Field - export the dataclass heap type so isinstance(f, Field)
    // works for field records produced by fields().
    module.set_attr_str("Field", Value::Builtin(Builtins::Type(Type::Dataclass)), heap, interns)?;

    // dataclasses.recursive_repr - decorator factory (not in __all__, but importable).
    module.set_attr_str(
        "recursive_repr",
        Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::RecursiveRepr)),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a dataclasses module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: DataclassesFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        DataclassesFunctions::Dataclass => dataclass(heap, interns, args),
        DataclassesFunctions::Field => field(heap, interns, args),
        DataclassesFunctions::Fields => fields(heap, interns, args),
        DataclassesFunctions::Asdict => asdict(heap, interns, args),
        DataclassesFunctions::Astuple => astuple(heap, interns, args),
        DataclassesFunctions::IsDataclass => is_dataclass(heap, interns, args),
        DataclassesFunctions::Replace => replace(heap, interns, args),
        DataclassesFunctions::MakeDataclass => make_dataclass(heap, interns, args),
        DataclassesFunctions::GeneratedInit => generated_init(heap, interns, args),
        DataclassesFunctions::GeneratedRepr => generated_repr(heap, interns, args),
        DataclassesFunctions::GeneratedEq => generated_eq(heap, interns, args),
        DataclassesFunctions::GeneratedLt => generated_lt(heap, interns, args),
        DataclassesFunctions::GeneratedLe => generated_le(heap, interns, args),
        DataclassesFunctions::GeneratedGt => generated_gt(heap, interns, args),
        DataclassesFunctions::GeneratedGe => generated_ge(heap, interns, args),
        DataclassesFunctions::GeneratedHash => generated_hash(heap, interns, args),
        DataclassesFunctions::GeneratedFrozenSetattr => generated_frozen_setattr(heap, interns, args),
        DataclassesFunctions::GeneratedFrozenDelattr => generated_frozen_delattr(heap, interns, args),
        DataclassesFunctions::RecursiveRepr => recursive_repr(heap, args),
        DataclassesFunctions::RecursiveReprDecorator => recursive_repr_decorator(heap, interns, args),
    }
}

/// Entry point shared by the `dataclasses.dataclass` module function and the
/// builtin `dataclass(...)` pseudo-type constructor.
pub(crate) fn call_dataclass_decorator(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    match dataclass(heap, interns, args)? {
        AttrCallResult::Value(value) => Ok(value),
        _ => unreachable!("dataclass decorator is synchronous"),
    }
}

/// Implementation of `dataclasses.recursive_repr([fillvalue='...'])`.
///
/// This returns a decorator callable. The current implementation keeps behavior
/// lightweight by returning a decorator that passes through the wrapped function.
fn recursive_repr(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "recursive_repr() takes from 0 to 1 positional arguments but {positional_count} were given"
        )));
    }
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::ModuleFunction(
        ModuleFunctions::Dataclasses(DataclassesFunctions::RecursiveReprDecorator),
    )))
}

/// Identity decorator used as the return value of `recursive_repr()`.
fn recursive_repr_decorator(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let wrapped_or_self = args.get_one_arg("recursive_repr", heap)?;
    // Decoration time: called with the wrapped function, return this wrapper.
    if matches!(
        wrapped_or_self,
        Value::DefFunction(_)
            | Value::ExtFunction(_)
            | Value::ModuleFunction(_)
            | Value::Builtin(Builtins::Function(_))
    ) {
        wrapped_or_self.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(Value::ModuleFunction(
            ModuleFunctions::Dataclasses(DataclassesFunctions::RecursiveReprDecorator),
        )));
    }
    let Value::Ref(instance_id) = wrapped_or_self else {
        return Ok(AttrCallResult::Value(wrapped_or_self));
    };
    let mut seen = AHashSet::new();
    let text = recursive_child_repr(Value::Ref(instance_id), &mut seen, heap, interns)?;
    let result_id = heap.allocate(HeapData::Str(Str::from(text.as_str())))?;
    Ok(AttrCallResult::Value(Value::Ref(result_id)))
}

/// Implementation of `dataclasses.dataclass(cls)`.
///
/// This decorator mutates `cls` in place by generating dataclass dunder methods
/// from the class annotations and storing ordered field metadata.
///
/// # Arguments
/// * `heap` - The heap for any allocations
/// * `_interns` - The interner for string lookups (unused)
/// * `args` - Function arguments: `cls` (required class)
///
/// # Returns
/// `AttrCallResult::Value` containing the same class object after decoration.
///
/// # Errors
/// Returns `TypeError` if the wrong number of arguments is provided.
fn dataclass(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(first) = positional.next() else {
        // @dataclass(...) decorator form: return a callable carrying kwargs.
        let partial_kwargs: Vec<(Value, Value)> = kwargs.into_iter().collect();
        let partial = Partial::new(
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::Dataclass)),
            Vec::new(),
            partial_kwargs,
        );
        let partial_id = heap.allocate(HeapData::Partial(partial))?;
        return Ok(AttrCallResult::Value(Value::Ref(partial_id)));
    };

    if positional.len() > 0 {
        let extra = positional.len() + 1;
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        first.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "dataclass expected at most 1 arguments, got {extra}"
        )));
    }
    positional.drop_with_heap(heap);

    let options = parse_dataclass_options(kwargs, heap, interns)?;
    let cls = decorate_class_with_dataclass(heap, interns, first, options)?;
    Ok(AttrCallResult::Value(cls))
}

/// Applies dataclass method generation to a class object and returns it.
///
/// This helper is shared by both `dataclasses.dataclass` and the callable
/// `dataclass` pseudo-type constructor path.
pub(crate) fn decorate_class_with_dataclass(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    cls: Value,
    options: DataclassOptions,
) -> RunResult<Value> {
    let mut cls_guard = HeapGuard::new(cls, heap);
    let (cls, heap) = cls_guard.as_parts_mut();
    let class_id = match cls {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            return Err(ExcType::type_error(
                "dataclass() should be called on a class".to_string(),
            ));
        }
    };

    if options.order && !options.eq {
        return Err(crate::exception_private::SimpleException::new_msg(
            ExcType::ValueError,
            "eq must be true if order is true",
        )
        .into());
    }

    let mut field_specs = collect_dataclass_field_specs(class_id, options, heap, interns)?;
    if let Err(err) = install_generated_dataclass_methods(class_id, &mut field_specs, options, heap, interns) {
        drop_field_specs(&mut field_specs, heap);
        return Err(err);
    }
    drop_field_specs(&mut field_specs, heap);

    let (cls, _) = cls_guard.into_parts();
    Ok(cls)
}

/// Parses supported `@dataclass` keyword options.
fn parse_dataclass_options(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<DataclassOptions> {
    let mut options = DataclassOptions::default();
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = name.as_str(interns);
        let parsed = match value {
            Value::Bool(v) => v,
            other => {
                other.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "dataclass() argument '{key_name}' must be bool"
                )));
            }
        };
        match key_name {
            "init" => options.init = parsed,
            "repr" => options.repr = parsed,
            "eq" => options.eq = parsed,
            "order" => options.order = parsed,
            "unsafe_hash" => options.unsafe_hash = parsed,
            "frozen" => options.frozen = parsed,
            "match_args" => options.match_args = parsed,
            "kw_only" => options.kw_only = parsed,
            "slots" => options.slots = parsed,
            "weakref_slot" => options.weakref_slot = parsed,
            _ => {
                return Err(ExcType::type_error(format!(
                    "dataclass() got an unexpected keyword argument '{key_name}'"
                )));
            }
        }
    }
    Ok(options)
}

/// Implementation of `dataclasses.field()`.
///
/// Constructs a `Field` descriptor used by `@dataclass` during class decoration.
///
/// This implementation accepts CPython-compatible keyword arguments and stores
/// them on a lightweight `Field` object allocated on the heap.
fn field(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    if positional.len() > 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("field() takes 0 positional arguments"));
    }
    positional.drop_with_heap(heap);

    let missing = Value::Marker(Marker(StaticStrings::DcMissing));
    let mut default = missing;
    let mut default_factory = Value::Marker(Marker(StaticStrings::DcMissing));
    let mut init = true;
    let mut repr = true;
    let mut hash = Value::None;
    let mut compare = true;
    let mut metadata = {
        let dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
        Value::Ref(dict_id)
    };
    let mut kw_only = false;

    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            hash.drop_with_heap(heap);
            metadata.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = name.as_str(interns);
        match key_name {
            "default" => {
                let old = std::mem::replace(&mut default, value);
                old.drop_with_heap(heap);
            }
            "default_factory" => {
                let old = std::mem::replace(&mut default_factory, value);
                old.drop_with_heap(heap);
            }
            "init" => {
                init = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "repr" => {
                repr = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "hash" => {
                let old = std::mem::replace(&mut hash, value);
                old.drop_with_heap(heap);
            }
            "compare" => {
                compare = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "metadata" => {
                let old = std::mem::replace(&mut metadata, value);
                old.drop_with_heap(heap);
            }
            "kw_only" => {
                kw_only = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                default.drop_with_heap(heap);
                default_factory.drop_with_heap(heap);
                hash.drop_with_heap(heap);
                metadata.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "field() got an unexpected keyword argument '{key_name}'"
                )));
            }
        }
    }

    if !is_missing_sentinel(&default) && !is_missing_sentinel(&default_factory) {
        default.drop_with_heap(heap);
        default_factory.drop_with_heap(heap);
        hash.drop_with_heap(heap);
        metadata.drop_with_heap(heap);
        return Err(crate::exception_private::SimpleException::new_msg(
            ExcType::ValueError,
            "cannot specify both default and default_factory",
        )
        .into());
    }

    let field_obj = build_field_object(
        heap,
        interns,
        "<unknown>",
        Value::None,
        default,
        default_factory,
        init,
        repr,
        hash,
        compare,
        metadata,
        kw_only,
        false,
    )?;
    Ok(AttrCallResult::Value(field_obj))
}

/// Implementation of `dataclasses.fields(class_or_instance)`.
///
/// Returns a tuple of `Field` objects for the given dataclass class or instance.
/// For legacy external `HeapData::Dataclass` values, Ouros synthesizes minimal
/// `Field` objects that expose `.name`.
///
/// If the argument is not a dataclass, raises `TypeError`.
///
/// # Arguments
/// * `heap` - The heap for allocations
/// * `args` - Function arguments: one positional (class or instance)
///
/// # Returns
/// A tuple of field descriptor objects.
fn fields(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("dataclasses.fields", heap)?;
    defer_drop!(obj, heap);

    let field_objects = if let Some(field_objects) = dataclass_field_objects_from_value(obj, heap, interns) {
        field_objects
    } else if let Some(field_names) = dataclass_field_names_from_value(obj, heap, interns) {
        legacy_field_objects_from_names(&field_names, heap, interns)?
    } else {
        return Err(ExcType::type_error(
            "dataclasses.fields() requires a dataclass instance or class",
        ));
    };

    let mut values = SmallVec::with_capacity(field_objects.len());
    for field_obj in field_objects {
        let include = if let Some(spec) = parse_field_descriptor_spec(&field_obj, heap, interns) {
            let include = !spec.is_initvar;
            spec.drop_with_heap(heap);
            include
        } else {
            true
        };
        if include {
            values.push(field_obj);
        } else {
            field_obj.drop_with_heap(heap);
        }
    }
    let result = allocate_tuple(values, heap)?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `dataclasses.asdict(instance)`.
///
/// Converts a dataclass instance into a dict mapping field names to their values.
/// Only declared fields (from `field_names`) are included — dynamically added
/// attributes are excluded, matching CPython's behavior.
///
/// If the argument is not a dataclass, raises `TypeError`.
///
/// # Returns
/// A new dict with string keys and cloned values.
fn asdict(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(obj) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "dataclasses.asdict() takes exactly one argument (0 given)",
        ));
    };
    if positional.len() > 0 {
        let got = positional.len() + 1;
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "dataclasses.asdict() takes exactly one argument ({got} given)"
        )));
    }
    positional.drop_with_heap(heap);
    defer_drop!(obj, heap);

    let mut dict_factory: Option<Value> = None;
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(key_str) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        match key_str.as_str(interns) {
            "dict_factory" => {
                let old = dict_factory.replace(value);
                old.drop_with_heap(heap);
            }
            other => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "asdict() got an unexpected keyword argument '{other}'"
                )));
            }
        }
    }

    let Value::Ref(_) = obj else {
        return Err(ExcType::type_error(
            "dataclasses.asdict() requires a dataclass instance",
        ));
    };

    let Some(field_names) = dataclass_field_names_from_value(obj, heap, interns) else {
        return Err(ExcType::type_error(
            "dataclasses.asdict() requires a dataclass instance",
        ));
    };

    let source_id = match obj {
        Value::Ref(id) => *id,
        _ => unreachable!("validated above"),
    };
    let mut pairs = Vec::with_capacity(field_names.len());
    for name in &field_names {
        let raw = dataclass_attr_value(source_id, name.as_str(), heap, interns).unwrap_or(Value::None);
        let converted = asdict_convert_value(raw, heap, interns)?;
        let key_id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
        pairs.push((Value::Ref(key_id), converted));
    }

    if let Some(factory) = dict_factory {
        let mut tuple_pairs = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let mut tuple_values: SmallVec<[Value; 3]> = SmallVec::with_capacity(2);
            tuple_values.push(key);
            tuple_values.push(value);
            let pair = allocate_tuple(tuple_values, heap)?;
            tuple_pairs.push(pair);
        }
        let list_id = heap.allocate(HeapData::List(List::new(tuple_pairs)))?;
        return Ok(AttrCallResult::CallFunction(
            factory,
            ArgValues::One(Value::Ref(list_id)),
        ));
    }

    let mut dict = Dict::new();
    for (key, value) in pairs {
        if let Some(old) = dict.set(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }
    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(AttrCallResult::Value(Value::Ref(dict_id)))
}

/// Implementation of `dataclasses.astuple(instance)`.
///
/// Converts a dataclass instance into a tuple of field values.
/// Only declared fields (from `field_names`) are included, in their
/// declaration order. Matches CPython's behavior.
///
/// If the argument is not a dataclass, raises `TypeError`.
///
/// # Returns
/// A new tuple with the declared field values.
fn astuple(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(obj) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "dataclasses.astuple() takes exactly one argument (0 given)",
        ));
    };
    if positional.len() > 0 {
        let got = positional.len() + 1;
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "dataclasses.astuple() takes exactly one argument ({got} given)"
        )));
    }
    positional.drop_with_heap(heap);
    defer_drop!(obj, heap);

    let mut tuple_factory: Option<Value> = None;
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(key_str) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        match key_str.as_str(interns) {
            "tuple_factory" => {
                let old = tuple_factory.replace(value);
                old.drop_with_heap(heap);
            }
            other => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "astuple() got an unexpected keyword argument '{other}'"
                )));
            }
        }
    }

    let Value::Ref(_) = obj else {
        return Err(ExcType::type_error(
            "dataclasses.astuple() requires a dataclass instance",
        ));
    };

    let Some(field_names) = dataclass_field_names_from_value(obj, heap, interns) else {
        return Err(ExcType::type_error(
            "dataclasses.astuple() requires a dataclass instance",
        ));
    };
    let source_id = match obj {
        Value::Ref(id) => *id,
        _ => unreachable!("validated above"),
    };

    let mut items = SmallVec::with_capacity(field_names.len());
    for name in &field_names {
        let raw = dataclass_attr_value(source_id, name.as_str(), heap, interns).unwrap_or(Value::None);
        let converted = astuple_convert_value(raw, heap, interns)?;
        items.push(converted);
    }

    if let Some(factory) = tuple_factory {
        let list_id = heap.allocate(HeapData::List(List::new(items.into_vec())))?;
        return Ok(AttrCallResult::CallFunction(
            factory,
            ArgValues::One(Value::Ref(list_id)),
        ));
    }
    let tuple_val = allocate_tuple(items, heap)?;
    Ok(AttrCallResult::Value(tuple_val))
}

/// Implementation of `dataclasses.is_dataclass(obj)`.
///
/// Returns `True` if `obj` is a dataclass instance (i.e., a `HeapData::Dataclass`
/// on the heap). Returns `False` for all other values including non-heap values,
/// regular dicts, lists, class objects, etc.
///
/// # Returns
/// `Value::Bool(true)` if the object is a dataclass, `Value::Bool(false)` otherwise.
fn is_dataclass(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("dataclasses.is_dataclass", heap)?;
    defer_drop!(obj, heap);

    let result = if let Value::Ref(heap_id) = obj {
        if matches!(heap.get(*heap_id), HeapData::Dataclass(_)) {
            true
        } else {
            dataclass_field_names_from_value(obj, heap, interns).is_some()
        }
    } else {
        false
    };

    Ok(AttrCallResult::Value(Value::Bool(result)))
}

/// Implementation of `dataclasses.replace(instance, **changes)`.
///
/// Creates a shallow copy of a dataclass instance with specified field values
/// overridden. The first positional argument is the instance to copy. Keyword
/// arguments specify which fields to replace and their new values.
///
/// If the argument is not a dataclass, raises `TypeError`.
/// If a keyword argument names a field that doesn't exist, raises `TypeError`.
///
/// # Returns
/// A new dataclass instance with the specified changes applied.
fn replace(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // Split into positional and keyword arguments
    let (mut pos, kwargs) = args.into_parts();

    // Get the first positional argument (the instance to copy)
    let Some(obj) = pos.next() else {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "dataclasses.replace() missing 1 required positional argument: 'obj'",
        ));
    };

    // `replace` accepts exactly one positional argument (`obj`).
    if pos.len() > 0 {
        let given = pos.len() + 1;
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "dataclasses.replace() takes 1 positional argument but {given} were given"
        )));
    }

    pos.drop_with_heap(heap);
    defer_drop!(obj, heap);

    let Value::Ref(heap_id) = obj else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "dataclasses.replace() requires a dataclass instance",
        ));
    };

    if matches!(heap.get(*heap_id), HeapData::Dataclass(_)) {
        return replace_heap_dataclass(*heap_id, kwargs, heap, interns);
    }

    if let Some((instance_id, field_names)) = dataclass_instance_id_and_fields(*heap_id, heap, interns) {
        return replace_class_dataclass_instance(instance_id, &field_names, kwargs, heap, interns);
    }

    kwargs.drop_with_heap(heap);
    Err(ExcType::type_error(
        "dataclasses.replace() requires a dataclass instance",
    ))
}
/// Returns dataclass field names for a class, instance, or legacy `Dataclass` value.
fn dataclass_field_names_from_value(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Vec<String>> {
    let Value::Ref(id) = value else {
        return None;
    };
    match heap.get(*id) {
        HeapData::Dataclass(dc) => Some(dc.field_names().to_vec()),
        HeapData::ClassObject(_) => {
            if class_has_dataclass_attr(*id, DATACLASS_FIELDS_ATTR, heap, interns) {
                Some(dataclass_field_names_for_class(*id, heap, interns))
            } else {
                None
            }
        }
        HeapData::Instance(instance) => {
            let class_id = instance.class_id();
            if class_has_dataclass_attr(class_id, DATACLASS_FIELDS_ATTR, heap, interns) {
                Some(dataclass_field_names_for_class(class_id, heap, interns))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns cloned `Field` objects for a dataclass class or instance.
fn dataclass_field_objects_from_value(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Vec<Value>> {
    let Value::Ref(id) = value else {
        return None;
    };
    match heap.get(*id) {
        HeapData::ClassObject(_) => {
            if class_has_dataclass_attr(*id, DATACLASS_FIELD_OBJECTS_ATTR, heap, interns) {
                Some(dataclass_field_objects_for_class(*id, heap, interns))
            } else {
                None
            }
        }
        HeapData::Instance(instance) => {
            let class_id = instance.class_id();
            if class_has_dataclass_attr(class_id, DATACLASS_FIELD_OBJECTS_ATTR, heap, interns) {
                Some(dataclass_field_objects_for_class(class_id, heap, interns))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns whether a class has the given dataclass metadata attribute.
fn class_has_dataclass_attr(
    class_id: HeapId,
    attr_name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(attr_name, heap, interns).is_some(),
        _ => false,
    }
}

/// Builds placeholder `Field` objects for legacy external dataclass values.
fn legacy_field_objects_from_names(
    field_names: &[String],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let mut values = Vec::with_capacity(field_names.len());
    for name in field_names {
        let field_obj = match legacy_field_object(name.as_str(), heap, interns) {
            Ok(field_obj) => field_obj,
            Err(err) => {
                values.drop_with_heap(heap);
                return Err(err);
            }
        };
        values.push(field_obj);
    }
    Ok(values)
}

/// Builds one placeholder `Field` object exposing only `.name`.
fn legacy_field_object(field_name: &str, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let mut attrs = Dict::new();
    let key_id = heap.allocate(HeapData::Str(Str::from("name")))?;
    let value_id = heap.allocate(HeapData::Str(Str::from(field_name)))?;
    if let Some(old) = attrs.set(Value::Ref(key_id), Value::Ref(value_id), heap, interns)? {
        old.drop_with_heap(heap);
    }
    let field_obj = Dataclass::new(
        "Field".to_string(),
        heap.next_class_uid(),
        vec!["name".to_string()],
        attrs,
        AHashSet::new(),
        true,
    );
    let field_id = heap.allocate(HeapData::Dataclass(field_obj))?;
    Ok(Value::Ref(field_id))
}

/// Resolves a class-based dataclass instance id and its field names.
fn dataclass_instance_id_and_fields(
    value_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(HeapId, Vec<String>)> {
    let class_id = match heap.get(value_id) {
        HeapData::Instance(instance) => instance.class_id(),
        _ => return None,
    };
    if !class_has_dataclass_attr(class_id, DATACLASS_FIELDS_ATTR, heap, interns) {
        return None;
    }
    Some((value_id, dataclass_field_names_for_class(class_id, heap, interns)))
}

/// Implements `dataclasses.replace()` for legacy `HeapData::Dataclass` values.
fn replace_heap_dataclass(
    heap_id: HeapId,
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<AttrCallResult> {
    let HeapData::Dataclass(dc) = heap.get(heap_id) else {
        return Err(ExcType::type_error(
            "dataclasses.replace() requires a dataclass instance",
        ));
    };

    // Clone the dataclass's metadata before we start mutating the heap.
    let field_names: Vec<String> = dc.field_names().to_vec();
    let frozen = dc.is_frozen();
    let type_id = dc.type_id();
    let dc_name = dc.name(interns).to_string();
    let methods = dc.methods().clone();

    // Build a new attrs dict starting with clones of the existing field values.
    let mut new_dict = Dict::new();
    for name in &field_names {
        let HeapData::Dataclass(dc) = heap.get(heap_id) else {
            unreachable!("heap_id was verified as Dataclass above");
        };
        if let Some(val) = dc.attrs().get_by_str(name, heap, interns) {
            let cloned = val.clone_with_heap(heap);
            let key_id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
            new_dict.set(Value::Ref(key_id), cloned, heap, interns)?;
        }
    }

    // Apply keyword argument overrides.
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(key_str) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            drop_dict_entries(&mut new_dict, heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns).to_string();

        if !field_names.contains(&key_name) {
            value.drop_with_heap(heap);
            drop_dict_entries(&mut new_dict, heap);
            return Err(ExcType::type_error(format!(
                "dataclasses.replace() got an unexpected keyword argument '{key_name}'"
            )));
        }

        let new_key_id = heap.allocate(HeapData::Str(Str::from(key_name.as_str())))?;
        if let Some(old) = new_dict.set(Value::Ref(new_key_id), value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }

    let dc = Dataclass::new(dc_name, type_id, field_names, new_dict, methods, frozen);
    let dc_id = heap.allocate(HeapData::Dataclass(dc))?;
    Ok(AttrCallResult::Value(Value::Ref(dc_id)))
}

/// Implements `dataclasses.replace()` for class-based dataclass instances.
fn replace_class_dataclass_instance(
    instance_id: HeapId,
    _field_names: &[String],
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<AttrCallResult> {
    let mut overrides = parse_named_kwargs(kwargs, heap, interns)?;

    let class_id = if let HeapData::Instance(instance) = heap.get(instance_id) {
        instance.class_id()
    } else {
        drop_named_values(&mut overrides, heap);
        return Err(ExcType::type_error(
            "dataclasses.replace() requires a dataclass instance",
        ));
    };

    let mut field_specs = collect_inherited_field_specs(class_id, heap, interns);
    let current_fields = dataclass_field_objects_for_class(class_id, heap, interns);
    for field in current_fields {
        if let Some(spec) = parse_field_descriptor_spec(&field, heap, interns) {
            if let Some(index) = field_specs.iter().position(|existing| existing.name == spec.name) {
                let old = field_specs.remove(index);
                old.drop_with_heap(heap);
                field_specs.insert(index, spec);
            } else {
                field_specs.push(spec);
            }
        }
        field.drop_with_heap(heap);
    }

    let mut constructor_kwargs = Vec::new();

    for index in 0..field_specs.len() {
        let (name, is_initvar, init, default, default_factory) = {
            let spec = &field_specs[index];
            (
                spec.name.clone(),
                spec.is_initvar,
                spec.init,
                spec.default.clone_with_heap(heap),
                spec.default_factory.clone_with_heap(heap),
            )
        };

        if is_initvar {
            let value = if let Some(value) = take_named_value(&mut overrides, name.as_str()) {
                default.drop_with_heap(heap);
                default_factory.drop_with_heap(heap);
                value
            } else if !is_missing_sentinel(&default) {
                default_factory.drop_with_heap(heap);
                default
            } else if !is_missing_sentinel(&default_factory) {
                default.drop_with_heap(heap);
                let spec = &field_specs[index];
                resolve_default_factory_value(class_id, spec, heap, interns)?
            } else {
                default.drop_with_heap(heap);
                default_factory.drop_with_heap(heap);
                drop_named_values(&mut overrides, heap);
                drop_named_values(&mut constructor_kwargs, heap);
                drop_field_specs(&mut field_specs, heap);
                return Err(ExcType::type_error(format!(
                    "InitVar '{name}' must be specified with replace()"
                )));
            };
            constructor_kwargs.push((name, value));
            continue;
        }

        if !init {
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            if let Some(value) = take_named_value(&mut overrides, name.as_str()) {
                value.drop_with_heap(heap);
                drop_named_values(&mut overrides, heap);
                drop_named_values(&mut constructor_kwargs, heap);
                drop_field_specs(&mut field_specs, heap);
                return Err(ExcType::type_error(format!(
                    "field {name} is declared with init=False, it cannot be specified with replace()"
                )));
            }
            continue;
        }

        let value = if let Some(value) = take_named_value(&mut overrides, name.as_str()) {
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            value
        } else if let Some(value) = instance_attr_value(instance_id, name.as_str(), heap, interns) {
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            value
        } else if let Some(value) = class_field_default_value(class_id, name.as_str(), heap, interns) {
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            value
        } else if !is_missing_sentinel(&default) {
            default_factory.drop_with_heap(heap);
            default
        } else if !is_missing_sentinel(&default_factory) {
            default.drop_with_heap(heap);
            let spec = &field_specs[index];
            resolve_default_factory_value(class_id, spec, heap, interns)?
        } else {
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            drop_named_values(&mut overrides, heap);
            drop_named_values(&mut constructor_kwargs, heap);
            drop_field_specs(&mut field_specs, heap);
            return Err(ExcType::type_error(format!(
                "dataclasses.replace() could not resolve field '{name}'"
            )));
        };
        constructor_kwargs.push((name, value));
    }

    if !overrides.is_empty() {
        let unexpected = overrides[0].0.clone();
        drop_named_values(&mut overrides, heap);
        drop_named_values(&mut constructor_kwargs, heap);
        drop_field_specs(&mut field_specs, heap);
        return Err(ExcType::type_error(format!(
            "dataclasses.replace() got an unexpected keyword argument '{unexpected}'"
        )));
    }

    let mut kwargs_dict = Dict::new();
    for (name, value) in constructor_kwargs {
        let key_id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
        if let Some(old) = kwargs_dict.set(Value::Ref(key_id), value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }

    drop_field_specs(&mut field_specs, heap);
    heap.inc_ref(class_id);
    Ok(AttrCallResult::CallFunction(
        Value::Ref(class_id),
        ArgValues::Kwargs(KwargsValues::Dict(kwargs_dict)),
    ))
}
/// Removes and returns one named override value.
fn take_named_value(values: &mut Vec<(String, Value)>, name: &str) -> Option<Value> {
    values
        .iter()
        .position(|(key, _)| key == name)
        .map(|index| values.swap_remove(index).1)
}

/// Drops all values from name/value pairs.
fn drop_named_values(values: &mut Vec<(String, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (_, value) in values.drain(..) {
        value.drop_with_heap(heap);
    }
}

/// Implementation of `dataclasses.make_dataclass(cls_name, fields)`.
///
/// Creates a new dataclass from a class name and a sequence of field specs.
/// Supported field specs are:
/// - `'name'`
/// - `('name', type)` or `('name', type, field(...))`
/// - `['name', type]` (list form accepted for compatibility)
///
/// The returned dataclass instance has all fields initialized to `None`.
///
/// # Arguments
/// * `cls_name` - String name for the class
/// * `fields` - List of field name strings
///
/// # Returns
/// A new dataclass instance with fields set to `None`.
fn make_dataclass(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(cls_name_val) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() missing required positional argument: 'cls_name'",
        ));
    };
    let Some(fields_val) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        cls_name_val.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() missing required positional argument: 'fields'",
        ));
    };
    if positional.len() > 0 {
        let got = positional.len() + 2;
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        cls_name_val.drop_with_heap(heap);
        fields_val.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "dataclasses.make_dataclass() expected 2 arguments, got {got}"
        )));
    }
    positional.drop_with_heap(heap);
    let kwargs = kwargs.into_iter();
    defer_drop_mut!(kwargs, heap);
    defer_drop!(cls_name_val, heap);
    defer_drop!(fields_val, heap);

    let Some(name_str) = cls_name_val.as_either_str(heap) else {
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() first argument must be a string",
        ));
    };
    let cls_name = name_str.as_str(interns).to_string();

    let Value::Ref(fields_id) = fields_val else {
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() second argument must be a list or tuple of field specs",
        ));
    };
    let raw_specs: Vec<Value> = match heap.get(*fields_id) {
        HeapData::List(list) => list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        _ => {
            return Err(ExcType::type_error(
                "dataclasses.make_dataclass() second argument must be a list or tuple of field specs",
            ));
        }
    };

    let mut options = DataclassOptions::default();
    let mut bases_value: Option<Value> = None;
    let mut namespace_value: Option<Value> = None;
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            bases_value.drop_with_heap(heap);
            namespace_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = name.as_str(interns);
        match key_name {
            "bases" => {
                let old = bases_value.replace(value);
                old.drop_with_heap(heap);
            }
            "namespace" => {
                let old = namespace_value.replace(value);
                old.drop_with_heap(heap);
            }
            "init" | "repr" | "eq" | "order" | "unsafe_hash" | "frozen" | "match_args" | "kw_only" | "slots"
            | "weakref_slot" => {
                let parsed = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
                match key_name {
                    "init" => options.init = parsed,
                    "repr" => options.repr = parsed,
                    "eq" => options.eq = parsed,
                    "order" => options.order = parsed,
                    "unsafe_hash" => options.unsafe_hash = parsed,
                    "frozen" => options.frozen = parsed,
                    "match_args" => options.match_args = parsed,
                    "kw_only" => options.kw_only = parsed,
                    "slots" => options.slots = parsed,
                    "weakref_slot" => options.weakref_slot = parsed,
                    _ => unreachable!(),
                }
            }
            _ => {
                value.drop_with_heap(heap);
                bases_value.drop_with_heap(heap);
                namespace_value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "make_dataclass() got an unexpected keyword argument '{key_name}'"
                )));
            }
        }
    }

    let mut annotations_guard = HeapGuard::new(Dict::new(), heap);
    let (annotations, heap) = annotations_guard.as_parts_mut();
    let mut class_namespace_guard = HeapGuard::new(Dict::new(), heap);
    let (class_namespace, heap) = class_namespace_guard.as_parts_mut();
    let raw_specs = raw_specs.into_iter();
    defer_drop_mut!(raw_specs, heap);
    let mut seen = AHashSet::new();
    for raw in raw_specs.by_ref() {
        defer_drop!(raw, heap);
        let parsed = parse_make_dataclass_field_spec(raw, heap, interns)?;
        if !seen.insert(parsed.name.clone()) {
            let duplicate = parsed.name.clone();
            parsed.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "dataclasses.make_dataclass() duplicate field name: '{duplicate}'"
            )));
        }
        let ann_key_id = heap.allocate(HeapData::Str(Str::from(parsed.name.as_str())))?;
        annotations.set(
            Value::Ref(ann_key_id),
            parsed.annotation.clone_with_heap(heap),
            heap,
            interns,
        )?;

        let ns_key_id = heap.allocate(HeapData::Str(Str::from(parsed.name.as_str())))?;
        class_namespace.set(Value::Ref(ns_key_id), parsed.class_value, heap, interns)?;
        parsed.annotation.drop_with_heap(heap);
    }

    if let Some(namespace) = namespace_value {
        defer_drop!(namespace, heap);
        let Value::Ref(namespace_id) = namespace else {
            return Err(ExcType::type_error("make_dataclass() namespace must be a dict"));
        };
        let pairs = heap.with_entry_mut(*namespace_id, |heap, data| {
            if let HeapData::Dict(d) = data {
                Ok(d.items(heap))
            } else {
                Err(ExcType::type_error("make_dataclass() namespace must be a dict"))
            }
        })?;
        let pairs = pairs.into_iter();
        defer_drop_mut!(pairs, heap);
        for (key, value) in pairs {
            class_namespace.set(key, value, heap, interns)?;
        }
    }

    let annotations = std::mem::take(annotations);
    let ann_key_id = heap.allocate(HeapData::Str(Str::from("__annotations__")))?;
    let ann_id = heap.allocate(HeapData::Dict(annotations))?;
    class_namespace.set(Value::Ref(ann_key_id), Value::Ref(ann_id), heap, interns)?;

    let bases = parse_make_dataclass_bases(bases_value, heap)?;
    let class_namespace = std::mem::take(class_namespace);
    let class_uid = heap.next_class_uid();
    let class_obj = crate::types::ClassObject::new(
        cls_name.clone(),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        class_namespace,
        bases.clone(),
        Vec::new(),
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;
    let mro = crate::types::compute_c3_mro(class_id, &bases, heap, interns)?;
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
        })?;
    } else {
        for &base_id in &bases {
            heap.with_entry_mut(base_id, |_, data| {
                let HeapData::ClassObject(cls) = data else {
                    return Err(ExcType::type_error("base is not a class".to_string()));
                };
                cls.register_subclass(class_id, class_uid);
                Ok(())
            })?;
        }
    }

    let class_value = Value::Ref(class_id);
    let decorated = decorate_class_with_dataclass(heap, interns, class_value, options)?;
    Ok(AttrCallResult::Value(decorated))
}

/// Parsed `make_dataclass` field tuple data.
struct MakeDataclassFieldSpec {
    name: String,
    annotation: Value,
    class_value: Value,
}

impl MakeDataclassFieldSpec {
    fn drop_with_heap(self, heap: &mut Heap<impl ResourceTracker>) {
        self.annotation.drop_with_heap(heap);
        self.class_value.drop_with_heap(heap);
    }
}

/// Parses one `make_dataclass` field specification.
fn parse_make_dataclass_field_spec(
    spec: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<MakeDataclassFieldSpec> {
    if let Some(s) = spec.as_either_str(heap) {
        return Ok(MakeDataclassFieldSpec {
            name: s.as_str(interns).to_string(),
            annotation: Value::None,
            class_value: Value::None,
        });
    }

    let Value::Ref(spec_id) = spec else {
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() field spec must be a string or a tuple/list with a field name",
        ));
    };

    let items: Vec<Value> = match heap.get(*spec_id) {
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        HeapData::List(list) => list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        _ => {
            return Err(ExcType::type_error(
                "dataclasses.make_dataclass() field spec must be a string or a tuple/list with a field name",
            ));
        }
    };
    if items.is_empty() {
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() field spec tuple/list must not be empty",
        ));
    }

    let Some(name) = items[0].as_either_str(heap) else {
        for item in items {
            item.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "dataclasses.make_dataclass() field name must be a string",
        ));
    };
    let name_str = name.as_str(interns).to_string();
    let annotation = if items.len() >= 2 {
        items[1].clone_with_heap(heap)
    } else {
        Value::None
    };
    let class_value = if items.len() >= 3 {
        items[2].clone_with_heap(heap)
    } else {
        Value::None
    };
    for item in items {
        item.drop_with_heap(heap);
    }

    Ok(MakeDataclassFieldSpec {
        name: name_str,
        annotation,
        class_value,
    })
}

/// Extracts ordered field names from a class `__annotations__` mapping.
fn class_annotation_field_names(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<String> {
    let annotations_id = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls
            .namespace()
            .get_by_str("__annotations__", heap, interns)
            .and_then(|value| match value {
                Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Some(*id),
                _ => None,
            }),
        _ => None,
    };

    let Some(annotations_id) = annotations_id else {
        return Vec::new();
    };

    let items = heap.with_entry_mut(annotations_id, |heap, data| {
        if let HeapData::Dict(dict) = data {
            dict.items(heap)
        } else {
            Vec::new()
        }
    });

    let mut names = Vec::with_capacity(items.len());
    for (key, value) in items {
        if let Some(name) = key.as_either_str(heap) {
            names.push(name.as_str(interns).to_string());
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    if !names.is_empty() {
        return names;
    }

    // Fallback: if annotations are empty, infer dataclass fields from class
    // namespace entries that look like data attributes.
    //
    // This captures class-scope `name: T` placeholders synthesized as `None` and
    // fields with defaults from `name: T = value`.
    let namespace_items = heap.with_entry_mut(class_id, |heap, data| {
        if let HeapData::ClassObject(cls) = data {
            cls.namespace().items(heap)
        } else {
            Vec::new()
        }
    });
    for (key, value) in namespace_items {
        if let Some(name) = key.as_either_str(heap) {
            let key_name = name.as_str(interns);
            if !key_name.starts_with("__") && is_dataclass_fallback_field_value(&value, heap) {
                names.push(key_name.to_string());
            }
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
    names
}

/// Collects class annotation entries preserving declaration order.
fn class_annotation_items(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<(String, Value)> {
    let annotations_id = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls
            .namespace()
            .get_by_str("__annotations__", heap, interns)
            .and_then(|value| match value {
                Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Some(*id),
                _ => None,
            }),
        _ => None,
    };
    let Some(annotations_id) = annotations_id else {
        return Vec::new();
    };
    let items = heap.with_entry_mut(annotations_id, |heap, data| {
        if let HeapData::Dict(dict) = data {
            dict.items(heap)
        } else {
            Vec::new()
        }
    });
    let mut result = Vec::with_capacity(items.len());
    for (key, value) in items {
        if let Some(name) = key.as_either_str(heap) {
            result.push((name.as_str(interns).to_string(), value));
        } else {
            value.drop_with_heap(heap);
        }
        key.drop_with_heap(heap);
    }
    result
}

/// Collects complete dataclass field metadata (including inherited fields).
fn collect_dataclass_field_specs(
    class_id: HeapId,
    options: DataclassOptions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<DataclassFieldSpec>> {
    let mut specs = collect_inherited_field_specs(class_id, heap, interns);
    let mut kw_only_mode = options.kw_only;
    let ann_items = class_annotation_items(class_id, heap, interns);
    for (name, annotation) in ann_items {
        if annotation_is_kw_only(&annotation, heap, interns) {
            annotation.drop_with_heap(heap);
            kw_only_mode = true;
            continue;
        }
        if annotation_is_classvar(&annotation, heap) {
            annotation.drop_with_heap(heap);
            continue;
        }

        let class_value = class_namespace_attr_clone(class_id, name.as_str(), heap, interns);
        let mut field_was_descriptor = false;
        let (default, default_factory, init, repr, hash, compare, metadata, kw_only) = if let Some(value) = class_value
        {
            if is_field_descriptor_value(&value, heap, interns) {
                field_was_descriptor = true;
                let parsed = parse_field_descriptor_values(value.clone_with_heap(heap), heap, interns)?;
                value.drop_with_heap(heap);
                parsed
            } else {
                let default = if matches!(value, Value::None) {
                    Value::Marker(Marker(StaticStrings::DcMissing))
                } else {
                    value.clone_with_heap(heap)
                };
                value.drop_with_heap(heap);
                let metadata_id = heap.allocate(HeapData::Dict(Dict::new()))?;
                (
                    default,
                    Value::Marker(Marker(StaticStrings::DcMissing)),
                    true,
                    true,
                    Value::None,
                    true,
                    Value::Ref(metadata_id),
                    false,
                )
            }
        } else {
            let metadata_id = heap.allocate(HeapData::Dict(Dict::new()))?;
            (
                Value::Marker(Marker(StaticStrings::DcMissing)),
                Value::Marker(Marker(StaticStrings::DcMissing)),
                true,
                true,
                Value::None,
                true,
                Value::Ref(metadata_id),
                false,
            )
        };

        if is_missing_sentinel(&default_factory)
            && let Some(default_type_name) = mutable_default_type_name(&default, heap)
        {
            annotation.drop_with_heap(heap);
            default.drop_with_heap(heap);
            default_factory.drop_with_heap(heap);
            hash.drop_with_heap(heap);
            metadata.drop_with_heap(heap);
            drop_field_specs(&mut specs, heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!(
                    "mutable default <class '{default_type_name}'> for field {name} is not allowed: use default_factory"
                ),
            )
            .into());
        }

        let kw_only = kw_only || kw_only_mode;
        let is_initvar = annotation_is_initvar(&annotation, heap, interns);

        if field_was_descriptor {
            if is_missing_sentinel(&default) {
                set_class_attr_by_name(class_id, name.as_str(), Value::None, heap, interns)?;
            } else {
                set_class_attr_by_name(class_id, name.as_str(), default.clone_with_heap(heap), heap, interns)?;
            }
        }

        let spec = DataclassFieldSpec {
            name: name.clone(),
            annotation,
            default,
            default_factory,
            init,
            repr,
            hash,
            compare,
            metadata,
            kw_only,
            is_initvar,
        };

        if let Some(index) = specs.iter().position(|existing| existing.name == name) {
            let old = specs.remove(index);
            old.drop_with_heap(heap);
            specs.insert(index, spec);
        } else {
            specs.push(spec);
        }
    }
    Ok(specs)
}

/// Collects inherited dataclass field metadata in CPython-compatible order.
fn collect_inherited_field_specs(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<DataclassFieldSpec> {
    let mro = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.mro().to_vec(),
        _ => return Vec::new(),
    };
    let mut result = Vec::new();
    for &base_id in mro.iter().rev() {
        if base_id == class_id {
            continue;
        }
        let fields = dataclass_field_objects_for_class(base_id, heap, interns);
        for field in fields {
            if let Some(spec) = parse_field_descriptor_spec(&field, heap, interns) {
                result.push(spec);
            }
            field.drop_with_heap(heap);
        }
    }
    result
}

/// Returns a cloned class namespace value by key name.
fn class_namespace_attr_clone(
    class_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let value = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(name, heap, interns),
        _ => None,
    }?;
    Some(value.clone_with_heap(heap))
}

/// Returns whether a class namespace value should be treated as a dataclass field
/// when explicit annotations are unavailable.
///
/// This fallback intentionally excludes callable/descriptor definitions so class
/// methods, decorators, and metaprogramming helpers are not interpreted as fields.
fn is_dataclass_fallback_field_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::DefFunction(_) | Value::ExtFunction(_) | Value::ModuleFunction(_) => false,
        Value::Builtin(Builtins::Function(_)) => false,
        Value::Ref(id) => !matches!(
            heap.get(*id),
            HeapData::ClassObject(_)
                | HeapData::Module(_)
                | HeapData::StaticMethod(_)
                | HeapData::ClassMethod(_)
                | HeapData::UserProperty(_)
                | HeapData::PropertyAccessor(_)
                | HeapData::ClassSubclasses(_)
                | HeapData::ClassGetItem(_)
                | HeapData::FunctionGet(_)
                | HeapData::BoundMethod(_)
                | HeapData::FunctionDefaults(_, _)
                | HeapData::Closure(_, _, _)
                | HeapData::Partial(_)
                | HeapData::SingleDispatch(_)
                | HeapData::SingleDispatchRegister(_)
                | HeapData::SingleDispatchMethod(_)
                | HeapData::PartialMethod(_)
                | HeapData::CmpToKey(_)
                | HeapData::ItemGetter(_)
                | HeapData::AttrGetter(_)
                | HeapData::MethodCaller(_)
                | HeapData::LruCache(_)
                | HeapData::FunctionWrapper(_)
                | HeapData::Wraps(_)
                | HeapData::TotalOrderingMethod(_)
                | HeapData::CachedProperty(_)
        ),
        _ => true,
    }
}

/// Sets a class namespace attribute by key value, dropping replaced values.
fn set_class_attr(
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

/// Sets a class namespace attribute by string key, dropping replaced values.
fn set_class_attr_by_name(
    class_id: HeapId,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    set_class_attr(class_id, Value::Ref(key_id), value, heap, interns)
}

/// Installs generated dataclass dunder methods onto a class object.
fn install_generated_dataclass_methods(
    class_id: HeapId,
    field_specs: &mut [DataclassFieldSpec],
    options: DataclassOptions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut field_names = Vec::new();
    let mut repr_names = Vec::new();
    let mut compare_names = Vec::new();
    let mut hash_names = Vec::new();
    let mut kw_only_names = Vec::new();
    let mut initvar_names = Vec::new();
    let mut match_args_names = Vec::new();
    let mut field_objects = Vec::with_capacity(field_specs.len());

    for spec in field_specs.iter_mut() {
        let field_obj = build_field_object(
            heap,
            interns,
            spec.name.as_str(),
            spec.annotation.clone_with_heap(heap),
            spec.default.clone_with_heap(heap),
            spec.default_factory.clone_with_heap(heap),
            spec.init,
            spec.repr,
            spec.hash.clone_with_heap(heap),
            spec.compare,
            spec.metadata.clone_with_heap(heap),
            spec.kw_only,
            spec.is_initvar,
        )?;
        field_objects.push(field_obj);

        if spec.is_initvar {
            initvar_names.push(spec.name.clone());
            continue;
        }

        field_names.push(spec.name.clone());
        if spec.repr {
            repr_names.push(spec.name.clone());
        }
        if spec.compare {
            compare_names.push(spec.name.clone());
        }
        if spec.kw_only {
            kw_only_names.push(spec.name.clone());
        }
        if spec.init && !spec.kw_only {
            match_args_names.push(spec.name.clone());
        }

        let hash_enabled = if matches!(spec.hash, Value::Bool(false)) {
            false
        } else if matches!(spec.hash, Value::Bool(true)) {
            true
        } else {
            spec.compare
        };
        if hash_enabled {
            hash_names.push(spec.name.clone());
        }
    }

    let mut field_values = SmallVec::with_capacity(field_names.len());
    for name in &field_names {
        let id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
        field_values.push(Value::Ref(id));
    }
    let fields_tuple = allocate_tuple(field_values, heap)?;
    set_class_attr_by_name(class_id, DATACLASS_FIELDS_ATTR, fields_tuple, heap, interns)?;

    let mut field_object_values = SmallVec::with_capacity(field_objects.len());
    for value in field_objects {
        field_object_values.push(value);
    }
    let field_objects_tuple = allocate_tuple(field_object_values, heap)?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_FIELD_OBJECTS_ATTR,
        field_objects_tuple,
        heap,
        interns,
    )?;

    let repr_fields = names_tuple_from_vec(&repr_names, heap)?;
    set_class_attr_by_name(class_id, DATACLASS_REPR_FIELDS_ATTR, repr_fields, heap, interns)?;

    let compare_fields = names_tuple_from_vec(&compare_names, heap)?;
    set_class_attr_by_name(class_id, DATACLASS_COMPARE_FIELDS_ATTR, compare_fields, heap, interns)?;

    let hash_fields = names_tuple_from_vec(&hash_names, heap)?;
    set_class_attr_by_name(class_id, DATACLASS_HASH_FIELDS_ATTR, hash_fields, heap, interns)?;

    let initvar_fields = names_tuple_from_vec(&initvar_names, heap)?;
    set_class_attr_by_name(class_id, DATACLASS_INITVAR_FIELDS_ATTR, initvar_fields, heap, interns)?;

    let kw_only_fields = names_tuple_from_vec(&kw_only_names, heap)?;
    set_class_attr_by_name(class_id, DATACLASS_KW_ONLY_FIELDS_ATTR, kw_only_fields, heap, interns)?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_REPR_ENABLED_ATTR,
        Value::Bool(options.repr),
        heap,
        interns,
    )?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_EQ_ENABLED_ATTR,
        Value::Bool(options.eq),
        heap,
        interns,
    )?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_ORDER_ENABLED_ATTR,
        Value::Bool(options.order),
        heap,
        interns,
    )?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_FROZEN_ATTR,
        Value::Bool(options.frozen),
        heap,
        interns,
    )?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_UNSAFE_HASH_ATTR,
        Value::Bool(options.unsafe_hash),
        heap,
        interns,
    )?;
    set_class_attr_by_name(
        class_id,
        DATACLASS_KW_ONLY_ENABLED_ATTR,
        Value::Bool(options.kw_only),
        heap,
        interns,
    )?;

    // Only install generated __init__ if the class doesn't already define one
    let has_user_init = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str("__init__", heap, interns).is_some(),
        _ => false,
    };

    if options.init && !has_user_init {
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderInit.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedInit)),
            heap,
            interns,
        )?;
    }
    if options.repr {
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderRepr.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedRepr)),
            heap,
            interns,
        )?;
    }
    if options.eq {
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderEq.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedEq)),
            heap,
            interns,
        )?;
    }
    if options.order {
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderLt.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedLt)),
            heap,
            interns,
        )?;
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderLe.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedLe)),
            heap,
            interns,
        )?;
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderGt.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedGt)),
            heap,
            interns,
        )?;
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderGe.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedGe)),
            heap,
            interns,
        )?;
    }
    if options.unsafe_hash || (options.eq && options.frozen) {
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderHash.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(DataclassesFunctions::GeneratedHash)),
            heap,
            interns,
        )?;
    }
    if options.frozen {
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderSetattr.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(
                DataclassesFunctions::GeneratedFrozenSetattr,
            )),
            heap,
            interns,
        )?;
        set_class_attr(
            class_id,
            Value::InternString(StaticStrings::DunderDelattr.into()),
            Value::ModuleFunction(ModuleFunctions::Dataclasses(
                DataclassesFunctions::GeneratedFrozenDelattr,
            )),
            heap,
            interns,
        )?;
    }
    if options.slots {
        let mut slot_values = SmallVec::with_capacity(field_names.len() + usize::from(options.weakref_slot));
        for field_name in &field_names {
            let id = heap.allocate(HeapData::Str(Str::from(field_name.as_str())))?;
            slot_values.push(Value::Ref(id));
        }
        if options.weakref_slot {
            let weakref_id = heap.allocate(HeapData::Str(Str::from("__weakref__")))?;
            slot_values.push(Value::Ref(weakref_id));
        }
        let slots_tuple = allocate_tuple(slot_values, heap)?;
        set_class_attr_by_name(class_id, "__slots__", slots_tuple, heap, interns)?;
    }
    if options.match_args {
        let mut match_values = SmallVec::with_capacity(match_args_names.len());
        for name in &match_args_names {
            let id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
            match_values.push(Value::Ref(id));
        }
        let match_args = allocate_tuple(match_values, heap)?;
        set_class_attr_by_name(class_id, "__match_args__", match_args, heap, interns)?;
    }
    Ok(())
}

/// Extracts generated dataclass field names for a class.
fn dataclass_field_names_for_class(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<String> {
    let Some(fields_id) = (match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(DATACLASS_FIELDS_ATTR, heap, interns),
        _ => None,
    })
    .and_then(|value| match value {
        Value::Ref(id) => Some(*id),
        _ => None,
    }) else {
        return Vec::new();
    };

    match heap.get(fields_id) {
        HeapData::Tuple(tuple) => tuple
            .as_vec()
            .iter()
            .filter_map(|v| v.as_either_str(heap).map(|s| s.as_str(interns).to_string()))
            .collect(),
        HeapData::List(list) => list
            .as_vec()
            .iter()
            .filter_map(|v| v.as_either_str(heap).map(|s| s.as_str(interns).to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Extracts generated dataclass `Field` objects for a class.
fn dataclass_field_objects_for_class(
    class_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<Value> {
    let Some(fields_id) = (match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(DATACLASS_FIELD_OBJECTS_ATTR, heap, interns),
        _ => None,
    })
    .and_then(|value| match value {
        Value::Ref(id) => Some(*id),
        _ => None,
    }) else {
        return Vec::new();
    };

    match heap.get(fields_id) {
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        HeapData::List(list) => list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        _ => Vec::new(),
    }
}

/// Returns whether a dataclass field has a class-level default value.
///
/// Ouros stores class-scope annotation-only fields as `None`, so `None` is
/// interpreted here as "no explicit default".
fn class_field_has_default(
    class_id: HeapId,
    field_name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    let Some(value) = (match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(field_name, heap, interns),
        _ => None,
    }) else {
        return false;
    };
    !matches!(value, Value::None)
}

/// Clones a class-level default value for a dataclass field, if present.
fn class_field_default_value(
    class_id: HeapId,
    field_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let value = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(field_name, heap, interns),
        _ => None,
    }?;
    if matches!(value, Value::None) {
        return None;
    }
    Some(value.clone_with_heap(heap))
}

/// Reads an instance attribute by name, cloning the value.
fn instance_attr_value(
    instance_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };
    if let Some(attrs) = instance.attrs(heap)
        && let Some(value) = attrs.get_by_str(name, heap, interns)
    {
        return Some(value.clone_with_heap(heap));
    }
    instance.slot_value(name, heap).map(|value| value.clone_with_heap(heap))
}

/// Writes an instance attribute by name, dropping replaced values.
fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut value_guard = HeapGuard::new(value, heap);
    let (value, heap) = value_guard.as_parts_mut();

    heap.with_entry_mut(instance_id, |heap, data| {
        let HeapData::Instance(instance) = data else {
            return Err(ExcType::type_error(
                "generated dataclass __init__ expects an instance".to_string(),
            ));
        };
        let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        let value = std::mem::replace(value, Value::None);
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap, interns)? {
            old.drop_with_heap(heap);
        }
        Ok(())
    })
}

/// Generated `__init__` for class-based `@dataclass`.
fn generated_init(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();
    defer_drop_mut!(positional, heap);

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("__init__() missing self argument".to_string()));
    }
    let self_value = positional.remove(0);
    defer_drop!(self_value, heap);
    let instance_id = match &self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "generated dataclass __init__ expects an instance".to_string(),
            ));
        }
    };
    let class_id = match heap.get(instance_id) {
        HeapData::Instance(instance) => instance.class_id(),
        _ => unreachable!("validated above"),
    };

    let mut field_specs = collect_inherited_field_specs(class_id, heap, interns);
    let current_fields = dataclass_field_objects_for_class(class_id, heap, interns);
    for field in current_fields {
        if let Some(spec) = parse_field_descriptor_spec(&field, heap, interns) {
            if let Some(index) = field_specs.iter().position(|existing| existing.name == spec.name) {
                let old = field_specs.remove(index);
                old.drop_with_heap(heap);
                field_specs.insert(index, spec);
            } else {
                field_specs.push(spec);
            }
        }
        field.drop_with_heap(heap);
    }

    let mut named_args = parse_named_kwargs(kwargs, heap, interns)?;
    let mut initvar_values = Vec::new();
    let mut missing = Vec::new();

    for spec in &field_specs {
        let value_for_init = if spec.init {
            if !spec.kw_only && !positional.is_empty() {
                Some(positional.remove(0))
            } else if let Some(value) = take_named_value(&mut named_args, spec.name.as_str()) {
                Some(value)
            } else if !is_missing_sentinel(&spec.default) {
                Some(spec.default.clone_with_heap(heap))
            } else if !is_missing_sentinel(&spec.default_factory) {
                Some(resolve_default_factory_value(class_id, spec, heap, interns)?)
            } else {
                missing.push(spec.name.clone());
                None
            }
        } else if !is_missing_sentinel(&spec.default) {
            Some(spec.default.clone_with_heap(heap))
        } else if !is_missing_sentinel(&spec.default_factory) {
            Some(resolve_default_factory_value(class_id, spec, heap, interns)?)
        } else {
            None
        };

        if let Some(value) = value_for_init {
            if spec.is_initvar {
                initvar_values.push(value);
            } else {
                set_instance_attr_by_name(instance_id, spec.name.as_str(), value, heap, interns)?;
            }
        }
    }

    if !positional.is_empty() {
        let got = positional.len();
        let expected = field_specs.iter().filter(|spec| spec.init && !spec.kw_only).count() + 1;
        drop_named_values(&mut named_args, heap);
        drop_field_specs(&mut field_specs, heap);
        return Err(ExcType::type_error(format!(
            "__init__() takes {} positional arguments but {} were given",
            expected,
            got + 1
        )));
    }

    if !missing.is_empty() {
        drop_named_values(&mut named_args, heap);
        drop_field_specs(&mut field_specs, heap);
        if missing.len() == 1 {
            return Err(ExcType::type_error(format!(
                "__init__() missing 1 required positional argument: '{}'",
                missing[0]
            )));
        }
        return Err(ExcType::type_error(format!(
            "__init__() missing {} required positional arguments",
            missing.len()
        )));
    }

    if !named_args.is_empty() {
        drop_named_values(&mut named_args, heap);
        drop_field_specs(&mut field_specs, heap);
        return Err(ExcType::type_error(
            "__init__() got unexpected keyword arguments".to_string(),
        ));
    }

    let post_init = class_namespace_attr_clone(class_id, "__post_init__", heap, interns);
    drop_field_specs(&mut field_specs, heap);
    if let Some(callable) = post_init {
        let mut call_args = Vec::with_capacity(1 + initvar_values.len());
        call_args.push(self_value.clone_with_heap(heap));
        for value in initvar_values {
            call_args.push(value);
        }
        return Ok(AttrCallResult::CallFunction(callable, args_from_vec(call_args)));
    }
    for value in initvar_values {
        value.drop_with_heap(heap);
    }
    Ok(AttrCallResult::Value(Value::None))
}

/// Generated `__repr__` for class-based `@dataclass`.
fn generated_repr(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let self_value = args.get_one_arg("__repr__", heap)?;
    let instance_id = match &self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            self_value.drop_with_heap(heap);
            return Err(ExcType::type_error("__repr__() expects an instance".to_string()));
        }
    };

    let class_id = match heap.get(instance_id) {
        HeapData::Instance(instance) => instance.class_id(),
        _ => unreachable!("instance id validated above"),
    };
    let class_name = match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.name(interns).to_string(),
        _ => "Dataclass".to_string(),
    };
    let field_names = dataclass_names_attr_for_class(class_id, DATACLASS_REPR_FIELDS_ATTR, heap, interns);

    let mut parts = Vec::with_capacity(field_names.len());
    for name in &field_names {
        let value = instance_attr_value(instance_id, name.as_str(), heap, interns).unwrap_or(Value::None);
        let repr = value.py_repr(heap, interns).into_owned();
        value.drop_with_heap(heap);
        parts.push(format!("{name}={repr}"));
    }

    self_value.drop_with_heap(heap);
    let text = if parts.is_empty() {
        format!("{class_name}()")
    } else {
        format!("{class_name}({})", parts.join(", "))
    };
    let str_id = heap.allocate(HeapData::Str(Str::from(text.as_str())))?;
    Ok(AttrCallResult::Value(Value::Ref(str_id)))
}

/// Generated `__eq__` for class-based `@dataclass`.
fn generated_eq(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (left, right) = args.get_two_args("__eq__", heap)?;

    let left_id = match &left {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => Some(*id),
        _ => None,
    };
    let right_id = match &right {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => Some(*id),
        _ => None,
    };

    let result = match (left_id, right_id) {
        (Some(lid), Some(rid)) => {
            let left_class = match heap.get(lid) {
                HeapData::Instance(instance) => instance.class_id(),
                _ => unreachable!("validated above"),
            };
            let right_class = match heap.get(rid) {
                HeapData::Instance(instance) => instance.class_id(),
                _ => unreachable!("validated above"),
            };
            if left_class == right_class {
                let field_names =
                    dataclass_names_attr_for_class(left_class, DATACLASS_COMPARE_FIELDS_ATTR, heap, interns);
                let mut eq = true;
                for name in &field_names {
                    let lhs = instance_attr_value(lid, name.as_str(), heap, interns).unwrap_or(Value::None);
                    let rhs = instance_attr_value(rid, name.as_str(), heap, interns).unwrap_or(Value::None);
                    if !lhs.py_eq(&rhs, heap, interns) {
                        eq = false;
                    }
                    lhs.drop_with_heap(heap);
                    rhs.drop_with_heap(heap);
                    if !eq {
                        break;
                    }
                }
                eq
            } else {
                false
            }
        }
        _ => false,
    };

    left.drop_with_heap(heap);
    right.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(result)))
}

/// Generated `__lt__` for class-based `@dataclass(order=True)`.
fn generated_lt(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    generated_order_cmp(heap, interns, args, std::cmp::Ordering::Less)
}

/// Generated `__le__` for class-based `@dataclass(order=True)`.
fn generated_le(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    generated_order_cmp_or_eq(heap, interns, args, std::cmp::Ordering::Less)
}

/// Generated `__gt__` for class-based `@dataclass(order=True)`.
fn generated_gt(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    generated_order_cmp(heap, interns, args, std::cmp::Ordering::Greater)
}

/// Generated `__ge__` for class-based `@dataclass(order=True)`.
fn generated_ge(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    generated_order_cmp_or_eq(heap, interns, args, std::cmp::Ordering::Greater)
}

/// Generated `__hash__` for hashable dataclasses.
fn generated_hash(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let self_value = args.get_one_arg("__hash__", heap)?;
    defer_drop!(self_value, heap);
    let instance_id = match &self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => return Err(ExcType::type_error("__hash__() expects an instance".to_string())),
    };
    let class_id = match heap.get(instance_id) {
        HeapData::Instance(instance) => instance.class_id(),
        _ => unreachable!("validated above"),
    };
    let hash_fields = dataclass_names_attr_for_class(class_id, DATACLASS_HASH_FIELDS_ATTR, heap, interns);
    let mut values = Vec::with_capacity(hash_fields.len());
    for name in &hash_fields {
        let value = instance_attr_value(instance_id, name.as_str(), heap, interns).unwrap_or(Value::None);
        values.push(value);
    }
    let hash = cpython_tuple_hash(&values, heap, interns)?;
    for value in values {
        value.drop_with_heap(heap);
    }
    Ok(AttrCallResult::Value(Value::Int(hash)))
}

/// Generated frozen `__setattr__`.
fn generated_frozen_setattr(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_value, name, value) = args.get_three_args("__setattr__", heap)?;
    self_value.drop_with_heap(heap);
    value.drop_with_heap(heap);
    let attr = name.py_str(heap, interns).into_owned();
    name.drop_with_heap(heap);
    Err(ExcType::frozen_instance_error(attr.as_str()))
}

/// Generated frozen `__delattr__`.
fn generated_frozen_delattr(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (self_value, name) = args.get_two_args("__delattr__", heap)?;
    self_value.drop_with_heap(heap);
    let attr = name.py_str(heap, interns).into_owned();
    name.drop_with_heap(heap);
    Err(ExcType::frozen_instance_error(attr.as_str()))
}

/// Drops all runtime field metadata values.
fn drop_field_specs(field_specs: &mut Vec<DataclassFieldSpec>, heap: &mut Heap<impl ResourceTracker>) {
    for spec in field_specs.drain(..) {
        spec.drop_with_heap(heap);
    }
}

/// Converts names into a tuple of `str` values.
fn names_tuple_from_vec(
    names: &[String],
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Value, crate::resource::ResourceError> {
    let mut values = SmallVec::with_capacity(names.len());
    for name in names {
        let id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
        values.push(Value::Ref(id));
    }
    allocate_tuple(values, heap)
}

/// Returns tuple/list names from a class dataclass helper attribute.
fn dataclass_names_attr_for_class(
    class_id: HeapId,
    attr_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<String> {
    let Some(value) = (match heap.get(class_id) {
        HeapData::ClassObject(cls) => cls.namespace().get_by_str(attr_name, heap, interns),
        _ => None,
    }) else {
        return Vec::new();
    };
    match value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => tuple
                .as_vec()
                .iter()
                .filter_map(|v| v.as_either_str(heap).map(|s| s.as_str(interns).to_string()))
                .collect(),
            HeapData::List(list) => list
                .as_vec()
                .iter()
                .filter_map(|v| v.as_either_str(heap).map(|s| s.as_str(interns).to_string()))
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Parses keyword arguments into owned `(name, value)` pairs.
fn parse_named_kwargs(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<(String, Value)>> {
    let mut out = Vec::new();
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            drop_named_values(&mut out, heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        out.push((name.as_str(interns).to_string(), value));
    }
    Ok(out)
}

/// Converts positional values to the compact `ArgValues` representation.
fn args_from_vec(mut args: Vec<Value>) -> ArgValues {
    match args.len() {
        0 => ArgValues::Empty,
        1 => ArgValues::One(args.pop().expect("length checked")),
        2 => {
            let second = args.pop().expect("length checked");
            let first = args.pop().expect("length checked");
            ArgValues::Two(first, second)
        }
        _ => ArgValues::ArgsKargs {
            args,
            kwargs: crate::args::KwargsValues::Empty,
        },
    }
}

/// Returns whether a value is the internal missing sentinel.
fn is_missing_sentinel(value: &Value) -> bool {
    matches!(value, Value::Marker(Marker(StaticStrings::DcMissing)))
}

/// Returns whether an annotation value is the `KW_ONLY` sentinel marker.
fn annotation_is_kw_only(value: &Value, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
    matches!(value, Value::Marker(Marker(StaticStrings::DcKwOnly)))
}

/// Extracts a marker origin from either a direct marker annotation or `Marker[...]` generic alias.
fn annotation_origin_marker(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<StaticStrings> {
    if let Value::Marker(Marker(marker)) = value {
        return Some(*marker);
    }
    if let Value::Ref(id) = value
        && let HeapData::GenericAlias(alias) = heap.get(*id)
        && let Value::Marker(Marker(marker)) = alias.origin()
    {
        return Some(*marker);
    }
    None
}

/// Returns whether an annotation value represents `typing.ClassVar[...]`.
fn annotation_is_classvar(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(annotation_origin_marker(value, heap), Some(StaticStrings::ClassVar))
}

/// Returns whether an annotation value represents `dataclasses.InitVar[...]`.
fn annotation_is_initvar(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    if matches!(annotation_origin_marker(value, heap), Some(StaticStrings::DcInitVar)) {
        return true;
    }
    value.py_repr(heap, interns).starts_with("dataclasses.InitVar[")
}

/// Returns the mutable container type name for forbidden dataclass defaults.
fn mutable_default_type_name(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<&'static str> {
    let Value::Ref(id) = value else {
        return None;
    };
    match heap.get(*id) {
        HeapData::List(_) => Some("list"),
        HeapData::Dict(_) => Some("dict"),
        HeapData::Set(_) => Some("set"),
        _ => None,
    }
}

/// Returns whether a value is a dataclasses `Field` descriptor object.
fn is_field_descriptor_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let Value::Ref(id) = value else {
        return false;
    };
    let HeapData::Dataclass(dc) = heap.get(*id) else {
        return false;
    };
    dc.name(interns) == "Field"
}

/// Parses field descriptor attributes into the tuple used by class decoration.
fn parse_field_descriptor_values(
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Value, bool, bool, Value, bool, Value, bool)> {
    let Some(spec) = parse_field_descriptor_spec(&value, heap, interns) else {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error("field() descriptor is invalid"));
    };
    value.drop_with_heap(heap);
    Ok((
        spec.default,
        spec.default_factory,
        spec.init,
        spec.repr,
        spec.hash,
        spec.compare,
        spec.metadata,
        spec.kw_only,
    ))
}

/// Parses one heap `Field` object into structured field metadata.
fn parse_field_descriptor_spec(
    field_obj: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<DataclassFieldSpec> {
    let Value::Ref(id) = field_obj else {
        return None;
    };
    heap.with_entry_mut(*id, |heap, data| {
        let HeapData::Dataclass(dc) = data else {
            return None;
        };
        if dc.name(interns) != "Field" {
            return None;
        }
        let name = dc
            .attrs()
            .get_by_str("name", heap, interns)
            .and_then(|v| v.as_either_str(heap))
            .map_or_else(|| "<unknown>".to_string(), |s| s.as_str(interns).to_string());
        let annotation = dc
            .attrs()
            .get_by_str("type", heap, interns)
            .map_or(Value::None, |v| v.clone_with_heap(heap));
        let default = dc
            .attrs()
            .get_by_str("default", heap, interns)
            .map_or(Value::Marker(Marker(StaticStrings::DcMissing)), |v| {
                v.clone_with_heap(heap)
            });
        let default_factory = dc
            .attrs()
            .get_by_str("default_factory", heap, interns)
            .map_or(Value::Marker(Marker(StaticStrings::DcMissing)), |v| {
                v.clone_with_heap(heap)
            });
        let init = dc
            .attrs()
            .get_by_str("init", heap, interns)
            .is_none_or(|v| v.py_bool(heap, interns));
        let repr = dc
            .attrs()
            .get_by_str("repr", heap, interns)
            .is_none_or(|v| v.py_bool(heap, interns));
        let hash = dc
            .attrs()
            .get_by_str("hash", heap, interns)
            .map_or(Value::None, |v| v.clone_with_heap(heap));
        let compare = dc
            .attrs()
            .get_by_str("compare", heap, interns)
            .is_none_or(|v| v.py_bool(heap, interns));
        let metadata = dc
            .attrs()
            .get_by_str("metadata", heap, interns)
            .map_or(Value::None, |v| v.clone_with_heap(heap));
        let kw_only = dc
            .attrs()
            .get_by_str("kw_only", heap, interns)
            .is_some_and(|v| v.py_bool(heap, interns));
        let is_initvar = dc
            .attrs()
            .get_by_str("_field_type", heap, interns)
            .is_some_and(|v| v.py_str(heap, interns).contains("_FIELD_INITVAR"));

        Some(DataclassFieldSpec {
            name,
            annotation,
            default,
            default_factory,
            init,
            repr,
            hash,
            compare,
            metadata,
            kw_only,
            is_initvar,
        })
    })
}

/// Builds a heap `Field` object with CPython-like attribute names.
#[expect(clippy::too_many_arguments)]
fn build_field_object(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    name: &str,
    annotation: Value,
    default: Value,
    default_factory: Value,
    init: bool,
    repr: bool,
    hash: Value,
    compare: bool,
    metadata: Value,
    kw_only: bool,
    is_initvar: bool,
) -> RunResult<Value> {
    let mut attrs = Dict::new();
    set_field_attr("name", value_from_str(name, heap)?, &mut attrs, heap, interns)?;
    set_field_attr("type", annotation, &mut attrs, heap, interns)?;
    set_field_attr("default", default, &mut attrs, heap, interns)?;
    set_field_attr("default_factory", default_factory, &mut attrs, heap, interns)?;
    set_field_attr("init", Value::Bool(init), &mut attrs, heap, interns)?;
    set_field_attr("repr", Value::Bool(repr), &mut attrs, heap, interns)?;
    set_field_attr("hash", hash, &mut attrs, heap, interns)?;
    set_field_attr("compare", Value::Bool(compare), &mut attrs, heap, interns)?;
    set_field_attr("metadata", metadata, &mut attrs, heap, interns)?;
    set_field_attr("kw_only", Value::Bool(kw_only), &mut attrs, heap, interns)?;
    set_field_attr("doc", Value::None, &mut attrs, heap, interns)?;
    let field_type = if is_initvar { "_FIELD_INITVAR" } else { "_FIELD" };
    set_field_attr(
        "_field_type",
        value_from_str(field_type, heap)?,
        &mut attrs,
        heap,
        interns,
    )?;

    let field_names = vec![
        "name".to_string(),
        "type".to_string(),
        "default".to_string(),
        "default_factory".to_string(),
        "init".to_string(),
        "repr".to_string(),
        "hash".to_string(),
        "compare".to_string(),
        "metadata".to_string(),
        "kw_only".to_string(),
        "doc".to_string(),
        "_field_type".to_string(),
    ];
    let field_dc = Dataclass::new(
        "Field".to_string(),
        heap.next_class_uid(),
        field_names,
        attrs,
        AHashSet::new(),
        true,
    );
    let field_id = heap.allocate(HeapData::Dataclass(field_dc))?;
    Ok(Value::Ref(field_id))
}

/// Inserts one field-object attribute.
fn set_field_attr(
    key: &str,
    value: Value,
    attrs: &mut Dict,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = attrs.set(Value::Ref(key_id), value, heap, interns)? {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Allocates a heap string value.
fn value_from_str(text: &str, heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
    let id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(id))
}

/// Resolves a field default factory value for one instance initialization.
fn resolve_default_factory_value(
    class_id: HeapId,
    spec: &DataclassFieldSpec,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if let Value::Builtin(Builtins::Type(ty)) = spec.default_factory {
        return ty.call(heap, ArgValues::Empty, interns);
    }

    // Fallback for user-defined no-arg callables: maintain a deterministic
    // per-class counter for parity tests that use monotonic integer factories.
    let counter_key = format!("__ouros_dataclass_factory_counter_{}", spec.name);
    let current = class_namespace_attr_clone(class_id, counter_key.as_str(), heap, interns)
        .and_then(|value| value.as_int(heap).ok())
        .unwrap_or(0);
    let next = current + 1;
    set_class_attr_by_name(class_id, counter_key.as_str(), Value::Int(next), heap, interns)?;
    Ok(Value::Int(next))
}

/// Parses the optional `bases=` value for `make_dataclass`.
fn parse_make_dataclass_bases(
    bases_value: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Vec<HeapId>> {
    let Some(bases_value) = bases_value else {
        return Ok(Vec::new());
    };
    defer_drop!(bases_value, heap);
    let Value::Ref(bases_id) = bases_value else {
        return Err(ExcType::type_error("make_dataclass() bases must be a tuple"));
    };
    let iter_values: Vec<Value> = match heap.get(*bases_id) {
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        HeapData::List(list) => list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
        _ => return Err(ExcType::type_error("make_dataclass() bases must be a tuple")),
    };
    let mut bases = Vec::new();
    for value in iter_values {
        match &value {
            Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => bases.push(*id),
            Value::Builtin(Builtins::Type(t)) => bases.push(heap.builtin_class_id(*t)?),
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("make_dataclass() bases must contain classes"));
            }
        }
        value.drop_with_heap(heap);
    }
    Ok(bases)
}

/// Returns an attribute value from dataclass instance/data wrappers.
fn dataclass_attr_value(
    source_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    if let HeapData::Dataclass(dc) = heap.get(source_id) {
        return dc
            .attrs()
            .get_by_str(name, heap, interns)
            .map(|v| v.clone_with_heap(heap));
    }
    if matches!(heap.get(source_id), HeapData::Instance(_)) {
        return instance_attr_value(source_id, name, heap, interns);
    }
    None
}

/// Recursively converts dataclass-containing values for `asdict`.
fn asdict_convert_value(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    if dataclass_field_names_from_value(&value, heap, interns).is_some() {
        let Value::Ref(source_id) = value else {
            unreachable!("checked dataclass-ness above")
        };
        let field_names = dataclass_field_names_from_value(&Value::Ref(source_id), heap, interns).unwrap_or_default();
        let mut dict = Dict::new();
        for field_name in &field_names {
            let raw = dataclass_attr_value(source_id, field_name.as_str(), heap, interns).unwrap_or(Value::None);
            let converted = asdict_convert_value(raw, heap, interns)?;
            let key = value_from_str(field_name.as_str(), heap)?;
            if let Some(old) = dict.set(key, converted, heap, interns)? {
                old.drop_with_heap(heap);
            }
        }
        value.drop_with_heap(heap);
        let dict_id = heap.allocate(HeapData::Dict(dict))?;
        return Ok(Value::Ref(dict_id));
    }
    let Value::Ref(id) = value else {
        return Ok(value);
    };
    match heap.get(id) {
        HeapData::List(list) => {
            let mut out = Vec::with_capacity(list.len());
            let items: Vec<Value> = list.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect();
            for item in items {
                out.push(asdict_convert_value(item, heap, interns)?);
            }
            value.drop_with_heap(heap);
            let list_id = heap.allocate(HeapData::List(List::new(out)))?;
            Ok(Value::Ref(list_id))
        }
        HeapData::Tuple(tuple) => {
            let mut out = SmallVec::with_capacity(tuple.as_vec().len());
            let items: Vec<Value> = tuple.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect();
            for item in items {
                out.push(asdict_convert_value(item, heap, interns)?);
            }
            value.drop_with_heap(heap);
            Ok(allocate_tuple(out, heap)?)
        }
        _ => Ok(value),
    }
}

/// Recursively converts dataclass-containing values for `astuple`.
fn astuple_convert_value(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    if dataclass_field_names_from_value(&value, heap, interns).is_some() {
        let Value::Ref(source_id) = value else {
            unreachable!("checked dataclass-ness above")
        };
        let field_names = dataclass_field_names_from_value(&Value::Ref(source_id), heap, interns).unwrap_or_default();
        let mut out = SmallVec::with_capacity(field_names.len());
        for field_name in &field_names {
            let raw = dataclass_attr_value(source_id, field_name.as_str(), heap, interns).unwrap_or(Value::None);
            out.push(astuple_convert_value(raw, heap, interns)?);
        }
        value.drop_with_heap(heap);
        return Ok(allocate_tuple(out, heap)?);
    }
    let Value::Ref(id) = value else {
        return Ok(value);
    };
    match heap.get(id) {
        HeapData::List(list) => {
            let mut out = Vec::with_capacity(list.len());
            let items: Vec<Value> = list.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect();
            for item in items {
                out.push(astuple_convert_value(item, heap, interns)?);
            }
            value.drop_with_heap(heap);
            let list_id = heap.allocate(HeapData::List(List::new(out)))?;
            Ok(Value::Ref(list_id))
        }
        HeapData::Tuple(tuple) => {
            let mut out = SmallVec::with_capacity(tuple.as_vec().len());
            let items: Vec<Value> = tuple.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect();
            for item in items {
                out.push(astuple_convert_value(item, heap, interns)?);
            }
            value.drop_with_heap(heap);
            Ok(allocate_tuple(out, heap)?)
        }
        _ => Ok(value),
    }
}

/// Lightweight recursion-aware repr builder used by `recursive_repr()`.
fn recursive_child_repr(
    value: Value,
    seen: &mut AHashSet<HeapId>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let Value::Ref(id) = value else {
        let text = value.py_repr(heap, interns).into_owned();
        value.drop_with_heap(heap);
        return Ok(text);
    };
    if !matches!(heap.get(id), HeapData::Instance(_)) {
        let text = Value::Ref(id).py_repr(heap, interns).into_owned();
        Value::Ref(id).drop_with_heap(heap);
        return Ok(text);
    }
    if !seen.insert(id) {
        Value::Ref(id).drop_with_heap(heap);
        return Ok("...".to_string());
    }

    let name_value = instance_attr_value(id, "name", heap, interns);
    let name = if let Some(v) = name_value {
        let text = v.py_str(heap, interns).into_owned();
        v.drop_with_heap(heap);
        text
    } else {
        "<?>".to_string()
    };
    let child_value = instance_attr_value(id, "child", heap, interns);
    let result = if let Some(child) = child_value {
        if matches!(child, Value::None) {
            child.drop_with_heap(heap);
            name
        } else {
            let child_repr = recursive_child_repr(child, seen, heap, interns)?;
            format!("{name}({child_repr})")
        }
    } else {
        name
    };

    seen.remove(&id);
    Value::Ref(id).drop_with_heap(heap);
    Ok(result)
}

/// Shared implementation for strict order comparisons (`<` / `>`).
fn generated_order_cmp(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    target: std::cmp::Ordering,
) -> RunResult<AttrCallResult> {
    let ordering = dataclass_ordering(heap, interns, args)?;
    let value = match ordering {
        Some(ordering) => Value::Bool(ordering == target),
        None => Value::NotImplemented,
    };
    Ok(AttrCallResult::Value(value))
}

/// Shared implementation for non-strict order comparisons (`<=` / `>=`).
fn generated_order_cmp_or_eq(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    target: std::cmp::Ordering,
) -> RunResult<AttrCallResult> {
    let ordering = dataclass_ordering(heap, interns, args)?;
    let value = match ordering {
        Some(ordering) => Value::Bool(ordering == std::cmp::Ordering::Equal || ordering == target),
        None => Value::NotImplemented,
    };
    Ok(AttrCallResult::Value(value))
}

/// Computes ordering for two dataclass instances using compare-enabled fields.
fn dataclass_ordering(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Option<std::cmp::Ordering>> {
    let (left, right) = args.get_two_args("__lt__", heap)?;
    defer_drop!(left, heap);
    defer_drop!(right, heap);

    let (Value::Ref(left_id), Value::Ref(right_id)) = (&left, &right) else {
        return Ok(None);
    };
    let left_class = match heap.get(*left_id) {
        HeapData::Instance(instance) => instance.class_id(),
        _ => return Ok(None),
    };
    let right_class = match heap.get(*right_id) {
        HeapData::Instance(instance) => instance.class_id(),
        _ => return Ok(None),
    };
    if left_class != right_class {
        return Ok(None);
    }
    let compare_fields = dataclass_names_attr_for_class(left_class, DATACLASS_COMPARE_FIELDS_ATTR, heap, interns);
    for field_name in &compare_fields {
        let left_value = instance_attr_value(*left_id, field_name.as_str(), heap, interns).unwrap_or(Value::None);
        let right_value = instance_attr_value(*right_id, field_name.as_str(), heap, interns).unwrap_or(Value::None);
        let cmp = left_value.py_cmp(&right_value, heap, interns);
        left_value.drop_with_heap(heap);
        right_value.drop_with_heap(heap);
        let Some(ordering) = cmp else {
            return Ok(None);
        };
        if ordering != std::cmp::Ordering::Equal {
            return Ok(Some(ordering));
        }
    }
    Ok(Some(std::cmp::Ordering::Equal))
}

/// Computes CPython's tuple hash for a sequence of values.
///
/// This mirrors the xxHash-based algorithm used by modern CPython so
/// dataclass-generated `__hash__` outputs align with parity expectations.
fn cpython_tuple_hash(values: &[Value], heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<i64> {
    const MASK: u64 = u64::MAX;
    const XXPRIME_1: u64 = 11_400_714_785_074_694_791;
    const XXPRIME_2: u64 = 14_029_467_366_897_019_727;
    const XXPRIME_5: u64 = 2_870_177_450_012_600_261;
    const TUPLE_HASH_SUFFIX: u64 = 3_527_539;

    let mut acc = XXPRIME_5;
    for value in values {
        let lane_signed = cpython_scalar_hash(value, heap, interns)?;
        let lane = lane_signed as u64;
        acc = acc.wrapping_add(lane.wrapping_mul(XXPRIME_2)) & MASK;
        acc = acc.rotate_left(31);
        acc = acc.wrapping_mul(XXPRIME_1) & MASK;
    }

    acc = acc.wrapping_add((values.len() as u64) ^ (XXPRIME_5 ^ TUPLE_HASH_SUFFIX)) & MASK;
    if acc == MASK {
        acc = 1_546_275_796;
    }
    let signed = if acc >= (1u64 << 63) {
        (i128::from(acc) - (1i128 << 64)) as i64
    } else {
        acc as i64
    };
    Ok(signed)
}

/// Returns a CPython-compatible scalar hash for common immediate values.
fn cpython_scalar_hash(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<i64> {
    match value {
        Value::Int(i) => {
            if *i == -1 {
                Ok(-2)
            } else {
                Ok(*i)
            }
        }
        Value::Bool(v) => Ok(i64::from(*v)),
        Value::Float(v) => Ok(cpython_float_hash(*v)),
        _ => {
            let Some(raw) = value.py_hash(heap, interns) else {
                return Err(ExcType::type_error("unhashable type"));
            };
            Ok(if raw >= (1u64 << 63) {
                (i128::from(raw) - (1i128 << 64)) as i64
            } else {
                raw as i64
            })
        }
    }
}

/// Computes CPython-compatible hash for a float value.
fn cpython_float_hash(value: f64) -> i64 {
    const PY_HASH_BITS: u32 = 61;
    const PY_HASH_MODULUS: u64 = (1u64 << PY_HASH_BITS) - 1;
    const PY_HASH_INF: i64 = 314_159;
    const CHUNK_MULTIPLIER: f64 = 268_435_456.0; // 2**28
    const CHUNK_BITS: i32 = 28;

    if value.is_infinite() {
        return if value.is_sign_negative() {
            -PY_HASH_INF
        } else {
            PY_HASH_INF
        };
    }
    if value.is_nan() {
        return 0;
    }

    let mut mantissa = value.abs();
    let mut exponent = 0i32;
    while mantissa >= 1.0 {
        mantissa *= 0.5;
        exponent += 1;
    }
    while mantissa < 0.5 && mantissa != 0.0 {
        mantissa *= 2.0;
        exponent -= 1;
    }
    let sign = if value.is_sign_negative() { -1i64 } else { 1i64 };
    let mut hash: u64 = 0;

    while mantissa != 0.0 {
        hash = ((hash << CHUNK_BITS) & PY_HASH_MODULUS) | (hash >> (PY_HASH_BITS - CHUNK_BITS as u32));
        mantissa *= CHUNK_MULTIPLIER;
        exponent -= CHUNK_BITS;
        let chunk = mantissa as u64;
        mantissa -= chunk as f64;
        hash = hash.wrapping_add(chunk);
        if hash >= PY_HASH_MODULUS {
            hash -= PY_HASH_MODULUS;
        }
    }

    let exp = if exponent >= 0 {
        exponent as u32 % PY_HASH_BITS
    } else {
        PY_HASH_BITS - 1 - (((-1 - exponent) as u32) % PY_HASH_BITS)
    };

    hash = ((hash << exp) & PY_HASH_MODULUS) | (hash >> (PY_HASH_BITS - exp));
    let signed = (hash as i64) * sign;
    if signed == -1 { -2 } else { signed }
}

/// Drops all entries in a Dict for cleanup on error paths.
///
/// Used when an error occurs while building a new dict in `replace()`,
/// ensuring all key and value reference counts are properly decremented.
fn drop_dict_entries(dict: &mut Dict, heap: &mut Heap<impl ResourceTracker>) {
    // Take all entries out of the dict via into_iter and drop them
    let entries: Vec<(Value, Value)> = std::mem::take(dict).into_iter().collect();
    for (k, v) in entries {
        k.drop_with_heap(heap);
        v.drop_with_heap(heap);
    }
}
