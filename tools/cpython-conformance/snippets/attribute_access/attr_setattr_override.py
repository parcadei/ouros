# conformance: attribute_access
# description: __setattr__ intercepts all attribute assignment
# tags: setattr,override
# ---
class LogSet:
    def __setattr__(self, name, value):
        print(f"setting {name}={value}")
        object.__setattr__(self, name, value)

obj = LogSet()
obj.x = 10
obj.y = 20
print(obj.x)
print(obj.y)
