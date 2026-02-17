//! Decimal type for arbitrary precision decimal arithmetic.
//!
//! This module provides a `Decimal` type that implements fixed-point and floating-point
//! decimal arithmetic. It follows the General Decimal Arithmetic Specification,
//! similar to Python's `decimal` module.
//!
//! The implementation uses a coefficient (BigInt) and an exponent (i32) representation:
//! value = coefficient * 10^exponent
//!
//! Currently supported:
//! - Construction from strings and integers
//! - Basic arithmetic: add, subtract, multiply, divide, floor_div, modulo, power
//! - Comparisons
//! - repr and str formatting
//! - quantize method
//! - to_eng_string method
//! - is_finite, is_infinite, is_nan, is_zero, is_signed predicates
//! - copy_abs, copy_negate, copy_sign methods

use std::{
    cmp::Ordering,
    fmt::{self, Write},
    str::FromStr,
};

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::Interns,
    modules::decimal_mod,
    resource::ResourceTracker,
    types::{LongInt, PyTrait, StdlibObject, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// A decimal number with arbitrary precision.
///
/// Stored as coefficient * 10^exponent, where coefficient is a BigInt.
/// For finite values, trailing zeros are preserved to keep significance semantics
/// aligned with Python's `decimal.Decimal`.
/// Special values (Infinity, -Infinity, NaN, -NaN, sNaN, -sNaN) are stored as flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Decimal {
    /// The significand/coefficient of the decimal number.
    /// For special values, this stores 0.
    coefficient: BigInt,
    /// The exponent (power of 10).
    /// For special values, this is 0.
    exponent: i32,
    /// Special value flag.
    special: SpecialValue,
    /// Sign bit for finite zero values.
    ///
    /// `BigInt` does not preserve the sign of zero, but Decimal needs `-0`.
    negative_zero: bool,
}

/// Special decimal values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum SpecialValue {
    /// Normal finite number.
    None,
    /// Positive or negative infinity.
    Infinity { negative: bool },
    /// Quiet NaN (not a number), may have a sign.
    Nan { negative: bool, signaling: bool },
}

/// Supported rounding modes for decimal quantization operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecimalRoundingMode {
    Up,
    Down,
    Ceiling,
    Floor,
    HalfUp,
    HalfDown,
    HalfEven,
    O5Up,
}

impl DecimalRoundingMode {
    /// Parses the CPython decimal rounding mode string constant.
    #[must_use]
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "ROUND_UP" => Some(Self::Up),
            "ROUND_DOWN" => Some(Self::Down),
            "ROUND_CEILING" => Some(Self::Ceiling),
            "ROUND_FLOOR" => Some(Self::Floor),
            "ROUND_HALF_UP" => Some(Self::HalfUp),
            "ROUND_HALF_DOWN" => Some(Self::HalfDown),
            "ROUND_HALF_EVEN" => Some(Self::HalfEven),
            "ROUND_05UP" => Some(Self::O5Up),
            _ => None,
        }
    }
}

impl Decimal {
    /// Creates a new Decimal from coefficient and exponent.
    pub(crate) fn new(coefficient: BigInt, exponent: i32) -> Self {
        Self {
            coefficient,
            exponent,
            special: SpecialValue::None,
            negative_zero: false,
        }
    }

    /// Creates a Decimal from a string.
    ///
    /// Supports formats like:
    /// - "123", "-123", "+123"
    /// - "123.456", "-123.456"
    /// - "1.23E+10", "1.23e-10"
    /// - "Infinity", "-Infinity", "NaN", "-NaN", "sNaN", "-sNaN"
    ///
    /// # Errors
    /// Returns an error if the string is not a valid decimal representation.
    pub fn from_string(s: &str) -> Result<Self, String> {
        let s = s.trim();

        // Handle special values
        let lower = s.to_lowercase();
        if lower == "infinity" || lower == "inf" {
            return Ok(Self::infinity(false));
        }
        if lower == "-infinity" || lower == "-inf" {
            return Ok(Self::infinity(true));
        }
        if lower == "nan" || lower == "+nan" {
            return Ok(Self::nan(false, false));
        }
        if lower == "-nan" {
            return Ok(Self::nan(true, false));
        }
        if lower == "snan" || lower == "+snan" {
            return Ok(Self::nan(false, true));
        }
        if lower == "-snan" {
            return Ok(Self::nan(true, true));
        }

        // Parse regular number
        let (negative, rest) = if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = s.strip_prefix('+') {
            (false, rest)
        } else {
            (false, s)
        };

        // Find exponent indicator
        let (mantissa, exp_str) = if let Some(pos) = rest.to_lowercase().find('e') {
            (&rest[..pos], &rest[pos + 1..])
        } else {
            (rest, "")
        };

        // Parse exponent
        let mut exponent: i32 = 0;
        if !exp_str.is_empty() {
            exponent = exp_str
                .parse::<i32>()
                .map_err(|_| format!("Invalid exponent in decimal string: {s}"))?;
        }

        // Parse mantissa (handle decimal point)
        let (int_part, frac_part) = if let Some(pos) = mantissa.find('.') {
            (&mantissa[..pos], &mantissa[pos + 1..])
        } else {
            (mantissa, "")
        };

        // Combine digits
        let digits = format!("{}{}", int_part.trim_start_matches('0'), frac_part);
        let frac_len = frac_part.len() as i32;
        exponent -= frac_len;

        // Parse coefficient
        let all_zero = digits.is_empty() || digits.chars().all(|c| c == '0');
        let coefficient = if all_zero {
            BigInt::ZERO
        } else {
            BigInt::from_str(&digits).map_err(|_| format!("Invalid digits in decimal string: {s}"))?
        };

        let coefficient = if negative { -coefficient } else { coefficient };
        let mut value = Self::new(coefficient, exponent);
        if all_zero {
            value.negative_zero = negative;
        }
        Ok(value)
    }

    /// Creates positive or negative infinity.
    fn infinity(negative: bool) -> Self {
        Self {
            coefficient: BigInt::ZERO,
            exponent: 0,
            special: SpecialValue::Infinity { negative },
            negative_zero: false,
        }
    }

    /// Creates a NaN value.
    fn nan(negative: bool, signaling: bool) -> Self {
        Self {
            coefficient: BigInt::ZERO,
            exponent: 0,
            special: SpecialValue::Nan { negative, signaling },
            negative_zero: false,
        }
    }

    /// Creates a Decimal from an i64.
    pub(crate) fn from_i64(n: i64) -> Self {
        Self::new(BigInt::from(n), 0)
    }

