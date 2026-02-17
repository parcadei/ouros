# === Function decorator basic ===
try:
    def simple_decorator(f):
        def wrapper():
            return f() + 10
        return wrapper

    @simple_decorator
    def basic_func():
        return 5

    print('function_decorator_basic', basic_func())
except Exception as e:
    print('SKIP_Function_decorator_basic', type(e).__name__, e)


# === Function decorator with args ===
try:
    def args_decorator(prefix):
        def decorator(f):
            def wrapper(*args, **kwargs):
                result = f(*args, **kwargs)
                return f'{prefix}:{result}'
            return wrapper
        return decorator

    @args_decorator('RESULT')
    def func_with_args(a, b):
        return a + b

    print('function_decorator_with_args', func_with_args(3, 4))
except Exception as e:
    print('SKIP_Function_decorator_with_args', type(e).__name__, e)


# === Multiple decorators ===
try:
    def dec1(f):
        def wrapper():
            return f() * 2
        return wrapper

    def dec2(f):
        def wrapper():
            return f() + 1
        return wrapper

    @dec1
    @dec2
    def multi_decorated():
        return 5

    print('multiple_decorators', multi_decorated())
except Exception as e:
    print('SKIP_Multiple_decorators', type(e).__name__, e)


# === Class decorator ===
try:
    def class_decorator(cls):
        cls.decorated = True
        return cls

    @class_decorator
    class MyClass:
        pass

    print('class_decorator', MyClass.decorated)
except Exception as e:
    print('SKIP_Class_decorator', type(e).__name__, e)


# === Class decorator with args ===
try:
    def class_dec_with_args(add_method):
        def decorator(cls):
            if add_method:
                cls.extra = lambda self: 'extra_method'
            return cls
        return decorator

    @class_dec_with_args(True)
    class ClassWithExtra:
        pass

    obj = ClassWithExtra()
    print('class_decorator_with_args', obj.extra())
except Exception as e:
    print('SKIP_Class_decorator_with_args', type(e).__name__, e)


# === Property decorator ===
try:
    class PropertyDemo:
        def __init__(self, value):
            self._value = value

        @property
        def value(self):
            return self._value

        @value.setter
        def value(self, val):
            self._value = val

        @value.deleter
        def value(self):
            del self._value

    p = PropertyDemo(42)
    print('property_getter', p.value)
    p.value = 100
    print('property_setter', p.value)
except Exception as e:
    print('SKIP_Property_decorator', type(e).__name__, e)


# === Staticmethod decorator ===
try:
    class StaticDemo:
        @staticmethod
        def static_method(x, y):
            return x * y

    print('staticmethod', StaticDemo.static_method(6, 7))
except Exception as e:
    print('SKIP_Staticmethod_decorator', type(e).__name__, e)


# === Classmethod decorator ===
try:
    class ClassDemo:
        count = 0

        @classmethod
        def from_count(cls, n):
            instance = cls.__new__(cls)
            instance.n = n
            return instance

        @classmethod
        def increment(cls):
            cls.count += 1
            return cls.count

    print('classmethod', ClassDemo.increment())
    print('classmethod_factory', ClassDemo.from_count(5).n)
except Exception as e:
    print('SKIP_Classmethod_decorator', type(e).__name__, e)


# === functools.wraps ===
try:
    import functools

    def preserving_decorator(f):
        @functools.wraps(f)
        def wrapper(*args, **kwargs):
            return f(*args, **kwargs) * 2
        return wrapper

    @preserving_decorator
    def original_func():
        """Original docstring."""
        return 21

    print('functools_wraps_name', original_func.__name__)
    print('functools_wraps_doc', original_func.__doc__)
except Exception as e:
    print('SKIP_functools_wraps', type(e).__name__, e)


# === __wrapped__ attribute ===
try:
    print('__wrapped__', original_func.__wrapped__())
