//! Implementation of the `random` module.
//!
//! Provides pseudo-random number generation compatible with CPython's random module.
//! Uses the Mersenne Twister algorithm (MT19937) to match CPython's implementation.
#![expect(clippy::cast_possible_truncation, reason = "random narrowing is bounds-checked")]
#![expect(clippy::cast_sign_loss, reason = "signed/unsigned reinterpretation is intentional")]
#![expect(clippy::cast_possible_wrap, reason = "wrapping preserves CPython parity")]
#![expect(clippy::many_single_char_names, reason = "translated algorithms use math notation")]
#![expect(clippy::type_complexity, reason = "tuple-heavy helper APIs mirror CPython")]
use std::{
    cell::RefCell,
    f64::consts::{PI, TAU},
};

use num_bigint::{BigInt, Sign};
use rand::RngCore;
use sha2::{Digest, Sha512};

use crate::{
    args::{ArgValues, KwargsValues},
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{
        AttrCallResult, Bytes, ClassObject, Dict, List, LongInt, OurosIter, PyTrait, Str, Type, allocate_tuple,
        compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// Mersenne Twister 19937 parameters
const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;
const BPF: i64 = 53;
const RECIP_BPF: f64 = 1.0 / 9_007_199_254_740_992.0;
const LOG4: f64 = 1.386_294_361_119_890_6;
const SG_MAGICCONST: f64 = 2.504_077_396_776_274;
const NV_MAGICCONST: f64 = 1.715_527_769_921_413_5;
const RANDOM_STATE_ATTR: &str = "_ouros_random_state";
const RANDOM_GAUSS_ATTR: &str = "_ouros_random_gauss";

/// Mersenne Twister 19937 random number generator.
#[derive(Debug, Clone)]
struct Mt19937 {
    state: [u32; N],
    index: usize,
}

impl Default for Mt19937 {
    fn default() -> Self {
        Self::new()
    }
}

impl Mt19937 {
    /// Creates a new MT19937 generator in the unseeded state.
    fn new() -> Self {
        Self {
            state: [0; N],
            index: N + 1,
        }
    }

    /// Seeds the generator from one 32-bit word.
    fn seed_from_u32(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..N {
            self.state[i] = 1_812_433_253u32
                .wrapping_mul(self.state[i - 1] ^ (self.state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    /// Seeds the generator from CPython-compatible key words.
    fn seed_from_array(&mut self, key: &[u32]) {
        self.seed_from_u32(19_650_218);
        let mut i = 1_usize;
        let mut j = 0_usize;
        let mut k = if N > key.len() { N } else { key.len() };

        while k > 0 {
            self.state[i] = (self.state[i] ^ ((self.state[i - 1] ^ (self.state[i - 1] >> 30)).wrapping_mul(1_664_525)))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.state[0] = self.state[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }

        k = N - 1;
        while k > 0 {
            self.state[i] = (self.state[i]
                ^ ((self.state[i - 1] ^ (self.state[i - 1] >> 30)).wrapping_mul(1_566_083_941)))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.state[0] = self.state[N - 1];
                i = 1;
            }
            k -= 1;
        }

        self.state[0] = 0x8000_0000;
        self.index = N;
    }

    /// Advances the state array by one twist operation.
    fn generate(&mut self) {
        for i in 0..N {
            let x = (self.state[i] & UPPER_MASK) | (self.state[(i + 1) % N] & LOWER_MASK);
            let mut x_a = x >> 1;
            if !x.is_multiple_of(2) {
                x_a ^= MATRIX_A;
            }
            self.state[i] = self.state[(i + M) % N] ^ x_a;
        }
        self.index = 0;
    }

    /// Returns the next tempered 32-bit output word.
    fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            if self.index > N {
                self.seed_from_u32(5489);
            }
            self.generate();
        }

        let mut y = self.state[self.index];
        self.index += 1;

        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;

        y
    }

    /// Returns the next 64 random bits.
    fn next_u64(&mut self) -> u64 {
        let high = u64::from(self.next_u32());
        let low = u64::from(self.next_u32());
        (high << 32) | low
    }

    /// Returns a float in `[0.0, 1.0)` matching CPython's algorithm.
    fn random(&mut self) -> f64 {
        let a = self.next_u32() >> 5;
        let b = self.next_u32() >> 6;
        (f64::from(a) * 67_108_864.0 + f64::from(b)) * RECIP_BPF
    }

    /// Fills `buf` with pseudo-random bytes.
    fn fill_bytes(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(4) {
            let val = self.next_u32();
            let bytes = val.to_le_bytes();
            for (i, byte) in chunk.iter_mut().enumerate() {
                *byte = bytes[i];
            }
        }
    }

    /// Returns up to 64 random bits as a `u64`.
    fn getrandbits_u64(&mut self, k: u32) -> u64 {
        if k == 0 {
            return 0;
        }
        let mut bits_remaining = k;
        let mut shift = 0u32;
        let mut value = 0u64;

        while bits_remaining > 0 {
            let take = bits_remaining.min(32);
            let mut word = self.next_u32();
            if take < 32 {
                word >>= 32 - take;
            }
            value |= u64::from(word) << shift;
            shift += 32;
            bits_remaining -= take;
        }

        value
    }

    /// Returns an unbiased integer in `[0, n)`.
    fn randbelow_u64(&mut self, n: u64) -> u64 {
        if n <= 1 {
            return 0;
        }
        let k = u64::BITS - n.leading_zeros();
        loop {
            let r = self.getrandbits_u64(k);
            if r < n {
                return r;
            }
        }
    }

    /// Returns `k` random bits as a non-negative `BigInt`.
    fn getrandbits(&mut self, k: u64) -> BigInt {
        if k == 0 {
            return BigInt::from(0);
        }

        if k <= 64 {
            return BigInt::from(self.getrandbits_u64(k as u32));
        }

        let mut bits_remaining = k;
        let mut shift = 0u64;
        let mut value = BigInt::from(0u8);

        while bits_remaining > 0 {
            let take = bits_remaining.min(32);
            let mut word = self.next_u32();
            if take < 32 {
                word >>= 32 - take;
            }
            value += BigInt::from(u64::from(word)) << shift;
            shift += 32;
            bits_remaining -= take;
        }

        value
    }
}

/// Generates a 128-bit entropy key for random seeding.
fn random_seed_entropy_key() -> [u32; 4] {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    [
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
    ]
}

thread_local! {
    static RNG: RefCell<Mt19937> = RefCell::new({
        let mut rng = Mt19937::new();
        let key = random_seed_entropy_key();
        rng.seed_from_array(&key);
        rng
    });
    static GAUSS_NEXT: RefCell<Option<f64>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum RandomFunctions {
    Random,
    Randint,
    Choice,
    Shuffle,
    Seed,
    Getstate,
    Setstate,
    Uniform,
    Randrange,
    Randbytes,
    Getrandbits,
    Triangular,
    Expovariate,
    Paretovariate,
    Weibullvariate,
    Binomialvariate,
    Gauss,
    Normalvariate,
    Lognormvariate,
    Gammavariate,
    Betavariate,
    Vonmisesvariate,
    Choices,
    Sample,
    RandomInit,
    RandomMethodSeed,
    RandomMethodRandom,
    RandomMethodGetstate,
    RandomMethodSetstate,
    RandomMethodRandint,
    RandomMethodRandrange,
    RandomMethodChoice,
    RandomMethodChoices,
    RandomMethodShuffle,
    RandomMethodSample,
    RandomMethodGauss,
    RandomMethodUniform,
    RandomMethodGetrandbits,
    RandomMethodRandbytes,
    SystemRandomInit,
    SystemRandomMethodRandom,
    SystemRandomMethodRandint,
    SystemRandomMethodRandrange,
    SystemRandomMethodChoice,
    SystemRandomMethodChoices,
    SystemRandomMethodShuffle,
    SystemRandomMethodSample,
    SystemRandomMethodUniform,
    SystemRandomMethodGetrandbits,
    SystemRandomMethodRandbytes,
}

/// Creates a helper class dictionary entry with a string key.
fn dict_set_str_attr(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), crate::resource::ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Initializes helper class MRO and registers it as a subclass of `object`.
fn initialize_helper_class_mro(
    class_id: HeapId,
    object_class: HeapId,
    class_uid: u64,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    let mro = compute_c3_mro(class_id, &[object_class], heap, interns)
        .expect("random helper class should always have a valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(class_obj) = heap.get_mut(class_id) {
        class_obj.set_mro(mro);
    }
    heap.with_entry_mut(object_class, |_, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("object class registry should be mutable");
}

/// Creates the runtime `random.Random` helper class.
fn create_random_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "seed",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodSeed)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "random",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodRandom)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "getstate",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodGetstate)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "setstate",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodSetstate)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "randint",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodRandint)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "randrange",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodRandrange)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "choice",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodChoice)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "choices",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodChoices)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "shuffle",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodShuffle)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "sample",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodSample)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "gauss",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodGauss)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "uniform",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodUniform)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "getrandbits",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodGetrandbits)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "randbytes",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::RandomMethodRandbytes)),
        heap,
        interns,
    )?;

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap("random.Random".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        attrs,
        vec![object_class],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;
    initialize_helper_class_mro(class_id, object_class, class_uid, heap, interns);
    Ok(class_id)
}

/// Creates the runtime `random.SystemRandom` helper class.
fn create_system_random_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let object_class = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_class);

    let mut attrs = Dict::new();
    dict_set_str_attr(
        &mut attrs,
        "__init__",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomInit)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "random",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodRandom)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "randint",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodRandint)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "randrange",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodRandrange)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "choice",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodChoice)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "choices",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodChoices)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "shuffle",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodShuffle)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "sample",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodSample)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "uniform",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodUniform)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "getrandbits",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodGetrandbits)),
        heap,
        interns,
    )?;
    dict_set_str_attr(
        &mut attrs,
        "randbytes",
        Value::ModuleFunction(ModuleFunctions::Random(RandomFunctions::SystemRandomMethodRandbytes)),
        heap,
        interns,
    )?;

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap("random.SystemRandom".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        attrs,
        vec![object_class],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;
    initialize_helper_class_mro(class_id, object_class, class_uid, heap, interns);
    Ok(class_id)
}

pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Random);

    let functions = [
        (StaticStrings::Random, RandomFunctions::Random),
        (StaticStrings::RdRandint, RandomFunctions::Randint),
        (StaticStrings::RdChoice, RandomFunctions::Choice),
        (StaticStrings::RdShuffle, RandomFunctions::Shuffle),
        (StaticStrings::RdSeed, RandomFunctions::Seed),
        (StaticStrings::RdGetstate, RandomFunctions::Getstate),
        (StaticStrings::RdSetstate, RandomFunctions::Setstate),
        (StaticStrings::RdUniform, RandomFunctions::Uniform),
        (StaticStrings::RdRandrange, RandomFunctions::Randrange),
        (StaticStrings::RdRandbytes, RandomFunctions::Randbytes),
        (StaticStrings::RdGetrandbits, RandomFunctions::Getrandbits),
        (StaticStrings::RdTriangular, RandomFunctions::Triangular),
        (StaticStrings::RdExpovariate, RandomFunctions::Expovariate),
        (StaticStrings::RdParetovariate, RandomFunctions::Paretovariate),
        (StaticStrings::RdWeibullvariate, RandomFunctions::Weibullvariate),
        (StaticStrings::RdBinomialvariate, RandomFunctions::Binomialvariate),
        (StaticStrings::RdGauss, RandomFunctions::Gauss),
        (StaticStrings::RdNormalvariate, RandomFunctions::Normalvariate),
        (StaticStrings::RdLognormvariate, RandomFunctions::Lognormvariate),
        (StaticStrings::RdGammavariate, RandomFunctions::Gammavariate),
        (StaticStrings::RdBetavariate, RandomFunctions::Betavariate),
        (StaticStrings::RdVonmisesvariate, RandomFunctions::Vonmisesvariate),
        (StaticStrings::RdChoices, RandomFunctions::Choices),
        (StaticStrings::RdSample, RandomFunctions::Sample),
    ];

    for (name, func) in functions {
        module.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::Random(func)),
            heap,
            interns,
        );
    }

    module.set_attr_str("BPF", Value::Int(BPF), heap, interns)?;
    module.set_attr_str("LOG4", Value::Float(LOG4), heap, interns)?;
    module.set_attr_str("NV_MAGICCONST", Value::Float(NV_MAGICCONST), heap, interns)?;
    module.set_attr_str("RECIP_BPF", Value::Float(RECIP_BPF), heap, interns)?;
    module.set_attr_str("SG_MAGICCONST", Value::Float(SG_MAGICCONST), heap, interns)?;
    module.set_attr_str("TWOPI", Value::Float(TAU), heap, interns)?;

    let random_class_id = create_random_class(heap, interns)?;
    module.set_attr_str("Random", Value::Ref(random_class_id), heap, interns)?;

    let system_random_class_id = create_system_random_class(heap, interns)?;
    module.set_attr_str("SystemRandom", Value::Ref(system_random_class_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: RandomFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    match function {
        RandomFunctions::Random => random(heap, args),
        RandomFunctions::Randint => randint(heap, args),
        RandomFunctions::Choice => choice(heap, interns, args),
        RandomFunctions::Shuffle => shuffle(heap, interns, args),
        RandomFunctions::Seed => seed(heap, interns, args),
        RandomFunctions::Getstate => getstate(heap, args),
        RandomFunctions::Setstate => setstate(heap, args),
        RandomFunctions::Uniform => uniform(heap, args),
        RandomFunctions::Randrange => randrange(heap, args),
        RandomFunctions::Randbytes => randbytes(heap, args),
        RandomFunctions::Getrandbits => getrandbits(heap, args),
        RandomFunctions::Triangular => triangular(heap, args),
        RandomFunctions::Expovariate => expovariate(heap, args),
        RandomFunctions::Paretovariate => paretovariate(heap, args),
        RandomFunctions::Weibullvariate => weibullvariate(heap, args),
        RandomFunctions::Binomialvariate => binomialvariate(heap, args),
        RandomFunctions::Gauss => gauss(heap, args),
        RandomFunctions::Normalvariate => normalvariate(heap, args),
        RandomFunctions::Lognormvariate => lognormvariate(heap, args),
        RandomFunctions::Gammavariate => gammavariate(heap, args),
        RandomFunctions::Betavariate => betavariate(heap, args),
        RandomFunctions::Vonmisesvariate => vonmisesvariate(heap, args),
        RandomFunctions::Choices => choices(heap, interns, args),
        RandomFunctions::Sample => sample(heap, interns, args),
        RandomFunctions::RandomInit => random_init_method(heap, interns, args),
        RandomFunctions::RandomMethodSeed => random_method_seed(heap, interns, args),
        RandomFunctions::RandomMethodRandom => random_method_random(heap, interns, args),
        RandomFunctions::RandomMethodGetstate => random_method_getstate(heap, interns, args),
        RandomFunctions::RandomMethodSetstate => random_method_setstate(heap, interns, args),
        RandomFunctions::RandomMethodRandint => random_method_randint(heap, interns, args),
        RandomFunctions::RandomMethodRandrange => random_method_randrange(heap, interns, args),
        RandomFunctions::RandomMethodChoice => random_method_choice(heap, interns, args),
        RandomFunctions::RandomMethodChoices => random_method_choices(heap, interns, args),
        RandomFunctions::RandomMethodShuffle => random_method_shuffle(heap, interns, args),
        RandomFunctions::RandomMethodSample => random_method_sample(heap, interns, args),
        RandomFunctions::RandomMethodGauss => random_method_gauss(heap, interns, args),
        RandomFunctions::RandomMethodUniform => random_method_uniform(heap, interns, args),
        RandomFunctions::RandomMethodGetrandbits => random_method_getrandbits(heap, interns, args),
        RandomFunctions::RandomMethodRandbytes => random_method_randbytes(heap, interns, args),
        RandomFunctions::SystemRandomInit => system_random_init_method(heap, interns, args),
        RandomFunctions::SystemRandomMethodRandom => system_random_method_random(heap, args),
        RandomFunctions::SystemRandomMethodRandint => system_random_method_randint(heap, args),
        RandomFunctions::SystemRandomMethodRandrange => system_random_method_randrange(heap, args),
        RandomFunctions::SystemRandomMethodChoice => system_random_method_choice(heap, interns, args),
        RandomFunctions::SystemRandomMethodChoices => system_random_method_choices(heap, interns, args),
        RandomFunctions::SystemRandomMethodShuffle => system_random_method_shuffle(heap, interns, args),
        RandomFunctions::SystemRandomMethodSample => system_random_method_sample(heap, interns, args),
        RandomFunctions::SystemRandomMethodUniform => system_random_method_uniform(heap, args),
        RandomFunctions::SystemRandomMethodGetrandbits => system_random_method_getrandbits(heap, args),
        RandomFunctions::SystemRandomMethodRandbytes => system_random_method_randbytes(heap, args),
    }
}

/// Rebuilds an `ArgValues` from collected positional/keyword parts.
fn arg_values_from_parts(positional: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    if kwargs.is_empty() {
        match positional.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(positional.into_iter().next().expect("length checked")),
            2 => {
                let mut iter = positional.into_iter();
                ArgValues::Two(
                    iter.next().expect("length checked"),
                    iter.next().expect("length checked"),
                )
            }
            _ => ArgValues::ArgsKargs {
                args: positional,
                kwargs,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        }
    }
}

/// Extracts `self` from an instance method call and returns remaining args.
fn extract_instance_self_and_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    method_name: &str,
) -> RunResult<(HeapId, ArgValues)> {
    let (positional_iter, kwargs) = args.into_parts();
    let mut positional: Vec<Value> = positional_iter.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(method_name, 1, 0));
    }
    let self_value = positional.remove(0);
    let self_id = match &self_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => *id,
        _ => {
            self_value.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(ExcType::type_error(format!("{method_name} expected instance")));
        }
    };
    self_value.drop_with_heap(heap);
    Ok((self_id, arg_values_from_parts(positional, kwargs)))
}

/// Sets an instance attribute by string key and drops replaced values.
fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    heap.with_entry_mut(instance_id, |heap_inner, data| -> RunResult<()> {
        let HeapData::Instance(instance) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("random helper expected instance"));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

/// Fetches an instance attribute by string key, cloning the stored value.
fn get_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };
    instance
        .attrs(heap)
        .and_then(|attrs| attrs.get_by_str(name, heap, interns))
        .map(|value| value.clone_with_heap(heap))
}

/// Serializes one MT state into the `bytes` payload used by `getstate`.
fn encode_mt_state(rng: &Mt19937) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + N * 4);
    bytes.extend_from_slice(&(rng.index as u32).to_le_bytes());
    for &word in &rng.state {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

/// Parses serialized MT state bytes as used by `setstate`.
fn decode_mt_state(state_bytes: &[u8]) -> RunResult<Mt19937> {
    if state_bytes.len() < 4 + N * 4 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "state vector is invalid").into());
    }

    let index = u32::from_le_bytes([state_bytes[0], state_bytes[1], state_bytes[2], state_bytes[3]]) as usize;
    if index > N + 1 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "state vector is invalid").into());
    }

    let mut state = [0u32; N];
    for (i, word) in state.iter_mut().enumerate() {
        let offset = 4 + i * 4;
        *word = u32::from_le_bytes([
            state_bytes[offset],
            state_bytes[offset + 1],
            state_bytes[offset + 2],
            state_bytes[offset + 3],
        ]);
    }

    Ok(Mt19937 { state, index })
}

/// Applies CPython-compatible seed coercion to a specific MT generator.
fn seed_mt_with_value(
    rng: &mut Mt19937,
    arg: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    match arg {
        Value::None => {
            rng.seed_from_array(&random_seed_entropy_key());
        }
        Value::Bool(flag) => {
            let key = bigint_to_key(BigInt::from(u8::from(*flag)));
            rng.seed_from_array(&key);
        }
        Value::Int(n) => {
            let key = bigint_to_key(BigInt::from(*n));
            rng.seed_from_array(&key);
        }
        Value::Float(f) => {
            let hash = py_hash_double(*f);
            let unsigned_hash = u64::from_ne_bytes(hash.to_ne_bytes());
            let key = bigint_to_key(BigInt::from(unsigned_hash));
            rng.seed_from_array(&key);
        }
        Value::InternBytes(bytes_id) => {
            let data = interns.get_bytes(*bytes_id);
            let key = hash_seed_bytes_to_key(data);
            rng.seed_from_array(&key);
        }
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => {
                let key = hash_seed_bytes_to_key(bytes.as_slice());
                rng.seed_from_array(&key);
            }
            HeapData::Bytearray(bytes) => {
                let key = hash_seed_bytes_to_key(bytes.as_slice());
                rng.seed_from_array(&key);
            }
            HeapData::Str(s) => {
                let key = hash_seed_bytes_to_key(s.as_bytes());
                rng.seed_from_array(&key);
            }
            HeapData::LongInt(li) => {
                let key = bigint_to_key(li.inner().clone());
                rng.seed_from_array(&key);
            }
            _ => {
                return Err(SimpleException::new_msg(
                    ExcType::TypeError,
                    "The only supported seed types are: None,\nint, float, str, bytes, and bytearray.",
                )
                .into());
            }
        },
        Value::InternString(sid) => {
            let s = interns.get_str(*sid);
            let key = hash_seed_bytes_to_key(s.as_bytes());
            rng.seed_from_array(&key);
        }
        _ => {
            return Err(SimpleException::new_msg(
                ExcType::TypeError,
                "The only supported seed types are: None,\nint, float, str, bytes, and bytearray.",
            )
            .into());
        }
    }
    Ok(())
}

