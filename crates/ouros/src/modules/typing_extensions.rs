//! Runtime compatibility implementation of `typing_extensions`.
//!
//! The goal of this module is practical parity with CPython's
//! `typing_extensions` import surface. We reuse Ouros's `typing` runtime helpers
//! whenever possible and provide focused compatibility shims for names that are
//! specific to `typing_extensions`.

use smallvec::SmallVec;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::{BuiltinModule, ModuleFunctions, types_mod::TypesFunctions, typing::TypingFunctions},
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, Instance, Module, PyTrait, Str, Type, allocate_tuple},
    value::Value,
};

/// `typing_extensions` callables implemented directly by this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum TypingExtensionsFunctions {
    /// `typing_extensions.IntVar(name)` – `TypeVar`-style runtime marker.
    #[strum(serialize = "IntVar")]
    IntVar,
    /// `typing_extensions.disjoint_base(cls)` – runtime no-op decorator.
    #[strum(serialize = "disjoint_base")]
    DisjointBase,
    /// `typing_extensions.assert_never(value)` – always raises `AssertionError`.
    #[strum(serialize = "assert_never")]
    AssertNever,
    /// `typing_extensions.deprecated(...)` – runtime-safe decorator/factory.
    #[strum(serialize = "deprecated")]
    Deprecated,
    /// Internal helper returned by `deprecated(...)`.
    #[strum(serialize = "_deprecated_decorator")]
    DeprecatedDecorator,
    /// `typing_extensions.type_repr(value)` – typing-style repr formatter.
    #[strum(serialize = "type_repr")]
    TypeRepr,
    /// `typing_extensions.get_annotations(obj, ...)` – annotation mapping lookup.
    #[strum(serialize = "get_annotations")]
    GetAnnotations,
    /// `typing_extensions.evaluate_forward_ref(ref, ...)` compatibility shim.
    #[strum(serialize = "evaluate_forward_ref")]
    EvaluateForwardRef,
    /// `typing_extensions.Doc(text)` – wraps text in an object with `.documentation`.
    #[strum(serialize = "Doc")]
    DocCtor,
    /// `typing_extensions.Sentinel(name, repr=None)` – allocates a unique object.
    #[strum(serialize = "Sentinel")]
    SentinelCtor,
    /// `typing_extensions.TypeForm(obj)` – runtime identity helper.
    #[strum(serialize = "TypeForm")]
    TypeForm,
}

/// CPython 3.14 public API names expected on `typing_extensions`.
const CPYTHON_PUBLIC_API: &[&str] = &[
    "AbstractSet",
    "Annotated",
    "Any",
    "AnyStr",
    "AsyncContextManager",
    "AsyncGenerator",
    "AsyncIterable",
    "AsyncIterator",
    "Awaitable",
    "BinaryIO",
    "Buffer",
    "Callable",
    "CapsuleType",
    "ChainMap",
    "ClassVar",
    "Collection",
    "Concatenate",
    "Container",
    "ContextManager",
    "Coroutine",
    "Counter",
    "DefaultDict",
    "Deque",
    "Dict",
    "Doc",
    "Final",
    "Format",
    "ForwardRef",
    "FrozenSet",
    "Generator",
    "Generic",
    "GenericMeta",
    "Hashable",
    "IO",
    "IntVar",
    "ItemsView",
    "Iterable",
    "Iterator",
    "KT",
    "KeysView",
    "List",
    "Literal",
    "LiteralString",
    "Mapping",
    "MappingView",
    "Match",
    "MutableMapping",
    "MutableSequence",
    "MutableSet",
    "NamedTuple",
    "Never",
    "NewType",
    "NoDefault",
    "NoExtraItems",
    "NoReturn",
    "NotRequired",
    "Optional",
    "OrderedDict",
    "PEP_560",
    "ParamSpec",
    "ParamSpecArgs",
    "ParamSpecKwargs",
    "Pattern",
    "Protocol",
    "ReadOnly",
    "Reader",
    "Required",
    "Reversible",
    "Self",
    "Sentinel",
    "Sequence",
    "Set",
    "Sized",
    "SupportsAbs",
    "SupportsBytes",
    "SupportsComplex",
    "SupportsFloat",
    "SupportsIndex",
    "SupportsInt",
    "SupportsRound",
    "T",
    "TYPE_CHECKING",
    "T_co",
    "T_contra",
    "Text",
    "TextIO",
    "Tuple",
    "Type",
    "TypeAlias",
    "TypeAliasType",
    "TypeForm",
    "TypeGuard",
    "TypeIs",
    "TypeVar",
    "TypeVarTuple",
    "TypedDict",
    "Union",
    "Unpack",
    "VT",
    "ValuesView",
    "Writer",
    "abc",
    "annotationlib",
    "assert_never",
    "assert_type",
    "builtins",
    "cast",
    "clear_overloads",
    "collections",
    "contextlib",
    "dataclass_transform",
    "deprecated",
    "disjoint_base",
    "enum",
    "evaluate_forward_ref",
    "final",
    "functools",
    "get_annotations",
    "get_args",
    "get_origin",
    "get_original_bases",
    "get_overloads",
    "get_protocol_members",
    "get_type_hints",
    "inspect",
    "io",
    "is_protocol",
    "is_typeddict",
    "keyword",
    "no_type_check",
    "no_type_check_decorator",
    "operator",
    "overload",
    "override",
    "reveal_type",
    "runtime",
    "runtime_checkable",
    "sys",
    "type_repr",
    "typing",
    "warnings",
];

