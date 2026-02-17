import timeit

# === default_timer ===
try:
    t1 = timeit.default_timer()
    t2 = timeit.default_timer()
    print('default_timer_type_1', type(t1).__name__)
    print('default_timer_type_2', type(t2).__name__)
    print('default_timer_monotonic', t2 >= t1)
except Exception as e:
    print('SKIP_default_timer', type(e).__name__, e)

# === timeit ===
try:
    value = timeit.timeit(stmt='pass', number=3)
    print('timeit_type', type(value).__name__)
    print('timeit_non_negative', value >= 0.0)
    print('timeit_zero_type', type(timeit.timeit(number=0)).__name__)
except Exception as e:
    print('SKIP_timeit', type(e).__name__, e)

# === repeat ===
try:
    values = timeit.repeat(stmt='pass', repeat=4, number=2)
    print('repeat_type', type(values).__name__)
    print('repeat_len', len(values))
    print('repeat_item_types', [type(x).__name__ for x in values])
    print('repeat_zero', timeit.repeat(repeat=0, number=1))
except Exception as e:
    print('SKIP_repeat', type(e).__name__, e)
