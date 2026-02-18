use std::{
    borrow::Cow,
    cell::Cell,
    collections::{BTreeMap, VecDeque, hash_map::DefaultHasher},
    fmt::Write,
    hash::{Hash, Hasher},
    mem::{ManuallyDrop, discriminant},
    ptr::addr_of,
    sync::atomic::{AtomicUsize, Ordering},
    vec,
};

use ahash::{AHashMap, AHashSet};
use num_integer::Integer;
use serde::ser::SerializeStruct;
use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    asyncio::{Coroutine, GatherFuture, GatherItem},
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    intern::{FunctionId, Interns, StringId},
    py_hash::{cpython_hash_bytes_seed0, cpython_hash_str_seed0},
    resource::{LARGE_RESULT_THRESHOLD, MAX_DATA_RECURSION_DEPTH, ResourceError, ResourceTracker},
    types::{
        AttrCallResult, AttrGetter, Bytes, CachedProperty, ChainMap, ClassGetItem, ClassObject, ClassSubclasses,
        CmpToKey, Dataclass, Decimal, DefaultDict, Deque, Dict, DictItems, DictKeys, DictValues, ExitCallback,
        Fraction, FrozenSet, FunctionGet, Generator, GenericAlias, Instance, ItemGetter, List, LongInt, MappingProxy,
        MethodCaller, Module, NamedTuple, OurosIter, Partial, PartialMethod, Path, Placeholder, PyTrait, Range,
        ReMatch, RePattern, SafeUuid, Set, SetStorage, SingleDispatch, SingleDispatchMethod, SingleDispatchRegister,
        Slice, SlotDescriptor, StdlibObject, Str, TeeState, TextWrapper, Tuple, Type, Uuid, WeakRef, allocate_tuple,
    },
    value::{EitherStr, Value},
};

/// Snapshot of heap state at a point in time.
///
/// Captures object counts by type, total allocations, and memory estimates.
/// Used for monitoring heap growth and comparing states via heap diffs.
///
/// The `objects_by_type` map uses `BTreeMap` for deterministic iteration order,
/// making snapshots suitable for display and comparison without sort overhead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapStats {
    /// Total number of live objects on the heap.
    pub live_objects: usize,
    /// Number of free (recycled) slots available for reuse.
    pub free_slots: usize,
    /// Total heap capacity (live + free).
    pub total_slots: usize,
    /// Breakdown of live objects by `HeapData` variant name.
    ///
    /// Keys are static variant names (e.g., "List", "Dict", "Str").
    /// Values are the count of live objects of that type.
    pub objects_by_type: BTreeMap<&'static str, usize>,
    /// Number of interned strings in the session's interner.
    ///
    /// This counts only dynamically interned strings (not the base set of
    /// pre-interned attribute names and ASCII characters).
    pub interned_strings: usize,
    /// Resource tracker allocation count, if using `LimitedTracker`.
    ///
    /// `None` when the heap uses `NoLimitTracker` (the default for REPL sessions).
    pub tracker_allocations: Option<usize>,
    /// Resource tracker memory usage in bytes, if using `LimitedTracker`.
    ///
    /// `None` when the heap uses `NoLimitTracker` (the default for REPL sessions).
    pub tracker_memory_bytes: Option<usize>,
}

/// Difference between two heap snapshots.
///
/// Computed by comparing a "before" and "after" `HeapStats` via
/// [`HeapStats::diff`]. Positive deltas mean growth (more objects, more memory),
/// negative means shrinkage. This is useful for understanding what a code
/// snippet allocated, freed, or changed on the heap.
///
/// Only types present in at least one of the two snapshots appear in
/// `objects_by_type_delta`. Types exclusive to the "after" snapshot are
/// listed in `new_types`; types exclusive to the "before" snapshot are in
/// `removed_types`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapDiff {
    /// Change in live object count (`after - before`).
    pub live_objects_delta: isize,
    /// Change in free slot count.
    pub free_slots_delta: isize,
    /// Change in total slot count.
    pub total_slots_delta: isize,
    /// Per-type deltas. Only includes types present in either snapshot.
    /// Positive means more objects of this type, negative means fewer.
    pub objects_by_type_delta: BTreeMap<&'static str, isize>,
    /// Types that appeared in "after" but not "before".
    pub new_types: Vec<&'static str>,
    /// Types that appeared in "before" but not "after".
    pub removed_types: Vec<&'static str>,
    /// Change in interned string count.
    pub interned_strings_delta: isize,
    /// Change in tracker allocations (only if both snapshots have the value).
    pub tracker_allocations_delta: Option<isize>,
    /// Change in tracker memory bytes (only if both snapshots have the value).
    pub tracker_memory_bytes_delta: Option<isize>,
}

impl HeapStats {
    /// Computes the difference between `self` ("before") and `other` ("after").
    ///
    /// Returns a [`HeapDiff`] where positive deltas indicate growth from
    /// `self` to `other`, and negative deltas indicate shrinkage. For tracker
    /// fields, a delta is computed only when both snapshots contain `Some`.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::collections::BTreeMap;
    /// # use ouros::HeapStats;
    /// let before = HeapStats {
    ///     live_objects: 2, free_slots: 0, total_slots: 2,
    ///     objects_by_type: BTreeMap::new(), interned_strings: 0,
    ///     tracker_allocations: None, tracker_memory_bytes: None,
    /// };
    /// let after = HeapStats {
    ///     live_objects: 5, free_slots: 1, total_slots: 6,
    ///     objects_by_type: BTreeMap::new(), interned_strings: 0,
    ///     tracker_allocations: None, tracker_memory_bytes: None,
    /// };
    /// let diff = before.diff(&after);
    /// assert_eq!(diff.live_objects_delta, 3);
    /// ```
    #[must_use]
    pub fn diff(&self, other: &Self) -> HeapDiff {
        let live_objects_delta = isize_delta(self.live_objects, other.live_objects);
        let free_slots_delta = isize_delta(self.free_slots, other.free_slots);
        let total_slots_delta = isize_delta(self.total_slots, other.total_slots);
        let interned_strings_delta = isize_delta(self.interned_strings, other.interned_strings);

        let (objects_by_type_delta, new_types, removed_types) =
            compute_type_deltas(&self.objects_by_type, &other.objects_by_type);

        let tracker_allocations_delta = optional_isize_delta(self.tracker_allocations, other.tracker_allocations);
        let tracker_memory_bytes_delta = optional_isize_delta(self.tracker_memory_bytes, other.tracker_memory_bytes);

        HeapDiff {
            live_objects_delta,
            free_slots_delta,
            total_slots_delta,
            objects_by_type_delta,
            new_types,
            removed_types,
            interned_strings_delta,
            tracker_allocations_delta,
            tracker_memory_bytes_delta,
        }
    }
}

impl HeapDiff {
    /// Returns `true` when all deltas are zero and no types were added or removed.
    ///
    /// Useful for quickly checking whether any meaningful change occurred
    /// between two heap snapshots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live_objects_delta == 0
            && self.free_slots_delta == 0
            && self.total_slots_delta == 0
            && self.interned_strings_delta == 0
            && self.new_types.is_empty()
            && self.removed_types.is_empty()
            && self.objects_by_type_delta.values().all(|&v| v == 0)
            && self.tracker_allocations_delta.is_none_or(|d| d == 0)
            && self.tracker_memory_bytes_delta.is_none_or(|d| d == 0)
    }
}

impl std::fmt::Display for HeapDiff {
    /// Produces a human-readable summary of what changed between two heap
    /// snapshots. Example output:
    ///
    /// ```text
    /// HeapDiff: +3 live objects, +4 slots
    ///   List: +1
    ///   Str: +2
    ///   New types: Dict
    /// ```
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "HeapDiff: no changes");
        }

        write!(
            f,
            "HeapDiff: {:+} live objects, {:+} slots",
            self.live_objects_delta, self.total_slots_delta
        )?;

        // Per-type deltas (skip zero deltas for conciseness).
        for (&type_name, &delta) in &self.objects_by_type_delta {
            if delta != 0 {
                write!(f, "\n  {type_name}: {delta:+}")?;
            }
        }

        if !self.new_types.is_empty() {
            write!(f, "\n  New types: {}", self.new_types.join(", "))?;
        }
        if !self.removed_types.is_empty() {
            write!(f, "\n  Removed types: {}", self.removed_types.join(", "))?;
        }

        if self.interned_strings_delta != 0 {
            write!(f, "\n  Interned strings: {:+}", self.interned_strings_delta)?;
        }

        if let Some(alloc_delta) = self.tracker_allocations_delta
            && alloc_delta != 0
        {
            write!(f, "\n  Tracker allocations: {alloc_delta:+}")?;
        }
        if let Some(mem_delta) = self.tracker_memory_bytes_delta
            && mem_delta != 0
        {
            write!(f, "\n  Tracker memory: {mem_delta:+} bytes")?;
        }

        Ok(())
    }
}

/// Computes `after - before` as `isize`, handling the `usize -> isize` conversion.
fn isize_delta(before: usize, after: usize) -> isize {
    (after as isize).wrapping_sub(before as isize)
}

/// Computes the delta between two optional `usize` values.
///
/// Returns `Some(delta)` only when both values are `Some`.
fn optional_isize_delta(before: Option<usize>, after: Option<usize>) -> Option<isize> {
    match (before, after) {
        (Some(b), Some(a)) => Some(isize_delta(b, a)),
        _ => None,
    }
}

/// Computes per-type deltas, plus lists of new and removed types.
///
/// Iterates the union of keys from both `BTreeMap`s, producing a delta for
/// each type name. Types only in `after` are "new"; types only in `before`
/// are "removed".
fn compute_type_deltas(
    before: &BTreeMap<&'static str, usize>,
    after: &BTreeMap<&'static str, usize>,
) -> (BTreeMap<&'static str, isize>, Vec<&'static str>, Vec<&'static str>) {
    let mut deltas = BTreeMap::new();
    let mut new_types = Vec::new();
    let mut removed_types = Vec::new();

    // Process all keys from "before".
    for (&type_name, &count) in before {
        let after_count = after.get(type_name).copied().unwrap_or(0);
        deltas.insert(type_name, isize_delta(count, after_count));
        if !after.contains_key(type_name) {
            removed_types.push(type_name);
        }
    }

    // Process keys only in "after" (not already seen).
    for (&type_name, &count) in after {
        if !before.contains_key(type_name) {
            deltas.insert(type_name, count as isize);
            new_types.push(type_name);
        }
    }

    (deltas, new_types, removed_types)
}

/// Unique identifier for values stored inside the heap arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HeapId(usize);

impl HeapId {
    /// Returns the raw index value.
    #[inline]
    pub fn index(self) -> usize {
        self.0
    }
}

/// Implementation of `object.__new__(cls)` for creating new instances.
///
/// When called as `cls.__new__(cls)` in a classmethod, this creates a new
/// bare instance of the given class without calling `__init__`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ObjectNewImpl;

impl ObjectNewImpl {
    /// Calls `object.__new__(cls)` to create a new instance.
    ///
    /// Expects the first argument to be the class to instantiate.
    pub fn call(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
    ) -> crate::exception_private::RunResult<Value> {
        use crate::exception_private::ExcType;

        // Get the first positional argument (the class)
        let (mut pos, kwargs) = args.into_parts();
        let cls = pos.next();

        // Drop remaining args
        for v in pos {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);

        let Some(cls) = cls else {
            return Err(ExcType::type_error(
                "object.__new__(): not enough arguments".to_string(),
            ));
        };

        // Check that the first argument is a class
        let class_id = match &cls {
            Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
            _ => {
                cls.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "object.__new__(X): X is not a type object".to_string(),
                ));
            }
        };

        // Create a bare instance of the class
        let instance = Self::allocate_bare_instance(heap, class_id)?;
        cls.drop_with_heap(heap);
        Ok(instance)
    }

    /// Allocates a bare instance of the given class without calling `__init__`.
    fn allocate_bare_instance(
        heap: &mut Heap<impl ResourceTracker>,
        class_id: HeapId,
    ) -> crate::exception_private::RunResult<Value> {
        use crate::exception_private::ExcType;

        // Get class info
        let (instance_has_dict, slot_len) = match heap.get(class_id) {
            HeapData::ClassObject(cls) => (cls.instance_has_dict(), cls.slot_layout().len()),
            _ => {
                return Err(ExcType::type_error(
                    "object.__new__(X): X is not a type object".to_string(),
                ));
            }
        };

        heap.inc_ref(class_id);
        let attrs_id = if instance_has_dict {
            Some(heap.allocate(HeapData::Dict(Dict::new()))?)
        } else {
            None
        };
        let mut slot_values = Vec::with_capacity(slot_len);
        slot_values.resize_with(slot_len, || Value::Undefined);
        let weakref_ids = Vec::new();
        let instance = Instance::new(class_id, attrs_id, slot_values, weakref_ids);
        let instance_heap_id = heap.allocate(HeapData::Instance(instance))?;
        Ok(Value::Ref(instance_heap_id))
    }
}

/// HeapData captures every runtime value that must live in the arena.
///
/// Each variant wraps a type that implements `AbstractValue`, providing
/// Python-compatible operations. The trait is manually implemented to dispatch
/// to the appropriate variant's implementation.
///
/// Note: The `Value` variant is special - it wraps boxed immediate values
/// that need heap identity (e.g., when `id()` is called on an int).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum HeapData {
    Str(Str),
    Bytes(Bytes),
    Bytearray(Bytes),
    List(List),
    Tuple(Tuple),
    NamedTuple(NamedTuple),
    /// Factory callable returned by `collections.namedtuple`.
    NamedTupleFactory(crate::types::NamedTupleFactory),
    Dict(Dict),
    /// A `collections.Counter` mapping of element counts.
    Counter(crate::types::Counter),
    /// An ordered dictionary preserving insertion order.
    OrderedDict(crate::types::OrderedDict),
    Deque(Deque),
    DefaultDict(DefaultDict),
    ChainMap(ChainMap),
    Set(Set),
    FrozenSet(FrozenSet),
    /// A closure: a function that captures variables from enclosing scopes.
    ///
    /// Contains a reference to the function definition, a vector of captured cell HeapIds,
    /// and evaluated default values (if any). When the closure is called, these cells are
    /// passed to the RunFrame for variable access. When the closure is dropped, we must
    /// decrement the ref count on each captured cell and each default value.
    Closure(FunctionId, Vec<HeapId>, Vec<Value>),
    /// A function with evaluated default parameter values (non-closure).
    ///
    /// Contains a reference to the function definition and the evaluated default values.
    /// When the function is called, defaults are cloned for missing optional parameters.
    /// When dropped, we must decrement the ref count on each default value.
    FunctionDefaults(FunctionId, Vec<Value>),
    /// A cell wrapping a single mutable value for closure support.
    ///
    /// Cells enable nonlocal variable access by providing a heap-allocated
    /// container that can be shared between a function and its nested functions.
    /// Both the outer function and inner function hold references to the same
    /// cell, allowing modifications to propagate across scope boundaries.
    Cell(Value),
    /// A range object (e.g., `range(10)` or `range(1, 10, 2)`).
    ///
    /// Stored on the heap to keep `Value` enum small (16 bytes). Range objects
    /// are immutable and hashable.
    Range(Range),
    /// A slice object (e.g., `slice(1, 10, 2)` or from `x[1:10:2]`).
    ///
    /// Stored on the heap to keep `Value` enum small. Slice objects represent
    /// start:stop:step indices for sequence slicing operations.
    Slice(Slice),
    /// An exception instance (e.g., `ValueError('message')`).
    ///
    /// Stored on the heap to keep `Value` enum small (16 bytes). Exceptions
    /// are created when exception types are called or when `raise` is executed.
    Exception(SimpleException),
    /// A dataclass instance with fields and method references.
    ///
    /// Contains a class name, a Dict of field name -> value mappings, and a set
    /// of method names that trigger external function calls when invoked.
    Dataclass(Dataclass),
    /// An iterator for for-loop iteration and the `iter()` type constructor.
    ///
    /// Created by the `GetIter` opcode or `iter()` builtin, advanced by `ForIter`.
    /// Stores iteration state for lists, tuples, strings, ranges, dicts, and sets.
    Iter(OurosIter),
    /// Shared state for `itertools.tee` iterators.
    ///
    /// Stores the source iterator and buffer for tee clones.
    Tee(TeeState),
    /// An arbitrary precision integer (LongInt).
    ///
    /// Stored on the heap to keep `Value` enum at 16 bytes. Python has one `int` type,
    /// so LongInt is an implementation detail - we use `Value::Int(i64)` for performance
    /// when values fit, and promote to LongInt on overflow. When LongInt results fit back
    /// in i64, they are demoted back to `Value::Int` for performance.
    LongInt(LongInt),
    /// A Python module (e.g., `sys`, `typing`).
    ///
    /// Modules have a name and a dictionary of attributes. They are created by
    /// import statements and can have refs to other heap values in their attributes.
    Module(Module),
    /// A coroutine object from an async function call.
    ///
    /// Contains pre-bound arguments and captured cells, ready to be awaited.
    /// When awaited, a new frame is pushed using the stored namespace.
    Coroutine(Coroutine),
    /// A gather() result tracking multiple coroutines/tasks.
    ///
    /// Created by asyncio.gather() and spawns tasks when awaited.
    GatherFuture(GatherFuture),
    /// A filesystem path from `pathlib.Path`.
    ///
    /// Stored on the heap to provide Python-compatible path operations.
    /// Pure methods (name, parent, etc.) are handled directly by the VM.
    /// I/O methods (exists, read_text, etc.) yield external function calls.
    Path(Path),
    /// A hash object from `hashlib` (e.g., `hashlib.sha256()`).
    ///
    /// Stores the internal hash state and provides methods like `.hexdigest()`,
    /// `.digest()`, and `.update()` for incremental hashing.
    Hash(crate::modules::hashlib::HashObject),
    /// A `zlib.compressobj` streaming compressor state wrapper.
    ZlibCompress(crate::modules::zlib_mod::ZlibCompressObject),
    /// A `zlib.decompressobj` streaming decompressor state wrapper.
    ZlibDecompress(crate::modules::zlib_mod::ZlibDecompressObject),
    /// A Python class object created by a `class` statement.
    ///
    /// Contains the class name and a namespace dict with class attributes and methods.
    /// When called, creates an Instance and invokes `__init__` if defined.
    ClassObject(crate::types::ClassObject),
    /// A read-only mapping proxy for a class namespace (`type.__dict__`).
    MappingProxy(MappingProxy),
    /// A runtime generic alias (e.g., `C[int]`).
    GenericAlias(GenericAlias),
    /// A built-in descriptor created for `__slots__` entries.
    SlotDescriptor(SlotDescriptor),
    /// A weak reference created by `weakref.ref`.
    WeakRef(WeakRef),
    /// A Python class instance created by calling a ClassObject.
    ///
    /// Contains a reference to the class (HeapId) and instance-specific attributes.
    /// Attribute lookup checks instance attrs first, then class attrs.
    Instance(crate::types::Instance),
    /// A bound method object (function + bound self/cls).
    BoundMethod(crate::types::BoundMethod),
    /// A super() proxy for MRO-based attribute delegation.
    ///
    /// When `super()` is called inside a method, this proxy is created. Attribute
    /// access on it starts searching from the next class after `current_class_id`
    /// in the instance's MRO.
    SuperProxy(crate::types::SuperProxy),
    /// A `@staticmethod` wrapper - function that doesn't receive `self` or `cls`.
    StaticMethod(crate::types::StaticMethod),
    /// A `@classmethod` wrapper - function that receives the class as first argument.
    ClassMethod(crate::types::ClassMethod),
    /// A `@property` descriptor with getter/setter/deleter.
    UserProperty(crate::types::UserProperty),
    /// A callable returned by `property.setter`/`property.deleter` for chaining.
    PropertyAccessor(crate::types::PropertyAccessor),
    /// Built-in bound `__subclasses__` method for classes.
    ClassSubclasses(ClassSubclasses),
    /// Built-in default `__class_getitem__` for generic classes.
    ClassGetItem(ClassGetItem),
    /// Built-in bound `function.__get__` descriptor wrapper.
    FunctionGet(FunctionGet),
    /// A `functools.partial` object with pre-applied args.
    ///
    /// When called, the VM prepends the stored args and merges stored kwargs
    /// before forwarding to the wrapped function.
    Partial(Partial),
    /// A `functools.cmp_to_key` wrapper that adapts a comparison function into a key function.
    CmpToKey(CmpToKey),
    /// An `operator.itemgetter` callable that indexes its argument.
    ItemGetter(ItemGetter),
    /// An `operator.attrgetter` callable that retrieves attributes from its argument.
    AttrGetter(AttrGetter),
    /// An `operator.methodcaller` callable that invokes a method on its argument.
    MethodCaller(MethodCaller),
    /// A `functools.lru_cache` decorator or cache wrapper.
    LruCache(crate::types::LruCache),
    /// A `functools.update_wrapper` result that exposes wrapped metadata.
    FunctionWrapper(crate::types::FunctionWrapper),
    /// A `functools.wraps` decorator factory.
    Wraps(crate::types::Wraps),
    /// Generated ordering method from `functools.total_ordering`.
    TotalOrderingMethod(crate::types::TotalOrderingMethod),
    /// A `functools.cached_property` descriptor.
    CachedProperty(CachedProperty),
    /// A `functools.singledispatch` callable.
    SingleDispatch(SingleDispatch),
    /// Decorator wrapper returned by `singledispatch.register(cls)`.
    SingleDispatchRegister(SingleDispatchRegister),
    /// A `functools.singledispatchmethod` descriptor.
    SingleDispatchMethod(SingleDispatchMethod),
    /// A `functools.partialmethod` descriptor.
    PartialMethod(PartialMethod),
    /// Singleton placeholder sentinel used by `functools.partial`.
    Placeholder(Placeholder),
    /// A `textwrap.TextWrapper` runtime object.
    TextWrapper(TextWrapper),
    /// A `re.Match` object from a successful regex match.
    ///
    /// Stores the matched text, start/end positions, and captured groups.
    /// No heap references — only plain Rust data.
    ReMatch(ReMatch),
    /// A `re.Pattern` object from `re.compile()`.
    ///
    /// Stores the pattern string and flags for reuse across multiple operations.
    /// No heap references — only plain Rust data.
    RePattern(RePattern),
    /// Lightweight object used for stdlib class parity shims.
    StdlibObject(StdlibObject),
    /// A generator object from a generator function call.
    ///
    /// Contains pre-bound arguments and captured cells, ready to be iterated.
    /// When `__next__()` is called, a new frame is pushed using the stored namespace.
    Generator(Generator),
    /// A timedelta object from `datetime.timedelta`.
    ///
    /// Represents a duration, the difference between two datetime values.
    Timedelta(crate::types::Timedelta),
    /// A date object from `datetime.date`.
    ///
    /// Represents a date (year, month, day) in the Gregorian calendar.
    Date(crate::types::Date),
    /// A datetime object from `datetime.datetime`.
    ///
    /// Represents a date and time combination.
    Datetime(crate::types::Datetime),
    /// A time object from `datetime.time`.
    ///
    /// Represents a time of day (hour, minute, second, microsecond, tzinfo).
    Time(crate::types::Time),
    /// A timezone object from `datetime.timezone`.
    ///
    /// Represents a fixed UTC offset for timezone-aware datetimes.
    Timezone(crate::types::Timezone),
    /// A decimal number from the `decimal` module.
    ///
    /// Provides arbitrary precision decimal arithmetic.
    Decimal(Decimal),
    /// A rational number fraction from `fractions.Fraction`.
    ///
    /// Stores arbitrary precision numerator and denominator.
    Fraction(Fraction),
    /// A UUID value from `uuid.UUID`.
    Uuid(Uuid),
    /// A `uuid.SafeUUID` enum-member value.
    SafeUuid(SafeUuid),
    /// Implementation of `object.__new__(cls)` for creating new instances.
    ///
    /// This is stored on the heap so it can be wrapped in a StaticMethod and added
    /// to the object class's namespace, making `cls.__new__(cls)` work in classmethods.
    ObjectNewImpl(ObjectNewImpl),
    /// A dynamic view of a dict's keys.
    DictKeys(DictKeys),
    /// A dynamic view of a dict's values.
    DictValues(DictValues),
    /// A dynamic view of a dict's (key, value) pairs.
    DictItems(DictItems),
}

