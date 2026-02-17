//! Minimal CPython-compatible `traceback` module implementation.
//!
//! The runtime currently does not expose VM traceback frame objects (`tb_next`,
//! `tb_frame`) to Python code, so this module focuses on the public formatting
//! API shape and class helpers (`FrameSummary`, `StackSummary`,
//! `TracebackException`) over the traceback data that is available at runtime.

use std::sync::{Mutex, OnceLock};

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    exception_private::{ExcType, RunError, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassMethod, ClassObject, Dict, Instance, List, Module, PyTrait, Str, Type, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// Function and method entry points for the `traceback` module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum TracebackFunctions {
    FormatExc,
    FormatException,
    FormatTb,
    FormatStack,
    PrintExc,
    PrintException,
    PrintTb,
    PrintStack,
    ExtractTb,
    ExtractStack,
    FormatList,
    FrameSummaryInit,
    StackSummaryInit,
    StackSummaryExtract,
    StackSummaryFormat,
    StackSummaryIter,
    StackSummaryLen,
    StackSummaryGetitem,
    StackSummaryAppend,
    TracebackExceptionInit,
    TracebackExceptionFormat,
    TracebackExceptionFormatExceptionOnly,
}

/// Keeps the helper class ids available to module call handlers.
static FRAME_SUMMARY_CLASS_ID: OnceLock<Mutex<Option<HeapId>>> = OnceLock::new();
static STACK_SUMMARY_CLASS_ID: OnceLock<Mutex<Option<HeapId>>> = OnceLock::new();
static TRACEBACK_EXCEPTION_CLASS_ID: OnceLock<Mutex<Option<HeapId>>> = OnceLock::new();

/// Creates the `traceback` module and allocates it on the heap.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Traceback);

    register(&mut module, "format_exc", TracebackFunctions::FormatExc, heap, interns)?;
    register(
        &mut module,
        "format_exception",
        TracebackFunctions::FormatException,
        heap,
        interns,
    )?;
    register(&mut module, "format_tb", TracebackFunctions::FormatTb, heap, interns)?;
    register(
        &mut module,
        "format_stack",
        TracebackFunctions::FormatStack,
        heap,
        interns,
    )?;
    register(&mut module, "print_exc", TracebackFunctions::PrintExc, heap, interns)?;
    register(
        &mut module,
        "print_exception",
        TracebackFunctions::PrintException,
        heap,
        interns,
    )?;
    register(&mut module, "print_tb", TracebackFunctions::PrintTb, heap, interns)?;
    register(
        &mut module,
        "print_stack",
        TracebackFunctions::PrintStack,
        heap,
        interns,
    )?;
    register(&mut module, "extract_tb", TracebackFunctions::ExtractTb, heap, interns)?;
    register(
        &mut module,
        "extract_stack",
        TracebackFunctions::ExtractStack,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "format_list",
        TracebackFunctions::FormatList,
        heap,
        interns,
    )?;

    let frame_summary_class = create_frame_summary_class(heap, interns)?;
    module.set_attr_text("FrameSummary", Value::Ref(frame_summary_class), heap, interns)?;

    let stack_summary_class = create_stack_summary_class(heap, interns)?;
    module.set_attr_text("StackSummary", Value::Ref(stack_summary_class), heap, interns)?;

    let traceback_exception_class = create_traceback_exception_class(heap, interns)?;
    module.set_attr_text(
        "TracebackException",
        Value::Ref(traceback_exception_class),
        heap,
        interns,
    )?;

    *frame_summary_class_slot()
        .lock()
        .expect("traceback FrameSummary class mutex poisoned") = Some(frame_summary_class);
    *stack_summary_class_slot()
        .lock()
        .expect("traceback StackSummary class mutex poisoned") = Some(stack_summary_class);
    *traceback_exception_class_slot()
        .lock()
        .expect("traceback TracebackException class mutex poisoned") = Some(traceback_exception_class);

    heap.allocate(HeapData::Module(module))
}

/// Dispatches `traceback` module and helper class function calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TracebackFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        TracebackFunctions::FormatExc => format_exc(heap, interns, args)?,
        TracebackFunctions::FormatException => format_exception(heap, interns, args)?,
        TracebackFunctions::FormatTb => format_tb(heap, interns, args)?,
        TracebackFunctions::FormatStack => format_stack(heap, interns, args)?,
        TracebackFunctions::PrintExc => print_exc(heap, interns, args)?,
        TracebackFunctions::PrintException => print_exception(heap, interns, args)?,
        TracebackFunctions::PrintTb => print_tb(heap, interns, args)?,
        TracebackFunctions::PrintStack => print_stack(heap, interns, args)?,
        TracebackFunctions::ExtractTb => extract_tb(heap, interns, args)?,
        TracebackFunctions::ExtractStack => extract_stack(heap, interns, args)?,
        TracebackFunctions::FormatList => format_list(heap, interns, args)?,
        TracebackFunctions::FrameSummaryInit => frame_summary_init(heap, interns, args)?,
        TracebackFunctions::StackSummaryInit => stack_summary_init(heap, interns, args)?,
        TracebackFunctions::StackSummaryExtract => stack_summary_extract(heap, interns, args)?,
        TracebackFunctions::StackSummaryFormat => stack_summary_format(heap, interns, args)?,
        TracebackFunctions::StackSummaryIter => stack_summary_iter(heap, interns, args)?,
        TracebackFunctions::StackSummaryLen => stack_summary_len(heap, interns, args)?,
        TracebackFunctions::StackSummaryGetitem => stack_summary_getitem(heap, interns, args)?,
        TracebackFunctions::StackSummaryAppend => stack_summary_append(heap, interns, args)?,
        TracebackFunctions::TracebackExceptionInit => traceback_exception_init(heap, interns, args)?,
        TracebackFunctions::TracebackExceptionFormat => traceback_exception_format(heap, interns, args)?,
        TracebackFunctions::TracebackExceptionFormatExceptionOnly => {
            traceback_exception_format_exception_only(heap, interns, args)?
        }
    };
    Ok(AttrCallResult::Value(value))
}

