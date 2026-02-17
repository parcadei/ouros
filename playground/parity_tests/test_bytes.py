# Comprehensive bytes and bytearray parity tests
# Tests all methods for both bytes and bytearray types

# === capitalize ===
try:
    print('bytes_capitalize', b'hello world'.capitalize())
    print('bytes_capitalize_empty', b''.capitalize())
    print('bytes_capitalize_upper', b'HELLO'.capitalize())
    print('bytearray_capitalize', bytearray(b'hello world').capitalize())
except Exception as e:
    print('SKIP_capitalize', type(e).__name__, e)

# === center ===
try:
    print('bytes_center', b'hello'.center(10))
    print('bytes_center_fill', b'hello'.center(10, b'-'))
    print('bytes_center_narrow', b'hello'.center(3))
    print('bytearray_center', bytearray(b'hi').center(5, b'*'))
except Exception as e:
    print('SKIP_center', type(e).__name__, e)

# === count ===
try:
    print('bytes_count', b'abababa'.count(b'a'))
    print('bytes_count_overlap', b'aaaa'.count(b'aa'))
    print('bytes_count_start_end', b'hello world'.count(b'l', 0, 5))
    print('bytearray_count', bytearray(b'test test').count(b't'))
except Exception as e:
    print('SKIP_count', type(e).__name__, e)

# === decode ===
try:
    print('bytes_decode_utf8', b'hello'.decode())
    print('bytes_decode_ascii', b'hello'.decode('ascii'))
    print('bytes_decode_latin1', b'caf\xe9'.decode('latin1'))
    print('bytearray_decode', bytearray(b'world').decode())
except Exception as e:
    print('SKIP_decode', type(e).__name__, e)

# === endswith ===
try:
    print('bytes_endswith', b'hello.txt'.endswith(b'.txt'))
    print('bytes_endswith_tuple', b'hello.txt'.endswith((b'.txt', b'.py')))
    print('bytes_endswith_start_end', b'hello.txt'.endswith(b'lo', 0, 5))
    print('bytearray_endswith', bytearray(b'test').endswith(b'st'))
except Exception as e:
    print('SKIP_endswith', type(e).__name__, e)

# === expandtabs ===
try:
    print('bytes_expandtabs', b'a\tb'.expandtabs())
    print('bytes_expandtabs_4', b'a\tb'.expandtabs(4))
    print('bytes_expandtabs_multi', b'a\tb\tc'.expandtabs(4))
    print('bytearray_expandtabs', bytearray(b'x\ty').expandtabs(8))
except Exception as e:
    print('SKIP_expandtabs', type(e).__name__, e)

# === find ===
try:
    print('bytes_find', b'hello'.find(b'l'))
    print('bytes_find_missing', b'hello'.find(b'z'))
    print('bytes_find_start', b'hello'.find(b'l', 3))
    print('bytes_find_start_end', b'hello world'.find(b'o', 0, 5))
    print('bytearray_find', bytearray(b'test').find(b'e'))
except Exception as e:
    print('SKIP_find', type(e).__name__, e)

# === fromhex ===
try:
    print('bytes_fromhex', bytes.fromhex('48656c6c6f'))
    print('bytes_fromhex_spaces', bytes.fromhex('48 65 6c 6c 6f'))
    print('bytes_fromhex_empty', bytes.fromhex(''))
    print('bytearray_fromhex', bytearray.fromhex('776f726c64'))
except Exception as e:
    print('SKIP_fromhex', type(e).__name__, e)

# === hex ===
try:
    print('bytes_hex', b'Hello'.hex())
    print('bytes_hex_sep', b'Hello'.hex('-'))
    print('bytes_hex_bytes_sep', b'Hello'.hex('-', 2))
    print('bytearray_hex', bytearray(b'world').hex())
except Exception as e:
    print('SKIP_hex', type(e).__name__, e)

# === index ===
try:
    print('bytes_index', b'hello'.index(b'e'))
    print('bytes_index_start', b'hello'.index(b'l', 3))
    print('bytearray_index', bytearray(b'test').index(b's'))
except Exception as e:
    print('SKIP_index', type(e).__name__, e)

