/// Python named tuple type, combining tuple-like indexing with named attribute access.
///
/// Named tuples are like regular tuples but with field names, providing two ways
/// to access elements:
/// - By index: `version_info[0]` returns the major version
/// - By name: `version_info.major` returns the same value
///
/// Named tuples are:
/// - Immutable (all tuple semantics apply)
/// - Hashable (if all elements are hashable)
/// - Have a descriptive repr: `sys.version_info(major=3, minor=14, ...)`
/// - Support `len()` and iteration
///
/// # Use Case
///
/// This type is used for `sys.version_info` and similar structured tuples where
/// named access improves usability and readability.
use std::fmt::Write;

use ahash::AHashSet;
use smallvec::SmallVec;

use super::PyTrait;
use crate::{
    args::{ArgValues, KwargsValues},
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, Dict, OurosIter, Str, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// Python named tuple value stored on the heap.
///
/// Wraps a `Vec<Value>` with associated field names and provides both index-based
/// and name-based access. Named tuples are conceptually immutable, though this is
/// not enforced at the type level for internal operations.
///
/// # Reference Counting
///
/// When a named tuple is freed, all contained heap references have their refcounts
/// decremented via `py_dec_ref_ids`.
///
/// # GC Optimization
///
/// The `contains_refs` flag tracks whether the tuple contains any `Value::Ref` items.
/// This allows `py_dec_ref_ids` to skip iteration when the tuple contains only
/// primitive values (ints, bools, None, etc.).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct NamedTuple {
    /// Type name for repr (e.g., "sys.version_info").
    name: EitherStr,
    /// Field names in order, e.g., `major`, `minor`, `micro`, `releaselevel`, `serial`.
    field_names: Vec<EitherStr>,
    /// Values in order (same length as field_names).
    items: Vec<Value>,
    /// True if any item is a `Value::Ref`. Set at creation time since named tuples are immutable.
    contains_refs: bool,
}

impl NamedTuple {
    /// Creates a new named tuple.
    ///
    /// # Arguments
    ///
    /// * `type_name` - The type name for repr (e.g., "sys.version_info")
    /// * `field_names` - Field names as interned StringIds, in order
    /// * `items` - Values corresponding to each field name
    ///
    /// # Panics
    ///
    /// Panics if `field_names.len() != items.len()`.
    #[must_use]
    pub fn new(name: impl Into<EitherStr>, field_names: Vec<EitherStr>, items: Vec<Value>) -> Self {
        assert_eq!(
            field_names.len(),
            items.len(),
            "NamedTuple field_names and items must have same length"
        );
        let contains_refs = items.iter().any(|v| matches!(v, Value::Ref(_)));
        Self {
            name: name.into(),
            field_names,
            items,
            contains_refs,
        }
    }

    /// Returns the type name (e.g., "sys.version_info").
    #[must_use]
    pub fn name<'a>(&'a self, interns: &'a Interns) -> &'a str {
        self.name.as_str(interns)
    }

    /// Returns a reference to the field names.
    #[must_use]
    pub fn field_names(&self) -> &[EitherStr] {
        &self.field_names
    }

    /// Returns a reference to the underlying items vector.
    #[must_use]
    pub fn as_vec(&self) -> &Vec<Value> {
        &self.items
    }

    /// Returns the number of elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the tuple contains any heap references.
    ///
    /// When false, `py_dec_ref_ids` can skip iteration.
    #[inline]
    #[must_use]
    pub fn contains_refs(&self) -> bool {
        self.contains_refs
    }

    /// Gets a field value by name (StringId).
    ///
    /// Compares field names by actual string content, not just variant type.
    /// This allows lookup to work regardless of whether the field name was
    /// stored as an interned `StringId` or a heap-allocated `String`.
    ///
    /// Returns `Some(value)` if the field exists, `None` otherwise.
    #[must_use]
    pub fn get_by_name(&self, name_id: StringId, interns: &Interns) -> Option<&Value> {
        let name_str = interns.get_str(name_id);
        self.field_names
            .iter()
            .position(|field_name| field_name.as_str(interns) == name_str)
            .map(|idx| &self.items[idx])
    }

