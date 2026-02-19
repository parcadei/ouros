# conformance: binary_ops
# description: a | b where a.__or__ returns NotImplemented, falls to b.__ror__
# tags: or,ror,notimplemented,fallback
# ---
class A:
    def __or__(self, other):
        return NotImplemented

class B:
    def __ror__(self, other):
        return "B.__ror__"

print(A() | B())
