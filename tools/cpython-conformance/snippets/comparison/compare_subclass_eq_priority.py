# conformance: comparison
# description: Subclass __eq__ gets priority in reflected position
# tags: eq,subclass,priority,reflection
# ---
class Base:
    def __eq__(self, other):
        return "Base.__eq__"

class Sub(Base):
    def __eq__(self, other):
        return "Sub.__eq__"

base = Base()
sub = Sub()
# Sub.__eq__ should be tried first since Sub is a subclass
print(base == sub)
print(sub == base)
