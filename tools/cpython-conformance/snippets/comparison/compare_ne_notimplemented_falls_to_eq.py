# conformance: comparison
# description: __ne__ not defined, falls back to negation of __eq__
# tags: ne,eq,fallback,negation
# ---
class A:
    def __init__(self, v):
        self.v = v
    def __eq__(self, other):
        if isinstance(other, A):
            return self.v == other.v
        return NotImplemented

print(A(1) != A(1))
print(A(1) != A(2))
