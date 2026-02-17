//! CPython-compatible implementation of the `warnings` module.
//!
//! The implementation mirrors the public API shape from CPython 3.14 and keeps
//! all state inside the module object (`filters`, `defaultaction`, and
//! `onceregistry`) so Python code can introspect and mutate it naturally.
//! Internally, Ouros tracks active `catch_warnings(record=True)` contexts in a
//! small runtime stack to support warning capture without host I/O.

use std::{
    path::Path,
    str::FromStr,
    sync::{Mutex, OnceLock},
};

use fancy_regex::Regex;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::{BuiltinModule, ModuleFunctions},
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, Instance, List, Module, PyTrait, Str, Type, allocate_tuple, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// Function and method entry points for the `warnings` module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum WarningsFunctions {
    Warn,
    WarnExplicit,
    Filterwarnings,
    Simplefilter,
    Resetwarnings,
    Formatwarning,
    Showwarning,
    WarningMessageInit,
    WarningMessageStr,
    CatchWarningsInit,
    CatchWarningsEnter,
    CatchWarningsExit,
    CatchWarningsRepr,
    DeprecatedInit,
    DeprecatedCall,
}

/// Holds the most recently created `warnings` module id.
static WARNINGS_MODULE_ID: OnceLock<Mutex<Option<HeapId>>> = OnceLock::new();
/// Stack of active `catch_warnings(record=True)` log list ids.
static WARNINGS_RECORD_STACK: OnceLock<Mutex<Vec<HeapId>>> = OnceLock::new();
/// Monotonic filter version counter used for registry invalidation.
static WARNINGS_FILTERS_VERSION: OnceLock<Mutex<i64>> = OnceLock::new();

/// Supported warning actions from CPython.
const VALID_ACTIONS: &[&str] = &["error", "ignore", "always", "all", "default", "module", "once"];

/// Creates the `warnings` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Warnings);

    register(&mut module, "warn", WarningsFunctions::Warn, heap, interns)?;
    register(
        &mut module,
        "warn_explicit",
        WarningsFunctions::WarnExplicit,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "filterwarnings",
        WarningsFunctions::Filterwarnings,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "simplefilter",
        WarningsFunctions::Simplefilter,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "resetwarnings",
        WarningsFunctions::Resetwarnings,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "formatwarning",
        WarningsFunctions::Formatwarning,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "showwarning",
        WarningsFunctions::Showwarning,
        heap,
        interns,
    )?;

    // Public module state.
    let default_action_id = heap.allocate(HeapData::Str(Str::from("default")))?;
    module.set_attr_text("defaultaction", Value::Ref(default_action_id), heap, interns)?;

    let onceregistry_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    module.set_attr_text("onceregistry", Value::Ref(onceregistry_id), heap, interns)?;

    let filters_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    module.set_attr_text("filters", Value::Ref(filters_id), heap, interns)?;

    // Runtime-only registry used by `warn()` when call-site globals are unavailable.
    let runtime_registry_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    module.set_attr_text("_ouros_registry", Value::Ref(runtime_registry_id), heap, interns)?;

    // Export the helper classes.
    let warning_message_class_id = create_warning_message_class(heap, interns)?;
    module.set_attr_text("WarningMessage", Value::Ref(warning_message_class_id), heap, interns)?;

    let catch_warnings_class_id = create_catch_warnings_class(heap, interns)?;
    module.set_attr_text("catch_warnings", Value::Ref(catch_warnings_class_id), heap, interns)?;

    let deprecated_class_id = create_deprecated_class(heap, interns)?;
    module.set_attr_text("deprecated", Value::Ref(deprecated_class_id), heap, interns)?;

    // Keep `warnings.sys` available like CPython.
    let sys_module_id = BuiltinModule::Sys.create(heap, interns)?;
    module.set_attr_text("sys", Value::Ref(sys_module_id), heap, interns)?;

    // Populate the default CPython filter set.
    initialize_default_filters(filters_id, heap)?;

    let module_id = heap.allocate(HeapData::Module(module))?;
    *warnings_module_slot()
        .lock()
        .expect("warnings module id mutex poisoned") = Some(module_id);
    *warnings_filter_version_slot()
        .lock()
        .expect("warnings filter version mutex poisoned") = 1;

    Ok(module_id)
}

/// Dispatches `warnings` module and helper class function calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: WarningsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        WarningsFunctions::Warn => warn(heap, interns, args)?,
        WarningsFunctions::WarnExplicit => warn_explicit(heap, interns, args)?,
        WarningsFunctions::Filterwarnings => filterwarnings(heap, interns, args)?,
        WarningsFunctions::Simplefilter => simplefilter(heap, interns, args)?,
        WarningsFunctions::Resetwarnings => resetwarnings(heap, interns, args)?,
        WarningsFunctions::Formatwarning => formatwarning(heap, interns, args)?,
        WarningsFunctions::Showwarning => showwarning(heap, interns, args)?,
        WarningsFunctions::WarningMessageInit => warning_message_init(heap, interns, args)?,
        WarningsFunctions::WarningMessageStr => warning_message_str(heap, interns, args)?,
        WarningsFunctions::CatchWarningsInit => catch_warnings_init(heap, interns, args)?,
        WarningsFunctions::CatchWarningsEnter => catch_warnings_enter(heap, interns, args)?,
        WarningsFunctions::CatchWarningsExit => catch_warnings_exit(heap, interns, args)?,
        WarningsFunctions::CatchWarningsRepr => catch_warnings_repr(heap, interns, args)?,
        WarningsFunctions::DeprecatedInit => deprecated_init(heap, interns, args)?,
        WarningsFunctions::DeprecatedCall => deprecated_call(heap, interns, args)?,
    };

    Ok(AttrCallResult::Value(value))
}

/// Implements `warnings.warn(message, category=None, stacklevel=1, source=None, *, skip_file_prefixes=())`.
fn warn(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::TypeError, "warn() missing required argument 'message' (pos 1)").into(),
        );
    }

    let message = positional.remove(0);
    let mut category = positional
        .first()
        .map_or(Value::None, |value| value.clone_with_heap(heap));
    let mut stacklevel = positional
        .get(1)
        .map_or(Value::Int(1), |value| value.clone_with_heap(heap));
    let mut source = positional
        .get(2)
        .map_or(Value::None, |value| value.clone_with_heap(heap));
    let mut skip_file_prefixes = Value::None;

    if positional.len() > 4 {
        for value in positional {
            value.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        message.drop_with_heap(heap);
        category.drop_with_heap(heap);
        stacklevel.drop_with_heap(heap);
        source.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("warn", 4, 5, 0));
    }

    let mut saw_category_pos = !positional.is_empty();
    let mut saw_stacklevel_pos = positional.len() >= 2;
    let mut saw_source_pos = positional.len() >= 3;

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            stacklevel.drop_with_heap(heap);
            source.drop_with_heap(heap);
            skip_file_prefixes.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "category" => {
                if saw_category_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    stacklevel.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    skip_file_prefixes.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn", "category"));
                }
                saw_category_pos = true;
                category.drop_with_heap(heap);
                category = value;
            }
            "stacklevel" => {
                if saw_stacklevel_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    stacklevel.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    skip_file_prefixes.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn", "stacklevel"));
                }
                saw_stacklevel_pos = true;
                stacklevel.drop_with_heap(heap);
                stacklevel = value;
            }
            "source" => {
                if saw_source_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    stacklevel.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    skip_file_prefixes.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn", "source"));
                }
                saw_source_pos = true;
                source.drop_with_heap(heap);
                source = value;
            }
            "skip_file_prefixes" => {
                skip_file_prefixes.drop_with_heap(heap);
                skip_file_prefixes = value;
            }
            _ => {
                value.drop_with_heap(heap);
                message.drop_with_heap(heap);
                category.drop_with_heap(heap);
                stacklevel.drop_with_heap(heap);
                source.drop_with_heap(heap);
                skip_file_prefixes.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("warn", &key_text));
            }
        }
    }

    if !matches!(skip_file_prefixes, Value::None) && !value_is_tuple(&skip_file_prefixes, heap) {
        let type_name = skip_file_prefixes.py_type(heap).to_string();
        message.drop_with_heap(heap);
        category.drop_with_heap(heap);
        stacklevel.drop_with_heap(heap);
        source.drop_with_heap(heap);
        skip_file_prefixes.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("warn() argument 'skip_file_prefixes' must be tuple, not {type_name}"),
        )
        .into());
    }

    let category_value = if matches!(message, Value::Ref(id) if matches!(heap.get(id), HeapData::Exception(_))) {
        category.drop_with_heap(heap);
        Value::Builtin(Builtins::ExcType(ExcType::Exception))
    } else if matches!(category, Value::None) {
        category.drop_with_heap(heap);
        warning_base_class_value()
    } else {
        category
    };

    if !is_warning_subclass(&category_value, heap, interns)? {
        let type_name = category_value.py_type(heap);
        message.drop_with_heap(heap);
        category_value.drop_with_heap(heap);
        stacklevel.drop_with_heap(heap);
        source.drop_with_heap(heap);
        skip_file_prefixes.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("category must be a Warning subclass, not '{type_name}'"),
        )
        .into());
    }

    // The VM currently does not expose frame metadata to module shims, so we
    // conservatively use CPython's `<sys>:0` fallback location.
    let filename = "<sys>".to_string();
    let lineno = if matches!(stacklevel, Value::None) {
        1
    } else {
        stacklevel.as_int(heap).unwrap_or(1)
    };
    stacklevel.drop_with_heap(heap);
    skip_file_prefixes.drop_with_heap(heap);

    let registry = get_runtime_registry_id(heap, interns)?;
    warn_explicit_impl(
        heap,
        interns,
        message,
        category_value,
        filename,
        lineno,
        Some("sys".to_string()),
        Some(registry),
        source,
    )
}

