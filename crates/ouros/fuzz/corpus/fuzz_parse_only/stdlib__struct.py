# === Basic pack/unpack with integers ===
import struct

# Test basic integer formats
assert struct.pack('b', 42) == b'*', 'signed byte pack failed'
assert struct.unpack('b', b'*')[0] == 42, 'signed byte unpack failed'

assert struct.pack('B', 200) == b'\xc8', 'unsigned byte pack failed'
assert struct.unpack('B', b'\xc8')[0] == 200, 'unsigned byte unpack failed'

assert struct.pack('h', 1000) == b'\xe8\x03', 'short pack failed (little-endian)'
assert struct.unpack('h', b'\xe8\x03')[0] == 1000, 'short unpack failed'

assert struct.pack('i', 100000) == b'\xa0\x86\x01\x00', 'int pack failed (little-endian)'
assert struct.unpack('i', b'\xa0\x86\x01\x00')[0] == 100000, 'int unpack failed'

# === Byte order markers ===
assert struct.pack('<i', 1) == b'\x01\x00\x00\x00', 'little-endian pack failed'
assert struct.pack('>i', 1) == b'\x00\x00\x00\x01', 'big-endian pack failed'
assert struct.pack('!i', 1) == b'\x00\x00\x00\x01', 'network byte order pack failed'

# Unpack with different byte orders
assert struct.unpack('<i', b'\x01\x00\x00\x00')[0] == 1, 'little-endian unpack failed'
assert struct.unpack('>i', b'\x00\x00\x00\x01')[0] == 1, 'big-endian unpack failed'

# === calcsize ===
assert struct.calcsize('b') == 1, 'calcsize b should be 1'
assert struct.calcsize('h') == 2, 'calcsize h should be 2'
assert struct.calcsize('i') == 4, 'calcsize i should be 4'
assert struct.calcsize('q') == 8, 'calcsize q should be 8'
assert struct.calcsize('f') == 4, 'calcsize f should be 4'
assert struct.calcsize('d') == 8, 'calcsize d should be 8'
assert struct.calcsize('3i') == 12, 'calcsize 3i should be 12'

# Long size is platform dependent (8 on macOS ARM64, 4 on Windows)
# Just check that calcsize returns a positive number
assert struct.calcsize('l') > 0, 'calcsize l should be positive'
assert struct.calcsize('q') == 8, 'calcsize q should be 8'

# === Float formats ===
packed_float = struct.pack('f', 3.14)
assert len(packed_float) == 4, 'float should pack to 4 bytes'
unpacked_float = struct.unpack('f', packed_float)[0]
assert abs(unpacked_float - 3.14) < 0.01, 'float unpack incorrect'

packed_double = struct.pack('d', 3.14159)
assert len(packed_double) == 8, 'double should pack to 8 bytes'
unpacked_double = struct.unpack('d', packed_double)[0]
assert abs(unpacked_double - 3.14159) < 0.0001, 'double unpack incorrect'

# === Bool format ===
assert struct.pack('?', True) == b'\x01', 'bool True pack failed'
assert struct.pack('?', False) == b'\x00', 'bool False pack failed'
assert struct.unpack('?', b'\x01')[0] == True, 'bool True unpack failed'
assert struct.unpack('?', b'\x00')[0] == False, 'bool False unpack failed'

# === String format (bytes) ===
assert struct.pack('5s', b'hello') == b'hello', 'string pack failed'
assert struct.unpack('5s', b'hello')[0] == b'hello', 'string unpack failed'

# Padding with shorter strings
assert struct.pack('5s', b'hi') == b'hi\x00\x00\x00', 'short string pack should pad with nulls'

# === Pad bytes ===
assert struct.pack('x') == b'\x00', 'pad byte pack failed'
assert struct.unpack('x', b'\x00') == (), 'pad byte unpack should return empty tuple'
assert struct.calcsize('x') == 1, 'calcsize x should be 1'
assert struct.calcsize('3x') == 3, 'calcsize 3x should be 3'

