# Comprehensive str methods parity test file
# Tests all 47 public str methods in Python 3.14

# === capitalize ===
try:
    print('capitalize_lower', 'hello world'.capitalize())
    print('capitalize_upper', 'HELLO'.capitalize())
    print('capitalize_empty', ''.capitalize())
    print('capitalize_single', 'a'.capitalize())
except Exception as e:
    print('SKIP_capitalize', type(e).__name__, e)

# === casefold ===
try:
    print('casefold_lower', 'hello'.casefold())
    print('casefold_upper', 'HELLO'.casefold())
    print('casefold_mixed', 'HeLLo'.casefold())
    print('casefold_german', 'ß'.casefold())
    print('casefold_empty', ''.casefold())
except Exception as e:
    print('SKIP_casefold', type(e).__name__, e)

# === center ===
try:
    print('center_basic', 'hello'.center(10))
    print('center_with_fill', 'hello'.center(10, '*'))
    print('center_narrow', 'hello'.center(3))
    print('center_empty', ''.center(5, '-'))
except Exception as e:
    print('SKIP_center', type(e).__name__, e)

# === count ===
try:
    print('count_basic', 'hello world'.count('l'))
    print('count_with_start', 'hello world'.count('l', 5))
    print('count_with_range', 'hello world'.count('l', 0, 5))
    print('count_empty_sub', 'hello'.count(''))
    print('count_not_found', 'hello'.count('z'))
except Exception as e:
    print('SKIP_count', type(e).__name__, e)

# === encode ===
try:
    print('encode_utf8', 'hello'.encode())
    print('encode_ascii', 'hello'.encode('ascii'))
    print('encode_latin1', 'café'.encode('latin-1'))
    print('encode_ignore', 'hello'.encode('ascii', 'ignore'))
    print('encode_replace', 'hello'.encode('ascii', 'replace'))
except Exception as e:
    print('SKIP_encode', type(e).__name__, e)

# === endswith ===
try:
    print('endswith_basic', 'hello.txt'.endswith('.txt'))
    print('endswith_tuple', 'hello.txt'.endswith(('.txt', '.py')))
    print('endswith_start', 'hello.txt'.endswith('lo', 0, 5))
    print('endswith_false', 'hello.txt'.endswith('.py'))
    print('endswith_empty', ''.endswith('x'))
except Exception as e:
    print('SKIP_endswith', type(e).__name__, e)

# === expandtabs ===
try:
    print('expandtabs_default', 'a\tb\tc'.expandtabs())
    print('expandtabs_4', 'a\tb\tc'.expandtabs(4))
    print('expandtabs_2', 'a\tb\tc'.expandtabs(2))
    print('expandtabs_empty', ''.expandtabs())
except Exception as e:
    print('SKIP_expandtabs', type(e).__name__, e)

# === find ===
try:
    print('find_basic', 'hello world'.find('world'))
    print('find_not_found', 'hello world'.find('xyz'))
    print('find_with_start', 'hello world'.find('l', 5))
    print('find_with_range', 'hello world'.find('l', 0, 3))
    print('find_empty', 'hello'.find(''))
except Exception as e:
    print('SKIP_find', type(e).__name__, e)

# === format ===
try:
    print('format_basic', 'Hello, {}!'.format('world'))
    print('format_positional', '{} + {} = {}'.format(1, 2, 3))
    print('format_named', 'Hello, {name}!'.format(name='Alice'))
    print('format_indexed', '{0}, {1}, {0}'.format('a', 'b'))
    print('format_formatting', 'Pi = {:.2f}'.format(3.14159))
except Exception as e:
    print('SKIP_format', type(e).__name__, e)

# === format_map ===
try:
    print('format_map_basic', 'Hello, {name}!'.format_map({'name': 'Bob'}))
    print('format_map_multiple', '{x} + {y} = {z}'.format_map({'x': 1, 'y': 2, 'z': 3}))
except Exception as e:
    print('SKIP_format_map', type(e).__name__, e)

# === index ===
try:
    print('index_basic', 'hello world'.index('world'))
    print('index_with_start', 'hello world'.index('l', 5))
    print('index_with_range', 'hello world'.index('l', 0, 4))
except Exception as e:
    print('SKIP_index', type(e).__name__, e)

