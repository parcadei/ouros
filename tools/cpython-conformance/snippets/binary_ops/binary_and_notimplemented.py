# conformance: binary_ops
# description: a & b where a.__and__ returns NotImplemented, falls to b.__rand__
# tags: and,rand,notimplemented,fallback
# ---
class A:
    def __and__(self, other):
        return NotImplemented

class B:
    def __rand__(self, other):
        return "B.__rand__"

print(A() & B())