/// Implements `warnings.warn_explicit(...)`.
fn warn_explicit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();

    let mut missing = Vec::new();
    if positional.is_empty() {
        missing.push("message");
    }
    if positional.len() < 2 {
        missing.push("category");
    }
    if positional.len() < 3 {
        missing.push("filename");
    }
    if positional.len() < 4 {
        missing.push("lineno");
    }
    if !missing.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        let message = match missing[0] {
            "message" => "warn_explicit() missing required argument 'message' (pos 1)",
            "category" => "warn_explicit() missing required argument 'category' (pos 2)",
            "filename" => "warn_explicit() missing required argument 'filename' (pos 3)",
            _ => "warn_explicit() missing required argument 'lineno' (pos 4)",
        };
        return Err(SimpleException::new_msg(ExcType::TypeError, message).into());
    }

    if positional.len() > 8 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("warn_explicit", 8, 9, 0));
    }

    let message = positional.remove(0);
    let category = positional.remove(0);
    let filename_value = positional.remove(0);
    let lineno_value = positional.remove(0);

    let mut module = positional
        .first()
        .map_or(Value::None, |value| value.clone_with_heap(heap));
    let mut registry = positional
        .get(1)
        .map_or(Value::None, |value| value.clone_with_heap(heap));
    let mut module_globals = positional
        .get(2)
        .map_or(Value::None, |value| value.clone_with_heap(heap));
    let mut source = positional
        .get(3)
        .map_or(Value::None, |value| value.clone_with_heap(heap));

    let mut saw_module_pos = !positional.is_empty();
    let mut saw_registry_pos = positional.len() >= 2;
    let mut saw_module_globals_pos = positional.len() >= 3;
    let mut saw_source_pos = positional.len() >= 4;

    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            filename_value.drop_with_heap(heap);
            lineno_value.drop_with_heap(heap);
            module.drop_with_heap(heap);
            registry.drop_with_heap(heap);
            module_globals.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "module" => {
                if saw_module_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    filename_value.drop_with_heap(heap);
                    lineno_value.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    registry.drop_with_heap(heap);
                    module_globals.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn_explicit", "module"));
                }
                saw_module_pos = true;
                module.drop_with_heap(heap);
                module = value;
            }
            "registry" => {
                if saw_registry_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    filename_value.drop_with_heap(heap);
                    lineno_value.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    registry.drop_with_heap(heap);
                    module_globals.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn_explicit", "registry"));
                }
                saw_registry_pos = true;
                registry.drop_with_heap(heap);
                registry = value;
            }
            "module_globals" => {
                if saw_module_globals_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    filename_value.drop_with_heap(heap);
                    lineno_value.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    registry.drop_with_heap(heap);
                    module_globals.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn_explicit", "module_globals"));
                }
                saw_module_globals_pos = true;
                module_globals.drop_with_heap(heap);
                module_globals = value;
            }
            "source" => {
                if saw_source_pos {
                    value.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    filename_value.drop_with_heap(heap);
                    lineno_value.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    registry.drop_with_heap(heap);
                    module_globals.drop_with_heap(heap);
                    source.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("warn_explicit", "source"));
                }
                saw_source_pos = true;
                source.drop_with_heap(heap);
                source = value;
            }
            _ => {
                value.drop_with_heap(heap);
                message.drop_with_heap(heap);
                category.drop_with_heap(heap);
                filename_value.drop_with_heap(heap);
                lineno_value.drop_with_heap(heap);
                module.drop_with_heap(heap);
                registry.drop_with_heap(heap);
                module_globals.drop_with_heap(heap);
                source.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("warn_explicit", &key_text));
            }
        }
    }

    let filename = filename_value.py_str(heap, interns).into_owned();
    filename_value.drop_with_heap(heap);

    let lineno = match lineno_value.as_int(heap) {
        Ok(value) => value,
        Err(err) => {
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            lineno_value.drop_with_heap(heap);
            module.drop_with_heap(heap);
            registry.drop_with_heap(heap);
            module_globals.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return Err(err);
        }
    };
    lineno_value.drop_with_heap(heap);

    if !matches!(module_globals, Value::None)
        && !matches!(module_globals, Value::Ref(id) if matches!(heap.get(id), HeapData::Dict(_)))
    {
        let type_name = module_globals.py_type(heap);
        message.drop_with_heap(heap);
        category.drop_with_heap(heap);
        module.drop_with_heap(heap);
        registry.drop_with_heap(heap);
        module_globals.drop_with_heap(heap);
        source.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("module_globals must be a dict, not '{type_name}'"),
        )
        .into());
    }
    module_globals.drop_with_heap(heap);

    let registry_id = if matches!(registry, Value::None) {
        registry.drop_with_heap(heap);
        None
    } else if let Value::Ref(id_ref) = &registry {
        let id = *id_ref;
        if matches!(heap.get(id), HeapData::Dict(_)) {
            registry.drop_with_heap(heap);
            Some(id)
        } else {
            registry.drop_with_heap(heap);
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            module.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::TypeError, "'registry' must be a dict or None").into());
        }
    } else {
        registry.drop_with_heap(heap);
        message.drop_with_heap(heap);
        category.drop_with_heap(heap);
        module.drop_with_heap(heap);
        source.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "'registry' must be a dict or None").into());
    };

    let module_name = if matches!(module, Value::None) {
        module.drop_with_heap(heap);
        Some(default_module_name_from_filename(&filename))
    } else {
        let module_text = module.py_str(heap, interns).into_owned();
        module.drop_with_heap(heap);
        Some(module_text)
    };

    warn_explicit_impl(
        heap,
        interns,
        message,
        category,
        filename,
        lineno,
        module_name,
        registry_id,
        source,
    )
}

/// Implements `warnings.filterwarnings(...)`.
fn filterwarnings(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (action, message, category, module, lineno, append) = parse_filterwarnings_args(heap, interns, args)?;

    add_filter_item(
        heap,
        interns,
        action,
        if message.is_empty() { None } else { Some(message) },
        category,
        if module.is_empty() { None } else { Some(module) },
        lineno,
        append,
    )?;

    Ok(Value::None)
}

/// Implements `warnings.simplefilter(...)`.
fn simplefilter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (action, category, lineno, append) = parse_simplefilter_args(heap, interns, args)?;

    add_filter_item(heap, interns, action, None, category, None, lineno, append)?;
    Ok(Value::None)
}

/// Implements `warnings.resetwarnings()`.
fn resetwarnings(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count != 0 || !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(
            "resetwarnings",
            0,
            positional_count.max(1),
            0,
        ));
    }

    let filters_id = get_filters_list_id(heap, interns)?;
    let HeapData::List(list) = heap.get_mut(filters_id) else {
        return Err(ExcType::type_error("warnings.filters must be a list"));
    };
    let removed: Vec<Value> = list.as_vec_mut().drain(..).collect();
    for value in removed {
        value.drop_with_heap(heap);
    }

    bump_filters_version();
    Ok(Value::None)
}

/// Implements `warnings.formatwarning(...)`.
fn formatwarning(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (message, category, filename, lineno, _file, line, _source) =
        parse_warning_message_like_args(heap, interns, args, "formatwarning", false, true)?;

    let text = format_warning_text(
        heap,
        interns,
        &message,
        &category,
        &filename,
        &lineno,
        line.as_ref(),
        None,
    )?;

    message.drop_with_heap(heap);
    category.drop_with_heap(heap);
    filename.drop_with_heap(heap);
    lineno.drop_with_heap(heap);
    if let Some(line) = line {
        line.drop_with_heap(heap);
    }

    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(text_id))
}

/// Implements `warnings.showwarning(...)`.
fn showwarning(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (message, category, filename, lineno, file, line, _source) =
        parse_warning_message_like_args(heap, interns, args, "showwarning", true, false)?;

    let text = format_warning_text(
        heap,
        interns,
        &message,
        &category,
        &filename,
        &lineno,
        line.as_ref(),
        None,
    )?;

    let result = if let Some(ref file_value) = file {
        write_warning_to_file(file_value, &text, heap, interns)
    } else {
        eprint!("{text}");
        Ok(())
    };

    message.drop_with_heap(heap);
    category.drop_with_heap(heap);
    filename.drop_with_heap(heap);
    lineno.drop_with_heap(heap);
    if let Some(file) = file {
        file.drop_with_heap(heap);
    }
    if let Some(line) = line {
        line.drop_with_heap(heap);
    }

    result?;
    Ok(Value::None)
}

/// Implements `WarningMessage.__init__(...)`.
fn warning_message_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "WarningMessage.__init__")?;
    defer_drop!(self_value, heap);
    let (message, category, filename, lineno, file, line, source) =
        parse_warning_message_like_args(heap, interns, method_args, "WarningMessage", true, true)?;

    set_instance_attr_by_name(self_id, "message", message, heap, interns)?;
    set_instance_attr_by_name(self_id, "category", category.clone_with_heap(heap), heap, interns)?;
    set_instance_attr_by_name(self_id, "filename", filename, heap, interns)?;
    set_instance_attr_by_name(self_id, "lineno", lineno, heap, interns)?;
    set_instance_attr_by_name(self_id, "file", file.unwrap_or(Value::None), heap, interns)?;
    set_instance_attr_by_name(self_id, "line", line.unwrap_or(Value::None), heap, interns)?;
    set_instance_attr_by_name(self_id, "source", source.unwrap_or(Value::None), heap, interns)?;

    let category_name = if matches!(category, Value::None) {
        Value::None
    } else {
        let name = category_name(&category, heap, interns)?;
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        Value::Ref(name_id)
    };
    category.drop_with_heap(heap);
    set_instance_attr_by_name(self_id, "_category_name", category_name, heap, interns)?;

    Ok(Value::None)
}

