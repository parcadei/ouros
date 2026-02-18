# conformance: cross_protocol
# description: __eq__ in a mixin class makes the final class unhashable even if another parent defines __hash__
# tags: eq,hash,mixin,diamond,unhashable,cross_protocol
# ---
class EqMixin:
    def __eq__(self, other):
        return True

class HashMixin:
    def __hash__(self):
        return 99

# In CPython, if a class defines __eq__ but not __hash__, __hash__ is set to None.
# When D inherits from both, MRO is D -> EqMixin -> HashMixin -> object.
# EqMixin defines __eq__ without __hash__, so EqMixin.__hash__ is implicitly None.
# MRO finds __hash__ = None on EqMixin before HashMixin's __hash__.
class D(EqMixin, HashMixin):
    pass

d = D()
try:
    hash(d)
    print("hash succeeded (unexpected in CPython)")
except TypeError:
    print("TypeError: unhashable")

# Reverse MRO: HashMixin first
class D2(HashMixin, EqMixin):
    pass

d2 = D2()
try:
    result = hash(d2)
    print(f"hash={result}")
except TypeError:
    print("TypeError: unhashable")
