//! Implementation of the `re` module.
//!
//! Provides regular expression support matching Python's `re` API:
//!
//! **Module-level functions:**
//! - `search(pattern, string, flags=0)` — search for pattern anywhere in string, return Match
//! - `match(pattern, string, flags=0)` — match pattern at start of string, return Match
//! - `fullmatch(pattern, string, flags=0)` — match pattern against entire string, return Match
//! - `findall(pattern, string, flags=0)` — find all non-overlapping matches as strings
//! - `finditer(pattern, string, flags=0)` — find all matches, return list of Match objects
//! - `sub(pattern, repl, string, count=0, flags=0)` — replace matches
//! - `subn(pattern, repl, string, count=0, flags=0)` — replace and return `(result, n_subs)`
//! - `split(pattern, string, flags=0)` — split string by pattern
//! - `compile(pattern, flags=0)` — return a compiled `re.Pattern` object
//! - `escape(pattern)` — escape special regex characters
//! - `purge()` — clear the regex cache (no-op in Ouros)
//!
//! **Flag constants:**
//! - `IGNORECASE` / `I` — case-insensitive matching (value 2)
//! - `MULTILINE` / `M` — `^`/`$` match line boundaries (value 8)
//! - `DOTALL` / `S` — `.` matches newline (value 16)
//! - `VERBOSE` / `X` — ignore whitespace/comments in pattern (value 64)
//! - `ASCII` / `A` — ASCII-only matching (value 256)
//! - `UNICODE` / `U` — Unicode matching behavior flag alias (value 32)
//! - `NOFLAG` — explicit no-flags value (value 0)
//! - `error` — exception alias for regex compilation/runtime errors
//!
//! **Match objects** (returned by `search`, `match`, `fullmatch`, `finditer`):
//! - `.group(n=0)` — text of group `n` (0 = whole match)
//! - `.groups()` — tuple of all captured groups
//! - `.start()` / `.end()` — byte offsets
//! - `.span()` — `(start, end)` tuple
//! - `.pos` / `.endpos` — search bounds used to produce the match
//! - `.string` — the original input string
//!
//! **Pattern objects** (returned by `compile`):
//! - `.search(s)`, `.match(s)`, `.fullmatch(s)`, `.findall(s)`, `.sub(r, s)`, `.split(s)`
//! - `.pattern` — the pattern string
//! - `.flags` — the integer flags
//! - `.groups` — number of capturing groups

use fancy_regex::{Captures, Regex};

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, Bytes, List, Module, PyTrait, ReMatch, RePattern, ReScannerRule, StdlibObject, Str, Type,
        allocate_tuple,
    },
    value::Value,
};

/// Regex module functions.
///
/// Each variant maps to a callable function in Python's `re` module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum ReFunctions {
    Search,
    Match,
    Fullmatch,
    Findall,
    Finditer,
    Sub,
    Subn,
    Split,
    Compile,
    Scanner,
    Escape,
    Purge,
}

/// Python regex flag constants.
///
/// These match CPython's values so Python code using `re.IGNORECASE | re.MULTILINE`
/// works correctly.
const IGNORECASE: i64 = 2;
const MULTILINE: i64 = 8;
const DOTALL: i64 = 16;
const LOCALE: i64 = 4;
const UNICODE: i64 = 32;
const VERBOSE: i64 = 64;
const DEBUG: i64 = 128;
const ASCII: i64 = 256;
const NOFLAG: i64 = 0;

/// Replacement mode for `re.sub`/`re.subn`.
pub(crate) enum ReReplacement {
    /// Literal/template replacement with backreferences.
    Template(String),
    /// Callable replacement — the `Value` is the Python callable that receives
    /// a `re.Match` object and returns a replacement string.
    Callable(Value),
}

