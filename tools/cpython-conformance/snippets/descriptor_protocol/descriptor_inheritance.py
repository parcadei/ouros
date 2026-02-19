# conformance: descriptor_protocol
# description: Descriptor inherited through class hierarchy
# tags: descriptor,inheritance,mro
# ---
class Desc:
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return "from descriptor"
    def __set__(self, obj, value):
        print(f"setting to {value}")

class Base:
    x = Desc()

class Sub(Base):
    pass

s = Sub()
print(s.x)
s.x = 42
