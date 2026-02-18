use std::{
    borrow::Cow,
    fmt::{self, Display, Write},
};

use serde::{Deserialize, Serialize};
use smallvec::smallvec;
use strum::{Display, EnumString, IntoStaticStr};

use crate::{
    args::ArgValues,
    exception_public::{Exception, StackFrame},
    fstring::FormatError,
    heap::{DropWithHeap, Heap, HeapData, HeapGuard},
    intern::{Interns, StaticStrings, StringId},
    parse::CodeRange,
    resource::ResourceTracker,
    types::{
        AttrCallResult, PyTrait, Str, Type, allocate_tuple,
        str::{StringRepr, string_repr_fmt},
    },
    value::Value,
};

/// Result type alias for operations that can produce a runtime error.
pub type RunResult<T> = Result<T, RunError>;

/// Python exception types supported by the interpreter.
///
/// Uses strum derives for automatic `Display`, `FromStr`, and `Into<&'static str>` implementations.
/// The string representation matches the variant name exactly (e.g., `ValueError` -> "ValueError").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, IntoStaticStr, Serialize, Deserialize)]
pub enum ExcType {
    /// primary exception class - matches any exception in isinstance checks.
    Exception,
    /// Grouped exceptions raised and handled via `except*` semantics.
    ExceptionGroup,

    /// System exit exceptions
    BaseException,
    SystemExit,
    KeyboardInterrupt,
    /// Exception raised when a generator's close() method is called.
    /// Inherits from BaseException, not Exception.
    GeneratorExit,

    // --- ArithmeticError hierarchy ---
    /// Intermediate class for arithmetic errors.
    ArithmeticError,
    /// Subclass of ArithmeticError.
    FloatingPointError,
    /// Subclass of ArithmeticError.
    OverflowError,
    /// Subclass of ArithmeticError.
    ZeroDivisionError,

    // --- LookupError hierarchy ---
    /// Intermediate class for lookup errors.
    LookupError,
    /// Subclass of LookupError.
    IndexError,
    /// Subclass of LookupError.
    KeyError,

    // --- RuntimeError hierarchy ---
    /// Intermediate class for runtime errors.
    RuntimeError,
    /// Subclass of RuntimeError.
    NotImplementedError,
    /// Subclass of RuntimeError.
    RecursionError,

    // --- AttributeError hierarchy ---
    AttributeError,
    /// Subclass of AttributeError (from dataclasses module).
    FrozenInstanceError,

    // --- NameError hierarchy ---
    NameError,
    /// Subclass of NameError - for accessing local variable before assignment.
    UnboundLocalError,

    // --- ValueError hierarchy ---
    ValueError,
    /// Subclass of ValueError - for encoding/decoding errors.
    UnicodeDecodeError,
    /// Subclass of ValueError used by `json` decoding failures.
    #[strum(serialize = "JSONDecodeError")]
    JSONDecodeError,
    /// Subclass of ValueError used by `tomllib` decoding failures.
    #[strum(serialize = "TOMLDecodeError")]
    TOMLDecodeError,

    // --- ImportError hierarchy ---
    /// Import-related errors (module not found, name not in module).
    ImportError,
    /// Subclass of ImportError - for when a module cannot be found.
    ModuleNotFoundError,

    // --- OSError hierarchy ---
    /// OS-related errors (file not found, permission denied, etc.)
    OSError,
    /// Subclass of OSError - for when a file or directory cannot be found.
    FileNotFoundError,
    /// Subclass of OSError - for when a file already exists.
    FileExistsError,
    /// Subclass of OSError - for when a path is a directory but a file was expected.
    IsADirectoryError,
    /// Subclass of OSError - for when a path is not a directory but one was expected.
    NotADirectoryError,
    /// Subclass of OSError - for when an operation lacks required permissions.
    PermissionError,

    // --- Standalone exception types ---
    AssertionError,
    BufferError,
    EOFError,
    MemoryError,
    ReferenceError,
    StopAsyncIteration,
    StopIteration,
    /// Base class for parser/compiler syntax failures.
    SyntaxError,
    /// Subclass of SyntaxError for invalid block indentation.
    IndentationError,
    TimeoutError,
    TypeError,
}

impl ExcType {
    /// Checks if this exception type is a subclass of another exception type.
    ///
    /// Implements Python's exception hierarchy for try/except matching:
    /// - `Exception` is the base class for all standard exceptions
    /// - `LookupError` is the base for `KeyError` and `IndexError`
    /// - `ArithmeticError` is the base for `FloatingPointError`, `ZeroDivisionError`, and `OverflowError`
    /// - `RuntimeError` is the base for `RecursionError` and `NotImplementedError`
    /// - `SyntaxError` is the base for `IndentationError`
    ///
    /// Returns true if `self` would be caught by `except handler_type:`.
    #[must_use]
    pub fn is_subclass_of(self, handler_type: Self) -> bool {
        if self == handler_type {
            return true;
        }
        match handler_type {
            // BaseException catches all exceptions
            Self::BaseException => true,
            // Exception catches everything except BaseException, and direct subclasses: KeyboardInterrupt, SystemExit, GeneratorExit
            Self::Exception => !matches!(
                self,
                Self::BaseException | Self::KeyboardInterrupt | Self::SystemExit | Self::GeneratorExit
            ),
            // LookupError catches KeyError and IndexError
            Self::LookupError => matches!(self, Self::KeyError | Self::IndexError),
            // ArithmeticError catches ZeroDivisionError and OverflowError
            Self::ArithmeticError => {
                matches!(
                    self,
                    Self::FloatingPointError | Self::ZeroDivisionError | Self::OverflowError
                )
            }
            // RuntimeError catches RecursionError and NotImplementedError
            Self::RuntimeError => matches!(self, Self::RecursionError | Self::NotImplementedError),
            // AttributeError catches FrozenInstanceError
            Self::AttributeError => matches!(self, Self::FrozenInstanceError),
            // NameError catches UnboundLocalError
            Self::NameError => matches!(self, Self::UnboundLocalError),
            // ValueError catches UnicodeDecodeError, JSONDecodeError, and TOMLDecodeError
            Self::ValueError => {
                matches!(
                    self,
                    Self::UnicodeDecodeError | Self::JSONDecodeError | Self::TOMLDecodeError
                )
            }
            // ImportError catches ModuleNotFoundError
            Self::ImportError => matches!(self, Self::ModuleNotFoundError),
            // OSError catches FileNotFoundError, FileExistsError, IsADirectoryError, NotADirectoryError, PermissionError
            Self::OSError => matches!(
                self,
                Self::FileNotFoundError
                    | Self::FileExistsError
                    | Self::IsADirectoryError
                    | Self::NotADirectoryError
                    | Self::PermissionError
            ),
            // SyntaxError catches IndentationError
            Self::SyntaxError => matches!(self, Self::IndentationError),
            // All other types only match exactly (handled by self == handler_type above)
            _ => false,
        }
    }

    /// Creates an exception instance from an exception type and arguments.
    ///
    /// Handles exception constructors like `ValueError('message')`.
    /// Currently supports zero or one string argument.
    ///
    /// The `interns` parameter provides access to interned string content.
    /// Returns a heap-allocated exception value.
    pub(crate) fn call(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        let exc = if self == Self::ExceptionGroup {
            Self::call_exception_group(args, heap, interns)?
        } else {
            SimpleException::from_args(self, args, heap, interns)?
        };
        let heap_id = heap.allocate(HeapData::Exception(exc))?;
        Ok(Value::Ref(heap_id))
    }