/// Implements `WarningMessage.__str__()`.
fn warning_message_str(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "WarningMessage.__str__")?;
    defer_drop!(self_value, heap);
    method_args.check_zero_args("WarningMessage.__str__", heap)?;

    let message = get_instance_attr_by_name(self_id, "message", heap, interns).unwrap_or(Value::None);
    let category_name = get_instance_attr_by_name(self_id, "_category_name", heap, interns).unwrap_or(Value::None);
    let filename = get_instance_attr_by_name(self_id, "filename", heap, interns).unwrap_or(Value::None);
    let lineno = get_instance_attr_by_name(self_id, "lineno", heap, interns).unwrap_or(Value::Int(0));
    let line = get_instance_attr_by_name(self_id, "line", heap, interns).unwrap_or(Value::None);

    let text = format!(
        "{{message : {}, category : {}, filename : {}, lineno : {}, line : {}}}",
        message.py_repr(heap, interns),
        category_name.py_repr(heap, interns),
        filename.py_repr(heap, interns),
        lineno.py_str(heap, interns),
        line.py_repr(heap, interns),
    );

    message.drop_with_heap(heap);
    category_name.drop_with_heap(heap);
    filename.drop_with_heap(heap);
    lineno.drop_with_heap(heap);
    line.drop_with_heap(heap);

    let id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(id))
}

/// Implements `catch_warnings.__init__(...)`.
fn catch_warnings_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "catch_warnings.__init__")?;
    defer_drop!(self_value, heap);
    let (positional_iter, kwargs) = method_args.into_parts();
    let positional: Vec<Value> = positional_iter.collect();

    if !positional.is_empty() {
        let count = positional.len() + 1;
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(
            "catch_warnings.__init__",
            1,
            count,
            0,
        ));
    }

    let mut record = Value::Bool(false);
    let mut module = {
        let module_id = warnings_module_id()?;
        heap.inc_ref(module_id);
        Value::Ref(module_id)
    };
    let mut action = Value::None;
    let mut category = warning_base_class_value();
    let mut lineno = Value::Int(0);
    let mut append = Value::Bool(false);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            record.drop_with_heap(heap);
            module.drop_with_heap(heap);
            action.drop_with_heap(heap);
            category.drop_with_heap(heap);
            lineno.drop_with_heap(heap);
            append.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "record" => {
                record.drop_with_heap(heap);
                record = value;
            }
            "module" => {
                module.drop_with_heap(heap);
                module = value;
            }
            "action" => {
                action.drop_with_heap(heap);
                action = value;
            }
            "category" => {
                category.drop_with_heap(heap);
                category = value;
            }
            "lineno" => {
                lineno.drop_with_heap(heap);
                lineno = value;
            }
            "append" => {
                append.drop_with_heap(heap);
                append = value;
            }
            _ => {
                value.drop_with_heap(heap);
                record.drop_with_heap(heap);
                module.drop_with_heap(heap);
                action.drop_with_heap(heap);
                category.drop_with_heap(heap);
                lineno.drop_with_heap(heap);
                append.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(
                    "catch_warnings.__init__",
                    &key_text,
                ));
            }
        }
    }

    set_instance_attr_by_name(self_id, "_record", record, heap, interns)?;
    set_instance_attr_by_name(self_id, "_module", module, heap, interns)?;
    set_instance_attr_by_name(self_id, "_entered", Value::Bool(false), heap, interns)?;

    if matches!(action, Value::None) {
        action.drop_with_heap(heap);
        category.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        set_instance_attr_by_name(self_id, "_filter", Value::None, heap, interns)?;
    } else {
        let filter = allocate_tuple(smallvec::smallvec![action, category, lineno, append], heap)?;
        set_instance_attr_by_name(self_id, "_filter", filter, heap, interns)?;
    }

    Ok(Value::None)
}

/// Implements `catch_warnings.__enter__()`.
fn catch_warnings_enter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "catch_warnings.__enter__")?;
    defer_drop!(self_value, heap);
    method_args.check_zero_args("catch_warnings.__enter__", heap)?;

    let already_entered = if let Some(value) = get_instance_attr_by_name(self_id, "_entered", heap, interns) {
        let entered = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        entered
    } else {
        false
    };
    if already_entered {
        let repr_value = Value::Ref(self_id);
        heap.inc_ref(self_id);
        let repr = repr_value.py_repr(heap, interns).into_owned();
        repr_value.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::RuntimeError, format!("Cannot enter {repr} twice")).into());
    }

    set_instance_attr_by_name(self_id, "_entered", Value::Bool(true), heap, interns)?;

    let filters_id = get_filters_list_id(heap, interns)?;
    let snapshot = clone_list(filters_id, heap)?;
    set_instance_attr_by_name(self_id, "_filters", Value::Ref(snapshot), heap, interns)?;

    let record_enabled = if let Some(value) = get_instance_attr_by_name(self_id, "_record", heap, interns) {
        let enabled = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        enabled
    } else {
        false
    };

    let log_value = if record_enabled {
        let list_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
        push_record_list(list_id);
        set_instance_attr_by_name(self_id, "_log", Value::Ref(list_id), heap, interns)?;
        heap.inc_ref(list_id);
        Value::Ref(list_id)
    } else {
        Value::None
    };

    if let Some(filter_value) = get_instance_attr_by_name(self_id, "_filter", heap, interns)
        && !matches!(filter_value, Value::None)
    {
        let filter = filter_value;
        let values = tuple_to_vec(&filter, heap)?.unwrap_or_default();
        if values.len() == 4 {
            let action = values[0].clone_with_heap(heap);
            let category = values[1].clone_with_heap(heap);
            let lineno = values[2].clone_with_heap(heap);
            let append = values[3].clone_with_heap(heap);
            let method_args = ArgValues::ArgsKargs {
                args: vec![action, category, lineno, append],
                kwargs: KwargsValues::Empty,
            };
            simplefilter(heap, interns, method_args)?;
        }
        filter.drop_with_heap(heap);
    }

    Ok(log_value)
}

/// Implements `catch_warnings.__exit__(*exc_info)`.
fn catch_warnings_exit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "catch_warnings.__exit__")?;
    defer_drop!(self_value, heap);
    let (positional, kwargs) = method_args.into_parts();

    if positional.len() != 3 || !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("catch_warnings.__exit__", 3, 0));
    }
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let entered = if let Some(value) = get_instance_attr_by_name(self_id, "_entered", heap, interns) {
        let is_entered = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        is_entered
    } else {
        false
    };
    if !entered {
        let repr_value = Value::Ref(self_id);
        heap.inc_ref(self_id);
        let repr = repr_value.py_repr(heap, interns).into_owned();
        repr_value.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::RuntimeError,
            format!("Cannot exit {repr} without entering first"),
        )
        .into());
    }

    if let Some(saved_filters) = get_instance_attr_by_name(self_id, "_filters", heap, interns) {
        if let Value::Ref(saved_id) = &saved_filters {
            let cloned = clone_list(*saved_id, heap)?;
            set_filters_list_id(cloned, heap, interns)?;
        }
        saved_filters.drop_with_heap(heap);
    }

    let record_enabled = if let Some(value) = get_instance_attr_by_name(self_id, "_record", heap, interns) {
        let enabled = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        enabled
    } else {
        false
    };
    if record_enabled {
        let _ = pop_record_list();
    }

    bump_filters_version();
    Ok(Value::None)
}

/// Implements `catch_warnings.__repr__()`.
fn catch_warnings_repr(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "catch_warnings.__repr__")?;
    defer_drop!(self_value, heap);
    method_args.check_zero_args("catch_warnings.__repr__", heap)?;

    let mut parts = Vec::new();
    if let Some(value) = get_instance_attr_by_name(self_id, "_record", heap, interns) {
        let is_record = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
        if is_record {
            parts.push("record=True".to_string());
        }
    }

    let module_id = warnings_module_id()?;
    if let Some(module_value) = get_instance_attr_by_name(self_id, "_module", heap, interns) {
        let is_warnings_module = matches!(module_value, Value::Ref(id) if id == module_id);
        if !is_warnings_module {
            let module_repr = module_value.py_repr(heap, interns).into_owned();
            parts.push(format!("module={module_repr}"));
        }
        module_value.drop_with_heap(heap);
    }

    let repr = if parts.is_empty() {
        "catch_warnings()".to_string()
    } else {
        format!("catch_warnings({})", parts.join(", "))
    };

    let repr_id = heap.allocate(HeapData::Str(Str::from(repr)))?;
    Ok(Value::Ref(repr_id))
}

/// Implements `deprecated.__init__(message, *, category=DeprecationWarning, stacklevel=1)`.
fn deprecated_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "deprecated.__init__")?;
    defer_drop!(self_value, heap);
    let (positional_iter, kwargs) = method_args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            "deprecated.__init__",
            &["message"],
        ));
    }
    if positional.len() > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("deprecated.__init__", 2, 3, 0));
    }

    let message = positional.remove(0);
    if !value_is_string(&message, heap) {
        let type_name = message.py_type(heap).to_string();
        message.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("Expected an object of type str for 'message', not '{type_name}'"),
        )
        .into());
    }

    let mut category = Value::Builtin(Builtins::ExcType(ExcType::Exception));
    let mut stacklevel = Value::Int(1);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            stacklevel.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "category" => {
                category.drop_with_heap(heap);
                category = value;
            }
            "stacklevel" => {
                stacklevel.drop_with_heap(heap);
                stacklevel = value;
            }
            _ => {
                value.drop_with_heap(heap);
                message.drop_with_heap(heap);
                category.drop_with_heap(heap);
                stacklevel.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("deprecated.__init__", &key_text));
            }
        }
    }

    set_instance_attr_by_name(self_id, "message", message.clone_with_heap(heap), heap, interns)?;
    set_instance_attr_by_name(self_id, "category", category, heap, interns)?;
    set_instance_attr_by_name(self_id, "stacklevel", stacklevel, heap, interns)?;

    message.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `deprecated.__call__(arg)`.
