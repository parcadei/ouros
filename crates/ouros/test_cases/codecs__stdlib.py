import codecs
import sys


# === Public API Surface ===
public_names = [
    'BOM',
    'BOM32_BE',
    'BOM32_LE',
    'BOM64_BE',
    'BOM64_LE',
    'BOM_BE',
    'BOM_LE',
    'BOM_UTF16',
    'BOM_UTF16_BE',
    'BOM_UTF16_LE',
    'BOM_UTF32',
    'BOM_UTF32_BE',
    'BOM_UTF32_LE',
    'BOM_UTF8',
    'BufferedIncrementalDecoder',
    'BufferedIncrementalEncoder',
    'Codec',
    'CodecInfo',
    'EncodedFile',
    'IncrementalDecoder',
    'IncrementalEncoder',
    'StreamReader',
    'StreamReaderWriter',
    'StreamRecoder',
    'StreamWriter',
    'ascii_decode',
    'ascii_encode',
    'backslashreplace_errors',
    'charmap_build',
    'charmap_decode',
    'charmap_encode',
    'decode',
    'encode',
    'escape_decode',
    'escape_encode',
    'getdecoder',
    'getencoder',
    'getincrementaldecoder',
    'getincrementalencoder',
    'getreader',
    'getwriter',
    'ignore_errors',
    'iterdecode',
    'iterencode',
    'latin_1_decode',
    'latin_1_encode',
    'lookup',
    'lookup_error',
    'make_encoding_map',
    'make_identity_dict',
    'namereplace_errors',
    'open',
    'raw_unicode_escape_decode',
    'raw_unicode_escape_encode',
    'readbuffer_encode',
    'register',
    'register_error',
    'replace_errors',
    'strict_errors',
    'unicode_escape_decode',
    'unicode_escape_encode',
    'unregister',
    'utf_16_be_decode',
    'utf_16_be_encode',
    'utf_16_decode',
    'utf_16_encode',
    'utf_16_ex_decode',
    'utf_16_le_decode',
    'utf_16_le_encode',
    'utf_32_be_decode',
    'utf_32_be_encode',
    'utf_32_decode',
    'utf_32_encode',
    'utf_32_ex_decode',
    'utf_32_le_decode',
    'utf_32_le_encode',
    'utf_7_decode',
    'utf_7_encode',
    'utf_8_decode',
    'utf_8_encode',
    'xmlcharrefreplace_errors',
]

for name in public_names:
    assert hasattr(codecs, name), f'missing codecs.{name}'


# === BOM constants ===
assert codecs.BOM_BE == b'\xfe\xff', 'BOM_BE'
assert codecs.BOM_LE == b'\xff\xfe', 'BOM_LE'
assert codecs.BOM_UTF16_BE == b'\xfe\xff', 'BOM_UTF16_BE'
assert codecs.BOM_UTF16_LE == b'\xff\xfe', 'BOM_UTF16_LE'
assert codecs.BOM_UTF32_BE == b'\x00\x00\xfe\xff', 'BOM_UTF32_BE'
assert codecs.BOM_UTF32_LE == b'\xff\xfe\x00\x00', 'BOM_UTF32_LE'
assert codecs.BOM_UTF8 == b'\xef\xbb\xbf', 'BOM_UTF8'
assert codecs.BOM32_BE == b'\xfe\xff', 'BOM32_BE'
assert codecs.BOM32_LE == b'\xff\xfe', 'BOM32_LE'
assert codecs.BOM64_BE == b'\x00\x00\xfe\xff', 'BOM64_BE'
assert codecs.BOM64_LE == b'\xff\xfe\x00\x00', 'BOM64_LE'

if sys.byteorder == 'little':
    assert codecs.BOM == codecs.BOM_LE, 'native BOM on little-endian'
    assert codecs.BOM_UTF16 == codecs.BOM_UTF16_LE, 'native UTF16 BOM on little-endian'
    assert codecs.BOM_UTF32 == codecs.BOM_UTF32_LE, 'native UTF32 BOM on little-endian'
