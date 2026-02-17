use ouros::Object;

/// Tests for `Object::is_truthy()` - Python's truth value testing rules.

#[test]
fn is_truthy_none_is_falsy() {
    assert!(!Object::None.is_truthy());
}

#[test]
fn is_truthy_ellipsis_is_truthy() {
    assert!(Object::Ellipsis.is_truthy());
}

#[test]
fn is_truthy_false_is_falsy() {
    assert!(!Object::Bool(false).is_truthy());
}

#[test]
fn is_truthy_true_is_truthy() {
    assert!(Object::Bool(true).is_truthy());
}

#[test]
fn is_truthy_zero_int_is_falsy() {
    assert!(!Object::Int(0).is_truthy());
}

#[test]
fn is_truthy_nonzero_int_is_truthy() {
    assert!(Object::Int(1).is_truthy());
    assert!(Object::Int(-1).is_truthy());
    assert!(Object::Int(42).is_truthy());
}

#[test]
fn is_truthy_zero_float_is_falsy() {
    assert!(!Object::Float(0.0).is_truthy());
}

#[test]
fn is_truthy_nonzero_float_is_truthy() {
    assert!(Object::Float(1.0).is_truthy());
    assert!(Object::Float(-0.5).is_truthy());
    assert!(Object::Float(f64::INFINITY).is_truthy());
}

#[test]
fn is_truthy_empty_string_is_falsy() {
    assert!(!Object::String(String::new()).is_truthy());
}

#[test]
fn is_truthy_nonempty_string_is_truthy() {
    assert!(Object::String("hello".to_string()).is_truthy());
    assert!(Object::String(" ".to_string()).is_truthy());
}

#[test]
fn is_truthy_empty_bytes_is_falsy() {
    assert!(!Object::Bytes(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_bytes_is_truthy() {
    assert!(Object::Bytes(vec![0]).is_truthy());
    assert!(Object::Bytes(vec![1, 2, 3]).is_truthy());
}

#[test]
fn is_truthy_empty_list_is_falsy() {
    assert!(!Object::List(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_list_is_truthy() {
    assert!(Object::List(vec![Object::Int(1)]).is_truthy());
}

#[test]
fn is_truthy_empty_tuple_is_falsy() {
    assert!(!Object::Tuple(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_tuple_is_truthy() {
    assert!(Object::Tuple(vec![Object::Int(1)]).is_truthy());
}

#[test]
fn is_truthy_empty_dict_is_falsy() {
    assert!(!Object::dict(vec![]).is_truthy());
}

#[test]
fn is_truthy_nonempty_dict_is_truthy() {
    let dict = vec![(Object::String("key".to_string()), Object::Int(1))];
    assert!(Object::dict(dict).is_truthy());
}

/// Tests for `Object::type_name()` - Python type names.

#[test]
fn type_name() {
    assert_eq!(Object::None.type_name(), "NoneType");
    assert_eq!(Object::Ellipsis.type_name(), "ellipsis");
    assert_eq!(Object::Bool(true).type_name(), "bool");
    assert_eq!(Object::Bool(false).type_name(), "bool");
    assert_eq!(Object::Int(0).type_name(), "int");
    assert_eq!(Object::Int(42).type_name(), "int");
    assert_eq!(Object::Float(0.0).type_name(), "float");
    assert_eq!(Object::Float(2.5).type_name(), "float");
    assert_eq!(Object::String(String::new()).type_name(), "str");
    assert_eq!(Object::String("hello".to_string()).type_name(), "str");
    assert_eq!(Object::Bytes(vec![]).type_name(), "bytes");
    assert_eq!(Object::Bytes(vec![1, 2, 3]).type_name(), "bytes");
    assert_eq!(Object::List(vec![]).type_name(), "list");
    assert_eq!(Object::Tuple(vec![]).type_name(), "tuple");
    assert_eq!(Object::dict(vec![]).type_name(), "dict");
}
