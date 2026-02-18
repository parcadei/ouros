# conformance: attribute_access
# description: __getattribute__ overrides all attribute access
# tags: getattribute,override
# ---
class Interceptor:
    def __init__(self):
        self.x = 42
    def __getattribute__(self, name):
        return f"intercepted:{name}"

i = Interceptor()
print(i.x)
print(i.y)
print(i.anything)
