//! Custom exception types for the Ouros Python interpreter.
//!
//! Provides a hierarchy of exception types that wrap Ouros's internal exceptions,
//! preserving traceback information and allowing Python code to distinguish
//! between syntax errors, runtime errors, and type checking errors from Ouros-executed code.
//!
//! ## Exception Hierarchy
//!
//! ```text
//! SandboxError(Exception)        # Base class for all sandbox exceptions
//! ├── SandboxSyntaxError         # Raised when syntax is invalid or can't be parsed
//! ├── SandboxRuntimeError        # Raised when code fails during execution
//! └── SandboxTypingError         # Raised when type checking finds errors in the code
//! ```

use ::ouros::{ExcType, Exception, StackFrame};
use ouros_type_checking::TypeCheckingDiagnostics;
use pyo3::{
    PyClassInitializer, PyTypeCheck,
    exceptions::{self},
    prelude::*,
    types::{PyDict, PyList, PyString},
};

use crate::dataclass::get_frozen_instance_error;

/// Base exception for all sandbox interpreter errors.
///
/// This is the parent class for `SandboxSyntaxError`, `SandboxRuntimeError`, and `SandboxTypingError`.
/// Catching `SandboxError` will catch any exception raised by the sandbox.
#[pyclass(name = "SandboxError", extends=exceptions::PyException, module="ouros", subclass)]
#[derive(Clone)]
pub struct OurosError {
    /// The underlying Ouros exception.
    exc: Exception,
}

impl OurosError {
    /// Converts an Ouros exception to a `PyErr`.
    ///
    /// For `SyntaxError` exceptions, creates a `SandboxSyntaxError`.
    /// For all other exceptions, creates a `SandboxRuntimeError` with all the exception
    /// information preserved, including the traceback frames and display string.
    #[must_use]
    pub fn new_err(py: Python<'_>, exc: Exception) -> PyErr {
        // Syntax errors get their own exception type
        if exc.exc_type() == ExcType::SyntaxError {
            OurosSyntaxError::new_err(py, exc)
        } else {
            OurosRuntimeError::new_err(py, exc)
        }
    }
}

impl OurosError {
    /// Creates a new `OurosError` wrapping a `Exception`.
    #[must_use]
    pub fn new(exc: Exception) -> Self {
        Self { exc }
    }

    /// Returns the exception type.
    fn exc_type(&self) -> ExcType {
        self.exc.exc_type()
    }

    /// Returns the exception message, if any.
    fn message(&self) -> Option<&str> {
        self.exc.message()
    }
}

#[pymethods]
impl OurosError {
    /// Returns the inner exception as a Python exception object.
    ///
    /// This recreates a native Python exception (e.g., `ValueError`, `TypeError`)
    /// from the stored exception type and message.
    fn exception(&self, py: Python<'_>) -> Py<PyAny> {
        let py_err = exc_ouros_to_py(py, self.exc.clone());
        py_err.into_value(py).into_any()
    }

    fn __str__(&self) -> String {
        self.message().unwrap_or_default().to_string()
    }

    fn __repr__(&self) -> String {
        let exc_type_name = self.exc_type();
        if let Some(msg) = self.message() {
            format!("SandboxError({exc_type_name}: {msg})")
        } else {
            format!("SandboxError({exc_type_name})")
        }
    }
}

/// Raised when Python code has syntax errors or cannot be parsed.
///
/// Inherits from `SandboxError`. The inner exception is always a `SyntaxError`.
#[pyclass(name = "SandboxSyntaxError", extends=OurosError, module="ouros")]
#[derive(Clone)]
pub struct OurosSyntaxError;

impl OurosSyntaxError {
    /// Creates a new `SandboxSyntaxError` with the given message.
    #[must_use]
    pub fn new_err(py: Python<'_>, exc: Exception) -> PyErr {
        let base_error = OurosError::new(exc);
        let init = PyClassInitializer::from(base_error).add_subclass(Self);
        match Py::new(py, init) {
            Ok(err) => PyErr::from_value(err.into_bound(py).into_any()),
            Err(e) => e,
        }
    }
}

