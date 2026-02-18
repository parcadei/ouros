# conformance: container_protocol
# description: __getitem__ raising KeyError/IndexError
# tags: getitem,keyerror,indexerror
# ---
class MyDict:
    def __init__(self):
        self.data = {"a": 1}
    def __getitem__(self, key):
        return self.data[key]

d = MyDict()
print(d["a"])
try:
    d["b"]
except KeyError:
    print("KeyError raised")
