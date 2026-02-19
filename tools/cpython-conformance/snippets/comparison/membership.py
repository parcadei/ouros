# conformance: comparison
# description: Membership operators (in, not in)
# tags: comparison,membership,operator
# ---
print(1 in [1, 2, 3])
print(4 in [1, 2, 3])
print(4 not in [1, 2, 3])
print("a" in "abc")
print("d" in "abc")
print("hello" in {"hello": 1})
print(1 in {1, 2, 3})
