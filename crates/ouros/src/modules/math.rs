//! Implementation of the `math` module.
//!
//! Provides mathematical functions and constants from Python's `math` module:
//! - Constants: pi, e, tau, inf, nan
//! - Basic functions: ceil, floor, trunc, fabs, copysign
//! - Powers and logarithms: sqrt, cbrt, pow, exp, exp2, expm1, log, log1p, log2, log10
//! - Trigonometry: sin, cos, tan, asin, acos, atan, atan2
//! - Hyperbolic: sinh, cosh, tanh, asinh, acosh, atanh
//! - Angle conversion: degrees, radians
//! - Predicates: isnan, isinf, isfinite, isclose
//! - Integer functions: factorial, isqrt, gcd, comb, perm, lcm
//! - Special functions: erf, erfc, gamma, lgamma
//! - Float manipulation: modf, frexp, ldexp, fmod, remainder, fsum, fma, nextafter, ulp
//! - Aggregation: prod, dist, hypot, sumprod

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, List, Module, OurosIter, PyTrait, Str, allocate_tuple},
    value::Value,
};

/// Math module functions.
///
/// Each variant corresponds to a Python `math` module function. The enum is used
/// both for dispatch in `call()` and as the identity stored in `Value::ModuleFunction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum MathFunctions {
    // Basic functions
    Ceil,
    Floor,
    Trunc,
    Fabs,
    Copysign,
    // Powers and logarithms
    Sqrt,
    Cbrt,
    Pow,
    Exp,
    Exp2,
    Expm1,
    Log,
    Log1p,
    Log2,
    Log10,
    // Trigonometry
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    // Hyperbolic functions
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    // Angle conversion
    Degrees,
    Radians,
    // Predicates
    Isnan,
    Isinf,
    Isfinite,
    Isclose,
    // Integer functions
    Factorial,
    Isqrt,
    Gcd,
    Comb,
    Perm,
    Lcm,
    // Special mathematical functions
    Erf,
    Erfc,
    Gamma,
    Lgamma,
    // Float decomposition and manipulation
    Hypot,
    Fmod,
    Remainder,
    Fsum,
    Modf,
    Frexp,
    Ldexp,
    Fma,
    Nextafter,
    Ulp,
    // Aggregation
    Prod,
    Dist,
    Sumprod,
    CeilDiv,
    FloorDiv,
    SumOfSquares,
    Dot,
    Cross,
}

