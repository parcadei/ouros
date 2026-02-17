//! Sandboxed compatibility implementation of Python's `logging` module.
//!
//! The real CPython module is large and class-heavy. This implementation focuses
//! on behavior that stdlib consumers rely on most often:
//! - stable logger identity via `getLogger(name)`
//! - level name mapping helpers (`addLevelName`, `getLevelName`, `getLevelNamesMapping`)
//! - root-manager disable threshold (`disable`)
//! - logger instance methods (`setLevel`, `isEnabledFor`, `getEffectiveLevel`,
//!   `addHandler`, `removeHandler`, `hasHandlers`, `getChild`)
//! - module-level convenience wrappers (`debug`, `info`, `warning`, ...)
//!
//! The module remains fully sandbox-safe: it does not perform filesystem, network,
//! subprocess, or host-environment access.

use std::sync::{Mutex, OnceLock};

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, FrozenSet, List, Module, Partial, PyTrait, Set, Str, Type, allocate_tuple,
        compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// `logging` module callables exposed by Ouros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum LoggingFunctions {
    Getlogger,
    Basicconfig,
    Shutdown,
    Disable,
    Capturewarnings,
    Addlevelname,
    Getlevelname,
    Getlevelnamesmapping,
    Getloggerclass,
    Setloggerclass,
    Getlogrecordfactory,
    Setlogrecordfactory,
    Makelogrecord,
    Gethandlerbyname,
    Gethandlernames,
    Currentframe,
    Fatal,
    Debug,
    Info,
    Warning,
    Warn,
    Error,
    Exception,
    Critical,
    Log,
    Loggerdebug,
    Loggerinfo,
    Loggerwarning,
    Loggerwarn,
    Loggererror,
    Loggerexception,
    Loggercritical,
    Loggerfatal,
    Loggerlog,
    Loggersetlevel,
    Loggeraddhandler,
    Loggerremovehandler,
    Loggergeteffectivelevel,
    Loggerisenabledfor,
    Loggerhashandlers,
    Loggergetchild,
    Handlersetlevel,
    Handlersetname,
    Handlergetname,
    Logrecordgetmessage,
}

/// Level values stored in the runtime level-name registry.
#[derive(Debug, Clone)]
enum LevelValue {
    Int(i64),
    Text(String),
}

/// Runtime state used by the logging shim.
#[derive(Debug, Default)]
struct LoggingRuntimeState {
    module_id: Option<HeapId>,
    root_logger_id: Option<HeapId>,
    manager_id: Option<HeapId>,
    logger_ids: Vec<(String, HeapId)>,
    handler_ids: Vec<HeapId>,
    disable: i64,
    capture_warnings: bool,
    level_to_name_int: Vec<(i64, String)>,
    level_to_name_text: Vec<(String, String)>,
    name_to_level: Vec<(String, LevelValue)>,
}

/// Global mutable logging state.
static LOGGING_STATE: OnceLock<Mutex<LoggingRuntimeState>> = OnceLock::new();

/// Returns the process-global logging state mutex.
fn logging_state() -> &'static Mutex<LoggingRuntimeState> {
    LOGGING_STATE.get_or_init(|| Mutex::new(LoggingRuntimeState::default()))
}

/// Clones a heap reference into a `Value::Ref` without constructing a temporary
/// `Value::Ref` that would panic on drop in `ref-count-panic` mode.
fn clone_ref_value(id: HeapId, heap: &Heap<impl ResourceTracker>) -> Value {
    heap.inc_ref(id);
    Value::Ref(id)
}

/// Initializes canonical level-name mappings.
fn reset_level_maps(state: &mut LoggingRuntimeState) {
    state.level_to_name_int = vec![
        (50, "CRITICAL".to_owned()),
        (40, "ERROR".to_owned()),
        (30, "WARNING".to_owned()),
        (20, "INFO".to_owned()),
        (10, "DEBUG".to_owned()),
        (0, "NOTSET".to_owned()),
    ];
    state.level_to_name_text.clear();
    state.name_to_level = vec![
        ("CRITICAL".to_owned(), LevelValue::Int(50)),
        ("FATAL".to_owned(), LevelValue::Int(50)),
        ("ERROR".to_owned(), LevelValue::Int(40)),
        ("WARNING".to_owned(), LevelValue::Int(30)),
        ("WARN".to_owned(), LevelValue::Int(30)),
        ("INFO".to_owned(), LevelValue::Int(20)),
        ("DEBUG".to_owned(), LevelValue::Int(10)),
        ("NOTSET".to_owned(), LevelValue::Int(0)),
    ];
}

/// Drops stale heap IDs from state maps.
fn prune_dead_ids(state: &mut LoggingRuntimeState, heap: &Heap<impl ResourceTracker>) {
    state.logger_ids.retain(|(_, id)| heap.get_if_live(*id).is_some());
    state.handler_ids.retain(|id| heap.get_if_live(*id).is_some());

    if let Some(root_id) = state.root_logger_id
        && heap.get_if_live(root_id).is_none()
    {
        state.root_logger_id = None;
    }
    if let Some(manager_id) = state.manager_id
        && heap.get_if_live(manager_id).is_none()
    {
        state.manager_id = None;
    }
    if let Some(module_id) = state.module_id
        && heap.get_if_live(module_id).is_none()
    {
        state.module_id = None;
    }
}

