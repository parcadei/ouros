from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


p = Point(1, 2)
assert p.x == 1, 'dataclass field access'
assert p.y == 2, 'dataclass field access'
assert repr(p) == 'Point(x=1, y=2)', 'dataclass repr'
assert Point(1, 2) == Point(1, 2), 'dataclass eq'
assert Point(1, 2) != Point(3, 4), 'dataclass neq'
print('dataclass test passed')
