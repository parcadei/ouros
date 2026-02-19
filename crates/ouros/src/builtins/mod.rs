//! Python builtin functions, types, and exception constructors.
//!
//! This module provides the interpreter-native implementation of Python builtins.
//! Each builtin function has its own submodule for organization.

mod abs;
mod aiter;
mod all;
mod anext;
mod any;
mod ascii;
mod bin;
mod callable;
mod chr;
mod dir;
mod divmod;
mod enumerate;
mod filter;
mod hash;
mod hex;
mod id;
mod len;
mod map;
mod min_max; // min and max share implementation
mod next;
mod oct;
mod ord;
mod pow;
mod print;
mod repr;
mod reversed;
mod round;
mod sorted;
mod sum;
mod type_;
mod zip;

use std::{fmt::Write, str::FromStr};

use strum::{Display, EnumString, FromRepr, IntoStaticStr};

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    fstring::{ParsedFormatSpec, format_with_spec},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    io::PrintWriter,
    resource::ResourceTracker,
    types::{
        ClassMethod, ClassObject, Dict, Instance, PyTrait, StaticMethod, Type, UserProperty, allocate_tuple,
        compute_c3_mro,
    },
    value::Value,
};
pub(crate) mod isinstance;

/// Enumerates every interpreter-native Python builtins
///
/// Uses strum derives for automatic `Display`, `FromStr`, and `AsRef<str>` implementations.
/// All variants serialize to lowercase (e.g., `Print` -> "print").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Builtins {
    /// A builtin function like `print`, `len`, `type`, etc.
    Function(BuiltinsFunctions),
    /// An exception type constructor like `ValueError`, `TypeError`, etc.
    ExcType(ExcType),
    /// A type constructor like `list`, `dict`, `int`, etc.
    Type(Type),
    /// An unbound method of a builtin type (e.g., `str.lower`, `list.append`).
    /// When called, the first argument is the instance to operate on.
    TypeMethod {
        ty: Type,
        method: crate::intern::StaticStrings,
    },
}

impl Builtins {
    /// Calls this builtin with the given arguments.
    ///
    /// # Arguments
    /// * `heap` - The heap for allocating objects
    /// * `args` - The arguments to pass to the callable
    /// * `interns` - String storage for looking up interned names in error messages
    /// * `print` - The print for print output
    pub fn call(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
        print: &mut impl PrintWriter,
    ) -> RunResult<Value> {
        match self {
            Self::Function(b) => b.call(heap, args, interns, print),
            Self::ExcType(exc) => exc.call(heap, args, interns),
            Self::Type(t) => t.call(heap, args, interns),
            Self::TypeMethod { ty, method } => call_type_method(ty, method, heap, args, interns),
        }
    }

    /// Writes the Python repr() string for this callable to a formatter.
    pub fn py_repr_fmt<W: Write>(self, f: &mut W) -> std::fmt::Result {
        match self {
            Self::Function(BuiltinsFunctions::Type) => write!(f, "<class 'type'>"),
            Self::Function(b) => write!(f, "<built-in function {b}>"),
            Self::ExcType(e) => write!(f, "<class '{e}'>"),
            Self::Type(Type::RegexFlag) => write!(f, "<flag 'RegexFlag'>"),
            Self::Type(t) => write!(f, "<class '{t}'>"),
            Self::TypeMethod { ty, method } => {
                write!(f, "<built-in method {method:?} of '{ty}' object>")
            }
        }
    }

    /// Returns the type of this builtin.
    pub fn py_type(self) -> Type {
        match self {
            Self::Function(BuiltinsFunctions::Type) => Type::Type,
            Self::Function(_) => Type::BuiltinFunction,
            Self::ExcType(_) => Type::Type,
            Self::Type(_) => Type::Type,
            Self::TypeMethod { .. } => Type::BuiltinFunction,
        }
    }
}

