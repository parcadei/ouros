# conformance: cross_protocol
# description: Exception in __radd__ propagates; it is not swallowed after __add__ returns NotImplemented
# tags: radd,exception,propagation,cross_protocol
# ---
class A:
    def __add__(self, other):
        return NotImplemented  # Falls through to B.__radd__

class B:
    def __radd__(self, other):
        raise RuntimeError("radd exploded")

try:
    A() + B()
except RuntimeError as e:
    print(f"RuntimeError: {e}")

# Exception in __add__ of LHS also propagates (no __radd__ attempt)
class C:
    def __add__(self, other):
        raise ValueError("add exploded")

class D:
    def __radd__(self, other):
        return "D.__radd__"  # Should never be reached

try:
    C() + D()
except ValueError as e:
    print(f"ValueError: {e}")

# Exception in reflected method during subclass priority
class Base:
    def __add__(self, other):
        return NotImplemented

class Sub(Base):
    def __radd__(self, other):
        raise TypeError("sub radd exploded")

try:
    Base() + Sub()
except TypeError as e:
    print(f"TypeError from subclass __radd__: {e}")
