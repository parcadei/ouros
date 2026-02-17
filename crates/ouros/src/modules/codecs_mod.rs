//! Compatibility implementation of Python's `codecs` module.
//!
//! This module exposes the public `codecs` API surface used by stdlib callers and
//! parity tests. Ouros keeps the implementation sandbox-safe (no host file access)
//! and focuses on deterministic behavior for common codec workflows.

use std::{collections::HashMap, fmt::Write as _};

use smallvec::smallvec;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::{BuiltinModule, ModuleFunctions},
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Bytes, Dict, List, Module, OurosIter, PyTrait, Str, Type, allocate_tuple},
    value::Value,
};

/// `codecs` module functions implemented by Ouros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum CodecsFunctions {
    Encode,
    Decode,
    Lookup,
    Register,
    Unregister,
    LookupError,
    RegisterError,
    Getencoder,
    Getdecoder,
    Getincrementalencoder,
    Getincrementaldecoder,
    Getreader,
    Getwriter,
    Iterencode,
    Iterdecode,
    Open,
    EncodedFile,
    #[strum(serialize = "ascii_encode")]
    AsciiEncode,
    #[strum(serialize = "ascii_decode")]
    AsciiDecode,
    #[strum(serialize = "latin_1_encode")]
    Latin1Encode,
    #[strum(serialize = "latin_1_decode")]
    Latin1Decode,
    #[strum(serialize = "utf_8_encode")]
    Utf8Encode,
    #[strum(serialize = "utf_8_decode")]
    Utf8Decode,
    #[strum(serialize = "utf_7_encode")]
    Utf7Encode,
    #[strum(serialize = "utf_7_decode")]
    Utf7Decode,
    #[strum(serialize = "utf_16_encode")]
    Utf16Encode,
    #[strum(serialize = "utf_16_decode")]
    Utf16Decode,
    #[strum(serialize = "utf_16_ex_decode")]
    Utf16ExDecode,
    #[strum(serialize = "utf_16_le_encode")]
    Utf16LeEncode,
    #[strum(serialize = "utf_16_le_decode")]
    Utf16LeDecode,
    #[strum(serialize = "utf_16_be_encode")]
    Utf16BeEncode,
    #[strum(serialize = "utf_16_be_decode")]
    Utf16BeDecode,
    #[strum(serialize = "utf_32_encode")]
    Utf32Encode,
    #[strum(serialize = "utf_32_decode")]
    Utf32Decode,
    #[strum(serialize = "utf_32_ex_decode")]
    Utf32ExDecode,
    #[strum(serialize = "utf_32_le_encode")]
    Utf32LeEncode,
    #[strum(serialize = "utf_32_le_decode")]
    Utf32LeDecode,
    #[strum(serialize = "utf_32_be_encode")]
    Utf32BeEncode,
    #[strum(serialize = "utf_32_be_decode")]
    Utf32BeDecode,
    #[strum(serialize = "unicode_escape_encode")]
    UnicodeEscapeEncode,
    #[strum(serialize = "unicode_escape_decode")]
    UnicodeEscapeDecode,
    #[strum(serialize = "raw_unicode_escape_encode")]
    RawUnicodeEscapeEncode,
    #[strum(serialize = "raw_unicode_escape_decode")]
    RawUnicodeEscapeDecode,
    #[strum(serialize = "escape_encode")]
    EscapeEncode,
    #[strum(serialize = "escape_decode")]
    EscapeDecode,
    #[strum(serialize = "charmap_encode")]
    CharmapEncode,
    #[strum(serialize = "charmap_decode")]
    CharmapDecode,
    #[strum(serialize = "charmap_build")]
    CharmapBuild,
    #[strum(serialize = "make_encoding_map")]
    MakeEncodingMap,
    #[strum(serialize = "make_identity_dict")]
    MakeIdentityDict,
    #[strum(serialize = "readbuffer_encode")]
    ReadbufferEncode,
    #[strum(serialize = "strict_errors")]
    StrictErrors,
    #[strum(serialize = "ignore_errors")]
    IgnoreErrors,
    #[strum(serialize = "replace_errors")]
    ReplaceErrors,
    #[strum(serialize = "backslashreplace_errors")]
    BackslashreplaceErrors,
    #[strum(serialize = "xmlcharrefreplace_errors")]
    XmlcharrefreplaceErrors,
    #[strum(serialize = "namereplace_errors")]
    NamereplaceErrors,
}

/// Endianness used by UTF-16/UTF-32 codec helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Little,
    Big,
}

/// Normalized codec identities handled by this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodecKind {
    Ascii,
    Latin1,
    Utf8,
    Utf7,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
    UnicodeEscape,
    RawUnicodeEscape,
    Escape,
    Charmap,
}

/// Creates the `codecs` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Codecs);

    let functions: [(&str, CodecsFunctions); 57] = [
        ("encode", CodecsFunctions::Encode),
        ("decode", CodecsFunctions::Decode),
        ("lookup", CodecsFunctions::Lookup),
        ("register", CodecsFunctions::Register),
        ("unregister", CodecsFunctions::Unregister),
        ("lookup_error", CodecsFunctions::LookupError),
        ("register_error", CodecsFunctions::RegisterError),
        ("getencoder", CodecsFunctions::Getencoder),
        ("getdecoder", CodecsFunctions::Getdecoder),
        ("getincrementalencoder", CodecsFunctions::Getincrementalencoder),
        ("getincrementaldecoder", CodecsFunctions::Getincrementaldecoder),
        ("getreader", CodecsFunctions::Getreader),
        ("getwriter", CodecsFunctions::Getwriter),
        ("iterencode", CodecsFunctions::Iterencode),
        ("iterdecode", CodecsFunctions::Iterdecode),
        ("open", CodecsFunctions::Open),
        ("EncodedFile", CodecsFunctions::EncodedFile),
        ("ascii_encode", CodecsFunctions::AsciiEncode),
        ("ascii_decode", CodecsFunctions::AsciiDecode),
        ("latin_1_encode", CodecsFunctions::Latin1Encode),
        ("latin_1_decode", CodecsFunctions::Latin1Decode),
        ("utf_8_encode", CodecsFunctions::Utf8Encode),
        ("utf_8_decode", CodecsFunctions::Utf8Decode),
        ("utf_7_encode", CodecsFunctions::Utf7Encode),
        ("utf_7_decode", CodecsFunctions::Utf7Decode),
        ("utf_16_encode", CodecsFunctions::Utf16Encode),
        ("utf_16_decode", CodecsFunctions::Utf16Decode),
        ("utf_16_ex_decode", CodecsFunctions::Utf16ExDecode),
        ("utf_16_le_encode", CodecsFunctions::Utf16LeEncode),
        ("utf_16_le_decode", CodecsFunctions::Utf16LeDecode),
        ("utf_16_be_encode", CodecsFunctions::Utf16BeEncode),
        ("utf_16_be_decode", CodecsFunctions::Utf16BeDecode),
        ("utf_32_encode", CodecsFunctions::Utf32Encode),
        ("utf_32_decode", CodecsFunctions::Utf32Decode),
        ("utf_32_ex_decode", CodecsFunctions::Utf32ExDecode),
        ("utf_32_le_encode", CodecsFunctions::Utf32LeEncode),
        ("utf_32_le_decode", CodecsFunctions::Utf32LeDecode),
        ("utf_32_be_encode", CodecsFunctions::Utf32BeEncode),
        ("utf_32_be_decode", CodecsFunctions::Utf32BeDecode),
        ("unicode_escape_encode", CodecsFunctions::UnicodeEscapeEncode),
        ("unicode_escape_decode", CodecsFunctions::UnicodeEscapeDecode),
        ("raw_unicode_escape_encode", CodecsFunctions::RawUnicodeEscapeEncode),
        ("raw_unicode_escape_decode", CodecsFunctions::RawUnicodeEscapeDecode),
        ("escape_encode", CodecsFunctions::EscapeEncode),
        ("escape_decode", CodecsFunctions::EscapeDecode),
        ("charmap_encode", CodecsFunctions::CharmapEncode),
        ("charmap_decode", CodecsFunctions::CharmapDecode),
        ("charmap_build", CodecsFunctions::CharmapBuild),
        ("make_encoding_map", CodecsFunctions::MakeEncodingMap),
        ("make_identity_dict", CodecsFunctions::MakeIdentityDict),
        ("readbuffer_encode", CodecsFunctions::ReadbufferEncode),
        ("strict_errors", CodecsFunctions::StrictErrors),
        ("ignore_errors", CodecsFunctions::IgnoreErrors),
        ("replace_errors", CodecsFunctions::ReplaceErrors),
        ("backslashreplace_errors", CodecsFunctions::BackslashreplaceErrors),
        ("xmlcharrefreplace_errors", CodecsFunctions::XmlcharrefreplaceErrors),
        ("namereplace_errors", CodecsFunctions::NamereplaceErrors),
    ];

    for (name, function) in functions {
        register_attr(&mut module, name, function, heap, interns)?;
    }

    // CPython exposes these helper classes from codecs.
    for class_name in [
        "Codec",
        "CodecInfo",
        "IncrementalEncoder",
        "IncrementalDecoder",
        "BufferedIncrementalEncoder",
        "BufferedIncrementalDecoder",
        "StreamReader",
        "StreamWriter",
        "StreamReaderWriter",
        "StreamRecoder",
    ] {
        module.set_attr_text(class_name, Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    }

    // `codecs.builtins` and `codecs.sys` are module attributes in CPython.
    let builtins_id = BuiltinModule::BuiltinsMod.create(heap, interns)?;
    module.set_attr_text("builtins", Value::Ref(builtins_id), heap, interns)?;
    let sys_id = BuiltinModule::Sys.create(heap, interns)?;
    module.set_attr_text("sys", Value::Ref(sys_id), heap, interns)?;

    // BOM constants mirror CPython aliases.
    let bom_utf16_native: &[u8] = if cfg!(target_endian = "little") {
        &[0xff, 0xfe]
    } else {
        &[0xfe, 0xff]
    };
    let bom_utf32_native: &[u8] = if cfg!(target_endian = "little") {
        &[0xff, 0xfe, 0x00, 0x00]
    } else {
        &[0x00, 0x00, 0xfe, 0xff]
    };

    set_bytes_constant(&mut module, "BOM", bom_utf16_native, heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF16", bom_utf16_native, heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF32", bom_utf32_native, heap, interns)?;
    set_bytes_constant(&mut module, "BOM32_BE", &[0xfe, 0xff], heap, interns)?;
    set_bytes_constant(&mut module, "BOM32_LE", &[0xff, 0xfe], heap, interns)?;
    set_bytes_constant(&mut module, "BOM64_BE", &[0x00, 0x00, 0xfe, 0xff], heap, interns)?;
    set_bytes_constant(&mut module, "BOM64_LE", &[0xff, 0xfe, 0x00, 0x00], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_BE", &[0xfe, 0xff], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_LE", &[0xff, 0xfe], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF16_BE", &[0xfe, 0xff], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF16_LE", &[0xff, 0xfe], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF32_BE", &[0x00, 0x00, 0xfe, 0xff], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF32_LE", &[0xff, 0xfe, 0x00, 0x00], heap, interns)?;
    set_bytes_constant(&mut module, "BOM_UTF8", &[0xef, 0xbb, 0xbf], heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `codecs` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: CodecsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        CodecsFunctions::Encode => encode_api(heap, interns, args)?,
        CodecsFunctions::Decode => decode_api(heap, interns, args)?,
        CodecsFunctions::Lookup => lookup_api(heap, interns, args)?,
        CodecsFunctions::Register => register_api(heap, interns, args)?,
        CodecsFunctions::Unregister => unregister_api(heap, interns, args)?,
        CodecsFunctions::LookupError => lookup_error_api(heap, interns, args)?,
        CodecsFunctions::RegisterError => register_error_api(heap, interns, args)?,
        CodecsFunctions::Getencoder => get_encoder_like_api(heap, interns, args, true)?,
        CodecsFunctions::Getdecoder => get_encoder_like_api(heap, interns, args, false)?,
        CodecsFunctions::Getincrementalencoder
        | CodecsFunctions::Getincrementaldecoder
        | CodecsFunctions::Getreader
        | CodecsFunctions::Getwriter => get_incremental_reader_writer_api(heap, interns, args)?,
        CodecsFunctions::Iterencode => iterencode_api(heap, interns, args)?,
        CodecsFunctions::Iterdecode => iterdecode_api(heap, interns, args)?,
        CodecsFunctions::Open => sandboxed_io_error(heap, args, "codecs.open() is unavailable in Ouros sandbox")?,
        CodecsFunctions::EncodedFile => {
            sandboxed_io_error(heap, args, "codecs.EncodedFile() is unavailable in Ouros sandbox")?
        }
        CodecsFunctions::AsciiEncode => ascii_encode_api(heap, interns, args)?,
        CodecsFunctions::AsciiDecode => ascii_decode_api(heap, interns, args)?,
        CodecsFunctions::Latin1Encode => latin1_encode_api(heap, interns, args)?,
        CodecsFunctions::Latin1Decode => latin1_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf8Encode => utf8_encode_api(heap, interns, args)?,
        CodecsFunctions::Utf8Decode => utf8_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf7Encode => utf7_encode_api(heap, interns, args)?,
        CodecsFunctions::Utf7Decode => utf7_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf16Encode => utf16_encode_api(heap, interns, args)?,
        CodecsFunctions::Utf16Decode => utf16_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf16ExDecode => utf16_ex_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf16LeEncode => utf16_endian_encode_api(heap, interns, args, Endian::Little)?,
        CodecsFunctions::Utf16LeDecode => utf16_endian_decode_api(heap, interns, args, Endian::Little)?,
        CodecsFunctions::Utf16BeEncode => utf16_endian_encode_api(heap, interns, args, Endian::Big)?,
        CodecsFunctions::Utf16BeDecode => utf16_endian_decode_api(heap, interns, args, Endian::Big)?,
        CodecsFunctions::Utf32Encode => utf32_encode_api(heap, interns, args)?,
        CodecsFunctions::Utf32Decode => utf32_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf32ExDecode => utf32_ex_decode_api(heap, interns, args)?,
        CodecsFunctions::Utf32LeEncode => utf32_endian_encode_api(heap, interns, args, Endian::Little)?,
        CodecsFunctions::Utf32LeDecode => utf32_endian_decode_api(heap, interns, args, Endian::Little)?,
        CodecsFunctions::Utf32BeEncode => utf32_endian_encode_api(heap, interns, args, Endian::Big)?,
        CodecsFunctions::Utf32BeDecode => utf32_endian_decode_api(heap, interns, args, Endian::Big)?,
        CodecsFunctions::UnicodeEscapeEncode => unicode_escape_encode_api(heap, interns, args)?,
        CodecsFunctions::UnicodeEscapeDecode => unicode_escape_decode_api(heap, interns, args, false)?,
        CodecsFunctions::RawUnicodeEscapeEncode => raw_unicode_escape_encode_api(heap, interns, args)?,
        CodecsFunctions::RawUnicodeEscapeDecode => unicode_escape_decode_api(heap, interns, args, true)?,
        CodecsFunctions::EscapeEncode => escape_encode_api(heap, interns, args)?,
        CodecsFunctions::EscapeDecode => escape_decode_api(heap, interns, args)?,
        CodecsFunctions::CharmapEncode => charmap_encode_api(heap, interns, args)?,
        CodecsFunctions::CharmapDecode => charmap_decode_api(heap, interns, args)?,
        CodecsFunctions::CharmapBuild => charmap_build_api(heap, interns, args)?,
        CodecsFunctions::MakeEncodingMap => make_encoding_map_api(heap, interns, args)?,
        CodecsFunctions::MakeIdentityDict => make_identity_dict_api(heap, interns, args)?,
        CodecsFunctions::ReadbufferEncode => readbuffer_encode_api(heap, interns, args)?,
        CodecsFunctions::StrictErrors => strict_errors_api(heap, args)?,
        CodecsFunctions::IgnoreErrors => ignore_errors_api(heap, args)?,
        CodecsFunctions::ReplaceErrors => replace_errors_api(heap, args)?,
        CodecsFunctions::BackslashreplaceErrors => backslashreplace_errors_api(heap, args)?,
        CodecsFunctions::XmlcharrefreplaceErrors => xmlcharrefreplace_errors_api(heap, args)?,
        CodecsFunctions::NamereplaceErrors => namereplace_errors_api(heap, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Implements `codecs.encode(obj, encoding='utf-8', errors='strict')`.
fn encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (obj, encoding, errors) = parse_encode_decode_args("encode", args, heap, interns)?;
    defer_drop!(obj, heap);
    let kind = lookup_codec_kind(&encoding)?;
    encode_with_kind(kind, obj, &errors, heap, interns)
}

/// Implements `codecs.decode(obj, encoding='utf-8', errors='strict')`.
fn decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (obj, encoding, errors) = parse_encode_decode_args("decode", args, heap, interns)?;
    defer_drop!(obj, heap);
    let kind = lookup_codec_kind(&encoding)?;
    let (decoded, _) = decode_with_kind(kind, obj, &errors, true, heap, interns)?;
    Ok(decoded)
}

/// Implements `codecs.lookup(encoding)`.
fn lookup_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let encoding_value = args.get_one_arg("lookup", heap)?;
    defer_drop!(encoding_value, heap);
    let encoding = extract_string_arg(
        "lookup",
        "argument",
        "lookup() argument must be str",
        encoding_value,
        heap,
        interns,
    )?;
    let kind = lookup_codec_kind(&encoding)?;
    codec_info_tuple(kind, heap)
}

