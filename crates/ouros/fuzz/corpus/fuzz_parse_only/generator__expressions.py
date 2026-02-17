# === Generator expression returns a generator, not a list ===
g = (x for x in range(5))
assert type(g).__name__ == 'generator', 'generator expression creates generator object'

# === Generator expression is lazy (not eagerly evaluated) ===
evaluated = []


def track(x):
    evaluated.append(x)
    return x


g = (track(x) for x in range(3))
assert evaluated == [], 'generator expression is lazy, nothing evaluated yet'
next(g)
assert evaluated == [0], 'only first element evaluated after one next()'
next(g)
assert evaluated == [0, 1], 'second element evaluated after two next() calls'
next(g)
assert evaluated == [0, 1, 2], 'third element evaluated after three next() calls'

# === Generator expression consumed by list() ===
result = list(x * 2 for x in range(5))
assert result == [0, 2, 4, 6, 8], 'generator expression consumed by list()'

# === Generator expression in sum() ===
assert sum(x for x in range(5)) == 10, 'generator expression in sum()'

# === Generator expression in any() ===
assert any(x > 3 for x in range(5)), 'generator expression in any()'
assert not any(x > 10 for x in range(5)), 'generator expression in any() false'

# === Generator expression in all() ===
assert all(x < 5 for x in range(5)), 'generator expression in all()'
assert not all(x < 3 for x in range(5)), 'generator expression in all() false'

# === Generator expression with condition ===
result = list(x for x in range(10) if x % 2 == 0)
assert result == [0, 2, 4, 6, 8], 'generator expression with condition'

# === Generator expression with transformation and condition ===
result = list(x * x for x in range(6) if x % 2 != 0)
assert result == [1, 9, 25], 'generator expression with transform and condition'

# === Nested generator expression ===
result = list(x + y for x in range(3) for y in range(2))
assert result == [0, 1, 1, 2, 2, 3], 'nested generator expression'

# === Generator expression is single-use (exhaustion) ===
g = (x for x in range(3))
first = list(g)
second = list(g)
assert first == [0, 1, 2], 'first consumption of generator expression'
assert second == [], 'second consumption of exhausted generator expression'

# === Generator expression __iter__ returns self ===
g = (x for x in range(3))
assert iter(g) is g, 'generator expression __iter__ returns self'

# === Generator expression in for loop ===
result = []
for x in (i * 10 for i in range(4)):
    result.append(x)
assert result == [0, 10, 20, 30], 'generator expression in for loop'

# === Generator expression with tuple unpacking ===
pairs = [(1, 2), (3, 4), (5, 6)]
result = list(a + b for a, b in pairs)
assert result == [3, 7, 11], 'generator expression with tuple unpacking'

# === Generator expression captures variables from enclosing scope ===
multiplier = 3
result = list(x * multiplier for x in range(4))
assert result == [0, 3, 6, 9], 'generator expression captures enclosing scope variable'

# === Generator expression with multiple conditions ===
result = list(x for x in range(20) if x % 2 == 0 if x % 3 == 0)
assert result == [0, 6, 12, 18], 'generator expression with multiple conditions'

# === Generator expression with string operations ===
words = ['hello', 'world', 'foo']
result = list(w + '!' for w in words)
assert result == ['hello!', 'world!', 'foo!'], 'generator expression with strings'

# === Generator expression passed directly to tuple() ===
result = tuple(x * x for x in range(5))
assert result == (0, 1, 4, 9, 16), 'generator expression to tuple'

# === Empty generator expression ===
result = list(x for x in [])
assert result == [], 'empty generator expression'

# === Generator expression where condition filters everything ===
result = list(x for x in range(5) if x > 100)
assert result == [], 'generator expression where condition filters all'

# === Generator expression with bool conversion ===
result = list(not x for x in [True, False, True])
assert result == [False, True, False], 'generator expression with bool conversion'


# === Nested function with generator expression ===
def make_gen(n):
    return (x * 2 for x in range(n))


assert list(make_gen(4)) == [0, 2, 4, 6], 'function returning generator expression'

# === Generator expression: outermost iterator created eagerly, but list mutation is visible ===
# CPython calls iter() on the outermost iterable at creation time.
# For a list, the iterator sees mutations to the list (appends, etc.)
# because list iterators check length dynamically.
source = [1, 2, 3]
g = (x * 10 for x in source)
source.append(4)  # mutation visible because list iterator checks length
result = list(g)
assert result == [10, 20, 30, 40], 'list mutations visible through generator expression iterator'

# Replacing the source entirely after creation does not affect the generator.
# The generator holds a reference to the original list via the iterator.
source2 = [1, 2, 3]
g2 = (x for x in source2)
source2 = [99]  # rebind, does not affect the iterator
assert list(g2) == [1, 2, 3], 'rebinding source variable does not affect generator'
