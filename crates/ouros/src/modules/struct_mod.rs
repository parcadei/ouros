//! Implementation of the `struct` module.
//!
//! Provides functions to convert between Python values and C structs:
//! - `pack(fmt, v1, v2, ...)`: Pack values into a bytes object according to format
//! - `unpack(fmt, buffer)`: Unpack bytes into a tuple according to format
//! - `calcsize(fmt)`: Return the size of the struct described by format
//! - `error`: Exception class for struct errors
//!
//! Format string syntax:
//! - Byte order: @ (native), = (native std), < (little-endian), > (big-endian), ! (network = big)
//! - Format chars: x (pad), c (char), b/B (signed/unsigned byte), ? (bool)
//! - More chars: h/H (short), i/I (int), l/L (long), q/Q (long long)
//! - More chars: f (float), d (double), s (string), p (pascal string)
#![expect(clippy::cast_possible_truncation, reason = "narrowing matches struct format widths")]
#![expect(clippy::cast_sign_loss, reason = "unsigned reinterpretation is intentional")]
#![expect(clippy::cast_possible_wrap, reason = "wrapping mirrors CPython struct behavior")]
#![expect(clippy::unreadable_literal, reason = "bounds remain in canonical decimal form")]

use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Bytes, LongInt, PyTrait, Type, allocate_tuple},
    value::Value,
};

/// Struct module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum StructFunctions {
    Pack,
    Unpack,
    Calcsize,
    IterUnpack,
    PackInto,
    UnpackFrom,
}