/// Registers one callable on the module.
fn register(
    module: &mut Module,
    name: &str,
    function: TracebackFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Traceback(function)),
        heap,
        interns,
    )
}

/// Creates `traceback.FrameSummary`.
fn create_frame_summary_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut attrs = Dict::new();
    set_dict_attr_by_name(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::FrameSummaryInit)),
        heap,
        interns,
    )?;
    create_helper_class("traceback.FrameSummary", attrs, &[Type::Object], heap, interns)
}

/// Creates `traceback.StackSummary`.
fn create_stack_summary_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut attrs = Dict::new();
    set_dict_attr_by_name(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryInit)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "format",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryFormat)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "__iter__",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryIter)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "__len__",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryLen)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "__getitem__",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryGetitem)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "append",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryAppend)),
        heap,
        interns,
    )?;

    let extract_method = Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::StackSummaryExtract));
    let extract_id = heap.allocate(HeapData::ClassMethod(ClassMethod::new(extract_method)))?;
    set_dict_attr_by_name(&mut attrs, "extract", Value::Ref(extract_id), heap, interns)?;

    create_helper_class("traceback.StackSummary", attrs, &[Type::Object], heap, interns)
}

/// Creates `traceback.TracebackException`.
fn create_traceback_exception_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut attrs = Dict::new();
    set_dict_attr_by_name(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::TracebackExceptionInit)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "format",
        Value::ModuleFunction(ModuleFunctions::Traceback(TracebackFunctions::TracebackExceptionFormat)),
        heap,
        interns,
    )?;
    set_dict_attr_by_name(
        &mut attrs,
        "format_exception_only",
        Value::ModuleFunction(ModuleFunctions::Traceback(
            TracebackFunctions::TracebackExceptionFormatExceptionOnly,
        )),
        heap,
        interns,
    )?;
    create_helper_class("traceback.TracebackException", attrs, &[Type::Object], heap, interns)
}

/// Allocates a helper class inheriting from the provided base types.
fn create_helper_class(
    class_name: &str,
    attrs: Dict,
    base_types: &[Type],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut base_ids = Vec::with_capacity(base_types.len());
    for base in base_types {
        let id = heap.builtin_class_id(*base)?;
        heap.inc_ref(id);
        base_ids.push(id);
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap(class_name.to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        attrs,
        base_ids.clone(),
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &base_ids, heap, interns).expect("traceback helper class MRO should be valid");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(class_obj) = heap.get_mut(class_id) {
        class_obj.set_mro(mro);
    }

    for &base_id in &base_ids {
        heap.with_entry_mut(base_id, |_, data| {
            let HeapData::ClassObject(base_cls) = data else {
                return Err(ExcType::type_error("traceback helper base is not a class"));
            };
            base_cls.register_subclass(class_id, class_uid);
            Ok(())
        })
        .expect("traceback helper class registration should succeed");
    }

    Ok(class_id)
}

/// Sets a dictionary attribute by string key.
fn set_dict_attr_by_name(
    dict: &mut Dict,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string dict keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Returns the mutable slot for `FrameSummary` helper class id.
fn frame_summary_class_slot() -> &'static Mutex<Option<HeapId>> {
    FRAME_SUMMARY_CLASS_ID.get_or_init(|| Mutex::new(None))
}

/// Returns the mutable slot for `StackSummary` helper class id.
fn stack_summary_class_slot() -> &'static Mutex<Option<HeapId>> {
    STACK_SUMMARY_CLASS_ID.get_or_init(|| Mutex::new(None))
}

/// Returns the mutable slot for `TracebackException` helper class id.
fn traceback_exception_class_slot() -> &'static Mutex<Option<HeapId>> {
    TRACEBACK_EXCEPTION_CLASS_ID.get_or_init(|| Mutex::new(None))
}

/// Returns the configured `FrameSummary` class id.
fn frame_summary_class_id() -> RunResult<HeapId> {
    frame_summary_class_slot()
        .lock()
        .expect("traceback FrameSummary class mutex poisoned")
        .as_ref()
        .copied()
        .ok_or_else(|| RunError::internal("traceback FrameSummary class not initialized"))
}

/// Returns the configured `StackSummary` class id.
fn stack_summary_class_id() -> RunResult<HeapId> {
    stack_summary_class_slot()
        .lock()
        .expect("traceback StackSummary class mutex poisoned")
        .as_ref()
        .copied()
        .ok_or_else(|| RunError::internal("traceback StackSummary class not initialized"))
}

