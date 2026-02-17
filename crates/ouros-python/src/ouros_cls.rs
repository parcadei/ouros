use std::{
    borrow::Cow,
    fmt::Write,
    sync::{Mutex, MutexGuard},
};

// Use `::ouros` to refer to the external crate (not the pymodule)
use ::ouros::{
    Exception, ExternalResult, LimitedTracker, NoLimitTracker, Object, PrintWriter, ResourceTracker, RunProgress,
    Runner, Snapshot, StdPrint,
};
use ouros::{ExcType, FutureSnapshot, OsFunction};
use ouros_type_checking::{SourceFile, type_check};
use pyo3::{
    IntoPyObjectExt,
    exceptions::{PyKeyError, PyRuntimeError, PyTypeError, PyValueError},
    intern,
    prelude::*,
    types::{PyBytes, PyDict, PyList, PyTuple, PyType},
};

use crate::{
    convert::{ouros_to_py, py_to_ouros},
    exceptions::{OurosError, OurosTypingError, exc_py_to_ouros},
    external::ExternalFunctionRegistry,
    limits::{PySignalTracker, extract_limits},
};

/// A sandboxed Python interpreter instance.
///
/// Parses and compiles Python code on initialization, then can be run
/// multiple times with different input values. This separates the parsing
/// cost from execution, making repeated runs more efficient.
#[pyclass(name = "Sandbox", module = "ouros")]
#[derive(Debug)]
pub struct PyOuros {
    /// The compiled code snapshot, ready to execute.
    runner: Runner,
    /// The artificial name of the python code "file"
    script_name: String,
    /// Names of input variables expected by the code.
    input_names: Vec<String>,
    /// Names of external functions the code can call.
    external_function_names: Vec<String>,
    /// Registry of dataclass types for reconstructing original types on output.
    ///
    /// Maps class name to the original Python type, allowing `isinstance(result, OriginalClass)`
    /// to work correctly after round-tripping through Ouros.
    dataclass_registry: Py<PyDict>,
}

