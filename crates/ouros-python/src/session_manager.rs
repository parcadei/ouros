//! PyO3 bindings for `SessionManager` -- multi-session Python interpreter management.
//!
//! Exposes [`SessionManager`](ouros::session_manager::SessionManager) to Python
//! as a `ouros.SessionManager` class. All Rust output structs
//! (`ExecuteOutput`, `VariableValue`, `HeapStats`, etc.) are converted to plain
//! Python dicts so that the Python API stays simple and duck-type friendly.
//!
//! The class is marked `unsendable` because the underlying `ReplSession`
//! contains `Cell` internals that are not `Send`/`Sync`.

use ::ouros::{
    ExternalResult, HeapDiff, HeapStats, ReplProgress,
    session_manager::{
        ChangedVariable, EvalOutput, ExecuteOutput, HeapDiffResult, RewindResult, SaveResult, SavedSessionInfo,
        SessionError, SessionInfo, SessionManager, StorageBackend, VariableDiff, VariableInfo, VariableValue,
    },
};
use pyo3::{
    exceptions::PyRuntimeError,
    prelude::*,
    types::{PyBool, PyBytes, PyDict, PyList},
};
use serde_json::Value as JsonValue;

// =============================================================================
// Error conversion
// =============================================================================

/// Converts a [`SessionError`] into a Python `RuntimeError`.
///
/// Each error variant gets a descriptive prefix so callers can match on the
/// error message (e.g. `match='not found'`).
///
/// This is a free function rather than a `From` impl because the orphan rule
/// forbids implementing foreign traits (`From`) for foreign types (`PyErr`)
/// when the error type also lives in another crate.
fn session_err_to_py(err: SessionError) -> PyErr {
    match err {
        SessionError::NotFound(msg) => PyRuntimeError::new_err(format!("session not found: {msg}")),
        SessionError::AlreadyExists(msg) => PyRuntimeError::new_err(format!("session already exists: {msg}")),
        SessionError::InvalidState(msg) => PyRuntimeError::new_err(format!("invalid state: {msg}")),
        SessionError::Storage(msg) => PyRuntimeError::new_err(format!("storage error: {msg}")),
        SessionError::InvalidArgument(msg) => PyRuntimeError::new_err(format!("invalid argument: {msg}")),
        SessionError::Repl(e) => PyRuntimeError::new_err(e.to_string()),
    }
}

// =============================================================================
// PyCallbackBackend - bridges StorageBackend to a Python object
// =============================================================================

/// Storage backend that delegates to a Python object's methods.
///
/// The Python object must implement:
/// - `save(name: str, data: bytes) -> None`
/// - `load(name: str) -> bytes`
/// - `list() -> list[dict]` (each dict has "name" and "size_bytes" keys)
/// - `delete(name: str) -> bool`
struct PyCallbackBackend {
    /// The Python backend object (stored as `Py<PyAny>` so it's `Send`).
    inner: pyo3::Py<pyo3::PyAny>,
}

impl std::fmt::Debug for PyCallbackBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyCallbackBackend").finish()
    }
}

impl StorageBackend for PyCallbackBackend {
    fn save(&self, name: &str, data: &[u8]) -> Result<(), String> {
        pyo3::Python::attach(|py| {
            let bytes = PyBytes::new(py, data);
            self.inner
                .call_method1(py, "save", (name, bytes))
                .map_err(|e| format!("Python save() failed: {e}"))?;
            Ok(())
        })
    }

    fn load(&self, name: &str) -> Result<Vec<u8>, String> {
        pyo3::Python::attach(|py| {
            let result = self
                .inner
                .call_method1(py, "load", (name,))
                .map_err(|e| format!("Python load() failed: {e}"))?;
            let bytes: &[u8] = result
                .extract(py)
                .map_err(|e| format!("Python load() must return bytes: {e}"))?;
            Ok(bytes.to_vec())
        })
    }

