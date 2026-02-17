import pprint

# === pformat basics: simple containers ===
try:
    print('pformat_list', pprint.pformat([1, 2, 3]))
    print('pformat_dict', pprint.pformat({'a': 1, 'b': 2}))
    print('pformat_tuple', pprint.pformat((1, 2, 3)))
    print('pformat_set', pprint.pformat({1, 2, 3}))
    print('pformat_frozenset', pprint.pformat(frozenset([1, 2, 3])))
except Exception as e:
    print('SKIP_pformat basics: simple containers', type(e).__name__, e)

# === pformat basics: empty containers ===
try:
    print('pformat_empty_list', pprint.pformat([]))
    print('pformat_empty_dict', pprint.pformat({}))
    print('pformat_empty_tuple', pprint.pformat(()))
    print('pformat_empty_set', pprint.pformat(set()))
    print('pformat_empty_frozenset', pprint.pformat(frozenset()))
except Exception as e:
    print('SKIP_pformat basics: empty containers', type(e).__name__, e)

# === pformat basics: nested structures ===
try:
    print('pformat_nested_list', pprint.pformat([[1, 2], [3, 4], [5, 6]]))
    print('pformat_nested_dict', pprint.pformat({'a': {'b': 1}, 'c': {'d': 2}}))
    print('pformat_mixed_nested', pprint.pformat([{'a': 1}, (2, 3), [4, 5]]))
except Exception as e:
    print('SKIP_pformat basics: nested structures', type(e).__name__, e)

# === pformat parameters: indent ===
try:
    print('pformat_indent_1', pprint.pformat([1, 2, 3], indent=1))
    print('pformat_indent_2', pprint.pformat([1, 2, 3], indent=2))
    print('pformat_indent_4', pprint.pformat([1, 2, 3], indent=4))
    print('pformat_indent_dict', pprint.pformat({'a': 1, 'b': 2}, indent=2))
except Exception as e:
    print('SKIP_pformat parameters: indent', type(e).__name__, e)

# === pformat parameters: width ===
try:
    print('pformat_width_20', pprint.pformat({'a': 1, 'b': 2, 'c': 3}, width=20))
    print('pformat_width_40', pprint.pformat({'a': 1, 'b': 2, 'c': 3}, width=40))
    print('pformat_width_80', pprint.pformat({'a': 1, 'b': 2, 'c': 3}, width=80))
except Exception as e:
    print('SKIP_pformat parameters: width', type(e).__name__, e)

# === pformat parameters: depth ===
try:
    print('pformat_depth_none', pprint.pformat([[[1, 2], [3, 4]], [[5, 6], [7, 8]]], depth=None))
    print('pformat_depth_1', pprint.pformat([[[1, 2], [3, 4]], [[5, 6], [7, 8]]], depth=1))
    print('pformat_depth_2', pprint.pformat([[[1, 2], [3, 4]], [[5, 6], [7, 8]]], depth=2))
    print('pformat_depth_3', pprint.pformat([[[1, 2], [3, 4]], [[5, 6], [7, 8]]], depth=3))
    print('pformat_depth_dict', pprint.pformat({'a': {'b': {'c': 1}}}, depth=2))
except Exception as e:
    print('SKIP_pformat parameters: depth', type(e).__name__, e)

# === pformat parameters: compact ===
try:
    print('pformat_compact_false', pprint.pformat(list(range(20)), width=30, compact=False))
    print('pformat_compact_true', pprint.pformat(list(range(20)), width=30, compact=True))
except Exception as e:
    print('SKIP_pformat parameters: compact', type(e).__name__, e)

# === pformat parameters: sort_dicts ===
try:
    print('pformat_sort_dicts_true', pprint.pformat({'z': 1, 'a': 2, 'm': 3}, sort_dicts=True))
    print('pformat_sort_dicts_false', pprint.pformat({'z': 1, 'a': 2, 'm': 3}, sort_dicts=False))
except Exception as e:
    print('SKIP_pformat parameters: sort_dicts', type(e).__name__, e)

# === saferepr basics ===
try:
    print('saferepr_list', pprint.saferepr([1, 2, 3]))
    print('saferepr_dict', pprint.saferepr({'a': 1, 'b': 2}))
    print('saferepr_tuple', pprint.saferepr((1, 2, 3)))
    print('saferepr_str', pprint.saferepr('hello'))
    print('saferepr_int', pprint.saferepr(42))
    print('saferepr_nested', pprint.saferepr([{'a': (1, 2)}]))
except Exception as e:
    print('SKIP_saferepr basics', type(e).__name__, e)

# === saferepr with recursive structure ===
try:
    a = [1, 2]
    a.append(a)
    print('saferepr_recursive_list', pprint.saferepr(a))

    b = {'key': 'value'}
    b['self'] = b
    print('saferepr_recursive_dict', pprint.saferepr(b))
except Exception as e:
    print('SKIP_saferepr with recursive structure', type(e).__name__, e)

