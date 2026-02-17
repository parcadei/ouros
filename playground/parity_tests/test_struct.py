import struct

# === Struct class ===
try:
    s = struct.Struct('>I')
    print('struct_class_type', type(s) is struct.Struct)
    print('struct_class_size', s.size)
    print('struct_class_format', s.format)

    # Struct.pack
    packed = s.pack(42)
    print('struct_pack', packed)

    # Struct.unpack
    unpacked = s.unpack(packed)
    print('struct_unpack', unpacked)

    # Struct.pack_into
    import array
    buf = bytearray(8)
    s.pack_into(buf, 0, 0xDEADBEEF)
    print('struct_pack_into', buf[:4])

    # Struct.unpack_from
    result = s.unpack_from(buf, 0)
    print('struct_unpack_from', result)

    # Struct.iter_unpack
    data = b'\x00\x00\x00\x01\x00\x00\x00\x02'
    results = list(s.iter_unpack(data))
    print('struct_iter_unpack', results)
except Exception as e:
    print('SKIP_Struct_class', type(e).__name__, e)

# === pack various formats ===
try:
    # Big-endian unsigned int
    packed_uint_be = struct.pack('>I', 42)
    print('pack_uint_be', packed_uint_be)

    # Little-endian unsigned short
    packed_ushort_le = struct.pack('<H', 1000)
    print('pack_ushort_le', packed_ushort_le)

    # Network (big-endian) double
    packed_double_net = struct.pack('!d', 3.14159)
    print('pack_double_net', packed_double_net)

    # Little-endian signed int
    packed_int_le = struct.pack('<i', -12345)
    print('pack_int_le', packed_int_le)

    # Big-endian float
    packed_float_be = struct.pack('>f', 2.5)
    print('pack_float_be', packed_float_be)

    # Signed byte
    packed_byte = struct.pack('b', -50)
    print('pack_byte', packed_byte)

    # Unsigned byte
    packed_ubyte = struct.pack('B', 200)
    print('pack_ubyte', packed_ubyte)

    # Bool (True)
    packed_bool_true = struct.pack('?', True)
    print('pack_bool_true', packed_bool_true)

    # Bool (False)
    packed_bool_false = struct.pack('?', False)
    print('pack_bool_false', packed_bool_false)

    # Short (signed)
    packed_short = struct.pack('h', -30000)
    print('pack_short', packed_short)

    # Unsigned short
    packed_ushort = struct.pack('H', 60000)
    print('pack_ushort', packed_ushort)

    # Long long (signed)
    packed_longlong = struct.pack('q', -9007199254740992)
    print('pack_longlong', packed_longlong)

    # Unsigned long long
    packed_ulonglong = struct.pack('Q', 9007199254740992)
    print('pack_ulonglong', packed_ulonglong)

    # Pad byte
    packed_pad = struct.pack('x')
    print('pack_pad', packed_pad)

    # Char (bytes of length 1)
    packed_char = struct.pack('c', b'A')
    print('pack_char', packed_char)
except Exception as e:
    print('SKIP_pack_various_formats', type(e).__name__, e)

# === Half precision float (e format) ===
try:
    try:
        packed_half = struct.pack('e', 1.5)
        print('pack_half_float', packed_half)
        unpacked_half = struct.unpack('e', packed_half)
        print('unpack_half_float', unpacked_half)
    except struct.error:
        print('pack_half_float', 'not_supported')
except Exception as e:
    print('SKIP_Half_precision_float_(e_format)', type(e).__name__, e)

# === ssize_t and size_t ===
try:
    try:
        packed_ssize = struct.pack('n', 42)
        print('pack_ssize_t', packed_ssize)
    except struct.error:
        print('pack_ssize_t', 'not_supported')

    try:
        packed_size = struct.pack('N', 42)
        print('pack_size_t', packed_size)
    except struct.error:
        print('pack_size_t', 'not_supported')
except Exception as e:
    print('SKIP_ssize_t_and_size_t', type(e).__name__, e)

# === void pointer ===
try:
    try:
        packed_ptr = struct.pack('P', 0)
        print('pack_void_ptr', packed_ptr)
    except struct.error:
        print('pack_void_ptr', 'not_supported')
except Exception as e:
    print('SKIP_void_pointer', type(e).__name__, e)

