//! Implementation of the `weakref` module.
//!
//! Provides weak reference helpers with sandbox-friendly behavior:
//!
//! - `ref(obj)`: Create a real weak reference to `obj` if the object supports it
//!   (has `__weakref__` slot via `__slots__` or an instance `__dict__`).
//! - `proxy(obj)`: Returns a weak proxy-compatible handle.
//! - `getweakrefcount(obj)`: Returns number of currently-live weak refs registered
//!   on the object (for supported instance objects).
//! - `getweakrefs(obj)`: Returns currently-live weak reference objects.
//! - `finalize(obj, func, *args, **kwargs)`: Returns a callable finalizer handle
//!   with `alive`, `detach()`, and `peek()` compatibility.
//! - `WeakSet`, `WeakValueDictionary`, `WeakKeyDictionary`: Compatibility wrappers
//!   over builtin `set`/`dict` constructors.
//! - `WeakMethod`: Alias of `ref` semantics for method objects.

use smallvec::smallvec;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, List, Module, Partial, PyTrait, Set, Type, WeakRef, allocate_tuple},
    value::{EitherStr, Value},
};

/// Weakref module functions.
///
/// Each variant maps to a function in Python's `weakref` module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum WeakrefFunctions {
    /// `ref(obj)` — create a weak reference.
    Ref,
    /// `proxy(obj)` — create a weak proxy-compatible handle.
    Proxy,
    /// `getweakrefcount(obj)` — returns the number of live weakrefs registered on `obj`.
    Getweakrefcount,
    /// `getweakrefs(obj)` — returns live weakref objects registered on `obj`.
    Getweakrefs,
    /// `finalize(obj, func, *args)` — returns a callable partial for manual finalization.
    Finalize,
    /// `WeakSet(iterable=None)` — thin wrapper returning a set.
    #[strum(serialize = "WeakSet")]
    WeakSet,
    /// `WeakValueDictionary(mapping=None)` — thin wrapper returning a dict.
    #[strum(serialize = "WeakValueDictionary")]
    WeakValueDictionary,
    /// `WeakKeyDictionary(mapping=None)` — thin wrapper returning a dict.
    #[strum(serialize = "WeakKeyDictionary")]
    WeakKeyDictionary,
    /// `WeakMethod(method, callback=None)` — weak reference wrapper for methods.
    #[strum(serialize = "WeakMethod")]
    WeakMethod,
    /// Internal helper used by `weakref.finalize` bound-method emulation.
    #[strum(serialize = "__finalize_detach__")]
    FinalizeDetach,
    /// Internal helper used by `weakref.finalize` bound-method emulation.
    #[strum(serialize = "__finalize_peek__")]
    FinalizePeek,
}

