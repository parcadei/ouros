# === Basic class ===
try:
    class Point:
        def __init__(self, x):
            self.x = x

    p = Point(5)
    print('basic_class_attr', p.x)
except Exception as e:
    print('SKIP_Basic class', type(e).__name__, e)

# === Class with __new__ ===
try:
    class Singleton:
        _instance = None
        
        def __new__(cls):
            if cls._instance is None:
                cls._instance = super().__new__(cls)
            return cls._instance
        
        def __init__(self):
            self.value = 42

    s1 = Singleton()
    s2 = Singleton()
    print('singleton_same_instance', s1 is s2)
    print('singleton_value', s1.value)
except Exception as e:
    print('SKIP_Class with __new__', type(e).__name__, e)

# === Class with both __new__ and __init__ ===
try:
    class NewAndInit:
        def __new__(cls, value):
            instance = super().__new__(cls)
            instance.from_new = value * 2
            return instance
        
        def __init__(self, value):
            self.from_init = value

    ni = NewAndInit(10)
    print('new_and_init_new', ni.from_new)
    print('new_and_init_init', ni.from_init)
except Exception as e:
    print('SKIP_Class with both __new__ and __init__', type(e).__name__, e)

# === Instance methods ===
try:
    class Counter:
        def __init__(self):
            self.count = 0
        
        def increment(self):
            self.count += 1
            return self.count
        
        def add(self, n):
            self.count += n
            return self.count

    c = Counter()
    print('instance_method_1', c.increment())
    print('instance_method_2', c.increment())
    print('instance_method_add', c.add(5))
except Exception as e:
    print('SKIP_Instance methods', type(e).__name__, e)

# === Class methods ===
try:
    class ClassMethodDemo:
        class_attr = 100
        
        def __init__(self, value):
            self.value = value
        
        @classmethod
        def create_with_default(cls):
            return cls(42)
        
        @classmethod
        def get_class_attr(cls):
            return cls.class_attr

    cmd = ClassMethodDemo.create_with_default()
    print('classmethod_create', cmd.value)
    print('classmethod_get_attr', ClassMethodDemo.get_class_attr())
except Exception as e:
    print('SKIP_Class methods', type(e).__name__, e)

# === Static methods ===
try:
    class StaticMethodDemo:
        @staticmethod
        def add(a, b):
            return a + b
        
        @staticmethod
        def greet(name):
            return f'Hello, {name}'

    print('staticmethod_add', StaticMethodDemo.add(3, 4))
    print('staticmethod_greet', StaticMethodDemo.greet('World'))
except Exception as e:
    print('SKIP_Static methods', type(e).__name__, e)

# === Method types combined ===
try:
    class MethodTypes:
        attr = 'class_attr'
        
        def __init__(self, val):
            self.val = val
        
        def instance_method(self):
            return f'instance: {self.val}'
        
        @classmethod
        def class_method(cls):
            return f'class: {cls.attr}'
        
        @staticmethod
        def static_method():
            return 'static: no self'

    mt = MethodTypes('my_val')
    print('method_types_instance', mt.instance_method())
    print('method_types_class', mt.class_method())
    print('method_types_static', mt.static_method())
    print('method_types_static_cls', MethodTypes.static_method())
except Exception as e:
    print('SKIP_Method types combined', type(e).__name__, e)

# === Single inheritance ===
try:
    class Animal:
        def __init__(self, name):
            self.name = name
        
        def speak(self):
            return 'Some sound'
        
        def identify(self):
            return f'I am {self.name}'

    class Dog(Animal):
        def speak(self):
            return 'Woof!'

    d = Dog('Buddy')
    print('inherit_speak', d.speak())
    print('inherit_identify', d.identify())
    print('inherit_name', d.name)
    print('isinstance_dog_animal', isinstance(d, Animal))
    print('isinstance_dog_dog', isinstance(d, Dog))
except Exception as e:
    print('SKIP_Single inheritance', type(e).__name__, e)

# === Multiple inheritance ===
try:
    class Flyer:
        def fly(self):
            return 'Flying'

    class Swimmer:
        def swim(self):
            return 'Swimming'

    class Duck(Flyer, Swimmer):
        def quack(self):
            return 'Quack!'

    duck = Duck()
    print('multi_inherit_fly', duck.fly())
    print('multi_inherit_swim', duck.swim())
    print('multi_inherit_quack', duck.quack())
except Exception as e:
    print('SKIP_Multiple inheritance', type(e).__name__, e)

# === Diamond inheritance ===
try:
    class A:
        def method(self):
            return 'A'

    class B(A):
        def method(self):
            return 'B-' + super().method()

    class C(A):
        def method(self):
            return 'C-' + super().method()

    class D(B, C):
        def method(self):
            return 'D-' + super().method()

    d_inst = D()
    print('diamond_method', d_inst.method())
except Exception as e:
    print('SKIP_Diamond inheritance', type(e).__name__, e)

# === Method Resolution Order (MRO) ===
try:
    print('mro_d', [cls.__name__ for cls in D.__mro__])
    print('mro_duck', [cls.__name__ for cls in Duck.__mro__])
except Exception as e:
    print('SKIP_Method Resolution Order (MRO)', type(e).__name__, e)

