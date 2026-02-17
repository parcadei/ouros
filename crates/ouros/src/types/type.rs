use std::fmt;

use num_bigint::BigInt;
use strum::EnumString;

use crate::{
    args::ArgValues,
    builtins::{Builtins, BuiltinsFunctions},
    defer_drop,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData},
    intern::{Interns, StaticStrings},
    io::NoPrint,
    modules::{ModuleFunctions, struct_mod::StructFunctions},
    resource::ResourceTracker,
    types::{
        AttrCallResult, BoundMethod, Bytes, ClassMethod, Dict, FrozenSet, Instance, List, LongInt, OurosIter, Path,
        PyTrait, Range, Set, Slice, StaticMethod, StdlibObject, Str, Tuple, Uuid, str::StringRepr,
    },
    value::Value,
};

/// Represents the Python type of a value.
///
/// This enum is used both for type checking and as a callable constructor.
/// When parsed from a string (e.g., "list", "dict"), it can be used to create
/// new instances of that type.
///
/// Note: `Exception` variants is disabled for strum's `EnumString` (they can't be parsed from strings).
#[derive(Debug, Clone, Copy, EnumString, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
#[expect(clippy::enum_variant_names)]
pub enum Type {
    Ellipsis,
    Type,
    NoneType,
    Bool,
    Int,
    Float,
    /// A complex number value.
    ///
    /// Runtime values use `StdlibObject::Complex` so parser-lowered literals,
    /// `complex(...)` constructor calls, and core numeric operations can share
    /// one representation.
    Complex,
    Range,
    Slice,
    Str,
    Bytes,
    Bytearray,
    List,
    Tuple,
    /// Runtime type tag for namedtuple instances.
    ///
    /// Disabled for `EnumString` because `namedtuple` is not a Python builtin
    /// global name; it must resolve through normal runtime namespace lookup so
    /// imports/assignments like `from collections import namedtuple` can shadow.
    #[strum(disabled)]
    NamedTuple,
    Dict,
    #[strum(disabled)]
    Counter,
    #[strum(disabled)]
    OrderedDict,
    /// A dynamic view of a dict's keys.
    #[strum(serialize = "dict_keys")]
    DictKeys,
    /// A dynamic view of a dict's values.
    #[strum(serialize = "dict_values")]
    DictValues,
    /// A dynamic view of a dict's (key, value) pairs.
    #[strum(serialize = "dict_items")]
    DictItems,
    Set,
    FrozenSet,
    Dataclass,
    #[strum(disabled)]
    Exception(ExcType),
    Function,
    BuiltinFunction,
    Cell,
    #[strum(serialize = "iter")]
    Iterator,
    /// Coroutine type for async functions and external futures.
    Coroutine,
    /// Generator type for generator functions.
    Generator,
    /// Async generator type used by `aiter(...)` compatibility wrappers.
    #[strum(disabled)]
    AsyncGenerator,
    Module,
    /// Marker types like stdout/stderr - displays as "TextIOWrapper"
    #[strum(serialize = "TextIOWrapper")]
    TextIOWrapper,
    /// typing module special forms (Any, Optional, Union, etc.) - displays as "typing._SpecialForm"
    #[strum(serialize = "typing._SpecialForm")]
    SpecialForm,
    /// Runtime type tag used by `typing.AnyStr` to mirror CPython's `TypeVar`.
    #[strum(disabled)]
    TypingTypeVar,
    /// A filesystem path from `pathlib.Path` - displays as "PosixPath"
    #[strum(serialize = "PosixPath")]
    Path,
    /// Pure path base class from `pathlib.PurePath`.
    #[strum(serialize = "PurePath")]
    PurePath,
    /// Pure POSIX path class from `pathlib.PurePosixPath`.
    #[strum(serialize = "PurePosixPath")]
    PurePosixPath,
    /// Pure Windows path class from `pathlib.PureWindowsPath`.
    #[strum(serialize = "PureWindowsPath")]
    PureWindowsPath,
    /// A hash object from `hashlib` - displays as "_hashlib.HASH"
    #[strum(serialize = "_hashlib.HASH")]
    Hash,
    /// A property descriptor - displays as "property"
    #[strum(serialize = "property")]
    Property,
    /// A `@staticmethod` wrapper descriptor.
    #[strum(disabled)]
    StaticMethod,
    /// A `@classmethod` wrapper descriptor.
    #[strum(disabled)]
    ClassMethod,
    /// A slot/member descriptor created from `__slots__`.
    #[strum(disabled)]
    MemberDescriptor,
    /// A getset descriptor for `__dict__`/`__weakref__` slots.
    #[strum(disabled)]
    GetSetDescriptor,
    /// A read-only view of a class namespace (`type.__dict__`).
    #[strum(disabled)]
    MappingProxy,
    /// A `weakref.ref` object (displays as `weakref.ReferenceType`).
    #[strum(disabled)]
    WeakRef,
    /// A `types.GenericAlias` produced by `__class_getitem__`.
    #[strum(disabled)]
    GenericAlias,
    /// A bound method object created by attribute access on instances.
    #[strum(disabled)]
    Method,
    /// A user-defined class instance - displays as the class name
    #[strum(disabled)]
    Instance,
    /// The Python `object` base type - all classes implicitly inherit from object
    #[strum(serialize = "object")]
    Object,
    /// A `re.Match` object from a successful regex search/match.
    #[strum(disabled)]
    ReMatch,
    /// A `re.Pattern` object from `re.compile()`.
    #[strum(disabled)]
    RePattern,
    /// A `re.RegexFlag` enum value.
    #[strum(disabled)]
    RegexFlag,
    /// A scanner object returned by `re.Pattern.scanner()` / `re.Scanner`.
    #[strum(disabled)]
    SreScanner,
    /// A double-ended queue from `collections.deque`.
    #[strum(disabled)]
    Deque,
    /// A defaultdict from `collections.defaultdict`.
    #[strum(disabled)]
    DefaultDict,
    /// A chain map from `collections.ChainMap`.
    #[strum(disabled)]
    ChainMap,
    /// A timedelta object from `datetime.timedelta`.
    #[strum(serialize = "timedelta")]
    Timedelta,
    /// A date object from `datetime.date`.
    #[strum(serialize = "date")]
    Date,
    /// A datetime object from `datetime.datetime`.
    ///
    /// Disabled for `EnumString` because bare `datetime` must resolve to the `datetime` module,
    /// not the `datetime.datetime` class. Use `datetime.datetime(...)` to construct datetime objects.
    #[strum(disabled)]
    Datetime,
    /// A time object from `datetime.time`.
    ///
    /// Disabled for `EnumString` because bare `time` must resolve to the `time` module,
    /// not the `datetime.time` class. Use `datetime.time(...)` to construct time objects.
    #[strum(disabled)]
    Time,
    /// A timezone object from `datetime.timezone`.
    #[strum(serialize = "timezone")]
    Timezone,
    /// A tzinfo object from `datetime.tzinfo` (abstract base class).
    #[strum(serialize = "tzinfo")]
    Tzinfo,
    /// A decimal number from the `decimal` module.
    #[strum(disabled)]
    Decimal,
    /// A decimal arithmetic context object from `decimal.Context`.
    #[strum(disabled)]
    DecimalContext,
    /// A rational number fraction from `fractions.Fraction`.
    ///
    /// Disabled for strum's `EnumString` because `Fraction` is a module-level type
    /// (from `fractions`), not a Python builtin. The parser should not eagerly resolve
    /// the name `Fraction` to this type — it must go through runtime namespace lookup
    /// so that user-defined classes named `Fraction` can shadow it correctly.
    #[strum(disabled)]
    Fraction,
    /// An asyncio Future object from `asyncio.Future`.
    #[strum(serialize = "asyncio.Future")]
    Future,
    /// An asyncio Task object from `asyncio.Task`.
    #[strum(serialize = "asyncio.Task")]
    Task,
    /// An asyncio Queue object from `asyncio.Queue`.
    #[strum(serialize = "asyncio.Queue")]
    Queue,
    /// An asyncio Event object from `asyncio.Event`.
    #[strum(serialize = "asyncio.Event")]
    Event,
    /// An asyncio Lock object from `asyncio.Lock`.
    #[strum(serialize = "asyncio.Lock")]
    Lock,
    /// An asyncio Semaphore object from `asyncio.Semaphore`.
    #[strum(serialize = "asyncio.Semaphore")]
    Semaphore,
    /// A compiled struct format from `struct.Struct`.
    #[strum(disabled)]
    Struct,
    /// A UUID value from `uuid.UUID`.
    #[strum(disabled)]
    Uuid,
    /// A `uuid.SafeUUID` enum-member value.
    #[strum(disabled)]
    SafeUuid,
    /// The `dataclasses.MISSING` sentinel type.
    #[strum(serialize = "_MISSING_TYPE")]
    DataclassMissingType,
    /// The `dataclasses.KW_ONLY` sentinel type.
    #[strum(serialize = "_KW_ONLY_TYPE")]
    DataclassKwOnlyType,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ellipsis => f.write_str("ellipsis"),
            Self::Type => f.write_str("type"),
            Self::NoneType => f.write_str("NoneType"),
            Self::Bool => f.write_str("bool"),
            Self::Int => f.write_str("int"),
            Self::Float => f.write_str("float"),
            Self::Complex => f.write_str("complex"),
            Self::Range => f.write_str("range"),
            Self::Slice => f.write_str("slice"),
            Self::Str => f.write_str("str"),
            Self::Bytes => f.write_str("bytes"),
            Self::Bytearray => f.write_str("bytearray"),
            Self::List => f.write_str("list"),
            Self::Tuple => f.write_str("tuple"),
            Self::NamedTuple => f.write_str("namedtuple"),
            Self::Dict => f.write_str("dict"),
            Self::Counter => f.write_str("collections.Counter"),
            Self::OrderedDict => f.write_str("collections.OrderedDict"),
            Self::DictKeys => f.write_str("dict_keys"),
            Self::DictValues => f.write_str("dict_values"),
            Self::DictItems => f.write_str("dict_items"),
            Self::Set => f.write_str("set"),
            Self::FrozenSet => f.write_str("frozenset"),
            Self::Dataclass => f.write_str("dataclass"),
            Self::Exception(exc_type) => write!(f, "{exc_type}"),
            Self::Function => f.write_str("function"),
            Self::BuiltinFunction => f.write_str("builtin_function_or_method"),
            Self::Cell => f.write_str("cell"),
            Self::Iterator => f.write_str("iterator"),
            Self::Coroutine => f.write_str("coroutine"),
            Self::Generator => f.write_str("generator"),
            Self::AsyncGenerator => f.write_str("async_generator"),
            Self::Module => f.write_str("module"),
            Self::TextIOWrapper => f.write_str("_io.TextIOWrapper"),
            Self::SpecialForm => f.write_str("typing._SpecialForm"),
            Self::TypingTypeVar => f.write_str("TypeVar"),
            Self::Path => f.write_str("pathlib.PosixPath"),
            Self::PurePath => f.write_str("pathlib.PurePath"),
            Self::PurePosixPath => f.write_str("pathlib.PurePosixPath"),
            Self::PureWindowsPath => f.write_str("pathlib.PureWindowsPath"),
            Self::Hash => f.write_str("_hashlib.HASH"),
            Self::Property => f.write_str("property"),
            Self::StaticMethod => f.write_str("staticmethod"),
            Self::ClassMethod => f.write_str("classmethod"),
            Self::MemberDescriptor => f.write_str("member_descriptor"),
            Self::GetSetDescriptor => f.write_str("getset_descriptor"),
            Self::MappingProxy => f.write_str("mappingproxy"),
            Self::WeakRef => f.write_str("weakref.ReferenceType"),
            Self::GenericAlias => f.write_str("types.GenericAlias"),
            Self::Method => f.write_str("method"),
            Self::Instance => f.write_str("instance"),
            Self::Object => f.write_str("object"),
            Self::ReMatch => f.write_str("re.Match"),
            Self::RePattern => f.write_str("re.Pattern"),
            Self::RegexFlag => f.write_str("RegexFlag"),
            Self::SreScanner => f.write_str("SRE_Scanner"),
            Self::Struct => f.write_str("struct.Struct"),
            Self::Deque => f.write_str("collections.deque"),
            Self::DefaultDict => f.write_str("collections.defaultdict"),
            Self::ChainMap => f.write_str("collections.ChainMap"),
            Self::Timedelta => f.write_str("datetime.timedelta"),
            Self::Date => f.write_str("datetime.date"),
            Self::Datetime => f.write_str("datetime.datetime"),
            Self::Time => f.write_str("datetime.time"),
            Self::Timezone => f.write_str("datetime.timezone"),
            Self::Tzinfo => f.write_str("datetime.tzinfo"),
            Self::Decimal => f.write_str("decimal.Decimal"),
            Self::DecimalContext => f.write_str("decimal.Context"),
            Self::Fraction => f.write_str("fractions.Fraction"),
            Self::Future => f.write_str("asyncio.Future"),
            Self::Task => f.write_str("asyncio.Task"),
            Self::Queue => f.write_str("asyncio.Queue"),
            Self::Event => f.write_str("asyncio.Event"),
            Self::Lock => f.write_str("asyncio.Lock"),
            Self::Semaphore => f.write_str("asyncio.Semaphore"),
            Self::Uuid => f.write_str("UUID"),
            Self::SafeUuid => f.write_str("SafeUUID"),
            Self::DataclassMissingType => f.write_str("_MISSING_TYPE"),
            Self::DataclassKwOnlyType => f.write_str("_KW_ONLY_TYPE"),
        }
    }
}

