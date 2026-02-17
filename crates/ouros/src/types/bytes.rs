/// Python bytes type, wrapping a `Vec<u8>`.
///
/// This type provides Python bytes semantics with operations on ASCII bytes only.
/// Unlike str methods which operate on Unicode codepoints, bytes methods only
/// recognize ASCII characters (0-127) for case transformations and predicates.
///
/// # Implemented Methods
///
/// ## Encoding/Decoding
/// - `decode([encoding[, errors]])` - Decode to string (UTF-8, ASCII, Latin-1)
/// - `hex([sep[, bytes_per_sep]])` - Return hex string representation
/// - `fromhex(string)` - Create bytes from hex string (classmethod)
///
/// ## Simple Transformations
/// - `lower()` - Convert ASCII uppercase to lowercase
/// - `upper()` - Convert ASCII lowercase to uppercase
/// - `capitalize()` - First byte uppercase, rest lowercase
/// - `title()` - Titlecase ASCII letters
/// - `swapcase()` - Swap ASCII case
///
/// ## Predicates
/// - `isalpha()` - All bytes are ASCII letters
/// - `isdigit()` - All bytes are ASCII digits
/// - `isalnum()` - All bytes are ASCII alphanumeric
/// - `isspace()` - All bytes are ASCII whitespace
/// - `islower()` - Has cased bytes, all lowercase
/// - `isupper()` - Has cased bytes, all uppercase
/// - `isascii()` - All bytes are ASCII (0-127)
/// - `istitle()` - Titlecased
///
/// ## Search Methods
/// - `count(sub[, start[, end]])` - Count non-overlapping occurrences
/// - `find(sub[, start[, end]])` - Find first occurrence (-1 if not found)
/// - `rfind(sub[, start[, end]])` - Find last occurrence (-1 if not found)
/// - `index(sub[, start[, end]])` - Find first occurrence (raises ValueError)
/// - `rindex(sub[, start[, end]])` - Find last occurrence (raises ValueError)
/// - `startswith(prefix[, start[, end]])` - Check if starts with prefix
/// - `endswith(suffix[, start[, end]])` - Check if ends with suffix
///
/// ## Strip/Trim Methods
/// - `strip([chars])` - Remove leading/trailing bytes
/// - `lstrip([chars])` - Remove leading bytes
/// - `rstrip([chars])` - Remove trailing bytes
/// - `removeprefix(prefix)` - Remove prefix if present
/// - `removesuffix(suffix)` - Remove suffix if present
///
/// ## Split Methods
/// - `split([sep[, maxsplit]])` - Split on separator
/// - `rsplit([sep[, maxsplit]])` - Split from right
/// - `splitlines([keepends])` - Split on line boundaries
/// - `partition(sep)` - Split into 3 parts at first sep
/// - `rpartition(sep)` - Split into 3 parts at last sep
///
/// ## Replace/Padding Methods
/// - `replace(old, new[, count])` - Replace occurrences
/// - `center(width[, fillbyte])` - Center with fill byte
/// - `ljust(width[, fillbyte])` - Left justify with fill byte
/// - `rjust(width[, fillbyte])` - Right justify with fill byte
/// - `zfill(width)` - Pad with zeros
///
/// ## Other Methods
/// - `join(iterable)` - Join bytes sequences
/// - `expandtabs(tabsize=8)` - Tab expansion
/// - `maketrans(frm, to)` - Create translation table (classmethod)
/// - `translate(table[, delete])` - Character translation/deletion
use std::fmt::Write;

use ahash::AHashSet;
use smallvec::smallvec;

use super::{OurosIter, PyTrait, Type, str::Str};
use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::ResourceTracker,
    types::List,
    value::{EitherStr, Value},
};

// =============================================================================
// ASCII byte helper functions
// =============================================================================

/// Returns true if the byte is Python ASCII whitespace.
///
/// Python considers these bytes as whitespace: space, tab, newline, carriage return,
/// vertical tab (0x0b), and form feed (0x0c). Note: Rust's `is_ascii_whitespace()`
/// does not include vertical tab (0x0b).
#[inline]
fn is_py_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Gets the byte at a given index, handling negative indices.
///
/// Returns `None` if the index is out of bounds.
/// Negative indices count from the end: -1 is the last byte.
pub fn get_byte_at_index(bytes: &[u8], index: i64) -> Option<u8> {
    let len = i64::try_from(bytes.len()).ok()?;
    let normalized = if index < 0 { index + len } else { index };

    if normalized < 0 || normalized >= len {
        return None;
    }

    let idx = usize::try_from(normalized).ok()?;
    Some(bytes[idx])
}

/// Extracts a slice of a byte array.
///
/// Handles both positive and negative step values. For negative step,
/// iterates backward from start down to (but not including) stop.
/// The `stop` parameter uses a sentinel value of `len + 1` for negative
/// step to indicate "go to the beginning".
///
/// Note: step must be non-zero (callers should validate this via `slice.indices()`).
pub(crate) fn get_bytes_slice(bytes: &[u8], start: usize, stop: usize, step: i64) -> Vec<u8> {
    let mut result = Vec::new();

    // try_from succeeds for non-negative step; step==0 rejected upstream by slice.indices()
    if let Ok(step_usize) = usize::try_from(step) {
        // Positive step: iterate forward
        let mut i = start;
        while i < stop && i < bytes.len() {
            result.push(bytes[i]);
            i += step_usize;
        }
    } else {
        // Negative step: iterate backward
        // start is the highest index, stop is the sentinel
        // stop > bytes.len() means "go to the beginning"
        let step_abs = usize::try_from(-step).expect("step is negative so -step is positive");
        let step_abs_i64 = i64::try_from(step_abs).expect("step magnitude fits in i64");
        let mut i = i64::try_from(start).expect("start index fits in i64");
        let stop_i64 = if stop > bytes.len() {
            -1
        } else {
            i64::try_from(stop).expect("stop bounded by bytes.len() fits in i64")
        };

        while let Ok(i_usize) = usize::try_from(i) {
            if i_usize >= bytes.len() || i <= stop_i64 {
                break;
            }
            result.push(bytes[i_usize]);
            i -= step_abs_i64;
        }
    }

    result
}

/// Python bytes value stored on the heap.
///
/// Wraps a `Vec<u8>` and provides Python-compatible operations.
/// See the module-level documentation for implemented and unimplemented methods.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Bytes(Vec<u8>);

