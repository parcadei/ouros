# Tests for collections.abc public API and core compatibility behavior

# === Module import ===
import collections.abc as cabc

# === Public API presence ===
expected_public_names = [
    'ABCMeta',
    'AsyncGenerator',
    'AsyncIterable',
    'AsyncIterator',
    'Awaitable',
    'Buffer',
    'ByteString',
    'Callable',
    'Collection',
    'Container',
    'Coroutine',
    'EllipsisType',
    'FunctionType',
    'Generator',
    'GenericAlias',
    'Hashable',
    'ItemsView',
    'Iterable',
    'Iterator',
    'KeysView',
    'Mapping',
    'MappingView',
    'MutableMapping',
    'MutableSequence',
    'MutableSet',
    'Reversible',
    'Sequence',
    'Set',
    'Sized',
    'ValuesView',
    'abstractmethod',
    'async_generator',
    'bytearray_iterator',
    'bytes_iterator',
    'coroutine',
    'dict_itemiterator',
    'dict_items',
    'dict_keyiterator',
    'dict_keys',
    'dict_valueiterator',
    'dict_values',
    'framelocalsproxy',
    'generator',
    'list_iterator',
    'list_reverseiterator',
    'longrange_iterator',
    'mappingproxy',
    'range_iterator',
    'set_iterator',
    'str_iterator',
    'sys',
    'tuple_iterator',
    'zip_iterator',
]

for name in expected_public_names:
    assert hasattr(cabc, name), f'collections.abc should export {name}'

# === Module-level bindings ===
assert cabc.sys is not None, 'collections.abc.sys should be available'
assert hasattr(cabc.sys, 'platform'), 'collections.abc.sys should expose sys-like attributes'


# === Decorator behavior ===
def sample_function():
    return 42


decorated = cabc.abstractmethod(sample_function)
assert decorated is sample_function, 'abstractmethod should return the original function'
assert getattr(decorated, '__isabstractmethod__', False) is True, 'abstractmethod should mark function as abstract'

try:
    cabc.abstractmethod()
except TypeError:
    pass
else:
    assert False, 'abstractmethod should raise TypeError when called without arguments'

# === Core type aliases ===
assert cabc.EllipsisType is type(...), 'EllipsisType should match ellipsis literal type'
assert cabc.FunctionType is type(sample_function), 'FunctionType should match function type'
assert isinstance(cabc.GenericAlias, type), 'GenericAlias should be a type-like object'
assert isinstance(cabc.mappingproxy, type), 'mappingproxy should be a type-like object'
assert isinstance(cabc.generator, type), 'generator should be a type-like object'
assert isinstance(cabc.coroutine, type), 'coroutine should be a type-like object'
assert isinstance(cabc.async_generator, type), 'async_generator should be a type-like object'
assert isinstance(cabc.framelocalsproxy, type), 'framelocalsproxy should be a type-like object'

# === Concrete runtime checks ===
d = {'a': 1, 'b': 2}
assert isinstance(d.keys(), cabc.dict_keys), 'dict_keys should match dict.keys() type'
assert isinstance(d.values(), cabc.dict_values), 'dict_values should match dict.values() type'
assert isinstance(d.items(), cabc.dict_items), 'dict_items should match dict.items() type'

assert isinstance(iter([1]), cabc.list_iterator), 'list_iterator should match iter(list)'
assert isinstance(iter((1,)), cabc.tuple_iterator), 'tuple_iterator should match iter(tuple)'
assert isinstance(iter(range(3)), cabc.range_iterator), 'range_iterator should match iter(range)'
assert isinstance(iter({'k': 1}.keys()), cabc.dict_keyiterator), 'dict_keyiterator should match iter(dict.keys())'
assert isinstance(iter({'k': 1}.values()), cabc.dict_valueiterator), (
    'dict_valueiterator should match iter(dict.values())'
)
assert isinstance(iter({'k': 1}.items()), cabc.dict_itemiterator), 'dict_itemiterator should match iter(dict.items())'
assert isinstance(iter({1, 2}), cabc.set_iterator), 'set_iterator should match iter(set)'
assert isinstance(iter('ab'), cabc.str_iterator), 'str_iterator should match iter(str)'
assert isinstance(iter(b'ab'), cabc.bytes_iterator), 'bytes_iterator should match iter(bytes)'
assert isinstance(iter(bytearray(b'ab')), cabc.bytearray_iterator), 'bytearray_iterator should match iter(bytearray)'
assert isinstance(cabc.list_reverseiterator, type), 'list_reverseiterator should be a type-like object'
assert isinstance(cabc.longrange_iterator, type), 'longrange_iterator should be a type-like object'
assert isinstance(cabc.zip_iterator, type), 'zip_iterator should be a type-like object'


# === Generator instance type ===
def make_generator():
    yield 1


assert isinstance(make_generator(), cabc.generator), 'generator should match generator instance type'