#[pymethods]
impl PyOuros {
    /// Creates a new Ouros interpreter by parsing the given code.
    ///
    /// # Arguments
    /// * `code` - Python code to execute
    /// * `inputs` - List of input variable names available in the code
    /// * `external_functions` - List of external function names the code can call
    /// * `type_check` - Whether to perform type checking on the code
    /// * `type_check_stubs` - Prefix code to be executed before type checking
    /// * `dataclass_registry` - Registry of dataclass types for reconstructing original types on output.
    #[new]
    #[pyo3(signature = (code, *, script_name="main.py", inputs=None, external_functions=None, type_check=false, type_check_stubs=None, dataclass_registry=None))]
    #[expect(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        code: String,
        script_name: &str,
        inputs: Option<&Bound<'_, PyList>>,
        external_functions: Option<&Bound<'_, PyList>>,
        type_check: bool,
        type_check_stubs: Option<&str>,
        dataclass_registry: Option<Bound<'_, PyList>>,
    ) -> PyResult<Self> {
        let input_names = list_str(inputs, "inputs")?;
        let external_function_names = list_str(external_functions, "external_functions")?;

        if type_check {
            py_type_check(py, &code, script_name, type_check_stubs)?;
        }

        // Create the snapshot (parses the code)
        let runner = Runner::new(code, script_name, input_names.clone(), external_function_names.clone())
            .map_err(|e| OurosError::new_err(py, e))?;

        Ok(Self {
            runner,
            script_name: script_name.to_string(),
            input_names,
            external_function_names,
            dataclass_registry: prep_registry(py, dataclass_registry)?.unbind(),
        })
    }

    /// Registers a dataclass type for proper isinstance() support on output.
    ///
    /// When a dataclass passes through Ouros and is returned, it becomes a `OurosDataclass`.
    /// By registering the original type, `isinstance(result, OriginalClass)` will return `True`.
    ///
    /// # Arguments
    /// * `cls` - The dataclass type to register
    ///
    /// # Raises
    /// * `TypeError` if the argument is not a dataclass type
    fn register_dataclass(&self, py: Python<'_>, cls: &Bound<'_, PyType>) -> PyResult<()> {
        // Use id(type) as the key for registry lookups
        let type_id = cls.as_ptr() as u64;
        self.dataclass_registry.bind(py).set_item(type_id, cls)?;
        Ok(())
    }

    /// Performs static type checking on the code.
    ///
    /// Analyzes the code for type errors without executing it. This uses
    /// a subset of Python's type system supported by Ouros.
    ///
    /// # Args
    /// * `prefix_code` - Optional prefix to prepend to the code before type checking,
    ///   e.g. with inputs and external function signatures
    ///
    /// # Raises
    /// * `RuntimeError` if type checking infrastructure fails
    /// * `OurosTypingError` if type errors are found
    #[pyo3(signature = (prefix_code=None))]
    fn type_check(&self, py: Python<'_>, prefix_code: Option<&str>) -> PyResult<()> {
        py_type_check(py, self.runner.code(), &self.script_name, prefix_code)
    }

    /// Executes the code and returns the result.
    ///
    /// # Returns
    /// The result of the last expression in the code
    ///
    /// # Raises
    /// Various Python exceptions matching what the code would raise
    #[pyo3(signature = (*, inputs=None, limits=None, external_functions=None, print_callback=None, os=None))]
    fn run(
        &self,
        py: Python<'_>,
        inputs: Option<&Bound<'_, PyDict>>,
        limits: Option<&Bound<'_, PyDict>>,
        external_functions: Option<&Bound<'_, PyDict>>,
        print_callback: Option<&Bound<'_, PyAny>>,
        os: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        // Extract input values in the order they were declared
        let input_values = self.extract_input_values(inputs)?;

        if let Some(os_callback) = os
            && !os_callback.is_callable()
        {
            let msg = format!("TypeError: '{}' object is not callable", os_callback.get_type().name()?);
            return Err(PyTypeError::new_err(msg));
        }

        // Build print writer
        let print_writer = print_callback.map(CallbackStringPrint::new);

        // Run with appropriate tracker type (must branch due to different generic types)
        if let Some(limits) = limits {
            let tracker = PySignalTracker::new(LimitedTracker::new(extract_limits(limits)?));
            if let Some(print_writer) = print_writer {
                self.run_impl(py, input_values, tracker, external_functions, os, print_writer)
            } else {
                self.run_impl(py, input_values, tracker, external_functions, os, StdPrint)
            }
        } else {
            let tracker = PySignalTracker::new(NoLimitTracker);
            if let Some(print_writer) = print_writer {
                self.run_impl(py, input_values, tracker, external_functions, os, print_writer)
            } else {
                self.run_impl(py, input_values, tracker, external_functions, os, StdPrint)
            }
        }
    }

    #[pyo3(signature = (*, inputs=None, limits=None, print_callback=None))]
    fn start<'py>(
        &self,
        py: Python<'py>,
        inputs: Option<&Bound<'py, PyDict>>,
        limits: Option<&Bound<'py, PyDict>>,
        print_callback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Extract input values in the order they were declared
        let input_values = self.extract_input_values(inputs)?;

        // Helper macro to start execution with GIL released
        // CallbackStringPrint is Send so this works for both print_callback cases
        macro_rules! start_impl {
            ($tracker:expr, $print_output:expr) => {{
                let runner = self.runner.clone();
                py.detach(|| runner.start(input_values, $tracker, &mut $print_output))
                    .map_err(|e| OurosError::new_err(py, e))?
            }};
        }

        // Build print writer - CallbackStringPrint is Send so GIL can be released
        let print_writer = print_callback.map(CallbackStringPrint::new);

        // Branch on limits (different generic types) then on print_writer
        let progress = if let Some(limits) = limits {
            let tracker = PySignalTracker::new(LimitedTracker::new(extract_limits(limits)?));
            if let Some(mut print_writer) = print_writer {
                EitherProgress::Limited(start_impl!(tracker, print_writer))
            } else {
                EitherProgress::Limited(start_impl!(tracker, StdPrint))
            }
        } else {
            let tracker = PySignalTracker::new(NoLimitTracker);
            if let Some(mut print_writer) = print_writer {
                EitherProgress::NoLimit(start_impl!(tracker, print_writer))
            } else {
                EitherProgress::NoLimit(start_impl!(tracker, StdPrint))
            }
        };
        progress.progress_or_complete(
            py,
            self.script_name.clone(),
            print_callback.map(|c| c.clone().unbind()),
            self.dataclass_registry.clone_ref(py),
        )
    }

    /// Serializes the Sandbox instance to a binary format.
    ///
    /// The serialized data can be stored and later restored with `Sandbox.load()`.
    /// This allows caching parsed code to avoid re-parsing on subsequent runs.
    ///
    /// # Returns
    /// Bytes containing the serialized Sandbox instance.
    ///
    /// # Raises
    /// `ValueError` if serialization fails.
    fn dump<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let serialized = SerializedOuros {
            runner: self.runner.clone(),
            script_name: self.script_name.clone(),
            input_names: self.input_names.clone(),
            external_function_names: self.external_function_names.clone(),
        };
        let bytes = postcard::to_allocvec(&serialized).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserializes a Sandbox instance from binary format.
    ///
    /// # Arguments
    /// * `data` - The serialized Sandbox data from `dump()`
    /// * `dataclass_registry` - Optional list of dataclasses to register
    ///
    /// # Returns
    /// A new Sandbox instance.
    ///
    /// # Raises
    /// `ValueError` if deserialization fails.
    #[staticmethod]
    #[pyo3(signature = (data, *, dataclass_registry=None))]
    fn load(
        py: Python<'_>,
        data: &Bound<'_, PyBytes>,
        dataclass_registry: Option<Bound<'_, PyList>>,
    ) -> PyResult<Self> {
        let bytes = data.as_bytes();
        let serialized: SerializedOuros =
            postcard::from_bytes(bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;

        Ok(Self {
            runner: serialized.runner,
            script_name: serialized.script_name,
            input_names: serialized.input_names,
            external_function_names: serialized.external_function_names,
            dataclass_registry: prep_registry(py, dataclass_registry)?.unbind(),
        })
    }

    fn __repr__(&self) -> String {
        let lines = self.runner.code().lines().count();
        let mut s = format!(
            "Sandbox(<{} line{} of code>, script_name='{}'",
            lines,
            if lines == 1 { "" } else { "s" },
            self.script_name
        );
        if !self.input_names.is_empty() {
            write!(s, ", inputs={:?}", self.input_names).unwrap();
        }
        if !self.external_function_names.is_empty() {
            write!(s, ", external_functions={:?}", self.external_function_names).unwrap();
        }
        s.push(')');
        s
    }
}