/// Creates the `logging` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let manager_id = create_manager_object(heap, interns)?;
    let root_logger_id = create_logger_object("root", 30, manager_id, None, heap, interns)?;
    let last_resort_id = create_handler_object("_StderrHandler", 30, heap, interns)?;

    // Core class placeholders used by type checks and setLoggerClass parity.
    let object_base = heap.builtin_class_id(Type::Object)?;
    let logger_class_id = create_logging_class(heap, interns, "Logger", vec![object_base])?;
    let root_logger_class_id = create_logging_class(heap, interns, "RootLogger", vec![logger_class_id])?;
    let log_record_class_id = create_logging_class(heap, interns, "LogRecord", vec![object_base])?;
    let handler_class_id = create_logging_class(heap, interns, "Handler", vec![object_base])?;
    let stream_handler_class_id = create_logging_class(heap, interns, "StreamHandler", vec![handler_class_id])?;
    let file_handler_class_id = create_logging_class(heap, interns, "FileHandler", vec![handler_class_id])?;
    let null_handler_class_id = create_logging_class(heap, interns, "NullHandler", vec![handler_class_id])?;
    let formatter_class_id = create_logging_class(heap, interns, "Formatter", vec![object_base])?;
    let buffering_formatter_class_id =
        create_logging_class(heap, interns, "BufferingFormatter", vec![formatter_class_id])?;
    let filter_class_id = create_logging_class(heap, interns, "Filter", vec![object_base])?;
    let filterer_class_id = create_logging_class(heap, interns, "Filterer", vec![object_base])?;
    let logger_adapter_class_id = create_logging_class(heap, interns, "LoggerAdapter", vec![object_base])?;
    let manager_class_id = create_logging_class(heap, interns, "Manager", vec![object_base])?;
    let placeholder_class_id = create_logging_class(heap, interns, "PlaceHolder", vec![object_base])?;
    let percent_style_class_id = create_logging_class(heap, interns, "PercentStyle", vec![object_base])?;
    let str_style_class_id = create_logging_class(heap, interns, "StrFormatStyle", vec![object_base])?;
    let template_style_class_id = create_logging_class(heap, interns, "StringTemplateStyle", vec![object_base])?;

    let mut module = Module::new(StaticStrings::Logging);

    // Public module-level functions.
    register(&mut module, "getLogger", LoggingFunctions::Getlogger, heap, interns)?;
    register(&mut module, "basicConfig", LoggingFunctions::Basicconfig, heap, interns)?;
    register(&mut module, "shutdown", LoggingFunctions::Shutdown, heap, interns)?;
    register(&mut module, "disable", LoggingFunctions::Disable, heap, interns)?;
    register(
        &mut module,
        "captureWarnings",
        LoggingFunctions::Capturewarnings,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "addLevelName",
        LoggingFunctions::Addlevelname,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "getLevelName",
        LoggingFunctions::Getlevelname,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "getLevelNamesMapping",
        LoggingFunctions::Getlevelnamesmapping,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "getLoggerClass",
        LoggingFunctions::Getloggerclass,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "setLoggerClass",
        LoggingFunctions::Setloggerclass,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "getLogRecordFactory",
        LoggingFunctions::Getlogrecordfactory,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "setLogRecordFactory",
        LoggingFunctions::Setlogrecordfactory,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "makeLogRecord",
        LoggingFunctions::Makelogrecord,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "getHandlerByName",
        LoggingFunctions::Gethandlerbyname,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "getHandlerNames",
        LoggingFunctions::Gethandlernames,
        heap,
        interns,
    )?;
    register(
        &mut module,
        "currentframe",
        LoggingFunctions::Currentframe,
        heap,
        interns,
    )?;
    register(&mut module, "fatal", LoggingFunctions::Fatal, heap, interns)?;
    register(&mut module, "debug", LoggingFunctions::Debug, heap, interns)?;
    register(&mut module, "info", LoggingFunctions::Info, heap, interns)?;
    register(&mut module, "warning", LoggingFunctions::Warning, heap, interns)?;
    register(&mut module, "warn", LoggingFunctions::Warn, heap, interns)?;
    register(&mut module, "error", LoggingFunctions::Error, heap, interns)?;
    register(&mut module, "exception", LoggingFunctions::Exception, heap, interns)?;
    register(&mut module, "critical", LoggingFunctions::Critical, heap, interns)?;
    register(&mut module, "log", LoggingFunctions::Log, heap, interns)?;

    // Common level constants.
    module.set_attr_text("CRITICAL", Value::Int(50), heap, interns)?;
    module.set_attr_text("FATAL", Value::Int(50), heap, interns)?;
    module.set_attr_text("ERROR", Value::Int(40), heap, interns)?;
    module.set_attr_text("WARNING", Value::Int(30), heap, interns)?;
    module.set_attr_text("WARN", Value::Int(30), heap, interns)?;
    module.set_attr_text("INFO", Value::Int(20), heap, interns)?;
    module.set_attr_text("DEBUG", Value::Int(10), heap, interns)?;
    module.set_attr_text("NOTSET", Value::Int(0), heap, interns)?;

    let basic_format = heap.allocate(HeapData::Str(Str::from("%(levelname)s:%(name)s:%(message)s")))?;
    module.set_attr_text("BASIC_FORMAT", Value::Ref(basic_format), heap, interns)?;

    // Class-like exports.
    module.set_attr_text("Logger", Value::Ref(logger_class_id), heap, interns)?;
    module.set_attr_text("RootLogger", Value::Ref(root_logger_class_id), heap, interns)?;
    module.set_attr_text("LogRecord", Value::Ref(log_record_class_id), heap, interns)?;
    module.set_attr_text("Handler", Value::Ref(handler_class_id), heap, interns)?;
    module.set_attr_text("StreamHandler", Value::Ref(stream_handler_class_id), heap, interns)?;
    module.set_attr_text("FileHandler", Value::Ref(file_handler_class_id), heap, interns)?;
    module.set_attr_text("NullHandler", Value::Ref(null_handler_class_id), heap, interns)?;
    module.set_attr_text("Formatter", Value::Ref(formatter_class_id), heap, interns)?;
    module.set_attr_text(
        "BufferingFormatter",
        Value::Ref(buffering_formatter_class_id),
        heap,
        interns,
    )?;
    module.set_attr_text("Filter", Value::Ref(filter_class_id), heap, interns)?;
    module.set_attr_text("Filterer", Value::Ref(filterer_class_id), heap, interns)?;
    module.set_attr_text("LoggerAdapter", Value::Ref(logger_adapter_class_id), heap, interns)?;
    module.set_attr_text("Manager", Value::Ref(manager_class_id), heap, interns)?;
    module.set_attr_text("PlaceHolder", Value::Ref(placeholder_class_id), heap, interns)?;
    module.set_attr_text("PercentStyle", Value::Ref(percent_style_class_id), heap, interns)?;
    module.set_attr_text("StrFormatStyle", Value::Ref(str_style_class_id), heap, interns)?;
    module.set_attr_text(
        "StringTemplateStyle",
        Value::Ref(template_style_class_id),
        heap,
        interns,
    )?;

    // Misc public exports present in CPython's module namespace.
    module.set_attr_text("Template", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text(
        "GenericAlias",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text("atexit", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text(
        "collections",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text("io", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("os", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("re", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("sys", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("threading", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("time", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("traceback", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("warnings", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    module.set_attr_text("weakref", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;

    module.set_attr_text("raiseExceptions", Value::Bool(true), heap, interns)?;
    module.set_attr_text("logThreads", Value::Bool(true), heap, interns)?;
    module.set_attr_text("logMultiprocessing", Value::Bool(true), heap, interns)?;
    module.set_attr_text("logProcesses", Value::Bool(true), heap, interns)?;
    module.set_attr_text("logAsyncioTasks", Value::Bool(true), heap, interns)?;

    module.set_attr_text(
        "__logger_class__",
        clone_ref_value(logger_class_id, heap),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "__log_record_factory__",
        clone_ref_value(log_record_class_id, heap),
        heap,
        interns,
    )?;

    module.set_attr_text("root", Value::Ref(root_logger_id), heap, interns)?;
    module.set_attr_text("lastResort", Value::Ref(last_resort_id), heap, interns)?;

    let module_id = heap.allocate(HeapData::Module(module))?;

    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    *state = LoggingRuntimeState::default();
    state.module_id = Some(module_id);
    state.manager_id = Some(manager_id);
    state.root_logger_id = Some(root_logger_id);
    state.logger_ids.push(("root".to_owned(), root_logger_id));
    state.handler_ids.push(last_resort_id);
    state.disable = 0;
    state.capture_warnings = false;
    reset_level_maps(&mut state);

    Ok(module_id)
}

/// Dispatches `logging` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: LoggingFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        LoggingFunctions::Getlogger => get_logger(heap, interns, args),
        LoggingFunctions::Basicconfig => basic_config(heap, interns, args),
        LoggingFunctions::Shutdown => shutdown(heap, args),
        LoggingFunctions::Disable => disable(heap, interns, args),
        LoggingFunctions::Capturewarnings => capture_warnings(heap, interns, args),
        LoggingFunctions::Addlevelname => add_level_name(heap, interns, args),
        LoggingFunctions::Getlevelname => get_level_name(heap, interns, args),
        LoggingFunctions::Getlevelnamesmapping => get_level_names_mapping(heap, interns),
        LoggingFunctions::Getloggerclass => get_logger_class(heap, interns, args),
        LoggingFunctions::Setloggerclass => set_logger_class(heap, interns, args),
        LoggingFunctions::Getlogrecordfactory => get_log_record_factory(heap, interns, args),
        LoggingFunctions::Setlogrecordfactory => set_log_record_factory(heap, interns, args),
        LoggingFunctions::Makelogrecord => make_log_record(heap, interns, args),
        LoggingFunctions::Gethandlerbyname => get_handler_by_name(heap, interns, args),
        LoggingFunctions::Gethandlernames => get_handler_names(heap, interns, args),
        LoggingFunctions::Currentframe => {
            args.drop_with_heap(heap);
            Ok(AttrCallResult::Value(Value::None))
        }
        LoggingFunctions::Fatal => module_level_log_at_level(heap, interns, args, 50, "logging.fatal"),
        LoggingFunctions::Debug => module_level_log_at_level(heap, interns, args, 10, "logging.debug"),
        LoggingFunctions::Info => module_level_log_at_level(heap, interns, args, 20, "logging.info"),
        LoggingFunctions::Warning => module_level_log_at_level(heap, interns, args, 30, "logging.warning"),
        LoggingFunctions::Warn => module_level_log_at_level(heap, interns, args, 30, "logging.warn"),
        LoggingFunctions::Error => module_level_log_at_level(heap, interns, args, 40, "logging.error"),
        LoggingFunctions::Exception => module_level_log_at_level(heap, interns, args, 40, "logging.exception"),
        LoggingFunctions::Critical => module_level_log_at_level(heap, interns, args, 50, "logging.critical"),
        LoggingFunctions::Log => module_level_log(heap, interns, args),
        LoggingFunctions::Loggerdebug => logger_log_at_level(heap, interns, args, 10, "Logger.debug"),
        LoggingFunctions::Loggerinfo => logger_log_at_level(heap, interns, args, 20, "Logger.info"),
        LoggingFunctions::Loggerwarning => logger_log_at_level(heap, interns, args, 30, "Logger.warning"),
        LoggingFunctions::Loggerwarn => logger_log_at_level(heap, interns, args, 30, "Logger.warn"),
        LoggingFunctions::Loggererror => logger_log_at_level(heap, interns, args, 40, "Logger.error"),
        LoggingFunctions::Loggerexception => logger_log_at_level(heap, interns, args, 40, "Logger.exception"),
        LoggingFunctions::Loggercritical => logger_log_at_level(heap, interns, args, 50, "Logger.critical"),
        LoggingFunctions::Loggerfatal => logger_log_at_level(heap, interns, args, 50, "Logger.fatal"),
        LoggingFunctions::Loggerlog => logger_log(heap, interns, args),
        LoggingFunctions::Loggersetlevel => logger_set_level(heap, interns, args),
        LoggingFunctions::Loggeraddhandler => logger_add_handler(heap, interns, args),
        LoggingFunctions::Loggerremovehandler => logger_remove_handler(heap, interns, args),
        LoggingFunctions::Loggergeteffectivelevel => logger_get_effective_level(heap, interns, args),
        LoggingFunctions::Loggerisenabledfor => logger_is_enabled_for(heap, interns, args),
        LoggingFunctions::Loggerhashandlers => logger_has_handlers(heap, interns, args),
        LoggingFunctions::Loggergetchild => logger_get_child(heap, interns, args),
        LoggingFunctions::Handlersetlevel => handler_set_level(heap, interns, args),
        LoggingFunctions::Handlersetname => handler_set_name(heap, interns, args),
        LoggingFunctions::Handlergetname => handler_get_name(heap, interns, args),
        LoggingFunctions::Logrecordgetmessage => log_record_get_message(heap, interns, args),
    }
}

/// Registers one module-level function.
fn register(
    module: &mut Module,
    name: &str,
    function: LoggingFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Logging(function)),
        heap,
        interns,
    )
}

/// Creates a class object with explicit base class IDs.
fn create_logging_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    class_name: &str,
    bases: Vec<HeapId>,
) -> Result<HeapId, ResourceError> {
    for base_id in &bases {
        heap.inc_ref(*base_id);
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap(class_name.to_owned()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        bases.clone(),
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &bases, heap, interns).expect("logging class MRO must be valid");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    for &base_id in &bases {
        let _ = heap.with_entry_mut(base_id, |_, data| {
            let HeapData::ClassObject(base_cls) = data else {
                return Err(ExcType::type_error("logging base is not a class".to_string()));
            };
            base_cls.register_subclass(class_id, class_uid);
            Ok(())
        });
    }

    Ok(class_id)
}

/// Creates a minimal manager object exposing `disable`.
fn create_manager_object(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut manager = Module::new(StaticStrings::Logging);
    manager.set_attr_text("disable", Value::Int(0), heap, interns)?;
    let logger_dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    manager.set_attr_text("loggerDict", Value::Ref(logger_dict_id), heap, interns)?;
    heap.allocate(HeapData::Module(manager))
}

/// Creates a logger object module with bound method callables.
fn create_logger_object(
    name: &str,
    level: i64,
    manager_id: HeapId,
    parent_id: Option<HeapId>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut logger = Module::new(StaticStrings::Logging);
    logger.set_attr_text(
        "name",
        Value::Ref(heap.allocate(HeapData::Str(Str::from(name)))?),
        heap,
        interns,
    )?;
    logger.set_attr_text("level", Value::Int(level), heap, interns)?;
    logger.set_attr_text("propagate", Value::Bool(true), heap, interns)?;
    logger.set_attr_text("disabled", Value::Bool(false), heap, interns)?;
    logger.set_attr_text("manager", clone_ref_value(manager_id, heap), heap, interns)?;
    logger.set_attr_text(
        "parent",
        parent_id.map_or(Value::None, |id| clone_ref_value(id, heap)),
        heap,
        interns,
    )?;
    logger.set_attr_text(
        "handlers",
        Value::Ref(heap.allocate(HeapData::List(List::new(Vec::new())))?),
        heap,
        interns,
    )?;

    let logger_id = heap.allocate(HeapData::Module(logger))?;

    let _ = heap.with_entry_mut(logger_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error(
                "internal logging object is not a module".to_string(),
            ));
        };

        set_bound_method(module, "debug", LoggingFunctions::Loggerdebug, logger_id, heap, interns)?;
        set_bound_method(module, "info", LoggingFunctions::Loggerinfo, logger_id, heap, interns)?;
        set_bound_method(
            module,
            "warning",
            LoggingFunctions::Loggerwarning,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(module, "warn", LoggingFunctions::Loggerwarn, logger_id, heap, interns)?;
        set_bound_method(module, "error", LoggingFunctions::Loggererror, logger_id, heap, interns)?;
        set_bound_method(
            module,
            "exception",
            LoggingFunctions::Loggerexception,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "critical",
            LoggingFunctions::Loggercritical,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(module, "fatal", LoggingFunctions::Loggerfatal, logger_id, heap, interns)?;
        set_bound_method(module, "log", LoggingFunctions::Loggerlog, logger_id, heap, interns)?;
        set_bound_method(
            module,
            "setLevel",
            LoggingFunctions::Loggersetlevel,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "addHandler",
            LoggingFunctions::Loggeraddhandler,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "removeHandler",
            LoggingFunctions::Loggerremovehandler,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "getEffectiveLevel",
            LoggingFunctions::Loggergeteffectivelevel,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "isEnabledFor",
            LoggingFunctions::Loggerisenabledfor,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "hasHandlers",
            LoggingFunctions::Loggerhashandlers,
            logger_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "getChild",
            LoggingFunctions::Loggergetchild,
            logger_id,
            heap,
            interns,
        )?;

        Ok(())
    });

    Ok(logger_id)
}

/// Creates a minimal handler object with naming/level methods.
fn create_handler_object(
    kind: &str,
    level: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut handler = Module::new(StaticStrings::Logging);
    handler.set_attr_text(
        "kind",
        Value::Ref(heap.allocate(HeapData::Str(Str::from(kind)))?),
        heap,
        interns,
    )?;
    handler.set_attr_text("level", Value::Int(level), heap, interns)?;
    handler.set_attr_text("name", Value::None, heap, interns)?;

    let handler_id = heap.allocate(HeapData::Module(handler))?;

    let _ = heap.with_entry_mut(handler_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error(
                "internal handler object is not a module".to_string(),
            ));
        };
        set_bound_method(
            module,
            "setLevel",
            LoggingFunctions::Handlersetlevel,
            handler_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "set_name",
            LoggingFunctions::Handlersetname,
            handler_id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "get_name",
            LoggingFunctions::Handlergetname,
            handler_id,
            heap,
            interns,
        )?;
        Ok(())
    });

    Ok(handler_id)
}

/// Adds one bound partial method to a module-like object.
fn set_bound_method(
    module: &mut Module,
    method_name: &str,
    function: LoggingFunctions,
    self_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Logging(function)),
        vec![clone_ref_value(self_id, heap)],
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    module.set_attr_text(method_name, Value::Ref(partial_id), heap, interns)
}

/// Reads a module attribute from an object module by heap id.
fn module_attr(
    module_id: HeapId,
    attr_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    heap.with_entry_mut(module_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Ok(None);
        };
        Ok(module
            .attrs()
            .get_by_str(attr_name, heap, interns)
            .map(|value| value.clone_with_heap(heap)))
    })
}

/// Writes a module attribute on an object module by heap id.
fn set_module_attr(
    module_id: HeapId,
    attr_name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    heap.with_entry_mut(module_id, |heap, data| {
        let HeapData::Module(module) = data else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "internal logging object is not a module".to_string(),
            ));
        };
        module
            .set_attr_text(attr_name, value, heap, interns)
            .map_err(crate::exception_private::RunError::from)
    })
}

