from dataclasses import dataclass
from typing import Any

import pytest
from inline_snapshot import snapshot

import ouros


def test_ouros_dump_load_roundtrip():
    m = ouros.Sandbox('x + 1', inputs=['x'])
    data = m.dump()

    assert isinstance(data, bytes)
    assert len(data) > 0

    m2 = ouros.Sandbox.load(data)
    assert m2.run(inputs={'x': 41}) == snapshot(42)


def test_ouros_dump_load_preserves_script_name():
    m = ouros.Sandbox('1', script_name='custom.py')
    data = m.dump()

    m2 = ouros.Sandbox.load(data)
    assert repr(m2) == snapshot("Sandbox(<1 line of code>, script_name='custom.py')")


def test_ouros_dump_load_preserves_inputs():
    m = ouros.Sandbox('x + y', inputs=['x', 'y'])
    data = m.dump()

    m2 = ouros.Sandbox.load(data)
    assert m2.run(inputs={'x': 1, 'y': 2}) == snapshot(3)


def test_ouros_dump_load_preserves_external_functions():
    m = ouros.Sandbox('func()', external_functions=['func'])
    data = m.dump()

    m2 = ouros.Sandbox.load(data)
    result = m2.run(external_functions={'func': lambda: 42})
    assert result == snapshot(42)


def test_ouros_load_invalid_data():
    with pytest.raises(ValueError) as exc_info:
        ouros.Sandbox.load(b'invalid data')
    assert str(exc_info.value) == snapshot('Hit the end of buffer, expected more data')


def test_progress_dump_load_roundtrip():
    m = ouros.Sandbox('func(1, 2)', external_functions=['func'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)

    data = progress.dump()
    assert isinstance(data, bytes)
    assert len(data) > 0

    progress2 = ouros.Snapshot.load(data)
    assert progress2.function_name == snapshot('func')
    assert progress2.args == snapshot((1, 2))
    assert progress2.kwargs == snapshot({})

    result = progress2.resume(return_value=100)
    assert isinstance(result, ouros.Complete)
    assert result.output == snapshot(100)


def test_progress_dump_load_preserves_script_name():
    m = ouros.Sandbox('func()', script_name='test.py', external_functions=['func'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)

    data = progress.dump()
    progress2 = ouros.Snapshot.load(data)
    assert progress2.script_name == snapshot('test.py')


def test_progress_dump_load_with_kwargs():
    m = ouros.Sandbox('func(a=1, b="hello")', external_functions=['func'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)

    data = progress.dump()
    progress2 = ouros.Snapshot.load(data)
    assert progress2.function_name == snapshot('func')
    assert progress2.args == snapshot(())
    assert progress2.kwargs == snapshot({'a': 1, 'b': 'hello'})


def test_progress_dump_after_resume_fails():
    m = ouros.Sandbox('func()', external_functions=['func'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)

    progress.resume(return_value=1)

    with pytest.raises(RuntimeError) as exc_info:
        progress.dump()
    assert exc_info.value.args[0] == snapshot('Cannot dump progress that has already been resumed')


def test_progress_load_invalid_data():
    with pytest.raises(ValueError):
        ouros.Snapshot.load(b'invalid data')


def test_progress_dump_load_multiple_calls():
    m = ouros.Sandbox('a() + b()', external_functions=['a', 'b'])

    # First call
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)
    assert progress.function_name == snapshot('a')

    # Dump and load the state
    data = progress.dump()
    progress2 = ouros.Snapshot.load(data)

    # Resume with first return value
    progress3 = progress2.resume(return_value=10)
    assert isinstance(progress3, ouros.Snapshot)
    assert progress3.function_name == snapshot('b')

    # Dump and load again
    data2 = progress3.dump()
    progress4 = ouros.Snapshot.load(data2)

    # Resume with second return value
    result = progress4.resume(return_value=5)
    assert isinstance(result, ouros.Complete)
    assert result.output == snapshot(15)


def test_progress_load_with_print_callback():
    output: list[tuple[str, str]] = []

    def callback(stream: str, text: str) -> None:
        output.append((stream, text))

    m = ouros.Sandbox('print("before"); func(); print("after")', external_functions=['func'])
    progress = m.start(print_callback=callback)
    assert isinstance(progress, ouros.Snapshot)
    assert output == snapshot([('stdout', "before\n")])

    # Dump and load with new callback
    data = progress.dump()
    output.clear()
    progress2 = ouros.Snapshot.load(data, print_callback=callback)

    result = progress2.resume(return_value=None)
    assert isinstance(result, ouros.Complete)
    assert output == snapshot([('stdout', "after\n")])


def test_progress_load_without_print_callback():
    m = ouros.Sandbox('func()', external_functions=['func'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)

    data = progress.dump()
    progress2 = ouros.Snapshot.load(data)

    result = progress2.resume(return_value=42)
    assert isinstance(result, ouros.Complete)
    assert result.output == snapshot(42)


@pytest.mark.parametrize(
    'code,expected',
    [
        ('1 + 1', 2),
        ('"hello"', 'hello'),
        ('[1, 2, 3]', [1, 2, 3]),
        ('{"a": 1}', {'a': 1}),
        ('True', True),
        ('None', None),
    ],
)
def test_ouros_dump_load_various_outputs(code: str, expected: Any):
    m = ouros.Sandbox(code)
    data = m.dump()
    m2 = ouros.Sandbox.load(data)
    assert m2.run() == expected


def test_progress_dump_load_with_limits():
    m = ouros.Sandbox('func()', external_functions=['func'])
    limits = ouros.ResourceLimits(max_allocations=1000)
    progress = m.start(limits=limits)
    assert isinstance(progress, ouros.Snapshot)

    data = progress.dump()
    progress2 = ouros.Snapshot.load(data)

    result = progress2.resume(return_value=99)
    assert isinstance(result, ouros.Complete)
    assert result.output == snapshot(99)


@dataclass
class Person:
    name: str
    age: int


def test_ouros_load_dataclass():
    m = ouros.Sandbox('x', inputs=['x'])
    data = m.dump()

    m2 = ouros.Sandbox.load(data)
    m2.register_dataclass(Person)
    result = m2.run(inputs={'x': Person(name='Alice', age=30)})
    assert isinstance(result, Person)


def test_progress_dump_load_dataclass():
    m = ouros.Sandbox('func()', external_functions=['func'])
    progress = m.start()
    assert isinstance(progress, ouros.Snapshot)

    data = progress.dump()
    assert isinstance(data, bytes)
    assert len(data) > 0

    progress2 = ouros.Snapshot.load(data, dataclass_registry=[Person])
    assert progress2.function_name == snapshot('func')
    assert progress2.args == snapshot(())
    assert progress2.kwargs == snapshot({})

    result = progress2.resume(return_value=Person(name='Alice', age=30))
    assert isinstance(result, ouros.Complete)
    assert isinstance(result.output, Person)
    assert result.output.name == snapshot('Alice')
    assert result.output.age == snapshot(30)
