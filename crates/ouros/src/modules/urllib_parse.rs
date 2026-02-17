//! Minimal implementation of `urllib.parse`.
//!
//! Provides commonly used URL parsing, query-string, and percent-encoding helpers.
//! The implementation is intentionally string-only and deterministic, with no network
//! or filesystem interaction.

use std::fmt::Write;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, List, Module, NamedTuple, OurosIter, PyTrait, Str, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// URL schemes that support relative URL resolution in `urljoin`.
const USES_RELATIVE: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "imap", "wais", "file", "https", "shttp", "mms", "prospero", "rtsp", "rtsps",
    "rtspu", "sftp", "svn", "svn+ssh", "ws", "wss",
];

/// URL schemes that keep an explicit netloc (authority) component.
const USES_NETLOC: &[&str] = &[
    "",
    "ftp",
    "http",
    "gopher",
    "nntp",
    "telnet",
    "imap",
    "wais",
    "file",
    "mms",
    "https",
    "shttp",
    "snews",
    "prospero",
    "rtsp",
    "rtsps",
    "rtspu",
    "rsync",
    "svn",
    "svn+ssh",
    "sftp",
    "nfs",
    "git",
    "git+ssh",
    "ws",
    "wss",
    "itms-services",
];

/// URL schemes where `urlparse` splits path params (`;...`) into `params`.
const USES_PARAMS: &[&str] = &[
    "", "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp", "rtsps", "rtspu", "sip", "sips", "mms",
    "sftp", "tel",
];

/// Valid characters for a URL scheme prefix.
const SCHEME_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-.";

/// `urllib.parse` module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum UrllibParseFunctions {
    Quote,
    Unquote,
    QuotePlus,
    UnquotePlus,
    Urlparse,
    Urlunparse,
    Urlsplit,
    Urlunsplit,
    Urlencode,
    ParseQs,
    ParseQsl,
    Urljoin,
}

/// Result of splitting a URL into RFC3986-like components.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SplitUrlParts {
    scheme: Option<String>,
    netloc: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

/// Result of parsing a URL with legacy `;params` splitting.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParseUrlParts {
    scheme: Option<String>,
    netloc: Option<String>,
    path: String,
    params: Option<String>,
    query: Option<String>,
    fragment: Option<String>,
}

/// Creates the `urllib.parse` submodule.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::UrllibParse);
    module.set_attr_text(
        "quote",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Quote)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "unquote",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Unquote)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "quote_plus",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::QuotePlus)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "unquote_plus",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::UnquotePlus)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "urlparse",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Urlparse)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "urlunparse",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Urlunparse)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "urlsplit",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Urlsplit)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "urlunsplit",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Urlunsplit)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "urlencode",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Urlencode)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "parse_qs",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::ParseQs)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "parse_qsl",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::ParseQsl)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "urljoin",
        Value::ModuleFunction(ModuleFunctions::UrllibParse(UrllibParseFunctions::Urljoin)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `urllib.parse` functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: UrllibParseFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        UrllibParseFunctions::Quote => quote(heap, interns, args)?,
        UrllibParseFunctions::Unquote => unquote(heap, interns, args, false)?,
        UrllibParseFunctions::QuotePlus => quote_plus(heap, interns, args)?,
        UrllibParseFunctions::UnquotePlus => unquote(heap, interns, args, true)?,
        UrllibParseFunctions::Urlparse => urlparse(heap, interns, args)?,
        UrllibParseFunctions::Urlunparse => urlunparse(heap, interns, args)?,
        UrllibParseFunctions::Urlsplit => urlsplit(heap, interns, args)?,
        UrllibParseFunctions::Urlunsplit => urlunsplit(heap, interns, args)?,
        UrllibParseFunctions::Urlencode => urlencode(heap, interns, args)?,
        UrllibParseFunctions::ParseQs => parse_qs(heap, interns, args)?,
        UrllibParseFunctions::ParseQsl => parse_qsl(heap, interns, args)?,
        UrllibParseFunctions::Urljoin => urljoin(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(result))
}

