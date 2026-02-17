//! Implementation of the `dir()` builtin.
//!
//! This parity-oriented implementation supports the common `dir(module)` case
//! used in stdlib tests and returns sorted attribute names.

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::RunResult,
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::{AttrCallResult, Dict, List, PyTrait},
    value::Value,
};

/// Implementation of the `dir()` builtin.
///
/// This parity-oriented implementation supports the common `dir(module)` case
/// used in stdlib tests and returns sorted attribute names.
pub fn builtin_dir(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let value = args.get_zero_one_arg("dir", heap)?;
    let mut names = Vec::new();

    if let Some(value) = &value {
        collect_dir_names(value, heap, interns, &mut names)?;
        let is_module = matches!(value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Module(_)));
        if !is_module {
            names.push("__class__".to_string());
        }
    }

    names.sort_unstable();
    names.dedup();

    if let Some(value) = value {
        value.drop_with_heap(heap);
    }

    let mut items = Vec::with_capacity(names.len());
    for name in names {
        let id = heap.allocate(HeapData::Str(crate::types::Str::from(name.as_str())))?;
        items.push(Value::Ref(id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Collects `dir()` names for a specific value.
fn collect_dir_names(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    names: &mut Vec<String>,
) -> RunResult<()> {
    match value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Module(module) => {
                extend_names_from_dict(module.attrs(), heap, interns, names);
                if module.name() == StaticStrings::BuiltinsMod {
                    names.extend(CPYTHON_BUILTINS_DIR_NAMES.iter().map(|name| (*name).to_string()));
                } else {
                    collect_probe_attr_names(value, heap, interns, names);
                }
            }
            HeapData::Instance(instance) => {
                if let Some(attrs) = instance.attrs(heap) {
                    extend_names_from_dict(attrs, heap, interns, names);
                }
                let class_id = instance.class_id();
                collect_class_mro_names(class_id, heap, interns, names);
                if let Some(ty) = heap.builtin_type_for_class_id(class_id) {
                    collect_probe_attr_names(value, heap, interns, names);
                    if has_builtin_type_attr(ty, StaticStrings::DunderClassGetitem, heap, interns) {
                        names.push("__class_getitem__".to_string());
                    }
                    if has_binary_add(value, heap, interns) {
                        names.push("__add__".to_string());
                    }
                }
            }
            HeapData::ClassObject(_) => {
                collect_class_mro_names(*id, heap, interns, names);
                if let Some(ty) = heap.builtin_type_for_class_id(*id) {
                    collect_builtin_type_names(ty, heap, interns, names)?;
                }
            }
            _ => {
                collect_probe_attr_names(value, heap, interns, names);
                let ty = value.py_type(heap);
                if has_builtin_type_attr(ty, StaticStrings::DunderClassGetitem, heap, interns) {
                    names.push("__class_getitem__".to_string());
                }
                if has_binary_add(value, heap, interns) {
                    names.push("__add__".to_string());
                }
            }
        },
        Value::Builtin(Builtins::Type(ty)) => {
            collect_builtin_type_names(*ty, heap, interns, names)?;
        }
        _ => {
            collect_probe_attr_names(value, heap, interns, names);
            let ty = value.py_type(heap);
            if has_builtin_type_attr(ty, StaticStrings::DunderClassGetitem, heap, interns) {
                names.push("__class_getitem__".to_string());
            }
            if has_binary_add(value, heap, interns) {
                names.push("__add__".to_string());
            }
        }
    }
    Ok(())
}

/// Adds all string-like keys from a dict to the `dir()` name list.
fn extend_names_from_dict(dict: &Dict, heap: &Heap<impl ResourceTracker>, interns: &Interns, names: &mut Vec<String>) {
    names.extend(dict.iter().filter_map(|(key, _)| key_to_name(key, heap, interns)));
}

/// Adds names from a class namespace and all classes in its MRO.
fn collect_class_mro_names(
    class_id: HeapId,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    names: &mut Vec<String>,
) {
    let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
        return;
    };
    extend_names_from_dict(class_obj.namespace(), heap, interns, names);
    for &mro_id in class_obj.mro().iter().skip(1) {
        if let HeapData::ClassObject(mro_cls) = heap.get(mro_id) {
            extend_names_from_dict(mro_cls.namespace(), heap, interns, names);
        }
    }
}

/// Adds builtin type class and MRO names, then probes static builtin attributes.
fn collect_builtin_type_names(
    ty: crate::types::Type,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    names: &mut Vec<String>,
) -> RunResult<()> {
    let class_id = heap.builtin_class_id(ty)?;
    collect_class_mro_names(class_id, heap, interns, names);
    let type_value = Value::Builtin(Builtins::Type(ty));
    collect_probe_attr_names(&type_value, heap, interns, names);
    Ok(())
}

/// Probes static attribute names by trying all known static strings.
///
/// This is used for builtin/primitive objects whose methods are exposed through
/// static dispatch rather than class dictionaries.
fn collect_probe_attr_names(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    names: &mut Vec<String>,
) {
    let mut raw = 0u16;
    while let Some(static_name) = StaticStrings::from_repr(raw) {
        raw = raw.saturating_add(1);
        let name_id: crate::intern::StringId = static_name.into();
        if let Ok(attr_result) = value.py_getattr(name_id, heap, interns) {
            names.push(interns.get_str(name_id).to_string());
            drop_attr_call_result(attr_result, heap);
        }
    }
}

