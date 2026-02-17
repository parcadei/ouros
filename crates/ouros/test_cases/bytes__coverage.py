# === bytes() constructor ===
# bytes() with no args
b_empty = bytes()
assert b_empty == b'', 'bytes() returns empty bytes'
assert len(b_empty) == 0, 'bytes() length is 0'

# bytes(int) returns zero-filled bytes
b_five = bytes(5)
assert b_five == b'\x00\x00\x00\x00\x00', 'bytes(5) returns 5 zero bytes'
assert len(b_five) == 5, 'bytes(5) length is 5'

b_zero = bytes(0)
assert b_zero == b'', 'bytes(0) returns empty bytes'

# bytes(bytes) returns a copy
b_copy = bytes(b'abc')
assert b_copy == b'abc', 'bytes(bytes) returns copy'

# bytes() with invalid type
try:
    bytes(3.14)
    assert False, 'bytes(float) should raise TypeError'
except TypeError:
    pass

# === Heap-allocated bytes operations ===
# Operations on bytes created via bytes() constructor (heap-allocated)
h = bytes(b'hello world')
assert len(h) == 11, 'heap bytes len'
assert h == bytes(b'hello world'), 'heap bytes equality'
assert h != bytes(b'other'), 'heap bytes inequality'
assert bool(h) is True, 'heap bytes bool true'
assert bool(bytes()) is False, 'heap bytes bool false'
assert repr(h) == "b'hello world'", 'heap bytes repr'

# Method calls on heap-allocated bytes
h2 = bytes(b'HELLO')
assert h2.lower() == b'hello', 'heap bytes lower'
assert h2.upper() == b'HELLO', 'heap bytes upper'
assert h2.decode() == 'HELLO', 'heap bytes decode'

# === Slicing ===
# Basic slicing
assert b'hello'[1:3] == b'el', 'slice basic'
assert b'hello'[::2] == b'hlo', 'slice with step'
assert b'hello'[::-1] == b'olleh', 'slice reverse'
assert b'hello'[4:1:-1] == b'oll', 'slice negative step partial'
assert b'hello'[::-2] == b'olh', 'slice reverse step 2'

# Slicing with negative step and explicit stop
assert b'abcde'[3:0:-1] == b'dcb', 'slice negative step with stop'
assert b'abcde'[4:0:-2] == b'ec', 'slice negative step 2 with stop'

# Heap bytes slicing
hb = bytes(b'abcdef')
assert hb[1:4] == b'bcd', 'heap bytes slice'
assert hb[::-1] == b'fedcba', 'heap bytes reverse slice'

# === repr edge cases ===
# Bytes containing single quote should use double quotes
assert repr(b"it's") == 'b"it\'s"', 'repr with single quote uses double quotes'

# === bytes.decode() edge cases ===
# decode with non-string encoding
try:
    b'hello'.decode(123)
    assert False, 'decode with int encoding should raise TypeError'
except TypeError:
    pass

# decode with errors argument (covered but checking path)
assert b'hello'.decode('utf-8', 'strict') == 'hello', 'decode with errors=strict'
assert b'hello'.decode('utf-8', 'replace') == 'hello', 'decode with errors=replace'

# === bytes.count() error paths ===
# count with str argument (should raise TypeError)
try:
    b'hello'.count('l')
    assert False, 'count with str arg should raise TypeError'
except TypeError:
    pass

# count with negative start (normalized)
assert b'hello'.count(b'l', -3) == 2, 'count with negative start'

# === bytes.find() error paths ===
# find with str argument
try:
    b'hello'.find('l')
    assert False, 'find with str arg should raise TypeError'
except TypeError:
    pass

# find with negative start
assert b'hello'.find(b'l', -3) == 2, 'find with negative start'

# === bytes.rfind() more cases ===
# rfind where needle > haystack
assert b'hi'.rfind(b'hello') == -1, 'rfind needle longer than haystack'