impl Type {
    /// Checks if a value of type `self` is an instance of `other`.
    ///
    /// This handles Python's subtype relationships:
    /// - `bool` is a subtype of `int` (so `isinstance(True, int)` returns True)
    #[must_use]
    pub fn is_instance_of(self, other: Self) -> bool {
        if self == other {
            true
        } else if other == Self::Object {
            // Everything is an instance of object in Python
            true
        } else if self == Self::Bool && other == Self::Int {
            // bool is a subtype of int in Python
            true
        } else if self == Self::DefaultDict && other == Self::Dict {
            // defaultdict is a subtype of dict in Python
            true
        } else if (self == Self::Counter || self == Self::OrderedDict) && other == Self::Dict {
            // Counter and OrderedDict are dict subclasses in Python
            true
        } else if self == Self::Path && (other == Self::PurePosixPath || other == Self::PurePath) {
            // pathlib.Path is a concrete POSIX path and is also a PurePosixPath/PurePath
            true
        } else if (self == Self::PurePosixPath || self == Self::PureWindowsPath) && other == Self::PurePath {
            // Both concrete pure flavors are instances of PurePath
            true
        } else {
            false
        }
    }

    /// Converts a callable type to a u8 for the `CallBuiltinType` opcode.
    ///
    /// Returns `Some(u8)` for types that can be called as constructors,
    /// `None` for non-callable types.
    #[must_use]
    pub fn callable_to_u8(self) -> Option<u8> {
        match self {
            Self::Bool => Some(0),
            Self::Int => Some(1),
            Self::Float => Some(2),
            Self::Complex => Some(16),
            Self::Str => Some(3),
            Self::Bytes => Some(4),
            Self::Bytearray => Some(15),
            Self::List => Some(5),
            Self::Tuple => Some(6),
            Self::Dict => Some(7),
            Self::Set => Some(8),
            Self::FrozenSet => Some(9),
            Self::Range => Some(10),
            Self::Slice => Some(11),
            Self::Iterator => Some(12),
            Self::Path => Some(13),
            Self::Queue => Some(14),
            _ => None,
        }
    }