    /// Creates a Decimal that exactly represents an IEEE-754 float.
    ///
    /// This matches `Decimal.from_float()` behavior for finite values by converting
    /// `n / 2^k` into `n * 5^k * 10^-k`.
    pub(crate) fn from_f64_exact(value: f64) -> Self {
        if value.is_nan() {
            return Self::nan(value.is_sign_negative(), false);
        }
        if value.is_infinite() {
            return Self::infinity(value.is_sign_negative());
        }
        if value == 0.0 {
            return Self::new(BigInt::ZERO, 0).with_sign(value.is_sign_negative());
        }

        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction_bits = bits & ((1u64 << 52) - 1);

        let (mut numerator, exponent_two) = if exponent_bits == 0 {
            (BigInt::from(fraction_bits), -1022 - 52)
        } else {
            (BigInt::from(fraction_bits | (1u64 << 52)), exponent_bits - 1023 - 52)
        };

        let mut exponent_ten = 0i32;
        if exponent_two >= 0 {
            numerator <<= usize::try_from(exponent_two).unwrap_or(0);
        } else {
            let power = u32::try_from(-exponent_two).unwrap_or(0);
            numerator *= BigInt::from(5u8).pow(power);
            exponent_ten = -i32::try_from(power).unwrap_or(i32::MAX);
        }
        if negative {
            numerator = -numerator;
        }
        let mut result = Self::new(numerator, exponent_ten);
        result.normalize();
        result
    }

