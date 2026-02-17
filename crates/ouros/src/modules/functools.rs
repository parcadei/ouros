//! Implementation of the `functools` module.
//!
//! Provides higher-order functions and operations on callable objects:
//! - `reduce(function, iterable, initial=None)`: Apply function cumulatively to items
//! - `partial(func, *args, **kwargs)`: Return a new callable with pre-applied arguments
//! - `cmp_to_key(mycmp)`: Convert a comparison function to a key function
//! - `lru_cache(maxsize=128)` / `cache()`: Return cache wrappers/decorators
//! - `wraps(wrapped)` / `update_wrapper(wrapper, wrapped)`: Function metadata wrappers
//! - `get_cache_token()`: Return the singledispatch cache token
//! - `recursive_repr(fillvalue='...')`: Return a repr recursion-guard decorator
//! - `total_ordering(cls)`: Class decorator entrypoint
//!
//! `reduce()` returns `AttrCallResult::ReduceCall` to delegate the actual function
//! calling to the VM, which has access to frame management.
//!
//! `partial()` stores a function together with pre-applied positional and keyword
//! arguments. When the resulting partial object is called, the pre-applied args are
//! prepended to the call-site args before forwarding to the wrapped function.
//!
//! `cmp_to_key()` wraps a comparison function into a key object whose ordering
//! is derived by calling the comparison function. This allows old-style comparison
//! functions to be used with `sorted()` and `list.sort()`.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Module, OurosIter, PyTrait, allocate_tuple},
    value::Value,
};

/// Functools module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum FunctoolsFunctions {
    Reduce,
    Partial,
    #[strum(serialize = "cmp_to_key")]
    CmpToKey,
    #[strum(serialize = "lru_cache")]
    LruCache,
    Cache,
    #[strum(serialize = "cached_property")]
    CachedProperty,
    Singledispatch,
    Singledispatchmethod,
    Partialmethod,
    Wraps,
    #[strum(serialize = "update_wrapper")]
    UpdateWrapper,
    #[strum(serialize = "get_cache_token")]
    GetCacheToken,
    #[strum(serialize = "recursive_repr")]
    RecursiveRepr,
    #[strum(serialize = "recursive_repr._decorator")]
    RecursiveReprDecorator,
    #[strum(serialize = "total_ordering")]
    TotalOrdering,
}

