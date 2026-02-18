# conformance: unary_ops
# description: Custom __neg__ on a class
# tags: neg,unary,custom
# ---
class Vec:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __neg__(self):
        return Vec(-self.x, -self.y)
    def __repr__(self):
        return f'Vec({self.x}, {self.y})'

v = Vec(3, -4)
print(-v)
