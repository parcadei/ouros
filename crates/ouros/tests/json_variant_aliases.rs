//! Tests for Python-style serde aliases on `Object` variants.

use ouros::Object;

#[test]
fn json_deserialize_python_style_primitive_aliases() {
    let string: Object = serde_json::from_str(r#"{"str":"hello"}"#).unwrap();
    let bool_val: Object = serde_json::from_str(r#"{"bool":true}"#).unwrap();
    let int: Object = serde_json::from_str(r#"{"int":42}"#).unwrap();
    let float: Object = serde_json::from_str(r#"{"float":2.5}"#).unwrap();
    let none: Object = serde_json::from_str(r#""NoneType""#).unwrap();

    assert_eq!(string, Object::String("hello".to_owned()));
    assert_eq!(bool_val, Object::Bool(true));
    assert_eq!(int, Object::Int(42));
    assert_eq!(float, Object::Float(2.5));
    assert_eq!(none, Object::None);
}

#[test]
fn json_deserialize_python_style_container_aliases() {
    let list: Object = serde_json::from_str(r#"{"list":[{"int":1},{"str":"two"}]}"#).unwrap();
    assert_eq!(
        list,
        Object::List(vec![Object::Int(1), Object::String("two".to_owned())])
    );

    let tuple: Object = serde_json::from_str(r#"{"tuple":[{"bool":false}]}"#).unwrap();
    assert_eq!(tuple, Object::Tuple(vec![Object::Bool(false)]));

    let dict: Object = serde_json::from_str(r#"{"dict":[[{"str":"k"},{"int":3}]]}"#).unwrap();
    if let Object::Dict(pairs) = dict {
        let pairs_vec: Vec<_> = pairs.into_iter().collect();
        assert_eq!(pairs_vec, vec![(Object::String("k".to_owned()), Object::Int(3))]);
    } else {
        panic!("expected Dict");
    }
}
