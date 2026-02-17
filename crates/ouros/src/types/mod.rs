/// Type definitions for Python runtime values.
///
/// This module contains structured types that wrap heap-allocated data
/// and provide Python-like semantics for operations like append, insert, etc.
///
/// The `AbstractValue` trait provides a common interface for all heap-allocated
/// types, enabling efficient dispatch via `enum_dispatch`.
pub mod bytes;
pub mod chain_map;
pub mod class;
pub mod counter;
pub mod dataclass;
pub mod datetime_types;
pub mod decimal;
pub mod defaultdict;
pub mod deque;
pub mod dict;
pub mod fraction;
pub mod functools;
pub mod generator;
pub mod generic_alias;
pub mod iter;
pub mod list;
pub mod long_int;
pub mod mapping_proxy;
pub mod module;
pub mod namedtuple;
pub mod operator_callables;
pub mod ordered_dict;
pub mod partial;
pub mod path;
pub mod property;
pub mod py_trait;
pub mod range;
pub mod re_types;
pub mod set;
pub mod slice;
pub mod stdlib_objects;
pub mod str;
pub mod textwrap;
pub mod tuple;
pub mod r#type;
pub mod uuid;
pub mod weakref;

pub(crate) use bytes::Bytes;
pub(crate) use chain_map::ChainMap;
pub(crate) use class::{
    BoundMethod, ClassGetItem, ClassMethod, ClassObject, ClassSubclasses, FunctionGet, Instance, PropertyAccessor,
    SlotDescriptor, SlotDescriptorKind, StaticMethod, SubclassEntry, SuperProxy, UserProperty, compute_c3_mro,
};
pub(crate) use counter::Counter;
pub(crate) use dataclass::Dataclass;
pub(crate) use datetime_types::{Date, Datetime, Time, Timedelta, Timezone};
pub(crate) use decimal::Decimal;
pub(crate) use defaultdict::DefaultDict;
pub(crate) use deque::Deque;
pub(crate) use dict::{Dict, DictItems, DictKeys, DictValues};
pub(crate) use fraction::Fraction;
pub(crate) use functools::{
    CachedProperty, FunctionWrapper, LruCache, PartialMethod, Placeholder, SingleDispatch, SingleDispatchMethod,
    SingleDispatchRegister, TotalOrderingMethod, Wraps,
};
pub(crate) use generator::{Generator, GeneratorState};
pub(crate) use generic_alias::{GenericAlias, make_generic_alias};
pub(crate) use iter::{OurosIter, TeeState};
pub(crate) use list::List;
pub(crate) use long_int::LongInt;
pub(crate) use mapping_proxy::MappingProxy;
pub(crate) use module::Module;
pub(crate) use namedtuple::{NamedTuple, NamedTupleFactory};
pub(crate) use operator_callables::{AttrGetter, ItemGetter, MethodCaller};
pub(crate) use ordered_dict::OrderedDict;
pub(crate) use partial::{CmpToKey, Partial};
pub(crate) use path::Path;
pub(crate) use property::Property;
pub(crate) use py_trait::{AttrCallResult, PyTrait};
pub(crate) use range::Range;
pub(crate) use re_types::{ReMatch, RePattern};
pub(crate) use set::{FrozenSet, Set, SetStorage};
pub(crate) use slice::Slice;
pub(crate) use stdlib_objects::{ExitCallback, ReScannerRule, StdlibObject};
pub(crate) use str::{Str, call_str_method};
pub(crate) use textwrap::TextWrapper;
pub(crate) use tuple::{Tuple, allocate_tuple};
pub(crate) use r#type::Type;
pub(crate) use uuid::{SafeUuid, SafeUuidKind, Uuid};
pub(crate) use weakref::WeakRef;
