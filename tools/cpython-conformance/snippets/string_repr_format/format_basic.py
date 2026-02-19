# conformance: string_repr_format
# description: format() builtin calls __format__; f-strings use it for non-repr/str conversions
# tags: format,dunder,fstring
# ---
class Money:
    def __init__(self, amount):
        self.amount = amount
    def __format__(self, spec):
        if spec == "dollars":
            return f"${self.amount:.2f}"
        elif spec == "cents":
            return f"{int(self.amount * 100)}c"
        return str(self.amount)

m = Money(42.5)
print(format(m, "dollars"))   # $42.50
print(format(m, "cents"))     # 4250c
print(format(m, ""))          # 42.5
print(f"{m:dollars}")         # $42.50 (f-string format spec)

# Default __format__ (from object) handles standard format spec
class Plain:
    pass

p = Plain()
# format(p, "") is equivalent to str(p)
# format(p, "non-empty") raises TypeError in CPython 3.x
try:
    format(p, "not-empty")
except TypeError as e:
    print(f"TypeError for non-empty spec on default: {e}")

# __format__ returning non-str raises TypeError
class BadFormat:
    def __format__(self, spec):
        return 42

try:
    format(BadFormat(), "")
except TypeError as e:
    print(f"TypeError: {e}")