/// Creates the `struct` module and allocates it on the heap.
///
/// Sets up all struct functions and the error exception.
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

    let mut module = Module::new(StaticStrings::StructMod);

    // struct.pack - pack values into bytes
    module.set_attr(
        StaticStrings::StructPack,
        Value::ModuleFunction(ModuleFunctions::Struct(StructFunctions::Pack)),
        heap,
        interns,
    );

    // struct.unpack - unpack bytes into tuple
    module.set_attr(
        StaticStrings::StructUnpack,
        Value::ModuleFunction(ModuleFunctions::Struct(StructFunctions::Unpack)),
        heap,
        interns,
    );

    // struct.calcsize - return size of struct
    module.set_attr(
        StaticStrings::StructCalcsize,
        Value::ModuleFunction(ModuleFunctions::Struct(StructFunctions::Calcsize)),
        heap,
        interns,
    );

    // struct.iter_unpack - iterator for unpacking
    module.set_attr(
        StaticStrings::StructIterUnpack,
        Value::ModuleFunction(ModuleFunctions::Struct(StructFunctions::IterUnpack)),
        heap,
        interns,
    );

    // struct.pack_into - pack into existing buffer
    module.set_attr(
        StaticStrings::StructPackInto,
        Value::ModuleFunction(ModuleFunctions::Struct(StructFunctions::PackInto)),
        heap,
        interns,
    );

    // struct.unpack_from - unpack from offset
    module.set_attr(
        StaticStrings::StructUnpackFrom,
        Value::ModuleFunction(ModuleFunctions::Struct(StructFunctions::UnpackFrom)),
        heap,
        interns,
    );

    // struct.error - exception class (subclass of Exception)
    // Note: Using Exception directly as struct.error
    module.set_attr(
        StaticStrings::ReError, // ReError serializes to "error"
        Value::Builtin(crate::builtins::Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    );

    // struct.Struct - compiled struct format type
    module.set_attr(
        StaticStrings::StructType,
        Value::Builtin(Builtins::Type(Type::Struct)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a struct module function.
///
/// All struct functions return immediate values.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: StructFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        StructFunctions::Pack => struct_pack(heap, interns, args),
        StructFunctions::Unpack => struct_unpack(heap, interns, args),
        StructFunctions::Calcsize => struct_calcsize(heap, interns, args),
        StructFunctions::IterUnpack => struct_iter_unpack(heap, interns, args),
        StructFunctions::PackInto => struct_pack_into(heap, interns, args),
        StructFunctions::UnpackFrom => struct_unpack_from(heap, interns, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

/// Byte order for struct format strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    /// Native byte order, size & alignment (default)
    Native,
    /// Native byte order, standard size & alignment
    NativeStandard,
    /// Little-endian
    Little,
    /// Big-endian
    Big,
}

impl ByteOrder {
    /// Returns true if native alignment should be used.
    fn use_native_alignment(self) -> bool {
        matches!(self, Self::Native)
    }
}

/// Size configuration for native formats.
/// These are platform-dependent.
struct NativeSizes;

impl NativeSizes {
    /// Size of C 'long' type in bytes.
    /// On most 64-bit Unix systems (Linux, macOS), long is 8 bytes.
    /// On 64-bit Windows and some embedded systems, long is 4 bytes.
    #[cfg(all(target_pointer_width = "64", not(target_os = "windows")))]
    const LONG_SIZE: usize = 8;

    #[cfg(not(all(target_pointer_width = "64", not(target_os = "windows"))))]
    const LONG_SIZE: usize = 4;

    /// Alignment of C 'long' type.
    #[cfg(all(target_pointer_width = "64", not(target_os = "windows")))]
    const LONG_ALIGN: usize = 8;

    #[cfg(not(all(target_pointer_width = "64", not(target_os = "windows"))))]
    const LONG_ALIGN: usize = 4;

    /// Size of pointer-sized native integers.
    const POINTER_SIZE: usize = std::mem::size_of::<usize>();

    /// Alignment of pointer-sized native integers.
    const POINTER_ALIGN: usize = std::mem::align_of::<usize>();
}

/// Parsed format string component.
#[derive(Debug, Clone)]
enum FormatItem {
    /// Pad byte (x)
    Pad,
    /// Character (c)
    Char,
    /// Signed byte (b)
    SignedByte,
    /// Unsigned byte (B)
    UnsignedByte,
    /// Bool (?)
    Bool,
    /// Short (h)
    Short,
    /// Unsigned short (H)
    UnsignedShort,
    /// Int (i)
    Int,
    /// Unsigned int (I)
    UnsignedInt,
    /// Long (l) - platform dependent size
    Long,
    /// Unsigned long (L) - platform dependent size
    UnsignedLong,
    /// Long long (q)
    LongLong,
    /// Unsigned long long (Q)
    UnsignedLongLong,
    /// Float (f)
    Float,
    /// Half precision float (e)
    HalfFloat,
    /// Double (d)
    Double,
    /// Native ssize_t (n)
    SSizeT,
    /// Native size_t (N)
    SizeT,
    /// Native void* (P)
    VoidPtr,
    /// String with count (s, count)
    String(usize),
    /// Pascal string with count (p, count)
    PascalString(usize),
}

/// Parsed format string with byte order and items.
#[derive(Debug, Clone)]
struct ParsedFormat {
    /// Byte order for the format
    byte_order: ByteOrder,
    /// Items in the format string
    items: Vec<FormatItem>,
}

/// Error type for format parsing.
struct FormatError {
    message: String,
}

impl FormatError {
    fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into() }
    }
}

/// Parses a format string into a ParsedFormat.
///
/// # Errors
/// Returns a FormatError if the format string is invalid.
fn parse_format(format: &str) -> Result<ParsedFormat, FormatError> {
    let mut chars = format.chars().peekable();

    // Parse byte order prefix
    let byte_order = match chars.peek() {
        Some('@') => {
            chars.next();
            ByteOrder::Native
        }
        Some('=') => {
            chars.next();
            ByteOrder::NativeStandard
        }
        Some('<') => {
            chars.next();
            ByteOrder::Little
        }
        Some('>') => {
            chars.next();
            ByteOrder::Big
        }
        Some('!') => {
            chars.next();
            ByteOrder::Big
        }
        _ => ByteOrder::Native,
    };

    let mut items = Vec::new();

    while let Some(ch) = chars.next() {
        // Skip whitespace
        if ch.is_whitespace() {
            continue;
        }

        // Parse repeat count
        let mut count: usize = 0;
        let format_char = if ch.is_ascii_digit() {
            count = (ch as u8 - b'0') as usize;
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_ascii_digit() {
                    count = count * 10 + (next_ch as u8 - b'0') as usize;
                    chars.next();
                } else {
                    break;
                }
            }
            // Get the actual format char after digits
            match chars.next() {
                Some(c) => c,
                None => return Err(FormatError::new("repeat count given without format specifier")),
            }
        } else {
            ch
        };

        let item = match format_char {
            'x' => {
                let repeat = if count == 0 { 1 } else { count };
                for _ in 0..repeat {
                    items.push(FormatItem::Pad);
                }
                continue;
            }
            'c' => FormatItem::Char,
            'b' => FormatItem::SignedByte,
            'B' => FormatItem::UnsignedByte,
            '?' => FormatItem::Bool,
            'h' => FormatItem::Short,
            'H' => FormatItem::UnsignedShort,
            'i' => FormatItem::Int,
            'I' => FormatItem::UnsignedInt,
            'l' => FormatItem::Long,
            'L' => FormatItem::UnsignedLong,
            'q' => FormatItem::LongLong,
            'Q' => FormatItem::UnsignedLongLong,
            'f' => FormatItem::Float,
            'e' => FormatItem::HalfFloat,
            'd' => FormatItem::Double,
            'n' => FormatItem::SSizeT,
            'N' => FormatItem::SizeT,
            'P' => FormatItem::VoidPtr,
            's' => {
                let len = if count == 0 { 1 } else { count };
                FormatItem::String(len)
            }
            'p' => {
                let len = if count == 0 { 1 } else { count };
                FormatItem::PascalString(len)
            }
            _ => return Err(FormatError::new("bad char in struct format")),
        };

        if count == 0 || format_char == 's' || format_char == 'p' {
            // Single item (or s/p which handles count specially)
            items.push(item);
        } else {
            // Repeat the item count times
            for _ in 0..count {
                items.push(item.clone());
            }
        }
    }

    Ok(ParsedFormat { byte_order, items })
}

/// Gets the size of a format item.
fn get_item_size(item: &FormatItem, byte_order: ByteOrder) -> usize {
    match item {
        FormatItem::Pad => 1,
        FormatItem::Char => 1,
        FormatItem::SignedByte => 1,
        FormatItem::UnsignedByte => 1,
        FormatItem::Bool => 1,
        FormatItem::Short => 2,
        FormatItem::UnsignedShort => 2,
        FormatItem::Int => 4,
        FormatItem::UnsignedInt => 4,
        FormatItem::Long => {
            if byte_order == ByteOrder::Native {
                NativeSizes::LONG_SIZE
            } else {
                4 // Standard size
            }
        }
        FormatItem::UnsignedLong => {
            if byte_order == ByteOrder::Native {
                NativeSizes::LONG_SIZE
            } else {
                4 // Standard size
            }
        }
        FormatItem::LongLong => 8,
        FormatItem::UnsignedLongLong => 8,
        FormatItem::Float => 4,
        FormatItem::HalfFloat => 2,
        FormatItem::Double => 8,
        FormatItem::SSizeT | FormatItem::SizeT | FormatItem::VoidPtr => NativeSizes::POINTER_SIZE,
        FormatItem::String(len) => *len,
        FormatItem::PascalString(len) => *len,
    }
}

