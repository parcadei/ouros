//! Implementation of Python's `binascii` module.
//!
//! This module provides binary/ASCII transforms used by `base64`, `quopri`, and
//! general byte-processing code. The implementation targets CPython 3.14 behavior
//! for the public API surface:
//! - `a2b_base64`, `b2a_base64`
//! - `a2b_hex` / `unhexlify`, `b2a_hex` / `hexlify`
//! - `a2b_qp`, `b2a_qp`
//! - `a2b_uu`, `b2a_uu`
//! - `crc32`, `crc_hqx`
//! - exported exception classes `Error` and `Incomplete`

use std::borrow::Cow;

use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Bytes, Module, PyTrait},
    value::Value,
};

/// Base64 alphabet used by `a2b_base64` and `b2a_base64`.
const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `binascii` module functions implemented by Ouros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum BinasciiFunctions {
    A2bBase64,
    Hexlify,
    Unhexlify,
    Crc32,
    CrcHqx,
    B2aHex,
    A2bHex,
    A2bQp,
    A2bUu,
    B2aBase64,
    B2aQp,
    B2aUu,
}

/// Creates the `binascii` module and registers all public attributes.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Binascii);

    register(&mut module, "a2b_base64", BinasciiFunctions::A2bBase64, heap, interns)?;
    register(&mut module, "a2b_hex", BinasciiFunctions::A2bHex, heap, interns)?;
    register(&mut module, "a2b_qp", BinasciiFunctions::A2bQp, heap, interns)?;
    register(&mut module, "a2b_uu", BinasciiFunctions::A2bUu, heap, interns)?;
    register(&mut module, "b2a_base64", BinasciiFunctions::B2aBase64, heap, interns)?;
    register(&mut module, "b2a_hex", BinasciiFunctions::B2aHex, heap, interns)?;
    register(&mut module, "b2a_qp", BinasciiFunctions::B2aQp, heap, interns)?;
    register(&mut module, "b2a_uu", BinasciiFunctions::B2aUu, heap, interns)?;
    register(&mut module, "crc32", BinasciiFunctions::Crc32, heap, interns)?;
    register(&mut module, "crc_hqx", BinasciiFunctions::CrcHqx, heap, interns)?;
    register(&mut module, "hexlify", BinasciiFunctions::Hexlify, heap, interns)?;
    register(&mut module, "unhexlify", BinasciiFunctions::Unhexlify, heap, interns)?;

    // CPython exposes module-local exception classes here. Ouros currently maps
    // them to equivalent builtin exception classes for behavior parity.
    module.set_attr_text(
        "Error",
        Value::Builtin(Builtins::ExcType(ExcType::ValueError)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "Incomplete",
        Value::Builtin(Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `binascii` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: BinasciiFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        BinasciiFunctions::A2bBase64 => a2b_base64(heap, interns, args)?,
        BinasciiFunctions::Hexlify => hexlify(heap, interns, args, "hexlify")?,
        BinasciiFunctions::B2aHex => hexlify(heap, interns, args, "b2a_hex")?,
        BinasciiFunctions::Unhexlify | BinasciiFunctions::A2bHex => unhexlify(heap, interns, args)?,
        BinasciiFunctions::Crc32 => crc32(heap, interns, args)?,
        BinasciiFunctions::CrcHqx => crc_hqx(heap, interns, args)?,
        BinasciiFunctions::B2aBase64 => b2a_base64(heap, interns, args)?,
        BinasciiFunctions::A2bQp => a2b_qp(heap, interns, args)?,
        BinasciiFunctions::B2aQp => b2a_qp(heap, interns, args)?,
        BinasciiFunctions::A2bUu => a2b_uu(heap, interns, args)?,
        BinasciiFunctions::B2aUu => b2a_uu(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Implements `binascii.a2b_base64(data, *, strict_mode=False)`.
fn a2b_base64(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "a2b_base64() takes exactly 1 positional argument ({positional_count} given)"
        )));
    }

    let data_value = positional.next().expect("validated length");
    positional.drop_with_heap(heap);

    let mut strict_mode = false;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name != "strict_mode" {
            let key_name = key_name.to_owned();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "a2b_base64() got an unexpected keyword argument '{key_name}'"
            )));
        }
        strict_mode = value.py_bool(heap, interns);
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let data = extract_a2b_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let decoded = decode_base64_binascii(&data, strict_mode)?;
    let id = heap.allocate(HeapData::Bytes(Bytes::new(decoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.b2a_base64(data, *, newline=True)`.
fn b2a_base64(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "b2a_base64() takes exactly 1 positional argument ({positional_count} given)"
        )));
    }

    let data_value = positional.next().expect("validated length");
    positional.drop_with_heap(heap);

    let mut newline = true;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name != "newline" {
            let key_name = key_name.to_owned();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "b2a_base64() got an unexpected keyword argument '{key_name}'"
            )));
        }
        newline = value.py_bool(heap, interns);
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let data = extract_binary_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let mut encoded = encode_base64_bytes(&data);
    if newline {
        encoded.push(b'\n');
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(encoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.hexlify(data, sep=None, bytes_per_sep=1)`.
fn hexlify(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    function_name: &str,
) -> RunResult<Value> {
    let (data_value, sep_value, bytes_per_sep) = parse_hexlify_args(heap, interns, args, function_name)?;
    let data = extract_binary_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let mut out = Vec::with_capacity(data.len().saturating_mul(2));

    if let Some(sep) = sep_value {
        if bytes_per_sep == 0 || data.is_empty() {
            append_hex_bytes(&data, &mut out);
        } else {
            let group_size = usize::try_from(bytes_per_sep.unsigned_abs()).unwrap_or(usize::MAX);
            if bytes_per_sep > 0 {
                let first_group = if data.len().is_multiple_of(group_size) {
                    group_size
                } else {
                    data.len() % group_size
                };
                append_hex_bytes(&data[..first_group], &mut out);
                for chunk in data[first_group..].chunks(group_size) {
                    out.extend_from_slice(&sep);
                    append_hex_bytes(chunk, &mut out);
                }
            } else {
                let mut first = true;
                for chunk in data.chunks(group_size) {
                    if !first {
                        out.extend_from_slice(&sep);
                    }
                    first = false;
                    append_hex_bytes(chunk, &mut out);
                }
            }
        }
    } else {
        append_hex_bytes(&data, &mut out);
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(out)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.unhexlify(hexstr)`.
fn unhexlify(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("binascii.unhexlify"));
    }

    let Some(input) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("binascii.unhexlify", 1, 0));
    };
    if let Some(extra) = positional.next() {
        let mut actual = 2usize;
        extra.drop_with_heap(heap);
        for value in positional {
            actual += 1;
            value.drop_with_heap(heap);
        }
        input.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("binascii.unhexlify", 1, actual));
    }
    positional.drop_with_heap(heap);

    let source = extract_a2b_input(&input, heap, interns)?;
    input.drop_with_heap(heap);

    if !source.len().is_multiple_of(2) {
        return Err(binascii_error("Odd-length string"));
    }

    let mut out = Vec::with_capacity(source.len() / 2);
    let mut index = 0usize;
    while index < source.len() {
        let hi = hex_nibble(source[index])?;
        let lo = hex_nibble(source[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(out)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.crc32(data, crc=0)`.
fn crc32(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("binascii.crc32"));
    }

    let Some(data_value) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("crc32", 1, 0));
    };
    let seed_value = positional.next();
    if let Some(extra) = positional.next() {
        let mut actual = 3usize;
        extra.drop_with_heap(heap);
        for value in positional {
            actual += 1;
            value.drop_with_heap(heap);
        }
        data_value.drop_with_heap(heap);
        seed_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("crc32", 2, actual));
    }
    positional.drop_with_heap(heap);

    let data = extract_binary_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let seed = if let Some(value) = seed_value.as_ref() {
        int_value_mod_u32(value, heap, interns)?
    } else {
        0
    };
    seed_value.drop_with_heap(heap);

    let crc = crc32_accumulate(seed, &data);
    Ok(Value::Int(i64::from(crc)))
}

