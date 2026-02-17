//! Minimal implementation of the `html` module.
//!
//! This provides deterministic helpers used by Ouros's stdlib tests:
//! `html.escape()` and `html.unescape()`.

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Module, PyTrait, Str},
    value::Value,
};

/// `html` module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum HtmlFunctions {
    Escape,
    Unescape,
}

/// Creates the `html` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Html);
    module.set_attr_text(
        "escape",
        Value::ModuleFunction(ModuleFunctions::Html(HtmlFunctions::Escape)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "unescape",
        Value::ModuleFunction(ModuleFunctions::Html(HtmlFunctions::Unescape)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `html` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: HtmlFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        HtmlFunctions::Escape => html_escape(heap, interns, args)?,
        HtmlFunctions::Unescape => html_unescape(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(result))
}

/// Implements `html.escape(s, quote=True)`.
fn html_escape(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (s, quote) = parse_single_string_with_optional_bool(args, "escape", "s", "quote", true, heap, interns)?;
    let mut escaped = s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    if quote {
        escaped = escaped.replace('"', "&quot;").replace('\'', "&#x27;");
    }
    let id = heap.allocate(HeapData::Str(Str::from(escaped)))?;
    Ok(Value::Ref(id))
}

/// Implements `html.unescape(s)`.
fn html_unescape(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (s, _) = parse_single_string_with_optional_bool(args, "unescape", "s", "__unused__", true, heap, interns)?;
    let unescaped = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let id = heap.allocate(HeapData::Str(Str::from(unescaped)))?;
    Ok(Value::Ref(id))
}

/// Parses one required string argument and one optional bool/int-like keyword argument.
fn parse_single_string_with_optional_bool(
    args: ArgValues,
    function_name: &str,
    required_name: &str,
    optional_name: &str,
    default_optional: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, bool)> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();

    let mut required: Option<Value> = positional.next();
    let mut optional: Option<Value> = if optional_name == "__unused__" {
        None
    } else {
        positional.next()
    };
    if positional.next().is_some() {
        return Err(ExcType::type_error_too_many_positional(
            function_name,
            if optional_name == "__unused__" { 1 } else { 2 },
            if optional_name == "__unused__" { 2 } else { 3 },
            0,
        ));
    }

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            required.drop_with_heap(heap);
            optional.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name == required_name {
            if required.is_some() {
                value.drop_with_heap(heap);
                required.drop_with_heap(heap);
                optional.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, required_name));
            }
            required = Some(value);
            continue;
        }
        if optional_name != "__unused__" && key_name == optional_name {
            if optional.is_some() {
                value.drop_with_heap(heap);
                required.drop_with_heap(heap);
                optional.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, optional_name));
            }
            optional = Some(value);
            continue;
        }

        value.drop_with_heap(heap);
        required.drop_with_heap(heap);
        optional.drop_with_heap(heap);
        return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
    }

    let Some(required_value) = required else {
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &[required_name],
        ));
    };
    let required_text = html_string_arg(required_value, function_name, heap, interns)?;
    let optional_bool = if let Some(value) = optional {
        let b = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        b
    } else {
        default_optional
    };

    Ok((required_text, optional_bool))
}

/// Converts input to the `html` module string behavior used by CPython helpers.
fn html_string_arg(
    value: Value,
    function_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    defer_drop!(value, heap);
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            HeapData::Bytes(_) | HeapData::Bytearray(_) => {
                Err(ExcType::type_error("a bytes-like object is required, not 'str'"))
            }
            _ if function_name == "escape" => Err(SimpleException::new_msg(
                ExcType::AttributeError,
                format!("'{}' object has no attribute 'replace'", heap.get(*id).py_type(heap)),
            )
            .into()),
            _ => Err(ExcType::type_error(format!(
                "argument of type '{}' is not a container or iterable",
                heap.get(*id).py_type(heap)
            ))),
        },
        Value::InternBytes(_) => Err(ExcType::type_error("a bytes-like object is required, not 'str'")),
        _ if function_name == "escape" => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(
                ExcType::AttributeError,
                format!("'{type_name}' object has no attribute 'replace'"),
            )
            .into())
        }
        _ => Err(ExcType::type_error(format!(
            "argument of type '{}' is not a container or iterable",
            value.py_type(heap)
        ))),
    }
}
