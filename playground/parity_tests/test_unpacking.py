# === Basic tuple unpacking ===
try:
    a, b = 1, 2
    print('basic_unpack', a, b)
except Exception as e:
    print('SKIP_Basic tuple unpacking', type(e).__name__, e)

# === Basic list unpacking ===
try:
    c, d = [3, 4]
    print('list_unpack', c, d)
except Exception as e:
    print('SKIP_Basic list unpacking', type(e).__name__, e)

# === Basic unpacking with parens ===
try:
    (e, f) = (5, 6)
    print('parens_unpack', e, f)
except Exception as e:
    print('SKIP_Basic unpacking with parens', type(e).__name__, e)

# === Unpacking with trailing comma ===
try:
    g, h = 7, 8
    print('trailing_comma', g, h)
except Exception as e:
    print('SKIP_Unpacking with trailing comma', type(e).__name__, e)

# === Single element unpacking ===
try:
    (i,) = [9]
    print('single_element', i)
except Exception as e:
    print('SKIP_Single element unpacking', type(e).__name__, e)

# === Extended unpacking with star at beginning ===
try:
    *first, last = [1, 2, 3, 4, 5]
    print('star_first', first, last)
except Exception as e:
    print('SKIP_Extended unpacking with star at beginning', type(e).__name__, e)

# === Extended unpacking with star at end ===
try:
    first_elem, *rest = [1, 2, 3, 4, 5]
    print('star_end', first_elem, rest)
except Exception as e:
    print('SKIP_Extended unpacking with star at end', type(e).__name__, e)

# === Extended unpacking with star in middle ===
try:
    head, *middle, tail = [1, 2, 3, 4, 5]
    print('star_middle', head, middle, tail)
except Exception as e:
    print('SKIP_Extended unpacking with star in middle', type(e).__name__, e)

# === Extended unpacking single element ===
try:
    *all_items, = [1, 2, 3]
    print('star_all', all_items)
except Exception as e:
    print('SKIP_Extended unpacking single element', type(e).__name__, e)

# === Extended unpacking empty rest ===
try:
    only, *empty = [42]
    print('star_empty_rest', only, empty)
except Exception as e:
    print('SKIP_Extended unpacking empty rest', type(e).__name__, e)

# === Extended unpacking all in rest ===
try:
    *all_rest, = [1, 2, 3]
    print('star_all_rest', all_rest)
except Exception as e:
    print('SKIP_Extended unpacking all in rest', type(e).__name__, e)

# === Nested tuple unpacking ===
try:
    ((a1, a2), (b1, b2)) = ((1, 2), (3, 4))
    print('nested_tuple', a1, a2, b1, b2)
except Exception as e:
    print('SKIP_Nested tuple unpacking', type(e).__name__, e)

# === Nested list unpacking ===
try:
    [[c1, c2], [d1, d2]] = [[5, 6], [7, 8]]
    print('nested_list', c1, c2, d1, d2)
except Exception as e:
    print('SKIP_Nested list unpacking', type(e).__name__, e)

# === Mixed nested unpacking ===
try:
    ((e1, e2), [f1, f2]) = ((9, 10), [11, 12])
    print('mixed_nested', e1, e2, f1, f2)
except Exception as e:
    print('SKIP_Mixed nested unpacking', type(e).__name__, e)

# === Deeply nested unpacking ===
try:
    (((x1, x2), x3), (y1, (y2, y3))) = (((1, 2), 3), (4, (5, 6)))
    print('deeply_nested', x1, x2, x3, y1, y2, y3)
except Exception as e:
    print('SKIP_Deeply nested unpacking', type(e).__name__, e)

# === Nested unpacking with star ===
try:
    ((g1, *g2), h1) = ((1, 2, 3, 4), 5)
    print('nested_with_star', g1, g2, h1)
except Exception as e:
    print('SKIP_Nested unpacking with star', type(e).__name__, e)