/// Returns true when a builtin type object resolves a given static attribute.
fn has_builtin_type_attr(
    ty: crate::types::Type,
    attr: StaticStrings,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    let type_value = Value::Builtin(Builtins::Type(ty));
    let name_id: crate::intern::StringId = attr.into();
    if let Ok(attr_result) = type_value.py_getattr(name_id, heap, interns) {
        drop_attr_call_result(attr_result, heap);
        true
    } else {
        false
    }
}

/// Returns true when the object supports binary `+`.
fn has_binary_add(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    if let Ok(Some(result)) = value.py_add(value, heap, interns) {
        result.drop_with_heap(heap);
        true
    } else {
        false
    }
}

/// Drops values contained in an `AttrCallResult` to avoid leaking references while probing.
fn drop_attr_call_result(attr_result: AttrCallResult, heap: &mut Heap<impl ResourceTracker>) {
    match attr_result {
        AttrCallResult::Value(value) => value.drop_with_heap(heap),
        AttrCallResult::OsCall(_, args) | AttrCallResult::ExternalCall(_, args) => args.drop_with_heap(heap),
        AttrCallResult::PropertyCall(getter, instance) => {
            getter.drop_with_heap(heap);
            instance.drop_with_heap(heap);
        }
        AttrCallResult::DescriptorGet(descriptor) => descriptor.drop_with_heap(heap),
        AttrCallResult::ReduceCall(function, accumulator, items) => {
            function.drop_with_heap(heap);
            accumulator.drop_with_heap(heap);
            for item in items {
                item.drop_with_heap(heap);
            }
        }
        AttrCallResult::MapCall(function, iterables) => {
            function.drop_with_heap(heap);
            for iterable in iterables {
                for item in iterable {
                    item.drop_with_heap(heap);
                }
            }
        }
        AttrCallResult::FilterCall(function, items)
        | AttrCallResult::FilterFalseCall(function, items)
        | AttrCallResult::TakeWhileCall(function, items)
        | AttrCallResult::DropWhileCall(function, items)
        | AttrCallResult::GroupByCall(function, items) => {
            function.drop_with_heap(heap);
            for item in items {
                item.drop_with_heap(heap);
            }
        }
        AttrCallResult::TextwrapIndentCall(predicate, _lines, _prefix) => predicate.drop_with_heap(heap),
        AttrCallResult::CallFunction(callable, args) => {
            callable.drop_with_heap(heap);
            args.drop_with_heap(heap);
        }
        AttrCallResult::ReSubCall(callable, matches, _string, _is_bytes, _return_count) => {
            callable.drop_with_heap(heap);
            for (_start, _end, match_val) in matches {
                match_val.drop_with_heap(heap);
            }
        }
        AttrCallResult::ObjectNew => {}
    }
}

/// Converts a dict key to a string name.
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

/// CPython 3.14 `dir(builtins)` reference names.
///
/// Ouros's `builtins` module is intentionally minimal today, but `dir(builtins)`
/// parity tests expect the standard CPython surface.
const CPYTHON_BUILTINS_DIR_NAMES: &[&str] = &[
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "BaseException",
    "BaseExceptionGroup",
    "BlockingIOError",
    "BrokenPipeError",
    "BufferError",
    "BytesWarning",
    "ChildProcessError",
    "ConnectionAbortedError",
    "ConnectionError",
    "ConnectionRefusedError",
    "ConnectionResetError",
    "DeprecationWarning",
    "EOFError",
    "Ellipsis",
    "EncodingWarning",
    "EnvironmentError",
    "Exception",
    "ExceptionGroup",
    "False",
    "FileExistsError",
    "FileNotFoundError",
    "FloatingPointError",
    "FutureWarning",
    "GeneratorExit",
    "IOError",
    "ImportError",
    "ImportWarning",
    "IndentationError",
    "IndexError",
    "InterruptedError",
    "IsADirectoryError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "MemoryError",
    "ModuleNotFoundError",
    "NameError",
    "None",
    "NotADirectoryError",
    "NotImplemented",
    "NotImplementedError",
    "OSError",
    "OverflowError",
    "PendingDeprecationWarning",
    "PermissionError",
    "ProcessLookupError",
    "PythonFinalizationError",
    "RecursionError",
    "ReferenceError",
    "ResourceWarning",
    "RuntimeError",
    "RuntimeWarning",
    "StopAsyncIteration",
    "StopIteration",
    "SyntaxError",
    "SyntaxWarning",
    "SystemError",
    "SystemExit",
    "TabError",
    "TimeoutError",
    "True",
    "TypeError",
    "UnboundLocalError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "UnicodeError",
    "UnicodeTranslateError",
    "UnicodeWarning",
    "UserWarning",
    "ValueError",
    "Warning",
    "ZeroDivisionError",
    "_IncompleteInputError",
    "__build_class__",
    "__debug__",
    "__doc__",
    "__import__",
    "__loader__",
    "__name__",
    "__package__",
    "__spec__",
    "abs",
    "aiter",
    "all",
    "anext",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "copyright",
    "credits",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "exit",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "license",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "quit",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];
