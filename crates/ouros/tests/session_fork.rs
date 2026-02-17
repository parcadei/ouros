//! Tests for `ReplSession::fork()` — independent deep copy of session state.
//!
//! These tests verify that forking a REPL session creates a fully independent
//! clone: changes to the original do not affect the fork and vice versa.

use ouros::{CollectStringPrint, NoPrint, Object, ReplSession};

// =============================================================================
// 1. Basic Fork Creation
// =============================================================================

/// Forking a fresh session produces a session that can execute code.
#[test]
fn fork_fresh_session() {
    let session = ReplSession::new(vec![], "<stdin>");
    let mut forked = session.fork();

    let result = forked.execute("1 + 1", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(2), "forked session should execute code");
}

/// Forked session inherits the script name from the original.
#[test]
fn fork_preserves_script_name() {
    let session = ReplSession::new(vec![], "my_script.py");
    let forked = session.fork();

    assert_eq!(
        forked.script_name(),
        "my_script.py",
        "forked session should keep the original script name"
    );
}

/// Forking after executing code preserves previously defined variables.
#[test]
fn fork_preserves_existing_variables() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();

    let mut forked = session.fork();
    let result = forked.execute("x", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(42),
        "forked session should see variables defined before fork"
    );
}

// =============================================================================
// 2. Independence After Fork
// =============================================================================

/// Changes in the original session do not affect the fork.
#[test]
fn original_changes_do_not_affect_fork() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    // Mutate x in original
    session.execute("x = 999", &mut NoPrint).unwrap();

    // Fork should still see x = 1
    let result = forked.execute("x", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(1),
        "fork should not see changes made to original after fork"
    );
}

/// Changes in the fork do not affect the original session.
#[test]
fn fork_changes_do_not_affect_original() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    // Mutate x in fork
    forked.execute("x = 999", &mut NoPrint).unwrap();

    // Original should still see x = 1
    let result = session.execute("x", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(1), "original should not see changes made to fork");
}

/// New variables in the fork are not visible in the original.
#[test]
fn new_fork_variables_invisible_in_original() {
    let mut session = ReplSession::new(vec![], "<stdin>");

    let mut forked = session.fork();
    forked.execute("y = 100", &mut NoPrint).unwrap();

    // Accessing y in original should fail
    let result = session.execute("y", &mut NoPrint);
    assert!(
        result.is_err(),
        "original should not see variables defined only in fork"
    );
}

/// Both sessions can diverge independently with different code.
#[test]
fn sessions_diverge_independently() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 10", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    // Run different code in each
    session.execute("x = x + 1", &mut NoPrint).unwrap();
    forked.execute("x = x * 2", &mut NoPrint).unwrap();

    let orig_result = session.execute("x", &mut NoPrint).unwrap();
    let fork_result = forked.execute("x", &mut NoPrint).unwrap();

    assert_eq!(orig_result, Object::Int(11), "original should have x = 11");
    assert_eq!(fork_result, Object::Int(20), "fork should have x = 20");
}

// =============================================================================
// 3. Heap Independence
// =============================================================================

/// Heap objects (like lists) are deep-copied — mutations to one don't affect the other.
#[test]
fn fork_deep_copies_heap_objects() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("lst = [1, 2, 3]", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    // Mutate the list in the original
    session.execute("lst.append(4)", &mut NoPrint).unwrap();

    // Fork should still have the original list
    let fork_result = forked.execute("len(lst)", &mut NoPrint).unwrap();
    assert_eq!(
        fork_result,
        Object::Int(3),
        "forked list should not see append from original"
    );

    let orig_result = session.execute("len(lst)", &mut NoPrint).unwrap();
    assert_eq!(
        orig_result,
        Object::Int(4),
        "original list should have the appended element"
    );
}

/// Strings defined before fork are preserved in the fork.
#[test]
fn fork_preserves_string_values() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("msg = 'hello world'", &mut NoPrint).unwrap();

    let mut forked = session.fork();
    let result = forked.execute("msg", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::String("hello world".into()),
        "forked session should preserve string values"
    );
}

/// Dict objects are deep-copied across fork.
#[test]
fn fork_deep_copies_dicts() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("d = {'a': 1}", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    // Mutate dict in original
    session.execute("d['b'] = 2", &mut NoPrint).unwrap();

    // Fork should not have the new key
    let fork_result = forked.execute("len(d)", &mut NoPrint).unwrap();
    assert_eq!(fork_result, Object::Int(1), "forked dict should not see new key");

    let orig_result = session.execute("len(d)", &mut NoPrint).unwrap();
    assert_eq!(orig_result, Object::Int(2), "original dict should have both keys");
}