/// Loads per-instance MT and gauss cache state from hidden attributes.
fn load_random_instance_state(
    instance_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Mt19937, Option<f64>)> {
    let mut rng = if let Some(state_value) = get_instance_attr_by_name(instance_id, RANDOM_STATE_ATTR, heap, interns) {
        defer_drop!(state_value, heap);
        match state_value {
            Value::Ref(bytes_id) => match heap.get(*bytes_id) {
                HeapData::Bytes(bytes) => decode_mt_state(bytes.as_slice())?,
                _ => return Err(ExcType::type_error("random helper state is corrupted")),
            },
            _ => return Err(ExcType::type_error("random helper state is corrupted")),
        }
    } else {
        let mut rng = Mt19937::new();
        rng.seed_from_array(&random_seed_entropy_key());
        rng
    };

    let gauss_next = if let Some(gauss_value) = get_instance_attr_by_name(instance_id, RANDOM_GAUSS_ATTR, heap, interns)
    {
        defer_drop!(gauss_value, heap);
        match gauss_value {
            Value::None => None,
            Value::Float(value) => Some(*value),
            Value::Int(value) => Some(*value as f64),
            _ => return Err(ExcType::type_error("gauss state must be None or a float")),
        }
    } else {
        None
    };

    if rng.index > N + 1 {
        rng.index = N;
    }
    Ok((rng, gauss_next))
}

/// Stores per-instance MT and gauss cache state into hidden attributes.
fn store_random_instance_state(
    instance_id: HeapId,
    rng: &Mt19937,
    gauss_next: Option<f64>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let state_bytes = encode_mt_state(rng);
    let state_id = heap.allocate(HeapData::Bytes(Bytes::new(state_bytes)))?;
    set_instance_attr_by_name(instance_id, RANDOM_STATE_ATTR, Value::Ref(state_id), heap, interns)?;
    set_instance_attr_by_name(
        instance_id,
        RANDOM_GAUSS_ATTR,
        gauss_next.map_or(Value::None, Value::Float),
        heap,
        interns,
    )?;
    Ok(())
}

/// Swaps module-global RNG state with a per-instance state tuple.
///
/// The swap is done without keeping any outstanding `RefMut` guards so callers can
/// freely invoke module helpers that borrow `RNG` / `GAUSS_NEXT` again.
fn swap_rng_state_with_globals(rng: &mut Mt19937, gauss_next: &mut Option<f64>) {
    RNG.with(|global_rng| {
        let mut global_rng = global_rng.borrow_mut();
        std::mem::swap(&mut *global_rng, rng);
    });
    GAUSS_NEXT.with(|global_gauss| {
        let mut global_gauss = global_gauss.borrow_mut();
        std::mem::swap(&mut *global_gauss, gauss_next);
    });
}

/// Temporarily swaps the module RNG globals with per-instance state.
fn with_temporary_rng_state<T>(rng: &mut Mt19937, gauss_next: &mut Option<f64>, f: impl FnOnce() -> T) -> T {
    swap_rng_state_with_globals(rng, gauss_next);
    let out = f();
    swap_rng_state_with_globals(rng, gauss_next);
    out
}

/// Runs one Random instance method while persisting per-instance RNG state.
fn call_with_random_instance_rng<T: ResourceTracker>(
    instance_id: HeapId,
    heap: &mut Heap<T>,
    interns: &Interns,
    call: impl FnOnce(&mut Heap<T>, &Interns) -> RunResult<AttrCallResult>,
) -> RunResult<AttrCallResult> {
    let (mut rng, mut gauss_next) = load_random_instance_state(instance_id, heap, interns)?;
    let result = with_temporary_rng_state(&mut rng, &mut gauss_next, || call(heap, interns));
    store_random_instance_state(instance_id, &rng, gauss_next, heap, interns)?;
    result
}

/// Implements `Random.__init__(self, x=None)`.
fn random_init_method(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.__init__")?;
    let (mut positional, kwargs) = method_args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("Random.__init__ takes no keyword arguments"));
    }
    let seed_value = positional.next().unwrap_or(Value::None);
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        seed_value.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("Random.__init__", 2, 3));
    }
    positional.drop_with_heap(heap);
    defer_drop!(seed_value, heap);

    let mut rng = Mt19937::new();
    seed_mt_with_value(&mut rng, seed_value, heap, interns)?;
    store_random_instance_state(instance_id, &rng, None, heap, interns)?;
    Ok(AttrCallResult::Value(Value::None))
}

/// Implements `Random.seed(self, a=None)`.
fn random_method_seed(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.seed")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, interns| {
        seed(heap, interns, method_args)
    })
}

/// Implements `Random.random(self)`.
fn random_method_random(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.random")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| random(heap, method_args))
}

/// Implements `Random.getstate(self)`.
fn random_method_getstate(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.getstate")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| getstate(heap, method_args))
}

/// Implements `Random.setstate(self, state)`.
fn random_method_setstate(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.setstate")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| setstate(heap, method_args))
}

/// Implements `Random.randint(self, a, b)`.
fn random_method_randint(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.randint")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| randint(heap, method_args))
}

/// Implements `Random.randrange(self, ...)`.
fn random_method_randrange(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.randrange")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| randrange(heap, method_args))
}

/// Implements `Random.choice(self, seq)`.
fn random_method_choice(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.choice")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, interns| {
        choice(heap, interns, method_args)
    })
}

/// Implements `Random.choices(self, population, ...)`.
fn random_method_choices(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.choices")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, interns| {
        choices(heap, interns, method_args)
    })
}

/// Implements `Random.shuffle(self, x)`.
fn random_method_shuffle(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.shuffle")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, interns| {
        shuffle(heap, interns, method_args)
    })
}

/// Implements `Random.sample(self, population, k, ...)`.
fn random_method_sample(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.sample")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, interns| {
        sample(heap, interns, method_args)
    })
}

/// Implements `Random.gauss(self, mu, sigma)`.
fn random_method_gauss(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.gauss")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| gauss(heap, method_args))
}

/// Implements `Random.uniform(self, a, b)`.
fn random_method_uniform(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.uniform")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| uniform(heap, method_args))
}

/// Implements `Random.getrandbits(self, k)`.
fn random_method_getrandbits(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.getrandbits")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| {
        getrandbits(heap, method_args)
    })
}

/// Implements `Random.randbytes(self, n)`.
fn random_method_randbytes(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (instance_id, method_args) = extract_instance_self_and_args(args, heap, "Random.randbytes")?;
    call_with_random_instance_rng(instance_id, heap, interns, move |heap, _| randbytes(heap, method_args))
}

/// Implements `SystemRandom.__init__(self, *args, **kwargs)`.
fn system_random_init_method(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.__init__")?;
    method_args.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

/// Returns a cryptographically strong random float in `[0.0, 1.0)`.
fn os_random_f64() -> f64 {
    let mut rng = rand::rngs::OsRng;
    let a = rng.next_u32() >> 5;
    let b = rng.next_u32() >> 6;
    (f64::from(a) * 67_108_864.0 + f64::from(b)) * RECIP_BPF
}

/// Returns up to 64 random bits from OS entropy.
fn os_getrandbits_u64(k: u32) -> u64 {
    if k == 0 {
        return 0;
    }
    let mut rng = rand::rngs::OsRng;
    let mut bits_remaining = k;
    let mut shift = 0u32;
    let mut value = 0u64;
    while bits_remaining > 0 {
        let take = bits_remaining.min(32);
        let mut word = rng.next_u32();
        if take < 32 {
            word >>= 32 - take;
        }
        value |= u64::from(word) << shift;
        shift += 32;
        bits_remaining -= take;
    }
    value
}

/// Returns `k` random bits from OS entropy as a non-negative `BigInt`.
fn os_getrandbits(k: u64) -> BigInt {
    if k == 0 {
        return BigInt::from(0);
    }
    if k <= 64 {
        return BigInt::from(os_getrandbits_u64(k as u32));
    }
    let mut bits_remaining = k;
    let mut shift = 0u64;
    let mut value = BigInt::from(0u8);
    let mut rng = rand::rngs::OsRng;
    while bits_remaining > 0 {
        let take = bits_remaining.min(32);
        let mut word = rng.next_u32();
        if take < 32 {
            word >>= 32 - take;
        }
        value += BigInt::from(u64::from(word)) << shift;
        shift += 32;
        bits_remaining -= take;
    }
    value
}

/// Returns an unbiased integer in `[0, n)` using OS entropy.
fn os_randbelow_u64(n: u64) -> u64 {
    if n <= 1 {
        return 0;
    }
    let k = u64::BITS - n.leading_zeros();
    loop {
        let r = os_getrandbits_u64(k);
        if r < n {
            return r;
        }
    }
}

/// Fills a byte slice with OS entropy.
fn os_fill_bytes(buf: &mut [u8]) {
    rand::rngs::OsRng.fill_bytes(buf);
}

/// Implements `SystemRandom.random(self)`.
fn system_random_method_random(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.random")?;
    method_args.check_zero_args("SystemRandom.random", heap)?;
    Ok(AttrCallResult::Value(Value::Float(os_random_f64())))
}

/// Implements `SystemRandom.randint(self, a, b)`.
fn system_random_method_randint(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.randint")?;
    let (a, b) = method_args.get_two_args("SystemRandom.randint", heap)?;
    let a_i64 = a.as_int(heap)?;
    let b_i64 = b.as_int(heap)?;
    a.drop_with_heap(heap);
    b.drop_with_heap(heap);
    if b_i64 < a_i64 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "empty range for randrange()").into());
    }
    let range_size = (i128::from(b_i64) - i128::from(a_i64) + 1) as u64;
    let offset = os_randbelow_u64(range_size);
    let value = a_i64 + i64::try_from(offset).expect("randint offset fits in i64");
    Ok(AttrCallResult::Value(Value::Int(value)))
}

