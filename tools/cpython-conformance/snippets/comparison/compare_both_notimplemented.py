# conformance: comparison
# description: Both sides return NotImplemented for lt -> falls back to default
# tags: lt,notimplemented,both_sides
# ---
class A:
    def __lt__(self, other):
        return NotImplemented

class B:
    def __gt__(self, other):
        return NotImplemented

a = A()
b = B()
try:
    result = a < b
    print("TypeError not raised")
except TypeError as e:
    print("TypeError raised")
