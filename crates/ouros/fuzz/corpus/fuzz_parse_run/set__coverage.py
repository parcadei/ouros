# === Set construction from various iterables ===
s = set('hello')
assert len(s) == 4, 'set from string deduplicates'
assert 'h' in s, 'set from string contains h'
assert 'e' in s, 'set from string contains e'
assert 'l' in s, 'set from string contains l'
assert 'o' in s, 'set from string contains o'

s = set(range(5))
assert len(s) == 5, 'set from range'
assert 0 in s, 'set from range contains 0'
assert 4 in s, 'set from range contains 4'

s = set((1, 2, 3))
assert len(s) == 3, 'set from tuple'

# === Set.add() with unhashable element (lines 123-125) ===
s = set()
try:
    s.add([1, 2])
    assert False, 'add unhashable should raise TypeError'
except TypeError:
    pass

try:
    s.add({'a': 1})
    assert False, 'add dict should raise TypeError'
except TypeError:
    pass

# === Set.remove() raises KeyError for missing (lines 495-499) ===
s = {1, 2, 3}
try:
    s.remove(99)
    assert False, 'remove missing should raise KeyError'
except KeyError:
    pass

# Remove existing element
s = {1, 2, 3}
s.remove(2)
assert 2 not in s, 'remove existing element'
assert len(s) == 2, 'remove decrements length'

# === Set.pop() on empty set (line 194) ===
s = set()
try:
    s.pop()
    assert False, 'pop on empty set should raise KeyError'
except KeyError:
    pass

# === Set.contains() (lines 534-536) ===
s = {1, 2, 3}
assert 1 in s, 'in operator for set'
assert 99 not in s, 'not in operator for set'
assert 'a' not in s, 'not in with different type'

# === Set bool (lines 582-584) ===
assert bool(set()) == False, 'empty set is falsy'
assert bool({1}) == True, 'non-empty set is truthy'
assert bool({1, 2, 3}) == True, 'multi-element set is truthy'

# === Set repr ===
assert repr(set()) == 'set()', 'empty set repr'
s = {1}
r = repr(s)
assert r == '{1}', 'single element set repr'

# === Set equality (line 277) ===
assert {1, 2, 3} == {3, 2, 1}, 'set equality order independent'
assert {1, 2} != {1, 2, 3}, 'set inequality different size'
assert {1, 2, 3} != {1, 2, 4}, 'set inequality different elements'

# === Set.copy() ===
s = {1, 2, 3}
s2 = s.copy()
assert s == s2, 'copy equals original'
assert s is not s2, 'copy is different object'
s.add(4)
assert 4 not in s2, 'copy is independent'

# === Set.clear() ===
s = {1, 2, 3}
s.clear()
assert len(s) == 0, 'clear makes set empty'
assert s == set(), 'cleared set equals empty set'

# === Set.discard() ===
s = {1, 2, 3}
s.discard(2)
assert 2 not in s, 'discard removes element'
s.discard(99)
assert len(s) == 2, 'discard non-existing is no-op'

# === Set.update() (lines 738-751) ===
s = {1, 2}
s.update({3, 4})
assert s == {1, 2, 3, 4}, 'update with set'

s = {1, 2}
s.update([3, 4, 5])
assert s == {1, 2, 3, 4, 5}, 'update with list'

s = {1}
s.update(range(3))
assert 0 in s and 1 in s and 2 in s, 'update with range'

# === Set.union() ===
s1 = {1, 2}
s2 = {2, 3}
u = s1.union(s2)
assert u == {1, 2, 3}, 'union'
assert s1 == {1, 2}, 'union does not modify original'

# Union with list
u = {1, 2}.union([3, 4])
assert u == {1, 2, 3, 4}, 'union with list'

# === Set.intersection() ===
s1 = {1, 2, 3}
s2 = {2, 3, 4}
i = s1.intersection(s2)
assert i == {2, 3}, 'intersection'

# Intersection with list
i = {1, 2, 3}.intersection([2, 3, 4])
assert i == {2, 3}, 'intersection with list'

# === Set.difference() ===
s1 = {1, 2, 3}
s2 = {2, 3, 4}
d = s1.difference(s2)
assert d == {1}, 'difference'

# Difference with list
d = {1, 2, 3}.difference([2, 3])
assert d == {1}, 'difference with list'

# === Set.symmetric_difference() ===
s1 = {1, 2, 3}
s2 = {2, 3, 4}
sd = s1.symmetric_difference(s2)
assert sd == {1, 4}, 'symmetric_difference'

# Symmetric difference with list
sd = {1, 2, 3}.symmetric_difference([2, 3, 4])
assert sd == {1, 4}, 'symmetric_difference with list'

# === Set.issubset() (lines 835-838) ===
assert {1, 2}.issubset({1, 2, 3}) == True, 'issubset with set true'
assert {1, 2, 3}.issubset({1, 2}) == False, 'issubset with set false'
assert set().issubset({1, 2, 3}) == True, 'empty issubset of any'
assert {1, 2}.issubset({1, 2}) == True, 'issubset equal sets'

