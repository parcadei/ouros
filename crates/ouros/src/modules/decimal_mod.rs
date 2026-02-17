//! Implementation of the `decimal` module.
//!
//! This module exposes enough of CPython's `decimal` surface for parity tests:
//! - `Decimal` and `Context` constructors
//! - Context helpers (`getcontext`, `setcontext`, `localcontext`)
//! - Common rounding constants and context singletons
//! - Decimal signal/exception type aliases
//!
//! The runtime keeps a lightweight thread-local decimal context (`prec`, `rounding`).

use std::cell::RefCell;

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Decimal, Module, PyTrait, StdlibObject, Type},
    value::Value,
};

/// Default rounding mode used by CPython's default context.
pub(crate) const ROUND_HALF_EVEN: &str = "ROUND_HALF_EVEN";

/// Mutable decimal context state shared by decimal operations in the current thread.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DecimalContextConfig {
    /// Precision used by context-sensitive decimal operations.
    pub prec: i64,
    /// Active rounding mode string (e.g. `ROUND_HALF_EVEN`).
    pub rounding: String,
}

impl DecimalContextConfig {
    /// Returns the CPython-compatible default decimal context.
    #[must_use]
    pub fn default_context() -> Self {
        Self {
            prec: 28,
            rounding: ROUND_HALF_EVEN.to_string(),
        }
    }
}

thread_local! {
    /// Global decimal context state for the current interpreter thread.
    static DECIMAL_CONTEXT: RefCell<DecimalContextConfig> =
        RefCell::new(DecimalContextConfig::default_context());
}

/// Returns a cloned snapshot of the current decimal context.
#[must_use]
pub(crate) fn get_current_context_config() -> DecimalContextConfig {
    DECIMAL_CONTEXT.with(|ctx| ctx.borrow().clone())
}

/// Replaces the current decimal context.
pub(crate) fn set_current_context_config(config: DecimalContextConfig) {
    DECIMAL_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = config;
    });
}

/// Returns the current precision clamped to a valid positive i32 range.
#[must_use]
pub(crate) fn current_precision() -> i32 {
    let prec = get_current_context_config().prec;
    if prec <= 0 {
        1
    } else if prec > i64::from(i32::MAX) {
        i32::MAX
    } else {
        // Safe due explicit bounds checks above.
        i32::try_from(prec).unwrap_or(28)
    }
}

/// Decimal module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum DecimalFunctions {
    /// Returns the current global decimal context.
    Getcontext,
    /// Replaces the current global decimal context.
    Setcontext,
    /// Creates a context-manager context object.
    Localcontext,
}