/// Creates the `re` module and allocates it on the heap.
///
/// Registers all module-level functions and flag constants as module attributes.
///
/// # Returns
/// A `HeapId` pointing to the newly allocated module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Re);

    // Functions
    let funcs: &[(StaticStrings, ReFunctions)] = &[
        (StaticStrings::ReSearch, ReFunctions::Search),
        (StaticStrings::ReMatch, ReFunctions::Match),
        (StaticStrings::ReFullmatch, ReFunctions::Fullmatch),
        (StaticStrings::ReFindall, ReFunctions::Findall),
        (StaticStrings::ReFinditer, ReFunctions::Finditer),
        (StaticStrings::ReSub, ReFunctions::Sub),
        (StaticStrings::ReSubn, ReFunctions::Subn),
        (StaticStrings::Split, ReFunctions::Split),
        (StaticStrings::ReCompile, ReFunctions::Compile),
        (StaticStrings::ReEscape, ReFunctions::Escape),
        (StaticStrings::RePurge, ReFunctions::Purge),
    ];

    for &(name, func) in funcs {
        module.set_attr(name, Value::ModuleFunction(ModuleFunctions::Re(func)), heap, interns);
    }
    module.set_attr_str(
        "Scanner",
        Value::ModuleFunction(ModuleFunctions::Re(ReFunctions::Scanner)),
        heap,
        interns,
    )?;

    // Flag constants — long and short names
    let ignorecase = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(IGNORECASE)))?);
    let multiline = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(MULTILINE)))?);
    let dotall = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(DOTALL)))?);
    let verbose = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(VERBOSE)))?);
    let ascii = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(ASCII)))?);
    let noflag = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(NOFLAG)))?);
    let unicode = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(UNICODE)))?);
    let locale = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(LOCALE)))?);
    let debug = Value::Ref(heap.allocate(HeapData::StdlibObject(StdlibObject::new_regex_flag(DEBUG)))?);

    module.set_attr(
        StaticStrings::ReIgnorecase,
        ignorecase.clone_with_heap(heap),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::ReMultiline,
        multiline.clone_with_heap(heap),
        heap,
        interns,
    );
    module.set_attr(StaticStrings::ReDotall, dotall.clone_with_heap(heap), heap, interns);
    module.set_attr(StaticStrings::ReVerbose, verbose.clone_with_heap(heap), heap, interns);
    module.set_attr(StaticStrings::ReAscii, ascii.clone_with_heap(heap), heap, interns);
    module.set_attr(StaticStrings::ReNoflag, noflag.clone_with_heap(heap), heap, interns);
    module.set_attr(StaticStrings::ReUnicode, unicode.clone_with_heap(heap), heap, interns);
    module.set_attr(
        StaticStrings::ReUnicodeShort,
        unicode.clone_with_heap(heap),
        heap,
        interns,
    );
    // CPython exposes additional aliases and compatibility flags.
    module.set_attr_str("I", ignorecase.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("M", multiline.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("S", dotall.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("X", verbose.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("A", ascii.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("L", locale.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("LOCALE", locale.clone_with_heap(heap), heap, interns)?;
    module.set_attr_str("DEBUG", debug.clone_with_heap(heap), heap, interns)?;
    // RegexFlag is exposed as a type/class, not a StdlibObject
    // This makes it callable (RegexFlag(value)) and gives it type 'type'
    module.set_attr_str(
        "RegexFlag",
        Value::Builtin(Builtins::Type(Type::RegexFlag)),
        heap,
        interns,
    )?;
    ignorecase.drop_with_heap(heap);
    multiline.drop_with_heap(heap);
    dotall.drop_with_heap(heap);
    verbose.drop_with_heap(heap);
    ascii.drop_with_heap(heap);
    noflag.drop_with_heap(heap);
    unicode.drop_with_heap(heap);
    locale.drop_with_heap(heap);
    debug.drop_with_heap(heap);

    // re.error is the module's exception alias for regex compilation/runtime failures.
    module.set_attr(
        StaticStrings::ReError,
        Value::Builtin(Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    );
    module.set_attr_str(
        "PatternError",
        Value::Builtin(Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    )?;
    // CPython exposes re.Match/re.Pattern aliases for runtime isinstance checks.
    module.set_attr_str("Match", Value::Builtin(Builtins::Type(Type::ReMatch)), heap, interns)?;
    module.set_attr_str(
        "Pattern",
        Value::Builtin(Builtins::Type(Type::RePattern)),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a `re` module function.
///
/// All `re` module functions are pure computations that don't require host involvement.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ReFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        ReFunctions::Search => search(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Match => match_(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Fullmatch => fullmatch(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Findall => findall(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Finditer => finditer(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Sub => sub(heap, interns, args),
        ReFunctions::Subn => subn(heap, interns, args),
        ReFunctions::Split => split(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Compile => compile(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Scanner => scanner(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Escape => escape(heap, interns, args).map(AttrCallResult::Value),
        ReFunctions::Purge => purge(heap, args).map(AttrCallResult::Value),
    }
}

// ===========================================================================
// Module-level function implementations
// ===========================================================================

/// `re.search(pattern, string, flags=0)` — search for the pattern anywhere in `string`.
///
/// Returns a `re.Match` object if found, or `None`.
fn search(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern, string, flags, is_bytes) = extract_two_or_three_text_args("re.search", heap, interns, args)?;
    do_search(&pattern, &string, flags, is_bytes, heap)
}

/// `re.match(pattern, string, flags=0)` — match the pattern at the start of `string`.
///
/// Returns a `re.Match` object if the start of `string` matches, or `None`.
fn match_(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern, string, flags, is_bytes) = extract_two_or_three_text_args("re.match", heap, interns, args)?;
    do_match(&pattern, &string, flags, is_bytes, heap)
}

/// `re.fullmatch(pattern, string, flags=0)` — match the pattern against the entire `string`.
///
/// Returns a `re.Match` object if the whole string matches, or `None`.
fn fullmatch(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern, string, flags, is_bytes) = extract_two_or_three_text_args("re.fullmatch", heap, interns, args)?;
    do_fullmatch(&pattern, &string, flags, is_bytes, heap)
}

/// `re.findall(pattern, string, flags=0)` — find all non-overlapping matches.
///
/// Returns a list of matched strings. If the pattern has groups, returns the
/// group contents instead.
fn findall(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern, string, flags, is_bytes) = extract_two_or_three_text_args("re.findall", heap, interns, args)?;
    do_findall(&pattern, &string, flags, is_bytes, heap)
}

/// `re.finditer(pattern, string, flags=0)` — find all matches as Match objects.
///
/// Returns a list of `re.Match` objects (Python returns an iterator, but Ouros
/// returns a list since generators aren't fully supported yet).
fn finditer(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern, string, flags, is_bytes) = extract_two_or_three_text_args("re.finditer", heap, interns, args)?;
    do_finditer(&pattern, &string, flags, is_bytes, heap)
}

/// `re.sub(pattern, repl, string, count=0, flags=0)` — replace matches.
///
/// Replaces up to `count` occurrences (0 = all) of `pattern` with `repl`.
/// When `repl` is a callable, returns `AttrCallResult::ReSubCall` so the VM
/// can invoke the callback for each match.
fn sub(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (pattern, repl, string, count, flags, is_bytes) = extract_sub_args("re.sub", heap, interns, args)?;
    do_sub(&pattern, repl, &string, flags, count, is_bytes, heap)
}

/// `re.subn(pattern, repl, string, count=0, flags=0)` — replace and count.
///
/// Like `sub`, but returns a tuple `(new_string, number_of_subs_made)`.
/// When `repl` is a callable, returns `AttrCallResult::ReSubCall` so the VM
/// can invoke the callback for each match.
fn subn(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (pattern, repl, string, count, flags, is_bytes) = extract_sub_args("re.subn", heap, interns, args)?;
    do_subn(&pattern, repl, &string, flags, count, is_bytes, heap)
}

/// `re.split(pattern, string, flags=0)` — split string by pattern.
///
/// Returns a list of substrings.
fn split(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern, string, maxsplit, flags, is_bytes) = extract_split_args("re.split", heap, interns, args)?;
    do_split(&pattern, &string, flags, maxsplit, is_bytes, heap)
}

/// `re.compile(pattern, flags=0)` — compile a regular expression pattern.
///
/// Returns a `re.Pattern` object that can be reused for multiple operations.
fn compile(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (pattern_val, flags_val) = args.get_one_two_args("re.compile", heap)?;

    let Some((pattern_str, is_bytes)) = get_text(&pattern_val, heap, interns) else {
        pattern_val.drop_with_heap(heap);
        if let Some(fv) = flags_val {
            fv.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("first argument must be a string or bytes"));
    };
    pattern_val.drop_with_heap(heap);

    let mut flags = extract_int_flag(flags_val, heap);
    if !is_bytes && flags & ASCII == 0 {
        flags |= UNICODE;
    }

    // Validate the pattern by trying to build the regex.
    let regex = build_regex(&pattern_str, flags)?;
    let groups = regex.captures_len().saturating_sub(1);
    let groupindex = extract_groupindex(&regex);

    let re_pattern = RePattern::new(pattern_str, flags, groups, groupindex, is_bytes);
    let id = heap.allocate(HeapData::RePattern(re_pattern))?;
    Ok(Value::Ref(id))
}

/// `re.Scanner(lexicon)` — build a scanner object.
fn scanner(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let lexicon_value = args.get_one_arg("re.Scanner", heap)?;
    let rules = parse_scanner_rules(&lexicon_value, heap, interns)?;
    lexicon_value.drop_with_heap(heap);
    let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_re_scanner(rules)))?;
    Ok(Value::Ref(id))
}

/// `re.escape(pattern)` — escape special regex characters in `pattern`.
///
/// Returns a string with all regex metacharacters backslash-escaped, matching
/// CPython's `re.escape`. The escaped characters are:
/// `\`, `.`, `^`, `$`, `*`, `+`, `?`, `{`, `}`, `[`, `]`, `|`, `(`, `)`.
fn escape(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let val = args.get_one_arg("re.escape", heap)?;
    let Some((s, is_bytes)) = get_text(&val, heap, interns) else {
        val.drop_with_heap(heap);
        return Err(ExcType::type_error("argument must be a string or bytes"));
    };
    val.drop_with_heap(heap);

    let mut result = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        if is_regex_metacharacter(ch) {
            result.push('\\');
        }
        result.push(ch);
    }

    let id = if is_bytes {
        heap.allocate(HeapData::Bytes(Bytes::from(result.as_bytes().to_vec())))?
    } else {
        heap.allocate(HeapData::Str(Str::from(result)))?
    };
    Ok(Value::Ref(id))
}

/// `re.purge()` — clear the regex cache.
///
/// Ouros does not maintain a regex cache, so this is a no-op.
fn purge(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("re.purge", heap)?;
    Ok(Value::None)
}

// ===========================================================================
// Core regex operations (shared between module functions and Pattern methods)
// ===========================================================================

/// Core implementation for `search` — called by both `re.search()` and `Pattern.search()`.
pub(crate) fn do_search(
    pattern: &str,
    string: &str,
    flags: i64,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let regex = build_regex(pattern, flags)?;
    let groups_count = regex.captures_len().saturating_sub(1);
    let groupindex = extract_groupindex(&regex);

    if let Some(caps) = regex.captures(string).map_err(regex_error)? {
        let m = caps.get(0).expect("group 0 always exists");
        let groups = extract_groups(&caps);
        let group_spans = extract_group_spans(&caps);
        let match_obj = ReMatch::new(
            m.as_str().to_owned(),
            m.start(),
            m.end(),
            0,
            string.len(),
            groups,
            group_spans,
            string.to_owned(),
            is_bytes,
            pattern.to_owned(),
            flags,
            groups_count,
            groupindex,
        );
        let id = heap.allocate(HeapData::ReMatch(match_obj))?;
        Ok(Value::Ref(id))
    } else {
        Ok(Value::None)
    }
}

/// Core implementation for `match` — anchored at the start of the string.
pub(crate) fn do_match(
    pattern: &str,
    string: &str,
    flags: i64,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let regex = build_regex(pattern, flags)?;
    let groups_count = regex.captures_len().saturating_sub(1);
    let groupindex = extract_groupindex(&regex);

    if let Some(caps) = regex.captures(string).map_err(regex_error)? {
        let m = caps.get(0).expect("group 0 always exists");
        // Only succeed if the match starts at position 0
        if m.start() == 0 {
            let groups = extract_groups(&caps);
            let group_spans = extract_group_spans(&caps);
            let match_obj = ReMatch::new(
                m.as_str().to_owned(),
                m.start(),
                m.end(),
                0,
                string.len(),
                groups,
                group_spans,
                string.to_owned(),
                is_bytes,
                pattern.to_owned(),
                flags,
                groups_count,
                groupindex,
            );
            let id = heap.allocate(HeapData::ReMatch(match_obj))?;
            return Ok(Value::Ref(id));
        }
    }
    Ok(Value::None)
}

/// Core implementation for `fullmatch` — the pattern must cover the entire string.
pub(crate) fn do_fullmatch(
    pattern: &str,
    string: &str,
    flags: i64,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let regex = build_regex(pattern, flags)?;
    let groups_count = regex.captures_len().saturating_sub(1);
    let groupindex = extract_groupindex(&regex);

    if let Some(caps) = regex.captures(string).map_err(regex_error)? {
        let m = caps.get(0).expect("group 0 always exists");
        if m.start() == 0 && m.end() == string.len() {
            let groups = extract_groups(&caps);
            let group_spans = extract_group_spans(&caps);
            let match_obj = ReMatch::new(
                m.as_str().to_owned(),
                m.start(),
                m.end(),
                0,
                string.len(),
                groups,
                group_spans,
                string.to_owned(),
                is_bytes,
                pattern.to_owned(),
                flags,
                groups_count,
                groupindex,
            );
            let id = heap.allocate(HeapData::ReMatch(match_obj))?;
            return Ok(Value::Ref(id));
        }
    }
    Ok(Value::None)
}

/// Core implementation for `findall` — returns list of matched strings.
///
/// If the pattern has one capturing group, returns group contents.
/// If no groups, returns full match strings.
pub(crate) fn do_findall(
    pattern: &str,
    string: &str,
    flags: i64,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let regex = build_regex(pattern, flags)?;
    let groups_len = regex.captures_len().saturating_sub(1);

    let mut matches: Vec<Value> = Vec::new();
    for caps in regex.captures_iter(string) {
        let caps = caps.map_err(regex_error)?;
        if groups_len == 1 {
            // Single capturing group — return group 1 content
            let text = caps.get(1).map_or("", |m| m.as_str());
            let id = if is_bytes {
                heap.allocate(HeapData::Bytes(Bytes::from(text.as_bytes().to_vec())))?
            } else {
                heap.allocate(HeapData::Str(Str::from(text)))?
            };
            matches.push(Value::Ref(id));
        } else if groups_len > 1 {
            let mut tuple_items = smallvec::SmallVec::new();
            for idx in 1..=groups_len {
                let value = match caps.get(idx) {
                    Some(group) => {
                        if is_bytes {
                            Value::Ref(heap.allocate(HeapData::Bytes(Bytes::from(group.as_str().as_bytes().to_vec())))?)
                        } else {
                            Value::Ref(heap.allocate(HeapData::Str(Str::from(group.as_str())))?)
                        }
                    }
                    None => Value::None,
                };
                tuple_items.push(value);
            }
            matches.push(allocate_tuple(tuple_items, heap)?);
        } else {
            // No groups — return full match
            let m = caps.get(0).expect("group 0 always exists");
            let id = if is_bytes {
                heap.allocate(HeapData::Bytes(Bytes::from(m.as_str().as_bytes().to_vec())))?
            } else {
                heap.allocate(HeapData::Str(Str::from(m.as_str())))?
            };
            matches.push(Value::Ref(id));
        }
    }

    let list = List::new(matches);
    let list_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(list_id))
}