# === bytes.rindex() not found ===
try:
    b'hello'.rindex(b'x')
    assert False, 'rindex not found should raise ValueError'
except ValueError:
    pass

# === bytes.strip() with None ===
assert b'  hello  '.strip(None) == b'hello', 'strip with None arg'
assert b'  hello  '.lstrip(None) == b'hello  ', 'lstrip with None arg'
assert b'  hello  '.rstrip(None) == b'  hello', 'rstrip with None arg'

# === bytes.split() edge cases ===
# split with empty separator
try:
    b'hello'.split(b'')
    assert False, 'split with empty sep should raise ValueError'
except ValueError:
    pass

# rsplit with empty separator
try:
    b'hello'.rsplit(b'')
    assert False, 'rsplit with empty sep should raise ValueError'
except ValueError:
    pass

# split with None separator (whitespace splitting)
assert b'  a  b  c  '.split(None) == [b'a', b'b', b'c'], 'split with None sep'
assert b'  a  b  c  '.rsplit(None) == [b'a', b'b', b'c'], 'rsplit with None sep'

# split with maxsplit via keyword
assert b'a,b,c'.split(b',', maxsplit=1) == [b'a', b'b,c'], 'split with kwarg maxsplit'
assert b'a,b,c'.rsplit(b',', maxsplit=1) == [b'a,b', b'c'], 'rsplit with kwarg maxsplit'

# split with sep as keyword
assert b'a,b,c'.split(sep=b',') == [b'a', b'b', b'c'], 'split with kwarg sep'
assert b'a,b,c'.rsplit(sep=b',') == [b'a', b'b', b'c'], 'rsplit with kwarg sep'

# split with None sep and maxsplit
assert b'a b c d'.split(None, 2) == [b'a', b'b', b'c d'], 'split whitespace with maxsplit'
assert b'a b c d'.rsplit(None, 2) == [b'a b', b'c', b'd'], 'rsplit whitespace with maxsplit'

# split with unknown kwarg
try:
    b'hello'.split(foo=1)
    assert False, 'split with unknown kwarg should raise TypeError'
except TypeError:
    pass

# rsplit with unknown kwarg
try:
    b'hello'.rsplit(foo=1)
    assert False, 'rsplit with unknown kwarg should raise TypeError'
except TypeError:
    pass

# === bytes.splitlines() edge cases ===
# splitlines with keepends kwarg
assert b'a\nb'.splitlines(keepends=True) == [b'a\n', b'b'], 'splitlines with kwarg keepends'

# splitlines with unknown kwarg
try:
    b'a\nb'.splitlines(foo=1)
    assert False, 'splitlines with unknown kwarg should raise TypeError'
except TypeError:
    pass

# splitlines with too many args
try:
    b'a\nb'.splitlines(True, True)
    assert False, 'splitlines with too many args should raise TypeError'
except TypeError:
    pass

# === bytes.partition() with empty sep ===
try:
    b'hello'.partition(b'')
    assert False, 'partition with empty sep should raise ValueError'
except ValueError:
    pass

# === bytes.rpartition() with empty sep ===
try:
    b'hello'.rpartition(b'')
    assert False, 'rpartition with empty sep should raise ValueError'
except ValueError:
    pass

# === bytes.replace() edge cases ===
# replace with empty old (inserts between each byte)
assert b'abc'.replace(b'', b'-') == b'-a-b-c-', 'replace empty old inserts between'
assert b''.replace(b'', b'-') == b'-', 'replace empty old on empty bytes'

# replace with empty old and count
assert b'abc'.replace(b'', b'-', 2) == b'-a-bc', 'replace empty old with count'

# replace with count=0
assert b'hello'.replace(b'l', b'L', 0) == b'hello', 'replace with count 0'

# replace with too few args
try:
    b'hello'.replace(b'l')
    assert False, 'replace with too few args should raise TypeError'
except TypeError:
    pass

# replace with no args
try:
    b'hello'.replace()
    assert False, 'replace with no args should raise TypeError'
except TypeError:
    pass

