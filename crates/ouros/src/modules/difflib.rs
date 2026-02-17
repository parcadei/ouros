//! Compatibility implementation of the `difflib` module.
//!
//! This module provides a pure-Rust compatibility layer for the commonly used
//! `difflib` API surface:
//! - `SequenceMatcher`, `Differ`, `HtmlDiff`, `Match`
//! - `IS_CHARACTER_JUNK`, `IS_LINE_JUNK`
//! - `get_close_matches`, `ndiff`, `restore`, `unified_diff`, `context_diff`,
//!   `diff_bytes`
//!
//! The implementation is intentionally self-contained in this file so the
//! `difflib` work stays scoped to one module.

use std::{cmp::Ordering, fmt::Write};

use smallvec::smallvec;

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, Instance, List, Module, NamedTuple, NamedTupleFactory, OurosIter, Partial,
        PyTrait, Str, Type, allocate_tuple, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

const SEQ_A_ATTR: &str = "_difflib_seq_a";
const SEQ_B_ATTR: &str = "_difflib_seq_b";
const SEQ_ISJUNK_ATTR: &str = "_difflib_isjunk";
const SEQ_AUTOJUNK_ATTR: &str = "_difflib_autojunk";
const DIFFER_LINEJUNK_ATTR: &str = "_difflib_linejunk";
const DIFFER_CHARJUNK_ATTR: &str = "_difflib_charjunk";
const HTML_TABSIZE_ATTR: &str = "_difflib_tabsize";
const HTML_WRAPCOLUMN_ATTR: &str = "_difflib_wrapcolumn";
const HTML_LINEJUNK_ATTR: &str = "_difflib_linejunk";
const HTML_CHARJUNK_ATTR: &str = "_difflib_charjunk";

/// `difflib` module callables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum DifflibFunctions {
    #[strum(serialize = "IS_CHARACTER_JUNK")]
    IsCharacterJunk,
    #[strum(serialize = "IS_LINE_JUNK")]
    IsLineJunk,
    ContextDiff,
    DiffBytes,
    GetCloseMatches,
    Ndiff,
    Restore,
    UnifiedDiff,

    // `SequenceMatcher` runtime class methods.
    SequenceMatcherNew,
    SequenceMatcherInit,
    SequenceMatcherSetSeqs,
    SequenceMatcherSetSeq1,
    SequenceMatcherSetSeq2,
    SequenceMatcherFindLongestMatch,
    SequenceMatcherGetMatchingBlocks,
    SequenceMatcherGetOpcodes,
    SequenceMatcherGetGroupedOpcodes,
    SequenceMatcherRatio,
    SequenceMatcherQuickRatio,
    SequenceMatcherRealQuickRatio,

    // `Differ` runtime class methods.
    DifferNew,
    DifferInit,
    DifferCompare,

    // `HtmlDiff` runtime class methods.
    HtmlDiffNew,
    HtmlDiffInit,
    HtmlDiffMakeFile,
    HtmlDiffMakeTable,
}