# === unpack ===
try:
    # Unpack what was packed and verify
    unpacked_uint_be = struct.unpack('>I', packed_uint_be)
    print('unpack_uint_be', unpacked_uint_be)
    assert unpacked_uint_be == (42,), 'unpack_uint_be failed'

    unpacked_ushort_le = struct.unpack('<H', packed_ushort_le)
    print('unpack_ushort_le', unpacked_ushort_le)
    assert unpacked_ushort_le == (1000,), 'unpack_ushort_le failed'

    unpacked_int_le = struct.unpack('<i', packed_int_le)
    print('unpack_int_le', unpacked_int_le)
    assert unpacked_int_le == (-12345,), 'unpack_int_le failed'

    unpacked_byte = struct.unpack('b', packed_byte)
    print('unpack_byte', unpacked_byte)
    assert unpacked_byte == (-50,), 'unpack_byte failed'

    unpacked_ubyte = struct.unpack('B', packed_ubyte)
    print('unpack_ubyte', unpacked_ubyte)
    assert unpacked_ubyte == (200,), 'unpack_ubyte failed'

    unpacked_bool_true = struct.unpack('?', packed_bool_true)
    print('unpack_bool_true', unpacked_bool_true)
    assert unpacked_bool_true == (True,), 'unpack_bool_true failed'

    unpacked_char = struct.unpack('c', packed_char)
    print('unpack_char', unpacked_char)
    assert unpacked_char == (b'A',), 'unpack_char failed'
except Exception as e:
    print('SKIP_unpack', type(e).__name__, e)

# === calcsize ===
try:
    size_uint_be = struct.calcsize('>I')
    print('calcsize_uint_be', size_uint_be)
    assert size_uint_be == 4, 'calcsize_uint_be failed'

    size_double = struct.calcsize('d')
    print('calcsize_double', size_double)
    assert size_double == 8, 'calcsize_double failed'

    size_float = struct.calcsize('f')
    print('calcsize_float', size_float)
    assert size_float == 4, 'calcsize_float failed'

    size_short = struct.calcsize('h')
    print('calcsize_short', size_short)
    assert size_short == 2, 'calcsize_short failed'

    size_longlong = struct.calcsize('q')
    print('calcsize_longlong', size_longlong)
    assert size_longlong == 8, 'calcsize_longlong failed'

    # Verify calcsize matches packed data length
    assert struct.calcsize('>I') == len(packed_uint_be), 'calcsize mismatch for uint'
    assert struct.calcsize('<H') == len(packed_ushort_le), 'calcsize mismatch for ushort'
except Exception as e:
    print('SKIP_calcsize', type(e).__name__, e)

# === byte order specifiers ===
try:
    # Native (@)
    native_short = struct.pack('@h', 1000)
    print('pack_native_short', native_short)
    print('calcsize_native_short', struct.calcsize('@h'))

    # Native standard (=)
    std_int = struct.pack('=i', 50000)
    print('pack_std_int', std_int)
    print('calcsize_std_int', struct.calcsize('=i'))

    # Little-endian (<)
    le_long = struct.pack('<l', 123456)
    print('pack_le_long', le_long)

    # Big-endian (>)
    be_long = struct.pack('>l', 123456)
    print('pack_be_long', be_long)

    # Network/big-endian (!)
    net_long = struct.pack('!l', 123456)
    print('pack_net_long', net_long)

    # Verify big-endian and network are the same
    assert be_long == net_long, 'big-endian and network should be identical'
except Exception as e:
    print('SKIP_byte_order_specifiers', type(e).__name__, e)

# === multiple values ===
try:
    packed_multi = struct.pack('>HH', 1, 2)
    print('pack_multi_hh', packed_multi)
    unpacked_multi = struct.unpack('>HH', packed_multi)
    print('unpack_multi_hh', unpacked_multi)
    assert unpacked_multi == (1, 2), 'unpack multiple values failed'

    packed_multi3 = struct.pack('<iif', 10, 20, 3.5)
    print('pack_multi_iif', packed_multi3)
    unpacked_multi3 = struct.unpack('<iif', packed_multi3)
    print('unpack_multi_iif', unpacked_multi3)
    assert unpacked_multi3[0] == 10, 'unpack multi3 first value failed'
    assert unpacked_multi3[1] == 20, 'unpack multi3 second value failed'
    assert abs(unpacked_multi3[2] - 3.5) < 0.001, 'unpack multi3 third value failed'

    packed_mixed = struct.pack('>Bf?', 127, 1.5, True)
    print('pack_mixed_bf', packed_mixed)
    unpacked_mixed = struct.unpack('>Bf?', packed_mixed)
    print('unpack_mixed_bf', unpacked_mixed)
    assert unpacked_mixed[0] == 127, 'unpack mixed first value failed'
    assert abs(unpacked_mixed[1] - 1.5) < 0.001, 'unpack mixed second value failed'
    assert unpacked_mixed[2] == True, 'unpack mixed third value failed'
