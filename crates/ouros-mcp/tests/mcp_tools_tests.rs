use ouros_mcp::handler::McpHandler;
use serde_json::json;

// =============================================================================
// Existing tests — backward compatibility (default session, no session_id)
// =============================================================================

#[test]
fn tools_list_returns_expected_tool_definitions() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    // Original 5 tools still present, plus new session/heap tools.
    assert!(names.contains(&"execute"));
    assert!(names.contains(&"resume"));
    assert!(names.contains(&"list_variables"));
    assert!(names.contains(&"get_variable"));
    assert!(names.contains(&"reset"));
    // New tools.
    assert!(names.contains(&"create_session"));
    assert!(names.contains(&"destroy_session"));
    assert!(names.contains(&"list_sessions"));
    assert!(names.contains(&"fork_session"));
    assert!(names.contains(&"heap_stats"));
    assert!(names.contains(&"snapshot_heap"));
    assert!(names.contains(&"diff_heap"));
}

#[test]
fn execute_tool_runs_code_and_returns_result() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "1 + 2"})).unwrap();
    assert_eq!(result, json!({"status": "complete", "result": 3, "repr": "3"}));
}

#[test]
fn execute_tool_omits_result_and_repr_for_none() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "x = 3"})).unwrap();
    assert_eq!(result, json!({"status": "complete"}));
    assert!(
        result.get("result").is_none(),
        "result should be omitted for None results"
    );
    assert!(result.get("repr").is_none(), "repr should be omitted for None results");
}

#[test]
fn execute_tool_returns_string_result() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "'hello'"})).unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["result"], json!("hello"));
    assert_eq!(result["repr"], json!("'hello'"));
}

#[test]
fn execute_tool_returns_result_from_trailing_expression() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "x = 10\nx + 5"})).unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["result"], json!(15));
    assert_eq!(result["repr"], json!("15"));
}

#[test]
fn execute_tool_omits_result_for_print_statement() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "print('hi')"})).unwrap();
    assert_eq!(result["status"], "complete");
    assert!(
        result.get("result").is_none(),
        "result should be omitted for print (returns None)"
    );
    assert!(
        result.get("repr").is_none(),
        "repr should be omitted for print (returns None)"
    );
    assert_eq!(result["stdout"], json!("hi\n"));
}

#[test]
fn execute_tool_with_external_function_returns_function_call() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let result = handler.call_tool("execute", json!({"code": "fetch(1)"})).unwrap();
    assert_eq!(result["status"], json!("function_call"));
    assert_eq!(result["function_name"], json!("fetch"));
    assert_eq!(result["args"], json!([1]));
}

#[test]
fn resume_tool_continues_execution() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let progress = handler.call_tool("execute", json!({"code": "fetch(1) + 1"})).unwrap();
    let call_id = progress["call_id"].as_u64().expect("call_id should be present");

    let result = handler
        .call_tool(
            "resume",
            json!({
                "call_id": call_id,
                "result": {"Int": 41}
            }),
        )
        .unwrap();
    assert_eq!(result, json!({"status": "complete", "result": 42, "repr": "42"}));
}

// =============================================================================
// Existing tools with explicit session_id targeting the default session
// =============================================================================

#[test]
fn execute_with_explicit_default_session_id_works() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler
        .call_tool("execute", json!({"code": "2 + 3", "session_id": "default"}))
        .unwrap();
    assert_eq!(result, json!({"status": "complete", "result": 5, "repr": "5"}));
}

#[test]
fn list_variables_with_session_id_works() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 42"})).unwrap();
    let result = handler
        .call_tool("list_variables", json!({"session_id": "default"}))
        .unwrap();
    let variables = result["variables"].as_array().unwrap();
    assert!(variables.iter().any(|v| v["name"] == "x"));
}

#[test]
fn get_variable_with_session_id_works() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "y = 99"})).unwrap();
    let result = handler
        .call_tool("get_variable", json!({"name": "y", "session_id": "default"}))
        .unwrap();
    assert_eq!(result["name"], json!("y"));
}

#[test]
fn reset_with_session_id_resets_that_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "z = 1"})).unwrap();
    handler.call_tool("reset", json!({"session_id": "default"})).unwrap();
    let result = handler
        .call_tool("list_variables", json!({"session_id": "default"}))
        .unwrap();
    let variables = result["variables"].as_array().unwrap();
    assert!(variables.is_empty());
}

// =============================================================================
// Session management: create, destroy, list
// =============================================================================

#[test]
fn create_session_creates_named_session() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler
        .call_tool("create_session", json!({"session_id": "analysis"}))
        .unwrap();
    assert_eq!(result, json!({"status": "ok", "session_id": "analysis"}));
}

#[test]
fn create_session_with_external_functions() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler
        .call_tool(
            "create_session",
            json!({"session_id": "ext", "external_functions": ["fetch", "send"]}),
        )
        .unwrap();
    assert_eq!(result["status"], json!("ok"));

    // Verify external functions work in the new session.
    let progress = handler
        .call_tool("execute", json!({"code": "fetch(1)", "session_id": "ext"}))
        .unwrap();
    assert_eq!(progress["status"], json!("function_call"));
    assert_eq!(progress["function_name"], json!("fetch"));
}

#[test]
fn create_session_duplicate_id_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "dup"}))
        .unwrap();
    let err = handler
        .call_tool("create_session", json!({"session_id": "dup"}))
        .unwrap_err();
    assert!(
        err.contains("already exists"),
        "Expected 'already exists' error, got: {err}"
    );
}

#[test]
fn create_session_named_default_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler
        .call_tool("create_session", json!({"session_id": "default"}))
        .unwrap_err();
    assert!(
        err.contains("already exists"),
        "Expected 'already exists' error, got: {err}"
    );
}

#[test]
fn destroy_session_removes_named_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "temp"}))
        .unwrap();
    let result = handler
        .call_tool("destroy_session", json!({"session_id": "temp"}))
        .unwrap();
    assert_eq!(result, json!({"status": "ok"}));

    // Verify session is gone.
    let err = handler
        .call_tool("execute", json!({"code": "1", "session_id": "temp"}))
        .unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

#[test]
fn destroy_default_session_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler
        .call_tool("destroy_session", json!({"session_id": "default"}))
        .unwrap_err();
    assert!(
        err.contains("cannot destroy"),
        "Expected 'cannot destroy' error, got: {err}"
    );
}

#[test]
fn destroy_nonexistent_session_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler
        .call_tool("destroy_session", json!({"session_id": "nope"}))
        .unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

#[test]
fn list_sessions_shows_all_sessions() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "alpha"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "beta"}))
        .unwrap();

    let result = handler.call_tool("list_sessions", json!({})).unwrap();
    let sessions = result["sessions"].as_array().unwrap();
    let ids: Vec<&str> = sessions.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"default"));
    assert!(ids.contains(&"alpha"));
    assert!(ids.contains(&"beta"));
    assert_eq!(ids.len(), 3);
}

#[test]
fn list_sessions_shows_variable_count() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "a = 1\nb = 2"})).unwrap();

    let result = handler.call_tool("list_sessions", json!({})).unwrap();
    let sessions = result["sessions"].as_array().unwrap();
    let default_entry = sessions.iter().find(|s| s["id"] == "default").unwrap();
    assert!(
        default_entry["variables"].as_u64().unwrap() >= 2,
        "Expected at least 2 variables, got: {}",
        default_entry["variables"]
    );
}

// =============================================================================
// Executing in named sessions
// =============================================================================