# === super() with no arguments ===
try:
    class Base:
        def __init__(self):
            self.base_initialized = True
        
        def do_work(self):
            return 'Base work'

    class Derived(Base):
        def __init__(self):
            super().__init__()
            self.derived_initialized = True
        
        def do_work(self):
            return 'Derived-' + super().do_work()

    der = Derived()
    print('super_no_args_base', der.base_initialized)
    print('super_no_args_derived', der.derived_initialized)
    print('super_no_args_work', der.do_work())
except Exception as e:
    print('SKIP_super() with no arguments', type(e).__name__, e)

# === super() with explicit arguments ===
try:
    class SuperExplicit:
        def method(self):
            return 'SuperExplicit'

    class SubExplicit(SuperExplicit):
        def method(self):
            return super(SubExplicit, self).method()

    se = SubExplicit()
    print('super_explicit', se.method())
except Exception as e:
    print('SKIP_super() with explicit arguments', type(e).__name__, e)

# === super() in multiple inheritance ===
try:
    class MROBase:
        def __init__(self):
            self.base_val = 1

    class MROMiddle1(MROBase):
        def __init__(self):
            super().__init__()
            self.m1_val = 2

    class MROMiddle2(MROBase):
        def __init__(self):
            super().__init__()
            self.m2_val = 3

    class MROBottom(MROMiddle1, MROMiddle2):
        def __init__(self):
            super().__init__()
            self.bottom_val = 4

    mro = MROBottom()
    print('mro_base_val', mro.base_val)
    print('mro_m1_val', mro.m1_val)
    print('mro_bottom_val', mro.bottom_val)
except Exception as e:
    print('SKIP_super() in multiple inheritance', type(e).__name__, e)

# === Properties - basic getter ===
try:
    class Circle:
        def __init__(self, radius):
            self._radius = radius
        
        @property
        def radius(self):
            return self._radius
        
        @property
        def area(self):
            import math
            return math.pi * self._radius ** 2

    circ = Circle(5)
    print('property_getter_radius', circ.radius)
    print('property_getter_area', round(circ.area, 2))
except Exception as e:
    print('SKIP_Properties - basic getter', type(e).__name__, e)

# === Properties - getter and setter ===
try:
    class Temperature:
        def __init__(self, celsius):
            self._celsius = celsius
        
        @property
        def celsius(self):
            return self._celsius
        
        @celsius.setter
        def celsius(self, value):
            self._celsius = value
        
        @property
        def fahrenheit(self):
            return self._celsius * 9/5 + 32
        
        @fahrenheit.setter
        def fahrenheit(self, value):
            self._celsius = (value - 32) * 5/9

    temp = Temperature(0)
    print('temp_celsius_init', temp.celsius)
    temp.celsius = 100
    print('temp_celsius_set', temp.celsius)
    print('temp_fahrenheit', temp.fahrenheit)
    temp.fahrenheit = 212
    print('temp_fahrenheit_set_celsius', round(temp.celsius, 1))
except Exception as e:
    print('SKIP_Properties - getter and setter', type(e).__name__, e)

# === Properties - getter, setter, deleter ===
try:
    class ManagedAttribute:
        def __init__(self):
            self._data = None
        
        @property
        def data(self):
            if self._data is None:
                return 'No data'
            return self._data
        
        @data.setter
        def data(self, value):
            self._data = value
        
        @data.deleter
        def data(self):
            self._data = None

    ma = ManagedAttribute()
    print('prop_delete_init', ma.data)
    ma.data = 'hello'
    print('prop_delete_set', ma.data)
    del ma.data
    print('prop_delete_deleted', ma.data)
except Exception as e:
    print('SKIP_Properties - getter, setter, deleter', type(e).__name__, e)

# === Property with docstring ===
try:
    class DocumentedProperty:
        @property
        def documented(self):
            """This is a documented property."""
            return 42

    dp = DocumentedProperty()
    print('property_docstring', DocumentedProperty.documented.__doc__)
except Exception as e:
    print('SKIP_Property with docstring', type(e).__name__, e)

# === __str__ special method ===
try:
    class Person:
        def __init__(self, name):
            self.name = name
        
        def __str__(self):
            return f'Person: {self.name}'

    p = Person('Alice')
    print('str_method', str(p))
except Exception as e:
    print('SKIP___str__ special method', type(e).__name__, e)

# === __repr__ special method ===
try:
    class Vector:
        def __init__(self, x, y):
            self.x = x
            self.y = y
        
        def __repr__(self):
            return f'Vector({self.x!r}, {self.y!r})'

    v = Vector(1, 2)
    print('repr_method', repr(v))
except Exception as e:
    print('SKIP___repr__ special method', type(e).__name__, e)

# === __str__ and __repr__ together ===
try:
    class BothStrRepr:
        def __str__(self):
            return 'str_output'
        
        def __repr__(self):
            return 'repr_output'

    bsr = BothStrRepr()
    print('both_str', str(bsr))
    print('both_repr', repr(bsr))
except Exception as e:
    print('SKIP___str__ and __repr__ together', type(e).__name__, e)

# === __eq__ special method ===
try:
    class Equality:
        def __init__(self, value):
            self.value = value
        
        def __eq__(self, other):
            if isinstance(other, Equality):
                return self.value == other.value
            return NotImplemented

    e1 = Equality(5)
    e2 = Equality(5)
    e3 = Equality(10)
    print('eq_true', e1 == e2)
    print('eq_false', e1 == e3)
    print('eq_same', e1 == e1)
