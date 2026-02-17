# === Decimal construction from string ===
import decimal
from decimal import Decimal

# Basic construction
assert Decimal('3.14') == Decimal('3.14'), 'Decimal string construction'
assert Decimal('-5.5') == Decimal('-5.5'), 'Decimal negative'

# === Decimal construction from int ===
assert Decimal(42) == Decimal('42'), 'Decimal from int'
assert Decimal(0) == Decimal('0'), 'Decimal from zero'

# === Arithmetic operations ===
# Addition
assert Decimal('3.14') + Decimal('2.86') == Decimal('6.00'), 'Decimal add'

# Subtraction
assert Decimal('5.5') - Decimal('2.2') == Decimal('3.3'), 'Decimal sub'

# Multiplication
assert Decimal('2.5') * Decimal('4') == Decimal('10.0'), 'Decimal mul'

# Floor division
assert Decimal('10') // Decimal('3') == Decimal('3'), 'Decimal floor div'

# Modulo
assert Decimal('10') % Decimal('3') == Decimal('1'), 'Decimal mod'

# Power
assert Decimal('2') ** Decimal('3') == Decimal('8'), 'Decimal pow'

# === Comparisons ===
assert Decimal('3.14') < Decimal('3.15'), 'Decimal less than'
assert Decimal('3.15') > Decimal('3.14'), 'Decimal greater than'

# === Methods ===
# quantize
assert Decimal('3.14159').quantize(Decimal('0.01')) == Decimal('3.14'), 'Decimal quantize'
assert Decimal('19.995').quantize(Decimal('0.01'), rounding=decimal.ROUND_HALF_UP) == Decimal(
    '20.00'
), 'Decimal quantize ROUND_HALF_UP'
assert Decimal('19.995').quantize(Decimal('0.01'), rounding=decimal.ROUND_DOWN) == Decimal(
    '19.99'
), 'Decimal quantize ROUND_DOWN'
assert Decimal('2.45').quantize(Decimal('0.1'), rounding=decimal.ROUND_HALF_EVEN) == Decimal(
    '2.4'
), 'Decimal quantize ROUND_HALF_EVEN tie'
assert Decimal('2.51').quantize(Decimal('0.1'), rounding=decimal.ROUND_UP) == Decimal(
    '2.6'
), 'Decimal quantize ROUND_UP'
assert Decimal('-2.51').quantize(Decimal('0.1'), rounding=decimal.ROUND_FLOOR) == Decimal(
    '-2.6'
), 'Decimal quantize ROUND_FLOOR'
assert Decimal('2.51').quantize(Decimal('0.1'), rounding=decimal.ROUND_CEILING) == Decimal(
    '2.6'
), 'Decimal quantize ROUND_CEILING'
assert Decimal('2.55').quantize(Decimal('0.1'), rounding=decimal.ROUND_HALF_DOWN) == Decimal(
    '2.5'
), 'Decimal quantize ROUND_HALF_DOWN tie'
assert Decimal('1.04').quantize(Decimal('0.1'), rounding=decimal.ROUND_05UP) == Decimal(
    '1.1'
), 'Decimal quantize ROUND_05UP'
context = decimal.Context(prec=28, rounding=decimal.ROUND_DOWN)
assert Decimal('1.29').quantize(Decimal('0.1'), context=context) == Decimal(
    '1.2'
), 'Decimal quantize context rounding'
assert Decimal('1.29').quantize(Decimal('0.1'), rounding=decimal.ROUND_UP, context=context) == Decimal(
    '1.3'
), 'Decimal quantize explicit rounding overrides context'

# copy_abs
assert Decimal('-5').copy_abs() == Decimal('5'), 'Decimal copy_abs'

# copy_negate
assert Decimal('5').copy_negate() == Decimal('-5'), 'Decimal copy_negate'

# copy_sign
assert Decimal('5').copy_sign(Decimal('-1')) == Decimal('-5'), 'Decimal copy_sign'

# === Predicates ===
assert Decimal('3.14').is_finite(), 'Decimal is_finite'
assert Decimal('0').is_zero(), 'Decimal is_zero'
assert not Decimal('5').is_signed(), 'Decimal not is_signed'
assert Decimal('-5').is_signed(), 'Decimal is_signed'

# === String representation ===
assert repr(Decimal('3.14')) == "Decimal('3.14')", 'Decimal repr'
assert str(Decimal('3.14')) == '3.14', 'Decimal str'

# === Module attributes expected by stdlib users ===
assert decimal.DecimalTuple is not None, 'decimal.DecimalTuple should be exported'
assert decimal.MAX_PREC == 999_999_999_999_999_999, 'decimal.MAX_PREC constant'
assert decimal.DecimalException is not None, 'decimal.DecimalException should be exported'
assert decimal.Context.__name__ == 'Context', 'decimal.Context.__name__'
assert type(decimal.getcontext()).__name__ == 'Context', 'type(decimal.getcontext()).__name__'
assert type(Decimal('1')).__name__ == 'Decimal', 'type(Decimal(...)).__name__'

# === int(Decimal) conversion semantics ===
assert int(Decimal('42.9')) == 42, 'int(Decimal) truncates positive toward zero'
assert int(Decimal('-42.9')) == -42, 'int(Decimal) truncates negative toward zero'

try:
    int(Decimal('NaN'))
    assert False, 'int(Decimal("NaN")) should raise ValueError'
except ValueError as exc:
    assert exc.args[0] == 'cannot convert NaN to integer', f'NaN conversion error message: {exc.args}'

try:
    int(Decimal('Infinity'))
    assert False, 'int(Decimal("Infinity")) should raise OverflowError'
except OverflowError as exc:
    assert exc.args[0] == 'cannot convert Infinity to integer', f'Infinity conversion error message: {exc.args}'

try:
    int(Decimal('-Infinity'))
    assert False, 'int(Decimal("-Infinity")) should raise OverflowError'
except OverflowError as exc:
    assert exc.args[0] == 'cannot convert Infinity to integer', f'-Infinity conversion error message: {exc.args}'

# === float(Decimal) conversion semantics ===
assert float(Decimal('3.14')) == 3.14, 'float(Decimal) finite conversion'
assert float(Decimal('Infinity')) == float('inf'), 'float(Decimal("Infinity"))'
assert float(Decimal('-Infinity')) == float('-inf'), 'float(Decimal("-Infinity"))'
assert str(float(Decimal('NaN'))) == 'nan', 'float(Decimal("NaN")) produces nan'

try:
    float(Decimal('sNaN'))
    assert False, 'float(Decimal("sNaN")) should raise ValueError'
except ValueError as exc:
    assert exc.args[0] == 'cannot convert signaling NaN to float', f'sNaN conversion error message: {exc.args}'
