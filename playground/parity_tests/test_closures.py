# === Basic closure ===
try:
    # Simple nested function capturing a variable
    def outer_basic(x):
        def inner():
            return x
        return inner

    f = outer_basic(10)
    print('closure_basic', f())
except Exception as e:
    print('SKIP_Basic closure', type(e).__name__, e)

# === Closure with multiple captures ===
try:
    def outer_multiple(a, b):
        def inner():
            return a + b
        return inner

    f_multi = outer_multiple(5, 7)
    print('closure_multiple_captures', f_multi())
except Exception as e:
    print('SKIP_Closure with multiple captures', type(e).__name__, e)

# === Closure modifying captured variable via nonlocal ===
try:
    def make_counter():
        count = 0
        def counter():
            nonlocal count
            count += 1
            return count
        return counter

    c = make_counter()
    print('closure_nonlocal_1', c())
    print('closure_nonlocal_2', c())
    print('closure_nonlocal_3', c())
except Exception as e:
    print('SKIP_Closure modifying captured variable via nonlocal', type(e).__name__, e)

# === Nonlocal with multiple scopes ===
try:
    def level_one():
        x = 'level_one'
        def level_two():
            x = 'level_two'  # shadows level_one's x
            def level_three():
                nonlocal x
                x = 'modified_level_two'
                return x
            return level_three
        return level_two

    f_levels = level_one()()
    print('closure_nonlocal_multi_scope', f_levels())
except Exception as e:
    print('SKIP_Nonlocal with multiple scopes', type(e).__name__, e)

# === __closure__ attribute introspection ===
try:
    def make_closure(val):
        def inner():
            return val
        return inner

    closure_func = make_closure(42)
    print('closure_has_attr', hasattr(closure_func, '__closure__'))
    print('closure_not_none', closure_func.__closure__ is not None)
    print('closure_cell_count', len(closure_func.__closure__))
    print('closure_cell_contents', closure_func.__closure__[0].cell_contents)
except Exception as e:
    print('SKIP___closure__ attribute introspection', type(e).__name__, e)

# === __closure__ with multiple cells ===
try:
    def make_multi_closure(a, b, c):
        def inner():
            return a + b + c
        return inner

    multi_closure = make_multi_closure(1, 2, 3)
    print('closure_multi_cell_count', len(multi_closure.__closure__))
    print('closure_cell_0', multi_closure.__closure__[0].cell_contents)
    print('closure_cell_1', multi_closure.__closure__[1].cell_contents)
    print('closure_cell_2', multi_closure.__closure__[2].cell_contents)
except Exception as e:
    print('SKIP___closure__ with multiple cells', type(e).__name__, e)

# === __code__.co_freevars ===
try:
    print('closure_co_freevars', closure_func.__code__.co_freevars)
    print('closure_multi_freevars', multi_closure.__code__.co_freevars)
except Exception as e:
    print('SKIP___code__.co_freevars', type(e).__name__, e)

# === Late binding closure (classic gotcha) ===
try:
    def make_late_binders():
        funcs = []
        for i in range(3):
            def binder():
                return i  # i is looked up at call time, not definition time
            funcs.append(binder)
        return funcs

    late_funcs = make_late_binders()
    print('late_binding_0', late_funcs[0]())
    print('late_binding_1', late_funcs[1]())
    print('late_binding_2', late_funcs[2]())
except Exception as e:
    print('SKIP_Late binding closure (classic gotcha)', type(e).__name__, e)

# === Fixed late binding with default argument ===
try:
    def make_early_binders():
        funcs = []
        for i in range(3):
            def binder(i=i):  # default arg captures current value
                return i
            funcs.append(binder)
        return funcs

    early_funcs = make_early_binders()
    print('early_binding_0', early_funcs[0]())
    print('early_binding_1', early_funcs[1]())
    print('early_binding_2', early_funcs[2]())
except Exception as e:
    print('SKIP_Fixed late binding with default argument', type(e).__name__, e)

# === Factory function - power function ===
try:
    def make_power(n):
        def power(x):
            return x ** n
        return power

    square = make_power(2)
    cube = make_power(3)
    print('factory_square_3', square(3))
    print('factory_cube_2', cube(2))
except Exception as e:
    print('SKIP_Factory function - power function', type(e).__name__, e)

# === Factory function - multiplier ===
try:
    def make_multiplier(factor):
        def multiply(x):
            return x * factor
        return multiply
    double = make_multiplier(2)
    triple = make_multiplier(3)
    print('factory_double_5', double(5))
    print('factory_triple_5', triple(5))
except Exception as e:
    print('SKIP_Factory function - multiplier', type(e).__name__, e)

# === Closure capturing mutable object ===
try:
    def make_accumulator():
        items = []
        def accumulator(item):
            items.append(item)
            return items
        return accumulator

    acc = make_accumulator()
    print('closure_mutable_1', acc('a'))
    print('closure_mutable_2', acc('b'))
    print('closure_mutable_3', acc('c'))
except Exception as e:
    print('SKIP_Closure capturing mutable object', type(e).__name__, e)