# === isalnum ===
try:
    print('isalnum_true', 'abc123'.isalnum())
    print('isalnum_alpha', 'abc'.isalnum())
    print('isalnum_digit', '123'.isalnum())
    print('isalnum_space', 'abc 123'.isalnum())
    print('isalnum_empty', ''.isalnum())
    print('isalnum_punct', 'abc-123'.isalnum())
except Exception as e:
    print('SKIP_isalnum', type(e).__name__, e)

# === isalpha ===
try:
    print('isalpha_true', 'abc'.isalpha())
    print('isalpha_digit', 'abc123'.isalpha())
    print('isalpha_space', 'a b'.isalpha())
    print('isalpha_empty', ''.isalpha())
except Exception as e:
    print('SKIP_isalpha', type(e).__name__, e)

# === isascii ===
try:
    print('isascii_true', 'hello'.isascii())
    print('isascii_space', 'hello world'.isascii())
    print('isascii_unicode', 'café'.isascii())
    print('isascii_empty', ''.isascii())
except Exception as e:
    print('SKIP_isascii', type(e).__name__, e)

# === isdecimal ===
try:
    print('isdecimal_true', '123'.isdecimal())
    print('isdecimal_alpha', 'abc'.isdecimal())
    print('isdecimal_mixed', '123abc'.isdecimal())
    print('isdecimal_space', '1 2 3'.isdecimal())
    print('isdecimal_empty', ''.isdecimal())
    print('isdecimal_unicode', '²³'.isdecimal())
except Exception as e:
    print('SKIP_isdecimal', type(e).__name__, e)

# === isdigit ===
try:
    print('isdigit_true', '123'.isdigit())
    print('isdigit_superscript', '²'.isdigit())
    print('isdigit_alpha', 'abc'.isdigit())
    print('isdigit_empty', ''.isdigit())
except Exception as e:
    print('SKIP_isdigit', type(e).__name__, e)

# === isidentifier ===
try:
    print('isidentifier_valid', 'hello_world'.isidentifier())
    print('isidentifier_start_digit', '123abc'.isidentifier())
    print('isidentifier_keyword', 'class'.isidentifier())
    print('isidentifier_space', 'hello world'.isidentifier())
    print('isidentifier_empty', ''.isidentifier())
except Exception as e:
    print('SKIP_isidentifier', type(e).__name__, e)

# === islower ===
try:
    print('islower_true', 'hello'.islower())
    print('islower_upper', 'HELLO'.islower())
    print('islower_mixed', 'Hello'.islower())
    print('islower_nocase', '123'.islower())
    print('islower_empty', ''.islower())
except Exception as e:
    print('SKIP_islower', type(e).__name__, e)

# === isnumeric ===
try:
    print('isnumeric_true', '123'.isnumeric())
    print('isnumeric_roman', 'Ⅷ'.isnumeric())
    print('isnumeric_fraction', '½'.isnumeric())
    print('isnumeric_alpha', 'abc'.isnumeric())
    print('isnumeric_empty', ''.isnumeric())
except Exception as e:
    print('SKIP_isnumeric', type(e).__name__, e)

# === isprintable ===
try:
    print('isprintable_true', 'hello world'.isprintable())
    print('isprintable_tab', 'hello\tworld'.isprintable())
    print('isprintable_newline', 'hello\nworld'.isprintable())
    print('isprintable_space', ' '.isprintable())
    print('isprintable_empty', ''.isprintable())
except Exception as e:
    print('SKIP_isprintable', type(e).__name__, e)

# === isspace ===
try:
    print('isspace_space', '   '.isspace())
    print('isspace_tab', '\t\n'.isspace())
    print('isspace_false', 'hello'.isspace())
    print('isspace_mixed', '  hello  '.isspace())
    print('isspace_empty', ''.isspace())
except Exception as e:
    print('SKIP_isspace', type(e).__name__, e)

# === istitle ===
try:
    print('istitle_true', 'Hello World'.istitle())
    print('istitle_false', 'HELLO WORLD'.istitle())
    print('istitle_lower', 'hello world'.istitle())
    print('istitle_empty', ''.istitle())
except Exception as e:
    print('SKIP_istitle', type(e).__name__, e)

# === isupper ===
try:
    print('isupper_true', 'HELLO'.isupper())
    print('isupper_lower', 'hello'.isupper())
    print('isupper_mixed', 'Hello'.isupper())
    print('isupper_empty', ''.isupper())
