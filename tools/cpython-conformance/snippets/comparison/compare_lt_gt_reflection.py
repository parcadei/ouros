# conformance: comparison
# description: __lt__ reflects to __gt__ when NotImplemented
# tags: lt,gt,reflection,notimplemented
# ---
class A:
    def __lt__(self, other):
        return NotImplemented

class B:
    def __gt__(self, other):
        return "B.__gt__"

a = A()
b = B()
print(a < b)
