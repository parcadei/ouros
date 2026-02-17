//! Implementation of the `pprint` module.
//!
//! Provides pretty-printing functionality for Python data structures:
//! - `pprint(object, stream=None, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False)`:
//!   Pretty-print to a stream (default sys.stdout)
//! - `pformat(object, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False)`:
//!   Format as a string
//! - `pp(object, *args, sort_dicts=False, **kwargs)`: Pretty-print like pprint but with different defaults
//! - `isreadable(object)`: Check if representation is readable by eval()
//! - `isrecursive(object)`: Check if object has recursive references
//! - `saferepr(object)`: Safe repr that handles recursive data structures

use std::collections::HashSet;

use crate::{
    args::ArgValues,
    builtins::{Builtins, BuiltinsFunctions},
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, ClassObject, Dict, PyTrait, Str, Type, compute_c3_mro},
    value::{EitherStr, Value},
};

/// pprint module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum PprintFunctions {
    Pprint,
    Pformat,
    Pp,
    Isreadable,
    Isrecursive,
    Saferepr,
    PrettyPrinter,
}

/// Default width for pretty-printing.
const DEFAULT_WIDTH: usize = 80;

/// Default indent for pretty-printing.
const DEFAULT_INDENT: usize = 1;

/// Creates the `pprint` module and allocates it on the heap.
///
/// Sets up all pprint functions.
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

    let mut module = Module::new(StaticStrings::Pprint);

    // pprint.pprint - pretty-print to stream
    module.set_attr(
        StaticStrings::Pprint,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::Pprint)),
        heap,
        interns,
    );

    // pprint.pformat - format as string
    module.set_attr(
        StaticStrings::Pformat,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::Pformat)),
        heap,
        interns,
    );

    // pprint.pp - pretty-print with different defaults
    module.set_attr(
        StaticStrings::Pp,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::Pp)),
        heap,
        interns,
    );

    // pprint.isreadable - check if representation is readable
    module.set_attr(
        StaticStrings::Isreadable,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::Isreadable)),
        heap,
        interns,
    );

    // pprint.isrecursive - check if object is recursive
    module.set_attr(
        StaticStrings::Isrecursive,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::Isrecursive)),
        heap,
        interns,
    );

    // pprint.saferepr - safe repr with recursion protection
    module.set_attr(
        StaticStrings::Saferepr,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::Saferepr)),
        heap,
        interns,
    );

    // pprint.PrettyPrinter - class constructor (stub for now)
    module.set_attr(
        StaticStrings::PrettyPrinter,
        Value::ModuleFunction(ModuleFunctions::Pprint(PprintFunctions::PrettyPrinter)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a pprint module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: PprintFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        PprintFunctions::Pprint => pprint_func(heap, interns, args),
        PprintFunctions::Pformat => pformat(heap, interns, args),
        PprintFunctions::Pp => pp_func(heap, interns, args),
        PprintFunctions::Isreadable => isreadable(heap, interns, args),
        PprintFunctions::Isrecursive => isrecursive(heap, interns, args),
        PprintFunctions::Saferepr => saferepr(heap, interns, args),
        PprintFunctions::PrettyPrinter => pretty_printer(heap, interns, args),
    }
}

/// Parameters for pretty-printing.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(crate) struct PprintParams {
    pub(crate) indent: usize,
    pub(crate) width: usize,
    pub(crate) depth: Option<usize>,
    pub(crate) compact: bool,
    pub(crate) sort_dicts: bool,
    pub(crate) underscore_numbers: bool,
}

impl Default for PprintParams {
    fn default() -> Self {
        Self {
            indent: DEFAULT_INDENT,
            width: DEFAULT_WIDTH,
            depth: None,
            compact: false,
            sort_dicts: true,
            underscore_numbers: false,
        }
    }
}

/// Extract pprint parameters from keyword arguments.
fn extract_pprint_params(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    kwargs: impl Iterator<Item = (Value, Value)>,
    defaults: PprintParams,
) -> RunResult<PprintParams> {
    let mut params = defaults;

    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(keyword_name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = keyword_name.as_str(interns);

        match key_name {
            "indent" => {
                let val = value.as_int(heap)?;
                value.drop_with_heap(heap);
                if val < 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "indent must be >= 0").into());
                }
                params.indent = val as usize;
            }
            "width" => {
                let val = value.as_int(heap)?;
                value.drop_with_heap(heap);
                if val < 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "width must be >= 0").into());
                }
                params.width = val as usize;
            }
            "depth" => {
                if matches!(value, Value::None) {
                    params.depth = None;
                    value.drop_with_heap(heap);
                } else {
                    let val = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    if val < 0 {
                        return Err(SimpleException::new_msg(ExcType::ValueError, "depth must be >= 0").into());
                    }
                    params.depth = Some(val as usize);
                }
            }
            "compact" => {
                params.compact = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "sort_dicts" => {
                params.sort_dicts = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "underscore_numbers" => {
                params.underscore_numbers = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("pprint", key_name));
            }
        }
    }

    Ok(params)
}

