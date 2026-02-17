//! Implementation of the `tokenize` module.
//!
//! Ouros provides a deterministic, sandbox-safe tokenizer for string input.
//! The implementation mirrors core CPython APIs used by parity tests:
//! - `tokenize.tokenize(readline)`
//! - `tokenize.generate_tokens(readline)`
//! - `tokenize.detect_encoding(readline)`
//! - `tokenize.untokenize(iterable)`
//! - `tokenize.open(filename)` (sandbox error)
//! - `TokenInfo` namedtuple and `TokenError` exception class

use smallvec::smallvec;

use super::token_mod::{exact_token_types, token_constants};
use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, List, Module, NamedTuple, NamedTupleFactory, OurosIter, Str, Type,
        allocate_tuple, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

const TOK_ENDMARKER: i64 = 0;
const TOK_NAME: i64 = 1;
const TOK_NUMBER: i64 = 2;
const TOK_STRING: i64 = 3;
const TOK_NEWLINE: i64 = 4;
const TOK_INDENT: i64 = 5;
const TOK_DEDENT: i64 = 6;
const TOK_SOFT_KEYWORD: i64 = 58;
const TOK_COMMENT: i64 = 65;
const TOK_NL: i64 = 66;
const TOK_ERRORTOKEN: i64 = 67;
const TOK_ENCODING: i64 = 68;

/// `tokenize` module function entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum TokenizeFunctions {
    /// Implements `tokenize.tokenize(readline)`.
    Tokenize,
    /// Implements `tokenize.generate_tokens(readline)`.
    GenerateTokens,
    /// Implements `tokenize.detect_encoding(readline)`.
    DetectEncoding,
    /// Implements `tokenize.open(filename)`.
    Open,
    /// Implements `tokenize.untokenize(iterable)`.
    Untokenize,
    /// Implements `tokenize.ISTERMINAL(x)`.
    Isterminal,
    /// Implements `tokenize.ISNONTERMINAL(x)`.
    Isnonterminal,
    /// Implements `tokenize.ISEOF(x)`.
    Iseof,
}

/// Lightweight token record used while building `TokenInfo` values.
#[derive(Debug, Clone)]
struct TokenSpec {
    token_type: i64,
    text: String,
    start: (usize, usize),
    end: (usize, usize),
    line: String,
}