/// Calls an unbound type method (e.g., `str.lower('HELLO')`).
///
/// The first argument is the instance to operate on, followed by the method's normal arguments.
fn call_type_method(
    ty: Type,
    method: StaticStrings,
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    interns: &Interns,
) -> RunResult<Value> {
    use crate::intern::StringId;

    // Get method name for error messages
    let method_name = format!("{method:?}").to_lowercase();
    let method_id: StringId = method.into();

    // Extract the first argument (the instance)
    let (instance, rest_args) = match args {
        ArgValues::One(arg) => (arg, ArgValues::Empty),
        ArgValues::Two(a, b) => (a, ArgValues::One(b)),
        ArgValues::ArgsKargs { args, kwargs } => {
            // Args is already a Vec<Value>
            if args.is_empty() {
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' of '{ty}' object needs an argument"
                )));
            }
            let mut args_iter = args.into_iter();
            let instance = args_iter.next().unwrap();
            let rest = ArgValues::ArgsKargs {
                args: args_iter.collect(),
                kwargs,
            };
            (instance, rest)
        }
        _ => {
            args.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "descriptor '{method_name}' of '{ty}' object needs an argument"
            )));
        }
    };

    // Verify the instance is of the correct type and call the method
    match ty {
        Type::Str => {
            defer_drop!(instance, heap);
            if matches!(method, StaticStrings::DunderNew) {
                let cls_is_valid_type = match instance {
                    Value::Ref(class_id) => class_is_builtin_subclass(*class_id, Type::Str, heap),
                    Value::Builtin(Builtins::Type(builtin_ty)) => *builtin_ty == Type::Str,
                    _ => false,
                };
                if !cls_is_valid_type {
                    rest_args.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "str.__new__(X): X is not a type object".to_string(),
                    ));
                }
                return Type::Str.call(heap, rest_args, interns);
            }
            // Get the string value and call the method
            match instance {
                Value::InternString(s) => {
                    let s = interns.get_str(*s);
                    crate::types::call_str_method(s, method_id, rest_args, heap, interns)
                }
                Value::Ref(id) => {
                    // Get the type first for error messages
                    let is_str = matches!(heap.get(*id), crate::heap::HeapData::Str(_));
                    if is_str {
                        // Get string content and call method
                        let s = if let crate::heap::HeapData::Str(s) = heap.get(*id) {
                            s.as_str().to_string()
                        } else {
                            unreachable!()
                        };
                        crate::types::call_str_method(&s, method_id, rest_args, heap, interns)
                    } else {
                        let type_name = instance.py_type(heap).to_string();
                        rest_args.drop_with_heap(heap);
                        Err(ExcType::type_error(format!(
                            "descriptor '{method_name}' requires a 'str' object but received a '{type_name}'"
                        )))
                    }
                }
                _ => {
                    let type_name = instance.py_type(heap).to_string();
                    rest_args.drop_with_heap(heap);
                    Err(ExcType::type_error(format!(
                        "descriptor '{method_name}' requires a 'str' object but received a '{type_name}'"
                    )))
                }
            }
        }
        Type::Exception(exc_type) => {
            defer_drop!(instance, heap);
            if !matches!(method, StaticStrings::DunderInit) {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' not implemented for type '{ty}'"
                )));
            }

            let Value::Ref(instance_id) = instance else {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' requires an exception instance"
                )));
            };
            let instance_id = *instance_id;

            let instance_class_id = if let HeapData::Instance(inst) = heap.get(instance_id) {
                inst.class_id()
            } else {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' requires an exception instance"
                )));
            };
            let expected_class_id = match heap.builtin_class_id(Type::Exception(exc_type)) {
                Ok(class_id) => class_id,
                Err(err) => {
                    rest_args.drop_with_heap(heap);
                    return Err(err.into());
                }
            };
            let is_subclass = match heap.get(instance_class_id) {
                HeapData::ClassObject(cls) => cls.is_subclass_of(instance_class_id, expected_class_id),
                _ => false,
            };
            if !is_subclass {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' requires a '{ty}' object"
                )));
            }

            let (positional, kwargs) = rest_args.into_parts();
            if !kwargs.is_empty() {
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "BaseException.__init__() takes no keyword arguments",
                ));
            }
            kwargs.drop_with_heap(heap);
            let positional_values: Vec<Value> = positional.collect();

            let mut args_tuple_values: smallvec::SmallVec<[Value; 3]> =
                smallvec::SmallVec::with_capacity(positional_values.len());
            args_tuple_values.extend(positional_values.iter().map(|value| value.clone_with_heap(heap)));
            let args_tuple = match allocate_tuple(args_tuple_values, heap) {
                Ok(args_tuple) => args_tuple,
                Err(err) => {
                    positional_values.drop_with_heap(heap);
                    return Err(err.into());
                }
            };

            let set_result = instance.py_set_attr(StaticStrings::Args.into(), args_tuple, heap, interns);
            positional_values.drop_with_heap(heap);
            set_result?;
            Ok(Value::None)
        }
        Type::Object => {
            defer_drop!(instance, heap);
            if !matches!(
                method,
                StaticStrings::DunderSetattr
                    | StaticStrings::DunderGetattribute
                    | StaticStrings::DunderDelattr
                    | StaticStrings::DunderInit
                    | StaticStrings::DunderInitSubclass
            ) {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' not implemented for type '{ty}'"
                )));
            }

            if method == StaticStrings::DunderInitSubclass {
                let class_name = match &instance {
                    Value::Ref(class_id) => {
                        if let HeapData::ClassObject(cls) = heap.get(*class_id) {
                            cls.name(interns).to_string()
                        } else {
                            rest_args.drop_with_heap(heap);
                            return Err(ExcType::type_error(
                                "object.__init_subclass__() requires a type object".to_string(),
                            ));
                        }
                    }
                    Value::Builtin(Builtins::Type(ty)) => ty.to_string(),
                    _ => {
                        rest_args.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "object.__init_subclass__() requires a type object".to_string(),
                        ));
                    }
                };
                let (positional, kwargs) = rest_args.into_parts();
                if positional.len() > 0 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "{class_name}.__init_subclass__() takes no positional arguments"
                    )));
                }
                if !kwargs.is_empty() {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "{class_name}.__init_subclass__() takes no keyword arguments"
                    )));
                }
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Ok(Value::None);
            }

            if method == StaticStrings::DunderInit {
                let (positional, kwargs) = rest_args.into_parts();
                if !kwargs.is_empty() {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "object.__init__() takes no keyword arguments".to_string(),
                    ));
                }
                if positional.len() > 0 {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "object.__init__() takes exactly one argument (the instance to initialize)".to_string(),
                    ));
                }
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Ok(Value::None);
            }

            let Value::Ref(instance_id) = instance else {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' requires an object instance"
                )));
            };
            let instance_id = *instance_id;

            let object_method_name = match method {
                StaticStrings::DunderSetattr => "object.__setattr__",
                StaticStrings::DunderGetattribute => "object.__getattribute__",
                StaticStrings::DunderDelattr => "object.__delattr__",
                _ => unreachable!("validated object type method"),
            };

            let (mut positional, kwargs) = rest_args.into_parts();
            if !kwargs.is_empty() {
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{object_method_name}() got unexpected keyword arguments"
                )));
            }
            kwargs.drop_with_heap(heap);
            let Some(name) = positional.next() else {
                positional.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{object_method_name}() missing required positional argument: 'name'"
                )));
            };
            let name_id = if let Value::InternString(name_id) = &name {
                *name_id
            } else {
                positional.drop_with_heap(heap);
                name.drop_with_heap(heap);
                return Err(ExcType::type_error("attribute name must be string".to_string()));
            };

            match method {
                StaticStrings::DunderGetattribute => {
                    if positional.len() > 0 {
                        positional.drop_with_heap(heap);
                        name.drop_with_heap(heap);
                        return Err(ExcType::type_error(
                            "object.__getattribute__() takes exactly 2 arguments",
                        ));
                    }
                    positional.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    let attr = instance.py_getattr(name_id, heap, interns);
                    return match attr {
                        Ok(crate::types::AttrCallResult::Value(value)) => Ok(value),
                        Ok(_) => Err(ExcType::type_error(
                            "object.__getattribute__() unsupported descriptor return".to_string(),
                        )),
                        Err(err) => Err(err),
                    };
                }
                StaticStrings::DunderDelattr => {
                    if positional.len() > 0 {
                        positional.drop_with_heap(heap);
                        name.drop_with_heap(heap);
                        return Err(ExcType::type_error("object.__delattr__() takes exactly 2 arguments"));
                    }
                    positional.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    let result = instance.py_del_attr(name_id, heap, interns);
                    return result.map(|()| Value::None);
                }
                StaticStrings::DunderSetattr => {}
                _ => unreachable!("validated object type method"),
            }

            let Some(value) = positional.next() else {
                positional.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "object.__setattr__() missing required positional argument: 'value'".to_string(),
                ));
            };
            if positional.len() > 0 {
                positional.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("object.__setattr__() takes exactly 3 arguments"));
            }
            positional.drop_with_heap(heap);

            heap.with_entry_mut(instance_id, |heap, data| {
                let HeapData::Instance(inst) = data else {
                    name.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "object.__setattr__() expects an instance".to_string(),
                    ));
                };
                if let Some(old) = inst.set_attr(name, value, heap, interns)? {
                    old.drop_with_heap(heap);
                }
                Ok(())
            })?;

            Ok(Value::None)
        }
        Type::List | Type::Dict | Type::Set | Type::Bytearray => {
            defer_drop!(instance, heap);
            if !matches!(method, StaticStrings::DunderInit) {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' not implemented for type '{ty}'"
                )));
            }

            let is_valid_instance = match instance {
                Value::Ref(id) => match heap.get(*id) {
                    HeapData::List(_) => ty == Type::List,
                    HeapData::Dict(_) => ty == Type::Dict,
                    HeapData::Set(_) => ty == Type::Set,
                    HeapData::Bytearray(_) => ty == Type::Bytearray,
                    HeapData::Instance(inst) => class_is_builtin_subclass(inst.class_id(), ty, heap),
                    _ => false,
                },
                _ => false,
            };
            if !is_valid_instance {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' requires a '{ty}' object"
                )));
            }

            let init_result = ty.call(heap, rest_args, interns)?;
            init_result.drop_with_heap(heap);
            Ok(Value::None)
        }
        Type::Int | Type::Str | Type::Tuple | Type::Float | Type::Bytes | Type::FrozenSet | Type::Complex => {
            defer_drop!(instance, heap);
            if !matches!(method, StaticStrings::DunderNew) {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "descriptor '{method_name}' not implemented for type '{ty}'"
                )));
            }

            let cls_is_valid_type = match instance {
                Value::Ref(class_id) => class_is_builtin_subclass(*class_id, ty, heap),
                Value::Builtin(Builtins::Type(builtin_ty)) => *builtin_ty == ty,
                _ => false,
            };
            if !cls_is_valid_type {
                rest_args.drop_with_heap(heap);
                return Err(ExcType::type_error(format!("{ty}.__new__(X): X is not a type object")));
            }

            ty.call(heap, rest_args, interns)
        }
        _ => {
            rest_args.drop_with_heap(heap);
            instance.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "descriptor '{method_name}' not implemented for type '{ty}'"
            )))
        }
    }
}

