//! Implementation of the `inspect` module.
//!
//! Provides a minimal runtime-compatible subset used by parity tests:
//! - `inspect.signature(callable)` returning an object with a `parameters` mapping.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunError, RunResult},
    heap::{Heap, HeapData, HeapId},
    intern::{FunctionId, Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Dict, Module, PyTrait},
    value::Value,
};

/// Inspect module functions that can be called at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum InspectFunctions {
    /// `inspect.signature(callable)` - returns a minimal signature object.
    Signature,
}

/// Creates the `inspect` module and allocates it on the heap.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Inspect);
    module.set_attr(
        StaticStrings::InspectSignature,
        Value::ModuleFunction(ModuleFunctions::Inspect(InspectFunctions::Signature)),
        heap,
        interns,
    );
    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to an inspect module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: InspectFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        InspectFunctions::Signature => signature(heap, interns, args),
    }
}

/// Implements `inspect.signature(callable)`.
///
/// This returns a lightweight object exposing `.parameters` as an ordered dict
/// keyed by parameter names.
fn signature(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let mut target = args.get_one_arg("inspect.signature", heap)?;

    // Follow __wrapped__ links like CPython's inspect.signature().
    let wrapped_attr: crate::intern::StringId = StaticStrings::DunderWrapped.into();
    for _ in 0..32 {
        match target.py_getattr(wrapped_attr, heap, interns) {
            Ok(AttrCallResult::Value(wrapped)) => {
                target.drop_with_heap(heap);
                target = wrapped;
            }
            Err(RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::AttributeError => {
                break;
            }
            Ok(_) => {
                target.drop_with_heap(heap);
                return Err(RunError::internal(
                    "inspect.signature: __wrapped__ lookup returned non-value",
                ));
            }
            Err(err) => {
                target.drop_with_heap(heap);
                return Err(err);
            }
        }
    }

    let Some(function_id) = function_id_from_value(&target, heap) else {
        let type_name = target.py_type(heap);
        target.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "inspect.signature() expected function, got '{type_name}'"
        )));
    };

    let mut pairs: Vec<(Value, Value)> = Vec::new();
    for param_name in interns.get_function(function_id).signature.param_names() {
        pairs.push((Value::InternString(param_name), Value::None));
    }
    let parameters = Dict::from_pairs(pairs, heap, interns)?;
    let parameters_id = heap.allocate(HeapData::Dict(parameters))?;

    let mut result = Module::new(StaticStrings::Inspect);
    result.set_attr(StaticStrings::Parameters, Value::Ref(parameters_id), heap, interns);
    let result_id = heap.allocate(HeapData::Module(result))?;

    target.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Ref(result_id)))
}

/// Extracts a function id from a Python function-like value.
fn function_id_from_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<FunctionId> {
    match value {
        Value::DefFunction(function_id) => Some(*function_id),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Closure(function_id, _, _) | HeapData::FunctionDefaults(function_id, _) => Some(*function_id),
            _ => None,
        },
        _ => None,
    }
}
