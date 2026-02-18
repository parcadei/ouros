# conformance: attribute_access
# description: Property-based attribute access
# tags: attribute,property,descriptor
# ---
class Circle:
    def __init__(self, radius):
        self._radius = radius

    @property
    def radius(self):
        return self._radius

    @radius.setter
    def radius(self, value):
        if value < 0:
            raise ValueError("negative radius")
        self._radius = value

c = Circle(5)
print(c.radius)
c.radius = 10
print(c.radius)

try:
    c.radius = -1
except ValueError as e:
    print(type(e).__name__, e)
