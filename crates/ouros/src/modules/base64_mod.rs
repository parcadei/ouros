//! Implementation of the `base64` module.
//!
//! Provides base64, base32, base16, base85, ASCII85, and Z85 encoding/decoding:
//! - `b64encode(b)`: Encode bytes to standard base64 bytes
//! - `b64decode(b)`: Decode standard base64 bytes to bytes
//! - `standard_b64encode(b)`: Alias of `b64encode`
//! - `standard_b64decode(b)`: Alias of `b64decode`
//! - `urlsafe_b64encode(b)`: Encode bytes to URL-safe base64 bytes (uses `-` and `_`)
//! - `urlsafe_b64decode(b)`: Decode URL-safe base64 bytes to bytes
//! - `encodebytes(b)`: Base64-encode bytes with line breaks every 76 characters
//! - `decodebytes(b)`: Decode base64 bytes after stripping ASCII whitespace
//! - `b32encode(b)`: Encode bytes to base32 bytes
//! - `b32decode(b)`: Decode base32 bytes to bytes
//! - `b32hexencode(b)`: Encode bytes to base32 using the extended hex alphabet
//! - `b32hexdecode(b)`: Decode base32 hex bytes to bytes
//! - `b16encode(b)`: Encode bytes to base16 (uppercase hex) bytes
//! - `b16decode(b)`: Decode base16 (uppercase hex) bytes to bytes
//! - `b85encode(b)`: Encode bytes to base85 (RFC 1924) bytes
//! - `b85decode(b)`: Decode base85 (RFC 1924) bytes to bytes
//! - `a85encode(b)`: Encode bytes to ASCII85 bytes
//! - `a85decode(b)`: Decode ASCII85 bytes to bytes
//! - `z85encode(b)`: Encode bytes to ZeroMQ Z85 bytes
//! - `z85decode(b)`: Decode ZeroMQ Z85 bytes to bytes
//! - `encode(input, output)`: Not implemented in sandboxed mode
//! - `decode(input, output)`: Not implemented in sandboxed mode
//!
//! All functions accept and return `bytes` objects, matching CPython's API.

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::{ModuleFunctions, hashlib::extract_bytes as extract_hash_bytes},
    resource::ResourceTracker,
    types::{AttrCallResult, Bytes, OurosIter},
    value::{EitherStr, Value},
};

/// The standard base64 alphabet used for encoding.
const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The URL-safe base64 alphabet (uses `-` and `_` instead of `+` and `/`).
const URLSAFE_BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The base32 alphabet (RFC 4648).
const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The base32 extended hex alphabet (RFC 4648).
const BASE32HEX_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

/// Padding character used in base64/base32 encoding.
const PAD: u8 = b'=';

/// The ASCII85 alphabet starts at `!` and ends at `u`.
const ASCII85_FIRST: u8 = b'!';

/// The ASCII85 alphabet ends at `u`.
const ASCII85_LAST: u8 = b'u';

/// The base85 alphabet used by Python's `b85encode`/`b85decode` (RFC 1924 variant).
///
/// This is the same 85-character set used by CPython for `base64.b85encode`.
const BASE85_ALPHABET: &[u8; 85] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

/// The ZeroMQ Z85 alphabet used by Python's `z85encode`/`z85decode`.
const Z85_ALPHABET: &[u8; 85] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

/// Line length used by `encodebytes` for legacy base64 wrapping.
const BASE64_LINE_WRAP: usize = 76;

/// Input chunk size used by `base64.encode`.
const MAX_BINSIZE: i64 = 57;

/// Base64 module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum Base64Functions {
    B64encode,
    B64decode,
    #[strum(serialize = "standard_b64encode")]
    StandardB64encode,
    #[strum(serialize = "standard_b64decode")]
    StandardB64decode,
    #[strum(serialize = "urlsafe_b64encode")]
    UrlsafeB64encode,
    #[strum(serialize = "urlsafe_b64decode")]
    UrlsafeB64decode,
    Encodebytes,
    Decodebytes,
    B32encode,
    B32decode,
    B32hexencode,
    B32hexdecode,
    B16encode,
    B16decode,
    B85encode,
    B85decode,
    A85encode,
    A85decode,
    Z85encode,
    Z85decode,
    Encode,
    Decode,
}

/// Creates the `base64` module and allocates it on the heap.
///
/// The module provides encoding and decoding functions for base64, URL-safe base64,
/// base32, base16, and base85 formats.
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
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Base64);

    let attrs: &[(StaticStrings, Base64Functions)] = &[
        (StaticStrings::B64Encode, Base64Functions::B64encode),
        (StaticStrings::B64Decode, Base64Functions::B64decode),
        (StaticStrings::StandardB64Encode, Base64Functions::StandardB64encode),
        (StaticStrings::StandardB64Decode, Base64Functions::StandardB64decode),
        (StaticStrings::UrlsafeB64Encode, Base64Functions::UrlsafeB64encode),
        (StaticStrings::UrlsafeB64Decode, Base64Functions::UrlsafeB64decode),
        (StaticStrings::EncodeBytes, Base64Functions::Encodebytes),
        (StaticStrings::DecodeBytes, Base64Functions::Decodebytes),
        (StaticStrings::B32Encode, Base64Functions::B32encode),
        (StaticStrings::B32Decode, Base64Functions::B32decode),
        (StaticStrings::B32HexEncode, Base64Functions::B32hexencode),
        (StaticStrings::B32HexDecode, Base64Functions::B32hexdecode),
        (StaticStrings::B16Encode, Base64Functions::B16encode),
        (StaticStrings::B16Decode, Base64Functions::B16decode),
        (StaticStrings::B85Encode, Base64Functions::B85encode),
        (StaticStrings::B85Decode, Base64Functions::B85decode),
        (StaticStrings::A85Encode, Base64Functions::A85encode),
        (StaticStrings::A85Decode, Base64Functions::A85decode),
        (StaticStrings::Z85Encode, Base64Functions::Z85encode),
        (StaticStrings::Z85Decode, Base64Functions::Z85decode),
        (StaticStrings::Encode, Base64Functions::Encode),
        (StaticStrings::Decode, Base64Functions::Decode),
    ];

    for &(name, func) in attrs {
        module.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::Base64(func)),
            heap,
            interns,
        );
    }
    module.set_attr(StaticStrings::Base64MaxBinSize, Value::Int(MAX_BINSIZE), heap, interns);
    module.set_attr(
        StaticStrings::Base64MaxLineSize,
        Value::Int(BASE64_LINE_WRAP as i64),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a base64 module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: Base64Functions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        Base64Functions::B64encode => b64encode(heap, interns, args),
        Base64Functions::B64decode => b64decode(heap, interns, args),
        Base64Functions::StandardB64encode => standard_b64encode(heap, interns, args),
        Base64Functions::StandardB64decode => standard_b64decode(heap, interns, args),
        Base64Functions::UrlsafeB64encode => urlsafe_b64encode(heap, interns, args),
        Base64Functions::UrlsafeB64decode => urlsafe_b64decode(heap, interns, args),
        Base64Functions::Encodebytes => encodebytes(heap, interns, args),
        Base64Functions::Decodebytes => decodebytes(heap, interns, args),
        Base64Functions::B32encode => b32encode(heap, interns, args),
        Base64Functions::B32decode => b32decode(heap, interns, args),
        Base64Functions::B32hexencode => b32hexencode(heap, interns, args),
        Base64Functions::B32hexdecode => b32hexdecode(heap, interns, args),
        Base64Functions::B16encode => b16encode(heap, interns, args),
        Base64Functions::B16decode => b16decode(heap, interns, args),
        Base64Functions::B85encode => b85encode(heap, interns, args),
        Base64Functions::B85decode => b85decode(heap, interns, args),
        Base64Functions::A85encode => a85encode(heap, interns, args),
        Base64Functions::A85decode => a85decode(heap, interns, args),
        Base64Functions::Z85encode => z85encode(heap, interns, args),
        Base64Functions::Z85decode => z85decode(heap, interns, args),
        Base64Functions::Encode => encode(heap, interns, args),
        Base64Functions::Decode => decode(heap, interns, args),
    }
}