/// Creates the `difflib` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Difflib);

    register_callable(
        &mut module,
        "IS_CHARACTER_JUNK",
        DifflibFunctions::IsCharacterJunk,
        heap,
        interns,
    )?;
    register_callable(&mut module, "IS_LINE_JUNK", DifflibFunctions::IsLineJunk, heap, interns)?;
    register_callable(
        &mut module,
        "context_diff",
        DifflibFunctions::ContextDiff,
        heap,
        interns,
    )?;
    register_callable(&mut module, "diff_bytes", DifflibFunctions::DiffBytes, heap, interns)?;
    register_callable(
        &mut module,
        "get_close_matches",
        DifflibFunctions::GetCloseMatches,
        heap,
        interns,
    )?;
    register_callable(&mut module, "ndiff", DifflibFunctions::Ndiff, heap, interns)?;
    register_callable(&mut module, "restore", DifflibFunctions::Restore, heap, interns)?;
    register_callable(
        &mut module,
        "unified_diff",
        DifflibFunctions::UnifiedDiff,
        heap,
        interns,
    )?;

    // CPython exports GenericAlias from types.
    module.set_attr_text(
        "GenericAlias",
        Value::Builtin(Builtins::Type(Type::GenericAlias)),
        heap,
        interns,
    )?;

    let match_id = create_match_factory(heap)?;
    module.set_attr_text("Match", Value::Ref(match_id), heap, interns)?;

    let sequence_matcher_class_id = create_sequence_matcher_class(heap, interns)?;
    module.set_attr_text("SequenceMatcher", Value::Ref(sequence_matcher_class_id), heap, interns)?;

    let differ_class_id = create_differ_class(heap, interns)?;
    module.set_attr_text("Differ", Value::Ref(differ_class_id), heap, interns)?;

    let html_diff_class_id = create_html_diff_class(heap, interns)?;
    module.set_attr_text("HtmlDiff", Value::Ref(html_diff_class_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `difflib` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: DifflibFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        DifflibFunctions::IsCharacterJunk => is_character_junk(heap, interns, args)?,
        DifflibFunctions::IsLineJunk => is_line_junk(heap, interns, args)?,
        DifflibFunctions::ContextDiff => context_diff(heap, interns, args)?,
        DifflibFunctions::DiffBytes => diff_bytes(heap, interns, args)?,
        DifflibFunctions::GetCloseMatches => get_close_matches(heap, interns, args)?,
        DifflibFunctions::Ndiff => ndiff(heap, interns, args)?,
        DifflibFunctions::Restore => restore(heap, interns, args)?,
        DifflibFunctions::UnifiedDiff => unified_diff(heap, interns, args)?,

        DifflibFunctions::SequenceMatcherNew => sequence_matcher_new(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherInit => trivial_init(heap, args),
        DifflibFunctions::SequenceMatcherSetSeqs => sequence_matcher_set_seqs(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherSetSeq1 => sequence_matcher_set_seq1(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherSetSeq2 => sequence_matcher_set_seq2(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherFindLongestMatch => sequence_matcher_find_longest_match(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherGetMatchingBlocks => {
            sequence_matcher_get_matching_blocks(heap, interns, args)?
        }
        DifflibFunctions::SequenceMatcherGetOpcodes => sequence_matcher_get_opcodes(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherGetGroupedOpcodes => {
            sequence_matcher_get_grouped_opcodes(heap, interns, args)?
        }
        DifflibFunctions::SequenceMatcherRatio => sequence_matcher_ratio(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherQuickRatio => sequence_matcher_quick_ratio(heap, interns, args)?,
        DifflibFunctions::SequenceMatcherRealQuickRatio => sequence_matcher_real_quick_ratio(heap, interns, args)?,

        DifflibFunctions::DifferNew => differ_new(heap, interns, args)?,
        DifflibFunctions::DifferInit => trivial_init(heap, args),
        DifflibFunctions::DifferCompare => differ_compare(heap, interns, args)?,

        DifflibFunctions::HtmlDiffNew => html_diff_new(heap, interns, args)?,
        DifflibFunctions::HtmlDiffInit => trivial_init(heap, args),
        DifflibFunctions::HtmlDiffMakeFile => html_diff_make_file(heap, interns, args)?,
        DifflibFunctions::HtmlDiffMakeTable => html_diff_make_table(heap, interns, args)?,
    };

    Ok(AttrCallResult::Value(value))
}

fn trivial_init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> Value {
    args.drop_with_heap(heap);
    Value::None
}

/// Implements `difflib.IS_CHARACTER_JUNK(ch, ws=' \t')`.
fn is_character_junk(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (character, ws) = args.get_one_two_args_with_keyword("IS_CHARACTER_JUNK", "ws", heap, interns)?;
    let character = match parse_text_value(character, heap, interns) {
        Ok(value) => value,
        Err(err) => {
            ws.drop_with_heap(heap);
            return Err(err);
        }
    };

    let ws = if let Some(ws) = ws {
        parse_text_value(ws, heap, interns)?
    } else {
        " \t".to_owned()
    };

    Ok(Value::Bool(ws.contains(character.as_str())))
}

/// Implements `difflib.IS_LINE_JUNK(line, pat=None)`.
fn is_line_junk(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (line, pat) = args.get_one_two_args_with_keyword("IS_LINE_JUNK", "pat", heap, interns)?;
    pat.drop_with_heap(heap);

    let text = parse_text_value(line, heap, interns)?;
    let trimmed_end = text.trim_end_matches(['\n', '\r']);
    let trimmed = trimmed_end.trim_matches([' ', '\t']);
    Ok(Value::Bool(trimmed.is_empty() || trimmed == "#"))
}

/// Implements `difflib.SequenceMatcher.__new__(cls, isjunk=None, a='', b='', autojunk=True)`.
fn sequence_matcher_new(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.by_ref().collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("SequenceMatcher", 1, 0));
    }

    let cls = positional.remove(0);
    let class_id = class_id_from_value(&cls, heap, "SequenceMatcher")?;
    cls.drop_with_heap(heap);

    let mut isjunk = Value::None;
    let mut a = Value::InternString(StaticStrings::EmptyString.into());
    let mut b = Value::InternString(StaticStrings::EmptyString.into());
    let mut autojunk = true;

    if !positional.is_empty() {
        isjunk = positional.remove(0);
    }
    if !positional.is_empty() {
        a = positional.remove(0);
    }
    if !positional.is_empty() {
        b = positional.remove(0);
    }
    if !positional.is_empty() {
        let value = positional.remove(0);
        autojunk = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.pop() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        isjunk.drop_with_heap(heap);
        a.drop_with_heap(heap);
        b.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("SequenceMatcher", 4, 5));
    }

    let mut kwargs_iter = kwargs.into_iter();
    while let Some((key, value)) = kwargs_iter.next() {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            drop_remaining_kwargs(kwargs_iter, heap);
            isjunk.drop_with_heap(heap);
            a.drop_with_heap(heap);
            b.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "isjunk" => {
                isjunk.drop_with_heap(heap);
                isjunk = value;
            }
            "a" => {
                a.drop_with_heap(heap);
                a = value;
            }
            "b" => {
                b.drop_with_heap(heap);
                b = value;
            }
            "autojunk" => {
                autojunk = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                drop_remaining_kwargs(kwargs_iter, heap);
                isjunk.drop_with_heap(heap);
                a.drop_with_heap(heap);
                b.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("SequenceMatcher", &key_name));
            }
        }
    }

    let seq_a = match collect_iterable_strings(a, heap, interns) {
        Ok(lines) => lines,
        Err(err) => {
            isjunk.drop_with_heap(heap);
            b.drop_with_heap(heap);
            return Err(err);
        }
    };
    let seq_b = match collect_iterable_strings(b, heap, interns) {
        Ok(lines) => lines,
        Err(err) => {
            isjunk.drop_with_heap(heap);
            return Err(err);
        }
    };

    let instance_id = create_instance_for_class(class_id, heap)?;
    let mut instance_guard = HeapGuard::new(Value::Ref(instance_id), heap);
    let (_, heap) = instance_guard.as_parts_mut();

    set_instance_attr_by_name(instance_id, SEQ_ISJUNK_ATTR, isjunk, heap, interns)?;
    set_instance_attr_by_name(instance_id, SEQ_AUTOJUNK_ATTR, Value::Bool(autojunk), heap, interns)?;
    set_sequence_attr(instance_id, SEQ_A_ATTR, &seq_a, heap, interns)?;
    set_sequence_attr(instance_id, SEQ_B_ATTR, &seq_b, heap, interns)?;

    attach_bound_method(
        instance_id,
        "set_seqs",
        DifflibFunctions::SequenceMatcherSetSeqs,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "set_seq1",
        DifflibFunctions::SequenceMatcherSetSeq1,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "set_seq2",
        DifflibFunctions::SequenceMatcherSetSeq2,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "find_longest_match",
        DifflibFunctions::SequenceMatcherFindLongestMatch,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "get_matching_blocks",
        DifflibFunctions::SequenceMatcherGetMatchingBlocks,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "get_opcodes",
        DifflibFunctions::SequenceMatcherGetOpcodes,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "get_grouped_opcodes",
        DifflibFunctions::SequenceMatcherGetGroupedOpcodes,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "ratio",
        DifflibFunctions::SequenceMatcherRatio,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "quick_ratio",
        DifflibFunctions::SequenceMatcherQuickRatio,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "real_quick_ratio",
        DifflibFunctions::SequenceMatcherRealQuickRatio,
        heap,
        interns,
    )?;

    let (value, _) = instance_guard.into_parts();
    Ok(value)
}

fn sequence_matcher_set_seqs(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.set_seqs")?;
    let (a, b) = args.get_two_args("SequenceMatcher.set_seqs", heap)?;
    let seq_a = collect_iterable_strings(a, heap, interns)?;
    let seq_b = collect_iterable_strings(b, heap, interns)?;
    set_sequence_attr(self_id, SEQ_A_ATTR, &seq_a, heap, interns)?;
    set_sequence_attr(self_id, SEQ_B_ATTR, &seq_b, heap, interns)?;
    Ok(Value::None)
}

fn sequence_matcher_set_seq1(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.set_seq1")?;
    let a = args.get_one_arg("SequenceMatcher.set_seq1", heap)?;
    let seq_a = collect_iterable_strings(a, heap, interns)?;
    set_sequence_attr(self_id, SEQ_A_ATTR, &seq_a, heap, interns)?;
    Ok(Value::None)
}

fn sequence_matcher_set_seq2(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.set_seq2")?;
    let b = args.get_one_arg("SequenceMatcher.set_seq2", heap)?;
    let seq_b = collect_iterable_strings(b, heap, interns)?;
    set_sequence_attr(self_id, SEQ_B_ATTR, &seq_b, heap, interns)?;
    Ok(Value::None)
}

fn sequence_matcher_find_longest_match(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.find_longest_match")?;
    let (a, b) = get_sequence_pair(self_id, heap, interns)?;

    let mut alo = 0usize;
    let mut ahi = a.len();
    let mut blo = 0usize;
    let mut bhi = b.len();

    let (mut positional, kwargs) = args.into_parts();
    if let Some(value) = positional.next() {
        alo = clamp_index(value.as_int(heap)?, a.len());
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        ahi = clamp_index(value.as_int(heap)?, a.len());
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        blo = clamp_index(value.as_int(heap)?, b.len());
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        bhi = clamp_index(value.as_int(heap)?, b.len());
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("SequenceMatcher.find_longest_match", 4, 5));
    }

    if let Some((key, value)) = kwargs.into_iter().next() {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "SequenceMatcher.find_longest_match takes no keyword arguments",
        ));
    }

    if ahi < alo {
        ahi = alo;
    }
    if bhi < blo {
        bhi = blo;
    }

    let (i, j, size) = longest_common_substring(&a, &b, alo, ahi, blo, bhi);
    make_match_value(i, j, size, heap)
}

