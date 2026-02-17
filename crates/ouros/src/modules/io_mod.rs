//! Implementation of the `io` module.
//!
//! Provides in-memory stream implementations:
//! - `StringIO`: Text I/O using an in-memory buffer
//! - `BytesIO`: Binary I/O using an in-memory buffer
//!
//! These classes implement the standard I/O interface with methods like
//! `read()`, `write()`, `seek()`, `tell()`, `getvalue()`, and `close()`.

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::{Builtins, BuiltinsFunctions},
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::{AttrCallResult, Bytes, PyTrait, Type},
    value::Value,
};

/// Creates the `io` module and allocates it on the heap.
///
/// The module provides:
/// - `StringIO(initial_value='', newline='\\n')`: In-memory text stream
/// - `BytesIO(initial_bytes=b'')`: In-memory binary stream
/// - `DEFAULT_BUFFER_SIZE`: Default buffer size constant
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

    let mut module = Module::new(StaticStrings::Io);

    // StringIO class constructor
    module.set_attr(
        StaticStrings::StringIO,
        Value::ModuleFunction(crate::modules::ModuleFunctions::Io(IoFunctions::StringIO)),
        heap,
        interns,
    );

    // BytesIO class constructor
    module.set_attr(
        StaticStrings::BytesIO,
        Value::ModuleFunction(crate::modules::ModuleFunctions::Io(IoFunctions::BytesIO)),
        heap,
        interns,
    );

    // DEFAULT_BUFFER_SIZE constant
    module.set_attr(
        StaticStrings::DefaultBufferSize,
        Value::Int(131_072), // Match CPython's io.DEFAULT_BUFFER_SIZE
        heap,
        interns,
    );

    // Minimal base classes needed by stdlib imports.
    module.set_attr_str(
        "TextIOBase",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_str("RawIOBase", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_str(
        "BufferedIOBase",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;

    // Alias io.open to builtin open.
    module.set_attr_str(
        "open",
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Open)),
        heap,
        interns,
    )?;

    // File seek constants used by seek(..., whence=...)
    module.set_attr_str("SEEK_SET", Value::Int(0), heap, interns)?;
    module.set_attr_str("SEEK_CUR", Value::Int(1), heap, interns)?;
    module.set_attr_str("SEEK_END", Value::Int(2), heap, interns)?;

    // `io.UnsupportedOperation` is a dedicated class in CPython. For parity with
    // current tests, alias it to OSError so exception matching works.
    module.set_attr_str(
        "UnsupportedOperation",
        Value::Builtin(Builtins::ExcType(ExcType::OSError)),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

/// Io module functions (class constructors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum IoFunctions {
    #[strum(serialize = "StringIO")]
    StringIO,
    #[strum(serialize = "BytesIO")]
    BytesIO,
}

/// Dispatches a call to an io module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: IoFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        IoFunctions::StringIO => string_io_constructor(heap, interns, args),
        IoFunctions::BytesIO => bytes_io_constructor(heap, interns, args),
    }
}