/// Creates the `functools` module and allocates it on the heap.
///
/// Sets up functools callables and constants used by wrapper helpers.
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
    let mut module = Module::new(StaticStrings::Functools);

    // Wrapper metadata defaults (mirrors CPython constant names/contents).
    let wrapper_assignments = allocate_tuple(
        smallvec::smallvec![
            StaticStrings::DunderModule.into(),
            StaticStrings::DunderName.into(),
            StaticStrings::DunderQualname.into(),
            StaticStrings::DunderDoc.into(),
            StaticStrings::DunderAnnotate.into(),
            StaticStrings::DunderTypeParams.into(),
        ],
        heap,
    )?;
    module.set_attr(StaticStrings::FtWrapperAssignments, wrapper_assignments, heap, interns);

    let wrapper_updates = allocate_tuple(smallvec::smallvec![StaticStrings::DunderDictAttr.into()], heap)?;
    module.set_attr(StaticStrings::FtWrapperUpdates, wrapper_updates, heap, interns);

    macro_rules! reg {
        ($name:expr, $func:ident) => {
            module.set_attr(
                $name,
                Value::ModuleFunction(ModuleFunctions::Functools(FunctoolsFunctions::$func)),
                heap,
                interns,
            );
        };
    }

    // Core functools callables.
    reg!(StaticStrings::FtReduce, Reduce);
    reg!(StaticStrings::FtPartial, Partial);
    reg!(StaticStrings::FtCmpToKey, CmpToKey);
    reg!(StaticStrings::FtLruCache, LruCache);
    reg!(StaticStrings::FtCache, Cache);
    reg!(StaticStrings::FtCachedProperty, CachedProperty);
    reg!(StaticStrings::FtSingledispatch, Singledispatch);
    reg!(StaticStrings::FtSingledispatchmethod, Singledispatchmethod);
    reg!(StaticStrings::FtPartialmethod, Partialmethod);
    reg!(StaticStrings::FtWraps, Wraps);
    reg!(StaticStrings::FtUpdateWrapper, UpdateWrapper);
    reg!(StaticStrings::FtGetCacheToken, GetCacheToken);
    reg!(StaticStrings::FtRecursiveRepr, RecursiveRepr);
    reg!(StaticStrings::FtTotalOrdering, TotalOrdering);

    let placeholder_id = heap.allocate(HeapData::Placeholder(crate::types::Placeholder))?;
    module.set_attr(StaticStrings::FtPlaceholder, Value::Ref(placeholder_id), heap, interns);

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a functools module function.
///
/// For `reduce()`, returns `AttrCallResult::ReduceCall` to delegate function
/// calling to the VM (which has access to frame management for user-defined functions).
/// For `partial()`, allocates a `Partial` object on the heap.
/// For `cmp_to_key()`, allocates a `CmpToKey` wrapper on the heap.
/// For wrapper/cache helpers, allocates their corresponding functools heap wrappers.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: FunctoolsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        FunctoolsFunctions::Reduce => reduce(heap, interns, args),
        FunctoolsFunctions::Partial => partial(heap, args),
        FunctoolsFunctions::CmpToKey => cmp_to_key(heap, args),
        FunctoolsFunctions::LruCache => lru_cache(heap, interns, args),
        FunctoolsFunctions::Cache => cache(heap, interns, args),
        FunctoolsFunctions::CachedProperty => cached_property(heap, interns, args),
        FunctoolsFunctions::Singledispatch => singledispatch(heap, args),
        FunctoolsFunctions::Singledispatchmethod => singledispatchmethod(heap, args),
        FunctoolsFunctions::Partialmethod => partialmethod(heap, args),
        FunctoolsFunctions::Wraps => wraps(heap, interns, args),
        FunctoolsFunctions::UpdateWrapper => update_wrapper(heap, interns, args),
        FunctoolsFunctions::GetCacheToken => get_cache_token(heap, args),
        FunctoolsFunctions::RecursiveRepr => recursive_repr(heap, args),
        FunctoolsFunctions::RecursiveReprDecorator => recursive_repr_decorator(heap, args),
        FunctoolsFunctions::TotalOrdering => total_ordering(heap, interns, args),
    }
}

/// Implementation of `functools.cached_property(func)`.
fn cached_property(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("functools.cached_property", heap)?;
    let attr_name = if let Value::DefFunction(function_id) = &func {
        Some(
            interns
                .get_str(interns.get_function(*function_id).name.name_id)
                .to_owned(),
        )
    } else {
        None
    };
    let descriptor = crate::types::CachedProperty::new(func, attr_name);
    let id = heap.allocate(HeapData::CachedProperty(descriptor))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.singledispatch(func)`.
fn singledispatch(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("functools.singledispatch", heap)?;
    let dispatcher = crate::types::SingleDispatch::new(func, 0);
    let id = heap.allocate(HeapData::SingleDispatch(dispatcher))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.singledispatchmethod(func)`.
fn singledispatchmethod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("functools.singledispatchmethod", heap)?;
    let dispatcher = crate::types::SingleDispatch::new(func, 1);
    let dispatcher_id = heap.allocate(HeapData::SingleDispatch(dispatcher))?;
    heap.inc_ref(dispatcher_id);
    let method = crate::types::SingleDispatchMethod::new(Value::Ref(dispatcher_id));
    let method_id = heap.allocate(HeapData::SingleDispatchMethod(method))?;
    Ok(AttrCallResult::Value(Value::Ref(method_id)))
}

