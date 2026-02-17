//! Implementation of the `textwrap` module.
//!
//! Provides text wrapping and indentation utilities.

use crate::{
    args::ArgValues,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, List, PyTrait, Str, TextWrapper},
    value::Value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum TextwrapFunctions {
    Dedent,
    Indent,
    Fill,
    Wrap,
    Shorten,
    #[strum(serialize = "TextWrapper")]
    TextWrapper,
}

pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Textwrap);

    module.set_attr(
        StaticStrings::TwDedent,
        Value::ModuleFunction(ModuleFunctions::Textwrap(TextwrapFunctions::Dedent)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::TwIndent,
        Value::ModuleFunction(ModuleFunctions::Textwrap(TextwrapFunctions::Indent)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::TwFill,
        Value::ModuleFunction(ModuleFunctions::Textwrap(TextwrapFunctions::Fill)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::TwWrap,
        Value::ModuleFunction(ModuleFunctions::Textwrap(TextwrapFunctions::Wrap)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::TwShorten,
        Value::ModuleFunction(ModuleFunctions::Textwrap(TextwrapFunctions::Shorten)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::TwTextWrapper,
        Value::ModuleFunction(ModuleFunctions::Textwrap(TextwrapFunctions::TextWrapper)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TextwrapFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        TextwrapFunctions::Dedent => dedent(heap, interns, args),
        TextwrapFunctions::Indent => indent(heap, interns, args),
        TextwrapFunctions::Fill => fill(heap, interns, args),
        TextwrapFunctions::Wrap => wrap(heap, interns, args),
        TextwrapFunctions::Shorten => shorten(heap, interns, args),
        TextwrapFunctions::TextWrapper => text_wrapper(heap, interns, args),
    }
}

fn text_wrapper(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();

    let mut wrapper = TextWrapper::new(70);

    if let Some(width_arg) = positional.next() {
        let width_val = width_arg.as_int(heap)?;
        width_arg.drop_with_heap(heap);
        if width_val <= 0 {
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::ValueError, "invalid width").into());
        }
        wrapper.width = usize::try_from(width_val).expect("validated positive width");
    }
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("TextWrapper", 1, 2));
    }

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = keyword_name.as_str(interns);
        key.drop_with_heap(heap);

        match key_name {
            "width" => {
                let width_val = value.as_int(heap)?;
                value.drop_with_heap(heap);
                if width_val <= 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "invalid width").into());
                }
                wrapper.width = usize::try_from(width_val).expect("validated positive width");
            }
            "initial_indent" => {
                wrapper.initial_indent = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "subsequent_indent" => {
                wrapper.subsequent_indent = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "placeholder" => {
                wrapper.placeholder = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "max_lines" => {
                if matches!(value, Value::None) {
                    wrapper.max_lines = None;
                    value.drop_with_heap(heap);
                } else {
                    let max = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    if max <= 0 {
                        return Err(SimpleException::new_msg(ExcType::ValueError, "max_lines must be > 0").into());
                    }
                    wrapper.max_lines = Some(usize::try_from(max).expect("validated positive max_lines"));
                }
            }
            "break_long_words" => {
                wrapper.break_long_words = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "expand_tabs" => {
                wrapper.expand_tabs = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "replace_whitespace" => {
                wrapper.replace_whitespace = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "fix_sentence_endings" => {
                wrapper.fix_sentence_endings = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "drop_whitespace" => {
                wrapper.drop_whitespace = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "break_on_hyphens" => {
                wrapper.break_on_hyphens = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "tabsize" => {
                let tabsize_val = value.as_int(heap)?;
                value.drop_with_heap(heap);
                if tabsize_val <= 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "tabsize must be > 0").into());
                }
                wrapper.tabsize = usize::try_from(tabsize_val).expect("validated positive tabsize");
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("TextWrapper", key_name));
            }
        }
    }

    let id = heap.allocate(HeapData::TextWrapper(wrapper))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

fn dedent(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let input = args.get_one_arg("textwrap.dedent", heap)?;

    let text = match input.py_str(heap, interns) {
        std::borrow::Cow::Borrowed(s) => s,
        std::borrow::Cow::Owned(owned_str) => {
            let result = dedent_string(&owned_str);
            input.drop_with_heap(heap);
            return allocate_string_result(result, heap);
        }
    };

    let result = dedent_string(text);
    input.drop_with_heap(heap);
    allocate_string_result(result, heap)
}

fn dedent_string(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut lines: Vec<String> = text.split('\n').map(ToOwned::to_owned).collect();

    // CPython strips whitespace-only lines before computing indentation margin.
    for line in &mut lines {
        if line.chars().all(|ch| ch == ' ' || ch == '\t') {
            line.clear();
        }
    }

    let mut margin: Option<String> = None;
    for line in &lines {
        if line.is_empty() {
            continue;
        }
        let indent: String = line.chars().take_while(|ch| *ch == ' ' || *ch == '\t').collect();
        margin = match margin {
            None => Some(indent),
            Some(current) if indent.starts_with(&current) => Some(current),
            Some(current) if current.starts_with(&indent) => Some(indent),
            Some(current) => Some(
                current
                    .chars()
                    .zip(indent.chars())
                    .take_while(|(left, right)| left == right)
                    .map(|(ch, _)| ch)
                    .collect(),
            ),
        };
    }

    let margin = margin.unwrap_or_default();
    if !margin.is_empty() {
        for line in &mut lines {
            if line.starts_with(&margin) {
                line.drain(..margin.len());
            }
        }
    }

    lines.join("\n")
}

fn indent(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(text_val) = positional.next() else {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("textwrap.indent expected at least 2 arguments"));
    };
    let Some(prefix_val) = positional.next() else {
        text_val.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("textwrap.indent expected at least 2 arguments"));
    };

    let mut predicate = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        text_val.drop_with_heap(heap);
        prefix_val.drop_with_heap(heap);
        predicate.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("textwrap.indent expected at most 3 arguments"));
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            text_val.drop_with_heap(heap);
            prefix_val.drop_with_heap(heap);
            predicate.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        if key_name == "predicate" {
            if predicate.is_some() {
                value.drop_with_heap(heap);
                text_val.drop_with_heap(heap);
                prefix_val.drop_with_heap(heap);
                predicate.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "textwrap.indent got multiple values for argument 'predicate'",
                ));
            }
            predicate = Some(value);
        } else {
            value.drop_with_heap(heap);
            text_val.drop_with_heap(heap);
            prefix_val.drop_with_heap(heap);
            predicate.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("textwrap.indent", &key_name));
        }
    }

    let text = text_val.py_str(heap, interns).into_owned();
    let prefix = prefix_val.py_str(heap, interns).into_owned();
    text_val.drop_with_heap(heap);
    prefix_val.drop_with_heap(heap);

    let lines = split_lines_keep_ends(&text);
    match predicate {
        None => {
            let result = indent_lines_with_default_predicate(lines, &prefix);
            allocate_string_result(result, heap)
        }
        Some(predicate) if matches!(predicate, Value::None) => {
            predicate.drop_with_heap(heap);
            let result = indent_lines_with_default_predicate(lines, &prefix);
            allocate_string_result(result, heap)
        }
        Some(predicate) => Ok(AttrCallResult::TextwrapIndentCall(predicate, lines, prefix)),
    }
}

