//! TDD tests for REPL support in the Ouros interpreter.
//!
//! These tests define the behavioral specification for `ReplSession`, the persistent REPL
//! session type that maintains state (heap, namespaces, interned strings, functions) across
//! multiple `execute()` calls. The tests are written BEFORE the implementation exists and
//! are expected to fail to compile until `ReplSession` is implemented.
//!
//! The specification is defined in `thoughts/repl-build/spec.md`.

use ouros::{CollectStringPrint, NoPrint, Object, ReplSession};

// =============================================================================
// 1. ReplSession Creation
// =============================================================================

/// A fresh ReplSession can be created with no external functions and a default script name.
#[test]
fn create_session_with_defaults() {
    let session = ReplSession::new(vec![], "<stdin>");
    assert_eq!(
        session.script_name(),
        "<stdin>",
        "script name should match what was passed to new()"
    );
}

/// A fresh session starts with no variables defined.
#[test]
fn fresh_session_has_empty_namespace() {
    let session = ReplSession::new(vec![], "<stdin>");
    let vars = session.list_variables();
    assert!(vars.is_empty(), "fresh session should have no variables, got: {vars:?}",);
}

// =============================================================================
// 2. Basic Execute
// =============================================================================

/// Executing an assignment statement returns None (assignments are not expressions).
#[test]
fn execute_assignment_returns_none() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let result = session.execute("x = 42", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::None,
        "assignment should return None, not the assigned value"
    );
}

/// Executing a bare expression returns its value.
#[test]
fn execute_expression_returns_value() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let result = session.execute("1 + 2", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(3), "expression '1 + 2' should return Int(3)");
}

/// After assigning a variable, evaluating that variable returns its value.
#[test]
fn execute_variable_after_assignment() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();
    let result = session.execute("x", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(42), "variable 'x' should be 42 after assignment");
}

/// Arithmetic using a previously assigned variable works correctly.
#[test]
fn execute_arithmetic_with_previous_variable() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();
    let result = session.execute("x + 1", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(43),
        "expression 'x + 1' should return 43 when x is 42"
    );
}

// =============================================================================
// 3. Variable Persistence Across Lines
// =============================================================================

/// Variables defined in one execute() call persist to the next.
#[test]
fn variable_persists_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("a = 10", &mut NoPrint).unwrap();
    session.execute("b = 20", &mut NoPrint).unwrap();
    let result = session.execute("a + b", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(30), "a + b should be 30 when a=10 and b=20");
}

/// A function defined in one line can be called in a subsequent line.
#[test]
fn function_defined_then_called() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session
        .execute("def double(n):\n    return n * 2", &mut NoPrint)
        .unwrap();
    let result = session.execute("double(21)", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(42), "calling double(21) should return 42");
}

/// A class defined in one line can be instantiated in a subsequent line.
#[test]
fn class_defined_then_instantiated() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session
        .execute(
            "class Point:\n    def __init__(self, x, y):\n        self.x = x\n        self.y = y",
            &mut NoPrint,
        )
        .unwrap();
    session.execute("p = Point(3, 4)", &mut NoPrint).unwrap();
    let result = session.execute("p.x + p.y", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(7), "p.x + p.y should be 7 for Point(3, 4)");
}

/// Reassigning a variable reuses the same namespace slot (no slot leak).
#[test]
fn reassignment_reuses_slot() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();

    // After first assignment, check the variable count
    let vars_after_first = session.list_variables();
    let count_after_first = vars_after_first.len();

    session.execute("x = 2", &mut NoPrint).unwrap();

    // After reassignment, the variable count should not increase
    let vars_after_second = session.list_variables();
    let count_after_second = vars_after_second.len();

    assert_eq!(
        count_after_first, count_after_second,
        "reassigning 'x' should not create a new slot"
    );

    let result = session.execute("x", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(2), "x should be 2 after reassignment");
}

/// Multiple variables accumulate across lines.
#[test]
fn multiple_variables_accumulate() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("a = 1", &mut NoPrint).unwrap();
    session.execute("b = 2", &mut NoPrint).unwrap();
    session.execute("c = 3", &mut NoPrint).unwrap();

    let vars = session.list_variables();
    assert!(
        vars.len() >= 3,
        "should have at least 3 variables after defining a, b, c; got {}",
        vars.len()
    );

    let result = session.execute("a + b + c", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(6), "a + b + c should be 6");
}

