use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::hash_map::DefaultHasher,
    fmt::{self, Write},
    hash::{Hash, Hasher},
    mem::discriminant,
    str::FromStr,
};

use ahash::AHashSet;
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};

use crate::{
    asyncio::CallId,
    builtins::{Builtins, BuiltinsFunctions},
    defer_drop,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{BytesId, ExtFunctionId, FunctionId, Interns, LongIntId, StaticStrings, StringId},
    modules::{
        ModuleFunctions, itertools::ItertoolsFunctions, statistics::create_normaldist_value, weakref::WeakrefFunctions,
    },
    proxy::ProxyId,
    py_hash::{cpython_hash_bytes_seed0, cpython_hash_float, cpython_hash_int, cpython_hash_str_seed0},
    resource::{LARGE_RESULT_THRESHOLD, ResourceTracker},
    types::{
        AttrCallResult, Dataclass, Decimal, Dict, DictItems, DictKeys, Fraction, FrozenSet, GeneratorState, LongInt,
        Property, PyTrait, SafeUuid, SafeUuidKind, Set, SetStorage, StdlibObject, Str, Type,
        bytes::{bytes_repr_fmt, get_byte_at_index, get_bytes_slice},
        path,
        str::{allocate_char, get_char_at_index, get_str_slice, string_repr_fmt},
    },
};

/// Primary value type representing Python objects at runtime.
///
/// This enum uses a hybrid design: small immediate values (Int, Bool, None) are stored
/// inline, while heap-allocated values (List, Str, Dict, etc.) are stored in the arena
/// and referenced via `Ref(HeapId)`.
///
/// NOTE: `Clone` is intentionally NOT derived. Use `clone_with_heap()` for heap values
/// or `clone_immediate()` for immediate values only. Direct cloning via `.clone()` would
/// bypass reference counting and cause memory leaks.
///
/// NOTE: it's important to keep this size small to minimize memory overhead!
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum Value {
    // Immediate values (stored inline, no heap allocation)
    Undefined,
    Ellipsis,
    None,
    /// Python's `NotImplemented` singleton.
    ///
    /// Returned by binary dunder methods (`__add__`, `__eq__`, etc.) to signal
    /// that the operation is not supported for the given operand types. The VM
    /// then tries the reflected operation on the other operand.
    NotImplemented,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// An interned string literal. The StringId references the string in the Interns table.
    /// To get the actual string content, use `interns.get(string_id)`.
    InternString(StringId),
    /// An interned bytes literal. The BytesId references the bytes in the Interns table.
    /// To get the actual bytes content, use `interns.get_bytes(bytes_id)`.
    InternBytes(BytesId),
    /// An interned long integer literal. The `LongIntId` references the `BigInt` in the Interns table.
    /// Used for integer literals exceeding i64 range. Converted to heap-allocated `LongInt` on load.
    InternLongInt(LongIntId),
    /// A builtin function or exception type
    Builtin(Builtins),
    /// A function from a module (not a global builtin).
    /// Module functions require importing a module to access (e.g., `asyncio.gather`).
    ModuleFunction(ModuleFunctions),
    /// A function defined in the module (not a closure, doesn't capture any variables)
    DefFunction(FunctionId),
    /// Reference to an external function defined on the host
    ExtFunction(ExtFunctionId),
    /// Opaque host-managed proxy handle.
    ///
    /// Proxy values are immediate handles that can round-trip between host and VM
    /// without allocating heap objects.
    Proxy(ProxyId),
    /// A marker value representing special objects like sys.stdout/stderr.
    /// These exist but have minimal functionality in the sandboxed environment.
    Marker(Marker),
    /// A property descriptor that computes its value when accessed.
    /// When retrieved via `py_getattr`, the property's getter is invoked.
    Property(Property),
    /// A pending external function call result.
    ///
    /// Created when the host calls `run_pending()` instead of `run(result)` for an
    /// external function call. The CallId correlates with the call that created it.
    /// When awaited, blocks the task until the host provides a result via `resume()`.
    ///
    /// ExternalFutures follow single-shot semantics like coroutines - awaiting an
    /// already-awaited ExternalFuture raises RuntimeError.
    ExternalFuture(CallId),

    // Heap-allocated values (stored in arena)
    Ref(HeapId),

    /// Sentinel value indicating this Value was properly cleaned up via `drop_with_heap`.
    /// Only exists when `ref-count-panic` feature is enabled. Used to verify reference counting
    /// correctness - if a `Ref` variant is dropped without calling `drop_with_heap`, the
    /// Drop impl will panic.
    #[cfg(feature = "ref-count-panic")]
    Dereferenced,
}

