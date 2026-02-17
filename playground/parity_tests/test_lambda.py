# === Basic lambda ===
try:
    # Simple lambda with single argument
    square = lambda x: x**2
    print('lambda_basic', square(5))
except Exception as e:
    print('SKIP_Basic lambda', type(e).__name__, e)

# === Lambda no arguments ===
try:
    # Lambda with no parameters
    no_args = lambda: 42
    print('lambda_no_args', no_args())
except Exception as e:
    print('SKIP_Lambda no arguments', type(e).__name__, e)

# === Lambda multiple arguments ===
try:
    # Lambda with two or more positional arguments
    add = lambda x, y: x + y
    print('lambda_two_args', add(3, 4))
    multiply = lambda a, b, c: a * b * c
    print('lambda_three_args', multiply(2, 3, 4))
except Exception as e:
    print('SKIP_Lambda multiple arguments', type(e).__name__, e)

# === Lambda default arguments ===
try:
    # Lambda with default parameter values
    greet = lambda name, greeting='Hello': f'{greeting}, {name}!'
    print('lambda_default_arg', greet('World'))
    print('lambda_default_override', greet('World', 'Hi'))

    # Lambda with all defaults
    power = lambda base, exp=2: base ** exp
    print('lambda_default_square', power(3))
    print('lambda_default_cube', power(2, 3))
except Exception as e:
    print('SKIP_Lambda default arguments', type(e).__name__, e)

# === Lambda keyword arguments ===
try:
    # Lambda using keyword argument calling
    divide = lambda a, b: a / b
    print('lambda_kwarg_call', divide(a=10, b=2))

    # Lambda with mixed positional and keyword
    calc = lambda x, y, z: x + y * z
    print('lambda_mixed_call', calc(1, z=3, y=2))
except Exception as e:
    print('SKIP_Lambda keyword arguments', type(e).__name__, e)

# === Lambda *args ===
try:
    # Lambda accepting variable positional arguments
    sum_all = lambda *args: sum(args)
    print('lambda_args_empty', sum_all())
    print('lambda_args_single', sum_all(1))
    print('lambda_args_multiple', sum_all(1, 2, 3, 4))

    # Lambda with positional and *args
    with_args = lambda x, *rest: x + sum(rest)
    print('lambda_pos_and_args', with_args(10, 1, 2, 3))
except Exception as e:
    print('SKIP_Lambda *args', type(e).__name__, e)

# === Lambda **kwargs ===
try:
    # Lambda accepting variable keyword arguments
    make_dict = lambda **kwargs: kwargs
    print('lambda_kwargs', make_dict(a=1, b=2))

    # Lambda with positional and **kwargs
    with_kwargs = lambda x, **kwargs: (x, kwargs)
    print('lambda_pos_and_kwargs', with_kwargs(5, y=10, z=20))
except Exception as e:
    print('SKIP_Lambda **kwargs', type(e).__name__, e)

# === Lambda *args and **kwargs ===
try:
    # Lambda with both variable arguments
    all_args = lambda *args, **kwargs: (args, kwargs)
    print('lambda_all_args', all_args(1, 2, x=3, y=4))

    # Full signature lambda
    full_sig = lambda a, b=2, *args, **kwargs: (a, b, args, kwargs)
    print('lambda_full_signature', full_sig(1, 3, 4, 5, x=6))
except Exception as e:
    print('SKIP_Lambda *args and **kwargs', type(e).__name__, e)

# === Lambda in map ===
try:
    # Using lambda with map function
    numbers = [1, 2, 3, 4, 5]
    squared = list(map(lambda x: x**2, numbers))
    print('lambda_map', squared)

    # Map with multiple iterables
    sums = list(map(lambda x, y: x + y, [1, 2, 3], [4, 5, 6]))
    print('lambda_map_multi', sums)
except Exception as e:
    print('SKIP_Lambda in map', type(e).__name__, e)

# === Lambda in filter ===
try:
    # Using lambda with filter function
    numbers = [1, 2, 3, 4, 5]
    evens = list(filter(lambda x: x % 2 == 0, numbers))
    print('lambda_filter', evens)

    # Filter with complex condition
    strings = ['hello', '', 'world', '', 'python']
    non_empty = list(filter(lambda s: len(s) > 0, strings))
    print('lambda_filter_strings', non_empty)
except Exception as e:
    print('SKIP_Lambda in filter', type(e).__name__, e)