    /// Gets a field value by index, supporting negative indexing.
    ///
    /// Returns `Some(value)` if the index is in bounds, `None` otherwise.
    /// Uses `index + len` instead of `-index` to avoid overflow on `i64::MIN`.
    #[must_use]
    pub fn get_by_index(&self, index: i64) -> Option<&Value> {
        let len = i64::try_from(self.items.len()).ok()?;
        let normalized = if index < 0 { index + len } else { index };
        if normalized < 0 || normalized >= len {
            return None;
        }
        self.items.get(usize::try_from(normalized).ok()?)
    }

    /// Returns derived attributes for ParseResult/SplitResult (hostname, port, username, password).
    ///
    /// These properties are computed from the netloc field (index 1) following CPython's behavior:
    /// - hostname: netloc without userinfo, port, brackets; lowercased
    /// - port: port number as int, or None
    /// - username: user from userinfo, or None
    /// - password: password from userinfo, or None
    fn parse_result_derived_attr(
        &self,
        attr_name: &str,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        // Get netloc from index 1 (second field)
        let netloc_value = self.items.get(1);
        let netloc = match netloc_value {
            Some(Value::Ref(id)) => match heap.get(*id) {
                HeapData::Str(s) => s.as_str(),
                _ => "",
            },
            Some(Value::InternString(sid)) => interns.get_str(*sid),
            _ => "",
        };

        if netloc.is_empty() {
            return Ok(Some(AttrCallResult::Value(Value::None)));
        }

        // Parse netloc: [user[:password]@]hostname[:port]
        let (userinfo, hostport) = match netloc.rfind('@') {
            Some(at_pos) => (&netloc[..at_pos], &netloc[at_pos + 1..]),
            None => ("", netloc),
        };

        let (username, password): (Option<&str>, Option<&str>) = if userinfo.is_empty() {
            (None, None)
        } else {
            match userinfo.find(':') {
                Some(colon_pos) => {
                    let user: &str = &userinfo[..colon_pos];
                    let pass: &str = &userinfo[colon_pos + 1..];
                    (Some(user), Some(pass))
                }
                None => (Some(userinfo), None),
            }
        };

        // Parse hostname and port from hostport
        let (hostname, port) = parse_hostport(hostport);

        let result = match attr_name {
            "hostname" => match hostname {
                Some(h) => {
                    let lower = h.to_ascii_lowercase();
                    let id = heap.allocate(HeapData::Str(Str::from(lower)))?;
                    AttrCallResult::Value(Value::Ref(id))
                }
                None => AttrCallResult::Value(Value::None),
            },
            "port" => match port {
                Some(p) => AttrCallResult::Value(Value::Int(i64::from(p))),
                None => AttrCallResult::Value(Value::None),
            },
            "username" => match username {
                Some(u) => {
                    let s: String = u.to_owned();
                    let id = heap.allocate(HeapData::Str(Str::from(s)))?;
                    AttrCallResult::Value(Value::Ref(id))
                }
                None => AttrCallResult::Value(Value::None),
            },
            "password" => match password {
                Some(p) => {
                    let s: String = p.to_owned();
                    let id = heap.allocate(HeapData::Str(Str::from(s)))?;
                    AttrCallResult::Value(Value::Ref(id))
                }
                None => AttrCallResult::Value(Value::None),
            },
            _ => return Ok(None),
        };

        Ok(Some(result))
    }
}

/// Parses host:port from a hostport string, handling IPv6 brackets.
fn parse_hostport(hostport: &str) -> (Option<&str>, Option<u16>) {
    if hostport.is_empty() {
        return (None, None);
    }

    // Handle IPv6 bracket notation: [::1]:8080 or [::1]
    if hostport.starts_with('[') {
        match hostport.find(']') {
            Some(close_bracket) => {
                let host = &hostport[1..close_bracket];
                // Check for port after the bracket
                if hostport.len() > close_bracket + 1 && hostport.chars().nth(close_bracket + 1) == Some(':') {
                    let port_str = &hostport[close_bracket + 2..];
                    if let Ok(port) = port_str.parse::<u16>() {
                        return (Some(host), Some(port));
                    }
                }
                return (Some(host), None);
            }
            None => return (Some(hostport), None), // Malformed, return as-is
        }
    }

    // Regular hostname:port parsing (from right to handle IPv6 literals without brackets)
    // CPython uses rfind(':', 0, hostport.rfind(':')) to find the port separator
    // We need to be careful to not split on colons that are part of IPv6 addresses
    if let Some(colon_pos) = hostport.rfind(':') {
        // Check if this looks like an IPv6 address (multiple colons)
        let before_colon = &hostport[..colon_pos];
        let after_colon = &hostport[colon_pos + 1..];

        // If there's a colon before this position, it might be IPv6
        if before_colon.contains(':') {
            // This is likely an IPv6 address without brackets
            return (Some(hostport), None);
        }

        // Try to parse the part after colon as port
        if let Ok(port) = after_colon.parse::<u16>() {
            return (Some(before_colon), Some(port));
        }
    }

    (Some(hostport), None)
}

