# conformance: inplace_ops
# description: All inplace+binary+reflected return NotImplemented -> TypeError
# tags: iadd,notimplemented,typeerror
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __iadd__(self, other):
        return NotImplemented
    def __add__(self, other):
        return NotImplemented
    def __radd__(self, other):
        return NotImplemented

class B:
    pass

a = A(1)
b = B()
try:
    a += b
except TypeError as e:
    print("TypeError raised")
