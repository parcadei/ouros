# === add ===
try:
    s = {1, 2}
    s.add(3)
    print('add_basic', sorted(s))

    s = {1, 2}
    s.add(2)
    print('add_duplicate', sorted(s))

    s = set()
    s.add(None)
    print('add_none', sorted(s, key=lambda x: (x is None, x)))

    s = {1}
    s.add('hello')
    print('add_string', sorted(s, key=str))

    s = {1}
    s.add((2, 3))
    print('add_tuple', sorted(s, key=str))
except Exception as e:
    print('SKIP_add', type(e).__name__, e)

# === clear ===
try:
    s = {1, 2, 3}
    s.clear()
    print('clear_non_empty', s)

    s = set()
    s.clear()
    print('clear_empty', s)
except Exception as e:
    print('SKIP_clear', type(e).__name__, e)

# === copy ===
try:
    s = {1, 2, 3}
    c = s.copy()
    print('copy_basic', sorted(c))
    c.add(4)
    print('copy_is_shallow', sorted(s), sorted(c))

    s = {1, (2, 3)}
    c = s.copy()
    print('copy_with_tuple', sorted(c, key=str))
except Exception as e:
    print('SKIP_copy', type(e).__name__, e)

# === difference ===
try:
    s1 = {1, 2, 3, 4}
    s2 = {2, 4}
    print('difference_basic', sorted(s1.difference(s2)))

    s1 = {1, 2, 3}
    print('difference_empty', sorted(s1.difference()))

    s1 = {1, 2, 3}
    s2 = {4, 5}
    print('difference_no_common', sorted(s1.difference(s2)))

    s1 = {1, 2, 3}
    s2 = {2}
    s3 = {3}
    print('difference_multiple', sorted(s1.difference(s2, s3)))

    # Test frozenset difference
    fs = frozenset([1, 2, 3, 4])
    s = {2, 4}
    print('frozenset_difference', sorted(fs.difference(s)))
except Exception as e:
    print('SKIP_difference', type(e).__name__, e)

# === difference_update ===
try:
    s1 = {1, 2, 3, 4}
    s2 = {2, 4}
    s1.difference_update(s2)
    print('difference_update_basic', sorted(s1))

    s1 = {1, 2, 3, 4}
    s1.difference_update()
    print('difference_update_empty', sorted(s1))

    s1 = {1, 2, 3, 4}
    s1.difference_update({1}, {3})
    print('difference_update_multiple', sorted(s1))
except Exception as e:
    print('SKIP_difference_update', type(e).__name__, e)

# === discard ===
try:
    s = {1, 2, 3}
    s.discard(2)
    print('discard_basic', sorted(s))

    s = {1, 2, 3}
    s.discard(99)
    print('discard_missing', sorted(s))

    s = {1, 2, 3}
    s.discard(None)
    print('discard_none', sorted(s))
except Exception as e:
    print('SKIP_discard', type(e).__name__, e)

# === intersection ===
try:
    s1 = {1, 2, 3, 4}
    s2 = {2, 4, 6}
    print('intersection_basic', sorted(s1.intersection(s2)))

    s1 = {1, 2, 3}
    print('intersection_empty', sorted(s1.intersection()))

    s1 = {1, 2, 3}
    s2 = {4, 5, 6}
    print('intersection_no_common', sorted(s1.intersection(s2)))

    s1 = {1, 2, 3, 4}
    s2 = {2, 3, 5}
    s3 = {3, 4, 5}
    print('intersection_multiple', sorted(s1.intersection(s2, s3)))

    # Test frozenset intersection
    fs = frozenset([1, 2, 3])
    s = {2, 3, 4}
    print('frozenset_intersection', sorted(fs.intersection(s)))
except Exception as e:
    print('SKIP_intersection', type(e).__name__, e)

# === intersection_update ===
try:
    s1 = {1, 2, 3, 4}
    s2 = {2, 4, 6}
    s1.intersection_update(s2)
    print('intersection_update_basic', sorted(s1))

    s1 = {1, 2, 3, 4}
    s1.intersection_update()
    print('intersection_update_empty', sorted(s1))

    s1 = {1, 2, 3, 4}
    s2 = {2, 3, 5}
    s3 = {2, 5, 6}
    s1.intersection_update(s2, s3)
    print('intersection_update_multiple', sorted(s1))
except Exception as e:
    print('SKIP_intersection_update', type(e).__name__, e)

# === isdisjoint ===
try:
    s1 = {1, 2, 3}
    s2 = {4, 5, 6}
    print('isdisjoint_true', s1.isdisjoint(s2))

    s1 = {1, 2, 3}
    s2 = {2, 4, 6}
    print('isdisjoint_false', s1.isdisjoint(s2))

    s1 = set()
    s2 = {1, 2, 3}
    print('isdisjoint_empty_set', s1.isdisjoint(s2))

    s1 = {1, 2, 3}
    s2 = []
    print('isdisjoint_list', s1.isdisjoint(s2))

    # Test frozenset isdisjoint
    fs = frozenset([1, 2, 3])
    s = {4, 5}
    print('frozenset_isdisjoint', fs.isdisjoint(s))
except Exception as e:
    print('SKIP_isdisjoint', type(e).__name__, e)

