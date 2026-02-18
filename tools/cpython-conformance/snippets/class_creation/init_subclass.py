# conformance: class_creation
# description: __init_subclass__ is called when a class is subclassed
# tags: init_subclass,class_creation,hook
# ---
class Plugin:
    plugins = []
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        print(f"registering {cls.__name__}")
        Plugin.plugins.append(cls.__name__)

class AuthPlugin(Plugin):
    pass

class CachePlugin(Plugin):
    pass

print(Plugin.plugins)  # ['AuthPlugin', 'CachePlugin']

# With keyword arguments
class Validator:
    def __init_subclass__(cls, validate=False, **kwargs):
        super().__init_subclass__(**kwargs)
        cls.validate = validate
        print(f"{cls.__name__} validate={validate}")

class StrictValidator(Validator, validate=True):
    pass

class LaxValidator(Validator, validate=False):
    pass

print(StrictValidator.validate)  # True
print(LaxValidator.validate)     # False

# __init_subclass__ NOT called on the class itself, only subclasses
class Base:
    def __init_subclass__(cls, **kwargs):
        print(f"subclass created: {cls.__name__}")

# No output here - Base itself doesn't trigger __init_subclass__
class Child(Base):
    pass  # prints "subclass created: Child"
