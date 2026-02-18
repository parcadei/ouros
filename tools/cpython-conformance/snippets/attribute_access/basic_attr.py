# conformance: attribute_access
# description: Basic attribute access on objects
# tags: attribute,getattr,setattr
# ---
class Foo:
    x = 10
    def __init__(self):
        self.y = 20

f = Foo()
print(f.x)
print(f.y)
f.z = 30
print(f.z)
print(hasattr(f, "x"))
print(hasattr(f, "w"))
print(getattr(f, "x"))
print(getattr(f, "w", "default"))
