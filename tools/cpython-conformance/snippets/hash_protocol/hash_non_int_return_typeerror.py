# conformance: hash_protocol
# description: __hash__ must return int; returning non-int raises TypeError
# tags: hash,return_type,typeerror
# ---
class HashString:
    def __hash__(self):
        return "not an int"

try:
    hash(HashString())
except TypeError as e:
    print(f"TypeError for str: {e}")

class HashFloat:
    def __hash__(self):
        return 3.14

try:
    hash(HashFloat())
except TypeError as e:
    print(f"TypeError for float: {e}")

class HashNone:
    def __hash__(self):
        return None

try:
    hash(HashNone())
except TypeError as e:
    print(f"TypeError for None: {e}")

# Returning a bool (subclass of int) IS allowed
class HashBool:
    def __hash__(self):
        return True

print(f"hash(HashBool()) = {hash(HashBool())}")  # 1

# Returning an int subclass with __index__ IS allowed
class MyInt(int):
    pass

class HashMyInt:
    def __hash__(self):
        return MyInt(42)

print(f"hash(HashMyInt()) = {hash(HashMyInt())}")  # 42
