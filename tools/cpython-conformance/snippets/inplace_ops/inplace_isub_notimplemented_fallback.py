# conformance: inplace_ops
# description: __isub__ returns NotImplemented, should fallback to __sub__
# tags: isub,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __isub__(self, other):
        return NotImplemented
    def __sub__(self, other):
        return A(self.v - other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(10)
b = A(3)
a -= b
print(a)
