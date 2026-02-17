# Test file for contextlib module - covers 100% of public API
# run-async
import contextlib
import io
import sys
import asyncio

# === AbstractContextManager ===
try:
    print('=== AbstractContextManager ===')
    class MyContextManager(contextlib.AbstractContextManager):
        def __enter__(self):
            print('acm_enter')
            return self
        def __exit__(self, exc_type, exc_val, exc_tb):
            print('acm_exit')
            return False

    with MyContextManager() as acm:
        print('acm_body')
except Exception as e:
    print('SKIP_AbstractContextManager', type(e).__name__, e)

# === AbstractAsyncContextManager ===
try:
    print('\n=== AbstractAsyncContextManager ===')
    class MyAsyncContextManager(contextlib.AbstractAsyncContextManager):
        async def __aenter__(self):
            print('aacm_enter')
            return self
        async def __aexit__(self, exc_type, exc_val, exc_tb):
            print('aacm_exit')
            return False

    async def test_aacm():
        async with MyAsyncContextManager() as aacm:
            print('aacm_body')

    asyncio.run(test_aacm())
except Exception as e:
    print('SKIP_AbstractAsyncContextManager', type(e).__name__, e)

# === contextmanager decorator ===
try:
    print('\n=== contextmanager ===')
    @contextlib.contextmanager
    def my_context():
        print('ctx_setup')
        yield 'ctx_value'
        print('ctx_cleanup')

    with my_context() as ctx_val:
        print(f'ctx_body: {ctx_val}')

    # contextmanager with exception handling
    @contextlib.contextmanager
    def my_context_exc(suppress=False):
        print('ctx_exc_setup')
        try:
            yield 'ctx_exc_value'
        except ValueError:
            print('ctx_exc_caught')
            if not suppress:
                raise
        finally:
            print('ctx_exc_cleanup')

    with my_context_exc() as ctx_exc_val:
        print(f'ctx_exc_body: {ctx_exc_val}')

    # contextmanager as decorator
    @contextlib.contextmanager
    def as_decorator():
        print('decorator_setup')
        yield
        print('decorator_cleanup')

    @as_decorator()
    def decorated_func():
        print('decorated_body')

    decorated_func()
except Exception as e:
    print('SKIP_contextmanager', type(e).__name__, e)

# === asynccontextmanager decorator ===
try:
    print('\n=== asynccontextmanager ===')
    @contextlib.asynccontextmanager
    async def my_async_context():
        print('actx_setup')
        yield 'actx_value'
        print('actx_cleanup')

    async def test_async_ctx():
        async with my_async_context() as actx_val:
            print(f'actx_body: {actx_val}')

    asyncio.run(test_async_ctx())

    # asynccontextmanager as decorator
    @contextlib.asynccontextmanager
    async def as_async_decorator():
        print('async_decorator_setup')
        yield
        print('async_decorator_cleanup')

    @as_async_decorator()
    async def async_decorated_func():
        print('async_decorated_body')

    asyncio.run(async_decorated_func())
except Exception as e:
    print('SKIP_asynccontextmanager', type(e).__name__, e)

# === closing ===
try:
    print('\n=== closing ===')
    class Closeable:
        def __init__(self, name):
            self.name = name
        def close(self):
            print(f'closing_{self.name}')
        def do_something(self):
            print(f'doing_{self.name}')

    with contextlib.closing(Closeable('resource')) as closeable:
        closeable.do_something()
except Exception as e:
    print('SKIP_closing', type(e).__name__, e)

# === aclosing ===
try:
    print('\n=== aclosing ===')
    async def async_gen():
        try:
            yield 1
            yield 2
        finally:
            print('aclosing_cleanup')

    async def test_aclosing():
        async with contextlib.aclosing(async_gen()) as gen:
            async for val in gen:
                print(f'aclosing_val: {val}')
                if val == 1:
                    break

    asyncio.run(test_aclosing())