impl HeapData {
    /// Returns the variant name as a static string slice.
    ///
    /// Used by `HeapStats` for per-type object breakdowns and by ref-count
    /// debugging output. Uses a match statement (not `std::any::type_name`)
    /// to return stable, human-readable names for each variant.
    fn variant_name(&self) -> &'static str {
        match self {
            Self::Str(_) => "Str",
            Self::Bytes(_) => "Bytes",
            Self::Bytearray(_) => "Bytearray",
            Self::List(_) => "List",
            Self::Tuple(_) => "Tuple",
            Self::NamedTuple(_) => "NamedTuple",
            Self::NamedTupleFactory(_) => "NamedTupleFactory",
            Self::Dict(_) => "Dict",
            Self::Counter(_) => "Counter",
            Self::OrderedDict(_) => "OrderedDict",
            Self::Deque(_) => "Deque",
            Self::DefaultDict(_) => "DefaultDict",
            Self::ChainMap(_) => "ChainMap",
            Self::Set(_) => "Set",
            Self::FrozenSet(_) => "FrozenSet",
            Self::Closure(..) => "Closure",
            Self::FunctionDefaults(..) => "FunctionDefaults",
            Self::Cell(_) => "Cell",
            Self::Range(_) => "Range",
            Self::Slice(_) => "Slice",
            Self::Exception(_) => "Exception",
            Self::Dataclass(_) => "Dataclass",
            Self::Iter(_) => "Iter",
            Self::Tee(_) => "Tee",
            Self::Module(_) => "Module",
            Self::LongInt(_) => "LongInt",
            Self::Path(_) => "Path",
            Self::Coroutine(_) => "Coroutine",
            Self::GatherFuture(_) => "GatherFuture",
            Self::ClassObject(_) => "ClassObject",
            Self::MappingProxy(_) => "MappingProxy",
            Self::GenericAlias(_) => "GenericAlias",
            Self::SlotDescriptor(_) => "SlotDescriptor",
            Self::WeakRef(_) => "WeakRef",
            Self::Instance(_) => "Instance",
            Self::BoundMethod(_) => "BoundMethod",
            Self::SuperProxy(_) => "SuperProxy",
            Self::StaticMethod(_) => "StaticMethod",
            Self::ClassMethod(_) => "ClassMethod",
            Self::UserProperty(_) => "UserProperty",
            Self::PropertyAccessor(_) => "PropertyAccessor",
            Self::ClassSubclasses(_) => "ClassSubclasses",
            Self::ClassGetItem(_) => "ClassGetItem",
            Self::FunctionGet(_) => "FunctionGet",
            Self::Hash(_) => "Hash",
            Self::ZlibCompress(_) => "ZlibCompress",
            Self::ZlibDecompress(_) => "ZlibDecompress",
            Self::Partial(_) => "Partial",
            Self::CmpToKey(_) => "CmpToKey",
            Self::ItemGetter(_) => "ItemGetter",
            Self::AttrGetter(_) => "AttrGetter",
            Self::MethodCaller(_) => "MethodCaller",
            Self::LruCache(_) => "LruCache",
            Self::FunctionWrapper(_) => "FunctionWrapper",
            Self::Wraps(_) => "Wraps",
            Self::TotalOrderingMethod(_) => "TotalOrderingMethod",
            Self::CachedProperty(_) => "CachedProperty",
            Self::SingleDispatch(_) => "SingleDispatch",
            Self::SingleDispatchRegister(_) => "SingleDispatchRegister",
            Self::SingleDispatchMethod(_) => "SingleDispatchMethod",
            Self::PartialMethod(_) => "PartialMethod",
            Self::Placeholder(_) => "Placeholder",
            Self::TextWrapper(_) => "TextWrapper",
            Self::ReMatch(_) => "ReMatch",
            Self::RePattern(_) => "RePattern",
            Self::StdlibObject(_) => "StdlibObject",
            Self::Generator(_) => "Generator",
            Self::Timedelta(_) => "Timedelta",
            Self::Date(_) => "Date",
            Self::Datetime(_) => "Datetime",
            Self::Time(_) => "Time",
            Self::Timezone(_) => "Timezone",
            Self::Decimal(_) => "Decimal",
            Self::Fraction(_) => "Fraction",
            Self::Uuid(_) => "Uuid",
            Self::SafeUuid(_) => "SafeUuid",
            Self::ObjectNewImpl(_) => "ObjectNewImpl",
            Self::DictKeys(_) => "DictKeys",
            Self::DictValues(_) => "DictValues",
            Self::DictItems(_) => "DictItems",
        }
    }

    /// Returns whether this heap data type can participate in reference cycles.
    ///
    /// Only container types that can hold references to other heap objects need to be
    /// tracked for GC purposes. Leaf types like Str, Bytes, Range, and Exception cannot
    /// form cycles and should not count toward the GC allocation threshold.
    ///
    /// This optimization allows programs that allocate many leaf objects (like strings)
    /// to avoid triggering unnecessary GC cycles.
    #[inline]
    pub fn is_gc_tracked(&self) -> bool {
        matches!(
            self,
            Self::List(_)
                | Self::Tuple(_)
                | Self::NamedTuple(_)
                | Self::NamedTupleFactory(_)
                | Self::Dict(_)
                | Self::Deque(_)
                | Self::DefaultDict(_)
                | Self::ChainMap(_)
                | Self::Counter(_)
                | Self::OrderedDict(_)
                | Self::Set(_)
                | Self::FrozenSet(_)
                | Self::Closure(_, _, _)
                | Self::FunctionDefaults(_, _)
                | Self::Cell(_)
                | Self::Dataclass(_)
                | Self::Iter(_)
                | Self::Tee(_)
                | Self::Module(_)
                | Self::Coroutine(_)
                | Self::GatherFuture(_)
                | Self::ClassObject(_)
                | Self::MappingProxy(_)
                | Self::Instance(_)
                | Self::BoundMethod(_)
                | Self::SuperProxy(_)
                | Self::StaticMethod(_)
                | Self::ClassMethod(_)
                | Self::UserProperty(_)
                | Self::PropertyAccessor(_)
                | Self::GenericAlias(_)
                | Self::ClassSubclasses(_)
                | Self::ClassGetItem(_)
                | Self::FunctionGet(_)
                | Self::Partial(_)
                | Self::CmpToKey(_)
                | Self::ItemGetter(_)
                | Self::AttrGetter(_)
                | Self::MethodCaller(_)
                | Self::LruCache(_)
                | Self::FunctionWrapper(_)
                | Self::Wraps(_)
                | Self::TotalOrderingMethod(_)
                | Self::CachedProperty(_)
                | Self::SingleDispatch(_)
                | Self::SingleDispatchRegister(_)
                | Self::SingleDispatchMethod(_)
                | Self::PartialMethod(_)
                | Self::TextWrapper(_)
                | Self::ReMatch(_)
                | Self::RePattern(_)
                | Self::StdlibObject(_)
                | Self::Generator(_)
        )
    }

    /// Returns whether this heap data currently contains any heap references (`Value::Ref`).
    ///
    /// Used during allocation to determine if this data could create reference cycles.
    /// When true, `mark_potential_cycle()` should be called to enable GC.
    ///
    /// Note: This is separate from `is_gc_tracked()` - a container may be GC-tracked
    /// (capable of holding refs) but not currently contain any refs.
    #[inline]
    pub fn has_refs(&self) -> bool {
        match self {
            // Bytearray is like Bytes - no refs
            Self::Bytearray(_) => false,
            Self::List(list) => list.contains_refs(),
            Self::Tuple(tuple) => tuple.contains_refs(),
            Self::NamedTuple(nt) => nt.contains_refs(),
            Self::NamedTupleFactory(factory) => factory.has_refs(),
            Self::Dict(dict) => dict.has_refs(),
            Self::Counter(counter) => counter.dict().has_refs(),
            Self::OrderedDict(ordered) => ordered.dict().has_refs(),
            Self::Deque(deque) => deque.contains_refs(),
            Self::DefaultDict(dd) => dd.has_refs(),
            Self::ChainMap(chain) => chain.has_refs(),
            Self::Set(set) => set.has_refs(),
            Self::FrozenSet(fset) => fset.has_refs(),
            // Closures always have refs when they have captured cells (HeapIds)
            Self::Closure(_, cells, defaults) => {
                !cells.is_empty() || defaults.iter().any(|v| matches!(v, Value::Ref(_)))
            }
            Self::FunctionDefaults(_, defaults) => defaults.iter().any(|v| matches!(v, Value::Ref(_))),
            Self::Cell(value) => matches!(value, Value::Ref(_)),
            Self::Dataclass(dc) => dc.has_refs(),
            Self::Iter(iter) => iter.has_refs(),
            Self::Tee(tee) => tee.has_refs(),
            Self::Module(m) => m.has_refs(),
            // Coroutines always have refs (namespace values, frame_cells)
            Self::Coroutine(coro) => {
                !coro.frame_cells.is_empty() || coro.namespace.iter().any(|v| matches!(v, Value::Ref(_)))
            }
            // GatherFutures have refs from coroutine items and results
            Self::GatherFuture(gather) => {
                gather
                    .items
                    .iter()
                    .any(|item| matches!(item, crate::asyncio::GatherItem::Coroutine(_)))
                    || gather
                        .results
                        .iter()
                        .any(|r| r.as_ref().is_some_and(|v| matches!(v, Value::Ref(_))))
            }
            Self::ClassObject(cls) => cls.has_refs(),
            Self::MappingProxy(mp) => mp.has_refs(),
            Self::Instance(inst) => inst.has_refs(),
            Self::BoundMethod(bm) => bm.has_refs(),
            Self::SuperProxy(sp) => sp.has_refs(),
            Self::StaticMethod(sm) => sm.has_refs(),
            Self::ClassMethod(cm) => cm.has_refs(),
            Self::UserProperty(up) => up.has_refs(),
            Self::PropertyAccessor(pa) => pa.has_refs(),
            Self::GenericAlias(ga) => {
                matches!(ga.origin(), Value::Ref(_))
                    || ga.args().iter().any(|v| matches!(v, Value::Ref(_)))
                    || ga.parameters().iter().any(|v| matches!(v, Value::Ref(_)))
            }
            Self::ClassSubclasses(_) => true,
            Self::ClassGetItem(_) => true,
            Self::FunctionGet(fg) => matches!(fg.func(), Value::Ref(_)),
            Self::Partial(p) => {
                matches!(p.func(), Value::Ref(_))
                    || p.args().iter().any(|v| matches!(v, Value::Ref(_)))
                    || p.kwargs()
                        .iter()
                        .any(|(k, v)| matches!(k, Value::Ref(_)) || matches!(v, Value::Ref(_)))
            }
            Self::CmpToKey(c) => matches!(c.func(), Value::Ref(_)),
            Self::ItemGetter(g) => g.items().iter().any(|v| matches!(v, Value::Ref(_))),
            Self::AttrGetter(g) => g.attrs().iter().any(|v| matches!(v, Value::Ref(_))),
            Self::MethodCaller(mc) => {
                matches!(mc.name(), Value::Ref(_))
                    || mc.args().iter().any(|v| matches!(v, Value::Ref(_)))
                    || mc
                        .kwargs()
                        .iter()
                        .any(|(k, v)| matches!(k, Value::Ref(_)) || matches!(v, Value::Ref(_)))
            }
            Self::LruCache(lru) => lru.has_refs(),
            Self::StdlibObject(obj) => obj.has_refs(),
            Self::FunctionWrapper(fw) => {
                matches!(fw.wrapper, Value::Ref(_))
                    || matches!(fw.wrapped, Value::Ref(_))
                    || matches!(fw.name, Value::Ref(_))
                    || matches!(fw.module, Value::Ref(_))
                    || matches!(fw.qualname, Value::Ref(_))
                    || matches!(fw.doc, Value::Ref(_))
            }
            Self::Wraps(wraps) => matches!(wraps.wrapped, Value::Ref(_)),
            Self::TotalOrderingMethod(_) => false,
            Self::CachedProperty(cached) => matches!(cached.func, Value::Ref(_)),
            Self::SingleDispatch(dispatcher) => {
                matches!(dispatcher.func, Value::Ref(_))
                    || dispatcher
                        .registry
                        .iter()
                        .any(|(cls, func)| matches!(cls, Value::Ref(_)) || matches!(func, Value::Ref(_)))
            }
            Self::SingleDispatchRegister(reg) => {
                matches!(reg.dispatcher, Value::Ref(_)) || matches!(reg.cls, Value::Ref(_))
            }
            Self::SingleDispatchMethod(method) => matches!(method.dispatcher, Value::Ref(_)),
            Self::PartialMethod(method) => {
                matches!(method.func, Value::Ref(_))
                    || method.args.iter().any(|v| matches!(v, Value::Ref(_)))
                    || method
                        .kwargs
                        .iter()
                        .any(|(k, v)| matches!(k, Value::Ref(_)) || matches!(v, Value::Ref(_)))
            }
            // Generators have refs from namespace values, saved_stack, and frame_cells
            Self::Generator(generator) => {
                !generator.frame_cells.is_empty()
                    || generator.namespace.iter().any(|v| matches!(v, Value::Ref(_)))
                    || generator.saved_stack.iter().any(|v| matches!(v, Value::Ref(_)))
            }
            // Leaf types cannot have refs
            Self::Str(_)
            | Self::Bytes(_)
            | Self::Range(_)
            | Self::Slice(_)
            | Self::Exception(_)
            | Self::LongInt(_)
            | Self::Path(_)
            | Self::SlotDescriptor(_)
            | Self::WeakRef(_)
            | Self::Hash(_)
            | Self::ZlibCompress(_)
            | Self::ZlibDecompress(_)
            | Self::Placeholder(_)
            | Self::TextWrapper(_)
            | Self::ReMatch(_)
            | Self::RePattern(_)
            | Self::Fraction(_)
            | Self::Uuid(_)
            | Self::SafeUuid(_)
            | Self::ObjectNewImpl(_) => false,
            // datetime types have no refs (Time has tzinfo but it's handled elsewhere)
            Self::Timedelta(_) => false,
            Self::Date(_) => false,
            Self::Datetime(dt) => dt.tzinfo().is_some(),
            Self::Time(t) => t.tzinfo().is_some(),
            Self::Timezone(_) => false,
            Self::Decimal(_) => false,
            // Dict views always have refs to their source dict
            Self::DictKeys(_) | Self::DictValues(_) | Self::DictItems(_) => true,
        }
    }

    /// Returns true if this heap data is a coroutine.
    #[inline]
    pub fn is_coroutine(&self) -> bool {
        matches!(self, Self::Coroutine(_))
    }

    /// Computes hash for immutable heap types that can be used as dict keys.
    ///
    /// Returns Some(hash) for immutable types (Str, Bytes, Tuple of hashables).
    /// Returns None for mutable types (List, Dict) which cannot be dict keys.
    ///
    /// This is called lazily when the value is first used as a dict key,
    /// avoiding unnecessary hash computation for values that are never used as keys.
    fn compute_hash_if_immutable(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Option<u64> {
        match self {
            // Hash just the actual string or bytes content for consistency with Value::InternString/InternBytes
            // hence we don't include the discriminant
            Self::Str(s) => Some(cpython_hash_str_seed0(s.as_str())),
            Self::Bytes(b) => Some(cpython_hash_bytes_seed0(b.as_slice())),
            // Bytearray is hashable (same as bytes)
            Self::Bytearray(b) => Some(cpython_hash_bytes_seed0(b.as_slice())),
            // datetime types are hashable - compute hash inline
            Self::Timedelta(td) => {
                let mut hasher = DefaultHasher::new();
                td.as_microseconds().hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::Date(d) => {
                let mut hasher = DefaultHasher::new();
                (d.year(), d.month(), d.day()).hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::Datetime(dt) => {
                let mut hasher = DefaultHasher::new();
                (
                    dt.year(),
                    dt.month(),
                    dt.day(),
                    dt.hour(),
                    dt.minute(),
                    dt.second(),
                    dt.microsecond(),
                )
                    .hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::Time(t) => {
                let mut hasher = DefaultHasher::new();
                (t.hour(), t.minute(), t.second(), t.microsecond()).hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::Timezone(tz) => {
                let mut hasher = DefaultHasher::new();
                tz.utcoffset().as_microseconds().hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::FrozenSet(fs) => {
                // FrozenSet hash is XOR of element hashes (order-independent)
                fs.compute_hash(heap, interns)
            }
            Self::Tuple(t) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                // Tuple is hashable only if all elements are hashable
                for obj in t.as_vec() {
                    let h = obj.py_hash(heap, interns)?;
                    h.hash(&mut hasher);
                }
                Some(hasher.finish())
            }
            Self::NamedTuple(nt) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                // Hash only by elements (not type_name) to match equality semantics
                for obj in nt.as_vec() {
                    let h = obj.py_hash(heap, interns)?;
                    h.hash(&mut hasher);
                }
                Some(hasher.finish())
            }
            Self::GenericAlias(ga) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                let origin_hash = ga.origin().py_hash(heap, interns)?;
                origin_hash.hash(&mut hasher);
                for obj in ga.args() {
                    let h = obj.py_hash(heap, interns)?;
                    h.hash(&mut hasher);
                }
                for obj in ga.parameters() {
                    let h = obj.py_hash(heap, interns)?;
                    h.hash(&mut hasher);
                }
                Some(hasher.finish())
            }
            Self::Closure(f, _, _) | Self::FunctionDefaults(f, _) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                // TODO, this is NOT proper hashing, we should somehow hash the function properly
                f.hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::Range(range) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                range.start.hash(&mut hasher);
                range.stop.hash(&mut hasher);
                range.step.hash(&mut hasher);
                Some(hasher.finish())
            }
            // Dataclass hashability depends on the mutable flag
            Self::Dataclass(dc) => dc.compute_hash(heap, interns),
            // Slices are immutable and hashable (like in CPython)
            Self::Slice(slice) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                slice.start.hash(&mut hasher);
                slice.stop.hash(&mut hasher);
                slice.step.hash(&mut hasher);
                Some(hasher.finish())
            }
            // Path is immutable and hashable
            Self::Path(path) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                path.hash_key().hash(&mut hasher);
                Some(hasher.finish())
            }
            // Fraction is immutable and hashable
            Self::Fraction(f) => {
                if f.denominator() == &num_bigint::BigInt::from(1) {
                    Some(LongInt::new(f.numerator().clone()).hash())
                } else {
                    let mut hasher = DefaultHasher::new();
                    discriminant(self).hash(&mut hasher);
                    f.numerator().hash(&mut hasher);
                    f.denominator().hash(&mut hasher);
                    Some(hasher.finish())
                }
            }
            // UUID values are immutable and hash as hash(uuid.int), matching CPython.
            Self::Uuid(uuid) => Some(uuid.python_hash()),
            // SafeUUID enum members are immutable and hash by identity of the member.
            Self::SafeUuid(safe) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                safe.kind().hash(&mut hasher);
                Some(hasher.finish())
            }
            Self::RePattern(pattern) => {
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                pattern.pattern.hash(&mut hasher);
                pattern.flags.hash(&mut hasher);
                Some(hasher.finish())
            }
            // Mutable types, exceptions, iterators, modules, classes, and async types cannot be hashed
            // (Cell is handled specially in get_or_compute_hash)
            Self::List(_)
            | Self::Dict(_)
            | Self::Deque(_)
            | Self::DefaultDict(_)
            | Self::ChainMap(_)
            | Self::Set(_)
            | Self::Cell(_)
            | Self::Exception(_)
            | Self::Iter(_)
            | Self::Module(_)
            | Self::Coroutine(_)
            | Self::GatherFuture(_)
            | Self::ClassObject(_)
            | Self::MappingProxy(_)
            | Self::SlotDescriptor(_)
            | Self::BoundMethod(_)
            | Self::WeakRef(_)
            | Self::ClassSubclasses(_)
            | Self::ClassGetItem(_)
            | Self::FunctionGet(_)
            | Self::SuperProxy(_)
            | Self::StaticMethod(_)
            | Self::ClassMethod(_)
            | Self::UserProperty(_)
            | Self::PropertyAccessor(_)
            | Self::Counter(_)
            | Self::OrderedDict(_)
            | Self::NamedTupleFactory(_)
            | Self::Partial(_)
            | Self::CmpToKey(_)
            | Self::ItemGetter(_)
            | Self::AttrGetter(_)
            | Self::MethodCaller(_)
            | Self::LruCache(_)
            | Self::FunctionWrapper(_)
            | Self::Wraps(_)
            | Self::TotalOrderingMethod(_)
            | Self::CachedProperty(_)
            | Self::SingleDispatch(_)
            | Self::SingleDispatchRegister(_)
            | Self::SingleDispatchMethod(_)
            | Self::PartialMethod(_)
            | Self::Placeholder(_)
            | Self::TextWrapper(_)
            | Self::ReMatch(_)
            | Self::StdlibObject(_)
            | Self::Tee(_) => None,
            // Instances are handled in get_or_compute_hash (needs HeapId for identity hash)
            Self::Instance(_) => None,
            // ObjectNewImpl is unhashable
            Self::ObjectNewImpl(_) => None,
            // Hash objects are unhashable
            Self::Hash(_) => None,
            // zlib stream objects are hashed by identity in `compute_hash_inner`.
            Self::ZlibCompress(_) | Self::ZlibDecompress(_) => None,
            // LongInt is immutable and hashable
            Self::LongInt(li) => Some(li.hash()),
            // Generators are mutable and unhashable
            Self::Generator(_) => None,
            // Decimal is immutable and hashable
            Self::Decimal(d) => {
                use std::hash::Hash;
                let mut hasher = DefaultHasher::new();
                discriminant(self).hash(&mut hasher);
                d.hash(&mut hasher);
                Some(hasher.finish())
            }
            // Dict views are mutable (reflect dict changes) and unhashable
            Self::DictKeys(_) | Self::DictValues(_) | Self::DictItems(_) => None,
        }
    }

    /// Deletes an item by key from a container.
    ///
    /// Supports Dict (removes key-value pair) and List (removes item at index).
    /// Returns a KeyError for Dict or IndexError for List if the key doesn't exist.
    pub fn py_delitem(
        &mut self,
        key: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        match self {
            Self::Dict(d) => {
                if let Some((old_key, old_value)) = d.pop(&key, heap, interns)? {
                    old_key.drop_with_heap(heap);
                    old_value.drop_with_heap(heap);
                    key.drop_with_heap(heap);
                    Ok(())
                } else {
                    let err = ExcType::key_error(&key, heap, interns);
                    key.drop_with_heap(heap);
                    Err(err)
                }
            }
            Self::Counter(counter) => {
                if let Some((old_key, old_value)) = counter.dict_mut().pop(&key, heap, interns)? {
                    old_key.drop_with_heap(heap);
                    old_value.drop_with_heap(heap);
                    key.drop_with_heap(heap);
                    Ok(())
                } else {
                    let err = ExcType::key_error(&key, heap, interns);
                    key.drop_with_heap(heap);
                    Err(err)
                }
            }
            Self::OrderedDict(ordered) => {
                if let Some((old_key, old_value)) = ordered.dict_mut().pop(&key, heap, interns)? {
                    old_key.drop_with_heap(heap);
                    old_value.drop_with_heap(heap);
                    key.drop_with_heap(heap);
                    Ok(())
                } else {
                    let err = ExcType::key_error(&key, heap, interns);
                    key.drop_with_heap(heap);
                    Err(err)
                }
            }
            Self::ChainMap(chain) => {
                if let Some(Value::Ref(first_id)) = chain.maps().first() {
                    let removed: Option<(Value, Value)> =
                        heap.with_entry_mut(*first_id, |heap_inner, data| match data {
                            Self::Dict(dict) => dict.pop(&key, heap_inner, interns),
                            _ => Err(ExcType::type_error("ChainMap first mapping is not a dict")),
                        })?;
                    if let Some((old_key, old_value)) = removed {
                        old_key.drop_with_heap(heap);
                        old_value.drop_with_heap(heap);
                        chain.rebuild_flat(heap, interns)?;
                        key.drop_with_heap(heap);
                        Ok(())
                    } else {
                        let err = ExcType::key_error(&key, heap, interns);
                        key.drop_with_heap(heap);
                        Err(err)
                    }
                } else {
                    key.drop_with_heap(heap);
                    Err(ExcType::type_error("ChainMap has no first mapping"))
                }
            }
            #[expect(clippy::cast_possible_wrap, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Self::List(l) => {
                let idx = key.as_index(heap, Type::List)?;
                let len = l.as_vec().len() as i64;
                let actual_idx = if idx < 0 { idx + len } else { idx };
                if actual_idx < 0 || actual_idx >= len {
                    key.drop_with_heap(heap);
                    Err(ExcType::list_assignment_index_error())
                } else {
                    let removed = l.as_vec_mut().remove(actual_idx as usize);
                    removed.drop_with_heap(heap);
                    key.drop_with_heap(heap);
                    Ok(())
                }
            }
            #[expect(clippy::cast_possible_wrap, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Self::Deque(d) => {
                let idx = key.as_index(heap, Type::Deque)?;
                let len = d.len() as i64;
                let actual_idx = if idx < 0 { idx + len } else { idx };
                if actual_idx < 0 || actual_idx >= len {
                    key.drop_with_heap(heap);
                    Err(SimpleException::new_msg(ExcType::IndexError, "deque index out of range").into())
                } else {
                    let removed = d
                        .remove_index(actual_idx as usize)
                        .expect("validated deque deletion index");
                    removed.drop_with_heap(heap);
                    key.drop_with_heap(heap);
                    Ok(())
                }
            }
            _ => {
                key.drop_with_heap(heap);
                Err(ExcType::type_error(format!(
                    "'{}' object does not support item deletion",
                    self.py_type(heap)
                )))
            }
        }
    }
}

/// Manual implementation of AbstractValue dispatch for HeapData.
///
/// This provides efficient dispatch without boxing overhead by matching on
/// the enum variant and delegating to the inner type's implementation.
impl PyTrait for HeapData {
    fn py_type(&self, heap: &Heap<impl ResourceTracker>) -> Type {
        match self {
            Self::Str(s) => s.py_type(heap),
            Self::Bytes(b) => b.py_type(heap),
            Self::Bytearray(_) => Type::Bytearray,
            Self::List(l) => l.py_type(heap),
            Self::Tuple(t) => t.py_type(heap),
            Self::NamedTuple(nt) => nt.py_type(heap),
            Self::NamedTupleFactory(_) => Type::Type,
            Self::Dict(d) => d.py_type(heap),
            Self::Counter(c) => c.py_type(heap),
            Self::OrderedDict(od) => od.py_type(heap),
            Self::Deque(d) => d.py_type(heap),
            Self::DefaultDict(dd) => dd.py_type(heap),
            Self::ChainMap(chain) => chain.py_type(heap),
            Self::Set(s) => s.py_type(heap),
            Self::FrozenSet(fs) => fs.py_type(heap),
            Self::Closure(_, _, _) | Self::FunctionDefaults(_, _) => Type::Function,
            Self::Cell(_) => Type::Cell,
            Self::Range(_) => Type::Range,
            Self::Slice(_) => Type::Slice,
            Self::Exception(e) => e.py_type(),
            Self::Dataclass(dc) => dc.py_type(heap),
            Self::Iter(_) => Type::Iterator,
            Self::Tee(tee) => tee.py_type(heap),
            // LongInt is still `int` in Python - it's an implementation detail
            Self::LongInt(_) => Type::Int,
            Self::Module(_) => Type::Module,
            Self::Coroutine(_) | Self::GatherFuture(_) => Type::Coroutine,
            Self::Path(p) => p.py_type(heap),
            Self::ClassObject(cls) => cls.py_type(heap),
            Self::MappingProxy(mp) => mp.py_type(heap),
            Self::GenericAlias(ga) => ga.py_type(heap),
            Self::SlotDescriptor(sd) => sd.py_type(heap),
            Self::WeakRef(wr) => wr.py_type(heap),
            Self::Instance(inst) => inst.py_type(heap),
            Self::BoundMethod(bm) => bm.py_type(heap),
            Self::SuperProxy(sp) => sp.py_type(heap),
            Self::StaticMethod(sm) => sm.py_type(heap),
            Self::ClassMethod(cm) => cm.py_type(heap),
            Self::UserProperty(up) => up.py_type(heap),
            Self::PropertyAccessor(pa) => pa.py_type(heap),
            Self::ClassSubclasses(cs) => cs.py_type(heap),
            Self::ClassGetItem(cg) => cg.py_type(heap),
            Self::FunctionGet(fg) => fg.py_type(heap),
            Self::Hash(_) => Type::Hash,
            Self::ZlibCompress(zc) => zc.py_type(heap),
            Self::ZlibDecompress(zd) => zd.py_type(heap),
            Self::Partial(_)
            | Self::CmpToKey(_)
            | Self::ItemGetter(_)
            | Self::AttrGetter(_)
            | Self::MethodCaller(_)
            | Self::LruCache(_)
            | Self::FunctionWrapper(_)
            | Self::Wraps(_)
            | Self::TotalOrderingMethod(_)
            | Self::CachedProperty(_)
            | Self::SingleDispatch(_)
            | Self::SingleDispatchRegister(_)
            | Self::SingleDispatchMethod(_)
            | Self::PartialMethod(_) => Type::Function,
            Self::Placeholder(_) => Type::Object,
            Self::TextWrapper(wrapper) => wrapper.py_type(heap),
            Self::ReMatch(m) => m.py_type(heap),
            Self::RePattern(p) => p.py_type(heap),
            Self::StdlibObject(obj) => obj.py_type(heap),
            Self::Generator(_) => Type::Generator,
            Self::Timedelta(_) => Type::Timedelta,
            Self::Date(_) => Type::Date,
            Self::Datetime(_) => Type::Datetime,
            Self::Time(_) => Type::Time,
            Self::Timezone(_) => Type::Timezone,
            Self::Decimal(d) => d.py_type(heap),
            Self::Fraction(f) => f.py_type(heap),
            Self::Uuid(uuid) => uuid.py_type(heap),
            Self::SafeUuid(safe) => safe.py_type(heap),
            // ObjectNewImpl is a builtin method
            Self::ObjectNewImpl(_) => Type::BuiltinFunction,
            Self::DictKeys(dk) => dk.py_type(heap),
            Self::DictValues(dv) => dv.py_type(heap),
            Self::DictItems(di) => di.py_type(heap),
        }
    }