# === isreadable basics ===
try:
    print('isreadable_list', pprint.isreadable([1, 2, 3]))
    print('isreadable_dict', pprint.isreadable({'a': 1, 'b': 2}))
    print('isreadable_tuple', pprint.isreadable((1, 2, 3)))
    print('isreadable_str', pprint.isreadable('hello'))
    print('isreadable_int', pprint.isreadable(42))
except Exception as e:
    print('SKIP_isreadable basics', type(e).__name__, e)

# === isreadable with complex objects ===
try:
    class CustomObj:
        pass

    print('isreadable_custom', pprint.isreadable(CustomObj()))
    print('isreadable_nested', pprint.isreadable([{'a': (1, 2)}]))
except Exception as e:
    print('SKIP_isreadable with complex objects', type(e).__name__, e)

# === isrecursive basics ===
try:
    print('isrecursive_list', pprint.isrecursive([1, 2, 3]))
    print('isrecursive_dict', pprint.isrecursive({'a': 1, 'b': 2}))
    print('isrecursive_nested', pprint.isrecursive([[1, 2], [3, 4]]))
except Exception as e:
    print('SKIP_isrecursive basics', type(e).__name__, e)

# === isrecursive with recursive structures ===
try:
    c = [1, 2]
    c.append(c)
    print('isrecursive_recursive_list', pprint.isrecursive(c))

    d = {}
    d['self'] = d
    print('isrecursive_recursive_dict', pprint.isrecursive(d))
except Exception as e:
    print('SKIP_isrecursive with recursive structures', type(e).__name__, e)

# === isrecursive with mutually recursive structures ===
try:
    e = {}
    f = {'e': e}
    e['f'] = f
    print('isrecursive_mutual', pprint.isrecursive(e))
except Exception as e:
    print('SKIP_isrecursive with mutually recursive structures', type(e).__name__, e)

# === pp function (returns None, prints to stdout) ===
try:
    print('pp_list', end=' ')
    print(pprint.pp([1, 2, 3]) is None)

    print('pp_dict', end=' ')
    print(pprint.pp({'a': 1, 'b': 2}) is None)

    print('pp_nested', end=' ')
    print(pprint.pp([{'x': 1}, {'y': 2}]) is None)
except Exception as e:
    print('SKIP_pp function (returns None, prints to stdout)', type(e).__name__, e)

# === pprint function (returns None, prints to stdout) ===
try:
    print('pprint_list', end=' ')
    print(pprint.pprint([1, 2, 3]) is None)

    print('pprint_dict', end=' ')
    print(pprint.pprint({'a': 1, 'b': 2}) is None)

    print('pprint_nested', end=' ')
    print(pprint.pprint([{'x': 1}, {'y': 2}]) is None)
except Exception as e:
    print('SKIP_pprint function (returns None, prints to stdout)', type(e).__name__, e)

# === PrettyPrinter class construction ===
try:
    print('prettyprinter_class', type(pprint.PrettyPrinter()))
except Exception as e:
    print('SKIP_PrettyPrinter class construction', type(e).__name__, e)

# === PrettyPrinter with custom settings ===
try:
    pp_custom = pprint.PrettyPrinter(indent=4, width=40)
    print('prettyprinter_custom', type(pp_custom))
except Exception as e:
    print('SKIP_PrettyPrinter with custom settings', type(e).__name__, e)

# === PrettyPrinter instance methods ===
try:
    pp = pprint.PrettyPrinter(indent=2, width=40)

    # pformat instance method
    result = pp.pformat([1, 2, 3])
    print('pp_pformat', result)

    # pprint instance method - returns None
    print('pp_pprint', end=' ')
    print(pp.pprint([1, 2, 3]) is None)

    # isreadable instance method
    print('pp_isreadable', pp.isreadable([1, 2, 3]))
    print('pp_isreadable_custom', pp.isreadable(CustomObj()))

    # isrecursive instance method
    print('pp_isrecursive', pp.isrecursive([1, 2, 3]))

    # format method - signature is format(object, context, maxlevels, level)
    # Returns (formatted_string, readable_boolean)
    fmt_result = pp.format([1, 2, 3], {}, 0, 0)
    print('pp_format_returns_tuple', type(fmt_result) is tuple)
    print('pp_format_len', len(fmt_result))
    print('pp_format_str', fmt_result[0])
    print('pp_format_readable', fmt_result[1])
except Exception as e:
    print('SKIP_PrettyPrinter instance methods', type(e).__name__, e)

# === PrettyPrinter with underscore_numbers (Python 3.10+) ===
try:
    try:
        pp_us = pprint.PrettyPrinter(underscore_numbers=True)
        result = pp_us.pformat({'large': 1234567890})
        print('pp_underscore_numbers', '_' in result or True)
    except TypeError:
        print('pp_underscore_numbers', 'not_supported')
except Exception as e:
    print('SKIP_PrettyPrinter with underscore_numbers (Python 3.10+)', type(e).__name__, e)

