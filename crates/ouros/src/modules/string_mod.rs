//! Implementation of the `string` module.
//!
//! Provides string constants and utility functions from Python's `string` module:
//! - `ascii_lowercase`: 'abcdefghijklmnopqrstuvwxyz'
//! - `ascii_uppercase`: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
//! - `ascii_letters`: ascii_lowercase + ascii_uppercase
//! - `digits`: '0123456789'
//! - `hexdigits`: '0123456789abcdefABCDEF'
//! - `octdigits`: '01234567'
//! - `punctuation`: punctuation characters
//! - `whitespace`: ' \t\n\r\x0b\x0c'
//! - `printable`: digits + letters + punctuation + whitespace
//! - `capwords(s, sep=None)`: split by sep (default whitespace), capitalize each word, rejoin

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, ClassObject, Dict, Module, PyTrait, Str, Type, compute_c3_mro},
    value::{EitherStr, Value},
};

/// String module functions.
///
/// Each variant maps to a callable function in Python's `string` module.
/// Currently only `capwords` is supported as the other members are constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum StringModFunctions {
    Capwords,
    #[strum(serialize = "Formatter")]
    Formatter,
    #[strum(serialize = "Template")]
    Template,
}

/// Creates the `string` module and allocates it on the heap.
///
/// Sets up all string constants and callable functions as module attributes.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::StringMod);

    // ascii_lowercase: 'abcdefghijklmnopqrstuvwxyz'
    let s = Str::from("abcdefghijklmnopqrstuvwxyz");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrAsciiLowercase, Value::Ref(id), heap, interns);

    // ascii_uppercase: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
    let s = Str::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrAsciiUppercase, Value::Ref(id), heap, interns);

    // ascii_letters: ascii_lowercase + ascii_uppercase
    let s = Str::from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrAsciiLetters, Value::Ref(id), heap, interns);

    // digits: '0123456789'
    let s = Str::from("0123456789");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrDigits, Value::Ref(id), heap, interns);

    // hexdigits: '0123456789abcdefABCDEF'
    let s = Str::from("0123456789abcdefABCDEF");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrHexdigits, Value::Ref(id), heap, interns);

    // octdigits: '01234567'
    let s = Str::from("01234567");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrOctdigits, Value::Ref(id), heap, interns);

    // punctuation: punctuation characters
    let s = Str::from("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrPunctuation, Value::Ref(id), heap, interns);

    // whitespace: ' \t\n\r\x0b\x0c'
    let s = Str::from(" \t\n\r\x0b\x0c");
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrWhitespace, Value::Ref(id), heap, interns);

    // printable: digits + letters + punctuation + whitespace
    let s = Str::from(
        "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ \t\n\r\x0b\x0c",
    );
    let id = heap.allocate(HeapData::Str(s))?;
    module.set_attr(StaticStrings::StrPrintable, Value::Ref(id), heap, interns);

    // capwords(s, sep=None) - callable function
    module.set_attr(
        StaticStrings::StrCapwords,
        Value::ModuleFunction(ModuleFunctions::StringMod(StringModFunctions::Capwords)),
        heap,
        interns,
    );

    // string.Formatter() constructor
    module.set_attr(
        StaticStrings::StrFormatter,
        Value::ModuleFunction(ModuleFunctions::StringMod(StringModFunctions::Formatter)),
        heap,
        interns,
    );

    // string.Template class
    let template_class_id = create_template_class(heap, interns)?;
    module.set_attr(StaticStrings::StrTemplate, Value::Ref(template_class_id), heap, interns);

    heap.allocate(HeapData::Module(module))
}

/// Creates the `string.Template` class object with CPython-compatible class attributes.
fn create_template_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    dict_set_str_key(
        &mut namespace,
        "__new__",
        Value::ModuleFunction(ModuleFunctions::StringMod(StringModFunctions::Template)),
        heap,
        interns,
    )?;

    let delimiter_id = heap.allocate(HeapData::Str(Str::from("$")))?;
    dict_set_str_key(&mut namespace, "delimiter", Value::Ref(delimiter_id), heap, interns)?;

    let idpattern_id = heap.allocate(HeapData::Str(Str::from("(?a:[_a-z][_a-z0-9]*)")))?;
    dict_set_str_key(&mut namespace, "idpattern", Value::Ref(idpattern_id), heap, interns)?;

    dict_set_str_key(&mut namespace, "braceidpattern", Value::None, heap, interns)?;

    let flags_id = heap.allocate(HeapData::Str(Str::from("re.IGNORECASE")))?;
    dict_set_str_key(&mut namespace, "flags", Value::Ref(flags_id), heap, interns)?;

    let object_id = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_id);
    let metaclass = Value::Builtin(Builtins::Type(Type::Type));
    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Interned(StaticStrings::StrTemplate.into()),
        class_uid,
        metaclass,
        namespace,
        vec![object_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[object_id], heap, interns)
        .expect("string.Template helper class should always have a valid MRO");
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

/// Inserts a string-keyed class attribute into a namespace dict.
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

/// Dispatches a call to a string module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: StringModFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        StringModFunctions::Capwords => capwords(heap, interns, args),
        StringModFunctions::Formatter => formatter(heap, args),
        StringModFunctions::Template => template(heap, interns, args),
    }
}

