# conformance: binary_ops
# description: a + b with no __add__ or __radd__ at all raises TypeError
# tags: add,typeerror,no_dunder
# ---
class A:
    pass

class B:
    pass

try:
    A() + B()
except TypeError as e:
    print("TypeError raised")
