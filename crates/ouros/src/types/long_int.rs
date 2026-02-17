//! LongInt wrapper for arbitrary precision integer support.
//!
//! This module provides the `LongInt` wrapper type around `num_bigint::BigInt`.
//! Named `LongInt` to avoid confusion with the external `BigInt` type. Python has
//! one `int` type, and LongInt is an implementation detail - we use i64 for performance
//! when values fit, and promote to LongInt on overflow.
//!
//! The design centralizes BigInt-related logic into methods on `LongInt` rather than
//! having freestanding functions scattered across the codebase.

use std::{
    fmt::{self, Display},
    ops::{Add, Mul, Neg, Sub},
};

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::{
    heap::{Heap, HeapData},
    py_hash::cpython_hash_int,
    resource::{ResourceError, ResourceTracker},
    value::Value,
};

/// Wrapper around `num_bigint::BigInt` for arbitrary precision integers.
///
/// Named `LongInt` to avoid confusion with the external `BigInt` type from `num_bigint`.
/// The inner `BigInt` is accessible via `.0` for arithmetic operations that need direct
/// access to the underlying type.
///
/// Python treats all integers as one type - we use `Value::Int(i64)` for values that fit
/// and `LongInt` for larger values. The `into_value()` method automatically demotes to
/// i64 when the value fits, maintaining this optimization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct LongInt(pub BigInt);

impl LongInt {
    /// Creates a new `LongInt` from a `BigInt`.
    pub fn new(bi: BigInt) -> Self {
        Self(bi)
    }

    /// Converts to a `Value`, demoting to i64 if it fits.
    ///
    /// For performance, we want to keep values as `Value::Int(i64)` whenever possible.
    /// This method checks if the value fits in an i64 and returns `Value::Int` if so,
    /// otherwise allocates a `HeapData::LongInt` on the heap.
    pub fn into_value(self, heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
        // Try to demote back to i64 for performance
        if let Some(i) = self.0.to_i64() {
            Ok(Value::Int(i))
        } else {
            let heap_id = heap.allocate(HeapData::LongInt(self))?;
            Ok(Value::Ref(heap_id))
        }
    }

    /// Computes a CPython-compatible hash using the Mersenne-prime modular algorithm.
    ///
    /// Critical: For values that fit in i64, this delegates to [`cpython_hash_int`]
    /// which ensures cross-type consistency (`hash(5) == hash(5.0)`). For values
    /// outside i64 range, it applies the same modular reduction (`n % (2^61 - 1)`)
    /// that CPython uses in `long_hash` (`Objects/longobject.c`).
    pub fn hash(&self) -> u64 {
        /// Mersenne prime `2^61 - 1`, matching CPython's `_PyHASH_MODULUS`.
        const MODULUS: u64 = (1 << 61) - 1;

        // If the LongInt fits in i64, delegate to the shared algorithm
        if let Some(i) = self.0.to_i64() {
            return cpython_hash_int(i);
        }

        // For LongInts outside i64 range, compute n % MODULUS directly on the BigInt.
        // This matches CPython's long_hash which reduces modulo 2^61 - 1.
        let modulus_big = BigInt::from(MODULUS);
        let remainder = &self.0 % &modulus_big;
        let result = remainder.to_i64().unwrap_or(0);
        let adjusted = if result == -1 { -2i64 } else { result };
        u64::from_ne_bytes(adjusted.to_ne_bytes())
    }

    /// Estimates memory size in bytes.
    ///
    /// Used for resource tracking. The actual size includes the Vec overhead
    /// plus the digit storage. Rounds up bits to bytes to avoid underestimating
    /// (e.g., 1 bit = 1 byte, not 0 bytes).
    pub fn estimate_size(&self) -> usize {
        // Each BigInt digit is typically a u32 or u64
        // We estimate based on the number of significant bits
        let bits = self.0.bits();
        // Convert bits to bytes (round up), add overhead for Vec and sign
        // On 32-bit platforms, truncate to usize::MAX if bits is too large
        let bit_bytes = usize::try_from(bits).unwrap_or(usize::MAX).saturating_add(7) / 8;
        bit_bytes + std::mem::size_of::<BigInt>()
    }