/// Returns a compact `ArgValues` from explicit positional and keyword values.
fn args_from_vec_kwargs(mut args: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    if kwargs.is_empty() {
        return match args.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(args.pop().expect("length checked")),
            2 => {
                let second = args.pop().expect("length checked");
                let first = args.pop().expect("length checked");
                ArgValues::Two(first, second)
            }
            _ => ArgValues::ArgsKargs { args, kwargs },
        };
    }
    ArgValues::ArgsKargs { args, kwargs }
}

/// Returns true if `value` is a class object for `setLoggerClass` validation.
fn is_class_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(value, Value::Builtin(Builtins::Type(_)))
        || matches!(value, Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)))
}

/// Returns a human-readable class name for error messages.
fn class_name_for_error(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> String {
    match value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::ClassObject(cls) => cls.name(interns).to_owned(),
            _ => value.py_type(heap).to_string(),
        },
        Value::Builtin(Builtins::Type(ty)) => ty.to_string(),
        _ => value.py_type(heap).to_string(),
    }
}

/// Returns whether `candidate` is a subclass of `base` under Ouros class semantics.
fn is_subclass_of(candidate: &Value, base: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match (candidate, base) {
        (Value::Ref(candidate_id), Value::Ref(base_id)) => {
            let HeapData::ClassObject(candidate_cls) = heap.get(*candidate_id) else {
                return false;
            };
            candidate_cls.mro().contains(base_id)
        }
        (Value::Builtin(Builtins::Type(candidate_ty)), Value::Builtin(Builtins::Type(base_ty))) => {
            candidate_ty == base_ty || *base_ty == Type::Object
        }
        _ => false,
    }
}