    /// Builds an `ExceptionGroup(message, exceptions)` instance.
    ///
    /// The first argument must be a string message and the second argument must be
    /// a list or tuple of exception instances. Nested `ExceptionGroup` instances are
    /// accepted because they are regular exception instances at runtime.
    fn call_exception_group(
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<SimpleException> {
        let (message_value, exceptions_value) = match args {
            ArgValues::Two(message_value, exceptions_value) => (message_value, exceptions_value),
            other => {
                other.drop_with_heap(heap);
                return Err(Self::type_error("ExceptionGroup() takes exactly 2 arguments"));
            }
        };

        let mut message_value_guard = HeapGuard::new(message_value, heap);
        let (message_value, heap) = message_value_guard.as_parts_mut();
        let mut exceptions_value_guard = HeapGuard::new(exceptions_value, heap);
        let (exceptions_value, heap) = exceptions_value_guard.as_parts_mut();

        let message = match message_value {
            Value::InternString(string_id) => interns.get_str(*string_id).to_owned(),
            Value::Ref(heap_id) => match heap.get(*heap_id) {
                HeapData::Str(s) => s.as_str().to_owned(),
                _ => return Err(Self::type_error("ExceptionGroup() argument 1 must be str")),
            },
            _ => return Err(Self::type_error("ExceptionGroup() argument 1 must be str")),
        };

        let exceptions = match exceptions_value {
            Value::Ref(heap_id) => match heap.get(*heap_id) {
                HeapData::List(list) => list.as_vec().as_slice(),
                HeapData::Tuple(tuple) => tuple.as_vec().as_slice(),
                _ => {
                    return Err(Self::type_error(
                        "ExceptionGroup() argument 2 must be a list or tuple of exceptions",
                    ));
                }
            },
            _ => {
                return Err(Self::type_error(
                    "ExceptionGroup() argument 2 must be a list or tuple of exceptions",
                ));
            }
        };

        let mut grouped: Vec<SimpleException> = Vec::with_capacity(exceptions.len());
        for exc in exceptions {
            match exc {
                Value::Ref(exc_id) => {
                    if let HeapData::Exception(simple_exc) = heap.get(*exc_id) {
                        grouped.push(simple_exc.clone());
                    } else {
                        return Err(Self::type_error("exceptions must derive from BaseException"));
                    }
                }
                _ => return Err(Self::type_error("exceptions must derive from BaseException")),
            }
        }

        Ok(SimpleException::with_exceptions(
            Self::ExceptionGroup,
            Some(message),
            &grouped,
        ))
    }

    /// Creates an AttributeError for when an attribute is not found (GET operation).
    ///
    /// Sets `hide_caret: true` because CPython doesn't show carets for attribute GET errors.
    #[must_use]
    pub(crate) fn attribute_error(type_name: impl Display, attr: &str) -> RunError {
        let exc = SimpleException::new_msg(
            Self::AttributeError,
            format!("'{type_name}' object has no attribute '{attr}'"),
        );
        RunError::Exc(Box::new(ExceptionRaise {
            exc,
            frame: None,
            hide_caret: true, // CPython doesn't show carets for attribute GET errors
            original_value: None,
        }))
    }

    /// Creates an AttributeError for a dataclass method that requires external call integration.
    ///
    /// This is a temporary error used when dataclass methods are called but the external
    /// call mechanism hasn't been integrated yet.
    #[must_use]
    pub(crate) fn attribute_error_method_not_implemented(class_name: &str, method_name: &str) -> RunError {
        SimpleException::new_msg(
            Self::AttributeError,
            format!("'{class_name}' object method '{method_name}' requires external call (not yet implemented)"),
        )
        .into()
    }

    /// Creates an AttributeError for attribute assignment on types that don't support it.
    ///
    /// Matches CPython's format for setting attributes on built-in types.
    #[must_use]
    pub(crate) fn attribute_error_no_setattr(type_: Type, attr_name: &str) -> RunError {
        SimpleException::new_msg(
            Self::AttributeError,
            format!("'{type_}' object has no attribute '{attr_name}' and no __dict__ for setting new attributes"),
        )
        .into()
    }

    /// Creates an AttributeError for attribute assignment/deletion on instances without `__dict__`.
    ///
    /// Matches CPython's format for slotted instances with no dict.
    #[must_use]
    pub(crate) fn attribute_error_no_dict_for_setting(class_name: &str, attr_name: &str) -> RunError {
        SimpleException::new_msg(
            Self::AttributeError,
            format!("'{class_name}' object has no attribute '{attr_name}' and no __dict__ for setting new attributes"),
        )
        .into()
    }

    /// Creates an AttributeError for attempts to write to `__weakref__`.
    ///
    /// Matches CPython's format: "attribute '__weakref__' of 'C' objects is not writable".
    #[must_use]
    pub(crate) fn attribute_error_weakref_not_writable(class_name: &str) -> RunError {
        SimpleException::new_msg(
            Self::AttributeError,
            format!("attribute '__weakref__' of '{class_name}' objects is not writable"),
        )
        .into()
    }

    /// Creates an AttributeError for a missing module attribute.
    ///
    /// Matches CPython's format: `AttributeError: module 'name' has no attribute 'attr'`
    /// Sets `hide_caret: true` because CPython doesn't show carets for attribute GET errors.
    #[must_use]
    pub(crate) fn attribute_error_module(module_name: &str, attr_name: &str) -> RunError {
        let exc = SimpleException::new_msg(
            Self::AttributeError,
            format!("module '{module_name}' has no attribute '{attr_name}'"),
        );
        RunError::Exc(Box::new(ExceptionRaise {
            exc,
            frame: None,
            hide_caret: true, // CPython doesn't show carets for attribute GET errors
            original_value: None,
        }))
    }

    /// Creates a FrozenInstanceError for assigning to a frozen dataclass.
    ///
    /// Matches CPython's `dataclasses.FrozenInstanceError` which is a subclass of `AttributeError`.
    /// Message format: "cannot assign to field 'attr_name'"
    #[must_use]
    pub(crate) fn frozen_instance_error(attr_name: &str) -> RunError {
        SimpleException::new_msg(
            Self::FrozenInstanceError,
            format!("cannot assign to field '{attr_name}'"),
        )
        .into()
    }

    #[must_use]
    pub(crate) fn type_error_not_sub(type_: Type) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("'{type_}' object is not subscriptable")).into()
    }

    /// Creates a TypeError for awaiting a non-awaitable object.
    ///
    /// Matches CPython's format: `TypeError: '{type}' object can't be awaited`
    #[must_use]
    pub(crate) fn object_not_awaitable(type_: Type) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("'{type_}' object can't be awaited")).into()
    }

    /// Creates a TypeError for item assignment on types that don't support it.
    ///
    /// Matches CPython's format: `TypeError: '{type}' object does not support item assignment`
    #[must_use]
    pub(crate) fn type_error_not_sub_assignment(type_: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("'{type_}' object does not support item assignment"),
        )
        .into()
    }

    /// Creates a TypeError for unhashable types when calling `hash()`.
    ///
    /// This matches Python 3.14's error message: `TypeError: unhashable type: 'list'`
    #[must_use]
    pub(crate) fn type_error_unhashable(type_: Type) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("unhashable type: '{type_}'")).into()
    }

    /// Creates a TypeError for unhashable types used as dict keys.
    ///
    /// This matches Python 3.14's error message:
    /// `TypeError: cannot use 'list' as a dict key (unhashable type: 'list')`
    #[must_use]
    pub(crate) fn type_error_unhashable_dict_key(type_: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("cannot use '{type_}' as a dict key (unhashable type: '{type_}')"),
        )
        .into()
    }

    /// Creates a TypeError for unhashable types used as set elements.
    ///
    /// This matches Python 3.14's error message:
    /// `TypeError: cannot use 'list' as a set element (unhashable type: 'list')`
    #[must_use]
    pub(crate) fn type_error_unhashable_set_element(type_: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("cannot use '{type_}' as a set element (unhashable type: '{type_}')"),
        )
        .into()
    }

    /// Creates a KeyError for a missing dict key.
    ///
    /// For string keys, uses the raw string value without extra quoting.
    #[must_use]
    pub(crate) fn key_error(key: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunError {
        let key_str = key.py_str(heap, interns).into_owned();
        SimpleException::new_msg(Self::KeyError, key_str).into()
    }

    /// Creates a KeyError for popping from an empty set.
    ///
    /// Matches CPython's error format: `KeyError: 'pop from an empty set'`
    #[must_use]
    pub(crate) fn key_error_pop_empty_set() -> RunError {
        SimpleException::new_msg(Self::KeyError, "pop from an empty set").into()
    }

    /// Creates a TypeError for when a function receives the wrong number of arguments.
    ///
    /// Matches CPython's error format exactly:
    /// - For 1 expected arg: `{name}() takes exactly one argument ({actual} given)`
    /// - For N expected args: `{name} expected {expected} arguments, got {actual}`
    ///
    /// # Arguments
    /// * `name` - The function name (e.g., "len" for builtins, "list.append" for methods)
    /// * `expected` - Number of expected arguments
    /// * `actual` - Number of arguments actually provided
    #[must_use]
    pub(crate) fn type_error_arg_count(name: &str, expected: usize, actual: usize) -> RunError {
        if expected == 1 {
            // CPython: "len() takes exactly one argument (2 given)"
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() takes exactly one argument ({actual} given)"),
            )
            .into()
        } else {
            // CPython: "insert expected 2 arguments, got 1"
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name} expected {expected} arguments, got {actual}"),
            )
            .into()
        }
    }

    /// Creates a TypeError for when a method that takes no arguments receives some.
    ///
    /// Matches CPython's format: `{name}() takes no arguments ({actual} given)`
    ///
    /// # Arguments
    /// * `name` - The method name (e.g., "dict.keys")
    /// * `actual` - Number of arguments actually provided
    #[must_use]
    pub(crate) fn type_error_no_args(name: &str, actual: usize) -> RunError {
        // CPython: "dict.keys() takes no arguments (1 given)"
        SimpleException::new_msg(Self::TypeError, format!("{name}() takes no arguments ({actual} given)")).into()
    }

    /// Creates a TypeError for when a function receives fewer arguments than required.
    ///
    /// Matches CPython's format: `{name} expected at least {min} argument, got {actual}`
    ///
    /// # Arguments
    /// * `name` - The function name (e.g., "get", "pop")
    /// * `min` - Minimum number of required arguments
    /// * `actual` - Number of arguments actually provided
    #[must_use]
    pub(crate) fn type_error_at_least(name: &str, min: usize, actual: usize) -> RunError {
        // CPython: "get expected at least 1 argument, got 0"
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name} expected at least {min} argument, got {actual}"),
        )
        .into()
    }

    /// Creates a TypeError for when a function receives more arguments than allowed.
    ///
    /// Matches CPython's format: `{name} expected at most {max} arguments, got {actual}`
    ///
    /// # Arguments
    /// * `name` - The function name (e.g., "get", "pop")
    /// * `max` - Maximum number of allowed arguments
    /// * `actual` - Number of arguments actually provided
    #[must_use]
    pub(crate) fn type_error_at_most(name: &str, max: usize, actual: usize) -> RunError {
        // CPython: "get expected at most 2 arguments, got 3"
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name} expected at most {max} arguments, got {actual}"),
        )
        .into()
    }

    /// Creates a TypeError for missing positional arguments.
    ///
    /// Matches CPython's format: `{name}() missing {count} required positional argument(s): 'a' and 'b'`
    #[must_use]
    pub(crate) fn type_error_missing_positional_with_names(name: &str, missing_names: &[&str]) -> RunError {
        let count = missing_names.len();
        let names_str = format_param_names(missing_names);
        if count == 1 {
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() missing 1 required positional argument: {names_str}"),
            )
            .into()
        } else {
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() missing {count} required positional arguments: {names_str}"),
            )
            .into()
        }
    }

    /// Creates a TypeError for missing keyword-only arguments.
    ///
    /// Matches CPython's format: `{name}() missing {count} required keyword-only argument(s): 'a' and 'b'`
    #[must_use]
    pub(crate) fn type_error_missing_kwonly_with_names(name: &str, missing_names: &[&str]) -> RunError {
        let count = missing_names.len();
        let names_str = format_param_names(missing_names);
        if count == 1 {
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() missing 1 required keyword-only argument: {names_str}"),
            )
            .into()
        } else {
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() missing {count} required keyword-only arguments: {names_str}"),
            )
            .into()
        }
    }

    /// Creates a TypeError for too many positional arguments.
    ///
    /// Matches CPython's format:
    /// - Simple: `{name}() takes {max} positional argument(s) but {actual} were given`
    /// - With kwonly: `{name}() takes {max} positional argument(s) but {actual} positional argument(s) (and N keyword-only argument(s)) were given`
    #[must_use]
    pub(crate) fn type_error_too_many_positional(
        name: &str,
        max: usize,
        actual: usize,
        kwonly_given: usize,
    ) -> RunError {
        let takes_word = if max == 1 { "argument" } else { "arguments" };

        if kwonly_given > 0 {
            // CPython includes keyword-only args in the "given" part when present
            let given_word = if actual == 1 { "argument" } else { "arguments" };
            let kwonly_word = if kwonly_given == 1 { "argument" } else { "arguments" };
            SimpleException::new_msg(
                Self::TypeError,
                format!(
                    "{name}() takes {max} positional {takes_word} but {actual} positional {given_word} (and {kwonly_given} keyword-only {kwonly_word}) were given"
                ),
            )
            .into()
        } else if max == 0 {
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() takes 0 positional arguments but {actual} were given"),
            )
            .into()
        } else {
            SimpleException::new_msg(
                Self::TypeError,
                format!("{name}() takes {max} positional {takes_word} but {actual} were given"),
            )
            .into()
        }
    }

    /// Creates a TypeError for positional-only parameter passed as keyword.
    ///
    /// Matches CPython's format: `{name}() got some positional-only arguments passed as keyword arguments: '{param}'`
    #[must_use]
    pub(crate) fn type_error_positional_only(name: &str, param: &str) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name}() got some positional-only arguments passed as keyword arguments: '{param}'"),
        )
        .into()
    }

    /// Creates a TypeError for duplicate argument.
    ///
    /// Matches CPython's format: `{name}() got multiple values for argument '{param}'`
    #[must_use]
    pub(crate) fn type_error_duplicate_arg(name: &str, param: &str) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name}() got multiple values for argument '{param}'"),
        )
        .into()
    }

    /// Creates a TypeError for duplicate keyword argument.
    ///
    /// Matches CPython's format: `{name}() got multiple values for keyword argument '{key}'`
    #[must_use]
    pub(crate) fn type_error_multiple_values(name: &str, key: &str) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name}() got multiple values for keyword argument '{key}'"),
        )
        .into()
    }

    /// Creates a TypeError for unexpected keyword argument.
    ///
    /// Matches CPython's format: `{name}() got an unexpected keyword argument '{key}'`
    #[must_use]
    pub(crate) fn type_error_unexpected_keyword(name: &str, key: &str) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name}() got an unexpected keyword argument '{key}'"),
        )
        .into()
    }

    /// Creates a TypeError for **kwargs argument that is not a mapping.
    ///
    /// Matches CPython's format: `{name}() argument after ** must be a mapping, not {type_name}`
    #[must_use]
    pub(crate) fn type_error_kwargs_not_mapping(name: &str, type_name: &str) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("{name}() argument after ** must be a mapping, not {type_name}"),
        )
        .into()
    }

    /// Creates a TypeError for **kwargs with non-string keys.
    ///
    /// Matches CPython's format: `{name}() keywords must be strings`
    #[must_use]
    pub(crate) fn type_error_kwargs_nonstring_key() -> RunError {
        SimpleException::new_msg(Self::TypeError, "keywords must be strings").into()
    }

    /// Creates a TypeError for invalid decimal rounding values.
    ///
    /// Matches CPython's `decimal` error message when an unsupported rounding
    /// name is passed to APIs like `Decimal.quantize(...)`.
    #[must_use]
    pub(crate) fn type_error_decimal_invalid_rounding() -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            "valid values for rounding are:\n  [ROUND_CEILING, ROUND_FLOOR, ROUND_UP, ROUND_DOWN,\n   ROUND_HALF_UP, ROUND_HALF_DOWN, ROUND_HALF_EVEN,\n   ROUND_05UP]",
        )
        .into()
    }

    /// Creates a simple TypeError with a custom message.
    #[must_use]
    pub(crate) fn type_error(msg: impl fmt::Display) -> RunError {
        SimpleException::new_msg(Self::TypeError, msg).into()
    }

    /// Creates a TypeError for bytes() constructor with invalid type.
    ///
    /// Matches CPython's format: `TypeError: cannot convert '{type}' object to bytes`
    #[must_use]
    pub(crate) fn type_error_bytes_init(type_: Type) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("cannot convert '{type_}' object to bytes")).into()
    }

    /// Creates a TypeError for calling a non-callable type.
    ///
    /// Matches CPython's format: `TypeError: cannot create '{type}' instances`
    #[must_use]
    pub(crate) fn type_error_not_callable(type_: Type) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("cannot create '{type_}' instances")).into()
    }

    /// Creates a TypeError for non-iterable type in list/tuple/etc constructors.
    ///
    /// Matches CPython's format: `TypeError: '{type}' object is not iterable`
    #[must_use]
    pub(crate) fn type_error_not_iterable(type_: Type) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("'{type_}' object is not iterable")).into()
    }

    /// Creates a TypeError for int() constructor with invalid type.
    ///
    /// Matches CPython's format: `TypeError: int() argument must be a string, a bytes-like object or a real number, not '{type}'`
    #[must_use]
    pub(crate) fn type_error_int_conversion(type_: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("int() argument must be a string, a bytes-like object or a real number, not '{type_}'"),
        )
        .into()
    }

    /// Creates a TypeError for float() constructor with invalid type.
    ///
    /// Matches CPython's format: `TypeError: float() argument must be a string or a real number, not '{type}'`
    #[must_use]
    pub(crate) fn type_error_float_conversion(type_: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("float() argument must be a string or a real number, not '{type_}'"),
        )
        .into()
    }

    /// Creates a ValueError for negative count in bytes().
    ///
    /// Matches CPython's format: `ValueError: negative count`
    #[must_use]
    pub(crate) fn value_error_negative_bytes_count() -> RunError {
        SimpleException::new_msg(Self::ValueError, "negative count").into()
    }

    /// Creates the ValueError raised by `int(Decimal('NaN'))`.
    ///
    /// Matches CPython's format: `ValueError: cannot convert NaN to integer`
    #[must_use]
    pub(crate) fn value_error_cannot_convert_nan_to_integer() -> RunError {
        SimpleException::new_msg(Self::ValueError, "cannot convert NaN to integer").into()
    }

    /// Creates the ValueError raised by `float(Decimal('sNaN'))`.
    ///
    /// Matches CPython's format: `ValueError: cannot convert signaling NaN to float`
    #[must_use]
    pub(crate) fn value_error_cannot_convert_signaling_nan_to_float() -> RunError {
        SimpleException::new_msg(Self::ValueError, "cannot convert signaling NaN to float").into()
    }

    /// Creates the OverflowError raised by `int(Decimal('Infinity'))`.
    ///
    /// Matches CPython's format: `OverflowError: cannot convert Infinity to integer`
    #[must_use]
    pub(crate) fn overflow_error_cannot_convert_infinity_to_integer() -> RunError {
        SimpleException::new_msg(Self::OverflowError, "cannot convert Infinity to integer").into()
    }

    /// Creates a ValueError for APIs that reject negative integer sizes.
    ///
    /// Matches CPython's format: `ValueError: Cannot convert negative int`
    #[must_use]
    pub(crate) fn value_error_cannot_convert_negative_int() -> RunError {
        SimpleException::new_msg(Self::ValueError, "Cannot convert negative int").into()
    }

    /// Creates a TypeError for isinstance() arg 2.
    ///
    /// Matches CPython's format: `TypeError: isinstance() arg 2 must be a type, a tuple of types, or a union`
    #[must_use]
    pub(crate) fn isinstance_arg2_error() -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            "isinstance() arg 2 must be a type, a tuple of types, or a union",
        )
        .into()
    }

    /// Creates a TypeError for invalid exception type in except clause.
    ///
    /// Matches CPython's format: `TypeError: catching classes that do not inherit from BaseException is not allowed`
    #[must_use]
    pub(crate) fn except_invalid_type_error() -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            "catching classes that do not inherit from BaseException is not allowed",
        )
        .into()
    }

    /// Creates a ValueError for range() step argument being zero.
    ///
    /// Matches CPython's format: `ValueError: range() arg 3 must not be zero`
    #[must_use]
    pub(crate) fn value_error_range_step_zero() -> RunError {
        SimpleException::new_msg(Self::ValueError, "range() arg 3 must not be zero").into()
    }

    /// Creates a ValueError for slice step being zero.
    ///
    /// Matches CPython's format: `ValueError: slice step cannot be zero`
    #[must_use]
    pub(crate) fn value_error_slice_step_zero() -> RunError {
        SimpleException::new_msg(Self::ValueError, "slice step cannot be zero").into()
    }

    /// Creates a TypeError for slice indices that are not integers or None.
    ///
    /// Matches CPython's format: `TypeError: slice indices must be integers or None or have an __index__ method`
    #[must_use]
    pub(crate) fn type_error_slice_indices() -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            "slice indices must be integers or None or have an __index__ method",
        )
        .into()
    }

    /// Creates a RuntimeError for dict mutation during iteration.
    ///
    /// Matches CPython's format: `RuntimeError: dictionary changed size during iteration`
    #[must_use]
    pub(crate) fn runtime_error_dict_changed_size() -> RunError {
        SimpleException::new_msg(Self::RuntimeError, "dictionary changed size during iteration").into()
    }

    /// Creates a RuntimeError for set mutation during iteration.
    ///
    /// Matches CPython's format: `RuntimeError: Set changed size during iteration`
    #[must_use]
    pub(crate) fn runtime_error_set_changed_size() -> RunError {
        SimpleException::new_msg(Self::RuntimeError, "Set changed size during iteration").into()
    }

    /// Creates a TypeError for functions that don't accept keyword arguments.
    ///
    /// Matches CPython's format: `TypeError: {name}() takes no keyword arguments`
    #[must_use]
    pub(crate) fn type_error_no_kwargs(name: &str) -> RunError {
        SimpleException::new_msg(Self::TypeError, format!("{name}() takes no keyword arguments")).into()
    }

    /// Creates an IndexError for list index out of range (getitem).
    ///
    /// Matches CPython's format: `IndexError('list index out of range')`
    #[must_use]
    pub(crate) fn list_index_error() -> RunError {
        SimpleException::new_msg(Self::IndexError, "list index out of range").into()
    }

    /// Creates an IndexError for list assignment index out of range (setitem).
    ///
    /// Matches CPython's format: `IndexError('list assignment index out of range')`
    #[must_use]
    pub(crate) fn list_assignment_index_error() -> RunError {
        SimpleException::new_msg(Self::IndexError, "list assignment index out of range").into()
    }

    /// Creates an IndexError for tuple index out of range.
    ///
    /// Matches CPython's format: `IndexError('tuple index out of range')`
    #[must_use]
    pub(crate) fn tuple_index_error() -> RunError {
        SimpleException::new_msg(Self::IndexError, "tuple index out of range").into()
    }

    /// Creates an IndexError for string index out of range.
    ///
    /// Matches CPython's format: `IndexError('string index out of range')`
    #[must_use]
    pub(crate) fn str_index_error() -> RunError {
        SimpleException::new_msg(Self::IndexError, "string index out of range").into()
    }

    /// Creates an IndexError for bytes index out of range.
    ///
    /// Matches CPython's format: `IndexError('index out of range')`
    #[must_use]
    pub(crate) fn bytes_index_error() -> RunError {
        SimpleException::new_msg(Self::IndexError, "index out of range").into()
    }

    /// Creates an IndexError for range index out of range.
    ///
    /// Matches CPython's format: `IndexError('range object index out of range')`
    #[must_use]
    pub(crate) fn range_index_error() -> RunError {
        SimpleException::new_msg(Self::IndexError, "range object index out of range").into()
    }

    /// Creates a TypeError for non-integer sequence indices (getitem).
    ///
    /// Matches CPython's format: `TypeError('{type}' indices must be integers, not '{index_type}')`
    #[must_use]
    pub(crate) fn type_error_indices(type_str: Type, index_type: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("{type_str} indices must be integers, not '{index_type}'"),
        )
        .into()
    }

    /// Creates a TypeError for non-integer list indices (setitem/assignment).
    ///
    /// Matches CPython's format: `TypeError('list indices must be integers or slices, not {index_type}')`
    #[must_use]
    pub(crate) fn type_error_list_assignment_indices(index_type: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("list indices must be integers or slices, not {index_type}"),
        )
        .into()
    }

    /// Creates a NameError for accessing a free variable (nonlocal/closure) before it's assigned.
    ///
    /// Matches CPython's format: `NameError: cannot access free variable 'x' where it is not
    /// associated with a value in enclosing scope`
    #[must_use]
    pub(crate) fn name_error_free_variable(name: &str) -> SimpleException {
        SimpleException::new_msg(
            Self::NameError,
            format!("cannot access free variable '{name}' where it is not associated with a value in enclosing scope"),
        )
    }

    /// Creates a NameError for accessing an undefined variable.
    ///
    /// Matches CPython's format: `NameError: name 'x' is not defined`
    #[must_use]
    pub(crate) fn name_error(name: &str) -> SimpleException {
        let mut msg = format!("name '{name}' is not defined");
        // add the same suffix as cpython, but only for the modules supported by Ouros
        if matches!(name, "asyncio" | "sys" | "typing" | "types") {
            write!(&mut msg, ". Did you forget to import '{name}'?").unwrap();
        }
        SimpleException::new_msg(Self::NameError, msg)
    }

    /// Creates an UnboundLocalError for accessing a local variable before assignment.
    ///
    /// Matches CPython's format: `UnboundLocalError: cannot access local variable 'x' where it is not associated with a value`
    #[must_use]
    pub(crate) fn unbound_local_error(name: &str) -> SimpleException {
        SimpleException::new_msg(
            Self::UnboundLocalError,
            format!("cannot access local variable '{name}' where it is not associated with a value"),
        )
    }

    /// Creates a ModuleNotFoundError for when a module cannot be found.
    ///
    /// Matches CPython's format: `ModuleNotFoundError: No module named 'name'`
    /// Sets `hide_caret: true` because CPython doesn't show carets for module not found errors.
    #[must_use]
    pub(crate) fn module_not_found_error(module_name: &str) -> RunError {
        let exc = SimpleException::new_msg(Self::ModuleNotFoundError, format!("No module named '{module_name}'"));
        RunError::Exc(Box::new(ExceptionRaise {
            exc,
            frame: None,
            hide_caret: true, // CPython doesn't show carets for module not found errors
            original_value: None,
        }))
    }

    /// Creates a PermissionError when a capability check denies an operation.
    ///
    /// Used by the capability system at the yield boundary to deny external function
    /// calls or proxy operations that are not in the session's capability set.
    #[must_use]
    pub(crate) fn permission_error(msg: impl fmt::Display) -> SimpleException {
        SimpleException::new_msg(Self::PermissionError, msg)
    }

    /// Creates a NotImplementedError for an unimplemented Python feature.
    ///
    /// Used during parsing when encountering Python syntax that Ouros doesn't yet support.
    /// The message format is: "The ouros syntax parser does not yet support {feature}"
    #[must_use]
    pub(crate) fn not_implemented(msg: impl fmt::Display) -> SimpleException {
        SimpleException::new_msg(Self::NotImplementedError, msg)
    }

    /// Creates a ZeroDivisionError for division by zero.
    ///
    /// Matches CPython 3.14's format: `ZeroDivisionError('division by zero')`
    #[must_use]
    pub(crate) fn zero_division() -> SimpleException {
        SimpleException::new_msg(Self::ZeroDivisionError, "division by zero")
    }

    /// Creates an OverflowError for string/sequence repetition with count too large.
    ///
    /// Matches CPython's format: `OverflowError('cannot fit 'int' into an index-sized integer')`
    #[must_use]
    pub(crate) fn overflow_repeat_count() -> SimpleException {
        SimpleException::new_msg(Self::OverflowError, "cannot fit 'int' into an index-sized integer")
    }

    /// Creates an IndexError for when an integer index is too large to fit in i64.
    ///
    /// Matches CPython's format: `IndexError: cannot fit 'int' into an index-sized integer`
    #[must_use]
    pub(crate) fn index_error_int_too_large() -> RunError {
        SimpleException::new_msg(Self::IndexError, "cannot fit 'int' into an index-sized integer").into()
    }

    /// Creates an ImportError for when a name cannot be imported from a module.
    ///
    /// Matches CPython's format for built-in modules:
    /// `ImportError: cannot import name 'name' from 'module' (unknown location)`
    ///
    /// Sets `hide_caret: true` because CPython doesn't show carets for import errors.
    #[must_use]
    pub(crate) fn cannot_import_name(name: &str, module_name: &str) -> RunError {
        let exc = SimpleException::new_msg(
            Self::ImportError,
            format!("cannot import name '{name}' from '{module_name}' (unknown location)"),
        );
        RunError::Exc(Box::new(ExceptionRaise {
            exc,
            frame: None,
            hide_caret: true,
            original_value: None,
        }))
    }

    /// Creates a ValueError for negative shift count in bitwise shift operations.
    ///
    /// Matches CPython's format: `ValueError: negative shift count`
    #[must_use]
    pub(crate) fn value_error_negative_shift_count() -> RunError {
        SimpleException::new_msg(Self::ValueError, "negative shift count").into()
    }

    /// Creates an OverflowError for shift count exceeding integer size.
    ///
    /// Matches CPython's format: `OverflowError: Python int too large to convert to C ssize_t`
    /// Note: CPython uses this message because it tries to convert to ssize_t for the shift amount.
    #[must_use]
    pub(crate) fn overflow_shift_count() -> RunError {
        SimpleException::new_msg(Self::OverflowError, "Python int too large to convert to C ssize_t").into()
    }

    /// Creates a TypeError for unsupported binary operations.
    ///
    /// For `+` or `+=` with str/list on the left side, uses CPython's special format:
    /// `can only concatenate {type} (not "{other}") to {type}`
    ///
    /// For other cases, uses the generic format:
    /// `unsupported operand type(s) for {op}: '{left}' and '{right}'`
    #[must_use]
    pub(crate) fn binary_type_error(op: &str, lhs_type: Type, rhs_type: Type) -> RunError {
        let message = if (op == "+" || op == "+=") && (lhs_type == Type::Str || lhs_type == Type::List) {
            format!("can only concatenate {lhs_type} (not \"{rhs_type}\") to {lhs_type}")
        } else {
            format!("unsupported operand type(s) for {op}: '{lhs_type}' and '{rhs_type}'")
        };
        SimpleException::new_msg(Self::TypeError, message).into()
    }

    /// Creates a TypeError for unsupported unary operations.
    ///
    /// Uses CPython's format: `bad operand type for unary {op}: '{type}'`
    #[must_use]
    pub(crate) fn unary_type_error(op: &str, value_type: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("bad operand type for unary {op}: '{value_type}'"),
        )
        .into()
    }

    /// Creates a TypeError for functions that require an integer argument.
    ///
    /// Matches CPython's format: `TypeError: '{type}' object cannot be interpreted as an integer`
    #[must_use]
    pub(crate) fn type_error_not_integer(type_: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("'{type_}' object cannot be interpreted as an integer"),
        )
        .into()
    }

    /// Creates a ZeroDivisionError for zero raised to a negative power.
    ///
    /// Matches CPython's format: `ZeroDivisionError: zero to a negative power`
    /// Note: CPython uses the same message for both int and float zero ** negative.
    #[must_use]
    pub(crate) fn zero_negative_power() -> RunError {
        SimpleException::new_msg(Self::ZeroDivisionError, "zero to a negative power").into()
    }

    /// Creates a ZeroDivisionError for zero raised to a negative or complex power.
    ///
    /// Matches CPython's format for complex exponentiation:
    /// `ZeroDivisionError: zero to a negative or complex power`.
    #[must_use]
    pub(crate) fn zero_negative_or_complex_power() -> RunError {
        SimpleException::new_msg(Self::ZeroDivisionError, "zero to a negative or complex power").into()
    }

    /// Creates an OverflowError for exponents that are too large.
    ///
    /// Matches CPython's format: `OverflowError: exponent too large`
    #[must_use]
    pub(crate) fn overflow_exponent_too_large() -> RunError {
        SimpleException::new_msg(Self::OverflowError, "exponent too large").into()
    }

    /// Creates a ZeroDivisionError for divmod by zero (both integer and float).
    ///
    /// Matches CPython's format: `ZeroDivisionError: division by zero`
    /// Note: CPython uses the same message for both integer and float divmod.
    #[must_use]
    pub(crate) fn divmod_by_zero() -> RunError {
        SimpleException::new_msg(Self::ZeroDivisionError, "division by zero").into()
    }

    /// Creates a TypeError for str.join() when an item is not a string.
    ///
    /// Matches CPython's format: `TypeError: sequence item {index}: expected str instance, {type} found`
    #[must_use]
    pub(crate) fn type_error_join_item(index: usize, item_type: Type) -> RunError {
        SimpleException::new_msg(
            Self::TypeError,
            format!("sequence item {index}: expected str instance, {item_type} found"),
        )
        .into()
    }

    /// Creates a TypeError for str.join() when the argument is not iterable.
    ///
    /// Matches CPython's format: `TypeError: can only join an iterable`
    #[must_use]
    pub(crate) fn type_error_join_not_iterable() -> RunError {
        SimpleException::new_msg(Self::TypeError, "can only join an iterable").into()
    }

    /// Creates a ValueError for str.index()/str.rindex() when substring is not found.
    ///
    /// Matches CPython's format: `ValueError: substring not found`
    #[must_use]
    pub(crate) fn value_error_substring_not_found() -> RunError {
        SimpleException::new_msg(Self::ValueError, "substring not found").into()
    }

    /// Creates a ValueError for str.partition()/str.rpartition() with empty separator.
    ///
    /// Matches CPython's format: `ValueError: empty separator`
    #[must_use]
    pub(crate) fn value_error_empty_separator() -> RunError {
        SimpleException::new_msg(Self::ValueError, "empty separator").into()
    }

    /// Creates a TypeError for fillchar argument that is not a single character.
    ///
    /// Matches CPython's format: `TypeError: The fill character must be exactly one character long`
    #[must_use]
    pub(crate) fn type_error_fillchar_must_be_single_char() -> RunError {
        SimpleException::new_msg(Self::TypeError, "The fill character must be exactly one character long").into()
    }

    /// Creates a StopIteration exception for when an iterator is exhausted.
    ///
    /// Matches CPython's format: `StopIteration`
    #[must_use]
    pub(crate) fn stop_iteration() -> RunError {
        SimpleException::new_none(Self::StopIteration).into()
    }

    /// Creates a StopIteration exception carrying a generator return value.
    ///
    /// `display_value` is used for `str(exc)` and `exc.args`. `encoded_value`
    /// is an internal typed payload used to reconstruct `exc.value`.
    #[must_use]
    pub(crate) fn stop_iteration_with_value(display_value: String, encoded_value: String) -> RunError {
        SimpleException::with_value(Self::StopIteration, Some(display_value), Some(encoded_value)).into()
    }

    /// Creates a ValueError for list.index() when item is not found.
    ///
    /// Matches CPython's format: `ValueError: list.index(x): x not in list`
    #[must_use]
    pub(crate) fn value_error_not_in_list() -> RunError {
        SimpleException::new_msg(Self::ValueError, "list.index(x): x not in list").into()
    }

    /// Creates a ValueError for tuple.index() when item is not found.
    ///
    /// Matches CPython's format: `ValueError: tuple.index(x): x not in tuple`
    #[must_use]
    pub(crate) fn value_error_not_in_tuple() -> RunError {
        SimpleException::new_msg(Self::ValueError, "tuple.index(x): x not in tuple").into()
    }

    /// Creates a ValueError for list.remove() when item is not found.
    ///
    /// Matches CPython's format: `ValueError: list.remove(x): x not in list`
    #[must_use]
    pub(crate) fn value_error_remove_not_in_list() -> RunError {
        SimpleException::new_msg(Self::ValueError, "list.remove(x): x not in list").into()
    }

    /// Creates an IndexError for popping from an empty list.
    ///
    /// Matches CPython's format: `IndexError: pop from empty list`
    #[must_use]
    pub(crate) fn index_error_pop_empty_list() -> RunError {
        SimpleException::new_msg(Self::IndexError, "pop from empty list").into()
    }

    /// Creates an IndexError for list.pop(index) with invalid index.
    ///
    /// Matches CPython's format: `IndexError: pop index out of range`
    #[must_use]
    pub(crate) fn index_error_pop_out_of_range() -> RunError {
        SimpleException::new_msg(Self::IndexError, "pop index out of range").into()
    }

    /// Creates a KeyError for popping from an empty dict.
    ///
    /// Matches CPython's format: `KeyError: 'popitem(): dictionary is empty'`
    #[must_use]
    pub(crate) fn key_error_popitem_empty_dict() -> RunError {
        SimpleException::new_msg(Self::KeyError, "popitem(): dictionary is empty").into()
    }

    /// Creates a LookupError for unknown encoding.
    ///
    /// Matches CPython's format: `LookupError: unknown encoding: {encoding}`
    #[must_use]
    pub(crate) fn lookup_error_unknown_encoding(encoding: &str) -> RunError {
        SimpleException::new_msg(Self::LookupError, format!("unknown encoding: {encoding}")).into()
    }

    /// Creates a UnicodeDecodeError for invalid UTF-8 bytes in decode().
    ///
    /// Matches CPython's format: `UnicodeDecodeError: 'utf-8' codec can't decode bytes...`
    #[must_use]
    pub(crate) fn unicode_decode_error_invalid_utf8() -> RunError {
        SimpleException::new_msg(
            Self::UnicodeDecodeError,
            "'utf-8' codec can't decode bytes: invalid utf-8 sequence",
        )
        .into()
    }

    /// Creates a UnicodeDecodeError for invalid ASCII bytes in decode().
    ///
    /// Matches CPython's format: `UnicodeDecodeError: 'ascii' codec can't decode bytes...`
    #[must_use]
    pub(crate) fn unicode_decode_error_invalid_ascii() -> RunError {
        SimpleException::new_msg(
            Self::UnicodeDecodeError,
            "'ascii' codec can't decode byte: ordinal not in range(128)",
        )
        .into()
    }

    /// Creates a ValueError for subsequence not found in bytes/str.
    ///
    /// Matches CPython's format: `ValueError: subsection not found`
    #[must_use]
    pub(crate) fn value_error_subsequence_not_found() -> RunError {
        SimpleException::new_msg(Self::ValueError, "subsection not found").into()
    }

    /// Creates a LookupError for unknown error handler.
    ///
    /// Matches CPython's format: `LookupError: unknown error handler name '{name}'`
    #[must_use]
    pub(crate) fn lookup_error_unknown_error_handler(name: &str) -> RunError {
        SimpleException::new_msg(Self::LookupError, format!("unknown error handler name '{name}'")).into()
    }

    /// Creates a ValueError for attempting to re-enter a running generator.
    ///
    /// Matches CPython's format: `ValueError: generator already executing`
    #[must_use]
    pub(crate) fn generator_already_executing() -> RunError {
        SimpleException::new_msg(Self::ValueError, "generator already executing").into()
    }

    /// Creates a TypeError for sending a non-None value to a just-started generator.
    ///
    /// Matches CPython's format: `TypeError: can't send non-None value to a just-started generator`
    #[must_use]
    pub(crate) fn generator_send_not_started() -> RunError {
        SimpleException::new_msg(Self::TypeError, "can't send non-None value to a just-started generator").into()
    }

    /// Creates a RuntimeError when a generator ignores GeneratorExit.
    ///
    /// Matches CPython's format: `RuntimeError: generator ignored GeneratorExit`
    #[must_use]
    pub(crate) fn generator_ignored_exit() -> RunError {
        SimpleException::new_msg(Self::RuntimeError, "generator ignored GeneratorExit").into()
    }

    /// Creates a RuntimeError when a generator raises StopIteration.
    ///
    /// Matches CPython's format: `RuntimeError: generator raised StopIteration`
    #[must_use]
    pub(crate) fn generator_raised_stop_iteration() -> RunError {
        SimpleException::new_msg(Self::RuntimeError, "generator raised StopIteration").into()
    }
}

