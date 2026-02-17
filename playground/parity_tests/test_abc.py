"""Comprehensive tests for Python's abc module."""
import abc
from abc import ABC, ABCMeta, abstractmethod, abstractclassmethod
from abc import abstractstaticmethod, abstractproperty, get_cache_token, update_abstractmethods

# === ABCMeta basic usage ===
try:
    class MyABCMeta(metaclass=ABCMeta):
        pass

    print('abcmeta_basic_isinstance', isinstance(MyABCMeta(), MyABCMeta))
    print('abcmeta_basic_issubclass', issubclass(MyABCMeta, ABC))
except Exception as e:
    print('SKIP_ABCMeta basic usage', type(e).__name__, e)

# === ABC helper class ===
try:
    class MyABC(ABC):
        pass

    print('abc_helper_isinstance', isinstance(MyABC(), MyABC))
    print('abc_helper_type', type(MyABC).__name__)
except Exception as e:
    print('SKIP_ABC helper class', type(e).__name__, e)

# === ABCMeta with abstractmethod ===
try:
    class AbstractShape(metaclass=ABCMeta):
        @abstractmethod
        def area(self):
            pass

        @abstractmethod
        def perimeter(self):
            pass

    print('abstract_shape_is_abstract', getattr(AbstractShape, '__abstractmethods__', set()))

    class Rectangle(AbstractShape):
        def __init__(self, w, h):
            self.w = w
            self.h = h
        
        def area(self):
            return self.w * self.h
        
        def perimeter(self):
            return 2 * (self.w + self.h)

    rect = Rectangle(3, 4)
    print('rectangle_area', rect.area())
    print('rectangle_perimeter', rect.perimeter())
    print('rectangle_isinstance_shape', isinstance(rect, AbstractShape))
except Exception as e:
    print('SKIP_ABCMeta with abstractmethod', type(e).__name__, e)

# === ABC with abstractmethod ===
try:
    class AbstractAnimal(ABC):
        @abstractmethod
        def speak(self):
            pass

    class Dog(AbstractAnimal):
        def speak(self):
            return 'woof'

    dog = Dog()
    print('dog_speak', dog.speak())
    print('dog_isinstance_animal', isinstance(dog, AbstractAnimal))
except Exception as e:
    print('SKIP_ABC with abstractmethod', type(e).__name__, e)

# === Abstract method enforcement ===
try:
    try:
        class Incomplete(AbstractAnimal):
            pass
        
        incomplete = Incomplete()
    except TypeError as e:
        print('abstract_enforcement_error', str(e))
except Exception as e:
    print('SKIP_Abstract method enforcement', type(e).__name__, e)

# === abstractmethod with property ===
try:
    class AbstractData(ABC):
        @property
        @abstractmethod
        def value(self):
            pass

    class ConcreteData(AbstractData):
        @property
        def value(self):
            return 42

    data = ConcreteData()
    print('concrete_data_value', data.value)
except Exception as e:
    print('SKIP_abstractmethod with property', type(e).__name__, e)

# === abstractmethod with classmethod ===
try:
    class AbstractFactory(ABC):
        @classmethod
        @abstractmethod
        def create(cls):
            pass

    class ConcreteFactory(AbstractFactory):
        @classmethod
        def create(cls):
            return cls()

    factory_obj = ConcreteFactory.create()
    print('factory_created', type(factory_obj).__name__)
except Exception as e:
    print('SKIP_abstractmethod with classmethod', type(e).__name__, e)

# === abstractmethod with staticmethod ===
try:
    class AbstractUtil(ABC):
        @staticmethod
        @abstractmethod
        def helper(x):
            pass

    class ConcreteUtil(AbstractUtil):
        @staticmethod
        def helper(x):
            return x * 2

    print('util_helper', ConcreteUtil.helper(5))
except Exception as e:
    print('SKIP_abstractmethod with staticmethod', type(e).__name__, e)

# === abstractclassmethod (deprecated) ===
try:
    class OldStyleABC(ABC):
        @abstractclassmethod
        def from_string(cls, s):
            pass

    class NewFromString(OldStyleABC):
        @classmethod
        def from_string(cls, s):
            return cls()

    nfs = NewFromString.from_string('test')
    print('old_style_classmethod', type(nfs).__name__)
