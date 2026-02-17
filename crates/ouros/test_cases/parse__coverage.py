# === Delete subscript ===
# Exercises lines 380-390 in parse.rs
d = {'a': 1, 'b': 2}
del d['a']
assert d == {'b': 2}, 'del subscript removes key'

# === Delete attribute ===
# Exercises lines 368-376 in parse.rs


class Obj:
    pass


o = Obj()
o.attr = 'hello'
del o.attr
try:
    _ = o.attr
    assert False, 'should raise AttributeError after del attr'
except AttributeError:
    pass

# === Augmented assignment to attribute ===
# Exercises lines 445-452 in parse.rs


class Box:
    def __init__(self, v):
        self.v = v


b = Box(10)
b.v += 5
assert b.v == 15, 'augmented assign to attribute +='

b.v *= 2
assert b.v == 30, 'augmented assign to attribute *='

# === Augmented assignment to subscript ===
# Exercises lines 455-465 in parse.rs
data = [10, 20, 30]
data[1] += 100
assert data[1] == 120, 'augmented assign to subscript +='

dd = {'x': 5}
dd['x'] -= 3
assert dd['x'] == 2, 'augmented assign to dict subscript -='

# === Annotated assignment with value ===
# Exercises line 472-474 in parse.rs
x: int = 42
assert x == 42, 'annotated assignment with value'

# === Annotated assignment without value (Pass) ===
# Exercises line 474 in parse.rs
y: int  # This should compile to a Pass node

# === For loop with else ===
# Exercises lines 491-495 in parse.rs
result = []
for i in range(3):
    result.append(i)
else:
    result.append('done')
assert result == [0, 1, 2, 'done'], 'for loop with else clause'

# === With statement ===
# Exercises lines 533-557 in parse.rs


class SimpleCtx:
    def __enter__(self):
        return 'entered'

    def __exit__(self, *args):
        return False


with SimpleCtx() as val:
    assert val == 'entered', 'with statement as clause'

# With statement without as clause
with SimpleCtx():
    pass

# === Raise without argument (bare raise) ===
# Exercises line 563-569 in parse.rs
try:
    try:
        raise ValueError('inner')
    except ValueError:
        raise
except ValueError as e:
    assert str(e) == 'inner', 'bare raise re-raises'

# === Try/except with else and finally ===
# Exercises lines 571-598 in parse.rs
result = []
try:
    result.append('try')
except ValueError:
    result.append('except')
else:
    result.append('else')
finally:
    result.append('finally')
assert result == ['try', 'else', 'finally'], 'try/except/else/finally'

# === Try/except with named exception ===
try:
    raise TypeError('test msg')
except TypeError as e:
    assert str(e) == 'test msg', 'named exception variable'

# === Assert without message ===
# Exercises lines 601-607 in parse.rs
assert True

# === Assert with message ===
assert 1 == 1, 'assert with message works'

# === Import statement ===
# Exercises lines 609-626 in parse.rs
import sys

assert sys.platform is not None, 'import sys works'

# === From import ===
# Exercises lines 628-700 in parse.rs
from sys import version_info

assert version_info is not None, 'from import works'

# === String literal ===
# Exercises lines 1116-1122 in parse.rs
s = 'hello world'
assert s == 'hello world', 'string literal'

# === Bytes literal ===
# Exercises lines 1123-1130 in parse.rs
bb = b'hello'
assert bb == b'hello', 'bytes literal'

# === Integer literal ===
# Exercises lines 1131-1149 in parse.rs
assert 42 == 42, 'integer literal'
assert -5 == -5, 'negative integer literal'

# === Float literal ===
assert 3.14 == 3.14, 'float literal'

# === Large integer literal (BigInt) ===
# Exercises line 1140-1143 in parse.rs
big = 99999999999999999999999999999999
assert big > 0, 'big int literal'

# === Hex literal (large) ===
# Exercises lines 1917-1921 in parse.rs (parse_int_literal)
big_hex = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFF
assert big_hex > 0, 'big hex literal'

# === Octal literal (large) ===
big_oct = 0o77777777777777777777777777777
assert big_oct > 0, 'big octal literal'

# === Binary literal (large) ===
big_bin = 0b1111111111111111111111111111111111111111111111111111111111111111111111
assert big_bin > 0, 'big binary literal'

