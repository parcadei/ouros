# Comprehensive parity test for Python list methods
# Testing all 11 public list methods: append, clear, copy, count, extend, index, insert, pop, remove, reverse, sort

# === append ===
try:
    # Basic append
    lst = [1, 2]
    lst.append(3)
    print('append_basic', lst)

    # Append to empty list
    lst = []
    lst.append(1)
    print('append_empty', lst)

    # Append different types
    lst = [1, 2]
    lst.append('hello')
    lst.append([3, 4])
    lst.append(None)
    print('append_mixed_types', lst)

    # Append returns None
    lst = [1, 2]
    result = lst.append(3)
    print('append_returns_none', result)
except Exception as e:
    print('SKIP_append', type(e).__name__, e)

# === extend ===
try:
    # Basic extend with list
    lst = [1, 2]
    lst.extend([3, 4, 5])
    print('extend_list', lst)

    # Extend with empty list
    lst = [1, 2]
    lst.extend([])
    print('extend_empty_list', lst)

    # Extend with tuple
    lst = [1, 2]
    lst.extend((3, 4))
    print('extend_tuple', lst)

    # Extend with string (iterable of characters)
    lst = ['a', 'b']
    lst.extend('cd')
    print('extend_string', lst)

    # Extend with set
    lst = [1, 2]
    lst.extend({3, 4})
    print('extend_set', sorted(lst))  # sorted because set order is undefined

    # Extend returns None
    lst = [1, 2]
    result = lst.extend([3])
    print('extend_returns_none', result)
except Exception as e:
    print('SKIP_extend', type(e).__name__, e)

# === insert ===
try:
    # Insert at beginning
    lst = [2, 3]
    lst.insert(0, 1)
    print('insert_beginning', lst)

    # Insert at end (equivalent to append)
    lst = [1, 2]
    lst.insert(2, 3)
    print('insert_end', lst)

    # Insert in middle
    lst = [1, 3]
    lst.insert(1, 2)
    print('insert_middle', lst)

    # Insert with negative index
    lst = [1, 2, 4]
    lst.insert(-1, 3)
    print('insert_negative_index', lst)

    # Insert with large index (beyond end)
    lst = [1, 2]
    lst.insert(100, 3)
    print('insert_large_index', lst)

    # Insert returns None
    lst = [1, 2]
    result = lst.insert(0, 0)
    print('insert_returns_none', result)
except Exception as e:
    print('SKIP_insert', type(e).__name__, e)

# === remove ===
try:
    # Basic remove
    lst = [1, 2, 3, 2]
    lst.remove(2)
    print('remove_basic', lst)

    # Remove only first occurrence
    lst = [1, 2, 2, 3]
    lst.remove(2)
    print('remove_first_occurrence', lst)

    # Remove returns None
    lst = [1, 2, 3]
    result = lst.remove(2)
    print('remove_returns_none', result)
except Exception as e:
    print('SKIP_remove', type(e).__name__, e)

# === pop ===
try:
    # Pop without index (last element)
    lst = [1, 2, 3]
    result = lst.pop()
    print('pop_no_index', result, lst)

    # Pop with index
    lst = [1, 2, 3]
    result = lst.pop(1)
    print('pop_with_index', result, lst)

    # Pop first element
    lst = [1, 2, 3]
    result = lst.pop(0)
    print('pop_first', result, lst)

    # Pop with negative index
    lst = [1, 2, 3]
    result = lst.pop(-1)
    print('pop_negative_index', result, lst)

    # Pop from single element list
    lst = [42]
    result = lst.pop()
    print('pop_single_element', result, lst)
except Exception as e:
    print('SKIP_pop', type(e).__name__, e)

# === clear ===
try:
    # Basic clear
    lst = [1, 2, 3]
    lst.clear()
    print('clear_basic', lst)

    # Clear empty list
    lst = []
    lst.clear()
    print('clear_empty', lst)

    # Clear returns None
    lst = [1, 2, 3]
    result = lst.clear()
    print('clear_returns_none', result)
except Exception as e:
    print('SKIP_clear', type(e).__name__, e)