#[pymethods]
impl OurosSyntaxError {
    /// Returns formatted exception string.
    ///
    /// Args:
    ///     format: 'type-msg' - 'ExceptionType: message' format
    ///             'msg' - just the message
    #[pyo3(signature = (format = "msg"))]
    #[expect(clippy::needless_pass_by_value, reason = "required by macro")]
    fn display(slf: PyRef<'_, Self>, format: &str) -> PyResult<String> {
        let parent = slf.as_super();
        match format {
            "msg" => Ok(parent.message().unwrap_or_default().to_string()),
            "type-msg" => Ok(parent.exc.summary()),
            _ => Err(exceptions::PyValueError::new_err(format!(
                "Invalid display format: '{format}'. Expected 'type-msg', or 'msg'"
            ))),
        }
    }

    #[expect(clippy::needless_pass_by_value, reason = "required by macro")]
    fn __str__(slf: PyRef<'_, Self>) -> String {
        slf.as_super().message().unwrap_or_default().to_string()
    }

    #[expect(clippy::needless_pass_by_value, reason = "required by macro")]
    fn __repr__(slf: PyRef<'_, Self>) -> String {
        let parent = slf.as_super();
        if let Some(msg) = parent.message() {
            format!("SandboxSyntaxError({msg})")
        } else {
            "SandboxSyntaxError()".to_string()
        }
    }
}

/// Raised when type checking finds errors in the code.
///
/// Inherits from `SandboxError`. This exception is raised when static type
/// analysis detects type errors. Stores the `TypeCheckingFailure` so diagnostics
/// can be re-rendered with different format/color settings via `display()`.
#[pyclass(name = "SandboxTypingError", extends=OurosError, module="ouros")]
pub struct OurosTypingError {
    failure: TypeCheckingDiagnostics,
}

impl OurosTypingError {
    /// Creates a `SandboxTypingError` from a `TypeCheckingFailure`.
    #[must_use]
    pub fn new_err(py: Python<'_>, failure: TypeCheckingDiagnostics) -> PyErr {
        // we need a Exception to create the base, but it shouldn't be visible anywhere
        let base = OurosError::new(Exception::new(ExcType::TypeError, None));
        let init = PyClassInitializer::from(base).add_subclass(Self { failure });
        match Py::new(py, init) {
            Ok(err) => PyErr::from_value(err.into_bound(py).into_any()),
            Err(e) => e,
        }
    }
}

#[pymethods]
impl OurosTypingError {
    /// Renders the type error diagnostics with the specified format and color.
    ///
    /// Args:
    ///     format: Output format
    ///     color: Whether to include ANSI color codes in the output.
    #[pyo3(signature = (format = "full", color = false))]
    fn display(&self, format: &str, color: bool) -> PyResult<String> {
        self.failure
            .clone()
            .color(color)
            .format_from_str(format)
            .map_err(exceptions::PyValueError::new_err)
            .map(|f| f.to_string())
    }

    fn __str__(&self) -> String {
        self.failure.to_string()
    }

    fn __repr__(&self) -> String {
        format!("SandboxTypingError({})", self.failure)
    }
}

/// Raised when sandbox code fails during execution.
///
/// Inherits from `SandboxError`. Additionally provides `traceback()` to access
/// the Ouros stack frames where the error occurred.
#[pyclass(name = "SandboxRuntimeError", extends=OurosError, module="ouros")]
pub struct OurosRuntimeError {
    /// The traceback frames where the error occurred (pre-converted to Python objects).
    frames: Vec<Py<PyFrame>>,
}

impl OurosRuntimeError {
    /// Creates a new `SandboxRuntimeError` from the given exception data.
    #[must_use]
    pub fn new_err(py: Python<'_>, exc: Exception) -> PyErr {
        // Convert stack frames to PyFrame objects
        let frames_result: PyResult<Vec<Py<PyFrame>>> = exc
            .traceback()
            .iter()
            .map(|f| Py::new(py, PyFrame::from_stack_frame(f)))
            .collect();

        let frames = match frames_result {
            Ok(frames) => frames,
            Err(e) => return e,
        };

        let base_error = OurosError::new(exc);
        // Create the SandboxRuntimeError with proper initialization
        let runtime_error = Self { frames };

        let init = pyo3::PyClassInitializer::from(base_error).add_subclass(runtime_error);
        match Py::new(py, init) {
            Ok(err) => PyErr::from_value(err.into_bound(py).into_any()),
            Err(e) => e,
        }
    }
}

