//! napi-rs bindings for `SessionManager` -- multi-session Python interpreter management.
//!
//! Wraps [`ouros::session_manager::SessionManager`] in a napi class (JS name: `SessionManager`)
//! so that TypeScript/JavaScript users can manage multiple named interpreter sessions,
//! execute code, inspect variables, fork/rewind, snapshot heaps, and persist sessions
//! without going through the MCP protocol layer.
//!
//! All `SessionError` variants from the core are converted to napi `Error` so they
//! propagate as JavaScript exceptions with descriptive messages.
//!
//! ## Architecture
//!
//! - `NapiSessionManager` holds the core `SessionManager` and delegates every method.
//! - Output types (`NapiExecuteResult`, `NapiVariableInfo`, etc.) are `#[napi(object)]`
//!   structs that napi-rs auto-generates TypeScript declarations for.
//! - JSON variable values (`serde_json::Value`) are returned as stringified JSON and
//!   parsed on the TypeScript side for simplicity and type safety.

use std::path::PathBuf;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use ouros::{
    session_manager::{
        EvalOutput, ExecuteOutput, HeapDiffResult, RewindResult, SaveResult, SavedSessionInfo, SessionError,
        SessionInfo, SessionManager, VariableInfo, VariableValue,
    },
    ExcType, Exception, ExternalResult, Object as OurosObject, ReplProgress,
};

// =============================================================================
// Error conversion
// =============================================================================

/// Converts a `SessionError` to a napi `Error` with a descriptive message.
fn session_error_to_napi(err: SessionError) -> napi::Error {
    napi::Error::from_reason(err.to_string())
}

// =============================================================================
// Output types -- napi(object) structs auto-generate TypeScript declarations
// =============================================================================

/// Result of executing code in a session.
///
/// Contains the execution output (stdout, result value, completion status)
/// and progress state for external function calls.
#[napi(object)]
pub struct NapiExecuteResult {
    /// Standard output captured during execution.
    pub stdout: String,
    /// Whether execution completed (true) or paused at an external call (false).
    pub is_complete: bool,
    /// The result value as JSON string, if execution completed with a value.
    /// `null` for None results or non-complete progress.
    pub result_json: Option<String>,
    /// For FunctionCall progress: the external function name.
    pub function_name: Option<String>,
    /// For FunctionCall progress: the call ID for resume.
    pub call_id: Option<u32>,
    /// For FunctionCall progress: positional args as JSON array string.
    pub args_json: Option<String>,
    /// For ResolveFutures progress: pending call IDs.
    pub pending_call_ids: Option<Vec<u32>>,
}

/// Info about a single variable in a session.
#[napi(object)]
pub struct NapiVariableInfo {
    /// Variable name.
    pub name: String,
    /// Python type name (e.g. "int", "str", "list").
    pub type_name: String,
}

/// A variable's value with JSON and repr representations.
#[napi(object)]
pub struct NapiVariableValue {
    /// JSON representation of the value (as a JSON string).
    pub json_value: String,
    /// Python `repr()` string, if available.
    pub repr: Option<String>,
}

/// Result of evaluating an expression without side effects.
#[napi(object)]
pub struct NapiEvalResult {
    /// The computed value.
    pub value: NapiVariableValue,
    /// Standard output captured during evaluation.
    pub stdout: String,
}

/// Summary info for one active session.
#[napi(object)]
pub struct NapiSessionInfo {
    /// The session ID.
    pub id: String,
    /// Number of defined global variables.
    pub variable_count: u32,
}

/// Result of a rewind operation.
#[napi(object)]
pub struct NapiRewindResult {
    /// Number of steps actually rewound.
    pub steps_rewound: u32,
    /// Number of history entries remaining after rewind.
    pub history_remaining: u32,
}

/// History depth info for a session.
#[napi(object)]
pub struct NapiHistoryInfo {
    /// Current undo history depth.
    pub current: u32,
    /// Maximum configured history depth.
    pub max: u32,
}

/// Heap statistics for a session.
#[napi(object)]
pub struct NapiHeapStats {
    /// Total number of live objects on the heap.
    pub live_objects: u32,
    /// Number of free (recycled) slots.
    pub free_slots: u32,
    /// Total heap capacity (live + free).
    pub total_slots: u32,
    /// Number of interned strings.
    pub interned_strings: u32,
}

