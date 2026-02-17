//! Sandboxed compatibility implementation of Python's `linecache` module.
//!
//! This mirrors the core CPython APIs used by traceback/warnings consumers:
//! - `getline(filename, lineno, module_globals=None)`
//! - `getlines(filename, module_globals=None)`
//! - `clearcache()`
//! - `checkcache(filename=None)` (sandbox no-op)
//! - `lazycache(filename, module_globals)`
//!
//! Ouros runs in a sandbox and should degrade gracefully when filesystem reads are
//! unavailable. Missing files return empty results rather than raising.

use std::{
    fs,
    sync::{Mutex, OnceLock},
};

use smallvec::smallvec;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, List, Module, PyTrait, Str, allocate_tuple},
    value::Value,
};

/// `linecache` module callables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum LinecacheFunctions {
    Getline,
    Getlines,
    Clearcache,
    Checkcache,
    Lazycache,
}

/// Runtime state for the `linecache` module.
#[derive(Debug, Default)]
struct LinecacheRuntimeState {
    module_id: Option<HeapId>,
}

static LINECACHE_STATE: OnceLock<Mutex<LinecacheRuntimeState>> = OnceLock::new();

fn linecache_state() -> &'static Mutex<LinecacheRuntimeState> {
    LINECACHE_STATE.get_or_init(|| Mutex::new(LinecacheRuntimeState::default()))
}

fn prune_dead_module_id(state: &mut LinecacheRuntimeState, heap: &Heap<impl ResourceTracker>) {
    if let Some(module_id) = state.module_id
        && heap.get_if_live(module_id).is_none()
    {
        state.module_id = None;
    }
}

fn linecache_module_id(heap: &Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let mut state = linecache_state().lock().expect("linecache state mutex poisoned");
    prune_dead_module_id(&mut state, heap);
    state
        .module_id
        .ok_or_else(|| SimpleException::new_msg(ExcType::RuntimeError, "linecache module not initialized").into())
}

fn linecache_cache_dict_id(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let module_id = linecache_module_id(heap)?;
    heap.with_entry_mut(module_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error(
                "internal linecache object is not a module".to_string(),
            ));
        };
        let Some(value) = module.attrs().get_by_str("cache", heap, interns) else {
            return Err(SimpleException::new_msg(ExcType::RuntimeError, "linecache.cache missing").into());
        };
        let Value::Ref(cache_id) = value else {
            return Err(ExcType::type_error("linecache.cache must be a dict".to_string()));
        };
        if !matches!(heap.get(*cache_id), HeapData::Dict(_)) {
            return Err(ExcType::type_error("linecache.cache must be a dict".to_string()));
        }
        Ok(*cache_id)
    })
}

/// Creates the `linecache` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Linecache);

    for (name, function) in [
        ("getline", LinecacheFunctions::Getline),
        ("getlines", LinecacheFunctions::Getlines),
        ("clearcache", LinecacheFunctions::Clearcache),
        ("checkcache", LinecacheFunctions::Checkcache),
        ("lazycache", LinecacheFunctions::Lazycache),
    ] {
        module.set_attr_text(
            name,
            Value::ModuleFunction(ModuleFunctions::Linecache(function)),
            heap,
            interns,
        )?;
    }

    let cache_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    module.set_attr_text("cache", Value::Ref(cache_id), heap, interns)?;

    let module_id = heap.allocate(HeapData::Module(module))?;
    let mut state = linecache_state().lock().expect("linecache state mutex poisoned");
    state.module_id = Some(module_id);
    Ok(module_id)
}

/// Dispatches a call to a `linecache` module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: LinecacheFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        LinecacheFunctions::Getline => getline(heap, interns, args).map(AttrCallResult::Value),
        LinecacheFunctions::Getlines => getlines(heap, interns, args).map(AttrCallResult::Value),
        LinecacheFunctions::Clearcache => clearcache(heap, interns, args).map(AttrCallResult::Value),
        LinecacheFunctions::Checkcache => checkcache(heap, interns, args).map(AttrCallResult::Value),
        LinecacheFunctions::Lazycache => lazycache(heap, interns, args).map(AttrCallResult::Value),
    }
}

fn getline(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (filename, lineno, module_globals) = parse_getline_args(args, heap, interns)?;
    let filename_text = filename.py_str(heap, interns).into_owned();
    filename.drop_with_heap(heap);

    let lineno_value = match lineno.as_int(heap) {
        Ok(value) => value,
        Err(err) => {
            lineno.drop_with_heap(heap);
            module_globals.drop_with_heap(heap);
            return Err(err);
        }
    };
    lineno.drop_with_heap(heap);
    module_globals.drop_with_heap(heap);

    if lineno_value <= 0 || filename_text.is_empty() {
        return Ok(StaticStrings::EmptyString.into());
    }

    let lines = getlines_impl(&filename_text, heap, interns)?;
    let line_index = usize::try_from(lineno_value.saturating_sub(1)).unwrap_or(usize::MAX);
    let result = line_from_list_value(&lines, line_index, heap).unwrap_or_else(|| StaticStrings::EmptyString.into());
    lines.drop_with_heap(heap);
    Ok(result)
}