/// Gets the alignment of a format item.
fn get_item_align(item: &FormatItem, byte_order: ByteOrder) -> usize {
    if !byte_order.use_native_alignment() {
        return 1; // Standard sizes have no alignment
    }

    match item {
        FormatItem::Short | FormatItem::UnsignedShort => 2,
        FormatItem::Int | FormatItem::UnsignedInt | FormatItem::Float => 4,
        FormatItem::HalfFloat => 2,
        FormatItem::Long | FormatItem::UnsignedLong => NativeSizes::LONG_ALIGN,
        FormatItem::LongLong | FormatItem::UnsignedLongLong | FormatItem::Double => 8,
        FormatItem::SSizeT | FormatItem::SizeT | FormatItem::VoidPtr => NativeSizes::POINTER_ALIGN,
        _ => 1,
    }
}

/// Calculates the size of a parsed format.
fn calc_format_size(format: &ParsedFormat) -> usize {
    let mut size = 0;

    for item in &format.items {
        let item_align = get_item_align(item, format.byte_order);
        let item_size = get_item_size(item, format.byte_order);

        // Apply alignment for native format
        if format.byte_order.use_native_alignment() && item_align > 1 {
            size = (size + item_align - 1) & !(item_align - 1);
        }

        size += item_size;
    }

    size
}

/// Creates a struct.error exception.
fn struct_error(msg: impl Into<String>) -> crate::exception_private::RunError {
    SimpleException::new_msg(ExcType::Exception, msg.into()).into()
}

/// Implementation of `struct.pack(fmt, v1, v2, ...)`.
///
/// Packs values into a bytes object according to the format string.
fn struct_pack(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    // Handle variable arguments - need at least format
    // Note: format with only pad bytes ('x') doesn't require any values
    let (format_arg, mut pos_args, kwargs) = match args {
        ArgValues::One(fmt) => (fmt, SmallVec::new(), KwargsValues::Empty),
        ArgValues::Two(fmt, v) => {
            let mut args = SmallVec::<[Value; 3]>::new();
            args.push(v);
            (fmt, args, KwargsValues::Empty)
        }
        ArgValues::ArgsKargs { args: pos, kwargs } => {
            if pos.is_empty() {
                return Err(struct_error("pack requires at least format string"));
            }
            let mut iter = pos.into_iter();
            let fmt = iter.next().unwrap();
            let values: SmallVec<[Value; 3]> = iter.collect();
            (fmt, values, kwargs)
        }
        _ => return Err(struct_error("pack requires at least format string")),
    };

    // Drop any kwargs (struct.pack doesn't accept keyword arguments)
    kwargs.drop_with_heap(heap);

    let format_str = format_arg.py_str(heap, interns).into_owned();
    format_arg.drop_with_heap(heap);

    let parsed = match parse_format(&format_str) {
        Ok(p) => p,
        Err(e) => {
            for arg in pos_args.drain(..) {
                arg.drop_with_heap(heap);
            }
            return Err(struct_error(e.message));
        }
    };

    // Count non-pad items to validate argument count
    let value_count = parsed.items.iter().filter(|i| !matches!(i, FormatItem::Pad)).count();

    if value_count != pos_args.len() {
        let msg = format!(
            "pack expected {} items for packing (got {})",
            value_count,
            pos_args.len()
        );
        for arg in pos_args.drain(..) {
            arg.drop_with_heap(heap);
        }
        return Err(struct_error(msg));
    }

    let mut result = Vec::with_capacity(calc_format_size(&parsed));
    let mut arg_idx = 0;

    for item in &parsed.items {
        if let FormatItem::Pad = item {
            result.push(0);
        } else {
            let value = &pos_args[arg_idx];
            pack_value(&mut result, value, item, parsed.byte_order, heap, interns)?;
            arg_idx += 1;
        }
    }

    // Drop remaining args (should be none, but just in case)
    for arg in pos_args.drain(arg_idx..) {
        arg.drop_with_heap(heap);
    }

    let bytes = Bytes::from(result);
    let id = heap.allocate(HeapData::Bytes(bytes))?;
    Ok(Value::Ref(id))
}

