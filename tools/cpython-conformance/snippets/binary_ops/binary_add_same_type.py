# conformance: binary_ops
# description: Binary add with same type, __add__ works directly
# tags: add,same_type
# ---
class Vec:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, other):
        return Vec(self.x + other.x, self.y + other.y)
    def __repr__(self):
        return f'Vec({self.x}, {self.y})'

print(Vec(1, 2) + Vec(3, 4))
