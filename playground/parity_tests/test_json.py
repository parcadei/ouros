import json
import io
import math

# === dumps basic types ===
try:
    print('dumps_none', json.dumps(None))
    print('dumps_true', json.dumps(True))
    print('dumps_false', json.dumps(False))
    print('dumps_int', json.dumps(42))
    print('dumps_float', json.dumps(3.14))
    print('dumps_float_int', json.dumps(3.0))
    print('dumps_str', json.dumps('hello'))
    print('dumps_empty_str', json.dumps(''))
    print('dumps_list', json.dumps([1, 2, 3]))
    print('dumps_empty_list', json.dumps([]))
    print('dumps_dict', json.dumps({'a': 1, 'b': 2}))
    print('dumps_empty_dict', json.dumps({}))
    print('dumps_tuple', json.dumps((1, 2, 3)))
    print('dumps_nested', json.dumps({'a': [1, 2, {'b': 3}]}))
except Exception as e:
    print('SKIP_dumps_basic_types', type(e).__name__, e)

# === dumps string escaping ===
try:
    print('dumps_quote', json.dumps('"foo"'))
    print('dumps_backslash', json.dumps('\\'))
    print('dumps_backspace', json.dumps('\b'))
    print('dumps_formfeed', json.dumps('\f'))
    print('dumps_newline', json.dumps('\n'))
    print('dumps_carriage', json.dumps('\r'))
    print('dumps_tab', json.dumps('\t'))
    print('dumps_unicode', json.dumps('\u1234'))
    print('dumps_unicode_escaped', json.dumps('\u0000'))
except Exception as e:
    print('SKIP_dumps_string_escaping', type(e).__name__, e)

# === dumps special floats ===
try:
    print('dumps_nan', json.dumps(float('nan')))
    print('dumps_inf', json.dumps(float('inf')))
    print('dumps_neg_inf', json.dumps(float('-inf')))
except Exception as e:
    print('SKIP_dumps_special_floats', type(e).__name__, e)

# === dumps skipkeys parameter ===
try:
    print('dumps_skipkeys_true', json.dumps({(1, 2): 'tuple_key'}, skipkeys=True))
    print('dumps_skipkeys_false', json.dumps({'valid': 'key'}))
except Exception as e:
    print('SKIP_dumps_skipkeys_parameter', type(e).__name__, e)

# === dumps ensure_ascii parameter ===
try:
    print('dumps_ensure_ascii_true', json.dumps('café', ensure_ascii=True))
    print('dumps_ensure_ascii_false', json.dumps('café', ensure_ascii=False))
    print('dumps_ensure_ascii_emoji', json.dumps('🎉', ensure_ascii=True))
    print('dumps_ensure_ascii_emoji_false', json.dumps('🎉', ensure_ascii=False))
except Exception as e:
    print('SKIP_dumps_ensure_ascii_parameter', type(e).__name__, e)

# === dumps check_circular parameter ===
try:
    a = [1]
    a.append(a)  # circular reference
    try:
        result = json.dumps(a, check_circular=True)
        print('dumps_check_circular_true', 'should_have_raised')
    except (ValueError, RecursionError) as e:
        print('dumps_check_circular_true', type(e).__name__)

    b = [2]
    b.append(b)
    try:
        result = json.dumps(b, check_circular=False)
        print('dumps_check_circular_false', 'unexpected_success')
    except RecursionError:
        print('dumps_check_circular_false', 'RecursionError')
except Exception as e:
    print('SKIP_dumps_check_circular_parameter', type(e).__name__, e)

# === dumps allow_nan parameter ===
try:
    print('dumps_allow_nan_true', json.dumps(float('nan'), allow_nan=True))
    print('dumps_allow_nan_true_inf', json.dumps(float('inf'), allow_nan=True))
    try:
        json.dumps(float('nan'), allow_nan=False)
        print('dumps_allow_nan_false', 'unexpected_success')
    except ValueError:
        print('dumps_allow_nan_false', 'ValueError')

    try:
        json.dumps(float('inf'), allow_nan=False)
        print('dumps_allow_nan_false_inf', 'unexpected_success')
    except ValueError:
        print('dumps_allow_nan_false_inf', 'ValueError')