# replace with str arg (type error)
try:
    b'hello'.replace('l', b'L')
    assert False, 'replace with str old should raise TypeError'
except TypeError:
    pass

try:
    b'hello'.replace(b'l', 'L')
    assert False, 'replace with str new should raise TypeError'
except TypeError:
    pass

# === bytes.center/ljust/rjust error paths ===
# justify with negative width
assert b'hello'.center(-1) == b'hello', 'center with negative width'
assert b'hello'.ljust(-1) == b'hello', 'ljust with negative width'
assert b'hello'.rjust(-1) == b'hello', 'rjust with negative width'

# fillbyte wrong length
try:
    b'hello'.center(10, b'ab')
    assert False, 'center with multi-byte fill should raise TypeError'
except TypeError:
    pass

try:
    b'hello'.ljust(10, b'ab')
    assert False, 'ljust with multi-byte fill should raise TypeError'
except TypeError:
    pass

try:
    b'hello'.rjust(10, b'ab')
    assert False, 'rjust with multi-byte fill should raise TypeError'
except TypeError:
    pass

# === bytes.zfill() edge cases ===
assert b'hello'.zfill(-1) == b'hello', 'zfill with negative width'
assert b'hello'.zfill(5) == b'hello', 'zfill with same width'

# === bytes.join() error paths ===
# join with non-bytes items
try:
    b','.join(['a', 'b'])
    assert False, 'join with str items should raise TypeError'
except TypeError:
    pass

# join with int items
try:
    b','.join([1, 2])
    assert False, 'join with int items should raise TypeError'
except TypeError:
    pass

# join with non-iterable
try:
    b','.join(123)
    assert False, 'join with non-iterable should raise TypeError'
except TypeError:
    pass

# join with heap-allocated bytes
b1 = bytes(b'hello')
b2 = bytes(b'world')
assert b' '.join([b1, b2]) == b'hello world', 'join with heap bytes'

# === bytes.hex() edge cases ===
# hex with too many args
try:
    b'\x01\x02'.hex(':', 1, 'extra')
    assert False, 'hex with too many args should raise TypeError'
except TypeError:
    pass

# hex with non-ASCII separator
try:
    b'\x01\x02'.hex('\xff')
    assert False, 'hex with non-ASCII sep should raise ValueError'
except ValueError:
    pass

# hex with empty string separator (should error -- not length 1)
try:
    b'\x01\x02'.hex('')
    assert False, 'hex with empty sep should raise ValueError'
except ValueError:
    pass

# hex with multi-char separator (should error)
try:
    b'\x01\x02'.hex('ab')
    assert False, 'hex with multi-char sep should raise ValueError'
except ValueError:
    pass

# hex with bytes_per_sep = 0 (no separator inserted)
assert b'\x01\x02\x03'.hex(':', 0) == '010203', 'hex with bytes_per_sep 0'

# hex on empty bytes with sep
assert b''.hex(':') == '', 'hex empty with sep'

# === bytes.fromhex() edge cases ===
# fromhex with non-str argument
try:
    bytes.fromhex(123)
    assert False, 'fromhex with int should raise TypeError'
except TypeError:
    pass


# === Unknown method on bytes ===
try:
    b'hello'.nonexistent_method()
    assert False, 'unknown method should raise AttributeError'
except AttributeError:
    pass

# Unknown method on heap bytes
try:
    bytes(b'hello').nonexistent_method()
    assert False, 'unknown method on heap bytes should raise AttributeError'
except AttributeError:
    pass

# === bytes.startswith/endswith with non-bytes arg ===
# startswith/endswith with non-bytes non-tuple (int)
try:
    b'hello'.startswith(123)
    assert False, 'startswith with int should raise TypeError'
except TypeError:
    pass

try:
    b'hello'.endswith(123)
    assert False, 'endswith with int should raise TypeError'
except TypeError:
    pass

