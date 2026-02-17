//! Multi-session manager for the Ouros interpreter.
//!
//! `SessionManager` wraps a registry of named [`ReplSession`] instances and
//! provides a typed, JSON-free API for session lifecycle management, code
//! execution, variable operations, undo history, heap introspection, and
//! session persistence.
//!
//! A "default" session is always present and is used when callers pass `None`
//! as the session ID. This provides a clean upgrade path from single-session
//! usage to multi-session orchestration.
//!
//! This module is the pure-logic core that the MCP handler (in `ouros-mcp`)
//! delegates to. It contains no JSON serialization or MCP protocol concerns.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fmt, fs,
    path::PathBuf,
};

use crate::{
    CollectStringPrint, ExternalResult, HeapDiff, HeapStats, Object, ReplError, ReplProgress, ReplSession,
    ResourceLimits,
};

/// The name of the session that is always present and cannot be destroyed.
const DEFAULT_SESSION_ID: &str = "default";
/// Default VM operation budget per execute/resume step.
const DEFAULT_MAX_OPERATIONS: usize = 10_000_000;
/// Default allocation budget per session heap.
const DEFAULT_MAX_ALLOCATIONS: usize = 1_000_000;
/// Default memory cap per session heap (256 MiB).
const DEFAULT_MAX_MEMORY_BYTES: usize = 256 * 1024 * 1024;
/// Default maximum undo history depth per session.
///
/// After each successful `execute` or `resume` call, the pre-execution session
/// state is pushed onto a history stack. When the stack exceeds this limit,
/// the oldest entries are dropped from the front.
const DEFAULT_MAX_HISTORY: usize = 20;

// =============================================================================
// Error types
// =============================================================================

