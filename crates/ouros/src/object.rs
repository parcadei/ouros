use std::{
    fmt::{self, Write},
    hash::{Hash, Hasher},
};

use ahash::AHashSet;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::Zero;

use crate::{
    builtins::{Builtins, BuiltinsFunctions},
    exception_private::{ExcType, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::Interns,
    proxy::ProxyId,
    resource::{ResourceError, ResourceTracker},
    types::{
        Dataclass, LongInt, NamedTuple, Path, PyTrait, Type, allocate_tuple,
        bytes::{Bytes, bytes_repr},
        dict::Dict,
        list::List,
        set::{FrozenSet, Set},
        str::{Str, string_repr_fmt},
    },
    value::{EitherStr, Value},
};

/// A Python value that can be passed to or returned from the interpreter.
///
/// This is the public-facing type for Python values. It owns all its data and can be
/// freely cloned, serialized, or stored. Unlike the internal `Value` type, `Object`
/// does not require a heap for operations.
///
/// # Input vs Output Variants
///
/// Most variants can be used both as inputs (passed to `Executor::run()`) and outputs
/// (returned from execution). However:
/// - `Repr` is output-only: represents values that have no direct `Object` mapping
/// - `Exception` can be used as input (to raise) or output (when code raises)
///
/// # Hashability
///
/// Only immutable variants are Python-hashable.
/// The Rust `Hash` impl remains total (non-panicking) for API safety, while Python-level
/// hashability checks are enforced by runtime `Value::py_hash`.
///
/// # JSON Serialization
///
/// `Object` supports JSON serialization with natural mappings:
///
/// **Bidirectional (can serialize and deserialize):**
/// - `None` ↔ JSON `null`
/// - `Bool` ↔ JSON `true`/`false`
/// - `Int` ↔ JSON integer
/// - `Float` ↔ JSON float
/// - `String` ↔ JSON string
/// - `List` ↔ JSON array
/// - `Dict` ↔ JSON object (keys must be interns)
///
/// **Output-only (serialize only, cannot deserialize from JSON):**
/// - `Ellipsis` → `{"$ellipsis": true}`
/// - `Tuple` → `{"$tuple": [...]}`
/// - `Bytes` → `{"$bytes": [...]}`
/// - `Exception` → `{"$exception": {"type": "...", "arg": "..."}}`
/// - `Repr` → `{"$repr": "..."}`
///
/// # Binary Serialization
///
/// For binary serialization (e.g., with postcard), `Object` uses derived serde
/// with internally tagged format. This differs from the natural JSON format.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Object {
    /// Python's `Ellipsis` singleton (`...`).
    #[serde(alias = "ellipsis")]
    Ellipsis,
    /// Python's `None` singleton.
    #[serde(alias = "none", alias = "NoneType")]
    None,
    /// Python boolean (`True` or `False`).
    #[serde(alias = "bool")]
    Bool(bool),
    /// Python integer (64-bit signed).
    #[serde(alias = "int")]
    Int(i64),
    /// Python arbitrary-precision integer (larger than i64).
    BigInt(BigInt),
    /// Python float (64-bit IEEE 754).
    #[serde(alias = "float")]
    Float(f64),
    /// Python string (UTF-8).
    #[serde(alias = "str")]
    String(String),
    /// Python bytes object.
    #[serde(alias = "bytes")]
    Bytes(Vec<u8>),
    /// Python list (mutable sequence).
    #[serde(alias = "list")]
    List(Vec<Self>),
    /// Python tuple (immutable sequence).
    #[serde(alias = "tuple")]
    Tuple(Vec<Self>),
    /// Python named tuple (immutable sequence with named fields).
    ///
    /// Named tuples behave like tuples but also support attribute access by field name.
    /// The type_name is used in repr (e.g., "os.stat_result"), and field_names provides
    /// the attribute names for each position.
    NamedTuple {
        /// Type name for repr (e.g., "os.stat_result").
        type_name: String,
        /// Field names in order.
        field_names: Vec<String>,
        /// Values in order (same length as field_names).
        values: Vec<Self>,
    },
    /// Python dictionary (insertion-ordered mapping).
    #[serde(alias = "dict")]
    Dict(DictPairs),
    /// Python set (mutable, unordered collection of unique elements).
    #[serde(alias = "set")]
    Set(Vec<Self>),
    /// Python frozenset (immutable, unordered collection of unique elements).
    #[serde(alias = "frozenset")]
    FrozenSet(Vec<Self>),
    /// Python exception with type and optional message argument.
    Exception {
        /// The exception type (e.g., `ValueError`, `TypeError`).
        exc_type: ExcType,
        /// Optional string argument passed to the exception constructor.
        arg: Option<String>,
    },
    /// A Python type object (e.g., `int`, `str`, `list`).
    ///
    /// Returned by the `type()` builtin and can be compared with other types.
    Type(Type),
    BuiltinFunction(BuiltinsFunctions),
    /// Python `pathlib.Path` object (or technically a `PurePosixPath`).
    ///
    /// Represents a filesystem path. Can be used both as input (from host) and output.
    Path(String),
    /// A dataclass instance with class name, field names, attributes, method names, and mutability.
    Dataclass {
        /// The class name (e.g., "Point", "User").
        name: String,
        /// Identifier of the type, from `id(type(dc))` in python.
        type_id: u64,
        /// Declared field names in definition order (for repr).
        field_names: Vec<String>,
        /// All attribute name -> value mapping (includes fields and extra attrs).
        attrs: DictPairs,
        /// Method names that trigger external function calls.
        methods: Vec<String>,
        /// Whether this dataclass instance is immutable.
        frozen: bool,
    },
    /// Fallback for values that cannot be represented as other variants.
    ///
    /// Contains the `repr()` string of the original value.
    ///
    /// This is output-only and cannot be used as an input to `Executor::run()`.
    Repr(String),
    /// Represents a cycle detected during Value-to-Object conversion.
    ///
    /// When converting cyclic structures (e.g., `a = []; a.append(a)`), this variant
    /// is used to break the infinite recursion. Contains the heap ID and the type-specific
    /// placeholder string (e.g., `"[...]"` for lists, `"{...}"` for dicts).
    ///
    /// This is output-only and cannot be used as an input to `Executor::run()`.
    Cycle(HeapId, String),
    /// Opaque host-managed proxy handle.
    Proxy(u32),
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => f.write_str(s),
            Self::Cycle(_, placeholder) => f.write_str(placeholder),
            Self::Type(t) => write!(f, "<class '{t}'>"),
            _ => self.repr_fmt(f),
        }
    }
}

