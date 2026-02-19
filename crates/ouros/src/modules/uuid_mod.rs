//! Implementation of the `uuid` module.
//!
//! This module implements the CPython-facing `uuid` API surface used by parity
//! tests, including:
//! - `UUID` constructor/type
//! - `SafeUUID` enum members
//! - `uuid1`, `uuid3`, `uuid4`, `uuid5`, `uuid6`, `uuid7`, `uuid8`
//! - `getnode`
//! - namespace and boundary constants

use std::{
    process::Command,
    sync::{LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use md5::{Digest, Md5};
use rand::{RngCore, rngs::OsRng};
use sha1::Sha1;
use uuid::Uuid as RustUuid;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Module, PyTrait, SafeUuidKind, Type, Uuid},
    value::Value,
};

/// UUID module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum UuidFunctions {
    /// Generate a random UUID v4.
    Uuid4,
    /// Generate a time-based UUID v1.
    Uuid1,
    /// Generate a name-based UUID v3 (MD5).
    Uuid3,
    /// Generate a name-based UUID v5 (SHA-1).
    Uuid5,
    /// Generate a time-based UUID v6.
    Uuid6,
    /// Generate a Unix timestamp UUID v7.
    Uuid7,
    /// Generate a custom UUID v8.
    Uuid8,
    /// Return the hardware node value.
    Getnode,
}

/// Number of 100-ns intervals between UUID epoch and Unix epoch.
const UUID_EPOCH_OFFSET_100NS: u128 = 0x01b2_1dd2_1381_4000;
/// Mask used to clear RFC 4122 version and variant bits.
const RFC_4122_CLEARFLAGS_MASK: u128 = !((0xf000u128 << 64) | (0xc000u128 << 48));
/// RFC 4122 version/variant flag for UUIDv3.
const RFC_4122_VERSION_3_FLAGS: u128 = (3u128 << 76) | (0x8000u128 << 48);
/// RFC 4122 version/variant flag for UUIDv5.
const RFC_4122_VERSION_5_FLAGS: u128 = (5u128 << 76) | (0x8000u128 << 48);
/// RFC 4122 version/variant flag for UUIDv6.
const RFC_4122_VERSION_6_FLAGS: u128 = (6u128 << 76) | (0x8000u128 << 48);
/// RFC 4122 version/variant flag for UUIDv7.
const RFC_4122_VERSION_7_FLAGS: u128 = (7u128 << 76) | (0x8000u128 << 48);
/// RFC 4122 version/variant flag for UUIDv8.
const RFC_4122_VERSION_8_FLAGS: u128 = (8u128 << 76) | (0x8000u128 << 48);
/// RFC 4122 version/variant flag for UUIDv1.
const RFC_4122_VERSION_1_FLAGS: u128 = (1u128 << 76) | (0x8000u128 << 48);

/// Shared state for UUIDv6 timestamp monotonicity.
struct Uuid6State {
    /// Last generated timestamp in 100-ns intervals since UUID epoch.
    last_timestamp: Option<u64>,
}

/// Shared state for UUIDv7 timestamp and counter monotonicity.
struct Uuid7State {
    /// Last generated Unix timestamp in milliseconds.
    last_timestamp_ms: Option<u64>,
    /// Last generated 42-bit counter.
    last_counter: u64,
}

/// Global UUIDv6 monotonicity state.
static UUID6_STATE: LazyLock<Mutex<Uuid6State>> = LazyLock::new(|| Mutex::new(Uuid6State { last_timestamp: None }));

/// Global UUIDv7 monotonicity state.
static UUID7_STATE: LazyLock<Mutex<Uuid7State>> = LazyLock::new(|| {
    Mutex::new(Uuid7State {
        last_timestamp_ms: None,
        last_counter: 0,
    })
});

/// Process-wide node value used by `getnode()` and default `uuid1/uuid6` node fields.
static UUID_NODE: LazyLock<u64> = LazyLock::new(|| {
    if let Some(node) = detect_system_node() {
        return node;
    }
    let fallback = random_bits(48) & 0xffff_ffff_ffff;
    if fallback == 0 { 1 } else { fallback }
});

