# === Module Import ===
import zlib

# === Public API Surface ===
expected_names = {
    'DEFLATED',
    'DEF_BUF_SIZE',
    'DEF_MEM_LEVEL',
    'MAX_WBITS',
    'ZLIB_RUNTIME_VERSION',
    'ZLIB_VERSION',
    'Z_BEST_COMPRESSION',
    'Z_BEST_SPEED',
    'Z_BLOCK',
    'Z_DEFAULT_COMPRESSION',
    'Z_DEFAULT_STRATEGY',
    'Z_FILTERED',
    'Z_FINISH',
    'Z_FIXED',
    'Z_FULL_FLUSH',
    'Z_HUFFMAN_ONLY',
    'Z_NO_COMPRESSION',
    'Z_NO_FLUSH',
    'Z_PARTIAL_FLUSH',
    'Z_RLE',
    'Z_SYNC_FLUSH',
    'Z_TREES',
    'adler32',
    'compress',
    'compressobj',
    'crc32',
    'decompress',
    'decompressobj',
    'error',
}
module_names = set(dir(zlib))
for name in expected_names:
    assert name in module_names, f'missing zlib public name: {name}'

# === Constants ===
assert zlib.DEFLATED == 8, 'DEFLATED constant'
assert zlib.DEF_BUF_SIZE == 16384, 'DEF_BUF_SIZE constant'
assert zlib.DEF_MEM_LEVEL == 8, 'DEF_MEM_LEVEL constant'
assert zlib.MAX_WBITS == 15, 'MAX_WBITS constant'
assert zlib.Z_NO_COMPRESSION == 0, 'Z_NO_COMPRESSION constant'
assert zlib.Z_BEST_SPEED == 1, 'Z_BEST_SPEED constant'
assert zlib.Z_BEST_COMPRESSION == 9, 'Z_BEST_COMPRESSION constant'
assert zlib.Z_DEFAULT_COMPRESSION == -1, 'Z_DEFAULT_COMPRESSION constant'
assert zlib.Z_DEFAULT_STRATEGY == 0, 'Z_DEFAULT_STRATEGY constant'
assert zlib.Z_FILTERED == 1, 'Z_FILTERED constant'
assert zlib.Z_HUFFMAN_ONLY == 2, 'Z_HUFFMAN_ONLY constant'
assert zlib.Z_RLE == 3, 'Z_RLE constant'
assert zlib.Z_FIXED == 4, 'Z_FIXED constant'
assert zlib.Z_NO_FLUSH == 0, 'Z_NO_FLUSH constant'
assert zlib.Z_PARTIAL_FLUSH == 1, 'Z_PARTIAL_FLUSH constant'
assert zlib.Z_SYNC_FLUSH == 2, 'Z_SYNC_FLUSH constant'
assert zlib.Z_FULL_FLUSH == 3, 'Z_FULL_FLUSH constant'
assert zlib.Z_FINISH == 4, 'Z_FINISH constant'
assert zlib.Z_BLOCK == 5, 'Z_BLOCK constant'
assert zlib.Z_TREES == 6, 'Z_TREES constant'
assert isinstance(zlib.ZLIB_VERSION, str) and zlib.ZLIB_VERSION != '', 'ZLIB_VERSION should be non-empty string'
assert isinstance(zlib.ZLIB_RUNTIME_VERSION, str) and zlib.ZLIB_RUNTIME_VERSION != '', 'ZLIB_RUNTIME_VERSION should be non-empty string'

# === crc32 / adler32 ===
assert zlib.crc32(b'') == 0, 'crc32 empty'
assert zlib.crc32(b'hello') == 907060870, 'crc32 hello'
assert zlib.crc32(b'hello', 1) == 191926070, 'crc32 with start value'
assert zlib.crc32(b'hello', -1) == 265137764, 'crc32 negative start value wraps mod 2**32'

assert zlib.adler32(b'') == 1, 'adler32 empty'
assert zlib.adler32(b'hello') == 103547413, 'adler32 hello'
assert zlib.adler32(b'hello', 7) == 105513499, 'adler32 with start value'
assert zlib.adler32(b'hello', -1) == 108724770, 'adler32 negative start value wraps mod 2**32'

# === compress / decompress round-trips ===
source = (b'Ouros zlib parity test ' * 20) + b'end'
compressed_default = zlib.compress(source)
assert isinstance(compressed_default, bytes), 'compress returns bytes'
assert zlib.decompress(compressed_default) == source, 'default round-trip'

compressed_raw = zlib.compress(source, wbits=-15)
assert zlib.decompress(compressed_raw, wbits=-15) == source, 'raw deflate round-trip'

compressed_gzip = zlib.compress(source, wbits=31)
assert zlib.decompress(compressed_gzip, wbits=31) == source, 'gzip round-trip'
assert zlib.decompress(compressed_gzip + b'trailing', wbits=47) == source, 'auto gzip/zlib with trailing bytes'

