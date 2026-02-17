import builtins

# === abs ===
try:
    print('abs_positive', abs(5))
    print('abs_negative', abs(-5))
    print('abs_float', abs(-3.14))
    print('abs_zero', abs(0))
except Exception as e:
    print('SKIP_abs', type(e).__name__, e)

# === aiter (Python 3.10+) ===
try:
    async def async_gen():
        yield 1
        yield 2
    gen = async_gen()
    print('aiter_type', type(aiter(gen)))
except Exception as e:
    print('SKIP_aiter_(python_3.10+)', type(e).__name__, e)

# === all ===
try:
    print('all_empty', all([]))
    print('all_true', all([1, 2, 3]))
    print('all_false', all([1, 0, 3]))
    print('all_all_true', all([True, True]))
    print('all_with_false', all([True, False]))
except Exception as e:
    print('SKIP_all', type(e).__name__, e)

# === anext (Python 3.10+) ===
try:
    async def simple_async():
        yield 42
    agen = simple_async()
    try:
        print('anext_default', anext(agen, 'default'))
    except StopAsyncIteration:
        print('anext_exhausted', 'exhausted')
except Exception as e:
    print('SKIP_anext_(python_3.10+)', type(e).__name__, e)

# === any ===
try:
    print('any_empty', any([]))
    print('any_true', any([0, 0, 1]))
    print('any_false', any([0, 0, 0]))
    print('any_all_false', any([False, False]))
    print('any_one_true', any([False, True]))
except Exception as e:
    print('SKIP_any', type(e).__name__, e)

# === ascii ===
try:
    print('ascii_string', ascii('hello'))
    print('ascii_unicode', ascii('café'))
    print('ascii_list', ascii([1, 2, 3]))
except Exception as e:
    print('SKIP_ascii', type(e).__name__, e)

# === bin ===
try:
    print('bin_positive', bin(10))
    print('bin_negative', bin(-5))
    print('bin_zero', bin(0))
except Exception as e:
    print('SKIP_bin', type(e).__name__, e)

# === bool ===
try:
    print('bool_true', bool(1))
    print('bool_false', bool(0))
    print('bool_empty_str', bool(''))
    print('bool_nonempty_str', bool('hello'))
    print('bool_empty_list', bool([]))
    print('bool_nonempty_list', bool([1, 2]))
except Exception as e:
    print('SKIP_bool', type(e).__name__, e)

# === bytearray ===
try:
    print('bytearray_empty', bytearray())
    print('bytearray_int', bytearray(5))
    print('bytearray_str', bytearray('hello', 'utf-8'))
    print('bytearray_iterable', bytearray([65, 66, 67]))
except Exception as e:
    print('SKIP_bytearray', type(e).__name__, e)

# === bytes ===
try:
    print('bytes_empty', bytes())
    print('bytes_int', bytes(5))
    print('bytes_str', bytes('hello', 'utf-8'))
    print('bytes_iterable', bytes([65, 66, 67]))
except Exception as e:
    print('SKIP_bytes', type(e).__name__, e)

# === callable ===
try:
    print('callable_func', callable(print))
    print('callable_class', callable(int))
    print('callable_str', callable('hello'))
    print('callable_int', callable(42))
except Exception as e:
    print('SKIP_callable', type(e).__name__, e)

# === chr ===
try:
    print('chr_upper_a', chr(65))
    print('chr_lower_a', chr(97))
    print('chr_zero', chr(48))
    print('chr_unicode', chr(8364))  # Euro sign
except Exception as e:
    print('SKIP_chr', type(e).__name__, e)

# === classmethod ===
try:
    class MyClass:
        @classmethod
        def my_method(cls):
            return cls.__name__
    print('classmethod_result', MyClass.my_method())
    print('classmethod_type', type(classmethod(lambda cls: None)))
except Exception as e:
    print('SKIP_classmethod', type(e).__name__, e)