impl PyTrait for NamedTuple {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::NamedTuple
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.py_estimate_size()
            + self.field_names.len() * std::mem::size_of::<StringId>()
            + self.items.len() * std::mem::size_of::<Value>()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        Some(self.items.len())
    }

    fn py_getitem(
        &mut self,
        key: &Value,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Value> {
        // Extract integer index from key, returning TypeError if not an int
        let index = match key {
            Value::Int(i) => *i,
            _ => return Err(ExcType::type_error_indices(Type::NamedTuple, key.py_type(heap))),
        };

        // Get by index with bounds checking
        match self.get_by_index(index) {
            Some(value) => Ok(value.clone_with_heap(heap)),
            None => Err(ExcType::tuple_index_error()),
        }
    }

    fn py_eq(&self, other: &Self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> bool {
        if self.name(interns) == "statistics.NormalDist" && other.name(interns) == "statistics.NormalDist" {
            if self.items.len() < 2 || other.items.len() < 2 {
                return false;
            }
            return self.items[0].py_eq(&other.items[0], heap, interns)
                && self.items[1].py_eq(&other.items[1], heap, interns);
        }

        // Compare only by items (not type_name) to match tuple semantics
        // This allows sys.version_info == (3, 14, 0, 'final', 0) to work
        if self.items.len() != other.items.len() {
            return false;
        }
        for (i1, i2) in self.items.iter().zip(&other.items) {
            if !i1.py_eq(i2, heap, interns) {
                return false;
            }
        }
        true
    }

    /// Pushes all heap IDs contained in this named tuple onto the stack.
    ///
    /// Called during garbage collection to decrement refcounts of nested values.
    /// When `ref-count-panic` is enabled, also marks all Values as Dereferenced.
    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        // Skip iteration if no refs - GC optimization for tuples of primitives
        if !self.contains_refs {
            return;
        }
        for obj in &mut self.items {
            if let Value::Ref(id) = obj {
                stack.push(*id);
                #[cfg(feature = "ref-count-panic")]
                obj.dec_ref_forget();
            }
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.items.is_empty()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        heap: &Heap<impl ResourceTracker>,
        heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        if self.name(interns) == "statistics.NormalDist" && self.items.len() >= 2 {
            f.write_str("NormalDist(mu=")?;
            self.items[0].py_repr_fmt(f, heap, heap_ids, interns)?;
            f.write_str(", sigma=")?;
            self.items[1].py_repr_fmt(f, heap, heap_ids, interns)?;
            f.write_char(')')?;
            return Ok(());
        }

        // Format: type_name(field1=value1, field2=value2, ...)
        write!(f, "{}(", self.name.as_str(interns))?;

        let mut first = true;
        for (field_name, value) in self.field_names.iter().zip(&self.items) {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            f.write_str(field_name.as_str(interns))?;
            f.write_char('=')?;
            value.py_repr_fmt(f, heap, heap_ids, interns)?;
        }

        f.write_char(')')
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        if attr_id == StringId::from(StaticStrings::NamedTupleFields) {
            let mut field_values: Vec<Value> = Vec::with_capacity(self.field_names.len());
            for field_name in &self.field_names {
                let field_id = heap.allocate(HeapData::Str(Str::from(field_name.as_str(interns).to_owned())))?;
                field_values.push(Value::Ref(field_id));
            }
            let tuple_values: SmallVec<[Value; 3]> = SmallVec::from_vec(field_values);
            let tuple = allocate_tuple(tuple_values, heap)?;
            return Ok(Some(AttrCallResult::Value(tuple)));
        }
        if interns.get_str(attr_id) == "_field_defaults" {
            let dict_id = heap.allocate(HeapData::Dict(Dict::new()))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(dict_id))));
        }
        // Handle __name__ attribute for namedtuple types like sys.version_info
        if interns.get_str(attr_id) == "__name__" {
            let name = self.name(interns);
            // For module-qualified names like "sys.version_info", return just "version_info"
            let short_name = name.rsplit('.').next().unwrap_or(name);
            let name_id = heap.allocate(HeapData::Str(Str::from(short_name.to_owned())))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(name_id))));
        }
        // Handle derived properties for ParseResult and SplitResult (urllib.parse)
        let type_name = self.name(interns);
        if type_name == "ParseResult" || type_name == "SplitResult" {
            let attr_name = interns.get_str(attr_id);
            if matches!(attr_name, "hostname" | "port" | "username" | "password") {
                return self.parse_result_derived_attr(attr_name, heap, interns);
            }
        }
        if let Some(value) = self.get_by_name(attr_id, interns) {
            Ok(Some(AttrCallResult::Value(value.clone_with_heap(heap))))
        } else {
            // we use name here, not `self.py_type(heap)` hence returning a Ok(None)
            Err(ExcType::attribute_error(self.name(interns), interns.get_str(attr_id)))
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        if let Some(static_attr) = attr.static_string() {
            match static_attr {
                StaticStrings::NamedTupleReplace => return namedtuple_replace(self, args, heap, interns),
                StaticStrings::NamedTupleAsDict => return namedtuple_asdict(self, args, heap, interns),
                _ => {}
            }
        }

        let attr_name = attr.as_str(interns);
        let Some(field_value) = self
            .field_names
            .iter()
            .position(|field_name| field_name.as_str(interns) == attr_name)
            .map(|idx| self.items[idx].clone_with_heap(heap))
        else {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(self.py_type(heap), attr_name));
        };

        let result = if let Value::Ref(id) = &field_value {
            if let HeapData::Partial(partial) = heap.get(*id) {
                if !partial.kwargs().is_empty() {
                    field_value.drop_with_heap(heap);
                    args.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "namedtuple method has unsupported bound keyword arguments",
                    ));
                }

                let (call_positional, call_kwargs) = args.into_parts();
                let mut merged_args: Vec<Value> = partial
                    .args()
                    .iter()
                    .map(|value: &Value| value.clone_with_heap(heap))
                    .collect();
                merged_args.extend(call_positional);

                let merged_arg_values = if call_kwargs.is_empty() {
                    args_from_vec(merged_args)
                } else {
                    ArgValues::ArgsKargs {
                        args: merged_args,
                        kwargs: call_kwargs,
                    }
                };

                if let Value::ModuleFunction(module_function) = partial.func() {
                    module_function.call(heap, interns, merged_arg_values)?
                } else {
                    field_value.drop_with_heap(heap);
                    return Err(ExcType::type_error("namedtuple method field is not directly callable"));
                }
            } else {
                field_value.drop_with_heap(heap);
                args.drop_with_heap(heap);
                return Err(ExcType::attribute_error(self.py_type(heap), attr_name));
            }
        } else {
            field_value.drop_with_heap(heap);
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(self.py_type(heap), attr_name));
        };

        field_value.drop_with_heap(heap);
        match result {
            AttrCallResult::Value(value) => Ok(value),
            AttrCallResult::OsCall(_, os_args) => {
                os_args.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported async result",
                ))
            }
            AttrCallResult::ExternalCall(_, ext_args) => {
                ext_args.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported async result",
                ))
            }
            AttrCallResult::PropertyCall(getter, instance) => {
                getter.drop_with_heap(heap);
                instance.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported descriptor result",
                ))
            }
            AttrCallResult::DescriptorGet(descriptor) => {
                descriptor.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported descriptor result",
                ))
            }
            AttrCallResult::ReduceCall(callable, state, list_items) => {
                callable.drop_with_heap(heap);
                state.drop_with_heap(heap);
                for item in list_items {
                    item.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported reduce result",
                ))
            }
            AttrCallResult::MapCall(callable, iterators) => {
                callable.drop_with_heap(heap);
                for iter in iterators {
                    for item in iter {
                        item.drop_with_heap(heap);
                    }
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported map result",
                ))
            }
            AttrCallResult::FilterCall(callable, items) => {
                callable.drop_with_heap(heap);
                for item in items {
                    item.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported filter result",
                ))
            }
            AttrCallResult::FilterFalseCall(callable, items) => {
                callable.drop_with_heap(heap);
                for item in items {
                    item.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported filterfalse result",
                ))
            }
            AttrCallResult::TakeWhileCall(callable, items) => {
                callable.drop_with_heap(heap);
                for item in items {
                    item.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported takewhile result",
                ))
            }
            AttrCallResult::DropWhileCall(callable, items) => {
                callable.drop_with_heap(heap);
                for item in items {
                    item.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported dropwhile result",
                ))
            }
            AttrCallResult::GroupByCall(callable, items) => {
                callable.drop_with_heap(heap);
                for item in items {
                    item.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported groupby result",
                ))
            }
            AttrCallResult::TextwrapIndentCall(callable, _, _) => {
                callable.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported textwrap indent result",
                ))
            }
            AttrCallResult::CallFunction(func, call_args) => {
                func.drop_with_heap(heap);
                call_args.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported function call result",
                ))
            }
            AttrCallResult::ReSubCall(callable, matches, _, _, _) => {
                callable.drop_with_heap(heap);
                for (_, _, match_val) in matches {
                    match_val.drop_with_heap(heap);
                }
                Err(ExcType::type_error(
                    "namedtuple method call returned unsupported re.sub result",
                ))
            }
            AttrCallResult::ObjectNew => Err(ExcType::type_error(
                "namedtuple method call returned unsupported object.__new__ result",
            )),
        }
    }
}

