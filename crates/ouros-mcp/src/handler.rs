use std::path::PathBuf;

use ouros::{
    ExcType, Exception, ExternalResult, HeapStats, Object, ReplProgress,
    session_manager::{ExecuteOutput, HeapDiffResult, SessionError, SessionManager, VariableDiff},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// =============================================================================
// Public types
// =============================================================================

/// Static MCP tool metadata exposed by `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name used by `tools/call`.
    pub name: String,
    /// Human-readable description for clients.
    pub description: String,
}

// =============================================================================
// McpHandler
// =============================================================================

/// Thin MCP adapter around [`SessionManager`].
///
/// Manages the translation layer between JSON-based MCP tool calls and the
/// typed Rust API provided by `SessionManager`. Each tool method parses JSON
/// arguments, delegates to the corresponding `SessionManager` method, and
/// serializes the result back to JSON.
///
/// A "default" session is always present and is used when callers omit the
/// optional `session_id` field from tool arguments. This ensures full backward
/// compatibility with the original single-session API.
pub struct McpHandler {
    /// The underlying session manager that handles all business logic.
    manager: SessionManager,
}

// =============================================================================
// Constructor and public API
// =============================================================================

impl McpHandler {
    /// Creates a new handler with a single "default" session.
    #[must_use]
    pub fn new(script_name: &str) -> Self {
        Self {
            manager: SessionManager::new(script_name),
        }
    }

    /// Configures the directory for session persistence.
    ///
    /// Creates the directory if it does not exist. When set, `save_session`
    /// and `load_session` tools become available.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn set_storage_dir(&mut self, dir: PathBuf) -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create storage directory: {e}"))?;
        self.manager.set_storage_dir(dir);
        Ok(())
    }

    /// Returns the tools supported by this handler.
    ///
    /// The first five entries are the original single-session tools (execute,
    /// resume, list_variables, get_variable, reset), followed by session
    /// management tools and heap introspection tools.
    #[must_use]
    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        vec![
            // Original tools.
            tool("execute", "Execute Python code in a session."),
            tool("resume", "Resume execution after an external/proxy call."),
            tool(
                "resume_as_pending",
                "Resume an external call as a pending async future instead of providing a result.",
            ),
            tool(
                "resume_futures",
                "Resume execution with results for pending async futures. Each entry may provide \
                 a 'result' value for success or an 'error' string to fail that future with a RuntimeError.",
            ),
            tool("list_variables", "List defined global variables and their types."),
            tool("get_variable", "Get one variable from a session namespace."),
            tool("set_variable", "Set or create a global variable in a session."),
            tool("delete_variable", "Delete a global variable from a session."),
            tool("transfer_variable", "Transfer a variable from one session to another."),
            tool(
                "eval_variable",
                "Evaluate a Python expression in a session without modifying state. Returns the result.",
            ),
            tool(
                "call_session",
                "Execute code in a source session and store the result as a variable in a target session.",
            ),
            tool("reset", "Reset a session with optional external functions."),
            // Session management.
            tool("create_session", "Create a new named session."),
            tool("destroy_session", "Destroy a named session (cannot destroy 'default')."),
            tool("list_sessions", "List all active sessions with variable counts."),
            tool("fork_session", "Fork an existing session into a new independent copy."),
            // Session persistence.
            tool(
                "save_session",
                "Save session state to disk as a named snapshot. Returns error if session has pending calls.",
            ),
            tool(
                "load_session",
                "Load a previously saved session from disk. Creates a new session with the restored state.",
            ),
            tool(
                "list_saved_sessions",
                "List all saved session snapshots available on disk.",
            ),
            // Undo history.
            tool(
                "rewind",
                "Undo the last N execute calls, restoring session to a previous state.",
            ),
            tool("history", "Get the number of available undo steps for a session."),
            tool(
                "set_history_depth",
                "Configure the maximum undo history depth for a session.",
            ),
            // Heap introspection.
            tool("heap_stats", "Get heap statistics for a session."),
            tool(
                "snapshot_heap",
                "Save current heap stats as a named snapshot for later diff.",
            ),
            tool(
                "diff_heap",
                "Compare two named snapshots or two sessions' current heap state.",
            ),
        ]
    }

    /// Dispatches one tool call by name.
    ///
    /// The return value is JSON-compatible and directly usable as MCP tool
    /// output. All tools accept an optional `session_id` string field in their
    /// arguments; when omitted the default session is used.
    pub fn call_tool(&mut self, tool_name: &str, arguments: Value) -> Result<Value, String> {
        match tool_name {
            "execute" => self.execute_tool(arguments),
            "resume" => self.resume_tool(arguments),
            "resume_as_pending" => self.resume_as_pending_tool(arguments),
            "resume_futures" => self.resume_futures_tool(arguments),
            "list_variables" => self.list_variables_tool(&arguments),
            "get_variable" => self.get_variable_tool(arguments),
            "set_variable" => self.set_variable_tool(arguments),
            "delete_variable" => self.delete_variable_tool(arguments),
            "transfer_variable" => self.transfer_variable_tool(arguments),
            "eval_variable" => self.eval_variable_tool(arguments),
            "call_session" => self.call_session_tool(arguments),
            "reset" => self.reset_tool(arguments),
            "create_session" => self.create_session_tool(arguments),
            "destroy_session" => self.destroy_session_tool(arguments),
            "list_sessions" => Ok(self.list_sessions_tool()),
            "fork_session" => self.fork_session_tool(arguments),
            "save_session" => self.save_session_tool(arguments),
            "load_session" => self.load_session_tool(arguments),
            "list_saved_sessions" => self.list_saved_sessions_tool(),
            "rewind" => self.rewind_tool(arguments),
            "history" => self.history_tool(&arguments),
            "set_history_depth" => self.set_history_depth_tool(arguments),
            "heap_stats" => self.heap_stats_tool(&arguments),
            "snapshot_heap" => self.snapshot_heap_tool(arguments),
            "diff_heap" => self.diff_heap_tool(arguments),
            other => Err(format!("unknown tool '{other}'")),
        }
    }
}

