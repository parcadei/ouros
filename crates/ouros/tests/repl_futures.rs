//! Tests for `ReplSession::resume_futures` -- parallel external call support.
//!
//! These tests exercise the async external-call pattern at the `ReplSession`
//! level: execute code that calls external functions, resume individual calls
//! as pending futures, then resolve them all via `resume_futures`.

use ouros::{ExcType, Exception, ExternalResult, NoPrint, Object, PendingFutureInfo, ReplProgress, ReplSession};

/// Helper: unwraps a `FunctionCall` progress, panicking on any other variant.
fn expect_function_call(progress: ReplProgress) -> (String, Vec<Object>, u32) {
    match progress {
        ReplProgress::FunctionCall {
            function_name,
            args,
            call_id,
            ..
        } => (function_name, args, call_id),
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

/// Helper: unwraps a `Complete` progress, panicking on any other variant.
fn expect_complete(progress: ReplProgress) -> Object {
    match progress {
        ReplProgress::Complete(value) => value,
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// Helper: unwraps a `ResolveFutures` progress, panicking on any other variant.
///
/// Returns both `pending_call_ids` and `pending_futures` for backward compat and
/// new-style assertions respectively.
fn expect_resolve_futures(progress: ReplProgress) -> (Vec<u32>, Vec<PendingFutureInfo>) {
    match progress {
        ReplProgress::ResolveFutures {
            pending_call_ids,
            pending_futures,
        } => (pending_call_ids, pending_futures),
        other => panic!("expected ResolveFutures, got {other:?}"),
    }
}

// ============================================================================
// resume with ExternalResult::Future
// ============================================================================

#[test]
fn resume_as_future_stores_external_future() {
    let mut session = ReplSession::new(vec!["fetch".to_string()], "<test>");
    let progress = session.execute_interactive("x = fetch('url')", &mut NoPrint).unwrap();
    let (_name, _args, _call_id) = expect_function_call(progress);

    // Resume as future -- x is assigned an ExternalFuture, execution completes.
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    assert!(
        matches!(progress, ReplProgress::Complete(_)),
        "expected Complete after resume-as-future for simple assignment, got {progress:?}"
    );
}

#[test]
fn two_sequential_futures_then_await_triggers_resolve_futures() {
    // This test uses the `# run-async` pattern: define an async function
    // that gathers two external calls.
    let mut session = ReplSession::new(vec!["async_call".to_string()], "<test>");

    // Execute async gather over two external calls.
    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (name, args, call_id_0) = expect_function_call(progress);
    assert_eq!(name, "async_call");
    assert_eq!(args, vec![Object::Int(1)]);

    // Resume first call as future.
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (name, args, call_id_1) = expect_function_call(progress);
    assert_eq!(name, "async_call");
    assert_eq!(args, vec![Object::Int(2)]);

    // Resume second call as future.
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (pending_ids, _pending_futures) = expect_resolve_futures(progress);
    assert_eq!(pending_ids.len(), 2, "should have 2 pending futures");
    assert!(pending_ids.contains(&call_id_0));
    assert!(pending_ids.contains(&call_id_1));

    // Resolve both futures.
    let results = vec![
        (call_id_0, ExternalResult::Return(Object::Int(10))),
        (call_id_1, ExternalResult::Return(Object::Int(20))),
    ];
    let progress = session.resume_futures(results, &mut NoPrint).unwrap();
    let result = expect_complete(progress);
    assert_eq!(result, Object::None, "top-level assignment returns None");

    // Verify the gathered results.
    let check = session.execute_interactive("result", &mut NoPrint).unwrap();
    let obj = expect_complete(check);
    // result should be [10, 20]
    match obj {
        Object::List(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Object::Int(10));
            assert_eq!(items[1], Object::Int(20));
        }
        other => panic!("expected list, got {other:?}"),
    }
}

#[test]
fn resume_futures_with_invalid_call_id_returns_error() {
    let mut session = ReplSession::new(vec!["async_call".to_string()], "<test>");

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (_name, _args, call_id_0) = expect_function_call(progress);
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (_name, _args, _call_id_1) = expect_function_call(progress);
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (_pending_ids, _pending_futures) = expect_resolve_futures(progress);

    // Try resolving with a bogus call_id.
    let results = vec![
        (call_id_0, ExternalResult::Return(Object::Int(10))),
        (999, ExternalResult::Return(Object::Int(20))),
    ];
    let err = session.resume_futures(results, &mut NoPrint);
    assert!(err.is_err(), "resume_futures with invalid call_id should error");

    // Session should recover: the error consumed pending state, so new code works.
    // Actually the error may have consumed the snapshot. Let's just verify it's an error.
    let err_msg = format!("{}", err.unwrap_err());
    assert!(
        err_msg.contains("999"),
        "error message should mention the invalid call_id 999, got: {err_msg}"
    );
}

#[test]
fn resume_futures_incremental_resolution() {
    // Resolve futures one at a time -- the first resume_futures should return
    // ResolveFutures again with the remaining pending call IDs.
    let mut session = ReplSession::new(vec!["async_call".to_string()], "<test>");

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (_name, _args, call_id_0) = expect_function_call(progress);
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (_name, _args, call_id_1) = expect_function_call(progress);
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (pending_ids, _pending_futures) = expect_resolve_futures(progress);
    assert_eq!(pending_ids.len(), 2);

    // Resolve only the first future.
    let results = vec![(call_id_0, ExternalResult::Return(Object::Int(10)))];
    let progress = session.resume_futures(results, &mut NoPrint).unwrap();
    // Should still be blocked waiting for call_id_1.
    let (remaining, _remaining_futures) = expect_resolve_futures(progress);
    assert_eq!(remaining, vec![call_id_1], "should have 1 remaining pending future");

    // Now resolve the second.
    let results = vec![(call_id_1, ExternalResult::Return(Object::Int(20)))];
    let progress = session.resume_futures(results, &mut NoPrint).unwrap();
    let _result = expect_complete(progress);

    // Verify gathered results.
    let check = session.execute_interactive("result", &mut NoPrint).unwrap();
    let obj = expect_complete(check);
    match obj {
        Object::List(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Object::Int(10));
            assert_eq!(items[1], Object::Int(20));
        }
        other => panic!("expected list, got {other:?}"),
    }
}

// ============================================================================
// fail_future exception handling (catchable by Python try/except)
// ============================================================================

#[test]
fn fail_future_exception_caught_by_try_except() {
    // When an external future fails with an error, the exception should be
    // catchable by Python's try/except block, allowing normal completion.
    let mut session = ReplSession::new(vec!["risky_call".to_string()], "<test>");

    let code = r"
import asyncio

async def run():
    try:
        r = await risky_call('bad input')
        return f'got: {r}'
    except Exception as e:
        return f'caught: {e}'

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (name, _args, call_id) = expect_function_call(progress);
    assert_eq!(name, "risky_call");

    // Resume as future so we can fail it via resume_futures.
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (pending_ids, _pending_futures) = expect_resolve_futures(progress);
    assert_eq!(pending_ids, vec![call_id]);

    // Fail the future with an error -- this should be caught by the try/except.
    let error_exc = Exception::new(ExcType::RuntimeError, Some("connection timeout".to_string()));
    let results = vec![(call_id, ExternalResult::Error(error_exc))];
    let progress = session
        .resume_futures(results, &mut NoPrint)
        .expect("resume_futures should succeed when exception is caught by try/except");
    let _result = expect_complete(progress);

    // Verify the except branch ran and produced the caught message.
    let check = session.execute_interactive("result", &mut NoPrint).unwrap();
    let obj = expect_complete(check);
    assert_eq!(
        obj,
        Object::String("caught: connection timeout".to_string()),
        "try/except should have caught the exception from fail_future"
    );
}

#[test]
fn fail_future_exception_propagates_without_try_except() {
    // When an external future fails and there is NO try/except, the exception
    // should propagate as a ReplError::Runtime.
    let mut session = ReplSession::new(vec!["risky_call".to_string()], "<test>");

    let code = r"
import asyncio

async def run():
    r = await risky_call('bad input')
    return f'got: {r}'

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (name, _args, call_id) = expect_function_call(progress);
    assert_eq!(name, "risky_call");

    // Resume as future.
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (pending_ids, _pending_futures) = expect_resolve_futures(progress);
    assert_eq!(pending_ids, vec![call_id]);

    // Fail the future with an error -- should propagate since there's no try/except.
    let error_exc = Exception::new(ExcType::RuntimeError, Some("connection timeout".to_string()));
    let results = vec![(call_id, ExternalResult::Error(error_exc))];
    let err = session.resume_futures(results, &mut NoPrint);
    assert!(
        err.is_err(),
        "resume_futures should return an error when exception is not caught"
    );
    let err_msg = format!("{}", err.unwrap_err());
    assert!(
        err_msg.contains("connection timeout"),
        "error should contain the exception message, got: {err_msg}"
    );
}

// ============================================================================
// PendingFutureInfo metadata in ResolveFutures
// ============================================================================

#[test]
fn resolve_futures_includes_function_name_and_args() {
    // When ResolveFutures is returned, each pending future should carry
    // the original function name and positional arguments so the host
    // can correlate call_ids without maintaining its own mapping.
    let mut session = ReplSession::new(vec!["fetch".to_string(), "compute".to_string()], "<test>");

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(fetch('http://example.com'), compute(42))

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (name0, args0, call_id_0) = expect_function_call(progress);
    assert_eq!(name0, "fetch");
    assert_eq!(args0, vec![Object::String("http://example.com".to_string())]);

    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (name1, args1, call_id_1) = expect_function_call(progress);
    assert_eq!(name1, "compute");
    assert_eq!(args1, vec![Object::Int(42)]);

    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (pending_ids, pending_futures) = expect_resolve_futures(progress);

    // Backward compat: pending_call_ids still present.
    assert_eq!(pending_ids.len(), 2);
    assert!(pending_ids.contains(&call_id_0));
    assert!(pending_ids.contains(&call_id_1));

    // New: pending_futures carry metadata.
    assert_eq!(pending_futures.len(), 2, "should have 2 pending future infos");

    // Find the info for each call_id (order not guaranteed).
    let info_0 = pending_futures
        .iter()
        .find(|f| f.call_id == call_id_0)
        .expect("missing info for call_id_0");
    assert_eq!(info_0.function_name, "fetch");
    assert_eq!(info_0.args, vec![Object::String("http://example.com".to_string())]);

    let info_1 = pending_futures
        .iter()
        .find(|f| f.call_id == call_id_1)
        .expect("missing info for call_id_1");
    assert_eq!(info_1.function_name, "compute");
    assert_eq!(info_1.args, vec![Object::Int(42)]);
}

#[test]
fn resolve_futures_metadata_survives_incremental_resolution() {
    // After resolving one future, the remaining ResolveFutures should still
    // carry correct metadata for the unresolved futures.
    let mut session = ReplSession::new(vec!["async_call".to_string()], "<test>");

    let code = r"
import asyncio

async def run():
    return await asyncio.gather(async_call(1), async_call(2))

result = await run()
";

    let progress = session.execute_interactive(code, &mut NoPrint).unwrap();
    let (_name, _args, call_id_0) = expect_function_call(progress);
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (_name, _args, call_id_1) = expect_function_call(progress);
    let progress = session.resume(ExternalResult::Future, &mut NoPrint).unwrap();
    let (_pending_ids, pending_futures) = expect_resolve_futures(progress);
    assert_eq!(pending_futures.len(), 2);

    // Resolve only the first future.
    let results = vec![(call_id_0, ExternalResult::Return(Object::Int(10)))];
    let progress = session.resume_futures(results, &mut NoPrint).unwrap();
    let (remaining_ids, remaining_futures) = expect_resolve_futures(progress);

    // Only call_id_1 should remain.
    assert_eq!(remaining_ids, vec![call_id_1]);
    assert_eq!(remaining_futures.len(), 1);
    assert_eq!(remaining_futures[0].call_id, call_id_1);
    assert_eq!(remaining_futures[0].function_name, "async_call");
    assert_eq!(remaining_futures[0].args, vec![Object::Int(2)]);
}
