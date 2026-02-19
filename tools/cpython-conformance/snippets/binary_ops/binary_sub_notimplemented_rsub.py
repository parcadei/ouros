# conformance: binary_ops
# description: a - b where a.__sub__ returns NotImplemented, falls to b.__rsub__
# tags: sub,rsub,notimplemented,fallback
# ---
class A:
    def __sub__(self, other):
        return NotImplemented

class B:
    def __rsub__(self, other):
        return "B.__rsub__"

print(A() - B())
