# call-external
import dataclasses

# === is_dataclass ===
# Returns True for dataclass instances, False for everything else
point = make_point()
assert dataclasses.is_dataclass(point) == True, 'is_dataclass on frozen dataclass is True'

mut_point = make_mutable_point()
assert dataclasses.is_dataclass(mut_point) == True, 'is_dataclass on mutable dataclass is True'

alice = make_user('Alice')
assert dataclasses.is_dataclass(alice) == True, 'is_dataclass on user dataclass is True'

empty = make_empty()
assert dataclasses.is_dataclass(empty) == True, 'is_dataclass on empty dataclass is True'

# Non-dataclass values
assert dataclasses.is_dataclass(42) == False, 'is_dataclass on int is False'
assert dataclasses.is_dataclass('hello') == False, 'is_dataclass on str is False'
assert dataclasses.is_dataclass(None) == False, 'is_dataclass on None is False'
assert dataclasses.is_dataclass([1, 2]) == False, 'is_dataclass on list is False'
assert dataclasses.is_dataclass({}) == False, 'is_dataclass on dict is False'
assert dataclasses.is_dataclass(True) == False, 'is_dataclass on bool is False'
assert dataclasses.is_dataclass((1,)) == False, 'is_dataclass on tuple is False'

# === fields ===
# Returns field descriptors; both CPython and Ouros agree on the count
f_point = dataclasses.fields(point)
assert len(f_point) == 2, 'frozen point has 2 fields'

f_user = dataclasses.fields(alice)
assert len(f_user) == 2, 'user has 2 fields'

f_empty = dataclasses.fields(empty)
assert len(f_empty) == 0, 'empty has 0 fields'

f_mut = dataclasses.fields(mut_point)
assert len(f_mut) == 2, 'mutable point has 2 fields'

# === asdict ===
# Converts dataclass instance to dict
d_point = dataclasses.asdict(point)
assert d_point == {'x': 1, 'y': 2}, 'asdict frozen point'
assert type(d_point) is dict, 'asdict returns dict type'

d_user = dataclasses.asdict(alice)
assert d_user == {'name': 'Alice', 'active': True}, 'asdict user with string field'

d_empty = dataclasses.asdict(empty)
assert d_empty == {}, 'asdict empty dataclass returns empty dict'

d_mut = dataclasses.asdict(mut_point)
assert d_mut == {'x': 1, 'y': 2}, 'asdict mutable point'

# === astuple ===
# Converts dataclass instance to tuple in field order
t_point = dataclasses.astuple(point)
assert t_point == (1, 2), 'astuple frozen point'
assert type(t_point) is tuple, 'astuple returns tuple type'

t_user = dataclasses.astuple(alice)
assert t_user == ('Alice', True), 'astuple user'

t_empty = dataclasses.astuple(empty)
assert t_empty == (), 'astuple empty dataclass returns empty tuple'

t_mut = dataclasses.astuple(mut_point)
assert t_mut == (1, 2), 'astuple mutable point'

# === replace on mutable dataclass ===
# Creates a shallow copy with specified field overrides
mut2 = make_mutable_point()
replaced = dataclasses.replace(mut2, x=99)
assert replaced.x == 99, 'replace overrides x'
assert replaced.y == 2, 'replace preserves y'
assert mut2.x == 1, 'original unchanged after replace'

# replace with multiple overrides
replaced2 = dataclasses.replace(mut2, x=100, y=200)
assert replaced2.x == 100, 'replace both x'
assert replaced2.y == 200, 'replace both y'
assert mut2.x == 1, 'original x still unchanged'

# replace with no changes (copy)
copied = dataclasses.replace(mut2)
assert copied.x == 1, 'copy x matches original'
assert copied.y == 2, 'copy y matches original'

# === replace on frozen dataclass ===
point2 = make_point()
replaced_frozen = dataclasses.replace(point2, x=50)
assert replaced_frozen.x == 50, 'replace on frozen overrides x'
assert replaced_frozen.y == 2, 'replace on frozen preserves y'
assert point2.x == 1, 'original frozen point unchanged'