    fn py_estimate_size(&self) -> usize {
        match self {
            Self::Str(s) => s.py_estimate_size(),
            Self::Bytes(b) => b.py_estimate_size(),
            Self::Bytearray(b) => b.py_estimate_size(),
            Self::List(l) => l.py_estimate_size(),
            Self::Tuple(t) => t.py_estimate_size(),
            Self::NamedTuple(nt) => nt.py_estimate_size(),
            Self::NamedTupleFactory(factory) => factory.py_estimate_size(),
            Self::Dict(d) => d.py_estimate_size(),
            Self::Counter(c) => c.py_estimate_size(),
            Self::OrderedDict(od) => od.py_estimate_size(),
            Self::Deque(d) => d.py_estimate_size(),
            Self::DefaultDict(dd) => dd.py_estimate_size(),
            Self::ChainMap(chain) => chain.py_estimate_size(),
            Self::Set(s) => s.py_estimate_size(),
            Self::FrozenSet(fs) => fs.py_estimate_size(),
            // TODO: should include size of captured cells and defaults
            Self::Closure(_, _, _) | Self::FunctionDefaults(_, _) => 0,
            Self::Cell(v) => std::mem::size_of::<Value>() + v.py_estimate_size(),
            Self::Range(_) => std::mem::size_of::<Range>(),
            Self::Slice(s) => s.py_estimate_size(),
            Self::Exception(e) => std::mem::size_of::<SimpleException>() + e.arg().map_or(0, String::len),
            Self::Dataclass(dc) => dc.py_estimate_size(),
            Self::Iter(_) => std::mem::size_of::<OurosIter>(),
            Self::Tee(tee) => tee.py_estimate_size(),
            Self::LongInt(li) => li.estimate_size(),
            Self::Module(m) => std::mem::size_of::<Module>() + m.attrs().py_estimate_size(),
            Self::Coroutine(coro) => {
                std::mem::size_of::<Coroutine>()
                    + coro.namespace.len() * std::mem::size_of::<Value>()
                    + coro.frame_cells.len() * std::mem::size_of::<HeapId>()
            }
            Self::GatherFuture(gather) => {
                std::mem::size_of::<GatherFuture>()
                    + gather.items.len() * std::mem::size_of::<crate::asyncio::GatherItem>()
                    + gather.results.len() * std::mem::size_of::<Option<Value>>()
                    + gather.pending_calls.len() * std::mem::size_of::<crate::asyncio::CallId>()
            }
            Self::Path(p) => p.py_estimate_size(),
            Self::ClassObject(cls) => cls.py_estimate_size(),
            Self::MappingProxy(mp) => mp.py_estimate_size(),
            Self::GenericAlias(ga) => ga.py_estimate_size(),
            Self::SlotDescriptor(sd) => sd.py_estimate_size(),
            Self::WeakRef(wr) => wr.py_estimate_size(),
            Self::Instance(inst) => inst.py_estimate_size(),
            Self::BoundMethod(bm) => bm.py_estimate_size(),
            Self::SuperProxy(sp) => sp.py_estimate_size(),
            Self::StaticMethod(sm) => sm.py_estimate_size(),
            Self::ClassMethod(cm) => cm.py_estimate_size(),
            Self::UserProperty(up) => up.py_estimate_size(),
            Self::PropertyAccessor(pa) => pa.py_estimate_size(),
            Self::ClassSubclasses(cs) => cs.py_estimate_size(),
            Self::ClassGetItem(cg) => cg.py_estimate_size(),
            Self::FunctionGet(fg) => fg.py_estimate_size(),
            // Hash objects don't contain heap references
            Self::Hash(_) => std::mem::size_of::<crate::modules::hashlib::HashObject>(),
            Self::ZlibCompress(zc) => zc.py_estimate_size(),
            Self::ZlibDecompress(zd) => zd.py_estimate_size(),
            Self::Partial(_) => 64,
            Self::CmpToKey(_) => 32,
            Self::ItemGetter(g) => std::mem::size_of::<ItemGetter>() + std::mem::size_of_val(g.items()),
            Self::AttrGetter(g) => std::mem::size_of::<AttrGetter>() + std::mem::size_of_val(g.attrs()),
            Self::MethodCaller(mc) => {
                std::mem::size_of::<MethodCaller>()
                    + mc.name().py_estimate_size()
                    + std::mem::size_of_val(mc.args())
                    + std::mem::size_of_val(mc.kwargs())
            }
            Self::LruCache(lru) => lru.py_estimate_size(),
            Self::FunctionWrapper(fw) => fw.py_estimate_size(),
            Self::Wraps(wraps) => wraps.py_estimate_size(),
            Self::TotalOrderingMethod(method) => method.py_estimate_size(),
            Self::CachedProperty(_) => std::mem::size_of::<CachedProperty>(),
            Self::SingleDispatch(dispatcher) => {
                std::mem::size_of::<SingleDispatch>()
                    + dispatcher.registry.len() * std::mem::size_of::<(Value, Value)>()
            }
            Self::SingleDispatchRegister(_) => std::mem::size_of::<SingleDispatchRegister>(),
            Self::SingleDispatchMethod(_) => std::mem::size_of::<SingleDispatchMethod>(),
            Self::PartialMethod(method) => {
                std::mem::size_of::<PartialMethod>()
                    + method.args.len() * std::mem::size_of::<Value>()
                    + method.kwargs.len() * std::mem::size_of::<(Value, Value)>()
            }
            Self::Placeholder(_) => std::mem::size_of::<Placeholder>(),
            Self::TextWrapper(wrapper) => wrapper.py_estimate_size(),
            Self::ReMatch(_) => 96,
            Self::RePattern(_) => 48,
            Self::StdlibObject(obj) => obj.py_estimate_size(),
            Self::Generator(generator) => {
                std::mem::size_of::<Generator>()
                    + generator.namespace.len() * std::mem::size_of::<Value>()
                    + generator.frame_cells.len() * std::mem::size_of::<HeapId>()
                    + generator.saved_stack.len() * std::mem::size_of::<Value>()
            }
            Self::Timedelta(td) => td.py_estimate_size(),
            Self::Date(d) => d.py_estimate_size(),
            Self::Datetime(dt) => dt.py_estimate_size(),
            Self::Time(t) => t.py_estimate_size(),
            Self::Timezone(tz) => tz.py_estimate_size(),
            Self::Decimal(d) => d.py_estimate_size(),
            Self::Fraction(f) => f.py_estimate_size(),
            Self::Uuid(uuid) => uuid.py_estimate_size(),
            Self::SafeUuid(safe) => safe.py_estimate_size(),
            Self::ObjectNewImpl(_) => std::mem::size_of::<ObjectNewImpl>(),
            Self::DictKeys(dk) => dk.py_estimate_size(),
            Self::DictValues(dv) => dv.py_estimate_size(),
            Self::DictItems(di) => di.py_estimate_size(),
        }
    }

    fn py_len(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<usize> {
        match self {
            Self::Str(s) => PyTrait::py_len(s, heap, interns),
            Self::Bytes(b) => PyTrait::py_len(b, heap, interns),
            Self::Bytearray(b) => PyTrait::py_len(b, heap, interns),
            Self::List(l) => PyTrait::py_len(l, heap, interns),
            Self::Tuple(t) => PyTrait::py_len(t, heap, interns),
            Self::NamedTuple(nt) => PyTrait::py_len(nt, heap, interns),
            Self::Dict(d) => PyTrait::py_len(d, heap, interns),
            Self::Counter(c) => PyTrait::py_len(c, heap, interns),
            Self::OrderedDict(od) => PyTrait::py_len(od, heap, interns),
            Self::Deque(d) => PyTrait::py_len(d, heap, interns),
            Self::DefaultDict(dd) => PyTrait::py_len(dd, heap, interns),
            Self::ChainMap(chain) => PyTrait::py_len(chain, heap, interns),
            Self::Set(s) => PyTrait::py_len(s, heap, interns),
            Self::FrozenSet(fs) => PyTrait::py_len(fs, heap, interns),
            Self::MappingProxy(mp) => PyTrait::py_len(mp, heap, interns),
            Self::Range(r) => Some(r.len()),
            // Cells, Slices, Exceptions, Dataclasses, Iterators, LongInts, Modules, Paths, and async types don't have length
            Self::Cell(_)
            | Self::Closure(_, _, _)
            | Self::FunctionDefaults(_, _)
            | Self::Slice(_)
            | Self::Exception(_)
            | Self::Dataclass(_)
            | Self::Iter(_)
            | Self::Tee(_)
            | Self::LongInt(_)
            | Self::Module(_)
            | Self::Coroutine(_)
            | Self::GatherFuture(_)
            | Self::Path(_)
            | Self::ClassObject(_)
            | Self::Instance(_)
            | Self::BoundMethod(_)
            | Self::SuperProxy(_)
            | Self::StaticMethod(_)
            | Self::ClassMethod(_)
            | Self::SlotDescriptor(_)
            | Self::UserProperty(_)
            | Self::PropertyAccessor(_)
            | Self::GenericAlias(_)
            | Self::WeakRef(_)
            | Self::ClassSubclasses(_)
            | Self::ClassGetItem(_)
            | Self::FunctionGet(_)
            | Self::Hash(_)
            | Self::ZlibCompress(_)
            | Self::ZlibDecompress(_)
            | Self::Partial(_)
            | Self::CmpToKey(_)
            | Self::ItemGetter(_)
            | Self::AttrGetter(_)
            | Self::MethodCaller(_)
            | Self::LruCache(_)
            | Self::FunctionWrapper(_)
            | Self::Wraps(_)
            | Self::TotalOrderingMethod(_)
            | Self::CachedProperty(_)
            | Self::SingleDispatch(_)
            | Self::SingleDispatchRegister(_)
            | Self::SingleDispatchMethod(_)
            | Self::PartialMethod(_)
            | Self::Placeholder(_)
            | Self::TextWrapper(_)
            | Self::NamedTupleFactory(_)
            | Self::ReMatch(_)
            | Self::RePattern(_)
            | Self::StdlibObject(_)
            | Self::Generator(_)
            | Self::Timedelta(_)
            | Self::Date(_)
            | Self::Datetime(_)
            | Self::Time(_)
            | Self::Timezone(_)
            | Self::Decimal(_)
            | Self::Fraction(_)
            | Self::Uuid(_)
            | Self::SafeUuid(_)
            | Self::ObjectNewImpl(_) => None,
            Self::DictKeys(dk) => PyTrait::py_len(dk, heap, interns),
            Self::DictValues(dv) => PyTrait::py_len(dv, heap, interns),
            Self::DictItems(di) => PyTrait::py_len(di, heap, interns),
        }
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => a.py_eq(b, heap, interns),
            (Self::Bytes(a), Self::Bytes(b)) => a.py_eq(b, heap, interns),
            (Self::Bytearray(a), Self::Bytearray(b)) => a.py_eq(b, heap, interns),
            (Self::List(a), Self::List(b)) => a.py_eq(b, heap, interns),
            (Self::Tuple(a), Self::Tuple(b)) => a.py_eq(b, heap, interns),
            (Self::NamedTuple(a), Self::NamedTuple(b)) => a.py_eq(b, heap, interns),
            (Self::GenericAlias(a), Self::GenericAlias(b)) => a.py_eq(b, heap, interns),
            // NamedTuple can compare with Tuple by elements (matching CPython behavior)
            (Self::NamedTuple(nt), Self::Tuple(t)) | (Self::Tuple(t), Self::NamedTuple(nt)) => {
                let nt_items = nt.as_vec();
                let t_items = t.as_vec();
                if nt_items.len() != t_items.len() {
                    return false;
                }
                nt_items
                    .iter()
                    .zip(t_items.iter())
                    .all(|(a, b)| a.py_eq(b, heap, interns))
            }
            (Self::Counter(a), Self::Counter(b)) => a.py_eq(b, heap, interns),
            (Self::Dict(a), Self::Dict(b)) => a.py_eq(b, heap, interns),
            (Self::Counter(counter), Self::Dict(dict)) | (Self::Dict(dict), Self::Counter(counter)) => {
                counter.dict().py_eq(dict, heap, interns)
            }
            (Self::OrderedDict(a), Self::OrderedDict(b)) => a.py_eq(b, heap, interns),
            (Self::OrderedDict(ordered), Self::Dict(dict)) | (Self::Dict(dict), Self::OrderedDict(ordered)) => {
                ordered.dict().py_eq(dict, heap, interns)
            }
            (Self::OrderedDict(ordered), Self::Counter(counter))
            | (Self::Counter(counter), Self::OrderedDict(ordered)) => {
                ordered.dict().py_eq(counter.dict(), heap, interns)
            }
            (Self::Deque(a), Self::Deque(b)) => a.py_eq(b, heap, interns),
            (Self::DefaultDict(a), Self::DefaultDict(b)) => a.py_eq(b, heap, interns),
            (Self::ChainMap(a), Self::ChainMap(b)) => a.py_eq(b, heap, interns),
            (Self::Counter(counter), Self::DefaultDict(dd)) | (Self::DefaultDict(dd), Self::Counter(counter)) => {
                counter.dict().py_eq(&dd.dict(), heap, interns)
            }
            (Self::OrderedDict(ordered), Self::DefaultDict(dd))
            | (Self::DefaultDict(dd), Self::OrderedDict(ordered)) => ordered.dict().py_eq(&dd.dict(), heap, interns),
            (Self::Set(a), Self::Set(b)) => a.py_eq(b, heap, interns),
            (Self::FrozenSet(a), Self::FrozenSet(b)) => a.py_eq(b, heap, interns),
            (Self::Closure(a_id, a_cells, _), Self::Closure(b_id, b_cells, _)) => *a_id == *b_id && a_cells == b_cells,
            (Self::FunctionDefaults(a_id, _), Self::FunctionDefaults(b_id, _)) => *a_id == *b_id,
            (Self::Range(a), Self::Range(b)) => a.py_eq(b, heap, interns),
            (Self::Dataclass(a), Self::Dataclass(b)) => a.py_eq(b, heap, interns),
            // LongInt equality
            (Self::LongInt(a), Self::LongInt(b)) => a == b,
            // Slice equality
            (Self::Slice(a), Self::Slice(b)) => a.py_eq(b, heap, interns),
            // Path equality
            (Self::Path(a), Self::Path(b)) => a.py_eq(b, heap, interns),
            (Self::ClassObject(a), Self::ClassObject(b)) => a.py_eq(b, heap, interns),
            (Self::MappingProxy(a), Self::MappingProxy(b)) => a.py_eq(b, heap, interns),
            (Self::SlotDescriptor(a), Self::SlotDescriptor(b)) => a.py_eq(b, heap, interns),
            (Self::Instance(a), Self::Instance(b)) => a.py_eq(b, heap, interns),
            (Self::BoundMethod(a), Self::BoundMethod(b)) => a.py_eq(b, heap, interns),
            (Self::SuperProxy(a), Self::SuperProxy(b)) => a.py_eq(b, heap, interns),
            (Self::StaticMethod(a), Self::StaticMethod(b)) => a.py_eq(b, heap, interns),
            (Self::ClassMethod(a), Self::ClassMethod(b)) => a.py_eq(b, heap, interns),
            (Self::UserProperty(a), Self::UserProperty(b)) => a.py_eq(b, heap, interns),
            (Self::PropertyAccessor(a), Self::PropertyAccessor(b)) => a.py_eq(b, heap, interns),
            (Self::StdlibObject(a), Self::StdlibObject(b)) => a.py_eq(b, heap, interns),
            // datetime types
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_eq(b, heap, interns),
            (Self::Date(a), Self::Date(b)) => a.py_eq(b, heap, interns),
            (Self::Datetime(a), Self::Datetime(b)) => a.py_eq(b, heap, interns),
            (Self::Time(a), Self::Time(b)) => a.py_eq(b, heap, interns),
            (Self::Timezone(a), Self::Timezone(b)) => a.py_eq(b, heap, interns),
            (Self::RePattern(a), Self::RePattern(b)) => a.py_eq(b, heap, interns),
            // Cells, Exceptions, Iterators, Modules, and async types compare by identity only (handled at Value level via HeapId comparison)
            (Self::Cell(_), Self::Cell(_))
            | (Self::Exception(_), Self::Exception(_))
            | (Self::Iter(_), Self::Iter(_))
            | (Self::Tee(_), Self::Tee(_))
            | (Self::Module(_), Self::Module(_))
            | (Self::Coroutine(_), Self::Coroutine(_))
            | (Self::GatherFuture(_), Self::GatherFuture(_))
            | (Self::Hash(_), Self::Hash(_)) => false,
            (Self::Fraction(a), Self::Fraction(b)) => a.py_eq(b, heap, interns),
            (Self::Uuid(a), Self::Uuid(b)) => a.py_eq(b, heap, interns),
            (Self::SafeUuid(a), Self::SafeUuid(b)) => a.py_eq(b, heap, interns),
            (Self::ObjectNewImpl(_), Self::ObjectNewImpl(_)) => true, // All ObjectNewImpl are equivalent
            (Self::DictKeys(a), Self::DictKeys(b)) => a.py_eq(b, heap, interns),
            (Self::DictValues(a), Self::DictValues(b)) => a.py_eq(b, heap, interns),
            (Self::DictItems(a), Self::DictItems(b)) => a.py_eq(b, heap, interns),
            // DictItems can compare equal to Set/FrozenSet if they contain the same (key, value) tuples
            (Self::DictItems(items), Self::Set(set)) | (Self::Set(set), Self::DictItems(items)) => {
                // Convert items to a SetStorage and compare
                match items.to_set_storage(heap, interns) {
                    Ok(items_storage) => {
                        let result = items_storage.eq(set.storage(), heap, interns);
                        items_storage.drop_all_values(heap);
                        result
                    }
                    Err(_) => false,
                }
            }
            (Self::DictItems(items), Self::FrozenSet(fset)) | (Self::FrozenSet(fset), Self::DictItems(items)) => {
                // Convert items to a SetStorage and compare
                match items.to_set_storage(heap, interns) {
                    Ok(items_storage) => {
                        let result = items_storage.eq(fset.storage(), heap, interns);
                        items_storage.drop_all_values(heap);
                        result
                    }
                    Err(_) => false,
                }
            }
            _ => false, // Different types are never equal
        }
    }

