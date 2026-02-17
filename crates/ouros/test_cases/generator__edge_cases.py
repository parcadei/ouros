# === Generator with try/finally: finally runs on close() ===
finally_ran = False


def gen_try_finally():
    global finally_ran
    try:
        yield 1
        yield 2
    finally:
        finally_ran = True


g = gen_try_finally()
next(g)
g.close()
assert finally_ran, 'finally block runs on generator close()'

# === Generator with try/finally: finally runs on exhaustion ===
finally_on_exhaust = False


def gen_finally_exhaust():
    global finally_on_exhaust
    try:
        yield 1
    finally:
        finally_on_exhaust = True


g = gen_finally_exhaust()
next(g)
try:
    next(g)
except StopIteration:
    pass
assert finally_on_exhaust, 'finally block runs on generator exhaustion'


# === Generator with try/except catches exceptions from throw() ===
def gen_try_except():
    try:
        yield 1
    except ValueError:
        yield 'caught ValueError'
    except TypeError:
        yield 'caught TypeError'


g = gen_try_except()
next(g)
assert g.throw(TypeError('oops')) == 'caught TypeError', 'try/except catches thrown TypeError'

# === Generator with try/except/finally: all run correctly ===
events = []


def gen_full_try():
    global events
    try:
        events.append('try')
        yield 1
    except ValueError:
        events.append('except')
        yield 2
    finally:
        events.append('finally')


g = gen_full_try()
next(g)
g.throw(ValueError('test'))
try:
    next(g)
except StopIteration:
    pass
assert events == ['try', 'except', 'finally'], 'try/except/finally all execute'


# === Generator exception during iteration marks generator as finished ===
def gen_raises():
    yield 1
    raise ValueError('mid-gen error')
    yield 2  # never reached


g = gen_raises()
assert next(g) == 1, 'yield before exception'
raised = False
try:
    next(g)
except ValueError as e:
    raised = str(e) == 'mid-gen error'
assert raised, 'generator raises exception during iteration'

# after exception, generator is finished
finished = False
try:
    next(g)
except StopIteration:
    finished = True
assert finished, 'generator is finished after unhandled exception'


# === Recursive generators ===
def gen_recursive(n):
    if n <= 0:
        return
    yield n
    yield from gen_recursive(n - 1)


assert list(gen_recursive(4)) == [4, 3, 2, 1], 'recursive generator'


# === Generator as class method ===
class Counter:
    def __init__(self, limit):
        self.limit = limit

    def count(self):
        i = 0
        while i < self.limit:
            yield i
            i = i + 1


c = Counter(3)
assert list(c.count()) == [0, 1, 2], 'generator as class method'


# === Generator with *args ===
def gen_varargs(*args):
    for a in args:
        yield a * 2


assert list(gen_varargs(1, 2, 3)) == [2, 4, 6], 'generator with *args'


# === Generator with **kwargs ===
def gen_kwargs(**kwargs):
    for k in kwargs:
        yield k + '=' + str(kwargs[k])


result = list(gen_kwargs(x=1, y=2))
assert result == ['x=1', 'y=2'], 'generator with **kwargs'


# === Generator with *args and **kwargs ===
def gen_mixed(*args, **kwargs):
    for a in args:
        yield ('arg', a)
    for k in kwargs:
        yield ('kwarg', k, kwargs[k])


result = list(gen_mixed(1, 2, a=10))
assert result == [('arg', 1), ('arg', 2), ('kwarg', 'a', 10)], 'generator with *args and **kwargs'


# === Generator with closures (captured variables) ===
def make_gen(multiplier):
    def gen():
        for i in range(3):
            yield i * multiplier

    return gen


gen3 = make_gen(3)
gen5 = make_gen(5)
assert list(gen3()) == [0, 3, 6], 'generator closure with multiplier 3'
assert list(gen5()) == [0, 5, 10], 'generator closure with multiplier 5'

# === Generator capturing loop variable (late binding) ===
generators = []
for i in range(3):

    def gen(n=i):
        yield n

    generators.append(gen)

assert list(generators[0]()) == [0], 'captured loop var gen 0'
assert list(generators[1]()) == [1], 'captured loop var gen 1'
assert list(generators[2]()) == [2], 'captured loop var gen 2'


# === StopIteration raised inside generator body ===
# PEP 479: StopIteration raised inside a generator is converted to RuntimeError
def gen_stop_inside():
    yield 1
    raise StopIteration('manual stop')


g = gen_stop_inside()
next(g)
pep479 = False
try:
    next(g)
except RuntimeError as e:
    pep479 = 'StopIteration' in str(e)
assert pep479, 'PEP 479: StopIteration inside generator becomes RuntimeError'


# === Generator with while True loop ===
def gen_infinite():
    i = 0
    while True:
        yield i
        i = i + 1


