# conformance: unary_ops
# description: Missing __neg__ raises TypeError
# tags: neg,missing,typeerror
# ---
class NoNeg:
    pass

try:
    -NoNeg()
except TypeError as e:
    print("TypeError raised")