/// Creates the `uuid` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Uuid);

    module.set_attr(
        StaticStrings::UuidClass,
        Value::Builtin(Builtins::Type(Type::Uuid)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidSafeClass,
        Value::Builtin(Builtins::Type(Type::SafeUuid)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::UuidUuid4,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid4)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidUuid1,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid1)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidUuid3,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid3)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidUuid5,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid5)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidUuid6,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid6)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidUuid7,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid7)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidUuid8,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Uuid8)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidGetnode,
        Value::ModuleFunction(ModuleFunctions::Uuid(UuidFunctions::Getnode)),
        heap,
        interns,
    );

    // Namespace UUID constants.
    module.set_attr(
        StaticStrings::UuidNamespaceDns,
        allocate_uuid_const(heap, "6ba7b810-9dad-11d1-80b4-00c04fd430c8")?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidNamespaceUrl,
        allocate_uuid_const(heap, "6ba7b811-9dad-11d1-80b4-00c04fd430c8")?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidNamespaceOid,
        allocate_uuid_const(heap, "6ba7b812-9dad-11d1-80b4-00c04fd430c8")?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidNamespaceX500,
        allocate_uuid_const(heap, "6ba7b814-9dad-11d1-80b4-00c04fd430c8")?,
        heap,
        interns,
    );

    // Boundary UUID constants.
    module.set_attr(
        StaticStrings::UuidNil,
        allocate_uuid_const(heap, "00000000-0000-0000-0000-000000000000")?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidMax,
        allocate_uuid_const(heap, "ffffffff-ffff-ffff-ffff-ffffffffffff")?,
        heap,
        interns,
    );

    // UUID variant constants.
    module.set_attr(
        StaticStrings::UuidReservedNcs,
        allocate_string(heap, crate::types::uuid::VARIANT_RESERVED_NCS)?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidRfc4122,
        allocate_string(heap, crate::types::uuid::VARIANT_RFC_4122)?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidReservedMicrosoft,
        allocate_string(heap, crate::types::uuid::VARIANT_RESERVED_MICROSOFT)?,
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::UuidReservedFuture,
        allocate_string(heap, crate::types::uuid::VARIANT_RESERVED_FUTURE)?,
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a uuid module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: UuidFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        UuidFunctions::Uuid4 => uuid4(heap, args),
        UuidFunctions::Uuid1 => uuid1(heap, interns, args),
        UuidFunctions::Uuid3 => uuid3(heap, interns, args),
        UuidFunctions::Uuid5 => uuid5(heap, interns, args),
        UuidFunctions::Uuid6 => uuid6(heap, interns, args),
        UuidFunctions::Uuid7 => uuid7(heap, args),
        UuidFunctions::Uuid8 => uuid8(heap, interns, args),
        UuidFunctions::Getnode => getnode(heap, args),
    }
}

/// Implementation of `uuid.uuid4()`.
fn uuid4(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("uuid.uuid4", heap)?;
    uuid_from_u128(heap, RustUuid::new_v4().as_u128(), SafeUuidKind::Unknown)
}

/// Implementation of `uuid.uuid1([node[, clock_seq]])`.
fn uuid1(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let parsed = parse_optional_int_args(heap, interns, args, "uuid.uuid1", &["node", "clock_seq"])?;

    let node_bits = parsed[0].map_or(*UUID_NODE, i64::cast_unsigned) & 0xffff_ffff_ffff;
    let clock_seq_bits = parsed[1].map_or_else(|| random_bits(14), i64::cast_unsigned) & 0x3fff;

    let timestamp = uuid6_timestamp_100ns()?;
    let time_low = timestamp & 0xffff_ffff;
    let time_mid = (timestamp >> 32) & 0xffff;
    let time_hi = (timestamp >> 48) & 0x0fff;

    let mut int_uuid = u128::from(time_low) << 96;
    int_uuid |= u128::from(time_mid) << 80;
    int_uuid |= u128::from(time_hi) << 64;
    int_uuid |= u128::from(clock_seq_bits) << 48;
    int_uuid |= u128::from(node_bits);
    int_uuid |= RFC_4122_VERSION_1_FLAGS;

    uuid_from_u128(heap, int_uuid, SafeUuidKind::Unknown)
}