# === Closure with mutable and nonlocal ===
try:
    def make_mutable_counter():
        data = {'count': 0, 'name': 'counter'}
        def counter():
            nonlocal data
            data['count'] += 1
            return data['count']
        def reset():
            nonlocal data
            data['count'] = 0
        def get_data():
            return data.copy()
        return counter, reset, get_data

    cnt, rst, get = make_mutable_counter()
    print('mutable_counter_1', cnt())
    print('mutable_counter_2', cnt())
    print('mutable_counter_get', get())
    rst()
    print('mutable_counter_after_reset', cnt())
except Exception as e:
    print('SKIP_Closure with mutable and nonlocal', type(e).__name__, e)

# === Deeply nested closures ===
try:
    def level_a(a):
        def level_b(b):
            def level_c(c):
                def level_d(d):
                    return a + b + c + d
                return level_d
            return level_c
        return level_b

    deep = level_a(1)(2)(3)
    print('deep_closure', deep(4))
except Exception as e:
    print('SKIP_Deeply nested closures', type(e).__name__, e)

# === Closure scope chain ===
try:
    def outer_scope(x):
        def middle_scope(y):
            z = y * 2
            def inner_scope():
                return x + y + z
            return inner_scope
        return middle_scope

    scope_test = outer_scope(10)(5)
    print('scope_chain', scope_test())
except Exception as e:
    print('SKIP_Closure scope chain', type(e).__name__, e)

# === Closure with no free variables ===
try:
    def make_no_capture():
        x = 100
        def inner():
            return 42  # doesn't use x
        return inner

    no_cap = make_no_capture()
    print('closure_no_capture', no_cap())
    print('closure_no_capture_closure_attr', no_cap.__closure__)
except Exception as e:
    print('SKIP_Closure with no free variables', type(e).__name__, e)

# === Closure with only some args captured ===
try:
    def partial_capture(x, y):
        z = x * 2  # not captured
        def inner():
            return x + y  # only x and y captured
        return inner

    partial = partial_capture(5, 3)
    print('closure_partial_capture', partial())
    print('closure_partial_freevars', partial.__code__.co_freevars)
except Exception as e:
    print('SKIP_Closure with only some args captured', type(e).__name__, e)

# === Decorator-style closure ===
try:
    def with_prefix(prefix):
        def decorator(func):
            def wrapper(name):
                return prefix + func(name)
            return wrapper
        return decorator

    hello_decorator = with_prefix('Hello, ')
    def greet(name):
        return name + '!'
    greeter = hello_decorator(greet)
    print('decorator_closure', greeter('World'))
except Exception as e:
    print('SKIP_Decorator-style closure', type(e).__name__, e)

# === Closure identity and redefinition ===
try:
    def make_closures_same_scope():
        x = 1
        def a():
            return x
        def b():
            return x
        return a, b

    f1, f2 = make_closures_same_scope()
    print('same_scope_closure_1', f1())
    print('same_scope_closure_2', f2())
    print('share_closure_cells', f1.__closure__[0] is f2.__closure__[0])
except Exception as e:
    print('SKIP_Closure identity and redefinition', type(e).__name__, e)

# === Closure with class ===
try:
    def make_class_closure():
        count = 0
        class CounterClass:
            def increment(self):
                nonlocal count
                count += 1
                return count
        return CounterClass

    Cls = make_class_closure()
    instance = Cls()
    print('class_closure_1', instance.increment())
    print('class_closure_2', instance.increment())
except Exception as e:
    print('SKIP_Closure with class', type(e).__name__, e)

# === Lambda as closure ===
try:
    def make_lambda_closure(x):
        return lambda: x * 2

    lam = make_lambda_closure(21)
    print('lambda_closure', lam())
    print('lambda_closure_has_closure', lam.__closure__ is not None)
except Exception as e:
    print('SKIP_Lambda as closure', type(e).__name__, e)

# === Closure with comprehension variable capture ===
try:
    def make_comprehension_closures():
        return [(lambda: i) for i in range(3)]

    comp_closures = make_comprehension_closures()
    print('comprehension_closure_0', comp_closures[0]())
    print('comprehension_closure_1', comp_closures[1]())
    print('comprehension_closure_2', comp_closures[2]())
except Exception as e:
    print('SKIP_Closure with comprehension variable capture', type(e).__name__, e)

# === Closure reassignment of function ===
try:
    def make_reassignable():
        x = 1
        def inner():
            return x
        return inner

    reassign = make_reassignable()
    original = reassign
    reassign = make_reassignable()
    print('reassign_different', original is not reassign)
    print('reassign_same_cells', original.__closure__[0] is reassign.__closure__[0])
    print('reassign_values', original(), reassign())
except Exception as e:
    print('SKIP_Closure reassignment of function', type(e).__name__, e)

# === Closure with exception handling ===
try:
    def make_safe_divider(divisor):
        def divide(x):
            try:
                return x / divisor
            except ZeroDivisionError:
                return float('inf')
        return divide

    div_by_2 = make_safe_divider(2)
    div_by_0 = make_safe_divider(0)
    print('safe_divide_10_by_2', div_by_2(10))
    print('safe_divide_10_by_0', div_by_0(10))