/// Builds a helper class instance with an optional `__dict__`.
fn create_instance_for_class(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let (slot_len, has_dict) = match heap.get(class_id) {
        HeapData::ClassObject(cls) => (cls.slot_layout().len(), cls.instance_has_dict()),
        _ => return Err(ExcType::type_error("traceback helper class is not a class object")),
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
    Ok(heap.allocate(HeapData::Instance(instance))?)
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
            return Err(ExcType::type_error("traceback helper expected instance"));
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

/// Extracts `self` from an instance method call and returns remaining args.
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

/// Rebuilds `ArgValues` from positional/keyword parts.
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

/// Converts a string list into a Python list of `str` values.
fn list_from_strings(strings: Vec<String>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let mut items = Vec::with_capacity(strings.len());
    for text in strings {
        let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
        items.push(Value::Ref(text_id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Returns `true` when the value is exactly `None`.
fn is_none(value: &Value) -> bool {
    matches!(value, Value::None)
}

/// Parses `limit` argument (`None` => no limit).
fn parse_limit(value: Option<&Value>, heap: &Heap<impl ResourceTracker>) -> RunResult<Option<i64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if is_none(value) {
        return Ok(None);
    }
    Ok(Some(value.as_int(heap)?))
}

/// Parses boolean argument using Python truthiness semantics.
fn parse_bool_or_default(
    value: Option<&Value>,
    default: bool,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    value.map_or(default, |v| v.py_bool(heap, interns))
}

/// Returns the function name for a class or exception type-like value.
fn exception_type_name(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> String {
    if let Value::Builtin(Builtins::ExcType(exc_type) | Builtins::Type(Type::Exception(exc_type))) = value {
        exc_type.to_string()
    } else {
        let name = value.py_getattr(StaticStrings::DunderName.into(), heap, interns);
        match name {
            Ok(AttrCallResult::Value(v) | AttrCallResult::DescriptorGet(v)) => {
                let text = v.py_str(heap, interns).into_owned();
                v.drop_with_heap(heap);
                text
            }
            _ => value.py_type(heap).to_string(),
        }
    }
}

/// Parses one frame record from tuple/list/FrameSummary-like input.
fn parse_frame_record(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<(String, i64, String, Option<String>)> {
    if let Value::Ref(id) = value
        && matches!(heap.get(*id), HeapData::Instance(_))
    {
        let filename = get_instance_attr_by_name(*id, "filename", heap, interns)?;
        let lineno = get_instance_attr_by_name(*id, "lineno", heap, interns)?;
        let name = get_instance_attr_by_name(*id, "name", heap, interns)?;
        let line = get_instance_attr_by_name(*id, "line", heap, interns);

        let filename_text = filename.py_str(heap, interns).into_owned();
        let lineno_int = lineno.as_int(heap).unwrap_or(0);
        let name_text = name.py_str(heap, interns).into_owned();
        let line_text = line.as_ref().and_then(|line_value| {
            if is_none(line_value) {
                None
            } else {
                Some(line_value.py_str(heap, interns).into_owned())
            }
        });

        filename.drop_with_heap(heap);
        lineno.drop_with_heap(heap);
        name.drop_with_heap(heap);
        if let Some(line) = line {
            line.drop_with_heap(heap);
        }
        return Some((filename_text, lineno_int, name_text, line_text));
    }

    let Value::Ref(id) = value else {
        return None;
    };
    let items: &[Value] = match heap.get(*id) {
        HeapData::Tuple(tuple) => tuple.as_vec(),
        HeapData::List(list) => list.as_vec(),
        _ => return None,
    };
    if items.len() < 3 {
        return None;
    }

    let filename = items[0].py_str(heap, interns).into_owned();
    let lineno = items[1].as_int(heap).unwrap_or(0);
    let name = items[2].py_str(heap, interns).into_owned();
    let line = items.get(3).and_then(|line_value| {
        if is_none(line_value) {
            None
        } else {
            Some(line_value.py_str(heap, interns).into_owned())
        }
    });

    Some((filename, lineno, name, line))
}

/// Formats one frame line in CPython-compatible style.
fn format_frame_line(filename: &str, lineno: i64, name: &str, line: Option<&str>) -> String {
    let mut rendered = format!("  File \"{filename}\", line {lineno}, in {name}\n");
    if let Some(line) = line {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            rendered.push_str("    ");
            rendered.push_str(trimmed);
            rendered.push('\n');
        }
    }
    rendered
}

/// Applies traceback frame limit semantics.
fn apply_frame_limit(mut frames: Vec<Value>, limit: Option<i64>, heap: &mut Heap<impl ResourceTracker>) -> Vec<Value> {
    let Some(limit) = limit else {
        return frames;
    };

    if limit >= 0 {
        let keep = usize::try_from(limit).unwrap_or(usize::MAX);
        if frames.len() > keep {
            let dropped = frames.split_off(keep);
            dropped.drop_with_heap(heap);
        }
        return frames;
    }

    let keep = limit.unsigned_abs() as usize;
    if keep >= frames.len() {
        return frames;
    }
    let dropped = frames.drain(..frames.len() - keep).collect::<Vec<_>>();
    dropped.drop_with_heap(heap);
    frames
}

/// Builds a vector of frame values from supported traceback-like inputs.
fn frame_values_from_source(source: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Vec<Value> {
    if is_none(source) {
        return Vec::new();
    }

    if let Value::Ref(source_id) = source
        && matches!(heap.get(*source_id), HeapData::Instance(_))
    {
        if let Some(frames_value) = get_instance_attr_by_name(*source_id, "_frames", heap, interns) {
            let values = frame_values_from_source(&frames_value, heap, interns);
            frames_value.drop_with_heap(heap);
            return values;
        }
        if let Some(stack_value) = get_instance_attr_by_name(*source_id, "_stack", heap, interns) {
            let values = frame_values_from_source(&stack_value, heap, interns);
            stack_value.drop_with_heap(heap);
            return values;
        }
    }

    let Value::Ref(source_id) = source else {
        return Vec::new();
    };
    match heap.get(*source_id) {
        HeapData::List(list) => list.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect(),
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect(),
        _ => Vec::new(),
    }
}

/// Formats traceback-like frame values to module output lines.
fn format_frames_from_source(
    source: &Value,
    limit: Option<i64>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<String> {
    let values = frame_values_from_source(source, heap, interns);
    let values = apply_frame_limit(values, limit, heap);
    let mut lines = Vec::with_capacity(values.len());
    for value in &values {
        if let Some((filename, lineno, name, line)) = parse_frame_record(value, heap, interns) {
            lines.push(format_frame_line(&filename, lineno, &name, line.as_deref()));
        }
    }
    values.drop_with_heap(heap);
    lines
}

/// Builds an exception-only rendered line.
fn format_exception_only_line(
    exc_type: Option<&Value>,
    exc_value: Option<&Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> String {
    if let Some(value) = exc_value
        && let Value::Ref(id) = value
        && let HeapData::Exception(simple_exc) = heap.get(*id)
    {
        let ty = simple_exc.exc_type().to_string();
        let msg = simple_exc.py_str();
        if msg.is_empty() {
            return format!("{ty}\n");
        }
        return format!("{ty}: {msg}\n");
    }

    if let Some(value) = exc_value
        && !is_none(value)
    {
        let ty_name = if let Some(ty) = exc_type {
            exception_type_name(ty, heap, interns)
        } else {
            value.py_type(heap).to_string()
        };
        let msg = value.py_str(heap, interns).into_owned();
        if msg.is_empty() {
            return format!("{ty_name}\n");
        }
        return format!("{ty_name}: {msg}\n");
    }

    "NoneType: None\n".to_string()
}

/// Formats traceback + exception into `traceback.format_exception` style lines.
fn format_exception_lines(
    exc_type: Option<&Value>,
    exc_value: Option<&Value>,
    tb: Option<&Value>,
    limit: Option<i64>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(tb_value) = tb {
        let tb_lines = format_frames_from_source(tb_value, limit, heap, interns);
        if !tb_lines.is_empty() {
            out.push("Traceback (most recent call last):\n".to_string());
            out.extend(tb_lines);
        }
    }
    out.push(format_exception_only_line(exc_type, exc_value, heap, interns));
    out
}

/// Writes text through a file-like `write(str)` API.
fn write_text_to_file(
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
        Err(err) => Err(err),
    }
}

/// Creates a `StackSummary` helper instance from frame values.
fn make_stack_summary_value(
    frames: Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let class_id = stack_summary_class_id()?;
    let instance_id = create_instance_for_class(class_id, heap)?;
    let frames_id = heap.allocate(HeapData::List(List::new(frames)))?;
    set_instance_attr_by_name(instance_id, "_frames", Value::Ref(frames_id), heap, interns)?;
    Ok(Value::Ref(instance_id))
}

/// Implements `traceback.format_exc(limit=None, chain=True)`.
fn format_exc(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("format_exc", 2, count));
    }

    let mut limit = positional.first().map(|value| value.clone_with_heap(heap));
    let mut chain = positional.get(1).map(|value| value.clone_with_heap(heap));
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            chain.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "limit" => {
                limit.drop_with_heap(heap);
                limit = Some(value);
            }
            "chain" => {
                chain.drop_with_heap(heap);
                chain = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                chain.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("format_exc", &key_text));
            }
        }
    }

    let _parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let _parsed_chain = parse_bool_or_default(chain.as_ref(), true, heap, interns);
    limit.drop_with_heap(heap);
    chain.drop_with_heap(heap);

    let text = "NoneType: None\n".to_string();
    let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(text_id))
}

/// Implements `traceback.format_exception(exc, value=None, tb=None, limit=None, chain=True)`.
fn format_exception(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("format_exception", 1, 0));
    }
    if positional.len() > 5 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("format_exception", 5, count));
    }

    let exc = positional.remove(0);
    let mut value = positional.first().map(|v| v.clone_with_heap(heap));
    let mut tb = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut limit = positional.get(2).map(|v| v.clone_with_heap(heap));
    let mut chain = positional.get(3).map(|v| v.clone_with_heap(heap));
    positional.drop_with_heap(heap);

    for (key, kw_value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            kw_value.drop_with_heap(heap);
            exc.drop_with_heap(heap);
            value.drop_with_heap(heap);
            tb.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            chain.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "value" => {
                value.drop_with_heap(heap);
                value = Some(kw_value);
            }
            "tb" => {
                tb.drop_with_heap(heap);
                tb = Some(kw_value);
            }
            "limit" => {
                limit.drop_with_heap(heap);
                limit = Some(kw_value);
            }
            "chain" => {
                chain.drop_with_heap(heap);
                chain = Some(kw_value);
            }
            _ => {
                kw_value.drop_with_heap(heap);
                exc.drop_with_heap(heap);
                value.drop_with_heap(heap);
                tb.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                chain.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("format_exception", &key_text));
            }
        }
    }

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let _parsed_chain = parse_bool_or_default(chain.as_ref(), true, heap, interns);

    let mut exc_type_ref: Option<&Value> = Some(&exc);
    let mut exc_value_ref: Option<&Value> = value.as_ref();
    if value.is_none()
        && let Value::Ref(id) = &exc
        && matches!(heap.get(*id), HeapData::Exception(_))
    {
        exc_type_ref = None;
        exc_value_ref = Some(&exc);
    }

    let lines = format_exception_lines(exc_type_ref, exc_value_ref, tb.as_ref(), parsed_limit, heap, interns);
    let result = list_from_strings(lines, heap);

    exc.drop_with_heap(heap);
    value.drop_with_heap(heap);
    tb.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    chain.drop_with_heap(heap);

    result
}