    fn py_cmp(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => a.py_cmp(b, heap, interns),
            (Self::Bytes(a), Self::Bytes(b)) => a.py_cmp(b, heap, interns),
            (Self::Bytearray(a), Self::Bytearray(b)) => a.py_cmp(b, heap, interns),
            (Self::List(a), Self::List(b)) => a.py_cmp(b, heap, interns),
            (Self::Tuple(a), Self::Tuple(b)) => a.py_cmp(b, heap, interns),
            (Self::Deque(a), Self::Deque(b)) => a.py_cmp(b, heap, interns),
            (Self::NamedTuple(a), Self::NamedTuple(b)) => a.py_cmp(b, heap, interns),
            (Self::Range(a), Self::Range(b)) => a.py_cmp(b, heap, interns),
            (Self::Path(a), Self::Path(b)) => a.py_cmp(b, heap, interns),
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_cmp(b, heap, interns),
            (Self::Date(a), Self::Date(b)) => a.py_cmp(b, heap, interns),
            (Self::Datetime(a), Self::Datetime(b)) => a.py_cmp(b, heap, interns),
            (Self::Time(a), Self::Time(b)) => a.py_cmp(b, heap, interns),
            (Self::Timezone(a), Self::Timezone(b)) => a.py_cmp(b, heap, interns),
            (Self::Fraction(a), Self::Fraction(b)) => a.py_cmp(b, heap, interns),
            _ => None,
        }
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        match self {
            Self::Str(s) => s.py_dec_ref_ids(stack),
            Self::Bytes(b) => b.py_dec_ref_ids(stack),
            Self::Bytearray(b) => b.py_dec_ref_ids(stack),
            Self::List(l) => l.py_dec_ref_ids(stack),
            Self::Tuple(t) => t.py_dec_ref_ids(stack),
            Self::NamedTuple(nt) => nt.py_dec_ref_ids(stack),
            Self::NamedTupleFactory(factory) => factory.py_dec_ref_ids(stack),
            Self::Dict(d) => d.py_dec_ref_ids(stack),
            Self::Counter(c) => c.py_dec_ref_ids(stack),
            Self::OrderedDict(od) => od.py_dec_ref_ids(stack),
            Self::Deque(d) => d.py_dec_ref_ids(stack),
            Self::DefaultDict(dd) => dd.py_dec_ref_ids(stack),
            Self::ChainMap(chain) => chain.py_dec_ref_ids(stack),
            Self::Set(s) => s.py_dec_ref_ids(stack),
            Self::FrozenSet(fs) => fs.py_dec_ref_ids(stack),
            Self::Closure(_, cells, defaults) => {
                // Decrement ref count for captured cells
                stack.extend(cells.iter().copied());
                // Decrement ref count for default values that are heap references
                for default in defaults.iter_mut() {
                    default.py_dec_ref_ids(stack);
                }
            }
            Self::FunctionDefaults(_, defaults) => {
                // Decrement ref count for default values that are heap references
                for default in defaults.iter_mut() {
                    default.py_dec_ref_ids(stack);
                }
            }
            Self::Cell(v) => v.py_dec_ref_ids(stack),
            Self::Dataclass(dc) => dc.py_dec_ref_ids(stack),
            Self::Iter(iter) => iter.py_dec_ref_ids(stack),
            Self::Tee(tee) => tee.py_dec_ref_ids(stack),
            Self::Module(m) => m.py_dec_ref_ids(stack),
            Self::GenericAlias(ga) => ga.py_dec_ref_ids(stack),
            Self::Coroutine(coro) => {
                // Decrement ref count for frame cells
                stack.extend(coro.frame_cells.iter().copied());
                // Decrement ref count for namespace values that are heap references
                for value in &mut coro.namespace {
                    value.py_dec_ref_ids(stack);
                }
            }
            Self::GatherFuture(gather) => {
                // Decrement ref count for coroutine HeapIds
                for item in &gather.items {
                    if let GatherItem::Coroutine(id) = item {
                        stack.push(*id);
                    }
                }
                // Decrement ref count for result values that are heap references
                for result in gather.results.iter_mut().flatten() {
                    result.py_dec_ref_ids(stack);
                }
            }
            Self::ClassObject(cls) => cls.py_dec_ref_ids(stack),
            Self::MappingProxy(mp) => mp.py_dec_ref_ids(stack),
            Self::SlotDescriptor(sd) => sd.py_dec_ref_ids(stack),
            Self::WeakRef(wr) => wr.py_dec_ref_ids(stack),
            Self::Instance(inst) => inst.py_dec_ref_ids(stack),
            Self::BoundMethod(bm) => bm.py_dec_ref_ids(stack),
            Self::SuperProxy(sp) => sp.py_dec_ref_ids(stack),
            Self::StaticMethod(sm) => sm.py_dec_ref_ids(stack),
            Self::ClassMethod(cm) => cm.py_dec_ref_ids(stack),
            Self::UserProperty(up) => up.py_dec_ref_ids(stack),
            Self::PropertyAccessor(pa) => pa.py_dec_ref_ids(stack),
            Self::ClassSubclasses(cs) => cs.py_dec_ref_ids(stack),
            Self::ClassGetItem(cg) => cg.py_dec_ref_ids(stack),
            Self::FunctionGet(fg) => fg.py_dec_ref_ids(stack),
            Self::Partial(p) => {
                p.func.py_dec_ref_ids(stack);
                for arg in &mut p.args {
                    arg.py_dec_ref_ids(stack);
                }
                for (key, value) in &mut p.kwargs {
                    key.py_dec_ref_ids(stack);
                    value.py_dec_ref_ids(stack);
                }
            }
            Self::CmpToKey(c) => {
                c.func.py_dec_ref_ids(stack);
            }
            Self::ItemGetter(g) => {
                for item in &mut g.items {
                    item.py_dec_ref_ids(stack);
                }
            }
            Self::AttrGetter(g) => {
                for attr in &mut g.attrs {
                    attr.py_dec_ref_ids(stack);
                }
            }
            Self::MethodCaller(mc) => {
                mc.name.py_dec_ref_ids(stack);
                for arg in &mut mc.args {
                    arg.py_dec_ref_ids(stack);
                }
                for (key, value) in &mut mc.kwargs {
                    key.py_dec_ref_ids(stack);
                    value.py_dec_ref_ids(stack);
                }
            }
            Self::LruCache(lru) => lru.py_dec_ref_ids(stack),
            Self::StdlibObject(obj) => obj.py_dec_ref_ids(stack),
            Self::FunctionWrapper(fw) => fw.py_dec_ref_ids(stack),
            Self::Wraps(wraps) => wraps.py_dec_ref_ids(stack),
            Self::TotalOrderingMethod(method) => method.py_dec_ref_ids(stack),
            Self::CachedProperty(cached) => cached.func.py_dec_ref_ids(stack),
            Self::SingleDispatch(dispatcher) => {
                dispatcher.func.py_dec_ref_ids(stack);
                for (cls, func) in &mut dispatcher.registry {
                    cls.py_dec_ref_ids(stack);
                    func.py_dec_ref_ids(stack);
                }
            }
            Self::SingleDispatchRegister(register) => {
                register.dispatcher.py_dec_ref_ids(stack);
                register.cls.py_dec_ref_ids(stack);
            }
            Self::SingleDispatchMethod(method) => method.dispatcher.py_dec_ref_ids(stack),
            Self::PartialMethod(method) => {
                method.func.py_dec_ref_ids(stack);
                for arg in &mut method.args {
                    arg.py_dec_ref_ids(stack);
                }
                for (key, value) in &mut method.kwargs {
                    key.py_dec_ref_ids(stack);
                    value.py_dec_ref_ids(stack);
                }
            }
            // Range, Slice, Exception, LongInt, Path, Hash, ReMatch, RePattern have no nested heap references
            Self::Range(_)
            | Self::Slice(_)
            | Self::Exception(_)
            | Self::LongInt(_)
            | Self::Path(_)
            | Self::Hash(_)
            | Self::ZlibCompress(_)
            | Self::ZlibDecompress(_)
            | Self::Placeholder(_)
            | Self::TextWrapper(_)
            | Self::ReMatch(_)
            | Self::RePattern(_)
            | Self::Uuid(_)
            | Self::SafeUuid(_) => {}
            Self::Generator(g) => {
                // Decrement ref count for namespace values
                for value in &mut g.namespace.iter_mut() {
                    value.py_dec_ref_ids(stack);
                }
                // Decrement ref count for saved stack values
                for value in &mut g.saved_stack.iter_mut() {
                    value.py_dec_ref_ids(stack);
                }
                // Decrement ref count for frame cells
                stack.extend(g.frame_cells.iter().copied());
            }
            // datetime types have no nested heap references (except Time with tzinfo)
            Self::Timedelta(_) => {}
            Self::Date(_) => {}
            Self::Datetime(_) => {}
            Self::Time(t) => {
                if let Some(tz_id) = t.tzinfo() {
                    stack.push(tz_id);
                }
            }
            Self::Timezone(_) => {}
            // Decimal is a leaf type with no heap references
            Self::Decimal(_) => {}
            // Fraction has no heap references
            Self::Fraction(_) => {}
            // ObjectNewImpl has no heap references
            Self::ObjectNewImpl(_) => {}
            Self::DictKeys(dk) => dk.py_dec_ref_ids(stack),
            Self::DictValues(dv) => dv.py_dec_ref_ids(stack),
            Self::DictItems(di) => di.py_dec_ref_ids(stack),
        }
    }

    fn py_bool(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        match self {
            Self::Str(s) => s.py_bool(heap, interns),
            Self::Bytes(b) => b.py_bool(heap, interns),
            Self::Bytearray(b) => b.py_bool(heap, interns),
            Self::List(l) => l.py_bool(heap, interns),
            Self::Tuple(t) => t.py_bool(heap, interns),
            Self::NamedTuple(nt) => nt.py_bool(heap, interns),
            Self::Dict(d) => d.py_bool(heap, interns),
            Self::Counter(c) => c.py_bool(heap, interns),
            Self::OrderedDict(od) => od.py_bool(heap, interns),
            Self::Deque(d) => d.py_bool(heap, interns),
            Self::DefaultDict(dd) => dd.py_bool(heap, interns),
            Self::ChainMap(chain) => chain.py_bool(heap, interns),
            Self::Set(s) => s.py_bool(heap, interns),
            Self::FrozenSet(fs) => fs.py_bool(heap, interns),
            Self::Closure(_, _, _) | Self::FunctionDefaults(_, _) => true,
            Self::Cell(_) => true, // Cells are always truthy
            Self::Range(r) => r.py_bool(heap, interns),
            Self::Slice(s) => s.py_bool(heap, interns),
            Self::Exception(_) => true, // Exceptions are always truthy
            Self::Dataclass(dc) => dc.py_bool(heap, interns),
            Self::Iter(_) => true, // Iterators are always truthy
            Self::Tee(_) => true,  // Tee state is always truthy
            Self::LongInt(li) => !li.is_zero(),
            Self::Module(_) => true,       // Modules are always truthy
            Self::Coroutine(_) => true,    // Coroutines are always truthy
            Self::GatherFuture(_) => true, // GatherFutures are always truthy
            Self::Path(p) => p.py_bool(heap, interns),
            Self::ClassObject(cls) => cls.py_bool(heap, interns),
            Self::MappingProxy(mp) => mp.py_bool(heap, interns),
            Self::GenericAlias(ga) => ga.py_bool(heap, interns),
            Self::SlotDescriptor(sd) => sd.py_bool(heap, interns),
            Self::WeakRef(wr) => wr.py_bool(heap, interns),
            Self::Instance(inst) => inst.py_bool(heap, interns),
            Self::BoundMethod(bm) => bm.py_bool(heap, interns),
            Self::SuperProxy(sp) => sp.py_bool(heap, interns),
            Self::StaticMethod(sm) => sm.py_bool(heap, interns),
            Self::ClassMethod(cm) => cm.py_bool(heap, interns),
            Self::UserProperty(up) => up.py_bool(heap, interns),
            Self::PropertyAccessor(pa) => pa.py_bool(heap, interns),
            Self::ClassSubclasses(cs) => cs.py_bool(heap, interns),
            Self::ClassGetItem(cg) => cg.py_bool(heap, interns),
            Self::FunctionGet(fg) => fg.py_bool(heap, interns),
            Self::NamedTupleFactory(_) => true,
            // Hash objects are always truthy
            Self::Hash(_) => true,
            Self::ZlibCompress(zc) => zc.py_bool(heap, interns),
            Self::ZlibDecompress(zd) => zd.py_bool(heap, interns),
            Self::Partial(_)
            | Self::CmpToKey(_)
            | Self::ItemGetter(_)
            | Self::AttrGetter(_)
            | Self::MethodCaller(_)
            | Self::LruCache(_)
            | Self::FunctionWrapper(_)
            | Self::Wraps(_)
            | Self::TotalOrderingMethod(_)
            | Self::CachedProperty(_)
            | Self::SingleDispatch(_)
            | Self::SingleDispatchRegister(_)
            | Self::SingleDispatchMethod(_)
            | Self::PartialMethod(_)
            | Self::Placeholder(_) => true,
            Self::TextWrapper(wrapper) => wrapper.py_bool(heap, interns),
            Self::ReMatch(m) => m.py_bool(heap, interns),
            Self::RePattern(p) => p.py_bool(heap, interns),
            Self::StdlibObject(obj) => obj.py_bool(heap, interns),
            Self::Generator(_) => true, // Generators are always truthy
            Self::Timedelta(td) => td.py_bool(heap, interns),
            Self::Date(d) => d.py_bool(heap, interns),
            Self::Datetime(dt) => dt.py_bool(heap, interns),
            Self::Time(t) => t.py_bool(heap, interns),
            Self::Timezone(_) => true, // Timezones are always truthy
            Self::Decimal(d) => d.py_bool(heap, interns),
            Self::Fraction(f) => f.py_bool(heap, interns),
            Self::Uuid(uuid) => uuid.py_bool(heap, interns),
            Self::SafeUuid(safe) => safe.py_bool(heap, interns),
            Self::ObjectNewImpl(_) => true, // ObjectNewImpl is always truthy
            Self::DictKeys(dk) => dk.py_bool(heap, interns),
            Self::DictValues(dv) => dv.py_bool(heap, interns),
            Self::DictItems(di) => di.py_bool(heap, interns),
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
            Self::Str(s) => s.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Bytes(b) => b.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Bytearray(b) => {
                f.write_str("bytearray(")?;
                b.py_repr_fmt(f, heap, heap_ids, interns)?;
                f.write_str(")")
            }
            Self::List(l) => l.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Tuple(t) => t.py_repr_fmt(f, heap, heap_ids, interns),
            Self::NamedTuple(nt) => nt.py_repr_fmt(f, heap, heap_ids, interns),
            Self::NamedTupleFactory(factory) => factory.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Dict(d) => d.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Counter(c) => c.py_repr_fmt(f, heap, heap_ids, interns),
            Self::OrderedDict(od) => od.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Deque(d) => d.py_repr_fmt(f, heap, heap_ids, interns),
            Self::DefaultDict(dd) => dd.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ChainMap(chain) => chain.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Set(s) => s.py_repr_fmt(f, heap, heap_ids, interns),
            Self::FrozenSet(fs) => fs.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Closure(f_id, _, _) | Self::FunctionDefaults(f_id, _) => {
                interns.get_function(*f_id).py_repr_fmt(f, interns, 0)
            }
            // Cell repr shows the contained value's type
            Self::Cell(v) => write!(f, "<cell: {} object>", v.py_type(heap)),
            Self::Range(r) => r.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Slice(s) => s.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Exception(e) => e.py_repr_fmt(f),
            Self::Dataclass(dc) => dc.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Iter(_) => write!(f, "<iterator>"),
            Self::Tee(tee) => tee.py_repr_fmt(f, heap, heap_ids, interns),
            Self::LongInt(li) => write!(f, "{li}"),
            Self::Module(m) => write!(f, "<module '{}'>", interns.get_str(m.name())),
            Self::Coroutine(coro) => {
                let func = interns.get_function(coro.func_id);
                let name = interns.get_str(func.name.name_id);
                write!(f, "<coroutine object {name}>")
            }
            Self::GatherFuture(gather) => write!(f, "<gather({})>", gather.item_count()),
            Self::Path(p) => p.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ClassObject(cls) => cls.py_repr_fmt(f, heap, heap_ids, interns),
            Self::MappingProxy(mp) => mp.py_repr_fmt(f, heap, heap_ids, interns),
            Self::GenericAlias(ga) => ga.py_repr_fmt(f, heap, heap_ids, interns),
            Self::SlotDescriptor(sd) => sd.py_repr_fmt(f, heap, heap_ids, interns),
            Self::WeakRef(wr) => wr.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Instance(inst) => inst.py_repr_fmt(f, heap, heap_ids, interns),
            Self::BoundMethod(bm) => bm.py_repr_fmt(f, heap, heap_ids, interns),
            Self::SuperProxy(sp) => sp.py_repr_fmt(f, heap, heap_ids, interns),
            Self::StaticMethod(sm) => sm.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ClassMethod(cm) => cm.py_repr_fmt(f, heap, heap_ids, interns),
            Self::UserProperty(up) => up.py_repr_fmt(f, heap, heap_ids, interns),
            Self::PropertyAccessor(pa) => pa.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ClassSubclasses(cs) => cs.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ClassGetItem(cg) => cg.py_repr_fmt(f, heap, heap_ids, interns),
            Self::FunctionGet(fg) => fg.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Hash(h) => h.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ZlibCompress(zc) => zc.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ZlibDecompress(zd) => zd.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Partial(partial) => partial.py_repr_fmt(f, heap, heap_ids, interns),
            Self::CmpToKey(_) => write!(f, "functools.cmp_to_key(...)"),
            Self::ItemGetter(g) => {
                f.write_str("operator.itemgetter(")?;
                let mut first = true;
                for item in &g.items {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;
                    item.py_repr_fmt(f, heap, heap_ids, interns)?;
                }
                f.write_char(')')
            }
            Self::AttrGetter(g) => {
                f.write_str("operator.attrgetter(")?;
                let mut first = true;
                for attr in &g.attrs {
                    if !first {
                        f.write_str(", ")?;
                    }
                    first = false;
                    attr.py_repr_fmt(f, heap, heap_ids, interns)?;
                }
                f.write_char(')')
            }
            Self::MethodCaller(mc) => {
                f.write_str("operator.methodcaller(")?;
                mc.name.py_repr_fmt(f, heap, heap_ids, interns)?;
                for arg in &mc.args {
                    f.write_str(", ")?;
                    arg.py_repr_fmt(f, heap, heap_ids, interns)?;
                }
                for (key, value) in &mc.kwargs {
                    let key_str = key.py_str(heap, interns);
                    f.write_str(", ")?;
                    f.write_str(key_str.as_ref())?;
                    f.write_char('=')?;
                    value.py_repr_fmt(f, heap, heap_ids, interns)?;
                }
                f.write_char(')')
            }
            Self::LruCache(lru) => lru.py_repr_fmt(f, heap, heap_ids, interns),
            Self::FunctionWrapper(fw) => fw.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Wraps(wraps) => wraps.py_repr_fmt(f, heap, heap_ids, interns),
            Self::TotalOrderingMethod(method) => method.py_repr_fmt(f, heap, heap_ids, interns),
            Self::CachedProperty(_) => write!(f, "<functools.cached_property object>"),
            Self::SingleDispatch(_) => write!(f, "<functools.singledispatch object>"),
            Self::SingleDispatchRegister(_) => write!(f, "<functools.singledispatch register>"),
            Self::SingleDispatchMethod(_) => write!(f, "<functools.singledispatchmethod object>"),
            Self::PartialMethod(_) => write!(f, "<functools.partialmethod object>"),
            Self::Placeholder(_) => write!(f, "functools.Placeholder"),
            Self::TextWrapper(wrapper) => wrapper.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ReMatch(m) => m.py_repr_fmt(f, heap, heap_ids, interns),
            Self::RePattern(p) => p.py_repr_fmt(f, heap, heap_ids, interns),
            Self::StdlibObject(obj) => obj.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Generator(generator) => {
                let func = interns.get_function(generator.func_id);
                let name = interns.get_str(func.name.name_id);
                write!(f, "<generator object {name}>")
            }
            Self::Timedelta(td) => td.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Date(d) => d.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Datetime(dt) => dt.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Time(t) => t.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Timezone(tz) => tz.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Decimal(d) => d.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Fraction(frac) => frac.py_repr_fmt(f, heap, heap_ids, interns),
            Self::Uuid(uuid) => uuid.py_repr_fmt(f, heap, heap_ids, interns),
            Self::SafeUuid(safe) => safe.py_repr_fmt(f, heap, heap_ids, interns),
            Self::ObjectNewImpl(_) => write!(f, "<built-in method __new__>"),
            Self::DictKeys(dk) => dk.py_repr_fmt(f, heap, heap_ids, interns),
            Self::DictValues(dv) => dv.py_repr_fmt(f, heap, heap_ids, interns),
            Self::DictItems(di) => di.py_repr_fmt(f, heap, heap_ids, interns),
        }
    }

    fn py_str(&self, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Cow<'static, str> {
        match self {
            // Strings return their value directly without quotes
            Self::Str(s) => s.py_str(heap, interns),
            // LongInt returns its string representation
            Self::LongInt(li) => Cow::Owned(li.to_string()),
            // Exceptions return just the message (or empty string if no message)
            Self::Exception(e) => Cow::Owned(e.py_str()),
            // Path-like values return their string form (Windows flavor uses backslashes)
            Self::Path(p) => Cow::Owned(p.display_path()),
            // Fraction returns its string form (e.g. "1/3" or "0")
            Self::Fraction(f) => f.py_str(heap, interns),
            // Datetime values have Python-specific str() formatting.
            Self::Timedelta(td) => td.py_str(heap, interns),
            Self::Date(d) => d.py_str(heap, interns),
            Self::Datetime(dt) => dt.py_str(heap, interns),
            Self::Time(t) => t.py_str(heap, interns),
            Self::Timezone(tz) => tz.py_str(heap, interns),
            Self::Decimal(d) => d.py_str(heap, interns),
            // UUID returns the canonical hyphenated string form
            Self::Uuid(uuid) => uuid.py_str(heap, interns),
            // Instances may provide specialized display semantics (e.g. enum members).
            Self::Instance(instance) => instance.py_str(heap, interns),
            // All other types use repr
            _ => self.py_repr(heap, interns),
        }
    }

    fn py_add(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => a.py_add(b, heap, interns),
            (Self::Bytes(a), Self::Bytes(b)) => a.py_add(b, heap, interns),
            (Self::Bytes(a), Self::Bytearray(b)) => a.py_add_with_result_type(b, heap, false),
            (Self::Bytearray(a), Self::Bytes(b)) => a.py_add_with_result_type(b, heap, true),
            (Self::Bytearray(a), Self::Bytearray(b)) => a.py_add_with_result_type(b, heap, true),
            (Self::List(a), Self::List(b)) => a.py_add(b, heap, interns),
            (Self::Tuple(a), Self::Tuple(b)) => a.py_add(b, heap, interns),
            (Self::Deque(a), Self::Deque(b)) => a.py_add(b, heap, interns),
            (Self::Dict(a), Self::Dict(b)) => a.py_add(b, heap, interns),
            (Self::Counter(a), Self::Counter(b)) => a.py_add(b, heap, interns),
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_add(b, heap, interns),
            (Self::Date(a), Self::Timedelta(b)) => a.py_add_days(b.days(), heap),
            (Self::Datetime(a), Self::Timedelta(b)) => a.py_add_timedelta(b, heap),
            // Cells and Dataclasses don't support arithmetic operations
            _ => Ok(None),
        }
    }

    fn py_sub(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Option<Value>, crate::resource::ResourceError> {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => a.py_sub(b, heap),
            (Self::Bytes(a), Self::Bytes(b)) => a.py_sub(b, heap),
            (Self::Bytearray(a), Self::Bytearray(b)) => a.py_sub(b, heap),
            (Self::List(a), Self::List(b)) => a.py_sub(b, heap),
            (Self::Tuple(a), Self::Tuple(b)) => a.py_sub(b, heap),
            (Self::Dict(a), Self::Dict(b)) => a.py_sub(b, heap),
            (Self::Set(a), Self::Set(b)) => a.py_sub(b, heap),
            (Self::FrozenSet(a), Self::FrozenSet(b)) => a.py_sub(b, heap),
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_sub(b, heap),
            (Self::Date(a), Self::Timedelta(b)) => a.py_add_days(-b.days(), heap),
            (Self::Date(a), Self::Date(b)) => a.py_sub_date(b, heap),
            (Self::Datetime(a), Self::Timedelta(b)) => a.py_sub_timedelta(b, heap),
            (Self::Datetime(a), Self::Datetime(b)) => a.py_sub_datetime(b, heap),
            // Cells don't support arithmetic operations
            _ => Ok(None),
        }
    }

    fn py_mult(
        &self,
        _other: &Self,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<Value>> {
        Ok(None)
    }

    fn py_div(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<Value>> {
        match (self, other) {
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_div(b, heap, interns),
            _ => Ok(None),
        }
    }

    fn py_floordiv(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Option<Value>> {
        match (self, other) {
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_floordiv(b, heap),
            _ => Ok(None),
        }
    }

    fn py_mod(
        &self,
        other: &Self,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> crate::exception_private::RunResult<Option<Value>> {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => a.py_mod(b, heap),
            (Self::Bytes(a), Self::Bytes(b)) => a.py_mod(b, heap),
            (Self::Bytearray(a), Self::Bytearray(b)) => a.py_mod(b, heap),
            (Self::List(a), Self::List(b)) => a.py_mod(b, heap),
            (Self::Tuple(a), Self::Tuple(b)) => a.py_mod(b, heap),
            (Self::Dict(a), Self::Dict(b)) => a.py_mod(b, heap),
            (Self::Timedelta(a), Self::Timedelta(b)) => a.py_mod(b, heap),
            (Self::LongInt(a), Self::LongInt(b)) => {
                if b.is_zero() {
                    Err(crate::exception_private::ExcType::zero_division().into())
                } else {
                    let bi = a.inner().mod_floor(b.inner());
                    Ok(LongInt::new(bi).into_value(heap).map(Some)?)
                }
            }
            // Cells don't support arithmetic operations
            _ => Ok(None),
        }
    }

    fn py_mod_eq(&self, other: &Self, right_value: i64) -> Option<bool> {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => a.py_mod_eq(b, right_value),
            (Self::Bytes(a), Self::Bytes(b)) => a.py_mod_eq(b, right_value),
            (Self::Bytearray(a), Self::Bytearray(b)) => a.py_mod_eq(b, right_value),
            (Self::List(a), Self::List(b)) => a.py_mod_eq(b, right_value),
            (Self::Tuple(a), Self::Tuple(b)) => a.py_mod_eq(b, right_value),
            (Self::Dict(a), Self::Dict(b)) => a.py_mod_eq(b, right_value),
            // Cells don't support arithmetic operations
            _ => None,
        }
    }

    fn py_iadd(
        &mut self,
        other: Value,
        heap: &mut Heap<impl ResourceTracker>,
        self_id: Option<HeapId>,
        interns: &Interns,
    ) -> RunResult<bool> {
        match self {
            Self::Str(s) => s.py_iadd(other, heap, self_id, interns),
            Self::Bytes(b) => b.py_iadd(other, heap, self_id, interns),
            Self::Bytearray(b) => b.py_iadd(other, heap, self_id, interns),
            Self::List(l) => l.py_iadd(other, heap, self_id, interns),
            Self::Tuple(t) => t.py_iadd(other, heap, self_id, interns),
            Self::Deque(d) => d.py_iadd(other, heap, self_id, interns),
            Self::Dict(d) => d.py_iadd(other, heap, self_id, interns),
            _ => {
                // Drop other if it's a Ref (ensure proper refcounting for unsupported types)
                other.drop_with_heap(heap);
                Ok(false)
            }
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match self {
            Self::Str(s) => s.py_call_attr(heap, attr, args, interns, self_id),
            Self::Bytes(b) => b.py_call_attr(heap, attr, args, interns, self_id),
            Self::Bytearray(b) => b.py_call_attr_bytearray(heap, attr, args, interns),
            Self::List(l) => l.py_call_attr(heap, attr, args, interns, self_id),
            Self::Tuple(t) => t.py_call_attr(heap, attr, args, interns, self_id),
            Self::Dict(d) => d.py_call_attr(heap, attr, args, interns, self_id),
            Self::Counter(c) => c.py_call_attr(heap, attr, args, interns, self_id),
            Self::OrderedDict(od) => od.py_call_attr(heap, attr, args, interns, self_id),
            Self::Deque(d) => d.py_call_attr(heap, attr, args, interns, self_id),
            Self::DefaultDict(dd) => dd.py_call_attr(heap, attr, args, interns, self_id),
            Self::ChainMap(chain) => chain.py_call_attr(heap, attr, args, interns, self_id),
            Self::Set(s) => s.py_call_attr(heap, attr, args, interns, self_id),
            Self::FrozenSet(fs) => fs.py_call_attr(heap, attr, args, interns, self_id),
            Self::Dataclass(dc) => dc.py_call_attr(heap, attr, args, interns, self_id),
            Self::Path(p) => p.py_call_attr(heap, attr, args, interns, self_id),
            Self::MappingProxy(mp) => mp.py_call_attr(heap, attr, args, interns, self_id),
            Self::Instance(inst) => inst.py_call_attr(heap, attr, args, interns, self_id),
            Self::Hash(h) => h.py_call_attr(heap, attr, args, interns, self_id),
            Self::ZlibCompress(zc) => zc.py_call_attr(heap, attr, args, interns, self_id),
            Self::ZlibDecompress(zd) => zd.py_call_attr(heap, attr, args, interns, self_id),
            Self::LruCache(lru) => lru.py_call_attr(heap, attr, args, interns, self_id),
            Self::NamedTuple(nt) => nt.py_call_attr(heap, attr, args, interns, self_id),
            Self::NamedTupleFactory(factory) => factory.py_call_attr(heap, attr, args, interns, self_id),
            Self::Partial(partial) => partial.py_call_attr(heap, attr, args, interns, self_id),
            Self::ReMatch(m) => m.py_call_attr(heap, attr, args, interns, self_id),
            Self::RePattern(p) => p.py_call_attr(heap, attr, args, interns, self_id),
            Self::TextWrapper(wrapper) => wrapper.py_call_attr(heap, attr, args, interns, self_id),
            Self::StdlibObject(obj) => obj.py_call_attr(heap, attr, args, interns, self_id),
            Self::Fraction(f) => f.py_call_attr(heap, attr, args, interns, self_id),
            Self::Decimal(d) => d.py_call_attr(heap, attr, args, interns, self_id),
            Self::Timedelta(td) => td.py_call_attr(heap, attr, args, interns, self_id),
            Self::Date(d) => d.py_call_attr(heap, attr, args, interns, self_id),
            Self::Datetime(dt) => dt.py_call_attr(heap, attr, args, interns, self_id),
            Self::Time(t) => t.py_call_attr(heap, attr, args, interns, self_id),
            Self::Timezone(tz) => tz.py_call_attr(heap, attr, args, interns, self_id),
            Self::Uuid(uuid) => uuid.py_call_attr(heap, attr, args, interns, self_id),
            Self::SafeUuid(safe) => safe.py_call_attr(heap, attr, args, interns, self_id),
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr.as_str(interns))),
        }
    }

    fn py_call_attr_raw(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<AttrCallResult> {
        match self {
            // Path has special handling for OS calls (exists, read_text, etc.)
            Self::Path(p) => p.py_call_attr_raw(heap, attr, args, interns, self_id),
            // Dataclass has special handling for external method calls
            Self::Dataclass(dc) => dc.py_call_attr_raw(heap, attr, args, interns, self_id),
            // Module has special handling for OS calls (os.getenv, etc.)
            Self::Module(m) => m.py_call_attr_raw(heap, attr, args, interns, self_id),
            // StdlibObject may return deferred VM calls (e.g. pprint.PrettyPrinter.pprint).
            Self::StdlibObject(obj) => obj.py_call_attr_raw(heap, attr, args, interns, self_id),
            // RePattern has special handling for sub/subn with callable replacement.
            Self::RePattern(pat) => pat.py_call_attr_raw(heap, attr, args, interns, self_id),
            // All other types use the default implementation (wrap py_call_attr)
            _ => self
                .py_call_attr(heap, attr, args, interns, self_id)
                .map(AttrCallResult::Value),
        }
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Value> {
        match self {
            Self::Str(s) => s.py_getitem(key, heap, interns),
            Self::Bytes(b) => b.py_getitem(key, heap, interns),
            Self::Bytearray(b) => b.py_getitem_bytearray(key, heap),
            Self::List(l) => l.py_getitem(key, heap, interns),
            Self::Tuple(t) => t.py_getitem(key, heap, interns),
            Self::NamedTuple(nt) => nt.py_getitem(key, heap, interns),
            Self::Dict(d) => d.py_getitem(key, heap, interns),
            Self::Counter(c) => c.py_getitem(key, heap, interns),
            Self::OrderedDict(od) => od.py_getitem(key, heap, interns),
            Self::Range(r) => r.py_getitem(key, heap, interns),
            Self::MappingProxy(mp) => mp.py_getitem(key, heap, interns),
            Self::Deque(d) => d.py_getitem(key, heap, interns),
            Self::DefaultDict(dd) => dd.py_getitem(key, heap, interns),
            Self::ChainMap(chain) => chain.py_getitem(key, heap, interns),
            _ => Err(ExcType::type_error_not_sub(self.py_type(heap))),
        }
    }

    fn py_setitem(
        &mut self,
        key: Value,
        value: Value,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<()> {
        match self {
            Self::Str(s) => s.py_setitem(key, value, heap, interns),
            Self::Bytes(b) => b.py_setitem(key, value, heap, interns),
            Self::Bytearray(b) => b.py_setitem_bytearray(key, value, heap),
            Self::List(l) => l.py_setitem(key, value, heap, interns),
            Self::Tuple(t) => t.py_setitem(key, value, heap, interns),
            Self::Dict(d) => d.py_setitem(key, value, heap, interns),
            Self::Counter(c) => c.py_setitem(key, value, heap, interns),
            Self::OrderedDict(od) => od.py_setitem(key, value, heap, interns),
            Self::Deque(d) => d.py_setitem(key, value, heap, interns),
            Self::DefaultDict(dd) => dd.py_setitem(key, value, heap, interns),
            Self::ChainMap(chain) => chain.py_setitem(key, value, heap, interns),
            Self::MappingProxy(mp) => mp.py_setitem(key, value, heap, interns),
            _ => Err(ExcType::type_error_not_sub_assignment(self.py_type(heap))),
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        match self {
            Self::Str(s) => s.py_getattr(attr_id, heap, interns),
            Self::List(l) => l.py_getattr(attr_id, heap, interns),
            Self::Dict(d) => d.py_getattr(attr_id, heap, interns),
            Self::DefaultDict(dd) => dd.py_getattr(attr_id, heap, interns),
            Self::Dataclass(dc) => dc.py_getattr(attr_id, heap, interns),
            Self::Module(m) => Ok(m.py_getattr(attr_id, heap, interns)),
            Self::NamedTuple(nt) => nt.py_getattr(attr_id, heap, interns),
            Self::Slice(s) => s.py_getattr(attr_id, heap, interns),
            Self::Exception(exc) => exc.py_getattr(attr_id, heap, interns),
            Self::Path(p) => p.py_getattr(attr_id, heap, interns),
            Self::ClassObject(cls) => cls.py_getattr(attr_id, heap, interns),
            Self::GenericAlias(ga) => ga.py_getattr(attr_id, heap, interns),
            Self::Instance(inst) => inst.py_getattr(attr_id, heap, interns),
            Self::BoundMethod(bm) => bm.py_getattr(attr_id, heap, interns),
            Self::UserProperty(up) => up.py_getattr(attr_id, heap, interns),
            Self::MappingProxy(mp) => mp.py_getattr(attr_id, heap, interns),
            Self::SlotDescriptor(sd) => sd.py_getattr(attr_id, heap, interns),
            Self::WeakRef(wr) => wr.py_getattr(attr_id, heap, interns),
            Self::ClassSubclasses(cs) => cs.py_getattr(attr_id, heap, interns),
            Self::ClassGetItem(cg) => cg.py_getattr(attr_id, heap, interns),
            Self::FunctionGet(fg) => fg.py_getattr(attr_id, heap, interns),
            Self::Partial(p) => p.py_getattr(attr_id, heap, interns),
            Self::LruCache(lru) => lru.py_getattr(attr_id, heap, interns),
            Self::FunctionWrapper(fw) => fw.py_getattr(attr_id, heap, interns),
            Self::SingleDispatch(dispatcher) => dispatcher.py_getattr(attr_id, heap, interns),
            Self::NamedTupleFactory(factory) => factory.py_getattr(attr_id, heap, interns),
            Self::SuperProxy(sp) => sp.py_getattr(attr_id, heap, interns),
            Self::ReMatch(m) => m.py_getattr(attr_id, heap, interns),
            Self::RePattern(p) => p.py_getattr(attr_id, heap, interns),
            Self::TextWrapper(wrapper) => wrapper.py_getattr(attr_id, heap, interns),
            Self::StdlibObject(obj) => obj.py_getattr(attr_id, heap, interns),
            Self::Hash(hash) => hash.py_getattr(attr_id, heap, interns),
            Self::ZlibCompress(zc) => zc.py_getattr(attr_id, heap, interns),
            Self::ZlibDecompress(zd) => zd.py_getattr(attr_id, heap, interns),
            Self::Fraction(f) => f.py_getattr(attr_id, heap, interns),
            Self::Deque(d) => d.py_getattr(attr_id, heap, interns),
            Self::ChainMap(chain) => chain.py_getattr(attr_id, heap, interns),
            Self::Timedelta(td) => td.py_getattr(attr_id, heap, interns),
            Self::Date(d) => d.py_getattr(attr_id, heap, interns),
            Self::Datetime(dt) => dt.py_getattr(attr_id, heap, interns),
            Self::Time(t) => t.py_getattr(attr_id, heap, interns),
            Self::Timezone(tz) => tz.py_getattr(attr_id, heap, interns),
            Self::Uuid(uuid) => uuid.py_getattr(attr_id, heap, interns),
            Self::SafeUuid(safe) => safe.py_getattr(attr_id, heap, interns),
            // All other types don't support attribute access via py_getattr
            _ => Ok(None),
        }
    }
}

/// Hash caching state stored alongside each heap entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum HashState {
    /// Hash has not yet been computed but the value might be hashable.
    Unknown,
    /// Cached hash value for immutable types that have been hashed at least once.
    Cached(u64),
    /// Value is unhashable (mutable types or tuples containing unhashables).
    Unhashable,
}

impl HashState {
    fn for_data(data: &HeapData) -> Self {
        match data {
            // Cells are hashable by identity (like all Python objects without __hash__ override)
            // FrozenSet is immutable and hashable
            // Range is immutable and hashable
            // Slice is immutable and hashable (like in CPython)
            // LongInt is immutable and hashable
            // NamedTuple is immutable and hashable (like Tuple)
            HeapData::Str(_)
            | HeapData::Bytes(_)
            | HeapData::Bytearray(_)
            | HeapData::Tuple(_)
            | HeapData::NamedTuple(_)
            | HeapData::NamedTupleFactory(_)
            | HeapData::FrozenSet(_)
            | HeapData::Cell(_)
            | HeapData::Closure(_, _, _)
            | HeapData::FunctionDefaults(_, _)
            | HeapData::Range(_)
            | HeapData::Slice(_)
            | HeapData::LongInt(_)
            | HeapData::SlotDescriptor(_)
            | HeapData::BoundMethod(_)
            | HeapData::GenericAlias(_)
            | HeapData::WeakRef(_)
            | HeapData::ClassSubclasses(_)
            | HeapData::ClassGetItem(_)
            | HeapData::FunctionGet(_)
            | HeapData::ZlibCompress(_)
            | HeapData::ZlibDecompress(_)
            | HeapData::LruCache(_)
            | HeapData::FunctionWrapper(_)
            | HeapData::Wraps(_)
            | HeapData::TotalOrderingMethod(_)
            | HeapData::RePattern(_) => Self::Unknown,
            // Dataclass hashability depends on the mutable flag
            HeapData::Dataclass(dc) => {
                if dc.is_frozen() {
                    Self::Unknown
                } else {
                    Self::Unhashable
                }
            }
            // Path is immutable and hashable
            HeapData::Path(_) => Self::Unknown,
            // UUID values are immutable and hashable.
            HeapData::Uuid(uuid) => Self::Cached(uuid.python_hash()),
            // SafeUUID enum members are immutable and hashable.
            HeapData::SafeUuid(safe) => {
                let mut hasher = DefaultHasher::new();
                safe.kind().hash(&mut hasher);
                Self::Cached(hasher.finish())
            }
            // Instances start Unknown - hashability depends on __eq__/__hash__ dunders,
            // checked lazily in get_or_compute_hash. Default: identity hash (like CPython object.__hash__).
            HeapData::Instance(_) => Self::Unknown,
            // Mutable containers, exceptions, iterators, modules, classes, and async types are unhashable
            HeapData::List(_)
            | HeapData::Dict(_)
            | HeapData::Deque(_)
            | HeapData::DefaultDict(_)
            | HeapData::ChainMap(_)
            | HeapData::Counter(_)
            | HeapData::OrderedDict(_)
            | HeapData::Set(_)
            | HeapData::Exception(_)
            | HeapData::Iter(_)
            | HeapData::Module(_)
            | HeapData::Coroutine(_)
            | HeapData::GatherFuture(_)
            | HeapData::ClassObject(_)
            | HeapData::MappingProxy(_)
            | HeapData::SuperProxy(_)
            | HeapData::StaticMethod(_)
            | HeapData::ClassMethod(_)
            | HeapData::UserProperty(_)
            | HeapData::PropertyAccessor(_)
            | HeapData::Hash(_)
            | HeapData::Partial(_)
            | HeapData::CmpToKey(_)
            | HeapData::ItemGetter(_)
            | HeapData::AttrGetter(_)
            | HeapData::MethodCaller(_)
            | HeapData::CachedProperty(_)
            | HeapData::SingleDispatch(_)
            | HeapData::SingleDispatchRegister(_)
            | HeapData::SingleDispatchMethod(_)
            | HeapData::PartialMethod(_)
            | HeapData::Placeholder(_)
            | HeapData::TextWrapper(_)
            | HeapData::ReMatch(_)
            | HeapData::StdlibObject(_)
            | HeapData::Tee(_) => Self::Unhashable,
            HeapData::Generator(_) => Self::Unhashable,
            // ObjectNewImpl is unhashable
            HeapData::ObjectNewImpl(_) => Self::Unhashable,
            // datetime types are immutable and hashable
            HeapData::Timedelta(_) => Self::Unknown,
            HeapData::Date(_) => Self::Unknown,
            HeapData::Datetime(_) => Self::Unknown,
            HeapData::Time(_) => Self::Unknown,
            HeapData::Timezone(_) => Self::Unknown,
            // Decimal is immutable and hashable
            HeapData::Decimal(_) => Self::Unknown,
            HeapData::Fraction(f) => {
                if f.denominator() == &num_bigint::BigInt::from(1) {
                    Self::Cached(LongInt::new(f.numerator().clone()).hash())
                } else {
                    let mut hasher = DefaultHasher::new();
                    discriminant(&HeapData::Fraction(f.clone())).hash(&mut hasher);
                    f.numerator().hash(&mut hasher);
                    f.denominator().hash(&mut hasher);
                    Self::Cached(hasher.finish())
                }
            }
            // Dict views are mutable (reflect dict changes) and unhashable
            HeapData::DictKeys(_) | HeapData::DictValues(_) | HeapData::DictItems(_) => Self::Unhashable,
        }
    }
}

/// A single entry inside the heap arena, storing refcount, payload, and hash metadata.
///
/// Serializes an `AtomicUsize` as a plain `usize` for snapshot compatibility.
fn serialize_atomic<S: serde::Serializer>(val: &AtomicUsize, s: S) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&val.load(Ordering::Relaxed), s)
}

/// Deserializes a plain `usize` into an `AtomicUsize`.
fn deserialize_atomic<'de, D: serde::Deserializer<'de>>(d: D) -> Result<AtomicUsize, D::Error> {
    let v = <usize as serde::Deserialize>::deserialize(d)?;
    Ok(AtomicUsize::new(v))
}

/// The `hash_state` field tracks whether the heap entry is hashable and, if so,
/// caches the computed hash. Mutable types (List, Dict) start as `Unhashable` and
/// will raise TypeError if used as dict keys.
///
/// The `data` field is an Option to support temporary borrowing: when methods like
/// `with_entry_mut` or `call_attr` need mutable access to both the data and the heap,
/// they can `.take()` the data out (leaving `None`), pass `&mut Heap` to user code,
/// then restore the data. This avoids unsafe code while keeping `refcount` accessible
/// for `inc_ref`/`dec_ref` during the borrow.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct HeapValue {
    #[serde(serialize_with = "serialize_atomic", deserialize_with = "deserialize_atomic")]
    refcount: AtomicUsize,
    /// The payload data. Temporarily `None` while borrowed via `with_entry_mut`/`call_attr`.
    data: Option<HeapData>,
    /// Current hashing status / cached hash value
    hash_state: HashState,
}