/// Result of a heap diff operation.
#[napi(object)]
pub struct NapiHeapDiffResult {
    /// Aggregate heap counter deltas.
    pub heap_diff: NapiHeapDiff,
    /// Variable-level changes between two states.
    pub variable_diff: NapiVariableDiff,
}

/// Aggregate heap counter deltas between two snapshots.
///
/// All fields represent signed deltas (after - before). Positive values
/// mean growth, negative values mean shrinkage.
#[napi(object)]
#[expect(
    clippy::struct_field_names,
    reason = "all fields represent deltas; the _delta suffix is intentional for clarity"
)]
pub struct NapiHeapDiff {
    /// Change in live object count.
    pub live_objects_delta: i32,
    /// Change in free slot count.
    pub free_slots_delta: i32,
    /// Change in total slot count.
    pub total_slots_delta: i32,
    /// Change in interned string count.
    pub interned_strings_delta: i32,
}

/// Variable-level diff summary between two heap states.
#[napi(object)]
pub struct NapiVariableDiff {
    /// Variables present only in the "after" state.
    pub added: Vec<String>,
    /// Variables present only in the "before" state.
    pub removed: Vec<String>,
    /// Variables whose repr changed between states.
    pub changed: Vec<NapiChangedVariable>,
    /// Variables with identical repr in both states.
    pub unchanged: Vec<String>,
}

/// One variable that changed between two heap snapshots.
#[napi(object)]
pub struct NapiChangedVariable {
    /// Variable name.
    pub name: String,
    /// Repr string in the "before" state.
    pub before: String,
    /// Repr string in the "after" state.
    pub after: String,
}

/// Result of a save operation.
#[napi(object)]
pub struct NapiSaveResult {
    /// The snapshot name used for the save.
    pub name: String,
    /// Size of the saved snapshot in bytes.
    pub size_bytes: u32,
}

/// Info about a saved session snapshot on disk.
#[napi(object)]
pub struct NapiSavedSessionInfo {
    /// Snapshot name (filename without extension).
    pub name: String,
    /// Size of the snapshot file in bytes.
    pub size_bytes: u32,
}

// =============================================================================
// Conversion helpers
// =============================================================================

/// Converts `ExecuteOutput` (core) to `NapiExecuteResult` (napi).
fn execute_output_to_napi(output: ExecuteOutput) -> NapiExecuteResult {
    match output.progress {
        ReplProgress::Complete(ref obj) => {
            let result_json = if matches!(obj, OurosObject::None) {
                None
            } else {
                Some(obj.to_json_value().to_string())
            };
            NapiExecuteResult {
                stdout: output.stdout,
                is_complete: true,
                result_json,
                function_name: None,
                call_id: None,
                args_json: None,
                pending_call_ids: None,
            }
        }
        ReplProgress::FunctionCall {
            function_name,
            args,
            call_id,
            ..
        } => {
            let args_json =
                serde_json::to_string(&args.iter().map(OurosObject::to_json_value).collect::<Vec<_>>()).ok();
            NapiExecuteResult {
                stdout: output.stdout,
                is_complete: false,
                result_json: None,
                function_name: Some(function_name),
                call_id: Some(call_id),
                args_json,
                pending_call_ids: None,
            }
        }
        ReplProgress::ProxyCall {
            method, call_id, args, ..
        } => {
            let args_json =
                serde_json::to_string(&args.iter().map(OurosObject::to_json_value).collect::<Vec<_>>()).ok();
            NapiExecuteResult {
                stdout: output.stdout,
                is_complete: false,
                result_json: None,
                function_name: Some(method),
                call_id: Some(call_id),
                args_json,
                pending_call_ids: None,
            }
        }
        ReplProgress::ResolveFutures { pending_call_ids, .. } => NapiExecuteResult {
            stdout: output.stdout,
            is_complete: false,
            result_json: None,
            function_name: None,
            call_id: None,
            args_json: None,
            pending_call_ids: Some(pending_call_ids),
        },
    }
}

/// Converts `VariableInfo` (core) to `NapiVariableInfo` (napi).
fn variable_info_to_napi(info: VariableInfo) -> NapiVariableInfo {
    NapiVariableInfo {
        name: info.name,
        type_name: info.type_name,
    }
}

