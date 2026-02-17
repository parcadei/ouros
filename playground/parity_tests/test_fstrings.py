# === Basic f-strings ===
try:
    name = 'world'
    print('fstring_basic', f'Hello {name}!')
    print('fstring_empty', f'')
    print('fstring_no_exprs', f'just a string')
except Exception as e:
    print('SKIP_Basic_f_strings', type(e).__name__, e)

# === Simple interpolation ===
try:
    x = 42
    print('fstring_int', f'value: {x}')
    s = 'test'
    print('fstring_str', f'string: {s}')
    f = 3.14159
    print('fstring_float', f'pi: {f}')
    b = True
    print('fstring_bool', f'flag: {b}')
    n = None
    print('fstring_none', f'nothing: {n}')
except Exception as e:
    print('SKIP_Simple_interpolation', type(e).__name__, e)

# === Multiple interpolations ===
try:
    a = 1
    b = 2
    print('fstring_multi', f'{a} + {b} = {a + b}')
except Exception as e:
    print('SKIP_Multiple_interpolations', type(e).__name__, e)

# === Expression evaluation ===
try:
    print('fstring_expr_arith', f'result: {10 + 20 * 2}')
    print('fstring_expr_paren', f'result: {(10 + 20) * 2}')
except Exception as e:
    print('SKIP_Expression_evaluation', type(e).__name__, e)

# === Value types ===
try:
    lst = [1, 2, 3]
    print('fstring_list', f'list: {lst}')
    d = {'a': 1, 'b': 2}
    print('fstring_dict', f'dict: {d}')
    t = (10, 20, 30)
    print('fstring_tuple', f'tuple: {t}')
except Exception as e:
    print('SKIP_Value_types', type(e).__name__, e)

# === Conversion flags ===
try:
    # !s - str()
    print('fstring_conv_s', f'{42!s}')
    print('fstring_conv_s_str', f'{"hello"!s}')
    # !r - repr()
    print('fstring_conv_r', f'{"hello"!r}')
    print('fstring_conv_r_int', f'{42!r}')
    print('fstring_conv_r_list', f'{[1, 2, 3]!r}')
    # !a - ascii()
    print('fstring_conv_a', f'{"café"!a}')
    print('fstring_conv_a_ascii', f'{"hello"!a}')
    print('fstring_conv_a_unicode', f'{"日本"!a}')
except Exception as e:
    print('SKIP_Conversion_flags', type(e).__name__, e)

# === String padding and alignment ===
try:
    # Width
    print('fstring_str_width', f'{"hi":10}')
    # Left align
    print('fstring_str_left', f'{"hi":<10}')
    # Right align
    print('fstring_str_right', f'{"hi":>10}')
    # Center align
    print('fstring_str_center', f'{"hi":^10}')
    print('fstring_str_center_odd', f'{"zip":^6}')
    # Fill character
    print('fstring_fill_right', f'{"hi":*>10}')
    print('fstring_fill_left', f'{"hi":_<10}')
    print('fstring_fill_center', f'{"hi":*^10}')
    # String truncation with precision
    print('fstring_str_trunc', f'{"xylophone":.5}')
    print('fstring_str_trunc_width', f'{"xylophone":10.5}')
except Exception as e:
    print('SKIP_String_padding_and_alignment', type(e).__name__, e)

# === Integer formatting ===
try:
    # Basic
    print('fstring_int_basic', f'{42}')
    print('fstring_int_type', f'{42:d}')
    # Padding
    print('fstring_int_pad', f'{42:4d}')
    print('fstring_int_zeropad', f'{42:04d}')
    # Sign
    print('fstring_int_pos', f'{42:+d}')
    print('fstring_int_space', f'{42: d}')
    print('fstring_int_neg_sign', f'{-42:+d}')
    print('fstring_int_neg_space', f'{-42: d}')
    # Sign-aware padding
    print('fstring_int_signpad', f'{-23:=5d}')
    # Grouping
    print('fstring_int_comma', f'{1000000:,}')
    print('fstring_int_underscore', f'{1000000:_}')
    # Alternate forms
    print('fstring_int_hex', f'{255:x}')
    print('fstring_int_hex_alt', f'{255:#x}')
    print('fstring_int_oct', f'{8:o}')
    print('fstring_int_oct_alt', f'{8:#o}')
    print('fstring_int_bin', f'{10:b}')
    print('fstring_int_bin_alt', f'{10:#b}')
    print('fstring_int_hex_upper', f'{255:X}')
    print('fstring_int_hex_upper_alt', f'{255:#X}')
except Exception as e:
    print('SKIP_Integer_formatting', type(e).__name__, e)

