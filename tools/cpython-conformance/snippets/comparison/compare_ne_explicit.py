# conformance: comparison
# description: Explicit __ne__ overrides the default not-__eq__ behavior
# tags: ne,explicit,override
# ---
class A:
    def __eq__(self, other):
        return True
    def __ne__(self, other):
        return True  # intentionally contradictory

a = A()
b = A()
print(a == b)
print(a != b)