/// Core implementation for `finditer` — returns list of Match objects.
///
/// Python's `finditer` returns an iterator, but Ouros returns a list since
/// full generator support isn't available yet.
fn do_finditer(
    pattern: &str,
    string: &str,
    flags: i64,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let regex = build_regex(pattern, flags)?;
    let groups_count = regex.captures_len().saturating_sub(1);
    let groupindex = extract_groupindex(&regex);

    let mut matches: Vec<Value> = Vec::new();
    for caps in regex.captures_iter(string) {
        let caps = caps.map_err(regex_error)?;
        let m = caps.get(0).expect("group 0 always exists");
        let groups = extract_groups(&caps);
        let group_spans = extract_group_spans(&caps);
        let match_obj = ReMatch::new(
            m.as_str().to_owned(),
            m.start(),
            m.end(),
            0,
            string.len(),
            groups,
            group_spans,
            string.to_owned(),
            is_bytes,
            pattern.to_owned(),
            flags,
            groups_count,
            groupindex.clone(),
        );
        let id = heap.allocate(HeapData::ReMatch(match_obj))?;
        matches.push(Value::Ref(id));
    }

    let list = List::new(matches);
    let list_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(list_id))
}

/// Core implementation for `sub` — replace matches, return new string.
///
/// For template replacements, performs the substitution synchronously.
/// For callable replacements, pre-computes match objects and returns
/// `AttrCallResult::ReSubCall` so the VM can invoke the callback.
pub(crate) fn do_sub(
    pattern: &str,
    repl: ReReplacement,
    string: &str,
    flags: i64,
    count: usize,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<AttrCallResult> {
    match repl {
        ReReplacement::Template(ref template) => {
            let (result, _) = run_substitution(pattern, template, string, flags, count)?;
            let id = if is_bytes {
                heap.allocate(HeapData::Bytes(Bytes::from(result.as_bytes().to_vec())))?
            } else {
                heap.allocate(HeapData::Str(Str::from(result)))?
            };
            Ok(AttrCallResult::Value(Value::Ref(id)))
        }
        ReReplacement::Callable(callable) => {
            let matches = collect_match_objects(pattern, string, flags, count, is_bytes, heap)?;
            Ok(AttrCallResult::ReSubCall(
                callable,
                matches,
                string.to_owned(),
                is_bytes,
                false,
            ))
        }
    }
}

/// Core implementation for `subn` — replace matches, return `(result, n_subs)`.
///
/// For template replacements, performs the substitution synchronously.
/// For callable replacements, pre-computes match objects and returns
/// `AttrCallResult::ReSubCall` with `return_count=true`.
fn do_subn(
    pattern: &str,
    repl: ReReplacement,
    string: &str,
    flags: i64,
    count: usize,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<AttrCallResult> {
    match repl {
        ReReplacement::Template(ref template) => {
            let (result, n_subs) = run_substitution(pattern, template, string, flags, count)?;
            let str_id = if is_bytes {
                heap.allocate(HeapData::Bytes(Bytes::from(result.as_bytes().to_vec())))?
            } else {
                heap.allocate(HeapData::Str(Str::from(result)))?
            };
            #[expect(clippy::cast_possible_wrap)]
            let items = smallvec::smallvec![Value::Ref(str_id), Value::Int(n_subs as i64)];
            Ok(AttrCallResult::Value(allocate_tuple(items, heap)?))
        }
        ReReplacement::Callable(callable) => {
            let matches = collect_match_objects(pattern, string, flags, count, is_bytes, heap)?;
            Ok(AttrCallResult::ReSubCall(
                callable,
                matches,
                string.to_owned(),
                is_bytes,
                true,
            ))
        }
    }
}

/// Core implementation for `split` — split string by pattern, return list.
pub(crate) fn do_split(
    pattern: &str,
    string: &str,
    flags: i64,
    maxsplit: usize,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let regex = build_regex(pattern, flags)?;
    let mut parts: Vec<Value> = Vec::new();
    let mut last_end = 0usize;
    for (splits, captures) in regex.captures_iter(string).enumerate() {
        let captures = captures.map_err(regex_error)?;
        let m = captures.get(0).expect("group 0 always exists");
        if maxsplit > 0 && splits >= maxsplit {
            break;
        }

        let text = &string[last_end..m.start()];
        let part_id = if is_bytes {
            heap.allocate(HeapData::Bytes(Bytes::from(text.as_bytes().to_vec())))?
        } else {
            heap.allocate(HeapData::Str(Str::from(text)))?
        };
        parts.push(Value::Ref(part_id));

        for group_index in 1..captures.len() {
            let group_value = captures.get(group_index).map_or("", |group| group.as_str());
            let group_id = if is_bytes {
                heap.allocate(HeapData::Bytes(Bytes::from(group_value.as_bytes().to_vec())))?
            } else {
                heap.allocate(HeapData::Str(Str::from(group_value)))?
            };
            parts.push(Value::Ref(group_id));
        }
        last_end = m.end();
    }

    let tail = &string[last_end..];
    let tail_id = if is_bytes {
        heap.allocate(HeapData::Bytes(Bytes::from(tail.as_bytes().to_vec())))?
    } else {
        heap.allocate(HeapData::Str(Str::from(tail)))?
    };
    parts.push(Value::Ref(tail_id));

    let list = List::new(parts);
    let list_id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(list_id))
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Builds a `regex::Regex` from a pattern string and Python-style flags.
///
/// Translates Python flag constants into the equivalent `(?...)` inline flags
/// prepended to the pattern, then compiles with the `regex` crate.
fn build_regex(pattern: &str, flags: i64) -> RunResult<Regex> {
    if let Some(pos) = find_unterminated_character_set(pattern) {
        #[expect(clippy::cast_possible_wrap)]
        let pos_i64 = pos as i64;
        #[expect(clippy::cast_possible_wrap)]
        let colno = (pos + 1) as i64;
        return Err(SimpleException::new_regex_error(
            format!("unterminated character set at position {pos}"),
            pos_i64,
            1,
            colno,
        )
        .into());
    }
    if let Some(name) = find_duplicate_named_group(pattern) {
        return Err(
            SimpleException::new_msg(ExcType::Exception, format!("redefinition of group name '{name}'")).into(),
        );
    }

    let mut prefix = String::new();
    if flags & IGNORECASE != 0 {
        prefix.push_str("(?i)");
    }
    if flags & MULTILINE != 0 {
        prefix.push_str("(?m)");
    }
    if flags & DOTALL != 0 {
        prefix.push_str("(?s)");
    }
    if flags & VERBOSE != 0 {
        prefix.push_str("(?x)");
    }
    let pattern = if flags & ASCII != 0 {
        ascii_pattern(pattern)
    } else {
        pattern.to_string()
    };

    let full_pattern = if prefix.is_empty() {
        pattern
    } else {
        format!("{prefix}{pattern}")
    };

    Regex::new(&full_pattern)
        .map_err(|e| SimpleException::new_msg(ExcType::Exception, format!("invalid regex pattern: {e}")).into())
}

/// Rewrites common Unicode character classes into ASCII-only equivalents.
fn ascii_pattern(pattern: &str) -> String {
    pattern
        .replace(r"\w", "[A-Za-z0-9_]")
        .replace(r"\W", "[^A-Za-z0-9_]")
        .replace(r"\d", "[0-9]")
        .replace(r"\D", "[^0-9]")
}

/// Finds the first unmatched `[` in a regex pattern.
fn find_unterminated_character_set(pattern: &str) -> Option<usize> {
    let mut escaped = false;
    let mut class_start: Option<usize> = None;
    for (idx, ch) in pattern.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if class_start.is_some() {
            if ch == ']' {
                class_start = None;
            }
            continue;
        }
        if ch == '[' {
            class_start = Some(idx);
        }
    }
    class_start
}

/// Finds the first duplicated `(?P<name>...)` group name in a pattern.
fn find_duplicate_named_group(pattern: &str) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    let bytes = pattern.as_bytes();
    let mut i = 0usize;
    while i + 4 < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'(' && bytes[i + 1] == b'?' && bytes[i + 2] == b'P' && bytes[i + 3] == b'<' {
            let name_start = i + 4;
            let mut j = name_start;
            while j < bytes.len() && bytes[j] != b'>' {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            let name = &pattern[name_start..j];
            if names.iter().any(|existing| existing == name) {
                return Some(name.to_string());
            }
            names.push(name.to_string());
            i = j + 1;
            continue;
        }
        i += 1;
    }
    None
}