// =============================================================================
// Execution tools
// =============================================================================

impl McpHandler {
    /// Executes Python code in a session.
    ///
    /// Accepts `{"code": "...", "session_id": "..."}` where `session_id` is
    /// optional and defaults to `"default"`.
    fn execute_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            code: String,
            session_id: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments).map_err(|err| format!("invalid execute args: {err}"))?;

        match self.manager.execute(args.session_id.as_deref(), &args.code) {
            Ok(output) => Ok(serialize_execute_output(output)),
            Err(SessionError::Repl(error)) => {
                let err_response = json!({
                    "status": "error",
                    "error": error.to_string()
                });
                Ok(err_response)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Resumes execution after an external function call.
    ///
    /// Accepts `{"call_id": N, "result": {...}, "session_id": "..."}` where
    /// `session_id` is optional. The `result` field accepts both natural JSON
    /// (e.g., `42`, `"hello"`, `null`) and the tagged format (e.g., `{"Int": 42}`)
    /// for backward compatibility.
    fn resume_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            call_id: u32,
            result: Value,
            session_id: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments).map_err(|err| format!("invalid resume args: {err}"))?;
        let result_obj = Object::from_json_value(args.result);
        let ext_result = ExternalResult::Return(result_obj);

        match self
            .manager
            .resume(args.session_id.as_deref(), args.call_id, ext_result)
        {
            Ok(output) => Ok(serialize_execute_output(output)),
            Err(SessionError::Repl(error)) => {
                let err_response = json!({
                    "status": "error",
                    "error": error.to_string()
                });
                Ok(err_response)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Resumes an external call as a pending async future.
    ///
    /// Instead of providing a concrete result, this tells the VM to continue
    /// execution with the call represented as an `ExternalFuture` value. The
    /// future can later be awaited, at which point the VM will yield
    /// `ResolveFutures` with all pending call IDs.
    ///
    /// Accepts `{"call_id": N, "session_id": "..."}` where `session_id` is
    /// optional. This is the MCP-level complement of
    /// `ReplSession::resume(ExternalResult::Future)`.
    fn resume_as_pending_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            call_id: u32,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid resume_as_pending args: {err}"))?;

        match self.manager.resume_as_pending(args.session_id.as_deref(), args.call_id) {
            Ok(output) => Ok(serialize_execute_output(output)),
            Err(SessionError::Repl(error)) => {
                let err_response = json!({
                    "status": "error",
                    "error": error.to_string()
                });
                Ok(err_response)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Resumes execution with results for pending async futures.
    ///
    /// Accepts `{"results": [{"call_id": N, "result": ..., "error": "..."}, ...], "session_id": "..."}`.
    /// Each entry in `results` may provide either:
    /// - `"result"`: a JSON value for the successful return value,
    /// - `"error"`: a string message to fail that future with a `RuntimeError`, or
    /// - neither: treated as a `None` return.
    ///
    /// Supports incremental resolution -- provide a subset of pending calls and
    /// execution will continue until blocked again. The `result` field accepts
    /// natural JSON (e.g., `42`, `"hello"`) and tagged format for backward compat.
    ///
    /// May return `{"status": "resolve_futures", "pending_call_ids": [...]}` if
    /// more futures still need resolution.
    fn resume_futures_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct FutureResult {
            call_id: u32,
            #[serde(default)]
            result: Option<serde_json::Value>,
            #[serde(default)]
            error: Option<String>,
        }

        #[derive(Deserialize)]
        struct Args {
            results: Vec<FutureResult>,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid resume_futures args: {err}"))?;

        let ext_results: Vec<(u32, ExternalResult)> = args
            .results
            .into_iter()
            .map(|r| {
                if let Some(error_msg) = r.error {
                    let exc = Exception::new(ExcType::RuntimeError, Some(error_msg));
                    (r.call_id, ExternalResult::Error(exc))
                } else if let Some(value) = r.result {
                    let obj = Object::from_json_value(value);
                    (r.call_id, ExternalResult::Return(obj))
                } else {
                    // Neither result nor error -- treat as None return.
                    (r.call_id, ExternalResult::Return(Object::None))
                }
            })
            .collect();

        match self.manager.resume_futures(args.session_id.as_deref(), ext_results) {
            Ok(output) => Ok(serialize_execute_output(output)),
            Err(SessionError::Repl(error)) => {
                let err_response = json!({
                    "status": "error",
                    "error": error.to_string()
                });
                Ok(err_response)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

// =============================================================================
// Variable tools
// =============================================================================

impl McpHandler {
    /// Lists global variables defined in a session.
    ///
    /// Accepts `{"session_id": "..."}` (optional). When called with an empty
    /// object `{}` or no arguments, uses the default session.
    fn list_variables_tool(&self, arguments: &Value) -> Result<Value, String> {
        let session_id = arguments.get("session_id").and_then(Value::as_str);
        let vars = self.manager.list_variables(session_id).map_err(|e| e.to_string())?;
        let variables: Vec<Value> = vars
            .into_iter()
            .map(|v| json!({ "name": v.name, "type": v.type_name }))
            .collect();
        Ok(json!({ "variables": variables }))
    }

    /// Gets one variable from a session namespace.
    ///
    /// Accepts `{"name": "...", "session_id": "..."}` where `session_id` is
    /// optional. Returns the value with its JSON representation and Python repr string.
    fn get_variable_tool(&self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid get_variable args: {err}"))?;

        let var_result = self.manager.get_variable(args.session_id.as_deref(), &args.name);

        // Build response: name + value + optional repr.
        let (json_value, repr) = match var_result {
            Ok(vv) => (Some(vv.json_value), vv.repr),
            Err(_) => (None, None),
        };

        let mut response = json!({ "name": args.name, "value": json_value });
        if let (Some(repr_str), Value::Object(map)) = (repr, &mut response) {
            map.insert("repr".to_string(), json!(repr_str));
        }

        // Add convenience `values` field for tuple variables.
        maybe_add_tuple_values(&mut response);

        Ok(response)
    }

    /// Sets or creates a global variable in a session.
    ///
    /// Accepts `{"name": "...", "value": <json>, "session_id": "..."}` where
    /// `session_id` is optional. The `value` field accepts both natural JSON
    /// (e.g., `42`, `"hello"`, `[1, 2, 3]`) and the tagged format (e.g.,
    /// `{"Int": 42}`) for backward compatibility.
    fn set_variable_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            value: Value,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid set_variable args: {err}"))?;

        let obj = Object::from_json_value(args.value);
        self.manager
            .set_variable_obj(args.session_id.as_deref(), &args.name, obj)
            .map_err(|e| e.to_string())?;

        Ok(json!({ "status": "ok", "name": args.name }))
    }

    /// Deletes a global variable from a session.
    ///
    /// Accepts `{"name": "...", "session_id": "..."}` where `session_id` is
    /// optional. Returns whether the variable existed and was removed.
    fn delete_variable_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid delete_variable args: {err}"))?;

        let existed = self
            .manager
            .delete_variable(args.session_id.as_deref(), &args.name)
            .map_err(|e| e.to_string())?;

        Ok(json!({ "status": "ok", "name": args.name, "existed": existed }))
    }

    /// Evaluates a Python expression without modifying session state.
    ///
    /// Internally forks the session, executes the expression in the fork,
    /// and returns the result. The fork is discarded, leaving the original
    /// session untouched. This is useful for orchestrators that need to
    /// peek at intermediate state without side effects.
    ///
    /// Accepts `{"expression": "...", "session_id": "..."}` where `session_id`
    /// is optional and defaults to `"default"`.
    ///
    /// Returns an error if the expression triggers an external function call,
    /// since `eval_variable` is intended for pure computation only.
    fn eval_variable_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            expression: String,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid eval_variable args: {err}"))?;

        let eval_output = self
            .manager
            .eval_variable(args.session_id.as_deref(), &args.expression)
            .map_err(|e| match e {
                SessionError::InvalidState(msg) => msg,
                SessionError::Repl(error) => format!("eval_variable failed: {error}"),
                other => other.to_string(),
            })?;

        let mut response = json!({
            "status": "ok",
            "result": eval_output.value.json_value,
        });
        if let (Some(repr), Value::Object(map)) = (eval_output.value.repr, &mut response) {
            map.insert("repr".to_owned(), json!(repr));
        }
        if !eval_output.stdout.is_empty()
            && let Value::Object(ref mut map) = response
        {
            map.insert("stdout".to_owned(), json!(eval_output.stdout));
        }
        // Add convenience `values` field for tuple results.
        maybe_add_tuple_values(&mut response);
        Ok(response)
    }
}

// =============================================================================
// Variable transfer and cross-session tools
// =============================================================================

impl McpHandler {
    /// Transfers a variable from one session to another.
    ///
    /// Reads the variable from the source session as a `Object` (which is
    /// heap-independent), then writes it into the target session via
    /// `set_variable` (which allocates fresh on the target heap). This ensures
    /// no raw `HeapId` references leak across session boundaries.
    ///
    /// Accepts `{"name": "...", "source_session_id": "...", "target_session_id": "...",
    /// "target_name": "..."}` where `target_name` is optional and defaults to `name`.
    fn transfer_variable_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            source_session_id: String,
            target_session_id: String,
            target_name: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid transfer_variable args: {err}"))?;
        let target_name_str = args.target_name.as_deref().unwrap_or(&args.name);

        self.manager
            .transfer_variable(
                &args.source_session_id,
                &args.target_session_id,
                &args.name,
                Some(target_name_str),
            )
            .map_err(|e| e.to_string())?;

        Ok(json!({ "status": "ok", "source_name": args.name, "target_name": target_name_str }))
    }

    /// Executes code in a source session and stores the result in a target session.
    ///
    /// This enables the pipeline pattern where session A's output feeds session B.
    /// The code is executed in the source session via `execute_interactive`. If
    /// execution completes successfully, the result is converted to a `Object`
    /// and stored as a variable in the target session via `set_variable`.
    ///
    /// If execution yields a `FunctionCall`, `ProxyCall`, or `ResolveFutures`
    /// progress (i.e. the source needs an external call resolved first), the
    /// progress is returned as-is and the target variable is **not** set. The
    /// caller must resolve the external call and retry.
    ///
    /// Accepts `{"code": "...", "source_session_id": "...", "target_session_id": "...",
    /// "target_variable": "..."}` where `source_session_id` is optional and
    /// defaults to `"default"`. Returns an error if source and target are the
    /// same session (use regular `execute` for that).
    fn call_session_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            code: String,
            source_session_id: Option<String>,
            target_session_id: String,
            target_variable: String,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid call_session args: {err}"))?;

        match self.manager.call_session(
            args.source_session_id.as_deref(),
            &args.target_session_id,
            &args.code,
            &args.target_variable,
        ) {
            Ok(output) => {
                if output.progress.is_complete() {
                    // Complete: serialize with source/target metadata.
                    let source_id = args.source_session_id.as_deref().unwrap_or("default");
                    let result_json = match &output.progress {
                        ReplProgress::Complete(obj) => obj.to_json_value(),
                        _ => unreachable!(),
                    };
                    let mut response = json!({
                        "status": "ok",
                        "source_session_id": source_id,
                        "target_session_id": args.target_session_id,
                        "target_variable": args.target_variable,
                        "result": result_json,
                    });
                    if !output.stdout.is_empty()
                        && let Value::Object(ref mut map) = response
                    {
                        map.insert("stdout".to_string(), json!(output.stdout));
                    }
                    // Add convenience `values` field for tuple results.
                    maybe_add_tuple_values(&mut response);
                    Ok(response)
                } else {
                    // Non-complete progress: return standard progress JSON.
                    Ok(serialize_progress(output.progress, &output.stdout))
                }
            }
            Err(SessionError::Repl(error)) => {
                let err_response = json!({
                    "status": "error",
                    "error": error.to_string()
                });
                Ok(err_response)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

// =============================================================================
// Session lifecycle tools
// =============================================================================

impl McpHandler {
    /// Resets a session with optional external functions.
    ///
    /// Accepts `{"external_functions": [...], "session_id": "..."}`. Both
    /// fields are optional. When `session_id` is omitted, resets the default
    /// session.
    fn reset_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            external_functions: Vec<String>,
            session_id: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments).map_err(|err| format!("invalid reset args: {err}"))?;
        self.manager
            .reset(args.session_id.as_deref(), args.external_functions)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "status": "ok" }))
    }

    /// Creates a new named session.
    ///
    /// Accepts `{"session_id": "...", "external_functions": [...]}`. The
    /// `external_functions` field is optional. Returns an error if a session
    /// with the given ID already exists.
    fn create_session_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            session_id: String,
            #[serde(default)]
            external_functions: Vec<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid create_session args: {err}"))?;
        self.manager
            .create_session(&args.session_id, args.external_functions)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "status": "ok", "session_id": args.session_id }))
    }

    /// Destroys a named session.
    ///
    /// Accepts `{"session_id": "..."}`. Returns an error if the session is
    /// the default session (which cannot be destroyed) or does not exist.
    fn destroy_session_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            session_id: String,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid destroy_session args: {err}"))?;
        self.manager
            .destroy_session(&args.session_id)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "status": "ok" }))
    }

    /// Lists all active sessions with their variable counts.
    ///
    /// Returns `{"sessions": [{"id": "...", "variables": N}, ...]}` sorted
    /// by session ID for deterministic output.
    fn list_sessions_tool(&self) -> Value {
        let sessions: Vec<Value> = self
            .manager
            .list_sessions()
            .into_iter()
            .map(|info| {
                json!({
                    "id": info.id,
                    "variables": info.variable_count,
                })
            })
            .collect();
        json!({ "sessions": sessions })
    }

    /// Forks an existing session into a new independent copy.
    ///
    /// Accepts `{"source_session_id": "...", "new_session_id": "..."}`. Uses
    /// `ReplSession::fork()` to produce a deep clone of the source session's
    /// heap and namespaces. Returns an error if the source does not exist or
    /// the target ID is already in use.
    fn fork_session_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            source_session_id: String,
            new_session_id: String,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid fork_session args: {err}"))?;
        self.manager
            .fork_session(&args.source_session_id, &args.new_session_id)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "status": "ok", "session_id": args.new_session_id }))
    }
}

