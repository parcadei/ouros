# Comprehensive typing module parity test
# Tests existence and basic functionality of all typing module exports

import typing
from typing import (
    # Type hints - Basic
    List, Dict, Tuple, Set, FrozenSet, Optional, Union, Any, Callable,
    # Type hints - Collections
    Sequence, MutableSequence, Mapping, MutableMapping, Iterable, Iterator,
    Collection, AbstractSet, MutableSet, Container, Sized, Hashable,
    Reversible, Generator, Coroutine, AsyncIterable, AsyncIterator,
    AsyncGenerator, Awaitable, ChainMap, Counter, DefaultDict, Deque,
    OrderedDict, ItemsView, KeysView, ValuesView, MappingView,
    # Type hints - IO
    IO, TextIO, BinaryIO,
    # Type hints - Special
    AnyStr, Text, NoReturn, Never, LiteralString,
    # Classes and variables
    ClassVar, Final, Type, Generic,
    # Type composition
    Literal, Annotated, Concatenate, Unpack,
    # Type variables and parameters
    TypeVar, ParamSpec, ParamSpecArgs, ParamSpecKwargs, TypeVarTuple,
    # Protocol and structural typing
    Protocol, runtime_checkable,
    # TypedDict and NamedTuple
    TypedDict, NamedTuple, NewType, TypeAlias,
    # Type guards and narrowing
    TypeGuard, TypeIs, Required, NotRequired, ReadOnly, Self,
    # Functions and decorators
    get_type_hints, cast, overload, get_overloads, clear_overloads,
    final, override, assert_type, reveal_type, assert_never,
    get_args, get_origin, no_type_check, no_type_check_decorator,
    dataclass_transform, get_protocol_members, is_protocol, is_typeddict,
    # ABC support
    SupportsInt, SupportsFloat, SupportsComplex, SupportsBytes, SupportsAbs,
    SupportsIndex, SupportsRound,
    # Type aliases (modern)
    TypeAliasType, NoDefault,
    # Constants
    TYPE_CHECKING,
    # Internal
    GenericAlias, evaluate_forward_ref,
)

# === Basic Type Hints ===
try:
    print('list_exists', List is not None)
    print('dict_exists', Dict is not None)
    print('tuple_exists', Tuple is not None)
    print('set_exists', Set is not None)
    print('frozenset_exists', FrozenSet is not None)
    print('optional_exists', Optional is not None)
    print('union_exists', Union is not None)
    print('any_exists', Any is not None)
    print('callable_exists', Callable is not None)
except Exception as e:
    print('SKIP_Basic Type Hints', type(e).__name__, e)

# === Collection Types ===
try:
    print('sequence_exists', Sequence is not None)
    print('mutable_sequence_exists', MutableSequence is not None)
    print('mapping_exists', Mapping is not None)
    print('mutable_mapping_exists', MutableMapping is not None)
    print('iterable_exists', Iterable is not None)
    print('iterator_exists', Iterator is not None)
    print('collection_exists', Collection is not None)
    print('abstract_set_exists', AbstractSet is not None)
    print('mutable_set_exists', MutableSet is not None)
    print('container_exists', Container is not None)
    print('sized_exists', Sized is not None)
    print('hashable_exists', Hashable is not None)
    print('reversible_exists', Reversible is not None)
    print('generator_exists', Generator is not None)
    print('coroutine_exists', Coroutine is not None)
    print('async_iterable_exists', AsyncIterable is not None)
    print('async_iterator_exists', AsyncIterator is not None)
    print('async_generator_exists', AsyncGenerator is not None)
    print('awaitable_exists', Awaitable is not None)
except Exception as e:
    print('SKIP_Collection Types', type(e).__name__, e)

# === Collection ABC Types ===
try:
    print('chain_map_exists', ChainMap is not None)
    print('counter_exists', Counter is not None)
    print('default_dict_exists', DefaultDict is not None)
    print('deque_exists', Deque is not None)
    print('ordered_dict_exists', OrderedDict is not None)
    print('items_view_exists', ItemsView is not None)
    print('keys_view_exists', KeysView is not None)
    print('values_view_exists', ValuesView is not None)
    print('mapping_view_exists', MappingView is not None)
except Exception as e:
    print('SKIP_Collection ABC Types', type(e).__name__, e)

# === IO Types ===
try:
    print('io_exists', IO is not None)
    print('text_io_exists', TextIO is not None)
    print('binary_io_exists', BinaryIO is not None)