# === issubset ===
try:
    s1 = {1, 2}
    s2 = {1, 2, 3, 4}
    print('issubset_true', s1.issubset(s2))

    s1 = {1, 2, 3}
    s2 = {1, 2}
    print('issubset_false', s1.issubset(s2))

    s1 = {1, 2, 3}
    s2 = {1, 2, 3}
    print('issubset_equal', s1.issubset(s2))

    s1 = set()
    s2 = {1, 2, 3}
    print('issubset_empty', s1.issubset(s2))

    # Test frozenset issubset
    fs = frozenset([1, 2])
    s = {1, 2, 3}
    print('frozenset_issubset', fs.issubset(s))
except Exception as e:
    print('SKIP_issubset', type(e).__name__, e)

# === issuperset ===
try:
    s1 = {1, 2, 3, 4}
    s2 = {1, 2}
    print('issuperset_true', s1.issuperset(s2))

    s1 = {1, 2}
    s2 = {1, 2, 3}
    print('issuperset_false', s1.issuperset(s2))

    s1 = {1, 2, 3}
    s2 = {1, 2, 3}
    print('issuperset_equal', s1.issuperset(s2))

    s1 = {1, 2, 3}
    s2 = set()
    print('issuperset_empty', s1.issuperset(s2))

    # Test frozenset issuperset
    fs = frozenset([1, 2, 3])
    s = {1, 2}
    print('frozenset_issuperset', fs.issuperset(s))
except Exception as e:
    print('SKIP_issuperset', type(e).__name__, e)

# === pop ===
try:
    s = {1, 2, 3}
    popped = s.pop()
    print('pop_returns_item', popped in {1, 2, 3}, len(s))

    s = {42}
    popped = s.pop()
    print('pop_single', popped, s)

    # Note: pop on empty set raises KeyError
except Exception as e:
    print('SKIP_pop', type(e).__name__, e)
# === remove ===
try:
    s = {1, 2, 3}
    s.remove(2)
    print('remove_basic', sorted(s))

    s = {1, 2, 3}
    try:
        s.remove(99)
    except KeyError:
        print('remove_missing_raises', True)
except Exception as e:
    print('SKIP_remove', type(e).__name__, e)

# === symmetric_difference ===
try:
    s1 = {1, 2, 3}
    s2 = {2, 3, 4}
    print('symmetric_difference_basic', sorted(s1.symmetric_difference(s2)))

    s1 = {1, 2, 3}
    s2 = {4, 5, 6}
    print('symmetric_difference_no_common', sorted(s1.symmetric_difference(s2)))

    s1 = {1, 2, 3}
    s2 = {1, 2, 3}
    print('symmetric_difference_same', sorted(s1.symmetric_difference(s2)))

    # Test with list
    s1 = {1, 2, 3}
    print('symmetric_difference_list', sorted(s1.symmetric_difference([2, 3, 4])))

    # Test frozenset symmetric_difference
    fs = frozenset([1, 2, 3])
    s = {3, 4, 5}
    print('frozenset_symmetric_difference', sorted(fs.symmetric_difference(s)))
except Exception as e:
    print('SKIP_symmetric_difference', type(e).__name__, e)

# === symmetric_difference_update ===
try:
    s1 = {1, 2, 3}
    s2 = {2, 3, 4}
    s1.symmetric_difference_update(s2)
    print('symmetric_difference_update_basic', sorted(s1))

    s1 = {1, 2, 3}
    s1.symmetric_difference_update([2, 3, 4])
    print('symmetric_difference_update_list', sorted(s1))
except Exception as e:
    print('SKIP_symmetric_difference_update', type(e).__name__, e)

# === union ===
try:
    s1 = {1, 2, 3}
    s2 = {3, 4, 5}
    print('union_basic', sorted(s1.union(s2)))

    s1 = {1, 2}
    s2 = {3, 4}
    s3 = {5, 6}
    print('union_multiple', sorted(s1.union(s2, s3)))

    s1 = {1, 2, 3}
    print('union_empty', sorted(s1.union()))

    # Test with list
    s1 = {1, 2, 3}
    print('union_list', sorted(s1.union([3, 4, 5])))

    # Test frozenset union
    fs = frozenset([1, 2])
    s = {3, 4}
    print('frozenset_union', sorted(fs.union(s)))
except Exception as e:
    print('SKIP_union', type(e).__name__, e)

# === update ===
try:
    s1 = {1, 2, 3}
    s2 = {3, 4, 5}
    s1.update(s2)
    print('update_basic', sorted(s1))

    s1 = {1, 2}
    s1.update([3, 4], [5, 6])
    print('update_multiple', sorted(s1))

    s1 = {1, 2, 3}
    s1.update()
    print('update_empty', sorted(s1))

    s1 = {1, 2}
    s1.update('abc')
    print('update_string', sorted(s1, key=str))

    s1 = {1, 2}
    s1.update({3}, {4, 5}, [6])
    print('update_mixed', sorted(s1))
except Exception as e:
    print('SKIP_update', type(e).__name__, e)

# === frozenset_only_copy ===
try:
    fs = frozenset([1, 2, 3])
    c = fs.copy()
    print('frozenset_copy', sorted(c))
except Exception as e:
    print('SKIP_frozenset_only_copy', type(e).__name__, e)

# === edge_cases ===
try:
    # Test that set methods work with any iterable
    s = {1, 2, 3}
    print('intersection_string', sorted(s.intersection('abc')))

    s = {1, 2, 3}
    s.update((4, 5))
    print('update_tuple', sorted(s))

    s = {1, 2, 3}
    print('difference_dict_keys', sorted(s.difference({2: 'a', 3: 'b'})))

    # Test with range
    s = {1, 2, 3, 4, 5}
    print('intersection_range', sorted(s.intersection(range(3, 10))))
except Exception as e:
    print('SKIP_edge_cases', type(e).__name__, e)