// =============================================================================
// Session persistence tools
// =============================================================================

impl McpHandler {
    /// Saves a session to disk as a named snapshot.
    ///
    /// Accepts `{"session_id": "...", "name": "..."}` where both fields are
    /// optional. `session_id` defaults to `"default"`, `name` defaults to the
    /// `session_id`. The session must be idle (not mid-yield).
    ///
    /// The snapshot is written to `<storage_dir>/<name>.bin`. The `name` field
    /// is validated to prevent path traversal: only alphanumeric characters,
    /// hyphens, and underscores are allowed.
    fn save_session_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            session_id: Option<String>,
            name: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid save_session args: {err}"))?;

        let save_result = self
            .manager
            .save_session(args.session_id.as_deref(), args.name.as_deref())
            .map_err(|e| e.to_string())?;

        Ok(json!({ "status": "ok", "name": save_result.name, "size_bytes": save_result.size_bytes }))
    }

    /// Loads a session from a previously saved snapshot.
    ///
    /// Accepts `{"name": "...", "session_id": "..."}` where `session_id` is
    /// optional and defaults to `name`. Creates a new session from the saved
    /// state. Returns an error if the `session_id` already exists.
    ///
    /// The `name` field is validated to prevent path traversal: only
    /// alphanumeric characters, hyphens, and underscores are allowed.
    fn load_session_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid load_session args: {err}"))?;

        let session_id = self
            .manager
            .load_session(&args.name, args.session_id.as_deref())
            .map_err(|e| e.to_string())?;

        Ok(json!({ "status": "ok", "session_id": session_id, "name": args.name }))
    }

    /// Lists all saved session snapshots on disk.
    ///
    /// Scans the configured storage directory for `.bin` files and returns
    /// their names (filename without extension) and sizes in bytes. Returns
    /// an error if storage is not configured. If the directory doesn't exist
    /// or is empty, returns an empty list.
    fn list_saved_sessions_tool(&self) -> Result<Value, String> {
        let saved = self.manager.list_saved_sessions().map_err(|e| e.to_string())?;
        let sessions: Vec<Value> = saved
            .into_iter()
            .map(|info| json!({"name": info.name, "size_bytes": info.size_bytes}))
            .collect();
        Ok(json!({"sessions": sessions}))
    }
}

