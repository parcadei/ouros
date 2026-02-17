//! Compatibility implementation of Python's `types` module.
//!
//! Ouros cannot execute arbitrary Python callables from module-native Rust paths
//! without VM continuation, so this implementation focuses on matching CPython
//! behavior for high-value synchronous paths and aliases while keeping strict
//! reference-count safety on every branch.

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::{Builtins, BuiltinsFunctions},
    defer_drop,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::NoPrint,
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, Module, NamedTuple, OurosIter, PyTrait, Type, allocate_tuple},
    value::{EitherStr, Marker, Value},
};

/// `types` module functions implemented by Ouros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum TypesFunctions {
    Simplenamespace,
    Coroutine,
    GetOriginalBases,
    NewClass,
    PrepareClass,
    ResolveBases,
}

/// Creates the `types` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::TypesMod);

    module.set_attr_text(
        "SimpleNamespace",
        Value::ModuleFunction(ModuleFunctions::Types(TypesFunctions::Simplenamespace)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "coroutine",
        Value::ModuleFunction(ModuleFunctions::Types(TypesFunctions::Coroutine)),
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
        "new_class",
        Value::ModuleFunction(ModuleFunctions::Types(TypesFunctions::NewClass)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "prepare_class",
        Value::ModuleFunction(ModuleFunctions::Types(TypesFunctions::PrepareClass)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "resolve_bases",
        Value::ModuleFunction(ModuleFunctions::Types(TypesFunctions::ResolveBases)),
        heap,
        interns,
    )?;

    // CPython-style public aliases.
    module.set_attr_text(
        "FunctionType",
        Value::Builtin(Builtins::Type(Type::Function)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "LambdaType",
        Value::Builtin(Builtins::Type(Type::Function)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "BuiltinFunctionType",
        Value::Builtin(Builtins::Type(Type::BuiltinFunction)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "MethodType",
        Value::Builtin(Builtins::Type(Type::Method)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ModuleType",
        Value::Builtin(Builtins::Type(Type::Module)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "MappingProxyType",
        Value::Builtin(Builtins::Type(Type::MappingProxy)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "GenericAlias",
        Value::Builtin(Builtins::Type(Type::GenericAlias)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "NoneType",
        Value::Builtin(Builtins::Type(Type::NoneType)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "AsyncGeneratorType",
        Value::Builtin(Builtins::Type(Type::AsyncGenerator)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "BuiltinMethodType",
        Value::Builtin(Builtins::Type(Type::BuiltinFunction)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "CapsuleType",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text("CellType", Value::Builtin(Builtins::Type(Type::Cell)), heap, interns)?;
    module.set_attr_text(
        "ClassMethodDescriptorType",
        Value::Builtin(Builtins::Type(Type::BuiltinFunction)),
        heap,
        interns,
    )?;
    // Ouros currently surfaces `func.__code__` as the function object.
    module.set_attr_text(
        "CodeType",
        Value::Builtin(Builtins::Type(Type::Function)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "CoroutineType",
        Value::Builtin(Builtins::Type(Type::Coroutine)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "DynamicClassAttribute",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "EllipsisType",
        Value::Builtin(Builtins::Type(Type::Ellipsis)),
        heap,
        interns,
    )?;
    // Ouros generator frame proxies are dataclass-backed objects.
    module.set_attr_text(
        "FrameType",
        Value::Builtin(Builtins::Type(Type::Dataclass)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "GeneratorType",
        Value::Builtin(Builtins::Type(Type::Generator)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "GetSetDescriptorType",
        Value::Builtin(Builtins::Type(Type::GetSetDescriptor)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "MemberDescriptorType",
        Value::Builtin(Builtins::Type(Type::MemberDescriptor)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "MethodDescriptorType",
        Value::Builtin(Builtins::Type(Type::BuiltinFunction)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "MethodWrapperType",
        Value::Builtin(Builtins::Type(Type::BuiltinFunction)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "NotImplementedType",
        // Ouros currently tags `NotImplemented` as `NoneType`.
        Value::Builtin(Builtins::Type(Type::NoneType)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "TracebackType",
        Value::Builtin(Builtins::Type(Type::Exception(ExcType::BaseException))),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "UnionType",
        Value::Marker(Marker(StaticStrings::UnionType)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "WrapperDescriptorType",
        Value::Builtin(Builtins::Type(Type::BuiltinFunction)),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `types` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TypesFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        TypesFunctions::Simplenamespace => simple_namespace(heap, interns, args).map(AttrCallResult::Value),
        TypesFunctions::Coroutine => types_coroutine(heap, interns, args).map(AttrCallResult::Value),
        TypesFunctions::GetOriginalBases => get_original_bases(heap, interns, args).map(AttrCallResult::Value),
        TypesFunctions::NewClass => new_class(heap, interns, args).map(AttrCallResult::Value),
        TypesFunctions::PrepareClass => prepare_class(heap, interns, args).map(AttrCallResult::Value),
        TypesFunctions::ResolveBases => resolve_bases(heap, interns, args).map(AttrCallResult::Value),
    }
}

/// Implements `types.SimpleNamespace(mapping_or_iterable=(), **kwargs)`.
///
/// Ouros models instances as namedtuple-backed objects for compact repr parity.
/// This captures common `types.SimpleNamespace(...)` usage while preserving
/// deterministic insertion order for repr output.
fn simple_namespace(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();

    let mut entries: Vec<(String, Value)> = Vec::new();

    let maybe_mapping = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        let actual = 2 + positional.len();
        positional.drop_with_heap(heap);
        maybe_mapping.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "SimpleNamespace expected at most 1 argument, got {actual}"
        )));
    }
    positional.drop_with_heap(heap);

    if let Some(mapping) = maybe_mapping {
        let mapping_dict = value_to_dict(mapping, heap, interns)?;
        let pairs = clone_dict_pairs(&mapping_dict, heap)?;
        mapping_dict.drop_with_heap(heap);

        for (key, value) in pairs {
            let Some(key_name) = key.as_either_str(heap) else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                drop_namespace_entries(entries, heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            upsert_namespace_entry(&mut entries, key_name.as_str(interns), value, heap);
            key.drop_with_heap(heap);
        }
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            drop_namespace_entries(entries, heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        upsert_namespace_entry(&mut entries, name.as_str(interns), value, heap);
        key.drop_with_heap(heap);
    }

    let mut field_names = Vec::with_capacity(entries.len());
    let mut items = Vec::with_capacity(entries.len());
    for (name, value) in entries {
        field_names.push(EitherStr::Heap(name));
        items.push(value);
    }

    let ns = NamedTuple::new("namespace".to_owned(), field_names, items);
    let ns_id = heap.allocate(HeapData::NamedTuple(ns))?;
    Ok(Value::Ref(ns_id))
}

/// Implements `types.coroutine(func)`.
///
/// Ouros currently mirrors CPython's call contract and callable validation, then
/// returns `func` unchanged.
fn types_coroutine(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("coroutine", heap)?;
    if !is_value_callable(&value, heap, interns) {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error("types.coroutine() expects a callable"));
    }
    let cloned = value.clone_with_heap(heap);
    value.drop_with_heap(heap);
    Ok(cloned)
}

/// Implements `types.get_original_bases(cls)`.
///
/// Returns `cls.__orig_bases__` when present, otherwise `cls.__bases__`.
fn get_original_bases(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let cls = args.get_one_arg("types.get_original_bases", heap)?;
    if cls.py_type(heap) != Type::Type {
        let ty = cls.py_type(heap);
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("Expected an instance of type, not '{ty}'")));
    }

    let dunder_orig_bases: crate::intern::StringId = StaticStrings::DunderOrigBases.into();
    if let Ok(AttrCallResult::Value(value)) = cls.py_getattr(dunder_orig_bases, heap, interns) {
        cls.drop_with_heap(heap);
        return Ok(value);
    }

    let dunder_bases: crate::intern::StringId = StaticStrings::DunderBases.into();
    let value = if let AttrCallResult::Value(value) = cls.py_getattr(dunder_bases, heap, interns)? {
        value
    } else {
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error("types.get_original_bases() expected a value"));
    };
    cls.drop_with_heap(heap);
    Ok(value)
}

/// Implements `types.resolve_bases(bases)`.
///
/// Non-class entries that define `__mro_entries__` are expanded; otherwise
/// `bases` is returned unchanged.
fn resolve_bases(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let bases = args.get_one_arg("types.resolve_bases", heap)?;
    resolve_bases_impl(bases, heap, interns)
}

/// Implements `types.prepare_class(name, bases=(), kwds=None)`.
fn prepare_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let parsed = parse_class_factory_args(args, "prepare_class", false, heap, interns)?;
    defer_drop!(parsed, heap);
    let prepared = prepare_class_impl(
        parsed.name.clone_with_heap(heap),
        parsed.bases.clone_with_heap(heap),
        parsed.kwds.clone_with_heap(heap),
        heap,
        interns,
    )?;
    defer_drop!(prepared, heap);

    Ok(allocate_tuple(
        vec![
            prepared.metaclass.clone_with_heap(heap),
            prepared.namespace.clone_with_heap(heap),
            prepared.kwds.clone_with_heap(heap),
        ]
        .into(),
        heap,
    )?)
}

/// Implements `types.new_class(name, bases=(), kwds=None, exec_body=None)`.
fn new_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let parsed = parse_class_factory_args(args, "new_class", true, heap, interns)?;
    defer_drop!(parsed, heap);

    let original_bases = parsed.bases.clone_with_heap(heap);
    defer_drop!(original_bases, heap);
    let resolved_bases = resolve_bases_impl(parsed.bases.clone_with_heap(heap), heap, interns)?;
    defer_drop!(resolved_bases, heap);
    let prepared = prepare_class_impl(
        parsed.name.clone_with_heap(heap),
        resolved_bases.clone_with_heap(heap),
        parsed.kwds.clone_with_heap(heap),
        heap,
        interns,
    )?;
    defer_drop!(prepared, heap);

    if let Some(exec_body) = &parsed.exec_body
        && !matches!(exec_body, Value::None)
    {
        if !is_value_callable(exec_body, heap, interns) {
            return Err(ExcType::type_error(format!(
                "'{}' object is not callable",
                exec_body.py_type(heap)
            )));
        }
        let call_args = ArgValues::One(prepared.namespace.clone_with_heap(heap));
        let exec_result = call_value_sync(exec_body.clone_with_heap(heap), call_args, heap, interns)?;
        defer_drop!(exec_result, heap);
    }

    if !original_bases.py_eq(resolved_bases, heap, interns) {
        set_dict_item(
            &prepared.namespace,
            Value::InternString(StaticStrings::DunderOrigBases.into()),
            original_bases.clone_with_heap(heap),
            heap,
            interns,
        )?;
    }

    let call_kwargs = dict_to_kwargs_values(&prepared.kwds, heap, interns)?;
    let class_args = ArgValues::ArgsKargs {
        args: vec![
            parsed.name.clone_with_heap(heap),
            resolved_bases.clone_with_heap(heap),
            prepared.namespace.clone_with_heap(heap),
        ],
        kwargs: call_kwargs,
    };
    call_value_sync(prepared.metaclass.clone_with_heap(heap), class_args, heap, interns)
}

/// Parsed shared argument state for `types.prepare_class` and `types.new_class`.
struct ParsedClassFactoryArgs {
    name: Value,
    bases: Value,
    kwds: Value,
    exec_body: Option<Value>,
}

impl<T: ResourceTracker> DropWithHeap<T> for ParsedClassFactoryArgs {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.name.drop_with_heap(heap);
        self.bases.drop_with_heap(heap);
        self.kwds.drop_with_heap(heap);
        self.exec_body.drop_with_heap(heap);
    }
}

/// Parsed `prepare_class` internals ready for tuple return or class construction.
struct PreparedClass {
    metaclass: Value,
    namespace: Value,
    kwds: Value,
}

impl<T: ResourceTracker> DropWithHeap<T> for PreparedClass {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.metaclass.drop_with_heap(heap);
        self.namespace.drop_with_heap(heap);
        self.kwds.drop_with_heap(heap);
    }
}

/// Parses `name`, `bases`, `kwds`, and optional `exec_body` from positional/keyword args.
fn parse_class_factory_args(
    args: ArgValues,
    function_name: &str,
    allow_exec_body: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<ParsedClassFactoryArgs> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let positional_count = positional.len();
    let max_positional = if allow_exec_body { 4 } else { 3 };
    if positional_count > max_positional {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(
            function_name,
            max_positional,
            positional_count,
        ));
    }

    let mut name = positional.next();
    let mut bases = positional.next();
    let mut kwds = positional.next();
    let mut exec_body = if allow_exec_body { positional.next() } else { None };
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            name.drop_with_heap(heap);
            bases.drop_with_heap(heap);
            kwds.drop_with_heap(heap);
            exec_body.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_string();
        key.drop_with_heap(heap);

        let slot = match key_name.as_str() {
            "name" => &mut name,
            "bases" => &mut bases,
            "kwds" => &mut kwds,
            "exec_body" if allow_exec_body => &mut exec_body,
            _ => {
                value.drop_with_heap(heap);
                name.drop_with_heap(heap);
                bases.drop_with_heap(heap);
                kwds.drop_with_heap(heap);
                exec_body.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, key_name.as_str()));
            }
        };
        if slot.is_some() {
            value.drop_with_heap(heap);
            name.drop_with_heap(heap);
            bases.drop_with_heap(heap);
            kwds.drop_with_heap(heap);
            exec_body.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg(function_name, key_name.as_str()));
        }
        *slot = Some(value);
    }

    let Some(name) = name else {
        bases.drop_with_heap(heap);
        kwds.drop_with_heap(heap);
        exec_body.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &["name"],
        ));
    };
    let bases = match bases {
        Some(value) => value,
        None => allocate_tuple(Vec::new().into(), heap)?,
    };
    let kwds = match kwds {
        Some(Value::None) | None => {
            let dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
            Value::Ref(dict_id)
        }
        Some(value) => match value_to_dict(value, heap, interns) {
            Ok(value) => value,
            Err(err) => {
                name.drop_with_heap(heap);
                bases.drop_with_heap(heap);
                exec_body.drop_with_heap(heap);
                return Err(err);
            }
        },
    };

    Ok(ParsedClassFactoryArgs {
        name,
        bases,
        kwds,
        exec_body,
    })
}

/// Implements shared `types.prepare_class` selection logic.
fn prepare_class_impl(
    name: Value,
    bases: Value,
    kwds: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<PreparedClass> {
    defer_drop!(name, heap);
    defer_drop!(bases, heap);
    defer_drop!(kwds, heap);

    let (explicit_metaclass, cleaned_kwds) = extract_metaclass_kwarg(kwds, heap, interns)?;
    let metaclass = if let Some(meta) = explicit_metaclass {
        meta
    } else {
        select_default_metaclass(bases, heap, interns)?
    };
    defer_drop!(metaclass, heap);
    defer_drop!(cleaned_kwds, heap);

    let mut namespace = Value::Ref(heap.allocate(HeapData::Dict(Dict::new()))?);
    let dunder_prepare: crate::intern::StringId = StaticStrings::DunderPrepare.into();
    if let Ok(AttrCallResult::Value(prepare_callable)) = metaclass.py_getattr(dunder_prepare, heap, interns) {
        if is_value_callable(&prepare_callable, heap, interns) {
            let prepare_kwargs = dict_to_kwargs_values(cleaned_kwds, heap, interns)?;
            let call_args = ArgValues::ArgsKargs {
                args: vec![name.clone_with_heap(heap), bases.clone_with_heap(heap)],
                kwargs: prepare_kwargs,
            };
            namespace.drop_with_heap(heap);
            namespace = call_value_sync(prepare_callable, call_args, heap, interns)?;
        } else {
            prepare_callable.drop_with_heap(heap);
        }
    }
    defer_drop!(namespace, heap);

    Ok(PreparedClass {
        metaclass: metaclass.clone_with_heap(heap),
        namespace: namespace.clone_with_heap(heap),
        kwds: cleaned_kwds.clone_with_heap(heap),
    })
}

/// Resolves base entries according to `__mro_entries__` when available.
fn resolve_bases_impl(bases: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    defer_drop!(bases, heap);
    let iterable_copy = bases.clone_with_heap(heap);
    let items = collect_iterable_items(iterable_copy, heap, interns)?;

    let mut changed = false;
    let mut resolved = Vec::new();

    for base in items {
        if base.py_type(heap) != Type::Type {
            let dunder_mro_entries: crate::intern::StringId = StaticStrings::DunderMroEntries.into();
            if let Ok(AttrCallResult::Value(mro_entries_callable)) = base.py_getattr(dunder_mro_entries, heap, interns)
            {
                changed = true;
                let call_args = ArgValues::One(bases.clone_with_heap(heap));
                let mro_entries = match call_value_sync(mro_entries_callable, call_args, heap, interns) {
                    Ok(value) => value,
                    Err(err) => {
                        base.drop_with_heap(heap);
                        resolved.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                let mro_entries_items = match tuple_items_as_cloned_values(&mro_entries, heap) {
                    Ok(value) => value,
                    Err(err) => {
                        mro_entries.drop_with_heap(heap);
                        base.drop_with_heap(heap);
                        resolved.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                mro_entries.drop_with_heap(heap);
                base.drop_with_heap(heap);
                resolved.extend(mro_entries_items);
                continue;
            }
        }
        resolved.push(base);
    }

    if !changed {
        resolved.drop_with_heap(heap);
        return Ok(bases.clone_with_heap(heap));
    }

    Ok(allocate_tuple(resolved.into(), heap)?)
}

/// Returns cloned tuple elements from `value`, enforcing tuple output type.
fn tuple_items_as_cloned_values(value: &Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Vec<Value>> {
    let Value::Ref(id) = value else {
        return Err(ExcType::type_error("__mro_entries__ must return a tuple"));
    };
    let HeapData::Tuple(tuple) = heap.get(*id) else {
        return Err(ExcType::type_error("__mro_entries__ must return a tuple"));
    };
    Ok(tuple.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect())
}

/// Returns all items from an iterable as owned values.
fn collect_iterable_items(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut items = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(err) => {
                items.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(items)
}

/// Returns `(explicit_metaclass, cleaned_kwds_without_metaclass)`.
fn extract_metaclass_kwarg(
    kwds: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<Value>, Value)> {
    let pairs = clone_dict_pairs(kwds, heap)?;
    let mut explicit_metaclass: Option<Value> = None;
    let mut cleaned_pairs = Vec::new();

    for (key, value) in pairs {
        if let Some(key_name) = key.as_either_str(heap)
            && key_name.as_str(interns) == "metaclass"
        {
            let old = explicit_metaclass.replace(value);
            old.drop_with_heap(heap);
            key.drop_with_heap(heap);
            continue;
        }
        cleaned_pairs.push((key, value));
    }

    let cleaned_dict = Dict::from_pairs(cleaned_pairs, heap, interns)?;
    let cleaned_dict_id = heap.allocate(HeapData::Dict(cleaned_dict))?;
    Ok((explicit_metaclass, Value::Ref(cleaned_dict_id)))
}

/// Chooses `type` by default, or the first base's metaclass when present.
fn select_default_metaclass(
    bases: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let bases_copy = bases.clone_with_heap(heap);
    let items = collect_iterable_items(bases_copy, heap, interns)?;
    let result = if let Some(first) = items.first() {
        match first {
            Value::Ref(id) => match heap.get(*id) {
                HeapData::ClassObject(class_obj) => match class_obj.metaclass() {
                    Value::Builtin(Builtins::Type(Type::Type)) => {
                        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type))
                    }
                    value => value.clone_with_heap(heap),
                },
                _ => Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)),
            },
            Value::Builtin(Builtins::Type(_) | Builtins::Function(BuiltinsFunctions::Type)) => {
                Value::Builtin(Builtins::Function(BuiltinsFunctions::Type))
            }
            _ => Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)),
        }
    } else {
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type))
    };
    items.drop_with_heap(heap);
    Ok(result)
}

/// Converts a dict `Value` into `KwargsValues::Dict` by cloning pairs.
fn dict_to_kwargs_values(
    dict_value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<KwargsValues> {
    let pairs = clone_dict_pairs(dict_value, heap)?;
    let dict = Dict::from_pairs(pairs, heap, interns)?;
    Ok(KwargsValues::Dict(dict))
}

/// Returns cloned key/value pairs from a dict `Value`.
fn clone_dict_pairs(dict_value: &Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Vec<(Value, Value)>> {
    let Value::Ref(dict_id) = dict_value else {
        return Err(ExcType::type_error("'kwds' must be a mapping"));
    };
    let HeapData::Dict(dict) = heap.get(*dict_id) else {
        return Err(ExcType::type_error("'kwds' must be a mapping"));
    };
    Ok(dict
        .iter()
        .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
        .collect())
}

/// Sets one dictionary item, dropping any replaced previous value.
fn set_dict_item(
    dict_value: &Value,
    key: Value,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let Value::Ref(dict_id) = dict_value else {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Err(ExcType::type_error("namespace is not a dict"));
    };
    if !matches!(heap.get(*dict_id), HeapData::Dict(_)) {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Err(ExcType::type_error("namespace is not a dict"));
    }
    let old = heap.with_entry_mut(*dict_id, |heap, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("namespace is not a dict"));
        };
        dict.set(key, value, heap, interns)
    })?;
    if let Some(old) = old {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Calls supported callable values synchronously from module-native code.
fn call_value_sync(
    callable: Value,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match callable {
        Value::Builtin(builtin) => {
            let mut print = NoPrint;
            builtin.call(heap, args, interns, &mut print)
        }
        Value::ModuleFunction(module_function) => match module_function.call(heap, interns, args)? {
            AttrCallResult::Value(value) => Ok(value),
            _ => Err(ExcType::type_error("types helper expected a value result")),
        },
        Value::Ref(heap_id) => {
            if let Some(builtin_type) = heap.builtin_type_for_class_id(heap_id) {
                let mut print = NoPrint;
                let result = Builtins::Type(builtin_type).call(heap, args, interns, &mut print);
                Value::Ref(heap_id).drop_with_heap(heap);
                return result;
            }
            if matches!(heap.get(heap_id), HeapData::BoundMethod(_)) {
                let (func, self_arg) = match heap.get(heap_id) {
                    HeapData::BoundMethod(method) => (
                        method.func().clone_with_heap(heap),
                        method.self_arg().clone_with_heap(heap),
                    ),
                    _ => unreachable!("checked bound method variant"),
                };
                Value::Ref(heap_id).drop_with_heap(heap);
                let (positional, kwargs) = args.into_parts();
                let mut forwarded = Vec::with_capacity(positional.len() + 1);
                forwarded.push(self_arg);
                forwarded.extend(positional);
                return call_value_sync(
                    func,
                    ArgValues::ArgsKargs {
                        args: forwarded,
                        kwargs,
                    },
                    heap,
                    interns,
                );
            }
            let ty = heap.get(heap_id).py_type(heap);
            Value::Ref(heap_id).drop_with_heap(heap);
            args.drop_with_heap(heap);
            Err(ExcType::type_error(format!("'{ty}' object is not callable")))
        }
        other => {
            args.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{}' object is not callable",
                other.py_type(heap)
            )))
        }
    }
}

/// Converts one value into a dict via `dict(...)`.
fn value_to_dict(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let dict_value = Dict::init(heap, ArgValues::One(value), interns)?;
    let Value::Ref(dict_id) = dict_value else {
        dict_value.drop_with_heap(heap);
        return Err(ExcType::type_error("expected dict result"));
    };
    if !matches!(heap.get(dict_id), HeapData::Dict(_)) {
        let ty = heap.get(dict_id).py_type(heap);
        dict_value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("expected dict result, got {ty}")));
    }
    Ok(dict_value)
}

/// Returns whether a runtime value is callable under Ouros VM dispatch rules.
fn is_value_callable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Marker(marker) => marker.is_callable(),
        Value::Ref(heap_id) => is_heap_value_callable(*heap_id, heap, interns),
        _ => false,
    }
}

/// Returns whether a heap value is callable.
fn is_heap_value_callable(heap_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match heap.get(heap_id) {
        HeapData::ClassSubclasses(_)
        | HeapData::ClassGetItem(_)
        | HeapData::GenericAlias(_)
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
    }
}

/// Updates or inserts one namespace entry while preserving insertion order.
fn upsert_namespace_entry(
    entries: &mut Vec<(String, Value)>,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
) {
    for (existing_key, existing_value) in entries.iter_mut() {
        if existing_key == key {
            let old = std::mem::replace(existing_value, value);
            old.drop_with_heap(heap);
            return;
        }
    }
    entries.push((key.to_owned(), value));
}

/// Drops all values tracked in `entries`.
fn drop_namespace_entries(entries: Vec<(String, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (_, value) in entries {
        value.drop_with_heap(heap);
    }
}