    /// Converts a u8 back to a callable `Type` for the `CallBuiltinType` opcode.
    ///
    /// Returns `Some(Type)` for valid callable type IDs, `None` otherwise.
    #[must_use]
    pub fn callable_from_u8(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Bool),
            1 => Some(Self::Int),
            2 => Some(Self::Float),
            16 => Some(Self::Complex),
            3 => Some(Self::Str),
            4 => Some(Self::Bytes),
            5 => Some(Self::List),
            15 => Some(Self::Bytearray),
            6 => Some(Self::Tuple),
            7 => Some(Self::Dict),
            8 => Some(Self::Set),
            9 => Some(Self::FrozenSet),
            10 => Some(Self::Range),
            11 => Some(Self::Slice),
            12 => Some(Self::Iterator),
            13 => Some(Self::Path),
            14 => Some(Self::Queue),
            _ => None,
        }
    }

    /// Calls this type as a constructor (e.g., `list(x)`, `int(x)`).
    ///
    /// Dispatches to the appropriate type's init method for container types,
    /// or handles primitive type conversions inline.
    pub(crate) fn call(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        match self {
            // Container types - delegate to init methods
            Self::List => List::init(heap, args, interns),
            Self::Tuple => Tuple::init(heap, args, interns),
            Self::Dict => Dict::init(heap, args, interns),
            Self::Set => Set::init(heap, args, interns),
            Self::FrozenSet => FrozenSet::init(heap, args, interns),
            Self::Str => Str::init(heap, args, interns),
            Self::Bytes => Bytes::init(heap, args, interns),
            Self::Bytearray => Bytes::init_bytearray(heap, args, interns),
            Self::Range => Range::init(heap, args),
            Self::Slice => Slice::init(heap, args),
            Self::Iterator => OurosIter::init(heap, args, interns),
            Self::Path => Path::init(heap, args, interns),
            Self::PurePath => Path::init_pure_path(heap, args, interns),
            Self::PurePosixPath => Path::init_pure_posix_path(heap, args, interns),
            Self::PureWindowsPath => Path::init_pure_windows_path(heap, args, interns),
            Self::Queue => crate::modules::asyncio::queue_init(heap, args),
            Self::Event => crate::modules::asyncio::event_init(heap, args),
            Self::Lock => crate::modules::asyncio::lock_init(heap, args),
            Self::Semaphore => crate::modules::asyncio::semaphore_init(heap, args),
            Self::Object => {
                args.check_zero_args("object", heap)?;

                let class_heap_id = heap.builtin_class_id(Self::Object)?;
                let (slot_len, has_dict, _has_weakref) = match heap.get(class_heap_id) {
                    HeapData::ClassObject(cls) => (
                        cls.slot_layout().len(),
                        cls.instance_has_dict(),
                        cls.instance_has_weakref(),
                    ),
                    _ => unreachable!("builtin object class must be a class object"),
                };

                heap.inc_ref(class_heap_id);
                let attrs_id = if has_dict {
                    Some(heap.allocate(HeapData::Dict(Dict::new()))?)
                } else {
                    None
                };
                let mut slot_values = Vec::with_capacity(slot_len);
                slot_values.resize_with(slot_len, || Value::Undefined);
                let weakref_ids = Vec::new();
                let instance = Instance::new(class_heap_id, attrs_id, slot_values, weakref_ids);
                let instance_heap_id = heap.allocate(HeapData::Instance(instance))?;
                Ok(Value::Ref(instance_heap_id))
            }
            Self::Uuid => Uuid::init(heap, args, interns),
            Self::Dataclass => crate::modules::dataclasses::call_dataclass_decorator(heap, interns, args),
            Self::StaticMethod => {
                let func = args.get_one_arg("staticmethod", heap)?;
                let sm = StaticMethod::new(func);
                let id = heap.allocate(HeapData::StaticMethod(sm))?;
                Ok(Value::Ref(id))
            }
            Self::ClassMethod => {
                let func = args.get_one_arg("classmethod", heap)?;
                let cm = ClassMethod::new(func);
                let id = heap.allocate(HeapData::ClassMethod(cm))?;
                Ok(Value::Ref(id))
            }
            Self::Method => {
                let (func, instance) = args.get_two_args("method", heap)?;
                let mut print = NoPrint;
                let callable_check = BuiltinsFunctions::Callable.call(
                    heap,
                    ArgValues::One(func.clone_with_heap(heap)),
                    interns,
                    &mut print,
                )?;
                let is_callable = matches!(callable_check, Value::Bool(true));
                callable_check.drop_with_heap(heap);
                if !is_callable {
                    func.drop_with_heap(heap);
                    instance.drop_with_heap(heap);
                    return Err(ExcType::type_error("first argument must be callable"));
                }
                if matches!(instance, Value::None) {
                    func.drop_with_heap(heap);
                    return Err(ExcType::type_error("instance must not be None"));
                }
                let method = BoundMethod::new(func, instance);
                let id = heap.allocate(HeapData::BoundMethod(method))?;
                Ok(Value::Ref(id))
            }
            Self::Fraction => crate::modules::fractions_mod::call_type(heap, interns, args).map(|res| match res {
                crate::types::AttrCallResult::Value(v) => v,
                _ => unreachable!(),
            }),
            Self::Decimal => crate::modules::decimal_mod::call_type(heap, interns, args).map(|res| match res {
                crate::types::AttrCallResult::Value(v) => v,
                _ => unreachable!(),
            }),
            Self::DecimalContext => crate::modules::decimal_mod::call_context_type(heap, interns, args),
            Self::Struct => {
                let format_value = args.get_one_arg("Struct", heap)?;
                let format_string = format_value.py_str(heap, interns).into_owned();
                format_value.drop_with_heap(heap);

                let format_id = heap.allocate(HeapData::Str(Str::from(format_string.as_str())))?;
                let calc_args = ArgValues::One(Value::Ref(format_id));
                let size_result = ModuleFunctions::Struct(StructFunctions::Calcsize).call(heap, interns, calc_args)?;
                let AttrCallResult::Value(size_value) = size_result else {
                    return Err(SimpleException::new_msg(
                        ExcType::RuntimeError,
                        "unexpected non-value result from struct.calcsize".to_string(),
                    )
                    .into());
                };
                let size = match size_value {
                    Value::Int(value) if value >= 0 => usize::try_from(value).expect("non-negative i64 fits usize"),
                    value => {
                        value.drop_with_heap(heap);
                        return Err(SimpleException::new_msg(
                            ExcType::RuntimeError,
                            "unexpected result type from struct.calcsize".to_string(),
                        )
                        .into());
                    }
                };
                let obj = StdlibObject::new_struct(format_string, size);
                let id = heap.allocate(HeapData::StdlibObject(obj))?;
                Ok(Value::Ref(id))
            }
            Self::Timedelta => crate::types::datetime_types::Timedelta::init(heap, args, interns),
            Self::Date => crate::types::datetime_types::Date::init(heap, args, interns),
            Self::Datetime => crate::types::datetime_types::Datetime::init(heap, args, interns),
            Self::Time => crate::types::datetime_types::Time::init(heap, args, interns),
            Self::Timezone => crate::types::datetime_types::Timezone::init(heap, args, interns),
            Self::Tzinfo => Err(SimpleException::new_msg(
                ExcType::NotImplementedError,
                "datetime.tzinfo() constructor not yet implemented",
            )
            .into()),

            // Primitive types - inline implementation
            Self::Int => call_int_constructor(heap, args, interns),
            Self::Float => {
                let Some(v) = args.get_zero_one_arg("float", heap)? else {
                    return Ok(Value::Float(0.0));
                };
                defer_drop!(v, heap);
                match v {
                    Value::Float(f) => Ok(Value::Float(*f)),
                    Value::Int(i) => Ok(Value::Float(*i as f64)),
                    Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
                    Value::InternString(string_id) => {
                        Ok(Value::Float(parse_f64_from_str(interns.get_str(*string_id))?))
                    }
                    Value::Ref(heap_id) => match heap.get(*heap_id) {
                        HeapData::Str(s) => Ok(Value::Float(parse_f64_from_str(s.as_str())?)),
                        HeapData::Fraction(frac) => Ok(Value::Float(frac.to_f64())),
                        HeapData::Decimal(decimal) => float_from_decimal(decimal),
                        HeapData::LongInt(li) => {
                            let f = li.to_f64().unwrap_or(f64::INFINITY);
                            Ok(Value::Float(f))
                        }
                        _ => Err(ExcType::type_error_float_conversion(v.py_type(heap))),
                    },
                    _ => Err(ExcType::type_error_float_conversion(v.py_type(heap))),
                }
            }
            Self::Complex => call_complex_constructor(heap, args, interns),
            Self::Bool => {
                let Some(v) = args.get_zero_one_arg("bool", heap)? else {
                    return Ok(Value::Bool(false));
                };
                defer_drop!(v, heap);
                Ok(Value::Bool(v.py_bool(heap, interns)))
            }
            Self::RegexFlag => {
                // RegexFlag(value) constructor - creates a new RegexFlagValue
                let value = args.get_one_arg("RegexFlag", heap)?;
                defer_drop!(value, heap);
                let bits = match *value {
                    Value::Int(i) => i,
                    Value::Bool(b) => i64::from(b),
                    _ => {
                        return Err(ExcType::type_error("RegexFlag() requires an integer argument"));
                    }
                };
                let obj = StdlibObject::new_regex_flag(bits);
                let id = heap.allocate(HeapData::StdlibObject(obj))?;
                Ok(Value::Ref(id))
            }

            // Non-callable types - raise TypeError
            _ => Err(ExcType::type_error_not_callable(self)),
        }
    }

    /// Returns a method descriptor for accessing methods on builtin types.
    ///
    /// This enables usage like `str.lower`, `list.append`, etc.
    pub(crate) fn py_getattr(&self, attr_id: StaticStrings) -> Option<AttrCallResult> {
        match self {
            Self::Str => Self::str_type_method(attr_id),
            Self::Object => Self::object_type_method(attr_id),
            Self::Date => Self::datetime_type_method(*self, attr_id),
            Self::Datetime => Self::datetime_type_method(*self, attr_id),
            Self::Time => Self::datetime_type_method(*self, attr_id),
            Self::Tzinfo => Self::datetime_type_method(*self, attr_id),
            _ => None,
        }
    }

    /// Returns a method descriptor for str type methods.
    fn str_type_method(attr_id: StaticStrings) -> Option<AttrCallResult> {
        // Map of str methods that can be accessed as unbound methods
        match attr_id {
            StaticStrings::Lower
            | StaticStrings::Upper
            | StaticStrings::Capitalize
            | StaticStrings::Title
            | StaticStrings::Swapcase
            | StaticStrings::Casefold
            | StaticStrings::Isalpha
            | StaticStrings::Isdigit
            | StaticStrings::Isalnum
            | StaticStrings::Isnumeric
            | StaticStrings::Isspace
            | StaticStrings::Islower
            | StaticStrings::Isupper
            | StaticStrings::Isascii
            | StaticStrings::Isdecimal
            | StaticStrings::Find
            | StaticStrings::Rfind
            | StaticStrings::Index
            | StaticStrings::Rindex
            | StaticStrings::Count
            | StaticStrings::Startswith
            | StaticStrings::Endswith
            | StaticStrings::Strip
            | StaticStrings::Lstrip
            | StaticStrings::Rstrip
            | StaticStrings::Removeprefix
            | StaticStrings::Removesuffix
            | StaticStrings::Split
            | StaticStrings::Rsplit
            | StaticStrings::Splitlines
            | StaticStrings::Partition
            | StaticStrings::Rpartition
            | StaticStrings::Replace
            | StaticStrings::Center
            | StaticStrings::Ljust
            | StaticStrings::Rjust
            | StaticStrings::Zfill
            | StaticStrings::Join
            | StaticStrings::Encode
            | StaticStrings::Isidentifier
            | StaticStrings::Istitle => {
                // Return a BuiltinTypeMethod that will be called with the instance as first arg
                Some(AttrCallResult::Value(Value::Builtin(Builtins::TypeMethod {
                    ty: Self::Str,
                    method: attr_id,
                })))
            }
            _ => None,
        }
    }

    /// Returns a method descriptor for object type methods.
    fn object_type_method(attr_id: StaticStrings) -> Option<AttrCallResult> {
        match attr_id {
            StaticStrings::DunderSetattr | StaticStrings::DunderGetattribute | StaticStrings::DunderDelattr => {
                Some(AttrCallResult::Value(Value::Builtin(Builtins::TypeMethod {
                    ty: Self::Object,
                    method: attr_id,
                })))
            }
            _ => None,
        }
    }

    /// Returns a method descriptor for datetime-related type methods.
    fn datetime_type_method(ty: Self, attr_id: StaticStrings) -> Option<AttrCallResult> {
        let method_supported = match ty {
            Self::Date => matches!(
                attr_id,
                StaticStrings::Today
                    | StaticStrings::Fromtimestamp
                    | StaticStrings::Fromordinal
                    | StaticStrings::Fromisoformat
            ),
            Self::Datetime => matches!(
                attr_id,
                StaticStrings::Now
                    | StaticStrings::Utcnow
                    | StaticStrings::Fromtimestamp
                    | StaticStrings::Utcfromtimestamp
                    | StaticStrings::Fromordinal
                    | StaticStrings::Fromisoformat
                    | StaticStrings::Combine
            ),
            Self::Time => matches!(attr_id, StaticStrings::Fromisoformat),
            Self::Tzinfo => matches!(
                attr_id,
                StaticStrings::Utcoffset | StaticStrings::Tzname | StaticStrings::Dst
            ),
            _ => false,
        };
        if method_supported {
            Some(AttrCallResult::Value(Value::Builtin(Builtins::TypeMethod {
                ty,
                method: attr_id,
            })))
        } else {
            None
        }
    }
}