/// Implements `namedtuple_instance._replace(**kwargs)`.
fn namedtuple_replace(
    named_tuple: &NamedTuple,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > 0 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("_replace", 0, positional_count));
    }
    positional.drop_with_heap(heap);

    let mut items: Vec<Value> = named_tuple.items.iter().map(|v| v.clone_with_heap(heap)).collect();
    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            items.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        let Some(field_index) = named_tuple
            .field_names
            .iter()
            .position(|field_name| field_name.as_str(interns) == key_name)
        else {
            value.drop_with_heap(heap);
            for (rest_key, rest_value) in kwargs_iter {
                rest_key.drop_with_heap(heap);
                rest_value.drop_with_heap(heap);
            }
            items.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                format!("Got unexpected field names: {key_name}"),
            )
            .into());
        };

        let old_value = std::mem::replace(&mut items[field_index], value);
        old_value.drop_with_heap(heap);
    }

    let replaced = NamedTuple::new(named_tuple.name.clone(), named_tuple.field_names.clone(), items);
    let id = heap.allocate(HeapData::NamedTuple(replaced))?;
    Ok(Value::Ref(id))
}

/// Implements `namedtuple_instance._asdict()`.
fn namedtuple_asdict(
    named_tuple: &NamedTuple,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    args.check_zero_args("_asdict", heap)?;
    let mut dict = Dict::new();
    for (field_name, value) in named_tuple.field_names.iter().zip(named_tuple.items.iter()) {
        let key_id = heap.allocate(HeapData::Str(Str::from(field_name.as_str(interns).to_owned())))?;
        let old = dict.set(Value::Ref(key_id), value.clone_with_heap(heap), heap, interns)?;
        old.drop_with_heap(heap);
    }
    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(dict_id))
}

