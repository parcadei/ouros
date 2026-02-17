//! Tests for `ReplSession::set_variable` and `ReplSession::delete_variable`.
//!
//! These tests verify that variables can be injected into a REPL session from
//! the host side (bypassing Python parsing) and that `delete_variable` properly
//! removes them. Each test checks both the direct API and integration with the
//! interpreter (e.g., injected variables are visible to subsequent `execute` calls).

use ouros::{CollectStringPrint, DictPairs, Object, ReplSession};

// =============================================================================
// 1. set_variable — basic types
// =============================================================================

/// Setting a new integer variable succeeds and is immediately readable.
#[test]
fn set_variable_new_int() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("x", Object::Int(42)).unwrap();
    let val = session.get_variable("x").unwrap();
    assert_eq!(val, Object::Int(42));
}

/// Setting a new string variable succeeds and is immediately readable.
#[test]
fn set_variable_new_string() {
    let mut session = ReplSession::new(vec![], "test");
    session
        .set_variable("name", Object::String("hello".to_string()))
        .unwrap();
    let val = session.get_variable("name").unwrap();
    assert_eq!(val, Object::String("hello".to_string()));
}

/// Setting a new list variable preserves element order and values.
#[test]
fn set_variable_new_list() {
    let mut session = ReplSession::new(vec![], "test");
    let list = Object::List(vec![Object::Int(1), Object::Int(2), Object::Int(3)]);
    session.set_variable("nums", list.clone()).unwrap();
    let val = session.get_variable("nums").unwrap();
    assert_eq!(val, list);
}

/// Setting a new dict variable preserves key-value pairs.
#[test]
fn set_variable_new_dict() {
    let mut session = ReplSession::new(vec![], "test");
    let pairs: DictPairs = vec![
        (Object::String("a".to_string()), Object::Int(1)),
        (Object::String("b".to_string()), Object::Int(2)),
    ]
    .into();
    let dict = Object::Dict(pairs.clone());
    session.set_variable("d", dict).unwrap();
    let val = session.get_variable("d").unwrap();
    if let Object::Dict(retrieved_pairs) = val {
        assert_eq!(retrieved_pairs, pairs);
    } else {
        panic!("expected Dict, got {val:?}");
    }
}

/// Setting a variable to None is valid and retrievable.
#[test]
fn set_variable_new_none() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("n", Object::None).unwrap();
    let val = session.get_variable("n").unwrap();
    assert_eq!(val, Object::None);
}

/// Setting a variable to a boolean is valid and retrievable.
#[test]
fn set_variable_new_bool() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("flag", Object::Bool(true)).unwrap();
    let val = session.get_variable("flag").unwrap();
    assert_eq!(val, Object::Bool(true));
}

/// Setting a variable to a float is valid and retrievable.
#[test]
fn set_variable_new_float() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("ratio", Object::Float(1.234_567)).unwrap();
    let val = session.get_variable("ratio").unwrap();
    assert_eq!(val, Object::Float(1.234_567));
}

// =============================================================================
// 2. set_variable — overwrite and type change
// =============================================================================

/// Overwriting an existing variable with the same type updates its value.
#[test]
fn set_variable_overwrite_existing() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("x", Object::Int(1)).unwrap();
    assert_eq!(session.get_variable("x").unwrap(), Object::Int(1));

    session.set_variable("x", Object::Int(2)).unwrap();
    assert_eq!(session.get_variable("x").unwrap(), Object::Int(2));
}

/// Overwriting a variable with a different type succeeds.
#[test]
fn set_variable_overwrite_type_change() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("x", Object::Int(42)).unwrap();
    session
        .set_variable("x", Object::String("now a string".to_string()))
        .unwrap();
    let val = session.get_variable("x").unwrap();
    assert_eq!(val, Object::String("now a string".to_string()));
}

// =============================================================================
// 3. set_variable — does not corrupt existing state
// =============================================================================

/// Setting a new variable via the API does not disturb variables created via execute.
#[test]
fn set_variable_does_not_corrupt_existing() {
    let mut session = ReplSession::new(vec![], "test");
    let mut printer = CollectStringPrint::new();
    session.execute("a = 10", &mut printer).unwrap();
    session.execute("b = 20", &mut printer).unwrap();

    session.set_variable("c", Object::Int(30)).unwrap();

    assert_eq!(session.get_variable("a").unwrap(), Object::Int(10));
    assert_eq!(session.get_variable("b").unwrap(), Object::Int(20));
    assert_eq!(session.get_variable("c").unwrap(), Object::Int(30));
}

// =============================================================================
// 4. set_variable — integration with execute
// =============================================================================