except Exception as e:
    print('SKIP_dumps_allow_nan_parameter', type(e).__name__, e)

# === dumps cls parameter ===
try:
    class CustomEncoder(json.JSONEncoder):
        def default(self, obj):
            if isinstance(obj, complex):
                return {'real': obj.real, 'imag': obj.imag}
            return super().default(obj)

    print('dumps_cls', json.dumps(1 + 2j, cls=CustomEncoder))
except Exception as e:
    print('SKIP_dumps_cls_parameter', type(e).__name__, e)

# === dumps indent parameter ===
try:
    print('dumps_indent_none', json.dumps({'a': 1, 'b': 2}))
    print('dumps_indent_int', json.dumps({'a': 1, 'b': 2}, indent=2))
    print('dumps_indent_zero', json.dumps({'a': 1}, indent=0))
    print('dumps_indent_str', json.dumps({'a': 1, 'b': 2}, indent='\t'))
    print('dumps_indent_empty', json.dumps({'a': 1}, indent=''))
except Exception as e:
    print('SKIP_dumps_indent_parameter', type(e).__name__, e)

# === dumps separators parameter ===
try:
    print('dumps_separators_compact', json.dumps([1, 2, 3], separators=(',', ':')))
    print('dumps_separators_space', json.dumps([1, 2, 3], separators=(', ', ': ')))
    print('dumps_separators_custom', json.dumps([1, 2, 3], separators=(' | ', ' = ')))
    print('dumps_separators_with_indent', json.dumps([1, 2], indent=2, separators=(',', ': ')))
except Exception as e:
    print('SKIP_dumps_separators_parameter', type(e).__name__, e)

# === dumps default parameter ===
try:
    def custom_default(obj):
        if isinstance(obj, set):
            return list(obj)
        raise TypeError(f'Cannot serialize {type(obj)}')

    print('dumps_default', json.dumps({1, 2, 3}, default=custom_default))

    try:
        json.dumps(object(), default=None)
        print('dumps_default_none', 'unexpected_success')
    except TypeError:
        print('dumps_default_none', 'TypeError')
except Exception as e:
    print('SKIP_dumps_default_parameter', type(e).__name__, e)

# === dumps sort_keys parameter ===
try:
    print('dumps_sort_keys_false', json.dumps({'c': 1, 'a': 2, 'b': 3}, sort_keys=False))
    print('dumps_sort_keys_true', json.dumps({'c': 1, 'a': 2, 'b': 3}, sort_keys=True))
except Exception as e:
    print('SKIP_dumps_sort_keys_parameter', type(e).__name__, e)

# === loads basic types ===
try:
    print('loads_null', json.loads('null'))
    print('loads_true', json.loads('true'))
    print('loads_false', json.loads('false'))
    print('loads_int', json.loads('42'))
    print('loads_float', json.loads('3.14'))
    print('loads_str', json.loads('"hello"'))
    print('loads_empty_str', json.loads('""'))
    print('loads_list', json.loads('[1, 2, 3]'))
    print('loads_empty_list', json.loads('[]'))
    print('loads_dict', json.loads('{"a": 1, "b": 2}'))
    print('loads_empty_dict', json.loads('{}'))
    print('loads_nested', json.loads('{"a": [1, 2, {"b": 3}]}'))
except Exception as e:
    print('SKIP_loads_basic_types', type(e).__name__, e)

# === loads whitespace handling ===
try:
    print('loads_whitespace', json.loads('  \n\t  {  "a"  :  1  }  \n  '))
except Exception as e:
    print('SKIP_loads_whitespace_handling', type(e).__name__, e)

