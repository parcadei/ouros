# conformance: inplace_ops
# description: Mixed types: a += b where a.__iadd__ returns NotImplemented, a.__add__ returns NotImplemented, b.__radd__ works
# tags: iadd,radd,mixed_types,fallback_chain
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __iadd__(self, other):
        if not isinstance(other, A):
            return NotImplemented
        return A(self.v + other.v)
    def __add__(self, other):
        if not isinstance(other, A):
            return NotImplemented
        return A(self.v + other.v)

class B:
    def __init__(self, v):
        self.v = v
    def __radd__(self, other):
        return B(other.v + self.v)
    def __repr__(self):
        return f'B({self.v})'

a = A(5)
b = B(10)
a += b
print(a)
print(type(a).__name__)