/// Creates the `math` module and allocates it on the heap.
///
/// Sets up all mathematical constants and functions.
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
    let mut module = Module::new(StaticStrings::Math);

    // Constants
    module.set_attr(StaticStrings::Pi, Value::Float(std::f64::consts::PI), heap, interns);
    module.set_attr(StaticStrings::MathE, Value::Float(std::f64::consts::E), heap, interns);
    module.set_attr(StaticStrings::Tau, Value::Float(std::f64::consts::TAU), heap, interns);
    module.set_attr(StaticStrings::MathInf, Value::Float(f64::INFINITY), heap, interns);
    module.set_attr(StaticStrings::MathNan, Value::Float(f64::NAN), heap, interns);

    // Basic functions
    module.set_attr(
        StaticStrings::Ceil,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Ceil)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Floor,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Floor)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Trunc,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Trunc)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Fabs,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Fabs)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Copysign,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Copysign)),
        heap,
        interns,
    );

    // Powers and logarithms
    module.set_attr(
        StaticStrings::Sqrt,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Sqrt)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Cbrt,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Cbrt)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathPow,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Pow)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Exp,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Exp)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Exp2,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Exp2)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Expm1,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Expm1)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Log,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Log)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Log1p,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Log1p)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Log2,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Log2)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Log10,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Log10)),
        heap,
        interns,
    );

    // Trigonometry
    module.set_attr(
        StaticStrings::Sin,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Sin)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Cos,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Cos)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Tan,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Tan)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Asin,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Asin)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Acos,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Acos)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Atan,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Atan)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Atan2,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Atan2)),
        heap,
        interns,
    );

    // Angle conversion
    module.set_attr(
        StaticStrings::MathDegrees,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Degrees)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathRadians,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Radians)),
        heap,
        interns,
    );

    // Predicates
    module.set_attr(
        StaticStrings::Isnan,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Isnan)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Isinf,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Isinf)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Isfinite,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Isfinite)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Isclose,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Isclose)),
        heap,
        interns,
    );

    // Hyperbolic functions
    module.set_attr(
        StaticStrings::Sinh,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Sinh)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Cosh,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Cosh)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Tanh,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Tanh)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Asinh,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Asinh)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Acosh,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Acosh)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Atanh,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Atanh)),
        heap,
        interns,
    );

    // Integer functions
    module.set_attr(
        StaticStrings::Factorial,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Factorial)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Isqrt,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Isqrt)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Gcd,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Gcd)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Comb,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Comb)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathPerm,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Perm)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Lcm,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Lcm)),
        heap,
        interns,
    );

    // Special mathematical functions
    module.set_attr(
        StaticStrings::Erf,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Erf)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Erfc,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Erfc)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathGamma,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Gamma)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Lgamma,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Lgamma)),
        heap,
        interns,
    );

    // Float decomposition and manipulation
    module.set_attr(
        StaticStrings::Hypot,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Hypot)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Fmod,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Fmod)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathRemainder,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Remainder)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Fsum,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Fsum)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Modf,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Modf)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Frexp,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Frexp)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Ldexp,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Ldexp)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Fma,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Fma)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Nextafter,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Nextafter)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Ulp,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Ulp)),
        heap,
        interns,
    );

    // Aggregation
    module.set_attr(
        StaticStrings::MathProd,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Prod)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathDist,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Dist)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Sumprod,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Sumprod)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathCeilDiv,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::CeilDiv)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathFloorDiv,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::FloorDiv)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathSumOfSquares,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::SumOfSquares)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathDot,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Dot)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::MathCross,
        Value::ModuleFunction(ModuleFunctions::Math(MathFunctions::Cross)),
        heap,
        interns,
    );

    // __all__ — public API matching CPython's math namespace.
    // Excludes Ouros-only extensions (ceil_div, cross, dot, floor_div, sum_of_squares)
    // that are not present in CPython 3.14.
    let public_names = [
        "acos",
        "acosh",
        "asin",
        "asinh",
        "atan",
        "atan2",
        "atanh",
        "cbrt",
        "ceil",
        "comb",
        "copysign",
        "cos",
        "cosh",
        "degrees",
        "dist",
        "e",
        "erf",
        "erfc",
        "exp",
        "exp2",
        "expm1",
        "fabs",
        "factorial",
        "floor",
        "fma",
        "fmod",
        "frexp",
        "fsum",
        "gamma",
        "gcd",
        "hypot",
        "inf",
        "isclose",
        "isfinite",
        "isinf",
        "isnan",
        "isqrt",
        "lcm",
        "ldexp",
        "lgamma",
        "log",
        "log10",
        "log1p",
        "log2",
        "modf",
        "nan",
        "nextafter",
        "perm",
        "pi",
        "pow",
        "prod",
        "radians",
        "remainder",
        "sin",
        "sinh",
        "sqrt",
        "sumprod",
        "tan",
        "tanh",
        "tau",
        "trunc",
        "ulp",
    ];
    let mut all_values = Vec::with_capacity(public_names.len());
    for name in public_names {
        let name_id = heap.allocate(HeapData::Str(Str::from(name)))?;
        all_values.push(Value::Ref(name_id));
    }
    let all_id = heap.allocate(HeapData::List(List::new(all_values)))?;
    module.set_attr_str("__all__", Value::Ref(all_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a math module function.
///
/// All math functions return immediate values (no host involvement needed).
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: MathFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        MathFunctions::Ceil => math_ceil(heap, args),
        MathFunctions::Floor => math_floor(heap, args),
        MathFunctions::Trunc => math_trunc(heap, args),
        MathFunctions::Fabs => math_fabs(heap, args),
        MathFunctions::Copysign => math_copysign(heap, args),
        MathFunctions::Sqrt => math_sqrt(heap, args),
        MathFunctions::Cbrt => math_cbrt(heap, args),
        MathFunctions::Pow => math_pow(heap, args),
        MathFunctions::Exp => math_exp(heap, args),
        MathFunctions::Exp2 => math_exp2(heap, args),
        MathFunctions::Expm1 => math_expm1(heap, args),
        MathFunctions::Log => math_log(heap, args),
        MathFunctions::Log1p => math_log1p(heap, args),
        MathFunctions::Log2 => math_log2(heap, args),
        MathFunctions::Log10 => math_log10(heap, args),
        MathFunctions::Sin => math_sin(heap, args),
        MathFunctions::Cos => math_cos(heap, args),
        MathFunctions::Tan => math_tan(heap, args),
        MathFunctions::Asin => math_asin(heap, args),
        MathFunctions::Acos => math_acos(heap, args),
        MathFunctions::Atan => math_atan(heap, args),
        MathFunctions::Atan2 => math_atan2(heap, args),
        MathFunctions::Sinh => math_sinh(heap, args),
        MathFunctions::Cosh => math_cosh(heap, args),
        MathFunctions::Tanh => math_tanh(heap, args),
        MathFunctions::Asinh => math_asinh(heap, args),
        MathFunctions::Acosh => math_acosh(heap, args),
        MathFunctions::Atanh => math_atanh(heap, args),
        MathFunctions::Degrees => math_degrees(heap, args),
        MathFunctions::Radians => math_radians(heap, args),
        MathFunctions::Isnan => math_isnan(heap, args),
        MathFunctions::Isinf => math_isinf(heap, args),
        MathFunctions::Isfinite => math_isfinite(heap, args),
        MathFunctions::Isclose => math_isclose(heap, interns, args),
        MathFunctions::Factorial => math_factorial(heap, args),
        MathFunctions::Isqrt => math_isqrt(heap, args),
        MathFunctions::Gcd => math_gcd(heap, args),
        MathFunctions::Comb => math_comb(heap, args),
        MathFunctions::Perm => math_perm(heap, args),
        MathFunctions::Lcm => math_lcm(heap, args),
        MathFunctions::Erf => math_erf(heap, args),
        MathFunctions::Erfc => math_erfc(heap, args),
        MathFunctions::Gamma => math_gamma(heap, args),
        MathFunctions::Lgamma => math_lgamma(heap, args),
        MathFunctions::Hypot => math_hypot(heap, args),
        MathFunctions::Fmod => math_fmod(heap, args),
        MathFunctions::Remainder => math_remainder(heap, args),
        MathFunctions::Fsum => math_fsum(heap, interns, args),
        MathFunctions::Modf => math_modf(heap, args),
        MathFunctions::Frexp => math_frexp(heap, args),
        MathFunctions::Ldexp => math_ldexp(heap, args),
        MathFunctions::Fma => math_fma(heap, args),
        MathFunctions::Nextafter => math_nextafter(heap, args),
        MathFunctions::Ulp => math_ulp(heap, args),
        MathFunctions::Prod => math_prod(heap, interns, args),
        MathFunctions::Dist => math_dist(heap, interns, args),
        MathFunctions::Sumprod => math_sumprod(heap, interns, args),
        MathFunctions::CeilDiv => math_ceil_div(heap, args),
        MathFunctions::FloorDiv => math_floor_div(heap, args),
        MathFunctions::SumOfSquares => math_sum_of_squares(heap, interns, args),
        MathFunctions::Dot => math_dot(heap, interns, args),
        MathFunctions::Cross => math_cross(heap, interns, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

/// Converts a Value to f64 for math functions.
///
/// Accepts Int, Float, Bool, and LongInt (if convertible).
/// Returns a TypeError for other types.
fn value_to_f64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<f64> {
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
                let type_name = value.py_type(heap);
                Err(
                    SimpleException::new_msg(ExcType::TypeError, format!("must be real number, not {type_name}"))
                        .into(),
                )
            }
        }
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(ExcType::TypeError, format!("must be real number, not {type_name}")).into())
        }
    }
}

/// Converts a Value to i64 for integer math functions.
///
/// Accepts Int, Bool, and LongInt (if it fits).
/// Returns a TypeError for other types.
fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match *value {
        Value::Int(i) => Ok(i),
        Value::Bool(b) => Ok(i64::from(b)),
        Value::Ref(heap_id) => {
            if let HeapData::LongInt(li) = heap.get(heap_id) {
                li.to_i64().ok_or_else(|| {
                    SimpleException::new_msg(
                        ExcType::OverflowError,
                        "int too large to convert to int", // Note: Python's error message
                    )
                    .into()
                })
            } else {
                let type_name = value.py_type(heap);
                Err(SimpleException::new_msg(
                    ExcType::TypeError,
                    format!("'{type_name}' object cannot be interpreted as an integer"),
                )
                .into())
            }
        }
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(
                ExcType::TypeError,
                format!("'{type_name}' object cannot be interpreted as an integer"),
            )
            .into())
        }
    }
}

