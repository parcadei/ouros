# conformance: callable_protocol
# description: __new__ called first, creates instance; __init__ called second to initialize it
# tags: new,init,instantiation,lifecycle
# ---
class Tracked:
    def __new__(cls, *args, **kwargs):
        print(f"__new__ called with args={args}")
        instance = super().__new__(cls)
        return instance

    def __init__(self, x, y=0):
        print(f"__init__ called with x={x}, y={y}")
        self.x = x
        self.y = y

t = Tracked(10, y=20)
print(f"x={t.x}, y={t.y}")

# __new__ with singleton pattern
class Singleton:
    _instance = None
    def __new__(cls):
        if cls._instance is None:
            print("creating new instance")
            cls._instance = super().__new__(cls)
        else:
            print("returning existing instance")
        return cls._instance
    def __init__(self):
        # __init__ is called every time, even if __new__ returns existing
        print("__init__ called")

s1 = Singleton()
s2 = Singleton()
print(s1 is s2)  # True

# __new__ receiving the class as first argument
class ShowCls:
    def __new__(cls):
        print(f"cls is ShowCls: {cls is ShowCls}")
        return super().__new__(cls)

ShowCls()
