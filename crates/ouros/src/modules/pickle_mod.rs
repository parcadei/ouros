//! Implementation of the `pickle` module.
//!
//! This provides a sandbox-safe pickle subset for common built-in values:
//! `None`, `bool`, `int`, `float`, `str`, `bytes`, `list`, `tuple`, `dict`,
//! `set`, and `frozenset`.
//!
//! Ouros serializes to an internal binary payload instead of CPython opcodes.
//! The API shape mirrors CPython (`dump`/`dumps`/`load`/`loads`) while keeping
//! deserialization fully inside the sandbox runtime.

use std::str::FromStr;

use ahash::AHashSet;
use num_bigint::BigInt;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, Bytes, ClassObject, Dict, FrozenSet, List, LongInt, Module, PyTrait, Set, Str, Type,
        allocate_tuple, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// Magic header for Ouros pickle payloads.
const PICKLE_MAGIC: &[u8; 8] = b"OUROPKL1";
/// Highest supported pickle protocol number.
pub(crate) const HIGHEST_PROTOCOL: i64 = 5;
/// Default pickle protocol number.
pub(crate) const DEFAULT_PROTOCOL: i64 = 5;

/// Pickle module function entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum PickleFunctions {
    Dump,
    Dumps,
    Load,
    Loads,
}

/// Serialized payload wrapper used by Ouros's pickle implementation.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PicklePayload {
    protocol: i64,
    value: PickleValue,
}

/// Pickle-compatible value graph for the supported subset.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum PickleValue {
    None,
    Bool(bool),
    Int(i64),
    BigInt(String),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<Self>),
    Tuple(Vec<Self>),
    Dict(Vec<(Self, Self)>),
    Set(Vec<Self>),
    FrozenSet(Vec<Self>),
}

/// Creates the `pickle` module and registers API attributes.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Pickle);
    module.set_attr(
        StaticStrings::Dump,
        Value::ModuleFunction(ModuleFunctions::Pickle(PickleFunctions::Dump)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Dumps,
        Value::ModuleFunction(ModuleFunctions::Pickle(PickleFunctions::Dumps)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Load,
        Value::ModuleFunction(ModuleFunctions::Pickle(PickleFunctions::Load)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Loads,
        Value::ModuleFunction(ModuleFunctions::Pickle(PickleFunctions::Loads)),
        heap,
        interns,
    );
    module.set_attr_str("HIGHEST_PROTOCOL", Value::Int(HIGHEST_PROTOCOL), heap, interns)?;
    module.set_attr_str("DEFAULT_PROTOCOL", Value::Int(DEFAULT_PROTOCOL), heap, interns)?;

    let pickling_error_id = create_pickle_exception_class(heap, interns, "PicklingError")?;
    module.set_attr_str("PicklingError", Value::Ref(pickling_error_id), heap, interns)?;

    let unpickling_error_id = create_pickle_exception_class(heap, interns, "UnpicklingError")?;
    module.set_attr_str("UnpicklingError", Value::Ref(unpickling_error_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches pickle module function calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: PickleFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        PickleFunctions::Dump => dump(heap, interns, args),
        PickleFunctions::Dumps => dumps(heap, interns, args),
        PickleFunctions::Load => load(heap, interns, args),
        PickleFunctions::Loads => loads(heap, interns, args),
    }
}

/// Serializes one value to pickle bytes with the requested protocol.
pub(crate) fn serialize_pickle_value(
    value: &Value,
    protocol: i64,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    let mut visited = AHashSet::new();
    let pickled = value_to_pickle(value, heap, interns, &mut visited)?;
    let payload = PicklePayload {
        protocol,
        value: pickled,
    };
    let encoded = postcard::to_allocvec(&payload)
        .map_err(|error| pickling_error(format!("failed to serialize pickle payload: {error}")))?;
    let mut out = Vec::with_capacity(PICKLE_MAGIC.len() + encoded.len());
    out.extend_from_slice(PICKLE_MAGIC);
    out.extend_from_slice(&encoded);
    Ok(out)
}

/// Deserializes one value from pickle bytes.
pub(crate) fn deserialize_pickle_value(
    data: &[u8],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    if data.len() < PICKLE_MAGIC.len() || data[..PICKLE_MAGIC.len()] != *PICKLE_MAGIC {
        return Err(unpickling_error("invalid load key, not an Ouros pickle payload"));
    }
    let payload: PicklePayload = postcard::from_bytes(&data[PICKLE_MAGIC.len()..])
        .map_err(|error| unpickling_error(format!("pickle data was truncated or corrupted: {error}")))?;
    pickle_to_value(payload.value, heap, interns)
}

/// Implements `pickle.dumps(obj, protocol=None)`.
fn dumps(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (obj, protocol_arg) = args.get_one_two_args_with_keyword("pickle.dumps", "protocol", heap, interns)?;
    defer_drop!(obj, heap);
    let protocol = resolve_protocol("pickle.dumps", protocol_arg, heap)?;
    let serialized = serialize_pickle_value(obj, protocol, heap, interns)?;
    let bytes_id = heap.allocate(HeapData::Bytes(Bytes::from(serialized)))?;
    Ok(AttrCallResult::Value(Value::Ref(bytes_id)))
}

/// Implements `pickle.dump(obj, file, protocol=None)`.
fn dump(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(obj) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("pickle.dump", 2, 0));
    };
    let Some(file_obj) = positional.next() else {
        obj.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("pickle.dump", 2, 1));
    };
    let mut protocol_arg = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        obj.drop_with_heap(heap);
        file_obj.drop_with_heap(heap);
        protocol_arg.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("pickle.dump", 3, 4));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            file_obj.drop_with_heap(heap);
            protocol_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        if key_name != "protocol" {
            value.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            file_obj.drop_with_heap(heap);
            protocol_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("pickle.dump", &key_name));
        }
        if protocol_arg.is_some() {
            value.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            file_obj.drop_with_heap(heap);
            protocol_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_multiple_values("pickle.dump", "protocol"));
        }
        protocol_arg = Some(value);
    }

    let protocol = match resolve_protocol("pickle.dump", protocol_arg, heap) {
        Ok(protocol) => protocol,
        Err(err) => {
            obj.drop_with_heap(heap);
            file_obj.drop_with_heap(heap);
            return Err(err);
        }
    };

    let serialized = match serialize_pickle_value(&obj, protocol, heap, interns) {
        Ok(serialized) => serialized,
        Err(err) => {
            obj.drop_with_heap(heap);
            file_obj.drop_with_heap(heap);
            return Err(err);
        }
    };
    obj.drop_with_heap(heap);

    write_pickle_to_file_like(&file_obj, &serialized, heap, interns)?;
    file_obj.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `pickle.loads(data)`.