#[test]
fn execute_in_named_session_is_isolated() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "sandbox"}))
        .unwrap();

    // Set variable in default session.
    handler.call_tool("execute", json!({"code": "x = 100"})).unwrap();
    // Set different variable in sandbox.
    handler
        .call_tool("execute", json!({"code": "y = 200", "session_id": "sandbox"}))
        .unwrap();

    // Default session should have x but not y.
    let default_vars = handler
        .call_tool("list_variables", json!({"session_id": "default"}))
        .unwrap();
    let default_names: Vec<&str> = default_vars["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert!(default_names.contains(&"x"));
    assert!(!default_names.contains(&"y"));

    // Sandbox session should have y but not x.
    let sandbox_vars = handler
        .call_tool("list_variables", json!({"session_id": "sandbox"}))
        .unwrap();
    let sandbox_names: Vec<&str> = sandbox_vars["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert!(sandbox_names.contains(&"y"));
    assert!(!sandbox_names.contains(&"x"));
}

#[test]
fn execute_in_nonexistent_session_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler
        .call_tool("execute", json!({"code": "1", "session_id": "ghost"}))
        .unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

// =============================================================================
// Fork session
// =============================================================================

#[test]
fn fork_session_creates_independent_copy() {
    let mut handler = McpHandler::new("<mcp>");
    // Set up state in default session.
    handler.call_tool("execute", json!({"code": "x = 10"})).unwrap();

    // Fork it.
    let result = handler
        .call_tool(
            "fork_session",
            json!({"source_session_id": "default", "new_session_id": "experiment"}),
        )
        .unwrap();
    assert_eq!(result, json!({"status": "ok", "session_id": "experiment"}));

    // Forked session should have x.
    let fork_result = handler
        .call_tool("execute", json!({"code": "x", "session_id": "experiment"}))
        .unwrap();
    assert_eq!(fork_result["result"], json!(10));

    // Mutate in forked session.
    handler
        .call_tool("execute", json!({"code": "x = 999", "session_id": "experiment"}))
        .unwrap();

    // Original should be unchanged.
    let original_result = handler
        .call_tool("execute", json!({"code": "x", "session_id": "default"}))
        .unwrap();
    assert_eq!(original_result["result"], json!(10));
}

#[test]
fn fork_nonexistent_session_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler
        .call_tool(
            "fork_session",
            json!({"source_session_id": "ghost", "new_session_id": "copy"}),
        )
        .unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

#[test]
fn fork_to_existing_session_id_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "existing"}))
        .unwrap();
    let err = handler
        .call_tool(
            "fork_session",
            json!({"source_session_id": "default", "new_session_id": "existing"}),
        )
        .unwrap_err();
    assert!(
        err.contains("already exists"),
        "Expected 'already exists' error, got: {err}"
    );
}

// =============================================================================
// Heap stats
// =============================================================================

#[test]
fn heap_stats_returns_valid_data_for_default_session() {
    let mut handler = McpHandler::new("<mcp>");
    // Create some objects to make the heap non-trivial.
    handler
        .call_tool("execute", json!({"code": "a = [1, 2, 3]\nb = 'hello'"}))
        .unwrap();

    let result = handler.call_tool("heap_stats", json!({})).unwrap();
    assert!(result["live_objects"].as_u64().unwrap() > 0);
    assert!(result["total_slots"].as_u64().unwrap() > 0);
    assert!(result.get("objects_by_type").is_some());
    assert!(result.get("interned_strings").is_some());
}

#[test]
fn heap_stats_for_named_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "analysis"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "x = [1, 2]", "session_id": "analysis"}))
        .unwrap();

    let result = handler
        .call_tool("heap_stats", json!({"session_id": "analysis"}))
        .unwrap();
    assert!(result["live_objects"].as_u64().unwrap() > 0);
}

// =============================================================================
// Snapshot and diff heap
// =============================================================================

#[test]
fn snapshot_heap_creates_named_snapshot() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler
        .call_tool("snapshot_heap", json!({"snapshot_name": "before"}))
        .unwrap();
    assert_eq!(result["status"], json!("ok"));
}

#[test]
fn diff_heap_between_two_snapshots() {
    let mut handler = McpHandler::new("<mcp>");

    handler
        .call_tool("execute", json!({"code": "counter = 0\ndata = []"}))
        .unwrap();

    // Snapshot before.
    handler
        .call_tool("snapshot_heap", json!({"snapshot_name": "before"}))
        .unwrap();

    // Update and add variables.
    handler
        .call_tool("execute", json!({"code": "counter = 5\nnew_var = [1, 2, 3]"}))
        .unwrap();

    // Snapshot after.
    handler
        .call_tool("snapshot_heap", json!({"snapshot_name": "after"}))
        .unwrap();

    // Diff.
    let result = handler
        .call_tool("diff_heap", json!({"snapshot_a": "before", "snapshot_b": "after"}))
        .unwrap();
    assert!(result.get("live_objects_delta").is_some());
    assert!(result.get("objects_by_type_delta").is_some());
    assert_eq!(result["variables"]["added"], json!(["new_var"]));
    assert_eq!(result["variables"]["removed"], json!([]));
    assert_eq!(result["variables"]["unchanged"], json!(["data"]));
    assert_eq!(
        result["variables"]["changed"],
        json!([{"name": "counter", "before": "0", "after": "5"}])
    );
}

#[test]
fn diff_heap_cross_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "sess_a"}))
        .unwrap();

    handler
        .call_tool("execute", json!({"code": "shared = 1\nonly_a = 'left'"}))
        .unwrap();

    // Add data to sess_a.
    handler
        .call_tool(
            "execute",
            json!({"code": "shared = 2\nonly_b = 'right'", "session_id": "sess_a"}),
        )
        .unwrap();

    let result = handler
        .call_tool("diff_heap", json!({"session_a": "default", "session_b": "sess_a"}))
        .unwrap();
    assert!(result.get("live_objects_delta").is_some());
    assert_eq!(result["variables"]["added"], json!(["only_b"]));
    assert_eq!(result["variables"]["removed"], json!(["only_a"]));
    assert_eq!(result["variables"]["unchanged"], json!([]));
    assert_eq!(
        result["variables"]["changed"],
        json!([{"name": "shared", "before": "1", "after": "2"}])
    );
}

#[test]
fn diff_heap_nonexistent_snapshot_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler
        .call_tool("diff_heap", json!({"snapshot_a": "nope", "snapshot_b": "nada"}))
        .unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

#[test]
fn snapshot_heap_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "monitored"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "x = 42", "session_id": "monitored"}))
        .unwrap();
    let result = handler
        .call_tool(
            "snapshot_heap",
            json!({"snapshot_name": "snap1", "session_id": "monitored"}),
        )
        .unwrap();
    assert_eq!(result["status"], json!("ok"));
}

// =============================================================================
// Resume with session_id
// =============================================================================

#[test]
fn resume_in_named_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool(
            "create_session",
            json!({"session_id": "with_ext", "external_functions": ["query"]}),
        )
        .unwrap();

    let progress = handler
        .call_tool(
            "execute",
            json!({"code": "query('hello') + 1", "session_id": "with_ext"}),
        )
        .unwrap();
    let call_id = progress["call_id"].as_u64().expect("call_id should be present");

    let result = handler
        .call_tool(
            "resume",
            json!({"call_id": call_id, "result": {"Int": 99}, "session_id": "with_ext"}),
        )
        .unwrap();
    assert_eq!(result, json!({"status": "complete", "result": 100, "repr": "100"}));
}

// =============================================================================
// Custom __repr__ tests
// =============================================================================

