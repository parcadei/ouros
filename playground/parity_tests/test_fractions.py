"""Parity tests for Python's fractions module."""

from fractions import Fraction
from decimal import Decimal

# === Construction: No args ===
try:
    f = Fraction()
    print('frac_no_args', f)
except Exception as e:
    print('SKIP_Construction: No args', type(e).__name__, e)

# === Construction: Single int ===
try:
    f = Fraction(3)
    print('frac_single_int', f)

    f = Fraction(-5)
    print('frac_single_int_neg', f)
except Exception as e:
    print('SKIP_Construction: Single int', type(e).__name__, e)

# === Construction: Two ints ===
try:
    f = Fraction(1, 2)
    print('frac_two_ints', f)
except Exception as e:
    print('SKIP_Construction: Two ints', type(e).__name__, e)

# === Construction: Normalization (GCD reduction) ===
try:
    f = Fraction(3, 9)
    print('frac_normalize_3_9', f)

    f = Fraction(10, 20)
    print('frac_normalize_10_20', f)

    f = Fraction(8, 12)
    print('frac_normalize_8_12', f)
except Exception as e:
    print('SKIP_Construction: Normalization (GCD reduction)', type(e).__name__, e)

# === Construction: Negative numerator ===
try:
    f = Fraction(-1, 2)
    print('frac_neg_num', f)
except Exception as e:
    print('SKIP_Construction: Negative numerator', type(e).__name__, e)

# === Construction: Negative denominator ===
try:
    f = Fraction(1, -2)
    print('frac_neg_den', f)
except Exception as e:
    print('SKIP_Construction: Negative denominator', type(e).__name__, e)

# === Construction: Both negative ===
try:
    f = Fraction(-1, -2)
    print('frac_both_neg', f)
except Exception as e:
    print('SKIP_Construction: Both negative', type(e).__name__, e)

# === Construction: From string ===
try:
    f = Fraction("3/7")
    print('frac_str_3_7', f)

    f = Fraction("-2/5")
    print('frac_str_neg_2_5', f)

    f = Fraction("10/25")
    print('frac_str_normalize', f)
except Exception as e:
    print('SKIP_Construction: From string', type(e).__name__, e)

# === Construction: From float ===
try:
    f = Fraction(0.5)
    print('frac_float_half', f)

    f = Fraction(0.25)
    print('frac_float_quarter', f)

    f = Fraction(0.75)
    print('frac_float_3_quarter', f)

    f = Fraction(1.5)
    print('frac_float_one_half', f)
except Exception as e:
    print('SKIP_Construction: From float', type(e).__name__, e)

# === Construction: From Decimal ===
try:
    f = Fraction(Decimal('0.5'))
    print('frac_decimal_half', f)

    f = Fraction(Decimal('3.14159'))
    print('frac_decimal_pi', f)
except Exception as e:
    print('SKIP_Construction: From Decimal', type(e).__name__, e)

# === Construction: From another Fraction ===
try:
    f1 = Fraction(1, 3)
    f2 = Fraction(f1)
    print('frac_from_frac', f2)
except Exception as e:
    print('SKIP_Construction: From another Fraction', type(e).__name__, e)

# === Class method from_float ===
try:
    f = Fraction.from_float(0.5)
    print('frac_from_float', f)
except Exception as e:
    print('SKIP_Class method from_float', type(e).__name__, e)

# === Class method from_decimal ===
try:
    f = Fraction.from_decimal(Decimal('0.25'))
    print('frac_from_decimal', f)
except Exception as e:
    print('SKIP_Class method from_decimal', type(e).__name__, e)

# === Class method from_number - Python 3.14+ ===
try:
    f = Fraction.from_number(0.5)
    print('frac_from_number', f)
except AttributeError:
    print('frac_from_number', 'not_available')
except Exception as e:
    print('SKIP_Class method from_number - Python 3.14+', type(e).__name__, e)

# === Properties: numerator ===
try:
    f = Fraction(3, 4)
    print('frac_num_3_4', f.numerator)

    f = Fraction(-2, 3)
    print('frac_num_neg', f.numerator)

    f = Fraction(10, 15)
    print('frac_num_normalized', f.numerator)
except Exception as e:
    print('SKIP_Properties: numerator', type(e).__name__, e)

# === Properties: denominator ===
try:
    f = Fraction(3, 4)
    print('frac_den_3_4', f.denominator)

    f = Fraction(-2, 3)
    print('frac_den_neg_num', f.denominator)

    f = Fraction(2, -3)
    print('frac_den_neg_den', f.denominator)

    f = Fraction(10, 15)
    print('frac_den_normalized', f.denominator)
except Exception as e:
    print('SKIP_Properties: denominator', type(e).__name__, e)