/// Errors that can occur during session management operations.
///
/// Separates domain-level failures (not found, already exists, invalid state)
/// from interpreter errors (`Repl`) and I/O errors (`Storage`). This lets
/// callers pattern-match on the failure category without string parsing.
#[derive(Debug, Clone)]
pub enum SessionError {
    /// An interpreter error from the underlying `ReplSession`.
    Repl(ReplError),
    /// The requested session was not found.
    NotFound(String),
    /// A session with the given ID already exists.
    AlreadyExists(String),
    /// The operation is invalid in the current state (e.g. destroying default,
    /// rewinding past available history, pending call mismatch).
    InvalidState(String),
    /// A storage/filesystem operation failed.
    Storage(String),
    /// An argument was invalid (e.g. zero rewind steps, bad snapshot name).
    InvalidArgument(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repl(e) => write!(f, "{e}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::AlreadyExists(msg) => write!(f, "already exists: {msg}"),
            Self::InvalidState(msg) => write!(f, "invalid state: {msg}"),
            Self::Storage(msg) => write!(f, "storage error: {msg}"),
            Self::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ReplError> for SessionError {
    fn from(error: ReplError) -> Self {
        Self::Repl(error)
    }
}

// =============================================================================
// Output types
// =============================================================================

/// Output from an `execute`, `resume`, or `call_session` operation.
///
/// Bundles the execution progress (complete, function call, proxy call, or
/// resolve futures) with any stdout output captured during execution.
#[derive(Debug, Clone)]
pub struct ExecuteOutput {
    /// The execution progress -- either complete with a result or paused
    /// waiting for external input.
    pub progress: ReplProgress,
    /// Standard output captured during execution (may be empty).
    pub stdout: String,
}

/// Summary info for one active session, as returned by `list_sessions`.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// The session ID.
    pub id: String,
    /// Number of defined global variables.
    pub variable_count: usize,
}

/// Info about a single variable in a session.
#[derive(Debug, Clone)]
pub struct VariableInfo {
    /// Variable name.
    pub name: String,
    /// Python type name (e.g. "int", "str", "list").
    pub type_name: String,
}

/// A variable's value with its JSON representation and optional repr string.
#[derive(Debug, Clone)]
pub struct VariableValue {
    /// JSON-compatible representation of the value.
    pub json_value: serde_json::Value,
    /// Python `repr()` string, if available.
    pub repr: Option<String>,
}

/// Result of an `eval_variable` operation.
///
/// Bundles the computed value with any stdout captured during evaluation.
#[derive(Debug, Clone)]
pub struct EvalOutput {
    /// The value computed by the expression.
    pub value: VariableValue,
    /// Standard output captured during evaluation (may be empty).
    pub stdout: String,
}

/// Result of a rewind operation.
#[derive(Debug, Clone)]
pub struct RewindResult {
    /// Number of steps actually rewound.
    pub steps_rewound: usize,
    /// Number of history entries remaining after rewind.
    pub history_remaining: usize,
}

/// Result of a save operation.
#[derive(Debug, Clone)]
pub struct SaveResult {
    /// The snapshot name used for the save.
    pub name: String,
    /// Size of the saved snapshot in bytes.
    pub size_bytes: usize,
}

/// Info about a saved session snapshot on disk.
#[derive(Debug, Clone)]
pub struct SavedSessionInfo {
    /// Snapshot name (filename without extension).
    pub name: String,
    /// Size of the snapshot file in bytes.
    pub size_bytes: u64,
}

/// Result of a heap diff operation.
///
/// Bundles the raw `HeapDiff` with optional variable-level change details.
#[derive(Debug, Clone)]
pub struct HeapDiffResult {
    /// Aggregate heap counter deltas.
    pub heap_diff: HeapDiff,
    /// Variable-level changes between the two states.
    pub variable_diff: VariableDiff,
}

/// Variable-level diff summary between two heap states.
///
/// Categorizes variables into added, removed, changed, and unchanged groups
/// for easy consumption by callers.
#[derive(Debug, Clone)]
pub struct VariableDiff {
    /// Variables present only in the "after" state.
    pub added: Vec<String>,
    /// Variables present only in the "before" state.
    pub removed: Vec<String>,
    /// Variables whose repr changed between states.
    pub changed: Vec<ChangedVariable>,
    /// Variables with identical repr in both states.
    pub unchanged: Vec<String>,
}

/// One entry in a variable diff's `changed` list.
#[derive(Debug, Clone)]
pub struct ChangedVariable {
    /// Variable name.
    pub name: String,
    /// Repr string in the "before" state.
    pub before: String,
    /// Repr string in the "after" state.
    pub after: String,
}

/// Snapshot payload for heap diff storage.
///
/// Contains the aggregate heap counters and optional per-variable repr strings.
#[derive(Debug, Clone)]
struct HeapSnapshotEntry {
    /// Aggregate heap counters at snapshot time.
    stats: HeapStats,
    /// Variable reprs keyed by global name, when available.
    variables: Option<HashMap<String, String>>,
}

// =============================================================================
// Session entry (private)
// =============================================================================

/// One entry in the session registry.
///
/// Groups a `ReplSession` with metadata for reset, pending-call tracking,
/// and undo history. The history is a bounded stack of previous session
/// snapshots pushed before each successful execute/resume.
struct SessionEntry {
    /// The live interpreter session.
    session: ReplSession,
    /// External function names registered for this session.
    external_functions: Vec<String>,
    /// If non-`None`, the call ID of the pending external function call
    /// that must be resolved via `resume` before further execution.
    pending_call_id: Option<u32>,
    /// Undo history stack. Front = oldest, back = most recent.
    history: VecDeque<ReplSession>,
    /// Maximum number of undo history entries to retain.
    max_history: usize,
}

// =============================================================================
// SessionManager
// =============================================================================

/// Multi-session manager for Ouros interpreter instances.
///
/// Manages a registry of named `ReplSession` instances and provides a typed
/// API for session lifecycle, code execution, variable management, undo
/// history, heap introspection, and session persistence.
///
/// A "default" session is always present and is used when callers pass `None`
/// as the optional session ID parameter.
///
/// # Example
///
/// ```
/// use ouros::session_manager::SessionManager;
/// let mut mgr = SessionManager::new("script.py");
/// mgr.execute(None, "x = 42").unwrap();
/// let val = mgr.get_variable(None, "x").unwrap();
/// assert_eq!(val.json_value, serde_json::json!(42));
/// ```
pub struct SessionManager {
    /// Named sessions keyed by session ID.
    sessions: HashMap<String, SessionEntry>,
    /// The session ID used when no session ID is provided.
    default_session: String,
    /// Script name passed to newly created sessions.
    script_name: String,
    /// Resource limits applied to each new or reset session.
    resource_limits: ResourceLimits,
    /// Named heap snapshots for diff comparisons.
    snapshots: HashMap<String, HeapSnapshotEntry>,
    /// Directory for storing saved sessions (None = persistence disabled).
    storage_dir: Option<PathBuf>,
}

// =============================================================================
// Constructor and configuration
// =============================================================================

impl SessionManager {
    /// Creates a new manager with a single "default" session using conservative
    /// default resource limits.
    ///
    /// The default limits are: 10M operations, 1M allocations, 256 MiB memory.
    #[must_use]
    pub fn new(script_name: &str) -> Self {
        Self::new_with_limits(script_name, default_resource_limits())
    }

    /// Creates a new manager with explicit per-session resource limits.
    ///
    /// A "default" session is created immediately using the given limits.
    #[must_use]
    pub fn new_with_limits(script_name: &str, resource_limits: ResourceLimits) -> Self {
        let mut mgr = Self {
            sessions: HashMap::new(),
            default_session: DEFAULT_SESSION_ID.to_owned(),
            script_name: script_name.to_owned(),
            resource_limits,
            snapshots: HashMap::new(),
            storage_dir: None,
        };
        mgr.sessions
            .insert(DEFAULT_SESSION_ID.to_owned(), mgr.build_session_entry(Vec::new()));
        mgr
    }

    /// Configures the directory for session persistence.
    ///
    /// When set, `save_session` and `load_session` become available. The
    /// directory is created if it does not exist.
    pub fn set_storage_dir(&mut self, dir: PathBuf) {
        let _ = fs::create_dir_all(&dir);
        self.storage_dir = Some(dir);
    }
}

// =============================================================================
// Execution
// =============================================================================

impl SessionManager {
    /// Executes Python code in a session.
    ///
    /// Before execution, the session state is forked for undo history. On
    /// success the fork is pushed onto the history stack. On failure it is
    /// discarded and the session is unchanged.
    ///
    /// Pass `None` for `session_id` to use the default session.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist, or
    /// `SessionError::Repl` for parse/compile/runtime errors.
    pub fn execute(&mut self, session_id: Option<&str>, code: &str) -> Result<ExecuteOutput, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        let snapshot = entry.session.fork();
        let mut printer = CollectStringPrint::new();
        let result = entry.session.execute_interactive(code, &mut printer);
        let stdout = printer.into_output();

        match result {
            Ok(progress) => {
                update_pending_call_id(entry, &progress);
                push_history(entry, snapshot);
                Ok(ExecuteOutput { progress, stdout })
            }
            Err(error) => Err(SessionError::Repl(error)),
        }
    }

    /// Resumes execution after an external function call.
    ///
    /// The `call_id` must match the pending call on the session. Accepts a
    /// concrete `ExternalResult` to provide the return value or error.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidState` if the call ID does not match.
    pub fn resume(
        &mut self,
        session_id: Option<&str>,
        call_id: u32,
        result: ExternalResult,
    ) -> Result<ExecuteOutput, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        if entry.pending_call_id != Some(call_id) {
            return Err(SessionError::InvalidState(format!(
                "resume call_id {call_id} does not match pending call {:?}",
                entry.pending_call_id
            )));
        }

        let snapshot = entry.session.fork();
        let mut printer = CollectStringPrint::new();
        let progress_result = entry.session.resume(result, &mut printer);
        let stdout = printer.into_output();

        match progress_result {
            Ok(progress) => {
                update_pending_call_id(entry, &progress);
                push_history(entry, snapshot);
                Ok(ExecuteOutput { progress, stdout })
            }
            Err(error) => Err(SessionError::Repl(error)),
        }
    }

