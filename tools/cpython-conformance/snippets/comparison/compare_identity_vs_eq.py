# conformance: comparison
# description: 'is' uses identity, '==' uses __eq__
# tags: is,eq,identity
# ---
class AlwaysEq:
    def __eq__(self, other):
        return True

a = AlwaysEq()
b = AlwaysEq()
print(a == b)
print(a is b)
print(a is a)
