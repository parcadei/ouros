# conformance: hash_protocol
# description: Plain class without __eq__ or __hash__ is hashable (inherits object defaults)
# tags: hash,default,object
# ---
class Plain:
    pass

p = Plain()
# Just verify it doesn't raise - the actual value is identity-based
print(type(hash(p)))
