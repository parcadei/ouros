# conformance: inplace_ops
# description: __ior__ returns NotImplemented, fallback to __or__
# tags: ior,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __ior__(self, other):
        return NotImplemented
    def __or__(self, other):
        return A(self.v | other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(0b1100)
b = A(0b1010)
a |= b
print(a)