# === isalnum ===
try:
    print('bytes_isalnum_true', b'abc123'.isalnum())
    print('bytes_isalnum_false', b'abc 123'.isalnum())
    print('bytes_isalnum_empty', b''.isalnum())
    print('bytearray_isalnum', bytearray(b'xyz789').isalnum())
except Exception as e:
    print('SKIP_isalnum', type(e).__name__, e)

# === isalpha ===
try:
    print('bytes_isalpha_true', b'abc'.isalpha())
    print('bytes_isalpha_false', b'abc123'.isalpha())
    print('bytes_isalpha_empty', b''.isalpha())
    print('bytearray_isalpha', bytearray(b'XYZ').isalpha())
except Exception as e:
    print('SKIP_isalpha', type(e).__name__, e)

# === isascii ===
try:
    print('bytes_isascii_true', b'hello'.isascii())
    print('bytes_isascii_false', b'caf\xe9'.isascii())
    print('bytes_isascii_empty', b''.isascii())
    print('bytearray_isascii', bytearray(b'test').isascii())
except Exception as e:
    print('SKIP_isascii', type(e).__name__, e)

# === isdigit ===
try:
    print('bytes_isdigit_true', b'123'.isdigit())
    print('bytes_isdigit_false', b'123a'.isdigit())
    print('bytes_isdigit_empty', b''.isdigit())
    print('bytearray_isdigit', bytearray(b'456').isdigit())
except Exception as e:
    print('SKIP_isdigit', type(e).__name__, e)

# === islower ===
try:
    print('bytes_islower_true', b'hello'.islower())
    print('bytes_islower_false', b'Hello'.islower())
    print('bytes_islower_cased', b'123'.islower())
    print('bytearray_islower', bytearray(b'abc').islower())
except Exception as e:
    print('SKIP_islower', type(e).__name__, e)

# === isspace ===
try:
    print('bytes_isspace_true', b'   '.isspace())
    print('bytes_isspace_false', b'  x  '.isspace())
    print('bytes_isspace_empty', b''.isspace())
    print('bytearray_isspace', bytearray(b'\t\n').isspace())
except Exception as e:
    print('SKIP_isspace', type(e).__name__, e)

# === istitle ===
try:
    print('bytes_istitle_true', b'Hello World'.istitle())
    print('bytes_istitle_false', b'hello world'.istitle())
    print('bytes_istitle_empty', b''.istitle())
    print('bytearray_istitle', bytearray(b'Title Case').istitle())
except Exception as e:
    print('SKIP_istitle', type(e).__name__, e)

# === isupper ===
try:
    print('bytes_isupper_true', b'HELLO'.isupper())
    print('bytes_isupper_false', b'Hello'.isupper())
    print('bytes_isupper_cased', b'123'.isupper())
    print('bytearray_isupper', bytearray(b'ABC').isupper())
except Exception as e:
    print('SKIP_isupper', type(e).__name__, e)

# === join ===
try:
    print('bytes_join', b'-'.join([b'a', b'b', b'c']))
    print('bytes_join_empty_sep', b''.join([b'a', b'b']))
    print('bytes_join_empty_iter', b'-'.join([]))
    print('bytearray_join', bytearray(b' ').join([bytearray(b'hello'), bytearray(b'world')]))
except Exception as e:
    print('SKIP_join', type(e).__name__, e)

# === ljust ===
try:
    print('bytes_ljust', b'hi'.ljust(5))
    print('bytes_ljust_fill', b'hi'.ljust(5, b'*'))
    print('bytes_ljust_narrow', b'hi'.ljust(1))
    print('bytearray_ljust', bytearray(b'x').ljust(4, b'-'))
except Exception as e:
    print('SKIP_ljust', type(e).__name__, e)

# === lower ===
try:
    print('bytes_lower', b'HELLO'.lower())
    print('bytes_lower_mixed', b'Hello World'.lower())
    print('bytes_lower_empty', b''.lower())
    print('bytearray_lower', bytearray(b'TEST').lower())
except Exception as e:
    print('SKIP_lower', type(e).__name__, e)

