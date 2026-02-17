# === f-string fallback to __repr__ when __str__ is not defined ===
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y

    def __repr__(self):
        return f'Point({self.x}, {self.y})'


p = Point(1, 2)
assert str(p) == 'Point(1, 2)', 'str() should use __repr__ fallback when __str__ is missing'
assert f'{p}' == 'Point(1, 2)', 'f-string should use __repr__ fallback when __str__ is missing'
assert f'point is: {p}' == 'point is: Point(1, 2)', 'mixed f-string should use __repr__ fallback'
