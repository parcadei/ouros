//! Pragmatic `argparse` compatibility module.
//!
//! This implements a focused subset of CPython's `argparse` API used by Ouros
//! stdlib parity tests:
//! - `ArgumentParser` with `add_argument`, `parse_args`, `parse_known_args`
//! - subparsers via `add_subparsers(...).add_parser(...)`
//! - argument groups and mutually exclusive groups
//! - `Namespace` constructor
//! - `FileType` factory

use std::sync::{Mutex, OnceLock};

use ahash::{AHashMap, AHashSet};
use smallvec::smallvec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::NoPrint,
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dataclass, Dict, List, Module, Partial, PyTrait, Str, Type, allocate_tuple},
    value::Value,
};

/// `argparse` module callables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ArgparseFunctions {
    #[strum(serialize = "ArgumentParser")]
    ArgumentParser,
    #[strum(serialize = "Namespace")]
    Namespace,
    #[strum(serialize = "FileType")]
    FileType,

    #[strum(serialize = "_parser_add_argument")]
    ParserAddArgument,
    #[strum(serialize = "_parser_parse_args")]
    ParserParseArgs,
    #[strum(serialize = "_parser_parse_known_args")]
    ParserParseKnownArgs,
    #[strum(serialize = "_parser_add_subparsers")]
    ParserAddSubparsers,
    #[strum(serialize = "_parser_add_argument_group")]
    ParserAddArgumentGroup,
    #[strum(serialize = "_parser_add_mutually_exclusive_group")]
    ParserAddMutuallyExclusiveGroup,
    #[strum(serialize = "_parser_format_help")]
    ParserFormatHelp,
    #[strum(serialize = "_parser_format_usage")]
    ParserFormatUsage,
    #[strum(serialize = "_parser_print_help")]
    ParserPrintHelp,
    #[strum(serialize = "_parser_print_usage")]
    ParserPrintUsage,
    #[strum(serialize = "_parser_error")]
    ParserError,
    #[strum(serialize = "_parser_exit")]
    ParserExit,

    #[strum(serialize = "_subparsers_add_parser")]
    SubparsersAddParser,
    #[strum(serialize = "_group_add_argument")]
    GroupAddArgument,
    #[strum(serialize = "_file_type_call")]
    FileTypeCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeConv {
    Raw,
    Str,
    Int,
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NargsSpec {
    Exact(usize),
    Optional,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Debug, Clone, PartialEq)]
enum Atom {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Self>),
}

#[derive(Debug, Clone)]
struct ArgSpec {
    names: Vec<String>,
    dest: String,
    optional: bool,
    action: String,
    nargs: Option<NargsSpec>,
    const_value: Option<Atom>,
    default_value: Option<Atom>,
    type_conv: TypeConv,
    choices: Option<Vec<Atom>>,
    required: bool,
    help: Option<String>,
    metavar: Option<String>,
    version: Option<String>,
    mutex_group: Option<usize>,
}

#[derive(Debug, Clone)]
struct MutexGroupState {
    required: bool,
    members: Vec<String>,
}

#[derive(Debug, Clone)]
struct SubparsersState {
    dest: Option<String>,
    required: bool,
    parsers: Vec<(String, HeapId)>,
}

#[derive(Debug, Clone)]
struct ParserState {
    prog: String,
    usage: Option<String>,
    description: Option<String>,
    epilog: Option<String>,
    argument_default: Option<Atom>,
    add_help: bool,
    allow_abbrev: bool,
    specs: Vec<ArgSpec>,
    mutex_groups: Vec<MutexGroupState>,
    subparsers: Option<SubparsersState>,
}

#[derive(Debug, Clone, Copy)]
struct GroupBinding {
    parser_id: HeapId,
    mutex_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct FileTypeState {
    mode: String,
    _bufsize: i64,
    _encoding: Option<String>,
    _errors: Option<String>,
}

#[derive(Debug, Default)]
struct ArgparseRuntime {
    parsers: AHashMap<HeapId, ParserState>,
    groups: AHashMap<HeapId, GroupBinding>,
    subparsers: AHashMap<HeapId, HeapId>,
    filetypes: AHashMap<HeapId, FileTypeState>,
}

static ARGPARSE_RUNTIME: OnceLock<Mutex<ArgparseRuntime>> = OnceLock::new();

fn runtime() -> &'static Mutex<ArgparseRuntime> {
    ARGPARSE_RUNTIME.get_or_init(|| Mutex::new(ArgparseRuntime::default()))
}

fn prune_runtime(rt: &mut ArgparseRuntime, heap: &Heap<impl ResourceTracker>) {
    rt.parsers.retain(|id, _| heap.get_if_live(*id).is_some());
    rt.groups
        .retain(|id, binding| heap.get_if_live(*id).is_some() && heap.get_if_live(binding.parser_id).is_some());
    rt.subparsers
        .retain(|id, parser_id| heap.get_if_live(*id).is_some() && heap.get_if_live(*parser_id).is_some());
    rt.filetypes.retain(|id, _| heap.get_if_live(*id).is_some());
}

pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Argparse);

    module.set_attr_text(
        "ArgumentParser",
        Value::ModuleFunction(ModuleFunctions::Argparse(ArgparseFunctions::ArgumentParser)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "Namespace",
        Value::ModuleFunction(ModuleFunctions::Argparse(ArgparseFunctions::Namespace)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "FileType",
        Value::ModuleFunction(ModuleFunctions::Argparse(ArgparseFunctions::FileType)),
        heap,
        interns,
    )?;

    // Class/exception placeholders for compatibility.
    module.set_attr_text(
        "HelpFormatter",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "RawDescriptionHelpFormatter",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "RawTextHelpFormatter",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ArgumentDefaultsHelpFormatter",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ArgumentError",
        Value::Builtin(Builtins::Type(Type::Exception(ExcType::TypeError))),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ArgumentTypeError",
        Value::Builtin(Builtins::Type(Type::Exception(ExcType::TypeError))),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ArgparseFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        ArgparseFunctions::ArgumentParser => argument_parser(heap, interns, args)?,
        ArgparseFunctions::Namespace => namespace_ctor(heap, interns, args)?,
        ArgparseFunctions::FileType => file_type_ctor(heap, interns, args)?,

        ArgparseFunctions::ParserAddArgument => parser_add_argument(heap, interns, args)?,
        ArgparseFunctions::ParserParseArgs => parser_parse_args(heap, interns, args, false)?,
        ArgparseFunctions::ParserParseKnownArgs => parser_parse_args(heap, interns, args, true)?,
        ArgparseFunctions::ParserAddSubparsers => parser_add_subparsers(heap, interns, args)?,
        ArgparseFunctions::ParserAddArgumentGroup => parser_add_argument_group(heap, interns, args)?,
        ArgparseFunctions::ParserAddMutuallyExclusiveGroup => parser_add_mutually_exclusive_group(heap, interns, args)?,
        ArgparseFunctions::ParserFormatHelp => parser_format_help(heap, interns, args)?,
        ArgparseFunctions::ParserFormatUsage => parser_format_usage(heap, interns, args)?,
        ArgparseFunctions::ParserPrintHelp => {
            let _ = parser_format_help(heap, interns, args)?;
            Value::None
        }
        ArgparseFunctions::ParserPrintUsage => {
            let _ = parser_format_usage(heap, interns, args)?;
            Value::None
        }
        ArgparseFunctions::ParserError => parser_error(heap, interns, args)?,
        ArgparseFunctions::ParserExit => parser_exit(heap, interns, args)?,

        ArgparseFunctions::SubparsersAddParser => subparsers_add_parser(heap, interns, args)?,
        ArgparseFunctions::GroupAddArgument => group_add_argument(heap, interns, args)?,
        ArgparseFunctions::FileTypeCall => file_type_call(heap, interns, args)?,
    };

    Ok(AttrCallResult::Value(value))
}

fn argument_parser(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut pos = positional.into_iter();

    let mut prog: Option<String> = None;
    let mut usage: Option<String> = None;
    let mut description: Option<String> = None;
    let mut epilog: Option<String> = None;
    let mut argument_default: Option<Atom> = None;
    let mut add_help = true;
    let mut allow_abbrev = true;

    if let Some(v) = pos.next() {
        prog = Some(value_to_string(v, heap, interns)?);
    }
    if let Some(v) = pos.next() {
        usage = atom_to_optional_string(value_to_atom(v, heap, interns)?);
    }
    if let Some(v) = pos.next() {
        description = atom_to_optional_string(value_to_atom(v, heap, interns)?);
    }
    if let Some(v) = pos.next() {
        epilog = atom_to_optional_string(value_to_atom(v, heap, interns)?);
    }

    // Ignore formatter_class/prefix_chars/fromfile_prefix_chars/conflict_handler/exit_on_error
    if let Some(v) = pos.next() {
        v.drop_with_heap(heap);
    }
    if let Some(v) = pos.next() {
        v.drop_with_heap(heap);
    }
    if let Some(v) = pos.next() {
        v.drop_with_heap(heap);
    }
    if let Some(v) = pos.next() {
        argument_default = Some(value_to_atom(v, heap, interns)?);
    }
    if let Some(v) = pos.next() {
        v.drop_with_heap(heap);
    }
    if let Some(v) = pos.next() {
        add_help = v.py_bool(heap, interns);
        v.drop_with_heap(heap);
    }
    if let Some(v) = pos.next() {
        allow_abbrev = v.py_bool(heap, interns);
        v.drop_with_heap(heap);
    }
    if let Some(v) = pos.next() {
        v.drop_with_heap(heap);
    }
    for extra in pos {
        extra.drop_with_heap(heap);
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "prog" => prog = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "usage" => usage = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "description" => description = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "epilog" => epilog = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "argument_default" => argument_default = Some(value_to_atom(value, heap, interns)?),
            "add_help" => {
                add_help = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "allow_abbrev" => {
                allow_abbrev = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
            }
        }
    }

    let parser_prog = prog.unwrap_or_else(|| "prog".to_owned());
    let parser_id = create_parser_object(heap, interns)?;

    let mut state = ParserState {
        prog: parser_prog,
        usage,
        description,
        epilog,
        argument_default,
        add_help,
        allow_abbrev,
        specs: Vec::new(),
        mutex_groups: Vec::new(),
        subparsers: None,
    };

    if state.add_help {
        state.specs.push(ArgSpec {
            names: vec!["-h".to_owned(), "--help".to_owned()],
            dest: "help".to_owned(),
            optional: true,
            action: "help".to_owned(),
            nargs: Some(NargsSpec::Exact(0)),
            const_value: None,
            default_value: None,
            type_conv: TypeConv::Raw,
            choices: None,
            required: false,
            help: Some("show this help message and exit".to_owned()),
            metavar: None,
            version: None,
            mutex_group: None,
        });
    }

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    rt.parsers.insert(parser_id, state);

    Ok(Value::Ref(parser_id))
}

fn create_parser_object(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let parser = Module::new(StaticStrings::Argparse);
    let id = heap.allocate(HeapData::Module(parser))?;

    // Reborrow mutable to install bound methods.
    let _ = heap.with_entry_mut(id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error("internal argparse parser is not a module"));
        };
        set_bound_method(
            module,
            "add_argument",
            ArgparseFunctions::ParserAddArgument,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "parse_args",
            ArgparseFunctions::ParserParseArgs,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "parse_known_args",
            ArgparseFunctions::ParserParseKnownArgs,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "add_subparsers",
            ArgparseFunctions::ParserAddSubparsers,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "add_argument_group",
            ArgparseFunctions::ParserAddArgumentGroup,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "add_mutually_exclusive_group",
            ArgparseFunctions::ParserAddMutuallyExclusiveGroup,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "format_help",
            ArgparseFunctions::ParserFormatHelp,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "format_usage",
            ArgparseFunctions::ParserFormatUsage,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "print_help",
            ArgparseFunctions::ParserPrintHelp,
            id,
            heap,
            interns,
        )?;
        set_bound_method(
            module,
            "print_usage",
            ArgparseFunctions::ParserPrintUsage,
            id,
            heap,
            interns,
        )?;
        set_bound_method(module, "error", ArgparseFunctions::ParserError, id, heap, interns)?;
        set_bound_method(module, "exit", ArgparseFunctions::ParserExit, id, heap, interns)?;
        Ok(())
    });

    Ok(id)
}

fn set_bound_method(
    module: &mut Module,
    method_name: &str,
    function: ArgparseFunctions,
    self_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    heap.inc_ref(self_id);
    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Argparse(function)),
        vec![Value::Ref(self_id)],
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    module.set_attr_text(method_name, Value::Ref(partial_id), heap, interns)
}

fn namespace_ctor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut entries: Vec<(String, Atom)> = Vec::new();

    let mut pos = positional.into_iter();
    if let Some(mapping) = pos.next() {
        extract_namespace_seed(mapping, &mut entries, heap, interns)?;
    }
    if let Some(extra) = pos.next() {
        extra.drop_with_heap(heap);
        for rest in pos {
            rest.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "Namespace() expected at most 1 positional argument",
        ));
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        let atom = value_to_atom(value, heap, interns)?;
        upsert_entry(&mut entries, name, atom);
    }

    create_namespace_value(entries, heap, interns)
}