#[pymethods]
impl OurosRuntimeError {
    /// Returns the Ouros traceback as a list of Frame objects.
    fn traceback(&self, py: Python<'_>) -> Py<PyList> {
        PyList::new(py, &self.frames)
            .expect("failed to create frames list")
            .unbind()
    }

    /// Returns formatted exception string.
    ///
    /// Overrides the base class to provide the full traceback when format='traceback'.
    #[pyo3(signature = (format = "traceback"))]
    #[expect(clippy::needless_pass_by_value, reason = "required by macro")]
    fn display(slf: PyRef<'_, Self>, format: &str) -> PyResult<String> {
        match format {
            "traceback" => Ok(slf.as_super().exc.to_string()),
            "type-msg" => Ok(slf.as_super().exc.summary()),
            "msg" => Ok(slf.as_super().message().unwrap_or_default().to_string()),
            _ => Err(exceptions::PyValueError::new_err(format!(
                "Invalid display format: '{format}'. Expected 'traceback', 'type-msg', or 'msg'"
            ))),
        }
    }

    #[expect(clippy::needless_pass_by_value, reason = "required by macro")]
    fn __str__(slf: PyRef<'_, Self>) -> String {
        let parent = slf.as_super();
        let exc_type_name = parent.exc_type();
        if let Some(msg) = parent.message()
            && !msg.is_empty()
        {
            return format!("{exc_type_name}: {msg}");
        }
        format!("{exc_type_name}")
    }

    #[expect(clippy::needless_pass_by_value, reason = "required by macro")]
    fn __repr__(slf: PyRef<'_, Self>) -> String {
        let parent = slf.as_super();
        let exc_type_name = parent.exc_type();
        if let Some(msg) = parent.message()
            && !msg.is_empty()
        {
            return format!("SandboxRuntimeError({exc_type_name}: {msg})");
        }
        format!("SandboxRuntimeError({exc_type_name})")
    }
}

/// A single frame in a Ouros traceback.
///
/// Contains all the information needed to display a traceback line:
/// the file location, function name, and optional source code preview.
#[pyclass(name = "Frame", module = "ouros", frozen)]
#[derive(Debug, Clone)]
pub struct PyFrame {
    /// The filename where the code is located.
    #[pyo3(get)]
    pub filename: String,
    /// Line number (1-based).
    #[pyo3(get)]
    pub line: u16,
    /// Column number (1-based).
    #[pyo3(get)]
    pub column: u16,
    /// End line number (1-based).
    #[pyo3(get)]
    pub end_line: u16,
    /// End column number (1-based).
    #[pyo3(get)]
    pub end_column: u16,
    /// The name of the function, or None for module-level code.
    #[pyo3(get)]
    pub function_name: Option<String>,
    /// The source code line for preview in the traceback.
    #[pyo3(get)]
    pub source_line: Option<String>,
}

#[pymethods]
impl PyFrame {
    fn dict(&self, py: Python<'_>) -> Py<PyDict> {
        let dict = PyDict::new(py);
        dict.set_item("filename", self.filename.clone()).unwrap();
        dict.set_item("line", self.line).unwrap();
        dict.set_item("column", self.column).unwrap();
        dict.set_item("end_line", self.end_line).unwrap();
        dict.set_item("end_column", self.end_column).unwrap();
        dict.set_item("function_name", self.function_name.clone()).unwrap();
        dict.set_item("source_line", self.source_line.clone()).unwrap();
        dict.unbind()
    }

    fn __repr__(&self) -> String {
        let func = self.function_name.as_ref().map_or("<module>".to_string(), Clone::clone);
        format!(
            "Frame(filename='{}', line={}, column={}, function_name='{}')",
            self.filename, self.line, self.column, func
        )
    }
}

