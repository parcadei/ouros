import collections

# === Counter ===
try:
    # Basic creation from list
    counter_basic = collections.Counter(['a', 'b', 'a'])
    print('counter_basic', dict(counter_basic))

    # Creation from string (counts characters)
    counter_string = collections.Counter('abracadabra')
    print('counter_string', dict(counter_string))

    # Creation from dict
    counter_dict = collections.Counter({'red': 4, 'blue': 2})
    print('counter_dict', dict(counter_dict))

    # Creation with keyword args
    counter_kwargs = collections.Counter(cats=4, dogs=8)
    print('counter_kwargs', dict(counter_kwargs))

    # Empty counter
    counter_empty = collections.Counter()
    print('counter_empty', dict(counter_empty))

    # most_common method
    counter_mc = collections.Counter('abracadabra')
    print('counter_most_common_all', counter_mc.most_common())
    print('counter_most_common_n', counter_mc.most_common(3))
    print('counter_most_common_zero', counter_mc.most_common(0))

    # elements method
    counter_elem = collections.Counter(a=4, b=2, c=0, d=-2)
    print('counter_elements', sorted(counter_elem.elements()))

    # subtract method
    counter_sub = collections.Counter(a=4, b=2, c=0)
    counter_sub.subtract({'a': 1, 'b': 2, 'c': 3, 'd': 4})
    print('counter_subtract', dict(counter_sub))

    # subtract with iterable
    counter_sub2 = collections.Counter('aaabbc')
    counter_sub2.subtract('aabb')
    print('counter_subtract_iter', dict(counter_sub2))

    # update method
    counter_upd = collections.Counter('aaabbc')
    counter_upd.update('bccdd')
    print('counter_update', dict(counter_upd))

    # update with dict
    counter_upd2 = collections.Counter(a=3, b=1)
    counter_upd2.update({'a': 1, 'b': 2, 'c': 3})
    print('counter_update_dict', dict(counter_upd2))

    # update with keyword args
    counter_upd3 = collections.Counter(a=1)
    counter_upd3.update(a=2, b=3)
    print('counter_update_kwargs', dict(counter_upd3))

    # Arithmetic operations
    c1 = collections.Counter(a=3, b=1)
    c2 = collections.Counter(a=1, b=2)
    print('counter_add', dict(c1 + c2))
    print('counter_sub', dict(c1 - c2))
    print('counter_and', dict(c1 & c2))
    print('counter_or', dict(c1 | c2))

    # Negative values in arithmetic
    c3 = collections.Counter(a=5, b=3, c=-1)
    c4 = collections.Counter(a=3, b=5, d=-1)
    print('counter_neg_sub', dict(c3 - c4))
    print('counter_neg_and', dict(c3 & c4))
    print('counter_neg_or', dict(c3 | c4))

    # Unary plus and minus
    c5 = collections.Counter(a=2, b=-3, c=0)
    print('counter_pos', dict(+c5))
    print('counter_neg', dict(-c5))

    # In-place operations
    c6 = collections.Counter(a=3, b=1)
    c6 += collections.Counter(a=1, b=2)
    print('counter_iadd', dict(c6))

    c7 = collections.Counter(a=5, b=3)
    c7 -= collections.Counter(a=1, b=4)
    print('counter_isub', dict(c7))

    c8 = collections.Counter(a=3, b=2)
    c8 &= collections.Counter(a=2, b=3)
    print('counter_iand', dict(c8))

    c9 = collections.Counter(a=3, b=2)
    c9 |= collections.Counter(a=2, b=3)
    print('counter_ior', dict(c9))

    # Counter with missing key returns 0
    counter_miss = collections.Counter(a=1)
    print('counter_missing', counter_miss['missing_key'])

    # del item
    counter_del = collections.Counter(a=1, b=2)
    del counter_del['a']
    print('counter_del', dict(counter_del))

    # total() method (Python 3.10+)
    counter_total = collections.Counter(a=2, b=3, c=1)
    try:
        print('counter_total', counter_total.total())
    except AttributeError:
        print('counter_total', sum(counter_total.values()))