/// Implements `binascii.crc_hqx(data, crc)`.
fn crc_hqx(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("binascii.crc_hqx"));
    }

    let Some(data_value) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("crc_hqx", 2, 0));
    };
    let Some(seed_value) = positional.next() else {
        data_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("crc_hqx", 2, 1));
    };
    if let Some(extra) = positional.next() {
        let mut actual = 3usize;
        extra.drop_with_heap(heap);
        for value in positional {
            actual += 1;
            value.drop_with_heap(heap);
        }
        data_value.drop_with_heap(heap);
        seed_value.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("crc_hqx", 2, actual));
    }
    positional.drop_with_heap(heap);

    let data = extract_binary_input(&data_value, heap, interns)?;
    let seed = int_value_mod_u16(&seed_value, heap, interns)?;
    data_value.drop_with_heap(heap);
    seed_value.drop_with_heap(heap);

    let crc = crc_hqx_accumulate(seed, &data);
    Ok(Value::Int(i64::from(crc)))
}

/// Implements `binascii.a2b_qp(data, header=False)`.
fn a2b_qp(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "a2b_qp() takes at most 2 arguments ({positional_count} given)"
        )));
    }

    let positional_data = positional.next();
    let positional_header = positional.next();
    positional.drop_with_heap(heap);

    let mut data_value = positional_data;
    let mut header_value = positional_header;

    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            header_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        match key_name {
            "data" => {
                if data_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    header_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "argument for a2b_qp() given by name ('data') and position (1)",
                    ));
                }
                data_value = Some(value);
            }
            "header" => {
                if header_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    header_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "argument for a2b_qp() given by name ('header') and position (2)",
                    ));
                }
                header_value = Some(value);
            }
            _ => {
                let key_name = key_name.to_owned();
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                data_value.drop_with_heap(heap);
                header_value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "a2b_qp() got an unexpected keyword argument '{key_name}'"
                )));
            }
        }
        key.drop_with_heap(heap);
    }

    let Some(data_value) = data_value else {
        header_value.drop_with_heap(heap);
        return Err(ExcType::type_error("a2b_qp() missing required argument 'data' (pos 1)"));
    };

    let header = if let Some(header_value) = header_value {
        let value = header_value.py_bool(heap, interns);
        header_value.drop_with_heap(heap);
        value
    } else {
        false
    };

    let data = extract_a2b_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let decoded = decode_quoted_printable(&data, header);
    let id = heap.allocate(HeapData::Bytes(Bytes::new(decoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.b2a_qp(data, quotetabs=False, istext=True, header=False)`.
fn b2a_qp(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 4 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "b2a_qp() takes at most 4 arguments ({positional_count} given)"
        )));
    }

    let positional_data = positional.next();
    let positional_quotetabs = positional.next();
    let positional_istext = positional.next();
    let positional_header = positional.next();
    positional.drop_with_heap(heap);

    let mut data_value = positional_data;
    let mut quotetabs_value = positional_quotetabs;
    let mut istext_value = positional_istext;
    let mut header_value = positional_header;

    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            quotetabs_value.drop_with_heap(heap);
            istext_value.drop_with_heap(heap);
            header_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        match key_name {
            "data" => {
                if data_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    quotetabs_value.drop_with_heap(heap);
                    istext_value.drop_with_heap(heap);
                    header_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "argument for b2a_qp() given by name ('data') and position (1)",
                    ));
                }
                data_value = Some(value);
            }
            "quotetabs" => {
                if quotetabs_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    quotetabs_value.drop_with_heap(heap);
                    istext_value.drop_with_heap(heap);
                    header_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "argument for b2a_qp() given by name ('quotetabs') and position (2)",
                    ));
                }
                quotetabs_value = Some(value);
            }
            "istext" => {
                if istext_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    quotetabs_value.drop_with_heap(heap);
                    istext_value.drop_with_heap(heap);
                    header_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "argument for b2a_qp() given by name ('istext') and position (3)",
                    ));
                }
                istext_value = Some(value);
            }
            "header" => {
                if header_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    quotetabs_value.drop_with_heap(heap);
                    istext_value.drop_with_heap(heap);
                    header_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "argument for b2a_qp() given by name ('header') and position (4)",
                    ));
                }
                header_value = Some(value);
            }
            _ => {
                let key_name = key_name.to_owned();
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                data_value.drop_with_heap(heap);
                quotetabs_value.drop_with_heap(heap);
                istext_value.drop_with_heap(heap);
                header_value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "b2a_qp() got an unexpected keyword argument '{key_name}'"
                )));
            }
        }
        key.drop_with_heap(heap);
    }

    let Some(data_value) = data_value else {
        quotetabs_value.drop_with_heap(heap);
        istext_value.drop_with_heap(heap);
        header_value.drop_with_heap(heap);
        return Err(ExcType::type_error("b2a_qp() missing required argument 'data' (pos 1)"));
    };

    let quotetabs = if let Some(value) = quotetabs_value {
        let flag = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        flag
    } else {
        false
    };
    let istext = if let Some(value) = istext_value {
        let flag = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        flag
    } else {
        true
    };
    let header = if let Some(value) = header_value {
        let flag = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        flag
    } else {
        false
    };

    let data = extract_binary_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let encoded = encode_quoted_printable(&data, quotetabs, istext, header);
    let id = heap.allocate(HeapData::Bytes(Bytes::new(encoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.b2a_uu(data, *, backtick=False)`.
fn b2a_uu(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "b2a_uu() takes exactly 1 positional argument ({positional_count} given)"
        )));
    }

    let data_value = positional.next().expect("validated length");
    positional.drop_with_heap(heap);

    let mut backtick = false;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name != "backtick" {
            let key_name = key_name.to_owned();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "b2a_uu() got an unexpected keyword argument '{key_name}'"
            )));
        }
        backtick = value.py_bool(heap, interns);
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let data = extract_binary_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    if data.len() > 45 {
        return Err(binascii_error("At most 45 bytes at once"));
    }

    let encoded = encode_uu_line(&data, backtick);
    let id = heap.allocate(HeapData::Bytes(Bytes::new(encoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `binascii.a2b_uu(data)`.
fn a2b_uu(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("binascii.a2b_uu"));
    }

    let Some(data_value) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("binascii.a2b_uu", 1, 0));
    };
    if let Some(extra) = positional.next() {
        let mut actual = 2usize;
        extra.drop_with_heap(heap);
        for value in positional {
            actual += 1;
            value.drop_with_heap(heap);
        }
        data_value.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("binascii.a2b_uu", 1, actual));
    }
    positional.drop_with_heap(heap);

    let data = extract_a2b_input(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let decoded = decode_uu_line(&data)?;
    let id = heap.allocate(HeapData::Bytes(Bytes::new(decoded)))?;
    Ok(Value::Ref(id))
}