impl Bytes {
    /// Creates a new Bytes from a byte vector.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Returns a reference to the inner byte slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns a mutable reference to the inner byte vector.
    pub fn as_vec_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }

    /// Creates bytes from the `bytes()` constructor call.
    ///
    /// - `bytes()` with no args returns empty bytes
    /// - `bytes(int)` returns bytes of that length filled with zeros
    /// - `bytes(string)` encodes the string as UTF-8 (simplified compatibility mode)
    /// - `bytes(string, encoding)` encodes the string using the given encoding
    /// - `bytes(bytes)` returns a copy of the bytes
    /// - `bytes(iterable_of_ints)` consumes integer items in range(0, 256)
    ///
    /// Note: Full Python semantics for bytes() are more complex (encoding, errors params).
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        let (value, encoding) = args.get_zero_one_two_args("bytes", heap)?;
        match (value, encoding) {
            (None, None) => {
                let heap_id = heap.allocate(HeapData::Bytes(Self::new(Vec::new())))?;
                Ok(Value::Ref(heap_id))
            }
            (Some(v), None) => {
                let result = match &v {
                    Value::Int(n) => {
                        if *n < 0 {
                            return Err(ExcType::value_error_negative_bytes_count());
                        }
                        let size = usize::try_from(*n).expect("bytes count validated non-negative");
                        let bytes = vec![0u8; size];
                        heap.allocate(HeapData::Bytes(Self::new(bytes)))
                    }
                    Value::InternString(string_id) => {
                        let s = interns.get_str(*string_id);
                        heap.allocate(HeapData::Bytes(Self::new(s.as_bytes().to_vec())))
                    }
                    Value::InternBytes(bytes_id) => {
                        let b = interns.get_bytes(*bytes_id);
                        heap.allocate(HeapData::Bytes(Self::new(b.to_vec())))
                    }
                    Value::Ref(id) => match heap.get(*id) {
                        HeapData::Str(s) => heap.allocate(HeapData::Bytes(Self::new(s.as_str().as_bytes().to_vec()))),
                        HeapData::Bytes(b) => heap.allocate(HeapData::Bytes(Self::new(b.as_slice().to_vec()))),
                        HeapData::Bytearray(b) => heap.allocate(HeapData::Bytes(Self::new(b.as_slice().to_vec()))),
                        HeapData::Path(p) => {
                            heap.allocate(HeapData::Bytes(Self::new(p.display_path().as_bytes().to_vec())))
                        }
                        HeapData::List(_) | HeapData::Tuple(_) | HeapData::Range(_) => {
                            let iterable = v.clone_with_heap(heap);
                            let bytes = bytes_from_int_iterable(iterable, heap, interns)?;
                            heap.allocate(HeapData::Bytes(Self::new(bytes)))
                        }
                        _ => {
                            let err = ExcType::type_error_bytes_init(v.py_type(heap));
                            v.drop_with_heap(heap);
                            return Err(err);
                        }
                    },
                    _ => {
                        let err = ExcType::type_error_bytes_init(v.py_type(heap));
                        v.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                v.drop_with_heap(heap);
                Ok(Value::Ref(result?))
            }
            (Some(v), Some(enc)) => {
                let source = bytes_constructor_source_text("bytes", &v, heap, interns);
                let encoding_name = bytes_constructor_encoding_arg("bytes", &enc, heap, interns);
                let result = match (source, encoding_name) {
                    (Ok(text), Ok(encoding_name)) => {
                        let encoded = bytes_constructor_encode_text(&text, &encoding_name)?;
                        Ok(Value::Ref(heap.allocate(HeapData::Bytes(Self::new(encoded)))?))
                    }
                    (Err(err), _) => Err(err),
                    (_, Err(err)) => Err(err),
                };
                v.drop_with_heap(heap);
                enc.drop_with_heap(heap);
                result
            }
            (None, Some(enc)) => {
                enc.drop_with_heap(heap);
                Err(ExcType::type_error_at_least("bytes", 1, 0))
            }
        }
    }

    /// Creates bytearray from the `bytearray()` constructor call.
    ///
    /// - `bytearray()` with no args returns empty bytearray
    /// - `bytearray(int)` returns bytearray of that length filled with zeros
    /// - `bytearray(string)` encodes the string as UTF-8
    /// - `bytearray(string, encoding)` encodes the string using the given encoding
    /// - `bytearray(bytes)` returns a copy of the bytes
    /// - `bytearray(iterable_of_ints)` consumes integer items in range(0, 256)
    pub fn init_bytearray(
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        let (value, encoding) = args.get_zero_one_two_args("bytearray", heap)?;
        match (value, encoding) {
            (None, None) => {
                let heap_id = heap.allocate(HeapData::Bytearray(Self::new(Vec::new())))?;
                Ok(Value::Ref(heap_id))
            }
            (Some(v), None) => {
                let result = match &v {
                    Value::Int(n) => {
                        if *n < 0 {
                            return Err(ExcType::value_error_negative_bytes_count());
                        }
                        let size = usize::try_from(*n).expect("bytes count validated non-negative");
                        let bytes = vec![0u8; size];
                        heap.allocate(HeapData::Bytearray(Self::new(bytes)))
                    }
                    Value::InternString(string_id) => {
                        let s = interns.get_str(*string_id);
                        heap.allocate(HeapData::Bytearray(Self::new(s.as_bytes().to_vec())))
                    }
                    Value::InternBytes(bytes_id) => {
                        let b = interns.get_bytes(*bytes_id);
                        heap.allocate(HeapData::Bytearray(Self::new(b.to_vec())))
                    }
                    Value::Ref(id) => match heap.get(*id) {
                        HeapData::Str(s) => {
                            heap.allocate(HeapData::Bytearray(Self::new(s.as_str().as_bytes().to_vec())))
                        }
                        HeapData::Bytes(b) => heap.allocate(HeapData::Bytearray(Self::new(b.as_slice().to_vec()))),
                        HeapData::Bytearray(b) => heap.allocate(HeapData::Bytearray(Self::new(b.as_slice().to_vec()))),
                        HeapData::List(_) | HeapData::Tuple(_) | HeapData::Range(_) => {
                            let iterable = v.clone_with_heap(heap);
                            let bytes = bytes_from_int_iterable(iterable, heap, interns)?;
                            heap.allocate(HeapData::Bytearray(Self::new(bytes)))
                        }
                        _ => {
                            let err = ExcType::type_error(format!("'{}' object is not iterable", v.py_type(heap)));
                            v.drop_with_heap(heap);
                            return Err(err);
                        }
                    },
                    _ => {
                        let err = ExcType::type_error(format!("'{}' object is not iterable", v.py_type(heap)));
                        v.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                v.drop_with_heap(heap);
                Ok(Value::Ref(result?))
            }
            (Some(v), Some(enc)) => {
                let source = bytes_constructor_source_text("bytearray", &v, heap, interns);
                let encoding_name = bytes_constructor_encoding_arg("bytearray", &enc, heap, interns);
                let result = match (source, encoding_name) {
                    (Ok(text), Ok(encoding_name)) => {
                        let encoded = bytes_constructor_encode_text(&text, &encoding_name)?;
                        Ok(Value::Ref(heap.allocate(HeapData::Bytearray(Self::new(encoded)))?))
                    }
                    (Err(err), _) => Err(err),
                    (_, Err(err)) => Err(err),
                };
                v.drop_with_heap(heap);
                enc.drop_with_heap(heap);
                result
            }
            (None, Some(enc)) => {
                enc.drop_with_heap(heap);
                Err(ExcType::type_error_at_least("bytearray", 1, 0))
            }
        }
    }
}

/// Extracts the source text used by `bytes(..., encoding)` / `bytearray(..., encoding)`.
///
/// CPython requires a string first argument when `encoding` is provided.
fn bytes_constructor_source_text(
    constructor_name: &str,
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    match value {
        Value::InternString(string_id) => Ok(interns.get_str(*string_id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(bytes_constructor_encoding_without_string(constructor_name)),
        },
        _ => Err(bytes_constructor_encoding_without_string(constructor_name)),
    }
}

/// Extracts and validates the `encoding` argument for bytes constructors.
fn bytes_constructor_encoding_arg(
    constructor_name: &str,
    encoding: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    match encoding {
        Value::InternString(string_id) => Ok(interns.get_str(*string_id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "{constructor_name}() argument 'encoding' must be str, not {}",
                encoding.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{constructor_name}() argument 'encoding' must be str, not {}",
            encoding.py_type(heap)
        ))),
    }
}

/// Encodes text for constructor calls using supported encodings.
fn bytes_constructor_encode_text(text: &str, encoding: &str) -> RunResult<Vec<u8>> {
    let normalized = encoding.to_ascii_lowercase();
    match normalized.as_str() {
        "utf-8" | "utf8" | "utf_8" | "cp65001" => Ok(text.as_bytes().to_vec()),
        "ascii" | "us-ascii" => {
            if text.is_ascii() {
                Ok(text.as_bytes().to_vec())
            } else {
                Err(ExcType::type_error("ascii codec cannot encode non-ASCII characters"))
            }
        }
        _ => Err(ExcType::lookup_error_unknown_encoding(encoding)),
    }
}

/// Creates CPython-compatible `encoding without a string argument` errors.
fn bytes_constructor_encoding_without_string(_constructor_name: &str) -> crate::exception_private::RunError {
    ExcType::type_error("encoding without a string argument")
}

/// Materializes an iterable of integers into bytes for `bytes(iterable)`.
///
/// Each item must be convertible to an integer and within `0..=255`.
fn bytes_from_int_iterable(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut out = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let parsed = item.as_int(heap);
                item.drop_with_heap(heap);
                let value = match parsed {
                    Ok(value) => value,
                    Err(err) => {
                        iter.drop_with_heap(heap);
                        return Err(err);
                    }
                };
                if !(0..=255).contains(&value) {
                    iter.drop_with_heap(heap);
                    return Err(SimpleException::new_msg(ExcType::ValueError, "bytes must be in range(0, 256)").into());
                }
                #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                {
                    out.push(value as u8);
                }
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

impl From<Vec<u8>> for Bytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for Bytes {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl From<Bytes> for Vec<u8> {
    fn from(bytes: Bytes) -> Self {
        bytes.0
    }
}

impl std::ops::Deref for Bytes {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PyTrait for Bytes {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Bytes
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.0.len()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.0.len())
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Value> {
        bytes_getitem_impl(&self.0, key, heap, false)
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self.0 == other.0
    }

    fn py_add(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        self.py_add_with_result_type(other, heap, false)
    }

    /// Bytes don't contain nested heap references.
    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // No-op: bytes don't hold Value references
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.0.is_empty()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        bytes_repr_fmt(&self.0, f)
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let Some(method) = attr.static_string() else {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(Type::Bytes, attr.as_str(interns)));
        };

        call_bytes_method_impl(self.as_slice(), method, args, heap, interns, false)
    }
}

impl Bytes {
    /// Concatenates two byte sequences and returns either bytes or bytearray.
    pub fn py_add_with_result_type(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        is_bytearray_result: bool,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        let mut bytes = Vec::with_capacity(self.0.len() + other.0.len());
        bytes.extend_from_slice(&self.0);
        bytes.extend_from_slice(&other.0);
        let id = if is_bytearray_result {
            heap.allocate(HeapData::Bytearray(Self::from(bytes)))?
        } else {
            heap.allocate(HeapData::Bytes(Self::from(bytes)))?
        };
        Ok(Some(Value::Ref(id)))
    }

    /// Calls bytearray indexing/slicing semantics.
    pub fn py_getitem_bytearray(&mut self, key: &Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        bytes_getitem_impl(&self.0, key, heap, true)
    }

    /// Implements bytearray item assignment (`bytearray[index] = value`).
    pub fn py_setitem_bytearray(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> RunResult<()> {
        let index_result = key.as_index(heap, Type::Bytearray);
        key.drop_with_heap(heap);
        let index = index_result?;

        let len = self.0.len();
        let len_i64 = i64::try_from(len).expect("bytearray length exceeds i64::MAX");
        let normalized_index = if index < 0 { index + len_i64 } else { index };
        if normalized_index < 0 || normalized_index >= len_i64 {
            value.drop_with_heap(heap);
            return Err(ExcType::bytes_index_error());
        }

        let byte = bytearray_extract_byte(value, heap)?;
        let index = usize::try_from(normalized_index).expect("index validated in range");
        self.0[index] = byte;
        Ok(())
    }

    /// Calls a bytearray method (same as bytes but returns bytearray from mutating methods).
    pub fn py_call_attr_bytearray(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        // `resize` is currently bytearray-only and not interned in StaticStrings.
        if attr.as_str(interns) == "resize" {
            return bytearray_resize(self, args, heap);
        }

        let Some(method) = attr.static_string() else {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(Type::Bytearray, attr.as_str(interns)));
        };

        call_bytearray_method(self, method, args, heap, interns)
    }
}

/// Dispatches bytearray methods, including mutable bytearray-only operations.
///
/// Methods shared with `bytes` are delegated to `call_bytes_method_impl`
/// with `is_bytearray=true` so they return bytearray-shaped results.
fn call_bytearray_method(
    bytearray: &mut Bytes,
    method: StaticStrings,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match method {
        StaticStrings::Append => bytearray_append(bytearray, args, heap),
        StaticStrings::Clear => bytearray_clear(bytearray, args, heap),
        StaticStrings::Copy => bytearray_copy(bytearray, args, heap),
        StaticStrings::Extend => bytearray_extend(bytearray, args, heap, interns),
        StaticStrings::Insert => bytearray_insert(bytearray, args, heap),
        StaticStrings::Pop => bytearray_pop(bytearray, args, heap),
        StaticStrings::Remove => bytearray_remove(bytearray, args, heap),
        StaticStrings::Reverse => bytearray_reverse(bytearray, args, heap),
        _ => call_bytes_method_impl(bytearray.as_slice(), method, args, heap, interns, true),
    }
}

/// Implements `bytearray.append(x)` by appending a single byte.
fn bytearray_append(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let value = args.get_one_arg("bytearray.append", heap)?;
    let byte = bytearray_extract_byte(value, heap)?;
    bytearray.as_vec_mut().push(byte);
    Ok(Value::None)
}

/// Implements `bytearray.clear()` by removing all bytes.
fn bytearray_clear(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    args.check_zero_args("bytearray.clear", heap)?;
    bytearray.as_vec_mut().clear();
    Ok(Value::None)
}

/// Implements `bytearray.copy()` by returning a shallow bytearray copy.
fn bytearray_copy(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    args.check_zero_args("bytearray.copy", heap)?;
    allocate_bytearray(bytearray.as_slice().to_vec(), heap)
}

/// Implements `bytearray.extend(iterable)` by appending bytes from the iterable.
fn bytearray_extend(
    bytearray: &mut Bytes,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let iterable = args.get_one_arg("bytearray.extend", heap)?;
    let extension = bytes_from_int_iterable(iterable, heap, interns)?;
    bytearray.as_vec_mut().extend(extension);
    Ok(Value::None)
}

/// Implements `bytearray.insert(index, x)` by inserting one byte at index.
fn bytearray_insert(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let (index_value, value) = args.get_two_args("bytearray.insert", heap)?;
    let index_result = index_value.as_int(heap);
    index_value.drop_with_heap(heap);
    let index = index_result?;
    let byte = bytearray_extract_byte(value, heap)?;

    let len = bytearray.as_slice().len();
    let len_i64 = i64::try_from(len).expect("bytearray length exceeds i64::MAX");
    let insert_index = if index < 0 {
        usize::try_from(index + len_i64).unwrap_or(0)
    } else {
        usize::try_from(index).unwrap_or(len)
    };
    bytearray.as_vec_mut().insert(insert_index.min(len), byte);
    Ok(Value::None)
}

/// Implements `bytearray.pop([index])` and returns the removed byte as an int.
fn bytearray_pop(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let index_arg = args.get_zero_one_arg("bytearray.pop", heap)?;
    let index = if let Some(v) = index_arg {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        result?
    } else {
        -1
    };

    if bytearray.as_slice().is_empty() {
        return Err(SimpleException::new_msg(ExcType::IndexError, "pop from empty bytearray").into());
    }

    let len = bytearray.as_slice().len();
    let len_i64 = i64::try_from(len).expect("bytearray length exceeds i64::MAX");
    let normalized_index = if index < 0 { index + len_i64 } else { index };
    if normalized_index < 0 || normalized_index >= len_i64 {
        return Err(SimpleException::new_msg(ExcType::IndexError, "pop index out of range").into());
    }

    let remove_index = usize::try_from(normalized_index).expect("index checked in range");
    let popped = bytearray.as_vec_mut().remove(remove_index);
    Ok(Value::Int(i64::from(popped)))
}

/// Implements `bytearray.remove(x)` by removing the first matching byte.
fn bytearray_remove(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let value = args.get_one_arg("bytearray.remove", heap)?;
    let byte = bytearray_extract_byte(value, heap)?;

    let Some(index) = bytearray.as_slice().iter().position(|&b| b == byte) else {
        return Err(SimpleException::new_msg(ExcType::ValueError, "value not found in bytearray").into());
    };
    bytearray.as_vec_mut().remove(index);
    Ok(Value::None)
}

/// Implements `bytearray.reverse()` in place.
fn bytearray_reverse(
    bytearray: &mut Bytes,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    args.check_zero_args("bytearray.reverse", heap)?;
    bytearray.as_vec_mut().reverse();
    Ok(Value::None)
}

/// Implements `bytearray.resize(size)` by truncating or growing with zero bytes.
fn bytearray_resize(bytearray: &mut Bytes, args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let size_value = args.get_one_arg("bytearray.resize", heap)?;
    let size_result = size_value.as_int(heap);
    size_value.drop_with_heap(heap);
    let size = size_result?;

    if size < 0 {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("Can only resize to positive sizes, got {size}"),
        )
        .into());
    }

    let size = usize::try_from(size).unwrap_or(usize::MAX);
    let bytes = bytearray.as_vec_mut();
    if size <= bytes.len() {
        bytes.truncate(size);
    } else {
        bytes.resize(size, 0);
    }
    Ok(Value::None)
}