# === Unpacking in for loop basic ===
try:
    result = []
    for m, n in [(1, 2), (3, 4), (5, 6)]:
        result.append((m, n))
    print('for_loop_basic', result)
except Exception as e:
    print('SKIP_Unpacking in for loop basic', type(e).__name__, e)

# === Unpacking in for loop with star ===
try:
    result2 = []
    for p, *q in [(1, 2, 3), (4, 5), (6,)]:
        result2.append((p, q))
    print('for_loop_star', result2)
except Exception as e:
    print('SKIP_Unpacking in for loop with star', type(e).__name__, e)

# === Unpacking in for loop nested ===
try:
    result3 = []
    for (r, s), t in [((1, 2), 3), ((4, 5), 6)]:
        result3.append((r, s, t))
    print('for_loop_nested', result3)
except Exception as e:
    print('SKIP_Unpacking in for loop nested', type(e).__name__, e)

# === Unpacking in enumerate ===
try:
    result4 = []
    for idx, (u, v) in enumerate([(10, 20), (30, 40)]):
        result4.append((idx, u, v))
    print('enumerate_unpack', result4)
except Exception as e:
    print('SKIP_Unpacking in enumerate', type(e).__name__, e)

# === Unpacking in zip ===
try:
    result5 = []
    for w, x in zip([1, 2, 3], ['a', 'b', 'c']):
        result5.append((w, x))
    print('zip_unpack', result5)
except Exception as e:
    print('SKIP_Unpacking in zip', type(e).__name__, e)

# === Multiple assignment unpacking ===
try:
    aa = bb = cc = 1
    print('multiple_assign', aa, bb, cc)
except Exception as e:
    print('SKIP_Multiple assignment unpacking', type(e).__name__, e)

# === Chained unpacking ===
try:
    xx, yy = zz, ww = 1, 2
    print('chained_unpack', xx, yy, zz, ww)
except Exception as e:
    print('SKIP_Chained unpacking', type(e).__name__, e)

# === Swapping variables ===
try:
    swap_a, swap_b = 10, 20
    swap_a, swap_b = swap_b, swap_a
    print('swap_vars', swap_a, swap_b)
except Exception as e:
    print('SKIP_Swapping variables', type(e).__name__, e)

# === Unpacking string ===
try:
    char1, char2, char3 = 'abc'
    print('string_unpack', char1, char2, char3)
except Exception as e:
    print('SKIP_Unpacking string', type(e).__name__, e)

# === Unpacking range ===
try:
    r1, r2, r3 = range(3)
    print('range_unpack', r1, r2, r3)
except Exception as e:
    print('SKIP_Unpacking range', type(e).__name__, e)

# === Unpacking generator ===
try:
    def gen():
        yield 1
        yield 2
        yield 3

    g1, g2, g3 = gen()
    print('generator_unpack', g1, g2, g3)
except Exception as e:
    print('SKIP_Unpacking generator', type(e).__name__, e)

# === Unpacking set (order may vary) ===
try:
    s = {1, 2, 3}
    *set_list, = s
    set_list.sort()
    print('set_unpack_sorted', set_list)
except Exception as e:
    print('SKIP_Unpacking set (order may vary)', type(e).__name__, e)

# === Unpacking dict keys ===
try:
    d = {'a': 1, 'b': 2, 'c': 3}
    k1, k2, k3 = d
    keys = list(sorted([k1, k2, k3]))
    print('dict_keys_unpack', keys)
except Exception as e:
    print('SKIP_Unpacking dict keys', type(e).__name__, e)

# === Unpacking dict items ===
try:
    d2 = {'x': 10, 'y': 20}
    item_list = []
    for key, val in d2.items():
        item_list.append((key, val))
    item_list.sort()
    print('dict_items_unpack', item_list)
except Exception as e:
    print('SKIP_Unpacking dict items', type(e).__name__, e)

# === Function args unpacking ===
try:
    def func_args(a, b, c):
        return (a, b, c)

    args = (1, 2, 3)
    result = func_args(*args)
    print('func_args_unpack', result)