/// Reference-counted arena that backs all heap-only runtime values.
///
/// Uses a free list to reuse slots from freed values, keeping memory usage
/// constant for long-running loops that repeatedly allocate and free values.
/// When an value is freed via `dec_ref`, its slot ID is added to the free list.
/// New allocations pop from the free list when available, otherwise append.
///
/// Generic over `T: ResourceTracker` to support different resource tracking strategies.
/// When `T = NoLimitTracker` (the default), all resource checks compile away to no-ops.
///
/// Serialization requires `T: Serialize` and `T: Deserialize`. Custom serde implementation
/// handles the Drop constraint by using `std::mem::take` during serialization.
#[derive(Debug)]
pub(crate) struct Heap<T: ResourceTracker> {
    entries: Vec<Option<HeapValue>>,
    /// Per-slot generation counters for Python-visible `id()` values.
    ///
    /// Internal heap identity (`HeapId`) still uses slot indices for fast arena
    /// access. This counter tracks how many times each slot has been reused so
    /// `id()` can stay distinct across reuse.
    slot_id_generations: Vec<u32>,
    /// IDs of freed slots available for reuse. Populated by `dec_ref`, consumed by `allocate`.
    free_list: Vec<HeapId>,
    /// Resource tracker for enforcing limits and scheduling GC.
    tracker: T,
    /// True if reference cycles may exist. Set when a container stores a Ref,
    /// cleared after GC completes. When false, GC can skip mark-sweep entirely.
    may_have_cycles: bool,
    /// Number of GC applicable allocations since the last GC.
    allocations_since_gc: u32,
    /// Cached HeapId for the empty tuple singleton `()`.
    ///
    /// Lazily allocated on first use via `get_or_create_empty_tuple()`.
    /// In Python, `() is ()` is always `True` because empty tuples are interned.
    /// This field enables the same optimization.
    empty_tuple_id: Option<HeapId>,
    /// Monotonic class UID counter for subclass registry entries.
    next_class_uid: u64,
    /// Cached heap IDs for builtin class objects (`object`, `type`).
    builtin_class_ids: AHashMap<Type, HeapId>,
    /// Per-function attribute dictionaries keyed by function object heap ID.
    ///
    /// Function objects (`Closure`/`FunctionDefaults`) keep custom attributes
    /// in these dictionaries to support patterns like decorator counters.
    /// Values are heap IDs pointing to `HeapData::Dict`.
    function_attr_dict_ids: AHashMap<HeapId, HeapId>,
    /// Per-`DefFunction` attribute dictionaries keyed by function definition ID.
    ///
    /// `Value::DefFunction` values are immediate and do not have per-object heap identity.
    /// This map stores attributes keyed by `FunctionId` so `f.__dict__` and custom
    /// function attributes work for non-closure functions without heap wrapping.
    def_function_attr_dict_ids: AHashMap<FunctionId, HeapId>,
    /// Cached HeapId for the ObjectNewImpl singleton.
    ///
    /// Lazily allocated on first use via `get_object_new_impl()`.
    /// This provides the callable for `object.__new__` when accessed via `cls.__new__`.
    object_new_impl_id: Option<HeapId>,
    /// Heap IDs for dicts constructed as `weakref.WeakValueDictionary`.
    weak_value_dict_ids: AHashSet<HeapId>,
    /// Heap IDs for dicts constructed as `weakref.WeakKeyDictionary`.
    weak_key_dict_ids: AHashSet<HeapId>,
    /// Heap IDs for sets constructed as `weakref.WeakSet`.
    weak_set_ids: AHashSet<HeapId>,
    /// Heap IDs for `Partial` objects created by `weakref.finalize()`.
    weak_finalize_partial_ids: AHashSet<HeapId>,
    /// `atexit` callback registry (LIFO order).
    atexit_callbacks: Vec<ExitCallback>,
    /// Remaining depth for data structure operations (repr, eq, hash).
    ///
    /// Uses `Cell` for interior mutability so repr functions (which take `&Heap`)
    /// can track depth without requiring `&mut Heap`. Resets to
    /// `MAX_DATA_RECURSION_DEPTH` on construction and deserialization.
    /// Not serialized — always starts fresh after load.
    data_depth_remaining: Cell<u16>,
}

impl<T: ResourceTracker + serde::Serialize> serde::Serialize for Heap<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Heap", 17)?;
        state.serialize_field("entries", &self.entries)?;
        state.serialize_field("slot_id_generations", &self.slot_id_generations)?;
        state.serialize_field("free_list", &self.free_list)?;
        state.serialize_field("tracker", &self.tracker)?;
        state.serialize_field("may_have_cycles", &self.may_have_cycles)?;
        state.serialize_field("allocations_since_gc", &self.allocations_since_gc)?;
        state.serialize_field("empty_tuple_id", &self.empty_tuple_id)?;
        state.serialize_field("next_class_uid", &self.next_class_uid)?;
        state.serialize_field("builtin_class_ids", &self.builtin_class_ids)?;
        state.serialize_field("function_attr_dict_ids", &self.function_attr_dict_ids)?;
        state.serialize_field("def_function_attr_dict_ids", &self.def_function_attr_dict_ids)?;
        state.serialize_field("object_new_impl_id", &self.object_new_impl_id)?;
        state.serialize_field("weak_value_dict_ids", &self.weak_value_dict_ids)?;
        state.serialize_field("weak_key_dict_ids", &self.weak_key_dict_ids)?;
        state.serialize_field("weak_set_ids", &self.weak_set_ids)?;
        state.serialize_field("weak_finalize_partial_ids", &self.weak_finalize_partial_ids)?;
        state.serialize_field("atexit_callbacks", &self.atexit_callbacks)?;
        state.end()
    }
}

impl<'de, T: ResourceTracker + serde::Deserialize<'de>> serde::Deserialize<'de> for Heap<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct HeapFields<T> {
            entries: Vec<Option<HeapValue>>,
            #[serde(default)]
            slot_id_generations: Vec<u32>,
            free_list: Vec<HeapId>,
            tracker: T,
            may_have_cycles: bool,
            allocations_since_gc: u32,
            empty_tuple_id: Option<HeapId>,
            #[serde(default = "default_next_class_uid")]
            next_class_uid: u64,
            #[serde(default)]
            builtin_class_ids: AHashMap<Type, HeapId>,
            #[serde(default)]
            function_attr_dict_ids: AHashMap<HeapId, HeapId>,
            #[serde(default)]
            def_function_attr_dict_ids: AHashMap<FunctionId, HeapId>,
            #[serde(default)]
            object_new_impl_id: Option<HeapId>,
            #[serde(default)]
            weak_value_dict_ids: AHashSet<HeapId>,
            #[serde(default)]
            weak_key_dict_ids: AHashSet<HeapId>,
            #[serde(default)]
            weak_set_ids: AHashSet<HeapId>,
            #[serde(default)]
            weak_finalize_partial_ids: AHashSet<HeapId>,
            #[serde(default)]
            atexit_callbacks: Vec<ExitCallback>,
        }
        let fields = HeapFields::<T>::deserialize(deserializer)?;
        Ok(Self {
            slot_id_generations: normalize_slot_id_generations(fields.slot_id_generations, fields.entries.len()),
            entries: fields.entries,
            free_list: fields.free_list,
            tracker: fields.tracker,
            may_have_cycles: fields.may_have_cycles,
            allocations_since_gc: fields.allocations_since_gc,
            empty_tuple_id: fields.empty_tuple_id,
            next_class_uid: fields.next_class_uid,
            builtin_class_ids: fields.builtin_class_ids,
            function_attr_dict_ids: fields.function_attr_dict_ids,
            def_function_attr_dict_ids: fields.def_function_attr_dict_ids,
            object_new_impl_id: fields.object_new_impl_id,
            weak_value_dict_ids: fields.weak_value_dict_ids,
            weak_key_dict_ids: fields.weak_key_dict_ids,
            weak_set_ids: fields.weak_set_ids,
            weak_finalize_partial_ids: fields.weak_finalize_partial_ids,
            atexit_callbacks: fields.atexit_callbacks,
            data_depth_remaining: Cell::new(MAX_DATA_RECURSION_DEPTH),
        })
    }
}

/// Provides the default class UID counter for deserialized heaps.
///
/// This keeps older snapshots compatible by supplying a sane starting UID
/// when the serialized payload predates the `next_class_uid` field.
fn default_next_class_uid() -> u64 {
    1
}

/// Normalizes per-slot `id()` generation metadata to match current heap size.
///
/// Older serialized heaps won't contain this field; those slots start at
/// generation 0 so existing snapshots remain compatible.
fn normalize_slot_id_generations(mut generations: Vec<u32>, entry_len: usize) -> Vec<u32> {
    if generations.len() < entry_len {
        generations.resize(entry_len, 0);
    } else if generations.len() > entry_len {
        generations.truncate(entry_len);
    }
    generations
}

/// Converts positional argument vectors to the most compact `ArgValues` variant.
fn args_vec_to_arg_values(mut args: Vec<Value>) -> ArgValues {
    match args.len() {
        0 => ArgValues::Empty,
        1 => ArgValues::One(args.pop().expect("length checked")),
        2 => {
            let second = args.pop().expect("length checked");
            let first = args.pop().expect("length checked");
            ArgValues::Two(first, second)
        }
        _ => ArgValues::ArgsKargs {
            args,
            kwargs: KwargsValues::Empty,
        },
    }
}

/// Drops one stored callback entry, decrementing all held references.
fn drop_exit_callback(callback: ExitCallback, heap: &mut Heap<impl ResourceTracker>) {
    match callback {
        ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.drop_with_heap(heap),
        ExitCallback::Callback { func, args, kwargs } => {
            func.drop_with_heap(heap);
            args.drop_with_heap(heap);
            for (key, value) in kwargs {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
            }
        }
    }
}

macro_rules! take_data {
    ($self:ident, $id:expr, $func_name:literal) => {
        $self
            .entries
            .get_mut($id.index())
            .expect(concat!("Heap::", $func_name, ": slot missing"))
            .as_mut()
            .expect(concat!("Heap::", $func_name, ": object already freed"))
            .data
            .take()
            .expect(concat!("Heap::", $func_name, ": data already borrowed"))
    };
}

macro_rules! restore_data {
    ($self:ident, $id:expr, $new_data:expr, $func_name:literal) => {{
        let entry = $self
            .entries
            .get_mut($id.index())
            .expect(concat!("Heap::", $func_name, ": slot missing"))
            .as_mut()
            .expect(concat!("Heap::", $func_name, ": object already freed"));
        entry.data = Some($new_data);
    }};
}

/// GC interval - run GC every 100,000 applicable allocations.
///
/// This is intentionally infrequent to minimize overhead while still
/// eventually collecting reference cycles.
const GC_INTERVAL: u32 = 100_000;