/// Returns the module id for the active logging module.
fn logging_module_id(heap: &Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state
        .module_id
        .ok_or_else(|| SimpleException::new_msg(ExcType::RuntimeError, "logging module not initialized").into())
}

/// Returns root logger id from runtime state.
fn root_logger_id(heap: &Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state
        .root_logger_id
        .ok_or_else(|| SimpleException::new_msg(ExcType::RuntimeError, "logging root not initialized").into())
}

/// Returns manager object id from runtime state.
fn manager_id(heap: &Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state
        .manager_id
        .ok_or_else(|| SimpleException::new_msg(ExcType::RuntimeError, "logging manager not initialized").into())
}

/// Returns a logger name for a known logger id.
fn logger_name_for_id(heap: &Heap<impl ResourceTracker>, logger_id: HeapId) -> Option<String> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state
        .logger_ids
        .iter()
        .find_map(|(name, id)| (*id == logger_id).then_some(name.clone()))
}

/// Returns a logger id by full logger name.
fn logger_id_for_name(heap: &Heap<impl ResourceTracker>, name: &str) -> Option<HeapId> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state
        .logger_ids
        .iter()
        .find_map(|(logger_name, id)| (logger_name == name).then_some(*id))
}

/// Returns ancestor logger ids for hierarchy lookups.
fn ancestor_logger_ids(heap: &Heap<impl ResourceTracker>, logger_name: &str) -> Vec<HeapId> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);

    let mut ids = Vec::new();
    if let Some(id) = state
        .logger_ids
        .iter()
        .find_map(|(name, id)| (name == logger_name).then_some(*id))
    {
        ids.push(id);
    }

    let mut search = logger_name;
    while let Some(dot_index) = search.rfind('.') {
        search = &search[..dot_index];
        if let Some(id) = state
            .logger_ids
            .iter()
            .find_map(|(name, id)| (name == search).then_some(*id))
        {
            ids.push(id);
        }
    }

    if let Some(root_id) = state.root_logger_id
        && !ids.contains(&root_id)
    {
        ids.push(root_id);
    }

    ids
}

/// Creates or fetches a cached logger by name.
fn get_or_create_logger_id(name: &str, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        if let Some(existing_id) = state
            .logger_ids
            .iter()
            .find_map(|(logger_name, id)| (logger_name == name).then_some(*id))
        {
            return Ok(existing_id);
        }
    }

    let manager = manager_id(heap)?;
    let root = root_logger_id(heap)?;

    // Choose the nearest existing ancestor logger as `parent` attribute.
    let mut parent_id = root;
    let mut search = name;
    while let Some(dot_index) = search.rfind('.') {
        search = &search[..dot_index];
        if let Some(found_id) = logger_id_for_name(heap, search) {
            parent_id = found_id;
            break;
        }
    }

    let logger_id = create_logger_object(name, 0, manager, Some(parent_id), heap, interns)?;

    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state.logger_ids.push((name.to_owned(), logger_id));

    Ok(logger_id)
}

/// Extracts one logger self argument from a bound logger method call.
fn take_logger_self(
    args: ArgValues,
    method_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<(HeapId, Vec<Value>, KwargsValues)> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(self_value) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 1, 0));
    };
    let logger_id = if let Value::Ref(id) = &self_value {
        *id
    } else {
        self_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("{method_name} expected logger instance")));
    };
    self_value.drop_with_heap(heap);

    if !matches!(heap.get_if_live(logger_id), Some(HeapData::Module(_))) {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::RuntimeError, "stale logger reference").into());
    }

    Ok((logger_id, positional.collect(), kwargs))
}

/// Reads a logger integer attribute with a default fallback.
fn logger_attr_int(
    logger_id: HeapId,
    attr_name: &str,
    default: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    let Some(value) = module_attr(logger_id, attr_name, heap, interns)? else {
        return Ok(default);
    };
    let parsed = value.as_int(heap).unwrap_or(default);
    value.drop_with_heap(heap);
    Ok(parsed)
}

/// Reads a logger boolean attribute with a default fallback.
fn logger_attr_bool(
    logger_id: HeapId,
    attr_name: &str,
    default: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    let Some(value) = module_attr(logger_id, attr_name, heap, interns)? else {
        return Ok(default);
    };
    let parsed = value.py_bool(heap, interns);
    value.drop_with_heap(heap);
    Ok(parsed)
}