fn deprecated_call(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "deprecated.__call__")?;
    defer_drop!(self_value, heap);
    let arg = method_args.get_one_arg("deprecated.__call__", heap)?;

    let category = get_instance_attr_by_name(self_id, "category", heap, interns).unwrap_or(Value::None);
    let message = get_instance_attr_by_name(self_id, "message", heap, interns).unwrap_or(Value::None);

    if matches!(category, Value::None) {
        set_value_deprecated_attr(&arg, &message, heap, interns)?;
        let result = arg.clone_with_heap(heap);
        arg.drop_with_heap(heap);
        category.drop_with_heap(heap);
        message.drop_with_heap(heap);
        return Ok(result);
    }

    if !is_callable_value(&arg, heap, interns)
        && !matches!(arg, Value::Ref(id) if matches!(heap.get(id), HeapData::ClassObject(_)))
    {
        let repr = arg.py_repr(heap, interns).into_owned();
        arg.drop_with_heap(heap);
        category.drop_with_heap(heap);
        message.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("@deprecated decorator with non-None category must be applied to a class or callable, not {repr}"),
        )
        .into());
    }

    // Ouros currently preserves decorator metadata but does not yet synthesize the
    // full wrapper behavior from CPython's `_py_warnings.deprecated` implementation.
    set_value_deprecated_attr(&arg, &message, heap, interns)?;
    let result = arg.clone_with_heap(heap);
    arg.drop_with_heap(heap);

    category.drop_with_heap(heap);
    message.drop_with_heap(heap);
    Ok(result)
}

/// Core implementation for `warn_explicit` once arguments have been normalized.
fn warn_explicit_impl(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    message: Value,
    category: Value,
    filename: String,
    lineno: i64,
    module: Option<String>,
    registry_id: Option<HeapId>,
    source: Value,
) -> RunResult<Value> {
    let module_name = module.unwrap_or_else(|| default_module_name_from_filename(&filename));

    let (text, warning_instance, warning_category) = normalize_warning_inputs(message, category, heap, interns)?;

    let filters_version = current_filters_version();
    if let Some(id) = registry_id {
        let current_version = registry_version(id, heap, interns);
        if current_version != filters_version {
            clear_dict(id, heap)?;
            set_registry_version(id, filters_version, heap, interns)?;
        }
    }

    let key = warning_key(&text, &warning_category, lineno, heap)?;
    if registry_contains_key(registry_id, &key, heap, interns)? {
        key.drop_with_heap(heap);
        warning_instance.drop_with_heap(heap);
        warning_category.drop_with_heap(heap);
        source.drop_with_heap(heap);
        return Ok(Value::None);
    }

    let action = match find_matching_filter_action(&text, &warning_category, &module_name, lineno, heap, interns)? {
        Some(action) => action,
        None => get_default_action(heap, interns)?,
    };

    match action.as_str() {
        "ignore" => {
            key.drop_with_heap(heap);
            warning_instance.drop_with_heap(heap);
            warning_category.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return Ok(Value::None);
        }
        "error" => {
            key.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return raise_warning_as_exception(warning_instance, &warning_category, heap, interns);
        }
        "once" => {
            set_registry_key_true(registry_id, key.clone_with_heap(heap), heap, interns)?;
            let oncekey = warning_once_key(&text, &warning_category, heap)?;
            if onceregistry_contains_key(&oncekey, heap, interns)? {
                oncekey.drop_with_heap(heap);
                key.drop_with_heap(heap);
                warning_instance.drop_with_heap(heap);
                warning_category.drop_with_heap(heap);
                source.drop_with_heap(heap);
                return Ok(Value::None);
            }
            set_onceregistry_key_true(oncekey, heap, interns)?;
        }
        "module" => {
            set_registry_key_true(registry_id, key.clone_with_heap(heap), heap, interns)?;
            let altkey = warning_key(&text, &warning_category, 0, heap)?;
            if registry_contains_key(registry_id, &altkey, heap, interns)? {
                altkey.drop_with_heap(heap);
                key.drop_with_heap(heap);
                warning_instance.drop_with_heap(heap);
                warning_category.drop_with_heap(heap);
                source.drop_with_heap(heap);
                return Ok(Value::None);
            }
            set_registry_key_true(registry_id, altkey, heap, interns)?;
        }
        "default" => {
            set_registry_key_true(registry_id, key.clone_with_heap(heap), heap, interns)?;
        }
        "always" | "all" => {}
        _ => {
            let runtime_error = SimpleException::new_msg(
                ExcType::RuntimeError,
                format!("Unrecognized action ({action:?}) in warnings.filters"),
            );
            key.drop_with_heap(heap);
            warning_instance.drop_with_heap(heap);
            warning_category.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return Err(runtime_error.into());
        }
    }

    key.drop_with_heap(heap);

    emit_warning_message(
        warning_instance,
        warning_category,
        filename,
        lineno,
        None,
        None,
        Some(source),
        heap,
        interns,
    )
}

/// Emits a warning either by recording it in `catch_warnings(record=True)` or by printing it.
fn emit_warning_message(
    message: Value,
    category: Value,
    filename: String,
    lineno: i64,
    file: Option<Value>,
    line: Option<Value>,
    source: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    defer_drop!(message, heap);
    defer_drop!(category, heap);
    defer_drop!(file, heap);
    defer_drop!(line, heap);
    defer_drop!(source, heap);

    let warning_value = build_warning_message_instance(
        message.clone_with_heap(heap),
        category.clone_with_heap(heap),
        filename.clone(),
        lineno,
        file.as_ref().map(|value| value.clone_with_heap(heap)),
        line.as_ref().map(|value| value.clone_with_heap(heap)),
        source.as_ref().map(|value| value.clone_with_heap(heap)),
        heap,
        interns,
    )?;
    let mut warning_value_guard = HeapGuard::new(warning_value, heap);
    let (warning_value, heap) = warning_value_guard.as_parts();

    if let Some(log_list_id) = current_record_list() {
        let contains_ref = matches!(warning_value, Value::Ref(_));
        let (warning_value, heap) = warning_value_guard.into_parts();
        let HeapData::List(list) = heap.get_mut(log_list_id) else {
            warning_value.drop_with_heap(heap);
            return Err(ExcType::type_error("warnings record log must be a list"));
        };
        list.as_vec_mut().push(warning_value);
        if contains_ref {
            list.set_contains_refs();
            heap.mark_potential_cycle();
        }
        return Ok(Value::None);
    }

    let lineno_value = Value::Int(lineno);
    let filename_value = Value::Ref(heap.allocate(HeapData::Str(Str::from(filename)))?);
    defer_drop!(filename_value, heap);
    let message_value = message_from_warning_message(warning_value, heap, interns);
    defer_drop!(message_value, heap);
    let text = format_warning_text(
        heap,
        interns,
        message_value,
        category,
        filename_value,
        &lineno_value,
        line.as_ref(),
        source.as_ref(),
    )?;

    if let Some(file) = file.as_ref() {
        write_warning_to_file(file, &text, heap, interns)?;
    } else {
        eprint!("{text}");
    }

    Ok(Value::None)
}

/// Creates a `WarningMessage` instance value.
fn build_warning_message_instance(
    message: Value,
    category: Value,
    filename: String,
    lineno: i64,
    file: Option<Value>,
    line: Option<Value>,
    source: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    defer_drop!(message, heap);
    defer_drop!(category, heap);
    defer_drop!(file, heap);
    defer_drop!(line, heap);
    defer_drop!(source, heap);

    let warning_message_class = get_module_attr("WarningMessage", heap, interns)?;
    defer_drop!(warning_message_class, heap);
    let class_id = if let Value::Ref(id) = warning_message_class {
        if matches!(heap.get(*id), HeapData::ClassObject(_)) {
            *id
        } else {
            return Err(ExcType::type_error("warnings.WarningMessage must be a class"));
        }
    } else {
        return Err(ExcType::type_error("warnings.WarningMessage must be a class"));
    };

    let instance_id = allocate_instance_of_class(class_id, heap)?;
    let filename_id = heap.allocate(HeapData::Str(Str::from(filename)))?;

    set_instance_attr_by_name(instance_id, "message", message.clone_with_heap(heap), heap, interns)?;
    set_instance_attr_by_name(instance_id, "category", category.clone_with_heap(heap), heap, interns)?;
    set_instance_attr_by_name(instance_id, "filename", Value::Ref(filename_id), heap, interns)?;
    set_instance_attr_by_name(instance_id, "lineno", Value::Int(lineno), heap, interns)?;
    set_instance_attr_by_name(
        instance_id,
        "file",
        file.as_ref().map_or(Value::None, |value| value.clone_with_heap(heap)),
        heap,
        interns,
    )?;
    set_instance_attr_by_name(
        instance_id,
        "line",
        line.as_ref().map_or(Value::None, |value| value.clone_with_heap(heap)),
        heap,
        interns,
    )?;
    set_instance_attr_by_name(
        instance_id,
        "source",
        source.as_ref().map_or(Value::None, |value| value.clone_with_heap(heap)),
        heap,
        interns,
    )?;

    let category_name_value = if matches!(category, Value::None) {
        Value::None
    } else {
        let name = category_name(category, heap, interns)?;
        let id = heap.allocate(HeapData::Str(Str::from(name)))?;
        Value::Ref(id)
    };
    set_instance_attr_by_name(instance_id, "_category_name", category_name_value, heap, interns)?;
    Ok(Value::Ref(instance_id))
}

