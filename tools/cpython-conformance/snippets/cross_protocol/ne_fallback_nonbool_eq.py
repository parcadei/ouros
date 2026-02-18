# conformance: cross_protocol
# description: __ne__ fallback negates __eq__ result; when __eq__ returns non-bool, __ne__ calls not(result) which invokes __bool__
# tags: ne,eq,fallback,nonbool,bool,cross_protocol
# ---
class Weird:
    def __init__(self, v):
        self.v = v
    def __eq__(self, other):
        # Returns a string, not a bool
        return "equal" if self.v == other.v else ""
    # No __ne__ defined -- CPython falls back to not(__eq__(other))

a = Weird(1)
b = Weird(1)
c = Weird(2)

# a == b returns "equal" (truthy string)
# a != b should be not "equal" which is False
print(repr(a == b))   # "equal"
print(repr(a != b))   # False (not "equal" -> False)
print(repr(a == c))   # "" (empty string)
print(repr(a != c))   # True (not "" -> True)

# Now with a class whose __eq__ returns an object with custom __bool__
class BoolResult:
    def __init__(self, val):
        self.val = val
    def __bool__(self):
        return self.val
    def __repr__(self):
        return f"BoolResult({self.val})"

class Custom:
    def __init__(self, v):
        self.v = v
    def __eq__(self, other):
        return BoolResult(self.v == other.v)

x = Custom(1)
y = Custom(1)
z = Custom(2)

eq_result = (x == y)
ne_result = (x != y)
print(type(eq_result).__name__)  # BoolResult
print(type(ne_result).__name__)  # bool (because `not` on BoolResult calls __bool__ -> bool)
print(ne_result)                 # False
print(x != z)                    # True
