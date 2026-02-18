# conformance: cross_protocol
# description: When __iadd__ raises an exception (not NotImplemented), it must NOT fall back to __add__/__radd__
# tags: iadd,add,exception,no_fallback,cross_protocol
# ---
class A:
    def __iadd__(self, other):
        raise ValueError("iadd exploded")
    def __add__(self, other):
        return "A.__add__"  # Should never be reached

class B:
    def __radd__(self, other):
        return "B.__radd__"  # Should never be reached

a = A()
b = B()
try:
    a += b
except ValueError as e:
    print(f"ValueError: {e}")

# Contrast with NotImplemented which DOES fall back
class C:
    def __iadd__(self, other):
        return NotImplemented
    def __add__(self, other):
        return "C.__add__"

c = C()
c += 1
print(c)  # C.__add__