except Exception as e:
    print('SKIP_Closure with exception handling', type(e).__name__, e)

# === Closure __closure__ is read-only tuple ===
try:
    def check_closure_immutable(x):
        def inner():
            return x
        return inner

    immutable_check = check_closure_immutable(5)
    try:
        immutable_check.__closure__[0] = None
    except TypeError as e:
        print('closure_tuple_immutable', str(e)[:50])
except Exception as e:
    print('SKIP_Closure __closure__ is read-only tuple', type(e).__name__, e)

# === Closure with keyword arguments ===
try:
    def make_kwarg_closure(default_val):
        def inner(x=None):
            if x is None:
                x = default_val
            return x * 2
        return inner

    kwarg_closure = make_kwarg_closure(100)
    print('kwarg_closure_default', kwarg_closure())
    print('kwarg_closure_override', kwarg_closure(50))
except Exception as e:
    print('SKIP_Closure with keyword arguments', type(e).__name__, e)

# === Closure with *args and **kwargs ===
try:
    def make_varargs_closure(base):
        def inner(*args, **kwargs):
            return base + sum(args) + sum(kwargs.values())
        return inner

    varargs = make_varargs_closure(10)
    print('varargs_closure', varargs(1, 2, 3, a=4, b=5))
except Exception as e:
    print('SKIP_Closure with *args and **kwargs', type(e).__name__, e)

# === Nested closure returning closures ===
try:
    def outer_factory(multiplier):
        def inner_factory(addend):
            def final(x):
                return x * multiplier + addend
            return final
        return inner_factory

    times_3_plus_5 = outer_factory(3)(5)
    print('nested_factory', times_3_plus_5(10))
except Exception as e:
    print('SKIP_Nested closure returning closures', type(e).__name__, e)

# === Closure with conditional capture ===
try:
    def make_conditional(x, condition):
        if condition:
            y = x * 2
            def inner():
                return y
        else:
            z = x * 3
            def inner():
                return z
        return inner

    cond_true = make_conditional(10, True)
    cond_false = make_conditional(10, False)
    print('conditional_true', cond_true())
    print('conditional_false', cond_false())
except Exception as e:
    print('SKIP_Closure with conditional capture', type(e).__name__, e)

# === Closure cell identity ===
try:
    def cell_identity_test():
        a = 1
        b = 2
        def f1():
            return a
        def f2():
            return a, b
        def f3():
            return b
        return f1, f2, f3

    f1, f2, f3 = cell_identity_test()
    print('cell_identity_a_f1_f2', f1.__closure__[0] is f2.__closure__[0])
    print('cell_identity_b_f2_f3', f2.__closure__[1] is f3.__closure__[0])
    print('cell_identity_not_shared', f1.__closure__[0] is not f3.__closure__[0])
except Exception as e:
    print('SKIP_Closure cell identity', type(e).__name__, e)

# === Closure with generator ===
try:
    def make_gen_closure():
        start = 0
        def gen():
            nonlocal start
            while start < 3:
                yield start
                start += 1
        return gen

    g = make_gen_closure()
    gen_instance = g()
    print('gen_closure', list(gen_instance))
except Exception as e:
    print('SKIP_Closure with generator', type(e).__name__, e)

# === Closure with async function (syntax only) ===
try:
    def make_async_closure(val):
        async def async_func():
            return val * 2
        return async_func

    async_closure = make_async_closure(21)
    print('async_closure_name', async_closure.__name__)
    print('async_closure_has_closure', async_closure.__closure__ is not None)
    print('async_closure_contents', async_closure.__closure__[0].cell_contents)
except Exception as e:
    print('SKIP_Closure with async function (syntax only)', type(e).__name__, e)

# === Empty closure tuple for non-closure nested function ===
try:
    def not_a_closure():
        def inner():
            return 42  # no free variables
        return inner

    non_closure = not_a_closure()
    print('non_closure_cell', non_closure.__closure__)
except Exception as e:
    print('SKIP_Empty closure tuple for non-closure nested function', type(e).__name__, e)

# === Closure with nested class capturing outer ===
try:
    def make_nested_class_closure(x):
        class InnerClass:
            def method(self):
                return x * 2
        return InnerClass

    Nested = make_nested_class_closure(25)
    print('nested_class_closure', Nested().method())
except Exception as e:
    print('SKIP_Closure with nested class capturing outer', type(e).__name__, e)

# === Closure with property ===
try:
    def make_property_closure():
        _value = 0
        
        class WithProperty:
            @property
            def value(self):
                return _value
            
            @value.setter
            def value(self, v):
                nonlocal _value
                _value = v
        
        return WithProperty

    PropClass = make_property_closure()
    prop_inst = PropClass()
    print('property_closure_get', prop_inst.value)
    prop_inst.value = 42
    print('property_closure_set', prop_inst.value)
except Exception as e:
    print('SKIP_Closure with property', type(e).__name__, e)
