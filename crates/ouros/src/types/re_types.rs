//! Types for Python's `re` module: `re.Match` and `re.Pattern`.
//!
//! These types support attribute/method access matching CPython's `re` API:
//!
//! **`re.Match`** (returned by `search`, `match`, `fullmatch`, `finditer`):
//! - `.group(n=0)` — return the matched text for group `n` (0 = whole match)
//! - `.groups()` — return a tuple of all captured groups
//! - `.start()` — start index of the match
//! - `.end()` — end index of the match
//! - `.span()` — `(start, end)` as a tuple
//! - `.pos` / `.endpos` — search bounds used to produce the match
//!
//! **`re.Pattern`** (returned by `re.compile`):
//! - `.search(string)` — search for the pattern in `string`
//! - `.match(string)` — match the pattern at the start of `string`
//! - `.fullmatch(string)` — match the pattern against the entire `string`
//! - `.findall(string)` — find all non-overlapping matches
//! - `.sub(repl, string)` — replace matches
//! - `.split(string)` — split string by matches
//! - `.pattern` — the pattern string
//! - `.flags` — the flags used to compile the pattern
//! - `.groups` — number of capturing groups
//!
//! Neither type holds heap references — they store only plain Rust data (strings,
//! integers). This keeps `py_dec_ref_ids` a no-op and avoids GC complexity.

use std::{borrow::Cow, fmt::Write};

use ahash::AHashSet;
use smallvec::{SmallVec, smallvec};

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, Bytes, Dict, PyTrait, Str, Type, allocate_tuple},
    value::{EitherStr, Value},
};

// ===========================================================================
// ReMatch — result of a successful regex search/match
// ===========================================================================

/// A `re.Match` object representing a single successful regex match.
///
/// Stores the matched text, start/end positions, and captured groups as
/// plain Rust data (no heap references). Methods like `.group()`, `.start()`,
/// `.end()`, `.span()`, and `.groups()` are dispatched through `py_call_attr`.
/// The `.string` property is available via `py_getattr`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReMatch {
    /// The full matched substring (group 0).
    pub matched: String,
    /// Byte offset of match start in the original string.
    pub start: usize,
    /// Byte offset of match end in the original string.
    pub end: usize,
    /// Start index of the search window.
    pub pos: usize,
    /// End index of the search window.
    pub endpos: usize,
    /// Captured groups (group 1, 2, …). `None` means the group didn't participate.
    pub groups: Vec<Option<String>>,
    /// Captured group spans as `(start, end)` for group 1,2,..., or None if unmatched.
    #[serde(default)]
    pub group_spans: Vec<Option<(usize, usize)>>,
    /// The original string that was searched.
    pub string: String,
    /// Whether this match came from a bytes pattern/string.
    #[serde(default)]
    pub is_bytes: bool,
    /// The pattern string that produced this match (for `repr()`).
    pub pattern: String,
    /// The pattern flags used by the compiled regex.
    #[serde(default)]
    pub pattern_flags: i64,
    /// Number of capturing groups in the pattern.
    #[serde(default)]
    pub pattern_groups: usize,
    /// Named groups as `(name, group_number)` pairs.
    #[serde(default)]
    pub groupindex: Vec<(String, usize)>,
}

impl ReMatch {
    /// Creates a new `ReMatch` from match components.
    #[expect(
        clippy::too_many_arguments,
        reason = "re.Match stores all fields explicitly for cheap attribute access"
    )]
    pub fn new(
        matched: String,
        start: usize,
        end: usize,
        pos: usize,
        endpos: usize,
        groups: Vec<Option<String>>,
        group_spans: Vec<Option<(usize, usize)>>,
        string: String,
        is_bytes: bool,
        pattern: String,
        pattern_flags: i64,
        pattern_groups: usize,
        groupindex: Vec<(String, usize)>,
    ) -> Self {
        Self {
            matched,
            start,
            end,
            pos,
            endpos,
            groups,
            group_spans,
            string,
            is_bytes,
            pattern,
            pattern_flags,
            pattern_groups,
            groupindex,
        }
    }
}