    fn list(&self) -> Result<Vec<SavedSessionInfo>, String> {
        pyo3::Python::attach(|py| {
            let result = self
                .inner
                .call_method0(py, "list")
                .map_err(|e| format!("Python list() failed: {e}"))?;
            let list: Vec<pyo3::Bound<'_, pyo3::PyAny>> = result
                .extract(py)
                .map_err(|e| format!("Python list() must return a list: {e}"))?;
            let mut sessions = Vec::new();
            for item in list {
                let name: String = item
                    .get_item("name")
                    .map_err(|e| format!("list item missing 'name': {e}"))?
                    .extract()
                    .map_err(|e| format!("'name' must be str: {e}"))?;
                let size_bytes: u64 = item
                    .get_item("size_bytes")
                    .map_err(|e| format!("list item missing 'size_bytes': {e}"))?
                    .extract()
                    .map_err(|e| format!("'size_bytes' must be int: {e}"))?;
                sessions.push(SavedSessionInfo { name, size_bytes });
            }
            Ok(sessions)
        })
    }

    fn delete(&self, name: &str) -> Result<bool, String> {
        pyo3::Python::attach(|py| {
            let result = self
                .inner
                .call_method1(py, "delete", (name,))
                .map_err(|e| format!("Python delete() failed: {e}"))?;
            result
                .extract(py)
                .map_err(|e| format!("Python delete() must return bool: {e}"))
        })
    }
}

// =============================================================================
// PySessionManager
// =============================================================================

/// Multi-session manager for the Ouros interpreter, exposed to Python.
///
/// Wraps `ouros::session_manager::SessionManager` and delegates all operations,
/// converting Rust result types to Python dicts. A "default" session is always
/// present and used when `session_id` is `None`.
///
/// Marked `unsendable` because `ReplSession` contains `Cell` internals.
#[pyclass(name = "SessionManager", module = "ouros", unsendable)]
pub struct PySessionManager {
    /// The underlying Rust session manager.
    inner: SessionManager,
}

#[pymethods]
impl PySessionManager {
    /// Creates a new `SessionManager` with a single "default" session.
    ///
    /// # Arguments
    /// * `script_name` - Name shown in tracebacks and error messages.
    #[new]
    #[pyo3(signature = (*, script_name="session.py"))]
    fn new(script_name: &str) -> Self {
        Self {
            inner: SessionManager::new(script_name),
        }
    }

    // -------------------------------------------------------------------------
    // Execution
    // -------------------------------------------------------------------------