/// Implements `urllib.parse.quote(string, ...)`.
fn quote(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let s = parse_required_string("quote", args, "string", heap, interns)?;
    let encoded = percent_encode(&s, "/", false);
    let id = heap.allocate(HeapData::Str(Str::from(encoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `urllib.parse.quote_plus(string, ...)`.
fn quote_plus(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let s = parse_required_string("quote_plus", args, "string", heap, interns)?;
    let encoded = percent_encode(&s, "", true);
    let id = heap.allocate(HeapData::Str(Str::from(encoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `urllib.parse.unquote(string, ...)` and `unquote_plus`.
fn unquote(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    plus_as_space: bool,
) -> RunResult<Value> {
    let s = parse_required_string(
        if plus_as_space { "unquote_plus" } else { "unquote" },
        args,
        "string",
        heap,
        interns,
    )?;
    let decoded = percent_decode(&s, plus_as_space);
    let id = heap.allocate(HeapData::Str(Str::from(decoded)))?;
    Ok(Value::Ref(id))
}

/// Implements `urllib.parse.urlparse(url, scheme='', allow_fragments=True)`.
fn urlparse(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (url, scheme, allow_fragments) = parse_url_split_args("urlparse", args, heap, interns)?;
    let parsed = parse_url_internal(&url, Some(&scheme), allow_fragments)?;
    allocate_parse_result(parsed, heap)
}

/// Implements `urllib.parse.urlsplit(url, scheme='', allow_fragments=True)`.
fn urlsplit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (url, scheme, allow_fragments) = parse_url_split_args("urlsplit", args, heap, interns)?;
    let parsed = split_url_internal(&url, Some(&scheme), allow_fragments)?;
    allocate_split_result(parsed, heap)
}

/// Implements `urllib.parse.urljoin(base, url, allow_fragments=True)`.
fn urljoin(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (base, url, allow_fragments) = parse_url_join_args(args, heap, interns)?;
    let joined = join_urls(&base, &url, allow_fragments)?;
    let id = heap.allocate(HeapData::Str(Str::from(joined)))?;
    Ok(Value::Ref(id))
}

/// Implements `urllib.parse.urlunsplit(components)`.
fn urlunsplit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let components = parse_single_required_value("urlunsplit", "components", args, heap, interns)?;
    let mut parts = unpack_url_components("urlunsplit", components, 5, heap, interns)?;

    let fragment = parts.pop().expect("fixed length");
    let query = parts.pop().expect("fixed length");
    let path = parts.pop().expect("fixed length").unwrap_or_default();
    let netloc = parts.pop().expect("fixed length");
    let scheme = parts.pop().expect("fixed length");

    let netloc = normalize_netloc_for_unsplit(scheme.as_deref(), netloc, &path);
    let url = url_unsplit_internal(
        normalize_optional_nonempty(scheme).as_deref(),
        netloc.as_deref(),
        &path,
        normalize_optional_nonempty(query).as_deref(),
        normalize_optional_nonempty(fragment).as_deref(),
    );
    let id = heap.allocate(HeapData::Str(Str::from(url)))?;
    Ok(Value::Ref(id))
}

/// Implements `urllib.parse.urlunparse(components)`.
fn urlunparse(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let components = parse_single_required_value("urlunparse", "components", args, heap, interns)?;
    let mut parts = unpack_url_components("urlunparse", components, 6, heap, interns)?;

    let fragment = parts.pop().expect("fixed length");
    let query = parts.pop().expect("fixed length");
    let params = parts.pop().expect("fixed length");
    let mut path = parts.pop().expect("fixed length").unwrap_or_default();
    let netloc = parts.pop().expect("fixed length");
    let scheme = parts.pop().expect("fixed length");

    if let Some(params) = params
        && !params.is_empty()
    {
        path.push(';');
        path.push_str(&params);
    }

    let netloc = normalize_netloc_for_unsplit(scheme.as_deref(), netloc, &path);
    let url = url_unsplit_internal(
        normalize_optional_nonempty(scheme).as_deref(),
        netloc.as_deref(),
        &path,
        normalize_optional_nonempty(query).as_deref(),
        normalize_optional_nonempty(fragment).as_deref(),
    );
    let id = heap.allocate(HeapData::Str(Str::from(url)))?;
    Ok(Value::Ref(id))
}

/// Implements `urllib.parse.parse_qsl(qs, keep_blank_values=False, strict_parsing=False)`.
fn parse_qsl(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (qs, keep_blank_values, strict_parsing) = parse_query_parse_args("parse_qsl", args, heap, interns)?;

    let pairs = match qs {
        None => Vec::new(),
        Some(qs) if qs.is_empty() => Vec::new(),
        Some(qs) => parse_qsl_pairs(&qs, keep_blank_values, strict_parsing)?,
    };

    let mut items = Vec::with_capacity(pairs.len());
    for (name, value) in pairs {
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        let value_id = heap.allocate(HeapData::Str(Str::from(value)))?;
        let tuple = allocate_tuple(smallvec::smallvec![Value::Ref(name_id), Value::Ref(value_id)], heap)?;
        items.push(tuple);
    }

    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Implements `urllib.parse.parse_qs(qs, keep_blank_values=False, strict_parsing=False)`.
fn parse_qs(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (qs, keep_blank_values, strict_parsing) = parse_query_parse_args("parse_qs", args, heap, interns)?;

    let pairs = match qs {
        None => Vec::new(),
        Some(qs) if qs.is_empty() => Vec::new(),
        Some(qs) => parse_qsl_pairs(&qs, keep_blank_values, strict_parsing)?,
    };

    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for (name, value) in pairs {
        if let Some((_, values)) = grouped.iter_mut().find(|(existing, _)| existing == &name) {
            values.push(value);
        } else {
            grouped.push((name, vec![value]));
        }
    }

    let mut result = Dict::new();
    for (name, values) in grouped {
        let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;

        let mut list_values = Vec::with_capacity(values.len());
        for value in values {
            let value_id = heap.allocate(HeapData::Str(Str::from(value)))?;
            list_values.push(Value::Ref(value_id));
        }

        let list_id = heap.allocate(HeapData::List(List::new(list_values)))?;
        if let Some(replaced) = result.set(Value::Ref(key_id), Value::Ref(list_id), heap, interns)? {
            replaced.drop_with_heap(heap);
        }
    }

    let dict_id = heap.allocate(HeapData::Dict(result))?;
    Ok(Value::Ref(dict_id))
}

/// Implements `urllib.parse.urlencode(query, doseq=False)`.
fn urlencode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (query, doseq) = parse_urlencode_args(args, heap, interns)?;
    let encoded = urlencode_internal(query, doseq, heap, interns)?;
    let id = heap.allocate(HeapData::Str(Str::from(encoded)))?;
    Ok(Value::Ref(id))
}

/// Parses URL arguments shared by `urlparse` and `urlsplit`.
fn parse_url_split_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String, bool)> {
    let mut values = parse_args_with_keywords(
        function_name,
        &["url", "scheme", "allow_fragments"],
        args,
        heap,
        interns,
    )?;

    let allow_fragments = values.pop().expect("fixed arg count");
    let scheme = values.pop().expect("fixed arg count");
    let url = values.pop().expect("fixed arg count");

    let Some(url) = url else {
        scheme.drop_with_heap(heap);
        allow_fragments.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &["url"],
        ));
    };

    let url = match value_into_string(url, heap, interns) {
        Ok(url) => url,
        Err(err) => {
            scheme.drop_with_heap(heap);
            allow_fragments.drop_with_heap(heap);
            return Err(err);
        }
    };

    let scheme = match scheme {
        Some(scheme) => match value_into_string(scheme, heap, interns) {
            Ok(scheme) => scheme,
            Err(err) => {
                allow_fragments.drop_with_heap(heap);
                return Err(err);
            }
        },
        None => String::new(),
    };

    let allow_fragments = option_value_to_bool(allow_fragments, true, heap, interns);
    Ok((url, scheme, allow_fragments))
}

/// Parses arguments for `urljoin(base, url, allow_fragments=True)`.
fn parse_url_join_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String, bool)> {
    let mut values = parse_args_with_keywords("urljoin", &["base", "url", "allow_fragments"], args, heap, interns)?;

    let allow_fragments = values.pop().expect("fixed arg count");
    let url = values.pop().expect("fixed arg count");
    let base = values.pop().expect("fixed arg count");

    let Some(base) = base else {
        url.drop_with_heap(heap);
        allow_fragments.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            "urljoin",
            &["base", "url"],
        ));
    };
    let Some(url) = url else {
        base.drop_with_heap(heap);
        allow_fragments.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names("urljoin", &["url"]));
    };

    let base = match value_into_string(base, heap, interns) {
        Ok(base) => base,
        Err(err) => {
            url.drop_with_heap(heap);
            allow_fragments.drop_with_heap(heap);
            return Err(err);
        }
    };

    let url = match value_into_string(url, heap, interns) {
        Ok(url) => url,
        Err(err) => {
            allow_fragments.drop_with_heap(heap);
            return Err(err);
        }
    };

    let allow_fragments = option_value_to_bool(allow_fragments, true, heap, interns);
    Ok((base, url, allow_fragments))
}

