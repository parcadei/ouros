# conformance: binary_ops
# description: a * b where a.__mul__ returns NotImplemented, falls to b.__rmul__
# tags: mul,rmul,notimplemented,fallback
# ---
class A:
    def __mul__(self, other):
        return NotImplemented

class B:
    def __rmul__(self, other):
        return "B.__rmul__"

print(A() * B())