fn sequence_matcher_get_matching_blocks(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.get_matching_blocks")?;
    args.check_zero_args("SequenceMatcher.get_matching_blocks", heap)?;

    let (a, b) = get_sequence_pair(self_id, heap, interns)?;
    let blocks = matching_blocks(&a, &b);
    let mut out = Vec::with_capacity(blocks.len());
    for (i, j, size) in blocks {
        out.push(make_match_value(i, j, size, heap)?);
    }

    let id = heap.allocate(HeapData::List(List::new(out)))?;
    Ok(Value::Ref(id))
}

fn sequence_matcher_get_opcodes(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.get_opcodes")?;
    args.check_zero_args("SequenceMatcher.get_opcodes", heap)?;

    let (a, b) = get_sequence_pair(self_id, heap, interns)?;
    let opcodes = opcodes_from_sequences(&a, &b);
    opcodes_to_value(&opcodes, heap)
}

fn sequence_matcher_get_grouped_opcodes(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.get_grouped_opcodes")?;
    let (n_value, _) = args.get_zero_one_two_args("SequenceMatcher.get_grouped_opcodes", heap)?;
    let n = if let Some(value) = n_value {
        let n = clamp_non_negative(value.as_int(heap)?);
        value.drop_with_heap(heap);
        n
    } else {
        3
    };

    let (a, b) = get_sequence_pair(self_id, heap, interns)?;
    let opcodes = opcodes_from_sequences(&a, &b);
    let groups = grouped_opcodes(&opcodes, n);

    let mut outer = Vec::with_capacity(groups.len());
    for group in groups {
        let inner = opcodes_to_value(&group, heap)?;
        outer.push(inner);
    }
    let id = heap.allocate(HeapData::List(List::new(outer)))?;
    Ok(Value::Ref(id))
}

fn sequence_matcher_ratio(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.ratio")?;
    args.check_zero_args("SequenceMatcher.ratio", heap)?;
    let (a, b) = get_sequence_pair(self_id, heap, interns)?;
    Ok(Value::Float(similarity_ratio(&a, &b)))
}

fn sequence_matcher_quick_ratio(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.quick_ratio")?;
    args.check_zero_args("SequenceMatcher.quick_ratio", heap)?;
    let (a, b) = get_sequence_pair(self_id, heap, interns)?;
    Ok(Value::Float(similarity_ratio(&a, &b)))
}

fn sequence_matcher_real_quick_ratio(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (self_id, args) = extract_instance_self_and_args(args, heap, "SequenceMatcher.real_quick_ratio")?;
    args.check_zero_args("SequenceMatcher.real_quick_ratio", heap)?;
    let (a, b) = get_sequence_pair(self_id, heap, interns)?;

    let total = a.len() + b.len();
    let ratio = if total == 0 {
        1.0
    } else {
        (2.0 * (a.len().min(b.len()) as f64)) / (total as f64)
    };
    Ok(Value::Float(ratio))
}

/// Implements `difflib.Differ.__new__(cls, linejunk=None, charjunk=None)`.
fn differ_new(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.by_ref().collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("Differ", 1, 0));
    }
    let cls = positional.remove(0);
    let class_id = class_id_from_value(&cls, heap, "Differ")?;
    cls.drop_with_heap(heap);

    let mut linejunk = Value::None;
    let mut charjunk = Value::None;

    if !positional.is_empty() {
        linejunk = positional.remove(0);
    }
    if !positional.is_empty() {
        charjunk = positional.remove(0);
    }
    if let Some(extra) = positional.pop() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        linejunk.drop_with_heap(heap);
        charjunk.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("Differ", 2, 3));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            linejunk.drop_with_heap(heap);
            charjunk.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "linejunk" => {
                linejunk.drop_with_heap(heap);
                linejunk = value;
            }
            "charjunk" => {
                charjunk.drop_with_heap(heap);
                charjunk = value;
            }
            _ => {
                value.drop_with_heap(heap);
                linejunk.drop_with_heap(heap);
                charjunk.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("Differ", &key_name));
            }
        }
    }

    let instance_id = create_instance_for_class(class_id, heap)?;
    let mut guard = HeapGuard::new(Value::Ref(instance_id), heap);
    let (_, heap) = guard.as_parts_mut();

    set_instance_attr_by_name(instance_id, DIFFER_LINEJUNK_ATTR, linejunk, heap, interns)?;
    set_instance_attr_by_name(instance_id, DIFFER_CHARJUNK_ATTR, charjunk, heap, interns)?;
    attach_bound_method(instance_id, "compare", DifflibFunctions::DifferCompare, heap, interns)?;

    let (value, _) = guard.into_parts();
    Ok(value)
}

fn differ_compare(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (_self_id, args) = extract_instance_self_and_args(args, heap, "Differ.compare")?;
    let (a, b) = args.get_two_args("Differ.compare", heap)?;
    let a_lines = collect_iterable_strings(a, heap, interns)?;
    let b_lines = collect_iterable_strings(b, heap, interns)?;
    let lines = build_ndiff_lines(&a_lines, &b_lines);
    alloc_string_list(&lines, heap)
}

/// Implements `difflib.HtmlDiff.__new__(...)`.
fn html_diff_new(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.by_ref().collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("HtmlDiff", 1, 0));
    }
    let cls = positional.remove(0);
    let class_id = class_id_from_value(&cls, heap, "HtmlDiff")?;
    cls.drop_with_heap(heap);

    let mut tabsize = 8i64;
    let mut wrapcolumn: Option<i64> = None;
    let mut linejunk = Value::None;
    let mut charjunk = Value::ModuleFunction(ModuleFunctions::Difflib(DifflibFunctions::IsCharacterJunk));

    if !positional.is_empty() {
        let value = positional.remove(0);
        tabsize = value.as_int(heap)?;
        value.drop_with_heap(heap);
    }
    if !positional.is_empty() {
        let value = positional.remove(0);
        wrapcolumn = if matches!(value, Value::None) {
            value.drop_with_heap(heap);
            None
        } else {
            let n = value.as_int(heap)?;
            value.drop_with_heap(heap);
            Some(n)
        };
    }
    if !positional.is_empty() {
        linejunk = positional.remove(0);
    }
    if !positional.is_empty() {
        let value = positional.remove(0);
        charjunk.drop_with_heap(heap);
        charjunk = value;
    }
    if let Some(extra) = positional.pop() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        linejunk.drop_with_heap(heap);
        charjunk.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("HtmlDiff", 4, 5));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            linejunk.drop_with_heap(heap);
            charjunk.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "tabsize" => {
                tabsize = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "wrapcolumn" => {
                wrapcolumn = if matches!(value, Value::None) {
                    value.drop_with_heap(heap);
                    None
                } else {
                    let n = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    Some(n)
                };
            }
            "linejunk" => {
                linejunk.drop_with_heap(heap);
                linejunk = value;
            }
            "charjunk" => {
                charjunk.drop_with_heap(heap);
                charjunk = value;
            }
            _ => {
                value.drop_with_heap(heap);
                linejunk.drop_with_heap(heap);
                charjunk.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("HtmlDiff", &key_name));
            }
        }
    }

    let instance_id = create_instance_for_class(class_id, heap)?;
    let mut guard = HeapGuard::new(Value::Ref(instance_id), heap);
    let (_, heap) = guard.as_parts_mut();

    set_instance_attr_by_name(instance_id, HTML_TABSIZE_ATTR, Value::Int(tabsize), heap, interns)?;
    let wrap_value = if let Some(value) = wrapcolumn {
        Value::Int(value)
    } else {
        Value::None
    };
    set_instance_attr_by_name(instance_id, HTML_WRAPCOLUMN_ATTR, wrap_value, heap, interns)?;
    set_instance_attr_by_name(instance_id, HTML_LINEJUNK_ATTR, linejunk, heap, interns)?;
    set_instance_attr_by_name(instance_id, HTML_CHARJUNK_ATTR, charjunk, heap, interns)?;
    attach_bound_method(
        instance_id,
        "make_file",
        DifflibFunctions::HtmlDiffMakeFile,
        heap,
        interns,
    )?;
    attach_bound_method(
        instance_id,
        "make_table",
        DifflibFunctions::HtmlDiffMakeTable,
        heap,
        interns,
    )?;

    let (value, _) = guard.into_parts();
    Ok(value)
}