# === lstrip ===
try:
    print('bytes_lstrip', b'  hello  '.lstrip())
    print('bytes_lstrip_chars', b'xyxhello'.lstrip(b'xy'))
    print('bytes_lstrip_empty', b''.lstrip())
    print('bytearray_lstrip', bytearray(b'...test').lstrip(b'.'))
except Exception as e:
    print('SKIP_lstrip', type(e).__name__, e)

# === maketrans ===
try:
    print('bytes_maketrans', bytes.maketrans(b'abc', b'xyz'))
    print('bytearray_maketrans', bytearray.maketrans(b'123', b'789'))
except Exception as e:
    print('SKIP_maketrans', type(e).__name__, e)

# === partition ===
try:
    print('bytes_partition', b'hello world'.partition(b' '))
    print('bytes_partition_missing', b'hello'.partition(b'x'))
    print('bytes_partition_empty', b''.partition(b'x'))
    print('bytearray_partition', bytearray(b'a-b-c').partition(b'-'))
except Exception as e:
    print('SKIP_partition', type(e).__name__, e)

# === removeprefix ===
try:
    print('bytes_removeprefix', b'HelloWorld'.removeprefix(b'Hello'))
    print('bytes_removeprefix_no_match', b'HelloWorld'.removeprefix(b'Bye'))
    print('bytes_removeprefix_empty', b''.removeprefix(b'x'))
    print('bytearray_removeprefix', bytearray(b'test.py').removeprefix(b'test.'))
except Exception as e:
    print('SKIP_removeprefix', type(e).__name__, e)

# === removesuffix ===
try:
    print('bytes_removesuffix', b'HelloWorld'.removesuffix(b'World'))
    print('bytes_removesuffix_no_match', b'HelloWorld'.removesuffix(b'Bye'))
    print('bytes_removesuffix_empty', b''.removesuffix(b'x'))
    print('bytearray_removesuffix', bytearray(b'test.py').removesuffix(b'.py'))
except Exception as e:
    print('SKIP_removesuffix', type(e).__name__, e)

# === replace ===
try:
    print('bytes_replace', b'hello world'.replace(b'l', b'x'))
    print('bytes_replace_count', b'hello'.replace(b'l', b'x', 1))
    print('bytes_replace_none', b'hello'.replace(b'z', b'x'))
    print('bytearray_replace', bytearray(b'a-b-c').replace(b'-', b':'))
except Exception as e:
    print('SKIP_replace', type(e).__name__, e)

# === rfind ===
try:
    print('bytes_rfind', b'hello'.rfind(b'l'))
    print('bytes_rfind_missing', b'hello'.rfind(b'z'))
    print('bytes_rfind_start', b'hello world'.rfind(b'o', 0, 5))
    print('bytearray_rfind', bytearray(b'test').rfind(b't'))
except Exception as e:
    print('SKIP_rfind', type(e).__name__, e)

# === rindex ===
try:
    print('bytes_rindex', b'hello'.rindex(b'l'))
    print('bytes_rindex_start', b'hello world'.rindex(b'o', 0, 8))
    print('bytearray_rindex', bytearray(b'test').rindex(b's'))
except Exception as e:
    print('SKIP_rindex', type(e).__name__, e)

# === rjust ===
try:
    print('bytes_rjust', b'hi'.rjust(5))
    print('bytes_rjust_fill', b'hi'.rjust(5, b'*'))
    print('bytes_rjust_narrow', b'hi'.rjust(1))
    print('bytearray_rjust', bytearray(b'x').rjust(4, b'-'))
except Exception as e:
    print('SKIP_rjust', type(e).__name__, e)

# === rpartition ===
try:
    print('bytes_rpartition', b'hello world test'.rpartition(b' '))
    print('bytes_rpartition_missing', b'hello'.rpartition(b'x'))
    print('bytearray_rpartition', bytearray(b'a-b-c').rpartition(b'-'))
except Exception as e:
    print('SKIP_rpartition', type(e).__name__, e)

# === rsplit ===
try:
    print('bytes_rsplit', b'a b c'.rsplit())
    print('bytes_rsplit_max', b'a b c d'.rsplit(maxsplit=1))
    print('bytes_rsplit_sep', b'a,b,c'.rsplit(b','))
    print('bytearray_rsplit', bytearray(b'x y z').rsplit())
