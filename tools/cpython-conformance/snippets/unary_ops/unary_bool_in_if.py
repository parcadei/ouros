# conformance: unary_ops
# description: __bool__ used in if-statement truthiness check
# tags: bool,if,truthiness
# ---
class AlwaysFalse:
    def __bool__(self):
        return False

class AlwaysTrue:
    def __bool__(self):
        return True

if AlwaysFalse():
    print("should not print")
else:
    print("false branch")

if AlwaysTrue():
    print("true branch")
else:
    print("should not print")