impl Object {
    /// Converts a `Value` into a `Object`, properly handling reference counting.
    ///
    /// Takes ownership of the `Value`, extracts its content to create a Object,
    /// then properly drops the Value via `drop_with_heap` to maintain reference counting.
    ///
    /// The `interns` parameter is used to look up interned string/bytes content.
    pub(crate) fn new(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Self {
        let py_obj = match &value {
            // Fast path: most benchmark returns are immediates, so avoid allocating
            // cycle-detection state via from_value() on the hot path.
            Value::Ellipsis => Self::Ellipsis,
            Value::None => Self::None,
            Value::Bool(b) => Self::Bool(*b),
            Value::Int(i) => Self::Int(*i),
            Value::Float(f) => Self::Float(*f),
            Value::InternString(string_id) => Self::String(interns.get_str(*string_id).to_owned()),
            Value::InternBytes(bytes_id) => Self::Bytes(interns.get_bytes(*bytes_id).to_owned()),
            Value::Builtin(Builtins::Type(t)) => Self::Type(*t),
            Value::Builtin(Builtins::ExcType(e)) => Self::Type(Type::Exception(*e)),
            Value::Builtin(Builtins::Function(f)) => Self::BuiltinFunction(*f),
            Value::Proxy(proxy_id) => Self::Proxy(proxy_id.raw()),
            #[cfg(feature = "ref-count-panic")]
            Value::Dereferenced => panic!("Dereferenced found while converting to Object"),
            _ => Self::from_value(&value, heap, interns),
        };
        value.drop_with_heap(heap);
        py_obj
    }

    /// Converts a borrowed runtime value to a public object without consuming it.
    ///
    /// This is used by introspection APIs (for example REPL variable inspection)
    /// that need a stable snapshot representation without mutating heap ownership.
    pub(crate) fn from_borrowed_value(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Self {
        Self::from_value(value, heap, interns)
    }

    /// Creates a new `Object` from something that can be converted into a `DictPairs`.
    pub fn dict(dict: impl Into<DictPairs>) -> Self {
        Self::Dict(dict.into())
    }

    /// Converts this `Object` into an `Value`, allocating on the heap if needed.
    ///
    /// Immediate values (None, Bool, Int, Float, Ellipsis, Exception) are created directly.
    /// Heap-allocated values (String, Bytes, List, Tuple, Dict) are allocated
    /// via the heap and wrapped in `Value::Ref`.
    ///
    /// # Errors
    /// Returns `InvalidInputError` if called on the `Repr` variant,
    /// as it is only valid as an output from code execution, not as an input.
    pub(crate) fn to_value(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Value, InvalidInputError> {
        match self {
            Self::Ellipsis => Ok(Value::Ellipsis),
            Self::None => Ok(Value::None),
            Self::Bool(b) => Ok(Value::Bool(b)),
            Self::Int(i) => Ok(Value::Int(i)),
            Self::BigInt(bi) => Ok(LongInt::new(bi).into_value(heap)?),
            Self::Float(f) => Ok(Value::Float(f)),
            Self::String(s) => Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(s)))?)),
            Self::Bytes(b) => Ok(Value::Ref(heap.allocate(HeapData::Bytes(Bytes::new(b)))?)),
            Self::List(items) => {
                let values: Vec<Value> = items
                    .into_iter()
                    .map(|item| item.to_value(heap, interns))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Ref(heap.allocate(HeapData::List(List::new(values)))?))
            }
            Self::Tuple(items) => {
                let values = items
                    .into_iter()
                    .map(|item| item.to_value(heap, interns))
                    .collect::<Result<_, _>>()?;
                allocate_tuple(values, heap).map_err(InvalidInputError::Resource)
            }
            Self::NamedTuple {
                type_name,
                field_names,
                values,
            } => {
                let values: Vec<Value> = values
                    .into_iter()
                    .map(|item| item.to_value(heap, interns))
                    .collect::<Result<_, _>>()?;
                let field_name_strs: Vec<EitherStr> = field_names.into_iter().map(Into::into).collect();
                let nt = NamedTuple::new(type_name, field_name_strs, values);
                Ok(Value::Ref(heap.allocate(HeapData::NamedTuple(nt))?))
            }
            Self::Dict(map) => {
                let pairs: Result<Vec<(Value, Value)>, InvalidInputError> = map
                    .into_iter()
                    .map(|(k, v)| Ok((k.to_value(heap, interns)?, v.to_value(heap, interns)?)))
                    .collect();
                let dict = Dict::from_pairs(pairs?, heap, interns)
                    .map_err(|_| InvalidInputError::invalid_type("unhashable dict keys"))?;
                Ok(Value::Ref(heap.allocate(HeapData::Dict(dict))?))
            }
            Self::Set(items) => {
                let mut set = Set::new();
                for item in items {
                    let value = item.to_value(heap, interns)?;
                    set.add(value, heap, interns)
                        .map_err(|_| InvalidInputError::invalid_type("unhashable set element"))?;
                }
                Ok(Value::Ref(heap.allocate(HeapData::Set(set))?))
            }
            Self::FrozenSet(items) => {
                let mut set = Set::new();
                for item in items {
                    let value = item.to_value(heap, interns)?;
                    set.add(value, heap, interns)
                        .map_err(|_| InvalidInputError::invalid_type("unhashable frozenset element"))?;
                }
                // Convert to frozenset by extracting storage
                let frozenset = FrozenSet::from_set(set);
                Ok(Value::Ref(heap.allocate(HeapData::FrozenSet(frozenset))?))
            }
            Self::Exception { exc_type, arg } => {
                let exc = SimpleException::new(exc_type, arg);
                Ok(Value::Ref(heap.allocate(HeapData::Exception(exc))?))
            }
            Self::Dataclass {
                name,
                type_id,
                field_names,
                attrs,
                methods,
                frozen,
            } => {
                // Convert attrs to Dict
                let pairs: Result<Vec<(Value, Value)>, InvalidInputError> = attrs
                    .into_iter()
                    .map(|(k, v)| Ok((k.to_value(heap, interns)?, v.to_value(heap, interns)?)))
                    .collect();
                let dict = Dict::from_pairs(pairs?, heap, interns)
                    .map_err(|_| InvalidInputError::invalid_type("unhashable dataclass attr keys"))?;
                // Convert methods Vec to AHashSet
                let methods_set: AHashSet<String> = methods.into_iter().collect();
                let dc = Dataclass::new(name, type_id, field_names, dict, methods_set, frozen);
                Ok(Value::Ref(heap.allocate(HeapData::Dataclass(dc))?))
            }
            Self::Path(s) => Ok(Value::Ref(heap.allocate(HeapData::Path(Path::new(s)))?)),
            Self::Type(t) => Ok(Value::Builtin(Builtins::Type(t))),
            Self::BuiltinFunction(f) => Ok(Value::Builtin(Builtins::Function(f))),
            Self::Proxy(proxy_id) => Ok(Value::Proxy(ProxyId::new(proxy_id))),
            Self::Repr(_) => Err(InvalidInputError::invalid_type("Repr")),
            Self::Cycle(_, _) => Err(InvalidInputError::invalid_type("Cycle")),
        }
    }

    fn from_value(object: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Self {
        let mut visited = AHashSet::new();
        Self::from_value_inner(object, heap, &mut visited, interns)
    }

    /// Internal helper for converting Value to Object with cycle detection.
    ///
    /// The `visited` set tracks HeapIds we're currently processing. When we encounter
    /// a HeapId already in the set, we've found a cycle and return `Object::Cycle`
    /// with an appropriate placeholder string.
    fn from_value_inner(
        object: &Value,
        heap: &Heap<impl ResourceTracker>,
        visited: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> Self {
        match object {
            Value::Undefined => panic!("Undefined found while converting to Object"),
            Value::Ellipsis => Self::Ellipsis,
            Value::None => Self::None,
            Value::Bool(b) => Self::Bool(*b),
            Value::Int(i) => Self::Int(*i),
            Value::Float(f) => Self::Float(*f),
            Value::InternString(string_id) => Self::String(interns.get_str(*string_id).to_owned()),
            Value::InternBytes(bytes_id) => Self::Bytes(interns.get_bytes(*bytes_id).to_owned()),
            Value::Ref(id) => {
                // Check for cycle
                if visited.contains(id) {
                    // Cycle detected - return appropriate placeholder
                    return match heap.get(*id) {
                        HeapData::List(_) | HeapData::Deque(_) => Self::Cycle(*id, "[...]".to_owned()),
                        HeapData::Tuple(_) | HeapData::NamedTuple(_) => Self::Cycle(*id, "(...)".to_owned()),
                        HeapData::Dict(_) | HeapData::DefaultDict(_) | HeapData::ChainMap(_) => {
                            Self::Cycle(*id, "{...}".to_owned())
                        }
                        _ => Self::Cycle(*id, "...".to_owned()),
                    };
                }

                // Mark this id as being visited
                visited.insert(*id);

                let result = match heap.get(*id) {
                    HeapData::Str(s) => Self::String(s.as_str().to_owned()),
                    HeapData::Bytes(b) => Self::Bytes(b.as_slice().to_owned()),
                    HeapData::Bytearray(b) => Self::Bytes(b.as_slice().to_owned()),
                    HeapData::List(list) => Self::List(
                        list.as_vec()
                            .iter()
                            .map(|obj| Self::from_value_inner(obj, heap, visited, interns))
                            .collect(),
                    ),
                    HeapData::Tuple(tuple) => Self::Tuple(
                        tuple
                            .as_vec()
                            .iter()
                            .map(|obj| Self::from_value_inner(obj, heap, visited, interns))
                            .collect(),
                    ),
                    HeapData::NamedTuple(nt) => Self::NamedTuple {
                        type_name: nt.name(interns).to_owned(),
                        field_names: nt
                            .field_names()
                            .iter()
                            .map(|field_name| field_name.as_str(interns).to_owned())
                            .collect(),
                        values: nt
                            .as_vec()
                            .iter()
                            .map(|obj| Self::from_value_inner(obj, heap, visited, interns))
                            .collect(),
                    },
                    HeapData::Dict(dict) => Self::Dict(DictPairs(
                        dict.into_iter()
                            .map(|(k, v)| {
                                (
                                    Self::from_value_inner(k, heap, visited, interns),
                                    Self::from_value_inner(v, heap, visited, interns),
                                )
                            })
                            .collect(),
                    )),
                    // Deque: convert to List for Object representation
                    HeapData::Deque(deque) => Self::List(
                        deque
                            .iter()
                            .map(|obj| Self::from_value_inner(obj, heap, visited, interns))
                            .collect(),
                    ),
                    // DefaultDict: convert to Dict for Object representation
                    HeapData::DefaultDict(dd) => {
                        let dict = dd.dict();
                        Self::Dict(DictPairs(
                            dict.iter()
                                .map(|(k, v)| {
                                    (
                                        Self::from_value_inner(k, heap, visited, interns),
                                        Self::from_value_inner(v, heap, visited, interns),
                                    )
                                })
                                .collect(),
                        ))
                    }
                    // ChainMap: convert merged view to Dict for Object representation
                    HeapData::ChainMap(chain_map) => Self::Dict(DictPairs(
                        chain_map
                            .flat()
                            .iter()
                            .map(|(k, v)| {
                                (
                                    Self::from_value_inner(k, heap, visited, interns),
                                    Self::from_value_inner(v, heap, visited, interns),
                                )
                            })
                            .collect(),
                    )),
                    HeapData::Set(set) => Self::Set(
                        set.storage()
                            .iter()
                            .map(|obj| Self::from_value_inner(obj, heap, visited, interns))
                            .collect(),
                    ),
                    HeapData::FrozenSet(frozenset) => Self::FrozenSet(
                        frozenset
                            .storage()
                            .iter()
                            .map(|obj| Self::from_value_inner(obj, heap, visited, interns))
                            .collect(),
                    ),
                    // Cells are internal closure implementation details
                    HeapData::Cell(inner) => {
                        // Show the cell's contents
                        Self::from_value_inner(inner, heap, visited, interns)
                    }
                    HeapData::Closure(..) | HeapData::FunctionDefaults(..) => {
                        Self::Repr(object.py_repr(heap, interns).into_owned())
                    }
                    HeapData::Range(range) => {
                        // Represent Range as a repr string since Object doesn't have a Range variant
                        let mut s = String::new();
                        let _ = range.py_repr_fmt(&mut s, heap, visited, interns);
                        Self::Repr(s)
                    }
                    HeapData::Exception(exc) => Self::Exception {
                        exc_type: exc.exc_type(),
                        arg: exc.arg().map(ToString::to_string),
                    },
                    HeapData::Dataclass(dc) => {
                        // Convert attrs to DictPairs
                        let attrs = DictPairs(
                            dc.attrs()
                                .into_iter()
                                .map(|(k, v)| {
                                    (
                                        Self::from_value_inner(k, heap, visited, interns),
                                        Self::from_value_inner(v, heap, visited, interns),
                                    )
                                })
                                .collect(),
                        );
                        // Convert methods set to sorted Vec for determinism
                        let mut methods: Vec<String> = dc.methods().iter().cloned().collect();
                        methods.sort();
                        Self::Dataclass {
                            name: dc.name(interns).to_owned(),
                            type_id: dc.type_id(),
                            field_names: dc.field_names().to_vec(),
                            attrs,
                            methods,
                            frozen: dc.is_frozen(),
                        }
                    }
                    HeapData::Iter(_) => {
                        // Iterators are internal objects - represent as a type string
                        Self::Repr("<iterator>".to_owned())
                    }
                    HeapData::Tee(_) => {
                        // Tee state is internal to itertools.tee
                        Self::Repr("<itertools.tee>".to_owned())
                    }
                    HeapData::LongInt(li) => Self::BigInt(li.inner().clone()),
                    HeapData::Module(m) => {
                        // Modules are represented as a repr string
                        Self::Repr(format!("<module '{}'>", interns.get_str(m.name())))
                    }
                    HeapData::Slice(slice) => {
                        // Represent Slice as a repr string since Object doesn't have a Slice variant
                        let mut s = String::new();
                        let _ = slice.py_repr_fmt(&mut s, heap, visited, interns);
                        Self::Repr(s)
                    }
                    HeapData::Coroutine(coro) => {
                        // Coroutines are represented as a repr string
                        let func = interns.get_function(coro.func_id);
                        let name = interns.get_str(func.name.name_id);
                        Self::Repr(format!("<coroutine object {name}>"))
                    }
                    HeapData::GatherFuture(gather) => {
                        // GatherFutures are represented as a repr string
                        Self::Repr(format!("<gather({})>", gather.item_count()))
                    }
                    HeapData::Path(path) => Self::Path(path.as_str().to_owned()),
                    HeapData::ClassObject(cls) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = cls.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::MappingProxy(mp) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = mp.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Instance(inst) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = inst.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::BoundMethod(bm) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = bm.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::SlotDescriptor(sd) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = sd.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::SuperProxy(sp) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = sp.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::StaticMethod(_) => Self::Repr("<staticmethod object>".to_owned()),
                    HeapData::ClassMethod(_) => Self::Repr("<classmethod object>".to_owned()),
                    HeapData::UserProperty(_) => Self::Repr("<property object>".to_owned()),
                    HeapData::PropertyAccessor(_) => Self::Repr("<property accessor>".to_owned()),
                    HeapData::GenericAlias(ga) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = ga.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::WeakRef(wr) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = wr.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::ClassSubclasses(cs) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = cs.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::ClassGetItem(cg) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = cg.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::FunctionGet(fg) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = fg.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Hash(h) => Self::Repr(h.py_repr()),
                    HeapData::ZlibCompress(_) => Self::Repr("<zlib.Compress object>".to_owned()),
                    HeapData::ZlibDecompress(_) => Self::Repr("<zlib.Decompress object>".to_owned()),
                    HeapData::Partial(_) => Self::Repr("<functools.partial object>".to_owned()),
                    HeapData::CmpToKey(_) => Self::Repr("<functools.cmp_to_key object>".to_owned()),
                    HeapData::ItemGetter(_) => Self::Repr("operator.itemgetter(...)".to_owned()),
                    HeapData::AttrGetter(_) => Self::Repr("operator.attrgetter(...)".to_owned()),
                    HeapData::MethodCaller(_) => Self::Repr("operator.methodcaller(...)".to_owned()),
                    HeapData::ReMatch(m) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = m.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::RePattern(p) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = p.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::NamedTupleFactory(factory) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = factory.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Counter(counter) => Self::Dict(DictPairs(
                        counter
                            .dict()
                            .into_iter()
                            .map(|(k, v)| {
                                (
                                    Self::from_value_inner(k, heap, visited, interns),
                                    Self::from_value_inner(v, heap, visited, interns),
                                )
                            })
                            .collect(),
                    )),
                    HeapData::OrderedDict(ordered) => Self::Dict(DictPairs(
                        ordered
                            .dict()
                            .into_iter()
                            .map(|(k, v)| {
                                (
                                    Self::from_value_inner(k, heap, visited, interns),
                                    Self::from_value_inner(v, heap, visited, interns),
                                )
                            })
                            .collect(),
                    )),
                    HeapData::LruCache(_) => Self::Repr("<functools.lru_cache object>".to_owned()),
                    HeapData::FunctionWrapper(_) => Self::Repr("<functools._FunctionWrapper object>".to_owned()),
                    HeapData::Wraps(_) => Self::Repr("<functools.wraps object>".to_owned()),
                    HeapData::TotalOrderingMethod(_) => Self::Repr("<functools.total_ordering method>".to_owned()),
                    HeapData::CachedProperty(_) => Self::Repr("<functools.cached_property object>".to_owned()),
                    HeapData::SingleDispatch(_) => Self::Repr("<functools.singledispatch object>".to_owned()),
                    HeapData::SingleDispatchRegister(_) => {
                        Self::Repr("<functools.singledispatch register object>".to_owned())
                    }
                    HeapData::SingleDispatchMethod(_) => {
                        Self::Repr("<functools.singledispatchmethod object>".to_owned())
                    }
                    HeapData::PartialMethod(_) => Self::Repr("<functools.partialmethod object>".to_owned()),
                    HeapData::Placeholder(_) => Self::Repr("functools.Placeholder".to_owned()),
                    HeapData::TextWrapper(_) => Self::Repr("<textwrap.TextWrapper object>".to_owned()),
                    HeapData::StdlibObject(obj) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = obj.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Generator(generator) => {
                        let func = interns.get_function(generator.func_id);
                        let name = interns.get_str(func.name.name_id);
                        Self::Repr(format!("<generator object {name}>"))
                    }
                    HeapData::Timedelta(td) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = td.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Date(d) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = d.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Datetime(dt) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = dt.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Time(t) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = t.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Timezone(tz) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = tz.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::Decimal(d) => Self::Repr(format!("Decimal('{d}')")),
                    HeapData::Fraction(f) => Self::Repr(format!("Fraction({}, {})", f.numerator(), f.denominator())),
                    HeapData::Uuid(uuid) => Self::Repr(format!("UUID('{}')", uuid.hyphenated())),
                    HeapData::SafeUuid(safe) => Self::Repr(safe.kind().display().to_owned()),
                    HeapData::ObjectNewImpl(_) => Self::Repr("<built-in method __new__>".to_string()),
                    // Dict views - use their repr
                    HeapData::DictKeys(dk) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = dk.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::DictValues(dv) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = dv.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                    HeapData::DictItems(di) => {
                        let mut s = String::new();
                        let mut visited_ids = ahash::AHashSet::new();
                        let _ = di.py_repr_fmt(&mut s, heap, &mut visited_ids, interns);
                        Self::Repr(s)
                    }
                };

                // Remove from visited set after processing
                visited.remove(id);
                result
            }
            Value::Builtin(Builtins::Type(t)) => Self::Type(*t),
            Value::Builtin(Builtins::ExcType(e)) => Self::Type(Type::Exception(*e)),
            Value::Builtin(Builtins::Function(f)) => Self::BuiltinFunction(*f),
            Value::Proxy(proxy_id) => Self::Proxy(proxy_id.raw()),
            #[cfg(feature = "ref-count-panic")]
            Value::Dereferenced => panic!("Dereferenced found while converting to Object"),
            _ => Self::Repr(object.py_repr(heap, interns).into_owned()),
        }
    }

    /// Returns the Python `repr()` string for this value.
    ///
    /// # Panics
    /// Could panic if out of memory.
    #[must_use]
    pub fn py_repr(&self) -> String {
        let mut s = String::new();
        self.repr_fmt(&mut s).expect("Unable to format repr display value");
        s
    }

    /// Converts this value to a natural JSON representation for MCP tool output.
    ///
    /// Unlike the derived `serde::Serialize` (which uses externally-tagged format like
    /// `{"Int": 42}`), this produces human-friendly JSON that MCP clients can consume
    /// directly without understanding Ouros's internal type system:
    ///
    /// - `None` → `null`
    /// - `Bool` → `true`/`false`
    /// - `Int` → JSON number
    /// - `BigInt` → `{"$bigint": "12345..."}`
    /// - `Float` → JSON number (NaN/Infinity → `null`)
    /// - `String` → JSON string
    /// - `Bytes` → `{"$bytes": [...]}`
    /// - `List` → JSON array
    /// - `Tuple` → `{"$tuple": [...]}`
    /// - `NamedTuple` → `{"$namedtuple": {"type": "...", "fields": {...}}}`
    /// - `Dict` → JSON object (keys coerced to strings via repr)
    /// - `Set`/`FrozenSet` → `{"$set": [...]}`/`{"$frozenset": [...]}`
    /// - `Exception` → `{"$exception": {"type": "...", "message": ...}}`
    /// - `Ellipsis` → `{"$ellipsis": true}`
    /// - `Path` → `{"$path": "..."}`
    /// - `Dataclass` → `{"$dataclass": {"name": "...", "fields": {...}}}`
    /// - `Type` → `{"$type": "..."}`
    /// - `BuiltinFunction` → `{"$builtin_function": "..."}`
    /// - `Repr` → `{"$repr": "..."}`
    /// - `Cycle` → `{"$cycle": "..."}`
    /// - `Proxy` → `{"$proxy": id}`
    ///
    /// The derived `Serialize` impl is preserved for binary serialization (postcard)
    /// and for MCP input deserialization where the tagged format is still accepted.
    #[must_use]
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::{Value as JV, json};
        match self {
            Self::None => JV::Null,
            Self::Ellipsis => json!({"$ellipsis": true}),
            Self::Bool(b) => JV::Bool(*b),
            Self::Int(i) => json!(i),
            Self::BigInt(bi) => json!({"$bigint": bi.to_string()}),
            Self::Float(f) => {
                if f.is_nan() || f.is_infinite() {
                    JV::Null
                } else {
                    json!(f)
                }
            }
            Self::String(s) => JV::String(s.clone()),
            Self::Bytes(b) => json!({"$bytes": b}),
            Self::List(items) => JV::Array(items.iter().map(Self::to_json_value).collect()),
            Self::Tuple(items) => json!({"$tuple": items.iter().map(Self::to_json_value).collect::<Vec<_>>()}),
            Self::NamedTuple {
                type_name,
                field_names,
                values,
            } => {
                let fields: serde_json::Map<String, JV> = field_names
                    .iter()
                    .zip(values)
                    .map(|(k, v)| (k.clone(), v.to_json_value()))
                    .collect();
                json!({"$namedtuple": {"type": type_name, "fields": fields}})
            }
            Self::Dict(pairs) => {
                let map: serde_json::Map<String, JV> = pairs
                    .iter()
                    .map(|(k, v)| {
                        // Use raw string for string keys (most common case) to produce
                        // clean JSON like {"name": "Alice"} instead of {"'name'": "Alice"}.
                        let key = match k {
                            Self::String(s) => s.clone(),
                            other => other.py_repr(),
                        };
                        (key, v.to_json_value())
                    })
                    .collect();
                JV::Object(map)
            }
            Self::Set(items) => json!({"$set": items.iter().map(Self::to_json_value).collect::<Vec<_>>()}),
            Self::FrozenSet(items) => {
                json!({"$frozenset": items.iter().map(Self::to_json_value).collect::<Vec<_>>()})
            }
            Self::Exception { exc_type, arg } => {
                json!({"$exception": {"type": exc_type.to_string(), "message": arg}})
            }
            Self::Type(t) => json!({"$type": t.to_string()}),
            Self::BuiltinFunction(f) => json!({"$builtin_function": f.to_string()}),
            Self::Path(p) => json!({"$path": p}),
            Self::Dataclass {
                name,
                field_names,
                attrs,
                ..
            } => {
                let fields: serde_json::Map<String, JV> = field_names
                    .iter()
                    .filter_map(|fname| {
                        attrs
                            .iter()
                            .find(|(k, _)| k == &Self::String(fname.clone()))
                            .map(|(_, v)| (fname.clone(), v.to_json_value()))
                    })
                    .collect();
                json!({"$dataclass": {"name": name, "fields": fields}})
            }
            Self::Repr(s) => json!({"$repr": s}),
            Self::Cycle(_, s) => json!({"$cycle": s}),
            Self::Proxy(id) => json!({"$proxy": id}),
        }
    }

    /// Converts a JSON value to a `Object`, supporting both natural JSON and
    /// the tagged format used by serde's derived `Deserialize`.
    ///
    /// This enables MCP clients to send values in plain JSON format:
    /// - `null` → `None`
    /// - `true`/`false` → `Bool`
    /// - integer → `Int`
    /// - float → `Float`
    /// - string → `String`
    /// - array → `List`
    /// - object → tries serde-tagged first (for `{"Int": 42}` backward compat),
    ///   then falls back to `Dict` with string keys
    ///
    /// The tagged-first approach means existing clients sending `{"Int": 42}` continue
    /// to work, while new clients can send plain `42`.
    pub fn from_json_value(value: serde_json::Value) -> Self {
        use serde_json::Value as JV;
        match value {
            JV::Null => Self::None,
            JV::Bool(b) => Self::Bool(b),
            JV::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Self::Float(f)
                } else {
                    // u64 that doesn't fit i64
                    Self::BigInt(BigInt::from(n.as_u64().unwrap_or(0)))
                }
            }
            JV::String(s) => Self::String(s),
            JV::Array(arr) => Self::List(arr.into_iter().map(Self::from_json_value).collect()),
            JV::Object(_) => {
                // Try tagged deserialization first for backward compat (e.g., {"Int": 42})
                if let Ok(obj) = serde_json::from_value::<Self>(value.clone()) {
                    return obj;
                }
                // Fall back to Dict with string keys
                if let JV::Object(map) = value {
                    let pairs: Vec<(Self, Self)> = map
                        .into_iter()
                        .map(|(k, v)| (Self::String(k), Self::from_json_value(v)))
                        .collect();
                    Self::Dict(DictPairs::from(pairs))
                } else {
                    unreachable!()
                }
            }
        }
    }

    fn repr_fmt(&self, f: &mut impl Write) -> fmt::Result {
        match self {
            Self::Ellipsis => f.write_str("Ellipsis"),
            Self::None => f.write_str("None"),
            Self::Bool(true) => f.write_str("True"),
            Self::Bool(false) => f.write_str("False"),
            Self::Int(v) => write!(f, "{v}"),
            Self::BigInt(v) => write!(f, "{v}"),
            Self::Float(v) => {
                let s = v.to_string();
                f.write_str(&s)?;
                if !s.contains('.') {
                    f.write_str(".0")?;
                }
                Ok(())
            }
            Self::String(s) => string_repr_fmt(s, f),
            Self::Bytes(b) => f.write_str(&bytes_repr(b)),
            Self::List(l) => {
                f.write_char('[')?;
                let mut iter = l.iter();
                if let Some(first) = iter.next() {
                    first.repr_fmt(f)?;
                    for item in iter {
                        f.write_str(", ")?;
                        item.repr_fmt(f)?;
                    }
                }
                f.write_char(']')
            }
            Self::Tuple(t) => {
                f.write_char('(')?;
                let mut iter = t.iter();
                if let Some(first) = iter.next() {
                    first.repr_fmt(f)?;
                    for item in iter {
                        f.write_str(", ")?;
                        item.repr_fmt(f)?;
                    }
                }
                f.write_char(')')
            }
            Self::NamedTuple {
                type_name,
                field_names,
                values,
            } => {
                // Format: type_name(field1=value1, field2=value2, ...)
                f.write_str(type_name)?;
                f.write_char('(')?;
                let mut first = true;
                for (name, value) in field_names.iter().zip(values) {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;
                    f.write_str(name)?;
                    f.write_char('=')?;
                    value.repr_fmt(f)?;
                }
                f.write_char(')')
            }
            Self::Dict(d) => {
                f.write_char('{')?;
                let mut iter = d.iter();
                if let Some((k, v)) = iter.next() {
                    k.repr_fmt(f)?;
                    f.write_str(": ")?;
                    v.repr_fmt(f)?;
                    for (k, v) in iter {
                        f.write_str(", ")?;
                        k.repr_fmt(f)?;
                        f.write_str(": ")?;
                        v.repr_fmt(f)?;
                    }
                }
                f.write_char('}')
            }
            Self::Set(s) => {
                if s.is_empty() {
                    f.write_str("set()")
                } else {
                    f.write_char('{')?;
                    let mut iter = s.iter();
                    if let Some(first) = iter.next() {
                        first.repr_fmt(f)?;
                        for item in iter {
                            f.write_str(", ")?;
                            item.repr_fmt(f)?;
                        }
                    }
                    f.write_char('}')
                }
            }
            Self::FrozenSet(fs) => {
                f.write_str("frozenset(")?;
                if !fs.is_empty() {
                    f.write_char('{')?;
                    let mut iter = fs.iter();
                    if let Some(first) = iter.next() {
                        first.repr_fmt(f)?;
                        for item in iter {
                            f.write_str(", ")?;
                            item.repr_fmt(f)?;
                        }
                    }
                    f.write_char('}')?;
                }
                f.write_char(')')
            }
            Self::Exception { exc_type, arg } => {
                let type_str: &'static str = exc_type.into();
                write!(f, "{type_str}(")?;

                if let Some(arg) = &arg {
                    string_repr_fmt(arg, f)?;
                }
                f.write_char(')')
            }
            Self::Dataclass {
                name,
                field_names,
                attrs,
                ..
            } => {
                // Format: ClassName(field1=value1, field2=value2, ...)
                // Only declared fields are shown, not extra attributes
                f.write_str(name)?;
                f.write_char('(')?;
                let mut first = true;
                for field_name in field_names {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;
                    f.write_str(field_name)?;
                    f.write_char('=')?;
                    // Look up value in attrs
                    let key = Self::String(field_name.clone());
                    if let Some(value) = attrs.iter().find(|(k, _)| k == &key).map(|(_, v)| v) {
                        value.repr_fmt(f)?;
                    } else {
                        f.write_str("<?>")?;
                    }
                }
                f.write_char(')')
            }
            Self::Path(p) => write!(f, "PosixPath('{p}')"),
            Self::Type(t) => write!(f, "<class '{t}'>"),
            Self::BuiltinFunction(func) => write!(f, "<built-in function {func}>"),
            Self::Repr(s) => f.write_str(s),
            Self::Cycle(_, placeholder) => f.write_str(placeholder),
            Self::Proxy(proxy_id) => write!(f, "<proxy #{proxy_id}>"),
        }
    }

    /// Returns `true` if this value is "truthy" according to Python's truth testing rules.
    ///
    /// In Python, the following values are considered falsy:
    /// - `None` and `Ellipsis`
    /// - `False`
    /// - Zero numeric values (`0`, `0.0`)
    /// - Empty sequences and collections (`""`, `b""`, `[]`, `()`, `{}`)
    ///
    /// All other values are truthy, including `Exception` and `Repr` variants.
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::None => false,
            Self::Ellipsis => true,
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::BigInt(bi) => !bi.is_zero(),
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::Bytes(b) => !b.is_empty(),
            Self::List(l) => !l.is_empty(),
            Self::Tuple(t) => !t.is_empty(),
            Self::NamedTuple { values, .. } => !values.is_empty(),
            Self::Dict(d) => !d.is_empty(),
            Self::Set(s) => !s.is_empty(),
            Self::FrozenSet(fs) => !fs.is_empty(),
            Self::Exception { .. } => true,
            Self::Path(_) => true,          // Path instances are always truthy
            Self::Dataclass { .. } => true, // Dataclass instances are always truthy
            Self::Type(_) | Self::BuiltinFunction(_) | Self::Repr(_) | Self::Cycle(_, _) | Self::Proxy(_) => true,
        }
    }

    /// Returns the Python type name for this value (e.g., `"int"`, `"str"`, `"list"`).
    ///
    /// These are the same names returned by Python's `type(x).__name__`.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::None => "NoneType",
            Self::Ellipsis => "ellipsis",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::BigInt(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Tuple(_) => "tuple",
            Self::NamedTuple { .. } => "namedtuple",
            Self::Dict(_) => "dict",
            Self::Set(_) => "set",
            Self::FrozenSet(_) => "frozenset",
            Self::Exception { .. } => "Exception",
            Self::Path(_) => "PosixPath",
            Self::Dataclass { .. } => "dataclass",
            Self::Type(_) => "type",
            Self::BuiltinFunction(_) => "builtin_function_or_method",
            Self::Repr(_) => "repr",
            Self::Cycle(_, _) => "cycle",
            Self::Proxy(_) => "proxy",
        }
    }
}