/// Extracts a single byte argument, enforcing the `0..=255` range.
fn bytearray_extract_byte(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<u8> {
    let int_result = value.as_int(heap);
    value.drop_with_heap(heap);
    let int = int_result?;
    u8::try_from(int).map_err(|_| SimpleException::new_msg(ExcType::ValueError, "byte must be in range(0, 256)").into())
}

/// Calls a bytes method on a byte slice by method name.
///
/// This is the entry point for bytes method calls from the VM on interned bytes.
/// Converts the `StringId` to `StaticStrings` and delegates to `call_bytes_method_impl`.
pub fn call_bytes_method(
    bytes: &[u8],
    method_id: StringId,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let Some(method) = StaticStrings::from_string_id(method_id) else {
        args.drop_with_heap(heap);
        return Err(ExcType::attribute_error(Type::Bytes, interns.get_str(method_id)));
    };
    call_bytes_method_impl(bytes, method, args, heap, interns, false)
}

/// Calls a bytes method on a byte slice.
///
/// This is the unified implementation for bytes method calls, used by both
/// heap-allocated `Bytes` (via `py_call_attr`) and interned bytes literals
/// (`Value::InternBytes`), as well as `Bytearray`.
///
/// When `is_bytearray` is true, methods that return new bytes objects will
/// return bytearray instead (for methods like capitalize, center, lower, etc.).
fn call_bytes_method_impl(
    bytes: &[u8],
    method: StaticStrings,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    match method {
        // Decode method
        StaticStrings::Decode => bytes_decode(bytes, args, heap, interns),
        // Simple transformations (no arguments)
        StaticStrings::Lower => {
            args.check_zero_args("bytes.lower", heap)?;
            bytes_lower(bytes, heap, is_bytearray)
        }
        StaticStrings::Upper => {
            args.check_zero_args("bytes.upper", heap)?;
            bytes_upper(bytes, heap, is_bytearray)
        }
        StaticStrings::Capitalize => {
            args.check_zero_args("bytes.capitalize", heap)?;
            bytes_capitalize(bytes, heap, is_bytearray)
        }
        StaticStrings::Title => {
            args.check_zero_args("bytes.title", heap)?;
            bytes_title(bytes, heap, is_bytearray)
        }
        StaticStrings::Swapcase => {
            args.check_zero_args("bytes.swapcase", heap)?;
            bytes_swapcase(bytes, heap, is_bytearray)
        }
        // Predicate methods (no arguments, return bool)
        StaticStrings::Isalpha => {
            args.check_zero_args("bytes.isalpha", heap)?;
            Ok(Value::Bool(bytes_isalpha(bytes)))
        }
        StaticStrings::Isdigit => {
            args.check_zero_args("bytes.isdigit", heap)?;
            Ok(Value::Bool(bytes_isdigit(bytes)))
        }
        StaticStrings::Isalnum => {
            args.check_zero_args("bytes.isalnum", heap)?;
            Ok(Value::Bool(bytes_isalnum(bytes)))
        }
        StaticStrings::Isspace => {
            args.check_zero_args("bytes.isspace", heap)?;
            Ok(Value::Bool(bytes_isspace(bytes)))
        }
        StaticStrings::Islower => {
            args.check_zero_args("bytes.islower", heap)?;
            Ok(Value::Bool(bytes_islower(bytes)))
        }
        StaticStrings::Isupper => {
            args.check_zero_args("bytes.isupper", heap)?;
            Ok(Value::Bool(bytes_isupper(bytes)))
        }
        StaticStrings::Isascii => {
            args.check_zero_args("bytes.isascii", heap)?;
            Ok(Value::Bool(bytes.iter().all(|&b| b <= 127)))
        }
        StaticStrings::Istitle => {
            args.check_zero_args("bytes.istitle", heap)?;
            Ok(Value::Bool(bytes_istitle(bytes)))
        }
        // Search methods
        StaticStrings::Count => bytes_count(bytes, args, heap, interns),
        StaticStrings::Find => bytes_find(bytes, args, heap, interns),
        StaticStrings::Rfind => bytes_rfind(bytes, args, heap, interns),
        StaticStrings::Index => bytes_index(bytes, args, heap, interns),
        StaticStrings::Rindex => bytes_rindex(bytes, args, heap, interns),
        StaticStrings::Startswith => bytes_startswith(bytes, args, heap, interns),
        StaticStrings::Endswith => bytes_endswith(bytes, args, heap, interns),
        // Strip/trim methods
        StaticStrings::Strip => bytes_strip(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Lstrip => bytes_lstrip(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Rstrip => bytes_rstrip(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Removeprefix => bytes_removeprefix(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Removesuffix => bytes_removesuffix(bytes, args, heap, interns, is_bytearray),
        // Split methods
        StaticStrings::Split => bytes_split(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Rsplit => bytes_rsplit(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Splitlines => bytes_splitlines(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Partition => bytes_partition(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Rpartition => bytes_rpartition(bytes, args, heap, interns, is_bytearray),
        // Replace/padding methods
        StaticStrings::Replace => bytes_replace(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Center => bytes_center(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Ljust => bytes_ljust(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Rjust => bytes_rjust(bytes, args, heap, interns, is_bytearray),
        StaticStrings::Zfill => bytes_zfill(bytes, args, heap, is_bytearray),
        // Join method
        StaticStrings::Join => {
            let iterable = args.get_one_arg("bytes.join", heap)?;
            bytes_join(bytes, iterable, heap, interns, is_bytearray)
        }
        // Hex method
        StaticStrings::Hex => bytes_hex(bytes, args, heap, interns),
        // fromhex is a classmethod but also accessible on instances
        StaticStrings::Fromhex => bytes_fromhex(args, heap, interns, is_bytearray),
        // translate method
        StaticStrings::Translate => bytes_translate(bytes, args, heap, interns, is_bytearray),
        // expandtabs method
        StaticStrings::Expandtabs => bytes_expandtabs(bytes, args, heap, is_bytearray),
        _ => {
            args.drop_with_heap(heap);
            Err(ExcType::attribute_error(
                if is_bytearray { Type::Bytearray } else { Type::Bytes },
                method.into(),
            ))
        }
    }
}

/// Implements shared bytes/bytearray getitem semantics.
fn bytes_getitem_impl(
    bytes: &[u8],
    key: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    is_bytearray: bool,
) -> RunResult<Value> {
    // Check for slice first (Value::Ref pointing to HeapData::Slice)
    if let Value::Ref(id) = key
        && let HeapData::Slice(slice) = heap.get(*id)
    {
        let (start, stop, step) = slice
            .indices(bytes.len())
            .map_err(|()| ExcType::value_error_slice_step_zero())?;

        let sliced_bytes = get_bytes_slice(bytes, start, stop, step);
        let heap_id = if is_bytearray {
            heap.allocate(HeapData::Bytearray(Bytes::new(sliced_bytes)))?
        } else {
            heap.allocate(HeapData::Bytes(Bytes::new(sliced_bytes)))?
        };
        return Ok(Value::Ref(heap_id));
    }

    // Extract integer index, accepting Int, Bool (True=1, False=0), and LongInt
    let container_type = if is_bytearray { Type::Bytearray } else { Type::Bytes };
    let index = key.as_index(heap, container_type)?;

    // Use helper for byte indexing
    let byte = get_byte_at_index(bytes, index).ok_or_else(ExcType::bytes_index_error)?;
    Ok(Value::Int(i64::from(byte)))
}

/// Writes a CPython-compatible repr string for bytes to a formatter.
///
/// Format: `b'...'` or `b"..."` depending on content.
/// - Uses single quotes by default
/// - Switches to double quotes if bytes contain `'` but not `"`
/// - Escapes: `\\`, `\t`, `\n`, `\r`, `\xNN` for non-printable bytes
pub fn bytes_repr_fmt(bytes: &[u8], f: &mut impl Write) -> std::fmt::Result {
    // Determine quote character: use double quotes if single quote present but not double
    let has_single = bytes.contains(&b'\'');
    let has_double = bytes.contains(&b'"');
    let quote = if has_single && !has_double { '"' } else { '\'' };

    f.write_char('b')?;
    f.write_char(quote)?;

    for &byte in bytes {
        match byte {
            b'\\' => f.write_str("\\\\")?,
            b'\t' => f.write_str("\\t")?,
            b'\n' => f.write_str("\\n")?,
            b'\r' => f.write_str("\\r")?,
            b'\'' if quote == '\'' => f.write_str("\\'")?,
            b'"' if quote == '"' => f.write_str("\\\"")?,
            // Printable ASCII (32-126)
            0x20..=0x7e => f.write_char(byte as char)?,
            // Non-printable: use \xNN format
            _ => write!(f, "\\x{byte:02x}")?,
        }
    }

    f.write_char(quote)
}

/// Returns a CPython-compatible repr string for bytes.
///
/// Convenience wrapper around `bytes_repr_fmt` that returns an owned String.
#[must_use]
pub fn bytes_repr(bytes: &[u8]) -> String {
    let mut result = String::new();
    // Writing to String never fails
    bytes_repr_fmt(bytes, &mut result).unwrap();
    result
}

/// Implements Python's `bytes.decode([encoding[, errors]])` method.
///
/// Converts bytes to a string.
///
/// Supports UTF-8, ASCII, and Latin-1 encodings plus common alias names.
fn bytes_decode(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (encoding, errors) = args.get_zero_one_two_args("bytes.decode", heap)?;

    // Check encoding (default UTF-8)
    let encoding_str = if let Some(enc) = encoding {
        let result = get_encoding_str(&enc, heap, interns);
        enc.drop_with_heap(heap);
        match result {
            Ok(s) => s,
            Err(e) => {
                if let Some(err) = errors {
                    err.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    } else {
        "utf-8".to_owned()
    };

    // Drop the errors argument (we don't use it yet)
    if let Some(err) = errors {
        err.drop_with_heap(heap);
    }

    // Support UTF-8, ASCII, and Latin-1 encodings.
    let normalized = encoding_str.to_lowercase();
    match normalized.as_str() {
        "utf-8" | "utf8" | "utf_8" => {
            // Decode as UTF-8
            match std::str::from_utf8(bytes) {
                Ok(s) => {
                    let heap_id = heap.allocate(HeapData::Str(Str::from(s.to_owned())))?;
                    Ok(Value::Ref(heap_id))
                }
                Err(_) => Err(ExcType::unicode_decode_error_invalid_utf8()),
            }
        }
        "ascii" | "us-ascii" => {
            // Decode as ASCII - all bytes must be <= 127
            if bytes.iter().all(|&b| b <= 127) {
                // SAFETY: we've verified all bytes are valid ASCII (0-127)
                let s = unsafe { std::str::from_utf8_unchecked(bytes) };
                let heap_id = heap.allocate(HeapData::Str(Str::from(s.to_owned())))?;
                Ok(Value::Ref(heap_id))
            } else {
                Err(ExcType::unicode_decode_error_invalid_ascii())
            }
        }
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "cp819" | "latin" | "l1" => {
            // Latin-1 is a direct one-byte to codepoint mapping.
            let decoded: String = bytes.iter().map(|&b| char::from(b)).collect();
            let heap_id = heap.allocate(HeapData::Str(Str::from(decoded)))?;
            Ok(Value::Ref(heap_id))
        }
        _ => Err(ExcType::lookup_error_unknown_encoding(&encoding_str)),
    }
}

/// Helper function to extract encoding string from a value.
fn get_encoding_str(encoding: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match encoding {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(
                "decode() argument 'encoding' must be str, not bytes",
            )),
        },
        _ => Err(ExcType::type_error("decode() argument 'encoding' must be str, not int")),
    }
}

/// Implements Python's `bytes.count(sub[, start[, end]])` method.
///
/// Returns the number of non-overlapping occurrences of the subsequence.
fn bytes_count(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (sub, start, end) = parse_bytes_sub_args("bytes.count", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let count = if sub.is_empty() {
        // Empty subsequence: count positions between each byte plus 1
        slice.len() + 1
    } else {
        count_non_overlapping(slice, &sub)
    };

    let count_i64 = i64::try_from(count).expect("count exceeds i64::MAX");
    Ok(Value::Int(count_i64))
}

/// Counts non-overlapping occurrences of needle in haystack.
fn count_non_overlapping(haystack: &[u8], needle: &[u8]) -> usize {
    let mut count = 0;
    let mut pos = 0;
    while pos + needle.len() <= haystack.len() {
        if &haystack[pos..pos + needle.len()] == needle {
            count += 1;
            pos += needle.len();
        } else {
            pos += 1;
        }
    }
    count
}

/// Implements Python's `bytes.find(sub[, start[, end]])` method.
///
/// Returns the lowest index where the subsequence is found, or -1 if not found.
fn bytes_find(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (sub, start, end) = parse_bytes_sub_args("bytes.find", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let result = if sub.is_empty() {
        // Empty subsequence: always found at start position
        Some(0)
    } else {
        find_subsequence(slice, &sub)
    };

    let idx = match result {
        Some(i) => i64::try_from(start + i).expect("index exceeds i64::MAX"),
        None => -1,
    };
    Ok(Value::Int(idx))
}

/// Finds the first occurrence of needle in haystack.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Implements Python's `bytes.index(sub[, start[, end]])` method.
///
/// Like find(), but raises ValueError if the subsequence is not found.
fn bytes_index(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (sub, start, end) = parse_bytes_sub_args("bytes.index", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let result = if sub.is_empty() {
        // Empty subsequence: always found at start position
        Some(0)
    } else {
        find_subsequence(slice, &sub)
    };

    match result {
        Some(i) => {
            let idx = i64::try_from(start + i).expect("index exceeds i64::MAX");
            Ok(Value::Int(idx))
        }
        None => Err(ExcType::value_error_subsequence_not_found()),
    }
}

/// Implements Python's `bytes.startswith(prefix[, start[, end]])` method.
///
/// Returns True if bytes starts with the specified prefix.
/// Accepts bytes or a tuple of bytes as prefix. If a tuple is given, returns True
/// if any of the prefixes match.
fn bytes_startswith(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (prefix_arg, start, end) =
        parse_bytes_prefix_suffix_args("bytes.startswith", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let result = match prefix_arg {
        PrefixSuffixArg::Single(prefix_bytes) => slice.starts_with(&prefix_bytes),
        PrefixSuffixArg::Multiple(prefixes) => prefixes.iter().any(|p| slice.starts_with(p)),
    };
    Ok(Value::Bool(result))
}

/// Implements Python's `bytes.endswith(suffix[, start[, end]])` method.
///
/// Returns True if bytes ends with the specified suffix.
/// Accepts bytes or a tuple of bytes as suffix. If a tuple is given, returns True
/// if any of the suffixes match.
fn bytes_endswith(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (suffix_arg, start, end) = parse_bytes_prefix_suffix_args("bytes.endswith", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let result = match suffix_arg {
        PrefixSuffixArg::Single(suffix_bytes) => slice.ends_with(&suffix_bytes),
        PrefixSuffixArg::Multiple(suffixes) => suffixes.iter().any(|s| slice.ends_with(s)),
    };
    Ok(Value::Bool(result))
}

/// Argument type for prefix/suffix matching methods.
///
/// Represents either a single bytes value or a tuple of bytes values
/// for matching in startswith/endswith.
enum PrefixSuffixArg {
    /// A single bytes value to match
    Single(Vec<u8>),
    /// Multiple bytes values to match (from a tuple)
    Multiple(Vec<Vec<u8>>),
}

/// Parses arguments for bytes.startswith/endswith methods.
///
/// Returns (prefix/suffix_arg, start, end) where start and end are normalized indices.
/// The prefix/suffix_arg can be a single bytes value or a tuple of bytes values.
/// Guarantees `start <= end` to prevent slice panics.
fn parse_bytes_prefix_suffix_args(
    method: &str,
    len: usize,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(PrefixSuffixArg, usize, usize)> {
    let (pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs(method));
    }

    let mut pos_iter = pos;
    let prefix_value = pos_iter
        .next()
        .ok_or_else(|| ExcType::type_error_at_least(method, 1, 0))?;
    let start_value = pos_iter.next();
    let end_value = pos_iter.next();

    // Check no extra arguments - must drop the 4th arg consumed by .next()
    if let Some(fourth) = pos_iter.next() {
        fourth.drop_with_heap(heap);
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        prefix_value.drop_with_heap(heap);
        if let Some(v) = start_value {
            v.drop_with_heap(heap);
        }
        if let Some(v) = end_value {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most(method, 3, 4));
    }

    // Extract prefix/suffix bytes (only bytes allowed, not str)
    // Use method-specific error message matching CPython
    let prefix = match extract_bytes_for_prefix_suffix(&prefix_value, method, heap, interns) {
        Ok(b) => b,
        Err(e) => {
            prefix_value.drop_with_heap(heap);
            if let Some(v) = start_value {
                v.drop_with_heap(heap);
            }
            if let Some(v) = end_value {
                v.drop_with_heap(heap);
            }
            return Err(e);
        }
    };
    prefix_value.drop_with_heap(heap);

    // Extract start (default 0)
    let start = if let Some(v) = start_value {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        match result {
            Ok(i) => normalize_bytes_index(i, len),
            Err(e) => {
                if let Some(ev) = end_value {
                    ev.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    } else {
        0
    };

    // Extract end (default len)
    let end = if let Some(v) = end_value {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        normalize_bytes_index(result?, len)
    } else {
        len
    };

    // Ensure start <= end to prevent slice panics
    let end = end.max(start);

    Ok((prefix, start, end))
}

/// Extracts bytes (or tuple of bytes) for startswith/endswith methods.
///
/// Returns `PrefixSuffixArg::Single` for a single bytes value, or
/// `PrefixSuffixArg::Multiple` for a tuple of bytes values.
fn extract_bytes_for_prefix_suffix(
    value: &Value,
    method: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<PrefixSuffixArg> {
    // Extract the method name (e.g., "startswith" from "bytes.startswith")
    let method_name = method.strip_prefix("bytes.").unwrap_or(method);

    match value {
        Value::InternBytes(id) => Ok(PrefixSuffixArg::Single(interns.get_bytes(*id).to_vec())),
        Value::InternString(_) => Err(ExcType::type_error(format!(
            "{method_name} first arg must be bytes or a tuple of bytes, not str"
        ))),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(b) => Ok(PrefixSuffixArg::Single(b.as_slice().to_vec())),
            HeapData::Str(_) => Err(ExcType::type_error(format!(
                "{method_name} first arg must be bytes or a tuple of bytes, not str"
            ))),
            HeapData::Tuple(tuple) => {
                // Extract each element as bytes
                let items = tuple.as_vec();
                let mut prefixes = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    if let Ok(b) = extract_single_bytes_for_prefix_suffix(item, heap, interns) {
                        prefixes.push(b);
                    } else {
                        let item_type = item.py_type(heap);
                        return Err(ExcType::type_error(format!(
                            "{method_name} first arg must be bytes or a tuple of bytes, \
                             not tuple containing {item_type} at index {i}"
                        )));
                    }
                }
                Ok(PrefixSuffixArg::Multiple(prefixes))
            }
            _ => Err(ExcType::type_error(format!(
                "{method_name} first arg must be bytes or a tuple of bytes, not {}",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{method_name} first arg must be bytes or a tuple of bytes, not {}",
            value.py_type(heap)
        ))),
    }
}

/// Extracts a single bytes value for tuple element in startswith/endswith.
fn extract_single_bytes_for_prefix_suffix(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::InternString(_) => Err(ExcType::type_error("expected bytes, not str")),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(b) => Ok(b.as_slice().to_vec()),
            _ => Err(ExcType::type_error("expected bytes")),
        },
        _ => Err(ExcType::type_error("expected bytes")),
    }
}

/// Extracts bytes from a Value (bytes only, NOT str - matches CPython behavior).
///
/// CPython raises `TypeError: a bytes-like object is required, not 'str'` when
/// a str is passed to bytes methods like find, count, index, startswith, endswith.
fn extract_bytes_only(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::InternString(_) => Err(ExcType::type_error("a bytes-like object is required, not 'str'")),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(b) => Ok(b.as_slice().to_vec()),
            HeapData::Str(_) => Err(ExcType::type_error("a bytes-like object is required, not 'str'")),
            _ => Err(ExcType::type_error("a bytes-like object is required")),
        },
        _ => Err(ExcType::type_error("a bytes-like object is required")),
    }
}

/// Parses arguments for bytes.find/count/index methods.
///
/// Returns (sub_bytes, start, end) where start and end are normalized indices.
/// Guarantees `start <= end` to prevent slice panics.
fn parse_bytes_sub_args(
    method: &str,
    len: usize,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Vec<u8>, usize, usize)> {
    let (pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs(method));
    }

    let mut pos_iter = pos;
    let sub_value = pos_iter
        .next()
        .ok_or_else(|| ExcType::type_error_at_least(method, 1, 0))?;
    let start_value = pos_iter.next();
    let end_value = pos_iter.next();

    // Check no extra arguments - must drop the 4th arg consumed by .next()
    if let Some(fourth) = pos_iter.next() {
        fourth.drop_with_heap(heap);
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        sub_value.drop_with_heap(heap);
        if let Some(v) = start_value {
            v.drop_with_heap(heap);
        }
        if let Some(v) = end_value {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most(method, 3, 4));
    }

    // Extract sub bytes (only bytes allowed, not str)
    let sub = match extract_bytes_only(&sub_value, heap, interns) {
        Ok(b) => b,
        Err(e) => {
            sub_value.drop_with_heap(heap);
            if let Some(v) = start_value {
                v.drop_with_heap(heap);
            }
            if let Some(v) = end_value {
                v.drop_with_heap(heap);
            }
            return Err(e);
        }
    };
    sub_value.drop_with_heap(heap);

    // Extract start (default 0)
    let start = if let Some(v) = start_value {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        match result {
            Ok(i) => normalize_bytes_index(i, len),
            Err(e) => {
                if let Some(ev) = end_value {
                    ev.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    } else {
        0
    };

    // Extract end (default len)
    let end = if let Some(v) = end_value {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        normalize_bytes_index(result?, len)
    } else {
        len
    };

    // Ensure start <= end to prevent slice panics (Python treats start > end as empty slice)
    let end = end.max(start);

    Ok((sub, start, end))
}

/// Normalizes a Python-style bytes index to a valid index in range [0, len].
fn normalize_bytes_index(index: i64, len: usize) -> usize {
    if index < 0 {
        let abs_index = usize::try_from(-index).unwrap_or(usize::MAX);
        len.saturating_sub(abs_index)
    } else {
        usize::try_from(index).unwrap_or(len).min(len)
    }
}

// =============================================================================
// Simple transformations (no arguments)
// =============================================================================

/// Implements Python's `bytes.lower()` method.
///
/// Returns a copy of the bytes with all ASCII uppercase characters converted to lowercase.
fn bytes_lower(bytes: &[u8], heap: &mut Heap<impl ResourceTracker>, is_bytearray: bool) -> RunResult<Value> {
    let result: Vec<u8> = bytes.iter().map(|&b| b.to_ascii_lowercase()).collect();
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.upper()` method.
///
/// Returns a copy of the bytes with all ASCII lowercase characters converted to uppercase.
fn bytes_upper(bytes: &[u8], heap: &mut Heap<impl ResourceTracker>, is_bytearray: bool) -> RunResult<Value> {
    let result: Vec<u8> = bytes.iter().map(|&b| b.to_ascii_uppercase()).collect();
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.capitalize()` method.
///
/// Returns a copy of the bytes with the first byte capitalized (if ASCII) and
/// the rest lowercased.
fn bytes_capitalize(bytes: &[u8], heap: &mut Heap<impl ResourceTracker>, is_bytearray: bool) -> RunResult<Value> {
    let mut result = Vec::with_capacity(bytes.len());
    if let Some((&first, rest)) = bytes.split_first() {
        result.push(first.to_ascii_uppercase());
        for &b in rest {
            result.push(b.to_ascii_lowercase());
        }
    }
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.title()` method.
///
/// Returns a titlecased version of the bytes where words start with an uppercase
/// ASCII character and the remaining characters are lowercase.
fn bytes_title(bytes: &[u8], heap: &mut Heap<impl ResourceTracker>, is_bytearray: bool) -> RunResult<Value> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut prev_is_cased = false;

    for &b in bytes {
        if prev_is_cased {
            result.push(b.to_ascii_lowercase());
        } else {
            result.push(b.to_ascii_uppercase());
        }
        prev_is_cased = b.is_ascii_alphabetic();
    }

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.swapcase()` method.
///
/// Returns a copy of the bytes with ASCII uppercase characters converted to
/// lowercase and vice versa.
fn bytes_swapcase(bytes: &[u8], heap: &mut Heap<impl ResourceTracker>, is_bytearray: bool) -> RunResult<Value> {
    let result: Vec<u8> = bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_uppercase() {
                b.to_ascii_lowercase()
            } else if b.is_ascii_lowercase() {
                b.to_ascii_uppercase()
            } else {
                b
            }
        })
        .collect();
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

// =============================================================================
// Predicate methods (no arguments, return bool)
// =============================================================================

/// Implements Python's `bytes.isalpha()` method.
///
/// Returns True if all bytes in the bytes are ASCII letters and there is at least one byte.
fn bytes_isalpha(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| b.is_ascii_alphabetic())
}

/// Implements Python's `bytes.isdigit()` method.
///
/// Returns True if all bytes in the bytes are ASCII digits and there is at least one byte.
fn bytes_isdigit(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| b.is_ascii_digit())
}

/// Implements Python's `bytes.isalnum()` method.
///
/// Returns True if all bytes in the bytes are ASCII alphanumeric and there is at least one byte.
fn bytes_isalnum(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| b.is_ascii_alphanumeric())
}

/// Implements Python's `bytes.isspace()` method.
///
/// Returns True if all bytes in the bytes are ASCII whitespace and there is at least one byte.
fn bytes_isspace(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| is_py_whitespace(b))
}

/// Implements Python's `bytes.islower()` method.
///
/// Returns True if all cased bytes are lowercase and there is at least one cased byte.
fn bytes_islower(bytes: &[u8]) -> bool {
    let mut has_cased = false;
    for &b in bytes {
        if b.is_ascii_uppercase() {
            return false;
        }
        if b.is_ascii_lowercase() {
            has_cased = true;
        }
    }
    has_cased
}

/// Implements Python's `bytes.isupper()` method.
///
/// Returns True if all cased bytes are uppercase and there is at least one cased byte.
fn bytes_isupper(bytes: &[u8]) -> bool {
    let mut has_cased = false;
    for &b in bytes {
        if b.is_ascii_lowercase() {
            return false;
        }
        if b.is_ascii_uppercase() {
            has_cased = true;
        }
    }
    has_cased
}

/// Implements Python's `bytes.istitle()` method.
///
/// Returns True if the bytes are titlecased: uppercase characters follow
/// uncased characters and lowercase characters follow cased characters.
fn bytes_istitle(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }

    let mut prev_cased = false;
    let mut has_cased = false;

    for &b in bytes {
        if b.is_ascii_uppercase() {
            if prev_cased {
                return false;
            }
            prev_cased = true;
            has_cased = true;
        } else if b.is_ascii_lowercase() {
            if !prev_cased {
                return false;
            }
            prev_cased = true;
            has_cased = true;
        } else {
            prev_cased = false;
        }
    }

    has_cased
}

// =============================================================================
// Search methods
// =============================================================================

/// Implements Python's `bytes.rfind(sub[, start[, end]])` method.
///
/// Returns the highest index where the subsequence is found, or -1 if not found.
fn bytes_rfind(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (sub, start, end) = parse_bytes_sub_args("bytes.rfind", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let result = if sub.is_empty() {
        // Empty subsequence: always found at end position
        Some(slice.len())
    } else {
        rfind_subsequence(slice, &sub)
    };

    let idx = match result {
        Some(i) => i64::try_from(start + i).expect("index exceeds i64::MAX"),
        None => -1,
    };
    Ok(Value::Int(idx))
}

/// Finds the last occurrence of needle in haystack.
fn rfind_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|window| window == needle)
}

/// Implements Python's `bytes.rindex(sub[, start[, end]])` method.
///
/// Like rfind(), but raises ValueError if the subsequence is not found.
fn bytes_rindex(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (sub, start, end) = parse_bytes_sub_args("bytes.rindex", bytes.len(), args, heap, interns)?;

    let slice = &bytes[start..end];
    let result = if sub.is_empty() {
        Some(slice.len())
    } else {
        rfind_subsequence(slice, &sub)
    };

    match result {
        Some(i) => {
            let idx = i64::try_from(start + i).expect("index exceeds i64::MAX");
            Ok(Value::Int(idx))
        }
        None => Err(ExcType::value_error_subsequence_not_found()),
    }
}

// =============================================================================
// Strip/trim methods
// =============================================================================

/// Implements Python's `bytes.strip([chars])` method.
///
/// Returns a copy of the bytes with leading and trailing bytes removed.
/// If chars is not specified, ASCII whitespace bytes are removed.
fn bytes_strip(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let chars = parse_bytes_strip_arg("bytes.strip", args, heap, interns)?;
    let result = match &chars {
        Some(c) => bytes_strip_both(bytes, c),
        None => bytes_strip_whitespace_both(bytes),
    };
    allocate_bytes_or_bytearray(result.to_vec(), heap, is_bytearray)
}

/// Implements Python's `bytes.lstrip([chars])` method.
///
/// Returns a copy of the bytes with leading bytes removed.
fn bytes_lstrip(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let chars = parse_bytes_strip_arg("bytes.lstrip", args, heap, interns)?;
    let result = match &chars {
        Some(c) => bytes_strip_start(bytes, c),
        None => bytes_strip_whitespace_start(bytes),
    };
    allocate_bytes_or_bytearray(result.to_vec(), heap, is_bytearray)
}

/// Implements Python's `bytes.rstrip([chars])` method.
///
/// Returns a copy of the bytes with trailing bytes removed.
fn bytes_rstrip(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let chars = parse_bytes_strip_arg("bytes.rstrip", args, heap, interns)?;
    let result = match &chars {
        Some(c) => bytes_strip_end(bytes, c),
        None => bytes_strip_whitespace_end(bytes),
    };
    allocate_bytes_or_bytearray(result.to_vec(), heap, is_bytearray)
}

/// Parses the optional chars argument for bytes strip methods.
fn parse_bytes_strip_arg(
    method: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Vec<u8>>> {
    let value = args.get_zero_one_arg(method, heap)?;
    match value {
        None => Ok(None),
        Some(Value::None) => Ok(None),
        Some(v) => {
            let result = extract_bytes_only(&v, heap, interns)?;
            v.drop_with_heap(heap);
            Ok(Some(result))
        }
    }
}

/// Strips bytes in `chars` from both ends of the byte slice.
fn bytes_strip_both<'a>(bytes: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = bytes.iter().position(|b| !chars.contains(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !chars.contains(b))
        .map_or(start, |pos| pos + 1);
    &bytes[start..end]
}

/// Strips bytes in `chars` from the start of the byte slice.
fn bytes_strip_start<'a>(bytes: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let start = bytes.iter().position(|b| !chars.contains(b)).unwrap_or(bytes.len());
    &bytes[start..]
}

/// Strips bytes in `chars` from the end of the byte slice.
fn bytes_strip_end<'a>(bytes: &'a [u8], chars: &[u8]) -> &'a [u8] {
    let end = bytes.iter().rposition(|b| !chars.contains(b)).map_or(0, |pos| pos + 1);
    &bytes[..end]
}

/// Strips ASCII whitespace from both ends of the byte slice.
fn bytes_strip_whitespace_both(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| !is_py_whitespace(*b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !is_py_whitespace(*b))
        .map_or(start, |pos| pos + 1);
    &bytes[start..end]
}

/// Strips ASCII whitespace from the start of the byte slice.
fn bytes_strip_whitespace_start(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| !is_py_whitespace(*b)).unwrap_or(bytes.len());
    &bytes[start..]
}

/// Strips ASCII whitespace from the end of the byte slice.
fn bytes_strip_whitespace_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|b| !is_py_whitespace(*b))
        .map_or(0, |pos| pos + 1);
    &bytes[..end]
}

/// Implements Python's `bytes.removeprefix(prefix)` method.
///
/// If the bytes start with the prefix, return bytes[len(prefix):].
/// Otherwise, return a copy of the original bytes.
fn bytes_removeprefix(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let prefix_value = args.get_one_arg("bytes.removeprefix", heap)?;
    let prefix = extract_bytes_only(&prefix_value, heap, interns)?;
    prefix_value.drop_with_heap(heap);

    let result = if bytes.starts_with(&prefix) {
        bytes[prefix.len()..].to_vec()
    } else {
        bytes.to_vec()
    };
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.removesuffix(suffix)` method.
///
/// If the bytes end with the suffix, return bytes[:-len(suffix)].
/// Otherwise, return a copy of the original bytes.
fn bytes_removesuffix(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let suffix_value = args.get_one_arg("bytes.removesuffix", heap)?;
    let suffix = extract_bytes_only(&suffix_value, heap, interns)?;
    suffix_value.drop_with_heap(heap);

    let result = if bytes.ends_with(&suffix) && !suffix.is_empty() {
        bytes[..bytes.len() - suffix.len()].to_vec()
    } else {
        bytes.to_vec()
    };
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

// =============================================================================
// Split methods
// =============================================================================

/// Implements Python's `bytes.split([sep[, maxsplit]])` method.
///
/// Returns a list of the bytes split by the separator.
fn bytes_split(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (sep, maxsplit) = parse_bytes_split_args("bytes.split", args, heap, interns)?;

    let parts: Vec<&[u8]> = match &sep {
        Some(sep) => {
            if sep.is_empty() {
                return Err(ExcType::value_error_empty_separator());
            }
            if maxsplit < 0 {
                bytes_split_by_seq(bytes, sep)
            } else {
                let max = usize::try_from(maxsplit).unwrap_or(usize::MAX);
                bytes_splitn_by_seq(bytes, sep, max + 1)
            }
        }
        None => {
            if maxsplit < 0 {
                bytes_split_whitespace(bytes)
            } else {
                let max = usize::try_from(maxsplit).unwrap_or(usize::MAX);
                bytes_splitn_whitespace(bytes, max)
            }
        }
    };

    let mut list_items = Vec::with_capacity(parts.len());
    for part in parts {
        list_items.push(allocate_bytes_or_bytearray(part.to_vec(), heap, is_bytearray)?);
    }

    let list = List::new(list_items);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Implements Python's `bytes.rsplit([sep[, maxsplit]])` method.
///
/// Returns a list of the bytes split by the separator, splitting from the right.
fn bytes_rsplit(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (sep, maxsplit) = parse_bytes_split_args("bytes.rsplit", args, heap, interns)?;

    let parts: Vec<&[u8]> = match &sep {
        Some(sep) => {
            if sep.is_empty() {
                return Err(ExcType::value_error_empty_separator());
            }
            if maxsplit < 0 {
                bytes_split_by_seq(bytes, sep)
            } else {
                let max = usize::try_from(maxsplit).unwrap_or(usize::MAX);
                bytes_rsplitn_by_seq(bytes, sep, max + 1)
            }
        }
        None => {
            if maxsplit < 0 {
                bytes_split_whitespace(bytes)
            } else {
                let max = usize::try_from(maxsplit).unwrap_or(usize::MAX);
                bytes_rsplitn_whitespace(bytes, max)
            }
        }
    };

    let mut list_items = Vec::with_capacity(parts.len());
    for part in parts {
        list_items.push(allocate_bytes_or_bytearray(part.to_vec(), heap, is_bytearray)?);
    }

    let list = List::new(list_items);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Parses arguments for bytes split methods.
fn parse_bytes_split_args(
    method: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<Vec<u8>>, i64)> {
    let (pos, kwargs) = args.into_parts();

    let mut pos_iter = pos;
    let sep_value = pos_iter.next();
    let maxsplit_value = pos_iter.next();

    // Check no extra positional arguments
    if pos_iter.next().is_some() {
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        if let Some(v) = sep_value {
            v.drop_with_heap(heap);
        }
        if let Some(v) = maxsplit_value {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(method, 2, 3));
    }

    // Extract positional sep (default None)
    // Drop before propagating error to avoid refcount leak
    let mut has_pos_sep = sep_value.is_some();
    let mut sep = if let Some(v) = sep_value {
        if matches!(v, Value::None) {
            v.drop_with_heap(heap);
            None
        } else {
            let result = extract_bytes_only(&v, heap, interns);
            v.drop_with_heap(heap);
            match result {
                Ok(r) => Some(r),
                Err(e) => {
                    if let Some(ms) = maxsplit_value {
                        ms.drop_with_heap(heap);
                    }
                    kwargs.drop_with_heap(heap);
                    return Err(e);
                }
            }
        }
    } else {
        None
    };

    // Extract positional maxsplit (default -1)
    // Drop before propagating error to avoid refcount leak
    let mut has_pos_maxsplit = maxsplit_value.is_some();
    let mut maxsplit = if let Some(v) = maxsplit_value {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        match result {
            Ok(r) => r,
            Err(e) => {
                kwargs.drop_with_heap(heap);
                return Err(e);
            }
        }
    } else {
        -1
    };

    // Process kwargs - use while let to allow draining on error
    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (k, v) in kwargs_iter {
                k.drop_with_heap(heap);
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);
        match key_str {
            "sep" => {
                if has_pos_sep {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    for (k, v) in kwargs_iter {
                        k.drop_with_heap(heap);
                        v.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error(format!(
                        "{method}() got multiple values for argument 'sep'"
                    )));
                }
                if matches!(value, Value::None) {
                    sep = None;
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                } else {
                    // Drop before propagating error to avoid refcount leak
                    let result = extract_bytes_only(&value, heap, interns);
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    match result {
                        Ok(r) => sep = Some(r),
                        Err(e) => {
                            for (k, v) in kwargs_iter {
                                k.drop_with_heap(heap);
                                v.drop_with_heap(heap);
                            }
                            return Err(e);
                        }
                    }
                }
                has_pos_sep = true;
            }
            "maxsplit" => {
                if has_pos_maxsplit {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    for (k, v) in kwargs_iter {
                        k.drop_with_heap(heap);
                        v.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error(format!(
                        "{method}() got multiple values for argument 'maxsplit'"
                    )));
                }
                // Drop before propagating error to avoid refcount leak
                let result = value.as_int(heap);
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                match result {
                    Ok(r) => maxsplit = r,
                    Err(e) => {
                        for (k, v) in kwargs_iter {
                            k.drop_with_heap(heap);
                            v.drop_with_heap(heap);
                        }
                        return Err(e);
                    }
                }
                has_pos_maxsplit = true;
            }
            _ => {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                for (k, v) in kwargs_iter {
                    k.drop_with_heap(heap);
                    v.drop_with_heap(heap);
                }
                return Err(ExcType::type_error(format!(
                    "'{key_str}' is an invalid keyword argument for {method}()"
                )));
            }
        }
    }

    Ok((sep, maxsplit))
}

/// Splits bytes by a separator sequence.
fn bytes_split_by_seq<'a>(bytes: &'a [u8], sep: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut start = 0;

    while let Some(pos) = find_subsequence(&bytes[start..], sep) {
        parts.push(&bytes[start..start + pos]);
        start = start + pos + sep.len();
    }
    parts.push(&bytes[start..]);

    parts
}

/// Splits bytes by a separator sequence, returning at most n parts.
fn bytes_splitn_by_seq<'a>(bytes: &'a [u8], sep: &[u8], n: usize) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut count = 0;

    while count + 1 < n {
        if let Some(pos) = find_subsequence(&bytes[start..], sep) {
            parts.push(&bytes[start..start + pos]);
            start = start + pos + sep.len();
            count += 1;
        } else {
            break;
        }
    }
    parts.push(&bytes[start..]);

    parts
}

/// Splits bytes by a separator sequence from the right, returning at most n parts.
fn bytes_rsplitn_by_seq<'a>(bytes: &'a [u8], sep: &[u8], n: usize) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut end = bytes.len();
    let mut count = 0;

    while count + 1 < n {
        if let Some(pos) = rfind_subsequence(&bytes[..end], sep) {
            parts.push(&bytes[pos + sep.len()..end]);
            end = pos;
            count += 1;
        } else {
            break;
        }
    }
    parts.push(&bytes[..end]);
    parts.reverse();

    parts
}

/// Splits bytes by ASCII whitespace, filtering empty parts.
fn bytes_split_whitespace(bytes: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = None;

    for (i, &b) in bytes.iter().enumerate() {
        if is_py_whitespace(b) {
            if let Some(s) = start {
                parts.push(&bytes[s..i]);
                start = None;
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }

    if let Some(s) = start {
        parts.push(&bytes[s..]);
    }

    parts
}

/// Splits bytes by ASCII whitespace, returning at most maxsplit+1 parts.
fn bytes_splitn_whitespace(bytes: &[u8], maxsplit: usize) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = None;
    let mut count = 0;

    let trimmed = bytes_strip_whitespace_start(bytes);
    let offset = bytes.len() - trimmed.len();

    for (i, &b) in trimmed.iter().enumerate() {
        if is_py_whitespace(b) {
            if let Some(s) = start
                && count < maxsplit
            {
                parts.push(&bytes[offset + s..offset + i]);
                count += 1;
                start = None;
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }

    if let Some(s) = start {
        parts.push(&bytes[offset + s..]);
    }

    parts
}

/// Splits bytes by ASCII whitespace from the right, returning at most maxsplit+1 parts.
fn bytes_rsplitn_whitespace(bytes: &[u8], maxsplit: usize) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut end = None;
    let mut count = 0;

    let trimmed = bytes_strip_whitespace_end(bytes);

    for i in (0..trimmed.len()).rev() {
        let b = trimmed[i];
        if is_py_whitespace(b) {
            if let Some(e) = end
                && count < maxsplit
            {
                parts.push(&trimmed[i + 1..e]);
                count += 1;
                end = None;
            }
        } else if end.is_none() {
            end = Some(i + 1);
        }
    }

    if let Some(e) = end {
        parts.push(&trimmed[..e]);
    }

    parts.reverse();
    parts
}

/// Implements Python's `bytes.splitlines([keepends])` method.
///
/// Returns a list of the lines in the bytes, breaking at line boundaries.
fn bytes_splitlines(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let keepends = parse_bytes_splitlines_args(args, heap, interns)?;

    let mut lines = Vec::new();
    let mut start = 0;
    let len = bytes.len();

    while start < len {
        let mut end = start;
        let mut line_end = start;

        while end < len {
            match bytes[end] {
                b'\n' => {
                    line_end = end;
                    end += 1;
                    break;
                }
                b'\r' => {
                    line_end = end;
                    end += 1;
                    if end < len && bytes[end] == b'\n' {
                        end += 1;
                    }
                    break;
                }
                _ => {
                    end += 1;
                    line_end = end;
                }
            }
        }

        let line = if keepends {
            &bytes[start..end]
        } else {
            &bytes[start..line_end]
        };
        lines.push(allocate_bytes_or_bytearray(line.to_vec(), heap, is_bytearray)?);
        start = end;
    }

    let list = List::new(lines);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(heap_id))
}

/// Parses arguments for bytes.splitlines method.
fn parse_bytes_splitlines_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    let (pos, kwargs) = args.into_parts();

    let mut pos_iter = pos;
    let keepends_value = pos_iter.next();

    // Check no extra positional arguments
    if pos_iter.next().is_some() {
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        if let Some(v) = keepends_value {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("bytes.splitlines", 1, 2));
    }

    // Extract positional keepends (default false)
    let mut has_pos_keepends = keepends_value.is_some();
    let mut keepends = if let Some(v) = keepends_value {
        let result = v.py_bool(heap, interns);
        v.drop_with_heap(heap);
        result
    } else {
        false
    };

    // Process kwargs
    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);
        if key_str == "keepends" {
            if has_pos_keepends {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "bytes.splitlines() got multiple values for argument 'keepends'",
                ));
            }
            keepends = value.py_bool(heap, interns);
            has_pos_keepends = true;
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_str}' is an invalid keyword argument for bytes.splitlines()"
            )));
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    Ok(keepends)
}

/// Implements Python's `bytes.partition(sep)` method.
///
/// Splits the bytes at the first occurrence of sep, and returns a 3-tuple.
fn bytes_partition(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let sep_value = args.get_one_arg("bytes.partition", heap)?;
    let sep = extract_bytes_only(&sep_value, heap, interns)?;
    sep_value.drop_with_heap(heap);

    if sep.is_empty() {
        return Err(ExcType::value_error_empty_separator());
    }

    let (before, sep_found, after) = match find_subsequence(bytes, &sep) {
        Some(pos) => (&bytes[..pos], &sep[..], &bytes[pos + sep.len()..]),
        None => (bytes, &[][..], &[][..]),
    };

    let before_val = allocate_bytes_or_bytearray(before.to_vec(), heap, is_bytearray)?;
    let sep_val = allocate_bytes_or_bytearray(sep_found.to_vec(), heap, is_bytearray)?;
    let after_val = allocate_bytes_or_bytearray(after.to_vec(), heap, is_bytearray)?;

    Ok(crate::types::allocate_tuple(
        smallvec![before_val, sep_val, after_val],
        heap,
    )?)
}

/// Implements Python's `bytes.rpartition(sep)` method.
///
/// Splits the bytes at the last occurrence of sep, and returns a 3-tuple.
fn bytes_rpartition(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let sep_value = args.get_one_arg("bytes.rpartition", heap)?;
    let sep = extract_bytes_only(&sep_value, heap, interns)?;
    sep_value.drop_with_heap(heap);

    if sep.is_empty() {
        return Err(ExcType::value_error_empty_separator());
    }

    let (before, sep_found, after) = match rfind_subsequence(bytes, &sep) {
        Some(pos) => (&bytes[..pos], &sep[..], &bytes[pos + sep.len()..]),
        None => (&[][..], &[][..], bytes),
    };

    let before_val = allocate_bytes_or_bytearray(before.to_vec(), heap, is_bytearray)?;
    let sep_val = allocate_bytes_or_bytearray(sep_found.to_vec(), heap, is_bytearray)?;
    let after_val = allocate_bytes_or_bytearray(after.to_vec(), heap, is_bytearray)?;

    Ok(crate::types::allocate_tuple(
        smallvec![before_val, sep_val, after_val],
        heap,
    )?)
}

// =============================================================================
// Replace/padding methods
// =============================================================================

/// Implements Python's `bytes.replace(old, new[, count])` method.
///
/// Returns a copy with all occurrences of old replaced by new.
fn bytes_replace(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (old, new, count) = parse_bytes_replace_args("bytes.replace", args, heap, interns)?;

    let result = if count < 0 {
        bytes_replace_all(bytes, &old, &new)
    } else {
        let n = usize::try_from(count).unwrap_or(usize::MAX);
        bytes_replace_n(bytes, &old, &new, n)
    };

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Parses arguments for bytes.replace method.
fn parse_bytes_replace_args(
    method: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Vec<u8>, Vec<u8>, i64)> {
    let (pos, kwargs) = args.into_parts();

    let mut pos_iter = pos;
    let Some(old_value) = pos_iter.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method, 2, 0));
    };
    let Some(new_value) = pos_iter.next() else {
        old_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method, 2, 1));
    };
    let count_value = pos_iter.next();

    // Check no extra positional arguments
    if pos_iter.next().is_some() {
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        old_value.drop_with_heap(heap);
        new_value.drop_with_heap(heap);
        if let Some(v) = count_value {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(method, 3, 4));
    }

    // Drop before propagating error to avoid refcount leak
    let old_result = extract_bytes_only(&old_value, heap, interns);
    old_value.drop_with_heap(heap);
    let old = match old_result {
        Ok(o) => o,
        Err(e) => {
            new_value.drop_with_heap(heap);
            if let Some(v) = count_value {
                v.drop_with_heap(heap);
            }
            kwargs.drop_with_heap(heap);
            return Err(e);
        }
    };

    // Drop before propagating error to avoid refcount leak
    let new_result = extract_bytes_only(&new_value, heap, interns);
    new_value.drop_with_heap(heap);
    let new = match new_result {
        Ok(n) => n,
        Err(e) => {
            if let Some(v) = count_value {
                v.drop_with_heap(heap);
            }
            kwargs.drop_with_heap(heap);
            return Err(e);
        }
    };

    let mut has_pos_count = count_value.is_some();
    let mut count = if let Some(v) = count_value {
        // Drop before propagating error to avoid refcount leak
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        match result {
            Ok(c) => c,
            Err(e) => {
                kwargs.drop_with_heap(heap);
                return Err(e);
            }
        }
    } else {
        -1
    };

    // Process kwargs - use while let to allow draining on error
    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (k, v) in kwargs_iter {
                k.drop_with_heap(heap);
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);
        if key_str == "count" {
            if has_pos_count {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                for (k, v) in kwargs_iter {
                    k.drop_with_heap(heap);
                    v.drop_with_heap(heap);
                }
                return Err(ExcType::type_error(format!(
                    "{method}() got multiple values for argument 'count'"
                )));
            }
            // Drop before propagating error to avoid refcount leak
            let result = value.as_int(heap);
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            match result {
                Ok(c) => count = c,
                Err(e) => {
                    for (k, v) in kwargs_iter {
                        k.drop_with_heap(heap);
                        v.drop_with_heap(heap);
                    }
                    return Err(e);
                }
            }
            has_pos_count = true;
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (k, v) in kwargs_iter {
                k.drop_with_heap(heap);
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error(format!(
                "'{key_str}' is an invalid keyword argument for {method}()"
            )));
        }
    }

    Ok((old, new, count))
}

/// Replaces all occurrences of `old` with `new` in bytes.
fn bytes_replace_all(bytes: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    if old.is_empty() {
        // Empty pattern: insert new before each byte and at the end
        let mut result = Vec::with_capacity(bytes.len() + new.len() * (bytes.len() + 1));
        for &b in bytes {
            result.extend_from_slice(new);
            result.push(b);
        }
        result.extend_from_slice(new);
        result
    } else {
        let mut result = Vec::new();
        let mut start = 0;
        while let Some(pos) = find_subsequence(&bytes[start..], old) {
            result.extend_from_slice(&bytes[start..start + pos]);
            result.extend_from_slice(new);
            start = start + pos + old.len();
        }
        result.extend_from_slice(&bytes[start..]);
        result
    }
}

/// Replaces at most n occurrences of `old` with `new` in bytes.
fn bytes_replace_n(bytes: &[u8], old: &[u8], new: &[u8], n: usize) -> Vec<u8> {
    if old.is_empty() {
        // Empty pattern: insert new before each byte (up to n times)
        let mut result = Vec::new();
        let mut count = 0;
        for &b in bytes {
            if count < n {
                result.extend_from_slice(new);
                count += 1;
            }
            result.push(b);
        }
        if count < n {
            result.extend_from_slice(new);
        }
        result
    } else {
        let mut result = Vec::new();
        let mut start = 0;
        let mut count = 0;
        while count < n {
            if let Some(pos) = find_subsequence(&bytes[start..], old) {
                result.extend_from_slice(&bytes[start..start + pos]);
                result.extend_from_slice(new);
                start = start + pos + old.len();
                count += 1;
            } else {
                break;
            }
        }
        result.extend_from_slice(&bytes[start..]);
        result
    }
}

/// Implements Python's `bytes.center(width[, fillbyte])` method.
///
/// Returns centered in a bytes of length width.
fn bytes_center(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (width, fillbyte) = parse_bytes_justify_args("bytes.center", args, heap, interns)?;
    let len = bytes.len();

    let result = if width <= len {
        bytes.to_vec()
    } else {
        let total_pad = width - len;
        // Match CPython's tie-break when width-len is odd:
        // add one extra fill byte on the left only for odd width/even len.
        let left_pad = total_pad / 2 + usize::from((total_pad & width & 1) != 0);
        let right_pad = total_pad - left_pad;
        let mut result = Vec::with_capacity(width);
        for _ in 0..left_pad {
            result.push(fillbyte);
        }
        result.extend_from_slice(bytes);
        for _ in 0..right_pad {
            result.push(fillbyte);
        }
        result
    };

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.ljust(width[, fillbyte])` method.
///
/// Returns left-justified in a bytes of length width.
fn bytes_ljust(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (width, fillbyte) = parse_bytes_justify_args("bytes.ljust", args, heap, interns)?;
    let len = bytes.len();

    let result = if width <= len {
        bytes.to_vec()
    } else {
        let pad = width - len;
        let mut result = Vec::with_capacity(width);
        result.extend_from_slice(bytes);
        for _ in 0..pad {
            result.push(fillbyte);
        }
        result
    };

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Implements Python's `bytes.rjust(width[, fillbyte])` method.
///
/// Returns right-justified in a bytes of length width.
fn bytes_rjust(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (width, fillbyte) = parse_bytes_justify_args("bytes.rjust", args, heap, interns)?;
    let len = bytes.len();

    let result = if width <= len {
        bytes.to_vec()
    } else {
        let pad = width - len;
        let mut result = Vec::with_capacity(width);
        for _ in 0..pad {
            result.push(fillbyte);
        }
        result.extend_from_slice(bytes);
        result
    };

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Parses arguments for bytes justify methods (center, ljust, rjust).
fn parse_bytes_justify_args(
    method: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(usize, u8)> {
    let (pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs(method));
    }

    let mut pos_iter = pos;
    let width_value = pos_iter
        .next()
        .ok_or_else(|| ExcType::type_error_at_least(method, 1, 0))?;
    let fillbyte_value = pos_iter.next();

    // Check no extra arguments
    if pos_iter.next().is_some() {
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        width_value.drop_with_heap(heap);
        if let Some(v) = fillbyte_value {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most(method, 2, 3));
    }

    // Drop before propagating error to avoid refcount leak
    let result = width_value.as_int(heap);
    width_value.drop_with_heap(heap);
    let width_i64 = match result {
        Ok(w) => w,
        Err(e) => {
            if let Some(v) = fillbyte_value {
                v.drop_with_heap(heap);
            }
            return Err(e);
        }
    };

    let width = if width_i64 < 0 {
        0
    } else {
        usize::try_from(width_i64).unwrap_or(usize::MAX)
    };

    let fillbyte = if let Some(v) = fillbyte_value {
        // Drop before propagating error to avoid refcount leak
        let result = extract_bytes_only(&v, heap, interns);
        v.drop_with_heap(heap);
        let fill_bytes = result?;
        if fill_bytes.len() != 1 {
            return Err(ExcType::type_error(format!(
                "{method}() argument 2 must be a byte string of length 1, not bytes of length {}",
                fill_bytes.len()
            )));
        }
        fill_bytes[0]
    } else {
        b' '
    };

    Ok((width, fillbyte))
}

/// Implements Python's `bytes.zfill(width)` method.
///
/// Returns a copy of the bytes left filled with ASCII '0' digits.
fn bytes_zfill(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    is_bytearray: bool,
) -> RunResult<Value> {
    let width_value = args.get_one_arg("bytes.zfill", heap)?;
    // Drop before propagating error to avoid refcount leak
    let result = width_value.as_int(heap);
    width_value.drop_with_heap(heap);
    let width_i64 = result?;

    let width = if width_i64 < 0 {
        0
    } else {
        usize::try_from(width_i64).unwrap_or(usize::MAX)
    };
    let len = bytes.len();

    let result = if width <= len {
        bytes.to_vec()
    } else {
        let pad = width - len;
        let mut result = Vec::with_capacity(width);

        // Handle sign prefix
        if !bytes.is_empty() && (bytes[0] == b'+' || bytes[0] == b'-') {
            result.push(bytes[0]);
            result.resize(pad + 1, b'0');
            result.extend_from_slice(&bytes[1..]);
        } else {
            result.resize(pad, b'0');
            result.extend_from_slice(bytes);
        }
        result
    };

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

// =============================================================================
// Join method
// =============================================================================

/// Implements Python's `bytes.join(iterable)` method.
///
/// Joins elements of the iterable with the separator bytes.
fn bytes_join(
    separator: &[u8],
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let Ok(mut iter) = OurosIter::new(iterable, heap, interns) else {
        return Err(ExcType::type_error_join_not_iterable());
    };

    let mut result = Vec::new();
    let mut index = 0usize;

    loop {
        let item = match iter.for_next(heap, interns) {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(e) => {
                iter.drop_with_heap(heap);
                return Err(e);
            }
        };

        if index > 0 {
            result.extend_from_slice(separator);
        }

        // Check item is bytes or bytearray and extract its content
        match &item {
            Value::InternBytes(id) => {
                result.extend_from_slice(interns.get_bytes(*id));
                item.drop_with_heap(heap);
            }
            Value::Ref(heap_id) => {
                if let HeapData::Bytes(b) = heap.get(*heap_id) {
                    result.extend_from_slice(b.as_slice());
                    item.drop_with_heap(heap);
                } else if let HeapData::Bytearray(b) = heap.get(*heap_id) {
                    result.extend_from_slice(b.as_slice());
                    item.drop_with_heap(heap);
                } else {
                    let t = item.py_type(heap);
                    item.drop_with_heap(heap);
                    iter.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "sequence item {index}: expected a bytes-like object, {t} found"
                    )));
                }
            }
            _ => {
                let t = item.py_type(heap);
                item.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "sequence item {index}: expected a bytes-like object, {t} found"
                )));
            }
        }

        index += 1;
    }

    iter.drop_with_heap(heap);
    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

// =============================================================================
// Hex method
// =============================================================================

/// Implements Python's `bytes.hex([sep[, bytes_per_sep]])` method.
///
/// Returns a string containing the hexadecimal representation of the bytes.
fn bytes_hex(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (sep, bytes_per_sep) = parse_bytes_hex_args(args, heap, interns)?;

    let hex_chars: Vec<char> = bytes
        .iter()
        .flat_map(|b| {
            let hi = (b >> 4) & 0xf;
            let lo = b & 0xf;
            let hi_char = if hi < 10 {
                (b'0' + hi) as char
            } else {
                (b'a' + hi - 10) as char
            };
            let lo_char = if lo < 10 {
                (b'0' + lo) as char
            } else {
                (b'a' + lo - 10) as char
            };
            [hi_char, lo_char]
        })
        .collect();

    let result = if let Some(sep) = sep {
        if bytes_per_sep == 0 || bytes.is_empty() {
            hex_chars.iter().collect()
        } else {
            // Insert separator every `bytes_per_sep` bytes (2*bytes_per_sep hex chars)
            let chars_per_group = usize::try_from(bytes_per_sep.unsigned_abs()).unwrap_or(usize::MAX) * 2;
            let mut result = String::new();

            if bytes_per_sep > 0 {
                // Positive: count from right, so partial group is at the START
                let total_len = hex_chars.len();
                let first_chunk_len = total_len % chars_per_group;
                let first_chunk_len = if first_chunk_len == 0 {
                    chars_per_group
                } else {
                    first_chunk_len
                };

                result.extend(&hex_chars[..first_chunk_len]);
                for chunk in hex_chars[first_chunk_len..].chunks(chars_per_group) {
                    result.push_str(&sep);
                    result.extend(chunk);
                }
            } else {
                // Negative: count from left, so partial group is at the END
                for (i, chunk) in hex_chars.chunks(chars_per_group).enumerate() {
                    if i > 0 {
                        result.push_str(&sep);
                    }
                    result.extend(chunk);
                }
            }
            result
        }
    } else {
        hex_chars.iter().collect()
    };

    crate::types::str::allocate_string(result, heap)
}

/// Parses arguments for bytes.hex method.
fn parse_bytes_hex_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<String>, i64)> {
    let (pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("bytes.hex"));
    }

    let mut pos_iter = pos;
    let sep_value = pos_iter.next();
    let bytes_per_sep_value = pos_iter.next();

    // Check no extra arguments
    if pos_iter.next().is_some() {
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        if let Some(v) = sep_value {
            v.drop_with_heap(heap);
        }
        if let Some(v) = bytes_per_sep_value {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("bytes.hex", 2, 3));
    }

    let sep = if let Some(v) = sep_value {
        let sep_str = match &v {
            Value::InternString(id) => {
                let s = interns.get_str(*id);
                // Must be single ASCII character
                if s.len() != 1 || !s.is_ascii() {
                    v.drop_with_heap(heap);
                    if let Some(bv) = bytes_per_sep_value {
                        bv.drop_with_heap(heap);
                    }
                    return Err(
                        SimpleException::new_msg(ExcType::ValueError, "sep must be a single ASCII character").into(),
                    );
                }
                s.to_owned()
            }
            Value::Ref(heap_id) => {
                if let HeapData::Str(s) = heap.get(*heap_id) {
                    let st = s.as_str();
                    if st.len() != 1 || !st.is_ascii() {
                        v.drop_with_heap(heap);
                        if let Some(bv) = bytes_per_sep_value {
                            bv.drop_with_heap(heap);
                        }
                        return Err(SimpleException::new_msg(
                            ExcType::ValueError,
                            "sep must be a single ASCII character",
                        )
                        .into());
                    }
                    st.to_owned()
                } else if let HeapData::Bytes(b) = heap.get(*heap_id) {
                    // Also accept single-byte bytes as separator
                    if b.len() != 1 {
                        v.drop_with_heap(heap);
                        if let Some(bv) = bytes_per_sep_value {
                            bv.drop_with_heap(heap);
                        }
                        return Err(SimpleException::new_msg(
                            ExcType::ValueError,
                            "sep must be a single ASCII character",
                        )
                        .into());
                    }
                    (b.as_slice()[0] as char).to_string()
                } else {
                    v.drop_with_heap(heap);
                    if let Some(bv) = bytes_per_sep_value {
                        bv.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error("sep must be str or bytes"));
                }
            }
            Value::InternBytes(id) => {
                let b = interns.get_bytes(*id);
                if b.len() != 1 {
                    v.drop_with_heap(heap);
                    if let Some(bv) = bytes_per_sep_value {
                        bv.drop_with_heap(heap);
                    }
                    return Err(
                        SimpleException::new_msg(ExcType::ValueError, "sep must be a single ASCII character").into(),
                    );
                }
                (b[0] as char).to_string()
            }
            _ => {
                v.drop_with_heap(heap);
                if let Some(bv) = bytes_per_sep_value {
                    bv.drop_with_heap(heap);
                }
                return Err(ExcType::type_error("sep must be str or bytes"));
            }
        };
        v.drop_with_heap(heap);
        Some(sep_str)
    } else {
        None
    };

    let bytes_per_sep = if let Some(v) = bytes_per_sep_value {
        if sep.is_none() {
            v.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "bytes.hex() requires sep when bytes_per_sep is given",
            ));
        }
        let result = v.as_int(heap)?;
        v.drop_with_heap(heap);
        result
    } else {
        1
    };

    Ok((sep, bytes_per_sep))
}

// =============================================================================
// fromhex classmethod
// =============================================================================

/// Implements Python's `bytes.fromhex(string)` and `bytearray.fromhex(string)` classmethods.
///
/// Creates bytes from a hexadecimal string. Whitespace is allowed between byte pairs,
/// but not between the two digits of a byte.
pub fn bytes_fromhex(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let hex_value = args.get_one_arg("bytes.fromhex", heap)?;

    let hex_str = match &hex_value {
        Value::InternString(id) => interns.get_str(*id).to_owned(),
        Value::Ref(heap_id) => {
            if let HeapData::Str(s) = heap.get(*heap_id) {
                s.as_str().to_owned()
            } else {
                hex_value.drop_with_heap(heap);
                return Err(ExcType::type_error("fromhex() argument must be str, not bytes"));
            }
        }
        _ => {
            let t = hex_value.py_type(heap);
            hex_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("fromhex() argument must be str, not {t}")));
        }
    };
    hex_value.drop_with_heap(heap);

    // CPython allows whitespace BETWEEN byte pairs, but NOT within a pair.
    // - "de ad" is valid (whitespace between pairs)
    // - "d e" or "0 1" are NOT valid (whitespace within a pair)
    // - " 01 " is valid (whitespace before/after)
    //
    // Error messages:
    // - Invalid char (including whitespace in wrong place): "non-hexadecimal number found ... at position X"
    // - Odd number of valid hex digits: "must contain an even number of hexadecimal digits"

    let mut result = Vec::new();
    let mut chars = hex_str.chars().enumerate().peekable();

    loop {
        // Skip whitespace BETWEEN byte pairs (before the high nibble)
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            chars.next();
        }

        // Get high nibble
        let Some((hi_pos, hi_char)) = chars.next() else {
            break; // End of string - we're done
        };

        let Some(hi_val) = hex_char_to_value(hi_char) else {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("non-hexadecimal number found in fromhex() arg at position {hi_pos}"),
            )
            .into());
        };

        // Get low nibble - must be IMMEDIATELY after high nibble (no whitespace)
        let Some((lo_pos, lo_char)) = chars.next() else {
            // End of string after high nibble = odd number of hex digits
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "fromhex() arg must contain an even number of hexadecimal digits",
            )
            .into());
        };

        let Some(lo_val) = hex_char_to_value(lo_char) else {
            // Invalid character (including whitespace) in low nibble position
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("non-hexadecimal number found in fromhex() arg at position {lo_pos}"),
            )
            .into());
        };

        result.push((hi_val << 4) | lo_val);
    }

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

