# conformance: binary_ops
# description: a @ b where a.__matmul__ returns NotImplemented, falls to b.__rmatmul__
# tags: matmul,rmatmul,notimplemented,fallback
# ---
class A:
    def __matmul__(self, other):
        return NotImplemented

class B:
    def __rmatmul__(self, other):
        return "B.__rmatmul__"

print(A() @ B())
