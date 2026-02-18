# conformance: container_protocol
# description: Custom __delitem__ for subscript deletion
# tags: delitem,subscript
# ---
class MyDict:
    def __init__(self):
        self.data = {}
    def __setitem__(self, key, value):
        self.data[key] = value
    def __getitem__(self, key):
        return self.data[key]
    def __delitem__(self, key):
        del self.data[key]

d = MyDict()
d["a"] = 1
d["b"] = 2
print(d["a"])
del d["a"]
try:
    print(d["a"])
except KeyError:
    print("KeyError")