/// Parses arguments for `hexlify` / `b2a_hex`.
fn parse_hexlify_args(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    function_name: &str,
) -> RunResult<(Value, Option<Vec<u8>>, i64)> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 3 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{function_name}() takes at most 3 arguments ({positional_count} given)"
        )));
    }

    let positional_data = positional.next();
    let positional_sep = positional.next();
    let positional_bytes_per_sep = positional.next();
    positional.drop_with_heap(heap);

    let mut data_value = positional_data;
    let mut sep_value = positional_sep;
    let mut bytes_per_sep_value = positional_bytes_per_sep;

    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            sep_value.drop_with_heap(heap);
            bytes_per_sep_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_name = key_str.as_str(interns);
        match key_name {
            "data" => {
                if data_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    sep_value.drop_with_heap(heap);
                    bytes_per_sep_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "argument for {function_name}() given by name ('data') and position (1)"
                    )));
                }
                data_value = Some(value);
            }
            "sep" => {
                if sep_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    sep_value.drop_with_heap(heap);
                    bytes_per_sep_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "argument for {function_name}() given by name ('sep') and position (2)"
                    )));
                }
                sep_value = Some(value);
            }
            "bytes_per_sep" => {
                if bytes_per_sep_value.is_some() {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    sep_value.drop_with_heap(heap);
                    bytes_per_sep_value.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "argument for {function_name}() given by name ('bytes_per_sep') and position (3)"
                    )));
                }
                bytes_per_sep_value = Some(value);
            }
            _ => {
                let key_name = key_name.to_owned();
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                data_value.drop_with_heap(heap);
                sep_value.drop_with_heap(heap);
                bytes_per_sep_value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{function_name}() got an unexpected keyword argument '{key_name}'"
                )));
            }
        }
        key.drop_with_heap(heap);
    }

    let Some(data_value) = data_value else {
        sep_value.drop_with_heap(heap);
        bytes_per_sep_value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{function_name}() missing required argument 'data' (pos 1)"
        )));
    };

    let separator = if let Some(sep_value) = sep_value {
        let parsed = parse_hex_separator(&sep_value, heap, interns)?;
        sep_value.drop_with_heap(heap);
        Some(parsed)
    } else {
        None
    };

    let bytes_per_sep = if let Some(value) = bytes_per_sep_value {
        let parsed = parse_c_int(&value, heap, interns)?;
        value.drop_with_heap(heap);
        i64::from(parsed)
    } else {
        1
    };

    Ok((data_value, separator, bytes_per_sep))
}

