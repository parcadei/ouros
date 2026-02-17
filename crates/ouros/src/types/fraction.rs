//! Fraction type for rational number arithmetic.
//!
//! Implements Python's `fractions.Fraction` class with exact rational arithmetic
//! using arbitrary precision integers. Fractions are automatically normalized
//! to have a positive denominator and GCD(numerator, denominator) = 1.

use std::{borrow::Cow, fmt::Write};

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StringId},
    resource::ResourceTracker,
    types::{AttrCallResult, PyTrait, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// A rational number represented as a fraction of two integers.
///
/// Fractions are always stored in normalized form:
/// - The denominator is always positive
/// - The numerator and denominator have no common factors (GCD = 1)
/// - Zero is represented as 0/1
///
/// Both numerator and denominator are stored as `BigInt` to support
/// arbitrary precision arithmetic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Fraction {
    numerator: BigInt,
    denominator: BigInt,
}

impl Fraction {
    /// Creates a new Fraction from numerator and denominator.
    ///
    /// The fraction is automatically normalized. If the denominator is zero,
    /// a ZeroDivisionError is returned.
    pub fn new(numerator: BigInt, denominator: BigInt) -> Result<Self, crate::exception_private::RunError> {
        if denominator.is_zero() {
            return Err(SimpleException::new_msg(ExcType::ZeroDivisionError, "Fraction(1, 0)").into());
        }

        // Normalize: ensure positive denominator and reduced form
        let (n, d) = Self::normalize(numerator, denominator);
        Ok(Self {
            numerator: n,
            denominator: d,
        })
    }

    /// Creates a Fraction from an i64 numerator and denominator.
    pub fn from_i64(numerator: i64, denominator: i64) -> RunResult<Self> {
        Self::new(BigInt::from(numerator), BigInt::from(denominator))
    }

    /// Creates a Fraction from a single i64 (denominator = 1).
    pub fn from_i64_single(value: i64) -> Self {
        Self {
            numerator: BigInt::from(value),
            denominator: BigInt::from(1),
        }
    }

    /// Converts a float to a fraction using continued fraction approximation.
    ///
    /// This matches CPython's behavior for float-to-Fraction conversion.
    pub fn from_float(value: f64) -> RunResult<Self> {
        if value.is_nan() {
            return Err(SimpleException::new_msg(ExcType::ValueError, "Cannot convert NaN to integer ratio").into());
        }
        if value.is_infinite() {
            return Err(
                SimpleException::new_msg(ExcType::OverflowError, "cannot convert Infinity to integer ratio").into(),
            );
        }

        // Convert the float to a fraction using the raw bits representation
        // This matches CPython's float.as_integer_ratio() behavior
        let bits = value.to_bits();
        let sign = if bits >> 63 == 0 { 1.0 } else { -1.0 };
        let mut exponent: i16 = ((bits >> 52) & 0x7ff) as i16;
        let mantissa = if exponent == 0 {
            (bits & 0xfffffffffffff) << 1
        } else {
            (bits & 0xfffffffffffff) | 0x10000000000000
        };

        if exponent == 0x7ff {
            // Should have been caught by is_infinite/is_nan checks above
            return Err(
                SimpleException::new_msg(ExcType::ValueError, "Cannot convert special float to fraction").into(),
            );
        }

        exponent -= 1023 + 52;
        let mut n = BigInt::from(mantissa as i64);
        let mut d = BigInt::from(1);

        if exponent > 0 {
            n <<= exponent as u32;
        } else if exponent < 0 {
            d <<= (-exponent) as u32;
        }

        if sign < 0.0 {
            n = -n;
        }

        Self::new(n, d)
    }

    /// Parses a Fraction from a string.
    ///
    /// Supports:
    /// - "numerator/denominator" format (e.g., "3/7", "-2/5")
    /// - Decimal format (e.g., "1.5", "-2.3", "3e-2")
    pub fn from_str(s: &str) -> RunResult<Self> {
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid literal for Fraction: ''").into());
        }

        // Try "numerator/denominator" format first
        if let Some(idx) = trimmed.find('/') {
            let num_str = &trimmed[..idx].trim();
            let den_str = &trimmed[idx + 1..].trim();

            let numerator = parse_bigint(num_str)?;
            let denominator = parse_bigint(den_str)?;

            return Self::new(numerator, denominator);
        }