/// Creates the `tokenize` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::TokenizeMod);

    for &(name, value) in token_constants() {
        module.set_attr_text(name, Value::Int(value), heap, interns)?;
    }

    module.set_attr_text(
        "tokenize",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::Tokenize)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "generate_tokens",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::GenerateTokens)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "detect_encoding",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::DetectEncoding)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "open",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::Open)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "untokenize",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::Untokenize)),
        heap,
        interns,
    )?;

    module.set_attr_text(
        "ISTERMINAL",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::Isterminal)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ISNONTERMINAL",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::Isnonterminal)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ISEOF",
        Value::ModuleFunction(ModuleFunctions::Tokenize(TokenizeFunctions::Iseof)),
        heap,
        interns,
    )?;

    let tok_name = create_tok_name_dict(heap, interns)?;
    module.set_attr_text("tok_name", tok_name, heap, interns)?;

    let exact = create_exact_token_types_dict(heap, interns)?;
    module.set_attr_text("EXACT_TOKEN_TYPES", exact, heap, interns)?;

    let token_info_id = heap.allocate(HeapData::NamedTupleFactory(create_token_info_factory()))?;
    module.set_attr_text("TokenInfo", Value::Ref(token_info_id), heap, interns)?;

    let token_error_id = create_token_error_class(heap, interns)?;
    module.set_attr_text("TokenError", Value::Ref(token_error_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches `tokenize` module calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TokenizeFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        TokenizeFunctions::Tokenize => tokenize(heap, interns, args)?,
        TokenizeFunctions::GenerateTokens => generate_tokens(heap, interns, args)?,
        TokenizeFunctions::DetectEncoding => detect_encoding(heap, interns, args)?,
        TokenizeFunctions::Open => open_sandboxed(heap, interns, args)?,
        TokenizeFunctions::Untokenize => untokenize(heap, interns, args)?,
        TokenizeFunctions::Isterminal => is_terminal(heap, interns, args)?,
        TokenizeFunctions::Isnonterminal => is_nonterminal(heap, interns, args)?,
        TokenizeFunctions::Iseof => is_eof(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Implements `tokenize.tokenize(readline)` for string input.
fn tokenize(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let readline = parse_named_required_arg(args, "tokenize", "readline", heap, interns)?;
    let source = extract_source_text(readline, heap, interns)?;
    let specs = tokenize_source(&source, true);
    specs_to_iterator(specs, heap, interns)
}

/// Implements `tokenize.generate_tokens(readline)` for string input.
fn generate_tokens(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let readline = parse_named_required_arg(args, "generate_tokens", "readline", heap, interns)?;
    let source = extract_source_text(readline, heap, interns)?;
    let specs = tokenize_source(&source, false);
    specs_to_iterator(specs, heap, interns)
}

/// Implements `tokenize.detect_encoding(readline)` for string input.
fn detect_encoding(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let readline = parse_named_required_arg(args, "detect_encoding", "readline", heap, interns)?;
    let source = extract_source_text(readline, heap, interns)?;

    let mut consumed_lines = Vec::new();
    for line in split_lines_with_endings(&source).into_iter().take(2) {
        consumed_lines.push(line.to_owned());
    }

    let encoding = detect_declared_encoding(&consumed_lines).unwrap_or("utf-8");
    let encoding_id = heap.allocate(HeapData::Str(Str::from(encoding)))?;

    let mut line_values = Vec::with_capacity(consumed_lines.len());
    for line in consumed_lines {
        let line_id = heap.allocate(HeapData::Str(Str::from(line)))?;
        line_values.push(Value::Ref(line_id));
    }
    let lines_id = heap.allocate(HeapData::List(List::new(line_values)))?;

    Ok(allocate_tuple(
        smallvec![Value::Ref(encoding_id), Value::Ref(lines_id)],
        heap,
    )?)
}

/// Implements `tokenize.open(filename)`.
///
/// Ouros cannot expose host filesystem I/O from sandboxed stdlib modules.
fn open_sandboxed(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let filename = parse_named_required_arg(args, "open", "filename", heap, interns)?;
    filename.drop_with_heap(heap);
    let _ = interns;
    Err(SimpleException::new_msg(ExcType::OSError, "tokenize.open() is unavailable in Ouros sandbox").into())
}

/// Implements `tokenize.untokenize(iterable)`.
fn untokenize(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let iterable = parse_named_required_arg(args, "untokenize", "iterable", heap, interns)?;
    let iter = OurosIter::new(iterable, heap, interns)?;
    defer_drop_mut!(iter, heap);

    let mut output = String::new();
    let mut current_row = 1usize;
    let mut current_col = 0usize;

    while let Some(item) = iter.for_next(heap, interns)? {
        defer_drop!(item, heap);
        let token = parse_untokenize_item(item, heap, interns)?;

        if token.token_type == TOK_ENCODING || token.token_type == TOK_ENDMARKER {
            continue;
        }

        if let Some((row, col)) = token.start {
            while current_row < row {
                output.push('\n');
                current_row += 1;
                current_col = 0;
            }
            while current_col < col {
                output.push(' ');
                current_col += 1;
            }
        }

        output.push_str(&token.text);
        for ch in token.text.chars() {
            if ch == '\n' {
                current_row += 1;
                current_col = 0;
            } else {
                current_col += 1;
            }
        }
    }

    let out_id = heap.allocate(HeapData::Str(Str::from(output)))?;
    Ok(Value::Ref(out_id))
}

/// Implements `tokenize.ISTERMINAL(x)`.
fn is_terminal(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = parse_x_arg(args, "ISTERMINAL", heap, interns)?;
    Ok(Value::Bool(value < 256))
}

/// Implements `tokenize.ISNONTERMINAL(x)`.
fn is_nonterminal(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = parse_x_arg(args, "ISNONTERMINAL", heap, interns)?;
    Ok(Value::Bool(value >= 256))
}

/// Implements `tokenize.ISEOF(x)`.
fn is_eof(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = parse_x_arg(args, "ISEOF", heap, interns)?;
    Ok(Value::Bool(value == 0))
}

/// Parses a required positional-or-keyword argument by name.
fn parse_named_required_arg(
    args: ArgValues,
    function_name: &str,
    arg_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let mut value = positional.next();

    if positional.next().is_some() {
        value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(function_name, 1, 2, 0));
    }

    for (key, kw_value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            kw_value.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name != arg_name {
            kw_value.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
        }

        if value.is_some() {
            kw_value.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg(function_name, arg_name));
        }

        value = Some(kw_value);
    }

    value.ok_or_else(|| ExcType::type_error_missing_positional_with_names(function_name, &[arg_name]))
}

/// Parses shared `x` argument used by `ISEOF`/`ISTERMINAL`/`ISNONTERMINAL`.
fn parse_x_arg(
    args: ArgValues,
    function_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    let value = parse_named_required_arg(args, function_name, "x", heap, interns)?;
    let out = value.as_int(heap);
    value.drop_with_heap(heap);
    out
}

/// Converts supported `readline` values to source text.
///
/// This implementation accepts `str`, `bytes`, and `bytearray` values.
fn extract_source_text(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let text = match &value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => std::str::from_utf8(bytes.as_slice())
                .map(std::borrow::ToOwned::to_owned)
                .map_err(|_| ExcType::unicode_decode_error_invalid_utf8()),
            _ => Err(ExcType::type_error(
                "readline must be str, bytes, or bytearray".to_owned(),
            )),
        },
        Value::InternBytes(bytes_id) => std::str::from_utf8(interns.get_bytes(*bytes_id))
            .map(std::borrow::ToOwned::to_owned)
            .map_err(|_| ExcType::unicode_decode_error_invalid_utf8()),
        _ => Err(ExcType::type_error(
            "readline must be str, bytes, or bytearray".to_owned(),
        )),
    };
    value.drop_with_heap(heap);
    text
}