/// Implements `codecs.register(search_function)`.
fn register_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let callable = args.get_one_arg("register", heap)?;
    defer_drop!(callable, heap);
    if !is_value_callable(callable, heap, interns) {
        return Err(ExcType::type_error("argument must be callable"));
    }
    Ok(Value::None)
}

/// Implements `codecs.unregister(search_function)`.
fn unregister_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let callable = args.get_one_arg("unregister", heap)?;
    defer_drop!(callable, heap);
    if !is_value_callable(callable, heap, interns) {
        return Err(ExcType::type_error("argument must be callable"));
    }
    Ok(Value::None)
}

/// Implements `codecs.register_error(name, handler)`.
fn register_error_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (name, handler) = args.get_two_args("register_error", heap)?;
    defer_drop!(name, heap);
    defer_drop!(handler, heap);

    let _name_str = extract_string_arg(
        "register_error",
        "1",
        "register_error() argument 1 must be str",
        name,
        heap,
        interns,
    )?;
    if !is_value_callable(handler, heap, interns) {
        return Err(ExcType::type_error("handler must be callable"));
    }
    Ok(Value::None)
}

/// Implements `codecs.lookup_error(name)` for built-in error handlers.
fn lookup_error_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let name_value = args.get_one_arg("lookup_error", heap)?;
    defer_drop!(name_value, heap);
    let name = extract_string_arg(
        "lookup_error",
        "argument",
        "lookup_error() argument must be str",
        name_value,
        heap,
        interns,
    )?;
    match name.as_str() {
        "strict" => Ok(module_function(CodecsFunctions::StrictErrors)),
        "ignore" => Ok(module_function(CodecsFunctions::IgnoreErrors)),
        "replace" => Ok(module_function(CodecsFunctions::ReplaceErrors)),
        "backslashreplace" => Ok(module_function(CodecsFunctions::BackslashreplaceErrors)),
        "xmlcharrefreplace" => Ok(module_function(CodecsFunctions::XmlcharrefreplaceErrors)),
        "namereplace" => Ok(module_function(CodecsFunctions::NamereplaceErrors)),
        _ => Err(ExcType::lookup_error_unknown_error_handler(&name)),
    }
}

/// Implements `codecs.getencoder()` / `codecs.getdecoder()`.
fn get_encoder_like_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    encode: bool,
) -> RunResult<Value> {
    let encoding_value = args.get_one_arg(if encode { "getencoder" } else { "getdecoder" }, heap)?;
    defer_drop!(encoding_value, heap);
    let encoding = extract_string_arg(
        if encode { "getencoder" } else { "getdecoder" },
        "argument",
        if encode {
            "getencoder() argument must be str"
        } else {
            "getdecoder() argument must be str"
        },
        encoding_value,
        heap,
        interns,
    )?;
    let kind = lookup_codec_kind(&encoding)?;
    if encode {
        Ok(module_function(encoder_function_for_kind(kind)))
    } else {
        Ok(module_function(decoder_function_for_kind(kind)))
    }
}

/// Implements `codecs.getincremental*`, `getreader`, and `getwriter`.
fn get_incremental_reader_writer_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let encoding_value = args.get_one_arg("codecs helper", heap)?;
    defer_drop!(encoding_value, heap);
    let encoding = extract_string_arg(
        "codecs helper",
        "argument",
        "encoding must be str",
        encoding_value,
        heap,
        interns,
    )?;
    let _ = lookup_codec_kind(&encoding)?;
    Ok(Value::Builtin(Builtins::Type(Type::Object)))
}

/// Implements `codecs.iterencode(iterator, encoding, errors='strict', **kwargs)`.
fn iterencode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (iterable, encoding, errors) = parse_iterencode_decode_args("iterencode", args, heap, interns)?;
    defer_drop!(iterable, heap);
    let kind = lookup_codec_kind(&encoding)?;

    let mut iter = OurosIter::new(iterable.clone_with_heap(heap), heap, interns)?;
    let mut out = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let encoded = encode_with_kind(kind, &item, &errors, heap, interns)?;
                item.drop_with_heap(heap);
                out.push(encoded);
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    let list_id = heap.allocate(HeapData::List(List::new(out)))?;
    Ok(Value::Ref(list_id))
}

/// Implements `codecs.iterdecode(iterator, encoding, errors='strict', **kwargs)`.
fn iterdecode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (iterable, encoding, errors) = parse_iterencode_decode_args("iterdecode", args, heap, interns)?;
    defer_drop!(iterable, heap);
    let kind = lookup_codec_kind(&encoding)?;

    let mut iter = OurosIter::new(iterable.clone_with_heap(heap), heap, interns)?;
    let mut out = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let (decoded, _) = decode_with_kind(kind, &item, &errors, true, heap, interns)?;
                item.drop_with_heap(heap);
                out.push(decoded);
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    let list_id = heap.allocate(HeapData::List(List::new(out)))?;
    Ok(Value::Ref(list_id))
}