/// Parses the separator value accepted by `hexlify` / `b2a_hex`.
fn parse_hex_separator(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => {
            let bytes = interns.get_bytes(*id);
            if bytes.len() != 1 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "sep must be length 1.").into());
            }
            Ok(bytes.to_vec())
        }
        Value::InternString(id) => {
            let s = interns.get_str(*id);
            if s.chars().count() != 1 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "sep must be length 1.").into());
            }
            Ok(s.as_bytes().to_vec())
        }
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => {
                if bytes.len() != 1 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "sep must be length 1.").into());
                }
                Ok(bytes.as_slice().to_vec())
            }
            HeapData::Str(s) => {
                if s.as_str().chars().count() != 1 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "sep must be length 1.").into());
                }
                Ok(s.as_str().as_bytes().to_vec())
            }
            HeapData::Bytearray(_) => Err(ExcType::type_error("sep must be str or bytes.")),
            _ => Err(ExcType::type_error(format!(
                "object of type '{}' has no len()",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "object of type '{}' has no len()",
            value.py_type(heap)
        ))),
    }
}

/// Extracts bytes-like input for `b2a_*` and checksum functions.
fn extract_binary_input(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            _ => Err(ExcType::type_error(format!(
                "a bytes-like object is required, not '{}'",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "a bytes-like object is required, not '{}'",
            value.py_type(heap)
        ))),
    }
}

