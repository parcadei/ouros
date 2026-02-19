# conformance: unary_ops
# description: __len__ fallback used in if-statement truthiness (no __bool__)
# tags: len,if,truthiness,fallback
# ---
class NonEmpty:
    def __len__(self):
        return 3

class IsEmpty:
    def __len__(self):
        return 0

if NonEmpty():
    print("non-empty is truthy")
else:
    print("should not print")

if IsEmpty():
    print("should not print")
else:
    print("empty is falsy")
