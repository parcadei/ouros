//! Minimal implementation of the `gc` module.
//!
//! This module provides a sandbox-safe compatibility surface for parity tests
//! that import `gc` and call `gc.collect()`. Ouros does not expose CPython's
//! process-global garbage collector controls, so these functions are no-ops
//! with deterministic return values.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, List, Module, Str, allocate_tuple},
    value::Value,
};

static GC_ENABLED: AtomicBool = AtomicBool::new(true);
static GC_DEBUG_FLAGS: AtomicI64 = AtomicI64::new(0);
static GC_THRESHOLD0: AtomicI64 = AtomicI64::new(2000);
static GC_THRESHOLD1: AtomicI64 = AtomicI64::new(10);
static GC_THRESHOLD2: AtomicI64 = AtomicI64::new(0);
static GC_COLLECTIONS_GEN0: AtomicI64 = AtomicI64::new(0);
static GC_COLLECTIONS_GEN1: AtomicI64 = AtomicI64::new(0);
static GC_COLLECTIONS_GEN2: AtomicI64 = AtomicI64::new(0);

/// `gc` module functions supported by Ouros compatibility mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum GcFunctions {
    /// `gc.collect([generation])`
    Collect,
    /// `gc.enable()`
    Enable,
    /// `gc.disable()`
    Disable,
    /// `gc.isenabled()`
    Isenabled,
    /// `gc.get_count()`
    GetCount,
    /// `gc.get_debug()`
    GetDebug,
    /// `gc.set_debug(flags)`
    SetDebug,
    /// `gc.get_threshold()`
    GetThreshold,
    /// `gc.set_threshold(t0[, t1[, t2]])`
    SetThreshold,
    /// `gc.get_stats()`
    GetStats,
    /// `gc.freeze()`
    Freeze,
    /// `gc.unfreeze()`
    Unfreeze,
    /// `gc.get_freeze_count()`
    GetFreezeCount,
    /// `gc.get_referents(*objs)`
    GetReferents,
    /// `gc.get_referrers(*objs)`
    GetReferrers,
    /// `gc.is_tracked(obj)`
    IsTracked,
    /// `gc.is_finalized(obj)`
    IsFinalized,
}

/// Creates the built-in `gc` module and allocates it on the heap.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Gc);
    module.set_attr_text(
        "collect",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::Collect)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "enable",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::Enable)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "disable",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::Disable)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "isenabled",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::Isenabled)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_count",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetCount)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_debug",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetDebug)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "set_debug",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::SetDebug)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_threshold",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetThreshold)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "set_threshold",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::SetThreshold)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_stats",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetStats)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "freeze",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::Freeze)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "unfreeze",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::Unfreeze)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_freeze_count",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetFreezeCount)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_referents",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetReferents)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "get_referrers",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::GetReferrers)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "is_tracked",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::IsTracked)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "is_finalized",
        Value::ModuleFunction(ModuleFunctions::Gc(GcFunctions::IsFinalized)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `gc` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: GcFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        GcFunctions::Collect => collect(heap, interns, args),
        GcFunctions::Enable => enable(heap, args),
        GcFunctions::Disable => disable(heap, args),
        GcFunctions::Isenabled => isenabled(heap, args),
        GcFunctions::GetCount => get_count(heap, args),
        GcFunctions::GetDebug => get_debug(heap, args),
        GcFunctions::SetDebug => set_debug(heap, args),
        GcFunctions::GetThreshold => get_threshold(heap, args),
        GcFunctions::SetThreshold => set_threshold(heap, args),
        GcFunctions::GetStats => get_stats(heap, interns, args),
        GcFunctions::Freeze => freeze(heap, args),
        GcFunctions::Unfreeze => unfreeze(heap, args),
        GcFunctions::GetFreezeCount => get_freeze_count(heap, args),
        GcFunctions::GetReferents => get_referents(heap, args),
        GcFunctions::GetReferrers => get_referrers(heap, args),
        GcFunctions::IsTracked => is_tracked(heap, args),
        GcFunctions::IsFinalized => is_finalized(heap, args),
    }
}

