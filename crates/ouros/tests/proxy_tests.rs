use ouros::{ExcType, NoPrint, Object, ReplError, ReplProgress, ReplSession};

fn expect_function_call(progress: ReplProgress) -> (String, Vec<Object>, Vec<(Object, Object)>, u32) {
    match progress {
        ReplProgress::FunctionCall {
            function_name,
            args,
            kwargs,
            call_id,
        } => (function_name, args, kwargs, call_id),
        other => panic!("expected function call, got {other:?}"),
    }
}

fn expect_proxy_call(progress: ReplProgress) -> (u32, String, Vec<Object>, Vec<(Object, Object)>, u32) {
    match progress {
        ReplProgress::ProxyCall {
            proxy_id,
            method,
            args,
            kwargs,
            call_id,
        } => (proxy_id, method, args, kwargs, call_id),
        other => panic!("expected proxy call, got {other:?}"),
    }
}

fn expect_complete(progress: ReplProgress) -> Object {
    match progress {
        ReplProgress::Complete(value) => value,
        other => panic!("expected complete progress, got {other:?}"),
    }
}

#[test]
fn resume_with_proxy_value_stores_it_as_variable() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, args, _kwargs, _call_id) = expect_function_call(progress);
    assert_eq!(args, vec![Object::Int(1)]);

    let result = session.resume(Object::Proxy(7), &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::None);
    assert_eq!(session.get_variable("p"), Some(Object::Proxy(7)));
}

#[test]
fn proxy_repr_displays_as_proxy_id() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(11), &mut NoPrint).unwrap();

    let repr_value = session.execute("repr(p)", &mut NoPrint).unwrap();
    assert_eq!(repr_value, Object::String("<proxy #11>".to_owned()));
}

#[test]
fn attribute_access_on_proxy_yields_proxy_call() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(5), &mut NoPrint).unwrap();

    let progress = session.execute_interactive("p.data", &mut NoPrint).unwrap();
    let (proxy_id, method, args, kwargs, _call_id) = expect_proxy_call(progress);
    assert_eq!(proxy_id, 5);
    assert_eq!(method, "data");
    assert!(args.is_empty());
    assert!(kwargs.is_empty());
}

#[test]
fn method_call_on_proxy_yields_proxy_call_with_args() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(9), &mut NoPrint).unwrap();

    let progress = session.execute_interactive("p.fetch(1, a=2)", &mut NoPrint).unwrap();
    let (proxy_id, method, args, kwargs, _call_id) = expect_proxy_call(progress);
    assert_eq!(proxy_id, 9);
    assert_eq!(method, "fetch");
    assert_eq!(args, vec![Object::Int(1)]);
    assert_eq!(kwargs, vec![(Object::String("a".to_owned()), Object::Int(2))]);
}

#[test]
fn proxy_passed_as_argument_to_external_function() {
    let mut session = ReplSession::new(vec!["source".to_owned(), "sink".to_owned()], "<stdin>");

    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(3), &mut NoPrint).unwrap();

    let progress = session.execute_interactive("sink(p)", &mut NoPrint).unwrap();
    let (function_name, args, _kwargs, _call_id) = expect_function_call(progress);
    assert_eq!(function_name, "sink");
    assert_eq!(args, vec![Object::Proxy(3)]);
}

#[test]
fn proxy_persists_across_repl_lines() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(6), &mut NoPrint).unwrap();

    session.execute("q = p", &mut NoPrint).unwrap();
    assert_eq!(session.get_variable("q"), Some(Object::Proxy(6)));
}

#[test]
fn multiple_proxies_keep_distinct_ids() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");

    let progress = session.execute_interactive("a = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(1), &mut NoPrint).unwrap();

    let progress = session.execute_interactive("b = source(2)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(2), &mut NoPrint).unwrap();

    assert_eq!(session.get_variable("a"), Some(Object::Proxy(1)));
    assert_eq!(session.get_variable("b"), Some(Object::Proxy(2)));
}

#[test]
fn proxy_arithmetic_and_comparison_raise_type_error() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(4), &mut NoPrint).unwrap();

    let arithmetic_error = session.execute("p + 1", &mut NoPrint).unwrap_err();
    match arithmetic_error {
        ReplError::Runtime(exc) => assert_eq!(exc.exc_type(), ExcType::TypeError),
        other => panic!("expected runtime TypeError for arithmetic, got {other:?}"),
    }

    let comparison_error = session.execute("p < 1", &mut NoPrint).unwrap_err();
    match comparison_error {
        ReplError::Runtime(exc) => assert_eq!(exc.exc_type(), ExcType::TypeError),
        other => panic!("expected runtime TypeError for comparison, got {other:?}"),
    }
}

#[test]
fn proxy_call_resume_returns_value() {
    let mut session = ReplSession::new(vec!["source".to_owned()], "<stdin>");
    let progress = session.execute_interactive("p = source(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);
    session.resume(Object::Proxy(8), &mut NoPrint).unwrap();

    let progress = session.execute_interactive("p.value", &mut NoPrint).unwrap();
    let (_proxy_id, _method, _args, _kwargs, _call_id) = expect_proxy_call(progress);

    let result = session.resume(Object::Int(99), &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(99));
}