fn extract_namespace_seed(
    value: Value,
    entries: &mut Vec<(String, Atom)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    match value {
        Value::Ref(id) => {
            let items = heap.with_entry_mut(id, |heap, data| -> RunResult<Option<Vec<(Value, Value)>>> {
                Ok(match data {
                    HeapData::Dataclass(dc) => Some(dc.attrs().items(heap)),
                    HeapData::Module(module) => Some(module.attrs().items(heap)),
                    HeapData::Dict(dict) => Some(dict.items(heap)),
                    _ => None,
                })
            })?;

            Value::Ref(id).drop_with_heap(heap);
            if let Some(items) = items {
                for (k, v) in items {
                    let name = value_to_string(k, heap, interns)?;
                    let atom = value_to_atom(v, heap, interns)?;
                    upsert_entry(entries, name, atom);
                }
            }
        }
        other => {
            other.drop_with_heap(heap);
        }
    }
    Ok(())
}

fn file_type_ctor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut pos = positional.into_iter();

    let mut mode = "r".to_owned();
    let mut bufsize = -1_i64;
    let mut encoding: Option<String> = None;
    let mut errors: Option<String> = None;

    if let Some(v) = pos.next() {
        mode = value_to_string(v, heap, interns)?;
    }
    if let Some(v) = pos.next() {
        bufsize = atom_to_int(value_to_atom(v, heap, interns)?).unwrap_or(-1);
    }
    if let Some(v) = pos.next() {
        encoding = atom_to_optional_string(value_to_atom(v, heap, interns)?);
    }
    if let Some(v) = pos.next() {
        errors = atom_to_optional_string(value_to_atom(v, heap, interns)?);
    }
    for extra in pos {
        extra.drop_with_heap(heap);
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "mode" => mode = value_to_string(value, heap, interns)?,
            "bufsize" => bufsize = atom_to_int(value_to_atom(value, heap, interns)?).unwrap_or(-1),
            "encoding" => encoding = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "errors" => errors = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            _ => value.drop_with_heap(heap),
        }
    }

    let module = Module::new(StaticStrings::Argparse);
    let id = heap.allocate(HeapData::Module(module))?;
    let _ = heap.with_entry_mut(id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error("internal argparse filetype is not a module"));
        };
        set_bound_method(module, "__call__", ArgparseFunctions::FileTypeCall, id, heap, interns)?;
        Ok(())
    });

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    rt.filetypes.insert(
        id,
        FileTypeState {
            mode,
            _bufsize: bufsize,
            _encoding: encoding,
            _errors: errors,
        },
    );

    Ok(Value::Ref(id))
}

fn file_type_call(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_id, mut positional, _kwargs) = take_self_and_rest(args, heap, interns, "FileType.__call__")?;

    let path = if positional.is_empty() {
        String::new()
    } else {
        value_to_string(positional.remove(0), heap, interns)?
    };
    for extra in positional {
        extra.drop_with_heap(heap);
    }

    let rt = runtime().lock().expect("argparse runtime mutex poisoned");
    let is_binary = rt.filetypes.get(&self_id).is_some_and(|ft| ft.mode.contains('b'));
    drop(rt);

    if is_binary {
        let bytes = path.into_bytes();
        let bytes_id = heap.allocate(HeapData::Bytes(crate::types::Bytes::new(bytes)))?;
        Ok(Value::Ref(bytes_id))
    } else {
        let str_id = heap.allocate(HeapData::Str(Str::from(path)))?;
        Ok(Value::Ref(str_id))
    }
}

fn parser_add_argument(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (parser_id, positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.add_argument")?;
    add_argument_for_parser(parser_id, None, positional, kwargs, heap, interns)
}

fn group_add_argument(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (group_id, positional, kwargs) = take_self_and_rest(args, heap, interns, "_Group.add_argument")?;

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    let Some(binding) = rt.groups.get(&group_id).copied() else {
        return Err(ExcType::type_error("invalid argparse argument group"));
    };
    drop(rt);

    add_argument_for_parser(
        binding.parser_id,
        binding.mutex_index,
        positional,
        kwargs,
        heap,
        interns,
    )
}

fn add_argument_for_parser(
    parser_id: HeapId,
    mutex_index: Option<usize>,
    positional: Vec<Value>,
    kwargs: Vec<(Value, Value)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut names = Vec::new();
    for value in positional {
        names.push(value_to_string(value, heap, interns)?);
    }

    if names.is_empty() {
        return Err(ExcType::type_error("add_argument() requires at least one name or flag"));
    }

    let optional = names.first().is_some_and(|name| name.starts_with('-'));
    let mut action = "store".to_owned();
    let mut nargs: Option<NargsSpec> = None;
    let mut const_value: Option<Atom> = None;
    let mut default_value: Option<Atom> = None;
    let mut type_conv = TypeConv::Raw;
    let mut choices: Option<Vec<Atom>> = None;
    let mut required = false;
    let mut help: Option<String> = None;
    let mut metavar: Option<String> = None;
    let mut dest: Option<String> = None;
    let mut version: Option<String> = None;

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "action" => action = value_to_string(value, heap, interns)?,
            "nargs" => nargs = Some(parse_nargs(value, heap, interns)?),
            "const" => const_value = Some(value_to_atom(value, heap, interns)?),
            "default" => default_value = Some(value_to_atom(value, heap, interns)?),
            "type" => {
                type_conv = value_to_type_conv(value, heap, interns)?;
            }
            "choices" => choices = Some(value_to_choices(value, heap, interns)?),
            "required" => {
                required = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "help" => help = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "metavar" => metavar = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "dest" => dest = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "version" => version = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            _ => {
                value.drop_with_heap(heap);
            }
        }
    }

    let dest = match dest {
        Some(dest) => dest,
        None if optional => derive_dest_from_flags(&names),
        None => names[0].clone(),
    };

    let spec = ArgSpec {
        names,
        dest: dest.clone(),
        optional,
        action,
        nargs,
        const_value,
        default_value,
        type_conv,
        choices,
        required: optional && required,
        help,
        metavar,
        version,
        mutex_group: mutex_index,
    };

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    let Some(parser) = rt.parsers.get_mut(&parser_id) else {
        return Err(ExcType::type_error("invalid ArgumentParser object"));
    };

    if let Some(group_index) = mutex_index
        && let Some(group) = parser.mutex_groups.get_mut(group_index)
    {
        group.members.push(dest);
    }

    parser.specs.push(spec);
    Ok(Value::None)
}

fn parser_add_subparsers(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (parser_id, positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.add_subparsers")?;
    for value in positional {
        value.drop_with_heap(heap);
    }

    let mut dest: Option<String> = None;
    let mut required = false;

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "dest" => dest = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            "required" => {
                required = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            _ => value.drop_with_heap(heap),
        }
    }

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    let Some(parser) = rt.parsers.get_mut(&parser_id) else {
        return Err(ExcType::type_error("invalid ArgumentParser object"));
    };
    if parser.subparsers.is_none() {
        parser.subparsers = Some(SubparsersState {
            dest,
            required,
            parsers: Vec::new(),
        });
    }

    let obj = Module::new(StaticStrings::Argparse);
    let obj_id = heap.allocate(HeapData::Module(obj))?;
    let _ = heap.with_entry_mut(obj_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error(
                "internal argparse subparsers object is not a module",
            ));
        };
        set_bound_method(
            module,
            "add_parser",
            ArgparseFunctions::SubparsersAddParser,
            obj_id,
            heap,
            interns,
        )?;
        Ok(())
    });

    rt.subparsers.insert(obj_id, parser_id);
    Ok(Value::Ref(obj_id))
}

