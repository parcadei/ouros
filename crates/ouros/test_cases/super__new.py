# === super().__new__(cls, ...) for immutable builtin subclasses ===
class MyInt(int):
    def __new__(cls, value):
        return super().__new__(cls, value)


class MyStr(str):
    def __new__(cls, value):
        return super().__new__(cls, value)


class MyTuple(tuple):
    def __new__(cls, values):
        return super().__new__(cls, values)


my_int = MyInt(42)
my_str = MyStr('hello')
my_tuple = MyTuple((1, 2))

assert my_int == 42, 'super().__new__ for int subclasses should construct a value without TypeError'
assert isinstance(my_int, int), 'result from int super().__new__ should behave like int'
assert my_str == 'hello', 'super().__new__ for str subclasses should construct a value without TypeError'
assert isinstance(my_str, str), 'result from str super().__new__ should behave like str'
assert my_tuple == (1, 2), 'super().__new__ for tuple subclasses should construct a value without TypeError'
assert isinstance(my_tuple, tuple), 'result from tuple super().__new__ should behave like tuple'