/// Extracts bytes input for `a2b_*` functions.
///
/// Accepts bytes-like values and ASCII strings, matching CPython's coercion.
fn extract_a2b_input(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::InternString(id) => extract_ascii_str(interns.get_str(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Str(s) => extract_ascii_str(s.as_str()),
            _ => Err(ExcType::type_error(format!(
                "argument should be bytes, buffer or ASCII string, not '{}'",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "argument should be bytes, buffer or ASCII string, not '{}'",
            value.py_type(heap)
        ))),
    }
}

/// Validates that a string is ASCII-only and returns its byte representation.
fn extract_ascii_str(input: &str) -> RunResult<Vec<u8>> {
    if !input.is_ascii() {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "string argument should contain only ASCII characters",
        )
        .into());
    }
    Ok(input.as_bytes().to_vec())
}

/// Parses an int-like value and wraps it to `u32` for CRC32 seeds.
fn int_value_mod_u32(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<u32> {
    match value {
        Value::Bool(flag) => Ok(u32::from(*flag)),
        Value::Int(v) => Ok(*v as u32),
        Value::InternLongInt(id) => Ok(bigint_mod_u32(interns.get_long_int(*id))),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                Ok(bigint_mod_u32(li.inner()))
            } else {
                Ok(value.as_int(heap)? as u32)
            }
        }
        _ => Ok(value.as_int(heap)? as u32),
    }
}

/// Parses an int-like value and wraps it to `u16` for CRC-HQX seeds.
fn int_value_mod_u16(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<u16> {
    match value {
        Value::Bool(flag) => Ok(u16::from(*flag)),
        Value::Int(v) => Ok(*v as u16),
        Value::InternLongInt(id) => Ok(bigint_mod_u16(interns.get_long_int(*id))),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                Ok(bigint_mod_u16(li.inner()))
            } else {
                Ok(value.as_int(heap)? as u16)
            }
        }
        _ => Ok(value.as_int(heap)? as u16),
    }
}

/// Parses a value as C `int` for the `bytes_per_sep` argument.
fn parse_c_int(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<i32> {
    let overflow =
        || SimpleException::new_msg(ExcType::OverflowError, "Python int too large to convert to C int").into();

    match value {
        Value::Bool(flag) => Ok(i32::from(*flag)),
        Value::Int(v) => i32::try_from(*v).map_err(|_| overflow()),
        Value::InternLongInt(id) => {
            let bigint = interns.get_long_int(*id);
            bigint.to_i32().ok_or_else(overflow)
        }
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                return li.inner().to_i32().ok_or_else(overflow);
            }
            let int_value = value.as_int(heap)?;
            i32::try_from(int_value).map_err(|_| overflow())
        }
        _ => {
            let int_value = value.as_int(heap)?;
            i32::try_from(int_value).map_err(|_| overflow())
        }
    }
}

