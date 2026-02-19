# conformance: binary_ops
# description: When both operands are the same subclass type, __radd__ is not prioritized
# tags: add,radd,subclass,same_type
# ---
class Base:
    def __add__(self, other):
        return "Base.__add__"
    def __radd__(self, other):
        return "Base.__radd__"

class Sub(Base):
    pass

# Both are Sub - no subclass priority trick, LHS __add__ wins
s1 = Sub()
s2 = Sub()
print(s1 + s2)