        // Try decimal/scientific notation
        if trimmed.contains('.') || trimmed.contains('e') || trimmed.contains('E') {
            let float_val: f64 = trimmed.parse().map_err(|_| {
                SimpleException::new_msg(
                    ExcType::ValueError,
                    format!("Invalid literal for Fraction: {}", StringRepr(trimmed)),
                )
            })?;
            return Self::from_float(float_val);
        }

        // Plain integer
        let numerator = parse_bigint(trimmed)?;
        Ok(Self::from_i64_single(numerator.try_into().unwrap_or(0)))
    }

    /// Returns the numerator.
    pub fn numerator(&self) -> &BigInt {
        &self.numerator
    }

    /// Returns the denominator.
    pub fn denominator(&self) -> &BigInt {
        &self.denominator
    }

    /// Converts the Fraction to an f64.
    pub fn to_f64(&self) -> f64 {
        let n: f64 = self.numerator.to_f64().unwrap_or(0.0);
        let d: f64 = self.denominator.to_f64().unwrap_or(1.0);
        n / d
    }

    /// Converts the Fraction to an i64 (truncates toward zero).
    pub fn to_i64(&self) -> i64 {
        (&self.numerator / &self.denominator).to_i64().unwrap_or(0)
    }

    /// Returns true if the fraction is zero.
    pub fn is_zero(&self) -> bool {
        self.numerator.is_zero()
    }

    /// Normalizes numerator and denominator.
    ///
    /// - Moves sign to numerator (denominator always positive)
    /// - Divides by GCD
    fn normalize(n: BigInt, d: BigInt) -> (BigInt, BigInt) {
        if d.is_zero() {
            return (n, d);
        }

        let mut n = n;
        let mut d = d;

        // Ensure denominator is positive
        if d.is_negative() {
            n = -n;
            d = -d;
        }

        // Divide by GCD
        let g = n.gcd(&d);
        if !g.is_one() {
            n /= &g;
            d /= &g;
        }

        (n, d)
    }

    /// Returns the absolute value of this fraction.
    pub fn abs(&self) -> Self {
        Self {
            numerator: self.numerator.abs(),
            denominator: self.denominator.clone(),
        }
    }

    /// Returns the reciprocal of this fraction.
    ///
    /// Returns None if the fraction is zero.
    pub fn recip(&self) -> Option<Self> {
        if self.numerator.is_zero() {
            return None;
        }
        Some(Self {
            numerator: self.denominator.clone(),
            denominator: self.numerator.clone(),
        })
    }

    /// Returns a fraction with a limited denominator.
    ///
    /// Finds the closest fraction to self with denominator at most `max_denominator`.
    /// This is useful for finding good rational approximations.
    pub fn limit_denominator(&self, max_denominator: u64) -> Self {
        if max_denominator == 0 {
            return self.clone();
        }

        let max_d = BigInt::from(max_denominator);

        // If already within limit, return self
        if self.denominator <= max_d {
            return self.clone();
        }

        // Use continued fraction expansion to find best approximation
        let zero = BigInt::from(0);
        let one = BigInt::from(1);

        let mut p0 = zero.clone();
        let mut p1 = one.clone();
        let mut q0 = one.clone();
        let mut q1 = zero.clone();

        let mut n = self.numerator.abs();
        let mut d = self.denominator.clone();

        loop {
            if d.is_zero() || q1 > max_d {
                break;
            }

            let a = &n / &d;
            let (p2, q2) = (a.clone() * &p1 + &p0, a.clone() * &q1 + &q0);

            if q2 > max_d {
                // Find best approximation between p0/q0 and p1/q1
                let k = (&max_d - &q0) / &q1;
                let bound1 = Self {
                    numerator: k.clone() * &p1 + &p0,
                    denominator: k.clone() * &q1 + &q0,
                };
                let bound2 = Self {
                    numerator: p1.clone(),
                    denominator: q1.clone(),
                };

                let self_abs = self.abs();
                let diff1 = if bound1 >= self_abs {
                    &bound1 - &self_abs
                } else {
                    &self_abs - &bound1
                };
                let diff2 = if bound2 >= self_abs {
                    &bound2 - &self_abs
                } else {
                    &self_abs - &bound2
                };

                let result = if diff1 <= diff2 { bound1 } else { bound2 };
                return if self.numerator.is_negative() { -result } else { result };
            }

            p0 = p1;
            p1 = p2;
            q0 = q1;
            q1 = q2;

            let (new_n, new_d) = (d.clone(), n - a * &d);
            n = new_n;
            d = new_d;
        }

        // Return closest of p0/q0 and p1/q1
        let self_abs = self.abs();
        let f0 = Self {
            numerator: p0,
            denominator: q0.clone(),
        };
        let f1 = Self {
            numerator: p1,
            denominator: q1.clone(),
        };

        let diff0 = if f0 >= self_abs {
            &f0 - &self_abs
        } else {
            &self_abs - &f0
        };
        let diff1 = if f1 >= self_abs {
            &f1 - &self_abs
        } else {
            &self_abs - &f1
        };

        let result = if diff0 <= diff1 { f0 } else { f1 };
        if self.numerator.is_negative() { -result } else { result }
    }

    /// Returns a Value representing this fraction.
    ///
    /// Allocates on the heap if needed.
    pub fn to_value(&self, heap: &mut Heap<impl ResourceTracker>) -> Result<Value, crate::resource::ResourceError> {
        let id = heap.allocate(HeapData::Fraction(self.clone()))?;
        Ok(Value::Ref(id))
    }
}

