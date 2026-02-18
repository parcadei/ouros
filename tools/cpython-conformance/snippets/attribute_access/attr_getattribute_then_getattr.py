# conformance: attribute_access
# description: __getattribute__ raises AttributeError -> __getattr__ fallback
# tags: getattribute,getattr,fallback,attributeerror
# ---
class Chain:
    def __getattribute__(self, name):
        if name == "special":
            return "from getattribute"
        raise AttributeError(name)
    def __getattr__(self, name):
        return f"from getattr:{name}"

c = Chain()
print(c.special)
print(c.other)