/// Converts a fancy-regex runtime error into a Python exception.
fn regex_error(err: impl std::fmt::Display) -> crate::exception_private::RunError {
    SimpleException::new_msg(ExcType::Exception, format!("invalid regex pattern: {err}")).into()
}

/// Extracts captured groups (1, 2, …) from a `Captures` into `Vec<Option<String>>`.
fn extract_groups(caps: &Captures<'_>) -> Vec<Option<String>> {
    (1..caps.len())
        .map(|i| caps.get(i).map(|m| m.as_str().to_owned()))
        .collect()
}

/// Parses scanner lexicon entries for `re.Scanner`.
fn parse_scanner_rules(
    lexicon: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<ReScannerRule>> {
    let entries: Vec<Value> = match lexicon {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::List(list) => list.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect(),
            HeapData::Tuple(tuple) => tuple.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect(),
            _ => return Err(ExcType::type_error("lexicon must be a list of 2-tuples")),
        },
        _ => return Err(ExcType::type_error("lexicon must be a list of 2-tuples")),
    };

    let mut rules = Vec::with_capacity(entries.len());
    for entry in entries {
        let pair: Vec<Value> = if let Value::Ref(id) = &entry {
            match heap.get(*id) {
                HeapData::Tuple(tuple) => {
                    if tuple.as_vec().len() != 2 {
                        entry.drop_with_heap(heap);
                        return Err(ExcType::type_error("lexicon entries must be 2-tuples"));
                    }
                    tuple.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect()
                }
                HeapData::List(list) => {
                    if list.as_vec().len() != 2 {
                        entry.drop_with_heap(heap);
                        return Err(ExcType::type_error("lexicon entries must be 2-tuples"));
                    }
                    list.as_vec().iter().map(|item| item.clone_with_heap(heap)).collect()
                }
                _ => {
                    entry.drop_with_heap(heap);
                    return Err(ExcType::type_error("lexicon entries must be 2-tuples"));
                }
            }
        } else {
            entry.drop_with_heap(heap);
            return Err(ExcType::type_error("lexicon entries must be 2-tuples"));
        };
        entry.drop_with_heap(heap);

        let mut iter = pair.into_iter();
        let pattern_val = iter.next().expect("len checked");
        let action_val = iter.next().expect("len checked");

        let Some((pattern, _is_bytes)) = get_text(&pattern_val, heap, interns) else {
            pattern_val.drop_with_heap(heap);
            action_val.drop_with_heap(heap);
            return Err(ExcType::type_error("scanner pattern must be a string"));
        };
        pattern_val.drop_with_heap(heap);

        let tag = if matches!(action_val, Value::None) {
            None
        } else {
            Some(infer_scanner_tag(&pattern))
        };
        action_val.drop_with_heap(heap);

        rules.push(ReScannerRule { pattern, tag });
    }

    Ok(rules)
}

