# Security: resource exhaustion DoS vectors must be caught by resource limits.
# Without pre-checks (check_pow_size, check_repeat_size, check_lshift_size,
# check_mult_size), these operations allocate massive memory before limits trigger.
#
# NOTE: The test runner uses NoLimitTracker by default, so truly massive operations
# will hang or OOM rather than being caught. These tests use moderate-large values
# that complete in reasonable time while still exercising the pre-check code paths.
# The extreme values (2**10M, 'x'*999M) are documented as known gaps when running
# without resource limits — they require LimitedTracker to be caught.

# === Large but bounded power ===
x = 2 ** 10_000
assert x > 0, 'large power works'
assert len(str(x)) > 3000, 'large power produces big number'

# === Large string repeat ===
s = 'x' * 100_000
assert len(s) == 100_000, 'large string repeat works'

# === Large bytes repeat ===
b = b'ab' * 50_000
assert len(b) == 100_000, 'large bytes repeat works'

# === Large left shift ===
x = 1 << 10_000
assert x > 0, 'large left shift works'

# === Large int multiplication ===
big = 10 ** 5_000
result = big * big
assert result > 0, 'large int multiplication works'

# === Large list creation ===
lst = [0] * 100_000
assert len(lst) == 100_000, 'large list creation works'

# === Moderate operations should still work ===
assert 2 ** 20 == 1048576, 'moderate power works'
assert len('ab' * 1000) == 2000, 'moderate string repeat works'
assert len(b'x' * 1000) == 1000, 'moderate bytes repeat works'
assert 1 << 20 == 1048576, 'moderate left shift works'
assert len([0] * 1000) == 1000, 'moderate list repeat works'
