import math

# === Constants ===
try:
    print('const_pi', math.pi)
    print('const_e', math.e)
    print('const_tau', math.tau)
    print('const_inf', math.inf)
    print('const_nan', math.nan)
    print('const_inf_is_inf', math.isinf(math.inf))
    print('const_nan_is_nan', math.isnan(math.nan))
except Exception as e:
    print('SKIP_Constants', type(e).__name__, e)

# === Number-theoretic functions ===
try:
    # comb(n, k) - binomial coefficient
    print('comb_5_2', math.comb(5, 2))
    print('comb_10_0', math.comb(10, 0))
    print('comb_10_10', math.comb(10, 10))
    print('comb_0_0', math.comb(0, 0))
    print('comb_5_6', math.comb(5, 6))  # k > n returns 0

    # factorial(n)
    print('factorial_0', math.factorial(0))
    print('factorial_1', math.factorial(1))
    print('factorial_5', math.factorial(5))
    print('factorial_10', math.factorial(10))

    # gcd(*integers)
    print('gcd_48_18', math.gcd(48, 18))
    print('gcd_0_0', math.gcd(0, 0))
    print('gcd_0_5', math.gcd(0, 5))
    print('gcd_neg', math.gcd(-48, 18))
    print('gcd_3args', math.gcd(12, 18, 24))
    print('gcd_no_args', math.gcd())

    # isqrt(n) - integer square root
    print('isqrt_0', math.isqrt(0))
    print('isqrt_1', math.isqrt(1))
    print('isqrt_4', math.isqrt(4))
    print('isqrt_10', math.isqrt(10))
    print('isqrt_100', math.isqrt(100))

    # lcm(*integers)
    print('lcm_4_6', math.lcm(4, 6))
    print('lcm_0_5', math.lcm(0, 5))
    print('lcm_0_0', math.lcm(0, 0))
    print('lcm_3args', math.lcm(4, 6, 8))
    print('lcm_no_args', math.lcm())

    # perm(n, k) - permutations
    print('perm_5_2', math.perm(5, 2))
    print('perm_5_none', math.perm(5))
    print('perm_5_0', math.perm(5, 0))
    print('perm_5_6', math.perm(5, 6))  # k > n returns 0
except Exception as e:
    print('SKIP_Number-theoretic functions', type(e).__name__, e)

# === Floating point arithmetic ===
try:
    # ceil(x)
    print('ceil_2_3', math.ceil(2.3))
    print('ceil_2_7', math.ceil(2.7))
    print('ceil_neg', math.ceil(-2.3))
    print('ceil_5', math.ceil(5.0))
    print('ceil_int', math.ceil(5))

    # fabs(x)
    print('fabs_3_5', math.fabs(3.5))
    print('fabs_neg', math.fabs(-3.5))
    print('fabs_0', math.fabs(0.0))

    # floor(x)
    print('floor_2_3', math.floor(2.3))
    print('floor_2_7', math.floor(2.7))
    print('floor_neg', math.floor(-2.3))
    print('floor_5', math.floor(5.0))
    print('floor_int', math.floor(5))

    # fma(x, y, z) - fused multiply-add
    print('fma_basic', math.fma(2.0, 3.0, 4.0))
    print('fma_neg', math.fma(-2.0, 3.0, 4.0))
    print('fma_zero', math.fma(0.0, 1.0, 5.0))

    # fmod(x, y) - floating point remainder
    print('fmod_10_3', math.fmod(10.0, 3.0))
    print('fmod_neg', math.fmod(-10.0, 3.0))
    print('fmod_5_2', math.fmod(5.0, 2.0))

    # modf(x) - fractional and integer parts
    print('modf_3_5', math.modf(3.5))
    print('modf_neg', math.modf(-3.5))
    print('modf_0', math.modf(0.0))

    # remainder(x, y) - IEEE 754 remainder
    print('remainder_10_3', math.remainder(10.0, 3.0))
    print('remainder_11_3', math.remainder(11.0, 3.0))
    print('remainder_neg', math.remainder(-10.0, 3.0))

    # trunc(x)
    print('trunc_2_3', math.trunc(2.3))
    print('trunc_2_7', math.trunc(2.7))
    print('trunc_neg', math.trunc(-2.3))
except Exception as e:
    print('SKIP_Floating point arithmetic', type(e).__name__, e)

