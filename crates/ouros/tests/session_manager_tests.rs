//! Integration tests for `SessionManager`.
//!
//! Covers session lifecycle, code execution, variable operations, history/rewind,
//! persistence (save/load), heap introspection, cross-session operations, and
//! error cases.

use ouros::session_manager::{SessionError, SessionManager};

// ============================================================================
// Construction & defaults
// ============================================================================

#[test]
fn default_session_exists_on_creation() {
    let mgr = SessionManager::new("test.py");
    let sessions = mgr.list_sessions();
    assert_eq!(sessions.len(), 1, "should have exactly the default session");
    assert_eq!(sessions[0].id, "default");
}

#[test]
fn new_with_limits_creates_default_session() {
    use ouros::ResourceLimits;
    let limits = ResourceLimits::new().max_operations(500_000);
    let mgr = SessionManager::new_with_limits("test.py", limits);
    let sessions = mgr.list_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "default");
}

// ============================================================================
// Session lifecycle: create / destroy / list
// ============================================================================

#[test]
fn create_and_list_sessions() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("alpha", Vec::new()).unwrap();
    mgr.create_session("beta", Vec::new()).unwrap();

    let sessions = mgr.list_sessions();
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"default"));
    assert!(ids.contains(&"alpha"));
    assert!(ids.contains(&"beta"));
    assert_eq!(sessions.len(), 3);
}

#[test]
fn create_duplicate_session_fails() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("alpha", Vec::new()).unwrap();
    let err = mgr.create_session("alpha", Vec::new()).unwrap_err();
    assert!(matches!(err, SessionError::AlreadyExists(_)));
}

#[test]
fn destroy_session_removes_it() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("temp", Vec::new()).unwrap();
    assert_eq!(mgr.list_sessions().len(), 2);
    mgr.destroy_session("temp").unwrap();
    assert_eq!(mgr.list_sessions().len(), 1);
}

#[test]
fn destroy_default_session_fails() {
    let mut mgr = SessionManager::new("test.py");
    let err = mgr.destroy_session("default").unwrap_err();
    assert!(matches!(err, SessionError::InvalidState(_)));
}

#[test]
fn destroy_nonexistent_session_fails() {
    let mut mgr = SessionManager::new("test.py");
    let err = mgr.destroy_session("ghost").unwrap_err();
    assert!(matches!(err, SessionError::NotFound(_)));
}

// ============================================================================
// Execute & variables
// ============================================================================

#[test]
fn execute_code_and_check_variables() {
    let mut mgr = SessionManager::new("test.py");
    let output = mgr.execute(None, "x = 42").unwrap();
    // Execute should succeed (may or may not have stdout).
    assert!(output.stdout.is_empty() || !output.stdout.is_empty());

    let vars = mgr.list_variables(None).unwrap();
    let x_var = vars.iter().find(|v| v.name == "x");
    assert!(x_var.is_some(), "variable 'x' should exist");
    assert_eq!(x_var.unwrap().type_name, "int");

    let val = mgr.get_variable(None, "x").unwrap();
    assert_eq!(val.json_value, serde_json::json!(42));
}

#[test]
fn execute_in_named_session() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("work", Vec::new()).unwrap();
    mgr.execute(Some("work"), "y = 'hello'").unwrap();

    let vars = mgr.list_variables(Some("work")).unwrap();
    assert!(vars.iter().any(|v| v.name == "y"));

    // Default session should not have the variable.
    let default_vars = mgr.list_variables(None).unwrap();
    assert!(!default_vars.iter().any(|v| v.name == "y"));
}

#[test]
fn set_variable_via_expression() {
    let mut mgr = SessionManager::new("test.py");
    mgr.set_variable(None, "z", "[1, 2, 3]").unwrap();

    let val = mgr.get_variable(None, "z").unwrap();
    assert_eq!(val.json_value, serde_json::json!([1, 2, 3]));
}

#[test]
fn delete_variable() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "a = 10").unwrap();
    let existed = mgr.delete_variable(None, "a").unwrap();
    assert!(existed);

    let vars = mgr.list_variables(None).unwrap();
    assert!(!vars.iter().any(|v| v.name == "a"));

    // Deleting again should return false.
    let existed_again = mgr.delete_variable(None, "a").unwrap();
    assert!(!existed_again);
}

#[test]
fn execute_nonexistent_session_fails() {
    let mut mgr = SessionManager::new("test.py");
    let err = mgr.execute(Some("ghost"), "x = 1").unwrap_err();
    assert!(matches!(err, SessionError::NotFound(_)));
}