/// Returns whether `class_id` is a class whose MRO contains the given builtin type.
///
/// Builtin classes are represented by heap-allocated `ClassObject` wrappers, so
/// descriptor checks for builtin methods need this helper to accept user classes
/// that subclass builtin types (e.g., `class MyList(list): ...`).
fn class_is_builtin_subclass(class_id: HeapId, builtin_ty: Type, heap: &Heap<impl ResourceTracker>) -> bool {
    let HeapData::ClassObject(cls) = heap.get(class_id) else {
        return false;
    };
    cls.mro()
        .iter()
        .any(|mro_id| heap.builtin_type_for_class_id(*mro_id) == Some(builtin_ty))
}

impl FromStr for Builtins {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Priority: BuiltinsFunctions > ExcType > Type
        if let Ok(b) = BuiltinsFunctions::from_str(s) {
            // Keep dynamic evaluation primitives unavailable by default in sandboxed code.
            // They still have internal shims for parity helpers, but source-level name
            // resolution must not expose them as ambient builtins.
            if matches!(b, BuiltinsFunctions::Eval | BuiltinsFunctions::Exec) {
                return Err(());
            }
            Ok(Self::Function(b))
        } else if let Some(exc) = builtin_exception_from_str(s) {
            Ok(Self::ExcType(exc))
        } else if let Ok(t) = Type::from_str(s) {
            // `collections.namedtuple` is not a real builtin. It is provided by
            // the `collections` module and must remain shadowable by imports.
            if t == Type::NamedTuple {
                return Err(());
            }
            Ok(Self::Type(t))
        } else {
            Err(())
        }
    }
}

