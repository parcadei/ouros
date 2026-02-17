# === map() with lambda ===
assert list(map(lambda x: x * 2, [1, 2, 3])) == [2, 4, 6], 'map with lambda'
assert list(map(lambda x: x + 1, [])) == [], 'map with lambda on empty list'

# === map() with lambda and multiple iterables ===
assert list(map(lambda x, y: x + y, [1, 2, 3], [10, 20, 30])) == [11, 22, 33], 'map with lambda multiple iterables'

# === filter() with lambda ===
assert list(filter(lambda x: x > 2, [1, 2, 3, 4, 5])) == [3, 4, 5], 'filter with lambda'
assert list(filter(lambda x: x % 2 == 0, [1, 2, 3, 4])) == [2, 4], 'filter with lambda even numbers'
assert list(filter(lambda x: True, [1, 2, 3])) == [1, 2, 3], 'filter with lambda always true'
assert list(filter(lambda x: False, [1, 2, 3])) == [], 'filter with lambda always false'

# === sorted() with lambda key ===
assert sorted([3, 1, 2], key=lambda x: -x) == [3, 2, 1], 'sorted with lambda key reverse'
assert sorted(['aaa', 'b', 'cc'], key=lambda s: len(s)) == ['b', 'cc', 'aaa'], 'sorted with lambda key=len'
assert sorted([1, 2, 3], key=lambda x: 0) == [1, 2, 3], 'sorted with lambda constant key'

# === sorted() with lambda key and reverse ===
assert sorted(['aaa', 'b', 'cc'], key=lambda s: len(s), reverse=True) == ['aaa', 'cc', 'b'], 'sorted lambda key+reverse'