fn subparsers_add_parser(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (subparsers_id, mut positional, kwargs) =
        take_self_and_rest(args, heap, interns, "_SubParsersAction.add_parser")?;

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("add_parser() missing required parser name"));
    }
    let name = value_to_string(positional.remove(0), heap, interns)?;
    for extra in positional {
        extra.drop_with_heap(heap);
    }

    let parent_id = {
        let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
        prune_runtime(&mut rt, heap);
        let Some(parent_id) = rt.subparsers.get(&subparsers_id).copied() else {
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error("invalid argparse subparsers object"));
        };
        parent_id
    };

    let parser_value = argument_parser(
        heap,
        interns,
        ArgValues::ArgsKargs {
            args: vec![Value::None],
            kwargs: KwargsValues::Empty,
        },
    )?;

    let parser_id = if let Value::Ref(id) = &parser_value {
        *id
    } else {
        parser_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("internal argparse parser creation failed"));
    };

    {
        let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
        prune_runtime(&mut rt, heap);

        // Apply subparser kwargs we care about.
        if let Some(state) = rt.parsers.get_mut(&parser_id) {
            state.prog = name.clone();
            for (key, value) in kwargs {
                let Some(k) = key.as_either_str(heap) else {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    continue;
                };
                let k = k.as_str(interns).to_owned();
                key.drop_with_heap(heap);
                match k.as_str() {
                    "description" => state.description = atom_to_optional_string(value_to_atom(value, heap, interns)?),
                    "epilog" => state.epilog = atom_to_optional_string(value_to_atom(value, heap, interns)?),
                    "usage" => state.usage = atom_to_optional_string(value_to_atom(value, heap, interns)?),
                    _ => value.drop_with_heap(heap),
                }
            }
        }

        if let Some(parent) = rt.parsers.get_mut(&parent_id)
            && let Some(subs) = parent.subparsers.as_mut()
        {
            subs.parsers.push((name, parser_id));
        }
    }

    // Return the owned parser object created above.
    Ok(parser_value)
}

fn parser_add_argument_group(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (parser_id, positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.add_argument_group")?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    create_group_object(parser_id, None, heap, interns)
}

fn parser_add_mutually_exclusive_group(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (parser_id, positional, kwargs) =
        take_self_and_rest(args, heap, interns, "ArgumentParser.add_mutually_exclusive_group")?;
    positional.drop_with_heap(heap);

    let mut required = false;
    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            continue;
        };
        let name = name.as_str(interns);
        key.drop_with_heap(heap);
        if name == "required" {
            required = value.py_bool(heap, interns);
        }
        value.drop_with_heap(heap);
    }

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    let Some(parser) = rt.parsers.get_mut(&parser_id) else {
        return Err(ExcType::type_error("invalid ArgumentParser object"));
    };
    let idx = parser.mutex_groups.len();
    parser.mutex_groups.push(MutexGroupState {
        required,
        members: Vec::new(),
    });
    drop(rt);

    create_group_object(parser_id, Some(idx), heap, interns)
}

fn create_group_object(
    parser_id: HeapId,
    mutex_index: Option<usize>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let group = Module::new(StaticStrings::Argparse);
    let group_id = heap.allocate(HeapData::Module(group))?;

    let _ = heap.with_entry_mut(group_id, |heap, data| {
        let HeapData::Module(module) = data else {
            return Err(ExcType::type_error("internal argparse group object is not a module"));
        };
        set_bound_method(
            module,
            "add_argument",
            ArgparseFunctions::GroupAddArgument,
            group_id,
            heap,
            interns,
        )?;
        Ok(())
    });

    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    rt.groups.insert(group_id, GroupBinding { parser_id, mutex_index });

    Ok(Value::Ref(group_id))
}

fn parser_format_usage(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (parser_id, positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.format_usage")?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let state = parser_state_snapshot(parser_id, heap)?;
    let usage = render_usage(&state);
    let id = heap.allocate(HeapData::Str(Str::from(usage)))?;
    Ok(Value::Ref(id))
}

fn parser_format_help(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (parser_id, positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.format_help")?;
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);

    let state = parser_state_snapshot(parser_id, heap)?;
    let help = render_help(&state);
    let id = heap.allocate(HeapData::Str(Str::from(help)))?;
    Ok(Value::Ref(id))
}

fn parser_error(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (_parser_id, mut positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.error")?;
    kwargs.drop_with_heap(heap);
    if !positional.is_empty() {
        positional.remove(0).drop_with_heap(heap);
    }
    positional.drop_with_heap(heap);
    Err(SimpleException::new_msg(ExcType::SystemExit, "2").into())
}

fn parser_exit(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (_parser_id, mut positional, kwargs) = take_self_and_rest(args, heap, interns, "ArgumentParser.exit")?;

    let mut status = 0_i64;
    let mut message: Option<String> = None;

    if !positional.is_empty() {
        status = atom_to_int(value_to_atom(positional.remove(0), heap, interns)?).unwrap_or(0);
    }
    if !positional.is_empty() {
        message = atom_to_optional_string(value_to_atom(positional.remove(0), heap, interns)?);
    }
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            continue;
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "status" => status = atom_to_int(value_to_atom(value, heap, interns)?).unwrap_or(status),
            "message" => message = atom_to_optional_string(value_to_atom(value, heap, interns)?),
            _ => value.drop_with_heap(heap),
        }
    }

    let text = message.unwrap_or_else(|| status.to_string());
    Err(SimpleException::new_msg(ExcType::SystemExit, text).into())
}

fn parser_parse_args(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    known_only: bool,
) -> RunResult<Value> {
    let (parser_id, mut positional, kwargs) = take_self_and_rest(
        args,
        heap,
        interns,
        if known_only {
            "ArgumentParser.parse_known_args"
        } else {
            "ArgumentParser.parse_args"
        },
    )?;

    let mut args_value: Option<Value> = None;
    let mut namespace_seed: Option<Value> = None;

    if !positional.is_empty() {
        args_value = Some(positional.remove(0));
    }
    if !positional.is_empty() {
        namespace_seed = Some(positional.remove(0));
    }
    positional.drop_with_heap(heap);

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            continue;
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "args" => args_value = Some(value),
            "namespace" => namespace_seed = Some(value),
            _ => value.drop_with_heap(heap),
        }
    }

    let tokens = extract_tokens(args_value, heap, interns)?;
    let (entries, remainder) = parse_parser(parser_id, tokens, namespace_seed, known_only, heap, interns)?;
    let namespace = create_namespace_value(entries, heap, interns)?;

    if known_only {
        let mut rem_values = Vec::with_capacity(remainder.len());
        for item in remainder {
            let id = heap.allocate(HeapData::Str(Str::from(item)))?;
            rem_values.push(Value::Ref(id));
        }
        let rem_list = Value::Ref(heap.allocate(HeapData::List(List::new(rem_values)))?) as Value;
        return Ok(allocate_tuple(smallvec![namespace, rem_list], heap)?);
    }

    if !remainder.is_empty() {
        return Err(ExcType::type_error(format!(
            "unrecognized arguments: {}",
            remainder.join(" ")
        )));
    }

    Ok(namespace)
}