#[test]
fn get_variable_invokes_custom_repr() {
    let mut handler = McpHandler::new("<mcp>");

    // Define a class with custom __repr__
    let code = r"
class Pt:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __repr__(self):
        return f'Pt({self.x}, {self.y})'

my_pt = Pt(3, 4)
";

    handler.call_tool("execute", json!({"code": code})).unwrap();

    // Get the variable and check repr
    let result = handler.call_tool("get_variable", json!({"name": "my_pt"})).unwrap();

    assert_eq!(result["name"], json!("my_pt"));
    assert!(result["value"].is_object(), "value should be present");
    assert_eq!(result["repr"], json!("Pt(3, 4)"), "Custom __repr__ should be invoked");
}

#[test]
fn get_variable_repr_without_custom_repr() {
    let mut handler = McpHandler::new("<mcp>");

    // Define a class without custom __repr__
    let code = r"
class NoRepr:
    def __init__(self, val):
        self.val = val

obj = NoRepr(42)
";

    handler.call_tool("execute", json!({"code": code})).unwrap();

    // Get the variable and check repr has default format
    let result = handler.call_tool("get_variable", json!({"name": "obj"})).unwrap();

    assert_eq!(result["name"], json!("obj"));
    assert!(result["value"].is_object(), "value should be present");

    // Should have default <module.ClassName object at 0x...> format
    let repr_str = result["repr"].as_str().unwrap();
    assert!(
        repr_str.contains("NoRepr object at 0x"),
        "Default repr should contain 'NoRepr object at 0x...', got: {repr_str}",
    );
}

// =============================================================================
// set_variable tool tests
// =============================================================================

/// The set_variable tool sets an integer and confirms via get_variable.
#[test]
fn set_variable_tool_basic() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler
        .call_tool("set_variable", json!({"name": "x", "value": 42}))
        .unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["name"], "x");

    // Verify the variable is set.
    let get_result = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(get_result["value"], 42);
}

/// The set_variable tool sets a string value.
#[test]
fn set_variable_tool_string() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("set_variable", json!({"name": "s", "value": "hello"}))
        .unwrap();
    let result = handler.call_tool("get_variable", json!({"name": "s"})).unwrap();
    assert_eq!(result["value"], "hello");
}

/// The set_variable tool works with an explicit session_id.
#[test]
fn set_variable_tool_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool(
            "create_session",
            json!({"session_id": "test_session", "external_functions": []}),
        )
        .unwrap();
    handler
        .call_tool(
            "set_variable",
            json!({"name": "x", "value": 99, "session_id": "test_session"}),
        )
        .unwrap();
    let result = handler
        .call_tool("get_variable", json!({"name": "x", "session_id": "test_session"}))
        .unwrap();
    assert_eq!(result["value"], 99);
}

/// The set_variable tool overwrites an existing variable.
#[test]
fn set_variable_tool_overwrite() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("set_variable", json!({"name": "x", "value": 1}))
        .unwrap();
    handler
        .call_tool("set_variable", json!({"name": "x", "value": 2}))
        .unwrap();
    let result = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(result["value"], 2);
}

/// A variable set via set_variable is usable by the execute tool.
#[test]
fn set_variable_tool_then_execute_uses_it() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("set_variable", json!({"name": "x", "value": 10}))
        .unwrap();
    let result = handler.call_tool("execute", json!({"code": "x * 2"})).unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["result"], 20);
}

/// The set_variable tool handles list (JSON array) values.
#[test]
fn set_variable_tool_list() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("set_variable", json!({"name": "nums", "value": [1, 2, 3]}))
        .unwrap();
    let result = handler.call_tool("get_variable", json!({"name": "nums"})).unwrap();
    assert_eq!(result["value"], json!([1, 2, 3]));
}

/// The set_variable tool handles null (Python None).
#[test]
fn set_variable_tool_null() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("set_variable", json!({"name": "n", "value": null}))
        .unwrap();
    let result = handler.call_tool("get_variable", json!({"name": "n"})).unwrap();
    assert!(result["value"].is_null());
}

// =============================================================================
// delete_variable tool tests
// =============================================================================

/// The delete_variable tool removes an existing variable.
#[test]
fn delete_variable_tool_basic() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("set_variable", json!({"name": "x", "value": 42}))
        .unwrap();
    let result = handler.call_tool("delete_variable", json!({"name": "x"})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["existed"], true);

    // Variable should be gone (get_variable returns null for absent vars).
    let get_result = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert!(
        get_result["value"].is_null(),
        "deleted variable should return null value"
    );
}

/// The delete_variable tool reports existed=false for nonexistent variables.
#[test]
fn delete_variable_tool_nonexistent() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("delete_variable", json!({"name": "nope"})).unwrap();
    assert_eq!(result["existed"], false);
}

/// The delete_variable tool works with explicit session_id.
#[test]
fn delete_variable_tool_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "del_sess"}))
        .unwrap();
    handler
        .call_tool(
            "set_variable",
            json!({"name": "y", "value": 5, "session_id": "del_sess"}),
        )
        .unwrap();
    let result = handler
        .call_tool("delete_variable", json!({"name": "y", "session_id": "del_sess"}))
        .unwrap();
    assert_eq!(result["existed"], true);
}

// =============================================================================
// transfer_variable tool tests
// =============================================================================

/// Transferring a variable between sessions copies the value to the target.
#[test]
fn transfer_variable_between_sessions() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "source"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "target"}))
        .unwrap();

    // Set variable in source.
    handler
        .call_tool(
            "set_variable",
            json!({"name": "data", "value": 42, "session_id": "source"}),
        )
        .unwrap();

    // Transfer to target.
    let result = handler
        .call_tool(
            "transfer_variable",
            json!({
                "name": "data",
                "source_session_id": "source",
                "target_session_id": "target"
            }),
        )
        .unwrap();
    assert_eq!(result["status"], "ok");

    // Verify in target.
    let get_result = handler
        .call_tool("get_variable", json!({"name": "data", "session_id": "target"}))
        .unwrap();
    assert_eq!(get_result["value"], 42);
}

/// Transfer with a target_name renames the variable in the target session.
#[test]
fn transfer_variable_with_rename() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "src"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();

    handler
        .call_tool(
            "set_variable",
            json!({"name": "old_name", "value": "hello", "session_id": "src"}),
        )
        .unwrap();

    handler
        .call_tool(
            "transfer_variable",
            json!({
                "name": "old_name",
                "source_session_id": "src",
                "target_session_id": "dst",
                "target_name": "new_name"
            }),
        )
        .unwrap();

    let result = handler
        .call_tool("get_variable", json!({"name": "new_name", "session_id": "dst"}))
        .unwrap();
    assert_eq!(result["value"], "hello");
}

/// Transfer preserves complex structures (dict with nested list) created via execute.
#[test]
fn transfer_variable_complex_type() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("create_session", json!({"session_id": "a"})).unwrap();
    handler.call_tool("create_session", json!({"session_id": "b"})).unwrap();

    // Create a complex structure via execute.
    handler
        .call_tool(
            "execute",
            json!({"code": "data = {'key': [1, 2, 3], 'nested': {'x': True}}", "session_id": "a"}),
        )
        .unwrap();

    // Transfer it.
    handler
        .call_tool(
            "transfer_variable",
            json!({
                "name": "data",
                "source_session_id": "a",
                "target_session_id": "b"
            }),
        )
        .unwrap();

    // Use it in target session.
    let result = handler
        .call_tool("execute", json!({"code": "data['key'][1]", "session_id": "b"}))
        .unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["result"], 2);
}