impl<T: ResourceTracker> Heap<T> {
    /// Creates a new heap with the given resource tracker.
    ///
    /// Use this to create heaps with custom resource limits or GC scheduling.
    pub fn new(capacity: usize, tracker: T) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            slot_id_generations: Vec::with_capacity(capacity),
            free_list: Vec::new(),
            tracker,
            may_have_cycles: false,
            allocations_since_gc: 0,
            empty_tuple_id: None,
            next_class_uid: 1,
            builtin_class_ids: AHashMap::new(),
            function_attr_dict_ids: AHashMap::new(),
            def_function_attr_dict_ids: AHashMap::new(),
            object_new_impl_id: None,
            weak_value_dict_ids: AHashSet::new(),
            weak_key_dict_ids: AHashSet::new(),
            weak_set_ids: AHashSet::new(),
            weak_finalize_partial_ids: AHashSet::new(),
            atexit_callbacks: Vec::new(),
            data_depth_remaining: Cell::new(MAX_DATA_RECURSION_DEPTH),
        }
    }

    /// Creates an independent deep copy of this heap via serialization round-trip.
    ///
    /// The cloned heap is a self-consistent snapshot: every entry, refcount,
    /// free-list slot, and tracker state is duplicated. Heap identities (`HeapId`)
    /// remain valid in the clone because the arena layout is preserved byte-for-byte.
    ///
    /// This is used by `ReplSession::fork()` to branch execution without sharing
    /// mutable heap state. The `data_depth_remaining` counter resets to
    /// `MAX_DATA_RECURSION_DEPTH` in the clone (same as deserialization).
    ///
    /// # Panics
    ///
    /// Panics if serialization or deserialization fails, which should not happen
    /// for a well-formed heap (the same codepath is exercised by snapshot dump/load).
    pub fn deep_clone(&self) -> Self
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let bytes = postcard::to_allocvec(self).expect("heap serialization should not fail");
        postcard::from_bytes(&bytes).expect("heap deserialization should not fail")
    }

    /// Resets the heap for reuse without deallocating backing storage.
    ///
    /// This is the key optimization for reducing VM setup/teardown overhead:
    /// instead of dropping a Heap (which deallocates all Vecs) and creating a
    /// new one (which re-allocates them), we clear the contents while retaining
    /// the allocated capacity. For short-lived executions like `1 + 2`, this
    /// avoids the dominant cost of repeated Vec allocation/deallocation.
    ///
    /// The caller must ensure all heap values have been properly cleaned up
    /// (ref counts decremented) before calling reset. After reset, the heap
    /// is in the same logical state as `Heap::new()` but with pre-allocated
    /// capacity from the previous run.
    ///
    /// # Arguments
    /// * `tracker` - New resource tracker for the next execution
    pub fn reset(&mut self, tracker: T) {
        // Drop atexit callbacks while heap objects are still live.
        let callbacks = std::mem::take(&mut self.atexit_callbacks);
        for callback in callbacks {
            drop_exit_callback(callback, self);
        }

        // Release singleton-owned references before clearing storage.
        if let Some(empty_tuple_id) = self.empty_tuple_id.take() {
            self.dec_ref(empty_tuple_id);
        }
        if let Some(object_new_impl_id) = self.object_new_impl_id.take() {
            self.dec_ref(object_new_impl_id);
        }

        // Release dictionary references owned by function attribute maps.
        // Both maps keep one owned reference per stored dict id.
        let function_attr_dict_ids: Vec<HeapId> = self.function_attr_dict_ids.drain().map(|(_, id)| id).collect();
        for dict_id in function_attr_dict_ids {
            self.dec_ref(dict_id);
        }
        let def_function_attr_dict_ids: Vec<HeapId> =
            self.def_function_attr_dict_ids.drain().map(|(_, id)| id).collect();
        for dict_id in def_function_attr_dict_ids {
            self.dec_ref(dict_id);
        }

        self.entries.clear();
        self.slot_id_generations.clear();
        self.free_list.clear();
        self.tracker = tracker;
        self.may_have_cycles = false;
        self.allocations_since_gc = 0;
        self.next_class_uid = 1;
        self.builtin_class_ids.clear();
        self.weak_value_dict_ids.clear();
        self.weak_key_dict_ids.clear();
        self.weak_set_ids.clear();
        self.weak_finalize_partial_ids.clear();
        self.data_depth_remaining.set(MAX_DATA_RECURSION_DEPTH);
    }

    /// Returns the Python-visible identity for a live heap reference.
    ///
    /// This keeps `id()` stable for the lifetime of an object while ensuring
    /// fresh IDs when freed slots are reused by later allocations.
    #[must_use]
    pub fn public_id_for_ref(&self, id: HeapId) -> usize {
        let slot_generation = self
            .slot_id_generations
            .get(id.index())
            .copied()
            .expect("Heap::public_id_for_ref: slot generation missing");
        let slot_generation = usize::try_from(slot_generation).expect("u32 generation must fit usize");
        #[cfg(target_pointer_width = "64")]
        let payload = (slot_generation << 32) | (id.index() & (u32::MAX as usize));
        #[cfg(target_pointer_width = "32")]
        let payload = id.index().wrapping_mul(0x9E37_79B1_usize)
            ^ usize::try_from(
                u32::try_from(slot_generation)
                    .expect("usize generation must fit u32 on 32-bit")
                    .rotate_left(13),
            )
            .expect("rotated u32 must fit usize");
        let internal_id = crate::value::heap_tagged_id_from_payload(payload);
        crate::value::public_id_from_internal_id(internal_id)
    }

    /// Marks a heap dict as a `weakref.WeakValueDictionary`.
    pub fn mark_weak_value_dict(&mut self, id: HeapId) {
        self.weak_value_dict_ids.insert(id);
    }

    /// Marks a heap dict as a `weakref.WeakKeyDictionary`.
    pub fn mark_weak_key_dict(&mut self, id: HeapId) {
        self.weak_key_dict_ids.insert(id);
    }

    /// Marks a heap set as a `weakref.WeakSet`.
    pub fn mark_weak_set(&mut self, id: HeapId) {
        self.weak_set_ids.insert(id);
    }

    /// Marks a heap partial as a `weakref.finalize` handle.
    pub fn mark_weak_finalize_partial(&mut self, id: HeapId) {
        self.weak_finalize_partial_ids.insert(id);
    }

    /// Registers one `atexit` callback entry.
    pub fn register_atexit_callback(&mut self, callback: ExitCallback) {
        self.atexit_callbacks.push(callback);
    }

    /// Returns number of currently registered `atexit` callbacks.
    #[must_use]
    pub fn atexit_callback_count(&self) -> usize {
        self.atexit_callbacks.len()
    }

    /// Removes all `atexit` callbacks equal to `target`, returning the number removed.
    pub fn unregister_atexit_callbacks(&mut self, target: &Value, interns: &Interns) -> usize {
        let callbacks = std::mem::take(&mut self.atexit_callbacks);
        let mut kept = Vec::with_capacity(callbacks.len());
        let mut removed = 0usize;

        for callback in callbacks {
            let is_match = match &callback {
                ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => value.py_eq(target, self, interns),
                ExitCallback::Callback { func, .. } => func.py_eq(target, self, interns),
            };
            if is_match {
                removed += 1;
                drop_exit_callback(callback, self);
            } else {
                kept.push(callback);
            }
        }

        self.atexit_callbacks = kept;
        removed
    }

    /// Clears all `atexit` callbacks.
    pub fn clear_atexit_callbacks(&mut self) {
        let callbacks = std::mem::take(&mut self.atexit_callbacks);
        for callback in callbacks {
            drop_exit_callback(callback, self);
        }
    }

    /// Returns one pending `atexit` callback invocation, if available (LIFO).
    pub fn take_pending_atexit_callback(&mut self, interns: &Interns) -> RunResult<Option<(Value, ArgValues)>> {
        let Some(callback) = self.atexit_callbacks.pop() else {
            return Ok(None);
        };

        let (func, args, kwargs) = match callback {
            ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => (value, Vec::new(), Vec::new()),
            ExitCallback::Callback { func, args, kwargs } => (func, args, kwargs),
        };

        let arg_values = args_vec_to_arg_values(args);
        let final_args = if kwargs.is_empty() {
            arg_values
        } else {
            let mut kwargs_dict = Dict::new();
            for (key, value) in kwargs {
                kwargs_dict.set(key, value, self, interns)?;
            }
            let positional_args = match arg_values {
                ArgValues::Empty => Vec::new(),
                ArgValues::One(v) => vec![v],
                ArgValues::Two(a, b) => vec![a, b],
                ArgValues::ArgsKargs { args, .. } => args,
                ArgValues::Kwargs(_) => Vec::new(),
            };
            if positional_args.is_empty() {
                ArgValues::Kwargs(crate::args::KwargsValues::Dict(kwargs_dict))
            } else {
                ArgValues::ArgsKargs {
                    args: positional_args,
                    kwargs: crate::args::KwargsValues::Dict(kwargs_dict),
                }
            }
        };
        Ok(Some((func, final_args)))
    }

    /// Returns true if the given heap id is tracked as `WeakValueDictionary`.
    #[must_use]
    pub fn is_weak_value_dict(&self, id: HeapId) -> bool {
        self.weak_value_dict_ids.contains(&id)
    }

    /// Returns true if the given heap id is tracked as `WeakKeyDictionary`.
    #[must_use]
    pub fn is_weak_key_dict(&self, id: HeapId) -> bool {
        self.weak_key_dict_ids.contains(&id)
    }

    /// Returns true if the given heap id is tracked as `WeakSet`.
    #[must_use]
    pub fn is_weak_set(&self, id: HeapId) -> bool {
        self.weak_set_ids.contains(&id)
    }

    /// Returns a reference to the resource tracker.
    pub fn tracker(&self) -> &T {
        &self.tracker
    }

    /// Returns a mutable reference to the resource tracker.
    pub fn tracker_mut(&mut self) -> &mut T {
        &mut self.tracker
    }

    /// Charges one allocation unit for inserting into an existing container.
    ///
    /// This is used by bytecode paths that grow containers in place (for
    /// example list/set/dict comprehension append operations). It enforces
    /// `max_allocations` even when no new heap object is created.
    pub fn on_container_insert(&mut self) -> Result<(), ResourceError> {
        self.tracker.on_container_insert()
    }

    /// Returns a snapshot of the current heap state.
    ///
    /// Iterates all heap slots to count live vs free entries and categorize
    /// live objects by their `HeapData` variant name. The `interned_strings`
    /// parameter is provided by the caller (e.g., `ReplSession`) since the
    /// interner is not owned by the heap.
    ///
    /// Tracker stats (`tracker_allocations`, `tracker_memory_bytes`) are
    /// populated only when the tracker is a `LimitedTracker`; for
    /// `NoLimitTracker` both fields are `None`.
    pub fn heap_stats(&self, interned_strings: usize) -> HeapStats {
        let mut live_objects: usize = 0;
        let mut free_slots: usize = 0;
        let mut objects_by_type: BTreeMap<&'static str, usize> = BTreeMap::new();

        for slot in &self.entries {
            match slot {
                Some(entry) => {
                    if let Some(data) = &entry.data {
                        live_objects += 1;
                        *objects_by_type.entry(data.variant_name()).or_insert(0) += 1;
                    } else {
                        // data temporarily taken (e.g., during with_entry_mut) -- count as live
                        live_objects += 1;
                    }
                }
                None => {
                    free_slots += 1;
                }
            }
        }

        let total_slots = self.entries.len();

        HeapStats {
            live_objects,
            free_slots,
            total_slots,
            objects_by_type,
            interned_strings,
            tracker_allocations: self.tracker.allocation_count(),
            tracker_memory_bytes: self.tracker.current_memory_bytes(),
        }
    }

    /// Attempts to enter one level of data structure recursion (repr, eq, hash).
    ///
    /// Returns `true` if within the depth limit, `false` if the limit is exceeded.
    /// When this returns `true`, the caller **must** call [`data_depth_exit`] on every
    /// return path. When it returns `false`, the depth was not decremented and
    /// `data_depth_exit` must **not** be called.
    ///
    /// Uses interior mutability (`Cell`) so it works through `&Heap` references,
    /// which is necessary for `py_repr_fmt` signatures.
    #[inline]
    pub fn data_depth_enter(&self) -> bool {
        let remaining = self.data_depth_remaining.get();
        if remaining == 0 {
            false
        } else {
            self.data_depth_remaining.set(remaining - 1);
            true
        }
    }

    /// Exits one level of data structure recursion.
    ///
    /// Must be called exactly once for each successful [`data_depth_enter`] call.
    #[inline]
    pub fn data_depth_exit(&self) {
        self.data_depth_remaining.set(self.data_depth_remaining.get() + 1);
    }

    /// Number of entries in the heap
    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// Returns a fresh unique class UID for subclass registry entries.
    pub fn next_class_uid(&mut self) -> u64 {
        let uid = self.next_class_uid;
        self.next_class_uid = self.next_class_uid.saturating_add(1);
        uid
    }

    /// Returns (and caches) a heap-allocated class object for a builtin type.
    ///
    /// This is used to support class-level operations like `type.__subclasses__`
    /// and metaclass bases without changing the `Value::Builtin` representation.
    pub fn builtin_class_id(&mut self, t: Type) -> Result<HeapId, ResourceError> {
        if let Some(id) = self.builtin_class_ids.get(&t) {
            return Ok(*id);
        }

        let class_uid = self.next_class_uid();
        let name = t.to_string();
        let mut bases: Vec<HeapId> = Vec::new();

        let base_type = match t {
            Type::Object => None,
            Type::Type => Some(Type::Object),
            Type::Bool => Some(Type::Int),
            _ => Some(Type::Object),
        };
        if let Some(base_type) = base_type {
            let base_id = self.builtin_class_id(base_type)?;
            bases.push(base_id);
        }

        let class_obj = ClassObject::new(
            name,
            class_uid,
            Value::Builtin(crate::builtins::Builtins::Type(Type::Type)),
            Dict::new(),
            bases.clone(),
            Vec::new(),
        );

        let class_id = self.allocate(HeapData::ClassObject(class_obj))?;

        let mut mro = vec![class_id];
        for &base_id in &bases {
            mro.push(base_id);
            if let HeapData::ClassObject(base_cls) = self.get(base_id) {
                for &mro_id in base_cls.mro().iter().skip(1) {
                    if !mro.contains(&mro_id) {
                        mro.push(mro_id);
                    }
                }
            }
        }

        if let HeapData::ClassObject(cls) = self.get_mut(class_id) {
            cls.set_mro(mro);
        }

        let (instance_has_dict, instance_has_weakref) = match t {
            Type::Type => (true, true),
            Type::Object => (false, false),
            _ => (false, false),
        };
        if let HeapData::ClassObject(cls) = self.get_mut(class_id) {
            cls.set_instance_flags(instance_has_dict, instance_has_weakref);
        }

        for &base_id in &bases {
            if let HeapData::ClassObject(base_cls) = self.get_mut(base_id) {
                base_cls.register_subclass(class_id, class_uid);
            }
        }

        self.builtin_class_ids.insert(t, class_id);
        Ok(class_id)
    }

    /// Returns the builtin `Type` for a cached builtin class HeapId, if any.
    ///
    /// This allows callers to map builtin class wrappers back to their `Type`
    /// representation (e.g., the class object for `list` -> `Type::List`).
    #[must_use]
    pub fn builtin_type_for_class_id(&self, class_id: HeapId) -> Option<Type> {
        self.builtin_class_ids
            .iter()
            .find_map(|(t, id)| if *id == class_id { Some(*t) } else { None })
    }

    /// Returns the attribute dictionary heap ID for a function object, if present.
    #[must_use]
    pub fn function_attr_dict_id(&self, function_id: HeapId) -> Option<HeapId> {
        self.function_attr_dict_ids.get(&function_id).copied()
    }

    /// Returns a copied function attribute value by name, if present.
    ///
    /// The returned `Value` has correct reference count ownership for the caller.
    #[must_use]
    pub fn function_attr_value_copy(&self, function_id: HeapId, attr_name: &str, interns: &Interns) -> Option<Value> {
        let dict_id = *self.function_attr_dict_ids.get(&function_id)?;
        let HeapData::Dict(dict) = self.get(dict_id) else {
            return None;
        };
        let value = dict.get_by_str(attr_name, self, interns)?.copy_for_extend();
        if let Value::Ref(id) = &value {
            self.inc_ref(*id);
        }
        Some(value)
    }

    /// Ensures a function object has an attribute dictionary and returns its heap ID.
    ///
    /// The dictionary is lazily allocated and retained until the function object dies.
    pub fn ensure_function_attr_dict(&mut self, function_id: HeapId) -> Result<HeapId, ResourceError> {
        if let Some(dict_id) = self.function_attr_dict_ids.get(&function_id) {
            return Ok(*dict_id);
        }

        let dict_id = self.allocate(HeapData::Dict(Dict::new()))?;
        // The allocation's initial reference is owned by the function attribute map.
        self.function_attr_dict_ids.insert(function_id, dict_id);
        Ok(dict_id)
    }

    /// Replaces a function object's attribute dictionary reference.
    ///
    /// The map takes ownership of one reference to `dict_id` and releases the
    /// previous dictionary reference if one existed.
    pub fn set_function_attr_dict(&mut self, function_id: HeapId, dict_id: HeapId) {
        self.inc_ref(dict_id);
        if let Some(old_dict_id) = self.function_attr_dict_ids.insert(function_id, dict_id) {
            self.dec_ref(old_dict_id);
        }
    }

    /// Returns the attribute dictionary heap ID for a `DefFunction`, if present.
    #[must_use]
    pub fn def_function_attr_dict_id(&self, function_id: FunctionId) -> Option<HeapId> {
        self.def_function_attr_dict_ids.get(&function_id).copied()
    }

    /// Returns a copied `DefFunction` attribute value by name, if present.
    #[must_use]
    pub fn def_function_attr_value_copy(
        &self,
        function_id: FunctionId,
        attr_name: &str,
        interns: &Interns,
    ) -> Option<Value> {
        let dict_id = *self.def_function_attr_dict_ids.get(&function_id)?;
        let HeapData::Dict(dict) = self.get(dict_id) else {
            return None;
        };
        let value = dict.get_by_str(attr_name, self, interns)?.copy_for_extend();
        if let Value::Ref(id) = &value {
            self.inc_ref(*id);
        }
        Some(value)
    }

    /// Ensures a `DefFunction` has an attribute dictionary and returns its heap ID.
    pub fn ensure_def_function_attr_dict(&mut self, function_id: FunctionId) -> Result<HeapId, ResourceError> {
        if let Some(dict_id) = self.def_function_attr_dict_ids.get(&function_id) {
            return Ok(*dict_id);
        }

        let dict_id = self.allocate(HeapData::Dict(Dict::new()))?;
        // The allocation's initial reference is owned by the def-function attribute map.
        self.def_function_attr_dict_ids.insert(function_id, dict_id);
        Ok(dict_id)
    }

    /// Replaces a `DefFunction` attribute dictionary reference.
    pub fn set_def_function_attr_dict(&mut self, function_id: FunctionId, dict_id: HeapId) {
        self.inc_ref(dict_id);
        if let Some(old_dict_id) = self.def_function_attr_dict_ids.insert(function_id, dict_id) {
            self.dec_ref(old_dict_id);
        }
    }

    /// Marks that a reference cycle may exist in the heap.
    ///
    /// Call this when a container (list, dict, tuple, etc.) stores a reference
    /// to another heap object. This enables the GC to skip mark-sweep entirely
    /// when no cycles are possible.
    #[inline]
    pub fn mark_potential_cycle(&mut self) {
        self.may_have_cycles = true;
    }

    /// Returns the number of GC-tracked allocations since the last garbage collection.
    ///
    /// This counter increments for each allocation of a GC-tracked type (List, Dict, etc.)
    /// and resets to 0 when `collect_garbage` runs. Useful for testing GC behavior.
    #[cfg(feature = "ref-count-return")]
    pub fn get_allocations_since_gc(&self) -> u32 {
        self.allocations_since_gc
    }

    /// Allocates a new heap entry.
    ///
    /// Returns `Err(ResourceError)` if allocation would exceed configured limits.
    /// Use this when you need to handle resource limit errors gracefully.
    ///
    /// Only GC-tracked types (containers that can hold references) count toward the
    /// GC allocation threshold. Leaf types like strings don't trigger GC.
    ///
    /// When allocating a container that contains heap references, marks potential
    /// cycles to enable garbage collection.
    pub fn allocate(&mut self, data: HeapData) -> Result<HeapId, ResourceError> {
        self.tracker.on_allocate(|| data.py_estimate_size())?;
        if data.is_gc_tracked() {
            self.allocations_since_gc = self.allocations_since_gc.wrapping_add(1);
            // Mark potential cycles if this container has heap references.
            // This is essential for types like Dict where setitem doesn't call
            // mark_potential_cycle() - the allocation is the only place to detect refs.
            if data.has_refs() {
                self.may_have_cycles = true;
            }
        }

        let hash_state = HashState::for_data(&data);
        let new_entry = HeapValue {
            refcount: AtomicUsize::new(1),
            data: Some(data),
            hash_state,
        };

        let id = if let Some(id) = self.free_list.pop() {
            // Reuse a freed slot
            let index = id.index();
            self.slot_id_generations[index] = self.slot_id_generations[index].wrapping_add(1);
            self.entries[index] = Some(new_entry);
            id
        } else {
            // No free slots, append new entry
            let id = self.entries.len();
            self.slot_id_generations.push(0);
            self.entries.push(Some(new_entry));
            HeapId(id)
        };

        Ok(id)
    }

    /// Returns the singleton empty tuple, creating it on first use.
    ///
    /// In Python, `() is ()` is always `True` because empty tuples are interned.
    /// This method provides the same optimization by returning the same `HeapId`
    /// for all empty tuple allocations.
    ///
    /// The returned `HeapId` has its reference count incremented, so the caller
    /// owns a reference and must call `dec_ref` when done.
    ///
    /// # Errors
    /// Returns `ResourceError` if allocating the empty tuple fails (only possible
    /// on first call when resource limits are exhausted).
    pub fn get_or_create_empty_tuple(&mut self) -> Result<HeapId, ResourceError> {
        if let Some(id) = self.empty_tuple_id {
            // Return existing singleton with incremented refcount
            self.inc_ref(id);
            Ok(id)
        } else {
            // First use - allocate the empty tuple singleton
            let id = self.allocate(HeapData::Tuple(Tuple::default()))?;
            self.empty_tuple_id = Some(id);
            // Keep an extra reference so the singleton is never freed
            self.inc_ref(id);
            Ok(id)
        }
    }

    /// Returns the HeapId for the ObjectNewImpl singleton, creating it if necessary.
    ///
    /// The ObjectNewImpl provides the callable for `object.__new__` when accessed
    /// via `cls.__new__` in classmethods. It is lazily allocated on first use.
    pub fn get_object_new_impl(&mut self) -> Result<HeapId, ResourceError> {
        if let Some(id) = self.object_new_impl_id {
            // Return existing singleton with incremented refcount
            self.inc_ref(id);
            Ok(id)
        } else {
            // First use - allocate the ObjectNewImpl singleton
            let id = self.allocate(HeapData::ObjectNewImpl(ObjectNewImpl))?;
            self.object_new_impl_id = Some(id);
            // Keep an extra reference so the singleton is never freed
            self.inc_ref(id);
            Ok(id)
        }
    }

    /// Increments the reference count for an existing heap entry.
    ///
    /// Uses interior mutability for the refcount, so only shared access to the heap
    /// is required. This avoids borrow conflicts during attribute and MRO lookups.
    ///
    /// # Panics
    /// Panics if the value ID is invalid or the value has already been freed.
    pub fn inc_ref(&self, id: HeapId) {
        let value = self
            .entries
            .get(id.index())
            .expect("Heap::inc_ref: slot missing")
            .as_ref()
            .expect("Heap::inc_ref: object already freed");
        value.refcount.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrements the reference count and frees the value (plus children) once it hits zero.
    ///
    /// When an value is freed, its slot ID is added to the free list for reuse by
    /// future allocations. Uses recursion for child cleanup - avoiding repeated Vec
    /// allocations and benefiting from call stack locality.
    ///
    /// # Panics
    /// Panics if the value ID is invalid or the value has already been freed.
    pub fn dec_ref(&mut self, id: HeapId) {
        let value = {
            let slot = self.entries.get_mut(id.index()).expect("Heap::dec_ref: slot missing");
            let entry = slot.as_mut().expect("Heap::dec_ref: object already freed");
            let count = entry.refcount.load(Ordering::Relaxed);
            if count > 1 {
                entry.refcount.store(count - 1, Ordering::Relaxed);
                return;
            }
            slot.take().expect("Heap::dec_ref: object already freed")
        };

        // Drop function attribute dictionary owned by this function object, if any.
        if let Some(dict_id) = self.function_attr_dict_ids.remove(&id) {
            self.dec_ref(dict_id);
        }
        self.weak_value_dict_ids.remove(&id);
        self.weak_key_dict_ids.remove(&id);
        self.weak_set_ids.remove(&id);
        self.weak_finalize_partial_ids.remove(&id);

        // refcount == 1, free the value and add slot to free list for reuse
        self.free_list.push(id);

        // Notify tracker of freed memory
        if let Some(ref data) = value.data {
            self.tracker.on_free(|| data.py_estimate_size());
        }

        // Collect child IDs and mark Values as Dereferenced (when ref-count-panic enabled)
        if let Some(mut data) = value.data {
            if let HeapData::Instance(inst) = &data {
                self.clear_instance_weakrefs(id, inst);
            }
            let mut child_ids = Vec::new();
            data.py_dec_ref_ids(&mut child_ids);
            drop(data);
            // Recursively decrement children
            for child_id in child_ids {
                self.dec_ref(child_id);
            }
        }
    }

    /// Clears weakrefs registered on a dying instance.
    ///
    /// This sets each weakref's target to `None` to prevent reuse of freed heap slots
    /// from resurrecting stale weak references.
    fn clear_instance_weakrefs(&mut self, instance_id: HeapId, inst: &Instance) {
        for &weakref_id in inst.weakref_ids() {
            if let Some(HeapData::WeakRef(wr)) = self.get_mut_if_live(weakref_id)
                && wr.target() == Some(instance_id)
            {
                wr.clear();
            }
        }
    }

    /// Returns an immutable reference to the heap data stored at the given ID.
    ///
    /// # Panics
    /// Panics if the value ID is invalid, the value has already been freed,
    /// or the data is currently borrowed via `with_entry_mut`/`call_attr`.
    #[must_use]
    pub fn get(&self, id: HeapId) -> &HeapData {
        self.entries
            .get(id.index())
            .expect("Heap::get: slot missing")
            .as_ref()
            .expect("Heap::get: object already freed")
            .data
            .as_ref()
            .expect("Heap::get: data currently borrowed")
    }

    /// Returns an immutable reference to heap data if the slot is live.
    ///
    /// Unlike `get`, this returns `None` instead of panicking when the slot is
    /// missing, freed, or temporarily borrowed.
    #[must_use]
    pub fn get_if_live(&self, id: HeapId) -> Option<&HeapData> {
        self.entries.get(id.index())?.as_ref()?.data.as_ref()
    }

    /// Returns a mutable reference to the heap data stored at the given ID.
    ///
    /// # Panics
    /// Panics if the value ID is invalid, the value has already been freed,
    /// or the data is currently borrowed via `with_entry_mut`/`call_attr`.
    pub fn get_mut(&mut self, id: HeapId) -> &mut HeapData {
        self.entries
            .get_mut(id.index())
            .expect("Heap::get_mut: slot missing")
            .as_mut()
            .expect("Heap::get_mut: object already freed")
            .data
            .as_mut()
            .expect("Heap::get_mut: data currently borrowed")
    }

    /// Returns a mutable reference to heap data if the slot is live.
    ///
    /// Unlike `get_mut`, this returns `None` instead of panicking when the slot is
    /// missing, freed, or temporarily borrowed.
    #[must_use]
    pub fn get_mut_if_live(&mut self, id: HeapId) -> Option<&mut HeapData> {
        self.entries.get_mut(id.index())?.as_mut()?.data.as_mut()
    }

    /// Returns the current refcount for a live heap value, or 0 when freed.
    fn live_refcount(&self, id: HeapId) -> usize {
        self.entries
            .get(id.index())
            .and_then(Option::as_ref)
            .map_or(0, |entry| entry.refcount.load(Ordering::Relaxed))
    }

    /// Drops stale IDs from weak-container/finalizer tracking sets.
    ///
    /// IDs are retained only when they still point to the expected backing type.
    fn prune_weak_tracking_ids(&mut self) {
        let weak_value_ids = std::mem::take(&mut self.weak_value_dict_ids);
        for id in weak_value_ids {
            if matches!(self.get_if_live(id), Some(HeapData::Dict(_))) {
                self.weak_value_dict_ids.insert(id);
            }
        }

        let weak_key_ids = std::mem::take(&mut self.weak_key_dict_ids);
        for id in weak_key_ids {
            if matches!(self.get_if_live(id), Some(HeapData::Dict(_))) {
                self.weak_key_dict_ids.insert(id);
            }
        }

        let weak_set_ids = std::mem::take(&mut self.weak_set_ids);
        for id in weak_set_ids {
            if matches!(self.get_if_live(id), Some(HeapData::Set(_))) {
                self.weak_set_ids.insert(id);
            }
        }

        let weak_finalize_ids = std::mem::take(&mut self.weak_finalize_partial_ids);
        for id in weak_finalize_ids {
            if let Some(HeapData::Partial(partial)) = self.get_if_live(id)
                && partial.is_weakref_finalize()
            {
                self.weak_finalize_partial_ids.insert(id);
            }
        }
    }

    /// Sets an item in a `weakref.WeakKeyDictionary` while preserving the original key object.
    ///
    /// This mirrors CPython's weak-key behavior where assigning an equal key updates
    /// the existing entry value instead of replacing the stored key identity.
    pub fn set_weak_key_dict_item(
        &mut self,
        dict_id: HeapId,
        key: Value,
        value: Value,
        interns: &Interns,
    ) -> RunResult<()> {
        let replaced = self.with_entry_mut(dict_id, |heap, data| {
            let HeapData::Dict(dict) = data else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error("WeakKeyDictionary backing object is not a dict"));
            };
            dict.set_preserve_existing_equal_key(key, value, heap, interns)
        })?;
        if let Some(old_value) = replaced {
            old_value.drop_with_heap(self);
        }
        Ok(())
    }

    /// Collects dead entries from weak containers tracked by `weakref` constructors.
    ///
    /// Ouros stores weak containers as plain dict/set instances for compatibility.
    /// This pass removes entries whose referents are now only owned by those containers.
    pub fn collect_weak_container_garbage(&mut self, interns: &Interns) -> RunResult<()> {
        self.prune_weak_tracking_ids();

        let weak_value_dict_ids: Vec<HeapId> = self.weak_value_dict_ids.iter().copied().collect();
        for dict_id in weak_value_dict_ids {
            let items = self.with_entry_mut(dict_id, |heap, data| match data {
                HeapData::Dict(dict) => dict.items(heap),
                _ => Vec::new(),
            });
            for (key, value) in items {
                let should_remove = matches!(value, Value::Ref(value_id) if self.live_refcount(value_id) == 2);
                if should_remove {
                    let popped = self.with_entry_mut(dict_id, |heap, data| -> RunResult<Option<(Value, Value)>> {
                        let HeapData::Dict(dict) = data else {
                            return Ok(None);
                        };
                        dict.pop(&key, heap, interns)
                    })?;
                    if let Some((old_key, old_value)) = popped {
                        old_key.drop_with_heap(self);
                        old_value.drop_with_heap(self);
                    }
                }
                key.drop_with_heap(self);
                value.drop_with_heap(self);
            }
        }

        let weak_key_dict_ids: Vec<HeapId> = self.weak_key_dict_ids.iter().copied().collect();
        for dict_id in weak_key_dict_ids {
            let keys = self.with_entry_mut(dict_id, |heap, data| match data {
                HeapData::Dict(dict) => dict.keys(heap),
                _ => Vec::new(),
            });
            for key in keys {
                let should_remove = matches!(key, Value::Ref(key_id) if self.live_refcount(key_id) == 2);
                if should_remove {
                    let popped = self.with_entry_mut(dict_id, |heap, data| -> RunResult<Option<(Value, Value)>> {
                        let HeapData::Dict(dict) = data else {
                            return Ok(None);
                        };
                        dict.pop(&key, heap, interns)
                    })?;
                    if let Some((old_key, old_value)) = popped {
                        old_key.drop_with_heap(self);
                        old_value.drop_with_heap(self);
                    }
                }
                key.drop_with_heap(self);
            }
        }

        let weak_set_ids: Vec<HeapId> = self.weak_set_ids.iter().copied().collect();
        for set_id in weak_set_ids {
            let entries = self.with_entry_mut(set_id, |heap, data| {
                let HeapData::Set(set) = data else {
                    return Vec::new();
                };
                let entries = set.storage().copy_entries();
                SetStorage::inc_refs_for_entries(&entries, heap);
                entries
            });
            for (value, _) in entries {
                let should_remove = matches!(value, Value::Ref(value_id) if self.live_refcount(value_id) == 2);
                if should_remove {
                    self.with_entry_mut(set_id, |heap, data| -> RunResult<()> {
                        let HeapData::Set(set) = data else {
                            return Ok(());
                        };
                        set.discard(&value, heap, interns)
                    })?;
                }
                value.drop_with_heap(self);
            }
        }

        Ok(())
    }

    /// Returns one pending `weakref.finalize()` callback invocation, if available.
    ///
    /// The callback is marked complete before returning so it can run at most once.
    pub fn take_pending_finalize_callback(&mut self, interns: &Interns) -> RunResult<Option<(Value, ArgValues)>> {
        self.prune_weak_tracking_ids();
        let tracked_ids: Vec<HeapId> = self.weak_finalize_partial_ids.iter().copied().collect();
        for partial_id in tracked_ids {
            let callback = self.with_entry_mut(
                partial_id,
                |heap, data| -> RunResult<Option<(Value, Vec<Value>, Vec<(Value, Value)>)>> {
                    let HeapData::Partial(partial) = data else {
                        return Ok(None);
                    };
                    if !partial.is_weakref_finalize() || !partial.weak_finalize_pending() {
                        return Ok(None);
                    }
                    if partial.weak_finalize_alive(heap) {
                        return Ok(None);
                    }

                    partial.mark_weak_finalize_complete();
                    let func = partial.func().clone_with_heap(heap);
                    let args = partial
                        .args()
                        .iter()
                        .map(|arg| arg.clone_with_heap(heap))
                        .collect::<Vec<_>>();
                    let kwargs = partial
                        .kwargs()
                        .iter()
                        .map(|(key, value)| (key.clone_with_heap(heap), value.clone_with_heap(heap)))
                        .collect::<Vec<_>>();
                    Ok(Some((func, args, kwargs)))
                },
            )?;

            let Some((func, args, kwargs)) = callback else {
                continue;
            };

            let call_args = if kwargs.is_empty() {
                args_vec_to_arg_values(args)
            } else {
                let kwargs_dict = Dict::from_pairs(kwargs, self, interns)?;
                ArgValues::ArgsKargs {
                    args,
                    kwargs: KwargsValues::Dict(kwargs_dict),
                }
            };
            return Ok(Some((func, call_args)));
        }
        Ok(None)
    }

    /// Returns the next weakref callback ready to run, if any.
    ///
    /// Weakref callbacks are executed lazily by `gc.collect()` to keep callback
    /// invocation in normal VM call flow. This helper scans for a weakref whose
    /// target is dead and callback is still pending, then returns `(callback, wr)`.
    pub fn take_pending_weakref_callback(&mut self) -> Option<(Value, Value)> {
        for idx in 0..self.entries.len() {
            let should_take = {
                let Some(entry) = self.entries.get(idx).and_then(Option::as_ref) else {
                    continue;
                };
                let Some(HeapData::WeakRef(wr)) = entry.data.as_ref() else {
                    continue;
                };
                let target_dead = if wr.direct_target().is_some() {
                    false
                } else {
                    wr.target()
                        .is_none_or(|target_id| self.get_if_live(target_id).is_none())
                };
                target_dead && wr.has_callback()
            };
            if !should_take {
                continue;
            }

            let Some(entry) = self.entries.get_mut(idx).and_then(Option::as_mut) else {
                continue;
            };
            let Some(HeapData::WeakRef(wr)) = entry.data.as_mut() else {
                continue;
            };
            let Some(callback) = wr.take_callback() else {
                continue;
            };
            wr.clear();

            let weakref_id = HeapId(idx);
            self.inc_ref(weakref_id);
            return Some((callback, Value::Ref(weakref_id)));
        }
        None
    }

    /// Returns or computes the hash for the heap entry at the given ID.
    ///
    /// Hashes are computed lazily on first use and then cached. Returns
    /// Some(hash) for immutable types (Str, Bytes, hashable Tuple), None
    /// for mutable types (List, Dict).
    ///
    /// # Panics
    /// Panics if the value ID is invalid or the value has already been freed.
    pub fn get_or_compute_hash(&mut self, id: HeapId, interns: &Interns) -> Option<u64> {
        let entry = self
            .entries
            .get_mut(id.index())
            .expect("Heap::get_or_compute_hash: slot missing")
            .as_mut()
            .expect("Heap::get_or_compute_hash: object already freed");

        match entry.hash_state {
            HashState::Unhashable => return None,
            HashState::Cached(hash) => return Some(hash),
            HashState::Unknown => {}
        }

        // Guard against stack overflow when hashing deeply nested structures
        // (e.g. tuples of tuples). Returns None (unhashable) at the depth limit.
        if !self.data_depth_enter() {
            return None;
        }
        let hash = self.compute_hash_inner(id, interns);
        self.data_depth_exit();
        hash
    }

    /// Inner hash computation, called after depth guard is acquired.
    ///
    /// Separated from `get_or_compute_hash` so that `data_depth_exit` is called
    /// exactly once regardless of which branch returns.
    fn compute_hash_inner(&mut self, id: HeapId, interns: &Interns) -> Option<u64> {
        let entry = self
            .entries
            .get_mut(id.index())
            .expect("Heap::compute_hash_inner: slot missing")
            .as_mut()
            .expect("Heap::compute_hash_inner: object already freed");

        // Handle Cell specially - uses identity-based hashing (like Python cell objects)
        if let Some(HeapData::Cell(_)) = &entry.data {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            entry.hash_state = HashState::Cached(hash);
            return Some(hash);
        }

        // Handle Instance: check for __eq__ without __hash__ (unhashable), otherwise use identity hash.
        // Proper __hash__ dunder dispatch is done at the VM level via hash() builtin.
        if let Some(HeapData::Instance(inst)) = &entry.data {
            let class_id = inst.class_id();
            let (has_eq, has_hash, hash_is_none) = match self.get(class_id) {
                HeapData::ClassObject(cls) => {
                    let hash_attr = cls.namespace().get_by_str("__hash__", self, interns);
                    let has_hash = hash_attr.is_some();
                    let hash_is_none = matches!(hash_attr, Some(Value::None));
                    let has_eq = cls.namespace().get_by_str("__eq__", self, interns).is_some();
                    (has_eq, has_hash, hash_is_none)
                }
                _ => (false, false, false),
            };

            let entry = self
                .entries
                .get_mut(id.index())
                .expect("Heap::compute_hash_inner: slot missing after instance check")
                .as_mut()
                .expect("Heap::compute_hash_inner: object freed during instance check");

            if hash_is_none || (has_eq && !has_hash) {
                entry.hash_state = HashState::Unhashable;
                return None;
            }

            // Default: identity-based hash (like Python's object.__hash__)
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            entry.hash_state = HashState::Cached(hash);
            return Some(hash);
        }

        // Slot descriptors are hashable by identity (like CPython descriptor objects).
        if let Some(HeapData::SlotDescriptor(_)) = &entry.data {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            entry.hash_state = HashState::Cached(hash);
            return Some(hash);
        }

        // Bound methods are hashable by identity (like CPython method objects).
        if let Some(HeapData::BoundMethod(_)) = &entry.data {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            entry.hash_state = HashState::Cached(hash);
            return Some(hash);
        }

        // Weak references hash like their target (for ref objects) or are unhashable (for proxies).
        if matches!(&entry.data, Some(HeapData::WeakRef(_))) {
            // Take the value out to avoid borrow conflicts while computing a target hash.
            let mut data = entry.data.take().expect("Heap::compute_hash_inner: data borrowed");
            let hash = match &mut data {
                HeapData::WeakRef(wr) => {
                    if wr.is_proxy() {
                        None
                    } else if let Some(cached) = wr.cached_hash() {
                        Some(cached)
                    } else if let Some(target) = wr.direct_target() {
                        let computed = target.py_hash(self, interns);
                        if let Some(computed_hash) = computed {
                            wr.set_cached_hash(computed_hash);
                        }
                        computed
                    } else if let Some(target_id) = wr.target() {
                        if self.get_if_live(target_id).is_some() {
                            let computed = self.get_or_compute_hash(target_id, interns);
                            if let Some(computed_hash) = computed {
                                wr.set_cached_hash(computed_hash);
                            }
                            computed
                        } else {
                            wr.clear();
                            wr.cached_hash()
                        }
                    } else {
                        wr.cached_hash()
                    }
                }
                _ => unreachable!(),
            };

            let entry = self
                .entries
                .get_mut(id.index())
                .expect("Heap::compute_hash_inner: slot missing after weakref hash")
                .as_mut()
                .expect("Heap::compute_hash_inner: object freed during weakref hash");
            entry.data = Some(data);
            entry.hash_state = match hash {
                Some(hash) => HashState::Cached(hash),
                None => HashState::Unhashable,
            };
            return hash;
        }

        // Builtin wrapper callables are hashable by identity.
        if let Some(
            HeapData::ClassSubclasses(_)
            | HeapData::ClassGetItem(_)
            | HeapData::FunctionGet(_)
            | HeapData::ZlibCompress(_)
            | HeapData::ZlibDecompress(_)
            | HeapData::NamedTupleFactory(_)
            | HeapData::LruCache(_)
            | HeapData::FunctionWrapper(_)
            | HeapData::Wraps(_)
            | HeapData::TotalOrderingMethod(_),
        ) = &entry.data
        {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            let hash = hasher.finish();
            entry.hash_state = HashState::Cached(hash);
            return Some(hash);
        }

        // Compute hash lazily - need to temporarily take data to avoid borrow conflict
        let data = entry.data.take().expect("Heap::compute_hash_inner: data borrowed");
        let hash = data.compute_hash_if_immutable(self, interns);

        // Restore data and cache the hash if computed
        let entry = self
            .entries
            .get_mut(id.index())
            .expect("Heap::compute_hash_inner: slot missing after compute")
            .as_mut()
            .expect("Heap::compute_hash_inner: object freed during compute");
        entry.data = Some(data);
        entry.hash_state = match hash {
            Some(value) => HashState::Cached(value),
            None => HashState::Unhashable,
        };
        hash
    }

    /// Caches a pre-computed hash value for a heap entry.
    ///
    /// This is used by the VM to cache the result of calling `__hash__()` on an instance
    /// so that subsequent dict operations (which call `get_or_compute_hash`) will use the
    /// Python-level hash rather than identity-based hash.
    ///
    /// # Panics
    /// Panics if the value ID is invalid or the value has already been freed.
    pub fn set_cached_hash(&mut self, id: HeapId, hash: u64) {
        let entry = self
            .entries
            .get_mut(id.index())
            .expect("Heap::set_cached_hash: slot missing")
            .as_mut()
            .expect("Heap::set_cached_hash: object already freed");
        entry.hash_state = HashState::Cached(hash);
    }

    /// Returns true if the heap entry at the given ID has its hash already cached.
    pub fn has_cached_hash(&self, id: HeapId) -> bool {
        let entry = self
            .entries
            .get(id.index())
            .expect("Heap::has_cached_hash: slot missing")
            .as_ref()
            .expect("Heap::has_cached_hash: object already freed");
        matches!(entry.hash_state, HashState::Cached(_))
    }

    /// Calls an attribute on the heap entry, returning an `AttrCallResult` that may signal
    /// OS or external calls.
    ///
    /// Temporarily takes ownership of the payload to avoid borrow conflicts when attribute
    /// implementations also need mutable heap access (e.g. for refcounting).
    ///
    /// Returns `AttrCallResult` which may be:
    /// - `Value(v)` - Method completed synchronously with value `v`
    /// - `OsCall(func, args)` - Method needs OS operation; VM should yield to host
    /// - `ExternalCall(id, args)` - Method needs external function call
    pub fn call_attr_raw(
        &mut self,
        id: HeapId,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<AttrCallResult> {
        // Take data out so the borrow of self.entries ends
        let mut data = take_data!(self, id, "call_attr");

        let result = data.py_call_attr_raw(self, attr, args, interns, Some(id));

        // Restore data
        restore_data!(self, id, data, "call_attr_raw");
        result
    }

    /// Gives mutable access to a heap entry while allowing reentrant heap usage
    /// inside the closure (e.g. to read other values or allocate results).
    ///
    /// The data is temporarily taken from the heap entry, so the closure can safely
    /// mutate both the entry data and the heap (e.g. to allocate new values).
    /// The data is automatically restored after the closure completes.
    pub fn with_entry_mut<F, R>(&mut self, id: HeapId, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut HeapData) -> R,
    {
        // Take data out in a block so the borrow of self.entries ends
        let mut data = take_data!(self, id, "with_entry_mut");

        let result = f(self, &mut data);

        // Restore data
        restore_data!(self, id, data, "with_entry_mut");
        result
    }

    /// Temporarily takes ownership of two heap entries so their data can be borrowed
    /// simultaneously while still permitting mutable access to the heap (e.g. to
    /// allocate results). Automatically restores both entries after the closure
    /// finishes executing.
    pub fn with_two<F, R>(&mut self, left: HeapId, right: HeapId, f: F) -> R
    where
        F: FnOnce(&mut Self, &HeapData, &HeapData) -> R,
    {
        if left == right {
            // Same value - take data once and pass it twice
            let data = take_data!(self, left, "with_two");

            let result = f(self, &data, &data);

            restore_data!(self, left, data, "with_two");
            result
        } else {
            // Different values - take both
            let left_data = take_data!(self, left, "with_two (left)");
            let right_data = take_data!(self, right, "with_two (right)");

            let result = f(self, &left_data, &right_data);

            // Restore in reverse order
            restore_data!(self, right, right_data, "with_two (right)");
            restore_data!(self, left, left_data, "with_two (left)");
            result
        }
    }

    /// Returns the reference count for the heap entry at the given ID.
    ///
    /// This is primarily used for testing reference counting behavior.
    ///
    /// # Panics
    /// Panics if the value ID is invalid or the value has already been freed.
    #[must_use]
    pub fn get_refcount(&self, id: HeapId) -> usize {
        self.entries
            .get(id.index())
            .expect("Heap::get_refcount: slot missing")
            .as_ref()
            .expect("Heap::get_refcount: object already freed")
            .refcount
            .load(Ordering::Relaxed)
    }

    /// Returns the number of live (non-freed) values on the heap.
    ///
    /// This is primarily used for testing to verify that all heap entries
    /// are accounted for in reference count tests.
    ///
    /// Excludes the empty tuple singleton since it's an internal optimization
    /// detail that persists even when not explicitly used by user code.
    /// TEMPORARY DEBUG: prints all remaining heap entries with refcounts.
    #[cfg(feature = "ref-count-panic")]
    pub fn debug_dump_remaining(&self) {
        eprintln!("[HEAP DUMP] Remaining entries before heap drop:");
        for (i, slot) in self.entries.iter().enumerate() {
            if let Some(entry) = slot {
                let type_name = entry.data.as_ref().map_or("(data taken)", |d| d.variant_name());
                eprintln!(
                    "  HeapId({i}): type={type_name}, refcount={}",
                    entry.refcount.load(Ordering::Relaxed)
                );
            }
        }
    }

    #[must_use]
    #[cfg(feature = "ref-count-return")]
    pub fn entry_count(&self) -> usize {
        let count = self.entries.iter().filter(|o| o.is_some()).count();
        // Subtract 1 for the empty tuple singleton if it exists
        if self.empty_tuple_id.is_some() {
            count.saturating_sub(1)
        } else {
            count
        }
    }

    /// Gets the value inside a cell, cloning it with proper refcount handling.
    ///
    /// Uses `clone_with_heap` to properly handle all value types including closures,
    /// which need their captured cell refcounts incremented.
    ///
    /// # Errors
    /// Returns an internal error if the entry is not a cell.
    ///
    /// # Panics
    /// Panics if the ID is invalid or the value has been freed.
    pub fn get_cell_value(&mut self, id: HeapId) -> RunResult<Value> {
        // Take the data out to avoid borrow conflicts when cloning
        let data = take_data!(self, id, "get_cell_value");

        let result = match &data {
            HeapData::Cell(v) => Ok(v.clone_with_heap(self)),
            _ => Err(RunError::internal("Heap::get_cell_value: entry is not a Cell")),
        };

        // Restore data before returning
        restore_data!(self, id, data, "get_cell_value");

        result
    }

    /// Sets the value inside a cell, properly dropping the old value.
    ///
    /// # Errors
    /// Returns an internal error if the entry is not a cell.
    ///
    /// # Panics
    /// Panics if the ID is invalid or the value has been freed.
    pub fn set_cell_value(&mut self, id: HeapId, value: Value) -> RunResult<()> {
        // Take the data out to avoid borrow conflicts
        let mut data = take_data!(self, id, "set_cell_value");

        if let HeapData::Cell(old_value) = &mut data {
            // Swap in the new value
            let old = std::mem::replace(old_value, value);
            // Restore data first, then drop old value
            restore_data!(self, id, data, "set_cell_value");
            old.drop_with_heap(self);
            Ok(())
        } else {
            // Value was moved into this function and would otherwise be dropped
            // without heap cleanup.
            value.drop_with_heap(self);
            restore_data!(self, id, data, "set_cell_value");
            Err(RunError::internal("Heap::set_cell_value: entry is not a Cell"))
        }
    }

    /// Helper for List in-place add: extends the destination vec with items from a heap list.
    ///
    /// This method exists to work around borrow checker limitations when List::py_iadd
    /// needs to read from one heap entry while extending another. By keeping both
    /// the read and the refcount increments within Heap's impl block, we can use the
    /// take/restore pattern to avoid the lifetime propagation issues.
    ///
    /// Returns `true` if successful, `false` if the source ID is not a List.
    pub fn iadd_extend_list(&mut self, source_id: HeapId, dest: &mut Vec<Value>) -> bool {
        // Take the source data temporarily
        let source_data = take_data!(self, source_id, "iadd_extend_list");

        if let HeapData::List(list) = &source_data {
            // Copy items and track which refs need incrementing
            let items: Vec<Value> = list.as_vec().iter().map(Value::copy_for_extend).collect();
            let ref_ids: Vec<HeapId> = items.iter().filter_map(Value::ref_id).collect();

            // Restore source data before mutating heap (inc_ref needs it)
            restore_data!(self, source_id, source_data, "iadd_extend_list");

            // Now increment refcounts
            for id in ref_ids {
                self.inc_ref(id);
            }

            // Extend destination
            dest.extend(items);
            true
        } else {
            // Not a list, restore and return false
            restore_data!(self, source_id, source_data, "iadd_extend_list");
            false
        }
    }

    /// Multiplies (repeats) a sequence by an integer count.
    ///
    /// This method handles sequence repetition for Python's `*` operator when applied
    /// to sequences (str, bytes, list, tuple). It creates a new heap-allocated sequence
    /// with the elements repeated `count` times.
    ///
    /// # Arguments
    /// * `id` - HeapId of the sequence to repeat
    /// * `count` - Number of times to repeat (0 returns empty sequence)
    ///
    /// # Returns
    /// * `Ok(Some(Value))` - The new repeated sequence
    /// * `Ok(None)` - If the heap entry is not a sequence type
    /// * `Err` - If allocation fails due to resource limits
    pub fn mult_sequence(&mut self, id: HeapId, count: usize) -> RunResult<Option<Value>> {
        // Take the data out to avoid borrow conflicts
        let data = take_data!(self, id, "mult_sequence");

        match &data {
            HeapData::Str(s) => {
                // Pre-check estimated result size before allocating
                let estimated = s.as_str().len().saturating_mul(count);
                if estimated > LARGE_RESULT_THRESHOLD {
                    self.tracker().check_large_result(estimated)?;
                }
                let repeated = s.as_str().repeat(count);
                restore_data!(self, id, data, "mult_sequence");
                Ok(Some(Value::Ref(self.allocate(HeapData::Str(repeated.into()))?)))
            }
            HeapData::Bytes(b) => {
                // Pre-check estimated result size before allocating
                let estimated = b.as_slice().len().saturating_mul(count);
                if estimated > LARGE_RESULT_THRESHOLD {
                    self.tracker().check_large_result(estimated)?;
                }
                let repeated = b.as_slice().repeat(count);
                restore_data!(self, id, data, "mult_sequence");
                Ok(Some(Value::Ref(self.allocate(HeapData::Bytes(repeated.into()))?)))
            }
            HeapData::Bytearray(b) => {
                // Pre-check estimated result size before allocating
                let estimated = b.as_slice().len().saturating_mul(count);
                if estimated > LARGE_RESULT_THRESHOLD {
                    self.tracker().check_large_result(estimated)?;
                }
                let repeated = b.as_slice().repeat(count);
                restore_data!(self, id, data, "mult_sequence");
                Ok(Some(Value::Ref(
                    self.allocate(HeapData::Bytearray(Bytes::new(repeated)))?,
                )))
            }
            HeapData::List(list) => {
                if count == 0 {
                    restore_data!(self, id, data, "mult_sequence");
                    Ok(Some(Value::Ref(self.allocate(HeapData::List(List::new(Vec::new())))?)))
                } else {
                    // Copy items and track which refs need incrementing
                    let items: Vec<Value> = list.as_vec().iter().map(Value::copy_for_extend).collect();
                    let ref_ids: Vec<HeapId> = items.iter().filter_map(Value::ref_id).collect();
                    let original_len = items.len();

                    // Restore data before heap operations
                    restore_data!(self, id, data, "mult_sequence");

                    // Build the repeated list with overflow check
                    let capacity = original_len
                        .checked_mul(count)
                        .ok_or_else(ExcType::overflow_repeat_count)?;

                    // Pre-check estimated result size before allocating
                    let estimated = capacity.saturating_mul(std::mem::size_of::<Value>());
                    if estimated > LARGE_RESULT_THRESHOLD {
                        self.tracker().check_large_result(estimated)?;
                    }

                    // Now increment refcounts for each copy we'll make
                    // We need (count) copies of each ref
                    for ref_id in &ref_ids {
                        for _ in 0..count {
                            self.inc_ref(*ref_id);
                        }
                    }

                    let mut result = Vec::with_capacity(capacity);
                    for _ in 0..count {
                        for item in &items {
                            result.push(item.copy_for_extend());
                        }
                    }

                    // Manually forget the items vec to avoid Drop panic
                    // The values have been copied to result with proper refcounts
                    std::mem::forget(items);

                    Ok(Some(Value::Ref(self.allocate(HeapData::List(List::new(result)))?)))
                }
            }
            HeapData::Deque(deque) => {
                let maxlen = deque.maxlen();
                if count == 0 {
                    restore_data!(self, id, data, "mult_sequence");
                    let empty = Deque::from_vec_deque_with_maxlen(VecDeque::new(), maxlen);
                    Ok(Some(Value::Ref(self.allocate(HeapData::Deque(empty))?)))
                } else {
                    let items: Vec<Value> = deque.iter().map(Value::copy_for_extend).collect();
                    let ref_ids: Vec<HeapId> = items.iter().filter_map(Value::ref_id).collect();
                    let original_len = items.len();

                    restore_data!(self, id, data, "mult_sequence");

                    let capacity = original_len
                        .checked_mul(count)
                        .ok_or_else(ExcType::overflow_repeat_count)?;

                    let estimated = capacity.saturating_mul(std::mem::size_of::<Value>());
                    if estimated > LARGE_RESULT_THRESHOLD {
                        self.tracker().check_large_result(estimated)?;
                    }

                    for ref_id in &ref_ids {
                        for _ in 0..count {
                            self.inc_ref(*ref_id);
                        }
                    }

                    let mut result = VecDeque::with_capacity(capacity);
                    for _ in 0..count {
                        for item in &items {
                            result.push_back(item.copy_for_extend());
                        }
                    }

                    if let Some(maxlen) = maxlen {
                        while result.len() > maxlen {
                            if let Some(removed) = result.pop_front() {
                                removed.drop_with_heap(self);
                            }
                        }
                    }

                    std::mem::forget(items);

                    let repeated = Deque::from_vec_deque_with_maxlen(result, maxlen);
                    Ok(Some(Value::Ref(self.allocate(HeapData::Deque(repeated))?)))
                }
            }
            HeapData::Tuple(tuple) => {
                if count == 0 {
                    restore_data!(self, id, data, "mult_sequence");
                    // Use empty tuple singleton
                    Ok(Some(Value::Ref(self.get_or_create_empty_tuple()?)))
                } else {
                    // Copy items and track which refs need incrementing
                    let items: Vec<Value> = tuple.as_vec().iter().map(Value::copy_for_extend).collect();
                    let ref_ids: Vec<HeapId> = items.iter().filter_map(Value::ref_id).collect();
                    let original_len = items.len();

                    // Restore data before heap operations
                    restore_data!(self, id, data, "mult_sequence");

                    // Build the repeated tuple with overflow check
                    let capacity = original_len
                        .checked_mul(count)
                        .ok_or_else(ExcType::overflow_repeat_count)?;

                    // Pre-check estimated result size before allocating
                    let estimated = capacity.saturating_mul(std::mem::size_of::<Value>());
                    if estimated > LARGE_RESULT_THRESHOLD {
                        self.tracker().check_large_result(estimated)?;
                    }

                    // Now increment refcounts for each copy we'll make
                    // We need (count) copies of each ref
                    for ref_id in &ref_ids {
                        for _ in 0..count {
                            self.inc_ref(*ref_id);
                        }
                    }

                    let mut result = SmallVec::with_capacity(capacity);
                    for _ in 0..count {
                        for item in &items {
                            result.push(item.copy_for_extend());
                        }
                    }

                    // Manually forget the items vec to avoid Drop panic
                    std::mem::forget(items);

                    Ok(Some(allocate_tuple(result, self)?))
                }
            }
            _ => {
                // Dicts, Cells, Callables, Functions and Closures don't support multiplication
                restore_data!(self, id, data, "mult_sequence");
                Ok(None)
            }
        }
    }

    /// Returns whether garbage collection should run.
    ///
    /// True if reference cycles count exist in the heap
    /// and the number of allocations since the last GC exceeds the interval.
    #[inline]
    pub fn should_gc(&self) -> bool {
        self.may_have_cycles && self.allocations_since_gc >= GC_INTERVAL
    }

    /// Runs mark-sweep garbage collection to free unreachable cycles.
    ///
    /// This method takes a closure that provides an iterator of root HeapIds
    /// (typically from Namespaces). It marks all reachable objects starting
    /// from roots, then sweeps (frees) any unreachable objects.
    ///
    /// This is necessary because reference counting alone cannot free cycles
    /// where objects reference each other but are unreachable from the program.
    ///
    /// # Caller Responsibility
    /// The caller should check `should_gc()` before calling this method.
    /// If no cycles are possible, the caller can skip GC entirely.
    ///
    /// # Arguments
    /// * `root` - HeapIds that are roots
    pub fn collect_garbage(&mut self, root: Vec<HeapId>) {
        // Mark phase: collect all reachable IDs using BFS
        // Use Vec<bool> instead of HashSet for O(1) operations without hashing overhead
        let mut reachable: Vec<bool> = vec![false; self.entries.len()];
        let mut work_list: Vec<HeapId> = root;

        while let Some(id) = work_list.pop() {
            let idx = id.index();
            // Skip if out of bounds or already visited
            if idx >= reachable.len() || reachable[idx] {
                continue;
            }
            reachable[idx] = true;

            // Add children to work list
            if let Some(Some(entry)) = self.entries.get(idx)
                && let Some(ref data) = entry.data
            {
                collect_child_ids(data, &mut work_list);
            }
        }

        // Sweep phase: free unreachable values
        for (id, value) in self.entries.iter_mut().enumerate() {
            if reachable[id] {
                continue;
            }

            // This entry is unreachable - free it
            if let Some(value) = value.take() {
                // Notify tracker of freed memory
                if let Some(ref data) = value.data {
                    self.tracker.on_free(|| data.py_estimate_size());
                }

                self.free_list.push(HeapId(id));

                // Mark Values as Dereferenced when ref-count-panic is enabled
                #[cfg(feature = "ref-count-panic")]
                if let Some(mut data) = value.data {
                    data.py_dec_ref_ids(&mut Vec::new());
                }
            }
        }

        // Reset cycle flag after GC - cycles have been collected
        self.may_have_cycles = false;
        self.allocations_since_gc = 0;
    }
}