/// Simple lightweight representation of an exception.
///
/// This is used for performance reasons for common exception patterns.
/// Exception messages use `String` for owned storage.
#[derive(Debug, Clone, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct SimpleException {
    exc_type: ExcType,
    arg: Option<String>,
    /// Optional encoded value payload associated with the exception.
    ///
    /// For `StopIteration`, this stores an internal typed encoding of the
    /// generator return value used to power `exc.value`.
    ///
    /// For `ExceptionGroup`, this stores a JSON-encoded list of child
    /// `SimpleException` values.
    ///
    /// For regex and JSON decode errors, this stores compact metadata payloads
    /// used to expose positional attributes like `.pos`, `.lineno`, and `.colno`.
    value: Option<String>,
    /// Serialized exception arguments for `exc.args`.
    args_serialized: Vec<u8>,
    /// Custom exception class name for user-defined exception instances.
    #[serde(default)]
    custom_class_name: Option<String>,
    /// User exception MRO class names (class first, then bases) for `except` matching.
    #[serde(default)]
    custom_mro_names: Vec<String>,
    /// Explicit chaining cause set by `raise X from Y`.
    #[serde(default)]
    cause: Option<Box<Self>>,
    /// Implicit chaining context set when raising during exception handling.
    #[serde(default)]
    context: Option<Box<Self>>,
    /// Whether implicit context should be suppressed in tracebacks.
    #[serde(default)]
    suppress_context: bool,
    /// Stringified custom attributes copied from user exception instances.
    #[serde(default)]
    custom_attrs: Vec<(String, String)>,
}