    /// Returns a normalized copy (trailing zeros removed).
    pub(crate) fn normalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.normalize();
        normalized
    }

    /// Returns the exponent field.
    pub(crate) fn exponent_value(&self) -> i32 {
        self.exponent
    }

    /// Returns the sign bit used by `as_tuple()`: `0` positive, `1` negative.
    pub(crate) fn sign_bit(&self) -> i64 {
        i64::from(self.is_signed())
    }

    /// Returns coefficient digits used by `as_tuple()`.
    pub(crate) fn coefficient_digits(&self) -> Vec<i64> {
        if !self.is_finite() {
            return Vec::new();
        }
        let digits = self.coefficient.abs().to_string();
        if digits.is_empty() {
            return vec![0];
        }
        digits
            .chars()
            .map(|ch| i64::from(ch.to_digit(10).unwrap_or(0)))
            .collect()
    }

    /// Returns true if this is a quiet NaN.
    pub(crate) fn is_qnan(&self) -> bool {
        matches!(self.special, SpecialValue::Nan { signaling: false, .. })
    }

    /// Returns true if this is a canonical decimal.
    ///
    /// Ouros's Decimal representation is always canonical.
    pub(crate) fn is_canonical(&self) -> bool {
        true
    }

    /// Returns true if this is a normal finite, non-zero decimal.
    pub(crate) fn is_normal(&self) -> bool {
        self.is_finite() && !self.is_zero()
    }

    /// Returns true if this is subnormal under the current context.
    ///
    /// Ouros's simplified implementation does not currently model subnormal
    /// ranges, so this always returns false.
    pub(crate) fn is_subnormal(&self) -> bool {
        false
    }

    /// Converts this decimal to f64 when finite and representable.
    fn to_f64(&self) -> Option<f64> {
        if !self.is_finite() {
            return None;
        }
        let coefficient = self.coefficient.to_f64()?;
        Some(coefficient * 10f64.powi(self.exponent))
    }

    /// Returns the adjusted exponent (`len(digits) + exp - 1`) for finite values.
    pub(crate) fn adjusted(&self) -> i32 {
        if !self.is_finite() || self.is_zero() {
            return 0;
        }
        let digits_len = i32::try_from(self.coefficient.abs().to_string().len()).unwrap_or(1);
        digits_len + self.exponent - 1
    }

    /// Returns this finite decimal as `(numerator, denominator)` in lowest terms.
    pub(crate) fn as_integer_ratio(&self) -> Option<(BigInt, BigInt)> {
        let fraction = self.to_fraction()?;
        Some((fraction.numerator().clone(), fraction.denominator().clone()))
    }

    /// Returns `-1`, `0`, `1`, or `NaN` according to decimal comparison rules.
    pub(crate) fn compare_result(&self, other: &Self) -> Self {
        match self.partial_cmp(other) {
            Some(Ordering::Less) => Self::from_i64(-1),
            Some(Ordering::Equal) => Self::from_i64(0),
            Some(Ordering::Greater) => Self::from_i64(1),
            None => Self::nan(false, false),
        }
    }

    /// Returns a rounded decimal approximation of a floating-point result.
    fn from_f64_with_context(value: f64, precision: i32) -> Self {
        if value.is_nan() {
            return Self::nan(false, false);
        }
        if value.is_infinite() {
            return Self::infinity(value.is_sign_negative());
        }
        if value == 0.0 {
            return Self::from_i64(0);
        }

        let digits_before_decimal = if value.abs() >= 1.0 {
            (value.abs().log10().floor() as i32) + 1
        } else {
            0
        };
        let decimals = if digits_before_decimal > 0 {
            (precision - digits_before_decimal).max(0)
        } else {
            precision.max(0)
        };
        let decimals = usize::try_from(decimals).unwrap_or(0);
        let rendered = format!("{value:.decimals$}");
        Self::from_string(&rendered).unwrap_or_else(|_| Self::from_i64(0))
    }

    /// Returns `self.exp()` under a given precision.
    pub(crate) fn exp_with_prec(&self, precision: i32) -> Self {
        let Some(value) = self.to_f64() else {
            return Self::nan(false, false);
        };
        Self::from_f64_with_context(value.exp(), precision)
    }

    /// Returns `self.ln()` under a given precision.
    pub(crate) fn ln_with_prec(&self, precision: i32) -> Self {
        let Some(value) = self.to_f64() else {
            return Self::nan(false, false);
        };
        Self::from_f64_with_context(value.ln(), precision)
    }

    /// Returns `self.log10()` under a given precision.
    pub(crate) fn log10_with_prec(&self, precision: i32) -> Self {
        let Some(value) = self.to_f64() else {
            return Self::nan(false, false);
        };
        Self::from_f64_with_context(value.log10(), precision)
    }

    /// Returns `self.sqrt()` under a given precision.
    pub(crate) fn sqrt_with_prec(&self, precision: i32) -> Self {
        let Some(value) = self.to_f64() else {
            return Self::nan(false, false);
        };
        Self::from_f64_with_context(value.sqrt(), precision)
    }

    /// Scales this decimal by `10^n`.
    pub(crate) fn scaleb(&self, n: i32) -> Self {
        if !self.is_finite() {
            return self.clone();
        }
        Self::new(self.coefficient.clone(), self.exponent + n)
    }

    /// Applies decimal shift semantics used by parity tests.
    ///
    /// Shift moves coefficient digits without changing exponent.
    pub(crate) fn shift(&self, n: i32) -> Self {
        if !self.is_finite() {
            return self.clone();
        }
        if n >= 0 {
            let factor = BigInt::from(10u32).pow(u32::try_from(n).unwrap_or(0));
            Self::new(&self.coefficient * factor, self.exponent)
        } else {
            let divisor = BigInt::from(10u32).pow(u32::try_from(-n).unwrap_or(0));
            Self::new(&self.coefficient / divisor, self.exponent)
        }
    }

    /// Returns the next representable value toward positive infinity.
    pub(crate) fn next_plus(&self) -> Self {
        if !self.is_finite() {
            return self.clone();
        }
        let step_exp = -(decimal_mod::current_precision() - 1);
        let step = Self::new(BigInt::from(1), step_exp);
        self.add(&step)
    }

    /// Returns the next representable value toward negative infinity.
    pub(crate) fn next_minus(&self) -> Self {
        if !self.is_finite() {
            return self.clone();
        }
        let step_exp = if self.coefficient.abs() == BigInt::from(1) && self.exponent == 0 {
            -decimal_mod::current_precision()
        } else {
            -(decimal_mod::current_precision() - 1)
        };
        let step = Self::new(BigInt::from(1), step_exp);
        self.sub(&step)
    }

    /// Returns the next representable value in the direction of `other`.
    pub(crate) fn next_toward(&self, other: &Self) -> Self {
        match self.partial_cmp(other) {
            Some(Ordering::Less) => self.next_plus(),
            Some(Ordering::Greater) => self.next_minus(),
            _ => self.clone(),
        }
    }

    /// Returns decimal class name strings matching CPython's number classes.
    pub(crate) fn number_class(&self) -> &'static str {
        match self.special {
            SpecialValue::Infinity { negative: false } => "+Infinity",
            SpecialValue::Infinity { negative: true } => "-Infinity",
            SpecialValue::Nan {
                signaling: false,
                negative: false,
            } => "NaN",
            SpecialValue::Nan {
                signaling: false,
                negative: true,
            } => "-NaN",
            SpecialValue::Nan {
                signaling: true,
                negative: false,
            } => "sNaN",
            SpecialValue::Nan {
                signaling: true,
                negative: true,
            } => "-sNaN",
            SpecialValue::None if self.is_zero() && self.is_signed() => "-Zero",
            SpecialValue::None if self.is_zero() => "+Zero",
            SpecialValue::None if self.is_signed() => "-Normal",
            SpecialValue::None => "+Normal",
        }
    }

    /// Performs digit-wise logical AND.
    pub(crate) fn logical_and(&self, other: &Self) -> Self {
        Self::logical_binary(self, other, |a, b| a & b)
    }

    /// Performs digit-wise logical OR.
    pub(crate) fn logical_or(&self, other: &Self) -> Self {
        Self::logical_binary(self, other, |a, b| a | b)
    }

    /// Performs digit-wise logical XOR.
    pub(crate) fn logical_xor(&self, other: &Self) -> Self {
        Self::logical_binary(self, other, |a, b| a ^ b)
    }

    /// Performs digit-wise logical inversion at current precision.
    pub(crate) fn logical_invert(&self) -> Self {
        if !self.is_finite() {
            return Self::nan(false, false);
        }
        let precision = usize::try_from(decimal_mod::current_precision()).unwrap_or(28);
        let mut digits = self.coefficient.abs().to_string();
        if digits.len() < precision {
            digits = format!("{}{}", "0".repeat(precision - digits.len()), digits);
        }
        let inverted: String = digits.chars().map(|ch| if ch == '0' { '1' } else { '0' }).collect();
        Self::from_string(&inverted).unwrap_or_else(|_| Self::from_i64(0))
    }

    /// Returns max(self, other) using normal decimal comparison.
    pub(crate) fn max_decimal(&self, other: &Self) -> Self {
        match self.partial_cmp(other) {
            Some(Ordering::Less) => other.clone(),
            _ => self.clone(),
        }
    }

    /// Returns min(self, other) using normal decimal comparison.
    pub(crate) fn min_decimal(&self, other: &Self) -> Self {
        match self.partial_cmp(other) {
            Some(Ordering::Greater) => other.clone(),
            _ => self.clone(),
        }
    }

    /// Returns max magnitude comparison.
    pub(crate) fn max_mag(&self, other: &Self) -> Self {
        let left_abs = self.abs();
        let right_abs = other.abs();
        match left_abs.partial_cmp(&right_abs) {
            Some(Ordering::Less) => other.clone(),
            _ => self.clone(),
        }
    }

    /// Returns min magnitude comparison.
    pub(crate) fn min_mag(&self, other: &Self) -> Self {
        let left_abs = self.abs();
        let right_abs = other.abs();
        match left_abs.partial_cmp(&right_abs) {
            Some(Ordering::Greater) => other.clone(),
            _ => self.clone(),
        }
    }

    /// Returns IEEE-style remainder-near using rounded quotient.
    pub(crate) fn remainder_near(&self, other: &Self) -> Self {
        let Some(left) = self.to_f64() else {
            return Self::nan(false, false);
        };
        let Some(right) = other.to_f64() else {
            return Self::nan(false, false);
        };
        if right == 0.0 {
            return Self::nan(false, false);
        }
        let quotient = (left / right).round();
        let remainder = left - quotient * right;
        Self::from_f64_with_context(remainder, decimal_mod::current_precision())
    }

    /// Performs digit-wise logical operation helper.
    fn logical_binary(&self, other: &Self, op: impl Fn(i64, i64) -> i64) -> Self {
        if !self.is_finite() || !other.is_finite() {
            return Self::nan(false, false);
        }
        let lhs = self.coefficient.abs().to_string();
        let rhs = other.coefficient.abs().to_string();
        let len = lhs.len().max(rhs.len());
        let lhs = format!("{}{}", "0".repeat(len.saturating_sub(lhs.len())), lhs);
        let rhs = format!("{}{}", "0".repeat(len.saturating_sub(rhs.len())), rhs);
        let mut result = String::with_capacity(len);
        for (a, b) in lhs.chars().zip(rhs.chars()) {
            let a = i64::from(a != '0');
            let b = i64::from(b != '0');
            result.push(if op(a, b) == 0 { '0' } else { '1' });
        }
        Self::from_string(&result).unwrap_or_else(|_| Self::from_i64(0))
    }

    /// Converts this Decimal to a Fraction.
    ///
    /// Returns None if the Decimal is NaN or Infinity.
    pub(crate) fn to_fraction(&self) -> Option<crate::types::Fraction> {
        use num_bigint::BigInt;
        use num_traits::Pow;

        if !self.is_finite() {
            return None;
        }

        let coefficient = &self.coefficient;

        if self.exponent >= 0 {
            // coefficient * 10^exponent / 1
            let ten = BigInt::from(10i32);
            let denominator = BigInt::from(1);
            let numerator = coefficient * ten.pow(self.exponent as u32);
            Some(crate::types::Fraction::new(numerator, denominator).ok()?)
        } else {
            // coefficient / 10^(-exponent)
            let ten = BigInt::from(10i32);
            let denominator = ten.pow((-self.exponent) as u32);
            let numerator = coefficient.clone();
            Some(crate::types::Fraction::new(numerator, denominator).ok()?)
        }
    }

    /// Returns true if this is a NaN (quiet or signaling).
    pub(crate) fn is_nan(&self) -> bool {
        matches!(self.special, SpecialValue::Nan { .. })
    }

    /// Returns true if this is a signaling NaN.
    pub(crate) fn is_snan(&self) -> bool {
        matches!(self.special, SpecialValue::Nan { signaling: true, .. })
    }

    /// Returns true if this is infinity.
    pub(crate) fn is_infinite(&self) -> bool {
        matches!(self.special, SpecialValue::Infinity { .. })
    }

    /// Returns true if this is a finite number (not infinity or NaN).
    pub(crate) fn is_finite(&self) -> bool {
        matches!(self.special, SpecialValue::None)
    }

    /// Returns the integer obtained by truncating this decimal toward zero.
    ///
    /// Returns `None` for non-finite values (`NaN`, `Infinity`).
    #[must_use]
    pub(crate) fn trunc_to_bigint(&self) -> Option<BigInt> {
        let (numerator, denominator) = self.as_integer_ratio()?;
        Some(numerator / denominator)
    }

    /// Returns true if this is zero.
    fn is_zero(&self) -> bool {
        self.is_finite() && self.coefficient.is_zero()
    }

    /// Returns true if this value is negative (including -0, -NaN, -Infinity).
    pub(crate) fn is_signed(&self) -> bool {
        match self.special {
            SpecialValue::None => self.coefficient.is_negative() || (self.coefficient.is_zero() && self.negative_zero),
            SpecialValue::Infinity { negative } => negative,
            SpecialValue::Nan { negative, .. } => negative,
        }
    }

    /// Returns the absolute value.
    fn abs(&self) -> Self {
        let mut result = self.clone();
        match result.special {
            SpecialValue::None => {
                result.coefficient = result.coefficient.abs();
                if result.coefficient.is_zero() {
                    result.negative_zero = false;
                }
            }
            SpecialValue::Infinity { negative: _ } => {
                result.special = SpecialValue::Infinity { negative: false };
            }
            SpecialValue::Nan { signaling, .. } => {
                result.special = SpecialValue::Nan {
                    negative: false,
                    signaling,
                };
            }
        }
        result
    }

    /// Returns the value with the sign negated.
    fn negate(&self) -> Self {
        let mut result = self.clone();
        match result.special {
            SpecialValue::None => {
                if result.coefficient.is_zero() {
                    result.negative_zero = !result.negative_zero;
                } else {
                    result.coefficient = -result.coefficient;
                }
            }
            SpecialValue::Infinity { negative } => {
                result.special = SpecialValue::Infinity { negative: !negative };
            }
            SpecialValue::Nan { negative, signaling } => {
                result.special = SpecialValue::Nan {
                    negative: !negative,
                    signaling,
                };
            }
        }
        result
    }

    /// Returns a copy with the sign of `other`.
    fn copy_sign(&self, other: &Self) -> Self {
        let mut result = self.clone();
        let other_negative = other.is_signed();
        match result.special {
            SpecialValue::None => {
                if result.coefficient.is_zero() {
                    result.negative_zero = other_negative;
                } else {
                    let current_negative = result.coefficient.is_negative();
                    if current_negative != other_negative {
                        result.coefficient = -result.coefficient;
                    }
                }
            }
            SpecialValue::Infinity { .. } => {
                result.special = SpecialValue::Infinity {
                    negative: other_negative,
                };
            }
            SpecialValue::Nan { signaling, .. } => {
                result.special = SpecialValue::Nan {
                    negative: other_negative,
                    signaling,
                };
            }
        }
        result
    }

    /// Normalizes the representation by removing trailing zeros.
    fn normalize(&mut self) {
        if !self.is_finite() {
            return;
        }

        if self.coefficient.is_zero() {
            self.exponent = 0;
            return;
        }

        // Count trailing zeros in coefficient
        let trailing_zeros = self
            .coefficient
            .to_string()
            .trim_start_matches('-')
            .trim_end_matches('0')
            .len();
        let original_len = self.coefficient.to_string().trim_start_matches('-').len();
        let zeros_to_remove = original_len - trailing_zeros;

        if zeros_to_remove > 0 {
            // Divide by 10^zeros_to_remove
            let divisor = BigInt::from(10u32).pow(u32::try_from(zeros_to_remove).unwrap_or(0));
            self.coefficient /= divisor;
            self.exponent += i32::try_from(zeros_to_remove).unwrap_or(0);
        }
    }

    /// Adjusts the exponent to the target value while preserving numeric value when possible.
    ///
    /// Scaling toward a larger exponent can require division and may truncate when the
    /// coefficient is not evenly divisible by the required power of ten.
    fn adjust_exponent(&self, target_exp: i32) -> Self {
        if !self.is_finite() {
            return self.clone();
        }

        let exp_diff = self.exponent - target_exp;
        if exp_diff == 0 {
            return self.clone();
        }

        let mut result = self.clone();
        if exp_diff > 0 {
            // Lower target exponent -> multiply coefficient by 10^exp_diff.
            let multiplier = BigInt::from(10u32).pow(u32::try_from(exp_diff).unwrap_or(0));
            result.coefficient *= multiplier;
        } else {
            // Higher target exponent -> divide coefficient by 10^(-exp_diff).
            // This truncates when exact scaling is impossible.
            let divisor = BigInt::from(10u32).pow(u32::try_from(-exp_diff).unwrap_or(0));
            result.coefficient /= divisor;
        }
        result.exponent = target_exp;
        result
    }

    /// Quantizes this decimal to have the same exponent as `other`.
    ///
    /// Uses the caller-provided decimal rounding mode whenever discarded digits
    /// are required to reach the target exponent.
    fn quantize(&self, other: &Self, rounding_mode: DecimalRoundingMode) -> Result<Self, String> {
        // Handle special values
        if self.is_nan() || other.is_nan() {
            return Ok(self.clone());
        }
        if self.is_infinite() {
            if other.is_infinite() {
                // Both infinite - return self
                return Ok(self.clone());
            }
            return Ok(self.clone());
        }
        if other.is_infinite() {
            return Err("Cannot quantize to infinity".to_string());
        }

        let target_exp = other.exponent;
        let exp_diff = self.exponent - target_exp;

        if exp_diff == 0 {
            return Ok(self.clone());
        }

        let mut result = self.clone();
        if exp_diff > 0 {
            // Need more fractional digits - multiply coefficient.
            let multiplier = BigInt::from(10u32).pow(u32::try_from(exp_diff).unwrap_or(0));
            result.coefficient *= multiplier;
        } else {
            // Need fewer fractional digits - divide and round.
            let divisor = BigInt::from(10u32).pow(u32::try_from(-exp_diff).unwrap_or(0));
            let (quotient, remainder) = result.coefficient.div_rem(&divisor);

            let should_round_away = Self::should_round_away_from_zero(rounding_mode, &quotient, &remainder, &divisor);
            result.coefficient = if should_round_away {
                if remainder.is_negative() {
                    quotient - 1
                } else {
                    quotient + 1
                }
            } else {
                quotient
            };
        }
        result.exponent = target_exp;
        if result.coefficient.is_zero() {
            result.negative_zero = self.is_signed();
        } else {
            result.negative_zero = false;
        }
        Ok(result)
    }

    /// Returns whether quantize should increment the truncated quotient away
    /// from zero for the provided remainder and rounding mode.
    fn should_round_away_from_zero(
        rounding_mode: DecimalRoundingMode,
        quotient: &BigInt,
        remainder: &BigInt,
        divisor: &BigInt,
    ) -> bool {
        if remainder.is_zero() {
            return false;
        }

        match rounding_mode {
            DecimalRoundingMode::Up => true,
            DecimalRoundingMode::Down => false,
            DecimalRoundingMode::Ceiling => remainder.is_positive(),
            DecimalRoundingMode::Floor => remainder.is_negative(),
            DecimalRoundingMode::HalfUp => (remainder.abs() * 2) >= *divisor,
            DecimalRoundingMode::HalfDown => (remainder.abs() * 2) > *divisor,
            DecimalRoundingMode::HalfEven => {
                let doubled_remainder = remainder.abs() * 2;
                if doubled_remainder > *divisor {
                    true
                } else if doubled_remainder == *divisor {
                    (quotient.abs() % BigInt::from(2u8)) == BigInt::from(1u8)
                } else {
                    false
                }
            }
            DecimalRoundingMode::O5Up => {
                let last_digit = quotient.abs() % BigInt::from(10u8);
                last_digit.is_zero() || last_digit == BigInt::from(5u8)
            }
        }
    }

    /// Converts to engineering string format (exponent is multiple of 3).
    fn to_eng_string(&self) -> String {
        if !self.is_finite() {
            return self.to_string();
        }

        if self.exponent <= 0 {
            let adjusted = i32::try_from(self.coefficient.abs().to_string().len()).unwrap_or(1) + self.exponent - 1;
            if adjusted >= -6 {
                return self.to_string();
            }
        }

        let digits = self.coefficient.abs().to_string();
        let sign = if self.coefficient.is_negative() { "-" } else { "" };
        let adjusted = i32::try_from(digits.len()).unwrap_or(1) + self.exponent - 1;
        let eng_exp = adjusted - adjusted.rem_euclid(3);
        let int_digits = usize::try_from(adjusted - eng_exp + 1).unwrap_or(1);

        let mut sig = digits;
        if sig.len() < int_digits {
            sig.push_str(&"0".repeat(int_digits - sig.len()));
        }
        if int_digits >= sig.len() {
            format!("{sign}{sig}E{eng_exp:+}")
        } else {
            let (int_part, frac_part) = sig.split_at(int_digits);
            format!("{sign}{int_part}.{frac_part}E{eng_exp:+}")
        }
    }

    /// Adds two decimals.
    pub(crate) fn add(&self, other: &Self) -> Self {
        // Handle NaN
        if self.is_snan() || other.is_snan() {
            return Self::nan(false, false);
        }
        if self.is_nan() {
            return self.clone();
        }
        if other.is_nan() {
            return other.clone();
        }

        // Handle infinity
        if self.is_infinite() && other.is_infinite() {
            let self_neg = self.is_signed();
            let other_neg = other.is_signed();
            if self_neg == other_neg {
                return self.clone();
            }
            // Inf + (-Inf) = NaN
            return Self::nan(false, false);
        }
        if self.is_infinite() {
            return self.clone();
        }
        if other.is_infinite() {
            return other.clone();
        }

        // Align exponents at the most precise (lowest) exponent.
        let target_exp = self.exponent.min(other.exponent);
        let a = self.adjust_exponent(target_exp);
        let b = other.adjust_exponent(target_exp);
        Self::new(&a.coefficient + &b.coefficient, target_exp)
    }

    /// Subtracts two decimals.
    pub(crate) fn sub(&self, other: &Self) -> Self {
        self.add(&other.negate())
    }

    /// Multiplies two decimals.
    pub(crate) fn mul(&self, other: &Self) -> Self {
        // Handle NaN
        if self.is_snan() || other.is_snan() {
            return Self::nan(false, false);
        }
        if self.is_nan() || other.is_nan() {
            return Self::nan(false, false);
        }

        // Handle infinity
        if self.is_infinite() || other.is_infinite() {
            if self.is_zero() || other.is_zero() {
                // 0 * Inf = NaN
                return Self::nan(false, false);
            }
            let result_negative = self.is_signed() != other.is_signed();
            return Self::infinity(result_negative);
        }

        let coefficient = &self.coefficient * &other.coefficient;
        let exponent = self.exponent + other.exponent;
        let mut result = Self::new(coefficient, exponent);
        if result.coefficient.is_zero() {
            result.negative_zero = self.is_signed() != other.is_signed();
        }
        result
    }

    /// Divides two decimals (true division).
    pub(crate) fn div(&self, other: &Self) -> Self {
        // Handle NaN
        if self.is_snan() || other.is_snan() {
            return Self::nan(false, false);
        }
        if self.is_nan() || other.is_nan() {
            return Self::nan(false, false);
        }

        // Handle infinity
        if self.is_infinite() && other.is_infinite() {
            return Self::nan(false, false);
        }
        if other.is_infinite() {
            let result_negative = self.is_signed() != other.is_signed();
            return Self::new(BigInt::ZERO, 0).with_sign(result_negative);
        }
        if self.is_infinite() {
            let result_negative = self.is_signed() != other.is_signed();
            return Self::infinity(result_negative);
        }

        // Division by zero
        if other.is_zero() {
            if self.is_zero() {
                // 0 / 0 = NaN
                return Self::nan(false, false);
            }
            let result_negative = self.is_signed() != other.is_signed();
            return Self::infinity(result_negative);
        }

        // Perform division using current decimal context precision.
        let precision = decimal_mod::current_precision();

        // Scale up the dividend to get desired precision
        let scale_factor =
            precision - self.coefficient.to_string().len() as i32 + other.coefficient.to_string().len() as i32;
        let scale_power = if scale_factor > 0 {
            BigInt::from(10u32).pow(u32::try_from(scale_factor).unwrap_or(0))
        } else {
            BigInt::from(1)
        };

        let scaled_dividend = &self.coefficient * scale_power;
        let (quotient, _remainder) = scaled_dividend.div_rem(&other.coefficient);

        let exponent = self.exponent - other.exponent - scale_factor;
        let mut result = Self::new(quotient, exponent);
        if result.coefficient.is_zero() {
            result.negative_zero = self.is_signed() != other.is_signed();
        }
        result.normalize();
        result
    }

    /// Floor division.
    pub(crate) fn floor_div(&self, other: &Self) -> Self {
        let div_result = self.div(other);
        if !div_result.is_finite() {
            return div_result;
        }
        div_result.to_integral_value(false)
    }

    /// Modulo operation.
    pub(crate) fn modulo(&self, other: &Self) -> Self {
        // a % b = a - b * (a // b)
        let div_result = self.floor_div(other);
        if !div_result.is_finite() {
            return Self::nan(false, false);
        }
        let product = other.mul(&div_result);
        self.sub(&product)
    }

    /// Raises to a power.
    pub(crate) fn pow(&self, exp: &Self) -> Self {
        // Handle NaN
        if self.is_snan() || exp.is_snan() {
            return Self::nan(false, false);
        }
        if self.is_nan() || exp.is_nan() {
            return Self::nan(false, false);
        }

        if !exp.is_finite() {
            return Self::nan(false, false);
        }

        if exp.exponent != 0 {
            let Some(base) = self.to_f64() else {
                return Self::nan(false, false);
            };
            let Some(power) = exp.to_f64() else {
                return Self::nan(false, false);
            };
            return Self::from_f64_with_context(base.powf(power), decimal_mod::current_precision());
        }

        // Try to convert exponent to i32
        let exp_i32 = match exp.coefficient.to_i32() {
            Some(n) => n,
            None => return Self::nan(false, false),
        };

        if exp_i32 < 0 {
            // Negative exponent: 1 / (self^|exp|)
            let base = Self::new(BigInt::from(1), 0);
            let pos_pow = self.pow(&Self::new(BigInt::from(-exp_i32), 0));
            return base.div(&pos_pow);
        }

        if exp_i32 == 0 {
            return Self::new(BigInt::from(1), 0);
        }

        // Handle infinity
        if self.is_infinite() {
            if exp_i32 == 0 {
                return Self::new(BigInt::from(1), 0);
            }
            let result_negative = if exp_i32 % 2 == 1 { self.is_signed() } else { false };
            return Self::infinity(result_negative);
        }

        // Compute power using BigInt
        let pow_coefficient = self.coefficient.pow(u32::try_from(exp_i32).unwrap_or(0));
        let pow_exponent = self.exponent * exp_i32;
        Self::new(pow_coefficient, pow_exponent)
    }

    /// Converts to an integral value.
    /// If floor is true, rounds toward negative infinity (floor).
    /// Otherwise rounds toward zero (truncate).
    fn to_integral_value(&self, floor: bool) -> Self {
        if !self.is_finite() {
            return self.clone();
        }

        if self.exponent >= 0 {
            // Already an integer
            return self.clone();
        }

        let divisor = BigInt::from(10u32).pow(u32::try_from(-self.exponent).unwrap_or(0));
        let (quotient, remainder) = self.coefficient.div_rem(&divisor);

        if remainder.is_zero() {
            return Self::new(quotient, 0);
        }

        if floor && remainder.is_negative() {
            // Round toward -infinity
            Self::new(quotient - 1, 0)
        } else {
            // Round toward zero
            Self::new(quotient, 0)
        }
    }

    /// Converts to integral using context rounding (half-even).
    pub(crate) fn to_integral_nearest(&self) -> Self {
        let target = Self::from_i64(1);
        self.quantize(&target, DecimalRoundingMode::HalfEven)
            .unwrap_or_else(|_| self.clone())
    }

    /// Helper to set the sign of a zero value.
    fn with_sign(mut self, negative: bool) -> Self {
        match self.special {
            SpecialValue::None => {
                if self.coefficient.is_zero() {
                    self.negative_zero = negative;
                } else if self.coefficient.is_negative() != negative {
                    self.coefficient = -self.coefficient;
                }
            }
            SpecialValue::Infinity { .. } => {
                self.special = SpecialValue::Infinity { negative };
            }
            SpecialValue::Nan { signaling, .. } => {
                self.special = SpecialValue::Nan { negative, signaling };
            }
        }
        self
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.special {
            SpecialValue::Infinity { negative } => {
                if negative {
                    write!(f, "-Infinity")
                } else {
                    write!(f, "Infinity")
                }
            }
            SpecialValue::Nan { negative, signaling } => {
                let prefix = if signaling { "s" } else { "" };
                let sign = if negative { "-" } else { "" };
                write!(f, "{sign}{prefix}NaN")
            }
            SpecialValue::None => {
                if self.coefficient.is_zero() {
                    let sign = if self.negative_zero { "-" } else { "" };
                    if self.exponent == 0 {
                        return write!(f, "{sign}0");
                    }
                    if self.exponent < 0 {
                        let frac_zeros = usize::try_from(-self.exponent - 1).unwrap_or(0);
                        return write!(f, "{sign}0.{}0", "0".repeat(frac_zeros));
                    }
                    return write!(f, "{sign}0E{:+}", self.exponent);
                }

                let digits = self.coefficient.abs().to_string();
                let sign = if self.coefficient.is_negative() { "-" } else { "" };
                let adjusted = i32::try_from(digits.len()).unwrap_or(1) + self.exponent - 1;

                if self.exponent <= 0 && adjusted >= -6 {
                    let point = i32::try_from(digits.len()).unwrap_or(0) + self.exponent;
                    if point > 0 {
                        let point = usize::try_from(point).unwrap_or(0);
                        let (int_part, frac_part) = digits.split_at(point);
                        if frac_part.is_empty() {
                            write!(f, "{sign}{int_part}")
                        } else {
                            write!(f, "{sign}{int_part}.{frac_part}")
                        }
                    } else {
                        let zeros = usize::try_from(-point).unwrap_or(0);
                        write!(f, "{sign}0.{}{}", "0".repeat(zeros), digits)
                    }
                } else if digits.len() == 1 {
                    write!(f, "{sign}{digits}E{adjusted:+}")
                } else {
                    let (first, rest) = digits.split_at(1);
                    write!(f, "{sign}{first}.{rest}E{adjusted:+}")
                }
            }
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Handle NaN (incomparable)
        if self.is_nan() || other.is_nan() {
            return None;
        }

        // Handle infinity
        if self.is_infinite() && other.is_infinite() {
            let self_neg = self.is_signed();
            let other_neg = other.is_signed();
            if self_neg == other_neg {
                return Some(Ordering::Equal);
            }
            return if self_neg {
                Some(Ordering::Less)
            } else {
                Some(Ordering::Greater)
            };
        }
        if self.is_infinite() {
            return if self.is_signed() {
                Some(Ordering::Less)
            } else {
                Some(Ordering::Greater)
            };
        }
        if other.is_infinite() {
            return if other.is_signed() {
                Some(Ordering::Greater)
            } else {
                Some(Ordering::Less)
            };
        }

        // Align exponents at the most precise (lowest) exponent.
        let target_exp = self.exponent.min(other.exponent);
        let a = self.adjust_exponent(target_exp);
        let b = other.adjust_exponent(target_exp);
        a.coefficient.partial_cmp(&b.coefficient)
    }
}