/// Implements `ascii_encode(str, errors=None)`.
fn ascii_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("ascii_encode", args, heap, interns)?;
    let encoded = encode_ascii(&text, &errors)?;
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `ascii_decode(data, errors=None)`.
fn ascii_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, _final) = parse_bytes_errors_final_args("ascii_decode", args, heap, interns, false)?;
    let (decoded, consumed) = decode_ascii(&data, &errors)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `latin_1_encode(str, errors=None)`.
fn latin1_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("latin_1_encode", args, heap, interns)?;
    let encoded = encode_latin1(&text, &errors)?;
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `latin_1_decode(data, errors=None)`.
fn latin1_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, _errors, _final) = parse_bytes_errors_final_args("latin_1_decode", args, heap, interns, false)?;
    let decoded: String = data.iter().map(|&b| char::from(b)).collect();
    tuple_str_len(decoded, data.len(), heap)
}

/// Implements `utf_8_encode(str, errors=None)`.
fn utf8_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("utf_8_encode", args, heap, interns)?;
    validate_encode_errors(&errors, true)?;
    let char_len = text.chars().count();
    tuple_bytes_len(text.into_bytes(), char_len, heap)
}

/// Implements `utf_8_decode(data, errors=None, final=False)`.
fn utf8_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args("utf_8_decode", args, heap, interns, true)?;
    let (decoded, consumed) = decode_utf8(&data, &errors, final_flag)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `utf_7_encode(str, errors=None)`.
fn utf7_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("utf_7_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let encoded = encode_utf7(&text);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `utf_7_decode(data, errors=None, final=False)`.
fn utf7_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args("utf_7_decode", args, heap, interns, true)?;
    let (decoded, consumed) = decode_utf7(&data, &errors, final_flag)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `utf_16_encode(str, errors=None, byteorder=0)`.
fn utf16_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, errors, byteorder) = parse_text_errors_byteorder_args("utf_16_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let encoded = encode_utf16(&text, byteorder);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `utf_16_decode(data, errors=None, final=False)`.
fn utf16_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args("utf_16_decode", args, heap, interns, true)?;
    let (decoded, consumed, _byteorder_out) = decode_utf16_ex(&data, &errors, 0, final_flag)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `utf_16_ex_decode(data, errors=None, byteorder=0, final=False)`.
fn utf16_ex_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, byteorder, final_flag) =
        parse_bytes_errors_byteorder_final_args("utf_16_ex_decode", args, heap, interns)?;
    let (decoded, consumed, byteorder_out) = decode_utf16_ex(&data, &errors, byteorder, final_flag)?;
    let decoded_value = allocate_str_value(decoded, heap)?;
    Ok(allocate_tuple(
        smallvec![
            decoded_value,
            Value::Int(usize_to_i64(consumed)),
            Value::Int(byteorder_out)
        ],
        heap,
    )?)
}

/// Implements fixed-endian UTF-16 encode helpers.
fn utf16_endian_encode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    endian: Endian,
) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("utf_16_endian_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let byteorder = if endian == Endian::Little { -1 } else { 1 };
    let encoded = encode_utf16(&text, byteorder);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements fixed-endian UTF-16 decode helpers.
fn utf16_endian_decode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    endian: Endian,
) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args("utf_16_endian_decode", args, heap, interns, true)?;
    let byteorder = if endian == Endian::Little { -1 } else { 1 };
    let (decoded, consumed, _) = decode_utf16_ex(&data, &errors, byteorder, final_flag)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `utf_32_encode(str, errors=None, byteorder=0)`.
fn utf32_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, errors, byteorder) = parse_text_errors_byteorder_args("utf_32_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let encoded = encode_utf32(&text, byteorder);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `utf_32_decode(data, errors=None, final=False)`.
fn utf32_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args("utf_32_decode", args, heap, interns, true)?;
    let (decoded, consumed, _byteorder_out) = decode_utf32_ex(&data, &errors, 0, final_flag)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `utf_32_ex_decode(data, errors=None, byteorder=0, final=False)`.
fn utf32_ex_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, errors, byteorder, final_flag) =
        parse_bytes_errors_byteorder_final_args("utf_32_ex_decode", args, heap, interns)?;
    let (decoded, consumed, byteorder_out) = decode_utf32_ex(&data, &errors, byteorder, final_flag)?;
    let decoded_value = allocate_str_value(decoded, heap)?;
    Ok(allocate_tuple(
        smallvec![
            decoded_value,
            Value::Int(usize_to_i64(consumed)),
            Value::Int(byteorder_out)
        ],
        heap,
    )?)
}

/// Implements fixed-endian UTF-32 encode helpers.
fn utf32_endian_encode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    endian: Endian,
) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("utf_32_endian_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let byteorder = if endian == Endian::Little { -1 } else { 1 };
    let encoded = encode_utf32(&text, byteorder);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements fixed-endian UTF-32 decode helpers.
fn utf32_endian_decode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    endian: Endian,
) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args("utf_32_endian_decode", args, heap, interns, true)?;
    let byteorder = if endian == Endian::Little { -1 } else { 1 };
    let (decoded, consumed, _) = decode_utf32_ex(&data, &errors, byteorder, final_flag)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `unicode_escape_encode(str, errors=None)`.
fn unicode_escape_encode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("unicode_escape_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let encoded = encode_unicode_escape(&text, false);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `raw_unicode_escape_encode(str, errors=None)`.
fn raw_unicode_escape_encode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (text, errors) = parse_text_errors_args("raw_unicode_escape_encode", args, heap, interns)?;
    validate_encode_errors(&errors, false)?;
    let encoded = encode_unicode_escape(&text, true);
    tuple_bytes_len(encoded, text.chars().count(), heap)
}

/// Implements `unicode_escape_decode` and `raw_unicode_escape_decode`.
fn unicode_escape_decode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    raw: bool,
) -> RunResult<Value> {
    let (data, errors, final_flag) = parse_bytes_errors_final_args(
        if raw {
            "raw_unicode_escape_decode"
        } else {
            "unicode_escape_decode"
        },
        args,
        heap,
        interns,
        true,
    )?;
    let (decoded, consumed) = decode_unicode_escape(&data, &errors, final_flag, raw)?;
    tuple_str_len(decoded, consumed, heap)
}

/// Implements `escape_encode(data, errors=None)`.
fn escape_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, _errors, _final) = parse_bytes_errors_final_args("escape_encode", args, heap, interns, false)?;
    let encoded = encode_escape_bytes(&data);
    tuple_bytes_len(encoded, data.len(), heap)
}

/// Implements `escape_decode(data, errors=None)`.
fn escape_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, _errors, _final) = parse_bytes_errors_final_args("escape_decode", args, heap, interns, false)?;
    let (decoded, consumed) = decode_escape_bytes(&data)?;
    tuple_bytes_len(decoded, consumed, heap)
}

/// Implements `charmap_encode(str, errors=None, mapping=None)`.
fn charmap_encode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text_value, errors, mapping) = parse_charmap_args("charmap_encode", args, heap, interns)?;
    defer_drop!(text_value, heap);
    let text = extract_text_arg_for_encode(text_value, "charmap_encode", heap, interns)?;

    let output = if let Some(mapping) = mapping {
        defer_drop!(mapping, heap);
        encode_charmap_with_mapping(&text, &errors, mapping, heap, interns)?
    } else {
        encode_latin1(&text, &errors)?
    };
    tuple_bytes_len(output, text.chars().count(), heap)
}

/// Implements `charmap_decode(data, errors=None, mapping=None)`.
fn charmap_decode_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_value, errors, mapping) = parse_charmap_args("charmap_decode", args, heap, interns)?;
    defer_drop!(data_value, heap);
    let data = extract_bytes_like(data_value, heap, interns)?;

    let (decoded, consumed) = if let Some(mapping) = mapping {
        defer_drop!(mapping, heap);
        decode_charmap_with_mapping(&data, &errors, mapping, heap, interns)?
    } else {
        (data.iter().map(|&b| char::from(b)).collect(), data.len())
    };

    tuple_str_len(decoded, consumed, heap)
}

/// Implements `charmap_build(map)`.
fn charmap_build_api(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let map_value = args.get_one_arg("charmap_build", heap)?;
    defer_drop!(map_value, heap);
    let map = extract_string_arg(
        "charmap_build",
        "argument",
        "charmap_build() argument must be str",
        map_value,
        heap,
        interns,
    )?;

    let mut dict = Dict::new();
    for (index, ch) in map.chars().enumerate() {
        let key = Value::Int(ch as i64);
        let value = Value::Int(usize_to_i64(index));
        if let Some(replaced) = dict.set(key, value, heap, interns).expect("int keys are hashable") {
            replaced.drop_with_heap(heap);
        }
    }

    let id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(id))
}

/// Implements `make_identity_dict(rng)`.
fn make_identity_dict_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let iterable = args.get_one_arg("make_identity_dict", heap)?;
    defer_drop!(iterable, heap);

    let mut iter = OurosIter::new(iterable.clone_with_heap(heap), heap, interns)?;
    let mut dict = Dict::new();

    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let key = item.clone_with_heap(heap);
                let value = item.clone_with_heap(heap);
                if let Some(replaced) = dict.set(key, value, heap, interns)? {
                    replaced.drop_with_heap(heap);
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
    let id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(id))
}

/// Implements `make_encoding_map(decoding_map)`.
fn make_encoding_map_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let mapping_value = args.get_one_arg("make_encoding_map", heap)?;
    defer_drop!(mapping_value, heap);

    let Value::Ref(mapping_id) = mapping_value else {
        return Err(ExcType::type_error("decoding_map must be a dict"));
    };
    let mapping = if let HeapData::Dict(mapping) = heap.get(*mapping_id) {
        mapping
    } else {
        return Err(ExcType::type_error("decoding_map must be a dict"));
    };

    let mut reversed: HashMap<EncodingMapKey, Option<i64>> = HashMap::new();
    for (decode_key, decode_value) in mapping {
        let byte_value = decode_key.as_int(heap)?;
        let Some(encode_key) = decode_map_value_to_encode_key_view(decode_value, heap, interns)? else {
            continue;
        };
        if let Some(existing) = reversed.get_mut(&encode_key) {
            *existing = None;
        } else {
            reversed.insert(encode_key, Some(byte_value));
        }
    }

    let mut out = Dict::new();
    for (encode_key, byte_value) in reversed {
        let key_value = match encode_key {
            EncodingMapKey::Int(value) => Value::Int(value),
            EncodingMapKey::Str(value) => {
                let id = heap.allocate(HeapData::Str(Str::from(value)))?;
                Value::Ref(id)
            }
        };
        let encoded_value = byte_value.map_or(Value::None, Value::Int);
        if let Some(replaced) = out.set(key_value, encoded_value, heap, interns)? {
            replaced.drop_with_heap(heap);
        }
    }

    let out_id = heap.allocate(HeapData::Dict(out))?;
    Ok(Value::Ref(out_id))
}