/// Implements the `complex()` constructor.
///
/// Complex values are represented as `StdlibObject::Complex` so runtime
/// operations (such as `isinstance(x, complex)` and `json` custom encoding)
/// can observe real/imaginary components directly.
fn call_complex_constructor(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    interns: &Interns,
) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        let arg_count = positional.len() + kwargs.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("complex", 2, arg_count));
    }
    kwargs.drop_with_heap(heap);
    let positional: Vec<Value> = positional.collect();
    if positional.len() > 2 {
        let arg_count = positional.len();
        for value in positional {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("complex", 2, arg_count));
    }

    let (real, imag) = match positional.len() {
        0 => (0.0, 0.0),
        1 => {
            let value = positional.into_iter().next().expect("len checked");
            let parts = match complex_parts_from_single_value(&value, heap, interns) {
                Ok(parts) => parts,
                Err(err) => {
                    value.drop_with_heap(heap);
                    return Err(err);
                }
            };
            value.drop_with_heap(heap);
            parts
        }
        2 => {
            let mut iter = positional.into_iter();
            let real_value = iter.next().expect("len checked");
            let imag_value = iter.next().expect("len checked");
            let real = match complex_component_from_value(&real_value, heap, interns) {
                Ok(real) => real,
                Err(err) => {
                    real_value.drop_with_heap(heap);
                    imag_value.drop_with_heap(heap);
                    return Err(err);
                }
            };
            let imag = match complex_component_from_value(&imag_value, heap, interns) {
                Ok(imag) => imag,
                Err(err) => {
                    real_value.drop_with_heap(heap);
                    imag_value.drop_with_heap(heap);
                    return Err(err);
                }
            };
            real_value.drop_with_heap(heap);
            imag_value.drop_with_heap(heap);
            (real, imag)
        }
        _ => unreachable!("validated above"),
    };

    let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(real, imag)))?;
    Ok(Value::Ref(id))
}

