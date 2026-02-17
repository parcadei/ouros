import functools

# === reduce ===
try:
    # Basic usage with lambda
    print('reduce_sum', functools.reduce(lambda x, y: x + y, [1, 2, 3, 4]))

    # With initial value
    print('reduce_sum_initial', functools.reduce(lambda x, y: x + y, [1, 2, 3, 4], 10))

    # Empty iterable with initial
    print('reduce_empty_with_initial', functools.reduce(lambda x, y: x + y, [], 0))

    # String concatenation
    print('reduce_concat', functools.reduce(lambda x, y: x + y, ['a', 'b', 'c']))

    # Multiplication
    print('reduce_product', functools.reduce(lambda x, y: x * y, [1, 2, 3, 4, 5]))

    # Find max
    print('reduce_max', functools.reduce(lambda x, y: x if x > y else y, [3, 1, 4, 1, 5, 9, 2, 6]))

    # Single element
    print('reduce_single', functools.reduce(lambda x, y: x + y, [42]))
except Exception as e:
    print('SKIP_reduce', type(e).__name__, e)

# === partial ===
try:
    # Basic partial application
    basetwo = functools.partial(int, base=2)
    print('partial_basetwo', basetwo('1010'))

    # Multiple args frozen
    add_five = functools.partial(lambda x, y, z: x + y + z, 5, 10)
    print('partial_add_five', add_five(3))

    # Access attributes
    print('partial_func', functools.partial(int, base=2).func)
    print('partial_args', functools.partial(int, '1010', base=2).args)
    print('partial_keywords', functools.partial(int, base=2).keywords)

    # Overriding keywords
    print('partial_override', basetwo('FF', base=16))
except Exception as e:
    print('SKIP_partial', type(e).__name__, e)

# === partialmethod ===
try:
    class MyClass:
        def method(self, a, b, c):
            return a + b + c

        partial_method = functools.partialmethod(method, 10, 20)

    obj = MyClass()
    print('partialmethod_result', obj.partial_method(30))

    # With keyword args
    class MyClass2:
        def method(self, a, b, c=0):
            return a + b + c

        partial_method = functools.partialmethod(method, c=100)

    obj2 = MyClass2()
    print('partialmethod_kwarg', obj2.partial_method(5, 10))
except Exception as e:
    print('SKIP_partialmethod', type(e).__name__, e)

# === lru_cache ===
try:
    # Basic usage
    @functools.lru_cache(maxsize=128)
    def fibonacci(n):
        if n < 2:
            return n
        return fibonacci(n - 1) + fibonacci(n - 2)

    fib_result = fibonacci(10)
    print('lru_fib_10', fib_result)

    # Check cache_info
    info = fibonacci.cache_info()
    print('lru_cache_hits', info.hits)
    print('lru_cache_misses', info.misses)
    print('lru_cache_maxsize', info.maxsize)
    print('lru_cache_currsize', info.currsize)

    # Clear cache
    fibonacci.cache_clear()
    print('lru_after_clear', fibonacci.cache_info().currsize)

    # maxsize=None (unbounded)
    @functools.lru_cache(maxsize=None)
    def unbounded_cache(x):
        return x * x

    print('unbounded_5', unbounded_cache(5))
    print('unbounded_maxsize', unbounded_cache.cache_info().maxsize)

    # typed=True
    @functools.lru_cache(maxsize=128, typed=True)
    def typed_cache(x):
        return str(x)

    print('typed_int', typed_cache(3))
    print('typed_float', typed_cache(3.0))
    print('typed_cache_info_hits', typed_cache.cache_info().hits)

    # cache_parameters
    params = fibonacci.cache_parameters()
    print('lru_params_maxsize', params['maxsize'])
    print('lru_params_typed', params['typed'])

    # __wrapped__
    @functools.lru_cache(maxsize=128)
    def original_func(x):
        return x * 2

    print('lru_wrapped', original_func.__wrapped__(5))

    # Direct decorator usage (no parentheses)
    @functools.lru_cache
    def simple_lru(x):
        return x + 1

    print('lru_direct', simple_lru(10))
except Exception as e:
    print('SKIP_lru_cache', type(e).__name__, e)

# === cache ===
try:
    # Simple cache decorator
    @functools.cache
    def factorial(n):
        if n == 0:
            return 1
        return n * factorial(n - 1)

    print('cache_factorial_5', factorial(5))
    print('cache_factorial_again', factorial(5))  # Should hit cache

    info = factorial.cache_info()
    print('cache_info_hits', info.hits)
    print('cache_info_misses', info.misses)

    factorial.cache_clear()
    print('cache_cleared', factorial.cache_info().currsize)
except Exception as e:
    print('SKIP_cache', type(e).__name__, e)

