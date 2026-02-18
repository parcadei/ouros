# conformance: cross_protocol
# description: When __bool__ raises an exception, it must NOT fall back to __len__; the exception propagates
# tags: bool,len,exception,no_fallback,cross_protocol
# ---
class BoolRaises:
    def __bool__(self):
        raise ValueError("bool exploded")
    def __len__(self):
        return 5  # Should never be reached

obj = BoolRaises()

# Direct bool() call
try:
    bool(obj)
except ValueError as e:
    print(f"ValueError: {e}")

# In if-statement (truthiness check)
try:
    if obj:
        print("should not reach")
except ValueError as e:
    print(f"if ValueError: {e}")

# In while condition
try:
    while obj:
        break
except ValueError as e:
    print(f"while ValueError: {e}")

# In boolean operator (and/or)
try:
    result = obj and True
except ValueError as e:
    print(f"and ValueError: {e}")

# Contrast: __bool__ returning NotImplemented is NOT special
# (unlike binary ops, __bool__ has no NotImplemented protocol)
class BoolNotImpl:
    def __bool__(self):
        return NotImplemented  # This is truthy! Not a special signal.
    def __len__(self):
        return 0

# NotImplemented is truthy, so bool() should... actually raise TypeError
# because __bool__ must return bool
try:
    bool(BoolNotImpl())
    print("no error (NotImplemented is truthy)")
except TypeError as e:
    print(f"TypeError: {e}")