/// Converts one `complex()` argument into `(real, imag)` components.
///
/// This path accepts:
/// - existing complex objects
/// - string inputs (including compact forms such as `'3+4j'` and `'2j'`)
/// - numeric-like values treated as real parts
fn complex_parts_from_single_value(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(f64, f64)> {
    let text = match value {
        Value::InternString(id) => Some(interns.get_str(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str()),
            HeapData::StdlibObject(StdlibObject::Complex { real, imag }) => return Ok((*real, *imag)),
            _ => None,
        },
        _ => None,
    };
    if let Some(text) = text {
        return parse_complex_literal(text);
    }

    Ok((complex_component_from_value(value, heap, interns)?, 0.0))
}

/// Parses a string literal accepted by `complex(<single-string-arg>)`.
fn parse_complex_literal(value: &str) -> RunResult<(f64, f64)> {
    let mut trimmed = value.trim();
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|inner| inner.strip_suffix(')')) {
        trimmed = inner.trim();
    }

    if !trimmed.contains(['j', 'J']) {
        return Ok((parse_f64_from_str(trimmed)?, 0.0));
    }

    let Some(without_j) = trimmed.strip_suffix(['j', 'J']) else {
        return Err(value_error_complex_malformed_string());
    };
    if without_j.is_empty() || without_j == "+" {
        return Ok((0.0, 1.0));
    }
    if without_j == "-" {
        return Ok((0.0, -1.0));
    }

    let mut split_at = None;
    for (idx, ch) in without_j.char_indices().skip(1) {
        if (ch == '+' || ch == '-') && !without_j[..idx].ends_with(['e', 'E']) {
            split_at = Some(idx);
        }
    }
    if let Some(split_at) = split_at {
        let real = parse_f64_from_str(&without_j[..split_at])?;
        let imag = parse_f64_from_str(&without_j[split_at..])?;
        return Ok((real, imag));
    }

    let imag = parse_f64_from_str(without_j)?;
    Ok((0.0, imag))
}