/// Converts a hex character to its numeric value.
fn hex_char_to_value(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

// =============================================================================
// maketrans classmethod
// =============================================================================

/// Implements Python's `bytes.maketrans(from, to)` classmethod.
///
/// Returns a 256-byte translation table where each input byte maps to itself
/// unless overridden by the corresponding `from` -> `to` pair.
pub fn bytes_maketrans(args: ArgValues, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let (pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("bytes.maketrans"));
    }

    let mut pos_iter = pos;
    let from_value = pos_iter.next();
    let to_value = pos_iter.next();

    // Reject any third positional argument.
    if let Some(third) = pos_iter.next() {
        third.drop_with_heap(heap);
        for v in pos_iter {
            v.drop_with_heap(heap);
        }
        if let Some(v) = from_value {
            v.drop_with_heap(heap);
        }
        if let Some(v) = to_value {
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("maketrans expected 2 arguments, got 3"));
    }

    let Some(from_value) = from_value else {
        return Err(ExcType::type_error("maketrans expected 2 arguments, got 0"));
    };
    let Some(to_value) = to_value else {
        from_value.drop_with_heap(heap);
        return Err(ExcType::type_error("maketrans expected 2 arguments, got 1"));
    };

    let from = extract_bytes_only(&from_value, heap, interns);
    from_value.drop_with_heap(heap);
    let from = match from {
        Ok(from) => from,
        Err(err) => {
            to_value.drop_with_heap(heap);
            return Err(err);
        }
    };

    let to = extract_bytes_only(&to_value, heap, interns);
    to_value.drop_with_heap(heap);
    let to = to?;

    if from.len() != to.len() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "maketrans arguments must have same length").into());
    }

    let mut table: Vec<u8> = (0_u8..=u8::MAX).collect();
    for (src, dst) in from.into_iter().zip(to) {
        table[src as usize] = dst;
    }

    allocate_bytes(table, heap)
}