// =============================================================================
// 4. Interns Merging
// =============================================================================

/// A string interned in line 1 has the same identity when referenced in line 2.
/// This tests that the InternerBuilder is shared across REPL lines, not recreated.
#[test]
fn string_identity_preserved_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    // The string "hello" gets interned during line 1
    session.execute("s = 'hello'", &mut NoPrint).unwrap();
    // When we reference the same string literal in line 2, it should be the same interned string
    let result = session.execute("s == 'hello'", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Bool(true),
        "same string literal across REPL lines should compare equal"
    );
}

/// New strings in line 2 are properly interned without colliding with line 1 strings.
#[test]
fn new_strings_in_later_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("a = 'first'", &mut NoPrint).unwrap();
    session.execute("b = 'second'", &mut NoPrint).unwrap();
    let result = session.execute("a + ' ' + b", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::String("first second".to_string()),
        "string concatenation across REPL lines should work"
    );
}

/// A function defined in line 1 retains a valid FunctionId when called in line 2.
#[test]
fn function_id_valid_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session
        .execute("def greet(name):\n    return 'Hello, ' + name", &mut NoPrint)
        .unwrap();
    let result = session.execute("greet('world')", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::String("Hello, world".to_string()),
        "function from line 1 should work correctly in line 2"
    );
}

// =============================================================================
// 5. Namespace Growth
// =============================================================================

/// After defining 2 variables, the session reports at least 2 defined variables.
#[test]
fn namespace_grows_with_new_variables() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();
    session.execute("y = 2", &mut NoPrint).unwrap();

    let vars = session.list_variables();
    assert!(
        vars.len() >= 2,
        "namespace should have at least 2 variables after defining x and y; got {}",
        vars.len()
    );
}

/// After a third variable is introduced, the namespace has at least 3 slots.
#[test]
fn namespace_grows_incrementally() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();
    session.execute("y = 2", &mut NoPrint).unwrap();

    let count_after_two = session.list_variables().len();

    session.execute("z = 3", &mut NoPrint).unwrap();

    let count_after_three = session.list_variables().len();
    assert!(
        count_after_three > count_after_two,
        "defining a new variable should increase namespace size; was {count_after_two}, now {count_after_three}",
    );
}

/// Existing variable values are untouched when the namespace grows.
#[test]
fn existing_values_untouched_on_namespace_growth() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 100", &mut NoPrint).unwrap();
    session.execute("y = 200", &mut NoPrint).unwrap();

    // Introduce a new variable, causing namespace growth
    session.execute("z = 300", &mut NoPrint).unwrap();

    // Verify existing values are unchanged
    let x_val = session.execute("x", &mut NoPrint).unwrap();
    let y_val = session.execute("y", &mut NoPrint).unwrap();
    let z_val = session.execute("z", &mut NoPrint).unwrap();

    assert_eq!(x_val, Object::Int(100), "x should still be 100");
    assert_eq!(y_val, Object::Int(200), "y should still be 200");
    assert_eq!(z_val, Object::Int(300), "z should be 300");
}

// =============================================================================
// 6. Error Recovery
// =============================================================================

/// An exception in one execute() call does not prevent subsequent calls from succeeding.
#[test]
fn exception_does_not_prevent_next_line() {
    let mut session = ReplSession::new(vec![], "<stdin>");

    // Line 1: causes a NameError (undefined_var is not defined)
    let err = session.execute("undefined_var", &mut NoPrint);
    assert!(err.is_err(), "referencing an undefined variable should return an error");

    // Line 2: should succeed despite the previous error
    let result = session.execute("42", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(42),
        "session should recover after an error and execute the next line"
    );
}

/// After a NameError, defining the missing name in the next line works.
#[test]
fn define_name_after_name_error() {
    let mut session = ReplSession::new(vec![], "<stdin>");

    // Line 1: NameError because 'x' is not defined
    let err = session.execute("x", &mut NoPrint);
    assert!(err.is_err(), "should get NameError for undefined 'x'");

    // Line 2: define 'x'
    session.execute("x = 99", &mut NoPrint).unwrap();

    // Line 3: now 'x' is available
    let result = session.execute("x", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(99),
        "x should be 99 after defining it post-NameError"
    );
}