// =============================================================================
// Undo history tools
// =============================================================================

impl McpHandler {
    /// Rewinds a session by N steps, restoring it to a previous state.
    ///
    /// Accepts `{"steps": N, "session_id": "..."}` where `steps` defaults to 1
    /// and `session_id` is optional. Pops N entries from the history stack and
    /// replaces the current session with the state at position N in the stack.
    ///
    /// Returns `{"status": "ok", "steps_rewound": N, "history_remaining": M}`
    /// on success, or an error if N exceeds the available history depth.
    fn rewind_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default = "default_rewind_steps")]
            steps: usize,
            session_id: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments).map_err(|err| format!("invalid rewind args: {err}"))?;

        let result = self
            .manager
            .rewind(args.session_id.as_deref(), args.steps)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "status": "ok",
            "steps_rewound": result.steps_rewound,
            "history_remaining": result.history_remaining,
        }))
    }

    /// Returns the current undo history depth for a session.
    ///
    /// Accepts `{"session_id": "..."}` (optional). Returns
    /// `{"depth": N, "max_depth": M}` where N is how many undo steps are
    /// available and M is the configured maximum.
    fn history_tool(&self, arguments: &Value) -> Result<Value, String> {
        let session_id = arguments.get("session_id").and_then(Value::as_str);
        let (depth, max_depth) = self.manager.history(session_id).map_err(|e| e.to_string())?;
        Ok(json!({
            "depth": depth,
            "max_depth": max_depth,
        }))
    }

    /// Configures the maximum undo history depth for a session.
    ///
    /// Accepts `{"max_depth": N, "session_id": "..."}` where `session_id` is
    /// optional. If the new maximum is less than the current history depth,
    /// the oldest entries are trimmed from the front.
    ///
    /// Returns `{"status": "ok", "max_depth": N, "trimmed": T}` where T is
    /// the number of entries that were dropped.
    fn set_history_depth_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            max_depth: usize,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid set_history_depth args: {err}"))?;

        let trimmed = self
            .manager
            .set_history_depth(args.session_id.as_deref(), args.max_depth)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "status": "ok",
            "max_depth": args.max_depth,
            "trimmed": trimmed,
        }))
    }
}