    /// Returns a reference to the inner `BigInt`.
    ///
    /// Use this when you need read-only access to the underlying `BigInt`
    /// for operations like formatting or comparison.
    pub fn inner(&self) -> &BigInt {
        &self.0
    }

    /// Checks if the value is zero.
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Checks if the value is negative.
    pub fn is_negative(&self) -> bool {
        self.0.is_negative()
    }

    /// Tries to convert to i64.
    ///
    /// Returns `Some(i64)` if the value fits, `None` otherwise.
    pub fn to_i64(&self) -> Option<i64> {
        self.0.to_i64()
    }

    /// Tries to convert to f64.
    ///
    /// Returns `Some(f64)` if the conversion is possible, `None` if the value
    /// is too large to represent as f64.
    pub fn to_f64(&self) -> Option<f64> {
        self.0.to_f64()
    }

    /// Tries to convert to u32.
    ///
    /// Returns `Some(u32)` if the value fits, `None` otherwise.
    pub fn to_u32(&self) -> Option<u32> {
        self.0.to_u32()
    }

    /// Tries to convert to u64.
    ///
    /// Returns `Some(u64)` if the value fits, `None` otherwise.
    pub fn to_u64(&self) -> Option<u64> {
        self.0.to_u64()
    }

    /// Tries to convert to usize.
    ///
    /// Returns `Some(usize)` if the value fits, `None` otherwise.
    /// Useful for sequence repetition counts.
    pub fn to_usize(&self) -> Option<usize> {
        self.0.to_usize()
    }

    /// Returns the absolute value as a new `LongInt`.
    pub fn abs(&self) -> Self {
        Self(self.0.abs())
    }

    /// Returns the number of significant bits in this LongInt.
    ///
    /// Zero returns 0 bits. For non-zero values, this is the position of the
    /// highest set bit plus one.
    pub fn bits(&self) -> u64 {
        self.0.bits()
    }

    /// Estimates the result size of `base ** exponent` in bytes.
    ///
    /// Returns `None` on overflow, which indicates an astronomically large result
    /// that should likely be rejected. For special cases (0, 1, -1), the actual
    /// result is small regardless of exponent, so callers should check for those
    /// before calling this function.
    pub fn estimate_pow_bytes(base_bits: u64, exponent: u64) -> Option<usize> {
        // result_bits ≈ base_bits * exponent
        let result_bits = base_bits.checked_mul(exponent)?;
        // Round up to bytes
        usize::try_from(result_bits.div_ceil(8)).ok()
    }

    /// Estimates the result size of `value << shift_amount` in bytes.
    ///
    /// Returns `None` on overflow, which indicates an astronomically large result.
    /// For zero values, the result is always zero regardless of shift amount.
    pub fn estimate_lshift_bytes(value_bits: u64, shift_amount: u64) -> Option<usize> {
        let result_bits = value_bits.checked_add(shift_amount)?;
        // Round up to bytes
        usize::try_from(result_bits.div_ceil(8)).ok()
    }

    /// Estimates the result size of `a * b` in bytes.
    ///
    /// Returns `None` on overflow. The result of multiplying two numbers has at most
    /// `a_bits + b_bits` bits.
    pub fn estimate_mult_bytes(a_bits: u64, b_bits: u64) -> Option<usize> {
        let result_bits = a_bits.checked_add(b_bits)?;
        // Round up to bytes
        usize::try_from(result_bits.div_ceil(8)).ok()
    }
}

// === Trait Implementations ===

impl From<BigInt> for LongInt {
    fn from(bi: BigInt) -> Self {
        Self(bi)
    }
}

impl From<i64> for LongInt {
    fn from(i: i64) -> Self {
        Self(BigInt::from(i))
    }
}

impl Add for LongInt {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for LongInt {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Mul for LongInt {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self(self.0 * rhs.0)
    }
}

impl Neg for LongInt {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Display for LongInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
