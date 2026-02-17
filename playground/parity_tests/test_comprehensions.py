# === List comprehension - basic ===
try:
    print('list_comp_basic', [x * 2 for x in [1, 2, 3]])
except Exception as e:
    print('SKIP_list_comp_basic', type(e).__name__, e)

# === List comprehension - with range ===
try:
    print('list_comp_range', [x ** 2 for x in range(5)])
except Exception as e:
    print('SKIP_list_comp_range', type(e).__name__, e)

# === List comprehension - with condition ===
try:
    print('list_comp_if', [x for x in range(10) if x % 2 == 0])
except Exception as e:
    print('SKIP_list_comp_if', type(e).__name__, e)

# === List comprehension - with multiple conditions ===
try:
    print('list_comp_if_multi', [x for x in range(20) if x % 2 == 0 if x % 3 == 0])
except Exception as e:
    print('SKIP_list_comp_if_multi', type(e).__name__, e)

# === List comprehension - with if else expression ===
try:
    print('list_comp_if_else', ['even' if x % 2 == 0 else 'odd' for x in range(5)])
except Exception as e:
    print('SKIP_list_comp_if_else', type(e).__name__, e)

# === List comprehension - multiple for clauses ===
try:
    print('list_comp_multi_for', [(x, y) for x in [1, 2] for y in ['a', 'b']])
except Exception as e:
    print('SKIP_list_comp_multi_for', type(e).__name__, e)

# === List comprehension - nested for with if ===
try:
    print('list_comp_nested_for_if', [(x, y) for x in [1, 2, 3] for y in [3, 1, 4] if x != y])
except Exception as e:
    print('SKIP_list_comp_nested_for_if', type(e).__name__, e)

# === List comprehension - flatten nested list ===
try:
    vec = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
    print('list_comp_flatten', [num for elem in vec for num in elem])
except Exception as e:
    print('SKIP_list_comp_flatten', type(e).__name__, e)

# === List comprehension - tuple result ===
try:
    print('list_comp_tuple', [(x, x ** 2) for x in range(4)])
except Exception as e:
    print('SKIP_list_comp_tuple', type(e).__name__, e)

# === List comprehension - nested list comprehension ===
try:
    matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
    print('list_comp_nested', [[row[i] for row in matrix] for i in range(3)])
except Exception as e:
    print('SKIP_list_comp_nested', type(e).__name__, e)

# === List comprehension - with string method ===
try:
    freshfruit = ['  banana', '  loganberry ', 'passion fruit  ']
    print('list_comp_method', [weapon.strip() for weapon in freshfruit])
except Exception as e:
    print('SKIP_list_comp_method', type(e).__name__, e)

# === List comprehension - with walrus operator ===
try:
    print('list_comp_walrus', [y for x in range(5) if (y := x * 2) > 4])
except Exception as e:
    print('SKIP_list_comp_walrus', type(e).__name__, e)

# === List comprehension - walrus in expression ===
try:
    print('list_comp_walrus_expr', [(y := x * 2, y + 1) for x in range(3)])
except Exception as e:
    print('SKIP_list_comp_walrus_expr', type(e).__name__, e)

# === List comprehension - empty input ===
try:
    print('list_comp_empty', [x * 2 for x in []])
except Exception as e:
    print('SKIP_list_comp_empty', type(e).__name__, e)

# === List comprehension - all elements filtered ===
try:
    print('list_comp_all_filtered', [x for x in range(5) if x > 10])
except Exception as e:
    print('SKIP_list_comp_all_filtered', type(e).__name__, e)

# === List comprehension - with enumerate ===
try:
    print('list_comp_enumerate', [(i, v) for i, v in enumerate(['a', 'b', 'c'])])
except Exception as e:
    print('SKIP_list_comp_enumerate', type(e).__name__, e)

# === List comprehension - with zip ===
try:
    print('list_comp_zip', [(a, b) for a, b in zip([1, 2], ['x', 'y'])])
except Exception as e:
    print('SKIP_list_comp_zip', type(e).__name__, e)


# === Dictionary comprehension - basic ===
try:
    print('dict_comp_basic', {x: x ** 2 for x in range(5)})