except Exception as e:
    print('SKIP_isupper', type(e).__name__, e)

# === join ===
try:
    print('join_list', '-'.join(['a', 'b', 'c']))
    print('join_tuple', ''.join(('x', 'y', 'z')))
    print('join_empty', ''.join([]))
    print('join_single', '-'.join(['solo']))
except Exception as e:
    print('SKIP_join', type(e).__name__, e)

# === ljust ===
try:
    print('ljust_basic', 'hello'.ljust(10))
    print('ljust_with_fill', 'hello'.ljust(10, '*'))
    print('ljust_narrow', 'hello'.ljust(3))
except Exception as e:
    print('SKIP_ljust', type(e).__name__, e)

# === lower ===
try:
    print('lower_basic', 'HELLO WORLD'.lower())
    print('lower_mixed', 'HeLLo'.lower())
    print('lower_already', 'hello'.lower())
    print('lower_empty', ''.lower())
except Exception as e:
    print('SKIP_lower', type(e).__name__, e)

# === lstrip ===
try:
    print('lstrip_basic', '  hello  '.lstrip())
    print('lstrip_chars', 'xyxhello'.lstrip('xy'))
    print('lstrip_none', 'hello'.lstrip())
    print('lstrip_empty', ''.lstrip())
except Exception as e:
    print('SKIP_lstrip', type(e).__name__, e)

# === maketrans and translate ===
try:
    table = str.maketrans('abc', '123')
    print('maketrans_basic', table)
    print('translate_basic', 'abc xyz'.translate(table))
    table_delete = str.maketrans('', '', 'aeiou')
    print('translate_delete', 'hello world'.translate(table_delete))
except Exception as e:
    print('SKIP_maketrans and translate', type(e).__name__, e)

# === partition ===
try:
    print('partition_basic', 'hello-world-test'.partition('-'))
    print('partition_not_found', 'hello'.partition('-'))
    # partition_empty_sep raises ValueError, skipping
    print('partition_first', 'a-b-a'.partition('-'))
except Exception as e:
    print('SKIP_partition', type(e).__name__, e)

# === removeprefix ===
try:
    print('removeprefix_basic', 'test_file.py'.removeprefix('test_'))
    print('removeprefix_no_match', 'hello.py'.removeprefix('test_'))
    print('removeprefix_empty', ''.removeprefix('test'))
except Exception as e:
    print('SKIP_removeprefix', type(e).__name__, e)

# === removesuffix ===
try:
    print('removesuffix_basic', 'test_file.py'.removesuffix('.py'))
    print('removesuffix_no_match', 'hello.txt'.removesuffix('.py'))
    print('removesuffix_empty', ''.removesuffix('.py'))
except Exception as e:
    print('SKIP_removesuffix', type(e).__name__, e)

# === replace ===
try:
    print('replace_basic', 'hello world'.replace('world', 'python'))
    print('replace_count', 'a a a'.replace('a', 'b', 2))
    print('replace_all', 'aaa'.replace('a', 'b'))
    print('replace_none', 'hello'.replace('xyz', 'abc'))
    print('replace_empty', ''.replace('a', 'b'))
except Exception as e:
    print('SKIP_replace', type(e).__name__, e)

# === rfind ===
try:
    print('rfind_basic', 'hello world'.rfind('l'))
    print('rfind_not_found', 'hello'.rfind('z'))
    print('rfind_with_start', 'hello world'.rfind('l', 5))
    print('rfind_with_range', 'hello world'.rfind('l', 0, 5))
except Exception as e:
    print('SKIP_rfind', type(e).__name__, e)

# === rindex ===
try:
    print('rindex_basic', 'hello world'.rindex('l'))
    print('rindex_with_start', 'hello world'.rindex('l', 5))
    print('rindex_with_range', 'hello world'.rindex('l', 0, 5))
except Exception as e:
    print('SKIP_rindex', type(e).__name__, e)

# === rjust ===
try:
    print('rjust_basic', 'hello'.rjust(10))
    print('rjust_with_fill', 'hello'.rjust(10, '*'))
    print('rjust_narrow', 'hello'.rjust(3))
except Exception as e:
    print('SKIP_rjust', type(e).__name__, e)