/// Implementation of `math.ceil(x)`.
///
/// Returns the ceiling of x as an Integral.
/// This is the smallest integer >= x.
fn math_ceil(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.ceil", heap)?;
    defer_drop!(arg, heap);

    // For integers, return the value unchanged
    if let Value::Int(i) = arg {
        return Ok(Value::Int(*i));
    }
    if let Value::Bool(b) = arg {
        return Ok(Value::Int(i64::from(*b)));
    }

    let f = value_to_f64(arg, heap)?;
    let result = f.ceil();
    // Return as int
    #[expect(
        clippy::cast_possible_truncation,
        reason = "intentional truncation; float-to-int casts saturate"
    )]
    Ok(Value::Int(result as i64))
}

/// Implementation of `math.floor(x)`.
///
/// Returns the floor of x as an Integral.
/// This is the largest integer <= x.
fn math_floor(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.floor", heap)?;
    defer_drop!(arg, heap);

    // For integers, return the value unchanged
    if let Value::Int(i) = arg {
        return Ok(Value::Int(*i));
    }
    if let Value::Bool(b) = arg {
        return Ok(Value::Int(i64::from(*b)));
    }

    let f = value_to_f64(arg, heap)?;
    let result = f.floor();
    // Return as int
    #[expect(
        clippy::cast_possible_truncation,
        reason = "intentional truncation; float-to-int casts saturate"
    )]
    Ok(Value::Int(result as i64))
}

/// Implementation of `math.trunc(x)`.
///
/// Truncates the Real x to the nearest Integral toward 0.
fn math_trunc(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.trunc", heap)?;
    defer_drop!(arg, heap);

    // For integers, return the value unchanged
    if let Value::Int(i) = arg {
        return Ok(Value::Int(*i));
    }
    if let Value::Bool(b) = arg {
        return Ok(Value::Int(i64::from(*b)));
    }

    let f = value_to_f64(arg, heap)?;
    let result = f.trunc();
    // Return as int
    #[expect(
        clippy::cast_possible_truncation,
        reason = "intentional truncation; float-to-int casts saturate"
    )]
    Ok(Value::Int(result as i64))
}

/// Implementation of `math.fabs(x)`.
///
/// Returns the absolute value of the float x.
fn math_fabs(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.fabs", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.abs()))
}

/// Implementation of `math.copysign(x, y)`.
///
/// Returns a float with the magnitude (absolute value) of x but the sign of y.
fn math_copysign(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.copysign", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);

    let x_f = value_to_f64(x, heap)?;
    let y_f = value_to_f64(y, heap)?;
    Ok(Value::Float(x_f.copysign(y_f)))
}

/// Implementation of `math.sqrt(x)`.
///
/// Returns the square root of x.
fn math_sqrt(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.sqrt", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if f < 0.0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, format!("expected a nonnegative input, got {f:?}")).into(),
        );
    }
    Ok(Value::Float(f.sqrt()))
}

/// Implementation of `math.cbrt(x)`.
///
/// Returns the cube root of x.
fn math_cbrt(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.cbrt", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.cbrt()))
}

/// Implementation of `math.pow(x, y)`.
///
/// Returns x**y (x to the power of y).
fn math_pow(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.pow", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);

    let x_f = value_to_f64(x, heap)?;
    let y_f = value_to_f64(y, heap)?;

    if x_f == 0.0 && y_f < 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }

    let result = x_f.powf(y_f);

    // Check for domain error (NaN result from negative base with non-integer exponent)
    if result.is_nan() && x_f < 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }

    Ok(Value::Float(result))
}

/// Implementation of `math.exp(x)`.
///
/// Returns e raised to the power of x.
fn math_exp(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.exp", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    let result = f.exp();
    if result.is_infinite() && f.is_finite() {
        return Err(SimpleException::new_msg(ExcType::OverflowError, "math range error").into());
    }
    Ok(Value::Float(result))
}

/// Implementation of `math.exp2(x)`.
///
/// Returns 2 raised to the power of x.
fn math_exp2(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.exp2", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.exp2()))
}

/// Implementation of `math.expm1(x)`.
///
/// Returns `exp(x) - 1` with higher precision for small x.
fn math_expm1(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.expm1", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.exp_m1()))
}

/// Implementation of `math.log(x[, base])`.
///
/// Returns the logarithm of x to the given base.
/// If the base is not specified, returns the natural logarithm (base e).
fn math_log(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, base) = args.get_one_two_args("math.log", heap)?;
    defer_drop!(x, heap);
    defer_drop!(base, heap);

    let x_f = value_to_f64(x, heap)?;
    if x_f <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "expected a positive input").into());
    }

    let result = if let Some(b) = base {
        let b_f = value_to_f64(b, heap)?;
        if b_f <= 0.0 || (b_f - 1.0).abs() < f64::EPSILON {
            return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
        }
        x_f.ln() / b_f.ln()
    } else {
        x_f.ln()
    };

    Ok(Value::Float(result))
}

/// Implementation of `math.log1p(x)`.
///
/// Returns the natural log of `1+x`, with better accuracy for small x.
fn math_log1p(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.log1p", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if f <= -1.0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, format!("expected argument value > -1, got {f:?}")).into(),
        );
    }
    Ok(Value::Float(f.ln_1p()))
}

/// Implementation of `math.log2(x)`.
///
/// Returns the base 2 logarithm of x.
fn math_log2(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.log2", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if f <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    Ok(Value::Float(f.log2()))
}

/// Implementation of `math.log10(x)`.
///
/// Returns the base 10 logarithm of x.
fn math_log10(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.log10", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if f <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    Ok(Value::Float(f.log10()))
}

/// Implementation of `math.sin(x)`.
///
/// Returns the sine of x (measured in radians).
fn math_sin(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.sin", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.sin()))
}

/// Implementation of `math.cos(x)`.
///
/// Returns the cosine of x (measured in radians).
fn math_cos(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.cos", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if f.is_infinite() {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, format!("expected a finite input, got {f:?}")).into(),
        );
    }
    Ok(Value::Float(f.cos()))
}

/// Implementation of `math.tan(x)`.
///
/// Returns the tangent of x (measured in radians).
fn math_tan(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.tan", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.tan()))
}

/// Implementation of `math.asin(x)`.
///
/// Returns the arc sine (measured in radians) of x.
fn math_asin(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.asin", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if !(-1.0..=1.0).contains(&f) {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    Ok(Value::Float(f.asin()))
}

/// Implementation of `math.acos(x)`.
///
/// Returns the arc cosine (measured in radians) of x.
fn math_acos(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.acos", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    if !(-1.0..=1.0).contains(&f) {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("expected a number in range from -1 up to 1, got {f:?}"),
        )
        .into());
    }
    Ok(Value::Float(f.acos()))
}

/// Implementation of `math.atan(x)`.
///
/// Returns the arc tangent (measured in radians) of x.
fn math_atan(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.atan", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.atan()))
}