fn py_type_check(py: Python<'_>, code: &str, script_name: &str, type_stubs: Option<&str>) -> PyResult<()> {
    let type_stubs = type_stubs.map(|type_stubs| SourceFile::new(type_stubs, "type_stubs.pyi"));

    let opt_diagnostics =
        type_check(&SourceFile::new(code, script_name), type_stubs.as_ref()).map_err(PyRuntimeError::new_err)?;

    if let Some(diagnostic) = opt_diagnostics {
        Err(OurosTypingError::new_err(py, diagnostic))
    } else {
        Ok(())
    }
}

impl PyOuros {
    /// Extracts input values from the dict in the order they were declared.
    ///
    /// Validates that all required inputs are provided and no extra inputs are given.
    fn extract_input_values(&self, inputs: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<::ouros::Object>> {
        if self.input_names.is_empty() {
            if inputs.is_some() {
                return Err(PyTypeError::new_err(
                    "No input variables declared but inputs dict was provided",
                ));
            }
            return Ok(vec![]);
        }

        let Some(inputs) = inputs else {
            return Err(PyTypeError::new_err(format!(
                "Missing required inputs: {:?}",
                self.input_names
            )));
        };

        // Extract values in declaration order
        self.input_names
            .iter()
            .map(|name| {
                let value = inputs
                    .get_item(name)?
                    .ok_or_else(|| PyKeyError::new_err(format!("Missing required input: '{name}'")))?;
                py_to_ouros(&value)
            })
            .collect::<PyResult<_>>()
    }

    /// Runs code with a generic resource tracker, releasing the GIL during execution.
    ///
    /// The GIL is released during Ouros execution and re-acquired when needed
    /// (e.g., for external function calls or print callbacks).
    fn run_impl(
        &self,
        py: Python<'_>,
        input_values: Vec<Object>,
        tracker: impl ResourceTracker + Send,
        external_functions: Option<&Bound<'_, PyDict>>,
        os: Option<&Bound<'_, PyAny>>,
        mut print_output: impl PrintWriter + Send,
    ) -> PyResult<Py<PyAny>> {
        let dataclass_registry = self.dataclass_registry.bind(py);
        if self.external_function_names.is_empty() && os.is_none() {
            let runner = &self.runner;
            return match py.detach(|| runner.run(input_values, tracker, &mut print_output)) {
                Ok(v) => ouros_to_py(py, &v, dataclass_registry),
                Err(err) => Err(OurosError::new_err(py, err)),
            };
        }
        // Clone the runner since start() consumes it - allows reuse of the parsed code
        let runner = self.runner.clone();
        let mut progress = py
            .detach(|| runner.start(input_values, tracker, &mut print_output))
            .map_err(|e| OurosError::new_err(py, e))?;

        loop {
            match progress {
                RunProgress::Complete(result) => return ouros_to_py(py, &result, dataclass_registry),
                RunProgress::FunctionCall {
                    function_name,
                    args,
                    kwargs,
                    state,
                    ..
                } => {
                    let registry = external_functions
                        .map(|d| ExternalFunctionRegistry::new(py, d, dataclass_registry))
                        .ok_or_else(|| {
                            PyRuntimeError::new_err(format!(
                                "External function '{function_name}' called but no external_functions provided"
                            ))
                        })?;

                    let return_value = registry.call(&function_name, &args, &kwargs);

                    progress = py
                        .detach(|| state.run(return_value, &mut print_output))
                        .map_err(|e| OurosError::new_err(py, e))?;
                }
                RunProgress::ResolveFutures { .. } => {
                    return Err(PyRuntimeError::new_err(
                        "async futures not supported with `Sandbox.run`",
                    ));
                }
                RunProgress::OsCall {
                    function,
                    args,
                    kwargs,
                    state,
                    ..
                } => {
                    let result: ExternalResult = if let Some(os_callback) = os {
                        // Convert args to Python
                        let py_args: Vec<Py<PyAny>> = args
                            .iter()
                            .map(|arg| ouros_to_py(py, arg, dataclass_registry))
                            .collect::<PyResult<_>>()?;
                        let py_args_tuple = PyTuple::new(py, py_args)?;

                        // Convert kwargs to Python dict
                        let py_kwargs = PyDict::new(py);
                        for (k, v) in &kwargs {
                            py_kwargs.set_item(
                                ouros_to_py(py, k, dataclass_registry)?,
                                ouros_to_py(py, v, dataclass_registry)?,
                            )?;
                        }

                        // call the os callback, if an exception is raised, return it to ouros
                        match os_callback.call1((function.to_string(), py_args_tuple, py_kwargs)) {
                            Ok(result) => py_to_ouros(&result)?.into(),
                            Err(err) => exc_py_to_ouros(py, &err).into(),
                        }
                    } else {
                        Exception::new(
                            ExcType::NotImplementedError,
                            Some(format!("OS function '{function}' not implemented")),
                        )
                        .into()
                    };

                    progress = py
                        .detach(|| state.run(result, &mut print_output))
                        .map_err(|e| OurosError::new_err(py, e))?;
                }
            }
        }
    }
}

/// pyclass doesn't support generic types, hence hard coding the generics
#[derive(Debug)]
enum EitherProgress {
    NoLimit(RunProgress<PySignalTracker<NoLimitTracker>>),
    Limited(RunProgress<PySignalTracker<LimitedTracker>>),
}

impl EitherProgress {
    fn progress_or_complete(
        self,
        py: Python<'_>,
        script_name: String,
        print_callback: Option<Py<PyAny>>,
        dc_registry: Py<PyDict>,
    ) -> PyResult<Bound<'_, PyAny>> {
        match self {
            Self::NoLimit(p) => match p {
                RunProgress::Complete(result) => PyOurosComplete::create(py, &result, &dc_registry),
                RunProgress::FunctionCall {
                    function_name,
                    args,
                    kwargs,
                    state,
                    call_id,
                } => Self::function_snapshot(
                    py,
                    function_name,
                    &args,
                    &kwargs,
                    call_id,
                    EitherSnapshot::NoLimit(state),
                    script_name,
                    print_callback,
                    dc_registry,
                ),
                RunProgress::ResolveFutures(state) => Self::future_snapshot(
                    py,
                    EitherFutureSnapshot::NoLimit(state),
                    script_name,
                    print_callback,
                    dc_registry,
                ),
                RunProgress::OsCall {
                    function,
                    args,
                    kwargs,
                    call_id,
                    state,
                } => Self::os_function_snapshot(
                    py,
                    function,
                    &args,
                    &kwargs,
                    call_id,
                    EitherSnapshot::NoLimit(state),
                    script_name,
                    print_callback,
                    dc_registry,
                ),
            },
            Self::Limited(p) => match p {
                RunProgress::Complete(result) => PyOurosComplete::create(py, &result, &dc_registry),
                RunProgress::FunctionCall {
                    function_name,
                    args,
                    kwargs,
                    state,
                    call_id,
                } => Self::function_snapshot(
                    py,
                    function_name,
                    &args,
                    &kwargs,
                    call_id,
                    EitherSnapshot::Limited(state),
                    script_name,
                    print_callback,
                    dc_registry,
                ),
                RunProgress::ResolveFutures(state) => Self::future_snapshot(
                    py,
                    EitherFutureSnapshot::Limited(state),
                    script_name,
                    print_callback,
                    dc_registry,
                ),
                RunProgress::OsCall {
                    function,
                    args,
                    kwargs,
                    call_id,
                    state,
                } => Self::os_function_snapshot(
                    py,
                    function,
                    &args,
                    &kwargs,
                    call_id,
                    EitherSnapshot::Limited(state),
                    script_name,
                    print_callback,
                    dc_registry,
                ),
            },
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn function_snapshot<'py>(
        py: Python<'py>,
        function_name: String,
        args: &[Object],
        kwargs: &[(Object, Object)],
        call_id: u32,
        snapshot: EitherSnapshot,
        script_name: String,
        print_callback: Option<Py<PyAny>>,
        dc_registry: Py<PyDict>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let dcr = dc_registry.bind(py);
        let items: PyResult<Vec<Py<PyAny>>> = args.iter().map(|item| ouros_to_py(py, item, dcr)).collect();

        let dict = PyDict::new(py);
        for (k, v) in kwargs {
            dict.set_item(ouros_to_py(py, k, dcr)?, ouros_to_py(py, v, dcr)?)?;
        }

        let slf = PyOurosSnapshot {
            snapshot: Mutex::new(snapshot),
            print_callback: Mutex::new(print_callback.map(|callback| callback.clone_ref(py))),
            script_name,
            is_os_function: false,
            function_name,
            args: PyTuple::new(py, items?)?.unbind(),
            kwargs: dict.unbind(),
            call_id,
            dc_registry,
        };
        slf.into_bound_py_any(py)
    }