impl std::ops::Add for Fraction {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let n = self.numerator * &rhs.denominator + rhs.numerator * &self.denominator;
        let d = self.denominator * rhs.denominator;
        let (n, d) = Self::normalize(n, d);
        Self {
            numerator: n,
            denominator: d,
        }
    }
}

impl std::ops::Sub for Fraction {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let n = self.numerator * &rhs.denominator - rhs.numerator * &self.denominator;
        let d = self.denominator * rhs.denominator;
        let (n, d) = Self::normalize(n, d);
        Self {
            numerator: n,
            denominator: d,
        }
    }
}

impl std::ops::Mul for Fraction {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let n = self.numerator * rhs.numerator;
        let d = self.denominator * rhs.denominator;
        let (n, d) = Self::normalize(n, d);
        Self {
            numerator: n,
            denominator: d,
        }
    }
}

impl std::ops::Div for Fraction {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        let n = self.numerator * rhs.denominator;
        let d = self.denominator * rhs.numerator;
        let (n, d) = Self::normalize(n, d);
        Self {
            numerator: n,
            denominator: d,
        }
    }
}

impl std::ops::Neg for Fraction {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            numerator: -self.numerator,
            denominator: self.denominator,
        }
    }
}

impl std::ops::Rem for Fraction {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self::Output {
        let div = &self / &rhs;
        let truncated = Self {
            numerator: div.numerator / div.denominator,
            denominator: BigInt::from(1),
        };
        self - (truncated * rhs)
    }
}

impl std::ops::Add<&Fraction> for &Fraction {
    type Output = Fraction;

    fn add(self, rhs: &Fraction) -> Fraction {
        self.clone() + rhs.clone()
    }
}

impl std::ops::Sub<&Fraction> for &Fraction {
    type Output = Fraction;

    fn sub(self, rhs: &Fraction) -> Fraction {
        self.clone() - rhs.clone()
    }
}

impl std::ops::Mul<&Fraction> for &Fraction {
    type Output = Fraction;

    fn mul(self, rhs: &Fraction) -> Fraction {
        self.clone() * rhs.clone()
    }
}

impl std::ops::Div<&Fraction> for &Fraction {
    type Output = Fraction;

    fn div(self, rhs: &Fraction) -> Fraction {
        self.clone() / rhs.clone()
    }
}

impl PartialOrd for Fraction {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fraction {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by cross-multiplication: a/b < c/d iff a*d < c*b (when b,d > 0)
        let left = &self.numerator * &other.denominator;
        let right = &other.numerator * &self.denominator;
        left.cmp(&right)
    }
}

