# === String slicing (getitem_slice, lines 66-75) ===
s = 'hello'
assert s[0:3] == 'hel', 'slice start to mid'
assert s[1:4] == 'ell', 'slice mid range'
assert s[:] == 'hello', 'slice full copy'
assert s[::2] == 'hlo', 'slice with step 2'
assert s[::-1] == 'olleh', 'slice reverse'
assert s[1:4:2] == 'el', 'slice with start stop step'
assert s[-3:] == 'llo', 'slice negative start'
assert s[:-2] == 'hel', 'slice negative stop'

# Unicode slicing
u = 'caf\xe9'
assert u[0:3] == 'caf', 'unicode slice first 3 chars'
assert u[3] == '\xe9', 'unicode slice accented char'

# Empty slice
assert 'hello'[2:2] == '', 'empty slice same indices'
assert 'hello'[3:1] == '', 'empty slice reversed indices'

# Step zero should raise ValueError
try:
    'hello'[::0]
    assert False, 'slice step zero should raise ValueError'
except ValueError:
    pass

# === String iadd with InternString (lines 299-301) ===
s = 'hello'
s += 'x'
assert s == 'hellox', 'iadd single char (InternString)'

s = 'abc'
s += ''
assert s == 'abc', 'iadd empty InternString'

# === String iadd non-str (lines 303) ===
# Trying += with non-string should raise TypeError
try:
    s = 'hello'
    s += 5
    assert False, 'iadd int should raise TypeError'
except TypeError:
    pass

# === Unknown string method (lines 315-316, 335-336, 472-473) ===
try:
    'hello'.nonexistent_method()
    assert False, 'unknown method should raise AttributeError'
except AttributeError as e:
    assert 'str' in str(e), 'error should mention str type'

# === Join with non-string items (lines 532-536) ===
try:
    ','.join([1, 2, 3])
    assert False, 'join with ints should raise TypeError'
except TypeError as e:
    assert 'sequence item' in str(e), 'join error should mention sequence item'

try:
    ','.join(['a', 1])
    assert False, 'join with mixed types should raise TypeError'
except TypeError as e:
    assert 'sequence item' in str(e), 'join mixed error mentions sequence item'

# === String repr with special chars (lines 570-573, 583-586) ===
# Backslash in repr
assert repr('a\\b') == "'a\\\\b'", 'repr with backslash'

# Tab in repr
assert repr('a\tb') == "'a\\tb'", 'repr with tab'

# Carriage return in repr
assert repr('a\rb') == "'a\\rb'", 'repr with carriage return'

# String with single quotes uses double quotes in repr
assert repr("it's") == '"it\'s"', 'repr with single quote uses double quotes'

# String with both quotes (lines 570-573 double-quoted branch)
s = 'it\'s a "test"'
r = repr(s)
assert '\\' in r, 'repr with both quotes contains escapes'

# Backslash/tab/cr in double-quoted branch
assert repr("it's\t") == '"it\'s\\t"', 'repr double-quoted with tab'

# === Unicode digit methods (lines 852-888) ===
# Osmanya digits (U+104A0..U+104A9) - testing isdigit on extended Unicode
# These may not render but they exercise the code paths
osmanya = '\U000104a0'
assert osmanya.isdigit() == True, 'isdigit Osmanya digit'
assert osmanya.isdecimal() == True, 'isdecimal Osmanya digit'

# Brahmi digits (U+11066..U+1106F)
brahmi = '\U00011066'
assert brahmi.isdigit() == True, 'isdigit Brahmi digit'

# === Circled/parenthesized digits in isdigit (lines 915-929) ===
# Circled digit 1 (U+2460)
circled_1 = '\u2460'
assert circled_1.isdigit() == True, 'isdigit circled digit 1'

# Superscript 0 (U+2070)
super_0 = '\u2070'
assert super_0.isdigit() == True, 'isdigit superscript 0'

# Subscript 0 (U+2080)
sub_0 = '\u2080'
assert sub_0.isdigit() == True, 'isdigit subscript 0'

# Parenthesized digit 1 (U+2474)
paren_1 = '\u2474'
assert paren_1.isdigit() == True, 'isdigit parenthesized digit 1'