/// Converts `VariableValue` (core) to `NapiVariableValue` (napi).
fn variable_value_to_napi(val: VariableValue) -> NapiVariableValue {
    NapiVariableValue {
        json_value: val.json_value.to_string(),
        repr: val.repr,
    }
}

/// Converts `EvalOutput` (core) to `NapiEvalResult` (napi).
fn eval_output_to_napi(output: EvalOutput) -> NapiEvalResult {
    NapiEvalResult {
        value: variable_value_to_napi(output.value),
        stdout: output.stdout,
    }
}

/// Converts `SessionInfo` (core) to `NapiSessionInfo` (napi).
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; session variable counts won't exceed u32::MAX"
)]
fn session_info_to_napi(info: SessionInfo) -> NapiSessionInfo {
    NapiSessionInfo {
        id: info.id,
        variable_count: info.variable_count as u32,
    }
}

/// Converts `RewindResult` (core) to `NapiRewindResult` (napi).
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; history depth won't exceed u32::MAX"
)]
fn rewind_result_to_napi(result: RewindResult) -> NapiRewindResult {
    NapiRewindResult {
        steps_rewound: result.steps_rewound as u32,
        history_remaining: result.history_remaining as u32,
    }
}

/// Converts `SaveResult` (core) to `NapiSaveResult` (napi).
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; snapshot size safely fits u32"
)]
fn save_result_to_napi(result: SaveResult) -> NapiSaveResult {
    NapiSaveResult {
        name: result.name,
        size_bytes: result.size_bytes as u32,
    }
}

/// Converts `SavedSessionInfo` (core) to `NapiSavedSessionInfo` (napi).
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; file size safely fits u32"
)]
fn saved_session_info_to_napi(info: SavedSessionInfo) -> NapiSavedSessionInfo {
    NapiSavedSessionInfo {
        name: info.name,
        size_bytes: info.size_bytes as u32,
    }
}

/// Converts `HeapDiffResult` (core) to `NapiHeapDiffResult` (napi).
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires i32; heap deltas won't exceed i32::MAX"
)]
fn heap_diff_result_to_napi(result: HeapDiffResult) -> NapiHeapDiffResult {
    NapiHeapDiffResult {
        heap_diff: NapiHeapDiff {
            live_objects_delta: result.heap_diff.live_objects_delta as i32,
            free_slots_delta: result.heap_diff.free_slots_delta as i32,
            total_slots_delta: result.heap_diff.total_slots_delta as i32,
            interned_strings_delta: result.heap_diff.interned_strings_delta as i32,
        },
        variable_diff: NapiVariableDiff {
            added: result.variable_diff.added,
            removed: result.variable_diff.removed,
            changed: result
                .variable_diff
                .changed
                .into_iter()
                .map(|c| NapiChangedVariable {
                    name: c.name,
                    before: c.before,
                    after: c.after,
                })
                .collect(),
            unchanged: result.variable_diff.unchanged,
        },
    }
}

// =============================================================================
// NapiSessionManager - Main napi class
// =============================================================================

/// Multi-session Python interpreter manager for JavaScript/TypeScript.
///
/// Wraps the core `SessionManager` to provide napi-compatible methods for
/// session lifecycle, code execution, variable management, history/rewind,
/// heap introspection, and session persistence.
///
/// A "default" session is always present and is used when no session ID is
/// specified.
#[napi(js_name = "SessionManager")]
pub struct NapiSessionManager {
    /// The underlying core session manager.
    inner: SessionManager,
}

#[napi]
impl NapiSessionManager {
    /// Creates a new session manager with an optional script name and storage directory.
    ///
    /// The script name appears in tracebacks and error messages. Defaults to "session.py".
    /// If `storage_dir` is provided, session persistence (save/load) is enabled.
    #[napi(constructor)]
    #[must_use]
    pub fn new(script_name: Option<String>, storage_dir: Option<String>) -> Self {
        let name = script_name.as_deref().unwrap_or("session.py");
        let mut mgr = SessionManager::new(name);
        if let Some(dir) = storage_dir {
            mgr.set_storage_dir(PathBuf::from(dir));
        }
        Self { inner: mgr }
    }