# === Properties: real and imag ===
try:
    f = Fraction(3, 4)
    print('frac_real', f.real)
    print('frac_imag', f.imag)
    print('frac_imag_is_zero', f.imag == 0)
except Exception as e:
    print('SKIP_Properties: real and imag', type(e).__name__, e)

# === Arithmetic: Addition ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    print('frac_add_1_2_1_3', f1 + f2)

    f1 = Fraction(3, 4)
    f2 = Fraction(1, 4)
    print('frac_add_3_4_1_4', f1 + f2)

    f1 = Fraction(-1, 2)
    f2 = Fraction(1, 3)
    print('frac_add_neg_pos', f1 + f2)
except Exception as e:
    print('SKIP_Arithmetic: Addition', type(e).__name__, e)

# === Arithmetic: Addition with int ===
try:
    f = Fraction(1, 2)
    print('frac_add_int', f + 1)

    f = Fraction(3, 4)
    print('frac_add_int_2', f + 2)
except Exception as e:
    print('SKIP_Arithmetic: Addition with int', type(e).__name__, e)

# === Arithmetic: Subtraction ===
try:
    f1 = Fraction(3, 4)
    f2 = Fraction(1, 4)
    print('frac_sub_3_4_1_4', f1 - f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    print('frac_sub_1_2_1_3', f1 - f2)

    f1 = Fraction(1, 3)
    f2 = Fraction(1, 2)
    print('frac_sub_1_3_1_2', f1 - f2)
except Exception as e:
    print('SKIP_Arithmetic: Subtraction', type(e).__name__, e)

# === Arithmetic: Subtraction with int ===
try:
    f = Fraction(3, 2)
    print('frac_sub_int', f - 1)
except Exception as e:
    print('SKIP_Arithmetic: Subtraction with int', type(e).__name__, e)

# === Arithmetic: Multiplication ===
try:
    f1 = Fraction(2, 3)
    f2 = Fraction(3, 4)
    print('frac_mul_2_3_3_4', f1 * f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    print('frac_mul_1_2_1_3', f1 * f2)

    f1 = Fraction(-1, 2)
    f2 = Fraction(2, 3)
    print('frac_mul_neg', f1 * f2)
except Exception as e:
    print('SKIP_Arithmetic: Multiplication', type(e).__name__, e)

# === Arithmetic: Multiplication with int ===
try:
    f = Fraction(2, 3)
    print('frac_mul_int', f * 3)
except Exception as e:
    print('SKIP_Arithmetic: Multiplication with int', type(e).__name__, e)

# === Arithmetic: Division ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(1, 4)
    print('frac_div_1_2_1_4', f1 / f2)

    f1 = Fraction(3, 4)
    f2 = Fraction(1, 2)
    print('frac_div_3_4_1_2', f1 / f2)

    f1 = Fraction(2, 3)
    f2 = Fraction(4, 5)
    print('frac_div_2_3_4_5', f1 / f2)
except Exception as e:
    print('SKIP_Arithmetic: Division', type(e).__name__, e)

# === Arithmetic: Division with int ===
try:
    f = Fraction(3, 2)
    print('frac_div_int', f / 2)
except Exception as e:
    print('SKIP_Arithmetic: Division with int', type(e).__name__, e)

# === Arithmetic: Floor division ===
try:
    f1 = Fraction(7, 3)
    f2 = Fraction(1, 2)
    print('frac_floordiv_7_3_1_2', f1 // f2)

    f1 = Fraction(5, 2)
    f2 = Fraction(3, 4)
    print('frac_floordiv_5_2_3_4', f1 // f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(3, 4)
    print('frac_floordiv_less_than', f1 // f2)
except Exception as e:
    print('SKIP_Arithmetic: Floor division', type(e).__name__, e)

# === Arithmetic: Floor division with int ===
try:
    f = Fraction(7, 2)
    print('frac_floordiv_int', f // 2)
except Exception as e:
    print('SKIP_Arithmetic: Floor division with int', type(e).__name__, e)

# === Arithmetic: Modulo ===
try:
    f1 = Fraction(7, 3)
    f2 = Fraction(1, 2)
    print('frac_mod', f1 % f2)
except Exception as e:
    print('SKIP_Arithmetic: Modulo', type(e).__name__, e)

# === Arithmetic: Modulo with int ===
try:
    f = Fraction(7, 2)
    print('frac_mod_int', f % 2)
except Exception as e:
    print('SKIP_Arithmetic: Modulo with int', type(e).__name__, e)

# === Arithmetic: Power ===
try:
    f1 = Fraction(2, 3)
    f2 = Fraction(3, 1)
    print('frac_pow_frac', f1 ** f2)

    f = Fraction(3, 2)
    print('frac_pow_int', f ** 2)
except Exception as e:
    print('SKIP_Arithmetic: Power', type(e).__name__, e)

# === Comparison: Equal ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(1, 2)
    print('frac_eq_true', f1 == f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_eq_normalized', f1 == f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    print('frac_eq_false', f1 == f2)
except Exception as e:
    print('SKIP_Comparison: Equal', type(e).__name__, e)

# === Comparison: Not equal ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    print('frac_neq_true', f1 != f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_neq_false', f1 != f2)
except Exception as e:
    print('SKIP_Comparison: Not equal', type(e).__name__, e)

# === Comparison: Less than ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(3, 4)
    print('frac_lt_true', f1 < f2)

    f1 = Fraction(3, 4)
    f2 = Fraction(1, 2)
    print('frac_lt_false', f1 < f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_lt_equal', f1 < f2)
except Exception as e:
    print('SKIP_Comparison: Less than', type(e).__name__, e)

# === Comparison: Greater than ===
try:
    f1 = Fraction(3, 4)
    f2 = Fraction(1, 2)
    print('frac_gt_true', f1 > f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(3, 4)
    print('frac_gt_false', f1 > f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_gt_equal', f1 > f2)
except Exception as e:
    print('SKIP_Comparison: Greater than', type(e).__name__, e)

# === Comparison: Less than or equal ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(3, 4)
    print('frac_le_true', f1 <= f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_le_equal', f1 <= f2)

    f1 = Fraction(3, 4)
    f2 = Fraction(1, 2)
    print('frac_le_false', f1 <= f2)
except Exception as e:
    print('SKIP_Comparison: Less than or equal', type(e).__name__, e)

# === Comparison: Greater than or equal ===
try:
    f1 = Fraction(3, 4)
    f2 = Fraction(1, 2)
    print('frac_ge_true', f1 >= f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_ge_equal', f1 >= f2)

    f1 = Fraction(1, 2)
    f2 = Fraction(3, 4)
    print('frac_ge_false', f1 >= f2)
except Exception as e:
    print('SKIP_Comparison: Greater than or equal', type(e).__name__, e)

# === Comparison: With int ===
try:
    f = Fraction(3, 1)
    print('frac_eq_int', f == 3)

    f = Fraction(7, 2)
    print('frac_lt_int', f < 5)
    print('frac_gt_int', f > 2)
except Exception as e:
    print('SKIP_Comparison: With int', type(e).__name__, e)

# === Methods: limit_denominator ===
try:
    f = Fraction(355, 113)
    print('frac_limit_denom_default', f.limit_denominator())

    f = Fraction(355, 113)
    print('frac_limit_denom_10', f.limit_denominator(10))

    f = Fraction(355, 113)
    print('frac_limit_denom_100', f.limit_denominator(100))

    f = Fraction(1, 10)
    print('frac_limit_denom_larger', f.limit_denominator(100))

    f = Fraction(333, 106)
    print('frac_limit_denom_pi_approx', f.limit_denominator(10))
except Exception as e:
    print('SKIP_Methods: limit_denominator', type(e).__name__, e)

# === Methods: as_integer_ratio ===
try:
    f = Fraction(3, 4)
    num, den = f.as_integer_ratio()
    print('frac_as_integer_ratio_num', num)
    print('frac_as_integer_ratio_den', den)
except Exception as e:
    print('SKIP_Methods: as_integer_ratio', type(e).__name__, e)

# === Methods: is_integer ===
try:
    f = Fraction(4, 2)
    print('frac_is_integer_true', f.is_integer())

    f = Fraction(3, 2)
    print('frac_is_integer_false', f.is_integer())
except Exception as e:
    print('SKIP_Methods: is_integer', type(e).__name__, e)

# === Methods: conjugate ===
try:
    f = Fraction(3, 4)
    print('frac_conjugate', f.conjugate())
    print('frac_conjugate_is_self', f.conjugate() == f)
except Exception as e:
    print('SKIP_Methods: conjugate', type(e).__name__, e)

# === Methods: __round__ ===
try:
    f = Fraction(3, 2)
    print('frac_round', round(f))

    f = Fraction(7, 3)
    print('frac_round_2', round(f))

    f = Fraction(355, 113)
    print('frac_round_ndigits', round(f, 2))
except Exception as e:
    print('SKIP_Methods: __round__', type(e).__name__, e)

# === Methods: __floor__, __ceil__, __trunc__ ===
try:
    import math
    f = Fraction(7, 3)
    print('frac_floor', math.floor(f))
    print('frac_ceil', math.ceil(f))
    print('frac_trunc', math.trunc(f))
except Exception as e:
    print('SKIP_Methods: __floor__, __ceil__, __trunc__', type(e).__name__, e)

# === Edge cases: Zero numerator ===
try:
    f = Fraction(0, 5)
    print('frac_zero_num', f)

    f = Fraction(0, 1)
    print('frac_zero_1', f)

    f = Fraction(0, -5)
    print('frac_zero_neg_den', f)
except Exception as e:
    print('SKIP_Edge cases: Zero numerator', type(e).__name__, e)

# === Edge cases: Large numbers ===
try:
    f1 = Fraction(123456789, 1000000000)
    f2 = Fraction(987654321, 1000000000)
    print('frac_large_add', f1 + f2)

    f1 = Fraction(1000000, 3)
    f2 = Fraction(1, 1000000)
    print('frac_large_mul', f1 * f2)

    f = Fraction(999999999, 1000000000)
    print('frac_large_numerator', f.numerator)
    print('frac_large_denominator', f.denominator)
except Exception as e:
    print('SKIP_Edge cases: Large numbers', type(e).__name__, e)

# === Edge cases: Proper normalization ===
try:
    f = Fraction(1071, 462)
    print('frac_normalize_gcd', f)

    f = Fraction(100, 25)
    print('frac_normalize_100_25', f)

    f = Fraction(-100, 25)
    print('frac_normalize_neg_100_25', f)

    f = Fraction(100, -25)
    print('frac_normalize_100_neg_25', f)

    f = Fraction(-100, -25)
    print('frac_normalize_both_neg', f)
except Exception as e:
    print('SKIP_Edge cases: Proper normalization', type(e).__name__, e)

# === Edge cases: Identity operations ===
try:
    f = Fraction(1, 2)
    print('frac_add_zero', f + Fraction(0))

    f = Fraction(1, 2)
    print('frac_mul_one', f * Fraction(1))

    f = Fraction(1, 2)
    print('frac_sub_self', f - f)

    f = Fraction(1, 2)
    print('frac_div_self', f / f)
except Exception as e:
    print('SKIP_Edge cases: Identity operations', type(e).__name__, e)

# === Edge cases: Negative zero handling ===
try:
    f = Fraction(0, 1)
    print('frac_zero_num_property', f.numerator)
    print('frac_zero_den_property', f.denominator)
except Exception as e:
    print('SKIP_Edge cases: Negative zero handling', type(e).__name__, e)

# === Repr and str ===
try:
    f = Fraction(1, 2)
    print('frac_repr', repr(f))

    f = Fraction(3, 9)
    print('frac_repr_normalized', repr(f))

    f = Fraction(-1, 2)
    print('frac_repr_neg', repr(f))

    f = Fraction(1, 2)
    print('frac_str', str(f))
except Exception as e:
    print('SKIP_Repr and str', type(e).__name__, e)

# === Hash consistency ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(2, 4)
    print('frac_hash_equal', hash(f1) == hash(f2))

    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    print('frac_hash_diff', hash(f1) != hash(f2))
except Exception as e:
    print('SKIP_Hash consistency', type(e).__name__, e)

# === Chained operations ===
try:
    f1 = Fraction(1, 2)
    f2 = Fraction(1, 3)
    f3 = Fraction(1, 6)
    print('frac_chained_add', f1 + f2 + f3)

    f = Fraction(1, 2)
    print('frac_chained_mul_div', f * 2 / 3)
except Exception as e:
    print('SKIP_Chained operations', type(e).__name__, e)

# === Complex expressions ===
try:
    result = Fraction(1, 2) + Fraction(1, 3) * Fraction(2, 5)
    print('frac_complex_expr1', result)

    result = (Fraction(1, 2) + Fraction(1, 3)) * Fraction(2, 5)
    print('frac_complex_expr2', result)
except Exception as e:
    print('SKIP_Complex expressions', type(e).__name__, e)

# === Float conversion round-trip ===
try:
    f1 = Fraction(0.5)
    f2 = Fraction(f1)
    print('frac_float_roundtrip', f2)
except Exception as e:
    print('SKIP_Float conversion round-trip', type(e).__name__, e)

# === Zero division edge case (should raise) ===
try:
    f = Fraction(1, 0)
    print('frac_zero_div_error', 'NO_ERROR_RAISED')
except ZeroDivisionError as e:
    print('frac_zero_div_error', str(e))
except Exception as e:
    print('SKIP_Zero division edge case (should raise)', type(e).__name__, e)

# === Float to Fraction with exact float issues ===
try:
    f = Fraction(0.1)
    print('frac_float_0_1', f)

    f = Fraction(0.3)
    print('frac_float_0_3', f)
except Exception as e:
    print('SKIP_Float to Fraction with exact float issues', type(e).__name__, e)

# === Hash with int ===
try:
    f = Fraction(6, 2)
    print('frac_hash_eq_int', hash(f) == hash(3))
except Exception as e:
    print('SKIP_Hash with int', type(e).__name__, e)