except Exception as e:
    print('SKIP_dict_comp_basic', type(e).__name__, e)

# === Dictionary comprehension - from list ===
try:
    print('dict_comp_from_list', {k: len(k) for k in ['apple', 'banana', 'cherry']})
except Exception as e:
    print('SKIP_dict_comp_from_list', type(e).__name__, e)

# === Dictionary comprehension - with condition ===
try:
    print('dict_comp_if', {x: x ** 2 for x in range(10) if x % 2 == 0})
except Exception as e:
    print('SKIP_dict_comp_if', type(e).__name__, e)

# === Dictionary comprehension - multiple for clauses ===
try:
    print('dict_comp_multi_for', {f'{x}-{y}': x + y for x in [1, 2] for y in [10, 20]})
except Exception as e:
    print('SKIP_dict_comp_multi_for', type(e).__name__, e)

# === Dictionary comprehension - swap keys values ===
try:
    original = {'a': 1, 'b': 2, 'c': 3}
    print('dict_comp_swap', {v: k for k, v in original.items()})
except Exception as e:
    print('SKIP_dict_comp_swap', type(e).__name__, e)

# === Dictionary comprehension - with walrus ===
try:
    print('dict_comp_walrus', {x: (y := x * 2) for x in range(3) if y > 2})
except Exception as e:
    print('SKIP_dict_comp_walrus', type(e).__name__, e)

# === Dictionary comprehension - nested ===
try:
    print('dict_comp_nested', {i: {j: i * j for j in range(3)} for i in range(2)})
except Exception as e:
    print('SKIP_dict_comp_nested', type(e).__name__, e)

# === Dictionary comprehension - empty ===
try:
    print('dict_comp_empty', {x: x for x in []})
except Exception as e:
    print('SKIP_dict_comp_empty', type(e).__name__, e)

# === Dictionary comprehension - with enumerate ===
try:
    print('dict_comp_enumerate', {i: v for i, v in enumerate(['x', 'y', 'z'])})
except Exception as e:
    print('SKIP_dict_comp_enumerate', type(e).__name__, e)


# === Set comprehension - basic ===
try:
    print('set_comp_basic', {x ** 2 for x in range(10)})
except Exception as e:
    print('SKIP_set_comp_basic', type(e).__name__, e)

# === Set comprehension - from string ===
try:
    print('set_comp_string', {c for c in 'hello world'})
except Exception as e:
    print('SKIP_set_comp_string', type(e).__name__, e)

# === Set comprehension - with condition ===
try:
    print('set_comp_if', {x for x in range(20) if x % 3 == 0})
except Exception as e:
    print('SKIP_set_comp_if', type(e).__name__, e)

# === Set comprehension - multiple for ===
try:
    print('set_comp_multi_for', {x + y for x in [1, 2, 3] for y in [10, 20]})
except Exception as e:
    print('SKIP_set_comp_multi_for', type(e).__name__, e)

# === Set comprehension - with walrus ===
try:
    print('set_comp_walrus', {(y := x * 2) for x in range(5) if y > 3})
except Exception as e:
    print('SKIP_set_comp_walrus', type(e).__name__, e)

# === Set comprehension - duplicates removed ===
try:
    print('set_comp_dedup', {x % 3 for x in range(10)})
except Exception as e:
    print('SKIP_set_comp_dedup', type(e).__name__, e)

# === Set comprehension - empty ===
try:
    print('set_comp_empty', {x for x in []})
except Exception as e:
    print('SKIP_set_comp_empty', type(e).__name__, e)


# === Generator expression - basic ===
try:
    gen = (x * 2 for x in range(5))
    print('gen_exp_basic', list(gen))
except Exception as e:
    print('SKIP_gen_exp_basic', type(e).__name__, e)

# === Generator expression - with condition ===
try:
    gen = (x ** 2 for x in range(10) if x % 2 == 0)
    print('gen_exp_if', list(gen))
except Exception as e:
    print('SKIP_gen_exp_if', type(e).__name__, e)

# === Generator expression - multiple for ===
try:
    gen = ((x, y) for x in [1, 2] for y in ['a', 'b'])
    print('gen_exp_multi_for', list(gen))
