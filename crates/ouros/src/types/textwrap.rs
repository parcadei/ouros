//! Heap type for `textwrap.TextWrapper` instances.
//!
//! Ouros models `TextWrapper` as a native heap object with configurable wrap
//! behavior and methods (`wrap`, `fill`) exposed via attribute calls.

use std::fmt::Write;

use ahash::AHashSet;

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, List, PyTrait, Str, Type},
    value::{EitherStr, Value},
};

/// Runtime object for `textwrap.TextWrapper`.
///
/// Stores configuration options for text wrapping and provides callable methods
/// for `wrap()` and `fill()`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct TextWrapper {
    /// Target line width in code points.
    pub(crate) width: usize,
    /// Prefix for the first line.
    pub(crate) initial_indent: String,
    /// Prefix for subsequent lines.
    pub(crate) subsequent_indent: String,
    /// Placeholder appended when `max_lines` truncates output.
    pub(crate) placeholder: String,
    /// Optional cap on emitted line count.
    pub(crate) max_lines: Option<usize>,
    /// Whether over-wide words may be split to fit.
    pub(crate) break_long_words: bool,
    /// Whether to expand tabs to spaces.
    pub(crate) expand_tabs: bool,
    /// Whether to replace whitespace characters with spaces.
    pub(crate) replace_whitespace: bool,
    /// Whether to add extra space after sentence endings.
    pub(crate) fix_sentence_endings: bool,
    /// Whether to drop leading/trailing whitespace from lines.
    pub(crate) drop_whitespace: bool,
    /// Whether to break at hyphens.
    pub(crate) break_on_hyphens: bool,
    /// Tab expansion size.
    pub(crate) tabsize: usize,
}

impl TextWrapper {
    /// Creates a new wrapper object with default settings.
    #[must_use]
    pub fn new(width: usize) -> Self {
        Self {
            width,
            initial_indent: String::new(),
            subsequent_indent: String::new(),
            placeholder: " [...]".to_owned(),
            max_lines: None,
            break_long_words: true,
            expand_tabs: true,
            replace_whitespace: true,
            fix_sentence_endings: false,
            drop_whitespace: true,
            break_on_hyphens: true,
            tabsize: 8,
        }
    }

    /// Wraps text into lines according to the configured settings.
    pub(crate) fn wrap_text(&self, text: &str) -> Vec<String> {
        wrap_lines(text, self)
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for TextWrapper {
    fn drop_with_heap(self, _heap: &mut Heap<T>) {}
}

impl PyTrait for TextWrapper {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Object
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {}

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
        f.write_str("<textwrap.TextWrapper object>")
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.initial_indent.len() + self.subsequent_indent.len() + self.placeholder.len()
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr.as_str(interns) {
            "wrap" => {
                let text_value = args.get_one_arg("TextWrapper.wrap", heap)?;
                validate_wrapper_config(self)?;
                let text = text_value.py_str(heap, interns).into_owned();
                text_value.drop_with_heap(heap);
                let lines = self.wrap_text(&text);
                let list = allocate_string_list(lines, heap)?;
                Ok(list)
            }
            "fill" => {
                let text_value = args.get_one_arg("TextWrapper.fill", heap)?;
                validate_wrapper_config(self)?;
                let text = text_value.py_str(heap, interns).into_owned();
                text_value.drop_with_heap(heap);
                let lines = self.wrap_text(&text);
                let joined = lines.join("\n");
                let id = heap.allocate(HeapData::Str(Str::from(joined)))?;
                Ok(Value::Ref(id))
            }
            _ => {
                args.drop_with_heap(heap);
                Err(ExcType::attribute_error(self.py_type(heap), attr.as_str(interns)))
            }
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr = StaticStrings::from_string_id(attr_id);
        let value = match attr {
            Some(StaticStrings::TwWidth) => Value::Int(i64::try_from(self.width).unwrap_or(i64::MAX)),
            Some(StaticStrings::TwInitialIndent) => allocate_string(&self.initial_indent, heap)?,
            Some(StaticStrings::TwSubsequentIndent) => allocate_string(&self.subsequent_indent, heap)?,
            Some(StaticStrings::TwPlaceholder) => allocate_string(&self.placeholder, heap)?,
            Some(StaticStrings::TwMaxLines) => {
                if let Some(max_lines) = self.max_lines {
                    Value::Int(i64::try_from(max_lines).unwrap_or(i64::MAX))
                } else {
                    Value::None
                }
            }
            Some(StaticStrings::TwBreakLongWords) => Value::Bool(self.break_long_words),
            Some(StaticStrings::TwExpandTabs) => Value::Bool(self.expand_tabs),
            Some(StaticStrings::TwReplaceWhitespace) => Value::Bool(self.replace_whitespace),
            Some(StaticStrings::TwFixSentenceEndings) => Value::Bool(self.fix_sentence_endings),
            Some(StaticStrings::TwDropWhitespace) => Value::Bool(self.drop_whitespace),
            Some(StaticStrings::TwBreakOnHyphens) => Value::Bool(self.break_on_hyphens),
            Some(StaticStrings::TwTabsize) => Value::Int(i64::try_from(self.tabsize).unwrap_or(i64::MAX)),
            _ => return Ok(None),
        };
        Ok(Some(AttrCallResult::Value(value)))
    }
}

/// Expands tabs in text according to tabsize.
fn expand_tabs_text(text: &str, tabsize: usize) -> String {
    let mut result = String::with_capacity(text.len());
    let mut col = 0;
    for ch in text.chars() {
        if ch == '\t' {
            let spaces = tabsize - (col % tabsize);
            result.push_str(&" ".repeat(spaces));
            col += spaces;
        } else {
            result.push(ch);
            if ch == '\n' {
                col = 0;
            } else {
                col += 1;
            }
        }
    }
    result
}

/// Returns true for whitespace characters that `textwrap` normalizes.
fn is_textwrap_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}')
}

