# conformance: binary_ops
# description: a / b where a.__truediv__ returns NotImplemented, falls to b.__rtruediv__
# tags: truediv,rtruediv,notimplemented,fallback
# ---
class A:
    def __truediv__(self, other):
        return NotImplemented

class B:
    def __rtruediv__(self, other):
        return "B.__rtruediv__"

print(A() / B())
