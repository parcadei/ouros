# conformance: inplace_ops
# description: __imod__ returns NotImplemented, fallback to __mod__
# tags: imod,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __imod__(self, other):
        return NotImplemented
    def __mod__(self, other):
        return A(self.v % other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(10)
b = A(3)
a %= b
print(a)