/// Extracts `message` from a `WarningMessage` object when available.
fn message_from_warning_message(message_obj: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Value {
    if let Value::Ref(id) = message_obj
        && let HeapData::Instance(instance) = heap.get(*id)
        && let Some(attrs) = instance.attrs(heap)
        && let Some(value) = attrs.get_by_str("message", heap, interns)
    {
        return value.clone_with_heap(heap);
    }
    message_obj.clone_with_heap(heap)
}

/// Formats warning text in CPython's default shape.
fn format_warning_text(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    message: &Value,
    category: &Value,
    filename: &Value,
    lineno: &Value,
    line: Option<&Value>,
    _source: Option<&Value>,
) -> RunResult<String> {
    let category_text = category_name(category, heap, interns)?;
    let filename_text = filename.py_str(heap, interns).into_owned();
    let lineno_text = lineno.py_str(heap, interns).into_owned();
    let message_text = message.py_str(heap, interns).into_owned();

    let mut text = format!("{filename_text}:{lineno_text}: {category_text}: {message_text}\n");

    if let Some(line_value) = line
        && !matches!(line_value, Value::None)
    {
        let stripped = line_value.py_str(heap, interns).trim().to_string();
        if !stripped.is_empty() {
            text.push_str("  ");
            text.push_str(stripped.as_str());
            text.push('\n');
        }
    }

    Ok(text)
}

/// Writes warning text through a file-like `write(str)` API.
fn write_warning_to_file(
    file: &Value,
    text: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    let mut file_value = file.clone_with_heap(heap);
    let write_result = file_value.py_call_attr(
        heap,
        &EitherStr::Heap("write".to_string()),
        ArgValues::One(Value::Ref(text_id)),
        interns,
        None,
    );
    file_value.drop_with_heap(heap);

    match write_result {
        Ok(value) => {
            value.drop_with_heap(heap);
            Ok(())
        }
        Err(err) => {
            if let RunError::Exc(exc) = &err
                && exc.exc.exc_type().is_subclass_of(ExcType::OSError)
            {
                Ok(())
            } else {
                Err(err)
            }
        }
    }
}

/// Returns the category's `__name__` string.
fn category_name(category: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match category {
        Value::Builtin(Builtins::ExcType(exc)) => Ok(exc.to_string()),
        Value::Builtin(Builtins::Type(Type::Exception(exc))) => Ok(exc.to_string()),
        _ => {
            let name = category.py_getattr(StaticStrings::DunderName.into(), heap, interns)?;
            let name_value = match name {
                AttrCallResult::Value(value) | AttrCallResult::DescriptorGet(value) => value,
                _ => {
                    return Err(ExcType::attribute_error(category.py_type(heap), "__name__"));
                }
            };
            let text = name_value.py_str(heap, interns).into_owned();
            name_value.drop_with_heap(heap);
            Ok(text)
        }
    }
}

/// Normalizes `warn` / `warn_explicit` message and category arguments.
fn normalize_warning_inputs(
    message: Value,
    category: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, Value, Value)> {
    if matches!(message, Value::Ref(id) if matches!(heap.get(id), HeapData::Exception(_))) {
        let text = message.py_str(heap, interns).into_owned();
        category.drop_with_heap(heap);
        return Ok((text, message, Value::Builtin(Builtins::ExcType(ExcType::Exception))));
    }

    let text = message.py_str(heap, interns).into_owned();

    let warning_instance = match &category {
        Value::Builtin(Builtins::ExcType(exc)) => {
            let exception = SimpleException::new_msg(*exc, text.clone());
            let id = heap.allocate(HeapData::Exception(exception))?;
            Value::Ref(id)
        }
        Value::Builtin(Builtins::Type(Type::Exception(exc))) => {
            let exception = SimpleException::new_msg(*exc, text.clone());
            let id = heap.allocate(HeapData::Exception(exception))?;
            Value::Ref(id)
        }
        _ => {
            let mut category_callable = category.clone_with_heap(heap);
            let text_id = heap.allocate(HeapData::Str(Str::from(text.as_str())))?;
            let result = category_callable.py_call_attr(
                heap,
                &EitherStr::Heap("__call__".to_string()),
                ArgValues::One(Value::Ref(text_id)),
                interns,
                None,
            )?;
            category_callable.drop_with_heap(heap);
            result
        }
    };

    message.drop_with_heap(heap);
    Ok((text, warning_instance, category))
}

/// Raises a warning instance as an exception.
fn raise_warning_as_exception(
    warning_instance: Value,
    category: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match warning_instance {
        Value::Ref(id) => {
            if let HeapData::Exception(exc) = heap.get(id) {
                Err(exc.clone().into())
            } else {
                let text = warning_instance.py_str(heap, interns).into_owned();
                warning_instance.drop_with_heap(heap);
                if let Value::Builtin(Builtins::ExcType(exc_type)) = category {
                    Err(SimpleException::new_msg(*exc_type, text).into())
                } else if let Value::Builtin(Builtins::Type(Type::Exception(exc_type))) = category {
                    Err(SimpleException::new_msg(*exc_type, text).into())
                } else {
                    Err(SimpleException::new_msg(ExcType::Exception, text).into())
                }
            }
        }
        other => {
            let text = other.py_str(heap, interns).into_owned();
            other.drop_with_heap(heap);
            Err(SimpleException::new_msg(ExcType::Exception, text).into())
        }
    }
}

/// Finds the first matching filter action.
fn find_matching_filter_action(
    text: &str,
    category: &Value,
    module: &str,
    lineno: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<String>> {
    let filters_id = get_filters_list_id(heap, interns)?;
    let filter_values = clone_list_values(filters_id, heap)?;

    for filter_value in filter_values {
        let Some(parts) = tuple_to_vec(&filter_value, heap)? else {
            filter_value.drop_with_heap(heap);
            continue;
        };
        if parts.len() != 5 {
            filter_value.drop_with_heap(heap);
            continue;
        }

        let action_text = parts[0].py_str(heap, interns).into_owned();
        let message_matcher = &parts[1];
        let category_matcher = &parts[2];
        let module_matcher = &parts[3];
        let lineno_match = parts[4].as_int(heap).unwrap_or(0);

        let message_ok = matcher_matches(message_matcher, text, true, heap, interns)?;
        let category_ok = is_subclass_for_filter(category, category_matcher, heap, interns)?;
        let module_ok = matcher_matches(module_matcher, module, false, heap, interns)?;
        let lineno_ok = lineno_match == 0 || lineno == lineno_match;

        parts.drop_with_heap(heap);
        filter_value.drop_with_heap(heap);

        if message_ok && category_ok && module_ok && lineno_ok {
            return Ok(Some(action_text));
        }
    }

    Ok(None)
}

/// Returns the module's current default action.
fn get_default_action(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let value = get_module_attr("defaultaction", heap, interns)?;
    let text = value.py_str(heap, interns).into_owned();
    value.drop_with_heap(heap);
    Ok(text)
}

/// Adds one filter tuple to `warnings.filters` with CPython duplicate semantics.
fn add_filter_item(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    action: String,
    message: Option<String>,
    category: Value,
    module: Option<String>,
    lineno: i64,
    append: bool,
) -> RunResult<()> {
    let filters_id = get_filters_list_id(heap, interns)?;

    let action_id = heap.allocate(HeapData::Str(Str::from(action.as_str())))?;
    let message_value = match message {
        Some(pattern) => Value::Ref(heap.allocate(HeapData::Str(Str::from(pattern)))?),
        None => Value::None,
    };
    let module_value = match module {
        Some(pattern) => Value::Ref(heap.allocate(HeapData::Str(Str::from(pattern)))?),
        None => Value::None,
    };

    let filter_tuple = allocate_tuple(
        smallvec::smallvec![
            Value::Ref(action_id),
            message_value,
            category,
            module_value,
            Value::Int(lineno)
        ],
        heap,
    )?;

    let duplicate_index = find_list_item_index(filters_id, &filter_tuple, heap, interns)?;

    heap.with_entry_mut(filters_id, move |heap_inner, data| {
        let HeapData::List(list) = data else {
            filter_tuple.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("warnings.filters must be a list"));
        };
        if append {
            if duplicate_index.is_none() {
                list.append(heap_inner, filter_tuple);
            } else {
                filter_tuple.drop_with_heap(heap_inner);
            }
        } else {
            if let Some(index) = duplicate_index {
                let removed = list.as_vec_mut().remove(index);
                removed.drop_with_heap(heap_inner);
            }
            list.insert(heap_inner, 0, filter_tuple);
        }
        Ok(())
    })?;

    bump_filters_version();
    Ok(())
}

/// Parses arguments for `filterwarnings(...)`.
fn parse_filterwarnings_args(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, String, Value, String, i64, bool)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            "filterwarnings",
            &["action"],
        ));
    }
    if positional.len() > 6 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("filterwarnings", 6, 7, 0));
    }

    let action = positional.remove(0);
    let mut message = if positional.is_empty() {
        Value::InternString(StaticStrings::EmptyString.into())
    } else {
        positional.remove(0)
    };
    let mut category = if positional.is_empty() {
        warning_base_class_value()
    } else {
        positional.remove(0)
    };
    let mut module = if positional.is_empty() {
        Value::InternString(StaticStrings::EmptyString.into())
    } else {
        positional.remove(0)
    };
    let mut lineno = if positional.is_empty() {
        Value::Int(0)
    } else {
        positional.remove(0)
    };
    let mut append = if positional.is_empty() {
        Value::Bool(false)
    } else {
        positional.remove(0)
    };

    let mut saw_message_pos = !matches!(message, Value::InternString(id) if id == StaticStrings::EmptyString);
    let mut saw_category_pos = !matches!(category, Value::Builtin(Builtins::ExcType(ExcType::Exception)));
    let mut saw_module_pos = !matches!(module, Value::InternString(id) if id == StaticStrings::EmptyString);
    let mut saw_lineno_pos = !matches!(lineno, Value::Int(0));
    let mut saw_append_pos = !matches!(append, Value::Bool(false));

    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            action.drop_with_heap(heap);
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            module.drop_with_heap(heap);
            lineno.drop_with_heap(heap);
            append.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "message" => {
                if saw_message_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("filterwarnings", "message"));
                }
                saw_message_pos = true;
                message.drop_with_heap(heap);
                message = value;
            }
            "category" => {
                if saw_category_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("filterwarnings", "category"));
                }
                saw_category_pos = true;
                category.drop_with_heap(heap);
                category = value;
            }
            "module" => {
                if saw_module_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("filterwarnings", "module"));
                }
                saw_module_pos = true;
                module.drop_with_heap(heap);
                module = value;
            }
            "lineno" => {
                if saw_lineno_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("filterwarnings", "lineno"));
                }
                saw_lineno_pos = true;
                lineno.drop_with_heap(heap);
                lineno = value;
            }
            "append" => {
                if saw_append_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    message.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    module.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("filterwarnings", "append"));
                }
                saw_append_pos = true;
                append.drop_with_heap(heap);
                append = value;
            }
            _ => {
                value.drop_with_heap(heap);
                action.drop_with_heap(heap);
                message.drop_with_heap(heap);
                category.drop_with_heap(heap);
                module.drop_with_heap(heap);
                lineno.drop_with_heap(heap);
                append.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("filterwarnings", &key_text));
            }
        }
    }

    let action_string = parse_action_value(&action, heap, interns)?;
    action.drop_with_heap(heap);

    let message_string = if value_is_string(&message, heap) {
        message.py_str(heap, interns).into_owned()
    } else {
        message.drop_with_heap(heap);
        category.drop_with_heap(heap);
        module.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "message must be a string").into());
    };
    message.drop_with_heap(heap);

    if !is_warning_subclass(&category, heap, interns)? {
        category.drop_with_heap(heap);
        module.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "category must be a Warning subclass").into());
    }

    let module_string = if value_is_string(&module, heap) {
        module.py_str(heap, interns).into_owned()
    } else {
        category.drop_with_heap(heap);
        module.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "module must be a string").into());
    };
    module.drop_with_heap(heap);

    let lineno_int = if let Ok(value) = lineno.as_int(heap) {
        value
    } else {
        category.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "lineno must be an int").into());
    };
    lineno.drop_with_heap(heap);
    if lineno_int < 0 {
        category.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "lineno must be an int >= 0").into());
    }

    let append_bool = append.py_bool(heap, interns);
    append.drop_with_heap(heap);

    Ok((
        action_string,
        message_string,
        category,
        module_string,
        lineno_int,
        append_bool,
    ))
}

