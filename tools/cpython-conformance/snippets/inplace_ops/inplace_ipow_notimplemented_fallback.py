# conformance: inplace_ops
# description: __ipow__ returns NotImplemented, fallback to __pow__
# tags: ipow,notimplemented,fallback
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __ipow__(self, other):
        return NotImplemented
    def __pow__(self, other):
        return A(self.v ** other.v)
    def __repr__(self):
        return f'A({self.v})'

a = A(2)
b = A(3)
a **= b
print(a)