/// Implements `SystemRandom.randrange(self, ...)`.
fn system_random_method_randrange(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.randrange")?;
    let (start_val, stop_val, step_val) = extract_randrange_args(method_args, heap)?;
    defer_drop!(start_val, heap);
    defer_drop!(stop_val, heap);
    defer_drop!(step_val, heap);

    let start = start_val.as_int(heap)?;
    let stop = stop_val.as_int(heap)?;
    let step = step_val.as_int(heap)?;
    if step == 0 {
        return Err(ExcType::value_error_range_step_zero());
    }
    let width = i128::from(stop) - i128::from(start);
    let step_i128 = i128::from(step);
    if (step_i128 > 0 && width <= 0) || (step_i128 < 0 && width >= 0) {
        return Err(SimpleException::new_msg(ExcType::ValueError, "empty range for randrange()").into());
    }
    let n = if step_i128 > 0 {
        (width + step_i128 - 1) / step_i128
    } else {
        (width + step_i128 + 1) / step_i128
    };
    if n <= 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "empty range for randrange()").into());
    }
    let index = os_randbelow_u64(n as u64);
    let value_i128 = i128::from(start) + step_i128 * i128::from(index);
    let value = i64::try_from(value_i128).expect("randrange result fits in i64");
    Ok(AttrCallResult::Value(Value::Int(value)))
}

/// Implements `SystemRandom.choice(self, seq)`.
fn system_random_method_choice(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.choice")?;
    choice_with_randbelow(heap, interns, method_args, os_randbelow_u64)
}

/// Implements `SystemRandom.choices(self, population, ...)`.
fn system_random_method_choices(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.choices")?;
    let (population_val, weights_val, cum_weights_val, k_val) = extract_choices_args(method_args, heap, interns)?;
    defer_drop!(population_val, heap);
    let k = match k_val {
        Some(k_val) => {
            defer_drop!(k_val, heap);
            k_val.as_int(heap)?
        }
        None => 1,
    };
    if k < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "k must be non-negative").into());
    }
    let k_usize = usize::try_from(k).expect("k validated non-negative");

    let population = sample_population_items(population_val, heap, interns)?;
    let len = population.len();
    if len == 0 {
        population.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::IndexError, "Cannot choose from an empty sequence").into());
    }

    let cumulative = if let Some(weights_val) = weights_val {
        defer_drop!(weights_val, heap);
        let weights = extract_choices_weights(weights_val, len, heap, interns)?;
        let mut cumulative = Vec::with_capacity(len);
        let mut total = 0.0;
        for w in weights {
            total += w;
            cumulative.push(total);
        }
        Some(cumulative)
    } else if let Some(cum_weights_val) = cum_weights_val {
        defer_drop!(cum_weights_val, heap);
        Some(extract_choices_weights(cum_weights_val, len, heap, interns)?)
    } else {
        None
    };

    let mut results = Vec::with_capacity(k_usize);
    if let Some(cumulative) = cumulative {
        let total = *cumulative.last().ok_or_else(|| {
            SimpleException::new_msg(ExcType::ValueError, "Total of weights must be greater than zero")
        })?;
        if !total.is_finite() {
            population.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::ValueError, "Total of weights must be finite").into());
        }
        if total <= 0.0 {
            population.drop_with_heap(heap);
            return Err(
                SimpleException::new_msg(ExcType::ValueError, "Total of weights must be greater than zero").into(),
            );
        }
        for _ in 0..k_usize {
            let u = os_random_f64() * total;
            let mut lo = 0usize;
            let mut hi = cumulative.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if u < cumulative[mid] {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            let idx = lo.min(len - 1);
            results.push(population[idx].clone_with_heap(heap));
        }
    } else {
        for _ in 0..k_usize {
            let idx = (os_random_f64() * len as f64) as usize;
            results.push(population[idx].clone_with_heap(heap));
        }
    }
    population.drop_with_heap(heap);

    let list = List::new(results);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Implements `SystemRandom.shuffle(self, x)`.
fn system_random_method_shuffle(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.shuffle")?;
    shuffle_with_randbelow(heap, interns, method_args, os_randbelow_u64)
}

/// Implements `SystemRandom.sample(self, population, k, ...)`.
fn system_random_method_sample(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.sample")?;
    let (mut positional, kwargs) = method_args.into_parts();
    let Some(population_val) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("random.sample", 2, 0));
    };
    let positional_k = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        population_val.drop_with_heap(heap);
        positional_k.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("random.sample", 2, 3));
    }
    positional.drop_with_heap(heap);
    defer_drop!(population_val, heap);

    let mut k_val = positional_k;
    let mut counts_val: Option<Value> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            counts_val.drop_with_heap(heap);
            k_val.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "counts" => {
                if let Some(old) = counts_val.replace(value) {
                    old.drop_with_heap(heap);
                    counts_val.drop_with_heap(heap);
                    k_val.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "random.sample() got multiple values for argument 'counts'",
                    ));
                }
            }
            "k" => {
                if k_val.is_some() {
                    value.drop_with_heap(heap);
                    counts_val.drop_with_heap(heap);
                    k_val.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "random.sample() got multiple values for argument 'k'",
                    ));
                }
                k_val = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                counts_val.drop_with_heap(heap);
                k_val.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for random.sample()"
                )));
            }
        }
    }

    let Some(k_val) = k_val else {
        counts_val.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("random.sample", 2, 1));
    };
    defer_drop!(k_val, heap);

    let mut pool = if let Some(counts_val) = counts_val {
        defer_drop!(counts_val, heap);
        let population = sample_population_items(population_val, heap, interns)?;
        let counts = sample_population_items(counts_val, heap, interns)?;
        if counts.len() != population.len() {
            population.drop_with_heap(heap);
            counts.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "The number of counts does not match the population",
            )
            .into());
        }
        let mut expanded = Vec::new();
        for (item, count_value) in population.iter().zip(counts.iter()) {
            let count = count_value.as_int(heap)?;
            if count < 0 {
                population.drop_with_heap(heap);
                counts.drop_with_heap(heap);
                expanded.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "Counts must be non-negative").into());
            }
            let count_usize = usize::try_from(count).expect("count validated non-negative");
            for _ in 0..count_usize {
                expanded.push(item.clone_with_heap(heap));
            }
        }
        population.drop_with_heap(heap);
        counts.drop_with_heap(heap);
        expanded
    } else {
        sample_population_items(population_val, heap, interns)?
    };

    let k = k_val.as_int(heap)?;
    if k < 0 {
        pool.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "k must be non-negative").into());
    }
    let len = pool.len();
    let k_usize = usize::try_from(k).expect("k validated non-negative");
    if k_usize > len {
        pool.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "sample larger than population or is negative").into(),
        );
    }

    let mut results = Vec::with_capacity(k_usize);
    for _ in 0..k_usize {
        let j = os_randbelow_u64(pool.len() as u64) as usize;
        let selected = pool.swap_remove(j);
        results.push(selected.clone_with_heap(heap));
        selected.drop_with_heap(heap);
    }
    for value in pool {
        value.drop_with_heap(heap);
    }

    let list = List::new(results);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Implements `SystemRandom.uniform(self, a, b)`.
fn system_random_method_uniform(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.uniform")?;
    let (a, b) = method_args.get_two_args("SystemRandom.uniform", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    let a_f = value_to_f64(a, heap)?;
    let b_f = value_to_f64(b, heap)?;
    let u = os_random_f64();
    Ok(AttrCallResult::Value(Value::Float(a_f + (b_f - a_f) * u)))
}

/// Implements `SystemRandom.getrandbits(self, k)`.
fn system_random_method_getrandbits(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.getrandbits")?;
    let k_val = method_args.get_one_arg("SystemRandom.getrandbits", heap)?;
    defer_drop!(k_val, heap);
    let k = k_val.as_int(heap)?;
    if k < 0 {
        return Err(ExcType::value_error_cannot_convert_negative_int());
    }
    if k == 0 {
        return Ok(AttrCallResult::Value(Value::Int(0)));
    }
    let k_u64 = u64::try_from(k).expect("validated non-negative");
    if k_u64 <= 63 {
        let value = os_getrandbits_u64(k_u64 as u32);
        let value_i64 = i64::try_from(value).expect("getrandbits <= 63 bits fits in i64");
        return Ok(AttrCallResult::Value(Value::Int(value_i64)));
    }
    let big_int = os_getrandbits(k_u64);
    let long_int = LongInt::new(big_int);
    Ok(AttrCallResult::Value(long_int.into_value(heap)?))
}

/// Implements `SystemRandom.randbytes(self, n)`.
fn system_random_method_randbytes(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (_instance_id, method_args) = extract_instance_self_and_args(args, heap, "SystemRandom.randbytes")?;
    let n_val = method_args.get_one_arg("SystemRandom.randbytes", heap)?;
    defer_drop!(n_val, heap);
    let n = n_val.as_int(heap)?;
    if n < 0 {
        return Err(ExcType::value_error_cannot_convert_negative_int());
    }
    let size = usize::try_from(n).expect("randbytes size validated non-negative");
    let mut bytes = vec![0u8; size];
    os_fill_bytes(&mut bytes);
    let heap_id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

/// Shared sequence-choice helper with injectable `randbelow`.
fn choice_with_randbelow(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
    randbelow: impl Fn(u64) -> u64,
) -> RunResult<AttrCallResult> {
    let seq = args.get_one_arg("random.choice", heap)?;
    let Value::Ref(list_id) = &seq else {
        seq.drop_with_heap(heap);
        return Err(ExcType::type_error("random.choice requires a list"));
    };
    let list = heap.get(*list_id);
    let HeapData::List(list_ref) = list else {
        seq.drop_with_heap(heap);
        return Err(ExcType::type_error("random.choice requires a list"));
    };
    let len = list_ref.len();
    if len == 0 {
        seq.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::IndexError, "Cannot choose from an empty sequence").into());
    }
    let index = randbelow(len as u64) as usize;
    let list_data = heap.get(*list_id);
    let result = if let HeapData::List(list) = list_data {
        list.as_vec()[index].clone_with_heap(heap)
    } else {
        unreachable!("list was validated above")
    };
    seq.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

/// Shared list-shuffle helper with injectable `randbelow`.
fn shuffle_with_randbelow(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
    args: ArgValues,
    randbelow: impl Fn(u64) -> u64,
) -> RunResult<AttrCallResult> {
    let x = args.get_one_arg("random.shuffle", heap)?;
    let Value::Ref(list_id) = &x else {
        x.drop_with_heap(heap);
        return Err(ExcType::type_error("random.shuffle requires a list"));
    };
    let list_data = heap.get(*list_id);
    let HeapData::List(list_ref) = list_data else {
        x.drop_with_heap(heap);
        return Err(ExcType::type_error("random.shuffle requires a list"));
    };
    let len = list_ref.len();
    if len <= 1 {
        x.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(Value::None));
    }
    let list_data = heap.get_mut(*list_id);
    let HeapData::List(list) = list_data else {
        x.drop_with_heap(heap);
        return Err(ExcType::type_error("random.shuffle requires a list"));
    };
    let items = list.as_vec_mut();
    for i in (1..len).rev() {
        let j = randbelow((i + 1) as u64) as usize;
        items.swap(i, j);
    }
    x.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

fn random(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("random.random", heap)?;
    let value = RNG.with(|rng| rng.borrow_mut().random());
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn randint(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (a, b) = args.get_two_args("random.randint", heap)?;
    let a_i64 = a.as_int(heap)?;
    let b_i64 = b.as_int(heap)?;
    a.drop_with_heap(heap);
    b.drop_with_heap(heap);

    if b_i64 < a_i64 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "empty range for randrange()").into());
    }
    let range_size = (i128::from(b_i64) - i128::from(a_i64) + 1) as u64;
    let offset = RNG.with(|rng| rng.borrow_mut().randbelow_u64(range_size));
    let value = a_i64 + i64::try_from(offset).expect("randint offset fits in i64");
    Ok(AttrCallResult::Value(Value::Int(value)))
}