/// Converts token specs into an iterator of `TokenInfo` namedtuples.
fn specs_to_iterator(
    specs: Vec<TokenSpec>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let mut items = Vec::with_capacity(specs.len());
    for spec in specs {
        items.push(allocate_token_info(spec, heap)?);
    }
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    let iter = OurosIter::new(Value::Ref(list_id), heap, interns)?;
    let iter_id = heap.allocate(HeapData::Iter(iter))?;
    Ok(Value::Ref(iter_id))
}

/// Allocates one `TokenInfo(type, string, start, end, line)` namedtuple.
fn allocate_token_info(spec: TokenSpec, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let string_id = heap.allocate(HeapData::Str(Str::from(spec.text)))?;
    let line_id = heap.allocate(HeapData::Str(Str::from(spec.line)))?;

    let start_row = usize_to_i64(spec.start.0)?;
    let start_col = usize_to_i64(spec.start.1)?;
    let end_row = usize_to_i64(spec.end.0)?;
    let end_col = usize_to_i64(spec.end.1)?;

    let start_tuple = allocate_tuple(smallvec![Value::Int(start_row), Value::Int(start_col)], heap)?;
    let end_tuple = allocate_tuple(smallvec![Value::Int(end_row), Value::Int(end_col)], heap)?;

    let named = NamedTuple::new(
        EitherStr::Heap("TokenInfo".to_owned()),
        token_info_fields(),
        vec![
            Value::Int(spec.token_type),
            Value::Ref(string_id),
            start_tuple,
            end_tuple,
            Value::Ref(line_id),
        ],
    );
    let id = heap.allocate(HeapData::NamedTuple(named))?;
    Ok(Value::Ref(id))
}

/// Returns `TokenInfo` field names.
fn token_info_fields() -> Vec<EitherStr> {
    vec![
        EitherStr::Heap("type".to_owned()),
        EitherStr::Heap("string".to_owned()),
        EitherStr::Heap("start".to_owned()),
        EitherStr::Heap("end".to_owned()),
        EitherStr::Heap("line".to_owned()),
    ]
}

/// Creates the `TokenInfo` namedtuple factory exposed from the module.
fn create_token_info_factory() -> NamedTupleFactory {
    NamedTupleFactory::new_with_options(
        EitherStr::Heap("TokenInfo".to_owned()),
        token_info_fields(),
        Vec::new(),
        EitherStr::Heap("tokenize".to_owned()),
    )
}