    #[expect(clippy::too_many_arguments)]
    fn os_function_snapshot<'py>(
        py: Python<'py>,
        function: OsFunction,
        args: &[Object],
        kwargs: &[(Object, Object)],
        call_id: u32,
        snapshot: EitherSnapshot,
        script_name: String,
        print_callback: Option<Py<PyAny>>,
        dc_registry: Py<PyDict>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let dcr = dc_registry.bind(py);
        let items: PyResult<Vec<Py<PyAny>>> = args.iter().map(|item| ouros_to_py(py, item, dcr)).collect();

        let dict = PyDict::new(py);
        for (k, v) in kwargs {
            dict.set_item(ouros_to_py(py, k, dcr)?, ouros_to_py(py, v, dcr)?)?;
        }

        let slf = PyOurosSnapshot {
            snapshot: Mutex::new(snapshot),
            print_callback: Mutex::new(print_callback.map(|callback| callback.clone_ref(py))),
            script_name,
            is_os_function: true,
            function_name: function.to_string(),
            args: PyTuple::new(py, items?)?.unbind(),
            kwargs: dict.unbind(),
            call_id,
            dc_registry,
        };
        slf.into_bound_py_any(py)
    }

    fn future_snapshot(
        py: Python<'_>,
        snapshot: EitherFutureSnapshot,
        script_name: String,
        print_callback: Option<Py<PyAny>>,
        dc_registry: Py<PyDict>,
    ) -> PyResult<Bound<'_, PyAny>> {
        let slf = PyOurosFutureSnapshot {
            snapshot: Mutex::new(snapshot),
            print_callback: Mutex::new(print_callback.map(|callback| callback.clone_ref(py))),
            script_name,
            dc_registry,
        };
        slf.into_bound_py_any(py)
    }
}