/// Parses query-string function args for `parse_qs` and `parse_qsl`.
fn parse_query_parse_args(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Option<String>, bool, bool)> {
    let mut values = parse_args_with_keywords(
        function_name,
        &["qs", "keep_blank_values", "strict_parsing"],
        args,
        heap,
        interns,
    )?;

    let strict_parsing = values.pop().expect("fixed arg count");
    let keep_blank_values = values.pop().expect("fixed arg count");
    let qs = values.pop().expect("fixed arg count");

    let Some(qs) = qs else {
        keep_blank_values.drop_with_heap(heap);
        strict_parsing.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &["qs"],
        ));
    };

    let qs = match value_into_optional_string(qs, heap, interns) {
        Ok(qs) => qs,
        Err(err) => {
            keep_blank_values.drop_with_heap(heap);
            strict_parsing.drop_with_heap(heap);
            return Err(err);
        }
    };

    let keep_blank_values = option_value_to_bool(keep_blank_values, false, heap, interns);
    let strict_parsing = option_value_to_bool(strict_parsing, false, heap, interns);
    Ok((qs, keep_blank_values, strict_parsing))
}

/// Parses args for `urlencode(query, doseq=False)`.
fn parse_urlencode_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, bool)> {
    let mut values = parse_args_with_keywords("urlencode", &["query", "doseq"], args, heap, interns)?;

    let doseq = values.pop().expect("fixed arg count");
    let query = values.pop().expect("fixed arg count");

    let Some(query) = query else {
        doseq.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            "urlencode",
            &["query"],
        ));
    };

    let doseq = option_value_to_bool(doseq, false, heap, interns);
    Ok((query, doseq))
}

