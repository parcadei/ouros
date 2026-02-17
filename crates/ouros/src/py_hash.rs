//! CPython-compatible hash helpers used for deterministic Ouros hashing.
//!
//! Ouros intentionally uses deterministic hashing equivalent to
//! `PYTHONHASHSEED=0` so parity tests remain stable. CPython hashes text/bytes
//! with SipHash-1-3 and a zeroed key under that seed; these helpers expose that
//! behavior for string/bytes hash paths that affect dict/set ordering.
//!
//! ## Cross-type hash invariant
//!
//! CPython guarantees that if `a == b`, then `hash(a) == hash(b)`. Since
//! `0 == 0.0 == False` and `1 == 1.0 == True`, the hash functions for int,
//! float, and bool must produce identical values for equivalent inputs. This
//! module provides [`cpython_hash_int`] and [`cpython_hash_float`] which
//! implement the same Mersenne-prime modular algorithm used by CPython's
//! `Objects/longobject.c` and `Objects/floatobject.c`.

/// Hashes raw bytes using CPython's `PYTHONHASHSEED=0` SipHash-1-3 behavior.
///
/// This applies CPython's two key conventions:
/// - empty input hashes to `0`
/// - a computed hash of `-1` is remapped to `-2`
#[must_use]
pub(crate) fn cpython_hash_bytes_seed0(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }

    let raw = siphash13_with_seed0(bytes);
    let signed = i64::from_ne_bytes(raw.to_ne_bytes());
    let adjusted = if signed == -1 { -2 } else { signed };
    u64::from_ne_bytes(adjusted.to_ne_bytes())
}

/// Hashes bytes and returns the signed Python hash value.
///
/// This is a convenience wrapper for sites that operate on signed hash lanes
/// (for example tuple hash mixing).
#[must_use]
pub(crate) fn cpython_hash_bytes_seed0_i64(bytes: &[u8]) -> i64 {
    i64::from_ne_bytes(cpython_hash_bytes_seed0(bytes).to_ne_bytes())
}

/// Hashes UTF-8 string content with CPython's deterministic seed-0 algorithm.
#[must_use]
pub(crate) fn cpython_hash_str_seed0(value: &str) -> u64 {
    cpython_hash_bytes_seed0(value.as_bytes())
}

/// Mersenne prime used by CPython for numeric hashing: `2^61 - 1`.
///
/// All numeric types (int, float, bool, Fraction, Decimal) hash modulo this
/// prime so that equal values across types produce identical hashes.
const MODULUS: i64 = (1 << 61) - 1;

/// Hashes a signed 64-bit integer using CPython's modular algorithm.
///
/// The algorithm is `n % MODULUS` (sign-preserving), with the special case
/// that a result of `-1` is remapped to `-2` (CPython reserves `-1` as an
/// internal error sentinel in C). This matches CPython's `long_hash` in
/// `Objects/longobject.c`.
///
/// The returned `u64` is the bit-reinterpretation of the signed result,
/// matching the convention used by all other `cpython_hash_*` helpers.
#[must_use]
pub(crate) fn cpython_hash_int(value: i64) -> u64 {
    let result = cpython_hash_int_signed(value);
    u64::from_ne_bytes(result.to_ne_bytes())
}

/// Signed version of [`cpython_hash_int`], used internally and by float hashing.
fn cpython_hash_int_signed(value: i64) -> i64 {
    if value == 0 {
        return 0;
    }

    let sign: i64 = if value < 0 { -1 } else { 1 };

    // For positive modulus: work with the absolute value.
    // We need to be careful with i64::MIN whose absolute value overflows i64.
    let abs_val: u64 = i128::from(value).unsigned_abs() as u64;
    let modulus_u = MODULUS as u64;
    let remainder = (abs_val % modulus_u) as i64;

    let result = sign * remainder;
    if result == -1 { -2 } else { result }
}

/// Hashes an `f64` using CPython's float hashing algorithm.
///
/// For integral float values (like `1.0`, `42.0`), this delegates to
/// [`cpython_hash_int`] so that `hash(n) == hash(float(n))` holds. For
/// non-integral floats it uses a `frexp`-based decomposition identical to
/// CPython's `float_hash` in `Objects/floatobject.c`.
///
/// Special values:
/// - `+inf` hashes to `314159`
/// - `-inf` hashes to `-314159`
/// - `NaN` hashes to the platform's `_Py_HashNaN` (0 in CPython 3.10+)
#[must_use]
pub(crate) fn cpython_hash_float(value: f64) -> u64 {
    let result = cpython_hash_float_signed(value);
    u64::from_ne_bytes(result.to_ne_bytes())
}

