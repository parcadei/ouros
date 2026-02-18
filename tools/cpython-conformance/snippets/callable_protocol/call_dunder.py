# conformance: callable_protocol
# description: __call__ makes instances callable; lookup is on the type, not the instance
# tags: call,callable,dunder
# ---
class Adder:
    def __init__(self, base):
        self.base = base
    def __call__(self, x):
        return self.base + x

add5 = Adder(5)
print(add5(10))    # 15
print(add5(20))    # 25
print(callable(add5))  # True

# __call__ on the TYPE matters, not the instance dict
class C:
    def __call__(self):
        return "type-level call"

c = C()
c.__call__ = lambda: "instance-level call"
# CPython uses type-level __call__, not instance attribute
print(c())  # "type-level call"

# Nested calls
class Doubler:
    def __call__(self, x):
        return x * 2

class Composer:
    def __init__(self, f, g):
        self.f = f
        self.g = g
    def __call__(self, x):
        return self.f(self.g(x))

composed = Composer(Doubler(), Adder(3))
print(composed(4))  # Doubler(Adder(3)(4)) = Doubler(7) = 14

# Not callable: no __call__
class Plain:
    pass

try:
    Plain()()
except TypeError as e:
    print("TypeError: not callable")