/// Creates the custom `tokenize.TokenError` exception class.
fn create_token_error_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let exception_base = heap.builtin_class_id(Type::Exception(ExcType::Exception))?;
    heap.inc_ref(exception_base);

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap("TokenError".to_owned()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        Dict::new(),
        vec![exception_base],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[exception_base], heap, interns).expect("TokenError MRO should be valid");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    heap.with_entry_mut(exception_base, |_, data| {
        let HeapData::ClassObject(base_cls) = data else {
            return Err(ExcType::type_error("TokenError base is not a class".to_owned()));
        };
        base_cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("TokenError base mutation should succeed");

    Ok(class_id)
}

/// Builds `tokenize.tok_name` mapping token ids to token names.
fn create_tok_name_dict(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<Value, ResourceError> {
    let mut dict = Dict::new();
    for &(name, value) in token_constants() {
        if name == "N_TOKENS" || name == "NT_OFFSET" {
            continue;
        }
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        let _ = dict.set(Value::Int(value), Value::Ref(name_id), heap, interns);
    }
    let id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(id))
}

/// Builds `tokenize.EXACT_TOKEN_TYPES` mapping operator text to token ids.
fn create_exact_token_types_dict(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<Value, ResourceError> {
    let mut dict = Dict::new();
    for &(operator, token_type) in exact_token_types() {
        let operator_id = heap.allocate(HeapData::Str(Str::from(operator)))?;
        let _ = dict.set(Value::Ref(operator_id), Value::Int(token_type), heap, interns);
    }
    let id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(id))
}

/// Finds an encoding declaration in the first two source lines.
fn detect_declared_encoding(lines: &[String]) -> Option<&str> {
    for line in lines {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        let coding_index = lower.find("coding")?;
        let mut rest = &trimmed[coding_index + "coding".len()..];
        rest = rest.trim_start();

        let sep = rest.chars().next()?;
        if sep != ':' && sep != '=' {
            continue;
        }
        rest = rest[sep.len_utf8()..].trim_start();

        let mut end = 0usize;
        for (idx, ch) in rest.char_indices() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                end = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if end > 0 {
            return Some(&rest[..end]);
        }
    }
    None
}

/// Splits source into lines while preserving trailing newline characters.
fn split_lines_with_endings(source: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in source.char_indices() {
        if ch == '\n' {
            lines.push(&source[start..=idx]);
            start = idx + 1;
        }
    }
    if start < source.len() {
        lines.push(&source[start..]);
    }
    lines
}

/// Tokenizes source code into `TokenSpec` records.
fn tokenize_source(source: &str, include_encoding: bool) -> Vec<TokenSpec> {
    let mut out = Vec::new();

    if include_encoding {
        out.push(TokenSpec {
            token_type: TOK_ENCODING,
            text: "utf-8".to_owned(),
            start: (0, 0),
            end: (0, 0),
            line: String::new(),
        });
    }

    let lines = split_lines_with_endings(source);
    let mut indent_stack = vec![0usize];

    for (idx, line) in lines.iter().enumerate() {
        let row = idx + 1;
        let (content, has_newline) = if let Some(without) = line.strip_suffix('\n') {
            (without, true)
        } else {
            (*line, false)
        };

        let mut non_ws_byte = 0usize;
        let mut indent_col = 0usize;
        for (byte_idx, ch) in content.char_indices() {
            if matches!(ch, ' ' | '\t' | '\x0c') {
                indent_col += 1;
                non_ws_byte = byte_idx + ch.len_utf8();
            } else {
                break;
            }
        }

        let trimmed = content.trim_start_matches([' ', '\t', '\x0c']);
        let is_blank_or_comment = trimmed.is_empty() || trimmed.starts_with('#');

        if !is_blank_or_comment {
            let current_indent = *indent_stack.last().unwrap_or(&0);
            if indent_col > current_indent {
                indent_stack.push(indent_col);
                out.push(TokenSpec {
                    token_type: TOK_INDENT,
                    text: content[..non_ws_byte].to_owned(),
                    start: (row, 0),
                    end: (row, indent_col),
                    line: (*line).to_owned(),
                });
            } else if indent_col < current_indent {
                while indent_stack.len() > 1 && indent_col < *indent_stack.last().unwrap_or(&0) {
                    indent_stack.pop();
                    out.push(TokenSpec {
                        token_type: TOK_DEDENT,
                        text: String::new(),
                        start: (row, indent_col),
                        end: (row, indent_col),
                        line: (*line).to_owned(),
                    });
                }
            }
        }

        let mut i = non_ws_byte;
        let mut saw_code = false;
        while i < content.len() {
            let Some(ch) = content[i..].chars().next() else {
                break;
            };

            if matches!(ch, ' ' | '\t' | '\x0c' | '\r') {
                i += ch.len_utf8();
                continue;
            }

            let start_col = content[..i].chars().count();

            if ch == '#' {
                let text = content[i..].to_owned();
                let end_col = content.chars().count();
                out.push(TokenSpec {
                    token_type: TOK_COMMENT,
                    text,
                    start: (row, start_col),
                    end: (row, end_col),
                    line: (*line).to_owned(),
                });
                break;
            }

            if is_ident_start(ch) {
                let mut j = i + ch.len_utf8();
                while j < content.len() {
                    let Some(next_ch) = content[j..].chars().next() else {
                        break;
                    };
                    if is_ident_continue(next_ch) {
                        j += next_ch.len_utf8();
                    } else {
                        break;
                    }
                }
                let text = &content[i..j];
                let end_col = content[..j].chars().count();
                out.push(TokenSpec {
                    token_type: if is_soft_keyword(text) {
                        TOK_SOFT_KEYWORD
                    } else {
                        TOK_NAME
                    },
                    text: text.to_owned(),
                    start: (row, start_col),
                    end: (row, end_col),
                    line: (*line).to_owned(),
                });
                saw_code = true;
                i = j;
                continue;
            }

            if ch.is_ascii_digit() {
                let j = scan_number(content, i);
                let text = &content[i..j];
                let end_col = content[..j].chars().count();
                out.push(TokenSpec {
                    token_type: TOK_NUMBER,
                    text: text.to_owned(),
                    start: (row, start_col),
                    end: (row, end_col),
                    line: (*line).to_owned(),
                });
                saw_code = true;
                i = j;
                continue;
            }

            if ch == '\'' || ch == '"' {
                let (j, token_type) = scan_string(content, i, ch);
                let text = &content[i..j];
                let end_col = content[..j].chars().count();
                out.push(TokenSpec {
                    token_type,
                    text: text.to_owned(),
                    start: (row, start_col),
                    end: (row, end_col),
                    line: (*line).to_owned(),
                });
                saw_code = true;
                i = j;
                continue;
            }

            if let Some((op_len, token_type)) = match_operator(&content[i..]) {
                let j = i + op_len;
                let text = &content[i..j];
                let end_col = content[..j].chars().count();
                out.push(TokenSpec {
                    token_type,
                    text: text.to_owned(),
                    start: (row, start_col),
                    end: (row, end_col),
                    line: (*line).to_owned(),
                });
                saw_code = true;
                i = j;
                continue;
            }

            let j = i + ch.len_utf8();
            let end_col = content[..j].chars().count();
            out.push(TokenSpec {
                token_type: TOK_ERRORTOKEN,
                text: ch.to_string(),
                start: (row, start_col),
                end: (row, end_col),
                line: (*line).to_owned(),
            });
            saw_code = true;
            i = j;
        }

        if has_newline {
            let token_type = if is_blank_or_comment || !saw_code {
                TOK_NL
            } else {
                TOK_NEWLINE
            };
            let col = content.chars().count();
            out.push(TokenSpec {
                token_type,
                text: "\n".to_owned(),
                start: (row, col),
                end: (row, col + 1),
                line: (*line).to_owned(),
            });
        } else if saw_code {
            let col = content.chars().count();
            out.push(TokenSpec {
                token_type: TOK_NEWLINE,
                text: String::new(),
                start: (row, col),
                end: (row, col),
                line: (*line).to_owned(),
            });
        }
    }

    let end_row = if lines.is_empty() { 1 } else { lines.len() + 1 };
    while indent_stack.len() > 1 {
        indent_stack.pop();
        out.push(TokenSpec {
            token_type: TOK_DEDENT,
            text: String::new(),
            start: (end_row, 0),
            end: (end_row, 0),
            line: String::new(),
        });
    }

    out.push(TokenSpec {
        token_type: TOK_ENDMARKER,
        text: String::new(),
        start: (end_row, 0),
        end: (end_row, 0),
        line: String::new(),
    });

    out
}

/// Returns true for soft keywords used by modern Python grammars.
fn is_soft_keyword(name: &str) -> bool {
    matches!(name, "match" | "case" | "type")
}

/// Returns true if a character can start an identifier.
fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

/// Returns true if a character can continue an identifier.
fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// Scans a numeric token starting at `start`.
fn scan_number(content: &str, start: usize) -> usize {
    let mut i = start;
    let bytes = content.as_bytes();

    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }

    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }

    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }

    i
}