# === compile ===
try:
    code = compile('x + 1', '<string>', 'eval')
    print('compile_type', type(code))
    print('compile_result', eval(code, {'x': 5}))
except Exception as e:
    print('SKIP_compile', type(e).__name__, e)

# === complex ===
try:
    print('complex_int', complex(3))
    print('complex_floats', complex(3.0, 4.0))
    print('complex_str', complex('3+4j'))
    print('complex_zero', complex(0))
except Exception as e:
    print('SKIP_complex', type(e).__name__, e)

# === delattr ===
try:
    class Temp:
        attr = 5
        pass
    t = Temp()
    t.x = 10
    print('delattr_before', hasattr(t, 'x'))
    delattr(t, 'x')
    print('delattr_after', hasattr(t, 'x'))
except Exception as e:
    print('SKIP_delattr', type(e).__name__, e)

# === dict ===
try:
    print('dict_empty', dict())
    print('dict_kwargs', dict(a=1, b=2))
    print('dict_list_tuples', dict([('a', 1), ('b', 2)]))
    print('dict_fromkeys', dict.fromkeys(['a', 'b'], 0))
except Exception as e:
    print('SKIP_dict', type(e).__name__, e)

# === dir ===
try:
    print('dir_module_len', len(dir(builtins)))
    print('dir_object', dir([])[:3])  # First 3
    print('dir_empty', dir())
except Exception as e:
    print('SKIP_dir', type(e).__name__, e)

# === divmod ===
try:
    print('divmod_positive', divmod(10, 3))
    print('divmod_negative', divmod(-10, 3))
    print('divmod_zero_remainder', divmod(10, 2))
    print('divmod_float', divmod(10.5, 2.5))
except Exception as e:
    print('SKIP_divmod', type(e).__name__, e)

# === enumerate ===
try:
    print('enumerate_basic', list(enumerate(['a', 'b', 'c'])))
    print('enumerate_start', list(enumerate(['a', 'b'], start=5)))
except Exception as e:
    print('SKIP_enumerate', type(e).__name__, e)

# === eval ===
try:
    print('eval_expr', eval('2 + 3'))
    print('eval_with_vars', eval('x + 1', {'x': 5}))
except Exception as e:
    print('SKIP_eval', type(e).__name__, e)

# === exec ===
try:
    namespace = {}
    exec('y = 10', namespace)
    print('exec_result', namespace.get('y'))
except Exception as e:
    print('SKIP_exec', type(e).__name__, e)

# === filter ===
try:
    print('filter_basic', list(filter(lambda x: x > 0, [-1, 0, 1, 2])))
    print('filter_none', list(filter(None, [0, 1, '', 'a', None, True])))
    print('filter_empty', list(filter(lambda x: x > 5, [1, 2, 3])))
except Exception as e:
    print('SKIP_filter', type(e).__name__, e)

# === float ===
try:
    print('float_int', float(5))
    print('float_str', float('3.14'))
    print('float_inf', float('inf'))
    print('float_nan', float('nan'))
except Exception as e:
    print('SKIP_float', type(e).__name__, e)

# === format ===
try:
    print('format_int', format(12345, ','))
    print('format_float', format(3.14159, '.2f'))
    print('format_str', format('hello', '>10'))
    print('format_binary', format(10, 'b'))
except Exception as e:
    print('SKIP_format', type(e).__name__, e)

# === frozenset ===
try:
    print('frozenset_empty', frozenset())
    print('frozenset_list', frozenset([1, 2, 3]))
    print('frozenset_string', frozenset('hello'))
except Exception as e:
    print('SKIP_frozenset', type(e).__name__, e)

# === getattr ===
try:
    class Obj:
        x = 5
    o = Obj()
    print('getattr_existing', getattr(o, 'x'))
    print('getattr_default', getattr(o, 'y', 'default'))
except Exception as e:
    print('SKIP_getattr', type(e).__name__, e)

# === globals ===
try:
    print('globals_has_builtins', '__builtins__' in globals())
except Exception as e:
    print('SKIP_globals', type(e).__name__, e)