// =============================================================================
// translate method
// =============================================================================

/// Implements Python's `bytes.translate(table[, /, delete=b''])` and
/// `bytearray.translate(table[, /, delete=b''])`.
///
/// `table` is either `None` (identity mapping) or a bytes-like object of
/// length 256. Bytes present in `delete` are removed before translation.
fn bytes_translate(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    is_bytearray: bool,
) -> RunResult<Value> {
    let (table, delete) = parse_bytes_translate_args(args, heap, interns)?;

    if let Some(table) = &table
        && table.len() != 256
    {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "translation table must be 256 characters long").into(),
        );
    }

    let mut delete_mask = [false; 256];
    for byte in delete {
        delete_mask[byte as usize] = true;
    }

    let mut translated = Vec::with_capacity(bytes.len());
    match table {
        Some(table) => {
            for &byte in bytes {
                if !delete_mask[byte as usize] {
                    translated.push(table[byte as usize]);
                }
            }
        }
        None => {
            for &byte in bytes {
                if !delete_mask[byte as usize] {
                    translated.push(byte);
                }
            }
        }
    }

    allocate_bytes_or_bytearray(translated, heap, is_bytearray)
}

/// Parses arguments for `bytes.translate`/`bytearray.translate`.
///
/// The first argument (`table`) is positional-only to match CPython.
/// The optional `delete` argument can be passed positionally or by keyword.
fn parse_bytes_translate_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<Vec<u8>>, Vec<u8>)> {
    let (mut pos, kwargs) = args.into_parts();
    let positional_count = pos.len();
    let kwargs_count = kwargs.len();
    let total_count = positional_count + kwargs_count;

    if positional_count == 0 {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("translate", 1, 0));
    }

    if total_count > 2 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("translate", 2, total_count));
    }

    let table_value = pos.next().expect("positional_count > 0");
    let positional_delete_value = pos.next();

    let mut keyword_delete_value = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            table_value.drop_with_heap(heap);
            positional_delete_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_str = key_name.as_str(interns);
        key.drop_with_heap(heap);

        if key_str == "delete" {
            keyword_delete_value = Some(value);
        } else {
            value.drop_with_heap(heap);
            table_value.drop_with_heap(heap);
            positional_delete_value.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "translate() got an unexpected keyword argument '{key_str}'"
            )));
        }
    }

    let table = if matches!(table_value, Value::None) {
        table_value.drop_with_heap(heap);
        None
    } else {
        let extracted = extract_bytes_like_for_translate(&table_value, heap, interns);
        table_value.drop_with_heap(heap);
        match extracted {
            Ok(table) => Some(table),
            Err(err) => {
                positional_delete_value.drop_with_heap(heap);
                keyword_delete_value.drop_with_heap(heap);
                return Err(err);
            }
        }
    };

    let delete_value = if let Some(delete) = positional_delete_value {
        Some(delete)
    } else {
        keyword_delete_value
    };

    let delete = if let Some(delete_value) = delete_value {
        let extracted = extract_bytes_like_for_translate(&delete_value, heap, interns);
        delete_value.drop_with_heap(heap);
        extracted?
    } else {
        Vec::new()
    };

    Ok((table, delete))
}