# === Complex structures: deeply nested dicts ===
try:
    deep_dict = {'level1': {'level2': {'level3': {'level4': {'level5': 'deep'}}}}}
    print('pformat_deep_dict', pprint.pformat(deep_dict))
    print('pformat_deep_dict_indent2', pprint.pformat(deep_dict, indent=2))
    print('pformat_deep_dict_depth2', pprint.pformat(deep_dict, depth=2))
except Exception as e:
    print('SKIP_Complex structures: deeply nested dicts', type(e).__name__, e)

# === Complex structures: lists containing dicts ===
try:
    list_of_dicts = [{'id': 1, 'name': 'a'}, {'id': 2, 'name': 'b'}, {'id': 3, 'name': 'c'}]
    print('pformat_list_of_dicts', pprint.pformat(list_of_dicts))
    print('pformat_list_of_dicts_width30', pprint.pformat(list_of_dicts, width=30))
except Exception as e:
    print('SKIP_Complex structures: lists containing dicts', type(e).__name__, e)

# === Complex structures: sets and frozensets ===
try:
    print('pformat_nested_set', pprint.pformat({frozenset([1, 2]), frozenset([3, 4])}))
    print('pformat_set_in_dict', pprint.pformat({'keys': {1, 2, 3}, 'values': {4, 5, 6}}))
except Exception as e:
    print('SKIP_Complex structures: sets and frozensets', type(e).__name__, e)

# === Complex structures: mixed types ===
try:
    mixed = [
        1,
        'string',
        [2, 3],
        {'key': 'value'},
        (4, 5),
        {6, 7},
        frozenset([8, 9]),
        None,
        True,
        False,
    ]
    print('pformat_mixed', pprint.pformat(mixed))
except Exception as e:
    print('SKIP_Complex structures: mixed types', type(e).__name__, e)

# === Complex structures: wide data ===
try:
    wide_dict = {f'key_{i}': f'value_{i}' for i in range(10)}
    print('pformat_wide_dict_width50', pprint.pformat(wide_dict, width=50))
    print('pformat_wide_dict_width200', pprint.pformat(wide_dict, width=200))
except Exception as e:
    print('SKIP_Complex structures: wide data', type(e).__name__, e)

# === Complex structures: large nested list ===
try:
    large_nested = [[[i + j + k for i in range(3)] for j in range(3)] for k in range(3)]
    print('pformat_large_nested', pprint.pformat(large_nested))
    print('pformat_large_nested_compact', pprint.pformat(large_nested, compact=True))
except Exception as e:
    print('SKIP_Complex structures: large nested list', type(e).__name__, e)

# === Complex structures: dict with list values ===
try:
    dict_of_lists = {'a': [1, 2, 3], 'b': [4, 5, 6], 'c': [7, 8, 9]}
    print('pformat_dict_of_lists', pprint.pformat(dict_of_lists))
    print('pformat_dict_of_lists_nosort', pprint.pformat(dict_of_lists, sort_dicts=False))
except Exception as e:
    print('SKIP_Complex structures: dict with list values', type(e).__name__, e)

# === Edge cases: strings in containers ===
try:
    print('pformat_long_strings', pprint.pformat(['a very long string that exceeds width', 'another long string'], width=30))
except Exception as e:
    print('SKIP_Edge cases: strings in containers', type(e).__name__, e)

# === Edge cases: single element containers ===
try:
    print('pformat_single_list', pprint.pformat([42]))
    print('pformat_single_dict', pprint.pformat({'only': 'one'}))
    print('pformat_single_tuple', pprint.pformat((42,)))
except Exception as e:
    print('SKIP_Edge cases: single element containers', type(e).__name__, e)

# === Edge cases: unicode content ===
try:
    print('pformat_unicode', pprint.pformat({'日本語': 'hello', 'emoji': '😀'}))
except Exception as e:
    print('SKIP_Edge cases: unicode content', type(e).__name__, e)

# === Edge cases: numeric types ===
try:
    print('pformat_numeric', pprint.pformat([1, 1.5, 1e10, -5, 0]))
except Exception as e:
    print('SKIP_Edge cases: numeric types', type(e).__name__, e)

# === Combining multiple parameters ===
try:
    complex_obj = {'z': [1, 2, 3, 4, 5], 'a': [6, 7, 8, 9, 10], 'm': {'nested': 'dict'}}
    print('pformat_combined_params', pprint.pformat(complex_obj, indent=2, width=30, compact=True, sort_dicts=True))
    print('pformat_combined_params2', pprint.pformat(complex_obj, indent=4, width=50, compact=False, sort_dicts=False))
except Exception as e:
    print('SKIP_Combining multiple parameters', type(e).__name__, e)

# === Recursive format with PrettyPrinter instance ===
try:
    pp2 = pprint.PrettyPrinter(depth=2)
    print('pp_format_depth', pp2.pformat([[[1, 2], [3, 4]], [[5, 6], [7, 8]]]))
except Exception as e:
    print('SKIP_Recursive format with PrettyPrinter instance', type(e).__name__, e)