/// Reads handler list values from a logger object.
fn logger_handlers(
    logger_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let Some(handlers_attr) = module_attr(logger_id, "handlers", heap, interns)? else {
        return Ok(Vec::new());
    };
    let list_id = if let Value::Ref(id) = &handlers_attr {
        *id
    } else {
        handlers_attr.drop_with_heap(heap);
        return Ok(Vec::new());
    };
    handlers_attr.drop_with_heap(heap);

    heap.with_entry_mut(list_id, |heap, data| {
        let HeapData::List(list) = data else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(list.len());
        for item in list.as_vec() {
            out.push(item.clone_with_heap(heap));
        }
        Ok(out)
    })
}

/// Replaces logger handlers with a freshly allocated list.
fn set_logger_handlers(
    logger_id: HeapId,
    handlers: Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let list_id = heap.allocate(HeapData::List(List::new(handlers)))?;
    set_module_attr(logger_id, "handlers", Value::Ref(list_id), heap, interns)
}

/// Parses a strict logging level (`_checkLevel`) for APIs like `disable`/`setLevel`.
fn check_level(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<i64> {
    if let Ok(level) = value.as_int(heap) {
        return Ok(level);
    }

    if let Some(level_name) = value.as_either_str(heap) {
        let level_name = level_name.as_str(interns).to_owned();
        let state = logging_state().lock().expect("logging state mutex poisoned");
        if let Some((_, LevelValue::Int(level))) = state.name_to_level.iter().find(|(name, _)| name == &level_name) {
            return Ok(*level);
        }
        return Err(SimpleException::new_msg(ExcType::ValueError, format!("Unknown level: '{level_name}'")).into());
    }

    let rendered = value.py_repr(heap, interns).into_owned();
    Err(SimpleException::new_msg(
        ExcType::TypeError,
        format!("Level not an integer or a valid string: {rendered}"),
    )
    .into())
}

/// Converts a level value into the internal addLevelName registry representation.
fn add_level_value(value: &Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> LevelValue {
    if let Ok(level) = value.as_int(heap) {
        return LevelValue::Int(level);
    }
    LevelValue::Text(value.py_str(heap, interns).into_owned())
}

/// Implements `logging.getLogger([name])`.
fn get_logger(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let name_arg = args.get_zero_one_arg("getLogger", heap)?;

    let logger_id = match name_arg {
        None => root_logger_id(heap)?,
        Some(value) => {
            if matches!(value, Value::None) {
                value.drop_with_heap(heap);
                root_logger_id(heap)?
            } else {
                let Some(name_value) = value.as_either_str(heap) else {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error("A logger name must be a string"));
                };
                let name = name_value.as_str(interns).to_owned();
                value.drop_with_heap(heap);

                if name.is_empty() || name == "root" {
                    root_logger_id(heap)?
                } else {
                    get_or_create_logger_id(&name, heap, interns)?
                }
            }
        }
    };

    Ok(AttrCallResult::Value(clone_ref_value(logger_id, heap)))
}

/// Implements `logging.getLoggerClass()`.
fn get_logger_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    args.check_zero_args("getLoggerClass", heap)?;
    let module_id = logging_module_id(heap)?;

    if let Some(value) = module_attr(module_id, "__logger_class__", heap, interns)? {
        return Ok(AttrCallResult::Value(value));
    }
    if let Some(value) = module_attr(module_id, "Logger", heap, interns)? {
        return Ok(AttrCallResult::Value(value));
    }

    Ok(AttrCallResult::Value(Value::Builtin(Builtins::Type(Type::Object))))
}

/// Implements `logging.setLoggerClass(cls)`.
fn set_logger_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("setLoggerClass", heap)?;

    if !is_class_value(&cls, heap) {
        cls.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "issubclass() arg 1 must be a class").into());
    }

    let module_id = logging_module_id(heap)?;
    let base_logger =
        module_attr(module_id, "Logger", heap, interns)?.unwrap_or(Value::Builtin(Builtins::Type(Type::Object)));

    if !is_subclass_of(&cls, &base_logger, heap) {
        let cls_name = class_name_for_error(&cls, heap, interns);
        cls.drop_with_heap(heap);
        base_logger.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("logger not derived from logging.Logger: {cls_name}"),
        )
        .into());
    }

    base_logger.drop_with_heap(heap);
    set_module_attr(module_id, "__logger_class__", cls, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.getLogRecordFactory()`.
fn get_log_record_factory(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    args.check_zero_args("getLogRecordFactory", heap)?;
    let module_id = logging_module_id(heap)?;
    if let Some(value) = module_attr(module_id, "__log_record_factory__", heap, interns)? {
        return Ok(AttrCallResult::Value(value));
    }
    Ok(AttrCallResult::Value(Value::Builtin(Builtins::Type(Type::Object))))
}

/// Implements `logging.setLogRecordFactory(factory)`.
fn set_log_record_factory(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let factory = args.get_one_arg("setLogRecordFactory", heap)?;
    let module_id = logging_module_id(heap)?;
    set_module_attr(module_id, "__log_record_factory__", factory, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.makeLogRecord(mapping)`.
fn make_log_record(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let mapping = args.get_one_arg("makeLogRecord", heap)?;

    let module_id = logging_module_id(heap)?;
    let factory = module_attr(module_id, "__log_record_factory__", heap, interns)?
        .unwrap_or(Value::Builtin(Builtins::Type(Type::Object)));
    let default_factory =
        module_attr(module_id, "LogRecord", heap, interns)?.unwrap_or(Value::Builtin(Builtins::Type(Type::Object)));

    let is_default = matches!((&factory, &default_factory), (Value::Ref(a), Value::Ref(b)) if a == b);
    default_factory.drop_with_heap(heap);

    if !is_default {
        // CPython calls the factory with default ctor-style args; mapping updates happen
        // afterwards. We model the constructor call here and keep mapping handling for the
        // default factory path.
        let empty_str = Value::Ref(heap.allocate(HeapData::Str(Str::from("")))?);
        let empty_tuple = allocate_tuple(smallvec::smallvec![], heap)?;
        let forwarded = args_from_vec_kwargs(
            vec![
                Value::None,
                Value::None,
                empty_str.clone_with_heap(heap),
                Value::Int(0),
                empty_str,
                empty_tuple,
                Value::None,
                Value::None,
            ],
            KwargsValues::Empty,
        );
        mapping.drop_with_heap(heap);
        return Ok(AttrCallResult::CallFunction(factory, forwarded));
    }

    factory.drop_with_heap(heap);

    let mapping_id = if let Value::Ref(id) = &mapping {
        *id
    } else {
        mapping.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "makeLogRecord() argument must be a dict").into());
    };

    let record_id = create_log_record_from_dict(mapping_id, heap, interns)?;
    mapping.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Ref(record_id)))
}

/// Creates a lightweight record object from a dict payload.
fn create_log_record_from_dict(
    mapping_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<HeapId> {
    let mut record = Module::new(StaticStrings::Logging);

    let defaults: [(&str, Value); 10] = [
        ("name", Value::Ref(heap.allocate(HeapData::Str(Str::from("root")))?)),
        ("msg", Value::Ref(heap.allocate(HeapData::Str(Str::from("")))?)),
        ("args", allocate_tuple(smallvec::smallvec![], heap)?),
        (
            "levelname",
            Value::Ref(heap.allocate(HeapData::Str(Str::from("NOTSET")))?),
        ),
        ("levelno", Value::Int(0)),
        ("pathname", Value::Ref(heap.allocate(HeapData::Str(Str::from("")))?)),
        ("lineno", Value::Int(0)),
        ("exc_info", Value::None),
        ("func", Value::None),
        ("sinfo", Value::None),
    ];

    for (name, default_value) in defaults {
        record.set_attr_text(name, default_value, heap, interns)?;
    }

    let copied_fields: Vec<(String, Value)> =
        heap.with_entry_mut(mapping_id, |heap, data| -> RunResult<Vec<(String, Value)>> {
            let HeapData::Dict(mapping) = data else {
                return Ok(Vec::new());
            };

            let mut out = Vec::new();
            for key in [
                "name",
                "msg",
                "args",
                "levelname",
                "levelno",
                "pathname",
                "lineno",
                "exc_info",
                "func",
                "sinfo",
            ] {
                if let Some(value) = mapping.get_by_str(key, heap, interns) {
                    out.push((key.to_owned(), value.clone_with_heap(heap)));
                }
            }
            Ok(out)
        })?;
    for (key, value) in copied_fields {
        record.set_attr_text(&key, value, heap, interns)?;
    }

    let record_id = heap.allocate(HeapData::Module(record))?;
    let _ = heap.with_entry_mut(record_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error(
                "internal log record object is not a module".to_string(),
            ));
        };
        set_bound_method(
            module,
            "getMessage",
            LoggingFunctions::Logrecordgetmessage,
            record_id,
            heap,
            interns,
        )?;
        Ok(())
    });

    Ok(record_id)
}

