# conformance: container_protocol
# description: __len__ must return int; non-int raises TypeError; negative raises ValueError
# tags: len,return_type,typeerror
# ---
class LenString:
    def __len__(self):
        return "not an int"

try:
    len(LenString())
except TypeError as e:
    print(f"TypeError for str: {e}")

class LenFloat:
    def __len__(self):
        return 3.14

try:
    len(LenFloat())
except TypeError as e:
    print(f"TypeError for float: {e}")

class LenNone:
    def __len__(self):
        return None

try:
    len(LenNone())
except TypeError as e:
    print(f"TypeError for None: {e}")

# Negative int raises ValueError (not TypeError)
class LenNeg:
    def __len__(self):
        return -1

try:
    len(LenNeg())
except ValueError as e:
    print(f"ValueError for negative: {e}")

# Bool return is accepted (bool is int subclass)
class LenBool:
    def __len__(self):
        return True  # True == 1

print(f"len(LenBool()) = {len(LenBool())}")  # 1

# __len__ returning non-int also affects bool() fallback
class LenStringBool:
    def __len__(self):
        return "five"

try:
    bool(LenStringBool())
except TypeError as e:
    print(f"TypeError in bool/len fallback: {e}")