/// Default value for the `steps` field in the `rewind` tool arguments.
fn default_rewind_steps() -> usize {
    1
}

// =============================================================================
// Heap introspection tools
// =============================================================================

impl McpHandler {
    /// Returns heap statistics for a session.
    ///
    /// Accepts `{"session_id": "..."}` (optional). Returns live object count,
    /// free slot count, total slots, per-type breakdown, and interned string
    /// count as a JSON object.
    fn heap_stats_tool(&self, arguments: &Value) -> Result<Value, String> {
        let session_id = arguments.get("session_id").and_then(Value::as_str);
        let stats = self.manager.heap_stats(session_id).map_err(|e| e.to_string())?;
        Ok(serialize_heap_stats(&stats))
    }

    /// Saves the current heap stats for a session as a named snapshot.
    ///
    /// Accepts `{"snapshot_id": "...", "session_id": "..."}`. The
    /// `session_id` is optional. The snapshot can later be compared using
    /// `diff_heap`. Overwrites any existing snapshot with the same name.
    fn snapshot_heap_tool(&mut self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(alias = "snapshot_name")]
            snapshot_id: String,
            session_id: Option<String>,
        }

        let args: Args =
            serde_json::from_value(arguments).map_err(|err| format!("invalid snapshot_heap args: {err}"))?;

        self.manager
            .snapshot_heap(args.session_id.as_deref(), &args.snapshot_id)
            .map_err(|e| e.to_string())?;

        Ok(json!({ "status": "ok", "snapshot_id": args.snapshot_id }))
    }

    /// Compares two heap states and returns the diff.
    ///
    /// Supports two modes:
    ///
    /// 1. **Snapshot comparison**: `{"before_snapshot_id": "before", "after_snapshot_id": "after"}`
    ///    Compares two previously saved snapshots by name.
    ///
    /// 2. **Cross-session comparison**: `{"before_session_id": "default", "after_session_id": "experiment"}`
    ///    Compares the *current* heap state of two sessions.
    ///
    /// Returns error if the referenced snapshots or sessions are not found.
    fn diff_heap_tool(&self, arguments: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        #[expect(clippy::struct_field_names, reason = "user-facing MCP API field names")]
        struct Args {
            #[serde(alias = "snapshot_a")]
            before_snapshot_id: Option<String>,
            #[serde(alias = "snapshot_b")]
            after_snapshot_id: Option<String>,
            #[serde(alias = "session_a")]
            before_session_id: Option<String>,
            #[serde(alias = "session_b")]
            after_session_id: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments).map_err(|err| format!("invalid diff_heap args: {err}"))?;

        let diff_result = if let (Some(snap_a), Some(snap_b)) = (&args.before_snapshot_id, &args.after_snapshot_id) {
            self.manager.diff_heap(snap_a, snap_b).map_err(|e| e.to_string())?
        } else if let (Some(sess_a), Some(sess_b)) = (&args.before_session_id, &args.after_session_id) {
            self.manager
                .diff_heap_sessions(sess_a, sess_b)
                .map_err(|e| e.to_string())?
        } else {
            return Err(
                "diff_heap requires either (before_snapshot_id, after_snapshot_id) or (before_session_id, after_session_id)"
                    .to_owned(),
            );
        };

        Ok(serialize_heap_diff(&diff_result))
    }
}