    // =========================================================================
    // Execution
    // =========================================================================

    /// Executes Python code in a session.
    ///
    /// Pass `null`/`undefined` for `sessionId` to use the default session.
    #[napi]
    pub fn execute(&mut self, code: String, session_id: Option<String>) -> Result<NapiExecuteResult> {
        let result = self
            .inner
            .execute(session_id.as_deref(), &code)
            .map_err(session_error_to_napi)?;
        Ok(execute_output_to_napi(result))
    }

    /// Resumes execution after an external function call with a return value.
    ///
    /// The `value_json` is parsed as JSON and converted to an `OurosObject`.
    #[napi]
    pub fn resume_with_value(
        &mut self,
        session_id: Option<String>,
        call_id: u32,
        value_json: String,
    ) -> Result<NapiExecuteResult> {
        let json_value: serde_json::Value =
            serde_json::from_str(&value_json).map_err(|e| napi::Error::from_reason(format!("invalid JSON: {e}")))?;
        let obj = OurosObject::from_json_value(json_value);
        let result = self
            .inner
            .resume(session_id.as_deref(), call_id, ExternalResult::Return(obj))
            .map_err(session_error_to_napi)?;
        Ok(execute_output_to_napi(result))
    }

    /// Resumes execution after an external function call with an exception.
    #[napi]
    pub fn resume_with_exception(
        &mut self,
        session_id: Option<String>,
        call_id: u32,
        exc_type: String,
        exc_message: String,
    ) -> Result<NapiExecuteResult> {
        let exc_type_parsed = string_to_exc_type(&exc_type)?;
        let exc = Exception::new(exc_type_parsed, Some(exc_message));
        let result = self
            .inner
            .resume(session_id.as_deref(), call_id, ExternalResult::Error(exc))
            .map_err(session_error_to_napi)?;
        Ok(execute_output_to_napi(result))
    }

    /// Resumes an external call as a pending async future.
    #[napi]
    pub fn resume_as_pending(&mut self, session_id: Option<String>, call_id: u32) -> Result<NapiExecuteResult> {
        let result = self
            .inner
            .resume_as_pending(session_id.as_deref(), call_id)
            .map_err(session_error_to_napi)?;
        Ok(execute_output_to_napi(result))
    }

    /// Resumes execution with results for pending async futures.
    ///
    /// Each entry maps a `call_id` to either a return value JSON or an exception.
    #[napi]
    pub fn resume_futures(
        &mut self,
        session_id: Option<String>,
        results: Vec<NapiFutureResult>,
    ) -> Result<NapiExecuteResult> {
        let mut pairs: Vec<(u32, ExternalResult)> = Vec::with_capacity(results.len());
        for r in results {
            let ext_result = if let Some(exc_type) = r.exception_type {
                let exc_type_parsed = string_to_exc_type(&exc_type)?;
                let exc = Exception::new(exc_type_parsed, Some(r.exception_message.unwrap_or_default()));
                ExternalResult::Error(exc)
            } else if let Some(json) = r.return_value_json {
                let json_value: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                let obj = OurosObject::from_json_value(json_value);
                ExternalResult::Return(obj)
            } else {
                ExternalResult::Return(OurosObject::None)
            };
            pairs.push((r.call_id, ext_result));
        }

        let result = self
            .inner
            .resume_futures(session_id.as_deref(), pairs)
            .map_err(session_error_to_napi)?;
        Ok(execute_output_to_napi(result))
    }

    // =========================================================================
    // Variable operations
    // =========================================================================

    /// Lists defined global variables and their types in a session.
    #[napi]
    pub fn list_variables(&self, session_id: Option<String>) -> Result<Vec<NapiVariableInfo>> {
        let vars = self
            .inner
            .list_variables(session_id.as_deref())
            .map_err(session_error_to_napi)?;
        Ok(vars.into_iter().map(variable_info_to_napi).collect())
    }

    /// Gets one variable's value from a session namespace.
    #[napi]
    pub fn get_variable(&self, session_id: Option<String>, name: String) -> Result<NapiVariableValue> {
        let val = self
            .inner
            .get_variable(session_id.as_deref(), &name)
            .map_err(session_error_to_napi)?;
        Ok(variable_value_to_napi(val))
    }

