# conformance: inplace_ops
# description: Subclass __iadd__ returns NotImplemented, parent __add__ used
# tags: iadd,notimplemented,subclass,fallback
# ---
class Base:
    def __init__(self, v):
        self.v = v
    def __add__(self, other):
        return Base(self.v + other.v)
    def __repr__(self):
        return f'Base({self.v})'

class Sub(Base):
    def __iadd__(self, other):
        return NotImplemented
    def __repr__(self):
        return f'Sub({self.v})'

s = Sub(5)
b = Base(3)
s += b
print(s)
print(type(s).__name__)
