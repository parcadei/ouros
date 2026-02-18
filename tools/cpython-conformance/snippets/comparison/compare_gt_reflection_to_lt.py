# conformance: comparison
# description: a > b reflects to b.__lt__(a) when a.__gt__ returns NotImplemented
# tags: gt,lt,reflection
# ---
class A:
    def __gt__(self, other):
        return NotImplemented

class B:
    def __lt__(self, other):
        return "B.__lt__"

print(A() > B())