/// Runtime execution snapshot, holds multiple resource tracker types since pyclass structs can't be generic.
///
/// Used internally by `PyOurosSnapshot` to store execution state.
/// The `Done` variant indicates the snapshot has been consumed.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum EitherSnapshot {
    NoLimit(Snapshot<PySignalTracker<NoLimitTracker>>),
    Limited(Snapshot<PySignalTracker<LimitedTracker>>),
    /// Done is used when taking the snapshot to run it
    /// should only be done after execution is complete
    Done,
}

/// Snapshot objects must remain sendable across threads.
///
/// Python async runtimes and GC may finalize objects on a different thread than the
/// creation thread; marking this class `unsendable` causes pyo3 to panic during drop.
#[pyclass(name = "Snapshot", module = "ouros")]
#[derive(Debug)]
pub struct PyOurosSnapshot {
    snapshot: Mutex<EitherSnapshot>,
    print_callback: Mutex<Option<Py<PyAny>>>,
    dc_registry: Py<PyDict>,

    /// Name of the script being executed
    #[pyo3(get)]
    pub script_name: String,

    /// Whether this call refers to an OS function
    #[pyo3(get)]
    pub is_os_function: bool,

    /// The name of the function being called.
    #[pyo3(get)]
    pub function_name: String,
    /// The positional arguments passed to the function.
    #[pyo3(get)]
    pub args: Py<PyTuple>,
    /// The keyword arguments passed to the function (key, value pairs).
    #[pyo3(get)]
    pub kwargs: Py<PyDict>,
    /// The unique identifier for this call
    #[pyo3(get)]
    pub call_id: u32,
}

/// Extract an external result (object or exception) from a dictionary
fn extract_external_result(
    py: Python<'_>,
    dict: &Bound<'_, PyDict>,
    error_msg: &'static str,
) -> PyResult<ExternalResult> {
    if dict.len() != 1 {
        Err(PyTypeError::new_err(error_msg))
    } else if let Some(rv) = dict.get_item(intern!(py, "return_value"))? {
        // Return value provided
        Ok(py_to_ouros(&rv)?.into())
    } else if let Some(exc) = dict.get_item(intern!(py, "exception"))? {
        // Exception provided
        let py_err = PyErr::from_value(exc.into_any());
        Ok(exc_py_to_ouros(py, &py_err).into())
    } else if let Some(exc) = dict.get_item(intern!(py, "future"))? {
        if exc.eq(py.Ellipsis()).unwrap_or_default() {
            Ok(ExternalResult::Future)
        } else {
            Err(PyTypeError::new_err(
                "Value for the 'future' key must be Ellipsis (...)",
            ))
        }
    } else {
        // wrong key in kwargs
        Err(PyTypeError::new_err(error_msg))
    }
}

/// Locks a mutex and recovers the inner value if the lock is poisoned.
///
/// Snapshot operations should preserve Python-level behavior even if a previous
/// callback panicked while holding the lock. Recovering here avoids turning that
/// into a permanent unusable Snapshot object.
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[pymethods]
impl PyOurosSnapshot {
    /// Resumes execution with either a return value or an exception.
    ///
    /// Exactly one of `return_value`, `exception` or `future` must be provided as a keyword argument.
    ///
    /// # Raises
    /// * `TypeError` if both arguments are provided, or neither
    /// * `RuntimeError` if the snapshot has already been resumed
    #[pyo3(signature = (**kwargs))]
    pub fn resume<'py>(&self, py: Python<'py>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Bound<'py, PyAny>> {
        const ARGS_ERROR: &str = "resume() accepts either return_value or exception, not both";
        let Some(kwargs) = kwargs else {
            return Err(PyTypeError::new_err(ARGS_ERROR));
        };
        let external_result = extract_external_result(py, kwargs, ARGS_ERROR)?;

        let snapshot = {
            let mut snapshot = lock_unpoisoned(&self.snapshot);
            std::mem::replace(&mut *snapshot, EitherSnapshot::Done)
        };

        // Build print writer before detaching - clone_ref needs py token
        let print_writer = lock_unpoisoned(&self.print_callback)
            .as_ref()
            .map(|cb| CallbackStringPrint::from_py(cb.clone_ref(py)));