fn getlines(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (filename, module_globals) = parse_getlines_args(args, heap, interns)?;
    let filename_text = filename.py_str(heap, interns).into_owned();
    filename.drop_with_heap(heap);
    module_globals.drop_with_heap(heap);
    getlines_impl(&filename_text, heap, interns)
}

fn clearcache(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("clearcache", heap)?;
    let cache_dict_id = linecache_cache_dict_id(heap, interns)?;
    heap.with_entry_mut(cache_dict_id, |heap, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("linecache.cache must be a dict".to_string()));
        };
        dict.drop_all_entries(heap);
        Ok(())
    })?;
    Ok(Value::None)
}

fn checkcache(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let filename = parse_checkcache_args(args, heap, interns)?;
    if let Some(filename) = filename {
        filename.drop_with_heap(heap);
    }
    // Sandbox no-op: filesystem metadata is not authoritative in Ouros runtime.
    Ok(Value::None)
}

fn lazycache(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (filename, module_globals) = parse_lazycache_args(args, heap, interns)?;
    let filename_text = filename.py_str(heap, interns).into_owned();
    filename.drop_with_heap(heap);
    module_globals.drop_with_heap(heap);

    if filename_text.is_empty() {
        return Ok(Value::Bool(false));
    }

    if let Some(lines) = cached_lines_for_filename(&filename_text, heap, interns)? {
        lines.drop_with_heap(heap);
        return Ok(Value::Bool(true));
    }

    let Some((source_size, lines)) = load_source_lines(&filename_text) else {
        return Ok(Value::Bool(false));
    };

    let lines_list_id = allocate_lines_list(&lines, heap)?;
    store_cache_entry(&filename_text, source_size, lines_list_id, heap, interns)?;
    Ok(Value::Bool(true))
}

fn getlines_impl(filename: &str, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    if filename.is_empty() {
        return allocate_empty_list(heap);
    }

    if let Some(lines) = cached_lines_for_filename(filename, heap, interns)? {
        return Ok(lines);
    }

    let Some((source_size, lines)) = load_source_lines(filename) else {
        return allocate_empty_list(heap);
    };

    let lines_list_id = allocate_lines_list(&lines, heap)?;
    store_cache_entry(filename, source_size, lines_list_id, heap, interns)?;
    Ok(Value::Ref(lines_list_id))
}

fn cached_lines_for_filename(
    filename: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let cache_dict_id = linecache_cache_dict_id(heap, interns)?;
    let Some(entry_value) = dict_value_by_str(cache_dict_id, filename, heap, interns) else {
        return Ok(None);
    };

    let result = cached_lines_from_entry(&entry_value, heap).map(|lines| lines.clone_with_heap(heap));
    entry_value.drop_with_heap(heap);
    Ok(result)
}

fn dict_value_by_str(
    dict_id: HeapId,
    key: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Dict(dict) = heap.get(dict_id) else {
        return None;
    };
    dict.get_by_str(key, heap, interns)
        .map(|value| value.clone_with_heap(heap))
}

fn cached_lines_from_entry<'a>(entry: &'a Value, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a Value> {
    let Value::Ref(entry_id) = entry else {
        return None;
    };
    match heap.get(*entry_id) {
        HeapData::List(_) => Some(entry),
        HeapData::Tuple(tuple) => tuple.as_vec().get(2).and_then(|value| match value {
            Value::Ref(lines_id) if matches!(heap.get(*lines_id), HeapData::List(_)) => Some(value),
            _ => None,
        }),
        _ => None,
    }
}

fn line_from_list_value(lines: &Value, line_index: usize, heap: &Heap<impl ResourceTracker>) -> Option<Value> {
    let Value::Ref(lines_id) = lines else {
        return None;
    };
    let HeapData::List(list) = heap.get(*lines_id) else {
        return None;
    };
    list.as_vec().get(line_index).map(|value| value.clone_with_heap(heap))
}

fn allocate_empty_list(heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let list_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    Ok(Value::Ref(list_id))
}

fn allocate_lines_list(lines: &[String], heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let mut values = Vec::with_capacity(lines.len());
    for line in lines {
        let line_id = heap.allocate(HeapData::Str(Str::from(line.clone())))?;
        values.push(Value::Ref(line_id));
    }
    heap.allocate(HeapData::List(List::new(values))).map_err(Into::into)
}