/// Implementation of `uuid.uuid3(namespace, name)`.
fn uuid3(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (namespace_val, name_val) = args.get_two_args("uuid.uuid3", heap)?;
    defer_drop!(namespace_val, heap);
    defer_drop!(name_val, heap);

    let namespace = extract_uuid_namespace(namespace_val, heap, interns, "uuid.uuid3")?;
    let name_bytes = extract_uuid_name_bytes(name_val, heap, interns, "uuid.uuid3")?;

    let mut hasher = Md5::new();
    hasher.update(namespace.as_bytes());
    hasher.update(&name_bytes);
    let digest = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest);
    let mut int_uuid = u128::from_be_bytes(bytes);
    int_uuid &= RFC_4122_CLEARFLAGS_MASK;
    int_uuid |= RFC_4122_VERSION_3_FLAGS;

    uuid_from_u128(heap, int_uuid, SafeUuidKind::Unknown)
}

/// Implementation of `uuid.uuid5(namespace, name)`.
fn uuid5(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (namespace_val, name_val) = args.get_two_args("uuid.uuid5", heap)?;
    defer_drop!(namespace_val, heap);
    defer_drop!(name_val, heap);

    let namespace = extract_uuid_namespace(namespace_val, heap, interns, "uuid.uuid5")?;
    let name_bytes = extract_uuid_name_bytes(name_val, heap, interns, "uuid.uuid5")?;

    let mut hasher = Sha1::new();
    hasher.update(namespace.as_bytes());
    hasher.update(&name_bytes);
    let digest = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    let mut int_uuid = u128::from_be_bytes(bytes);
    int_uuid &= RFC_4122_CLEARFLAGS_MASK;
    int_uuid |= RFC_4122_VERSION_5_FLAGS;

    uuid_from_u128(heap, int_uuid, SafeUuidKind::Unknown)
}

/// Implementation of `uuid.uuid6([node[, clock_seq]])`.
fn uuid6(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let parsed = parse_optional_int_args(heap, interns, args, "uuid.uuid6", &["node", "clock_seq"])?;
    let node = parsed[0].map_or(*UUID_NODE, i64::cast_unsigned) & 0xffff_ffff_ffff;
    let clock_seq = parsed[1].map_or_else(|| random_bits(14), i64::cast_unsigned) & 0x3fff;

    let timestamp = uuid6_timestamp_100ns()?;
    let time_hi_and_mid = (timestamp >> 12) & 0xffff_ffff_ffff;
    let time_lo = timestamp & 0x0fff;

    let mut int_uuid = u128::from(time_hi_and_mid) << 80;
    int_uuid |= u128::from(time_lo) << 64;
    int_uuid |= u128::from(clock_seq) << 48;
    int_uuid |= u128::from(node);
    int_uuid |= RFC_4122_VERSION_6_FLAGS;

    uuid_from_u128(heap, int_uuid, SafeUuidKind::Unknown)
}

/// Implementation of `uuid.uuid7()`.
fn uuid7(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("uuid.uuid7", heap)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SimpleException::new_msg(ExcType::RuntimeError, "system time before Unix epoch"))?;
    let mut timestamp_ms = u64::try_from(now.as_millis())
        .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "timestamp is too large"))?;

    let mut rng = OsRng;
    let (counter, tail) = {
        let mut state = UUID7_STATE.lock().expect("uuid7 state lock poisoned");
        let (counter, tail) = match state.last_timestamp_ms {
            None => uuid7_counter_and_tail(&mut rng),
            Some(last_ts) if timestamp_ms > last_ts => uuid7_counter_and_tail(&mut rng),
            Some(last_ts) => {
                if timestamp_ms < last_ts {
                    timestamp_ms = last_ts + 1;
                }
                let new_counter = state.last_counter + 1;
                if new_counter > 0x3ff_ffff_ffff {
                    timestamp_ms += 1;
                    uuid7_counter_and_tail(&mut rng)
                } else {
                    let new_tail = rng.next_u32();
                    (new_counter, new_tail)
                }
            }
        };

        state.last_timestamp_ms = Some(timestamp_ms);
        state.last_counter = counter;
        (counter, tail)
    };

    let unix_ts_ms = u128::from(timestamp_ms) & 0xffff_ffff_ffff;
    let counter_hi = u128::from((counter >> 30) & 0x0fff);
    let counter_lo = u128::from(counter & 0x3fff_ffff);
    let tail = u128::from(tail) & 0xffff_ffff;

    let mut int_uuid = unix_ts_ms << 80;
    int_uuid |= counter_hi << 64;
    int_uuid |= counter_lo << 32;
    int_uuid |= tail;
    int_uuid |= RFC_4122_VERSION_7_FLAGS;

    uuid_from_u128(heap, int_uuid, SafeUuidKind::Unknown)
}

