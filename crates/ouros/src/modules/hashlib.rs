//! Implementation of the `hashlib` module.
//!
//! Provides hash functions for MD5, SHA-1, SHA-2, SHA-3, and SHAKE variants,
//! returning hash objects with methods like `.hexdigest()`, `.digest()`, and `.update()`
//! matching CPython's API for the supported subset.
//!
//! Functions accept `bytes` input (matching CPython's API).
//!
//! Module-level attributes:
//! - `algorithms_available`: set of available algorithm names
//! - `algorithms_guaranteed`: set of guaranteed algorithm names
//!
//! # Example
//! ```python
//! import hashlib
//!
//! # MD5 hash object with methods
//! h = hashlib.md5(b'hello')
//! assert h.hexdigest() == '5d41402abc4b2a76b9719d911017c592'
//!
//! # SHA-256 hash object
//! h2 = hashlib.sha256(b'hello')
//! assert h2.hexdigest() == '2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824'
//!
//! # Incremental hashing with update()
//! h3 = hashlib.sha256()
//! h3.update(b'hello')
//! h3.update(b' world')
//! assert h3.hexdigest() == hashlib.sha256(b'hello world').hexdigest()
//! ```

use std::{cmp, fmt::Write, str::FromStr};

use ahash::AHashSet;
use hmac::{Hmac, Mac, digest::KeyInit};
use md5::{Digest, Md5};
use ripemd::Ripemd160;
use scrypt::{Params as ScryptParams, scrypt as scrypt_derive};
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512, Sha512_224, Sha512_256, digest::OutputSizeUser};
use sha3::{
    Sha3_224, Sha3_256, Sha3_384, Sha3_512, Shake128, Shake256,
    digest::{ExtendableOutput, Update, XofReader},
};
use sm3::Sm3;

use crate::{
    args::{ArgValues, KwargsValues},
    defer_drop,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, Bytes, List, PyTrait, Set, Str, Type},
    value::{EitherStr, Value},
};

/// Hashlib module functions.
///
/// Each variant maps to a hash algorithm that accepts bytes input
/// and returns the hex digest as a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum HashlibFunctions {
    /// Compute MD5 hash and return hash object.
    Md5,
    /// Generic hash constructor that dispatches by algorithm name.
    New,
    /// Compute SHA-1 hash and return hash object.
    Sha1,
    /// Compute SHA-224 hash and return hash object.
    Sha224,
    /// Compute SHA-256 hash and return hash object.
    Sha256,
    /// Compute SHA-384 hash and return hash object.
    Sha384,
    /// Compute SHA-512 hash and return hash object.
    Sha512,
    /// Compute SHA-512/224 hash and return hash object.
    Sha512_224,
    /// Compute SHA-512/256 hash and return hash object.
    Sha512_256,
    /// Compute SHA3-224 hash and return hash object.
    Sha3_224,
    /// Compute SHA3-256 hash and return hash object.
    Sha3_256,
    /// Compute SHA3-384 hash and return hash object.
    Sha3_384,
    /// Compute SHA3-512 hash and return hash object.
    Sha3_512,
    /// Compute BLAKE2b hash and return hash object.
    Blake2b,
    /// Compute BLAKE2s hash and return hash object.
    Blake2s,
    /// Compute SHAKE-128 hash and return hash object.
    Shake128,
    /// Compute SHAKE-256 hash and return hash object.
    Shake256,
    /// Derive a key using scrypt.
    Scrypt,
    /// Derive a key using PBKDF2-HMAC.
    Pbkdf2Hmac,
    /// Compute hash from a file-like object.
    FileDigest,
}

/// Algorithm type for hash objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum HashAlgorithm {
    /// MD5 algorithm.
    Md5,
    /// SHA-1 algorithm.
    Sha1,
    /// SHA-224 algorithm.
    Sha224,
    /// SHA-256 algorithm.
    Sha256,
    /// SHA-384 algorithm.
    Sha384,
    /// SHA-512 algorithm.
    Sha512,
    /// SHA-512/224 algorithm.
    Sha512_224,
    /// SHA-512/256 algorithm.
    Sha512_256,
    /// SHA3-224 algorithm.
    Sha3_224,
    /// SHA3-256 algorithm.
    Sha3_256,
    /// SHA3-384 algorithm.
    Sha3_384,
    /// SHA3-512 algorithm.
    Sha3_512,
    /// BLAKE2b algorithm.
    Blake2b,
    /// BLAKE2s algorithm.
    Blake2s,
    /// SHAKE-128 extendable output function.
    Shake128,
    /// SHAKE-256 extendable output function.
    Shake256,
    /// RIPEMD-160 algorithm.
    Ripemd160,
    /// SM3 algorithm.
    Sm3,
    /// OpenSSL-compatible combined MD5+SHA1 algorithm.
    Md5Sha1,
}

impl HashAlgorithm {
    /// Returns the canonical hashlib name for this algorithm.
    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha224 => "sha224",
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
            Self::Sha512_224 => "sha512_224",
            Self::Sha512_256 => "sha512_256",
            Self::Sha3_224 => "sha3_224",
            Self::Sha3_256 => "sha3_256",
            Self::Sha3_384 => "sha3_384",
            Self::Sha3_512 => "sha3_512",
            Self::Blake2b => "blake2b",
            Self::Blake2s => "blake2s",
            Self::Shake128 => "shake_128",
            Self::Shake256 => "shake_256",
            Self::Ripemd160 => "ripemd160",
            Self::Sm3 => "sm3",
            Self::Md5Sha1 => "md5-sha1",
        }
    }

    /// Returns true when the digest length must be provided by the caller.
    fn requires_output_len(self) -> bool {
        matches!(self, Self::Shake128 | Self::Shake256)
    }

    /// Returns the digest size in bytes exposed by `hashlib` objects.
    #[must_use]
    fn digest_size(self) -> usize {
        match self {
            Self::Md5 => 16,
            Self::Sha1 => 20,
            Self::Sha224 => 28,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
            Self::Sha512_224 => 28,
            Self::Sha512_256 => 32,
            Self::Sha3_224 => 28,
            Self::Sha3_256 => 32,
            Self::Sha3_384 => 48,
            Self::Sha3_512 => 64,
            Self::Blake2b => 64,
            Self::Blake2s => 32,
            Self::Ripemd160 => 20,
            Self::Sm3 => 32,
            Self::Md5Sha1 => 36,
            Self::Shake128 | Self::Shake256 => 0,
        }
    }

    /// Returns the hash block size in bytes exposed by `hashlib` objects.
    #[must_use]
    fn block_size(self) -> usize {
        match self {
            Self::Md5 | Self::Sha1 | Self::Sha224 | Self::Sha256 => 64,
            Self::Sha384 | Self::Sha512 | Self::Sha512_224 | Self::Sha512_256 | Self::Blake2b => 128,
            Self::Sha3_224 => 144,
            Self::Sha3_256 => 136,
            Self::Sha3_384 => 104,
            Self::Sha3_512 => 72,
            Self::Blake2s => 64,
            Self::Ripemd160 => 64,
            Self::Sm3 => 64,
            Self::Md5Sha1 => 64,
            Self::Shake128 => 168,
            Self::Shake256 => 136,
        }
    }
}

