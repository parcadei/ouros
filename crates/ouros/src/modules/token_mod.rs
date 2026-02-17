//! Implementation of the `token` module.
//!
//! This module exposes Python token numeric constants and helper predicates used
//! by higher-level tooling (for example `tokenize`).

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, Module, Str},
    value::Value,
};

/// `token` module function entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum TokenFunctions {
    /// Implements `token.ISTERMINAL(x)`.
    Isterminal,
    /// Implements `token.ISNONTERMINAL(x)`.
    Isnonterminal,
    /// Implements `token.ISEOF(x)`.
    Iseof,
}

/// Ordered token `(name, value)` pairs used for constants and `tok_name`.
const TOKEN_CONSTANTS: &[(&str, i64)] = &[
    ("ENDMARKER", 0),
    ("NAME", 1),
    ("NUMBER", 2),
    ("STRING", 3),
    ("NEWLINE", 4),
    ("INDENT", 5),
    ("DEDENT", 6),
    ("LPAR", 7),
    ("RPAR", 8),
    ("LSQB", 9),
    ("RSQB", 10),
    ("COLON", 11),
    ("COMMA", 12),
    ("SEMI", 13),
    ("PLUS", 14),
    ("MINUS", 15),
    ("STAR", 16),
    ("SLASH", 17),
    ("VBAR", 18),
    ("AMPER", 19),
    ("LESS", 20),
    ("GREATER", 21),
    ("EQUAL", 22),
    ("DOT", 23),
    ("PERCENT", 24),
    ("LBRACE", 25),
    ("RBRACE", 26),
    ("EQEQUAL", 27),
    ("NOTEQUAL", 28),
    ("LESSEQUAL", 29),
    ("GREATEREQUAL", 30),
    ("TILDE", 31),
    ("CIRCUMFLEX", 32),
    ("LEFTSHIFT", 33),
    ("RIGHTSHIFT", 34),
    ("DOUBLESTAR", 35),
    ("PLUSEQUAL", 36),
    ("MINEQUAL", 37),
    ("STAREQUAL", 38),
    ("SLASHEQUAL", 39),
    ("PERCENTEQUAL", 40),
    ("AMPEREQUAL", 41),
    ("VBAREQUAL", 42),
    ("CIRCUMFLEXEQUAL", 43),
    ("LEFTSHIFTEQUAL", 44),
    ("RIGHTSHIFTEQUAL", 45),
    ("DOUBLESTAREQUAL", 46),
    ("DOUBLESLASH", 47),
    ("DOUBLESLASHEQUAL", 48),
    ("AT", 49),
    ("ATEQUAL", 50),
    ("RARROW", 51),
    ("ELLIPSIS", 52),
    ("COLONEQUAL", 53),
    ("EXCLAMATION", 54),
    ("OP", 55),
    ("TYPE_IGNORE", 56),
    ("TYPE_COMMENT", 57),
    ("SOFT_KEYWORD", 58),
    ("FSTRING_START", 59),
    ("FSTRING_MIDDLE", 60),
    ("FSTRING_END", 61),
    ("TSTRING_START", 62),
    ("TSTRING_MIDDLE", 63),
    ("TSTRING_END", 64),
    ("COMMENT", 65),
    ("NL", 66),
    ("ERRORTOKEN", 67),
    ("ENCODING", 68),
    ("N_TOKENS", 69),
    ("NT_OFFSET", 256),
];

/// Operator mapping used by `token.EXACT_TOKEN_TYPES`.
const EXACT_TOKEN_TYPES: &[(&str, i64)] = &[
    ("(", 7),
    (")", 8),
    ("[", 9),
    ("]", 10),
    (":", 11),
    (",", 12),
    (";", 13),
    ("+", 14),
    ("-", 15),
    ("*", 16),
    ("/", 17),
    ("|", 18),
    ("&", 19),
    ("<", 20),
    (">", 21),
    ("=", 22),
    (".", 23),
    ("%", 24),
    ("{", 25),
    ("}", 26),
    ("==", 27),
    ("!=", 28),
    ("<=", 29),
    (">=", 30),
    ("~", 31),
    ("^", 32),
    ("<<", 33),
    (">>", 34),
    ("**", 35),
    ("+=", 36),
    ("-=", 37),
    ("*=", 38),
    ("/=", 39),
    ("%=", 40),
    ("&=", 41),
    ("|=", 42),
    ("^=", 43),
    ("<<=", 44),
    (">>=", 45),
    ("**=", 46),
    ("//", 47),
    ("//=", 48),
    ("@", 49),
    ("@=", 50),
    ("->", 51),
    ("...", 52),
    (":=", 53),
    ("!", 54),
];

