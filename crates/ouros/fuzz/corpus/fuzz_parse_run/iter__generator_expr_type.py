gen_result = (x * 2 for x in range(5))
assert type(gen_result).__name__ == 'generator', 'generator expr returns generator object'
assert list(gen_result) == [0, 2, 4, 6, 8], 'generator expr items'
