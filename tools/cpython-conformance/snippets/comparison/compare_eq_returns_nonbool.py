# conformance: comparison
# description: __eq__ can return non-bool values
# tags: eq,nonbool,return
# ---
class A:
    def __eq__(self, other):
        return "yes"

a = A()
b = A()
result = (a == b)
print(result)
print(type(result).__name__)