/// Extracts bytes for base64 operations.
///
/// Most calls use the standard bytes-like extractor shared with hashlib.
/// For parity with callers that pass integer iterables through `bytes(...)`,
/// this also materializes list/tuple/range values as byte sequences.
/// Additionally, accepts ASCII strings (matching CPython's behavior).
fn extract_bytes(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    if let Ok(bytes) = extract_hash_bytes(value, heap, interns) {
        return Ok(bytes);
    }

    // Accept ASCII strings (matching CPython behavior)
    if let Some(s) = value.as_either_str(heap) {
        let s_str = s.as_str(interns);
        if s_str.is_ascii() {
            return Ok(s_str.as_bytes().to_vec());
        }
        return Err(SimpleException::new_msg(ExcType::ValueError, "string argument must be ASCII-only").into());
    }

    let Value::Ref(id) = value else {
        return Err(ExcType::type_error("a bytes-like object is required"));
    };
    if !matches!(
        heap.get(*id),
        HeapData::Range(_) | HeapData::List(_) | HeapData::Tuple(_)
    ) {
        return Err(ExcType::type_error("a bytes-like object is required"));
    }

    let iterable = value.clone_with_heap(heap);
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut out = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let int_value = match item.as_int(heap) {
                    Ok(v) => v,
                    Err(err) => {
                        item.drop_with_heap(heap);
                        iter.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                if !(0..=255).contains(&int_value) {
                    item.drop_with_heap(heap);
                    iter.drop_with_heap(heap);
                    return Err(SimpleException::new_msg(ExcType::ValueError, "bytes must be in range(0, 256)").into());
                }
                #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                {
                    out.push(int_value as u8);
                }
                item.drop_with_heap(heap);
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(out)
}

// ===========================================================================
// Standard base64
// ===========================================================================

/// Implementation of `base64.b64encode(b)`.
///
/// Encodes a bytes object to standard base64 format, returning bytes.
fn b64encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_len = positional.len();
    if positional_len == 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.b64encode()", 1, 0));
    }
    if positional_len > 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("base64.b64encode()", 2, positional_len));
    }

    let input = positional.next().expect("validated positional length");
    defer_drop!(input, heap);
    let mut altchars_value = positional.next();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            altchars_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        if key_str.as_str(interns) == "altchars" {
            altchars_value.replace(value).drop_with_heap(heap);
        } else {
            let key_name = key_str.as_str(interns).to_string();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            altchars_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for base64.b64encode()"
            )));
        }
        key.drop_with_heap(heap);
    }

    let input_bytes = extract_bytes(input, heap, interns)?;
    let mut encoded = encode_base64_bytes(&input_bytes, BASE64_ALPHABET);

    if let Some(altchars) = altchars_value {
        let altchars_bytes = extract_bytes(&altchars, heap, interns);
        altchars.drop_with_heap(heap);
        let altchars_bytes = altchars_bytes?;
        if altchars_bytes.len() != 2 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "altchars must be length 2").into());
        }
        for byte in &mut encoded {
            if *byte == b'+' {
                *byte = altchars_bytes[0];
            } else if *byte == b'/' {
                *byte = altchars_bytes[1];
            }
        }
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(encoded)))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `base64.b64decode(b)`.
///
/// Decodes standard base64-encoded bytes, returning the decoded bytes.
fn b64decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_len = positional.len();
    if positional_len == 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.b64decode()", 1, 0));
    }
    if positional_len > 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("base64.b64decode()", 2, positional_len));
    }

    let input = positional.next().expect("validated positional length");
    defer_drop!(input, heap);
    let mut altchars_value = positional.next();
    positional.drop_with_heap(heap);

    let mut validate = false;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            altchars_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_str.as_str(interns);
        if key_name == "altchars" {
            altchars_value.replace(value).drop_with_heap(heap);
        } else if key_name == "validate" {
            validate = matches!(value, Value::Bool(true));
            value.drop_with_heap(heap);
        } else {
            let key_name = key_name.to_string();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            altchars_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for base64.b64decode()"
            )));
        }
        key.drop_with_heap(heap);
    }

    let mut input_bytes = extract_bytes(input, heap, interns)?;

    if let Some(altchars) = altchars_value {
        let altchars_bytes = extract_bytes(&altchars, heap, interns);
        altchars.drop_with_heap(heap);
        let altchars_bytes = altchars_bytes?;
        if altchars_bytes.len() != 2 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "altchars must be length 2").into());
        }
        for byte in &mut input_bytes {
            if *byte == altchars_bytes[0] {
                *byte = b'+';
            } else if *byte == altchars_bytes[1] {
                *byte = b'/';
            }
        }
    }

    if !validate {
        input_bytes = strip_ascii_whitespace(&input_bytes);
    }

    let result = decode_base64_generic(&input_bytes, false, heap)?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.standard_b64encode(b)`.
///
/// This is an alias of `b64encode` for compatibility with CPython.
fn standard_b64encode(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    b64encode(heap, interns, args)
}

/// Implementation of `base64.standard_b64decode(b)`.
///
/// This is an alias of `b64decode` for compatibility with CPython.
fn standard_b64decode(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    b64decode(heap, interns, args)
}

// ===========================================================================
// URL-safe base64
// ===========================================================================

/// Implementation of `base64.urlsafe_b64encode(b)`.
///
/// Encodes bytes to URL-safe base64 format using `-` and `_` instead of `+` and `/`.
fn urlsafe_b64encode(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.urlsafe_b64encode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = encode_base64_generic(&input_bytes, URLSAFE_BASE64_ALPHABET, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.urlsafe_b64decode(b)`.
///
/// Decodes URL-safe base64-encoded bytes (accepts `-` and `_`), returning decoded bytes.
fn urlsafe_b64decode(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.urlsafe_b64decode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = decode_base64_generic(&input_bytes, true, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// Legacy line-wrapped base64
// ===========================================================================

/// Implementation of `base64.encodebytes(b)`.
///
/// Encodes bytes to base64 and inserts a newline every 76 characters, always
/// terminating with a newline for non-empty input.
fn encodebytes(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.encodebytes", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let encoded = encode_base64_bytes(&input_bytes, BASE64_ALPHABET);
    let wrapped = wrap_base64_lines(&encoded);
    let id = heap.allocate(HeapData::Bytes(Bytes::new(wrapped)))?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `base64.decodebytes(b)`.
///
/// Removes ASCII whitespace and decodes base64 bytes.
fn decodebytes(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.decodebytes", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let filtered = strip_ascii_whitespace(&input_bytes);
    let result = decode_base64_generic(&filtered, false, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// Base32
// ===========================================================================

/// Implementation of `base64.b32encode(b)`.
///
/// Encodes bytes to base32 format (RFC 4648), returning bytes.
fn b32encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.b32encode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = encode_base32(&input_bytes, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.b32decode(s, casefold=False)`.
///
/// Decodes base32-encoded bytes or ASCII string (RFC 4648), returning the decoded bytes.
/// When `casefold=True`, lowercase characters are accepted.
fn b32decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(input) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.b32decode()", 1, 0));
    };
    positional.drop_with_heap(heap);

    let mut casefold = false;
    let mut map01: Option<u8> = None;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        if key_str.as_str(interns) == "casefold" {
            casefold = matches!(value, Value::Bool(true));
            value.drop_with_heap(heap);
        } else if key_str.as_str(interns) == "map01" {
            if matches!(value, Value::None) {
                map01 = None;
                value.drop_with_heap(heap);
            } else {
                let map_bytes = extract_bytes(&value, heap, interns);
                value.drop_with_heap(heap);
                let map_bytes = map_bytes?;
                if map_bytes.len() != 1 {
                    key.drop_with_heap(heap);
                    input.drop_with_heap(heap);
                    return Err(SimpleException::new_msg(ExcType::ValueError, "map01 must be a single byte").into());
                }
                map01 = Some(map_bytes[0].to_ascii_uppercase());
            }
        } else {
            let key_name = key_str.as_str(interns).to_string();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for base64.b32decode()"
            )));
        }
        key.drop_with_heap(heap);
    }

    let mut input_bytes = extract_bytes(&input, heap, interns)?;
    input.drop_with_heap(heap);

    if casefold {
        input_bytes.make_ascii_uppercase();
    }
    if let Some(map01_char) = map01 {
        for byte in &mut input_bytes {
            if *byte == b'0' {
                *byte = b'O';
            } else if *byte == b'1' {
                *byte = map01_char;
            }
        }
    }

    let result = decode_base32(&input_bytes, heap)?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.b32hexencode(b)`.
///
/// Encodes bytes to base32 using the extended hex alphabet, returning bytes.
fn b32hexencode(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.b32hexencode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = encode_base32_hex(&input_bytes, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.b32hexdecode(s, casefold=False)`.
///
/// Decodes base32 hex bytes or ASCII string, returning the decoded bytes.
/// When `casefold=True`, lowercase characters are accepted.
fn b32hexdecode(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(input) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.b32hexdecode()", 1, 0));
    };
    positional.drop_with_heap(heap);

    let mut casefold = false;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        if key_str.as_str(interns) == "casefold" {
            casefold = matches!(value, Value::Bool(true));
            value.drop_with_heap(heap);
        } else {
            let key_name = key_str.as_str(interns).to_string();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for base64.b32hexdecode()"
            )));
        }
        key.drop_with_heap(heap);
    }

    let mut input_bytes = extract_bytes(&input, heap, interns)?;
    input.drop_with_heap(heap);

    if casefold {
        input_bytes.make_ascii_uppercase();
    }

    let result = decode_base32_hex(&input_bytes, heap)?;
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// Base16
// ===========================================================================

/// Implementation of `base64.b16encode(b)`.
///
/// Encodes bytes to base16 (uppercase hex) format, returning bytes.
fn b16encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.b16encode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = encode_base16(&input_bytes, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.b16decode(s, casefold=False)`.
///
/// Decodes base16 (uppercase hex) encoded bytes or ASCII string, returning the decoded bytes.
/// When `casefold=True`, lowercase hex characters are accepted.
fn b16decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(input) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.b16decode()", 1, 0));
    };
    positional.drop_with_heap(heap);

    let mut casefold = false;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        if key_str.as_str(interns) == "casefold" {
            casefold = matches!(value, Value::Bool(true));
            value.drop_with_heap(heap);
        } else {
            let key_name = key_str.as_str(interns).to_string();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            input.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for base64.b16decode()"
            )));
        }
        key.drop_with_heap(heap);
    }

    let mut input_bytes = extract_bytes(&input, heap, interns)?;
    input.drop_with_heap(heap);

    if casefold {
        input_bytes.make_ascii_uppercase();
    }

    let result = decode_base16(&input_bytes, heap)?;
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// Base85
// ===========================================================================

/// Implementation of `base64.b85encode(b)`.
///
/// Encodes bytes to base85 (RFC 1924 variant) format, returning bytes.
/// This matches CPython's `base64.b85encode` which uses the RFC 1924 alphabet.
fn b85encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_len = positional.len();
    if positional_len == 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.b85encode()", 1, 0));
    }
    if positional_len > 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("base64.b85encode()", 2, positional_len));
    }

    let input = positional.next().expect("validated positional length");
    defer_drop!(input, heap);

    let mut pad = false;
    if let Some(pad_value) = positional.next() {
        pad = matches!(pad_value, Value::Bool(true));
        pad_value.drop_with_heap(heap);
    }
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        if key_str.as_str(interns) == "pad" {
            pad = matches!(value, Value::Bool(true));
            value.drop_with_heap(heap);
        } else {
            let key_name = key_str.as_str(interns).to_string();
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for base64.b85encode()"
            )));
        }
        key.drop_with_heap(heap);
    }

    let mut input_bytes = extract_bytes(input, heap, interns)?;
    if pad && !input_bytes.len().is_multiple_of(4) {
        let pad_count = 4 - (input_bytes.len() % 4);
        input_bytes.extend(std::iter::repeat_n(0, pad_count));
    }

    let result = encode_base85(&input_bytes, heap)?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.b85decode(b)`.
///
/// Decodes base85 (RFC 1924 variant) encoded bytes, returning the decoded bytes.
fn b85decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.b85decode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = decode_base85(&input_bytes, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// ASCII85
// ===========================================================================

/// Implementation of `base64.a85encode(b)`.
///
/// Encodes bytes to ASCII85 format, returning bytes.
fn a85encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_len = positional.len();
    if positional_len == 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.a85encode()", 1, 0));
    }
    if positional_len > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("base64.a85encode()", 1, positional_len));
    }

    let input = positional.next().expect("validated positional length");
    defer_drop!(input, heap);
    positional.drop_with_heap(heap);

    let mut foldspaces = false;
    let mut wrapcol = 0usize;
    let mut pad = false;
    let mut adobe = false;

    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        match key_str.as_str(interns) {
            "foldspaces" => {
                foldspaces = matches!(value, Value::Bool(true));
                value.drop_with_heap(heap);
            }
            "wrapcol" => {
                let wrapcol_value = value.as_int(heap);
                value.drop_with_heap(heap);
                let wrapcol_value = wrapcol_value?;
                if wrapcol_value < 0 {
                    key.drop_with_heap(heap);
                    return Err(SimpleException::new_msg(ExcType::ValueError, "wrapcol must be non-negative").into());
                }
                wrapcol = usize::try_from(wrapcol_value)
                    .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "wrapcol is too large"))?;
            }
            "pad" => {
                pad = matches!(value, Value::Bool(true));
                value.drop_with_heap(heap);
            }
            "adobe" => {
                adobe = matches!(value, Value::Bool(true));
                value.drop_with_heap(heap);
            }
            _ => {
                let key_name = key_str.as_str(interns).to_string();
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for base64.a85encode()"
                )));
            }
        }
        key.drop_with_heap(heap);
    }

    let mut input_bytes = extract_bytes(input, heap, interns)?;
    if pad && !input_bytes.len().is_multiple_of(4) {
        let pad_count = 4 - (input_bytes.len() % 4);
        input_bytes.extend(std::iter::repeat_n(0, pad_count));
    }

    let mut encoded = encode_ascii85(&input_bytes, foldspaces);
    if wrapcol > 0 {
        encoded = wrap_encoded_lines(&encoded, wrapcol);
    }
    if adobe {
        let mut wrapped = Vec::with_capacity(encoded.len() + 4);
        wrapped.extend_from_slice(b"<~");
        wrapped.extend_from_slice(&encoded);
        wrapped.extend_from_slice(b"~>");
        encoded = wrapped;
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(encoded)))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `base64.a85decode(b)`.
///
/// Decodes ASCII85-encoded bytes, returning the decoded bytes.
fn a85decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let positional_len = positional.len();
    if positional_len == 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("base64.a85decode()", 1, 0));
    }
    if positional_len > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("base64.a85decode()", 1, positional_len));
    }

    let input = positional.next().expect("validated positional length");
    defer_drop!(input, heap);
    positional.drop_with_heap(heap);

    let mut foldspaces = false;
    let mut adobe = false;
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        match key_str.as_str(interns) {
            "foldspaces" => {
                foldspaces = matches!(value, Value::Bool(true));
                value.drop_with_heap(heap);
            }
            "adobe" => {
                adobe = matches!(value, Value::Bool(true));
                value.drop_with_heap(heap);
            }
            _ => {
                let key_name = key_str.as_str(interns).to_string();
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for base64.a85decode()"
                )));
            }
        }
        key.drop_with_heap(heap);
    }

    let input_bytes = extract_bytes(input, heap, interns)?;
    let result = decode_ascii85(&input_bytes, foldspaces, adobe, heap)?;
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// ZeroMQ Z85
// ===========================================================================

/// Implementation of `base64.z85encode(b)`.
///
/// Encodes bytes to the ZeroMQ Z85 format, returning bytes.
fn z85encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.z85encode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = encode_z85(&input_bytes, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `base64.z85decode(b)`.
///
/// Decodes Z85-encoded bytes, returning the decoded bytes.
fn z85decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("base64.z85decode", heap)?;
    let input_bytes = extract_bytes(&input, heap, interns)?;
    let result = decode_z85(&input_bytes, heap)?;
    input.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

// ===========================================================================
// File-based stubs
// ===========================================================================

/// Implementation of `base64.encode(input, output)`.
///
/// Reads all bytes from `input`, base64-encodes them with legacy newline wrapping,
/// and writes the encoded bytes to `output`.
fn encode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (input, output) = args.get_two_args("base64.encode", heap)?;
    let input_id = if let Value::Ref(id) = input {
        id
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::type_error("base64.encode requires file objects"));
    };
    let output_id = if let Value::Ref(id) = output {
        id
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::type_error("base64.encode requires file objects"));
    };

    let read_attr = EitherStr::Heap("read".to_owned());
    let read_result = if let Ok(result) = heap.call_attr_raw(input_id, &read_attr, ArgValues::Empty, interns) {
        result
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::not_implemented("base64.encode requires file objects").into());
    };
    let read_value = if let AttrCallResult::Value(value) = read_result {
        value
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::not_implemented("base64.encode requires file objects").into());
    };
    let input_bytes = extract_bytes(&read_value, heap, interns);
    read_value.drop_with_heap(heap);
    let input_bytes = input_bytes?;
    let encoded = wrap_base64_lines(&encode_base64_bytes(&input_bytes, BASE64_ALPHABET));
    let encoded_id = heap.allocate(HeapData::Bytes(Bytes::new(encoded)))?;
    let write_arg = Value::Ref(encoded_id);

    let write_attr = EitherStr::Heap("write".to_owned());
    let write_result =
        if let Ok(result) = heap.call_attr_raw(output_id, &write_attr, ArgValues::One(write_arg), interns) {
            result
        } else {
            input.drop_with_heap(heap);
            output.drop_with_heap(heap);
            return Err(ExcType::not_implemented("base64.encode requires file objects").into());
        };
    let result = match write_result {
        AttrCallResult::Value(value) => {
            value.drop_with_heap(heap);
            Ok(AttrCallResult::Value(Value::None))
        }
        _ => Err(ExcType::not_implemented("base64.encode requires file objects").into()),
    };
    input.drop_with_heap(heap);
    output.drop_with_heap(heap);
    result
}

/// Implementation of `base64.decode(input, output)`.
///
/// Reads base64 bytes from `input`, decodes them, and writes the decoded bytes to `output`.
fn decode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (input, output) = args.get_two_args("base64.decode", heap)?;
    let input_id = if let Value::Ref(id) = input {
        id
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::type_error("base64.decode requires file objects"));
    };
    let output_id = if let Value::Ref(id) = output {
        id
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::type_error("base64.decode requires file objects"));
    };

    let read_attr = EitherStr::Heap("read".to_owned());
    let read_result = if let Ok(result) = heap.call_attr_raw(input_id, &read_attr, ArgValues::Empty, interns) {
        result
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::not_implemented("base64.decode requires file objects").into());
    };
    let read_value = if let AttrCallResult::Value(value) = read_result {
        value
    } else {
        input.drop_with_heap(heap);
        output.drop_with_heap(heap);
        return Err(ExcType::not_implemented("base64.decode requires file objects").into());
    };
    let input_bytes = extract_bytes(&read_value, heap, interns);
    read_value.drop_with_heap(heap);
    let input_bytes = input_bytes?;
    let filtered = strip_ascii_whitespace(&input_bytes);
    let decoded_value = decode_base64_generic(&filtered, false, heap)?;

    let write_attr = EitherStr::Heap("write".to_owned());
    let write_result =
        if let Ok(result) = heap.call_attr_raw(output_id, &write_attr, ArgValues::One(decoded_value), interns) {
            result
        } else {
            input.drop_with_heap(heap);
            output.drop_with_heap(heap);
            return Err(ExcType::not_implemented("base64.decode requires file objects").into());
        };
    let result = match write_result {
        AttrCallResult::Value(value) => {
            value.drop_with_heap(heap);
            Ok(AttrCallResult::Value(Value::None))
        }
        _ => Err(ExcType::not_implemented("base64.decode requires file objects").into()),
    };
    input.drop_with_heap(heap);
    output.drop_with_heap(heap);
    result
}

// ===========================================================================
// Encoding/decoding helpers
// ===========================================================================

/// Encodes a byte slice using a base64 alphabet and allocates the result on the heap.
///
/// Works for both standard and URL-safe base64 by accepting the alphabet as a parameter.
/// Processes input in chunks of 3 bytes, producing 4 encoded characters each.
fn encode_base64_generic(
    input: &[u8],
    alphabet: &[u8; 64],
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Value, crate::resource::ResourceError> {
    let output = encode_base64_bytes(input, alphabet);
    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Encodes a byte slice using a base64 alphabet and returns the raw bytes.
///
/// Processes input in chunks of 3 bytes, producing 4 encoded characters each.
fn encode_base64_bytes(input: &[u8], alphabet: &[u8; 64]) -> Vec<u8> {
    let len = input.len();
    let output_len = len.div_ceil(3) * 4;
    let mut output = Vec::with_capacity(output_len);

    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        output.push(alphabet[((b0 >> 2) & 0x3F) as usize]);
        output.push(alphabet[(((b0 << 4) | (b1 >> 4)) & 0x3F) as usize]);

        if chunk.len() > 1 {
            output.push(alphabet[(((b1 << 2) | (b2 >> 6)) & 0x3F) as usize]);
        } else {
            output.push(PAD);
        }

        if chunk.len() > 2 {
            output.push(alphabet[(b2 & 0x3F) as usize]);
        } else {
            output.push(PAD);
        }
    }

    output
}

/// Wraps base64-encoded bytes to the legacy 76-character line width.
///
/// Returns empty output for empty input; otherwise inserts `\\n` after each line.
fn wrap_base64_lines(encoded: &[u8]) -> Vec<u8> {
    if encoded.is_empty() {
        return Vec::new();
    }

    let lines = encoded.len().div_ceil(BASE64_LINE_WRAP);
    let mut output = Vec::with_capacity(encoded.len() + lines);

    for chunk in encoded.chunks(BASE64_LINE_WRAP) {
        output.extend_from_slice(chunk);
        output.push(b'\n');
    }

    output
}

/// Wraps an encoded byte stream to a fixed line width without trailing newline.
fn wrap_encoded_lines(encoded: &[u8], line_width: usize) -> Vec<u8> {
    if line_width == 0 || encoded.is_empty() {
        return encoded.to_vec();
    }

    let lines = encoded.len().div_ceil(line_width);
    let mut output = Vec::with_capacity(encoded.len() + lines.saturating_sub(1));
    for (i, chunk) in encoded.chunks(line_width).enumerate() {
        if i > 0 {
            output.push(b'\n');
        }
        output.extend_from_slice(chunk);
    }
    output
}

/// Removes ASCII whitespace bytes from the input.
fn strip_ascii_whitespace(input: &[u8]) -> Vec<u8> {
    input.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect()
}

/// Decodes a base64-encoded byte slice (standard or URL-safe) and allocates the result.
///
/// When `url_safe` is true, accepts `-` and `_` in addition to `+` and `/`.
fn decode_base64_generic(input: &[u8], url_safe: bool, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let len = input.len();

    if len == 0 {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    if !len.is_multiple_of(4) {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "Invalid base64-encoded string: length must be a multiple of 4",
        )
        .into());
    }

    let mut padding = 0;
    if len >= 2 && input[len - 1] == PAD {
        padding += 1;
        if input[len - 2] == PAD {
            padding += 1;
        }
    }
    let output_len = (len / 4) * 3 - padding;
    let mut output = Vec::with_capacity(output_len);

    for chunk in input.chunks(4) {
        let c0 = decode_base64_char(chunk[0], url_safe)?;
        let c1 = decode_base64_char(chunk[1], url_safe)?;
        let c2 = if chunk[2] == PAD {
            0
        } else {
            decode_base64_char(chunk[2], url_safe)?
        };
        let c3 = if chunk[3] == PAD {
            0
        } else {
            decode_base64_char(chunk[3], url_safe)?
        };

        output.push((c0 << 2) | (c1 >> 4));

        if chunk[2] != PAD {
            output.push((c1 << 4) | (c2 >> 2));
        }
        if chunk[3] != PAD {
            output.push((c2 << 6) | c3);
        }
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes a single base64 character to its 6-bit value.
///
/// When `url_safe` is true, accepts `-` (62) and `_` (63) in addition to `+` and `/`.
fn decode_base64_char(c: u8, url_safe: bool) -> RunResult<u8> {
    match c {
        b'A'..=b'Z' => Ok(c - b'A'),
        b'a'..=b'z' => Ok(c - b'a' + 26),
        b'0'..=b'9' => Ok(c - b'0' + 52),
        b'+' if !url_safe => Ok(62),
        b'/' if !url_safe => Ok(63),
        b'-' if url_safe => Ok(62),
        b'_' if url_safe => Ok(63),
        _ => Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("Invalid base64 character: {:?}", char::from(c)),
        )
        .into()),
    }
}

/// Encodes a byte slice to base32 (RFC 4648) and allocates the result on the heap.
///
/// Processes input in groups of 5 bytes, producing 8 base32 characters each.
/// Pads with `=` to make the output length a multiple of 8.
fn encode_base32(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
    encode_base32_generic(input, BASE32_ALPHABET, heap)
}

/// Encodes a byte slice to base32 using the extended hex alphabet.
fn encode_base32_hex(
    input: &[u8],
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Value, crate::resource::ResourceError> {
    encode_base32_generic(input, BASE32HEX_ALPHABET, heap)
}

/// Decodes base32-encoded bytes (RFC 4648) and allocates the result on the heap.
///
/// Processes input in groups of 8 characters, producing up to 5 bytes each.
fn decode_base32(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    decode_base32_generic(input, BASE32_ALPHABET, "base32", heap)
}

/// Decodes base32 hex encoded bytes and allocates the result on the heap.
fn decode_base32_hex(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    decode_base32_generic(input, BASE32HEX_ALPHABET, "base32hex", heap)
}

/// Encodes a byte slice to base32 using the provided alphabet.
fn encode_base32_generic(
    input: &[u8],
    alphabet: &[u8; 32],
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Value, crate::resource::ResourceError> {
    let output_len = input.len().div_ceil(5) * 8;
    let mut output = Vec::with_capacity(output_len);

    for chunk in input.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);

        // 5 bytes -> 8 base32 characters
        output.push(alphabet[(buf[0] >> 3) as usize]);
        output.push(alphabet[(((buf[0] & 0x07) << 2) | (buf[1] >> 6)) as usize]);

        if chunk.len() > 1 {
            output.push(alphabet[((buf[1] >> 1) & 0x1F) as usize]);
            output.push(alphabet[(((buf[1] & 0x01) << 4) | (buf[2] >> 4)) as usize]);
        } else {
            output.push(PAD);
            output.push(PAD);
        }

        if chunk.len() > 2 {
            output.push(alphabet[(((buf[2] & 0x0F) << 1) | (buf[3] >> 7)) as usize]);
        } else {
            output.push(PAD);
        }

        if chunk.len() > 3 {
            output.push(alphabet[((buf[3] >> 2) & 0x1F) as usize]);
            output.push(alphabet[(((buf[3] & 0x03) << 3) | (buf[4] >> 5)) as usize]);
        } else {
            output.push(PAD);
            output.push(PAD);
        }

        if chunk.len() > 4 {
            output.push(alphabet[(buf[4] & 0x1F) as usize]);
        } else {
            output.push(PAD);
        }
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes base32-encoded bytes using the provided alphabet.
///
/// Processes input in groups of 8 characters, producing up to 5 bytes each.
fn decode_base32_generic(
    input: &[u8],
    alphabet: &[u8; 32],
    label: &str,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    if input.is_empty() {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    if !input.len().is_multiple_of(8) {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "Invalid base32-encoded string: length must be a multiple of 8",
        )
        .into());
    }

    let mut output = Vec::with_capacity(input.len() * 5 / 8);

    for chunk in input.chunks(8) {
        let mut vals = [0u8; 8];
        let mut pad_start = 8;
        for (i, &c) in chunk.iter().enumerate() {
            if c == PAD {
                pad_start = pad_start.min(i);
                vals[i] = 0;
            } else {
                vals[i] = decode_base32_value(c, alphabet, label)?;
            }
        }

        // Always produce first byte (need at least 2 chars)
        output.push((vals[0] << 3) | (vals[1] >> 2));

        if pad_start > 2 {
            output.push((vals[1] << 6) | (vals[2] << 1) | (vals[3] >> 4));
        }
        if pad_start > 4 {
            output.push((vals[3] << 4) | (vals[4] >> 1));
        }
        if pad_start > 5 {
            output.push((vals[4] << 7) | (vals[5] << 2) | (vals[6] >> 3));
        }
        if pad_start > 7 {
            output.push((vals[6] << 5) | vals[7]);
        }
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes a single base32 character to its 5-bit value using the provided alphabet.
fn decode_base32_value(c: u8, alphabet: &[u8; 32], label: &str) -> RunResult<u8> {
    for (i, &ch) in alphabet.iter().enumerate() {
        if ch == c {
            #[expect(clippy::cast_possible_truncation, reason = "i is at most 31 (alphabet size is 32)")]
            return Ok(i as u8);
        }
    }

    Err(SimpleException::new_msg(
        ExcType::ValueError,
        format!("Invalid {label} character: {:?}", char::from(c)),
    )
    .into())
}

/// Encodes a byte slice to base16 (uppercase hex) and allocates the result on the heap.
///
/// Each input byte produces exactly 2 uppercase hex characters.
fn encode_base16(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
    let mut output = Vec::with_capacity(input.len() * 2);
    for &b in input {
        output.push(hex_nibble(b >> 4));
        output.push(hex_nibble(b & 0x0F));
    }
    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Converts a nibble (0-15) to its uppercase hex ASCII byte.
fn hex_nibble(n: u8) -> u8 {
    if n < 10 { b'0' + n } else { b'A' + n - 10 }
}

/// Decodes base16 (uppercase hex) encoded bytes and allocates the result on the heap.
///
/// Input must have even length and contain only uppercase hex characters.
fn decode_base16(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if input.is_empty() {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    if !input.len().is_multiple_of(2) {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "Invalid base16-encoded string: odd-length string").into(),
        );
    }

    let mut output = Vec::with_capacity(input.len() / 2);
    for chunk in input.chunks(2) {
        let high = decode_hex_char(chunk[0])?;
        let low = decode_hex_char(chunk[1])?;
        output.push((high << 4) | low);
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes a single hex character (uppercase) to its 4-bit value.
fn decode_hex_char(c: u8) -> RunResult<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("Non-hexadecimal digit found: {:?}", char::from(c)),
        )
        .into()),
    }
}

/// Encodes a byte slice to base85 (RFC 1924 variant) and allocates the result on the heap.
///
/// Processes input in groups of 4 bytes, producing 5 base85 characters each.
/// The last group may be shorter and produces ceil(len*5/4) characters.
fn encode_base85(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
    encode_base85_generic(input, BASE85_ALPHABET, heap)
}

/// Encodes a byte slice to ZeroMQ Z85 and allocates the result on the heap.
///
/// Uses the Z85 alphabet and the same 4-byte to 5-character mapping as base85.
fn encode_z85(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
    encode_base85_generic(input, Z85_ALPHABET, heap)
}

/// Encodes a byte slice to base85 using the provided alphabet.
///
/// Processes input in groups of 4 bytes, producing 5 characters each.
/// The last group may be shorter and produces ceil(len*5/4) characters.
fn encode_base85_generic(
    input: &[u8],
    alphabet: &[u8; 85],
    heap: &mut Heap<impl ResourceTracker>,
) -> Result<Value, crate::resource::ResourceError> {
    if input.is_empty() {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    let output_len = input.len().div_ceil(4) * 5;
    let mut output = Vec::with_capacity(output_len);

    for chunk in input.chunks(4) {
        // Pack up to 4 bytes into a u32 (big-endian, zero-padded)
        let mut acc: u32 = 0;
        for &b in chunk {
            acc = (acc << 8) | u32::from(b);
        }
        // Pad remaining bytes with zeros
        for _ in chunk.len()..4 {
            acc <<= 8;
        }

        // Convert to 5 base85 digits (most significant first)
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = alphabet[(acc % 85) as usize];
            acc /= 85;
        }

        // For partial groups, only output ceil(chunk.len() * 5 / 4) digits
        let out_count = match chunk.len() {
            1 => 2,
            2 => 3,
            3 => 4,
            _ => 5,
        };
        output.extend_from_slice(&digits[..out_count]);
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes base85 (RFC 1924 variant) encoded bytes and allocates the result on the heap.
///
/// Processes input in groups of 5 characters, producing 4 bytes each.
/// The last group may be shorter.
fn decode_base85(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if input.is_empty() {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    let mut output = Vec::with_capacity(input.len() * 4 / 5);

    for chunk in input.chunks(5) {
        // Decode each character to its base85 value
        let mut acc: u64 = 0;
        let mut vals = [0u8; 5];
        for (i, &c) in chunk.iter().enumerate() {
            vals[i] = decode_base85_char(c)?;
        }

        // Pad short groups with the highest value (84) — same as CPython
        for v in vals.iter_mut().skip(chunk.len()) {
            *v = 84;
        }

        for &v in &vals {
            acc = acc * 85 + u64::from(v);
        }

        if acc > u64::from(u32::MAX) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "base85 overflow in chunk").into());
        }

        #[expect(clippy::cast_possible_truncation, reason = "already validated acc <= u32::MAX")]
        let word = acc as u32;
        let bytes = word.to_be_bytes();

        // For partial groups, output ceil(chunk.len() * 4 / 5) bytes
        let out_count = match chunk.len() {
            1 => 0,
            2 => 1,
            3 => 2,
            4 => 3,
            _ => 4,
        };
        output.extend_from_slice(&bytes[..out_count]);
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes a single base85 character to its numeric value (0-84).
///
/// Uses the RFC 1924 alphabet matching CPython's `base64.b85encode/b85decode`.
fn decode_base85_char(c: u8) -> RunResult<u8> {
    decode_base85_value(c, BASE85_ALPHABET).ok_or_else(|| {
        SimpleException::new_msg(
            ExcType::ValueError,
            format!("Invalid base85 character: {:?}", char::from(c)),
        )
        .into()
    })
}

/// Decodes Z85-encoded bytes and allocates the result on the heap.
///
/// Uses the ZeroMQ alphabet and raises `ValueError` on invalid characters.
fn decode_z85(input: &[u8], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if input.is_empty() {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    let mut output = Vec::with_capacity(input.len() * 4 / 5);
    let mut group = [0u8; 5];
    let mut group_len = 0usize;
    let mut group_start = 0usize;

    for (idx, &c) in input.iter().enumerate() {
        let Some(val) = decode_base85_value(c, Z85_ALPHABET) else {
            return Err(
                SimpleException::new_msg(ExcType::ValueError, format!("bad z85 character at position {idx}")).into(),
            );
        };

        if group_len == 0 {
            group_start = idx;
        }
        group[group_len] = val;
        group_len += 1;

        if group_len == 5 {
            decode_z85_group(group, 5, group_start, &mut output)?;
            group_len = 0;
        }
    }

    if group_len > 0 {
        for v in group.iter_mut().skip(group_len) {
            *v = 84;
        }
        decode_z85_group(group, group_len, group_start, &mut output)?;
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes a Z85 group of 1-5 characters into bytes, handling overflow errors.
fn decode_z85_group(group: [u8; 5], group_len: usize, group_start: usize, output: &mut Vec<u8>) -> RunResult<()> {
    let mut acc: u64 = 0;
    for v in group {
        acc = acc * 85 + u64::from(v);
    }

    if acc > u64::from(u32::MAX) {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("z85 overflow in hunk starting at byte {group_start}"),
        )
        .into());
    }

    #[expect(clippy::cast_possible_truncation, reason = "already validated acc <= u32::MAX")]
    let word = acc as u32;
    let bytes = word.to_be_bytes();

    let out_count = match group_len {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    };
    output.extend_from_slice(&bytes[..out_count]);
    Ok(())
}

/// Encodes a byte slice to ASCII85 bytes.
///
/// Uses `z` compression for full zero groups and optionally `y` compression
/// for full space groups when `foldspaces` is enabled.
fn encode_ascii85(input: &[u8], foldspaces: bool) -> Vec<u8> {
    if input.is_empty() {
        return Vec::new();
    }

    let output_len = input.len().div_ceil(4) * 5;
    let mut output = Vec::with_capacity(output_len);

    for chunk in input.chunks(4) {
        if chunk.len() == 4 && chunk.iter().all(|&b| b == 0) {
            output.push(b'z');
            continue;
        }
        if foldspaces && chunk.len() == 4 && chunk.iter().all(|&b| b == b' ') {
            output.push(b'y');
            continue;
        }

        // Pack up to 4 bytes into a u32 (big-endian, zero-padded)
        let mut acc: u32 = 0;
        for &b in chunk {
            acc = (acc << 8) | u32::from(b);
        }
        for _ in chunk.len()..4 {
            acc <<= 8;
        }

        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (acc % 85) as u8;
            acc /= 85;
        }

        let out_count = match chunk.len() {
            1 => 2,
            2 => 3,
            3 => 4,
            _ => 5,
        };
        for &digit in &digits[..out_count] {
            output.push(ASCII85_FIRST + digit);
        }
    }
    output
}

/// Decodes ASCII85-encoded bytes and allocates the result on the heap.
///
/// Accepts `z` for zero groups and ignores ASCII whitespace.
fn decode_ascii85(
    input: &[u8],
    foldspaces: bool,
    adobe: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let payload = if adobe {
        let mut start = 0usize;
        while start < input.len() && input[start].is_ascii_whitespace() {
            start += 1;
        }
        let mut end = input.len();
        while end > start && input[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        if end.saturating_sub(start) < 4 || &input[start..start + 2] != b"<~" || &input[end - 2..end] != b"~>" {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "Ascii85 encoded data must begin with <~ and end with ~>",
            )
            .into());
        }
        &input[start + 2..end - 2]
    } else {
        input
    };

    if payload.is_empty() {
        let id = heap.allocate(HeapData::Bytes(Bytes::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    let mut output = Vec::with_capacity(payload.len() * 4 / 5);
    let mut group = [0u8; 5];
    let mut group_len = 0usize;

    for &c in payload {
        if c.is_ascii_whitespace() {
            continue;
        }

        if c == b'z' {
            if group_len != 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "z inside Ascii85 5-tuple").into());
            }
            output.extend_from_slice(&[0u8; 4]);
            continue;
        }
        if c == b'y' {
            if !foldspaces {
                return Err(SimpleException::new_msg(ExcType::ValueError, "Non-Ascii85 digit found: y").into());
            }
            if group_len != 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "y inside Ascii85 5-tuple").into());
            }
            output.extend_from_slice(b"    ");
            continue;
        }

        if !(ASCII85_FIRST..=ASCII85_LAST).contains(&c) {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("Non-Ascii85 digit found: {}", char::from(c)),
            )
            .into());
        }

        group[group_len] = c - ASCII85_FIRST;
        group_len += 1;

        if group_len == 5 {
            decode_ascii85_group(group, 5, &mut output)?;
            group_len = 0;
        }
    }

    if group_len > 0 {
        for v in group.iter_mut().skip(group_len) {
            *v = 84;
        }
        decode_ascii85_group(group, group_len, &mut output)?;
    }

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Decodes an ASCII85 group of 1-5 characters into bytes, handling overflow errors.
fn decode_ascii85_group(group: [u8; 5], group_len: usize, output: &mut Vec<u8>) -> RunResult<()> {
    let mut acc: u64 = 0;
    for v in group {
        acc = acc * 85 + u64::from(v);
    }

    if acc > u64::from(u32::MAX) {
        return Err(SimpleException::new_msg(ExcType::ValueError, "Ascii85 overflow").into());
    }

    #[expect(clippy::cast_possible_truncation, reason = "already validated acc <= u32::MAX")]
    let word = acc as u32;
    let bytes = word.to_be_bytes();

    let out_count = match group_len {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    };
    output.extend_from_slice(&bytes[..out_count]);
    Ok(())
}

/// Decodes a base85 character to its numeric value (0-84) using the provided alphabet.
fn decode_base85_value(c: u8, alphabet: &[u8; 85]) -> Option<u8> {
    // Linear search is fine for 85 characters; the alphabet is not contiguous.
    for (i, &ch) in alphabet.iter().enumerate() {
        if ch == c {
            #[expect(clippy::cast_possible_truncation, reason = "i is at most 84 (alphabet size is 85)")]
            return Some(i as u8);
        }
    }
    None
}
