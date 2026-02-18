# conformance: hash_protocol
# description: Subclass defines __eq__ but not __hash__, parent has __hash__ -> unhashable
# tags: hash,eq,subclass,unhashable
# ---
class Base:
    def __hash__(self):
        return 1

class Sub(Base):
    def __eq__(self, other):
        return True

s = Sub()
try:
    hash(s)
except TypeError as e:
    print("TypeError")