/// Infers a scanner token label from the pattern.
fn infer_scanner_tag(pattern: &str) -> String {
    if pattern.contains(r"\d") || pattern.contains("[0-9]") {
        "NUMBER".to_string()
    } else if pattern.contains("[a-zA-Z]") || pattern.contains(r"\w") {
        "WORD".to_string()
    } else {
        "TOKEN".to_string()
    }
}

/// Extracts captured group spans from a capture set.
fn extract_group_spans(caps: &Captures<'_>) -> Vec<Option<(usize, usize)>> {
    (1..caps.len())
        .map(|i| caps.get(i).map(|m| (m.start(), m.end())))
        .collect()
}

/// Extracts named capture group mappings from a compiled regex.
fn extract_groupindex(regex: &Regex) -> Vec<(String, usize)> {
    regex
        .capture_names()
        .enumerate()
        .filter_map(|(index, name)| name.map(|name| (name.to_owned(), index)))
        .collect()
}

/// Extracts two required text args plus an optional integer flags arg.
///
/// Returns `(pattern, string, flags, is_bytes)`.
fn extract_two_or_three_text_args(
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, String, i64, bool)> {
    let (pos, kwargs) = args.into_parts();
    kwargs.drop_with_heap(heap);
    let mut pos_iter = pos;

    let pattern_val = pos_iter.next();
    let string_val = pos_iter.next();
    let flags_val = pos_iter.next();
    for extra in pos_iter {
        extra.drop_with_heap(heap);
    }

    let Some(pv) = pattern_val else {
        if let Some(sv) = string_val {
            sv.drop_with_heap(heap);
        }
        if let Some(fv) = flags_val {
            fv.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'pattern'"
        )));
    };
    let Some(sv) = string_val else {
        pv.drop_with_heap(heap);
        if let Some(fv) = flags_val {
            fv.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'string'"
        )));
    };

    let Some((pattern_str, pattern_is_bytes)) = get_text(&pv, heap, interns) else {
        pv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        if let Some(fv) = flags_val {
            fv.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("first argument must be a string or bytes"));
    };

    let Some((string_str, string_is_bytes)) = get_text(&sv, heap, interns) else {
        pv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        if let Some(fv) = flags_val {
            fv.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("second argument must be a string or bytes"));
    };
    if pattern_is_bytes != string_is_bytes {
        pv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        if let Some(fv) = flags_val {
            fv.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "cannot use a bytes pattern on a string-like object",
        ));
    }

    pv.drop_with_heap(heap);
    sv.drop_with_heap(heap);

    let flags = extract_int_flag(flags_val, heap);

    Ok((pattern_str, string_str, flags, pattern_is_bytes))
}

/// Extracts args for `re.sub` and `re.subn`:
/// `(pattern, repl, string, count=0, flags=0)`.
fn extract_sub_args(
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, ReReplacement, String, usize, i64, bool)> {
    let (pos, kwargs) = args.into_parts();
    let mut pos_iter = pos;

    // Collect all positional args first, then validate
    let pattern_val = pos_iter.next();
    let repl_val = pos_iter.next();
    let string_val = pos_iter.next();
    let count_val = pos_iter.next();
    let flags_val = pos_iter.next();
    for extra in pos_iter {
        extra.drop_with_heap(heap);
    }
    let mut kw_count: Option<Value> = None;
    let mut kw_flags: Option<Value> = None;
    for (key, value) in kwargs {
        let key_name = key.py_str(heap, interns).into_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "count" => {
                if let Some(old) = kw_count.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            "flags" => {
                if let Some(old) = kw_flags.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            _ => {
                value.drop_with_heap(heap);
                kw_count.drop_with_heap(heap);
                kw_flags.drop_with_heap(heap);
                pattern_val.drop_with_heap(heap);
                repl_val.drop_with_heap(heap);
                string_val.drop_with_heap(heap);
                count_val.drop_with_heap(heap);
                flags_val.drop_with_heap(heap);
                return Err(ExcType::type_error(format!("invalid keyword argument for {name}")));
            }
        }
    }

    // Helper to drop all remaining args on error
    macro_rules! drop_all {
        ($($v:expr),*) => {
            $(if let Some(v) = $v { v.drop_with_heap(heap); })*
        };
    }

    let Some(pv) = pattern_val else {
        drop_all!(repl_val, string_val, count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'pattern'"
        )));
    };
    let Some(rv) = repl_val else {
        pv.drop_with_heap(heap);
        drop_all!(string_val, count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'repl'"
        )));
    };
    let Some(sv) = string_val else {
        pv.drop_with_heap(heap);
        rv.drop_with_heap(heap);
        drop_all!(count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'string'"
        )));
    };

    let Some((pattern_str, pattern_is_bytes)) = get_text(&pv, heap, interns) else {
        pv.drop_with_heap(heap);
        rv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        drop_all!(count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error("first argument must be a string or bytes"));
    };
    let (repl, repl_is_bytes) = if let Some((repl_str, repl_is_bytes)) = get_text(&rv, heap, interns) {
        (ReReplacement::Template(repl_str), repl_is_bytes)
    } else if is_callable_value(&rv, heap) {
        (ReReplacement::Callable(rv.clone_with_heap(heap)), pattern_is_bytes)
    } else {
        pv.drop_with_heap(heap);
        rv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        drop_all!(count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error("second argument must be a string or callable"));
    };
    let Some((string_str, string_is_bytes)) = get_text(&sv, heap, interns) else {
        pv.drop_with_heap(heap);
        rv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        drop_all!(count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error("third argument must be a string or bytes"));
    };
    if pattern_is_bytes != string_is_bytes || repl_is_bytes != string_is_bytes {
        pv.drop_with_heap(heap);
        rv.drop_with_heap(heap);
        sv.drop_with_heap(heap);
        drop_all!(count_val, flags_val);
        kw_count.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error("cannot mix bytes and string arguments"));
    }

    pv.drop_with_heap(heap);
    rv.drop_with_heap(heap);
    sv.drop_with_heap(heap);

    let count = extract_int_count(kw_count.or(count_val), heap);
    let flags = extract_int_flag(kw_flags.or(flags_val), heap);

    Ok((pattern_str, repl, string_str, count, flags, pattern_is_bytes))
}