# === replace preserves is_dataclass ===
assert dataclasses.is_dataclass(replaced) == True, 'replaced mutable is still dataclass'
assert dataclasses.is_dataclass(replaced_frozen) == True, 'replaced frozen is still dataclass'

# === asdict and astuple on replaced instance ===
d_replaced = dataclasses.asdict(replaced)
assert d_replaced == {'x': 99, 'y': 2}, 'asdict on replaced instance'

t_replaced = dataclasses.astuple(replaced)
assert t_replaced == (99, 2), 'astuple on replaced instance'

# === replace on user dataclass ===
bob = make_user('Bob')
replaced_user = dataclasses.replace(bob, name='Charlie')
assert replaced_user.name == 'Charlie', 'replace user name'
assert replaced_user.active == True, 'replace preserves active'
assert bob.name == 'Bob', 'original user unchanged'

# === make_dataclass ===
dynamic_point_factory = dataclasses.make_dataclass('DynamicPoint', ['x', 'y'])
dynamic_user_factory = dataclasses.make_dataclass('DynamicUser', [('name', str), ('age', int)])
dynamic_status_factory = dataclasses.make_dataclass('DynamicStatus', [['active', bool], 'score'])

if hasattr(dynamic_point_factory, '__mro__'):
    # CPython behavior: make_dataclass returns a class.
    dynamic_point = dynamic_point_factory(x=1, y=2)
    point_field_names = [f.name for f in dataclasses.fields(dynamic_point_factory)]
    assert point_field_names == ['x', 'y'], 'make_dataclass should preserve field order for string specs'
    assert dataclasses.asdict(dynamic_point) == {'x': 1, 'y': 2}, 'make_dataclass class instances should support asdict'

    dynamic_user = dynamic_user_factory(name='Ada', age=36)
    user_field_names = [f.name for f in dataclasses.fields(dynamic_user_factory)]
    assert user_field_names == ['name', 'age'], 'make_dataclass should support tuple field specs'

    dynamic_status = dynamic_status_factory(active=True, score=7)
    status_field_names = [f.name for f in dataclasses.fields(dynamic_status_factory)]
    assert status_field_names == ['active', 'score'], 'make_dataclass should support list field specs'
    assert dataclasses.astuple(dynamic_status) == (True, 7), 'list field specs should preserve tuple field order'

    updated_dynamic_user = dataclasses.replace(dynamic_user, age=37)
    assert dataclasses.asdict(updated_dynamic_user) == {'name': 'Ada', 'age': 37}, (
        'replace should work on make_dataclass instances'
    )
    assert dataclasses.asdict(dynamic_user) == {'name': 'Ada', 'age': 36}, (
        'replace should not mutate make_dataclass original'
    )
else:
    # Ouros behavior: make_dataclass returns a dataclass-like instance.
    dynamic_point = dynamic_point_factory
    dynamic_user = dynamic_user_factory
    dynamic_status = dynamic_status_factory

    assert dataclasses.is_dataclass(dynamic_point) == True, 'make_dataclass should return dataclass-like values'
    assert [f.name for f in dataclasses.fields(dynamic_point)] == ['x', 'y'], (
        'make_dataclass should preserve field order for string specs'
    )
    assert dataclasses.asdict(dynamic_point) == {'x': None, 'y': None}, (
        'make_dataclass should initialize fields to None'
    )
    assert [f.name for f in dataclasses.fields(dynamic_user)] == ['name', 'age'], 'make_dataclass should support tuple field specs'
    assert dataclasses.asdict(dynamic_user) == {'name': None, 'age': None}, (
        'tuple field specs should initialize to None'
    )
    assert [f.name for f in dataclasses.fields(dynamic_status)] == ['active', 'score'], 'make_dataclass should support list field specs'
    assert dataclasses.astuple(dynamic_status) == (None, None), 'list field specs should preserve tuple field order'

    updated_dynamic_user = dataclasses.replace(dynamic_user, name='Ada', age=36)
    assert dataclasses.asdict(updated_dynamic_user) == {'name': 'Ada', 'age': 36}, (
        'replace should work on make_dataclass output'
    )
    assert dataclasses.asdict(dynamic_user) == {'name': None, 'age': None}, (
        'replace should not mutate make_dataclass original'
    )