fn html_diff_make_table(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (_self_id, args) = extract_instance_self_and_args(args, heap, "HtmlDiff.make_table")?;

    let (mut positional, kwargs) = args.into_parts();
    let Some(fromlines_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("HtmlDiff.make_table", 2, 0));
    };
    let Some(tolines_value) = positional.next() else {
        fromlines_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("HtmlDiff.make_table", 2, 1));
    };

    let mut fromdesc = String::new();
    let mut todesc = String::new();
    let mut context = false;
    let mut numlines = 5usize;

    if let Some(value) = positional.next() {
        fromdesc = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        todesc = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        context = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        numlines = clamp_non_negative(value.as_int(heap)?);
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        fromlines_value.drop_with_heap(heap);
        tolines_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("HtmlDiff.make_table", 6, 7));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            fromlines_value.drop_with_heap(heap);
            tolines_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match key_name.as_str() {
            "fromdesc" => {
                fromdesc = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "todesc" => {
                todesc = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "context" => {
                context = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "numlines" => {
                numlines = clamp_non_negative(value.as_int(heap)?);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                fromlines_value.drop_with_heap(heap);
                tolines_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("HtmlDiff.make_table", &key_name));
            }
        }
    }

    let from_lines = collect_iterable_strings(fromlines_value, heap, interns)?;
    let to_lines = collect_iterable_strings(tolines_value, heap, interns)?;
    let table = build_html_table(&from_lines, &to_lines, &fromdesc, &todesc, context, numlines);

    let id = heap.allocate(HeapData::Str(Str::from(table)))?;
    Ok(Value::Ref(id))
}

fn html_diff_make_file(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (_self_id, args) = extract_instance_self_and_args(args, heap, "HtmlDiff.make_file")?;

    let (mut positional, kwargs) = args.into_parts();
    let Some(fromlines_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("HtmlDiff.make_file", 2, 0));
    };
    let Some(tolines_value) = positional.next() else {
        fromlines_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("HtmlDiff.make_file", 2, 1));
    };

    let mut fromdesc = String::new();
    let mut todesc = String::new();
    let mut context = false;
    let mut numlines = 5usize;
    let mut charset = "utf-8".to_owned();

    if let Some(value) = positional.next() {
        fromdesc = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        todesc = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        context = value.py_bool(heap, interns);
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        numlines = clamp_non_negative(value.as_int(heap)?);
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        charset = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        fromlines_value.drop_with_heap(heap);
        tolines_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("HtmlDiff.make_file", 7, 8));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            fromlines_value.drop_with_heap(heap);
            tolines_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "fromdesc" => {
                fromdesc = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "todesc" => {
                todesc = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "context" => {
                context = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "numlines" => {
                numlines = clamp_non_negative(value.as_int(heap)?);
                value.drop_with_heap(heap);
            }
            "charset" => {
                charset = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                fromlines_value.drop_with_heap(heap);
                tolines_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("HtmlDiff.make_file", &key_name));
            }
        }
    }

    let from_lines = collect_iterable_strings(fromlines_value, heap, interns)?;
    let to_lines = collect_iterable_strings(tolines_value, heap, interns)?;

    let table = build_html_table(&from_lines, &to_lines, &fromdesc, &todesc, context, numlines);
    let file = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"{}\">\n<title></title>\n</head>\n<body>\n{}\n</body>\n</html>",
        html_escape(&charset),
        table
    );

    let id = heap.allocate(HeapData::Str(Str::from(file)))?;
    Ok(Value::Ref(id))
}

/// Implements `difflib.get_close_matches(word, possibilities, n=3, cutoff=0.6)`.
fn get_close_matches(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(word_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("get_close_matches", 2, 0));
    };
    let Some(possibilities_value) = positional.next() else {
        word_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("get_close_matches", 2, 1));
    };

    let mut n = 3usize;
    let mut cutoff = 0.6f64;

    if let Some(value) = positional.next() {
        n = clamp_non_negative(value.as_int(heap)?);
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        cutoff = value_as_f64(&value, heap).unwrap_or(0.0);
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        word_value.drop_with_heap(heap);
        possibilities_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("get_close_matches", 4, 5));
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            word_value.drop_with_heap(heap);
            possibilities_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "n" => {
                n = clamp_non_negative(value.as_int(heap)?);
                value.drop_with_heap(heap);
            }
            "cutoff" => {
                cutoff = value_as_f64(&value, heap).unwrap_or(0.0);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                word_value.drop_with_heap(heap);
                possibilities_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("get_close_matches", &name));
            }
        }
    }

    if n == 0 {
        word_value.drop_with_heap(heap);
        possibilities_value.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be > 0").into());
    }
    if !(0.0..=1.0).contains(&cutoff) {
        word_value.drop_with_heap(heap);
        possibilities_value.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "cutoff must be in [0.0, 1.0]").into());
    }

    let word = word_value.py_str(heap, interns).into_owned();
    word_value.drop_with_heap(heap);

    let mut iter = OurosIter::new(possibilities_value, heap, interns)?;
    let mut scored: Vec<(f64, String)> = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let candidate = item.py_str(heap, interns).into_owned();
                item.drop_with_heap(heap);
                let ratio = similarity_ratio_chars(&word, &candidate);
                if ratio >= cutoff {
                    scored.push((ratio, candidate));
                }
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);

    scored.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(Ordering::Equal));
    let best = scored.into_iter().take(n).map(|(_, s)| s).collect::<Vec<_>>();
    alloc_string_list(&best, heap)
}