impl PyFrame {
    /// Creates a `PyFrame` from Ouros's `StackFrame`.
    #[must_use]
    pub fn from_stack_frame(frame: &StackFrame) -> Self {
        Self {
            filename: frame.filename.clone(),
            line: frame.start.line,
            column: frame.start.column,
            end_line: frame.end.line,
            end_column: frame.end.column,
            function_name: frame.frame_name.clone(),
            source_line: frame.preview_line.clone(),
        }
    }
}

/// Converts Ouros's `Exception` to the matching Python exception value.
///
/// Creates an appropriate Python exception type with the message.
/// The traceback information is included in the exception message
/// since PyO3 doesn't provide direct traceback manipulation.
pub fn exc_ouros_to_py(py: Python<'_>, exc: Exception) -> PyErr {
    let exc_type = exc.exc_type();
    let msg = exc.into_message().unwrap_or_default();

    match exc_type {
        ExcType::Exception => exceptions::PyException::new_err(msg),
        #[cfg(Py_3_11)]
        ExcType::ExceptionGroup => exceptions::PyBaseExceptionGroup::new_err((msg, Vec::<PyErr>::new())),
        #[cfg(not(Py_3_11))]
        ExcType::ExceptionGroup => exceptions::PyException::new_err(msg),
        ExcType::BaseException => exceptions::PyBaseException::new_err(msg),
        ExcType::SystemExit => exceptions::PySystemExit::new_err(msg),
        ExcType::KeyboardInterrupt => exceptions::PyKeyboardInterrupt::new_err(msg),
        ExcType::ArithmeticError => exceptions::PyArithmeticError::new_err(msg),
        ExcType::FloatingPointError => exceptions::PyFloatingPointError::new_err(msg),
        ExcType::OverflowError => exceptions::PyOverflowError::new_err(msg),
        ExcType::ZeroDivisionError => exceptions::PyZeroDivisionError::new_err(msg),
        ExcType::LookupError => exceptions::PyLookupError::new_err(msg),
        ExcType::IndexError => exceptions::PyIndexError::new_err(msg),
        ExcType::KeyError => exceptions::PyKeyError::new_err(msg),
        ExcType::RuntimeError => exceptions::PyRuntimeError::new_err(msg),
        ExcType::NotImplementedError => exceptions::PyNotImplementedError::new_err(msg),
        ExcType::RecursionError => exceptions::PyRecursionError::new_err(msg),
        ExcType::AssertionError => exceptions::PyAssertionError::new_err(msg),
        ExcType::AttributeError => exceptions::PyAttributeError::new_err(msg),
        ExcType::FrozenInstanceError => {
            if let Ok(exc_cls) = get_frozen_instance_error(py)
                && let Ok(exc_instance) = exc_cls.call1((PyString::new(py, &msg),))
            {
                return PyErr::from_value(exc_instance);
            }
            // if creating the right exception fails, fallback to AttributeError which it's a subclass of
            exceptions::PyAttributeError::new_err(msg)
        }
        ExcType::MemoryError => exceptions::PyMemoryError::new_err(msg),
        ExcType::BufferError => exceptions::PyBufferError::new_err(msg),
        ExcType::EOFError => exceptions::PyEOFError::new_err(msg),
        ExcType::NameError => exceptions::PyNameError::new_err(msg),
        ExcType::UnboundLocalError => exceptions::PyUnboundLocalError::new_err(msg),
        ExcType::ReferenceError => exceptions::PyReferenceError::new_err(msg),
        ExcType::StopAsyncIteration => exceptions::PyStopAsyncIteration::new_err(msg),
        ExcType::StopIteration => exceptions::PyStopIteration::new_err(msg),
        ExcType::SyntaxError => exceptions::PySyntaxError::new_err(msg),
        ExcType::IndentationError => {
            if let Ok(exc_cls) = get_indentation_error(py)
                && let Ok(exc_instance) = exc_cls.call1((PyString::new(py, &msg),))
            {
                return PyErr::from_value(exc_instance);
            }
            exceptions::PySyntaxError::new_err(msg)
        }
        ExcType::TimeoutError => exceptions::PyTimeoutError::new_err(msg),
        ExcType::TypeError => exceptions::PyTypeError::new_err(msg),
        ExcType::ValueError => exceptions::PyValueError::new_err(msg),
        ExcType::UnicodeDecodeError => exceptions::PyUnicodeDecodeError::new_err(msg),
        ExcType::JSONDecodeError => exceptions::PyValueError::new_err(msg),
        ExcType::TOMLDecodeError => exceptions::PyValueError::new_err(msg),
        ExcType::ImportError => exceptions::PyImportError::new_err(msg),
        ExcType::ModuleNotFoundError => exceptions::PyModuleNotFoundError::new_err(msg),
        ExcType::OSError => exceptions::PyOSError::new_err(msg),
        ExcType::FileNotFoundError => exceptions::PyFileNotFoundError::new_err(msg),
        ExcType::FileExistsError => exceptions::PyFileExistsError::new_err(msg),
        ExcType::IsADirectoryError => exceptions::PyIsADirectoryError::new_err(msg),
        ExcType::NotADirectoryError => exceptions::PyNotADirectoryError::new_err(msg),
        ExcType::PermissionError => exceptions::PyPermissionError::new_err(msg),
        ExcType::GeneratorExit => exceptions::PyGeneratorExit::new_err(msg),
    }
}