fn choice(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let seq = args.get_one_arg("random.choice", heap)?;

    let Value::Ref(list_id) = &seq else {
        seq.drop_with_heap(heap);
        return Err(ExcType::type_error("random.choice requires a list"));
    };

    let list = heap.get(*list_id);
    let HeapData::List(list_ref) = list else {
        seq.drop_with_heap(heap);
        return Err(ExcType::type_error("random.choice requires a list"));
    };
    let len = list_ref.len();

    if len == 0 {
        seq.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::IndexError, "Cannot choose from an empty sequence").into());
    }

    let index = RNG.with(|rng| rng.borrow_mut().randbelow_u64(len as u64) as usize);
    let list_data = heap.get(*list_id);
    let result = if let HeapData::List(list) = list_data {
        list.as_vec()[index].clone_with_heap(heap)
    } else {
        unreachable!("list was validated above")
    };

    seq.drop_with_heap(heap);
    Ok(AttrCallResult::Value(result))
}

fn shuffle(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let x = args.get_one_arg("random.shuffle", heap)?;

    let Value::Ref(list_id) = &x else {
        x.drop_with_heap(heap);
        return Err(ExcType::type_error("random.shuffle requires a list"));
    };

    let list_data = heap.get(*list_id);
    let HeapData::List(list_ref) = list_data else {
        x.drop_with_heap(heap);
        return Err(ExcType::type_error("random.shuffle requires a list"));
    };
    let len = list_ref.len();

    if len <= 1 {
        x.drop_with_heap(heap);
        return Ok(AttrCallResult::Value(Value::None));
    }

    let list_data = heap.get_mut(*list_id);
    let HeapData::List(list) = list_data else {
        x.drop_with_heap(heap);
        return Err(ExcType::type_error("random.shuffle requires a list"));
    };

    let items = list.as_vec_mut();
    for i in (1..len).rev() {
        let j = RNG.with(|rng| rng.borrow_mut().randbelow_u64((i + 1) as u64) as usize);
        items.swap(i, j);
    }

    x.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::None))
}

fn hash_bytes_to_key(data: &[u8]) -> Vec<u32> {
    let mut key = Vec::new();
    let mut current: u32 = 0;
    let mut shift = 0;

    for &byte in data {
        current |= u32::from(byte) << shift;
        shift += 8;
        if shift >= 32 {
            key.push(current);
            current = 0;
            shift = 0;
        }
    }

    if shift > 0 || key.is_empty() {
        key.push(current);
    }

    key
}

fn py_hash_double(value: f64) -> i64 {
    const PY_HASH_BITS: u32 = 61;
    const PY_HASH_MODULUS: u64 = (1u64 << PY_HASH_BITS) - 1;
    const PY_HASH_INF: i64 = 314_159;
    const CHUNK_MULTIPLIER: f64 = 268_435_456.0; // 2**28
    const CHUNK_BITS: i32 = 28;

    if value.is_infinite() {
        return if value.is_sign_negative() {
            -PY_HASH_INF
        } else {
            PY_HASH_INF
        };
    }
    if value.is_nan() {
        return 0;
    }

    let mut mantissa = value.abs();
    let mut exponent = 0i32;
    while mantissa >= 1.0 {
        mantissa *= 0.5;
        exponent += 1;
    }
    while mantissa < 0.5 && mantissa != 0.0 {
        mantissa *= 2.0;
        exponent -= 1;
    }
    let sign = if value.is_sign_negative() { -1i64 } else { 1i64 };
    let mut hash: u64 = 0;

    while mantissa != 0.0 {
        hash = ((hash << CHUNK_BITS) & PY_HASH_MODULUS) | (hash >> (PY_HASH_BITS - CHUNK_BITS as u32));
        mantissa *= CHUNK_MULTIPLIER;
        exponent -= CHUNK_BITS;
        let chunk = mantissa as u64;
        mantissa -= chunk as f64;
        hash = hash.wrapping_add(chunk);
        if hash >= PY_HASH_MODULUS {
            hash -= PY_HASH_MODULUS;
        }
    }

    let exp = if exponent >= 0 {
        exponent as u32 % PY_HASH_BITS
    } else {
        PY_HASH_BITS - 1 - (((-1 - exponent) as u32) % PY_HASH_BITS)
    };

    hash = ((hash << exp) & PY_HASH_MODULUS) | (hash >> (PY_HASH_BITS - exp));
    let signed = (hash as i64) * sign;
    if signed == -1 { -2 } else { signed }
}

fn bigint_to_key(mut value: BigInt) -> Vec<u32> {
    if value.sign() == Sign::Minus {
        value = -value;
    }
    let (_, bytes) = value.to_bytes_le();
    if bytes.is_empty() {
        return vec![0];
    }
    let mut key = Vec::with_capacity(bytes.len().div_ceil(4));
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        key.push(u32::from_le_bytes(word));
    }
    key
}

fn hash_seed_bytes_to_key(data: &[u8]) -> Vec<u32> {
    let mut hasher = Sha512::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut seed = Vec::with_capacity(data.len() + digest.len());
    seed.extend_from_slice(data);
    seed.extend_from_slice(&digest);
    let seed_int = BigInt::from_bytes_be(Sign::Plus, &seed);
    bigint_to_key(seed_int)
}

fn seed(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let arg = match args {
        ArgValues::Empty => Value::None,
        ArgValues::One(val) => val,
        _ => args.get_one_arg("random.seed", heap)?,
    };
    defer_drop!(arg, heap);

    RNG.with(|rng| seed_mt_with_value(&mut rng.borrow_mut(), arg, heap, interns))?;

    GAUSS_NEXT.with(|gauss| {
        *gauss.borrow_mut() = None;
    });

    Ok(AttrCallResult::Value(Value::None))
}

fn getstate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    args.check_zero_args("random.getstate", heap)?;

    let state_bytes = RNG.with(|rng| {
        let rng = rng.borrow();
        encode_mt_state(&rng)
    });
    let gauss_state = GAUSS_NEXT.with(|gauss| *gauss.borrow());

    let state_bytes_id = heap.allocate(HeapData::Bytes(Bytes::new(state_bytes)))?;
    let state = allocate_tuple(
        vec![
            Value::Int(3),
            Value::Ref(state_bytes_id),
            gauss_state.map_or(Value::None, Value::Float),
        ]
        .into(),
        heap,
    )?;
    Ok(AttrCallResult::Value(state))
}

fn setstate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let state_val = args.get_one_arg("random.setstate", heap)?;
    defer_drop!(state_val, heap);

    let Value::Ref(state_id) = state_val else {
        return Err(ExcType::type_error("state vector must be a tuple"));
    };
    let HeapData::Tuple(state_tuple) = heap.get(*state_id) else {
        return Err(ExcType::type_error("state vector must be a tuple"));
    };

    if state_tuple.as_vec().len() != 3 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "state vector is the wrong size").into());
    }

    let version = state_tuple.as_vec()[0].as_int(heap)?;
    if version != 3 {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("state with version {version} passed to Random.setstate() of version 3"),
        )
        .into());
    }

    let gauss_state = match &state_tuple.as_vec()[2] {
        Value::None => None,
        Value::Float(value) => Some(*value),
        Value::Int(value) => Some(*value as f64),
        _ => return Err(ExcType::type_error("gauss state must be None or a float")),
    };

    let state_bytes = match &state_tuple.as_vec()[1] {
        Value::Ref(bytes_id) => match heap.get(*bytes_id) {
            HeapData::Bytes(bytes) => bytes.as_slice(),
            _ => return Err(ExcType::type_error("state vector must be a bytes object")),
        },
        _ => return Err(ExcType::type_error("state vector must be a bytes object")),
    };

    let parsed_rng = decode_mt_state(state_bytes)?;

    RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        *rng = parsed_rng;
    });
    GAUSS_NEXT.with(|gauss| {
        *gauss.borrow_mut() = gauss_state;
    });

    Ok(AttrCallResult::Value(Value::None))
}

