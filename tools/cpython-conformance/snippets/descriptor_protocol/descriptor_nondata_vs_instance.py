# conformance: descriptor_protocol
# description: Non-data descriptor (__get__ only) loses to instance attribute
# tags: nondata_descriptor,priority,instance
# ---
class NonDataDesc:
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return "from descriptor"

class C:
    x = NonDataDesc()

c = C()
print(c.x)  # descriptor wins (no instance attr yet)
c.__dict__['x'] = "from instance"
print(c.x)  # instance wins over non-data descriptor