/// Parses a single required value argument (by name).
fn parse_single_required_value(
    function_name: &str,
    arg_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut values = parse_args_with_keywords(function_name, &[arg_name], args, heap, interns)?;
    match values.pop().expect("single arg") {
        Some(value) => Ok(value),
        None => Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &[arg_name],
        )),
    }
}

/// Parses positional and keyword arguments into slots by parameter name.
fn parse_args_with_keywords(
    function_name: &str,
    param_names: &[&str],
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Option<Value>>> {
    let (positional, kwargs) = args.into_parts();
    let positional_count = positional.len();

    if positional_count > param_names.len() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(
            function_name,
            param_names.len(),
            positional_count,
            0,
        ));
    }

    let mut slots: Vec<Option<Value>> = (0..param_names.len()).map(|_| None).collect();
    for (index, value) in positional.into_iter().enumerate() {
        slots[index] = Some(value);
    }

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            slots.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        let Some(index) = param_names.iter().position(|name| *name == key_name) else {
            value.drop_with_heap(heap);
            slots.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
        };

        if slots[index].is_some() {
            value.drop_with_heap(heap);
            slots.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg(function_name, param_names[index]));
        }
        slots[index] = Some(value);
    }

    Ok(slots)
}

/// Converts a Value into a strict Python string, dropping the input value.
fn value_into_string(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let out = match &value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "expected str, got '{}'",
                heap.get(*id).py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "expected str, got '{}'",
            value.py_type(heap)
        ))),
    };
    value.drop_with_heap(heap);
    out
}

/// Converts a Value into `Option<String>`, treating `None` as missing.
fn value_into_optional_string(
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<String>> {
    if matches!(value, Value::None) {
        value.drop_with_heap(heap);
        return Ok(None);
    }
    value_into_string(value, heap, interns).map(Some)
}

/// Converts an optional Value into a bool using Python truthiness.
fn option_value_to_bool(
    value: Option<Value>,
    default: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    match value {
        Some(value) => {
            let out = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
            out
        }
        None => default,
    }
}

/// Allocates a `ParseResult(...)` namedtuple.
fn allocate_parse_result(parts: ParseUrlParts, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let scheme = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.scheme.unwrap_or_default())))?);
    let netloc = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.netloc.unwrap_or_default())))?);
    let path = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.path)))?);
    let params = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.params.unwrap_or_default())))?);
    let query = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.query.unwrap_or_default())))?);
    let fragment = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.fragment.unwrap_or_default())))?);

    let named = NamedTuple::new(
        "ParseResult".to_owned(),
        parse_result_field_names(),
        vec![scheme, netloc, path, params, query, fragment],
    );
    let id = heap.allocate(HeapData::NamedTuple(named))?;
    Ok(Value::Ref(id))
}

/// Allocates a `SplitResult(...)` namedtuple.
fn allocate_split_result(parts: SplitUrlParts, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let scheme = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.scheme.unwrap_or_default())))?);
    let netloc = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.netloc.unwrap_or_default())))?);
    let path = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.path)))?);
    let query = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.query.unwrap_or_default())))?);
    let fragment = Value::Ref(heap.allocate(HeapData::Str(Str::from(parts.fragment.unwrap_or_default())))?);

    let named = NamedTuple::new(
        "SplitResult".to_owned(),
        split_result_field_names(),
        vec![scheme, netloc, path, query, fragment],
    );
    let id = heap.allocate(HeapData::NamedTuple(named))?;
    Ok(Value::Ref(id))
}

/// Returns field names for `ParseResult` namedtuples.
fn parse_result_field_names() -> Vec<EitherStr> {
    vec![
        "scheme".to_owned().into(),
        "netloc".to_owned().into(),
        "path".to_owned().into(),
        "params".to_owned().into(),
        "query".to_owned().into(),
        "fragment".to_owned().into(),
    ]
}

/// Returns field names for `SplitResult` namedtuples.
fn split_result_field_names() -> Vec<EitherStr> {
    vec![
        "scheme".to_owned().into(),
        "netloc".to_owned().into(),
        "path".to_owned().into(),
        "query".to_owned().into(),
        "fragment".to_owned().into(),
    ]
}

