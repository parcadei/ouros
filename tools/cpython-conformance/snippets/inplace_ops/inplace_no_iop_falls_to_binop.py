# conformance: inplace_ops
# description: No __iadd__ defined at all, += falls through to __add__
# tags: iadd,fallback,no_inplace
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __add__(self, other):
        return A(self.v + other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(5)
b = A(10)
a += b
print(a)