/// Left-strips `textwrap` whitespace.
fn lstrip_textwrap_whitespace(s: &str) -> &str {
    let byte_idx = s
        .char_indices()
        .find_map(|(idx, ch)| if is_textwrap_whitespace(ch) { None } else { Some(idx) })
        .unwrap_or(s.len());
    &s[byte_idx..]
}

/// Munges whitespace according to CPython `TextWrapper._munge_whitespace`.
fn munge_whitespace(text: &str, expand_tabs: bool, replace_whitespace: bool, tabsize: usize) -> String {
    let expanded = if expand_tabs {
        expand_tabs_text(text, tabsize)
    } else {
        text.to_owned()
    };

    if !replace_whitespace {
        return expanded;
    }

    expanded
        .chars()
        .map(|ch| if is_textwrap_whitespace(ch) { ' ' } else { ch })
        .collect()
}

/// Splits a string at a char index.
fn split_at_char_index(s: &str, char_idx: usize) -> (&str, &str) {
    if char_idx == 0 {
        return ("", s);
    }
    let byte_idx = s.char_indices().nth(char_idx).map_or(s.len(), |(idx, _)| idx);
    (&s[..byte_idx], &s[byte_idx..])
}

/// Splits a token at single-hyphen boundaries, preserving the trailing hyphen.
fn split_word_on_hyphens(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() < 3 {
        return vec![token.to_owned()];
    }

    let mut parts = Vec::new();
    let mut start = 0usize;
    for idx in 1..chars.len().saturating_sub(1) {
        if chars[idx] == '-' && chars[idx - 1] != '-' && chars[idx + 1] != '-' {
            let segment: String = chars[start..=idx].iter().collect();
            if !segment.is_empty() {
                parts.push(segment);
            }
            start = idx + 1;
        }
    }
    let tail: String = chars[start..].iter().collect();
    if !tail.is_empty() {
        parts.push(tail);
    }
    if parts.is_empty() {
        vec![token.to_owned()]
    } else {
        parts
    }
}

/// Splits a token around em-dash runs (`--` or longer), preserving the runs.
fn split_on_dash_runs(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] != '-' {
            idx += 1;
            continue;
        }

        let run_start = idx;
        while idx < chars.len() && chars[idx] == '-' {
            idx += 1;
        }
        let run_len = idx - run_start;
        if run_len >= 2 {
            if run_start > start {
                parts.push(chars[start..run_start].iter().collect());
            }
            parts.push(chars[run_start..idx].iter().collect());
            start = idx;
        }
    }

    if start < chars.len() {
        parts.push(chars[start..].iter().collect());
    }

    if parts.is_empty() {
        vec![token.to_owned()]
    } else {
        parts
    }
}

