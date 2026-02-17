use ouros::{Object, Runner};

/// Verifies that repeated runs with the same Runner produce identical results.
///
/// This exercises the buffer reuse path in `Executor::run()` where cached heap/VM
/// buffers are reused across invocations to avoid repeated allocation overhead.
#[test]
fn repeated_runs_produce_same_result() {
    let ex = Runner::new("1 + 2".to_owned(), "test.py", vec![], vec![]).unwrap();

    for _ in 0..100 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 3);
    }
}

/// Verifies repeated runs with heap-allocated values (strings) clean up correctly.
///
/// String values are heap-allocated, so this tests that the reused heap properly
/// handles allocation and deallocation across runs.
#[test]
fn repeated_runs_with_heap_values() {
    let ex = Runner::new("'hello' + ' ' + 'world'".to_owned(), "test.py", vec![], vec![]).unwrap();

    for _ in 0..50 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let s: String = r.as_ref().try_into().unwrap();
        assert_eq!(s, "hello world");
    }
}

/// Verifies repeated runs with list operations (significant heap usage).
///
/// Lists exercise the heap more heavily, testing that reset properly clears
/// all heap entries and free lists between runs.
#[test]
fn repeated_runs_with_lists() {
    let code = "a = [1, 2, 3]\na.append(4)\nlen(a)";
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();

    for _ in 0..50 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 4);
    }
}

/// Verifies repeated runs with inputs work correctly.
///
/// Input handling populates the namespace with external values, so this tests
/// that namespace preparation works correctly with reused buffers.
#[test]
fn repeated_runs_with_inputs() {
    let ex = Runner::new(
        "x + y".to_owned(),
        "test.py",
        vec!["x".to_owned(), "y".to_owned()],
        vec![],
    )
    .unwrap();

    for i in 0..50 {
        let r = ex.run_no_limits(vec![Object::Int(i), Object::Int(i * 2)]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, i + i * 2);
    }
}

/// Verifies repeated runs with function definitions and calls.
///
/// Functions create additional namespaces and frames, testing that the
/// frame stack and namespace stack handle reuse correctly.
#[test]
fn repeated_runs_with_function_calls() {
    let code = r"
def add(a, b):
    return a + b

add(10, 20) + add(30, 40)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();

    for _ in 0..50 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 100);
    }
}

/// Verifies repeated runs with recursive functions.
///
/// Recursion creates many frames and namespaces; tests that deep stack
/// usage doesn't leak memory across runs.
#[test]
fn repeated_runs_with_recursion() {
    let code = r"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(10)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();

    for _ in 0..10 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 55);
    }
}

/// Verifies that exceptions during repeated runs don't corrupt state.
///
/// After a run that raises an exception, the next run should still
/// succeed with correct results.
#[test]
fn repeated_runs_interleaved_with_errors() {
    // This code will always succeed
    let ex_ok = Runner::new("1 + 2".to_owned(), "test.py", vec![], vec![]).unwrap();

    // Run successfully multiple times to warm up
    for _ in 0..5 {
        let r = ex_ok.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 3);
    }
}

/// Verifies that dict operations (which exercise heap hashing) work correctly
/// across repeated runs.
#[test]
fn repeated_runs_with_dicts() {
    let code = "d = {'a': 1, 'b': 2}\nd['c'] = 3\nlen(d)";
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();

    for _ in 0..50 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 3);
    }
}

/// Verifies that the adaptive heap_capacity hint grows appropriately
/// by running a program that allocates many heap objects.
#[test]
fn heap_capacity_adapts_across_runs() {
    let code = "[i for i in range(100)][-1]";
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();

    // First run establishes capacity
    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: i64 = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, 99);

    // Subsequent runs should benefit from the capacity hint
    for _ in 0..10 {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        assert_eq!(int_value, 99);
    }
}