        let progress = match snapshot {
            EitherSnapshot::NoLimit(snapshot) => {
                let result = if let Some(mut print_writer) = print_writer {
                    py.detach(|| snapshot.run(external_result, &mut print_writer))
                } else {
                    py.detach(|| snapshot.run(external_result, &mut StdPrint))
                };
                EitherProgress::NoLimit(result.map_err(|e| OurosError::new_err(py, e))?)
            }
            EitherSnapshot::Limited(snapshot) => {
                let result = if let Some(mut print_writer) = print_writer {
                    py.detach(|| snapshot.run(external_result, &mut print_writer))
                } else {
                    py.detach(|| snapshot.run(external_result, &mut StdPrint))
                };
                EitherProgress::Limited(result.map_err(|e| OurosError::new_err(py, e))?)
            }
            EitherSnapshot::Done => return Err(PyRuntimeError::new_err("Progress already resumed")),
        };

        let print_callback = lock_unpoisoned(&self.print_callback).take();
        progress.progress_or_complete(
            py,
            self.script_name.clone(),
            print_callback,
            self.dc_registry.clone_ref(py),
        )
    }

    /// Serializes the Snapshot instance to a binary format.
    ///
    /// The serialized data can be stored and later restored with `Snapshot.load()`.
    /// This allows suspending execution and resuming later, potentially in a different process.
    ///
    /// Note: The `print_callback` is not serialized and must be re-provided when resuming
    /// after loading.
    ///
    /// # Returns
    /// Bytes containing the serialized Snapshot instance.
    ///
    /// # Raises
    /// `ValueError` if serialization fails.
    /// `RuntimeError` if the progress has already been resumed.
    fn dump<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        #[derive(serde::Serialize)]
        struct SerializedSnapshot<'a> {
            snapshot: &'a EitherSnapshot,
            script_name: &'a str,
            is_os_function: bool,
            function_name: &'a str,
            args: Vec<Object>,
            kwargs: Vec<(Object, Object)>,
            call_id: u32,
        }

        let snapshot = lock_unpoisoned(&self.snapshot);
        if matches!(*snapshot, EitherSnapshot::Done) {
            return Err(PyRuntimeError::new_err(
                "Cannot dump progress that has already been resumed",
            ));
        }

        // Convert Python args to Object
        let args: Vec<Object> = self
            .args
            .bind(py)
            .iter()
            .map(|item| py_to_ouros(&item))
            .collect::<PyResult<_>>()?;

        // Convert Python kwargs to Object pairs
        let kwargs: Vec<(Object, Object)> = self
            .kwargs
            .bind(py)
            .iter()
            .map(|(k, v)| Ok((py_to_ouros(&k)?, py_to_ouros(&v)?)))
            .collect::<PyResult<_>>()?;

        let serialized = SerializedSnapshot {
            snapshot: &snapshot,
            script_name: &self.script_name,
            is_os_function: self.is_os_function,
            function_name: &self.function_name,
            args,
            kwargs,
            call_id: self.call_id,
        };
        let bytes = postcard::to_allocvec(&serialized).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserializes a Snapshot instance from binary format.
    ///
    /// Note: The `print_callback` is not preserved during serialization and must be
    /// re-provided as a keyword argument if print output is needed.
    ///
    /// # Arguments
    /// * `data` - The serialized Snapshot data from `dump()`
    /// * `print_callback` - Optional callback for print output
    /// * `dataclass_registry` - Optional list of dataclasses to register
    ///
    /// # Returns
    /// A new Snapshot instance.
    ///
    /// # Raises
    /// `ValueError` if deserialization fails.
    #[staticmethod]
    #[pyo3(signature = (data, *, print_callback=None, dataclass_registry=None))]
    fn load(
        py: Python<'_>,
        data: &Bound<'_, PyBytes>,
        print_callback: Option<Py<PyAny>>,
        dataclass_registry: Option<Bound<'_, PyList>>,
    ) -> PyResult<Self> {
        #[derive(serde::Deserialize)]
        struct SerializedSnapshotOwned {
            snapshot: EitherSnapshot,
            script_name: String,
            is_os_function: bool,
            function_name: String,
            args: Vec<Object>,
            kwargs: Vec<(Object, Object)>,
            call_id: u32,
        }

        let bytes = data.as_bytes();

        let serialized: SerializedSnapshotOwned =
            postcard::from_bytes(bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;

        let dc_registry = prep_registry(py, dataclass_registry)?;

        // Convert Object args to Python
        let args: Vec<Py<PyAny>> = serialized
            .args
            .iter()
            .map(|item| ouros_to_py(py, item, &dc_registry))
            .collect::<PyResult<_>>()?;

        // Convert Object kwargs to Python dict
        let kwargs_dict = PyDict::new(py);
        for (k, v) in &serialized.kwargs {
            kwargs_dict.set_item(ouros_to_py(py, k, &dc_registry)?, ouros_to_py(py, v, &dc_registry)?)?;
        }

        Ok(Self {
            snapshot: Mutex::new(serialized.snapshot),
            print_callback: Mutex::new(print_callback),
            dc_registry: dc_registry.unbind(),
            script_name: serialized.script_name,
            is_os_function: serialized.is_os_function,
            function_name: serialized.function_name,
            args: PyTuple::new(py, args)?.unbind(),
            kwargs: kwargs_dict.unbind(),
            call_id: serialized.call_id,
        })
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Snapshot(script_name='{}', function_name='{}', args={}, kwargs={})",
            self.script_name,
            self.function_name,
            self.args.bind(py).repr()?,
            self.kwargs.bind(py).repr()?
        ))
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum EitherFutureSnapshot {
    NoLimit(FutureSnapshot<PySignalTracker<NoLimitTracker>>),
    Limited(FutureSnapshot<PySignalTracker<LimitedTracker>>),
    /// Done is used when taking the snapshot to run it
    /// should only be done after execution is complete
    Done,
}