/// Implementation of `uuid.uuid8([a[, b[, c]]])`.
fn uuid8(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let parsed = parse_optional_int_args(heap, interns, args, "uuid.uuid8", &["a", "b", "c"])?;
    let a_bits = parsed[0].map_or_else(|| random_bits(48), i64::cast_unsigned);
    let b_bits = parsed[1].map_or_else(|| random_bits(12), i64::cast_unsigned);
    let c_bits = parsed[2].map_or_else(|| random_bits(62), i64::cast_unsigned);

    let mut int_uuid = (u128::from(a_bits) & 0xffff_ffff_ffff) << 80;
    int_uuid |= (u128::from(b_bits) & 0x0fff) << 64;
    int_uuid |= u128::from(c_bits) & 0x3fff_ffff_ffff_ffff;
    int_uuid |= RFC_4122_VERSION_8_FLAGS;

    uuid_from_u128(heap, int_uuid, SafeUuidKind::Unknown)
}

/// Implementation of `uuid.getnode()`.
fn getnode(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("uuid.getnode", heap)?;
    Ok(AttrCallResult::Value(Value::Int(*UUID_NODE as i64)))
}

// ===========================================================================
// UUID helpers
// ===========================================================================

/// Parses a namespace UUID from a UUID object or textual UUID value.
fn extract_uuid_namespace(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<RustUuid> {
    match value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Uuid(uuid) => Ok(RustUuid::from_u128(uuid.as_u128())),
            HeapData::Str(s) => RustUuid::parse_str(s.as_str()).map_err(|_| {
                SimpleException::new_msg(ExcType::ValueError, "badly formed hexadecimal UUID string").into()
            }),
            _ => Err(ExcType::type_error(format!(
                "{func_name}() argument must be str or UUID, not {}",
                value.py_type(heap)
            ))),
        },
        Value::InternString(id) => RustUuid::parse_str(interns.get_str(*id))
            .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "badly formed hexadecimal UUID string").into()),
        _ => Err(ExcType::type_error(format!(
            "{func_name}() argument must be str or UUID, not {}",
            value.py_type(heap)
        ))),
    }
}

/// Extracts name bytes for name-based UUIDs, accepting str or bytes.
fn extract_uuid_name_bytes(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<Vec<u8>> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).as_bytes().to_vec()),
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().as_bytes().to_vec()),
            HeapData::Bytes(b) => Ok(b.as_slice().to_vec()),
            _ => Err(ExcType::type_error(format!(
                "{func_name}() argument must be str or bytes, not {}",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{func_name}() argument must be str or bytes, not {}",
            value.py_type(heap)
        ))),
    }
}

/// Extracts optional integer values from positional/keyword argument sets.
fn parse_optional_int_args(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    func_name: &str,
    names: &[&str],
) -> RunResult<Vec<Option<i64>>> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();

    let positional_len = positional.len();
    if positional_len > names.len() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(func_name, names.len(), positional_len));
    }

    let mut values: Vec<Option<Value>> = positional.into_iter().map(Some).collect();
    while values.len() < names.len() {
        values.push(None);
    }

    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(keyword) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            drop_optional_values(values, heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let keyword = keyword.as_str(interns);
        let Some(index) = names.iter().position(|name| *name == keyword) else {
            value.drop_with_heap(heap);
            drop_optional_values(values, heap);
            return Err(ExcType::type_error_unexpected_keyword(func_name, keyword));
        };
        if values[index].is_some() {
            value.drop_with_heap(heap);
            drop_optional_values(values, heap);
            return Err(ExcType::type_error_multiple_values(func_name, names[index]));
        }
        values[index] = Some(value);
    }

    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        parsed.push(extract_optional_int(value, heap)?);
    }
    Ok(parsed)
}

