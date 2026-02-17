use ouros::Runner;

/// Test we can reuse exec without borrow checker issues.
#[test]
fn repeat_exec() {
    let ex = Runner::new("1 + 2".to_owned(), "test.py", vec![], vec![]).unwrap();

    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: i64 = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, 3);

    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: i64 = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, 3);
}

#[test]
fn test_get_interned_string() {
    let ex = Runner::new("'foobar'".to_owned(), "test.py", vec![], vec![]).unwrap();

    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: String = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, "foobar");

    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: String = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, "foobar");
}