/// Creates the `typing_extensions` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::TypingExtensions);
    let typing_id = super::typing::create_module(heap, interns)?;

    let build_result: Result<(), ResourceError> = (|| {
        for name in collect_public_typing_names(typing_id, heap, interns) {
            if name == "ByteString" || name == "GenericAlias" {
                continue;
            }
            let _ = copy_attr_from_module(&mut module, name.as_str(), typing_id, heap, interns)?;
        }

        // Keep a direct module handle for `typing_extensions.typing`.
        heap.inc_ref(typing_id);
        module.set_attr_text("typing", Value::Ref(typing_id), heap, interns)?;

        register_module_aliases(&mut module, typing_id, heap, interns)?;
        register_callable_aliases(&mut module, heap, interns)?;
        register_runtime_markers(&mut module, heap, interns)?;
        ensure_typevar_exports(&mut module, heap, interns)?;

        // Fill any still-missing names with a conservative object sentinel so the
        // public surface remains import-compatible.
        for &name in CPYTHON_PUBLIC_API {
            ensure_attr_exists(
                &mut module,
                name,
                Value::Builtin(Builtins::Type(Type::Object)),
                heap,
                interns,
            )?;
        }
        Ok(())
    })();

    match build_result {
        Ok(()) => {
            // Drop temporary source typing module after cloning/re-exporting values.
            heap.dec_ref(typing_id);
            heap.allocate(HeapData::Module(module))
        }
        Err(err) => {
            module.drop_with_heap(heap);
            heap.dec_ref(typing_id);
            Err(err)
        }
    }
}

/// Dispatches calls for `typing_extensions` module-level callables.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TypingExtensionsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        TypingExtensionsFunctions::IntVar => int_var(heap, interns, args),
        TypingExtensionsFunctions::DisjointBase => identity_one_arg(heap, args, "typing_extensions.disjoint_base"),
        TypingExtensionsFunctions::AssertNever => assert_never(heap, interns, args),
        TypingExtensionsFunctions::Deprecated => deprecated(heap, interns, args),
        TypingExtensionsFunctions::DeprecatedDecorator => {
            identity_one_arg(heap, args, "typing_extensions.deprecated.<decorator>")
        }
        TypingExtensionsFunctions::TypeRepr => type_repr(heap, interns, args),
        TypingExtensionsFunctions::GetAnnotations => get_annotations(heap, interns, args),
        TypingExtensionsFunctions::EvaluateForwardRef => evaluate_forward_ref(heap, interns, args),
        TypingExtensionsFunctions::DocCtor => doc_ctor(heap, interns, args),
        TypingExtensionsFunctions::SentinelCtor => sentinel_ctor(heap, interns, args),
        TypingExtensionsFunctions::TypeForm => identity_one_arg(heap, args, "typing_extensions.TypeForm"),
    }
}