except Exception as e:
    print('SKIP_gen_exp_multi_for', type(e).__name__, e)

# === Generator expression - with walrus ===
try:
    gen = ((y := x * 2) for x in range(5) if y > 2)
    print('gen_exp_walrus', list(gen))
except Exception as e:
    print('SKIP_gen_exp_walrus', type(e).__name__, e)

# === Generator expression - lazy evaluation ===
try:
    counter = [0]
    def track(x):
        counter[0] += 1
        return x * 2
    gen = (track(x) for x in range(3))
    print('gen_exp_lazy_before', counter[0])
    result = list(gen)
    print('gen_exp_lazy_after', counter[0], result)
except Exception as e:
    print('SKIP_gen_exp_lazy', type(e).__name__, e)

# === Generator expression - used with sum ===
try:
    print('gen_exp_sum', sum(x ** 2 for x in range(5)))
except Exception as e:
    print('SKIP_gen_exp_sum', type(e).__name__, e)

# === Generator expression - used with max ===
try:
    print('gen_exp_max', max(x * 2 for x in range(10)))
except Exception as e:
    print('SKIP_gen_exp_max', type(e).__name__, e)

# === Generator expression - used with min ===
try:
    print('gen_exp_min', min(x ** 2 for x in range(1, 6)))
except Exception as e:
    print('SKIP_gen_exp_min', type(e).__name__, e)

# === Generator expression - used with any ===
try:
    print('gen_exp_any', any(x > 5 for x in range(10)))
except Exception as e:
    print('SKIP_gen_exp_any', type(e).__name__, e)

# === Generator expression - used with all ===
try:
    print('gen_exp_all', all(x >= 0 for x in range(-5, 5)))
except Exception as e:
    print('SKIP_gen_exp_all', type(e).__name__, e)

# === Generator expression - used with tuple() ===
try:
    print('gen_exp_tuple', tuple(x * 3 for x in range(4)))
except Exception as e:
    print('SKIP_gen_exp_tuple', type(e).__name__, e)

# === Generator expression - used with set() ===
try:
    print('gen_exp_set', set(x % 3 for x in range(10)))
except Exception as e:
    print('SKIP_gen_exp_set', type(e).__name__, e)

# === Generator expression - empty ===
try:
    print('gen_exp_empty', list(x for x in []))
except Exception as e:
    print('SKIP_gen_exp_empty', type(e).__name__, e)


# === Nested comprehension - list of lists ===
try:
    print('nested_list_of_lists', [[i * j for j in range(3)] for i in range(3)])
except Exception as e:
    print('SKIP_nested_list_of_lists', type(e).__name__, e)

# === Nested comprehension - list of dicts ===
try:
    print('nested_list_of_dicts', [{f'key_{j}': j for j in range(3)} for i in range(2)])
except Exception as e:
    print('SKIP_nested_list_of_dicts', type(e).__name__, e)

# === Nested comprehension - dict of lists ===
try:
    print('nested_dict_of_lists', {i: [j for j in range(i + 1)] for i in range(3)})
except Exception as e:
    print('SKIP_nested_dict_of_lists', type(e).__name__, e)

# === Nested comprehension - set of tuples ===
try:
    print('nested_set_tuples', {tuple(range(i)) for i in range(1, 4)})
except Exception as e:
    print('SKIP_nested_set_tuples', type(e).__name__, e)

# === Nested comprehension - three levels ===
try:
    print('nested_three_level', [[[k for k in range(2)] for j in range(2)] for i in range(2)])
except Exception as e:
    print('SKIP_nested_three_level', type(e).__name__, e)

# === Nested comprehension - mixed types ===
try:
    print('nested_mixed', [{i: {j for j in range(2)}} for i in range(2)])
except Exception as e:
    print('SKIP_nested_mixed', type(e).__name__, e)


# === Walrus operator in list comp - filter and use ===
try:
    print('walrus_filter_use', [y for x in range(10) if (y := x ** 2) > 10])
except Exception as e:
    print('SKIP_walrus_filter_use', type(e).__name__, e)

