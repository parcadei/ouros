# conformance: unary_ops
# description: Custom __invert__ on a class
# tags: invert,unary,custom
# ---
class Bits:
    def __init__(self, v):
        self.v = v
    def __invert__(self):
        return Bits(~self.v)
    def __repr__(self):
        return f'Bits({self.v})'

b = Bits(0)
print(~b)
b2 = Bits(5)
print(~b2)