/// Implements `readbuffer_encode(data, errors=None)`.
fn readbuffer_encode_api(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (data_value, errors_value) =
        args.get_one_two_args_with_keyword("readbuffer_encode", "errors", heap, interns)?;
    defer_drop!(data_value, heap);

    if let Some(errors_value) = errors_value {
        defer_drop!(errors_value, heap);
        if !matches!(errors_value, Value::None)
            && !matches!(errors_value, Value::InternString(_))
            && !matches!(errors_value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Str(_)))
        {
            return Err(ExcType::type_error(format!(
                "readbuffer_encode() argument 2 must be str or None, not {}",
                errors_value.py_type(heap)
            )));
        }
    }

    let (bytes, consumed) = if let Value::InternString(id) = data_value {
        let text = interns.get_str(*id);
        (text.as_bytes().to_vec(), text.chars().count())
    } else {
        let bytes = extract_bytes_like(data_value, heap, interns)?;
        let consumed = bytes.len();
        (bytes, consumed)
    };

    tuple_bytes_len(bytes, consumed, heap)
}

/// Implements `strict_errors(exc)`.
fn strict_errors_api(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let err = args.get_one_arg("strict_errors", heap)?;
    err.drop_with_heap(heap);
    Err(ExcType::type_error("encoding/decoding error in strict mode"))
}

/// Implements `ignore_errors(exc)`.
fn ignore_errors_api(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("ignore_errors", heap)?;
    value.drop_with_heap(heap);
    Err(ExcType::type_error(
        "don't know how to handle UnicodeError in error callback",
    ))
}

/// Implements `replace_errors(exc)`.
fn replace_errors_api(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("replace_errors", heap)?;
    value.drop_with_heap(heap);
    Err(ExcType::type_error(
        "don't know how to handle UnicodeError in error callback",
    ))
}

/// Implements `backslashreplace_errors(exc)`.
fn backslashreplace_errors_api(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("backslashreplace_errors", heap)?;
    value.drop_with_heap(heap);
    Err(ExcType::type_error(
        "don't know how to handle UnicodeError in error callback",
    ))
}

/// Implements `xmlcharrefreplace_errors(exc)`.
fn xmlcharrefreplace_errors_api(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("xmlcharrefreplace_errors", heap)?;
    value.drop_with_heap(heap);
    Err(ExcType::type_error(
        "don't know how to handle UnicodeError in error callback",
    ))
}

/// Implements `namereplace_errors(exc)`.
fn namereplace_errors_api(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("namereplace_errors", heap)?;
    value.drop_with_heap(heap);
    Err(ExcType::type_error(
        "don't know how to handle UnicodeError in error callback",
    ))
}

/// Encodes an object using one normalized codec kind.
fn encode_with_kind(
    kind: CodecKind,
    obj: &Value,
    errors: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match kind {
        CodecKind::Ascii => {
            let text = extract_text_arg_for_encode(obj, "utf_8_encode", heap, interns)?;
            let bytes = encode_ascii(&text, errors)?;
            allocate_bytes_value(bytes, heap)
        }
        CodecKind::Latin1 => {
            let text = extract_text_arg_for_encode(obj, "latin_1_encode", heap, interns)?;
            let bytes = encode_latin1(&text, errors)?;
            allocate_bytes_value(bytes, heap)
        }
        CodecKind::Utf8 => {
            let text = extract_text_arg_for_encode(obj, "utf_8_encode", heap, interns)?;
            validate_encode_errors(errors, true)?;
            allocate_bytes_value(text.into_bytes(), heap)
        }
        CodecKind::Utf7 => {
            let text = extract_text_arg_for_encode(obj, "utf_7_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf7(&text), heap)
        }
        CodecKind::Utf16 => {
            let text = extract_text_arg_for_encode(obj, "utf_16_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf16(&text, 0), heap)
        }
        CodecKind::Utf16Le => {
            let text = extract_text_arg_for_encode(obj, "utf_16_le_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf16(&text, -1), heap)
        }
        CodecKind::Utf16Be => {
            let text = extract_text_arg_for_encode(obj, "utf_16_be_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf16(&text, 1), heap)
        }
        CodecKind::Utf32 => {
            let text = extract_text_arg_for_encode(obj, "utf_32_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf32(&text, 0), heap)
        }
        CodecKind::Utf32Le => {
            let text = extract_text_arg_for_encode(obj, "utf_32_le_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf32(&text, -1), heap)
        }
        CodecKind::Utf32Be => {
            let text = extract_text_arg_for_encode(obj, "utf_32_be_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_utf32(&text, 1), heap)
        }
        CodecKind::UnicodeEscape => {
            let text = extract_text_arg_for_encode(obj, "unicode_escape_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_unicode_escape(&text, false), heap)
        }
        CodecKind::RawUnicodeEscape => {
            let text = extract_text_arg_for_encode(obj, "raw_unicode_escape_encode", heap, interns)?;
            validate_encode_errors(errors, false)?;
            allocate_bytes_value(encode_unicode_escape(&text, true), heap)
        }
        CodecKind::Escape => {
            let data = extract_bytes_like(obj, heap, interns)?;
            allocate_bytes_value(encode_escape_bytes(&data), heap)
        }
        CodecKind::Charmap => {
            let text = extract_text_arg_for_encode(obj, "charmap_encode", heap, interns)?;
            let bytes = encode_latin1(&text, errors)?;
            allocate_bytes_value(bytes, heap)
        }
    }
}

/// Decodes an object using one normalized codec kind.
fn decode_with_kind(
    kind: CodecKind,
    obj: &Value,
    errors: &str,
    final_flag: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, usize)> {
    let (decoded, consumed) = match kind {
        CodecKind::Ascii => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_ascii(&data, errors)?
        }
        CodecKind::Latin1 => {
            let data = extract_bytes_like(obj, heap, interns)?;
            (data.iter().map(|&b| char::from(b)).collect(), data.len())
        }
        CodecKind::Utf8 => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf8(&data, errors, final_flag)?
        }
        CodecKind::Utf7 => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf7(&data, errors, final_flag)?
        }
        CodecKind::Utf16 => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf16_ex(&data, errors, 0, final_flag).map(|(s, n, _)| (s, n))?
        }
        CodecKind::Utf16Le => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf16_ex(&data, errors, -1, final_flag).map(|(s, n, _)| (s, n))?
        }
        CodecKind::Utf16Be => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf16_ex(&data, errors, 1, final_flag).map(|(s, n, _)| (s, n))?
        }
        CodecKind::Utf32 => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf32_ex(&data, errors, 0, final_flag).map(|(s, n, _)| (s, n))?
        }
        CodecKind::Utf32Le => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf32_ex(&data, errors, -1, final_flag).map(|(s, n, _)| (s, n))?
        }
        CodecKind::Utf32Be => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_utf32_ex(&data, errors, 1, final_flag).map(|(s, n, _)| (s, n))?
        }
        CodecKind::UnicodeEscape => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_unicode_escape(&data, errors, final_flag, false)?
        }
        CodecKind::RawUnicodeEscape => {
            let data = extract_bytes_like(obj, heap, interns)?;
            decode_unicode_escape(&data, errors, final_flag, true)?
        }
        CodecKind::Escape => {
            let data = extract_bytes_like(obj, heap, interns)?;
            let (decoded, consumed) = decode_escape_bytes(&data)?;
            let decoded_string = decoded.iter().map(|&b| char::from(b)).collect();
            (decoded_string, consumed)
        }
        CodecKind::Charmap => {
            let data = extract_bytes_like(obj, heap, interns)?;
            (data.iter().map(|&b| char::from(b)).collect(), data.len())
        }
    };

    let decoded_value = allocate_str_value(decoded, heap)?;
    Ok((decoded_value, consumed))
}

/// Resolves a normalized codec kind from an encoding name.
fn lookup_codec_kind(encoding: &str) -> RunResult<CodecKind> {
    codec_kind_from_name(encoding).ok_or_else(|| ExcType::lookup_error_unknown_encoding(encoding))
}

/// Maps a codec name to one supported codec kind.
fn codec_kind_from_name(name: &str) -> Option<CodecKind> {
    let normalized = normalize_encoding_name(name);
    match normalized.as_str() {
        "ascii" | "us_ascii" | "646" => Some(CodecKind::Ascii),
        "latin_1" | "latin1" | "iso8859_1" | "iso_8859_1" | "latin" | "l1" | "cp819" => Some(CodecKind::Latin1),
        "utf_8" | "utf8" | "u8" | "cp65001" => Some(CodecKind::Utf8),
        "utf_7" | "utf7" => Some(CodecKind::Utf7),
        "utf_16" | "utf16" => Some(CodecKind::Utf16),
        "utf_16_le" | "utf16le" => Some(CodecKind::Utf16Le),
        "utf_16_be" | "utf16be" => Some(CodecKind::Utf16Be),
        "utf_32" | "utf32" => Some(CodecKind::Utf32),
        "utf_32_le" | "utf32le" => Some(CodecKind::Utf32Le),
        "utf_32_be" | "utf32be" => Some(CodecKind::Utf32Be),
        "unicode_escape" => Some(CodecKind::UnicodeEscape),
        "raw_unicode_escape" => Some(CodecKind::RawUnicodeEscape),
        "escape" | "string_escape" => Some(CodecKind::Escape),
        "charmap" => Some(CodecKind::Charmap),
        _ => None,
    }
}

/// Returns one codec-specific encoder function variant.
fn encoder_function_for_kind(kind: CodecKind) -> CodecsFunctions {
    match kind {
        CodecKind::Ascii => CodecsFunctions::AsciiEncode,
        CodecKind::Latin1 => CodecsFunctions::Latin1Encode,
        CodecKind::Utf8 => CodecsFunctions::Utf8Encode,
        CodecKind::Utf7 => CodecsFunctions::Utf7Encode,
        CodecKind::Utf16 => CodecsFunctions::Utf16Encode,
        CodecKind::Utf16Le => CodecsFunctions::Utf16LeEncode,
        CodecKind::Utf16Be => CodecsFunctions::Utf16BeEncode,
        CodecKind::Utf32 => CodecsFunctions::Utf32Encode,
        CodecKind::Utf32Le => CodecsFunctions::Utf32LeEncode,
        CodecKind::Utf32Be => CodecsFunctions::Utf32BeEncode,
        CodecKind::UnicodeEscape => CodecsFunctions::UnicodeEscapeEncode,
        CodecKind::RawUnicodeEscape => CodecsFunctions::RawUnicodeEscapeEncode,
        CodecKind::Escape => CodecsFunctions::EscapeEncode,
        CodecKind::Charmap => CodecsFunctions::CharmapEncode,
    }
}

/// Returns one codec-specific decoder function variant.
fn decoder_function_for_kind(kind: CodecKind) -> CodecsFunctions {
    match kind {
        CodecKind::Ascii => CodecsFunctions::AsciiDecode,
        CodecKind::Latin1 => CodecsFunctions::Latin1Decode,
        CodecKind::Utf8 => CodecsFunctions::Utf8Decode,
        CodecKind::Utf7 => CodecsFunctions::Utf7Decode,
        CodecKind::Utf16 => CodecsFunctions::Utf16Decode,
        CodecKind::Utf16Le => CodecsFunctions::Utf16LeDecode,
        CodecKind::Utf16Be => CodecsFunctions::Utf16BeDecode,
        CodecKind::Utf32 => CodecsFunctions::Utf32Decode,
        CodecKind::Utf32Le => CodecsFunctions::Utf32LeDecode,
        CodecKind::Utf32Be => CodecsFunctions::Utf32BeDecode,
        CodecKind::UnicodeEscape => CodecsFunctions::UnicodeEscapeDecode,
        CodecKind::RawUnicodeEscape => CodecsFunctions::RawUnicodeEscapeDecode,
        CodecKind::Escape => CodecsFunctions::EscapeDecode,
        CodecKind::Charmap => CodecsFunctions::CharmapDecode,
    }
}

