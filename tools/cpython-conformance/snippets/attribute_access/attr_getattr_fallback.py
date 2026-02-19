# conformance: attribute_access
# description: __getattr__ as fallback after normal attribute lookup fails
# tags: getattr,fallback
# ---
class Fallback:
    def __init__(self):
        self.x = 10
    def __getattr__(self, name):
        return f"fallback:{name}"

f = Fallback()
print(f.x)
print(f.y)
print(f.missing)