impl Hash for Object {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the discriminant first (but Int and BigInt share discriminant for consistency)
        match self {
            Self::Int(_) | Self::BigInt(_) => {
                // Use Int discriminant for both to maintain hash consistency
                std::mem::discriminant(&Self::Int(0)).hash(state);
            }
            _ => std::mem::discriminant(self).hash(state),
        }

        match self {
            Self::Ellipsis | Self::None => {}
            Self::Bool(bool) => bool.hash(state),
            Self::Int(i) => i.hash(state),
            Self::BigInt(bi) => {
                // For hash consistency, if BigInt fits in i64, hash as i64
                if let Ok(i) = i64::try_from(bi) {
                    i.hash(state);
                } else {
                    // For large BigInts, hash the signed bytes
                    bi.to_signed_bytes_le().hash(state);
                }
            }
            Self::Float(f) => f.to_bits().hash(state),
            Self::String(string) => string.hash(state),
            Self::Bytes(bytes) => bytes.hash(state),
            Self::Path(path) => path.hash(state),
            Self::Type(t) => t.to_string().hash(state),
            Self::Proxy(proxy_id) => proxy_id.hash(state),
            // Keep this impl total to avoid host panics when Objects are used in Rust maps.
            // Python-level hash semantics are enforced by Value::py_hash and dict/set ops.
            Self::Cycle(_, _) => self.type_name().hash(state),
            _ => self.type_name().hash(state),
        }
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Ellipsis, Self::Ellipsis) => true,
            (Self::None, Self::None) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            // Cross-compare Int and BigInt
            (Self::Int(a), Self::BigInt(b)) | (Self::BigInt(b), Self::Int(a)) => BigInt::from(*a) == *b,
            // Use to_bits() for float comparison to be consistent with Hash
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Tuple(a), Self::Tuple(b)) => a == b,
            (
                Self::NamedTuple {
                    type_name: a_type,
                    field_names: a_fields,
                    values: a_values,
                },
                Self::NamedTuple {
                    type_name: b_type,
                    field_names: b_fields,
                    values: b_values,
                },
            ) => a_type == b_type && a_fields == b_fields && a_values == b_values,
            // NamedTuple can compare with Tuple by values only (matching Python semantics)
            (Self::NamedTuple { values, .. }, Self::Tuple(t)) | (Self::Tuple(t), Self::NamedTuple { values, .. }) => {
                values == t
            }
            (Self::Dict(a), Self::Dict(b)) => a == b,
            (Self::Set(a), Self::Set(b)) => a == b,
            (Self::FrozenSet(a), Self::FrozenSet(b)) => a == b,
            (
                Self::Exception {
                    exc_type: a_type,
                    arg: a_arg,
                },
                Self::Exception {
                    exc_type: b_type,
                    arg: b_arg,
                },
            ) => a_type == b_type && a_arg == b_arg,
            (
                Self::Dataclass {
                    name: a_name,
                    type_id: a_type_id,
                    field_names: a_field_names,
                    attrs: a_attrs,
                    methods: a_methods,
                    frozen: a_frozen,
                },
                Self::Dataclass {
                    name: b_name,
                    type_id: b_type_id,
                    field_names: b_field_names,
                    attrs: b_attrs,
                    methods: b_methods,
                    frozen: b_frozen,
                },
            ) => {
                a_name == b_name
                    && a_type_id == b_type_id
                    && a_field_names == b_field_names
                    && a_attrs == b_attrs
                    && a_methods == b_methods
                    && a_frozen == b_frozen
            }
            (Self::Path(a), Self::Path(b)) => a == b,
            (Self::Repr(a), Self::Repr(b)) => a == b,
            (Self::Cycle(a, _), Self::Cycle(b, _)) => a == b,
            (Self::Type(a), Self::Type(b)) => a == b,
            (Self::Proxy(a), Self::Proxy(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Object {}

impl AsRef<Self> for Object {
    fn as_ref(&self) -> &Self {
        self
    }
}

/// Error returned when a `Object` cannot be converted to the requested Rust type.
///
/// This error is returned by the `TryFrom` implementations when attempting to extract
/// a specific type from a `Object` that holds a different variant.
#[derive(Debug)]
pub struct ConversionError {
    /// The type name that was expected (e.g., "int", "str").
    pub expected: &'static str,
    /// The actual type name of the `Object` (e.g., "list", "NoneType").
    pub actual: &'static str,
}

impl ConversionError {
    /// Creates a new `ConversionError` with the expected and actual type names.
    #[must_use]
    pub fn new(expected: &'static str, actual: &'static str) -> Self {
        Self { expected, actual }
    }
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected {}, got {}", self.expected, self.actual)
    }
}

impl std::error::Error for ConversionError {}

/// Error returned when a `Object` cannot be used as an input to code execution.
///
/// This can occur when:
/// - A `Object` variant (like `Repr`) is only valid as an output, not an input
/// - A resource limit (memory, allocations) is exceeded during conversion
#[derive(Debug, Clone)]
pub enum InvalidInputError {
    /// The input type is not valid for conversion to a runtime Value.
    /// The type name of the invalid input value
    InvalidType(&'static str),
    /// A resource limit was exceeded during conversion.
    Resource(ResourceError),
}

impl InvalidInputError {
    /// Creates a new `InvalidInputError` for the given type name.
    #[must_use]
    pub fn invalid_type(type_name: &'static str) -> Self {
        Self::InvalidType(type_name)
    }
}

impl fmt::Display for InvalidInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidType(type_name) => write!(f, "'{type_name}' is not a valid input value"),
            Self::Resource(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InvalidInputError {}

impl From<crate::resource::ResourceError> for InvalidInputError {
    fn from(err: crate::resource::ResourceError) -> Self {
        Self::Resource(err)
    }
}

/// Attempts to convert a Object to an i64 integer.
/// Returns an error if the object is not an Int variant.
impl TryFrom<&Object> for i64 {
    type Error = ConversionError;

    fn try_from(value: &Object) -> Result<Self, Self::Error> {
        match value {
            Object::Int(i) => Ok(*i),
            _ => Err(ConversionError::new("int", value.type_name())),
        }
    }
}

/// Attempts to convert a Object to an f64 float.
/// Returns an error if the object is not a Float or Int variant.
/// Int values are automatically converted to f64 to match python's behavior.
impl TryFrom<&Object> for f64 {
    type Error = ConversionError;

    fn try_from(value: &Object) -> Result<Self, Self::Error> {
        match value {
            Object::Float(f) => Ok(*f),
            Object::Int(i) => Ok(*i as Self),
            _ => Err(ConversionError::new("float", value.type_name())),
        }
    }
}

/// Attempts to convert a Object to a String.
/// Returns an error if the object is not a heap-allocated Str variant.
impl TryFrom<&Object> for String {
    type Error = ConversionError;

    fn try_from(value: &Object) -> Result<Self, Self::Error> {
        if let Object::String(s) = value {
            Ok(s.clone())
        } else {
            Err(ConversionError::new("str", value.type_name()))
        }
    }
}

/// Attempts to convert a `Object` to a bool.
/// Returns an error if the object is not a True or False variant.
/// Note: This does NOT use Python's truthiness rules (use Object::bool for that).
impl TryFrom<&Object> for bool {
    type Error = ConversionError;

    fn try_from(value: &Object) -> Result<Self, Self::Error> {
        match value {
            Object::Bool(b) => Ok(*b),
            _ => Err(ConversionError::new("bool", value.type_name())),
        }
    }
}

/// A collection of key-value pairs representing Python dictionary contents.
///
/// Used internally by `Object::Dict` to store dictionary entries while preserving
/// insertion order. Keys and values are both `Object` instances.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DictPairs(Vec<(Object, Object)>);

impl From<Vec<(Object, Object)>> for DictPairs {
    fn from(pairs: Vec<(Object, Object)>) -> Self {
        Self(pairs)
    }
}

impl From<IndexMap<Object, Object>> for DictPairs {
    fn from(map: IndexMap<Object, Object>) -> Self {
        Self(map.into_iter().collect())
    }
}

impl From<DictPairs> for IndexMap<Object, Object> {
    fn from(pairs: DictPairs) -> Self {
        pairs.into_iter().collect()
    }
}

impl IntoIterator for DictPairs {
    type Item = (Object, Object);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'a> IntoIterator for &'a DictPairs {
    type Item = &'a (Object, Object);
    type IntoIter = std::slice::Iter<'a, (Object, Object)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl FromIterator<(Object, Object)> for DictPairs {
    fn from_iter<T: IntoIterator<Item = (Object, Object)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl DictPairs {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &(Object, Object)> {
        self.0.iter()
    }
}
