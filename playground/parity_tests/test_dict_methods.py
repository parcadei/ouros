# Comprehensive dict methods parity test file
# Tests all 11 public dict methods: clear, copy, fromkeys, get, items, keys, pop, popitem, setdefault, update, values

# === clear ===
try:
    # Basic clear
    d = {'a': 1, 'b': 2}
    d.clear()
    print('clear_basic', d)

    # Clear empty dict
    d = {}
    d.clear()
    print('clear_empty', d)
except Exception as e:
    print('SKIP_clear', type(e).__name__, e)

# === copy ===
try:
    # Basic shallow copy
    d = {'a': 1, 'b': 2}
    c = d.copy()
    print('copy_basic', c)
    print('copy_is_distinct', c is not d)

    # Copy doesn't share items (shallow copy)
    d = {'a': [1, 2], 'b': [3, 4]}
    c = d.copy()
    c['a'].append(5)
    print('copy_shallow', d['a'])

    # Copy empty dict
    d = {}
    c = d.copy()
    print('copy_empty', c)
except Exception as e:
    print('SKIP_copy', type(e).__name__, e)

# === fromkeys ===
try:
    # Basic fromkeys with list
    keys = ['a', 'b', 'c']
    d = dict.fromkeys(keys)
    print('fromkeys_list', d)

    # fromkeys with default value
    keys = ['a', 'b', 'c']
    d = dict.fromkeys(keys, 0)
    print('fromkeys_default', d)

    # fromkeys with mutable default (shared reference)
    keys = ['a', 'b']
    d = dict.fromkeys(keys, [])
    d['a'].append(1)
    print('fromkeys_mutable_shared', d)

    # fromkeys with tuple
    keys = ('x', 'y')
    d = dict.fromkeys(keys, 1)
    print('fromkeys_tuple', d)

    # fromkeys with empty iterable
    d = dict.fromkeys([], 1)
    print('fromkeys_empty', d)

    # fromkeys with string (iterable of chars)
    d = dict.fromkeys('ab', 0)
    print('fromkeys_string', d)
except Exception as e:
    print('SKIP_fromkeys', type(e).__name__, e)

# === get ===
try:
    # Basic get
    d = {'a': 1, 'b': 2}
    print('get_basic', d.get('a'))

    # Get non-existent key (no default)
    d = {'a': 1}
    print('get_missing_no_default', d.get('b'))

    # Get with default
    d = {'a': 1}
    print('get_with_default', d.get('b', 'default'))

    # Get existing key with default ignored
    d = {'a': 1}
    print('get_existing_with_default', d.get('a', 'default'))

    # Get with None as value
    d = {'a': None}
    print('get_none_value', d.get('a', 'default'))

    # Get with falsy values
    d = {'a': 0, 'b': False, 'c': ''}
    print('get_falsy_zero', d.get('a', 'default'))
    print('get_falsy_false', d.get('b', 'default'))
    print('get_falsy_empty', d.get('c', 'default'))
except Exception as e:
    print('SKIP_get', type(e).__name__, e)

# === items ===
try:
    # Basic items
    d = {'a': 1, 'b': 2}
    items = d.items()
    print('items_type', type(items).__name__)
    print('items_basic', list(items))

    # Items is a view (live update)
    d = {'a': 1}
    items = d.items()
    d['b'] = 2
    print('items_view_live', list(items))

    # Items view reflects deletions
    d = {'a': 1, 'b': 2}
    items = d.items()
    del d['a']
    print('items_view_delete', list(items))

    # Items contains
    d = {'a': 1, 'b': 2}
    items = d.items()
    print('items_contains_true', ('a', 1) in items)
    print('items_contains_false', ('a', 2) in items)

    # Items is set-like
    d = {'a': 1, 'b': 2}
    print('items_len', len(d.items()))

    # Empty dict items
    d = {}
    print('items_empty', list(d.items()))
except Exception as e:
    print('SKIP_items', type(e).__name__, e)

# === keys ===
try:
    # Basic keys
    d = {'a': 1, 'b': 2}
    keys = d.keys()
    print('keys_type', type(keys).__name__)
    print('keys_basic', list(keys))

    # Keys is a view (live update)
    d = {'a': 1}
    keys = d.keys()
    d['b'] = 2
    print('keys_view_live', list(keys))

    # Keys view reflects deletions
    d = {'a': 1, 'b': 2}
    keys = d.keys()
    del d['a']
    print('keys_view_delete', list(keys))

    # Keys contains
    d = {'a': 1, 'b': 2}
    keys = d.keys()
    print('keys_contains_true', 'a' in keys)
    print('keys_contains_false', 'c' in keys)

    # Keys is set-like (supports set operations in Python 3)
    d = {'a': 1, 'b': 2}
    keys = d.keys()
    print('keys_set_ops', keys | {'c'})

    # Empty dict keys
    d = {}
    print('keys_empty', list(d.keys()))
except Exception as e:
    print('SKIP_keys', type(e).__name__, e)