/// Converts numeric-like values into one complex component value.
fn complex_component_from_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<f64> {
    match value {
        #[expect(
            clippy::cast_precision_loss,
            reason = "complex() uses f64 storage for numeric components"
        )]
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::InternString(id) => parse_f64_from_str(interns.get_str(*id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => parse_f64_from_str(s.as_str()),
            HeapData::LongInt(long_int) => parse_f64_from_str(&long_int.to_string()),
            _ => Err(type_error_complex_argument_must_be_number_or_string()),
        },
        _ => Err(type_error_complex_argument_must_be_number_or_string()),
    }
}

/// Creates the `TypeError` raised when `complex()` receives unsupported args.
fn type_error_complex_argument_must_be_number_or_string() -> RunError {
    ExcType::type_error("complex() argument must be a number or string")
}

/// Creates the `ValueError` raised for malformed string input in `complex()`.
fn value_error_complex_malformed_string() -> RunError {
    SimpleException::new_msg(ExcType::ValueError, "complex() arg is a malformed string").into()
}

/// Implements `int()` constructor argument handling, including optional `base`.
///
/// Supports:
/// - `int()` -> `0`
/// - `int(x)` for numbers and strings
/// - `int(x, base)` / `int(x, base=...)` for string inputs
fn call_int_constructor(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();

    let mut kw_base: Option<Value> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for pos in positional {
                pos.drop_with_heap(heap);
            }
            kw_base.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name == "base" {
            if kw_base.is_some() {
                value.drop_with_heap(heap);
                for pos in positional {
                    pos.drop_with_heap(heap);
                }
                kw_base.drop_with_heap(heap);
                return Err(ExcType::type_error_multiple_values("int", "base"));
            }
            kw_base = Some(value);
        } else {
            value.drop_with_heap(heap);
            for pos in positional {
                pos.drop_with_heap(heap);
            }
            kw_base.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("int", &key_name));
        }
    }

    let positional_len = positional.len();
    if positional_len > 2 {
        for pos in positional {
            pos.drop_with_heap(heap);
        }
        kw_base.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("int", 2, positional_len));
    }

    let mut positional = positional.into_iter();
    let value = positional.next();
    let mut base = positional.next();

    if let Some(kw_base) = kw_base {
        if base.is_some() {
            kw_base.drop_with_heap(heap);
            base.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_multiple_values("int", "base"));
        }
        base = Some(kw_base);
    }

    let Some(value) = value else {
        let has_base = base.is_some();
        base.drop_with_heap(heap);
        return if has_base {
            Err(type_error_int_missing_string_argument())
        } else {
            Ok(Value::Int(0))
        };
    };

    let parsed_base = if let Some(base_value) = base {
        let parsed_base = parse_int_base_arg(&base_value, heap);
        base_value.drop_with_heap(heap);
        match parsed_base {
            Ok(base) => Some(base),
            Err(err) => {
                value.drop_with_heap(heap);
                return Err(err);
            }
        }
    } else {
        None
    };

    if let Some(base) = parsed_base {
        int_from_value_with_base(value, base, heap, interns)
    } else {
        int_from_value(value, heap, interns)
    }
}