g = gen_infinite()
assert next(g) == 0, 'infinite generator first'
assert next(g) == 1, 'infinite generator second'
assert next(g) == 2, 'infinite generator third'
g.close()


# === Generator that yields other generators (not yield from) ===
def gen_of_gens():
    yield (x for x in range(2))
    yield (x for x in range(2, 4))


g = gen_of_gens()
first = next(g)
second = next(g)
assert list(first) == [0, 1], 'first inner generator'
assert list(second) == [2, 3], 'second inner generator'


# === Generator with complex local state ===
def fibonacci():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b


g = fibonacci()
fibs = []
for _ in range(8):
    fibs.append(next(g))
assert fibs == [0, 1, 1, 2, 3, 5, 8, 13], 'fibonacci generator'


# === Generator used in zip() ===
def gen_a():
    yield 'a'
    yield 'b'
    yield 'c'


def gen_nums():
    yield 1
    yield 2
    yield 3


result = list(zip(gen_a(), gen_nums()))
assert result == [('a', 1), ('b', 2), ('c', 3)], 'generators in zip()'


# === Generator used in enumerate() ===
def gen_items():
    yield 'x'
    yield 'y'
    yield 'z'


result = list(enumerate(gen_items()))
assert result == [(0, 'x'), (1, 'y'), (2, 'z')], 'generator in enumerate()'


# === Generator with boolean conditions ===
def gen_filtered(items, pred):
    for item in items:
        if pred(item):
            yield item


evens = list(gen_filtered(range(10), lambda x: x % 2 == 0))
assert evens == [0, 2, 4, 6, 8], 'generator with lambda predicate'


# === Nested generators: outer yields from multiple inners ===
def gen_concat(*iterables):
    for it in iterables:
        yield from it


result = list(gen_concat([1, 2], [3, 4], [5]))
assert result == [1, 2, 3, 4, 5], 'concat generator from multiple iterables'

# === Generator with try/finally and return value ===
finally_events = []


def gen_finally_return():
    try:
        yield 1
        return 'done'
    finally:
        finally_events.append('finally')


g = gen_finally_return()
next(g)
ret = None
try:
    next(g)
except StopIteration as e:
    ret = e.value
assert ret == 'done', 'generator return value with finally'
assert finally_events == ['finally'], 'finally runs even with return in generator'


# === send() and yield interaction with try/except ===
def gen_send_with_try():
    try:
        x = yield 'start'
        yield 'got: ' + str(x)
    except ValueError as e:
        yield 'error: ' + str(e)


g = gen_send_with_try()
assert next(g) == 'start', 'send+try start'
assert g.send(42) == 'got: 42', 'send value inside try block'


# === Generator that returns without yielding ===
def gen_return_only():
    if False:
        yield  # makes it a generator
    return 'never yielded'


g = gen_return_only()
ret = None
try:
    next(g)
except StopIteration as e:
    ret = e.value
assert ret == 'never yielded', 'generator that returns without yielding'


# === Yield in conditional branches ===
def gen_conditional_yield(flag):
    if flag:
        yield 'true branch'
    else:
        yield 'false branch'
    yield 'after'


assert list(gen_conditional_yield(True)) == ['true branch', 'after'], 'yield in true branch'
assert list(gen_conditional_yield(False)) == ['false branch', 'after'], 'yield in false branch'


# === Multiple generators interleaved ===
def gen_letters():
    yield 'a'
    yield 'b'
    yield 'c'


def gen_numbers():
    yield 1
    yield 2
    yield 3


g_letters = gen_letters()
g_numbers = gen_numbers()
interleaved = []
for _ in range(3):
    interleaved.append(next(g_letters))
    interleaved.append(next(g_numbers))
assert interleaved == ['a', 1, 'b', 2, 'c', 3], 'interleaved generators'


# === next(generator) in comprehension keeps generator alive across suspension ===
def gen_counter():
    n = 0
    while True:
        yield n
        n = n + 1


g = gen_counter()
values = [next(g) for _ in range(6)]
assert values == [0, 1, 2, 3, 4, 5], 'next(generator) in comprehension yields expected values'
assert next(g) == 6, 'generator remains valid after comprehension-driven next() calls'


# === Regression: send()-driven generator teardown after exhaustion ===
def gen_send_cleanup():
    total = 0
    while True:
        incoming = yield total
        if incoming is None:
            return total
        total = total + incoming


acc = gen_send_cleanup()
assert next(acc) == 0, 'send cleanup: initial yield'
assert acc.send(4) == 4, 'send cleanup: first send'
assert acc.send(9) == 13, 'send cleanup: second send'
stop_value = None
try:
    acc.send(None)
except StopIteration as e:
    stop_value = e.value
assert stop_value == 13, 'send cleanup: stop value'
