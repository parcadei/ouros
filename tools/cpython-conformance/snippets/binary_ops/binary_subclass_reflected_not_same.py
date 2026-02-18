# conformance: binary_ops
# description: Reflected not called if same type (no subclass relationship)
# tags: add,radd,same_type,no_reflect
# ---
class A:
    def __add__(self, other):
        return "A.__add__"
    def __radd__(self, other):
        return "A.__radd__"

# Same type: __add__ is tried first, succeeds, __radd__ is NOT called
print(A() + A())