except Exception as e:
    print('SKIP___eq__ special method', type(e).__name__, e)

# === __hash__ special method ===
try:
    class HashableItem:
        def __init__(self, id):
            self.id = id
        
        def __eq__(self, other):
            if isinstance(other, HashableItem):
                return self.id == other.id
            return NotImplemented
        
        def __hash__(self):
            return hash(self.id)

    h1 = HashableItem(1)
    h2 = HashableItem(1)
    h3 = HashableItem(2)
    print('hash_equal', hash(h1) == hash(h2))
    print('hash_in_dict', h1 in {h2: 'found'})
    print('hash_set_len', len({h1, h2, h3}))
except Exception as e:
    print('SKIP___hash__ special method', type(e).__name__, e)

# === Comparison operators ===
try:
    class Comparable:
        def __init__(self, value):
            self.value = value
        
        def __eq__(self, other):
            if isinstance(other, Comparable):
                return self.value == other.value
            return NotImplemented
        
        def __lt__(self, other):
            if isinstance(other, Comparable):
                return self.value < other.value
            return NotImplemented
        
        def __le__(self, other):
            if isinstance(other, Comparable):
                return self.value <= other.value
            return NotImplemented
        
        def __gt__(self, other):
            if isinstance(other, Comparable):
                return self.value > other.value
            return NotImplemented
        
        def __ge__(self, other):
            if isinstance(other, Comparable):
                return self.value >= other.value
            return NotImplemented

    c1 = Comparable(5)
    c2 = Comparable(10)
    c3 = Comparable(5)
    print('comp_eq', c1 == c3)
    print('comp_lt', c1 < c2)
    print('comp_le', c1 <= c3)
    print('comp_gt', c2 > c1)
    print('comp_ge', c1 >= c3)
    print('comp_ne', c1 != c2)
except Exception as e:
    print('SKIP_Comparison operators', type(e).__name__, e)

# === __bool__ special method ===
try:
    class Truthy:
        def __init__(self, value):
            self.value = value
        
        def __bool__(self):
            return bool(self.value)

    t_true = Truthy(1)
    t_false = Truthy(0)
    t_empty = Truthy('')
    t_str = Truthy('hello')
    print('bool_true', bool(t_true))
    print('bool_false', bool(t_false))
    print('bool_empty_str', bool(t_empty))
    print('bool_str', bool(t_str))
except Exception as e:
    print('SKIP___bool__ special method', type(e).__name__, e)

# === __len__ special method ===
try:
    class Container:
        def __init__(self, items):
            self.items = items
        
        def __len__(self):
            return len(self.items)

    cont = Container([1, 2, 3, 4, 5])
    print('len_method', len(cont))
except Exception as e:
    print('SKIP___len__ special method', type(e).__name__, e)

# === __getitem__ special method ===
try:
    class Indexable:
        def __init__(self, data):
            self.data = data
        
        def __getitem__(self, key):
            return self.data[key]

    idx = Indexable(['a', 'b', 'c'])
    print('getitem_0', idx[0])
    print('getitem_1', idx[1])
    print('getitem_slice', idx[0:2])
except Exception as e:
    print('SKIP___getitem__ special method', type(e).__name__, e)

# === __setitem__ special method ===
try:
    class MutableIndexable:
        def __init__(self):
            self.data = {}
        
        def __setitem__(self, key, value):
            self.data[key] = value
        
        def __getitem__(self, key):
            return self.data[key]

    mi = MutableIndexable()
    mi['key1'] = 'value1'
    mi[42] = 'value2'
    print('setitem_key1', mi['key1'])
    print('setitem_num', mi[42])
except Exception as e:
    print('SKIP___setitem__ special method', type(e).__name__, e)

# === __delitem__ special method ===
try:
    class DeletableIndexable:
        def __init__(self):
            self.data = {'a': 1, 'b': 2, 'c': 3}
        
        def __delitem__(self, key):
            del self.data[key]
        
        def __getitem__(self, key):
            return self.data[key]
        
        def keys(self):
            return list(self.data.keys())

    di = DeletableIndexable()
    print('delitem_before', di.keys())
    del di['b']
    print('delitem_after', di.keys())
except Exception as e:
    print('SKIP___delitem__ special method', type(e).__name__, e)

# === __iter__ and __next__ special methods ===
try:
    class Countdown:
        def __init__(self, start):
            self.start = start
            self.current = start
        
        def __iter__(self):
            self.current = self.start
            return self
        
        def __next__(self):
            if self.current < 0:
                raise StopIteration
            num = self.current
            self.current -= 1
            return num

    cd = Countdown(3)
    print('iter_next', list(cd))
    cd2 = Countdown(5)
    print('iter_next_2', list(cd2))
except Exception as e:
    print('SKIP___iter__ and __next__ special methods', type(e).__name__, e)

# === __contains__ special method ===
try:
    class Membership:
        def __init__(self, items):
            self.items = set(items)
        
        def __contains__(self, item):
            return item in self.items

    mem = Membership([1, 2, 3, 4, 5])
    print('contains_yes', 3 in mem)
    print('contains_no', 10 in mem)
except Exception as e:
    print('SKIP___contains__ special method', type(e).__name__, e)

