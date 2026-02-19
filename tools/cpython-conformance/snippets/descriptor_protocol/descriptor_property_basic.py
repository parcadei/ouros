# conformance: descriptor_protocol
# description: Built-in property descriptor behavior
# tags: property,descriptor,builtin
# ---
class C:
    def __init__(self):
        self._x = 0
    @property
    def x(self):
        return self._x
    @x.setter
    def x(self, value):
        self._x = value * 2

c = C()
print(c.x)
c.x = 5
print(c.x)
