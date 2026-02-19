# conformance: descriptor_protocol
# description: Data descriptor (__get__ + __set__) takes priority over instance attribute
# tags: data_descriptor,priority,instance
# ---
class DataDesc:
    def __set_name__(self, owner, name):
        self.name = name
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return f"desc_get:{obj.__dict__.get('_' + self.name, 'unset')}"
    def __set__(self, obj, value):
        obj.__dict__['_' + self.name] = value

class C:
    x = DataDesc()

c = C()
c.x = 42
print(c.x)
# Even though _x is in __dict__, the descriptor __get__ controls access
c.__dict__['x'] = "direct"
print(c.x)  # Still goes through descriptor