    /// Resumes an external call as a pending async future instead of providing
    /// a concrete result.
    ///
    /// The call is deferred and represented as an `ExternalFuture` value in the
    /// VM. The future can later be awaited, yielding `ResolveFutures`.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidState` if the call ID does not match.
    pub fn resume_as_pending(&mut self, session_id: Option<&str>, call_id: u32) -> Result<ExecuteOutput, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        if entry.pending_call_id != Some(call_id) {
            return Err(SessionError::InvalidState(format!(
                "resume_as_pending call_id {call_id} does not match pending call {:?}",
                entry.pending_call_id
            )));
        }

        let snapshot = entry.session.fork();
        let mut printer = CollectStringPrint::new();
        let progress_result = entry.session.resume(ExternalResult::Future, &mut printer);
        let stdout = printer.into_output();

        match progress_result {
            Ok(progress) => {
                update_pending_call_id(entry, &progress);
                push_history(entry, snapshot);
                Ok(ExecuteOutput { progress, stdout })
            }
            Err(error) => Err(SessionError::Repl(error)),
        }
    }

    /// Resumes execution with results for pending async futures.
    ///
    /// Accepts a list of `(call_id, ExternalResult)` pairs. Supports
    /// incremental resolution -- provide a subset and execution continues
    /// until blocked again.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist, or
    /// `SessionError::Repl` for runtime errors.
    pub fn resume_futures(
        &mut self,
        session_id: Option<&str>,
        results: Vec<(u32, ExternalResult)>,
    ) -> Result<ExecuteOutput, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        let snapshot = entry.session.fork();
        let mut printer = CollectStringPrint::new();
        let progress_result = entry.session.resume_futures(results, &mut printer);
        let stdout = printer.into_output();

        match progress_result {
            Ok(progress) => {
                update_pending_call_id(entry, &progress);
                push_history(entry, snapshot);
                Ok(ExecuteOutput { progress, stdout })
            }
            Err(error) => Err(SessionError::Repl(error)),
        }
    }
}

// =============================================================================
// Variable operations
// =============================================================================