# === compressobj ===
cobj = zlib.compressobj()
part1 = cobj.compress(source[:25])
part2 = cobj.compress(source[25:])
part3 = cobj.flush()
joined = part1 + part2 + part3
assert zlib.decompress(joined) == source, 'compressobj stream round-trip'

cobj_copy_src = zlib.compressobj()
_ = cobj_copy_src.compress(b'abc')
cobj_copy = cobj_copy_src.copy()
out_a = cobj_copy_src.compress(b'def') + cobj_copy_src.flush()
out_b = cobj_copy.compress(b'def') + cobj_copy.flush()
assert out_a == out_b, 'compressobj.copy duplicates state'

finished_obj = zlib.compressobj()
_ = finished_obj.compress(b'data')
_ = finished_obj.flush(zlib.Z_FINISH)
try:
    finished_obj.copy()
    assert False, 'compressobj.copy after finish should fail'
except Exception as exc:
    assert exc is not None, 'compressobj.copy after finish raised'
    assert str(exc) == 'Inconsistent stream state', 'compressobj.copy after finish message'

# === decompressobj ===
dobj = zlib.decompressobj()
out1 = dobj.decompress(compressed_default[:11])
out2 = dobj.decompress(compressed_default[11:])
out3 = dobj.flush()
assert out1 + out2 + out3 == source, 'decompressobj stream round-trip'
assert dobj.eof is True, 'decompressobj eof should be true after end of stream'
assert dobj.unconsumed_tail == b'', 'decompressobj unconsumed_tail empty after complete consume'
assert dobj.unused_data == b'', 'decompressobj unused_data empty for exact input'

dobj_trailing = zlib.decompressobj()
out_trailing = dobj_trailing.decompress(compressed_default + b'xyz')
assert out_trailing == source, 'decompressobj output with trailing bytes'
assert dobj_trailing.eof is True, 'decompressobj eof true with trailing bytes'
assert dobj_trailing.unused_data == b'xyz', 'decompressobj unused_data captures trailing bytes'
assert dobj_trailing.unconsumed_tail == b'', 'decompressobj unconsumed_tail remains empty here'

dobj_copy_src = zlib.decompressobj()
_ = dobj_copy_src.decompress(compressed_default[:7])
dobj_copy = dobj_copy_src.copy()
out_a = dobj_copy_src.decompress(compressed_default[7:]) + dobj_copy_src.flush()
out_b = dobj_copy.decompress(compressed_default[7:]) + dobj_copy.flush()
assert out_a == out_b, 'decompressobj.copy duplicates state'

try:
    dobj.copy()
    assert False, 'decompressobj.copy after EOF should fail'
except Exception as exc:
    assert exc is not None, 'decompressobj.copy after EOF raised'
    assert str(exc) == 'Inconsistent stream state', 'decompressobj.copy after EOF message'

# === Error handling and coercion ===
try:
    zlib.compress('text')
    assert False, 'compress should reject str'
except TypeError as exc:
    assert str(exc) == "a bytes-like object is required, not 'str'", 'compress str type error message'

try:
    zlib.decompress('text')
    assert False, 'decompress should reject str'
except TypeError as exc:
    assert str(exc) == "a bytes-like object is required, not 'str'", 'decompress str type error message'

try:
    zlib.compressobj(level=10)
    assert False, 'compressobj invalid level should fail'
except Exception as exc:
    assert str(exc) == 'Invalid initialization option', 'compressobj invalid level message'

try:
    zlib.decompressobj(wbits=100)
    assert False, 'decompressobj invalid wbits should fail'
except Exception as exc:
    assert str(exc) == 'Invalid initialization option', 'decompressobj invalid wbits message'

try:
    zlib.decompress(compressed_default[:-3])
    assert False, 'decompress truncated should fail'
except Exception as exc:
    assert str(exc) == 'Error -5 while decompressing data: incomplete or truncated stream', 'decompress truncated message'

try:
    zlib.decompress(b'not-zlib-data')
    assert False, 'decompress invalid data should fail'
except Exception as exc:
    assert str(exc) != '', 'decompress invalid data should include an error message'

try:
    zlib.decompressobj().decompress(b'', -1)
    assert False, 'decompressobj.decompress negative max_length should fail'
except Exception as exc:
    assert str(exc) == 'max_length must be non-negative', 'decompressobj.decompress negative max_length message'

try:
    zlib.decompressobj().flush(0)
    assert False, 'decompressobj.flush length <= 0 should fail'
except Exception as exc:
    assert str(exc) == 'length must be greater than zero', 'decompressobj.flush invalid length message'

try:
    zlib.decompress(compressed_default, bufsize=-1)
    assert False, 'decompress negative bufsize should fail'
except Exception as exc:
    assert str(exc) == 'bufsize must be non-negative', 'decompress negative bufsize message'

try:
    zlib.decompressobj(zdict='x')
    assert False, 'decompressobj zdict str should fail'
except TypeError as exc:
    assert str(exc) == 'zdict argument must support the buffer protocol', 'decompressobj zdict type error message'