/// Implements `LogRecord.getMessage()` for lightweight record objects.
fn log_record_get_message(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (record_id, positional, kwargs) = take_logger_self(args, "LogRecord.getMessage", heap)?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let msg = module_attr(record_id, "msg", heap, interns)?.unwrap_or(Value::None);
    let args_value = module_attr(record_id, "args", heap, interns)?.unwrap_or(Value::None);
    let rendered_result = render_log_record_message(&msg, &args_value, heap, interns);
    msg.drop_with_heap(heap);
    args_value.drop_with_heap(heap);
    let rendered = rendered_result?;

    let text_id = heap.allocate(HeapData::Str(Str::from(rendered)))?;
    Ok(AttrCallResult::Value(Value::Ref(text_id)))
}

/// Mirrors CPython's `LogRecord.getMessage()` behavior:
/// `msg = str(self.msg); if self.args: msg = msg % self.args`.
fn render_log_record_message(
    msg: &Value,
    args_value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let msg_text = msg.py_str(heap, interns).into_owned();
    if !args_value.py_bool(heap, interns) {
        return Ok(msg_text);
    }

    if let Some(formatted) = msg.py_mod(args_value, heap)? {
        let text = formatted.py_str(heap, interns).into_owned();
        formatted.drop_with_heap(heap);
        return Ok(text);
    }

    render_percent_fallback(&msg_text, args_value, heap, interns)
}

/// Provides `%`-formatting fallback for strings since Ouros's `Value::py_mod` only
/// supports numeric modulo today.
fn render_percent_fallback(
    msg: &str,
    args_value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let positional = collect_percent_positional_args(args_value, heap);
    let rendered = render_percent_message(msg, args_value, &positional, heap, interns);
    positional.drop_with_heap(heap);
    rendered
}

/// Collects positional `%` args from tuple/list values, or treats other values as
/// a single positional argument.
fn collect_percent_positional_args(args_value: &Value, heap: &mut Heap<impl ResourceTracker>) -> Vec<Value> {
    if let Value::Ref(id) = args_value {
        match heap.get(*id) {
            HeapData::Tuple(tuple) => {
                return tuple.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect();
            }
            HeapData::List(list) => {
                return list.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect();
            }
            _ => {}
        }
    }
    vec![args_value.clone_with_heap(heap)]
}

/// Renders old-style `%` format strings with enough coverage for stdlib logging.
fn render_percent_message(
    msg: &str,
    args_value: &Value,
    positional: &[Value],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let chars: Vec<char> = msg.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    let mut positional_index = 0usize;
    let mut used_mapping = false;

    while index < chars.len() {
        let ch = chars[index];
        if ch != '%' {
            out.push(ch);
            index += 1;
            continue;
        }

        index += 1;
        if index >= chars.len() {
            return Err(SimpleException::new_msg(ExcType::ValueError, "incomplete format").into());
        }
        if chars[index] == '%' {
            out.push('%');
            index += 1;
            continue;
        }

        let mut mapping_key: Option<String> = None;
        if chars[index] == '(' {
            used_mapping = true;
            index += 1;
            let key_start = index;
            while index < chars.len() && chars[index] != ')' {
                index += 1;
            }
            if index >= chars.len() {
                return Err(SimpleException::new_msg(ExcType::ValueError, "incomplete format key").into());
            }
            mapping_key = Some(chars[key_start..index].iter().collect());
            index += 1; // Skip ')'
        }

        // flags
        while index < chars.len() && matches!(chars[index], '-' | '+' | ' ' | '#' | '0') {
            index += 1;
        }
        // width
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        // precision
        if index < chars.len() && chars[index] == '.' {
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
        }
        // length modifier
        while index < chars.len() && matches!(chars[index], 'h' | 'l' | 'L') {
            index += 1;
        }

        if index >= chars.len() {
            return Err(SimpleException::new_msg(ExcType::ValueError, "incomplete format").into());
        }

        let conversion_index = index;
        let conversion = chars[index];
        index += 1;

        if !matches!(
            conversion,
            's' | 'r' | 'a' | 'd' | 'i' | 'u' | 'o' | 'x' | 'X' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'c'
        ) {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!(
                    "unsupported format character '{}' (0x{:x}) at index {}",
                    conversion, conversion as u32, conversion_index
                ),
            )
            .into());
        }

        let formatted_piece = if let Some(key) = mapping_key {
            let value = percent_mapping_get(args_value, &key, heap, interns)?;
            let text = percent_render_arg(conversion, &value, heap, interns);
            value.drop_with_heap(heap);
            text
        } else {
            if positional_index >= positional.len() {
                return Err(
                    SimpleException::new_msg(ExcType::TypeError, "not enough arguments for format string").into(),
                );
            }
            let text = percent_render_arg(conversion, &positional[positional_index], heap, interns);
            positional_index += 1;
            text
        };

        out.push_str(&formatted_piece);
    }

    let args_is_dict = matches!(args_value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)));
    if positional_index < positional.len() && (!args_is_dict || used_mapping) {
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "not all arguments converted during string formatting",
        )
        .into());
    }

    Ok(out)
}

/// Resolves mapping `%(<key>)...` arguments.
fn percent_mapping_get(
    args_value: &Value,
    key: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let Value::Ref(dict_id) = args_value else {
        return Err(SimpleException::new_msg(ExcType::TypeError, "format requires a mapping").into());
    };
    heap.with_entry_mut(*dict_id, |heap, data| {
        let HeapData::Dict(mapping) = data else {
            return Err(SimpleException::new_msg(ExcType::TypeError, "format requires a mapping").into());
        };
        let Some(value) = mapping.get_by_str(key, heap, interns) else {
            return Err(SimpleException::new_msg(ExcType::KeyError, key.to_owned()).into());
        };
        Ok(value.clone_with_heap(heap))
    })
}

