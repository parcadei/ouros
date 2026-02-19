# conformance: unary_ops
# description: Custom __abs__ for abs() builtin
# tags: abs,custom,unary
# ---
class Vec:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __abs__(self):
        return (self.x ** 2 + self.y ** 2) ** 0.5

v = Vec(3, 4)
print(abs(v))