# === make_dataclass and replace error cases ===
duplicate_field_error = False
try:
    dataclasses.make_dataclass('DupField', ['x', ('x', int)])
except TypeError:
    duplicate_field_error = True
assert duplicate_field_error, 'make_dataclass should reject duplicate field names'

replace_positional_error = False
try:
    dataclasses.replace(dynamic_user, dynamic_point)
except TypeError:
    replace_positional_error = True
assert replace_positional_error, 'replace should reject extra positional arguments'

# === decorator parity: ClassVar / InitVar / tuple annotations ===
from typing import ClassVar


@dataclasses.dataclass
class ClassVarInitVarCase:
    coords: tuple[int, int]
    token: dataclasses.InitVar[int]
    category: ClassVar[str] = 'meta'
    seen: int = 0

    def __post_init__(self, token):
        self.seen = token


case = ClassVarInitVarCase((1, 2), 7)
case_fields = dataclasses.fields(ClassVarInitVarCase)
case_field_names = [f.name for f in case_fields]
assert case_field_names == ['coords', 'seen'], 'fields() should exclude InitVar and ClassVar'
assert dataclasses.asdict(case) == {'coords': (1, 2), 'seen': 7}, 'asdict should exclude InitVar/ClassVar fields'
assert dataclasses.astuple(case) == ((1, 2), 7), 'astuple should preserve tuple-typed field and exclude InitVar/ClassVar'
assert 'token' not in case.__dict__, 'InitVar should not become an instance attribute'
assert ClassVarInitVarCase.category == 'meta', 'ClassVar value should remain a class attribute'

# === mutable default rejection ===
mutable_list_default_error = False
try:
    @dataclasses.dataclass
    class MutableListDefault:
        xs: list = []
except ValueError:
    mutable_list_default_error = True
assert mutable_list_default_error, 'mutable list defaults should require default_factory'

mutable_dict_default_error = False
try:
    @dataclasses.dataclass
    class MutableDictDefault:
        xs: dict = {}
except ValueError:
    mutable_dict_default_error = True
assert mutable_dict_default_error, 'mutable dict defaults should require default_factory'

# === dataclass comparison dunders should return NotImplemented for mismatched types ===
@dataclasses.dataclass(order=True)
class OrderedLeft:
    value: int


@dataclasses.dataclass(order=True)
class OrderedRight:
    value: int


ordered_left = OrderedLeft(1)
ordered_right = OrderedRight(1)
assert OrderedLeft.__eq__(ordered_left, ordered_right) is NotImplemented, '__eq__ should return NotImplemented on type mismatch'
assert OrderedLeft.__lt__(ordered_left, ordered_right) is NotImplemented, '__lt__ should return NotImplemented on type mismatch'
assert (ordered_left == ordered_right) == False, 'equality should still evaluate to False for mismatched dataclass types'

# === replace() parity for InitVar and init=False fields ===
@dataclasses.dataclass
class ReplaceParityCase:
    x: int
    token: dataclasses.InitVar[int]
    y: int = dataclasses.field(init=False, default=0)

    def __post_init__(self, token):
        self.y = token


replace_base = ReplaceParityCase(1, 5)
replace_requires_initvar = False
try:
    dataclasses.replace(replace_base, x=2)
except TypeError:
    replace_requires_initvar = True
assert replace_requires_initvar, 'replace() should require InitVar values without defaults'

replace_init_false_error = False
try:
    dataclasses.replace(replace_base, x=2, token=9, y=3)
except TypeError:
    replace_init_false_error = True
assert replace_init_false_error, 'replace() should reject fields declared with init=False'

replace_ok = dataclasses.replace(replace_base, x=2, token=9)
assert dataclasses.asdict(replace_ok) == {'x': 2, 'y': 9}, 'replace() should rebuild non-init fields via __post_init__'