/// Packs a single value into the result buffer.
fn pack_value(
    result: &mut Vec<u8>,
    value: &Value,
    item: &FormatItem,
    byte_order: ByteOrder,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let is_big = matches!(byte_order, ByteOrder::Big);

    match item {
        FormatItem::Char => {
            // 'c' format requires a bytes-like object of length 1
            let bytes = get_bytes_for_char(value, heap, interns)?;
            if bytes.len() != 1 {
                return Err(struct_error("char format requires a bytes object of length 1"));
            }
            result.push(bytes[0]);
            Ok(())
        }
        FormatItem::SignedByte => {
            let v = value_to_i64(value, heap)?;
            if !(-128..=127).contains(&v) {
                return Err(struct_error("'b' format requires -128 <= number <= 127"));
            }
            result.push(v as u8);
            Ok(())
        }
        FormatItem::UnsignedByte => {
            let v = value_to_i64(value, heap)?;
            if !(0..=255).contains(&v) {
                return Err(struct_error("'B' format requires 0 <= number <= 255"));
            }
            result.push(v as u8);
            Ok(())
        }
        FormatItem::Bool => {
            let v = value.py_bool(heap, interns);
            result.push(u8::from(v));
            Ok(())
        }
        FormatItem::Short => {
            let v = value_to_i64(value, heap)?;
            if !(-32768..=32767).contains(&v) {
                return Err(struct_error("'h' format requires -32768 <= number <= 32767"));
            }
            let bytes = if is_big {
                (v as i16).to_be_bytes()
            } else {
                (v as i16).to_le_bytes()
            };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::UnsignedShort => {
            let v = value_to_i64(value, heap)?;
            if !(0..=65535).contains(&v) {
                return Err(struct_error("'H' format requires 0 <= number <= 65535"));
            }
            let bytes = if is_big {
                (v as u16).to_be_bytes()
            } else {
                (v as u16).to_le_bytes()
            };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::Int => {
            let v = value_to_i64(value, heap)?;
            // Check range for 32-bit signed int
            if !(-2147483648..=2147483647).contains(&v) {
                return Err(struct_error("'i' format requires -2147483648 <= number <= 2147483647"));
            }
            let bytes = if is_big {
                (v as i32).to_be_bytes()
            } else {
                (v as i32).to_le_bytes()
            };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::UnsignedInt => {
            let v = value_to_u64(value, heap)?;
            if v > u64::from(u32::MAX) {
                return Err(struct_error("'I' format requires 0 <= number <= 4294967295"));
            }
            let bytes = if is_big {
                (v as u32).to_be_bytes()
            } else {
                (v as u32).to_le_bytes()
            };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::Long => {
            let v = value_to_i64(value, heap)?;
            let size = if byte_order == ByteOrder::Native {
                NativeSizes::LONG_SIZE
            } else {
                4
            };
            if size == 8 {
                let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
                result.extend_from_slice(&bytes);
            } else {
                // 4-byte long
                if !(-2147483648..=2147483647).contains(&v) {
                    return Err(struct_error("int format requires -2147483648 <= number <= 2147483647"));
                }
                let bytes = if is_big {
                    (v as i32).to_be_bytes()
                } else {
                    (v as i32).to_le_bytes()
                };
                result.extend_from_slice(&bytes);
            }
            Ok(())
        }
        FormatItem::UnsignedLong => {
            let v = value_to_u64(value, heap)?;
            let size = if byte_order == ByteOrder::Native {
                NativeSizes::LONG_SIZE
            } else {
                4
            };
            if size == 8 {
                let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
                result.extend_from_slice(&bytes);
            } else {
                // 4-byte long
                if v > u64::from(u32::MAX) {
                    return Err(struct_error("uint format requires 0 <= number <= 4294967295"));
                }
                let bytes = if is_big {
                    (v as u32).to_be_bytes()
                } else {
                    (v as u32).to_le_bytes()
                };
                result.extend_from_slice(&bytes);
            }
            Ok(())
        }
        FormatItem::LongLong => {
            let v = value_to_i64(value, heap)?;
            let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::UnsignedLongLong => {
            let v = value_to_u64(value, heap)?;
            let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::Float => {
            let v = value_to_f64(value, heap)?;
            let bytes = if is_big {
                (v as f32).to_be_bytes()
            } else {
                (v as f32).to_le_bytes()
            };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::HalfFloat => {
            let v = value_to_f64(value, heap)?;
            let bits = f32_to_f16_bits(v as f32);
            let bytes = if is_big { bits.to_be_bytes() } else { bits.to_le_bytes() };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::Double => {
            let v = value_to_f64(value, heap)?;
            let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
            result.extend_from_slice(&bytes);
            Ok(())
        }
        FormatItem::SSizeT => {
            let v = value_to_i64(value, heap)?;
            if NativeSizes::POINTER_SIZE == 8 {
                let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
                result.extend_from_slice(&bytes);
                Ok(())
            } else {
                if v < i64::from(i32::MIN) || v > i64::from(i32::MAX) {
                    return Err(struct_error("argument out of range"));
                }
                let bytes = if is_big {
                    (v as i32).to_be_bytes()
                } else {
                    (v as i32).to_le_bytes()
                };
                result.extend_from_slice(&bytes);
                Ok(())
            }
        }
        FormatItem::SizeT | FormatItem::VoidPtr => {
            let v = value_to_u64(value, heap)?;
            if NativeSizes::POINTER_SIZE == 8 {
                let bytes = if is_big { v.to_be_bytes() } else { v.to_le_bytes() };
                result.extend_from_slice(&bytes);
                Ok(())
            } else {
                if v > u64::from(u32::MAX) {
                    return Err(struct_error("argument out of range"));
                }
                let bytes = if is_big {
                    (v as u32).to_be_bytes()
                } else {
                    (v as u32).to_le_bytes()
                };
                result.extend_from_slice(&bytes);
                Ok(())
            }
        }
        FormatItem::String(len) => {
            let bytes = get_bytes_for_string(value, heap, interns)?;
            let to_write = if bytes.len() >= *len {
                &bytes[..*len]
            } else {
                result.extend_from_slice(&bytes);
                // Pad with nulls
                for _ in bytes.len()..*len {
                    result.push(0);
                }
                return Ok(());
            };
            result.extend_from_slice(to_write);
            Ok(())
        }
        FormatItem::PascalString(len) => {
            let bytes = get_bytes_for_string(value, heap, interns)?;
            let max_len = len.saturating_sub(1); // First byte is length
            let write_len = bytes.len().min(max_len).min(255);
            result.push(write_len as u8);
            result.extend_from_slice(&bytes[..write_len]);
            // Pad remaining space
            for _ in write_len..max_len {
                result.push(0);
            }
            Ok(())
        }
        FormatItem::Pad => Ok(()),
    }
}

/// Gets bytes from a value for 's' and 'p' formats.
/// These formats require a bytes-like object.
fn get_bytes_for_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => {
            if let HeapData::Bytes(bytes) = heap.get(*id) {
                Ok(bytes.as_slice().to_vec())
            } else {
                Err(struct_error("argument for 's' must be a bytes object"))
            }
        }
        _ => Err(struct_error("argument for 's' must be a bytes object")),
    }
}

/// Gets bytes from a value for 'c' format.
fn get_bytes_for_char(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => {
            if let HeapData::Bytes(bytes) = heap.get(*id) {
                Ok(bytes.as_slice().to_vec())
            } else if let HeapData::Str(s) = heap.get(*id) {
                // Single character string works too
                let s_str = s.as_str();
                if s_str.len() == 1 {
                    Ok(s_str.bytes().collect())
                } else {
                    Err(struct_error("char format requires a bytes object of length 1"))
                }
            } else {
                Err(struct_error("char format requires a bytes object of length 1"))
            }
        }
        Value::InternString(id) => {
            let s = interns.get_str(*id);
            if s.len() == 1 {
                Ok(s.bytes().collect())
            } else {
                Err(struct_error("char format requires a bytes object of length 1"))
            }
        }
        _ => Err(struct_error("char format requires a bytes object of length 1")),
    }
}

/// Converts a Value to i64.
fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match *value {
        Value::Int(i) => Ok(i),
        Value::Bool(b) => Ok(i64::from(b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(id) {
                li.to_i64().ok_or_else(|| struct_error("argument out of range"))
            } else {
                Err(struct_error("required argument is not an integer"))
            }
        }
        _ => Err(struct_error("required argument is not an integer")),
    }
}

/// Converts a Value to u64.
fn value_to_u64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<u64> {
    match *value {
        Value::Int(i) => {
            if i < 0 {
                Err(struct_error("can't convert negative value to unsigned"))
            } else {
                Ok(i as u64)
            }
        }
        Value::Bool(b) => Ok(u64::from(b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(id) {
                if li.is_negative() {
                    Err(struct_error("can't convert negative value to unsigned"))
                } else {
                    li.to_u64().ok_or_else(|| struct_error("argument out of range"))
                }
            } else {
                Err(struct_error("required argument is not an integer"))
            }
        }
        _ => Err(struct_error("required argument is not an integer")),
    }
}

/// Converts a Value to f64.
fn value_to_f64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<f64> {
    match *value {
        Value::Int(i) => Ok(i as f64),
        Value::Float(f) => Ok(f),
        Value::Bool(b) => Ok(f64::from(b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(id) {
                li.to_f64()
                    .ok_or_else(|| struct_error("int too large to convert to float"))
            } else {
                Err(struct_error("required argument is not a float"))
            }
        }
        _ => Err(struct_error("required argument is not a float")),
    }
}

/// Converts an unsigned 64-bit value to a Python int value.
fn u64_to_value(v: u64, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if let Ok(i) = i64::try_from(v) {
        Ok(Value::Int(i))
    } else {
        LongInt::new(BigInt::from(v))
            .into_value(heap)
            .map_err(|_| struct_error("argument out of range"))
    }
}

/// Converts an f32 to IEEE-754 binary16 bits.
fn f32_to_f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x7f_ffff;

    if exp == 0xff {
        return if frac == 0 { sign | 0x7c00 } else { sign | 0x7e00 };
    }

    let exp16 = exp - 127 + 15;
    if exp16 >= 0x1f {
        return sign | 0x7c00;
    }

    if exp16 <= 0 {
        if exp16 < -10 {
            return sign;
        }
        let frac_with_hidden = frac | 0x80_0000;
        let shift = 14 - exp16;
        let mut frac16 = (frac_with_hidden >> shift) as u16;
        if ((frac_with_hidden >> (shift - 1)) & 1) != 0 {
            frac16 = frac16.wrapping_add(1);
        }
        return sign | frac16;
    }

    let mut frac16 = (frac >> 13) as u16;
    if (frac & 0x1000) != 0 {
        frac16 = frac16.wrapping_add(1);
        if (frac16 & 0x0400) != 0 {
            frac16 = 0;
            let next_exp = exp16 + 1;
            if next_exp >= 0x1f {
                return sign | 0x7c00;
            }
            return sign | ((next_exp as u16) << 10) | frac16;
        }
    }

    sign | ((exp16 as u16) << 10) | frac16
}

/// Converts IEEE-754 binary16 bits to f32.
fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = (u32::from(bits & 0x8000)) << 16;
    let exp = (bits >> 10) & 0x1f;
    let frac = u32::from(bits & 0x03ff);

    let f32_bits = if exp == 0 {
        if frac == 0 {
            sign
        } else {
            let mut frac_norm = frac;
            let mut exp32 = -14i32;
            while (frac_norm & 0x0400) == 0 {
                frac_norm <<= 1;
                exp32 -= 1;
            }
            frac_norm &= 0x03ff;
            sign | (((exp32 + 127) as u32) << 23) | (frac_norm << 13)
        }
    } else if exp == 0x1f {
        sign | 0x7f80_0000 | (frac << 13)
    } else {
        let exp32 = (i32::from(exp) - 15 + 127) as u32;
        sign | (exp32 << 23) | (frac << 13)
    };

    f32::from_bits(f32_bits)
}

/// Implementation of `struct.unpack(fmt, buffer)`.
///
/// Unpacks bytes into a tuple according to the format string.
fn struct_unpack(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (format_value, buffer_value) = args.get_two_args("struct.unpack", heap)?;
    let format_str = format_value.py_str(heap, interns).into_owned();
    format_value.drop_with_heap(heap);

    let parsed = match parse_format(&format_str) {
        Ok(p) => p,
        Err(e) => {
            buffer_value.drop_with_heap(heap);
            return Err(struct_error(e.message));
        }
    };

    let buffer = get_bytes(buffer_value, heap, interns)?;
    let expected_size = calc_format_size(&parsed);

    if buffer.len() != expected_size {
        return Err(struct_error(format!(
            "unpack requires a buffer of {expected_size} bytes"
        )));
    }

    let mut values: SmallVec<[Value; 3]> = SmallVec::with_capacity(parsed.items.len());
    let mut offset = 0;

    for item in &parsed.items {
        if let FormatItem::Pad = item {
            offset += 1;
            continue;
        }

        // Handle alignment for native format
        if parsed.byte_order.use_native_alignment() {
            let align = get_item_align(item, parsed.byte_order);
            if align > 1 {
                offset = (offset + align - 1) & !(align - 1);
            }
        }

        let value = unpack_value(&buffer, &mut offset, item, parsed.byte_order, heap, interns)?;
        values.push(value);
    }

    Ok(allocate_tuple(values, heap)?)
}

/// Gets bytes from a Value.
fn get_bytes(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    defer_drop!(value, heap);
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            _ => Err(struct_error("a bytes-like object is required")),
        },
        _ => Err(struct_error("a bytes-like object is required")),
    }
}

/// Unpacks a single value from the buffer.
fn unpack_value(
    buffer: &[u8],
    offset: &mut usize,
    item: &FormatItem,
    byte_order: ByteOrder,
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
) -> RunResult<Value> {
    let is_big = matches!(byte_order, ByteOrder::Big);

    match item {
        FormatItem::Char => {
            let ch = buffer[*offset];
            *offset += 1;
            // CPython returns a bytes object of length 1 for 'c' format
            let bytes = Bytes::from(vec![ch]);
            let id = heap.allocate(HeapData::Bytes(bytes))?;
            Ok(Value::Ref(id))
        }
        FormatItem::SignedByte => {
            let v = buffer[*offset] as i8;
            *offset += 1;
            Ok(Value::Int(i64::from(v)))
        }
        FormatItem::UnsignedByte => {
            let v = buffer[*offset];
            *offset += 1;
            Ok(Value::Int(i64::from(v)))
        }
        FormatItem::Bool => {
            let v = buffer[*offset] != 0;
            *offset += 1;
            Ok(Value::Bool(v))
        }
        FormatItem::Short => {
            let bytes = &buffer[*offset..*offset + 2];
            *offset += 2;
            let v = if is_big {
                i16::from_be_bytes([bytes[0], bytes[1]])
            } else {
                i16::from_le_bytes([bytes[0], bytes[1]])
            };
            Ok(Value::Int(i64::from(v)))
        }
        FormatItem::UnsignedShort => {
            let bytes = &buffer[*offset..*offset + 2];
            *offset += 2;
            let v = if is_big {
                u16::from_be_bytes([bytes[0], bytes[1]])
            } else {
                u16::from_le_bytes([bytes[0], bytes[1]])
            };
            Ok(Value::Int(i64::from(v)))
        }
        FormatItem::Int => {
            let bytes = &buffer[*offset..*offset + 4];
            *offset += 4;
            let v = if is_big {
                i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            };
            Ok(Value::Int(i64::from(v)))
        }
        FormatItem::UnsignedInt => {
            let bytes = &buffer[*offset..*offset + 4];
            *offset += 4;
            let v = if is_big {
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            };
            Ok(Value::Int(i64::from(v)))
        }
        FormatItem::Long => {
            let size = if byte_order == ByteOrder::Native {
                NativeSizes::LONG_SIZE
            } else {
                4
            };
            if size == 8 {
                let bytes = &buffer[*offset..*offset + 8];
                *offset += 8;
                let v = if is_big {
                    i64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                } else {
                    i64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                };
                Ok(Value::Int(v))
            } else {
                let bytes = &buffer[*offset..*offset + 4];
                *offset += 4;
                let v = if is_big {
                    i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                };
                Ok(Value::Int(i64::from(v)))
            }
        }
        FormatItem::UnsignedLong => {
            let size = if byte_order == ByteOrder::Native {
                NativeSizes::LONG_SIZE
            } else {
                4
            };
            if size == 8 {
                let bytes = &buffer[*offset..*offset + 8];
                *offset += 8;
                let v = if is_big {
                    u64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                } else {
                    u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                };
                u64_to_value(v, heap)
            } else {
                let bytes = &buffer[*offset..*offset + 4];
                *offset += 4;
                let v = if is_big {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                };
                Ok(Value::Int(i64::from(v)))
            }
        }
        FormatItem::LongLong => {
            let bytes = &buffer[*offset..*offset + 8];
            *offset += 8;
            let v = if is_big {
                i64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            } else {
                i64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            };
            Ok(Value::Int(v))
        }
        FormatItem::UnsignedLongLong => {
            let bytes = &buffer[*offset..*offset + 8];
            *offset += 8;
            let v = if is_big {
                u64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            } else {
                u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            };
            u64_to_value(v, heap)
        }
        FormatItem::Float => {
            let bytes = &buffer[*offset..*offset + 4];
            *offset += 4;
            let v = if is_big {
                f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            };
            Ok(Value::Float(f64::from(v)))
        }
        FormatItem::HalfFloat => {
            let bytes = &buffer[*offset..*offset + 2];
            *offset += 2;
            let bits = if is_big {
                u16::from_be_bytes([bytes[0], bytes[1]])
            } else {
                u16::from_le_bytes([bytes[0], bytes[1]])
            };
            Ok(Value::Float(f64::from(f16_bits_to_f32(bits))))
        }
        FormatItem::Double => {
            let bytes = &buffer[*offset..*offset + 8];
            *offset += 8;
            let v = if is_big {
                f64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            } else {
                f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            };
            Ok(Value::Float(v))
        }
        FormatItem::SSizeT => {
            if NativeSizes::POINTER_SIZE == 8 {
                let bytes = &buffer[*offset..*offset + 8];
                *offset += 8;
                let v = if is_big {
                    i64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                } else {
                    i64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                };
                Ok(Value::Int(v))
            } else {
                let bytes = &buffer[*offset..*offset + 4];
                *offset += 4;
                let v = if is_big {
                    i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                };
                Ok(Value::Int(i64::from(v)))
            }
        }
        FormatItem::SizeT | FormatItem::VoidPtr => {
            if NativeSizes::POINTER_SIZE == 8 {
                let bytes = &buffer[*offset..*offset + 8];
                *offset += 8;
                let v = if is_big {
                    u64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                } else {
                    u64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    ])
                };
                u64_to_value(v, heap)
            } else {
                let bytes = &buffer[*offset..*offset + 4];
                *offset += 4;
                let v = if is_big {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                };
                Ok(Value::Int(i64::from(v)))
            }
        }
        FormatItem::String(len) => {
            let end = (*offset + *len).min(buffer.len());
            let data = &buffer[*offset..end];
            *offset += *len;
            let bytes = Bytes::from(data.to_vec());
            let id = heap.allocate(HeapData::Bytes(bytes))?;
            Ok(Value::Ref(id))
        }
        FormatItem::PascalString(len) => {
            if *len == 0 {
                let bytes = Bytes::from(Vec::new());
                let id = heap.allocate(HeapData::Bytes(bytes))?;
                return Ok(Value::Ref(id));
            }
            let str_len = buffer[*offset] as usize;
            *offset += 1;
            let data_len = str_len.min(len - 1);
            let data = &buffer[*offset..*offset + data_len];
            *offset += len - 1;
            let bytes = Bytes::from(data.to_vec());
            let id = heap.allocate(HeapData::Bytes(bytes))?;
            Ok(Value::Ref(id))
        }
        FormatItem::Pad => unreachable!(),
    }
}

/// Implementation of `struct.calcsize(fmt)`.
///
/// Returns the size of the struct described by the format string.
fn struct_calcsize(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let format_value = args.get_one_arg("struct.calcsize", heap)?;
    let format_str = format_value.py_str(heap, interns).into_owned();
    format_value.drop_with_heap(heap);

    let parsed = match parse_format(&format_str) {
        Ok(p) => p,
        Err(e) => return Err(struct_error(e.message)),
    };

    Ok(Value::Int(calc_format_size(&parsed) as i64))
}

/// Implementation of `struct.iter_unpack(fmt, buffer)`.
///
/// Returns an iterator that unpacks the buffer in chunks.
fn struct_iter_unpack(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (format_value, buffer_value) = args.get_two_args("struct.iter_unpack", heap)?;

    let format_str = format_value.py_str(heap, interns).into_owned();
    format_value.drop_with_heap(heap);

    let parsed = match parse_format(&format_str) {
        Ok(p) => p,
        Err(e) => {
            buffer_value.drop_with_heap(heap);
            return Err(struct_error(e.message));
        }
    };

    let buffer = get_bytes(buffer_value, heap, interns)?;
    let item_size = calc_format_size(&parsed);

    if item_size == 0 {
        return Err(struct_error("cannot iter_unpack with a struct of length 0"));
    }

    if buffer.len() % item_size != 0 {
        return Err(struct_error(format!(
            "iter_unpack requires a buffer of at least {} bytes (got {})",
            item_size,
            buffer.len()
        )));
    }

    let mut list = crate::types::List::new(Vec::new());
    for chunk_start in (0..buffer.len()).step_by(item_size) {
        let chunk = &buffer[chunk_start..chunk_start + item_size];
        let mut values: SmallVec<[Value; 3]> = SmallVec::with_capacity(parsed.items.len());
        let mut offset = 0;

        for item in &parsed.items {
            if let FormatItem::Pad = item {
                offset += 1;
                continue;
            }

            // Handle alignment for native format
            if parsed.byte_order.use_native_alignment() {
                let align = get_item_align(item, parsed.byte_order);
                if align > 1 {
                    offset = (offset + align - 1) & !(align - 1);
                }
            }

            match unpack_value(chunk, &mut offset, item, parsed.byte_order, heap, interns) {
                Ok(value) => values.push(value),
                Err(e) => {
                    for value in values.drain(..) {
                        value.drop_with_heap(heap);
                    }
                    return Err(e);
                }
            }
        }

        let tuple = allocate_tuple(values, heap)?;
        list.append(heap, tuple);
    }

    let id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(id))
}

