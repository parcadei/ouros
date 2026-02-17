import random

# === uniform ===
x = random.uniform(1.0, 5.0)
assert isinstance(x, float), 'uniform should return float'
assert 1.0 <= x <= 5.0, 'uniform should be within range'

# === randrange ===
x = random.randrange(10)
assert isinstance(x, int), 'randrange should return int'
assert 0 <= x < 10, 'randrange(stop) should be within range'
x = random.randrange(5, 15)
assert 5 <= x < 15, 'randrange(start, stop) should be within range'
x = random.randrange(10, 0, -2)
assert x in range(10, 0, -2), 'randrange with negative step should respect range'
try:
    random.randrange(0)
    assert False, 'randrange(0) should raise ValueError'
except ValueError as exc:
    assert str(exc) == 'empty range for randrange()', 'randrange empty range message should match'

# === randbytes ===
b = random.randbytes(0)
assert isinstance(b, bytes), 'randbytes should return bytes'
assert len(b) == 0, 'randbytes(0) should return empty bytes'
b = random.randbytes(5)
assert isinstance(b, bytes), 'randbytes should return bytes'
assert len(b) == 5, 'randbytes should return requested length'
try:
    random.randbytes(-1)
    assert False, 'randbytes negative should raise ValueError'
except ValueError as exc:
    assert str(exc) == 'Cannot convert negative int', 'randbytes negative message should match'

# === getrandbits ===
v = random.getrandbits(0)
assert isinstance(v, int), 'getrandbits should return int'
assert v == 0, 'getrandbits(0) should be zero'
v = random.getrandbits(5)
assert 0 <= v < 32, 'getrandbits(5) should be within 5 bits'
try:
    random.getrandbits(-1)
    assert False, 'getrandbits negative should raise ValueError'
except ValueError as exc:
    assert str(exc) == 'Cannot convert negative int', 'getrandbits negative message should match'

# === getstate / setstate ===
random.seed(987654321)
state = random.getstate()
assert isinstance(state, tuple), 'getstate should return tuple'
assert len(state) == 3, 'getstate tuple should have 3 items'
assert state[0] == 3, 'getstate version should be 3'

a = random.random()
b = random.getrandbits(40)
random.setstate(state)
a2 = random.random()
b2 = random.getrandbits(40)
assert a == a2, 'setstate should restore random() sequence'
assert b == b2, 'setstate should restore getrandbits() sequence'

try:
    random.setstate((1, state[1], None))
    assert False, 'setstate with unsupported version should raise ValueError'
except ValueError as exc:
    assert str(exc) == 'state with version 1 passed to Random.setstate() of version 3', (
        'setstate version message should match'
    )

# === triangular ===
x = random.triangular()
assert isinstance(x, float), 'triangular should return float'
assert 0.0 <= x <= 1.0, 'triangular default should be within range'
x = random.triangular(2.0, 5.0, 3.0)
assert 2.0 <= x <= 5.0, 'triangular should be within range'

# === expovariate, paretovariate, weibullvariate ===
x = random.expovariate(1.5)
assert isinstance(x, float), 'expovariate should return float'
assert x >= 0.0, 'expovariate should be non-negative'
x = random.paretovariate(2.0)
assert isinstance(x, float), 'paretovariate should return float'
assert x >= 1.0, 'paretovariate should be >= 1.0'
x = random.weibullvariate(1.0, 2.0)
assert isinstance(x, float), 'weibullvariate should return float'
assert x >= 0.0, 'weibullvariate should be non-negative'

# === binomialvariate ===
x = random.binomialvariate(10, 0.5)
assert isinstance(x, int), 'binomialvariate should return int'
assert 0 <= x <= 10, 'binomialvariate result should be within [0, n]'
try:
    random.binomialvariate(-1, 0.5)
    assert False, 'binomialvariate negative n should raise ValueError'
except ValueError as exc:
    assert str(exc) == 'n must be non-negative', 'binomialvariate error message should match'

# === gauss, normalvariate, lognormvariate ===
x = random.gauss(0.0, 1.0)
assert isinstance(x, float), 'gauss should return float'
x = random.normalvariate(0.0, 1.0)
assert isinstance(x, float), 'normalvariate should return float'
x = random.lognormvariate(0.0, 1.0)
assert isinstance(x, float), 'lognormvariate should return float'
assert x > 0.0, 'lognormvariate should be positive'

# === gammavariate, betavariate, vonmisesvariate ===
x = random.gammavariate(2.0, 3.0)
assert isinstance(x, float), 'gammavariate should return float'
assert x >= 0.0, 'gammavariate should be non-negative'
x = random.betavariate(0.5, 0.5)
assert isinstance(x, float), 'betavariate should return float'
assert 0.0 <= x <= 1.0, 'betavariate should be within [0, 1]'
x = random.vonmisesvariate(0.0, 1.0)
assert isinstance(x, float), 'vonmisesvariate should return float'

# === choices ===
pop = [1, 2, 3]
res = random.choices(pop)
assert isinstance(res, list), 'choices should return list'
assert len(res) == 1, 'choices default k should be 1'
assert res[0] in pop, 'choices should select from population'
res_kw = random.choices(pop, k=4)
assert len(res_kw) == 4, 'choices should accept keyword-only k'
assert all(item in pop for item in res_kw), 'choices k results should be from population'
try:
    # Ouros currently accepts positional k; CPython requires k to be keyword-only.
    res = random.choices(pop, [1.0, 2.0, 3.0], 5)
except TypeError:
    res = random.choices(pop, [1.0, 2.0, 3.0], k=5)
assert len(res) == 5, 'choices should return k items'
assert all(item in pop for item in res), 'choices results should be from population'

# === sample ===
res = random.sample([1, 2, 3, 4], 2)
assert isinstance(res, list), 'sample should return list'
assert len(res) == 2, 'sample should return k items'
assert len(set(res)) == 2, 'sample should be without replacement'
assert all(item in [1, 2, 3, 4] for item in res), 'sample results should be from population'
res = random.sample([1, 2, 3], 0)
assert res == [], 'sample of zero should be empty'
try:
    random.sample([1], 2)
    assert False, 'sample larger than population should raise ValueError'
except ValueError as exc:
    assert (
        str(exc) == 'sample larger than population or is negative'
        or str(exc) == 'Sample larger than population or is negative'
    ), 'sample error message should match'