/// Computes `value mod 2^32`, preserving CPython's wrap behavior for negative values.
fn bigint_mod_u32(value: &BigInt) -> u32 {
    let modulus = BigInt::from(1_u64 << 32);
    let mut rem = value % &modulus;
    if rem.sign() == Sign::Minus {
        rem += &modulus;
    }
    rem.to_u32().expect("modulo 2^32 result must fit u32")
}

/// Computes `value mod 2^16`, preserving CPython's wrap behavior for negative values.
fn bigint_mod_u16(value: &BigInt) -> u16 {
    let modulus = BigInt::from(1_u32 << 16);
    let mut rem = value % &modulus;
    if rem.sign() == Sign::Minus {
        rem += &modulus;
    }
    rem.to_u16().expect("modulo 2^16 result must fit u16")
}

/// Decodes base64 according to `binascii.a2b_base64` semantics.
fn decode_base64_binascii(input: &[u8], strict_mode: bool) -> RunResult<Vec<u8>> {
    if strict_mode {
        decode_base64_strict(input)
    } else {
        decode_base64_non_strict(input)
    }
}

/// Strict base64 decoding (`strict_mode=True`).
fn decode_base64_strict(input: &[u8]) -> RunResult<Vec<u8>> {
    let mut data_count = 0usize;
    let mut pad_count = 0usize;

    for &byte in input {
        if base64_value(byte).is_some() {
            if pad_count == 1 {
                return Err(binascii_error("Discontinuous padding not allowed"));
            }
            if pad_count >= 2 {
                return Err(binascii_error("Excess data after padding"));
            }
            data_count += 1;
            continue;
        }

        if byte == b'=' {
            if data_count == 0 {
                return Err(binascii_error("Leading padding not allowed"));
            }
            pad_count += 1;
            continue;
        }

        return Err(binascii_error("Only base64 data is allowed"));
    }

    validate_base64_counts(data_count, pad_count)?;

    if data_count == 0 {
        return Ok(Vec::new());
    }

    decode_base64_clean(input, data_count)
}

/// Non-strict base64 decoding (`strict_mode=False`).
fn decode_base64_non_strict(input: &[u8]) -> RunResult<Vec<u8>> {
    let mut data = Vec::with_capacity(input.len());
    let mut pending_pad = 0usize;
    let mut final_pad = 0usize;

    for &byte in input {
        if base64_value(byte).is_some() {
            if pending_pad >= 2 {
                break;
            }
            pending_pad = 0;
            data.push(byte);
            continue;
        }

        if byte == b'=' {
            pending_pad += 1;
            if pending_pad > 2 {
                break;
            }
            final_pad = pending_pad;
            continue;
        }

        if pending_pad >= 2 {
            break;
        }
    }

    if data.is_empty() {
        return Ok(Vec::new());
    }

    let data_count = data.len();
    validate_base64_counts(data_count, final_pad)?;

    let mut cleaned = data;
    cleaned.extend(std::iter::repeat_n(b'=', final_pad));
    decode_base64_clean(&cleaned, data_count)
}

/// Validates base64 data/padding counts and emits CPython-compatible errors.
fn validate_base64_counts(data_count: usize, pad_count: usize) -> RunResult<()> {
    if data_count % 4 == 1 {
        return Err(binascii_error(format!(
            "Invalid base64-encoded string: number of data characters ({data_count}) cannot be 1 more than a multiple of 4"
        )));
    }

    if data_count == 0 {
        return Ok(());
    }

    if pad_count > 2 {
        return Err(binascii_error("Excess data after padding"));
    }

    let total = data_count + pad_count;
    if !total.is_multiple_of(4) {
        return Err(binascii_error("Incorrect padding"));
    }

    if pad_count == 1 && data_count % 4 != 3 {
        return Err(binascii_error("Incorrect padding"));
    }
    if pad_count == 2 && data_count % 4 != 2 {
        return Err(binascii_error("Incorrect padding"));
    }

    Ok(())
}

