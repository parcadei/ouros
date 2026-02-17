#!/usr/bin/env python3
"""Parity tests for Python's copy module."""

import copy

# === Shallow copy basics ===
try:
    # Test copy.copy() on lists
    original_list = [1, 2, 3]
    shallow_list = copy.copy(original_list)
    print('shallow_copy_list', shallow_list)
    print('shallow_list_is_new', shallow_list is not original_list)
    print('shallow_list_equal', shallow_list == original_list)

    # Test copy.copy() on dicts
    original_dict = {'a': 1, 'b': 2}
    shallow_dict = copy.copy(original_dict)
    print('shallow_copy_dict', shallow_dict)
    print('shallow_dict_is_new', shallow_dict is not original_dict)
    print('shallow_dict_equal', shallow_dict == original_dict)

    # Test copy.copy() on sets
    original_set = {1, 2, 3}
    shallow_set = copy.copy(original_set)
    print('shallow_copy_set', sorted(shallow_set))
    print('shallow_set_is_new', shallow_set is not original_set)
    print('shallow_set_equal', shallow_set == original_set)

    # Test copy.copy() on tuples
    original_tuple = (1, 2, 3)
    shallow_tuple = copy.copy(original_tuple)
    print('shallow_copy_tuple', shallow_tuple)
    print('shallow_tuple_is_same', shallow_tuple is original_tuple)
except Exception as e:
    print('SKIP_Shallow copy basics', type(e).__name__, e)

# === Shallow copy behavior (nested objects shared) ===
try:
    nested_list = [[1, 2], [3, 4]]
    shallow_nested = copy.copy(nested_list)
    print('shallow_nested_outer_is_new', shallow_nested is not nested_list)
    print('shallow_nested_inner_shared', shallow_nested[0] is nested_list[0])
    print('shallow_nested_inner_shared_both', shallow_nested[1] is nested_list[1])

    # Modify original to verify shared reference
    nested_list[0].append(999)
    print('shallow_nested_modified', shallow_nested[0])
    nested_list[0].pop()
except Exception as e:
    print('SKIP_Shallow copy behavior (nested objects shared)', type(e).__name__, e)

# === Deep copy basics ===
try:
    # Test copy.deepcopy() on lists
    deep_original_list = [1, 2, 3]
    deep_list = copy.deepcopy(deep_original_list)
    print('deep_copy_list', deep_list)
    print('deep_list_is_new', deep_list is not deep_original_list)
    print('deep_list_equal', deep_list == deep_original_list)

    # Test copy.deepcopy() on dicts
    deep_original_dict = {'a': 1, 'b': 2}
    deep_dict = copy.deepcopy(deep_original_dict)
    print('deep_copy_dict', deep_dict)
    print('deep_dict_is_new', deep_dict is not deep_original_dict)
    print('deep_dict_equal', deep_dict == deep_original_dict)

    # Test copy.deepcopy() on sets
    deep_original_set = {1, 2, 3}
    deep_set = copy.deepcopy(deep_original_set)
    print('deep_copy_set', sorted(deep_set))
    print('deep_set_is_new', deep_set is not deep_original_set)
    print('deep_set_equal', deep_set == deep_original_set)

    # Test copy.deepcopy() on tuples
    deep_original_tuple = (1, 2, 3)
    deep_tuple = copy.deepcopy(deep_original_tuple)
    print('deep_copy_tuple', deep_tuple)
    print('deep_tuple_equal', deep_tuple == deep_original_tuple)
except Exception as e:
    print('SKIP_Deep copy basics', type(e).__name__, e)

# === Nested structures ===
try:
    # Deep copy of nested lists
    nested_list2 = [[1, 2], [3, 4]]
    deep_nested = copy.deepcopy(nested_list2)
    print('deep_nested_outer_is_new', deep_nested is not nested_list2)
    print('deep_nested_inner_is_new', deep_nested[0] is not nested_list2[0])
    print('deep_nested_inner2_is_new', deep_nested[1] is not nested_list2[1])
    print('deep_nested_equal', deep_nested == nested_list2)

    # Modify original to verify independence
    nested_list2[0].append(999)
    print('deep_nested_stays_same', deep_nested[0])
    nested_list2[0].pop()

    # Deep copy of dicts with nested lists
    nested_dict = {'items': [1, 2, 3], 'meta': {'count': 3}}
    deep_nested_dict = copy.deepcopy(nested_dict)
    print('deep_nested_dict_outer_is_new', deep_nested_dict is not nested_dict)
    print('deep_nested_dict_list_is_new', deep_nested_dict['items'] is not nested_dict['items'])
    print('deep_nested_dict_meta_is_new', deep_nested_dict['meta'] is not nested_dict['meta'])
    print('deep_nested_dict_equal', deep_nested_dict == nested_dict)