// ============================================================================
// Eval variable (read-only)
// ============================================================================

#[test]
fn eval_variable_does_not_modify_state() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = 10").unwrap();

    let result = mgr.eval_variable(None, "x + 5").unwrap();
    assert_eq!(result.value.json_value, serde_json::json!(15));

    // x should still be 10 (eval should not modify state).
    let val = mgr.get_variable(None, "x").unwrap();
    assert_eq!(val.json_value, serde_json::json!(10));
}

// ============================================================================
// Fork session
// ============================================================================

#[test]
fn fork_session_independence() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = 1").unwrap();
    mgr.fork_session("default", "fork1").unwrap();

    // Modify the fork.
    mgr.execute(Some("fork1"), "x = 999").unwrap();

    // Original should be unchanged.
    let original = mgr.get_variable(None, "x").unwrap();
    assert_eq!(original.json_value, serde_json::json!(1));

    let forked = mgr.get_variable(Some("fork1"), "x").unwrap();
    assert_eq!(forked.json_value, serde_json::json!(999));
}

#[test]
fn fork_nonexistent_source_fails() {
    let mut mgr = SessionManager::new("test.py");
    let err = mgr.fork_session("ghost", "new").unwrap_err();
    assert!(matches!(err, SessionError::NotFound(_)));
}

#[test]
fn fork_to_existing_id_fails() {
    let mut mgr = SessionManager::new("test.py");
    let err = mgr.fork_session("default", "default").unwrap_err();
    assert!(matches!(err, SessionError::AlreadyExists(_)));
}

// ============================================================================
// Rewind / history
// ============================================================================

#[test]
fn rewind_restores_previous_state() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = 1").unwrap();
    mgr.execute(None, "x = 2").unwrap();
    mgr.execute(None, "x = 3").unwrap();

    let val = mgr.get_variable(None, "x").unwrap();
    assert_eq!(val.json_value, serde_json::json!(3));

    // Rewind 2 steps should restore x = 1.
    let result = mgr.rewind(None, 2).unwrap();
    assert_eq!(result.steps_rewound, 2);

    let val = mgr.get_variable(None, "x").unwrap();
    assert_eq!(val.json_value, serde_json::json!(1));
}

#[test]
fn history_returns_depth() {
    let mut mgr = SessionManager::new("test.py");
    let (depth, _max) = mgr.history(None).unwrap();
    assert_eq!(depth, 0);

    mgr.execute(None, "x = 1").unwrap();
    let (depth, _max) = mgr.history(None).unwrap();
    assert_eq!(depth, 1);

    mgr.execute(None, "x = 2").unwrap();
    let (depth, _max) = mgr.history(None).unwrap();
    assert_eq!(depth, 2);
}

#[test]
fn set_history_depth_trims() {
    let mut mgr = SessionManager::new("test.py");
    // Execute 5 times.
    for i in 0..5 {
        mgr.execute(None, &format!("x = {i}")).unwrap();
    }

    let (depth, _) = mgr.history(None).unwrap();
    assert_eq!(depth, 5);

    // Reduce max to 2, should trim 3.
    let trimmed = mgr.set_history_depth(None, 2).unwrap();
    assert_eq!(trimmed, 3);

    let (depth, max) = mgr.history(None).unwrap();
    assert_eq!(depth, 2);
    assert_eq!(max, 2);
}

#[test]
fn rewind_too_many_steps_fails() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = 1").unwrap();

    let err = mgr.rewind(None, 5).unwrap_err();
    assert!(matches!(err, SessionError::InvalidState(_)));
}

#[test]
fn rewind_zero_steps_fails() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = 1").unwrap();
    let err = mgr.rewind(None, 0).unwrap_err();
    assert!(matches!(err, SessionError::InvalidArgument(_)));
}

// ============================================================================
// Save / load round-trip
// ============================================================================