/// Core URL splitter that mirrors CPython `_urlsplit` semantics for strings.
fn split_url_internal(url: &str, scheme: Option<&str>, allow_fragments: bool) -> RunResult<SplitUrlParts> {
    let mut url = sanitize_url(url, true);
    let mut scheme = scheme.map(|scheme| sanitize_url(scheme, false));

    let mut netloc = None;
    let mut query = None;
    let mut fragment = None;

    if let Some(i) = url.find(':')
        && i > 0
    {
        let prefix = &url[..i];
        let mut chars = prefix.chars();
        if chars
            .next()
            .is_some_and(|first| first.is_ascii() && first.is_ascii_alphabetic())
            && prefix.chars().all(|ch| SCHEME_CHARS.contains(ch))
        {
            scheme = Some(prefix.to_ascii_lowercase());
            url = url[i + 1..].to_owned();
        }
    }

    if url.starts_with("//") {
        let (split_netloc, rest) = split_netloc(&url, 2);
        if has_invalid_ipv6_brackets(split_netloc) {
            return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid IPv6 URL").into());
        }
        netloc = Some(split_netloc.to_owned());
        url = rest.to_owned();
    }

    if allow_fragments && let Some(index) = url.find('#') {
        fragment = Some(url[index + 1..].to_owned());
        url = url[..index].to_owned();
    }

    if let Some(index) = url.find('?') {
        query = Some(url[index + 1..].to_owned());
        url = url[..index].to_owned();
    }

    Ok(SplitUrlParts {
        scheme,
        netloc,
        path: url,
        query,
        fragment,
    })
}

/// Core URL parser that also extracts `params` for legacy schemes.
fn parse_url_internal(url: &str, scheme: Option<&str>, allow_fragments: bool) -> RunResult<ParseUrlParts> {
    let mut split = split_url_internal(url, scheme, allow_fragments)?;
    let params = if uses_params(split.scheme.as_deref().unwrap_or("")) && split.path.contains(';') {
        let (path, params) = split_params(&split.path, true);
        split.path = path;
        params
    } else {
        None
    };

    Ok(ParseUrlParts {
        scheme: split.scheme,
        netloc: split.netloc,
        path: split.path,
        params,
        query: split.query,
        fragment: split.fragment,
    })
}

/// Joins `base` and `url` following `urllib.parse.urljoin` behavior.
fn join_urls(base: &str, url: &str, allow_fragments: bool) -> RunResult<String> {
    if base.is_empty() {
        return Ok(url.to_owned());
    }
    if url.is_empty() {
        return Ok(base.to_owned());
    }

    let base_split = split_url_internal(base, None, allow_fragments)?;
    let url_split = split_url_internal(url, None, allow_fragments)?;

    let bscheme = base_split.scheme;
    let bnetloc = base_split.netloc;
    let bpath = base_split.path;
    let bquery = base_split.query;
    let bfragment = base_split.fragment;

    let mut scheme = url_split.scheme;
    let mut netloc = url_split.netloc;
    let mut path = url_split.path;
    let mut query = url_split.query;
    let mut fragment = url_split.fragment;

    if scheme.is_none() {
        scheme = bscheme.clone();
    }

    if scheme != bscheme || scheme.as_deref().is_some_and(|s| !s.is_empty() && !uses_relative(s)) {
        return Ok(url.to_owned());
    }

    let scheme_name = scheme.as_deref().unwrap_or("");
    if scheme_name.is_empty() || uses_netloc(scheme_name) {
        if netloc.is_some() {
            return Ok(url_unsplit_internal(
                scheme.as_deref(),
                netloc.as_deref(),
                &path,
                query.as_deref(),
                fragment.as_deref(),
            ));
        }
        netloc = bnetloc;
    }

    if path.is_empty() {
        path = bpath;
        if query.is_none() {
            query = bquery;
            if fragment.is_none() {
                fragment = bfragment;
            }
        }

        return Ok(url_unsplit_internal(
            scheme.as_deref(),
            netloc.as_deref(),
            &path,
            query.as_deref(),
            fragment.as_deref(),
        ));
    }

    let mut base_parts = bpath.split('/').map(ToOwned::to_owned).collect::<Vec<_>>();
    if !base_parts.last().is_some_and(String::is_empty) {
        base_parts.pop();
    }

    let segments = if path.starts_with('/') {
        path.split('/').map(ToOwned::to_owned).collect::<Vec<_>>()
    } else {
        let mut segments = base_parts;
        segments.extend(path.split('/').map(ToOwned::to_owned));

        if segments.len() > 2 {
            let end = segments.len() - 1;
            let mut filtered = Vec::with_capacity(segments.len());
            filtered.push(segments[0].clone());
            for segment in &segments[1..end] {
                if !segment.is_empty() {
                    filtered.push(segment.clone());
                }
            }
            filtered.push(segments[end].clone());
            segments = filtered;
        }
        segments
    };

    let mut resolved = Vec::with_capacity(segments.len());
    for segment in &segments {
        if segment == ".." {
            let _ = resolved.pop();
        } else if segment != "." {
            resolved.push(segment.clone());
        }
    }

    if segments.last().is_some_and(|last| last == "." || last == "..") {
        resolved.push(String::new());
    }

    let resolved_path = {
        let joined = resolved.join("/");
        if joined.is_empty() { "/".to_owned() } else { joined }
    };

    Ok(url_unsplit_internal(
        scheme.as_deref(),
        netloc.as_deref(),
        &resolved_path,
        query.as_deref(),
        fragment.as_deref(),
    ))
}