/// Drop implementation that panics if a `Ref` variant is dropped without calling `drop_with_heap`.
/// This helps catch reference counting bugs during development/testing.
/// Only enabled when the `ref-count-panic` feature is active.
#[cfg(feature = "ref-count-panic")]
impl Drop for Value {
    fn drop(&mut self) {
        if let Self::Ref(id) = self {
            panic!("Value::Ref({id:?}) dropped without calling drop_with_heap() - this is a reference counting bug");
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl PyTrait for Value {
    fn py_type(&self, heap: &Heap<impl ResourceTracker>) -> Type {
        match self {
            Self::Undefined => panic!("Cannot get type of undefined value"),
            Self::Ellipsis => Type::Ellipsis,
            Self::None => Type::NoneType,
            Self::NotImplemented => Type::NoneType, // NotImplemented has its own type in CPython, but NoneType works for our purposes
            Self::Bool(_) => Type::Bool,
            Self::Int(_) | Self::InternLongInt(_) => Type::Int,
            Self::Float(_) => Type::Float,
            Self::InternString(_) => Type::Str,
            Self::InternBytes(_) => Type::Bytes,
            Self::Builtin(c) => c.py_type(),
            Self::ModuleFunction(_) => Type::BuiltinFunction,
            Self::DefFunction(_) | Self::ExtFunction(_) => Type::Function,
            Self::Proxy(_) => Type::Object,
            Self::Marker(m) => m.py_type(),
            Self::Property(_) => Type::Property,
            Self::ExternalFuture(_) => Type::Coroutine,
            Self::Ref(id) => heap.get(*id).py_type(heap),
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot access Dereferenced object"),
        }
    }

    /// Returns 0 for Value since immediate values are stack-allocated.
    ///
    /// Heap-allocated values (Ref variants) have their size tracked when
    /// the HeapData is allocated, not here.
    fn py_estimate_size(&self) -> usize {
        // Value is stack-allocated; heap data is sized separately when allocated
        0
    }

    fn py_len(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<usize> {
        match self {
            // Count Unicode characters, not bytes, to match Python semantics
            Self::InternString(string_id) => Some(interns.get_str(*string_id).chars().count()),
            Self::InternBytes(bytes_id) => Some(interns.get_bytes(*bytes_id).len()),
            Self::Ref(id) => heap.get(*id).py_len(heap, interns),
            _ => None,
        }
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        if let (Self::Ref(id1), Self::Ref(id2)) = (self, other)
            && *id1 == *id2
        {
            return true;
        }
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            return left.partial_cmp(&right) == Some(Ordering::Equal);
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            return left == right;
        }
        if let Some((real, imag)) = extract_complex_components(self, heap) {
            if let Some((other_real, other_imag)) = extract_complex_components(other, heap) {
                return real == other_real && imag == other_imag;
            }
            if let Some(other_scalar) = extract_numeric_scalar(other, heap) {
                return real == other_scalar && imag == 0.0;
            }
        }
        if let Some(self_scalar) = extract_numeric_scalar(self, heap)
            && let Some((real, imag)) = extract_complex_components(other, heap)
        {
            return self_scalar == real && imag == 0.0;
        }

        match (self, other) {
            (Self::Undefined, _) => false,
            (_, Self::Undefined) => false,
            (Self::Int(v1), Self::Int(v2)) => v1 == v2,
            (Self::Bool(v1), Self::Bool(v2)) => v1 == v2,
            (Self::Bool(v1), Self::Int(v2)) => i64::from(*v1) == *v2,
            (Self::Int(v1), Self::Bool(v2)) => *v1 == i64::from(*v2),
            (Self::Float(v1), Self::Float(v2)) => v1 == v2,
            (Self::Int(v1), Self::Float(v2)) => (*v1 as f64) == *v2,
            (Self::Float(v1), Self::Int(v2)) => *v1 == (*v2 as f64),
            (Self::Bool(v1), Self::Float(v2)) => (i64::from(*v1) as f64) == *v2,
            (Self::Float(v1), Self::Bool(v2)) => *v1 == (i64::from(*v2) as f64),
            (Self::None, Self::None) => true,

            // Int == LongInt comparison
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    BigInt::from(*a) == *li.inner()
                } else if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*id) {
                    *a == *bits
                } else {
                    false
                }
            }
            // LongInt == Int comparison
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    *li.inner() == BigInt::from(*b)
                } else if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*id) {
                    *bits == *b
                } else {
                    false
                }
            }

            // For interned interns, compare by StringId first (fast path for same interned string)
            (Self::InternString(s1), Self::InternString(s2)) => s1 == s2,
            // for strings we need to account for the fact they might be either interned or not
            (Self::InternString(string_id), Self::Ref(id2)) => {
                if let HeapData::Str(s2) = heap.get(*id2) {
                    interns.get_str(*string_id) == s2.as_str()
                } else {
                    false
                }
            }
            (Self::Ref(id1), Self::InternString(string_id)) => {
                if let HeapData::Str(s1) = heap.get(*id1) {
                    s1.as_str() == interns.get_str(*string_id)
                } else {
                    false
                }
            }

            // For interned bytes, compare by content (bytes are not deduplicated unlike interns)
            (Self::InternBytes(b1), Self::InternBytes(b2)) => {
                // Fast path: same BytesId means same content
                b1 == b2 || interns.get_bytes(*b1) == interns.get_bytes(*b2)
            }
            // same for bytes
            (Self::InternBytes(bytes_id), Self::Ref(id2)) => {
                if let HeapData::Bytes(b2) = heap.get(*id2) {
                    interns.get_bytes(*bytes_id) == b2.as_slice()
                } else {
                    false
                }
            }
            (Self::Ref(id1), Self::InternBytes(bytes_id)) => {
                if let HeapData::Bytes(b1) = heap.get(*id1) {
                    b1.as_slice() == interns.get_bytes(*bytes_id)
                } else {
                    false
                }
            }

            (Self::Ref(id1), Self::Ref(id2)) => {
                // Guard against deeply nested container equality blowing the stack.
                if !heap.data_depth_enter() {
                    return false;
                }
                // Need to use with_two for proper borrow management
                let result = heap.with_two(*id1, *id2, |heap, left, right| left.py_eq(right, heap, interns));
                heap.data_depth_exit();
                result
            }

            // Builtins equality - just check the enums are equal
            (Self::Builtin(b1), Self::Builtin(b2)) => b1 == b2,
            // Module functions equality
            (Self::ModuleFunction(mf1), Self::ModuleFunction(mf2)) => mf1 == mf2,
            (Self::DefFunction(f1), Self::DefFunction(f2)) => f1 == f2,
            (Self::Proxy(p1), Self::Proxy(p2)) => p1 == p2,
            // Markers compare equal if they're the same variant
            (Self::Marker(m1), Self::Marker(m2)) => m1 == m2,
            // Properties compare equal if they're the same variant
            (Self::Property(p1), Self::Property(p2)) => p1 == p2,

            _ => false,
        }
    }

    fn py_cmp(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<Ordering> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            return left.partial_cmp(&right);
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            return left.partial_cmp(&right);
        }

        match (self, other) {
            (Self::Int(s), Self::Int(o)) => s.partial_cmp(o),
            (Self::Float(s), Self::Float(o)) => s.partial_cmp(o),
            (Self::Int(s), Self::Float(o)) => (*s as f64).partial_cmp(o),
            (Self::Float(s), Self::Int(o)) => s.partial_cmp(&(*o as f64)),
            (Self::Bool(s), _) => Self::Int(i64::from(*s)).py_cmp(other, heap, interns),
            (_, Self::Bool(s)) => self.py_cmp(&Self::Int(i64::from(*s)), heap, interns),
            // Int vs LongInt comparison
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    BigInt::from(*a).partial_cmp(li.inner())
                } else if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*id) {
                    a.partial_cmp(bits)
                } else {
                    None
                }
            }
            // LongInt vs Int comparison
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    li.inner().partial_cmp(&BigInt::from(*b))
                } else if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*id) {
                    bits.partial_cmp(b)
                } else {
                    None
                }
            }
            // LongInt vs LongInt comparison
            (Self::Ref(id1), Self::Ref(id2)) => {
                // Guard against deeply nested container comparison blowing the stack.
                if !heap.data_depth_enter() {
                    return None;
                }
                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                let result = if is_longint1 && is_longint2 {
                    heap.with_two(*id1, *id2, |_heap, left, right| {
                        if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                            a.inner().partial_cmp(b.inner())
                        } else {
                            None
                        }
                    })
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| match (left, right) {
                        (HeapData::Str(a), HeapData::Str(b)) => a.as_str().partial_cmp(b.as_str()),
                        (HeapData::Tuple(a), HeapData::Tuple(b)) => {
                            compare_sequence_lexicographically(a.as_vec(), b.as_vec(), heap, interns)
                        }
                        (HeapData::List(a), HeapData::List(b)) => {
                            compare_sequence_lexicographically(a.as_vec(), b.as_vec(), heap, interns)
                        }
                        (HeapData::Uuid(a), HeapData::Uuid(b)) => a.py_cmp(b, heap, interns),
                        (left, right) => left.py_cmp(right, heap, interns),
                    })
                };
                heap.data_depth_exit();
                result
            }
            (Self::InternString(s1), Self::InternString(s2)) => interns.get_str(*s1).partial_cmp(interns.get_str(*s2)),
            // InternString vs Ref(HeapData::Str)
            (Self::InternString(s1), Self::Ref(id)) => {
                if let HeapData::Str(s2) = heap.get(*id) {
                    interns.get_str(*s1).partial_cmp(s2.as_str())
                } else {
                    None
                }
            }
            // Ref(HeapData::Str) vs InternString
            (Self::Ref(id), Self::InternString(s2)) => {
                if let HeapData::Str(s1) = heap.get(*id) {
                    s1.as_str().partial_cmp(interns.get_str(*s2))
                } else {
                    None
                }
            }
            (Self::InternBytes(b1), Self::InternBytes(b2)) => {
                interns.get_bytes(*b1).partial_cmp(interns.get_bytes(*b2))
            }
            // InternBytes vs Ref(HeapData::Bytes)
            (Self::InternBytes(b1), Self::Ref(id)) => {
                if let HeapData::Bytes(b2) = heap.get(*id) {
                    interns.get_bytes(*b1).partial_cmp(b2.as_slice())
                } else {
                    None
                }
            }
            // Ref(HeapData::Bytes) vs InternBytes
            (Self::Ref(id), Self::InternBytes(b2)) => {
                if let HeapData::Bytes(b1) = heap.get(*id) {
                    b1.as_slice().partial_cmp(interns.get_bytes(*b2))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        if let Self::Ref(id) = self {
            stack.push(*id);
            // Mark as Dereferenced to prevent Drop panic
            #[cfg(feature = "ref-count-panic")]
            self.dec_ref_forget();
        }
    }

    fn py_bool(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        match self {
            Self::Undefined => false,
            Self::Ellipsis => true,
            Self::None | Self::NotImplemented => false,
            Self::Bool(b) => *b,
            Self::Int(v) => *v != 0,
            Self::Float(f) => *f != 0.0,
            // InternLongInt is always truthy (if it were zero, it would fit in i64)
            Self::InternLongInt(_) => true,
            Self::Builtin(_) | Self::ModuleFunction(_) => true, // Builtins are always truthy
            Self::DefFunction(_) | Self::ExtFunction(_) => true, // Functions are always truthy
            Self::Proxy(_) => true,                             // Proxies are always truthy
            Self::Marker(_) => true,                            // Markers are always truthy
            Self::Property(_) => true,                          // Properties are always truthy
            Self::ExternalFuture(_) => true,                    // ExternalFutures are always truthy
            Self::InternString(string_id) => !interns.get_str(*string_id).is_empty(),
            Self::InternBytes(bytes_id) => !interns.get_bytes(*bytes_id).is_empty(),
            Self::Ref(id) => heap.get(*id).py_bool(heap, interns),
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot access Dereferenced object"),
        }
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        match self {
            Self::Undefined => f.write_str("Undefined"),
            Self::Ellipsis => f.write_str("Ellipsis"),
            Self::None => f.write_str("None"),
            Self::NotImplemented => f.write_str("NotImplemented"),
            Self::Bool(true) => f.write_str("True"),
            Self::Bool(false) => f.write_str("False"),
            Self::Int(v) => write!(f, "{v}"),
            Self::InternLongInt(long_int_id) => write!(f, "{}", interns.get_long_int(*long_int_id)),
            Self::Float(v) => f.write_str(&float_repr(*v)),
            Self::Builtin(b) => b.py_repr_fmt(f),
            Self::ModuleFunction(mf) => mf.py_repr_fmt(f, self.public_id()),
            Self::DefFunction(f_id) => interns.get_function(*f_id).py_repr_fmt(f, interns, self.public_id()),
            Self::ExtFunction(f_id) => write!(
                f,
                "<function {} at 0x{:x}>",
                interns.get_external_function_name(*f_id),
                self.public_id()
            ),
            Self::Proxy(proxy_id) => write!(f, "<proxy #{}>", proxy_id.raw()),
            Self::InternString(string_id) => string_repr_fmt(interns.get_str(*string_id), f),
            Self::InternBytes(bytes_id) => bytes_repr_fmt(interns.get_bytes(*bytes_id), f),
            Self::Marker(m) => m.py_repr_fmt(f, self.public_id()),
            Self::Property(p) => write!(f, "<property {p:?}>"),
            Self::ExternalFuture(call_id) => write!(f, "<coroutine external_future({})>", call_id.raw()),
            Self::Ref(id) => {
                if heap_ids.contains(id) {
                    // Cycle detected - write type-specific placeholder following Python semantics
                    match heap.get(*id) {
                        HeapData::List(_) => f.write_str("[...]"),
                        HeapData::Tuple(_) => f.write_str("(...)"),
                        HeapData::Dict(_) => f.write_str("{...}"),
                        // Other types don't typically have cycles, but handle gracefully
                        _ => f.write_str("..."),
                    }
                } else if !heap.data_depth_enter() {
                    // Data recursion depth exceeded - truncate to prevent stack overflow
                    // on deeply nested (but non-circular) structures.
                    f.write_str("...")
                } else {
                    heap_ids.insert(*id);
                    let result = match heap.get(*id) {
                        HeapData::Generator(generator) => {
                            let func = interns.get_function(generator.func_id);
                            write!(
                                f,
                                "<generator object {} at 0x{:x}>",
                                func.qualname.as_str(interns),
                                self.public_id()
                            )
                        }
                        HeapData::Coroutine(coro) => {
                            let func = interns.get_function(coro.func_id);
                            write!(
                                f,
                                "<coroutine object {} at 0x{:x}>",
                                func.qualname.as_str(interns),
                                self.public_id()
                            )
                        }
                        HeapData::StdlibObject(StdlibObject::AsyncGenerator(_)) => {
                            write!(f, "<async_generator object at 0x{:x}>", self.public_id())
                        }
                        HeapData::StdlibObject(StdlibObject::AnextAwaitable(_)) => {
                            write!(f, "<anext_awaitable object at 0x{:x}>", self.public_id())
                        }
                        HeapData::Instance(instance) => {
                            instance.py_repr_fmt_with_id(f, heap, heap_ids, interns, self.public_id())
                        }
                        other => other.py_repr_fmt(f, heap, heap_ids, interns),
                    };
                    heap_ids.remove(id);
                    heap.data_depth_exit();
                    result
                }
            }
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot access Dereferenced object"),
        }
    }

    fn py_str(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Cow<'static, str> {
        match self {
            Self::InternString(string_id) => interns.get_str(*string_id).to_owned().into(),
            Self::Ref(id) => match heap.get(*id) {
                HeapData::StdlibObject(StdlibObject::AsyncGenerator(_)) => {
                    Cow::Owned(format!("<async_generator object at 0x{:x}>", self.public_id()))
                }
                HeapData::StdlibObject(StdlibObject::AnextAwaitable(_)) => {
                    Cow::Owned(format!("<anext_awaitable object at 0x{:x}>", self.public_id()))
                }
                HeapData::Instance(instance) => {
                    // Instance py_str may return enum/exception-specific strings.
                    // When it falls back to default repr, it needs the Value-level
                    // public_id for CPython-compatible output like
                    // `<__main__.C object at 0x...>`.
                    instance.py_str_with_id(heap, interns, self.public_id())
                }
                _ => heap.get(*id).py_str(heap, interns),
            },
            _ => self.py_repr(heap, interns),
        }
    }

    fn py_add(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.add(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            let result = left + right;
            return result.to_value(heap).map(Some);
        }

        if let (Some((mu1, sigma1)), Some((mu2, sigma2))) = (
            extract_normaldist_params(self, heap),
            extract_normaldist_params(other, heap),
        ) {
            return create_normaldist_value(heap, mu1 + mu2, f64::hypot(sigma1, sigma2)).map(Some);
        }
        if let Some((mu, sigma)) = extract_normaldist_params(self, heap)
            && let Some(shift) = extract_numeric_scalar(other, heap)
        {
            return create_normaldist_value(heap, mu + shift, sigma).map(Some);
        }
        if let Some(shift) = extract_numeric_scalar(self, heap)
            && let Some((mu, sigma)) = extract_normaldist_params(other, heap)
        {
            return create_normaldist_value(heap, mu + shift, sigma).map(Some);
        }
        if let Some((real, imag)) = extract_complex_components(self, heap) {
            if let Some((other_real, other_imag)) = extract_complex_components(other, heap) {
                return allocate_complex_value(heap, real + other_real, imag + other_imag).map(Some);
            }
            if let Some(other_scalar) = extract_numeric_scalar(other, heap) {
                return allocate_complex_value(heap, real + other_scalar, imag).map(Some);
            }
        }
        if let Some(self_scalar) = extract_numeric_scalar(self, heap)
            && let Some((real, imag)) = extract_complex_components(other, heap)
        {
            return allocate_complex_value(heap, self_scalar + real, imag).map(Some);
        }

        match (self, other) {
            // Int + Int with overflow detection
            (Self::Int(a), Self::Int(b)) => {
                if let Some(result) = a.checked_add(*b) {
                    Ok(Some(Self::Int(result)))
                } else {
                    // Overflow - promote to LongInt
                    let li = LongInt::from(*a) + LongInt::from(*b);
                    li.into_value(heap).map(Some)
                }
            }
            // Bool participates in integer arithmetic (True=1, False=0)
            (Self::Int(a), Self::Bool(b)) | (Self::Bool(b), Self::Int(a)) => {
                let b_int = i64::from(*b);
                if let Some(result) = a.checked_add(b_int) {
                    Ok(Some(Self::Int(result)))
                } else {
                    let li = LongInt::from(*a) + LongInt::from(b_int);
                    li.into_value(heap).map(Some)
                }
            }
            (Self::Bool(a), Self::Bool(b)) => Ok(Some(Self::Int(i64::from(*a) + i64::from(*b)))),
            // Int + LongInt
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::from(*a) + LongInt::new(li.inner().clone());
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // Bool + LongInt
            (Self::Bool(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::from(i64::from(*a)) + LongInt::new(li.inner().clone());
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // LongInt + Int
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::new(li.inner().clone()) + LongInt::from(*b);
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // LongInt + Bool
            (Self::Ref(id), Self::Bool(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::new(li.inner().clone()) + LongInt::from(i64::from(*b));
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            (Self::Float(v1), Self::Float(v2)) => Ok(Some(Self::Float(v1 + v2))),
            // Int + Float and Float + Int
            (Self::Int(a), Self::Float(b)) => Ok(Some(Self::Float(*a as f64 + b))),
            (Self::Float(a), Self::Int(b)) => Ok(Some(Self::Float(a + *b as f64))),
            (Self::Bool(a), Self::Float(b)) => Ok(Some(Self::Float(f64::from(*a) + b))),
            (Self::Float(a), Self::Bool(b)) => Ok(Some(Self::Float(a + f64::from(*b)))),
            (Self::Ref(id1), Self::Ref(id2)) => {
                // Check if both are LongInts
                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                if is_longint1 && is_longint2 {
                    heap.with_two(*id1, *id2, |heap, left, right| {
                        if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                            let result = LongInt::new(a.inner() + b.inner());
                            result.into_value(heap).map(Some)
                        } else {
                            Ok(None)
                        }
                    })
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| left.py_add(right, heap, interns))
                }
            }
            (Self::InternString(s1), Self::InternString(s2)) => {
                let concat = format!("{}{}", interns.get_str(*s1), interns.get_str(*s2));
                Ok(Some(Self::Ref(heap.allocate(HeapData::Str(concat.into()))?)))
            }
            // for strings we need to account for the fact they might be either interned or not
            (Self::InternString(string_id), Self::Ref(id2)) => {
                if let HeapData::Str(s2) = heap.get(*id2) {
                    let concat = format!("{}{}", interns.get_str(*string_id), s2.as_str());
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Str(concat.into()))?)))
                } else {
                    Ok(None)
                }
            }
            (Self::Ref(id1), Self::InternString(string_id)) => {
                if let HeapData::Str(s1) = heap.get(*id1) {
                    let concat = format!("{}{}", s1.as_str(), interns.get_str(*string_id));
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Str(concat.into()))?)))
                } else {
                    Ok(None)
                }
            }
            // same for bytes
            (Self::InternBytes(b1), Self::InternBytes(b2)) => {
                let bytes1 = interns.get_bytes(*b1);
                let bytes2 = interns.get_bytes(*b2);
                let mut b = Vec::with_capacity(bytes1.len() + bytes2.len());
                b.extend_from_slice(bytes1);
                b.extend_from_slice(bytes2);
                Ok(Some(Self::Ref(heap.allocate(HeapData::Bytes(b.into()))?)))
            }
            (Self::InternBytes(bytes_id), Self::Ref(id2)) => match heap.get(*id2) {
                HeapData::Bytes(b2) | HeapData::Bytearray(b2) => {
                    let bytes1 = interns.get_bytes(*bytes_id);
                    let mut b = Vec::with_capacity(bytes1.len() + b2.len());
                    b.extend_from_slice(bytes1);
                    b.extend_from_slice(b2);
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Bytes(b.into()))?)))
                }
                _ => Ok(None),
            },
            (Self::Ref(id1), Self::InternBytes(bytes_id)) => match heap.get(*id1) {
                HeapData::Bytes(b1) => {
                    let bytes2 = interns.get_bytes(*bytes_id);
                    let mut b = Vec::with_capacity(b1.len() + bytes2.len());
                    b.extend_from_slice(b1);
                    b.extend_from_slice(bytes2);
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Bytes(b.into()))?)))
                }
                HeapData::Bytearray(b1) => {
                    let bytes2 = interns.get_bytes(*bytes_id);
                    let mut b = Vec::with_capacity(b1.len() + bytes2.len());
                    b.extend_from_slice(b1);
                    b.extend_from_slice(bytes2);
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Bytearray(b.into()))?)))
                }
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }

    fn py_sub(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Self>, crate::resource::ResourceError> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.sub(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            let result = left - right;
            return result.to_value(heap).map(Some);
        }

        if let (Some((mu1, sigma1)), Some((mu2, sigma2))) = (
            extract_normaldist_params(self, heap),
            extract_normaldist_params(other, heap),
        ) {
            return create_normaldist_value(heap, mu1 - mu2, f64::hypot(sigma1, sigma2)).map(Some);
        }
        if let Some((mu, sigma)) = extract_normaldist_params(self, heap)
            && let Some(shift) = extract_numeric_scalar(other, heap)
        {
            return create_normaldist_value(heap, mu - shift, sigma).map(Some);
        }
        if let Some(shift) = extract_numeric_scalar(self, heap)
            && let Some((mu, sigma)) = extract_normaldist_params(other, heap)
        {
            return create_normaldist_value(heap, shift - mu, sigma).map(Some);
        }
        if let Some((real, imag)) = extract_complex_components(self, heap) {
            if let Some((other_real, other_imag)) = extract_complex_components(other, heap) {
                return allocate_complex_value(heap, real - other_real, imag - other_imag).map(Some);
            }
            if let Some(other_scalar) = extract_numeric_scalar(other, heap) {
                return allocate_complex_value(heap, real - other_scalar, imag).map(Some);
            }
        }
        if let Some(self_scalar) = extract_numeric_scalar(self, heap)
            && let Some((real, imag)) = extract_complex_components(other, heap)
        {
            return allocate_complex_value(heap, self_scalar - real, -imag).map(Some);
        }

        match (self, other) {
            // Int - Int with overflow detection
            (Self::Int(a), Self::Int(b)) => {
                if let Some(result) = a.checked_sub(*b) {
                    Ok(Some(Self::Int(result)))
                } else {
                    // Overflow - promote to LongInt
                    let li = LongInt::from(*a) - LongInt::from(*b);
                    li.into_value(heap).map(Some)
                }
            }
            // Bool participates in integer arithmetic (True=1, False=0)
            (Self::Int(a), Self::Bool(b)) => {
                let b_int = i64::from(*b);
                if let Some(result) = a.checked_sub(b_int) {
                    Ok(Some(Self::Int(result)))
                } else {
                    let li = LongInt::from(*a) - LongInt::from(b_int);
                    li.into_value(heap).map(Some)
                }
            }
            (Self::Bool(a), Self::Int(b)) => {
                let a_int = i64::from(*a);
                if let Some(result) = a_int.checked_sub(*b) {
                    Ok(Some(Self::Int(result)))
                } else {
                    let li = LongInt::from(a_int) - LongInt::from(*b);
                    li.into_value(heap).map(Some)
                }
            }
            (Self::Bool(a), Self::Bool(b)) => Ok(Some(Self::Int(i64::from(*a) - i64::from(*b)))),
            // Int - LongInt
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::from(*a) - LongInt::new(li.inner().clone());
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // Bool - LongInt
            (Self::Bool(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::from(i64::from(*a)) - LongInt::new(li.inner().clone());
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // LongInt - Int
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::new(li.inner().clone()) - LongInt::from(*b);
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // LongInt - Bool
            (Self::Ref(id), Self::Bool(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let result = LongInt::new(li.inner().clone()) - LongInt::from(i64::from(*b));
                    result.into_value(heap).map(Some)
                } else {
                    Ok(None)
                }
            }
            // LongInt - LongInt
            (Self::Ref(id1), Self::Ref(id2)) => {
                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                if is_longint1 && is_longint2 {
                    heap.with_two(*id1, *id2, |heap, left, right| {
                        if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                            let result = LongInt::new(a.inner() - b.inner());
                            result.into_value(heap).map(Some)
                        } else {
                            Ok(None)
                        }
                    })
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| left.py_sub(right, heap))
                }
            }
            // Float - Float
            (Self::Float(a), Self::Float(b)) => Ok(Some(Self::Float(a - b))),
            // Int - Float and Float - Int
            (Self::Int(a), Self::Float(b)) => Ok(Some(Self::Float(*a as f64 - b))),
            (Self::Float(a), Self::Int(b)) => Ok(Some(Self::Float(a - *b as f64))),
            (Self::Bool(a), Self::Float(b)) => Ok(Some(Self::Float(f64::from(*a) - b))),
            (Self::Float(a), Self::Bool(b)) => Ok(Some(Self::Float(a - f64::from(*b)))),
            _ => Ok(None),
        }
    }

    fn py_mod(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Self>> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.modulo(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            if right.is_zero() {
                return Err(ExcType::zero_division().into());
            }
            let numerator = left.numerator().clone() * right.denominator().clone();
            let denominator = left.denominator().clone() * right.numerator().clone();
            let quotient = numerator.div_floor(&denominator);
            let quotient_fraction = Fraction::new(quotient, BigInt::from(1))?;
            let result = left - (quotient_fraction * right);
            return result.to_value(heap).map(Some).map_err(Into::into);
        }

        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    // Python modulo: result has the same sign as divisor (b)
                    // Standard remainder (%) in Rust has same sign as dividend (a)
                    // We need to adjust when signs differ and remainder is non-zero
                    let r = *a % *b;
                    let result = if r != 0 && (*a < 0) != (*b < 0) { r + *b } else { r };
                    Ok(Some(Self::Int(result)))
                }
            }
            (Self::Bool(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    let a_int = i64::from(*a);
                    let r = a_int % *b;
                    let result = if r != 0 && (a_int < 0) != (*b < 0) { r + *b } else { r };
                    Ok(Some(Self::Int(result)))
                }
            }
            (Self::Int(a), Self::Bool(b)) => {
                let b_int = i64::from(*b);
                if b_int == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    let r = *a % b_int;
                    let result = if r != 0 && (*a < 0) != (b_int < 0) {
                        r + b_int
                    } else {
                        r
                    };
                    Ok(Some(Self::Int(result)))
                }
            }
            (Self::Bool(a), Self::Bool(b)) => {
                let a_int = i64::from(*a);
                let b_int = i64::from(*b);
                if b_int == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    let r = a_int % b_int;
                    let result = if r != 0 && (a_int < 0) != (b_int < 0) {
                        r + b_int
                    } else {
                        r
                    };
                    Ok(Some(Self::Int(result)))
                }
            }
            // Int % LongInt
            (Self::Int(a), Self::Ref(id)) => {
                // Clone to avoid borrow conflict with heap mutation
                let b_clone = if let HeapData::LongInt(li) = heap.get(*id) {
                    if li.is_zero() {
                        return Err(ExcType::zero_division().into());
                    }
                    li.inner().clone()
                } else {
                    return Ok(None);
                };
                let bi = BigInt::from(*a).mod_floor(&b_clone);
                Ok(Some(LongInt::new(bi).into_value(heap)?))
            }
            // Bool % LongInt
            (Self::Bool(a), Self::Ref(id)) => {
                let b_clone = if let HeapData::LongInt(li) = heap.get(*id) {
                    if li.is_zero() {
                        return Err(ExcType::zero_division().into());
                    }
                    li.inner().clone()
                } else {
                    return Ok(None);
                };
                let bi = BigInt::from(i64::from(*a)).mod_floor(&b_clone);
                Ok(Some(LongInt::new(bi).into_value(heap)?))
            }
            // LongInt % Int
            (Self::Ref(id), Self::Int(b)) => {
                if *b == 0 {
                    return Err(ExcType::zero_division().into());
                }
                // Clone to avoid borrow conflict with heap mutation
                let a_clone = if let HeapData::LongInt(li) = heap.get(*id) {
                    li.inner().clone()
                } else {
                    return Ok(None);
                };
                let bi = a_clone.mod_floor(&BigInt::from(*b));
                Ok(Some(LongInt::new(bi).into_value(heap)?))
            }
            // LongInt % Bool
            (Self::Ref(id), Self::Bool(b)) => {
                let b_int = i64::from(*b);
                if b_int == 0 {
                    return Err(ExcType::zero_division().into());
                }
                let a_clone = if let HeapData::LongInt(li) = heap.get(*id) {
                    li.inner().clone()
                } else {
                    return Ok(None);
                };
                let bi = a_clone.mod_floor(&BigInt::from(b_int));
                Ok(Some(LongInt::new(bi).into_value(heap)?))
            }
            // LongInt % LongInt
            (Self::Ref(id1), Self::Ref(id2)) => {
                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                if is_longint1 && is_longint2 {
                    // Check for zero division first
                    if matches!(heap.get(*id2), HeapData::LongInt(li) if li.is_zero()) {
                        return Err(ExcType::zero_division().into());
                    }
                    Ok(heap.with_two(*id1, *id2, |heap, left, right| {
                        if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                            let bi = a.inner().mod_floor(b.inner());
                            LongInt::new(bi).into_value(heap).map(Some)
                        } else {
                            Ok(None)
                        }
                    })?)
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| left.py_mod(right, heap))
                }
            }
            (Self::Float(v1), Self::Float(v2)) => {
                if *v2 == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(v1 % v2)))
                }
            }
            (Self::Float(v1), Self::Int(v2)) => {
                if *v2 == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(v1 % (*v2 as f64))))
                }
            }
            (Self::Int(v1), Self::Float(v2)) => {
                if *v2 == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float((*v1 as f64) % v2)))
                }
            }
            (Self::Bool(v1), Self::Float(v2)) => {
                if *v2 == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(f64::from(*v1) % v2)))
                }
            }
            (Self::Float(v1), Self::Bool(v2)) => {
                let divisor = f64::from(*v2);
                if divisor == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(v1 % divisor)))
                }
            }
            _ => Ok(None),
        }
    }

    fn py_mod_eq(&self, other: &Self, right_value: i64) -> Option<bool> {
        match (self, other) {
            (Self::Int(v1), Self::Int(v2)) => {
                // Use Python's modulo semantics (result has same sign as divisor)
                let r = *v1 % *v2;
                let result = if r != 0 && (*v1 < 0) != (*v2 < 0) { r + *v2 } else { r };
                Some(result == right_value)
            }
            (Self::Bool(v1), Self::Int(v2)) => {
                let left = i64::from(*v1);
                let r = left % *v2;
                let result = if r != 0 && (left < 0) != (*v2 < 0) { r + *v2 } else { r };
                Some(result == right_value)
            }
            (Self::Int(v1), Self::Bool(v2)) => {
                let right = i64::from(*v2);
                let r = *v1 % right;
                let result = if r != 0 && (*v1 < 0) != (right < 0) {
                    r + right
                } else {
                    r
                };
                Some(result == right_value)
            }
            (Self::Bool(v1), Self::Bool(v2)) => {
                let left = i64::from(*v1);
                let right = i64::from(*v2);
                let r = left % right;
                let result = if r != 0 && (left < 0) != (right < 0) {
                    r + right
                } else {
                    r
                };
                Some(result == right_value)
            }
            (Self::Float(v1), Self::Float(v2)) => Some(v1 % v2 == right_value as f64),
            (Self::Float(v1), Self::Int(v2)) => Some(v1 % (*v2 as f64) == right_value as f64),
            (Self::Int(v1), Self::Float(v2)) => Some((*v1 as f64) % v2 == right_value as f64),
            (Self::Bool(v1), Self::Float(v2)) => Some(f64::from(*v1) % v2 == right_value as f64),
            (Self::Float(v1), Self::Bool(v2)) => Some(v1 % f64::from(*v2) == right_value as f64),
            _ => None,
        }
    }

    fn py_iadd(
        &mut self,
        other: Self,
        heap: &mut Heap<impl ResourceTracker>,
        _self_id: Option<HeapId>,
        interns: &Interns,
    ) -> RunResult<bool> {
        match (&self, &other) {
            (Self::Int(v1), Self::Int(v2)) => {
                if let Some(result) = v1.checked_add(*v2) {
                    *self = Self::Int(result);
                } else {
                    // Overflow - promote to LongInt
                    let li = LongInt::from(*v1) + LongInt::from(*v2);
                    *self = li.into_value(heap)?;
                }
                Ok(true)
            }
            (Self::Int(v1), Self::Bool(v2)) => {
                let rhs = i64::from(*v2);
                if let Some(result) = v1.checked_add(rhs) {
                    *self = Self::Int(result);
                } else {
                    let li = LongInt::from(*v1) + LongInt::from(rhs);
                    *self = li.into_value(heap)?;
                }
                Ok(true)
            }
            (Self::Bool(v1), Self::Int(v2)) => {
                let lhs = i64::from(*v1);
                if let Some(result) = lhs.checked_add(*v2) {
                    *self = Self::Int(result);
                } else {
                    let li = LongInt::from(lhs) + LongInt::from(*v2);
                    *self = li.into_value(heap)?;
                }
                Ok(true)
            }
            (Self::Bool(v1), Self::Bool(v2)) => {
                *self = Self::Int(i64::from(*v1) + i64::from(*v2));
                Ok(true)
            }
            (Self::Float(v1), Self::Float(v2)) => {
                *self = Self::Float(*v1 + *v2);
                Ok(true)
            }
            (Self::InternString(s1), Self::InternString(s2)) => {
                let concat = format!("{}{}", interns.get_str(*s1), interns.get_str(*s2));
                *self = Self::Ref(heap.allocate(HeapData::Str(concat.into()))?);
                Ok(true)
            }
            (Self::InternString(string_id), Self::Ref(id2)) => {
                let result = if let HeapData::Str(s2) = heap.get(*id2) {
                    let concat = format!("{}{}", interns.get_str(*string_id), s2.as_str());
                    *self = Self::Ref(heap.allocate(HeapData::Str(concat.into()))?);
                    true
                } else {
                    false
                };
                // Drop the other value - we've consumed it
                other.drop_with_heap(heap);
                Ok(result)
            }
            (Self::Ref(id1), Self::InternString(string_id)) => {
                if let HeapData::Str(s1) = heap.get_mut(*id1) {
                    s1.as_string_mut().push_str(interns.get_str(*string_id));
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            // same for bytes
            (Self::InternBytes(b1), Self::InternBytes(b2)) => {
                let bytes1 = interns.get_bytes(*b1);
                let bytes2 = interns.get_bytes(*b2);
                let mut b = Vec::with_capacity(bytes1.len() + bytes2.len());
                b.extend_from_slice(bytes1);
                b.extend_from_slice(bytes2);
                *self = Self::Ref(heap.allocate(HeapData::Bytes(b.into()))?);
                Ok(true)
            }
            (Self::InternBytes(bytes_id), Self::Ref(id2)) => {
                let result = if let HeapData::Bytes(b2) = heap.get(*id2) {
                    let bytes1 = interns.get_bytes(*bytes_id);
                    let mut b = Vec::with_capacity(bytes1.len() + b2.len());
                    b.extend_from_slice(bytes1);
                    b.extend_from_slice(b2);
                    *self = Self::Ref(heap.allocate(HeapData::Bytes(b.into()))?);
                    true
                } else {
                    false
                };
                // Drop the other value - we've consumed it
                other.drop_with_heap(heap);
                Ok(result)
            }
            (Self::Ref(id1), Self::InternBytes(bytes_id)) => {
                if let HeapData::Bytes(b1) = heap.get_mut(*id1) {
                    b1.as_vec_mut().extend_from_slice(interns.get_bytes(*bytes_id));
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            (Self::Ref(id), Self::Ref(_)) => {
                heap.with_entry_mut(*id, |heap, data| data.py_iadd(other, heap, Some(*id), interns))
            }
            _ => {
                // Drop other if it's a Ref (ensure proper refcounting for unsupported type combinations)
                other.drop_with_heap(heap);
                Ok(false)
            }
        }
    }

    fn py_mult(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.mul(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            let result = left * right;
            return result.to_value(heap).map(Some).map_err(Into::into);
        }

        if let Some((mu, sigma)) = extract_normaldist_params(self, heap)
            && let Some(scale) = extract_numeric_scalar(other, heap)
        {
            return create_normaldist_value(heap, mu * scale, sigma * scale.abs())
                .map(Some)
                .map_err(Into::into);
        }
        if let Some(scale) = extract_numeric_scalar(self, heap)
            && let Some((mu, sigma)) = extract_normaldist_params(other, heap)
        {
            return create_normaldist_value(heap, mu * scale, sigma * scale.abs())
                .map(Some)
                .map_err(Into::into);
        }
        if let Some((real, imag)) = extract_complex_components(self, heap) {
            if let Some(scale) = extract_numeric_scalar(other, heap) {
                let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(
                    real * scale,
                    imag * scale,
                )))?;
                return Ok(Some(Self::Ref(id)));
            }
            if let Some((other_real, other_imag)) = extract_complex_components(other, heap) {
                let result_real = real * other_real - imag * other_imag;
                let result_imag = real * other_imag + imag * other_real;
                let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(
                    result_real,
                    result_imag,
                )))?;
                return Ok(Some(Self::Ref(id)));
            }
        }
        if let Some(scale) = extract_numeric_scalar(self, heap)
            && let Some((real, imag)) = extract_complex_components(other, heap)
        {
            let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(
                real * scale,
                imag * scale,
            )))?;
            return Ok(Some(Self::Ref(id)));
        }

        match (self, other) {
            // Numeric multiplication with overflow promotion to LongInt
            (Self::Int(a), Self::Int(b)) => {
                if let Some(result) = a.checked_mul(*b) {
                    Ok(Some(Self::Int(result)))
                } else {
                    // Overflow - promote to LongInt
                    let li = LongInt::from(*a) * LongInt::from(*b);
                    Ok(Some(li.into_value(heap)?))
                }
            }
            // Int * LongInt
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    // Check size before computing to prevent DoS
                    let a_bits = i64_bits(*a);
                    let b_bits = li.bits();
                    if let Some(estimated) = LongInt::estimate_mult_bytes(a_bits, b_bits)
                        && estimated > LARGE_RESULT_THRESHOLD
                    {
                        heap.tracker().check_large_result(estimated)?;
                    }
                    let result = LongInt::from(*a) * LongInt::new(li.inner().clone());
                    Ok(Some(result.into_value(heap)?))
                } else {
                    // Check for sequence repetition first.
                    if let Ok(count) = i64_to_repeat_count(*a)
                        && let Some(value) = heap.mult_sequence(*id, count)?
                    {
                        return Ok(Some(value));
                    }
                    // Timedelta supports multiplication by int.
                    if let HeapData::Timedelta(td) = heap.get(*id) {
                        let micros = td
                            .as_microseconds()
                            .checked_mul(*a)
                            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "timedelta overflow"))?;
                        let scaled = crate::types::datetime_types::Timedelta::from_microseconds(micros)?;
                        return Ok(Some(Self::Ref(heap.allocate(HeapData::Timedelta(scaled))?)));
                    }
                    Ok(None)
                }
            }
            // LongInt * Int
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    // Check size before computing to prevent DoS
                    let a_bits = li.bits();
                    let b_bits = i64_bits(*b);
                    if let Some(estimated) = LongInt::estimate_mult_bytes(a_bits, b_bits)
                        && estimated > LARGE_RESULT_THRESHOLD
                    {
                        heap.tracker().check_large_result(estimated)?;
                    }
                    let result = LongInt::new(li.inner().clone()) * LongInt::from(*b);
                    Ok(Some(result.into_value(heap)?))
                } else {
                    // Check for sequence repetition first.
                    if let Ok(count) = i64_to_repeat_count(*b)
                        && let Some(value) = heap.mult_sequence(*id, count)?
                    {
                        return Ok(Some(value));
                    }
                    // Timedelta supports multiplication by int.
                    if let HeapData::Timedelta(td) = heap.get(*id) {
                        let micros = td
                            .as_microseconds()
                            .checked_mul(*b)
                            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "timedelta overflow"))?;
                        let scaled = crate::types::datetime_types::Timedelta::from_microseconds(micros)?;
                        return Ok(Some(Self::Ref(heap.allocate(HeapData::Timedelta(scaled))?)));
                    }
                    Ok(None)
                }
            }
            // LongInt * LongInt or sequence * LongInt
            (Self::Ref(id1), Self::Ref(id2)) => {
                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                if is_longint1 && is_longint2 {
                    // LongInt * LongInt - get bits for size check
                    let a_bits = if let HeapData::LongInt(li) = heap.get(*id1) {
                        li.bits()
                    } else {
                        0
                    };
                    let b_bits = if let HeapData::LongInt(li) = heap.get(*id2) {
                        li.bits()
                    } else {
                        0
                    };
                    // Check size before computing to prevent DoS
                    if let Some(estimated) = LongInt::estimate_mult_bytes(a_bits, b_bits)
                        && estimated > LARGE_RESULT_THRESHOLD
                    {
                        heap.tracker().check_large_result(estimated)?;
                    }
                    Ok(heap.with_two(*id1, *id2, |heap, left, right| {
                        if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                            let result = LongInt::new(a.inner() * b.inner());
                            result.into_value(heap).map(Some)
                        } else {
                            Ok(None)
                        }
                    })?)
                } else if is_longint2 {
                    // sequence * LongInt - get the repeat count from LongInt
                    let count = if let HeapData::LongInt(li) = heap.get(*id2) {
                        longint_to_repeat_count(li)?
                    } else {
                        return Ok(None);
                    };
                    heap.mult_sequence(*id1, count)
                } else if is_longint1 {
                    // LongInt * sequence - get the repeat count from LongInt
                    let count = if let HeapData::LongInt(li) = heap.get(*id1) {
                        longint_to_repeat_count(li)?
                    } else {
                        return Ok(None);
                    };
                    heap.mult_sequence(*id2, count)
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| left.py_mult(right, heap, interns))
                }
            }
            (Self::Float(a), Self::Float(b)) => Ok(Some(Self::Float(a * b))),
            (Self::Int(a), Self::Float(b)) => Ok(Some(Self::Float(*a as f64 * b))),
            (Self::Float(a), Self::Int(b)) => Ok(Some(Self::Float(a * *b as f64))),

            // Bool numeric multiplication (True=1, False=0)
            (Self::Bool(a), Self::Int(b)) => {
                let a_int = i64::from(*a);
                Ok(Some(Self::Int(a_int * b)))
            }
            (Self::Int(a), Self::Bool(b)) => {
                let b_int = i64::from(*b);
                Ok(Some(Self::Int(a * b_int)))
            }
            (Self::Bool(a), Self::Float(b)) => {
                let a_float = if *a { 1.0 } else { 0.0 };
                Ok(Some(Self::Float(a_float * b)))
            }
            (Self::Float(a), Self::Bool(b)) => {
                let b_float = if *b { 1.0 } else { 0.0 };
                Ok(Some(Self::Float(a * b_float)))
            }
            (Self::Bool(a), Self::Bool(b)) => {
                let result = i64::from(*a) * i64::from(*b);
                Ok(Some(Self::Int(result)))
            }

            // String repetition: "ab" * 3 or 3 * "ab"
            (Self::InternString(s), Self::Int(n)) | (Self::Int(n), Self::InternString(s)) => {
                let count = i64_to_repeat_count(*n)?;
                let result = interns.get_str(*s).repeat(count);
                Ok(Some(Self::Ref(heap.allocate(HeapData::Str(result.into()))?)))
            }

            // Bytes repetition: b"ab" * 3 or 3 * b"ab"
            (Self::InternBytes(b), Self::Int(n)) | (Self::Int(n), Self::InternBytes(b)) => {
                let count = i64_to_repeat_count(*n)?;
                let result: Vec<u8> = interns.get_bytes(*b).repeat(count);
                Ok(Some(Self::Ref(heap.allocate(HeapData::Bytes(result.into()))?)))
            }

            // String repetition with LongInt: "ab" * bigint or bigint * "ab"
            (Self::InternString(s), Self::Ref(id)) | (Self::Ref(id), Self::InternString(s)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let count = longint_to_repeat_count(li)?;
                    let result = interns.get_str(*s).repeat(count);
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Str(result.into()))?)))
                } else {
                    Ok(None)
                }
            }

            // Bytes repetition with LongInt: b"ab" * bigint or bigint * b"ab"
            (Self::InternBytes(b), Self::Ref(id)) | (Self::Ref(id), Self::InternBytes(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    let count = longint_to_repeat_count(li)?;
                    let result: Vec<u8> = interns.get_bytes(*b).repeat(count);
                    Ok(Some(Self::Ref(heap.allocate(HeapData::Bytes(result.into()))?)))
                } else {
                    Ok(None)
                }
            }

            _ => Ok(None),
        }
    }

    fn py_div(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.div(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            if right.is_zero() {
                return Err(ExcType::zero_division().into());
            }
            let numerator = left.numerator().clone() * right.denominator().clone();
            let denominator = left.denominator().clone() * right.numerator().clone();
            let result = Fraction::new(numerator, denominator)?;
            return result.to_value(heap).map(Some).map_err(Into::into);
        }

        if let Some((mu, sigma)) = extract_normaldist_params(self, heap)
            && let Some(scale) = extract_numeric_scalar(other, heap)
        {
            if scale == 0.0 {
                return Err(ExcType::zero_division().into());
            }
            return create_normaldist_value(heap, mu / scale, sigma / scale.abs())
                .map(Some)
                .map_err(Into::into);
        }
        if let Some((real, imag)) = extract_complex_components(self, heap) {
            if let Some((other_real, other_imag)) = extract_complex_components(other, heap) {
                let denominator = other_real * other_real + other_imag * other_imag;
                if denominator == 0.0 {
                    return Err(ExcType::zero_division().into());
                }
                let result_real = (real * other_real + imag * other_imag) / denominator;
                let result_imag = (imag * other_real - real * other_imag) / denominator;
                return allocate_complex_value(heap, result_real, result_imag)
                    .map(Some)
                    .map_err(Into::into);
            }
            if let Some(other_scalar) = extract_numeric_scalar(other, heap) {
                if other_scalar == 0.0 {
                    return Err(ExcType::zero_division().into());
                }
                return allocate_complex_value(heap, real / other_scalar, imag / other_scalar)
                    .map(Some)
                    .map_err(Into::into);
            }
        }
        if let Some(self_scalar) = extract_numeric_scalar(self, heap)
            && let Some((real, imag)) = extract_complex_components(other, heap)
        {
            let denominator = real * real + imag * imag;
            if denominator == 0.0 {
                return Err(ExcType::zero_division().into());
            }
            let result_real = (self_scalar * real) / denominator;
            let result_imag = (-self_scalar * imag) / denominator;
            return allocate_complex_value(heap, result_real, result_imag)
                .map(Some)
                .map_err(Into::into);
        }

        match (self, other) {
            // True division always returns float
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(*a as f64 / *b as f64)))
                }
            }
            // Int / LongInt
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if li.is_zero() {
                        Err(ExcType::zero_division().into())
                    } else {
                        // Convert both to f64 for division
                        let a_f64 = *a as f64;
                        let b_f64 = li.to_f64().unwrap_or(f64::INFINITY);
                        Ok(Some(Self::Float(a_f64 / b_f64)))
                    }
                } else {
                    Ok(None)
                }
            }
            // LongInt / Int
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if *b == 0 {
                        Err(ExcType::zero_division().into())
                    } else {
                        // Convert both to f64 for division
                        let a_f64 = li.to_f64().unwrap_or(f64::INFINITY);
                        let b_f64 = *b as f64;
                        Ok(Some(Self::Float(a_f64 / b_f64)))
                    }
                } else if let HeapData::Timedelta(td) = heap.get(*id) {
                    if *b == 0 {
                        Err(ExcType::zero_division().into())
                    } else {
                        let micros = td.as_microseconds().div_euclid(*b);
                        let result = crate::types::datetime_types::Timedelta::from_microseconds(micros)?;
                        Ok(Some(Self::Ref(heap.allocate(HeapData::Timedelta(result))?)))
                    }
                } else {
                    Ok(None)
                }
            }
            // LongInt / LongInt or LongInt / Float or Float / LongInt
            (Self::Ref(id1), Self::Ref(id2)) => {
                if matches!(heap.get(*id1), HeapData::Path(_)) {
                    return path::path_div(*id1, other, heap, interns);
                }
                if matches!(heap.get(*id2), HeapData::Path(_)) {
                    return path::path_rdiv(self, *id2, heap, interns);
                }

                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                if is_longint1 && is_longint2 {
                    // Check for zero division first
                    if matches!(heap.get(*id2), HeapData::LongInt(li) if li.is_zero()) {
                        return Err(ExcType::zero_division().into());
                    }
                    Ok(
                        heap.with_two(*id1, *id2, |_heap, left, right| -> RunResult<Option<Self>> {
                            if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                                let a_f64 = a.to_f64().unwrap_or(f64::INFINITY);
                                let b_f64 = b.to_f64().unwrap_or(f64::INFINITY);
                                Ok(Some(Self::Float(a_f64 / b_f64)))
                            } else {
                                Ok(None)
                            }
                        })?,
                    )
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| left.py_div(right, heap, interns))
                }
            }
            // LongInt / Float
            (Self::Ref(id), Self::Float(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if *b == 0.0 {
                        Err(ExcType::zero_division().into())
                    } else {
                        let a_f64 = li.to_f64().unwrap_or(f64::INFINITY);
                        Ok(Some(Self::Float(a_f64 / b)))
                    }
                } else {
                    Ok(None)
                }
            }
            // Float / LongInt
            (Self::Float(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if li.is_zero() {
                        Err(ExcType::zero_division().into())
                    } else {
                        let b_f64 = li.to_f64().unwrap_or(f64::INFINITY);
                        Ok(Some(Self::Float(a / b_f64)))
                    }
                } else {
                    Ok(None)
                }
            }
            (Self::Float(a), Self::Float(b)) => {
                if *b == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(a / b)))
                }
            }
            (Self::Int(a), Self::Float(b)) => {
                if *b == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(*a as f64 / b)))
                }
            }
            (Self::Float(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(a / *b as f64)))
                }
            }
            // Bool division (True=1, False=0)
            (Self::Bool(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(f64::from(*a) / *b as f64)))
                }
            }
            (Self::Int(a), Self::Bool(b)) => {
                if *b {
                    Ok(Some(Self::Float(*a as f64))) // a / 1 = a
                } else {
                    Err(ExcType::zero_division().into())
                }
            }
            (Self::Bool(a), Self::Float(b)) => {
                if *b == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float(f64::from(*a) / b)))
                }
            }
            (Self::Float(a), Self::Bool(b)) => {
                if *b {
                    Ok(Some(Self::Float(*a))) // a / 1.0 = a
                } else {
                    Err(ExcType::zero_division().into())
                }
            }
            (Self::Bool(a), Self::Bool(b)) => {
                if *b {
                    Ok(Some(Self::Float(f64::from(*a)))) // a / 1 = a
                } else {
                    Err(ExcType::zero_division().into())
                }
            }
            _ => {
                // Check for Path / (str or Path) - path concatenation
                if let Self::Ref(id) = self
                    && matches!(heap.get(*id), HeapData::Path(_))
                {
                    return path::path_div(*id, other, heap, interns);
                }
                // Reverse path concatenation: str / Path
                if let Self::Ref(id) = other
                    && matches!(heap.get(*id), HeapData::Path(_))
                {
                    return path::path_rdiv(self, *id, heap, interns);
                }
                Ok(None)
            }
        }
    }

    fn py_floordiv(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Value>> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.floor_div(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            if right.is_zero() {
                return Err(ExcType::zero_division().into());
            }
            let numerator = left.numerator().clone() * right.denominator().clone();
            let denominator = left.denominator().clone() * right.numerator().clone();
            let result = numerator.div_floor(&denominator);
            return LongInt::new(result).into_value(heap).map(Some).map_err(Into::into);
        }

        match (self, other) {
            // Floor division: int // int returns int
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    // Python floor division rounds toward negative infinity
                    // div_euclid doesn't match Python semantics, so compute manually
                    let d = a / b;
                    let r = a % b;
                    // If there's a remainder and signs differ, round down (toward -∞)
                    let result = if r != 0 && (*a < 0) != (*b < 0) { d - 1 } else { d };
                    Ok(Some(Self::Int(result)))
                }
            }
            // Int // LongInt
            (Self::Int(a), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if li.is_zero() {
                        Err(ExcType::zero_division().into())
                    } else {
                        let bi = BigInt::from(*a).div_floor(li.inner());
                        Ok(Some(LongInt::new(bi).into_value(heap)?))
                    }
                } else {
                    Ok(None)
                }
            }
            // LongInt // Int
            (Self::Ref(id), Self::Int(b)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if *b == 0 {
                        Err(ExcType::zero_division().into())
                    } else {
                        let bi = li.inner().div_floor(&BigInt::from(*b));
                        Ok(Some(LongInt::new(bi).into_value(heap)?))
                    }
                } else if let HeapData::Timedelta(td) = heap.get(*id) {
                    if *b == 0 {
                        Err(ExcType::zero_division().into())
                    } else {
                        let micros = td.as_microseconds().div_euclid(*b);
                        let result = crate::types::datetime_types::Timedelta::from_microseconds(micros)?;
                        Ok(Some(Self::Ref(heap.allocate(HeapData::Timedelta(result))?)))
                    }
                } else {
                    Ok(None)
                }
            }
            // LongInt // LongInt
            (Self::Ref(id1), Self::Ref(id2)) => {
                let is_longint1 = matches!(heap.get(*id1), HeapData::LongInt(_));
                let is_longint2 = matches!(heap.get(*id2), HeapData::LongInt(_));
                if is_longint1 && is_longint2 {
                    // Check for zero division first
                    if matches!(heap.get(*id2), HeapData::LongInt(li) if li.is_zero()) {
                        return Err(ExcType::zero_division().into());
                    }
                    Ok(heap.with_two(*id1, *id2, |heap, left, right| {
                        if let (HeapData::LongInt(a), HeapData::LongInt(b)) = (left, right) {
                            let bi = a.inner().div_floor(b.inner());
                            LongInt::new(bi).into_value(heap).map(Some)
                        } else {
                            Ok(None)
                        }
                    })?)
                } else {
                    heap.with_two(*id1, *id2, |heap, left, right| left.py_floordiv(right, heap))
                }
            }
            // Float floor division returns float
            (Self::Float(a), Self::Float(b)) => {
                if *b == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float((a / b).floor())))
                }
            }
            (Self::Int(a), Self::Float(b)) => {
                if *b == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float((*a as f64 / b).floor())))
                }
            }
            (Self::Float(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float((a / *b as f64).floor())))
                }
            }
            // Bool floor division (True=1, False=0)
            (Self::Bool(a), Self::Int(b)) => {
                if *b == 0 {
                    Err(ExcType::zero_division().into())
                } else {
                    let a_int = i64::from(*a);
                    // Use same floor division logic as Int // Int
                    let d = a_int / b;
                    let r = a_int % b;
                    let result = if r != 0 && (a_int < 0) != (*b < 0) { d - 1 } else { d };
                    Ok(Some(Self::Int(result)))
                }
            }
            (Self::Int(a), Self::Bool(b)) => {
                if *b {
                    Ok(Some(Self::Int(*a))) // a // 1 = a
                } else {
                    Err(ExcType::zero_division().into())
                }
            }
            (Self::Bool(a), Self::Float(b)) => {
                if *b == 0.0 {
                    Err(ExcType::zero_division().into())
                } else {
                    Ok(Some(Self::Float((f64::from(*a) / b).floor())))
                }
            }
            (Self::Float(a), Self::Bool(b)) => {
                if *b {
                    Ok(Some(Self::Float(a.floor()))) // a // 1.0 = floor(a)
                } else {
                    Err(ExcType::zero_division().into())
                }
            }
            (Self::Bool(a), Self::Bool(b)) => {
                if *b {
                    Ok(Some(Self::Int(i64::from(*a)))) // a // 1 = a
                } else {
                    Err(ExcType::zero_division().into())
                }
            }
            _ => Ok(None),
        }
    }

    fn py_pow(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Value>> {
        if let Some((left, right)) = decimal_operands(self, other, heap) {
            let result = left.pow(&right);
            let id = heap.allocate(HeapData::Decimal(result))?;
            return Ok(Some(Self::Ref(id)));
        }
        if let Some((left, right)) = fraction_operands(self, other, heap) {
            if right.denominator() != &BigInt::from(1) {
                return Ok(None);
            }
            let Some(exp_i64) = right.numerator().to_i64() else {
                return Err(SimpleException::new_msg(ExcType::OverflowError, "exponent too large").into());
            };
            if exp_i64 < 0 && left.is_zero() {
                return Err(ExcType::zero_negative_power());
            }

            let Ok(exp_u32) = u32::try_from(exp_i64.unsigned_abs()) else {
                return Err(SimpleException::new_msg(ExcType::OverflowError, "exponent too large").into());
            };

            let (numerator, denominator) = if exp_i64 >= 0 {
                (
                    left.numerator().clone().pow(exp_u32),
                    left.denominator().clone().pow(exp_u32),
                )
            } else {
                (
                    left.denominator().clone().pow(exp_u32),
                    left.numerator().clone().pow(exp_u32),
                )
            };
            let result = Fraction::new(numerator, denominator)?;
            return result.to_value(heap).map(Some).map_err(Into::into);
        }
        if let Some((base_real, base_imag)) = extract_complex_or_scalar_components(self, heap)
            && let Some((exp_real, exp_imag)) = extract_complex_or_scalar_components(other, heap)
            && (is_runtime_complex(self, heap) || is_runtime_complex(other, heap))
        {
            let (result_real, result_imag) = complex_pow_components(base_real, base_imag, exp_real, exp_imag)?;
            return allocate_complex_value(heap, result_real, result_imag)
                .map(Some)
                .map_err(Into::into);
        }

        match (self, other) {
            (Self::Int(base), Self::Int(exp)) => {
                if *base == 0 && *exp < 0 {
                    Err(ExcType::zero_negative_power())
                } else if *exp >= 0 {
                    // Positive exponent: try to return int, promote to LongInt on overflow
                    if let Ok(exp_u32) = u32::try_from(*exp) {
                        if let Some(result) = base.checked_pow(exp_u32) {
                            Ok(Some(Self::Int(result)))
                        } else {
                            // Overflow - promote to LongInt
                            // Check size before computing to prevent DoS
                            check_pow_size(i64_bits(*base), u64::from(exp_u32), heap)?;
                            let bi = BigInt::from(*base).pow(exp_u32);
                            Ok(Some(LongInt::new(bi).into_value(heap)?))
                        }
                    } else {
                        // exp > u32::MAX - use BigInt with modpow-style exponentiation
                        // For very large exponents, we still need LongInt
                        // Safety: exp >= 0 is guaranteed by the outer if condition
                        #[expect(clippy::cast_sign_loss)]
                        let exp_u64 = *exp as u64;
                        // Check size before computing to prevent DoS
                        check_pow_size(i64_bits(*base), exp_u64, heap)?;
                        let bi = bigint_pow(BigInt::from(*base), exp_u64);
                        Ok(Some(LongInt::new(bi).into_value(heap)?))
                    }
                } else {
                    // Negative exponent: return float
                    // Use powi if exp fits in i32, otherwise use powf
                    if let Ok(exp_i32) = i32::try_from(*exp) {
                        Ok(Some(Self::Float((*base as f64).powi(exp_i32))))
                    } else {
                        Ok(Some(Self::Float((*base as f64).powf(*exp as f64))))
                    }
                }
            }
            // LongInt ** Int
            (Self::Ref(id), Self::Int(exp)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if li.is_zero() && *exp < 0 {
                        Err(ExcType::zero_negative_power())
                    } else if *exp >= 0 {
                        // Use BigInt pow for positive exponents
                        if let Ok(exp_u32) = u32::try_from(*exp) {
                            // Check size before computing to prevent DoS
                            check_pow_size(li.bits(), u64::from(exp_u32), heap)?;
                            let bi = li.inner().pow(exp_u32);
                            Ok(Some(LongInt::new(bi).into_value(heap)?))
                        } else {
                            // Safety: exp >= 0 is guaranteed by the outer if condition
                            #[expect(clippy::cast_sign_loss)]
                            let exp_u64 = *exp as u64;
                            // Check size before computing to prevent DoS
                            check_pow_size(li.bits(), exp_u64, heap)?;
                            let bi = bigint_pow(li.inner().clone(), exp_u64);
                            Ok(Some(LongInt::new(bi).into_value(heap)?))
                        }
                    } else {
                        // Negative exponent: return float (LongInt base becomes 0.0 for large values)
                        if let Some(base_f64) = li.to_f64() {
                            if let Ok(exp_i32) = i32::try_from(*exp) {
                                Ok(Some(Self::Float(base_f64.powi(exp_i32))))
                            } else {
                                Ok(Some(Self::Float(base_f64.powf(*exp as f64))))
                            }
                        } else {
                            // Base too large for f64, result approaches 0
                            Ok(Some(Self::Float(0.0)))
                        }
                    }
                } else {
                    Ok(None)
                }
            }
            // Int ** LongInt (only small positive exponents make sense)
            (Self::Int(base), Self::Ref(id)) => {
                if let HeapData::LongInt(li) = heap.get(*id) {
                    if *base == 0 && li.is_negative() {
                        Err(ExcType::zero_negative_power())
                    } else if !li.is_negative() {
                        // For very large exponents, most results are huge or 0/1
                        // Check for x ** 0 = 1 first (including 0 ** 0 = 1)
                        if li.is_zero() {
                            Ok(Some(Self::Int(1)))
                        } else if *base == 0 {
                            Ok(Some(Self::Int(0)))
                        } else if *base == 1 {
                            Ok(Some(Self::Int(1)))
                        } else if *base == -1 {
                            // (-1) ** n = 1 if n is even, -1 if n is odd
                            let is_even = (li.inner() % 2i32).is_zero();
                            Ok(Some(Self::Int(if is_even { 1 } else { -1 })))
                        } else if let Some(exp_u32) = li.to_u32() {
                            // Reasonable exponent size
                            if let Some(result) = base.checked_pow(exp_u32) {
                                Ok(Some(Self::Int(result)))
                            } else {
                                // Check size before computing to prevent DoS
                                check_pow_size(i64_bits(*base), u64::from(exp_u32), heap)?;
                                let bi = BigInt::from(*base).pow(exp_u32);
                                Ok(Some(LongInt::new(bi).into_value(heap)?))
                            }
                        } else {
                            // Exponent too large - result would be astronomically large
                            // Python handles this, but it would take forever. Use OverflowError
                            Err(SimpleException::new_msg(ExcType::OverflowError, "exponent too large").into())
                        }
                    } else {
                        // Negative LongInt exponent: return float
                        if let (Some(base_f64), Some(exp_f64)) = (Some(*base as f64), li.to_f64()) {
                            Ok(Some(Self::Float(base_f64.powf(exp_f64))))
                        } else {
                            Ok(Some(Self::Float(0.0)))
                        }
                    }
                } else {
                    Ok(None)
                }
            }
            (Self::Float(base), Self::Float(exp)) => {
                if *base == 0.0 && *exp < 0.0 {
                    Err(ExcType::zero_negative_power())
                } else if *base < 0.0 && exp.is_finite() && !float_is_integral(*exp) {
                    let (result_real, result_imag) = complex_pow_components(*base, 0.0, *exp, 0.0)?;
                    Ok(Some(allocate_complex_value(heap, result_real, result_imag)?))
                } else {
                    Ok(Some(Self::Float(base.powf(*exp))))
                }
            }
            (Self::Int(base), Self::Float(exp)) => {
                if *base == 0 && *exp < 0.0 {
                    Err(ExcType::zero_negative_power())
                } else if *base < 0 && exp.is_finite() && !float_is_integral(*exp) {
                    let (result_real, result_imag) = complex_pow_components(*base as f64, 0.0, *exp, 0.0)?;
                    Ok(Some(allocate_complex_value(heap, result_real, result_imag)?))
                } else {
                    Ok(Some(Self::Float((*base as f64).powf(*exp))))
                }
            }
            (Self::Float(base), Self::Int(exp)) => {
                if *base == 0.0 && *exp < 0 {
                    Err(ExcType::zero_negative_power())
                } else if let Ok(exp_i32) = i32::try_from(*exp) {
                    // Use powi if exp fits in i32
                    Ok(Some(Self::Float(base.powi(exp_i32))))
                } else {
                    // Fall back to powf for exponents outside i32 range
                    Ok(Some(Self::Float(base.powf(*exp as f64))))
                }
            }
            // Bool power operations (True=1, False=0)
            (Self::Bool(base), Self::Int(exp)) => {
                let base_int = i64::from(*base);
                if base_int == 0 && *exp < 0 {
                    Err(ExcType::zero_negative_power())
                } else if *exp >= 0 {
                    // Positive exponent: 1**n=1, 0**n=0 (for n>0), 0**0=1
                    if let Ok(exp_u32) = u32::try_from(*exp) {
                        match base_int.checked_pow(exp_u32) {
                            Some(result) => Ok(Some(Self::Int(result))),
                            None => Ok(Some(Self::Float((base_int as f64).powf(*exp as f64)))),
                        }
                    } else {
                        Ok(Some(Self::Float((base_int as f64).powf(*exp as f64))))
                    }
                } else {
                    // Negative exponent: return float (1**-n=1.0)
                    if let Ok(exp_i32) = i32::try_from(*exp) {
                        Ok(Some(Self::Float((base_int as f64).powi(exp_i32))))
                    } else {
                        Ok(Some(Self::Float((base_int as f64).powf(*exp as f64))))
                    }
                }
            }
            (Self::Int(base), Self::Bool(exp)) => {
                // n ** True = n, n ** False = 1
                if *exp {
                    Ok(Some(Self::Int(*base)))
                } else {
                    Ok(Some(Self::Int(1)))
                }
            }
            (Self::Bool(base), Self::Float(exp)) => {
                let base_float = f64::from(*base);
                if base_float == 0.0 && *exp < 0.0 {
                    Err(ExcType::zero_negative_power())
                } else {
                    Ok(Some(Self::Float(base_float.powf(*exp))))
                }
            }
            (Self::Float(base), Self::Bool(exp)) => {
                // base ** True = base, base ** False = 1.0
                if *exp {
                    Ok(Some(Self::Float(*base)))
                } else {
                    Ok(Some(Self::Float(1.0)))
                }
            }
            (Self::Bool(base), Self::Bool(exp)) => {
                // True ** True = 1, True ** False = 1, False ** True = 0, False ** False = 1
                let base_int = i64::from(*base);
                let exp_int = i64::from(*exp);
                if exp_int == 0 {
                    Ok(Some(Self::Int(1))) // anything ** 0 = 1
                } else {
                    Ok(Some(Self::Int(base_int))) // base ** 1 = base
                }
            }
            _ => Ok(None),
        }
    }

    fn py_getitem(&mut self, key: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Self> {
        match self {
            Self::Ref(id) => {
                // Need to take entry out to allow mutable heap access
                let id = *id;
                heap.with_entry_mut(id, |heap, data| data.py_getitem(key, heap, interns))
            }
            Self::InternString(string_id) => {
                // Check for slice first
                if let Self::Ref(key_id) = key
                    && let HeapData::Slice(slice_obj) = heap.get(*key_id)
                {
                    let s = interns.get_str(*string_id);
                    let char_count = s.chars().count();
                    let (start, stop, step) = slice_obj
                        .indices(char_count)
                        .map_err(|()| ExcType::value_error_slice_step_zero())?;
                    let result_str = get_str_slice(s, start, stop, step);
                    let heap_id = heap.allocate(HeapData::Str(Str::from(result_str)))?;
                    return Ok(Self::Ref(heap_id));
                }

                // Handle interned string indexing, accepting Int and Bool
                let index = match key {
                    Self::Int(i) => *i,
                    Self::Bool(b) => i64::from(*b),
                    _ => return Err(ExcType::type_error_indices(Type::Str, key.py_type(heap))),
                };

                let s = interns.get_str(*string_id);
                let c = get_char_at_index(s, index).ok_or_else(ExcType::str_index_error)?;
                Ok(allocate_char(c, heap)?)
            }
            Self::InternBytes(bytes_id) => {
                // Check for slice first
                if let Self::Ref(key_id) = key
                    && let HeapData::Slice(slice_obj) = heap.get(*key_id)
                {
                    let bytes = interns.get_bytes(*bytes_id);
                    let (start, stop, step) = slice_obj
                        .indices(bytes.len())
                        .map_err(|()| ExcType::value_error_slice_step_zero())?;
                    let result_bytes = get_bytes_slice(bytes, start, stop, step);
                    let heap_id = heap.allocate(HeapData::Bytes(crate::types::Bytes::new(result_bytes)))?;
                    return Ok(Self::Ref(heap_id));
                }

                // Handle interned bytes indexing - returns integer byte value
                let index = match key {
                    Self::Int(i) => *i,
                    Self::Bool(b) => i64::from(*b),
                    _ => return Err(ExcType::type_error_indices(Type::Bytes, key.py_type(heap))),
                };

                let bytes = interns.get_bytes(*bytes_id);
                let byte = get_byte_at_index(bytes, index).ok_or_else(ExcType::bytes_index_error)?;
                Ok(Self::Int(i64::from(byte)))
            }
            // `typing.Generic[T]` is a runtime typing shim used as a base class.
            // Lower it to `object` so class construction can continue.
            Self::Marker(marker) if marker.0 == StaticStrings::Generic => {
                let _ = key;
                Ok(Self::Builtin(Builtins::Type(Type::Object)))
            }
            // Typing special forms support subscription at runtime to build
            // `GenericAlias` values used by annotations and runtime helpers.
            Self::Marker(marker) if marker.0 == StaticStrings::UnionType || marker.py_type() == Type::SpecialForm => {
                let item = key.clone_with_heap(heap);
                if marker.0 == StaticStrings::Optional {
                    let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                    items.push(item);
                    items.push(Self::Builtin(Builtins::Type(Type::NoneType)));
                    let union_item = crate::types::allocate_tuple(items, heap)?;
                    crate::types::make_generic_alias(
                        Self::Marker(Marker(StaticStrings::UnionType)),
                        union_item,
                        heap,
                        interns,
                    )
                } else {
                    crate::types::make_generic_alias(Self::Marker(*marker), item, heap, interns)
                }
            }
            _ => Err(ExcType::type_error_not_sub(self.py_type(heap))),
        }
    }

    fn py_setitem(
        &mut self,
        key: Self,
        value: Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        match self {
            Self::Ref(id) => {
                let id = *id;
                heap.with_entry_mut(id, |heap, data| data.py_setitem(key, value, heap, interns))
            }
            _ => Err(ExcType::type_error(format!(
                "'{}' object does not support item assignment",
                self.py_type(heap)
            ))),
        }
    }
}