# === Subscript expression ===
# Exercises lines 1173-1181 in parse.rs
items = [10, 20, 30]
assert items[0] == 10, 'subscript expression'
assert items[-1] == 30, 'negative subscript'

# === Slice expression ===
# Exercises lines 1220-1235 in parse.rs
sdata = [0, 1, 2, 3, 4, 5]
assert sdata[1:4] == [1, 2, 3], 'basic slice'
assert sdata[::2] == [0, 2, 4], 'slice with step'
assert sdata[::-1] == [5, 4, 3, 2, 1, 0], 'reverse slice'
assert sdata[1:5:2] == [1, 3], 'slice with all three'

# === Comparison operators ===
assert 1 < 2, 'less than'
assert 2 > 1, 'greater than'
assert 1 <= 1, 'less equal'
assert 2 >= 2, 'greater equal'
assert 1 == 1, 'equal'
assert 1 != 2, 'not equal'
assert 1 in [1, 2, 3], 'in operator'
assert 4 not in [1, 2, 3], 'not in operator'

# === Chain comparison ===
# Exercises lines 1296-1330 in parse.rs
assert 1 < 2 < 3, 'chain comparison ascending'
assert 1 < 2 < 3 < 4, 'chain comparison four elements'
assert 3 > 2 > 1, 'chain comparison descending'
assert 1 <= 1 <= 2, 'chain comparison with equality'

# === Tuple unpacking in assignment ===
# Exercises lines 1339-1356 in parse.rs
a, b = 1, 2
assert a == 1, 'tuple unpack first'
assert b == 2, 'tuple unpack second'

a, b, c = [10, 20, 30]
assert (a, b, c) == (10, 20, 30), 'list unpack to three vars'

# === Starred unpack target ===
# Exercises lines 1358-1369 in parse.rs
first, *rest = [1, 2, 3, 4]
assert first == 1, 'starred unpack first'
assert rest == [2, 3, 4], 'starred unpack rest'

*head, last = [10, 20, 30]
assert head == [10, 20], 'starred unpack head'
assert last == 30, 'starred unpack last'

# === List unpacking target ===
# Exercises lines 1370-1388 in parse.rs
[a, b, c] = [7, 8, 9]
assert (a, b, c) == (7, 8, 9), 'list unpack target'

# === Nested tuple unpacking ===
(a, (b, c)) = (1, (2, 3))
assert (a, b, c) == (1, 2, 3), 'nested tuple unpack'

# === Function keyword arguments ===
# Exercises lines 1247-1270 in parse.rs


def func_kwargs(x, y=10, z=20):
    return x + y + z


assert func_kwargs(1) == 31, 'kwargs default values'
assert func_kwargs(1, y=2) == 23, 'kwargs named arg'
assert func_kwargs(1, y=2, z=3) == 6, 'kwargs all named'

# === F-string with literal part ===
# Exercises lines 1503-1509 in parse.rs
name = 'world'
greeting = f'hello {name}!'
assert greeting == 'hello world!', 'f-string with literal prefix and suffix'

# === F-string concatenated with regular string ===
prefix = 'hi'
msg = f'{prefix} there'
assert msg == 'hi there', 'f-string concat with regular string'

# === Comprehension with tuple unpacking ===
pairs = [(1, 'a'), (2, 'b')]
assert [v for k, v in pairs] == ['a', 'b'], 'comp with tuple unpack'

# === Dict comprehension ===
assert {k: v for k, v in pairs} == {1: 'a', 2: 'b'}, 'dict comprehension'

# === Set literal ===
# Exercises line 945-947 in parse.rs
s = {1, 2, 3}
assert len(s) == 3, 'set literal'
assert 2 in s, 'set contains element'

# === Set comprehension ===
assert {x * 2 for x in [1, 2, 3]} == {2, 4, 6}, 'set comprehension'

# === Unary operators ===
assert -5 == -5, 'unary minus'
assert +5 == 5, 'unary plus'
assert ~0 == -1, 'unary invert'

# === Ternary (if-else expression) ===
x = 5
assert (x if x > 0 else -x) == 5, 'ternary true branch'
x = -3
assert (x if x > 0 else -x) == 3, 'ternary false branch'

# === Lambda expression ===
f = lambda x: x + 1
assert f(5) == 6, 'lambda expression'

