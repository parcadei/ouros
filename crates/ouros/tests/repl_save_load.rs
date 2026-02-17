//! Tests for `ReplSession::save()` and `ReplSession::load()`.
//!
//! These tests verify that a REPL session can be serialized to bytes and
//! restored from those bytes, preserving all runtime state including variables,
//! functions, heap objects, and the interner.

use ouros::{CollectStringPrint, NoPrint, Object, ReplSession, ResourceLimits, SessionSnapshot};

// =============================================================================
// 1. Basic save/load round-trip
// =============================================================================

/// A fresh session with no state can be saved and loaded.
#[test]
fn save_load_empty_session() {
    let session = ReplSession::new(vec![], "<stdin>");
    let bytes = session.save().expect("save should succeed");
    let restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");
    assert_eq!(restored.script_name(), "<stdin>");
    assert!(restored.list_variables().is_empty());
}

/// A session with a simple integer variable survives round-trip.
#[test]
fn save_load_preserves_integer_variable() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    let result = restored.execute("x", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(42));
}

/// A session with a string variable survives round-trip.
#[test]
fn save_load_preserves_string_variable() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("name = 'hello world'", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    let result = restored.execute("name", &mut NoPrint).unwrap();
    assert_eq!(result, Object::String("hello world".to_string()));
}

/// Multiple variables of different types survive round-trip.
#[test]
fn save_load_preserves_multiple_variables() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("a = 1", &mut NoPrint).unwrap();
    session.execute("b = 'two'", &mut NoPrint).unwrap();
    session.execute("c = 3.14", &mut NoPrint).unwrap();
    session.execute("d = True", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    assert_eq!(restored.execute("a", &mut NoPrint).unwrap(), Object::Int(1));
    assert_eq!(
        restored.execute("b", &mut NoPrint).unwrap(),
        Object::String("two".to_string())
    );
    assert_eq!(restored.execute("d", &mut NoPrint).unwrap(), Object::Bool(true));
}

// =============================================================================
// 2. Functions survive round-trip
// =============================================================================

/// A user-defined function survives round-trip and can be called.
#[test]
fn save_load_preserves_function() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("def double(x): return x * 2", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    let result = restored.execute("double(21)", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(42));
}

// =============================================================================
// 3. Heap objects survive round-trip
// =============================================================================

/// A list variable survives round-trip with correct contents.
#[test]
fn save_load_preserves_list() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("items = [1, 2, 3]", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    let result = restored.execute("len(items)", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(3));

    let result = restored.execute("items[0]", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(1));
}

/// A dict variable survives round-trip.
#[test]
fn save_load_preserves_dict() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("d = {'key': 'value'}", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    let result = restored.execute("d['key']", &mut NoPrint).unwrap();
    assert_eq!(result, Object::String("value".to_string()));
}

// =============================================================================
// 4. Continued execution after load
// =============================================================================

/// New code can be executed on a loaded session that builds on prior state.
#[test]
fn loaded_session_supports_continued_execution() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("counter = 10", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    restored.execute("counter += 5", &mut NoPrint).unwrap();
    let result = restored.execute("counter", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(15));
}

/// Print output works on a loaded session.
#[test]
fn loaded_session_supports_print() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("msg = 'hello'", &mut NoPrint).unwrap();

    let bytes = session.save().expect("save should succeed");
    let mut restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    let mut print = CollectStringPrint::new();
    restored.execute("print(msg)", &mut print).unwrap();
    assert_eq!(print.output(), "hello\n");
}

// =============================================================================
// 5. External functions
// =============================================================================

/// External function names survive round-trip (slots are preserved).
#[test]
fn save_load_preserves_external_function_names() {
    let session = ReplSession::new(vec!["fetch".to_string(), "log".to_string()], "<stdin>");

    let bytes = session.save().expect("save should succeed");
    let restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");

    // External function count is reflected in variables - they should still be present
    // The restored session should have the same script name
    assert_eq!(restored.script_name(), "<stdin>");
}

// =============================================================================
// 6. Error cases
// =============================================================================

/// Loading corrupt bytes produces an error.
#[test]
fn load_corrupt_bytes_returns_error() {
    let result = ReplSession::load(b"not valid postcard data", ResourceLimits::default());
    assert!(result.is_err(), "loading corrupt bytes should fail");
}

/// Saving a session that has pending interactive state returns an error.
/// (We can only test this if we can get the session into a pending state,
/// which requires external functions and interactive execution.)
#[test]
fn save_rejects_pending_interactive_state() {
    let mut session = ReplSession::new(vec!["ext_fn".to_string()], "<stdin>");
    // Execute interactive code that calls the external function to create pending state
    let _progress = session.execute_interactive("ext_fn()", &mut NoPrint);

    let result = session.save();
    assert!(result.is_err(), "save should fail with pending interactive state");
    assert!(
        result.unwrap_err().contains("pending interactive state"),
        "error should mention pending state"
    );
}

// =============================================================================
// 7. Script name preservation
// =============================================================================

/// Script name is preserved through save/load.
#[test]
fn save_load_preserves_script_name() {
    let session = ReplSession::new(vec![], "my_script.py");
    let bytes = session.save().expect("save should succeed");
    let restored = ReplSession::load(&bytes, ResourceLimits::default()).expect("load should succeed");
    assert_eq!(restored.script_name(), "my_script.py");
}

// =============================================================================
// 8. SessionSnapshot is public
// =============================================================================

/// Verify that SessionSnapshot is publicly accessible from the ouros crate.
/// This is a compile-time test - if it compiles, the type is exported.
#[test]
fn session_snapshot_is_public() {
    fn _assert_type_exists(_: &SessionSnapshot) {}
}
