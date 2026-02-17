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
