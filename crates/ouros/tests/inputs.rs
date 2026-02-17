//! Tests for passing input values to the executor.
//!
//! These tests verify that `Object` inputs are correctly converted to `Object`
//! and can be used in Python code execution.

use indexmap::IndexMap;
use ouros::{ExcType, Object, Runner};

// === Immediate Value Tests ===

#[test]
fn input_int() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Int(42)]).unwrap();
    assert_eq!(result, Object::Int(42));
}

#[test]
fn input_int_arithmetic() {
    let ex = Runner::new("x + 1".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Int(41)]).unwrap();
    assert_eq!(result, Object::Int(42));
}

#[test]
fn input_bool_true() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Bool(true)]).unwrap();
    assert_eq!(result, Object::Bool(true));
}

#[test]
fn input_bool_false() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Bool(false)]).unwrap();
    assert_eq!(result, Object::Bool(false));
}

#[test]
fn input_float() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Float(2.5)]).unwrap();
    assert_eq!(result, Object::Float(2.5));
}

#[test]
fn input_none() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::None]).unwrap();
    assert_eq!(result, Object::None);
}

#[test]
fn input_ellipsis() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Ellipsis]).unwrap();
    assert_eq!(result, Object::Ellipsis);
}

// === Heap-Allocated Value Tests ===

#[test]
fn input_string() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::String("hello".to_string())]).unwrap();
    assert_eq!(result, Object::String("hello".to_string()));
}

#[test]
fn input_string_concat() {
    let ex = Runner::new("x + ' world'".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::String("hello".to_string())]).unwrap();
    assert_eq!(result, Object::String("hello world".to_string()));
}

#[test]
fn input_bytes() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Bytes(vec![1, 2, 3])]).unwrap();
    assert_eq!(result, Object::Bytes(vec![1, 2, 3]));
}

#[test]
fn input_list() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::List(vec![Object::Int(1), Object::Int(2)])])
        .unwrap();
    assert_eq!(result, Object::List(vec![Object::Int(1), Object::Int(2)]));
}

#[test]
fn input_list_append() {
    let ex = Runner::new("x.append(3)\nx".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::List(vec![Object::Int(1), Object::Int(2)])])
        .unwrap();
    assert_eq!(
        result,
        Object::List(vec![Object::Int(1), Object::Int(2), Object::Int(3)])
    );
}

#[test]
fn input_tuple() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::Tuple(vec![
            Object::Int(1),
            Object::String("two".to_string()),
        ])])
        .unwrap();
    assert_eq!(
        result,
        Object::Tuple(vec![Object::Int(1), Object::String("two".to_string())])
    );
}

#[test]
fn input_dict() {
    let mut map = IndexMap::new();
    map.insert(Object::String("a".to_string()), Object::Int(1));

    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::dict(map)]).unwrap();

    // Build expected map for comparison
    let mut expected = IndexMap::new();
    expected.insert(Object::String("a".to_string()), Object::Int(1));
    assert_eq!(result, Object::Dict(expected.into()));
}

#[test]
fn input_dict_get() {
    let mut map = IndexMap::new();
    map.insert(Object::String("key".to_string()), Object::Int(42));

    let ex = Runner::new("x['key']".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::dict(map)]).unwrap();
    assert_eq!(result, Object::Int(42));
}

// === Multiple Inputs ===

#[test]
fn multiple_inputs_two() {
    let ex = Runner::new(
        "x + y".to_owned(),
        "test.py",
        vec!["x".to_owned(), "y".to_owned()],
        vec![],
    )
    .unwrap();
    let result = ex.run_no_limits(vec![Object::Int(10), Object::Int(32)]).unwrap();
    assert_eq!(result, Object::Int(42));
}

#[test]
fn multiple_inputs_three() {
    let ex = Runner::new(
        "x + y + z".to_owned(),
        "test.py",
        vec!["x".to_owned(), "y".to_owned(), "z".to_owned()],
        vec![],
    )
    .unwrap();
    let result = ex
        .run_no_limits(vec![Object::Int(10), Object::Int(20), Object::Int(12)])
        .unwrap();
    assert_eq!(result, Object::Int(42));
}

#[test]
fn multiple_inputs_mixed_types() {
    // Create a list from two inputs
    let ex = Runner::new(
        "[x, y]".to_owned(),
        "test.py",
        vec!["x".to_owned(), "y".to_owned()],
        vec![],
    )
    .unwrap();
    let result = ex
        .run_no_limits(vec![Object::Int(1), Object::String("two".to_string())])
        .unwrap();
    assert_eq!(
        result,
        Object::List(vec![Object::Int(1), Object::String("two".to_string())])
    );
}

// === Edge Cases ===

#[test]
fn no_inputs() {
    let ex = Runner::new("42".to_owned(), "test.py", vec![], vec![]).unwrap();
    let result = ex.run_no_limits(vec![]).unwrap();
    assert_eq!(result, Object::Int(42));
}

#[test]
fn nested_list() {
    let ex = Runner::new("x[0][1]".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::List(vec![Object::List(vec![
            Object::Int(1),
            Object::Int(2),
        ])])])
        .unwrap();
    assert_eq!(result, Object::Int(2));
}

#[test]
fn empty_list_input() {
    let ex = Runner::new("len(x)".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::List(vec![])]).unwrap();
    assert_eq!(result, Object::Int(0));
}

