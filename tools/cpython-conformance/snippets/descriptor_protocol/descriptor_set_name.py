# conformance: descriptor_protocol
# description: __set_name__ called on descriptor when class is created
# tags: set_name,descriptor,class_creation
# ---
class Desc:
    def __set_name__(self, owner, name):
        print(f"set_name: owner={owner.__name__}, name={name}")
    def __get__(self, obj, objtype=None):
        return "value"

class MyClass:
    foo = Desc()
    bar = Desc()

print(MyClass.foo)