# === Lambda in sorted ===
try:
    # Using lambda as key function
    words = ['cherry', 'apple', 'banana']
    sorted_by_len = sorted(words, key=lambda s: len(s))
    print('lambda_sorted_key', sorted_by_len)

    # Sort by last character
    sorted_by_last = sorted(words, key=lambda s: s[-1])
    print('lambda_sorted_last', sorted_by_last)

    # Sort with reverse
    numbers = [1, 2, 3, 4, 5]
    sorted_desc = sorted(numbers, key=lambda x: x, reverse=True)
    print('lambda_sorted_reverse', sorted_desc)
except Exception as e:
    print('SKIP_Lambda in sorted', type(e).__name__, e)

# === Lambda in reduce ===
try:
    # Using lambda with functools.reduce
    from functools import reduce
    product = reduce(lambda x, y: x * y, [1, 2, 3, 4, 5])
    print('lambda_reduce', product)

    # Find max using reduce
    max_val = reduce(lambda x, y: x if x > y else y, [3, 1, 4, 1, 5])
    print('lambda_reduce_max', max_val)
except Exception as e:
    print('SKIP_Lambda in reduce', type(e).__name__, e)

# === Lambda closure ===
try:
    # Lambda capturing enclosing scope variable
    def make_multiplier(n):
        return lambda x: x * n

    double = make_multiplier(2)
    triple = make_multiplier(3)
    print('lambda_closure_double', double(5))
    print('lambda_closure_triple', triple(5))

    # Multiple captures
    def make_adder(a, b):
        return lambda x: x + a + b

    add_five = make_adder(2, 3)
    print('lambda_closure_multi', add_five(10))
except Exception as e:
    print('SKIP_Lambda closure', type(e).__name__, e)

# === Lambda closure late binding ===
try:
    # Demonstrating late binding behavior with lambdas in loops
    late_binders = [(lambda: i) for i in range(3)]
    print('lambda_late_binding_all', [f() for f in late_binders])

    # Fixed with default argument
    early_binders = [(lambda i=i: i) for i in range(3)]
    print('lambda_early_binding_all', [f() for f in early_binders])
except Exception as e:
    print('SKIP_Lambda closure late binding', type(e).__name__, e)

# === Lambda conditional expression ===
try:
    # Lambda with ternary conditional
    sign = lambda x: 'positive' if x > 0 else 'negative' if x < 0 else 'zero'
    print('lambda_conditional_pos', sign(5))
    print('lambda_conditional_neg', sign(-3))
    print('lambda_conditional_zero', sign(0))

    # Lambda with conditional in calculation
    abs_lambda = lambda x: x if x >= 0 else -x
    print('lambda_abs_pos', abs_lambda(5))
    print('lambda_abs_neg', abs_lambda(-5))
except Exception as e:
    print('SKIP_Lambda conditional expression', type(e).__name__, e)

# === Lambda returning lambda ===
try:
    # Higher-order lambda that returns another lambda
    make_power = lambda n: (lambda x: x**n)
    square_fn = make_power(2)
    cube_fn = make_power(3)
    print('lambda_returns_lambda_square', square_fn(4))
    print('lambda_returns_lambda_cube', cube_fn(2))

    # Nested lambda return
    make_adder_lambda = lambda a: lambda b: a + b
    add_ten = make_adder_lambda(10)
    print('lambda_nested_add', add_ten(5))

    # Triple nested
    make_multiplier_chain = lambda x: lambda y: lambda z: x * y * z
    print('lambda_triple_nested', make_multiplier_chain(2)(3)(4))
except Exception as e:
    print('SKIP_Lambda returning lambda', type(e).__name__, e)

# === Immediately invoked lambda expression ===
try:
    # IIFE - Immediately Invoked Function Expression
    result = (lambda x: x * 2)(21)
    print('lambda_iife_single', result)

    # IIFE with multiple args
    sum_result = (lambda x, y, z: x + y + z)(1, 2, 3)
    print('lambda_iife_multi', sum_result)

    # IIFE with default
    with_default = (lambda x, y=10: x + y)(5)
    print('lambda_iife_default', with_default)

    # IIFE returning lambda
    created_lambda = (lambda x: lambda y: x + y)(100)
    print('lambda_iife_returns', created_lambda(50))
except Exception as e:
    print('SKIP_Immediately invoked lambda expression', type(e).__name__, e)

# === Lambda as dict key function ===
try:
    # Using lambda with max(), min() by key
    pairs = [(1, 'one'), (2, 'two'), (3, 'three')]
    max_by_num = max(pairs, key=lambda p: p[0])
    print('lambda_max_key', max_by_num)

    min_by_len = min(pairs, key=lambda p: len(p[1]))
    print('lambda_min_key', min_by_len)