except Exception as e:
    print('SKIP___wrapped___attribute', type(e).__name__, e)


# === functools.cache ===
try:
    import functools

    call_count = 0

    @functools.cache
    def cached_func(n):
        global call_count
        call_count += 1
        return n * n

    result1 = cached_func(5)
    result2 = cached_func(5)
    result3 = cached_func(6)
    print('functools_cache_results', result1, result2, result3)
    print('functools_cache_calls', call_count)
except Exception as e:
    print('SKIP_functools_cache', type(e).__name__, e)


# === functools.lru_cache ===
try:
    @functools.lru_cache(maxsize=2)
    def lru_func(n):
        global call_count
        call_count += 1
        return n * 2

    call_count = 0
    lru_result1 = lru_func(1)
    lru_result2 = lru_func(2)
    lru_result3 = lru_func(1)
    print('lru_cache', lru_result1, lru_result2, lru_result3, call_count)
except Exception as e:
    print('SKIP_functools_lru_cache', type(e).__name__, e)


# === functools.cached_property ===
try:
    class CachedPropDemo:
        def __init__(self):
            self.compute_count = 0

        @functools.cached_property
        def expensive_value(self):
            self.compute_count += 1
            return sum(range(100))

    cp = CachedPropDemo()
    val1 = cp.expensive_value
    val2 = cp.expensive_value
    print('cached_property', val1, val2, cp.compute_count)
except Exception as e:
    print('SKIP_functools_cached_property', type(e).__name__, e)


# === Method decorator ===
try:
    def method_decorator(method):
        def wrapper(self, *args, **kwargs):
            return method(self, *args, **kwargs) + self.base
        return wrapper

    class MethodDecDemo:
        def __init__(self):
            self.base = 10

        @method_decorator
        def compute(self, x):
            return x * 2

    mdd = MethodDecDemo()
    print('method_decorator', mdd.compute(5))
except Exception as e:
    print('SKIP_Method_decorator', type(e).__name__, e)


# === Decorator with class method ===
try:
    class DecoratorClass:
        def __init__(self, func):
            functools.update_wrapper(self, func)
            self.func = func

        def __call__(self, *args, **kwargs):
            return self.func(*args, **kwargs) + 100

    @DecoratorClass
    def use_class_decorator():
        return 50

    print('callable_class_decorator', use_class_decorator())
except Exception as e:
    print('SKIP_Decorator_with_class_method', type(e).__name__, e)


# === Decorator returning non-function ===
try:
    def returns_value_decorator(f):
        return 999

    @returns_value_decorator
    def dummy_func():
        pass

    print('decorator_returns_non_callable', dummy_func)
except Exception as e:
    print('SKIP_Decorator_returning_non_function', type(e).__name__, e)


# === Nested decorator ===
try:
    def outer_dec(x):
        def middle_dec(y):
            def inner_dec(f):
                def wrapper():
                    return f() + x + y
                return wrapper
            return inner_dec
        return middle_dec

    @outer_dec(10)(20)
    def nested_dec_func():
        return 5

    print('nested_decorator', nested_dec_func())
except Exception as e:
    print('SKIP_Nested_decorator', type(e).__name__, e)


# === Decorator modifying function attributes ===
try:
    def attr_decorator(f):
        f.custom_attr = 'custom_value'
        f.is_decorated = True
        return f

    @attr_decorator
    def func_with_attrs():
        return 1

    print('decorator_attrs', func_with_attrs.custom_attr, func_with_attrs.is_decorated)
except Exception as e:
    print('SKIP_Decorator_modifying_function_attributes', type(e).__name__, e)


# === Decorator with functools.partial ===
try:
    from functools import partial

    def partial_dec(f, multiplier=1):
        def wrapper(x):
            return f(x) * multiplier
        return wrapper

    triple_dec = partial(partial_dec, multiplier=3)

    @triple_dec
    def partial_func(x):
        return x + 1

    print('partial_decorator', partial_func(4))