/// Implementation of `functools.partialmethod(func, *args, **kwargs)`.
fn partialmethod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(func) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::TypeError, "partialmethod() requires at least one argument").into(),
        );
    };

    let partial_args: Vec<Value> = positional.collect();
    let partial_kwargs: Vec<(Value, Value)> = kwargs.into_iter().collect();
    let descriptor = crate::types::PartialMethod::new(func, partial_args, partial_kwargs);
    let id = heap.allocate(HeapData::PartialMethod(descriptor))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.get_cache_token()`.
fn get_cache_token(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("get_cache_token", heap)?;
    Ok(AttrCallResult::Value(Value::Int(0)))
}

/// Implementation of `functools.recursive_repr(fillvalue='...')`.
///
/// Ouros currently returns a simple identity decorator. This preserves behavior
/// for non-recursive repr paths used by parity tests.
fn recursive_repr(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let fillvalue = args.get_zero_one_arg("functools.recursive_repr", heap)?;
    if let Some(fillvalue) = fillvalue {
        fillvalue.drop_with_heap(heap);
    }
    Ok(AttrCallResult::Value(Value::ModuleFunction(
        ModuleFunctions::Functools(FunctoolsFunctions::RecursiveReprDecorator),
    )))
}

/// Identity decorator used by `recursive_repr()`.
fn recursive_repr_decorator(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("functools.recursive_repr.<decorator>", heap)?;
    Ok(AttrCallResult::Value(func))
}

/// Implementation of `functools.lru_cache(maxsize=128)`.
///
/// This wiring layer constructs the existing `LruCache` heap type either as:
/// - a decorator factory (`func=None`) when called with no arguments or maxsize-like values
/// - a directly wrapped callable (`func=Some(...)`) when called with a non-size positional argument
///
/// Supports keyword arguments `maxsize` and `typed`.
fn lru_cache(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    lru_cache_with_default(heap, interns, args, Some(128), "functools.lru_cache")
}

/// Implementation of `functools.cache(...)`.
///
/// `cache` is an alias for `lru_cache(maxsize=None)`, so it reuses the same
/// constructor logic with an unbounded default cache size.
fn cache(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    lru_cache_with_default(heap, interns, args, None, "functools.cache")
}

/// Shared parser/constructor for `lru_cache`-style wrappers.
fn lru_cache_with_default(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    default_maxsize: Option<usize>,
    name: &str,
) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();

    // Handle keyword arguments (only maxsize is supported)
    let mut maxsize_kwarg: Option<Value> = None;
    let mut typed_kwarg: Option<Value> = None;
    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            drop_arg_pos_iter(positional, heap);
            if let Some(ms) = maxsize_kwarg {
                ms.drop_with_heap(heap);
            }
            if let Some(t) = typed_kwarg {
                t.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let keyword_name = keyword_name.as_str(interns);
        key.drop_with_heap(heap);

        if keyword_name == "maxsize" {
            if maxsize_kwarg.is_some() {
                value.drop_with_heap(heap);
                drop_arg_pos_iter(positional, heap);
                if let Some(t) = typed_kwarg {
                    t.drop_with_heap(heap);
                }
                return Err(ExcType::type_error(
                    "lru_cache() got multiple values for argument 'maxsize'".to_string(),
                ));
            }
            maxsize_kwarg = Some(value);
        } else if keyword_name == "typed" {
            // typed is accepted but ignored for now (we don't implement typed caching)
            typed_kwarg = Some(value);
        } else {
            value.drop_with_heap(heap);
            drop_arg_pos_iter(positional, heap);
            if let Some(ms) = maxsize_kwarg {
                ms.drop_with_heap(heap);
            }
            if let Some(t) = typed_kwarg {
                t.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_unexpected_keyword("lru_cache", keyword_name));
        }
    }

    let mut positional_args: Vec<Value> = positional.collect();
    let arg_count = positional_args.len();

    // Check for conflict between positional and keyword maxsize
    if arg_count > 0 && maxsize_kwarg.is_some() {
        for arg in positional_args {
            arg.drop_with_heap(heap);
        }
        if let Some(ms) = maxsize_kwarg {
            ms.drop_with_heap(heap);
        }
        if let Some(t) = typed_kwarg {
            t.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "lru_cache() got multiple values for argument 'maxsize'".to_string(),
        ));
    }

    if arg_count > 1 {
        for arg in positional_args {
            arg.drop_with_heap(heap);
        }
        if let Some(ms) = maxsize_kwarg {
            ms.drop_with_heap(heap);
        }
        if let Some(t) = typed_kwarg {
            t.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most(name, 1, arg_count));
    }

    let typed = if let Some(t) = typed_kwarg {
        let typed = t.py_bool(heap, interns);
        t.drop_with_heap(heap);
        typed
    } else {
        false
    };

    let (maxsize, func) = if let Some(arg) = positional_args.pop() {
        // Positional argument - could be maxsize or the function
        match arg {
            Value::None => (None, None),
            Value::Bool(b) => (Some(usize::from(b)), None),
            Value::Int(i) => {
                let normalized = if i < 0 {
                    0
                } else {
                    usize::try_from(i).unwrap_or(usize::MAX)
                };
                (Some(normalized), None)
            }
            callable => (default_maxsize, Some(callable)),
        }
    } else if let Some(ms) = maxsize_kwarg {
        // Keyword argument maxsize
        let maxsize_val = match &ms {
            Value::None => None,
            Value::Bool(b) => Some(usize::from(*b)),
            Value::Int(i) => {
                let normalized = if *i < 0 {
                    0
                } else {
                    usize::try_from(*i).unwrap_or(usize::MAX)
                };
                Some(normalized)
            }
            _ => {
                ms.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "lru_cache() maxsize must be an integer or None".to_string(),
                ));
            }
        };
        ms.drop_with_heap(heap);
        (maxsize_val, None)
    } else {
        (default_maxsize, None)
    };

    let wrapper = crate::types::LruCache::new(maxsize, typed, func);
    let id = heap.allocate(HeapData::LruCache(wrapper))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.wraps(wrapped, assigned=..., updated=...)`.