/// Parses arguments for `simplefilter(...)`.
fn parse_simplefilter_args(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, Value, i64, bool)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            "simplefilter",
            &["action"],
        ));
    }
    if positional.len() > 4 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional("simplefilter", 4, 5, 0));
    }

    let action = positional.remove(0);
    let mut category = if positional.is_empty() {
        warning_base_class_value()
    } else {
        positional.remove(0)
    };
    let mut lineno = if positional.is_empty() {
        Value::Int(0)
    } else {
        positional.remove(0)
    };
    let mut append = if positional.is_empty() {
        Value::Bool(false)
    } else {
        positional.remove(0)
    };

    let mut saw_category_pos = !matches!(category, Value::Builtin(Builtins::ExcType(ExcType::Exception)));
    let mut saw_lineno_pos = !matches!(lineno, Value::Int(0));
    let mut saw_append_pos = !matches!(append, Value::Bool(false));

    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            action.drop_with_heap(heap);
            category.drop_with_heap(heap);
            lineno.drop_with_heap(heap);
            append.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "category" => {
                if saw_category_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("simplefilter", "category"));
                }
                saw_category_pos = true;
                category.drop_with_heap(heap);
                category = value;
            }
            "lineno" => {
                if saw_lineno_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("simplefilter", "lineno"));
                }
                saw_lineno_pos = true;
                lineno.drop_with_heap(heap);
                lineno = value;
            }
            "append" => {
                if saw_append_pos {
                    value.drop_with_heap(heap);
                    action.drop_with_heap(heap);
                    category.drop_with_heap(heap);
                    lineno.drop_with_heap(heap);
                    append.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("simplefilter", "append"));
                }
                saw_append_pos = true;
                append.drop_with_heap(heap);
                append = value;
            }
            _ => {
                value.drop_with_heap(heap);
                action.drop_with_heap(heap);
                category.drop_with_heap(heap);
                lineno.drop_with_heap(heap);
                append.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("simplefilter", &key_text));
            }
        }
    }

    let action_string = parse_action_value(&action, heap, interns)?;
    action.drop_with_heap(heap);

    if !is_warning_subclass(&category, heap, interns)? {
        category.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "category must be a Warning subclass").into());
    }

    let lineno_int = if let Ok(value) = lineno.as_int(heap) {
        value
    } else {
        category.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "lineno must be an int").into());
    };
    lineno.drop_with_heap(heap);
    if lineno_int < 0 {
        category.drop_with_heap(heap);
        append.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "lineno must be an int >= 0").into());
    }

    let append_bool = append.py_bool(heap, interns);
    append.drop_with_heap(heap);

    Ok((action_string, category, lineno_int, append_bool))
}

/// Converts an action value to a validated action string.
fn parse_action_value(action: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    if let Some(action_text) = value_as_exact_str(action, heap, interns)
        && VALID_ACTIONS.contains(&action_text.as_str())
    {
        return Ok(action_text);
    }

    let repr = action.py_repr(heap, interns).into_owned();
    Err(SimpleException::new_msg(ExcType::ValueError, format!("invalid action: {repr}")).into())
}

/// Parses `(message, category, filename, lineno, file, line, source)` style signatures.
fn parse_warning_message_like_args(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    function_name: &str,
    allow_file: bool,
    allow_source: bool,
) -> RunResult<(Value, Value, Value, Value, Option<Value>, Option<Value>, Option<Value>)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();

    if positional.len() < 4 {
        let missing = ["message", "category", "filename", "lineno"];
        let missing_slice = &missing[positional.len()..];
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            missing_slice,
        ));
    }

    let message = positional.remove(0);
    let category = positional.remove(0);
    let filename = positional.remove(0);
    let lineno = positional.remove(0);

    let mut file = if allow_file && !positional.is_empty() {
        Some(positional.remove(0))
    } else {
        None
    };
    let mut line = if positional.is_empty() {
        None
    } else {
        Some(positional.remove(0))
    };
    let mut source = if allow_source && !positional.is_empty() {
        Some(positional.remove(0))
    } else {
        None
    };

    if !positional.is_empty() {
        let actual = 4 + usize::from(file.is_some()) + usize::from(line.is_some()) + usize::from(source.is_some()) + 1;
        message.drop_with_heap(heap);
        category.drop_with_heap(heap);
        filename.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        file.drop_with_heap(heap);
        line.drop_with_heap(heap);
        source.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(function_name, 7, actual, 0));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            message.drop_with_heap(heap);
            category.drop_with_heap(heap);
            filename.drop_with_heap(heap);
            lineno.drop_with_heap(heap);
            file.drop_with_heap(heap);
            line.drop_with_heap(heap);
            source.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_text.as_str() {
            "file" if allow_file => {
                file.drop_with_heap(heap);
                file = Some(value);
            }
            "line" => {
                line.drop_with_heap(heap);
                line = Some(value);
            }
            "source" if allow_source => {
                source.drop_with_heap(heap);
                source = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                message.drop_with_heap(heap);
                category.drop_with_heap(heap);
                filename.drop_with_heap(heap);
                lineno.drop_with_heap(heap);
                file.drop_with_heap(heap);
                line.drop_with_heap(heap);
                source.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(function_name, &key_text));
            }
        }
    }

    Ok((message, category, filename, lineno, file, line, source))
}

/// Initializes CPython-like default warning filters.
fn initialize_default_filters(filters_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> Result<(), ResourceError> {
    let dep = warning_class_value("DeprecationWarning");
    let pending_dep = warning_class_value("PendingDeprecationWarning");
    let import = warning_class_value("ImportWarning");
    let resource = warning_class_value("ResourceWarning");

    let module_main = Value::Ref(heap.allocate(HeapData::Str(Str::from("__main__")))?);

    let filter1 = allocate_tuple(
        smallvec::smallvec![
            Value::Ref(heap.allocate(HeapData::Str(Str::from("default")))?),
            Value::None,
            dep.clone_with_heap(heap),
            module_main,
            Value::Int(0)
        ],
        heap,
    )?;
    let filter2 = allocate_tuple(
        smallvec::smallvec![
            Value::Ref(heap.allocate(HeapData::Str(Str::from("ignore")))?),
            Value::None,
            dep,
            Value::None,
            Value::Int(0)
        ],
        heap,
    )?;
    let filter3 = allocate_tuple(
        smallvec::smallvec![
            Value::Ref(heap.allocate(HeapData::Str(Str::from("ignore")))?),
            Value::None,
            pending_dep,
            Value::None,
            Value::Int(0)
        ],
        heap,
    )?;
    let filter4 = allocate_tuple(
        smallvec::smallvec![
            Value::Ref(heap.allocate(HeapData::Str(Str::from("ignore")))?),
            Value::None,
            import,
            Value::None,
            Value::Int(0)
        ],
        heap,
    )?;
    let filter5 = allocate_tuple(
        smallvec::smallvec![
            Value::Ref(heap.allocate(HeapData::Str(Str::from("ignore")))?),
            Value::None,
            resource,
            Value::None,
            Value::Int(0)
        ],
        heap,
    )?;

    if let HeapData::List(list) = heap.get_mut(filters_id) {
        list.as_vec_mut().extend([filter1, filter2, filter3, filter4, filter5]);
        list.set_contains_refs();
    } else {
        filter1.drop_with_heap(heap);
        filter2.drop_with_heap(heap);
        filter3.drop_with_heap(heap);
        filter4.drop_with_heap(heap);
        filter5.drop_with_heap(heap);
    }

    Ok(())
}

/// Creates the `WarningMessage` class.
fn create_warning_message_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::WarningMessageInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "__str__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::WarningMessageStr)),
        heap,
        interns,
    )?;

    let mut details: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
    for name in ["message", "category", "filename", "lineno", "file", "line", "source"] {
        let id = heap.allocate(HeapData::Str(Str::from(name)))?;
        details.push(Value::Ref(id));
    }
    let details_tuple = allocate_tuple(details, heap)?;
    dict_set_str_attr(&mut attrs, "_WARNING_DETAILS", details_tuple, heap, interns)?;

    create_helper_class("_py_warnings.WarningMessage", attrs, heap, interns)
}

/// Creates the `catch_warnings` class.
fn create_catch_warnings_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::CatchWarningsInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "__enter__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::CatchWarningsEnter)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "__exit__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::CatchWarningsExit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "__repr__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::CatchWarningsRepr)),
        heap,
        interns,
    )?;

    create_helper_class("_py_warnings.catch_warnings", attrs, heap, interns)
}

/// Creates the `deprecated` decorator class.
fn create_deprecated_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::DeprecatedInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "__call__",
        Value::ModuleFunction(ModuleFunctions::Warnings(WarningsFunctions::DeprecatedCall)),
        heap,
        interns,
    )?;

    create_helper_class("_py_warnings.deprecated", attrs, heap, interns)
}