/// A hash object that stores accumulated data and computes digests on demand.
///
/// This stores the accumulated bytes rather than the hasher state because
/// the underlying hasher types don't support serde serialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct HashObject {
    /// The hash algorithm being used.
    algorithm: HashAlgorithm,
    /// Accumulated data that has been fed to the hash.
    data: Vec<u8>,
    /// Optional digest size for variable-length hashes (blake2b, blake2s).
    /// None means use the default digest size for the algorithm.
    digest_size: Option<usize>,
}

impl HashObject {
    /// Creates a new hash object with the specified algorithm and optional initial data.
    #[must_use]
    fn new(algorithm: HashAlgorithm, data: Option<&[u8]>, digest_size: Option<usize>) -> Self {
        let mut accumulated = Vec::new();
        if let Some(bytes) = data {
            accumulated.extend_from_slice(bytes);
        }
        Self {
            algorithm,
            data: accumulated,
            digest_size,
        }
    }

    /// Updates the hash with additional data.
    fn update(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Computes and returns the digest as bytes, handling fixed and extendable algorithms.
    fn compute_digest(&self, output_len: Option<usize>) -> RunResult<Vec<u8>> {
        match self.algorithm {
            HashAlgorithm::Md5 => Ok(compute_fixed_digest::<Md5>(&self.data)),
            HashAlgorithm::Sha1 => Ok(compute_fixed_digest::<Sha1>(&self.data)),
            HashAlgorithm::Sha224 => Ok(compute_fixed_digest::<Sha224>(&self.data)),
            HashAlgorithm::Sha256 => Ok(compute_fixed_digest::<Sha256>(&self.data)),
            HashAlgorithm::Sha384 => Ok(compute_fixed_digest::<Sha384>(&self.data)),
            HashAlgorithm::Sha512 => Ok(compute_fixed_digest::<Sha512>(&self.data)),
            HashAlgorithm::Sha512_224 => Ok(compute_fixed_digest::<Sha512_224>(&self.data)),
            HashAlgorithm::Sha512_256 => Ok(compute_fixed_digest::<Sha512_256>(&self.data)),
            HashAlgorithm::Sha3_224 => Ok(compute_fixed_digest::<Sha3_224>(&self.data)),
            HashAlgorithm::Sha3_256 => Ok(compute_fixed_digest::<Sha3_256>(&self.data)),
            HashAlgorithm::Sha3_384 => Ok(compute_fixed_digest::<Sha3_384>(&self.data)),
            HashAlgorithm::Sha3_512 => Ok(compute_fixed_digest::<Sha3_512>(&self.data)),
            HashAlgorithm::Blake2b => Ok(compute_blake2b_digest(&self.data, self.digest_size.unwrap_or(64))),
            HashAlgorithm::Blake2s => Ok(compute_blake2s_digest(&self.data, self.digest_size.unwrap_or(32))),
            HashAlgorithm::Ripemd160 => Ok(compute_fixed_digest::<Ripemd160>(&self.data)),
            HashAlgorithm::Sm3 => Ok(compute_fixed_digest::<Sm3>(&self.data)),
            HashAlgorithm::Md5Sha1 => {
                let mut output = compute_fixed_digest::<Md5>(&self.data);
                output.extend_from_slice(&compute_fixed_digest::<Sha1>(&self.data));
                Ok(output)
            }
            HashAlgorithm::Shake128 => compute_shake_digest::<Shake128>(output_len, &self.data),
            HashAlgorithm::Shake256 => compute_shake_digest::<Shake256>(output_len, &self.data),
        }
    }

    /// Returns the digest as a bytes object.
    fn digest(&self, output_len: Option<usize>) -> RunResult<Vec<u8>> {
        self.compute_digest(output_len)
    }

    /// Returns the digest as a hex string.
    fn hexdigest(&self, output_len: Option<usize>) -> RunResult<String> {
        Ok(bytes_to_hex(&self.digest(output_len)?))
    }

    /// Returns the repr string for this hash object.
    #[must_use]
    pub fn py_repr(&self) -> String {
        let algo = self.algorithm.name();
        format!("<{algo} _hashlib.HASH object>")
    }
}

impl PyTrait for HashObject {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Hash
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        // Hash objects don't have a length
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        // Hash objects compare by identity only, not by content
        false
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {
        // Hash objects don't contain heap references
    }

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.data.capacity()
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        // Hash objects are always truthy
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(f, "{}", self.py_repr())
    }

