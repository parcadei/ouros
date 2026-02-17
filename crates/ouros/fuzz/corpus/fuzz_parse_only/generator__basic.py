# === Generator function returns generator object ===
def gen_simple():
    yield 1


g = gen_simple()
assert type(g).__name__ == 'generator', 'generator function returns generator object'


# === next() yields values and then raises StopIteration ===
def gen_multi():
    yield 1
    yield 2
    yield 3


g = gen_multi()
assert next(g) == 1, 'first next() yields first value'
assert next(g) == 2, 'second next() yields second value'
assert next(g) == 3, 'third next() yields third value'

stopped = False
try:
    next(g)
except StopIteration:
    stopped = True
assert stopped, 'generator raises StopIteration after exhaustion'


# === Generator local state survives across yields ===
def gen_state():
    x = 10
    yield x
    x = x + 7
    yield x


g = gen_state()
assert next(g) == 10, 'first yield sees initial local state'
assert next(g) == 17, 'second yield sees updated local state'


# === Regression: for x in gen() consumes all yielded values ===
def gen_for_regression():
    yield 4
    yield 5
    yield 6


for_values = []
for x in gen_for_regression():
    for_values.append(x)
assert for_values == [4, 5, 6], 'for-loop should resume generator across multiple yields'


# === Regression: list(gen()) collects every value ===
def gen_list_regression():
    yield 'left'
    yield 'middle'
    yield 'right'


assert list(gen_list_regression()) == ['left', 'middle', 'right'], 'list(gen()) should collect all yields'


# === Regression: list comprehension resumes generator repeatedly ===
def gen_comp_regression():
    yield 2
    yield 4
    yield 6


assert [x + 1 for x in gen_comp_regression()] == [3, 5, 7], 'list comprehension should consume full generator'


# === Regression: sum(gen()) consumes every yielded int ===
def gen_sum_regression():
    yield 10
    yield 20
    yield 30


assert sum(gen_sum_regression()) == 60, 'sum(gen()) should consume all yielded ints'


# === Regression: nested generator iteration in list comprehension ===
def inner_gen_regression():
    yield 1
    yield 3
    yield 5


def outer_gen_regression():
    inner = inner_gen_regression()
    yield next(inner)
    yield next(inner)
    yield next(inner)


assert [x for x in outer_gen_regression()] == [1, 3, 5], 'nested generator iteration should resume until exhaustion'

# Return=None
