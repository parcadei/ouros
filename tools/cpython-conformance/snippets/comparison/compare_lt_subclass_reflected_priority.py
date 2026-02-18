# conformance: comparison
# description: Subclass reflected comparison takes priority: base < sub tries sub.__gt__ first
# tags: lt,gt,subclass,reflected,priority
# ---
class Base:
    def __lt__(self, other):
        return "Base.__lt__"
    def __gt__(self, other):
        return "Base.__gt__"

class Sub(Base):
    def __gt__(self, other):
        return "Sub.__gt__"

base = Base()
sub = Sub()

# base < sub: Sub is subclass, so Sub.__gt__ is tried first (reflected)
print(base < sub)   # Sub.__gt__

# sub < base: Sub.__lt__ inherited from Base, used directly (no subclass priority on LHS)
print(sub < base)   # Base.__lt__ (inherited)

# Same pattern for le/ge
class Base2:
    def __le__(self, other):
        return "Base2.__le__"
    def __ge__(self, other):
        return "Base2.__ge__"

class Sub2(Base2):
    def __ge__(self, other):
        return "Sub2.__ge__"

print(Base2() <= Sub2())  # Sub2.__ge__ (subclass reflected priority)
print(Sub2() <= Base2())  # Base2.__le__ (inherited from Base2)

# Sub overrides __lt__ specifically
class Sub3(Base):
    def __lt__(self, other):
        return "Sub3.__lt__"
    def __gt__(self, other):
        return "Sub3.__gt__"

# base < sub3: tries Sub3.__gt__ first (subclass reflected priority)
print(base < Sub3())   # Sub3.__gt__
# sub3 < base: tries Sub3.__lt__ (own method, not reflected)
print(Sub3() < base)   # Sub3.__lt__
