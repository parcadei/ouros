# conformance: container_protocol
# description: Custom __setitem__ for subscript assignment
# tags: setitem,subscript
# ---
class MyList:
    def __init__(self, *args):
        self.data = list(args)
    def __setitem__(self, index, value):
        self.data[index] = value
    def __getitem__(self, index):
        return self.data[index]

m = MyList(1, 2, 3)
m[0] = 99
m[2] = 77
print(m[0])
print(m[1])
print(m[2])