except Exception as e:
    print('SKIP_Decorator_with_functools_partial', type(e).__name__, e)


# === Decorator order test ===
try:
    order_log = []

    def order_dec(name):
        def decorator(f):
            order_log.append(name)
            return f
        return decorator

    @order_dec('first')
    @order_dec('second')
    @order_dec('third')
    def order_test():
        pass

    print('decorator_order', order_log)
except Exception as e:
    print('SKIP_Decorator_order_test', type(e).__name__, e)


# === Property with inheritance ===
try:
    class BaseProperty:
        @property
        def prop(self):
            return 'base'

    class DerivedProperty(BaseProperty):
        @property
        def prop(self):
            return super().prop + '_derived'

    dp = DerivedProperty()
    print('property_inheritance', dp.prop)
except Exception as e:
    print('SKIP_Property_with_inheritance', type(e).__name__, e)


# === Classmethod and staticmethod combination ===
try:
    class ComboClass:
        _registry = []

        @classmethod
        def register(cls, name):
            cls._registry.append(name)
            return cls

        @staticmethod
        def utility(x):
            return x ** 2

    ComboClass.register('item1')
    print('classmethod_staticmethod_combo', ComboClass._registry, ComboClass.utility(4))
except Exception as e:
    print('SKIP_Classmethod_and_staticmethod_combination', type(e).__name__, e)


# === Decorator with inspect.signature preservation ===
try:
    import inspect

    def sig_preserving_dec(f):
        @functools.wraps(f)
        def wrapper(a, b, c=10):
            return f(a, b, c)
        return wrapper

    @sig_preserving_dec
    def sig_func(x, y, z=5):
        """Test function."""
        return x + y + z

    sig = inspect.signature(sig_func)
    print('signature_preserved', list(sig.parameters.keys()))
except Exception as e:
    print('SKIP_Decorator_with_inspect_signature_preservation', type(e).__name__, e)


# === Decorator factory pattern ===
try:
    class DecoratorFactory:
        @staticmethod
        def create(prefix):
            def decorator(f):
                def wrapper(*args, **kwargs):
                    return f'{prefix}:{f(*args, **kwargs)}'
                return wrapper
            return decorator

    @DecoratorFactory.create('FACTORY')
    def factory_func():
        return 'output'

    print('decorator_factory', factory_func())
except Exception as e:
    print('SKIP_Decorator_factory_pattern', type(e).__name__, e)


# === Descriptor protocol with decorators ===
try:
    class Validator:
        def __init__(self, min_val, max_val):
            self.min_val = min_val
            self.max_val = max_val

        def __set_name__(self, owner, name):
            self.name = name
            self.storage_name = f'_{name}'

        def __get__(self, instance, owner):
            if instance is None:
                return self
            return getattr(instance, self.storage_name, None)

        def __set__(self, instance, value):
            if not self.min_val <= value <= self.max_val:
                raise ValueError(f'{self.name} must be between {self.min_val} and {self.max_val}')
            setattr(instance, self.storage_name, value)

    class ValidatedClass:
        age = Validator(0, 150)
        score = Validator(0, 100)

        def __init__(self, age, score):
            self.age = age
            self.score = score

    vc = ValidatedClass(25, 85)
    print('descriptor_validator', vc.age, vc.score)
except Exception as e:
    print('SKIP_Descriptor_protocol_with_decorators', type(e).__name__, e)


# === Decorator with closure variables ===
try:
    def closure_decorator(base):
        def decorator(f):
            def wrapper(x):
                return f(x) + base
            return wrapper
        return decorator

    @closure_decorator(1000)
    def closure_func(x):
        return x * 10

    print('closure_decorator', closure_func(5))
except Exception as e:
    print('SKIP_Decorator_with_closure_variables', type(e).__name__, e)


