# conformance: binary_ops
# description: a // b where a.__floordiv__ returns NotImplemented, falls to b.__rfloordiv__
# tags: floordiv,rfloordiv,notimplemented,fallback
# ---
class A:
    def __floordiv__(self, other):
        return NotImplemented

class B:
    def __rfloordiv__(self, other):
        return "B.__rfloordiv__"

print(A() // B())
