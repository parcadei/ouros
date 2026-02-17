import itertools
import operator

# === accumulate ===
try:
    # Normal usage with default addition
    print('accumulate_list', list(itertools.accumulate([1, 2, 3, 4])))
    # Empty list
    print('accumulate_empty', list(itertools.accumulate([])))
    # Custom function (multiplication)
    print('accumulate_mul', list(itertools.accumulate([1, 2, 3, 4], operator.mul)))
    # With initial value
    print('accumulate_initial', list(itertools.accumulate([1, 2, 3], initial=100)))
    # With max function
    print('accumulate_max', list(itertools.accumulate([3, 1, 4, 1, 5], max)))
    # Single element
    print('accumulate_single', list(itertools.accumulate([42])))
except Exception as e:
    print('SKIP_accumulate', type(e).__name__, e)

# === batched ===
try:
    # Normal usage
    print('batched_normal', list(itertools.batched('ABCDEFG', 3)))
    # Exact batch size
    print('batched_exact', list(itertools.batched('ABCDEF', 3)))
    # Batch size 1
    print('batched_size_1', list(itertools.batched('ABC', 1)))
    # Batch size larger than iterable
    print('batched_large_n', list(itertools.batched('AB', 5)))
    # Empty iterable
    print('batched_empty', list(itertools.batched([], 3)))
    # Strict mode (no partial batch)
    try:
        list(itertools.batched('ABCDEFG', 3, strict=True))
    except ValueError as e:
        print('batched_strict_error', str(e))
    # Strict mode with exact batch
    print('batched_strict_ok', list(itertools.batched('ABCDEF', 3, strict=True)))
except Exception as e:
    print('SKIP_batched', type(e).__name__, e)

# === chain ===
try:
    # Normal usage
    print('chain_two', list(itertools.chain('ABC', 'DEF')))
    # Multiple iterables
    print('chain_three', list(itertools.chain('A', 'BC', 'DEF')))
    # Empty iterables
    print('chain_with_empty', list(itertools.chain('', 'ABC', '')))
    # All empty
    print('chain_all_empty', list(itertools.chain('', '')))
    # Single iterable
    print('chain_single', list(itertools.chain('ABC')))
    # No iterables
    print('chain_none', list(itertools.chain()))
    
    # === chain.from_iterable ===
    # Normal usage
    print('chain_from_iterable', list(itertools.chain.from_iterable(['ABC', 'DEF'])))
    # Nested lists
    print('chain_from_iterable_lists', list(itertools.chain.from_iterable([[1, 2], [3, 4]])))
    # Mixed types
    print('chain_from_iterable_mixed', list(itertools.chain.from_iterable(['AB', [3, 4]])))
    # Empty outer
    print('chain_from_iterable_empty_outer', list(itertools.chain.from_iterable([])))
    # Empty inner
    print('chain_from_iterable_empty_inner', list(itertools.chain.from_iterable(['', 'ABC'])))
except Exception as e:
    print('SKIP_chain', type(e).__name__, e)

# === combinations ===
try:
    # Normal usage
    print('combinations_2', list(itertools.combinations('ABCD', 2)))
    # r = 0
    print('combinations_r0', list(itertools.combinations('ABC', 0)))
    # r = 1
    print('combinations_r1', list(itertools.combinations('ABC', 1)))
    # r = len(iterable)
    print('combinations_rN', list(itertools.combinations('ABC', 3)))
    # r > len(iterable)
    print('combinations_r_gt_n', list(itertools.combinations('ABC', 5)))
    # Empty iterable
    print('combinations_empty', list(itertools.combinations([], 2)))
    # List input
    print('combinations_list', list(itertools.combinations([1, 2, 3], 2)))
except Exception as e:
    print('SKIP_combinations', type(e).__name__, e)

# === combinations_with_replacement ===
try:
    # Normal usage
    print('combinations_wr_2', list(itertools.combinations_with_replacement('ABC', 2)))
    # r = 0
    print('combinations_wr_r0', list(itertools.combinations_with_replacement('ABC', 0)))
    # r = 1
    print('combinations_wr_r1', list(itertools.combinations_with_replacement('ABC', 1)))
    # Larger r
    print('combinations_wr_3', list(itertools.combinations_with_replacement('AB', 3)))
    # Empty iterable
    print('combinations_wr_empty', list(itertools.combinations_with_replacement([], 2)))
except Exception as e:
    print('SKIP_combinations_with_replacement', type(e).__name__, e)

# === compress ===
try:
    # Normal usage
    print('compress_normal', list(itertools.compress('ABCDEF', [1, 0, 1, 0, 1, 1])))
    # With booleans
    print('compress_bool', list(itertools.compress('ABC', [True, False, True])))
    # Shorter selectors
    print('compress_short_selectors', list(itertools.compress('ABCDEF', [1, 0, 1])))
    # Empty data
    print('compress_empty_data', list(itertools.compress('', [1, 0])))
    # Empty selectors
    print('compress_empty_selectors', list(itertools.compress('ABC', [])))
    # Both empty
    print('compress_both_empty', list(itertools.compress([], [])))