except Exception as e:
    print('SKIP_Lambda as dict key function', type(e).__name__, e)

# === Lambda in list comprehension ===
try:
    # Lambdas in comprehensions
    funcs = [(lambda x: x * i) for i in range(1, 4)]
    results = [f(5) for f in funcs]
    print('lambda_in_comp', results)
except Exception as e:
    print('SKIP_Lambda in list comprehension', type(e).__name__, e)

# === Lambda in dictionary ===
try:
    # Storing lambdas in dict
    ops = {
        'add': lambda x, y: x + y,
        'sub': lambda x, y: x - y,
        'mul': lambda x, y: x * y,
        'div': lambda x, y: x / y if y != 0 else 'error'
    }
    print('lambda_dict_add', ops['add'](10, 5))
    print('lambda_dict_div', ops['div'](10, 2))
    print('lambda_dict_div_zero', ops['div'](10, 0))
except Exception as e:
    print('SKIP_Lambda in dictionary', type(e).__name__, e)

# === Lambda in list operations ===
try:
    # Using lambda for custom sorting of complex data
    data = [
        {'name': 'Alice', 'age': 30},
        {'name': 'Bob', 'age': 25},
        {'name': 'Charlie', 'age': 35}
    ]
    sorted_by_age = sorted(data, key=lambda d: d['age'])
    print('lambda_sort_dict', [d['name'] for d in sorted_by_age])

    # Extract with lambda
    names = list(map(lambda d: d['name'], data))
    print('lambda_extract', names)
except Exception as e:
    print('SKIP_Lambda in list operations', type(e).__name__, e)

# === Lambda with any/all ===
try:
    # Using lambda for predicate functions
    values = [2, 4, 6, 8]
    all_even = all(map(lambda x: x % 2 == 0, values))
    print('lambda_all_even', all_even)

    any_gt_five = any(map(lambda x: x > 5, values))
    print('lambda_any_gt_five', any_gt_five)
except Exception as e:
    print('SKIP_Lambda with any/all', type(e).__name__, e)

# === Lambda identity and attributes ===
try:
    # Lambda function attributes
    my_lambda = lambda x: x + 1
    print('lambda_name', my_lambda.__name__)
    print('lambda_callable', callable(my_lambda))

    # Lambda code object attributes
    code = my_lambda.__code__
    print('lambda_code_name', code.co_name)
    print('lambda_code_argcount', code.co_argcount)
    print('lambda_code_varnames', code.co_varnames)
except Exception as e:
    print('SKIP_Lambda identity and attributes', type(e).__name__, e)

# === Lambda with None check ===
try:
    # Lambda handling None values
    maybe_double = lambda x: None if x is None else x * 2
    print('lambda_none_check', maybe_double(5))
    print('lambda_none_result', maybe_double(None))
except Exception as e:
    print('SKIP_Lambda with None check', type(e).__name__, e)

# === Lambda with boolean operations ===
try:
    # Lambda using and/or
    bool_check = lambda x: x > 0 and 'positive' or 'non-positive'
    print('lambda_bool_pos', bool_check(5))
    print('lambda_bool_neg', bool_check(-1))
except Exception as e:
    print('SKIP_Lambda with boolean operations', type(e).__name__, e)

# === Lambda with unpacking ===
try:
    # Lambda with tuple/list unpacking
    sum_tuple = lambda t: t[0] + t[1]
    print('lambda_sum_tuple', sum_tuple((3, 4)))

    # Lambda unpacking in map
    pairs = [(1, 2), (3, 4), (5, 6)]
    sums = list(map(lambda t: t[0] + t[1], pairs))
    print('lambda_map_unpack', sums)
except Exception as e:
    print('SKIP_Lambda with unpacking', type(e).__name__, e)

# === Lambda recursion via fixed-point combinator ===
try:
    # Y combinator for lambda recursion
    Y = lambda f: (lambda x: f(lambda v: x(x)(v)))(lambda x: f(lambda v: x(x)(v)))
    factorial_gen = lambda f: lambda n: 1 if n == 0 else n * f(n - 1)
    factorial = Y(factorial_gen)
    print('lambda_y_combinator_fact', factorial(5))
except Exception as e:
    print('SKIP_Lambda recursion via fixed-point combinator', type(e).__name__, e)

# === Lambda default mutable argument ===
try:
    # Lambda with mutable default (classic gotcha)
    mutable_default = lambda lst=[]: lst.append(1) or lst
    print('lambda_mutable_default_1', mutable_default())
    print('lambda_mutable_default_2', mutable_default())
    print('lambda_mutable_default_3', mutable_default())