# === loads string escaping ===
try:
    print('loads_escaped_quote', json.loads('"\\"foo\\""'))
    print('loads_escaped_backslash', json.loads('"\\\\"'))
    print('loads_escaped_newline', json.loads('"\\n"'))
    print('loads_escaped_tab', json.loads('"\\t"'))
    print('loads_escaped_unicode', json.loads('"\\u0041"'))
    print('loads_escaped_unicode_1234', json.loads('"\\u1234"'))
except Exception as e:
    print('SKIP_loads_string_escaping', type(e).__name__, e)

# === loads special floats ===
try:
    print('loads_nan', json.loads('NaN'))
    print('loads_infinity', json.loads('Infinity'))
    print('loads_neg_infinity', json.loads('-Infinity'))
except Exception as e:
    print('SKIP_loads_special_floats', type(e).__name__, e)

# === loads cls parameter ===
try:
    class CustomDecoder(json.JSONDecoder):
        def __init__(self, *args, **kwargs):
            super().__init__(object_hook=self.dict_to_complex, *args, **kwargs)

        def dict_to_complex(self, dct):
            if 'real' in dct and 'imag' in dct:
                return complex(dct['real'], dct['imag'])
            return dct

    print('loads_cls', json.loads('{"real": 1, "imag": 2}', cls=CustomDecoder))
except Exception as e:
    print('SKIP_loads_cls_parameter', type(e).__name__, e)

# === loads object_hook parameter ===
try:
    def as_complex(dct):
        if '__complex__' in dct:
            return complex(dct['real'], dct['imag'])
        return dct

    print('loads_object_hook', json.loads('{"__complex__": true, "real": 1, "imag": 2}', object_hook=as_complex))
except Exception as e:
    print('SKIP_loads_object_hook_parameter', type(e).__name__, e)

# === loads parse_float parameter ===
try:
    from decimal import Decimal
    print('loads_parse_float', json.loads('1.1', parse_float=Decimal))
    print('loads_parse_float_str', json.loads('{"a": 1.5}', parse_float=lambda x: float(x) * 2))
except Exception as e:
    print('SKIP_loads_parse_float_parameter', type(e).__name__, e)

# === loads parse_int parameter ===
try:
    print('loads_parse_int', json.loads('42', parse_int=lambda x: int(x) * 10))
    print('loads_parse_int_str', json.loads('{"a": 5}', parse_int=lambda x: int(x) + 100))
except Exception as e:
    print('SKIP_loads_parse_int_parameter', type(e).__name__, e)

# === loads parse_constant parameter ===
try:
    def handle_constant(const):
        if const == 'NaN':
            return 'not_a_number'
        elif const == 'Infinity':
            return 'positive_infinity'
        elif const == '-Infinity':
            return 'negative_infinity'
        return const

    print('loads_parse_constant_nan', json.loads('NaN', parse_constant=handle_constant))
    print('loads_parse_constant_inf', json.loads('Infinity', parse_constant=handle_constant))
    print('loads_parse_constant_neg_inf', json.loads('-Infinity', parse_constant=handle_constant))
except Exception as e:
    print('SKIP_loads_parse_constant_parameter', type(e).__name__, e)

# === loads object_pairs_hook parameter ===
try:
    def dict_to_tuple_list(pairs):
        return [('key', k, 'value', v) for k, v in pairs]

    print('loads_object_pairs_hook', json.loads('{"a": 1, "b": 2}', object_pairs_hook=dict_to_tuple_list))
except Exception as e:
    print('SKIP_loads_object_pairs_hook_parameter', type(e).__name__, e)