# === cached_property ===
try:
    class DataSet:
        def __init__(self, sequence):
            self._data = tuple(sequence)

        @functools.cached_property
        def total(self):
            return sum(self._data)

        @functools.cached_property
        def count(self):
            return len(self._data)

    ds = DataSet([1, 2, 3, 4, 5])
    print('cached_prop_total', ds.total)
    print('cached_prop_count', ds.count)

    # Check it's cached (accessing twice should not recompute)
    total1 = ds.total
    total2 = ds.total
    print('cached_same_value', total1 == total2)

    # Can delete to clear cache
    ds2 = DataSet([10, 20])
    first = ds2.total
    del ds2.total
    # After delete, accessing recompute (but we can't see that directly)
    print('cached_after_del', ds2.total == 30)
except Exception as e:
    print('SKIP_cached_property', type(e).__name__, e)

# === total_ordering ===
try:
    @functools.total_ordering
    class Person:
        def __init__(self, name, age):
            self.name = name
            self.age = age

        def __eq__(self, other):
            if not isinstance(other, Person):
                return NotImplemented
            return self.age == other.age

        def __lt__(self, other):
            if not isinstance(other, Person):
                return NotImplemented
            return self.age < other.age

    p1 = Person('Alice', 30)
    p2 = Person('Bob', 25)
    p3 = Person('Charlie', 30)

    print('to_eq', p1 == p3)
    print('to_ne', p1 != p2)
    print('to_lt', p2 < p1)
    print('to_le', p2 <= p1)
    print('to_le_eq', p1 <= p3)
    print('to_gt', p1 > p2)
    print('to_ge', p1 >= p2)
    print('to_ge_eq', p1 >= p3)
except Exception as e:
    print('SKIP_total_ordering', type(e).__name__, e)

# === wraps ===
try:
    # Basic usage
    def my_decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            return func(*args, **kwargs)
        return wrapper

    @my_decorator
    def example_func():
        """This is the docstring."""
        return 42

    print('wraps_name', example_func.__name__)
    print('wraps_doc', example_func.__doc__)
except Exception as e:
    print('SKIP_wraps', type(e).__name__, e)

# === WRAPPER_ASSIGNMENTS ===
try:
    print('wrapper_assignments', functools.WRAPPER_ASSIGNMENTS)
except Exception as e:
    print('SKIP_WRAPPER_ASSIGNMENTS', type(e).__name__, e)

# === WRAPPER_UPDATES ===
try:
    print('wrapper_updates', functools.WRAPPER_UPDATES)
except Exception as e:
    print('SKIP_WRAPPER_UPDATES', type(e).__name__, e)

# === update_wrapper ===
try:
    def original_function():
        """Original docstring."""
        return 'original'

    def wrapper_function():
        """Wrapper docstring."""
        return 'wrapper'

    updated = functools.update_wrapper(wrapper_function, original_function)
    print('update_wrapper_name', updated.__name__)
    print('update_wrapper_doc', updated.__doc__)
    print('update_wrapper_wrapped', updated.__wrapped__ is original_function)

    # Custom assigned and updated
    assigned = ('__name__',)
    updated_tuple = ()

    def custom_wrapper():
        return 'custom'

    result = functools.update_wrapper(custom_wrapper, original_function, assigned=assigned, updated=updated_tuple)
    print('update_wrapper_custom', result.__name__)
except Exception as e:
    print('SKIP_update_wrapper', type(e).__name__, e)

# === singledispatch ===
try:
    @functools.singledispatch
    def process(arg):
        return f'default: {type(arg).__name__}'

    @process.register(int)
    def _(arg):
        return f'int: {arg}'

    @process.register(str)
    def _(arg):
        return f'str: {arg}'

    @process.register(list)
    def _(arg):
        return f'list with {len(arg)} items'

    print('singledispatch_int', process(42))
    print('singledispatch_str', process('hello'))
    print('singledispatch_list', process([1, 2, 3]))
    print('singledispatch_default', process(3.14))

    # registry
    print('singledispatch_registry_keys', list(process.registry.keys()))

    # dispatch
    print('singledispatch_dispatch_int', process.dispatch(int) is not None)
except Exception as e:
    print('SKIP_singledispatch', type(e).__name__, e)

# === singledispatchmethod ===
try:
    class MyProcessor:
        @functools.singledispatchmethod
        def process(self, arg):
            return f'default: {type(arg).__name__}'

        @process.register(int)
        def _(self, arg):
            return f'int: {arg}'

        @process.register(str)
        def _(self, arg):
            return f'str: {arg.upper()}'

    processor = MyProcessor()
    print('singledispatchmethod_int', processor.process(100))
    print('singledispatchmethod_str', processor.process('test'))
    print('singledispatchmethod_default', processor.process(3.14))