/// Decodes validated base64 bytes into raw bytes.
fn decode_base64_clean(cleaned: &[u8], data_count: usize) -> RunResult<Vec<u8>> {
    let output_len = (data_count / 4) * 3
        + match data_count % 4 {
            2 => 1,
            3 => 2,
            _ => 0,
        };

    let mut out = Vec::with_capacity(output_len);
    for chunk in cleaned.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let c0 = base64_value(chunk[0]).ok_or_else(|| binascii_error("Only base64 data is allowed"))?;
        let c1 = base64_value(chunk[1]).ok_or_else(|| binascii_error("Only base64 data is allowed"))?;
        let c2 = if chunk[2] == b'=' {
            0
        } else {
            base64_value(chunk[2]).ok_or_else(|| binascii_error("Only base64 data is allowed"))?
        };
        let c3 = if chunk[3] == b'=' {
            0
        } else {
            base64_value(chunk[3]).ok_or_else(|| binascii_error("Only base64 data is allowed"))?
        };

        out.push((c0 << 2) | (c1 >> 4));
        if chunk[2] != b'=' {
            out.push((c1 << 4) | (c2 >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c2 << 6) | c3);
        }
    }

    Ok(out)
}

/// Maps one base64 character to its 6-bit value.
fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Encodes raw bytes to base64 without trailing newline.
fn encode_base64_bytes(input: &[u8]) -> Vec<u8> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        let i0 = b0 >> 2;
        let i1 = ((b0 & 0x03) << 4) | (b1 >> 4);
        let i2 = ((b1 & 0x0f) << 2) | (b2 >> 6);
        let i3 = b2 & 0x3f;

        out.push(BASE64_ALPHABET[usize::from(i0)]);
        out.push(BASE64_ALPHABET[usize::from(i1)]);

        if chunk.len() >= 2 {
            out.push(BASE64_ALPHABET[usize::from(i2)]);
        } else {
            out.push(b'=');
        }

        if chunk.len() == 3 {
            out.push(BASE64_ALPHABET[usize::from(i3)]);
        } else {
            out.push(b'=');
        }
    }

    out
}

/// Decodes quoted-printable bytes (`a2b_qp`).
fn decode_quoted_printable(input: &[u8], header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0usize;

    while index < input.len() {
        let mut byte = input[index];
        if header && byte == b'_' {
            byte = b' ';
        }

        if byte != b'=' {
            out.push(byte);
            index += 1;
            continue;
        }

        if index + 1 >= input.len() {
            break;
        }

        let next = input[index + 1];
        if next == b'\n' {
            index += 2;
            continue;
        }
        if next == b'\r' {
            if index + 2 < input.len() && input[index + 2] == b'\n' {
                index += 3;
                continue;
            }
            break;
        }

        if index + 2 < input.len()
            && let (Some(hi), Some(lo)) = (hex_nibble_optional(next), hex_nibble_optional(input[index + 2]))
        {
            out.push((hi << 4) | lo);
            index += 3;
            continue;
        }

        out.push(b'=');
        index += 1;
    }

    out
}

/// Encodes bytes as quoted-printable (`b2a_qp`).
fn encode_quoted_printable(input: &[u8], quotetabs: bool, istext: bool, header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len().saturating_mul(3));
    let mut line_len = 0usize;
    let mut index = 0usize;

    while index < input.len() {
        let byte = input[index];

        if istext && byte == b'\n' {
            out.push(b'\n');
            line_len = 0;
            index += 1;
            continue;
        }
        if istext && byte == b'\r' {
            if index + 1 < input.len() && input[index + 1] == b'\n' {
                out.extend_from_slice(b"\r\n");
                index += 2;
            } else {
                out.push(b'\r');
                index += 1;
            }
            line_len = 0;
            continue;
        }

        let encoded: Cow<'static, [u8]> = if header && byte == b' ' && !quotetabs {
            Cow::Borrowed(b"_")
        } else if (header && byte == b'_') || needs_qp_escape(input, index, quotetabs, istext) {
            Cow::Owned(hex_escape(byte))
        } else {
            Cow::Owned(vec![byte])
        };

        if line_len + encoded.len() > 75 {
            out.extend_from_slice(b"=\n");
            line_len = 0;
        }

        out.extend_from_slice(&encoded);
        line_len += encoded.len();
        index += 1;
    }

    out
}

/// Returns whether byte at `index` needs quoted-printable escaping.
fn needs_qp_escape(input: &[u8], index: usize, quotetabs: bool, istext: bool) -> bool {
    let byte = input[index];

    if byte == b'=' {
        return true;
    }

    if !istext && (byte == b'\r' || byte == b'\n') {
        return true;
    }

    if byte == b'\t' || byte == b' ' {
        if quotetabs {
            return true;
        }
        if index + 1 == input.len() {
            return true;
        }
        let next = input[index + 1];
        return next == b'\n' || next == b'\r';
    }

    !(33..=126).contains(&byte)
}