/// Implements `gc.collect(...)` as a deterministic no-op.
///
/// Returning `0` matches CPython's common "nothing collected" path and is
/// sufficient for weakref parity tests, which only rely on this call existing.
fn collect(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let generation = parse_optional_generation(args, heap, interns)?;
    match generation.unwrap_or(2) {
        0 => {
            GC_COLLECTIONS_GEN0.fetch_add(1, Ordering::Relaxed);
        }
        1 => {
            GC_COLLECTIONS_GEN1.fetch_add(1, Ordering::Relaxed);
        }
        _ => {
            GC_COLLECTIONS_GEN2.fetch_add(1, Ordering::Relaxed);
        }
    }
    heap.collect_weak_container_garbage(interns)?;
    if let Some((func, callback_args)) = heap.take_pending_finalize_callback(interns)? {
        return Ok(AttrCallResult::CallFunction(func, callback_args));
    }
    if let Some((callback, weakref_arg)) = heap.take_pending_weakref_callback() {
        return Ok(AttrCallResult::CallFunction(callback, ArgValues::One(weakref_arg)));
    }
    Ok(AttrCallResult::Value(Value::Int(0)))
}

/// Implements `gc.enable()` as a no-op.
fn enable(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.enable", heap)?;
    GC_ENABLED.store(true, Ordering::Relaxed);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `gc.disable()` as a no-op.
fn disable(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.disable", heap)?;
    GC_ENABLED.store(false, Ordering::Relaxed);
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `gc.isenabled()` with a fixed enabled state.
fn isenabled(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.isenabled", heap)?;
    Ok(AttrCallResult::Value(Value::Bool(GC_ENABLED.load(Ordering::Relaxed))))
}

fn get_count(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.get_count", heap)?;
    let count0 = 0;
    let tuple = allocate_tuple(
        smallvec::smallvec![Value::Int(count0), Value::Int(0), Value::Int(0)],
        heap,
    )?;
    Ok(AttrCallResult::Value(tuple))
}

fn get_debug(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.get_debug", heap)?;
    Ok(AttrCallResult::Value(Value::Int(
        GC_DEBUG_FLAGS.load(Ordering::Relaxed),
    )))
}

fn set_debug(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let flags = args.get_one_arg("gc.set_debug", heap)?;
    let flags_i64 = flags.as_int(heap)?;
    flags.drop_with_heap(heap);
    GC_DEBUG_FLAGS.store(flags_i64, Ordering::Relaxed);
    Ok(AttrCallResult::Value(Value::None))
}

fn get_threshold(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.get_threshold", heap)?;
    let tuple = allocate_tuple(
        smallvec::smallvec![
            Value::Int(GC_THRESHOLD0.load(Ordering::Relaxed)),
            Value::Int(GC_THRESHOLD1.load(Ordering::Relaxed)),
            Value::Int(GC_THRESHOLD2.load(Ordering::Relaxed))
        ],
        heap,
    )?;
    Ok(AttrCallResult::Value(tuple))
}

fn set_threshold(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    let mut values = positional.collect::<Vec<_>>();
    if !kwargs.is_empty() {
        values.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("gc.set_threshold() takes no keyword arguments"));
    }
    if values.is_empty() || values.len() > 3 {
        let len = values.len();
        values.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "gc.set_threshold expected 1 to 3 arguments, got {len}"
        )));
    }
    let t0 = parse_threshold(values.remove(0), heap)?;
    let t1 = if let Some(value) = values.first_mut() {
        parse_threshold(std::mem::replace(value, Value::None), heap)?
    } else {
        GC_THRESHOLD1.load(Ordering::Relaxed)
    };
    let t2 = if values.len() > 1 {
        parse_threshold(values.remove(1), heap)?
    } else {
        GC_THRESHOLD2.load(Ordering::Relaxed)
    };
    values.drop_with_heap(heap);
    GC_THRESHOLD0.store(t0, Ordering::Relaxed);
    GC_THRESHOLD1.store(t1, Ordering::Relaxed);
    GC_THRESHOLD2.store(t2, Ordering::Relaxed);
    Ok(AttrCallResult::Value(Value::None))
}

