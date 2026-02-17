import json
import sys

try:
    import io
except ModuleNotFoundError:
    io = None

# === dump/load ===
payload = {'a': 1, 'b': [2, 3]}
if sys.platform == 'ouros':
    buf = []
    json.dump(payload, buf)
    assert buf == ['{"a": 1, "b": [2, 3]}'], 'dump writes JSON string to list buffer'
    value = json.load(buf)
    assert value['a'] == 1, 'load parses dict value'
    assert value['b'] == [2, 3], 'load parses list value'
    assert buf == ['{"a": 1, "b": [2, 3]}'], 'load does not mutate buffer'
else:
    buf = io.StringIO()
    json.dump(payload, buf)
    assert buf.getvalue() == '{"a": 1, "b": [2, 3]}', 'dump writes JSON string to file-like buffer'
    buf.seek(0)
    value = json.load(buf)
    assert value['a'] == 1, 'load parses dict value'
    assert value['b'] == [2, 3], 'load parses list value'

# === JSONDecodeError ===
try:
    json.loads('{')
    assert False, 'json.loads invalid payload should raise JSONDecodeError'
except json.JSONDecodeError:
    pass

# === JSONEncoder / JSONDecoder classes ===
assert json.JSONEncoder is not None, 'JSONEncoder class should exist'
assert json.JSONDecoder is not None, 'JSONDecoder class should exist'

encoder = json.JSONEncoder(indent=2, sort_keys=True)
encoded = encoder.encode({'b': 2, 'a': 1})
assert encoded == '{\n  "a": 1,\n  "b": 2\n}', 'JSONEncoder.encode supports indent and sort_keys'
chunks = list(json.JSONEncoder().iterencode({'x': 1}))
assert ''.join(chunks) == '{"x": 1}', 'JSONEncoder.iterencode returns JSON chunks'

decoder = json.JSONDecoder()
decoded = decoder.decode('{"x": 1}')
assert decoded == {'x': 1}, 'JSONDecoder.decode parses a JSON document'

raw_obj, raw_end = decoder.raw_decode('{"x": 1} trailing')
assert raw_obj == {'x': 1}, 'JSONDecoder.raw_decode returns decoded object'
assert raw_end == 8, 'JSONDecoder.raw_decode returns end position in source string'

# === detect_encoding ===
assert json.detect_encoding(b'{}') == 'utf-8', 'detect_encoding defaults to utf-8'
assert json.detect_encoding(b'\xef\xbb\xbf{}') == 'utf-8-sig', 'detect_encoding detects UTF-8 BOM'