fn loads(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let data = args.get_one_arg("pickle.loads", heap)?;
    let bytes = if let Some(bytes) = value_to_bytes(&data, heap, interns) {
        bytes.to_vec()
    } else {
        data.drop_with_heap(heap);
        return Err(ExcType::type_error("a bytes-like object is required, not 'str'"));
    };
    data.drop_with_heap(heap);
    let value = deserialize_pickle_value(&bytes, heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Implements `pickle.load(file)`.
fn load(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let file_obj = args.get_one_arg("pickle.load", heap)?;
    let payload = read_pickle_from_file_like(&file_obj, heap, interns)?;
    file_obj.drop_with_heap(heap);
    let value = deserialize_pickle_value(&payload, heap, interns)?;
    Ok(AttrCallResult::Value(value))
}

/// Resolves and validates the optional protocol argument.
fn resolve_protocol(
    function_name: &str,
    protocol_arg: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<i64> {
    let Some(protocol_value) = protocol_arg else {
        return Ok(DEFAULT_PROTOCOL);
    };
    let protocol = if matches!(protocol_value, Value::None) {
        DEFAULT_PROTOCOL
    } else {
        protocol_value.as_int(heap)?
    };
    protocol_value.drop_with_heap(heap);

    if protocol < 0 {
        return Ok(HIGHEST_PROTOCOL);
    }
    if protocol > HIGHEST_PROTOCOL {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("{function_name}() protocol must be <= {HIGHEST_PROTOCOL}, got {protocol}"),
        )
        .into());
    }
    Ok(protocol)
}

/// Converts one runtime value into the supported pickle graph.
fn value_to_pickle(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    visited: &mut AHashSet<HeapId>,
) -> RunResult<PickleValue> {
    match value {
        Value::None => Ok(PickleValue::None),
        Value::Bool(v) => Ok(PickleValue::Bool(*v)),
        Value::Int(v) => Ok(PickleValue::Int(*v)),
        Value::InternLongInt(id) => Ok(PickleValue::BigInt(interns.get_long_int(*id).to_string())),
        Value::Float(v) => Ok(PickleValue::Float(*v)),
        Value::InternString(id) => Ok(PickleValue::Str(interns.get_str(*id).to_owned())),
        Value::InternBytes(id) => Ok(PickleValue::Bytes(interns.get_bytes(*id).to_vec())),
        Value::Ref(id) => value_ref_to_pickle(*id, heap, interns, visited),
        other => Err(pickling_error(format!(
            "cannot pickle '{}' object",
            other.py_type(heap)
        ))),
    }
}

/// Converts one heap-backed runtime value into the supported pickle graph.
fn value_ref_to_pickle(
    value_id: HeapId,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    visited: &mut AHashSet<HeapId>,
) -> RunResult<PickleValue> {
    match heap.get(value_id) {
        HeapData::Str(s) => Ok(PickleValue::Str(s.as_str().to_owned())),
        HeapData::Bytes(b) | HeapData::Bytearray(b) => Ok(PickleValue::Bytes(b.as_slice().to_vec())),
        HeapData::LongInt(long_int) => Ok(PickleValue::BigInt(long_int.to_string())),
        HeapData::List(_) | HeapData::Tuple(_) | HeapData::Dict(_) | HeapData::Set(_) | HeapData::FrozenSet(_) => {
            if !visited.insert(value_id) {
                return Err(pickling_error("cannot pickle recursive objects"));
            }
            let result = match heap.get(value_id) {
                HeapData::List(list) => {
                    let mut items = Vec::with_capacity(list.len());
                    for item in list.as_vec() {
                        items.push(value_to_pickle(item, heap, interns, visited)?);
                    }
                    Ok(PickleValue::List(items))
                }
                HeapData::Tuple(tuple) => {
                    let mut items = Vec::with_capacity(tuple.as_vec().len());
                    for item in tuple.as_vec() {
                        items.push(value_to_pickle(item, heap, interns, visited)?);
                    }
                    Ok(PickleValue::Tuple(items))
                }
                HeapData::Dict(dict) => {
                    let mut items = Vec::with_capacity(dict.len());
                    for (key, value) in dict {
                        let key = value_to_pickle(key, heap, interns, visited)?;
                        let value = value_to_pickle(value, heap, interns, visited)?;
                        items.push((key, value));
                    }
                    Ok(PickleValue::Dict(items))
                }
                HeapData::Set(set) => {
                    let mut items = Vec::with_capacity(set.len());
                    for item in set.storage().iter() {
                        items.push(value_to_pickle(item, heap, interns, visited)?);
                    }
                    Ok(PickleValue::Set(items))
                }
                HeapData::FrozenSet(set) => {
                    let mut items = Vec::with_capacity(set.len());
                    for item in set.storage().iter() {
                        items.push(value_to_pickle(item, heap, interns, visited)?);
                    }
                    Ok(PickleValue::FrozenSet(items))
                }
                _ => unreachable!("container type checked above"),
            };
            visited.remove(&value_id);
            result
        }
        other => Err(pickling_error(format!(
            "cannot pickle '{}' object",
            other.py_type(heap)
        ))),
    }
}

/// Converts one pickled graph value back into a runtime value.
fn pickle_to_value(value: PickleValue, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    match value {
        PickleValue::None => Ok(Value::None),
        PickleValue::Bool(v) => Ok(Value::Bool(v)),
        PickleValue::Int(v) => Ok(Value::Int(v)),
        PickleValue::BigInt(v) => {
            let bigint =
                BigInt::from_str(&v).map_err(|_| unpickling_error("invalid bigint literal in pickle payload"))?;
            Ok(LongInt::new(bigint).into_value(heap)?)
        }
        PickleValue::Float(v) => Ok(Value::Float(v)),
        PickleValue::Str(v) => {
            let id = heap.allocate(HeapData::Str(Str::from(v)))?;
            Ok(Value::Ref(id))
        }
        PickleValue::Bytes(v) => {
            let id = heap.allocate(HeapData::Bytes(Bytes::from(v)))?;
            Ok(Value::Ref(id))
        }
        PickleValue::List(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(pickle_to_value(item, heap, interns)?);
            }
            let list_id = heap.allocate(HeapData::List(List::new(values)))?;
            Ok(Value::Ref(list_id))
        }
        PickleValue::Tuple(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(pickle_to_value(item, heap, interns)?);
            }
            Ok(allocate_tuple(values.into(), heap)?)
        }
        PickleValue::Dict(items) => {
            let mut dict = Dict::new();
            for (key, value) in items {
                let key_value = pickle_to_value(key, heap, interns)?;
                let value_value = pickle_to_value(value, heap, interns)?;
                let replaced = dict
                    .set(key_value, value_value, heap, interns)
                    .map_err(|_| unpickling_error("unhashable dict key in pickle payload"))?;
                if let Some(replaced) = replaced {
                    replaced.drop_with_heap(heap);
                }
            }
            let dict_id = heap.allocate(HeapData::Dict(dict))?;
            Ok(Value::Ref(dict_id))
        }
        PickleValue::Set(items) => {
            let mut set = Set::new();
            for item in items {
                let value = pickle_to_value(item, heap, interns)?;
                set.add(value, heap, interns)
                    .map_err(|_| unpickling_error("unhashable set element in pickle payload"))?;
            }
            let set_id = heap.allocate(HeapData::Set(set))?;
            Ok(Value::Ref(set_id))
        }
        PickleValue::FrozenSet(items) => {
            let mut set = Set::new();
            for item in items {
                let value = pickle_to_value(item, heap, interns)?;
                set.add(value, heap, interns)
                    .map_err(|_| unpickling_error("unhashable frozenset element in pickle payload"))?;
            }
            let frozen_id = heap.allocate(HeapData::FrozenSet(FrozenSet::from_set(set)))?;
            Ok(Value::Ref(frozen_id))
        }
    }
}