/// Creates the `decimal` module and allocates it on the heap.
///
/// Exposes the `Decimal` and `Context` constructors along with the subset of
/// constants/functions used by parity tests.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<crate::heap::HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Decimal);

    module.set_attr(
        StaticStrings::DecimalClass,
        Value::Builtin(Builtins::Type(Type::Decimal)),
        heap,
        interns,
    );
    module.set_attr_str(
        "Context",
        Value::Builtin(Builtins::Type(Type::DecimalContext)),
        heap,
        interns,
    )?;
    // `DecimalTuple` is the named-tuple-like return type of `Decimal.as_tuple()`.
    // Ouros models it as a lightweight object type for runtime compatibility.
    module.set_attr_str(
        "DecimalTuple",
        Value::Builtin(Builtins::Type(Type::Object)),
        heap,
        interns,
    )?;
    // CPython's 64-bit `decimal.MAX_PREC` constant.
    module.set_attr_str("MAX_PREC", Value::Int(999_999_999_999_999_999), heap, interns)?;
    module.set_attr_str(
        "DecimalException",
        Value::Builtin(Builtins::ExcType(ExcType::Exception)),
        heap,
        interns,
    )?;

    module.set_attr_str(
        "getcontext",
        Value::ModuleFunction(ModuleFunctions::Decimal(DecimalFunctions::Getcontext)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "setcontext",
        Value::ModuleFunction(ModuleFunctions::Decimal(DecimalFunctions::Setcontext)),
        heap,
        interns,
    )?;
    module.set_attr_str(
        "localcontext",
        Value::ModuleFunction(ModuleFunctions::Decimal(DecimalFunctions::Localcontext)),
        heap,
        interns,
    )?;

    set_context_singleton(&mut module, "BasicContext", 9, "ROUND_HALF_UP", heap, interns)?;
    set_context_singleton(&mut module, "DefaultContext", 28, ROUND_HALF_EVEN, heap, interns)?;
    set_context_singleton(&mut module, "ExtendedContext", 9, ROUND_HALF_EVEN, heap, interns)?;

    for mode in [
        "ROUND_UP",
        "ROUND_DOWN",
        "ROUND_CEILING",
        "ROUND_FLOOR",
        "ROUND_HALF_UP",
        "ROUND_HALF_DOWN",
        ROUND_HALF_EVEN,
        "ROUND_05UP",
    ] {
        set_str_constant(&mut module, mode, mode, heap, interns)?;
    }

    for exc in [
        "InvalidOperation",
        "DivisionByZero",
        "Overflow",
        "Underflow",
        "Inexact",
        "Rounded",
        "Clamped",
        "DivisionImpossible",
        "DivisionUndefined",
        "FloatOperation",
        "InvalidContext",
        "Subnormal",
    ] {
        module.set_attr_str(
            exc,
            Value::Builtin(Builtins::ExcType(ExcType::Exception)),
            heap,
            interns,
        )?;
    }

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a decimal module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    function: DecimalFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        DecimalFunctions::Getcontext => getcontext(heap, args),
        DecimalFunctions::Setcontext => setcontext(heap, args),
        DecimalFunctions::Localcontext => localcontext(heap, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

/// Dispatches `decimal.Decimal(...)` constructor calls from `Type::Decimal`.
pub(crate) fn call_type(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    Ok(AttrCallResult::Value(decimal_constructor(heap, interns, args)?))
}

/// Dispatches `decimal.Context(...)` constructor calls from `Type::DecimalContext`.
pub(crate) fn call_context_type(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    context_constructor(heap, interns, args)
}

/// Creates a new Decimal from a supported argument value.
fn decimal_constructor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let Some(arg) = args.get_zero_one_arg("Decimal", heap)? else {
        let heap_id = heap.allocate(HeapData::Decimal(Decimal::from_i64(0)))?;
        return Ok(Value::Ref(heap_id));
    };
    defer_drop!(arg, heap);

    let decimal = match arg {
        Value::InternString(string_id) => parse_decimal_from_str(interns.get_str(*string_id))?,
        Value::Int(i) => Decimal::from_i64(*i),
        Value::Bool(b) => Decimal::from_i64(i64::from(*b)),
        Value::Float(f) => Decimal::from_f64_exact(*f),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Decimal(d) => d.clone(),
            HeapData::LongInt(li) => Decimal::new(li.inner().clone(), 0),
            HeapData::Str(s) => parse_decimal_from_str(s.as_str())?,
            _ => {
                let type_name = arg.py_type(heap);
                return Err(ExcType::type_error(format!(
                    "conversion from {type_name} to Decimal is not supported"
                )));
            }
        },
        _ => {
            let type_name = arg.py_type(heap);
            return Err(ExcType::type_error(format!(
                "conversion from {type_name} to Decimal is not supported"
            )));
        }
    };

    let heap_id = heap.allocate(HeapData::Decimal(decimal))?;
    Ok(Value::Ref(heap_id))
}

/// Parses a string as a Decimal, mapping parse failures to `ValueError`.
fn parse_decimal_from_str(value: &str) -> RunResult<Decimal> {
    Decimal::from_string(value)
        .map_err(|e| SimpleException::new_msg(ExcType::ValueError, format!("Invalid literal for Decimal: {e}")).into())
}

/// Implements `decimal.getcontext()`.
fn getcontext(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("getcontext", heap)?;
    create_context_value(heap, get_current_context_config(), None)
}

/// Implements `decimal.setcontext(ctx)`.
fn setcontext(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let ctx = args.get_one_arg("setcontext", heap)?;
    defer_drop!(ctx, heap);
    let config = context_from_value(ctx, heap)?;
    set_current_context_config(config);
    Ok(Value::None)
}

/// Implements `decimal.localcontext([ctx])`.
fn localcontext(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let base = if let Some(ctx) = args.get_zero_one_arg("localcontext", heap)? {
        defer_drop!(ctx, heap);
        context_from_value(ctx, heap)?
    } else {
        get_current_context_config()
    };
    let saved = get_current_context_config();
    create_context_value(heap, base, Some(saved))
}

/// Implements `decimal.Context(...)`.
fn context_constructor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional.collect();
    let positional_len = positional.len();
    if positional_len > 1 {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("Context", 1, positional_len));
    }

    let mut config = get_current_context_config();
    if let Some(positional_prec) = positional.pop() {
        let parsed = positional_prec.as_int(heap);
        positional_prec.drop_with_heap(heap);
        config.prec = parsed?;
    }

    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = key_name.as_str(interns);
        key.drop_with_heap(heap);

        match key_name {
            "prec" => {
                let parsed = value.as_int(heap);
                value.drop_with_heap(heap);
                config.prec = parsed?;
            }
            "rounding" => {
                config.rounding = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
            }
            other => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("Context", other));
            }
        }
    }

    create_context_value(heap, config, None)
}

