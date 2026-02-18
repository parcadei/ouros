# conformance: hash_protocol
# description: Class with __hash__ = None is unhashable
# tags: hash,unhashable,hash_none
# ---
class Unhashable:
    __hash__ = None

u = Unhashable()
try:
    hash(u)
except TypeError as e:
    print("TypeError")