/// Allocates a simple helper class inheriting from `object`.
fn create_helper_class(
    class_name: &str,
    attrs: Dict,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let object_id = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_id);

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap(class_name.to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        attrs,
        vec![object_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[object_id], heap, interns).expect("warnings helper class MRO should be valid");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(class_obj) = heap.get_mut(class_id) {
        class_obj.set_mro(mro);
    }

    heap.with_entry_mut(object_id, |_, data| {
        let HeapData::ClassObject(base_cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        base_cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("warnings helper class registration should succeed");

    Ok(class_id)
}

/// Extracts `self` from an instance method call.
fn extract_instance_self_and_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    method_name: &str,
) -> RunResult<(HeapId, Value, ArgValues)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 1, 0));
    }

    let mut self_value_guard = HeapGuard::new(positional.remove(0), heap);
    let (self_value, heap) = self_value_guard.as_parts_mut();
    let self_id = match self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("{method_name} expected instance")));
        }
    };
    let self_value = self_value_guard.into_inner();

    Ok((self_id, self_value, arg_values_from_parts(positional, kwargs)))
}

/// Rebuilds `ArgValues` from positional and keyword parts.
fn arg_values_from_parts(positional: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    if kwargs.is_empty() {
        match positional.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(positional.into_iter().next().expect("checked length")),
            2 => {
                let mut iter = positional.into_iter();
                ArgValues::Two(
                    iter.next().expect("checked length"),
                    iter.next().expect("checked length"),
                )
            }
            _ => ArgValues::ArgsKargs {
                args: positional,
                kwargs,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        }
    }
}

/// Allocates an instance for a class id.
fn allocate_instance_of_class(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let (slot_len, has_dict) = match heap.get(class_id) {
        HeapData::ClassObject(class_obj) => (class_obj.slot_layout().len(), class_obj.instance_has_dict()),
        _ => return Err(ExcType::type_error("warnings helper class is not a class object")),
    };

    let attrs_id = if has_dict {
        Some(heap.allocate(HeapData::Dict(Dict::new()))?)
    } else {
        None
    };

    let mut slot_values = Vec::with_capacity(slot_len);
    slot_values.resize_with(slot_len, || Value::Undefined);

    heap.inc_ref(class_id);
    let instance = Instance::new(class_id, attrs_id, slot_values, Vec::new());
    let instance_id = heap.allocate(HeapData::Instance(instance))?;
    Ok(instance_id)
}

/// Sets an instance attribute by string key.
fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    heap.with_entry_mut(instance_id, |heap_inner, data| {
        let HeapData::Instance(instance) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("warnings helper expected instance"));
        };

        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }

        Ok(())
    })
}

/// Gets an instance attribute by string key.
fn get_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };

    instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str(name, heap, interns))
        .map(|value| value.clone_with_heap(heap))
}

/// Returns whether a value is exact string-like storage.
fn value_is_string(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(value, Value::InternString(_))
        || matches!(value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Str(_)))
}

/// Returns an exact string if the value is a Python string object.
fn value_as_exact_str(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
    match value {
        Value::InternString(id) => Some(interns.get_str(*id).to_string()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str().to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Returns whether a value is tuple-like.
fn value_is_tuple(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Tuple(_)))
}

/// Converts tuple-like values to owned vectors.
fn tuple_to_vec(value: &Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Vec<Value>>> {
    let Value::Ref(id) = value else {
        return Ok(None);
    };

    match heap.get(*id) {
        HeapData::Tuple(tuple) => Ok(Some(tuple.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect())),
        HeapData::List(list) => Ok(Some(list.as_vec().iter().map(|v| v.clone_with_heap(heap)).collect())),
        _ => Ok(None),
    }
}

/// Clones a list object.
fn clone_list(list_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let HeapData::List(list) = heap.get(list_id) else {
        return Err(ExcType::type_error("expected list"));
    };

    let cloned = list.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect();
    let id = heap.allocate(HeapData::List(List::new(cloned)))?;
    Ok(id)
}

/// Clones all values in a list.
fn clone_list_values(list_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Vec<Value>> {
    let HeapData::List(list) = heap.get(list_id) else {
        return Err(ExcType::type_error("expected list"));
    };

    Ok(list.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect())
}

/// Finds an item index in a list using Python equality.
fn find_list_item_index(
    list_id: HeapId,
    needle: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<usize>> {
    let values = clone_list_values(list_id, heap)?;
    for (idx, value) in values.iter().enumerate() {
        if value.py_eq(needle, heap, interns) {
            values.drop_with_heap(heap);
            return Ok(Some(idx));
        }
    }
    values.drop_with_heap(heap);
    Ok(None)
}

/// Tests whether a filter matcher value matches `text`.
fn matcher_matches(
    matcher: &Value,
    text: &str,
    case_insensitive: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    if matches!(matcher, Value::None) {
        return Ok(true);
    }

    if let Some(pattern) = value_as_exact_str(matcher, heap, interns) {
        let regex_text = if case_insensitive {
            format!("(?i)^(?:{pattern})")
        } else {
            format!("^(?:{pattern})")
        };
        let regex = Regex::new(regex_text.as_str()).map_err(|_| {
            RunError::from(SimpleException::new_msg(
                ExcType::ValueError,
                "invalid warning filter pattern",
            ))
        })?;
        return Ok(regex.is_match(text).unwrap_or(false));
    }

    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    let mut matcher_value = matcher.clone_with_heap(heap);
    let result = matcher_value.py_call_attr(
        heap,
        &EitherStr::Heap("match".to_string()),
        ArgValues::One(Value::Ref(text_id)),
        interns,
        None,
    );
    matcher_value.drop_with_heap(heap);

    match result {
        Ok(value) => {
            let matched = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
            Ok(matched)
        }
        Err(err) => Err(err),
    }
}

/// Checks `issubclass(category, filter_category)` for warning filters.
fn is_subclass_for_filter(
    category: &Value,
    filter_category: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
) -> RunResult<bool> {
    match (category, filter_category) {
        (Value::Builtin(Builtins::ExcType(left)), Value::Builtin(Builtins::ExcType(right))) => {
            Ok(left.is_subclass_of(*right))
        }
        (Value::Builtin(Builtins::Type(Type::Exception(left))), Value::Builtin(Builtins::ExcType(right))) => {
            Ok(left.is_subclass_of(*right))
        }
        (Value::Builtin(Builtins::ExcType(left)), Value::Builtin(Builtins::Type(Type::Exception(right)))) => {
            Ok(left.is_subclass_of(*right))
        }
        (
            Value::Builtin(Builtins::Type(Type::Exception(left))),
            Value::Builtin(Builtins::Type(Type::Exception(right))),
        ) => Ok(left.is_subclass_of(*right)),
        (Value::Ref(left_id), Value::Ref(right_id))
            if matches!(heap.get(*left_id), HeapData::ClassObject(_))
                && matches!(heap.get(*right_id), HeapData::ClassObject(_)) =>
        {
            let HeapData::ClassObject(left_cls) = heap.get(*left_id) else {
                return Ok(false);
            };
            Ok(left_cls.mro().contains(right_id))
        }
        (Value::Ref(left_id), Value::Builtin(Builtins::ExcType(right)))
            if matches!(heap.get(*left_id), HeapData::ClassObject(_)) =>
        {
            let right_id = heap.builtin_class_id(Type::Exception(*right))?;
            let HeapData::ClassObject(left_cls) = heap.get(*left_id) else {
                return Ok(false);
            };
            Ok(left_cls.mro().contains(&right_id))
        }
        (Value::Ref(left_id), Value::Builtin(Builtins::Type(Type::Exception(right))))
            if matches!(heap.get(*left_id), HeapData::ClassObject(_)) =>
        {
            let right_id = heap.builtin_class_id(Type::Exception(*right))?;
            let HeapData::ClassObject(left_cls) = heap.get(*left_id) else {
                return Ok(false);
            };
            Ok(left_cls.mro().contains(&right_id))
        }
        _ => Ok(false),
    }
}

/// Returns true for values that represent warning classes.
fn is_warning_subclass(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
    match value {
        Value::Builtin(Builtins::ExcType(_) | Builtins::Type(Type::Exception(_))) => Ok(true),
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => {
            let exception_id = heap.builtin_class_id(Type::Exception(ExcType::Exception))?;
            let HeapData::ClassObject(class_obj) = heap.get(*id) else {
                return Ok(false);
            };
            Ok(class_obj.mro().contains(&exception_id) || class_obj.name(interns) == "Warning")
        }
        _ => Ok(false),
    }
}

/// Returns a base warning class value.
fn warning_base_class_value() -> Value {
    Builtins::from_str("Warning")
        .map(Value::Builtin)
        .unwrap_or(Value::Builtin(Builtins::ExcType(ExcType::Exception)))
}

/// Returns a warning-class builtin by name, falling back to `Warning`.
fn warning_class_value(name: &str) -> Value {
    Builtins::from_str(name).map_or_else(|()| warning_base_class_value(), Value::Builtin)
}

/// Builds a `(text, category, lineno)` warning key tuple.
fn warning_key(text: &str, category: &Value, lineno: i64, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(allocate_tuple(
        smallvec::smallvec![Value::Ref(text_id), category.clone_with_heap(heap), Value::Int(lineno)],
        heap,
    )?)
}

/// Builds a `(text, category)` warning key tuple used by `once` logic.
fn warning_once_key(text: &str, category: &Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(allocate_tuple(
        smallvec::smallvec![Value::Ref(text_id), category.clone_with_heap(heap)],
        heap,
    )?)
}

/// Returns the registry dict id used by `warn()` fallback.
fn get_runtime_registry_id(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let value = get_module_attr("_ouros_registry", heap, interns)?;
    if let Value::Ref(id_ref) = &value {
        let id = *id_ref;
        if matches!(heap.get(id), HeapData::Dict(_)) {
            value.drop_with_heap(heap);
            Ok(id)
        } else {
            value.drop_with_heap(heap);
            Err(ExcType::type_error("warnings runtime registry must be a dict"))
        }
    } else {
        value.drop_with_heap(heap);
        Err(ExcType::type_error("warnings runtime registry must be a dict"))
    }
}

/// Reads registry version from a dict.
fn registry_version(dict_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> i64 {
    let HeapData::Dict(dict) = heap.get(dict_id) else {
        return 0;
    };
    dict.get_by_str("version", heap, interns)
        .and_then(|value| value.as_int(heap).ok())
        .unwrap_or(0)
}

/// Sets `registry['version']`.
fn set_registry_version(
    dict_id: HeapId,
    version: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from("version")))?;
    heap.with_entry_mut(dict_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("registry must be a dict"));
        };
        if let Some(old) = dict.set(Value::Ref(key_id), Value::Int(version), heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })
}

/// Clears a dict by invoking its `clear()` method.
fn clear_dict(dict_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<()> {
    heap.with_entry_mut(dict_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("registry must be a dict"));
        };
        dict.drop_all_entries(heap_inner);
        Ok(())
    })
}