/// Transferring a nonexistent variable from the source session returns an error.
#[test]
fn transfer_variable_nonexistent_source() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "src"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();

    let result = handler.call_tool(
        "transfer_variable",
        json!({
            "name": "nope",
            "source_session_id": "src",
            "target_session_id": "dst"
        }),
    );
    assert!(result.is_err(), "transfer of nonexistent variable should fail");
}

/// Transfer does not remove the variable from the source session (it is a copy).
#[test]
fn transfer_variable_preserves_source() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "src"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();

    handler
        .call_tool("set_variable", json!({"name": "x", "value": 99, "session_id": "src"}))
        .unwrap();

    handler
        .call_tool(
            "transfer_variable",
            json!({
                "name": "x",
                "source_session_id": "src",
                "target_session_id": "dst"
            }),
        )
        .unwrap();

    // Source still has the variable.
    let src_result = handler
        .call_tool("get_variable", json!({"name": "x", "session_id": "src"}))
        .unwrap();
    assert_eq!(src_result["value"], 99);
}

/// Transfer to the default session (no create_session needed) works.
#[test]
fn transfer_variable_to_default_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "origin"}))
        .unwrap();

    handler
        .call_tool(
            "set_variable",
            json!({"name": "val", "value": 7, "session_id": "origin"}),
        )
        .unwrap();

    handler
        .call_tool(
            "transfer_variable",
            json!({
                "name": "val",
                "source_session_id": "origin",
                "target_session_id": "default"
            }),
        )
        .unwrap();

    let result = handler.call_tool("get_variable", json!({"name": "val"})).unwrap();
    assert_eq!(result["value"], 7);
}

// =============================================================================
// resume_as_pending / resume_futures tool tests
// =============================================================================

/// The tool list includes both resume_as_pending and resume_futures.
#[test]
fn tools_list_contains_resume_as_pending_and_resume_futures() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"resume_as_pending"), "missing resume_as_pending tool");
    assert!(names.contains(&"resume_futures"), "missing resume_futures tool");
}

/// resume_as_pending converts a pending external call into an ExternalFuture.
/// For simple assignment (`x = fetch(...)`), execution completes because the
/// future value is just stored without being awaited.
#[test]
fn resume_as_pending_creates_future_and_completes() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let result = handler
        .call_tool("execute", json!({"code": "x = fetch('url')"}))
        .unwrap();
    assert_eq!(result["status"], "function_call");
    let call_id = result["call_id"].as_u64().unwrap();

    let result = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id}))
        .unwrap();
    // x holds an ExternalFuture, execution completes since it is not awaited.
    assert_eq!(result["status"], "complete");
}

/// resume_as_pending rejects a mismatched call_id.
#[test]
fn resume_as_pending_rejects_wrong_call_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let result = handler.call_tool("execute", json!({"code": "fetch('url')"})).unwrap();
    assert_eq!(result["status"], "function_call");

    let err = handler
        .call_tool("resume_as_pending", json!({"call_id": 999}))
        .unwrap_err();
    assert!(err.contains("does not match"), "expected mismatch error, got: {err}");
}

/// Full async gather flow via MCP tools: execute -> function_call -> resume_as_pending
/// -> function_call -> resume_as_pending -> resolve_futures -> resume_futures -> complete.
#[test]
fn async_gather_via_mcp_tools() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["async_call"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    // Step 1: execute -- should yield first external call.
    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    assert_eq!(progress["status"], "function_call");
    assert_eq!(progress["function_name"], "async_call");
    assert_eq!(progress["args"], json!([1]));
    let call_id_0 = progress["call_id"].as_u64().unwrap();

    // Step 2: resume_as_pending for first call.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    assert_eq!(progress["status"], "function_call");
    assert_eq!(progress["function_name"], "async_call");
    assert_eq!(progress["args"], json!([2]));
    let call_id_1 = progress["call_id"].as_u64().unwrap();

    // Step 3: resume_as_pending for second call.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_1}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");
    let pending = progress["pending_call_ids"].as_array().unwrap();
    assert_eq!(pending.len(), 2);

    // Step 4: resume_futures with both results.
    let progress = handler
        .call_tool(
            "resume_futures",
            json!({
                "results": [
                    {"call_id": call_id_0, "result": 10},
                    {"call_id": call_id_1, "result": 20}
                ]
            }),
        )
        .unwrap();
    assert_eq!(progress["status"], "complete");

    // Verify gathered result via get_variable.
    let var = handler.call_tool("get_variable", json!({"name": "result"})).unwrap();
    assert_eq!(var["value"], json!([10, 20]));
}

/// resume_futures supports incremental resolution: provide one result at a time.
#[test]
fn resume_futures_incremental_via_mcp() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["async_call"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    let call_id_0 = progress["call_id"].as_u64().unwrap();

    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    let call_id_1 = progress["call_id"].as_u64().unwrap();

    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_1}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    // Resolve only first future.
    let progress = handler
        .call_tool(
            "resume_futures",
            json!({"results": [{"call_id": call_id_0, "result": 100}]}),
        )
        .unwrap();
    assert_eq!(
        progress["status"], "resolve_futures",
        "should still be blocked with one pending future"
    );
    let remaining = progress["pending_call_ids"].as_array().unwrap();
    assert_eq!(remaining.len(), 1);

    // Resolve second future.
    let progress = handler
        .call_tool(
            "resume_futures",
            json!({"results": [{"call_id": call_id_1, "result": 200}]}),
        )
        .unwrap();
    assert_eq!(progress["status"], "complete");

    let var = handler.call_tool("get_variable", json!({"name": "result"})).unwrap();
    assert_eq!(var["value"], json!([100, 200]));
}

/// resume_futures with invalid call_id returns an error response.
#[test]
fn resume_futures_invalid_call_id_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["async_call"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    let call_id_0 = progress["call_id"].as_u64().unwrap();
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    let call_id_1 = progress["call_id"].as_u64().unwrap();
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_1}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    // Pass a bogus call_id.
    let result = handler
        .call_tool("resume_futures", json!({"results": [{"call_id": 999, "result": 0}]}))
        .unwrap();
    // The tool returns an error in the JSON status (not Err), since the VM handles it.
    assert_eq!(result["status"], "error");
}

/// resume_as_pending works with explicit session_id.
#[test]
fn resume_as_pending_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool(
            "create_session",
            json!({"session_id": "async_sess", "external_functions": ["fetch"]}),
        )
        .unwrap();

    let result = handler
        .call_tool("execute", json!({"code": "x = fetch(1)", "session_id": "async_sess"}))
        .unwrap();
    assert_eq!(result["status"], "function_call");
    let call_id = result["call_id"].as_u64().unwrap();

    let result = handler
        .call_tool(
            "resume_as_pending",
            json!({"call_id": call_id, "session_id": "async_sess"}),
        )
        .unwrap();
    assert_eq!(result["status"], "complete");
}

/// resume_futures works with explicit session_id.
#[test]
fn resume_futures_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool(
            "create_session",
            json!({"session_id": "async_sess", "external_functions": ["async_call"]}),
        )
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = handler
        .call_tool("execute", json!({"code": code, "session_id": "async_sess"}))
        .unwrap();
    let call_id_0 = progress["call_id"].as_u64().unwrap();

    let progress = handler
        .call_tool(
            "resume_as_pending",
            json!({"call_id": call_id_0, "session_id": "async_sess"}),
        )
        .unwrap();
    let call_id_1 = progress["call_id"].as_u64().unwrap();

    let progress = handler
        .call_tool(
            "resume_as_pending",
            json!({"call_id": call_id_1, "session_id": "async_sess"}),
        )
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    let progress = handler
        .call_tool(
            "resume_futures",
            json!({
                "results": [
                    {"call_id": call_id_0, "result": "a"},
                    {"call_id": call_id_1, "result": "b"}
                ],
                "session_id": "async_sess"
            }),
        )
        .unwrap();
    assert_eq!(progress["status"], "complete");

    let var = handler
        .call_tool("get_variable", json!({"name": "result", "session_id": "async_sess"}))
        .unwrap();
    assert_eq!(var["value"], json!(["a", "b"]));
}