impl SessionManager {
    /// Lists defined global variables and their types in a session.
    ///
    /// External function bindings and undefined slots are excluded.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist.
    pub fn list_variables(&self, session_id: Option<&str>) -> Result<Vec<VariableInfo>, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session(sid)?;
        let vars = entry
            .session
            .list_variables()
            .into_iter()
            .map(|(name, type_name)| VariableInfo { name, type_name })
            .collect();
        Ok(vars)
    }

    /// Gets one variable's value from a session namespace.
    ///
    /// Returns the JSON representation and Python repr string of the variable.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session or variable does not exist.
    pub fn get_variable(&self, session_id: Option<&str>, name: &str) -> Result<VariableValue, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session(sid)?;

        let obj = entry
            .session
            .get_variable(name)
            .ok_or_else(|| SessionError::NotFound(format!("variable '{name}' not found")))?;

        let json_value = obj.to_json_value();
        let repr = entry.session.get_variable_repr(name);
        Ok(VariableValue { json_value, repr })
    }

    /// Sets or creates a global variable by evaluating a Python expression.
    ///
    /// The `value` parameter is a Python expression string (e.g. `"[1, 2, 3]"`,
    /// `"42"`, `"'hello'"`) that is executed in a forked session. The result is
    /// then injected into the real session as the named variable.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidState` if the session has a pending call,
    /// or `SessionError::Repl` if the expression fails.
    pub fn set_variable(&mut self, session_id: Option<&str>, name: &str, value: &str) -> Result<(), SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        if entry.pending_call_id.is_some() {
            return Err(SessionError::InvalidState(
                "cannot set variable while a call is pending".to_owned(),
            ));
        }

        // Execute the value expression in a fork to get the Object.
        let mut fork = entry.session.fork();
        let mut printer = CollectStringPrint::new();
        let result = fork.execute_interactive(value, &mut printer);

        match result {
            Ok(ReplProgress::Complete(obj)) => {
                entry
                    .session
                    .set_variable(name, obj)
                    .map_err(|err| SessionError::InvalidState(format!("set_variable failed: {err}")))?;
                Ok(())
            }
            Ok(_) => Err(SessionError::InvalidState(
                "set_variable expression triggered external call".to_owned(),
            )),
            Err(e) => Err(SessionError::Repl(e)),
        }
    }

    /// Sets or creates a global variable from a pre-built `Object`.
    ///
    /// Unlike [`set_variable`](Self::set_variable) which evaluates a Python
    /// expression, this method directly injects a `Object` that has
    /// already been constructed (e.g. from JSON deserialization). This is the
    /// entry point used by MCP handlers that receive values as JSON.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidState` if the session has a pending call,
    /// or if the underlying `ReplSession::set_variable` fails.
    pub fn set_variable_obj(&mut self, session_id: Option<&str>, name: &str, obj: Object) -> Result<(), SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        if entry.pending_call_id.is_some() {
            return Err(SessionError::InvalidState(
                "cannot set variable while a call is pending".to_owned(),
            ));
        }

        entry
            .session
            .set_variable(name, obj)
            .map_err(|err| SessionError::InvalidState(format!("set_variable failed: {err}")))
    }

    /// Deletes a global variable from a session.
    ///
    /// Returns `true` if the variable existed and was removed, `false` if the
    /// name was unknown or already undefined.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidState` if the session has a pending call.
    pub fn delete_variable(&mut self, session_id: Option<&str>, name: &str) -> Result<bool, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        if entry.pending_call_id.is_some() {
            return Err(SessionError::InvalidState(
                "cannot delete variable while a call is pending".to_owned(),
            ));
        }

        entry
            .session
            .delete_variable(name)
            .map_err(|err| SessionError::InvalidState(format!("delete_variable failed: {err}")))
    }

    /// Transfers a variable from one session to another.
    ///
    /// Reads the variable from the source session as a `Object` (which is
    /// heap-independent), then writes it into the target session via
    /// `set_variable`. This ensures no raw heap references leak across sessions.
    ///
    /// # Errors
    ///
    /// Returns errors if either session does not exist, the variable is not
    /// found, or the target has a pending call.
    pub fn transfer_variable(
        &mut self,
        source: &str,
        target: &str,
        name: &str,
        target_name: Option<&str>,
    ) -> Result<(), SessionError> {
        // Read from source (immutable, captures heap-independent Object).
        let obj = {
            let source_entry = self.get_session(source)?;
            source_entry
                .session
                .get_variable(name)
                .ok_or_else(|| SessionError::NotFound(format!("variable '{name}' not found in session '{source}'")))?
        };

        // Write to target (mutable -- separate scope from source borrow).
        let dest_name = target_name.unwrap_or(name);
        let target_entry = self.get_session_mut(target)?;

        if target_entry.pending_call_id.is_some() {
            return Err(SessionError::InvalidState(
                "cannot transfer variable while target session has a pending call".to_owned(),
            ));
        }

        target_entry
            .session
            .set_variable(dest_name, obj)
            .map_err(|err| SessionError::InvalidState(format!("transfer_variable failed: {err}")))
    }

    /// Evaluates a Python expression without modifying session state.
    ///
    /// Internally forks the session, executes the expression in the fork, and
    /// returns the result. The fork is discarded, leaving the original session
    /// untouched.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression triggers an external function call
    /// or fails at runtime.
    pub fn eval_variable(&mut self, session_id: Option<&str>, expression: &str) -> Result<EvalOutput, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session(sid)?;

        let mut fork = entry.session.fork();
        let mut printer = CollectStringPrint::new();
        let result = fork.execute_interactive(expression, &mut printer);
        let stdout = printer.into_output();

        match result {
            Ok(ReplProgress::Complete(obj)) => {
                let repr = (!matches!(obj, Object::None)).then(|| obj.py_repr());
                let json_value = obj.to_json_value();
                Ok(EvalOutput {
                    value: VariableValue { json_value, repr },
                    stdout,
                })
            }
            Ok(ReplProgress::FunctionCall { .. } | ReplProgress::ProxyCall { .. }) => Err(SessionError::InvalidState(
                "eval_variable does not support external function calls".to_owned(),
            )),
            Ok(ReplProgress::ResolveFutures { .. }) => Err(SessionError::InvalidState(
                "eval_variable does not support async futures".to_owned(),
            )),
            Err(e) => Err(SessionError::Repl(e)),
        }
    }
}