# === __add__ and __radd__ special methods ===
try:
    class Addable:
        def __init__(self, value):
            self.value = value
        
        def __add__(self, other):
            if isinstance(other, Addable):
                return Addable(self.value + other.value)
            elif isinstance(other, (int, float)):
                return Addable(self.value + other)
            return NotImplemented
        
        def __radd__(self, other):
            return self.__add__(other)
        
        def __repr__(self):
            return f'Addable({self.value!r})'

    a1 = Addable(5)
    a2 = Addable(3)
    result1 = a1 + a2
    result2 = a1 + 10
    result3 = 20 + a1
    print('add_addable', result1.value)
    print('add_int', result2.value)
    print('radd_int', result3.value)
except Exception as e:
    print('SKIP___add__ and __radd__ special methods', type(e).__name__, e)

# === __sub__, __mul__, __truediv__ special methods ===
try:
    class Arithmetic:
        def __init__(self, value):
            self.value = value
        
        def __sub__(self, other):
            if isinstance(other, Arithmetic):
                return Arithmetic(self.value - other.value)
            return Arithmetic(self.value - other)
        
        def __mul__(self, other):
            if isinstance(other, Arithmetic):
                return Arithmetic(self.value * other.value)
            return Arithmetic(self.value * other)
        
        def __truediv__(self, other):
            if isinstance(other, Arithmetic):
                return Arithmetic(self.value / other.value)
            return Arithmetic(self.value / other)
        
        def __repr__(self):
            return f'Arithmetic({self.value!r})'

    ar1 = Arithmetic(10)
    ar2 = Arithmetic(3)
    print('arith_sub', (ar1 - ar2).value)
    print('arith_mul', (ar1 * ar2).value)
    print('arith_div', (ar1 / ar2).value)
except Exception as e:
    print('SKIP___sub__, __mul__, __truediv__ special methods', type(e).__name__, e)

# === __iadd__ (in-place add) special method ===
try:
    class InPlaceAdd:
        def __init__(self, value):
            self.value = value
        
        def __iadd__(self, other):
            if isinstance(other, InPlaceAdd):
                self.value += other.value
            else:
                self.value += other
            return self

    ipa = InPlaceAdd(5)
    ipa += 3
    print('iadd_result', ipa.value)
    ipa2 = InPlaceAdd(10)
    ipa += ipa2
    print('iadd_obj_result', ipa.value)
except Exception as e:
    print('SKIP___iadd__ (in-place add) special method', type(e).__name__, e)

# === __neg__, __pos__, __abs__ special methods ===
try:
    class Signed:
        def __init__(self, value):
            self.value = value
        
        def __neg__(self):
            return Signed(-self.value)
        
        def __pos__(self):
            return Signed(+self.value)
        
        def __abs__(self):
            return Signed(abs(self.value))
        
        def __repr__(self):
            return f'Signed({self.value!r})'

    s_pos = Signed(5)
    s_neg = Signed(-3)
    print('neg_pos', (-s_pos).value)
    print('neg_neg', (-s_neg).value)
    print('pos_pos', (+s_pos).value)
    print('abs_neg', (abs(s_neg)).value)
except Exception as e:
    print('SKIP___neg__, __pos__, __abs__ special methods', type(e).__name__, e)

# === __int__, __float__, __complex__ special methods ===
try:
    class Convertible:
        def __init__(self, value):
            self.value = value
        
        def __int__(self):
            return int(self.value)
        
        def __float__(self):
            return float(self.value)
        
        def __complex__(self):
            return complex(self.value)

    conv = Convertible(3.14)
    print('int_conv', int(conv))
    print('float_conv', float(conv))
    print('complex_conv', complex(conv))
except Exception as e:
    print('SKIP___int__, __float__, __complex__ special methods', type(e).__name__, e)

# === __index__ special method ===
try:
    class IndexableValue:
        def __init__(self, value):
            self.value = value
        
        def __index__(self):
            return self.value

    iv = IndexableValue(3)
    items = [10, 20, 30, 40, 50]
    print('index_slice', items[iv])
except Exception as e:
    print('SKIP___index__ special method', type(e).__name__, e)

# === __call__ special method ===
try:
    class Callable:
        def __init__(self, prefix):
            self.prefix = prefix
            self.call_count = 0
        
        def __call__(self, name):
            self.call_count += 1
            return f'{self.prefix}: {name}'

    call = Callable('Hello')
    print('call_1', call('World'))
    print('call_2', call('Python'))
    print('call_count', call.call_count)
except Exception as e:
    print('SKIP___call__ special method', type(e).__name__, e)

# === __enter__ and __exit__ (context manager) ===
try:
    class ContextManager:
        def __init__(self, name):
            self.name = name
            self.entered = False
            self.exited = False
        
        def __enter__(self):
            self.entered = True
            return self
        
        def __exit__(self, exc_type, exc_val, exc_tb):
            self.exited = True
            return False

    with ContextManager('test') as cm:
        print('cm_entered', cm.entered)
    print('cm_exited', cm.exited)
except Exception as e:
    print('SKIP___enter__ and __exit__ (context manager)', type(e).__name__, e)