/// Creates a `CodecInfo`-like tuple value for `codecs.lookup`.
fn codec_info_tuple(kind: CodecKind, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let encoder = module_function(encoder_function_for_kind(kind));
    let decoder = module_function(decoder_function_for_kind(kind));
    let reader = Value::Builtin(Builtins::Type(Type::Object));
    let writer = Value::Builtin(Builtins::Type(Type::Object));
    Ok(allocate_tuple(smallvec![encoder, decoder, reader, writer], heap)?)
}

/// Parses arguments for top-level `codecs.encode` / `codecs.decode`.
fn parse_encode_decode_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, String, String)> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(obj) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 1, 0));
    };

    let mut encoding_value = positional.next();
    let mut errors_value = positional.next();

    if positional.next().is_some() {
        obj.drop_with_heap(heap);
        encoding_value.drop_with_heap(heap);
        errors_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(function_name, 3, 4));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            obj.drop_with_heap(heap);
            encoding_value.drop_with_heap(heap);
            errors_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_string = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_string.as_str() {
            "encoding" => {
                if encoding_value.is_some() {
                    value.drop_with_heap(heap);
                    obj.drop_with_heap(heap);
                    encoding_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "encoding"));
                }
                encoding_value = Some(value);
            }
            "errors" => {
                if errors_value.is_some() {
                    value.drop_with_heap(heap);
                    obj.drop_with_heap(heap);
                    encoding_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "errors"));
                }
                errors_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                obj.drop_with_heap(heap);
                encoding_value.drop_with_heap(heap);
                errors_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, &key_string));
            }
        }
    }

    let encoding = parse_optional_string_default(
        encoding_value,
        "utf-8",
        heap,
        interns,
        format!("{function_name}() argument 'encoding' must be str"),
    )?;
    let errors = match parse_optional_string_default(
        errors_value,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    ) {
        Ok(errors) => errors,
        Err(err) => {
            obj.drop_with_heap(heap);
            return Err(err);
        }
    };
    Ok((obj, encoding, errors))
}

/// Parses arguments for `iterencode` and `iterdecode`.
fn parse_iterencode_decode_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, String, String)> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(iterable) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 2, 0));
    };
    let Some(encoding_value) = positional.next() else {
        iterable.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 2, 1));
    };

    let mut errors_value = positional.next();
    if positional.next().is_some() {
        iterable.drop_with_heap(heap);
        encoding_value.drop_with_heap(heap);
        errors_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(function_name, 3, 4));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            iterable.drop_with_heap(heap);
            encoding_value.drop_with_heap(heap);
            errors_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_string = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        if key_string == "errors" {
            if errors_value.is_some() {
                value.drop_with_heap(heap);
                iterable.drop_with_heap(heap);
                encoding_value.drop_with_heap(heap);
                errors_value.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, "errors"));
            }
            errors_value = Some(value);
        } else {
            // CPython forwards extra kwargs to incremental codecs. Ouros currently
            // ignores the payload while still accepting the public signature.
            value.drop_with_heap(heap);
        }
    }

    let encoding = extract_string_arg(
        function_name,
        "encoding",
        &format!("{function_name}() argument 'encoding' must be str"),
        &encoding_value,
        heap,
        interns,
    )?;
    encoding_value.drop_with_heap(heap);

    let errors = parse_optional_string_default(
        errors_value,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    )?;

    Ok((iterable, encoding, errors))
}

/// Parses `str, errors?` signatures for encoder helpers.
fn parse_text_errors_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String)> {
    let (value, errors) = args.get_one_two_args_with_keyword(function_name, "errors", heap, interns)?;
    defer_drop!(value, heap);
    let text = extract_text_arg_for_encode(value, function_name, heap, interns)?;
    let errors = parse_optional_string_default(
        errors,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    )?;
    Ok((text, errors))
}

/// Parses `data, errors?, final?` signatures for decoder helpers.
fn parse_bytes_errors_final_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    allow_final: bool,
) -> RunResult<(Vec<u8>, String, bool)> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(data_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 1, 0));
    };

    let mut errors_value = positional.next();
    let mut final_value = positional.next();

    if positional.next().is_some() {
        data_value.drop_with_heap(heap);
        errors_value.drop_with_heap(heap);
        final_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(
            function_name,
            if allow_final { 3 } else { 2 },
            4,
        ));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            errors_value.drop_with_heap(heap);
            final_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_string = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_string.as_str() {
            "errors" => {
                if errors_value.is_some() {
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    final_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "errors"));
                }
                errors_value = Some(value);
            }
            "final" if allow_final => {
                if final_value.is_some() {
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    final_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "final"));
                }
                final_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                data_value.drop_with_heap(heap);
                errors_value.drop_with_heap(heap);
                final_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, &key_string));
            }
        }
    }

    let bytes = extract_bytes_like(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let errors = parse_optional_string_default(
        errors_value,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    )?;

    let final_flag = if allow_final {
        if let Some(final_value) = final_value {
            let value = final_value.py_bool(heap, interns);
            final_value.drop_with_heap(heap);
            value
        } else {
            false
        }
    } else {
        final_value.drop_with_heap(heap);
        false
    };

    Ok((bytes, errors, final_flag))
}

/// Parses `str, errors?, byteorder?` signatures.
fn parse_text_errors_byteorder_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String, i64)> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(text_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 1, 0));
    };

    let mut errors_value = positional.next();
    let mut byteorder_value = positional.next();

    if positional.next().is_some() {
        text_value.drop_with_heap(heap);
        errors_value.drop_with_heap(heap);
        byteorder_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(function_name, 3, 4));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            text_value.drop_with_heap(heap);
            errors_value.drop_with_heap(heap);
            byteorder_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_string = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_string.as_str() {
            "errors" => {
                if errors_value.is_some() {
                    value.drop_with_heap(heap);
                    text_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    byteorder_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "errors"));
                }
                errors_value = Some(value);
            }
            "byteorder" => {
                if byteorder_value.is_some() {
                    value.drop_with_heap(heap);
                    text_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    byteorder_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "byteorder"));
                }
                byteorder_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                text_value.drop_with_heap(heap);
                errors_value.drop_with_heap(heap);
                byteorder_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, &key_string));
            }
        }
    }

    let text = extract_text_arg_for_encode(&text_value, function_name, heap, interns)?;
    text_value.drop_with_heap(heap);

    let errors = parse_optional_string_default(
        errors_value,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    )?;

    let byteorder = if let Some(value) = byteorder_value {
        let parsed = value.as_int(heap)?;
        value.drop_with_heap(heap);
        parsed
    } else {
        0
    };

    Ok((text, errors, byteorder))
}

/// Parses `bytes, errors?, byteorder?, final?` signatures.
fn parse_bytes_errors_byteorder_final_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Vec<u8>, String, i64, bool)> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(data_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 1, 0));
    };

    let mut errors_value = positional.next();
    let mut byteorder_value = positional.next();
    let mut final_value = positional.next();

    if positional.next().is_some() {
        data_value.drop_with_heap(heap);
        errors_value.drop_with_heap(heap);
        byteorder_value.drop_with_heap(heap);
        final_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(function_name, 4, 5));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_value.drop_with_heap(heap);
            errors_value.drop_with_heap(heap);
            byteorder_value.drop_with_heap(heap);
            final_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_string = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_string.as_str() {
            "errors" => {
                if errors_value.is_some() {
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    byteorder_value.drop_with_heap(heap);
                    final_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "errors"));
                }
                errors_value = Some(value);
            }
            "byteorder" => {
                if byteorder_value.is_some() {
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    byteorder_value.drop_with_heap(heap);
                    final_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "byteorder"));
                }
                byteorder_value = Some(value);
            }
            "final" => {
                if final_value.is_some() {
                    value.drop_with_heap(heap);
                    data_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    byteorder_value.drop_with_heap(heap);
                    final_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "final"));
                }
                final_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                data_value.drop_with_heap(heap);
                errors_value.drop_with_heap(heap);
                byteorder_value.drop_with_heap(heap);
                final_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, &key_string));
            }
        }
    }

    let bytes = extract_bytes_like(&data_value, heap, interns)?;
    data_value.drop_with_heap(heap);

    let errors = parse_optional_string_default(
        errors_value,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    )?;

    let byteorder = if let Some(value) = byteorder_value {
        let parsed = value.as_int(heap)?;
        value.drop_with_heap(heap);
        parsed
    } else {
        0
    };

    let final_flag = if let Some(value) = final_value {
        let parsed = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        parsed
    } else {
        false
    };

    Ok((bytes, errors, byteorder, final_flag))
}

/// Parses `data, errors?, mapping?` signatures for charmap helpers.
fn parse_charmap_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, String, Option<Value>)> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(first_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(function_name, 1, 0));
    };

    let mut errors_value = positional.next();
    let mut mapping_value = positional.next();
    if positional.next().is_some() {
        first_value.drop_with_heap(heap);
        errors_value.drop_with_heap(heap);
        mapping_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(function_name, 3, 4));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            first_value.drop_with_heap(heap);
            errors_value.drop_with_heap(heap);
            mapping_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_string = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_string.as_str() {
            "errors" => {
                if errors_value.is_some() {
                    value.drop_with_heap(heap);
                    first_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    mapping_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "errors"));
                }
                errors_value = Some(value);
            }
            "mapping" => {
                if mapping_value.is_some() {
                    value.drop_with_heap(heap);
                    first_value.drop_with_heap(heap);
                    errors_value.drop_with_heap(heap);
                    mapping_value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg(function_name, "mapping"));
                }
                mapping_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                first_value.drop_with_heap(heap);
                errors_value.drop_with_heap(heap);
                mapping_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, &key_string));
            }
        }
    }

    let errors = parse_optional_string_default(
        errors_value,
        "strict",
        heap,
        interns,
        format!("{function_name}() argument 'errors' must be str"),
    )?;

    Ok((first_value, errors, mapping_value))
}

/// Parses one optional string value and applies a default.
fn parse_optional_string_default(
    value: Option<Value>,
    default: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    type_error_message: String,
) -> RunResult<String> {
    match value {
        None => Ok(default.to_owned()),
        Some(Value::None) => Ok(default.to_owned()),
        Some(value) => {
            let result = extract_string_arg("codecs", "argument", &type_error_message, &value, heap, interns)?;
            value.drop_with_heap(heap);
            Ok(result)
        }
    }
}

/// Extracts a string argument with one custom message.
fn extract_string_arg(
    _function_name: &str,
    _argument_name: &str,
    type_error_message: &str,
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "{type_error_message}, not {}",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{type_error_message}, not {}",
            value.py_type(heap)
        ))),
    }
}