/// Unpacks URL components for `urlunparse`/`urlunsplit` and coerces items to `Option<String>`.
fn unpack_url_components(
    _function_name: &str,
    components: Value,
    expected_len: usize,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Option<String>>> {
    let mut iter = OurosIter::new(components, heap, interns)?;
    let mut output = Vec::with_capacity(expected_len);

    loop {
        let item = match iter.for_next(heap, interns) {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        if output.len() == expected_len {
            item.drop_with_heap(heap);
            let mut actual = expected_len + 1;
            loop {
                match iter.for_next(heap, interns) {
                    Ok(Some(extra)) => {
                        actual += 1;
                        extra.drop_with_heap(heap);
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            iter.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("too many values to unpack (expected {expected_len}, got {actual})"),
            )
            .into());
        }

        match value_into_optional_string(item, heap, interns) {
            Ok(value) => output.push(value),
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }

    iter.drop_with_heap(heap);

    if output.len() < expected_len {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!(
                "not enough values to unpack (expected {expected_len}, got {})",
                output.len()
            ),
        )
        .into());
    }

    Ok(output)
}

/// Parses query-string pairs with `parse_qsl` behavior for `&`-separated fields.
fn parse_qsl_pairs(qs: &str, keep_blank_values: bool, strict_parsing: bool) -> RunResult<Vec<(String, String)>> {
    if qs.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for name_value in qs.split('&') {
        if !name_value.is_empty() || strict_parsing {
            let (name, has_eq, value) = partition_once(name_value, '=');
            if !has_eq && strict_parsing {
                return Err(
                    SimpleException::new_msg(ExcType::ValueError, format!("bad query field: '{name_value}'")).into(),
                );
            }

            if !value.is_empty() || keep_blank_values {
                out.push((percent_decode(name, true), percent_decode(value, true)));
            }
        }
    }
    Ok(out)
}

/// Encodes a query object into a URL query string.
fn urlencode_internal(
    query: Value,
    doseq: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    if let Some(mapping_items) = mapping_items_for_urlencode(&query, heap) {
        query.drop_with_heap(heap);

        let mut encoded_parts = Vec::new();
        let mut items = mapping_items.into_iter();
        while let Some((key, value)) = items.next() {
            if let Err(err) = append_urlencoded_pair(&mut encoded_parts, key, value, doseq, heap, interns) {
                items.drop_with_heap(heap);
                return Err(err);
            }
        }

        return Ok(encoded_parts.join("&"));
    }

    let query_len = if let Some(query_len) = query.py_len(heap, interns) {
        query_len
    } else {
        query.drop_with_heap(heap);
        return Err(ExcType::type_error("not a valid non-string sequence or mapping object"));
    };

    if query_len > 0 {
        let first = if let Ok(first) = sequence_first_item(&query, heap, interns) {
            first
        } else {
            query.drop_with_heap(heap);
            return Err(ExcType::type_error("not a valid non-string sequence or mapping object"));
        };

        if first.py_type(heap) != Type::Tuple {
            first.drop_with_heap(heap);
            query.drop_with_heap(heap);
            return Err(ExcType::type_error("not a valid non-string sequence or mapping object"));
        }
        first.drop_with_heap(heap);
    }

    let mut iter = match OurosIter::new(query, heap, interns) {
        Ok(iter) => iter,
        Err(_) => {
            return Err(ExcType::type_error("not a valid non-string sequence or mapping object"));
        }
    };

    let mut encoded_parts = Vec::new();
    loop {
        let item = match iter.for_next(heap, interns) {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        let (key, value) = match unpack_urlencode_item_pair(item, heap, interns) {
            Ok(pair) => pair,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        };

        if let Err(err) = append_urlencoded_pair(&mut encoded_parts, key, value, doseq, heap, interns) {
            iter.drop_with_heap(heap);
            return Err(err);
        }
    }

    iter.drop_with_heap(heap);
    Ok(encoded_parts.join("&"))
}

/// Returns shallow mapping items for supported mapping-like types.
fn mapping_items_for_urlencode(value: &Value, heap: &mut Heap<impl ResourceTracker>) -> Option<Vec<(Value, Value)>> {
    let Value::Ref(id) = value else {
        return None;
    };

    heap.with_entry_mut(*id, |heap_inner, data| match data {
        HeapData::Dict(dict) => Some(dict.items(heap_inner)),
        HeapData::DefaultDict(default_dict) => Some(default_dict.dict().items(heap_inner)),
        HeapData::Counter(counter) => Some(counter.dict().items(heap_inner)),
        HeapData::OrderedDict(ordered_dict) => Some(ordered_dict.dict().items(heap_inner)),
        HeapData::ChainMap(chain_map) => Some(chain_map.flat_items(heap_inner)),
        _ => None,
    })
}

/// Gets sequence index 0 from a value using Python indexing semantics.
fn sequence_first_item(sequence: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let mut sequence = sequence.clone_with_heap(heap);
    let result = sequence.py_getitem(&Value::Int(0), heap, interns);
    sequence.drop_with_heap(heap);
    result
}

/// Unpacks a single item from urlencode's outer sequence into a `(key, value)` pair.
fn unpack_urlencode_item_pair(
    item: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Value)> {
    let mut pair_iter = OurosIter::new(item, heap, interns)?;

    let key = match pair_iter.for_next(heap, interns) {
        Ok(Some(key)) => key,
        Ok(None) => {
            pair_iter.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "not enough values to unpack (expected 2, got 0)",
            )
            .into());
        }
        Err(err) => {
            pair_iter.drop_with_heap(heap);
            return Err(err);
        }
    };

    let value = match pair_iter.for_next(heap, interns) {
        Ok(Some(value)) => value,
        Ok(None) => {
            key.drop_with_heap(heap);
            pair_iter.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "not enough values to unpack (expected 2, got 1)",
            )
            .into());
        }
        Err(err) => {
            key.drop_with_heap(heap);
            pair_iter.drop_with_heap(heap);
            return Err(err);
        }
    };

    if let Ok(Some(extra)) = pair_iter.for_next(heap, interns) {
        extra.drop_with_heap(heap);
        let mut count = 3;
        loop {
            match pair_iter.for_next(heap, interns) {
                Ok(Some(remaining)) => {
                    count += 1;
                    remaining.drop_with_heap(heap);
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        pair_iter.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("too many values to unpack (expected 2, got {count})"),
        )
        .into());
    }

    pair_iter.drop_with_heap(heap);
    Ok((key, value))
}

