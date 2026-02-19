# conformance: hash_protocol
# description: Diamond inheritance with mixed hash state
# tags: hash,diamond,mro,inheritance
# ---
class A:
    __hash__ = None

class B(A):
    def __hash__(self):
        return 10

class C(A):
    pass

class D(B, C):
    pass

# D -> B (has __hash__) -> C -> A (__hash__ = None)
# B's __hash__ should win via MRO
d = D()
print(hash(d))