# === Float formatting ===
try:
    # Basic
    print('fstring_float_basic', f'{3.14159}')
    print('fstring_float_type', f'{3.141592653589793:f}')
    # Precision
    print('fstring_float_prec2', f'{3.141592653589793:.2f}')
    print('fstring_float_prec4', f'{3.141592653589793:.4f}')
    # Width and precision
    print('fstring_float_zeropad', f'{3.141592653589793:06.2f}')
    print('fstring_float_width_prec', f'{3.141592653589793:10.2f}')
    # Sign
    print('fstring_float_pos', f'{3.14:+.2f}')
    print('fstring_float_neg', f'{-3.14:+.2f}')
    print('fstring_float_minus', f'{3.14:-.2f}')
    print('fstring_float_minus_neg', f'{-3.14:-.2f}')
    # Exponential
    print('fstring_float_exp', f'{1234.5678:e}')
    print('fstring_float_exp_upper', f'{1234.5678:E}')
    print('fstring_float_exp_prec', f'{1234.5678:.2e}')
    print('fstring_float_exp_small', f'{0.00012345:.2e}')
    # General format
    print('fstring_float_gen', f'{1.5:g}')
    print('fstring_float_gen_strip', f'{1.500:g}')
    print('fstring_float_gen_large', f'{1234567890:g}')
    # Percentage
    print('fstring_float_pct', f'{0.25:%}')
    print('fstring_float_pct_prec', f'{0.25:.1%}')
    print('fstring_float_pct_zero', f'{0.125:.0%}')
except Exception as e:
    print('SKIP_Float_formatting', type(e).__name__, e)

# === Nested format specs ===
try:
    width = 10
    print('fstring_nested_width', f'{"hi":{width}}')
    align = '^'
    print('fstring_nested_align', f'{"test":{align}{width}}')
    prec = 3
    print('fstring_nested_prec', f'{"xylophone":.{prec}}')
    # Multiple nested
    fill = '*'
    print('fstring_nested_multi', f'{"hi":{fill}{align}{width}}')
except Exception as e:
    print('SKIP_Nested_format_specs', type(e).__name__, e)

# === Self-documenting expressions (=) ===
try:
    a = 42
    print('fstring_debug_basic', f'{a=}')
    print('fstring_debug_space', f'{a = }')
    name = 'test'
    print('fstring_debug_str', f'{name=}')
    print('fstring_debug_str_space', f'{name = }')
    print('fstring_debug_conv_s', f'{name=!s}')
    print('fstring_debug_conv_r', f'{name=!r}')
    print('fstring_debug_conv_a', f'{name=!a}')
    print('fstring_debug_expr', f'{1+1=}')
    print('fstring_debug_expr_space', f'{1 + 1 = }')
    x = 10
    y = 20
    print('fstring_debug_multi', f'{x=} {y=}')
    # Debug with format spec
    print('fstring_debug_format', f'{a=:05d}')
    print('fstring_debug_format_prec', f'{3.14159=:.2f}')
except Exception as e:
    print('SKIP_Self_documenting_expressions_(=)', type(e).__name__, e)

# === Multi-line f-strings ===
try:
    name = 'world'
    value = 42
    print('fstring_multiline', f'''Hello
    {name}
    value: {value}''')
    # Triple quoted with expressions
    print('fstring_triple_expr', f"""result: {1 + 2}
    value: {value}""")
except Exception as e:
    print('SKIP_Multi_line_f_strings', type(e).__name__, e)

# === Escaping braces ===
try:
    print('fstring_escape_empty', f'{{}}')
    print('fstring_escape_content', f'{{x}}')
    print('fstring_escape_value', f'{{{42}}}')
    print('fstring_escape_double', f'{{{{}}}}')
except Exception as e:
    print('SKIP_Escaping_braces', type(e).__name__, e)

# === Complex expressions ===
try:
    # Subscript
    arr = [10, 20, 30]
    print('fstring_subscript', f'{arr[1]}')
    print('fstring_subscript_neg', f'{arr[-1]}')
    # Dict lookup
    d = {'key': 'value', 'num': 123}
    print('fstring_dict_lookup', f'{d["key"]}')
    # Attribute access
    class Point:
        def __init__(self, x, y):
            self.x = x
            self.y = y
    p = Point(10, 20)
    print('fstring_attr', f'{p.x}')
    print('fstring_attr_chain', f'{p.x} {p.y}')
    # Nested expression evaluation
    print('fstring_nested_expr', f'{arr[0] + arr[1]}')
except Exception as e:
    print('SKIP_Complex_expressions', type(e).__name__, e)

# === Unicode handling ===
try:
    ch = 'café'
    print('fstring_unicode', f'{ch}')
    print('fstring_unicode_pad', f'{ch:_<10}')
    print('fstring_unicode_pad_right', f'{ch:_>10}')
    print('fstring_unicode_center', f'{ch:_^10}')
    # Unicode fill character
    print('fstring_unicode_fill', f'{"hi":é<10}')