except Exception as e:
    print('SKIP_Counter', type(e).__name__, e)

# === OrderedDict ===
try:
    # Basic creation
    od_basic = collections.OrderedDict([('a', 1), ('b', 2), ('c', 3)])
    print('od_basic', list(od_basic.items()))

    # Creation from kwargs
    od_kwargs = collections.OrderedDict(x=1, y=2, z=3)
    print('od_kwargs', list(od_kwargs.items()))

    # Creation from dict (preserves insertion order in modern Python)
    od_from_dict = collections.OrderedDict({'a': 1, 'b': 2})
    print('od_from_dict', list(od_from_dict.items()))

    # move_to_end
    od_move = collections.OrderedDict([('a', 1), ('b', 2), ('c', 3)])
    od_move.move_to_end('a')
    print('od_move_to_end', list(od_move.items()))

    # move_to_end with last=False
    od_move2 = collections.OrderedDict([('a', 1), ('b', 2), ('c', 3)])
    od_move2.move_to_end('c', last=False)
    print('od_move_to_end_last_false', list(od_move2.items()))

    # popitem (LIFO by default)
    od_pop = collections.OrderedDict([('a', 1), ('b', 2), ('c', 3)])
    item = od_pop.popitem()
    print('od_popitem', item, list(od_pop.items()))

    # popitem with last=False (FIFO)
    od_pop2 = collections.OrderedDict([('a', 1), ('b', 2), ('c', 3)])
    item2 = od_pop2.popitem(last=False)
    print('od_popitem_fifo', item2, list(od_pop2.items()))

    # reversed (Python 3.8+)
    od_rev = collections.OrderedDict([('a', 1), ('b', 2), ('c', 3)])
    try:
        print('od_reversed', list(reversed(od_rev)))
    except TypeError:
        print('od_reversed', list(od_rev.keys())[::-1])

    # equality with regular dict
    od_eq = collections.OrderedDict([('a', 1), ('b', 2)])
    d_eq = {'a': 1, 'b': 2}
    print('od_eq_dict', od_eq == d_eq)
except Exception as e:
    print('SKIP_OrderedDict', type(e).__name__, e)

# === defaultdict ===
try:
    # Basic creation with int factory
    dd_int = collections.defaultdict(int)
    dd_int['a'] += 1
    dd_int['b'] += 2
    print('dd_int', dict(dd_int))

    # Creation with list factory
    dd_list = collections.defaultdict(list)
    dd_list['fruits'].append('apple')
    dd_list['fruits'].append('banana')
    dd_list['veggies'].append('carrot')
    print('dd_list', dict(dd_list))

    # Creation with set factory
    dd_set = collections.defaultdict(set)
    dd_set['tags'].add('python')
    dd_set['tags'].add('coding')
    dd_set['tags'].add('python')  # duplicate
    print('dd_set', dict(dd_set))

    # Creation with lambda factory
    dd_lambda = collections.defaultdict(lambda: 'default_value')
    print('dd_lambda_missing', dd_lambda['nonexistent'])
    dd_lambda['existing'] = 'real_value'
    print('dd_lambda', dict(dd_lambda))

    # Custom factory function
    def factory_func():
        return {'count': 0, 'items': []}

    dd_custom = collections.defaultdict(factory_func)
    dd_custom['key1']['count'] += 1
    print('dd_custom', dict(dd_custom))

    # __missing__ behavior
    dd_miss = collections.defaultdict(int)
    print('dd_missing_val', dd_miss['missing'])
    print('dd_missing_added', dict(dd_miss))
except Exception as e:
    print('SKIP_defaultdict', type(e).__name__, e)

