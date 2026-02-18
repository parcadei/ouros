# conformance: hash_protocol
# description: Basic hash protocol - hashable types
# tags: hash,protocol
# ---
# All hashable types should produce consistent int hashes
print(type(hash(42)).__name__)
print(type(hash("hello")).__name__)
print(type(hash((1, 2, 3))).__name__)
print(type(hash(True)).__name__)
print(type(hash(None)).__name__)
print(type(hash(3.14)).__name__)

# hash consistency: same value same hash
print(hash(42) == hash(42))
print(hash("hello") == hash("hello"))
print(hash(True) == hash(1))
print(hash(False) == hash(0))