/// Implementation of `struct.pack_into(fmt, buffer, offset, v1, v2, ...)`.
///
/// Packs values into an existing buffer at the given offset.
fn struct_pack_into(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    // Handle variable arguments - need format, buffer, offset, and at least one value
    let (mut pos_iter, kwargs) = match args {
        ArgValues::ArgsKargs { args, kwargs } => (args.into_iter(), kwargs),
        _ => return Err(struct_error("pack_into requires format, buffer, offset, and values")),
    };

    // Drop kwargs (pack_into doesn't accept them)
    kwargs.drop_with_heap(heap);

    let format_arg = pos_iter
        .next()
        .ok_or_else(|| struct_error("pack_into requires format"))?;
    let buffer_arg = pos_iter
        .next()
        .ok_or_else(|| struct_error("pack_into requires buffer"))?;
    let offset_arg = pos_iter
        .next()
        .ok_or_else(|| struct_error("pack_into requires offset"))?;

    let format_str = format_arg.py_str(heap, interns).into_owned();
    format_arg.drop_with_heap(heap);

    let parsed = match parse_format(&format_str) {
        Ok(p) => p,
        Err(e) => {
            buffer_arg.drop_with_heap(heap);
            offset_arg.drop_with_heap(heap);
            for arg in pos_iter {
                arg.drop_with_heap(heap);
            }
            return Err(struct_error(e.message));
        }
    };

    // Get offset value
    let offset_raw = value_to_i64(&offset_arg, heap)?;
    offset_arg.drop_with_heap(heap);
    if offset_raw < 0 {
        buffer_arg.drop_with_heap(heap);
        for arg in pos_iter {
            arg.drop_with_heap(heap);
        }
        return Err(struct_error("offset must be non-negative"));
    }
    let offset = usize::try_from(offset_raw).expect("non-negative i64 fits usize");

    let mut values: SmallVec<[Value; 3]> = pos_iter.collect();
    let value_count = parsed
        .items
        .iter()
        .filter(|item| !matches!(item, FormatItem::Pad))
        .count();
    if value_count != values.len() {
        let msg = format!("pack expected {} items for packing (got {})", value_count, values.len());
        for value in values.drain(..) {
            value.drop_with_heap(heap);
        }
        buffer_arg.drop_with_heap(heap);
        return Err(struct_error(msg));
    }

    let mut packed = Vec::with_capacity(calc_format_size(&parsed));
    let mut arg_idx = 0;
    for item in &parsed.items {
        if let FormatItem::Pad = item {
            packed.push(0);
        } else {
            let value = &values[arg_idx];
            if let Err(err) = pack_value(&mut packed, value, item, parsed.byte_order, heap, interns) {
                for value in values.drain(..) {
                    value.drop_with_heap(heap);
                }
                buffer_arg.drop_with_heap(heap);
                return Err(err);
            }
            arg_idx += 1;
        }
    }
    for value in values.drain(..) {
        value.drop_with_heap(heap);
    }

    let result = match &buffer_arg {
        Value::Ref(buffer_id) => {
            if let HeapData::Bytearray(buffer) = heap.get_mut(*buffer_id) {
                let end = offset
                    .checked_add(packed.len())
                    .ok_or_else(|| struct_error("pack_into out of range"))?;
                if end > buffer.len() {
                    Err(struct_error("pack_into requires a buffer of at least required size"))
                } else {
                    buffer.as_vec_mut()[offset..end].copy_from_slice(&packed);
                    Ok(Value::None)
                }
            } else {
                Err(struct_error("argument must be read-write bytes-like object"))
            }
        }
        _ => Err(struct_error("argument must be read-write bytes-like object")),
    };

    buffer_arg.drop_with_heap(heap);
    result
}