    /// Executes Python code in a session and returns the result dict.
    ///
    /// The result dict contains `progress` (execution state) and `stdout`
    /// (captured print output).
    #[pyo3(signature = (code, *, session_id=None))]
    fn execute<'py>(&mut self, py: Python<'py>, code: &str, session_id: Option<&str>) -> PyResult<Bound<'py, PyDict>> {
        let output = self.inner.execute(session_id, code).map_err(session_err_to_py)?;
        execute_output_to_dict(py, &output)
    }

    /// Resumes execution after an external function call returned a value.
    #[pyo3(signature = (call_id, value, *, session_id=None))]
    fn resume<'py>(
        &mut self,
        py: Python<'py>,
        call_id: u32,
        value: &Bound<'py, PyAny>,
        session_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let ouros_obj = crate::convert::py_to_ouros(value)?;
        let result: ExternalResult = ouros_obj.into();
        let output = self
            .inner
            .resume(session_id, call_id, result)
            .map_err(session_err_to_py)?;
        execute_output_to_dict(py, &output)
    }

    /// Resumes an external call as a pending async future.
    #[pyo3(signature = (call_id, *, session_id=None))]
    fn resume_as_pending<'py>(
        &mut self,
        py: Python<'py>,
        call_id: u32,
        session_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let output = self
            .inner
            .resume_as_pending(session_id, call_id)
            .map_err(session_err_to_py)?;
        execute_output_to_dict(py, &output)
    }

    /// Resumes execution with results for pending async futures.
    ///
    /// `results` is a dict mapping call_id (int) to a dict with either
    /// `return_value` or `exception` key.
    #[pyo3(signature = (results, *, session_id=None))]
    fn resume_futures<'py>(
        &mut self,
        py: Python<'py>,
        results: &Bound<'py, PyDict>,
        session_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let mut pairs = Vec::new();
        for (k, v) in results.iter() {
            let call_id: u32 = k.extract()?;
            let ouros_obj = crate::convert::py_to_ouros(&v)?;
            let ext_result: ExternalResult = ouros_obj.into();
            pairs.push((call_id, ext_result));
        }
        let output = self
            .inner
            .resume_futures(session_id, pairs)
            .map_err(session_err_to_py)?;
        execute_output_to_dict(py, &output)
    }

    // -------------------------------------------------------------------------
    // Variable operations
    // -------------------------------------------------------------------------

    /// Lists defined global variables and their types.
    #[pyo3(signature = (*, session_id=None))]
    fn list_variables<'py>(&self, py: Python<'py>, session_id: Option<&str>) -> PyResult<Bound<'py, PyList>> {
        let vars = self.inner.list_variables(session_id).map_err(session_err_to_py)?;
        let items: Vec<Bound<'py, PyDict>> = vars.into_iter().map(|v| variable_info_to_dict(py, &v)).collect();
        PyList::new(py, items)
    }

    /// Gets one variable's value from a session namespace.
    ///
    /// Returns a dict with `json_value` and `repr` keys.
    #[pyo3(signature = (name, *, session_id=None))]
    fn get_variable<'py>(&self, py: Python<'py>, name: &str, session_id: Option<&str>) -> PyResult<Bound<'py, PyDict>> {
        let val = self.inner.get_variable(session_id, name).map_err(session_err_to_py)?;
        variable_value_to_dict(py, &val)
    }

    /// Sets or creates a global variable by evaluating a Python expression string.
    #[pyo3(signature = (name, value_expr, *, session_id=None))]
    fn set_variable(&mut self, name: &str, value_expr: &str, session_id: Option<&str>) -> PyResult<()> {
        self.inner
            .set_variable(session_id, name, value_expr)
            .map_err(session_err_to_py)?;
        Ok(())
    }

    /// Deletes a global variable. Returns `True` if it existed.
    #[pyo3(signature = (name, *, session_id=None))]
    fn delete_variable(&mut self, name: &str, session_id: Option<&str>) -> PyResult<bool> {
        self.inner.delete_variable(session_id, name).map_err(session_err_to_py)
    }

    /// Transfers a variable from one session to another.
    #[pyo3(signature = (source, target, name, *, target_name=None))]
    fn transfer_variable(&mut self, source: &str, target: &str, name: &str, target_name: Option<&str>) -> PyResult<()> {
        self.inner
            .transfer_variable(source, target, name, target_name)
            .map_err(session_err_to_py)?;
        Ok(())
    }

    /// Evaluates a Python expression without modifying session state.
    ///
    /// Returns a dict with `value` (variable value dict) and `stdout`.
    #[pyo3(signature = (expression, *, session_id=None))]
    fn eval_variable<'py>(
        &mut self,
        py: Python<'py>,
        expression: &str,
        session_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let output = self
            .inner
            .eval_variable(session_id, expression)
            .map_err(session_err_to_py)?;
        eval_output_to_dict(py, &output)
    }

    // -------------------------------------------------------------------------
    // Session lifecycle
    // -------------------------------------------------------------------------

    /// Creates a new named session. Returns the session ID.
    #[pyo3(signature = (id, *, external_functions=None))]
    fn create_session(&mut self, id: &str, external_functions: Option<Vec<String>>) -> PyResult<String> {
        self.inner
            .create_session(id, external_functions.unwrap_or_default())
            .map_err(session_err_to_py)?;
        Ok(id.to_owned())
    }

    /// Destroys a named session.
    fn destroy_session(&mut self, id: &str) -> PyResult<()> {
        self.inner.destroy_session(id).map_err(session_err_to_py)?;
        Ok(())
    }

    /// Lists all active sessions with their variable counts.
    fn list_sessions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let sessions = self.inner.list_sessions();
        let items: Vec<Bound<'py, PyDict>> = sessions.into_iter().map(|s| session_info_to_dict(py, &s)).collect();
        PyList::new(py, items)
    }

    /// Forks an existing session into a new independent copy.
    fn fork_session(&mut self, source: &str, new_id: &str) -> PyResult<()> {
        self.inner.fork_session(source, new_id).map_err(session_err_to_py)?;
        Ok(())
    }

    /// Resets a session to a fresh state.
    #[pyo3(signature = (*, session_id=None, external_functions=None))]
    fn reset(&mut self, session_id: Option<&str>, external_functions: Option<Vec<String>>) -> PyResult<()> {
        self.inner
            .reset(session_id, external_functions.unwrap_or_default())
            .map_err(session_err_to_py)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Persistence
    // -------------------------------------------------------------------------

    /// Configures the directory for session persistence.
    #[expect(clippy::unnecessary_wraps)]
    fn set_storage_dir(&mut self, dir: &str) -> PyResult<()> {
        self.inner.set_storage_dir(dir.into());
        Ok(())
    }

    /// Configures a custom storage backend using a Python object.
    ///
    /// The backend object must implement:
    /// - `save(name: str, data: bytes) -> None`
    /// - `load(name: str) -> bytes`
    /// - `list() -> list[dict]` — each dict has `name` (str) and `size_bytes` (int)
    /// - `delete(name: str) -> bool`
    #[expect(clippy::unnecessary_wraps)]
    fn set_storage_backend(&mut self, backend: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_backend = PyCallbackBackend {
            inner: backend.clone().unbind(),
        };
        self.inner.set_storage_backend(Box::new(py_backend));
        Ok(())
    }

    /// Saves a session to disk as a named snapshot.
    ///
    /// Returns a dict with `name` and `size_bytes`.
    #[pyo3(signature = (*, session_id=None, name=None))]
    fn save_session<'py>(
        &self,
        py: Python<'py>,
        session_id: Option<&str>,
        name: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let result = self.inner.save_session(session_id, name).map_err(session_err_to_py)?;
        save_result_to_dict(py, &result)
    }

    /// Loads a previously saved session from disk.
    ///
    /// Returns the session ID that was created.
    #[pyo3(signature = (name, *, session_id=None))]
    fn load_session(&mut self, name: &str, session_id: Option<&str>) -> PyResult<String> {
        self.inner.load_session(name, session_id).map_err(session_err_to_py)
    }

    /// Lists all saved session snapshots on disk.
    fn list_saved_sessions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let saved = self.inner.list_saved_sessions().map_err(session_err_to_py)?;
        let items: Vec<Bound<'py, PyDict>> = saved.into_iter().map(|s| saved_session_to_dict(py, &s)).collect();
        PyList::new(py, items)
    }

    /// Deletes a saved session snapshot. Returns `True` if it existed.
    fn delete_saved_session(&self, name: &str) -> PyResult<bool> {
        self.inner.delete_saved_session(name).map_err(session_err_to_py)
    }

    // -------------------------------------------------------------------------
    // History / rewind
    // -------------------------------------------------------------------------

    /// Rewinds a session by N steps, restoring a previous state.
    ///
    /// Returns a dict with `steps_rewound` and `history_remaining`.
    #[pyo3(signature = (*, steps=1, session_id=None))]
    fn rewind<'py>(&mut self, py: Python<'py>, steps: usize, session_id: Option<&str>) -> PyResult<Bound<'py, PyDict>> {
        let result = self.inner.rewind(session_id, steps).map_err(session_err_to_py)?;
        rewind_result_to_dict(py, &result)
    }

    /// Returns `(current_depth, max_depth)` for session history.
    #[pyo3(signature = (*, session_id=None))]
    fn history(&self, session_id: Option<&str>) -> PyResult<(usize, usize)> {
        self.inner.history(session_id).map_err(session_err_to_py)
    }

    /// Configures the maximum undo history depth.
    ///
    /// Returns the number of entries trimmed.
    #[pyo3(signature = (max_depth, *, session_id=None))]
    fn set_history_depth(&mut self, max_depth: usize, session_id: Option<&str>) -> PyResult<usize> {
        self.inner
            .set_history_depth(session_id, max_depth)
            .map_err(session_err_to_py)
    }

    // -------------------------------------------------------------------------
    // Heap introspection
    // -------------------------------------------------------------------------

    /// Returns heap statistics for a session as a dict.
    #[pyo3(signature = (*, session_id=None))]
    fn heap_stats<'py>(&self, py: Python<'py>, session_id: Option<&str>) -> PyResult<Bound<'py, PyDict>> {
        let stats = self.inner.heap_stats(session_id).map_err(session_err_to_py)?;
        heap_stats_to_dict(py, &stats)
    }

    /// Saves the current heap stats as a named snapshot for later diff.
    #[pyo3(signature = (name, *, session_id=None))]
    fn snapshot_heap(&mut self, name: &str, session_id: Option<&str>) -> PyResult<()> {
        self.inner.snapshot_heap(session_id, name).map_err(session_err_to_py)?;
        Ok(())
    }

    /// Compares two named heap snapshots and returns the diff.
    fn diff_heap<'py>(&self, py: Python<'py>, before: &str, after: &str) -> PyResult<Bound<'py, PyDict>> {
        let result = self.inner.diff_heap(before, after).map_err(session_err_to_py)?;
        heap_diff_result_to_dict(py, &result)
    }

    // -------------------------------------------------------------------------
    // Cross-session pipeline
    // -------------------------------------------------------------------------

    /// Executes code in the default (or specified source) session and stores
    /// the result in a target session variable.
    #[pyo3(signature = (*, target, code, target_variable, source=None))]
    fn call_session<'py>(
        &mut self,
        py: Python<'py>,
        target: &str,
        code: &str,
        target_variable: &str,
        source: Option<&str>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let output = self
            .inner
            .call_session(source, target, code, target_variable)
            .map_err(session_err_to_py)?;
        execute_output_to_dict(py, &output)
    }
}