# issubset with list
assert {1, 2}.issubset([1, 2, 3]) == True, 'issubset with list'
assert {1, 2, 3}.issubset([1, 2]) == False, 'issubset with list false'

# === Set.issuperset() (lines 868-871) ===
assert {1, 2, 3}.issuperset({1, 2}) == True, 'issuperset with set true'
assert {1, 2}.issuperset({1, 2, 3}) == False, 'issuperset with set false'
assert {1, 2, 3}.issuperset(set()) == True, 'issuperset of empty'
assert {1, 2}.issuperset({1, 2}) == True, 'issuperset equal sets'

# issuperset with list
assert {1, 2, 3}.issuperset([1, 2]) == True, 'issuperset with list'
assert {1, 2}.issuperset([1, 2, 3]) == False, 'issuperset with list false'

# === Set.isdisjoint() (lines 901-904) ===
assert {1, 2}.isdisjoint({3, 4}) == True, 'isdisjoint with set true'
assert {1, 2}.isdisjoint({2, 3}) == False, 'isdisjoint with set false'
assert set().isdisjoint({1, 2}) == True, 'empty isdisjoint of any'
assert {1, 2}.isdisjoint(set()) == True, 'any isdisjoint of empty'

# isdisjoint with list
assert {1, 2}.isdisjoint([3, 4]) == True, 'isdisjoint with list true'
assert {1, 2}.isdisjoint([2, 3]) == False, 'isdisjoint with list false'

# === Set isdisjoint with larger self (line 304) ===
# When self is larger than other, it should iterate other
big = {1, 2, 3, 4, 5}
small = {6, 7}
assert big.isdisjoint(small) == True, 'isdisjoint large self small other'
assert big.isdisjoint({5}) == False, 'isdisjoint large self small other overlap'

# === Unknown set method (line 709) ===
try:
    s = {1, 2}
    s.bogus_method()
    assert False, 'unknown set method should raise AttributeError'
except AttributeError as e:
    assert 'set' in str(e), 'error mentions set'

# === Set iteration ===
s = {10, 20, 30}
items = []
for x in s:
    items.append(x)
assert len(items) == 3, 'set iteration yields all elements'
assert set(items) == {10, 20, 30}, 'set iteration yields correct elements'

# === Set comprehension ===
s = {x * 2 for x in range(4)}
assert s == {0, 2, 4, 6}, 'set comprehension'

# === Frozenset construction ===
fs = frozenset()
assert len(fs) == 0, 'empty frozenset'

fs = frozenset([1, 2, 3])
assert len(fs) == 3, 'frozenset from list'

fs = frozenset({1, 2, 3})
assert len(fs) == 3, 'frozenset from set'

fs = frozenset('hello')
assert len(fs) == 4, 'frozenset from string deduplicates'

# === Frozenset repr ===
assert repr(frozenset()) == 'frozenset()', 'empty frozenset repr'
fs = frozenset([1])
r = repr(fs)
assert 'frozenset' in r, 'frozenset repr has type name'

# === Frozenset bool ===
assert bool(frozenset()) == False, 'empty frozenset is falsy'
assert bool(frozenset([1])) == True, 'non-empty frozenset is truthy'

# === Frozenset equality ===
assert frozenset([1, 2, 3]) == frozenset([3, 2, 1]), 'frozenset equality'
assert frozenset([1, 2]) != frozenset([1, 2, 3]), 'frozenset inequality'

# === Frozenset in dict/set (hashable) ===
d = {frozenset([1, 2]): 'pair'}
assert d[frozenset([1, 2])] == 'pair', 'frozenset as dict key'
assert d[frozenset([2, 1])] == 'pair', 'frozenset as dict key order independent'

# === Frozenset.copy() (lines 1085-1087) ===
fs = frozenset([1, 2, 3])
fs2 = fs.copy()
assert fs == fs2, 'frozenset copy equals original'

# === Frozenset set operations (lines 1127-1195) ===
fs1 = frozenset([1, 2, 3])
fs2 = frozenset([2, 3, 4])

# Union
u = fs1.union(fs2)
assert u == frozenset([1, 2, 3, 4]), 'frozenset union'

# Intersection
i = fs1.intersection(fs2)
assert i == frozenset([2, 3]), 'frozenset intersection'

# Difference
d = fs1.difference(fs2)
assert d == frozenset([1]), 'frozenset difference'

# Symmetric difference
sd = fs1.symmetric_difference(fs2)
assert sd == frozenset([1, 4]), 'frozenset symmetric_difference'

# === Frozenset issubset/issuperset/isdisjoint (lines 1199-1307) ===
fs1 = frozenset([1, 2])
fs2 = frozenset([1, 2, 3])

assert fs1.issubset(fs2) == True, 'frozenset issubset true'
assert fs2.issubset(fs1) == False, 'frozenset issubset false'
assert fs2.issuperset(fs1) == True, 'frozenset issuperset true'
assert fs1.issuperset(fs2) == False, 'frozenset issuperset false'

fs3 = frozenset([4, 5])
assert fs1.isdisjoint(fs3) == True, 'frozenset isdisjoint true'
assert fs1.isdisjoint(fs2) == False, 'frozenset isdisjoint false'

