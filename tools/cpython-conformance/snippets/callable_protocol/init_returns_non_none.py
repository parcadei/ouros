# conformance: callable_protocol
# description: __init__ returning non-None raises TypeError in CPython
# tags: init,return,non_none,typeerror
# ---
class BadInit:
    def __init__(self):
        return 42  # Not allowed!

try:
    BadInit()
except TypeError as e:
    print(f"TypeError: {e}")

# Returning None explicitly is fine
class GoodInit:
    def __init__(self):
        return None

g = GoodInit()
print("GoodInit created successfully")

# Implicit return (no return statement) is fine
class ImplicitInit:
    def __init__(self):
        self.x = 1

i = ImplicitInit()
print(f"x={i.x}")

# Returning a string
class BadInit2:
    def __init__(self):
        return "hello"

try:
    BadInit2()
except TypeError as e:
    print(f"TypeError for string return: {e}")