/// Implementation of `pprint.pprint(object, stream=None, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False)`.
///
/// Pretty-print a Python object to a stream [default is sys.stdout].
fn pprint_func(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos_args, kwargs) = args.into_parts();

    // Get object (required)
    let Some(obj) = pos_args.next() else {
        for v in pos_args {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "pprint.pprint() missing required argument: 'object'".to_string(),
        ));
    };
    defer_drop!(obj, heap);

    // Get stream (optional, default None)
    let stream = pos_args.next();

    // Check for extra positional args
    if let Some(extra) = pos_args.next() {
        let extra_count = pos_args.len() + 2;
        extra.drop_with_heap(heap);
        stream.drop_with_heap(heap);
        for v in pos_args {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("pprint.pprint", 2, extra_count));
    }

    // Extract parameters from kwargs
    let params = extract_pprint_params(heap, interns, kwargs.into_iter(), PprintParams::default())?;

    // Format the object (defer_drop! will handle cleanup)
    let formatted = format_object(heap, interns, obj, &params)?;
    stream.drop_with_heap(heap);

    // TODO: Honor `stream` when provided.
    build_print_call_result(heap, formatted)
}

/// Implementation of `pprint.pformat(object, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False)`.
///
/// Format a Python object into a pretty-printed representation.
fn pformat(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos_args, kwargs) = args.into_parts();

    // Get object (required)
    let Some(obj) = pos_args.next() else {
        for v in pos_args {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "pprint.pformat() missing required argument: 'object'".to_string(),
        ));
    };
    defer_drop!(obj, heap);

    // Check for extra positional args
    if let Some(extra) = pos_args.next() {
        let extra_count = pos_args.len() + 1;
        extra.drop_with_heap(heap);
        for v in pos_args {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("pformat", 1, extra_count));
    }

    // Extract parameters from kwargs
    let params = extract_pprint_params(heap, interns, kwargs.into_iter(), PprintParams::default())?;

    // Format the object
    let formatted = format_object(heap, interns, obj, &params)?;

    // Allocate result string
    let str_obj = Str::from(formatted);
    let id = heap.allocate(HeapData::Str(str_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `pprint.pp(object, *args, sort_dicts=False, **kwargs)`.
///
/// Pretty-print a Python object like pprint() but with sort_dicts=False default.
fn pp_func(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos_args, kwargs) = args.into_parts();

    // Get object (required)
    let Some(obj) = pos_args.next() else {
        for v in pos_args {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "pprint.pp() missing required argument: 'object'".to_string(),
        ));
    };
    defer_drop!(obj, heap);

    // Get stream (optional, default None) - pp allows positional stream
    let stream = pos_args.next();

    // Check for extra positional args
    if let Some(extra) = pos_args.next() {
        let extra_count = pos_args.len() + 2;
        extra.drop_with_heap(heap);
        stream.drop_with_heap(heap);
        for v in pos_args {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("pp", 2, extra_count));
    }

    // Extract parameters from kwargs, with sort_dicts=False default
    let defaults = PprintParams {
        sort_dicts: false,
        ..PprintParams::default()
    };
    let params = extract_pprint_params(heap, interns, kwargs.into_iter(), defaults)?;

    // Format the object
    let formatted = format_object(heap, interns, obj, &params)?;
    stream.drop_with_heap(heap);

    // TODO: Honor `stream` when provided.
    build_print_call_result(heap, formatted)
}

/// Implementation of `pprint.isreadable(object)`.
///
/// Determine if saferepr(object) is readable by eval().
fn isreadable(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("pprint.isreadable", heap)?;
    defer_drop!(obj, heap);

    let readable = object_is_readable(heap, obj)?;
    Ok(AttrCallResult::Value(Value::Bool(readable)))
}

/// Implementation of `pprint.isrecursive(object)`.
///
/// Determine if object requires a recursive representation.
fn isrecursive(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("pprint.isrecursive", heap)?;
    defer_drop!(obj, heap);

    let is_recursive = object_is_recursive(heap, obj)?;

    Ok(AttrCallResult::Value(Value::Bool(is_recursive)))
}