except Exception as e:
    print('SKIP_rsplit', type(e).__name__, e)

# === rstrip ===
try:
    print('bytes_rstrip', b'  hello  '.rstrip())
    print('bytes_rstrip_chars', b'helloxyx'.rstrip(b'xy'))
    print('bytes_rstrip_empty', b''.rstrip())
    print('bytearray_rstrip', bytearray(b'test...').rstrip(b'.'))
except Exception as e:
    print('SKIP_rstrip', type(e).__name__, e)

# === split ===
try:
    print('bytes_split', b'a b c'.split())
    print('bytes_split_max', b'a b c d'.split(maxsplit=1))
    print('bytes_split_sep', b'a,b,c'.split(b','))
    print('bytes_split_linesep', b'a\nb\nc'.split(b'\n'))
    print('bytearray_split', bytearray(b'x y z').split())
except Exception as e:
    print('SKIP_split', type(e).__name__, e)

# === splitlines ===
try:
    print('bytes_splitlines', b'a\nb\nc'.splitlines())
    print('bytes_splitlines_keep', b'a\nb\n'.splitlines(keepends=True))
    print('bytes_splitlines_empty', b''.splitlines())
    print('bytes_splitlines_mixed', b'a\rb\r\nc'.splitlines())
    print('bytearray_splitlines', bytearray(b'x\ny').splitlines())
except Exception as e:
    print('SKIP_splitlines', type(e).__name__, e)

# === startswith ===
try:
    print('bytes_startswith', b'hello.txt'.startswith(b'hello'))
    print('bytes_startswith_tuple', b'hello.txt'.startswith((b'hello', b'world')))
    print('bytes_startswith_start_end', b'hello.txt'.startswith(b'lo', 3, 5))
    print('bytearray_startswith', bytearray(b'test').startswith(b'te'))
except Exception as e:
    print('SKIP_startswith', type(e).__name__, e)

# === strip ===
try:
    print('bytes_strip', b'  hello  '.strip())
    print('bytes_strip_chars', b'xyxhelloxyx'.strip(b'xy'))
    print('bytes_strip_empty', b''.strip())
    print('bytearray_strip', bytearray(b'...test...').strip(b'.'))
except Exception as e:
    print('SKIP_strip', type(e).__name__, e)

# === swapcase ===
try:
    print('bytes_swapcase', b'Hello World'.swapcase())
    print('bytes_swapcase_lower', b'hello'.swapcase())
    print('bytes_swapcase_upper', b'HELLO'.swapcase())
    print('bytes_swapcase_empty', b''.swapcase())
    print('bytearray_swapcase', bytearray(b'TeSt').swapcase())
except Exception as e:
    print('SKIP_swapcase', type(e).__name__, e)

# === title ===
try:
    print('bytes_title', b'hello world'.title())
    print('bytes_title_mixed', b'hElLo wOrLd'.title())
    print('bytes_title_empty', b''.title())
    print('bytearray_title', bytearray(b'test case').title())
except Exception as e:
    print('SKIP_title', type(e).__name__, e)

# === translate ===
try:
    print('bytes_translate', b'hello'.translate(bytes.maketrans(b'el', b'xy')))
    print('bytearray_translate', bytearray(b'test').translate(bytes.maketrans(b't', b'x')))
except Exception as e:
    print('SKIP_translate', type(e).__name__, e)

# === upper ===
try:
    print('bytes_upper', b'hello'.upper())
    print('bytes_upper_mixed', b'Hello World'.upper())
    print('bytes_upper_empty', b''.upper())
    print('bytearray_upper', bytearray(b'test').upper())
except Exception as e:
    print('SKIP_upper', type(e).__name__, e)

# === zfill ===
try:
    print('bytes_zfill', b'42'.zfill(5))
    print('bytes_zfill_signed', b'-42'.zfill(5))
    print('bytes_zfill_plus', b'+42'.zfill(5))
    print('bytes_zfill_wide', b'12345'.zfill(3))
    print('bytearray_zfill', bytearray(b'7').zfill(4))
except Exception as e:
    print('SKIP_zfill', type(e).__name__, e)

# === bytearray-specific mutable methods ===

# === append ===
try:
    ba = bytearray(b'hello')
    ba.append(ord('!'))
    print('bytearray_append', ba)
