//! Compatibility implementation of Python's `cmath` module.
//!
//! This exposes the common complex-number API surface used by libraries and
//! provides deterministic pure-Rust math in sandboxed mode.

use std::f64::consts::{E, PI, TAU};

use crate::{
    args::ArgValues,
    defer_drop, defer_drop_mut,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Module, PyTrait, StdlibObject, allocate_tuple},
    value::Value,
};

/// `cmath` module functions implemented by Ouros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum CmathFunctions {
    Acos,
    Acosh,
    Asin,
    Asinh,
    Atan,
    Atanh,
    Cos,
    Cosh,
    Exp,
    Isclose,
    Isfinite,
    Isinf,
    Isnan,
    Log,
    Log10,
    Phase,
    Polar,
    Rect,
    Sin,
    Sinh,
    Sqrt,
    Tan,
    Tanh,
}

/// Creates the `cmath` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Cmath);

    register(&mut module, "acos", CmathFunctions::Acos, heap, interns)?;
    register(&mut module, "acosh", CmathFunctions::Acosh, heap, interns)?;
    register(&mut module, "asin", CmathFunctions::Asin, heap, interns)?;
    register(&mut module, "asinh", CmathFunctions::Asinh, heap, interns)?;
    register(&mut module, "atan", CmathFunctions::Atan, heap, interns)?;
    register(&mut module, "atanh", CmathFunctions::Atanh, heap, interns)?;
    register(&mut module, "cos", CmathFunctions::Cos, heap, interns)?;
    register(&mut module, "cosh", CmathFunctions::Cosh, heap, interns)?;
    register(&mut module, "exp", CmathFunctions::Exp, heap, interns)?;
    register(&mut module, "isclose", CmathFunctions::Isclose, heap, interns)?;
    register(&mut module, "isfinite", CmathFunctions::Isfinite, heap, interns)?;
    register(&mut module, "isinf", CmathFunctions::Isinf, heap, interns)?;
    register(&mut module, "isnan", CmathFunctions::Isnan, heap, interns)?;
    register(&mut module, "log", CmathFunctions::Log, heap, interns)?;
    register(&mut module, "log10", CmathFunctions::Log10, heap, interns)?;
    register(&mut module, "phase", CmathFunctions::Phase, heap, interns)?;
    register(&mut module, "polar", CmathFunctions::Polar, heap, interns)?;
    register(&mut module, "rect", CmathFunctions::Rect, heap, interns)?;
    register(&mut module, "sin", CmathFunctions::Sin, heap, interns)?;
    register(&mut module, "sinh", CmathFunctions::Sinh, heap, interns)?;
    register(&mut module, "sqrt", CmathFunctions::Sqrt, heap, interns)?;
    register(&mut module, "tan", CmathFunctions::Tan, heap, interns)?;
    register(&mut module, "tanh", CmathFunctions::Tanh, heap, interns)?;

    module.set_attr_text("pi", Value::Float(PI), heap, interns)?;
    module.set_attr_text("e", Value::Float(E), heap, interns)?;
    module.set_attr_text("tau", Value::Float(TAU), heap, interns)?;
    module.set_attr_text("inf", Value::Float(f64::INFINITY), heap, interns)?;
    module.set_attr_text("nan", Value::Float(f64::NAN), heap, interns)?;
    let infj_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(0.0, f64::INFINITY)))?;
    let nanj_id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(0.0, f64::NAN)))?;
    module.set_attr_text("infj", Value::Ref(infj_id), heap, interns)?;
    module.set_attr_text("nanj", Value::Ref(nanj_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `cmath` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: CmathFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        CmathFunctions::Acos => unary_complex(heap, args, "cmath.acos", ComplexNum::acos)?,
        CmathFunctions::Acosh => unary_complex(heap, args, "cmath.acosh", ComplexNum::acosh)?,
        CmathFunctions::Asin => unary_complex(heap, args, "cmath.asin", ComplexNum::asin)?,
        CmathFunctions::Asinh => unary_complex(heap, args, "cmath.asinh", ComplexNum::asinh)?,
        CmathFunctions::Atan => unary_complex(heap, args, "cmath.atan", ComplexNum::atan)?,
        CmathFunctions::Atanh => atanh(heap, args)?,
        CmathFunctions::Cos => unary_complex(heap, args, "cmath.cos", ComplexNum::cos)?,
        CmathFunctions::Cosh => unary_complex(heap, args, "cmath.cosh", ComplexNum::cosh)?,
        CmathFunctions::Exp => unary_complex(heap, args, "cmath.exp", ComplexNum::exp)?,
        CmathFunctions::Sin => unary_complex(heap, args, "cmath.sin", ComplexNum::sin)?,
        CmathFunctions::Sinh => unary_complex(heap, args, "cmath.sinh", ComplexNum::sinh)?,
        CmathFunctions::Sqrt => unary_complex(heap, args, "cmath.sqrt", ComplexNum::sqrt)?,
        CmathFunctions::Tan => unary_complex(heap, args, "cmath.tan", ComplexNum::tan)?,
        CmathFunctions::Tanh => unary_complex(heap, args, "cmath.tanh", ComplexNum::tanh)?,
        CmathFunctions::Isfinite => unary_bool(heap, args, "cmath.isfinite", |z| z.re.is_finite() && z.im.is_finite())?,
        CmathFunctions::Isinf => unary_bool(heap, args, "cmath.isinf", |z| z.re.is_infinite() || z.im.is_infinite())?,
        CmathFunctions::Isnan => unary_bool(heap, args, "cmath.isnan", |z| z.re.is_nan() || z.im.is_nan())?,
        CmathFunctions::Log => log(heap, args)?,
        CmathFunctions::Log10 => log10(heap, args)?,
        CmathFunctions::Phase => phase(heap, args)?,
        CmathFunctions::Polar => polar(heap, args)?,
        CmathFunctions::Rect => rect(heap, args)?,
        CmathFunctions::Isclose => isclose(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Applies a single-argument complex transform and returns complex output.
fn unary_complex(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    name: &str,
    op: impl Fn(ComplexNum) -> ComplexNum,
) -> RunResult<Value> {
    let value = extract_one_positional_no_kwargs(heap, args, name)?;
    defer_drop!(value, heap);
    let z = extract_complex_like(value, heap)?;
    let out = op(z);
    make_complex_value(out.re, out.im, heap)
}

/// Applies a single-argument complex predicate and returns bool output.
fn unary_bool(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    name: &str,
    op: impl Fn(ComplexNum) -> bool,
) -> RunResult<Value> {
    let value = extract_one_positional_no_kwargs(heap, args, name)?;
    defer_drop!(value, heap);
    let z = extract_complex_like(value, heap)?;
    Ok(Value::Bool(op(z)))
}

/// Implements `cmath.atanh(z)`.
fn atanh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = extract_one_positional_no_kwargs(heap, args, "cmath.atanh")?;
    defer_drop!(value, heap);
    let z = extract_complex_like(value, heap)?;
    if z.is_real_plus_or_minus_one() {
        return Err(value_error_math_domain_error());
    }
    let out = z.atanh();
    make_complex_value(out.re, out.im, heap)
}

/// Implements `cmath.log(z[, base])`.
fn log(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (z_value, base_value) = extract_one_two_positional_no_kwargs(heap, args, "log", "cmath.log")?;
    defer_drop!(z_value, heap);
    let z = extract_complex_like(z_value, heap)?;
    let out = if let Some(base_value) = base_value {
        defer_drop!(base_value, heap);
        let base = extract_complex_like(base_value, heap)?;
        if base.is_zero() || base.is_one_real() {
            return Err(value_error_math_domain_error());
        }
        let numerator = if z.is_zero() {
            ComplexNum::new(f64::NEG_INFINITY, 0.0)
        } else {
            z.ln()
        };
        numerator.div(base.ln())
    } else {
        if z.is_zero() {
            return Err(value_error_math_domain_error());
        }
        z.ln()
    };
    make_complex_value(out.re, out.im, heap)
}

/// Implements `cmath.log10(z)`.
fn log10(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = extract_one_positional_no_kwargs(heap, args, "cmath.log10")?;
    defer_drop!(value, heap);
    let z = extract_complex_like(value, heap)?;
    if z.is_zero() {
        return Err(value_error_math_domain_error());
    }
    let out = z.ln().div(ComplexNum::real(10.0).ln());
    make_complex_value(out.re, out.im, heap)
}

/// Implements `cmath.phase(z)`.
fn phase(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = extract_one_positional_no_kwargs(heap, args, "cmath.phase")?;
    defer_drop!(value, heap);
    let z = extract_complex_like(value, heap)?;
    Ok(Value::Float(z.arg()))
}

/// Implements `cmath.polar(z)`.
fn polar(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = extract_one_positional_no_kwargs(heap, args, "cmath.polar")?;
    defer_drop!(value, heap);
    let z = extract_complex_like(value, heap)?;
    Ok(allocate_tuple(
        vec![Value::Float(z.abs()), Value::Float(z.arg())].into(),
        heap,
    )?)
}

/// Implements `cmath.rect(r, phi)`.
fn rect(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (r_value, phi_value) = extract_two_positional_no_kwargs(heap, args, "rect", "cmath.rect")?;
    defer_drop!(r_value, heap);
    defer_drop!(phi_value, heap);

    let r = extract_real_like(r_value, heap)?;
    let phi = extract_real_like(phi_value, heap)?;

    if phi.is_infinite() && r != 0.0 && !r.is_nan() {
        return Err(value_error_math_domain_error());
    }
    if r.is_infinite() && phi.is_nan() {
        return make_complex_value(f64::INFINITY, f64::NAN, heap);
    }

    let cos_phi = phi.cos();
    let sin_phi = phi.sin();
    let real = rect_component(r, cos_phi);
    let imag = rect_component(r, sin_phi);
    make_complex_value(real, imag, heap)
}

/// Implements `cmath.isclose(a, b, *, rel_tol=1e-09, abs_tol=0.0)`.
fn isclose(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let mut a = positional.next();
    let mut b = positional.next();
    let a_from_positional = a.is_some();
    let b_from_positional = b.is_some();
    if let Some(extra) = positional.next() {
        let mut count = 3;
        extra.drop_with_heap(heap);
        for value in positional.by_ref() {
            value.drop_with_heap(heap);
            count += 1;
        }
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        a.drop_with_heap(heap);
        b.drop_with_heap(heap);
        return Err(type_error_isclose_positional_count(count));
    }
    positional.drop_with_heap(heap);

    let mut rel_tol_value: Option<Value> = None;
    let mut abs_tol_value: Option<Value> = None;
    let kwargs_iter = kwargs.into_iter();
    defer_drop_mut!(kwargs_iter, heap);
    for (key, value) in kwargs_iter.by_ref() {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            a.drop_with_heap(heap);
            b.drop_with_heap(heap);
            rel_tol_value.drop_with_heap(heap);
            abs_tol_value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let keyword_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        match keyword_name.as_str() {
            "a" => {
                if a.is_some() {
                    value.drop_with_heap(heap);
                    a.drop_with_heap(heap);
                    b.drop_with_heap(heap);
                    rel_tol_value.drop_with_heap(heap);
                    abs_tol_value.drop_with_heap(heap);
                    if a_from_positional {
                        return Err(type_error_isclose_name_and_position("a", 1));
                    }
                    return Err(type_error_isclose_multiple_keyword("a"));
                }
                a = Some(value);
            }
            "b" => {
                if b.is_some() {
                    value.drop_with_heap(heap);
                    a.drop_with_heap(heap);
                    b.drop_with_heap(heap);
                    rel_tol_value.drop_with_heap(heap);
                    abs_tol_value.drop_with_heap(heap);
                    if b_from_positional {
                        return Err(type_error_isclose_name_and_position("b", 2));
                    }
                    return Err(type_error_isclose_multiple_keyword("b"));
                }
                b = Some(value);
            }
            "rel_tol" => {
                if rel_tol_value.is_some() {
                    value.drop_with_heap(heap);
                    a.drop_with_heap(heap);
                    b.drop_with_heap(heap);
                    rel_tol_value.drop_with_heap(heap);
                    abs_tol_value.drop_with_heap(heap);
                    return Err(type_error_isclose_multiple_keyword("rel_tol"));
                }
                rel_tol_value = Some(value);
            }
            "abs_tol" => {
                if abs_tol_value.is_some() {
                    value.drop_with_heap(heap);
                    a.drop_with_heap(heap);
                    b.drop_with_heap(heap);
                    rel_tol_value.drop_with_heap(heap);
                    abs_tol_value.drop_with_heap(heap);
                    return Err(type_error_isclose_multiple_keyword("abs_tol"));
                }
                abs_tol_value = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                a.drop_with_heap(heap);
                b.drop_with_heap(heap);
                rel_tol_value.drop_with_heap(heap);
                abs_tol_value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("isclose", &keyword_name));
            }
        }
    }

    let Some(a_value) = a else {
        b.drop_with_heap(heap);
        rel_tol_value.drop_with_heap(heap);
        abs_tol_value.drop_with_heap(heap);
        return Err(type_error_isclose_missing_arg("a", 1));
    };
    let Some(b_value) = b else {
        a_value.drop_with_heap(heap);
        rel_tol_value.drop_with_heap(heap);
        abs_tol_value.drop_with_heap(heap);
        return Err(type_error_isclose_missing_arg("b", 2));
    };
    defer_drop!(a_value, heap);
    defer_drop!(b_value, heap);
    let a = extract_complex_like(a_value, heap)?;
    let b = extract_complex_like(b_value, heap)?;

    let rel_tol = if let Some(rel_tol_value) = rel_tol_value {
        defer_drop!(rel_tol_value, heap);
        extract_real_like(rel_tol_value, heap)?
    } else {
        1e-9_f64
    };
    let abs_tol = if let Some(abs_tol_value) = abs_tol_value {
        defer_drop!(abs_tol_value, heap);
        extract_real_like(abs_tol_value, heap)?
    } else {
        0.0_f64
    };
    if rel_tol < 0.0 || abs_tol < 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "tolerances must be non-negative").into());
    }
    #[expect(
        clippy::float_cmp,
        reason = "CPython returns True for exact complex equality before tolerance checks"
    )]
    if a.re == b.re && a.im == b.im {
        return Ok(Value::Bool(true));
    }
    if !a.re.is_finite() || !a.im.is_finite() || !b.re.is_finite() || !b.im.is_finite() {
        return Ok(Value::Bool(false));
    }
    let diff = a.sub(b).abs();
    let limit = (rel_tol * a.abs().max(b.abs())).max(abs_tol);
    Ok(Value::Bool(diff <= limit))
}