/// resume_futures with no prior pending state should return an error.
#[test]
fn resume_futures_without_pending_state_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("resume_futures", json!({"results": [{"call_id": 0, "result": 42}]}));
    // This should either be Err (tool dispatch error) or Ok with status "error".
    match result {
        Err(msg) => assert!(
            msg.contains("pending") || msg.contains("resume") || msg.contains("not found"),
            "unexpected error: {msg}"
        ),
        Ok(val) => assert_eq!(val["status"], "error"),
    }
}

/// resume_futures supports failing individual futures via the "error" field.
///
/// Sets up an async gather with 2 external calls, fails one with an error message,
/// succeeds the other, and verifies the execution completes with an error (since
/// one future raised a RuntimeError).
#[test]
fn resume_futures_with_error_field_fails_individual_future() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["async_call"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    // Step 1: execute -- should yield first external call.
    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    assert_eq!(progress["status"], "function_call");
    let call_id_0 = progress["call_id"].as_u64().unwrap();

    // Step 2: resume_as_pending for first call.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    assert_eq!(progress["status"], "function_call");
    let call_id_1 = progress["call_id"].as_u64().unwrap();

    // Step 3: resume_as_pending for second call.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_1}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    // Step 4: resume_futures -- fail call_id_0 with error, succeed call_id_1.
    let progress = handler
        .call_tool(
            "resume_futures",
            json!({
                "results": [
                    {"call_id": call_id_0, "error": "something went wrong"},
                    {"call_id": call_id_1, "result": 20}
                ]
            }),
        )
        .unwrap();

    // The execution should complete with an error because one future raised RuntimeError.
    assert_eq!(progress["status"], "error", "expected error status, got: {progress}");
    let error_text = progress["error"].as_str().unwrap_or("");
    assert!(
        error_text.contains("something went wrong"),
        "error message should contain the original message, got: {error_text}"
    );
}

/// resume_futures accepts result=null when error is absent (treats as None return).
#[test]
fn resume_futures_with_neither_result_nor_error_returns_none() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["async_call"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1))

result = await run()
";

    // Step 1: execute -- should yield first external call.
    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    assert_eq!(progress["status"], "function_call");
    let call_id_0 = progress["call_id"].as_u64().unwrap();

    // Step 2: resume_as_pending.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    // Step 3: resume_futures with no result and no error -- should treat as None.
    let progress = handler
        .call_tool(
            "resume_futures",
            json!({
                "results": [
                    {"call_id": call_id_0}
                ]
            }),
        )
        .unwrap();
    assert_eq!(progress["status"], "complete");

    // Verify gathered result is [None].
    let var = handler.call_tool("get_variable", json!({"name": "result"})).unwrap();
    assert_eq!(var["value"], json!([null]));
}

/// resolve_futures response includes `pending_futures` with function name and args.
#[test]
fn resolve_futures_includes_pending_futures_metadata() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch", "compute"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(fetch('http://example.com'), compute(42))

result = await run()
";

    // Step 1: execute -- yields first external call.
    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    assert_eq!(progress["status"], "function_call");
    assert_eq!(progress["function_name"], "fetch");
    let call_id_0 = progress["call_id"].as_u64().unwrap();

    // Step 2: resume_as_pending for first call.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    assert_eq!(progress["status"], "function_call");
    assert_eq!(progress["function_name"], "compute");
    let call_id_1 = progress["call_id"].as_u64().unwrap();

    // Step 3: resume_as_pending for second call.
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_1}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    // Backward compat: pending_call_ids still present.
    let pending_ids = progress["pending_call_ids"].as_array().unwrap();
    assert_eq!(pending_ids.len(), 2);

    // New: pending_futures carries metadata.
    let pending_futures = progress["pending_futures"]
        .as_array()
        .expect("pending_futures should be present");
    assert_eq!(pending_futures.len(), 2, "should have 2 pending future entries");

    // Find metadata for each call_id (order not guaranteed).
    let info_0 = pending_futures
        .iter()
        .find(|f| f["call_id"].as_u64().unwrap() == call_id_0)
        .expect("missing pending_futures entry for call_id_0");
    assert_eq!(info_0["function_name"], "fetch");
    assert_eq!(info_0["args"], json!(["http://example.com"]));

    let info_1 = pending_futures
        .iter()
        .find(|f| f["call_id"].as_u64().unwrap() == call_id_1)
        .expect("missing pending_futures entry for call_id_1");
    assert_eq!(info_1["function_name"], "compute");
    assert_eq!(info_1["args"], json!([42]));
}

/// After incremental resolution, remaining pending_futures still has correct metadata.
#[test]
fn resolve_futures_metadata_survives_incremental_resolution_mcp() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["async_call"]}))
        .unwrap();

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = handler.call_tool("execute", json!({"code": code})).unwrap();
    let call_id_0 = progress["call_id"].as_u64().unwrap();
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_0}))
        .unwrap();
    let call_id_1 = progress["call_id"].as_u64().unwrap();
    let progress = handler
        .call_tool("resume_as_pending", json!({"call_id": call_id_1}))
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");
    assert_eq!(progress["pending_futures"].as_array().unwrap().len(), 2);

    // Resolve only first future.
    let progress = handler
        .call_tool(
            "resume_futures",
            json!({"results": [{"call_id": call_id_0, "result": 100}]}),
        )
        .unwrap();
    assert_eq!(progress["status"], "resolve_futures");

    // Remaining should have metadata for call_id_1 only.
    let remaining_futures = progress["pending_futures"].as_array().unwrap();
    assert_eq!(remaining_futures.len(), 1);
    assert_eq!(remaining_futures[0]["call_id"].as_u64().unwrap(), call_id_1);
    assert_eq!(remaining_futures[0]["function_name"], "async_call");
    assert_eq!(remaining_futures[0]["args"], json!([2]));
}

// =============================================================================
// save_session / load_session tool tests
// =============================================================================

/// The tool list includes save_session and load_session.
#[test]
fn tools_list_contains_save_and_load_session() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"save_session"), "missing save_session tool");
    assert!(names.contains(&"load_session"), "missing load_session tool");
}

/// save_session returns error when storage_dir is not configured.
#[test]
fn save_session_without_storage_dir_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler.call_tool("save_session", json!({"name": "snap"})).unwrap_err();
    assert!(
        err.contains("storage not configured"),
        "Expected 'storage not configured' error, got: {err}"
    );
}

/// load_session returns error when storage_dir is not configured.
#[test]
fn load_session_without_storage_dir_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let err = handler.call_tool("load_session", json!({"name": "snap"})).unwrap_err();
    assert!(
        err.contains("storage not configured"),
        "Expected 'storage not configured' error, got: {err}"
    );
}

/// save_session saves the default session and reports size.
#[test]
fn save_session_basic() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    handler.call_tool("execute", json!({"code": "x = 42"})).unwrap();

    let result = handler.call_tool("save_session", json!({"name": "snap1"})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["name"], "snap1");
    assert!(
        result["size_bytes"].as_u64().unwrap() > 0,
        "size_bytes should be positive"
    );

    // Verify the file was written.
    assert!(dir.path().join("snap1.bin").exists());
}