except Exception as e:
    print('SKIP_Nested structures', type(e).__name__, e)

# === Circular references ===
try:
    # Create a list that references itself
    a = []
    a.append(a)
    print('circular_self_ref', a[0] is a)

    # Deep copy it
    circular_copy = copy.deepcopy(a)
    print('circular_copy_exists', circular_copy is not None)
    print('circular_copy_is_new', circular_copy is not a)
    print('circular_copy_maintains_ref', circular_copy[0] is circular_copy)

    # More complex circular reference
    b = [1, 2]
    c = [3, b]
    b.append(c)
    print('complex_circular', b[2][1] is b)

    complex_copy = copy.deepcopy(b)
    print('complex_copy_is_new', complex_copy is not b)
    print('complex_copy_nested_new', complex_copy[2] is not b[2])
    print('complex_copy_maintains_ref', complex_copy[2][1] is complex_copy)
except Exception as e:
    print('SKIP_Circular references', type(e).__name__, e)

# === Immutable types ===
try:
    # copy.copy and copy.deepcopy on int
    num = 42
    copy_int = copy.copy(num)
    deep_int = copy.deepcopy(num)
    print('copy_int_value', copy_int)
    print('deepcopy_int_value', deep_int)
    print('copy_int_is_same', copy_int is num)
    print('deepcopy_int_is_same', deep_int is num)

    # copy.copy and copy.deepcopy on str
    text = "hello"
    copy_str = copy.copy(text)
    deep_str = copy.deepcopy(text)
    print('copy_str_value', copy_str)
    print('deepcopy_str_value', deep_str)
    print('copy_str_is_same', copy_str is text)
    print('deepcopy_str_is_same', deep_str is text)

    # copy.copy and copy.deepcopy on tuple of immutables
    tuple_imm = (1, "two", 3.0)
    copy_tuple_imm = copy.copy(tuple_imm)
    deep_tuple_imm = copy.deepcopy(tuple_imm)
    print('copy_tuple_imm_value', copy_tuple_imm)
    print('deepcopy_tuple_imm_value', deep_tuple_imm)
    print('copy_tuple_imm_is_same', copy_tuple_imm is tuple_imm)
    print('deepcopy_tuple_imm_is_same', deep_tuple_imm is tuple_imm)

    # bytes
    byte_data = b"binary data"
    copy_bytes = copy.copy(byte_data)
    deep_bytes = copy.deepcopy(byte_data)
    print('copy_bytes_value', copy_bytes)
    print('deepcopy_bytes_value', deep_bytes)
    print('copy_bytes_is_same', copy_bytes is byte_data)
    print('deepcopy_bytes_is_same', deep_bytes is byte_data)

    # frozenset
    frozenset_data = frozenset([1, 2, 3])
    copy_frozenset = copy.copy(frozenset_data)
    deep_frozenset = copy.deepcopy(frozenset_data)
    print('copy_frozenset_value', sorted(copy_frozenset))
    print('deepcopy_frozenset_value', sorted(deep_frozenset))
    print('copy_frozenset_is_same', copy_frozenset is frozenset_data)
    print('deepcopy_frozenset_is_same', deep_frozenset is frozenset_data)
except Exception as e:
    print('SKIP_Immutable types', type(e).__name__, e)

# === Memo parameter ===
try:
    # Test deepcopy with explicit memo dict
    memo = {}
    memo_list = [[1, 2], [3, 4]]
    deep_memo = copy.deepcopy(memo_list, memo)
    print('deep_memo_result', deep_memo)
    print('deep_memo_is_new', deep_memo is not memo_list)
    print('deep_memo_inner_is_new', deep_memo[0] is not memo_list[0])
    print('memo_is_populated', len(memo) > 0)

    # Test that memo is used to track objects
    memo2 = {}
    shared = [1, 2]
    memo_test = [shared, shared]
    deep_memo2 = copy.deepcopy(memo_test, memo2)
    print('deep_memo_shared_equal', deep_memo2[0] == deep_memo2[1])
    print('deep_memo_shared_same', deep_memo2[0] is deep_memo2[1])
except Exception as e:
    print('SKIP_Memo parameter', type(e).__name__, e)

