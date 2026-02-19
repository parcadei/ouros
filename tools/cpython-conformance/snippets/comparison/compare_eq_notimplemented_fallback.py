# conformance: comparison
# description: __eq__ returns NotImplemented, falls to other's __eq__
# tags: eq,notimplemented,fallback
# ---
class A:
    def __eq__(self, other):
        return NotImplemented

class B:
    def __eq__(self, other):
        return "B says equal"

a = A()
b = B()
print(a == b)
print(b == a)