fn uniform(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (a, b) = args.get_two_args("random.uniform", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);

    let a_f = value_to_f64(a, heap)?;
    let b_f = value_to_f64(b, heap)?;

    let u = RNG.with(|rng| rng.borrow_mut().random());
    let value = a_f + (b_f - a_f) * u;
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn randrange(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (start_val, stop_val, step_val) = extract_randrange_args(args, heap)?;
    defer_drop!(start_val, heap);
    defer_drop!(stop_val, heap);
    defer_drop!(step_val, heap);

    let start = start_val.as_int(heap)?;
    let stop = stop_val.as_int(heap)?;
    let step = step_val.as_int(heap)?;

    if step == 0 {
        return Err(ExcType::value_error_range_step_zero());
    }

    let width = i128::from(stop) - i128::from(start);
    let step_i128 = i128::from(step);
    if (step_i128 > 0 && width <= 0) || (step_i128 < 0 && width >= 0) {
        return Err(SimpleException::new_msg(ExcType::ValueError, "empty range for randrange()").into());
    }

    let n = if step_i128 > 0 {
        (width + step_i128 - 1) / step_i128
    } else {
        (width + step_i128 + 1) / step_i128
    };
    if n <= 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "empty range for randrange()").into());
    }

    let index = RNG.with(|rng| rng.borrow_mut().randbelow_u64(n as u64));
    let value_i128 = i128::from(start) + step_i128 * i128::from(index);
    let value = i64::try_from(value_i128).expect("randrange result fits in i64");
    Ok(AttrCallResult::Value(Value::Int(value)))
}

fn randbytes(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let n_val = args.get_one_arg("random.randbytes", heap)?;
    defer_drop!(n_val, heap);

    let n = n_val.as_int(heap)?;
    if n < 0 {
        return Err(ExcType::value_error_cannot_convert_negative_int());
    }

    let size = usize::try_from(n).expect("randbytes size validated non-negative");
    let mut bytes = vec![0u8; size];
    RNG.with(|rng| rng.borrow_mut().fill_bytes(&mut bytes));
    let heap_id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

fn getrandbits(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let k_val = args.get_one_arg("random.getrandbits", heap)?;
    defer_drop!(k_val, heap);

    let k = k_val.as_int(heap)?;
    if k < 0 {
        return Err(ExcType::value_error_cannot_convert_negative_int());
    }
    if k == 0 {
        return Ok(AttrCallResult::Value(Value::Int(0)));
    }

    let k_u64 = u64::try_from(k).expect("getrandbits validated non-negative");
    if k_u64 <= 63 {
        let value = RNG.with(|rng| rng.borrow_mut().getrandbits_u64(k_u64 as u32));
        let value_i64 = i64::try_from(value).expect("getrandbits <= 63 bits fits in i64");
        return Ok(AttrCallResult::Value(Value::Int(value_i64)));
    }

    let big_int = RNG.with(|rng| rng.borrow_mut().getrandbits(k_u64));
    let long_int = LongInt::new(big_int);
    let result = long_int.into_value(heap)?;
    Ok(AttrCallResult::Value(result))
}

fn triangular(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (low_val, high_val, mode_val) = extract_triangular_args(args, heap)?;
    defer_drop!(low_val, heap);
    defer_drop!(high_val, heap);
    let low = value_to_f64(low_val, heap)?;
    let high = value_to_f64(high_val, heap)?;
    let mode = match mode_val {
        Some(mode_val) => {
            defer_drop!(mode_val, heap);
            value_to_f64(mode_val, heap)?
        }
        None => f64::midpoint(low, high),
    };

    if (high - low).abs() <= f64::EPSILON {
        return Ok(AttrCallResult::Value(Value::Float(low)));
    }

    let u = RNG.with(|rng| rng.borrow_mut().random());
    let c = (mode - low) / (high - low);
    let value = if u <= c {
        low + (u * (high - low) * (mode - low)).sqrt()
    } else {
        high - ((1.0 - u) * (high - low) * (high - mode)).sqrt()
    };
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn expovariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let lambd_val = args.get_one_arg("random.expovariate", heap)?;
    defer_drop!(lambd_val, heap);

    let lambd = value_to_f64(lambd_val, heap)?;
    let u = RNG.with(|rng| rng.borrow_mut().random());
    let value = -((1.0 - u).ln()) / lambd;
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn paretovariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let alpha_val = args.get_one_arg("random.paretovariate", heap)?;
    defer_drop!(alpha_val, heap);

    let alpha = value_to_f64(alpha_val, heap)?;
    let u = RNG.with(|rng| rng.borrow_mut().random());
    let value = 1.0 / (1.0 - u).powf(1.0 / alpha);
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn weibullvariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (alpha_val, beta_val) = args.get_two_args("random.weibullvariate", heap)?;
    defer_drop!(alpha_val, heap);
    defer_drop!(beta_val, heap);

    let alpha = value_to_f64(alpha_val, heap)?;
    let beta = value_to_f64(beta_val, heap)?;
    let u = 1.0 - RNG.with(|rng| rng.borrow_mut().random());
    let value = alpha * (-u.ln()).powf(1.0 / beta);
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn binomialvariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (n_val, p_val) = args.get_two_args("random.binomialvariate", heap)?;
    defer_drop!(n_val, heap);
    defer_drop!(p_val, heap);

    let n = n_val.as_int(heap)?;
    let p = value_to_f64(p_val, heap)?;
    if n < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be non-negative").into());
    }
    let value = binomialvariate_impl(n, p)?;
    Ok(AttrCallResult::Value(Value::Int(value)))
}

fn gauss(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mu_val, sigma_val) = args.get_two_args("random.gauss", heap)?;
    defer_drop!(mu_val, heap);
    defer_drop!(sigma_val, heap);

    let mu = value_to_f64(mu_val, heap)?;
    let sigma = value_to_f64(sigma_val, heap)?;
    let z = GAUSS_NEXT.with(|gauss| {
        let mut gauss = gauss.borrow_mut();
        let cached = *gauss;
        *gauss = None;
        cached
    });
    let z = if let Some(z) = z {
        z
    } else {
        let x2pi = RNG.with(|rng| rng.borrow_mut().random()) * TAU;
        let g2rad = (-2.0 * (1.0 - RNG.with(|rng| rng.borrow_mut().random())).ln()).sqrt();
        let z = x2pi.cos() * g2rad;
        let next = x2pi.sin() * g2rad;
        GAUSS_NEXT.with(|gauss| {
            *gauss.borrow_mut() = Some(next);
        });
        z
    };
    let value = mu + z * sigma;
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn normalvariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mu_val, sigma_val) = args.get_two_args("random.normalvariate", heap)?;
    defer_drop!(mu_val, heap);
    defer_drop!(sigma_val, heap);

    let mu = value_to_f64(mu_val, heap)?;
    let sigma = value_to_f64(sigma_val, heap)?;
    let z = loop {
        let u1 = RNG.with(|rng| rng.borrow_mut().random());
        let u2 = 1.0 - RNG.with(|rng| rng.borrow_mut().random());
        let z = NV_MAGICCONST * (u1 - 0.5) / u2;
        let zz = z * z / 4.0;
        if zz <= -u2.ln() {
            break z;
        }
    };
    let value = mu + z * sigma;
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn lognormvariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mu_val, sigma_val) = args.get_two_args("random.lognormvariate", heap)?;
    defer_drop!(mu_val, heap);
    defer_drop!(sigma_val, heap);

    let mu = value_to_f64(mu_val, heap)?;
    let sigma = value_to_f64(sigma_val, heap)?;
    let z = loop {
        let u1 = RNG.with(|rng| rng.borrow_mut().random());
        let u2 = 1.0 - RNG.with(|rng| rng.borrow_mut().random());
        let z = NV_MAGICCONST * (u1 - 0.5) / u2;
        let zz = z * z / 4.0;
        if zz <= -u2.ln() {
            break z;
        }
    };
    let value = (mu + sigma * z).exp();
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn gammavariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (alpha_val, beta_val) = args.get_two_args("random.gammavariate", heap)?;
    defer_drop!(alpha_val, heap);
    defer_drop!(beta_val, heap);

    let alpha = value_to_f64(alpha_val, heap)?;
    let beta = value_to_f64(beta_val, heap)?;
    let value = gammavariate_impl(alpha, beta)?;
    Ok(AttrCallResult::Value(Value::Float(value)))
}

fn betavariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (alpha_val, beta_val) = args.get_two_args("random.betavariate", heap)?;
    defer_drop!(alpha_val, heap);
    defer_drop!(beta_val, heap);

    let alpha = value_to_f64(alpha_val, heap)?;
    let beta = value_to_f64(beta_val, heap)?;
    if alpha <= 0.0 || beta <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "gammavariate: alpha and beta must be > 0.0").into());
    }
    let y = gammavariate_impl(alpha, 1.0)?;
    let value = if y == 0.0 {
        0.0
    } else {
        y / (y + gammavariate_impl(beta, 1.0)?)
    };
    Ok(AttrCallResult::Value(Value::Float(value)))
}

#[expect(
    clippy::many_single_char_names,
    reason = "mathematical formula uses standard variable names"
)]
fn vonmisesvariate(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mu_val, kappa_val) = args.get_two_args("random.vonmisesvariate", heap)?;
    defer_drop!(mu_val, heap);
    defer_drop!(kappa_val, heap);

    let mu = value_to_f64(mu_val, heap)?;
    let kappa = value_to_f64(kappa_val, heap)?;
    if kappa <= 1e-6 {
        let theta = TAU * RNG.with(|rng| rng.borrow_mut().random());
        return Ok(AttrCallResult::Value(Value::Float(theta)));
    }

    let s = 0.5 / kappa;
    let r = s + (1.0 + s * s).sqrt();

    let (z, _d) = loop {
        let u1 = RNG.with(|rng| rng.borrow_mut().random());
        let z = (PI * u1).cos();
        let d = z / (r + z);
        let u2 = RNG.with(|rng| rng.borrow_mut().random());
        if u2 < 1.0 - d * d || u2 <= (1.0 - d) * d.exp() {
            break (z, d);
        }
    };

    let q = 1.0 / r;
    let f = (q + z) / (1.0 + q * z);
    let u3 = RNG.with(|rng| rng.borrow_mut().random());
    let theta = if u3 > 0.5 {
        (mu + f.acos()).rem_euclid(TAU)
    } else {
        (mu - f.acos()).rem_euclid(TAU)
    };

    Ok(AttrCallResult::Value(Value::Float(theta)))
}

