import cmath
from fractions import Fraction


def assert_complex_close(actual, expected, label):
    diff = abs(actual - expected)
    scale = max(1.0, abs(expected))
    assert diff <= 1e-12 * scale, f'{label}: expected {expected!r}, got {actual!r}'


def assert_float_close(actual, expected, label):
    diff = abs(actual - expected)
    scale = max(1.0, abs(expected))
    assert diff <= 1e-12 * scale, f'{label}: expected {expected!r}, got {actual!r}'


def assert_raises(callable_obj, exc_type, message, label):
    try:
        callable_obj()
        assert False, f'{label}: expected {exc_type.__name__}'
    except exc_type as exc:
        assert str(exc) == message, f'{label}: expected {message!r}, got {str(exc)!r}'


# === Public API Surface ===
public_names = [name for name in dir(cmath) if not name.startswith('__')]
assert public_names == [
    'acos',
    'acosh',
    'asin',
    'asinh',
    'atan',
    'atanh',
    'cos',
    'cosh',
    'e',
    'exp',
    'inf',
    'infj',
    'isclose',
    'isfinite',
    'isinf',
    'isnan',
    'log',
    'log10',
    'nan',
    'nanj',
    'phase',
    'pi',
    'polar',
    'rect',
    'sin',
    'sinh',
    'sqrt',
    'tan',
    'tanh',
    'tau',
], f'public api mismatch: {public_names!r}'


# === Constants ===
assert_float_close(cmath.pi, 3.141592653589793, 'pi')
assert_float_close(cmath.e, 2.718281828459045, 'e')
assert_float_close(cmath.tau, 6.283185307179586, 'tau')
assert cmath.isinf(cmath.inf), 'inf should be infinite'
assert cmath.isnan(cmath.nan), 'nan should be nan'
assert cmath.isinf(cmath.infj), 'infj should be infinite'
assert cmath.isnan(cmath.nanj), 'nanj should be nan'


# === Core Transcendentals ===
assert_complex_close(cmath.acos(2), -1.3169578969248166j, 'acos')
assert_complex_close(cmath.acosh(2), 1.3169578969248166 + 0j, 'acosh')
assert_complex_close(cmath.asin(2), 1.5707963267948966 + 1.3169578969248166j, 'asin')
assert_complex_close(cmath.asinh(2), 1.4436354751788103 + 0j, 'asinh')
assert_complex_close(cmath.atan(2), 1.1071487177940904 + 0j, 'atan')
assert_complex_close(cmath.atanh(0.5), 0.5493061443340549 + 0j, 'atanh')
assert_complex_close(cmath.cos(1 + 2j), 2.0327230070196656 - 3.0518977991517997j, 'cos')
assert_complex_close(cmath.cosh(1 + 2j), -0.64214812471552 + 1.0686074213827783j, 'cosh')
assert_complex_close(cmath.exp(1 + 2j), -1.1312043837568135 + 2.4717266720048188j, 'exp')
assert_complex_close(cmath.log(2 + 3j), 1.2824746787307684 + 0.982793723247329j, 'log')
assert_complex_close(cmath.log10(2 + 3j), 0.5569716761534184 + 0.42682189085546657j, 'log10')
assert_complex_close(cmath.sin(1 + 2j), 3.165778513216168 + 1.959601041421606j, 'sin')
assert_complex_close(cmath.sinh(1 + 2j), -0.4890562590412937 + 1.4031192506220405j, 'sinh')
assert_complex_close(cmath.sqrt(1 + 2j), 1.272019649514069 + 0.7861513777574233j, 'sqrt')
assert_complex_close(cmath.tan(1 + 2j), 0.0338128260798967 + 1.0147936161466333j, 'tan')
assert_complex_close(cmath.tanh(1 + 2j), 1.16673625724092 - 0.24345820118572534j, 'tanh')


# === Polar/Phase/Rect ===
assert_float_close(cmath.phase(1 + 1j), 0.7853981633974483, 'phase')
polar_value = cmath.polar(3 + 4j)
assert_float_close(polar_value[0], 5.0, 'polar radius')
assert_float_close(polar_value[1], 0.9272952180016122, 'polar angle')
assert_complex_close(cmath.rect(2.0, 0.5), 1.7551651237807455 + 0.958851077208406j, 'rect basic')


# === Predicate Helpers ===
assert cmath.isfinite(1 + 2j), 'isfinite true branch'
assert not cmath.isfinite(complex(float('inf'), 0.0)), 'isfinite false on inf'
assert cmath.isinf(complex(float('inf'), 0.0)), 'isinf true on inf'
assert not cmath.isinf(1 + 2j), 'isinf false on finite'
assert cmath.isnan(complex(float('nan'), 0.0)), 'isnan true on nan'
assert not cmath.isnan(1 + 2j), 'isnan false on finite'


# === isclose ===
assert cmath.isclose(1 + 2j, 1 + 2.0000000001j), 'isclose default tolerance'
assert not cmath.isclose(0j, 1e-12 + 0j), 'isclose false with default abs tolerance'
assert cmath.isclose(0j, 1e-12 + 0j, abs_tol=1e-9), 'isclose abs_tol'
assert cmath.isclose(a=1 + 0j, b=1 + 0j), 'isclose keyword args for a/b'
assert cmath.isclose(1 + 0j, 1 + 0j, rel_tol=float('nan')), 'isclose accepts nan rel_tol for equal values'
assert cmath.isclose(1 + 0j, 2 + 0j, rel_tol=float('inf')), 'isclose accepts inf rel_tol'