/// save_session defaults session_id to "default" and name to session_id.
#[test]
fn save_session_defaults() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let result = handler.call_tool("save_session", json!({})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["name"], "default");
    assert!(dir.path().join("default.bin").exists());
}

/// save_session with explicit session_id saves that session.
#[test]
fn save_session_named_session() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    handler
        .call_tool("create_session", json!({"session_id": "worker"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "y = 99", "session_id": "worker"}))
        .unwrap();

    let result = handler
        .call_tool("save_session", json!({"session_id": "worker", "name": "worker_snap"}))
        .unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["name"], "worker_snap");
}

/// load_session restores a previously saved session with its variables.
#[test]
fn load_session_restores_state() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    // Set up and save.
    handler.call_tool("execute", json!({"code": "x = 42"})).unwrap();
    handler
        .call_tool("save_session", json!({"name": "checkpoint"}))
        .unwrap();

    // Load into a new session.
    let result = handler
        .call_tool("load_session", json!({"name": "checkpoint", "session_id": "restored"}))
        .unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["session_id"], "restored");
    assert_eq!(result["name"], "checkpoint");

    // Verify the restored session has the variable.
    let var = handler
        .call_tool("get_variable", json!({"name": "x", "session_id": "restored"}))
        .unwrap();
    assert_eq!(var["value"], 42);
}

/// load_session defaults session_id to the name.
#[test]
fn load_session_defaults_session_id_to_name() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    handler.call_tool("execute", json!({"code": "a = 1"})).unwrap();
    handler.call_tool("save_session", json!({"name": "my_snap"})).unwrap();

    let result = handler.call_tool("load_session", json!({"name": "my_snap"})).unwrap();
    assert_eq!(result["session_id"], "my_snap");

    // Verify it's accessible as a session.
    let var = handler
        .call_tool("get_variable", json!({"name": "a", "session_id": "my_snap"}))
        .unwrap();
    assert_eq!(var["value"], 1);
}

/// load_session returns error if session_id already exists.
#[test]
fn load_session_duplicate_session_id_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    handler.call_tool("save_session", json!({"name": "snap"})).unwrap();

    // Try to load into the already-existing "default" session.
    let err = handler
        .call_tool("load_session", json!({"name": "snap", "session_id": "default"}))
        .unwrap_err();
    assert!(
        err.contains("already exists"),
        "Expected 'already exists' error, got: {err}"
    );
}

/// load_session returns error for nonexistent snapshot file.
#[test]
fn load_session_missing_file_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let err = handler
        .call_tool("load_session", json!({"name": "does_not_exist"}))
        .unwrap_err();
    assert!(
        err.contains("not found") || err.contains("No such file"),
        "Expected file-not-found error, got: {err}"
    );
}

/// save_session rejects names with path traversal characters.
#[test]
fn save_session_rejects_path_traversal() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let err = handler
        .call_tool("save_session", json!({"name": "../escape"}))
        .unwrap_err();
    assert!(
        err.contains("invalid snapshot name"),
        "Expected 'invalid snapshot name' error, got: {err}"
    );
}

/// load_session rejects names with path traversal characters.
#[test]
fn load_session_rejects_path_traversal() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let err = handler
        .call_tool("load_session", json!({"name": "../../etc/passwd"}))
        .unwrap_err();
    assert!(
        err.contains("invalid snapshot name"),
        "Expected 'invalid snapshot name' error, got: {err}"
    );
}

/// save_session rejects names with slashes.
#[test]
fn save_session_rejects_slashes() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let err = handler
        .call_tool("save_session", json!({"name": "foo/bar"}))
        .unwrap_err();
    assert!(
        err.contains("invalid snapshot name"),
        "Expected 'invalid snapshot name' error, got: {err}"
    );
}

/// save_session rejects empty names.
#[test]
fn save_session_rejects_empty_name() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let err = handler.call_tool("save_session", json!({"name": ""})).unwrap_err();
    assert!(
        err.contains("invalid snapshot name"),
        "Expected 'invalid snapshot name' error, got: {err}"
    );
}

/// save_session allows hyphens and underscores in names.
#[test]
fn save_session_allows_hyphens_and_underscores() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let result = handler
        .call_tool("save_session", json!({"name": "my-snap_v2"}))
        .unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["name"], "my-snap_v2");
}

/// save_session returns error when session has pending call.
#[test]
fn save_session_pending_call_returns_error() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();
    handler.call_tool("execute", json!({"code": "fetch(1)"})).unwrap();

    let err = handler.call_tool("save_session", json!({"name": "snap"})).unwrap_err();
    assert!(
        err.contains("pending"),
        "Expected error about pending state, got: {err}"
    );
}

/// Loaded session is independent from the original (mutations don't propagate).
#[test]
fn loaded_session_is_independent() {
    let mut handler = McpHandler::new("<mcp>");
    let dir = tempfile::tempdir().unwrap();
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    handler.call_tool("execute", json!({"code": "x = 10"})).unwrap();
    handler.call_tool("save_session", json!({"name": "snap"})).unwrap();

    // Load into a new session.
    handler
        .call_tool("load_session", json!({"name": "snap", "session_id": "clone"}))
        .unwrap();

    // Mutate the loaded session.
    handler
        .call_tool("execute", json!({"code": "x = 999", "session_id": "clone"}))
        .unwrap();

    // Original should be unchanged.
    let original = handler
        .call_tool("execute", json!({"code": "x", "session_id": "default"}))
        .unwrap();
    assert_eq!(original["result"], 10);
}

// =============================================================================
// list_saved_sessions tool tests
// =============================================================================

/// list_saved_sessions returns error when storage_dir is not configured.
#[test]
fn list_saved_sessions_no_storage_configured() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("list_saved_sessions", json!({}));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("storage not configured"));
}

/// list_saved_sessions returns empty list when storage directory exists but is empty.
#[test]
fn list_saved_sessions_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut handler = McpHandler::new("<mcp>");
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    let result = handler.call_tool("list_saved_sessions", json!({})).unwrap();
    assert_eq!(result["sessions"], json!([]));
}

/// list_saved_sessions returns saved sessions sorted by name with sizes.
#[test]
fn list_saved_sessions_returns_saved_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let mut handler = McpHandler::new("<mcp>");
    handler.set_storage_dir(dir.path().to_path_buf()).unwrap();

    // Save two sessions.
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("save_session", json!({"name": "alpha"})).unwrap();
    handler.call_tool("save_session", json!({"name": "beta"})).unwrap();

    let result = handler.call_tool("list_saved_sessions", json!({})).unwrap();
    let sessions = result["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0]["name"], "alpha");
    assert_eq!(sessions[1]["name"], "beta");
    // Size should be > 0.
    assert!(sessions[0]["size_bytes"].as_u64().unwrap() > 0);
}

/// The tool list includes list_saved_sessions.
#[test]
fn tools_list_contains_list_saved_sessions() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"list_saved_sessions"),
        "missing list_saved_sessions tool"
    );
}

// =============================================================================
// eval_variable tool tests
// =============================================================================

/// eval_variable evaluates an expression and returns the result.
#[test]
fn eval_variable_returns_expression_result() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 10; y = 20"})).unwrap();

    let result = handler
        .call_tool("eval_variable", json!({"expression": "x + y"}))
        .unwrap();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["result"], 30);
    assert_eq!(result["repr"], "30");
}

/// eval_variable does not modify the original session state.
#[test]
fn eval_variable_does_not_modify_session() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 10"})).unwrap();

    // Eval that would modify x -- but shouldn't affect the real session.
    handler
        .call_tool("eval_variable", json!({"expression": "x = 999"}))
        .unwrap();

    // x should still be 10.
    let var = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(var["value"], 10);
}