# === hasattr ===
try:
    print('hasattr_true', hasattr(o, 'x'))
    print('hasattr_false', hasattr(o, 'nonexistent'))
except Exception as e:
    print('SKIP_hasattr', type(e).__name__, e)

# === hash ===
try:
    print('hash_int', hash(42))
    print('hash_str', hash('hello'))
    print('hash_tuple', hash((1, 2, 3)))
except Exception as e:
    print('SKIP_hash', type(e).__name__, e)

# === hex ===
try:
    print('hex_positive', hex(255))
    print('hex_negative', hex(-42))
    print('hex_zero', hex(0))
except Exception as e:
    print('SKIP_hex', type(e).__name__, e)

# === id ===
try:
    a = object()
    print('id_type', type(id(a)))
    print('id_same_obj', id(a) == id(a))
except Exception as e:
    print('SKIP_id', type(e).__name__, e)

# === input === (skip - requires interactive)
# Can't test input() in non-interactive mode
try:
    print('input_is_builtin', 'input' in dir(builtins))
except Exception as e:
    print('SKIP_input_(skip_-_requires_interactive)', type(e).__name__, e)

# === int ===
try:
    print('int_float', int(3.7))
    print('int_str', int('42'))
    print('int_str_base', int('ff', 16))
    print('int_binary', int('1010', 2))
    print('int_zero', int())
except Exception as e:
    print('SKIP_int', type(e).__name__, e)

# === isinstance ===
try:
    print('isinstance_true', isinstance(5, int))
    print('isinstance_false', isinstance(5, str))
    print('isinstance_tuple', isinstance(5, (int, str)))
    print('isinstance_list', isinstance([], list))
except Exception as e:
    print('SKIP_isinstance', type(e).__name__, e)

# === issubclass ===
try:
    print('issubclass_true', issubclass(int, object))
    print('issubclass_false', issubclass(str, int))
    print('issubclass_tuple', issubclass(int, (int, str)))
except Exception as e:
    print('SKIP_issubclass', type(e).__name__, e)

# === iter ===
try:
    print('iter_list', next(iter([1, 2, 3])))

    # iter with sentinel - first arg must be callable
    def read_until_stop():
        vals = [1, 2, 3]
        read_until_stop.idx = getattr(read_until_stop, 'idx', 0)
        if read_until_stop.idx < len(vals):
            v = vals[read_until_stop.idx]
            read_until_stop.idx += 1
            return v
        return None
    print('iter_sentinel', list(iter(read_until_stop, None)))
except Exception as e:
    print('SKIP_iter', type(e).__name__, e)

# === len ===
try:
    print('len_list', len([1, 2, 3]))
    print('len_str', len('hello'))
    print('len_empty', len(''))
    print('len_dict', len({'a': 1, 'b': 2}))
except Exception as e:
    print('SKIP_len', type(e).__name__, e)

# === list ===
try:
    print('list_empty', list())
    print('list_str', list('abc'))
    print('list_range', list(range(5)))
except Exception as e:
    print('SKIP_list', type(e).__name__, e)

# === locals ===
try:
    def test_locals():
        local_var = 42
        return locals()['local_var']
    print('locals_result', test_locals())
except Exception as e:
    print('SKIP_locals', type(e).__name__, e)

# === map ===
try:
    print('map_basic', list(map(lambda x: x * 2, [1, 2, 3])))
    print('map_multiple', list(map(lambda x, y: x + y, [1, 2, 3], [4, 5, 6])))
    print('map_empty', list(map(str, [])))
except Exception as e:
    print('SKIP_map', type(e).__name__, e)

# === max ===
try:
    print('max_list', max([1, 5, 3]))
    print('max_args', max(1, 5, 3))
    print('max_str', max('apple', 'banana', 'cherry'))
    print('max_key', max([1, 2, 3, 4], key=lambda x: -x))
except Exception as e:
    print('SKIP_max', type(e).__name__, e)