# === Custom classes ===
try:
    # Class with __copy__ method
    class Copyable:
        def __init__(self, value):
            self.value = value
            self.copy_called = False

        def __copy__(self):
            new_obj = Copyable(self.value)
            new_obj.copy_called = True
            return new_obj

    copyable = Copyable(42)
    copyable_shallow = copy.copy(copyable)
    print('copyable_shallow_value', copyable_shallow.value)
    print('copyable_shallow_is_new', copyable_shallow is not copyable)
    print('copyable_shallow_copy_called', copyable_shallow.copy_called)

    # Class with __deepcopy__ method
    class DeepCopyable:
        def __init__(self, value, nested):
            self.value = value
            self.nested = nested
            self.deepcopy_called = False

        def __deepcopy__(self, memo):
            new_obj = DeepCopyable(
                copy.deepcopy(self.value, memo),
                copy.deepcopy(self.nested, memo)
            )
            new_obj.deepcopy_called = True
            return new_obj

    deep_copyable = DeepCopyable(42, [1, 2, 3])
    deep_copyable_copy = copy.deepcopy(deep_copyable)
    print('deepcopyable_value', deep_copyable_copy.value)
    print('deepcopyable_nested', deep_copyable_copy.nested)
    print('deepcopyable_is_new', deep_copyable_copy is not deep_copyable)
    print('deepcopyable_nested_is_new', deep_copyable_copy.nested is not deep_copyable.nested)
    print('deepcopyable_deepcopy_called', deep_copyable_copy.deepcopy_called)

    # Class without custom copy methods (default behavior)
    class PlainClass:
        def __init__(self, x):
            self.x = x

    plain = PlainClass([1, 2, 3])
    plain_shallow = copy.copy(plain)
    plain_deep = copy.deepcopy(plain)
    print('plain_shallow_x', plain_shallow.x)
    print('plain_shallow_is_new', plain_shallow is not plain)
    print('plain_shallow_x_shared', plain_shallow.x is plain.x)
    print('plain_deep_x', plain_deep.x)
    print('plain_deep_is_new', plain_deep is not plain)
    print('plain_deep_x_is_new', plain_deep.x is not plain.x)
except Exception as e:
    print('SKIP_Custom classes', type(e).__name__, e)

# === Error handling ===
try:
    # Check that copy.Error exists
    print('copy_error_exists', hasattr(copy, 'Error'))
    print('copy_error_is_exception', issubclass(copy.Error, Exception))

    # Check that copy.error exists (alias for Error)
    print('copy_error_alias_exists', hasattr(copy, 'error'))
    print('copy_error_is_copy_error', copy.error is copy.Error)
except Exception as e:
    print('SKIP_Error handling', type(e).__name__, e)

# === Empty containers ===
try:
    print('shallow_empty_list', copy.copy([]))
    print('shallow_empty_dict', copy.copy({}))
    print('shallow_empty_set', copy.copy(set()))
    print('shallow_empty_tuple', copy.copy(()))
    print('deep_empty_list', copy.deepcopy([]))
    print('deep_empty_dict', copy.deepcopy({}))
    print('deep_empty_set', copy.deepcopy(set()))
    print('deep_empty_tuple', copy.deepcopy(()))
except Exception as e:
    print('SKIP_Empty containers', type(e).__name__, e)

# === copy.replace() - new in Python 3.13 ===
try:
    # Test replace on simple objects that support it
    from dataclasses import dataclass

    @dataclass
    class Point:
        x: int
        y: int

    p = Point(1, 2)
    p_replaced = copy.replace(p, x=10)
    print('replace_x', p_replaced.x)
    print('replace_y', p_replaced.y)
    print('replace_original_unchanged', p.x)

    # Test replace on namedtuple
    from collections import namedtuple
    NTPoint = namedtuple('NTPoint', ['x', 'y'])
    ntp = NTPoint(1, 2)
    ntp_replaced = copy.replace(ntp, x=100)
    print('replace_namedtuple_x', ntp_replaced.x)
    print('replace_namedtuple_y', ntp_replaced.y)
except Exception as e:
    print('SKIP_copy.replace() - new in Python 3.13', type(e).__name__, e)

# === copy.dispatch_table ===
try:
    # Check that dispatch_table exists
    print('dispatch_table_exists', hasattr(copy, 'dispatch_table'))
    print('dispatch_table_is_dict', type(copy.dispatch_table) is dict)
except Exception as e:
    print('SKIP_copy.dispatch_table', type(e).__name__, e)