fn parse_parser(
    parser_id: HeapId,
    tokens: Vec<String>,
    namespace_seed: Option<Value>,
    known_only: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Vec<(String, Atom)>, Vec<String>)> {
    let state = parser_state_snapshot(parser_id, heap)?;
    let mut entries = Vec::new();
    if let Some(seed) = namespace_seed {
        extract_namespace_seed(seed, &mut entries, heap, interns)?;
    }

    // Initialize defaults.
    for spec in &state.specs {
        if find_entry(&entries, &spec.dest).is_some() {
            continue;
        }
        let initial = if let Some(default) = &spec.default_value {
            default.clone()
        } else if let Some(default) = &state.argument_default {
            default.clone()
        } else {
            match spec.action.as_str() {
                "store_true" => Atom::Bool(false),
                "store_false" => Atom::Bool(true),
                _ => Atom::None,
            }
        };
        upsert_entry(&mut entries, spec.dest.clone(), initial);
    }

    let mut seen: AHashMap<String, usize> = AHashMap::new();
    let mut remainder = Vec::new();
    let mut positional_inputs = Vec::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];

        if token == "--" {
            positional_inputs.extend(tokens[i + 1..].iter().cloned());
            break;
        }

        if let Some(subs) = &state.subparsers
            && !token.starts_with('-')
            && let Some((_, child_id)) = subs.parsers.iter().find(|(name, _)| name == token)
        {
            if let Some(dest) = &subs.dest {
                upsert_entry(&mut entries, dest.clone(), Atom::Str(token.clone()));
                *seen.entry(dest.clone()).or_default() += 1;
            }
            let child_tokens = tokens[i + 1..].to_vec();
            let (child_entries, child_rem) = parse_parser(*child_id, child_tokens, None, known_only, heap, interns)?;
            for (k, v) in child_entries {
                upsert_entry(&mut entries, k, v);
            }
            remainder.extend(child_rem);
            break;
        }

        if token.starts_with('-') && token != "-" {
            if let Some((spec_idx, consumed_token, attached_value)) = find_optional_spec(&state, token) {
                let spec = &state.specs[spec_idx];
                let mut values = Vec::new();
                let expected = effective_nargs(spec);

                let consumed = match expected {
                    NargsSpec::Exact(0) => 0,
                    NargsSpec::Exact(n) => {
                        if let Some(attached) = attached_value {
                            values.push(attached);
                            let needed = n.saturating_sub(1);
                            for offset in 0..needed {
                                let idx = i + 1 + offset;
                                if idx >= tokens.len() {
                                    return Err(ExcType::type_error(format!(
                                        "argument {consumed_token}: expected {n} value(s)"
                                    )));
                                }
                                values.push(tokens[idx].clone());
                            }
                            needed
                        } else {
                            for offset in 0..n {
                                let idx = i + 1 + offset;
                                if idx >= tokens.len() {
                                    return Err(ExcType::type_error(format!(
                                        "argument {consumed_token}: expected {n} value(s)"
                                    )));
                                }
                                values.push(tokens[idx].clone());
                            }
                            n
                        }
                    }
                    NargsSpec::Optional => {
                        if let Some(attached) = attached_value {
                            values.push(attached);
                            0
                        } else if i + 1 < tokens.len() && !tokens[i + 1].starts_with('-') {
                            values.push(tokens[i + 1].clone());
                            1
                        } else {
                            0
                        }
                    }
                    NargsSpec::ZeroOrMore => {
                        if let Some(attached) = attached_value {
                            values.push(attached);
                        }
                        let mut consumed = 0usize;
                        let mut idx = i + 1;
                        while idx < tokens.len() && !tokens[idx].starts_with('-') {
                            values.push(tokens[idx].clone());
                            consumed += 1;
                            idx += 1;
                        }
                        consumed
                    }
                    NargsSpec::OneOrMore => {
                        if let Some(attached) = attached_value {
                            values.push(attached);
                        }
                        let mut consumed = 0usize;
                        let mut idx = i + 1;
                        while idx < tokens.len() && !tokens[idx].starts_with('-') {
                            values.push(tokens[idx].clone());
                            consumed += 1;
                            idx += 1;
                        }
                        if values.is_empty() {
                            return Err(ExcType::type_error(format!(
                                "argument {consumed_token}: expected at least one value"
                            )));
                        }
                        consumed
                    }
                };

                apply_spec(
                    spec,
                    &values,
                    &mut entries,
                    &mut seen,
                    heap,
                    interns,
                    &state,
                    known_only,
                )?;
                i += 1 + consumed;
                continue;
            }

            remainder.push(token.clone());
            i += 1;
            continue;
        }

        positional_inputs.push(token.clone());
        i += 1;
    }

    // Parse positional specs.
    let positional_specs: Vec<&ArgSpec> = state.specs.iter().filter(|spec| !spec.optional).collect();
    let mut cursor = 0usize;
    for (idx, spec) in positional_specs.iter().enumerate() {
        let remaining_specs = &positional_specs[idx + 1..];
        let remaining_min = remaining_specs
            .iter()
            .map(|s| match effective_nargs(s) {
                NargsSpec::Exact(n) => n,
                NargsSpec::Optional | NargsSpec::ZeroOrMore => 0,
                NargsSpec::OneOrMore => 1,
            })
            .sum::<usize>();

        let nargs = effective_nargs(spec);
        let mut values = Vec::new();
        match nargs {
            NargsSpec::Exact(n) => {
                if cursor + n > positional_inputs.len() {
                    return Err(ExcType::type_error(format!("missing required argument: {}", spec.dest)));
                }
                values.extend_from_slice(&positional_inputs[cursor..cursor + n]);
                cursor += n;
            }
            NargsSpec::Optional => {
                if cursor < positional_inputs.len() {
                    values.push(positional_inputs[cursor].clone());
                    cursor += 1;
                }
            }
            NargsSpec::ZeroOrMore => {
                let available = positional_inputs.len().saturating_sub(cursor);
                let take = available.saturating_sub(remaining_min);
                values.extend_from_slice(&positional_inputs[cursor..cursor + take]);
                cursor += take;
            }
            NargsSpec::OneOrMore => {
                let available = positional_inputs.len().saturating_sub(cursor);
                let take = available.saturating_sub(remaining_min);
                if take == 0 {
                    return Err(ExcType::type_error(format!("missing required argument: {}", spec.dest)));
                }
                values.extend_from_slice(&positional_inputs[cursor..cursor + take]);
                cursor += take;
            }
        }

        apply_spec(
            spec,
            &values,
            &mut entries,
            &mut seen,
            heap,
            interns,
            &state,
            known_only,
        )?;
    }

    if cursor < positional_inputs.len() {
        remainder.extend_from_slice(&positional_inputs[cursor..]);
    }

    // Required optionals.
    for spec in &state.specs {
        if spec.optional && spec.required && seen.get(&spec.dest).copied().unwrap_or(0) == 0 {
            return Err(ExcType::type_error(format!(
                "argument {} is required",
                spec.names.first().map_or(spec.dest.as_str(), String::as_str)
            )));
        }
    }

    // Mutually exclusive group constraints.
    for group in &state.mutex_groups {
        let count = group
            .members
            .iter()
            .map(|name| seen.get(name).copied().unwrap_or(0))
            .filter(|count| *count > 0)
            .count();
        if count > 1 {
            return Err(ExcType::type_error(
                "not allowed with argument in mutually exclusive group",
            ));
        }
        if group.required && count == 0 {
            return Err(ExcType::type_error("one of the arguments in this group is required"));
        }
    }

    if let Some(subs) = &state.subparsers
        && subs.required
        && subs
            .dest
            .as_ref()
            .is_some_and(|dest| seen.get(dest).copied().unwrap_or(0) == 0)
    {
        return Err(ExcType::type_error("a subcommand is required"));
    }

    Ok((entries, remainder))
}