/// Implements `traceback.format_tb(tb, limit=None)`.
fn format_tb(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("format_tb", 2, count));
    }

    let mut tb = positional.first().map(|v| v.clone_with_heap(heap));
    let mut limit = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut tb_seen = tb.is_some();
    let mut limit_seen = limit.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            tb.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "tb" => {
                if tb_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("format_tb", "tb"));
                }
                tb = Some(value);
                tb_seen = true;
            }
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("format_tb", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                tb.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("format_tb", &key_text));
            }
        }
    }

    let Some(tb) = tb else {
        limit.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("format_tb", 1, 0));
    };

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let lines = format_frames_from_source(&tb, parsed_limit, heap, interns);
    let result = list_from_strings(lines, heap);
    tb.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    result
}

/// Implements `traceback.format_stack(f=None, limit=None)`.
fn format_stack(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("format_stack", 2, count));
    }

    let mut frame = positional.first().map_or(Value::None, |v| v.clone_with_heap(heap));
    let mut limit = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut frame_seen = !positional.is_empty();
    let mut limit_seen = limit.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            frame.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "f" => {
                if frame_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("format_stack", "f"));
                }
                frame.drop_with_heap(heap);
                frame = value;
                frame_seen = true;
            }
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("format_stack", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                frame.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("format_stack", &key_text));
            }
        }
    }

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let lines = format_frames_from_source(&frame, parsed_limit, heap, interns);
    let result = list_from_strings(lines, heap);
    frame.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    result
}