/// Converts positional arguments into the most compact `ArgValues` representation.
fn args_from_vec(mut args: Vec<Value>) -> ArgValues {
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

/// A factory callable returned by `collections.namedtuple`.
/// A callable factory that builds `NamedTuple` values.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct NamedTupleFactory {
    /// The type name for the namedtuple (e.g., "Point").
    name: EitherStr,
    /// Field names in declaration order (e.g., `["x", "y"]`).
    field_names: Vec<EitherStr>,
    /// Trailing default values applied when calls omit optional fields.
    defaults: Vec<Value>,
    /// Module name exposed via `__module__`.
    module: EitherStr,
    /// Whether to unpack a single positional iterable into constructor fields.
    ///
    /// CPython struct-sequence types like `time.struct_time` use a
    /// `type(sequence)` constructor shape rather than one positional argument
    /// per field. This flag enables that construction mode for selected
    /// factories without affecting regular `collections.namedtuple` behavior.
    #[serde(default)]
    single_positional_iterable: bool,
}

impl NamedTupleFactory {
    /// Creates a namedtuple factory that accepts positional constructor arguments.
    #[must_use]
    pub fn new(name: impl Into<EitherStr>, field_names: Vec<EitherStr>) -> Self {
        Self::new_with_options(name, field_names, Vec::new(), EitherStr::Heap("__main__".to_owned()))
    }