# Period digit 1 (U+2488)
period_1 = '\u2488'
assert period_1.isdigit() == True, 'isdigit period digit 1'

# Double circled digit 1 (U+24F5)
dbl_circled_1 = '\u24f5'
assert dbl_circled_1.isdigit() == True, 'isdigit double circled digit 1'

# Dingbat circled sans-serif digit 1 (U+2780)
dingbat_1 = '\u2780'
assert dingbat_1.isdigit() == True, 'isdigit dingbat circled digit 1'

# Dingbat negative circled digit 1 (U+278A)
neg_dingbat_1 = '\u278a'
assert neg_dingbat_1.isdigit() == True, 'isdigit negative dingbat digit 1'

# === rindex not found (line 1005) ===
try:
    'hello'.rindex('xyz')
    assert False, 'rindex should raise ValueError for not found'
except ValueError:
    pass

# === Too many args to search methods (lines 1084-1094) ===
try:
    'hello'.find('l', 0, 5, 99)
    assert False, 'find with too many args should raise TypeError'
except TypeError:
    pass

try:
    'hello'.rfind('l', 0, 5, 99)
    assert False, 'rfind with too many args should raise TypeError'
except TypeError:
    pass

try:
    'hello'.index('l', 0, 5, 99)
    assert False, 'index with too many args should raise TypeError'
except TypeError:
    pass

try:
    'hello'.count('l', 0, 5, 99)
    assert False, 'count with too many args should raise TypeError'
except TypeError:
    pass

# === Too many args to startswith/endswith (lines 1160-1170) ===
try:
    'hello'.startswith('h', 0, 5, 99)
    assert False, 'startswith with too many args should raise TypeError'
except TypeError:
    pass

try:
    'hello'.endswith('o', 0, 5, 99)
    assert False, 'endswith with too many args should raise TypeError'
except TypeError:
    pass

# === extract_str_or_tuple_of_str edge cases (lines 1231-1233) ===
# Passing non-string/non-tuple to startswith
try:
    'hello'.startswith(123)
    assert False, 'startswith with int should raise TypeError'
except TypeError:
    pass

try:
    'hello'.endswith(123)
    assert False, 'endswith with int should raise TypeError'
except TypeError:
    pass

# === split/rsplit with whitespace maxsplit (lines 1432-1433, 1478-1479) ===
assert '  a  b  c  '.split(None, 1) == ['a', 'b  c  '], 'split whitespace maxsplit 1'
assert '  a  b  c  '.split(None, 2) == ['a', 'b', 'c  '], 'split whitespace maxsplit 2'
assert '  a  b  c  '.rsplit(None, 1) == ['  a  b', 'c'], 'rsplit whitespace maxsplit 1'
assert '  a  b  c  '.rsplit(None, 2) == ['  a', 'b', 'c'], 'rsplit whitespace maxsplit 2'

# Single word with maxsplit
assert '  hello  '.split(None, 5) == ['hello'], 'split whitespace maxsplit > words'
assert '  hello  '.rsplit(None, 5) == ['hello'], 'rsplit whitespace maxsplit > words'

# === split/rsplit too many positional args (lines 1512-1522) ===
try:
    'a b c'.split(',', 1, 99)
    assert False, 'split with too many args should raise TypeError'
except TypeError:
    pass

try:
    'a b c'.rsplit(',', 1, 99)
    assert False, 'rsplit with too many args should raise TypeError'
except TypeError:
    pass

# === split with None sep positional (lines 1528-1530) ===
assert 'a b c'.split(None) == ['a', 'b', 'c'], 'split None sep explicit'
assert 'a b c'.rsplit(None) == ['a', 'b', 'c'], 'rsplit None sep explicit'

# === splitlines too many positional args (lines 1716-1723) ===
try:
    'a\nb'.splitlines(True, False)
    assert False, 'splitlines with too many args should raise TypeError'
except TypeError:
    pass