except Exception as e:
    print('SKIP_Function args unpacking', type(e).__name__, e)

# === Function args unpacking with extra ===
try:
    def func_args2(a, b, c, d):
        return (a, b, c, d)

    result = func_args2(1, *[2, 3], 4)
    print('func_args_mixed', result)
except Exception as e:
    print('SKIP_Function args unpacking with extra', type(e).__name__, e)

# === Function kwargs unpacking ===
try:
    def func_kwargs(a, b, c):
        return (a, b, c)

    kwargs = {'a': 1, 'b': 2, 'c': 3}
    result = func_kwargs(**kwargs)
    print('func_kwargs_unpack', result)
except Exception as e:
    print('SKIP_Function kwargs unpacking', type(e).__name__, e)

# === Function args and kwargs combined ===
try:
    def func_combined(a, b, c, d):
        return (a, b, c, d)

    args = (1, 2)
    kwargs = {'c': 3, 'd': 4}
    result = func_combined(*args, **kwargs)
    print('func_combined_unpack', result)
except Exception as e:
    print('SKIP_Function args and kwargs combined', type(e).__name__, e)

# === Function *args parameter ===
try:
    def func_varargs(*args):
        return args

    result = func_varargs(1, 2, 3, 4, 5)
    print('func_varargs', result)
except Exception as e:
    print('SKIP_Function *args parameter', type(e).__name__, e)

# === Function **kwargs parameter ===
try:
    def func_varkwargs(**kwargs):
        return kwargs

    result = func_varkwargs(a=1, b=2, c=3)
    print('func_varkwargs', result)
except Exception as e:
    print('SKIP_Function **kwargs parameter', type(e).__name__, e)

# === Function *args and **kwargs ===
try:
    def func_both(*args, **kwargs):
        return (args, kwargs)

    result = func_both(1, 2, x=10, y=20)
    print('func_both', result)
except Exception as e:
    print('SKIP_Function *args and **kwargs', type(e).__name__, e)

# === Function with positional only and unpack ===
try:
    def func_pos_only(a, b, /, c):
        return (a, b, c)

    result = func_pos_only(1, 2, c=3)
    print('func_pos_only', result)
except Exception as e:
    print('SKIP_Function with positional only and unpack', type(e).__name__, e)

# === Function with keyword only and unpack ===
try:
    def func_kw_only(a, *, b, c):
        return (a, b, c)

    result = func_kw_only(1, b=2, c=3)
    print('func_kw_only', result)
except Exception as e:
    print('SKIP_Function with keyword only and unpack', type(e).__name__, e)

# === List literal unpacking ===
try:
    lst = [1, 2, 3]
    result = [*lst, 4, 5]
    print('list_literal_unpack', result)
except Exception as e:
    print('SKIP_List literal unpacking', type(e).__name__, e)

# === List literal multiple unpacks ===
try:
    lst1 = [1, 2]
    lst2 = [3, 4]
    result = [*lst1, *lst2, 5]
    print('list_multi_unpack', result)
except Exception as e:
    print('SKIP_List literal multiple unpacks', type(e).__name__, e)

# === Tuple literal unpacking ===
try:
    t = (1, 2, 3)
    result = (*t, 4, 5)
    print('tuple_literal_unpack', result)
except Exception as e:
    print('SKIP_Tuple literal unpacking', type(e).__name__, e)

# === Set literal unpacking ===
try:
    s1 = {1, 2}
    s2 = {2, 3}
    result = {*s1, *s2, 4}
    print('set_literal_unpack', sorted(result))
except Exception as e:
    print('SKIP_Set literal unpacking', type(e).__name__, e)

# === Dict literal unpacking ===
try:
    d1 = {'a': 1, 'b': 2}
    d2 = {'c': 3, 'd': 4}
    result = {**d1, **d2}
    print('dict_literal_unpack', result)
except Exception as e:
    print('SKIP_Dict literal unpacking', type(e).__name__, e)