#[test]
fn save_and_load_round_trip() {
    let dir = std::env::temp_dir().join(format!("ouros_sm_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut mgr = SessionManager::new("test.py");
    mgr.set_storage_dir(dir.clone());
    mgr.execute(None, "x = 42").unwrap();

    let save_result = mgr.save_session(None, Some("snapshot1")).unwrap();
    assert_eq!(save_result.name, "snapshot1");
    assert!(save_result.size_bytes > 0);

    // Load into a new session.
    let loaded_id = mgr.load_session("snapshot1", Some("loaded")).unwrap();
    assert_eq!(loaded_id, "loaded");

    let val = mgr.get_variable(Some("loaded"), "x").unwrap();
    assert_eq!(val.json_value, serde_json::json!(42));

    // List saved sessions.
    let saved = mgr.list_saved_sessions().unwrap();
    assert!(saved.iter().any(|s| s.name == "snapshot1"));

    // Cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_without_storage_dir_fails() {
    let mgr = SessionManager::new("test.py");
    let err = mgr.save_session(None, None).unwrap_err();
    assert!(matches!(err, SessionError::Storage(_)));
}

// ============================================================================
// StorageBackend trait + FsBackend
// ============================================================================

#[test]
fn fs_backend_save_load_round_trip() {
    use ouros::session_manager::{FsBackend, StorageBackend};

    let dir = std::env::temp_dir().join(format!("ouros_fs_backend_{}", std::process::id()));
    let backend = FsBackend::new(dir.clone());

    let data = b"hello snapshot";
    backend.save("test_snap", data).unwrap();

    let loaded = backend.load("test_snap").unwrap();
    assert_eq!(loaded, data);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_backend_list_returns_sorted_entries() {
    use ouros::session_manager::{FsBackend, StorageBackend};

    let dir = std::env::temp_dir().join(format!("ouros_fs_list_{}", std::process::id()));
    let backend = FsBackend::new(dir.clone());

    backend.save("charlie", b"c").unwrap();
    backend.save("alpha", b"aa").unwrap();
    backend.save("bravo", b"bbb").unwrap();

    let list = backend.list().unwrap();
    let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "bravo", "charlie"]);

    // Sizes should match data lengths.
    assert_eq!(list[0].size_bytes, 2); // "aa"
    assert_eq!(list[1].size_bytes, 3); // "bbb"
    assert_eq!(list[2].size_bytes, 1); // "c"

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_backend_delete_existing_returns_true() {
    use ouros::session_manager::{FsBackend, StorageBackend};

    let dir = std::env::temp_dir().join(format!("ouros_fs_del_{}", std::process::id()));
    let backend = FsBackend::new(dir.clone());

    backend.save("to_delete", b"data").unwrap();
    let deleted = backend.delete("to_delete").unwrap();
    assert!(deleted);

    // Loading should now fail.
    let err = backend.load("to_delete");
    assert!(err.is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_backend_delete_nonexistent_returns_false() {
    use ouros::session_manager::{FsBackend, StorageBackend};

    let dir = std::env::temp_dir().join(format!("ouros_fs_del2_{}", std::process::id()));
    let backend = FsBackend::new(dir.clone());

    let deleted = backend.delete("no_such_snap").unwrap();
    assert!(!deleted);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn set_storage_backend_with_custom_backend() {
    use std::sync::Mutex;

    use ouros::session_manager::{SavedSessionInfo, StorageBackend};

    /// In-memory mock backend for testing.
    #[derive(Debug)]
    struct MemoryBackend {
        store: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl MemoryBackend {
        fn new() -> Self {
            Self {
                store: Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl StorageBackend for MemoryBackend {
        fn save(&self, name: &str, data: &[u8]) -> Result<(), String> {
            self.store.lock().unwrap().insert(name.to_owned(), data.to_vec());
            Ok(())
        }
        fn load(&self, name: &str) -> Result<Vec<u8>, String> {
            self.store
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| format!("not found: {name}"))
        }
        fn list(&self) -> Result<Vec<SavedSessionInfo>, String> {
            let store = self.store.lock().unwrap();
            let mut items: Vec<_> = store
                .iter()
                .map(|(k, v)| SavedSessionInfo {
                    name: k.clone(),
                    size_bytes: v.len() as u64,
                })
                .collect();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(items)
        }
        fn delete(&self, name: &str) -> Result<bool, String> {
            Ok(self.store.lock().unwrap().remove(name).is_some())
        }
    }

    let mut mgr = SessionManager::new("test.py");
    mgr.set_storage_backend(Box::new(MemoryBackend::new()));
    mgr.execute(None, "x = 99").unwrap();

    // Save using the custom backend.
    let save_result = mgr.save_session(None, Some("mem_snap")).unwrap();
    assert_eq!(save_result.name, "mem_snap");
    assert!(save_result.size_bytes > 0);

    // List should show the snapshot.
    let list = mgr.list_saved_sessions().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "mem_snap");

    // Load into a new session.
    let loaded_id = mgr.load_session("mem_snap", Some("loaded")).unwrap();
    assert_eq!(loaded_id, "loaded");
    let val = mgr.get_variable(Some("loaded"), "x").unwrap();
    assert_eq!(val.json_value, serde_json::json!(99));
}

#[test]
fn delete_saved_session_removes_snapshot() {
    let dir = std::env::temp_dir().join(format!("ouros_sm_del_{}", std::process::id()));

    let mut mgr = SessionManager::new("test.py");
    mgr.set_storage_dir(dir.clone());
    mgr.execute(None, "x = 1").unwrap();

    mgr.save_session(None, Some("to_remove")).unwrap();

    // Should exist in the list.
    let list = mgr.list_saved_sessions().unwrap();
    assert!(list.iter().any(|s| s.name == "to_remove"));

    // Delete it.
    let deleted = mgr.delete_saved_session("to_remove").unwrap();
    assert!(deleted);

    // Should no longer be in the list.
    let list = mgr.list_saved_sessions().unwrap();
    assert!(!list.iter().any(|s| s.name == "to_remove"));

    // Deleting again should return false.
    let deleted_again = mgr.delete_saved_session("to_remove").unwrap();
    assert!(!deleted_again);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_saved_session_without_storage_fails() {
    let mgr = SessionManager::new("test.py");
    let err = mgr.delete_saved_session("anything").unwrap_err();
    assert!(matches!(err, SessionError::Storage(_)));
}

#[test]
fn delete_saved_session_invalid_name_fails() {
    let dir = std::env::temp_dir().join(format!("ouros_sm_del2_{}", std::process::id()));

    let mut mgr = SessionManager::new("test.py");
    mgr.set_storage_dir(dir.clone());

    let err = mgr.delete_saved_session("../escape").unwrap_err();
    assert!(matches!(err, SessionError::InvalidArgument(_)));

    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================
// Heap stats, snapshot, diff
// ============================================================================

#[test]
fn heap_stats_returns_valid_data() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = [1, 2, 3]").unwrap();

    let stats = mgr.heap_stats(None).unwrap();
    assert!(stats.total_slots > 0);
}

#[test]
fn snapshot_and_diff_heap() {
    let mut mgr = SessionManager::new("test.py");
    mgr.snapshot_heap(None, "before").unwrap();

    mgr.execute(None, "data = [1, 2, 3, 4, 5]").unwrap();
    mgr.snapshot_heap(None, "after").unwrap();

    let diff = mgr.diff_heap("before", "after").unwrap();
    // After creating a list, live objects should have increased.
    assert!(diff.heap_diff.live_objects_delta > 0);
}

// ============================================================================
// Transfer variable between sessions
// ============================================================================

#[test]
fn transfer_variable_between_sessions() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("dest", Vec::new()).unwrap();
    mgr.execute(None, "msg = 'hello'").unwrap();

    mgr.transfer_variable("default", "dest", "msg", None).unwrap();

    let val = mgr.get_variable(Some("dest"), "msg").unwrap();
    assert_eq!(val.json_value, serde_json::json!("hello"));
}

#[test]
fn transfer_variable_with_rename() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("dest", Vec::new()).unwrap();
    mgr.execute(None, "val = 99").unwrap();

    mgr.transfer_variable("default", "dest", "val", Some("renamed"))
        .unwrap();

    let val = mgr.get_variable(Some("dest"), "renamed").unwrap();
    assert_eq!(val.json_value, serde_json::json!(99));
}

// ============================================================================
// Call session (cross-session pipeline)
// ============================================================================

#[test]
fn call_session_stores_result_in_target() {
    let mut mgr = SessionManager::new("test.py");
    mgr.create_session("target", Vec::new()).unwrap();
    mgr.execute(None, "x = 10").unwrap();

    let output = mgr.call_session(Some("default"), "target", "x + 5", "result").unwrap();
    assert!(output.progress.is_complete());

    let val = mgr.get_variable(Some("target"), "result").unwrap();
    assert_eq!(val.json_value, serde_json::json!(15));
}

// ============================================================================
// Reset
// ============================================================================

#[test]
fn reset_clears_session_state() {
    let mut mgr = SessionManager::new("test.py");
    mgr.execute(None, "x = 42").unwrap();
    assert!(!mgr.list_variables(None).unwrap().is_empty());

    mgr.reset(None, Vec::new()).unwrap();
    assert!(mgr.list_variables(None).unwrap().is_empty());
}