/// Collects child HeapIds from a HeapData value for GC traversal.
fn collect_child_ids(data: &HeapData, work_list: &mut Vec<HeapId>) {
    match data {
        // Leaf types with no heap references
        HeapData::Str(_)
        | HeapData::Bytes(_)
        | HeapData::Bytearray(_)
        | HeapData::Range(_)
        | HeapData::Exception(_)
        | HeapData::LongInt(_)
        | HeapData::Slice(_)
        | HeapData::Path(_)
        | HeapData::SlotDescriptor(_)
        | HeapData::Hash(_)
        | HeapData::ZlibCompress(_)
        | HeapData::ZlibDecompress(_)
        | HeapData::ReMatch(_)
        | HeapData::RePattern(_)
        | HeapData::Fraction(_)
        | HeapData::Uuid(_)
        | HeapData::SafeUuid(_) => {}
        HeapData::StdlibObject(obj) => {
            if !obj.has_refs() {
                return;
            }
            obj.collect_ref_ids(work_list);
        }
        HeapData::List(list) => {
            // Skip iteration if no refs - major GC optimization for lists of primitives
            if !list.contains_refs() {
                return;
            }
            for value in list.as_vec() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Tuple(tuple) => {
            // Skip iteration if no refs - GC optimization for tuples of primitives
            if !tuple.contains_refs() {
                return;
            }
            for value in tuple.as_vec() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::NamedTuple(nt) => {
            // Skip iteration if no refs - GC optimization for namedtuples of primitives
            if !nt.contains_refs() {
                return;
            }
            for value in nt.as_vec() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Dict(dict) => {
            // Skip iteration if no refs - major GC optimization for dicts of primitives
            if !dict.has_refs() {
                return;
            }
            for (k, v) in dict {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Deque(deque) => {
            // Skip iteration if no refs - GC optimization for deques of primitives
            if !deque.contains_refs() {
                return;
            }
            for value in deque.iter() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::DefaultDict(dd) => {
            // Skip iteration if no refs - GC optimization for defaultdicts of primitives
            if !dd.has_refs() {
                return;
            }
            let dict = dd.dict();
            for (k, v) in &*dict {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
            // Also check the default factory
            if let Some(Value::Ref(id)) = dd.default_factory() {
                work_list.push(*id);
            }
        }
        HeapData::ChainMap(chain_map) => {
            if !chain_map.has_refs() {
                return;
            }
            for map in chain_map.maps() {
                if let Value::Ref(id) = map {
                    work_list.push(*id);
                }
            }
            for (key, value) in chain_map.flat() {
                if let Value::Ref(id) = key {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Set(set) => {
            for value in set.storage().iter() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::FrozenSet(frozenset) => {
            for value in frozenset.storage().iter() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Closure(_, cells, defaults) => {
            // Add captured cells to work list
            for cell_id in cells {
                work_list.push(*cell_id);
            }
            // Add default values that are heap references
            for default in defaults {
                if let Value::Ref(id) = default {
                    work_list.push(*id);
                }
            }
        }
        HeapData::FunctionDefaults(_, defaults) => {
            // Add default values that are heap references
            for default in defaults {
                if let Value::Ref(id) = default {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Cell(value) => {
            // Cell can contain a reference to another heap value
            if let Value::Ref(id) = value {
                work_list.push(*id);
            }
        }
        HeapData::Dataclass(dc) => {
            // Dataclass attrs are stored in a Dict - iterate through entries
            for (k, v) in dc.attrs() {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Iter(iter) => {
            // Iterator holds a reference to the iterable being iterated
            if let Value::Ref(id) = iter.value() {
                work_list.push(*id);
            }
        }
        HeapData::Module(m) => {
            // Module attrs can contain references to heap values
            if !m.has_refs() {
                return;
            }
            for (k, v) in m.attrs() {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Coroutine(coro) => {
            // Add captured cells to work list
            for cell_id in &coro.frame_cells {
                work_list.push(*cell_id);
            }
            // Add namespace values that are heap references
            for value in &coro.namespace {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::GatherFuture(gather) => {
            // Add coroutine HeapIds to work list
            for item in &gather.items {
                if let GatherItem::Coroutine(coro_id) = item {
                    work_list.push(*coro_id);
                }
            }
            // Add result values that are heap references
            for result in gather.results.iter().flatten() {
                if let Value::Ref(id) = result {
                    work_list.push(*id);
                }
            }
        }
        HeapData::ClassObject(cls) => {
            // Class namespace can contain references to heap values (methods, closures)
            for (k, v) in cls.namespace() {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
            if let Value::Ref(id) = cls.metaclass() {
                work_list.push(*id);
            }
            // Base classes and MRO are heap references
            for &base_id in cls.bases() {
                work_list.push(base_id);
            }
            for &mro_id in cls.mro() {
                work_list.push(mro_id);
            }
        }
        HeapData::MappingProxy(mp) => {
            work_list.push(mp.class_id());
        }
        HeapData::Instance(inst) => {
            // Instance always has class_id ref, plus attrs may have refs
            work_list.push(inst.class_id());
            if let Some(attrs_id) = inst.attrs_id() {
                work_list.push(attrs_id);
            }
            for value in inst.slot_values() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
            // Weakrefs are not strong references; do not mark them here.
        }
        HeapData::BoundMethod(bm) => {
            if let Value::Ref(id) = bm.func() {
                work_list.push(*id);
            }
            if let Value::Ref(id) = bm.self_arg() {
                work_list.push(*id);
            }
        }
        HeapData::SuperProxy(sp) => {
            work_list.push(sp.instance_id());
            work_list.push(sp.current_class_id());
        }
        HeapData::StaticMethod(sm) => {
            if let Value::Ref(id) = sm.func() {
                work_list.push(*id);
            }
        }
        HeapData::ClassMethod(cm) => {
            if let Value::Ref(id) = cm.func() {
                work_list.push(*id);
            }
        }
        HeapData::UserProperty(up) => {
            if let Some(Value::Ref(id)) = up.fget() {
                work_list.push(*id);
            }
            if let Some(Value::Ref(id)) = up.fset() {
                work_list.push(*id);
            }
            if let Some(Value::Ref(id)) = up.fdel() {
                work_list.push(*id);
            }
            if let Some(Value::Ref(id)) = up.doc() {
                work_list.push(*id);
            }
        }
        HeapData::PropertyAccessor(pa) => {
            let (fget, fset, fdel, doc) = pa.parts();
            if let Some(Value::Ref(id)) = fget {
                work_list.push(*id);
            }
            if let Some(Value::Ref(id)) = fset {
                work_list.push(*id);
            }
            if let Some(Value::Ref(id)) = fdel {
                work_list.push(*id);
            }
            if let Some(Value::Ref(id)) = doc {
                work_list.push(*id);
            }
        }
        HeapData::GenericAlias(ga) => {
            if let Value::Ref(id) = ga.origin() {
                work_list.push(*id);
            }
            for value in ga.args() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
            for value in ga.parameters() {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::ClassSubclasses(cs) => {
            work_list.push(cs.class_id());
        }
        HeapData::ClassGetItem(cg) => {
            work_list.push(cg.class_id());
        }
        HeapData::FunctionGet(fg) => {
            if let Value::Ref(id) = fg.func() {
                work_list.push(*id);
            }
        }
        HeapData::Partial(p) => {
            if let Value::Ref(id) = p.func() {
                work_list.push(*id);
            }
            for arg in p.args() {
                if let Value::Ref(id) = arg {
                    work_list.push(*id);
                }
            }
            for (key, value) in p.kwargs() {
                if let Value::Ref(id) = key {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::CmpToKey(c) => {
            if let Value::Ref(id) = c.func() {
                work_list.push(*id);
            }
        }
        HeapData::Tee(tee) => {
            if !tee.has_refs() {
                return;
            }
            tee.collect_child_ids(work_list);
        }
        HeapData::ItemGetter(g) => {
            for item in g.items() {
                if let Value::Ref(id) = item {
                    work_list.push(*id);
                }
            }
        }
        HeapData::AttrGetter(g) => {
            for attr in g.attrs() {
                if let Value::Ref(id) = attr {
                    work_list.push(*id);
                }
            }
        }
        HeapData::MethodCaller(mc) => {
            if let Value::Ref(id) = mc.name() {
                work_list.push(*id);
            }
            for arg in mc.args() {
                if let Value::Ref(id) = arg {
                    work_list.push(*id);
                }
            }
            for (key, value) in mc.kwargs() {
                if let Value::Ref(id) = key {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::WeakRef(_) => {}
        HeapData::Counter(counter) => {
            if !counter.dict().has_refs() {
                return;
            }
            for (k, v) in counter.dict() {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
        }
        HeapData::OrderedDict(ordered) => {
            if !ordered.dict().has_refs() {
                return;
            }
            for (k, v) in ordered.dict() {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
        }
        HeapData::LruCache(lru) => {
            if let Some(Value::Ref(id)) = &lru.func {
                work_list.push(*id);
            }
            for key in &lru.order {
                if let Value::Ref(id) = key {
                    work_list.push(*id);
                }
            }
            for (k, v) in &lru.cache {
                if let Value::Ref(id) = k {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = v {
                    work_list.push(*id);
                }
            }
        }
        HeapData::FunctionWrapper(fw) => {
            for value in [&fw.wrapper, &fw.wrapped, &fw.name, &fw.module, &fw.qualname, &fw.doc] {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        HeapData::Wraps(wraps) => {
            if let Value::Ref(id) = &wraps.wrapped {
                work_list.push(*id);
            }
        }
        HeapData::CachedProperty(cached) => {
            if let Value::Ref(id) = &cached.func {
                work_list.push(*id);
            }
        }
        HeapData::SingleDispatch(dispatcher) => {
            if let Value::Ref(id) = &dispatcher.func {
                work_list.push(*id);
            }
            for (cls, func) in &dispatcher.registry {
                if let Value::Ref(id) = cls {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = func {
                    work_list.push(*id);
                }
            }
        }
        HeapData::SingleDispatchRegister(register) => {
            if let Value::Ref(id) = &register.dispatcher {
                work_list.push(*id);
            }
            if let Value::Ref(id) = &register.cls {
                work_list.push(*id);
            }
        }
        HeapData::SingleDispatchMethod(method) => {
            if let Value::Ref(id) = &method.dispatcher {
                work_list.push(*id);
            }
        }
        HeapData::PartialMethod(method) => {
            if let Value::Ref(id) = &method.func {
                work_list.push(*id);
            }
            for arg in &method.args {
                if let Value::Ref(id) = arg {
                    work_list.push(*id);
                }
            }
            for (key, value) in &method.kwargs {
                if let Value::Ref(id) = key {
                    work_list.push(*id);
                }
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        // NamedTupleFactory and TotalOrderingMethod have no heap references
        HeapData::NamedTupleFactory(_)
        | HeapData::TotalOrderingMethod(_)
        | HeapData::Placeholder(_)
        | HeapData::TextWrapper(_) => {}
        // datetime types - most have no refs, but Datetime/Time may have tzinfo
        HeapData::Timedelta(_) => {}
        HeapData::Date(_) => {}
        HeapData::Datetime(dt) => {
            if let Some(tz_id) = dt.tzinfo() {
                work_list.push(tz_id);
            }
        }
        HeapData::Time(t) => {
            if let Some(tz_id) = t.tzinfo() {
                work_list.push(tz_id);
            }
        }
        HeapData::Timezone(_) => {}
        // Decimal is a leaf type with no heap references
        HeapData::Decimal(_) => {}
        // ObjectNewImpl is a leaf type with no heap references
        HeapData::ObjectNewImpl(_) => {}
        HeapData::Generator(generator) => {
            // Add captured cells to work list
            for cell_id in &generator.frame_cells {
                work_list.push(*cell_id);
            }
            // Add namespace values that are heap references
            for value in &generator.namespace {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
            // Add saved stack values that are heap references
            for value in &generator.saved_stack {
                if let Value::Ref(id) = value {
                    work_list.push(*id);
                }
            }
        }
        // Dict views hold a reference to their source dict
        HeapData::DictKeys(dk) => work_list.push(dk.dict_id()),
        HeapData::DictValues(dv) => work_list.push(dv.dict_id()),
        HeapData::DictItems(di) => work_list.push(di.dict_id()),
    }
}

/// Drop implementation for Heap that marks all contained Objects as Dereferenced
/// before dropping to prevent panics when the `ref-count-panic` feature is enabled.
#[cfg(feature = "ref-count-panic")]
impl<T: ResourceTracker> Drop for Heap<T> {
    fn drop(&mut self) {
        // Short-circuit if the heap has been reset or is empty - nothing to clean up.
        // This avoids allocating the dummy_stack Vec and iterating an empty entries Vec,
        // which is a meaningful optimization when Heap is reused via reset().
        if self.entries.is_empty() {
            return;
        }
        // Mark all contained Objects as Dereferenced before dropping.
        // We use py_dec_ref_ids for this since it handles the marking
        // (we ignore the collected IDs since we're dropping everything anyway).
        let mut dummy_stack = Vec::new();
        for value in self.entries.iter_mut().flatten() {
            if let Some(data) = &mut value.data {
                data.py_dec_ref_ids(&mut dummy_stack);
            }
        }
    }
}

/// This trait represents types that contain a `Heap`; it allows for more complex structures
/// to participate in the `HeapGuard` pattern.
pub(crate) trait ContainsHeap<T: ResourceTracker> {
    fn heap_mut(&mut self) -> &mut Heap<T>;
}

impl<T: ResourceTracker> ContainsHeap<T> for Heap<T> {
    #[inline]
    fn heap_mut(&mut self) -> &mut Self {
        self
    }
}

/// Trait for types that require heap access for proper cleanup.
///
/// Rust's standard `Drop` trait cannot decrement heap reference counts because it has no
/// access to the `Heap`. This trait provides an explicit drop-with-heap method so that
/// ref-counted values (and containers of them) can properly decrement their counts when
/// they are no longer needed.
///
/// **All types implementing this trait must be cleaned up on every code path** — not just
/// the happy path, but also early returns, conditional branches, `continue`, etc. A missed
/// call on any branch leaks reference counts. Prefer [`defer_drop!`] or [`HeapGuard`] to
/// guarantee cleanup automatically rather than inserting manual calls in every branch.
///
/// Implemented for `Value`, `Option<V>`, `Vec<Value>`, `ArgValues`, iterators, and other
/// types that hold heap references.
pub(crate) trait DropWithHeap<T: ResourceTracker> {
    /// Consume `self` and decrement reference counts for any heap-allocated values contained within.
    fn drop_with_heap(self, heap: &mut Heap<T>);
}

impl<T: ResourceTracker> DropWithHeap<T> for Value {
    #[inline]
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        Self::drop_with_heap(self, heap);
    }
}

impl<T: ResourceTracker, U: DropWithHeap<T>> DropWithHeap<T> for Option<U> {
    #[inline]
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        if let Some(value) = self {
            value.drop_with_heap(heap);
        }
    }
}

impl<T: ResourceTracker, U: DropWithHeap<T>> DropWithHeap<T> for Vec<U> {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        for value in self {
            value.drop_with_heap(heap);
        }
    }
}

impl<T: ResourceTracker, U: DropWithHeap<T>> DropWithHeap<T> for vec::IntoIter<U> {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        for value in self {
            value.drop_with_heap(heap);
        }
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for (Value, Value) {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        let (key, value) = self;
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for Dict {
    fn drop_with_heap(mut self, heap: &mut Heap<T>) {
        self.drop_all_entries(heap);
    }
}

/// RAII guard that ensures a [`DropWithHeap`] value is cleaned up on every code path.
///
/// The guard's `Drop` impl calls [`DropWithHeap::drop_with_heap`] automatically, so
/// cleanup happens whether the scope exits normally, via `?`, `continue`, early return,
/// or any other branch. This eliminates the need to manually insert `drop_with_heap`
/// calls in every branch.
///
/// On the normal path, the guarded value can be borrowed via [`as_parts`](Self::as_parts) /
/// [`as_parts_mut`](Self::as_parts_mut), or reclaimed via [`into_inner`](Self::into_inner) /
/// [`into_parts`](Self::into_parts) (which consume the guard without dropping the value).
///
/// Prefer the [`defer_drop!`] macro for the common case where you just need to ensure a
/// value is dropped at scope exit. Use `HeapGuard` directly when you need to conditionally
/// reclaim the value (e.g. push it back onto the stack on success) or need mutable access
/// to both the value and heap through [`as_parts_mut`](Self::as_parts_mut).
pub(crate) struct HeapGuard<'a, T: ResourceTracker, H: ContainsHeap<T>, V: DropWithHeap<T>> {
    // manually dropped because it needs to be dropped by move.
    value: ManuallyDrop<V>,
    heap: &'a mut H,
    _tracker: std::marker::PhantomData<T>,
}

impl<'a, T: ResourceTracker, H: ContainsHeap<T>, V: DropWithHeap<T>> HeapGuard<'a, T, H, V> {
    /// Creates a new `HeapGuard` for the given value and heap.
    #[inline]
    pub fn new(value: V, heap: &'a mut H) -> Self {
        Self {
            value: ManuallyDrop::new(value),
            heap,
            _tracker: std::marker::PhantomData,
        }
    }

    /// Consumes the guard and returns the contained value without dropping it.
    ///
    /// Use this when the value should survive beyond the guard's scope (e.g. returning
    /// a computed result from a function that used the guard for error-path safety).
    #[inline]
    pub fn into_inner(self) -> V {
        let mut this = ManuallyDrop::new(self);
        // SAFETY: [DH] - `ManuallyDrop::new(self)` prevents `Drop` on self, so we can take the value out
        unsafe { ManuallyDrop::take(&mut this.value) }
    }

    /// Borrows the value (immutably) and heap (mutably) out of the guard.
    ///
    /// This is what [`defer_drop!`] calls internally. The returned references are tied
    /// to the guard's lifetime, so the value cannot escape.
    #[inline]
    pub fn as_parts(&mut self) -> (&V, &mut H) {
        (&self.value, self.heap)
    }

    /// Borrows the value (mutably) and heap (mutably) out of the guard.
    ///
    /// This is what [`defer_drop_mut!`] calls internally. Use this when the value needs
    /// to be mutated in place (e.g. advancing an iterator, swapping during min/max).
    #[inline]
    pub fn as_parts_mut(&mut self) -> (&mut V, &mut H) {
        (&mut self.value, self.heap)
    }

    /// Consumes the guard and returns the value and heap separately, without dropping.
    ///
    /// Use this when you need to reclaim both the value *and* the heap reference — for
    /// example, to push the value back onto the VM stack via the heap owner.
    #[inline]
    pub fn into_parts(self) -> (V, &'a mut H) {
        let mut this = ManuallyDrop::new(self);
        // SAFETY: [DH] - `ManuallyDrop` prevents `Drop` on self, so we can recover the parts
        unsafe { (ManuallyDrop::take(&mut this.value), addr_of!(this.heap).read()) }
    }

    /// Borrows just the heap out of the guard
    #[inline]
    pub fn heap(&mut self) -> &mut H {
        self.heap
    }
}

impl<T: ResourceTracker, H: ContainsHeap<T>, V: DropWithHeap<T>> Drop for HeapGuard<'_, T, H, V> {
    fn drop(&mut self) {
        // SAFETY: [DH] - value is never manually dropped until this point
        unsafe { ManuallyDrop::take(&mut self.value) }.drop_with_heap(self.heap.heap_mut());
    }
}

/// The preferred way to ensure a [`DropWithHeap`] value is cleaned up on every code path.
///
/// Creates a [`HeapGuard`] and immediately rebinds `$value` as `&V` and `$heap` as
/// `&mut H` via [`HeapGuard::as_parts`]. The original owned value is moved into the
/// guard, which will call [`DropWithHeap::drop_with_heap`] when scope exits — whether
/// that's normal completion, early return via `?`, `continue`, or any other branch.
///
/// Beyond safety, this is often much more concise than inserting `drop_with_heap` calls
/// in every branch of complex control flow. For mutable access to the value, use
/// [`defer_drop_mut!`].
///
/// # Limitation
///
/// The macro rebinds `$heap` as a new `let` binding, so it cannot be used when `$heap`
/// is `self`. In `&mut self` methods, first assign `let this = self;` and pass `this`.
#[macro_export]
macro_rules! defer_drop {
    ($value:ident, $heap:ident) => {
        let mut _guard = $crate::heap::HeapGuard::new($value, $heap);
        #[allow(
            clippy::allow_attributes,
            reason = "the reborrowed parts may not both be used in every case, so allow unused vars to avoid warnings"
        )]
        #[allow(unused_variables)]
        let ($value, $heap) = _guard.as_parts();
    };
}

/// Like [`defer_drop!`], but rebinds `$value` as `&mut V` via [`HeapGuard::as_parts_mut`].
///
/// Use this when the value needs to be mutated in place — for example, advancing an
/// iterator with `for_next()`, or swapping values during a min/max comparison.
#[macro_export]
macro_rules! defer_drop_mut {
    ($value:ident, $heap:ident) => {
        let mut _guard = $crate::heap::HeapGuard::new($value, $heap);
        #[allow(
            clippy::allow_attributes,
            reason = "the reborrowed parts may not both be used in every case, so allow unused vars to avoid warnings"
        )]
        #[allow(unused_variables)]
        let ($value, $heap) = _guard.as_parts_mut();
    };
}