/// Implements `typing_extensions.IntVar(name)`.
fn int_var(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let name = args.get_one_arg("typing_extensions.IntVar", heap)?;
    if !is_string_value(&name, heap) {
        name.drop_with_heap(heap);
        return Err(ExcType::type_error("IntVar() argument 'name' must be str"));
    }
    let value = create_typevar_like_instance(name, false, false, heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Implements `typing_extensions.assert_never(value)`.
fn assert_never(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("typing_extensions.assert_never", heap)?;
    let repr = value.py_repr(heap, interns).into_owned();
    value.drop_with_heap(heap);
    Err(SimpleException::new_msg(
        ExcType::AssertionError,
        format!("Expected code to be unreachable, but got: {repr}"),
    )
    .into())
}

/// Implements `typing_extensions.deprecated(...)` as a runtime-safe decorator.
fn deprecated(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let first = positional.next();
    let second = positional.next();

    // Direct decorator usage: deprecated(callable) -> callable.
    if second.is_none()
        && kwargs.is_empty()
        && let Some(value) = first
    {
        if looks_callable(&value, heap, interns) {
            return Ok(AttrCallResult::Value(value));
        }
        value.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(Value::ModuleFunction(
            ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::DeprecatedDecorator),
        )));
    }

    first.drop_with_heap(heap);
    second.drop_with_heap(heap);
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    // Factory form: deprecated(message=..., ...)(obj)
    Ok(AttrCallResult::Value(Value::ModuleFunction(
        ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::DeprecatedDecorator),
    )))
}

/// Implements `typing_extensions.type_repr(value)`.
fn type_repr(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg("typing_extensions.type_repr", heap)?;
    let text = type_repr_text(&value, heap, interns);
    value.drop_with_heap(heap);
    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(AttrCallResult::Value(Value::Ref(text_id)))
}

/// Implements `typing_extensions.get_annotations(obj, ...)`.
fn get_annotations(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(obj) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("typing_extensions.get_annotations", 1, 0));
    };

    let mut positional_count = 1usize;
    for extra in positional {
        positional_count += 1;
        extra.drop_with_heap(heap);
    }
    if positional_count > 1 {
        kwargs.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(
            "typing_extensions.get_annotations",
            1,
            positional_count,
        ));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns);
        if !matches!(key_name, "globals" | "locals" | "eval_str" | "format") {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(
                "typing_extensions.get_annotations",
                key_name,
            ));
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let annotations_id: crate::intern::StringId = StaticStrings::DunderAnnotations.into();
    let out = match obj.py_getattr(annotations_id, heap, interns) {
        Ok(AttrCallResult::Value(value)) => value,
        _ => Value::None,
    };
    obj.drop_with_heap(heap);

    if let Value::Ref(id) = &out
        && matches!(heap.get(*id), HeapData::Dict(_))
    {
        return Ok(AttrCallResult::Value(out));
    }
    out.drop_with_heap(heap);
    let dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    Ok(AttrCallResult::Value(Value::Ref(dict_id)))
}

/// Implements `typing_extensions.evaluate_forward_ref(ref, ...)`.
fn evaluate_forward_ref(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(forward_ref) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(
            "typing_extensions.evaluate_forward_ref",
            1,
            0,
        ));
    };

    let mut positional_count = 1usize;
    for extra in positional {
        positional_count += 1;
        extra.drop_with_heap(heap);
    }
    if positional_count > 1 {
        kwargs.drop_with_heap(heap);
        forward_ref.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(
            "typing_extensions.evaluate_forward_ref",
            1,
            positional_count,
        ));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            forward_ref.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns);
        if !matches!(
            key_name,
            "owner" | "globals" | "locals" | "type_params" | "format" | "_recursive_guard"
        ) {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            forward_ref.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(
                "typing_extensions.evaluate_forward_ref",
                key_name,
            ));
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    // Prefer evaluated value, then forward arg, finally identity fallback.
    let is_evaluated = match get_instance_attr_by_name(&forward_ref, "__forward_evaluated__", heap, interns) {
        Some(Value::Bool(value)) => value,
        Some(other) => {
            other.drop_with_heap(heap);
            false
        }
        None => false,
    };
    if is_evaluated && let Some(value) = get_instance_attr_by_name(&forward_ref, "__forward_value__", heap, interns) {
        forward_ref.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(value));
    }
    if let Some(value) = get_instance_attr_by_name(&forward_ref, "__forward_arg__", heap, interns) {
        if let Some(resolved) = resolve_forward_builtin(&value, heap, interns) {
            value.drop_with_heap(heap);
            forward_ref.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(resolved));
        }
        forward_ref.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(value));
    }

    Ok(AttrCallResult::Value(forward_ref))
}