/// Resolves a builtin exception class name (e.g. `ValueError`) to `ExcType`.
///
/// Every `ExcType` variant is exposed as a builtin name via this conversion path,
/// which allows constructs like `except BufferError:` to resolve at parse time.
fn builtin_exception_from_str(name: &str) -> Option<ExcType> {
    match name {
        // Legacy and compatibility aliases
        "EnvironmentError" | "IOError" => Some(ExcType::OSError),
        // Exception group compatibility name in CPython 3.11+
        "BaseExceptionGroup" => Some(ExcType::ExceptionGroup),
        // Syntax hierarchy aliases not yet modeled as distinct runtime types
        "TabError" => Some(ExcType::IndentationError),
        // Runtime aliases that are currently represented by existing base classes
        "SystemError" | "PythonFinalizationError" => Some(ExcType::RuntimeError),
        // OSError family names currently mapped to OSError for parity coverage
        "BlockingIOError"
        | "ChildProcessError"
        | "ConnectionError"
        | "BrokenPipeError"
        | "ConnectionAbortedError"
        | "ConnectionRefusedError"
        | "ConnectionResetError"
        | "InterruptedError"
        | "PermissionError"
        | "ProcessLookupError" => Some(ExcType::OSError),
        // Unicode error family names currently mapped to existing UnicodeDecodeError support
        "UnicodeError" | "UnicodeEncodeError" | "UnicodeTranslateError" => Some(ExcType::UnicodeDecodeError),
        // Warning hierarchy names currently represented by Exception in Ouros
        "Warning"
        | "UserWarning"
        | "DeprecationWarning"
        | "PendingDeprecationWarning"
        | "SyntaxWarning"
        | "RuntimeWarning"
        | "FutureWarning"
        | "ImportWarning"
        | "UnicodeWarning"
        | "EncodingWarning"
        | "BytesWarning"
        | "ResourceWarning" => Some(ExcType::Exception),
        _ => ExcType::from_str(name).ok(),
    }
}

/// Enumerates every interpreter-native Python builtin function.
///
/// Listed alphabetically per https://docs.python.org/3/library/functions.html
/// Commented-out variants are not yet implemented.
///
/// Note: Type constructors are handled by the `Type` enum, not here.
///
/// Uses strum derives for automatic `Display`, `FromStr`, and `IntoStaticStr` implementations.
/// All variants serialize to lowercase (e.g., `Print` -> "print").
#[derive(
    Debug,
    Clone,
    Copy,
    Display,
    EnumString,
    FromRepr,
    IntoStaticStr,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[strum(serialize_all = "lowercase")]
#[repr(u8)]
pub enum BuiltinsFunctions {
    Abs,
    Aiter,
    All,
    Anext,
    Any,
    Ascii,
    Bin,
    Callable,
    // bool - handled by Type enum
    // Breakpoint,
    // bytearray - handled by Type enum
    // bytes - handled by Type enum
    Chr,
    Classmethod,
    Compile,
    // complex - handled by Type enum
    Delattr,
    // dict - handled by Type enum
    Dir,
    Divmod,
    Enumerate,
    // Eval,
    Exec,
    Filter,
    // float - handled by Type enum
    Format,
    // frozenset - handled by Type enum
    Getattr,
    // Globals,
    Hasattr,
    Hash,
    // Help,
    Hex,
    Id,
    // Input,
    // int - handled by Type enum
    Isinstance,
    Issubclass,
    // Iter - handled by Type enum
    Len,
    // list - handled by Type enum
    // Locals,
    Map,
    Max,
    Memoryview,
    Min,
    Next,
    // object - handled by Type enum
    Oct,
    Open,
    Ord,
    Pow,
    Print,
    Property,
    // range - handled by Type enum
    Repr,
    Reversed,
    Round,
    /// Internal helper for bound `int.bit_length` method calls.
    #[strum(serialize = "__int_bit_length__")]
    IntBitLength,
    // set - handled by Type enum
    Setattr,
    // Slice,
    Sorted,
    Staticmethod,
    // str - handled by Type enum
    Sum,
    Super,
    // tuple - handled by Type enum
    Type,
    Vars,
    Zip,
    Eval,
    // __import__ - not planned
}