fn store_cache_entry(
    filename: &str,
    source_size: i64,
    lines_list_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let cache_dict_id = linecache_cache_dict_id(heap, interns)?;
    let key_id = heap.allocate(HeapData::Str(Str::from(filename)))?;
    let fullname_id = heap.allocate(HeapData::Str(Str::from(filename)))?;
    let cache_entry = allocate_tuple(
        smallvec![
            Value::Int(source_size),
            Value::None,
            Value::Ref(lines_list_id).clone_with_heap(heap),
            Value::Ref(fullname_id),
        ],
        heap,
    )?;

    heap.with_entry_mut(cache_dict_id, |heap, data| {
        let HeapData::Dict(dict) = data else {
            Value::Ref(key_id).drop_with_heap(heap);
            cache_entry.drop_with_heap(heap);
            return Err(ExcType::type_error("linecache.cache must be a dict".to_string()));
        };
        let old_value = dict.set(Value::Ref(key_id), cache_entry, heap, interns)?;
        if let Some(old_value) = old_value {
            old_value.drop_with_heap(heap);
        }
        Ok(())
    })
}

fn load_source_lines(filename: &str) -> Option<(i64, Vec<String>)> {
    // Preserve sandbox behavior: fail closed (None) when reads are not permitted.
    let source = fs::read_to_string(filename).ok()?;
    let size = i64::try_from(source.len()).ok()?;
    if source.is_empty() {
        return Some((size, Vec::new()));
    }
    let lines = source.split_inclusive('\n').map(str::to_owned).collect();
    Some((size, lines))
}

fn parse_getline_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Value, Value)> {
    let (positional_iter, kwargs_values) = args.into_parts();
    let positional: Vec<Value> = positional_iter.collect();
    let kwargs: Vec<(Value, Value)> = kwargs_values.into_iter().collect();

    if positional.len() > 3 {
        let positional_count = positional.len();
        drop_positional_kwargs(positional, kwargs, heap);
        return Err(ExcType::type_error_too_many_positional(
            "getline",
            3,
            positional_count,
            0,
        ));
    }

    let mut filename = None;
    let mut lineno = None;
    let mut module_globals = None;

    for (index, value) in positional.into_iter().enumerate() {
        match index {
            0 => filename = Some(value),
            1 => lineno = Some(value),
            2 => module_globals = Some(value),
            _ => unreachable!("length already validated"),
        }
    }

    let mut keyword_error: Option<RunError> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            set_keyword_error(&mut keyword_error, ExcType::type_error_kwargs_nonstring_key());
            continue;
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "filename" => assign_optional_value("getline", "filename", value, &mut filename, &mut keyword_error, heap),
            "lineno" => assign_optional_value("getline", "lineno", value, &mut lineno, &mut keyword_error, heap),
            "module_globals" => assign_optional_value(
                "getline",
                "module_globals",
                value,
                &mut module_globals,
                &mut keyword_error,
                heap,
            ),
            _ => {
                value.drop_with_heap(heap);
                set_keyword_error(
                    &mut keyword_error,
                    ExcType::type_error_unexpected_keyword("getline", &key_text),
                );
            }
        }
    }

    if let Some(err) = keyword_error {
        filename.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        module_globals.drop_with_heap(heap);
        return Err(err);
    }

    let Some(filename) = filename else {
        lineno.drop_with_heap(heap);
        module_globals.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "getline() missing required argument 'filename' (pos 1)",
        )
        .into());
    };
    let Some(lineno) = lineno else {
        filename.drop_with_heap(heap);
        module_globals.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "getline() missing required argument 'lineno' (pos 2)",
        )
        .into());
    };

    Ok((filename, lineno, module_globals.unwrap_or(Value::None)))
}

fn parse_getlines_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Value)> {
    let (positional_iter, kwargs_values) = args.into_parts();
    let positional: Vec<Value> = positional_iter.collect();
    let kwargs: Vec<(Value, Value)> = kwargs_values.into_iter().collect();

    if positional.len() > 2 {
        let positional_count = positional.len();
        drop_positional_kwargs(positional, kwargs, heap);
        return Err(ExcType::type_error_too_many_positional(
            "getlines",
            2,
            positional_count,
            0,
        ));
    }

    let mut filename = None;
    let mut module_globals = None;

    for (index, value) in positional.into_iter().enumerate() {
        match index {
            0 => filename = Some(value),
            1 => module_globals = Some(value),
            _ => unreachable!("length already validated"),
        }
    }

    let mut keyword_error: Option<RunError> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            set_keyword_error(&mut keyword_error, ExcType::type_error_kwargs_nonstring_key());
            continue;
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "filename" => assign_optional_value("getlines", "filename", value, &mut filename, &mut keyword_error, heap),
            "module_globals" => assign_optional_value(
                "getlines",
                "module_globals",
                value,
                &mut module_globals,
                &mut keyword_error,
                heap,
            ),
            _ => {
                value.drop_with_heap(heap);
                set_keyword_error(
                    &mut keyword_error,
                    ExcType::type_error_unexpected_keyword("getlines", &key_text),
                );
            }
        }
    }

    if let Some(err) = keyword_error {
        filename.drop_with_heap(heap);
        module_globals.drop_with_heap(heap);
        return Err(err);
    }

    let Some(filename) = filename else {
        module_globals.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "getlines() missing required argument 'filename' (pos 1)",
        )
        .into());
    };

    Ok((filename, module_globals.unwrap_or(Value::None)))
}