except Exception as e:
    print('SKIP_abstractclassmethod (deprecated)', type(e).__name__, e)

# === abstractstaticmethod (deprecated) ===
try:
    class OldStaticABC(ABC):
        @abstractstaticmethod
        def process(x):
            pass

    class NewStatic(OldStaticABC):
        @staticmethod
        def process(x):
            return x + 1

    print('old_style_staticmethod', NewStatic.process(10))
except Exception as e:
    print('SKIP_abstractstaticmethod (deprecated)', type(e).__name__, e)

# === abstractproperty (deprecated) ===
try:
    class OldPropABC(ABC):
        @abstractproperty
        def name(self):
            pass

    class NewProp(OldPropABC):
        @property
        def name(self):
            return 'newprop'

    np = NewProp()
    print('old_style_property', np.name)
except Exception as e:
    print('SKIP_abstractproperty (deprecated)', type(e).__name__, e)

# === ABCMeta.register for virtual subclasses ===
try:
    class VirtualBase(ABC):
        pass

    class UnrelatedClass:
        pass

    VirtualBase.register(UnrelatedClass)
    print('virtual_subclass_check', issubclass(UnrelatedClass, VirtualBase))
    print('virtual_instance_check', isinstance(UnrelatedClass(), VirtualBase))
except Exception as e:
    print('SKIP_ABCMeta.register for virtual subclasses', type(e).__name__, e)

# === ABCMeta.register as decorator ===
try:
    class AnotherBase(ABC):
        pass

    @AnotherBase.register
    class DecoratedClass:
        pass

    print('decorator_register', issubclass(DecoratedClass, AnotherBase))
except Exception as e:
    print('SKIP_ABCMeta.register as decorator', type(e).__name__, e)

# === __subclasshook__ ===
try:
    class SizedABC(ABC):
        @classmethod
        def __subclasshook__(cls, subclass):
            if cls is SizedABC:
                if any('__len__' in B.__dict__ for B in subclass.__mro__):
                    return True
            return NotImplemented

    class MySized:
        def __len__(self):
            return 10

    print('subclasshook_check', issubclass(MySized, SizedABC))
    print('subclasshook_instance', isinstance(MySized(), SizedABC))
except Exception as e:
    print('SKIP___subclasshook__', type(e).__name__, e)

# === get_cache_token ===
try:
    token1 = get_cache_token()
    print('cache_token_type', type(token1).__name__)
    print('cache_token_value', token1)

    # Token changes after register
    class TokenTest(ABC):
        pass

    token2 = get_cache_token()
    TokenTest.register(dict)
    token3 = get_cache_token()
    print('token_changed_after_register', token2 != token3)
except Exception as e:
    print('SKIP_get_cache_token', type(e).__name__, e)

# === update_abstractmethods ===
try:
    class DynamicABC(ABC):
        pass

    def new_method(self):
        pass

    new_method.__isabstractmethod__ = True
    DynamicABC.new_abstract = new_method

    # Before update
    print('before_update_abstracts', getattr(DynamicABC, '__abstractmethods__', set()))

    update_abstractmethods(DynamicABC)

    # After update
    print('after_update_abstracts', getattr(DynamicABC, '__abstractmethods__', set()))
except Exception as e:
    print('SKIP_update_abstractmethods', type(e).__name__, e)

# === Multiple inheritance with ABC ===
try:
    class MixinA(ABC):
        @abstractmethod
        def method_a(self):
            pass

    class MixinB(ABC):
        @abstractmethod
        def method_b(self):
            pass

    class Combined(MixinA, MixinB):
        def method_a(self):
            return 'a'
        
        def method_b(self):
            return 'b'

    c = Combined()
    print('combined_a', c.method_a())
    print('combined_b', c.method_b())
    print('combined_isinstance_a', isinstance(c, MixinA))
    print('combined_isinstance_b', isinstance(c, MixinB))
except Exception as e:
    print('SKIP_Multiple inheritance with ABC', type(e).__name__, e)

# === Abstract method with implementation ===
try:
    class TemplateMethod(ABC):
        @abstractmethod
        def hook(self):
            pass
        
        def algorithm(self):
            return f'hook returned: {self.hook()}'

    class ConcreteTemplate(TemplateMethod):
        def hook(self):
            return 'concrete'

    ct = ConcreteTemplate()
    print('template_algorithm', ct.algorithm())