    fn py_getattr(
        &self,
        attr_id: crate::intern::StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        match interns.get_str(attr_id) {
            "digest_size" => {
                #[expect(clippy::cast_possible_wrap, reason = "digest sizes fit i64")]
                let size = self.digest_size.unwrap_or_else(|| self.algorithm.digest_size()) as i64;
                Ok(Some(AttrCallResult::Value(Value::Int(size))))
            }
            "block_size" => {
                #[expect(clippy::cast_possible_wrap, reason = "block sizes fit i64")]
                let size = self.algorithm.block_size() as i64;
                Ok(Some(AttrCallResult::Value(Value::Int(size))))
            }
            "name" => {
                let id = heap.allocate(HeapData::Str(Str::from(self.algorithm.name())))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
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
        let Some(method) = attr.static_string() else {
            return Err(ExcType::attribute_error(Type::Hash, attr.as_str(interns)));
        };

        match method {
            StaticStrings::HlHexdigest => {
                let output_len = extract_digest_length(self.algorithm, "hexdigest", args, heap)?;
                let hex_str = self.hexdigest(output_len)?;
                let str_obj = Str::from(hex_str);
                let id = heap.allocate(HeapData::Str(str_obj))?;
                Ok(Value::Ref(id))
            }
            StaticStrings::HlDigest => {
                let output_len = extract_digest_length(self.algorithm, "digest", args, heap)?;
                let digest_bytes = self.digest(output_len)?;
                let bytes_obj = Bytes::new(digest_bytes);
                let id = heap.allocate(HeapData::Bytes(bytes_obj))?;
                Ok(Value::Ref(id))
            }
            StaticStrings::Update => {
                let data = args.get_one_arg("update", heap)?;
                defer_drop!(data, heap);
                let data_bytes = extract_bytes(data, heap, interns)?;
                self.update(&data_bytes);
                Ok(Value::None)
            }
            StaticStrings::Copy => {
                args.check_zero_args("copy", heap)?;
                let copy_obj = Self {
                    algorithm: self.algorithm,
                    data: self.data.clone(),
                    digest_size: self.digest_size,
                };
                let id = heap.allocate(HeapData::Hash(copy_obj))?;
                Ok(Value::Ref(id))
            }
            _ => Err(ExcType::attribute_error(Type::Hash, attr.as_str(interns))),
        }
    }
}

/// Creates the `hashlib` module and allocates it on the heap.
///
/// The module provides:
/// - `md5(b)`: Create MD5 hash object from bytes `b`
/// - `new(name, b=b'')`: Create hash object based on algorithm name
/// - `sha1(b)`: Create SHA-1 hash object from bytes `b`
/// - `sha224(b)`: Create SHA-224 hash object from bytes `b`
/// - `sha256(b)`: Create SHA-256 hash object from bytes `b`
/// - `sha384(b)`: Create SHA-384 hash object from bytes `b`
/// - `sha512(b)`: Create SHA-512 hash object from bytes `b`
/// - `sha512_224(b)`: Create SHA-512/224 hash object from bytes `b`
/// - `sha512_256(b)`: Create SHA-512/256 hash object from bytes `b`
/// - `sha3_224(b)`: Create SHA3-224 hash object from bytes `b`
/// - `sha3_256(b)`: Create SHA3-256 hash object from bytes `b`
/// - `sha3_384(b)`: Create SHA3-384 hash object from bytes `b`
/// - `sha3_512(b)`: Create SHA3-512 hash object from bytes `b`
/// - `blake2b(b, digest_size=64)`: Create BLAKE2b hash object from bytes `b`
/// - `blake2s(b, digest_size=32)`: Create BLAKE2s hash object from bytes `b`
/// - `shake_128(b)`: Create SHAKE-128 hash object from bytes `b`
/// - `shake_256(b)`: Create SHAKE-256 hash object from bytes `b`
/// - `scrypt(...)`: Derive a key using scrypt
/// - `pbkdf2_hmac(name, password, salt, iterations, dklen=None)`: Derive a key via PBKDF2-HMAC
/// - `file_digest(fileobj, digest)`: Compute hash from file-like object
/// - `algorithms_available`: set of available algorithm names
/// - `algorithms_guaranteed`: set of guaranteed algorithm names
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
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Hashlib);

    // Hash functions
    let functions: &[(StaticStrings, HashlibFunctions)] = &[
        (StaticStrings::HlMd5, HashlibFunctions::Md5),
        (StaticStrings::HlNew, HashlibFunctions::New),
        (StaticStrings::HlSha1, HashlibFunctions::Sha1),
        (StaticStrings::HlSha224, HashlibFunctions::Sha224),
        (StaticStrings::HlSha256, HashlibFunctions::Sha256),
        (StaticStrings::HlSha384, HashlibFunctions::Sha384),
        (StaticStrings::HlSha512, HashlibFunctions::Sha512),
        (StaticStrings::HlSha512_224, HashlibFunctions::Sha512_224),
        (StaticStrings::HlSha512_256, HashlibFunctions::Sha512_256),
        (StaticStrings::HlSha3_224, HashlibFunctions::Sha3_224),
        (StaticStrings::HlSha3_256, HashlibFunctions::Sha3_256),
        (StaticStrings::HlSha3_384, HashlibFunctions::Sha3_384),
        (StaticStrings::HlSha3_512, HashlibFunctions::Sha3_512),
        (StaticStrings::HlBlake2b, HashlibFunctions::Blake2b),
        (StaticStrings::HlBlake2s, HashlibFunctions::Blake2s),
        (StaticStrings::HlShake128, HashlibFunctions::Shake128),
        (StaticStrings::HlShake256, HashlibFunctions::Shake256),
        (StaticStrings::HlScrypt, HashlibFunctions::Scrypt),
        (StaticStrings::HlPbkdf2Hmac, HashlibFunctions::Pbkdf2Hmac),
        (StaticStrings::HlFileDigest, HashlibFunctions::FileDigest),
    ];