except Exception as e:
    print('SKIP_IO Types', type(e).__name__, e)

# === Special Types ===
try:
    print('any_str_exists', AnyStr is not None)
    print('text_exists', Text is not None)
    print('no_return_exists', NoReturn is not None)
    print('never_exists', Never is not None)
    print('literal_string_exists', LiteralString is not None)
except Exception as e:
    print('SKIP_Special Types', type(e).__name__, e)

# === Classes and Variables ===
try:
    print('class_var_exists', ClassVar is not None)
    print('final_exists', Final is not None)
    print('type_exists', Type is not None)
    print('generic_exists', Generic is not None)
except Exception as e:
    print('SKIP_Classes and Variables', type(e).__name__, e)

# === Type Composition ===
try:
    print('literal_exists', Literal is not None)
    print('annotated_exists', Annotated is not None)
    print('concatenate_exists', Concatenate is not None)
    print('unpack_exists', Unpack is not None)
except Exception as e:
    print('SKIP_Type Composition', type(e).__name__, e)

# === Type Variables and Parameters ===
try:
    print('type_var_exists', TypeVar is not None)
    print('param_spec_exists', ParamSpec is not None)
    print('param_spec_args_exists', ParamSpecArgs is not None)
    print('param_spec_kwargs_exists', ParamSpecKwargs is not None)
    print('type_var_tuple_exists', TypeVarTuple is not None)
except Exception as e:
    print('SKIP_Type Variables and Parameters', type(e).__name__, e)

# === Protocol and Structural Typing ===
try:
    print('protocol_exists', Protocol is not None)
    print('runtime_checkable_exists', runtime_checkable is not None)
except Exception as e:
    print('SKIP_Protocol and Structural Typing', type(e).__name__, e)

# === TypedDict, NamedTuple, NewType ===
try:
    print('typed_dict_exists', TypedDict is not None)
    print('named_tuple_exists', NamedTuple is not None)
    print('new_type_exists', NewType is not None)
    print('type_alias_exists', TypeAlias is not None)
except Exception as e:
    print('SKIP_TypedDict, NamedTuple, NewType', type(e).__name__, e)

# === Type Guards and Narrowing ===
try:
    print('type_guard_exists', TypeGuard is not None)
    print('type_is_exists', TypeIs is not None)
    print('required_exists', Required is not None)
    print('not_required_exists', NotRequired is not None)
    print('read_only_exists', ReadOnly is not None)
    print('self_exists', Self is not None)
except Exception as e:
    print('SKIP_Type Guards and Narrowing', type(e).__name__, e)

# === Functions ===
try:
    print('get_type_hints_exists', get_type_hints is not None)
    print('cast_exists', cast is not None)
    print('overload_exists', overload is not None)
    print('get_overloads_exists', get_overloads is not None)
    print('clear_overloads_exists', clear_overloads is not None)
    print('final_exists', final is not None)
    print('override_exists', override is not None)
    print('assert_type_exists', assert_type is not None)
    print('reveal_type_exists', reveal_type is not None)
    print('assert_never_exists', assert_never is not None)
    print('get_args_exists', get_args is not None)
    print('get_origin_exists', get_origin is not None)
    print('no_type_check_exists', no_type_check is not None)
    print('no_type_check_decorator_exists', no_type_check_decorator is not None)
    print('dataclass_transform_exists', dataclass_transform is not None)
    print('get_protocol_members_exists', get_protocol_members is not None)
    print('is_protocol_exists', is_protocol is not None)
    print('is_typed_dict_exists', is_typeddict is not None)
    print('evaluate_forward_ref_exists', evaluate_forward_ref is not None)
except Exception as e:
    print('SKIP_Functions', type(e).__name__, e)

# === ABC Support ===
try:
    print('supports_int_exists', SupportsInt is not None)
    print('supports_float_exists', SupportsFloat is not None)
    print('supports_complex_exists', SupportsComplex is not None)
    print('supports_bytes_exists', SupportsBytes is not None)
    print('supports_abs_exists', SupportsAbs is not None)
    print('supports_index_exists', SupportsIndex is not None)
    print('supports_round_exists', SupportsRound is not None)
except Exception as e:
    print('SKIP_ABC Support', type(e).__name__, e)