except Exception as e:
    print('SKIP_Abstract method with implementation', type(e).__name__, e)

# === Calling abstract method via super ===
try:
    class BaseWithImpl(ABC):
        @abstractmethod
        def compute(self):
            return 'base'

    class DerivedCallsSuper(BaseWithImpl):
        def compute(self):
            base = super().compute()
            return f'derived({base})'

    dcs = DerivedCallsSuper()
    print('super_call', dcs.compute())
except Exception as e:
    print('SKIP_Calling abstract method via super', type(e).__name__, e)

# === Abstract method inheritance ===
try:
    class Level1(ABC):
        @abstractmethod
        def required(self):
            pass

    class Level2(Level1):
        pass

    class Level3(Level2):
        def required(self):
            return 'implemented'

    l3 = Level3()
    print('level3_required', l3.required())
    print('level3_isinstance_level1', isinstance(l3, Level1))
except Exception as e:
    print('SKIP_Abstract method inheritance', type(e).__name__, e)

# === __isabstractmethod__ attribute ===
try:
    class AttrTest(ABC):
        @abstractmethod
        def foo(self):
            pass

    print('isabstractmethod_attr', getattr(AttrTest.foo, '__isabstractmethod__', None))
except Exception as e:
    print('SKIP___isabstractmethod__ attribute', type(e).__name__, e)

# === ABC with __slots__ ===
try:
    class SlottedABC(ABC):
        __slots__ = ('x',)
        
        @abstractmethod
        def do_something(self):
            pass

    class SlottedConcrete(SlottedABC):
        __slots__ = ('y',)
        
        def do_something(self):
            return 'done'

    sc = SlottedConcrete()
    sc.x = 1
    sc.y = 2
    print('slotted_works', sc.do_something())
except Exception as e:
    print('SKIP_ABC with __slots__', type(e).__name__, e)

# === MRO preservation with ABC ===
try:
    class MROA:
        def method(self):
            return 'a'

    class MROB(MROA, ABC):
        @abstractmethod
        def other(self):
            pass

    class MROC(MROB):
        def other(self):
            return super().method()

    mroc = MROC()
    print('mro_preserved', mroc.other())
    print('mro_list', [cls.__name__ for cls in MROC.__mro__])
except Exception as e:
    print('SKIP_MRO preservation with ABC', type(e).__name__, e)

# === Abstract methods in __dict__ ===
try:
    class DictCheck(ABC):
        @abstractmethod
        def abs_method(self):
            pass

    print('abstract_in_dict', 'abs_method' in DictCheck.__abstractmethods__)
except Exception as e:
    print('SKIP_Abstract methods in __dict__', type(e).__name__, e)

# === Register returns the subclass ===
try:
    class ReturnTest(ABC):
        pass

    class SomeClass:
        pass

    result = ReturnTest.register(SomeClass)
    print('register_returns_subclass', result is SomeClass)
except Exception as e:
    print('SKIP_Register returns the subclass', type(e).__name__, e)

# === ABC subclasscheck edge cases ===
try:
    class EdgeABC(ABC):
        pass

    print('edge_issubclass_self', issubclass(EdgeABC, EdgeABC))
    print('edge_isinstance_type', isinstance(EdgeABC, type))
except Exception as e:
    print('SKIP_ABC subclasscheck edge cases', type(e).__name__, e)

# === Empty ABC ===
try:
    class EmptyABC(ABC):
        pass

    print('empty_can_instantiate', bool(EmptyABC()))
    print('empty_is_abstract', bool(getattr(EmptyABC, '__abstractmethods__', set())))
except Exception as e:
    print('SKIP_Empty ABC', type(e).__name__, e)

# === Check ABC is in abc module ===
try:
    print('abc_in_module', 'ABC' in dir(abc))
    print('abcmeta_in_module', 'ABCMeta' in dir(abc))
    print('abstractmethod_in_module', 'abstractmethod' in dir(abc))
except Exception as e:
    print('SKIP_Check ABC is in abc module', type(e).__name__, e)

try:
    print('=== All ABC tests completed ===')
except Exception as e:
    print('SKIP_All ABC tests completed', type(e).__name__, e)