/// eval_variable returns an error when the expression triggers an external function call.
#[test]
fn eval_variable_errors_on_external_call() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let result = handler.call_tool("eval_variable", json!({"expression": "fetch('url')"}));
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("external function"),
        "error should mention external function calls"
    );
}

/// eval_variable returns an error when the expression raises a Python exception.
#[test]
fn eval_variable_returns_error_on_exception() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("eval_variable", json!({"expression": "1 / 0"}));
    assert!(result.is_err());
}

/// eval_variable works with an explicit session_id.
#[test]
fn eval_variable_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "eval_sess"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "val = 42", "session_id": "eval_sess"}))
        .unwrap();

    let result = handler
        .call_tool(
            "eval_variable",
            json!({"expression": "val * 2", "session_id": "eval_sess"}),
        )
        .unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["result"], 84);
}

/// eval_variable captures stdout output from print statements.
#[test]
fn eval_variable_captures_stdout() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 5"})).unwrap();

    let result = handler
        .call_tool("eval_variable", json!({"expression": "print(x)"}))
        .unwrap();

    assert_eq!(result["status"], "ok");
    assert!(
        result.get("stdout").is_some(),
        "stdout should be present when print is used"
    );
    assert!(result["stdout"].as_str().unwrap().contains('5'));
}

/// The tool list includes eval_variable.
#[test]
fn tools_list_contains_eval_variable() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"eval_variable"), "missing eval_variable tool");
}

// =============================================================================
// Cross-session pipeline: call_session
// =============================================================================

/// call_session executes code in one session and stores the result in another.
#[test]
fn call_session_executes_and_stores_result() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "src"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "x = 42", "session_id": "src"}))
        .unwrap();

    let result = handler
        .call_tool(
            "call_session",
            json!({
                "code": "x * 2",
                "source_session_id": "src",
                "target_session_id": "dst",
                "target_variable": "piped_result"
            }),
        )
        .unwrap();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["source_session_id"], "src");
    assert_eq!(result["target_session_id"], "dst");
    assert_eq!(result["target_variable"], "piped_result");

    // Verify the variable landed in the target session.
    let var = handler
        .call_tool("get_variable", json!({"session_id": "dst", "name": "piped_result"}))
        .unwrap();
    assert_eq!(var["value"], 84);
}

/// call_session defaults source_session_id to "default" when omitted.
#[test]
fn call_session_defaults_source_to_default() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "val = 10"})).unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();

    let result = handler
        .call_tool(
            "call_session",
            json!({
                "code": "val + 5",
                "target_session_id": "dst",
                "target_variable": "out"
            }),
        )
        .unwrap();

    assert_eq!(result["status"], "ok");

    let var = handler
        .call_tool("get_variable", json!({"session_id": "dst", "name": "out"}))
        .unwrap();
    assert_eq!(var["value"], 15);
}

/// call_session errors when source and target are the same session.
#[test]
fn call_session_errors_same_session() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool(
        "call_session",
        json!({
            "code": "1 + 1",
            "source_session_id": "default",
            "target_session_id": "default",
            "target_variable": "x"
        }),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("same session"),
        "Expected 'same session' error, got: {err}"
    );
}

/// call_session returns function_call status when code triggers an external call.
#[test]
fn call_session_returns_function_call_if_external() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "src"}))
        .unwrap();
    handler
        .call_tool("reset", json!({"session_id": "src", "external_functions": ["fetch"]}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();

    let result = handler
        .call_tool(
            "call_session",
            json!({
                "code": "fetch('url')",
                "source_session_id": "src",
                "target_session_id": "dst",
                "target_variable": "data"
            }),
        )
        .unwrap();

    assert_eq!(result["status"], "function_call");
    assert_eq!(result["function_name"], "fetch");
    // Variable should NOT have been set in the target session.
    let var = handler
        .call_tool("get_variable", json!({"session_id": "dst", "name": "data"}))
        .unwrap();
    assert!(
        var["value"].is_null(),
        "target variable should not be set on function_call"
    );
}

/// call_session errors when source session does not exist.
#[test]
fn call_session_errors_nonexistent_source() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "dst"}))
        .unwrap();

    let result = handler.call_tool(
        "call_session",
        json!({
            "code": "1",
            "source_session_id": "no_such_session",
            "target_session_id": "dst",
            "target_variable": "x"
        }),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

/// call_session errors when target session does not exist.
#[test]
fn call_session_errors_nonexistent_target() {
    let mut handler = McpHandler::new("<mcp>");

    let result = handler.call_tool(
        "call_session",
        json!({
            "code": "1",
            "source_session_id": "default",
            "target_session_id": "no_such_session",
            "target_variable": "x"
        }),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("not found"), "Expected 'not found' error, got: {err}");
}

/// The tool list includes call_session.
#[test]
fn tools_list_contains_call_session() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"call_session"), "missing call_session tool");
}

// =============================================================================
// Tuple `values` convenience field tests
// =============================================================================

/// eval_variable with a tuple expression includes a `values` field with the plain array.
#[test]
fn eval_variable_tuple_has_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("execute", json!({"code": "t = (1, 'hello', [2, 3])"}))
        .unwrap();

    let result = handler.call_tool("eval_variable", json!({"expression": "t"})).unwrap();

    assert_eq!(result["status"], "ok");
    // The result field should have the tagged $tuple encoding.
    assert_eq!(
        result["result"],
        json!({"$tuple": [1, "hello", [2, 3]]}),
        "result should use $tuple encoding"
    );
    // The values field should have the plain array.
    assert_eq!(
        result["values"],
        json!([1, "hello", [2, 3]]),
        "values should be a plain JSON array for tuples"
    );
}

/// eval_variable with a non-tuple result does NOT include a `values` field.
#[test]
fn eval_variable_non_tuple_no_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 42"})).unwrap();

    let result = handler.call_tool("eval_variable", json!({"expression": "x"})).unwrap();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["result"], 42);
    assert!(
        result.get("values").is_none(),
        "values field should NOT be present for non-tuple results"
    );
}

/// eval_variable with a list result does NOT include a `values` field.
#[test]
fn eval_variable_list_no_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("execute", json!({"code": "lst = [1, 2, 3]"}))
        .unwrap();

    let result = handler
        .call_tool("eval_variable", json!({"expression": "lst"}))
        .unwrap();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["result"], json!([1, 2, 3]));
    assert!(
        result.get("values").is_none(),
        "values field should NOT be present for list results"
    );
}

/// get_variable for a tuple variable includes a `values` field.
#[test]
fn get_variable_tuple_has_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("execute", json!({"code": "pair = (10, 20)"}))
        .unwrap();

    let result = handler.call_tool("get_variable", json!({"name": "pair"})).unwrap();

    assert_eq!(result["name"], "pair");
    assert_eq!(
        result["value"],
        json!({"$tuple": [10, 20]}),
        "value should use $tuple encoding"
    );
    assert_eq!(
        result["values"],
        json!([10, 20]),
        "values should be a plain JSON array for tuple variables"
    );
}

/// get_variable for a non-tuple variable does NOT include `values`.
#[test]
fn get_variable_non_tuple_no_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "name = 'alice'"})).unwrap();

    let result = handler.call_tool("get_variable", json!({"name": "name"})).unwrap();

    assert_eq!(result["name"], "name");
    assert_eq!(result["value"], "alice");
    assert!(
        result.get("values").is_none(),
        "values field should NOT be present for non-tuple variables"
    );
}

