import timeit

# === default_timer ===
t1 = timeit.default_timer()
t2 = timeit.default_timer()
assert isinstance(t1, float), 'default_timer_type'
assert isinstance(t2, float), 'default_timer_type_2'
assert t2 >= t1, 'default_timer_monotonic'

# === timeit ===
value = timeit.timeit(stmt='pass', number=3)
assert isinstance(value, float), 'timeit_type'
assert value >= 0.0, 'timeit_non_negative'
assert isinstance(timeit.timeit(number=0), float), 'timeit_zero_number_type'

# === repeat ===
items = timeit.repeat(stmt='pass', repeat=4, number=2)
assert isinstance(items, list), 'repeat_type'
assert len(items) == 4, 'repeat_len'
assert all(isinstance(x, float) for x in items), 'repeat_item_type'
assert timeit.repeat(repeat=0, number=1) == [], 'repeat_zero'