impl PyTrait for ReMatch {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::ReMatch
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self == other
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // No heap references — nothing to do
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        // Match objects are always truthy (they only exist for successful matches)
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(
            f,
            "<re.Match object; span=({}, {}), match='{}'>",
            self.start, self.end, self.matched
        )
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        Cow::Owned(format!(
            "<re.Match object; span=({}, {}), match='{}'>",
            self.start, self.end, self.matched
        ))
    }

    fn py_estimate_size(&self) -> usize {
        96 + self.matched.len() + self.string.len() + self.pattern.len()
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        if attr.as_str(interns) == "groupdict" {
            return self.call_groupdict(heap, interns, args);
        }

        let Some(method) = attr.static_string() else {
            let attr_name = attr.as_str(interns);
            if attr_name == "expand" {
                return self.call_expand(heap, interns, args);
            }
            return Err(ExcType::attribute_error(Type::ReMatch, attr_name));
        };

        match method {
            StaticStrings::Group => self.call_group(heap, interns, args),
            StaticStrings::Groups => self.call_groups(heap, interns, args),
            StaticStrings::Start => self.call_start(heap, interns, args),
            StaticStrings::ReEnd => self.call_end(heap, interns, args),
            StaticStrings::Span => self.call_span(heap, interns, args),
            _ => Err(ExcType::attribute_error(Type::ReMatch, attr.as_str(interns))),
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        if attr_name == "lastindex" {
            return Ok(Some(AttrCallResult::Value(self.lastindex_value())));
        }
        if attr_name == "lastgroup" {
            return Ok(Some(AttrCallResult::Value(self.lastgroup_value(heap)?)));
        }
        if attr_name == "regs" {
            return Ok(Some(AttrCallResult::Value(self.regs_value(heap)?)));
        }

        match StaticStrings::from_string_id(attr_id) {
            Some(StaticStrings::StringMod) => {
                // .string — the original string passed to search/match
                Ok(Some(AttrCallResult::Value(
                    self.alloc_text_or_bytes(self.string.as_str(), heap)?,
                )))
            }
            Some(StaticStrings::OperatorPos) => {
                #[expect(clippy::cast_possible_wrap)]
                let pos = self.pos as i64;
                Ok(Some(AttrCallResult::Value(Value::Int(pos))))
            }
            Some(StaticStrings::ReEndpos) => {
                #[expect(clippy::cast_possible_wrap)]
                let endpos = self.endpos as i64;
                Ok(Some(AttrCallResult::Value(Value::Int(endpos))))
            }
            Some(StaticStrings::Re) => {
                let pattern = RePattern::new(
                    self.pattern.clone(),
                    self.pattern_flags,
                    self.pattern_groups,
                    self.groupindex.clone(),
                    self.is_bytes,
                );
                let id = heap.allocate(HeapData::RePattern(pattern))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
            _ => Ok(None),
        }
    }
}

impl ReMatch {
    /// `.group(n=0)` — return the text of group `n`.
    ///
    /// Group 0 is the entire match. Groups 1+ are capturing groups.
    /// Returns `None` if the group didn't participate in the match.
    fn call_group(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        args: ArgValues,
    ) -> RunResult<Value> {
        let (mut pos, kwargs) = args.into_parts();
        kwargs.drop_with_heap(heap);

        if pos.len() == 0 {
            return self.group_value(0, heap);
        }

        if pos.len() == 1 {
            let group = pos.next().expect("length checked");
            let idx = self.resolve_group_value(&group, heap, interns)?;
            group.drop_with_heap(heap);
            return self.group_value(idx, heap);
        }

        let mut values: SmallVec<[Value; 3]> = SmallVec::new();
        for group in pos {
            let idx = self.resolve_group_value(&group, heap, interns)?;
            group.drop_with_heap(heap);
            values.push(self.group_value(idx, heap)?);
        }
        Ok(allocate_tuple(values, heap)?)
    }

    /// Resolves a group argument (`int` or `str`) into the numeric group index.
    fn resolve_group_value(
        &self,
        group: &Value,
        heap: &Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<usize> {
        match group {
            Value::Int(i) => {
                if *i < 0 {
                    return Err(SimpleException::new_msg(ExcType::IndexError, "no such group").into());
                }
                #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                {
                    Ok(*i as usize)
                }
            }
            Value::InternString(id) => self.resolve_group_name(interns.get_str(*id)),
            Value::Ref(id) => {
                if let HeapData::Str(s) = heap.get(*id) {
                    self.resolve_group_name(s.as_str())
                } else {
                    Err(ExcType::type_error("group index must be an integer"))
                }
            }
            _ => Err(ExcType::type_error("group index must be an integer")),
        }
    }

    /// Resolves a named group into its numeric index.
    pub(crate) fn resolve_group_name(&self, name: &str) -> RunResult<usize> {
        self.groupindex
            .iter()
            .find_map(|(group_name, group_number)| (group_name == name).then_some(*group_number))
            .ok_or_else(|| SimpleException::new_msg(ExcType::IndexError, "no such group").into())
    }

    /// Returns a group value by numeric index (0=whole match).
    pub(crate) fn group_value(&self, group_num: usize, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        if group_num == 0 {
            return self.alloc_text_or_bytes(self.matched.as_str(), heap);
        }

        let idx = group_num - 1;
        if idx >= self.groups.len() {
            return Err(SimpleException::new_msg(ExcType::IndexError, "no such group").into());
        }
        match &self.groups[idx] {
            Some(text) => self.alloc_text_or_bytes(text.as_str(), heap),
            None => Ok(Value::None),
        }
    }

    /// Allocates a str/bytes value based on the source pattern type.
    fn alloc_text_or_bytes(&self, text: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        if self.is_bytes {
            let id = heap.allocate(HeapData::Bytes(Bytes::from(text.as_bytes().to_vec())))?;
            Ok(Value::Ref(id))
        } else {
            let id = heap.allocate(HeapData::Str(Str::from(text)))?;
            Ok(Value::Ref(id))
        }
    }

    /// Returns a start/end span tuple for a specific capture group.
    fn group_span(&self, group_num: usize) -> RunResult<(i64, i64)> {
        if group_num == 0 {
            #[expect(clippy::cast_possible_wrap)]
            {
                return Ok((self.start as i64, self.end as i64));
            }
        }

        let idx = group_num - 1;
        if idx >= self.group_spans.len() {
            return Err(SimpleException::new_msg(ExcType::IndexError, "no such group").into());
        }
        let span = self.group_spans[idx];
        #[expect(clippy::cast_possible_wrap)]
        Ok(match span {
            Some((start, end)) => (start as i64, end as i64),
            None => (-1, -1),
        })
    }

    /// `.groups()` — return a tuple of all captured group texts.
    ///
    /// Groups that didn't participate return `None`.
    fn call_groups(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        args: ArgValues,
    ) -> RunResult<Value> {
        let (pos, kwargs) = args.into_parts();
        let mut default_value = Value::None;
        for (key, value) in kwargs {
            let key_name = match &key {
                Value::InternString(id) => interns.get_str(*id).to_owned(),
                Value::Ref(id) => {
                    if let HeapData::Str(s) = heap.get(*id) {
                        s.as_str().to_owned()
                    } else {
                        key.drop_with_heap(heap);
                        value.drop_with_heap(heap);
                        default_value.drop_with_heap(heap);
                        pos.drop_with_heap(heap);
                        return Err(ExcType::type_error_kwargs_nonstring_key());
                    }
                }
                _ => {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    default_value.drop_with_heap(heap);
                    pos.drop_with_heap(heap);
                    return Err(ExcType::type_error_kwargs_nonstring_key());
                }
            };
            key.drop_with_heap(heap);
            if key_name == "default" {
                default_value.drop_with_heap(heap);
                default_value = value;
            } else {
                value.drop_with_heap(heap);
                default_value.drop_with_heap(heap);
                pos.drop_with_heap(heap);
                return Err(ExcType::type_error("invalid keyword argument"));
            }
        }
        let pos_len = pos.len();
        if pos_len > 1 {
            default_value.drop_with_heap(heap);
            pos.drop_with_heap(heap);
            return Err(ExcType::type_error_at_most("re.Match.groups", 1, pos_len));
        }
        if let Some(value) = pos.into_iter().next() {
            default_value.drop_with_heap(heap);
            default_value = value;
        }

        let items: SmallVec<[Value; 3]> = self
            .groups
            .iter()
            .map(|g| match g {
                Some(text) => self.alloc_text_or_bytes(text.as_str(), heap).unwrap_or(Value::None),
                None => default_value.clone_with_heap(heap),
            })
            .collect();

        default_value.drop_with_heap(heap);
        Ok(allocate_tuple(items, heap)?)
    }

    /// `.groupdict(default=None)` — return named groups as a dict.
    fn call_groupdict(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        args: ArgValues,
    ) -> RunResult<Value> {
        let (mut pos, kwargs) = args.into_parts();
        let mut default_value = pos.next();
        for (key, value) in kwargs {
            let key_name = key.py_str(heap, interns).into_owned();
            key.drop_with_heap(heap);
            if key_name == "default" {
                if let Some(old) = default_value.replace(value) {
                    old.drop_with_heap(heap);
                }
            } else {
                value.drop_with_heap(heap);
                if let Some(default) = default_value {
                    default.drop_with_heap(heap);
                }
                return Err(ExcType::type_error("invalid keyword argument"));
            }
        }

        let mut dict = Dict::new();
        for (name, group_number) in &self.groupindex {
            let key = Value::Ref(heap.allocate(HeapData::Str(Str::from(name.as_str())))?);
            let value = match group_number.checked_sub(1).and_then(|idx| self.groups.get(idx)) {
                Some(Some(text)) => Value::Ref(heap.allocate(HeapData::Str(Str::from(text.as_str())))?),
                Some(None) | None => default_value
                    .as_ref()
                    .map_or(Value::None, |default| default.clone_with_heap(heap)),
            };
            let old = dict.set(key, value, heap, interns)?;
            old.drop_with_heap(heap);
        }

        if let Some(default) = default_value {
            default.drop_with_heap(heap);
        }

        let id = heap.allocate(HeapData::Dict(dict))?;
        Ok(Value::Ref(id))
    }

    /// `.start()` — return the start byte offset of the match.
    fn call_start(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        args: ArgValues,
    ) -> RunResult<Value> {
        let (group_arg, _) = args.get_zero_one_two_args("re.Match.start", heap)?;
        let group_num = if let Some(group) = group_arg {
            let idx = self.resolve_group_value(&group, heap, interns)?;
            group.drop_with_heap(heap);
            idx
        } else {
            0
        };
        let (start, _) = self.group_span(group_num)?;
        Ok(Value::Int(start))
    }

    /// `.end()` — return the end byte offset of the match.
    fn call_end(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
        let (group_arg, _) = args.get_zero_one_two_args("re.Match.end", heap)?;
        let group_num = if let Some(group) = group_arg {
            let idx = self.resolve_group_value(&group, heap, interns)?;
            group.drop_with_heap(heap);
            idx
        } else {
            0
        };
        let (_, end) = self.group_span(group_num)?;
        Ok(Value::Int(end))
    }

    /// `.span()` — return `(start, end)` as a tuple.
    fn call_span(&self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
        let (group_arg, _) = args.get_zero_one_two_args("re.Match.span", heap)?;
        let group_num = if let Some(group) = group_arg {
            let idx = self.resolve_group_value(&group, heap, interns)?;
            group.drop_with_heap(heap);
            idx
        } else {
            0
        };
        let (start, end) = self.group_span(group_num)?;
        let items: SmallVec<[Value; 3]> = smallvec![Value::Int(start), Value::Int(end)];
        Ok(allocate_tuple(items, heap)?)
    }

    /// `expand(template)` expands backreferences in a replacement template.
    fn call_expand(
        &self,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        args: ArgValues,
    ) -> RunResult<Value> {
        let template_value = args.get_one_arg("re.Match.expand", heap)?;
        let template = extract_string(&template_value, heap, interns)?;
        template_value.drop_with_heap(heap);
        let expanded = crate::modules::re::expand_template(&template, self, heap, interns)?;
        if self.is_bytes {
            let id = heap.allocate(HeapData::Bytes(Bytes::from(expanded.as_bytes().to_vec())))?;
            Ok(Value::Ref(id))
        } else {
            let id = heap.allocate(HeapData::Str(Str::from(expanded)))?;
            Ok(Value::Ref(id))
        }
    }

    /// Returns `.lastindex` value.
    fn lastindex_value(&self) -> Value {
        let mut last = None;
        for (idx, span) in self.group_spans.iter().enumerate() {
            if span.is_some() {
                last = Some(idx + 1);
            }
        }
        match last {
            Some(index) => {
                #[expect(clippy::cast_possible_wrap)]
                {
                    Value::Int(index as i64)
                }
            }
            None => Value::None,
        }
    }

    /// Returns `.lastgroup` value.
    fn lastgroup_value(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let lastindex = self.lastindex_value();
        let Value::Int(lastindex) = lastindex else {
            return Ok(Value::None);
        };
        let Some((name, _)) = self.groupindex.iter().find(|(_, index)| *index as i64 == lastindex) else {
            return Ok(Value::None);
        };
        let id = heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
        Ok(Value::Ref(id))
    }

    /// Returns `.regs` tuple.
    fn regs_value(&self, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
        let mut items: SmallVec<[Value; 3]> = SmallVec::new();
        let (start, end) = self.group_span(0)?;
        items.push(allocate_tuple(smallvec![Value::Int(start), Value::Int(end)], heap)?);
        for idx in 1..=self.group_spans.len() {
            let (s, e) = self.group_span(idx)?;
            items.push(allocate_tuple(smallvec![Value::Int(s), Value::Int(e)], heap)?);
        }
        Ok(allocate_tuple(items, heap)?)
    }
}

// ===========================================================================
// RePattern — compiled regex pattern
// ===========================================================================

/// A `re.Pattern` object returned by `re.compile()`.
///
/// Stores the pattern string and flags so methods like `.search()`, `.match()`,
/// `.findall()` etc. can be called on it. The actual regex compilation happens
/// per-call (matching CPython's thread-safe behavior). No heap references are held.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RePattern {
    /// The regex pattern string.
    pub pattern: String,
    /// The flags used when compiling (combination of IGNORECASE, MULTILINE, etc.).
    pub flags: i64,
    /// Number of capturing groups in the pattern.
    #[serde(default)]
    pub groups: usize,
    /// Named groups as `(name, group_number)` pairs.
    #[serde(default)]
    pub groupindex: Vec<(String, usize)>,
    /// Whether this is a bytes pattern.
    #[serde(default)]
    pub is_bytes: bool,
}

