# === Complex literals ===
assert 3 + 4j == complex(3, 4), 'complex literal 3+4j parses and evaluates'
assert 1j == complex(0, 1), 'imaginary-only literal 1j parses and evaluates'

# === complex() constructor ===
assert complex(1, 2) == 1 + 2j, 'complex(real, imag) constructor matches literal'

# === Arithmetic ===
assert (3 + 4j) + (1 - 2j) == 4 + 2j, 'complex addition produces expected value'
assert (3 + 4j) - (1 - 2j) == 2 + 6j, 'complex subtraction produces expected value'
assert (3 + 4j) + 2 == 5 + 4j, 'complex + int produces expected value'
assert 2 + (3 + 4j) == 5 + 4j, 'int + complex produces expected value'
assert (3 + 4j) - 2 == 1 + 4j, 'complex - int produces expected value'
assert 2 - (3 + 4j) == -1 - 4j, 'int - complex produces expected value'
assert (3 + 4j) * (1 - 2j) == 11 - 2j, 'complex multiplication produces expected value'
assert (3 + 4j) * 2 == 6 + 8j, 'complex multiplied by int produces expected value'
assert 2 * (3 + 4j) == 6 + 8j, 'int multiplied by complex produces expected value'
assert (3 + 4j) / (1 - 2j) == -1 + 2j, 'complex division produces expected value'

# === Unary and abs ===
assert -(3 + 4j) == -3 - 4j, 'unary minus negates both complex components'
assert +(3 + 4j) == 3 + 4j, 'unary plus preserves complex value'
assert abs(3 + 4j) == 5.0, 'abs(complex) returns Euclidean magnitude as float'
assert (3 + 4j).conjugate() == 3 - 4j, 'complex.conjugate() returns conjugated value'

# === Equality with real scalars ===
assert (1 + 0j) == 1, 'complex with zero imaginary part equals int'
assert 1 == (1 + 0j), 'int equals complex with zero imaginary part'

# === Power ===
pow_cc = (1 + 1j) ** (2 + 3j)
assert abs(pow_cc.real - (-0.16345093210735503)) < 1e-12, 'complex ** complex real part matches CPython'
assert abs(pow_cc.imag - 0.09600498360894892) < 1e-12, 'complex ** complex imag part matches CPython'

pow_sc = 2 ** (1 + 1j)
assert abs(pow_sc.real - 1.5384778027279442) < 1e-12, 'real ** complex real part matches CPython'
assert abs(pow_sc.imag - 1.2779225526272695) < 1e-12, 'real ** complex imag part matches CPython'

assert (1 + 1j) ** 2 == 2j, 'complex ** int matches CPython'
pow_neg_frac = (-2) ** 0.5
assert abs(pow_neg_frac.real - 8.659560562354934e-17) < 1e-12, 'negative real ** fractional real part matches CPython'
assert abs(pow_neg_frac.imag - 1.4142135623730951) < 1e-12, 'negative real ** fractional imag part matches CPython'

# === Real and imaginary parts ===
z = 3 + 4j
assert z.real == 3.0, 'complex.real returns float real part'
assert z.imag == 4.0, 'complex.imag returns float imaginary part'
assert complex(1, 2).real == 1.0, 'constructor result has expected .real value'
assert complex(1, 2).imag == 2.0, 'constructor result has expected .imag value'

# === Error behavior ===
try:
    _ = 0j ** (-1 + 0j)
    assert False, '0j ** (-1+0j) should raise ZeroDivisionError'
except ZeroDivisionError as e:
    assert str(e) == 'zero to a negative or complex power', 'zero-complex negative power error message'

try:
    _ = 0j ** (1j)
    assert False, '0j ** (1j) should raise ZeroDivisionError'
except ZeroDivisionError as e:
    assert str(e) == 'zero to a negative or complex power', 'zero-complex imaginary power error message'

try:
    _ = pow(1 + 1j, 2, 3)
    assert False, 'pow(complex, complex/int, mod) should raise ValueError'
except ValueError as e:
    assert str(e) == 'complex modulo', 'pow complex modulo error message'
