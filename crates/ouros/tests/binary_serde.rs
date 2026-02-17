//! Tests for binary serialization and deserialization of `Runner` and `RunProgress`.
//!
//! These tests verify that execution state can be serialized with postcard for:
//! - Caching parsed code to avoid re-parsing
//! - Snapshotting execution state for external function calls

use ouros::{NoLimitTracker, Object, RunProgress, Runner, StdPrint};

// === Runner dump/load Tests ===

#[test]
fn ouros_run_dump_load_simple() {
    // Create a runner, dump it, load it, and verify it produces the same result
    let runner = Runner::new("1 + 2".to_owned(), "test.py", vec![], vec![]).unwrap();
    let bytes = runner.dump().unwrap();
    let loaded = Runner::load(&bytes).unwrap();

    let result = loaded.run_no_limits(vec![]).unwrap();
    assert_eq!(result, Object::Int(3));
}

#[test]
fn ouros_run_dump_load_with_inputs() {
    // Test that input names are preserved across dump/load
    let runner = Runner::new(
        "x + y * 2".to_owned(),
        "test.py",
        vec!["x".to_owned(), "y".to_owned()],
        vec![],
    )
    .unwrap();
    let bytes = runner.dump().unwrap();
    let loaded = Runner::load(&bytes).unwrap();

    let result = loaded.run_no_limits(vec![Object::Int(10), Object::Int(5)]).unwrap();
    assert_eq!(result, Object::Int(20));
}

#[test]
fn ouros_run_dump_load_preserves_code() {
    // Verify the code string is preserved
    let code = "def foo(x):\n    return x * 2\nfoo(21)".to_owned();
    let runner = Runner::new(code.clone(), "test.py", vec![], vec![]).unwrap();
    let bytes = runner.dump().unwrap();
    let loaded = Runner::load(&bytes).unwrap();

    assert_eq!(loaded.code(), code);
    let result = loaded.run_no_limits(vec![]).unwrap();
    assert_eq!(result, Object::Int(42));
}

#[test]
fn ouros_run_dump_load_complex_code() {
    // Test with more complex code including functions, loops, conditionals
    let code = r"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

result = []
for i in range(10):
    result.append(fib(i))
result
"
    .to_owned();

    let runner = Runner::new(code, "test.py", vec![], vec![]).unwrap();
    let bytes = runner.dump().unwrap();
    let loaded = Runner::load(&bytes).unwrap();

    let result = loaded.run_no_limits(vec![]).unwrap();
    // First 10 Fibonacci numbers: 0, 1, 1, 2, 3, 5, 8, 13, 21, 34
    let expected = Object::List(vec![
        Object::Int(0),
        Object::Int(1),
        Object::Int(1),
        Object::Int(2),
        Object::Int(3),
        Object::Int(5),
        Object::Int(8),
        Object::Int(13),
        Object::Int(21),
        Object::Int(34),
    ]);
    assert_eq!(result, expected);
}

#[test]
fn ouros_run_dump_load_multiple_runs() {
    // A loaded runner can be run multiple times
    let runner = Runner::new("x * 2".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let bytes = runner.dump().unwrap();
    let loaded = Runner::load(&bytes).unwrap();

    assert_eq!(loaded.run_no_limits(vec![Object::Int(5)]).unwrap(), Object::Int(10));
    assert_eq!(loaded.run_no_limits(vec![Object::Int(21)]).unwrap(), Object::Int(42));
}

// === RunProgress dump/load Tests ===

#[test]
fn run_progress_dump_load_roundtrip() {
    // Start execution with an external function, dump at the call, load and resume
    let runner = Runner::new(
        "ext_fn(42) + 1".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .unwrap();

    let progress = runner.start(vec![], NoLimitTracker, &mut StdPrint).unwrap();

    // Dump the progress at the external call
    let bytes = progress.dump().unwrap();

    // Load it back
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).unwrap();

    // Should still be at the external function call
    let (fn_name, args, _, _call_id, state) = loaded.into_function_call().expect("should be at function call");
    assert_eq!(fn_name, "ext_fn");
    assert_eq!(args, vec![Object::Int(42)]);

    // Resume execution with a return value
    let result = state.run(Object::Int(100), &mut StdPrint).unwrap();
    assert_eq!(result.into_complete().unwrap(), Object::Int(101)); // 100 + 1
}

#[test]
fn run_progress_dump_load_multiple_calls() {
    // Test multiple external calls with dump/load between each
    let runner = Runner::new(
        "x = ext_fn(1); y = ext_fn(2); x + y".to_owned(),
        "test.py",
        vec![],
        vec!["ext_fn".to_owned()],
    )
    .unwrap();

    // First call
    let progress = runner.start(vec![], NoLimitTracker, &mut StdPrint).unwrap();
    let bytes = progress.dump().unwrap();
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).unwrap();
    let (fn_name, args, _, _call_id, state) = loaded.into_function_call().unwrap();
    assert_eq!(fn_name, "ext_fn");
    assert_eq!(args, vec![Object::Int(1)]);

    // Resume first call
    let progress = state.run(Object::Int(10), &mut StdPrint).unwrap();

    // Dump/load at second call
    let bytes = progress.dump().unwrap();
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).unwrap();
    let (fn_name, args, _, _call_id, state) = loaded.into_function_call().unwrap();
    assert_eq!(fn_name, "ext_fn");
    assert_eq!(args, vec![Object::Int(2)]);

    // Resume second call to completion
    let result = state.run(Object::Int(20), &mut StdPrint).unwrap();
    assert_eq!(result.into_complete().unwrap(), Object::Int(30)); // 10 + 20
}

#[test]
fn run_progress_complete_roundtrip() {
    // When execution completes, we can still dump/load the Complete variant
    let runner = Runner::new("1 + 2".to_owned(), "test.py", vec![], vec![]).unwrap();
    let progress = runner.start(vec![], NoLimitTracker, &mut StdPrint).unwrap();

    let bytes = progress.dump().unwrap();
    let loaded: RunProgress<NoLimitTracker> = RunProgress::load(&bytes).unwrap();

    assert_eq!(loaded.into_complete().unwrap(), Object::Int(3));
}
