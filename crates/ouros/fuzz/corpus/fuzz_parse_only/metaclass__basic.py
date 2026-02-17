# === type() 1-arg returns type ===
assert type(42) == int, 'type(42) should be int'
assert type('hi') == str, 'type(hi) should be str'

# === __init_subclass__ called ===
class Base:
    subclasses = []

    def __init_subclass__(cls, **kwargs):
        Base.subclasses.append(cls)


class Child(Base):
    pass


assert len(Base.subclasses) == 1, '__init_subclass__ should be called'
assert Base.subclasses[0] is Child, 'Child should be in subclasses'