# === Lambda with defaults ===
g = lambda x, y=10: x + y
assert g(5) == 15, 'lambda with default'

# === Attribute access ===
# Exercises lines 1150-1172 in parse.rs


class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y


p = Point(3, 4)
assert p.x == 3, 'attribute access x'
assert p.y == 4, 'attribute access y'

# === Method call ===
items2 = [3, 1, 2]
items2.sort()
assert items2 == [1, 2, 3], 'method call sort'

# === Attribute assignment ===
p.x = 100
assert p.x == 100, 'attribute assignment'

# === Subscript assignment ===
ddd = {'a': 1}
ddd['b'] = 2
assert ddd == {'a': 1, 'b': 2}, 'subscript assignment'

# === Name mangling inside class ===
# Exercises lines 1429, 1431-1441 in parse.rs


class Mangled:
    def __init__(self):
        self.__private = 42

    def get_private(self):
        return self.__private


m = Mangled()
assert m.get_private() == 42, 'name mangling access via method'
assert m._Mangled__private == 42, 'name mangling direct access'

# === Class with dunder (not mangled) ===
# Exercises lines 1898-1899 in parse.rs (is_mangling_candidate)


class DunderTest:
    def __init__(self):
        self.__init_val__ = 'not mangled'


dt = DunderTest()
assert dt.__init_val__ == 'not mangled', 'dunder names not mangled'

# === Mangling: class name with leading underscores ===


class __LeadingUnderscores:
    def __init__(self):
        self.__private = 99

    def get(self):
        return self.__private


lu = __LeadingUnderscores()
assert lu.get() == 99, 'mangling with leading underscore class name'

# === Walrus operator (named expression) ===
if (n := 10) > 5:
    assert n == 10, 'walrus in if condition'

# === Multiple comprehension generators ===
result = [(x, y) for x in range(2) for y in range(2)]
assert result == [(0, 0), (0, 1), (1, 0), (1, 1)], 'multiple generators'

# === Comprehension with multiple filters ===
result = [x for x in range(20) if x % 2 == 0 if x % 3 == 0]
assert result == [0, 6, 12, 18], 'comprehension with multiple if filters'

# === Keyword-only arguments ===


def kw_only(*, x, y=10):
    return x + y


assert kw_only(x=5) == 15, 'keyword only args'
assert kw_only(x=5, y=20) == 25, 'keyword only args override'

# === Positional-only arguments ===


def pos_only(a, b, /):
    return a + b


assert pos_only(1, 2) == 3, 'positional only args'

# === *args and **kwargs ===


def varargs(*args, **kwargs):
    return (args, kwargs)


assert varargs(1, 2, a=3) == ((1, 2), {'a': 3}), 'varargs and kwargs'

# === Function decorators (staticmethod) ===


class WithDecorator:
    @staticmethod
    def static_fn(x):
        return x + 1


assert WithDecorator.static_fn(5) == 6, 'staticmethod decorator'

# === Boolean operator (and / or) ===
assert (1 and 2) == 2, 'and returns last truthy'
assert (0 or 5) == 5, 'or returns first truthy'
assert (0 and 5) == 0, 'and short-circuits on falsy'
assert (1 or 5) == 1, 'or short-circuits on truthy'

# === Class inheritance ===


class Base:
    def method(self):
        return 'base'


class Child(Base):
    def method(self):
        return 'child'


cc = Child()
assert cc.method() == 'child', 'class inheritance'

# === Exponent operator ===
assert 2**10 == 1024, 'exponent operator'
assert 3**3 == 27, 'exponent operator'

# === Modulo operator ===
assert 10 % 3 == 1, 'modulo operator'

# === Floor division ===
assert 7 // 2 == 3, 'floor division'
assert -7 // 2 == -4, 'negative floor division'

# === Bitwise operators ===
assert (0b1100 & 0b1010) == 0b1000, 'bitwise and'
assert (0b1100 | 0b1010) == 0b1110, 'bitwise or'
assert (0b1100 ^ 0b1010) == 0b0110, 'bitwise xor'

# === Shift operators ===
assert (1 << 3) == 8, 'left shift'
assert (16 >> 2) == 4, 'right shift'

# === While loop with else ===
i = 0
while i < 3:
    i += 1
else:
    result = 'completed'
assert result == 'completed', 'while with else'

