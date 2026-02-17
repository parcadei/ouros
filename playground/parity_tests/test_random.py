import random

# === Bookkeeping Functions ===
try:
    # seed with int
    random.seed(42)
    print('seed_int_random', random.random())

    # seed with None (system time)
    random.seed(None)
    print('seed_none_random', random.random())

    # seed with string
    random.seed('hello world')
    print('seed_str_random', random.random())

    # seed with bytes
    random.seed(b'byte seed')
    print('seed_bytes_random', random.random())

    # seed with bytearray
    random.seed(bytearray(b'bytearray seed'))
    print('seed_bytearray_random', random.random())

    # seed with float
    random.seed(3.14159)
    print('seed_float_random', random.random())

    # getstate and setstate
    random.seed(12345)
    state = random.getstate()
    print('getstate_random1', random.random())
    print('getstate_random2', random.random())
    random.setstate(state)
    print('setstate_random', random.random())
except Exception as e:
    print('SKIP_Bookkeeping Functions', type(e).__name__, e)

# === Functions for Bytes ===
try:
    # randbytes
    random.seed(42)
    print('randbytes_8', list(random.randbytes(8)))
    print('randbytes_0', list(random.randbytes(0)))
    print('randbytes_16', list(random.randbytes(16)))
except Exception as e:
    print('SKIP_Functions for Bytes', type(e).__name__, e)

# === Functions for Integers ===
try:
    # randrange with stop only
    random.seed(42)
    print('randrange_stop', random.randrange(10))

    # randrange with start and stop
    random.seed(42)
    print('randrange_start_stop', random.randrange(5, 15))

    # randrange with start, stop, step
    random.seed(42)
    print('randrange_start_stop_step', random.randrange(0, 100, 10))

    # randint
    random.seed(42)
    print('randint', random.randint(1, 100))

    # getrandbits
    random.seed(42)
    print('getrandbits_8', random.getrandbits(8))
    print('getrandbits_16', random.getrandbits(16))
    print('getrandbits_32', random.getrandbits(32))
    print('getrandbits_64', random.getrandbits(64))
    print('getrandbits_0', random.getrandbits(0))
except Exception as e:
    print('SKIP_Functions for Integers', type(e).__name__, e)

# === Functions for Sequences ===
try:
    # choice
    random.seed(42)
    items = ['a', 'b', 'c', 'd', 'e']
    print('choice', random.choice(items))

    # choices
    random.seed(42)
    print('choices_default', random.choices(items, k=5))

    # choices with weights
    random.seed(42)
    weights = [10, 1, 1, 1, 1]
    print('choices_weights', random.choices(items, weights=weights, k=5))

    # choices with cum_weights
    random.seed(42)
    cum_weights = [10, 11, 12, 13, 14]
    print('choices_cum_weights', random.choices(items, cum_weights=cum_weights, k=5))

    # shuffle
    random.seed(42)
    lst = [1, 2, 3, 4, 5]
    random.shuffle(lst)
    print('shuffle', lst)

    # shuffle with larger list
    random.seed(42)
    lst = list(range(20))
    random.shuffle(lst)
    print('shuffle_large', lst[:10], lst[10:])

    # sample
    random.seed(42)
    print('sample', random.sample(items, 3))

    # sample with k=len
    random.seed(42)
    print('sample_all', random.sample(items, len(items)))

    # sample from range
    random.seed(42)
    print('sample_range', random.sample(range(100), 5))

    # sample with counts (Python 3.9+)
    random.seed(42)
    try:
        print('sample_counts', random.sample(['a', 'b', 'c'], counts=[5, 5, 5], k=10))
    except TypeError:
        print('sample_counts_not_supported', 'N/A')
except Exception as e:
    print('SKIP_Functions for Sequences', type(e).__name__, e)

# === Real-valued Distributions ===
try:
    # random (0.0 <= X < 1.0)
    random.seed(42)
    print('random_float1', random.random())
    print('random_float2', random.random())
    print('random_float3', random.random())

    # uniform
    random.seed(42)
    print('uniform_0_1', random.uniform(0, 1))
    print('uniform_10_20', random.uniform(10, 20))
    print('uniform_neg', random.uniform(-5, 5))

    # triangular
    random.seed(42)
    print('triangular_default', random.triangular(0, 10))
    print('triangular_mode', random.triangular(0, 10, 5))

    # gauss
    random.seed(42)
    print('gauss_0_1', random.gauss(0, 1))
    print('gauss_10_2', random.gauss(10, 2))
    print('gauss_neg', random.gauss(-5, 3))

    # normalvariate (same distribution as gauss)
    random.seed(42)
    print('normalvariate_0_1', random.normalvariate(0, 1))
    print('normalvariate_10_2', random.normalvariate(10, 2))

    # lognormvariate
    random.seed(42)
    print('lognormvariate_0_1', random.lognormvariate(0, 1))
    print('lognormvariate_1_0.5', random.lognormvariate(1, 0.5))

    # expovariate
    random.seed(42)
    print('expovariate_1', random.expovariate(1))
    print('expovariate_0.5', random.expovariate(0.5))
    print('expovariate_2', random.expovariate(2))

    # vonmisesvariate
    random.seed(42)
    print('vonmisesvariate_0_1', random.vonmisesvariate(0, 1))
    print('vonmisesvariate_pi_2', random.vonmisesvariate(3.14159, 2))

    # gammavariate
    random.seed(42)
    print('gammavariate_1_1', random.gammavariate(1, 1))
    print('gammavariate_2_2', random.gammavariate(2, 2))

    # betavariate
    random.seed(42)
    print('betavariate_1_1', random.betavariate(1, 1))
    print('betavariate_2_5', random.betavariate(2, 5))

    # paretovariate
    random.seed(42)
    print('paretovariate_1', random.paretovariate(1))
    print('paretovariate_2', random.paretovariate(2))

    # weibullvariate
    random.seed(42)
    print('weibullvariate_1_1', random.weibullvariate(1, 1))
    print('weibullvariate_2_5', random.weibullvariate(2, 5))