except Exception as e:
    print('SKIP_compress', type(e).__name__, e)

# === count ===
try:
    # Take first 5 from count()
    print('count_default', list(itertools.islice(itertools.count(), 5)))
    # With start
    print('count_start', list(itertools.islice(itertools.count(10), 5)))
    # With start and step
    print('count_step', list(itertools.islice(itertools.count(10, 2), 5)))
    # Negative step
    print('count_neg_step', list(itertools.islice(itertools.count(10, -1), 5)))
    # Float step
    print('count_float_step', list(itertools.islice(itertools.count(0, 0.5), 5)))
    # Zero step (infinite same value)
    print('count_zero_step', list(itertools.islice(itertools.count(5, 0), 3)))
except Exception as e:
    print('SKIP_count', type(e).__name__, e)

# === cycle ===
try:
    # Normal usage
    print('cycle_str', list(itertools.islice(itertools.cycle('AB'), 5)))
    # List input
    print('cycle_list', list(itertools.islice(itertools.cycle([1, 2, 3]), 7)))
    # Single element
    print('cycle_single', list(itertools.islice(itertools.cycle('A'), 3)))
    # Empty iterable
    print('cycle_empty', list(itertools.cycle([])))
except Exception as e:
    print('SKIP_cycle', type(e).__name__, e)

# === dropwhile ===
try:
    # Normal usage
    print('dropwhile_normal', list(itertools.dropwhile(lambda x: x < 5, [1, 4, 6, 3, 8])))
    # Predicate never true
    print('dropwhile_never', list(itertools.dropwhile(lambda x: x < 0, [1, 4, 6])))
    # Predicate always true
    print('dropwhile_always', list(itertools.dropwhile(lambda x: x < 10, [1, 4, 6])))
    # Empty iterable
    print('dropwhile_empty', list(itertools.dropwhile(lambda x: x < 5, [])))
except Exception as e:
    print('SKIP_dropwhile', type(e).__name__, e)

# === filterfalse ===
try:
    # Normal usage
    print('filterfalse_normal', list(itertools.filterfalse(lambda x: x < 5, [1, 4, 6, 3, 8])))
    # None as predicate (filters falsy values)
    print('filterfalse_none', list(itertools.filterfalse(None, [0, 1, '', 'a', None, True, False])))
    # Empty iterable
    print('filterfalse_empty', list(itertools.filterfalse(lambda x: x < 5, [])))
    # All pass
    print('filterfalse_all_pass', list(itertools.filterfalse(lambda x: x < 0, [1, 2, 3])))
    # None fail
    print('filterfalse_none_pass', list(itertools.filterfalse(lambda x: x > 0, [1, 2, 3])))
except Exception as e:
    print('SKIP_filterfalse', type(e).__name__, e)

# === groupby ===
try:
    # Normal usage - must sort first for typical grouping
    data = 'AAAABBBCCDAABBB'
    print('groupby_normal', [(k, list(g)) for k, g in itertools.groupby(data)])
    # With key function
    data = ['apple', 'apricot', 'banana', 'cherry', 'avocado']
    print('groupby_key', [(k, list(g)) for k, g in itertools.groupby(data, key=lambda x: x[0])])
    # Empty iterable
    print('groupby_empty', list(itertools.groupby([])))
    # Single group
    print('groupby_single', [(k, list(g)) for k, g in itertools.groupby('AAA')])
    # All unique
    print('groupby_unique', [(k, list(g)) for k, g in itertools.groupby('ABC')])
except Exception as e:
    print('SKIP_groupby', type(e).__name__, e)

# === islice ===
try:
    # Normal usage - stop only
    print('islice_stop', list(itertools.islice('ABCDEFG', 3)))
    # Start and stop
    print('islice_start_stop', list(itertools.islice('ABCDEFG', 2, 5)))
    # Start, stop, step
    print('islice_step', list(itertools.islice('ABCDEFG', 0, 6, 2)))
    # Only start (to end)
    print('islice_start_only', list(itertools.islice('ABCDEFG', 3, None)))
    # Step only (from start)
    print('islice_step_only', list(itertools.islice('ABCDEFG', None, None, 2)))
    # Stop beyond iterable
    print('islice_beyond', list(itertools.islice('ABC', 10)))
    # Empty result
    print('islice_empty', list(itertools.islice('ABC', 5, 2)))
    # Empty iterable
    print('islice_empty_iter', list(itertools.islice([], 3)))
except Exception as e:
    print('SKIP_islice', type(e).__name__, e)

# === pairwise ===
try:
    # Normal usage
    print('pairwise_normal', list(itertools.pairwise('ABCDEFG')))
    # List input
    print('pairwise_list', list(itertools.pairwise([1, 2, 3, 4])))
    # Two elements
    print('pairwise_two', list(itertools.pairwise('AB')))
    # Single element
    print('pairwise_single', list(itertools.pairwise('A')))
    # Empty iterable
    print('pairwise_empty', list(itertools.pairwise([])))
