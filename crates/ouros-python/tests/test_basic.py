from inline_snapshot import snapshot

import ouros


def test_simple_expression():
    m = ouros.Sandbox('1 + 2')
    assert m.run() == snapshot(3)


def test_arithmetic():
    m = ouros.Sandbox('10 * 5 - 3')
    assert m.run() == snapshot(47)


def test_string_concatenation():
    m = ouros.Sandbox('"hello" + " " + "world"')
    assert m.run() == snapshot('hello world')


def test_multiple_runs_same_instance():
    m = ouros.Sandbox('x * 2', inputs=['x'])
    assert m.run(inputs={'x': 5}) == snapshot(10)
    assert m.run(inputs={'x': 10}) == snapshot(20)
    assert m.run(inputs={'x': -3}) == snapshot(-6)


def test_repr_no_inputs():
    m = ouros.Sandbox('1 + 1')
    assert repr(m) == snapshot("Sandbox(<1 line of code>, script_name='main.py')")


def test_repr_with_inputs():
    m = ouros.Sandbox('x', inputs=['x', 'y'])
    assert repr(m) == snapshot('Sandbox(<1 line of code>, script_name=\'main.py\', inputs=["x", "y"])')


def test_repr_with_external_functions():
    m = ouros.Sandbox('foo()', external_functions=['foo'])
    assert repr(m) == snapshot('Sandbox(<1 line of code>, script_name=\'main.py\', external_functions=["foo"])')


def test_repr_with_inputs_and_external_functions():
    m = ouros.Sandbox('foo(x)', inputs=['x'], external_functions=['foo'])
    assert repr(m) == snapshot(
        'Sandbox(<1 line of code>, script_name=\'main.py\', inputs=["x"], external_functions=["foo"])'
    )


def test_multiline_code():
    code = """
x = 1
y = 2
x + y
"""
    m = ouros.Sandbox(code)
    assert m.run() == snapshot(3)


def test_function_definition_and_call():
    code = """
def add(a, b):
    return a + b

add(3, 4)
"""
    m = ouros.Sandbox(code)
    assert m.run() == snapshot(7)