// =============================================================================
// Conversion helpers: Rust structs -> Python dicts
// =============================================================================

/// Converts a `serde_json::Value` to a Python object.
///
/// Maps JSON types to their natural Python equivalents: null -> None,
/// bool -> bool, number -> int/float, string -> str, array -> list,
/// object -> dict.
fn json_to_py(py: Python<'_>, value: &JsonValue) -> PyResult<Py<PyAny>> {
    match value {
        JsonValue::Null => Ok(py.None()),
        JsonValue::Bool(b) => Ok(PyBool::new(py, *b).to_owned().into_any().unbind()),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                // u64 that doesn't fit in i64
                Ok(n.as_u64()
                    .expect("serde_json number must be i64, u64, or f64")
                    .into_pyobject(py)?
                    .into_any()
                    .unbind())
            }
        }
        JsonValue::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        JsonValue::Array(arr) => {
            let items: PyResult<Vec<Py<PyAny>>> = arr.iter().map(|v| json_to_py(py, v)).collect();
            Ok(PyList::new(py, items?)?.into_any().unbind())
        }
        JsonValue::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Converts an `ExecuteOutput` to a Python dict with `progress` and `stdout` keys.
fn execute_output_to_dict<'py>(py: Python<'py>, output: &ExecuteOutput) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("stdout", &output.stdout)?;
    dict.set_item("progress", progress_to_dict(py, &output.progress)?)?;
    dict.set_item("is_complete", matches!(output.progress, ReplProgress::Complete(_)))?;

    // Convenience: if complete, include the result value directly
    if let ReplProgress::Complete(ref obj) = output.progress {
        let json_val = obj.to_json_value();
        dict.set_item("result", json_to_py(py, &json_val)?)?;
    }

    Ok(dict)
}