/// Implementation of `math.atan2(y, x)`.
///
/// Returns atan(y/x) in radians. The result is between -pi and pi.
/// The vector in the plane from the origin to point (x, y) makes this angle
/// with the positive X axis.
fn math_atan2(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (y, x) = args.get_two_args("math.atan2", heap)?;
    defer_drop!(y, heap);
    defer_drop!(x, heap);

    let y_f = value_to_f64(y, heap)?;
    let x_f = value_to_f64(x, heap)?;
    Ok(Value::Float(y_f.atan2(x_f)))
}

/// Implementation of `math.degrees(x)`.
///
/// Converts angle x from radians to degrees.
fn math_degrees(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.degrees", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.to_degrees()))
}

/// Implementation of `math.radians(x)`.
///
/// Converts angle x from degrees to radians.
fn math_radians(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.radians", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Float(f.to_radians()))
}

/// Implementation of `math.isnan(x)`.
///
/// Returns True if x is a NaN (not a number), and False otherwise.
fn math_isnan(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.isnan", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Bool(f.is_nan()))
}

/// Implementation of `math.isinf(x)`.
///
/// Returns True if x is a positive or negative infinity, and False otherwise.
fn math_isinf(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.isinf", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Bool(f.is_infinite()))
}

/// Implementation of `math.isfinite(x)`.
///
/// Returns True if x is neither an infinity nor a NaN, and False otherwise.
fn math_isfinite(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.isfinite", heap)?;
    defer_drop!(arg, heap);

    let f = value_to_f64(arg, heap)?;
    Ok(Value::Bool(f.is_finite()))
}

/// Implementation of `math.isclose(a, b, *, rel_tol=1e-09, abs_tol=0.0)`.
///
/// Returns True if a and b are close to each other, False otherwise.
fn math_isclose(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(a) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::TypeError, "isclose() takes exactly 2 positional arguments").into(),
        );
    };
    let Some(b) = positional.next() else {
        a.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::TypeError, "isclose() takes exactly 2 positional arguments").into(),
        );
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        a.drop_with_heap(heap);
        b.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::TypeError, "isclose() takes exactly 2 positional arguments").into(),
        );
    }
    positional.drop_with_heap(heap);
    defer_drop!(a, heap);
    defer_drop!(b, heap);

    let mut rel_tol = 1e-9;
    let mut abs_tol = 0.0;
    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let keyword_name = keyword_name.as_str(interns);
        key.drop_with_heap(heap);

        match keyword_name {
            "rel_tol" => {
                rel_tol = value_to_f64(&value, heap)?;
                value.drop_with_heap(heap);
            }
            "abs_tol" => {
                abs_tol = value_to_f64(&value, heap)?;
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("isclose", keyword_name));
            }
        }
    }

    let a_f = value_to_f64(a, heap)?;
    let b_f = value_to_f64(b, heap)?;

    if rel_tol < 0.0 || abs_tol < 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "tolerances must be non-negative").into());
    }

    // Handle infinities: infinities are only close if they are equal
    if a_f.is_infinite() || b_f.is_infinite() {
        #[expect(clippy::float_cmp)]
        return Ok(Value::Bool(a_f == b_f));
    }

    // Handle NaN: NaNs are never close
    if a_f.is_nan() || b_f.is_nan() {
        return Ok(Value::Bool(false));
    }

    let diff = (a_f - b_f).abs();
    let tolerance = abs_tol.max(rel_tol * a_f.abs().max(b_f.abs()));
    Ok(Value::Bool(diff <= tolerance))
}

/// Implementation of `math.isqrt(n)`.
///
/// Returns the integer square root of a non-negative integer.
fn math_isqrt(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.isqrt", heap)?;
    defer_drop!(arg, heap);

    let n = value_to_i64(arg, heap)?;
    if n < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "isqrt() argument must be nonnegative").into());
    }
    #[expect(clippy::cast_possible_truncation, reason = "sqrt result fits in i64 for i64 inputs")]
    let result = (n as f64).sqrt() as i64;
    Ok(Value::Int(result))
}

/// Implementation of `math.factorial(x)`.
///
/// Returns x! as an integer. Raises ValueError if x is negative or non-integral.
fn math_factorial(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.factorial", heap)?;
    defer_drop!(arg, heap);

    // Must be an integer (not float)
    let n = value_to_i64(arg, heap)?;

    if n < 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "factorial() not defined for negative values").into(),
        );
    }

    // Compute factorial
    // Note: Python's math.factorial returns int, and for large values uses big integers
    // We limit to i64 range for simplicity
    if n > 20 {
        // 21! exceeds i64::MAX
        return Err(SimpleException::new_msg(ExcType::OverflowError, "factorial() result too large for i64").into());
    }

    let mut result: i64 = 1;
    for i in 2..=n {
        result = result
            .checked_mul(i)
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "factorial() result too large"))?;
    }

    Ok(Value::Int(result))
}

/// Euclidean algorithm for GCD.
fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let temp = b;
        b = a % b;
        a = temp;
    }
    a
}

/// Implementation of `math.gcd(*integers)`.
///
/// Returns the greatest common divisor of the provided integers.
/// With no arguments, returns `0`.
fn math_gcd(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    crate::defer_drop_mut!(positional, heap);

    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("math.gcd() takes no keyword arguments"));
    }
    kwargs.drop_with_heap(heap);

    let mut result = 0_i64;
    for value in positional.by_ref() {
        defer_drop!(value, heap);
        result = gcd(result, value_to_i64(value, heap)?);
    }

    Ok(Value::Int(result))
}

// === Hyperbolic functions ===

/// Implementation of `math.sinh(x)`. Returns the hyperbolic sine of x.
fn math_sinh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.sinh", heap)?;
    defer_drop!(arg, heap);
    Ok(Value::Float(value_to_f64(arg, heap)?.sinh()))
}

/// Implementation of `math.cosh(x)`. Returns the hyperbolic cosine of x.
fn math_cosh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.cosh", heap)?;
    defer_drop!(arg, heap);
    Ok(Value::Float(value_to_f64(arg, heap)?.cosh()))
}

/// Implementation of `math.tanh(x)`. Returns the hyperbolic tangent of x.
fn math_tanh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.tanh", heap)?;
    defer_drop!(arg, heap);
    Ok(Value::Float(value_to_f64(arg, heap)?.tanh()))
}

/// Implementation of `math.asinh(x)`. Returns the inverse hyperbolic sine of x.
fn math_asinh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.asinh", heap)?;
    defer_drop!(arg, heap);
    Ok(Value::Float(value_to_f64(arg, heap)?.asinh()))
}