/// Renders one `%` conversion argument.
fn percent_render_arg(
    conversion: char,
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> String {
    match conversion {
        'r' | 'a' => value.py_repr(heap, interns).into_owned(),
        _ => value.py_str(heap, interns).into_owned(),
    }
}

/// Implements `logging.basicConfig(...)`.
fn basic_config(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    positional.drop_with_heap(heap);

    let mut force = false;
    let mut level_value: Option<Value> = None;

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            level_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns);
        key.drop_with_heap(heap);

        if key_name == "force" {
            force = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
            continue;
        }
        if key_name == "level" {
            level_value.drop_with_heap(heap);
            level_value = Some(value);
            continue;
        }

        // Ignore unsupported kwargs for compatibility.
        value.drop_with_heap(heap);
    }

    let root_id = root_logger_id(heap)?;

    let mut handlers = logger_handlers(root_id, heap, interns)?;
    if force {
        handlers.drop_with_heap(heap);
        handlers = Vec::new();
    }
    if handlers.is_empty() {
        let handler_id = create_handler_object("StreamHandler", 0, heap, interns)?;
        handlers.push(Value::Ref(handler_id));
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        state.handler_ids.push(handler_id);
    }
    set_logger_handlers(root_id, handlers, heap, interns)?;

    if let Some(level) = level_value {
        let parsed_level = check_level(&level, heap, interns)?;
        level.drop_with_heap(heap);
        set_module_attr(root_id, "level", Value::Int(parsed_level), heap, interns)?;
    }

    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.shutdown()`.
fn shutdown(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.disable(level=CRITICAL)`.
fn disable(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let level_value = args.get_zero_one_arg("disable", heap)?;
    let level = match level_value {
        None => 50,
        Some(value) => {
            let parsed = check_level(&value, heap, interns)?;
            value.drop_with_heap(heap);
            parsed
        }
    };

    {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        state.disable = level;
    }

    let manager = manager_id(heap)?;
    set_module_attr(manager, "disable", Value::Int(level), heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.captureWarnings(flag)`.
fn capture_warnings(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let flag = args.get_one_arg("captureWarnings", heap)?;
    let enabled = flag.py_bool(heap, interns);
    flag.drop_with_heap(heap);

    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);
    state.capture_warnings = enabled;

    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.addLevelName(level, levelName)`.
fn add_level_name(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (level_value, name_value) = args.get_two_args("addLevelName", heap)?;
    let level = add_level_value(&level_value, heap, interns);
    let level_name = name_value.py_str(heap, interns).into_owned();

    level_value.drop_with_heap(heap);
    name_value.drop_with_heap(heap);

    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);

    state.name_to_level.retain(|(name, _)| name != &level_name);
    state.name_to_level.push((level_name.clone(), level.clone()));

    match level {
        LevelValue::Int(number) => {
            state.level_to_name_int.retain(|(level, _)| *level != number);
            state.level_to_name_int.push((number, level_name));
        }
        LevelValue::Text(text) => {
            state.level_to_name_text.retain(|(level, _)| level != &text);
            state.level_to_name_text.push((text, level_name));
        }
    }

    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.getLevelName(level)`.
fn get_level_name(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let level = args.get_one_arg("getLevelName", heap)?;

    if let Ok(number) = level.as_int(heap) {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);

        let value = if let Some((_, name)) = state.level_to_name_int.iter().find(|(level, _)| *level == number) {
            Value::Ref(heap.allocate(HeapData::Str(Str::from(name.as_str())))?)
        } else {
            Value::Ref(heap.allocate(HeapData::Str(Str::from(format!("Level {number}"))))?)
        };
        level.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(value));
    }

    let level_text = level.py_str(heap, interns).into_owned();
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);

    let value = if let Some((_, mapped)) = state.name_to_level.iter().find(|(name, _)| name == &level_text) {
        match mapped {
            LevelValue::Int(number) => Value::Int(*number),
            LevelValue::Text(text) => Value::Ref(heap.allocate(HeapData::Str(Str::from(text.as_str())))?),
        }
    } else if let Some((_, name)) = state
        .level_to_name_text
        .iter()
        .find(|(level_name, _)| level_name == &level_text)
    {
        Value::Ref(heap.allocate(HeapData::Str(Str::from(name.as_str())))?)
    } else {
        Value::Ref(heap.allocate(HeapData::Str(Str::from(format!("Level {level_text}"))))?)
    };

    level.drop_with_heap(heap);
    Ok(AttrCallResult::Value(value))
}

/// Implements `logging.getLevelNamesMapping()`.
fn get_level_names_mapping(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<AttrCallResult> {
    let mut state = logging_state().lock().expect("logging state mutex poisoned");
    prune_dead_ids(&mut state, heap);

    let mut mapping = Dict::new();
    for (name, level) in &state.name_to_level {
        let key_id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
        let level_value = match level {
            LevelValue::Int(number) => Value::Int(*number),
            LevelValue::Text(text) => Value::Ref(heap.allocate(HeapData::Str(Str::from(text.as_str())))?),
        };
        mapping
            .set(Value::Ref(key_id), level_value, heap, interns)
            .expect("logging level keys are hashable");
    }

    let mapping_id = heap.allocate(HeapData::Dict(mapping))?;
    Ok(AttrCallResult::Value(Value::Ref(mapping_id)))
}

/// Implements `logging.getHandlerByName(name)`.
fn get_handler_by_name(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let query = args.get_one_arg("getHandlerByName", heap)?;

    let handler_ids = {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        state.handler_ids.clone()
    };

    for handler_id in handler_ids {
        let Some(name_value) = module_attr(handler_id, "name", heap, interns)? else {
            continue;
        };
        if !matches!(name_value, Value::None) && query.py_eq(&name_value, heap, interns) {
            name_value.drop_with_heap(heap);
            query.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(clone_ref_value(handler_id, heap)));
        }
        name_value.drop_with_heap(heap);
    }

    query.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `logging.getHandlerNames()`.
fn get_handler_names(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    args.check_zero_args("getHandlerNames", heap)?;

    let handler_ids = {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        state.handler_ids.clone()
    };

    let mut names = Set::new();
    for handler_id in handler_ids {
        let Some(name_value) = module_attr(handler_id, "name", heap, interns)? else {
            continue;
        };
        if matches!(name_value, Value::None) {
            name_value.drop_with_heap(heap);
            continue;
        }
        let _ = names.add(name_value, heap, interns)?;
    }

    let set_id = heap.allocate(HeapData::FrozenSet(FrozenSet::from_set(names)))?;
    Ok(AttrCallResult::Value(Value::Ref(set_id)))
}

/// Forwards a module-level convenience logging call to the root logger.
fn module_level_log_at_level(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    level: i64,
    method_name: &str,
) -> RunResult<AttrCallResult> {
    let root = root_logger_id(heap)?;
    let (positional, kwargs) = args.into_parts();
    let mut forwarded = Vec::with_capacity(positional.len() + 1);
    forwarded.push(clone_ref_value(root, heap));
    forwarded.extend(positional);
    logger_log_at_level(
        heap,
        interns,
        args_from_vec_kwargs(forwarded, kwargs),
        level,
        method_name,
    )
}

/// Implements module-level `logging.log(level, msg, *args, **kwargs)`.
fn module_level_log(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let root = root_logger_id(heap)?;
    let (positional, kwargs) = args.into_parts();
    let mut forwarded = Vec::with_capacity(positional.len() + 1);
    forwarded.push(clone_ref_value(root, heap));
    forwarded.extend(positional);
    logger_log(heap, interns, args_from_vec_kwargs(forwarded, kwargs))
}

/// Implements `Logger.<level>(msg, *args, **kwargs)`.
fn logger_log_at_level(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    level: i64,
    method_name: &str,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, method_name, heap)?;
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 2, 1));
    }

    // Message and format args are currently consumed for API parity checks,
    // while emission remains sandbox-safe/no-op by default.
    let mut iter = positional.into_iter();
    let msg = iter.next().expect("non-empty checked");
    let format_args: Vec<Value> = iter.collect();

    let enabled = logger_enabled_for_int(logger_id, level, heap, interns)?;

    msg.drop_with_heap(heap);
    format_args.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let _ = enabled;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Logger.log(level, msg, *args, **kwargs)`.
fn logger_log(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.log", heap)?;
    let positional_len = positional.len();
    if positional_len < 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("Logger.log", 3, 1 + positional_len));
    }

    let mut iter = positional.into_iter();
    let level_value = iter.next().expect("len checked");
    let msg = iter.next().expect("len checked");

    let level = level_value
        .as_int(heap)
        .map_err(|_| SimpleException::new_msg(ExcType::TypeError, "level must be an integer"))?;

    let format_args: Vec<Value> = iter.collect();
    let enabled = logger_enabled_for_int(logger_id, level, heap, interns)?;

    level_value.drop_with_heap(heap);
    msg.drop_with_heap(heap);
    format_args.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let _ = enabled;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Logger.setLevel(level)`.
fn logger_set_level(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.setLevel", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("Logger.setLevel", 2, 1 + positional_len));
    }

    let mut iter = positional.into_iter();
    let level_value = iter.next().expect("len checked");
    let level = check_level(&level_value, heap, interns)?;
    level_value.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    set_module_attr(logger_id, "level", Value::Int(level), heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Logger.addHandler(handler)`.
fn logger_add_handler(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.addHandler", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count(
            "Logger.addHandler",
            2,
            1 + positional_len,
        ));
    }

    let mut iter = positional.into_iter();
    let handler = iter.next().expect("len checked");

    let mut handlers = logger_handlers(logger_id, heap, interns)?;
    let exists = handlers.iter().any(|existing| existing.py_eq(&handler, heap, interns));
    if !exists {
        handlers.push(handler.clone_with_heap(heap));
    }

    let handler_for_tracking = handler.clone_with_heap(heap);
    handler.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    set_logger_handlers(logger_id, handlers, heap, interns)?;

    if let Value::Ref(handler_id) = &handler_for_tracking {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        if !state.handler_ids.contains(handler_id) {
            state.handler_ids.push(*handler_id);
        }
    }
    handler_for_tracking.drop_with_heap(heap);

    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Logger.removeHandler(handler)`.
fn logger_remove_handler(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.removeHandler", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count(
            "Logger.removeHandler",
            2,
            1 + positional_len,
        ));
    }

    let mut iter = positional.into_iter();
    let target = iter.next().expect("len checked");

    let handlers = logger_handlers(logger_id, heap, interns)?;
    let mut remaining = Vec::new();
    for handler in handlers {
        if handler.py_eq(&target, heap, interns) {
            handler.drop_with_heap(heap);
        } else {
            remaining.push(handler);
        }
    }

    target.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    set_logger_handlers(logger_id, remaining, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Computes logger effective level by walking logger ancestors.
fn effective_level_for_logger(
    logger_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    let Some(logger_name) = logger_name_for_id(heap, logger_id) else {
        return Ok(0);
    };

    for ancestor_id in ancestor_logger_ids(heap, &logger_name) {
        let level = logger_attr_int(ancestor_id, "level", 0, heap, interns)?;
        if level != 0 {
            return Ok(level);
        }
    }

    Ok(0)
}

/// Returns whether a logger is enabled for an integer level.
fn logger_enabled_for_int(
    logger_id: HeapId,
    level: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    if logger_attr_bool(logger_id, "disabled", false, heap, interns)? {
        return Ok(false);
    }

    let disable_threshold = {
        let mut state = logging_state().lock().expect("logging state mutex poisoned");
        prune_dead_ids(&mut state, heap);
        state.disable
    };
    if disable_threshold >= level {
        return Ok(false);
    }

    let effective = effective_level_for_logger(logger_id, heap, interns)?;
    Ok(level >= effective)
}

/// Implements `Logger.getEffectiveLevel()`.
fn logger_get_effective_level(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.getEffectiveLevel", heap)?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    let level = effective_level_for_logger(logger_id, heap, interns)?;
    Ok(AttrCallResult::Value(Value::Int(level)))
}

/// Implements `Logger.isEnabledFor(level)`.
fn logger_is_enabled_for(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.isEnabledFor", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count(
            "Logger.isEnabledFor",
            2,
            1 + positional_len,
        ));
    }

    let mut iter = positional.into_iter();
    let level_value = iter.next().expect("len checked");

    let enabled = if let Ok(level) = level_value.as_int(heap) {
        logger_enabled_for_int(logger_id, level, heap, interns)?
    } else if let Value::Float(level) = level_value {
        let effective = f64::from(effective_level_for_logger(logger_id, heap, interns)? as i32);
        let disable_threshold = {
            let mut state = logging_state().lock().expect("logging state mutex poisoned");
            prune_dead_ids(&mut state, heap);
            f64::from(state.disable as i32)
        };
        if logger_attr_bool(logger_id, "disabled", false, heap, interns)? {
            false
        } else {
            level >= effective && disable_threshold < level
        }
    } else {
        let other_type = level_value.py_type(heap);
        level_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!(">= not supported between instances of 'int' and '{other_type}'"),
        )
        .into());
    };

    level_value.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(enabled)))
}

/// Implements `Logger.hasHandlers()`.
fn logger_has_handlers(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.hasHandlers", heap)?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let Some(logger_name) = logger_name_for_id(heap, logger_id) else {
        return Ok(AttrCallResult::Value(Value::Bool(false)));
    };

    for ancestor_id in ancestor_logger_ids(heap, &logger_name) {
        let handlers = logger_handlers(ancestor_id, heap, interns)?;
        let has_handlers = !handlers.is_empty();
        handlers.drop_with_heap(heap);
        if has_handlers {
            return Ok(AttrCallResult::Value(Value::Bool(true)));
        }

        if !logger_attr_bool(ancestor_id, "propagate", true, heap, interns)? {
            break;
        }
    }

    Ok(AttrCallResult::Value(Value::Bool(false)))
}

/// Implements `Logger.getChild(suffix)`.
fn logger_get_child(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (logger_id, positional, kwargs) = take_logger_self(args, "Logger.getChild", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("Logger.getChild", 2, 1 + positional_len));
    }

    let mut iter = positional.into_iter();
    let suffix = iter.next().expect("len checked");
    let Some(suffix_text) = suffix.as_either_str(heap) else {
        let suffix_type = suffix.py_type(heap);
        suffix.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("sequence item 1: expected str instance, {suffix_type} found"),
        )
        .into());
    };

    let suffix_text = suffix_text.as_str(interns).to_owned();
    suffix.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let parent_name = logger_name_for_id(heap, logger_id).unwrap_or_else(|| "root".to_owned());
    let child_name = if parent_name == "root" {
        suffix_text
    } else {
        format!("{parent_name}.{suffix_text}")
    };

    let child_id = get_or_create_logger_id(&child_name, heap, interns)?;
    Ok(AttrCallResult::Value(clone_ref_value(child_id, heap)))
}

/// Implements `Handler.setLevel(level)`.
fn handler_set_level(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (handler_id, positional, kwargs) = take_logger_self(args, "Handler.setLevel", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("Handler.setLevel", 2, 1 + positional_len));
    }

    let mut iter = positional.into_iter();
    let level_value = iter.next().expect("len checked");
    let level = check_level(&level_value, heap, interns)?;
    level_value.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    set_module_attr(handler_id, "level", Value::Int(level), heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Handler.set_name(name)`.
fn handler_set_name(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (handler_id, positional, kwargs) = take_logger_self(args, "Handler.set_name", heap)?;
    let positional_len = positional.len();
    if positional_len != 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("Handler.set_name", 2, 1 + positional_len));
    }

    let mut iter = positional.into_iter();
    let name_value = iter.next().expect("len checked");
    kwargs.drop_with_heap(heap);

    set_module_attr(handler_id, "name", name_value, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Handler.get_name()`.
fn handler_get_name(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (handler_id, positional, kwargs) = take_logger_self(args, "Handler.get_name", heap)?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let name = module_attr(handler_id, "name", heap, interns)?.unwrap_or(Value::None);
    Ok(AttrCallResult::Value(name))
}