# === Dict literal unpack with override ===
try:
    d3 = {'x': 1, 'y': 2}
    d4 = {'y': 3, 'z': 4}
    result = {**d3, **d4}
    print('dict_override_unpack', result)
except Exception as e:
    print('SKIP_Dict literal unpack with override', type(e).__name__, e)

# === Dict literal mixed unpacking ===
try:
    d5 = {'a': 1}
    result = {**d5, 'b': 2, 'c': 3}
    print('dict_mixed_unpack', result)
except Exception as e:
    print('SKIP_Dict literal mixed unpacking', type(e).__name__, e)

# === Nested function call unpacking ===
try:
    def outer(a, b):
        def inner(c, d):
            return (a, b, c, d)
        return inner

    result = outer(1, 2)(*(3, 4))
    print('nested_call_unpack', result)
except Exception as e:
    print('SKIP_Nested function call unpacking', type(e).__name__, e)

# === Unpacking in list comprehension ===
try:
    data = [(1, 2), (3, 4), (5, 6)]
    result = [x + y for x, y in data]
    print('listcomp_unpack', result)
except Exception as e:
    print('SKIP_Unpacking in list comprehension', type(e).__name__, e)

# === Unpacking in dict comprehension ===
try:
    data = [('a', 1), ('b', 2)]
    result = {k: v * 2 for k, v in data}
    print('dictcomp_unpack', result)
except Exception as e:
    print('SKIP_Unpacking in dict comprehension', type(e).__name__, e)

# === Unpacking in generator expression ===
try:
    data = [(1, 2), (3, 4)]
    g = (x * y for x, y in data)
    result = list(g)
    print('genexp_unpack', result)
except Exception as e:
    print('SKIP_Unpacking in generator expression', type(e).__name__, e)

# === Unpacking in set comprehension ===
try:
    data = [(1, 2), (2, 3), (1, 2)]
    result = {x + y for x, y in data}
    print('setcomp_unpack', sorted(result))
except Exception as e:
    print('SKIP_Unpacking in set comprehension', type(e).__name__, e)

# === Unpacking with slices assignment ===
try:
    nums = [1, 2, 3, 4, 5]
    a, *b, c = nums
    print('slice_assign_unpack', a, b, c)
except Exception as e:
    print('SKIP_Unpacking with slices assignment', type(e).__name__, e)

# === Unpacking empty sequence with star only ===
try:
    *empty_result, = []
    print('empty_star_only', empty_result)
except Exception as e:
    print('SKIP_Unpacking empty sequence with star only', type(e).__name__, e)

# === Unpacking single item with star ===
try:
    *single_item, = [42]
    print('single_item_star', single_item)
except Exception as e:
    print('SKIP_Unpacking single item with star', type(e).__name__, e)

# === Unpacking into existing variables ===
try:
    existing_a = existing_b = None
    existing_a, existing_b = 100, 200
    print('existing_vars', existing_a, existing_b)
except Exception as e:
    print('SKIP_Unpacking into existing variables', type(e).__name__, e)

# === Unpacking class attributes ===
try:
    class Point:
        def __init__(self):
            self.x = 10
            self.y = 20

    p = Point()
    x_coord, y_coord = p.x, p.y
    print('class_attr_unpack', x_coord, y_coord)
except Exception as e:
    print('SKIP_Unpacking class attributes', type(e).__name__, e)

# === Unpacking from method return ===
try:
    class Container:
        def get_pair(self):
            return (1, 2)

    c = Container()
    val1, val2 = c.get_pair()
    print('method_return_unpack', val1, val2)
except Exception as e:
    print('SKIP_Unpacking from method return', type(e).__name__, e)

# === Unpacking builtin enumerate ===
try:
    items = ['a', 'b', 'c']
    result = []
    for idx, val in enumerate(items):
        result.append((idx, val))
    print('builtin_enumerate', result)
except Exception as e:
    print('SKIP_Unpacking builtin enumerate', type(e).__name__, e)

