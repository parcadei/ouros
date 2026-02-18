# conformance: descriptor_protocol
# description: Basic descriptor protocol (__get__, __set__)
# tags: descriptor,protocol
# ---
class Verbose:
    def __set_name__(self, owner, name):
        self.name = name

    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return obj.__dict__.get(self.name, 0)

    def __set__(self, obj, value):
        obj.__dict__[self.name] = value

class MyClass:
    attr = Verbose()

m = MyClass()
print(m.attr)
m.attr = 42
print(m.attr)
