# conformance: binary_ops
# description: a + b where both __add__ and __radd__ return NotImplemented -> TypeError
# tags: add,radd,notimplemented,typeerror
# ---
class A:
    def __add__(self, other):
        return NotImplemented

class B:
    def __radd__(self, other):
        return NotImplemented

a = A()
b = B()
try:
    a + b
except TypeError as e:
    print("TypeError raised")