/// Parses the `base` argument from `int(x, base)` into an `i64`.
///
/// Accepts `int`, `bool`, and `LongInt` values (when they fit in `i64`).
fn parse_int_base_arg(base: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match base {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Ref(heap_id) => {
            if let HeapData::LongInt(li) = heap.get(*heap_id) {
                li.to_i64().ok_or_else(ExcType::overflow_shift_count)
            } else {
                Err(ExcType::type_error_not_integer(base.py_type(heap)))
            }
        }
        _ => Err(ExcType::type_error_not_integer(base.py_type(heap))),
    }
}

/// Converts a value for `int(x)` with default base semantics.
fn int_from_value(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    defer_drop!(value, heap);
    match value {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Float(f) => Ok(Value::Int(f64_to_i64_truncate(*f))),
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::InternString(string_id) => parse_int_from_str(interns.get_str(*string_id), heap),
        Value::Ref(heap_id) => {
            // Clone data to release the borrow on heap before mutation.
            match heap.get(*heap_id) {
                HeapData::Str(s) => {
                    let s = s.to_string();
                    parse_int_from_str(&s, heap)
                }
                HeapData::LongInt(li) => li.clone().into_value(heap).map_err(Into::into),
                HeapData::Fraction(frac) => {
                    // int(Fraction) truncates toward zero, same as __trunc__
                    let truncated = frac.numerator() / frac.denominator();
                    LongInt::new(truncated).into_value(heap).map_err(Into::into)
                }
                HeapData::Decimal(decimal) => {
                    let decimal = decimal.clone();
                    int_from_decimal(&decimal, heap)
                }
                _ => Err(ExcType::type_error_int_conversion(value.py_type(heap))),
            }
        }
        _ => Err(ExcType::type_error_int_conversion(value.py_type(heap))),
    }
}

/// Converts `decimal.Decimal` to an integer using CPython `int(Decimal(...))` semantics.
///
/// - Finite values are truncated toward zero.
/// - `NaN` raises `ValueError("cannot convert NaN to integer")`.
/// - `Infinity` raises `OverflowError("cannot convert Infinity to integer")`.
fn int_from_decimal(decimal: &crate::types::Decimal, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if decimal.is_nan() {
        return Err(ExcType::value_error_cannot_convert_nan_to_integer());
    }
    if decimal.is_infinite() {
        return Err(ExcType::overflow_error_cannot_convert_infinity_to_integer());
    }

    let truncated = decimal
        .trunc_to_bigint()
        .ok_or_else(|| ExcType::type_error_int_conversion(Type::Decimal))?;
    LongInt::new(truncated).into_value(heap).map_err(Into::into)
}

/// Converts `decimal.Decimal` to `float` using CPython-compatible semantics.
///
/// - Quiet NaN returns `nan`.
/// - Signaling NaN raises `ValueError("cannot convert signaling NaN to float")`.
/// - Infinities map to `+/-inf`.
/// - Finite values are parsed from their decimal string form.
fn float_from_decimal(decimal: &crate::types::Decimal) -> RunResult<Value> {
    if decimal.is_snan() {
        return Err(ExcType::value_error_cannot_convert_signaling_nan_to_float());
    }
    if decimal.is_nan() {
        return Ok(Value::Float(f64::NAN));
    }
    if decimal.is_infinite() {
        return Ok(Value::Float(if decimal.is_signed() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }));
    }

    let decimal_string = decimal.to_string();
    let parsed = decimal_string.parse::<f64>().unwrap_or(if decimal.is_signed() {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    });
    Ok(Value::Float(parsed))
}

/// Converts a value for `int(x, base)` where `base` is explicitly provided.
///
/// Per CPython semantics, explicit base conversion only accepts string inputs.
fn int_from_value_with_base(
    value: Value,
    base: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    defer_drop!(value, heap);
    match value {
        Value::InternString(string_id) => parse_int_from_str_with_base(interns.get_str(*string_id), base, heap),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Str(s) => {
                let s = s.to_string();
                parse_int_from_str_with_base(&s, base, heap)
            }
            _ => Err(type_error_int_non_string_with_explicit_base()),
        },
        _ => Err(type_error_int_non_string_with_explicit_base()),
    }
}