except Exception as e:
    print('SKIP_Real-valued Distributions', type(e).__name__, e)

# === binomialvariate (Python 3.12+) ===
try:
    random.seed(42)
    try:
        print('binomialvariate_10_0.5', random.binomialvariate(10, 0.5))
        print('binomialvariate_100_0.3', random.binomialvariate(100, 0.3))
        print('binomialvariate_20_0.8', random.binomialvariate(20, 0.8))
    except AttributeError:
        print('binomialvariate_not_supported', 'N/A')
except Exception as e:
    print('SKIP_binomialvariate (Python 3.12+)', type(e).__name__, e)

# === Random Class ===
try:
    # Random instance creation
    r1 = random.Random(42)
    r2 = random.Random(42)
    print('Random_instance_same_seed', r1.random(), r2.random())

    # Random instance independent state
    r1 = random.Random(42)
    r2 = random.Random(100)
    print('Random_instance_diff_seed', r1.random(), r2.random())

    # Random getstate/setstate
    r = random.Random(12345)
    state = r.getstate()
    a = r.random()
    r.setstate(state)
    b = r.random()
    print('Random_getset_state', a, b)

    # Random methods
    r = random.Random(42)
    print('Random_randint', r.randint(1, 100))
    print('Random_randrange', r.randrange(10))
    items = ['a', 'b', 'c', 'd', 'e']
    print('Random_choice', r.choice(items))
    print('Random_shuffle', end=' ')
    l = [1, 2, 3, 4, 5]
    r.shuffle(l)
    print(l)
    print('Random_sample', r.sample(items, 3))
    print('Random_gauss', r.gauss(0, 1))
    print('Random_uniform', r.uniform(0, 1))

    # Random seed with different types
    r = random.Random()
    r.seed(42)
    print('Random_seed_int', r.random())
    r.seed('test')
    print('Random_seed_str', r.random())
    r.seed(b'bytes')
    print('Random_seed_bytes', r.random())
except Exception as e:
    print('SKIP_Random Class', type(e).__name__, e)

# === SystemRandom Class ===
try:
    # SystemRandom instance
    sr = random.SystemRandom()

    # SystemRandom random
    print('SystemRandom_random', sr.random())

    # SystemRandom randint
    print('SystemRandom_randint', sr.randint(1, 100))

    # SystemRandom randrange
    print('SystemRandom_randrange', sr.randrange(10, 100, 5))

    # SystemRandom choice
    items = ['a', 'b', 'c', 'd', 'e']
    print('SystemRandom_choice', sr.choice(items))

    # SystemRandom choices
    print('SystemRandom_choices', sr.choices(items, k=3))

    # SystemRandom shuffle
    l = [1, 2, 3, 4, 5]
    sr.shuffle(l)
    print('SystemRandom_shuffle', l)

    # SystemRandom sample
    print('SystemRandom_sample', sr.sample(items, 3))

    # SystemRandom uniform
    print('SystemRandom_uniform', sr.uniform(0, 100))

    # SystemRandom getrandbits
    print('SystemRandom_getrandbits', sr.getrandbits(32))

    # SystemRandom randbytes
    print('SystemRandom_randbytes', list(sr.randbytes(8)))
except Exception as e:
    print('SKIP_SystemRandom Class', type(e).__name__, e)

# === Module Constants ===
try:
    print('constant_BPF', random.BPF)
    print('constant_LOG4', random.LOG4)
    print('constant_NV_MAGICCONST', random.NV_MAGICCONST)
    print('constant_RECIP_BPF', random.RECIP_BPF)
    print('constant_SG_MAGICCONST', random.SG_MAGICCONST)
    print('constant_TWOPI', random.TWOPI)
except Exception as e:
    print('SKIP_Module Constants', type(e).__name__, e)

# === Edge Cases ===
try:
    # choice with single element
    random.seed(42)
    print('choice_single', random.choice(['only']))

    # sample k=1
    random.seed(42)
    items = ['a', 'b', 'c', 'd', 'e']
    print('sample_k1', random.sample(items, 1))

    # shuffle empty list
    empty = []
    random.shuffle(empty)
    print('shuffle_empty', empty)

    # shuffle single element
    single = ['x']
    random.shuffle(single)
    print('shuffle_single', single)

    # randbytes large
    random.seed(42)
    large = random.randbytes(100)
    print('randbytes_large_len', len(large))
    print('randbytes_large_first5', list(large[:5]))

    # getrandbits large
    random.seed(42)
    print('getrandbits_128', random.getrandbits(128))
    print('getrandbits_256', random.getrandbits(256))

    # multiple seeds in sequence
    random.seed(1)
    a = random.random()
    random.seed(2)
    b = random.random()
    random.seed(1)
    c = random.random()
    print('multiple_seeds', a, b, c, a == c)
except Exception as e:
    print('SKIP_Edge Cases', type(e).__name__, e)