# === deque ===
try:
    # Basic creation
    dq_basic = collections.deque([1, 2, 3])
    print('dq_basic', list(dq_basic))

    # Empty deque
    dq_empty = collections.deque()
    print('dq_empty', list(dq_empty))

    # Creation with maxlen
    dq_max = collections.deque([1, 2, 3, 4, 5], maxlen=3)
    print('dq_maxlen', list(dq_max))

    # append
    dq_append = collections.deque([1, 2])
    dq_append.append(3)
    print('dq_append', list(dq_append))

    # append with maxlen (pops from left)
    dq_append_max = collections.deque([1, 2, 3], maxlen=3)
    dq_append_max.append(4)
    print('dq_append_max', list(dq_append_max))

    # appendleft
    dq_appendleft = collections.deque([2, 3])
    dq_appendleft.appendleft(1)
    print('dq_appendleft', list(dq_appendleft))

    # appendleft with maxlen (pops from right)
    dq_appendleft_max = collections.deque([2, 3, 4], maxlen=3)
    dq_appendleft_max.appendleft(1)
    print('dq_appendleft_max', list(dq_appendleft_max))

    # pop
    dq_pop = collections.deque([1, 2, 3])
    popped = dq_pop.pop()
    print('dq_pop', popped, list(dq_pop))

    # popleft
    dq_popleft = collections.deque([1, 2, 3])
    popped_left = dq_popleft.popleft()
    print('dq_popleft', popped_left, list(dq_popleft))

    # extend
    dq_extend = collections.deque([1, 2])
    dq_extend.extend([3, 4, 5])
    print('dq_extend', list(dq_extend))

    # extendleft
    dq_extendleft = collections.deque([3, 4])
    dq_extendleft.extendleft([2, 1])
    print('dq_extendleft', list(dq_extendleft))

    # rotate positive
    dq_rot = collections.deque([1, 2, 3, 4, 5])
    dq_rot.rotate(2)
    print('dq_rotate_pos', list(dq_rot))

    # rotate negative
    dq_rot2 = collections.deque([1, 2, 3, 4, 5])
    dq_rot2.rotate(-2)
    print('dq_rotate_neg', list(dq_rot2))

    # rotate zero
    dq_rot0 = collections.deque([1, 2, 3])
    dq_rot0.rotate(0)
    print('dq_rotate_zero', list(dq_rot0))

    # rotate more than length
    dq_rot_big = collections.deque([1, 2, 3])
    dq_rot_big.rotate(10)
    print('dq_rotate_big', list(dq_rot_big))

    # clear
    dq_clear = collections.deque([1, 2, 3])
    dq_clear.clear()
    print('dq_clear', list(dq_clear))

    # count
    dq_count = collections.deque([1, 2, 2, 3, 2, 4])
    print('dq_count_2', dq_count.count(2))
    print('dq_count_5', dq_count.count(5))

    # index
    dq_index = collections.deque([10, 20, 30, 20, 40])
    print('dq_index_30', dq_index.index(30))
    print('dq_index_20_start', dq_index.index(20, 2))

    # insert
    dq_insert = collections.deque([1, 3, 4])
    dq_insert.insert(1, 2)
    print('dq_insert', list(dq_insert))

    # remove
    dq_remove = collections.deque([1, 2, 3, 2, 4])
    dq_remove.remove(2)
    print('dq_remove', list(dq_remove))

    # copy
    dq_copy = collections.deque([1, 2, 3])
    dq_copy2 = dq_copy.copy()
    dq_copy2.append(4)
    print('dq_copy_orig', list(dq_copy))
    print('dq_copy_new', list(dq_copy2))

    # reverse
    dq_reverse = collections.deque([1, 2, 3, 4])
    dq_reverse.reverse()
    print('dq_reverse', list(dq_reverse))

    # maxlen property
    dq_maxprop = collections.deque([1, 2, 3], maxlen=5)
    dq_nomax = collections.deque([1, 2, 3])
    print('dq_maxlen_prop', dq_maxprop.maxlen)
    print('dq_maxlen_none', dq_nomax.maxlen)

    # __getitem__ and __setitem__
    dq_get = collections.deque([10, 20, 30])
    print('dq_getitem', dq_get[0], dq_get[1], dq_get[-1])
    dq_get[1] = 25
    print('dq_setitem', list(dq_get))

    # __contains__
    dq_contains = collections.deque([1, 2, 3])
    print('dq_contains_yes', 2 in dq_contains)
    print('dq_contains_no', 5 in dq_contains)

    # __len__
    dq_len = collections.deque([1, 2, 3, 4])
    print('dq_len', len(dq_len))
