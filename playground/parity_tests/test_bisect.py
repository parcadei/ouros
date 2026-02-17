import bisect

# === bisect_left ===
try:
    # Normal usage with sorted list
    arr = [1, 3, 5, 7]
    print('bisect_left_existing', bisect.bisect_left(arr, 3))
    print('bisect_left_not_existing', bisect.bisect_left(arr, 4))
    print('bisect_left_before_first', bisect.bisect_left(arr, 0))
    print('bisect_left_after_last', bisect.bisect_left(arr, 9))
    print('bisect_left_duplicate', bisect.bisect_left([1, 2, 2, 2, 3], 2))

    # Edge cases
    print('bisect_left_empty', bisect.bisect_left([], 1))
    print('bisect_left_single_match', bisect.bisect_left([5], 5))
    print('bisect_left_single_less', bisect.bisect_left([5], 3))
    print('bisect_left_single_greater', bisect.bisect_left([5], 7))

    # With lo and hi parameters
    arr2 = [1, 3, 5, 7, 9, 11, 13]
    print('bisect_left_with_lo', bisect.bisect_left(arr2, 7, lo=2))
    print('bisect_left_with_hi', bisect.bisect_left(arr2, 7, hi=5))
    print('bisect_left_with_lo_hi', bisect.bisect_left(arr2, 7, lo=2, hi=5))
    print('bisect_left_lo_equals_hi', bisect.bisect_left(arr2, 7, lo=3, hi=3))
    print('bisect_left_lo_at_end', bisect.bisect_left(arr2, 15, lo=7))

    # With key parameter
    arr3 = ['apple', 'Banana', 'cherry', 'date']
    print('bisect_left_key_lower', bisect.bisect_left(arr3, 'banana', key=str.lower))
    print('bisect_left_key_len', bisect.bisect_left(['a', 'bb', 'ccc'], 2, key=len))
except Exception as e:
    print('SKIP_bisect_left', type(e).__name__, e)

# === bisect_right ===
try:
    arr = [1, 3, 5, 7]
    print('bisect_right_existing', bisect.bisect_right(arr, 3))
    print('bisect_right_not_existing', bisect.bisect_right(arr, 4))
    print('bisect_right_before_first', bisect.bisect_right(arr, 0))
    print('bisect_right_after_last', bisect.bisect_right(arr, 9))
    print('bisect_right_duplicate', bisect.bisect_right([1, 2, 2, 2, 3], 2))

    # Edge cases
    print('bisect_right_empty', bisect.bisect_right([], 1))
    print('bisect_right_single_match', bisect.bisect_right([5], 5))
    print('bisect_right_single_less', bisect.bisect_right([5], 3))
    print('bisect_right_single_greater', bisect.bisect_right([5], 7))

    # With lo and hi parameters
    arr2 = [1, 3, 5, 7, 9, 11, 13]
    print('bisect_right_with_lo', bisect.bisect_right(arr2, 7, lo=2))
    print('bisect_right_with_hi', bisect.bisect_right(arr2, 7, hi=5))
    print('bisect_right_with_lo_hi', bisect.bisect_right(arr2, 7, lo=2, hi=5))
    print('bisect_right_lo_equals_hi', bisect.bisect_right(arr2, 7, lo=3, hi=3))
    print('bisect_right_lo_at_end', bisect.bisect_right(arr2, 15, lo=7))

    # With key parameter
    arr3 = ['apple', 'Banana', 'cherry', 'date']
    print('bisect_right_key_lower', bisect.bisect_right(arr3, 'banana', key=str.lower))
    print('bisect_right_key_len', bisect.bisect_right(['a', 'bb', 'ccc'], 2, key=len))
except Exception as e:
    print('SKIP_bisect_right', type(e).__name__, e)

# === bisect (alias for bisect_right) ===
try:
    arr = [1, 3, 5, 7]
    print('bisect_alias_existing', bisect.bisect(arr, 3))
    print('bisect_alias_not_existing', bisect.bisect(arr, 4))
    print('bisect_alias_duplicate', bisect.bisect([1, 2, 2, 2, 3], 2))
    print('bisect_alias_empty', bisect.bisect([], 1))

    # Verify alias behavior matches bisect_right
    arr_dup = [1, 2, 2, 2, 3]
    print('bisect_vs_bisect_right_same', bisect.bisect(arr_dup, 2) == bisect.bisect_right(arr_dup, 2))
except Exception as e:
    print('SKIP_bisect', type(e).__name__, e)

# === insort_left ===
try:
    # Normal insertion
    arr = [1, 3, 5, 7]
    bisect.insort_left(arr, 4)
    print('insort_left_result', arr)

    # Insert at beginning
    arr = [2, 3, 4]
    bisect.insort_left(arr, 1)
    print('insort_left_beginning', arr)

    # Insert at end
    arr = [1, 2, 3]
    bisect.insort_left(arr, 4)
    print('insort_left_end', arr)

    # Insert duplicate (goes to left of existing)
    arr = [1, 2, 2, 2, 3]
    bisect.insort_left(arr, 2)
    print('insort_left_duplicate', arr)

    # Insert into empty
    arr = []
    bisect.insort_left(arr, 5)
    print('insort_left_empty', arr)

    # Insert single
    arr = [10]
    bisect.insort_left(arr, 5)
    print('insort_left_single_less', arr)

    arr = [10]
    bisect.insort_left(arr, 15)
    print('insort_left_single_greater', arr)

    # With lo and hi parameters
    arr = [1, 3, 5, 7, 9, 11]
    bisect.insort_left(arr, 6, lo=2, hi=4)
    print('insort_left_lo_hi', arr)

    # With key parameter
    arr = ['apple', 'Banana', 'cherry']
    bisect.insort_left(arr, 'apricot', key=str.lower)
    print('insort_left_key', arr)
