# conformance: inplace_ops
# description: __iadd__ returns NotImplemented, fallback to other's __radd__
# tags: iadd,radd,notimplemented,fallback,mixed_types
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __iadd__(self, other):
        return NotImplemented
    def __add__(self, other):
        return NotImplemented

class B:
    def __init__(self, v):
        self.v = v
    def __radd__(self, other):
        return B(other.v + self.v)
    def __repr__(self):
        return f'B({self.v})'

a = A(10)
b = B(20)
a += b
print(a)
