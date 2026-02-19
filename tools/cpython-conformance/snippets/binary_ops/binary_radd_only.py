# conformance: binary_ops
# description: Only __radd__ defined on RHS, LHS has no __add__
# tags: radd,only,no_add
# ---
class A:
    pass

class B:
    def __radd__(self, other):
        return "B.__radd__"

print(A() + B())
