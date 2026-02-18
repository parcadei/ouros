# conformance: inplace_ops
# description: __ifloordiv__ returns NotImplemented, fallback to __floordiv__
# tags: ifloordiv,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __ifloordiv__(self, other):
        return NotImplemented
    def __floordiv__(self, other):
        return A(self.v // other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(10)
b = A(3)
a //= b
print(a)
