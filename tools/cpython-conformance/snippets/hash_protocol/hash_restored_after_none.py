# conformance: hash_protocol
# description: Subclass restores __hash__ after parent set it to None
# tags: hash,restore,inheritance
# ---
class Base:
    __hash__ = None

class Sub(Base):
    def __hash__(self):
        return 42

s = Sub()
print(hash(s))