except Exception as e:
    print('SKIP_Unicode_handling', type(e).__name__, e)

# === Zero padding with negative numbers ===
try:
    x = -42
    print('fstring_zero_neg', f'{x:05d}')
    print('fstring_zero_neg_float', f'{-3.14:07.2f}')
except Exception as e:
    print('SKIP_Zero_padding_with_negative_numbers', type(e).__name__, e)

# === Combining conversion and format ===
try:
    print('fstring_conv_format', f'{"hello"!r:>15}')
    print('fstring_conv_format_center', f'{"hi"!r:^10}')
except Exception as e:
    print('SKIP_Combining_conversion_and_format', type(e).__name__, e)

# === Nested f-strings (f-string inside expression) ===
try:
    inner = 'inner'
    print('fstring_nested_fstring', f'outer {f"{inner}"}')
except Exception as e:
    print('SKIP_Nested_f_strings_(f_string_inside_expression)', type(e).__name__, e)

# === Empty and whitespace ===
try:
    print('fstring_whitespace', f'{"x":5}')
    print('fstring_whitespace_prec', f'{"hello":10.3}')
except Exception as e:
    print('SKIP_Empty_and_whitespace', type(e).__name__, e)

# === Large numbers ===
try:
    print('fstring_large_int', f'{1234567890123456789}')
    print('fstring_large_float', f'{1e20}')
    print('fstring_small_float', f'{1e-20}')
except Exception as e:
    print('SKIP_Large_numbers', type(e).__name__, e)

# === Boolean in f-string ===
try:
    flag = True
    print('fstring_bool_upper', f'{flag}')
    print('fstring_bool_expr', f'{flag and False}')
except Exception as e:
    print('SKIP_Boolean_in_f_string', type(e).__name__, e)

# === None handling ===
try:
    val = None
    print('fstring_none_val', f'{val}')
except Exception as e:
    print('SKIP_None_handling', type(e).__name__, e)

# === F-string with multiple types ===
try:
    i = 10
    s = 'text'
    f = 2.5
    print('fstring_multi_types', f'{i} {s} {f}')
except Exception as e:
    print('SKIP_F_string_with_multiple_types', type(e).__name__, e)

# === Nested dictionary and list access ===
try:
    data = {'items': [1, 2, {'name': 'nested'}]}
    print('fstring_nested_access', f'{data["items"][2]["name"]}')
except Exception as e:
    print('SKIP_Nested_dictionary_and_list_access', type(e).__name__, e)

# === Repr of various types ===
try:
    print('fstring_repr_dict', f'{d!r}')
    print('fstring_repr_list', f'{arr!r}')
    print('fstring_repr_tuple', f'{t!r}')
except Exception as e:
    print('SKIP_Repr_of_various_types', type(e).__name__, e)

# === String format spec edge cases ===
try:
    print('fstring_str_exact', f'{"hi":2}')
    print('fstring_str_narrow', f'{"hello":3}')
except Exception as e:
    print('SKIP_String_format_spec_edge_cases', type(e).__name__, e)

# === Float special values ===
try:
    print('fstring_inf', f'{float("inf")}')
    print('fstring_neg_inf', f'{float("-inf")}')
    print('fstring_nan', f'{float("nan")}')
except Exception as e:
    print('SKIP_Float_special_values', type(e).__name__, e)

# === Integer bases ===
try:
    print('fstring_bin', f'{5:b}')
    print('fstring_oct', f'{9:o}')
    print('fstring_hex_lower', f'{255:x}')
    print('fstring_hex_upper', f'{255:X}')
except Exception as e:
    print('SKIP_Integer_bases', type(e).__name__, e)

# === Format with grouping and precision ===
try:
    print('fstring_group_float', f'{1234567.89:,.2f}')
    print('fstring_group_int', f'{1234567890:,d}')
except Exception as e:
    print('SKIP_Format_with_grouping_and_precision', type(e).__name__, e)

# === Variable width and precision ===
try:
    value = 3.14159
    width = 8
    prec = 2
    print('fstring_var_width_prec', f'{value:{width}.{prec}f}')
except Exception as e:
    print('SKIP_Variable_width_and_precision', type(e).__name__, e)

# === Multiple conversions in one f-string ===
try:
    a = 'hello'
    b = 'world'
    print('fstring_multi_conv', f'{a!r} {b!s}')
except Exception as e:
    print('SKIP_Multiple_conversions_in_one_f_string', type(e).__name__, e)

# === f-string with conditional expression ===
try:
    x = 5
    print('fstring_conditional_pos', f'{"positive" if x > 0 else "non-positive"}')
    x = -5
    print('fstring_conditional_neg', f'{"positive" if x > 0 else "non-positive"}')
except Exception as e:
    print('SKIP_f_string_with_conditional_expression', type(e).__name__, e)