    for &(name, func) in functions {
        module.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::Hashlib(func)),
            heap,
            interns,
        );
    }

    // algorithms_available - include the CPython set used by parity tests.
    let algos_available = [
        "md5",
        "md5-sha1",
        "ripemd160",
        "sha1",
        "sha224",
        "sha256",
        "sha384",
        "sha512",
        "sha512_224",
        "sha512_256",
        "sha3_224",
        "sha3_256",
        "sha3_384",
        "sha3_512",
        "blake2b",
        "blake2s",
        "shake_128",
        "shake_256",
        "sm3",
    ];
    let algos_set = create_algorithm_set(heap, interns, &algos_available)?;
    module.set_attr(
        StaticStrings::HlAlgorithmsAvailable,
        Value::Ref(algos_set),
        heap,
        interns,
    );

    // algorithms_guaranteed - the core algorithms guaranteed by CPython (14 algorithms)
    // CPython guarantees: blake2b, blake2s, md5, sha1, sha224, sha256, sha384,
    //                     sha3_224, sha3_256, sha3_384, sha3_512, sha512, shake_128, shake_256
    // Note: sha512_224 and sha512_256 are NOT in CPython's guaranteed set
    let algos_guaranteed = [
        "md5",
        "sha1",
        "sha224",
        "sha256",
        "sha384",
        "sha512",
        "sha3_224",
        "sha3_256",
        "sha3_384",
        "sha3_512",
        "blake2b",
        "blake2s",
        "shake_128",
        "shake_256",
    ];
    let algos_guaranteed_set = create_algorithm_set(heap, interns, &algos_guaranteed)?;
    module.set_attr(
        StaticStrings::HlAlgorithmsGuaranteed,
        Value::Ref(algos_guaranteed_set),
        heap,
        interns,
    );

    // __all__ — public API matching CPython's hashlib namespace.
    // Excludes sha512_224, sha512_256 (Ouros-only) and scrypt (not in CPython's __all__).
    let public_names = [
        "algorithms_available",
        "algorithms_guaranteed",
        "blake2b",
        "blake2s",
        "file_digest",
        "md5",
        "new",
        "pbkdf2_hmac",
        "sha1",
        "sha224",
        "sha256",
        "sha384",
        "sha3_224",
        "sha3_256",
        "sha3_384",
        "sha3_512",
        "sha512",
        "shake_128",
        "shake_256",
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

/// Creates a set containing the specified hash algorithm names.
///
/// Used for `algorithms_available` and `algorithms_guaranteed` with different
/// sets of algorithm names.
fn create_algorithm_set(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    algo_names: &[&str],
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut set = Set::with_capacity(algo_names.len());
    for name in algo_names {
        let value = if let Ok(static_name) = StaticStrings::from_str(name) {
            Value::InternString(static_name.into())
        } else {
            let str_obj = Str::from(*name);
            let str_id = heap.allocate(HeapData::Str(str_obj))?;
            Value::Ref(str_id)
        };
        // add() takes ownership; on duplicate it drops the value.
        set.add(value, heap, interns)
            .expect("string values are always hashable");
    }

    heap.allocate(HeapData::Set(set))
}

/// Dispatches a call to a hashlib module function.
///
/// Returns `AttrCallResult::Value` for all functions as they complete immediately.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: HashlibFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        HashlibFunctions::Md5 => hash_object_dispatch(heap, interns, args, "hashlib.md5", HashAlgorithm::Md5),
        HashlibFunctions::New => hashlib_new(heap, interns, args),
        HashlibFunctions::Sha1 => hash_object_dispatch(heap, interns, args, "hashlib.sha1", HashAlgorithm::Sha1),
        HashlibFunctions::Sha224 => hash_object_dispatch(heap, interns, args, "hashlib.sha224", HashAlgorithm::Sha224),
        HashlibFunctions::Sha256 => hash_object_dispatch(heap, interns, args, "hashlib.sha256", HashAlgorithm::Sha256),
        HashlibFunctions::Sha384 => hash_object_dispatch(heap, interns, args, "hashlib.sha384", HashAlgorithm::Sha384),
        HashlibFunctions::Sha512 => hash_object_dispatch(heap, interns, args, "hashlib.sha512", HashAlgorithm::Sha512),
        HashlibFunctions::Sha512_224 => {
            hash_object_dispatch(heap, interns, args, "hashlib.sha512_224", HashAlgorithm::Sha512_224)
        }
        HashlibFunctions::Sha512_256 => {
            hash_object_dispatch(heap, interns, args, "hashlib.sha512_256", HashAlgorithm::Sha512_256)
        }
        HashlibFunctions::Sha3_224 => {
            hash_object_dispatch(heap, interns, args, "hashlib.sha3_224", HashAlgorithm::Sha3_224)
        }
        HashlibFunctions::Sha3_256 => {
            hash_object_dispatch(heap, interns, args, "hashlib.sha3_256", HashAlgorithm::Sha3_256)
        }
        HashlibFunctions::Sha3_384 => {
            hash_object_dispatch(heap, interns, args, "hashlib.sha3_384", HashAlgorithm::Sha3_384)
        }
        HashlibFunctions::Sha3_512 => {
            hash_object_dispatch(heap, interns, args, "hashlib.sha3_512", HashAlgorithm::Sha3_512)
        }
        HashlibFunctions::Blake2b => {
            hash_object_dispatch(heap, interns, args, "hashlib.blake2b", HashAlgorithm::Blake2b)
        }
        HashlibFunctions::Blake2s => {
            hash_object_dispatch(heap, interns, args, "hashlib.blake2s", HashAlgorithm::Blake2s)
        }
        HashlibFunctions::Shake128 => {
            hash_object_dispatch(heap, interns, args, "hashlib.shake_128", HashAlgorithm::Shake128)
        }
        HashlibFunctions::Shake256 => {
            hash_object_dispatch(heap, interns, args, "hashlib.shake_256", HashAlgorithm::Shake256)
        }
        HashlibFunctions::Scrypt => scrypt(heap, interns, args),
        HashlibFunctions::Pbkdf2Hmac => pbkdf2_hmac(heap, interns, args),
        HashlibFunctions::FileDigest => file_digest(heap, interns, args),
    }
}