impl PyTrait for Fraction {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Fraction
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + (self.numerator.bits() as usize).div_ceil(8)
            + (self.denominator.bits() as usize).div_ceil(8)
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self == other
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Fraction has no heap references
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.numerator.is_zero()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut ahash::AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        // repr: Fraction(numerator, denominator)
        write!(f, "Fraction({}, {})", self.numerator, self.denominator)
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Cow<'static, str> {
        // str: "numerator/denominator" unless denominator is 1
        if self.denominator.is_one() {
            Cow::Owned(self.numerator.to_string())
        } else {
            Cow::Owned(format!("{}/{}", self.numerator, self.denominator))
        }
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let attr_name = interns.get_str(attr_id);
        match attr_name {
            "numerator" => {
                let val = LongInt::new(self.numerator.clone()).into_value(heap)?;
                Ok(Some(AttrCallResult::Value(val)))
            }
            "denominator" => {
                let val = LongInt::new(self.denominator.clone()).into_value(heap)?;
                Ok(Some(AttrCallResult::Value(val)))
            }
            "real" => Ok(Some(AttrCallResult::Value(self.clone().to_value(heap)?))),
            "imag" => Ok(Some(AttrCallResult::Value(Value::Int(0)))),
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let name = attr.as_str(interns);
        if name == "limit_denominator" {
            self.call_limit_denominator(heap, args)
        } else if name == "as_integer_ratio" {
            self.call_as_integer_ratio(heap, args)
        } else if name == "is_integer" {
            self.call_is_integer(heap, args)
        } else if name == "conjugate" {
            self.call_conjugate(heap, args)
        } else {
            args.drop_with_heap(heap);
            Err(ExcType::attribute_error("Fraction", name))
        }
    }
}

use crate::types::LongInt;

impl Fraction {
    /// Handles `Fraction.limit_denominator(max_denominator=1000000)`.
    fn call_limit_denominator(&self, heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
        let max_d = args.get_zero_one_arg("Fraction.limit_denominator", heap)?;
        defer_drop!(max_d, heap);

        let max_denominator = match max_d {
            Some(v) => match v {
                Value::Int(i) => *i as u64,
                Value::Bool(b) => u64::from(*b),
                Value::Ref(heap_id) => {
                    if let HeapData::LongInt(li) = heap.get(*heap_id) {
                        li.to_u64().unwrap_or(1_000_000)
                    } else {
                        let type_name = v.py_type(heap);
                        return Err(SimpleException::new_msg(
                            ExcType::TypeError,
                            format!("'{type_name}' object cannot be interpreted as an integer"),
                        )
                        .into());
                    }
                }
                _ => {
                    let type_name = v.py_type(heap);
                    return Err(SimpleException::new_msg(
                        ExcType::TypeError,
                        format!("'{type_name}' object cannot be interpreted as an integer"),
                    )
                    .into());
                }
            },
            None => 1_000_000, // Default value
        };

        let result = self.limit_denominator(max_denominator);
        result.to_value(heap).map_err(Into::into)
    }

    /// Handles `Fraction.as_integer_ratio()`.
    fn call_as_integer_ratio(&self, heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
        args.check_zero_args("Fraction.as_integer_ratio", heap)?;

        let num = LongInt::new(self.numerator.clone()).into_value(heap)?;
        let den = LongInt::new(self.denominator.clone()).into_value(heap)?;

        allocate_tuple(smallvec::smallvec![num, den], heap).map_err(Into::into)
    }

    /// Handles `Fraction.is_integer()`.
    fn call_is_integer(&self, heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
        args.check_zero_args("Fraction.is_integer", heap)?;
        Ok(Value::Bool(self.denominator.is_one()))
    }

    /// Handles `Fraction.conjugate()`.
    fn call_conjugate(&self, heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
        args.check_zero_args("Fraction.conjugate", heap)?;
        self.clone().to_value(heap).map_err(Into::into)
    }
}

/// Helper struct for string representation in error messages.
struct StringRepr<'a>(&'a str);

impl std::fmt::Display for StringRepr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "'{}'", self.0)
    }
}

/// Parses a BigInt from a string.
fn parse_bigint(s: &str) -> RunResult<BigInt> {
    s.parse().map_err(|_| {
        SimpleException::new_msg(
            ExcType::ValueError,
            format!("Invalid literal for Fraction: {}", StringRepr(s)),
        )
        .into()
    })
}
