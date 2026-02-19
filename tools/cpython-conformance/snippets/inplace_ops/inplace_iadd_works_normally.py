# conformance: inplace_ops
# description: __iadd__ returns a value (not NotImplemented), used directly
# tags: iadd,normal
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __iadd__(self, other):
        return A(self.v + other.v + 100)
    def __add__(self, other):
        return A(self.v + other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(1)
b = A(2)
a += b
print(a)
