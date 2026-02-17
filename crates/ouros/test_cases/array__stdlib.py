import array


def assert_raises(exc_type, func, contains=None):
    try:
        func()
    except Exception as e:
        assert isinstance(e, exc_type), f'expected {exc_type}, got {type(e)}: {e}'
        if contains is not None:
            assert contains in str(e), f"expected '{contains}' in error: {e}"
    else:
        assert False, f'expected {exc_type} to be raised'


# === Module API ===
for tc in 'bBuhHiIlLqQfd':
    assert tc in array.typecodes
assert array.ArrayType is array.array

# === Constructor and attributes ===
a = array.array('i', [1, 2, 3])
assert a.typecode == 'i'
assert a.itemsize == 4
assert len(a) == 3
assert repr(a) == "array('i', [1, 2, 3])"

assert array.array('i').tolist() == []
assert array.array('b', b'\x01\xff').tolist() == [1, -1]
assert array.array('u', 'ab').tolist() == ['a', 'b']
assert repr(array.array('u', 'ab')) == "array('u', 'ab')"

# === Basic mutation APIs ===
a.append(4)
assert a.tolist() == [1, 2, 3, 4]

a.extend([5, 6])
assert a.tolist() == [1, 2, 3, 4, 5, 6]

a.extend(array.array('i', [7, 8]))
assert a.tolist() == [1, 2, 3, 4, 5, 6, 7, 8]
assert_raises(TypeError, lambda: a.extend(array.array('h', [9])), 'same kind')

a.insert(0, -1)
a.insert(999, 9)
a.insert(-999, -2)
assert a.tolist() == [-2, -1, 1, 2, 3, 4, 5, 6, 7, 8, 9]

assert a.pop() == 9
assert a.pop(0) == -2
assert_raises(IndexError, lambda: a.pop(999), 'range')

a.remove(-1)
assert -1 not in a
assert_raises(ValueError, lambda: a.remove(9999), 'not in array')

# === Indexing and slicing ===
b = array.array('i', [0, 1, 2, 3, 4, 5])
assert b[0] == 0
assert b[-1] == 5
assert b[1:5].tolist() == [1, 2, 3, 4]
assert b[::-1].tolist() == [5, 4, 3, 2, 1, 0]

b[1] = 100
assert b.tolist() == [0, 100, 2, 3, 4, 5]

b[2:4] = array.array('i', [20, 30, 40])
assert b.tolist() == [0, 100, 20, 30, 40, 4, 5]

b[::2] = array.array('i', [9, 8, 7, 6])
assert b.tolist() == [9, 100, 8, 30, 7, 4, 6]

# explicit extended-slice mismatch on assignment target
def _assign_extended_slice_mismatch():
    x = array.array('i', [0, 1, 2, 3])
    x[::2] = array.array('i', [5])


def _assign_list_to_slice():
    x = array.array('i', [0, 1])
    x[:1] = [9]


assert_raises(ValueError, _assign_extended_slice_mismatch, 'extended slice')
assert_raises(TypeError, _assign_list_to_slice, 'array slice')

c = array.array('i', [10, 11, 12, 13, 14, 15])
del c[1]
assert c.tolist() == [10, 12, 13, 14, 15]
del c[1:4]
assert c.tolist() == [10, 15]

d = array.array('i', [0, 1, 2, 3, 4, 5, 6])
del d[::2]
assert d.tolist() == [1, 3, 5]

# === Query APIs ===
e = array.array('i', [1, 2, 2, 3])
assert 2 in e
assert 9 not in e
assert e.index(2) == 1
assert_raises(ValueError, lambda: e.index(9), 'not in array')
assert e.count(2) == 2

e.reverse()
assert e.tolist() == [3, 2, 2, 1]

# === Iteration ===
assert list(e.__iter__()) == [3, 2, 2, 1]
if hasattr(e, '__reversed__'):
    assert list(e.__reversed__()) == [1, 2, 2, 3]
else:
    assert list(reversed(e)) == [1, 2, 2, 3]