/// Splits text into lines preserving line terminators (like Python `splitlines(True)`).
fn split_lines_keep_ends(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            lines.push(text[start..=idx].to_owned());
            start = idx + 1;
        }
    }
    if start < text.len() {
        lines.push(text[start..].to_owned());
    }
    lines
}

/// Applies CPython's default indent predicate (`lambda line: line.strip()`).
fn indent_lines_with_default_predicate(lines: Vec<String>, prefix: &str) -> String {
    let mut output = String::new();
    for line in lines {
        if !line.trim().is_empty() {
            output.push_str(prefix);
        }
        output.push_str(&line);
    }
    output
}

fn fill(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (text, lines) = wrap_with_options(args, "textwrap.fill", heap, interns)?;
    text.drop_with_heap(heap);
    allocate_string_result(lines.join("\n"), heap)
}

fn wrap(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (text, lines) = wrap_with_options(args, "textwrap.wrap", heap, interns)?;
    text.drop_with_heap(heap);
    allocate_string_list(lines, heap)
}

struct WrapOptions {
    width: usize,
    initial_indent: String,
    subsequent_indent: String,
    placeholder: String,
    max_lines: Option<usize>,
    break_long_words: bool,
    expand_tabs: bool,
    replace_whitespace: bool,
    fix_sentence_endings: bool,
    drop_whitespace: bool,
    break_on_hyphens: bool,
    tabsize: usize,
}