fn get_stats(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.get_stats", heap)?;
    let stats = vec![
        gc_generation_stats_dict(GC_COLLECTIONS_GEN0.load(Ordering::Relaxed), 0, 0, heap, interns)?,
        gc_generation_stats_dict(GC_COLLECTIONS_GEN1.load(Ordering::Relaxed), 0, 0, heap, interns)?,
        gc_generation_stats_dict(GC_COLLECTIONS_GEN2.load(Ordering::Relaxed), 0, 0, heap, interns)?,
    ];
    let list_id = heap.allocate(HeapData::List(List::new(stats)))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

fn freeze(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.freeze", heap)?;
    Ok(AttrCallResult::Value(Value::None))
}

fn unfreeze(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.unfreeze", heap)?;
    Ok(AttrCallResult::Value(Value::None))
}

fn get_freeze_count(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("gc.get_freeze_count", heap)?;
    Ok(AttrCallResult::Value(Value::Int(0)))
}

fn get_referents(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    let list_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

fn get_referrers(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (positional, kwargs) = args.into_parts();
    positional.drop_with_heap(heap);
    kwargs.drop_with_heap(heap);
    let list_id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
    Ok(AttrCallResult::Value(Value::Ref(list_id)))
}

fn is_tracked(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("gc.is_tracked", heap)?;
    let tracked = match obj {
        Value::Ref(id) => matches!(
            heap.get(id),
            HeapData::List(_)
                | HeapData::Tuple(_)
                | HeapData::NamedTuple(_)
                | HeapData::Dict(_)
                | HeapData::Set(_)
                | HeapData::FrozenSet(_)
                | HeapData::ClassObject(_)
                | HeapData::Instance(_)
                | HeapData::Module(_)
        ),
        _ => false,
    };
    obj.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(tracked)))
}

fn is_finalized(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let obj = args.get_one_arg("gc.is_finalized", heap)?;
    obj.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Bool(false)))
}

fn parse_optional_generation(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<i64>> {
    let (mut positional, kwargs) = args.into_parts();
    let generation = match positional.next() {
        Some(value) => {
            let parsed = value.as_int(heap)?;
            value.drop_with_heap(heap);
            Some(parsed)
        }
        None => None,
    };
    if positional.next().is_some() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("gc.collect", 1, 2));
    }
    if let Some((key, value)) = kwargs.into_iter().next() {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        if key_name != "generation" {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword("gc.collect", &key_name));
        }
        if generation.is_some() {
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg("gc.collect", "generation"));
        }
        let parsed = value.as_int(heap)?;
        value.drop_with_heap(heap);
        return Ok(Some(parsed));
    }
    Ok(generation)
}

fn parse_threshold(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<i64> {
    let out = value.as_int(heap)?;
    value.drop_with_heap(heap);
    Ok(out)
}

fn gc_generation_stats_dict(
    collections: i64,
    collected: i64,
    uncollectable: i64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let key_collections = heap.allocate(HeapData::Str(Str::from("collections")))?;
    let key_collected = heap.allocate(HeapData::Str(Str::from("collected")))?;
    let key_uncollectable = heap.allocate(HeapData::Str(Str::from("uncollectable")))?;
    let dict = Dict::from_pairs(
        vec![
            (Value::Ref(key_collections), Value::Int(collections)),
            (Value::Ref(key_collected), Value::Int(collected)),
            (Value::Ref(key_uncollectable), Value::Int(uncollectable)),
        ],
        heap,
        interns,
    )?;
    let id = heap.allocate(HeapData::Dict(dict))?;
    Ok(Value::Ref(id))
}
