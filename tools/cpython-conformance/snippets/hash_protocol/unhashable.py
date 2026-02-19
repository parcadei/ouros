# conformance: hash_protocol
# description: Unhashable types should raise TypeError
# tags: hash,protocol,error
# ---
try:
    hash([1, 2, 3])
except TypeError as e:
    print("list:", type(e).__name__)

try:
    hash({"a": 1})
except TypeError as e:
    print("dict:", type(e).__name__)

try:
    hash({1, 2, 3})
except TypeError as e:
    print("set:", type(e).__name__)