# === dump function (file-like object) ===
try:
    fp1 = io.StringIO()
    json.dump({'a': 1, 'b': 2}, fp1)
    print('dump_basic', fp1.getvalue())

    fp2 = io.StringIO()
    json.dump([1, 2, 3], fp2, indent=2)
    print('dump_indent', fp2.getvalue())

    fp3 = io.StringIO()
    json.dump({'c': 1, 'a': 2}, fp3, sort_keys=True)
    print('dump_sort_keys', fp3.getvalue())

    fp4 = io.StringIO()
    json.dump('café', fp4, ensure_ascii=False)
    print('dump_ensure_ascii_false', fp4.getvalue())

    fp5 = io.StringIO()
    json.dump({(1, 2): 'skip'}, fp5, skipkeys=True)
    print('dump_skipkeys', fp5.getvalue())

    decimal_val = json.loads('1.1', parse_float=Decimal)

    fp6 = io.StringIO()
    json.dump([1, 2], fp6, separators=(',', ':'))
    print('dump_separators', fp6.getvalue())

    def dump_default(obj):
        if isinstance(obj, set):
            return sorted(obj)
        raise TypeError(f'Cannot serialize {type(obj)}')

    fp7 = io.StringIO()
    json.dump({3, 1, 2}, fp7, default=dump_default)
    print('dump_default', fp7.getvalue())
except Exception as e:
    print('SKIP_dump_function_(file-like_object)', type(e).__name__, e)

# === load function (file-like object) ===
try:
    fp_load1 = io.StringIO('[1, 2, 3]')
    print('load_basic', json.load(fp_load1))

    fp_load2 = io.StringIO('{"a": 1, "b": 2}')
    print('load_dict', json.load(fp_load2))

    fp_load3 = io.StringIO('  \n  [true, false, null]  ')
    print('load_whitespace', json.load(fp_load3))

    def load_object_hook(dct):
        if 'value' in dct:
            dct['value'] = dct['value'] * 10
        return dct

    fp_load4 = io.StringIO('{"value": 5}')
    print('load_object_hook', json.load(fp_load4, object_hook=load_object_hook))

    fp_load5 = io.StringIO('1.5')
    print('load_parse_float', json.load(fp_load5, parse_float=lambda x: float(x) * 2))

    fp_load6 = io.StringIO('42')
    print('load_parse_int', json.load(fp_load6, parse_int=lambda x: int(x) + 100))
except Exception as e:
    print('SKIP_load_function_(file-like_object)', type(e).__name__, e)

# === JSONEncoder class ===
try:
    encoder1 = json.JSONEncoder()
    print('encoder_default_encode', encoder1.encode({'a': 1}))
    print('encoder_default_list', encoder1.encode([1, 2, 3]))

    encoder2 = json.JSONEncoder(indent=2)
    print('encoder_indent_encode', encoder2.encode({'b': 2}))

    encoder3 = json.JSONEncoder(sort_keys=True)
    print('encoder_sort_keys', encoder3.encode({'c': 1, 'a': 2}))

    encoder4 = json.JSONEncoder(ensure_ascii=False)
    print('encoder_ensure_ascii', encoder4.encode('hello'))

    encoder5 = json.JSONEncoder(ensure_ascii=False)
    print('encoder_ensure_ascii_unicode', encoder5.encode('café'))

    encoder6 = json.JSONEncoder(separators=(',', ':'))
    print('encoder_separators', encoder6.encode([1, 2, {'a': 3}]))

    # JSONEncoder.iterencode
    encoder7 = json.JSONEncoder()
    chunks = list(encoder7.iterencode([1, 2, 3]))
    print('encoder_iterencode', chunks)

    encoder8 = json.JSONEncoder(indent=2)
    chunks = list(encoder8.iterencode({'a': [1, 2]}))
    print('encoder_iterencode_indent', chunks)

    # JSONEncoder with default
    class SetEncoder(json.JSONEncoder):
        def default(self, obj):
            if isinstance(obj, set):
                return sorted(obj)
            return super().default(obj)

    encoder9 = SetEncoder()
    print('encoder_custom_default', encoder9.encode({3, 1, 2}))

    # JSONEncoder attributes
    encoder10 = json.JSONEncoder()
    print('encoder_item_separator', repr(encoder10.item_separator))
    print('encoder_key_separator', repr(encoder10.key_separator))

    encoder11 = json.JSONEncoder(separators=(',', ':'))
    print('encoder_custom_item_sep', repr(encoder11.item_separator))
    print('encoder_custom_key_sep', repr(encoder11.key_separator))

    # JSONEncoder default method error
    encoder12 = json.JSONEncoder()
    try:
        encoder12.encode(object())
        print('encoder_default_error', 'unexpected_success')
    except TypeError as e:
        print('encoder_default_error', 'TypeError')
