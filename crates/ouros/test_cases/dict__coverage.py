# === dict() constructor with non-dict/non-iterable (lines 441-443, 449-451) ===
try:
    dict(42)
    assert False, 'dict(int) should raise TypeError'
except TypeError:
    pass

try:
    dict(True)
    assert False, 'dict(bool) should raise TypeError'
except TypeError:
    pass

# === dict() constructor copies dict (lines 463-473) ===
original = {'a': 1, 'b': 2}
copy = dict(original)
assert copy == {'a': 1, 'b': 2}, 'dict() copies dict'
assert copy is not original, 'dict() creates new dict'
original['c'] = 3
assert 'c' not in copy, 'dict() copy is independent'

# === Dict equality with different lengths (line 555) ===
assert ({'a': 1} == {'a': 1, 'b': 2}) == False, 'eq different length false'
assert ({'a': 1, 'b': 2} == {'a': 1}) == False, 'eq different length false reverse'

# === Dict equality with different values (line 566) ===
assert ({'a': 1, 'b': 2} == {'a': 1, 'b': 3}) == False, 'eq same keys different values'

# === Dict bool (line 610) ===
assert bool({}) == False, 'empty dict is falsy'
assert bool({'a': 1}) == True, 'non-empty dict is truthy'

# === Dict attribute error (line 656) ===
try:
    d = {'a': 1}
    d.nonexistent()
    assert False, 'unknown dict method should raise AttributeError'
except AttributeError as e:
    assert 'dict' in str(e), 'error mentions dict'

# === Dict.pop() missing key without default raises KeyError (line 727) ===
d = {'a': 1}
try:
    d.pop('missing')
    assert False, 'pop missing key no default should raise KeyError'
except KeyError:
    pass

# === Unknown dict method on class (line 759) ===
try:
    d = {'a': 1}
    d.bogus_method()
    assert False, 'bogus method should raise AttributeError'
except AttributeError:
    pass

# === dict.update() too many positional args (lines 810-818) ===
try:
    d = {}
    d.update({'a': 1}, {'b': 2})
    assert False, 'update with 2 positional args should raise TypeError'
except TypeError:
    pass

# === dict.update() with iterable of pairs (lines 834-979) ===
# List of tuples
d = {}
d.update([('a', 1), ('b', 2)])
assert d == {'a': 1, 'b': 2}, 'update with list of tuples'

# Tuple of tuples
d = {}
d.update((('x', 10), ('y', 20)))
assert d == {'x': 10, 'y': 20}, 'update with tuple of tuples'

# Update with dict (fast path, lines 834-863)
d = {'a': 1}
other = {'b': 2, 'c': 3}
d.update(other)
assert d == {'a': 1, 'b': 2, 'c': 3}, 'update with dict fast path'

# Update overwrites existing keys
d = {'a': 1, 'b': 2}
d.update({'b': 20, 'c': 30})
assert d == {'a': 1, 'b': 20, 'c': 30}, 'update dict overwrites'

# === dict.update() sequence element too short (lines 902-907, 920-927) ===
try:
    d = {}
    d.update([(), ('b', 2)])
    assert False, 'update with empty tuple should raise'
except (TypeError, ValueError) as e:
    assert 'length' in str(e).lower(), 'error mentions length'

try:
    d = {}
    d.update([('a',)])
    assert False, 'update with 1-element tuple should raise'
except (TypeError, ValueError) as e:
    assert 'length' in str(e).lower(), 'error mentions length for 1-element'

# === dict.update() sequence element too long (lines 940-957) ===
try:
    d = {}
    d.update([('a', 1, 'extra')])
    assert False, 'update with 3-element tuple should raise'
except (TypeError, ValueError) as e:
    assert 'length' in str(e).lower(), 'error mentions length for 3-element'

# === dict.setdefault() with unhashable key (lines 1031-1034) ===
try:
    d = {'a': 1}
    d.setdefault([1, 2], 'default')
    assert False, 'setdefault with list key should raise TypeError'
except TypeError:
    pass