# === splitlines with None as keepends (truthy check on None, line 1773-1775) ===
assert 'a\nb'.splitlines(None) == ['a', 'b'], 'splitlines None keepends is falsy'
assert 'a\nb'.splitlines(0) == ['a', 'b'], 'splitlines 0 keepends is falsy'
assert 'a\nb'.splitlines(1) == ['a\n', 'b'], 'splitlines 1 keepends is truthy'

# === partition/rpartition empty sep (lines 1809, 1827, 1842) ===
try:
    'hello'.partition('')
    assert False, 'partition empty sep should raise ValueError'
except ValueError:
    pass

try:
    'hello'.rpartition('')
    assert False, 'rpartition empty sep should raise ValueError'
except ValueError:
    pass

# === replace error paths (lines 1880-1901) ===
# replace with no args
try:
    'hello'.replace()
    assert False, 'replace no args should raise TypeError'
except TypeError:
    pass

# replace with one arg
try:
    'hello'.replace('l')
    assert False, 'replace one arg should raise TypeError'
except TypeError:
    pass

# replace with too many args
try:
    'hello'.replace('l', 'L', 1, 99)
    assert False, 'replace too many args should raise TypeError'
except TypeError:
    pass

# === center/ljust/rjust with negative width (line 2060) ===
assert 'hi'.center(-1) == 'hi', 'center negative width returns original'
assert 'hi'.ljust(-1) == 'hi', 'ljust negative width returns original'
assert 'hi'.rjust(-1) == 'hi', 'rjust negative width returns original'

# === center/ljust/rjust too many args (lines 2045-2052) ===
try:
    'hi'.center(10, '-', 'x')
    assert False, 'center too many args should raise TypeError'
except TypeError:
    pass

try:
    'hi'.ljust(10, '-', 'x')
    assert False, 'ljust too many args should raise TypeError'
except TypeError:
    pass

try:
    'hi'.rjust(10, '-', 'x')
    assert False, 'rjust too many args should raise TypeError'
except TypeError:
    pass

# === center/ljust/rjust fillchar not single char (line 2069) ===
try:
    'hi'.center(10, 'ab')
    assert False, 'center multi-char fillchar should raise TypeError'
except TypeError:
    pass

# === zfill with negative width (line 2090) ===
assert '42'.zfill(-1) == '42', 'zfill negative width'

# === encode with unknown encoding (line 2134) ===
try:
    'hello'.encode('totally-fake-encoding-xyz')
    assert False, 'encode fake encoding should raise LookupError'
except LookupError:
    pass

# === encode with valid error handlers (line 2140) ===
# Test that known error handlers are accepted
assert 'hello'.encode('utf-8', 'strict') == b'hello', 'encode strict handler'
assert 'hello'.encode('utf-8', 'ignore') == b'hello', 'encode ignore handler'
assert 'hello'.encode('utf-8', 'replace') == b'hello', 'encode replace handler'
assert 'hello'.encode('utf-8', 'backslashreplace') == b'hello', 'encode backslashreplace handler'

# === Bool truthiness of strings (lines 249-251) ===
assert bool('') == False, 'empty string is falsy'
assert bool('x') == True, 'non-empty string is truthy'
assert bool('hello') == True, 'longer string is truthy'

# === String Deref (lines 203-205) ===
# Tested implicitly via len, but exercise the path explicitly
s = 'hello'
assert len(s) == 5, 'len on heap string exercises Deref'

# === String from conversion (lines 91-93) ===
# String conversion is implicit in many operations
assert str('hello') == 'hello', 'str() of string'

# === String __contains__ ===
assert 'ell' in 'hello', 'contains substring'
assert 'xyz' not in 'hello', 'not contains substring'
assert '' in 'hello', 'contains empty string'

# === Additional split with sep=None kwarg (lines 1568-1570) ===
assert 'a b c'.split(sep=None) == ['a', 'b', 'c'], 'split sep=None kwarg'

# === Splitlines keepends kwarg (lines 1746-1751) ===
# Already tested above but test the double value error path
# This is tested via splitlines(keepends=True) in existing tests

# === Replace with count kwarg edge cases (lines 1922-1943) ===
# replace with count=0 means replace nothing
assert 'aaa'.replace('a', 'b', 0) == 'aaa', 'replace count 0'