/// Converts a python exception to ouros.
///
/// Used when resuming execution with an exception from Python.
pub fn exc_py_to_ouros(py: Python<'_>, py_err: &PyErr) -> Exception {
    let exc = py_err.value(py);
    let exc_type = py_err_to_exc_type(exc);
    let arg = exc.str().ok().map(|s| s.to_string_lossy().into_owned());

    Exception::new(exc_type, arg)
}

/// Converts a Python exception to Ouros's `Object::Exception`.
pub fn exc_to_ouros_object(exc: &Bound<'_, exceptions::PyBaseException>) -> ::ouros::Object {
    let exc_type = py_err_to_exc_type(exc);
    let arg = exc.str().ok().map(|s| s.to_string_lossy().into_owned());

    ::ouros::Object::Exception { exc_type, arg }
}

/// Maps a Python exception type to Ouros's `ExcType` enum.
///
/// NOTE: order matters here as some exceptions are subclasses of others!
/// In general we group exceptions by their type hierarchy to improve performance.
fn py_err_to_exc_type(exc: &Bound<'_, exceptions::PyBaseException>) -> ExcType {
    #[cfg(Py_3_11)]
    if exceptions::PyBaseExceptionGroup::type_check(exc) {
        return ExcType::ExceptionGroup;
    }

    // Exception hierarchy
    if exceptions::PyException::type_check(exc) {
        // put the most commonly used exceptions first
        if exceptions::PyTypeError::type_check(exc) {
            ExcType::TypeError
        // ValueError hierarchy (check UnicodeDecodeError first as it's a subclass)
        } else if exceptions::PyValueError::type_check(exc) {
            if exceptions::PyUnicodeDecodeError::type_check(exc) {
                ExcType::UnicodeDecodeError
            } else if is_json_decode_error(exc) {
                ExcType::JSONDecodeError
            } else {
                ExcType::ValueError
            }
        } else if exceptions::PyAssertionError::type_check(exc) {
            ExcType::AssertionError
        } else if exceptions::PySyntaxError::type_check(exc) {
            if is_indentation_error(exc) {
                ExcType::IndentationError
            } else {
                ExcType::SyntaxError
            }
        // LookupError hierarchy
        } else if exceptions::PyLookupError::type_check(exc) {
            if exceptions::PyKeyError::type_check(exc) {
                ExcType::KeyError
            } else if exceptions::PyIndexError::type_check(exc) {
                ExcType::IndexError
            } else {
                ExcType::LookupError
            }
        // ArithmeticError hierarchy
        } else if exceptions::PyArithmeticError::type_check(exc) {
            if exceptions::PyZeroDivisionError::type_check(exc) {
                ExcType::ZeroDivisionError
            } else if exceptions::PyOverflowError::type_check(exc) {
                ExcType::OverflowError
            } else if exceptions::PyFloatingPointError::type_check(exc) {
                ExcType::FloatingPointError
            } else {
                ExcType::ArithmeticError
            }
        // RuntimeError hierarchy
        } else if exceptions::PyRuntimeError::type_check(exc) {
            if exceptions::PyNotImplementedError::type_check(exc) {
                ExcType::NotImplementedError
            } else if exceptions::PyRecursionError::type_check(exc) {
                ExcType::RecursionError
            } else {
                ExcType::RuntimeError
            }
        // AttributeError hierarchy
        } else if exceptions::PyAttributeError::type_check(exc) {
            if is_frozen_instance_error(exc) {
                ExcType::FrozenInstanceError
            } else {
                ExcType::AttributeError
            }
        // NameError hierarchy (check UnboundLocalError first as it's a subclass)
        } else if exceptions::PyNameError::type_check(exc) {
            if exceptions::PyUnboundLocalError::type_check(exc) {
                ExcType::UnboundLocalError
            } else {
                ExcType::NameError
            }
        } else if exceptions::PyBufferError::type_check(exc) {
            ExcType::BufferError
        } else if exceptions::PyEOFError::type_check(exc) {
            ExcType::EOFError
        } else if exceptions::PyReferenceError::type_check(exc) {
            ExcType::ReferenceError
        } else if exceptions::PyStopAsyncIteration::type_check(exc) {
            ExcType::StopAsyncIteration
        // OSError hierarchy (check specific subclasses first)
        } else if exceptions::PyOSError::type_check(exc) {
            if exceptions::PyFileNotFoundError::type_check(exc) {
                ExcType::FileNotFoundError
            } else if exceptions::PyFileExistsError::type_check(exc) {
                ExcType::FileExistsError
            } else if exceptions::PyIsADirectoryError::type_check(exc) {
                ExcType::IsADirectoryError
            } else if exceptions::PyNotADirectoryError::type_check(exc) {
                ExcType::NotADirectoryError
            } else if exceptions::PyPermissionError::type_check(exc) {
                ExcType::PermissionError
            } else {
                ExcType::OSError
            }
        // other standalone exception types
        } else if exceptions::PyTimeoutError::type_check(exc) {
            ExcType::TimeoutError
        } else if exceptions::PyMemoryError::type_check(exc) {
            ExcType::MemoryError
        } else {
            ExcType::Exception
        }
    // BaseException direct subclasses
    } else if exceptions::PySystemExit::type_check(exc) {
        ExcType::SystemExit
    } else if exceptions::PyKeyboardInterrupt::type_check(exc) {
        ExcType::KeyboardInterrupt
    // Catch-all for BaseException
    } else {
        ExcType::BaseException
    }
}