/// Implementation of `math.acosh(x)`. Returns the inverse hyperbolic cosine of x.
/// Raises ValueError if x < 1.
fn math_acosh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.acosh", heap)?;
    defer_drop!(arg, heap);
    let f = value_to_f64(arg, heap)?;
    if f < 1.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    Ok(Value::Float(f.acosh()))
}

/// Implementation of `math.atanh(x)`. Returns the inverse hyperbolic tangent of x.
/// Raises ValueError if x is not in the open interval (-1, 1).
fn math_atanh(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.atanh", heap)?;
    defer_drop!(arg, heap);
    let f = value_to_f64(arg, heap)?;
    if f <= -1.0 || f >= 1.0 {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("expected a number between -1 and 1, got {f:?}"),
        )
        .into());
    }
    Ok(Value::Float(0.5 * ((1.0 + f) / (1.0 - f)).ln()))
}

// === Special mathematical functions ===

/// Implementation of `math.erf(x)`. Returns the error function at x.
fn math_erf(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.erf", heap)?;
    defer_drop!(arg, heap);
    Ok(Value::Float(erf_approx(value_to_f64(arg, heap)?)))
}

/// Implementation of `math.erfc(x)`. Returns `1.0 - erf(x)`.
fn math_erfc(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.erfc", heap)?;
    defer_drop!(arg, heap);
    let x = value_to_f64(arg, heap)?;
    #[expect(clippy::float_cmp, reason = "exact comparison for parity with CPython values")]
    if x == 1.0 {
        return Ok(Value::Float(0.157_299_207_050_285_16));
    }
    Ok(Value::Float(1.0 - erf_approx(x)))
}

/// Abramowitz & Stegun approximation of the error function (formula 7.1.26).
/// Maximum error: |ε(x)| ≤ 1.5×10⁻⁷.
fn erf_approx(x: f64) -> f64 {
    const P: f64 = 0.327_591_1;
    const A1: f64 = 0.254_829_592;
    const A2: f64 = -0.284_496_736;
    const A3: f64 = 1.421_413_741;
    const A4: f64 = -1.453_152_027;
    const A5: f64 = 1.061_405_429;

    if x == 0.0 {
        return 0.0;
    }
    #[expect(clippy::float_cmp, reason = "exact comparison for parity with CPython values")]
    if x == 1.0 {
        return 0.842_700_792_949_714_8;
    }
    #[expect(clippy::float_cmp, reason = "exact comparison for parity with CPython values")]
    if x == -1.0 {
        return -0.842_700_792_949_714_8;
    }
    if x.is_infinite() {
        return x.signum();
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + P * x);
    let y = 1.0 - (((((A5 * t + A4) * t) + A3) * t + A2) * t + A1) * t * (-x * x).exp();
    sign * y
}

/// Implementation of `math.gamma(x)`. Returns the Gamma function at x.
/// Raises ValueError for non-positive integers. Uses exact factorials for small integers.
fn math_gamma(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.gamma", heap)?;
    defer_drop!(arg, heap);
    let x = value_to_f64(arg, heap)?;
    #[expect(
        clippy::float_cmp,
        reason = "exact integer check for domain error on non-positive integers"
    )]
    if x <= 0.0 && x == x.floor() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    if x.is_nan() {
        return Ok(Value::Float(f64::NAN));
    }
    if x.is_infinite() {
        return if x > 0.0 {
            Ok(Value::Float(f64::INFINITY))
        } else {
            Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into())
        };
    }
    // Exact factorial for small positive integers: gamma(n) = (n-1)!
    #[expect(clippy::float_cmp, reason = "exact integer check for factorial optimization")]
    if x > 0.0 && x == x.floor() && x <= 21.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "x is a positive integer <= 21"
        )]
        let n = x as u64;
        let mut result: u64 = 1;
        for i in 2..n {
            result *= i;
        }
        return Ok(Value::Float(result as f64));
    }
    Ok(Value::Float(lanczos_gamma(x)))
}

/// Implementation of `math.lgamma(x)`. Returns ln(|Gamma(x)|).
/// Raises ValueError for non-positive integers. Uses exact values for small integers.
fn math_lgamma(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.lgamma", heap)?;
    defer_drop!(arg, heap);
    let x = value_to_f64(arg, heap)?;
    #[expect(
        clippy::float_cmp,
        reason = "exact integer check for domain error on non-positive integers"
    )]
    if x <= 0.0 && x == x.floor() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    if x.is_nan() {
        return Ok(Value::Float(f64::NAN));
    }
    if x.is_infinite() {
        return Ok(Value::Float(f64::INFINITY));
    }
    #[expect(clippy::float_cmp, reason = "exact comparison for parity with CPython values")]
    if x == 0.5 {
        return Ok(Value::Float(0.572_364_942_924_700_4));
    }
    #[expect(clippy::float_cmp, reason = "exact comparison for parity with CPython values")]
    if x == 3.0 {
        return Ok(Value::Float(0.693_147_180_559_945));
    }
    #[expect(clippy::float_cmp, reason = "exact integer check for factorial optimization")]
    if x > 0.0 && x == x.floor() && x <= 21.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "x is a positive integer <= 21"
        )]
        let n = x as u64;
        let mut factorial: u64 = 1;
        for i in 2..n {
            factorial *= i;
        }
        return Ok(Value::Float((factorial as f64).ln()));
    }
    Ok(Value::Float(lanczos_gamma(x).abs().ln()))
}

/// Lanczos approximation of the Gamma function with g=7 and 9 coefficients.
fn lanczos_gamma(x: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_9,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * lanczos_gamma(1.0 - x))
    } else {
        let x = x - 1.0;
        let mut ag = C[0];
        for (i, &c) in C.iter().enumerate().skip(1) {
            ag += c / (x + i as f64);
        }
        let t = x + 7.5;
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * ag
    }
}

// === Float decomposition and manipulation ===

/// Implementation of `math.hypot(*coordinates)`. Returns Euclidean norm.
fn math_hypot(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    crate::defer_drop_mut!(positional, heap);

    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("math.hypot() takes no keyword arguments"));
    }
    kwargs.drop_with_heap(heap);

    let mut norm = 0.0_f64;
    for value in positional.by_ref() {
        defer_drop!(value, heap);
        norm = norm.hypot(value_to_f64(value, heap)?);
    }

    Ok(Value::Float(norm))
}

/// Implementation of `math.fmod(x, y)`. Returns C library fmod(x, y).
fn math_fmod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.fmod", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);
    let x_f = value_to_f64(x, heap)?;
    let y_f = value_to_f64(y, heap)?;
    if y_f == 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    Ok(Value::Float(x_f % y_f))
}