/// Extracts one positional argument and rejects any keyword arguments.
fn extract_one_positional_no_kwargs(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    name: &str,
) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(type_error_no_keyword_arguments(name));
    }
    kwargs.drop_with_heap(heap);
    let mut positional: Vec<Value> = positional.collect();
    if positional.len() != 1 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count(name, 1, count));
    }
    Ok(positional.pop().expect("len checked above"))
}

/// Extracts one required positional argument and one optional positional argument.
fn extract_one_two_positional_no_kwargs(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    count_name: &str,
    full_name: &str,
) -> RunResult<(Value, Option<Value>)> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(type_error_no_keyword_arguments(full_name));
    }
    kwargs.drop_with_heap(heap);
    let positional: Vec<Value> = positional.collect();
    if positional.is_empty() {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(count_name, 1, 0));
    }
    if positional.len() > 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(count_name, 2, count));
    }

    let mut iter = positional.into_iter();
    let first = iter.next().expect("len checked above");
    Ok((first, iter.next()))
}

/// Extracts exactly two positional arguments and rejects keyword arguments.
fn extract_two_positional_no_kwargs(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    count_name: &str,
    full_name: &str,
) -> RunResult<(Value, Value)> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(type_error_no_keyword_arguments(full_name));
    }
    kwargs.drop_with_heap(heap);
    let positional: Vec<Value> = positional.collect();
    if positional.len() != 2 {
        let count = positional.len();
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count(count_name, 2, count));
    }

    let mut iter = positional.into_iter();
    let first = iter.next().expect("len checked above");
    let second = iter.next().expect("len checked above");
    Ok((first, second))
}