/// Implements `typing_extensions.Doc(text)`.
fn doc_ctor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let documentation = args.get_one_arg("typing_extensions.Doc", heap)?;
    if !is_string_value(&documentation, heap) {
        documentation.drop_with_heap(heap);
        return Err(ExcType::type_error("Doc() argument 'documentation' must be str"));
    }
    let value = create_runtime_attrs_instance(vec![("documentation", documentation)], heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Implements `typing_extensions.Sentinel(name, repr=None)`.
fn sentinel_ctor(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (name, repr_value) = args.get_one_two_args("typing_extensions.Sentinel", heap)?;
    if !is_string_value(&name, heap) {
        name.drop_with_heap(heap);
        repr_value.drop_with_heap(heap);
        return Err(ExcType::type_error("Sentinel() argument 'name' must be str"));
    }
    if let Some(ref_value) = &repr_value
        && !is_string_value(ref_value, heap)
    {
        name.drop_with_heap(heap);
        repr_value.drop_with_heap(heap);
        return Err(ExcType::type_error("Sentinel() argument 'repr' must be str or None"));
    }

    let value = create_runtime_attrs_instance(
        vec![("_name", name), ("_repr", repr_value.unwrap_or(Value::None))],
        heap,
        interns,
    )?;
    Ok(AttrCallResult::Value(value))
}

/// Returns the input value unchanged, validating exactly one argument.
fn identity_one_arg(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, name: &str) -> RunResult<AttrCallResult> {
    let value = args.get_one_arg(name, heap)?;
    Ok(AttrCallResult::Value(value))
}

/// Registers module-valued exports expected by `typing_extensions`.
fn register_module_aliases(
    module: &mut Module,
    typing_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let aliases: &[(&str, BuiltinModule)] = &[
        ("abc", BuiltinModule::Abc),
        ("builtins", BuiltinModule::BuiltinsMod),
        ("collections", BuiltinModule::Collections),
        ("contextlib", BuiltinModule::Contextlib),
        ("enum", BuiltinModule::Enum),
        ("functools", BuiltinModule::Functools),
        ("inspect", BuiltinModule::Inspect),
        ("io", BuiltinModule::Io),
        ("operator", BuiltinModule::Operator),
        ("sys", BuiltinModule::Sys),
        ("warnings", BuiltinModule::Warnings),
    ];

    for &(name, builtin_module) in aliases {
        let module_id = builtin_module.create(heap, interns)?;
        module.set_attr_text(name, Value::Ref(module_id), heap, interns)?;
    }

    // `keyword` exists as a module in CPython; we provide a lightweight stub module.
    let keyword_module = Module::new(StaticStrings::KeywordMod);
    let keyword_id = heap.allocate(HeapData::Module(keyword_module))?;
    module.set_attr_text("keyword", Value::Ref(keyword_id), heap, interns)?;

    // Reuse typing module as a practical annotationlib fallback.
    heap.inc_ref(typing_id);
    module.set_attr_text("annotationlib", Value::Ref(typing_id), heap, interns)?;
    Ok(())
}

/// Registers callable compatibility shims exported by `typing_extensions`.
fn register_callable_aliases(
    module: &mut Module,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        "IntVar",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::IntVar)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "assert_never",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(
            TypingExtensionsFunctions::AssertNever,
        )),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "deprecated",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::Deprecated)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "disjoint_base",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(
            TypingExtensionsFunctions::DisjointBase,
        )),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "evaluate_forward_ref",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(
            TypingExtensionsFunctions::EvaluateForwardRef,
        )),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_annotations",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(
            TypingExtensionsFunctions::GetAnnotations,
        )),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_original_bases",
        Value::ModuleFunction(ModuleFunctions::Types(TypesFunctions::GetOriginalBases)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "runtime",
        Value::ModuleFunction(ModuleFunctions::Typing(TypingFunctions::RuntimeCheckable)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "type_repr",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::TypeRepr)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "Doc",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::DocCtor)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "Sentinel",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(
            TypingExtensionsFunctions::SentinelCtor,
        )),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "TypeForm",
        Value::ModuleFunction(ModuleFunctions::TypingExtensions(TypingExtensionsFunctions::TypeForm)),
        heap,
        interns,
    )?;
    Ok(())
}

