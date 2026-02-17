use ouros::{ExcType, Exception, ExternalResult, NoPrint, Object, ReplError, ReplProgress, ReplSession};

fn expect_complete(progress: ReplProgress) -> Object {
    match progress {
        ReplProgress::Complete(value) => value,
        other => panic!("expected complete progress, got {other:?}"),
    }
}

fn expect_function_call(progress: ReplProgress) -> (String, Vec<Object>, Vec<(Object, Object)>, u32) {
    match progress {
        ReplProgress::FunctionCall {
            function_name,
            args,
            kwargs,
            call_id,
        } => (function_name, args, kwargs, call_id),
        other => panic!("expected function call progress, got {other:?}"),
    }
}

#[test]
fn execute_interactive_returns_complete_for_simple_expression() {
    let mut session = ReplSession::new(vec![], "<stdin>");
    let result = session.execute_interactive("1 + 2", &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(3));
}

#[test]
fn execute_interactive_returns_function_call_for_external_function() {
    let mut session = ReplSession::new(vec!["ext".to_owned()], "<stdin>");
    let progress = session.execute_interactive("ext(10)", &mut NoPrint).unwrap();
    let (function_name, args, kwargs, _call_id) = expect_function_call(progress);
    assert_eq!(function_name, "ext");
    assert_eq!(args, vec![Object::Int(10)]);
    assert!(kwargs.is_empty(), "expected no kwargs, got {kwargs:?}");
}

#[test]
fn resume_after_function_call_continues_execution() {
    let mut session = ReplSession::new(vec!["ext".to_owned()], "<stdin>");
    let progress = session.execute_interactive("ext(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);

    let result = session.resume(Object::Int(7), &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(7));
}

#[test]
fn external_function_result_is_used_in_expression() {
    let mut session = ReplSession::new(vec!["ext".to_owned()], "<stdin>");
    let progress = session.execute_interactive("x = ext(1) + 2\nx", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);

    let result = session.resume(Object::Int(5), &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(7));
}

#[test]
fn multiple_external_calls_in_sequence() {
    let mut session = ReplSession::new(vec!["ext".to_owned()], "<stdin>");
    let progress = session
        .execute_interactive("x = ext(1)\ny = ext(2)\nx + y", &mut NoPrint)
        .unwrap();
    let (_function_name, args, _kwargs, _call_id) = expect_function_call(progress);
    assert_eq!(args, vec![Object::Int(1)]);

    let progress = session.resume(Object::Int(10), &mut NoPrint).unwrap();
    let (_function_name, args, _kwargs, _call_id) = expect_function_call(progress);
    assert_eq!(args, vec![Object::Int(2)]);

    let result = session.resume(Object::Int(20), &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(30));
}

#[test]
fn external_call_in_function_definition_called_later() {
    let mut session = ReplSession::new(vec!["ext".to_owned()], "<stdin>");
    let definition = session
        .execute_interactive("def f(x):\n    return ext(x) + 1", &mut NoPrint)
        .unwrap();
    assert_eq!(expect_complete(definition), Object::None);

    let progress = session.execute_interactive("f(41)", &mut NoPrint).unwrap();
    let (_function_name, args, _kwargs, _call_id) = expect_function_call(progress);
    assert_eq!(args, vec![Object::Int(41)]);

    let result = session.resume(Object::Int(41), &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(42));
}

#[test]
fn error_recovery_after_failed_external_call() {
    let mut session = ReplSession::new(vec!["ext".to_owned()], "<stdin>");
    let progress = session.execute_interactive("ext(1)", &mut NoPrint).unwrap();
    let (_function_name, _args, _kwargs, _call_id) = expect_function_call(progress);

    let error = ExternalResult::Error(Exception::new(
        ExcType::RuntimeError,
        Some("external failure".to_owned()),
    ));
    let resume_error = session.resume(error, &mut NoPrint).unwrap_err();
    match resume_error {
        ReplError::Runtime(exc) => {
            assert_eq!(exc.exc_type(), ExcType::RuntimeError);
            assert_eq!(exc.message(), Some("external failure"));
        }
        other => panic!("expected runtime error, got {other:?}"),
    }

    let result = session.execute_interactive("1 + 1", &mut NoPrint).unwrap();
    assert_eq!(expect_complete(result), Object::Int(2));
}

#[test]
fn execute_interactive_without_external_functions_matches_execute() {
    let mut interactive_session = ReplSession::new(vec![], "<stdin>");
    let mut standard_session = ReplSession::new(vec![], "<stdin>");

    let interactive = interactive_session
        .execute_interactive("x = 20\nx + 22", &mut NoPrint)
        .unwrap();
    let standard = standard_session.execute("x = 20\nx + 22", &mut NoPrint).unwrap();

    assert_eq!(expect_complete(interactive), standard);
}