/// Signed implementation of [`cpython_hash_float`].
///
/// Follows CPython `Objects/floatobject.c` `float_hash()` logic:
/// 1. Handle infinities and NaN as special cases.
/// 2. If the float is an exact integer, hash via the integer path.
/// 3. Otherwise decompose with `frexp` and accumulate modulo `MODULUS`.
fn cpython_hash_float_signed(value: f64) -> i64 {
    // Special cases
    if value.is_infinite() {
        return if value > 0.0 { 314159 } else { -314159 };
    }
    if value.is_nan() {
        // CPython 3.10+ returns _Py_HashNaN which varies; on most platforms it is 0.
        // Actually, CPython returns `sys.hash_info.nan` which is 0 on 3.10+.
        // Let's match CPython's actual behavior.
        return 0;
    }

    // If the float is an exact integer value, hash as integer for cross-type consistency.
    // This is the critical path that ensures hash(1.0) == hash(1).
    let truncated = value.trunc();
    if value == truncated {
        // Value is integral. Convert to i64 if possible, otherwise use big-int path.
        if truncated >= i64::MIN as f64 && truncated <= i64::MAX as f64 {
            return cpython_hash_int_signed(truncated as i64);
        }
        // For very large integral floats, use the frexp path below (matches CPython).
    }

    // Non-integral float (or integral float outside i64 range): frexp-based algorithm.
    // This matches CPython's `_Py_HashDouble` in `Python/pyhash.c`.
    let (frac, exp) = frexp(value);
    let mut m = frac;
    let mut e = exp;

    let sign: i64 = if m < 0.0 {
        m = -m;
        -1
    } else {
        1
    };

    // Process the mantissa bits in 28-bit chunks (matching CPython).
    let mut x: u64 = 0;
    while m > 0.0 {
        x = ((x << 28) & (MODULUS as u64)) | (x >> 33);
        m *= 268_435_456.0; // 2^28
        e -= 28;
        let w = m as u64;
        m -= w as f64;
        x = x.wrapping_add(w);
        if x >= MODULUS as u64 {
            x -= MODULUS as u64;
        }
    }

    // Incorporate the exponent.
    e %= 61;
    if e < 0 {
        e += 61;
    }
    x = ((x << e as u32) & (MODULUS as u64)) | (x >> (61 - e) as u32);

    let result = (sign * x as i64) % MODULUS;
    if result == -1 { -2 } else { result }
}

/// Returns `(frac, exp)` such that `value == frac * 2^exp` with `0.5 <= |frac| < 1.0`.
///
/// This is equivalent to C's `frexp()` and Python's `math.frexp()`.
fn frexp(value: f64) -> (f64, i32) {
    if value == 0.0 || value.is_nan() || value.is_infinite() {
        return (value, 0);
    }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    if exponent == 0 {
        // Subnormal: multiply by 2^64 to normalize, then adjust exponent
        let normalized = value * (1u64 << 63) as f64 * 2.0;
        let (frac, exp) = frexp(normalized);
        return (frac, exp - 64);
    }
    // Clear exponent bits and set to bias (1022 = bias-1, giving 0.5 <= |frac| < 1.0)
    let frac_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | (0x3FE0_0000_0000_0000);
    let frac = f64::from_bits(frac_bits);
    let exp = exponent - 1022; // 1023 (bias) - 1 (for 0.5 <= frac < 1.0)
    (frac, exp)
}

/// Computes SipHash-1-3 with a zero key, matching CPython seed-0 parameters.
#[must_use]
fn siphash13_with_seed0(bytes: &[u8]) -> u64 {
    const K0: u64 = 0;
    const K1: u64 = 0;

    let mut v0 = K0 ^ 0x736f_6d65_7073_6575;
    let mut v1 = K1 ^ 0x646f_7261_6e64_6f6d;
    let mut v2 = K0 ^ 0x6c79_6765_6e65_7261;
    let mut v3 = K1 ^ 0x7465_6462_7974_6573;

    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let mut block = [0_u8; 8];
        block.copy_from_slice(chunk);
        let message = u64::from_le_bytes(block);
        v3 ^= message;
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= message;
    }

    let mut tail = (bytes.len() as u64) << 56;
    for (index, byte) in chunks.remainder().iter().copied().enumerate() {
        tail |= u64::from(byte) << (index * 8);
    }

    v3 ^= tail;
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= tail;
    v2 ^= 0xff;
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);

    v0 ^ v1 ^ v2 ^ v3
}

/// Performs one SipHash round.
fn sip_round(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13);
    *v1 ^= *v0;
    *v0 = v0.rotate_left(32);

    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16);
    *v3 ^= *v2;

    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21);
    *v3 ^= *v0;

    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17);
    *v1 ^= *v2;
    *v2 = v2.rotate_left(32);
}