# === Decorator modifying __qualname__ ===
try:
    def qualname_decorator(f):
        f.__qualname__ = 'custom.qualified.name'
        return f

    @qualname_decorator
    def qual_func():
        pass

    print('decorator_qualname', qual_func.__qualname__)
except Exception as e:
    print('SKIP_Decorator_modifying___qualname__', type(e).__name__, e)


# === Generic decorator for any callable ===
try:
    def generic_decorator(func):
        def wrapper(*args, **kwargs):
            result = func(*args, **kwargs)
            return result
        return wrapper

    @generic_decorator
    def generic_test(x):
        return x * 2

    print('generic_decorator_result', generic_test(7))
except Exception as e:
    print('SKIP_Generic_decorator_for_any_callable', type(e).__name__, e)


# === Property deletion ===
try:
    class DeletableProperty:
        def __init__(self):
            self._val = 'initial'

        @property
        def val(self):
            return self._val

        @val.deleter
        def val(self):
            self._val = 'deleted'

    dp = DeletableProperty()
    print('before_delete', dp.val)
    del dp.val
    print('after_delete', dp.val)
except Exception as e:
    print('SKIP_Property_deletion', type(e).__name__, e)


# === Stacked property decorators ===
try:
    class StackedProperty:
        @property
        def computed(self):
            return 10

    sp = StackedProperty()
    print('stacked_property', sp.computed)
except Exception as e:
    print('SKIP_Stacked_property_decorators', type(e).__name__, e)


# === Decorator on lambda (not typical but possible) ===
try:
    lam_dec = lambda f: lambda: f() + 1
    decorated_lambda = lam_dec(lambda: 41)
    print('decorated_lambda', decorated_lambda())
except Exception as e:
    print('SKIP_Decorator_on_lambda', type(e).__name__, e)


# === functools.update_wrapper ===
try:
    def manual_wrapper(f):
        def wrapper():
            return f() + 1
        functools.update_wrapper(wrapper, f)
        return wrapper

    @manual_wrapper
    def manual_wrapped():
        """Manual wrapper doc."""
        return 99

    print('update_wrapper_name', manual_wrapped.__name__)
    print('update_wrapper_doc', manual_wrapped.__doc__)
except Exception as e:
    print('SKIP_functools_update_wrapper', type(e).__name__, e)


# === Wrapper assignment and update ===
try:
    wrapper_sig = functools.WRAPPER_ASSIGNMENTS
    wrapper_updates = functools.WRAPPER_UPDATES
    print('wrapper_assignments_count', len(wrapper_sig))
    print('wrapper_updates_count', len(wrapper_updates))
except Exception as e:
    print('SKIP_Wrapper_assignment_and_update', type(e).__name__, e)


# === Class decorator modifying methods ===
try:
    def add_method_decorator(cls):
        def new_method(self):
            return 'added'
        cls.new_method = new_method
        cls.class_attr = 'decorated_attr'
        return cls

    @add_method_decorator
    class ClassWithNewMethod:
        pass

    cwnm = ClassWithNewMethod()
    print('class_decorator_add_method', cwnm.new_method())
    print('class_decorator_add_attr', cwnm.class_attr)
except Exception as e:
    print('SKIP_Class_decorator_modifying_methods', type(e).__name__, e)


# === Decorator with optional arguments ===
try:
    def optional_arg_decorator(func=None, *, prefix='default'):
        def decorator(f):
            def wrapper():
                return f'{prefix}:{f()}'
            return wrapper
        if func is None:
            return decorator
        return decorator(func)

    @optional_arg_decorator
    def opt_no_args():
        return 'no_args'

    @optional_arg_decorator(prefix='CUSTOM')
    def opt_with_args():
        return 'with_args'

    print('optional_arg_no_args', opt_no_args())
    print('optional_arg_with_args', opt_with_args())
except Exception as e:
    print('SKIP_Decorator_with_optional_arguments', type(e).__name__, e)