/// Extracts args for `re.split(pattern, string, maxsplit=0, flags=0)`.
fn extract_split_args(
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, String, usize, i64, bool)> {
    let (mut pos, kwargs) = args.into_parts();
    let pattern = pos.next();
    let string = pos.next();
    let positional_maxsplit = pos.next();
    let positional_flags = pos.next();
    for extra in pos {
        extra.drop_with_heap(heap);
    }

    let mut kw_maxsplit: Option<Value> = None;
    let mut kw_flags: Option<Value> = None;
    for (key, value) in kwargs {
        let key_name = key.py_str(heap, interns).into_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "maxsplit" => {
                if let Some(old) = kw_maxsplit.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            "flags" => {
                if let Some(old) = kw_flags.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            _ => {
                value.drop_with_heap(heap);
                if let Some(v) = kw_maxsplit {
                    v.drop_with_heap(heap);
                }
                if let Some(v) = kw_flags {
                    v.drop_with_heap(heap);
                }
                return Err(ExcType::type_error(format!("invalid keyword argument for {name}")));
            }
        }
    }

    let Some(pattern_value) = pattern else {
        positional_maxsplit.drop_with_heap(heap);
        positional_flags.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'pattern'"
        )));
    };
    let Some(string_value) = string else {
        pattern_value.drop_with_heap(heap);
        positional_maxsplit.drop_with_heap(heap);
        positional_flags.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{name}() missing required argument 'string'"
        )));
    };

    let Some((pattern_str, pattern_is_bytes)) = get_text(&pattern_value, heap, interns) else {
        pattern_value.drop_with_heap(heap);
        string_value.drop_with_heap(heap);
        positional_maxsplit.drop_with_heap(heap);
        positional_flags.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error("first argument must be a string or bytes"));
    };
    let Some((string_str, string_is_bytes)) = get_text(&string_value, heap, interns) else {
        pattern_value.drop_with_heap(heap);
        string_value.drop_with_heap(heap);
        positional_maxsplit.drop_with_heap(heap);
        positional_flags.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error("second argument must be a string or bytes"));
    };
    if pattern_is_bytes != string_is_bytes {
        pattern_value.drop_with_heap(heap);
        string_value.drop_with_heap(heap);
        positional_maxsplit.drop_with_heap(heap);
        positional_flags.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        kw_flags.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "cannot use a bytes pattern on a string-like object",
        ));
    }

    pattern_value.drop_with_heap(heap);
    string_value.drop_with_heap(heap);
    let maxsplit = extract_int_count(kw_maxsplit.or(positional_maxsplit), heap);
    let flags = extract_int_flag(kw_flags.or(positional_flags), heap);
    Ok((pattern_str, string_str, maxsplit, flags, pattern_is_bytes))
}

/// Extracts an optional integer value from a `Value`, defaulting to 0.
fn extract_int_flag(val: Option<Value>, heap: &mut Heap<impl ResourceTracker>) -> i64 {
    match val {
        Some(Value::Int(i)) => i,
        Some(Value::Ref(id)) => {
            let bits = if let HeapData::StdlibObject(StdlibObject::RegexFlagValue(bits)) = heap.get(id) {
                *bits
            } else {
                0
            };
            heap.dec_ref(id);
            bits
        }
        Some(other) => {
            other.drop_with_heap(heap);
            0
        }
        None => 0,
    }
}

/// Extracts an optional count integer, defaulting to 0 (meaning "all").
fn extract_int_count(val: Option<Value>, heap: &mut Heap<impl ResourceTracker>) -> usize {
    match val {
        #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Some(Value::Int(i)) if i >= 0 => i as usize,
        Some(other) => {
            other.drop_with_heap(heap);
            0
        }
        None => 0,
    }
}