/// Implements `difflib.ndiff(a, b, linejunk=None, charjunk=IS_CHARACTER_JUNK)`.
fn ndiff(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(a_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("ndiff", 2, 0));
    };
    let Some(b_value) = positional.next() else {
        a_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("ndiff", 2, 1));
    };

    if let Some(value) = positional.next() {
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        a_value.drop_with_heap(heap);
        b_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("ndiff", 4, 5));
    }
    for (key, value) in kwargs {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }

    let a_lines = collect_iterable_strings(a_value, heap, interns)?;
    let b_lines = collect_iterable_strings(b_value, heap, interns)?;
    let lines = build_ndiff_lines(&a_lines, &b_lines);
    alloc_string_list(&lines, heap)
}

/// Implements `difflib.restore(delta, which)`.
fn restore(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (delta_value, which_value) = args.get_two_args("restore", heap)?;
    let which = which_value.as_int(heap)?;
    which_value.drop_with_heap(heap);
    if which != 1 && which != 2 {
        delta_value.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "which must be 1 or 2").into());
    }

    let lines = collect_iterable_strings(delta_value, heap, interns)?;
    let mut restored = Vec::new();
    for line in lines {
        if line.len() < 2 {
            continue;
        }
        let prefix = &line[..2];
        if prefix == "  " || (which == 1 && prefix == "- ") || (which == 2 && prefix == "+ ") {
            restored.push(line[2..].to_owned());
        }
    }

    alloc_string_list(&restored, heap)
}

/// Implements `difflib.unified_diff(...)`.
fn unified_diff(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let params = parse_line_diff_params("unified_diff", heap, interns, args)?;
    let lines = build_unified_diff_lines(
        &params.a,
        &params.b,
        &params.fromfile,
        &params.tofile,
        &params.fromfiledate,
        &params.tofiledate,
        params.n,
        &params.lineterm,
    );
    alloc_string_list(&lines, heap)
}

/// Implements `difflib.context_diff(...)`.
fn context_diff(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let params = parse_line_diff_params("context_diff", heap, interns, args)?;
    let lines = build_context_diff_lines(
        &params.a,
        &params.b,
        &params.fromfile,
        &params.tofile,
        &params.fromfiledate,
        &params.tofiledate,
        params.n,
        &params.lineterm,
    );
    alloc_string_list(&lines, heap)
}

/// Implements `difflib.diff_bytes(...)`.
fn diff_bytes(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(dfunc) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("diff_bytes", 3, 0));
    };
    let Some(a_value) = positional.next() else {
        dfunc.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("diff_bytes", 3, 1));
    };
    let Some(b_value) = positional.next() else {
        dfunc.drop_with_heap(heap);
        a_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("diff_bytes", 3, 2));
    };

    let mut fromfile = Vec::new();
    let mut tofile = Vec::new();
    let mut fromfiledate = Vec::new();
    let mut tofiledate = Vec::new();
    let mut n = 3usize;
    let mut lineterm = vec![b'\n'];

    if let Some(value) = positional.next() {
        fromfile = value_to_bytes(value, heap, interns)?;
    }
    if let Some(value) = positional.next() {
        tofile = value_to_bytes(value, heap, interns)?;
    }
    if let Some(value) = positional.next() {
        fromfiledate = value_to_bytes(value, heap, interns)?;
    }
    if let Some(value) = positional.next() {
        tofiledate = value_to_bytes(value, heap, interns)?;
    }
    if let Some(value) = positional.next() {
        n = clamp_non_negative(value.as_int(heap)?);
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        lineterm = value_to_bytes(value, heap, interns)?;
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        dfunc.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("diff_bytes", 8, 9));
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            dfunc.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match name.as_str() {
            "fromfile" => fromfile = value_to_bytes(value, heap, interns)?,
            "tofile" => tofile = value_to_bytes(value, heap, interns)?,
            "fromfiledate" => fromfiledate = value_to_bytes(value, heap, interns)?,
            "tofiledate" => tofiledate = value_to_bytes(value, heap, interns)?,
            "n" => {
                n = clamp_non_negative(value.as_int(heap)?);
                value.drop_with_heap(heap);
            }
            "lineterm" => lineterm = value_to_bytes(value, heap, interns)?,
            _ => {
                value.drop_with_heap(heap);
                dfunc.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("diff_bytes", &name));
            }
        }
    }

    let a_lines = collect_iterable_bytes_as_strings(a_value, heap, interns)?;
    let b_lines = collect_iterable_bytes_as_strings(b_value, heap, interns)?;

    let fromfile = latin1_decode(&fromfile);
    let tofile = latin1_decode(&tofile);
    let fromfiledate = latin1_decode(&fromfiledate);
    let tofiledate = latin1_decode(&tofiledate);
    let lineterm = latin1_decode(&lineterm);

    let rendered = match dfunc {
        Value::ModuleFunction(ModuleFunctions::Difflib(DifflibFunctions::UnifiedDiff)) => build_unified_diff_lines(
            &a_lines,
            &b_lines,
            &fromfile,
            &tofile,
            &fromfiledate,
            &tofiledate,
            n,
            &lineterm,
        ),
        Value::ModuleFunction(ModuleFunctions::Difflib(DifflibFunctions::ContextDiff)) => build_context_diff_lines(
            &a_lines,
            &b_lines,
            &fromfile,
            &tofile,
            &fromfiledate,
            &tofiledate,
            n,
            &lineterm,
        ),
        Value::ModuleFunction(ModuleFunctions::Difflib(DifflibFunctions::Ndiff)) => {
            build_ndiff_lines(&a_lines, &b_lines)
        }
        _ => {
            dfunc.drop_with_heap(heap);
            return Err(ExcType::type_error(
                "diff_bytes dfunc must be difflib.unified_diff/context_diff/ndiff",
            ));
        }
    };
    dfunc.drop_with_heap(heap);

    let mut out = Vec::with_capacity(rendered.len());
    for line in rendered {
        let id = heap.allocate(HeapData::Bytes(crate::types::Bytes::new(latin1_encode(&line))))?;
        out.push(Value::Ref(id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(out)))?;
    Ok(Value::Ref(list_id))
}

/// Shared parsed parameters for `unified_diff`/`context_diff`.
struct LineDiffParams {
    a: Vec<String>,
    b: Vec<String>,
    fromfile: String,
    tofile: String,
    fromfiledate: String,
    tofiledate: String,
    n: usize,
    lineterm: String,
}