assert_raises(
    lambda: cmath.isclose(),
    TypeError,
    "isclose() missing required argument 'a' (pos 1)",
    'isclose missing a',
)
assert_raises(
    lambda: cmath.isclose(1 + 0j),
    TypeError,
    "isclose() missing required argument 'b' (pos 2)",
    'isclose missing b',
)
assert_raises(
    lambda: cmath.isclose(1 + 0j, 1 + 0j, 1e-9),
    TypeError,
    'isclose() takes exactly 2 positional arguments (3 given)',
    'isclose too many positional',
)
assert_raises(
    lambda: cmath.isclose(1 + 0j, **{'a': 1 + 0j, 'b': 1 + 0j}),
    TypeError,
    "argument for isclose() given by name ('a') and position (1)",
    'isclose a by position and keyword',
)
assert_raises(
    lambda: cmath.isclose(1 + 0j, 1 + 0j, banana=1.0),
    TypeError,
    "isclose() got an unexpected keyword argument 'banana'",
    'isclose unexpected keyword',
)
assert_raises(
    lambda: cmath.isclose(1 + 0j, 1 + 0j, rel_tol=-1.0),
    ValueError,
    'tolerances must be non-negative',
    'isclose rel_tol non-negative',
)


# === Type Coercion ===
assert_complex_close(cmath.sqrt(True), 1 + 0j, 'sqrt(bool)')
assert_complex_close(cmath.sin(Fraction(1, 3)), 0.3271946967961522 + 0j, 'sin(Fraction)')
assert_complex_close(
    cmath.rect(Fraction(1, 2), Fraction(1, 3)),
    0.47247847315736885 + 0.1635973483980761j,
    'rect(Fraction, Fraction)',
)


# === Domain Errors ===
assert_raises(lambda: cmath.log(0), ValueError, 'math domain error', 'log(0)')
assert_raises(lambda: cmath.log10(0), ValueError, 'math domain error', 'log10(0)')
assert_raises(lambda: cmath.log(2, 1), ValueError, 'math domain error', 'log base=1')
assert_raises(lambda: cmath.log(2, 0), ValueError, 'math domain error', 'log base=0')
assert_raises(lambda: cmath.atanh(1), ValueError, 'math domain error', 'atanh(1)')
assert_raises(lambda: cmath.atanh(-1), ValueError, 'math domain error', 'atanh(-1)')
assert_raises(lambda: cmath.rect(1.0, float('inf')), ValueError, 'math domain error', 'rect finite r with inf phi')


# === Domain/Special Values ===
zero_log_base = cmath.log(0, 2)
assert zero_log_base.real == float('-inf'), 'log(0, 2) real should be -inf'
assert cmath.isnan(zero_log_base.imag), 'log(0, 2) imag should be nan'

rect_inf_nan = cmath.rect(float('inf'), float('nan'))
assert cmath.isinf(rect_inf_nan.real), 'rect(inf, nan) real should be inf'
assert cmath.isnan(rect_inf_nan.imag), 'rect(inf, nan) imag should be nan'
assert cmath.rect(0.0, float('inf')) == 0j, 'rect(0, inf) should be 0j'
assert cmath.sqrt(complex(-0.0, -0.0)) == -0j, 'sqrt preserves signed zero branch cut'


# === Keyword-Rejection Errors (non-isclose APIs) ===
assert_raises(
    lambda: cmath.sin(x=1),
    TypeError,
    'cmath.sin() takes no keyword arguments',
    'sin kwargs rejected',
)
assert_raises(
    lambda: cmath.log(z=1),
    TypeError,
    'cmath.log() takes no keyword arguments',
    'log kwargs rejected',
)
assert_raises(
    lambda: cmath.rect(r=1, phi=2),
    TypeError,
    'cmath.rect() takes no keyword arguments',
    'rect kwargs rejected',
)


# === Type Errors ===
assert_raises(
    lambda: cmath.sin('x'),
    TypeError,
    'must be real number, not str',
    'sin type error',
)
assert_raises(
    lambda: cmath.phase('1+2j'),
    TypeError,
    'must be real number, not str',
    'phase type error',
)
assert_raises(
    lambda: cmath.rect(1 + 0j, 0.5),
    TypeError,
    'must be real number, not complex',
    'rect rejects complex radius',
)
assert_raises(
    lambda: cmath.isclose('x', 1 + 0j),
    TypeError,
    'must be real number, not str',
    'isclose type error',
)


# === Argument Count Errors ===
assert_raises(
    lambda: cmath.log(),
    TypeError,
    'log expected at least 1 argument, got 0',
    'log missing arg',
)
assert_raises(
    lambda: cmath.log(1, 2, 3),
    TypeError,
    'log expected at most 2 arguments, got 3',
    'log too many args',
)
assert_raises(
    lambda: cmath.rect(1),
    TypeError,
    'rect expected 2 arguments, got 1',
    'rect missing arg',
)
assert_raises(
    lambda: cmath.sin(),
    TypeError,
    'cmath.sin() takes exactly one argument (0 given)',
    'sin missing arg',
)
