import types

# === Public API Surface ===
public_names = [
    'AsyncGeneratorType',
    'BuiltinFunctionType',
    'BuiltinMethodType',
    'CapsuleType',
    'CellType',
    'ClassMethodDescriptorType',
    'CodeType',
    'CoroutineType',
    'DynamicClassAttribute',
    'EllipsisType',
    'FrameType',
    'FunctionType',
    'GeneratorType',
    'GenericAlias',
    'GetSetDescriptorType',
    'LambdaType',
    'MappingProxyType',
    'MemberDescriptorType',
    'MethodDescriptorType',
    'MethodType',
    'MethodWrapperType',
    'ModuleType',
    'NoneType',
    'NotImplementedType',
    'SimpleNamespace',
    'TracebackType',
    'UnionType',
    'WrapperDescriptorType',
    'coroutine',
    'get_original_bases',
    'new_class',
    'prepare_class',
    'resolve_bases',
]
for name in public_names:
    assert hasattr(types, name), f'types should expose {name}'

# === Alias Relationships ===
assert types.BuiltinMethodType is types.BuiltinFunctionType, 'BuiltinMethodType alias parity'
assert types.LambdaType is types.FunctionType, 'LambdaType alias parity'

# === MethodType Constructor ===
def _id(self):
    return self

obj = object()
bound = types.MethodType(_id, obj)
assert bound() is obj, 'MethodType should bind instance as first argument'
assert bound.__self__ is obj, 'MethodType should expose __self__'
assert bound.__func__ is _id, 'MethodType should expose __func__'

try:
    types.MethodType(1, obj)
    assert False, 'MethodType should reject non-callable first argument'
except TypeError as exc:
    assert str(exc) == 'first argument must be callable', 'MethodType non-callable error message'

try:
    types.MethodType(_id, None)
    assert False, 'MethodType should reject None instance'
except TypeError as exc:
    assert str(exc) == 'instance must not be None', 'MethodType None instance error message'

# === coroutine ===
wrapped = types.coroutine(_id)
assert callable(wrapped), 'types.coroutine should return a callable wrapper'
assert wrapped(1) == 1, 'types.coroutine wrapper should preserve call behavior'

try:
    types.coroutine(1)
    assert False, 'types.coroutine should reject non-callables'
except TypeError as exc:
    assert str(exc) == 'types.coroutine() expects a callable', 'types.coroutine error message'

# === get_original_bases ===
class Base:
    pass


class Child(Base):
    pass

orig = types.get_original_bases(Child)
assert isinstance(orig, tuple), 'get_original_bases should return a tuple for normal classes'
assert len(orig) == 1 and orig[0] is Base, 'get_original_bases should return __bases__ fallback'

try:
    types.get_original_bases(1)
    assert False, 'get_original_bases should reject non-type arguments'
except TypeError as exc:
    assert str(exc) == "Expected an instance of type, not 'int'", 'get_original_bases non-type error'

# === resolve_bases ===
base_tuple = (int, str)
resolved_tuple = types.resolve_bases(base_tuple)
assert isinstance(resolved_tuple, tuple), 'resolve_bases tuple should stay tuple when unchanged'
assert resolved_tuple == base_tuple, 'resolve_bases should preserve unchanged tuple entries'

base_list = [int, str]
resolved_list = types.resolve_bases(base_list)
assert isinstance(resolved_list, list), 'resolve_bases list should stay list when unchanged'
assert resolved_list == base_list, 'resolve_bases should preserve unchanged list entries'

try:
    types.resolve_bases(1)
    assert False, 'resolve_bases should reject non-iterables'
except TypeError as exc:
    assert str(exc) == "'int' object is not iterable", 'resolve_bases non-iterable error'

# === prepare_class ===
meta, ns, kw = types.prepare_class('Prepared')
assert callable(meta), 'prepare_class should return callable metaclass'
assert isinstance(ns, dict), 'prepare_class should return dict namespace by default'
assert isinstance(kw, dict) and kw == {}, 'prepare_class should return cleaned kwargs dict'

meta2, ns2, kw2 = types.prepare_class('Prepared2', (), {'metaclass': type, 'flag': 1})
assert callable(meta2), 'prepare_class should keep explicit metaclass'
assert isinstance(ns2, dict), 'prepare_class namespace should remain dict with explicit metaclass'
assert kw2 == {'flag': 1}, 'prepare_class should remove metaclass from kwargs output'

# === new_class ===
Dynamic = types.new_class('Dynamic')
assert Dynamic.__name__ == 'Dynamic', 'new_class should create class with provided name'
assert Dynamic.__mro__[1].__name__ == 'object', 'new_class should default base to object'

SubDynamic = types.new_class('SubDynamic', (Dynamic,))
assert SubDynamic.__mro__[1] is Dynamic, 'new_class should honor explicit bases'

try:
    types.new_class('BadExec', (), {}, 1)
    assert False, 'new_class should reject non-callable exec_body'
except TypeError as exc:
    assert str(exc) == "'int' object is not callable", 'new_class exec_body non-callable error'

# === SimpleNamespace ===
ns0 = types.SimpleNamespace()
assert repr(ns0) == 'namespace()', 'SimpleNamespace empty repr'

ns1 = types.SimpleNamespace(a=1)
assert repr(ns1) == 'namespace(a=1)', 'SimpleNamespace keyword repr'
assert ns1.a == 1, 'SimpleNamespace attribute access'

ns2 = types.SimpleNamespace({'a': 1}, b=2)
assert ns2.a == 1 and ns2.b == 2, 'SimpleNamespace mapping + kwargs merge'
assert repr(ns2) == 'namespace(a=1, b=2)', 'SimpleNamespace merged repr order'

try:
    types.SimpleNamespace({1: 2})
    assert False, 'SimpleNamespace should reject non-string mapping keys'
except TypeError as exc:
    assert str(exc) == 'keywords must be strings', 'SimpleNamespace non-string key error message'

try:
    types.SimpleNamespace(1, 2)
    assert False, 'SimpleNamespace should reject too many positional args'
except TypeError as exc:
    assert str(exc) == 'SimpleNamespace expected at most 1 argument, got 2', 'SimpleNamespace arg count error'