/// Extracts a bytes-like object for `bytes.translate` arguments.
///
/// This accepts `bytes`, `bytearray`, and interned bytes literals.
fn extract_bytes_like_for_translate(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(b) | HeapData::Bytearray(b) => Ok(b.as_slice().to_vec()),
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

// =============================================================================
// expandtabs method
// =============================================================================

/// Implements Python's `bytes.expandtabs(tabsize=8)` method.
///
/// Returns a copy of the bytes where all tab characters (\t) are replaced by
/// spaces to align at tab stops. Default tabsize is 8.
fn bytes_expandtabs(
    bytes: &[u8],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    is_bytearray: bool,
) -> RunResult<Value> {
    let tabsize_value = args.get_zero_one_arg("bytes.expandtabs", heap)?;

    let tabsize = if let Some(v) = tabsize_value {
        let result = v.as_int(heap);
        v.drop_with_heap(heap);
        let size = result?;
        if size < 0 {
            // CPython allows negative tabsizes, treats them as 0
            0
        } else {
            usize::try_from(size).unwrap_or(usize::MAX)
        }
    } else {
        8 // Default tabsize
    };

    // Calculate the expanded size first
    let mut col = 0usize;
    let mut expanded_len = 0usize;

    for &b in bytes {
        if b == b'\t' {
            if tabsize > 0 {
                let spaces = tabsize - (col % tabsize);
                expanded_len += spaces;
                col += spaces;
            }
            // If tabsize is 0, tab is replaced with nothing
        } else {
            expanded_len += 1;
            if b == b'\n' || b == b'\r' {
                col = 0;
            } else {
                col += 1;
            }
        }
    }

    // Build the result
    let mut result = Vec::with_capacity(expanded_len);
    col = 0;

    for &b in bytes {
        if b == b'\t' {
            if tabsize > 0 {
                let spaces = tabsize - (col % tabsize);
                result.resize(result.len() + spaces, b' ');
                col += spaces;
            }
            // If tabsize is 0, skip the tab character
        } else {
            result.push(b);
            if b == b'\n' || b == b'\r' {
                col = 0;
            } else {
                col += 1;
            }
        }
    }

    allocate_bytes_or_bytearray(result, heap, is_bytearray)
}

// =============================================================================
// Helper function for bytes allocation
// =============================================================================

/// Allocates bytes on the heap.
fn allocate_bytes(bytes: Vec<u8>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let heap_id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(Value::Ref(heap_id))
}

/// Allocates bytearray on the heap.
fn allocate_bytearray(bytes: Vec<u8>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let heap_id = heap.allocate(HeapData::Bytearray(Bytes::new(bytes)))?;
    Ok(Value::Ref(heap_id))
}

/// Allocates bytes or bytearray on the heap based on `is_bytearray` flag.
fn allocate_bytes_or_bytearray(
    bytes: Vec<u8>,
    heap: &mut Heap<impl ResourceTracker>,
    is_bytearray: bool,
) -> RunResult<Value> {
    if is_bytearray {
        allocate_bytearray(bytes, heap)
    } else {
        allocate_bytes(bytes, heap)
    }
}