# === Unpacking builtin zip ===
try:
    keys = ['a', 'b', 'c']
    vals = [1, 2, 3]
    result = []
    for k, v in zip(keys, vals):
        result.append((k, v))
    print('builtin_zip', result)
except Exception as e:
    print('SKIP_Unpacking builtin zip', type(e).__name__, e)

# === Unpacking builtin zip_longest equivalent ===
try:
    from itertools import zip_longest
    short = [1, 2]
    long = ['a', 'b', 'c']
    result = []
    for x, y in zip_longest(short, long, fillvalue=None):
        result.append((x, y))
    print('zip_longest_unpack', result)
except Exception as e:
    print('SKIP_Unpacking builtin zip_longest equivalent', type(e).__name__, e)

# === Unpacking reversed ===
try:
    rev = [3, 2, 1]
    r1, r2, r3 = reversed(rev)
    print('reversed_unpack', r1, r2, r3)
except Exception as e:
    print('SKIP_Unpacking reversed', type(e).__name__, e)

# === Unpacking map result ===
try:
    mapped = map(lambda x: (x, x * 2), [1, 2, 3])
    result = []
    for orig, doubled in mapped:
        result.append((orig, doubled))
    print('map_unpack', result)
except Exception as e:
    print('SKIP_Unpacking map result', type(e).__name__, e)

# === Unpacking filter with map ===
try:
    data = [(True, 1), (False, 2), (True, 3)]
    filtered = [(f, v) for f, v in data if f]
    print('filter_map_unpack', filtered)
except Exception as e:
    print('SKIP_Unpacking filter with map', type(e).__name__, e)

# === Unpacking from tuple subclass ===
try:
    from collections import namedtuple
    Person = namedtuple('Person', 'name age')
    person = Person('Alice', 30)
    name, age = person
    print('namedtuple_unpack', name, age)
except Exception as e:
    print('SKIP_Unpacking from tuple subclass', type(e).__name__, e)

# === Unpacking with underscore convention ===
try:
    first, _, third = (1, 2, 3)
    print('underscore_unpack', first, third)
except Exception as e:
    print('SKIP_Unpacking with underscore convention', type(e).__name__, e)

# === Unpacking multiple underscores ===
try:
    a, _, c, _, e = (1, 2, 3, 4, 5)
    print('multi_underscore', a, c, e)
except Exception as e:
    print('SKIP_Unpacking multiple underscores', type(e).__name__, e)

# === Extended unpacking with underscore ===
try:
    first, *_, last = (1, 2, 3, 4, 5)
    print('star_underscore', first, last)
except Exception as e:
    print('SKIP_Extended unpacking with underscore', type(e).__name__, e)

# === Unpacking from bytes ===
try:
    b1, b2, b3 = b'abc'
    print('bytes_unpack', b1, b2, b3)
except Exception as e:
    print('SKIP_Unpacking from bytes', type(e).__name__, e)

# === Unpacking from bytearray ===
try:
    ba = bytearray(b'xyz')
    x, y, z = ba
    print('bytearray_unpack', x, y, z)
except Exception as e:
    print('SKIP_Unpacking from bytearray', type(e).__name__, e)

# === Unpacking from memoryview ===
try:
    mv = memoryview(b'123')
    m1, m2, m3 = mv
    print('memoryview_unpack', m1, m2, m3)
except Exception as e:
    print('SKIP_Unpacking from memoryview', type(e).__name__, e)

# === Unpacking tuple of lists ===
try:
    ([tl1, tl2], [tl3, tl4]) = ([1, 2], [3, 4])
    print('tuple_of_lists', tl1, tl2, tl3, tl4)
except Exception as e:
    print('SKIP_Unpacking tuple of lists', type(e).__name__, e)

# === Unpacking list of tuples ===
try:
    [lt1, lt2], [lt3, lt4] = [(1, 2), (3, 4)]
    print('list_of_tuples', lt1, lt2, lt3, lt4)
