# conformance: hash_protocol
# description: Class with both __eq__ and __hash__ is hashable
# tags: hash,eq,hashable
# ---
class Both:
    def __eq__(self, other):
        return isinstance(other, Both)
    def __hash__(self):
        return 99

b = Both()
print(hash(b))