// =============================================================================
// JSON serialization helpers
// =============================================================================

/// Creates a `ToolDefinition` from a name and description.
fn tool(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: description.to_owned(),
    }
}

/// Serializes an `ExecuteOutput` (progress + stdout) into MCP JSON.
///
/// Handles all four progress variants: complete, function_call, proxy_call,
/// and resolve_futures. Includes stdout and tuple values fields when applicable.
fn serialize_execute_output(output: ExecuteOutput) -> Value {
    serialize_progress(output.progress, &output.stdout)
}

/// Converts a `ReplProgress` into a JSON value. Includes stdout output if non-empty.
///
/// This is a pure serialization function -- it does not mutate any session state.
/// The pending_call_id updates are handled internally by `SessionManager`.
fn serialize_progress(progress: ReplProgress, stdout: &str) -> Value {
    let mut result = match progress {
        ReplProgress::Complete(result) => {
            let is_none = matches!(result, Object::None);
            let mut complete = json!({"status": "complete"});
            if !is_none && let Value::Object(ref mut map) = complete {
                map.insert("result".to_string(), result.to_json_value());
                map.insert("repr".to_string(), json!(result.py_repr()));
            }
            complete
        }
        ReplProgress::FunctionCall {
            function_name,
            args,
            kwargs,
            call_id,
        } => {
            let json_args: Vec<Value> = args.iter().map(Object::to_json_value).collect();
            let json_kwargs: serde_json::Map<String, Value> = kwargs
                .iter()
                .map(|(k, v)| (kwarg_key_to_string(k), v.to_json_value()))
                .collect();
            json!({
                "status": "function_call",
                "function_name": function_name,
                "args": json_args,
                "kwargs": json_kwargs,
                "call_id": call_id,
            })
        }
        ReplProgress::ProxyCall {
            proxy_id,
            method,
            args,
            kwargs,
            call_id,
        } => {
            let json_args: Vec<Value> = args.iter().map(Object::to_json_value).collect();
            let json_kwargs: serde_json::Map<String, Value> = kwargs
                .iter()
                .map(|(k, v)| (kwarg_key_to_string(k), v.to_json_value()))
                .collect();
            json!({
                "status": "proxy_call",
                "proxy_id": proxy_id,
                "method": method,
                "args": json_args,
                "kwargs": json_kwargs,
                "call_id": call_id,
            })
        }
        ReplProgress::ResolveFutures {
            pending_call_ids,
            pending_futures,
        } => {
            let futures_json: Vec<Value> = pending_futures
                .iter()
                .map(|info| {
                    let json_args: Vec<Value> = info.args.iter().map(Object::to_json_value).collect();
                    json!({
                        "call_id": info.call_id,
                        "function_name": info.function_name,
                        "args": json_args,
                    })
                })
                .collect();
            json!({
                "status": "resolve_futures",
                "pending_call_ids": pending_call_ids,
                "pending_futures": futures_json,
            })
        }
    };

    // Add stdout field if there was any output.
    if !stdout.is_empty()
        && let Value::Object(ref mut map) = result
    {
        map.insert("stdout".to_string(), json!(stdout));
    }

    // Add convenience `values` field for tuple results.
    maybe_add_tuple_values(&mut result);

    result
}

