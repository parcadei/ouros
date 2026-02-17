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
