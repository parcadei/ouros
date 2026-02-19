# conformance: container_protocol
# description: Custom __getitem__ for subscript access
# tags: getitem,subscript
# ---
class MyList:
    def __init__(self, *args):
        self.data = list(args)
    def __getitem__(self, index):
        return self.data[index]

m = MyList(10, 20, 30)
print(m[0])
print(m[1])
print(m[2])
print(m[-1])