/// Converts a runtime context value into plain context config fields.
fn context_from_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<DecimalContextConfig> {
    let Value::Ref(heap_id) = value else {
        return Err(ExcType::type_error("argument must be a Context"));
    };
    let HeapData::StdlibObject(obj) = heap.get(*heap_id) else {
        return Err(ExcType::type_error("argument must be a Context"));
    };
    obj.decimal_context_config()
        .ok_or_else(|| ExcType::type_error("argument must be a Context"))
}

/// Allocates a decimal context object value.
fn create_context_value(
    heap: &mut Heap<impl ResourceTracker>,
    config: DecimalContextConfig,
    saved: Option<DecimalContextConfig>,
) -> RunResult<Value> {
    let object = StdlibObject::new_decimal_context(config.prec, config.rounding, saved);
    let heap_id = heap.allocate(HeapData::StdlibObject(object))?;
    Ok(Value::Ref(heap_id))
}

/// Stores a context singleton module attribute.
fn set_context_singleton(
    module: &mut Module,
    name: &str,
    prec: i64,
    rounding: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), crate::resource::ResourceError> {
    let config = DecimalContextConfig {
        prec,
        rounding: rounding.to_string(),
    };
    let object = StdlibObject::new_decimal_context(config.prec, config.rounding, None);
    let id = heap.allocate(HeapData::StdlibObject(object))?;
    module.set_attr_str(name, Value::Ref(id), heap, interns)
}

/// Stores a string constant module attribute.
fn set_str_constant(
    module: &mut Module,
    name: &str,
    value: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), crate::resource::ResourceError> {
    let id = heap.allocate(HeapData::Str(value.into()))?;
    module.set_attr_str(name, Value::Ref(id), heap, interns)
}

/// Converts a Value to a Decimal.
///
/// Accepts Decimal (Ref), Int, Bool, and LongInt.
/// Returns a TypeError for other types.
pub(crate) fn value_to_decimal(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<Decimal> {
    match value {
        Value::Ref(heap_id) => {
            if let HeapData::Decimal(d) = heap.get(*heap_id) {
                Ok(d.clone())
            } else if let HeapData::LongInt(li) = heap.get(*heap_id) {
                Ok(Decimal::new(li.inner().clone(), 0))
            } else {
                let type_name = value.py_type(heap);
                Err(
                    SimpleException::new_msg(ExcType::TypeError, format!("Cannot convert {type_name} to Decimal"))
                        .into(),
                )
            }
        }
        Value::Int(i) => Ok(Decimal::from_i64(*i)),
        Value::Bool(b) => Ok(Decimal::from_i64(i64::from(*b))),
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(ExcType::TypeError, format!("Cannot convert {type_name} to Decimal")).into())
        }
    }
}

/// Implements arithmetic operations on Decimal values.
///
/// This is called from the VM when performing binary operations on Decimals.
pub fn decimal_binary_op(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    left: &Decimal,
    right: &Decimal,
    op: DecimalOp,
) -> RunResult<Value> {
    let result = match op {
        DecimalOp::Add => left.add(right),
        DecimalOp::Sub => left.sub(right),
        DecimalOp::Mul => left.mul(right),
        DecimalOp::Div => left.div(right),
        DecimalOp::FloorDiv => left.floor_div(right),
        DecimalOp::Mod => left.modulo(right),
        DecimalOp::Pow => left.pow(right),
    };

    let heap_id = heap.allocate(HeapData::Decimal(result))?;
    Ok(Value::Ref(heap_id))
}

/// Decimal binary operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
}

/// Implements comparison operations on Decimal values.
///
/// This is called from the VM when comparing Decimals.
pub fn decimal_compare(left: &Decimal, right: &Decimal) -> Option<std::cmp::Ordering> {
    left.partial_cmp(right)
}

/// Checks if a Value is a Decimal.
pub fn is_decimal(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(
        value,
        Value::Ref(heap_id) if matches!(heap.get(*heap_id), HeapData::Decimal(_))
    )
}

/// Gets the Decimal from a Value if it is one.
pub fn get_decimal(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<Decimal> {
    match value {
        Value::Ref(heap_id) => {
            if let HeapData::Decimal(d) = heap.get(*heap_id) {
                Some(d.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}