impl WrapOptions {
    fn new() -> Self {
        Self {
            width: 70,
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
}

fn wrap_with_options(
    args: ArgValues,
    func_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Vec<String>)> {
    let (pos_args, kwargs) = args.into_parts();
    defer_drop_mut!(pos_args, heap);
    let kwargs = kwargs.into_iter();
    defer_drop_mut!(kwargs, heap);

    let mut options = WrapOptions::new();

    let Some(text_val) = pos_args.next() else {
        for (k, v) in kwargs {
            k.drop_with_heap(heap);
            v.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(format!(
            "{func_name}() missing 1 required positional argument: 'text'"
        )));
    };

    let pos_width = pos_args.next();

    if let Some(extra) = pos_args.next() {
        extra.drop_with_heap(heap);
        text_val.drop_with_heap(heap);
        pos_width.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{func_name}() takes at most 2 positional arguments"
        )));
    }

    let mut kwarg_width: Option<Value> = None;
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        let Some(keyword_name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);

        match key_str {
            "width" => {
                kwarg_width = Some(value);
            }
            "initial_indent" => {
                options.initial_indent = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "subsequent_indent" => {
                options.subsequent_indent = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "placeholder" => {
                options.placeholder = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            "max_lines" => {
                if matches!(value, Value::None) {
                    options.max_lines = None;
                    value.drop_with_heap(heap);
                } else {
                    let max = value.as_int(heap)?;
                    value.drop_with_heap(heap);
                    if max <= 0 {
                        return Err(SimpleException::new_msg(ExcType::ValueError, "max_lines must be > 0").into());
                    }
                    options.max_lines = Some(usize::try_from(max).expect("validated positive max_lines"));
                }
            }
            "break_long_words" => {
                options.break_long_words = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "expand_tabs" => {
                options.expand_tabs = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "replace_whitespace" => {
                options.replace_whitespace = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "fix_sentence_endings" => {
                options.fix_sentence_endings = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "drop_whitespace" => {
                options.drop_whitespace = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "break_on_hyphens" => {
                options.break_on_hyphens = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            "tabsize" => {
                let tabsize_val = value.as_int(heap)?;
                value.drop_with_heap(heap);
                if tabsize_val <= 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "tabsize must be > 0").into());
                }
                options.tabsize = usize::try_from(tabsize_val).expect("validated positive tabsize");
            }
            "text" => {
                value.drop_with_heap(heap);
                text_val.drop_with_heap(heap);
                kwarg_width.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{func_name}() got multiple values for argument 'text'"
                )));
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(func_name, key_str));
            }
        }
    }

    if let (Some(_), Some(_)) = (&pos_width, &kwarg_width) {
        pos_width.drop_with_heap(heap);
        kwarg_width.drop_with_heap(heap);
        text_val.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "{func_name}() got multiple values for argument 'width'"
        )));
    }

    let width_val = kwarg_width.or(pos_width);
    if let Some(width_val) = width_val {
        let width = width_val.as_int(heap)?;
        width_val.drop_with_heap(heap);
        if width <= 0 {
            text_val.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::ValueError, "width must be positive").into());
        }
        options.width = usize::try_from(width).expect("width is non-negative and fits in usize");
    }

    let text = text_val.py_str(heap, interns).into_owned();

    let wrapper = TextWrapper {
        width: options.width,
        initial_indent: options.initial_indent,
        subsequent_indent: options.subsequent_indent,
        placeholder: options.placeholder,
        max_lines: options.max_lines,
        break_long_words: options.break_long_words,
        expand_tabs: options.expand_tabs,
        replace_whitespace: options.replace_whitespace,
        fix_sentence_endings: options.fix_sentence_endings,
        drop_whitespace: options.drop_whitespace,
        break_on_hyphens: options.break_on_hyphens,
        tabsize: options.tabsize,
    };

    let lines = wrap_text_with_wrapper(&text, &wrapper);

    Ok((text_val, lines))
}