/// Extracts text from `str`/`bytes` values and returns `(text, is_bytes)`.
fn get_text(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> Option<(String, bool)> {
    match value {
        Value::InternString(id) => Some((interns.get_str(*id).to_string(), false)),
        Value::InternBytes(id) => Some((String::from_utf8_lossy(interns.get_bytes(*id)).to_string(), true)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some((s.as_str().to_string(), false)),
            HeapData::Bytes(b) | HeapData::Bytearray(b) => {
                Some((String::from_utf8_lossy(b.as_slice()).to_string(), true))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Returns true when a value can be invoked like a Python callable.
fn is_callable_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    match value {
        Value::Builtin(_) | Value::ModuleFunction(_) | Value::DefFunction(_) | Value::ExtFunction(_) => true,
        Value::Ref(id) => matches!(
            heap.get(*id),
            HeapData::Closure(_, _, _)
                | HeapData::FunctionDefaults(_, _)
                | HeapData::ClassObject(_)
                | HeapData::BoundMethod(_)
                | HeapData::Partial(_)
                | HeapData::SingleDispatch(_)
                | HeapData::SingleDispatchRegister(_)
                | HeapData::SingleDispatchMethod(_)
                | HeapData::CmpToKey(_)
                | HeapData::ItemGetter(_)
                | HeapData::AttrGetter(_)
                | HeapData::MethodCaller(_)
                | HeapData::PropertyAccessor(_)
        ),
        _ => false,
    }
}

/// Returns `true` if the character is a regex metacharacter that needs escaping.
///
/// Matches the set of characters escaped by CPython's `re.escape()`.
fn is_regex_metacharacter(ch: char) -> bool {
    matches!(
        ch,
        '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '{' | '}' | '[' | ']' | '|' | '(' | ')'
    )
}

/// Expands a replacement template using a concrete `re.Match`.
///
/// Supports numeric (`\1`) and named (`\g<name>`) backreferences and escaped backslashes.
pub(crate) fn expand_template(
    template: &str,
    match_obj: &ReMatch,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch != '\\' {
            out.push(ch);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            out.push('\\');
            break;
        }
        match chars[i] {
            '\\' => {
                out.push('\\');
                i += 1;
            }
            'g' => {
                i += 1;
                if i < chars.len() && chars[i] == '<' {
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != '>' {
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(ExcType::type_error("invalid group reference"));
                    }
                    let name: String = chars[start..i].iter().collect();
                    i += 1;
                    let group = if name == "0" {
                        match_obj.matched.clone()
                    } else if let Ok(index) = name.parse::<i64>() {
                        let value = match_obj.group_value(index as usize, heap)?;
                        let text = value.py_str(heap, interns).into_owned();
                        value.drop_with_heap(heap);
                        text
                    } else {
                        let index = match_obj.resolve_group_name(&name)?;
                        let value = match_obj.group_value(index, heap)?;
                        let text = value.py_str(heap, interns).into_owned();
                        value.drop_with_heap(heap);
                        text
                    };
                    out.push_str(&group);
                } else {
                    out.push('g');
                }
            }
            d if d.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let number: String = chars[start..i].iter().collect();
                let index = number.parse::<usize>().unwrap_or(0);
                let value = match_obj.group_value(index, heap)?;
                let text = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
                out.push_str(&text);
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Expands a template against a concrete regex capture set.
fn expand_template_with_captures(
    template: &str,
    caps: &Captures<'_>,
    groupindex: &[(String, usize)],
) -> RunResult<String> {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        if ch != '\\' {
            out.push(ch);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            out.push('\\');
            break;
        }
        match chars[i] {
            '\\' => {
                out.push('\\');
                i += 1;
            }
            'g' => {
                i += 1;
                if i < chars.len() && chars[i] == '<' {
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != '>' {
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(ExcType::type_error("invalid group reference"));
                    }
                    let token: String = chars[start..i].iter().collect();
                    i += 1;
                    if token == "0" {
                        out.push_str(caps.get(0).map_or("", |m| m.as_str()));
                    } else if let Ok(index) = token.parse::<usize>() {
                        out.push_str(caps.get(index).map_or("", |m| m.as_str()));
                    } else if let Some((_, index)) = groupindex.iter().find(|(name, _)| *name == token) {
                        out.push_str(caps.get(*index).map_or("", |m| m.as_str()));
                    } else {
                        return Err(SimpleException::new_msg(ExcType::IndexError, "no such group").into());
                    }
                } else {
                    out.push('g');
                }
            }
            digit if digit.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let number: String = chars[start..i].iter().collect();
                let index = number.parse::<usize>().unwrap_or(0);
                out.push_str(caps.get(index).map_or("", |m| m.as_str()));
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }

    Ok(out)
}

/// Executes regex substitution with a template string and Python-style `\1`/`\g<name>` backrefs.
///
/// This only handles template (string) replacements. Callable replacements are
/// handled by the VM via `AttrCallResult::ReSubCall`.
fn run_substitution(
    pattern: &str,
    template: &str,
    string: &str,
    flags: i64,
    count: usize,
) -> RunResult<(String, usize)> {
    let regex = build_regex(pattern, flags)?;
    let groupindex = extract_groupindex(&regex);
    let mut result = String::with_capacity(string.len());
    let mut last_end = 0usize;
    let mut search_pos = 0usize;
    let mut replaced = 0usize;

    while search_pos <= string.len() {
        if count > 0 && replaced >= count {
            break;
        }
        let Some(caps) = regex.captures_from_pos(string, search_pos).map_err(regex_error)? else {
            break;
        };
        let m = caps.get(0).expect("group 0 always exists");
        result.push_str(&string[last_end..m.start()]);
        let replacement = expand_template_with_captures(template, &caps, &groupindex)?;
        result.push_str(&replacement);
        last_end = m.end();
        replaced += 1;

        if m.start() == m.end() {
            if search_pos >= string.len() {
                break;
            }
            // Advance one UTF-8 codepoint to avoid infinite zero-width matches.
            let next = string[search_pos..]
                .char_indices()
                .nth(1)
                .map_or(string.len(), |(offset, _)| search_pos + offset);
            search_pos = next;
        } else {
            search_pos = m.end();
        }
    }
    result.push_str(&string[last_end..]);
    Ok((result, replaced))
}

/// Pre-computes all regex match objects for callable `re.sub`/`re.subn`.
///
/// Finds all matches (up to `count` if non-zero) and creates `ReMatch` objects
/// on the heap. These are passed to the VM which calls the user's callback for
/// each match and assembles the final string.
///
/// Returns a vector of `(match_start, match_end, match_value)` tuples where
/// `match_value` is a heap-allocated `ReMatch`.
pub(crate) fn collect_match_objects(
    pattern: &str,
    string: &str,
    flags: i64,
    count: usize,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Vec<(usize, usize, Value)>> {
    let regex = build_regex(pattern, flags)?;
    let groups_count = regex.captures_len().saturating_sub(1);
    let groupindex = extract_groupindex(&regex);

    let mut matches = Vec::new();
    let mut search_pos = 0usize;
    let mut replaced = 0usize;

    while search_pos <= string.len() {
        if count > 0 && replaced >= count {
            break;
        }
        let Some(caps) = regex.captures_from_pos(string, search_pos).map_err(regex_error)? else {
            break;
        };
        let m = caps.get(0).expect("group 0 always exists");
        let groups = extract_groups(&caps);
        let group_spans = extract_group_spans(&caps);
        let match_obj = ReMatch::new(
            m.as_str().to_owned(),
            m.start(),
            m.end(),
            0,
            string.len(),
            groups,
            group_spans,
            string.to_owned(),
            is_bytes,
            pattern.to_owned(),
            flags,
            groups_count,
            groupindex.clone(),
        );
        let id = heap.allocate(HeapData::ReMatch(match_obj))?;
        matches.push((m.start(), m.end(), Value::Ref(id)));
        replaced += 1;

        if m.start() == m.end() {
            if search_pos >= string.len() {
                break;
            }
            let next = string[search_pos..]
                .char_indices()
                .nth(1)
                .map_or(string.len(), |(offset, _)| search_pos + offset);
            search_pos = next;
        } else {
            search_pos = m.end();
        }
    }
    Ok(matches)
}

/// Parses `Pattern.<op>(string, pos=0, endpos=len(string))` style arguments.
fn extract_pattern_target(
    name: &str,
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, usize, usize)> {
    let (mut pos, kwargs) = args.into_parts();
    let string_value = pos
        .next()
        .ok_or_else(|| ExcType::type_error(format!("{name}() missing required argument")))?;
    let positional_pos = pos.next();
    let positional_endpos = pos.next();
    for extra in pos {
        extra.drop_with_heap(heap);
    }

    let mut kw_pos: Option<Value> = None;
    let mut kw_endpos: Option<Value> = None;
    for (key, value) in kwargs {
        let key_name = key.py_str(heap, interns).into_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "pos" => {
                if let Some(old) = kw_pos.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            "endpos" => {
                if let Some(old) = kw_endpos.replace(value) {
                    old.drop_with_heap(heap);
                }
            }
            _ => {
                value.drop_with_heap(heap);
                kw_pos.drop_with_heap(heap);
                kw_endpos.drop_with_heap(heap);
                string_value.drop_with_heap(heap);
                positional_pos.drop_with_heap(heap);
                positional_endpos.drop_with_heap(heap);
                return Err(ExcType::type_error(format!("invalid keyword argument for {name}")));
            }
        }
    }

    let Some((string, string_is_bytes)) = get_text(&string_value, heap, interns) else {
        string_value.drop_with_heap(heap);
        positional_pos.drop_with_heap(heap);
        positional_endpos.drop_with_heap(heap);
        kw_pos.drop_with_heap(heap);
        kw_endpos.drop_with_heap(heap);
        return Err(ExcType::type_error("expected string or bytes"));
    };
    if string_is_bytes != is_bytes {
        string_value.drop_with_heap(heap);
        positional_pos.drop_with_heap(heap);
        positional_endpos.drop_with_heap(heap);
        kw_pos.drop_with_heap(heap);
        kw_endpos.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "cannot use a bytes pattern on a string-like object",
        ));
    }
    string_value.drop_with_heap(heap);

    let len = string.len();
    let mut start = extract_index(kw_pos.or(positional_pos), heap).unwrap_or(0);
    let mut end = extract_index(kw_endpos.or(positional_endpos), heap).unwrap_or(len);
    start = start.min(len);
    end = end.min(len);
    if end < start {
        end = start;
    }
    Ok((string, start, end))
}

/// Parses `Pattern.split(string, maxsplit=0)`.
fn extract_pattern_split_target(
    is_bytes: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<(String, usize)> {
    let (mut pos, kwargs) = args.into_parts();
    let string_value = pos
        .next()
        .ok_or_else(|| ExcType::type_error("Pattern.split() missing required argument"))?;
    let positional_maxsplit = pos.next();
    for extra in pos {
        extra.drop_with_heap(heap);
    }
    let mut kw_maxsplit: Option<Value> = None;
    for (key, value) in kwargs {
        let key_name = key.py_str(heap, interns).into_owned();
        key.drop_with_heap(heap);
        if key_name == "maxsplit" {
            if let Some(old) = kw_maxsplit.replace(value) {
                old.drop_with_heap(heap);
            }
        } else {
            value.drop_with_heap(heap);
            kw_maxsplit.drop_with_heap(heap);
            string_value.drop_with_heap(heap);
            positional_maxsplit.drop_with_heap(heap);
            return Err(ExcType::type_error("invalid keyword argument for Pattern.split"));
        }
    }

    let Some((string, string_is_bytes)) = get_text(&string_value, heap, interns) else {
        string_value.drop_with_heap(heap);
        positional_maxsplit.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        return Err(ExcType::type_error("expected string or bytes"));
    };
    if string_is_bytes != is_bytes {
        string_value.drop_with_heap(heap);
        positional_maxsplit.drop_with_heap(heap);
        kw_maxsplit.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "cannot use a bytes pattern on a string-like object",
        ));
    }
    string_value.drop_with_heap(heap);

    let maxsplit = extract_int_count(kw_maxsplit.or(positional_maxsplit), heap);
    Ok((string, maxsplit))
}

/// Converts a value to a non-negative index.
fn extract_index(value: Option<Value>, heap: &mut Heap<impl ResourceTracker>) -> Option<usize> {
    let value = value?;
    let index = match &value {
        #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Value::Int(i) if *i >= 0 => *i as usize,
        _ => 0,
    };
    value.drop_with_heap(heap);
    Some(index)
}

/// Rewrites match fields from bounded-slice coordinates to original-string coordinates.
fn adjust_match_bounds(
    result: Value,
    offset: usize,
    endpos: usize,
    original: &str,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    if let Value::Ref(id) = result
        && let HeapData::ReMatch(m) = heap.get_mut(id)
    {
        m.start += offset;
        m.end += offset;
        m.pos = offset;
        m.endpos = endpos;
        m.string = original.to_owned();
        for (start, end) in m.group_spans.iter_mut().flatten() {
            *start += offset;
            *end += offset;
        }
    }
    Ok(result)
}

/// Wrapper for `Pattern.search`.
pub(crate) fn pattern_search(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (string_str, pos, endpos) = extract_pattern_target("Pattern.search", pattern.is_bytes, heap, interns, args)?;
    let bounded = &string_str[pos..endpos];
    let result = do_search(&pattern.pattern, bounded, pattern.flags, pattern.is_bytes, heap)?;
    adjust_match_bounds(result, pos, endpos, &string_str, heap)
}

/// Wrapper for `Pattern.match`.
pub(crate) fn pattern_match(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (string_str, pos, endpos) = extract_pattern_target("Pattern.match", pattern.is_bytes, heap, interns, args)?;
    let bounded = &string_str[pos..endpos];
    let result = do_match(&pattern.pattern, bounded, pattern.flags, pattern.is_bytes, heap)?;
    adjust_match_bounds(result, pos, endpos, &string_str, heap)
}

/// Wrapper for `Pattern.fullmatch`.
pub(crate) fn pattern_fullmatch(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (string_str, pos, endpos) = extract_pattern_target("Pattern.fullmatch", pattern.is_bytes, heap, interns, args)?;
    let bounded = &string_str[pos..endpos];
    let result = do_fullmatch(&pattern.pattern, bounded, pattern.flags, pattern.is_bytes, heap)?;
    adjust_match_bounds(result, pos, endpos, &string_str, heap)
}

/// Wrapper for `Pattern.findall`.
pub(crate) fn pattern_findall(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let string = args.get_one_arg("Pattern.findall", heap)?;
    let string_str = string.py_str(heap, interns).into_owned();
    string.drop_with_heap(heap);
    do_findall(&pattern.pattern, &string_str, pattern.flags, pattern.is_bytes, heap)
}

/// Wrapper for `Pattern.finditer`.
pub(crate) fn pattern_finditer(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let string = args.get_one_arg("Pattern.finditer", heap)?;
    let string_str = string.py_str(heap, interns).into_owned();
    string.drop_with_heap(heap);
    do_finditer(&pattern.pattern, &string_str, pattern.flags, pattern.is_bytes, heap)
}

/// Wrapper for `Pattern.sub` — returns `AttrCallResult` to support callable replacements.
pub(crate) fn pattern_sub(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (repl, string) = args.get_two_args("Pattern.sub", heap)?;
    let string_str = string.py_str(heap, interns).into_owned();
    string.drop_with_heap(heap);
    let repl_mode = if is_callable_value(&repl, heap) {
        let callable = repl.clone_with_heap(heap);
        repl.drop_with_heap(heap);
        ReReplacement::Callable(callable)
    } else {
        let repl_str = repl.py_str(heap, interns).into_owned();
        repl.drop_with_heap(heap);
        ReReplacement::Template(repl_str)
    };
    do_sub(
        &pattern.pattern,
        repl_mode,
        &string_str,
        pattern.flags,
        0,
        pattern.is_bytes,
        heap,
    )
}

/// Wrapper for `Pattern.subn` — returns `AttrCallResult` to support callable replacements.
pub(crate) fn pattern_subn(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (repl, string) = args.get_two_args("Pattern.subn", heap)?;
    let string_str = string.py_str(heap, interns).into_owned();
    string.drop_with_heap(heap);
    let repl_mode = if is_callable_value(&repl, heap) {
        let callable = repl.clone_with_heap(heap);
        repl.drop_with_heap(heap);
        ReReplacement::Callable(callable)
    } else {
        let repl_str = repl.py_str(heap, interns).into_owned();
        repl.drop_with_heap(heap);
        ReReplacement::Template(repl_str)
    };
    do_subn(
        &pattern.pattern,
        repl_mode,
        &string_str,
        pattern.flags,
        0,
        pattern.is_bytes,
        heap,
    )
}

/// Wrapper for `Pattern.split`.
pub(crate) fn pattern_split(
    pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (string_str, maxsplit) = extract_pattern_split_target(pattern.is_bytes, heap, interns, args)?;
    do_split(
        &pattern.pattern,
        &string_str,
        pattern.flags,
        maxsplit,
        pattern.is_bytes,
        heap,
    )
}

/// Wrapper for `Pattern.scanner`.
pub(crate) fn pattern_scanner(
    _pattern: &RePattern,
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let target = args.get_one_arg("Pattern.scanner", heap)?;
    target.drop_with_heap(heap);
    let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_empty_re_scanner()))?;
    Ok(Value::Ref(id))
}
