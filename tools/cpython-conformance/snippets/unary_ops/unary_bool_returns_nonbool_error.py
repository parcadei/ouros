# conformance: unary_ops
# description: __bool__ must return bool, non-bool return raises TypeError
# tags: bool,return_type,typeerror
# ---
class BadBool:
    def __bool__(self):
        return 1  # int, not bool

try:
    bool(BadBool())
except TypeError as e:
    print("TypeError raised")