except Exception as e:
    print('SKIP_deque', type(e).__name__, e)

# === namedtuple ===
try:
    Point = collections.namedtuple('Point', ['x', 'y'])
    p = Point(11, y=22)
    print('namedtuple_basic', p[0], p[1])
    print('namedtuple_fields', p.x, p.y)
    print('namedtuple_repr', repr(p))

    # _fields attribute
    print('namedtuple_fields_attr', Point._fields)

    # _replace method
    p2 = p._replace(x=33)
    print('namedtuple_replace', p2.x, p2.y)

    # _asdict method
    print('namedtuple_asdict', dict(p._asdict()))

    # _make class method
    t = (44, 55)
    p3 = Point._make(t)
    print('namedtuple_make', p3.x, p3.y)

    # Named tuple with defaults
    Person = collections.namedtuple('Person', ['name', 'age', 'gender'], defaults=['unknown', 0])
    person1 = Person('Alice')
    print('namedtuple_defaults', person1.name, person1.age, person1.gender)

    # Named tuple with field renaming (invalid identifiers become positional)
    try:
        WithInvalid = collections.namedtuple('WithInvalid', ['abc', 'def', 'ghi'], rename=True)
        wi = WithInvalid(1, 2, 3)
        print('namedtuple_rename', wi[0], wi[1], wi[2])
    except:
        pass

    # Named tuple with module parameter
    try:
        CustomMod = collections.namedtuple('CustomMod', 'a b', module='custom')
        print('namedtuple_module', CustomMod.__module__)
    except:
        pass
except Exception as e:
    print('SKIP_namedtuple', type(e).__name__, e)

# === ChainMap ===
try:
    # Basic creation
    cm_basic = collections.ChainMap({'a': 1, 'b': 2}, {'b': 3, 'c': 4})
    print('cm_basic', dict(cm_basic))

    # Lookup order
    cm_lookup = collections.ChainMap({'a': 1}, {'a': 2, 'b': 3})
    print('cm_lookup_a', cm_lookup['a'])
    print('cm_lookup_b', cm_lookup['b'])

    # Empty creation
    cm_empty = collections.ChainMap()
    print('cm_empty', dict(cm_empty))

    # maps attribute
    cm_maps = collections.ChainMap({'a': 1}, {'b': 2})
    print('cm_maps', cm_maps.maps)

    # new_child
    cm_parent = collections.ChainMap({'a': 1})
    cm_child = cm_parent.new_child()
    cm_child['a'] = 100
    cm_child['b'] = 200
    print('cm_child_a', cm_child['a'])
    print('cm_parent_a', cm_parent['a'])
    print('cm_child_maps', cm_child.maps)

    # new_child with m parameter
    cm_parent2 = collections.ChainMap({'a': 1})
    cm_child2 = cm_parent2.new_child({'b': 2})
    print('cm_child2_maps', cm_child2.maps)

    # new_child with kwargs
    cm_parent3 = collections.ChainMap({'a': 1})
    cm_child3 = cm_parent3.new_child(m={'b': 2}, c=3)
    print('cm_child3_maps', cm_child3.maps)

    # parents property
    cm_parents = collections.ChainMap({'a': 1, 'b': 2}, {'b': 3, 'c': 4}, {'d': 5})
    print('cm_parents', cm_parents.parents.maps)

    # Writing only affects first map
    cm_write = collections.ChainMap({'a': 1}, {'a': 2})
    cm_write['a'] = 100
    print('cm_write_first', cm_write.maps[0]['a'])
    print('cm_write_second', cm_write.maps[1]['a'])

    # Deleting only from first map
    cm_del = collections.ChainMap({'a': 1, 'b': 2}, {'b': 3})
    del cm_del['b']
    print('cm_del_maps', cm_del.maps)

    # len
    cm_len = collections.ChainMap({'a': 1, 'b': 2}, {'b': 3, 'c': 4})
    print('cm_len', len(cm_len))

    # iter
    cm_iter = collections.ChainMap({'a': 1}, {'b': 2})
    print('cm_iter', sorted(list(cm_iter)))

    # items
    cm_items = collections.ChainMap({'a': 1}, {'b': 2})
    print('cm_items', sorted(list(cm_items.items())))

    # keys and values
    cm_kv = collections.ChainMap({'a': 1, 'b': 2}, {'b': 3, 'c': 4})
    print('cm_keys', sorted(list(cm_kv.keys())))

    # Union operator (Python 3.9+)
    cm1 = collections.ChainMap({'a': 1, 'b': 2})
    cm2 = {'b': 3, 'c': 4}
    try:
        cm_union = cm1 | cm2
        print('cm_union', dict(cm_union))
    except TypeError:
        pass

    # In-place union
    cm_iunion = collections.ChainMap({'a': 1})
    cm_iunion |= {'b': 2}
    print('cm_iunion', dict(cm_iunion))