except Exception as e:
    print('SKIP_JSONEncoder_class', type(e).__name__, e)

# === JSONDecoder class ===
try:
    decoder1 = json.JSONDecoder()
    print('decoder_decode', decoder1.decode('[1, 2, 3]'))
    print('decoder_decode_obj', decoder1.decode('{"a": 1}'))
    print('decoder_decode_str', decoder1.decode('"hello"'))

    # JSONDecoder.raw_decode
    decoder2 = json.JSONDecoder()
    result, idx = decoder2.raw_decode('[1, 2, 3]   trailing')
    print('decoder_raw_decode_result', result)
    print('decoder_raw_decode_idx', idx)

    result2, idx2 = decoder2.raw_decode('{"a": 1}  more')
    print('decoder_raw_decode_obj', result2)
    print('decoder_raw_decode_obj_idx', idx2)

    # JSONDecoder with object_hook
    def decoder_object_hook(dct):
        if 'multiplier' in dct:
            dct['result'] = dct['value'] * dct['multiplier']
        return dct

    decoder3 = json.JSONDecoder(object_hook=decoder_object_hook)
    print('decoder_object_hook', decoder3.decode('{"value": 5, "multiplier": 3}'))

    # JSONDecoder with parse_float
    decoder4 = json.JSONDecoder(parse_float=lambda x: float(x) * 3)
    print('decoder_parse_float', decoder4.decode('2.5'))

    # JSONDecoder with parse_int
    decoder5 = json.JSONDecoder(parse_int=lambda x: int(x) + 50)
    print('decoder_parse_int', decoder5.decode('10'))

    # JSONDecoder with parse_constant
    decoder6 = json.JSONDecoder(parse_constant=lambda x: f'const:{x}')
    print('decoder_parse_constant_nan', decoder6.decode('NaN'))
    print('decoder_parse_constant_inf', decoder6.decode('Infinity'))

    # JSONDecoder with object_pairs_hook
    def decoder_pairs_hook(pairs):
        return {f'k_{k}': v * 2 for k, v in pairs}

    decoder7 = json.JSONDecoder(object_pairs_hook=decoder_pairs_hook)
    print('decoder_object_pairs_hook', decoder7.decode('{"a": 5, "b": 10}'))

    # JSONDecoder strict parameter
    decoder8 = json.JSONDecoder(strict=True)
    print('decoder_strict_true', decoder8.decode('"hello\\nworld"'))

    decoder9 = json.JSONDecoder(strict=False)
    print('decoder_strict_false', decoder9.decode('"hello\\nworld"'))
except Exception as e:
    print('SKIP_JSONDecoder_class', type(e).__name__, e)

# === JSONDecodeError ===
try:
    try:
        json.loads('{"invalid": }')
        print('jsondecodeerror_invalid', 'unexpected_success')
    except json.JSONDecodeError as e:
        print('jsondecodeerror_invalid_msg', str(e))
        print('jsondecodeerror_invalid_lineno', e.lineno)
        print('jsondecodeerror_invalid_colno', e.colno)
        print('jsondecodeerror_invalid_pos', e.pos)

    try:
        json.loads('[1, 2, ]')
        print('jsondecodeerror_trailing_comma', 'unexpected_success')
    except json.JSONDecodeError as e:
        print('jsondecodeerror_trailing_comma_msg', str(e))

    try:
        json.loads('{"unclosed": "string}')
        print('jsondecodeerror_unclosed', 'unexpected_success')
    except json.JSONDecodeError as e:
        print('jsondecodeerror_unclosed_type', type(e).__name__)

    try:
        json.loads('undefined')
        print('jsondecodeerror_undefined', 'unexpected_success')
    except json.JSONDecodeError as e:
        print('jsondecodeerror_undefined_type', type(e).__name__)

    # JSONDecodeError with multi-line input
    try:
        json.loads('{\n  "key": \n  invalid\n}')
        print('jsondecodeerror_multiline', 'unexpected_success')
    except json.JSONDecodeError as e:
        print('jsondecodeerror_multiline_lineno', e.lineno)