    /// Sets or creates a global variable by evaluating a Python expression.
    #[napi]
    pub fn set_variable(&mut self, session_id: Option<String>, name: String, value_expr: String) -> Result<()> {
        self.inner
            .set_variable(session_id.as_deref(), &name, &value_expr)
            .map_err(session_error_to_napi)
    }

    /// Deletes a global variable from a session. Returns true if the variable existed.
    #[napi]
    pub fn delete_variable(&mut self, session_id: Option<String>, name: String) -> Result<bool> {
        self.inner
            .delete_variable(session_id.as_deref(), &name)
            .map_err(session_error_to_napi)
    }

    /// Transfers a variable from one session to another.
    #[napi]
    pub fn transfer_variable(
        &mut self,
        source: String,
        target: String,
        name: String,
        target_name: Option<String>,
    ) -> Result<()> {
        self.inner
            .transfer_variable(&source, &target, &name, target_name.as_deref())
            .map_err(session_error_to_napi)
    }

    /// Evaluates a Python expression without modifying session state.
    #[napi]
    pub fn eval_variable(&mut self, session_id: Option<String>, expression: String) -> Result<NapiEvalResult> {
        let output = self
            .inner
            .eval_variable(session_id.as_deref(), &expression)
            .map_err(session_error_to_napi)?;
        Ok(eval_output_to_napi(output))
    }

    // =========================================================================
    // Session lifecycle
    // =========================================================================

    /// Creates a new named session.
    #[napi]
    pub fn create_session(&mut self, id: String, external_functions: Option<Vec<String>>) -> Result<()> {
        self.inner
            .create_session(&id, external_functions.unwrap_or_default())
            .map_err(session_error_to_napi)
    }

    /// Destroys a named session. The default session cannot be destroyed.
    #[napi]
    pub fn destroy_session(&mut self, id: String) -> Result<()> {
        self.inner.destroy_session(&id).map_err(session_error_to_napi)
    }

    /// Lists all active sessions with their variable counts.
    #[napi]
    pub fn list_sessions(&self) -> Vec<NapiSessionInfo> {
        self.inner
            .list_sessions()
            .into_iter()
            .map(session_info_to_napi)
            .collect()
    }

    /// Forks an existing session into a new independent copy.
    #[napi]
    pub fn fork_session(&mut self, source: String, new_id: String) -> Result<()> {
        self.inner.fork_session(&source, &new_id).map_err(session_error_to_napi)
    }

    /// Resets a session, replacing it with a fresh interpreter instance.
    #[napi]
    pub fn reset(&mut self, session_id: Option<String>, external_functions: Option<Vec<String>>) -> Result<()> {
        self.inner
            .reset(session_id.as_deref(), external_functions.unwrap_or_default())
            .map_err(session_error_to_napi)
    }

    // =========================================================================
    // Persistence
    // =========================================================================

    /// Saves a session to disk as a named snapshot.
    #[napi]
    pub fn save_session(&self, session_id: Option<String>, name: Option<String>) -> Result<NapiSaveResult> {
        let result = self
            .inner
            .save_session(session_id.as_deref(), name.as_deref())
            .map_err(session_error_to_napi)?;
        Ok(save_result_to_napi(result))
    }

    /// Loads a previously saved session from disk.
    ///
    /// Returns the session ID that was created.
    #[napi]
    pub fn load_session(&mut self, name: String, session_id: Option<String>) -> Result<String> {
        self.inner
            .load_session(&name, session_id.as_deref())
            .map_err(session_error_to_napi)
    }

    /// Lists all saved session snapshots on disk.
    #[napi]
    pub fn list_saved_sessions(&self) -> Result<Vec<NapiSavedSessionInfo>> {
        let sessions = self.inner.list_saved_sessions().map_err(session_error_to_napi)?;
        Ok(sessions.into_iter().map(saved_session_info_to_napi).collect())
    }

    // =========================================================================
    // History / rewind
    // =========================================================================

    /// Rewinds a session by N steps, restoring it to a previous state.
    #[napi]
    pub fn rewind(&mut self, session_id: Option<String>, steps: u32) -> Result<NapiRewindResult> {
        let result = self
            .inner
            .rewind(session_id.as_deref(), steps as usize)
            .map_err(session_error_to_napi)?;
        Ok(rewind_result_to_napi(result))
    }