/// Implementation of `struct.unpack_from(fmt, buffer, offset=0)`.
///
/// Unpacks bytes from a buffer starting at the given offset.
fn struct_unpack_from(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    // Handle variable arguments: unpack_from(fmt, buffer, offset=0)
    let (format_arg, buffer_arg, offset_arg, kwargs) = match args {
        ArgValues::Two(fmt, buf) => (fmt, buf, None, KwargsValues::Empty),
        ArgValues::ArgsKargs { args, kwargs } => {
            let mut iter = args.into_iter();
            let fmt = iter.next().ok_or_else(|| struct_error("unpack_from requires format"))?;
            let buf = iter.next().ok_or_else(|| struct_error("unpack_from requires buffer"))?;
            let off = iter.next();
            // Drop any extra args
            for extra in iter {
                extra.drop_with_heap(heap);
            }
            (fmt, buf, off, kwargs)
        }
        _ => return Err(struct_error("unpack_from requires format and buffer")),
    };

    // Drop kwargs (unpack_from doesn't accept them)
    kwargs.drop_with_heap(heap);

    let format_str = format_arg.py_str(heap, interns).into_owned();
    format_arg.drop_with_heap(heap);

    let parsed = match parse_format(&format_str) {
        Ok(p) => p,
        Err(e) => {
            buffer_arg.drop_with_heap(heap);
            if let Some(off) = offset_arg {
                off.drop_with_heap(heap);
            }
            return Err(struct_error(e.message));
        }
    };

    let offset = if let Some(off) = offset_arg {
        let v = value_to_i64(&off, heap)?;
        off.drop_with_heap(heap);
        if v < 0 {
            return Err(struct_error("offset must be non-negative"));
        }
        v as usize
    } else {
        0
    };

    let buffer = get_bytes(buffer_arg, heap, interns)?;

    if offset > buffer.len() {
        return Err(struct_error("offset exceeds buffer size"));
    }

    let expected_size = calc_format_size(&parsed);
    let available = buffer.len() - offset;

    if available < expected_size {
        return Err(struct_error(format!(
            "unpack_from requires a buffer of at least {expected_size} bytes (got {available})"
        )));
    }

    // Unpack from the sliced buffer
    let sliced = &buffer[offset..];
    let mut values: SmallVec<[Value; 3]> = SmallVec::with_capacity(parsed.items.len());
    let mut pos = 0;

    for item in &parsed.items {
        if let FormatItem::Pad = item {
            pos += 1;
            continue;
        }

        // Handle alignment for native format
        if parsed.byte_order.use_native_alignment() {
            let align = get_item_align(item, parsed.byte_order);
            if align > 1 {
                pos = (pos + align - 1) & !(align - 1);
            }
        }

        let value = unpack_value(sliced, &mut pos, item, parsed.byte_order, heap, interns)?;
        values.push(value);
    }

    Ok(allocate_tuple(values, heap)?)
}
