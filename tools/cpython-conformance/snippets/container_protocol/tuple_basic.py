# conformance: container_protocol
# description: Basic tuple operations
# tags: tuple,container,getitem,len
# ---
t = (1, 2, 3, 4, 5)
print(t[0])
print(t[-1])
print(t[1:3])
print(len(t))
print(3 in t)
print(6 in t)
print(t + (6, 7))
print(t * 2)
print(t.count(3))
print(t.index(4))