# === dict.setdefault() returns existing and inserts new ===
d = {'a': 1}
assert d.setdefault('a', 99) == 1, 'setdefault existing returns value'
assert d.setdefault('b') is None, 'setdefault new returns None'
assert d['b'] is None, 'setdefault inserts None'
assert d.setdefault('c', 42) == 42, 'setdefault new returns default'
assert d['c'] == 42, 'setdefault inserts default'

# === dict.popitem() on empty dict (lines 1048-1049 via popitem_empty test, but exercise here too) ===
try:
    d = {}
    d.popitem()
    assert False, 'popitem on empty dict should raise KeyError'
except KeyError:
    pass

# === dict.fromkeys() with tuple iterable (lines 1132-1134) ===
d = dict.fromkeys((1, 2, 3), 'val')
assert d == {1: 'val', 2: 'val', 3: 'val'}, 'fromkeys with tuple iterable'

# === dict.fromkeys() with non-iterable (lines 1132-1134) ===
try:
    dict.fromkeys(42)
    assert False, 'fromkeys with int should raise TypeError'
except TypeError:
    pass

# === Dict comprehension ===
d = {k: k * 2 for k in range(3)}
assert d == {0: 0, 1: 2, 2: 4}, 'dict comprehension'

# === Dict in operator ===
d = {'a': 1, 'b': 2}
assert 'a' in d, 'key in dict'
assert 'c' not in d, 'key not in dict'
assert 1 not in d, 'value not in dict (checks keys)'

# === Dict iteration ===
d = {'a': 1, 'b': 2, 'c': 3}
keys = []
for k in d:
    keys.append(k)
assert keys == ['a', 'b', 'c'], 'dict iteration yields keys in order'

# === Dict keys/values/items (already tested but more coverage) ===
d = {'x': 10, 'y': 20}
assert list(d.keys()) == ['x', 'y'], 'keys returns list of keys'
assert list(d.values()) == [10, 20], 'values returns list of values'
assert list(d.items()) == [('x', 10), ('y', 20)], 'items returns list of tuples'

# === Dict merge operator | (if supported) ===
d1 = {'a': 1}
d2 = {'b': 2}
try:
    merged = d1 | d2
    assert merged == {'a': 1, 'b': 2}, 'dict merge operator'
except TypeError:
    pass  # operator may not be implemented yet

# === Dict with bool keys ===
d = {True: 'yes', False: 'no'}
assert d[True] == 'yes', 'bool key True'
assert d[False] == 'no', 'bool key False'

# === Dict.get() with unhashable key (lines 262-268) ===
try:
    d = {'a': 1}
    d.get([1, 2])
    assert False, 'get with list key should raise TypeError'
except TypeError:
    pass

# === Empty dict repr ===
assert repr({}) == '{}', 'empty dict repr'

# === Dict with nested values ===
d = {'a': [1, 2], 'b': {'c': 3}}
assert d['a'] == [1, 2], 'nested list value'
assert d['b'] == {'c': 3}, 'nested dict value'

# === Dict.copy() is shallow ===
inner = [1, 2, 3]
d = {'a': inner}
d2 = d.copy()
assert d2['a'] is inner, 'copy is shallow - same inner object'

# === Dict.clear() on dict with refs ===
d = {'a': [1, 2], 'b': 'hello'}
d.clear()
assert d == {}, 'clear removes all including ref values'
assert len(d) == 0, 'clear results in len 0'

# === Dict.update() with kwargs only ===
d = {}
d.update(x=1, y=2, z=3)
assert d == {'x': 1, 'y': 2, 'z': 3}, 'update kwargs only'

# === Dict.update() with both dict and kwargs ===
d = {}
d.update({'a': 1}, b=2)
assert d == {'a': 1, 'b': 2}, 'update dict + kwargs'

# === Dict deletion ===
d = {'a': 1, 'b': 2, 'c': 3}
del d['b']
assert d == {'a': 1, 'c': 3}, 'del removes key'
assert len(d) == 2, 'del decrements length'

try:
    del d['missing']
    assert False, 'del missing key should raise KeyError'
except KeyError:
    pass

# === Dict with None key ===
d = {None: 'nothing'}
assert d[None] == 'nothing', 'None as dict key'

# === Dict with tuple keys ===
d = {(1, 2): 'pair'}
assert d[(1, 2)] == 'pair', 'tuple key lookup'