/// Constructor for `io.StringIO(initial_value='', newline='\\n')`.
fn string_io_constructor(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    // Parse: StringIO(initial_value='', newline='\n')
    let (mut positional, kwargs) = args.into_parts();

    // Check for extra positional args first
    let first = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        first.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("StringIO", 1, 2));
    }

    let mut kwarg_pairs = match collect_kwargs(kwargs, heap, interns) {
        Ok(pairs) => pairs,
        Err(err) => {
            first.drop_with_heap(heap);
            return Err(err);
        }
    };

    let initial_value = match (first, find_and_remove_kwarg(&mut kwarg_pairs, "initial_value")) {
        (Some(positional), Some(keyword)) => {
            positional.drop_with_heap(heap);
            keyword.drop_with_heap(heap);
            drop_kwarg_values(kwarg_pairs, heap);
            return Err(ExcType::type_error(
                "argument for StringIO() given by name ('initial_value') and position (1)".to_string(),
            ));
        }
        (Some(positional), None) => positional,
        (None, Some(keyword)) => keyword,
        (None, None) => Value::InternString(StaticStrings::EmptyString.into()),
    };

    // Extract newline parameter
    let newline = find_and_remove_kwarg(&mut kwarg_pairs, "newline");

    // Check for unexpected kwargs
    if let Some((name, value)) = kwarg_pairs.pop() {
        value.drop_with_heap(heap);
        drop_kwarg_values(kwarg_pairs, heap);
        return Err(ExcType::type_error(format!(
            "'{name}' is an invalid keyword argument for StringIO()"
        )));
    }

    let content = match value_to_stringio_initial(&initial_value, heap, interns) {
        Ok(content) => content,
        Err(err) => {
            initial_value.drop_with_heap(heap);
            newline.drop_with_heap(heap);
            return Err(err);
        }
    };
    initial_value.drop_with_heap(heap);

    let newline_str = match newline {
        Some(newline_value) => {
            let parsed = match value_to_stringio_newline(&newline_value, heap, interns) {
                Ok(newline) => newline,
                Err(err) => {
                    newline_value.drop_with_heap(heap);
                    return Err(err);
                }
            };
            newline_value.drop_with_heap(heap);
            parsed
        }
        None => "\n".to_owned(),
    };

    let object = crate::types::StdlibObject::new_string_io(content, newline_str);
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Constructor for `io.BytesIO(initial_bytes=b'')`.
fn bytes_io_constructor(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    // Parse: BytesIO(initial_bytes=b'')
    let (mut positional, kwargs) = args.into_parts();

    // Check for extra positional args first
    let first = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        first.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("BytesIO", 1, 2));
    }

    let mut kwarg_pairs = match collect_kwargs(kwargs, heap, interns) {
        Ok(pairs) => pairs,
        Err(err) => {
            first.drop_with_heap(heap);
            return Err(err);
        }
    };

    let initial_bytes = match (first, find_and_remove_kwarg(&mut kwarg_pairs, "initial_bytes")) {
        (Some(positional), Some(keyword)) => {
            positional.drop_with_heap(heap);
            keyword.drop_with_heap(heap);
            drop_kwarg_values(kwarg_pairs, heap);
            return Err(ExcType::type_error_at_most("BytesIO", 1, 2));
        }
        (Some(positional), None) => positional,
        (None, Some(keyword)) => keyword,
        (None, None) => {
            let empty_bytes = Bytes::from(Vec::new());
            let id = heap.allocate(HeapData::Bytes(empty_bytes))?;
            Value::Ref(id)
        }
    };

    // Check for unexpected kwargs
    if let Some((name, value)) = kwarg_pairs.pop() {
        value.drop_with_heap(heap);
        drop_kwarg_values(kwarg_pairs, heap);
        return Err(ExcType::type_error(format!(
            "'{name}' is an invalid keyword argument for BytesIO()"
        )));
    }

    // Convert initial_bytes to Vec<u8>
    let content = match value_to_bytes(&initial_bytes, heap, interns) {
        Ok(content) => content,
        Err(err) => {
            initial_bytes.drop_with_heap(heap);
            return Err(err);
        }
    };
    initial_bytes.drop_with_heap(heap);

    let object = crate::types::StdlibObject::new_bytes_io(content);
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Finds and removes a keyword argument by name.
fn find_and_remove_kwarg(pairs: &mut Vec<(String, Value)>, name: &str) -> Option<Value> {
    pairs.iter().position(|(k, _)| k == name).map(|pos| pairs.remove(pos).1)
}

fn collect_kwargs(
    kwargs: KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<(String, Value)>> {
    let mut pairs = Vec::new();
    for (key, value) in kwargs {
        let Some(key_str) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            drop_kwarg_values(pairs, heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let name = key_str.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        pairs.push((name, value));
    }
    Ok(pairs)
}

fn drop_kwarg_values(pairs: Vec<(String, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (_, value) in pairs {
        value.drop_with_heap(heap);
    }
}

fn value_to_stringio_initial(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match value {
        Value::None => Ok(String::new()),
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "initial_value must be str or None, not {}",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "initial_value must be str or None, not {}",
            value.py_type(heap)
        ))),
    }
}

fn value_to_stringio_newline(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let newline = match value {
        Value::None => return Ok(String::new()),
        Value::InternString(id) => interns.get_str(*id).to_owned(),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => s.as_str().to_owned(),
            _ => {
                return Err(ExcType::type_error(format!(
                    "newline must be str or None, not {}",
                    value.py_type(heap)
                )));
            }
        },
        _ => {
            return Err(ExcType::type_error(format!(
                "newline must be str or None, not {}",
                value.py_type(heap)
            )));
        }
    };

    if matches!(newline.as_str(), "" | "\n" | "\r" | "\r\n") {
        Ok(newline)
    } else {
        Err(SimpleException::new_msg(ExcType::ValueError, format!("illegal newline value: '{newline}'")).into())
    }
}

/// Converts a Value to a Vec<u8>.
fn value_to_bytes(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::InternString(_) => Err(ExcType::type_error(
            "a bytes-like object is required, not 'str'".to_string(),
        )),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Bytearray(bytearray) => Ok(bytearray.as_slice().to_vec()),
            HeapData::Str(_) => Err(ExcType::type_error(
                "a bytes-like object is required, not 'str'".to_string(),
            )),
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