/// Adds a top-level `"values"` convenience field to a JSON response when its
/// `"result"` or `"value"` field contains a `{"$tuple": [...]}` tagged encoding.
///
/// Orchestrators can use `values` directly as a plain JSON array instead of
/// having to unwrap the `$tuple` tag. The original tagged encoding is preserved
/// for faithful round-tripping. For non-tuple results this is a no-op.
fn maybe_add_tuple_values(response: &mut Value) {
    let Some(map) = response.as_object_mut() else {
        return;
    };

    // Check both "result" (eval_variable, execute, call_session) and "value" (get_variable).
    for field in &["result", "value"] {
        if let Some(inner) = map.get(*field).and_then(|v| v.as_object())
            && let Some(arr) = inner.get("$tuple")
        {
            let values = arr.clone();
            map.insert("values".to_owned(), values);
            return;
        }
    }
}

/// Extracts a string key from a `Object` used as a kwarg key.
///
/// Python kwargs are always string-keyed, so this extracts the inner string
/// directly when possible, falling back to `py_repr()` for other types.
fn kwarg_key_to_string(key: &Object) -> String {
    match key {
        Object::String(s) => s.clone(),
        other => other.py_repr(),
    }
}

/// Serializes `HeapStats` to a JSON value suitable for MCP tool output.
fn serialize_heap_stats(stats: &HeapStats) -> Value {
    let objects_by_type: serde_json::Map<String, Value> = stats
        .objects_by_type
        .iter()
        .map(|(k, v)| ((*k).to_owned(), json!(v)))
        .collect();
    let mut result = json!({
        "live_objects": stats.live_objects,
        "free_slots": stats.free_slots,
        "total_slots": stats.total_slots,
        "objects_by_type": objects_by_type,
        "interned_strings": stats.interned_strings,
        "note": "live_objects counts only heap-allocated (ref-counted) objects. Primitive values like int, float, bool, and None are stack-inlined and not counted."
    });
    if let Some(allocs) = stats.tracker_allocations {
        result["tracker_allocations"] = json!(allocs);
    }
    if let Some(mem) = stats.tracker_memory_bytes {
        result["tracker_memory_bytes"] = json!(mem);
    }
    result
}