    /// Returns the current undo history depth and configured maximum for a session.
    #[napi]
    pub fn history(&self, session_id: Option<String>) -> Result<NapiHistoryInfo> {
        let (current, max) = self
            .inner
            .history(session_id.as_deref())
            .map_err(session_error_to_napi)?;
        Ok(history_to_napi(current, max))
    }

    /// Configures the maximum undo history depth for a session.
    ///
    /// Returns the number of entries that were trimmed.
    #[napi]
    pub fn set_history_depth(&mut self, session_id: Option<String>, max_depth: u32) -> Result<u32> {
        let trimmed = self
            .inner
            .set_history_depth(session_id.as_deref(), max_depth as usize)
            .map_err(session_error_to_napi)?;
        Ok(usize_to_u32(trimmed))
    }

    // =========================================================================
    // Heap introspection
    // =========================================================================

    /// Returns heap statistics for a session.
    #[napi]
    pub fn heap_stats(&self, session_id: Option<String>) -> Result<NapiHeapStats> {
        let stats = self
            .inner
            .heap_stats(session_id.as_deref())
            .map_err(session_error_to_napi)?;
        Ok(heap_stats_to_napi(stats))
    }

    /// Saves the current heap stats as a named snapshot for later diff.
    #[napi]
    pub fn snapshot_heap(&mut self, session_id: Option<String>, name: String) -> Result<()> {
        self.inner
            .snapshot_heap(session_id.as_deref(), &name)
            .map_err(session_error_to_napi)
    }

    /// Compares two named heap snapshots and returns the diff.
    #[napi]
    pub fn diff_heap(&self, before: String, after: String) -> Result<NapiHeapDiffResult> {
        let result = self.inner.diff_heap(&before, &after).map_err(session_error_to_napi)?;
        Ok(heap_diff_result_to_napi(result))
    }

    // =========================================================================
    // Cross-session pipeline
    // =========================================================================

    /// Executes code in a source session and stores the result in a target session.
    #[napi]
    pub fn call_session(
        &mut self,
        source: Option<String>,
        target: String,
        code: String,
        target_variable: String,
    ) -> Result<NapiExecuteResult> {
        let result = self
            .inner
            .call_session(source.as_deref(), &target, &code, &target_variable)
            .map_err(session_error_to_napi)?;
        Ok(execute_output_to_napi(result))
    }
}

/// Converts history depth pair to napi output.
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; history depth won't exceed u32::MAX"
)]
fn history_to_napi(current: usize, max: usize) -> NapiHistoryInfo {
    NapiHistoryInfo {
        current: current as u32,
        max: max as u32,
    }
}

/// Converts heap stats from core to napi output.
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; heap stats won't exceed u32::MAX"
)]
fn heap_stats_to_napi(stats: ouros::HeapStats) -> NapiHeapStats {
    NapiHeapStats {
        live_objects: stats.live_objects as u32,
        free_slots: stats.free_slots as u32,
        total_slots: stats.total_slots as u32,
        interned_strings: stats.interned_strings as u32,
    }
}

/// Truncates a `usize` to `u32` for napi compatibility.
///
/// In practice, values passed through this function (trimmed history count, etc.)
/// will never approach `u32::MAX`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "napi requires u32; values are small counts"
)]
fn usize_to_u32(value: usize) -> u32 {
    value as u32
}

// =============================================================================
// Private helpers
// =============================================================================

/// Parses an exception type name string into an `ExcType` enum variant.
fn string_to_exc_type(type_name: &str) -> Result<ExcType> {
    type_name
        .parse()
        .map_err(|_| Error::from_reason(format!("Invalid exception type: '{type_name}'")))
}

// =============================================================================
// Input types for resume_futures
// =============================================================================

/// Input for resuming a single future with either a value or exception.
#[napi(object)]
pub struct NapiFutureResult {
    /// The call ID of the future to resolve.
    pub call_id: u32,
    /// JSON string of the return value (mutually exclusive with exception fields).
    pub return_value_json: Option<String>,
    /// Exception type name (mutually exclusive with return_value_json).
    pub exception_type: Option<String>,
    /// Exception message (used with exception_type).
    pub exception_message: Option<String>,
}
