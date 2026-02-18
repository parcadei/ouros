# conformance: binary_ops
# description: a % b where a.__mod__ returns NotImplemented, falls to b.__rmod__
# tags: mod,rmod,notimplemented,fallback
# ---
class A:
    def __mod__(self, other):
        return NotImplemented

class B:
    def __rmod__(self, other):
        return "B.__rmod__"

print(A() % B())