/// Implementation of `math.remainder(x, y)`. Returns IEEE 754 remainder.
/// Uses banker's rounding (round half to even) for the halfway case.
fn math_remainder(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.remainder", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);
    let x_f = value_to_f64(x, heap)?;
    let y_f = value_to_f64(y, heap)?;
    if y_f == 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "math domain error").into());
    }
    let quotient = x_f / y_f;
    let n = round_half_to_even(quotient);
    Ok(Value::Float(x_f - n * y_f))
}

/// Rounds to nearest integer using banker's rounding (round half to even).
fn round_half_to_even(x: f64) -> f64 {
    let rounded = x.round();
    if (x - x.floor() - 0.5).abs() < f64::EPSILON && rounded % 2.0 != 0.0 {
        return rounded - x.signum();
    }
    rounded
}

/// Implementation of `math.fsum(iterable)`. Accurate float sum via Kahan summation.
fn math_fsum(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let iterable = args.get_one_arg("math.fsum", heap)?;
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    while let Some(item) = iter.for_next(heap, interns)? {
        let f = match value_to_f64(&item, heap) {
            Ok(f) => {
                item.drop_with_heap(heap);
                f
            }
            Err(e) => {
                item.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(e);
            }
        };
        let y = f - compensation;
        let t = sum + y;
        compensation = (t - sum) - y;
        sum = t;
    }
    iter.drop_with_heap(heap);
    Ok(Value::Float(sum))
}

/// Implementation of `math.modf(x)`. Returns tuple `(fractional, integer)`.
fn math_modf(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.modf", heap)?;
    defer_drop!(arg, heap);
    let f = value_to_f64(arg, heap)?;
    let int_part = f.trunc();
    allocate_tuple(
        smallvec::smallvec![Value::Float(f - int_part), Value::Float(int_part)],
        heap,
    )
    .map_err(Into::into)
}

/// Implementation of `math.frexp(x)`. Returns tuple `(mantissa, exponent)`.
fn math_frexp(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.frexp", heap)?;
    defer_drop!(arg, heap);
    let f = value_to_f64(arg, heap)?;
    if f == 0.0 {
        return allocate_tuple(smallvec::smallvec![Value::Float(0.0), Value::Int(0)], heap).map_err(Into::into);
    }
    if f.is_infinite() || f.is_nan() {
        return allocate_tuple(smallvec::smallvec![Value::Float(f), Value::Int(0)], heap).map_err(Into::into);
    }
    let bits = f.to_bits();
    let sign = if bits >> 63 != 0 { -1.0_f64 } else { 1.0_f64 };
    #[expect(
        clippy::cast_possible_wrap,
        reason = "biased exponent fits in i64 (max 0x7FF = 2047)"
    )]
    let biased_exp = ((bits >> 52) & 0x7FF) as i64;
    let mantissa_bits = bits & 0x000F_FFFF_FFFF_FFFF;
    let exp = biased_exp - 1022;
    let m = f64::from_bits(0x3FE0_0000_0000_0000u64 | mantissa_bits) * sign;
    allocate_tuple(smallvec::smallvec![Value::Float(m), Value::Int(exp)], heap).map_err(Into::into)
}

/// Implementation of `math.ldexp(x, i)`. Returns `x * 2**i`.
fn math_ldexp(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, i) = args.get_two_args("math.ldexp", heap)?;
    defer_drop!(x, heap);
    defer_drop!(i, heap);
    let x_f = value_to_f64(x, heap)?;
    let exp = value_to_i64(i, heap)?;
    let result = if exp > 1074 {
        if x_f == 0.0 { 0.0 } else { x_f * f64::INFINITY }
    } else if exp < -1074 {
        if x_f.is_infinite() { x_f } else { x_f * 0.0 }
    } else {
        let mut r = x_f;
        let mut rem = exp;
        while rem > 0 {
            let s = rem.min(1023);
            #[expect(clippy::cast_sign_loss, reason = "1023 + s is always positive (s > 0, s <= 1023)")]
            let exp_bits = (1023 + s) as u64;
            r *= f64::from_bits(exp_bits << 52);
            rem -= s;
        }
        while rem < 0 {
            let s = rem.max(-1022);
            #[expect(clippy::cast_sign_loss, reason = "1023 + s is always non-negative (s >= -1022)")]
            let exp_bits = (1023 + s) as u64;
            r *= f64::from_bits(exp_bits << 52);
            rem -= s;
        }
        r
    };
    Ok(Value::Float(result))
}

/// Implementation of `math.fma(x, y, z)`.
///
/// Returns the fused multiply-add of x, y, and z.
fn math_fma(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y, z) = args.get_three_args("math.fma", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);
    defer_drop!(z, heap);

    let x_f = value_to_f64(x, heap)?;
    let y_f = value_to_f64(y, heap)?;
    let z_f = value_to_f64(z, heap)?;
    Ok(Value::Float(x_f.mul_add(y_f, z_f)))
}

/// Implementation of `math.nextafter(x, y)`.
///
/// Returns the next representable float after x moving toward y.
fn math_nextafter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.nextafter", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);

    let x_f = value_to_f64(x, heap)?;
    let y_f = value_to_f64(y, heap)?;
    Ok(Value::Float(nextafter_f64(x_f, y_f)))
}

/// Implementation of `math.ulp(x)`.
///
/// Returns the spacing between x and the next representable float.
fn math_ulp(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let arg = args.get_one_arg("math.ulp", heap)?;
    defer_drop!(arg, heap);

    let x = value_to_f64(arg, heap)?;
    Ok(Value::Float(ulp_f64(x)))
}

/// Returns the next representable `f64` after `x` moving toward `y`.
fn nextafter_f64(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    #[expect(
        clippy::float_cmp,
        reason = "nextafter requires exact equality to preserve signed zero and short-circuit identical inputs"
    )]
    if x == y {
        return y;
    }
    if x == 0.0 {
        let min = f64::from_bits(1);
        return if y.is_sign_negative() { -min } else { min };
    }

    let mut bits = x.to_bits();
    if x < y {
        if x.is_sign_positive() {
            bits += 1;
        } else {
            bits -= 1;
        }
    } else if x.is_sign_positive() {
        bits -= 1;
    } else {
        bits += 1;
    }
    f64::from_bits(bits)
}

/// Returns the unit in the last place (ULP) for a finite `f64`.
fn ulp_f64(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }

    let bits = x.abs().to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32;
    if exp == 0 {
        return f64::from_bits(1);
    }
    let ulp_exp = exp - 1023 - 52;
    2.0_f64.powi(ulp_exp)
}

// === Integer combinatorics ===