    /// Creates a namedtuple factory with optional defaults and a module name.
    #[must_use]
    pub fn new_with_options(
        name: impl Into<EitherStr>,
        field_names: Vec<EitherStr>,
        defaults: Vec<Value>,
        module: impl Into<EitherStr>,
    ) -> Self {
        Self {
            name: name.into(),
            field_names,
            defaults,
            module: module.into(),
            single_positional_iterable: false,
        }
    }

    /// Configures the factory to unpack a single iterable positional argument.
    ///
    /// This is used for struct-sequence compatibility constructors where the
    /// callable takes one sequence object and materializes field values from it.
    #[must_use]
    pub fn with_single_positional_iterable_constructor(mut self) -> Self {
        self.single_positional_iterable = true;
        self
    }

    /// Returns the factory type name.
    #[must_use]
    pub fn name(&self) -> &EitherStr {
        &self.name
    }

    /// Returns the declared field names for instances created by this factory.
    #[must_use]
    pub fn field_names(&self) -> &[EitherStr] {
        &self.field_names
    }

    /// Returns trailing defaults used by this factory.
    #[must_use]
    pub fn defaults(&self) -> &[Value] {
        &self.defaults
    }

    /// Returns the module name exposed by this factory.
    #[must_use]
    pub fn module(&self) -> &EitherStr {
        &self.module
    }

    /// Constructs a `NamedTuple` instance from call arguments.
    pub fn instantiate(
        &self,
        args: ArgValues,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<NamedTuple> {
        let callable_name = self.name.as_str(interns);
        let field_count = self.field_names.len();
        let (positional, kwargs) = args.into_parts();
        let mut positional: Vec<Value> = positional.collect();

        if self.single_positional_iterable && positional.len() == 1 && kwargs.is_empty() {
            let iterable = positional.pop().expect("length checked");
            let items = match collect_iterable_items(iterable, heap, interns) {
                Ok(items) => items,
                Err(err) => {
                    positional.drop_with_heap(heap);
                    kwargs.drop_with_heap(heap);
                    return Err(err);
                }
            };
            positional = items;
        }

        if positional.len() > field_count {
            let count = positional.len();
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error_at_most(callable_name, field_count, count));
        }

        let mut slots: Vec<Option<Value>> = Vec::with_capacity(field_count);
        slots.resize_with(field_count, || None);
        for (index, value) in positional.into_iter().enumerate() {
            slots[index] = Some(value);
        }

        let mut kwargs_iter = kwargs.into_iter();
        while let Some((key, value)) = kwargs_iter.next() {
            let Some(key_name) = key.as_either_str(heap) else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                drop_remaining_kwargs(kwargs_iter, heap);
                drop_slots(&mut slots, heap);
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            let key_name = key_name.as_str(interns).to_owned();
            key.drop_with_heap(heap);

            let Some(index) = self
                .field_names
                .iter()
                .position(|field_name| field_name.as_str(interns) == key_name)
            else {
                value.drop_with_heap(heap);
                drop_remaining_kwargs(kwargs_iter, heap);
                drop_slots(&mut slots, heap);
                return Err(ExcType::type_error_unexpected_keyword(callable_name, &key_name));
            };

            if slots[index].is_some() {
                value.drop_with_heap(heap);
                drop_remaining_kwargs(kwargs_iter, heap);
                drop_slots(&mut slots, heap);
                return Err(ExcType::type_error_multiple_values(callable_name, &key_name));
            }
            slots[index] = Some(value);
        }

        let default_start = field_count.saturating_sub(self.defaults.len());
        let mut missing_names: Vec<String> = Vec::new();
        let mut items: Vec<Value> = Vec::with_capacity(field_count);
        for (index, slot) in slots.iter_mut().enumerate() {
            if let Some(value) = slot.take() {
                items.push(value);
                continue;
            }
            if index >= default_start {
                items.push(self.defaults[index - default_start].clone_with_heap(heap));
                continue;
            }
            missing_names.push(self.field_names[index].as_str(interns).to_owned());
        }

        if !missing_names.is_empty() {
            items.drop_with_heap(heap);
            let missing_refs: Vec<&str> = missing_names.iter().map(String::as_str).collect();
            return Err(ExcType::type_error_missing_positional_with_names(
                callable_name,
                &missing_refs,
            ));
        }

        Ok(NamedTuple::new(self.name.clone(), self.field_names.clone(), items))
    }