// =============================================================================
// Cross-session pipeline
// =============================================================================

impl SessionManager {
    /// Executes code in a source session and stores the result in a target session.
    ///
    /// Enables the pipeline pattern where session A's output feeds session B.
    /// If execution yields a non-complete progress (function call, proxy call,
    /// futures), the progress is returned and the target variable is NOT set.
    ///
    /// # Errors
    ///
    /// Returns errors if sessions don't exist, source == target, or runtime fails.
    pub fn call_session(
        &mut self,
        source: Option<&str>,
        target: &str,
        code: &str,
        target_variable: &str,
    ) -> Result<ExecuteOutput, SessionError> {
        let source_id = resolve_session_id(source);
        let target_id = target;

        if source_id == target_id {
            return Err(SessionError::InvalidArgument(format!(
                "source and target are the same session '{source_id}'; use regular execute"
            )));
        }

        // Verify target exists before executing in source.
        if !self.sessions.contains_key(target_id) {
            return Err(SessionError::NotFound(format!("session '{target_id}' not found")));
        }

        // Execute in source.
        let source_entry = self.get_session_mut(source_id)?;
        let mut printer = CollectStringPrint::new();
        let result = source_entry.session.execute_interactive(code, &mut printer);
        let stdout = printer.into_output();

        match result {
            Ok(ReplProgress::Complete(result_obj)) => {
                // Clear pending call on source.
                let source_entry = self.get_session_mut(source_id)?;
                source_entry.pending_call_id = None;

                // Store result in target.
                let target_entry = self.get_session_mut(target_id)?;
                if target_entry.pending_call_id.is_some() {
                    return Err(SessionError::InvalidState(
                        "cannot set variable while target session has a pending call".to_owned(),
                    ));
                }

                let progress = ReplProgress::Complete(result_obj.clone());
                target_entry
                    .session
                    .set_variable(target_variable, result_obj)
                    .map_err(|err| SessionError::InvalidState(format!("call_session set_variable failed: {err}")))?;

                Ok(ExecuteOutput { progress, stdout })
            }
            Ok(progress) => {
                // Non-complete progress: update source pending state, don't touch target.
                let source_entry = self.get_session_mut(source_id)?;
                update_pending_call_id(source_entry, &progress);
                Ok(ExecuteOutput { progress, stdout })
            }
            Err(error) => Err(SessionError::Repl(error)),
        }
    }
}

// =============================================================================
// Session lifecycle
// =============================================================================

impl SessionManager {
    /// Creates a new named session.
    ///
    /// The session starts empty with the given external function bindings.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::AlreadyExists` if a session with `id` exists.
    pub fn create_session(&mut self, id: &str, external_functions: Vec<String>) -> Result<(), SessionError> {
        if self.sessions.contains_key(id) {
            return Err(SessionError::AlreadyExists(format!("session '{id}' already exists")));
        }
        self.sessions
            .insert(id.to_owned(), self.build_session_entry(external_functions));
        Ok(())
    }

    /// Destroys a named session.
    ///
    /// The default session cannot be destroyed.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidState` for the default session, or
    /// `SessionError::NotFound` if the session does not exist.
    pub fn destroy_session(&mut self, id: &str) -> Result<(), SessionError> {
        if id == self.default_session {
            return Err(SessionError::InvalidState(format!(
                "cannot destroy the default session '{}'",
                self.default_session
            )));
        }
        if self.sessions.remove(id).is_none() {
            return Err(SessionError::NotFound(format!("session '{id}' not found")));
        }
        Ok(())
    }