/// Extracts an optional integer value, treating `None` as missing.
fn extract_optional_int(value: Option<Value>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<i64>> {
    match value {
        None => Ok(None),
        Some(Value::None) => Ok(None),
        Some(val) => {
            defer_drop!(val, heap);
            Ok(Some(val.as_int(heap)?))
        }
    }
}

/// Allocates a UUID from a known constant string.
fn allocate_uuid_const(
    heap: &mut Heap<impl ResourceTracker>,
    value: &str,
) -> Result<Value, crate::resource::ResourceError> {
    let parsed = RustUuid::parse_str(value).expect("uuid module constant must be valid");
    Uuid::from_u128(parsed.as_u128(), SafeUuidKind::Unknown).to_value(heap)
}

/// Allocates a heap string value.
fn allocate_string(
    heap: &mut Heap<impl ResourceTracker>,
    value: &str,
) -> Result<Value, crate::resource::ResourceError> {
    let id = heap.allocate(HeapData::Str(value.into()))?;
    Ok(Value::Ref(id))
}

/// Generates a UUID object from a 128-bit integer.
fn uuid_from_u128(
    heap: &mut Heap<impl ResourceTracker>,
    value: u128,
    is_safe: SafeUuidKind,
) -> RunResult<AttrCallResult> {
    let uuid = Uuid::from_u128(value, is_safe);
    Ok(AttrCallResult::Value(uuid.to_value(heap)?))
}

/// Generates a monotonic UUIDv6 timestamp in 100-ns intervals.
fn uuid6_timestamp_100ns() -> RunResult<u64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SimpleException::new_msg(ExcType::RuntimeError, "system time before Unix epoch"))?;
    let timestamp = now.as_nanos() / 100 + UUID_EPOCH_OFFSET_100NS;
    let mut timestamp_u64 = u64::try_from(timestamp)
        .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "timestamp is too large"))?;

    let mut state = UUID6_STATE.lock().expect("uuid6 state lock poisoned");
    if let Some(last) = state.last_timestamp
        && timestamp_u64 <= last
    {
        timestamp_u64 = last + 1;
    }
    state.last_timestamp = Some(timestamp_u64);
    Ok(timestamp_u64)
}

/// Returns a random counter/tail pair for UUIDv7.
fn uuid7_counter_and_tail(rng: &mut OsRng) -> (u64, u32) {
    let counter = rng.next_u64() & 0x1ff_ffff_ffff;
    let tail = rng.next_u32();
    (counter, tail)
}

/// Generates a random value with the specified number of bits.
fn random_bits(bits: u8) -> u64 {
    let mut rng = OsRng;
    if bits >= 64 {
        return rng.next_u64();
    }
    let mask = (1u64 << bits) - 1;
    rng.next_u64() & mask
}

/// Attempts to discover a stable hardware node value from host interfaces.
fn detect_system_node() -> Option<u64> {
    parse_node_from_ifconfig()
}

/// Parses the first `ether xx:xx:xx:xx:xx:xx` entry from `ifconfig` output.
fn parse_node_from_ifconfig() -> Option<u64> {
    let output = Command::new("ifconfig").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(mac) = trimmed.strip_prefix("ether ")
            && let Some(node) = parse_mac_node(mac)
        {
            return Some(node);
        }
    }
    None
}

/// Parses a colon-delimited MAC address into a 48-bit node value.
fn parse_mac_node(mac: &str) -> Option<u64> {
    let mut value = 0u64;
    let mut parts = mac.split(':');
    for _ in 0..6 {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        let byte = u8::from_str_radix(part, 16).ok()?;
        value = (value << 8) | u64::from(byte);
    }
    if parts.next().is_some() || value == 0 {
        None
    } else {
        Some(value)
    }
}

/// Drops optional values while preserving heap reference-count correctness.
fn drop_optional_values<T: ResourceTracker>(values: Vec<Option<Value>>, heap: &mut Heap<T>) {
    for value in values.into_iter().flatten() {
        value.drop_with_heap(heap);
    }
}