except Exception as e:
    print('SKIP_Lambda default mutable argument', type(e).__name__, e)

# === Lambda with complex expressions ===
try:
    # Lambda with method calls
    strip_upper = lambda s: s.strip().upper()
    print('lambda_method_chain', strip_upper('  hello  '))

    # Lambda with multiple operations
    compute = lambda x: (x ** 2 + 2 * x + 1) / (x + 1) if x != -1 else 0
    print('lambda_complex_expr', compute(4))
except Exception as e:
    print('SKIP_Lambda with complex expressions', type(e).__name__, e)

# === Lambda as key for groupby ===
try:
    from itertools import groupby
    words = ['apple', 'apricot', 'banana', 'blueberry', 'cherry']
    grouped = {k: list(v) for k, v in groupby(sorted(words), key=lambda x: x[0])}
    print('lambda_groupby_keys', list(grouped.keys()))
    print('lambda_groupby_a_count', len(grouped['a']))
except Exception as e:
    print('SKIP_Lambda as key for groupby', type(e).__name__, e)

# === Lambda partial application workaround ===
try:
    # Using default args for partial application
    partial_add = (lambda x: lambda y, x=x: x + y)(10)
    print('lambda_partial', partial_add(5))
except Exception as e:
    print('SKIP_Lambda partial application workaround', type(e).__name__, e)

# === Lambda with walrus operator (assignment expression) ===
try:
    # Using := inside lambda
    accumulate = lambda n: [(last := 0)] + [last := last + i for i in range(1, n + 1)]
    print('lambda_walrus_accumulate', accumulate(5))

    # Lambda with walrus for caching
    fib_cache = {}
    fib_lambda = lambda n: fib_cache[n] if n in fib_cache else (fib_cache.__setitem__(n, n if n < 2 else fib_lambda(n-1) + fib_lambda(n-2)) or fib_cache[n])
    print('lambda_fib_cached', fib_lambda(10))
except Exception as e:
    print('SKIP_Lambda with walrus operator (assignment expression)', type(e).__name__, e)

# === Lambda with getattr/hasattr ===
try:
    # Lambda for safe attribute access
    get_name = lambda obj: getattr(obj, 'name', 'unknown')
    class Thing:
        name = 'thing_name'
    print('lambda_getattr', get_name(Thing()))
    print('lambda_getattr_default', get_name(42))
except Exception as e:
    print('SKIP_Lambda with getattr/hasattr', type(e).__name__, e)

# === Lambda truthiness testing ===
try:
    # Lambda used for filtering by truthiness
    truthy = lambda x: bool(x)
    values = [0, 1, '', 'hello', [], [1, 2], None, True]
    truthy_values = list(filter(truthy, values))
    print('lambda_truthy_count', len(truthy_values))
except Exception as e:
    print('SKIP_Lambda truthiness testing', type(e).__name__, e)

# === Lambda in tuple unpacking ===
try:
    # Lambda returning multiple values
    split_divmod = lambda x, y: (x // y, x % y)
    quot, rem = split_divmod(17, 5)
    print('lambda_tuple_return_quot', quot)
    print('lambda_tuple_return_rem', rem)
except Exception as e:
    print('SKIP_Lambda in tuple unpacking', type(e).__name__, e)

# === Lambda with nested ternary ===
try:
    # Complex nested conditional
    categorize = lambda x: 'small' if x < 10 else ('medium' if x < 100 else ('large' if x < 1000 else 'huge'))
    print('lambda_nested_ternary_5', categorize(5))
    print('lambda_nested_ternary_50', categorize(50))
    print('lambda_nested_ternary_500', categorize(500))
    print('lambda_nested_ternary_5000', categorize(5000))
except Exception as e:
    print('SKIP_Lambda with nested ternary', type(e).__name__, e)

# === Lambda composition ===
try:
    from functools import reduce
    # Function composition with lambdas
    compose = lambda f, g: lambda x: f(g(x))
    add_one = lambda x: x + 1
    double_it = lambda x: x * 2
    composed = compose(double_it, add_one)
    print('lambda_compose', composed(5))

    # Multiple composition
    pipeline = lambda *fns: reduce(lambda f, g: lambda x: f(g(x)), fns)
    add_then_mul = pipeline(lambda x: x * 3, lambda x: x + 2)
    print('lambda_pipeline', add_then_mul(4))
except Exception as e:
    print('SKIP_Lambda composition', type(e).__name__, e)
