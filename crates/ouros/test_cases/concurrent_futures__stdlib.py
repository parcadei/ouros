import concurrent
import concurrent.futures
from concurrent import futures


assert concurrent.futures is not None, 'concurrent_package_has_futures'
assert futures is not None, 'from_concurrent_import_futures'


def square(x):
    return x * x


with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
    immediate = executor.submit(len, [1, 2, 3])
    assert immediate.done() is True, 'submit_builtin_done_true'
    assert immediate.cancelled() is False, 'submit_builtin_not_cancelled'
    assert immediate.result() == 3, 'submit_builtin_result'
    assert immediate.exception() is None, 'submit_builtin_exception_none'

    seen = []

    def on_done(fut):
        seen.append(fut.result())

    callback_ret = immediate.add_done_callback(on_done)
    assert callback_ret is None, 'add_done_callback_return_none'
    assert seen == [3], 'add_done_callback_called'

    deferred = executor.submit(square, 6)
    assert deferred.result() == 36, 'submit_def_result'

    mapped = list(executor.map(square, [1, 2, 3, 4]))
    assert mapped == [1, 4, 9, 16], 'executor_map_results'

    fs = [executor.submit(len, [1]), executor.submit(len, [1, 2])]
    done, not_done = concurrent.futures.wait(fs)
    assert len(done) == 2, 'wait_done_len'
    assert len(not_done) == 0, 'wait_not_done_len'

    completed = list(concurrent.futures.as_completed(fs))
    assert len(completed) == 2, 'as_completed_len'

assert concurrent.futures.ProcessPoolExecutor is not None, 'process_pool_alias_available'

manual = concurrent.futures.Future()
assert manual.done() is False, 'manual_future_initial_done'
assert manual.cancelled() is False, 'manual_future_initial_cancelled'
assert manual.cancel() is True, 'manual_future_cancel_true'
assert manual.done() is True, 'manual_future_done_after_cancel'
assert manual.cancelled() is True, 'manual_future_cancelled_after_cancel'