/// Extracts text for encoding operations.
fn extract_text_arg_for_encode(
    value: &Value,
    function_name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "{function_name}() argument 1 must be str, not {}",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{function_name}() argument 1 must be str, not {}",
            value.py_type(heap)
        ))),
    }
}

/// Extracts bytes-like data from one runtime value.
fn extract_bytes_like(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
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

/// Returns true when a value can be called by the VM.
fn is_value_callable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Marker(marker) => marker.is_callable(),
        Value::Ref(heap_id) => is_heap_value_callable(*heap_id, heap, interns),
        _ => false,
    }
}

/// Returns true when one heap object can be called.
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
            let HeapData::ClassObject(class_obj) = heap.get(instance.class_id()) else {
                return false;
            };
            class_obj.mro_has_attr("__call__", instance.class_id(), heap, interns)
        }
        _ => false,
    }
}

/// Validates supported encoding error handlers.
fn validate_encode_errors(errors: &str, allow_namereplace: bool) -> RunResult<()> {
    if matches!(
        errors,
        "strict" | "ignore" | "replace" | "backslashreplace" | "xmlcharrefreplace" | "surrogateescape"
    ) {
        return Ok(());
    }
    if allow_namereplace && errors == "namereplace" {
        return Ok(());
    }
    Err(ExcType::lookup_error_unknown_error_handler(errors))
}

/// Encodes one string to ASCII using CPython-compatible error handling names.
fn encode_ascii(text: &str, errors: &str) -> RunResult<Vec<u8>> {
    validate_encode_errors(errors, true)?;
    let mut out = Vec::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        if ch.is_ascii() {
            out.push(ch as u8);
            continue;
        }
        let cp = ch as u32;
        match errors {
            "strict" => {
                return Err(SimpleException::new_msg(
                    ExcType::UnicodeDecodeError,
                    format!(
                        "'ascii' codec can't encode character '{}' in position {index}: ordinal not in range(128)",
                        escaped_codepoint(cp)
                    ),
                )
                .into());
            }
            "ignore" => {}
            "replace" => out.push(b'?'),
            "backslashreplace" => out.extend(format!("\\x{cp:02x}").bytes()),
            "xmlcharrefreplace" => out.extend(format!("&#{cp};").bytes()),
            "namereplace" => out.extend(format!("\\N{{U+{cp:04X}}}").bytes()),
            _ => return Err(ExcType::lookup_error_unknown_error_handler(errors)),
        }
    }
    Ok(out)
}

/// Decodes one ASCII byte sequence with common error handlers.
fn decode_ascii(data: &[u8], errors: &str) -> RunResult<(String, usize)> {
    if !matches!(
        errors,
        "strict" | "ignore" | "replace" | "backslashreplace" | "surrogateescape"
    ) {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut out = String::new();
    for (index, &byte) in data.iter().enumerate() {
        if byte <= 0x7f {
            out.push(char::from(byte));
            continue;
        }
        match errors {
            "strict" => {
                return Err(SimpleException::new_msg(
                    ExcType::UnicodeDecodeError,
                    format!(
                        "'ascii' codec can't decode byte 0x{byte:02x} in position {index}: ordinal not in range(128)"
                    ),
                )
                .into());
            }
            "ignore" => {}
            "replace" => out.push('\u{fffd}'),
            "backslashreplace" => push_backslash_x(&mut out, byte),
            "surrogateescape" => {
                out.push(char::from_u32(0xdc00 + u32::from(byte)).expect("valid surrogateescape code point"));
            }
            _ => unreachable!(),
        }
    }
    Ok((out, data.len()))
}

/// Encodes one string to Latin-1.
fn encode_latin1(text: &str, errors: &str) -> RunResult<Vec<u8>> {
    validate_encode_errors(errors, true)?;
    let mut out = Vec::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        let cp = ch as u32;
        if cp <= 0xff {
            out.push(cp as u8);
            continue;
        }
        match errors {
            "strict" => {
                return Err(SimpleException::new_msg(
                    ExcType::UnicodeDecodeError,
                    format!(
                        "'latin-1' codec can't encode character '{}' in position {index}: ordinal not in range(256)",
                        escaped_codepoint(cp)
                    ),
                )
                .into());
            }
            "ignore" => {}
            "replace" => out.push(b'?'),
            "backslashreplace" => out.extend(format!("\\x{cp:02x}").bytes()),
            "xmlcharrefreplace" => out.extend(format!("&#{cp};").bytes()),
            "namereplace" => out.extend(format!("\\N{{U+{cp:04X}}}").bytes()),
            _ => return Err(ExcType::lookup_error_unknown_error_handler(errors)),
        }
    }
    Ok(out)
}

/// Decodes UTF-8 incrementally with CPython-like consumed-byte semantics.
fn decode_utf8(data: &[u8], errors: &str, final_flag: bool) -> RunResult<(String, usize)> {
    if !matches!(
        errors,
        "strict" | "ignore" | "replace" | "backslashreplace" | "surrogateescape"
    ) {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut out = String::new();
    let mut index = 0usize;

    while index < data.len() {
        match std::str::from_utf8(&data[index..]) {
            Ok(valid_tail) => {
                out.push_str(valid_tail);
                index = data.len();
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                if valid > 0 {
                    let valid_slice = &data[index..index + valid];
                    let valid_text = std::str::from_utf8(valid_slice).expect("valid UTF-8 prefix");
                    out.push_str(valid_text);
                    index += valid;
                }

                if let Some(len) = err.error_len() {
                    let bad_bytes = &data[index..index + len];
                    match errors {
                        "strict" => {
                            let bad = bad_bytes[0];
                            return Err(SimpleException::new_msg(
                                ExcType::UnicodeDecodeError,
                                format!(
                                    "'utf-8' codec can't decode byte 0x{bad:02x} in position {index}: invalid start byte"
                                ),
                            )
                            .into());
                        }
                        "ignore" => {}
                        "replace" => out.push('\u{fffd}'),
                        "backslashreplace" => {
                            for &byte in bad_bytes {
                                push_backslash_x(&mut out, byte);
                            }
                        }
                        "surrogateescape" => {
                            for &byte in bad_bytes {
                                out.push(
                                    char::from_u32(0xdc00 + u32::from(byte)).expect("valid surrogateescape code point"),
                                );
                            }
                        }
                        _ => unreachable!(),
                    }
                    index += len;
                } else {
                    // Incomplete trailing sequence.
                    if final_flag {
                        match errors {
                            "strict" => {
                                return Err(SimpleException::new_msg(
                                    ExcType::UnicodeDecodeError,
                                    "'utf-8' codec can't decode bytes in position 0-0: unexpected end of data",
                                )
                                .into());
                            }
                            "ignore" => {
                                index = data.len();
                            }
                            "replace" => {
                                out.push('\u{fffd}');
                                index = data.len();
                            }
                            "backslashreplace" => {
                                for &byte in &data[index..] {
                                    push_backslash_x(&mut out, byte);
                                }
                                index = data.len();
                            }
                            "surrogateescape" => {
                                for &byte in &data[index..] {
                                    out.push(
                                        char::from_u32(0xdc00 + u32::from(byte))
                                            .expect("valid surrogateescape code point"),
                                    );
                                }
                                index = data.len();
                            }
                            _ => unreachable!(),
                        }
                    }
                    break;
                }
            }
        }
    }

    Ok((out, index))
}

/// Encodes UTF-16 with CPython-like `byteorder` behavior.
fn encode_utf16(text: &str, byteorder: i64) -> Vec<u8> {
    let endian = if byteorder > 0 { Endian::Big } else { Endian::Little };

    let mut out = Vec::new();
    if byteorder == 0 {
        if cfg!(target_endian = "little") {
            out.extend([0xff, 0xfe]);
        } else {
            out.extend([0xfe, 0xff]);
        }
    }

    for unit in text.encode_utf16() {
        let bytes = match endian {
            Endian::Little => unit.to_le_bytes(),
            Endian::Big => unit.to_be_bytes(),
        };
        out.extend(bytes);
    }
    out
}

/// Decodes UTF-16 with optional BOM detection and `utf_16_ex_decode` byteorder output.
fn decode_utf16_ex(data: &[u8], errors: &str, byteorder: i64, final_flag: bool) -> RunResult<(String, usize, i64)> {
    if !matches!(errors, "strict" | "ignore" | "replace") {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut endian = if byteorder > 0 { Endian::Big } else { Endian::Little };
    let mut out_byteorder = byteorder;
    let mut offset = 0usize;

    if byteorder == 0 && data.len() >= 2 {
        match (data[0], data[1]) {
            (0xff, 0xfe) => {
                endian = Endian::Little;
                out_byteorder = -1;
                offset = 2;
            }
            (0xfe, 0xff) => {
                endian = Endian::Big;
                out_byteorder = 1;
                offset = 2;
            }
            _ => {
                endian = if cfg!(target_endian = "little") {
                    Endian::Little
                } else {
                    Endian::Big
                };
                out_byteorder = 0;
            }
        }
    }

    let body = &data[offset..];
    let body_len_even = body.len() / 2 * 2;
    if final_flag && body_len_even != body.len() && errors == "strict" {
        let codec_name = if endian == Endian::Little {
            "utf-16-le"
        } else {
            "utf-16-be"
        };
        return Err(SimpleException::new_msg(
            ExcType::UnicodeDecodeError,
            format!("'{codec_name}' codec can't decode byte 0xff in position 0: truncated data"),
        )
        .into());
    }

    let mut units = Vec::with_capacity(body_len_even / 2);
    let mut i = 0usize;
    while i + 1 < body_len_even {
        let unit = match endian {
            Endian::Little => u16::from_le_bytes([body[i], body[i + 1]]),
            Endian::Big => u16::from_be_bytes([body[i], body[i + 1]]),
        };
        units.push(unit);
        i += 2;
    }

    let mut out = String::new();
    for decoded in char::decode_utf16(units.into_iter()) {
        match decoded {
            Ok(ch) => out.push(ch),
            Err(_) => match errors {
                "strict" => {
                    return Err(SimpleException::new_msg(
                        ExcType::UnicodeDecodeError,
                        "'utf-16' codec can't decode bytes: illegal encoding",
                    )
                    .into());
                }
                "ignore" => {}
                "replace" => out.push('\u{fffd}'),
                _ => unreachable!(),
            },
        }
    }

    let consumed = offset + body_len_even;
    Ok((out, consumed, out_byteorder))
}

/// Encodes UTF-32 with CPython-like `byteorder` behavior.
fn encode_utf32(text: &str, byteorder: i64) -> Vec<u8> {
    let endian = if byteorder > 0 { Endian::Big } else { Endian::Little };

    let mut out = Vec::new();
    if byteorder == 0 {
        if cfg!(target_endian = "little") {
            out.extend([0xff, 0xfe, 0x00, 0x00]);
        } else {
            out.extend([0x00, 0x00, 0xfe, 0xff]);
        }
    }

    for ch in text.chars() {
        let cp = ch as u32;
        let bytes = match endian {
            Endian::Little => cp.to_le_bytes(),
            Endian::Big => cp.to_be_bytes(),
        };
        out.extend(bytes);
    }
    out
}

/// Decodes UTF-32 with optional BOM detection and `utf_32_ex_decode` byteorder output.
fn decode_utf32_ex(data: &[u8], errors: &str, byteorder: i64, final_flag: bool) -> RunResult<(String, usize, i64)> {
    if !matches!(errors, "strict" | "ignore" | "replace") {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut endian = if byteorder > 0 { Endian::Big } else { Endian::Little };
    let mut out_byteorder = byteorder;
    let mut offset = 0usize;

    if byteorder == 0 && data.len() >= 4 {
        match (data[0], data[1], data[2], data[3]) {
            (0xff, 0xfe, 0x00, 0x00) => {
                endian = Endian::Little;
                out_byteorder = -1;
                offset = 4;
            }
            (0x00, 0x00, 0xfe, 0xff) => {
                endian = Endian::Big;
                out_byteorder = 1;
                offset = 4;
            }
            _ => {
                endian = if cfg!(target_endian = "little") {
                    Endian::Little
                } else {
                    Endian::Big
                };
                out_byteorder = 0;
            }
        }
    }

    let body = &data[offset..];
    let body_len_aligned = body.len() / 4 * 4;
    if final_flag && body_len_aligned != body.len() && errors == "strict" {
        return Err(SimpleException::new_msg(
            ExcType::UnicodeDecodeError,
            "'utf-32' codec can't decode bytes: truncated data",
        )
        .into());
    }

    let mut out = String::new();
    let mut i = 0usize;
    while i + 3 < body_len_aligned {
        let cp = match endian {
            Endian::Little => u32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]),
            Endian::Big => u32::from_be_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]),
        };

        match char::from_u32(cp) {
            Some(ch) => out.push(ch),
            None => match errors {
                "strict" => {
                    return Err(SimpleException::new_msg(
                        ExcType::UnicodeDecodeError,
                        "'utf-32' codec can't decode bytes: invalid code point",
                    )
                    .into());
                }
                "ignore" => {}
                "replace" => out.push('\u{fffd}'),
                _ => unreachable!(),
            },
        }
        i += 4;
    }

    let consumed = offset + body_len_aligned;
    Ok((out, consumed, out_byteorder))
}

