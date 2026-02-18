# conformance: inplace_ops
# description: __imul__ returns NotImplemented, fallback to __mul__
# tags: imul,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __imul__(self, other):
        return NotImplemented
    def __mul__(self, other):
        return A(self.v * other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(3)
b = A(4)
a *= b
print(a)
