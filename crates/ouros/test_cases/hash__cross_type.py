# Cross-type hash consistency (CPython invariant)
# CPython guarantees: if a == b, then hash(a) == hash(b)
# This must hold across int, float, and bool types.

# === Bool/Int/Float zero equivalence ===
assert hash(0) == hash(0.0), f'hash(0)={hash(0)} != hash(0.0)={hash(0.0)}'
assert hash(0) == hash(False), f'hash(0)={hash(0)} != hash(False)={hash(False)}'
assert hash(0.0) == hash(False), f'hash(0.0)={hash(0.0)} != hash(False)={hash(False)}'

# === Bool/Int/Float one equivalence ===
assert hash(1) == hash(1.0), f'hash(1)={hash(1)} != hash(1.0)={hash(1.0)}'
assert hash(1) == hash(True), f'hash(1)={hash(1)} != hash(True)={hash(True)}'
assert hash(1.0) == hash(True), f'hash(1.0)={hash(1.0)} != hash(True)={hash(True)}'

# === Negative values ===
assert hash(-1) == hash(-1.0), f'hash(-1)={hash(-1)} != hash(-1.0)={hash(-1.0)}'
assert hash(-42) == hash(-42.0), f'hash(-42)={hash(-42)} != hash(-42.0)={hash(-42.0)}'

# === Positive integer/float equivalence ===
assert hash(42) == hash(42.0), f'hash(42)={hash(42)} != hash(42.0)={hash(42.0)}'
assert hash(1000000) == hash(1000000.0), f'hash(1000000)={hash(1000000)} != hash(1000000.0)={hash(1000000.0)}'
assert hash(100) == hash(100.0), f'hash(100)={hash(100)} != hash(100.0)={hash(100.0)}'

# === hash(-1) must be remapped to -2 (CPython convention) ===
h = hash(-1)
assert h == -2, f'hash(-1) should be -2, got {h}'

# === Dict lookup with mixed keys (the practical consequence) ===
d = {0: 'zero'}
assert d[0.0] == 'zero', f'd[0.0] should find 0 key'
assert d[False] == 'zero', f'd[False] should find 0 key'

d = {1: 'one'}
assert d[1.0] == 'one', f'd[1.0] should find 1 key'
assert d[True] == 'one', f'd[True] should find 1 key'

# === Dict with float key, lookup with int ===
d2 = {1.0: 'float_one'}
assert d2[1] == 'float_one', f'd2[1] should find 1.0 key'
assert d2[True] == 'float_one', f'd2[True] should find 1.0 key'

# === Dict with bool key, lookup with int/float ===
d3 = {True: 'bool_true'}
assert d3[1] == 'bool_true', f'd3[1] should find True key'
assert d3[1.0] == 'bool_true', f'd3[1.0] should find True key'

print('all cross-type hash tests passed')
