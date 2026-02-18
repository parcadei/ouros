# conformance: binary_ops
# description: a ^ b where a.__xor__ returns NotImplemented, falls to b.__rxor__
# tags: xor,rxor,notimplemented,fallback
# ---
class A:
    def __xor__(self, other):
        return NotImplemented

class B:
    def __rxor__(self, other):
        return "B.__rxor__"

print(A() ^ B())
