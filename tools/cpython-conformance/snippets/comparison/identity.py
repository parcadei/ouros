# conformance: comparison
# description: Identity operators (is, is not)
# tags: comparison,identity,operator
# ---
print(None is None)
print(None is not None)
print(True is True)
print(1 is not None)
a = [1, 2, 3]
b = a
c = [1, 2, 3]
print(a is b)
print(a is c)
print(a is not c)
