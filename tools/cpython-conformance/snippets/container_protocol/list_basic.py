# conformance: container_protocol
# description: Basic list operations
# tags: list,container,getitem,setitem,len
# ---
x = [10, 20, 30, 40, 50]
print(x[0])
print(x[-1])
print(x[1:3])
print(x[:2])
print(x[3:])
print(x[::2])
print(len(x))
x[1] = 99
print(x)
del x[0]
print(x)