/// Scans a single-line string token.
///
/// Returns `(end_index, token_type)` where `token_type` is `STRING` or
/// `ERRORTOKEN` when the delimiter is unterminated.
fn scan_string(content: &str, start: usize, quote: char) -> (usize, i64) {
    let quote_len = quote.len_utf8();
    let mut i = start + quote_len;

    let triple = content[start..].starts_with(if quote == '\'' { "'''" } else { "\"\"\"" });
    if triple {
        i = start + 3 * quote_len;
    }

    let mut escaped = false;
    while i < content.len() {
        let Some(ch) = content[i..].chars().next() else {
            break;
        };

        if escaped {
            escaped = false;
            i += ch.len_utf8();
            continue;
        }

        if ch == '\\' {
            escaped = true;
            i += ch.len_utf8();
            continue;
        }

        if ch == quote {
            if triple {
                let marker = if quote == '\'' { "'''" } else { "\"\"\"" };
                if content[i..].starts_with(marker) {
                    return (i + marker.len(), TOK_STRING);
                }
            } else {
                return (i + ch.len_utf8(), TOK_STRING);
            }
        }

        i += ch.len_utf8();
    }

    (content.len(), TOK_ERRORTOKEN)
}

/// Matches the longest exact operator token at the current byte offset.
fn match_operator(slice: &str) -> Option<(usize, i64)> {
    const MATCH_ORDER: &[&str] = &[
        "**=", "//=", "<<=", ">>=", "==", "!=", "<=", ">=", "**", "//", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=",
        "|=", "^=", "@=", "->", "...", ":=", "!", "(", ")", "[", "]", ":", ",", ";", "+", "-", "*", "/", "|", "&", "<",
        ">", "=", ".", "%", "{", "}", "~", "^", "@",
    ];

    for operator in MATCH_ORDER {
        if slice.starts_with(operator)
            && let Some((_, token_type)) = exact_token_types().iter().find(|(text, _)| text == operator)
        {
            return Some((operator.len(), *token_type));
        }
    }

    None
}

