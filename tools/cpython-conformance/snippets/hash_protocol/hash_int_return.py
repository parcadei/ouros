# conformance: hash_protocol
# description: __hash__ must return an int; returning non-int raises TypeError
# tags: hash,return_type,typeerror
# ---
class BadHash:
    def __hash__(self):
        return "not_an_int"

b = BadHash()
try:
    hash(b)
except TypeError as e:
    print("TypeError")