/// Converts a runtime value into a real number accepted by `cmath`.
fn extract_real_like(value: &Value, heap: &Heap<impl ResourceTracker>) -> Result<f64, RunError> {
    match value {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(f64::from(*b)),
        Value::Ref(heap_id) => {
            if let HeapData::LongInt(li) = heap.get(*heap_id) {
                li.to_f64().ok_or_else(|| {
                    SimpleException::new_msg(ExcType::OverflowError, "int too large to convert to float").into()
                })
            } else if let HeapData::Fraction(frac) = heap.get(*heap_id) {
                Ok(frac.to_f64())
            } else {
                Err(type_error_must_be_real_number(value, heap))
            }
        }
        _ => Err(type_error_must_be_real_number(value, heap)),
    }
}

/// Converts a runtime value into complex components.
fn extract_complex_like(value: &Value, heap: &Heap<impl ResourceTracker>) -> Result<ComplexNum, RunError> {
    if let Value::Ref(id) = value
        && let HeapData::StdlibObject(StdlibObject::Complex { real, imag }) = heap.get(*id)
    {
        return Ok(ComplexNum::new(*real, *imag));
    }
    Ok(ComplexNum::real(extract_real_like(value, heap)?))
}

/// Returns one `rect()` output component while preserving CPython edge-case behavior.
fn rect_component(radius: f64, trig: f64) -> f64 {
    if radius == 0.0 {
        return 0.0;
    }
    if radius.is_nan() && trig == 0.0 {
        return 0.0f64.copysign(trig);
    }
    if radius.is_infinite() && trig == 0.0 {
        return 0.0f64.copysign(radius * trig);
    }
    radius * trig
}