/// Implements `traceback.print_exc(limit=None, file=None, chain=True)`.
fn print_exc(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 3 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("print_exc", 3, count));
    }
    let mut limit = positional.first().map(|v| v.clone_with_heap(heap));
    let mut file = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut chain = positional.get(2).map(|v| v.clone_with_heap(heap));
    let mut limit_seen = limit.is_some();
    let mut file_seen = file.is_some();
    let mut chain_seen = chain.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            file.drop_with_heap(heap);
            chain.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exc", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            "file" => {
                if file_seen {
                    value.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exc", "file"));
                }
                file.drop_with_heap(heap);
                file = Some(value);
                file_seen = true;
            }
            "chain" => {
                if chain_seen {
                    value.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exc", "chain"));
                }
                chain.drop_with_heap(heap);
                chain = Some(value);
                chain_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                file.drop_with_heap(heap);
                chain.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("print_exc", &key_text));
            }
        }
    }

    let _parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let _parsed_chain = parse_bool_or_default(chain.as_ref(), true, heap, interns);

    let text = "NoneType: None\n";
    if let Some(file_value) = &file {
        if is_none(file_value) {
            eprint!("{text}");
        } else {
            write_text_to_file(file_value, text, heap, interns)?;
        }
    } else {
        eprint!("{text}");
    }
    limit.drop_with_heap(heap);
    file.drop_with_heap(heap);
    chain.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `traceback.print_exception(...)`.
fn print_exception(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("print_exception", 1, 0));
    }
    if positional.len() > 6 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("print_exception", 6, count));
    }

    let exc = positional.remove(0);
    let mut value = positional.first().map(|v| v.clone_with_heap(heap));
    let mut tb = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut limit = positional.get(2).map(|v| v.clone_with_heap(heap));
    let mut file = positional.get(3).map(|v| v.clone_with_heap(heap));
    let mut chain = positional.get(4).map(|v| v.clone_with_heap(heap));
    let mut value_seen = value.is_some();
    let mut tb_seen = tb.is_some();
    let mut limit_seen = limit.is_some();
    let mut file_seen = file.is_some();
    let mut chain_seen = chain.is_some();
    positional.drop_with_heap(heap);

    for (key, kw_value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            kw_value.drop_with_heap(heap);
            exc.drop_with_heap(heap);
            value.drop_with_heap(heap);
            tb.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            file.drop_with_heap(heap);
            chain.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "value" => {
                if value_seen {
                    kw_value.drop_with_heap(heap);
                    exc.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exception", "value"));
                }
                value.drop_with_heap(heap);
                value = Some(kw_value);
                value_seen = true;
            }
            "tb" => {
                if tb_seen {
                    kw_value.drop_with_heap(heap);
                    exc.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exception", "tb"));
                }
                tb.drop_with_heap(heap);
                tb = Some(kw_value);
                tb_seen = true;
            }
            "limit" => {
                if limit_seen {
                    kw_value.drop_with_heap(heap);
                    exc.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exception", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(kw_value);
                limit_seen = true;
            }
            "file" => {
                if file_seen {
                    kw_value.drop_with_heap(heap);
                    exc.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exception", "file"));
                }
                file.drop_with_heap(heap);
                file = Some(kw_value);
                file_seen = true;
            }
            "chain" => {
                if chain_seen {
                    kw_value.drop_with_heap(heap);
                    exc.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    chain.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_exception", "chain"));
                }
                chain.drop_with_heap(heap);
                chain = Some(kw_value);
                chain_seen = true;
            }
            _ => {
                kw_value.drop_with_heap(heap);
                exc.drop_with_heap(heap);
                value.drop_with_heap(heap);
                tb.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                file.drop_with_heap(heap);
                chain.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("print_exception", &key_text));
            }
        }
    }

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let _parsed_chain = parse_bool_or_default(chain.as_ref(), true, heap, interns);

    let mut exc_type_ref: Option<&Value> = Some(&exc);
    let mut exc_value_ref: Option<&Value> = value.as_ref();
    if value.is_none()
        && let Value::Ref(id) = &exc
        && matches!(heap.get(*id), HeapData::Exception(_))
    {
        exc_type_ref = None;
        exc_value_ref = Some(&exc);
    }

    let lines = format_exception_lines(exc_type_ref, exc_value_ref, tb.as_ref(), parsed_limit, heap, interns);
    let rendered = lines.concat();
    if let Some(file_value) = &file {
        if is_none(file_value) {
            eprint!("{rendered}");
        } else {
            write_text_to_file(file_value, &rendered, heap, interns)?;
        }
    } else {
        eprint!("{rendered}");
    }

    exc.drop_with_heap(heap);
    value.drop_with_heap(heap);
    tb.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    file.drop_with_heap(heap);
    chain.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `traceback.print_tb(tb, limit=None, file=None)`.
