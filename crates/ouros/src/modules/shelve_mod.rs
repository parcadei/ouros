//! Implementation of the `shelve` module.
//!
//! Ouros has no filesystem access, so this module provides an in-memory shelve
//! backend keyed by filename while preserving the `shelve.open()` API shape.
//! Values are serialized via the local `pickle` module implementation.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use indexmap::IndexMap;

use super::pickle_mod::{DEFAULT_PROTOCOL, deserialize_pickle_value, serialize_pickle_value};
use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, ClassObject, Dict, Instance, List, Module, PyTrait, Type, allocate_tuple, compute_c3_mro},
    value::{EitherStr, Value},
};

/// Function and method entry points exposed by the `shelve` module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ShelveFunctions {
    Open,
    #[strum(serialize = "__getitem__")]
    ShelfGetitem,
    #[strum(serialize = "__setitem__")]
    ShelfSetitem,
    #[strum(serialize = "__delitem__")]
    ShelfDelitem,
    #[strum(serialize = "__contains__")]
    ShelfContains,
    #[strum(serialize = "keys")]
    ShelfKeys,
    #[strum(serialize = "values")]
    ShelfValues,
    #[strum(serialize = "items")]
    ShelfItems,
    #[strum(serialize = "close")]
    ShelfClose,
    #[strum(serialize = "sync")]
    ShelfSync,
}

/// Name of the per-instance attribute storing the logical shelf filename.
const SHELF_FILENAME_ATTR: &str = "_ouros_filename";
/// Name of the per-instance attribute storing close state.
const SHELF_CLOSED_ATTR: &str = "_ouros_closed";

/// Process-local in-memory shelve backing store.
static SHELVE_DATA: OnceLock<Mutex<HashMap<String, IndexMap<String, Vec<u8>>>>> = OnceLock::new();

/// Creates the `shelve` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Shelve);
    module.set_attr_str(
        "open",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::Open)),
        heap,
        interns,
    )?;

    let shelf_class_id = create_shelf_class(heap, interns)?;
    module.set_attr_str("Shelf", Value::Ref(shelf_class_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches `shelve` module and `Shelf` method calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ShelveFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        ShelveFunctions::Open => shelve_open(heap, interns, args)?,
        ShelveFunctions::ShelfGetitem => shelf_getitem(heap, interns, args)?,
        ShelveFunctions::ShelfSetitem => shelf_setitem(heap, interns, args)?,
        ShelveFunctions::ShelfDelitem => shelf_delitem(heap, interns, args)?,
        ShelveFunctions::ShelfContains => shelf_contains(heap, interns, args)?,
        ShelveFunctions::ShelfKeys => shelf_keys(heap, interns, args)?,
        ShelveFunctions::ShelfValues => shelf_values(heap, interns, args)?,
        ShelveFunctions::ShelfItems => shelf_items(heap, interns, args)?,
        ShelveFunctions::ShelfClose => shelf_close(heap, interns, args)?,
        ShelveFunctions::ShelfSync => shelf_sync(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Implements `shelve.open(filename, flag='c', protocol=None, writeback=False)`.
fn shelve_open(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("shelve.open", 1, 0));
    }
    if positional.len() > 4 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("shelve.open", 4, count));
    }

    let filename_value = positional.remove(0);
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            filename_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "flag" | "protocol" | "writeback" => value.drop_with_heap(heap),
            _ => {
                value.drop_with_heap(heap);
                filename_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("shelve.open", &key_name));
            }
        }
    }

    let filename = value_to_required_str(filename_value, heap, interns, "shelve.open() filename")?;

    {
        let mut shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves.entry(filename.clone()).or_default();
    }

    let class_id = create_shelf_class(heap, interns)?;
    let instance_id = create_instance_for_class(class_id, heap)?;

    let filename_id = heap.allocate(HeapData::Str(crate::types::Str::from(filename)))?;
    set_instance_attr_by_name(instance_id, SHELF_FILENAME_ATTR, Value::Ref(filename_id), heap, interns)?;
    set_instance_attr_by_name(instance_id, SHELF_CLOSED_ATTR, Value::Bool(false), heap, interns)?;

    Ok(Value::Ref(instance_id))
}