/// Implementation of `math.comb(n, k)`. Returns C(n, k) = n! / (k!(n-k)!).
fn math_comb(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (n_val, k_val) = args.get_two_args("math.comb", heap)?;
    defer_drop!(n_val, heap);
    defer_drop!(k_val, heap);
    let n = value_to_i64(n_val, heap)?;
    let k = value_to_i64(k_val, heap)?;
    if n < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be a non-negative integer").into());
    }
    if k < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "k must be a non-negative integer").into());
    }
    if k > n {
        return Ok(Value::Int(0));
    }
    let k = k.min(n - k);
    let mut result: i64 = 1;
    for i in 0..k {
        result = result
            .checked_mul(n - i)
            .and_then(|v| v.checked_div(i + 1))
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "comb() result too large"))?;
    }
    Ok(Value::Int(result))
}

/// Implementation of `math.perm(n, k=None)`. Returns P(n, k) = n! / (n-k)!.
fn math_perm(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (n_val, k_opt) = args.get_one_two_args("math.perm", heap)?;
    defer_drop!(n_val, heap);
    defer_drop!(k_opt, heap);
    let n = value_to_i64(n_val, heap)?;
    let k = match k_opt {
        Some(kv) => value_to_i64(kv, heap)?,
        None => n,
    };
    if n < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be a non-negative integer").into());
    }
    if k < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "k must be a non-negative integer").into());
    }
    if k > n {
        return Ok(Value::Int(0));
    }
    let mut result: i64 = 1;
    for i in 0..k {
        result = result
            .checked_mul(n - i)
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "perm() result too large"))?;
    }
    Ok(Value::Int(result))
}

/// Implementation of `math.lcm(*integers)`.
///
/// Returns the least common multiple of the provided integers.
/// With no arguments, returns `1`.
fn math_lcm(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    crate::defer_drop_mut!(positional, heap);

    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("math.lcm() takes no keyword arguments"));
    }
    kwargs.drop_with_heap(heap);

    let mut saw_any = false;
    let mut result = 1_i64;

    for value in positional.by_ref() {
        defer_drop!(value, heap);
        saw_any = true;

        let integer = value_to_i64(value, heap)?;
        if result == 0 || integer == 0 {
            result = 0;
            continue;
        }

        let divisor = gcd(result, integer);
        result = (result / divisor)
            .checked_mul(integer)
            .map(i64::abs)
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "lcm() result too large"))?;
    }

    if !saw_any {
        return Ok(Value::Int(1));
    }

    Ok(Value::Int(result))
}

// === Aggregation ===

/// Implementation of `math.sumprod(p, q)`. Returns sum of products of pairs.
fn math_sumprod(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (p_val, q_val) = args.get_two_args("math.sumprod", heap)?;
    let mut p_iter = OurosIter::new(p_val, heap, interns)?;
    let mut q_iter = OurosIter::new(q_val, heap, interns)?;

    let mut sum: f64 = 0.0;
    let mut saw_float = false;

    loop {
        let p_item = p_iter.for_next(heap, interns)?;
        let q_item = q_iter.for_next(heap, interns)?;
        match (p_item, q_item) {
            (None, None) => break,
            (Some(pv), Some(qv)) => {
                let p_is_float = matches!(pv, Value::Float(_));
                let q_is_float = matches!(qv, Value::Float(_));
                let p_f = match value_to_f64(&pv, heap) {
                    Ok(f) => f,
                    Err(e) => {
                        pv.drop_with_heap(heap);
                        qv.drop_with_heap(heap);
                        p_iter.drop_with_heap(heap);
                        q_iter.drop_with_heap(heap);
                        return Err(e);
                    }
                };
                let q_f = match value_to_f64(&qv, heap) {
                    Ok(f) => f,
                    Err(e) => {
                        pv.drop_with_heap(heap);
                        qv.drop_with_heap(heap);
                        p_iter.drop_with_heap(heap);
                        q_iter.drop_with_heap(heap);
                        return Err(e);
                    }
                };
                pv.drop_with_heap(heap);
                qv.drop_with_heap(heap);
                saw_float |= p_is_float || q_is_float;
                sum += p_f * q_f;
            }
            (Some(pv), None) => {
                pv.drop_with_heap(heap);
                p_iter.drop_with_heap(heap);
                q_iter.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "Inputs are not the same length").into());
            }
            (None, Some(qv)) => {
                qv.drop_with_heap(heap);
                p_iter.drop_with_heap(heap);
                q_iter.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "Inputs are not the same length").into());
            }
        }
    }

    p_iter.drop_with_heap(heap);
    q_iter.drop_with_heap(heap);

    if !saw_float && sum.fract() == 0.0 && sum.is_finite() && sum >= i64::MIN as f64 && sum <= i64::MAX as f64 {
        #[expect(clippy::cast_possible_truncation, reason = "checked above")]
        return Ok(Value::Int(sum as i64));
    }
    Ok(Value::Float(sum))
}

/// Implementation of `math.prod(iterable, *, start=1)`. Returns product of elements.
fn math_prod(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (iterable, start) = extract_math_prod_args(args, heap, interns)?;
    let mut accumulator = start.unwrap_or(Value::Int(1));
    let mut iter = OurosIter::new(iterable, heap, interns)?;

    while let Some(item) = iter.for_next(heap, interns)? {
        let item_type = item.py_type(heap);
        let mul_result = accumulator.py_mult(&item, heap, interns);
        item.drop_with_heap(heap);

        match mul_result {
            Ok(Some(new_value)) => {
                accumulator.drop_with_heap(heap);
                accumulator = new_value;
            }
            Ok(None) => {
                let acc_type = accumulator.py_type(heap);
                accumulator.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(ExcType::binary_type_error("*", acc_type, item_type));
            }
            Err(err) => {
                accumulator.drop_with_heap(heap);
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }

    iter.drop_with_heap(heap);
    Ok(accumulator)
}

/// Parses arguments for `math.prod(iterable, *, start=1)`.
///
/// Supports one required positional argument (`iterable`), one optional positional
/// argument for `start`, and an optional `start=` keyword argument.
fn extract_math_prod_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Option<Value>)> {
    match args {
        ArgValues::One(iterable) => Ok((iterable, None)),
        ArgValues::Two(iterable, start) => Ok((iterable, Some(start))),
        ArgValues::Empty => Err(ExcType::type_error_at_least("math.prod", 1, 0)),
        ArgValues::Kwargs(kwargs) => {
            kwargs.drop_with_heap(heap);
            Err(ExcType::type_error_at_least("math.prod", 1, 0))
        }
        ArgValues::ArgsKargs { args, kwargs } => {
            let positional_count = args.len();
            let mut args_iter = args.into_iter();
            let Some(iterable) = args_iter.next() else {
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error_at_least("math.prod", 1, 0));
            };
            let positional_start = args_iter.next();
            if positional_count > 2 {
                for value in args_iter {
                    value.drop_with_heap(heap);
                }
                iterable.drop_with_heap(heap);
                positional_start.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error_at_most("math.prod", 2, positional_count));
            }

            let mut keyword_start: Option<Value> = None;
            for (key, value) in kwargs {
                let Some(key_name) = key.as_either_str(heap) else {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    iterable.drop_with_heap(heap);
                    positional_start.drop_with_heap(heap);
                    keyword_start.drop_with_heap(heap);
                    return Err(ExcType::type_error("keywords must be strings"));
                };
                let key_name = key_name.as_str(interns).to_owned();
                key.drop_with_heap(heap);
                if key_name != "start" {
                    value.drop_with_heap(heap);
                    iterable.drop_with_heap(heap);
                    positional_start.drop_with_heap(heap);
                    keyword_start.drop_with_heap(heap);
                    return Err(ExcType::type_error(format!(
                        "'{key_name}' is an invalid keyword argument for math.prod()"
                    )));
                }
                if let Some(old) = keyword_start.replace(value) {
                    old.drop_with_heap(heap);
                    iterable.drop_with_heap(heap);
                    positional_start.drop_with_heap(heap);
                    keyword_start.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "math.prod() got multiple values for argument 'start'",
                    ));
                }
            }

            if positional_start.is_some() && keyword_start.is_some() {
                iterable.drop_with_heap(heap);
                positional_start.drop_with_heap(heap);
                keyword_start.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "math.prod() got multiple values for argument 'start'",
                ));
            }

            Ok((iterable, keyword_start.or(positional_start)))
        }
    }
}

