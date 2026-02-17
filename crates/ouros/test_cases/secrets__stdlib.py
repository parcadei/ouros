import secrets

# === SystemRandom export ===
assert hasattr(secrets, 'SystemRandom'), 'secrets must export SystemRandom'
system_random = secrets.SystemRandom()
assert hasattr(system_random, 'randbytes'), 'secrets.SystemRandom instance must expose randbytes'
randbytes_sample = system_random.randbytes(4)
assert isinstance(randbytes_sample, bytes), 'SystemRandom.randbytes must return bytes'
assert len(randbytes_sample) == 4, 'SystemRandom.randbytes must honor requested length'

# === token_bytes ===
default_bytes = secrets.token_bytes()
assert isinstance(default_bytes, bytes), 'token_bytes() must return bytes'
assert len(default_bytes) == 32, 'token_bytes() default size must be 32'

none_bytes = secrets.token_bytes(None)
assert isinstance(none_bytes, bytes), 'token_bytes(None) must return bytes'
assert len(none_bytes) == 32, 'token_bytes(None) must use default size'

zero_bytes = secrets.token_bytes(0)
assert isinstance(zero_bytes, bytes), 'token_bytes(0) must return bytes'
assert zero_bytes == b'', 'token_bytes(0) must return empty bytes'

one_byte = secrets.token_bytes(True)
assert isinstance(one_byte, bytes), 'token_bytes(True) must return bytes'
assert len(one_byte) == 1, 'token_bytes(True) must treat bool as int(1)'

try:
    secrets.token_bytes(-1)
    assert False, 'token_bytes(-1) must raise ValueError'
except ValueError as exc:
    assert str(exc) == 'negative argument not allowed', 'token_bytes negative error message must match'

# === token_hex ===
default_hex = secrets.token_hex()
assert isinstance(default_hex, str), 'token_hex() must return str'
assert len(default_hex) == 64, 'token_hex() default length must be 64 hex chars'
assert int(default_hex, 16) >= 0, 'token_hex() output must be valid hex'

small_hex = secrets.token_hex(3)
assert isinstance(small_hex, str), 'token_hex(3) must return str'
assert len(small_hex) == 6, 'token_hex(3) must return 2*n hex chars'
assert int(small_hex, 16) >= 0, 'token_hex(3) output must be valid hex'

try:
    secrets.token_hex(-1)
    assert False, 'token_hex(-1) must raise ValueError'
except ValueError as exc:
    assert str(exc) == 'negative argument not allowed', 'token_hex negative error message must match'

# === token_urlsafe ===
default_urlsafe = secrets.token_urlsafe()
assert isinstance(default_urlsafe, str), 'token_urlsafe() must return str'
assert len(default_urlsafe) == 43, 'token_urlsafe() default length for 32 bytes must be 43'
assert default_urlsafe.isascii(), 'token_urlsafe() output must be ASCII'
assert '=' not in default_urlsafe, 'token_urlsafe() output must strip padding'

zero_urlsafe = secrets.token_urlsafe(0)
assert zero_urlsafe == '', 'token_urlsafe(0) must return empty string'

small_urlsafe = secrets.token_urlsafe(3)
assert isinstance(small_urlsafe, str), 'token_urlsafe(3) must return str'
assert len(small_urlsafe) == 4, 'token_urlsafe(3) must use 4 base64 chars'
assert small_urlsafe.isascii(), 'token_urlsafe(3) output must be ASCII'
assert '=' not in small_urlsafe, 'token_urlsafe(3) output must strip padding'

try:
    secrets.token_urlsafe(-1)
    assert False, 'token_urlsafe(-1) must raise ValueError'
except ValueError as exc:
    assert str(exc) == 'negative argument not allowed', 'token_urlsafe negative error message must match'

# === randbelow ===
for _ in range(25):
    value = secrets.randbelow(7)
    assert isinstance(value, int), 'randbelow must return int'
    assert 0 <= value < 7, 'randbelow result must be in [0, n)'

assert secrets.randbelow(True) == 0, 'randbelow(True) must return 0 for upper bound 1'

try:
    secrets.randbelow(0)
    assert False, 'randbelow(0) must raise ValueError'
except ValueError as exc:
    assert str(exc) == 'Upper bound must be positive.', 'randbelow zero error message must match'

try:
    secrets.randbelow(-3)
    assert False, 'randbelow(-3) must raise ValueError'
except ValueError as exc:
    assert str(exc) == 'Upper bound must be positive.', 'randbelow negative error message must match'

# === choice ===
for _ in range(20):
    value = secrets.choice([10, 20, 30])
    assert value in [10, 20, 30], 'choice(list) must return an item from the list'

for _ in range(20):
    value = secrets.choice((1, 2, 3, 4))
    assert value in (1, 2, 3, 4), 'choice(tuple) must return an item from the tuple'

for _ in range(20):
    value = secrets.choice('abcd')
    assert value in 'abcd', 'choice(str) must return a character from the string'

try:
    secrets.choice([])
    assert False, 'choice([]) must raise IndexError'
except IndexError as exc:
    assert str(exc) == 'Cannot choose from an empty sequence', 'choice empty error message must match'

# === compare_digest ===
assert secrets.compare_digest(b'abc', b'abc') is True, 'compare_digest must return True for equal bytes'
assert secrets.compare_digest(b'abc', b'abd') is False, 'compare_digest must return False for unequal bytes'
assert secrets.compare_digest('abc', 'abc') is True, 'compare_digest must return True for equal ASCII strings'
assert secrets.compare_digest('abc', 'abd') is False, 'compare_digest must return False for unequal ASCII strings'

try:
    secrets.compare_digest('café', 'cafe')
    assert False, 'compare_digest must reject non-ASCII strings'
except TypeError as exc:
    assert str(exc) == 'comparing strings with non-ASCII characters is not supported', (
        'compare_digest non-ASCII string error message must match'
    )

try:
    secrets.compare_digest('abc', b'abc')
    assert False, 'compare_digest(str, bytes) must raise TypeError'
except TypeError as exc:
    assert str(exc) == "a bytes-like object is required, not 'str'", (
        'compare_digest mixed str/bytes error message must match'
    )

try:
    secrets.compare_digest(b'abc', 1)
    assert False, 'compare_digest(bytes, int) must raise TypeError'
except TypeError as exc:
    assert str(exc) == "a bytes-like object is required, not 'int'", (
        'compare_digest bytes/int error message must match'
    )

try:
    secrets.compare_digest([1], [1])
    assert False, 'compare_digest(list, list) must raise TypeError'
except TypeError as exc:
    assert str(exc) == "unsupported operand types(s) or combination of types: 'list' and 'list'", (
        'compare_digest unsupported type combination error message must match'
    )
