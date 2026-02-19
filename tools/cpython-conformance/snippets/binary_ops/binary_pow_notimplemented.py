# conformance: binary_ops
# description: a ** b where a.__pow__ returns NotImplemented, falls to b.__rpow__
# tags: pow,rpow,notimplemented,fallback
# ---
class A:
    def __pow__(self, other):
        return NotImplemented

class B:
    def __rpow__(self, other):
        return "B.__rpow__"

print(A() ** B())
