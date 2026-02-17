# === Module import and public API surface ===
import binascii

expected_public = [
    'Error',
    'Incomplete',
    'a2b_base64',
    'a2b_hex',
    'a2b_qp',
    'a2b_uu',
    'b2a_base64',
    'b2a_hex',
    'b2a_qp',
    'b2a_uu',
    'crc32',
    'crc_hqx',
    'hexlify',
    'unhexlify',
]
for name in expected_public:
    assert hasattr(binascii, name), f'missing public attribute: {name}'


# === Exception classes ===
err = binascii.Error('boom')
inc = binascii.Incomplete('partial')
assert isinstance(err, Exception), 'Error should be an exception type'
assert isinstance(inc, Exception), 'Incomplete should be an exception type'
assert err.args == ('boom',), 'Error args should be preserved'
assert inc.args == ('partial',), 'Incomplete args should be preserved'


def expect_error(fn, expected_message):
    try:
        fn()
        assert False, f'expected error: {expected_message}'
    except Exception as exc:
        assert str(exc) == expected_message, f'expected {expected_message!r}, got {str(exc)!r}'


# === hexlify / b2a_hex ===
assert binascii.hexlify(b'\x00\xffab') == b'00ff6162', 'hexlify basic bytes'
assert binascii.hexlify(bytearray(b'\x00\xffab')) == b'00ff6162', 'hexlify accepts bytearray'
assert binascii.hexlify(b'UUDDLRLRAB', b':', 2) == b'5555:4444:4c52:4c52:4142', 'hexlify sep from right'
assert binascii.hexlify(b'UUDDLRLRAB', ':', -4) == b'55554444:4c524c52:4142', 'hexlify sep from left'
assert binascii.hexlify(b'abc', bytes_per_sep=2) == b'616263', 'bytes_per_sep ignored without sep'
assert binascii.b2a_hex(b'abc', b':', 2) == b'61:6263', 'b2a_hex alias behavior'

expect_error(lambda: binascii.hexlify(b'abc', b'--'), 'sep must be length 1.')
expect_error(lambda: binascii.hexlify('abc'), "a bytes-like object is required, not 'str'")


# === unhexlify / a2b_hex ===
assert binascii.unhexlify(b'00ff6162') == b'\x00\xffab', 'unhexlify bytes input'
assert binascii.unhexlify('00ff6162') == b'\x00\xffab', 'unhexlify str input'
assert binascii.unhexlify(bytearray(b'616263')) == b'abc', 'unhexlify accepts bytearray'
assert binascii.a2b_hex('6162') == b'ab', 'a2b_hex alias behavior'

expect_error(lambda: binascii.unhexlify('abc'), 'Odd-length string')
expect_error(lambda: binascii.unhexlify('abxz'), 'Non-hexadecimal digit found')
expect_error(lambda: binascii.unhexlify('abé0'), 'string argument should contain only ASCII characters')
expect_error(lambda: binascii.unhexlify(1), "argument should be bytes, buffer or ASCII string, not 'int'")


# === crc32 / crc_hqx ===
assert binascii.crc32(b'hello') == 907060870, 'crc32 basic'
assert binascii.crc32(b'hello', 1) == 191926070, 'crc32 with seed'
assert binascii.crc32(b'hello', -1) == 265137764, 'crc32 wraps negative seed'
assert binascii.crc32(b'hello', 2**65 + 3) == 1907416150, 'crc32 wraps large seed'

assert binascii.crc_hqx(b'123456789', 0) == 12739, 'crc_hqx basic vector'
assert binascii.crc_hqx(b'123456789', 0xFFFF) == 10673, 'crc_hqx with nonzero seed'
assert binascii.crc_hqx(b'abc', -1) == 20810, 'crc_hqx wraps negative seed'
assert binascii.crc_hqx(b'abc', 1 << 40) == 40406, 'crc_hqx wraps large seed'

expect_error(lambda: binascii.crc32('hello'), "a bytes-like object is required, not 'str'")
expect_error(lambda: binascii.crc_hqx('hello', 0), "a bytes-like object is required, not 'str'")


# === a2b_base64 / b2a_base64 ===
assert binascii.a2b_base64(b'YQ==') == b'a', 'a2b_base64 bytes input'
assert binascii.a2b_base64('YQ==') == b'a', 'a2b_base64 str input'
assert binascii.a2b_base64(b'Y!Q==') == b'a', 'a2b_base64 non-strict ignores non-base64 bytes'
assert binascii.a2b_base64(b'YQ==', strict_mode=True) == b'a', 'a2b_base64 strict valid input'
expect_error(lambda: binascii.a2b_base64(b'Y Q==', strict_mode=True), 'Only base64 data is allowed')
expect_error(
    lambda: binascii.a2b_base64(b'A'),
    'Invalid base64-encoded string: number of data characters (1) cannot be 1 more than a multiple of 4',
)

assert binascii.b2a_base64(b'a') == b'YQ==\n', 'b2a_base64 default newline'
assert binascii.b2a_base64(b'a', newline=False) == b'YQ==', 'b2a_base64 newline=False'


# === a2b_qp / b2a_qp ===
assert binascii.a2b_qp(b'a=3Db') == b'a=b', 'a2b_qp hex decode'
assert binascii.a2b_qp(b'a_b', header=True) == b'a b', 'a2b_qp header underscore-to-space'
assert binascii.a2b_qp(b'abc=\nxyz') == b'abcxyz', 'a2b_qp soft line break'
assert binascii.a2b_qp('a=3Db') == b'a=b', 'a2b_qp ascii string input'
expect_error(lambda: binascii.a2b_qp('é'), 'string argument should contain only ASCII characters')

assert binascii.b2a_qp(b'a=b') == b'a=3Db', 'b2a_qp equals escaping'
assert binascii.b2a_qp(b'a\tb', quotetabs=True) == b'a=09b', 'b2a_qp quotetabs'
assert binascii.b2a_qp(b'a b', header=True) == b'a_b', 'b2a_qp header space-to-underscore'
assert binascii.b2a_qp(b'a\n', istext=False) == b'a=0A', 'b2a_qp istext=False newline escaping'
expect_error(lambda: binascii.b2a_qp('a=b'), "a bytes-like object is required, not 'str'")


# === a2b_uu / b2a_uu ===
assert binascii.b2a_uu(b'cat') == b'#8V%T\n', 'b2a_uu basic'
assert binascii.b2a_uu(b'\x00', backtick=True) == b'!````\n', 'b2a_uu backtick'
expect_error(lambda: binascii.b2a_uu(b'a' * 46), 'At most 45 bytes at once')

assert binascii.a2b_uu(b'#0V%T\n') == b'Cat', 'a2b_uu basic decode'
assert binascii.a2b_uu(b'#0V') == b'C`\x00', 'a2b_uu short input zero-pads'
assert binascii.a2b_uu('') == (b'\x00' * 32), 'a2b_uu empty string compatibility behavior'
expect_error(lambda: binascii.a2b_uu(b'!\xff\n'), 'Illegal char')
expect_error(lambda: binascii.a2b_uu(b'#0V%Txx'), 'Trailing garbage')