# === Floating point manipulation functions ===
try:
    # copysign(x, y)
    print('copysign_1_pos', math.copysign(1.0, 1.0))
    print('copysign_1_neg', math.copysign(1.0, -1.0))
    print('copysign_neg_pos', math.copysign(-1.0, 1.0))
    print('copysign_neg_neg', math.copysign(-1.0, -1.0))

    # frexp(x) - mantissa and exponent
    print('frexp_8', math.frexp(8.0))
    print('frexp_0_5', math.frexp(0.5))
    print('frexp_0', math.frexp(0.0))
    print('frexp_neg', math.frexp(-8.0))

    # isclose(a, b)
    print('isclose_same', math.isclose(1.0, 1.0))
    print('isclose_close', math.isclose(1.0, 1.0000001))
    print('isclose_far', math.isclose(1.0, 2.0))
    print('isclose_tol', math.isclose(1.0, 1.1, rel_tol=0.2))
    print('isclose_abs', math.isclose(1.0, 1.0000001, abs_tol=0.000001))

    # isfinite(x)
    print('isfinite_1', math.isfinite(1.0))
    print('isfinite_inf', math.isfinite(math.inf))
    print('isfinite_nan', math.isfinite(math.nan))
    print('isfinite_0', math.isfinite(0.0))

    # isinf(x)
    print('isinf_1', math.isinf(1.0))
    print('isinf_inf', math.isinf(math.inf))
    print('isinf_neg_inf', math.isinf(-math.inf))
    print('isinf_nan', math.isinf(math.nan))

    # isnan(x)
    print('isnan_1', math.isnan(1.0))
    print('isnan_nan', math.isnan(math.nan))
    print('isnan_inf', math.isnan(math.inf))

    # ldexp(x, i) - x * 2**i
    print('ldexp_1_0', math.ldexp(1.0, 0))
    print('ldexp_1_3', math.ldexp(1.0, 3))
    print('ldexp_1_neg', math.ldexp(1.0, -1))
    print('ldexp_frexp', math.ldexp(*math.frexp(8.0)))

    # nextafter(x, y)
    print('nextafter_1_2', math.nextafter(1.0, 2.0))
    print('nextafter_1_0', math.nextafter(1.0, 0.0))
    print('nextafter_same', math.nextafter(1.0, 1.0))
    print('nextafter_toward_inf', math.nextafter(1.0, math.inf))

    # ulp(x) - unit in the last place
    print('ulp_1', math.ulp(1.0))
    print('ulp_2', math.ulp(2.0))
    print('ulp_0', math.ulp(0.0))
    print('ulp_nan', math.ulp(math.nan))
except Exception as e:
    print('SKIP_Floating point manipulation functions', type(e).__name__, e)

# === Power, exponential and logarithmic functions ===
try:
    # cbrt(x) - cube root
    print('cbrt_8', math.cbrt(8.0))
    print('cbrt_27', math.cbrt(27.0))
    print('cbrt_0', math.cbrt(0.0))
    print('cbrt_neg', math.cbrt(-8.0))
    print('cbrt_1', math.cbrt(1.0))

    # exp(x)
    print('exp_0', math.exp(0.0))
    print('exp_1', math.exp(1.0))
    print('exp_2', math.exp(2.0))
    print('exp_neg', math.exp(-1.0))

    # exp2(x) - 2**x
    print('exp2_0', math.exp2(0.0))
    print('exp2_1', math.exp2(1.0))
    print('exp2_3', math.exp2(3.0))
    print('exp2_neg', math.exp2(-1.0))

    # expm1(x) - exp(x) - 1
    print('expm1_0', math.expm1(0.0))
    print('expm1_1', math.expm1(1.0))
    print('expm1_small', math.expm1(1e-10))

    # log(x[, base])
    print('log_e', math.log(math.e))
    print('log_1', math.log(1.0))
    print('log_10', math.log(10.0))
    print('log_2_base', math.log(8.0, 2.0))
    print('log_10_base', math.log(100.0, 10.0))

    # log1p(x) - log(1+x)
    print('log1p_0', math.log1p(0.0))
    print('log1p_e_minus_1', math.log1p(math.e - 1))
    print('log1p_small', math.log1p(1e-10))

    # log2(x)
    print('log2_1', math.log2(1.0))
    print('log2_2', math.log2(2.0))
    print('log2_8', math.log2(8.0))
    print('log2_1024', math.log2(1024.0))

    # log10(x)
    print('log10_1', math.log10(1.0))
    print('log10_10', math.log10(10.0))
    print('log10_100', math.log10(100.0))
    print('log10_1000', math.log10(1000.0))

    # pow(x, y)
    print('pow_2_3', math.pow(2.0, 3.0))
    print('pow_2_0', math.pow(2.0, 0.0))
    print('pow_2_neg', math.pow(2.0, -1.0))
    print('pow_4_0_5', math.pow(4.0, 0.5))
    print('pow_e_1', math.pow(math.e, 1.0))

    # sqrt(x)
    print('sqrt_0', math.sqrt(0.0))
    print('sqrt_1', math.sqrt(1.0))
    print('sqrt_4', math.sqrt(4.0))
    print('sqrt_2', math.sqrt(2.0))
    print('sqrt_9', math.sqrt(9.0))