fn print_tb(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 3 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("print_tb", 3, count));
    }

    let mut tb = positional.first().map(|v| v.clone_with_heap(heap));
    let mut limit = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut file = positional.get(2).map(|v| v.clone_with_heap(heap));
    let mut tb_seen = tb.is_some();
    let mut limit_seen = limit.is_some();
    let mut file_seen = file.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            tb.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            file.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "tb" => {
                if tb_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_tb", "tb"));
                }
                tb = Some(value);
                tb_seen = true;
            }
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_tb", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            "file" => {
                if file_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_tb", "file"));
                }
                file.drop_with_heap(heap);
                file = Some(value);
                file_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                tb.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                file.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("print_tb", &key_text));
            }
        }
    }

    let Some(tb) = tb else {
        limit.drop_with_heap(heap);
        file.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("print_tb", 1, 0));
    };

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let rendered = format_frames_from_source(&tb, parsed_limit, heap, interns).concat();
    if let Some(file_value) = &file {
        if is_none(file_value) {
            eprint!("{rendered}");
        } else {
            write_text_to_file(file_value, &rendered, heap, interns)?;
        }
    } else {
        eprint!("{rendered}");
    }

    tb.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    file.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `traceback.print_stack(f=None, limit=None, file=None)`.
fn print_stack(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 3 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("print_stack", 3, count));
    }
    let mut frame = positional.first().map_or(Value::None, |v| v.clone_with_heap(heap));
    let mut limit = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut file = positional.get(2).map(|v| v.clone_with_heap(heap));
    let mut frame_seen = !positional.is_empty();
    let mut limit_seen = limit.is_some();
    let mut file_seen = file.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            frame.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            file.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "f" => {
                if frame_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_stack", "f"));
                }
                frame.drop_with_heap(heap);
                frame = value;
                frame_seen = true;
            }
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_stack", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            "file" => {
                if file_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    file.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("print_stack", "file"));
                }
                file.drop_with_heap(heap);
                file = Some(value);
                file_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                frame.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                file.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("print_stack", &key_text));
            }
        }
    }

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let rendered = format_frames_from_source(&frame, parsed_limit, heap, interns).concat();
    if let Some(file_value) = &file {
        if is_none(file_value) {
            eprint!("{rendered}");
        } else {
            write_text_to_file(file_value, &rendered, heap, interns)?;
        }
    } else {
        eprint!("{rendered}");
    }

    frame.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    file.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `traceback.extract_tb(tb, limit=None)`.
fn extract_tb(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("extract_tb", 2, count));
    }

    let mut tb = positional.first().map(|v| v.clone_with_heap(heap));
    let mut limit = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut tb_seen = tb.is_some();
    let mut limit_seen = limit.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            tb.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "tb" => {
                if tb_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("extract_tb", "tb"));
                }
                tb = Some(value);
                tb_seen = true;
            }
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    tb.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("extract_tb", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                tb.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("extract_tb", &key_text));
            }
        }
    }

    let Some(tb) = tb else {
        limit.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("extract_tb", 1, 0));
    };

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let frames = apply_frame_limit(frame_values_from_source(&tb, heap, interns), parsed_limit, heap);
    let result = make_stack_summary_value(frames, heap, interns);
    tb.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    result
}

/// Implements `traceback.extract_stack(f=None, limit=None)`.
fn extract_stack(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("extract_stack", 2, count));
    }

    let mut frame = positional.first().map_or(Value::None, |v| v.clone_with_heap(heap));
    let mut limit = positional.get(1).map(|v| v.clone_with_heap(heap));
    let mut frame_seen = !positional.is_empty();
    let mut limit_seen = limit.is_some();
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            frame.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "f" => {
                if frame_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("extract_stack", "f"));
                }
                frame.drop_with_heap(heap);
                frame = value;
                frame_seen = true;
            }
            "limit" => {
                if limit_seen {
                    value.drop_with_heap(heap);
                    frame.drop_with_heap(heap);
                    limit.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("extract_stack", "limit"));
                }
                limit.drop_with_heap(heap);
                limit = Some(value);
                limit_seen = true;
            }
            _ => {
                value.drop_with_heap(heap);
                frame.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("extract_stack", &key_text));
            }
        }
    }

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let frames = apply_frame_limit(frame_values_from_source(&frame, heap, interns), parsed_limit, heap);
    let result = make_stack_summary_value(frames, heap, interns);
    frame.drop_with_heap(heap);
    limit.drop_with_heap(heap);
    result
}

/// Implements `traceback.format_list(extracted_list)`.
fn format_list(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let extracted = args.get_one_arg("format_list", heap)?;
    let lines = format_frames_from_source(&extracted, None, heap, interns);
    let result = list_from_strings(lines, heap);
    extracted.drop_with_heap(heap);
    result
}