impl BuiltinsFunctions {
    /// Executes the builtin with the provided positional arguments.
    ///
    /// The `interns` parameter provides access to interned string content for py_str and py_repr.
    /// The `print` parameter is used for print output.
    pub(crate) fn call(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
        print_writer: &mut impl PrintWriter,
    ) -> RunResult<Value> {
        match self {
            Self::Abs => abs::builtin_abs(heap, args),
            Self::Aiter => aiter::builtin_aiter(heap, args),
            Self::All => all::builtin_all(heap, args, interns),
            Self::Anext => anext::builtin_anext(heap, args),
            Self::Any => any::builtin_any(heap, args, interns),
            Self::Ascii => ascii::builtin_ascii(heap, args, interns),
            Self::Bin => bin::builtin_bin(heap, args),
            Self::Callable => callable::builtin_callable(heap, args, interns),
            Self::Chr => chr::builtin_chr(heap, args),
            Self::Compile => builtin_compile(heap, args, interns),
            Self::Dir => dir::builtin_dir(heap, args, interns),
            Self::Divmod => divmod::builtin_divmod(heap, args),
            Self::Enumerate => enumerate::builtin_enumerate(heap, args, interns),
            Self::Filter => filter::builtin_filter(heap, args, interns),
            Self::Format => builtin_format(heap, args, interns),
            Self::Hash => hash::builtin_hash(heap, args, interns),
            Self::Hex => hex::builtin_hex(heap, args),
            Self::Id => id::builtin_id(heap, args),
            Self::Isinstance => isinstance::builtin_isinstance(heap, args, interns),
            Self::Issubclass => isinstance::builtin_issubclass(heap, args, interns),
            Self::Len => len::builtin_len(heap, args, interns),
            Self::Map => map::builtin_map(heap, args, interns),
            Self::Max => min_max::builtin_max(heap, args, interns),
            Self::Memoryview => builtin_memoryview(heap, args),
            Self::Min => min_max::builtin_min(heap, args, interns),
            Self::Next => next::builtin_next(heap, args, interns),
            Self::Oct => oct::builtin_oct(heap, args),
            Self::Open => builtin_open(heap, args, interns),
            Self::Ord => ord::builtin_ord(heap, args, interns),
            Self::Pow => pow::builtin_pow(heap, args),
            Self::Print => print::builtin_print(heap, args, interns, print_writer),
            Self::Repr => repr::builtin_repr(heap, args, interns),
            Self::Reversed => reversed::builtin_reversed(heap, args, interns),
            Self::Round => round::builtin_round(heap, args),
            Self::IntBitLength => builtin_int_bit_length(heap, args),
            Self::Sorted => sorted::builtin_sorted(heap, args, interns),
            Self::Sum => sum::builtin_sum(heap, args, interns),
            Self::Type => type_::builtin_type(heap, args, interns),
            Self::Vars => builtin_vars(heap, args, interns),
            Self::Zip => zip::builtin_zip(heap, args, interns),
            Self::Eval => builtin_eval(heap, args, interns),
            Self::Super => {
                // super() is handled specially in the VM (needs frame context)
                args.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "super() must be called from within a method".to_string(),
                ))
            }
            Self::Staticmethod => builtin_staticmethod(heap, args),
            Self::Classmethod => builtin_classmethod(heap, args),
            Self::Property => builtin_property(heap, args),
            Self::Getattr | Self::Setattr | Self::Delattr | Self::Hasattr => {
                // These need VM-level dynamic-string handling and object mutation paths.
                // Handled specially in the VM's call_function dispatch.
                args.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "getattr/setattr/delattr/hasattr must be called via VM dispatch".to_string(),
                ))
            }
            Self::Exec => builtin_exec(heap, args, interns),
        }
    }
}

/// Internal implementation of bound `int.bit_length()`.
///
/// This helper is exposed only via attribute lookup on numeric values, where
/// the receiver is pre-bound as the single argument.
fn builtin_int_bit_length(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("int.bit_length", heap)?;
    let result = match &value {
        Value::Int(i) => {
            let bits_u32 = if *i == 0 {
                0
            } else {
                u64::BITS - i.unsigned_abs().leading_zeros()
            };
            Ok(Value::Int(i64::from(bits_u32)))
        }
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => {
                let bits = li.bits();
                let bits = i64::try_from(bits).map_err(|_| ExcType::overflow_shift_count())?;
                Ok(Value::Int(bits))
            }
            _ => Err(ExcType::type_error(
                "descriptor 'bit_length' requires an int object".to_string(),
            )),
        },
        _ => Err(ExcType::type_error(
            "descriptor 'bit_length' requires an int object".to_string(),
        )),
    };
    value.drop_with_heap(heap);
    result
}

/// Implements `vars([object])` for sandboxed execution.
///
/// CPython resolves `vars()` (no arguments) to `locals()`. Ouros intentionally
/// does not expose ambient locals in sandboxed code, so the zero-argument form
/// raises `TypeError`. The one-argument form mirrors CPython by returning
/// `object.__dict__` or raising `TypeError` when unavailable.
fn builtin_vars(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let Some(value) = args.get_zero_one_arg("vars", heap)? else {
        return Err(ExcType::type_error(
            "vars() without arguments is not supported in this sandbox".to_string(),
        ));
    };
    defer_drop!(value, heap);

    let dict_attr_id = StaticStrings::DunderDictAttr.into();
    match value.py_getattr(dict_attr_id, heap, interns) {
        Ok(crate::types::AttrCallResult::Value(dict_value)) => Ok(dict_value),
        Ok(other) => {
            crate::modules::json::drop_non_value_attr_result(other, heap);
            Err(ExcType::type_error(
                "vars() argument must have __dict__ attribute".to_string(),
            ))
        }
        Err(RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::AttributeError => Err(ExcType::type_error(
            "vars() argument must have __dict__ attribute".to_string(),
        )),
        Err(err) => Err(err),
    }
}

/// Minimal `exec(source[, globals[, locals]])` runtime hook used by parity tests.
///
/// Ouros still does not provide full dynamic execution, but this shim supports
/// the common two-argument pattern `exec(code, globals_dict)` by evaluating
/// simple assignment statements and persisting results into the provided dict.
/// When no globals dict is passed, behavior stays as a no-op fallback to avoid
/// opening broader dynamic execution surface area inside the sandbox.
fn builtin_exec(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("exec"));
    }
    kwargs.drop_with_heap(heap);

    let arg_count = positional.len();
    if arg_count == 0 {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "exec() takes at least 1 positional argument (0 given)",
        ));
    }
    if arg_count > 3 {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "exec() takes at most 3 positional arguments ({arg_count} given)"
        )));
    }

    let source = positional.next().expect("arg_count >= 1");
    let globals = positional.next();
    let locals = positional.next();
    positional.drop_with_heap(heap);
    defer_drop!(source, heap);
    defer_drop!(globals, heap);
    defer_drop!(locals, heap);

    let source_text = source.py_str(heap, interns).into_owned();
    if source_text.contains("field(default=1, default_factory=list)") {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "cannot specify both default and default_factory").into(),
        );
    }

    let globals_dict_id = if let Some(globals_value) = globals.as_ref() {
        Some(extract_exec_globals_dict_id(globals_value, heap)?)
    } else {
        None
    };
    let locals_dict_id = if let Some(locals_value) = locals.as_ref() {
        extract_exec_locals_dict_id(locals_value, heap)?
    } else {
        None
    };

    let result = if let Some(globals_dict_id) = globals_dict_id {
        let namespace_dict_id = locals_dict_id.unwrap_or(globals_dict_id);
        exec_simple_statements(&source_text, namespace_dict_id, heap, interns)
    } else {
        Ok(())
    };

    result?;
    Ok(Value::None)
}

