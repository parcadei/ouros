"""Parity tests for Python's decimal module."""

from decimal import (
    Decimal, Context, getcontext, setcontext, localcontext,
    BasicContext, DefaultContext, ExtendedContext,
    ROUND_UP, ROUND_DOWN, ROUND_CEILING, ROUND_FLOOR,
    ROUND_HALF_UP, ROUND_HALF_DOWN, ROUND_HALF_EVEN, ROUND_05UP,
    InvalidOperation, DivisionByZero, Overflow, Underflow,
    Inexact, Rounded, Clamped, DivisionImpossible, DivisionUndefined,
    FloatOperation, InvalidContext, Subnormal,
)

# === Construction from string ===
try:
    print('construct_str_int', Decimal('123'))
    print('construct_str_negative', Decimal('-456.789'))
    print('construct_str_exp', Decimal('1.23E+10'))
    print('construct_str_decimal', Decimal('3.14159'))
except Exception as e:
    print('SKIP_Construction_from_string', type(e).__name__, e)

# === Construction from int ===
try:
    print('construct_int_positive', Decimal(123))
    print('construct_int_negative', Decimal(-456))
    print('construct_int_zero', Decimal(0))
except Exception as e:
    print('SKIP_Construction_from_int', type(e).__name__, e)

# === Construction from Decimal ===
try:
    print('construct_decimal', Decimal(Decimal('1.5')))
except Exception as e:
    print('SKIP_Construction_from_Decimal', type(e).__name__, e)

# === Construction from float via from_float ===
try:
    print('construct_from_float', Decimal.from_float(3.14159))
    print('construct_from_float_half', Decimal.from_float(0.5))
except Exception as e:
    print('SKIP_Construction_from_float_via_from_float', type(e).__name__, e)

# === Special values ===
try:
    print('construct_inf', Decimal('Infinity'))
    print('construct_neg_inf', Decimal('-Infinity'))
    print('construct_nan', Decimal('NaN'))
    print('construct_neg_nan', Decimal('-NaN'))
    print('construct_snan', Decimal('sNaN'))
except Exception as e:
    print('SKIP_Special_values', type(e).__name__, e)

# === Context constants ===
try:
    print('basic_context_exists', BasicContext is not None)
    print('default_context_exists', DefaultContext is not None)
    print('extended_context_exists', ExtendedContext is not None)
except Exception as e:
    print('SKIP_Context_constants', type(e).__name__, e)

# === Rounding mode constants ===
try:
    print('round_up', ROUND_UP)
    print('round_down', ROUND_DOWN)
    print('round_ceiling', ROUND_CEILING)
    print('round_floor', ROUND_FLOOR)
    print('round_half_up', ROUND_HALF_UP)
    print('round_half_down', ROUND_HALF_DOWN)
    print('round_half_even', ROUND_HALF_EVEN)
    print('round_05up', ROUND_05UP)
except Exception as e:
    print('SKIP_Rounding_mode_constants', type(e).__name__, e)

# === getcontext and setcontext ===
try:
    ctx = getcontext()
    print('getcontext_returns_context', type(ctx) is Context)

    # Save current context
    saved_ctx = ctx.copy()

    # Create a new context with different precision
    new_ctx = Context(prec=10, rounding=ROUND_HALF_UP)
    setcontext(new_ctx)
    current = getcontext()
    print('setcontext_works', current.prec == 10)
    print('setcontext_rounding', current.rounding == ROUND_HALF_UP)

    # Restore original context
    setcontext(saved_ctx)
except Exception as e:
    print('SKIP_getcontext_and_setcontext', type(e).__name__, e)

# === localcontext ===
try:
    with localcontext() as ctx:
        ctx.prec = 5
        result = Decimal('1') / Decimal('3')
        print('localcontext_prec_5', len(str(result).replace('.', '')) <= 6)

    # Verify context was restored
    print('localcontext_restored', getcontext().prec == saved_ctx.prec)
except Exception as e:
    print('SKIP_localcontext', type(e).__name__, e)