/// Implementation of `hashlib.new(name, data=b'')`.
///
/// Dispatches to a supported algorithm based on `name` and returns a hash object.
/// For blake2b/blake2s, also accepts `digest_size` keyword argument.
fn hashlib_new(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    // Extract name argument (required, first positional)
    let name_val = pos
        .next()
        .ok_or_else(|| ExcType::type_error("hashlib.new() argument 1 is required"))?;
    defer_drop!(name_val, heap);
    let name = extract_string_arg(name_val, heap, interns, "hashlib.new")?;

    // Extract optional data argument (second positional)
    let data_val = pos.next();
    let data = match data_val {
        None => None,
        Some(val) => {
            defer_drop!(val, heap);
            Some(extract_bytes(val, heap, interns)?)
        }
    };

    // Drop any remaining positional arguments
    pos.drop_with_heap(heap);

    let algo = if let Some(algo) = hash_algorithm_from_name(name.to_lowercase().as_str()) {
        algo
    } else {
        kwargs.drop_with_heap(heap);
        return Err(unsupported_hash_type_error(&name));
    };

    let digest_size = parse_hash_constructor_kwargs(&kwargs, algo, heap, interns, "hashlib.new")?;

    // Drop remaining kwargs
    kwargs.drop_with_heap(heap);

    let hash_obj = HashObject::new(algo, data.as_deref(), digest_size);
    let id = heap.allocate(HeapData::Hash(hash_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `hashlib.scrypt`.
///
/// Supports the CPython-style signature:
/// `scrypt(password, *, salt, n, r, p, maxmem=0, dklen=64)`.
fn scrypt(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    let pos_len = pos.len();
    if pos_len != 1 {
        kwargs.drop_with_heap(heap);
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("hashlib.scrypt", 1, pos_len));
    }

    let password_val = pos.next().expect("len checked above");
    defer_drop!(password_val, heap);
    let password = extract_bytes(password_val, heap, interns)?;

    let parsed_kwargs = parse_scrypt_kwargs(&kwargs, heap, interns)?;
    kwargs.drop_with_heap(heap);

    let ParsedScryptKwargs { salt, n, r, p, dklen } = parsed_kwargs;
    let salt = salt.ok_or_else(|| ExcType::type_error("hashlib.scrypt() missing required keyword argument: 'salt'"))?;
    let n = n.ok_or_else(|| ExcType::type_error("hashlib.scrypt() missing required keyword argument: 'n'"))?;
    let r = r.ok_or_else(|| ExcType::type_error("hashlib.scrypt() missing required keyword argument: 'r'"))?;
    let p = p.ok_or_else(|| ExcType::type_error("hashlib.scrypt() missing required keyword argument: 'p'"))?;
    let dklen = dklen.unwrap_or(64);

    if n < 2 || (n & (n - 1)) != 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be a power of 2 greater than 1").into());
    }
    let log_n = u8::try_from(n.ilog2())
        .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "n is too large for scrypt"))?;

    let params = ScryptParams::new(log_n, r, p, dklen)
        .map_err(|e| SimpleException::new_msg(ExcType::ValueError, format!("invalid scrypt parameters: {e}")))?;
    let mut output = vec![0_u8; dklen];
    scrypt_derive(&password, &salt, &params, &mut output)
        .map_err(|e| SimpleException::new_msg(ExcType::ValueError, format!("scrypt failed: {e}")))?;

    let bytes_obj = Bytes::new(output);
    let id = heap.allocate(HeapData::Bytes(bytes_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `hashlib.file_digest(fileobj, digest)`.
///
/// Reads all bytes from `fileobj.read()` and hashes them with `digest`.
fn file_digest(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("hashlib.file_digest"));
    }
    kwargs.drop_with_heap(heap);

    let pos_len = pos.len();
    if pos_len < 2 {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("hashlib.file_digest", 2, pos_len));
    }
    if pos_len > 2 {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("hashlib.file_digest", 2, pos_len));
    }

    let fileobj = pos.next().expect("len checked above");
    let digest_spec = pos.next().expect("len checked above");
    defer_drop!(fileobj, heap);
    defer_drop!(digest_spec, heap);

    let fileobj_id = if let Value::Ref(id) = fileobj {
        *id
    } else {
        return Err(ExcType::type_error(
            "hashlib.file_digest() fileobj must be a file-like object",
        ));
    };

    let read_result = heap.call_attr_raw(
        fileobj_id,
        &EitherStr::from("read".to_owned()),
        ArgValues::Empty,
        interns,
    )?;
    let read_value = match read_result {
        AttrCallResult::Value(value) => value,
        _ => {
            return Err(SimpleException::new_msg(
                ExcType::NotImplementedError,
                "hashlib.file_digest() only supports synchronous read() calls in Ouros",
            )
            .into());
        }
    };
    defer_drop!(read_value, heap);
    let file_bytes = extract_bytes(read_value, heap, interns)?;

    let digest_name = extract_string_arg(digest_spec, heap, interns, "hashlib.file_digest")?;
    let algorithm = hash_algorithm_from_name(digest_name.to_lowercase().as_str())
        .ok_or_else(|| unsupported_hash_type_error(&digest_name))?;

    let hash_obj = HashObject::new(algorithm, Some(file_bytes.as_slice()), None);
    let id = heap.allocate(HeapData::Hash(hash_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Generic hash dispatch that constructs a hash object for a fixed algorithm.
///
/// Accepts zero or one bytes argument and returns a heap-allocated `HashObject`.
/// For blake2b/blake2s, also accepts `digest_size` keyword argument.
fn hash_object_dispatch(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    func_name: &str,
    algorithm: HashAlgorithm,
) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    // Extract data argument (positional, zero or one)
    let data_val = pos.next();
    let data_bytes = match data_val {
        None => None,
        Some(val) => {
            defer_drop!(val, heap);
            Some(extract_bytes(val, heap, interns)?)
        }
    };

    // Drop any remaining positional arguments
    pos.drop_with_heap(heap);

    let digest_size = parse_hash_constructor_kwargs(&kwargs, algorithm, heap, interns, func_name)?;

    // Drop remaining kwargs
    kwargs.drop_with_heap(heap);

    let hash_obj = HashObject::new(algorithm, data_bytes.as_deref(), digest_size);
    let id = heap.allocate(HeapData::Hash(hash_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Implementation of `hashlib.pbkdf2_hmac`.
///
/// Derives a key from a password using PBKDF2-HMAC with the requested hash.
fn pbkdf2_hmac(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut pos, kwargs) = args.into_parts();

    let (dklen_kwarg_provided, dklen_kwarg) = parse_pbkdf2_kwargs(&kwargs, heap, interns)?;
    kwargs.drop_with_heap(heap);

    let pos_len = pos.len();
    if pos_len < 4 {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("hashlib.pbkdf2_hmac", 4, pos_len));
    }
    if pos_len > 5 {
        pos.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("hashlib.pbkdf2_hmac", 5, pos_len));
    }

    let hash_name_val = pos.next().expect("pos_len >= 4");
    let password_val = pos.next().expect("pos_len >= 4");
    let salt_val = pos.next().expect("pos_len >= 4");
    let iterations_val = pos.next().expect("pos_len >= 4");
    let dklen_positional = pos.next();

    defer_drop!(hash_name_val, heap);
    defer_drop!(password_val, heap);
    defer_drop!(salt_val, heap);
    defer_drop!(iterations_val, heap);

    let hash_name = extract_string_arg(hash_name_val, heap, interns, "hashlib.pbkdf2_hmac")?;
    let password = extract_bytes(password_val, heap, interns)?;
    let salt = extract_bytes(salt_val, heap, interns)?;

    let iterations_i64 = iterations_val.as_int(heap)?;
    if iterations_i64 <= 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "iteration value must be greater than 0.").into());
    }
    let iterations = u32::try_from(iterations_i64)
        .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "iteration value is too large"))?;

    if dklen_kwarg_provided && dklen_positional.is_some() {
        if let Some(value) = dklen_positional {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error(
            "hashlib.pbkdf2_hmac() got multiple values for argument 'dklen'",
        ));
    }

    let dklen = match dklen_positional {
        Some(value) => {
            defer_drop!(value, heap);
            parse_optional_length_kwarg(
                value,
                heap,
                "key length must be greater than 0.",
                "key length is too large",
            )?
        }
        None => dklen_kwarg,
    };

    let derived = match hash_name.to_lowercase().as_str() {
        "sha1" => pbkdf2_dispatch::<Hmac<Sha1>>(&password, &salt, iterations, dklen),
        "sha224" => pbkdf2_dispatch::<Hmac<Sha224>>(&password, &salt, iterations, dklen),
        "sha256" => pbkdf2_dispatch::<Hmac<Sha256>>(&password, &salt, iterations, dklen),
        "sha384" => pbkdf2_dispatch::<Hmac<Sha384>>(&password, &salt, iterations, dklen),
        "sha512" => pbkdf2_dispatch::<Hmac<Sha512>>(&password, &salt, iterations, dklen),
        _ => {
            return Err(unsupported_hash_type_error(&hash_name));
        }
    };

    let bytes_obj = Bytes::new(derived);
    let id = heap.allocate(HeapData::Bytes(bytes_obj))?;
    Ok(AttrCallResult::Value(Value::Ref(id)))
}

/// Performs PBKDF2-HMAC using a MAC type (e.g., `Hmac<Sha256>`).
///
/// Uses the concrete `M: Mac + KeyInit` type directly to avoid complex
/// trait bounds on the digest type parameter.
fn pbkdf2_dispatch<M>(password: &[u8], salt: &[u8], iterations: u32, dklen: Option<usize>) -> Vec<u8>
where
    M: Mac + KeyInit,
{
    let digest_len = <M as OutputSizeUser>::output_size();
    let dklen = dklen.unwrap_or(digest_len);

    let mut output = Vec::with_capacity(dklen);
    let mut block_num = 1u32;

    while output.len() < dklen {
        let block = pbkdf2_block::<M>(password, salt, iterations, block_num);
        let needed = cmp::min(dklen - output.len(), block.len());
        output.extend_from_slice(&block[..needed]);
        block_num = block_num.wrapping_add(1);
    }

    output
}