/// Implements `FrameSummary.__init__(...)`.
fn frame_summary_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "FrameSummary.__init__")?;
    let (pos_iter, kwargs) = method_args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() < 3 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("FrameSummary", 3, 0));
    }
    if positional.len() > 6 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("FrameSummary", 6, count));
    }

    let filename = positional[0].clone_with_heap(heap);
    let lineno = positional[1].clone_with_heap(heap);
    let name = positional[2].clone_with_heap(heap);
    let mut lookup_line = positional.get(3).map(|v| v.clone_with_heap(heap));
    let mut locals = positional.get(4).map_or(Value::None, |v| v.clone_with_heap(heap));
    let mut line = positional.get(5).map_or(Value::None, |v| v.clone_with_heap(heap));
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            filename.drop_with_heap(heap);
            lineno.drop_with_heap(heap);
            name.drop_with_heap(heap);
            lookup_line.drop_with_heap(heap);
            locals.drop_with_heap(heap);
            line.drop_with_heap(heap);
            self_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "lookup_line" => {
                lookup_line.drop_with_heap(heap);
                lookup_line = Some(value);
            }
            "locals" => {
                locals.drop_with_heap(heap);
                locals = value;
            }
            "line" => {
                line.drop_with_heap(heap);
                line = value;
            }
            _ => {
                value.drop_with_heap(heap);
                filename.drop_with_heap(heap);
                lineno.drop_with_heap(heap);
                name.drop_with_heap(heap);
                lookup_line.drop_with_heap(heap);
                locals.drop_with_heap(heap);
                line.drop_with_heap(heap);
                self_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("FrameSummary", &key_text));
            }
        }
    }

    let _lookup_line_enabled = parse_bool_or_default(lookup_line.as_ref(), true, heap, interns);
    let lineno_value = match lineno.as_int(heap) {
        Ok(v) => Value::Int(v),
        Err(_) => lineno.clone_with_heap(heap),
    };

    set_instance_attr_by_name(self_id, "filename", filename, heap, interns)?;
    set_instance_attr_by_name(self_id, "lineno", lineno_value, heap, interns)?;
    set_instance_attr_by_name(self_id, "name", name, heap, interns)?;
    set_instance_attr_by_name(self_id, "line", line, heap, interns)?;
    set_instance_attr_by_name(self_id, "locals", locals, heap, interns)?;

    lineno.drop_with_heap(heap);
    lookup_line.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `StackSummary.__init__(frames=None)`.
fn stack_summary_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "StackSummary.__init__")?;
    let maybe_frames = method_args.get_zero_one_arg("StackSummary", heap)?;
    let mut frames_vec = maybe_frames
        .as_ref()
        .map_or_else(Vec::new, |value| frame_values_from_source(value, heap, interns));
    if let Some(frames) = maybe_frames {
        frames.drop_with_heap(heap);
    }
    let frames_id = heap.allocate(HeapData::List(List::new(std::mem::take(&mut frames_vec))))?;
    set_instance_attr_by_name(self_id, "_frames", Value::Ref(frames_id), heap, interns)?;
    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `StackSummary.extract(frame_gen, *, lookup_lines=True, capture_locals=False)`.
fn stack_summary_extract(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (pos_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = pos_iter.collect();
    if positional.len() < 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("StackSummary.extract", 2, 0));
    }
    if positional.len() > 4 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("StackSummary.extract", 4, count));
    }

    let cls_value = positional.remove(0);
    cls_value.drop_with_heap(heap);
    let frame_gen = positional.remove(0);
    let mut lookup_lines = positional.first().map(|v| v.clone_with_heap(heap));
    let mut capture_locals = positional.get(1).map(|v| v.clone_with_heap(heap));
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            frame_gen.drop_with_heap(heap);
            lookup_lines.drop_with_heap(heap);
            capture_locals.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "lookup_lines" => {
                lookup_lines.drop_with_heap(heap);
                lookup_lines = Some(value);
            }
            "capture_locals" => {
                capture_locals.drop_with_heap(heap);
                capture_locals = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                frame_gen.drop_with_heap(heap);
                lookup_lines.drop_with_heap(heap);
                capture_locals.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(
                    "StackSummary.extract",
                    &key_text,
                ));
            }
        }
    }

    let _ = parse_bool_or_default(lookup_lines.as_ref(), true, heap, interns);
    let _ = parse_bool_or_default(capture_locals.as_ref(), false, heap, interns);
    let frames = frame_values_from_source(&frame_gen, heap, interns);
    let result = make_stack_summary_value(frames, heap, interns);

    frame_gen.drop_with_heap(heap);
    lookup_lines.drop_with_heap(heap);
    capture_locals.drop_with_heap(heap);
    result
}

/// Implements `StackSummary.format()`.
fn stack_summary_format(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "StackSummary.format")?;
    method_args.check_zero_args("StackSummary.format", heap)?;
    let frames = get_instance_attr_by_name(self_id, "_frames", heap, interns).unwrap_or(Value::None);
    let lines = format_frames_from_source(&frames, None, heap, interns);
    let result = list_from_strings(lines, heap);
    frames.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    result
}

/// Implements `StackSummary.__iter__()`.
fn stack_summary_iter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "StackSummary.__iter__")?;
    method_args.check_zero_args("StackSummary.__iter__", heap)?;
    let mut frames = get_instance_attr_by_name(self_id, "_frames", heap, interns).unwrap_or(Value::None);
    let iter = frames.py_call_attr(
        heap,
        &EitherStr::Heap("__iter__".to_string()),
        ArgValues::Empty,
        interns,
        None,
    )?;
    frames.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(iter)
}

/// Implements `StackSummary.__len__()`.
fn stack_summary_len(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "StackSummary.__len__")?;
    method_args.check_zero_args("StackSummary.__len__", heap)?;
    let frames = get_instance_attr_by_name(self_id, "_frames", heap, interns).unwrap_or(Value::None);
    let len = match &frames {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::List(list) => {
                i64::try_from(list.len()).map_err(|_| RunError::internal("StackSummary length exceeds i64"))?
            }
            _ => 0,
        },
        _ => 0,
    };
    frames.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::Int(len))
}