except Exception as e:
    print('SKIP_insort_left', type(e).__name__, e)

# === insort_right ===
try:
    # Normal insertion
    arr = [1, 3, 5, 7]
    bisect.insort_right(arr, 4)
    print('insort_right_result', arr)

    # Insert at beginning
    arr = [2, 3, 4]
    bisect.insort_right(arr, 1)
    print('insort_right_beginning', arr)

    # Insert at end
    arr = [1, 2, 3]
    bisect.insort_right(arr, 4)
    print('insort_right_end', arr)

    # Insert duplicate (goes to right of existing)
    arr = [1, 2, 2, 2, 3]
    bisect.insort_right(arr, 2)
    print('insort_right_duplicate', arr)

    # Insert into empty
    arr = []
    bisect.insort_right(arr, 5)
    print('insort_right_empty', arr)

    # Insert single
    arr = [10]
    bisect.insort_right(arr, 5)
    print('insort_right_single_less', arr)

    arr = [10]
    bisect.insort_right(arr, 15)
    print('insort_right_single_greater', arr)

    # With lo and hi parameters
    arr = [1, 3, 5, 7, 9, 11]
    bisect.insort_right(arr, 6, lo=2, hi=4)
    print('insort_right_lo_hi', arr)

    # With key parameter
    arr = ['apple', 'Banana', 'cherry']
    bisect.insort_right(arr, 'apricot', key=str.lower)
    print('insort_right_key', arr)
except Exception as e:
    print('SKIP_insort_right', type(e).__name__, e)

# === insort (alias for insort_right) ===
try:
    arr = [1, 2, 2, 2, 3]
    bisect.insort(arr, 2)
    print('insort_alias_duplicate', arr)

    # Verify alias behavior matches insort_right
    arr1 = [1, 2, 2, 2, 3]
    arr2 = [1, 2, 2, 2, 3]
    bisect.insort(arr1, 2)
    bisect.insort_right(arr2, 2)
    print('insort_vs_insort_right_same', arr1 == arr2)
except Exception as e:
    print('SKIP_insort', type(e).__name__, e)

# === Additional edge cases ===
try:
    # Large lists
    large_arr = list(range(0, 1000, 2))
    print('bisect_left_large', bisect.bisect_left(large_arr, 500))
    print('bisect_right_large', bisect.bisect_right(large_arr, 500))

    # Negative numbers
    neg_arr = [-10, -5, 0, 5, 10]
    print('bisect_left_negative', bisect.bisect_left(neg_arr, -7))
    print('bisect_right_negative', bisect.bisect_right(neg_arr, -7))

    # Floats
    float_arr = [1.1, 2.2, 3.3, 4.4]
    print('bisect_left_float', bisect.bisect_left(float_arr, 2.5))
    print('bisect_right_float', bisect.bisect_right(float_arr, 2.5))

    # Tuples (lexicographic ordering)
    tuple_arr = [(1, 'a'), (2, 'b'), (3, 'c')]
    print('bisect_left_tuple', bisect.bisect_left(tuple_arr, (2, 'a')))
    print('bisect_right_tuple', bisect.bisect_right(tuple_arr, (2, 'b')))

    # Tuples with insort
    tuple_arr = [(1, 'a'), (3, 'c')]
    bisect.insort_left(tuple_arr, (2, 'b'))
    print('insort_left_tuple', tuple_arr)

    # Mixed comparisons with key
    class Item:
        def __init__(self, name, value):
            self.name = name
            self.value = value
        def __repr__(self):
            return f'Item({self.name!r}, {self.value})'

    items = [Item('a', 1), Item('b', 3), Item('c', 5)]
    bisect.insort_left(items, Item('d', 4), key=lambda x: x.value)
    print('insort_left_custom_key', [i.value for i in items])

    # Verify bisect_left vs bisect_right on same value
    arr = [1, 2, 2, 2, 3, 3, 4]
    print('left_vs_right_on_dup_left', bisect.bisect_left(arr, 2))
    print('left_vs_right_on_dup_right', bisect.bisect_right(arr, 2))
    print('left_vs_right_on_single', bisect.bisect_left(arr, 4), bisect.bisect_right(arr, 4))

    # Boundary conditions with lo/hi
    arr = [1, 2, 3, 4, 5]
    print('bisect_lo_zero', bisect.bisect_left(arr, 3, lo=0))
    print('bisect_hi_len', bisect.bisect_left(arr, 3, hi=5))
    print('bisect_lo_one_less', bisect.bisect_left(arr, 1, lo=1))
    print('bisect_hi_one_past', bisect.bisect_left(arr, 5, hi=4))

    # Verify all functions are callable
    print('callable_bisect_left', callable(bisect.bisect_left))
    print('callable_bisect_right', callable(bisect.bisect_right))
    print('callable_bisect', callable(bisect.bisect))
    print('callable_insort_left', callable(bisect.insort_left))
    print('callable_insort_right', callable(bisect.insort_right))
    print('callable_insort', callable(bisect.insort))
except Exception as e:
    print('SKIP_Additional_edge_cases', type(e).__name__, e)