except Exception as e:
    print('SKIP_Power, exponential and logarithmic functions', type(e).__name__, e)

# === Summation and product functions ===
try:
    # dist(p, q) - Euclidean distance
    print('dist_2d', math.dist([0, 0], [3, 4]))
    print('dist_1d', math.dist([0], [5]))
    print('dist_3d', math.dist([0, 0, 0], [1, 2, 2]))
    print('dist_same', math.dist([1, 2], [1, 2]))

    # fsum(iterable) - accurate floating point sum
    print('fsum_empty', math.fsum([]))
    print('fsum_single', math.fsum([1.0]))
    print('fsum_simple', math.fsum([1.0, 2.0, 3.0]))
    print('fsum_precise', math.fsum([0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]))

    # hypot(*coordinates) - Euclidean norm
    print('hypot_3_4', math.hypot(3.0, 4.0))
    print('hypot_1_1', math.hypot(1.0, 1.0))
    print('hypot_3d', math.hypot(1.0, 2.0, 2.0))
    print('hypot_neg', math.hypot(-3.0, 4.0))
    print('hypot_single', math.hypot(5.0))

    # prod(iterable[, start])
    print('prod_empty', math.prod([]))
    print('prod_single', math.prod([5]))
    print('prod_simple', math.prod([1, 2, 3, 4]))
    print('prod_start', math.prod([1, 2, 3], start=2))
    print('prod_float', math.prod([1.5, 2.0, 3.0]))

    # sumprod(p, q) - sum of products
    print('sumprod_empty', math.sumprod([], []))
    print('sumprod_simple', math.sumprod([1, 2, 3], [4, 5, 6]))
    print('sumprod_dot', math.sumprod([1, 0], [0, 1]))
    print('sumprod_float', math.sumprod([1.5, 2.5], [2.0, 4.0]))
except Exception as e:
    print('SKIP_Summation and product functions', type(e).__name__, e)

# === Angular conversion ===
try:
    # degrees(x)
    print('degrees_pi', math.degrees(math.pi))
    print('degrees_0', math.degrees(0.0))
    print('degrees_tau', math.degrees(math.tau))
    print('degrees_half_pi', math.degrees(math.pi / 2))

    # radians(x)
    print('radians_180', math.radians(180.0))
    print('radians_0', math.radians(0.0))
    print('radians_360', math.radians(360.0))
    print('radians_90', math.radians(90.0))
except Exception as e:
    print('SKIP_Angular conversion', type(e).__name__, e)

# === Trigonometric functions ===
try:
    # acos(x)
    print('acos_1', math.acos(1.0))
    print('acos_0', math.acos(0.0))
    print('acos_neg1', math.acos(-1.0))
    print('acos_half', math.acos(0.5))

    # asin(x)
    print('asin_0', math.asin(0.0))
    print('asin_1', math.asin(1.0))
    print('asin_neg1', math.asin(-1.0))
    print('asin_half', math.asin(0.5))

    # atan(x)
    print('atan_0', math.atan(0.0))
    print('atan_1', math.atan(1.0))
    print('atan_inf', math.atan(math.inf))
    print('atan_neg_inf', math.atan(-math.inf))

    # atan2(y, x)
    print('atan2_1_0', math.atan2(1.0, 0.0))
    print('atan2_0_1', math.atan2(0.0, 1.0))
    print('atan2_1_1', math.atan2(1.0, 1.0))
    print('atan2_neg', math.atan2(-1.0, -1.0))

    # cos(x)
    print('cos_0', math.cos(0.0))
    print('cos_pi', math.cos(math.pi))
    print('cos_half_pi', math.cos(math.pi / 2))
    print('cos_tau', math.cos(math.tau))

    # sin(x)
    print('sin_0', math.sin(0.0))
    print('sin_half_pi', math.sin(math.pi / 2))
    print('sin_pi', math.sin(math.pi))
    print('sin_tau', math.sin(math.tau))

    # tan(x)
    print('tan_0', math.tan(0.0))
    print('tan_pi_4', math.tan(math.pi / 4))
    print('tan_neg', math.tan(-math.pi / 4))