/// Computes a single PBKDF2-HMAC block (F function).
///
/// The `M` type parameter is a concrete MAC type (e.g., `Hmac<Sha256>`)
/// rather than the raw digest, keeping the trait bounds simple.
fn pbkdf2_block<M>(password: &[u8], salt: &[u8], iterations: u32, block_num: u32) -> Vec<u8>
where
    M: Mac + KeyInit,
{
    let mut mac = <M as KeyInit>::new_from_slice(password).expect("HMAC can take any key length");
    mac.update(salt);
    mac.update(&block_num.to_be_bytes());
    let mut u = mac.finalize().into_bytes().to_vec();
    let mut t = u.clone();

    for _ in 1..iterations {
        let mut mac = <M as KeyInit>::new_from_slice(password).expect("HMAC can take any key length");
        mac.update(&u);
        u = mac.finalize().into_bytes().to_vec();
        for (t_i, u_i) in t.iter_mut().zip(u.iter()) {
            *t_i ^= u_i;
        }
    }

    t
}

/// Builds a ValueError for unsupported hash algorithm names.
fn unsupported_hash_type_error(name: &str) -> RunError {
    SimpleException::new_msg(ExcType::ValueError, format!("unsupported hash type {name}")).into()
}

/// Extracts the digest length argument for `digest()` and `hexdigest()`.
fn extract_digest_length(
    algorithm: HashAlgorithm,
    method: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Option<usize>> {
    if algorithm.requires_output_len() {
        let value = args.get_one_arg(method, heap)?;
        defer_drop!(value, heap);
        let length_i64 = value.as_int(heap)?;
        if length_i64 < 0 {
            return Err(SimpleException::new_msg(ExcType::ValueError, "length must be non-negative").into());
        }
        let length = usize::try_from(length_i64)
            .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "length is too large"))?;
        Ok(Some(length))
    } else {
        args.check_zero_args(method, heap)?;
        Ok(None)
    }
}

/// Computes the digest for a fixed-length hash algorithm.
fn compute_fixed_digest<D: Digest + Default>(input: &[u8]) -> Vec<u8> {
    let mut hasher = D::new();
    hasher.update(input);
    hasher.finalize().to_vec()
}

/// Computes the digest for a SHAKE algorithm with an explicit output length.
fn compute_shake_digest<D: Default + ExtendableOutput + Update>(
    output_len: Option<usize>,
    input: &[u8],
) -> RunResult<Vec<u8>> {
    let length = output_len.ok_or_else(|| {
        SimpleException::new_msg(ExcType::TypeError, "digest length is required for SHAKE algorithms")
    })?;
    let mut hasher = D::default();
    hasher.update(input);
    let mut reader = hasher.finalize_xof();
    let mut output = vec![0u8; length];
    reader.read(&mut output);
    Ok(output)
}

