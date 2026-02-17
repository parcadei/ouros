//! Implementation of the `fractions` module.
//!
//! Provides the `Fraction` class for rational number arithmetic.
//! Fractions are stored as normalized pairs of integers (numerator, denominator)
//! with arbitrary precision support.
//!
//! # Examples
//! ```python
//! from fractions import Fraction
//!
//! # Basic construction
//! Fraction(1, 2)      # 1/2
//! Fraction(3)         # 3/1
//! Fraction()          # 0/1
//!
//! # From string
//! Fraction("3/7")     # 3/7
//! Fraction("1.5")     # 3/2
//!
//! # From float
//! Fraction(0.5)       # 1/2
//! Fraction.from_float(0.1)
//!
//! # Arithmetic
//! Fraction(1, 2) + Fraction(1, 3)  # 5/6
//!
//! # Properties
//! f = Fraction(3, 4)
//! f.numerator         # 3
//! f.denominator       # 4
//!
//! # Methods
//! Fraction(355, 113).limit_denominator()  # 22/7
//! ```

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::{AttrCallResult, Fraction, PyTrait, Type},
    value::Value,
};

/// Creates the `fractions` module and allocates it on the heap.
///
/// Sets up the Fraction class constructor and any module-level functions.
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
    let mut module = crate::types::Module::new(StaticStrings::Fractions);

    // Fraction class/type
    module.set_attr(
        StaticStrings::FractionClass,
        Value::Builtin(Builtins::Type(Type::Fraction)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to the Fraction type constructor.
///
/// Handles Fraction() constructor calls with various argument patterns:
/// - Fraction() -> 0/1
/// - Fraction(numerator) -> numerator/1
/// - Fraction(numerator, denominator)
/// - Fraction(string) -> parse from string
/// - Fraction(float) -> convert from float
fn fraction_constructor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (num_arg, den_arg) = args.get_zero_one_two_args("Fraction", heap)?;

    match (num_arg, den_arg) {
        // Fraction() -> 0/1
        (None, None) => {
            let frac = Fraction::from_i64(0, 1)?;
            let id = heap.allocate(HeapData::Fraction(frac))?;
            Ok(Value::Ref(id))
        }
        // Fraction(numerator) or Fraction(string) or Fraction(float)
        (Some(value), None) => fraction_from_single_arg(&value, heap, interns),
        // Fraction(numerator, denominator)
        (Some(num), Some(den)) => fraction_from_two_args(&num, &den, heap, interns),
        // Fraction(None, denominator) is invalid
        (None, Some(den)) => {
            den.drop_with_heap(heap);
            Err(ExcType::type_error(
                "Fraction() missing 1 required positional argument: 'numerator'",
            ))
        }
    }
}

/// Creates a Fraction from a single argument.
fn fraction_from_single_arg(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match value {
        // Integer -> Fraction(n, 1)
        Value::Int(i) => {
            let frac = Fraction::from_i64_single(*i);
            let id = heap.allocate(HeapData::Fraction(frac))?;
            Ok(Value::Ref(id))
        }
        Value::Bool(b) => {
            let frac = Fraction::from_i64_single(i64::from(*b));
            let id = heap.allocate(HeapData::Fraction(frac))?;
            Ok(Value::Ref(id))
        }
        // String -> parse
        Value::InternString(sid) => {
            let s = interns.get_str(*sid);
            let frac = Fraction::from_str(s)?;
            let id = heap.allocate(HeapData::Fraction(frac))?;
            Ok(Value::Ref(id))
        }
        // Float -> convert using as_integer_ratio
        Value::Float(f) => {
            let frac = Fraction::from_float(*f)?;
            let id = heap.allocate(HeapData::Fraction(frac))?;
            Ok(Value::Ref(id))
        }
        Value::Ref(heap_id) => {
            let data = heap.get(*heap_id);
            match data {
                // LongInt -> convert
                HeapData::LongInt(li) => {
                    let n = li.inner().clone();
                    let frac = Fraction::new(n, num_bigint::BigInt::from(1))?;
                    let id = heap.allocate(HeapData::Fraction(frac))?;
                    Ok(Value::Ref(id))
                }
                // String -> parse
                HeapData::Str(s) => {
                    let frac = Fraction::from_str(s.as_str())?;
                    let id = heap.allocate(HeapData::Fraction(frac))?;
                    Ok(Value::Ref(id))
                }
                // Another Fraction -> copy
                HeapData::Fraction(f) => {
                    let frac = f.clone();
                    let id = heap.allocate(HeapData::Fraction(frac))?;
                    Ok(Value::Ref(id))
                }
                // Decimal -> convert using to_fraction
                HeapData::Decimal(d) => {
                    let frac = d.to_fraction().ok_or_else(|| {
                        SimpleException::new_msg(ExcType::ValueError, "Cannot convert Decimal to Fraction")
                    })?;
                    let id = heap.allocate(HeapData::Fraction(frac))?;
                    Ok(Value::Ref(id))
                }
                _ => {
                    let type_name = value.py_type(heap);
                    Err(
                        SimpleException::new_msg(ExcType::TypeError, format!("Cannot convert {type_name} to Fraction"))
                            .into(),
                    )
                }
            }
        }
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(ExcType::TypeError, format!("Cannot convert {type_name} to Fraction")).into())
        }
    }
}

/// Creates a Fraction from two arguments (numerator, denominator).
fn fraction_from_two_args(
    num: &Value,
    den: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
) -> RunResult<Value> {
    // Helper to extract BigInt from Value
    fn extract_bigint(value: &Value, heap: &Heap<impl ResourceTracker>, name: &str) -> RunResult<num_bigint::BigInt> {
        match value {
            Value::Int(i) => Ok(num_bigint::BigInt::from(*i)),
            Value::Bool(b) => Ok(num_bigint::BigInt::from(i64::from(*b))),
            Value::Ref(heap_id) => {
                if let HeapData::LongInt(li) = heap.get(*heap_id) {
                    Ok(li.inner().clone())
                } else {
                    let type_name = value.py_type(heap);
                    Err(SimpleException::new_msg(
                        ExcType::TypeError,
                        format!("'Fraction' {name} must be an integer, not {type_name}"),
                    )
                    .into())
                }
            }
            _ => {
                let type_name = value.py_type(heap);
                Err(SimpleException::new_msg(
                    ExcType::TypeError,
                    format!("'Fraction' {name} must be an integer, not {type_name}"),
                )
                .into())
            }
        }
    }

    // Extract numerator and denominator
    let numerator = extract_bigint(num, heap, "numerator")?;
    let denominator = extract_bigint(den, heap, "denominator")?;

    let frac = Fraction::new(numerator, denominator)?;

    let id = heap.allocate(HeapData::Fraction(frac))?;
    Ok(Value::Ref(id))
}

/// Dispatches a call to a fractions module function.
///
/// Currently the fractions module only exports the Fraction type,
/// which is handled as a type constructor.
pub fn call_type(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = fraction_constructor(heap, interns, args)?;
    Ok(AttrCallResult::Value(result))
}
