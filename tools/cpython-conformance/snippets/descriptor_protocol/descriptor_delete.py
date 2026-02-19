# conformance: descriptor_protocol
# description: Data descriptor with __delete__ method
# tags: descriptor,delete
# ---
class Desc:
    def __set_name__(self, owner, name):
        self.name = name
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return obj.__dict__.get('_' + self.name, 'unset')
    def __set__(self, obj, value):
        obj.__dict__['_' + self.name] = value
    def __delete__(self, obj):
        print(f"deleting {self.name}")
        obj.__dict__.pop('_' + self.name, None)

class C:
    x = Desc()

c = C()
c.x = 10
print(c.x)
del c.x
print(c.x)