except Exception as e:
    print('SKIP_ChainMap', type(e).__name__, e)

# === UserDict ===
try:
    # Basic creation
    ud_basic = collections.UserDict({'a': 1, 'b': 2})
    print('ud_basic', dict(ud_basic))

    # Empty creation
    ud_empty = collections.UserDict()
    print('ud_empty', dict(ud_empty))

    # Creation from kwargs
    ud_kwargs = collections.UserDict(a=1, b=2)
    print('ud_kwargs', dict(ud_kwargs))

    # data attribute
    ud_data = collections.UserDict({'x': 10})
    print('ud_data', ud_data.data)

    # get/set/del
    cd = collections.UserDict({'a': 1})
    print('ud_get', cd['a'])
    cd['b'] = 2
    print('ud_set', dict(cd))
    del cd['a']
    print('ud_del', dict(cd))

    # get method with default
    ud_get = collections.UserDict({'a': 1})
    print('ud_get_method', ud_get.get('a'), ud_get.get('missing', 'default'))

    # setdefault
    ud_sd = collections.UserDict({'a': 1})
    print('ud_setdefault_existing', ud_sd.setdefault('a', 100))
    print('ud_setdefault_new', ud_sd.setdefault('b', 200))
    print('ud_setdefault_result', dict(ud_sd))

    # update
    ud_upd = collections.UserDict({'a': 1})
    ud_upd.update({'b': 2})
    print('ud_update', dict(ud_upd))

    # pop
    ud_pop = collections.UserDict({'a': 1, 'b': 2})
    print('ud_pop', ud_pop.pop('a'), dict(ud_pop))
    print('ud_pop_default', ud_pop.pop('z', 'default'))

    # popitem
    ud_pi = collections.UserDict({'a': 1})
    print('ud_popitem', ud_pi.popitem())

    # clear
    ud_clear = collections.UserDict({'a': 1})
    ud_clear.clear()
    print('ud_clear', dict(ud_clear))

    # copy
    ud_copy = collections.UserDict({'a': 1, 'b': 2})
    ud_copy2 = ud_copy.copy()
    ud_copy2['c'] = 3
    print('ud_copy_orig', dict(ud_copy))
    print('ud_copy_new', dict(ud_copy2))

    # contains
    ud_contains = collections.UserDict({'a': 1, 'b': 2})
    print('ud_contains_yes', 'a' in ud_contains)
    print('ud_contains_no', 'z' in ud_contains)

    # len
    ud_len = collections.UserDict({'a': 1, 'b': 2, 'c': 3})
    print('ud_len', len(ud_len))

    # iter
    ud_iter = collections.UserDict({'a': 1, 'b': 2})
    print('ud_iter', sorted(list(ud_iter)))

    # keys, values, items
    ud_kvi = collections.UserDict({'a': 1, 'b': 2})
    print('ud_keys', sorted(list(ud_kvi.keys())))
    print('ud_values', sorted(list(ud_kvi.values())))
    print('ud_items', sorted(list(ud_kvi.items())))

    # repr
    ud_repr = collections.UserDict({'a': 1})
    print('ud_repr', repr(ud_repr))
except Exception as e:
    print('SKIP_UserDict', type(e).__name__, e)

