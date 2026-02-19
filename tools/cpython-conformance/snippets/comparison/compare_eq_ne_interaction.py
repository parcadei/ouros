# conformance: comparison
# description: __ne__ defaults to not-__eq__ when __ne__ is not defined
# tags: eq,ne,default,interaction
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __eq__(self, other):
        if isinstance(other, A):
            return self.v == other.v
        return NotImplemented

a1 = A(1)
a2 = A(1)
a3 = A(2)
print(a1 == a2)
print(a1 != a2)
print(a1 == a3)
print(a1 != a3)
