//! Compatibility implementation of `collections.abc`.
//!
//! This module exports the public names expected from CPython's
//! `collections.abc` surface. Ouros currently maps these names to existing
//! runtime classes/functions where possible so stdlib and third-party imports
//! can resolve the same identifiers.

use crate::{
    builtins::Builtins,
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::{ModuleFunctions, abc::AbcFunctions},
    resource::{ResourceError, ResourceTracker},
    types::{Module, Type},
    value::Value,
};

/// Creates the `collections.abc` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::CollectionsAbc);

    let abc_module_id = super::abc::create_module(heap, interns)?;
    if let Some(abc_meta_id) = module_attr_ref_id(abc_module_id, "ABCMeta", heap, interns) {
        heap.inc_ref(abc_meta_id);
        module.set_attr_text("ABCMeta", Value::Ref(abc_meta_id), heap, interns)?;
    } else {
        set_type_alias(&mut module, "ABCMeta", Type::Type, heap, interns)?;
    }
    Value::Ref(abc_module_id).drop_with_heap(heap);

    module.set_attr_text(
        "abstractmethod",
        Value::ModuleFunction(ModuleFunctions::Abc(AbcFunctions::Abstractmethod)),
        heap,
        interns,
    )?;

    let abstract_aliases: [(&str, Type); 26] = [
        ("Awaitable", Type::Object),
        ("Coroutine", Type::Object),
        ("AsyncIterable", Type::Object),
        ("AsyncIterator", Type::Object),
        ("AsyncGenerator", Type::Object),
        ("Hashable", Type::Object),
        ("Iterable", Type::Object),
        ("Iterator", Type::Iterator),
        ("Generator", Type::Generator),
        ("Reversible", Type::Object),
        ("Sized", Type::Object),
        ("Container", Type::Object),
        ("Collection", Type::Object),
        ("Callable", Type::Object),
        ("Set", Type::Set),
        ("MutableSet", Type::Set),
        ("Mapping", Type::Dict),
        ("MutableMapping", Type::Dict),
        ("MappingView", Type::Object),
        ("KeysView", Type::DictKeys),
        ("ItemsView", Type::DictItems),
        ("ValuesView", Type::DictValues),
        ("Sequence", Type::List),
        ("MutableSequence", Type::List),
        ("ByteString", Type::Bytes),
        ("Buffer", Type::Bytes),
    ];
    for (name, ty) in abstract_aliases {
        set_type_alias(&mut module, name, ty, heap, interns)?;
    }

    let concrete_type_aliases: [(&str, Type); 9] = [
        ("EllipsisType", Type::Ellipsis),
        ("FunctionType", Type::Function),
        ("GenericAlias", Type::GenericAlias),
        ("async_generator", Type::AsyncGenerator),
        ("coroutine", Type::Coroutine),
        ("generator", Type::Generator),
        ("dict_keys", Type::DictKeys),
        ("dict_items", Type::DictItems),
        ("dict_values", Type::DictValues),
    ];
    for (name, ty) in concrete_type_aliases {
        set_type_alias(&mut module, name, ty, heap, interns)?;
    }

    let iterator_aliases = [
        "bytearray_iterator",
        "bytes_iterator",
        "dict_itemiterator",
        "dict_keyiterator",
        "dict_valueiterator",
        "list_iterator",
        "list_reverseiterator",
        "longrange_iterator",
        "range_iterator",
        "set_iterator",
        "str_iterator",
        "tuple_iterator",
        "zip_iterator",
    ];
    for name in iterator_aliases {
        set_type_alias(&mut module, name, Type::Iterator, heap, interns)?;
    }

    set_type_alias(&mut module, "mappingproxy", Type::MappingProxy, heap, interns)?;
    set_type_alias(&mut module, "framelocalsproxy", Type::Object, heap, interns)?;

    let sys_module_id = super::sys::create_module(heap, interns)?;
    module.set_attr_text("sys", Value::Ref(sys_module_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Returns a referenced module attribute heap id when `attr` is a `Value::Ref`.
fn module_attr_ref_id(
    module_id: HeapId,
    attr: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<HeapId> {
    let HeapData::Module(module) = heap.get(module_id) else {
        return None;
    };
    match module.attrs().get_by_str(attr, heap, interns) {
        Some(Value::Ref(id)) => Some(*id),
        _ => None,
    }
}

/// Registers a class-like alias in `collections.abc`.
fn set_type_alias(
    module: &mut Module,
    name: &str,
    ty: Type,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(name, Value::Builtin(Builtins::Type(ty)), heap, interns)
}