# === UserList ===
try:
    # Basic creation
    ul_basic = collections.UserList([1, 2, 3])
    print('ul_basic', list(ul_basic))

    # Empty creation
    ul_empty = collections.UserList()
    print('ul_empty', list(ul_empty))

    # data attribute
    ul_data = collections.UserList([1, 2, 3])
    print('ul_data', ul_data.data)

    # get/set/del
    ul = collections.UserList([1, 2, 3])
    print('ul_get', ul[0], ul[1])
    ul[1] = 20
    print('ul_set', list(ul))
    del ul[0]
    print('ul_del', list(ul))

    # slice operations
    ul_slice = collections.UserList([1, 2, 3, 4, 5])
    print('ul_slice_get', list(ul_slice[1:4]))
    ul_slice[1:3] = [20, 30]
    print('ul_slice_set', list(ul_slice))

    # append
    ul_append = collections.UserList([1, 2])
    ul_append.append(3)
    print('ul_append', list(ul_append))

    # insert
    ul_insert = collections.UserList([1, 3, 4])
    ul_insert.insert(1, 2)
    print('ul_insert', list(ul_insert))

    # pop
    ul_pop = collections.UserList([1, 2, 3])
    print('ul_pop', ul_pop.pop(), list(ul_pop))
    print('ul_pop_idx', ul_pop.pop(0), list(ul_pop))

    # remove
    ul_remove = collections.UserList([1, 2, 3, 2])
    ul_remove.remove(2)
    print('ul_remove', list(ul_remove))

    # clear
    ul_clear = collections.UserList([1, 2, 3])
    ul_clear.clear()
    print('ul_clear', list(ul_clear))

    # reverse
    ul_reverse = collections.UserList([1, 2, 3])
    ul_reverse.reverse()
    print('ul_reverse', list(ul_reverse))

    # extend
    ul_extend = collections.UserList([1, 2])
    ul_extend.extend([3, 4])
    print('ul_extend', list(ul_extend))

    # count
    ul_count = collections.UserList([1, 2, 2, 3, 2])
    print('ul_count', ul_count.count(2))

    # index
    ul_index = collections.UserList([10, 20, 30, 20])
    print('ul_index', ul_index.index(20))
    print('ul_index_start', ul_index.index(20, 2))

    # copy
    ul_copy = collections.UserList([1, 2, 3])
    ul_copy2 = ul_copy.copy()
    ul_copy2.append(4)
    print('ul_copy_orig', list(ul_copy))
    print('ul_copy_new', list(ul_copy2))

    # contains
    ul_contains = collections.UserList([1, 2, 3])
    print('ul_contains_yes', 2 in ul_contains)
    print('ul_contains_no', 5 in ul_contains)

    # len
    ul_len = collections.UserList([1, 2, 3, 4])
    print('ul_len', len(ul_len))

    # add (concatenation)
    ul_add = collections.UserList([1, 2]) + collections.UserList([3, 4])
    print('ul_add', list(ul_add))

    # iadd (in-place add)
    ul_iadd = collections.UserList([1, 2])
    ul_iadd += [3, 4]
    print('ul_iadd', list(ul_iadd))

    # mul
    ul_mul = collections.UserList([1, 2]) * 3
    print('ul_mul', list(ul_mul))

    # imul
    ul_imul = collections.UserList([1, 2])
    ul_imul *= 2
    print('ul_imul', list(ul_imul))

    # repr
    ul_repr = collections.UserList([1, 2, 3])
    print('ul_repr', repr(ul_repr))
except Exception as e:
    print('SKIP_UserList', type(e).__name__, e)