/// execute tool with a tuple result includes `values` in the progress response.
#[test]
fn execute_tuple_result_has_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "(1, 2, 3)"})).unwrap();

    assert_eq!(result["status"], "complete");
    assert_eq!(
        result["result"],
        json!({"$tuple": [1, 2, 3]}),
        "result should use $tuple encoding"
    );
    assert_eq!(
        result["values"],
        json!([1, 2, 3]),
        "values should be a plain JSON array for tuple results"
    );
}

/// execute tool with a non-tuple result does NOT include `values`.
#[test]
fn execute_non_tuple_result_no_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    let result = handler.call_tool("execute", json!({"code": "1 + 2"})).unwrap();

    assert_eq!(result["status"], "complete");
    assert_eq!(result["result"], 3);
    assert!(
        result.get("values").is_none(),
        "values field should NOT be present for non-tuple results"
    );
}

/// call_session with a tuple result includes `values` in the response.
#[test]
fn call_session_tuple_result_has_values_field() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "src_tup"}))
        .unwrap();
    handler
        .call_tool("create_session", json!({"session_id": "dst_tup"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "a = 1; b = 2", "session_id": "src_tup"}))
        .unwrap();

    let result = handler
        .call_tool(
            "call_session",
            json!({
                "code": "(a, b)",
                "source_session_id": "src_tup",
                "target_session_id": "dst_tup",
                "target_variable": "tup_result"
            }),
        )
        .unwrap();

    assert_eq!(result["status"], "ok");
    assert_eq!(
        result["result"],
        json!({"$tuple": [1, 2]}),
        "result should use $tuple encoding"
    );
    assert_eq!(
        result["values"],
        json!([1, 2]),
        "values should be a plain JSON array for tuple results"
    );
}

// =============================================================================
// Rewind / undo history tool tests
// =============================================================================

/// The tool list includes rewind, history, and set_history_depth.
#[test]
fn tools_list_contains_rewind_history_tools() {
    let handler = McpHandler::new("<mcp>");
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"rewind"), "missing rewind tool");
    assert!(names.contains(&"history"), "missing history tool");
    assert!(names.contains(&"set_history_depth"), "missing set_history_depth tool");
}

/// Basic rewind: execute x=1, execute x=2, rewind 1, get x -> should be 1.
#[test]
fn rewind_basic() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 2"})).unwrap();

    let result = handler.call_tool("rewind", json!({"steps": 1})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["steps_rewound"], 1);

    let var = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(var["value"], 1);
}

/// Rewind multiple steps: execute x=1, x=2, x=3, rewind 2, get x -> 1.
#[test]
fn rewind_multiple_steps() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 2"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 3"})).unwrap();

    let result = handler.call_tool("rewind", json!({"steps": 2})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["steps_rewound"], 2);

    let var = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(var["value"], 1);
}

/// Rewind beyond available history returns an error.
#[test]
fn rewind_beyond_history_errors() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();

    let result = handler.call_tool("rewind", json!({"steps": 5}));
    assert!(result.is_err(), "rewind beyond history should error");
    let err = result.unwrap_err();
    assert!(
        err.contains("history") || err.contains("steps"),
        "error should mention history, got: {err}"
    );
}

/// Rewind defaults to 1 step when steps is omitted.
#[test]
fn rewind_defaults_to_one_step() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 2"})).unwrap();

    let result = handler.call_tool("rewind", json!({})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["steps_rewound"], 1);

    let var = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(var["value"], 1);
}

/// The history tool reports the number of available undo steps.
#[test]
fn history_depth() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 2"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 3"})).unwrap();

    let result = handler.call_tool("history", json!({})).unwrap();
    assert_eq!(result["depth"], 3);
    assert_eq!(result["max_depth"], 20);
}

/// set_history_depth truncates history from the front when new max < current depth.
#[test]
fn set_history_depth_truncates() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 2"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 3"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 4"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 5"})).unwrap();

    let result = handler.call_tool("set_history_depth", json!({"max_depth": 2})).unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["max_depth"], 2);
    assert_eq!(result["trimmed"], 3);

    // History should now have depth 2.
    let hist = handler.call_tool("history", json!({})).unwrap();
    assert_eq!(hist["depth"], 2);
}

/// Rewind works with an explicit session_id.
#[test]
fn rewind_with_session_id() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("create_session", json!({"session_id": "rew"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "x = 10", "session_id": "rew"}))
        .unwrap();
    handler
        .call_tool("execute", json!({"code": "x = 20", "session_id": "rew"}))
        .unwrap();

    let result = handler
        .call_tool("rewind", json!({"steps": 1, "session_id": "rew"}))
        .unwrap();
    assert_eq!(result["status"], "ok");

    let var = handler
        .call_tool("get_variable", json!({"name": "x", "session_id": "rew"}))
        .unwrap();
    assert_eq!(var["value"], 10);
}

/// A forked session starts with an empty history (does not copy parent history).
#[test]
fn fork_starts_with_empty_history() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();
    handler.call_tool("execute", json!({"code": "x = 2"})).unwrap();

    handler
        .call_tool(
            "fork_session",
            json!({"source_session_id": "default", "new_session_id": "forked"}),
        )
        .unwrap();

    let result = handler.call_tool("history", json!({"session_id": "forked"})).unwrap();
    assert_eq!(result["depth"], 0, "forked session should start with empty history");
}

/// Failed executions should NOT create history entries.
#[test]
fn failed_execute_does_not_create_history() {
    let mut handler = McpHandler::new("<mcp>");
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();

    // This should fail (division by zero).
    handler.call_tool("execute", json!({"code": "1 / 0"})).unwrap();

    let result = handler.call_tool("history", json!({})).unwrap();
    // Only the successful x=1 should be in history.
    assert_eq!(result["depth"], 1, "failed execution should not create history entry");
}

/// History is capped at max_history (default 20). Oldest entries are dropped.
#[test]
fn history_respects_max_depth() {
    let mut handler = McpHandler::new("<mcp>");

    // Set a small max to make this test fast.
    handler.call_tool("set_history_depth", json!({"max_depth": 3})).unwrap();

    // Execute 5 times.
    for i in 1..=5 {
        handler
            .call_tool("execute", json!({"code": format!("x = {i}")}))
            .unwrap();
    }

    let result = handler.call_tool("history", json!({})).unwrap();
    assert_eq!(result["depth"], 3, "history should be capped at max_depth");

    // Rewind 3 steps should work, bringing us back to x=2 (state before x=3 was executed).
    handler.call_tool("rewind", json!({"steps": 3})).unwrap();
    let var = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(var["value"], 2);
}

/// resume also creates history entries when it completes successfully.
#[test]
fn rewind_after_resume() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    // First: execute x=1 (creates history entry).
    handler.call_tool("execute", json!({"code": "x = 1"})).unwrap();

    // Now execute with external call (creates history entry for the pre-execution state).
    let progress = handler
        .call_tool("execute", json!({"code": "x = fetch(1) + 1"}))
        .unwrap();
    let call_id = progress["call_id"].as_u64().unwrap();

    // Resume the external call (creates history entry for the pre-resume state).
    handler
        .call_tool("resume", json!({"call_id": call_id, "result": 41}))
        .unwrap();

    // x should now be 42. Verify.
    let var = handler.call_tool("get_variable", json!({"name": "x"})).unwrap();
    assert_eq!(var["value"], 42);

    // History should have entries. Rewind 1 should undo the resume.
    let hist = handler.call_tool("history", json!({})).unwrap();
    assert!(
        hist["depth"].as_u64().unwrap() >= 2,
        "should have at least 2 history entries"
    );

    handler.call_tool("rewind", json!({"steps": 1})).unwrap();
    // After rewinding the resume, we should be back to the state before resume.
    // The session should be in the state right before the resume was called.
}