# === values ===
try:
    # Basic values
    d = {'a': 1, 'b': 2}
    values = d.values()
    print('values_type', type(values).__name__)
    print('values_basic', list(values))

    # Values is a view (live update)
    d = {'a': 1}
    values = d.values()
    d['b'] = 2
    print('values_view_live', list(values))

    # Values view reflects updates
    d = {'a': 1, 'b': 2}
    values = d.values()
    d['a'] = 10
    print('values_view_update', list(values))

    # Values view reflects deletions
    d = {'a': 1, 'b': 2}
    values = d.values()
    del d['a']
    print('values_view_delete', list(values))

    # Values len
    d = {'a': 1, 'b': 2}
    print('values_len', len(d.values()))

    # Empty dict values
    d = {}
    print('values_empty', list(d.values()))

    # Values are not unique
    d = {'a': 1, 'b': 1}
    print('values_duplicates', list(d.values()))
except Exception as e:
    print('SKIP_values', type(e).__name__, e)

# === pop ===
try:
    # Basic pop
    d = {'a': 1, 'b': 2}
    v = d.pop('a')
    print('pop_basic_value', v)
    print('pop_basic_dict', d)

    # Pop with default
    d = {'a': 1}
    v = d.pop('b', 'default')
    print('pop_with_default', v)
    print('pop_with_default_dict', d)

    # Pop without default raises KeyError
    d = {'a': 1}
    try:
        d.pop('b')
    except KeyError as e:
        print('pop_keyerror', str(e))

    # Pop only remaining item
    d = {'a': 1}
    v = d.pop('a')
    print('pop_last_value', v)
    print('pop_last_dict', d)
except Exception as e:
    print('SKIP_pop', type(e).__name__, e)

# === popitem ===
try:
    # Basic popitem (LIFO order since Python 3.7)
    d = {'a': 1, 'b': 2}
    item = d.popitem()
    print('popitem_basic', item)
    print('popitem_basic_dict', d)

    # Popitem multiple times
    d = {'a': 1, 'b': 2, 'c': 3}
    item1 = d.popitem()
    item2 = d.popitem()
    print('popitem_order_1', item1)
    print('popitem_order_2', item2)

    # Popitem empty dict raises KeyError
    d = {}
    try:
        d.popitem()
    except KeyError as e:
        print('popitem_keyerror', str(e))
except Exception as e:
    print('SKIP_popitem', type(e).__name__, e)

# === setdefault ===
try:
    # Basic setdefault with existing key
    d = {'a': 1}
    v = d.setdefault('a', 100)
    print('setdefault_existing_value', v)
    print('setdefault_existing_dict', d)

    # Setdefault with missing key
    d = {'a': 1}
    v = d.setdefault('b', 100)
    print('setdefault_missing_value', v)
    print('setdefault_missing_dict', d)

    # Setdefault with no default (uses None)
    d = {'a': 1}
    v = d.setdefault('b')
    print('setdefault_no_default_value', v)
    print('setdefault_no_default_dict', d)

    # Setdefault with mutable default
    d = {}
    v = d.setdefault('a', [])
    v.append(1)
    print('setdefault_mutable_value', v)
    print('setdefault_mutable_dict', d)
except Exception as e:
    print('SKIP_setdefault', type(e).__name__, e)

# === update ===
try:
    # Update with another dict
    d = {'a': 1, 'b': 2}
    d.update({'b': 20, 'c': 3})
    print('update_dict', d)

    # Update with keyword args
    d = {'a': 1}
    d.update(b=2, c=3)
    print('update_kwargs', d)

    # Update with dict and kwargs
    d = {'a': 1}
    d.update({'b': 2}, c=3)
    print('update_dict_and_kwargs', d)

    # Update with iterable of pairs
    d = {'a': 1}
    d.update([('b', 2), ('c', 3)])
    print('update_iterable', d)

    # Update empty dict
    d = {}
    d.update({'a': 1})
    print('update_empty', d)

    # Update with nothing
    d = {'a': 1}
    d.update()
    print('update_nothing', d)

    # Update overwrites existing
    d = {'a': 1}
    d.update({'a': 2})
    print('update_overwrite', d)

    # Update with generator
    d = {}
    d.update((c, ord(c)) for c in 'ab')
    print('update_generator', d)
except Exception as e:
    print('SKIP_update', type(e).__name__, e)

# === Additional edge cases ===
try:
    # Dict views set operations (Python 3+)
    d1 = {'a': 1, 'b': 2}
    d2 = {'b': 3, 'c': 4}
    print('keys_intersection', list(d1.keys() & d2.keys()))
    print('keys_union', list(d1.keys() | d2.keys()))
    print('keys_difference', list(d1.keys() - d2.keys()))

    # Items comparison
    d = {'a': 1, 'b': 2}
    print('items_equality', d.items() == {('a', 1), ('b', 2)})

    # Reversed on dict views (Python 3.8+)
    d = {'a': 1, 'b': 2}
    print('reversed_keys', list(reversed(d.keys())))
    print('reversed_items', list(reversed(d.items())))
except Exception as e:
    print('SKIP_Additional edge cases', type(e).__name__, e)