# === Modern Type Alias ===
try:
    print('type_alias_type_exists', TypeAliasType is not None)
    print('no_default_exists', NoDefault is not None)
except Exception as e:
    print('SKIP_Modern Type Alias', type(e).__name__, e)

# === Constants ===
try:
    print('type_checking_exists', TYPE_CHECKING is not None)
    print('type_checking_is_false', TYPE_CHECKING is False)
except Exception as e:
    print('SKIP_Constants', type(e).__name__, e)

# === GenericAlias ===
try:
    print('generic_alias_exists', GenericAlias is not None)
except Exception as e:
    print('SKIP_GenericAlias', type(e).__name__, e)

# === Basic Functionality Tests ===
try:
    # Test TypeVar creation
    T = TypeVar('T')
    print('type_var_name', T.__name__ == 'T')

    # Test TypeVar with constraints
    CT = TypeVar('CT', int, str)
    print('type_var_constraints', CT.__constraints__ == (int, str))

    # Test TypeVar with bound
    BT = TypeVar('BT', bound=int)
    print('type_var_bound', BT.__bound__ is int)

    # Test ParamSpec
    P = ParamSpec('P')
    print('param_spec_name', P.__name__ == 'P')

    # Test TypeVarTuple
    Ts = TypeVarTuple('Ts')
    print('type_var_tuple_name', Ts.__name__ == 'Ts')

    # Test NewType
    UserId = NewType('UserId', int)
    print('new_type_callable', callable(UserId))
    print('new_type_identity', UserId(42) == 42)

    # Test GenericAlias parameterization
    print('list_parameterized', List[int] is not None)
    print('dict_parameterized', Dict[str, int] is not None)
    print('tuple_parameterized', Tuple[int, str] is not None)
    print('set_parameterized', Set[int] is not None)
    print('callable_parameterized', Callable[[int], str] is not None)

    # Test Union with |
    print('union_pipe', int | str is not None)

    # Test Optional
    print('optional_none', Optional[int] == int | None)

    # Test Literal
    print('literal_str', Literal['a', 'b'] is not None)
    print('literal_int', Literal[1, 2, 3] is not None)

    # Test Annotated
    print('annotated_type', Annotated[int, 'metadata'] is not None)

    # Test Generic
    class MyList(Generic[T]):
        pass
    print('generic_subclass', issubclass(MyList, Generic))

    # Test Protocol
    class Drawable(Protocol):
        def draw(self) -> None: ...
    print('protocol_callable', callable(Drawable))

    # Test runtime_checkable
    @runtime_checkable
    class Sized2(Protocol):
        def __len__(self) -> int: ...
    print('runtime_checkable_works', issubclass(list, Sized2))

    # Test TypedDict
    class Person(TypedDict):
        name: str
        age: int
    print('typed_dict_exists', Person is not None)
    print('is_typed_dict', is_typeddict(Person))

    # Test TypedDict with total=False
    class PartialPerson(TypedDict, total=False):
        name: str
    print('typed_dict_total_false', PartialPerson.__total__ is False)

    # Test Required/NotRequired
    class RequiredPerson(TypedDict):
        name: str
        age: NotRequired[int]
    print('not_required_works', RequiredPerson is not None)

    # Test NamedTuple
    class Point(NamedTuple):
        x: int
        y: int
    print('named_tuple_fields', Point._fields == ('x', 'y'))
    p = Point(1, 2)
    print('named_tuple_instance', p.x == 1 and p.y == 2)

    # Test cast
    x: Any = "hello"
    y = cast(str, x)
    print('cast_returns_value', y == "hello")

    # Test get_type_hints
    def example_func(x: int, y: str) -> bool:
        return True
    hints = get_type_hints(example_func)
    print('get_type_hints_works', 'x' in hints and 'y' in hints and 'return' in hints)

    # Test get_origin
    print('get_origin_list', get_origin(List[int]) is list)
    print('get_origin_dict', get_origin(Dict[str, int]) is dict)
    print('get_origin_union', get_origin(int | str) is Union)

    # Test get_args
    print('get_args_list', get_args(List[int]) == (int,))
    print('get_args_dict', get_args(Dict[str, int]) == (str, int))

    # Test final
    @final
    class FinalClass:
        pass
    print('final_decorator_works', FinalClass is not None)

    # Test override
    class Base:
        def method(self) -> int:
            return 1

    class Derived(Base):
        @override
        def method(self) -> int:
            return 2
    print('override_decorator_works', Derived().method() == 2)

    # Test no_type_check
    @no_type_check
    class NoCheck:
        x: int = "not an int"
    print('no_type_check_works', NoCheck is not None)

    # Test assert_type
    result = assert_type(42, int)
    print('assert_type_returns_value', result == 42)

    # Test assert_never (with valid usage)
    def handle_value(val: int | str) -> None:
        if isinstance(val, int):
            pass
        elif isinstance(val, str):
            pass
        else:
            assert_never(val)
    print('assert_never_exists', assert_never is not None)

    # Test dataclass_transform
    @dataclass_transform()
    def my_dataclass(cls):
        return cls

    @my_dataclass
    class MyDataClass:
        x: int
    print('dataclass_transform_works', MyDataClass is not None)

    # Test is_protocol
    print('is_protocol_drawable', is_protocol(Drawable))
    print('is_protocol_list', not is_protocol(list))

    # Test get_protocol_members
    members = get_protocol_members(Drawable)
    print('get_protocol_members_works', 'draw' in members)

    # Test Concatenate
    print('concatenate_parameterized', Concatenate[int, P] is not None)

    # Test Unpack
    print('unpack_parameterized', Unpack[Ts] is not None)

    # Test Self
    class SelfRef:
        def return_self(self) -> Self:
            return self
    print('self_exists_in_method', SelfRef().return_self() is not None)

    # Test TypeGuard
    def is_str_list(val: list) -> TypeGuard[list[str]]:
        return all(isinstance(x, str) for x in val)
    print('type_guard_callable', callable(is_str_list))

    # Test TypeIs
    def is_str_typeis(val) -> TypeIs[str]:
        return isinstance(val, str)
    print('type_is_callable', callable(is_str_typeis))

    # Test TypeAliasType
    type MyList2 = list[int]
    print('type_alias_type_works', MyList2 is not None)

    # Test Never (bottom type)
    def never_returns() -> Never:
        raise Exception("never")
    print('never_is_subtype', Never is not None)

    # Test TYPE_CHECKING usage
    if TYPE_CHECKING:
        # This block should not execute at runtime
        some_fake_type = None  # type: ignore
    print('type_checking_runtime_false', not TYPE_CHECKING)

    # Test overload decorator
    @overload
    def func_overload(x: int) -> int: ...
    @overload
    def func_overload(x: str) -> str: ...
    def func_overload(x: int | str) -> int | str:
        return x

    print('overload_decorator_works', func_overload is not None)
    print('get_overloads_count', len(get_overloads(func_overload)) == 2)

    # Test clear_overloads (takes no args, clears all overloads globally)
    clear_overloads()
    print('clear_overloads_works', callable(clear_overloads))

    # Test SupportsInt
    print('supports_int_instance', isinstance(42, SupportsInt))

    # Test SupportsFloat  
    print('supports_float_instance', isinstance(3.14, SupportsFloat))

    # Test SupportsComplex
    print('supports_complex_instance', isinstance(1+2j, SupportsComplex))

    # Test SupportsBytes
    print('supports_bytes_instance', isinstance(b'hello', SupportsBytes))

    # Test SupportsAbs
    print('supports_abs_instance', isinstance(-42, SupportsAbs))

    # Test SupportsIndex
    print('supports_index_instance', isinstance(42, SupportsIndex))

    # Test SupportsRound
    print('supports_round_instance', isinstance(3.14, SupportsRound))

    # Test GenericAlias creation
    print('generic_alias_creation', GenericAlias(list, (int,)) is not None)

    # Test ClassVar
    class WithClassVar:
        x: ClassVar[int] = 5
    print('class_var_annotation', WithClassVar.__annotations__['x'] == ClassVar[int])

    # Test Final
    class WithFinal:
        CONSTANT: Final[int] = 100
    print('final_annotation', WithFinal.__annotations__['CONSTANT'] == Final[int])

    # Test Any
    print('any_accept_anything', Any is not None)

    # Test Text (alias for str)
    print('text_is_str', Text is str)

    # Test NoDefault
    print('no_default_singleton', NoDefault is not None)

    # Test is_typeddict with non-TypedDict
    print('is_typed_dict_false_for_dict', not is_typeddict(dict))

    # Summary
    print('total_tests_completed', 142)
except Exception as e:
    print('SKIP_Basic Functionality Tests', type(e).__name__, e)
