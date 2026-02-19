import math

# === roots and exponentials ===
assert abs(math.cbrt(27.0) - 3.0) < 1e-15, 'cbrt of 27'
assert abs(math.cbrt(-8.0) - (-2.0)) < 1e-15, 'cbrt of negative'
assert math.exp2(3) == 8.0, 'exp2 of 3'
assert abs(math.expm1(1e-6) - 1.0000005000001665e-06) < 1e-20, 'expm1 small value'

# === fused multiply-add ===
assert math.fma(2.0, 3.0, 4.0) == 10.0, 'fma basic'

# === integer sqrt ===
assert math.isqrt(10) == 3, 'isqrt of 10'
assert math.isqrt(0) == 0, 'isqrt of zero'

# === combinatorics and products ===
assert math.comb(5, 2) == 10, 'comb basic'
assert math.perm(5, 2) == 20, 'perm with k'
assert math.perm(5) == 120, 'perm with omitted k'
assert math.prod([2, 3, 4]) == 24, 'prod of integers'
assert math.prod((2, 3, 4), start=5) == 120, 'prod supports start keyword'

# === log1p ===
assert abs(math.log1p(0.5) - 0.4054651081081644) < 1e-15, 'log1p of 0.5'

# === gcd and lcm varargs ===
assert math.gcd() == 0, 'gcd with no args returns 0'
assert math.gcd(48) == 48, 'gcd one arg'
assert math.gcd(48, 18, 30) == 6, 'gcd multiple args'
assert math.lcm() == 1, 'lcm with no args returns 1'
assert math.lcm(6) == 6, 'lcm one arg'
assert math.lcm(4, 6, 8) == 24, 'lcm multiple args'

# === nextafter ===
assert math.nextafter(1.0, 2.0) == 1.0000000000000002, 'nextafter toward larger'
assert math.nextafter(1.0, 0.0) == 0.9999999999999999, 'nextafter toward smaller'
assert math.nextafter(0.0, 1.0) == 5e-324, 'nextafter from zero toward positive'
assert math.nextafter(0.0, -1.0) == -5e-324, 'nextafter from zero toward negative'

# === sumprod ===
assert math.sumprod([1, 2, 3], [4, 5, 6]) == 32, 'sumprod ints'
assert math.sumprod([1.0, 2.0], [3.0, 4.0]) == 11.0, 'sumprod floats'

# === ceil_div and floor_div ===
if hasattr(math, 'ceil_div') and hasattr(math, 'floor_div'):
    assert math.ceil_div(5, 2) == 3, 'ceil_div positive'
    assert math.ceil_div(-5, 2) == -2, 'ceil_div negative numerator'
    assert math.ceil_div(5, -2) == -2, 'ceil_div negative denominator'
    assert math.floor_div(5, 2) == 2, 'floor_div positive'
    assert math.floor_div(-5, 2) == -3, 'floor_div negative numerator'
    assert math.floor_div(5, -2) == -3, 'floor_div negative denominator'
    try:
        math.ceil_div(1, 0)
        assert False, 'ceil_div division by zero should fail'
    except ZeroDivisionError as e:
        assert str(e) == 'division by zero', f'unexpected ceil_div zero message: {e}'

# === sum_of_squares, dot, cross ===
if hasattr(math, 'sum_of_squares'):
    assert math.sum_of_squares([1, 2, 3]) == 14, 'sum_of_squares ints'
    assert math.sum_of_squares([1.0, 2.0, 3.0]) == 14.0, 'sum_of_squares floats'
if hasattr(math, 'dot'):
    assert math.dot([1, 2, 3], [4, 5, 6]) == 32.0, 'dot product'
if hasattr(math, 'cross'):
    assert math.cross([1, 2, 3], [4, 5, 6]) == (-3.0, 6.0, -3.0), 'cross product'

# === ulp ===
assert math.ulp(1.0) == 2.220446049250313e-16, 'ulp of 1.0'
assert math.ulp(0.0) == 5e-324, 'ulp of zero'
assert math.ulp(-2.5) == 4.440892098500626e-16, 'ulp of negative'