# === Walrus operator in list comp - multiple assignments ===
try:
    print('walrus_multi', [(a := x, b := x * 2, a + b) for x in range(3)])
except Exception as e:
    print('SKIP_walrus_multi', type(e).__name__, e)

# === Walrus operator in dict comp ===
try:
    print('walrus_dict', {k: (v := k * 2) for k in range(3) if (v := k * 2) < 5})
except Exception as e:
    print('SKIP_walrus_dict', type(e).__name__, e)

# === Walrus operator in set comp ===
try:
    print('walrus_set', {(n := i * i) for i in range(5) if (n := i * i) % 2 == 0})
except Exception as e:
    print('SKIP_walrus_set', type(e).__name__, e)

# === Walrus operator in gen exp ===
try:
    print('walrus_gen', list((t := i + 1) for i in range(4) if (t := i + 1) > 2))
except Exception as e:
    print('SKIP_walrus_gen', type(e).__name__, e)


# === Complex - list comp with function call ===
try:
    def transform(x):
        return x * 10 + 1
    print('complex_func_call', [transform(x) for x in range(4)])
except Exception as e:
    print('SKIP_complex_func_call', type(e).__name__, e)

# === Complex - conditional expression in comp ===
try:
    print('complex_cond_expr', [x * 2 if x % 2 == 0 else x * 3 for x in range(6)])
except Exception as e:
    print('SKIP_complex_cond_expr', type(e).__name__, e)

# === Complex - chained conditions ===
try:
    print('complex_chained', [x for x in range(20) if x > 5 if x < 15 if x % 2 == 0])
except Exception as e:
    print('SKIP_complex_chained', type(e).__name__, e)

# === Complex - three for clauses ===
try:
    print('complex_three_for', [(a, b, c) for a in [1] for b in [2] for c in [3]])
except Exception as e:
    print('SKIP_complex_three_for', type(e).__name__, e)

# === Complex - four for clauses ===
try:
    print('complex_four_for', [(w, x, y, z) for w in [1] for x in [2] for y in [3] for z in [4]])
except Exception as e:
    print('SKIP_complex_four_for', type(e).__name__, e)

# === Complex - filter at each level ===
try:
    print('complex_multi_filter', [(a, b) for a in range(5) if a > 2 for b in range(5) if b > 2])
except Exception as e:
    print('SKIP_complex_multi_filter', type(e).__name__, e)

# === Complex - zip in source ===
try:
    print('complex_zip_source', [a + b for a, b in zip([1, 2, 3], [10, 20, 30])])
except Exception as e:
    print('SKIP_complex_zip_source', type(e).__name__, e)

# === Complex - enumerate in source ===
try:
    print('complex_enum_source', [f'{i}:{v}' for i, v in enumerate(['x', 'y', 'z'])])
except Exception as e:
    print('SKIP_complex_enum_source', type(e).__name__, e)

# === Complex - reversed in source ===
try:
    print('complex_reversed', [x for x in reversed([1, 2, 3])])
except Exception as e:
    print('SKIP_complex_reversed', type(e).__name__, e)

# === Complex - sorted in source ===
try:
    print('complex_sorted', [x for x in sorted([3, 1, 2])])
except Exception as e:
    print('SKIP_complex_sorted', type(e).__name__, e)

# === Complex - slice in source ===
try:
    items = [0, 1, 2, 3, 4, 5]
    print('complex_slice', [x for x in items[2:5]])
except Exception as e:
    print('SKIP_complex_slice', type(e).__name__, e)

# === Complex - star expression in source (list) ===
try:
    print('complex_star', [y for y in [*range(3), *range(5, 7)]])
except Exception as e:
    print('SKIP_complex_star', type(e).__name__, e)

# === Complex - unpacking in target ===
try:
    pairs = [(1, 'a'), (2, 'b'), (3, 'c')]
    print('complex_unpack_target', [f'{n}:{c}' for n, c in pairs])
except Exception as e:
    print('SKIP_complex_unpack_target', type(e).__name__, e)

# === Complex - nested unpacking ===
try:
    nested = [((1, 2), 'x'), ((3, 4), 'y')]
    print('complex_nested_unpack', [(a, b, c) for (a, b), c in nested])
