import typing_extensions

# === Public API surface ===
expected_public_names = [
    'AbstractSet',
    'Annotated',
    'Any',
    'AnyStr',
    'AsyncContextManager',
    'AsyncGenerator',
    'AsyncIterable',
    'AsyncIterator',
    'Awaitable',
    'BinaryIO',
    'Buffer',
    'Callable',
    'CapsuleType',
    'ChainMap',
    'ClassVar',
    'Collection',
    'Concatenate',
    'Container',
    'ContextManager',
    'Coroutine',
    'Counter',
    'DefaultDict',
    'Deque',
    'Dict',
    'Doc',
    'Final',
    'Format',
    'ForwardRef',
    'FrozenSet',
    'Generator',
    'Generic',
    'GenericMeta',
    'Hashable',
    'IO',
    'IntVar',
    'ItemsView',
    'Iterable',
    'Iterator',
    'KT',
    'KeysView',
    'List',
    'Literal',
    'LiteralString',
    'Mapping',
    'MappingView',
    'Match',
    'MutableMapping',
    'MutableSequence',
    'MutableSet',
    'NamedTuple',
    'Never',
    'NewType',
    'NoDefault',
    'NoExtraItems',
    'NoReturn',
    'NotRequired',
    'Optional',
    'OrderedDict',
    'PEP_560',
    'ParamSpec',
    'ParamSpecArgs',
    'ParamSpecKwargs',
    'Pattern',
    'Protocol',
    'ReadOnly',
    'Reader',
    'Required',
    'Reversible',
    'Self',
    'Sentinel',
    'Sequence',
    'Set',
    'Sized',
    'SupportsAbs',
    'SupportsBytes',
    'SupportsComplex',
    'SupportsFloat',
    'SupportsIndex',
    'SupportsInt',
    'SupportsRound',
    'T',
    'TYPE_CHECKING',
    'T_co',
    'T_contra',
    'Text',
    'TextIO',
    'Tuple',
    'Type',
    'TypeAlias',
    'TypeAliasType',
    'TypeForm',
    'TypeGuard',
    'TypeIs',
    'TypeVar',
    'TypeVarTuple',
    'TypedDict',
    'Union',
    'Unpack',
    'VT',
    'ValuesView',
    'Writer',
    'abc',
    'annotationlib',
    'assert_never',
    'assert_type',
    'builtins',
    'cast',
    'clear_overloads',
    'collections',
    'contextlib',
    'dataclass_transform',
    'deprecated',
    'disjoint_base',
    'enum',
    'evaluate_forward_ref',
    'final',
    'functools',
    'get_annotations',
    'get_args',
    'get_origin',
    'get_original_bases',
    'get_overloads',
    'get_protocol_members',
    'get_type_hints',
    'inspect',
    'io',
    'is_protocol',
    'is_typeddict',
    'keyword',
    'no_type_check',
    'no_type_check_decorator',
    'operator',
    'overload',
    'override',
    'reveal_type',
    'runtime',
    'runtime_checkable',
    'sys',
    'type_repr',
    'typing',
    'warnings',
]

missing_names = [name for name in expected_public_names if not hasattr(typing_extensions, name)]
assert missing_names == [], f'missing typing_extensions names: {missing_names}'

dir_names = dir(typing_extensions)
for name in expected_public_names:
    assert name in dir_names, f'{name} should appear in dir(typing_extensions)'

# === Basic invariants ===
assert typing_extensions.PEP_560 == True, 'PEP_560 should be True'
assert typing_extensions.runtime is typing_extensions.runtime_checkable, 'runtime should alias runtime_checkable'
assert typing_extensions.type_repr(int) == 'int', 'type_repr(int) should be int'
assert typing_extensions.type_repr(str) == 'str', 'type_repr(str) should be str'
assert typing_extensions.NoExtraItems is not None, 'NoExtraItems should exist'
assert typing_extensions.Format.VALUE == 1, 'Format.VALUE should be 1'
assert typing_extensions.Format.STRING == 4, 'Format.STRING should be 4'