/// Splits text into wrappable chunks (whitespace chunks and non-whitespace chunks).
fn split_chunks(text: &str, break_on_hyphens: bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut idx = 0usize;

    while idx < chars.len() {
        if is_textwrap_whitespace(chars[idx]) {
            let start = idx;
            while idx < chars.len() && is_textwrap_whitespace(chars[idx]) {
                idx += 1;
            }
            chunks.push(chars[start..idx].iter().collect());
            continue;
        }

        let start = idx;
        while idx < chars.len() && !is_textwrap_whitespace(chars[idx]) {
            idx += 1;
        }
        let token: String = chars[start..idx].iter().collect();
        if break_on_hyphens {
            for segment in split_on_dash_runs(&token) {
                if segment.chars().all(|ch| ch == '-') && segment.chars().count() >= 2 {
                    chunks.push(segment);
                } else {
                    chunks.extend(split_word_on_hyphens(&segment));
                }
            }
        } else {
            chunks.push(token);
        }
    }

    chunks
}

/// Checks whether a chunk ends with sentence punctuation in CPython's simple rule.
fn chunk_ends_sentence(chunk: &str) -> bool {
    if chunk.is_empty() {
        return false;
    }
    let mut chars: Vec<char> = chunk.chars().collect();
    if matches!(chars.last(), Some('"' | '\'')) {
        chars.pop();
    }
    if chars.len() < 2 {
        return false;
    }
    let punct = chars[chars.len() - 1];
    if !matches!(punct, '.' | '!' | '?') {
        return false;
    }
    chars[chars.len() - 2].is_ascii_lowercase()
}

/// Applies sentence-ending spacing fix to chunks.
fn fix_sentence_endings_chunks(chunks: &mut [String]) {
    let mut idx = 0usize;
    while idx + 1 < chunks.len() {
        if chunks[idx + 1] == " " && chunk_ends_sentence(&chunks[idx]) {
            chunks[idx + 1] = "  ".to_owned();
            idx += 2;
        } else {
            idx += 1;
        }
    }
}

/// Handles a chunk that is too long to fit on any line.
fn handle_long_word(
    reversed_chunks: &mut Vec<String>,
    cur_line: &mut Vec<String>,
    cur_len: usize,
    width: usize,
    break_long_words: bool,
    break_on_hyphens: bool,
) {
    let space_left = if width < 1 { 1 } else { width.saturating_sub(cur_len) };

    if break_long_words {
        let Some(chunk) = reversed_chunks.last_mut() else {
            return;
        };
        let chunk_len = chunk.chars().count();
        let mut end = space_left.min(chunk_len);

        if break_on_hyphens && chunk_len > space_left {
            let chars: Vec<char> = chunk.chars().collect();
            let mut hyphen_break = None;
            for idx in (1..space_left.min(chars.len())).rev() {
                if chars[idx] == '-' && chars[..idx].iter().any(|ch| *ch != '-') {
                    hyphen_break = Some(idx + 1);
                    break;
                }
            }
            if let Some(pos) = hyphen_break {
                end = pos;
            }
        }

        let owned_chunk = chunk.clone();
        let (prefix, suffix) = split_at_char_index(&owned_chunk, end);
        cur_line.push(prefix.to_owned());
        *chunk = suffix.to_owned();
        if chunk.is_empty() {
            reversed_chunks.pop();
        }
    } else if cur_line.is_empty()
        && let Some(chunk) = reversed_chunks.pop()
    {
        cur_line.push(chunk);
    }
}

