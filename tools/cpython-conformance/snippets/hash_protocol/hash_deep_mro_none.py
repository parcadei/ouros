# conformance: hash_protocol
# description: __hash__ = None found deep in MRO chain
# tags: hash,mro,deep,unhashable
# ---
class A:
    __hash__ = None

class B(A):
    pass

class C(B):
    pass

class D(C):
    pass

try:
    hash(D())
except TypeError:
    print("TypeError")