/// Encodes one byte as quoted-printable hex escape.
fn hex_escape(byte: u8) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    vec![b'=', HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]]
}

/// Encodes a single UU line.
fn encode_uu_line(input: &[u8], backtick: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + input.len().div_ceil(3) * 4 + 1);
    out.push(uu_encode_sixbit(input.len() as u8, backtick));

    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        let c0 = b0 >> 2;
        let c1 = ((b0 & 0x03) << 4) | (b1 >> 4);
        let c2 = ((b1 & 0x0f) << 2) | (b2 >> 6);
        let c3 = b2 & 0x3f;

        out.push(uu_encode_sixbit(c0, backtick));
        out.push(uu_encode_sixbit(c1, backtick));
        out.push(uu_encode_sixbit(c2, backtick));
        out.push(uu_encode_sixbit(c3, backtick));
    }

    out.push(b'\n');
    out
}

/// Decodes a single UU line.
fn decode_uu_line(input: &[u8]) -> RunResult<Vec<u8>> {
    if input.is_empty() {
        return Ok(vec![0; 32]);
    }

    let length = uu_decode_char(input[0])? as usize;
    let groups = length.div_ceil(3);
    let needed_chars = groups * 4;

    let mut data_chars = Vec::with_capacity(needed_chars);
    let mut index = 1usize;

    while data_chars.len() < needed_chars {
        if index >= input.len() {
            data_chars.push(0);
            continue;
        }

        let byte = input[index];
        if byte == b'\n' || byte == b'\r' {
            data_chars.push(0);
            continue;
        }

        data_chars.push(uu_decode_char(byte)?);
        index += 1;
    }

    let mut out = Vec::with_capacity(groups * 3);
    for group in data_chars.chunks(4) {
        let a = group[0];
        let b = group[1];
        let c = group[2];
        let d = group[3];

        out.push((a << 2) | (b >> 4));
        out.push((b << 4) | (c >> 2));
        out.push((c << 6) | d);
    }

    if out.len() > length {
        if out[length..].iter().any(|byte| *byte != 0) {
            return Err(binascii_error("Trailing garbage"));
        }
        out.truncate(length);
    }

    while index < input.len() {
        let byte = input[index];
        if byte == b'\n' || byte == b'\r' || byte == b' ' {
            index += 1;
            continue;
        }
        return Err(binascii_error("Trailing garbage"));
    }

    Ok(out)
}

/// Encodes a 6-bit value into UU alphabet.
fn uu_encode_sixbit(value: u8, backtick: bool) -> u8 {
    let encoded = (value & 0x3f) + b' ';
    if backtick && encoded == b' ' { b'`' } else { encoded }
}

/// Decodes one UU encoded character to a 6-bit value.
fn uu_decode_char(byte: u8) -> RunResult<u8> {
    if (b' '..=b'`').contains(&byte) {
        return Ok((byte - b' ') & 0x3f);
    }
    Err(binascii_error("Illegal char"))
}

/// Converts one hexadecimal digit byte into its numeric value.
fn hex_nibble(byte: u8) -> RunResult<u8> {
    hex_nibble_optional(byte).ok_or_else(|| binascii_error("Non-hexadecimal digit found"))
}

/// Optional hex digit decoder used by quoted-printable parser.
fn hex_nibble_optional(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Appends lowercase hexadecimal representation of `bytes` into `out`.
fn append_hex_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 0x0f)]);
    }
}

/// Computes CRC32 from an initial value and data bytes.
fn crc32_accumulate(initial: u32, bytes: &[u8]) -> u32 {
    let mut crc = !initial;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & 0xedb8_8320;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Computes CRC-HQX (`CRC-16-CCITT`) from an initial seed and data bytes.
fn crc_hqx_accumulate(initial: u16, bytes: &[u8]) -> u16 {
    let mut crc = initial;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Returns a `binascii.Error`-compatible runtime error.
fn binascii_error(message: impl Into<String>) -> crate::exception_private::RunError {
    SimpleException::new_msg(ExcType::ValueError, message.into()).into()
}

/// Registers one module-level function.
fn register(
    module: &mut Module,
    name: &str,
    function: BinasciiFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Binascii(function)),
        heap,
        interns,
    )
}