# === memoryview ===
try:
    data = b'hello'
    mv = memoryview(data)
    print('memoryview_type', type(mv))
    print('memoryview_len', len(mv))
    print('memoryview_item', mv[0])
except Exception as e:
    print('SKIP_memoryview', type(e).__name__, e)

# === min ===
try:
    print('min_list', min([5, 1, 3]))
    print('min_args', min(5, 1, 3))
    print('min_str', min('banana', 'apple', 'cherry'))
    print('min_key', min([1, 2, 3, 4], key=lambda x: -x))
except Exception as e:
    print('SKIP_min', type(e).__name__, e)

# === next ===
try:
    iterator = iter([1, 2, 3])
    print('next_first', next(iterator))
    print('next_second', next(iterator))
    print('next_default', next(iterator, 'default'))
    print('next_default_exhausted', next(iterator, 'default'))
except Exception as e:
    print('SKIP_next', type(e).__name__, e)

# === object ===
try:
    print('object_type', type(object()))
    o = object()
    print('object_str', str(o)[:30])  # Just show start
except Exception as e:
    print('SKIP_object', type(e).__name__, e)

# === oct ===
try:
    print('oct_positive', oct(8))
    print('oct_negative', oct(-8))
    print('oct_zero', oct(0))
except Exception as e:
    print('SKIP_oct', type(e).__name__, e)

# === open === (test with a temp file)
try:
    import tempfile
    import os
    with tempfile.NamedTemporaryFile(mode='w', delete=False) as f:
        f.write('test content')
        temp_path = f.name
    try:
        with open(temp_path, 'r') as f:
            print('open_read', f.read())
    finally:
        os.unlink(temp_path)
except Exception as e:
    print('SKIP_open_(test_with_a_temp_file)', type(e).__name__, e)

# === ord ===
try:
    print('ord_upper_a', ord('A'))
    print('ord_lower_a', ord('a'))
    print('ord_zero', ord('0'))
    print('ord_unicode', ord('€'))  # Euro sign
except Exception as e:
    print('SKIP_ord', type(e).__name__, e)

# === pow ===
try:
    print('pow_basic', pow(2, 3))
    print('pow_mod', pow(2, 3, 5))
    print('pow_negative_exp', pow(2, -1))
    print('pow_modulo', pow(10, 2, 7))
except Exception as e:
    print('SKIP_pow', type(e).__name__, e)

# === print ===
try:
    print('print_basic', 'hello')
    print('print_multiple', 1, 2, 3)
except Exception as e:
    print('SKIP_print', type(e).__name__, e)

# === property ===
try:
    class PropertyTest:
        def __init__(self):
            self._x = 0
        @property
        def x(self):
            return self._x
        @x.setter
        def x(self, value):
            self._x = value

    pt = PropertyTest()
    pt.x = 10
    print('property_get', pt.x)
except Exception as e:
    print('SKIP_property', type(e).__name__, e)

# === range ===
try:
    print('range_basic', list(range(5)))
    print('range_start', list(range(2, 8)))
    print('range_step', list(range(0, 10, 2)))
    print('range_negative_step', list(range(10, 0, -2)))
except Exception as e:
    print('SKIP_range', type(e).__name__, e)

# === repr ===
try:
    print('repr_int', repr(42))
    print('repr_str', repr('hello'))
    print('repr_list', repr([1, 2, 3]))
except Exception as e:
    print('SKIP_repr', type(e).__name__, e)

# === reversed ===
try:
    print('reversed_list', list(reversed([1, 2, 3])))
    print('reversed_str', list(reversed('abc')))
    print('reversed_tuple', list(reversed((1, 2, 3))))
except Exception as e:
    print('SKIP_reversed', type(e).__name__, e)

# === round ===
try:
    print('round_int', round(3.7))
    print('round_down', round(3.2))
    print('round_half', round(2.5))
    print('round_ndigits', round(3.14159, 2))
except Exception as e:
    print('SKIP_round', type(e).__name__, e)