/// Truncates f64 to i64 with clamping for out-of-range values.
///
/// Python's `int(float)` truncates toward zero. For values outside i64 range,
/// we clamp to i64::MAX/MIN (Python would use arbitrary precision ints, which
/// we don't support).
fn f64_to_i64_truncate(value: f64) -> i64 {
    // trunc() rounds toward zero, matching Python's int(float) behavior
    let truncated = value.trunc();
    if truncated >= i64::MAX as f64 {
        i64::MAX
    } else if truncated <= i64::MIN as f64 {
        i64::MIN
    } else {
        // SAFETY for clippy: truncated is guaranteed to be in (i64::MIN, i64::MAX)
        // after the bounds checks above, so truncation cannot overflow
        #[expect(clippy::cast_possible_truncation, reason = "bounds checked above")]
        let result = truncated as i64;
        result
    }
}

/// Parses a Python `float()` string argument into an `f64`.
///
/// This supports:
/// - Leading/trailing whitespace (e.g. `"  1.5  "`)
/// - The special values `inf`, `-inf`, `infinity`, and `nan` (case-insensitive)
///
/// Underscore digit separators are not currently supported.
fn parse_f64_from_str(value: &str) -> RunResult<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(value_error_could_not_convert_string_to_float(value));
    }

    let lower = trimmed.to_ascii_lowercase();
    let parsed = match lower.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => f64::INFINITY,
        "-inf" | "-infinity" => f64::NEG_INFINITY,
        "nan" | "+nan" => f64::NAN,
        "-nan" => -f64::NAN,
        _ => trimmed
            .parse::<f64>()
            .map_err(|_| value_error_could_not_convert_string_to_float(value))?,
    };

    Ok(parsed)
}

/// Creates the `ValueError` raised by `float()` when a string cannot be parsed.
///
/// Matches CPython's message format: `could not convert string to float: '...'`.
fn value_error_could_not_convert_string_to_float(value: &str) -> RunError {
    SimpleException::new_msg(
        ExcType::ValueError,
        format!("could not convert string to float: {}", StringRepr(value)),
    )
    .into()
}

/// Parses a Python `int()` string argument into an `Int` or `LongInt`.
///
/// Handles whitespace stripping and removing `_` separators. Returns `Value::Int` if the value
/// fits in i64, otherwise allocates a `LongInt` on the heap. Returns `ValueError` on failure.
fn parse_int_from_str(value: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    parse_int_from_str_with_base(value, 10, heap)
}

/// Parses a Python `int()` string argument with an explicit base.
///
/// Supports base values in `[2, 36]` plus `0` (autodetect prefixes). Underscores are
/// accepted as digit separators using the same normalization strategy as the existing
/// base-10 parser.
fn parse_int_from_str_with_base(value: &str, base: i64, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if base != 0 && !(2..=36).contains(&base) {
        return Err(value_error_int_base_range());
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(value_error_invalid_literal_for_int_with_base(value, base));
    }

    let (is_negative, mut digits) = if let Some(rest) = trimmed.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (false, rest)
    } else {
        (false, trimmed)
    };

    let mut radix = if base == 0 {
        10
    } else {
        u32::try_from(base).expect("base validated to be non-negative and <= 36")
    };

    if base == 0 {
        if let Some(rest) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
            radix = 16;
            digits = rest;
        } else if let Some(rest) = digits.strip_prefix("0o").or_else(|| digits.strip_prefix("0O")) {
            radix = 8;
            digits = rest;
        } else if let Some(rest) = digits.strip_prefix("0b").or_else(|| digits.strip_prefix("0B")) {
            radix = 2;
            digits = rest;
        }
    } else if base == 16 {
        if let Some(rest) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
            digits = rest;
        }
    } else if base == 8 {
        if let Some(rest) = digits.strip_prefix("0o").or_else(|| digits.strip_prefix("0O")) {
            digits = rest;
        }
    } else if base == 2
        && let Some(rest) = digits.strip_prefix("0b").or_else(|| digits.strip_prefix("0B"))
    {
        digits = rest;
    }

    // Match CPython behavior for base=0 where decimal-looking literals with a leading
    // zero and non-zero digits are rejected (e.g. "010", "+010", "-010").
    let normalized_for_leading_zero_check = digits.replace('_', "");
    if base == 0
        && radix == 10
        && normalized_for_leading_zero_check.len() > 1
        && normalized_for_leading_zero_check.starts_with('0')
        && normalized_for_leading_zero_check.chars().any(|c| c != '0')
    {
        return Err(value_error_invalid_literal_for_int_with_base(value, base));
    }

    let normalized = digits.replace('_', "");
    if normalized.is_empty() {
        return Err(value_error_invalid_literal_for_int_with_base(value, base));
    }

    let Some(mut bigint) = BigInt::parse_bytes(normalized.as_bytes(), radix) else {
        return Err(value_error_invalid_literal_for_int_with_base(value, base));
    };
    if is_negative {
        bigint = -bigint;
    }

    Ok(LongInt::new(bigint).into_value(heap)?)
}

/// Creates the `ValueError` raised by `int()` for invalid string input at a specific base.
fn value_error_invalid_literal_for_int_with_base(value: &str, base: i64) -> RunError {
    SimpleException::new_msg(
        ExcType::ValueError,
        format!("invalid literal for int() with base {base}: {}", StringRepr(value)),
    )
    .into()
}

/// Creates the `ValueError` raised by `int()` when base is outside valid range.
fn value_error_int_base_range() -> RunError {
    SimpleException::new_msg(ExcType::ValueError, "int() base must be >= 2 and <= 36, or 0").into()
}

/// Creates the `TypeError` raised by `int()` when explicit base is used with non-strings.
fn type_error_int_non_string_with_explicit_base() -> RunError {
    SimpleException::new_msg(ExcType::TypeError, "int() can't convert non-string with explicit base").into()
}

/// Creates the `TypeError` raised by `int()` when `base` is provided without `x`.
fn type_error_int_missing_string_argument() -> RunError {
    SimpleException::new_msg(ExcType::TypeError, "int() missing string argument").into()
}