fn parse_line_diff_params(
    fn_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<LineDiffParams> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(a_value) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(fn_name, 2, 0));
    };
    let Some(b_value) = positional.next() else {
        a_value.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(fn_name, 2, 1));
    };

    let mut fromfile = String::new();
    let mut tofile = String::new();
    let mut fromfiledate = String::new();
    let mut tofiledate = String::new();
    let mut n = 3usize;
    let mut lineterm = "\n".to_owned();

    if let Some(value) = positional.next() {
        fromfile = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        tofile = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        fromfiledate = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        tofiledate = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        n = clamp_non_negative(value.as_int(heap)?);
        value.drop_with_heap(heap);
    }
    if let Some(value) = positional.next() {
        lineterm = value.py_str(heap, interns).into_owned();
        value.drop_with_heap(heap);
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        a_value.drop_with_heap(heap);
        b_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(fn_name, 8, 9));
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            a_value.drop_with_heap(heap);
            b_value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match name.as_str() {
            "fromfile" => {
                fromfile = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "tofile" => {
                tofile = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "fromfiledate" => {
                fromfiledate = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "tofiledate" => {
                tofiledate = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "n" => {
                n = clamp_non_negative(value.as_int(heap)?);
                value.drop_with_heap(heap);
            }
            "lineterm" => {
                lineterm = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                a_value.drop_with_heap(heap);
                b_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(fn_name, &name));
            }
        }
    }

    let a = collect_iterable_strings(a_value, heap, interns)?;
    let b = collect_iterable_strings(b_value, heap, interns)?;

    Ok(LineDiffParams {
        a,
        b,
        fromfile,
        tofile,
        fromfiledate,
        tofiledate,
        n,
        lineterm,
    })
}

/// Builds lines for `ndiff`.
fn build_ndiff_lines(a: &[String], b: &[String]) -> Vec<String> {
    let opcodes = opcodes_from_sequences(a, b);
    let mut out = Vec::new();

    for op in opcodes {
        match op.tag {
            "equal" => {
                for line in &a[op.i1..op.i2] {
                    out.push(format!("  {line}"));
                }
            }
            "delete" => {
                for line in &a[op.i1..op.i2] {
                    out.push(format!("- {line}"));
                }
            }
            "insert" => {
                for line in &b[op.j1..op.j2] {
                    out.push(format!("+ {line}"));
                }
            }
            "replace" => {
                for line in &a[op.i1..op.i2] {
                    out.push(format!("- {line}"));
                }
                for line in &b[op.j1..op.j2] {
                    out.push(format!("+ {line}"));
                }
            }
            _ => {}
        }
    }

    out
}

/// Builds lines for `unified_diff`.
fn build_unified_diff_lines(
    a: &[String],
    b: &[String],
    fromfile: &str,
    tofile: &str,
    fromfiledate: &str,
    tofiledate: &str,
    n: usize,
    lineterm: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    out.push(unified_file_header("---", fromfile, fromfiledate, lineterm));
    out.push(unified_file_header("+++", tofile, tofiledate, lineterm));

    let groups = grouped_opcodes(&opcodes_from_sequences(a, b), n);
    for group in groups {
        let first = group.first().expect("groups never empty");
        let last = group.last().expect("groups never empty");
        let old_range = format_range_unified(first.i1, last.i2);
        let new_range = format_range_unified(first.j1, last.j2);
        out.push(format!("@@ -{old_range} +{new_range} @@{lineterm}"));

        for op in group {
            match op.tag {
                "equal" => {
                    for line in &a[op.i1..op.i2] {
                        out.push(format!(" {line}"));
                    }
                }
                "delete" => {
                    for line in &a[op.i1..op.i2] {
                        out.push(format!("-{line}"));
                    }
                }
                "insert" => {
                    for line in &b[op.j1..op.j2] {
                        out.push(format!("+{line}"));
                    }
                }
                "replace" => {
                    for line in &a[op.i1..op.i2] {
                        out.push(format!("-{line}"));
                    }
                    for line in &b[op.j1..op.j2] {
                        out.push(format!("+{line}"));
                    }
                }
                _ => {}
            }
        }
    }

    out
}

/// Builds lines for `context_diff`.
fn build_context_diff_lines(
    a: &[String],
    b: &[String],
    fromfile: &str,
    tofile: &str,
    fromfiledate: &str,
    tofiledate: &str,
    n: usize,
    lineterm: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    out.push(unified_file_header("***", fromfile, fromfiledate, lineterm));
    out.push(unified_file_header("---", tofile, tofiledate, lineterm));

    let groups = grouped_opcodes(&opcodes_from_sequences(a, b), n);
    for group in groups {
        let first = group.first().expect("groups never empty");
        let last = group.last().expect("groups never empty");
        out.push(format!("***************{lineterm}"));
        out.push(format!(
            "*** {} ****{}",
            format_range_context(first.i1, last.i2),
            lineterm
        ));

        for op in &group {
            match op.tag {
                "equal" => {
                    for line in &a[op.i1..op.i2] {
                        out.push(format!("  {line}"));
                    }
                }
                "delete" => {
                    for line in &a[op.i1..op.i2] {
                        out.push(format!("- {line}"));
                    }
                }
                "replace" => {
                    for line in &a[op.i1..op.i2] {
                        out.push(format!("! {line}"));
                    }
                }
                "insert" => {}
                _ => {}
            }
        }

        out.push(format!(
            "--- {} ----{}",
            format_range_context(first.j1, last.j2),
            lineterm
        ));
        for op in group {
            match op.tag {
                "equal" => {
                    for line in &b[op.j1..op.j2] {
                        out.push(format!("  {line}"));
                    }
                }
                "insert" => {
                    for line in &b[op.j1..op.j2] {
                        out.push(format!("+ {line}"));
                    }
                }
                "replace" => {
                    for line in &b[op.j1..op.j2] {
                        out.push(format!("! {line}"));
                    }
                }
                "delete" => {}
                _ => {}
            }
        }
    }

    out
}

fn build_html_table(
    from_lines: &[String],
    to_lines: &[String],
    from_desc: &str,
    to_desc: &str,
    _context: bool,
    _numlines: usize,
) -> String {
    let mut out = String::new();
    out.push_str("<table class=\"diff\" cellspacing=\"0\" cellpadding=\"0\" rules=\"groups\">\n");
    out.push_str("<thead><tr>");
    out.push_str("<th class=\"diff_header\">From</th>");
    write!(out, "<th class=\"diff_header\">{}</th>", html_escape(from_desc))
        .expect("writing to String should never fail");
    out.push_str("<th class=\"diff_header\">To</th>");
    write!(out, "<th class=\"diff_header\">{}</th>", html_escape(to_desc))
        .expect("writing to String should never fail");
    out.push_str("</tr></thead>\n<tbody>\n");

    let limit = from_lines.len().max(to_lines.len());
    for index in 0..limit {
        let left = from_lines.get(index).map_or("", String::as_str);
        let right = to_lines.get(index).map_or("", String::as_str);
        let class = if left == right { "diff_equal" } else { "diff_chg" };
        writeln!(
            out,
            "<tr class=\"{class}\"><td class=\"diff_header\">{}</td><td>{}</td><td class=\"diff_header\">{}</td><td>{}</td></tr>",
            index + 1,
            html_escape(left),
            index + 1,
            html_escape(right)
        )
        .expect("writing to String should never fail");
    }

    out.push_str("</tbody>\n</table>");
    out
}

#[derive(Clone)]
struct Opcode {
    tag: &'static str,
    i1: usize,
    i2: usize,
    j1: usize,
    j2: usize,
}

fn opcodes_from_sequences(a: &[String], b: &[String]) -> Vec<Opcode> {
    let blocks = matching_blocks(a, b);
    let mut opcodes = Vec::new();

    let mut i = 0usize;
    let mut j = 0usize;

    for (ai, bj, size) in blocks {
        let tag = if i < ai && j < bj {
            Some("replace")
        } else if i < ai {
            Some("delete")
        } else if j < bj {
            Some("insert")
        } else {
            None
        };

        if let Some(tag) = tag {
            opcodes.push(Opcode {
                tag,
                i1: i,
                i2: ai,
                j1: j,
                j2: bj,
            });
        }
        if size > 0 {
            opcodes.push(Opcode {
                tag: "equal",
                i1: ai,
                i2: ai + size,
                j1: bj,
                j2: bj + size,
            });
        }
        i = ai + size;
        j = bj + size;
    }

    opcodes
}

fn grouped_opcodes(opcodes: &[Opcode], n: usize) -> Vec<Vec<Opcode>> {
    if opcodes.is_empty() {
        return Vec::new();
    }

    let mut codes = opcodes.to_vec();
    if let Some(first) = codes.first_mut()
        && first.tag == "equal"
    {
        first.i1 = first.i2.saturating_sub(n);
        first.j1 = first.j2.saturating_sub(n);
    }
    if let Some(last) = codes.last_mut()
        && last.tag == "equal"
    {
        last.i2 = (last.i1 + n).min(last.i2);
        last.j2 = (last.j1 + n).min(last.j2);
    }

    let nn = n.saturating_mul(2);
    let mut groups: Vec<Vec<Opcode>> = Vec::new();
    let mut group: Vec<Opcode> = Vec::new();

    for code in codes {
        if code.tag == "equal" && code.i2.saturating_sub(code.i1) > nn {
            let first_half = Opcode {
                tag: "equal",
                i1: code.i1,
                i2: code.i1 + n,
                j1: code.j1,
                j2: code.j1 + n,
            };
            group.push(first_half);
            if !group.is_empty() {
                groups.push(group);
                group = Vec::new();
            }
            let second_half = Opcode {
                tag: "equal",
                i1: code.i2.saturating_sub(n),
                i2: code.i2,
                j1: code.j2.saturating_sub(n),
                j2: code.j2,
            };
            group.push(second_half);
        } else {
            group.push(code);
        }
    }

    if !(group.len() == 1 && group[0].tag == "equal") {
        groups.push(group);
    }

    groups
}

fn similarity_ratio(a: &[String], b: &[String]) -> f64 {
    let matches = matching_blocks(a, b)
        .into_iter()
        .map(|(_, _, size)| size)
        .sum::<usize>();
    let total = a.len() + b.len();
    if total == 0 {
        1.0
    } else {
        (2.0 * matches as f64) / (total as f64)
    }
}

fn similarity_ratio_chars(a: &str, b: &str) -> f64 {
    let a_chars = a.chars().map(|c| c.to_string()).collect::<Vec<_>>();
    let b_chars = b.chars().map(|c| c.to_string()).collect::<Vec<_>>();
    similarity_ratio(&a_chars, &b_chars)
}

fn matching_blocks(left_seq: &[String], right_seq: &[String]) -> Vec<(usize, usize, usize)> {
    let left_len = left_seq.len();
    let right_len = right_seq.len();
    let mut dp = vec![vec![0usize; right_len + 1]; left_len + 1];

    for left_index in (0..left_len).rev() {
        for right_index in (0..right_len).rev() {
            dp[left_index][right_index] = if left_seq[left_index] == right_seq[right_index] {
                dp[left_index + 1][right_index + 1] + 1
            } else {
                dp[left_index + 1][right_index].max(dp[left_index][right_index + 1])
            };
        }
    }

    let mut left_cursor = 0usize;
    let mut right_cursor = 0usize;
    let mut blocks = Vec::new();

    while left_cursor < left_len && right_cursor < right_len {
        if left_seq[left_cursor] == right_seq[right_cursor] {
            let left_start = left_cursor;
            let right_start = right_cursor;
            while left_cursor < left_len && right_cursor < right_len && left_seq[left_cursor] == right_seq[right_cursor]
            {
                left_cursor += 1;
                right_cursor += 1;
            }
            blocks.push((left_start, right_start, left_cursor - left_start));
        } else if dp[left_cursor + 1][right_cursor] >= dp[left_cursor][right_cursor + 1] {
            left_cursor += 1;
        } else {
            right_cursor += 1;
        }
    }

    blocks.push((left_len, right_len, 0));
    blocks
}

fn longest_common_substring(
    a: &[String],
    b: &[String],
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> (usize, usize, usize) {
    let mut best_i = alo;
    let mut best_j = blo;
    let mut best_size = 0usize;

    for i in alo..ahi {
        for j in blo..bhi {
            let mut k = 0usize;
            while i + k < ahi && j + k < bhi && a[i + k] == b[j + k] {
                k += 1;
            }
            if k > best_size {
                best_size = k;
                best_i = i;
                best_j = j;
            }
        }
    }

    (best_i, best_j, best_size)
}

fn opcodes_to_value(opcodes: &[Opcode], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let mut out = Vec::with_capacity(opcodes.len());
    for op in opcodes {
        let tag_id = heap.allocate(HeapData::Str(Str::from(op.tag)))?;
        let tuple = allocate_tuple(
            smallvec![
                Value::Ref(tag_id),
                Value::Int(usize_to_i64(op.i1)?),
                Value::Int(usize_to_i64(op.i2)?),
                Value::Int(usize_to_i64(op.j1)?),
                Value::Int(usize_to_i64(op.j2)?),
            ],
            heap,
        )?;
        out.push(tuple);
    }

    let id = heap.allocate(HeapData::List(List::new(out)))?;
    Ok(Value::Ref(id))
}

fn make_match_value(i: usize, j: usize, size: usize, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let tuple = NamedTuple::new(
        EitherStr::Heap("Match".to_owned()),
        vec![
            EitherStr::Heap("a".to_owned()),
            EitherStr::Heap("b".to_owned()),
            EitherStr::Heap("size".to_owned()),
        ],
        vec![
            Value::Int(usize_to_i64(i)?),
            Value::Int(usize_to_i64(j)?),
            Value::Int(usize_to_i64(size)?),
        ],
    );
    let id = heap.allocate(HeapData::NamedTuple(tuple))?;
    Ok(Value::Ref(id))
}

fn create_match_factory(heap: &mut Heap<impl ResourceTracker>) -> Result<HeapId, ResourceError> {
    let factory = NamedTupleFactory::new_with_options(
        EitherStr::Heap("Match".to_owned()),
        vec![
            EitherStr::Heap("a".to_owned()),
            EitherStr::Heap("b".to_owned()),
            EitherStr::Heap("size".to_owned()),
        ],
        Vec::new(),
        EitherStr::Heap("difflib".to_owned()),
    );
    heap.allocate(HeapData::NamedTupleFactory(factory))
}

fn create_sequence_matcher_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    create_simple_runtime_class(
        heap,
        interns,
        "SequenceMatcher",
        DifflibFunctions::SequenceMatcherNew,
        DifflibFunctions::SequenceMatcherInit,
    )
}

fn create_differ_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    create_simple_runtime_class(
        heap,
        interns,
        "Differ",
        DifflibFunctions::DifferNew,
        DifflibFunctions::DifferInit,
    )
}