# === set ===
try:
    print('set_empty', set())
    print('set_list', set([1, 2, 3]))
    print('set_str', set('hello'))
except Exception as e:
    print('SKIP_set', type(e).__name__, e)

# === setattr ===
try:
    class SetattrTest:
        pass
    st = SetattrTest()
    setattr(st, 'x', 100)
    print('setattr_result', st.x)
except Exception as e:
    print('SKIP_setattr', type(e).__name__, e)

# === slice ===
try:
    print('slice_basic', slice(5))
    print('slice_start_stop', slice(2, 8))
    print('slice_all', slice(1, 10, 2))
    s = slice(2, 5)
    print('slice_indices', s.indices(10))
except Exception as e:
    print('SKIP_slice', type(e).__name__, e)

# === sorted ===
try:
    print('sorted_asc', sorted([3, 1, 4, 1, 5]))
    print('sorted_desc', sorted([3, 1, 4, 1, 5], reverse=True))
    print('sorted_key', sorted(['banana', 'pie', 'Washington'], key=len))
except Exception as e:
    print('SKIP_sorted', type(e).__name__, e)

# === staticmethod ===
try:
    class StaticTest:
        @staticmethod
        def static_method():
            return 'static'
    print('staticmethod_result', StaticTest.static_method())
except Exception as e:
    print('SKIP_staticmethod', type(e).__name__, e)

# === str ===
try:
    print('str_int', str(42))
    print('str_float', str(3.14))
    print('str_list', str([1, 2, 3]))
    print('str_empty', str())
except Exception as e:
    print('SKIP_str', type(e).__name__, e)

# === sum ===
try:
    print('sum_list', sum([1, 2, 3, 4, 5]))
    print('sum_start', sum([1, 2, 3], 10))
    print('sum_empty', sum([]))
    print('sum_floats', sum([0.1, 0.1, 0.1], 0.0))
except Exception as e:
    print('SKIP_sum', type(e).__name__, e)

# === super ===
try:
    class Base:
        def method(self):
            return 'base'
    class Derived(Base):
        def method(self):
            return super().method() + '-derived'
    d = Derived()
    print('super_result', d.method())
except Exception as e:
    print('SKIP_super', type(e).__name__, e)

# === tuple ===
try:
    print('tuple_empty', tuple())
    print('tuple_list', tuple([1, 2, 3]))
    print('tuple_str', tuple('abc'))
except Exception as e:
    print('SKIP_tuple', type(e).__name__, e)

# === type ===
try:
    print('type_int', type(5))
    print('type_str', type('hello'))
    print('type_list', type([]))

    # Create a class dynamically
    NewClass = type('NewClass', (), {'x': 10})
    print('type_new_class', NewClass().x)
except Exception as e:
    print('SKIP_type', type(e).__name__, e)

# === vars ===
try:
    class VarsTest:
        def __init__(self):
            self.a = 1
            self.b = 2
    vt = VarsTest()
    print('vars_instance', vars(vt))
    print('vars_module_keys', list(vars(builtins).keys())[:3])
except Exception as e:
    print('SKIP_vars', type(e).__name__, e)

# === zip ===
try:
    print('zip_basic', list(zip([1, 2, 3], ['a', 'b', 'c'])))
    print('zip_three', list(zip([1, 2], ['a', 'b'], [True, False])))
    print('zip_unequal', list(zip([1, 2, 3], ['a', 'b'])))
    print('zip_empty', list(zip([], [])))
except Exception as e:
    print('SKIP_zip', type(e).__name__, e)

# === breakpoint === (just verify it exists)
try:
    print('breakpoint_exists', callable(breakpoint))
except Exception as e:
    print('SKIP_breakpoint_(just_verify_it_exists)', type(e).__name__, e)

# === help === (just verify it exists - actually calling it is interactive)
try:
    print('help_exists', callable(help))
except Exception as e:
    print('SKIP_help_(just_verify_it_exists_-_actually_calling_it_is_interactive)', type(e).__name__, e)