#[expect(clippy::too_many_arguments)]
fn apply_spec(
    spec: &ArgSpec,
    raw_values: &[String],
    entries: &mut Vec<(String, Atom)>,
    seen: &mut AHashMap<String, usize>,
    _heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    _state: &ParserState,
    _known_only: bool,
) -> RunResult<()> {
    let mut values = Vec::new();
    for raw in raw_values {
        let value = apply_type_conv(spec.type_conv, raw)?;
        if let Some(choices) = &spec.choices
            && !choices.iter().any(|choice| atom_eq(choice, &value))
        {
            return Err(ExcType::type_error(format!("invalid choice: {raw}")));
        }
        values.push(value);
    }

    match spec.action.as_str() {
        "store" => {
            let value = match effective_nargs(spec) {
                NargsSpec::Exact(0) => spec.const_value.clone().unwrap_or(Atom::None),
                NargsSpec::Exact(1) | NargsSpec::Optional => values.first().cloned().unwrap_or(Atom::None),
                _ => Atom::List(values),
            };
            upsert_entry(entries, spec.dest.clone(), value);
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "store_const" => {
            upsert_entry(
                entries,
                spec.dest.clone(),
                spec.const_value.clone().unwrap_or(Atom::None),
            );
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "store_true" => {
            upsert_entry(entries, spec.dest.clone(), Atom::Bool(true));
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "store_false" => {
            upsert_entry(entries, spec.dest.clone(), Atom::Bool(false));
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "append" => {
            let existing = find_entry(entries, &spec.dest)
                .cloned()
                .unwrap_or(Atom::List(Vec::new()));
            let mut items = match existing {
                Atom::List(items) => items,
                Atom::None => Vec::new(),
                other => vec![other],
            };
            let appended = match effective_nargs(spec) {
                NargsSpec::Exact(1) | NargsSpec::Optional => values.first().cloned().unwrap_or(Atom::None),
                _ => Atom::List(values),
            };
            items.push(appended);
            upsert_entry(entries, spec.dest.clone(), Atom::List(items));
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "append_const" => {
            let existing = find_entry(entries, &spec.dest)
                .cloned()
                .unwrap_or(Atom::List(Vec::new()));
            let mut items = match existing {
                Atom::List(items) => items,
                Atom::None => Vec::new(),
                other => vec![other],
            };
            items.push(spec.const_value.clone().unwrap_or(Atom::None));
            upsert_entry(entries, spec.dest.clone(), Atom::List(items));
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "count" => {
            let current = match find_entry(entries, &spec.dest) {
                Some(Atom::Int(i)) => *i,
                _ => 0,
            };
            upsert_entry(entries, spec.dest.clone(), Atom::Int(current + 1));
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
        "help" => return Err(SimpleException::new_msg(ExcType::SystemExit, "0").into()),
        "version" => {
            let text = spec.version.clone().unwrap_or_else(|| "0".to_owned());
            return Err(SimpleException::new_msg(ExcType::SystemExit, text).into());
        }
        _ => {
            let value = values.first().cloned().unwrap_or(Atom::None);
            upsert_entry(entries, spec.dest.clone(), value);
            *seen.entry(spec.dest.clone()).or_default() += 1;
        }
    }

    Ok(())
}

fn parser_state_snapshot(parser_id: HeapId, heap: &Heap<impl ResourceTracker>) -> RunResult<ParserState> {
    let mut rt = runtime().lock().expect("argparse runtime mutex poisoned");
    prune_runtime(&mut rt, heap);
    let Some(state) = rt.parsers.get(&parser_id) else {
        return Err(ExcType::type_error("invalid ArgumentParser object"));
    };
    Ok(state.clone())
}

fn render_usage(state: &ParserState) -> String {
    if let Some(usage) = &state.usage {
        return usage.clone();
    }

    let mut parts = vec!["usage:".to_owned(), state.prog.clone()];

    for spec in state.specs.iter().filter(|spec| spec.optional) {
        let display = spec.names.first().cloned().unwrap_or_else(|| spec.dest.clone());
        let item = if effective_nargs(spec) == NargsSpec::Exact(0) {
            format!("[{display}]")
        } else {
            format!(
                "[{display} {}]",
                spec.metavar.clone().unwrap_or_else(|| spec.dest.to_uppercase())
            )
        };
        parts.push(item);
    }

    for spec in state.specs.iter().filter(|spec| !spec.optional) {
        parts.push(spec.metavar.clone().unwrap_or_else(|| spec.dest.clone()));
    }

    if let Some(subs) = &state.subparsers
        && !subs.parsers.is_empty()
    {
        let names: Vec<&str> = subs.parsers.iter().map(|(name, _)| name.as_str()).collect();
        parts.push(format!("{{{}}}", names.join(",")));
    }

    parts.join(" ")
}

fn render_help(state: &ParserState) -> String {
    let mut out = String::new();
    out.push_str(&render_usage(state));
    out.push('\n');

    if let Some(description) = &state.description
        && !description.is_empty()
    {
        out.push('\n');
        out.push_str(description);
        out.push('\n');
    }

    let optionals: Vec<&ArgSpec> = state.specs.iter().filter(|spec| spec.optional).collect();
    if !optionals.is_empty() {
        out.push('\n');
        out.push_str("options:\n");
        for spec in optionals {
            let names = spec.names.join(", ");
            out.push_str("  ");
            out.push_str(&names);
            if !matches!(effective_nargs(spec), NargsSpec::Exact(0)) {
                out.push(' ');
                out.push_str(&spec.metavar.clone().unwrap_or_else(|| spec.dest.to_uppercase()));
            }
            if let Some(help) = &spec.help {
                out.push_str("  ");
                out.push_str(help);
            }
            out.push('\n');
        }
    }

    let positionals: Vec<&ArgSpec> = state.specs.iter().filter(|spec| !spec.optional).collect();
    if !positionals.is_empty() {
        out.push('\n');
        out.push_str("positional arguments:\n");
        for spec in positionals {
            out.push_str("  ");
            out.push_str(&spec.metavar.clone().unwrap_or_else(|| spec.dest.clone()));
            if let Some(help) = &spec.help {
                out.push_str("  ");
                out.push_str(help);
            }
            out.push('\n');
        }
    }

    if let Some(epilog) = &state.epilog
        && !epilog.is_empty()
    {
        out.push('\n');
        out.push_str(epilog);
        out.push('\n');
    }

    out
}

fn take_self_and_rest(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    name: &str,
) -> RunResult<(HeapId, Vec<Value>, Vec<(Value, Value)>)> {
    let (positional, kwargs) = args.into_parts();
    let mut pos = positional.into_iter();
    let Some(self_value) = pos.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(format!("{name} missing bound self")));
    };
    let parser_id = match &self_value {
        Value::Ref(id) => *id,
        other => {
            let ty = other.py_type(heap);
            self_value.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("{name} expected bound object, got {ty}")));
        }
    };
    // Consume the bound receiver from call arguments.
    self_value.drop_with_heap(heap);

    let positional: Vec<Value> = pos.collect();
    let kwargs: Vec<(Value, Value)> = kwargs.into_iter().collect();
    let _ = interns;
    Ok((parser_id, positional, kwargs))
}

fn extract_tokens(
    args_value: Option<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<String>> {
    let Some(value) = args_value else {
        return Ok(Vec::new());
    };

    let tokens = match &value {
        Value::None => Vec::new(),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::List(list) => {
                let mut out = Vec::with_capacity(list.as_vec().len());
                for item in list.as_vec() {
                    out.push(item.py_str(heap, interns).into_owned());
                }
                out
            }
            HeapData::Tuple(tuple) => {
                let mut out = Vec::with_capacity(tuple.as_vec().len());
                for item in tuple.as_vec() {
                    out.push(item.py_str(heap, interns).into_owned());
                }
                out
            }
            HeapData::Str(s) => s.as_str().split_whitespace().map(ToOwned::to_owned).collect(),
            _ => vec![Value::Ref(*id).py_str(heap, interns).into_owned()],
        },
        _ => vec![value.py_str(heap, interns).into_owned()],
    };
    value.drop_with_heap(heap);
    Ok(tokens)
}

fn value_to_atom(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Atom> {
    let atom = match &value {
        Value::None => Atom::None,
        Value::Bool(b) => Atom::Bool(*b),
        Value::Int(i) => Atom::Int(*i),
        Value::Float(f) => Atom::Float(*f),
        Value::InternString(id) => Atom::Str(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Atom::Str(s.as_str().to_owned()),
            _ => Atom::Str(value.py_str(heap, interns).into_owned()),
        },
        _ => Atom::Str(value.py_str(heap, interns).into_owned()),
    };
    value.drop_with_heap(heap);
    Ok(atom)
}

fn atom_to_value(atom: &Atom, heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
    Ok(match atom {
        Atom::None => Value::None,
        Atom::Bool(b) => Value::Bool(*b),
        Atom::Int(i) => Value::Int(*i),
        Atom::Float(f) => Value::Float(*f),
        Atom::Str(s) => Value::Ref(heap.allocate(HeapData::Str(Str::from(s.as_str())))?),
        Atom::List(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(atom_to_value(item, heap)?);
            }
            Value::Ref(heap.allocate(HeapData::List(List::new(values)))?)
        }
    })
}

fn atom_to_optional_string(atom: Atom) -> Option<String> {
    match atom {
        Atom::None => None,
        Atom::Str(s) => Some(s),
        Atom::Int(i) => Some(i.to_string()),
        Atom::Float(f) => Some(f.to_string()),
        Atom::Bool(b) => Some(if b { "True" } else { "False" }.to_owned()),
        Atom::List(_) => None,
    }
}

fn atom_to_int(atom: Atom) -> Option<i64> {
    match atom {
        Atom::Int(i) => Some(i),
        Atom::Bool(b) => Some(i64::from(b)),
        Atom::Str(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn value_to_string(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let s = value.py_str(heap, interns).into_owned();
    value.drop_with_heap(heap);
    Ok(s)
}

fn value_to_type_conv(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<TypeConv> {
    let conv = match value {
        Value::Builtin(Builtins::Type(Type::Str)) => TypeConv::Str,
        Value::Builtin(Builtins::Type(Type::Int)) => TypeConv::Int,
        Value::Builtin(Builtins::Type(Type::Float)) => TypeConv::Float,
        Value::None => TypeConv::Raw,
        Value::Ref(id) => {
            if let Some(builtin_type) = heap.builtin_type_for_class_id(id) {
                Value::Ref(id).drop_with_heap(heap);
                match builtin_type {
                    Type::Str => TypeConv::Str,
                    Type::Int => TypeConv::Int,
                    Type::Float => TypeConv::Float,
                    _ => TypeConv::Raw,
                }
            } else {
                let name = Value::Ref(id).py_str(heap, interns).into_owned();
                Value::Ref(id).drop_with_heap(heap);
                match name.as_str() {
                    "str" => TypeConv::Str,
                    "int" => TypeConv::Int,
                    "float" => TypeConv::Float,
                    _ => TypeConv::Raw,
                }
            }
        }
        other => {
            let text = other.py_str(heap, interns).into_owned();
            other.drop_with_heap(heap);
            match text.as_str() {
                "str" => TypeConv::Str,
                "int" => TypeConv::Int,
                "float" => TypeConv::Float,
                _ => TypeConv::Raw,
            }
        }
    };
    Ok(conv)
}

fn value_to_choices(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<Atom>> {
    let choices = match &value {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::List(list) => {
                let mut out = Vec::with_capacity(list.as_vec().len());
                for item in list.as_vec() {
                    out.push(Atom::Str(item.py_str(heap, interns).into_owned()));
                }
                out
            }
            HeapData::Tuple(tuple) => {
                let mut out = Vec::with_capacity(tuple.as_vec().len());
                for item in tuple.as_vec() {
                    out.push(Atom::Str(item.py_str(heap, interns).into_owned()));
                }
                out
            }
            _ => vec![Atom::Str(value.py_str(heap, interns).into_owned())],
        },
        _ => vec![Atom::Str(value.py_str(heap, interns).into_owned())],
    };
    value.drop_with_heap(heap);
    Ok(choices)
}

fn parse_nargs(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<NargsSpec> {
    let spec = match &value {
        Value::Int(i) if *i >= 0 => NargsSpec::Exact(*i as usize),
        Value::InternString(id) => match interns.get_str(*id) {
            "?" => NargsSpec::Optional,
            "*" => NargsSpec::ZeroOrMore,
            "+" => NargsSpec::OneOrMore,
            other => {
                let parsed = other
                    .parse::<usize>()
                    .map_err(|_| ExcType::type_error(format!("invalid nargs value: {other}")))?;
                NargsSpec::Exact(parsed)
            }
        },
        Value::Ref(id) => {
            if let HeapData::Str(s) = heap.get(*id) {
                match s.as_str() {
                    "?" => NargsSpec::Optional,
                    "*" => NargsSpec::ZeroOrMore,
                    "+" => NargsSpec::OneOrMore,
                    other => {
                        let parsed = other
                            .parse::<usize>()
                            .map_err(|_| ExcType::type_error(format!("invalid nargs value: {other}")))?;
                        NargsSpec::Exact(parsed)
                    }
                }
            } else {
                let text = value.py_str(heap, interns).into_owned();
                let parsed = text
                    .parse::<usize>()
                    .map_err(|_| ExcType::type_error(format!("invalid nargs value: {text}")))?;
                NargsSpec::Exact(parsed)
            }
        }
        _ => {
            let text = value.py_str(heap, interns).into_owned();
            let parsed = text
                .parse::<usize>()
                .map_err(|_| ExcType::type_error(format!("invalid nargs value: {text}")))?;
            NargsSpec::Exact(parsed)
        }
    };
    value.drop_with_heap(heap);
    Ok(spec)
}

fn apply_type_conv(conv: TypeConv, raw: &str) -> RunResult<Atom> {
    match conv {
        TypeConv::Raw | TypeConv::Str => Ok(Atom::Str(raw.to_owned())),
        TypeConv::Int => raw
            .parse::<i64>()
            .map(Atom::Int)
            .map_err(|_| ExcType::type_error(format!("invalid int value: {raw}"))),
        TypeConv::Float => raw
            .parse::<f64>()
            .map(Atom::Float)
            .map_err(|_| ExcType::type_error(format!("invalid float value: {raw}"))),
    }
}

fn atom_eq(left: &Atom, right: &Atom) -> bool {
    match (left, right) {
        (Atom::None, Atom::None) => true,
        (Atom::Bool(a), Atom::Bool(b)) => a == b,
        (Atom::Int(a), Atom::Int(b)) => a == b,
        (Atom::Float(a), Atom::Float(b)) => a == b,
        (Atom::Str(a), Atom::Str(b)) => a == b,
        (Atom::List(a), Atom::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| atom_eq(x, y)),
        _ => false,
    }
}

fn effective_nargs(spec: &ArgSpec) -> NargsSpec {
    if let Some(nargs) = spec.nargs {
        return nargs;
    }
    match spec.action.as_str() {
        "store_const" | "store_true" | "store_false" | "count" | "help" | "version" | "append_const" => {
            NargsSpec::Exact(0)
        }
        _ => NargsSpec::Exact(1),
    }
}

fn find_optional_spec(state: &ParserState, token: &str) -> Option<(usize, String, Option<String>)> {
    let (head, attached) = if let Some((left, right)) = token.split_once('=') {
        (left, Some(right.to_owned()))
    } else {
        (token, None)
    };

    if let Some((idx, _)) = state
        .specs
        .iter()
        .enumerate()
        .find(|(_, spec)| spec.optional && spec.names.iter().any(|name| name == head))
    {
        return Some((idx, head.to_owned(), attached));
    }

    if state.allow_abbrev && head.starts_with("--") {
        let mut matches = Vec::new();
        for (idx, spec) in state.specs.iter().enumerate() {
            if !spec.optional {
                continue;
            }
            for name in &spec.names {
                if name.starts_with("--") && name.starts_with(head) {
                    matches.push((idx, name.clone()));
                }
            }
        }
        if matches.len() == 1 {
            let (idx, matched) = matches.remove(0);
            return Some((idx, matched, attached));
        }
    }

    None
}

fn derive_dest_from_flags(names: &[String]) -> String {
    let selected = names
        .iter()
        .filter(|name| name.starts_with('-'))
        .max_by_key(|name| name.len())
        .cloned()
        .unwrap_or_else(|| names[0].clone());

    selected.trim_start_matches('-').replace('-', "_").trim().to_owned()
}

fn upsert_entry(entries: &mut Vec<(String, Atom)>, name: String, value: Atom) {
    if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| *key == name) {
        *existing = value;
    } else {
        entries.push((name, value));
    }
}

fn find_entry<'a>(entries: &'a [(String, Atom)], name: &str) -> Option<&'a Atom> {
    entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn create_namespace_value(
    entries: Vec<(String, Atom)>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut dict = Dict::new();
    let mut field_names = Vec::with_capacity(entries.len());

    for (name, atom) in entries {
        field_names.push(name.clone());
        let key = Value::Ref(heap.allocate(HeapData::Str(Str::from(name.as_str())))?) as Value;
        let value = atom_to_value(&atom, heap)?;
        let replaced = dict.set(key, value, heap, interns)?;
        if let Some(old) = replaced {
            old.drop_with_heap(heap);
        }
    }

    let dc = Dataclass::new(
        "Namespace".to_owned(),
        heap.next_class_uid(),
        field_names,
        dict,
        AHashSet::new(),
        false,
    );
    let id = heap.allocate(HeapData::Dataclass(dc))?;
    Ok(Value::Ref(id))
}

fn call_value_sync(
    callable: Value,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match callable {
        Value::Builtin(builtin) => {
            let mut print = NoPrint;
            builtin.call(heap, args, interns, &mut print)
        }
        Value::ModuleFunction(module_function) => match module_function.call(heap, interns, args)? {
            AttrCallResult::Value(value) => Ok(value),
            _ => Err(ExcType::type_error("argparse helper expected a value result")),
        },
        Value::Ref(heap_id) => {
            if let Some(builtin_type) = heap.builtin_type_for_class_id(heap_id) {
                let mut print = NoPrint;
                let result = Builtins::Type(builtin_type).call(heap, args, interns, &mut print);
                Value::Ref(heap_id).drop_with_heap(heap);
                return result;
            }
            let ty = heap.get(heap_id).py_type(heap);
            Value::Ref(heap_id).drop_with_heap(heap);
            args.drop_with_heap(heap);
            Err(ExcType::type_error(format!("'{ty}' object is not callable")))
        }
        other => {
            args.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{}' object is not callable",
                other.py_type(heap)
            )))
        }
    }
}
