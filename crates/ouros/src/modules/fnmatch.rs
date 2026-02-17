//! Minimal implementation of the `fnmatch` module.
//!
//! This currently supports deterministic wildcard matching for `*` and `?`.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, List, Module, Str},
    value::Value,
};

/// `fnmatch` module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum FnmatchFunctions {
    Fnmatch,
    Fnmatchcase,
    Filter,
}

/// Creates the `fnmatch` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Fnmatch);
    module.set_attr_text(
        "fnmatch",
        Value::ModuleFunction(ModuleFunctions::Fnmatch(FnmatchFunctions::Fnmatch)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "fnmatchcase",
        Value::ModuleFunction(ModuleFunctions::Fnmatch(FnmatchFunctions::Fnmatchcase)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "filter",
        Value::ModuleFunction(ModuleFunctions::Fnmatch(FnmatchFunctions::Filter)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `fnmatch` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: FnmatchFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        FnmatchFunctions::Fnmatch | FnmatchFunctions::Fnmatchcase => fnmatch_bool(heap, interns, args)?,
        FnmatchFunctions::Filter => fnmatch_filter(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(result))
}

/// Implements `fnmatch.fnmatch(name, pat)` / `fnmatch.fnmatchcase(name, pat)`.
fn fnmatch_bool(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (name, pat) = parse_two_strings("fnmatch", args, heap, interns)?;
    Ok(Value::Bool(wildcard_match(name.as_bytes(), pat.as_bytes())))
}

/// Implements `fnmatch.filter(names, pat)`.
fn fnmatch_filter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (names_value, pat_value) = args.get_two_args("filter", heap)?;
    let pat = value_to_string(&pat_value, heap, interns)?;

    // Collect matches while only borrowing the input list immutably.
    let matched_names: Vec<String> = if let Value::Ref(id) = &names_value {
        if let HeapData::List(list) = heap.get(*id) {
            list.as_vec()
                .iter()
                .filter_map(|item| {
                    let Ok(name) = value_to_string(item, heap, interns) else {
                        return None;
                    };
                    if wildcard_match(name.as_bytes(), pat.as_bytes()) {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let mut out = Vec::with_capacity(matched_names.len());
    for name in matched_names {
        let sid = heap.allocate(HeapData::Str(Str::from(name)))?;
        out.push(Value::Ref(sid));
    }

    names_value.drop_with_heap(heap);
    pat_value.drop_with_heap(heap);
    let list_id = heap.allocate(HeapData::List(List::new(out)))?;
    Ok(Value::Ref(list_id))
}

/// Parses two required string arguments.
fn parse_two_strings(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String)> {
    let (a, b) = args.get_two_args(function_name, heap)?;
    let sa = value_to_string(&a, heap, interns)?;
    let sb = value_to_string(&b, heap, interns)?;
    a.drop_with_heap(heap);
    b.drop_with_heap(heap);
    Ok((sa, sb))
}

/// Converts a string-like value to Rust `String`.
fn value_to_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error("expected str")),
        },
        _ => Err(ExcType::type_error("expected str")),
    }
}

/// Wildcard matcher supporting `*` and `?`.
fn wildcard_match(text: &[u8], pattern: &[u8]) -> bool {
    let (mut ti, mut pi, mut star_pi, mut star_ti) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            ti += 1;
            pi += 1;
            continue;
        }
        if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = Some(pi);
            pi += 1;
            star_ti = ti;
            continue;
        }
        if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
            continue;
        }
        return false;
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}
