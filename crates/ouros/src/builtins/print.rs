//! Implementation of the print() builtin function.

use crate::{
    args::{ArgValues, KwargsValues},
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{Heap, HeapData},
    intern::{Interns, StaticStrings},
    io::{PrintWriter, RedirectTarget, current_stderr_redirect, current_stdout_redirect},
    resource::ResourceTracker,
    types::{AttrCallResult, PyTrait, Str},
    value::{EitherStr, Marker, Value},
};

/// Implementation of the print() builtin function.
///
/// Supports the following keyword arguments:
/// - `sep`: separator between values (default: " ")
/// - `end`: string appended after the last value (default: "\n")
/// - `flush`: whether to flush the stream (accepted but ignored)
///
pub fn builtin_print(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    interns: &Interns,
    print: &mut impl PrintWriter,
) -> RunResult<Value> {
    // Split into positional args and kwargs
    let (positional, kwargs) = args.into_parts();

    // Extract kwargs first, consuming them - this handles cleanup on error
    let (sep, end, file) = match extract_print_kwargs(kwargs, heap, interns) {
        Ok(se) => se,
        Err(err) => {
            for value in positional {
                value.drop_with_heap(heap);
            }
            return Err(err);
        }
    };

    // Build output text first (print semantics are atomic per call).
    let mut output = String::new();
    let mut first = true;
    for value in positional {
        if first {
            first = false;
        } else if let Some(sep) = &sep {
            output.push_str(sep.as_str());
        } else {
            output.push(' ');
        }
        output.push_str(&value.py_str(heap, interns));
        value.drop_with_heap(heap);
    }

    // Append terminator.
    if let Some(end) = end {
        output.push_str(end.as_str());
    } else {
        output.push('\n');
    }

    let destination = match file {
        Some(file_target) => {
            let resolved = resolve_explicit_destination(&file_target, heap, interns);
            file_target.drop_with_heap(heap);
            resolved
        }
        None => resolve_implicit_destination(),
    };

    match destination {
        PrintDestination::Sink => {}
        PrintDestination::Stdout | PrintDestination::Stderr => {
            print.stdout_write(output.into())?;
        }
        PrintDestination::Heap(id) => {
            let text_id = heap.allocate(HeapData::Str(Str::new(output)))?;
            let write_attr = EitherStr::Heap("write".to_owned());
            let write_result = heap.call_attr_raw(id, &write_attr, ArgValues::One(Value::Ref(text_id)), interns)?;
            match write_result {
                AttrCallResult::Value(value) => value.drop_with_heap(heap),
                _ => return Err(ExcType::type_error("print file object does not support write()")),
            }
        }
    }

    Ok(Value::None)
}

/// Extracts sep and end kwargs from print() arguments.
///
/// Consumes the kwargs, dropping all values after extraction.
/// Returns (sep, end, error) where error is Some if a kwarg error occurred.
fn extract_print_kwargs(
    kwargs: KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<String>, Option<String>, Option<Value>)> {
    let mut sep: Option<String> = None;
    let mut end: Option<String> = None;
    let mut file: Option<Value> = None;
    let mut error: Option<RunError> = None;

    for (key, value) in kwargs {
        // If we already hit an error, just drop remaining values
        if error.is_some() {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            continue;
        }

        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            error = Some(SimpleException::new_msg(ExcType::TypeError, "keywords must be strings").into());
            continue;
        };

        let key_str = keyword_name.as_str(interns);
        match key_str {
            "sep" => match extract_string_kwarg(&value, "sep", heap, interns) {
                Ok(custom_sep) => sep = custom_sep,
                Err(e) => error = Some(e),
            },
            "end" => match extract_string_kwarg(&value, "end", heap, interns) {
                Ok(custom_end) => end = custom_end,
                Err(e) => error = Some(e),
            },
            "flush" => {} // Accepted but ignored (we don't buffer output)
            "file" => {
                file = Some(value.clone_with_heap(heap));
            }
            _ => {
                error = Some(ExcType::type_error_unexpected_keyword("print", key_str));
            }
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    if let Some(error) = error {
        if let Some(file) = file {
            file.drop_with_heap(heap);
        }
        Err(error)
    } else {
        Ok((sep, end, file))
    }
}

/// Extracts a string value from a print() kwarg.
///
/// The kwarg can be None (returns empty string) or a string.
/// Raises TypeError for other types.
fn extract_string_kwarg(
    value: &Value,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<String>> {
    match value {
        Value::None => Ok(None),
        Value::InternString(string_id) => Ok(Some(interns.get_str(*string_id).to_owned())),
        Value::Ref(id) => {
            if let HeapData::Str(s) = heap.get(*id) {
                return Ok(Some(s.as_str().to_owned()));
            }
            Err(SimpleException::new_msg(
                ExcType::TypeError,
                format!("{} must be None or a string, not {}", name, value.py_type(heap)),
            )
            .into())
        }
        _ => Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("{} must be None or a string, not {}", name, value.py_type(heap)),
        )
        .into()),
    }
}

/// Print output destination resolved from kwargs and active redirect stacks.
enum PrintDestination {
    Stdout,
    Stderr,
    Sink,
    Heap(crate::heap::HeapId),
}

/// Resolves output destination when `file=` was provided.
fn resolve_explicit_destination(
    file: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> PrintDestination {
    match file {
        // CPython treats `print(file=None)` the same as omitting `file=`.
        Value::None => resolve_implicit_stdout(),
        Value::Marker(Marker(StaticStrings::Stdout)) => resolve_implicit_stdout(),
        Value::Marker(Marker(StaticStrings::Stderr)) => resolve_explicit_stderr(),
        Value::Ref(id) => {
            if let HeapData::NamedTuple(named_tuple) = heap.get(*id)
                && named_tuple.name(interns) == "sys.TextIOWrapper"
            {
                return resolve_explicit_stderr();
            }
            PrintDestination::Heap(*id)
        }
        _ => PrintDestination::Stdout,
    }
}

/// Resolves output destination when `file=` was omitted.
fn resolve_implicit_destination() -> PrintDestination {
    resolve_implicit_stdout()
}

/// Resolves current stdout destination from redirect stack.
fn resolve_implicit_stdout() -> PrintDestination {
    match current_stdout_redirect() {
        Some(RedirectTarget::Sink) => PrintDestination::Sink,
        Some(RedirectTarget::Heap(id)) => PrintDestination::Heap(id),
        None => PrintDestination::Stdout,
    }
}

/// Resolves current stderr destination from redirect stack.
fn resolve_implicit_stderr() -> PrintDestination {
    match current_stderr_redirect() {
        Some(RedirectTarget::Sink) => PrintDestination::Sink,
        Some(RedirectTarget::Heap(id)) => PrintDestination::Heap(id),
        None => PrintDestination::Stderr,
    }
}

/// Resolves `print(file=sys.stderr)` with CPython's fallback semantics.
///
/// When `contextlib.redirect_stderr(None)` is active, `sys.stderr` behaves as
/// `None` and `print(..., file=sys.stderr)` falls back to stdout.
fn resolve_explicit_stderr() -> PrintDestination {
    match current_stderr_redirect() {
        Some(RedirectTarget::Sink) => resolve_implicit_stdout(),
        Some(RedirectTarget::Heap(id)) => PrintDestination::Heap(id),
        None => PrintDestination::Stderr,
    }
}
