# conformance: comparison
# description: Custom class with all rich comparison operators
# tags: lt,le,gt,ge,eq,ne,rich_comparison
# ---
class Num:
    def __init__(self, v):
        self.v = v
    def __lt__(self, other):
        return self.v < other.v
    def __le__(self, other):
        return self.v <= other.v
    def __gt__(self, other):
        return self.v > other.v
    def __ge__(self, other):
        return self.v >= other.v
    def __eq__(self, other):
        return self.v == other.v
    def __ne__(self, other):
        return self.v != other.v

a = Num(1)
b = Num(2)
c = Num(1)
print(a < b)
print(a <= b)
print(a > b)
print(a >= b)
print(a == c)
print(a != b)