impl RePattern {
    /// Creates a new `RePattern` from a pattern string and flags.
    pub fn new(pattern: String, flags: i64, groups: usize, groupindex: Vec<(String, usize)>, is_bytes: bool) -> Self {
        Self {
            pattern,
            flags,
            groups,
            groupindex,
            is_bytes,
        }
    }
}

impl PyTrait for RePattern {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::RePattern
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self == other
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // No heap references — nothing to do
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(f, "re.compile('{}'", self.pattern)?;
        if self.flags != 0 {
            write!(f, ", re.RegexFlag({}))", self.flags)?;
        } else {
            f.write_char(')')?;
        }
        Ok(())
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        if self.flags != 0 {
            Cow::Owned(format!("re.compile('{}', re.RegexFlag({}))", self.pattern, self.flags))
        } else {
            Cow::Owned(format!("re.compile('{}')", self.pattern))
        }
    }

    fn py_estimate_size(&self) -> usize {
        48 + self.pattern.len()
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
            return Err(ExcType::attribute_error(Type::RePattern, attr.as_str(interns)));
        };

        match method {
            StaticStrings::ReSearch => crate::modules::re::pattern_search(self, heap, interns, args),
            StaticStrings::ReMatch => crate::modules::re::pattern_match(self, heap, interns, args),
            StaticStrings::ReFullmatch => crate::modules::re::pattern_fullmatch(self, heap, interns, args),
            StaticStrings::ReFindall => crate::modules::re::pattern_findall(self, heap, interns, args),
            StaticStrings::Split => crate::modules::re::pattern_split(self, heap, interns, args),
            // sub/subn are fully handled in py_call_attr_raw (which intercepts before this method).
            // This branch should not be reached because HeapData::py_call_attr_raw dispatches
            // RePattern to py_call_attr_raw. Kept for completeness.
            StaticStrings::ReSub => {
                args.drop_with_heap(heap);
                Err(ExcType::type_error(
                    "Pattern.sub with callable requires py_call_attr_raw dispatch",
                ))
            }
            _ => {
                let name = attr.as_str(interns);
                if name == "finditer" {
                    crate::modules::re::pattern_finditer(self, heap, interns, args)
                } else if name == "subn" {
                    args.drop_with_heap(heap);
                    Err(ExcType::type_error(
                        "Pattern.subn with callable requires py_call_attr_raw dispatch",
                    ))
                } else if name == "scanner" {
                    crate::modules::re::pattern_scanner(self, heap, interns, args)
                } else if name == "__copy__" || name == "__deepcopy__" {
                    args.drop_with_heap(heap);
                    let id = heap.allocate(HeapData::RePattern(self.clone()))?;
                    Ok(Value::Ref(id))
                } else {
                    Err(ExcType::attribute_error(Type::RePattern, name))
                }
            }
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
        let name = attr.as_str(interns);
        // sub/subn need AttrCallResult to support callable replacements via ReSubCall
        if name == "sub" {
            return crate::modules::re::pattern_sub(self, heap, interns, args);
        }
        if name == "subn" {
            return crate::modules::re::pattern_subn(self, heap, interns, args);
        }
        // All other methods complete synchronously
        let value = self.py_call_attr(heap, attr, args, interns, self_id)?;
        Ok(AttrCallResult::Value(value))
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        match StaticStrings::from_string_id(attr_id) {
            Some(StaticStrings::Pattern) => {
                // .pattern — the pattern string
                if self.is_bytes {
                    let id = heap.allocate(HeapData::Bytes(Bytes::from(self.pattern.as_bytes().to_vec())))?;
                    Ok(Some(AttrCallResult::Value(Value::Ref(id))))
                } else {
                    let s = Str::from(self.pattern.as_str());
                    let id = heap.allocate(HeapData::Str(s))?;
                    Ok(Some(AttrCallResult::Value(Value::Ref(id))))
                }
            }
            Some(StaticStrings::Flags) => {
                // .flags — the integer flags
                Ok(Some(AttrCallResult::Value(Value::Int(self.flags))))
            }
            Some(StaticStrings::Groups) => {
                #[expect(clippy::cast_possible_wrap)]
                let groups = self.groups as i64;
                Ok(Some(AttrCallResult::Value(Value::Int(groups))))
            }
            _ if interns.get_str(attr_id) == "groupindex" => {
                let mut dict = Dict::new();
                for (name, group_number) in &self.groupindex {
                    let key = Value::Ref(heap.allocate(HeapData::Str(Str::from(name.as_str())))?);
                    #[expect(clippy::cast_possible_wrap)]
                    let value = Value::Int(*group_number as i64);
                    let old = dict.set(key, value, heap, interns)?;
                    old.drop_with_heap(heap);
                }
                let id = heap.allocate(HeapData::Dict(dict))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
            _ => Ok(None),
        }
    }
}

/// Helper to extract a string from a `Value`, returning an error if not a string.
fn extract_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_string()),
        Value::InternBytes(id) => Ok(String::from_utf8_lossy(interns.get_bytes(*id)).to_string()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_string()),
            HeapData::Bytes(b) | HeapData::Bytearray(b) => Ok(String::from_utf8_lossy(b.as_slice()).to_string()),
            _ => Err(ExcType::type_error("expected a string")),
        },
        _ => Err(ExcType::type_error("expected a string")),
    }
}