# === Conversion APIs ===
f = array.array('h', [0x1234, -2])
orig = f.tobytes()
assert isinstance(orig, bytes)

f2 = array.array('h')
f2.frombytes(orig)
assert f2.tolist() == [0x1234, -2]

f3 = array.array('h')
f3.fromlist([7, 8])
assert f3.tolist() == [7, 8]
assert_raises(TypeError, lambda: f3.fromlist((1, 2)), 'list')

assert f3.tolist() == [7, 8]

addr, length = f3.buffer_info()
assert isinstance(addr, int)
assert isinstance(length, int)
assert length == 2

# byteswap should be involutive
f.byteswap()
f.byteswap()
assert f.tobytes() == orig

# === Operators and comparisons ===
g = array.array('i', [1, 2])
h = array.array('i', [3])

assert (g + h).tolist() == [1, 2, 3]
assert_raises(TypeError, lambda: g + array.array('h', [3]), 'bad argument type')

x = array.array('i', [1])
x += array.array('i', [2, 3])
assert x.tolist() == [1, 2, 3]
def _iadd_non_array():
    x = array.array('i', [1])
    x += [2]


assert_raises(TypeError, _iadd_non_array, 'array')

assert (array.array('i', [4, 5]) * 2).tolist() == [4, 5, 4, 5]
assert (2 * array.array('i', [4, 5])).tolist() == [4, 5, 4, 5]

y = array.array('i', [9])
y *= 3
assert y.tolist() == [9, 9, 9]

assert array.array('i', [1, 2]) == array.array('h', [1, 2])
assert array.array('i', [1, 2]) != array.array('i', [1, 3])
assert array.array('i', [1, 2]) < array.array('i', [1, 3])
assert array.array('i', [1, 2]) <= array.array('i', [1, 2])
assert array.array('i', [2]) > array.array('i', [1, 100])
assert array.array('i', [2]) >= array.array('i', [2])

# === Roundtrip all typecodes ===
roundtrip_samples = {
    'b': [-2, 0, 2],
    'B': [0, 200, 255],
    'u': ['a', 'Z', '0'],
    'h': [-123, 0, 123],
    'H': [0, 123, 65535],
    'i': [-123456, 0, 123456],
    'I': [0, 123456, 2**32 - 1],
    'l': [-123456789, 0, 123456789],
    'L': [0, 123456789, 2**63],
    'q': [-(2**63), -1, 2**63 - 1],
    'Q': [0, 123456789, 2**64 - 1],
    'f': [0.5, -1.5, 10.0],
    'd': [0.5, -1.5, 10.0],
}

for tc, values in roundtrip_samples.items():
    arr = array.array(tc, values)
    blob = arr.tobytes()
    out = array.array(tc)
    out.frombytes(blob)
    assert out.tolist() == arr.tolist(), f'roundtrip mismatch for {tc}'

# === Error paths ===
assert_raises(TypeError, lambda: array.array(1), 'unicode character')
assert_raises(TypeError, lambda: array.array('zz'), 'length 2')
assert_raises(ValueError, lambda: array.array('?'), 'bad typecode')
assert_raises(TypeError, lambda: array.array('i', 'abc'), 'cannot use a str')
assert_raises(ValueError, lambda: array.array('i', b'abc'), 'item size')

assert_raises(OverflowError, lambda: array.array('B').append(-1), None)
assert_raises(OverflowError, lambda: array.array('B').append(256), None)
assert_raises(OverflowError, lambda: array.array('b').append(128), None)

assert_raises(TypeError, lambda: array.array('i').frombytes('abc'), 'bytes-like')
assert_raises(ValueError, lambda: array.array('i').frombytes(b'123'), 'item size')

empty = array.array('i')
assert_raises(IndexError, lambda: empty.pop(), 'empty')
assert_raises(IndexError, lambda: empty[0], 'range')
def _setitem_empty():
    x = array.array('i')
    x[0] = 1


def _delitem_empty():
    x = array.array('i')
    del x[0]


assert_raises(IndexError, _setitem_empty, 'range')
assert_raises(IndexError, _delitem_empty, 'range')