/// Creates the `token` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::TokenMod);

    for &(name, value) in TOKEN_CONSTANTS {
        module.set_attr_text(name, Value::Int(value), heap, interns)?;
    }

    module.set_attr_text(
        "ISTERMINAL",
        Value::ModuleFunction(ModuleFunctions::Token(TokenFunctions::Isterminal)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ISNONTERMINAL",
        Value::ModuleFunction(ModuleFunctions::Token(TokenFunctions::Isnonterminal)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "ISEOF",
        Value::ModuleFunction(ModuleFunctions::Token(TokenFunctions::Iseof)),
        heap,
        interns,
    )?;

    let tok_name = create_tok_name_dict(heap, interns)?;
    module.set_attr_text("tok_name", tok_name, heap, interns)?;

    let exact = create_exact_token_types_dict(heap, interns)?;
    module.set_attr_text("EXACT_TOKEN_TYPES", exact, heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches `token` module function calls.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: TokenFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        TokenFunctions::Isterminal => is_terminal(heap, interns, args)?,
        TokenFunctions::Isnonterminal => is_nonterminal(heap, interns, args)?,
        TokenFunctions::Iseof => is_eof(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(result))
}

/// Implements `token.ISTERMINAL(x)`.
fn is_terminal(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = parse_x_arg(args, "ISTERMINAL", heap, interns)?;
    Ok(Value::Bool(value < 256))
}

/// Implements `token.ISNONTERMINAL(x)`.
fn is_nonterminal(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = parse_x_arg(args, "ISNONTERMINAL", heap, interns)?;
    Ok(Value::Bool(value >= 256))
}

/// Implements `token.ISEOF(x)`.
fn is_eof(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = parse_x_arg(args, "ISEOF", heap, interns)?;
    Ok(Value::Bool(value == 0))
}

/// Parses the shared `x` argument used by the token predicate helpers.
fn parse_x_arg(
    args: ArgValues,
    function_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let mut x = positional.next();

    if positional.next().is_some() {
        x.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(function_name, 1, 2, 0));
    }

    for (key, value) in kwargs {
        let Some(name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            x.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        if key_name != "x" {
            value.drop_with_heap(heap);
            x.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
        }
        if x.is_some() {
            value.drop_with_heap(heap);
            x.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg(function_name, "x"));
        }
        x = Some(value);
    }

    let Some(x_value) = x else {
        return Err(ExcType::type_error_missing_positional_with_names(function_name, &["x"]));
    };

    let result = x_value.as_int(heap);
    x_value.drop_with_heap(heap);
    result
}

/// Builds `token.tok_name` mapping token ids to token names.
fn create_tok_name_dict(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<Value, ResourceError> {
    let mut dict = Dict::new();
    for &(name, value) in TOKEN_CONSTANTS {
        if name == "N_TOKENS" || name == "NT_OFFSET" {
            continue;
        }
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        let _ = dict.set(Value::Int(value), Value::Ref(name_id), heap, interns);
    }
    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(dict_id))
}

/// Builds `token.EXACT_TOKEN_TYPES` mapping operator spellings to token ids.
fn create_exact_token_types_dict(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<Value, ResourceError> {
    let mut dict = Dict::new();
    for &(operator, value) in EXACT_TOKEN_TYPES {
        let key_id = heap.allocate(HeapData::Str(Str::from(operator)))?;
        let _ = dict.set(Value::Ref(key_id), Value::Int(value), heap, interns);
    }
    let dict_id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(dict_id))
}

/// Returns the operator map used by `token` and `tokenize` implementations.
#[must_use]
pub(crate) fn exact_token_types() -> &'static [(&'static str, i64)] {
    EXACT_TOKEN_TYPES
}

/// Returns the token constants used by `token` and `tokenize` implementations.
#[must_use]
pub(crate) fn token_constants() -> &'static [(&'static str, i64)] {
    TOKEN_CONSTANTS
}
