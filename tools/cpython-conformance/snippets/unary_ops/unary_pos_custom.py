# conformance: unary_ops
# description: Custom __pos__ on a class
# tags: pos,unary,custom
# ---
class Wrapper:
    def __init__(self, v):
        self.v = v
    def __pos__(self):
        return abs(self.v)

w = Wrapper(-10)
print(+w)