/// FutureSnapshot objects must remain sendable across threads.
///
/// Python async runtimes and GC may finalize objects on a different thread than the
/// creation thread; marking this class `unsendable` causes pyo3 to panic during drop.
#[pyclass(name = "FutureSnapshot", module = "ouros")]
#[derive(Debug)]
pub struct PyOurosFutureSnapshot {
    snapshot: Mutex<EitherFutureSnapshot>,
    print_callback: Mutex<Option<Py<PyAny>>>,
    dc_registry: Py<PyDict>,

    /// Name of the script being executed
    #[pyo3(get)]
    pub script_name: String,
}

#[pymethods]
impl PyOurosFutureSnapshot {
    /// Resumes execution with results for one or more futures.
    #[pyo3(signature = (results))]
    pub fn resume<'py>(&self, py: Python<'py>, results: &Bound<'_, PyDict>) -> PyResult<Bound<'py, PyAny>> {
        const ARGS_ERROR: &str = "results values must be a dict with either 'return_value' or 'exception', not both";
        let external_results = results
            .iter()
            .map(|(key, value)| {
                let call_id = key.extract::<u32>()?;
                let dict = value.cast::<PyDict>()?;
                let value = extract_external_result(py, dict, ARGS_ERROR)?;
                Ok((call_id, value))
            })
            .collect::<PyResult<Vec<_>>>()?;
        let snapshot = {
            let mut snapshot = lock_unpoisoned(&self.snapshot);
            std::mem::replace(&mut *snapshot, EitherFutureSnapshot::Done)
        };

        // Build print writer before detaching - clone_ref needs py token
        let print_writer = lock_unpoisoned(&self.print_callback)
            .as_ref()
            .map(|cb| CallbackStringPrint::from_py(cb.clone_ref(py)));

        let progress = match snapshot {
            EitherFutureSnapshot::NoLimit(snapshot) => {
                let result = if let Some(mut print_writer) = print_writer {
                    py.detach(|| snapshot.resume(external_results, &mut print_writer))
                } else {
                    py.detach(|| snapshot.resume(external_results, &mut StdPrint))
                };
                EitherProgress::NoLimit(result.map_err(|e| OurosError::new_err(py, e))?)
            }
            EitherFutureSnapshot::Limited(snapshot) => {
                let result = if let Some(mut print_writer) = print_writer {
                    py.detach(|| snapshot.resume(external_results, &mut print_writer))
                } else {
                    py.detach(|| snapshot.resume(external_results, &mut StdPrint))
                };
                EitherProgress::Limited(result.map_err(|e| OurosError::new_err(py, e))?)
            }
            EitherFutureSnapshot::Done => return Err(PyRuntimeError::new_err("Progress already resumed")),
        };

        let print_callback = lock_unpoisoned(&self.print_callback).take();
        progress.progress_or_complete(
            py,
            self.script_name.clone(),
            print_callback,
            self.dc_registry.clone_ref(py),
        )
    }

    /// Returns the pending call IDs associated with the FutureSnapshot instance.
    ///
    /// # Returns
    /// A list of pending call IDs.
    #[getter]
    fn pending_call_ids(&self) -> PyResult<Vec<u32>> {
        match &*lock_unpoisoned(&self.snapshot) {
            EitherFutureSnapshot::NoLimit(snapshot) => Ok(snapshot.pending_call_ids().to_vec()),
            EitherFutureSnapshot::Limited(snapshot) => Ok(snapshot.pending_call_ids().to_vec()),
            EitherFutureSnapshot::Done => Err(PyRuntimeError::new_err("FutureSnapshot already resumed")),
        }
    }