    /// Lists all active sessions with their variable counts.
    ///
    /// Results are sorted by session ID for deterministic output.
    #[must_use]
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self
            .sessions
            .iter()
            .map(|(id, entry)| SessionInfo {
                id: id.clone(),
                variable_count: entry.session.list_variables().len(),
            })
            .collect();
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        sessions
    }

    /// Forks an existing session into a new independent copy.
    ///
    /// Uses `ReplSession::fork()` to produce a deep clone. The forked session
    /// starts with empty history and no pending call.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the source does not exist, or
    /// `SessionError::AlreadyExists` if the new ID is taken.
    pub fn fork_session(&mut self, source: &str, new_id: &str) -> Result<(), SessionError> {
        if self.sessions.contains_key(new_id) {
            return Err(SessionError::AlreadyExists(format!(
                "session '{new_id}' already exists"
            )));
        }
        let source_entry = self
            .sessions
            .get(source)
            .ok_or_else(|| SessionError::NotFound(format!("session '{source}' not found")))?;

        let forked = SessionEntry {
            session: source_entry.session.fork(),
            external_functions: source_entry.external_functions.clone(),
            pending_call_id: None,
            history: VecDeque::new(),
            max_history: source_entry.max_history,
        };
        self.sessions.insert(new_id.to_owned(), forked);
        Ok(())
    }

    /// Resets a session, replacing it with a fresh interpreter instance.
    ///
    /// The session's history is cleared but its `max_history` setting is
    /// preserved. Pass `None` for `session_id` to reset the default session.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist.
    pub fn reset(&mut self, session_id: Option<&str>, external_functions: Vec<String>) -> Result<(), SessionError> {
        let sid = resolve_session_id(session_id);
        let new_entry = self.build_session_entry(external_functions.clone());
        let entry = self.get_session_mut(sid)?;
        entry.external_functions = external_functions;
        entry.session = new_entry.session;
        entry.pending_call_id = None;
        entry.history.clear();
        Ok(())
    }
}

// =============================================================================
// Session persistence
// =============================================================================

impl SessionManager {
    /// Saves a session to disk as a named snapshot.
    ///
    /// The session must be idle (not mid-yield). The snapshot name is validated
    /// to prevent path traversal: only alphanumeric characters, hyphens, and
    /// underscores are allowed.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::Storage` if persistence is not configured or
    /// I/O fails, `SessionError::InvalidArgument` for invalid names.
    pub fn save_session(&self, session_id: Option<&str>, name: Option<&str>) -> Result<SaveResult, SessionError> {
        let storage_dir = self
            .storage_dir
            .as_ref()
            .ok_or_else(|| SessionError::Storage("storage not configured".to_owned()))?;

        let sid = resolve_session_id(session_id);
        let snapshot_name = name.unwrap_or(sid).to_owned();
        validate_snapshot_name(&snapshot_name)?;

        let entry = self.get_session(sid)?;
        let bytes = entry.session.save().map_err(SessionError::InvalidState)?;
        let path = storage_dir.join(format!("{snapshot_name}.bin"));
        fs::write(&path, &bytes).map_err(|e| SessionError::Storage(format!("failed to write snapshot: {e}")))?;

        Ok(SaveResult {
            name: snapshot_name,
            size_bytes: bytes.len(),
        })
    }

    /// Loads a previously saved session from disk.
    ///
    /// Creates a new session with the restored state. Returns the session ID
    /// that was created. If `session_id` is `None`, the snapshot name is used
    /// as the session ID.
    ///
    /// # Errors
    ///
    /// Returns errors if the snapshot is not found, the session ID already
    /// exists, or deserialization fails.
    pub fn load_session(&mut self, name: &str, session_id: Option<&str>) -> Result<String, SessionError> {
        let storage_dir = self
            .storage_dir
            .as_ref()
            .ok_or_else(|| SessionError::Storage("storage not configured".to_owned()))?;

        validate_snapshot_name(name)?;

        let sid = session_id.unwrap_or(name).to_owned();
        if self.sessions.contains_key(&sid) {
            return Err(SessionError::AlreadyExists(format!("session '{sid}' already exists")));
        }

        let path = storage_dir.join(format!("{name}.bin"));
        let bytes = fs::read(&path).map_err(|e| SessionError::Storage(format!("snapshot '{name}' not found: {e}")))?;
        let session = ReplSession::load(&bytes, self.resource_limits.clone())
            .map_err(|e| SessionError::Storage(format!("deserialization failed: {e}")))?;

        let entry = SessionEntry {
            session,
            external_functions: Vec::new(),
            pending_call_id: None,
            history: VecDeque::new(),
            max_history: DEFAULT_MAX_HISTORY,
        };
        self.sessions.insert(sid.clone(), entry);
        Ok(sid)
    }

    /// Lists all saved session snapshots on disk.
    ///
    /// Scans the configured storage directory for `.bin` files and returns
    /// their names and sizes. Results are sorted by name.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::Storage` if persistence is not configured.
    pub fn list_saved_sessions(&self) -> Result<Vec<SavedSessionInfo>, SessionError> {
        let storage_dir = self
            .storage_dir
            .as_ref()
            .ok_or_else(|| SessionError::Storage("storage not configured".to_owned()))?;

        let mut sessions = Vec::new();
        if storage_dir.exists() {
            let entries = fs::read_dir(storage_dir)
                .map_err(|e| SessionError::Storage(format!("failed to read storage dir: {e}")))?;
            for entry in entries {
                let entry = entry.map_err(|e| SessionError::Storage(format!("dir entry error: {e}")))?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("bin")
                    && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    sessions.push(SavedSessionInfo {
                        name: name.to_owned(),
                        size_bytes: size,
                    });
                }
            }
        }
        sessions.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(sessions)
    }
}

// =============================================================================
// History / rewind
// =============================================================================