    /// Returns true if this factory stores any heap references.
    #[must_use]
    pub fn has_refs(&self) -> bool {
        self.defaults.iter().any(|value| matches!(value, Value::Ref(_)))
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for NamedTupleFactory {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        self.defaults.drop_with_heap(heap);
    }
}

impl PyTrait for NamedTupleFactory {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Type
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, stack: &mut Vec<HeapId>) {
        for value in &self.defaults {
            if let Value::Ref(id) = value {
                stack.push(*id);
            }
        }
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        interns: &Interns,
    ) -> std::fmt::Result {
        write!(f, "<class '{}'>", self.name.as_str(interns))
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.field_names.len() * std::mem::size_of::<EitherStr>()
            + self.defaults.len() * std::mem::size_of::<Value>()
            + self.module.py_estimate_size()
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let Some(method) = attr.static_string() else {
            args.drop_with_heap(heap);
            return Err(ExcType::attribute_error(self.py_type(heap), attr.as_str(interns)));
        };
        if method == StaticStrings::NamedTupleMake {
            let iterable = args.get_one_arg("_make", heap)?;
            let items = collect_iterable_items(iterable, heap, interns)?;
            let tuple = self.instantiate(args_from_vec(items), heap, interns)?;
            let id = heap.allocate(HeapData::NamedTuple(tuple))?;
            Ok(Value::Ref(id))
        } else {
            args.drop_with_heap(heap);
            Err(ExcType::attribute_error(self.py_type(heap), attr.as_str(interns)))
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        if attr_id == StringId::from(StaticStrings::NamedTupleFields) {
            let mut field_values: Vec<Value> = Vec::with_capacity(self.field_names.len());
            for field_name in &self.field_names {
                let field_id = heap.allocate(HeapData::Str(Str::from(field_name.as_str(interns).to_owned())))?;
                field_values.push(Value::Ref(field_id));
            }
            let tuple_values: SmallVec<[Value; 3]> = SmallVec::from_vec(field_values);
            let tuple = allocate_tuple(tuple_values, heap)?;
            return Ok(Some(AttrCallResult::Value(tuple)));
        }
        if interns.get_str(attr_id) == "_field_defaults" {
            let default_start = self.field_names.len().saturating_sub(self.defaults.len());
            let mut dict = Dict::new();
            for (offset, default_value) in self.defaults.iter().enumerate() {
                let field_name = self.field_names[default_start + offset].as_str(interns);
                let field_id = heap.allocate(HeapData::Str(Str::from(field_name.to_owned())))?;
                let old = dict.set(Value::Ref(field_id), default_value.clone_with_heap(heap), heap, interns)?;
                old.drop_with_heap(heap);
            }
            let dict_id = heap.allocate(HeapData::Dict(dict))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(dict_id))));
        }
        if interns.get_str(attr_id) == "__module__" {
            let module_id = heap.allocate(HeapData::Str(Str::from(self.module.as_str(interns).to_owned())))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(module_id))));
        }
        if interns.get_str(attr_id) == "__name__" {
            let name = self.name.as_str(interns);
            let short_name = name.rsplit('.').next().unwrap_or(name);
            let name_id = heap.allocate(HeapData::Str(Str::from(short_name.to_owned())))?;
            return Ok(Some(AttrCallResult::Value(Value::Ref(name_id))));
        }
        Ok(None)
    }
}

/// Collects every element from an iterable into owned `Value` items.
///
/// This helper centralizes iteration and drop behavior so constructor paths can
/// safely materialize iterable arguments without leaking references on errors.
fn collect_iterable_items(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut items = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                items.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(items)
}

/// Drops any unconsumed keyword arguments from a parsing iterator.
fn drop_remaining_kwargs(kwargs_iter: impl Iterator<Item = (Value, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (key, value) in kwargs_iter {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
}

/// Drops any partially assigned argument slots.
fn drop_slots(slots: &mut [Option<Value>], heap: &mut Heap<impl ResourceTracker>) {
    for slot in slots {
        if let Some(value) = slot.take() {
            value.drop_with_heap(heap);
        }
    }
}