/// A variable injected via set_variable is visible to execute() arithmetic.
#[test]
fn set_variable_then_execute_reads_it() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("x", Object::Int(42)).unwrap();

    let mut printer = CollectStringPrint::new();
    let result = session.execute("x + 1", &mut printer).unwrap();
    assert_eq!(result, Object::Int(43));
}

/// A string variable injected via set_variable is usable in an f-string.
#[test]
fn set_variable_then_execute_uses_in_fstring() {
    let mut session = ReplSession::new(vec![], "test");
    session
        .set_variable("name", Object::String("world".to_string()))
        .unwrap();

    let mut printer = CollectStringPrint::new();
    let result = session.execute("f'hello {name}'", &mut printer).unwrap();
    assert_eq!(result, Object::String("hello world".to_string()));
}

// =============================================================================
// 5. set_variable — error cases
// =============================================================================

/// Attempting to overwrite an external function name is rejected.
#[test]
fn set_variable_rejects_external_function_name() {
    let mut session = ReplSession::new(vec!["fetch".to_string()], "test");
    let result = session.set_variable("fetch", Object::Int(1));
    assert!(result.is_err(), "set_variable should reject external function names");
}

// =============================================================================
// 6. set_variable — bulk operations
// =============================================================================

/// Multiple variables can be injected sequentially and all remain accessible.
#[test]
fn set_variable_multiple_new_variables() {
    let mut session = ReplSession::new(vec![], "test");
    for i in 0..10 {
        session.set_variable(&format!("var_{i}"), Object::Int(i)).unwrap();
    }
    for i in 0..10 {
        let val = session.get_variable(&format!("var_{i}")).unwrap();
        assert_eq!(val, Object::Int(i));
    }
}

// =============================================================================
// 7. delete_variable — basic behavior
// =============================================================================

/// Deleting an existing variable removes it and returns true.
#[test]
fn delete_variable_existing() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("x", Object::Int(42)).unwrap();
    assert!(session.get_variable("x").is_some());

    let existed = session.delete_variable("x").unwrap();
    assert!(existed, "delete_variable should return true for existing variable");
    assert!(
        session.get_variable("x").is_none(),
        "variable should be gone after delete"
    );
}

/// Deleting a nonexistent variable returns false without error.
#[test]
fn delete_variable_nonexistent() {
    let mut session = ReplSession::new(vec![], "test");
    let existed = session.delete_variable("nope").unwrap();
    assert!(!existed, "delete_variable should return false for nonexistent variable");
}

/// A variable can be re-set after deletion.
#[test]
fn delete_variable_then_set_again() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("x", Object::Int(1)).unwrap();
    session.delete_variable("x").unwrap();
    session.set_variable("x", Object::Int(2)).unwrap();
    assert_eq!(session.get_variable("x").unwrap(), Object::Int(2));
}

/// Attempting to delete an external function name is rejected.
#[test]
fn delete_variable_rejects_external_function() {
    let mut session = ReplSession::new(vec!["fetch".to_string()], "test");
    let result = session.delete_variable("fetch");
    assert!(result.is_err(), "delete_variable should reject external function names");
}

/// Deleting one variable does not affect other variables.
#[test]
fn delete_variable_does_not_affect_others() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("a", Object::Int(1)).unwrap();
    session.set_variable("b", Object::Int(2)).unwrap();
    session.delete_variable("a").unwrap();
    assert!(session.get_variable("a").is_none());
    assert_eq!(session.get_variable("b").unwrap(), Object::Int(2));
}

// =============================================================================
// 8. set_variable — nested/complex structures
// =============================================================================

/// A nested list-of-dicts structure round-trips through set/get.
#[test]
fn set_variable_roundtrip_nested_structure() {
    let mut session = ReplSession::new(vec![], "test");
    let nested = Object::List(vec![
        Object::Dict(vec![(Object::String("key".to_string()), Object::Int(1))].into()),
        Object::Tuple(vec![Object::Bool(true), Object::None]),
    ]);
    session.set_variable("data", nested.clone()).unwrap();
    let val = session.get_variable("data").unwrap();
    assert_eq!(val, nested);
}

// =============================================================================
// 9. set_variable — appears in list_variables
// =============================================================================

/// A variable injected via set_variable appears in list_variables.
#[test]
fn set_variable_appears_in_list_variables() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("injected", Object::Int(99)).unwrap();

    let vars = session.list_variables();
    let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        names.contains(&"injected"),
        "injected variable should appear in list_variables, got: {names:?}"
    );
}

/// A deleted variable no longer appears in list_variables.
#[test]
fn delete_variable_removed_from_list_variables() {
    let mut session = ReplSession::new(vec![], "test");
    session.set_variable("temp", Object::Int(1)).unwrap();
    session.delete_variable("temp").unwrap();

    let vars = session.list_variables();
    let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
    assert!(
        !names.contains(&"temp"),
        "deleted variable should not appear in list_variables"
    );
}