/// Checks if an exception is an instance of `dataclasses.FrozenInstanceError`.
///
/// Since `FrozenInstanceError` is not a built-in PyO3 exception type, we need to
/// check using Python's isinstance against the imported class.
fn is_frozen_instance_error(exc: &Bound<'_, exceptions::PyBaseException>) -> bool {
    if let Ok(frozen_error_cls) = get_frozen_instance_error(exc.py()) {
        exc.is_instance(frozen_error_cls).unwrap_or(false)
    } else {
        false
    }
}

/// Returns the Python `IndentationError` class from `builtins`.
fn get_indentation_error(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    let builtins = py.import("builtins")?;
    builtins.getattr("IndentationError")
}

/// Checks if an exception is an instance of builtins `IndentationError`.
fn is_indentation_error(exc: &Bound<'_, exceptions::PyBaseException>) -> bool {
    if let Ok(indentation_error_cls) = get_indentation_error(exc.py()) {
        exc.is_instance(&indentation_error_cls).unwrap_or(false)
    } else {
        false
    }
}

/// Returns the Python `json.JSONDecodeError` class from the `json` module.
fn get_json_decode_error(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    let json = py.import("json")?;
    json.getattr("JSONDecodeError")
}

/// Checks if an exception is an instance of `json.JSONDecodeError`.
fn is_json_decode_error(exc: &Bound<'_, exceptions::PyBaseException>) -> bool {
    if let Ok(json_decode_error_cls) = get_json_decode_error(exc.py()) {
        exc.is_instance(&json_decode_error_cls).unwrap_or(false)
    } else {
        false
    }
}