/// Computes a BLAKE2b digest with a configurable output size.
///
/// This is a direct implementation of RFC 7693 for the unkeyed default mode.
/// The output size can be configured from 1 to 64 bytes (default 64).
fn compute_blake2b_digest(input: &[u8], out_len: usize) -> Vec<u8> {
    const BLOCK_LEN: usize = 128;
    const ROUNDS: usize = 12;
    const IV: [u64; 8] = [
        0x6A09_E667_F3BC_C908,
        0xBB67_AE85_84CA_A73B,
        0x3C6E_F372_FE94_F82B,
        0xA54F_F53A_5F1D_36F1,
        0x510E_527F_ADE6_82D1,
        0x9B05_688C_2B3E_6C1F,
        0x1F83_D9AB_FB41_BD6B,
        0x5BE0_CD19_137E_2179,
    ];
    const SIGMA: [[usize; 16]; 12] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
        [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
        [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
        [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
        [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
        [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
        [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
        [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
        [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    ];

    let mut h = IV;
    h[0] ^= 0x0101_0000 ^ (out_len as u64);

    let mut counter: u128 = 0;
    let mut offset = 0;
    while offset + BLOCK_LEN < input.len() {
        let mut block = [0_u8; BLOCK_LEN];
        block.copy_from_slice(&input[offset..offset + BLOCK_LEN]);
        counter = counter.wrapping_add(BLOCK_LEN as u128);
        blake2b_compress(&mut h, &block, counter, false, &IV, &SIGMA, ROUNDS);
        offset += BLOCK_LEN;
    }

    let mut final_block = [0_u8; BLOCK_LEN];
    let remainder = &input[offset..];
    final_block[..remainder.len()].copy_from_slice(remainder);
    counter = counter.wrapping_add(remainder.len() as u128);
    blake2b_compress(&mut h, &final_block, counter, true, &IV, &SIGMA, ROUNDS);

    let mut out = Vec::with_capacity(out_len);
    for word in &h {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.truncate(out_len);
    out
}

/// Compression function for BLAKE2b.
fn blake2b_compress(
    h: &mut [u64; 8],
    block: &[u8; 128],
    counter: u128,
    is_last: bool,
    iv: &[u64; 8],
    sigma: &[[usize; 16]; 12],
    rounds: usize,
) {
    let mut m = [0_u64; 16];
    for (i, word) in m.iter_mut().enumerate() {
        let start = i * 8;
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&block[start..start + 8]);
        *word = u64::from_le_bytes(bytes);
    }

    let mut v = [0_u64; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(iv);
    let counter_lo = u64::try_from(counter & u128::from(u64::MAX)).expect("counter low part always fits u64");
    let counter_hi = u64::try_from(counter >> 64).expect("counter high part always fits u64");
    v[12] ^= counter_lo;
    v[13] ^= counter_hi;
    if is_last {
        v[14] = !v[14];
    }

    for s in sigma.iter().take(rounds) {
        blake2b_mix(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        blake2b_mix(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        blake2b_mix(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        blake2b_mix(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        blake2b_mix(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        blake2b_mix(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        blake2b_mix(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        blake2b_mix(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// BLAKE2b G mixing function.
fn blake2b_mix(
    state: &mut [u64; 16],
    index_a: usize,
    index_b: usize,
    index_c: usize,
    index_d: usize,
    msg_x: u64,
    msg_y: u64,
) {
    state[index_a] = state[index_a].wrapping_add(state[index_b]).wrapping_add(msg_x);
    state[index_d] = (state[index_d] ^ state[index_a]).rotate_right(32);
    state[index_c] = state[index_c].wrapping_add(state[index_d]);
    state[index_b] = (state[index_b] ^ state[index_c]).rotate_right(24);
    state[index_a] = state[index_a].wrapping_add(state[index_b]).wrapping_add(msg_y);
    state[index_d] = (state[index_d] ^ state[index_a]).rotate_right(16);
    state[index_c] = state[index_c].wrapping_add(state[index_d]);
    state[index_b] = (state[index_b] ^ state[index_c]).rotate_right(63);
}

/// Computes a BLAKE2s digest with a configurable output size.
///
/// This is a direct implementation of RFC 7693 for the unkeyed default mode.
/// The output size can be configured from 1 to 32 bytes (default 32).
fn compute_blake2s_digest(input: &[u8], out_len: usize) -> Vec<u8> {
    const BLOCK_LEN: usize = 64;
    const ROUNDS: usize = 10;
    const IV: [u32; 8] = [
        0x6A09_E667,
        0xBB67_AE85,
        0x3C6E_F372,
        0xA54F_F53A,
        0x510E_527F,
        0x9B05_688C,
        0x1F83_D9AB,
        0x5BE0_CD19,
    ];
    const SIGMA: [[usize; 16]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
        [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
        [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
        [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
        [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
        [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
        [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
        [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
        [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    ];

    let mut h = IV;
    h[0] ^= 0x0101_0000 ^ (out_len as u32);

    let mut counter: u128 = 0;
    let mut offset = 0;
    while offset + BLOCK_LEN < input.len() {
        let mut block = [0_u8; BLOCK_LEN];
        block.copy_from_slice(&input[offset..offset + BLOCK_LEN]);
        counter = counter.wrapping_add(BLOCK_LEN as u128);
        blake2s_compress(&mut h, &block, counter, false, &IV, &SIGMA, ROUNDS);
        offset += BLOCK_LEN;
    }

    let mut final_block = [0_u8; BLOCK_LEN];
    let remainder = &input[offset..];
    final_block[..remainder.len()].copy_from_slice(remainder);
    counter = counter.wrapping_add(remainder.len() as u128);
    blake2s_compress(&mut h, &final_block, counter, true, &IV, &SIGMA, ROUNDS);

    let mut out = Vec::with_capacity(out_len);
    for word in &h {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.truncate(out_len);
    out
}

/// Compression function for BLAKE2s.
fn blake2s_compress(
    h: &mut [u32; 8],
    block: &[u8; 64],
    counter: u128,
    is_last: bool,
    iv: &[u32; 8],
    sigma: &[[usize; 16]; 10],
    rounds: usize,
) {
    let mut m = [0_u32; 16];
    for (i, word) in m.iter_mut().enumerate() {
        let start = i * 4;
        let mut bytes = [0_u8; 4];
        bytes.copy_from_slice(&block[start..start + 4]);
        *word = u32::from_le_bytes(bytes);
    }

    let mut v = [0_u32; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(iv);
    let counter_lo = u32::try_from(counter & u128::from(u32::MAX)).expect("counter low part always fits u32");
    let counter_hi = u32::try_from((counter >> 32) & u128::from(u32::MAX)).expect("counter high part always fits u32");
    v[12] ^= counter_lo;
    v[13] ^= counter_hi;
    if is_last {
        v[14] = !v[14];
    }

    for s in sigma.iter().take(rounds) {
        blake2s_mix(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        blake2s_mix(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        blake2s_mix(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        blake2s_mix(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        blake2s_mix(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        blake2s_mix(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        blake2s_mix(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        blake2s_mix(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// BLAKE2s G mixing function.
fn blake2s_mix(
    state: &mut [u32; 16],
    index_a: usize,
    index_b: usize,
    index_c: usize,
    index_d: usize,
    msg_x: u32,
    msg_y: u32,
) {
    state[index_a] = state[index_a].wrapping_add(state[index_b]).wrapping_add(msg_x);
    state[index_d] = (state[index_d] ^ state[index_a]).rotate_right(16);
    state[index_c] = state[index_c].wrapping_add(state[index_d]);
    state[index_b] = (state[index_b] ^ state[index_c]).rotate_right(12);
    state[index_a] = state[index_a].wrapping_add(state[index_b]).wrapping_add(msg_y);
    state[index_d] = (state[index_d] ^ state[index_a]).rotate_right(8);
    state[index_c] = state[index_c].wrapping_add(state[index_d]);
    state[index_b] = (state[index_b] ^ state[index_c]).rotate_right(7);
}

/// Extracts a string from a `Value` that should be a string object.
///
/// Handles interned strings (`Value::InternString`) and heap-allocated strings
/// (`Value::Ref` -> `HeapData::Str`). Returns a `TypeError` if the value
/// is not a string.
fn extract_string_arg(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<String> {
    match value {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "{func_name}() argument must be str, not {}",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "{func_name}() argument must be str, not {}",
            value.py_type(heap)
        ))),
    }
}

/// Extracts raw byte data from a `Value` that should be a bytes object.
///
/// Handles interned bytes (`Value::InternBytes`) and heap-allocated bytes
/// (`Value::Ref` -> `HeapData::Bytes`). Returns a `TypeError` if the value
/// is not a bytes-like object.
pub(crate) fn extract_bytes(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(b) => Ok(b.as_slice().to_vec()),
            _ => Err(ExcType::type_error("a bytes-like object is required")),
        },
        _ => Err(ExcType::type_error("a bytes-like object is required")),
    }
}

/// Converts a byte slice to a hexadecimal string.
///
/// Each byte is represented as two lowercase hex digits.
fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(hex, "{byte:02x}").expect("writing to String should never fail");
    }
    hex
}

/// Resolves a hash algorithm name used by `hashlib.new`/`file_digest`.
fn hash_algorithm_from_name(name: &str) -> Option<HashAlgorithm> {
    match name {
        "md5" => Some(HashAlgorithm::Md5),
        "md5-sha1" => Some(HashAlgorithm::Md5Sha1),
        "sha1" => Some(HashAlgorithm::Sha1),
        "sha224" => Some(HashAlgorithm::Sha224),
        "sha256" => Some(HashAlgorithm::Sha256),
        "sha384" => Some(HashAlgorithm::Sha384),
        "sha512" => Some(HashAlgorithm::Sha512),
        "sha512_224" => Some(HashAlgorithm::Sha512_224),
        "sha512_256" => Some(HashAlgorithm::Sha512_256),
        "sha3_224" => Some(HashAlgorithm::Sha3_224),
        "sha3_256" => Some(HashAlgorithm::Sha3_256),
        "sha3_384" => Some(HashAlgorithm::Sha3_384),
        "sha3_512" => Some(HashAlgorithm::Sha3_512),
        "blake2b" => Some(HashAlgorithm::Blake2b),
        "blake2s" => Some(HashAlgorithm::Blake2s),
        "shake_128" => Some(HashAlgorithm::Shake128),
        "shake_256" => Some(HashAlgorithm::Shake256),
        "ripemd160" => Some(HashAlgorithm::Ripemd160),
        "sm3" => Some(HashAlgorithm::Sm3),
        _ => None,
    }
}

/// Parses kwargs accepted by hash constructors (`md5`, `sha256`, `new`, ...).
///
/// Supported kwargs:
/// - `digest_size` for BLAKE2 variants
/// - `usedforsecurity` for all constructors (accepted and ignored)
fn parse_hash_constructor_kwargs(
    kwargs: &KwargsValues,
    algo: HashAlgorithm,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<Option<usize>> {
    let mut digest_size = None;

    match kwargs {
        KwargsValues::Empty => {}
        KwargsValues::Inline(kvs) => {
            for (key_id, value) in kvs {
                parse_hash_constructor_kwarg(
                    interns.get_str(*key_id),
                    value,
                    algo,
                    &mut digest_size,
                    heap,
                    interns,
                    func_name,
                )?;
            }
        }
        KwargsValues::Dict(dict) => {
            for (key, value) in dict {
                let key_name = kwarg_name_from_value(key, heap, interns)?;
                parse_hash_constructor_kwarg(&key_name, value, algo, &mut digest_size, heap, interns, func_name)?;
            }
        }
    }

    Ok(digest_size)
}

/// Parses a single hash-constructor keyword argument.
fn parse_hash_constructor_kwarg(
    key: &str,
    value: &Value,
    algo: HashAlgorithm,
    digest_size: &mut Option<usize>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    func_name: &str,
) -> RunResult<()> {
    match key {
        "usedforsecurity" => {
            let _ = value.py_bool(heap, interns);
            Ok(())
        }
        "digest_size" => {
            if !matches!(algo, HashAlgorithm::Blake2b | HashAlgorithm::Blake2s) {
                return Err(invalid_kwarg_error(func_name, key));
            }
            let size_i64 = value.as_int(heap)?;
            if size_i64 <= 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "digest_size must be positive").into());
            }
            let size = usize::try_from(size_i64)
                .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "digest_size is too large"))?;
            let max_size = if matches!(algo, HashAlgorithm::Blake2b) { 64 } else { 32 };
            if size > max_size {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    format!("digest_size must be at most {max_size}"),
                )
                .into());
            }
            *digest_size = Some(size);
            Ok(())
        }
        _ => Err(invalid_kwarg_error(func_name, key)),
    }
}

/// Parses kwargs for `hashlib.pbkdf2_hmac`.
fn parse_pbkdf2_kwargs(
    kwargs: &KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(bool, Option<usize>)> {
    let mut dklen_provided = false;
    let mut dklen = None;

    match kwargs {
        KwargsValues::Empty => {}
        KwargsValues::Inline(kvs) => {
            for (key_id, value) in kvs {
                let key = interns.get_str(*key_id);
                if key == "dklen" {
                    dklen_provided = true;
                    dklen = parse_optional_length_kwarg(
                        value,
                        heap,
                        "key length must be greater than 0.",
                        "key length is too large",
                    )?;
                } else {
                    return Err(invalid_kwarg_error("hashlib.pbkdf2_hmac", key));
                }
            }
        }
        KwargsValues::Dict(dict) => {
            for (key, value) in dict {
                let key_name = kwarg_name_from_value(key, heap, interns)?;
                if key_name == "dklen" {
                    dklen_provided = true;
                    dklen = parse_optional_length_kwarg(
                        value,
                        heap,
                        "key length must be greater than 0.",
                        "key length is too large",
                    )?;
                } else {
                    return Err(invalid_kwarg_error("hashlib.pbkdf2_hmac", key_name.as_str()));
                }
            }
        }
    }

    Ok((dklen_provided, dklen))
}

/// Parsed keyword arguments for `hashlib.scrypt`.
#[derive(Debug, Default)]
struct ParsedScryptKwargs {
    /// Salt bytes (required).
    salt: Option<Vec<u8>>,
    /// CPU/memory cost parameter (required).
    n: Option<u64>,
    /// Block size parameter (required).
    r: Option<u32>,
    /// Parallelization parameter (required).
    p: Option<u32>,
    /// Output length (optional, defaults to 64).
    dklen: Option<usize>,
}

/// Parses kwargs for `hashlib.scrypt`.
fn parse_scrypt_kwargs(
    kwargs: &KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<ParsedScryptKwargs> {
    let mut parsed = ParsedScryptKwargs::default();

    match kwargs {
        KwargsValues::Empty => {}
        KwargsValues::Inline(kvs) => {
            for (key_id, value) in kvs {
                parse_scrypt_kwarg(interns.get_str(*key_id), value, &mut parsed, heap, interns)?;
            }
        }
        KwargsValues::Dict(dict) => {
            for (key, value) in dict {
                let key_name = kwarg_name_from_value(key, heap, interns)?;
                parse_scrypt_kwarg(&key_name, value, &mut parsed, heap, interns)?;
            }
        }
    }

    Ok(parsed)
}

/// Parses a single keyword for `hashlib.scrypt`.
fn parse_scrypt_kwarg(
    key: &str,
    value: &Value,
    parsed: &mut ParsedScryptKwargs,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    match key {
        "salt" => {
            parsed.salt = Some(extract_bytes(value, heap, interns)?);
            Ok(())
        }
        "n" => {
            let n = value.as_int(heap)?;
            if n <= 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "n must be a positive integer").into());
            }
            parsed.n =
                Some(u64::try_from(n).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "n is too large"))?);
            Ok(())
        }
        "r" => {
            let r = value.as_int(heap)?;
            if r <= 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "r must be a positive integer").into());
            }
            parsed.r =
                Some(u32::try_from(r).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "r is too large"))?);
            Ok(())
        }
        "p" => {
            let p = value.as_int(heap)?;
            if p <= 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "p must be a positive integer").into());
            }
            parsed.p =
                Some(u32::try_from(p).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "p is too large"))?);
            Ok(())
        }
        "dklen" => {
            parsed.dklen =
                parse_optional_length_kwarg(value, heap, "dklen must be greater than 0.", "dklen is too large")?;
            Ok(())
        }
        "maxmem" => {
            let maxmem = value.as_int(heap)?;
            if maxmem < 0 {
                return Err(SimpleException::new_msg(ExcType::ValueError, "maxmem must be non-negative").into());
            }
            Ok(())
        }
        _ => Err(invalid_kwarg_error("hashlib.scrypt", key)),
    }
}

/// Parses an optional integer length keyword argument.
fn parse_optional_length_kwarg(
    value: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    non_positive_error: &str,
    overflow_error: &str,
) -> RunResult<Option<usize>> {
    if matches!(value, Value::None) {
        return Ok(None);
    }
    let length = value.as_int(heap)?;
    if length <= 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, non_positive_error).into());
    }
    Ok(Some(usize::try_from(length).map_err(|_| {
        SimpleException::new_msg(ExcType::OverflowError, overflow_error)
    })?))
}

/// Extracts a keyword argument name from a `Value`.
fn kwarg_name_from_value(key: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match key {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error("keywords must be strings")),
        },
        _ => Err(ExcType::type_error("keywords must be strings")),
    }
}

/// Builds a TypeError for unsupported keyword arguments.
fn invalid_kwarg_error(func_name: &str, key: &str) -> RunError {
    ExcType::type_error(format!("'{key}' is an invalid keyword argument for {func_name}()"))
}
