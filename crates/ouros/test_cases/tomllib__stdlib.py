import tomllib
from io import BytesIO, StringIO


# === Public API ===
assert hasattr(tomllib, 'TOMLDecodeError'), 'tomllib exports TOMLDecodeError'
assert hasattr(tomllib, 'load'), 'tomllib exports load'
assert hasattr(tomllib, 'loads'), 'tomllib exports loads'
assert tomllib.TOMLDecodeError is not ValueError, 'TOMLDecodeError must be a distinct type'
assert issubclass(tomllib.TOMLDecodeError, ValueError), 'TOMLDecodeError must inherit ValueError'


# === loads: basic scalar and container decoding ===
doc = tomllib.loads(
    """
title = 'TOML Example'
enabled = true
count = 42
ratio = 1.5
items = [1, 2, 3]

[owner]
name = 'Tom'
"""
)
assert doc['title'] == 'TOML Example', 'loads parses strings'
assert doc['enabled'] is True, 'loads parses booleans'
assert doc['count'] == 42, 'loads parses integers'
assert doc['ratio'] == 1.5, 'loads parses floats'
assert doc['items'] == [1, 2, 3], 'loads parses arrays'
assert doc['owner']['name'] == 'Tom', 'loads parses tables'


# === loads: array of tables ===
doc = tomllib.loads(
    """
[[products]]
name = 'A'

[[products]]
name = 'B'
"""
)
assert doc['products'][0]['name'] == 'A', 'loads parses first array-of-tables element'
assert doc['products'][1]['name'] == 'B', 'loads parses second array-of-tables element'


# === loads: datetime/date/time ===
dt = tomllib.loads('x = 1979-05-27T07:32:00Z')['x']
assert dt.year == 1979 and dt.month == 5 and dt.day == 27, 'loads parses datetime date components'
assert dt.hour == 7 and dt.minute == 32 and dt.second == 0, 'loads parses datetime time components'

d = tomllib.loads('x = 1979-05-27')['x']
assert d.year == 1979 and d.month == 5 and d.day == 27, 'loads parses local date'

t = tomllib.loads('x = 07:32:00')['x']
assert t.hour == 7 and t.minute == 32 and t.second == 0, 'loads parses local time'


# === loads: parse_float ===
value = tomllib.loads('x = 1.5', parse_float=tuple)['x']
assert value == ('1', '.', '5'), 'parse_float receives TOML float lexeme'

value = tomllib.loads('x = 1_2.3_4', parse_float=tuple)['x']
assert value == ('1', '_', '2', '.', '3', '_', '4'), 'parse_float preserves underscores'

try:
    tomllib.loads('x = 1.5', parse_float=list)
    assert False, 'parse_float=list should fail because list return is disallowed'
except ValueError as exc:
    assert str(exc) == 'parse_float must not return dicts or lists', 'parse_float list-return error message'

try:
    tomllib.loads('x = 1.5', parse_float=1)
    assert False, 'parse_float=int should fail when a float token is present'
except TypeError as exc:
    assert str(exc) != '', 'parse_float non-callable error message should not be empty'

assert tomllib.loads('x = 1', parse_float=1) == {'x': 1}, 'parse_float is only used for float tokens'


# === loads: argument validation ===
try:
    tomllib.loads()
    assert False, 'loads() without args should fail'
except TypeError as exc:
    assert str(exc) == "loads() missing 1 required positional argument: 's'", 'loads missing argument message'

try:
    tomllib.loads('a=1', 'b=2')
    assert False, 'loads() with too many positional args should fail'
except TypeError as exc:
    assert str(exc) == 'loads() takes 1 positional argument but 2 were given', 'loads too-many-positional message'

try:
    tomllib.loads(s='a=1')
    assert False, 'loads() positional-only arg passed as keyword should fail'
except TypeError as exc:
    assert str(exc) == "loads() got some positional-only arguments passed as keyword arguments: 's'"

try:
    tomllib.loads('a=1', bad=True)
    assert False, 'loads() unexpected keyword should fail'
except TypeError as exc:
    assert str(exc) == "loads() got an unexpected keyword argument 'bad'", 'loads unexpected keyword message'

try:
    tomllib.loads(b'a=1')
    assert False, 'loads() with bytes should fail'
except TypeError as exc:
    assert str(exc) == "Expected str object, not 'bytes'", 'loads type requirement message'


# === loads: parse error type ===
try:
    tomllib.loads('a =')
    assert False, 'invalid TOML should raise TOMLDecodeError'
except tomllib.TOMLDecodeError as exc:
    assert str(exc) != '', 'TOMLDecodeError message should not be empty'


# === load: file-like binary input ===
class NoRead:
    pass


assert tomllib.load(BytesIO(b'a=1')) == {'a': 1}, 'load parses bytes from file-like read()'
assert tomllib.load(BytesIO(b'x=1.5'), parse_float=tuple) == {
    'x': ('1', '.', '5')
}, 'load applies parse_float callback'

for value in [StringIO('a=1')]:
    try:
        tomllib.load(value)
        assert False, 'load() requires binary mode read result'
    except TypeError as exc:
        assert str(exc) == "File must be opened in binary mode, e.g. use `open('foo.toml', 'rb')`"

try:
    tomllib.load(NoRead())
    assert False, 'load() requires read attribute'
except AttributeError as exc:
    assert str(exc) != '', 'missing read() must raise non-empty AttributeError message'


# === load: argument validation ===
try:
    tomllib.load()
    assert False, 'load() without args should fail'
except TypeError as exc:
    assert str(exc) == "load() missing 1 required positional argument: 'fp'"

try:
    tomllib.load(BytesIO(b'a=1'), BytesIO(b'b=2'))
    assert False, 'load() with too many positional args should fail'
except TypeError as exc:
    assert str(exc) == 'load() takes 1 positional argument but 2 were given'

try:
    tomllib.load(fp=BytesIO(b'a=1'))
    assert False, 'load() positional-only arg passed as keyword should fail'
except TypeError as exc:
    assert str(exc) == "load() got some positional-only arguments passed as keyword arguments: 'fp'"

try:
    tomllib.load(BytesIO(b'a=1'), bad=True)
    assert False, 'load() unexpected keyword should fail'
except TypeError as exc:
    assert str(exc) == "load() got an unexpected keyword argument 'bad'"
