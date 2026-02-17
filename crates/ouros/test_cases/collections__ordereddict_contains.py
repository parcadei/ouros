import collections

# === OrderedDict membership (in operator) ===
d = collections.OrderedDict()
d[1] = 'a'
d[2] = 'b'

# Basic key membership
assert 1 in d, 'ordereddict: existing key should be found with in'
assert 2 in d, 'ordereddict: second key should be found with in'
assert 3 not in d, 'ordereddict: missing key should not be found with in'

# String keys
d2 = collections.OrderedDict([('x', 10), ('y', 20)])
assert 'x' in d2, 'ordereddict: string key membership'
assert 'z' not in d2, 'ordereddict: missing string key not in'

# Empty OrderedDict
d3 = collections.OrderedDict()
assert 'anything' not in d3, 'ordereddict: empty dict contains nothing'

# === move_to_end preserves membership ===
d.move_to_end(1)
assert 1 in d, 'ordereddict: key still present after move_to_end'
assert list(d.keys()) == [2, 1], 'ordereddict: move_to_end changes order'

# move_to_end with last=False
d.move_to_end(1, last=False)
assert list(d.keys()) == [1, 2], 'ordereddict: move_to_end(last=False) moves to front'

# === popitem removes from membership ===
d4 = collections.OrderedDict([(1, 'a'), (2, 'b'), (3, 'c')])
item = d4.popitem()
assert item == (3, 'c'), 'ordereddict: popitem() pops last by default'
assert 3 not in d4, 'ordereddict: popped key no longer in dict'
assert 1 in d4, 'ordereddict: remaining key still in dict'

item2 = d4.popitem(last=False)
assert item2 == (1, 'a'), 'ordereddict: popitem(last=False) pops first'
assert 1 not in d4, 'ordereddict: popped first key no longer in dict'