/// Encodes one string as UTF-7 (subset with modified base64).
fn encode_utf7(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = Vec::new();

    for ch in text.chars() {
        if ch == '+' {
            flush_utf7_buffer(&mut out, &mut buffer);
            out.extend_from_slice(b"+-");
            continue;
        }

        let cp = ch as u32;
        if (0x20..=0x7e).contains(&cp) || ch == '\n' || ch == '\r' || ch == '\t' {
            flush_utf7_buffer(&mut out, &mut buffer);
            out.push(cp as u8);
        } else {
            for unit in ch.encode_utf16(&mut [0u16; 2]) {
                buffer.extend(unit.to_be_bytes());
            }
        }
    }
    flush_utf7_buffer(&mut out, &mut buffer);
    out
}

/// Flushes buffered UTF-16BE bytes as one modified base64 UTF-7 block.
fn flush_utf7_buffer(out: &mut Vec<u8>, buffer: &mut Vec<u8>) {
    if buffer.is_empty() {
        return;
    }
    let encoded = modified_base64_encode(buffer);
    out.push(b'+');
    out.extend(encoded);
    out.push(b'-');
    buffer.clear();
}

/// Decodes one UTF-7 byte sequence.
fn decode_utf7(data: &[u8], errors: &str, _final_flag: bool) -> RunResult<(String, usize)> {
    if !matches!(errors, "strict" | "ignore" | "replace") {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut out = String::new();
    let mut i = 0usize;

    while i < data.len() {
        let byte = data[i];
        if byte != b'+' {
            out.push(char::from(byte));
            i += 1;
            continue;
        }

        if i + 1 < data.len() && data[i + 1] == b'-' {
            out.push('+');
            i += 2;
            continue;
        }

        let start = i + 1;
        let mut end = start;
        while end < data.len() && data[end] != b'-' {
            end += 1;
        }

        let segment = &data[start..end];
        let decoded = modified_base64_decode(segment);
        if let Some(decoded_bytes) = decoded {
            let mut units = Vec::new();
            let mut j = 0usize;
            while j + 1 < decoded_bytes.len() {
                units.push(u16::from_be_bytes([decoded_bytes[j], decoded_bytes[j + 1]]));
                j += 2;
            }
            for ch in char::decode_utf16(units.into_iter()) {
                match ch {
                    Ok(ch) => out.push(ch),
                    Err(_) => match errors {
                        "strict" => {
                            return Err(SimpleException::new_msg(
                                ExcType::UnicodeDecodeError,
                                "'utf-7' codec can't decode bytes: invalid sequence",
                            )
                            .into());
                        }
                        "ignore" => {}
                        "replace" => out.push('\u{fffd}'),
                        _ => unreachable!(),
                    },
                }
            }
        } else {
            match errors {
                "strict" => {
                    return Err(SimpleException::new_msg(
                        ExcType::UnicodeDecodeError,
                        "'utf-7' codec can't decode bytes: invalid sequence",
                    )
                    .into());
                }
                "ignore" => {}
                "replace" => out.push('\u{fffd}'),
                _ => unreachable!(),
            }
        }

        i = if end < data.len() { end + 1 } else { end };
    }

    Ok((out, data.len()))
}

/// Encodes bytes using modified UTF-7 base64 alphabet.
fn modified_base64_encode(bytes: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };

        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize]);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize]);
        if i + 1 < bytes.len() {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize]);
        }
        if i + 2 < bytes.len() {
            out.push(ALPHABET[(n & 0x3f) as usize]);
        }

        i += 3;
    }

    out
}

/// Decodes bytes using modified UTF-7 base64 alphabet.
fn modified_base64_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    fn value_of(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b',' => Some(63),
            _ => None,
        }
    }

    let mut out = Vec::new();
    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;

    for &byte in bytes {
        let value = value_of(byte)?;
        chunk[chunk_len] = value;
        chunk_len += 1;

        if chunk_len == 4 {
            let n = (u32::from(chunk[0]) << 18)
                | (u32::from(chunk[1]) << 12)
                | (u32::from(chunk[2]) << 6)
                | u32::from(chunk[3]);
            out.push(((n >> 16) & 0xff) as u8);
            out.push(((n >> 8) & 0xff) as u8);
            out.push((n & 0xff) as u8);
            chunk_len = 0;
        }
    }

    if chunk_len == 2 {
        let n = (u32::from(chunk[0]) << 18) | (u32::from(chunk[1]) << 12);
        out.push(((n >> 16) & 0xff) as u8);
    } else if chunk_len == 3 {
        let n = (u32::from(chunk[0]) << 18) | (u32::from(chunk[1]) << 12) | (u32::from(chunk[2]) << 6);
        out.push(((n >> 16) & 0xff) as u8);
        out.push(((n >> 8) & 0xff) as u8);
    } else if chunk_len == 1 {
        return None;
    }

    Some(out)
}

/// Encodes a string as Python `unicode_escape` or `raw_unicode_escape` bytes.
fn encode_unicode_escape(text: &str, raw: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        match ch {
            '\\' if !raw => out.extend_from_slice(b"\\\\"),
            '\n' if !raw => out.extend_from_slice(b"\\n"),
            '\r' if !raw => out.extend_from_slice(b"\\r"),
            '\t' if !raw => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x80 => out.push(c as u8),
            c => {
                let cp = c as u32;
                if raw && cp <= 0xff {
                    out.push(cp as u8);
                } else if cp <= 0xff {
                    out.extend(format!("\\x{cp:02x}").bytes());
                } else if cp <= 0xffff {
                    out.extend(format!("\\u{cp:04x}").bytes());
                } else {
                    out.extend(format!("\\U{cp:08x}").bytes());
                }
            }
        }
    }
    out
}

/// Decodes `unicode_escape` / `raw_unicode_escape` byte sequences.
fn decode_unicode_escape(data: &[u8], errors: &str, final_flag: bool, raw: bool) -> RunResult<(String, usize)> {
    if !matches!(errors, "strict" | "ignore" | "replace") {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut out = String::new();
    let mut i = 0usize;

    while i < data.len() {
        if data[i] != b'\\' {
            out.push(char::from(data[i]));
            i += 1;
            continue;
        }

        if i + 1 >= data.len() {
            if final_flag && !raw && errors == "strict" {
                return Err(SimpleException::new_msg(
                    ExcType::UnicodeDecodeError,
                    "'unicodeescape' codec can't decode byte 0x5c in position 0: \\ at end of string",
                )
                .into());
            }
            if raw {
                out.push('\\');
                i += 1;
            }
            break;
        }

        let marker = data[i + 1] as char;
        if raw && marker != 'u' && marker != 'U' {
            out.push('\\');
            i += 1;
            continue;
        }

        match marker {
            'n' if !raw => {
                out.push('\n');
                i += 2;
            }
            'r' if !raw => {
                out.push('\r');
                i += 2;
            }
            't' if !raw => {
                out.push('\t');
                i += 2;
            }
            '\\' if !raw => {
                out.push('\\');
                i += 2;
            }
            '\'' if !raw => {
                out.push('\'');
                i += 2;
            }
            '"' if !raw => {
                out.push('"');
                i += 2;
            }
            'x' if !raw => {
                if i + 3 >= data.len() {
                    if final_flag && errors == "strict" {
                        return Err(SimpleException::new_msg(
                            ExcType::UnicodeDecodeError,
                            "'unicodeescape' codec can't decode bytes in position 0-2: truncated \\xXX escape",
                        )
                        .into());
                    }
                    break;
                }
                let hi = hex_to_val(data[i + 2]);
                let lo = hex_to_val(data[i + 3]);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push(char::from((hi << 4) | lo));
                        i += 4;
                    }
                    _ if errors == "strict" => {
                        return Err(SimpleException::new_msg(
                            ExcType::UnicodeDecodeError,
                            "'unicodeescape' codec can't decode bytes in position 0-1: truncated \\xXX escape",
                        )
                        .into());
                    }
                    _ if errors == "replace" => {
                        out.push('\u{fffd}');
                        i += 2;
                    }
                    _ => {
                        i += 2;
                    }
                }
            }
            'u' => {
                if i + 5 >= data.len() {
                    if final_flag && errors == "strict" {
                        let codec = if raw { "rawunicodeescape" } else { "unicodeescape" };
                        return Err(SimpleException::new_msg(
                            ExcType::UnicodeDecodeError,
                            format!("'{codec}' codec can't decode bytes in position 0-3: truncated \\uXXXX escape"),
                        )
                        .into());
                    }
                    break;
                }

                let mut value = 0u32;
                let mut ok = true;
                for offset in 0..4 {
                    if let Some(v) = hex_to_val(data[i + 2 + offset]) {
                        value = (value << 4) | u32::from(v);
                    } else {
                        ok = false;
                        break;
                    }
                }

                if ok {
                    if let Some(ch) = char::from_u32(value) {
                        out.push(ch);
                    } else if errors == "replace" {
                        out.push('\u{fffd}');
                    } else if errors == "strict" {
                        return Err(SimpleException::new_msg(
                            ExcType::UnicodeDecodeError,
                            "invalid Unicode code point",
                        )
                        .into());
                    }
                    i += 6;
                } else if errors == "strict" {
                    return Err(SimpleException::new_msg(ExcType::UnicodeDecodeError, "invalid \\uXXXX escape").into());
                } else if errors == "replace" {
                    out.push('\u{fffd}');
                    i += 2;
                } else {
                    i += 2;
                }
            }
            'U' => {
                if i + 9 >= data.len() {
                    if final_flag && errors == "strict" {
                        let codec = if raw { "rawunicodeescape" } else { "unicodeescape" };
                        return Err(SimpleException::new_msg(
                            ExcType::UnicodeDecodeError,
                            format!("'{codec}' codec can't decode bytes in position 0-9: truncated \\UXXXXXXXX escape"),
                        )
                        .into());
                    }
                    break;
                }

                let mut value = 0u32;
                let mut ok = true;
                for offset in 0..8 {
                    if let Some(v) = hex_to_val(data[i + 2 + offset]) {
                        value = (value << 4) | u32::from(v);
                    } else {
                        ok = false;
                        break;
                    }
                }

                if ok {
                    if let Some(ch) = char::from_u32(value) {
                        out.push(ch);
                    } else if errors == "replace" {
                        out.push('\u{fffd}');
                    } else if errors == "strict" {
                        return Err(SimpleException::new_msg(
                            ExcType::UnicodeDecodeError,
                            "invalid Unicode code point",
                        )
                        .into());
                    }
                    i += 10;
                } else if errors == "strict" {
                    return Err(
                        SimpleException::new_msg(ExcType::UnicodeDecodeError, "invalid \\UXXXXXXXX escape").into(),
                    );
                } else if errors == "replace" {
                    out.push('\u{fffd}');
                    i += 2;
                } else {
                    i += 2;
                }
            }
            _ => {
                if raw {
                    out.push('\\');
                    i += 1;
                } else {
                    out.push(marker);
                    i += 2;
                }
            }
        }
    }

    Ok((out, i))
}