/// A syntax error in one line does not corrupt session state.
#[test]
fn syntax_error_preserves_state() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();

    // Line 2: syntax error (parse failure) — should not mutate session state
    let err = session.execute("def @@@", &mut NoPrint);
    assert!(err.is_err(), "syntax error should return an error");

    // Line 3: previous state should still be intact
    let result = session.execute("x", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(42),
        "syntax error should not corrupt previous session state"
    );
}

// =============================================================================
// 7. Implicit Return
// =============================================================================

/// The last expression in a line is implicitly returned (REPL behavior).
#[test]
fn last_expression_is_returned() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let result = session.execute("2 + 3", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(5), "bare expression '2 + 3' should return Int(5)");
}

/// An assignment statement returns None because the last statement is not an expression.
#[test]
fn assignment_returns_none() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let result = session.execute("y = 10", &mut NoPrint).unwrap();
    assert_eq!(result, Object::None, "assignment should return None");
}

/// An expression as the last statement returns its value, even after prior statements.
#[test]
fn expression_after_statements_returns_value() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let result = session.execute("x = 5\nx * 2", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(10),
        "the last expression 'x * 2' should be the return value"
    );
}

/// print() is a side-effect function that returns None.
#[test]
fn print_returns_none() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let mut writer = CollectStringPrint::new();
    let result = session.execute("print('hi')", &mut writer).unwrap();
    assert_eq!(result, Object::None, "print() should return None");
    assert_eq!(
        writer.output(),
        "hi\n",
        "print() should have written to the print writer"
    );
}

// =============================================================================
// 8. Resource Limits
// =============================================================================

/// An infinite loop with a time limit times out but does not destroy the session.
/// The session should be usable for the next execute() call.
#[test]
fn infinite_loop_timeout_session_survives() {
    use std::time::Duration;

    use ouros::{LimitedTracker, ResourceLimits};

    let mut session = ReplSession::new(vec![], "<stdin>");

    // Line 1: define a safe variable
    session.execute("x = 1", &mut NoPrint).unwrap();

    // Line 2: infinite loop with a time limit — should timeout
    let limits = ResourceLimits {
        max_duration: Some(Duration::from_millis(50)),
        ..Default::default()
    };
    let tracker = LimitedTracker::new(limits);
    let err = session.execute_with_limits("while True: pass", tracker, &mut NoPrint);
    assert!(err.is_err(), "infinite loop should timeout and return an error");

    // Line 3: session should still work after timeout
    let result = session.execute("x + 1", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(2),
        "session should survive after a timeout and still access prior state"
    );
}

// =============================================================================
// 9. Complex State
// =============================================================================

/// A list created in line 1, mutated in line 2, and read in line 3 has the correct value.
#[test]
fn list_mutation_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("nums = [1, 2, 3]", &mut NoPrint).unwrap();
    session.execute("nums.append(4)", &mut NoPrint).unwrap();
    let result = session.execute("len(nums)", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(4), "list should have 4 elements after append");
}

/// Dict operations persist correctly across lines.
#[test]
fn dict_operations_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("d = {}", &mut NoPrint).unwrap();
    session.execute("d['a'] = 1", &mut NoPrint).unwrap();
    session.execute("d['b'] = 2", &mut NoPrint).unwrap();
    let result = session.execute("d['a'] + d['b']", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(3), "dict values should persist across lines");
}

/// Nested function calls work across lines.
#[test]
fn nested_function_calls_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session
        .execute("def add(a, b):\n    return a + b", &mut NoPrint)
        .unwrap();
    session
        .execute("def mul(a, b):\n    return a * b", &mut NoPrint)
        .unwrap();
    let result = session.execute("add(mul(2, 3), mul(4, 5))", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(26), "add(mul(2,3), mul(4,5)) = add(6, 20) = 26");
}

/// A closure defined in one line can capture a variable from a previous line.
#[test]
fn closure_captures_variable_from_previous_line() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("base = 100", &mut NoPrint).unwrap();
    session
        .execute("def offset(n):\n    return base + n", &mut NoPrint)
        .unwrap();
    let result = session.execute("offset(5)", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::Int(105),
        "closure should capture 'base' from the global namespace"
    );
}

