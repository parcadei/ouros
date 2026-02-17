# === __str__/__repr__ must return strings ===
class BadRepr:
    def __repr__(self):
        return 42

try:
    f'{BadRepr()}'
    assert False, 'f-string should raise TypeError when __str__/__repr__ returns non-string'
except TypeError as exc:
    assert exc.args[0] == '__str__ returned non-string (type int)', (
        f'expected __str__ non-string error, got {exc.args[0]!r}'
    )

try:
    f'{BadRepr()!r}'
    assert False, 'f-string !r should raise TypeError when __repr__ returns non-string'
except TypeError as exc:
    assert exc.args[0] == '__repr__ returned non-string (type int)', (
        f'expected __repr__ non-string error, got {exc.args[0]!r}'
    )

# === __format__ dispatch for f-string format specs ===
class Num:
    def __init__(self, value):
        self.value = value

    def __format__(self, spec):
        return format(self.value, spec)

n = Num(3.14159)
formatted_num = f'{n:.2f}'
assert formatted_num == '3.14', f'expected __format__ dispatch for f-string spec, got {formatted_num!r}'

# === Recursive repr through nested join(generator) ===
class Node:
    def __init__(self, val, children=None):
        self.val = val
        self.children = children or []

    def __repr__(self):
        if self.children:
            kids = ', '.join(f'{c}' for c in self.children)
            return f'Node({self.val}, [{kids}])'
        return f'Node({self.val})'

# Keep a direct generator-join assertion alongside the recursive repr case.
assert ', '.join(str(x) for x in [1, 2, 3]) == '1, 2, 3', 'join(generator) should keep all elements'

tree = Node(1, [Node(2, [Node(4)]), Node(3)])
result = f'{tree}'
assert result == 'Node(1, [Node(2, [Node(4)]), Node(3)])', f'recursive repr dropped elements: {result!r}'
