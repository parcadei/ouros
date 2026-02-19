# conformance: attribute_access
# description: __delattr__ intercepts attribute deletion
# tags: delattr,override
# ---
class LogDel:
    def __init__(self):
        self.x = 10
    def __delattr__(self, name):
        print(f"deleting {name}")
        object.__delattr__(self, name)

obj = LogDel()
del obj.x
try:
    print(obj.x)
except AttributeError:
    print("AttributeError after delete")
