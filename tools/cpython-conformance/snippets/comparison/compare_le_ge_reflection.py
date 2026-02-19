# conformance: comparison
# description: __le__ reflects to __ge__ when NotImplemented
# tags: le,ge,reflection,notimplemented
# ---
class A:
    def __le__(self, other):
        return NotImplemented

class B:
    def __ge__(self, other):
        return "B.__ge__"

print(A() <= B())
