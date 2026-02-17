# === Dict literals ===
assert {} == {}, 'empty literal'
assert {'a': 1} == {'a': 1}, 'single item literal'
assert {'a': 1, 'b': 2} == {'a': 1, 'b': 2}, 'multiple items literal'
assert {1: 'a', 2: 'b'} == {1: 'a', 2: 'b'}, 'int keys literal'

# === Dict length ===
assert len({}) == 0, 'len empty'
assert len({'a': 1, 'b': 2, 'c': 3}) == 3, 'len multiple'

# === Dict equality ===
assert ({'a': 1, 'b': 2} == {'b': 2, 'a': 1}) == True, 'equality true (order independent)'
assert ({'a': 1} == {'a': 2}) == False, 'equality false'

# === Dict subscript get ===
d = {'name': 'Alice', 'age': 30}
assert d['name'] == 'Alice', 'subscript get str key'
assert d['age'] == 30, 'subscript get value'

d = {1: 'one', 2: 'two'}
assert d[1] == 'one', 'subscript get int key'

# === Dict subscript set ===
d = {'a': 1}
d['b'] = 2
assert d == {'a': 1, 'b': 2}, 'subscript set new key'

d = {'a': 1}
d['a'] = 99
assert d == {'a': 99}, 'subscript set existing key'

# === Dict.get() method ===
d = {'a': 1, 'b': 2}
assert d.get('a') == 1, 'get existing'
assert d.get('missing') is None, 'get missing returns None'
assert d.get('missing', 'default') == 'default', 'get missing with default'

# === Dict.pop() method ===
d = {'a': 1, 'b': 2}
assert d.pop('a') == 1, 'pop existing'
assert d == {'b': 2}, 'pop removes key'

d = {'a': 1}
assert d.pop('missing', 'default') == 'default', 'pop missing with default'

# === Dict with tuple key ===
d = {(1, 2): 'value'}
assert d[(1, 2)] == 'value', 'tuple key'

# === Dict repr ===
assert repr({}) == '{}', 'empty repr'
assert repr({'a': 1}) == "{'a': 1}", 'repr with items'

# === Dict self-reference ===
d = {}
d['self'] = d
assert d['self'] is d, 'getitem self'

d = {}
assert d.get('missing', d) is d, 'get default same dict'