/// Creates the `TypeError` raised when keyword arguments are supplied to positional-only APIs.
fn type_error_no_keyword_arguments(name: &str) -> RunError {
    SimpleException::new_msg(ExcType::TypeError, format!("{name}() takes no keyword arguments")).into()
}

/// Creates the `TypeError` raised when `cmath` requires a real number.
fn type_error_must_be_real_number(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunError {
    let type_name = value.py_type(heap);
    SimpleException::new_msg(ExcType::TypeError, format!("must be real number, not {type_name}")).into()
}

/// Creates the `ValueError` used by `cmath` domain checks.
fn value_error_math_domain_error() -> RunError {
    SimpleException::new_msg(ExcType::ValueError, "math domain error").into()
}

/// Creates the `TypeError` raised when `isclose()` receives too many positional args.
fn type_error_isclose_positional_count(actual: usize) -> RunError {
    SimpleException::new_msg(
        ExcType::TypeError,
        format!("isclose() takes exactly 2 positional arguments ({actual} given)"),
    )
    .into()
}

/// Creates the `TypeError` raised when a required `isclose()` argument is missing.
fn type_error_isclose_missing_arg(name: &str, position: usize) -> RunError {
    SimpleException::new_msg(
        ExcType::TypeError,
        format!("isclose() missing required argument '{name}' (pos {position})"),
    )
    .into()
}

/// Creates the `TypeError` raised when `isclose()` argument is passed by both position and keyword.
fn type_error_isclose_name_and_position(name: &str, position: usize) -> RunError {
    SimpleException::new_msg(
        ExcType::TypeError,
        format!("argument for isclose() given by name ('{name}') and position ({position})"),
    )
    .into()
}

/// Creates the `TypeError` raised for duplicate keyword arguments in `cmath.isclose()`.
fn type_error_isclose_multiple_keyword(name: &str) -> RunError {
    SimpleException::new_msg(
        ExcType::TypeError,
        format!("cmath.isclose() got multiple values for keyword argument '{name}'"),
    )
    .into()
}

/// Allocates a complex result object on the heap.
fn make_complex_value(re: f64, im: f64, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let id = heap.allocate(HeapData::StdlibObject(StdlibObject::new_complex(re, im)))?;
    Ok(Value::Ref(id))
}

/// Registers one module-level function.
fn register(
    module: &mut Module,
    name: &str,
    function: CmathFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Cmath(function)),
        heap,
        interns,
    )
}

