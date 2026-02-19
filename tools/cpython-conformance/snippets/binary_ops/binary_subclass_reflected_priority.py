# conformance: binary_ops
# description: Subclass reflected method takes priority over parent forward method
# tags: add,radd,subclass,priority,mro
# ---
class Base:
    def __add__(self, other):
        return "Base.__add__"
    def __radd__(self, other):
        return "Base.__radd__"

class Sub(Base):
    def __radd__(self, other):
        return "Sub.__radd__"

base = Base()
sub = Sub()
# When sub is on right side AND sub is a subclass, Sub.__radd__ takes priority
print(base + sub)