/// Implements `Shelf.__getitem__(self, key)`.
fn shelf_getitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, key_value) = args.get_two_args("Shelf.__getitem__", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.__getitem__")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let key = value_to_required_str(key_value, heap, interns, "Shelf key")?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let encoded = {
        let shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves.get(&filename).and_then(|shelf| shelf.get(&key)).cloned()
    };

    let Some(encoded) = encoded else {
        return Err(SimpleException::new_msg(ExcType::KeyError, format!("{key:?}")).into());
    };

    deserialize_pickle_value(&encoded, heap, interns)
}

/// Implements `Shelf.__setitem__(self, key, value)`.
fn shelf_setitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, key_value, value) = args.get_three_args("Shelf.__setitem__", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.__setitem__")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let key = value_to_required_str(key_value, heap, interns, "Shelf key")?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let encoded = match serialize_pickle_value(&value, DEFAULT_PROTOCOL, heap, interns) {
        Ok(encoded) => encoded,
        Err(err) => {
            value.drop_with_heap(heap);
            return Err(err);
        }
    };
    value.drop_with_heap(heap);

    let mut shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
    shelves.entry(filename).or_default().insert(key, encoded);
    Ok(Value::None)
}

/// Implements `Shelf.__delitem__(self, key)`.
fn shelf_delitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, key_value) = args.get_two_args("Shelf.__delitem__", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.__delitem__")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let key = value_to_required_str(key_value, heap, interns, "Shelf key")?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let removed = {
        let mut shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves.entry(filename).or_default().shift_remove(&key).is_some()
    };

    if !removed {
        return Err(SimpleException::new_msg(ExcType::KeyError, format!("{key:?}")).into());
    }
    Ok(Value::None)
}

/// Implements `Shelf.__contains__(self, key)`.
fn shelf_contains(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, key_value) = args.get_two_args("Shelf.__contains__", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.__contains__")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let key = value_to_required_str(key_value, heap, interns, "Shelf key")?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let contains = {
        let shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves.get(&filename).is_some_and(|shelf| shelf.contains_key(&key))
    };
    Ok(Value::Bool(contains))
}

/// Implements `Shelf.keys(self)`.
fn shelf_keys(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("Shelf.keys", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.keys")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let keys: Vec<String> = {
        let shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves
            .get(&filename)
            .map(|shelf| shelf.keys().cloned().collect())
            .unwrap_or_default()
    };

    let mut values = Vec::with_capacity(keys.len());
    for key in keys {
        let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(key)))?;
        values.push(Value::Ref(key_id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(values)))?;
    Ok(Value::Ref(list_id))
}

/// Implements `Shelf.values(self)`.
fn shelf_values(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("Shelf.values", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.values")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let encoded_values: Vec<Vec<u8>> = {
        let shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves
            .get(&filename)
            .map(|shelf| shelf.values().cloned().collect())
            .unwrap_or_default()
    };

    let mut values = Vec::with_capacity(encoded_values.len());
    for encoded in encoded_values {
        values.push(deserialize_pickle_value(&encoded, heap, interns)?);
    }
    let list_id = heap.allocate(HeapData::List(List::new(values)))?;
    Ok(Value::Ref(list_id))
}