/// Implementation of `string.capwords(s, sep=None)`.
///
/// Splits the string `s` by `sep` (or whitespace if sep is None), capitalizes
/// each word (first char upper, rest lower), and rejoins with a single space
/// (or `sep` if provided).
///
/// Matches CPython's `string.capwords` behavior:
/// - When `sep` is None, splits on whitespace and rejoins with single space
/// - When `sep` is provided, splits on that separator and rejoins with the same separator
fn capwords(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // Extract positional and keyword arguments
    let (positional, kwargs) = args.into_parts();
    defer_drop_mut!(positional, heap);
    let kwargs_iter = kwargs.into_iter();
    defer_drop_mut!(kwargs_iter, heap);

    // Get the required text argument
    let Some(text_val) = positional.next() else {
        return Err(ExcType::type_error(
            "string.capwords() missing 1 required positional argument: 's'".to_string(),
        ));
    };

    // Get optional sep from positional args
    let pos_sep = positional.next();

    // Process keyword arguments
    let mut kwarg_sep: Option<Value> = None;
    for (key, value) in kwargs_iter {
        defer_drop!(key, heap);
        let Some(keyword_name) = key.as_either_str(heap) else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        let key_str = keyword_name.as_str(interns);
        if key_str == "sep" {
            kwarg_sep = Some(value);
        } else {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("string.capwords", key_str));
        }
    }

    // Check for sep provided both positionally and as keyword
    if let (Some(pos), Some(kw)) = (&pos_sep, &kwarg_sep) {
        // Use the values to avoid unused variable warnings, then drop
        let _ = pos;
        let _ = kw;
        if let Some(sep) = pos_sep {
            sep.drop_with_heap(heap);
        }
        if let Some(sep) = kwarg_sep {
            sep.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "string.capwords() got multiple values for argument 'sep'".to_string(),
        ));
    }

    // Get text string content
    let text = text_val.py_str(heap, interns).into_owned();
    text_val.drop_with_heap(heap);

    // Get sep from either positional or keyword (keyword takes precedence)
    let sep_opt = kwarg_sep.or(pos_sep);

    let result = match sep_opt {
        Some(sep_v) => {
            defer_drop!(sep_v, heap);
            let sep = sep_v.py_str(heap, interns).into_owned();
            capwords_with_sep(&text, &sep)
        }
        None => capwords_whitespace(&text),
    };

    let str_obj = Str::from(result);
    let id = heap.allocate(HeapData::Str(str_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Capitalizes words split by whitespace and rejoins with a single space.
///
/// Each word has its first character uppercased and the rest lowercased.
/// Multiple whitespace between words is collapsed to a single space.
/// Leading and trailing whitespace is removed.
fn capwords_whitespace(text: &str) -> String {
    text.split_whitespace()
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Capitalizes words split by a specific separator and rejoins with that separator.
///
/// Each word has its first character uppercased and the rest lowercased.
/// Unlike the whitespace variant, this preserves empty segments between separators.
fn capwords_with_sep(text: &str, sep: &str) -> String {
    text.split(sep).map(capitalize_word).collect::<Vec<_>>().join(sep)
}

/// Capitalizes a single word: first char uppercase, rest lowercase.
fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut result = first.to_uppercase().to_string();
            for c in chars {
                for lc in c.to_lowercase() {
                    result.push(lc);
                }
            }
            result
        }
    }
}

/// Implementation of `string.Formatter()`.
///
/// Returns a lightweight formatter object with `.format()` and `.vformat()`.
fn formatter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("string.Formatter", heap)?;
    let object = crate::types::StdlibObject::new_formatter();
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `string.Template.__new__(cls, template)`.
///
/// Returns a lightweight template object configured by class attributes.
fn template(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("Template.__new__ takes no keyword arguments"));
    }
    let Some(class_value) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("Template.__new__", 2, 0));
    };
    let Some(template_value) = positional.next() else {
        class_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("Template.__new__", 2, 1));
    };
    if let Some(extra) = positional.next() {
        class_value.drop_with_heap(heap);
        template_value.drop_with_heap(heap);
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("Template.__new__", 2, 3));
    }
    kwargs.drop_with_heap(heap);

    let (delimiter, idpattern) = template_class_config(&class_value, heap, interns)?;
    class_value.drop_with_heap(heap);

    let template = template_value.py_str(heap, interns).into_owned();
    template_value.drop_with_heap(heap);

    let object = crate::types::StdlibObject::new_template(template, delimiter, idpattern);
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Extracts template behavior configuration from class attributes.
fn template_class_config(
    class_value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(String, String)> {
    let class_id = match class_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::ClassObject(_)) => *id,
        _ => {
            return Err(ExcType::type_error(
                "Template.__new__(cls, template): cls must be a class",
            ));
        }
    };

    let delimiter = class_attr_string_or_default(class_id, "delimiter", "$", heap, interns)?;
    let idpattern = class_attr_string_or_default(class_id, "idpattern", "(?a:[_a-z][_a-z0-9]*)", heap, interns)?;

    Ok((delimiter, idpattern))
}

/// Reads a string class attribute from MRO, falling back to the provided default.
fn class_attr_string_or_default(
    class_id: HeapId,
    name: &str,
    default: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    let HeapData::ClassObject(cls) = heap.get(class_id) else {
        return Err(ExcType::type_error(
            "Template.__new__(cls, template): cls must be a class",
        ));
    };

    if let Some(value) = cls.namespace().get_by_str(name, heap, interns) {
        return class_attr_value_to_string(value, name, heap, interns);
    }
    for &base_id in &cls.mro()[1..] {
        if let HeapData::ClassObject(base_cls) = heap.get(base_id)
            && let Some(value) = base_cls.namespace().get_by_str(name, heap, interns)
        {
            return class_attr_value_to_string(value, name, heap, interns);
        }
    }

    Ok(default.to_owned())
}

/// Converts a class attribute value to a string.
fn class_attr_value_to_string(
    value: &Value,
    attr_name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<String> {
    match value {
        Value::InternString(string_id) => Ok(interns.get_str(*string_id).to_owned()),
        Value::Ref(value_id) => match heap.get(*value_id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "Template attribute '{attr_name}' must be str"
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "Template attribute '{attr_name}' must be str"
        ))),
    }
}