#[test]
fn empty_string_input() {
    let ex = Runner::new("len(x)".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::String(String::new())]).unwrap();
    assert_eq!(result, Object::Int(0));
}

// === Exception Input Tests ===

#[test]
fn input_exception() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::Exception {
            exc_type: ExcType::ValueError,
            arg: Some("test message".to_string()),
        }])
        .unwrap();
    assert_eq!(
        result,
        Object::Exception {
            exc_type: ExcType::ValueError,
            arg: Some("test message".to_string()),
        }
    );
}

#[test]
fn input_exception_no_arg() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::Exception {
            exc_type: ExcType::TypeError,
            arg: None,
        }])
        .unwrap();
    assert_eq!(
        result,
        Object::Exception {
            exc_type: ExcType::TypeError,
            arg: None,
        }
    );
}

#[test]
fn input_exception_in_list() {
    let ex = Runner::new("x[0]".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex
        .run_no_limits(vec![Object::List(vec![Object::Exception {
            exc_type: ExcType::KeyError,
            arg: Some("key".to_string()),
        }])])
        .unwrap();
    assert_eq!(
        result,
        Object::Exception {
            exc_type: ExcType::KeyError,
            arg: Some("key".to_string()),
        }
    );
}

#[test]
fn input_exception_raise() {
    // Test that an exception passed as input can be raised
    let ex = Runner::new("raise x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Exception {
        exc_type: ExcType::ValueError,
        arg: Some("input error".to_string()),
    }]);
    let exc = result.unwrap_err();
    assert_eq!(exc.exc_type(), ExcType::ValueError);
    assert_eq!(exc.message(), Some("input error"));
}

// === Invalid Input Tests ===

#[test]
fn invalid_input_repr() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    let result = ex.run_no_limits(vec![Object::Repr("some repr".to_string())]);
    assert!(result.is_err(), "Repr should not be a valid input");
}

#[test]
fn invalid_input_repr_nested_in_list() {
    let ex = Runner::new("x".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    // Repr nested inside a list should still be invalid
    let result = ex.run_no_limits(vec![Object::List(vec![Object::Repr("nested repr".to_string())])]);
    assert!(result.is_err(), "Repr nested in list should be invalid");
}

// === Function Parameter Shadowing Tests ===
// These tests verify that function parameters properly shadow script inputs with the same name.

#[test]
fn function_param_shadows_input() {
    // Function parameter `x` should shadow the script input `x`
    let code = "
def foo(x):
    return x + 1

foo(x * 2)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    // x=5 (input), foo(x * 2) = foo(10), inside foo x=10 (param), returns 11
    let result = ex.run_no_limits(vec![Object::Int(5)]).unwrap();
    assert_eq!(result, Object::Int(11));
}

#[test]
fn function_param_shadows_input_multiple_params() {
    // Multiple function parameters should all shadow their corresponding inputs
    let code = "
def add(x, y):
    return x + y

add(x * 10, y * 100)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned(), "y".to_owned()], vec![]).unwrap();
    // x=2, y=3 (inputs), add(20, 300), inside add x=20, y=300, returns 320
    let result = ex.run_no_limits(vec![Object::Int(2), Object::Int(3)]).unwrap();
    assert_eq!(result, Object::Int(320));
}

#[test]
fn function_param_shadows_input_but_global_accessible() {
    // Function parameter shadows input, but other inputs are still accessible as globals
    let code = "
def foo(x):
    return x + y

foo(100)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned(), "y".to_owned()], vec![]).unwrap();
    // x=5, y=3 (inputs), foo(100), inside foo x=100 (param), y=3 (global), returns 103
    let result = ex.run_no_limits(vec![Object::Int(5), Object::Int(3)]).unwrap();
    assert_eq!(result, Object::Int(103));
}

#[test]
fn function_param_shadows_input_accessible_outside() {
    // Script input should still be accessible outside the function that shadows it
    let code = "
def double(x):
    return x * 2

double(10) + x
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    // x=5 (input), double(10) = 20, then 20 + x (global) = 20 + 5 = 25
    let result = ex.run_no_limits(vec![Object::Int(5)]).unwrap();
    assert_eq!(result, Object::Int(25));
}

#[test]
fn function_param_with_default_shadows_input() {
    // Function parameter with default should shadow input when called with argument
    let code = "
def foo(x=100):
    return x + 1

foo(x * 2)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    // x=5 (input), foo(10), inside foo x=10 (param), returns 11
    let result = ex.run_no_limits(vec![Object::Int(5)]).unwrap();
    assert_eq!(result, Object::Int(11));
}

#[test]
fn function_uses_input_as_argument() {
    // Input can be passed as argument, and param shadows inside function
    let code = "
def double(x):
    return x * 2

double(x)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    // x=7 (input), double(7), inside double x=7 (param from arg), returns 14
    let result = ex.run_no_limits(vec![Object::Int(7)]).unwrap();
    assert_eq!(result, Object::Int(14));
}

#[test]
fn function_doesnt_uses_input_as_argument() {
    let code = "
def double(x):
    return x * 2

double(2)
";
    let ex = Runner::new(code.to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
    // x=7 (input), double(7), inside double x=7 (param from arg), returns 14
    let result = ex.run_no_limits(vec![Object::Int(7)]).unwrap();
    assert_eq!(result, Object::Int(4));
}
