# === yield from list delegates to iterable ===
def gen_from_list():
    yield from [1, 2, 3]


assert list(gen_from_list()) == [1, 2, 3], 'yield from list'


# === yield from tuple ===
def gen_from_tuple():
    yield from (10, 20, 30)


assert list(gen_from_tuple()) == [10, 20, 30], 'yield from tuple'


# === yield from string ===
def gen_from_str():
    yield from 'abc'


assert list(gen_from_str()) == ['a', 'b', 'c'], 'yield from string'


# === yield from range ===
def gen_from_range():
    yield from range(4)


assert list(gen_from_range()) == [0, 1, 2, 3], 'yield from range'


# === yield from generator delegates to sub-generator ===
def inner_gen():
    yield 'a'
    yield 'b'


def outer_gen():
    yield from inner_gen()


assert list(outer_gen()) == ['a', 'b'], 'yield from generator'


# === yield from with items before and after ===
def gen_with_surrounding():
    yield 1
    yield from [2, 3]
    yield 4


assert list(gen_with_surrounding()) == [1, 2, 3, 4], 'yield from with surrounding yields'


# === yield from returns the sub-generator return value ===
def inner_return():
    yield 1
    yield 2
    return 'inner done'


def outer_capture():
    result = yield from inner_return()
    yield result


assert list(outer_capture()) == [1, 2, 'inner done'], 'yield from returns sub-generator return value'


# === yield from with empty iterable ===
def gen_from_empty():
    yield from []
    yield 'after'


assert list(gen_from_empty()) == ['after'], 'yield from empty iterable'


# === yield from empty generator ===
def empty_inner():
    return 'empty result'
    yield  # makes it a generator


def outer_empty():
    result = yield from empty_inner()
    yield result


assert list(outer_empty()) == ['empty result'], 'yield from empty generator captures return'


# === Multiple yield from in sequence ===
def gen_multi_from():
    yield from [1, 2]
    yield from [3, 4]
    yield from [5, 6]


assert list(gen_multi_from()) == [1, 2, 3, 4, 5, 6], 'multiple yield from in sequence'


# === yield from propagates send() to sub-generator ===
def inner_send():
    x = yield 'first'
    yield 'got: ' + str(x)


def outer_send():
    yield from inner_send()


g = outer_send()
assert next(g) == 'first', 'yield from start'
assert g.send(42) == 'got: 42', 'send propagated through yield from'


# === yield from propagates throw() to sub-generator ===
def inner_throw():
    try:
        yield 1
    except ValueError as e:
        yield 'caught: ' + str(e)


def outer_throw():
    yield from inner_throw()


g = outer_throw()
assert next(g) == 1, 'yield from advance'
assert g.throw(ValueError('test')) == 'caught: test', 'throw propagated through yield from'


# === throw() on yield from propagates to sub-generator, uncaught bubbles up ===
def inner_no_catch():
    yield 1
    yield 2


def outer_no_catch():
    yield from inner_no_catch()


g = outer_no_catch()
next(g)
propagated = False
try:
    g.throw(TypeError('bad'))
except TypeError as e:
    propagated = str(e) == 'bad'
assert propagated, 'uncaught throw through yield from propagates to caller'

# === close() propagates to sub-generator in yield from ===
inner_closed = False


def inner_closeable():
    global inner_closed
    try:
        yield 1
    except GeneratorExit:
        inner_closed = True


def outer_closeable():
    yield from inner_closeable()


g = outer_closeable()
next(g)
g.close()
assert inner_closed, 'close() propagated to sub-generator via yield from'


# === Nested yield from chains ===
def gen_a():
    yield 1
    return 'a'


def gen_b():
    result = yield from gen_a()
    yield result
    return 'b'


def gen_c():
    result = yield from gen_b()
    yield result


assert list(gen_c()) == [1, 'a', 'b'], 'nested yield from chains'


# === Deeply nested yield from ===
def level0():
    yield 'deep'
    return 'L0'


def level1():
    r = yield from level0()
    return r + '+L1'


def level2():
    r = yield from level1()
    return r + '+L2'


def level3():
    r = yield from level2()
    yield r


assert list(level3()) == ['deep', 'L0+L1+L2'], 'deeply nested yield from'


# === yield from with send through multiple levels ===
def bottom():
    x = yield 'bottom'
    yield 'bottom got: ' + str(x)
    return 'bottom done'


def middle():
    r = yield from bottom()
    yield 'middle got: ' + r


def top():
    yield from middle()


g = top()
assert next(g) == 'bottom', 'nested send: start'
assert g.send(99) == 'bottom got: 99', 'nested send: propagated to bottom'
assert next(g) == 'middle got: bottom done', 'nested send: middle received return'


# === yield from dict yields keys ===
def gen_from_dict():
    yield from {'x': 1, 'y': 2}


assert list(gen_from_dict()) == ['x', 'y'], 'yield from dict yields keys'


# === yield from set yields elements (order may vary) ===
def gen_from_set():
    yield from {42}


result = list(gen_from_set())
assert result == [42], 'yield from single-element set'