# === Context manager with exception handling ===
try:
    class SuppressErrors:
        def __init__(self, *errors):
            self.errors = errors
        
        def __enter__(self):
            return self
        
        def __exit__(self, exc_type, exc_val, exc_tb):
            return exc_type is not None and issubclass(exc_type, self.errors)

    with SuppressErrors(ValueError):
        raise ValueError('This is suppressed')
    print('cm_suppress_worked', True)
except Exception as e:
    print('SKIP_Context manager with exception handling', type(e).__name__, e)

# === __slots__ basic ===
try:
    class WithSlots:
        __slots__ = ['x', 'y']
        
        def __init__(self, x, y):
            self.x = x
            self.y = y

    ws = WithSlots(1, 2)
    print('slots_x', ws.x)
    print('slots_y', ws.y)
    print('has_no_dict', not hasattr(ws, '__dict__'))
except Exception as e:
    print('SKIP___slots__ basic', type(e).__name__, e)

# === __slots__ with default and class attribute ===
try:
    class SlotsWithDefault:
        __slots__ = ['value', 'name']
        default = 'class_default'
        
        def __init__(self, value, name=None):
            self.value = value
            self.name = name if name else self.default

    swd = SlotsWithDefault(42)
    print('slots_default_value', swd.value)
    print('slots_default_name', swd.name)
except Exception as e:
    print('SKIP___slots__ with default and class attribute', type(e).__name__, e)

# === __getattr__ special method ===
try:
    class DynamicAttributes:
        def __init__(self):
            self.real_attr = 'exists'
        
        def __getattr__(self, name):
            if name.startswith('computed_'):
                return f'computed value for {name}'
            raise AttributeError(f'{name!r} not found')

    da = DynamicAttributes()
    print('getattr_real', da.real_attr)
    print('getattr_computed', da.computed_something)
    try:
        _ = da.nonexistent
    except AttributeError as e:
        print('getattr_missing', str(e))
except Exception as e:
    print('SKIP___getattr__ special method', type(e).__name__, e)

# === __getattribute__ special method ===
try:
    class InterceptedAttributes:
        def __init__(self):
            self._data = {'internal': 'stored'}
        
        def __getattribute__(self, name):
            if name.startswith('_'):
                return object.__getattribute__(self, name)
            data = object.__getattribute__(self, '_data')
            if name in data:
                return data[name]
            return f'intercepted: {name}'

    ia = InterceptedAttributes()
    print('getattribute_internal', ia.internal)
    print('getattribute_external', ia.anything)
except Exception as e:
    print('SKIP___getattribute__ special method', type(e).__name__, e)

# === __setattr__ special method ===
try:
    class ValidatedSetattr:
        def __init__(self):
            self._values = {}
        
        def __setattr__(self, name, value):
            if name.startswith('_'):
                object.__setattr__(self, name, value)
            else:
                if not isinstance(value, (int, float)):
                    raise TypeError('Only numbers allowed')
                self._values[name] = value
        
        def __getattr__(self, name):
            return self._values.get(name)

    vs = ValidatedSetattr()
    vs.x = 10
    vs.y = 3.14
    print('setattr_x', vs.x)
    print('setattr_y', vs.y)
    try:
        vs.z = 'string'
    except TypeError:
        print('setattr_validation', 'caught')
except Exception as e:
    print('SKIP___setattr__ special method', type(e).__name__, e)

# === __delattr__ special method ===
try:
    class TrackedDeletion:
        def __init__(self):
            self.x = 1
            self.y = 2
            self.deleted = []
        
        def __delattr__(self, name):
            self.deleted.append(name)
            object.__delattr__(self, name)

    td = TrackedDeletion()
    del td.x
    print('delattr_deleted', td.deleted)
    print('delattr_has_y', hasattr(td, 'y'))
    print('delattr_has_x', hasattr(td, 'x'))
except Exception as e:
    print('SKIP___delattr__ special method', type(e).__name__, e)

# === Descriptors - basic ===
try:
    class Descriptor:
        def __init__(self, name):
            self.name = name
            self.values = {}
        
        def __get__(self, obj, objtype=None):
            if obj is None:
                return self
            return self.values.get(id(obj), f'default_{self.name}')
        
        def __set__(self, obj, value):
            self.values[id(obj)] = value
        
        def __delete__(self, obj):
            del self.values[id(obj)]

    class WithDescriptor:
        attr = Descriptor('attr')

    wd1 = WithDescriptor()
    wd2 = WithDescriptor()
    wd1.attr = 'instance1_value'
    wd2.attr = 'instance2_value'
    print('descriptor_inst1', wd1.attr)
    print('descriptor_inst2', wd2.attr)
except Exception as e:
    print('SKIP_Descriptors - basic', type(e).__name__, e)

# === Descriptors - __set_name__ ===
try:
    class NamedDescriptor:
        def __set_name__(self, owner, name):
            self.name = name
            self.storage_name = f'_{name}'
        
        def __get__(self, obj, objtype=None):
            if obj is None:
                return self
            return getattr(obj, self.storage_name, None)
        
        def __set__(self, obj, value):
            setattr(obj, self.storage_name, value)

    class WithNamedDescriptor:
        x = NamedDescriptor()

    wnd = WithNamedDescriptor()
    wnd.x = 42
    print('descriptor_set_name', wnd.x)
    print('descriptor_storage', hasattr(wnd, '_x'))