/// Compares sequence values using Python's lexicographic ordering semantics.
///
/// For ordering operators (`<`, `<=`, `>`, `>=`) Python compares element-by-element:
/// 1. If two elements are equal, continue.
/// 2. At the first non-equal element, use that element ordering.
/// 3. If all shared-prefix elements are equal, shorter sequence sorts first.
///
/// Returns `None` when the first non-equal element pair has no ordering.
fn compare_sequence_lexicographically(
    left: &[Value],
    right: &[Value],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Ordering> {
    for (left_item, right_item) in left.iter().zip(right.iter()) {
        if left_item.py_eq(right_item, heap, interns) {
            continue;
        }
        return left_item.py_cmp(right_item, heap, interns);
    }
    Some(left.len().cmp(&right.len()))
}

impl Value {
    /// Deletes an item by key from a container value.
    ///
    /// Delegates to `HeapData::py_delitem` for heap-allocated containers.
    pub fn py_delitem(&mut self, key: Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<()> {
        if let Self::Ref(id) = self {
            let id = *id;
            heap.with_entry_mut(id, |heap, data| data.py_delitem(key, heap, interns))
        } else {
            key.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "'{}' object does not support item deletion",
                self.py_type(heap)
            )))
        }
    }
    /// Returns a stable, unique internal identifier for this value.
    ///
    /// This is used by interpreter internals for identity comparisons and caches.
    /// To expose `id()` values to Python code and repr output, use `public_id()`
    /// so internal tagging/layout details are not leaked.
    ///
    /// For immediate values (Int, Float, Builtins), this computes a deterministic ID
    /// based on the value's hash, avoiding heap allocation. This means `id(5) == id(5)` will
    /// return True (unlike CPython for large integers outside the interning range).
    ///
    /// Singletons (None, True, False, etc.) return IDs from a dedicated tagged range.
    /// Interned strings/bytes use their interner index for stable identity.
    /// Heap-allocated values (Ref) reuse their `HeapId` inside the heap-tagged range.
    pub fn id(&self) -> usize {
        match self {
            // Singletons have fixed tagged IDs
            Self::Undefined => singleton_id(SingletonSlot::Undefined),
            Self::Ellipsis => singleton_id(SingletonSlot::Ellipsis),
            Self::None => singleton_id(SingletonSlot::None),
            Self::NotImplemented => singleton_id(SingletonSlot::None) + 1, // Unique ID for NotImplemented
            Self::Bool(b) => {
                if *b {
                    singleton_id(SingletonSlot::True)
                } else {
                    singleton_id(SingletonSlot::False)
                }
            }
            // Interned strings/bytes/bigints use their index directly - the index is the stable identifier
            Self::InternString(string_id) => INTERN_STR_ID_TAG | (string_id.index() & INTERN_STR_ID_MASK),
            Self::InternBytes(bytes_id) => INTERN_BYTES_ID_TAG | (bytes_id.index() & INTERN_BYTES_ID_MASK),
            Self::InternLongInt(long_int_id) => {
                INTERN_LONG_INT_ID_TAG | (long_int_id.index() & INTERN_LONG_INT_ID_MASK)
            }
            // Already heap-allocated (includes Range and Exception), return id within a dedicated tag range
            Self::Ref(id) => heap_tagged_id(*id),
            // Value-based IDs for immediate types (no heap allocation!)
            Self::Int(v) => int_value_id(*v),
            Self::Float(v) => float_value_id(*v),
            Self::Builtin(c) => builtin_value_id(*c),
            Self::ModuleFunction(mf) => module_function_value_id(*mf),
            Self::DefFunction(f_id) => function_value_id(*f_id),
            Self::ExtFunction(f_id) => ext_function_value_id(*f_id),
            Self::Proxy(proxy_id) => proxy_value_id(*proxy_id),
            // Markers get deterministic IDs based on discriminant
            Self::Marker(m) => marker_value_id(*m),
            // Properties get deterministic IDs based on discriminant
            Self::Property(p) => property_value_id(*p),
            // ExternalFutures get IDs based on their call_id
            Self::ExternalFuture(call_id) => external_future_value_id(*call_id),
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot get id of Dereferenced object"),
        }
    }

    /// Returns the Python-visible identity for this value.
    ///
    /// This applies a deterministic transform over the internal ID so output
    /// remains stable while avoiding direct exposure of internal ID tagging.
    pub fn public_id(&self) -> usize {
        public_id_from_internal_id(self.id())
    }

    /// Returns the Ref ID if this value is a reference, otherwise returns None.
    pub fn ref_id(&self) -> Option<HeapId> {
        match self {
            Self::Ref(id) => Some(*id),
            _ => None,
        }
    }

    /// Returns the module name if this value is a module, otherwise returns "<unknown>".
    ///
    /// Used for error messages in `from module import name` when the name doesn't exist.
    pub fn module_name(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> String {
        match self {
            Self::Ref(id) => match heap.get(*id) {
                HeapData::Module(module) => interns.get_str(module.name()).to_string(),
                _ => "<unknown>".to_string(),
            },
            _ => "<unknown>".to_string(),
        }
    }

    /// Equivalent of Python's `is` operator.
    ///
    /// Compares value identity by comparing their IDs.
    pub fn is(&self, other: &Self) -> bool {
        self.id() == other.id()
    }

    /// Computes the hash value for this value, used for dict keys.
    ///
    /// Returns Some(hash) for hashable types (immediate values and immutable heap types).
    /// Returns None for unhashable types (list, dict).
    ///
    /// For heap-allocated values (Ref variant), this computes the hash lazily
    /// on first use and caches it for subsequent calls.
    ///
    /// The `interns` parameter is needed for InternString/InternBytes to look up
    /// their actual content and hash it consistently with equivalent heap Str/Bytes.
    pub fn py_hash(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<u64> {
        // strings bytes bigints and heap allocated values have their own hashing logic
        match self {
            // Hash just the actual string or bytes content for consistency with heap Str/Bytes
            // hence we don't include the discriminant
            Self::InternString(string_id) => {
                return Some(cpython_hash_str_seed0(interns.get_str(*string_id)));
            }
            Self::InternBytes(bytes_id) => {
                return Some(cpython_hash_bytes_seed0(interns.get_bytes(*bytes_id)));
            }
            // Hash BigInt using CPython's modular algorithm for cross-type consistency
            Self::InternLongInt(long_int_id) => {
                let bi = interns.get_long_int(*long_int_id);
                return Some(LongInt::new(bi.clone()).hash());
            }
            // For heap-allocated values (includes Range and Exception), compute hash lazily and cache it
            Self::Ref(id) => return heap.get_or_compute_hash(*id, interns),
            _ => {}
        }

        // Int, Float, and Bool use CPython's modular hash algorithm to maintain
        // the cross-type invariant: if a == b then hash(a) == hash(b).
        // In particular hash(0) == hash(0.0) == hash(False) == 0 and
        // hash(1) == hash(1.0) == hash(True) == 1.
        match self {
            Self::Bool(b) => return Some(cpython_hash_int(i64::from(*b))),
            Self::Int(i) => return Some(cpython_hash_int(*i)),
            Self::Float(f) => return Some(cpython_hash_float(*f)),
            _ => {}
        }

        let mut hasher = DefaultHasher::new();
        // hash based on discriminant to avoid collisions with different types
        discriminant(self).hash(&mut hasher);
        match self {
            // Immediate values can be hashed directly
            Self::Undefined | Self::Ellipsis | Self::None | Self::NotImplemented => {}
            // Int/Float/Bool handled above
            Self::Bool(_) | Self::Int(_) | Self::Float(_) => unreachable!("covered above"),
            Self::Builtin(b) => b.hash(&mut hasher),
            Self::ModuleFunction(mf) => mf.hash(&mut hasher),
            // Hash functions based on function ID
            Self::DefFunction(f_id) => f_id.hash(&mut hasher),
            Self::ExtFunction(f_id) => f_id.hash(&mut hasher),
            Self::Proxy(proxy_id) => proxy_id.hash(&mut hasher),
            // Markers are hashable based on their discriminant (already included above)
            Self::Marker(m) => m.hash(&mut hasher),
            // Properties are hashable based on their OS function discriminant
            Self::Property(p) => p.hash(&mut hasher),
            // ExternalFutures are hashable based on their call ID
            Self::ExternalFuture(call_id) => call_id.raw().hash(&mut hasher),
            Self::InternString(_) | Self::InternBytes(_) | Self::InternLongInt(_) | Self::Ref(_) => {
                unreachable!("covered above")
            }
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot access Dereferenced object"),
        }
        Some(hasher.finish())
    }

    /// TODO this doesn't have many tests!!! also doesn't cover bytes
    /// Checks if `item` is contained in `self` (the container).
    ///
    /// Implements Python's `in` operator for various container types:
    /// - List/Tuple: linear search with equality
    /// - Dict: key lookup
    /// - Set/FrozenSet: element lookup
    /// - Str: substring search
    pub fn py_contains(
        &self,
        item: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<bool> {
        match self {
            Self::Ref(heap_id) => {
                // Use with_entry_mut to temporarily take ownership of the container.
                // This allows iterating over container elements while calling py_eq
                // (which needs &mut Heap for comparing nested heap values).
                heap.with_entry_mut(*heap_id, |heap, data| match data {
                    HeapData::List(el) => Ok(el.as_vec().iter().any(|i| item.py_eq(i, heap, interns))),
                    HeapData::Tuple(el) => Ok(el.as_vec().iter().any(|i| item.py_eq(i, heap, interns))),
                    HeapData::Deque(deque) => Ok(deque.iter().any(|i| item.py_eq(i, heap, interns))),
                    HeapData::Dict(dict) => dict.get(item, heap, interns).map(|m| m.is_some()),
                    HeapData::OrderedDict(od) => od.dict().get(item, heap, interns).map(|m| m.is_some()),
                    HeapData::Counter(counter) => counter.dict().get(item, heap, interns).map(|m| m.is_some()),
                    HeapData::DefaultDict(dd) => dd.dict().get(item, heap, interns).map(|m| m.is_some()),
                    HeapData::ChainMap(chain_map) => chain_map.flat().get(item, heap, interns).map(|m| m.is_some()),
                    HeapData::Set(set) => set.contains(item, heap, interns),
                    HeapData::FrozenSet(fset) => fset.contains(item, heap, interns),
                    HeapData::Str(s) => str_contains(s.as_str(), item, heap, interns),
                    HeapData::Bytes(b) => bytes_contains(b.as_slice(), item, heap, interns),
                    HeapData::Bytearray(b) => bytes_contains(b.as_slice(), item, heap, interns),
                    HeapData::Range(range) => {
                        // Range containment is O(1) - check bounds and step alignment
                        let n = match item {
                            Self::Int(i) => *i,
                            Self::Bool(b) => i64::from(*b),
                            Self::Float(f) => {
                                // Floats are contained if they equal an integer in the range
                                // e.g., 3.0 in range(5) is True, but 3.5 in range(5) is False
                                if f.fract() != 0.0 {
                                    return Ok(false);
                                }
                                // Check if float is within i64 range and convert safely
                                // f64 can represent integers up to 2^53 exactly
                                let int_val = f.trunc();
                                if int_val < i64::MIN as f64 || int_val > i64::MAX as f64 {
                                    return Ok(false);
                                }
                                // Safe conversion: we've verified it's a whole number in i64 range
                                #[expect(clippy::cast_possible_truncation)]
                                let n = int_val as i64;
                                n
                            }
                            _ => return Ok(false),
                        };
                        Ok(range.contains(n))
                    }
                    // Dict views - check membership based on view type
                    // Note: All view types need to use with_entry_mut to avoid borrow issues
                    // when accessing the source dict while also using mutable heap access
                    HeapData::DictKeys(dk) => {
                        let dict_id = dk.dict_id();
                        heap.with_entry_mut(dict_id, |heap, data| {
                            let HeapData::Dict(dict) = data else {
                                return Ok(false); // Source dict was deleted
                            };
                            dict.get(item, heap, interns).map(|m| m.is_some())
                        })
                    }
                    HeapData::DictValues(dv) => {
                        // Collect values first before doing comparisons
                        let values: Vec<Self> = {
                            let Some(dict) = dv.get_dict(heap) else {
                                return Ok(false); // Source dict was deleted
                            };
                            dict.iter().map(|(_, v)| v.clone_with_heap(heap)).collect()
                        };
                        // Check if item equals any value
                        let result = values.iter().any(|v| item.py_eq(v, heap, interns));
                        // Drop the copied values
                        for v in values {
                            v.drop_with_heap(heap);
                        }
                        Ok(result)
                    }
                    HeapData::DictItems(di) => {
                        // Item should be a (key, value) tuple
                        // Extract key and value from the item tuple first
                        let (key, value) = match item {
                            Self::Ref(id) => match heap.get(*id) {
                                crate::heap::HeapData::Tuple(t) if t.as_vec().len() == 2 => {
                                    (t.as_vec()[0].clone_with_heap(heap), t.as_vec()[1].clone_with_heap(heap))
                                }
                                _ => return Ok(false),
                            },
                            _ => return Ok(false),
                        };
                        // Now check if key exists and value matches using with_entry_mut
                        let dict_id = di.dict_id();
                        let result: Result<bool, RunError> = heap.with_entry_mut(dict_id, |heap, data| {
                            let HeapData::Dict(dict) = data else {
                                return Ok::<bool, RunError>(false); // Source dict was deleted
                            };
                            match dict.get(&key, heap, interns) {
                                Ok(Some(v)) => Ok(value.py_eq(v, heap, interns)),
                                _ => Ok(false),
                            }
                        });
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        result
                    }
                    HeapData::ClassObject(class_obj) => {
                        // Enum classes expose `__member_values__` for declaration-order membership.
                        // This supports `Color.RED in Color` without making arbitrary classes iterable.
                        let member_list_id = class_obj
                            .namespace()
                            .get_by_str("__member_values__", heap, interns)
                            .and_then(|value| match value {
                                Self::Ref(id) if matches!(heap.get(*id), HeapData::List(_)) => Some(*id),
                                _ => None,
                            });
                        let Some(list_id) = member_list_id else {
                            let type_name = class_obj.py_type(heap);
                            return Err(ExcType::type_error(format!(
                                "argument of type '{type_name}' is not iterable"
                            )));
                        };

                        let members: Vec<Self> = heap.with_entry_mut(list_id, |heap, data| {
                            if let HeapData::List(list) = data {
                                list.as_vec()
                                    .iter()
                                    .map(|member| member.clone_with_heap(heap))
                                    .collect()
                            } else {
                                Vec::new()
                            }
                        });
                        let result = members.iter().any(|member| item.py_eq(member, heap, interns));
                        for member in members {
                            member.drop_with_heap(heap);
                        }
                        Ok(result)
                    }
                    other => {
                        let type_name = other.py_type(heap);
                        Err(ExcType::type_error(format!(
                            "argument of type '{type_name}' is not iterable"
                        )))
                    }
                })
            }
            Self::InternString(string_id) => {
                let container_str = interns.get_str(*string_id);
                str_contains(container_str, item, heap, interns)
            }
            Self::InternBytes(bytes_id) => bytes_contains(interns.get_bytes(*bytes_id), item, heap, interns),
            _ => {
                let type_name = self.py_type(heap);
                Err(ExcType::type_error(format!(
                    "argument of type '{type_name}' is not iterable"
                )))
            }
        }
    }

    /// Gets an attribute from this value.
    ///
    /// Dispatches to `py_getattr` on the underlying types where appropriate.
    ///
    /// Returns `AttributeError` for other types or unknown attributes.
    pub fn py_getattr(
        &self,
        name_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<AttrCallResult> {
        match self {
            Self::Int(_) | Self::Bool(_) => {
                if interns.get_str(name_id) == "bit_length" {
                    let bound_id = heap.allocate(HeapData::BoundMethod(crate::types::BoundMethod::new(
                        Self::Builtin(Builtins::Function(BuiltinsFunctions::IntBitLength)),
                        self.copy_for_extend(),
                    )))?;
                    return Ok(AttrCallResult::Value(Self::Ref(bound_id)));
                }
            }
            Self::Ref(heap_id) => {
                if interns.get_str(name_id) == "bit_length" && matches!(heap.get(*heap_id), HeapData::LongInt(_)) {
                    let bound_id = heap.allocate(HeapData::BoundMethod(crate::types::BoundMethod::new(
                        Self::Builtin(Builtins::Function(BuiltinsFunctions::IntBitLength)),
                        Self::Ref(*heap_id).clone_with_heap(heap),
                    )))?;
                    return Ok(AttrCallResult::Value(Self::Ref(bound_id)));
                }
                if let Some(generator_attr) = Self::py_get_generator_attr(*heap_id, name_id, heap, interns)? {
                    return Ok(AttrCallResult::Value(generator_attr));
                }
                if name_id == StaticStrings::DunderTypeParams {
                    match heap.get(*heap_id) {
                        HeapData::Closure(func_id, _, _) | HeapData::FunctionDefaults(func_id, _) => {
                            let func = interns.get_function(*func_id);
                            if func.type_params.is_empty() {
                                return Err(ExcType::attribute_error(Type::Function, "__type_params__"));
                            }
                            let mut params: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                            for name_id in &func.type_params {
                                params.push(Self::InternString(*name_id));
                            }
                            let tuple_val = crate::types::allocate_tuple(params, heap)?;
                            return Ok(AttrCallResult::Value(tuple_val));
                        }
                        _ => {}
                    }
                }
                if matches!(
                    heap.get(*heap_id),
                    HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)
                ) {
                    let func_id = match heap.get(*heap_id) {
                        HeapData::Closure(func_id, _, _) | HeapData::FunctionDefaults(func_id, _) => *func_id,
                        _ => unreachable!(),
                    };
                    let func = interns.get_function(func_id);
                    let attr_name = interns.get_str(name_id);

                    if attr_name == "__code__" {
                        return Ok(AttrCallResult::Value(Self::Ref(*heap_id).clone_with_heap(heap)));
                    }
                    if attr_name == "co_name" {
                        return Ok(AttrCallResult::Value(Self::InternString(func.name.name_id)));
                    }
                    if attr_name == "co_argcount" {
                        let argcount = i64::try_from(func.signature.param_count())
                            .map_err(|_| RunError::internal("function parameter count exceeds i64"))?;
                        return Ok(AttrCallResult::Value(Self::Int(argcount)));
                    }
                    if attr_name == "co_varnames" {
                        let param_count = func.signature.param_count();
                        if param_count > usize::from(u16::MAX) {
                            return Err(RunError::internal(
                                "function parameter count exceeds u16 for co_varnames",
                            ));
                        }
                        let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                        for slot in 0..param_count {
                            let slot = u16::try_from(slot)
                                .map_err(|_| RunError::internal("function parameter index exceeds u16"))?;
                            if let Some(name_id) = func.code.local_name(slot) {
                                items.push(Self::InternString(name_id));
                            }
                        }
                        let tuple_val = crate::types::allocate_tuple(items, heap)?;
                        return Ok(AttrCallResult::Value(tuple_val));
                    }

                    if attr_name == "co_freevars" {
                        let free_var_count = func.free_var_enclosing_slots.len();
                        let free_vars_start = func.cell_var_count;
                        let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                        for idx in 0..free_var_count {
                            let slot = free_vars_start
                                .checked_add(idx)
                                .ok_or_else(|| RunError::internal("function free-var slot index overflow"))?;
                            let slot = u16::try_from(slot)
                                .map_err(|_| RunError::internal("function free-var slot index exceeds u16"))?;
                            if let Some(name_id) = func.code.local_name(slot) {
                                items.push(Self::InternString(name_id));
                            }
                        }
                        let tuple_val = crate::types::allocate_tuple(items, heap)?;
                        return Ok(AttrCallResult::Value(tuple_val));
                    }
                    if attr_name == "__closure__" {
                        let closure = match heap.get(*heap_id) {
                            HeapData::Closure(_, cells, _) if !cells.is_empty() => {
                                let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                                for cell_id in cells {
                                    items.push(Self::Ref(*cell_id).clone_with_heap(heap));
                                }
                                crate::types::allocate_tuple(items, heap)?
                            }
                            _ => Self::None,
                        };
                        return Ok(AttrCallResult::Value(closure));
                    }

                    if name_id == StaticStrings::DunderDictAttr {
                        let dict_id = heap.ensure_function_attr_dict(*heap_id)?;
                        let dict_value = Self::visible_function_attr_dict(dict_id, heap, interns)?;
                        return Ok(AttrCallResult::Value(dict_value));
                    }
                    if let Some(attr_value) = heap.function_attr_value_copy(*heap_id, interns.get_str(name_id), interns)
                    {
                        return Ok(AttrCallResult::Value(attr_value));
                    }

                    if name_id == StaticStrings::DunderName {
                        return Ok(AttrCallResult::Value(Self::InternString(func.name.name_id)));
                    }
                    if name_id == StaticStrings::DunderQualname {
                        let value = match &func.qualname {
                            crate::value::EitherStr::Interned(id) => Self::InternString(*id),
                            crate::value::EitherStr::Heap(s) => {
                                let id = heap.allocate(HeapData::Str(Str::from(s.as_str())))?;
                                Self::Ref(id)
                            }
                        };
                        return Ok(AttrCallResult::Value(value));
                    }
                    if name_id == StaticStrings::DunderModule {
                        return Ok(AttrCallResult::Value(Self::InternString(func.module_name)));
                    }
                    if name_id == StaticStrings::DunderDefaults || name_id == StaticStrings::DunderKwdefaults {
                        let defaults = match heap.get(*heap_id) {
                            HeapData::Closure(_, _, defaults) | HeapData::FunctionDefaults(_, defaults) => {
                                defaults.iter().map(Self::copy_for_extend).collect::<Vec<_>>()
                            }
                            _ => unreachable!(),
                        };
                        for value in &defaults {
                            if let Self::Ref(id) = value {
                                heap.inc_ref(*id);
                            }
                        }
                        if defaults.is_empty() {
                            return Ok(AttrCallResult::Value(Self::None));
                        }
                        let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                        for value in defaults {
                            items.push(value);
                        }
                        let tuple_val = crate::types::allocate_tuple(items, heap)?;
                        return Ok(AttrCallResult::Value(tuple_val));
                    }
                    if name_id == StaticStrings::DunderDescGet {
                        heap.inc_ref(*heap_id);
                        let getter_id = heap.allocate(HeapData::FunctionGet(crate::types::FunctionGet::new(
                            Self::Ref(*heap_id),
                        )))?;
                        return Ok(AttrCallResult::Value(Self::Ref(getter_id)));
                    }
                }
                if interns.get_str(name_id) == "cell_contents" && matches!(heap.get(*heap_id), HeapData::Cell(_)) {
                    let value = match heap.get(*heap_id) {
                        HeapData::Cell(value) => value.clone_with_heap(heap),
                        _ => unreachable!(),
                    };
                    if matches!(value, Self::Undefined) {
                        value.drop_with_heap(heap);
                        return Err(SimpleException::new_msg(ExcType::ValueError, "Cell is empty").into());
                    }
                    return Ok(AttrCallResult::Value(value));
                }
                // Class __dict__ should return a live mappingproxy, not a copy.
                if name_id == StaticStrings::DunderDictAttr && matches!(heap.get(*heap_id), HeapData::ClassObject(_)) {
                    heap.inc_ref(*heap_id);
                    let proxy_id = heap.allocate(HeapData::MappingProxy(crate::types::MappingProxy::new(*heap_id)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(proxy_id)));
                }
                // Class __subclasses__ should return a bound builtin method.
                if name_id == StaticStrings::DunderSubclasses && matches!(heap.get(*heap_id), HeapData::ClassObject(_))
                {
                    heap.inc_ref(*heap_id);
                    let meth_id =
                        heap.allocate(HeapData::ClassSubclasses(crate::types::ClassSubclasses::new(*heap_id)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(meth_id)));
                }
                // Default __class_getitem__ for PEP 695 classes without an override.
                if name_id == StaticStrings::DunderClassGetitem
                    && matches!(heap.get(*heap_id), HeapData::ClassObject(_))
                {
                    let has_custom = match heap.get(*heap_id) {
                        HeapData::ClassObject(cls) => cls.mro_has_attr("__class_getitem__", *heap_id, heap, interns),
                        _ => false,
                    };
                    if !has_custom {
                        heap.inc_ref(*heap_id);
                        let meth_id =
                            heap.allocate(HeapData::ClassGetItem(crate::types::ClassGetItem::new(*heap_id)))?;
                        return Ok(AttrCallResult::Value(Self::Ref(meth_id)));
                    }
                }
                if let HeapData::Partial(partial) = heap.get(*heap_id)
                    && partial.is_weakref_finalize()
                    && (name_id == StaticStrings::Detach || name_id == StaticStrings::Peek)
                {
                    heap.inc_ref(*heap_id);
                    let helper = if name_id == StaticStrings::Detach {
                        WeakrefFunctions::FinalizeDetach
                    } else {
                        WeakrefFunctions::FinalizePeek
                    };
                    let bound_id = heap.allocate(HeapData::BoundMethod(crate::types::BoundMethod::new(
                        Self::ModuleFunction(ModuleFunctions::Weakref(helper)),
                        Self::Ref(*heap_id),
                    )))?;
                    return Ok(AttrCallResult::Value(Self::Ref(bound_id)));
                }
                // Check if this is an Instance - need to handle descriptors specially.
                let is_instance = matches!(heap.get(*heap_id), HeapData::Instance(_));

                // Use with_entry_mut to get access to both data and heap without borrow conflicts.
                // This allows py_getattr to allocate (for computed attributes) while we hold the data.
                let opt_result = heap.with_entry_mut(*heap_id, |heap, data| data.py_getattr(name_id, heap, interns))?;
                if let Some(call_result) = opt_result {
                    if !is_instance {
                        // For class objects, bind @classmethod to the class on attribute access.
                        if matches!(heap.get(*heap_id), HeapData::ClassObject(_)) {
                            return match call_result {
                                AttrCallResult::Value(value) => {
                                    if let Self::Ref(ref_id) = &value
                                        && matches!(heap.get(*ref_id), HeapData::ClassMethod(_))
                                    {
                                        let class_id = *heap_id;
                                        let func = match heap.get(*ref_id) {
                                            HeapData::ClassMethod(cm) => cm.func().clone_with_heap(heap),
                                            _ => unreachable!("class method type changed during lookup"),
                                        };
                                        value.drop_with_heap(heap);
                                        heap.inc_ref(class_id);
                                        let bound_id = heap.allocate(HeapData::BoundMethod(
                                            crate::types::BoundMethod::new(func, Self::Ref(class_id)),
                                        ))?;
                                        Ok(AttrCallResult::Value(Self::Ref(bound_id)))
                                    } else {
                                        Ok(AttrCallResult::Value(value))
                                    }
                                }
                                other => Ok(other),
                            };
                        }
                        return Ok(call_result);
                    }

                    // For Instance results, check for descriptor types that need unwrapping.
                    match call_result {
                        AttrCallResult::Value(value) => {
                            // If the attribute came from the instance dict, do not bind.
                            let attr_name = interns.get_str(name_id);
                            let from_instance_dict =
                                heap.with_entry_mut(*heap_id, |heap, data| -> RunResult<bool> {
                                    if let HeapData::Instance(inst) = data {
                                        if let Some(dict) = inst.attrs(heap)
                                            && dict.get_by_str(attr_name, heap, interns).is_some()
                                        {
                                            return Ok(true);
                                        }
                                        Ok(inst.slot_value(attr_name, heap).is_some())
                                    } else {
                                        Ok(false)
                                    }
                                })?;

                            if from_instance_dict {
                                if let Some(prop_value) =
                                    Self::lookup_class_property(*heap_id, attr_name, heap, interns)
                                {
                                    value.drop_with_heap(heap);
                                    return Ok(Self::call_property_value(prop_value, *heap_id, heap));
                                }
                                return Ok(AttrCallResult::Value(value));
                            }

                            if let Self::Ref(ref_id) = &value {
                                match heap.get(*ref_id) {
                                    HeapData::UserProperty(_) => {
                                        return Ok(Self::call_property_value(value, *heap_id, heap));
                                    }
                                    HeapData::StaticMethod(sm) => {
                                        // StaticMethod: return inner function directly
                                        let func = sm.func().clone_with_heap(heap);
                                        value.drop_with_heap(heap);
                                        return Ok(AttrCallResult::Value(func));
                                    }
                                    HeapData::ClassMethod(cm) => {
                                        // ClassMethod on instance: return a bound method with cls
                                        let func = cm.func().clone_with_heap(heap);
                                        value.drop_with_heap(heap);
                                        let class_id = match heap.get(*heap_id) {
                                            HeapData::Instance(inst) => inst.class_id(),
                                            _ => unreachable!("type changed during lookup"),
                                        };
                                        heap.inc_ref(class_id);
                                        let bound_id = heap.allocate(HeapData::BoundMethod(
                                            crate::types::BoundMethod::new(func, Self::Ref(class_id)),
                                        ))?;
                                        return Ok(AttrCallResult::Value(Self::Ref(bound_id)));
                                    }
                                    _ => {}
                                }
                            }

                            let is_function = match &value {
                                Self::DefFunction(_) => true,
                                Self::Ref(id) => matches!(
                                    heap.get(*id),
                                    HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)
                                ),
                                _ => false,
                            };

                            if is_function {
                                heap.inc_ref(*heap_id);
                                let bound_id = heap.allocate(HeapData::BoundMethod(crate::types::BoundMethod::new(
                                    value,
                                    Self::Ref(*heap_id),
                                )))?;
                                return Ok(AttrCallResult::Value(Self::Ref(bound_id)));
                            }
                            return Ok(AttrCallResult::Value(value));
                        }
                        other => return Ok(other),
                    }
                }
            }
            Self::Builtin(Builtins::Type(t)) => {
                if *t == Type::RegexFlag {
                    if name_id == StaticStrings::DunderName {
                        let id = heap.allocate(HeapData::Str(Str::from("RegexFlag")))?;
                        return Ok(AttrCallResult::Value(Self::Ref(id)));
                    }
                    // RegexFlag attributes (ASCII, IGNORECASE, etc.)
                    let attr_name = interns.get_str(name_id);
                    let flag_bits = match attr_name {
                        "ASCII" | "A" => Some(256),
                        "IGNORECASE" | "I" => Some(2),
                        "MULTILINE" | "M" => Some(8),
                        "DOTALL" | "S" => Some(16),
                        "UNICODE" | "U" => Some(32),
                        "VERBOSE" | "X" => Some(64),
                        "LOCALE" | "L" => Some(4),
                        "DEBUG" => Some(128),
                        "NOFLAG" => Some(0),
                        _ => None,
                    };
                    if let Some(bits) = flag_bits {
                        let obj = StdlibObject::new_regex_flag(bits);
                        let id = heap.allocate(HeapData::StdlibObject(obj))?;
                        return Ok(AttrCallResult::Value(Self::Ref(id)));
                    }
                }
                if *t == Type::SreScanner && name_id == StaticStrings::DunderName {
                    let id = heap.allocate(HeapData::Str(Str::from("SRE_Scanner")))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::TextIOWrapper && name_id == StaticStrings::DunderName {
                    let id = heap.allocate(HeapData::Str(Str::from("TextIOWrapper")))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::Timezone && name_id == StaticStrings::DunderName {
                    let id = heap.allocate(HeapData::Str(Str::from("timezone")))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::Timezone && name_id == StaticStrings::DunderQualname {
                    let id = heap.allocate(HeapData::Str(Str::from("timezone")))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if name_id == StaticStrings::DunderName {
                    let qualified_name = t.to_string();
                    let short_name = qualified_name.rsplit('.').next().unwrap_or(&qualified_name);
                    let id = heap.allocate(HeapData::Str(Str::from(short_name)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::Date && interns.get_str(name_id) == "min" {
                    let id = heap.allocate(HeapData::Date(crate::types::datetime_types::Date::min()))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::Date && interns.get_str(name_id) == "max" {
                    let id = heap.allocate(HeapData::Date(crate::types::datetime_types::Date::max()))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::Timezone && interns.get_str(name_id) == "utc" {
                    let id = heap.allocate(HeapData::Timezone(crate::types::datetime_types::Timezone::utc()))?;
                    return Ok(AttrCallResult::Value(Self::Ref(id)));
                }
                if *t == Type::Fraction {
                    let attr_name = interns.get_str(name_id);
                    if matches!(attr_name, "from_float" | "from_decimal" | "from_number") {
                        return Ok(AttrCallResult::Value(Self::Builtin(Builtins::Type(Type::Fraction))));
                    }
                }
                if *t == Type::Decimal {
                    let attr_name = interns.get_str(name_id);
                    if attr_name == "from_float" {
                        return Ok(AttrCallResult::Value(Self::Builtin(Builtins::Type(Type::Decimal))));
                    }
                }
                if name_id == StaticStrings::DunderClassGetitem {
                    let class_id = heap.builtin_class_id(*t)?;
                    heap.inc_ref(class_id);
                    let meth_id = heap.allocate(HeapData::ClassGetItem(crate::types::ClassGetItem::new(class_id)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(meth_id)));
                }
                if name_id == StaticStrings::DunderModule {
                    let str_id = heap.allocate(HeapData::Str(Str::from("builtins")))?;
                    return Ok(AttrCallResult::Value(Self::Ref(str_id)));
                }
                if name_id == StaticStrings::DunderQualname {
                    let qualified_name = t.to_string();
                    let short_name = qualified_name.rsplit('.').next().unwrap_or(&qualified_name);
                    let str_id = heap.allocate(HeapData::Str(Str::from(short_name)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(str_id)));
                }
                if name_id == StaticStrings::DunderDictAttr {
                    let class_id = heap.builtin_class_id(*t)?;
                    heap.inc_ref(class_id);
                    let proxy_id = heap.allocate(HeapData::MappingProxy(crate::types::MappingProxy::new(class_id)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(proxy_id)));
                }
                if *t == Type::SafeUuid {
                    let safe_kind = match interns.get_str(name_id) {
                        "safe" => Some(SafeUuidKind::Safe),
                        "unsafe" => Some(SafeUuidKind::Unsafe),
                        "unknown" => Some(SafeUuidKind::Unknown),
                        _ => None,
                    };
                    if let Some(kind) = safe_kind {
                        let value = SafeUuid::new(kind).to_value(heap)?;
                        return Ok(AttrCallResult::Value(value));
                    }
                }
                // Check for type methods (e.g., str.lower, list.append)
                if let Some(static_str) = crate::intern::StaticStrings::from_string_id(name_id)
                    && let Some(result) = t.py_getattr(static_str)
                {
                    return Ok(result);
                }
                let class_id = heap.builtin_class_id(*t)?;
                heap.inc_ref(class_id);
                let class_value = Self::Ref(class_id);
                defer_drop!(class_value, heap);
                return class_value.py_getattr(name_id, heap, interns);
            }
            Self::Builtin(Builtins::ExcType(exc_type)) => {
                // Expose exception class attributes (`__name__`, etc.) the same
                // way as other builtin type objects.
                let exception_type_value = Self::Builtin(Builtins::Type(Type::Exception(*exc_type)));
                return exception_type_value.py_getattr(name_id, heap, interns);
            }
            Self::ModuleFunction(module_function) => {
                if matches!(module_function, ModuleFunctions::Itertools(ItertoolsFunctions::Chain))
                    && interns.get_str(name_id) == "from_iterable"
                {
                    return Ok(AttrCallResult::Value(Self::ModuleFunction(ModuleFunctions::Itertools(
                        ItertoolsFunctions::ChainFromIterable,
                    ))));
                }
            }
            Self::DefFunction(f_id) => {
                let func = interns.get_function(*f_id);
                if name_id == StaticStrings::DunderDictAttr {
                    let dict_id = heap.ensure_def_function_attr_dict(*f_id)?;
                    let dict_value = Self::visible_function_attr_dict(dict_id, heap, interns)?;
                    return Ok(AttrCallResult::Value(dict_value));
                }
                if let Some(attr_value) = heap.def_function_attr_value_copy(*f_id, interns.get_str(name_id), interns) {
                    return Ok(AttrCallResult::Value(attr_value));
                }
                let attr_name = interns.get_str(name_id);

                if attr_name == "__code__" {
                    return Ok(AttrCallResult::Value(self.copy_for_extend()));
                }
                if attr_name == "co_name" {
                    return Ok(AttrCallResult::Value(Self::InternString(func.name.name_id)));
                }
                if attr_name == "co_argcount" {
                    let argcount = i64::try_from(func.signature.param_count())
                        .map_err(|_| RunError::internal("function parameter count exceeds i64"))?;
                    return Ok(AttrCallResult::Value(Self::Int(argcount)));
                }
                if attr_name == "co_varnames" {
                    let param_count = func.signature.param_count();
                    if param_count > usize::from(u16::MAX) {
                        return Err(RunError::internal(
                            "function parameter count exceeds u16 for co_varnames",
                        ));
                    }
                    let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                    for slot in 0..param_count {
                        let slot = u16::try_from(slot)
                            .map_err(|_| RunError::internal("function parameter index exceeds u16"))?;
                        if let Some(name_id) = func.code.local_name(slot) {
                            items.push(Self::InternString(name_id));
                        }
                    }
                    let tuple_val = crate::types::allocate_tuple(items, heap)?;
                    return Ok(AttrCallResult::Value(tuple_val));
                }

                if attr_name == "co_freevars" {
                    let free_var_count = func.free_var_enclosing_slots.len();
                    let free_vars_start = func.cell_var_count;
                    let mut items: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                    for idx in 0..free_var_count {
                        let slot = free_vars_start
                            .checked_add(idx)
                            .ok_or_else(|| RunError::internal("function free-var slot index overflow"))?;
                        let slot = u16::try_from(slot)
                            .map_err(|_| RunError::internal("function free-var slot index exceeds u16"))?;
                        if let Some(name_id) = func.code.local_name(slot) {
                            items.push(Self::InternString(name_id));
                        }
                    }
                    let tuple_val = crate::types::allocate_tuple(items, heap)?;
                    return Ok(AttrCallResult::Value(tuple_val));
                }
                if attr_name == "__closure__" {
                    return Ok(AttrCallResult::Value(Self::None));
                }

                if name_id == StaticStrings::DunderName {
                    return Ok(AttrCallResult::Value(Self::InternString(func.name.name_id)));
                }
                if name_id == StaticStrings::DunderQualname {
                    let value = match &func.qualname {
                        crate::value::EitherStr::Interned(id) => Self::InternString(*id),
                        crate::value::EitherStr::Heap(s) => {
                            let id = heap.allocate(HeapData::Str(Str::from(s.as_str())))?;
                            Self::Ref(id)
                        }
                    };
                    return Ok(AttrCallResult::Value(value));
                }
                if name_id == StaticStrings::DunderModule {
                    return Ok(AttrCallResult::Value(Self::InternString(func.module_name)));
                }
                if name_id == StaticStrings::DunderDefaults || name_id == StaticStrings::DunderKwdefaults {
                    return Ok(AttrCallResult::Value(Self::None));
                }
                if name_id == StaticStrings::DunderDescGet {
                    let func_value = self.copy_for_extend();
                    let getter_id = heap.allocate(HeapData::FunctionGet(crate::types::FunctionGet::new(func_value)))?;
                    return Ok(AttrCallResult::Value(Self::Ref(getter_id)));
                }
                if name_id == StaticStrings::DunderTypeParams {
                    let func = interns.get_function(*f_id);
                    if func.type_params.is_empty() {
                        return Err(ExcType::attribute_error(Type::Function, "__type_params__"));
                    }
                    let mut params: smallvec::SmallVec<[Self; 3]> = smallvec::SmallVec::new();
                    for name_id in &func.type_params {
                        params.push(Self::InternString(*name_id));
                    }
                    let tuple_val = crate::types::allocate_tuple(params, heap)?;
                    return Ok(AttrCallResult::Value(tuple_val));
                }
            }
            _ => {}
        }
        let type_name = self.py_type(heap);
        Err(ExcType::attribute_error(type_name, interns.get_str(name_id)))
    }

    /// Handles generator-specific introspection attributes identified by `StringId`.
    ///
    /// This path is used by static bytecode attribute access where the name is
    /// already interned.
    fn py_get_generator_attr(
        generator_id: HeapId,
        name_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Self>> {
        Self::py_get_generator_attr_by_name(generator_id, interns.get_str(name_id), heap, interns)
    }

    /// Handles generator-specific introspection attributes by dynamic string name.
    ///
    /// This mirrors CPython's generator inspection surface used by parity tests:
    /// `gi_frame`, `gi_running`, `gi_code`, and `gi_suspended`.
    pub(crate) fn py_get_generator_attr_by_name(
        generator_id: HeapId,
        attr_name: &str,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Self>> {
        if !matches!(attr_name, "gi_frame" | "gi_running" | "gi_code" | "gi_suspended") {
            return Ok(None);
        }

        let HeapData::Generator(generator) = heap.get(generator_id) else {
            return Ok(None);
        };
        let func_id = generator.func_id;
        let state = generator.state;
        let saved_lineno = generator.saved_lineno;

        match attr_name {
            "gi_running" => Ok(Some(Self::Bool(matches!(state, GeneratorState::Running)))),
            "gi_suspended" => Ok(Some(Self::Bool(matches!(state, GeneratorState::Suspended)))),
            "gi_code" => Ok(Some(Self::DefFunction(func_id))),
            "gi_frame" => {
                if matches!(state, GeneratorState::Finished) {
                    return Ok(Some(Self::None));
                }
                let func = interns.get_function(func_id);
                let definition_lineno = func.name.position.start().line;
                let frame_lineno = match state {
                    GeneratorState::New => definition_lineno,
                    GeneratorState::Suspended | GeneratorState::Running => saved_lineno.unwrap_or(definition_lineno),
                    GeneratorState::Finished => definition_lineno,
                };
                let frame_value = Self::new_generator_frame_proxy(func_id, frame_lineno, heap, interns)?;
                Ok(Some(frame_value))
            }
            _ => Ok(None),
        }
    }
    /// Creates the lightweight frame proxy returned by `generator.gi_frame`.
    ///
    /// The proxy exposes only the fields used by parity checks: `f_code` and `f_lineno`.
    fn new_generator_frame_proxy(
        func_id: FunctionId,
        lineno: u16,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let f_code_key = Self::Ref(heap.allocate(HeapData::Str(Str::from("f_code")))?);
        let f_lineno_key = Self::Ref(heap.allocate(HeapData::Str(Str::from("f_lineno")))?);
        let attrs = Dict::from_pairs(
            vec![
                (f_code_key, Self::DefFunction(func_id)),
                (f_lineno_key, Self::Int(i64::from(lineno))),
            ],
            heap,
            interns,
        )?;
        let frame = Dataclass::new(
            "generator_frame".to_owned(),
            0,
            vec!["f_code".to_owned(), "f_lineno".to_owned()],
            attrs,
            AHashSet::new(),
            true,
        );
        let frame_id = heap.allocate(HeapData::Dataclass(frame))?;
        Ok(Self::Ref(frame_id))
    }

    /// Returns a function attribute dictionary view suitable for `function.__dict__`.
    ///
    /// The bytecode compiler initializes `__doc__` and `__annotations__` by assigning
    /// attributes on newly created function objects. CPython stores those values on
    /// dedicated function fields rather than exposing them through `function.__dict__`.
    /// To match expected behavior for decorator state and attribute inspection, this
    /// helper returns a dictionary copy that omits those internal metadata keys.
    fn visible_function_attr_dict(
        dict_id: HeapId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Self> {
        let copied_pairs = {
            let HeapData::Dict(dict) = heap.get(dict_id) else {
                return Err(RunError::internal("function attribute dictionary must be a dict"));
            };
            let mut pairs = Vec::with_capacity(dict.len());
            for (key, value) in dict {
                if Self::is_hidden_function_attr_key(key, heap) {
                    continue;
                }
                pairs.push((key.copy_for_extend(), value.copy_for_extend()));
            }
            pairs
        };

        for (key, value) in &copied_pairs {
            if let Self::Ref(id) = key {
                heap.inc_ref(*id);
            }
            if let Self::Ref(id) = value {
                heap.inc_ref(*id);
            }
        }

        let visible = Dict::from_pairs(copied_pairs, heap, interns)?;
        let visible_id = heap.allocate(HeapData::Dict(visible))?;
        Ok(Self::Ref(visible_id))
    }

    /// Returns true for compiler-managed function metadata keys hidden from `__dict__`.
    fn is_hidden_function_attr_key(key: &Self, heap: &Heap<impl ResourceTracker>) -> bool {
        match key {
            Self::InternString(id) => *id == StaticStrings::DunderDoc || *id == StaticStrings::DunderAnnotations,
            Self::Ref(id) => {
                if let HeapData::Str(s) = heap.get(*id) {
                    s.as_str() == "__doc__" || s.as_str() == "__annotations__"
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Looks up a class-level property descriptor for an instance attribute.
    ///
    /// Returns `Some(property_value)` if the class MRO contains a `UserProperty`
    /// for the given attribute name. The returned value is cloned with proper
    /// refcount handling. Returns `None` if no property is defined.
    fn lookup_class_property(
        instance_id: HeapId,
        attr_name: &str,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<Self> {
        let class_id = match heap.get(instance_id) {
            HeapData::Instance(inst) => inst.class_id(),
            _ => return None,
        };

        let HeapData::ClassObject(cls) = heap.get(class_id) else {
            return None;
        };

        if let Some((value, _)) = cls.mro_lookup_attr(attr_name, class_id, heap, interns) {
            if let Self::Ref(prop_id) = &value
                && matches!(heap.get(*prop_id), HeapData::UserProperty(_))
            {
                return Some(value);
            }
            value.drop_with_heap(heap);
        }

        None
    }

    /// Converts a property descriptor into a `PropertyCall` if it has a getter.
    ///
    /// Falls back to returning `None` when no getter is defined, mirroring the
    /// current property behavior in Ouros.
    fn call_property_value(value: Self, instance_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> AttrCallResult {
        if let Self::Ref(ref_id) = &value
            && let HeapData::UserProperty(up) = heap.get(*ref_id)
            && let Some(getter) = up.fget()
        {
            let getter = getter.clone_with_heap(heap);
            value.drop_with_heap(heap);
            heap.inc_ref(instance_id);
            let instance_ref = Self::Ref(instance_id);
            return AttrCallResult::PropertyCall(getter, instance_ref);
        }

        value.drop_with_heap(heap);
        AttrCallResult::Value(Self::None)
    }

    /// Sets an attribute on this value.
    ///
    /// Currently only Dataclass objects support attribute setting.
    /// Returns AttributeError for other types.
    ///
    /// Takes ownership of `value` and drops it on error.
    /// On success, drops the old attribute value if one existed.
    pub fn py_set_attr(
        &self,
        name_id: StringId,
        value: Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let attr_name = interns.get_str(name_id);

        if let Self::DefFunction(function_id) = self {
            if name_id == StaticStrings::DunderDictAttr {
                let Self::Ref(dict_id) = value else {
                    let type_name = value.py_type(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "__dict__ must be set to a dictionary, not a '{type_name}'"
                    )));
                };
                if !matches!(heap.get(dict_id), HeapData::Dict(_)) {
                    let type_name = heap.get(dict_id).py_type(heap);
                    Self::Ref(dict_id).drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "__dict__ must be set to a dictionary, not a '{type_name}'"
                    )));
                }
                heap.set_def_function_attr_dict(*function_id, dict_id);
                Self::Ref(dict_id).drop_with_heap(heap);
                Ok(())
            } else {
                let dict_id = heap.ensure_def_function_attr_dict(*function_id)?;
                let name_value = Self::InternString(name_id);
                let old = heap.with_entry_mut(dict_id, |heap, data| {
                    if let HeapData::Dict(dict) = data {
                        dict.set(name_value, value, heap, interns)
                    } else {
                        unreachable!("def function attribute dictionary must be a dict")
                    }
                })?;
                if let Some(old) = old {
                    old.drop_with_heap(heap);
                }
                Ok(())
            }
        } else if let Self::Ref(heap_id) = self {
            let heap_id = *heap_id;
            let is_dataclass = matches!(heap.get(heap_id), HeapData::Dataclass(_));
            let is_instance = matches!(heap.get(heap_id), HeapData::Instance(_));
            let is_class_object = matches!(heap.get(heap_id), HeapData::ClassObject(_));
            let is_text_wrapper = matches!(heap.get(heap_id), HeapData::TextWrapper(_));
            let is_defaultdict = matches!(heap.get(heap_id), HeapData::DefaultDict(_));
            let is_decimal_context = matches!(
                heap.get(heap_id),
                HeapData::StdlibObject(crate::types::StdlibObject::DecimalContext(_))
            );
            let is_function_object = matches!(
                heap.get(heap_id),
                HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)
            );

            if is_dataclass {
                let name_value = Self::InternString(name_id);
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::Dataclass(dc) = data {
                        match dc.set_attr(name_value, value, heap, interns) {
                            Ok(old_value) => {
                                if let Some(old) = old_value {
                                    old.drop_with_heap(heap);
                                }
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_instance {
                let name_value = Self::InternString(name_id);
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::Instance(inst) = data {
                        match inst.set_attr(name_value, value, heap, interns) {
                            Ok(old_value) => {
                                if let Some(old) = old_value {
                                    old.drop_with_heap(heap);
                                }
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_class_object {
                let name_value = Self::InternString(name_id);
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::ClassObject(cls) = data {
                        match cls.set_attr(name_value, value, heap, interns) {
                            Ok(old_value) => {
                                if let Some(old) = old_value {
                                    old.drop_with_heap(heap);
                                }
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_text_wrapper {
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::TextWrapper(wrapper) = data {
                        match StaticStrings::from_string_id(name_id) {
                            Some(StaticStrings::TwWidth) => {
                                let width = match value.as_int(heap) {
                                    Ok(parsed) => parsed,
                                    Err(err) => {
                                        value.drop_with_heap(heap);
                                        return Err(err);
                                    }
                                };
                                if width < 0 {
                                    value.drop_with_heap(heap);
                                    return Err(
                                        SimpleException::new_msg(ExcType::ValueError, "width must be >= 0").into()
                                    );
                                }
                                wrapper.width = usize::try_from(width).expect("validated non-negative width");
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwInitialIndent) => {
                                wrapper.initial_indent = value.py_str(heap, interns).into_owned();
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwSubsequentIndent) => {
                                wrapper.subsequent_indent = value.py_str(heap, interns).into_owned();
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwPlaceholder) => {
                                wrapper.placeholder = value.py_str(heap, interns).into_owned();
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwMaxLines) => {
                                if matches!(value, Self::None) {
                                    wrapper.max_lines = None;
                                    value.drop_with_heap(heap);
                                    Ok(())
                                } else {
                                    let max_lines = match value.as_int(heap) {
                                        Ok(parsed) => parsed,
                                        Err(err) => {
                                            value.drop_with_heap(heap);
                                            return Err(err);
                                        }
                                    };
                                    if max_lines < 0 {
                                        value.drop_with_heap(heap);
                                        return Err(SimpleException::new_msg(
                                            ExcType::ValueError,
                                            "max_lines must be >= 0 or None",
                                        )
                                        .into());
                                    }
                                    wrapper.max_lines =
                                        Some(usize::try_from(max_lines).expect("validated non-negative max_lines"));
                                    value.drop_with_heap(heap);
                                    Ok(())
                                }
                            }
                            Some(StaticStrings::TwBreakLongWords) => {
                                wrapper.break_long_words = value.py_bool(heap, interns);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwExpandTabs) => {
                                wrapper.expand_tabs = value.py_bool(heap, interns);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwReplaceWhitespace) => {
                                wrapper.replace_whitespace = value.py_bool(heap, interns);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwFixSentenceEndings) => {
                                wrapper.fix_sentence_endings = value.py_bool(heap, interns);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwDropWhitespace) => {
                                wrapper.drop_whitespace = value.py_bool(heap, interns);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwBreakOnHyphens) => {
                                wrapper.break_on_hyphens = value.py_bool(heap, interns);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Some(StaticStrings::TwTabsize) => {
                                let tabsize = match value.as_int(heap) {
                                    Ok(parsed) => parsed,
                                    Err(err) => {
                                        value.drop_with_heap(heap);
                                        return Err(err);
                                    }
                                };
                                if tabsize < 0 {
                                    value.drop_with_heap(heap);
                                    return Err(
                                        SimpleException::new_msg(ExcType::ValueError, "tabsize must be >= 0").into()
                                    );
                                }
                                wrapper.tabsize = usize::try_from(tabsize).expect("validated non-negative tabsize");
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            _ => {
                                value.drop_with_heap(heap);
                                Err(ExcType::attribute_error_no_setattr(Type::Object, attr_name))
                            }
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_decimal_context {
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::StdlibObject(obj) = data {
                        if let Some(result) = obj.set_decimal_context_attr(attr_name, value, heap, interns) {
                            result
                        } else {
                            unreachable!("decimal context discriminant changed during borrow")
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_function_object {
                if name_id == StaticStrings::DunderDictAttr {
                    let Self::Ref(dict_id) = value else {
                        let type_name = value.py_type(heap);
                        value.drop_with_heap(heap);
                        return Err(ExcType::type_error(format!(
                            "__dict__ must be set to a dictionary, not a '{type_name}'"
                        )));
                    };
                    if !matches!(heap.get(dict_id), HeapData::Dict(_)) {
                        let type_name = heap.get(dict_id).py_type(heap);
                        Self::Ref(dict_id).drop_with_heap(heap);
                        return Err(ExcType::type_error(format!(
                            "__dict__ must be set to a dictionary, not a '{type_name}'"
                        )));
                    }
                    heap.set_function_attr_dict(heap_id, dict_id);
                    Self::Ref(dict_id).drop_with_heap(heap);
                    Ok(())
                } else {
                    let dict_id = heap.ensure_function_attr_dict(heap_id)?;
                    let name_value = Self::InternString(name_id);
                    let old = heap.with_entry_mut(dict_id, |heap, data| {
                        if let HeapData::Dict(dict) = data {
                            dict.set(name_value, value, heap, interns)
                        } else {
                            unreachable!("function attribute dictionary must be a dict")
                        }
                    })?;
                    if let Some(old) = old {
                        old.drop_with_heap(heap);
                    }
                    Ok(())
                }
            } else if is_defaultdict && attr_name == "default_factory" {
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::DefaultDict(default_dict) = data {
                        let new_factory = if matches!(value, Self::None) { None } else { Some(value) };
                        if let Some(old) = default_dict.replace_default_factory(new_factory) {
                            old.drop_with_heap(heap);
                        }
                        Ok(())
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else {
                let type_name = heap.get(heap_id).py_type(heap);
                value.drop_with_heap(heap);
                Err(ExcType::attribute_error_no_setattr(type_name, attr_name))
            }
        } else {
            let type_name = self.py_type(heap);
            value.drop_with_heap(heap);
            Err(ExcType::attribute_error_no_setattr(type_name, attr_name))
        }
    }

    /// Deletes an attribute from an object.
    ///
    /// Supports deleting attributes from instances and function objects.
    ///
    /// Raises `AttributeError` if the attribute doesn't exist or the object
    /// doesn't support attribute deletion.
    pub fn py_del_attr(
        &self,
        name_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        let attr_name = interns.get_str(name_id);

        if let Self::DefFunction(function_id) = self {
            if name_id == StaticStrings::DunderDictAttr {
                return Err(ExcType::type_error("cannot delete __dict__"));
            }
            let Some(dict_id) = heap.def_function_attr_dict_id(*function_id) else {
                return Err(ExcType::attribute_error(Type::Function, attr_name));
            };
            let name_value = Self::InternString(name_id);
            let removed = heap.with_entry_mut(dict_id, |heap, data| {
                if let HeapData::Dict(dict) = data {
                    dict.pop(&name_value, heap, interns)
                } else {
                    unreachable!("def function attribute dictionary must be a dict")
                }
            })?;
            match removed {
                Some((key, value)) => {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    Ok(())
                }
                None => Err(ExcType::attribute_error(Type::Function, attr_name)),
            }
        } else if let Self::Ref(heap_id) = self {
            let heap_id = *heap_id;
            let is_instance = matches!(heap.get(heap_id), HeapData::Instance(_));
            let is_function_object = matches!(
                heap.get(heap_id),
                HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)
            );

            if is_instance {
                let name_value = Self::InternString(name_id);
                heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::Instance(inst) = data {
                        match inst.del_attr(&name_value, heap, interns) {
                            Ok(Some((key, value))) => {
                                key.drop_with_heap(heap);
                                value.drop_with_heap(heap);
                                Ok(())
                            }
                            Ok(None) => Err(ExcType::attribute_error("instance", attr_name)),
                            Err(e) => Err(e),
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_function_object {
                if name_id == StaticStrings::DunderDictAttr {
                    return Err(ExcType::type_error("cannot delete __dict__"));
                }
                let Some(dict_id) = heap.function_attr_dict_id(heap_id) else {
                    return Err(ExcType::attribute_error(Type::Function, attr_name));
                };
                let name_value = Self::InternString(name_id);
                let removed = heap.with_entry_mut(dict_id, |heap, data| {
                    if let HeapData::Dict(dict) = data {
                        dict.pop(&name_value, heap, interns)
                    } else {
                        unreachable!("function attribute dictionary must be a dict")
                    }
                })?;
                match removed {
                    Some((key, value)) => {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        Ok(())
                    }
                    None => Err(ExcType::attribute_error(Type::Function, attr_name)),
                }
            } else {
                let type_name = heap.get(heap_id).py_type(heap);
                Err(ExcType::attribute_error_no_setattr(type_name, attr_name))
            }
        } else {
            let type_name = self.py_type(heap);
            Err(ExcType::attribute_error_no_setattr(type_name, attr_name))
        }
    }

    /// Extracts an integer value from the Value.
    ///
    /// Accepts `Int`, `re.RegexFlag` values, and `LongInt` (if it fits in i64).
    /// Returns a `TypeError` for other types and an `OverflowError` if the
    /// `LongInt` value is too large.
    ///
    /// Note: The LongInt-to-i64 conversion path is defensive code. In normal execution,
    /// heap-allocated `LongInt` values always exceed i64 range because `LongInt::into_value()`
    /// automatically demotes i64-fitting values to `Value::Int`. However, this path could be
    /// reached via deserialization of crafted snapshot data.
    pub fn as_int(&self, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
        match self {
            Self::Int(i) => Ok(*i),
            Self::Ref(heap_id) => {
                if let HeapData::LongInt(li) = heap.get(*heap_id) {
                    li.to_i64().ok_or_else(ExcType::overflow_shift_count)
                } else if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*heap_id) {
                    Ok(*bits)
                } else {
                    let msg = format!("'{}' object cannot be interpreted as an integer", self.py_type(heap));
                    Err(SimpleException::new_msg(ExcType::TypeError, msg).into())
                }
            }
            _ => {
                let msg = format!("'{}' object cannot be interpreted as an integer", self.py_type(heap));
                Err(SimpleException::new_msg(ExcType::TypeError, msg).into())
            }
        }
    }

    /// Extracts an index value for sequence operations.
    ///
    /// Accepts `Int`, `Bool` (True=1, False=0), `re.RegexFlag` values, and
    /// `LongInt` (if it fits in i64). Returns a `TypeError` for other types
    /// with the container type name included. Returns an `IndexError` if the
    /// `LongInt` value is too large to use as an index.
    ///
    /// Note: The LongInt-to-i64 conversion path is defensive code. In normal execution,
    /// heap-allocated `LongInt` values always exceed i64 range because `LongInt::into_value()`
    /// automatically demotes i64-fitting values to `Value::Int`. However, this path could be
    /// reached via deserialization of crafted snapshot data.
    pub fn as_index(&self, heap: &Heap<impl ResourceTracker>, container_type: Type) -> RunResult<i64> {
        match self {
            Self::Int(i) => Ok(*i),
            Self::Bool(b) => Ok(i64::from(*b)),
            Self::Ref(heap_id) => {
                if let HeapData::LongInt(li) = heap.get(*heap_id) {
                    li.to_i64().ok_or_else(ExcType::index_error_int_too_large)
                } else if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*heap_id) {
                    Ok(*bits)
                } else {
                    Err(ExcType::type_error_indices(container_type, self.py_type(heap)))
                }
            }
            _ => Err(ExcType::type_error_indices(container_type, self.py_type(heap))),
        }
    }

    /// Performs a binary bitwise operation on two values.
    ///
    /// Python only supports bitwise operations on integers (and bools, which coerce to int).
    /// Returns a `TypeError` if either operand is not an integer, bool, or LongInt.
    ///
    /// For shift operations:
    /// - Negative shift counts raise `ValueError`
    /// - Left shifts may produce LongInt results for large shifts
    /// - Right shifts with large counts return 0 (or -1 for negative numbers)
    pub fn py_bitwise(
        &self,
        other: &Self,
        op: BitwiseOp,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Self, RunError> {
        // Capture types for error messages
        let lhs_type = self.py_type(heap);
        let rhs_type = other.py_type(heap);

        // Check for set operations first (| & ^)
        if matches!(op, BitwiseOp::Or | BitwiseOp::And | BitwiseOp::Xor) {
            if let Some(result) = py_dict_bitwise(self, other, op, heap, interns)? {
                return Ok(result);
            }
            if let Some(result) = py_set_bitwise(self, other, op, heap, interns)? {
                return Ok(result);
            }
            if let Some(result) = py_counter_bitwise(self, other, op, heap, interns)? {
                return Ok(result);
            }
            if let Some(result) = py_chainmap_bitwise(self, other, op, heap, interns)? {
                return Ok(result);
            }
            let lhs_flag = extract_regex_flag_bits(self, heap);
            let rhs_flag = extract_regex_flag_bits(other, heap);
            let has_regex_flag_object = is_regex_flag_object(self, heap) || is_regex_flag_object(other, heap);
            if has_regex_flag_object && let (Some(lhs_bits), Some(rhs_bits)) = (lhs_flag, rhs_flag) {
                let bits = match op {
                    BitwiseOp::And => lhs_bits & rhs_bits,
                    BitwiseOp::Or => lhs_bits | rhs_bits,
                    BitwiseOp::Xor => lhs_bits ^ rhs_bits,
                    BitwiseOp::LShift | BitwiseOp::RShift => unreachable!("filtered above"),
                };
                let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(bits)))?;
                return Ok(Self::Ref(id));
            }
            if let Some(result) = py_type_union_bitwise(self, other, op, heap, interns)? {
                return Ok(result);
            }
        }

        // Extract BigInt from all numeric types
        let lhs_bigint = extract_bigint(self, heap);
        let rhs_bigint = extract_bigint(other, heap);

        if let (Some(l), Some(r)) = (lhs_bigint, rhs_bigint) {
            let result = match op {
                BitwiseOp::And => l & r,
                BitwiseOp::Or => l | r,
                BitwiseOp::Xor => l ^ r,
                BitwiseOp::LShift => {
                    // Get shift amount as i64 for validation
                    let shift_amount = r.to_i64();
                    if let Some(shift) = shift_amount {
                        if shift < 0 {
                            return Err(ExcType::value_error_negative_shift_count());
                        }
                        // Python allows arbitrarily large left shifts - use BigInt's shift
                        // Safety: shift >= 0 is guaranteed by the check above
                        #[expect(clippy::cast_sign_loss)]
                        let shift_u64 = shift as u64;
                        // Check size before computing to prevent DoS
                        // Skip check if value is 0 - result is always 0 regardless of shift
                        let value_bits = l.bits();
                        if value_bits > 0
                            && let Some(estimated) = LongInt::estimate_lshift_bytes(value_bits, shift_u64)
                            && estimated > LARGE_RESULT_THRESHOLD
                        {
                            heap.tracker().check_large_result(estimated)?;
                        }
                        l << shift_u64
                    } else if r.sign() == num_bigint::Sign::Minus {
                        return Err(ExcType::value_error_negative_shift_count());
                    } else {
                        // Shift amount too large to fit in i64 - this would be astronomically large
                        return Err(ExcType::overflow_shift_count());
                    }
                }
                BitwiseOp::RShift => {
                    // Get shift amount as i64 for validation
                    let shift_amount = r.to_i64();
                    if let Some(shift) = shift_amount {
                        if shift < 0 {
                            return Err(ExcType::value_error_negative_shift_count());
                        }
                        // Safety: shift >= 0 is guaranteed by the check above
                        #[expect(clippy::cast_sign_loss)]
                        let shift_u64 = shift as u64;
                        l >> shift_u64
                    } else if r.sign() == num_bigint::Sign::Minus {
                        return Err(ExcType::value_error_negative_shift_count());
                    } else {
                        // Shift amount too large - result is 0 or -1 depending on sign
                        if l.sign() == num_bigint::Sign::Minus {
                            BigInt::from(-1)
                        } else {
                            BigInt::from(0)
                        }
                    }
                }
            };
            // Convert result back to Value, demoting to i64 if it fits
            LongInt::new(result).into_value(heap).map_err(Into::into)
        } else {
            Err(ExcType::binary_type_error(op.as_str(), lhs_type, rhs_type))
        }
    }

    /// Clones an value with proper heap reference counting.
    ///
    /// For immediate values (Int, Bool, None, etc.), this performs a simple copy.
    /// For heap-allocated values (Ref variant), this increments the reference count
    /// and returns a new reference to the same heap value.
    ///
    /// # Important
    /// This method MUST be used instead of the derived `Clone` implementation to ensure
    /// proper reference counting. Using `.clone()` directly will bypass reference counting
    /// and cause memory leaks or double-frees.
    #[must_use]
    pub fn clone_with_heap(&self, heap: &Heap<impl ResourceTracker>) -> Self {
        match self {
            Self::Ref(id) => {
                heap.inc_ref(*id);
                Self::Ref(*id)
            }
            // Immediate values can be copied without heap interaction
            other => other.clone_immediate(),
        }
    }

    /// Drops an value, decrementing its heap reference count if applicable.
    ///
    /// For immediate values, this is a no-op. For heap-allocated values (Ref variant),
    /// this decrements the reference count and frees the value (and any children) when
    /// the count reaches zero. For Closure variants, this decrements ref counts on all
    /// captured cells.
    ///
    /// # Important
    /// This method MUST be called before overwriting a namespace slot or discarding
    /// a value to prevent memory leaks.
    #[cfg(not(feature = "ref-count-panic"))]
    #[inline]
    pub fn drop_with_heap(self, heap: &mut Heap<impl ResourceTracker>) {
        if let Self::Ref(id) = self {
            heap.dec_ref(id);
        }
    }
    /// With `ref-count-panic` enabled, `Ref` variants are replaced with `Dereferenced` and
    /// the original is forgotten to prevent the Drop impl from panicking. Non-Ref variants
    /// are left unchanged since they don't trigger the Drop panic.
    #[cfg(feature = "ref-count-panic")]
    pub fn drop_with_heap(mut self, heap: &mut Heap<impl ResourceTracker>) {
        let old = std::mem::replace(&mut self, Self::Dereferenced);
        if let Self::Ref(id) = &old {
            heap.dec_ref(*id);
            std::mem::forget(old);
        }
    }

    /// Internal helper for copying immediate values without heap interaction.
    ///
    /// This method should only be called by `clone_with_heap()` for immediate values.
    /// Attempting to clone a Ref variant will panic.
    pub fn clone_immediate(&self) -> Self {
        match self {
            Self::Ref(_) => panic!("Ref clones must go through clone_with_heap to maintain refcounts"),
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot clone Dereferenced object"),
            _ => self.copy_for_extend(),
        }
    }

    /// Creates a shallow copy of this Value without incrementing reference counts.
    ///
    /// IMPORTANT: For Ref variants, this copies the ValueId but does NOT increment
    /// the reference count. The caller MUST call `heap.inc_ref()` separately for any
    /// Ref variants to maintain correct reference counting.
    ///
    /// For Closure variants, this copies without incrementing cell ref counts.
    /// The caller MUST increment ref counts on the captured cells separately.
    ///
    /// This is useful when you need to copy Objects from a borrowed heap context
    /// and will increment refcounts in a separate step.
    pub(crate) fn copy_for_extend(&self) -> Self {
        match self {
            Self::Undefined => Self::Undefined,
            Self::Ellipsis => Self::Ellipsis,
            Self::None => Self::None,
            Self::NotImplemented => Self::NotImplemented,
            Self::Bool(b) => Self::Bool(*b),
            Self::Int(v) => Self::Int(*v),
            Self::Float(v) => Self::Float(*v),
            Self::Builtin(b) => Self::Builtin(*b),
            Self::ModuleFunction(mf) => Self::ModuleFunction(*mf),
            Self::DefFunction(f) => Self::DefFunction(*f),
            Self::ExtFunction(f) => Self::ExtFunction(*f),
            Self::Proxy(proxy_id) => Self::Proxy(*proxy_id),
            Self::InternString(s) => Self::InternString(*s),
            Self::InternBytes(b) => Self::InternBytes(*b),
            Self::InternLongInt(bi) => Self::InternLongInt(*bi),
            Self::Marker(m) => Self::Marker(*m),
            Self::Property(p) => Self::Property(*p),
            Self::ExternalFuture(call_id) => Self::ExternalFuture(*call_id),
            Self::Ref(id) => Self::Ref(*id), // Caller must increment refcount!
            #[cfg(feature = "ref-count-panic")]
            Self::Dereferenced => panic!("Cannot copy Dereferenced object"),
        }
    }

    /// Mark as Dereferenced to prevent Drop panic
    ///
    /// This should be called from `py_dec_ref_ids` methods only
    #[cfg(feature = "ref-count-panic")]
    pub fn dec_ref_forget(&mut self) {
        let old = std::mem::replace(self, Self::Dereferenced);
        std::mem::forget(old);
    }

    /// Converts the value into a keyword string representation if possible.
    ///
    /// Returns `Some(KeywordStr)` for `InternString` values or heap `str`
    /// objects, otherwise returns `None`.
    pub fn as_either_str(&self, heap: &Heap<impl ResourceTracker>) -> Option<EitherStr> {
        match self {
            Self::InternString(id) => Some(EitherStr::Interned(*id)),
            Self::Ref(heap_id) => match heap.get(*heap_id) {
                HeapData::Str(s) => Some(EitherStr::Heap(s.as_str().to_owned())),
                _ => None,
            },
            _ => None,
        }
    }

    /// check if the value is a string.
    pub fn is_str(&self, heap: &Heap<impl ResourceTracker>) -> bool {
        match self {
            Self::InternString(_) => true,
            Self::Ref(heap_id) => matches!(heap.get(*heap_id), HeapData::Str(_)),
            _ => false,
        }
    }
}

/// Returns a string representation of a float matching CPython's `repr()` behavior.
///
/// Uses the `ryu` crate which produces the shortest decimal representation
/// that round-trips through `f64` parsing, matching CPython's behavior.
/// Key behaviors:
/// - Special values: `inf`, `-inf`, `nan` (lowercase)
/// - Always includes decimal point or 'e' notation
/// - Uses scientific notation when appropriate
fn float_repr(f: f64) -> String {
    // Handle special values first
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }

    // Use ryu for the shortest round-tripping representation
    let mut buffer = ryu::Buffer::new();
    let s = buffer.format(f);

    // ryu produces "1e20" but CPython uses "1e+20" for positive exponents
    // Fix the exponent format
    fix_ryu_exponent(s)
}

/// Fixes ryu's exponent format to match CPython.
///
/// ryu produces "1e20" but CPython uses "1e+20" for positive exponents.
/// Also ensures ".0" suffix for numbers like "3" -> "3.0".
fn fix_ryu_exponent(s: &str) -> String {
    // Check if this has an exponent
    if let Some(e_pos) = s.find('e') {
        let (mantissa, exp_part) = s.split_at(e_pos);
        let exp = &exp_part[1..]; // Skip 'e'

        // Check if exponent is positive (no sign in ryu output means positive)
        if !exp.starts_with('-') {
            return format!("{mantissa}e+{exp}");
        }
        // Negative exponent already has '-' sign
        return s.to_string();
    }

    // No exponent - ensure it has a decimal point
    if !s.contains('.') {
        return format!("{s}.0");
    }

    s.to_string()
}

/// Interned or heap-owned string identifier.
///
/// Used when a string value can come from either the intern table (for known
/// static strings and keywords) or from a heap-allocated Python string object.
#[derive(Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum EitherStr {
    /// Interned string identifier (cheap comparisons and no allocation).
    Interned(StringId),
    /// Heap-owned string extracted from a `str` object.
    Heap(String),
}

impl From<StringId> for EitherStr {
    fn from(id: StringId) -> Self {
        Self::Interned(id)
    }
}

impl From<StaticStrings> for EitherStr {
    fn from(s: StaticStrings) -> Self {
        Self::Interned(s.into())
    }
}

/// Convert String to EitherStr: use Interned for known static strings,
/// otherwise use Heap for user-defined field names.
impl From<String> for EitherStr {
    fn from(s: String) -> Self {
        match StaticStrings::from_str(&s) {
            Ok(s) => s.into(),
            Err(_) => Self::Heap(s),
        }
    }
}

impl EitherStr {
    /// Returns the keyword as a str slice for error messages or comparisons.
    pub fn as_str<'a>(&'a self, interns: &'a Interns) -> &'a str {
        match self {
            Self::Interned(id) => interns.get_str(*id),
            Self::Heap(s) => s.as_str(),
        }
    }

    /// Checks whether this keyword matches the given interned identifier.
    pub fn matches(&self, target: StringId, interns: &Interns) -> bool {
        match self {
            Self::Interned(id) => *id == target,
            Self::Heap(s) => s == interns.get_str(target),
        }
    }

    /// Returns the `StringId` if this is an interned attribute.
    #[inline]
    pub fn string_id(&self) -> Option<StringId> {
        match self {
            Self::Interned(id) => Some(*id),
            Self::Heap(_) => None,
        }
    }

    /// Returns the `StaticStrings` if this is an interned attribute from `StaticStrings`s.
    #[inline]
    pub fn static_string(&self) -> Option<StaticStrings> {
        match self {
            Self::Interned(id) => StaticStrings::from_string_id(*id),
            Self::Heap(_) => None,
        }
    }

    pub fn py_estimate_size(&self) -> usize {
        match self {
            Self::Interned(_) => 0,
            Self::Heap(s) => s.capacity(),
        }
    }
}

/// Bitwise operation type for `py_bitwise`.
#[derive(Debug, Clone, Copy)]
pub enum BitwiseOp {
    And,
    Or,
    Xor,
    LShift,
    RShift,
}

impl BitwiseOp {
    /// Returns the operator symbol for error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::And => "&",
            Self::Or => "|",
            Self::Xor => "^",
            Self::LShift => "<<",
            Self::RShift => ">>",
        }
    }
}

/// Marker values for special objects that exist but have minimal functionality.
///
/// These are used for:
/// - System objects like `sys.stdout` and `sys.stderr` that need to exist but don't
///   provide functionality in the sandboxed environment
/// - Typing constructs from the `typing` module that are imported for type hints but
///   don't need runtime functionality
///
/// Wraps a `StaticStrings` variant to leverage its string conversion capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Marker(pub StaticStrings);

impl Marker {
    /// Returns the Python type of this marker.
    ///
    /// System markers (stdout, stderr) are `TextIOWrapper`.
    /// `typing.Union` has type `type` (matching CPython).
    /// Dataclasses sentinels have their own dedicated types.
    /// Other typing markers (Any, Optional, etc.) are `_SpecialForm`.
    pub(crate) fn py_type(self) -> Type {
        match self.0 {
            StaticStrings::Stdout | StaticStrings::Stderr => Type::TextIOWrapper,
            StaticStrings::UnionType => Type::Type,
            StaticStrings::AnyStr => Type::TypingTypeVar,
            StaticStrings::DcMissing => Type::DataclassMissingType,
            StaticStrings::DcKwOnly => Type::DataclassKwOnlyType,
            _ => Type::SpecialForm,
        }
    }

    /// Returns whether this marker should be treated as callable for `callable(...)`.
    ///
    /// CPython reports many typing helper singletons as callable even though they
    /// are not regular function objects, so this mirrors that API shape.
    pub(crate) fn is_callable(self) -> bool {
        matches!(
            self.0,
            StaticStrings::Any
                | StaticStrings::Optional
                | StaticStrings::UnionType
                | StaticStrings::ListType
                | StaticStrings::DictType
                | StaticStrings::TupleType
                | StaticStrings::SetType
                | StaticStrings::FrozenSet
                | StaticStrings::Callable
                | StaticStrings::Type
                | StaticStrings::Sequence
                | StaticStrings::Mapping
                | StaticStrings::Iterable
                | StaticStrings::IteratorType
                | StaticStrings::Generator
                | StaticStrings::ClassVar
                | StaticStrings::FinalType
                | StaticStrings::Literal
                | StaticStrings::Generic
                | StaticStrings::Annotated
                | StaticStrings::SelfType
                | StaticStrings::Never
                | StaticStrings::NoReturn
                | StaticStrings::Awaitable
                | StaticStrings::Coroutine
                | StaticStrings::AsyncIterator
                | StaticStrings::AsyncIterable
                | StaticStrings::AsyncGenerator
                | StaticStrings::MutableMapping
                | StaticStrings::MutableSequence
                | StaticStrings::MutableSet
                | StaticStrings::TypingDefaultDict
                | StaticStrings::CollOrderedDict
                | StaticStrings::Counter
                | StaticStrings::TypingDeque
                | StaticStrings::ChainMap
                | StaticStrings::TypingPattern
                | StaticStrings::TypingMatch
                | StaticStrings::TypingIO
                | StaticStrings::TypingTextIO
                | StaticStrings::TypingBinaryIO
                | StaticStrings::TypeGuard
                | StaticStrings::TypeIs
                | StaticStrings::Unpack
                | StaticStrings::ParamSpecArgs
                | StaticStrings::ParamSpecKwargs
                | StaticStrings::Concatenate
                | StaticStrings::TypeAlias
                | StaticStrings::Required
                | StaticStrings::NotRequired
                | StaticStrings::TypingNamedTuple
        )
    }

    /// Writes the Python repr for this marker.
    ///
    /// System markers have special repr formats ("<stdout>", "<stderr>").
    /// Dataclasses sentinels mirror CPython object repr output.
    /// `typing.Union` uses `<class 'typing.Union'>` format (matching CPython).
    /// Other typing markers are prefixed with "typing." (e.g., "typing.Any").
    fn py_repr_fmt(self, f: &mut impl Write, py_id: usize) -> fmt::Result {
        let s: &'static str = self.0.into();
        match self.0 {
            StaticStrings::Stdout => f.write_str("<stdout>")?,
            StaticStrings::Stderr => f.write_str("<stderr>")?,
            StaticStrings::DcMissing => write!(f, "<dataclasses._MISSING_TYPE object at 0x{py_id:x}>")?,
            StaticStrings::DcKwOnly => write!(f, "<dataclasses._KW_ONLY_TYPE object at 0x{py_id:x}>")?,
            StaticStrings::DcInitVar => f.write_str("dataclasses.InitVar")?,
            StaticStrings::UnionType => f.write_str("<class 'typing.Union'>")?,
            _ => write!(f, "typing.{s}")?,
        }
        Ok(())
    }
}

/// High-bit tag reserved for literal singletons (None, Ellipsis, booleans).
const SINGLETON_ID_TAG: usize = 1usize << (usize::BITS - 1);
/// High-bit tag reserved for interned string `id()` values.
const INTERN_STR_ID_TAG: usize = 1usize << (usize::BITS - 2);
/// High-bit tag reserved for interned bytes `id()` values to avoid colliding with any other space.
const INTERN_BYTES_ID_TAG: usize = 1usize << (usize::BITS - 3);
/// High-bit tag reserved for heap-backed `HeapId`s.
const HEAP_ID_TAG: usize = 1usize << (usize::BITS - 4);

/// Mask that keeps pointer-derived bits below the bytes tag bit.
const INTERN_BYTES_ID_MASK: usize = INTERN_BYTES_ID_TAG - 1;
/// Mask that keeps pointer-derived bits below the string tag bit.
const INTERN_STR_ID_MASK: usize = INTERN_STR_ID_TAG - 1;
/// Mask that keeps per-singleton offsets below the singleton tag bit.
const SINGLETON_ID_MASK: usize = SINGLETON_ID_TAG - 1;
/// Mask that keeps heap value IDs below the heap tag bit.
const HEAP_ID_MASK: usize = HEAP_ID_TAG - 1;

/// High-bit tag for Int value-based IDs (no heap allocation needed).
const INT_ID_TAG: usize = 1usize << (usize::BITS - 5);
/// High-bit tag for Float value-based IDs.
const FLOAT_ID_TAG: usize = 1usize << (usize::BITS - 6);
/// High-bit tag for Callable value-based IDs.
const BUILTIN_ID_TAG: usize = 1usize << (usize::BITS - 7);
/// High-bit tag for Function value-based IDs.
const FUNCTION_ID_TAG: usize = 1usize << (usize::BITS - 8);
/// High-bit tag for External Function value-based IDs.
const EXTFUNCTION_ID_TAG: usize = 1usize << (usize::BITS - 9);
/// High-bit tag for Marker value-based IDs (stdout, stderr, etc.).
const MARKER_ID_TAG: usize = 1usize << (usize::BITS - 10);
/// High-bit tag for ExternalFuture value-based IDs.
const EXTERNAL_FUTURE_ID_TAG: usize = 1usize << (usize::BITS - 11);
/// High-bit tag for ModuleFunction value-based IDs.
const MODULE_FUNCTION_ID_TAG: usize = 1usize << (usize::BITS - 12);
/// High-bit tag for interned LongInt `id()` values.
const INTERN_LONG_INT_ID_TAG: usize = 1usize << (usize::BITS - 13);
/// High-bit tag for Property value-based IDs.
const PROPERTY_ID_TAG: usize = 1usize << (usize::BITS - 14);
/// High-bit tag for Proxy value-based IDs.
const PROXY_ID_TAG: usize = 1usize << (usize::BITS - 15);

/// Masks for value-based ID tags (keep bits below the tag bit).
const INT_ID_MASK: usize = INT_ID_TAG - 1;
const FLOAT_ID_MASK: usize = FLOAT_ID_TAG - 1;
const BUILTIN_ID_MASK: usize = BUILTIN_ID_TAG - 1;
const FUNCTION_ID_MASK: usize = FUNCTION_ID_TAG - 1;
const EXTFUNCTION_ID_MASK: usize = EXTFUNCTION_ID_TAG - 1;
const MARKER_ID_MASK: usize = MARKER_ID_TAG - 1;
const EXTERNAL_FUTURE_ID_MASK: usize = EXTERNAL_FUTURE_ID_TAG - 1;
const MODULE_FUNCTION_ID_MASK: usize = MODULE_FUNCTION_ID_TAG - 1;
const INTERN_LONG_INT_ID_MASK: usize = INTERN_LONG_INT_ID_TAG - 1;
const PROPERTY_ID_MASK: usize = PROPERTY_ID_TAG - 1;
const PROXY_ID_MASK: usize = PROXY_ID_TAG - 1;

/// Odd multiplier used to scramble bits for public-facing IDs.
#[cfg(target_pointer_width = "64")]
const PUBLIC_ID_MIX_MULTIPLIER: usize = 0x9E37_79B9_7F4A_7C15;
/// Offset used to further decorrelate public-facing IDs from internal tags.
#[cfg(target_pointer_width = "64")]
const PUBLIC_ID_MIX_OFFSET: usize = 0x0010_0000_0000_03F0;
/// Base value that keeps visible IDs in a pointer-like user-space range.
#[cfg(target_pointer_width = "64")]
const PUBLIC_ID_BASE: usize = 0x0000_1000_0000_0000;
/// Mask controlling how many mixed bits are exposed in the visible ID.
#[cfg(target_pointer_width = "64")]
const PUBLIC_ID_MASK: usize = 0x0000_0FFF_FFFF_FFF0;

/// Odd multiplier used to scramble bits for public-facing IDs.
#[cfg(target_pointer_width = "32")]
const PUBLIC_ID_MIX_MULTIPLIER: usize = 0x9E37_79B1;
/// Offset used to further decorrelate public-facing IDs from internal tags.
#[cfg(target_pointer_width = "32")]
const PUBLIC_ID_MIX_OFFSET: usize = 0x1000_03F0;
/// Base value that keeps visible IDs in a pointer-like user-space range.
#[cfg(target_pointer_width = "32")]
const PUBLIC_ID_BASE: usize = 0x1000_0000;
/// Mask controlling how many mixed bits are exposed in the visible ID.
#[cfg(target_pointer_width = "32")]
const PUBLIC_ID_MASK: usize = 0x0FFF_FFF0;

/// Enumerates singleton literal slots so we can issue stable `id()` values without heap allocation.
#[repr(usize)]
#[derive(Copy, Clone)]
enum SingletonSlot {
    Undefined = 0,
    Ellipsis = 1,
    None = 2,
    False = 3,
    True = 4,
}

/// Returns the fully tagged `id()` value for the requested singleton literal.
#[inline]
const fn singleton_id(slot: SingletonSlot) -> usize {
    SINGLETON_ID_TAG | ((slot as usize) & SINGLETON_ID_MASK)
}

/// Converts a heap `HeapId` into its tagged `id()` value, ensuring it never collides with other spaces.
#[inline]
pub fn heap_tagged_id(heap_id: HeapId) -> usize {
    heap_tagged_id_from_payload(heap_id.index())
}

/// Converts an arbitrary heap payload into the tagged heap `id()` space.
///
/// This is used when a caller needs heap-tagged IDs that are not tied directly
/// to the current arena slot index (for example, Python-visible `id()` values
/// that must remain distinct across slot reuse).
#[inline]
pub(crate) fn heap_tagged_id_from_payload(payload: usize) -> usize {
    HEAP_ID_TAG | (payload & HEAP_ID_MASK)
}

/// Transforms an internal ID into a deterministic Python-visible identity.
///
/// This intentionally obscures internal tagging/layout details while keeping IDs
/// deterministic and pointer-like for repr output and `id()`.
#[inline]
pub(crate) fn public_id_from_internal_id(internal_id: usize) -> usize {
    let mixed = internal_id
        .wrapping_mul(PUBLIC_ID_MIX_MULTIPLIER)
        .wrapping_add(PUBLIC_ID_MIX_OFFSET);
    PUBLIC_ID_BASE.wrapping_add(mixed & PUBLIC_ID_MASK)
}

/// Computes a deterministic ID for an i64 integer value.
/// Uses the value's hash combined with a type tag to ensure uniqueness across types.
#[inline]
fn int_value_id(value: i64) -> usize {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    let hash_u64 = hasher.finish();
    // Mask to usize range before conversion to handle 32-bit platforms
    let masked = hash_u64 & (usize::MAX as u64);
    let hash_usize = usize::try_from(masked).expect("masked value fits in usize");
    INT_ID_TAG | (hash_usize & INT_ID_MASK)
}

/// Computes a deterministic ID for an f64 float value.
/// Uses the bit representation's hash for consistency (handles NaN, infinities, etc.).
#[inline]
fn float_value_id(value: f64) -> usize {
    let mut hasher = DefaultHasher::new();
    value.to_bits().hash(&mut hasher);
    let hash_u64 = hasher.finish();
    // Mask to usize range before conversion to handle 32-bit platforms
    let masked = hash_u64 & (usize::MAX as u64);
    let hash_usize = usize::try_from(masked).expect("masked value fits in usize");
    FLOAT_ID_TAG | (hash_usize & FLOAT_ID_MASK)
}

/// Computes a deterministic ID for a builtin based on its discriminant.
#[inline]
fn builtin_value_id(b: Builtins) -> usize {
    let mut hasher = DefaultHasher::new();
    b.hash(&mut hasher);
    let hash_u64 = hasher.finish();
    // wrapping here is fine
    #[expect(clippy::cast_possible_truncation)]
    let hash_usize = hash_u64 as usize;
    BUILTIN_ID_TAG | (hash_usize & BUILTIN_ID_MASK)
}

/// Computes a deterministic ID for a function based on its id.
#[inline]
fn function_value_id(f_id: FunctionId) -> usize {
    FUNCTION_ID_TAG | (f_id.index() & FUNCTION_ID_MASK)
}

/// Computes a deterministic ID for an external function based on its id.
#[inline]
fn ext_function_value_id(f_id: ExtFunctionId) -> usize {
    EXTFUNCTION_ID_TAG | (f_id.index() & EXTFUNCTION_ID_MASK)
}

/// Computes a deterministic ID for a marker value based on its discriminant.
#[inline]
fn marker_value_id(m: Marker) -> usize {
    MARKER_ID_TAG | ((m.0 as usize) & MARKER_ID_MASK)
}

/// Computes a deterministic ID for a property value based on its discriminant.
#[inline]
fn property_value_id(p: Property) -> usize {
    let discriminant = match p {
        Property::Os(os_fn) => os_fn as usize,
    };
    PROPERTY_ID_TAG | (discriminant & PROPERTY_ID_MASK)
}

/// Computes a deterministic ID for a proxy value based on its raw ID.
#[inline]
fn proxy_value_id(proxy_id: ProxyId) -> usize {
    PROXY_ID_TAG | ((proxy_id.raw() as usize) & PROXY_ID_MASK)
}

/// Computes a deterministic ID for an external future based on its call ID.
#[inline]
fn external_future_value_id(call_id: CallId) -> usize {
    EXTERNAL_FUTURE_ID_TAG | ((call_id.raw() as usize) & EXTERNAL_FUTURE_ID_MASK)
}

/// Computes a deterministic ID for a module function based on its discriminant.
#[inline]
fn module_function_value_id(mf: ModuleFunctions) -> usize {
    let mut hasher = DefaultHasher::new();
    mf.hash(&mut hasher);
    let hash_u64 = hasher.finish();
    // wrapping here is fine
    #[expect(clippy::cast_possible_truncation)]
    let hash_usize = hash_u64 as usize;
    MODULE_FUNCTION_ID_TAG | (hash_usize & MODULE_FUNCTION_ID_MASK)
}

/// Converts an i64 repeat count to usize, handling negative values and overflow.
///
/// Returns 0 for negative values (Python treats negative repeat counts as 0).
/// Returns `OverflowError` if the value exceeds `usize::MAX`.
#[inline]
fn i64_to_repeat_count(n: i64) -> RunResult<usize> {
    if n <= 0 {
        Ok(0)
    } else {
        usize::try_from(n).map_err(|_| ExcType::overflow_repeat_count().into())
    }
}

/// Converts a LongInt repeat count to usize, handling negative values and overflow.
///
/// Returns 0 for negative values (Python treats negative repeat counts as 0).
/// Returns `OverflowError` if the value exceeds `usize::MAX`.
#[inline]
fn longint_to_repeat_count(li: &LongInt) -> RunResult<usize> {
    if li.is_negative() {
        Ok(0)
    } else if let Some(count) = li.to_usize() {
        Ok(count)
    } else {
        Err(ExcType::overflow_repeat_count().into())
    }
}

/// Extracts a BigInt from a Value for bitwise operations.
///
/// Returns `Some(BigInt)` for Int, Bool, and LongInt values.
/// Returns `None` for other types (Float, Str, etc.).
fn extract_bigint(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<BigInt> {
    match value {
        Value::Int(i) => Some(BigInt::from(*i)),
        Value::Bool(b) => Some(BigInt::from(i64::from(*b))),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => Some(li.inner().clone()),
            HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) => Some(BigInt::from(*bits)),
            _ => None,
        },
        _ => None,
    }
}

fn extract_regex_flag_bits(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<i64> {
    match value {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Ref(id) => {
            if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(*id) {
                Some(*bits)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns true if this value is a heap-allocated `Decimal`.
fn is_decimal(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(value, Value::Ref(id) if matches!(heap.get_if_live(*id), Some(HeapData::Decimal(_))))
}

/// Converts a value to Decimal for decimal arithmetic coercion.
///
/// Supports decimal/int/bool/longint values.
fn extract_decimal(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<Decimal> {
    match value {
        Value::Int(i) => Some(Decimal::from_i64(*i)),
        Value::Bool(b) => Some(Decimal::from_i64(i64::from(*b))),
        Value::Ref(id) => match heap.get_if_live(*id)? {
            HeapData::Decimal(decimal) => Some(decimal.clone()),
            HeapData::LongInt(long) => Some(Decimal::new(long.inner().clone(), 0)),
            _ => None,
        },
        _ => None,
    }
}

/// Extracts normalized decimal operands when at least one side is a real `Decimal`.
fn decimal_operands(lhs: &Value, rhs: &Value, heap: &Heap<impl ResourceTracker>) -> Option<(Decimal, Decimal)> {
    if !is_decimal(lhs, heap) && !is_decimal(rhs, heap) {
        return None;
    }
    Some((extract_decimal(lhs, heap)?, extract_decimal(rhs, heap)?))
}

fn is_regex_flag_object(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(
        value,
        Value::Ref(id) if matches!(heap.get(*id), HeapData::StdlibObject(StdlibObject::RegexFlagValue(_)))
    )
}

/// Returns true if this value is a heap-allocated `Fraction`.
fn is_fraction(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(value, Value::Ref(id) if matches!(heap.get_if_live(*id), Some(HeapData::Fraction(_))))
}

/// Converts a numeric value to `Fraction` when possible.
///
/// Supports int-like values (`int`, `bool`, `LongInt`) and `Fraction`.
fn extract_fraction(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<Fraction> {
    match value {
        Value::Int(i) => Some(Fraction::from_i64_single(*i)),
        Value::Bool(b) => Some(Fraction::from_i64_single(i64::from(*b))),
        Value::Ref(id) => match heap.get_if_live(*id)? {
            HeapData::Fraction(f) => Some(f.clone()),
            HeapData::LongInt(li) => Fraction::new(li.inner().clone(), BigInt::from(1)).ok(),
            _ => None,
        },
        _ => None,
    }
}

/// Extracts normalized fraction operands when at least one side is a real `Fraction`.
fn fraction_operands(lhs: &Value, rhs: &Value, heap: &Heap<impl ResourceTracker>) -> Option<(Fraction, Fraction)> {
    if !is_fraction(lhs, heap) && !is_fraction(rhs, heap) {
        return None;
    }
    let left = extract_fraction(lhs, heap)?;
    let right = extract_fraction(rhs, heap)?;
    Some((left, right))
}

/// Converts a numeric scalar value to f64 when possible.
///
/// Supports `int`, `float`, `bool`, and `LongInt`.
fn extract_numeric_scalar(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                li.to_f64()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extracts `(real, imag)` for runtime complex objects.
fn extract_complex_components(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<(f64, f64)> {
    let Value::Ref(id) = value else {
        return None;
    };
    let HeapData::StdlibObject(StdlibObject::Complex { real, imag }) = heap.get(*id) else {
        return None;
    };
    Some((*real, *imag))
}

/// Allocates a runtime `complex` value.
fn allocate_complex_value(
    heap: &mut Heap<impl ResourceTracker>,
    real: f64,
    imag: f64,
) -> Result<Value, crate::resource::ResourceError> {
    let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(real, imag)))?;
    Ok(Value::Ref(id))
}

/// Returns true when `value` is a runtime complex object.
fn is_runtime_complex(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    extract_complex_components(value, heap).is_some()
}

/// Extracts complex-like components from either a runtime complex object or a numeric scalar.
fn extract_complex_or_scalar_components(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<(f64, f64)> {
    if let Some((real, imag)) = extract_complex_components(value, heap) {
        return Some((real, imag));
    }
    extract_numeric_scalar(value, heap).map(|real| (real, 0.0))
}

/// Returns true when a float value is integral (including `-0.0`).
fn float_is_integral(value: f64) -> bool {
    value.fract() == 0.0
}

/// Computes `complex(base_real, base_imag) ** complex(exp_real, exp_imag)`.
///
/// Uses polar form (`exp(exp * log(base))`) and mirrors CPython's special-case
/// behavior for zero bases.
fn complex_pow_components(base_real: f64, base_imag: f64, exp_real: f64, exp_imag: f64) -> RunResult<(f64, f64)> {
    if exp_real == 0.0 && exp_imag == 0.0 {
        return Ok((1.0, 0.0));
    }

    if exp_imag == 0.0
        && exp_real.is_finite()
        && float_is_integral(exp_real)
        && let Some(exp_i64) = f64_to_i64_exact(exp_real)
    {
        return complex_pow_integer_exponent(base_real, base_imag, exp_i64);
    }

    if base_real == 0.0 && base_imag == 0.0 {
        if exp_imag != 0.0 || exp_real < 0.0 {
            return Err(ExcType::zero_negative_or_complex_power());
        }
        return Ok((0.0, 0.0));
    }

    let radius = f64::hypot(base_real, base_imag);
    let theta = base_imag.atan2(base_real);
    let log_radius = radius.ln();

    let magnitude_log = exp_real * log_radius - exp_imag * theta;
    let angle = exp_real * theta + exp_imag * log_radius;
    let magnitude = magnitude_log.exp();

    Ok((magnitude * angle.cos(), magnitude * angle.sin()))
}

/// Converts an exact integral float to i64 when the value fits in range.
fn f64_to_i64_exact(value: f64) -> Option<i64> {
    if !value.is_finite() || !float_is_integral(value) {
        return None;
    }
    if value < i64::MIN as f64 || value > i64::MAX as f64 {
        return None;
    }
    Some(value as i64)
}

/// Computes complex power with an integer exponent using repeated squaring.
fn complex_pow_integer_exponent(base_real: f64, base_imag: f64, exponent: i64) -> RunResult<(f64, f64)> {
    if exponent == 0 {
        return Ok((1.0, 0.0));
    }
    if base_real == 0.0 && base_imag == 0.0 && exponent < 0 {
        return Err(ExcType::zero_negative_or_complex_power());
    }

    let mut exp = exponent.unsigned_abs();
    let mut base = (base_real, base_imag);
    let mut result = (1.0, 0.0);

    while exp > 0 {
        if exp & 1 == 1 {
            result = complex_mul(result, base);
        }
        exp >>= 1;
        if exp > 0 {
            base = complex_mul(base, base);
        }
    }

    if exponent > 0 {
        return Ok(result);
    }

    let denominator = result.0 * result.0 + result.1 * result.1;
    if denominator == 0.0 {
        return Err(ExcType::zero_negative_or_complex_power());
    }
    Ok((result.0 / denominator, -result.1 / denominator))
}

/// Multiplies two complex numbers represented as `(real, imag)` pairs.
fn complex_mul(lhs: (f64, f64), rhs: (f64, f64)) -> (f64, f64) {
    (lhs.0 * rhs.0 - lhs.1 * rhs.1, lhs.0 * rhs.1 + lhs.1 * rhs.0)
}

/// Extracts `(mu, sigma)` from a `statistics.NormalDist`-shaped named tuple.
///
/// The runtime currently represents `NormalDist` instances as named tuples with
/// fixed field layout; this helper detects that shape without requiring intern lookups.
pub(crate) fn extract_normaldist_params(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<(f64, f64)> {
    let Value::Ref(id) = value else {
        return None;
    };
    let HeapData::NamedTuple(named_tuple) = heap.get(*id) else {
        return None;
    };
    if !is_normaldist_namedtuple(named_tuple) {
        return None;
    }

    let items = named_tuple.as_vec();
    if items.len() < 2 {
        return None;
    }
    let mu = extract_numeric_scalar(&items[0], heap)?;
    let sigma = extract_numeric_scalar(&items[1], heap)?;
    Some((mu, sigma))
}

/// Returns true when a named tuple matches the `statistics.NormalDist` field layout.
fn is_normaldist_namedtuple(named_tuple: &crate::types::NamedTuple) -> bool {
    let expected = [
        "mean",
        "stdev",
        "variance",
        "pdf",
        "cdf",
        "inv_cdf",
        "overlap",
        "samples",
        "quantiles",
        "zscore",
    ];
    let field_names = named_tuple.field_names();
    field_names.len() == expected.len()
        && field_names
            .iter()
            .zip(expected.iter())
            .all(|(field_name, expected_name)| either_str_matches(field_name, expected_name))
}

/// Compares an `EitherStr` with an ASCII literal without requiring intern table access.
fn either_str_matches(value: &EitherStr, expected: &str) -> bool {
    match value {
        EitherStr::Heap(s) => s == expected,
        EitherStr::Interned(id) => StaticStrings::from_str(expected)
            .ok()
            .is_some_and(|s| *id == crate::intern::StringId::from(s)),
    }
}

/// Helper for dict merge operations (`dict | dict`).
///
/// Returns `Ok(Some(value))` when both operands are dicts and `op` is `|`,
/// otherwise returns `Ok(None)`.
fn py_dict_bitwise(
    lhs: &Value,
    rhs: &Value,
    op: BitwiseOp,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    if !matches!(op, BitwiseOp::Or) {
        return Ok(None);
    }

    let (Value::Ref(lhs_id), Value::Ref(rhs_id)) = (lhs, rhs) else {
        return Ok(None);
    };
    if !matches!(heap.get(*lhs_id), HeapData::Dict(_)) || !matches!(heap.get(*rhs_id), HeapData::Dict(_)) {
        return Ok(None);
    }

    let mut result = heap.with_entry_mut(*lhs_id, |heap_inner, data| match data {
        HeapData::Dict(dict) => dict.clone_with_heap(heap_inner, interns),
        _ => unreachable!("checked Dict above"),
    })?;

    let rhs_items = heap.with_entry_mut(*rhs_id, |heap_inner, data| -> RunResult<Vec<(Value, Value)>> {
        match data {
            HeapData::Dict(dict) => Ok(dict.items(heap_inner)),
            _ => unreachable!("checked Dict above"),
        }
    })?;

    for (key, value) in rhs_items {
        if let Some(old) = result.set(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }

    let result_id = heap.allocate(HeapData::Dict(result))?;
    Ok(Some(Value::Ref(result_id)))
}

/// Helper for set bitwise operations (| & ^).
///
/// Called by `py_bitwise` when both operands are sets, frozensets, or dict views.
/// Returns `Ok(Some(Value))` if the operation was handled,
/// `Ok(None)` if the values are not sets (fall back to numeric bitwise).
fn py_set_bitwise(
    lhs: &Value,
    rhs: &Value,
    op: BitwiseOp,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    // Extract set-like data from lhs (Set, FrozenSet, DictKeys, or DictItems)
    let Some((lhs_storage, lhs_is_frozen)) = extract_set_like(lhs, heap, interns)? else {
        return Ok(None);
    };

    // Extract set-like data from rhs
    let Some((rhs_storage, rhs_is_frozen)) = extract_set_like(rhs, heap, interns)? else {
        // Clean up lhs storage
        lhs_storage.drop_all_values(heap);
        return Ok(None);
    };

    // Perform the set operation
    let mut result_storage = match op {
        BitwiseOp::Or => lhs_storage.union(&rhs_storage, heap, interns)?,
        BitwiseOp::And => lhs_storage.intersection(&rhs_storage, heap, interns)?,
        BitwiseOp::Xor => lhs_storage.symmetric_difference(&rhs_storage, heap, interns)?,
        _ => {
            // Clean up storages
            lhs_storage.drop_all_values(heap);
            rhs_storage.drop_all_values(heap);
            return Ok(None);
        } // LShift/RShift don't apply to sets
    };

    // Clean up the source storages (result_storage has its own cloned values)
    lhs_storage.drop_all_values(heap);
    rhs_storage.drop_all_values(heap);

    // Normalize set-op result ordering to align iteration/repr behavior.
    result_storage.sort_by_hash(heap, interns);

    // If both operands are frozensets, return a frozenset, otherwise return a set
    let is_frozen = lhs_is_frozen && rhs_is_frozen;
    let result = if is_frozen {
        HeapData::FrozenSet(FrozenSet::from_storage(result_storage))
    } else {
        HeapData::Set(Set::from_storage(result_storage))
    };

    let heap_id = heap.allocate(result)?;
    Ok(Some(Value::Ref(heap_id)))
}

/// Helper for Counter subtraction (`Counter - Counter`).
///
/// Returns `Ok(Some(value))` when both operands are Counters, otherwise `Ok(None)`.
pub fn py_counter_subtract(
    lhs: &Value,
    rhs: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let (Value::Ref(lhs_id), Value::Ref(rhs_id)) = (lhs, rhs) else {
        return Ok(None);
    };

    if !matches!(heap.get(*lhs_id), HeapData::Counter(_)) || !matches!(heap.get(*rhs_id), HeapData::Counter(_)) {
        return Ok(None);
    }

    heap.with_two(*lhs_id, *rhs_id, |heap_inner, left, right| match (left, right) {
        (HeapData::Counter(lhs_counter), HeapData::Counter(rhs_counter)) => lhs_counter
            .binary_sub_value(rhs_counter, heap_inner, interns)
            .map(Some)
            .map_err(Into::into),
        _ => Ok(None),
    })
}

/// Helper for Counter bitwise operations (`Counter & Counter`, `Counter | Counter`).
///
/// Returns `Ok(Some(value))` when handled, otherwise `Ok(None)`.
pub fn py_counter_bitwise(
    lhs: &Value,
    rhs: &Value,
    op: BitwiseOp,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let (Value::Ref(lhs_id), Value::Ref(rhs_id)) = (lhs, rhs) else {
        return Ok(None);
    };

    if !matches!(heap.get(*lhs_id), HeapData::Counter(_)) || !matches!(heap.get(*rhs_id), HeapData::Counter(_)) {
        return Ok(None);
    }

    heap.with_two(*lhs_id, *rhs_id, |heap_inner, left, right| match (left, right) {
        (HeapData::Counter(lhs_counter), HeapData::Counter(rhs_counter)) => {
            let result = match op {
                BitwiseOp::And => lhs_counter.binary_and_value(rhs_counter, heap_inner, interns),
                BitwiseOp::Or => lhs_counter.binary_or_value(rhs_counter, heap_inner, interns),
                BitwiseOp::Xor | BitwiseOp::LShift | BitwiseOp::RShift => return Ok(None),
            };
            result.map(Some).map_err(Into::into)
        }
        _ => Ok(None),
    })
}

/// Helper for ChainMap union operations (`ChainMap | mapping`).
///
/// Returns `Ok(Some(value))` when handled, otherwise `Ok(None)`.
fn py_chainmap_bitwise(
    lhs: &Value,
    rhs: &Value,
    op: BitwiseOp,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    if !matches!(op, BitwiseOp::Or) {
        return Ok(None);
    }

    let Value::Ref(lhs_id) = lhs else {
        return Ok(None);
    };
    if !matches!(heap.get(*lhs_id), HeapData::ChainMap(_)) {
        return Ok(None);
    }

    let mut result = heap.with_entry_mut(*lhs_id, |heap_inner, data| match data {
        HeapData::ChainMap(chain_map) => chain_map.flat().clone_with_heap(heap_inner, interns),
        _ => unreachable!("checked ChainMap above"),
    })?;

    let rhs_items = match rhs {
        Value::Ref(rhs_id) => {
            heap.with_entry_mut(*rhs_id, |heap_inner, data| -> RunResult<Option<Vec<(Value, Value)>>> {
                match data {
                    HeapData::Dict(dict) => Ok(Some(dict.items(heap_inner))),
                    HeapData::DefaultDict(default_dict) => Ok(Some(default_dict.dict().items(heap_inner))),
                    HeapData::Counter(counter) => Ok(Some(counter.dict().items(heap_inner))),
                    HeapData::OrderedDict(ordered) => Ok(Some(ordered.dict().items(heap_inner))),
                    HeapData::ChainMap(chain_map) => Ok(Some(chain_map.flat_items(heap_inner))),
                    _ => Ok(None),
                }
            })?
        }
        _ => None,
    };

    let Some(rhs_items) = rhs_items else {
        result.drop_all_entries(heap);
        return Ok(None);
    };

    for (key, value) in rhs_items {
        if let Some(old) = result.set(key, value, heap, interns)? {
            old.drop_with_heap(heap);
        }
    }

    let result_id = heap.allocate(HeapData::Dict(result))?;
    Ok(Some(Value::Ref(result_id)))
}

/// Helper for runtime union type construction (`A | B`).
///
/// This mirrors the subset of `types.UnionType` behavior required by typing
/// parity tests, including `int | str` and `int | None`.
fn py_type_union_bitwise(
    lhs: &Value,
    rhs: &Value,
    op: BitwiseOp,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    if !matches!(op, BitwiseOp::Or) {
        return Ok(None);
    }
    let Some(mut lhs_items) = union_operand_items(lhs, heap) else {
        return Ok(None);
    };
    let Some(rhs_items) = union_operand_items(rhs, heap) else {
        for value in lhs_items {
            value.drop_with_heap(heap);
        }
        return Ok(None);
    };
    lhs_items.extend(rhs_items);
    let mut union_items: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
    union_items.extend(lhs_items);
    let item = crate::types::allocate_tuple(union_items, heap)?;
    crate::types::make_generic_alias(Value::Marker(Marker(StaticStrings::UnionType)), item, heap, interns).map(Some)
}

/// Expands a value into union operands used by `py_type_union_bitwise`.
///
/// Returns `None` when the value is not a type-like runtime object that
/// participates in union construction.
fn union_operand_items(value: &Value, heap: &mut Heap<impl ResourceTracker>) -> Option<Vec<Value>> {
    match value {
        Value::None => Some(vec![Value::Builtin(Builtins::Type(Type::NoneType))]),
        Value::Builtin(Builtins::Type(_) | Builtins::Function(BuiltinsFunctions::Type) | Builtins::ExcType(_)) => {
            Some(vec![value.clone_with_heap(heap)])
        }
        Value::Ref(id) => match heap.get(*id) {
            HeapData::ClassObject(_) => Some(vec![value.clone_with_heap(heap)]),
            HeapData::GenericAlias(alias)
                if matches!(alias.origin(), Value::Marker(Marker(StaticStrings::UnionType))) =>
            {
                Some(alias.args().iter().map(|arg| arg.clone_with_heap(heap)).collect())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Helper for unary plus on Counter values (`+counter`).
///
/// Returns `Ok(Some(value))` when `value` is a Counter, otherwise `Ok(None)`.
pub fn py_counter_unary_pos(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let Value::Ref(counter_id) = value else {
        return Ok(None);
    };

    if !matches!(heap.get(*counter_id), HeapData::Counter(_)) {
        return Ok(None);
    }

    heap.with_entry_mut(*counter_id, |heap_inner, data| match data {
        HeapData::Counter(counter) => counter
            .unary_pos_value(heap_inner, interns)
            .map(Some)
            .map_err(Into::into),
        _ => Ok(None),
    })
}

/// Helper for unary minus on Counter values (`-counter`).
///
/// Returns `Ok(Some(value))` when `value` is a Counter, otherwise `Ok(None)`.
pub fn py_counter_unary_neg(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let Value::Ref(counter_id) = value else {
        return Ok(None);
    };

    if !matches!(heap.get(*counter_id), HeapData::Counter(_)) {
        return Ok(None);
    }

    heap.with_entry_mut(*counter_id, |heap_inner, data| match data {
        HeapData::Counter(counter) => counter
            .unary_neg_value(heap_inner, interns)
            .map(Some)
            .map_err(Into::into),
        _ => Ok(None),
    })
}

/// Helper to extract set-like data from a value.
/// Returns Some((SetStorage, is_frozen)) or None if not set-like.
fn extract_set_like(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<(SetStorage, bool)>> {
    match value {
        Value::Ref(id) => {
            // First, extract what we need without keeping heap borrowed
            let data_type = match heap.get(*id) {
                HeapData::Set(_) => 0,
                HeapData::FrozenSet(_) => 1,
                HeapData::DictKeys(_) => 2,
                HeapData::DictItems(_) => 3,
                _ => return Ok(None),
            };

            match data_type {
                0 => {
                    // Set
                    let entries = if let HeapData::Set(s) = heap.get(*id) {
                        s.storage().copy_entries()
                    } else {
                        unreachable!()
                    };
                    // Increment refcounts for copied values
                    SetStorage::inc_refs_for_entries(&entries, heap);
                    Ok(Some((SetStorage::from_entries(entries), false)))
                }
                1 => {
                    // FrozenSet
                    let entries = if let HeapData::FrozenSet(s) = heap.get(*id) {
                        s.storage().copy_entries()
                    } else {
                        unreachable!()
                    };
                    // Increment refcounts for copied values
                    SetStorage::inc_refs_for_entries(&entries, heap);
                    Ok(Some((SetStorage::from_entries(entries), true)))
                }
                2 => {
                    // DictKeys - need mutable heap access
                    let dict_id = if let HeapData::DictKeys(dk) = heap.get(*id) {
                        dk.dict_id()
                    } else {
                        unreachable!()
                    };
                    let view = DictKeys::new(dict_id);
                    let storage = view.to_set_storage(heap, interns)?;
                    Ok(Some((storage, false)))
                }
                3 => {
                    // DictItems - need mutable heap access
                    let dict_id = if let HeapData::DictItems(di) = heap.get(*id) {
                        di.dict_id()
                    } else {
                        unreachable!()
                    };
                    let view = DictItems::new(dict_id);
                    let storage = view.to_set_storage(heap, interns)?;
                    Ok(Some((storage, false)))
                }
                _ => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

/// Helper for set difference operation (lhs - rhs).
///
/// Called by `binary_sub` when both operands are sets, frozensets, or dict views.
/// Returns `Ok(Some(Value))` if the operation was handled,
/// `Ok(None)` if the values are not sets (fall back to numeric subtraction).
pub(crate) fn py_set_difference(
    lhs: &Value,
    rhs: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    // Extract set-like data from lhs (Set, FrozenSet, DictKeys, or DictItems)
    let Some((lhs_storage, lhs_frozen)) = extract_set_like(lhs, heap, interns)? else {
        return Ok(None);
    };

    // Extract set-like data from rhs
    let Some((rhs_storage, rhs_frozen)) = extract_set_like(rhs, heap, interns)? else {
        // Clean up lhs storage
        lhs_storage.drop_all_values(heap);
        return Ok(None);
    };

    // Perform the set difference operation
    let mut result_storage = lhs_storage.difference(&rhs_storage, heap, interns)?;

    // Clean up the source storages (result_storage has its own cloned values)
    lhs_storage.drop_all_values(heap);
    rhs_storage.drop_all_values(heap);

    // Normalize set-op result ordering to align iteration/repr behavior.
    result_storage.sort_by_hash(heap, interns);

    // If both operands are frozensets, return a frozenset, otherwise return a set
    let is_frozen = lhs_frozen && rhs_frozen;
    let result = if is_frozen {
        HeapData::FrozenSet(FrozenSet::from_storage(result_storage))
    } else {
        HeapData::Set(Set::from_storage(result_storage))
    };

    let heap_id = heap.allocate(result)?;
    Ok(Some(Value::Ref(heap_id)))
}

/// Helper for containment checks in bytes containers.
///
/// Python bytes membership accepts either:
/// - an integer in range 0..=255 (single-byte membership)
/// - a bytes object (subsequence membership)
fn bytes_contains(
    container_bytes: &[u8],
    item: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    match item {
        Value::Int(i) => {
            let byte = u8::try_from(*i)
                .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "byte must be in range(0, 256)"))?;
            Ok(container_bytes.contains(&byte))
        }
        Value::Bool(b) => Ok(container_bytes.contains(&u8::from(*b))),
        Value::InternBytes(bytes_id) => {
            let needle = interns.get_bytes(*bytes_id);
            if needle.is_empty() {
                return Ok(true);
            }
            Ok(container_bytes.windows(needle.len()).any(|window| window == needle))
        }
        Value::Ref(item_heap_id) => match heap.get(*item_heap_id) {
            HeapData::Bytes(item_bytes) => {
                let needle = item_bytes.as_slice();
                if needle.is_empty() {
                    return Ok(true);
                }
                Ok(container_bytes.windows(needle.len()).any(|window| window == needle))
            }
            _ => Err(ExcType::type_error(format!(
                "a bytes-like object is required, not '{}'",
                item.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "a bytes-like object is required, not '{}'",
            item.py_type(heap)
        ))),
    }
}

/// Helper for substring containment check in strings.
///
/// Called by `py_contains` when the container is a string.
/// The item must also be a string (either interned or heap-allocated).
fn str_contains(
    container_str: &str,
    item: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    match item {
        Value::InternString(item_id) => {
            let item_str = interns.get_str(*item_id);
            Ok(container_str.contains(item_str))
        }
        Value::Ref(item_heap_id) => {
            if let HeapData::Str(item_str) = heap.get(*item_heap_id) {
                Ok(container_str.contains(item_str.as_str()))
            } else {
                Err(ExcType::type_error("'in <str>' requires string as left operand"))
            }
        }
        _ => Err(ExcType::type_error("'in <str>' requires string as left operand")),
    }
}

/// Computes the number of significant bits in an i64.
///
/// Returns 0 for 0, otherwise returns ceil(log2(|value|)) + 1 (accounting for sign).
/// For example: 0 -> 0, 1 -> 1, 2 -> 2, 255 -> 8, 256 -> 9.
fn i64_bits(value: i64) -> u64 {
    if value == 0 {
        0
    } else {
        // For negative numbers, use unsigned_abs to get magnitude
        u64::from(64 - value.unsigned_abs().leading_zeros())
    }
}

/// Checks if a pow operation result would exceed the large result threshold.
///
/// If the estimated result is larger than `LARGE_RESULT_THRESHOLD`, calls
/// `heap.tracker().check_large_result()` to allow the tracker to reject the operation.
/// Returns `Ok(())` if the operation should proceed, or an error to reject.
fn check_pow_size(base_bits: u64, exp: u64, heap: &Heap<impl ResourceTracker>) -> Result<(), RunError> {
    // Special case: 0 or 1 bit bases can't produce large results worth checking
    // (0**n = 0, 1**n = 1, -1**n = ±1)
    if base_bits <= 1 {
        return Ok(());
    }

    if let Some(estimated) = LongInt::estimate_pow_bytes(base_bits, exp)
        && estimated > LARGE_RESULT_THRESHOLD
    {
        heap.tracker().check_large_result(estimated)?;
    }
    // If estimate overflows, proceed anyway - into_value will catch it during allocation
    Ok(())
}

/// Computes BigInt exponentiation for exponents larger than u32::MAX.
///
/// Uses repeated squaring for efficiency. This is needed when the exponent
/// doesn't fit in a u32, which is required by the `num-bigint` pow method.
fn bigint_pow(base: BigInt, exp: u64) -> BigInt {
    if exp == 0 {
        return BigInt::from(1);
    }
    if exp == 1 {
        return base;
    }

    // Use repeated squaring
    let mut result = BigInt::from(1);
    let mut b = base;
    let mut e = exp;

    while e > 0 {
        if e & 1 == 1 {
            result *= &b;
        }
        b = &b * &b;
        e >>= 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;
    use crate::resource::NoLimitTracker;

    /// Creates a heap and directly allocates a LongInt with the given BigInt value.
    ///
    /// This bypasses `LongInt::into_value()` which would demote i64-fitting values.
    /// Used to test defensive code paths that handle LongInt-as-index scenarios.
    fn create_heap_with_longint(value: BigInt) -> (Heap<NoLimitTracker>, HeapId) {
        let mut heap = Heap::new(16, NoLimitTracker);
        let long_int = LongInt::new(value);
        let heap_id = heap.allocate(HeapData::LongInt(long_int)).unwrap();
        (heap, heap_id)
    }

    /// Tests that `as_index()` correctly handles a LongInt containing an i64-fitting value.
    ///
    /// This tests a defensive code path that's normally unreachable because
    /// `LongInt::into_value()` demotes i64-fitting values to `Value::Int`.
    /// However, this path could be reached via deserialization of crafted data.
    #[test]
    fn as_index_longint_fits_in_i64() {
        let (mut heap, heap_id) = create_heap_with_longint(BigInt::from(42));
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert_eq!(result.unwrap(), 42);
        value.drop_with_heap(&mut heap);
    }

    /// Tests that `as_index()` correctly handles a negative LongInt that fits in i64.
    #[test]
    fn as_index_longint_negative_fits_in_i64() {
        let (mut heap, heap_id) = create_heap_with_longint(BigInt::from(-100));
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert_eq!(result.unwrap(), -100);
        value.drop_with_heap(&mut heap);
    }

    /// Tests that `as_index()` returns IndexError for LongInt values too large for i64.
    #[test]
    fn as_index_longint_too_large() {
        // 2^100 is way larger than i64::MAX
        let big_value = BigInt::from(2).pow(100);
        let (mut heap, heap_id) = create_heap_with_longint(big_value);
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert!(result.is_err());
        value.drop_with_heap(&mut heap);
    }

    /// Tests that `as_int()` correctly handles a LongInt containing an i64-fitting value.
    ///
    /// Similar to `as_index`, this tests a defensive code path normally unreachable.
    #[test]
    fn as_int_longint_fits_in_i64() {
        let (mut heap, heap_id) = create_heap_with_longint(BigInt::from(12345));
        let value = Value::Ref(heap_id);

        let result = value.as_int(&heap);
        assert_eq!(result.unwrap(), 12345);
        value.drop_with_heap(&mut heap);
    }

    /// Tests that `as_int()` returns an error for LongInt values too large for i64.
    #[test]
    fn as_int_longint_too_large() {
        let big_value = BigInt::from(2).pow(100);
        let (mut heap, heap_id) = create_heap_with_longint(big_value);
        let value = Value::Ref(heap_id);

        let result = value.as_int(&heap);
        assert!(result.is_err());
        value.drop_with_heap(&mut heap);
    }

    /// Tests boundary values: i64::MAX as a LongInt.
    #[test]
    fn as_index_longint_at_i64_max() {
        let (mut heap, heap_id) = create_heap_with_longint(BigInt::from(i64::MAX));
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert_eq!(result.unwrap(), i64::MAX);
        value.drop_with_heap(&mut heap);
    }

    /// Tests boundary values: i64::MIN as a LongInt.
    #[test]
    fn as_index_longint_at_i64_min() {
        let (mut heap, heap_id) = create_heap_with_longint(BigInt::from(i64::MIN));
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert_eq!(result.unwrap(), i64::MIN);
        value.drop_with_heap(&mut heap);
    }

    /// Tests boundary values: i64::MAX + 1 as a LongInt (should fail).
    #[test]
    fn as_index_longint_just_over_i64_max() {
        let big_value = BigInt::from(i64::MAX) + BigInt::from(1);
        let (mut heap, heap_id) = create_heap_with_longint(big_value);
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert!(result.is_err());
        value.drop_with_heap(&mut heap);
    }

    /// Tests boundary values: i64::MIN - 1 as a LongInt (should fail).
    #[test]
    fn as_index_longint_just_under_i64_min() {
        let big_value = BigInt::from(i64::MIN) - BigInt::from(1);
        let (mut heap, heap_id) = create_heap_with_longint(big_value);
        let value = Value::Ref(heap_id);

        let result = value.as_index(&heap, Type::List);
        assert!(result.is_err());
        value.drop_with_heap(&mut heap);
    }
}
