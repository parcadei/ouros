import operator

# === arithmetic ===
assert operator.add(1, 2) == 3, 'add int'
assert operator.add(1.5, 2.5) == 4.0, 'add float'
assert operator.add('hello', ' world') == 'hello world', 'add str'
assert operator.sub(10, 3) == 7, 'sub int'
assert operator.mul(3, 4) == 12, 'mul int'
assert operator.mul('ab', 3) == 'ababab', 'mul str'
assert operator.truediv(10, 4) == 2.5, 'truediv'
assert operator.floordiv(10, 3) == 3, 'floordiv'
assert operator.mod(10, 3) == 1, 'mod'

# === pow ===
assert operator.pow(2, 3) == 8, 'pow int'
assert operator.pow(2, 0) == 1, 'pow zero'
assert operator.pow(5, 2) == 25, 'pow square'

# === unary ===
assert operator.neg(5) == -5, 'neg int'
assert operator.neg(-3.5) == 3.5, 'neg float'
assert operator.abs(-5) == 5, 'abs int'
assert operator.abs(5) == 5, 'abs positive'
assert operator.abs(-3.5) == 3.5, 'abs float'

# === pos ===
assert operator.pos(5) == 5, 'pos int'
assert operator.pos(-3) == -3, 'pos negative int'
assert operator.pos(3.5) == 3.5, 'pos float'
assert operator.pos(True) == 1, 'pos bool true'
assert operator.pos(False) == 0, 'pos bool false'

# === comparison ===
assert operator.eq(1, 1) is True, 'eq true'
assert operator.eq(1, 2) is False, 'eq false'
assert operator.ne(1, 2) is True, 'ne true'
assert operator.lt(1, 2) is True, 'lt true'
assert operator.lt(2, 1) is False, 'lt false'
assert operator.le(1, 1) is True, 'le equal'
assert operator.le(1, 2) is True, 'le less'
assert operator.gt(2, 1) is True, 'gt true'
assert operator.ge(1, 1) is True, 'ge equal'

# === boolean ===
assert operator.not_(True) is False, 'not_ true'
assert operator.not_(False) is True, 'not_ false'
assert operator.not_(0) is True, 'not_ zero'
assert operator.truth(1) is True, 'truth 1'
assert operator.truth(0) is False, 'truth 0'
assert operator.truth('hello') is True, 'truth str'
assert operator.truth('') is False, 'truth empty str'

# === bitwise ===
assert operator.and_(0b1100, 0b1010) == 0b1000, 'and_'
assert operator.or_(0b1100, 0b1010) == 0b1110, 'or_'
assert operator.xor(0b1100, 0b1010) == 0b0110, 'xor'
assert operator.invert(0) == -1, 'invert 0'
assert operator.invert(1) == -2, 'invert 1'
assert operator.invert(-1) == 0, 'invert -1'
assert operator.invert(True) == -2, 'invert bool'
assert operator.lshift(1, 4) == 16, 'lshift'
assert operator.rshift(16, 4) == 1, 'rshift'

# === index ===
assert operator.index(5) == 5, 'index int'

# === getitem ===
assert operator.getitem([1, 2, 3], 1) == 2, 'getitem list'
assert operator.getitem({'a': 1}, 'a') == 1, 'getitem dict'

# === contains ===
assert operator.contains([1, 2, 3], 2) is True, 'contains list true'
assert operator.contains([1, 2, 3], 5) is False, 'contains list false'
assert operator.contains('hello', 'ell') is True, 'contains str true'

# === concat ===
assert operator.concat([1, 2], [3, 4]) == [1, 2, 3, 4], 'concat lists'
assert operator.concat('foo', 'bar') == 'foobar', 'concat strings'

# === countOf ===
assert operator.countOf([1, 2, 2, 3, 2], 2) == 3, 'countOf list'
assert operator.countOf([1, 2, 3], 5) == 0, 'countOf list not found'
assert operator.countOf([], 1) == 0, 'countOf empty list'
assert operator.countOf((1, 2, 2, 3), 2) == 2, 'countOf tuple'

# === indexOf ===
assert operator.indexOf([10, 20, 30], 20) == 1, 'indexOf list'
assert operator.indexOf([10, 20, 30], 10) == 0, 'indexOf list first'
assert operator.indexOf((5, 10, 15), 15) == 2, 'indexOf tuple'

# === in-place operations ===
assert operator.iadd(3, 4) == 7, 'iadd int'
assert operator.iadd('foo', 'bar') == 'foobar', 'iadd str'
assert operator.isub(10, 3) == 7, 'isub int'
assert operator.imul(3, 4) == 12, 'imul int'
assert operator.itruediv(10, 4) == 2.5, 'itruediv'
assert operator.ifloordiv(10, 3) == 3, 'ifloordiv'
assert operator.imod(10, 3) == 1, 'imod'

# === iadd list (in-place extends) ===
x = [1, 2]
result = operator.iadd(x, [3, 4])
assert result == [1, 2, 3, 4], 'iadd list extends'
assert x == [1, 2, 3, 4], 'iadd list modifies in-place'

# === from import ===
from operator import add, eq

assert add(10, 20) == 30, 'from import add'
assert eq(5, 5) is True, 'from import eq'