impl SessionManager {
    /// Rewinds a session by N steps, restoring it to a previous state.
    ///
    /// Pops N entries from the history stack and replaces the current session
    /// with the state at position N. Clears pending call state.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidArgument` if steps == 0, or
    /// `SessionError::InvalidState` if steps exceeds available history.
    ///
    /// # Panics
    ///
    /// Panics if the history becomes unexpectedly empty after the bounds check
    /// passes. This should not happen in practice since we verify `steps <= history.len()`
    /// before popping.
    pub fn rewind(&mut self, session_id: Option<&str>, steps: usize) -> Result<RewindResult, SessionError> {
        if steps == 0 {
            return Err(SessionError::InvalidArgument(
                "rewind steps must be at least 1".to_owned(),
            ));
        }

        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;

        if steps > entry.history.len() {
            return Err(SessionError::InvalidState(format!(
                "cannot rewind {steps} steps: only {} history entries available",
                entry.history.len()
            )));
        }

        let mut restored = None;
        for _ in 0..steps {
            restored = entry.history.pop_back();
        }

        entry.session = restored.expect("at least one history entry should have been popped");
        entry.pending_call_id = None;

        Ok(RewindResult {
            steps_rewound: steps,
            history_remaining: entry.history.len(),
        })
    }

    /// Returns the current undo history depth and configured maximum for a session.
    ///
    /// Returns `(current_depth, max_depth)`.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist.
    pub fn history(&self, session_id: Option<&str>) -> Result<(usize, usize), SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session(sid)?;
        Ok((entry.history.len(), entry.max_history))
    }

    /// Configures the maximum undo history depth for a session.
    ///
    /// If the new maximum is less than the current history depth, the oldest
    /// entries are trimmed. Returns the number of entries that were trimmed.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist.
    pub fn set_history_depth(&mut self, session_id: Option<&str>, max_depth: usize) -> Result<usize, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session_mut(sid)?;
        entry.max_history = max_depth;
        Ok(trim_history(entry))
    }
}

// =============================================================================
// Heap introspection
// =============================================================================

impl SessionManager {
    /// Returns heap statistics for a session.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist.
    pub fn heap_stats(&self, session_id: Option<&str>) -> Result<HeapStats, SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session(sid)?;
        Ok(entry.session.heap_stats())
    }

    /// Saves the current heap stats as a named snapshot for later diff.
    ///
    /// Overwrites any existing snapshot with the same name. The snapshot
    /// includes per-variable repr strings for detailed diff output.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if the session does not exist.
    pub fn snapshot_heap(&mut self, session_id: Option<&str>, name: &str) -> Result<(), SessionError> {
        let sid = resolve_session_id(session_id);
        let entry = self.get_session(sid)?;
        let snapshot = capture_heap_snapshot(&entry.session);
        self.snapshots.insert(name.to_owned(), snapshot);
        Ok(())
    }

    /// Compares two named heap snapshots and returns the diff.
    ///
    /// Both `before` and `after` must be snapshot names previously created
    /// via `snapshot_heap`.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if either snapshot does not exist.
    pub fn diff_heap(&self, before: &str, after: &str) -> Result<HeapDiffResult, SessionError> {
        let snap_before = self
            .snapshots
            .get(before)
            .ok_or_else(|| SessionError::NotFound(format!("snapshot '{before}' not found")))?;
        let snap_after = self
            .snapshots
            .get(after)
            .ok_or_else(|| SessionError::NotFound(format!("snapshot '{after}' not found")))?;

        let heap_diff = snap_before.stats.diff(&snap_after.stats);
        let variable_diff = diff_variable_maps(snap_before.variables.as_ref(), snap_after.variables.as_ref());

        Ok(HeapDiffResult {
            heap_diff,
            variable_diff,
        })
    }

    /// Compares the current heap state of two sessions and returns the diff.
    ///
    /// Unlike [`diff_heap`](Self::diff_heap) which compares named snapshots,
    /// this method captures a live snapshot of each session and diffs them
    /// on the fly. Useful for cross-session comparison without requiring
    /// explicit snapshot creation.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::NotFound` if either session does not exist.
    pub fn diff_heap_sessions(
        &self,
        before_session: &str,
        after_session: &str,
    ) -> Result<HeapDiffResult, SessionError> {
        let entry_a = self.get_session(before_session)?;
        let entry_b = self.get_session(after_session)?;
        let snap_a = capture_heap_snapshot(&entry_a.session);
        let snap_b = capture_heap_snapshot(&entry_b.session);

        let heap_diff = snap_a.stats.diff(&snap_b.stats);
        let variable_diff = diff_variable_maps(snap_a.variables.as_ref(), snap_b.variables.as_ref());

        Ok(HeapDiffResult {
            heap_diff,
            variable_diff,
        })
    }
}

// =============================================================================
// Private helpers
// =============================================================================

impl SessionManager {
    /// Builds a fresh session entry using this manager's configured limits.
    fn build_session_entry(&self, external_functions: Vec<String>) -> SessionEntry {
        SessionEntry {
            session: ReplSession::new_with_resource_limits(
                external_functions.clone(),
                &self.script_name,
                self.resource_limits.clone(),
            ),
            external_functions,
            pending_call_id: None,
            history: VecDeque::new(),
            max_history: DEFAULT_MAX_HISTORY,
        }
    }