fn wrap_text_with_wrapper(text: &str, wrapper: &TextWrapper) -> Vec<String> {
    wrapper.wrap_text(text)
}

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

fn replace_whitespace_text(text: &str, replace_whitespace: bool, _expand_tabs: bool) -> String {
    if !replace_whitespace {
        return text.to_owned();
    }
    // Replace whitespace chars that CPython considers "whitespace" for text wrapping
    // CPython's WHITESPACE = ' \t\n\v\f\r' - all become spaces
    // Note: If expand_tabs was False, we still replace tabs with single spaces here
    text.chars()
        .map(|c| {
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\x0b' || c == '\x0c' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn fix_sentence_endings_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        result.push(chars[i]);

        if (chars[i] == '.' || chars[i] == '?' || chars[i] == '!')
            && i + 2 < chars.len()
            && chars[i + 1] == ' '
            && chars[i + 2].is_uppercase()
        {
            result.push(' ');
        }

        i += 1;
    }

    result
}

/// Split text into words along with the number of leading spaces for each word.
///
/// Returns a vector of (word, leading_spaces) tuples where leading_spaces indicates
/// how many spaces were before this word. A value of 2 or more typically indicates
/// a sentence break added by fix_sentence_endings.
fn split_words_with_spacing(text: &str, replace_whitespace: bool) -> Vec<(String, usize)> {
    if text.is_empty() {
        return Vec::new();
    }

    if replace_whitespace {
        // When replace_whitespace=True, we normalize all whitespace to single spaces
        // but track consecutive spaces to detect sentence breaks
        let normalized: String = text
            .chars()
            .map(|c| if c.is_ascii_whitespace() { ' ' } else { c })
            .collect();

        let mut result = Vec::new();

        // Split and track spaces
        let parts: Vec<&str> = normalized.split(' ').collect();
        let mut leading_spaces_for_next = 0;

        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                // Empty part means consecutive space
                leading_spaces_for_next += 1;
            } else {
                // Non-empty part is a word
                // First word has 0 leading spaces, others have at least 1
                let spaces = if i == 0 {
                    0
                } else {
                    (1 + leading_spaces_for_next).max(1)
                };
                result.push((part.to_string(), spaces));
                leading_spaces_for_next = 0;
            }
        }

        result
    } else {
        // When replace_whitespace=False, use standard whitespace splitting
        text.split_whitespace()
            .enumerate()
            .map(|(i, w)| (w.to_string(), usize::from(i != 0)))
            .collect()
    }
}

fn drop_whitespace_line(line: &str, drop_whitespace: bool) -> String {
    if !drop_whitespace {
        return line.to_owned();
    }
    line.trim().to_owned()
}

fn find_word_breaks(word: &str, width: usize, break_long_words: bool, break_on_hyphens: bool) -> Vec<String> {
    let mut parts = Vec::new();
    let word_chars: Vec<char> = word.chars().collect();
    let word_len = word_chars.len();

    if word_len <= width {
        return vec![word.to_owned()];
    }

    if !break_long_words {
        return vec![word.to_owned()];
    }

    let mut start = 0;
    while start < word_len {
        let remaining = word_len - start;

        if remaining <= width {
            let part: String = word_chars[start..].iter().collect();
            parts.push(part);
            break;
        }

        let end = (start + width).min(word_len);

        if break_on_hyphens {
            let mut best_break = end;
            for i in (start + 1..=end).rev() {
                if word_chars[i - 1] == '-' {
                    best_break = i;
                    break;
                }
            }

            let part: String = word_chars[start..best_break].iter().collect();
            parts.push(part);
            start = best_break;
        } else {
            let part: String = word_chars[start..end].iter().collect();
            parts.push(part);
            start = end;
        }
    }

    parts
}

