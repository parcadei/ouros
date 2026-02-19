# conformance: inplace_ops
# description: __iadd__ can return a different type (variable gets rebound)
# tags: iadd,rebind,different_type
# ---
class A:
    def __iadd__(self, other):
        return "now a string"

a = A()
a += 1
print(a)
print(type(a).__name__)