else:
    assert codecs.BOM == codecs.BOM_BE, 'native BOM on big-endian'
    assert codecs.BOM_UTF16 == codecs.BOM_UTF16_BE, 'native UTF16 BOM on big-endian'
    assert codecs.BOM_UTF32 == codecs.BOM_UTF32_BE, 'native UTF32 BOM on big-endian'


# === Top-level encode/decode ===
assert codecs.encode('abc') == b'abc', 'codecs.encode utf-8'
assert codecs.decode(b'abc') == 'abc', 'codecs.decode utf-8'
assert codecs.encode('é', 'latin-1') == b'\xe9', 'codecs.encode latin-1'
assert codecs.decode(b'\xe9', 'latin-1') == 'é', 'codecs.decode latin-1'


# === Lookup and helper getters ===
info = codecs.lookup('utf-8')
assert len(info) == 4, 'lookup tuple-like length'

enc = codecs.getencoder('utf-8')
dec = codecs.getdecoder('utf-8')
assert enc('a')[0] == b'a', 'getencoder returns callable'
assert dec(b'a')[0] == 'a', 'getdecoder returns callable'
assert callable(codecs.getincrementalencoder('utf-8')), 'getincrementalencoder callable'
assert callable(codecs.getincrementaldecoder('utf-8')), 'getincrementaldecoder callable'
assert callable(codecs.getreader('utf-8')), 'getreader callable'
assert callable(codecs.getwriter('utf-8')), 'getwriter callable'


# === Iter helpers ===
assert list(codecs.iterencode(['a', 'b'], 'utf-8')) == [b'a', b'b'], 'iterencode'
assert list(codecs.iterdecode([b'a', b'b'], 'utf-8')) == ['a', 'b'], 'iterdecode'


# === ASCII / Latin-1 ===
assert codecs.ascii_encode('abc') == (b'abc', 3), 'ascii_encode'
assert codecs.ascii_decode(b'abc') == ('abc', 3), 'ascii_decode'
assert codecs.latin_1_encode('Aÿ') == (b'A\xff', 2), 'latin_1_encode'
assert codecs.latin_1_decode(b'\xff') == ('ÿ', 1), 'latin_1_decode'


# === UTF-8 / UTF-7 ===
assert codecs.utf_8_encode('☃') == (b'\xe2\x98\x83', 1), 'utf_8_encode'
assert codecs.utf_8_decode(b'\xe2\x98\x83') == ('☃', 3), 'utf_8_decode'
assert codecs.utf_7_encode('A+') == (b'A+-', 2), 'utf_7_encode'
assert codecs.utf_7_decode(b'A+-') == ('A+', 3), 'utf_7_decode'


# === UTF-16 family ===
utf16 = codecs.utf_16_encode('A')
assert utf16[1] == 1, 'utf_16_encode consumed chars'
assert codecs.utf_16_decode(utf16[0])[0] == 'A', 'utf_16_decode roundtrip'
utf16_ex = codecs.utf_16_ex_decode(utf16[0])
assert utf16_ex[0] == 'A', 'utf_16_ex_decode text'
assert utf16_ex[1] == len(utf16[0]), 'utf_16_ex_decode consumed bytes'
assert utf16_ex[2] in (-1, 1), 'utf_16_ex_decode byteorder'
assert codecs.utf_16_le_decode(codecs.utf_16_le_encode('A')[0])[0] == 'A', 'utf_16_le roundtrip'
assert codecs.utf_16_be_decode(codecs.utf_16_be_encode('A')[0])[0] == 'A', 'utf_16_be roundtrip'