# Frozenset operations with list arguments
assert frozenset([1, 2]).issubset([1, 2, 3]) == True, 'frozenset issubset with list'
assert frozenset([1, 2, 3]).issuperset([1, 2]) == True, 'frozenset issuperset with list'
assert frozenset([1, 2]).isdisjoint([3, 4]) == True, 'frozenset isdisjoint with list'

# === Frozenset unknown method (line 1195) ===
try:
    fs = frozenset([1])
    fs.bogus_method()
    assert False, 'unknown frozenset method should raise AttributeError'
except AttributeError as e:
    assert 'frozenset' in str(e), 'error mentions frozenset'

# === Frozenset contains (lines 999-1006) ===
fs = frozenset([1, 2, 3])
assert 1 in fs, 'in operator for frozenset'
assert 99 not in fs, 'not in operator for frozenset'

# === Set with string elements ===
s = {'a', 'b', 'c'}
assert len(s) == 3, 'set of strings'
assert 'a' in s, 'string in set'
assert 'd' not in s, 'string not in set'

# === Set union with frozenset ===
s = {1, 2}
fs = frozenset([2, 3])
u = s.union(fs)
assert u == {1, 2, 3}, 'set union with frozenset'

# === Set intersection with frozenset ===
s = {1, 2, 3}
fs = frozenset([2, 3, 4])
i = s.intersection(fs)
assert i == {2, 3}, 'set intersection with frozenset'

# === Set difference with frozenset ===
s = {1, 2, 3}
fs = frozenset([2, 3])
d = s.difference(fs)
assert d == {1}, 'set difference with frozenset'

# === Set symmetric_difference with frozenset ===
s = {1, 2, 3}
fs = frozenset([2, 3, 4])
sd = s.symmetric_difference(fs)
assert sd == {1, 4}, 'set symmetric_difference with frozenset'

# === Set.update() with frozenset ===
s = {1, 2}
fs = frozenset([2, 3, 4])
s.update(fs)
assert s == {1, 2, 3, 4}, 'update with frozenset'

# === Set.issubset() with frozenset (lines 835-838) ===
assert {1, 2}.issubset(frozenset([1, 2, 3])) == True, 'issubset with frozenset'

# === Set.issuperset() with frozenset (lines 868-871) ===
assert {1, 2, 3}.issuperset(frozenset([1, 2])) == True, 'issuperset with frozenset'

# === Set.isdisjoint() with frozenset (lines 901-904) ===
assert {1, 2}.isdisjoint(frozenset([3, 4])) == True, 'isdisjoint with frozenset'

# === Frozenset operations with set arguments ===
fs = frozenset([1, 2, 3])
s = {2, 3, 4}

u = fs.union(s)
assert u == frozenset([1, 2, 3, 4]), 'frozenset union with set'

i = fs.intersection(s)
assert i == frozenset([2, 3]), 'frozenset intersection with set'

d = fs.difference(s)
assert d == frozenset([1]), 'frozenset difference with set'

sd = fs.symmetric_difference(s)
assert sd == frozenset([1, 4]), 'frozenset symmetric_difference with set'

# === Frozenset issubset with set ===
assert frozenset([1, 2]).issubset({1, 2, 3}) == True, 'frozenset issubset with set'
assert frozenset([1, 2, 3]).issubset({1, 2}) == False, 'frozenset issubset with set false'

# === Frozenset issuperset with set ===
assert frozenset([1, 2, 3]).issuperset({1, 2}) == True, 'frozenset issuperset with set'
assert frozenset([1, 2]).issuperset({1, 2, 3}) == False, 'frozenset issuperset with set false'

# === Frozenset isdisjoint with set ===
assert frozenset([1, 2]).isdisjoint({3, 4}) == True, 'frozenset isdisjoint with set'
assert frozenset([1, 2]).isdisjoint({2, 3}) == False, 'frozenset isdisjoint with set false'

# === Frozenset union with list ===
u = frozenset([1, 2]).union([3, 4])
assert u == frozenset([1, 2, 3, 4]), 'frozenset union with list'

# === Frozenset intersection with list ===
i = frozenset([1, 2, 3]).intersection([2, 3, 4])
assert i == frozenset([2, 3]), 'frozenset intersection with list'

# === Frozenset difference with list ===
d = frozenset([1, 2, 3]).difference([2, 3])
assert d == frozenset([1]), 'frozenset difference with list'

# === Frozenset symmetric_difference with list ===
sd = frozenset([1, 2, 3]).symmetric_difference([2, 3, 4])
assert sd == frozenset([1, 4]), 'frozenset symmetric_difference with list'

# === Set len ===
assert len(set()) == 0, 'empty set len'
assert len({1, 2, 3}) == 3, 'set len'
assert len(frozenset()) == 0, 'empty frozenset len'
assert len(frozenset([1, 2])) == 2, 'frozenset len'

# === Frozenset in set ===
s = {frozenset([1, 2]), frozenset([3, 4])}
assert frozenset([1, 2]) in s, 'frozenset in set'
assert frozenset([5, 6]) not in s, 'frozenset not in set'
