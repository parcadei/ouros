# conformance: cross_protocol
# description: str(x) falls back to __repr__ when __str__ is not defined
# tags: str,repr,fallback,cross_protocol
# ---
class HasBoth:
    def __str__(self):
        return "str result"
    def __repr__(self):
        return "repr result"

class ReprOnly:
    def __repr__(self):
        return "only repr"

class StrOnly:
    def __str__(self):
        return "only str"

class Neither:
    pass

# Has both: str() uses __str__
print(str(HasBoth()))      # str result
print(repr(HasBoth()))     # repr result

# Only __repr__: str() falls back to __repr__
print(str(ReprOnly()))     # only repr
print(repr(ReprOnly()))    # only repr

# Only __str__: repr() uses default object repr
s = StrOnly()
print(str(s))              # only str
r = repr(s)
print("StrOnly" in r)      # True (default repr includes class name)

# Neither: both use default
n = Neither()
s_r = str(n)
print("Neither" in s_r)    # True