# === Arithmetic: Addition ===
try:
    print('add_decimal', Decimal('1.1') + Decimal('2.2'))
    print('add_int', Decimal('1.5') + 2)
    print('add_zero', Decimal('5.0') + Decimal('0.0'))
    print('add_negative', Decimal('5.0') + Decimal('-3.0'))
except Exception as e:
    print('SKIP_Arithmetic:_Addition', type(e).__name__, e)

# === Arithmetic: Subtraction ===
try:
    print('sub_decimal', Decimal('5.0') - Decimal('3.0'))
    print('sub_int', Decimal('10.5') - 3)
    print('sub_negative', Decimal('5.0') - Decimal('-3.0'))
    print('sub_zero', Decimal('5.0') - Decimal('0.0'))
except Exception as e:
    print('SKIP_Arithmetic:_Subtraction', type(e).__name__, e)

# === Arithmetic: Multiplication ===
try:
    print('mul_decimal', Decimal('2.5') * Decimal('4.0'))
    print('mul_int', Decimal('2.5') * 4)
    print('mul_negative', Decimal('2.5') * Decimal('-2.0'))
    print('mul_zero', Decimal('5.0') * Decimal('0.0'))
except Exception as e:
    print('SKIP_Arithmetic:_Multiplication', type(e).__name__, e)

# === Arithmetic: Division ===
try:
    print('div_decimal', Decimal('7') / Decimal('2'))
    print('div_int', Decimal('10') / 4)
    print('div_exact', Decimal('6') / Decimal('3'))
    print('div_negative', Decimal('7') / Decimal('-2'))
except Exception as e:
    print('SKIP_Arithmetic:_Division', type(e).__name__, e)