except Exception as e:
    print('SKIP_Descriptors - __set_name__', type(e).__name__, e)

# === Descriptors - read-only ===
try:
    class ReadOnlyDescriptor:
        def __init__(self, value):
            self.value = value
        
        def __get__(self, obj, objtype=None):
            if obj is None:
                return self
            return self.value
        
        def __set__(self, obj, value):
            raise AttributeError('Read-only attribute')

    class WithReadOnly:
        constant = ReadOnlyDescriptor(100)

    wro = WithReadOnly()
    print('readonly_value', wro.constant)
    try:
        wro.constant = 200
    except AttributeError:
        print('readonly_error', 'caught')
except Exception as e:
    print('SKIP_Descriptors - read-only', type(e).__name__, e)

# === Class variables ===
try:
    class ClassVars:
        class_var = 'shared'
        
        def __init__(self):
            self.instance_var = 'unique'

    cv1 = ClassVars()
    cv2 = ClassVars()
    print('classvar_shared_init', cv1.class_var == cv2.class_var)
    ClassVars.class_var = 'modified'
    print('classvar_after_modify', cv1.class_var)
except Exception as e:
    print('SKIP_Class variables', type(e).__name__, e)

# === Class variables shadowing ===
try:
    cv1.class_var = 'shadowed'
    print('classvar_shadowed', cv1.class_var)
    print('classvar_not_shadowed', cv2.class_var)
except Exception as e:
    print('SKIP_Class variables shadowing', type(e).__name__, e)

# === Nested classes ===
try:
    class Outer:
        class_var = 'outer_value'
        
        class Inner:
            inner_var = 'inner_value'
            
            def get_outer_class(self):
                return Outer
        
        def create_inner(self):
            return self.Inner()

    outer = Outer()
    inner = outer.create_inner()
    print('nested_inner_var', inner.inner_var)
    print('nested_get_outer', inner.get_outer_class().class_var)
except Exception as e:
    print('SKIP_Nested classes', type(e).__name__, e)

# === Class as namespace ===
try:
    class Namespace:
        a = 1
        b = 2
        c = a + b

    print('namespace_c', Namespace.c)
    print('namespace_a', Namespace.a)
except Exception as e:
    print('SKIP_Class as namespace', type(e).__name__, e)

# === type() function - dynamic class ===
try:
    DynamicClass = type('DynamicClass', (), {'x': 10, 'greet': lambda self: 'hello'})
    dc = DynamicClass()
    print('dynamic_class_x', dc.x)
    print('dynamic_class_greet', dc.greet())
except Exception as e:
    print('SKIP_type() function - dynamic class', type(e).__name__, e)

# === type() with bases ===
try:
    DynamicSubClass = type('DynamicSubClass', (DynamicClass,), {'y': 20})
    dsc = DynamicSubClass()
    print('dynamic_subclass_x', dsc.x)
    print('dynamic_subclass_y', dsc.y)
except Exception as e:
    print('SKIP_type() with bases', type(e).__name__, e)

# === Metaclass basic ===
try:
    class Meta(type):
        def __new__(mcs, name, bases, namespace, **kwargs):
            namespace['meta_added'] = 'from_meta'
            return super().__new__(mcs, name, bases, namespace)
        
        def __init__(cls, name, bases, namespace, **kwargs):
            super().__init__(name, bases, namespace)
            cls.meta_initialized = True

    class WithMeta(metaclass=Meta):
        pass

    print('metaclass_added', WithMeta.meta_added)
    print('metaclass_initialized', WithMeta.meta_initialized)
except Exception as e:
    print('SKIP_Metaclass basic', type(e).__name__, e)

# === Metaclass with __call__ ===
try:
    class CountingMeta(type):
        instance_count = 0
        
        def __call__(cls, *args, **kwargs):
            CountingMeta.instance_count += 1
            return super().__call__(*args, **kwargs)

    class CountedClass(metaclass=CountingMeta):
        def __init__(self):
            self.id = CountingMeta.instance_count

    c1 = CountedClass()
    c2 = CountedClass()
    c3 = CountedClass()
    print('metaclass_call_c1', c1.id)
    print('metaclass_call_c2', c2.id)
    print('metaclass_call_c3', c3.id)
    print('metaclass_total', CountingMeta.instance_count)
except Exception as e:
    print('SKIP_Metaclass with __call__', type(e).__name__, e)

# === Metaclass with parameters ===
try:
    class ParamMeta(type):
        def __new__(mcs, name, bases, namespace, extra=None, **kwargs):
            cls = super().__new__(mcs, name, bases, namespace)
            cls.extra = extra
            return cls
        
        def __init__(cls, name, bases, namespace, extra=None, **kwargs):
            super().__init__(name, bases, namespace)

    class WithParamMeta(metaclass=ParamMeta, extra='custom_value'):
        pass

    print('metaclass_param', WithParamMeta.extra)
except Exception as e:
    print('SKIP_Metaclass with parameters', type(e).__name__, e)

# === Abstract Base Class ===
try:
    from abc import ABC, abstractmethod

    class Shape(ABC):
        @abstractmethod
        def area(self):
            pass
        
        @abstractmethod
        def perimeter(self):
            pass

    class Rectangle(Shape):
        def __init__(self, width, height):
            self.width = width
            self.height = height
        
        def area(self):
            return self.width * self.height
        
        def perimeter(self):
            return 2 * (self.width + self.height)

    rect = Rectangle(5, 3)
    print('abc_area', rect.area())
    print('abc_perimeter', rect.perimeter())
    print('abc_is_shape', isinstance(rect, Shape))