/// Returns whether a registry contains `key`.
fn registry_contains_key(
    registry_id: Option<HeapId>,
    key: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    let Some(dict_id) = registry_id else {
        return Ok(false);
    };

    heap.with_entry_mut(dict_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("registry must be a dict"));
        };
        Ok(dict.get(key, heap_inner, interns)?.is_some())
    })
}

/// Sets `registry[key] = True`.
fn set_registry_key_true(
    registry_id: Option<HeapId>,
    key: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let Some(dict_id) = registry_id else {
        key.drop_with_heap(heap);
        return Ok(());
    };

    heap.with_entry_mut(dict_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            key.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("registry must be a dict"));
        };
        if let Some(old) = dict.set(key, Value::Bool(true), heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })
}

/// Checks whether `onceregistry` contains a key.
fn onceregistry_contains_key(key: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<bool> {
    let onceregistry_id = get_onceregistry_id(heap, interns)?;
    heap.with_entry_mut(onceregistry_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("warnings.onceregistry must be a dict"));
        };
        Ok(dict.get(key, heap_inner, interns)?.is_some())
    })
}

/// Sets `onceregistry[key] = True`.
fn set_onceregistry_key_true(key: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
    let onceregistry_id = get_onceregistry_id(heap, interns)?;
    heap.with_entry_mut(onceregistry_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            key.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("warnings.onceregistry must be a dict"));
        };
        if let Some(old) = dict.set(key, Value::Bool(true), heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })
}

/// Returns `warnings.onceregistry` dict id.
fn get_onceregistry_id(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let value = get_module_attr("onceregistry", heap, interns)?;
    if let Value::Ref(id_ref) = &value {
        let id = *id_ref;
        if matches!(heap.get(id), HeapData::Dict(_)) {
            value.drop_with_heap(heap);
            Ok(id)
        } else {
            value.drop_with_heap(heap);
            Err(ExcType::type_error("warnings.onceregistry must be a dict"))
        }
    } else {
        value.drop_with_heap(heap);
        Err(ExcType::type_error("warnings.onceregistry must be a dict"))
    }
}

/// Returns `warnings.filters` list id.
fn get_filters_list_id(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let value = get_module_attr("filters", heap, interns)?;
    if let Value::Ref(id_ref) = &value {
        let id = *id_ref;
        if matches!(heap.get(id), HeapData::List(_)) {
            value.drop_with_heap(heap);
            Ok(id)
        } else {
            value.drop_with_heap(heap);
            Err(ExcType::type_error("warnings.filters must be a list"))
        }
    } else {
        value.drop_with_heap(heap);
        Err(ExcType::type_error("warnings.filters must be a list"))
    }
}

/// Replaces `warnings.filters` with a new list object.
fn set_filters_list_id(list_id: HeapId, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
    set_module_attr("filters", Value::Ref(list_id), heap, interns)
}

/// Gets a module attribute by name.
fn get_module_attr(name: &str, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let module_id = warnings_module_id()?;
    heap.with_entry_mut(module_id, |heap_inner, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error("warnings module state is invalid"));
        };
        module
            .attrs()
            .get_by_str(name, heap_inner, interns)
            .map(|value| value.clone_with_heap(heap_inner))
            .ok_or_else(|| ExcType::attribute_error_module("warnings", name))
    })
}

/// Sets a module attribute by name.
fn set_module_attr(
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let module_id = warnings_module_id()?;
    heap.with_entry_mut(module_id, |heap_inner, data| {
        let HeapData::Module(module) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("warnings module state is invalid"));
        };
        module
            .set_attr_str(name, value, heap_inner, interns)
            .map_err(Into::into)
    })
}

/// Returns the current warnings module id.
fn warnings_module_id() -> RunResult<HeapId> {
    warnings_module_slot()
        .lock()
        .expect("warnings module id mutex poisoned")
        .as_ref()
        .copied()
        .ok_or_else(|| SimpleException::new_msg(ExcType::RuntimeError, "warnings module not initialized").into())
}

/// Returns the global warnings module-id storage slot.
fn warnings_module_slot() -> &'static Mutex<Option<HeapId>> {
    WARNINGS_MODULE_ID.get_or_init(|| Mutex::new(None))
}

/// Returns the active warning-record stack.
fn warnings_record_stack_slot() -> &'static Mutex<Vec<HeapId>> {
    WARNINGS_RECORD_STACK.get_or_init(|| Mutex::new(Vec::new()))
}

/// Returns the global filter-version slot.
fn warnings_filter_version_slot() -> &'static Mutex<i64> {
    WARNINGS_FILTERS_VERSION.get_or_init(|| Mutex::new(1))
}

/// Returns current filter version.
fn current_filters_version() -> i64 {
    *warnings_filter_version_slot()
        .lock()
        .expect("warnings filter version mutex poisoned")
}

/// Increments filter version.
fn bump_filters_version() {
    let mut guard = warnings_filter_version_slot()
        .lock()
        .expect("warnings filter version mutex poisoned");
    *guard += 1;
}

/// Pushes a `catch_warnings(record=True)` log list id.
fn push_record_list(list_id: HeapId) {
    warnings_record_stack_slot()
        .lock()
        .expect("warnings record stack mutex poisoned")
        .push(list_id);
}

/// Pops a record list id from the record stack.
fn pop_record_list() -> Option<HeapId> {
    warnings_record_stack_slot()
        .lock()
        .expect("warnings record stack mutex poisoned")
        .pop()
}

/// Returns the currently active record list id.
fn current_record_list() -> Option<HeapId> {
    warnings_record_stack_slot()
        .lock()
        .expect("warnings record stack mutex poisoned")
        .last()
        .copied()
}

/// Creates a fallback module name from a filename.
fn default_module_name_from_filename(filename: &str) -> String {
    if Path::new(filename)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        filename[..filename.len().saturating_sub(3)].to_string()
    } else if filename.is_empty() {
        "<unknown>".to_string()
    } else {
        filename.to_string()
    }
}

/// Sets `__deprecated__` metadata on a value where supported.
fn set_value_deprecated_attr(
    target: &Value,
    message: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from("__deprecated__")))?;
    match target {
        Value::DefFunction(function_id) => {
            let dict_id = heap.ensure_def_function_attr_dict(*function_id)?;
            set_deprecated_attr_in_dict(dict_id, key_id, message, heap, interns)
        }
        Value::Ref(id) => heap.with_entry_mut(*id, |heap_inner, data| match data {
            HeapData::ClassObject(class_obj) => {
                if let Some(old) = class_obj.set_attr(
                    Value::Ref(key_id),
                    message.clone_with_heap(heap_inner),
                    heap_inner,
                    interns,
                )? {
                    old.drop_with_heap(heap_inner);
                }
                Ok(())
            }
            HeapData::Instance(instance) => {
                if let Some(old) = instance.set_attr(
                    Value::Ref(key_id),
                    message.clone_with_heap(heap_inner),
                    heap_inner,
                    interns,
                )? {
                    old.drop_with_heap(heap_inner);
                }
                Ok(())
            }
            HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _) => {
                let dict_id = heap_inner.ensure_function_attr_dict(*id)?;
                set_deprecated_attr_in_dict(dict_id, key_id, message, heap_inner, interns)
            }
            _ => Err(ExcType::attribute_error(target.py_type(heap_inner), "__deprecated__")),
        }),
        _ => Err(ExcType::attribute_error(target.py_type(heap), "__deprecated__")),
    }
}

/// Stores `__deprecated__` in a function attribute dictionary.
fn set_deprecated_attr_in_dict(
    dict_id: HeapId,
    key_id: HeapId,
    message: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    heap.with_entry_mut(dict_id, |heap_inner, data| {
        let HeapData::Dict(dict) = data else {
            return Err(ExcType::type_error("function attribute dictionary must be a dict"));
        };
        if let Some(old) = dict.set(
            Value::Ref(key_id),
            message.clone_with_heap(heap_inner),
            heap_inner,
            interns,
        )? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })
}

/// Returns whether a value is callable.
fn is_callable_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Ref(id) => match heap.get(*id) {
            HeapData::ClassObject(_)
            | HeapData::BoundMethod(_)
            | HeapData::Partial(_)
            | HeapData::SingleDispatch(_)
            | HeapData::SingleDispatchRegister(_)
            | HeapData::SingleDispatchMethod(_)
            | HeapData::Closure(_, _, _)
            | HeapData::FunctionDefaults(_, _)
            | HeapData::ObjectNewImpl(_) => true,
            HeapData::Instance(instance) => {
                let class_id = instance.class_id();
                let HeapData::ClassObject(class_obj) = heap.get(class_id) else {
                    return false;
                };
                class_obj.mro_has_attr("__call__", class_id, heap, interns)
            }
            _ => false,
        },
        _ => false,
    }
}

/// Inserts a string-keyed class attribute.
fn dict_set_str_attr(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Registers one module-level function.
fn register(
    module: &mut Module,
    name: &str,
    function: WarningsFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Warnings(function)),
        heap,
        interns,
    )
}
