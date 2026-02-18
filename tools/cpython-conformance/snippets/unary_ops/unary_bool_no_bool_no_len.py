# conformance: unary_ops
# description: Object without __bool__ or __len__ is always True
# tags: bool,default,true
# ---
class Empty:
    pass

print(bool(Empty()))
