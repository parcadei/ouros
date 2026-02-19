# conformance: cross_protocol
# description: __getattr__ raising non-AttributeError must propagate (not be swallowed as "attr not found")
# tags: getattr,exception,propagation,cross_protocol
# ---
class Explosive:
    def __getattr__(self, name):
        if name == "safe":
            return "safe value"
        raise ValueError(f"boom: {name}")

e = Explosive()
print(e.safe)  # "safe value"

try:
    e.dangerous
except ValueError as ex:
    print(f"ValueError propagated: {ex}")

# __getattribute__ raising non-AttributeError should also propagate
# (not trigger __getattr__ fallback)
class ExplodingGetattribute:
    def __getattribute__(self, name):
        raise RuntimeError(f"getattribute boom: {name}")
    def __getattr__(self, name):
        return "should not reach here"

try:
    ExplodingGetattribute().x
except RuntimeError as ex:
    print(f"RuntimeError propagated: {ex}")

# Only AttributeError from __getattribute__ triggers __getattr__
class SelectiveError:
    def __getattribute__(self, name):
        if name == "known":
            return "found it"
        raise AttributeError(name)  # This triggers __getattr__
    def __getattr__(self, name):
        return f"fallback: {name}"

s = SelectiveError()
print(s.known)     # "found it"
print(s.unknown)   # "fallback: unknown"