# === index ===
try:
    # Basic index
    lst = ['a', 'b', 'c']
    print('index_basic', lst.index('b'))

    # Index with start parameter
    lst = [1, 2, 3, 2, 4]
    print('index_with_start', lst.index(2, 2))

    # Index with start and end
    lst = [1, 2, 3, 2, 4]
    print('index_with_start_end', lst.index(2, 1, 3))

    # Index at position 0
    lst = [1, 2, 3]
    print('index_first', lst.index(1))

    # Index last element
    lst = [1, 2, 3]
    print('index_last', lst.index(3))
except Exception as e:
    print('SKIP_index', type(e).__name__, e)

# === count ===
try:
    # Basic count
    lst = [1, 2, 2, 3, 2, 4]
    print('count_basic', lst.count(2))

    # Count zero occurrences
    lst = [1, 2, 3]
    print('count_zero', lst.count(99))

    # Count all same elements
    lst = ['a', 'a', 'a']
    print('count_all_same', lst.count('a'))

    # Count in empty list
    lst = []
    print('count_empty', lst.count(1))

    # Count with mixed types (only exact matches)
    lst = [1, '1', 1.0, 1]
    print('count_mixed_types', lst.count(1))
except Exception as e:
    print('SKIP_count', type(e).__name__, e)

# === sort ===
try:
    # Basic sort
    lst = [3, 1, 4, 1, 5, 9, 2, 6]
    lst.sort()
    print('sort_basic', lst)

    # Sort reverse
    lst = [3, 1, 4, 1, 5, 9, 2, 6]
    lst.sort(reverse=True)
    print('sort_reverse', lst)

    # Sort with key (absolute value)
    lst = [-5, 3, -2, 4, -1]
    lst.sort(key=abs)
    print('sort_key_abs', lst)

    # Sort with key (length)
    lst = ['banana', 'pie', 'apple', 'strawberry']
    lst.sort(key=len)
    print('sort_key_len', lst)

    # Sort with key and reverse
    lst = ['banana', 'pie', 'apple', 'strawberry']
    lst.sort(key=len, reverse=True)
    print('sort_key_reverse', lst)

    # Sort strings (lexicographic)
    lst = ['cherry', 'apple', 'banana']
    lst.sort()
    print('sort_strings', lst)

    # Sort returns None
    lst = [3, 1, 2]
    result = lst.sort()
    print('sort_returns_none', result)

    # Sort already sorted
    lst = [1, 2, 3]
    lst.sort()
    print('sort_already_sorted', lst)

    # Sort reverse sorted
    lst = [3, 2, 1]
    lst.sort()
    print('sort_reverse_sorted', lst)
except Exception as e:
    print('SKIP_sort', type(e).__name__, e)

# === reverse ===
try:
    # Basic reverse
    lst = [1, 2, 3, 4, 5]
    lst.reverse()
    print('reverse_basic', lst)

    # Reverse empty list
    lst = []
    lst.reverse()
    print('reverse_empty', lst)

    # Reverse single element
    lst = [42]
    lst.reverse()
    print('reverse_single', lst)

    # Reverse two elements
    lst = [1, 2]
    lst.reverse()
    print('reverse_two', lst)

    # Reverse returns None
    lst = [1, 2, 3]
    result = lst.reverse()
    print('reverse_returns_none', result)

    # Double reverse returns original
    lst = [1, 2, 3]
    lst.reverse()
    lst.reverse()
    print('reverse_double', lst)
except Exception as e:
    print('SKIP_reverse', type(e).__name__, e)

# === copy ===
try:
    # Basic copy
    lst = [1, 2, 3]
    copied = lst.copy()
    print('copy_basic', copied)

    # Copy is shallow (independent list)
    lst = [1, 2, 3]
    copied = lst.copy()
    copied.append(4)
    print('copy_shallow_original', lst)
    print('copy_shallow_copied', copied)

    # Copy of empty list
    lst = []
    copied = lst.copy()
    print('copy_empty', copied)

    # Copy preserves nested structure (shallow)
    lst = [[1, 2], [3, 4]]
    copied = lst.copy()
    copied[0].append(99)
    print('copy_shallow_nested_original', lst)
    print('copy_shallow_nested_copied', copied)

    # Copy returns new list
    lst = [1, 2, 3]
    copied = lst.copy()
    print('copy_is_new_list', copied is not lst)
except Exception as e:
    print('SKIP_copy', type(e).__name__, e)