except Exception as e:
    print('SKIP_multiple_values', type(e).__name__, e)

# === string formats ===
try:
    # Fixed-size string (5s)
    packed_string = struct.pack('5s', b'hello')
    print('pack_string_5s', packed_string)
    unpacked_string = struct.unpack('5s', packed_string)
    print('unpack_string_5s', unpacked_string)
    assert unpacked_string == (b'hello',), 'unpack string failed'

    # String shorter than format pads with nulls
    packed_short_str = struct.pack('10s', b'hi')
    print('pack_short_string', packed_short_str)
    unpacked_short_str = struct.unpack('10s', packed_short_str)
    print('unpack_short_string', unpacked_short_str)
    assert unpacked_short_str == (b'hi\x00\x00\x00\x00\x00\x00\x00\x00',), 'unpack short string failed'

    # String longer than format truncates
    packed_long_str = struct.pack('3s', b'hello')
    print('pack_long_string', packed_long_str)
    unpacked_long_str = struct.unpack('3s', packed_long_str)
    print('unpack_long_string', unpacked_long_str)
    assert unpacked_long_str == (b'hel',), 'unpack long string failed'

    # Pascal string (first byte is length)
    packed_pascal = struct.pack('10p', b'hello')
    print('pack_pascal_10p', packed_pascal)
    # Note: pascal string stores length in first byte, max 255 chars
except Exception as e:
    print('SKIP_string_formats', type(e).__name__, e)

# === repeat counts ===
try:
    # Three signed bytes
    packed_3b = struct.pack('3b', 1, 2, 3)
    print('pack_3b', packed_3b)
    unpacked_3b = struct.unpack('3b', packed_3b)
    print('unpack_3b', unpacked_3b)
    assert unpacked_3b == (1, 2, 3), 'unpack 3b failed'

    # Four unsigned shorts
    packed_4H = struct.pack('4H', 100, 200, 300, 400)
    print('pack_4H', packed_4H)
    unpacked_4H = struct.unpack('4H', packed_4H)
    print('unpack_4H', unpacked_4H)
    assert unpacked_4H == (100, 200, 300, 400), 'unpack 4H failed'

    # Mixed with repeat
    packed_mixed_repeat = struct.pack('>2H2b', 1, 2, -1, -2)
    print('pack_mixed_repeat', packed_mixed_repeat)
    unpacked_mixed_repeat = struct.unpack('>2H2b', packed_mixed_repeat)
    print('unpack_mixed_repeat', unpacked_mixed_repeat)
    assert unpacked_mixed_repeat == (1, 2, -1, -2), 'unpack mixed repeat failed'
except Exception as e:
    print('SKIP_repeat_counts', type(e).__name__, e)

# === edge cases ===
try:
    # Empty format
    packed_empty = struct.pack('')
    print('pack_empty', packed_empty)
    print('calcsize_empty', struct.calcsize(''))
    assert packed_empty == b'', 'pack empty failed'

    # Format with only pad bytes
    packed_pads = struct.pack('xxx')
    print('pack_pads', packed_pads)
    print('calcsize_pads', struct.calcsize('xxx'))
    assert packed_pads == b'\x00\x00\x00', 'pack pads failed'
    assert len(packed_pads) == 3, 'pack pads length failed'

    # Large positive number
    packed_large = struct.pack('>Q', 18446744073709551615)  # max u64
    print('pack_large_uint64', packed_large)
    unpacked_large = struct.unpack('>Q', packed_large)
    print('unpack_large_uint64', unpacked_large)
    assert unpacked_large == (18446744073709551615,), 'unpack large uint64 failed'

    # Large negative number
    packed_neg_large = struct.pack('>q', -9223372036854775808)  # min i64
    print('pack_large_neg_int64', packed_neg_large)
    unpacked_neg_large = struct.unpack('>q', packed_neg_large)
    print('unpack_large_neg_int64', unpacked_neg_large)
    assert unpacked_neg_large == (-9223372036854775808,), 'unpack large neg int64 failed'

    # Negative numbers of various sizes
    packed_neg_int = struct.pack('i', -2147483648)  # min i32
    print('pack_neg_int32', packed_neg_int)
    unpacked_neg_int = struct.unpack('i', packed_neg_int)
    print('unpack_neg_int32', unpacked_neg_int)
    assert unpacked_neg_int == (-2147483648,), 'unpack neg int32 failed'

    packed_neg_short = struct.pack('h', -32768)  # min i16
    print('pack_neg_int16', packed_neg_short)
    unpacked_neg_short = struct.unpack('h', packed_neg_short)
    print('unpack_neg_int16', unpacked_neg_short)
    assert unpacked_neg_short == (-32768,), 'unpack neg int16 failed'

    # Zero values
    packed_zero_uint = struct.pack('I', 0)
    print('pack_zero_uint', packed_zero_uint)
    packed_zero_int = struct.pack('i', 0)
    print('pack_zero_int', packed_zero_int)
    packed_zero_float = struct.pack('f', 0.0)
    print('pack_zero_float', packed_zero_float)
    packed_neg_zero_float = struct.pack('f', -0.0)
    print('pack_neg_zero_float', packed_neg_zero_float)

    # Float special values
    packed_inf = struct.pack('f', float('inf'))
    print('pack_inf', packed_inf)
    packed_neg_inf = struct.pack('f', float('-inf'))
    print('pack_neg_inf', packed_neg_inf)
