# conformance: binary_ops
# description: Exception inside __add__ propagates without trying __radd__
# tags: add,exception,propagation
# ---
class A:
    def __add__(self, other):
        raise ValueError("add failed")

class B:
    def __radd__(self, other):
        return "B.__radd__"

try:
    A() + B()
except ValueError as e:
    print(f"ValueError: {e}")