except Exception as e:
    print('SKIP_pairwise', type(e).__name__, e)

# === permutations ===
try:
    # Normal usage
    print('permutations_2', list(itertools.permutations('ABCD', 2)))
    # Default r (len(iterable))
    print('permutations_default', list(itertools.permutations('ABC')))
    # r = 0
    print('permutations_r0', list(itertools.permutations('ABC', 0)))
    # r = 1
    print('permutations_r1', list(itertools.permutations('ABC', 1)))
    # r > len(iterable)
    print('permutations_r_gt_n', list(itertools.permutations('ABC', 5)))
    # Empty iterable
    print('permutations_empty', list(itertools.permutations([], 2)))
    # List input
    print('permutations_list', list(itertools.permutations([1, 2, 3], 2)))
except Exception as e:
    print('SKIP_permutations', type(e).__name__, e)

# === product ===
try:
    # Normal usage
    print('product_2', list(itertools.product('AB', 'CD')))
    # Three iterables
    print('product_3', list(itertools.product('AB', 'C', 'DE')))
    # Single iterable
    print('product_1', list(itertools.product('AB')))
    # No iterables
    print('product_0', list(itertools.product()))
    # With repeat
    print('product_repeat', list(itertools.product('AB', repeat=2)))
    # Repeat 3
    print('product_repeat3', list(itertools.product('AB', repeat=3)))
    # Empty iterable
    print('product_empty', list(itertools.product('AB', '')))
except Exception as e:
    print('SKIP_product', type(e).__name__, e)

# === repeat ===
try:
    # Normal usage
    print('repeat_normal', list(itertools.repeat('A', 5)))
    # Zero times
    print('repeat_zero', list(itertools.repeat('A', 0)))
    # Single repeat
    print('repeat_one', list(itertools.repeat('A', 1)))
    # With number
    print('repeat_num', list(itertools.repeat(42, 3)))
    # With list
    print('repeat_list', list(itertools.repeat([1, 2], 2)))
except Exception as e:
    print('SKIP_repeat', type(e).__name__, e)

# === starmap ===
try:
    # Normal usage
    print('starmap_normal', list(itertools.starmap(pow, [(2, 5), (3, 2), (10, 3)])))
    # Single arg functions
    print('starmap_sum', list(itertools.starmap(lambda x, y: x + y, [(1, 2), (3, 4)])))
    # Three args
    print('starmap_3args', list(itertools.starmap(lambda x, y, z: x + y + z, [(1, 2, 3), (4, 5, 6)])))
    # Empty iterable
    print('starmap_empty', list(itertools.starmap(pow, [])))
except Exception as e:
    print('SKIP_starmap', type(e).__name__, e)

# === takewhile ===
try:
    # Normal usage
    print('takewhile_normal', list(itertools.takewhile(lambda x: x < 5, [1, 4, 6, 3, 8])))
    # Predicate never true
    print('takewhile_never', list(itertools.takewhile(lambda x: x < 0, [1, 4, 6])))
    # Predicate always true
    print('takewhile_always', list(itertools.takewhile(lambda x: x < 10, [1, 4, 6])))
    # Empty iterable
    print('takewhile_empty', list(itertools.takewhile(lambda x: x < 5, [])))
except Exception as e:
    print('SKIP_takewhile', type(e).__name__, e)

# === tee ===
try:
    # Normal usage - returns n independent iterators
    it1, it2 = itertools.tee('ABC', 2)
    print('tee_2_first', list(it1))
    print('tee_2_second', list(it2))
    # Three tees
    it1, it2, it3 = itertools.tee([1, 2, 3], 3)
    print('tee_3_all', [list(it1), list(it2), list(it3)])
    # Single tee
    it1, = itertools.tee('ABC', 1)
    print('tee_1', list(it1))
    # Zero tees
    print('tee_0', list(itertools.tee('ABC', 0)))
    # Empty iterable
    it1, it2 = itertools.tee([], 2)
    print('tee_empty', [list(it1), list(it2)])
except Exception as e:
    print('SKIP_tee', type(e).__name__, e)

# === zip_longest ===
try:
    # Normal usage
    print('zip_longest_normal', list(itertools.zip_longest('ABCD', 'xy')))
    # With fillvalue
    print('zip_longest_fill', list(itertools.zip_longest('ABCD', 'xy', fillvalue='-')))
    # Three iterables
    print('zip_longest_3', list(itertools.zip_longest('AB', 'XYZ', '1234')))
    # Equal length
    print('zip_longest_equal', list(itertools.zip_longest('ABC', '123')))
    # First shorter
    print('zip_longest_first_short', list(itertools.zip_longest('AB', 'WXYZ')))
    # Single iterable
    print('zip_longest_1', list(itertools.zip_longest('ABC')))
    # Empty iterables
    print('zip_longest_empty', list(itertools.zip_longest('', '')))
except Exception as e:
    print('SKIP_zip_longest', type(e).__name__, e)
