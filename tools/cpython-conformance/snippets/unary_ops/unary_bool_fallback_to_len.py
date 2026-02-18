# conformance: unary_ops
# description: __bool__ fallback to __len__ when __bool__ not defined
# tags: bool,len,fallback
# ---
class HasLen:
    def __len__(self):
        return 5

class HasLenZero:
    def __len__(self):
        return 0

print(bool(HasLen()))
print(bool(HasLenZero()))
