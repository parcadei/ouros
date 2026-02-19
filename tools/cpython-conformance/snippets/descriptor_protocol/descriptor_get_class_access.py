# conformance: descriptor_protocol
# description: Descriptor __get__ on class access receives (None, owner)
# tags: descriptor,class_access,get
# ---
class Desc:
    def __get__(self, obj, objtype=None):
        return f"obj={obj}, type={objtype.__name__}"

class C:
    x = Desc()

print(C.x)
c = C()
print(c.x)
