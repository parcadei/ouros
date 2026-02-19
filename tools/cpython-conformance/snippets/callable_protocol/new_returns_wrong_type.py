# conformance: callable_protocol
# description: __new__ returning wrong type causes __init__ to be skipped
# tags: new,wrong_type,init_skip
# ---
class WrongType:
    def __new__(cls):
        print("__new__ returning a string")
        return "not an instance"  # Not an instance of WrongType
    def __init__(self):
        # This should NOT be called because __new__ didn't return a WrongType instance
        print("__init__ called (SHOULD NOT HAPPEN)")

result = WrongType()
print(f"result = {result!r}")
print(f"type = {type(result).__name__}")

# __new__ returning an instance of a DIFFERENT class
class Other:
    def __init__(self):
        self.from_other_init = True

class ReturnsOther:
    def __new__(cls):
        print("__new__ returning Other instance")
        return Other()
    def __init__(self):
        print("ReturnsOther.__init__ (SHOULD NOT HAPPEN)")

result = ReturnsOther()
print(f"type = {type(result).__name__}")

# __new__ returning correct type: __init__ IS called
class Correct:
    def __new__(cls):
        print("__new__ returning correct type")
        return super().__new__(cls)
    def __init__(self):
        print("__init__ called (correct)")

Correct()
