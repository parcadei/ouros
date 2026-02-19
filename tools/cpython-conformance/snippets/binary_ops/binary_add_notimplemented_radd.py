# conformance: binary_ops
# description: a + b where a.__add__ returns NotImplemented, falls to b.__radd__
# tags: add,radd,notimplemented,fallback
# ---
class A:
    def __add__(self, other):
        return NotImplemented

class B:
    def __radd__(self, other):
        return "B.__radd__ called"

a = A()
b = B()
print(a + b)