except Exception as e:
    print('SKIP_singledispatchmethod', type(e).__name__, e)

# === cmp_to_key ===
try:
    # Old-style comparison function
    def compare_length(a, b):
        return len(a) - len(b)

    words = ['python', 'is', 'a', 'programming', 'language']
    sorted_words = sorted(words, key=functools.cmp_to_key(compare_length))
    print('cmp_to_key_sorted', sorted_words)

    # Using with max
    def compare_numeric(a, b):
        return (a > b) - (a < b)  # Returns -1, 0, or 1

    max_val = max([3, 1, 4, 1, 5], key=functools.cmp_to_key(compare_numeric))
    print('cmp_to_key_max', max_val)

    # Reverse comparison
    def reverse_compare(a, b):
        if a < b:
            return 1
        elif a > b:
            return -1
        return 0

    sorted_desc = sorted([1, 5, 2, 4, 3], key=functools.cmp_to_key(reverse_compare))
    print('cmp_to_key_desc', sorted_desc)
except Exception as e:
    print('SKIP_cmp_to_key', type(e).__name__, e)

# === Placeholder ===
try:
    # Placeholder is used with partial for placeholder arguments
    print('placeholder_type', type(functools.Placeholder).__name__)
except Exception as e:
    print('SKIP_Placeholder', type(e).__name__, e)

# === get_cache_token ===
try:
    # Used with singledispatch for mutable types
    token = functools.get_cache_token()
    print('cache_token_type', type(token).__name__)
except Exception as e:
    print('SKIP_get_cache_token', type(e).__name__, e)

# === recursive_repr ===
try:
    # Used for __repr__ methods to handle recursive structures
    class Node:
        def __init__(self, value):
            self.value = value
            self.children = []

        @functools.recursive_repr()
        def __repr__(self):
            return f'Node({self.value!r}, children={self.children!r})'

    root = Node(1)
    child = Node(2)
    root.children.append(child)
    repr_result = repr(root)
    print('recursive_repr', 'Node(1' in repr_result)
except Exception as e:
    print('SKIP_recursive_repr', type(e).__name__, e)

# === Additional edge cases for reduce ===
try:
    # With tuple/list as elements
    tuples = [(1, 2), (3, 4), (5, 6)]
    print('reduce_tuples', functools.reduce(lambda x, y: (x[0] + y[0], x[1] + y[1]), tuples))

    # Nested reduce
    nested = functools.reduce(lambda x, y: x + y, functools.reduce(lambda x, y: x + [y], [[1, 2], [3, 4], [5]], []))
    print('reduce_nested', nested)
except Exception as e:
    print('SKIP_Additional edge cases for reduce', type(e).__name__, e)

# === lru_cache LRU eviction test ===
try:
    @functools.lru_cache(maxsize=2)
    def limited_cache(x):
        return x * x

    limited_cache(1)
    limited_cache(2)
    limited_cache(3)  # This should evict 1
    info = limited_cache.cache_info()
    print('lru_eviction_currsize', info.currsize)
    print('lru_eviction_maxsize', info.maxsize)
except Exception as e:
    print('SKIP_lru_cache LRU eviction test', type(e).__name__, e)

# === wraps with custom arguments ===
try:
    def custom_decorator_with_wraps(func):
        @functools.wraps(func, assigned=('__name__',), updated=())
        def wrapper(*args, **kwargs):
            return func(*args, **kwargs)
        return wrapper

    @custom_decorator_with_wraps
    def func_with_custom_wraps():
        """Doc should not be copied."""
        return 1

    print('custom_wraps_name', func_with_custom_wraps.__name__)
    print('custom_wraps_doc', func_with_custom_wraps.__doc__ is None)
except Exception as e:
    print('SKIP_wraps with custom arguments', type(e).__name__, e)

# === partial bound to instance method ===
try:
    class Calculator:
        def add(self, a, b, c):
            return a + b + c

    calc = Calculator()
    add_partial = functools.partial(calc.add, 10)
    print('partial_instance_method', add_partial(20, 30))
except Exception as e:
    print('SKIP_partial bound to instance method', type(e).__name__, e)

# === cached_property with slots class (edge case) ===
try:
    # cached_property requires __dict__, so this tests that behavior
    class WithSlots:
        __slots__ = ('value',)

        def __init__(self, value):
            self.value = value

    try:
        # This won't work with cached_property since no __dict__
        class BadClass:
            __slots__ = ('x',)

            @functools.cached_property
            def prop(self):
                return 1

        bad = BadClass()
        _ = bad.prop
        print('cached_prop_slots', 'unexpected_success')
    except (AttributeError, TypeError) as e:
        print('cached_prop_slots_error', True)
except Exception as e:
    print('SKIP_cached_property with slots class (edge case)', type(e).__name__, e)
