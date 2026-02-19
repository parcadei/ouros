# conformance: string_repr_format
# description: repr() calls __repr__ on instances; type-level lookup, not instance dict
# tags: repr,dunder,type_lookup
# ---
class Custom:
    def __repr__(self):
        return "Custom()"

c = Custom()
print(repr(c))       # Custom()
print(f"{c!r}")      # Custom() (f-string !r uses repr)

# Type-level lookup, not instance dict
class C:
    def __repr__(self):
        return "type-level"

c = C()
c.__repr__ = lambda: "instance-level"
print(repr(c))  # "type-level" (CPython uses type, not instance)

# __repr__ must return str
class BadRepr:
    def __repr__(self):
        return 42

try:
    repr(BadRepr())
except TypeError as e:
    print(f"TypeError: {e}")

# __repr__ in print() (print calls str() which falls back to __repr__)
class OnlyRepr:
    def __repr__(self):
        return "OnlyRepr repr"

print(OnlyRepr())  # calls str() -> falls to __repr__