except Exception as e:
    print('SKIP_append', type(e).__name__, e)

# === clear ===
try:
    ba = bytearray(b'hello')
    ba.clear()
    print('bytearray_clear', ba)
except Exception as e:
    print('SKIP_clear', type(e).__name__, e)

# === copy ===
try:
    ba = bytearray(b'hello')
    ba_copy = ba.copy()
    ba[0] = ord('H')
    print('bytearray_copy', ba, ba_copy)
except Exception as e:
    print('SKIP_copy', type(e).__name__, e)

# === extend ===
try:
    ba = bytearray(b'hello')
    ba.extend(b' world')
    print('bytearray_extend', ba)
    ba2 = bytearray(b'hi')
    ba2.extend([1, 2, 3])
    print('bytearray_extend_iter', ba2)
except Exception as e:
    print('SKIP_extend', type(e).__name__, e)

# === insert ===
try:
    ba = bytearray(b'hello')
    ba.insert(0, ord('!'))
    print('bytearray_insert_start', ba)
    ba2 = bytearray(b'hello')
    ba2.insert(5, ord('!'))
    print('bytearray_insert_end', ba2)
except Exception as e:
    print('SKIP_insert', type(e).__name__, e)

# === pop ===
try:
    ba = bytearray(b'hello')
    print('bytearray_pop', ba.pop())
    print('bytearray_pop_after', ba)
    ba2 = bytearray(b'hello')
    print('bytearray_pop_index', ba2.pop(0))
    print('bytearray_pop_index_after', ba2)
except Exception as e:
    print('SKIP_pop', type(e).__name__, e)

# === remove ===
try:
    ba = bytearray(b'hello')
    ba.remove(ord('l'))
    print('bytearray_remove', ba)
except Exception as e:
    print('SKIP_remove', type(e).__name__, e)

# === reverse ===
try:
    ba = bytearray(b'hello')
    ba.reverse()
    print('bytearray_reverse', ba)
except Exception as e:
    print('SKIP_reverse', type(e).__name__, e)

# === resize ===
try:
    ba = bytearray(b'hello')
    ba.resize(3)
    print('bytearray_resize_shrink', ba)
    ba2 = bytearray(b'hi')
    ba2.resize(5)
    print('bytearray_resize_grow', ba2)
except Exception as e:
    print('SKIP_resize', type(e).__name__, e)

# === Additional edge cases ===
try:
    print('bytes_empty', b'')
    print('bytearray_empty', bytearray())
    print('bytes_single', b'x')
    print('bytearray_single', bytearray(b'x'))

    # bytes/bool interactions
    print('bytes_bool_empty', bool(b''))
    print('bytes_bool_nonempty', bool(b'hello'))
    print('bytearray_bool_empty', bool(bytearray()))
    print('bytearray_bool_nonempty', bool(bytearray(b'hello')))

    # bytes/len
    print('bytes_len', len(b'hello'))
    print('bytearray_len', len(bytearray(b'hello')))

    # bytes/indexing
    b = b'hello'
    print('bytes_index_0', b[0])
    print('bytes_slice', b[1:4])
    print('bytes_neg_index', b[-1])

    ba = bytearray(b'hello')
    print('bytearray_index_0', ba[0])
    print('bytearray_slice', ba[1:4])
    print('bytearray_neg_index', ba[-1])

    # bytes/iteration
    print('bytes_iter', list(b'abc'))
    print('bytearray_iter', list(bytearray(b'abc')))

    # bytes/concat
    print('bytes_concat', b'hello' + b' world')
    print('bytearray_concat', bytearray(b'hello') + bytearray(b' world'))
    print('bytes_bytearray_concat', b'hello' + bytearray(b' world'))

    # bytes/repeat
    print('bytes_repeat', b'hi' * 3)
    print('bytearray_repeat', bytearray(b'x') * 5)

    # bytes/contains
    print('bytes_contains', b'l' in b'hello')
    print('bytes_contains_int', 108 in b'hello')
    print('bytearray_contains', ord('l') in bytearray(b'hello'))
except Exception as e:
    print('SKIP_edge_cases', type(e).__name__, e)