#[expect(clippy::too_many_arguments)]
fn wrap_lines(
    text: &str,
    width: usize,
    initial_indent: &str,
    subsequent_indent: &str,
    placeholder: &str,
    max_lines: Option<usize>,
    break_long_words: bool,
    expand_tabs: bool,
    replace_whitespace: bool,
    fix_sentence_endings: bool,
    drop_whitespace: bool,
    break_on_hyphens: bool,
    tabsize: usize,
) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    // Step 1: Expand tabs first (before any other processing)
    let text = if expand_tabs {
        expand_tabs_text(text, tabsize)
    } else {
        text.to_owned()
    };

    // Step 2: Process paragraphs separately
    // Split on \n\n to preserve paragraph breaks
    let raw_paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut all_lines = Vec::new();
    let mut is_first_para = true;

    for raw_para in &raw_paragraphs {
        // Handle paragraph break (empty line between paragraphs)
        if !is_first_para {
            // Add an empty line between paragraphs (unless drop_whitespace removes it)
            if !drop_whitespace {
                all_lines.push(String::new());
            }
        }
        is_first_para = false;

        // Skip empty paragraphs
        if raw_para.trim().is_empty() {
            continue;
        }

        // Step 3: Replace whitespace within paragraph
        let para = if replace_whitespace {
            // All whitespace becomes spaces
            raw_para
                .chars()
                .map(|c| if c.is_ascii_whitespace() { ' ' } else { c })
                .collect::<String>()
        } else {
            // Keep tabs (already expanded) and newlines, but replace other control chars
            raw_para
                .chars()
                .map(|c| {
                    if c == '\x0b' || c == '\x0c' || c == '\r' {
                        ' '
                    } else {
                        c
                    }
                })
                .collect::<String>()
        };

        // Step 4: Fix sentence endings BEFORE collapsing whitespace
        // This ensures the double spaces are preserved
        let para = if fix_sentence_endings {
            fix_sentence_endings_text(&para)
        } else {
            para
        };

        // Step 5: Wrap this paragraph
        // When replace_whitespace=True, we collapse whitespace in wrap_paragraph
        // When replace_whitespace=False, newlines are preserved as line boundaries
        let para_lines = wrap_paragraph(
            &para,
            width,
            initial_indent,
            subsequent_indent,
            placeholder,
            max_lines.as_ref().map(|m| {
                let used = all_lines.len();
                (*m).saturating_sub(used)
            }),
            break_long_words,
            drop_whitespace,
            break_on_hyphens,
            replace_whitespace,
        );

        all_lines.extend(para_lines);
    }

    if let Some(max) = max_lines
        && all_lines.len() > max
        && max > 0
    {
        let mut truncated: Vec<String> = all_lines.into_iter().take(max).collect();
        if let Some(last) = truncated.last_mut() {
            let is_last_first = max == 1;
            let indent = if is_last_first {
                initial_indent
            } else {
                subsequent_indent
            };
            let available = width.saturating_sub(indent.chars().count());

            let placeholder_chars: Vec<char> = placeholder.chars().collect();
            let placeholder_len = placeholder_chars.len();

            if placeholder_len <= available {
                let base = last.clone();
                let base_chars: Vec<char> = base.chars().collect();

                if base_chars.len() + placeholder_len > available {
                    let keep = available.saturating_sub(placeholder_len);
                    let truncated_base: String = base_chars.into_iter().take(keep).collect();
                    *last = format!("{indent}{truncated_base}{placeholder}");
                } else {
                    *last = format!("{indent}{base}{placeholder}");
                }
            }
        }
        return truncated;
    }

    all_lines
}

