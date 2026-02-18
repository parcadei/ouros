# conformance: inplace_ops
# description: __itruediv__ returns NotImplemented, fallback to __truediv__
# tags: itruediv,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __itruediv__(self, other):
        return NotImplemented
    def __truediv__(self, other):
        return A(self.v / other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(10)
b = A(4)
a /= b
print(a)
