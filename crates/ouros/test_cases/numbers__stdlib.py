# === Module Import ===
import numbers


# === Public API Surface ===
public_names = sorted(name for name in dir(numbers) if not name.startswith('_'))
assert public_names == [
    'ABCMeta',
    'Complex',
    'Integral',
    'Number',
    'Rational',
    'Real',
    'abstractmethod',
], f'public API mismatch: {public_names!r}'

assert numbers.__all__ == [
    'Number',
    'Complex',
    'Real',
    'Rational',
    'Integral',
], f'__all__ mismatch: {numbers.__all__!r}'


# === Core Exports ===
for class_name in ['Number', 'Complex', 'Real', 'Rational', 'Integral']:
    cls = getattr(numbers, class_name)
    assert cls.__module__ == 'numbers', f'{class_name}.__module__ should be numbers'
    assert cls.__name__ == class_name, f'{class_name}.__name__ should match export name'

assert numbers.ABCMeta.__name__ == 'ABCMeta', 'ABCMeta export should be named ABCMeta'


def probe():
    return 42


decorated = numbers.abstractmethod(probe)
assert decorated is probe, 'numbers.abstractmethod should return the input callable'


# === Class Hierarchy ===
assert numbers.Complex.__bases__ == (numbers.Number,), 'Complex base should be Number'
assert numbers.Real.__bases__ == (numbers.Complex,), 'Real base should be Complex'
assert numbers.Rational.__bases__ == (numbers.Real,), 'Rational base should be Real'
assert numbers.Integral.__bases__ == (numbers.Rational,), 'Integral base should be Rational'


# === Abstract Method Sets ===
assert sorted(numbers.Number.__abstractmethods__) == [], 'Number should have no abstract methods'
assert sorted(numbers.Complex.__abstractmethods__) == [
    '__abs__',
    '__add__',
    '__complex__',
    '__eq__',
    '__mul__',
    '__neg__',
    '__pos__',
    '__pow__',
    '__radd__',
    '__rmul__',
    '__rpow__',
    '__rtruediv__',
    '__truediv__',
    'conjugate',
    'imag',
    'real',
], 'Complex abstract methods mismatch'
assert sorted(numbers.Real.__abstractmethods__) == [
    '__abs__',
    '__add__',
    '__ceil__',
    '__eq__',
    '__float__',
    '__floor__',
    '__floordiv__',
    '__le__',
    '__lt__',
    '__mod__',
    '__mul__',
    '__neg__',
    '__pos__',
    '__pow__',
    '__radd__',
    '__rfloordiv__',
    '__rmod__',
    '__rmul__',
    '__round__',
    '__rpow__',
    '__rtruediv__',
    '__truediv__',
    '__trunc__',
], 'Real abstract methods mismatch'
assert sorted(numbers.Rational.__abstractmethods__) == [
    '__abs__',
    '__add__',
    '__ceil__',
    '__eq__',
    '__floor__',
    '__floordiv__',
    '__le__',
    '__lt__',
    '__mod__',
    '__mul__',
    '__neg__',
    '__pos__',
    '__pow__',
    '__radd__',
    '__rfloordiv__',
    '__rmod__',
    '__rmul__',
    '__round__',
    '__rpow__',
    '__rtruediv__',
    '__truediv__',
    '__trunc__',
    'denominator',
    'numerator',
], 'Rational abstract methods mismatch'
assert sorted(numbers.Integral.__abstractmethods__) == [
    '__abs__',
    '__add__',
    '__and__',
    '__ceil__',
    '__eq__',
    '__floor__',
    '__floordiv__',
    '__int__',
    '__invert__',
    '__le__',
    '__lshift__',
    '__lt__',
    '__mod__',
    '__mul__',
    '__neg__',
    '__or__',
    '__pos__',
    '__pow__',
    '__radd__',
    '__rand__',
    '__rfloordiv__',
    '__rlshift__',
    '__rmod__',
    '__rmul__',
    '__ror__',
    '__round__',
    '__rpow__',
    '__rrshift__',
    '__rshift__',
    '__rtruediv__',
    '__rxor__',
    '__truediv__',
    '__trunc__',
    '__xor__',
], 'Integral abstract methods mismatch'


# === Virtual Subclass Registration ===
assert issubclass(complex, numbers.Complex), 'complex should be a virtual subclass of Complex'
assert issubclass(float, numbers.Real), 'float should be a virtual subclass of Real'
assert issubclass(int, numbers.Integral), 'int should be a virtual subclass of Integral'
assert issubclass(bool, numbers.Integral), 'bool should be a virtual subclass of Integral'
assert issubclass(int, numbers.Number), 'int should be a virtual subclass of Number'
assert issubclass(type(1), numbers.Number), 'type(1) should satisfy Number'
assert issubclass(type(1.0), numbers.Real), 'type(1.0) should satisfy Real'
assert issubclass(type(1 + 2j), numbers.Complex), 'type(1+2j) should satisfy Complex'


# === Instantiation Semantics ===
number_instance = numbers.Number()
assert isinstance(number_instance, numbers.Number), 'Number should be instantiable'
assert numbers.Number.__hash__ is None, 'Number.__hash__ should be None'

for abstract_cls in [numbers.Complex, numbers.Real, numbers.Rational, numbers.Integral]:
    try:
        abstract_cls()
        raise AssertionError(f'{abstract_cls.__name__} should not be instantiable')
    except TypeError:
        pass


# === Concrete Mixin Helpers ===
assert numbers.Complex.__bool__(0j) is False, 'Complex.__bool__ false case'
assert numbers.Complex.__bool__(1 + 0j) is True, 'Complex.__bool__ true case'
assert numbers.Complex.__sub__(1 + 2j, 3) == (-2 + 2j), 'Complex.__sub__ mixin behavior'
assert numbers.Complex.__rsub__(1 + 2j, 3) == (2 - 2j), 'Complex.__rsub__ mixin behavior'

assert numbers.Real.conjugate(3.5) == 3.5, 'Real.conjugate mixin'
assert numbers.Real.__complex__(3.5) == (3.5 + 0j), 'Real.__complex__ mixin'
assert numbers.Real.__divmod__(7.5, 2.0) == divmod(7.5, 2.0), 'Real.__divmod__ mixin'
assert numbers.Real.__rdivmod__(7.5, 2.0) == divmod(2.0, 7.5), 'Real.__rdivmod__ mixin'

assert numbers.Integral.__index__(5) == 5, 'Integral.__index__ mixin'
assert numbers.Integral.__float__(5) == 5.0, 'Integral.__float__ mixin'