except Exception as e:
    print('SKIP_JSONDecodeError', type(e).__name__, e)

# === Edge cases and combinations ===
try:
    # Multiple parameters together
    print('dumps_combo', json.dumps({'z': 1, 'a': 2}, sort_keys=True, indent=2, ensure_ascii=True))

    # Non-string dict keys get converted
    print('dumps_int_key', json.dumps({1: 'a', 2: 'b'}))
    print('dumps_float_key', json.dumps({1.5: 'x'}))

    # Empty containers
    print('dumps_empty_nested', json.dumps({'a': [], 'b': {}}))

    # Large numbers
    print('dumps_large_int', json.dumps(10**20))
    print('dumps_small_float', json.dumps(1e-10))
    print('dumps_large_float', json.dumps(1e10))

    # Unicode beyond BMP
    print('dumps_unicode_bmp', json.dumps('𐍈'))
    print('dumps_unicode_ensure_ascii', json.dumps('𐍈', ensure_ascii=True))

    # Null bytes in strings
    print('dumps_null_byte', json.dumps('hello\x00world'))

    # Array of all types
    print('dumps_mixed_array', json.dumps([None, True, False, 1, 1.5, 'str', [], {}]))

    # Deeply nested structure
    deep = {'level1': {'level2': {'level3': {'level4': 'value'}}}}
    print('dumps_deeply_nested', json.dumps(deep, indent=2))

    # Loads with extra data (should fail at top level but raw_decode can handle)
    decoder_extra = json.JSONDecoder()
    try:
        json.loads('[1, 2] [3, 4]')
        print('loads_extra_data', 'unexpected_success')
    except json.JSONDecodeError as e:
        print('loads_extra_data', 'JSONDecodeError')

    result, idx = decoder_extra.raw_decode('[1, 2] [3, 4]')
    print('raw_decode_extra_result', result)
    print('raw_decode_extra_idx', idx)

    # loads with cls kwarg that passes extra to JSONDecoder
    class VerboseDecoder(json.JSONDecoder):
        pass

    print('loads_cls_kwarg', json.loads('[1, 2]', cls=VerboseDecoder))

    # Test that ValueError is raised for check_circular
    circular = []
    circular.append(circular)
    try:
        json.dumps(circular)
        print('dumps_circular_error', 'unexpected_success')
    except ValueError as e:
        print('dumps_circular_error', 'ValueError')

    # Test bytes input (should raise TypeError)
    try:
        json.loads(b'[1, 2, 3]')
        print('loads_bytes', 'success_or_bytes_works')
    except (TypeError, UnicodeDecodeError) as e:
        print('loads_bytes', type(e).__name__)

    # Test bytearray input
    try:
        json.loads(bytearray(b'[1, 2, 3]'))
        print('loads_bytearray', 'success_or_bytearray_works')
    except (TypeError, UnicodeDecodeError) as e:
        print('loads_bytearray', type(e).__name__)

    # Encoder with check_circular=False
    encoder_nocheck = json.JSONEncoder(check_circular=False)
    print('encoder_nocheck', encoder_nocheck.encode([1, 2, 3]))

    # Encoder with allow_nan=False
    encoder_no_nan = json.JSONEncoder(allow_nan=False)
    try:
        encoder_no_nan.encode(float('nan'))
        print('encoder_no_nan', 'unexpected_success')
    except ValueError:
        print('encoder_no_nan', 'ValueError')

    # Decoder with strict=False handling control chars
    decoder_nonstrict = json.JSONDecoder(strict=False)
    print('decoder_nonstrict', decoder_nonstrict.decode('"tab\there"'))
except Exception as e:
    print('SKIP_Edge_cases_and_combinations', type(e).__name__, e)