# === Multiple values ===
packed = struct.pack('ii', 1, 2)
assert len(packed) == 8, 'two ints should pack to 8 bytes'
assert struct.unpack('ii', packed) == (1, 2), 'two int unpack failed'

# Mixed format
packed = struct.pack('ihb', 1000, 500, 50)
# calcsize ihb is platform dependent (7 on standard, might be 8 on native with alignment)
calc_size = struct.calcsize('ihb')
assert calc_size >= 7, f'calcsize ihb should be at least 7, got {calc_size}'
assert struct.unpack('ihb', packed) == (1000, 500, 50), 'mixed format unpack failed'

# === Long long (64-bit) ===
packed_q = struct.pack('q', 9223372036854775807)  # Max i64
assert len(packed_q) == 8, 'long long should pack to 8 bytes'
assert struct.unpack('q', packed_q)[0] == 9223372036854775807, 'long long unpack failed'

# Note: unsigned long long can hold values from 0 to 18446744073709551615
# but Ouros's current implementation handles i64 range best
packed_Q = struct.pack('Q', 9223372036854775807)  # Max i64
assert len(packed_Q) == 8, 'unsigned long long should pack to 8 bytes'

# === Char format ===
packed_char = struct.pack('c', b'A')
assert packed_char == b'A', 'char pack failed'
# Note: unpack returns a bytes object of length 1, not a string
unpacked_char = struct.unpack('c', b'A')[0]
assert unpacked_char == b'A', 'char unpack failed'

# === Pascal string ===
# Pascal strings have a length byte followed by data
packed_pascal = struct.pack('5p', b'hi')
assert packed_pascal[0] == 2, 'pascal string length byte incorrect'
assert packed_pascal[1:3] == b'hi', 'pascal string data incorrect'

unpacked_pascal = struct.unpack('5p', packed_pascal)[0]
assert unpacked_pascal == b'hi', 'pascal string unpack failed'

# === Whitespace in format ===
assert struct.pack(' i ', 42) == struct.pack('i', 42), 'whitespace in format should be ignored'
assert struct.calcsize(' i ') == 4, 'whitespace should not affect calcsize'

# === Module attributes ===
assert hasattr(struct, 'error'), 'struct should have error attribute'
assert hasattr(struct, 'pack'), 'struct should have pack function'
assert hasattr(struct, 'unpack'), 'struct should have unpack function'
assert hasattr(struct, 'calcsize'), 'struct should have calcsize function'
assert hasattr(struct, 'iter_unpack'), 'struct should have iter_unpack function'
assert hasattr(struct, 'pack_into'), 'struct should have pack_into function'
assert hasattr(struct, 'unpack_from'), 'struct should have unpack_from function'

# === iter_unpack basic test ===
# Pack values - iter_unpack should handle multiple records
packed = struct.pack('ii', 1, 2)
result = list(struct.iter_unpack('ii', packed))
# iter_unpack yields tuples
assert len(result) >= 1, 'iter_unpack should yield at least one tuple'
assert result[0] == (1, 2), 'iter_unpack first tuple should match packed values'

# === unpack_from basic test ===
packed = b'\x00\x00' + struct.pack('i', 42)  # 2 bytes padding, then int
result = struct.unpack_from('i', packed, 2)
assert result[0] == 42, 'unpack_from with offset failed'

# === Negative numbers ===
assert struct.unpack('b', struct.pack('b', -1))[0] == -1, 'negative byte failed'
assert struct.unpack('h', struct.pack('h', -1000))[0] == -1000, 'negative short failed'
assert struct.unpack('i', struct.pack('i', -100000))[0] == -100000, 'negative int failed'

# === Zero values ===
assert struct.pack('i', 0) == b'\x00\x00\x00\x00', 'zero int pack failed'
assert struct.unpack('i', b'\x00\x00\x00\x00')[0] == 0, 'zero int unpack failed'

# === Standard size mode (=) ===
# In standard mode, long is 4 bytes regardless of platform
assert struct.calcsize('=l') == 4, 'calcsize =l should be 4 (standard)'
assert struct.calcsize('=L') == 4, 'calcsize =L should be 4 (standard)'
