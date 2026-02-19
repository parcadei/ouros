# conformance: inplace_ops
# description: Parent __iadd__ returns NotImplemented, inherited by subclass
# tags: iadd,notimplemented,inheritance,fallback
# ---
class Base:
    def __init__(self, v):
        self.v = v
    def __iadd__(self, other):
        return NotImplemented
    def __add__(self, other):
        return Base(self.v + other.v)
    def __repr__(self):
        return f'Base({self.v})'

class Sub(Base):
    def __repr__(self):
        return f'Sub({self.v})'

s = Sub(7)
b = Base(2)
s += b
print(s)
print(type(s).__name__)