# === rpartition ===
try:
    print('rpartition_basic', 'hello-world-test'.rpartition('-'))
    print('rpartition_not_found', 'hello'.rpartition('-'))
    print('rpartition_last', 'a-b-a'.rpartition('-'))
except Exception as e:
    print('SKIP_rpartition', type(e).__name__, e)

# === rsplit ===
try:
    print('rsplit_basic', 'a b c'.rsplit())
    print('rsplit_max', 'a b c d'.rsplit(maxsplit=2))
    print('rsplit_sep', 'a,b,c'.rsplit(','))
    print('rsplit_sep_max', 'a,b,c,d'.rsplit(',', 2))
    print('rsplit_empty', ''.rsplit())
    print('rsplit_whitespace', '  a  b  c  '.rsplit())
except Exception as e:
    print('SKIP_rsplit', type(e).__name__, e)

# === rstrip ===
try:
    print('rstrip_basic', '  hello  '.rstrip())
    print('rstrip_chars', 'helloxyx'.rstrip('xy'))
    print('rstrip_none', 'hello'.rstrip())
    print('rstrip_empty', ''.rstrip())
except Exception as e:
    print('SKIP_rstrip', type(e).__name__, e)

# === split ===
try:
    print('split_basic', 'a b c'.split())
    print('split_max', 'a b c d'.split(maxsplit=2))
    print('split_sep', 'a,b,c'.split(','))
    print('split_sep_max', 'a,b,c,d'.split(',', 2))
    print('split_empty', ''.split())
    print('split_newline', 'a\nb\nc'.split())
except Exception as e:
    print('SKIP_split', type(e).__name__, e)

# === splitlines ===
try:
    print('splitlines_basic', 'a\nb\nc'.splitlines())
    print('splitlines_crlf', 'a\r\nb\r\nc'.splitlines())
    print('splitlines_keep', 'a\nb'.splitlines(True))
    print('splitlines_keep_crlf', 'a\r\nb'.splitlines(True))
    print('splitlines_empty', ''.splitlines())
    print('splitlines_no_newline', 'abc'.splitlines())
except Exception as e:
    print('SKIP_splitlines', type(e).__name__, e)

# === startswith ===
try:
    print('startswith_basic', 'hello.txt'.startswith('hello'))
    print('startswith_tuple', 'hello.txt'.startswith(('hi', 'hello')))
    print('startswith_start', 'hello.txt'.startswith('lo', 3))
    print('startswith_range', 'hello.txt'.startswith('lo', 3, 5))
    print('startswith_false', 'hello.txt'.startswith('.txt'))
    print('startswith_empty', ''.startswith('x'))
except Exception as e:
    print('SKIP_startswith', type(e).__name__, e)

# === strip ===
try:
    print('strip_basic', '  hello  '.strip())
    print('strip_chars', 'xyxhelloxyx'.strip('xy'))
    print('strip_none', 'hello'.strip())
    print('strip_empty', ''.strip())
except Exception as e:
    print('SKIP_strip', type(e).__name__, e)

# === swapcase ===
try:
    print('swapcase_basic', 'Hello World'.swapcase())
    print('swapcase_already', 'HELLO'.swapcase())
    print('swapcase_lower', 'hello'.swapcase())
    print('swapcase_mixed', 'HeLLo'.swapcase())
    print('swapcase_empty', ''.swapcase())
except Exception as e:
    print('SKIP_swapcase', type(e).__name__, e)

# === title ===
try:
    print('title_basic', 'hello world'.title())
    print('title_already', 'Hello World'.title())
    print('title_upper', 'HELLO WORLD'.title())
    print('title_apostrophe', "they're".title())
    print('title_empty', ''.title())
except Exception as e:
    print('SKIP_title', type(e).__name__, e)

# === upper ===
try:
    print('upper_basic', 'hello world'.upper())
    print('upper_already', 'HELLO'.upper())
    print('upper_mixed', 'HeLLo'.upper())
    print('upper_empty', ''.upper())
except Exception as e:
    print('SKIP_upper', type(e).__name__, e)

# === zfill ===
try:
    print('zfill_basic', '42'.zfill(5))
    print('zfill_negative', '-42'.zfill(5))
    print('zfill_plus', '+42'.zfill(5))
    print('zfill_longer', '12345'.zfill(3))
    print('zfill_empty', ''.zfill(3))
except Exception as e:
    print('SKIP_zfill', type(e).__name__, e)