# === Module-valued exports ===
for module_name in [
    'abc',
    'annotationlib',
    'builtins',
    'collections',
    'contextlib',
    'enum',
    'functools',
    'inspect',
    'io',
    'keyword',
    'operator',
    'sys',
    'typing',
    'warnings',
]:
    module_value = getattr(typing_extensions, module_name)
    assert module_value is not None, f'{module_name} should be present'

# === TypeVar-like exports ===
for symbol_name in ['T', 'KT', 'VT', 'T_co', 'T_contra']:
    symbol_value = getattr(typing_extensions, symbol_name)
    assert hasattr(symbol_value, '__name__'), f'{symbol_name} should have __name__'
    assert symbol_value.__name__ == symbol_name, f'{symbol_name} should preserve symbol name'

int_var = typing_extensions.IntVar('MyInt')
assert hasattr(int_var, '__name__'), 'IntVar result should have __name__'
assert int_var.__name__ == 'MyInt', 'IntVar should preserve the provided name'
assert isinstance(int_var.__constraints__, tuple), 'IntVar __constraints__ should be tuple'

# === Runtime helper functions ===
assert typing_extensions.cast(int, 'x') == 'x', 'cast should return value unchanged'
sample = ['a', 'b']
assert typing_extensions.assert_type(sample, typing_extensions.Sequence) is sample, 'assert_type should be identity'
assert typing_extensions.reveal_type(sample) is sample, 'reveal_type should return original value'
assert typing_extensions.get_origin(int) is None, 'get_origin(int) should be None'
assert typing_extensions.get_args(int) == (), 'get_args(int) should be empty tuple'

annotation_result = None


def annotated_func(x: int) -> str:
    return str(x)


annotation_result = typing_extensions.get_annotations(annotated_func)
assert annotation_result == {'x': int, 'return': str}, 'get_annotations should return __annotations__ mapping'

forward_ref = typing_extensions.ForwardRef('int')
assert typing_extensions.evaluate_forward_ref(forward_ref) is int, 'evaluate_forward_ref should resolve builtins'

assert_never_message = None
try:
    typing_extensions.assert_never(123)
except AssertionError as exc:
    assert_never_message = exc.args[0]
assert (
    assert_never_message == 'Expected code to be unreachable, but got: 123'
), 'assert_never should raise AssertionError with CPython-compatible text'

# === Decorator helpers ===


@typing_extensions.disjoint_base
class DisjointBaseType:
    pass


assert DisjointBaseType.__name__ == 'DisjointBaseType', 'disjoint_base should return class unchanged'


@typing_extensions.deprecated('legacy API')
def legacy_fn(x):
    return x + 1


assert callable(legacy_fn), 'deprecated decorator should return a callable'

# === Class-ish helper constructors ===
doc = typing_extensions.Doc('some documentation')
assert doc.documentation == 'some documentation', 'Doc should expose documentation attribute'

sentinel_a = typing_extensions.Sentinel('TOKEN')
sentinel_b = typing_extensions.Sentinel('TOKEN')
assert sentinel_a is not sentinel_b, 'Sentinel should produce unique instances'
assert typing_extensions.TypeForm(int) is int, 'TypeForm should be runtime identity'

# === TypedDict and Protocol helpers ===


class Proto(typing_extensions.Protocol):
    def run(self):
        pass


assert typing_extensions.is_protocol(Proto) == True, 'is_protocol should recognize protocol classes'


@typing_extensions.runtime_checkable
class RuntimeProto(typing_extensions.Protocol):
    def run(self):
        pass


class Runner:
    def run(self):
        return 'ok'


assert isinstance(Runner(), RuntimeProto) == True, 'runtime_checkable protocols should support isinstance'


class TD(typing_extensions.TypedDict):
    value: int


assert typing_extensions.is_typeddict(TD) == True, 'is_typeddict should recognize TypedDict classes'
td_instance = TD(value=10)
assert td_instance == {'value': 10}, 'TypedDict class should construct dict values'

# === Misc helpers ===


class PlainClass:
    pass


original_bases = typing_extensions.get_original_bases(PlainClass)
assert isinstance(original_bases, tuple), 'get_original_bases should return a tuple'