# === Comparison operators ===
assert b'abc' < b'abd', 'bytes less than'
assert b'abc' <= b'abc', 'bytes less than or equal'
assert b'abd' > b'abc', 'bytes greater than'
assert b'abd' >= b'abd', 'bytes greater than or equal'
assert b'abc' == b'abc', 'bytes equal'
assert b'abc' != b'abd', 'bytes not equal'

# === Bytes concatenation ===
assert b'hello' + b' world' == b'hello world', 'bytes concatenation'
assert b'' + b'hello' == b'hello', 'bytes concat with empty left'
assert b'hello' + b'' == b'hello', 'bytes concat with empty right'

# === Heap bytes method calls ===
hb = bytes(b'Hello World')
assert hb.capitalize() == b'Hello world', 'heap bytes capitalize'
assert hb.title() == b'Hello World', 'heap bytes title'
assert hb.swapcase() == b'hELLO wORLD', 'heap bytes swapcase'
assert hb.isalpha() is False, 'heap bytes isalpha with space'
assert hb.find(b'World') == 6, 'heap bytes find'
assert hb.count(b'l') == 3, 'heap bytes count'
assert hb.startswith(b'Hello'), 'heap bytes startswith'
assert hb.endswith(b'World'), 'heap bytes endswith'
assert hb.strip() == b'Hello World', 'heap bytes strip'
assert hb.split(b' ') == [b'Hello', b'World'], 'heap bytes split'
assert hb.replace(b'World', b'Python') == b'Hello Python', 'heap bytes replace'
assert hb.center(20) == b'    Hello World     ', 'heap bytes center'
assert hb.ljust(20) == b'Hello World         ', 'heap bytes ljust'
assert hb.rjust(20) == b'         Hello World', 'heap bytes rjust'
assert hb.zfill(20) == b'000000000Hello World', 'heap bytes zfill'
assert hb.hex() == '48656c6c6f20576f726c64', 'heap bytes hex'

# === Split with sep=None keyword ===
assert b'a b c'.split(sep=None) == [b'a', b'b', b'c'], 'split with kwarg sep=None'
assert b'a b c'.rsplit(sep=None) == [b'a', b'b', b'c'], 'rsplit with kwarg sep=None'

# === bytes.fromhex on instance ===
# fromhex should work when called on an instance
assert b''.fromhex('4142') == b'AB', 'fromhex on empty instance'

# === Negative index normalization ===
assert b'hello'.find(b'l', -3, -1) == 2, 'find with negative start and end'
assert b'hello'.count(b'l', -4, -1) == 2, 'count with negative start and end'
assert b'hello'.startswith(b'llo', -3), 'startswith with negative start'
assert b'hello'.endswith(b'llo', -3), 'endswith with negative start'

# === Whitespace splitting edge cases ===
# Leading/trailing whitespace in split
assert b'  hello  '.split() == [b'hello'], 'split whitespace with padding'
assert b'\t\nhello\r\n'.split() == [b'hello'], 'split various whitespace'
assert b'  hello  '.rsplit() == [b'hello'], 'rsplit whitespace with padding'

# Whitespace splitting with maxsplit
assert b'  a  b  c  '.split(None, 1) == [b'a', b'b  c  '], 'split ws maxsplit 1'
assert b'  a  b  c  '.rsplit(None, 1) == [b'  a  b', b'c'], 'rsplit ws maxsplit 1'

# === bytes repr with double-quote-only string ===
# Bytes with only double quotes should still use single quotes
x = b'"hello"'
assert repr(x) == 'b\'"hello"\'', 'repr with double quotes only'

# === splitlines with keepends kwarg (various) ===
assert b'a\r\nb\nc'.splitlines() == [b'a', b'b', b'c'], 'splitlines crlf'
assert b'a\r\nb\nc'.splitlines(keepends=True) == [b'a\r\n', b'b\n', b'c'], 'splitlines crlf keepends'
assert b'a\rb\nc'.splitlines(True) == [b'a\r', b'b\n', b'c'], 'splitlines cr and lf keepends'