except Exception as e:
    print('SKIP_Abstract Base Class', type(e).__name__, e)

# === Dataclass ===
try:
    from dataclasses import dataclass, field

    @dataclass
    class PersonDC:
        name: str
        age: int = 0
        tags: list = field(default_factory=list)

    pdc1 = PersonDC('Alice', 30)
    pdc2 = PersonDC('Bob', 25, ['developer'])
    print('dataclass_name', pdc1.name)
    print('dataclass_age', pdc1.age)
    print('dataclass_tags', pdc2.tags)
    print('dataclass_repr', repr(pdc1))
except Exception as e:
    print('SKIP_Dataclass', type(e).__name__, e)

# === Dataclass frozen ===
try:
    @dataclass(frozen=True)
    class FrozenPoint:
        x: float
        y: float

    fp = FrozenPoint(1.0, 2.0)
    print('frozen_x', fp.x)
    print('frozen_hash', hash(fp))
except Exception as e:
    print('SKIP_Dataclass frozen', type(e).__name__, e)

# === Enum ===
try:
    from enum import Enum, auto

    class Color(Enum):
        RED = 1
        GREEN = 2
        BLUE = 3

    print('enum_name', Color.RED.name)
    print('enum_value', Color.RED.value)
    print('enum_access', Color(2).name)
except Exception as e:
    print('SKIP_Enum', type(e).__name__, e)

# === Enum with auto ===
try:
    class Status(Enum):
        PENDING = auto()
        RUNNING = auto()
        COMPLETED = auto()

    print('auto_pending', Status.PENDING.value)
    print('auto_running', Status.RUNNING.value)
except Exception as e:
    print('SKIP_Enum with auto', type(e).__name__, e)

# === NamedTuple ===
try:
    from typing import NamedTuple

    class PointNT(NamedTuple):
        x: int
        y: int

    pnt = PointNT(3, 4)
    print('namedtuple_x', pnt.x)
    print('namedtuple_y', pnt.y)
    print('namedtuple_index', pnt[0])
    print('namedtuple_unpack', (lambda x, y: x + y)(*pnt))
except Exception as e:
    print('SKIP_NamedTuple', type(e).__name__, e)

# === TypedDict ===
try:
    from typing import TypedDict

    class Movie(TypedDict):
        name: str
        year: int
        rating: float

    movie: Movie = {'name': 'Inception', 'year': 2010, 'rating': 8.8}
    print('typeddict_name', movie['name'])
    print('typeddict_year', movie['year'])
except Exception as e:
    print('SKIP_TypedDict', type(e).__name__, e)

# === Protocol ===
try:
    from typing import Protocol

    class Drawable(Protocol):
        def draw(self) -> str:
            ...

    class CircleShape:
        def draw(self):
            return 'Drawing circle'

    def render(item: Drawable):
        return item.draw()

    cs = CircleShape()
    print('protocol_draw', render(cs))
except Exception as e:
    print('SKIP_Protocol', type(e).__name__, e)

# === Generic class ===
try:
    from typing import TypeVar, Generic

    T = TypeVar('T')

    class Box(Generic[T]):
        def __init__(self, value: T):
            self.value = value
        
        def get(self) -> T:
            return self.value

    int_box = Box[int](42)
    str_box = Box[str]('hello')
    print('generic_int', int_box.get())
    print('generic_str', str_box.get())
except Exception as e:
    print('SKIP_Generic class', type(e).__name__, e)

# === Class method as alternative constructor ===
try:
    class Date:
        def __init__(self, year, month, day):
            self.year = year
            self.month = month
            self.day = day
        
        @classmethod
        def from_string(cls, date_str):
            year, month, day = map(int, date_str.split('-'))
            return cls(year, month, day)
        
        @classmethod
        def today(cls):
            return cls(2024, 1, 1)
        
        def __repr__(self):
            return f'Date({self.year}, {self.month}, {self.day})'

    d1 = Date.from_string('2024-03-15')
    d2 = Date.today()
    print('alt_constructor_str', repr(d1))
    print('alt_constructor_today', repr(d2))
except Exception as e:
    print('SKIP_Class method as alternative constructor', type(e).__name__, e)

# === Property caching ===
try:
    class LazyProperty:
        def __init__(self):
            self._expensive = None
        
        @property
        def expensive(self):
            if self._expensive is None:
                self._expensive = sum(range(1000))
            return self._expensive

    lp = LazyProperty()
    print('lazy_first', lp.expensive)
    print('lazy_second', lp.expensive)
except Exception as e:
    print('SKIP_Property caching', type(e).__name__, e)

# === Class with __dir__ ===
try:
    class DirOverride:
        def __dir__(self):
            return ['custom', 'attributes', 'only']

    dir_obj = DirOverride()
    print('dir_override', dir(dir_obj))
except Exception as e:
    print('SKIP_Class with __dir__', type(e).__name__, e)

# === Class with __sizeof__ ===
try:
    class WithSizeof:
        def __sizeof__(self):
            return 100

    ws_obj = WithSizeof()
    print('sizeof_custom', ws_obj.__sizeof__())
