from collections import Counter

# === Counter iteration yields keys in insertion order ===
c = Counter('abracadabra')
expected_keys = ['a', 'b', 'r', 'c', 'd']
assert list(c) == expected_keys, 'counter list() iteration yields keys'

for_keys = []
for key in c:
    for_keys.append(key)
assert for_keys == expected_keys, 'counter for-loop iteration yields keys'

comp_keys = [key for key in c]
assert comp_keys == expected_keys, 'counter comprehension iteration yields keys'

extended_keys = []
extended_keys.extend(c)
assert extended_keys == expected_keys, 'counter list.extend() consumes iterator keys'

# === Counter constructed from mapping still iterates keys ===
c_map = Counter({'x': 2, 'y': 1, 'z': 5})
assert list(c_map) == ['x', 'y', 'z'], 'counter mapping constructor keeps key iteration order'

# === Counter.values() returns a view that can be iterated ===
c2 = Counter(['a', 'b', 'a', 'c', 'a'])
vals = list(c2.values())
assert sorted(vals) == [1, 1, 3], 'counter values() returns counts'
assert sum(c2.values()) == 5, 'counter sum(values()) works'

# === Counter.values() in generator expression ===
c3 = Counter(['a', 'b', 'a', 'c', 'a'])
result = sum(v for v in c3.values())
assert result == 5, 'counter values() works in generator expression'

# === Counter.values() in list comprehension ===
c4 = Counter(['a', 'b', 'a', 'c', 'a'])
doubled = [v * 2 for v in c4.values()]
assert sorted(doubled) == [2, 2, 6], 'counter values() works in list comprehension'

# === Counter.keys() returns a view that can be iterated ===
c5 = Counter(['a', 'b', 'a', 'c', 'a'])
keys = list(c5.keys())
assert sorted(keys) == ['a', 'b', 'c'], 'counter keys() returns element keys'

# === Counter.items() returns a view that can be iterated ===
c6 = Counter(['a', 'b', 'a', 'c', 'a'])
items = list(c6.items())
items_sorted = sorted(items, key=lambda x: x[0])
assert items_sorted == [('a', 3), ('b', 1), ('c', 1)], 'counter items() returns (key, count) pairs'