/// Implements `StackSummary.__getitem__(index)`.
fn stack_summary_getitem(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "StackSummary.__getitem__")?;
    let index = method_args.get_one_arg("StackSummary.__getitem__", heap)?;
    let mut frames = get_instance_attr_by_name(self_id, "_frames", heap, interns).unwrap_or(Value::None);
    let result = frames.py_getitem(&index, heap, interns)?;
    index.drop_with_heap(heap);
    frames.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(result)
}

/// Implements `StackSummary.append(item)`.
fn stack_summary_append(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "StackSummary.append")?;
    let item = method_args.get_one_arg("StackSummary.append", heap)?;
    let mut frames = get_instance_attr_by_name(self_id, "_frames", heap, interns).unwrap_or(Value::None);
    let append_result = frames.py_call_attr(
        heap,
        &EitherStr::Heap("append".to_string()),
        ArgValues::One(item),
        interns,
        None,
    )?;
    append_result.drop_with_heap(heap);
    frames.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `TracebackException.__init__(exc_type, exc_value, exc_tb, *, limit=None, chain=True)`.
fn traceback_exception_init(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "TracebackException.__init__")?;
    let (pos_iter, kwargs) = method_args.into_parts();
    let positional: Vec<Value> = pos_iter.collect();
    if positional.len() < 3 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("TracebackException", 3, 0));
    }
    if positional.len() > 5 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("TracebackException", 5, count));
    }

    let exc_type = positional[0].clone_with_heap(heap);
    let exc_value = positional[1].clone_with_heap(heap);
    let exc_tb = positional[2].clone_with_heap(heap);
    let mut limit = positional.get(3).map(|v| v.clone_with_heap(heap));
    let mut chain = positional.get(4).map(|v| v.clone_with_heap(heap));
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            exc_type.drop_with_heap(heap);
            exc_value.drop_with_heap(heap);
            exc_tb.drop_with_heap(heap);
            limit.drop_with_heap(heap);
            chain.drop_with_heap(heap);
            self_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_text = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_text.as_str() {
            "limit" => {
                limit.drop_with_heap(heap);
                limit = Some(value);
            }
            "chain" => {
                chain.drop_with_heap(heap);
                chain = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                exc_type.drop_with_heap(heap);
                exc_value.drop_with_heap(heap);
                exc_tb.drop_with_heap(heap);
                limit.drop_with_heap(heap);
                chain.drop_with_heap(heap);
                self_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("TracebackException", &key_text));
            }
        }
    }

    let parsed_limit = parse_limit(limit.as_ref(), heap)?;
    let chain_flag = parse_bool_or_default(chain.as_ref(), true, heap, interns);

    let frames = apply_frame_limit(frame_values_from_source(&exc_tb, heap, interns), parsed_limit, heap);
    let frames_id = heap.allocate(HeapData::List(List::new(frames)))?;

    set_instance_attr_by_name(self_id, "exc_type", exc_type, heap, interns)?;
    set_instance_attr_by_name(self_id, "exc_value", exc_value, heap, interns)?;
    set_instance_attr_by_name(self_id, "exc_tb", exc_tb, heap, interns)?;
    set_instance_attr_by_name(self_id, "_stack", Value::Ref(frames_id), heap, interns)?;
    set_instance_attr_by_name(self_id, "_chain", Value::Bool(chain_flag), heap, interns)?;
    set_instance_attr_by_name(self_id, "__cause__", Value::None, heap, interns)?;
    set_instance_attr_by_name(self_id, "__context__", Value::None, heap, interns)?;
    set_instance_attr_by_name(self_id, "__suppress_context__", Value::Bool(false), heap, interns)?;

    limit.drop_with_heap(heap);
    chain.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

/// Implements `TracebackException.format(chain=True)`.
fn traceback_exception_format(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, self_value, method_args) = extract_instance_self_and_args(args, heap, "TracebackException.format")?;
    let chain_arg = method_args.get_zero_one_arg("TracebackException.format", heap)?;
    let chain = parse_bool_or_default(chain_arg.as_ref(), true, heap, interns);
    chain_arg.drop_with_heap(heap);

    let exc_type = get_instance_attr_by_name(self_id, "exc_type", heap, interns).unwrap_or(Value::None);
    let exc_value = get_instance_attr_by_name(self_id, "exc_value", heap, interns).unwrap_or(Value::None);
    let stack = get_instance_attr_by_name(self_id, "_stack", heap, interns).unwrap_or(Value::None);
    let chain_source = get_instance_attr_by_name(self_id, "_chain", heap, interns).unwrap_or(Value::Bool(chain));
    let use_chain = parse_bool_or_default(Some(&chain_source), chain, heap, interns);
    let _ = use_chain;

    let lines = format_exception_lines(Some(&exc_type), Some(&exc_value), Some(&stack), None, heap, interns);
    let result = list_from_strings(lines, heap);
    exc_type.drop_with_heap(heap);
    exc_value.drop_with_heap(heap);
    stack.drop_with_heap(heap);
    chain_source.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    result
}

/// Implements `TracebackException.format_exception_only()`.
fn traceback_exception_format_exception_only(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, self_value, method_args) =
        extract_instance_self_and_args(args, heap, "TracebackException.format_exception_only")?;
    method_args.check_zero_args("TracebackException.format_exception_only", heap)?;
    let exc_type = get_instance_attr_by_name(self_id, "exc_type", heap, interns).unwrap_or(Value::None);
    let exc_value = get_instance_attr_by_name(self_id, "exc_value", heap, interns).unwrap_or(Value::None);
    let line = format_exception_only_line(Some(&exc_type), Some(&exc_value), heap, interns);
    let result = list_from_strings(vec![line], heap);
    exc_type.drop_with_heap(heap);
    exc_value.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    result
}