/// Wrap a single paragraph, applying indentation to each line.
#[expect(clippy::too_many_arguments)]
fn wrap_paragraph(
    para: &str,
    width: usize,
    initial_indent: &str,
    subsequent_indent: &str,
    _placeholder: &str,
    max_lines: Option<usize>,
    break_long_words: bool,
    drop_whitespace: bool,
    break_on_hyphens: bool,
    replace_whitespace: bool,
) -> Vec<String> {
    let mut lines = Vec::new();

    // When replace_whitespace=False, newlines are preserved as line boundaries
    // When replace_whitespace=True, the whole para is treated as one line
    let input_lines: Vec<&str> = if replace_whitespace {
        vec![para]
    } else {
        para.lines().collect()
    };

    let mut is_first_line_of_para = true;

    for input_line in input_lines {
        // Get words from this input line along with spacing info
        let words_with_spacing = split_words_with_spacing(input_line, replace_whitespace);

        if words_with_spacing.is_empty() {
            // Empty input line becomes an empty output line (unless drop_whitespace)
            if !drop_whitespace {
                lines.push(String::new());
            }
            is_first_line_of_para = false;
            continue;
        }

        let mut current_line = String::new();
        for (i, (word, leading_spaces)) in words_with_spacing.into_iter().enumerate() {
            let indent = if is_first_line_of_para {
                initial_indent
            } else {
                subsequent_indent
            };
            let available_width = width.saturating_sub(indent.chars().count());

            if available_width == 0 {
                // No space available, just output the indent
                lines.push(indent.to_owned());
                continue;
            }

            let word_len = word.chars().count();

            // Determine spacing before this word
            // leading_spaces > 1 indicates sentence break from fix_sentence_endings
            let space_before = if i == 0 {
                0
            } else if leading_spaces >= 2 {
                2 // Sentence break - preserve double space
            } else {
                1 // Normal word break
            };

            if current_line.is_empty() {
                // Starting a new line
                if word_len <= available_width {
                    current_line.push_str(&word);
                } else {
                    // Word is longer than available width - need to break it
                    let broken = find_word_breaks(&word, available_width, break_long_words, break_on_hyphens);
                    for (j, part) in broken.into_iter().enumerate() {
                        if j == 0 {
                            current_line = part;
                        } else {
                            // Flush previous part
                            let line_with_indent = format!("{indent}{current_line}");
                            let line = drop_whitespace_line(&line_with_indent, drop_whitespace);
                            if !line.is_empty() || !drop_whitespace {
                                lines.push(line);
                            }
                            is_first_line_of_para = false;
                            current_line = part;
                        }
                    }
                }
            } else {
                // Adding to existing line
                let current_len = current_line.chars().count();
                let needed = current_len + space_before + word_len;

                if needed <= available_width {
                    // Add appropriate spacing
                    for _ in 0..space_before {
                        current_line.push(' ');
                    }
                    current_line.push_str(&word);
                } else {
                    // Flush current line
                    let line_with_indent = format!("{indent}{current_line}");
                    let line = drop_whitespace_line(&line_with_indent, drop_whitespace);
                    if !line.is_empty() || !drop_whitespace {
                        lines.push(line);
                    }
                    is_first_line_of_para = false;

                    // Start new line with word
                    let new_indent = if is_first_line_of_para {
                        initial_indent
                    } else {
                        subsequent_indent
                    };
                    let new_available = width.saturating_sub(new_indent.chars().count());

                    if word_len > new_available {
                        let broken = find_word_breaks(&word, new_available, break_long_words, break_on_hyphens);
                        for (j, part) in broken.into_iter().enumerate() {
                            if j == 0 {
                                current_line = part;
                            } else {
                                let line_with_indent = format!("{new_indent}{current_line}");
                                let line = drop_whitespace_line(&line_with_indent, drop_whitespace);
                                if !line.is_empty() || !drop_whitespace {
                                    lines.push(line);
                                }
                                is_first_line_of_para = false;
                                current_line = part;
                            }
                        }
                    } else {
                        current_line = word;
                    }
                }
            }
        }

        // Flush remaining content in current_line
        if !current_line.is_empty() {
            let indent = if is_first_line_of_para {
                initial_indent
            } else {
                subsequent_indent
            };
            let line_with_indent = format!("{indent}{current_line}");
            let line = drop_whitespace_line(&line_with_indent, drop_whitespace);
            if !line.is_empty() || !drop_whitespace {
                lines.push(line);
            }
            is_first_line_of_para = false;
        }

        // Check max_lines
        if let Some(max) = max_lines
            && lines.len() >= max
        {
            return lines.into_iter().take(max).collect();
        }
    }

    lines
}

