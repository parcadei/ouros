# conformance: cross_protocol
# description: __index__ converts custom int-like objects for use as list/tuple subscript indices
# tags: index,getitem,subscript,cross_protocol
# ---
class MyIndex:
    def __init__(self, v):
        self.v = v
    def __index__(self):
        return self.v

data = [10, 20, 30, 40, 50]

# Using __index__ as list subscript
idx = MyIndex(2)
print(data[idx])  # 30

# Using __index__ as slice argument
s = slice(MyIndex(1), MyIndex(4))
print(data[s])  # [20, 30, 40]

# Using __index__ in tuple subscript
t = (100, 200, 300)
print(t[MyIndex(0)])  # 100

# __index__ must return int
class BadIndex:
    def __index__(self):
        return "not an int"

try:
    data[BadIndex()]
except TypeError as e:
    print("TypeError: __index__ returned non-int")

# __index__ used in range
print(list(range(MyIndex(3))))  # [0, 1, 2]

# __index__ used in bin/hex/oct
print(bin(MyIndex(10)))   # 0b1010
print(hex(MyIndex(255)))  # 0xff