/// Implements `Shelf.items(self)`.
fn shelf_items(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("Shelf.items", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.items")?;
    ensure_shelf_open(self_id, heap, interns)?;
    let filename = shelf_filename(self_id, heap, interns)?;

    let entries: Vec<(String, Vec<u8>)> = {
        let shelves = shelve_data_slot().lock().expect("shelve data mutex poisoned");
        shelves
            .get(&filename)
            .map(|shelf| shelf.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
            .unwrap_or_default()
    };

    let mut items = Vec::with_capacity(entries.len());
    for (key, encoded) in entries {
        let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(key)))?;
        let value = deserialize_pickle_value(&encoded, heap, interns)?;
        let item = allocate_tuple(vec![Value::Ref(key_id), value].into(), heap)?;
        items.push(item);
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Implements `Shelf.close(self)`.
fn shelf_close(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("Shelf.close", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.close")?;
    set_instance_attr_by_name(self_id, SHELF_CLOSED_ATTR, Value::Bool(true), heap, interns)?;
    Ok(Value::None)
}

/// Implements `Shelf.sync(self)`.
fn shelf_sync(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("Shelf.sync", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_shelf_instance(self_value, heap, "Shelf.sync")?;
    ensure_shelf_open(self_id, heap, interns)?;
    Ok(Value::None)
}

/// Creates the runtime `Shelf` class object.
fn create_shelf_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let object_class_id = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class_id);

    let mut namespace = Dict::new();
    dict_set_str_key(
        &mut namespace,
        "__getitem__",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfGetitem)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__setitem__",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfSetitem)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__delitem__",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfDelitem)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__contains__",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfContains)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "keys",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfKeys)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "values",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfValues)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "items",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfItems)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "close",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfClose)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "sync",
        Value::ModuleFunction(ModuleFunctions::Shelve(ShelveFunctions::ShelfSync)),
        heap,
        interns,
    )?;

    let module_name_id = heap.allocate(HeapData::Str(crate::types::Str::from("shelve")))?;
    dict_set_str_key(&mut namespace, "__module__", Value::Ref(module_name_id), heap, interns)?;

    let class_uid = heap.next_class_uid();
    let class = ClassObject::new(
        EitherStr::Heap("Shelf".to_owned()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        namespace,
        vec![object_class_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class))?;

    let mro = compute_c3_mro(class_id, &[object_class_id], heap, interns)
        .expect("shelve.Shelf class should always have valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    heap.with_entry_mut(object_class_id, |_, data| {
        let HeapData::ClassObject(base_cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        base_cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("shelve.Shelf base class mutation should succeed");

    Ok(class_id)
}

/// Creates one instance for a previously allocated class id.
fn create_instance_for_class(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let attrs_id = Some(heap.allocate(HeapData::Dict(Dict::new()))?);
    heap.inc_ref(class_id);
    Ok(heap.allocate(HeapData::Instance(Instance::new(
        class_id,
        attrs_id,
        Vec::new(),
        Vec::new(),
    )))?)
}

/// Returns the global in-memory shelve storage map.
fn shelve_data_slot() -> &'static Mutex<HashMap<String, IndexMap<String, Vec<u8>>>> {
    SHELVE_DATA.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Validates and extracts `self` as a `Shelf` instance id.
fn expect_shelf_instance(
    self_value: &Value,
    heap: &Heap<impl ResourceTracker>,
    method_name: &str,
) -> RunResult<HeapId> {
    match self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => Ok(*id),
        _ => Err(ExcType::type_error(format!("{method_name} expected instance"))),
    }
}

/// Raises `ValueError` when a shelf instance is already closed.
fn ensure_shelf_open(instance_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
    let Some(closed_value) = get_instance_attr_by_name(instance_id, SHELF_CLOSED_ATTR, heap, interns) else {
        return Ok(());
    };
    let is_closed = matches!(closed_value, Value::Bool(true));
    closed_value.drop_with_heap(heap);
    if is_closed {
        return Err(SimpleException::new_msg(ExcType::ValueError, "invalid operation on closed shelf").into());
    }
    Ok(())
}

/// Fetches the persisted filename associated with a shelf instance.
fn shelf_filename(instance_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let Some(filename_value) = get_instance_attr_by_name(instance_id, SHELF_FILENAME_ATTR, heap, interns) else {
        return Err(SimpleException::new_msg(ExcType::RuntimeError, "shelf filename state is missing").into());
    };
    value_to_required_str(filename_value, heap, interns, "Shelf filename")
}

/// Converts a value to an owned `str`, requiring the runtime value to be exactly `str`.
fn value_to_required_str(
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    what: &str,
) -> RunResult<String> {
    let out = match &value {
        Value::InternString(id) => Some(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_owned()),
            _ => None,
        },
        _ => None,
    };

    let Some(out) = out else {
        let got_type = value.py_type(heap);
        value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("{what} must be str, not {got_type}")));
    };

    value.drop_with_heap(heap);
    Ok(out)
}

/// Sets one string-keyed instance attribute and drops any replaced value.
fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(name)))?;
    heap.with_entry_mut(instance_id, |heap_inner, data| -> RunResult<()> {
        let HeapData::Instance(instance) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("shelve expected instance"));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

/// Returns a cloned string-keyed instance attribute.
fn get_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };
    instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str(name, heap, interns))
        .map(|value| value.clone_with_heap(heap))
}

/// Sets one string key in a dict and drops any replaced value.
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
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}