/// Registers extra marker/class-like values not provided directly by `typing`.
fn register_runtime_markers(
    module: &mut Module,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text("PEP_560", Value::Bool(true), heap, interns)?;
    module.set_attr_text("GenericMeta", Value::Builtin(Builtins::Type(Type::Type)), heap, interns)?;
    module.set_attr_text("Buffer", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text(
        "CapsuleType",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text("Reader", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("Writer", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;

    let no_extra_items = create_runtime_attrs_instance(Vec::new(), heap, interns)?;
    module.set_attr_text("NoExtraItems", no_extra_items, heap, interns)?;

    let format_value = create_runtime_attrs_instance(
        vec![
            ("VALUE", Value::Int(1)),
            ("VALUE_WITH_FAKE_GLOBALS", Value::Int(2)),
            ("FORWARDREF", Value::Int(3)),
            ("STRING", Value::Int(4)),
        ],
        heap,
        interns,
    )?;
    module.set_attr_text("Format", format_value, heap, interns)?;
    Ok(())
}

/// Ensures the common exported `TypeVar` symbols exist.
fn ensure_typevar_exports(
    module: &mut Module,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    ensure_typevar_symbol(module, "T", false, false, heap, interns)?;
    ensure_typevar_symbol(module, "KT", false, false, heap, interns)?;
    ensure_typevar_symbol(module, "VT", false, false, heap, interns)?;
    ensure_typevar_symbol(module, "T_co", true, false, heap, interns)?;
    ensure_typevar_symbol(module, "T_contra", false, true, heap, interns)?;
    Ok(())
}

/// Ensures a single type-variable symbol is set on the module.
fn ensure_typevar_symbol(
    module: &mut Module,
    name: &str,
    covariant: bool,
    contravariant: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    if module.attrs().get_by_str(name, heap, interns).is_some() {
        return Ok(());
    }
    let name_value = Value::Ref(heap.allocate(HeapData::Str(Str::from(name)))?);
    let value = create_typevar_like_instance(name_value, covariant, contravariant, heap, interns)
        .expect("TypeVar compatibility object allocation should not fail with a runtime error");
    module.set_attr_text(name, value, heap, interns)
}

/// Creates a TypeVar-like runtime object with common public attributes.
fn create_typevar_like_instance(
    name: Value,
    covariant: bool,
    contravariant: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let constraints = allocate_tuple(SmallVec::new(), heap)?;
    create_runtime_attrs_instance(
        vec![
            ("__name__", name),
            ("__constraints__", constraints),
            ("__bound__", Value::None),
            ("__covariant__", Value::Bool(covariant)),
            ("__contravariant__", Value::Bool(contravariant)),
            ("__infer_variance__", Value::Bool(false)),
        ],
        heap,
        interns,
    )
    .map_err(Into::into)
}

/// Creates a lightweight instance with a mutable attribute dictionary.
fn create_runtime_attrs_instance(
    attrs: Vec<(&str, Value)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<Value, ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);
    let mut dict = Dict::new();
    for (name, value) in attrs {
        dict_set_string_key(&mut dict, name, value, heap, interns)?;
    }
    let attrs_id = heap.allocate(HeapData::Dict(dict))?;
    let instance = Instance::new(object_class, Some(attrs_id), Vec::new(), Vec::new());
    let instance_id = heap.allocate(HeapData::Instance(instance))?;
    Ok(Value::Ref(instance_id))
}

/// Sets a string-keyed entry in a dict while dropping any replaced value.
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

/// Returns true when a value is a Python string object.
fn is_string_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::InternString(_) => true,
        Value::Ref(id) => matches!(heap.get(*id), HeapData::Str(_)),
        _ => false,
    }
}

/// Returns true when a value can be treated as callable for decorator paths.
fn looks_callable(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let call_attr: crate::intern::StringId = StaticStrings::DunderCall.into();
    match value.py_getattr(call_attr, heap, interns) {
        Ok(AttrCallResult::Value(attr)) => {
            attr.drop_with_heap(heap);
            true
        }
        _ => false,
    }
}

/// Returns the best-effort typing-style representation for `type_repr`.
fn type_repr_text(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> String {
    match value {
        Value::Builtin(Builtins::Type(t)) => t.to_string(),
        Value::Builtin(Builtins::Function(function)) => function.to_string(),
        Value::DefFunction(function_id) => interns
            .get_str(interns.get_function(*function_id).name.name_id)
            .to_owned(),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::ClassObject(class_obj) => class_obj.name(interns).to_string(),
            _ => value.py_repr(heap, interns).into_owned(),
        },
        _ => value.py_repr(heap, interns).into_owned(),
    }
}

/// Reads a named attribute directly from an instance `__dict__`, if present.
fn get_instance_attr_by_name(
    value: &Value,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let Value::Ref(id) = value else {
        return None;
    };
    let HeapData::Instance(instance) = heap.get(*id) else {
        return None;
    };
    let attrs_id = instance.attrs_id()?;
    let HeapData::Dict(dict) = heap.get(attrs_id) else {
        return None;
    };
    dict.get_by_str(name, heap, interns).map(|v| v.clone_with_heap(heap))
}

/// Resolves a simple builtin forward-reference name into a runtime value.
fn resolve_forward_builtin(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<Value> {
    let name = match value {
        Value::InternString(id) => interns.get_str(*id),
        Value::Ref(id) => {
            let HeapData::Str(text) = heap.get(*id) else {
                return None;
            };
            text.as_str()
        }
        _ => return None,
    };

    match name {
        "int" => Some(Value::Builtin(Builtins::Type(Type::Int))),
        "str" => Some(Value::Builtin(Builtins::Type(Type::Str))),
        "float" => Some(Value::Builtin(Builtins::Type(Type::Float))),
        "bool" => Some(Value::Builtin(Builtins::Type(Type::Bool))),
        "bytes" => Some(Value::Builtin(Builtins::Type(Type::Bytes))),
        "list" => Some(Value::Builtin(Builtins::Type(Type::List))),
        "dict" => Some(Value::Builtin(Builtins::Type(Type::Dict))),
        "tuple" => Some(Value::Builtin(Builtins::Type(Type::Tuple))),
        "set" => Some(Value::Builtin(Builtins::Type(Type::Set))),
        "frozenset" => Some(Value::Builtin(Builtins::Type(Type::FrozenSet))),
        "object" => Some(Value::Builtin(Builtins::Type(Type::Object))),
        "None" => Some(Value::None),
        _ => None,
    }
}

/// Copies one attribute from a module object into another module.
fn copy_attr_from_module(
    target: &mut Module,
    name: &str,
    source_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<bool, ResourceError> {
    let maybe_value = {
        let HeapData::Module(source) = heap.get(source_id) else {
            return Ok(false);
        };
        source
            .attrs()
            .get_by_str(name, heap, interns)
            .map(|v| v.clone_with_heap(heap))
    };
    if let Some(value) = maybe_value {
        target.set_attr_text(name, value, heap, interns)?;
        return Ok(true);
    }
    Ok(false)
}

/// Ensures an attribute exists on the module, setting `value` only when absent.
fn ensure_attr_exists(
    module: &mut Module,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    if module.attrs().get_by_str(name, heap, interns).is_none() {
        module.set_attr_text(name, value, heap, interns)?;
        return Ok(());
    }
    value.drop_with_heap(heap);
    Ok(())
}

/// Collects public (non-underscore-prefixed) names from a typing module.
fn collect_public_typing_names(typing_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Vec<String> {
    let HeapData::Module(typing_module) = heap.get(typing_id) else {
        return Vec::new();
    };

    let mut names = Vec::new();
    for (key, _) in typing_module.attrs() {
        let name = match key {
            Value::InternString(id) => interns.get_str(*id).to_owned(),
            Value::Ref(id) => match heap.get(*id) {
                HeapData::Str(s) => s.as_str().to_owned(),
                _ => continue,
            },
            _ => continue,
        };
        if !name.starts_with('_') {
            names.push(name);
        }
    }
    names.sort();
    names.dedup();
    names
}