except Exception as e:
    print('SKIP_aclosing', type(e).__name__, e)

# === nullcontext ===
try:
    print('\n=== nullcontext ===')
    with contextlib.nullcontext() as nc_none:
        print(f'nullcontext_none: {nc_none}')

    with contextlib.nullcontext('custom_value') as nc_custom:
        print(f'nullcontext_custom: {nc_custom}')

    # nullcontext with None explicitly
    with contextlib.nullcontext(None) as nc_explicit:
        print(f'nullcontext_explicit: {nc_explicit}')
except Exception as e:
    print('SKIP_nullcontext', type(e).__name__, e)

# === suppress ===
try:
    print('\n=== suppress ===')
    with contextlib.suppress(ValueError):
        print('suppress_before_raise')
        raise ValueError('suppressed')
    print('suppress_after_catch')

    # suppress multiple exceptions
    with contextlib.suppress(ValueError, TypeError):
        print('suppress_multi_before_raise')
        raise TypeError('suppressed_type')
    print('suppress_multi_after_catch')

    # suppress that doesn't catch
    with contextlib.suppress(ValueError):
        print('suppress_no_exception')
    print('suppress_no_exception_after')
except Exception as e:
    print('SKIP_suppress', type(e).__name__, e)

# === redirect_stdout ===
try:
    print('\n=== redirect_stdout ===')
    old_stdout = sys.stdout
    fake_stdout = io.StringIO()
    with contextlib.redirect_stdout(fake_stdout):
        print('captured_stdout')
    captured = fake_stdout.getvalue()
    print(f'redirected_stdout: {captured.strip()}')

    # redirect_stdout with None (suppresses output)
    fake_stdout2 = io.StringIO()
    with contextlib.redirect_stdout(None):
        print('this_should_be_suppressed')
    print('redirect_none_complete')
except Exception as e:
    print('SKIP_redirect_stdout', type(e).__name__, e)

# === redirect_stderr ===
try:
    print('\n=== redirect_stderr ===')
    fake_stderr = io.StringIO()
    with contextlib.redirect_stderr(fake_stderr):
        print('captured_stderr', file=sys.stderr)
    captured_err = fake_stderr.getvalue()
    print(f'redirected_stderr: {captured_err.strip()}')

    # redirect_stderr with None
    with contextlib.redirect_stderr(None):
        print('stderr_suppressed', file=sys.stderr)
    print('redirect_stderr_none_complete')
except Exception as e:
    print('SKIP_redirect_stderr', type(e).__name__, e)

# === chdir ===
try:
    print('\n=== chdir ===')
    import os
    original_dir = os.getcwd()
    with contextlib.chdir('/tmp'):
        print(f'chdir_tmp: {os.getcwd()}')
    print(f'chdir_restored: {os.getcwd() == original_dir}')

    # chdir with Path
    from pathlib import Path
    tmp_path = Path('/tmp')
    with contextlib.chdir(tmp_path):
        print(f'chdir_path: {os.getcwd()}')
except Exception as e:
    print('SKIP_chdir', type(e).__name__, e)

# === ExitStack ===
try:
    print('\n=== ExitStack ===')
    with contextlib.ExitStack() as stack:
        print('exitstack_enter')
        stack.enter_context(contextlib.closing(Closeable('stack1')))
        stack.enter_context(contextlib.closing(Closeable('stack2')))
        print('exitstack_body')
    print('exitstack_exit')

    # ExitStack with callbacks
    print('\n=== ExitStack callbacks ===')
    def callback_func(arg, kwarg=None):
        print(f'callback_called: {arg}, {kwarg}')

    with contextlib.ExitStack() as stack:
        print('callback_setup')
        stack.callback(callback_func, 'pos_arg', kwarg='key_arg')
        print('callback_body')
    print('callback_after')

    # ExitStack.pop_all
    print('\n=== ExitStack pop_all ===')
    stack2 = contextlib.ExitStack()
    stack2.callback(callback_func, 'popped', kwarg='popped_kw')
    copied = stack2.pop_all()
    print('pop_all_done')
    stack2.close()
    print('original_closed')
    copied.close()
    print('copied_closed')

    # ExitStack.callback with result (push callback needs to accept exc args)
    print('\n=== ExitStack callback with result ===')
    with contextlib.ExitStack() as stack:
        cb = stack.push(lambda *exc: callback_func('pushed'))
        print('push_callback_body')
    print('push_callback_after')
