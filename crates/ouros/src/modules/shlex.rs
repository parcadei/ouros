//! Minimal implementation of the `shlex` module.
//!
//! This provides deterministic helpers used by Ouros's stdlib tests:
//! `shlex.quote()` and `shlex.split()`.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, List, Module, PyTrait, Str},
    value::Value,
};

/// `shlex` module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ShlexFunctions {
    Quote,
    Split,
}

/// Creates the `shlex` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Shlex);
    module.set_attr_text(
        "quote",
        Value::ModuleFunction(ModuleFunctions::Shlex(ShlexFunctions::Quote)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "split",
        Value::ModuleFunction(ModuleFunctions::Shlex(ShlexFunctions::Split)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `shlex` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ShlexFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        ShlexFunctions::Quote => shlex_quote(heap, interns, args)?,
        ShlexFunctions::Split => shlex_split(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(result))
}

/// Implements `shlex.quote(s)`.
fn shlex_quote(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let text = extract_shlex_string(args, "quote", "s", None, heap, interns)?.0;
    let quoted = if text.is_empty() {
        "''".to_owned()
    } else if text
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_@%+=:,./-".contains(ch))
    {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    };
    let id = heap.allocate(HeapData::Str(Str::from(quoted)))?;
    Ok(Value::Ref(id))
}

/// Implements `shlex.split(s, comments=False, posix=True)`.
fn shlex_split(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (text, options) = extract_shlex_string(args, "split", "s", Some(("comments", "posix")), heap, interns)?;
    let comments = options.0.unwrap_or(false);
    let _posix = options.1.unwrap_or(true);

    let source = if comments {
        if let Some((prefix, _)) = text.split_once('#') {
            prefix
        } else {
            &text
        }
    } else {
        &text
    };

    let mut items = Vec::new();
    for token in source.split_whitespace() {
        let id = heap.allocate(HeapData::Str(Str::from(token.to_owned())))?;
        items.push(Value::Ref(id));
    }

    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Parses shared argument patterns for `quote` and `split`.
///
/// Returns `(required_text, (opt1, opt2))`.
fn extract_shlex_string(
    args: ArgValues,
    function_name: &str,
    required_name: &str,
    option_names: Option<(&str, &str)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, (Option<bool>, Option<bool>))> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let mut required = positional.next();
    let mut opt1 = positional.next();
    let mut opt2 = positional.next();

    if positional.next().is_some() {
        required.drop_with_heap(heap);
        opt1.drop_with_heap(heap);
        opt2.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(
            function_name,
            if option_names.is_some() { 3 } else { 1 },
            if option_names.is_some() { 4 } else { 2 },
            0,
        ));
    }

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            required.drop_with_heap(heap);
            opt1.drop_with_heap(heap);
            opt2.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name == required_name {
            if required.is_some() {
                value.drop_with_heap(heap);
                required.drop_with_heap(heap);
                opt1.drop_with_heap(heap);
                opt2.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, required_name));
            }
            required = Some(value);
            continue;
        }

        let Some((name1, name2)) = option_names else {
            value.drop_with_heap(heap);
            required.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
        };

        if key_name == name1 {
            if opt1.is_some() {
                value.drop_with_heap(heap);
                required.drop_with_heap(heap);
                opt1.drop_with_heap(heap);
                opt2.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, name1));
            }
            opt1 = Some(value);
            continue;
        }
        if key_name == name2 {
            if opt2.is_some() {
                value.drop_with_heap(heap);
                required.drop_with_heap(heap);
                opt1.drop_with_heap(heap);
                opt2.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, name2));
            }
            opt2 = Some(value);
            continue;
        }

        value.drop_with_heap(heap);
        required.drop_with_heap(heap);
        opt1.drop_with_heap(heap);
        opt2.drop_with_heap(heap);
        return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
    }

    let Some(required_value) = required else {
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &[required_name],
        ));
    };
    let required_text = value_to_text(required_value, heap, interns)?;

    let b1 = opt1.map(|v| {
        let b = v.py_bool(heap, interns);
        v.drop_with_heap(heap);
        b
    });
    let b2 = opt2.map(|v| {
        let b = v.py_bool(heap, interns);
        v.drop_with_heap(heap);
        b
    });

    Ok((required_text, (b1, b2)))
}

/// Converts a value into text for `shlex` APIs.
fn value_to_text(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let out = match &value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "expected string or bytes-like object, got '{}'",
                heap.get(*id).py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "expected string or bytes-like object, got '{}'",
            value.py_type(heap)
        ))),
    };
    value.drop_with_heap(heap);
    out
}
