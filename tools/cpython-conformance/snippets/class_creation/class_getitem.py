# conformance: class_creation
# description: __class_getitem__ enables MyClass[X] syntax (PEP 585)
# tags: class_getitem,generic,subscript,pep585
# ---
class MyList:
    def __class_getitem__(cls, item):
        print(f"class_getitem: cls={cls.__name__}, item={item}")
        return f"MyList[{item}]"

result = MyList[int]
print(result)  # MyList[<class 'int'>]

result = MyList[str]
print(result)

# Multiple type args (tuple)
class MyDict:
    def __class_getitem__(cls, item):
        return f"MyDict[{item}]"

result = MyDict[str, int]
print(result)  # MyDict[(<class 'str'>, <class 'int'>)]

# Built-in types support this too (PEP 585)
print(list[int])           # list[int]
print(dict[str, int])      # dict[str, int]
print(tuple[int, ...])     # tuple[int, ...]

# __class_getitem__ is a CLASS method, not instance method
# It's looked up on the metaclass via BinarySubscr
class C:
    def __class_getitem__(cls, item):
        return f"C[{item}]"
    def __getitem__(self, item):
        return f"instance[{item}]"

# On the class: __class_getitem__
print(C[42])

# On an instance: __getitem__
c = C()
print(c[42])