/// Encodes bytes for `escape_encode`.
fn encode_escape_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &byte in data {
        match byte {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\'' => out.extend_from_slice(b"\\'"),
            0x20..=0x7e => out.push(byte),
            _ => out.extend(format!("\\x{byte:02x}").bytes()),
        }
    }
    out
}

/// Decodes bytes for `escape_decode`.
fn decode_escape_bytes(data: &[u8]) -> RunResult<(Vec<u8>, usize)> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0usize;

    while i < data.len() {
        if data[i] != b'\\' {
            out.push(data[i]);
            i += 1;
            continue;
        }

        if i + 1 >= data.len() {
            break;
        }

        match data[i + 1] {
            b'\\' => {
                out.push(b'\\');
                i += 2;
            }
            b'n' => {
                out.push(b'\n');
                i += 2;
            }
            b'r' => {
                out.push(b'\r');
                i += 2;
            }
            b't' => {
                out.push(b'\t');
                i += 2;
            }
            b'x' => {
                if i + 3 >= data.len() {
                    return Err(
                        SimpleException::new_msg(ExcType::ValueError, "invalid \\x escape at position 0").into(),
                    );
                }
                let hi = hex_to_val(data[i + 2]);
                let lo = hex_to_val(data[i + 3]);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi << 4) | lo);
                        i += 4;
                    }
                    _ => {
                        return Err(
                            SimpleException::new_msg(ExcType::ValueError, "invalid \\x escape at position 0").into(),
                        );
                    }
                }
            }
            b'0'..=b'7' => {
                let mut value = 0u16;
                let mut consumed = 0usize;
                while consumed < 3 && i + 1 + consumed < data.len() {
                    let ch = data[i + 1 + consumed];
                    if !(b'0'..=b'7').contains(&ch) {
                        break;
                    }
                    value = (value << 3) | u16::from(ch - b'0');
                    consumed += 1;
                }
                out.push((value & 0xff) as u8);
                i += 1 + consumed;
            }
            other => {
                out.push(other);
                i += 2;
            }
        }
    }

    Ok((out, i))
}

/// Encodes using a charmap mapping object.
fn encode_charmap_with_mapping(
    text: &str,
    errors: &str,
    mapping: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    validate_encode_errors(errors, true)?;

    let mut out = Vec::new();
    for (index, ch) in text.chars().enumerate() {
        let cp = ch as i64;
        let mapped = lookup_mapping_value(mapping, cp, heap, interns)?;
        match mapped {
            Some(MappedValue::Byte(byte)) => out.push(byte),
            Some(MappedValue::Bytes(bytes)) => out.extend(bytes),
            Some(MappedValue::Undefined) | None => match errors {
                "strict" => {
                    return Err(SimpleException::new_msg(
                        ExcType::UnicodeDecodeError,
                        format!(
                            "'charmap' codec can't encode character '{}' in position {index}: character maps to <undefined>",
                            escaped_codepoint(ch as u32)
                        ),
                    )
                    .into())
                }
                "ignore" => {}
                "replace" => out.push(b'?'),
                "backslashreplace" => out.extend(format!("\\x{:02x}", ch as u32).bytes()),
                "xmlcharrefreplace" => out.extend(format!("&#{};", ch as u32).bytes()),
                "namereplace" => out.extend(format!("\\N{{U+{:04X}}}", ch as u32).bytes()),
                _ => return Err(ExcType::lookup_error_unknown_error_handler(errors)),
            },
        }
    }
    Ok(out)
}

/// Decodes using a charmap mapping object.
fn decode_charmap_with_mapping(
    data: &[u8],
    errors: &str,
    mapping: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, usize)> {
    if !matches!(errors, "strict" | "ignore" | "replace") {
        return Err(ExcType::lookup_error_unknown_error_handler(errors));
    }

    let mut out = String::new();
    for (index, &byte) in data.iter().enumerate() {
        let mapped = lookup_mapping_value(mapping, i64::from(byte), heap, interns)?;
        match mapped {
            Some(MappedValue::Byte(b)) => out.push(char::from(b)),
            Some(MappedValue::Bytes(bytes)) => {
                for b in bytes {
                    out.push(char::from(b));
                }
            }
            Some(MappedValue::Undefined) | None => match errors {
                "strict" => {
                    return Err(SimpleException::new_msg(
                        ExcType::UnicodeDecodeError,
                        format!(
                            "'charmap' codec can't decode byte 0x{byte:02x} in position {index}: character maps to <undefined>"
                        ),
                    )
                    .into())
                }
                "ignore" => {}
                "replace" => out.push('\u{fffd}'),
                _ => unreachable!(),
            },
        }
    }

    Ok((out, data.len()))
}

/// One mapping lookup result used by charmap helpers.
enum MappedValue {
    Byte(u8),
    Bytes(Vec<u8>),
    Undefined,
}

/// Looks up one codepoint in a charmap mapping object.
fn lookup_mapping_value(
    mapping: &Value,
    key: i64,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<MappedValue>> {
    let Value::Ref(mapping_id) = mapping else {
        return Ok(None);
    };

    let dict = if let HeapData::Dict(dict) = heap.get(*mapping_id) {
        dict
    } else {
        return Ok(None);
    };

    for (candidate_key, candidate_value) in dict {
        if candidate_key.as_int(heap).ok() == Some(key) {
            let mapped = if matches!(candidate_value, Value::None) {
                Some(MappedValue::Undefined)
            } else if let Ok(v) = candidate_value.as_int(heap) {
                if (0..=255).contains(&v) {
                    Some(MappedValue::Byte(v as u8))
                } else {
                    None
                }
            } else {
                candidate_value
                    .as_either_str(heap)
                    .map(|s| MappedValue::Bytes(s.as_str(interns).as_bytes().to_vec()))
            };
            return Ok(mapped);
        }
    }

    Ok(None)
}

/// Key shape used by `make_encoding_map` while reversing a decoding map.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum EncodingMapKey {
    Int(i64),
    Str(String),
}

/// Converts one decoding-map value to one reversible encoding-map key.
fn decode_map_value_to_encode_key_view(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<EncodingMapKey>> {
    if matches!(value, Value::None) {
        return Ok(None);
    }

    if let Ok(int_value) = value.as_int(heap) {
        return Ok(Some(EncodingMapKey::Int(int_value)));
    }

    if let Some(string_value) = value.as_either_str(heap) {
        let s = string_value.as_str(interns);
        if s.chars().count() == 1 {
            return Ok(Some(EncodingMapKey::Str(s.to_owned())));
        }
        return Ok(None);
    }

    Ok(None)
}

/// Normalizes codec names in CPython style (case-folding and separator handling).
fn normalize_encoding_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut last_was_sep = false;

    for ch in name.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            last_was_sep = false;
            ch.to_ascii_lowercase()
        } else if ch == '-' || ch == ' ' || ch == '\t' {
            if last_was_sep {
                continue;
            }
            last_was_sep = true;
            '_'
        } else {
            ch.to_ascii_lowercase()
        };
        normalized.push(mapped);
    }

    normalized.trim_matches('_').to_owned()
}

/// Allocates one `bytes` runtime value.
fn allocate_bytes_value(bytes: Vec<u8>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(Value::Ref(id))
}

/// Allocates one `str` runtime value.
fn allocate_str_value(text: String, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(id))
}

/// Builds `(bytes, length)` for low-level encode helpers.
fn tuple_bytes_len(bytes: Vec<u8>, length: usize, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let bytes_value = allocate_bytes_value(bytes, heap)?;
    Ok(allocate_tuple(
        smallvec![bytes_value, Value::Int(usize_to_i64(length))],
        heap,
    )?)
}

/// Builds `(str, length)` for low-level decode helpers.
fn tuple_str_len(text: String, length: usize, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let str_value = allocate_str_value(text, heap)?;
    Ok(allocate_tuple(
        smallvec![str_value, Value::Int(usize_to_i64(length))],
        heap,
    )?)
}

/// Converts one `usize` to `i64` for tuple payloads.
fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).expect("length fits into i64")
}

/// Converts one hex digit byte to its numeric value.
fn hex_to_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Appends one `\\xNN` byte escape to a string buffer.
fn push_backslash_x(out: &mut String, byte: u8) {
    write!(out, "\\x{byte:02x}").expect("writing to String cannot fail");
}

/// Renders a Unicode codepoint as CPython-style escaped literal text.
fn escaped_codepoint(cp: u32) -> String {
    if cp <= 0xff {
        format!("\\x{cp:02x}")
    } else if cp <= 0xffff {
        format!("\\u{cp:04x}")
    } else {
        format!("\\U{cp:08x}")
    }
}

/// Creates a module function value for one codecs function.
fn module_function(function: CodecsFunctions) -> Value {
    Value::ModuleFunction(ModuleFunctions::Codecs(function))
}

/// Shared helper for sandboxed APIs that require host I/O.
fn sandboxed_io_error(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, message: &str) -> RunResult<Value> {
    args.drop_with_heap(heap);
    Err(SimpleException::new_msg(ExcType::RuntimeError, message).into())
}

/// Sets one bytes constant on the module.
fn set_bytes_constant(
    module: &mut Module,
    name: &str,
    value: &[u8],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let bytes_id = heap.allocate(HeapData::Bytes(Bytes::new(value.to_vec())))?;
    module.set_attr_text(name, Value::Ref(bytes_id), heap, interns)
}

/// Registers one module-level function.
fn register_attr(
    module: &mut Module,
    name: &str,
    function: CodecsFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Codecs(function)),
        heap,
        interns,
    )
}