/// Creates the `weakref` module and allocates it on the heap.
///
/// The module provides:
/// - `ref(obj)`: Create a weak reference to `obj` if supported
/// - `proxy(obj)`: Create a weak proxy-compatible handle
/// - `getweakrefcount(obj)`: Return live weakref count for supported objects
/// - `getweakrefs(obj)`: Return live weakref objects for supported objects
/// - `finalize(obj, func, *args)`: Return a callable `partial(func, *args, **kwargs)`
///
/// # Returns
/// A `HeapId` pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Weakref);

    // weakref.ref — create a weak reference
    module.set_attr(
        StaticStrings::Ref,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::Ref)),
        heap,
        interns,
    );

    // weakref.proxy — weak proxy-compatible handle
    module.set_attr(
        StaticStrings::WrProxy,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::Proxy)),
        heap,
        interns,
    );

    // weakref.getweakrefcount — count live weakrefs on the object
    module.set_attr(
        StaticStrings::WrGetweakrefcount,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::Getweakrefcount)),
        heap,
        interns,
    );

    // weakref.getweakrefs — return live weakrefs on the object
    module.set_attr(
        StaticStrings::WrGetweakrefs,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::Getweakrefs)),
        heap,
        interns,
    );

    // weakref.finalize — return a callable partial wrapper
    module.set_attr(
        StaticStrings::WrFinalize,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::Finalize)),
        heap,
        interns,
    );

    module.set_attr(
        StaticStrings::WrWeakSet,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::WeakSet)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::WrWeakValueDictionary,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::WeakValueDictionary)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::WrWeakKeyDictionary,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::WeakKeyDictionary)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::WrWeakMethod,
        Value::ModuleFunction(ModuleFunctions::Weakref(WeakrefFunctions::WeakMethod)),
        heap,
        interns,
    );

    // weakref.ReferenceType
    module.set_attr_text(
        "ReferenceType",
        Value::Builtin(Builtins::Type(Type::WeakRef)),
        heap,
        interns,
    )?;
    // weakref.ProxyType — keep as a distinct type object from callable proxies.
    module.set_attr_text("ProxyType", Value::Builtin(Builtins::Type(Type::Object)), heap, interns)?;
    // weakref.CallableProxyType
    module.set_attr_text(
        "CallableProxyType",
        Value::Builtin(Builtins::Type(Type::WeakRef)),
        heap,
        interns,
    )?;
    // weakref.KeyedRef compatibility alias
    module.set_attr_text("KeyedRef", Value::Builtin(Builtins::Type(Type::WeakRef)), heap, interns)?;

    // CPython exposes ProxyTypes as a tuple of proxy classes.
    // Ouros callable proxies reuse `weakref.ReferenceType`.
    let proxy_types = allocate_tuple(
        smallvec![
            Value::Builtin(Builtins::Type(Type::Object)),
            Value::Builtin(Builtins::Type(Type::WeakRef))
        ],
        heap,
    )?;
    module.set_attr(StaticStrings::WrProxyTypes, proxy_types, heap, interns);

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a weakref module function.
///
/// Returns `AttrCallResult::Value` for values computed immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    functions: WeakrefFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match functions {
        WeakrefFunctions::Ref => weakref_ref(heap, interns, args),
        WeakrefFunctions::Proxy => proxy(heap, interns, args),
        WeakrefFunctions::Getweakrefcount => getweakrefcount(heap, args),
        WeakrefFunctions::Getweakrefs => getweakrefs(heap, args),
        WeakrefFunctions::Finalize => finalize(heap, interns, args),
        WeakrefFunctions::WeakSet => weakset(heap, interns, args),
        WeakrefFunctions::WeakValueDictionary => weakvalue_dictionary(heap, interns, args),
        WeakrefFunctions::WeakKeyDictionary => weakkey_dictionary(heap, interns, args),
        WeakrefFunctions::WeakMethod => weakmethod(heap, interns, args),
        WeakrefFunctions::FinalizeDetach => finalize_detach(heap, interns, args),
        WeakrefFunctions::FinalizePeek => finalize_peek(heap, interns, args),
    }
}

/// Implementation of `weakref.ref(obj)`.
///
/// Returns a `weakref.ReferenceType` object that does not keep `obj` alive.
/// The target must be an instance whose class supports weak references (either
/// explicitly via `__slots__` containing `__weakref__` or implicitly via instance
/// `__dict__`).
///
/// # Errors
/// Returns `TypeError` if the object does not support weak references.
fn weakref_ref(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    create_weakref(heap, interns, args, "weakref.ref")
}

/// Implementation of `weakref.proxy(obj)`.
///
/// Returns a weak-reference-backed proxy-compatible object.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn proxy(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    create_weakref(heap, interns, args, "weakref.proxy")
}

/// Implementation of `weakref.getweakrefcount(obj)`.
///
/// Returns the number of currently-live weak references pointing to `obj`
/// when weakref tracking is supported for that object type.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn getweakrefcount(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("weakref.getweakrefcount", heap)?;
    let count = weakref_ids_for_object(&obj, heap).map_or(0, |ids| i64::try_from(ids.len()).unwrap_or(i64::MAX));
    obj.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Int(count)))
}