    /// Looks up a session by ID, returning a shared reference.
    fn get_session(&self, session_id: &str) -> Result<&SessionEntry, SessionError> {
        self.sessions
            .get(session_id)
            .ok_or_else(|| SessionError::NotFound(format!("session '{session_id}' not found")))
    }

    /// Looks up a session by ID, returning a mutable reference.
    fn get_session_mut(&mut self, session_id: &str) -> Result<&mut SessionEntry, SessionError> {
        self.sessions
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(format!("session '{session_id}' not found")))
    }
}

/// Resolves an optional session ID to the default when `None`.
fn resolve_session_id(session_id: Option<&str>) -> &str {
    session_id.unwrap_or(DEFAULT_SESSION_ID)
}

/// Returns conservative default resource limits for sessions.
fn default_resource_limits() -> ResourceLimits {
    ResourceLimits::new()
        .max_operations(DEFAULT_MAX_OPERATIONS)
        .max_allocations(DEFAULT_MAX_ALLOCATIONS)
        .max_memory(DEFAULT_MAX_MEMORY_BYTES)
}

/// Pushes a pre-execution session snapshot onto the history stack.
///
/// If the stack exceeds `max_history`, the oldest entries are dropped from the
/// front. Called after each successful execute/resume.
fn push_history(entry: &mut SessionEntry, snapshot: ReplSession) {
    entry.history.push_back(snapshot);
    trim_history(entry);
}

/// Trims the history deque to respect `max_history`, dropping from the front.
///
/// Returns the number of entries that were dropped.
fn trim_history(entry: &mut SessionEntry) -> usize {
    let mut trimmed = 0;
    while entry.history.len() > entry.max_history {
        entry.history.pop_front();
        trimmed += 1;
    }
    trimmed
}

/// Updates the `pending_call_id` on a session entry based on execution progress.
///
/// Sets the pending ID for function/proxy calls and clears it for complete
/// and resolve_futures progress states.
fn update_pending_call_id(entry: &mut SessionEntry, progress: &ReplProgress) {
    match progress {
        ReplProgress::FunctionCall { call_id, .. } | ReplProgress::ProxyCall { call_id, .. } => {
            entry.pending_call_id = Some(*call_id);
        }
        ReplProgress::Complete(_) | ReplProgress::ResolveFutures { .. } => {
            entry.pending_call_id = None;
        }
    }
}

/// Validates a snapshot name for use as a filesystem filename.
///
/// Only alphanumeric characters, hyphens, and underscores are allowed. This
/// prevents path traversal attacks.
fn validate_snapshot_name(name: &str) -> Result<(), SessionError> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(SessionError::InvalidArgument(format!(
            "invalid snapshot name '{name}': only alphanumeric characters, hyphens, and underscores are allowed"
        )));
    }
    Ok(())
}

/// Captures the current heap stats and variable reprs for one session.
fn capture_heap_snapshot(session: &ReplSession) -> HeapSnapshotEntry {
    HeapSnapshotEntry {
        stats: session.heap_stats(),
        variables: Some(collect_variable_reprs(session)),
    }
}

/// Collects Python repr strings for all defined global variables.
fn collect_variable_reprs(session: &ReplSession) -> HashMap<String, String> {
    session
        .list_variables()
        .into_iter()
        .map(|(name, _type_name)| {
            let repr = session
                .get_variable_repr(&name)
                .unwrap_or_else(|| "<repr unavailable>".to_owned());
            (name, repr)
        })
        .collect()
}

/// Computes variable-level differences between two snapshots.
///
/// Missing variable maps are treated as empty maps for backward compat.
fn diff_variable_maps(
    before: Option<&HashMap<String, String>>,
    after: Option<&HashMap<String, String>>,
) -> VariableDiff {
    let mut names = BTreeSet::new();
    if let Some(before_map) = before {
        names.extend(before_map.keys().cloned());
    }
    if let Some(after_map) = after {
        names.extend(after_map.keys().cloned());
    }

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();

    for name in names {
        match (
            before.and_then(|map| map.get(&name)),
            after.and_then(|map| map.get(&name)),
        ) {
            (None, Some(_)) => added.push(name),
            (Some(_), None) => removed.push(name),
            (Some(before_repr), Some(after_repr)) if before_repr == after_repr => {
                unchanged.push(name);
            }
            (Some(before_repr), Some(after_repr)) => changed.push(ChangedVariable {
                name,
                before: before_repr.clone(),
                after: after_repr.clone(),
            }),
            (None, None) => {}
        }
    }

    VariableDiff {
        added,
        removed,
        changed,
        unchanged,
    }
}

/// Helper trait to check if a `ReplProgress` is a `Complete` variant.
impl ReplProgress {
    /// Returns `true` if this progress is a `Complete` variant.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }
}
