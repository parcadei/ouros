# conformance: hash_protocol
# description: Subclass inherits __hash__ = None from parent, unhashable
# tags: hash,unhashable,inheritance
# ---
class Base:
    __hash__ = None

class Sub(Base):
    pass

s = Sub()
try:
    hash(s)
except TypeError as e:
    print("TypeError")