const RE_ERROR_META_PREFIX: &str = "re_error_meta:";
const JSON_DECODE_ERROR_META_PREFIX: &str = "json_decode_error_meta:";

impl fmt::Display for SimpleException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.py_repr_fmt(f)
    }
}
impl From<Exception> for SimpleException {
    fn from(exc: Exception) -> Self {
        Self {
            exc_type: exc.exc_type(),
            arg: exc.into_message(),
            value: None,
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }
}

impl SimpleException {
    /// Creates a new exception with the given type and optional argument message.
    #[must_use]
    pub fn new(exc_type: ExcType, arg: Option<String>) -> Self {
        Self {
            exc_type,
            arg,
            value: None,
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Creates a new exception with the given type and argument message.
    #[must_use]
    pub fn new_msg(exc_type: ExcType, arg: impl fmt::Display) -> Self {
        Self {
            exc_type,
            arg: Some(arg.to_string()),
            value: None,
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Creates a regex compilation/runtime exception with positional metadata.
    #[must_use]
    pub fn new_regex_error(arg: impl fmt::Display, pos: i64, lineno: i64, colno: i64) -> Self {
        Self {
            exc_type: ExcType::Exception,
            arg: Some(arg.to_string()),
            value: Some(format!("{RE_ERROR_META_PREFIX}{pos}:{lineno}:{colno}")),
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Creates a `json.JSONDecodeError` with positional metadata.
    #[must_use]
    pub fn new_json_decode_error(arg: impl fmt::Display, pos: i64, lineno: i64, colno: i64) -> Self {
        Self {
            exc_type: ExcType::JSONDecodeError,
            arg: Some(arg.to_string()),
            value: Some(format!("{JSON_DECODE_ERROR_META_PREFIX}{pos}:{lineno}:{colno}")),
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Creates a new exception with the given type and no argument message.
    #[must_use]
    pub fn new_none(exc_type: ExcType) -> Self {
        Self {
            exc_type,
            arg: None,
            value: None,
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Creates a new exception from the given type and arguments.
    ///
    /// Handles exception constructors like `ValueError('message')` or `ValueError(1, 2, 3)`.
    /// Accepts any number of arguments of any type.
    pub fn from_args(
        exc_type: ExcType,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let (pos_iter, kwargs) = args.into_parts();
        crate::defer_drop_mut!(pos_iter, heap);
        let kwargs = kwargs.into_iter();
        crate::defer_drop_mut!(kwargs, heap);
        let mut arg_values: Vec<Value> = Vec::new();
        let mut first_arg: Option<String> = None;

        // Collect all args and find first string-like arg for display.
        for (i, arg) in pos_iter.enumerate() {
            if i == 0 {
                first_arg = Self::value_to_string(&arg, heap, interns);
            }
            arg_values.push(arg);
        }

        // Ignore keyword args for compatibility with existing constructor behavior while
        // still ensuring reference-safe cleanup in ref-count panic mode.
        for (key, value) in kwargs {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
        }

        // Serialize all args for later retrieval via exc.args.
        let args_serialized = postcard::to_allocvec(&arg_values).unwrap_or_default();
        arg_values.drop_with_heap(heap);

        Ok(Self {
            exc_type,
            arg: first_arg,
            value: None,
            args_serialized,
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        })
    }

    /// Extracts a string from a Value if it's string-like.
    fn value_to_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<String> {
        match value {
            Value::InternString(s) => Some(interns.get_str(*s).to_string()),
            Value::Ref(heap_id) => {
                if let HeapData::Str(s) = heap.get(*heap_id) {
                    Some(s.as_str().to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Deserializes the args from the serialized bytes.
    fn deserialize_args(&self) -> Vec<Value> {
        if self.args_serialized.is_empty() {
            return Vec::new();
        }
        postcard::from_bytes(&self.args_serialized).unwrap_or_default()
    }

    /// Creates a new exception with the given type and value payload.
    ///
    /// Used by `StopIteration` to hold an encoded return value for `.value`.
    #[must_use]
    pub fn with_value(exc_type: ExcType, arg: Option<String>, value_str: Option<String>) -> Self {
        Self {
            exc_type,
            arg: arg.clone(),
            value: value_str,
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Creates an exception that carries grouped child exceptions.
    #[must_use]
    pub fn with_exceptions(exc_type: ExcType, arg: Option<String>, exceptions: &[Self]) -> Self {
        let encoded =
            serde_json::to_string(&exceptions).expect("serializing exception group children should never fail");
        Self {
            exc_type,
            arg,
            value: Some(encoded),
            args_serialized: Vec::new(),
            custom_class_name: None,
            custom_mro_names: Vec::new(),

            cause: None,

            context: None,

            suppress_context: false,

            custom_attrs: Vec::new(),
        }
    }

    /// Attaches user-defined exception metadata preserved across raise/except flow.
    #[must_use]
    pub fn with_custom_metadata(
        mut self,
        class_name: String,
        mro_names: Vec<String>,
        custom_attrs: Vec<(String, String)>,
    ) -> Self {
        self.custom_class_name = Some(class_name);
        self.custom_mro_names = mro_names;
        self.custom_attrs = custom_attrs;
        self
    }

    /// Returns true when `handler_name` matches a captured custom exception class in MRO.
    #[must_use]
    pub fn matches_custom_handler_name(&self, handler_name: &str) -> bool {
        self.custom_mro_names.iter().any(|name| name == handler_name)
    }

    /// Returns the value associated with this exception, if any.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    #[must_use]
    pub fn exc_type(&self) -> ExcType {
        self.exc_type
    }

    #[must_use]
    pub fn arg(&self) -> Option<&String> {
        self.arg.as_ref()
    }

    #[must_use]
    pub fn cause(&self) -> Option<&Self> {
        self.cause.as_deref()
    }

    #[must_use]
    pub fn context(&self) -> Option<&Self> {
        self.context.as_deref()
    }

    #[must_use]
    pub fn suppress_context(&self) -> bool {
        self.suppress_context
    }

    pub fn set_context(&mut self, context: Option<Self>) {
        self.context = context.map(Box::new);
    }

    pub fn set_cause(&mut self, cause: Option<Self>) {
        self.cause = cause.map(Box::new);
    }

    pub fn set_suppress_context(&mut self, suppress_context: bool) {
        self.suppress_context = suppress_context;
    }

    /// Returns grouped child exceptions, when present.
    #[must_use]
    pub fn exceptions(&self) -> Option<Vec<Self>> {
        if self.exc_type != ExcType::ExceptionGroup {
            return None;
        }
        let encoded = self.value.as_ref()?;
        serde_json::from_str(encoded).ok()
    }

    /// str() for an exception
    #[must_use]
    pub fn py_str(&self) -> String {
        match (self.exc_type, &self.arg) {
            // KeyError expecificaly uses repr of the key for str(exc)
            (ExcType::KeyError, Some(exc)) => StringRepr(exc).to_string(),
            (_, Some(arg)) => arg.to_owned(),
            (_, None) => String::new(),
        }
    }

    pub(crate) fn py_type(&self) -> Type {
        Type::Exception(self.exc_type)
    }

    /// Returns the exception formatted as Python would repr it.
    pub fn py_repr_fmt(&self, f: &mut impl Write) -> std::fmt::Result {
        let type_str: &'static str = self.exc_type.into();
        write!(f, "{type_str}(")?;

        if let Some(arg) = &self.arg {
            string_repr_fmt(arg, f)?;
        }

        f.write_char(')')
    }

    pub(crate) fn with_frame(self, frame: RawStackFrame) -> ExceptionRaise {
        ExceptionRaise {
            exc: self,
            frame: Some(frame),
            hide_caret: false,
            original_value: None,
        }
    }

    pub(crate) fn with_position(self, position: CodeRange) -> ExceptionRaise {
        ExceptionRaise {
            exc: self,
            frame: Some(RawStackFrame::from_position(position)),
            hide_caret: false,
            original_value: None,
        }
    }

    /// Gets an attribute from this exception.
    ///
    /// Handles `.args` for all exceptions, `.value` for StopIteration, and
    /// positional metadata attributes for regex/JSON decode exceptions.
    pub fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        if attr_name == "__cause__" {
            let value = if let Some(cause) = self.cause() {
                Value::Ref(heap.allocate(HeapData::Exception(cause.clone()))?)
            } else {
                Value::None
            };
            Ok(Some(AttrCallResult::Value(value)))
        } else if attr_name == "__context__" {
            let value = if let Some(context) = self.context() {
                Value::Ref(heap.allocate(HeapData::Exception(context.clone()))?)
            } else {
                Value::None
            };
            Ok(Some(AttrCallResult::Value(value)))
        } else if attr_name == "__suppress_context__" {
            Ok(Some(AttrCallResult::Value(Value::Bool(self.suppress_context()))))
        } else if attr_id == StaticStrings::Args {
            // Return all args as a tuple
            let elements: smallvec::SmallVec<[Value; 3]> = if self.exc_type == ExcType::StopIteration {
                // StopIteration uses special handling for its value payload
                if let Some(value) = self.stop_iteration_args_value(heap)? {
                    smallvec![value]
                } else {
                    smallvec![]
                }
            } else {
                // Use deserialized args if available, otherwise fall back to single arg
                let args = self.deserialize_args();
                if !args.is_empty() {
                    args.into()
                } else if let Some(arg_str) = &self.arg {
                    let str_id = heap.allocate(HeapData::Str(Str::from(arg_str.clone())))?;
                    smallvec![Value::Ref(str_id)]
                } else {
                    smallvec![]
                }
            };
            Ok(Some(AttrCallResult::Value(allocate_tuple(elements, heap)?)))
        } else if attr_id == StaticStrings::Value && self.exc_type == ExcType::StopIteration {
            Ok(Some(AttrCallResult::Value(self.stop_iteration_value_for_attr(heap)?)))
        } else if self.exc_type == ExcType::ExceptionGroup && attr_name == "exceptions" {
            Ok(Some(AttrCallResult::Value(
                self.exception_group_exceptions_for_attr(heap)?,
            )))
        } else if self.exc_type == ExcType::ExceptionGroup && attr_name == "message" {
            Ok(Some(AttrCallResult::Value(
                self.exception_group_message_for_attr(heap)?,
            )))
        } else if let Some((pos, lineno, colno)) = self.regex_error_metadata() {
            let attr = interns.get_str(attr_id);
            let value = match attr {
                "pos" => Some(Value::Int(pos)),
                "lineno" => Some(Value::Int(lineno)),
                "colno" => Some(Value::Int(colno)),
                _ => None,
            };
            Ok(value.map(AttrCallResult::Value))
        } else if let Some((pos, lineno, colno)) = self.json_decode_error_metadata() {
            let attr = interns.get_str(attr_id);
            let value = match attr {
                "pos" => Some(Value::Int(pos)),
                "lineno" => Some(Value::Int(lineno)),
                "colno" => Some(Value::Int(colno)),
                _ => None,
            };
            Ok(value.map(AttrCallResult::Value))
        } else if let Some((_, value)) = self
            .custom_attrs
            .iter()
            .find(|(name, _)| name.as_str() == interns.get_str(attr_id))
        {
            let value = Value::Ref(heap.allocate(HeapData::Str(Str::from(value.clone())))?);
            Ok(Some(AttrCallResult::Value(value)))
        } else {
            Ok(None)
        }
    }

    /// Returns regex error metadata `(pos, lineno, colno)` when present.
    fn regex_error_metadata(&self) -> Option<(i64, i64, i64)> {
        let encoded = self.value()?;
        let payload = encoded.strip_prefix(RE_ERROR_META_PREFIX)?;
        let mut parts = payload.split(':');
        let pos = parts.next()?.parse().ok()?;
        let lineno = parts.next()?.parse().ok()?;
        let colno = parts.next()?.parse().ok()?;
        Some((pos, lineno, colno))
    }

    /// Returns JSON decode metadata `(pos, lineno, colno)` when present.
    fn json_decode_error_metadata(&self) -> Option<(i64, i64, i64)> {
        let encoded = self.value()?;
        let payload = encoded.strip_prefix(JSON_DECODE_ERROR_META_PREFIX)?;
        let mut parts = payload.split(':');
        let pos = parts.next()?.parse().ok()?;
        let lineno = parts.next()?.parse().ok()?;
        let colno = parts.next()?.parse().ok()?;
        Some((pos, lineno, colno))
    }

    /// Returns the `.message` attribute for `ExceptionGroup`.
    fn exception_group_message_for_attr(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let message = self.arg.clone().unwrap_or_default();
        let message_id = heap.allocate(HeapData::Str(Str::from(message)))?;
        Ok(Value::Ref(message_id))
    }

    /// Returns the `.exceptions` attribute for `ExceptionGroup` as a tuple.
    fn exception_group_exceptions_for_attr(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let mut elements = smallvec![];
        if let Some(exceptions) = self.exceptions() {
            for exc in &exceptions {
                let exc_id = heap.allocate(HeapData::Exception(exc.clone()))?;
                elements.push(Value::Ref(exc_id));
            }
        }
        Ok(allocate_tuple(elements, heap)?)
    }

    /// Returns the typed StopIteration value for the `.value` attribute.
    ///
    /// If no explicit return value is stored, this matches CPython and returns `None`.
    fn stop_iteration_value_for_attr(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        if let Some(encoded) = self.value() {
            return Self::decode_stop_iteration_value(encoded, heap);
        }
        if let Some(arg) = self.arg() {
            let str_id = heap.allocate(HeapData::Str(Str::from(arg.clone())))?;
            return Ok(Value::Ref(str_id));
        }
        Ok(Value::None)
    }

    /// Returns the typed StopIteration value for `.args` when an argument exists.
    fn stop_iteration_args_value(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Value>> {
        if self.arg().is_none() {
            return Ok(None);
        }
        Ok(Some(self.stop_iteration_value_for_attr(heap)?))
    }

    /// Decodes the internal typed StopIteration value payload into a runtime `Value`.
    ///
    /// Encoding format:
    /// - `b:true` / `b:false` for bool
    /// - `i:<int>` for i64
    /// - `f:<float>` for f64
    /// - `s:<text>` for strings/fallback objects
    /// - legacy untagged strings are parsed best-effort for compatibility
    fn decode_stop_iteration_value(encoded: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        if encoded == "n" || encoded == "None" {
            return Ok(Value::None);
        }
        if encoded == "True" {
            return Ok(Value::Bool(true));
        }
        if encoded == "False" {
            return Ok(Value::Bool(false));
        }

        if let Some(bool_value) = encoded.strip_prefix("b:") {
            return match bool_value {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => {
                    let str_id = heap.allocate(HeapData::Str(Str::from(bool_value.to_owned())))?;
                    Ok(Value::Ref(str_id))
                }
            };
        }
        if let Some(int_value) = encoded.strip_prefix("i:")
            && let Ok(parsed) = int_value.parse::<i64>()
        {
            return Ok(Value::Int(parsed));
        }
        if let Some(float_value) = encoded.strip_prefix("f:")
            && let Ok(parsed) = float_value.parse::<f64>()
        {
            return Ok(Value::Float(parsed));
        }
        if let Some(string_value) = encoded.strip_prefix("s:") {
            let str_id = heap.allocate(HeapData::Str(Str::from(string_value.to_owned())))?;
            return Ok(Value::Ref(str_id));
        }

        // Legacy untagged payloads from older snapshots.
        if let Ok(parsed) = encoded.parse::<i64>() {
            return Ok(Value::Int(parsed));
        }
        if let Ok(parsed) = encoded.parse::<f64>() {
            return Ok(Value::Float(parsed));
        }

        let str_id = heap.allocate(HeapData::Str(Str::from(encoded.to_owned())))?;
        Ok(Value::Ref(str_id))
    }
}

/// A raised exception with optional stack frame for traceback.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExceptionRaise {
    pub exc: SimpleException,
    /// The stack frame where the exception was raised (first in vec is closest "bottom" frame).
    pub frame: Option<RawStackFrame>,
    /// Whether to hide the caret marker when creating the stack frame.
    ///
    /// CPython doesn't show carets for attribute GET errors, but does show them
    /// for attribute SET errors. This flag allows error creators to specify
    /// whether the caret should be hidden.
    #[serde(default)]
    pub hide_caret: bool,
    /// The original exception value for user-defined exception instances.
    ///
    /// When a user-defined exception instance is raised (e.g., `raise MyException()`),
    /// this field stores the original Value::Ref to preserve the instance identity.
    /// This ensures that `isinstance(e, MyException)` and `type(e)` work correctly
    /// when the exception is caught via a parent class (e.g., `except Exception as e`).
    ///
    /// For builtin exceptions raised without an instance (e.g., `raise ValueError`),
    /// this is None and a new exception value is created from `exc`.
    #[serde(skip, default)]
    pub original_value: Option<Value>,
}

impl Clone for ExceptionRaise {
    fn clone(&self) -> Self {
        Self {
            exc: self.exc.clone(),
            frame: self.frame.clone(),
            hide_caret: self.hide_caret,
            // original_value uses manual ref counting via clone_with_heap,
            // so we don't clone it here. This is fine because original_value
            // is only needed during the raise/catch flow and should not be
            // relied upon after cloning (the cloned exception will create a
            // new exception value from simple_exc if needed).
            original_value: None,
        }
    }
}

impl From<SimpleException> for ExceptionRaise {
    fn from(exc: SimpleException) -> Self {
        Self {
            exc,
            frame: None,
            hide_caret: false,
            original_value: None,
        }
    }
}

impl From<Exception> for ExceptionRaise {
    fn from(exc: Exception) -> Self {
        Self {
            exc: exc.into(),
            frame: None,
            hide_caret: false,
            original_value: None,
        }
    }
}

impl ExceptionRaise {
    /// Adds a caller's frame as the outermost frame in the traceback chain.
    ///
    /// This is used when an exception propagates up through call frames.
    /// The new frame becomes the ultimate parent (displayed first in traceback,
    /// since tracebacks show "most recent call last").
    ///
    /// Special case: If the innermost frame has no name yet (created with `with_position`),
    /// this sets its name instead of creating a new parent. This happens when the error
    /// is raised from a namespace lookup - the initial frame has the position but not
    /// the function name, which gets filled in as the error propagates.
    pub(crate) fn add_caller_frame(&mut self, position: CodeRange, name: StringId) {
        self.add_caller_frame_inner(position, name, false);
    }

    fn add_caller_frame_inner(&mut self, position: CodeRange, name: StringId, hide_caret: bool) {
        if let Some(ref mut frame) = self.frame {
            // If innermost frame has no name, set it instead of adding a parent
            // This handles errors from namespace lookups which create nameless frames
            if frame.frame_name.is_none() {
                frame.frame_name = Some(name);
                frame.hide_caret = hide_caret;
                return;
            }
            // Find the outermost frame (the one with no parent) and add the new frame as its parent
            let mut current = frame;
            while current.parent.is_some() {
                current = current.parent.as_mut().unwrap();
            }
            let mut new_frame = RawStackFrame::new(position, name, None);
            new_frame.hide_caret = hide_caret;
            current.parent = Some(Box::new(new_frame));
        } else {
            // No frame yet - create one
            let mut new_frame = RawStackFrame::new(position, name, None);
            new_frame.hide_caret = hide_caret;
            self.frame = Some(new_frame);
        }
    }

    /// Converts this exception to a `Exception` for the public API.
    ///
    /// Uses `Interns` to resolve `StringId` references to actual strings.
    /// Extracts preview lines from the source code for traceback display.
    #[must_use]
    pub fn into_python_exception(self, interns: &Interns, source: &str) -> Exception {
        let traceback = self
            .frame
            .map(|frame| {
                let mut frames = Vec::new();
                let mut current = Some(&frame);
                while let Some(f) = current {
                    frames.push(StackFrame::from_raw(f, interns, source));
                    current = f.parent.as_deref();
                }
                // Reverse so outermost frame is first (Python's "most recent call last" ordering)
                frames.reverse();
                frames
            })
            .unwrap_or_default();

        Exception::new_full(self.exc.exc_type(), self.exc.arg().cloned(), traceback)
    }
}

/// A stack frame for traceback information.
///
/// Stores position information and optional function name as StringId.
/// The actual name string must be looked up externally when formatting the traceback.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawStackFrame {
    pub position: CodeRange,
    /// The name of the frame (function name StringId, or None for module-level code).
    pub frame_name: Option<StringId>,
    pub parent: Option<Box<Self>>,
    /// Whether to hide the caret marker in the traceback for this frame.
    ///
    /// Set to `true` for:
    /// - `raise` statements (CPython doesn't show carets for raise)
    /// - `AttributeError` on attribute access (CPython doesn't show carets for these)
    pub hide_caret: bool,
}

impl RawStackFrame {
    /// Creates a new frame with a function name for traceback display.
    pub(crate) fn new(position: CodeRange, frame_name: StringId, parent: Option<&Self>) -> Self {
        Self {
            position,
            frame_name: Some(frame_name),
            parent: parent.map(|p| Box::new(p.clone())),
            hide_caret: false,
        }
    }

    /// Creates a new nameless frame for module-level errors at the given position.
    pub(crate) fn from_position(position: CodeRange) -> Self {
        Self {
            position,
            frame_name: None,
            parent: None,
            hide_caret: false,
        }
    }

    /// Creates a new frame for a raise statement (no caret will be shown).
    pub(crate) fn from_raise(position: CodeRange, frame_name: StringId) -> Self {
        Self {
            position,
            frame_name: Some(frame_name),
            parent: None,
            hide_caret: true,
        }
    }
}

/// Runtime error types that can occur during execution.
///
/// Three variants:
/// - `Internal`: Bug in interpreter implementation (static message)
/// - `Exc`: Python exception that can be caught by try/except (when implemented)
/// - `UncatchableExc`: Python exception from resource limits that CANNOT be caught
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum RunError {
    /// Internal interpreter error - indicates a bug in Ouros, not user code.
    Internal(Cow<'static, str>),
    /// Catchable Python exception (e.g., ValueError, TypeError).
    Exc(Box<ExceptionRaise>),
    /// Uncatchable Python exception from resource limits (MemoryError, TimeoutError, RecursionError).
    ///
    /// These exceptions display with proper tracebacks like normal Python exceptions,
    /// but cannot be caught by try/except blocks. This prevents untrusted code from
    /// suppressing resource limit violations.
    UncatchableExc(Box<ExceptionRaise>),
}

impl From<ExceptionRaise> for RunError {
    fn from(exc: ExceptionRaise) -> Self {
        Self::Exc(Box::new(exc))
    }
}

impl From<SimpleException> for RunError {
    fn from(exc: SimpleException) -> Self {
        Self::Exc(Box::new(exc.into()))
    }
}

impl From<Exception> for RunError {
    fn from(exc: Exception) -> Self {
        Self::Exc(Box::new(exc.into()))
    }
}

impl From<FormatError> for RunError {
    fn from(err: FormatError) -> Self {
        let exc_type = match &err {
            FormatError::Overflow(_) => ExcType::OverflowError,
            FormatError::InvalidAlignment(_) | FormatError::ValueError(_) => ExcType::ValueError,
        };
        Self::Exc(Box::new(SimpleException::new_msg(exc_type, err).into()))
    }
}

impl RunError {
    /// Converts this runtime error to a `Exception` for the public API.
    ///
    /// Internal errors are converted to `RuntimeError` exceptions with no traceback.
    #[must_use]
    pub fn into_python_exception(self, interns: &Interns, source: &str) -> Exception {
        match self {
            Self::Exc(exc) | Self::UncatchableExc(exc) => exc.into_python_exception(interns, source),
            Self::Internal(err) => Exception::runtime_error(format!("Internal error in ouros: {err}")),
        }
    }

    pub fn internal(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::Internal(msg.into())
    }

    /// Returns true if this error is a StopIteration exception.
    pub fn is_stop_iteration(&self) -> bool {
        match self {
            Self::Exc(exc) => exc.exc.exc_type() == ExcType::StopIteration,
            _ => false,
        }
    }

    /// Returns true if this error is a catchable exception of `exc_type`.
    pub fn is_exception_type(&self, exc_type: ExcType) -> bool {
        match self {
            Self::Exc(exc) => exc.exc.exc_type() == exc_type,
            _ => false,
        }
    }
}

/// Formats a list of parameter names for error messages.
///
/// Examples:
/// - `["a"]` -> `'a'`
/// - `["a", "b"]` -> `'a' and 'b'`
/// - `["a", "b", "c"]` -> `'a', 'b' and 'c'`
fn format_param_names(names: &[&str]) -> String {
    match names.len() {
        0 => String::new(),
        1 => format!("'{}'", names[0]),
        2 => format!("'{}' and '{}'", names[0], names[1]),
        _ => {
            let last = names.last().unwrap();
            let rest: Vec<_> = names[..names.len() - 1].iter().map(|n| format!("'{n}'")).collect();
            format!("{} and '{last}'", rest.join(", "))
        }
    }
}