fn choices(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (population_val, weights_val, cum_weights_val, k_val) = extract_choices_args(args, heap, interns)?;
    defer_drop!(population_val, heap);
    let k = match k_val {
        Some(k_val) => {
            defer_drop!(k_val, heap);
            k_val.as_int(heap)?
        }
        None => 1,
    };
    if k < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "k must be non-negative").into());
    }
    let k_usize = usize::try_from(k).expect("k validated non-negative");

    let population = sample_population_items(population_val, heap, interns)?;
    let len = population.len();
    if len == 0 {
        population.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::IndexError, "Cannot choose from an empty sequence").into());
    }

    let cumulative = if let Some(weights_val) = weights_val {
        defer_drop!(weights_val, heap);
        let weights = extract_choices_weights(weights_val, len, heap, interns)?;
        let mut cumulative = Vec::with_capacity(len);
        let mut total = 0.0;
        for w in weights {
            total += w;
            cumulative.push(total);
        }
        Some(cumulative)
    } else if let Some(cum_weights_val) = cum_weights_val {
        defer_drop!(cum_weights_val, heap);
        Some(extract_choices_weights(cum_weights_val, len, heap, interns)?)
    } else {
        None
    };

    let mut results = Vec::with_capacity(k_usize);
    if let Some(cumulative) = cumulative {
        let total = *cumulative.last().ok_or_else(|| {
            SimpleException::new_msg(ExcType::ValueError, "Total of weights must be greater than zero")
        })?;
        if !total.is_finite() {
            population.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::ValueError, "Total of weights must be finite").into());
        }
        if total <= 0.0 {
            population.drop_with_heap(heap);
            return Err(
                SimpleException::new_msg(ExcType::ValueError, "Total of weights must be greater than zero").into(),
            );
        }

        for _ in 0..k_usize {
            let u = RNG.with(|rng| rng.borrow_mut().random()) * total;
            let mut lo = 0usize;
            let mut hi = cumulative.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if u < cumulative[mid] {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            let idx = lo.min(len - 1);
            results.push(population[idx].clone_with_heap(heap));
        }
    } else {
        for _ in 0..k_usize {
            let idx = (RNG.with(|rng| rng.borrow_mut().random()) * len as f64) as usize;
            results.push(population[idx].clone_with_heap(heap));
        }
    }
    population.drop_with_heap(heap);

    let list = List::new(results);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

fn sample(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(population_val) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("random.sample", 2, 0));
    };
    let positional_k = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        population_val.drop_with_heap(heap);
        positional_k.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("random.sample", 2, 3));
    }
    positional.drop_with_heap(heap);
    defer_drop!(population_val, heap);

    let mut k_val = positional_k;
    let mut counts_val: Option<Value> = None;
    for (key, value) in kwargs {
        let Some(key_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            counts_val.drop_with_heap(heap);
            k_val.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = key_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "counts" => {
                if let Some(old) = counts_val.replace(value) {
                    old.drop_with_heap(heap);
                    counts_val.drop_with_heap(heap);
                    k_val.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "random.sample() got multiple values for argument 'counts'",
                    ));
                }
            }
            "k" => {
                if k_val.is_some() {
                    value.drop_with_heap(heap);
                    counts_val.drop_with_heap(heap);
                    k_val.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "random.sample() got multiple values for argument 'k'",
                    ));
                }
                k_val = Some(value);
            }
            _ => {
                value.drop_with_heap(heap);
                counts_val.drop_with_heap(heap);
                k_val.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for random.sample()"
                )));
            }
        }
    }

    let Some(k_val) = k_val else {
        counts_val.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("random.sample", 2, 1));
    };
    defer_drop!(k_val, heap);

    let mut pool = if let Some(counts_val) = counts_val {
        defer_drop!(counts_val, heap);
        let population = sample_population_items(population_val, heap, interns)?;
        let counts = sample_population_items(counts_val, heap, interns)?;
        if counts.len() != population.len() {
            population.drop_with_heap(heap);
            counts.drop_with_heap(heap);
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "The number of counts does not match the population",
            )
            .into());
        }
        let mut expanded = Vec::new();
        for (item, count_value) in population.iter().zip(counts.iter()) {
            let count = count_value.as_int(heap)?;
            if count < 0 {
                population.drop_with_heap(heap);
                counts.drop_with_heap(heap);
                expanded.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "Counts must be non-negative").into());
            }
            let count_usize = usize::try_from(count).expect("count validated non-negative");
            for _ in 0..count_usize {
                expanded.push(item.clone_with_heap(heap));
            }
        }
        population.drop_with_heap(heap);
        counts.drop_with_heap(heap);
        expanded
    } else {
        sample_population_items(population_val, heap, interns)?
    };

    let k = k_val.as_int(heap)?;
    if k < 0 {
        pool.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::ValueError, "k must be non-negative").into());
    }

    let len = pool.len();
    let k_usize = usize::try_from(k).expect("k validated non-negative");
    if k_usize > len {
        pool.drop_with_heap(heap);
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "sample larger than population or is negative").into(),
        );
    }

    let mut results = Vec::with_capacity(k_usize);
    for _ in 0..k_usize {
        let j = RNG.with(|rng| rng.borrow_mut().randbelow_u64(pool.len() as u64) as usize);
        let selected = pool.swap_remove(j);
        results.push(selected.clone_with_heap(heap));
        selected.drop_with_heap(heap);
    }

    for value in pool {
        value.drop_with_heap(heap);
    }

    let list = List::new(results);
    let heap_id = heap.allocate(HeapData::List(list))?;
    Ok(AttrCallResult::Value(Value::Ref(heap_id)))
}

fn sample_population_items(
    population: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    if let Value::Ref(id) = population
        && matches!(
            heap.get(*id),
            HeapData::Dict(_) | HeapData::Set(_) | HeapData::FrozenSet(_)
        )
    {
        return Err(ExcType::type_error(
            "Population must be a sequence.  For dicts or sets, use sorted(d).",
        ));
    }
    if population.py_len(heap, interns).is_none() {
        return Err(ExcType::type_error(
            "Population must be a sequence.  For dicts or sets, use sorted(d).",
        ));
    }

    let iterable = population.clone_with_heap(heap);
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut items = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                items.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(items)
}

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

fn extract_randrange_args(args: ArgValues, heap: &mut Heap<impl ResourceTracker>) -> RunResult<(Value, Value, Value)> {
    match args {
        ArgValues::One(stop) => Ok((Value::Int(0), stop, Value::Int(1))),
        ArgValues::Two(start, stop) => Ok((start, stop, Value::Int(1))),
        ArgValues::ArgsKargs { args, kwargs } if kwargs.is_empty() && args.len() == 3 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), iter.next().unwrap(), iter.next().unwrap()))
        }
        ArgValues::ArgsKargs { args, kwargs } if kwargs.is_empty() && args.len() == 2 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), iter.next().unwrap(), Value::Int(1)))
        }
        ArgValues::ArgsKargs { args, kwargs } if kwargs.is_empty() && args.len() == 1 => {
            let mut iter = args.into_iter();
            Ok((Value::Int(0), iter.next().unwrap(), Value::Int(1)))
        }
        ArgValues::Kwargs(kwargs) => {
            kwargs.drop_with_heap(heap);
            Err(ExcType::type_error_no_kwargs("random.randrange"))
        }
        ArgValues::ArgsKargs { args, kwargs } => {
            let count = args.len();
            let has_kwargs = !kwargs.is_empty();
            args.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            if has_kwargs {
                Err(ExcType::type_error_no_kwargs("random.randrange"))
            } else {
                Err(ExcType::type_error(format!(
                    "random.randrange() takes from 1 to 3 positional arguments but {count} were given"
                )))
            }
        }
        other @ (ArgValues::Empty | ArgValues::One(_) | ArgValues::Two(_, _)) => {
            let count = match &other {
                ArgValues::Empty => 0,
                ArgValues::One(_) => 1,
                ArgValues::Two(_, _) => 2,
                ArgValues::Kwargs(_) | ArgValues::ArgsKargs { .. } => {
                    unreachable!("explicitly matched Empty/One/Two above")
                }
            };
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "random.randrange() takes from 1 to 3 positional arguments but {count} were given"
            )))
        }
    }
}

fn extract_triangular_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<(Value, Value, Option<Value>)> {
    match args {
        ArgValues::Empty => Ok((Value::Float(0.0), Value::Float(1.0), None)),
        ArgValues::One(low) => Ok((low, Value::Float(1.0), None)),
        ArgValues::Two(low, high) => Ok((low, high, None)),
        ArgValues::ArgsKargs { args, kwargs } if kwargs.is_empty() && args.len() == 3 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), iter.next().unwrap(), Some(iter.next().unwrap())))
        }
        ArgValues::ArgsKargs { args, kwargs } if kwargs.is_empty() && args.len() == 2 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), iter.next().unwrap(), None))
        }
        ArgValues::ArgsKargs { args, kwargs } if kwargs.is_empty() && args.len() == 1 => {
            let mut iter = args.into_iter();
            Ok((iter.next().unwrap(), Value::Float(1.0), None))
        }
        ArgValues::Kwargs(kwargs) => {
            kwargs.drop_with_heap(heap);
            Err(ExcType::type_error_no_kwargs("random.triangular"))
        }
        ArgValues::ArgsKargs { args, kwargs } => {
            let count = args.len();
            args.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "random.triangular() takes from 0 to 3 positional arguments but {count} were given"
            )))
        }
    }
}