except Exception as e:
    print('SKIP_Class with __sizeof__', type(e).__name__, e)

# === Class with __format__ ===
try:
    class Formattable:
        def __init__(self, value):
            self.value = value
        
        def __format__(self, format_spec):
            if format_spec == 'upper':
                return str(self.value).upper()
            elif format_spec == 'reverse':
                return str(self.value)[::-1]
            return str(self.value)

    fmt = Formattable('hello')
    print('format_upper', format(fmt, 'upper'))
    print('format_reverse', format(fmt, 'reverse'))
    print('format_default', format(fmt, ''))
except Exception as e:
    print('SKIP_Class with __format__', type(e).__name__, e)

# === Class with __bytes__ ===
try:
    class ByteConvertible:
        def __bytes__(self):
            return b'byte representation'

    bc = ByteConvertible()
    print('bytes_method', bytes(bc))
except Exception as e:
    print('SKIP_Class with __bytes__', type(e).__name__, e)

# === Class with __hash__ and __eq__ for dict key ===
try:
    class DictKey:
        def __init__(self, id):
            self.id = id
        
        def __eq__(self, other):
            if isinstance(other, DictKey):
                return self.id == other.id
            return False
        
        def __hash__(self):
            return hash(self.id)
        
        def __repr__(self):
            return f'DictKey({self.id})'

    dk1 = DictKey(1)
    dk2 = DictKey(1)
    dk3 = DictKey(2)
    d = {dk1: 'value1'}
    print('dictkey_lookup', d[dk2])
    print('dictkey_same_hash', hash(dk1) == hash(dk2))
except Exception as e:
    print('SKIP_Class with __hash__ and __eq__ for dict key', type(e).__name__, e)

# === Weak reference support ===
try:
    import weakref

    class WeakRefable:
        pass

    wr = WeakRefable()
    ref = weakref.ref(wr)
    print('weakref_exists', ref() is wr)
    del wr
    print('weakref_dead', ref() is None)
except Exception as e:
    print('SKIP_Weak reference support', type(e).__name__, e)

# === Class with __del__ ===
try:
    class WithDestructor:
        destroyed = []
        
        def __init__(self, name):
            self.name = name
        
        def __del__(self):
            WithDestructor.destroyed.append(self.name)

    wd1 = WithDestructor('first')
    wd2 = WithDestructor('second')
    del wd1
    del wd2
    import gc
    gc.collect()
    print('destructor_called', 'first' in WithDestructor.destroyed)
except Exception as e:
    print('SKIP_Class with __del__', type(e).__name__, e)

# === Class with __getstate__ and __setstate__ ===
try:
    import pickle

    class CustomPickle:
        def __init__(self):
            self.data = 'sensitive'
            self.computed = 42
        
        def __getstate__(self):
            return {'computed': self.computed}
        
        def __setstate__(self, state):
            self.computed = state['computed']
            self.data = 'restored'

    cp = CustomPickle()
    pickled = pickle.dumps(cp)
    restored = pickle.loads(pickled)
    print('pickle_state_data', restored.data)
    print('pickle_state_computed', restored.computed)
except Exception as e:
    print('SKIP_Class with __getstate__ and __setstate__', type(e).__name__, e)

# === Class with __reduce__ ===
try:
    class Reducible:
        def __init__(self, value):
            self.value = value
        
        def __reduce__(self):
            return (Reducible, (self.value * 2,))

    red = Reducible(5)
    pickled_red = pickle.loads(pickle.dumps(red))
    print('reduce_value', pickled_red.value)
except Exception as e:
    print('SKIP_Class with __reduce__', type(e).__name__, e)

# === Class with __copy__ and __deepcopy__ ===
try:
    import copy

    class Copyable:
        def __init__(self, value):
            self.value = value
            self.copied = False
            self.deep_copied = False
        
        def __copy__(self):
            new = Copyable(self.value)
            new.copied = True
            return new
        
        def __deepcopy__(self, memo):
            new = Copyable(copy.deepcopy(self.value, memo))
            new.deep_copied = True
            return new

    orig = Copyable([1, 2, 3])
    shallow = copy.copy(orig)
    deep = copy.deepcopy(orig)
    print('copy_shallow', shallow.copied)
    print('copy_deep', deep.deep_copied)
except Exception as e:
    print('SKIP_Class with __copy__ and __deepcopy__', type(e).__name__, e)

# === Class with __instancecheck__ and __subclasscheck__ ===
try:
    class MetaABC(type):
        def __instancecheck__(cls, instance):
            return hasattr(instance, 'required_method')
        
        def __subclasscheck__(cls, subclass):
            return hasattr(subclass, 'required_method')

    class Interface(metaclass=MetaABC):
        pass

    class Implements:
        def required_method(self):
            pass

    class DoesNotImplement:
        pass

    print('abc_instance_yes', isinstance(Implements(), Interface))
    print('abc_instance_no', isinstance(DoesNotImplement(), Interface))
    print('abc_subclass_yes', issubclass(Implements, Interface))
    print('abc_subclass_no', issubclass(DoesNotImplement, Interface))
except Exception as e:
    print('SKIP_Class with __instancecheck__ and __subclasscheck__', type(e).__name__, e)

# === Final summary ===
print('all_tests_completed', True)