/// Appends one key/value pair to `urlencode` output, handling `doseq` expansion.
fn append_urlencoded_pair(
    encoded_parts: &mut Vec<String>,
    key: Value,
    value: Value,
    doseq: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let encoded_key = quote_plus_value(&key, heap, interns);

    if !doseq {
        let encoded_value = quote_plus_value(&value, heap, interns);
        encoded_parts.push(format!("{encoded_key}={encoded_value}"));
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Ok(());
    }

    if is_string_or_bytes(&value, heap) {
        let encoded_value = quote_plus_value(&value, heap, interns);
        encoded_parts.push(format!("{encoded_key}={encoded_value}"));
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Ok(());
    }

    let Some(_) = value.py_len(heap, interns) else {
        let encoded_value = quote_plus_value(&value, heap, interns);
        encoded_parts.push(format!("{encoded_key}={encoded_value}"));
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Ok(());
    };

    let iterable = value.clone_with_heap(heap);
    let mut iter = match OurosIter::new(iterable, heap, interns) {
        Ok(iter) => iter,
        Err(err) => {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(err);
        }
    };

    loop {
        let element = match iter.for_next(heap, interns) {
            Ok(Some(element)) => element,
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(err);
            }
        };

        let encoded_element = quote_plus_value(&element, heap, interns);
        element.drop_with_heap(heap);
        encoded_parts.push(format!("{encoded_key}={encoded_element}"));
    }

    iter.drop_with_heap(heap);
    key.drop_with_heap(heap);
    value.drop_with_heap(heap);
    Ok(())
}

/// Returns true for `str` and `bytes` values.
fn is_string_or_bytes(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::InternString(_) | Value::InternBytes(_) => true,
        Value::Ref(id) => matches!(heap.get(*id), HeapData::Str(_) | HeapData::Bytes(_)),
        _ => false,
    }
}

/// `quote_plus`-encodes a scalar query key/value.
fn quote_plus_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> String {
    match value {
        Value::InternString(id) => percent_encode(interns.get_str(*id), "", true),
        Value::InternBytes(id) => percent_encode_bytes(interns.get_bytes(*id), "", true),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => percent_encode(s.as_str(), "", true),
            HeapData::Bytes(b) => percent_encode_bytes(b.as_slice(), "", true),
            _ => {
                let text = value.py_str(heap, interns);
                percent_encode(&text, "", true)
            }
        },
        _ => {
            let text = value.py_str(heap, interns);
            percent_encode(&text, "", true)
        }
    }
}

/// Splits the netloc at the first path/query/fragment delimiter.
fn split_netloc(url: &str, start: usize) -> (&str, &str) {
    let mut delimiter = url.len();

    for ch in ['/', '?', '#'] {
        if let Some(pos) = url[start..].find(ch) {
            delimiter = delimiter.min(start + pos);
        }
    }

    (&url[start..delimiter], &url[delimiter..])
}

/// Splits `path;params` from the right-most path segment.
fn split_params(url: &str, allow_none: bool) -> (String, Option<String>) {
    let index = if url.contains('/') {
        let start = url.rfind('/').expect("contains('/') ensured");
        url[start..].find(';').map(|idx| start + idx)
    } else {
        url.find(';')
    };

    let Some(index) = index else {
        return (url.to_owned(), if allow_none { None } else { Some(String::new()) });
    };

    (url[..index].to_owned(), Some(url[index + 1..].to_owned()))
}