except Exception as e:
    print('SKIP_Unpacking list of tuples', type(e).__name__, e)

# === Unpacking with string method ===
try:
    parts = 'a,b,c'.split(',')
    p1, p2, p3 = parts
    print('split_unpack', p1, p2, p3)
except Exception as e:
    print('SKIP_Unpacking with string method', type(e).__name__, e)

# === Unpacking with list pop ===
try:
    stack = [3, 2, 1]
    first, *rest = stack
    print('stack_unpack', first, rest)
except Exception as e:
    print('SKIP_Unpacking with list pop', type(e).__name__, e)

# === Unpacking with queue simulation ===
try:
    from collections import deque
    queue = deque([1, 2, 3, 4])
    head, *tail = queue
    print('queue_unpack', head, tail)
except Exception as e:
    print('SKIP_Unpacking with queue simulation', type(e).__name__, e)

# === Unpacking json-like structure ===
try:
    json_data = {'items': [(1, 'a'), (2, 'b'), (3, 'c')]}
    result = []
    for num, letter in json_data['items']:
        result.append((num, letter))
    print('json_like_unpack', result)
except Exception as e:
    print('SKIP_Unpacking json-like structure', type(e).__name__, e)

# === Unpacking matrix row ===
try:
    matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
    row1, row2, row3 = matrix
    print('matrix_rows', row1, row2, row3)
except Exception as e:
    print('SKIP_Unpacking matrix row', type(e).__name__, e)

# === Unpacking matrix element ===
try:
    elem = matrix[0][0], matrix[1][1], matrix[2][2]
    print('matrix_diag', elem)
except Exception as e:
    print('SKIP_Unpacking matrix element', type(e).__name__, e)

# === Unpacking coordinates ===
try:
    coords = [(0, 0), (1, 0), (0, 1), (1, 1)]
    (x1, y1), (x2, y2), (x3, y3), (x4, y4) = coords
    print('coords_unpack', x1, y1, x2, y2, x3, y3, x4, y4)
except Exception as e:
    print('SKIP_Unpacking coordinates', type(e).__name__, e)

# === Unpacking with negative indices ===
try:
    neg = [10, 20, 30, 40]
    first, *middle, last = neg
    print('negative_concept', first, middle, last)
except Exception as e:
    print('SKIP_Unpacking with negative indices', type(e).__name__, e)

# === Unpacking operator itemgetter ===
try:
    from operator import itemgetter
    data = [('a', 1), ('b', 2), ('c', 3)]
    get_first = itemgetter(0)
    get_second = itemgetter(1)
    result = [(get_first(x), get_second(x)) for x in data]
    print('itemgetter_unpack', result)
except Exception as e:
    print('SKIP_Unpacking operator itemgetter', type(e).__name__, e)

# === Unpacking with partial ===
try:
    from functools import partial
    def func(a, b, c):
        return (a, b, c)

    partial_func = partial(func, 1)
    result = partial_func(2, 3)
    print('partial_unpack', result)
except Exception as e:
    print('SKIP_Unpacking with partial', type(e).__name__, e)

# === Unpacking in lambda ===
try:
    make_pair = lambda x, y: (x, y)
    a, b = make_pair(1, 2)
    print('lambda_unpack', a, b)
except Exception as e:
    print('SKIP_Unpacking in lambda', type(e).__name__, e)

# === Unpacking lambda result ===
try:
    get_values = lambda: (10, 20, 30)
    v1, v2, v3 = get_values()
    print('lambda_result_unpack', v1, v2, v3)
except Exception as e:
    print('SKIP_Unpacking lambda result', type(e).__name__, e)

# === Unpacking in conditional expression ===
try:
    flag = True
    result = (1, 2) if flag else (3, 4)
    a, b = result
    print('conditional_unpack', a, b)
except Exception as e:
    print('SKIP_Unpacking in conditional expression', type(e).__name__, e)