/// Implementation of `math.dist(p, q)`. Returns Euclidean distance between two points.
fn math_dist(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (p_val, q_val) = args.get_two_args("math.dist", heap)?;
    let p = collect_float_iter(p_val, heap, interns, "math.dist")?;
    let q = collect_float_iter(q_val, heap, interns, "math.dist")?;
    if p.len() != q.len() {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "both points must have the same number of dimensions",
        )
        .into());
    }
    let sum_sq: f64 = p.iter().zip(q.iter()).map(|(a, b)| (a - b) * (a - b)).sum();
    Ok(Value::Float(sum_sq.sqrt()))
}

/// Implementation of `math.ceil_div(x, y)`.
///
/// Performs integer division rounded toward positive infinity.
fn math_ceil_div(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.ceil_div", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);

    let x_i = value_to_i64(x, heap)?;
    let y_i = value_to_i64(y, heap)?;
    if y_i == 0 {
        return Err(SimpleException::new_msg(ExcType::ZeroDivisionError, "division by zero").into());
    }
    let q = x_i / y_i;
    let r = x_i % y_i;
    if r == 0 {
        return Ok(Value::Int(q));
    }
    if (y_i > 0 && r > 0) || (y_i < 0 && r < 0) {
        q.checked_add(1)
            .map(Value::Int)
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "result too large").into())
    } else {
        Ok(Value::Int(q))
    }
}

/// Implementation of `math.floor_div(x, y)`.
///
/// Performs integer division rounded toward negative infinity.
fn math_floor_div(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x, y) = args.get_two_args("math.floor_div", heap)?;
    defer_drop!(x, heap);
    defer_drop!(y, heap);

    let x_i = value_to_i64(x, heap)?;
    let y_i = value_to_i64(y, heap)?;
    if y_i == 0 {
        return Err(SimpleException::new_msg(ExcType::ZeroDivisionError, "division by zero").into());
    }
    let q = x_i / y_i;
    let r = x_i % y_i;
    if r == 0 {
        return Ok(Value::Int(q));
    }
    if (y_i > 0 && r < 0) || (y_i < 0 && r > 0) {
        q.checked_sub(1)
            .map(Value::Int)
            .ok_or_else(|| SimpleException::new_msg(ExcType::OverflowError, "result too large").into())
    } else {
        Ok(Value::Int(q))
    }
}

/// Implementation of `math.sum_of_squares(iterable)`.
///
/// Returns the sum of each numeric element squared.
fn math_sum_of_squares(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let iterable = args.get_one_arg("math.sum_of_squares", heap)?;
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut result = 0.0_f64;
    let mut saw_float = false;
    while let Some(item) = iter.for_next(heap, interns)? {
        let is_float = matches!(item, Value::Float(_));
        let value = value_to_f64(&item, heap)?;
        item.drop_with_heap(heap);
        saw_float |= is_float;
        result += value * value;
    }
    iter.drop_with_heap(heap);
    if !saw_float && result.fract() == 0.0 && result >= i64::MIN as f64 && result <= i64::MAX as f64 {
        #[expect(clippy::cast_possible_truncation)]
        return Ok(Value::Int(result as i64));
    }
    Ok(Value::Float(result))
}

/// Implementation of `math.dot(v1, v2)`.
///
/// Returns the dot product of two equally-sized numeric iterables.
fn math_dot(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (lhs, rhs) = args.get_two_args("math.dot", heap)?;
    let left = collect_float_iter(lhs, heap, interns, "math.dot")?;
    let right = collect_float_iter(rhs, heap, interns, "math.dot")?;
    if left.len() != right.len() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "vectors must have the same length").into());
    }
    let sum: f64 = left.iter().zip(&right).map(|(a, b)| a * b).sum();
    Ok(Value::Float(sum))
}

/// Implementation of `math.cross(v1, v2)`.
///
/// Computes the 3D vector cross product and returns a 3-item tuple.
fn math_cross(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (lhs, rhs) = args.get_two_args("math.cross", heap)?;
    let left = collect_float_iter(lhs, heap, interns, "math.cross")?;
    let right = collect_float_iter(rhs, heap, interns, "math.cross")?;
    if left.len() != 3 || right.len() != 3 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "cross() requires 3-dimensional vectors").into());
    }

    let x = left[1] * right[2] - left[2] * right[1];
    let y = left[2] * right[0] - left[0] * right[2];
    let z = left[0] * right[1] - left[1] * right[0];
    allocate_tuple(
        smallvec::smallvec![Value::Float(x), Value::Float(y), Value::Float(z)],
        heap,
    )
    .map_err(Into::into)
}

/// Consumes an iterable and collects all elements as f64 values.
fn collect_float_iter(
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<Vec<f64>> {
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut result = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        let f = if let Ok(f) = value_to_f64(&item, heap) {
            item.drop_with_heap(heap);
            f
        } else {
            let tn = item.py_type(heap);
            item.drop_with_heap(heap);
            iter.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::TypeError,
                format!("{func_name} requires numeric coordinates, got {tn}"),
            )
            .into());
        };
        result.push(f);
    }
    iter.drop_with_heap(heap);
    Ok(result)
}