# === Break in while loop (else not executed) ===
result = 'not set'
i = 0
while i < 10:
    if i == 3:
        result = 'broke'
        break
    i += 1
else:
    result = 'completed'
assert result == 'broke', 'while break skips else'

# === Continue in for loop ===
result = []
for i in range(5):
    if i % 2 == 0:
        continue
    result.append(i)
assert result == [1, 3], 'continue skips even numbers'

# === Nested class ===


class Outer:
    class Inner:
        value = 42


assert Outer.Inner.value == 42, 'nested class access'

# === Empty dict literal ===
assert {} == {}, 'empty dict literal'
assert len({}) == 0, 'empty dict length'

# === Dict with multiple items ===
d = {'a': 1, 'b': 2, 'c': 3}
assert len(d) == 3, 'dict with multiple items'

# === Tuple literal ===
t = (1, 2, 3)
assert t == (1, 2, 3), 'tuple literal'
assert len(t) == 3, 'tuple length'

# === Empty list ===
assert [] == [], 'empty list literal'

# === Global statement ===
# Exercises lines 599-611 in parse.rs
g = 0


def set_global():
    global g
    g = 42


set_global()
assert g == 42, 'global statement'

# === Nonlocal statement ===
# Exercises lines 612-655 in parse.rs


def test_nonlocal():
    x = 10

    def inner():
        nonlocal x
        x = 20

    inner()
    return x


assert test_nonlocal() == 20, 'nonlocal statement'

# === Type parameters (PEP 695) ===
# Lines 1289-1293 in parse.rs require Python 3.12+ syntax (class Foo[T]:)
# which ruff rejects when target is Python 3.10. Skipped to keep lint clean.

# === **kwargs unpacking in function call ===
# Exercises parse_keywords (lines 1247-1270 in parse.rs)


def take_kw(a=0, b=0):
    return a + b


kwargs_dict = {'a': 10, 'b': 20}
assert take_kw(**kwargs_dict) == 30, 'kwargs dict unpacking'

# === Dict literal unpacking ===
# Exercises dict literal parsing for `**mapping` entries.
base = {'x': 1, 'y': 2}
merged = {**base}
assert merged == {'x': 1, 'y': 2}, 'dict literal unpack basic'

mixed = {'a': 1, **{'b': 2}, 'a': 3}
assert mixed == {'a': 3, 'b': 2}, 'dict literal unpack keeps overwrite order'

# === *args unpacking in function call ===
# Exercises lines 1046-1051 in parse.rs


def add_three(a, b, c):
    return a + b + c


args_list = [1, 2, 3]
assert add_three(*args_list) == 6, 'args list unpacking'

# === Starred expression in assignment (star unpack) ===
a, *middle, z = [1, 2, 3, 4, 5]
assert a == 1, 'star unpack first'
assert middle == [2, 3, 4], 'star unpack middle'
assert z == 5, 'star unpack last'

# === List unpack target with star ===
[a, *rest] = [10, 20, 30]
assert a == 10, 'list unpack star first'
assert rest == [20, 30], 'list unpack star rest'

# === Nested format spec in f-string ===
# Exercises lines 1503-1619 in parse.rs
w = 10
assert f'{"hi":{w}}' == 'hi        ', 'f-string nested format spec'

# === F-string with conversion flag ===
assert f'{42!r}' == '42', 'f-string repr conversion'
assert f'{"hello"!s}' == 'hello', 'f-string str conversion'

# === Multiple f-string interpolations ===
a = 1
b = 2
assert f'{a} + {b} = {a + b}' == '1 + 2 = 3', 'f-string multiple interpolations'

# === Walrus in comprehension ===
result = [(x := i) for i in range(3)]
assert result == [0, 1, 2], 'walrus in comp element'
assert x == 2, 'walrus leaks from comprehension'

# === Dict items in comprehension ===
d = {k: v for k, v in [(1, 2), (3, 4)]}
assert d == {1: 2, 3: 4}, 'dict comp from pairs'

# === Complex class body ===


class Complex:
    class_var = 'hello'

    def __init__(self, x):
        self.x = x

    def method(self):
        return self.x + 1

    @staticmethod
    def static_method():
        return 42


cobj = Complex(10)
assert cobj.method() == 11, 'complex class method'
assert Complex.static_method() == 42, 'static method'
assert Complex.class_var == 'hello', 'class variable'