///
/// Returns a decorator-factory object that applies `update_wrapper` with the
/// provided `assigned` and `updated` configuration.
fn wraps(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let positional_args: Vec<Value> = positional.collect();
    let arg_count = positional_args.len();
    if arg_count < 1 {
        for arg in positional_args {
            arg.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("functools.wraps", 1, arg_count));
    }
    if arg_count > 3 {
        for arg in positional_args {
            arg.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("functools.wraps", 3, arg_count));
    }

    let mut iter = positional_args.into_iter();
    let wrapped = iter.next().expect("arg count checked");
    let mut assigned_value = iter.next();
    let mut updated_value = iter.next();

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            wrapped.drop_with_heap(heap);
            if let Some(v) = assigned_value {
                v.drop_with_heap(heap);
            }
            if let Some(v) = updated_value {
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let keyword_name = keyword_name.as_str(interns);
        key.drop_with_heap(heap);

        match keyword_name {
            "assigned" => {
                if assigned_value.is_some() {
                    value.drop_with_heap(heap);
                    wrapped.drop_with_heap(heap);
                    if let Some(v) = assigned_value {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = updated_value {
                        v.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error_multiple_values("wraps", "assigned"));
                }
                assigned_value = Some(value);
            }
            "updated" => {
                if updated_value.is_some() {
                    value.drop_with_heap(heap);
                    wrapped.drop_with_heap(heap);
                    if let Some(v) = assigned_value {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = updated_value {
                        v.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error_multiple_values("wraps", "updated"));
                }
                updated_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                wrapped.drop_with_heap(heap);
                if let Some(v) = assigned_value {
                    v.drop_with_heap(heap);
                }
                if let Some(v) = updated_value {
                    v.drop_with_heap(heap);
                }
                return Err(ExcType::type_error_unexpected_keyword("wraps", keyword_name));
            }
        }
    }

    let assigned = parse_wrapper_attr_list(
        assigned_value,
        &default_wrapper_assignment_ids(),
        "assigned",
        heap,
        interns,
    )?;
    let updated = parse_wrapper_attr_list(updated_value, &default_wrapper_update_ids(), "updated", heap, interns)?;

    let wraps = crate::types::Wraps::new(wrapped, assigned, updated);
    let id = heap.allocate(HeapData::Wraps(wraps))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.update_wrapper(wrapper, wrapped, ...)`.
///
/// This wiring path returns the `FunctionWrapper` heap object that exposes
/// copied metadata and `__wrapped__`.
///
/// Supports `assigned` / `updated` as optional positional or keyword arguments.
fn update_wrapper(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();

    let positional_args: Vec<Value> = positional.collect();
    let arg_count = positional_args.len();
    if arg_count < 2 {
        for arg in positional_args {
            arg.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_least("functools.update_wrapper", 2, arg_count));
    }
    if arg_count > 4 {
        for arg in positional_args {
            arg.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("functools.update_wrapper", 4, arg_count));
    }

    let mut iter = positional_args.into_iter();
    let wrapper = iter.next().expect("arg count checked");
    let wrapped = iter.next().expect("arg count checked");
    let mut assigned_value = iter.next();
    let mut updated_value = iter.next();

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            wrapper.drop_with_heap(heap);
            wrapped.drop_with_heap(heap);
            if let Some(v) = assigned_value {
                v.drop_with_heap(heap);
            }
            if let Some(v) = updated_value {
                v.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let keyword_name = keyword_name.as_str(interns);
        key.drop_with_heap(heap);

        match keyword_name {
            "assigned" => {
                if assigned_value.is_some() {
                    value.drop_with_heap(heap);
                    wrapper.drop_with_heap(heap);
                    wrapped.drop_with_heap(heap);
                    if let Some(v) = assigned_value {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = updated_value {
                        v.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error_multiple_values("update_wrapper", "assigned"));
                }
                assigned_value = Some(value);
            }
            "updated" => {
                if updated_value.is_some() {
                    value.drop_with_heap(heap);
                    wrapper.drop_with_heap(heap);
                    wrapped.drop_with_heap(heap);
                    if let Some(v) = assigned_value {
                        v.drop_with_heap(heap);
                    }
                    if let Some(v) = updated_value {
                        v.drop_with_heap(heap);
                    }
                    return Err(ExcType::type_error_multiple_values("update_wrapper", "updated"));
                }
                updated_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                wrapper.drop_with_heap(heap);
                wrapped.drop_with_heap(heap);
                if let Some(v) = assigned_value {
                    v.drop_with_heap(heap);
                }
                if let Some(v) = updated_value {
                    v.drop_with_heap(heap);
                }
                return Err(ExcType::type_error_unexpected_keyword("update_wrapper", keyword_name));
            }
        }
    }

    let assigned = parse_wrapper_attr_list(
        assigned_value,
        &default_wrapper_assignment_ids(),
        "assigned",
        heap,
        interns,
    )?;
    let updated = parse_wrapper_attr_list(updated_value, &default_wrapper_update_ids(), "updated", heap, interns)?;

    apply_update_wrapper_attrs(&wrapper, &wrapped, &assigned, &updated, heap, interns)?;
    wrapped.drop_with_heap(heap);
    Ok(AttrCallResult::Value(wrapper))
}

/// Applies CPython-compatible metadata updates used by `update_wrapper`/`wraps`.
///
/// Copies the default `WRAPPER_ASSIGNMENTS` attributes from `wrapped` to `wrapper`
/// when present, and always sets `wrapper.__wrapped__ = wrapped`.
pub(crate) fn apply_update_wrapper_attrs(
    wrapper: &Value,
    wrapped: &Value,
    assigned: &[StringId],
    _updated: &[StringId],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    for attr_id in assigned {
        match wrapped.py_getattr(*attr_id, heap, interns) {
            Ok(AttrCallResult::Value(value)) => {
                wrapper.py_set_attr(*attr_id, value, heap, interns)?;
            }
            Ok(other) => {
                // Non-value attribute dispatch doesn't apply for wrapper metadata.
                if let AttrCallResult::Value(_) = other {
                    unreachable!()
                }
            }
            Err(crate::exception_private::RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::AttributeError => {}
            Err(err) => return Err(err),
        }
    }

    let wrapped_copy = wrapped.clone_with_heap(heap);
    wrapper.py_set_attr(StaticStrings::DunderWrapped.into(), wrapped_copy, heap, interns)?;
    Ok(())
}

/// Returns CPython-compatible default `WRAPPER_ASSIGNMENTS`.
fn default_wrapper_assignment_ids() -> Vec<StringId> {
    vec![
        StaticStrings::DunderModule.into(),
        StaticStrings::DunderName.into(),
        StaticStrings::DunderQualname.into(),
        StaticStrings::DunderDoc.into(),
        StaticStrings::DunderAnnotate.into(),
        StaticStrings::DunderTypeParams.into(),
    ]
}

/// Returns CPython-compatible default `WRAPPER_UPDATES`.
fn default_wrapper_update_ids() -> Vec<StringId> {
    vec![StaticStrings::DunderDictAttr.into()]
}

/// Parses `assigned` / `updated` values into attribute name IDs.
fn parse_wrapper_attr_list(
    value: Option<Value>,
    default: &[StringId],
    param_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<StringId>> {
    let Some(value) = value else {
        return Ok(default.to_vec());
    };

    let mut iter = OurosIter::new(value, heap, interns)?;
    let mut names = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => match parse_wrapper_attr_name(item, param_name, heap, interns) {
                Ok(name) => names.push(name),
                Err(err) => {
                    iter.drop_with_heap(heap);
                    return Err(err);
                }
            },
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(names)
}

/// Converts a wrapper metadata attribute name into an interned ID.
fn parse_wrapper_attr_name(
    value: Value,
    param_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<StringId> {
    let Some(name) = value.as_either_str(heap) else {
        value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "functools.update_wrapper() {param_name} entries must be strings"
        )));
    };
    let name = name.as_str(interns);
    value.drop_with_heap(heap);

    let attr_id = match name {
        "__module__" => StaticStrings::DunderModule.into(),
        "__name__" => StaticStrings::DunderName.into(),
        "__qualname__" => StaticStrings::DunderQualname.into(),
        "__doc__" => StaticStrings::DunderDoc.into(),
        "__annotate__" => StaticStrings::DunderAnnotate.into(),
        "__type_params__" => StaticStrings::DunderTypeParams.into(),
        "__dict__" => StaticStrings::DunderDictAttr.into(),
        "__annotations__" => StaticStrings::DunderAnnotations.into(),
        _ => {
            return Err(ExcType::type_error(format!(
                "functools.update_wrapper() unsupported attribute name in {param_name}: {name}"
            )));
        }
    };
    Ok(attr_id)
}

/// Implementation of `functools.total_ordering(cls)`.
///
/// This is wired as a class-decorator entrypoint. Full generated comparison
/// behavior lives in VM callable dispatch; this stage returns the class object.
fn total_ordering(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let cls = args.get_one_arg("functools.total_ordering", heap)?;
    let Value::Ref(class_id) = cls else {
        let type_name = cls.py_type(heap);
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "functools.total_ordering() expected class type, got '{type_name}'"
        )));
    };

    let (has_lt, has_le, has_gt, has_ge) = if let HeapData::ClassObject(class_obj) = heap.get(class_id) {
        let has = |name: StaticStrings| {
            class_obj
                .namespace()
                .get_by_str(interns.get_str(name.into()), heap, interns)
                .is_some()
        };
        (
            has(StaticStrings::DunderLt),
            has(StaticStrings::DunderLe),
            has(StaticStrings::DunderGt),
            has(StaticStrings::DunderGe),
        )
    } else {
        let type_name = cls.py_type(heap);
        cls.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "functools.total_ordering() expected class type, got '{type_name}'"
        )));
    };

    let Some(base) = (if has_lt {
        Some(StaticStrings::DunderLt)
    } else if has_le {
        Some(StaticStrings::DunderLe)
    } else if has_gt {
        Some(StaticStrings::DunderGt)
    } else if has_ge {
        Some(StaticStrings::DunderGe)
    } else {
        None
    }) else {
        cls.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "must define at least one ordering operation: < > <= >=",
        )
        .into());
    };

    let generated: &[(StaticStrings, bool, bool)] = match base {
        StaticStrings::DunderLt => &[
            (StaticStrings::DunderLe, true, true),
            (StaticStrings::DunderGt, true, false),
            (StaticStrings::DunderGe, false, true),
        ],
        StaticStrings::DunderLe => &[
            (StaticStrings::DunderGe, true, false),
            (StaticStrings::DunderLt, true, true),
            (StaticStrings::DunderGt, false, true),
        ],
        StaticStrings::DunderGt => &[
            (StaticStrings::DunderLt, true, false),
            (StaticStrings::DunderGe, true, true),
            (StaticStrings::DunderLe, false, true),
        ],
        StaticStrings::DunderGe => &[
            (StaticStrings::DunderLe, true, false),
            (StaticStrings::DunderGt, true, true),
            (StaticStrings::DunderLt, false, true),
        ],
        _ => unreachable!("base is always an ordering dunder"),
    };

    for (name, swap, negate) in generated {
        let already_defined = match heap.get(class_id) {
            HeapData::ClassObject(class_obj) => class_obj
                .namespace()
                .get_by_str(interns.get_str((*name).into()), heap, interns)
                .is_some(),
            _ => return Err(RunError::internal("total_ordering target class mutated")),
        };
        if already_defined {
            continue;
        }

        let generated_method = crate::types::TotalOrderingMethod::new(base, *swap, *negate);
        let generated_id = heap.allocate(HeapData::TotalOrderingMethod(generated_method))?;
        cls.py_set_attr((*name).into(), Value::Ref(generated_id), heap, interns)?;
    }

    Ok(AttrCallResult::Value(cls))
}

/// Implementation of `functools.partial(func, *args, **kwargs)`.
///
/// Creates a new callable that, when called, prepends the stored positional
/// arguments and merges the stored keyword arguments before forwarding to the
/// wrapped function.
///
/// # Arguments
/// * `func` - A callable to wrap
/// * `*args` - Positional arguments to pre-apply
/// * `**kwargs` - Keyword arguments to pre-apply
///
/// # Returns
/// `AttrCallResult::Value` containing a heap-allocated `Partial` object.
///
/// # Errors
/// Returns `TypeError` if no arguments are provided (func is required).
fn partial(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();

    // First positional arg is the function to wrap
    let Some(func) = positional.next() else {
        drop_arg_pos_iter(positional, heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "partial() requires at least one argument").into());
    };

    // Remaining positional args are the pre-applied args
    let partial_args: Vec<Value> = positional.collect();

    // Collect kwargs into a vec of (key, value) pairs
    let partial_kwargs: Vec<(Value, Value)> = kwargs.into_iter().collect();

    // Allocate the partial object on the heap
    let partial_obj = crate::types::Partial::new(func, partial_args, partial_kwargs);
    let id = heap.allocate(HeapData::Partial(partial_obj))?;

    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.cmp_to_key(mycmp)`.
///
/// Wraps a comparison function into a key object. The key object's `__lt__`
/// method calls `mycmp(self.obj, other.obj) < 0`.
///
/// Since `cmp_to_key` returns an object whose ordering depends on calling the
/// user-provided comparison function, and our VM only supports simple key
/// functions for sorting, this implementation stores the comparison function
/// as a `CmpToKey` wrapper on the heap. The VM's sort logic handles this type.
///
/// # Arguments
/// * `mycmp` - A comparison function that takes two arguments and returns
///   negative, zero, or positive to indicate ordering.
///
/// # Returns
/// `AttrCallResult::Value` containing a heap-allocated `CmpToKey` wrapper.
///
/// # Errors
/// Returns `TypeError` if exactly one argument is not provided.
fn cmp_to_key(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let func = args.get_one_arg("functools.cmp_to_key", heap)?;

    let wrapper = crate::types::CmpToKey::new(func);
    let id = heap.allocate(HeapData::CmpToKey(wrapper))?;

    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `functools.reduce(function, iterable, initial=None)`.
///
/// Parses and validates arguments, collects items from the iterable, determines
/// the initial accumulator, and returns `AttrCallResult::ReduceCall` for the VM
/// to execute. This allows the reduce function to work with all callable types
/// including lambdas and user-defined functions.
///
/// # Arguments
/// * `function` - A callable that takes two arguments
/// * `iterable` - An iterable to reduce
/// * `initial` - Optional initial value (if not provided, first item is used)
///
/// # Returns
/// `AttrCallResult::ReduceCall(function, accumulator, remaining_items)` for VM dispatch,
/// or `AttrCallResult::Value(result)` when the result can be determined immediately
/// (e.g., single element with no initial value).
///
/// # Errors
/// Returns `TypeError` if:
/// - Fewer than 2 arguments are provided
/// - More than 3 arguments are provided
/// - The iterable is empty and no initial value is provided
fn reduce(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    // Parse arguments: reduce(function, iterable, initial=None)
    let (positional, kwargs) = args.into_parts();
    let mut initial_kwarg: Option<Value> = None;
    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            drop_arg_pos_iter(positional, heap);
            if let Some(initial) = initial_kwarg {
                initial.drop_with_heap(heap);
            }
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let keyword_name = keyword_name.as_str(interns);
        key.drop_with_heap(heap);
        if keyword_name != "initial" {
            value.drop_with_heap(heap);
            drop_arg_pos_iter(positional, heap);
            if let Some(initial) = initial_kwarg {
                initial.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_unexpected_keyword("reduce", keyword_name));
        }
        if initial_kwarg.is_some() {
            value.drop_with_heap(heap);
            drop_arg_pos_iter(positional, heap);
            if let Some(initial) = initial_kwarg {
                initial.drop_with_heap(heap);
            }
            return Err(ExcType::type_error(
                "reduce() got multiple values for argument 'initial'".to_string(),
            ));
        }
        initial_kwarg = Some(value);
    }

    // Collect positional arguments
    let args_vec: Vec<Value> = positional.collect();
    let arg_count = args_vec.len();

    if arg_count < 2 {
        for arg in args_vec {
            arg.drop_with_heap(heap);
        }
        if let Some(initial) = initial_kwarg {
            initial.drop_with_heap(heap);
        }
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("reduce() expected at least 2 arguments, got {arg_count}"),
        )
        .into());
    }

    if arg_count > 3 {
        for arg in args_vec {
            arg.drop_with_heap(heap);
        }
        if let Some(initial) = initial_kwarg {
            initial.drop_with_heap(heap);
        }
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("reduce() expected at most 3 arguments, got {arg_count}"),
        )
        .into());
    }

    // Extract function and iterable
    let mut args_iter = args_vec.into_iter();
    let function = args_iter.next().expect("len check ensures at least one value");
    let iterable = args_iter.next().expect("len check ensures at least two values");
    let initial = args_iter.next();
    if initial.is_some() && initial_kwarg.is_some() {
        function.drop_with_heap(heap);
        iterable.drop_with_heap(heap);
        if let Some(initial) = initial {
            initial.drop_with_heap(heap);
        }
        if let Some(initial) = initial_kwarg {
            initial.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "reduce() got multiple values for argument 'initial'".to_string(),
        ));
    }
    let initial = initial.or(initial_kwarg);

    // Create iterator for the iterable and collect all items
    let mut iterator = match OurosIter::new(iterable, heap, interns) {
        Ok(iter) => iter,
        Err(e) => {
            function.drop_with_heap(heap);
            if let Some(initial) = initial {
                initial.drop_with_heap(heap);
            }
            return Err(e);
        }
    };

    let mut items: Vec<Value> = Vec::new();
    loop {
        match iterator.for_next(heap, interns) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(e) => {
                iterator.drop_with_heap(heap);
                function.drop_with_heap(heap);
                if let Some(initial) = initial {
                    initial.drop_with_heap(heap);
                }
                for item in items {
                    item.drop_with_heap(heap);
                }
                return Err(e);
            }
        }
    }
    iterator.drop_with_heap(heap);

    // Determine initial accumulator
    let accumulator = if let Some(init) = initial {
        init
    } else if items.is_empty() {
        function.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::TypeError, "reduce() of empty iterable with no initial value").into(),
        );
    } else {
        items.remove(0)
    };

    // If no items remain, return the accumulator directly
    if items.is_empty() {
        function.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(accumulator));
    }

    // Return ReduceCall for the VM to execute the reduce loop
    Ok(AttrCallResult::ReduceCall(function, accumulator, items))
}

/// Helper to drop remaining items in ArgPosIter.
fn drop_arg_pos_iter(iter: crate::args::ArgPosIter, heap: &mut Heap<impl ResourceTracker>) {
    for item in iter {
        item.drop_with_heap(heap);
    }
}