fn extract_choices_args(
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Option<Value>, Option<Value>, Option<Value>)> {
    match args {
        ArgValues::One(population) => Ok((population, None, None, None)),
        ArgValues::Two(population, weights) => Ok((population, Some(weights), None, None)),
        ArgValues::ArgsKargs { args, kwargs } => {
            let positional_count = args.len();
            if positional_count == 0 {
                args.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error_at_least("random.choices", 1, 0));
            }
            if positional_count > 3 {
                args.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "random.choices() takes from 1 to 3 positional arguments but {positional_count} were given"
                )));
            }
            let mut iter = args.into_iter();
            let population = iter.next().expect("validated at least one positional argument");
            let positional_weights = iter.next();
            let positional_k = iter.next();

            let mut keyword_weights: Option<Value> = None;
            let mut keyword_cum_weights: Option<Value> = None;
            let mut keyword_k: Option<Value> = None;
            for (key, value) in kwargs {
                let Some(key_name) = key.as_either_str(heap) else {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    population.drop_with_heap(heap);
                    positional_weights.drop_with_heap(heap);
                    positional_k.drop_with_heap(heap);
                    keyword_weights.drop_with_heap(heap);
                    keyword_cum_weights.drop_with_heap(heap);
                    keyword_k.drop_with_heap(heap);
                    return Err(ExcType::type_error("keywords must be strings"));
                };
                let key_name = key_name.as_str(interns).to_owned();
                key.drop_with_heap(heap);
                match key_name.as_str() {
                    "weights" => {
                        if let Some(old) = keyword_weights.replace(value) {
                            old.drop_with_heap(heap);
                            population.drop_with_heap(heap);
                            positional_weights.drop_with_heap(heap);
                            positional_k.drop_with_heap(heap);
                            keyword_weights.drop_with_heap(heap);
                            keyword_cum_weights.drop_with_heap(heap);
                            keyword_k.drop_with_heap(heap);
                            return Err(ExcType::type_error(
                                "random.choices() got multiple values for argument 'weights'",
                            ));
                        }
                    }
                    "cum_weights" => {
                        if let Some(old) = keyword_cum_weights.replace(value) {
                            old.drop_with_heap(heap);
                            population.drop_with_heap(heap);
                            positional_weights.drop_with_heap(heap);
                            positional_k.drop_with_heap(heap);
                            keyword_weights.drop_with_heap(heap);
                            keyword_cum_weights.drop_with_heap(heap);
                            keyword_k.drop_with_heap(heap);
                            return Err(ExcType::type_error(
                                "random.choices() got multiple values for argument 'cum_weights'",
                            ));
                        }
                    }
                    "k" => {
                        if let Some(old) = keyword_k.replace(value) {
                            old.drop_with_heap(heap);
                            population.drop_with_heap(heap);
                            positional_weights.drop_with_heap(heap);
                            positional_k.drop_with_heap(heap);
                            keyword_weights.drop_with_heap(heap);
                            keyword_cum_weights.drop_with_heap(heap);
                            keyword_k.drop_with_heap(heap);
                            return Err(ExcType::type_error(
                                "random.choices() got multiple values for argument 'k'",
                            ));
                        }
                    }
                    _ => {
                        value.drop_with_heap(heap);
                        population.drop_with_heap(heap);
                        positional_weights.drop_with_heap(heap);
                        positional_k.drop_with_heap(heap);
                        keyword_weights.drop_with_heap(heap);
                        keyword_cum_weights.drop_with_heap(heap);
                        keyword_k.drop_with_heap(heap);
                        return Err(ExcType::type_error(format!(
                            "'{key_name}' is an invalid keyword argument for random.choices()"
                        )));
                    }
                }
            }

            if positional_weights.is_some() && keyword_weights.is_some() {
                population.drop_with_heap(heap);
                positional_weights.drop_with_heap(heap);
                positional_k.drop_with_heap(heap);
                keyword_weights.drop_with_heap(heap);
                keyword_cum_weights.drop_with_heap(heap);
                keyword_k.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "random.choices() got multiple values for argument 'weights'",
                ));
            }
            if positional_weights.is_some() && keyword_cum_weights.is_some() {
                population.drop_with_heap(heap);
                positional_weights.drop_with_heap(heap);
                positional_k.drop_with_heap(heap);
                keyword_weights.drop_with_heap(heap);
                keyword_cum_weights.drop_with_heap(heap);
                keyword_k.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "random.choices() got multiple values for argument 'weights'",
                ));
            }
            if keyword_weights.is_some() && keyword_cum_weights.is_some() {
                population.drop_with_heap(heap);
                positional_weights.drop_with_heap(heap);
                positional_k.drop_with_heap(heap);
                keyword_weights.drop_with_heap(heap);
                keyword_cum_weights.drop_with_heap(heap);
                keyword_k.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "Cannot specify both weights and cumulative weights",
                ));
            }
            if positional_k.is_some() && keyword_k.is_some() {
                population.drop_with_heap(heap);
                positional_weights.drop_with_heap(heap);
                positional_k.drop_with_heap(heap);
                keyword_weights.drop_with_heap(heap);
                keyword_cum_weights.drop_with_heap(heap);
                keyword_k.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "random.choices() got multiple values for argument 'k'",
                ));
            }

            Ok((
                population,
                keyword_weights.or(positional_weights),
                keyword_cum_weights,
                keyword_k.or(positional_k),
            ))
        }
        ArgValues::Kwargs(kwargs) => {
            kwargs.drop_with_heap(heap);
            Err(ExcType::type_error_at_least("random.choices", 1, 0))
        }
        other @ (ArgValues::Empty | ArgValues::One(_) | ArgValues::Two(_, _)) => {
            let count = match &other {
                ArgValues::Empty => 0,
                ArgValues::One(_) => 1,
                ArgValues::Two(_, _) => 2,
                ArgValues::Kwargs(_) | ArgValues::ArgsKargs { .. } => {
                    unreachable!("explicitly matched Empty/One/Two above")
                }
            };
            other.drop_with_heap(heap);
            Err(ExcType::type_error(format!(
                "random.choices() takes from 1 to 3 positional arguments but {count} were given"
            )))
        }
    }
}

fn extract_choices_weights(
    weights_value: &Value,
    population_len: usize,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<f64>> {
    if weights_value.py_len(heap, interns).is_none() {
        return Err(ExcType::type_error("random.choices requires a sequence of weights"));
    }

    let iterable = weights_value.clone_with_heap(heap);
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    let mut weights = Vec::new();
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let parsed = value_to_f64(&item, heap);
                item.drop_with_heap(heap);
                match parsed {
                    Ok(weight) => weights.push(weight),
                    Err(err) => {
                        iter.drop_with_heap(heap);
                        return Err(err);
                    }
                }
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);

    if weights.len() != population_len {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "weights and population must be the same length").into(),
        );
    }
    Ok(weights)
}

fn gammavariate_impl(alpha: f64, beta: f64) -> RunResult<f64> {
    if alpha <= 0.0 || beta <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "gammavariate: alpha and beta must be > 0.0").into());
    }

    if alpha > 1.0 {
        let ainv = (2.0 * alpha - 1.0).sqrt();
        let bbb = alpha - LOG4;
        let ccc = alpha + ainv;
        loop {
            let u1 = RNG.with(|rng| rng.borrow_mut().random());
            if !(1e-7 < u1 && u1 < 0.999_999_9) {
                continue;
            }
            let u2 = 1.0 - RNG.with(|rng| rng.borrow_mut().random());
            let v = (u1 / (1.0 - u1)).ln() / ainv;
            let x = alpha * v.exp();
            let z = u1 * u1 * u2;
            let r = bbb + ccc * v - x;
            if r + SG_MAGICCONST - 4.5 * z >= 0.0 || r >= z.ln() {
                return Ok(x * beta);
            }
        }
    }

    if (alpha - 1.0).abs() <= f64::EPSILON {
        let value = -((1.0 - RNG.with(|rng| rng.borrow_mut().random())).ln()) * beta;
        return Ok(value);
    }

    loop {
        let u = RNG.with(|rng| rng.borrow_mut().random());
        let b = (std::f64::consts::E + alpha) / std::f64::consts::E;
        let p = b * u;
        let x = if p <= 1.0 {
            p.powf(1.0 / alpha)
        } else {
            -((b - p) / alpha).ln()
        };
        let u1 = RNG.with(|rng| rng.borrow_mut().random());
        if p > 1.0 {
            if u1 <= x.powf(alpha - 1.0) {
                return Ok(x * beta);
            }
        } else if u1 <= (-x).exp() {
            return Ok(x * beta);
        }
    }
}

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

fn ln_gamma(x: f64) -> f64 {
    lanczos_gamma(x).abs().ln()
}

fn binomialvariate_impl(n: i64, p: f64) -> RunResult<i64> {
    if p <= 0.0 || p >= 1.0 {
        if p == 0.0 {
            return Ok(0);
        }
        if p == 1.0 {
            return Ok(n);
        }
        return Err(SimpleException::new_msg(ExcType::ValueError, "p must be in the range 0.0 <= p <= 1.0").into());
    }

    if n == 1 {
        let sample = RNG.with(|rng| rng.borrow_mut().random());
        return Ok(i64::from(sample < p));
    }

    if p > 0.5 {
        return Ok(n - binomialvariate_impl(n, 1.0 - p)?);
    }

    if (n as f64) * p < 10.0 {
        let mut x = 0i64;
        let mut y = 0.0f64;
        let c = (1.0 - p).log2();
        if c == 0.0 {
            return Ok(x);
        }
        loop {
            let sample = RNG.with(|rng| rng.borrow_mut().random());
            y += (sample.log2() / c).floor() + 1.0;
            if y > n as f64 {
                return Ok(x);
            }
            x += 1;
        }
    }

    let spq = (n as f64 * p * (1.0 - p)).sqrt();
    let b = 1.15 + 2.53 * spq;
    let a = -0.0873 + 0.0248 * b + 0.01 * p;
    let c = n as f64 * p + 0.5;
    let vr = 0.92 - 4.2 / b;

    let mut setup_complete = false;
    let mut alpha = 0.0;
    let mut lpq = 0.0;
    let mut m = 0.0;
    let mut h = 0.0;

    loop {
        let mut u = RNG.with(|rng| rng.borrow_mut().random());
        u -= 0.5;
        let us = 0.5 - u.abs();
        let k = ((2.0 * a / us + b) * u + c).floor();
        if k < 0.0 || k > n as f64 {
            continue;
        }

        let mut v = RNG.with(|rng| rng.borrow_mut().random());
        if us >= 0.07 && v <= vr {
            return Ok(k as i64);
        }

        if !setup_complete {
            alpha = (2.83 + 5.1 / b) * spq;
            lpq = (p / (1.0 - p)).ln();
            m = ((n + 1) as f64 * p).floor();
            h = ln_gamma(m + 1.0) + ln_gamma(n as f64 - m + 1.0);
            setup_complete = true;
        }

        v *= alpha / (a / (us * us) + b);
        if v <= 0.0 {
            continue;
        }
        if v.ln() <= h - ln_gamma(k + 1.0) - ln_gamma(n as f64 - k + 1.0) + (k - m) * lpq {
            return Ok(k as i64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt_seed_from_array_matches_cpython_for_42() {
        let mut rng = Mt19937::new();
        rng.seed_from_array(&[42]);
        assert_eq!(rng.next_u32(), 2_746_317_213);

        let mut rng = Mt19937::new();
        rng.seed_from_array(&[42]);
        assert_eq!(rng.random(), 0.6394267984578837);
    }

    #[test]
    fn float_hash_matches_cpython_reference() {
        assert_eq!(py_hash_double(3.14159), 326_484_311_674_566_659);
    }
}