/// Multiple functions defined across separate lines can call each other.
#[test]
fn multiple_functions_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session
        .execute("def square(n):\n    return n * n", &mut NoPrint)
        .unwrap();
    session
        .execute(
            "def sum_of_squares(a, b):\n    return square(a) + square(b)",
            &mut NoPrint,
        )
        .unwrap();
    let result = session.execute("sum_of_squares(3, 4)", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(25), "sum_of_squares(3, 4) = 9 + 16 = 25");
}

/// List indexing and slicing work across lines.
#[test]
fn list_indexing_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("items = [10, 20, 30, 40, 50]", &mut NoPrint).unwrap();
    let result = session.execute("items[2]", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(30), "items[2] should be 30");
}

/// String methods work on strings created in previous lines.
#[test]
fn string_methods_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("msg = 'hello world'", &mut NoPrint).unwrap();
    let result = session.execute("msg.upper()", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::String("HELLO WORLD".to_string()),
        "msg.upper() should return the uppercased string"
    );
}

/// Boolean expressions with variables from previous lines.
#[test]
fn boolean_expressions_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 10", &mut NoPrint).unwrap();
    session.execute("y = 20", &mut NoPrint).unwrap();
    let result = session.execute("x < y", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Bool(true), "10 < 20 should be True");
}

/// get_variable() returns the value of a named variable.
#[test]
fn get_variable_returns_value() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();

    let val = session.get_variable("x");
    assert_eq!(
        val,
        Some(Object::Int(42)),
        "get_variable('x') should return Some(Int(42))"
    );
}

/// get_variable() returns None for undefined variables.
#[test]
fn get_variable_returns_none_for_undefined() {
    let session = ReplSession::new(vec![], "<stdin>");
    let val = session.get_variable("nonexistent");
    assert_eq!(val, None, "get_variable for undefined name should return None");
}

/// list_variables() includes variable names and type descriptions.
#[test]
fn list_variables_includes_types() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 42", &mut NoPrint).unwrap();
    session.execute("s = 'hello'", &mut NoPrint).unwrap();

    let vars = session.list_variables();
    // Should have at least x and s
    let names: Vec<&str> = vars.iter().map(|(name, _): &(String, String)| name.as_str()).collect();
    assert!(
        names.contains(&"x"),
        "list_variables should include 'x', got: {names:?}",
    );
    assert!(
        names.contains(&"s"),
        "list_variables should include 's', got: {names:?}",
    );
}

/// A for loop modifies state that persists after the loop.
#[test]
fn for_loop_state_persists() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session
        .execute("total = 0\nfor i in range(5):\n    total += i", &mut NoPrint)
        .unwrap();
    let result = session.execute("total", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(10), "sum of 0..5 should be 10");
}

/// Redefining a function replaces the old definition.
#[test]
fn function_redefinition_replaces_old() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("def f():\n    return 1", &mut NoPrint).unwrap();
    let result1 = session.execute("f()", &mut NoPrint).unwrap();
    assert_eq!(result1, Object::Int(1), "first f() should return 1");

    session.execute("def f():\n    return 2", &mut NoPrint).unwrap();
    let result2 = session.execute("f()", &mut NoPrint).unwrap();
    assert_eq!(result2, Object::Int(2), "redefined f() should return 2");
}

/// Multiple errors in sequence do not accumulate corruption.
#[test]
fn multiple_errors_no_corruption() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 1", &mut NoPrint).unwrap();

    // Several errors in a row
    assert!(session.execute("undefined1", &mut NoPrint).is_err());
    assert!(session.execute("undefined2", &mut NoPrint).is_err());
    assert!(session.execute("1 / 0", &mut NoPrint).is_err());

    // State should still be intact
    let result = session.execute("x", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(1), "x should still be 1 after multiple errors");
}

/// Conditional logic works with state from previous lines.
#[test]
fn conditional_with_previous_state() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("x = 10", &mut NoPrint).unwrap();
    let result = session.execute("'big' if x > 5 else 'small'", &mut NoPrint).unwrap();
    assert_eq!(
        result,
        Object::String("big".to_string()),
        "conditional should use x=10 from previous line"
    );
}

/// Tuple unpacking across lines.
#[test]
fn tuple_unpacking_across_lines() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    session.execute("a, b, c = 1, 2, 3", &mut NoPrint).unwrap();
    let result = session.execute("a + b + c", &mut NoPrint).unwrap();
    assert_eq!(result, Object::Int(6), "tuple unpacking should define a=1, b=2, c=3");
}