impl PyTrait for Decimal {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Decimal
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        self == other
    }

    fn py_cmp(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> Option<Ordering> {
        self.partial_cmp(other)
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Decimal doesn't hold any HeapIds
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        !self.is_zero()
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut ahash::AHashSet<HeapId>,
        _interns: &Interns,
    ) -> fmt::Result {
        write!(f, "Decimal('{self}')")
    }

    fn py_str(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Owned(self.to_string())
    }

    fn py_estimate_size(&self) -> usize {
        // Estimate based on coefficient size
        let coeff_bytes = self.coefficient.to_bytes_le().1.len();
        coeff_bytes + std::mem::size_of::<Self>()
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let attr_str = attr.as_str(interns);

        match attr_str {
            "quantize" => {
                let (exp_arg, rounding_arg, context_arg) = parse_quantize_args(args, heap, interns)?;
                defer_drop!(exp_arg, heap);
                defer_drop!(rounding_arg, heap);
                defer_drop!(context_arg, heap);

                let rounding_mode =
                    resolve_quantize_rounding_mode(rounding_arg.as_ref(), context_arg.as_ref(), heap, interns)?;
                let other = value_to_decimal(exp_arg, heap)?;
                match self.quantize(&other, rounding_mode) {
                    Ok(result) => {
                        let heap_id = heap.allocate(HeapData::Decimal(result))?;
                        Ok(Value::Ref(heap_id))
                    }
                    Err(e) => Err(SimpleException::new_msg(ExcType::ValueError, e).into()),
                }
            }
            "to_eng_string" => {
                args.check_zero_args("to_eng_string", heap)?;
                // Create a string on the heap for the result
                let s = self.to_eng_string();
                let str_obj = crate::types::Str::from(s);
                let heap_id = heap.allocate(HeapData::Str(str_obj))?;
                Ok(Value::Ref(heap_id))
            }
            "copy_abs" => {
                args.check_zero_args("copy_abs", heap)?;
                let result = self.abs();
                let heap_id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(heap_id))
            }
            "copy_negate" => {
                args.check_zero_args("copy_negate", heap)?;
                let result = self.negate();
                let heap_id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(heap_id))
            }
            "copy_sign" => {
                let other_arg = args.get_one_arg("copy_sign", heap)?;
                defer_drop!(other_arg, heap);

                let other = value_to_decimal(other_arg, heap)?;
                let result = self.copy_sign(&other);
                let heap_id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(heap_id))
            }
            "is_finite" => {
                args.check_zero_args("is_finite", heap)?;
                Ok(Value::Bool(self.is_finite()))
            }
            "is_infinite" => {
                args.check_zero_args("is_infinite", heap)?;
                Ok(Value::Bool(self.is_infinite()))
            }
            "is_nan" => {
                args.check_zero_args("is_nan", heap)?;
                Ok(Value::Bool(self.is_nan()))
            }
            "is_zero" => {
                args.check_zero_args("is_zero", heap)?;
                Ok(Value::Bool(self.is_zero()))
            }
            "is_signed" => {
                args.check_zero_args("is_signed", heap)?;
                Ok(Value::Bool(self.is_signed()))
            }
            "as_tuple" => {
                args.check_zero_args("as_tuple", heap)?;
                let tuple = StdlibObject::new_decimal_tuple(
                    self.sign_bit(),
                    self.coefficient_digits(),
                    i64::from(self.exponent_value()),
                );
                let id = heap.allocate(HeapData::StdlibObject(tuple))?;
                Ok(Value::Ref(id))
            }
            "as_integer_ratio" => {
                args.check_zero_args("as_integer_ratio", heap)?;
                let Some((numerator, denominator)) = self.as_integer_ratio() else {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        "cannot convert non-finite Decimal to ratio",
                    )
                    .into());
                };
                let numerator = LongInt::new(numerator).into_value(heap)?;
                let denominator = LongInt::new(denominator).into_value(heap)?;
                let items = smallvec::smallvec![numerator, denominator];
                Ok(allocate_tuple(items, heap)?)
            }
            "adjusted" => {
                args.check_zero_args("adjusted", heap)?;
                Ok(Value::Int(i64::from(self.adjusted())))
            }
            "canonical" => {
                args.check_zero_args("canonical", heap)?;
                let id = heap.allocate(HeapData::Decimal(self.clone()))?;
                Ok(Value::Ref(id))
            }
            "compare" | "compare_signal" | "compare_total" | "compare_total_mag" => {
                let other_arg = args.get_one_arg(attr_str, heap)?;
                defer_drop!(other_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                let result = match attr_str {
                    "compare_total_mag" => self.abs().compare_result(&other.abs()),
                    _ => self.compare_result(&other),
                };
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "conjugate" => {
                args.check_zero_args("conjugate", heap)?;
                let id = heap.allocate(HeapData::Decimal(self.clone()))?;
                Ok(Value::Ref(id))
            }
            "exp" => {
                args.check_zero_args("exp", heap)?;
                let result = self.exp_with_prec(decimal_mod::current_precision());
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "fma" => {
                let (other_arg, third_arg) = args.get_two_args("fma", heap)?;
                defer_drop!(other_arg, heap);
                defer_drop!(third_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                let third = value_to_decimal(third_arg, heap)?;
                let result = self.mul(&other).add(&third);
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "is_canonical" => {
                args.check_zero_args("is_canonical", heap)?;
                Ok(Value::Bool(self.is_canonical()))
            }
            "is_normal" => {
                args.check_zero_args("is_normal", heap)?;
                Ok(Value::Bool(self.is_normal()))
            }
            "is_qnan" => {
                args.check_zero_args("is_qnan", heap)?;
                Ok(Value::Bool(self.is_qnan()))
            }
            "is_snan" => {
                args.check_zero_args("is_snan", heap)?;
                Ok(Value::Bool(self.is_snan()))
            }
            "is_subnormal" => {
                args.check_zero_args("is_subnormal", heap)?;
                Ok(Value::Bool(self.is_subnormal()))
            }
            "ln" => {
                args.check_zero_args("ln", heap)?;
                let result = self.ln_with_prec(decimal_mod::current_precision());
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "log10" => {
                args.check_zero_args("log10", heap)?;
                let result = self.log10_with_prec(decimal_mod::current_precision()).normalized();
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "logb" => {
                args.check_zero_args("logb", heap)?;
                let result = Self::from_i64(i64::from(self.adjusted()));
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "logical_and" | "logical_or" | "logical_xor" => {
                let other_arg = args.get_one_arg(attr_str, heap)?;
                defer_drop!(other_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                let result = match attr_str {
                    "logical_and" => self.logical_and(&other),
                    "logical_or" => self.logical_or(&other),
                    "logical_xor" => self.logical_xor(&other),
                    _ => unreachable!(),
                };
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "logical_invert" => {
                args.check_zero_args("logical_invert", heap)?;
                let id = heap.allocate(HeapData::Decimal(self.logical_invert()))?;
                Ok(Value::Ref(id))
            }
            "max" | "max_mag" | "min" | "min_mag" => {
                let other_arg = args.get_one_arg(attr_str, heap)?;
                defer_drop!(other_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                let result = match attr_str {
                    "max" => self.max_decimal(&other),
                    "max_mag" => self.max_mag(&other),
                    "min" => self.min_decimal(&other),
                    "min_mag" => self.min_mag(&other),
                    _ => unreachable!(),
                };
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "next_minus" => {
                args.check_zero_args("next_minus", heap)?;
                let id = heap.allocate(HeapData::Decimal(self.next_minus()))?;
                Ok(Value::Ref(id))
            }
            "next_plus" => {
                args.check_zero_args("next_plus", heap)?;
                let id = heap.allocate(HeapData::Decimal(self.next_plus()))?;
                Ok(Value::Ref(id))
            }
            "next_toward" => {
                let other_arg = args.get_one_arg("next_toward", heap)?;
                defer_drop!(other_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                let id = heap.allocate(HeapData::Decimal(self.next_toward(&other)))?;
                Ok(Value::Ref(id))
            }
            "normalize" => {
                args.check_zero_args("normalize", heap)?;
                let id = heap.allocate(HeapData::Decimal(self.normalized()))?;
                Ok(Value::Ref(id))
            }
            "number_class" => {
                args.check_zero_args("number_class", heap)?;
                let id = heap.allocate(HeapData::Str(self.number_class().into()))?;
                Ok(Value::Ref(id))
            }
            "radix" => {
                args.check_zero_args("radix", heap)?;
                Ok(Value::Int(10))
            }
            "remainder_near" => {
                let other_arg = args.get_one_arg("remainder_near", heap)?;
                defer_drop!(other_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                let id = heap.allocate(HeapData::Decimal(self.remainder_near(&other).normalized()))?;
                Ok(Value::Ref(id))
            }
            "rotate" => {
                let shift_arg = args.get_one_arg("rotate", heap)?;
                defer_drop!(shift_arg, heap);
                let shift = shift_arg.as_int(heap)?;
                let id = heap.allocate(HeapData::Decimal(self.shift(i32::try_from(shift).unwrap_or(0))))?;
                Ok(Value::Ref(id))
            }
            "same_quantum" => {
                let other_arg = args.get_one_arg("same_quantum", heap)?;
                defer_drop!(other_arg, heap);
                let other = value_to_decimal(other_arg, heap)?;
                Ok(Value::Bool(self.exponent_value() == other.exponent_value()))
            }
            "scaleb" => {
                let exp_arg = args.get_one_arg("scaleb", heap)?;
                defer_drop!(exp_arg, heap);
                let exp = exp_arg.as_int(heap)?;
                let exp = i32::try_from(exp).unwrap_or(0);
                let id = heap.allocate(HeapData::Decimal(self.scaleb(exp)))?;
                Ok(Value::Ref(id))
            }
            "shift" => {
                let exp_arg = args.get_one_arg("shift", heap)?;
                defer_drop!(exp_arg, heap);
                let exp = exp_arg.as_int(heap)?;
                let exp = i32::try_from(exp).unwrap_or(0);
                let id = heap.allocate(HeapData::Decimal(self.shift(exp)))?;
                Ok(Value::Ref(id))
            }
            "sqrt" => {
                args.check_zero_args("sqrt", heap)?;
                let result = self.sqrt_with_prec(decimal_mod::current_precision()).normalized();
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            "to_integral" | "to_integral_exact" | "to_integral_value" => {
                args.check_zero_args(attr_str, heap)?;
                let result = self.to_integral_nearest();
                let id = heap.allocate(HeapData::Decimal(result))?;
                Ok(Value::Ref(id))
            }
            _ => Err(ExcType::attribute_error(self.py_type(heap), attr_str)),
        }
    }
}

/// Parses `Decimal.quantize(exp, rounding=None, context=None)` arguments.
///
/// Accepts positional/keyword arguments following CPython's parameter names and
/// returns resolved slots for `exp`, `rounding`, and `context`.
fn parse_quantize_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Option<Value>, Option<Value>)> {
    const PARAM_NAMES: [&str; 3] = ["exp", "rounding", "context"];

    let (positional, kwargs) = args.into_parts();
    let positional_count = positional.len();
    if positional_count > PARAM_NAMES.len() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(
            "quantize",
            PARAM_NAMES.len(),
            positional_count,
            0,
        ));
    }

    let mut slots = vec![None, None, None];
    for (index, value) in positional.into_iter().enumerate() {
        slots[index] = Some(value);
    }

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            slots.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);

        let index = match key_name.as_str() {
            "exp" => 0,
            "rounding" => 1,
            "context" => 2,
            _ => {
                value.drop_with_heap(heap);
                slots.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("quantize", &key_name));
            }
        };
        if slots[index].is_some() {
            value.drop_with_heap(heap);
            slots.drop_with_heap(heap);
            return Err(ExcType::type_error_duplicate_arg("quantize", PARAM_NAMES[index]));
        }
        slots[index] = Some(value);
    }

    let Some(exp) = slots[0].take() else {
        slots.drop_with_heap(heap);
        return Err(ExcType::type_error_missing_positional_with_names("quantize", &["exp"]));
    };
    let rounding = slots[1].take();
    let context = slots[2].take();
    slots.drop_with_heap(heap);
    Ok((exp, rounding, context))
}

/// Resolves the effective quantize rounding mode from optional arguments.
///
/// Priority matches CPython:
/// 1. Explicit `rounding=` if not `None`
/// 2. Explicit `context.rounding` if `context` is provided and not `None`
/// 3. Current global decimal context rounding
fn resolve_quantize_rounding_mode(
    rounding: Option<&Value>,
    context: Option<&Value>,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<DecimalRoundingMode> {
    let rounding_name = if let Some(rounding) = rounding.filter(|value| !matches!(value, Value::None)) {
        rounding.py_str(heap, interns).into_owned()
    } else if let Some(context) = context.filter(|value| !matches!(value, Value::None)) {
        context_rounding_name(context, heap)?
    } else {
        decimal_mod::get_current_context_config().rounding
    };

    DecimalRoundingMode::from_name(&rounding_name).ok_or_else(ExcType::type_error_decimal_invalid_rounding)
}

/// Returns `context.rounding` from a runtime context object value.
fn context_rounding_name(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<String> {
    let Value::Ref(context_id) = value else {
        return Err(ExcType::type_error("optional argument must be a context"));
    };
    let HeapData::StdlibObject(context_obj) = heap.get(*context_id) else {
        return Err(ExcType::type_error("optional argument must be a context"));
    };
    let Some(context_config) = context_obj.decimal_context_config() else {
        return Err(ExcType::type_error("optional argument must be a context"));
    };
    Ok(context_config.rounding)
}

/// Converts a Value to a Decimal.
///
/// Accepts Decimal (Ref), Int, Bool, and LongInt.
/// Returns a TypeError for other types.
fn value_to_decimal(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<Decimal> {
    match value {
        Value::Ref(heap_id) => {
            if let HeapData::Decimal(d) = heap.get(*heap_id) {
                Ok(d.clone())
            } else if let HeapData::LongInt(li) = heap.get(*heap_id) {
                // Convert LongInt to Decimal
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_from_string() {
        let d = Decimal::from_string("3.14159").unwrap();
        assert_eq!(d.to_string(), "3.14159");

        let d = Decimal::from_string("-123.456").unwrap();
        assert_eq!(d.to_string(), "-123.456");

        let d = Decimal::from_string("1000").unwrap();
        assert_eq!(d.to_string(), "1000");
    }

    #[test]
    fn test_decimal_arithmetic() {
        let a = Decimal::from_string("3.14").unwrap();
        let b = Decimal::from_string("2.86").unwrap();

        let sum = a.add(&b);
        assert_eq!(sum.to_string(), "6.00");

        let diff = a.sub(&b);
        assert_eq!(diff.to_string(), "0.28");

        let prod = a.mul(&b);
        assert_eq!(prod.to_string(), "8.9804");
    }

    #[test]
    fn test_decimal_division() {
        let a = Decimal::from_string("10").unwrap();
        let b = Decimal::from_string("3").unwrap();

        let div = a.div(&b);
        assert!(div.to_string().starts_with("3.3333"));
    }

    #[test]
    fn test_decimal_quantize() {
        let d = Decimal::from_string("3.14159").unwrap();
        let exp = Decimal::from_string("0.01").unwrap();

        let quantized = d.quantize(&exp, DecimalRoundingMode::HalfEven).unwrap();
        assert_eq!(quantized.to_string(), "3.14");
    }

    #[test]
    fn test_decimal_quantize_rounding_modes() {
        let d = Decimal::from_string("19.995").unwrap();
        let exp = Decimal::from_string("0.01").unwrap();

        let rounded = d.quantize(&exp, DecimalRoundingMode::HalfUp).unwrap();
        assert_eq!(rounded.to_string(), "20.00");

        let truncated = d.quantize(&exp, DecimalRoundingMode::Down).unwrap();
        assert_eq!(truncated.to_string(), "19.99");
    }

    #[test]
    fn test_decimal_comparison() {
        let a = Decimal::from_string("3.14").unwrap();
        let b = Decimal::from_string("2.71").unwrap();

        assert!(a.partial_cmp(&b) == Some(Ordering::Greater));
        assert!(b.partial_cmp(&a) == Some(Ordering::Less));
        assert!(a.partial_cmp(&a) == Some(Ordering::Equal));
    }
}