/// Parsed untokenize item with optional positional metadata.
struct UntokenizeItem {
    token_type: i64,
    text: String,
    start: Option<(usize, usize)>,
}

/// Parses one item accepted by `untokenize`.
fn parse_untokenize_item(
    item: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<UntokenizeItem> {
    let parts = sequence_parts(item, heap)?;
    if parts.len() < 2 {
        return Err(ExcType::type_error(
            "untokenize() expects token entries with at least 2 fields".to_owned(),
        ));
    }

    let token_type = parts[0].as_int(heap)?;
    let text = value_as_string(&parts[1], heap, interns)?;
    let start = if parts.len() >= 3 {
        parse_position_tuple(&parts[2], heap)
    } else {
        None
    };

    Ok(UntokenizeItem {
        token_type,
        text,
        start,
    })
}

/// Returns sequence elements for tuple/list/namedtuple token entries.
fn sequence_parts<'a>(item: &'a Value, heap: &'a Heap<impl ResourceTracker>) -> RunResult<&'a [Value]> {
    match item {
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => Ok(tuple.as_vec().as_slice()),
            HeapData::List(list) => Ok(list.as_vec().as_slice()),
            HeapData::NamedTuple(named) => Ok(named.as_vec().as_slice()),
            _ => Err(ExcType::type_error(
                "untokenize() expected each token to be a tuple/list/TokenInfo".to_owned(),
            )),
        },
        _ => Err(ExcType::type_error(
            "untokenize() expected each token to be a tuple/list/TokenInfo".to_owned(),
        )),
    }
}

/// Parses a `(row, col)` tuple if present.
fn parse_position_tuple(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<(usize, usize)> {
    let Value::Ref(id) = value else {
        return None;
    };

    let parts = match heap.get(*id) {
        HeapData::Tuple(tuple) => tuple.as_vec().as_slice(),
        HeapData::List(list) => list.as_vec().as_slice(),
        _ => return None,
    };

    if parts.len() != 2 {
        return None;
    }

    let row = usize::try_from(parts[0].as_int(heap).ok()?).ok()?;
    let col = usize::try_from(parts[1].as_int(heap).ok()?).ok()?;
    Some((row, col))
}

/// Converts a token payload value into string text.
fn value_as_string(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => {
                Ok(String::from_utf8_lossy(bytes.as_slice()).to_string())
            }
            _ => Err(ExcType::type_error("token text must be str or bytes".to_owned())),
        },
        Value::InternBytes(bytes_id) => Ok(String::from_utf8_lossy(interns.get_bytes(*bytes_id)).to_string()),
        _ => Err(ExcType::type_error("token text must be str or bytes".to_owned())),
    }
}

/// Converts a `usize` into `i64` with overflow checking.
fn usize_to_i64(value: usize) -> RunResult<i64> {
    i64::try_from(value).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "int too large").into())
}