/// Implementation of `weakref.getweakrefs(obj)`.
///
/// Returns all currently-live weak reference objects pointing to `obj`
/// when weakref tracking is supported for that object type.
///
/// # Errors
/// Returns `TypeError` if not exactly one argument is provided.
fn getweakrefs(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("weakref.getweakrefs", heap)?;
    let values = weakref_ids_for_object(&obj, heap)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|weakref_id| match heap.get_if_live(weakref_id) {
            Some(HeapData::WeakRef(weakref)) if weakref.target().is_some() => {
                // Avoid constructing a temporary `Value::Ref` that would be dropped
                // without `drop_with_heap` under ref-count-panic.
                heap.inc_ref(weakref_id);
                Some(Value::Ref(weakref_id))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    obj.drop_with_heap(heap);
    let list = List::new(values);
    let id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `weakref.finalize(obj, func, *args)`.
///
/// Returns a callable handle that runs `func(*args, **kwargs)` at most once
/// when the referent dies or when called manually.
fn finalize(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let pos_len = positional.len();
    if pos_len < 2 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("weakref.finalize", 2, pos_len));
    }

    let obj = positional.next().expect("len checked");
    let func = positional.next().expect("len checked");
    let target_ref = create_finalize_target_weakref(&obj, heap, interns)?;
    obj.drop_with_heap(heap);
    let bound_args = positional.collect::<Vec<_>>();
    let bound_kwargs = kwargs.into_iter().collect::<Vec<_>>();

    let partial = Partial::new_weakref_finalize(func, bound_args, bound_kwargs, target_ref);
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    heap.mark_weak_finalize_partial(partial_id);
    Ok(AttrCallResult::Value(Value::Ref(partial_id)))
}

/// Implementation of `weakref.WeakSet(iterable=None)`.
fn weakset(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let value = Set::init(heap, args, interns)?;
    if let Value::Ref(set_id) = &value {
        heap.mark_weak_set(*set_id);
    }
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `weakref.WeakValueDictionary(mapping=None)`.
fn weakvalue_dictionary(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = Dict::init(heap, args, interns)?;
    if let Value::Ref(dict_id) = &value {
        heap.mark_weak_value_dict(*dict_id);
    }
    Ok(AttrCallResult::Value(value))
}

/// Implementation of `weakref.WeakKeyDictionary(mapping=None)`.
fn weakkey_dictionary(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = Dict::init(heap, args, interns)?;
    if let Value::Ref(dict_id) = &value {
        heap.mark_weak_key_dict(*dict_id);
    }
    Ok(AttrCallResult::Value(value))
}

/// Creates and registers the internal target weakref for `weakref.finalize`.
fn create_finalize_target_weakref(
    obj: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<HeapId> {
    let Value::Ref(target_id) = obj else {
        let type_name = obj.py_type(heap);
        return Err(ExcType::type_error(format!(
            "cannot create weak reference to '{type_name}' object"
        )));
    };
    ensure_weakrefable(obj, heap, interns)?;

    let weakref = WeakRef::new(*target_id);
    let weakref_id = heap.allocate(HeapData::WeakRef(weakref))?;
    let register_result = heap.with_entry_mut(*target_id, |_, data| {
        if let HeapData::Instance(inst) = data {
            inst.register_weakref(weakref_id);
        }
        Ok(())
    });
    match register_result {
        Ok(()) => Ok(weakref_id),
        Err(err) => {
            Value::Ref(weakref_id).drop_with_heap(heap);
            Err(err)
        }
    }
}

/// Implements bound `finalize.detach()` calls.
fn finalize_detach(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let partial = args.get_one_arg("finalize.detach", heap)?;
    defer_drop!(partial, heap);
    let Value::Ref(partial_id) = partial else {
        return Err(ExcType::attribute_error(Type::Function, "detach"));
    };
    let result = heap.with_entry_mut(*partial_id, |heap, data| {
        let HeapData::Partial(partial) = data else {
            return Err(ExcType::attribute_error(Type::Function, "detach"));
        };
        partial.py_call_attr(
            heap,
            &EitherStr::Heap("detach".to_owned()),
            ArgValues::Empty,
            interns,
            Some(*partial_id),
        )
    })?;
    Ok(AttrCallResult::Value(result))
}

/// Implements bound `finalize.peek()` calls.
fn finalize_peek(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let partial = args.get_one_arg("finalize.peek", heap)?;
    defer_drop!(partial, heap);
    let Value::Ref(partial_id) = partial else {
        return Err(ExcType::attribute_error(Type::Function, "peek"));
    };
    let result = heap.with_entry_mut(*partial_id, |heap, data| {
        let HeapData::Partial(partial) = data else {
            return Err(ExcType::attribute_error(Type::Function, "peek"));
        };
        partial.py_call_attr(
            heap,
            &EitherStr::Heap("peek".to_owned()),
            ArgValues::Empty,
            interns,
            Some(*partial_id),
        )
    })?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `weakref.WeakMethod(method, callback=None)`.
fn weakmethod(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (method, callback) = args.get_one_two_args("weakref.WeakMethod", heap)?;
    defer_drop!(method, heap);
    let Value::Ref(method_id) = method else {
        return Err(ExcType::type_error(format!(
            "argument should be a bound method, not {}",
            method.py_type(heap)
        )));
    };
    let (func, self_arg) = match heap.get(*method_id) {
        HeapData::BoundMethod(bound) => (
            bound.func().clone_with_heap(heap),
            bound.self_arg().clone_with_heap(heap),
        ),
        _ => {
            return Err(ExcType::type_error(format!(
                "argument should be a bound method, not {}",
                method.py_type(heap)
            )));
        }
    };
    let target_id = match self_arg {
        Value::Ref(id) => {
            heap.dec_ref(id);
            id
        }
        other => {
            other.drop_with_heap(heap);
            func.drop_with_heap(heap);
            return Err(ExcType::type_error("argument should be a bound method, not object"));
        }
    };

    let weakref = WeakRef::new_method(target_id, func, callback);
    let weakref_id = heap.allocate(HeapData::WeakRef(weakref))?;
    let register_result = heap.with_entry_mut(target_id, |_, data| {
        if let HeapData::Instance(inst) = data {
            inst.register_weakref(weakref_id);
        }
        Ok(())
    });

    match register_result {
        Ok(()) => Ok(AttrCallResult::Value(Value::Ref(weakref_id))),
        Err(err) => {
            Value::Ref(weakref_id).drop_with_heap(heap);
            Err(err)
        }
    }
}

/// Shared weakref constructor used by `weakref.ref` and `weakref.WeakMethod`.
fn create_weakref(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    func_name: &str,
) -> RunResult<AttrCallResult> {
    let (target, callback) = args.get_one_two_args(func_name, heap)?;
    defer_drop!(target, heap);

    if func_name == "weakref.ref"
        && let Value::DefFunction(func_id) = target
    {
        let weakref = WeakRef::new_direct(Value::DefFunction(*func_id), callback);
        let weakref_id = heap.allocate(HeapData::WeakRef(weakref))?;
        return Ok(AttrCallResult::Value(Value::Ref(weakref_id)));
    }

    let target_id = ensure_weakrefable(target, heap, interns)?;

    if func_name == "weakref.ref"
        && callback.is_none()
        && let Some(existing_id) = existing_reference_weakref_id(target_id, heap)
    {
        heap.inc_ref(existing_id);
        return Ok(AttrCallResult::Value(Value::Ref(existing_id)));
    }

    let is_proxy = func_name == "weakref.proxy";
    let weakref = if is_proxy {
        WeakRef::new_proxy(target_id, callback)
    } else {
        WeakRef::new_with(target_id, callback, crate::types::weakref::WeakRefKind::Reference)
    };
    let weakref_id = heap.allocate(HeapData::WeakRef(weakref))?;
    let register_result = heap.with_entry_mut(target_id, |_, data| {
        if let HeapData::Instance(inst) = data {
            inst.register_weakref(weakref_id);
        }
        Ok(())
    });

    match register_result {
        Ok(()) => Ok(AttrCallResult::Value(Value::Ref(weakref_id))),
        Err(err) => {
            Value::Ref(weakref_id).drop_with_heap(heap);
            Err(err)
        }
    }
}

/// Returns an existing callback-free non-proxy weakref for `weakref.ref(obj)` caching.
fn existing_reference_weakref_id(target_id: HeapId, heap: &Heap<impl ResourceTracker>) -> Option<HeapId> {
    let HeapData::Instance(instance) = heap.get_if_live(target_id)? else {
        return None;
    };
    for &weakref_id in instance.weakref_ids() {
        let Some(HeapData::WeakRef(wr)) = heap.get_if_live(weakref_id) else {
            continue;
        };
        if wr.target() == Some(target_id) && !wr.is_proxy() && !wr.has_callback() {
            return Some(weakref_id);
        }
    }
    None
}

/// Validates that a value can be weakly referenced and returns its heap id.
fn ensure_weakrefable(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<HeapId> {
    let Value::Ref(target_id) = value else {
        let type_name = value.py_type(heap);
        return Err(ExcType::type_error(format!(
            "cannot create weak reference to '{type_name}' object"
        )));
    };

    let has_weakref = match heap.get(*target_id) {
        HeapData::Instance(inst) => match heap.get(inst.class_id()) {
            HeapData::ClassObject(cls) => {
                cls.instance_has_weakref() && !is_tuple_subclass(inst.class_id(), heap, interns)
            }
            _ => false,
        },
        HeapData::Closure(_, _, _)
        | HeapData::FunctionDefaults(_, _)
        | HeapData::ClassObject(_)
        | HeapData::BoundMethod(_) => true,
        _ => false,
    };

    if !has_weakref {
        let type_name = match heap.get(*target_id) {
            HeapData::Instance(inst) => match heap.get(inst.class_id()) {
                HeapData::ClassObject(cls) => cls.name(interns).to_string(),
                _ => "instance".to_string(),
            },
            other => other.py_type(heap).to_string(),
        };
        return Err(ExcType::type_error(format!(
            "cannot create weak reference to '{type_name}' object"
        )));
    }

    Ok(*target_id)
}

/// Returns true when `class_id` is a tuple subclass.
fn is_tuple_subclass(class_id: HeapId, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> bool {
    let HeapData::ClassObject(cls) = heap.get(class_id) else {
        return false;
    };
    cls.mro().iter().any(|mro_id| match heap.get(*mro_id) {
        HeapData::ClassObject(base) => base.name(interns) == "tuple",
        _ => false,
    })
}

/// Returns registered weakref ids for an instance object.
fn weakref_ids_for_object(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<Vec<HeapId>> {
    let Value::Ref(id) = value else {
        return None;
    };
    let HeapData::Instance(instance) = heap.get(*id) else {
        return None;
    };
    Some(instance.weakref_ids().to_vec())
}