fn create_html_diff_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    create_simple_runtime_class(
        heap,
        interns,
        "HtmlDiff",
        DifflibFunctions::HtmlDiffNew,
        DifflibFunctions::HtmlDiffInit,
    )
}

fn create_simple_runtime_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    name: &str,
    new_function: DifflibFunctions,
    init_function: DifflibFunctions,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();

    dict_set_str_key(
        &mut namespace,
        "__new__",
        Value::ModuleFunction(ModuleFunctions::Difflib(new_function)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Difflib(init_function)),
        heap,
        interns,
    )?;

    let module_id = heap.allocate(HeapData::Str(Str::from("difflib")))?;
    dict_set_str_key(&mut namespace, "__module__", Value::Ref(module_id), heap, interns)?;

    let object_id = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_id);

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap(name.to_owned()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        namespace,
        vec![object_id],
        vec![],
    );

    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;
    let mro = compute_c3_mro(class_id, &[object_id], heap, interns)
        .expect("difflib helper class should always have a valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    heap.with_entry_mut(object_id, |_, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("builtin object class registry should be mutable");

    Ok(class_id)
}

fn create_instance_for_class(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let attrs_id = Some(heap.allocate(HeapData::Dict(Dict::new()))?);
    heap.inc_ref(class_id);
    Ok(heap.allocate(HeapData::Instance(Instance::new(
        class_id,
        attrs_id,
        Vec::new(),
        Vec::new(),
    )))?)
}