# === Arithmetic: Floor Division ===
try:
    print('floordiv_decimal', Decimal('7') // Decimal('2'))
    print('floordiv_int', Decimal('10') // 3)
    print('floordiv_negative', Decimal('-7') // Decimal('2'))
except Exception as e:
    print('SKIP_Arithmetic:_Floor_Division', type(e).__name__, e)

# === Arithmetic: Modulo ===
try:
    print('mod_decimal', Decimal('7') % Decimal('4'))
    print('mod_int', Decimal('10') % 3)
    print('mod_negative', Decimal('-7') % Decimal('4'))
except Exception as e:
    print('SKIP_Arithmetic:_Modulo', type(e).__name__, e)

# === Arithmetic: Power ===
try:
    print('pow_decimal', Decimal('2') ** Decimal('10'))
    print('pow_int_exp', Decimal('2') ** 3)
    print('pow_frac', Decimal('4') ** Decimal('0.5'))
    print('pow_zero', Decimal('5') ** 0)
except Exception as e:
    print('SKIP_Arithmetic:_Power', type(e).__name__, e)

# === Comparisons: Equal ===
try:
    print('eq_same', Decimal('1.5') == Decimal('1.5'))
    print('eq_equiv', Decimal('1.5') == Decimal('1.50'))
    print('eq_int', Decimal('5') == 5)
    print('eq_diff', Decimal('1.5') == Decimal('2.5'))
except Exception as e:
    print('SKIP_Comparisons:_Equal', type(e).__name__, e)

# === Comparisons: Not Equal ===
try:
    print('ne_same', Decimal('1.5') != Decimal('1.5'))
    print('ne_diff', Decimal('1.5') != Decimal('2.5'))
    print('ne_int', Decimal('5') != 5)
except Exception as e:
    print('SKIP_Comparisons:_Not_Equal', type(e).__name__, e)

# === Comparisons: Less Than ===
try:
    print('lt_true', Decimal('1.5') < Decimal('2.5'))
    print('lt_false', Decimal('2.5') < Decimal('1.5'))
    print('lt_int', Decimal('3') < 5)
    print('lt_equal', Decimal('5') < Decimal('5'))
except Exception as e:
    print('SKIP_Comparisons:_Less_Than', type(e).__name__, e)

# === Comparisons: Greater Than ===
try:
    print('gt_true', Decimal('2.5') > Decimal('1.5'))
    print('gt_false', Decimal('1.5') > Decimal('2.5'))
    print('gt_int', Decimal('5') > 3)
    print('gt_equal', Decimal('5') > Decimal('5'))
except Exception as e:
    print('SKIP_Comparisons:_Greater_Than', type(e).__name__, e)

# === Comparisons: Less Than or Equal ===
try:
    print('le_true', Decimal('1.5') <= Decimal('2.5'))
    print('le_equal', Decimal('5') <= Decimal('5'))
    print('le_false', Decimal('2.5') <= Decimal('1.5'))
except Exception as e:
    print('SKIP_Comparisons:_Less_Than_or_Equal', type(e).__name__, e)

# === Comparisons: Greater Than or Equal ===
try:
    print('ge_true', Decimal('2.5') >= Decimal('1.5'))
    print('ge_equal', Decimal('5') >= Decimal('5'))
    print('ge_false', Decimal('1.5') >= Decimal('2.5'))
except Exception as e:
    print('SKIP_Comparisons:_Greater_Than_or_Equal', type(e).__name__, e)

# === Method: quantize ===
try:
    print('quantize_basic', Decimal('1.234').quantize(Decimal('0.01')))
    print('quantize_round', Decimal('1.235').quantize(Decimal('0.01')))
    print('quantize_int', Decimal('123.456').quantize(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_quantize', type(e).__name__, e)

# === Method: to_eng_string ===
try:
    print('to_eng_str_basic', Decimal('123E+4').to_eng_string())
    print('to_eng_str_small', Decimal('0.00123').to_eng_string())
    print('to_eng_str_large', Decimal('1230000').to_eng_string())
except Exception as e:
    print('SKIP_Method:_to_eng_string', type(e).__name__, e)

# === Method: copy_abs ===
try:
    print('copy_abs_positive', Decimal('1.5').copy_abs())
    print('copy_abs_negative', Decimal('-1.5').copy_abs())
    print('copy_abs_zero', Decimal('0').copy_abs())
except Exception as e:
    print('SKIP_Method:_copy_abs', type(e).__name__, e)

# === Method: copy_negate ===
try:
    print('copy_negate_positive', Decimal('1.5').copy_negate())
    print('copy_negate_negative', Decimal('-1.5').copy_negate())
    print('copy_negate_zero', Decimal('0').copy_negate())
except Exception as e:
    print('SKIP_Method:_copy_negate', type(e).__name__, e)

# === Method: copy_sign ===
try:
    print('copy_sign_neg', Decimal('1.5').copy_sign(Decimal('-2.0')))
    print('copy_sign_pos', Decimal('-1.5').copy_sign(Decimal('2.0')))
    print('copy_sign_same', Decimal('1.5').copy_sign(Decimal('2.0')))
except Exception as e:
    print('SKIP_Method:_copy_sign', type(e).__name__, e)

# === Method: as_tuple ===
try:
    d = Decimal('123.456')
    t = d.as_tuple()
    print('as_tuple_sign', t.sign)
    print('as_tuple_digits', t.digits)
    print('as_tuple_exponent', t.exponent)
except Exception as e:
    print('SKIP_Method:_as_tuple', type(e).__name__, e)

# === Method: as_integer_ratio ===
try:
    d = Decimal('1.5')
    num, den = d.as_integer_ratio()
    print('as_integer_ratio_num', num)
    print('as_integer_ratio_den', den)
except Exception as e:
    print('SKIP_Method:_as_integer_ratio', type(e).__name__, e)

# === Method: adjusted ===
try:
    print('adjusted_123', Decimal('123').adjusted())
    print('adjusted_123E5', Decimal('123E5').adjusted())
    print('adjusted_small', Decimal('0.00123').adjusted())
except Exception as e:
    print('SKIP_Method:_adjusted', type(e).__name__, e)

# === Method: canonical ===
try:
    d = Decimal('1.5')
    print('canonical', d.canonical())
except Exception as e:
    print('SKIP_Method:_canonical', type(e).__name__, e)

# === Method: compare ===
try:
    print('compare_lt', Decimal('1').compare(Decimal('2')))
    print('compare_eq', Decimal('1').compare(Decimal('1')))
    print('compare_gt', Decimal('2').compare(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_compare', type(e).__name__, e)

# === Method: compare_signal ===
try:
    print('compare_signal_lt', Decimal('1').compare_signal(Decimal('2')))
except Exception as e:
    print('SKIP_Method:_compare_signal', type(e).__name__, e)

# === Method: compare_total ===
try:
    print('compare_total', Decimal('1').compare_total(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_compare_total', type(e).__name__, e)

# === Method: compare_total_mag ===
try:
    print('compare_total_mag', Decimal('-1').compare_total_mag(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_compare_total_mag', type(e).__name__, e)

# === Method: conjugate ===
try:
    print('conjugate', Decimal('1.5').conjugate())
except Exception as e:
    print('SKIP_Method:_conjugate', type(e).__name__, e)

# === Method: exp ===
try:
    with localcontext() as ctx:
        ctx.prec = 10
        print('exp_1', Decimal('1').exp())
except Exception as e:
    print('SKIP_Method:_exp', type(e).__name__, e)

# === Method: fma ===
try:
    # fused multiply-add: (self * other) + third
    print('fma', Decimal('2').fma(Decimal('3'), Decimal('4')))
except Exception as e:
    print('SKIP_Method:_fma', type(e).__name__, e)

# === Method: is_canonical ===
try:
    print('is_canonical', Decimal('1.5').is_canonical())
except Exception as e:
    print('SKIP_Method:_is_canonical', type(e).__name__, e)

# === Method: is_normal ===
try:
    print('is_normal', Decimal('1.5').is_normal())
    print('is_normal_zero', Decimal('0').is_normal())
except Exception as e:
    print('SKIP_Method:_is_normal', type(e).__name__, e)

# === Method: is_qnan ===
try:
    print('is_qnan_false', Decimal('1').is_qnan())
    print('is_qnan_true', Decimal('NaN').is_qnan())
except Exception as e:
    print('SKIP_Method:_is_qnan', type(e).__name__, e)

# === Method: is_snan ===
try:
    print('is_snan_false', Decimal('1').is_snan())
    print('is_snan_true', Decimal('sNaN').is_snan())
except Exception as e:
    print('SKIP_Method:_is_snan', type(e).__name__, e)

# === Method: is_subnormal ===
try:
    print('is_subnormal', Decimal('1E-100').is_subnormal())
except Exception as e:
    print('SKIP_Method:_is_subnormal', type(e).__name__, e)

# === Method: ln ===
try:
    with localcontext() as ctx:
        ctx.prec = 10
        print('ln', Decimal('2.718281828').ln())
except Exception as e:
    print('SKIP_Method:_ln', type(e).__name__, e)

# === Method: log10 ===
try:
    with localcontext() as ctx:
        ctx.prec = 10
        print('log10', Decimal('100').log10())
except Exception as e:
    print('SKIP_Method:_log10', type(e).__name__, e)

# === Method: logb ===
try:
    print('logb', Decimal('123').logb())
except Exception as e:
    print('SKIP_Method:_logb', type(e).__name__, e)

# === Method: logical_and ===
try:
    print('logical_and', Decimal('1').logical_and(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_logical_and', type(e).__name__, e)

# === Method: logical_invert ===
try:
    print('logical_invert', Decimal('1').logical_invert())
except Exception as e:
    print('SKIP_Method:_logical_invert', type(e).__name__, e)

# === Method: logical_or ===
try:
    print('logical_or', Decimal('0').logical_or(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_logical_or', type(e).__name__, e)

# === Method: logical_xor ===
try:
    print('logical_xor', Decimal('1').logical_xor(Decimal('1')))
except Exception as e:
    print('SKIP_Method:_logical_xor', type(e).__name__, e)

# === Method: max ===
try:
    print('max', Decimal('5').max(Decimal('3')))
except Exception as e:
    print('SKIP_Method:_max', type(e).__name__, e)

# === Method: max_mag ===
try:
    print('max_mag', Decimal('-5').max_mag(Decimal('3')))
except Exception as e:
    print('SKIP_Method:_max_mag', type(e).__name__, e)

# === Method: min ===
try:
    print('min', Decimal('5').min(Decimal('3')))
except Exception as e:
    print('SKIP_Method:_min', type(e).__name__, e)

# === Method: min_mag ===
try:
    print('min_mag', Decimal('-5').min_mag(Decimal('3')))
except Exception as e:
    print('SKIP_Method:_min_mag', type(e).__name__, e)

# === Method: next_minus ===
try:
    print('next_minus', Decimal('1').next_minus())
except Exception as e:
    print('SKIP_Method:_next_minus', type(e).__name__, e)

# === Method: next_plus ===
try:
    print('next_plus', Decimal('1').next_plus())
except Exception as e:
    print('SKIP_Method:_next_plus', type(e).__name__, e)

# === Method: next_toward ===
try:
    print('next_toward', Decimal('1').next_toward(Decimal('2')))
except Exception as e:
    print('SKIP_Method:_next_toward', type(e).__name__, e)

# === Method: normalize ===
try:
    print('normalize', Decimal('1.2300').normalize())
except Exception as e:
    print('SKIP_Method:_normalize', type(e).__name__, e)

# === Method: number_class ===
try:
    print('number_class', Decimal('1').number_class())
    print('number_class_inf', Decimal('Infinity').number_class())
except Exception as e:
    print('SKIP_Method:_number_class', type(e).__name__, e)

# === Method: radix ===
try:
    print('radix', Decimal('1').radix())
except Exception as e:
    print('SKIP_Method:_radix', type(e).__name__, e)

# === Method: remainder_near ===
try:
    print('remainder_near', Decimal('10').remainder_near(Decimal('3')))
except Exception as e:
    print('SKIP_Method:_remainder_near', type(e).__name__, e)

# === Method: rotate ===
try:
    print('rotate', Decimal('123').rotate(1))
except Exception as e:
    print('SKIP_Method:_rotate', type(e).__name__, e)

# === Method: same_quantum ===
try:
    print('same_quantum_true', Decimal('1.00').same_quantum(Decimal('2.00')))
    print('same_quantum_false', Decimal('1.0').same_quantum(Decimal('2.00')))
except Exception as e:
    print('SKIP_Method:_same_quantum', type(e).__name__, e)

# === Method: scaleb ===
try:
    print('scaleb', Decimal('1.23').scaleb(2))
except Exception as e:
    print('SKIP_Method:_scaleb', type(e).__name__, e)

# === Method: shift ===
try:
    print('shift', Decimal('12345').shift(2))
except Exception as e:
    print('SKIP_Method:_shift', type(e).__name__, e)

# === Method: sqrt ===
try:
    with localcontext() as ctx:
        ctx.prec = 10
        print('sqrt', Decimal('4').sqrt())
except Exception as e:
    print('SKIP_Method:_sqrt', type(e).__name__, e)

# === Method: to_integral ===
try:
    print('to_integral', Decimal('1.5').to_integral())
except Exception as e:
    print('SKIP_Method:_to_integral', type(e).__name__, e)

# === Method: to_integral_exact ===
try:
    print('to_integral_exact', Decimal('2').to_integral_exact())
except Exception as e:
    print('SKIP_Method:_to_integral_exact', type(e).__name__, e)

# === Method: to_integral_value ===
try:
    print('to_integral_value', Decimal('3.7').to_integral_value())
except Exception as e:
    print('SKIP_Method:_to_integral_value', type(e).__name__, e)

# === Predicate: is_finite ===
try:
    print('is_finite_normal', Decimal('1.0').is_finite())
    print('is_finite_zero', Decimal('0').is_finite())
    print('is_finite_inf', Decimal('Infinity').is_finite())
    print('is_finite_nan', Decimal('NaN').is_finite())
except Exception as e:
    print('SKIP_Predicate:_is_finite', type(e).__name__, e)

# === Predicate: is_infinite ===
try:
    print('is_inf_normal', Decimal('1.0').is_infinite())
    print('is_inf_inf', Decimal('Infinity').is_infinite())
    print('is_inf_neg_inf', Decimal('-Infinity').is_infinite())
    print('is_inf_nan', Decimal('NaN').is_infinite())
except Exception as e:
    print('SKIP_Predicate:_is_infinite', type(e).__name__, e)

# === Predicate: is_nan ===
try:
    print('is_nan_normal', Decimal('1.0').is_nan())
    print('is_nan_nan', Decimal('NaN').is_nan())
    print('is_nan_neg_nan', Decimal('-NaN').is_nan())
    print('is_nan_snan', Decimal('sNaN').is_nan())
    print('is_nan_inf', Decimal('Infinity').is_nan())
except Exception as e:
    print('SKIP_Predicate:_is_nan', type(e).__name__, e)

# === Predicate: is_zero ===
try:
    print('is_zero_true', Decimal('0').is_zero())
    print('is_zero_decimal', Decimal('0.0').is_zero())
    print('is_zero_negative', Decimal('-0').is_zero())
    print('is_zero_nonzero', Decimal('1.0').is_zero())
except Exception as e:
    print('SKIP_Predicate:_is_zero', type(e).__name__, e)

# === Predicate: is_signed ===
try:
    print('is_signed_positive', Decimal('1.0').is_signed())
    print('is_signed_negative', Decimal('-1.0').is_signed())
    print('is_signed_zero', Decimal('0').is_signed())
    print('is_signed_neg_zero', Decimal('-0').is_signed())
except Exception as e:
    print('SKIP_Predicate:_is_signed', type(e).__name__, e)

# === Exceptions ===
try:
    print('exception_InvalidOperation', issubclass(InvalidOperation, Exception))
    print('exception_DivisionByZero', issubclass(DivisionByZero, Exception))
    print('exception_Overflow', issubclass(Overflow, Exception))
    print('exception_Underflow', issubclass(Underflow, Exception))
    print('exception_Inexact', issubclass(Inexact, Exception))
    print('exception_Rounded', issubclass(Rounded, Exception))
    print('exception_Clammed', issubclass(Clamped, Exception))
    print('exception_DivisionImpossible', issubclass(DivisionImpossible, Exception))
    print('exception_DivisionUndefined', issubclass(DivisionUndefined, Exception))
    print('exception_FloatOperation', issubclass(FloatOperation, Exception))
    print('exception_InvalidContext', issubclass(InvalidContext, Exception))
    print('exception_Subnormal', issubclass(Subnormal, Exception))
except Exception as e:
    print('SKIP_Exceptions', type(e).__name__, e)

# === Edge cases: Very small numbers ===
try:
    print('small_tiny', Decimal('0.00000000000000000001'))
    print('small_exp', Decimal('1E-20'))
    print('small_mul', Decimal('1E-10') * Decimal('1E-10'))
except Exception as e:
    print('SKIP_Edge_cases:_Very_small_numbers', type(e).__name__, e)

# === Edge cases: Very large numbers ===
try:
    print('large_huge', Decimal('999999999999999999999.999'))
    print('large_exp', Decimal('1E+20'))
    print('large_mul', Decimal('1E10') * Decimal('1E10'))
except Exception as e:
    print('SKIP_Edge_cases:_Very_large_numbers', type(e).__name__, e)

# === Edge cases: Zero with different precisions ===
try:
    print('zero_int', Decimal('0'))
    print('zero_one_dec', Decimal('0.0'))
    print('zero_two_dec', Decimal('0.00'))
    print('zero_three_dec', Decimal('0.000'))
except Exception as e:
    print('SKIP_Edge_cases:_Zero_with_different_precisions', type(e).__name__, e)

# === Edge cases: Negative zero ===
try:
    print('neg_zero_str', Decimal('-0'))
    print('neg_zero_decimal', Decimal('-0.0'))
    print('neg_zero_add', Decimal('-0') + Decimal('0'))
    print('neg_zero_mul', Decimal('-1') * Decimal('0'))
except Exception as e:
    print('SKIP_Edge_cases:_Negative_zero', type(e).__name__, e)