except Exception as e:
    print('SKIP_complex_nested_unpack', type(e).__name__, e)

# === Complex - ignore variable ===
try:
    print('complex_ignore', [x for _ in range(3) for x in [_ + 10]])
except Exception as e:
    print('SKIP_complex_ignore', type(e).__name__, e)

# === Complex - attr access in comp ===
try:
    class Item:
        def __init__(self, val):
            self.value = val
    objs = [Item(1), Item(2), Item(3)]
    print('complex_attr_access', [obj.value for obj in objs])
except Exception as e:
    print('SKIP_complex_attr_access', type(e).__name__, e)

# === Complex - index access in comp ===
try:
    data = [{'key': 1}, {'key': 2}, {'key': 3}]
    print('complex_index_access', [d['key'] for d in data])
except Exception as e:
    print('SKIP_complex_index_access', type(e).__name__, e)

# === Complex - method chain in expression ===
try:
    words = ['  hello  ', 'WORLD', '  TeSt  ']
    print('complex_method_chain', [w.strip().lower().upper() for w in words])
except Exception as e:
    print('SKIP_complex_method_chain', type(e).__name__, e)


# === Edge case - single element ===
try:
    print('edge_single', [x * 2 for x in [42]])
except Exception as e:
    print('SKIP_edge_single', type(e).__name__, e)

# === Edge case - zero range ===
try:
    print('edge_zero_range', [x for x in range(0)])
except Exception as e:
    print('SKIP_edge_zero_range', type(e).__name__, e)

# === Edge case - negative range ===
try:
    print('edge_neg_range', [x for x in range(-5, -10)])
except Exception as e:
    print('SKIP_edge_neg_range', type(e).__name__, e)

# === Edge case - large range (just first/last) ===
try:
    large = [x for x in range(1000)]
    print('edge_large_range', large[0], large[-1], len(large))
except Exception as e:
    print('SKIP_edge_large_range', type(e).__name__, e)

# === Edge case - identity ===
try:
    print('edge_identity', [x for x in [1, 2, 3]])
except Exception as e:
    print('SKIP_edge_identity', type(e).__name__, e)

# === Edge case - constant expression ===
try:
    print('edge_const', [42 for _ in range(5)])
except Exception as e:
    print('SKIP_edge_const', type(e).__name__, e)

# === Edge case - None result ===
try:
    print('edge_none', [None for _ in range(3)])
except Exception as e:
    print('SKIP_edge_none', type(e).__name__, e)

# === Edge case - boolean expression ===
try:
    print('edge_bool', [x > 2 for x in range(5)])
except Exception as e:
    print('SKIP_edge_bool', type(e).__name__, e)

# === Edge case - complex numbers ===
try:
    print('edge_complex', [x * 1j for x in range(3)])
except Exception as e:
    print('SKIP_edge_complex', type(e).__name__, e)

# === Edge case - mixed types ===
try:
    print('edge_mixed', [x for x in [1, 'two', 3.0, True, None]])
except Exception as e:
    print('SKIP_edge_mixed', type(e).__name__, e)


# === Scope isolation - variable not leaked ===
try:
    comp_result = [x for x in range(3)]
    try:
        print('scope_check', x)
    except NameError:
        print('scope_isolated', True)
except Exception as e:
    print('SKIP_scope_isolation', type(e).__name__, e)

# === Scope - walrus operator DOES leak ===
try:
    [y := i for i in range(3)]
    print('scope_walrus_leaks', y)
except Exception as e:
    print('SKIP_scope_walrus', type(e).__name__, e)


# === Generator - send not supported (just iteration) ===
try:
    gen = (x for x in range(3))
    print('gen_iter', next(gen), next(gen))
except Exception as e:
    print('SKIP_gen_iter', type(e).__name__, e)

# === Generator - exhausted ===
try:
    gen = (x for x in range(2))
    list(gen)
    try:
        next(gen)
    except StopIteration:
        print('gen_exhausted', True)
except Exception as e:
    print('SKIP_gen_exhausted', type(e).__name__, e)