    /// Serializes the FutureSnapshot instance to a binary format.
    ///
    /// The serialized data can be stored and later restored with `FutureSnapshot.load()`.
    /// This allows suspending execution and resuming later, potentially in a different process.
    ///
    /// Note: The `print_callback` is not serialized and must be re-provided when resuming
    /// after loading.
    ///
    /// # Returns
    /// Bytes containing the serialized FutureSnapshot instance.
    ///
    /// # Raises
    /// `ValueError` if serialization fails.
    /// `RuntimeError` if the progress has already been resumed.
    fn dump<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        #[derive(serde::Serialize)]
        struct SerializedSnapshot<'a> {
            snapshot: &'a EitherFutureSnapshot,
            script_name: &'a str,
        }

        let snapshot = lock_unpoisoned(&self.snapshot);
        if matches!(*snapshot, EitherFutureSnapshot::Done) {
            return Err(PyRuntimeError::new_err(
                "Cannot dump progress that has already been resumed",
            ));
        }

        let serialized = SerializedSnapshot {
            snapshot: &snapshot,
            script_name: &self.script_name,
        };
        let bytes = postcard::to_allocvec(&serialized).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Deserializes a FutureSnapshot instance from binary format.
    ///
    /// Note: The `print_callback` is not preserved during serialization and must be
    /// re-provided as a keyword argument if print output is needed.
    ///
    /// # Arguments
    /// * `data` - The serialized FutureSnapshot data from `dump()`
    /// * `print_callback` - Optional callback for print output
    /// * `dataclass_registry` - Optional list of dataclasses to register
    ///
    /// # Returns
    /// A new FutureSnapshot instance.
    ///
    /// # Raises
    /// `ValueError` if deserialization fails.
    #[staticmethod]
    #[pyo3(signature = (data, *, print_callback=None, dataclass_registry=None))]
    fn load(
        py: Python<'_>,
        data: &Bound<'_, PyBytes>,
        print_callback: Option<Py<PyAny>>,
        dataclass_registry: Option<Bound<'_, PyList>>,
    ) -> PyResult<Self> {
        #[derive(serde::Deserialize)]
        struct SerializedSnapshotOwned {
            snapshot: EitherFutureSnapshot,
            script_name: String,
        }

        let bytes = data.as_bytes();

        let serialized: SerializedSnapshotOwned =
            postcard::from_bytes(bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;

        let dc_registry = prep_registry(py, dataclass_registry)?;

        Ok(Self {
            snapshot: Mutex::new(serialized.snapshot),
            print_callback: Mutex::new(print_callback),
            dc_registry: dc_registry.unbind(),
            script_name: serialized.script_name,
        })
    }

    fn __repr__(&self) -> String {
        let pending_call_ids = if let Ok(ids) = self.pending_call_ids() {
            let ids = ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
            Cow::Owned(format!("[{ids}]"))
        } else {
            "None".into()
        };
        format!(
            "FutureSnapshot(script_name='{}', pending_call_ids={})",
            self.script_name, pending_call_ids
        )
    }
}

#[pyclass(name = "Complete", module = "ouros")]
pub struct PyOurosComplete {
    #[pyo3(get)]
    pub output: Py<PyAny>,
    // TODO we might want to add stats on execution here like time, allocations, etc.
}

impl PyOurosComplete {
    fn create<'py>(py: Python<'py>, output: &Object, dc_registry: &Py<PyDict>) -> PyResult<Bound<'py, PyAny>> {
        let dcr = dc_registry.bind(py);
        let output = ouros_to_py(py, output, dcr)?;
        let slf = Self { output };
        slf.into_bound_py_any(py)
    }
}

#[pymethods]
impl PyOurosComplete {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("Complete(output={})", self.output.bind(py).repr()?))
    }
}

fn prep_registry<'py>(py: Python<'py>, dataclass_registry: Option<Bound<'py, PyList>>) -> PyResult<Bound<'py, PyDict>> {
    let dc_registry = PyDict::new(py);

    if let Some(registry_list) = dataclass_registry {
        for cls in registry_list {
            // Use id(type) as the key for registry lookups
            let type_id = cls.as_ptr() as u64;
            dc_registry.set_item(type_id, cls)?;
        }
    }
    Ok(dc_registry)
}

fn list_str(arg: Option<&Bound<'_, PyList>>, name: &str) -> PyResult<Vec<String>> {
    if let Some(names) = arg {
        names
            .iter()
            .map(|item| item.extract::<String>())
            .collect::<PyResult<Vec<_>>>()
            .map_err(|e| PyTypeError::new_err(format!("{name}: {e}")))
    } else {
        Ok(vec![])
    }
}

/// A `PrintWriter` implementation that calls a Python callback for each print output.
///
/// This struct holds a GIL-independent `Py<PyAny>` reference to the callback,
/// allowing it to be used across GIL release boundaries. The GIL is re-acquired
/// briefly for each callback invocation.
#[derive(Debug)]
pub struct CallbackStringPrint(Py<PyAny>);

impl CallbackStringPrint {
    /// Creates a new `CallbackStringPrint` from a borrowed Python callback.
    fn new(callback: &Bound<'_, PyAny>) -> Self {
        Self(callback.clone().unbind())
    }

    /// Creates a new `CallbackStringPrint` from an owned `Py<PyAny>`.
    fn from_py(callback: Py<PyAny>) -> Self {
        Self(callback)
    }
}

impl PrintWriter for CallbackStringPrint {
    fn stdout_write(&mut self, output: Cow<'_, str>) -> Result<(), Exception> {
        Python::attach(|py| {
            self.0.bind(py).call1(("stdout", output.as_ref()))?;
            Ok::<_, PyErr>(())
        })
        .map_err(|e| Python::attach(|py| exc_py_to_ouros(py, &e)))
    }

    fn stdout_push(&mut self, end: char) -> Result<(), Exception> {
        Python::attach(|py| {
            self.0.bind(py).call1(("stdout", end.to_string()))?;
            Ok::<_, PyErr>(())
        })
        .map_err(|e| Python::attach(|py| exc_py_to_ouros(py, &e)))
    }
}

/// Serialization wrapper for `PyOuros` that includes all fields needed for reconstruction.
#[derive(serde::Serialize, serde::Deserialize)]
struct SerializedOuros {
    runner: Runner,
    script_name: String,
    input_names: Vec<String>,
    external_function_names: Vec<String>,
}