fn parse_checkcache_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let (positional_iter, kwargs_values) = args.into_parts();
    let positional: Vec<Value> = positional_iter.collect();
    let kwargs: Vec<(Value, Value)> = kwargs_values.into_iter().collect();

    if positional.len() > 1 {
        let positional_count = positional.len();
        drop_positional_kwargs(positional, kwargs, heap);
        return Err(ExcType::type_error_too_many_positional(
            "checkcache",
            1,
            positional_count,
            0,
        ));
    }

    let mut filename = positional.into_iter().next();

    let mut keyword_error: Option<RunError> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            set_keyword_error(&mut keyword_error, ExcType::type_error_kwargs_nonstring_key());
            continue;
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_text.as_str() == "filename" {
            assign_optional_value("checkcache", "filename", value, &mut filename, &mut keyword_error, heap);
        } else {
            value.drop_with_heap(heap);
            set_keyword_error(
                &mut keyword_error,
                ExcType::type_error_unexpected_keyword("checkcache", &key_text),
            );
        }
    }

    if let Some(err) = keyword_error {
        filename.drop_with_heap(heap);
        return Err(err);
    }

    Ok(filename)
}

fn parse_lazycache_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Value)> {
    let (positional_iter, kwargs_values) = args.into_parts();
    let positional: Vec<Value> = positional_iter.collect();
    let kwargs: Vec<(Value, Value)> = kwargs_values.into_iter().collect();

    if positional.len() > 2 {
        let positional_count = positional.len();
        drop_positional_kwargs(positional, kwargs, heap);
        return Err(ExcType::type_error_too_many_positional(
            "lazycache",
            2,
            positional_count,
            0,
        ));
    }

    let mut filename = None;
    let mut module_globals = None;

    for (index, value) in positional.into_iter().enumerate() {
        match index {
            0 => filename = Some(value),
            1 => module_globals = Some(value),
            _ => unreachable!("length already validated"),
        }
    }

    let mut keyword_error: Option<RunError> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            set_keyword_error(&mut keyword_error, ExcType::type_error_kwargs_nonstring_key());
            continue;
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "filename" => {
                assign_optional_value("lazycache", "filename", value, &mut filename, &mut keyword_error, heap);
            }
            "module_globals" => {
                assign_optional_value(
                    "lazycache",
                    "module_globals",
                    value,
                    &mut module_globals,
                    &mut keyword_error,
                    heap,
                );
            }
            _ => {
                value.drop_with_heap(heap);
                set_keyword_error(
                    &mut keyword_error,
                    ExcType::type_error_unexpected_keyword("lazycache", &key_text),
                );
            }
        }
    }

    if let Some(err) = keyword_error {
        filename.drop_with_heap(heap);
        module_globals.drop_with_heap(heap);
        return Err(err);
    }

    let Some(filename) = filename else {
        module_globals.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "lazycache() missing required argument 'filename' (pos 1)",
        )
        .into());
    };
    let Some(module_globals) = module_globals else {
        filename.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "lazycache() missing required argument 'module_globals' (pos 2)",
        )
        .into());
    };

    Ok((filename, module_globals))
}

fn assign_optional_value(
    function_name: &str,
    parameter_name: &str,
    value: Value,
    target: &mut Option<Value>,
    keyword_error: &mut Option<RunError>,
    heap: &mut Heap<impl ResourceTracker>,
) {
    if target.is_some() {
        value.drop_with_heap(heap);
        set_keyword_error(
            keyword_error,
            ExcType::type_error_duplicate_arg(function_name, parameter_name),
        );
        return;
    }
    *target = Some(value);
}

fn set_keyword_error(error_slot: &mut Option<RunError>, new_error: RunError) {
    if error_slot.is_none() {
        *error_slot = Some(new_error);
    }
}

fn drop_positional_kwargs(positional: Vec<Value>, kwargs: Vec<(Value, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    positional.drop_with_heap(heap);
    for (key, value) in kwargs {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
}