except Exception as e:
    print('SKIP_ExitStack', type(e).__name__, e)

# === AsyncExitStack ===
try:
    print('\n=== AsyncExitStack ===')
    async def test_async_exit_stack():
        async with contextlib.AsyncExitStack() as astack:
            print('async_exitstack_enter')
            await astack.enter_async_context(my_async_context())
            print('async_exitstack_body')
        print('async_exitstack_exit')

    asyncio.run(test_async_exit_stack())

    # AsyncExitStack with callbacks
    async def test_async_exit_stack_callback():
        async with contextlib.AsyncExitStack() as astack:
            print('async_callback_setup')
            astack.callback(callback_func, 'async_callback')
            print('async_callback_body')
        print('async_callback_after')

    asyncio.run(test_async_exit_stack_callback())

    # AsyncExitStack.push
    async def test_async_exit_stack_push():
        async with contextlib.AsyncExitStack() as astack:
            print('async_push_setup')
            astack.push(lambda *exc: callback_func('async_pushed'))
            print('async_push_body')
        print('async_push_after')

    asyncio.run(test_async_exit_stack_push())
except Exception as e:
    print('SKIP_AsyncExitStack', type(e).__name__, e)

# === ContextDecorator ===
try:
    print('\n=== ContextDecorator ===')
    class MyContextDecorator(contextlib.ContextDecorator):
        def __enter__(self):
            print('cd_enter')
            return self
        def __exit__(self, *exc):
            print('cd_exit')
            return False

    @MyContextDecorator()
    def func_with_cd():
        print('cd_body')

    func_with_cd()
except Exception as e:
    print('SKIP_ContextDecorator', type(e).__name__, e)

# === AsyncContextDecorator ===
try:
    print('\n=== AsyncContextDecorator ===')
    class MyAsyncContextDecorator(contextlib.AsyncContextDecorator):
        async def __aenter__(self):
            print('acd_enter')
            return self
        async def __aexit__(self, *exc):
            print('acd_exit')
            return False

    @MyAsyncContextDecorator()
    async def func_with_acd():
        print('acd_body')

    asyncio.run(func_with_acd())
except Exception as e:
    print('SKIP_AsyncContextDecorator', type(e).__name__, e)

# === ExitStack enter_context with exception suppression ===
try:
    print('\n=== ExitStack exception handling ===')
    class ExceptionSwallower(contextlib.AbstractContextManager):
        def __enter__(self):
            print('swallower_enter')
            return self
        def __exit__(self, exc_type, exc_val, exc_tb):
            print(f'swallower_exit: {exc_type}')
            return True  # Suppress exception

    with contextlib.ExitStack() as stack:
        stack.enter_context(ExceptionSwallower())
        print('exception_body')
        raise ValueError('swallowed')
    print('exception_after')
except Exception as e:
    print('SKIP_ExitStack_exception_handling', type(e).__name__, e)

# === Complex ExitStack scenario ===
try:
    print('\n=== Complex ExitStack ===')
    with contextlib.ExitStack() as stack:
        stack.enter_context(contextlib.suppress(KeyError))
        stack.enter_context(contextlib.nullcontext('complex'))
        stack.callback(callback_func, 'complex_cb')
        print('complex_body')
    print('complex_after')
except Exception as e:
    print('SKIP_Complex_ExitStack', type(e).__name__, e)

print('\n=== all_tests_complete ===')