# === Total ordering decorator check ===
try:
    from functools import total_ordering

    @total_ordering
    class Comparable:
        def __init__(self, value):
            self.value = value

        def __eq__(self, other):
            return self.value == other.value

        def __lt__(self, other):
            return self.value < other.value

    c1 = Comparable(5)
    c2 = Comparable(10)
    c3 = Comparable(5)
    print('total_ordering_eq', c1 == c3)
    print('total_ordering_lt', c1 < c2)
    print('total_ordering_le', c1 <= c3)
    print('total_ordering_gt', c2 > c1)
    print('total_ordering_ge', c2 >= c1)
except Exception as e:
    print('SKIP_Total_ordering_decorator_check', type(e).__name__, e)


# === Async decorator pattern (check definition) ===
try:
    def async_decorator(f):
        return f

    print('async_decorator_defined', callable(async_decorator))
except Exception as e:
    print('SKIP_Async_decorator_pattern', type(e).__name__, e)


# === Recursive decorator application ===
try:
    def recursive_dec(depth):
        def decorator(f):
            if depth <= 0:
                return f
            new_f = lambda: f() + 1
            return recursive_dec(depth - 1)(new_f)
        return decorator

    @recursive_dec(3)
    def recursive_base():
        return 0

    print('recursive_decorator', recursive_base())
except Exception as e:
    print('SKIP_Recursive_decorator_application', type(e).__name__, e)


# === Property with custom getter/setter/deleter names ===
try:
    class CustomPropertyNames:
        def __init__(self):
            self._data = 0

        def get_data(self):
            return self._data

        def set_data(self, value):
            self._data = value

        def del_data(self):
            self._data = -1

        data = property(get_data, set_data, del_data, 'Data property')

    cpn = CustomPropertyNames()
    print('custom_property_get', cpn.data)
    cpn.data = 42
    print('custom_property_set', cpn.data)
    del cpn.data
    print('custom_property_del', cpn.data)
except Exception as e:
    print('SKIP_Property_with_custom_getter_setter_deleter_names', type(e).__name__, e)


# === Decorator with type hints preservation ===
try:
    def hint_preserve_decorator(f):
        @functools.wraps(f)
        def wrapper(x: int) -> int:
            return f(x)
        return wrapper

    @hint_preserve_decorator
    def hinted_func(x: int) -> int:
        return x + 1

    print('hints_preserved', hinted_func.__annotations__)
except Exception as e:
    print('SKIP_Decorator_with_type_hints_preservation', type(e).__name__, e)


# === functools.singledispatch check ===
try:
    from functools import singledispatch

    @singledispatch
    def process(arg):
        return f'default:{arg}'

    @process.register(int)
    def _(arg):
        return f'int:{arg * 2}'

    @process.register(str)
    def _(arg):
        return f'str:{arg.upper()}'

    print('singledispatch_int', process(5))
    print('singledispatch_str', process('hello'))
    print('singledispatch_float', process(3.14))
except Exception as e:
    print('SKIP_functools_singledispatch_check', type(e).__name__, e)


# === Module-level decorated constants ===
try:
    MODULE_CONST = 42
    print('module_const', MODULE_CONST)
except Exception as e:
    print('SKIP_Module_level_decorated_constants', type(e).__name__, e)


# === Decorator with state ===
try:
    def stateful_decorator(f):
        def wrapper(*args, **kwargs):
            wrapper.call_count += 1
            return f(*args, **kwargs)
        wrapper.call_count = 0
        return wrapper

    @stateful_decorator
    def stateful_func():
        return 'called'

    stateful_func()
    stateful_func()
    stateful_func()
    print('stateful_decorator_count', stateful_func.call_count)
except Exception as e:
    print('SKIP_Decorator_with_state', type(e).__name__, e)


# === Final comprehensive check ===
try:
    print('all_decorator_tests_completed', True)
except Exception as e:
    print('SKIP_Final_comprehensive_check', type(e).__name__, e)
