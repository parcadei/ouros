# conformance: unary_ops
# description: not operator with custom __bool__
# tags: not,bool,custom
# ---
class MyTrue:
    def __bool__(self):
        return True

class MyFalse:
    def __bool__(self):
        return False

print(not MyTrue())
print(not MyFalse())