/// Converts a `ReplProgress` to a Python dict summarizing the execution state.
fn progress_to_dict<'py>(py: Python<'py>, progress: &ReplProgress) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    match progress {
        ReplProgress::Complete(obj) => {
            dict.set_item("status", "complete")?;
            let json_val = obj.to_json_value();
            dict.set_item("result", json_to_py(py, &json_val)?)?;
        }
        ReplProgress::FunctionCall {
            function_name, call_id, ..
        } => {
            dict.set_item("status", "function_call")?;
            dict.set_item("function_name", function_name)?;
            dict.set_item("call_id", call_id)?;
        }
        ReplProgress::ProxyCall {
            proxy_id,
            method,
            call_id,
            ..
        } => {
            dict.set_item("status", "proxy_call")?;
            dict.set_item("proxy_id", proxy_id)?;
            dict.set_item("method", method)?;
            dict.set_item("call_id", call_id)?;
        }
        ReplProgress::ResolveFutures { pending_call_ids, .. } => {
            dict.set_item("status", "resolve_futures")?;
            dict.set_item("pending_call_ids", pending_call_ids)?;
        }
    }
    Ok(dict)
}

/// Converts a `VariableInfo` to a Python dict.
fn variable_info_to_dict<'py>(py: Python<'py>, info: &VariableInfo) -> Bound<'py, PyDict> {
    let dict = PyDict::new(py);
    dict.set_item("name", &info.name).expect("set_item failed");
    dict.set_item("type_name", &info.type_name).expect("set_item failed");
    dict
}

/// Converts a `VariableValue` to a Python dict with `json_value` and `repr` keys.
fn variable_value_to_dict<'py>(py: Python<'py>, val: &VariableValue) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("json_value", json_to_py(py, &val.json_value)?)?;
    dict.set_item("repr", &val.repr)?;
    Ok(dict)
}

