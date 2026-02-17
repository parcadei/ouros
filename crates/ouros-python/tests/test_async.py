import asyncio

import pytest
from dirty_equals import IsList
from inline_snapshot import snapshot

import ouros
from ouros import run_async


def test_async():
    code = 'await foobar(1, 2)'
    m = ouros.Sandbox(code, external_functions=['foobar'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)
    assert progress.function_name == snapshot('foobar')
    assert progress.args == snapshot((1, 2))
    call_id = progress.call_id
    progress = progress.resume(future=...)
    assert isinstance(progress, ouros.FutureSnapshot)
    assert progress.pending_call_ids == snapshot([call_id])
    progress = progress.resume({call_id: {'return_value': 3}})
    assert isinstance(progress, ouros.Complete)
    assert progress.output == snapshot(3)


def test_asyncio_gather():
    code = """
import asyncio

await asyncio.gather(foo(1), bar(2))
"""
    m = ouros.Sandbox(code, external_functions=['foo', 'bar'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)
    assert progress.function_name == snapshot('foo')
    assert progress.args == snapshot((1,))
    foo_call_ids = progress.call_id

    progress = progress.resume(future=...)
    assert isinstance(progress, ouros.Snapshot)
    assert progress.function_name == snapshot('bar')
    assert progress.args == snapshot((2,))
    bar_call_ids = progress.call_id
    progress = progress.resume(future=...)

    assert isinstance(progress, ouros.FutureSnapshot)
    dump_progress = progress.dump()

    assert progress.pending_call_ids == IsList(foo_call_ids, bar_call_ids, check_order=False)
    progress = progress.resume({foo_call_ids: {'return_value': 3}, bar_call_ids: {'return_value': 4}})
    assert isinstance(progress, ouros.Complete)
    assert progress.output == snapshot([3, 4])

    progress2 = ouros.FutureSnapshot.load(dump_progress)
    assert progress2.pending_call_ids == IsList(foo_call_ids, bar_call_ids, check_order=False)
    progress = progress2.resume({bar_call_ids: {'return_value': 14}, foo_call_ids: {'return_value': 13}})
    assert isinstance(progress, ouros.Complete)
    assert progress.output == snapshot([13, 14])

    progress3 = ouros.FutureSnapshot.load(dump_progress)
    progress = progress3.resume({bar_call_ids: {'return_value': 14}, foo_call_ids: {'future': ...}})
    assert isinstance(progress, ouros.FutureSnapshot)

    assert progress.pending_call_ids == [foo_call_ids]
    progress = progress.resume({foo_call_ids: {'return_value': 144}})
    assert isinstance(progress, ouros.Complete)
    assert progress.output == snapshot([144, 14])


# === Tests for run_async ===


async def test_run_async_sync_function():
    """Test run_async with a basic sync external function."""
    m = ouros.Sandbox('get_value()', external_functions=['get_value'])

    def get_value():
        return 42

    result = await run_async(m, external_functions={'get_value': get_value})
    assert result == snapshot(42)


async def test_run_async_async_function():
    """Test run_async with a basic async external function."""
    m = ouros.Sandbox('await fetch_data()', external_functions=['fetch_data'])

    async def fetch_data():
        await asyncio.sleep(0.001)
        return 'async result'

    result = await run_async(m, external_functions={'fetch_data': fetch_data})
    assert result == snapshot('async result')


async def test_run_async_function_not_found():
    """Test that missing external function raises wrapped error."""
    m = ouros.Sandbox('missing_func()', external_functions=['missing_func'])

    with pytest.raises(ouros.SandboxRuntimeError) as exc_info:
        await run_async(m, external_functions={})
    inner = exc_info.value.exception()
    assert isinstance(inner, KeyError)
    assert inner.args[0] == snapshot("'Function missing_func not found'")


async def test_run_async_sync_exception():
    """Test that sync function exceptions propagate correctly."""
    m = ouros.Sandbox('fail()', external_functions=['fail'])

    def fail():
        raise ValueError('sync error')

    with pytest.raises(ouros.SandboxRuntimeError) as exc_info:
        await run_async(m, external_functions={'fail': fail})
    inner = exc_info.value.exception()
    assert isinstance(inner, ValueError)
    assert inner.args[0] == snapshot('sync error')


async def test_run_async_async_exception():
    """Test that async function exceptions propagate correctly."""
    m = ouros.Sandbox('await async_fail()', external_functions=['async_fail'])

    async def async_fail():
        await asyncio.sleep(0.001)
        raise RuntimeError('async error')

    with pytest.raises(ouros.SandboxRuntimeError) as exc_info:
        await run_async(m, external_functions={'async_fail': async_fail})
    inner = exc_info.value.exception()
    assert isinstance(inner, RuntimeError)
    assert inner.args[0] == snapshot('async error')


async def test_run_async_exception_caught():
    """Test that exceptions caught in try/except don't propagate."""
    code = """
try:
    fail()
except ValueError:
    caught = True
caught
"""
    m = ouros.Sandbox(code, external_functions=['fail'])

    def fail():
        raise ValueError('caught error')

    result = await run_async(m, external_functions={'fail': fail})
    assert result == snapshot(True)


async def test_run_async_async_exception_caught():
    """Test that async external exceptions are catchable inside sandbox try/except."""
    code = """
try:
    result = await fetch('url')
except ValueError as e:
    result = str(e)
result
"""
    m = ouros.Sandbox(code, external_functions=['fetch'])

    async def fetch(url: str) -> str:
        raise ValueError('network error')

    result = await run_async(m, external_functions={'fetch': fetch})
    assert result == snapshot('network error')


async def test_run_async_multiple_async_functions():
    """Test asyncio.gather with multiple async functions."""
    code = """
import asyncio
await asyncio.gather(fetch_a(), fetch_b())
"""
    m = ouros.Sandbox(code, external_functions=['fetch_a', 'fetch_b'])

    async def fetch_a():
        await asyncio.sleep(0.01)
        return 'a'

    async def fetch_b():
        await asyncio.sleep(0.005)
        return 'b'

    result = await run_async(m, external_functions={'fetch_a': fetch_a, 'fetch_b': fetch_b})
    assert result == snapshot(['a', 'b'])


async def test_run_async_mixed_sync_async():
    """Test mix of sync and async external functions."""
    code = """
sync_val = sync_func()
async_val = await async_func()
sync_val + async_val
"""
    m = ouros.Sandbox(code, external_functions=['sync_func', 'async_func'])

    def sync_func():
        return 10

    async def async_func():
        await asyncio.sleep(0.001)
        return 5

    result = await run_async(m, external_functions={'sync_func': sync_func, 'async_func': async_func})
    assert result == snapshot(15)


async def test_run_async_with_inputs():
    """Test run_async with inputs parameter."""
    m = ouros.Sandbox('process(x, y)', inputs=['x', 'y'], external_functions=['process'])

    def process(a: int, b: int) -> int:
        return a * b

    result = await run_async(m, inputs={'x': 6, 'y': 7}, external_functions={'process': process})
    assert result == snapshot(42)


async def test_run_async_with_print_callback():
    """Test run_async with print_callback parameter."""
    output: list[tuple[str, str]] = []

    def callback(stream: str, text: str) -> None:
        output.append((stream, text))

    m = ouros.Sandbox('print("hello from async")')
    result = await run_async(m, print_callback=callback)
    assert result is None
    assert output == snapshot([('stdout', "hello from async\n")])


async def test_run_async_function_returning_none():
    """Test async function that returns None."""
    m = ouros.Sandbox('do_nothing()', external_functions=['do_nothing'])

    def do_nothing():
        return None

    result = await run_async(m, external_functions={'do_nothing': do_nothing})
    assert result is None


async def test_run_async_no_external_calls():
    """Test run_async when code has no external calls."""
    m = ouros.Sandbox('1 + 2 + 3')
    result = await run_async(m)
    assert result == snapshot(6)


# === Tests for run_async with os parameter ===


async def test_run_async_with_os():
    """run_async can use OSAccess for file operations."""
    from ouros import MemoryFile, OSAccess

    fs = OSAccess([MemoryFile('/test.txt', content='hello world')])

    m = ouros.Sandbox(
        """
from pathlib import Path
Path('/test.txt').read_text()
        """,
        external_functions=[],
    )

    result = await run_async(m, os=fs)
    assert result == snapshot('hello world')


async def test_run_async_os_with_external_functions():
    """run_async can combine OSAccess with external functions."""
    from ouros import MemoryFile, OSAccess

    fs = OSAccess([MemoryFile('/data.txt', content='test data')])

    async def process(text: str) -> str:
        return text.upper()

    m = ouros.Sandbox(
        """
from pathlib import Path
content = Path('/data.txt').read_text()
await process(content)
        """,
        external_functions=['process'],
    )

    result = await run_async(
        m,
        external_functions={'process': process},
        os=fs,
    )
    assert result == snapshot('TEST DATA')


async def test_run_async_os_file_not_found():
    """run_async propagates OS errors correctly."""
    from ouros import OSAccess

    fs = OSAccess()

    m = ouros.Sandbox(
        """
from pathlib import Path
Path('/missing.txt').read_text()
        """,
    )

    with pytest.raises(ouros.SandboxRuntimeError) as exc_info:
        await run_async(m, os=fs)
    assert str(exc_info.value) == snapshot("FileNotFoundError: [Errno 2] No such file or directory: '/missing.txt'")


async def test_run_async_os_not_provided():
    """run_async raises error when OS function called without os handler."""
    m = ouros.Sandbox(
        """
from pathlib import Path
Path('/test.txt').exists()
        """,
    )

    with pytest.raises(ouros.SandboxRuntimeError) as exc_info:
        await run_async(m)
    inner = exc_info.value.exception()
    assert isinstance(inner, RuntimeError)
    assert 'OS function' in inner.args[0]
    assert 'no os handler provided' in inner.args[0]


async def test_run_async_os_write_and_read():
    """run_async supports both reading and writing files."""
    from ouros import MemoryFile, OSAccess

    fs = OSAccess([MemoryFile('/file.txt', content='original')])

    m = ouros.Sandbox(
        """
from pathlib import Path
p = Path('/file.txt')
p.write_text('updated')
p.read_text()
        """,
    )

    result = await run_async(m, os=fs)
    assert result == snapshot('updated')
