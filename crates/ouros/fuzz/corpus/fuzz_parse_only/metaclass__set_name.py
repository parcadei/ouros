class Descriptor:
    def __set_name__(self, owner, name):
        self.owner = owner
        self.name = name

    def __get__(self, obj, objtype=None):
        return self.name


class MyClass:
    x = Descriptor()
    y = Descriptor()


assert MyClass.x == 'x', '__set_name__ should set name'
assert MyClass.y == 'y', '__set_name__ should set name'