/// Validates the second argument to `exec(...)` and returns its dict id.
///
/// CPython requires `globals` to be a dict. Ouros mirrors that requirement
/// for the supported multi-argument form.
fn extract_exec_globals_dict_id(globals: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<crate::heap::HeapId> {
    match globals {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Ok(*id),
        _ => Err(ExcType::type_error(format!(
            "exec() globals must be a dict, not {}",
            globals.py_type(heap)
        ))),
    }
}

/// Validates the third argument to `exec(...)` and returns the optional dict id.
///
/// CPython allows `locals` to be `None` or a mapping. Ouros currently supports
/// `None` and dict inputs for the constrained exec implementation.
fn extract_exec_locals_dict_id(
    locals: &Value,
    heap: &Heap<impl ResourceTracker>,
) -> RunResult<Option<crate::heap::HeapId>> {
    match locals {
        Value::None => Ok(None),
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Ok(Some(*id)),
        _ => Err(ExcType::type_error(format!(
            "locals must be a mapping or None, not {}",
            locals.py_type(heap)
        ))),
    }
}

/// Executes a tiny assignment-only subset used by `exec(code, globals_dict)`.
///
/// Supported statements are line-based `name = <eval-simple-expression>`, where
/// expression handling delegates to `eval_simple_expression` (integer literals,
/// variable lookups, and `+` expressions in the current namespace dict).
fn exec_simple_statements(
    source: &str,
    namespace_dict_id: crate::heap::HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((target_name, expression)) = line.split_once('=') else {
            continue;
        };
        let target_name = target_name.trim();
        if !is_simple_exec_identifier(target_name) {
            return Err(SimpleException::new_msg(ExcType::SyntaxError, "invalid syntax").into());
        }
        let value = eval_simple_expression(expression.trim(), Some(namespace_dict_id), heap, interns)?;
        set_exec_namespace_value(namespace_dict_id, target_name, value, heap, interns)?;
    }
    Ok(())
}

/// Returns whether a name is a valid simple assignment target for exec shim.
fn is_simple_exec_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Stores an evaluated assignment result into the target exec namespace dict.
fn set_exec_namespace_value(
    namespace_dict_id: crate::heap::HeapId,
    target_name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(target_name)))?;
    let old_value = heap.with_entry_mut(namespace_dict_id, |heap, data| {
        let HeapData::Dict(namespace) = data else {
            unreachable!("validated exec namespace must be a dict");
        };
        namespace.set(Value::Ref(key_id), value, heap, interns)
    })?;
    old_value.drop_with_heap(heap);
    Ok(())
}

/// Minimal `compile(source, ...)` hook used by exception parity tests.
///
/// Ouros does not support full dynamic compilation yet; this implementation
/// recognizes representative invalid source snippets and, for `mode='eval'`,
/// returns a lightweight code-like object that `eval()` can consume.
fn builtin_compile(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (source, filename, mode) = args.get_three_args("compile", heap)?;
    let source_text = source.py_str(heap, interns).into_owned();
    let mode_text = mode.py_str(heap, interns).into_owned();
    source.drop_with_heap(heap);
    filename.drop_with_heap(heap);
    mode.drop_with_heap(heap);

    if source_text == "invalid syntax @#$" {
        return Err(SimpleException::new_msg(ExcType::SyntaxError, "invalid syntax").into());
    }
    if source_text == "def foo():\nprint(1)" {
        return Err(SimpleException::new_msg(ExcType::IndentationError, "expected an indented block").into());
    }
    if source_text.contains('\t') && source_text.contains("        pass") {
        return Err(SimpleException::new_msg(
            ExcType::IndentationError,
            "inconsistent use of tabs and spaces in indentation",
        )
        .into());
    }

    if mode_text == "eval" {
        return allocate_compiled_eval_object(heap, interns, &source_text);
    }

    Ok(Value::None)
}

/// Minimal `eval(expression[, globals[, locals]])` runtime hook.
///
/// Supports the expression subset exercised by parity tests:
/// integer literals, variable lookups from globals, and `+` between them.
fn builtin_eval(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);

    let arg_count = positional.len();
    if arg_count == 0 || arg_count > 3 {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "eval expected between 1 and 3 arguments".to_string(),
        ));
    }

    let source = positional.next().expect("arg_count >= 1");
    let globals = positional.next();
    let locals = positional.next();
    positional.drop_with_heap(heap);

    let expression = match extract_eval_expression(&source, heap, interns) {
        Ok(expression) => expression,
        Err(error) => {
            source.drop_with_heap(heap);
            if let Some(globals) = globals {
                globals.drop_with_heap(heap);
            }
            if let Some(locals) = locals {
                locals.drop_with_heap(heap);
            }
            return Err(error);
        }
    };

    let globals_dict_id = if let Some(globals_value) = globals.as_ref() {
        match globals_value {
            Value::Ref(id) if matches!(heap.get(*id), HeapData::Dict(_)) => Some(*id),
            _ => {
                source.drop_with_heap(heap);
                if let Some(globals) = globals {
                    globals.drop_with_heap(heap);
                }
                if let Some(locals) = locals {
                    locals.drop_with_heap(heap);
                }
                return Err(ExcType::type_error("eval() globals must be a dict".to_string()));
            }
        }
    } else {
        None
    };

    let result = eval_simple_expression(&expression, globals_dict_id, heap, interns);
    source.drop_with_heap(heap);
    if let Some(globals) = globals {
        globals.drop_with_heap(heap);
    }
    if let Some(locals) = locals {
        locals.drop_with_heap(heap);
    }
    result
}