fn allocate_string_result(result: String, heap: &mut Heap<impl ResourceTracker>) -> RunResult<AttrCallResult> {
    let str_obj = Str::from(result);
    let id = heap.allocate(HeapData::Str(str_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

fn shorten(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();
    let mut text_val = pos.next();
    let mut width_val = pos.next();

    if let Some(v) = pos.next() {
        v.drop_with_heap(heap);
        text_val.drop_with_heap(heap);
        width_val.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "textwrap.shorten() takes 2 positional arguments".to_string(),
        ));
    }

    let mut placeholder = " [...]".to_owned();
    let mut break_long_words = true;
    let mut break_on_hyphens = true;
    for (key, value) in kwargs {
        let key_name = if let Some(name) = key.as_either_str(heap) {
            let s = name.as_str(interns).to_owned();
            key.drop_with_heap(heap);
            s
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            text_val.drop_with_heap(heap);
            width_val.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        if key_name == "placeholder" {
            placeholder = value.py_str(heap, interns).into_owned();
            value.drop_with_heap(heap);
        } else if key_name == "break_long_words" {
            break_long_words = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
        } else if key_name == "break_on_hyphens" {
            break_on_hyphens = value.py_bool(heap, interns);
            value.drop_with_heap(heap);
        } else if key_name == "width" {
            if width_val.is_some() {
                value.drop_with_heap(heap);
                text_val.drop_with_heap(heap);
                width_val.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "textwrap.shorten() got multiple values for argument 'width'".to_string(),
                ));
            }
            width_val = Some(value);
        } else if key_name == "text" {
            if text_val.is_some() {
                value.drop_with_heap(heap);
                text_val.drop_with_heap(heap);
                width_val.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "textwrap.shorten() got multiple values for argument 'text'".to_string(),
                ));
            }
            text_val = Some(value);
        } else {
            value.drop_with_heap(heap);
            text_val.drop_with_heap(heap);
            width_val.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for shorten()"
            )));
        }
    }

    let Some(text_val) = text_val else {
        width_val.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "textwrap.shorten() missing required argument: 'text'".to_string(),
        ));
    };
    let Some(width_val) = width_val else {
        text_val.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "textwrap.shorten() missing required argument: 'width'".to_string(),
        ));
    };

    let width = width_val.as_int(heap)?;
    width_val.drop_with_heap(heap);
    if width <= 0 {
        text_val.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::ValueError, format!("invalid width {width} (must be > 0)")).into(),
        );
    }
    let width = usize::try_from(width).expect("width is non-negative and fits in usize");

    let text = text_val.py_str(heap, interns).into_owned();
    text_val.drop_with_heap(heap);

    if placeholder.trim_start().chars().count() > width {
        return Err(SimpleException::new_msg(ExcType::ValueError, "placeholder too large for max width").into());
    }

    let result = shorten_text(&text, width, &placeholder, break_long_words, break_on_hyphens);
    allocate_string_result(result, heap)
}

fn shorten_text(text: &str, width: usize, placeholder: &str, break_long_words: bool, break_on_hyphens: bool) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.chars().count() <= width {
        return collapsed;
    }

    let wrapper = TextWrapper {
        width,
        initial_indent: String::new(),
        subsequent_indent: String::new(),
        placeholder: placeholder.to_owned(),
        max_lines: Some(1),
        break_long_words,
        expand_tabs: true,
        replace_whitespace: true,
        fix_sentence_endings: false,
        drop_whitespace: true,
        break_on_hyphens,
        tabsize: 8,
    };

    let lines = wrapper.wrap_text(&collapsed);
    lines.into_iter().next().unwrap_or_default()
}

fn allocate_string_list(lines: Vec<String>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<AttrCallResult> {
    let mut values: Vec<Value> = Vec::with_capacity(lines.len());
    for line in lines {
        let str_obj = Str::from(line);
        let id = heap.allocate(HeapData::Str(str_obj))?;
        values.push(Value::Ref(id));
    }
    let list = List::new(values);
    let id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}