/// Reads bytes from `file_obj.read()` for `pickle.load`.
fn read_pickle_from_file_like(
    file_obj: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    let Value::Ref(file_id) = file_obj else {
        return Err(ExcType::attribute_error(file_obj.py_type(heap), "read"));
    };
    let result = heap.call_attr_raw(*file_id, &EitherStr::Heap("read".to_owned()), ArgValues::Empty, interns)?;
    match result {
        AttrCallResult::Value(value) => {
            let bytes = if let Some(bytes) = value_to_bytes(&value, heap, interns) {
                bytes.to_vec()
            } else {
                value.drop_with_heap(heap);
                return Err(unpickling_error("pickle.load() expected fp.read() to return bytes"));
            };
            value.drop_with_heap(heap);
            Ok(bytes)
        }
        other => {
            super::json::drop_non_value_attr_result(other, heap);
            Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "pickle.load() expected fp.read() to return a value immediately".to_string(),
            )
            .into())
        }
    }
}

/// Writes bytes to `file_obj.write(...)` for `pickle.dump`.
fn write_pickle_to_file_like(
    file_obj: &Value,
    data: &[u8],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let Value::Ref(file_id) = file_obj else {
        return Err(ExcType::attribute_error(file_obj.py_type(heap), "write"));
    };
    let bytes_id = heap.allocate(HeapData::Bytes(Bytes::from(data.to_vec())))?;
    let result = heap.call_attr_raw(
        *file_id,
        &EitherStr::Heap("write".to_owned()),
        ArgValues::One(Value::Ref(bytes_id)),
        interns,
    )?;
    match result {
        AttrCallResult::Value(value) => {
            value.drop_with_heap(heap);
            Ok(())
        }
        other => {
            super::json::drop_non_value_attr_result(other, heap);
            Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "pickle.dump() expected fp.write() to return a value immediately".to_string(),
            )
            .into())
        }
    }
}