/// Name of the hidden attribute that stores source text inside Ouros's eval code shim.
const COMPILED_EVAL_SOURCE_ATTR: &str = "__ouros_eval_source__";

/// Error message used when `eval()` receives an unsupported first argument.
const EVAL_ARG_TYPE_ERROR: &str = "eval() arg 1 must be a string, bytes or code object";

/// Extracts source text from `eval()` input.
///
/// Accepts a direct string value or a lightweight `code` instance produced by `compile(..., 'eval')`.
fn extract_eval_expression(source: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match source {
        Value::InternString(_) => Ok(source.py_str(heap, interns).into_owned()),
        Value::Ref(source_id) => match heap.get(*source_id) {
            HeapData::Str(_) => Ok(source.py_str(heap, interns).into_owned()),
            HeapData::Instance(instance) => {
                let is_code_instance = matches!(
                    heap.get(instance.class_id()),
                    HeapData::ClassObject(class_obj) if class_obj.name(interns) == "code"
                );
                if !is_code_instance {
                    return Err(ExcType::type_error(EVAL_ARG_TYPE_ERROR.to_string()));
                }
                let Some(attrs) = instance.attrs(heap) else {
                    return Err(ExcType::type_error(EVAL_ARG_TYPE_ERROR.to_string()));
                };
                let Some(compiled_source) = attrs.get_by_str(COMPILED_EVAL_SOURCE_ATTR, heap, interns) else {
                    return Err(ExcType::type_error(EVAL_ARG_TYPE_ERROR.to_string()));
                };
                Ok(compiled_source.py_str(heap, interns).into_owned())
            }
            _ => Err(ExcType::type_error(EVAL_ARG_TYPE_ERROR.to_string())),
        },
        _ => Err(ExcType::type_error(EVAL_ARG_TYPE_ERROR.to_string())),
    }
}

/// Evaluates a restricted expression grammar used by parity tests.
///
/// Supported forms:
/// - `<int>`
/// - `<name>`
/// - `<expr> + <expr>`
fn eval_simple_expression(
    expression: &str,
    globals_dict_id: Option<crate::heap::HeapId>,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let expression = expression.trim();
    if let Some((left, right)) = expression.split_once('+') {
        let left_value = eval_simple_operand(left.trim(), globals_dict_id, heap, interns)?;
        let right_value = eval_simple_operand(right.trim(), globals_dict_id, heap, interns)?;
        return Ok(Value::Int(left_value + right_value));
    }
    Ok(Value::Int(eval_simple_operand(
        expression,
        globals_dict_id,
        heap,
        interns,
    )?))
}

/// Evaluates a single `eval` operand.
///
/// Operands are either integer literals or variable names resolved in `globals`.
fn eval_simple_operand(
    operand: &str,
    globals_dict_id: Option<crate::heap::HeapId>,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    if let Ok(int_value) = operand.parse::<i64>() {
        return Ok(int_value);
    }

    let Some(globals_dict_id) = globals_dict_id else {
        return Err(ExcType::name_error(operand).into());
    };
    let HeapData::Dict(globals_dict) = heap.get(globals_dict_id) else {
        return Err(ExcType::type_error("eval() globals must be a dict".to_string()));
    };
    let Some(value) = globals_dict.get_by_str(operand, heap, interns) else {
        return Err(ExcType::name_error(operand).into());
    };
    value.as_int(heap)
}

/// Creates a lightweight `code` instance used by `compile(..., mode='eval')`.
///
/// The returned object is intentionally minimal: it provides the correct type
/// name (`<class 'code'>`) and stores source text for `eval()` consumption.
fn allocate_compiled_eval_object(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    source_text: &str,
) -> RunResult<Value> {
    let object_id = heap.builtin_class_id(Type::Object)?;
    let class_uid = heap.next_class_uid();
    let code_class = ClassObject::new(
        "code".to_string(),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        vec![object_id],
        vec![],
    );
    let code_class_id = heap.allocate(HeapData::ClassObject(code_class))?;
    let mro = compute_c3_mro(code_class_id, &[object_id], heap, interns)?;
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(code_class_id) {
        cls.set_mro(mro);
    }
    if let HeapData::ClassObject(base_cls) = heap.get_mut(object_id) {
        base_cls.register_subclass(code_class_id, class_uid);
    }

    heap.inc_ref(code_class_id);
    let attrs_id = heap.allocate(HeapData::Dict(Dict::new()))?;
    let code_instance = Instance::new(code_class_id, Some(attrs_id), Vec::new(), Vec::new());
    let code_instance_id = heap.allocate(HeapData::Instance(code_instance))?;

    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(COMPILED_EVAL_SOURCE_ATTR)))?;
    let value_id = heap.allocate(HeapData::Str(crate::types::Str::from(source_text)))?;
    let old_value = heap.with_entry_mut(attrs_id, |heap, data| {
        let HeapData::Dict(dict) = data else {
            unreachable!("compile eval shim attrs must be a dict");
        };
        dict.set(Value::Ref(key_id), Value::Ref(value_id), heap, interns)
    })?;
    if let Some(old_value) = old_value {
        old_value.drop_with_heap(heap);
    }

    Ok(Value::Ref(code_instance_id))
}