/// Serializes a `HeapDiffResult` (heap diff + variable-level deltas) to MCP tool output JSON.
fn serialize_heap_diff(diff_result: &HeapDiffResult) -> Value {
    let diff = &diff_result.heap_diff;
    let objects_by_type_delta: serde_json::Map<String, Value> = diff
        .objects_by_type_delta
        .iter()
        .map(|(k, v)| ((*k).to_owned(), json!(v)))
        .collect();

    let variable_diff = serialize_variable_diff(&diff_result.variable_diff);

    let mut result = json!({
        "live_objects_delta": diff.live_objects_delta,
        "free_slots_delta": diff.free_slots_delta,
        "total_slots_delta": diff.total_slots_delta,
        "objects_by_type_delta": objects_by_type_delta,
        "new_types": diff.new_types,
        "removed_types": diff.removed_types,
        "interned_strings_delta": diff.interned_strings_delta,
        "variables": variable_diff,
    });
    if let Some(allocs) = diff.tracker_allocations_delta {
        result["tracker_allocations_delta"] = json!(allocs);
    }
    if let Some(mem) = diff.tracker_memory_bytes_delta {
        result["tracker_memory_bytes_delta"] = json!(mem);
    }
    result
}

/// Serializes a `VariableDiff` to a JSON value for inclusion in heap diff output.
fn serialize_variable_diff(vd: &VariableDiff) -> Value {
    let changed: Vec<Value> = vd
        .changed
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "before": c.before,
                "after": c.after,
            })
        })
        .collect();

    json!({
        "added": vd.added,
        "removed": vd.removed,
        "changed": changed,
        "unchanged": vd.unchanged,
    })
}