# === UserString ===
try:
    # Basic creation
    us_basic = collections.UserString('hello')
    print('us_basic', str(us_basic))

    # data attribute
    us_data = collections.UserString('world')
    print('us_data', us_data.data)

    # String methods
    us = collections.UserString('Hello World')
    print('us_lower', us.lower())
    print('us_upper', us.upper())
    print('us_capitalize', us.capitalize())
    print('us_title', us.title())
    print('us_swapcase', us.swapcase())

    # Searching methods
    us_search = collections.UserString('hello world hello')
    print('us_find', us_search.find('hello'))
    print('us_find_start', us_search.find('hello', 1))
    print('us_rfind', us_search.rfind('hello'))
    print('us_count', us_search.count('hello'))
    print('us_startswith', us_search.startswith('hello'))
    print('us_endswith', us_search.endswith('world'))

    # strip methods
    us_strip = collections.UserString('  hello world  ')
    print('us_strip', repr(us_strip.strip()))
    print('us_lstrip', repr(us_strip.lstrip()))
    print('us_rstrip', repr(us_strip.rstrip()))

    # split/join
    us_split = collections.UserString('a,b,c')
    print('us_split', us_split.split(','))
    print('us_split_max', us_split.split(',', 1))

    us_join = collections.UserString('-')
    print('us_join', us_join.join(['a', 'b', 'c']))

    # replace
    us_replace = collections.UserString('hello world hello')
    print('us_replace', us_replace.replace('hello', 'hi'))
    print('us_replace_max', us_replace.replace('hello', 'hi', 1))

    # center/ljust/rjust
    us_pad = collections.UserString('hi')
    print('us_center', repr(us_pad.center(10)))
    print('us_ljust', repr(us_pad.ljust(10)))
    print('us_rjust', repr(us_pad.rjust(10)))
    print('us_center_fill', repr(us_pad.center(10, '-')))

    # zfill
    us_zfill = collections.UserString('42')
    print('us_zfill', us_zfill.zfill(5))

    # expandtabs
    us_tabs = collections.UserString('a\tb\tc')
    print('us_expandtabs', repr(us_tabs.expandtabs(4)))

    # slicing
    us_slice = collections.UserString('hello world')
    print('us_slice', us_slice[0:5])
    print('us_slice_step', us_slice[::2])

    # concatenation
    us_cat = collections.UserString('hello')
    print('us_concat', us_cat + ' world')
    print('us_concat_us', us_cat + collections.UserString('!'))

    # repetition
    us_rep = collections.UserString('ab')
    print('us_rep', us_rep * 3)

    # contains
    us_contains = collections.UserString('hello world')
    print('us_contains_yes', 'world' in us_contains)
    print('us_contains_no', 'xyz' in us_contains)

    # len
    us_len = collections.UserString('hello')
    print('us_len', len(us_len))

    # indexing
    us_idx = collections.UserString('hello')
    print('us_index', us_idx[0], us_idx[-1])

    # iteration
    us_iter = collections.UserString('abc')
    print('us_iter', list(us_iter))

    # hash
    us_hash = collections.UserString('test')
    try:
        print('us_hash', hash(us_hash))
    except TypeError as e:
        print('us_hash_error', str(e))

    # repr
    us_repr = collections.UserString('hello')
    print('us_repr', repr(us_repr))

    # Comparison
    us_cmp1 = collections.UserString('abc')
    us_cmp2 = collections.UserString('abc')
    us_cmp3 = collections.UserString('def')
    print('us_eq', us_cmp1 == us_cmp2)
    print('us_eq_str', us_cmp1 == 'abc')
    print('us_lt', us_cmp1 < us_cmp3)

    # partition/rpartition
    us_part = collections.UserString('hello.world.test')
    print('us_partition', us_part.partition('.'))
    print('us_rpartition', us_part.rpartition('.'))

    # splitlines
    us_lines = collections.UserString('line1\nline2\r\nline3')
    print('us_splitlines', us_lines.splitlines())

    # isdigit/isalpha etc
    us_checks = collections.UserString('Hello123')
    us_digit = collections.UserString('123')
    us_alpha = collections.UserString('abc')
    us_space = collections.UserString('   ')
    print('us_isdigit', us_digit.isdigit())
    print('us_isalpha', us_alpha.isalpha())
    print('us_isalnum', us_checks.isalnum())
    print('us_isspace', us_space.isspace())
    print('us_islower', us_alpha.islower())
    print('us_isupper', us_alpha.isupper())
    print('us_istitle', us_checks.istitle())
except Exception as e:
    print('SKIP_UserString', type(e).__name__, e)