/// Wraps prepared chunks using CPython-style chunk logic.
fn wrap_chunks(mut chunks: Vec<String>, wrapper: &TextWrapper) -> Vec<String> {
    let mut lines = Vec::new();
    chunks.reverse();

    while !chunks.is_empty() {
        let mut cur_line = Vec::new();
        let mut cur_len = 0usize;

        let indent = if lines.is_empty() {
            &wrapper.initial_indent
        } else {
            &wrapper.subsequent_indent
        };
        let width = wrapper.width.saturating_sub(indent.chars().count());

        if wrapper.drop_whitespace && !lines.is_empty() && chunks.last().is_some_and(|chunk| chunk.trim().is_empty()) {
            chunks.pop();
        }

        while let Some(next) = chunks.last() {
            let next_len = next.chars().count();
            if cur_len + next_len <= width {
                let chunk = chunks.pop().expect("checked is_some");
                cur_len += next_len;
                cur_line.push(chunk);
            } else {
                break;
            }
        }

        if chunks.last().is_some_and(|chunk| chunk.chars().count() > width) {
            handle_long_word(
                &mut chunks,
                &mut cur_line,
                cur_len,
                width,
                wrapper.break_long_words,
                wrapper.break_on_hyphens,
            );
            cur_len = cur_line.iter().map(|chunk| chunk.chars().count()).sum();
        }

        if wrapper.drop_whitespace
            && cur_line.last().is_some_and(|chunk| chunk.trim().is_empty())
            && let Some(last) = cur_line.pop()
        {
            cur_len = cur_len.saturating_sub(last.chars().count());
        }

        if !cur_line.is_empty() {
            let should_push = wrapper.max_lines.is_none()
                || lines.len() + 1 < wrapper.max_lines.unwrap_or(usize::MAX)
                || ((chunks.is_empty()
                    || (wrapper.drop_whitespace && chunks.len() == 1 && chunks[0].trim().is_empty()))
                    && cur_len <= width);

            if should_push {
                lines.push(format!("{indent}{}", cur_line.join("")));
                continue;
            }

            let mut unresolved = cur_line;
            let placeholder_len = wrapper.placeholder.chars().count();
            while !unresolved.is_empty() {
                if unresolved.last().is_some_and(|chunk| !chunk.trim().is_empty()) && cur_len + placeholder_len <= width
                {
                    unresolved.push(wrapper.placeholder.clone());
                    lines.push(format!("{indent}{}", unresolved.join("")));
                    return lines;
                }
                if let Some(removed) = unresolved.pop() {
                    cur_len = cur_len.saturating_sub(removed.chars().count());
                }
            }

            if let Some(prev_line) = lines.last_mut() {
                let prev_trimmed = prev_line.trim_end().to_owned();
                if prev_trimmed.chars().count() + placeholder_len <= wrapper.width {
                    *prev_line = format!("{prev_trimmed}{}", wrapper.placeholder);
                    return lines;
                }
            }

            lines.push(format!("{indent}{}", lstrip_textwrap_whitespace(&wrapper.placeholder)));
            return lines;
        }
    }

    lines
}

/// Wraps text into lines under `width`, applying textwrap options.
fn wrap_lines(text: &str, wrapper: &TextWrapper) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let munged = munge_whitespace(text, wrapper.expand_tabs, wrapper.replace_whitespace, wrapper.tabsize);
    let mut chunks = split_chunks(&munged, wrapper.break_on_hyphens);
    if wrapper.fix_sentence_endings {
        fix_sentence_endings_chunks(&mut chunks);
    }
    wrap_chunks(chunks, wrapper)
}

/// Validates config values used by wrapping calls.
fn validate_wrapper_config(wrapper: &TextWrapper) -> RunResult<()> {
    if wrapper.width == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "invalid width 0 (must be > 0)").into());
    }
    if let Some(max_lines) = wrapper.max_lines {
        let indent = if max_lines > 1 {
            &wrapper.subsequent_indent
        } else {
            &wrapper.initial_indent
        };
        if indent.chars().count() + lstrip_textwrap_whitespace(&wrapper.placeholder).chars().count() > wrapper.width {
            return Err(SimpleException::new_msg(ExcType::ValueError, "placeholder too large for max width").into());
        }
    }
    Ok(())
}

/// Allocates a heap string value.
fn allocate_string(s: &str, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let id = heap.allocate(HeapData::Str(Str::from(s.to_owned())))?;
    Ok(Value::Ref(id))
}

/// Allocates a list of heap strings.
fn allocate_string_list(lines: Vec<String>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let mut values = Vec::with_capacity(lines.len());
    for line in lines {
        let id = heap.allocate(HeapData::Str(Str::from(line)))?;
        values.push(Value::Ref(id));
    }
    let list = List::new(values);
    let id = heap.allocate(HeapData::List(list))?;
    Ok(Value::Ref(id))
}
