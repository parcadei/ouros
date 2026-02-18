# conformance: inplace_ops
# description: __iand__ returns NotImplemented, fallback to __and__
# tags: iand,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __iand__(self, other):
        return NotImplemented
    def __and__(self, other):
        return A(self.v & other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(0b1100)
b = A(0b1010)
a &= b
print(a)