except Exception as e:
    print('SKIP_edge_cases', type(e).__name__, e)

# === error exception ===
try:
    print('error_class_exists', hasattr(struct, 'error'))
    print('error_is_exception', issubclass(struct.error, Exception))

    # Verify error can be raised and caught
    try:
        raise struct.error('test error')
    except struct.error as e:
        print('error_caught', str(e))

    # Verify error is raised on bad format
    try:
        struct.pack('bad format', 1)
        print('error_bad_format', False)
    except struct.error:
        print('error_bad_format', True)
except Exception as e:
    print('SKIP_error_exception', type(e).__name__, e)

# === unpack_from and pack_into ===
try:
    # unpack_from with offset
    buffer = b'\x00\x00\x01\x02\x03\x04'
    unpacked_from = struct.unpack_from('>I', buffer, 2)
    print('unpack_from_offset', unpacked_from)
    assert unpacked_from == (0x01020304,), 'unpack_from with offset failed'

    # pack_into
    buf = bytearray(8)
    struct.pack_into('>II', buf, 0, 1, 2)
    print('pack_into', buf)
    unpacked_into = struct.unpack('>II', buf)
    print('unpack_after_pack_into', unpacked_into)
    assert unpacked_into == (1, 2), 'pack_into failed'

    # pack_into with offset
    buf2 = bytearray(10)
    struct.pack_into('>I', buf2, 2, 0xDEADBEEF)
    print('pack_into_offset', buf2)
    unpacked_into_offset = struct.unpack_from('>I', buf2, 2)
    print('unpack_from_after_pack_into_offset', unpacked_into_offset)
    assert unpacked_into_offset == (0xDEADBEEF,), 'pack_into with offset failed'
except Exception as e:
    print('SKIP_unpack_from_and_pack_into', type(e).__name__, e)

# === iter_unpack ===
try:
    # iter_unpack unpacks repeated structures
    buffer = struct.pack('>HH', 1, 2) + struct.pack('>HH', 3, 4) + struct.pack('>HH', 5, 6)
    result = list(struct.iter_unpack('>HH', buffer))
    print('iter_unpack', result)
    assert result == [(1, 2), (3, 4), (5, 6)], 'iter_unpack failed'

    # iter_unpack with single item
    buffer_single = struct.pack('>I', 100)
    result_single = list(struct.iter_unpack('>I', buffer_single))
    print('iter_unpack_single', result_single)
    assert result_single == [(100,)], 'iter_unpack single failed'

    # iter_unpack empty buffer
    result_empty = list(struct.iter_unpack('>I', b''))
    print('iter_unpack_empty', result_empty)
    assert result_empty == [], 'iter_unpack empty failed'

    # iter_unpack requires buffer of exact multiple size
    # This will raise an error if not exact multiple
    try:
        buffer_partial = struct.pack('>HH', 1, 2) + b'\x00\x01'
        result_partial = list(struct.iter_unpack('>HH', buffer_partial))
        print('iter_unpack_partial', 'unexpected_success')
    except struct.error:
        # Expected behavior - partial buffer raises error
        print('iter_unpack_partial_raises', True)
except Exception as e:
    print('SKIP_iter_unpack', type(e).__name__, e)