# === Unpacking with getattr ===
try:
    class Config:
        default_host = 'localhost'
        default_port = 8080

    cfg = Config()
    host, port = getattr(cfg, 'default_host'), getattr(cfg, 'default_port')
    print('getattr_unpack', host, port)
except Exception as e:
    print('SKIP_Unpacking with getattr', type(e).__name__, e)

# === Unpacking with tuple unpacking in return ===
try:
    def return_pair():
        return 1, 2

    x, y = return_pair()
    print('return_pair_unpack', x, y)
except Exception as e:
    print('SKIP_Unpacking with tuple unpacking in return', type(e).__name__, e)

# === Unpacking with tuple unpacking in return star ===
try:
    def return_many():
        return [1, 2, 3, 4, 5]

    first, *rest = return_many()
    print('return_many_unpack', first, rest)
except Exception as e:
    print('SKIP_Unpacking with tuple unpacking in return star', type(e).__name__, e)

# === Unpacking with default values in loop ===
try:
    items = [(1,), (2, 3), (4, 5, 6)]
    result = []
    for item in items:
        first, *rest = item
        result.append((first, rest))
    print('variable_length_unpack', result)
except Exception as e:
    print('SKIP_Unpacking with default values in loop', type(e).__name__, e)

# === Unpacking with star in nested for ===
try:
    matrix = [[1, 2], [3, 4, 5], [6]]
    result = []
    for row in matrix:
        first, *rest = row
        result.append((first, rest))
    print('nested_for_star', result)
except Exception as e:
    print('SKIP_Unpacking with star in nested for', type(e).__name__, e)

# === Unpacking with iter ===
try:
    it = iter([1, 2, 3])
    a, b, c = it
    print('iter_unpack', a, b, c)
except Exception as e:
    print('SKIP_Unpacking with iter', type(e).__name__, e)

# === Unpacking chained iterators ===
try:
    from itertools import chain
    ch = chain([1, 2], [3, 4], [5])
    *chained, = ch
    print('chain_unpack', chained)
except Exception as e:
    print('SKIP_Unpacking chained iterators', type(e).__name__, e)

# === Unpacking tee result ===
try:
    from itertools import tee
    original = [1, 2, 3]
    t1, t2 = tee(original, 2)
    *tee1, = t1
    *tee2, = t2
    print('tee_unpack', tee1, tee2)
except Exception as e:
    print('SKIP_Unpacking tee result', type(e).__name__, e)

# === Unpacking islice ===
try:
    from itertools import islice
    sl = islice(range(10), 3, 6)
    *slice_result, = sl
    print('islice_unpack', slice_result)
except Exception as e:
    print('SKIP_Unpacking islice', type(e).__name__, e)

# === Unpacking accumulate ===
try:
    from itertools import accumulate
    acc = accumulate([1, 2, 3, 4, 5])
    *acc_result, = acc
    print('accumulate_unpack', acc_result)
except Exception as e:
    print('SKIP_Unpacking accumulate', type(e).__name__, e)

# === Unpacking groupby ===
try:
    from itertools import groupby
    data = [('a', 1), ('a', 2), ('b', 3)]
    result = []
    for key, group in groupby(data, key=lambda x: x[0]):
        items = list(group)
        result.append((key, items))
    print('groupby_unpack', result)
except Exception as e:
    print('SKIP_Unpacking groupby', type(e).__name__, e)

# === Unpacking with unpack in exception handler ===
try:
    try:
        raise ValueError('error', 42)
    except ValueError as e:
        msg, code = e.args
        print('exception_unpack', msg, code)
except Exception as e:
    print('SKIP_Unpacking with unpack in exception handler', type(e).__name__, e)

# === Unpacking with context manager result ===
try:
    class Context:
        def __enter__(self):
            return (1, 2)
        def __exit__(self, *args):
            pass

    with Context() as (val1, val2):
        print('context_unpack', val1, val2)
except Exception as e:
    print('SKIP_Unpacking with context manager result', type(e).__name__, e)
