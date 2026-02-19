# === vars(obj) returns __dict__ ===
class WithDict:
    def __init__(self):
        self.x = 1
        self.y = 2


obj = WithDict()
obj_vars = vars(obj)
assert obj_vars == {'x': 1, 'y': 2}, 'vars(obj) should return object __dict__ mapping'

obj_vars['z'] = 3
assert obj.z == 3, 'vars(obj) should return the live object __dict__'


# === vars(class) returns class namespace mapping ===
class ClassVars:
    marker = 7


class_vars = vars(ClassVars)
assert class_vars['marker'] == 7, 'vars(class) should expose class namespace'


# === vars(obj) without __dict__ raises TypeError ===
try:
    vars(1)
    assert False, 'vars(1) should raise TypeError because int has no __dict__'
except TypeError as exc:
    assert str(exc) == 'vars() argument must have __dict__ attribute', 'vars() missing __dict__ message should match'


# === vars() no-arg form: CPython locals dict or sandbox TypeError ===
try:
    no_arg_vars = vars()
except TypeError as exc:
    assert str(exc) == 'vars() without arguments is not supported in this sandbox', (
        'vars() no-arg sandbox error message should match'
    )
else:
    assert isinstance(no_arg_vars, dict), 'vars() without args should return locals dict when available'