/// Converts a `SessionInfo` to a Python dict.
fn session_info_to_dict<'py>(py: Python<'py>, info: &SessionInfo) -> Bound<'py, PyDict> {
    let dict = PyDict::new(py);
    dict.set_item("id", &info.id).expect("set_item failed");
    dict.set_item("variable_count", info.variable_count)
        .expect("set_item failed");
    dict
}

/// Converts an `EvalOutput` to a Python dict.
fn eval_output_to_dict<'py>(py: Python<'py>, output: &EvalOutput) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("value", variable_value_to_dict(py, &output.value)?)?;
    dict.set_item("stdout", &output.stdout)?;
    Ok(dict)
}

/// Converts a `RewindResult` to a Python dict.
fn rewind_result_to_dict<'py>(py: Python<'py>, result: &RewindResult) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("steps_rewound", result.steps_rewound)?;
    dict.set_item("history_remaining", result.history_remaining)?;
    Ok(dict)
}

/// Converts a `SaveResult` to a Python dict.
fn save_result_to_dict<'py>(py: Python<'py>, result: &SaveResult) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &result.name)?;
    dict.set_item("size_bytes", result.size_bytes)?;
    Ok(dict)
}

/// Converts a `SavedSessionInfo` to a Python dict.
fn saved_session_to_dict<'py>(py: Python<'py>, info: &SavedSessionInfo) -> Bound<'py, PyDict> {
    let dict = PyDict::new(py);
    dict.set_item("name", &info.name).expect("set_item failed");
    dict.set_item("size_bytes", info.size_bytes).expect("set_item failed");
    dict
}

/// Converts a `HeapStats` to a Python dict.
fn heap_stats_to_dict<'py>(py: Python<'py>, stats: &HeapStats) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("live_objects", stats.live_objects)?;
    dict.set_item("free_slots", stats.free_slots)?;
    dict.set_item("total_slots", stats.total_slots)?;
    dict.set_item("interned_strings", stats.interned_strings)?;
    dict.set_item("tracker_allocations", stats.tracker_allocations)?;
    dict.set_item("tracker_memory_bytes", stats.tracker_memory_bytes)?;

    let by_type = PyDict::new(py);
    for (type_name, count) in &stats.objects_by_type {
        by_type.set_item(*type_name, count)?;
    }
    dict.set_item("objects_by_type", by_type)?;
    Ok(dict)
}

/// Converts a `HeapDiffResult` to a Python dict.
fn heap_diff_result_to_dict<'py>(py: Python<'py>, result: &HeapDiffResult) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("heap_diff", heap_diff_to_dict(py, &result.heap_diff)?)?;
    dict.set_item("variable_diff", variable_diff_to_dict(py, &result.variable_diff)?)?;
    Ok(dict)
}

/// Converts a `HeapDiff` to a Python dict.
fn heap_diff_to_dict<'py>(py: Python<'py>, diff: &HeapDiff) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("live_objects_delta", diff.live_objects_delta)?;
    dict.set_item("free_slots_delta", diff.free_slots_delta)?;
    dict.set_item("total_slots_delta", diff.total_slots_delta)?;
    dict.set_item("interned_strings_delta", diff.interned_strings_delta)?;

    let by_type = PyDict::new(py);
    for (type_name, delta) in &diff.objects_by_type_delta {
        by_type.set_item(*type_name, delta)?;
    }
    dict.set_item("objects_by_type_delta", by_type)?;
    dict.set_item("new_types", &diff.new_types)?;
    dict.set_item("removed_types", &diff.removed_types)?;
    Ok(dict)
}

/// Converts a `VariableDiff` to a Python dict.
fn variable_diff_to_dict<'py>(py: Python<'py>, diff: &VariableDiff) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("added", &diff.added)?;
    dict.set_item("removed", &diff.removed)?;
    dict.set_item("unchanged", &diff.unchanged)?;

    let changed_list: Vec<Bound<'py, PyDict>> = diff.changed.iter().map(|c| changed_variable_to_dict(py, c)).collect();
    dict.set_item("changed", changed_list)?;
    Ok(dict)
}

/// Converts a `ChangedVariable` to a Python dict.
fn changed_variable_to_dict<'py>(py: Python<'py>, c: &ChangedVariable) -> Bound<'py, PyDict> {
    let dict = PyDict::new(py);
    dict.set_item("name", &c.name).expect("set_item failed");
    dict.set_item("before", &c.before).expect("set_item failed");
    dict.set_item("after", &c.after).expect("set_item failed");
    dict
}