/// Returns whether an object graph contains recursive references.
pub(crate) fn object_is_recursive(heap: &mut Heap<impl ResourceTracker>, obj: &Value) -> RunResult<bool> {
    let mut seen = HashSet::new();
    check_recursive(heap, obj, &mut seen)
}

/// Returns whether an object's repr is evaluable for basic container/value cases.
pub(crate) fn object_is_readable(heap: &mut Heap<impl ResourceTracker>, obj: &Value) -> RunResult<bool> {
    let mut seen = HashSet::new();
    object_is_readable_impl(heap, obj, &mut seen)
}

/// Recursive worker for `object_is_readable`.
fn object_is_readable_impl(
    heap: &mut Heap<impl ResourceTracker>,
    obj: &Value,
    seen: &mut HashSet<HeapId>,
) -> RunResult<bool> {
    match obj {
        Value::Undefined
        | Value::NotImplemented
        | Value::Builtin(_)
        | Value::ModuleFunction(_)
        | Value::DefFunction(_)
        | Value::ExtFunction(_)
        | Value::Marker(_)
        | Value::Property(_)
        | Value::ExternalFuture(_) => Ok(false),
        Value::Ref(heap_id) => {
            if !seen.insert(*heap_id) {
                return Ok(false);
            }

            let readable = match heap.get(*heap_id) {
                HeapData::Str(_) | HeapData::Bytes(_) | HeapData::LongInt(_) => true,
                HeapData::List(list) => {
                    let items: Vec<Value> = list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
                    let mut all = true;
                    for item in &items {
                        if !object_is_readable_impl(heap, item, seen)? {
                            all = false;
                            break;
                        }
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    all
                }
                HeapData::Tuple(tuple) => {
                    let items: Vec<Value> = tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
                    let mut all = true;
                    for item in &items {
                        if !object_is_readable_impl(heap, item, seen)? {
                            all = false;
                            break;
                        }
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    all
                }
                HeapData::Dict(_) => {
                    let entries: Vec<(Value, Value)> = heap.with_entry_mut(*heap_id, |heap, data| match data {
                        HeapData::Dict(dict) => {
                            if dict.is_empty() {
                                Vec::new()
                            } else {
                                dict.items(heap)
                            }
                        }
                        _ => unreachable!(),
                    });

                    let mut all = true;
                    for (k, v) in &entries {
                        if !object_is_readable_impl(heap, k, seen)? || !object_is_readable_impl(heap, v, seen)? {
                            all = false;
                            break;
                        }
                    }
                    for (k, v) in entries {
                        k.drop_with_heap(heap);
                        v.drop_with_heap(heap);
                    }
                    all
                }
                HeapData::Set(set) => {
                    let items: Vec<Value> = set.storage().iter().map(|v| v.clone_with_heap(heap)).collect();
                    let mut all = true;
                    for item in &items {
                        if !object_is_readable_impl(heap, item, seen)? {
                            all = false;
                            break;
                        }
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    all
                }
                HeapData::FrozenSet(fset) => {
                    let items: Vec<Value> = fset.storage().iter().map(|v| v.clone_with_heap(heap)).collect();
                    let mut all = true;
                    for item in &items {
                        if !object_is_readable_impl(heap, item, seen)? {
                            all = false;
                            break;
                        }
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    all
                }
                _ => false,
            };

            seen.remove(heap_id);
            Ok(readable)
        }
        _ => Ok(true),
    }
}

/// Check if an object has recursive references.
fn check_recursive(heap: &mut Heap<impl ResourceTracker>, obj: &Value, seen: &mut HashSet<HeapId>) -> RunResult<bool> {
    match obj {
        Value::Ref(heap_id) => {
            if !seen.insert(*heap_id) {
                // Already seen - recursive
                return Ok(true);
            }

            // Collect data first to avoid borrow issues
            let heap_data_type = match heap.get(*heap_id) {
                HeapData::List(_) => 0,
                HeapData::Tuple(_) => 1,
                HeapData::Dict(_) => 2,
                _ => 3,
            };

            let is_recursive = match heap_data_type {
                0 => {
                    let items: Vec<Value> = match heap.get(*heap_id) {
                        HeapData::List(list) => list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
                        _ => unreachable!(),
                    };
                    let mut found = false;
                    for item in &items {
                        if check_recursive(heap, item, seen)? {
                            found = true;
                            break;
                        }
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    found
                }
                1 => {
                    let items: Vec<Value> = match heap.get(*heap_id) {
                        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
                        _ => unreachable!(),
                    };
                    let mut found = false;
                    for item in &items {
                        if check_recursive(heap, item, seen)? {
                            found = true;
                            break;
                        }
                    }
                    for item in items {
                        item.drop_with_heap(heap);
                    }
                    found
                }
                2 => {
                    // Get entries from dict using with_entry_mut to avoid borrow issues
                    let entries: Vec<(Value, Value)> = heap.with_entry_mut(*heap_id, |heap, data| match data {
                        HeapData::Dict(dict) => {
                            if dict.is_empty() {
                                Vec::new()
                            } else {
                                dict.items(heap)
                            }
                        }
                        _ => unreachable!(),
                    });
                    let mut found = false;
                    for (k, v) in &entries {
                        if check_recursive(heap, k, seen)? || check_recursive(heap, v, seen)? {
                            found = true;
                            break;
                        }
                    }
                    for (k, v) in entries {
                        k.drop_with_heap(heap);
                        v.drop_with_heap(heap);
                    }
                    found
                }
                _ => false,
            };

            seen.remove(heap_id);
            Ok(is_recursive)
        }
        _ => Ok(false),
    }
}

/// Implementation of `pprint.saferepr(object)`.
///
/// Version of repr() which can handle recursive data structures.
fn saferepr(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("pprint.saferepr", heap)?;
    defer_drop!(obj, heap);

    let mut seen = HashSet::new();
    let repr = format_saferepr(heap, interns, obj, &mut seen)?;

    let str_obj = Str::from(repr);
    let id = heap.allocate(HeapData::Str(str_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Format a safe repr with recursion protection.
fn format_saferepr(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    obj: &Value,
    seen: &mut HashSet<HeapId>,
) -> RunResult<String> {
    match obj {
        Value::Ref(heap_id) => {
            if !seen.insert(*heap_id) {
                // Already seen - show recursion reference
                return Ok(format!(
                    "<Recursion on {} with id={}>",
                    obj.py_type(heap),
                    obj.public_id()
                ));
            }

            // Collect data first to avoid borrow issues
            let result = match heap.get(*heap_id) {
                HeapData::List(list) => {
                    let items: Vec<Value> = list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
                    if items.is_empty() {
                        "[]".to_string()
                    } else {
                        let mut parts = Vec::new();
                        for item in &items {
                            parts.push(format_saferepr(heap, interns, item, seen)?);
                        }
                        for item in items {
                            item.drop_with_heap(heap);
                        }
                        format!("[{}]", parts.join(", "))
                    }
                }
                HeapData::Tuple(tuple) => {
                    let items: Vec<Value> = tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect();
                    if items.is_empty() {
                        "()".to_string()
                    } else if items.len() == 1 {
                        let result = format!("({},)", format_saferepr(heap, interns, &items[0], seen)?);
                        for item in items {
                            item.drop_with_heap(heap);
                        }
                        result
                    } else {
                        let mut parts = Vec::new();
                        for item in &items {
                            parts.push(format_saferepr(heap, interns, item, seen)?);
                        }
                        for item in items {
                            item.drop_with_heap(heap);
                        }
                        format!("({})", parts.join(", "))
                    }
                }
                HeapData::Dict(_) => {
                    // Get items from dict using with_entry_mut to avoid borrow issues
                    let items: Vec<(Value, Value)> = heap.with_entry_mut(*heap_id, |heap, data| match data {
                        HeapData::Dict(dict) => {
                            if dict.is_empty() {
                                Vec::new()
                            } else {
                                dict.items(heap)
                            }
                        }
                        _ => unreachable!(),
                    });

                    if items.is_empty() {
                        return Ok("{}".to_string());
                    }

                    let mut parts = Vec::new();
                    for (k, v) in &items {
                        let key_repr = format_saferepr(heap, interns, k, seen)?;
                        let val_repr = format_saferepr(heap, interns, v, seen)?;
                        parts.push(format!("{key_repr}: {val_repr}"));
                    }
                    for (k, v) in items {
                        k.drop_with_heap(heap);
                        v.drop_with_heap(heap);
                    }
                    format!("{{{}}}", parts.join(", "))
                }
                HeapData::Set(set) => {
                    if set.is_empty() {
                        "set()".to_string()
                    } else {
                        let items: Vec<Value> = set.storage().iter().map(|v| v.clone_with_heap(heap)).collect();
                        let mut parts = Vec::new();
                        for item in &items {
                            parts.push(format_saferepr(heap, interns, item, seen)?);
                        }
                        for item in items {
                            item.drop_with_heap(heap);
                        }
                        format!("{{{}}}", parts.join(", "))
                    }
                }
                HeapData::FrozenSet(fset) => {
                    if fset.is_empty() {
                        "frozenset()".to_string()
                    } else {
                        let items: Vec<Value> = fset.storage().iter().map(|v| v.clone_with_heap(heap)).collect();
                        let mut parts = Vec::new();
                        for item in &items {
                            parts.push(format_saferepr(heap, interns, item, seen)?);
                        }
                        for item in items {
                            item.drop_with_heap(heap);
                        }
                        format!("frozenset({{{}}})", parts.join(", "))
                    }
                }
                _ => obj.py_repr(heap, interns).into_owned(),
            };

            seen.remove(heap_id);
            Ok(result)
        }
        _ => Ok(obj.py_repr(heap, interns).into_owned()),
    }
}

/// Format an object for pretty-printing.
pub(crate) fn format_object(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    obj: &Value,
    params: &PprintParams,
) -> RunResult<String> {
    let mut context = FormatContext::new(params);
    format_value(heap, interns, obj, &mut context, 0)
}

/// Context for formatting.
struct FormatContext<'a> {
    params: &'a PprintParams,
    seen: HashSet<HeapId>,
}

impl<'a> FormatContext<'a> {
    fn new(params: &'a PprintParams) -> Self {
        Self {
            params,
            seen: HashSet::new(),
        }
    }
}

/// Format a value with the given context and current level.
fn format_value(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    obj: &Value,
    context: &mut FormatContext<'_>,
    level: usize,
) -> RunResult<String> {
    if let Some(depth_repr) = depth_placeholder(heap, obj, context.params.depth, level) {
        return Ok(depth_repr);
    }

    match obj {
        Value::Ref(heap_id) => {
            // Check for recursion
            if !context.seen.insert(*heap_id) {
                return Ok(format!(
                    "<Recursion on {} with id={}>",
                    obj.py_type(heap),
                    obj.public_id()
                ));
            }

            // Collect data first to avoid borrow issues
            let heap_data_type = match heap.get(*heap_id) {
                HeapData::List(_) => 0,
                HeapData::Tuple(_) => 1,
                HeapData::Dict(_) => 2,
                HeapData::Set(_) => 3,
                HeapData::FrozenSet(_) => 4,
                _ => 5,
            };

            let result = match heap_data_type {
                0 => {
                    let items: Vec<Value> = match heap.get(*heap_id) {
                        HeapData::List(list) => list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
                        _ => unreachable!(),
                    };
                    format_list(heap, interns, &items, context, level)
                }
                1 => {
                    let items: Vec<Value> = match heap.get(*heap_id) {
                        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect(),
                        _ => unreachable!(),
                    };
                    format_tuple(heap, interns, &items, context, level)
                }
                2 => format_dict(heap, interns, heap_id, context, level),
                3 => {
                    let items: Vec<Value> = match heap.get(*heap_id) {
                        HeapData::Set(set) => set.storage().iter().map(|v| v.clone_with_heap(heap)).collect(),
                        _ => unreachable!(),
                    };
                    format_set(heap, interns, &items, "{", "}", context, level)
                }
                4 => {
                    let items: Vec<Value> = match heap.get(*heap_id) {
                        HeapData::FrozenSet(fset) => fset.storage().iter().map(|v| v.clone_with_heap(heap)).collect(),
                        _ => unreachable!(),
                    };
                    format_set(heap, interns, &items, "frozenset({", "})", context, level)
                }
                _ => Ok(obj.py_repr(heap, interns).into_owned()),
            };

            context.seen.remove(heap_id);
            result
        }
        _ => Ok(obj.py_repr(heap, interns).into_owned()),
    }
}

/// Returns the depth placeholder representation for a container at the current level.
fn depth_placeholder(
    heap: &Heap<impl ResourceTracker>,
    obj: &Value,
    max_depth: Option<usize>,
    level: usize,
) -> Option<String> {
    if let Some(max_depth) = max_depth {
        if level < max_depth {
            return None;
        }
        if let Value::Ref(heap_id) = obj {
            let value = match heap.get(*heap_id) {
                HeapData::List(_) => "[...]",
                HeapData::Tuple(_) => "(...)",
                HeapData::Dict(_) => "{...}",
                HeapData::Set(_) => "{...}",
                HeapData::FrozenSet(_) => "frozenset({...})",
                _ => return None,
            };
            return Some(value.to_string());
        }
    }
    None
}

/// Returns the visible width of a string in code points.
fn display_width(value: &str) -> usize {
    value.chars().count()
}

/// Returns remaining width available at a nesting level.
fn available_width(total_width: usize, level: usize, indent: usize) -> usize {
    total_width.saturating_sub(level.saturating_mul(indent))
}

/// Returns true when a value is represented as a quoted Python string literal.
fn is_string_value(heap: &Heap<impl ResourceTracker>, value: &Value) -> bool {
    matches!(value, Value::InternString(_))
        || matches!(value, Value::Ref(heap_id) if matches!(heap.get(*heap_id), HeapData::Str(_)))
}

/// Returns the byte index at the given character width.
fn byte_index_at_char_width(text: &str, char_width: usize) -> usize {
    if char_width == 0 {
        return 0;
    }
    let mut width = 0;
    for (idx, ch) in text.char_indices() {
        width += 1;
        if width == char_width {
            return idx + ch.len_utf8();
        }
    }
    text.len()
}

/// Wraps long single-quoted string reprs into adjacent literals for pprint-style output.
fn maybe_wrap_string_repr(
    heap: &Heap<impl ResourceTracker>,
    value: &Value,
    repr: String,
    max_width: usize,
    continuation_indent: &str,
) -> String {
    if !is_string_value(heap, value) || max_width <= 2 || display_width(&repr) <= max_width {
        return repr;
    }
    if !(repr.starts_with('\'') && repr.ends_with('\'')) {
        return repr;
    }

    let mut remaining = &repr[1..repr.len() - 1];
    let max_chunk = max_width.saturating_sub(2);
    if max_chunk == 0 {
        return repr;
    }

    let mut parts = Vec::new();
    while !remaining.is_empty() {
        if display_width(remaining) <= max_chunk {
            parts.push(remaining.to_string());
            break;
        }

        let mut split_at = None;
        for (chars, (idx, ch)) in remaining.char_indices().enumerate() {
            if chars >= max_chunk {
                break;
            }
            if ch == ' ' {
                split_at = Some(idx + ch.len_utf8());
            }
        }

        let split_idx = split_at.unwrap_or_else(|| byte_index_at_char_width(remaining, max_chunk));
        if split_idx == 0 || split_idx >= remaining.len() {
            parts.push(remaining.to_string());
            break;
        }

        parts.push(remaining[..split_idx].to_string());
        remaining = &remaining[split_idx..];
        while remaining.starts_with(' ') {
            remaining = &remaining[1..];
        }
    }

    if parts.len() <= 1 {
        return repr;
    }

    let mut wrapped = format!("'{}'", parts[0]);
    for part in parts.iter().skip(1) {
        wrapped.push('\n');
        wrapped.push_str(continuation_indent);
        wrapped.push('\'');
        wrapped.push_str(part);
        wrapped.push('\'');
    }
    wrapped
}

/// Builds compact multi-line container output with open/close delimiters.
fn format_compact_items(
    items: &[String],
    open: &str,
    close: &str,
    inner_indent: &str,
    width: usize,
    first_item_prefix: &str,
) -> String {
    let Some((first, rest)) = items.split_first() else {
        return format!("{open}{close}");
    };

    let mut lines = Vec::new();
    let mut current_line = format!("{open}{first_item_prefix}{first}");
    if !rest.is_empty() {
        current_line.push(',');
    }

    for (i, item) in rest.iter().enumerate() {
        let is_last = i == rest.len() - 1;
        let token = if is_last { item.clone() } else { format!("{item},") };
        let needs_space = !current_line.ends_with(' ');

        if display_width(&current_line) + usize::from(needs_space) + display_width(&token) <= width {
            if needs_space {
                current_line.push(' ');
            }
            current_line.push_str(&token);
        } else {
            lines.push(current_line.trim_end().to_string());
            current_line = format!("{inner_indent}{token}");
        }
        if !is_last {
            current_line.push(' ');
        }
    }

    current_line.push_str(close);
    lines.push(current_line.trim_end().to_string());
    lines.join("\n")
}

/// Builds non-compact multi-line container output with one item per line.
fn format_non_compact_items(
    items: &[String],
    open: &str,
    close: &str,
    inner_indent: &str,
    first_item_prefix: &str,
) -> String {
    let Some((first, rest)) = items.split_first() else {
        return format!("{open}{close}");
    };

    if rest.is_empty() {
        return format!("{open}{first_item_prefix}{first}{close}");
    }

    let mut lines = Vec::with_capacity(items.len());
    lines.push(format!("{open}{first_item_prefix}{first},"));

    if let Some((last, middle)) = rest.split_last() {
        for item in middle {
            lines.push(format!("{inner_indent}{item},"));
        }
        lines.push(format!("{inner_indent}{last}{close}"));
    }

    lines.join("\n")
}

/// Format a list.
fn format_list(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    items: &[Value],
    context: &mut FormatContext<'_>,
    level: usize,
) -> RunResult<String> {
    if items.is_empty() {
        return Ok("[]".to_string());
    }

    format_sequence(heap, interns, items, "[", "]", context, level)
}

/// Format a tuple.
fn format_tuple(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    items: &[Value],
    context: &mut FormatContext<'_>,
    level: usize,
) -> RunResult<String> {
    if items.is_empty() {
        return Ok("()".to_string());
    }

    if items.len() == 1 {
        let inner = format_value(heap, interns, &items[0], context, level + 1)?;
        return Ok(format!("({inner},)"));
    }

    format_sequence(heap, interns, items, "(", ")", context, level)
}

/// Format a dict.
fn format_dict(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    dict_id: &HeapId,
    context: &mut FormatContext<'_>,
    level: usize,
) -> RunResult<String> {
    // Get items from dict using with_entry_mut to avoid borrow issues
    let items: Vec<(Value, Value)> = heap.with_entry_mut(*dict_id, |heap, data| match data {
        HeapData::Dict(dict) => {
            if dict.is_empty() {
                Vec::new()
            } else {
                dict.items(heap)
            }
        }
        _ => unreachable!(),
    });

    if items.is_empty() {
        return Ok("{}".to_string());
    }

    // Sort entries if sort_dicts is enabled
    // Collect entries and their reprs for sorting
    let mut entries_with_repr: Vec<((Value, Value), String)> = items
        .into_iter()
        .map(|(k, v)| {
            let repr = k.py_repr(heap, interns);
            ((k, v), repr.into_owned())
        })
        .collect();

    if context.params.sort_dicts {
        // Sort by key repr for consistent ordering
        entries_with_repr.sort_by(|a, b| a.1.cmp(&b.1));
    }

    let indent = context.params.indent;
    let width = context.params.width;

    let mut rendered_items = Vec::with_capacity(entries_with_repr.len());
    for ((k, v), _) in &entries_with_repr {
        let key_repr = format_value(heap, interns, k, context, level + 1)?;
        let val_repr = format_value(heap, interns, v, context, level + 1)?;
        rendered_items.push(format!("{key_repr}: {val_repr}"));
    }

    let single_line = format!("{{{}}}", rendered_items.join(", "));
    if display_width(&single_line) <= available_width(width, level, indent) {
        // Drop cloned values before returning
        for ((k, v), _) in entries_with_repr {
            k.drop_with_heap(heap);
            v.drop_with_heap(heap);
        }
        return Ok(single_line);
    }

    let inner_indent = " ".repeat((level + 1) * indent);
    let first_item_prefix = " ".repeat(indent.saturating_sub(1));
    let result = if context.params.compact {
        format_compact_items(&rendered_items, "{", "}", &inner_indent, width, &first_item_prefix)
    } else {
        format_non_compact_items(&rendered_items, "{", "}", &inner_indent, &first_item_prefix)
    };

    // Drop cloned values
    for ((k, v), _) in entries_with_repr {
        k.drop_with_heap(heap);
        v.drop_with_heap(heap);
    }

    Ok(result)
}

/// Format a set or frozenset.
fn format_set(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    items: &[Value],
    open: &str,
    close: &str,
    context: &mut FormatContext<'_>,
    level: usize,
) -> RunResult<String> {
    if items.is_empty() {
        return Ok(if open == "{" {
            "set()".to_string()
        } else {
            "frozenset()".to_string()
        });
    }

    let mut items_with_repr: Vec<_> = items
        .iter()
        .map(|item| {
            let repr = item.py_repr(heap, interns);
            (item.clone_with_heap(heap), repr.into_owned())
        })
        .collect();
    items_with_repr.sort_by(|a, b| a.1.cmp(&b.1));
    if items_with_repr
        .iter()
        .all(|(item, _)| matches!(item, Value::Ref(id) if matches!(heap.get(*id), HeapData::FrozenSet(_))))
    {
        items_with_repr.reverse();
    }

    let indent = context.params.indent;
    let width = context.params.width;

    let mut rendered_items = Vec::with_capacity(items_with_repr.len());
    for (item, _) in &items_with_repr {
        rendered_items.push(format_value(heap, interns, item, context, level + 1)?);
    }
    let single_line = format!("{open}{}{close}", rendered_items.join(", "));

    if display_width(&single_line) <= available_width(width, level, indent) {
        // Drop cloned items before returning
        for (item, _) in items_with_repr {
            item.drop_with_heap(heap);
        }
        return Ok(single_line);
    }

    let inner_indent = " ".repeat((level + 1) * indent);
    let result = if context.params.compact {
        format_compact_items(&rendered_items, open, close, &inner_indent, width, "")
    } else {
        format_non_compact_items(&rendered_items, open, close, &inner_indent, "")
    };

    // Drop cloned items
    for (item, _) in items_with_repr {
        item.drop_with_heap(heap);
    }

    Ok(result)
}

/// Format a sequence (list or tuple).
fn format_sequence(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    items: &[Value],
    open: &str,
    close: &str,
    context: &mut FormatContext<'_>,
    level: usize,
) -> RunResult<String> {
    let indent = context.params.indent;
    let width = context.params.width;
    let inner_indent = " ".repeat((level + 1) * indent);
    let mut rendered_items = Vec::with_capacity(items.len());
    for item in items {
        let item_repr = format_value(heap, interns, item, context, level + 1)?;
        let wrapped = maybe_wrap_string_repr(
            heap,
            item,
            item_repr,
            available_width(width, level + 1, indent),
            &inner_indent,
        );
        rendered_items.push(wrapped);
    }
    let single_line = format!("{open}{}{close}", rendered_items.join(", "));

    if display_width(&single_line) <= available_width(width, level, indent) {
        return Ok(single_line);
    }

    if context.params.compact {
        Ok(format_compact_items(
            &rendered_items,
            open,
            close,
            &inner_indent,
            width,
            "",
        ))
    } else {
        Ok(format_non_compact_items(
            &rendered_items,
            open,
            close,
            &inner_indent,
            "",
        ))
    }
}

/// Builds a deferred builtin `print()` call for pprint output.
///
/// Returning `CallFunction` keeps pprint output on the same `PrintWriter`
/// pipeline as Python's `print(...)`, which preserves buffering and ordering.
pub(crate) fn build_print_call_result(
    heap: &mut Heap<impl ResourceTracker>,
    formatted: String,
) -> RunResult<AttrCallResult> {
    let formatted_id = heap.allocate(HeapData::Str(Str::from(formatted)))?;
    Ok(AttrCallResult::CallFunction(
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Print)),
        ArgValues::One(Value::Ref(formatted_id)),
    ))
}

/// Stub implementation of `pprint.PrettyPrinter` class constructor.
///
/// For now, returns None. A full implementation would return a PrettyPrinter
/// object that can be used to format multiple objects with the same settings.
fn pretty_printer(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut pos_args, kwargs) = args.into_parts();
    let pos_count = pos_args.len();
    if pos_count > 4 {
        pos_args.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("PrettyPrinter", 4, pos_count));
    }

    let mut params = PprintParams::default();

    if let Some(indent) = pos_args.next() {
        let val = indent.as_int(heap)?;
        indent.drop_with_heap(heap);
        if val < 0 {
            kwargs.drop_with_heap(heap);
            pos_args.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::ValueError, "indent must be >= 0").into());
        }
        params.indent = val as usize;
    }
    if let Some(width) = pos_args.next() {
        let val = width.as_int(heap)?;
        width.drop_with_heap(heap);
        if val < 0 {
            kwargs.drop_with_heap(heap);
            pos_args.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::ValueError, "width must be >= 0").into());
        }
        params.width = val as usize;
    }
    if let Some(depth) = pos_args.next() {
        if matches!(depth, Value::None) {
            params.depth = None;
        } else {
            let val = depth.as_int(heap)?;
            if val < 0 {
                depth.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                pos_args.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "depth must be >= 0").into());
            }
            params.depth = Some(val as usize);
        }
        depth.drop_with_heap(heap);
    }
    if let Some(stream) = pos_args.next() {
        stream.drop_with_heap(heap);
    }

    let params = extract_pprint_params(heap, interns, kwargs.into_iter(), params)?;
    let class_id = match create_pretty_printer_class(heap, interns) {
        Ok(class_id) => class_id,
        Err(_) => {
            return Err(SimpleException::new_msg(ExcType::RuntimeError, "failed to create PrettyPrinter class").into());
        }
    };
    let object = crate::types::StdlibObject::new_pretty_printer(params, class_id);
    let object_id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(object_id)))
}

/// Creates the runtime class object used by `type(pprint.PrettyPrinter(...))`.
fn create_pretty_printer_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap("pprint.PrettyPrinter".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        vec![object_class],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[object_class], heap, interns)
        .expect("PrettyPrinter helper class should always have a valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    heap.with_entry_mut(object_class, |_, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("builtin object class registry should be mutable");

    Ok(class_id)
}