except Exception as e:
    print('SKIP_Trigonometric functions', type(e).__name__, e)

# === Hyperbolic functions ===
try:
    # acosh(x)
    print('acosh_1', math.acosh(1.0))
    print('acosh_2', math.acosh(2.0))
    print('acosh_10', math.acosh(10.0))

    # asinh(x)
    print('asinh_0', math.asinh(0.0))
    print('asinh_1', math.asinh(1.0))
    print('asinh_neg', math.asinh(-1.0))

    # atanh(x)
    print('atanh_0', math.atanh(0.0))
    print('atanh_half', math.atanh(0.5))
    print('atanh_neg_half', math.atanh(-0.5))

    # cosh(x)
    print('cosh_0', math.cosh(0.0))
    print('cosh_1', math.cosh(1.0))
    print('cosh_neg', math.cosh(-1.0))

    # sinh(x)
    print('sinh_0', math.sinh(0.0))
    print('sinh_1', math.sinh(1.0))
    print('sinh_neg', math.sinh(-1.0))

    # tanh(x)
    print('tanh_0', math.tanh(0.0))
    print('tanh_1', math.tanh(1.0))
    print('tanh_neg', math.tanh(-1.0))
    print('tanh_inf', math.tanh(math.inf))
except Exception as e:
    print('SKIP_Hyperbolic functions', type(e).__name__, e)

# === Special functions ===
try:
    # erf(x) - error function
    print('erf_0', math.erf(0.0))
    print('erf_1', math.erf(1.0))
    print('erf_neg', math.erf(-1.0))
    print('erf_inf', math.erf(math.inf))
    print('erf_neg_inf', math.erf(-math.inf))

    # erfc(x) - complementary error function
    print('erfc_0', math.erfc(0.0))
    print('erfc_1', math.erfc(1.0))
    print('erfc_inf', math.erfc(math.inf))
    print('erfc_neg_inf', math.erfc(-math.inf))

    # gamma(x) - Gamma function
    print('gamma_1', math.gamma(1.0))
    print('gamma_2', math.gamma(2.0))
    print('gamma_3', math.gamma(3.0))
    print('gamma_0_5', math.gamma(0.5))
    print('gamma_5', math.gamma(5.0))

    # lgamma(x) - log of absolute gamma
    print('lgamma_1', math.lgamma(1.0))
    print('lgamma_2', math.lgamma(2.0))
    print('lgamma_3', math.lgamma(3.0))
    print('lgamma_0_5', math.lgamma(0.5))
except Exception as e:
    print('SKIP_Special functions', type(e).__name__, e)

# === Edge cases and exception handling ===
try:
    # Domain errors (handled with try/except)
    try:
        math.sqrt(-1)
    except ValueError as e:
        print('sqrt_neg_error', str(e))

    try:
        math.log(-1)
    except ValueError as e:
        print('log_neg_error', str(e))

    try:
        math.log(0)
    except ValueError as e:
        print('log_zero_error', str(e))

    try:
        math.acos(2)
    except ValueError as e:
        print('acos_gt1_error', str(e))

    try:
        math.acos(-2)
    except ValueError as e:
        print('acos_lt-1_error', str(e))

    try:
        math.factorial(-1)
    except ValueError as e:
        print('factorial_neg_error', str(e))

    try:
        math.comb(-1, 1)
    except ValueError as e:
        print('comb_neg_error', str(e))

    try:
        math.perm(-1, 1)
    except ValueError as e:
        print('perm_neg_error', str(e))

    # Infinity and NaN handling
    print('sqrt_inf', math.sqrt(math.inf))
    print('exp_inf', math.exp(math.inf))
    print('log_inf', math.log(math.inf))
    print('pow_inf_0', math.pow(math.inf, 0.0))

    try:
        math.pow(0.0, -1.0)
    except ValueError as e:
        print('pow_0_neg_error', str(e))

    print('pow_nan', math.isnan(math.pow(math.nan, 1.0)))
    print('sin_nan', math.isnan(math.sin(math.nan)))

    try:
        math.cos(math.inf)
    except ValueError as e:
        print('cos_inf_error', str(e))
    try:
        math.atanh(1.0)
    except ValueError as e:
        print('atanh_1_error', str(e))

    try:
        math.atanh(-1.0)
    except ValueError as e:
        print('atanh_neg1_error', str(e))

    # Overflow
    try:
        math.exp(1000)
    except OverflowError as e:
        print('exp_overflow_error', str(e))
except Exception as e:
    print('SKIP_Edge cases and exception handling', type(e).__name__, e)