// =============================================================================
// 4. Functions Across Fork
// =============================================================================

/// Functions defined before fork are callable in both sessions.
#[test]
fn fork_preserves_functions() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("def double(n): return n * 2", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    let orig_result = session.execute("double(5)", &mut NoPrint).unwrap();
    let fork_result = forked.execute("double(5)", &mut NoPrint).unwrap();

    assert_eq!(orig_result, Object::Int(10), "original should call double");
    assert_eq!(fork_result, Object::Int(10), "fork should call double");
}

/// New functions in the fork are not visible in the original.
#[test]
fn fork_new_function_invisible_in_original() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let mut forked = session.fork();

    forked.execute("def triple(n): return n * 3", &mut NoPrint).unwrap();

    // Original does not have triple
    let result = session.execute("triple(5)", &mut NoPrint);
    assert!(result.is_err(), "original should not see function defined in fork");
}

// =============================================================================
// 5. Pending Snapshot Not Cloned
// =============================================================================

/// Forking a session that has no pending snapshot works cleanly.
#[test]
fn fork_no_pending_snapshot() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();

    // No pending interactive state - just normal execution
    let mut forked = session.fork();
    let result = forked.execute("x + 1", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(2));
}

// =============================================================================
// 6. Capabilities Preserved
// =============================================================================

/// Fork inherits the capabilities of the original.
#[test]
fn fork_preserves_capabilities() {
    use ouros::capability::{Capability, CapabilitySet};

    let mut session = ReplSession::new(vec!["fetch".to_string()], "<stdin>");
    let caps = CapabilitySet::new(vec![Capability::CallFunction("fetch".into())]);
    session.set_capabilities(Some(caps));

    let forked = session.fork();
    let forked_caps = forked.capabilities().expect("fork should have capabilities");
    assert!(forked_caps.allows_function("fetch"), "fork should allow 'fetch'");
    assert!(!forked_caps.allows_function("exec"), "fork should deny 'exec'");
}

/// Fork with no capabilities (None) keeps None.
#[test]
fn fork_preserves_no_capabilities() {
    let session = ReplSession::new(vec![], "<stdin>");
    let forked = session.fork();
    assert!(
        forked.capabilities().is_none(),
        "fork of session without capabilities should also have None"
    );
}

// =============================================================================
// 7. Print Output Independence
// =============================================================================

/// Print statements in one session do not leak into the other.
#[test]
fn fork_print_output_independent() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 'hello'", &mut NoPrint).unwrap();

    let mut forked = session.fork();

    let mut orig_print = CollectStringPrint::new();
    session.execute("print(x)", &mut orig_print).unwrap();

    let mut fork_print = CollectStringPrint::new();
    forked.execute("print('world')", &mut fork_print).unwrap();

    assert_eq!(orig_print.output(), "hello\n", "original print should be 'hello'");
    assert_eq!(fork_print.output(), "world\n", "fork print should be 'world'");
}

// =============================================================================
// 8. Multiple Forks
// =============================================================================

/// Multiple forks from the same state are all independent.
#[test]
fn multiple_forks_independent() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 0", &mut NoPrint).unwrap();

    let mut fork1 = session.fork();
    let mut fork2 = session.fork();

    fork1.execute("x = 1", &mut NoPrint).unwrap();
    fork2.execute("x = 2", &mut NoPrint).unwrap();

    let r1 = fork1.execute("x", &mut NoPrint).unwrap();
    let r2 = fork2.execute("x", &mut NoPrint).unwrap();
    let r_orig = session.execute("x", &mut NoPrint).unwrap();

    assert_eq!(r1, Object::Int(1));
    assert_eq!(r2, Object::Int(2));
    assert_eq!(r_orig, Object::Int(0));
}

/// Fork of a fork works correctly.
#[test]
fn fork_of_fork() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();

    let mut fork1 = session.fork();
    fork1.execute("x = 2", &mut NoPrint).unwrap();

    let mut fork2 = fork1.fork();
    fork2.execute("x = 3", &mut NoPrint).unwrap();

    let r_orig = session.execute("x", &mut NoPrint).unwrap();
    let r_f1 = fork1.execute("x", &mut NoPrint).unwrap();
    let r_f2 = fork2.execute("x", &mut NoPrint).unwrap();

    assert_eq!(r_orig, Object::Int(1));
    assert_eq!(r_f1, Object::Int(2));
    assert_eq!(r_f2, Object::Int(3));
}