/// Returns a bytes-like slice for interned or heap-backed bytes objects.
fn value_to_bytes<'a>(
    value: &'a Value,
    heap: &'a Heap<impl ResourceTracker>,
    interns: &'a Interns,
) -> Option<&'a [u8]> {
    match value {
        Value::InternBytes(id) => Some(interns.get_bytes(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => Some(bytes.as_slice()),
            _ => None,
        },
        _ => None,
    }
}

/// Creates a module-scoped exception class deriving from `Exception`.
fn create_pickle_exception_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    class_name: &str,
) -> Result<HeapId, ResourceError> {
    let exception_class_id = heap.builtin_class_id(Type::Exception(ExcType::Exception))?;
    heap.inc_ref(exception_class_id);

    let class_uid = heap.next_class_uid();
    let class = ClassObject::new(
        EitherStr::Heap(format!("pickle.{class_name}")),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        vec![exception_class_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class))?;

    let mro =
        compute_c3_mro(class_id, &[exception_class_id], heap, interns).expect("pickle exception class should have MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(class_obj) = heap.get_mut(class_id) {
        class_obj.set_mro(mro);
    }

    heap.with_entry_mut(exception_class_id, |_, data| {
        let HeapData::ClassObject(base_cls) = data else {
            return Err(ExcType::type_error("pickle base exception is not a class".to_string()));
        };
        base_cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("pickle exception class base mutation should succeed");

    Ok(class_id)
}

/// Creates a `PicklingError`-style runtime error.
fn pickling_error(message: impl Into<String>) -> crate::exception_private::RunError {
    let class_name = "pickle.PicklingError".to_owned();
    let mro = vec![
        class_name.clone(),
        "Exception".to_owned(),
        "BaseException".to_owned(),
        "object".to_owned(),
    ];
    SimpleException::new_msg(ExcType::Exception, message.into())
        .with_custom_metadata(class_name, mro, Vec::new())
        .into()
}

/// Creates an `UnpicklingError`-style runtime error.
fn unpickling_error(message: impl Into<String>) -> crate::exception_private::RunError {
    let class_name = "pickle.UnpicklingError".to_owned();
    let mro = vec![
        class_name.clone(),
        "Exception".to_owned(),
        "BaseException".to_owned(),
        "object".to_owned(),
    ];
    SimpleException::new_msg(ExcType::Exception, message.into())
        .with_custom_metadata(class_name, mro, Vec::new())
        .into()
}