/// Lightweight complex number helper for implementing `cmath`.
#[derive(Clone, Copy, Debug)]
struct ComplexNum {
    re: f64,
    im: f64,
}

impl ComplexNum {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    /// Returns true when the complex value is exactly zero (including signed zeros).
    fn is_zero(self) -> bool {
        self.re == 0.0 && self.im == 0.0
    }

    /// Returns true when the complex value is exactly `1+0j`.
    #[expect(
        clippy::float_cmp,
        reason = "CPython branch behavior checks exact one for log base handling"
    )]
    fn is_one_real(self) -> bool {
        self.re == 1.0 && self.im == 0.0
    }

    /// Returns true for exactly `1+0j` or `-1+0j`.
    #[expect(
        clippy::float_cmp,
        reason = "CPython atanh singularity is only for exact +/-1 on the real axis"
    )]
    fn is_real_plus_or_minus_one(self) -> bool {
        self.im == 0.0 && (self.re == 1.0 || self.re == -1.0)
    }

    fn add(self, rhs: Self) -> Self {
        Self::new(self.re + rhs.re, self.im + rhs.im)
    }

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.re - rhs.re, self.im - rhs.im)
    }

    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.re.mul_add(rhs.re, -(self.im * rhs.im)),
            self.re.mul_add(rhs.im, self.im * rhs.re),
        )
    }

    fn div(self, rhs: Self) -> Self {
        let denom = rhs.re.mul_add(rhs.re, rhs.im * rhs.im);
        Self::new(
            (self.re.mul_add(rhs.re, self.im * rhs.im)) / denom,
            (self.im.mul_add(rhs.re, -(self.re * rhs.im))) / denom,
        )
    }

    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    fn sqrt(self) -> Self {
        let r = self.abs();
        let t = r.midpoint(self.re).sqrt();
        let u = r.midpoint(-self.re).sqrt().copysign(self.im);
        Self::new(t, u)
    }

    fn exp(self) -> Self {
        let scale = self.re.exp();
        Self::new(scale * self.im.cos(), scale * self.im.sin())
    }

    fn ln(self) -> Self {
        Self::new(self.abs().ln(), self.arg())
    }

    fn sin(self) -> Self {
        Self::new(self.re.sin() * self.im.cosh(), self.re.cos() * self.im.sinh())
    }

    fn cos(self) -> Self {
        Self::new(self.re.cos() * self.im.cosh(), -(self.re.sin() * self.im.sinh()))
    }

    fn tan(self) -> Self {
        self.sin().div(self.cos())
    }

    fn sinh(self) -> Self {
        Self::new(self.re.sinh() * self.im.cos(), self.re.cosh() * self.im.sin())
    }

    fn cosh(self) -> Self {
        Self::new(self.re.cosh() * self.im.cos(), self.re.sinh() * self.im.sin())
    }

    fn tanh(self) -> Self {
        self.sinh().div(self.cosh())
    }

    fn asin(self) -> Self {
        if self.im == 0.0 {
            if self.re.abs() <= 1.0 {
                return Self::new(self.re.asin(), 0.0f64.copysign(self.im));
            }

            let real = if self.re.is_sign_negative() {
                -PI / 2.0
            } else {
                PI / 2.0
            };
            let imag = self.re.abs().acosh().copysign(self.im);
            return Self::new(real, imag);
        }

        let i = Self::new(0.0, 1.0);
        let one = Self::real(1.0);
        let inside = i.mul(self).add(one.sub(self.mul(self)).sqrt());
        let out = inside.ln().mul(Self::new(0.0, -1.0));
        Self::new(out.re, out.im)
    }

    fn acos(self) -> Self {
        if self.im == 0.0 {
            if self.re.abs() <= 1.0 {
                return Self::new(self.re.acos(), (-0.0f64).copysign(self.im));
            }

            let real = if self.re.is_sign_negative() { PI } else { 0.0 };
            let imag = self.re.abs().acosh().copysign(-self.im);
            return Self::new(real, imag);
        }

        Self::real(PI / 2.0).sub(self.asin())
    }

    fn atan(self) -> Self {
        let i = Self::new(0.0, 1.0);
        let one = Self::real(1.0);
        let num = one.sub(i.mul(self)).ln();
        let den = one.add(i.mul(self)).ln();
        i.mul(num.sub(den)).mul(Self::real(0.5))
    }

    fn asinh(self) -> Self {
        let one = Self::real(1.0);
        self.add(self.mul(self).add(one).sqrt()).ln()
    }

    fn acosh(self) -> Self {
        let one = Self::real(1.0);
        self.add(self.add(one).sqrt().mul(self.sub(one).sqrt())).ln()
    }

    fn atanh(self) -> Self {
        let one = Self::real(1.0);
        one.add(self).ln().sub(one.sub(self).ln()).mul(Self::real(0.5))
    }
}