fn class_id_from_value(value: &Value, heap: &Heap<impl ResourceTracker>, name: &str) -> RunResult<HeapId> {
    match value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => Ok(*id),
        _ => Err(ExcType::type_error(format!(
            "{name}.__new__(cls, ...): cls must be a class"
        ))),
    }
}

fn set_sequence_attr(
    instance_id: HeapId,
    name: &str,
    values: &[String],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        let id = heap.allocate(HeapData::Str(Str::from(value.as_str())))?;
        items.push(Value::Ref(id));
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    set_instance_attr_by_name(instance_id, name, Value::Ref(list_id), heap, interns)
}

fn get_sequence_pair(
    instance_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Vec<String>, Vec<String>)> {
    let a = get_sequence_attr(instance_id, SEQ_A_ATTR, heap, interns)?;
    let b = get_sequence_attr(instance_id, SEQ_B_ATTR, heap, interns)?;
    Ok((a, b))
}

fn get_sequence_attr(
    instance_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<String>> {
    let Some(value) = get_instance_attr_by_name(instance_id, name, heap, interns) else {
        return Ok(Vec::new());
    };

    let out = match value {
        Value::Ref(id) => match heap.get(id) {
            HeapData::List(list) => {
                let mut values = Vec::with_capacity(list.len());
                for item in list.as_vec() {
                    values.push(item.py_str(heap, interns).into_owned());
                }
                values
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    value.drop_with_heap(heap);
    Ok(out)
}

fn attach_bound_method(
    instance_id: HeapId,
    name: &str,
    function: DifflibFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    heap.inc_ref(instance_id);
    let bound_self = Value::Ref(instance_id);
    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Difflib(function)),
        vec![bound_self],
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    set_instance_attr_by_name(instance_id, name, Value::Ref(partial_id), heap, interns)
}

fn extract_instance_self_and_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    method_name: &str,
) -> RunResult<(HeapId, ArgValues)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 1, 0));
    }

    let self_value = positional.remove(0);
    let self_id = match &self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            self_value.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("{method_name} expected instance")));
        }
    };
    self_value.drop_with_heap(heap);

    Ok((self_id, arg_values_from_parts(positional, kwargs)))
}

fn arg_values_from_parts(positional: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    if kwargs.is_empty() {
        match positional.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(positional.into_iter().next().expect("length checked")),
            2 => {
                let mut iter = positional.into_iter();
                ArgValues::Two(
                    iter.next().expect("length checked"),
                    iter.next().expect("length checked"),
                )
            }
            _ => ArgValues::ArgsKargs {
                args: positional,
                kwargs,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        }
    }
}

fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    heap.with_entry_mut(instance_id, |heap_inner, data| -> RunResult<()> {
        let HeapData::Instance(instance) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("difflib helper expected instance"));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

fn get_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };

    instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str(name, heap, interns))
        .map(|value| value.clone_with_heap(heap))
}

fn collect_iterable_strings(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<String>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut out = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                out.push(item.py_str(heap, interns).into_owned());
                item.drop_with_heap(heap);
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(out)
}

fn collect_iterable_bytes_as_strings(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<String>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut out = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let bytes = value_to_bytes(item, heap, interns)?;
                out.push(latin1_decode(&bytes));
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(out)
}

fn value_to_bytes(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    let out = match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(id).to_vec()),
        Value::Ref(id) => match heap.get(id) {
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            _ => Err(ExcType::type_error("a bytes-like object is required")),
        },
        _ => Err(ExcType::type_error("a bytes-like object is required")),
    };
    value.drop_with_heap(heap);
    out
}

fn alloc_string_list(lines: &[String], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let mut values = Vec::with_capacity(lines.len());
    for line in lines {
        let id = heap.allocate(HeapData::Str(Str::from(line.as_str())))?;
        values.push(Value::Ref(id));
    }
    let id = heap.allocate(HeapData::List(List::new(values)))?;
    Ok(Value::Ref(id))
}

fn register_callable(
    module: &mut Module,
    name: &str,
    function: DifflibFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Difflib(function)),
        heap,
        interns,
    )
}

fn parse_text_value(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let out = match value {
        Value::InternString(id) => Ok(interns.get_str(id).to_owned()),
        Value::Ref(id) => match heap.get(id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error("expected str")),
        },
        _ => Err(ExcType::type_error("expected str")),
    };
    value.drop_with_heap(heap);
    out
}

fn dict_set_str_key(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

fn clamp_index(index: i64, len: usize) -> usize {
    if index <= 0 {
        0
    } else {
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        index.min(len)
    }
}

fn clamp_non_negative(value: i64) -> usize {
    if value <= 0 {
        0
    } else {
        usize::try_from(value).unwrap_or(usize::MAX)
    }
}

fn usize_to_i64(value: usize) -> RunResult<i64> {
    i64::try_from(value).map_err(|_| ExcType::overflow_shift_count())
}

fn value_as_f64(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => long_int.to_f64(),
            _ => None,
        },
        _ => None,
    }
}

fn drop_remaining_kwargs(kwargs_iter: impl Iterator<Item = (Value, Value)>, heap: &mut Heap<impl ResourceTracker>) {
    for (key, value) in kwargs_iter {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
}

fn format_range_unified(start: usize, stop: usize) -> String {
    let mut beginning = start + 1;
    let length = stop.saturating_sub(start);
    if length == 0 {
        beginning = beginning.saturating_sub(1);
    }
    if length <= 1 {
        beginning.to_string()
    } else {
        format!("{beginning},{length}")
    }
}

fn format_range_context(start: usize, stop: usize) -> String {
    let mut beginning = start + 1;
    let length = stop.saturating_sub(start);
    if length == 0 {
        beginning = beginning.saturating_sub(1);
    }
    if length <= 1 {
        beginning.to_string()
    } else {
        format!("{beginning},{}", beginning + length - 1)
    }
}

fn unified_file_header(prefix: &str, file: &str, date: &str, lineterm: &str) -> String {
    if date.is_empty() {
        format!("{prefix} {file}{lineterm}")
    } else {
        format!("{prefix} {file}\t{date}{lineterm}")
    }
}

fn latin1_decode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

fn latin1_encode(text: &str) -> Vec<u8> {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            if code <= 0xFF { code as u8 } else { b'?' }
        })
        .collect()
}

fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
