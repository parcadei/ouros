# === 3-arg type() creates class ===
MyClass = type('MyClass', (), {'x': 42, 'greet': lambda self: 'hello'})

obj = MyClass()
assert obj.x == 42, 'dynamic class attribute'
assert obj.greet() == 'hello', 'dynamic class method'
assert type(obj).__name__ == 'MyClass', 'dynamic class name'

# === Inheritance via 3-arg type() ===
Base = type('Base', (), {'value': 10})
Child = type('Child', (Base,), {'extra': 20})

c = Child()
assert c.value == 10, 'inherited attribute'
assert c.extra == 20, 'child attribute'

print('type() 3-arg passed')