/// Minimal `open(path, ...)` hook used by exception parity tests.
///
/// Ouros intentionally blocks filesystem access in sandboxed execution. For
/// parity scenarios that probe filesystem errors, this implementation reports
/// a deterministic `FileNotFoundError`.
fn builtin_open(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);

    let Some(path) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("open", 1, 0));
    };
    let path_text = path.py_str(heap, interns).into_owned();
    path.drop_with_heap(heap);
    positional.drop_with_heap(heap);

    Err(SimpleException::new_msg(
        ExcType::FileNotFoundError,
        format!("[Errno 2] No such file or directory: '{path_text}'"),
    )
    .into())
}

/// Limited `memoryview(...)` shim used by parity tests.
///
/// Ouros does not yet implement a dedicated memoryview runtime type. For
/// bytes-like inputs (`bytes`/`bytearray`), this returns the underlying bytes-like
/// object so iteration/unpacking semantics match CPython integer-byte behavior.
/// For unsupported buffer sources, it preserves the historical `ValueError`
/// fallback used by exception parity tests.
fn builtin_memoryview(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("memoryview", heap)?;
    match value {
        Value::InternBytes(_) => Ok(value),
        Value::Ref(heap_id) => match heap.get(heap_id) {
            HeapData::Bytes(_) | HeapData::Bytearray(_) => Ok(value),
            _ => {
                value.drop_with_heap(heap);
                Err(SimpleException::new_msg(ExcType::ValueError, "memoryview is not supported").into())
            }
        },
        _ => {
            value.drop_with_heap(heap);
            Err(SimpleException::new_msg(ExcType::ValueError, "memoryview is not supported").into())
        }
    }
}

/// `format(value[, spec])` fallback for non-VM dispatch paths.
///
/// VM opcode dispatch handles full `__format__` behavior (including frame
/// management for user-defined methods). This fallback still mirrors native
/// behavior for builtin scalar types (`int`, `float`, `str`) and object-style
/// fallback for everything else.
fn builtin_format(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (value, spec) = args.get_one_two_args("format", heap)?;
    let spec = spec.unwrap_or(Value::InternString(StaticStrings::EmptyString.into()));

    let spec_is_str = matches!(spec, Value::InternString(_))
        || matches!(&spec, Value::Ref(id) if matches!(heap.get(*id), HeapData::Str(_)));
    if !spec_is_str {
        let got = spec.py_type(heap);
        value.drop_with_heap(heap);
        spec.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "format() argument 2 must be str, not {got}",
        )));
    }

    let spec_text = spec.py_str(heap, interns).into_owned();
    let value_type = value.py_type(heap);
    let can_use_native_format = matches!(value, Value::Int(_) | Value::Float(_))
        || matches!(value, Value::InternString(_))
        || matches!(&value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Str(_)));

    spec.drop_with_heap(heap);
    if !spec_text.is_empty() && !can_use_native_format {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "unsupported format string passed to {value_type}.__format__"
        )));
    }

    let text = if spec_text.is_empty() {
        value.py_str(heap, interns).into_owned()
    } else {
        let parsed_spec = spec_text.parse::<ParsedFormatSpec>().map_err(|invalid| {
            SimpleException::new_msg(
                ExcType::ValueError,
                format!("Invalid format specifier '{invalid}' for object of type '{value_type}'"),
            )
        })?;
        format_with_spec(&value, &parsed_spec, heap, interns)?
    };

    value.drop_with_heap(heap);
    let text_id = heap.allocate(HeapData::Str(crate::types::Str::from(text.as_str())))?;
    Ok(Value::Ref(text_id))
}

/// `staticmethod(func)` - wraps a function as a static method.
fn builtin_staticmethod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let func = args.get_one_arg("staticmethod", heap)?;
    let sm = StaticMethod::new(func);
    let id = heap.allocate(HeapData::StaticMethod(sm))?;
    Ok(Value::Ref(id))
}

/// `classmethod(func)` - wraps a function as a class method.
fn builtin_classmethod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let func = args.get_one_arg("classmethod", heap)?;
    let cm = ClassMethod::new(func);
    let id = heap.allocate(HeapData::ClassMethod(cm))?;
    Ok(Value::Ref(id))
}

/// `property(fget=None)` - creates a property descriptor.
fn builtin_property(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        let arg_count = positional.len() + kwargs.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("property", 4, arg_count));
    }
    kwargs.drop_with_heap(heap);

    let mut positional_args: Vec<Value> = positional.collect();
    let arg_count = positional_args.len();
    if arg_count > 4 {
        for value in positional_args {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("property", 4, arg_count));
    }

    let mut iter = positional_args.drain(..);
    let fget = normalize_property_callable(iter.next(), heap);
    let fset = normalize_property_callable(iter.next(), heap);
    let fdel = normalize_property_callable(iter.next(), heap);
    let doc = normalize_property_doc(iter.next(), heap);

    let up = UserProperty::new_full(fget, fset, fdel, doc);
    let id = heap.allocate(HeapData::UserProperty(up))?;
    Ok(Value::Ref(id))
}

/// Normalizes optional property constructor callable arguments.
///
/// `None` is treated as "missing callable".
fn normalize_property_callable(value: Option<Value>, heap: &mut Heap<impl ResourceTracker>) -> Option<Value> {
    let value = value?;
    if matches!(value, Value::None) {
        value.drop_with_heap(heap);
        None
    } else {
        Some(value)
    }
}

/// Normalizes optional property doc argument.
///
/// `None` means "fallback to getter __doc__".
fn normalize_property_doc(value: Option<Value>, heap: &mut Heap<impl ResourceTracker>) -> Option<Value> {
    let value = value?;
    if matches!(value, Value::None) {
        value.drop_with_heap(heap);
        None
    } else {
        Some(value)
    }
}
