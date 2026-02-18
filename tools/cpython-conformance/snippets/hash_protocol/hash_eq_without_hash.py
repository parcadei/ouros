# conformance: hash_protocol
# description: Class with __eq__ but no __hash__ should be unhashable
# tags: hash,eq,unhashable
# ---
class HasEq:
    def __eq__(self, other):
        return True

h = HasEq()
try:
    hash(h)
except TypeError as e:
    print("TypeError")
