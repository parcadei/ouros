# conformance: container_protocol
# description: Basic set operations
# tags: set,container,len,membership
# ---
s = {1, 2, 3, 4, 5}
print(len(s))
print(1 in s)
print(6 in s)
print(sorted(s))
s.add(6)
print(sorted(s))
s.discard(1)
print(sorted(s))
print(sorted({1, 2, 3} & {2, 3, 4}))
print(sorted({1, 2, 3} | {2, 3, 4}))
print(sorted({1, 2, 3} - {2, 3, 4}))
print(sorted({1, 2, 3} ^ {2, 3, 4}))