/// Rebuilds a URL from split components.
fn url_unsplit_internal(
    scheme: Option<&str>,
    netloc: Option<&str>,
    path: &str,
    query: Option<&str>,
    fragment: Option<&str>,
) -> String {
    let mut url = path.to_owned();

    if let Some(netloc) = netloc {
        if !url.is_empty() && !url.starts_with('/') {
            url.insert(0, '/');
        }
        url = format!("//{netloc}{url}");
    } else if url.starts_with("//") {
        url = format!("//{url}");
    }

    if let Some(scheme) = scheme
        && !scheme.is_empty()
    {
        url = format!("{scheme}:{url}");
    }

    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }

    url
}

/// Normalizes netloc for `urlunsplit`/`urlunparse` behavior.
fn normalize_netloc_for_unsplit(scheme: Option<&str>, netloc: Option<String>, path: &str) -> Option<String> {
    match netloc {
        Some(netloc) if !netloc.is_empty() => Some(netloc),
        _ => {
            if scheme.is_some_and(|scheme| !scheme.is_empty() && uses_netloc(scheme))
                && (path.is_empty() || path.starts_with('/'))
            {
                Some(String::new())
            } else {
                None
            }
        }
    }
}

/// Converts empty strings to `None` for optional URL components.
fn normalize_optional_nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|value| if value.is_empty() { None } else { Some(value) })
}

/// Sanitizes URL and scheme inputs like CPython's string path.
fn sanitize_url(text: &str, lstrip_only: bool) -> String {
    let mut out = if lstrip_only {
        text.trim_start_matches(is_c0_control_or_space).to_owned()
    } else {
        text.trim_matches(is_c0_control_or_space).to_owned()
    };

    for byte in ['\t', '\n', '\r'] {
        out = out.replace(byte, "");
    }
    out
}

/// Returns true for WHATWG C0 controls (<= 0x20) and space.
fn is_c0_control_or_space(ch: char) -> bool {
    ch <= '\u{20}'
}

/// Returns true when netloc has mismatched IPv6 brackets.
fn has_invalid_ipv6_brackets(netloc: &str) -> bool {
    (netloc.contains('[') && !netloc.contains(']')) || (netloc.contains(']') && !netloc.contains('['))
}

/// Returns true if a scheme supports relative URL joins.
fn uses_relative(scheme: &str) -> bool {
    USES_RELATIVE.contains(&scheme)
}

/// Returns true if a scheme uses an explicit netloc component.
fn uses_netloc(scheme: &str) -> bool {
    USES_NETLOC.contains(&scheme)
}

/// Returns true if a scheme splits path params in `urlparse`.
fn uses_params(scheme: &str) -> bool {
    USES_PARAMS.contains(&scheme)
}

/// Partitions once on delimiter; returns `(head, found_delimiter, tail)`.
fn partition_once(text: &str, delimiter: char) -> (&str, bool, &str) {
    match text.split_once(delimiter) {
        Some((head, tail)) => (head, true, tail),
        None => (text, false, ""),
    }
}

/// Parses one required string argument by positional or keyword name.
fn parse_required_string(
    function_name: &str,
    args: ArgValues,
    required_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let mut value = positional.next();
    if positional.next().is_some() {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(function_name, 1, 2, 0));
    }

    for (key, kw_value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            kw_value.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        if key_name == required_name {
            if value.is_some() {
                kw_value.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, required_name));
            }
            value = Some(kw_value);
            continue;
        }
        kw_value.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
    }

    let Some(value) = value else {
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &[required_name],
        ));
    };

    let out = match &value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "expected str, got '{}'",
                heap.get(*id).py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "expected str, got '{}'",
            value.py_type(heap)
        ))),
    };
    value.drop_with_heap(heap);
    out
}

/// Percent-encodes text using UTF-8 bytes.
fn percent_encode(text: &str, safe: &str, plus_for_space: bool) -> String {
    percent_encode_bytes(text.as_bytes(), safe, plus_for_space)
}

/// Percent-encodes arbitrary bytes.
fn percent_encode_bytes(bytes: &[u8], safe: &str, plus_for_space: bool) -> String {
    let safe_bytes = safe.as_bytes();
    let mut out = String::new();

    for &b in bytes {
        let is_unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if is_unreserved || safe_bytes.contains(&b) {
            out.push(char::from(b));
        } else if plus_for_space && b == b' ' {
            out.push('+');
        } else {
            out.push('%');
            write!(&mut out, "{b:02X}").expect("writing to String should not fail");
        }
    }

    out
}

/// Percent-decodes `%HH` bytes and optionally treats `+` as space.
fn percent_decode(text: &str, plus_as_space: bool) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut i = 0;
    let src = text.as_bytes();
    while i < src.len() {
        let b = src[i];
        if plus_as_space && b == b'+' {
            bytes.push(b' ');
            i += 1;
            continue;
        }
        if b == b'%' && i + 2 < src.len() {
            let hi = src[i + 1];
            let lo = src[i + 2];
            if let (Some(hi), Some(lo)) = (hex_nibble(hi), hex_nibble(lo)) {
                bytes.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        bytes.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Converts an ASCII hex nibble to value.
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