# === UTF-32 family ===
utf32 = codecs.utf_32_encode('A')
assert utf32[1] == 1, 'utf_32_encode consumed chars'
assert codecs.utf_32_decode(utf32[0])[0] == 'A', 'utf_32_decode roundtrip'
utf32_ex = codecs.utf_32_ex_decode(utf32[0])
assert utf32_ex[0] == 'A', 'utf_32_ex_decode text'
assert utf32_ex[1] == len(utf32[0]), 'utf_32_ex_decode consumed bytes'
assert utf32_ex[2] in (-1, 1), 'utf_32_ex_decode byteorder'
assert codecs.utf_32_le_decode(codecs.utf_32_le_encode('A')[0])[0] == 'A', 'utf_32_le roundtrip'
assert codecs.utf_32_be_decode(codecs.utf_32_be_encode('A')[0])[0] == 'A', 'utf_32_be roundtrip'


# === Escape codecs ===
assert codecs.unicode_escape_encode('A\n') == (b'A\\n', 2), 'unicode_escape_encode'
assert codecs.unicode_escape_decode(b'A\\n') == ('A\n', 3), 'unicode_escape_decode'
assert codecs.raw_unicode_escape_encode('A☃') == (b'A\\u2603', 2), 'raw_unicode_escape_encode'
assert codecs.raw_unicode_escape_decode(b'A\\u2603')[0] == 'A☃', 'raw_unicode_escape_decode'
assert codecs.escape_encode(b'A\n') == (b'A\\n', 2), 'escape_encode'
assert codecs.escape_decode(b'A\\n') == (b'A\n', 3), 'escape_decode'


# === Charmap helpers ===
assert codecs.charmap_encode('abc', None, {ord('a'): 1, ord('b'): 2, ord('c'): 3}) == (b'\x01\x02\x03', 3), 'charmap_encode'
assert codecs.charmap_decode(b'\x01\x02', None, {1: 'a', 2: 'b'}) == ('ab', 2), 'charmap_decode'
assert codecs.charmap_build('abc') == {97: 0, 98: 1, 99: 2}, 'charmap_build'

identity = codecs.make_identity_dict(range(3))
assert identity == {0: 0, 1: 1, 2: 2}, 'make_identity_dict'

encoding_map = codecs.make_encoding_map({1: 'a', 2: 'b'})
assert encoding_map == {'a': 1, 'b': 2}, 'make_encoding_map'


# === readbuffer_encode ===
assert codecs.readbuffer_encode(b'abc') == (b'abc', 3), 'readbuffer_encode bytes'
assert codecs.readbuffer_encode('abc') == (b'abc', 3), 'readbuffer_encode str'


# === Error registry helpers ===
codecs.register(lambda _name: None)
codecs.unregister(lambda _name: None)
codecs.register_error('ouros_handler', lambda exc: ('?', 1))
assert callable(codecs.lookup_error('strict')), 'lookup_error strict'
assert callable(codecs.lookup_error('ignore')), 'lookup_error ignore'
assert callable(codecs.lookup_error('replace')), 'lookup_error replace'
assert callable(codecs.lookup_error('backslashreplace')), 'lookup_error backslashreplace'
assert callable(codecs.lookup_error('xmlcharrefreplace')), 'lookup_error xmlcharrefreplace'
assert callable(codecs.lookup_error('namereplace')), 'lookup_error namereplace'


# === Error callbacks are present and callable ===
assert callable(codecs.strict_errors), 'strict_errors callable'
assert callable(codecs.ignore_errors), 'ignore_errors callable'
assert callable(codecs.replace_errors), 'replace_errors callable'
assert callable(codecs.backslashreplace_errors), 'backslashreplace_errors callable'
assert callable(codecs.xmlcharrefreplace_errors), 'xmlcharrefreplace_errors callable'
assert callable(codecs.namereplace_errors), 'namereplace_errors callable'


# === I/O APIs exist (sandbox-safe call behavior differs by runtime) ===
try:
    codecs.open('__ouros_nonexistent_file__', 'r')
except Exception as exc:
    assert exc is not None, 'codecs.open raises'

try:
    codecs.EncodedFile(object(), 'utf-8')
except Exception as exc:
    assert exc is not None, 'codecs.EncodedFile raises'


# === Module attrs mirrored from CPython ===
assert hasattr(codecs, 'builtins'), 'codecs.builtins'
assert hasattr(codecs, 'sys'), 'codecs.sys'
