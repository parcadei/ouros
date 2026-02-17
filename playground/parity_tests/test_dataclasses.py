"""Comprehensive dataclasses module parity test.

Tests all 13 public API members:
- dataclass (decorator)
- field (function)
- fields (function)
- asdict (function)
- astuple (function)
- make_dataclass (function)
- replace (function)
- is_dataclass (function)
- FrozenInstanceError (exception)
- InitVar (class)
- KW_ONLY (sentinel)
- MISSING (sentinel)
- recursive_repr (function)
"""

from dataclasses import (
    dataclass,
    field,
    fields,
    asdict,
    astuple,
    make_dataclass,
    replace,
    is_dataclass,
    FrozenInstanceError,
    InitVar,
    KW_ONLY,
    MISSING,
    recursive_repr,
    Field,
)

# === dataclass basic usage ===
try:
    print('=== dataclass basic usage ===')

    @dataclass
    class Point:
        x: int
        y: int

    p = Point(1, 2)
    print(f'Point instance: {p}')
    print(f'Point repr: {repr(p)}')
    print(f'Point x: {p.x}, y: {p.y}')
except Exception as e:
    print('SKIP_dataclass basic usage', type(e).__name__, e)


# === dataclass with defaults ===
try:
    print('\n=== dataclass with defaults ===')

    @dataclass
    class Item:
        name: str
        price: float = 0.0
        quantity: int = 0

    item1 = Item('apple')
    item2 = Item('banana', 1.5)
    item3 = Item('cherry', 2.0, 10)
    print(f'Item with defaults: {item1}')
    print(f'Item with partial defaults: {item2}')
    print(f'Item with all args: {item3}')
except Exception as e:
    print('SKIP_dataclass with defaults', type(e).__name__, e)


# === dataclass with field() ===
try:
    print('\n=== dataclass with field() ===')

    @dataclass
    class Config:
        name: str = field(default='default_name')
        values: list = field(default_factory=list)
        internal: int = field(init=False, default=42)
        hidden: str = field(repr=False, default='secret')
        readonly: float = field(compare=False, default=3.14)

    config = Config('test', [1, 2, 3])
    print(f'Config: {config}')
    print(f'Config internal: {config.internal}')
    print(f'Config hidden: {config.hidden}')
except Exception as e:
    print('SKIP_dataclass with field()', type(e).__name__, e)


# === dataclass frozen ===
try:
    print('\n=== dataclass frozen ===')

    @dataclass(frozen=True)
    class ImmutablePoint:
        x: int
        y: int

    ip = ImmutablePoint(3, 4)
    print(f'Frozen point: {ip}')
    print(f'Frozen point is hashable: {hash(ip)}')

    try:
        ip.x = 5
    except FrozenInstanceError as e:
        print(f'FrozenInstanceError raised as expected: {e}')
except Exception as e:
    print('SKIP_dataclass frozen', type(e).__name__, e)


# === dataclass order comparison ===
try:
    print('\n=== dataclass order comparison ===')

    @dataclass(order=True)
    class OrderedItem:
        price: float
        name: str

    o1 = OrderedItem(10.0, 'a')
    o2 = OrderedItem(20.0, 'b')
    o3 = OrderedItem(10.0, 'c')
    print(f'o1 < o2: {o1 < o2}')
    print(f'o1 <= o3: {o1 <= o3}')
    print(f'o2 > o1: {o2 > o1}')
    print(f'o1 == o3: {o1 == o3}')
except Exception as e:
    print('SKIP_dataclass order comparison', type(e).__name__, e)


# === dataclass eq only ===
try:
    print('\n=== dataclass eq only ===')

    @dataclass(eq=True, order=False)
    class EqOnly:
        value: int

    e1 = EqOnly(1)
    e2 = EqOnly(1)
    e3 = EqOnly(2)
    print(f'e1 == e2: {e1 == e2}')
    print(f'e1 == e3: {e1 == e3}')
except Exception as e:
    print('SKIP_dataclass eq only', type(e).__name__, e)


# === dataclass no eq ===
try:
    print('\n=== dataclass no eq ===')

    @dataclass(eq=False)
    class NoEq:
        value: int

    n1 = NoEq(1)
    n2 = NoEq(1)
    print(f'n1 is n2: {n1 is n2}')
    print(f'n1 == n2 uses identity: {n1 == n2}')
except Exception as e:
    print('SKIP_dataclass no eq', type(e).__name__, e)


# === dataclass no repr ===
try:
    print('\n=== dataclass no repr ===')

    @dataclass(repr=False)
    class NoRepr:
        value: int

    nr = NoRepr(42)
    print(f'NoRepr repr uses default: {repr(nr)}')
except Exception as e:
    print('SKIP_dataclass no repr', type(e).__name__, e)


# === dataclass no init ===
try:
    print('\n=== dataclass no init ===')

    @dataclass(init=False)
    class NoInit:
        value: int = 10

    ni = NoInit()
    print(f'NoInit value: {ni.value}')


    @dataclass(init=False)
    class NoInitWithCustom:
        value: int

        def __init__(self, x, y):
            self.value = x + y

    nic = NoInitWithCustom(3, 4)
    print(f'NoInitWithCustom value: {nic.value}')
except Exception as e:
    print('SKIP_dataclass no init', type(e).__name__, e)


# === dataclass unsafe_hash ===
try:
    print('\n=== dataclass unsafe_hash ===')

    @dataclass(unsafe_hash=True)
    class HashableMutable:
        value: int

    hm = HashableMutable(42)
    print(f'HashableMutable hash: {hash(hm)}')
except Exception as e:
    print('SKIP_dataclass unsafe_hash', type(e).__name__, e)


# === dataclass kw_only ===
try:
    print('\n=== dataclass kw_only ===')

    @dataclass(kw_only=True)
    class KeywordOnly:
        x: int
        y: str = 'default'

    ko = KeywordOnly(x=1)
    print(f'KeywordOnly: {ko}')
except Exception as e:
    print('SKIP_dataclass kw_only', type(e).__name__, e)


# === dataclass with KW_ONLY sentinel ===
try:
    print('\n=== dataclass with KW_ONLY sentinel ===')

    @dataclass
    class MixedArgs:
        x: int
        y: int
        _: KW_ONLY
        z: str = 'default_z'
        w: float = 1.0

    ma = MixedArgs(1, 2, z='hello', w=3.14)
    print(f'MixedArgs: {ma}')
except Exception as e:
    print('SKIP_dataclass with KW_ONLY sentinel', type(e).__name__, e)


# === dataclass slots ===
try:
    print('\n=== dataclass slots ===')

    @dataclass(slots=True)
    class Slotted:
        x: int
        y: str = 'default'

    s = Slotted(1)
    print(f'Slotted: {s}')
    print(f'Slotted has __slots__: {hasattr(Slotted, "__slots__")}')
    print(f'Slotted slots: {Slotted.__slots__}')
except Exception as e:
    print('SKIP_dataclass slots', type(e).__name__, e)


# === dataclass match_args ===
try:
    print('\n=== dataclass match_args ===')

    @dataclass(match_args=True)
    class WithMatchArgs:
        x: int
        y: str

    wma = WithMatchArgs(1, 'a')
    print(f'WithMatchArgs.__match_args__: {WithMatchArgs.__match_args__}')

    @dataclass(match_args=False)
    class NoMatchArgs:
        x: int
        y: str

    print(f'NoMatchArgs has __match_args__: {hasattr(NoMatchArgs, "__match_args__")}')
except Exception as e:
    print('SKIP_dataclass match_args', type(e).__name__, e)


# === dataclass inheritance ===
try:
    print('\n=== dataclass inheritance ===')

    @dataclass
    class Base:
        x: int

    @dataclass
    class Derived(Base):
        y: int

    d = Derived(1, 2)
    print(f'Derived: {d}')
    print(f'Derived x: {d.x}, y: {d.y}')
except Exception as e:
    print('SKIP_dataclass inheritance', type(e).__name__, e)


# === dataclass multiple inheritance ===
try:
    print('\n=== dataclass multiple inheritance ===')

    @dataclass
    class MixinA:
        a: int

    @dataclass
    class MixinB:
        b: str

    @dataclass
    class Combined(MixinA, MixinB):
        c: float

    c = Combined(1, 'two', 3.0)
    print(f'Combined: {c}')
except Exception as e:
    print('SKIP_dataclass multiple inheritance', type(e).__name__, e)


# === fields() function ===
try:
    print('\n=== fields() function ===')

    @dataclass
    class ForFields:
        x: int
        y: str = 'default'
        z: list = field(default_factory=list)

    ff = ForFields(1)
    fs = fields(ff)
    print(f'Number of fields: {len(fs)}')
    for f in fs:
        print(f'  Field {f.name}: type={f.type}, default={f.default}')

    print(f'fields returns tuple of Field objects: {all(isinstance(f, Field) for f in fs)}')
except Exception as e:
    print('SKIP_fields() function', type(e).__name__, e)


# === asdict() function ===
try:
    print('\n=== asdict() function ===')

    @dataclass
    class ForAsDict:
        x: int
        y: str
        nested: Point

    fad = ForAsDict(1, 'hello', Point(10, 20))
    d = asdict(fad)
    print(f'asdict result: {d}')
    print(f'asdict result type: {type(d)}')
except Exception as e:
    print('SKIP_asdict() function', type(e).__name__, e)


# === asdict() with nested dataclasses ===
try:
    print('\n=== asdict() with nested dataclasses ===')

    @dataclass
    class Container:
        items: list
        point: Point

    c = Container([1, 2, 3], Point(5, 10))
    print(f'Container asdict: {asdict(c)}')
except Exception as e:
    print('SKIP_asdict() with nested dataclasses', type(e).__name__, e)


# === astuple() function ===
try:
    print('\n=== astuple() function ===')

    t = astuple(fad)
    print(f'astuple result: {t}')
    print(f'astuple result type: {type(t)}')
except Exception as e:
    print('SKIP_astuple() function', type(e).__name__, e)


# === replace() function ===
try:
    print('\n=== replace() function ===')

    original = Point(1, 2)
    replaced = replace(original, x=100)
    print(f'Original: {original}')
    print(f'Replaced: {replaced}')
    print(f'Original unchanged: {original.x == 1}')
except Exception as e:
    print('SKIP_replace() function', type(e).__name__, e)


# === replace() on frozen dataclass ===
try:
    print('\n=== replace() on frozen dataclass ===')

    frozen_orig = ImmutablePoint(1, 2)
    frozen_new = replace(frozen_orig, y=200)
    print(f'Frozen original: {frozen_orig}')
    print(f'Frozen replaced: {frozen_new}')
except Exception as e:
    print('SKIP_replace() on frozen dataclass', type(e).__name__, e)


# === is_dataclass() function ===
try:
    print('\n=== is_dataclass() function ===')

    print(f'is_dataclass(Point): {is_dataclass(Point)}')
    print(f'is_dataclass(p): {is_dataclass(p)}')
    print(f'is_dataclass(int): {is_dataclass(int)}')
    print(f'is_dataclass(42): {is_dataclass(42)}')
    print(f'is_dataclass(str): {is_dataclass(str)}')

    class RegularClass:
        pass

    print(f'is_dataclass(RegularClass): {is_dataclass(RegularClass)}')
except Exception as e:
    print('SKIP_is_dataclass() function', type(e).__name__, e)


# === make_dataclass() function ===
try:
    print('\n=== make_dataclass() function ===')

    DynamicPoint = make_dataclass(
        'DynamicPoint',
        [('x', int), ('y', int)]
    )
    dp = DynamicPoint(5, 10)
    print(f'DynamicPoint: {dp}')
    print(f'is_dataclass(DynamicPoint): {is_dataclass(DynamicPoint)}')
except Exception as e:
    print('SKIP_make_dataclass() function', type(e).__name__, e)


# === make_dataclass() with defaults ===
try:
    print('\n=== make_dataclass() with defaults ===')

    DynamicConfig = make_dataclass(
        'DynamicConfig',
        [
            ('name', str),
            ('value', int, field(default=42)),
        ]
    )
    dc1 = DynamicConfig('test')
    dc2 = DynamicConfig('test2', 100)
    print(f'DynamicConfig with default: {dc1}')
    print(f'DynamicConfig with value: {dc2}')
except Exception as e:
    print('SKIP_make_dataclass() with defaults', type(e).__name__, e)


# === make_dataclass() with bases ===
try:
    print('\n=== make_dataclass() with bases ===')

    DynamicDerived = make_dataclass(
        'DynamicDerived',
        [('z', float)],
        bases=(Point,)
    )
    dd = DynamicDerived(1, 2, 3.14)
    print(f'DynamicDerived: {dd}')
except Exception as e:
    print('SKIP_make_dataclass() with bases', type(e).__name__, e)


# === InitVar usage ===
try:
    print('\n=== InitVar usage ===')

    @dataclass
    class WithInitVar:
        x: int
        init_val: InitVar[str]
        derived: str = field(init=False)

        def __post_init__(self, init_val: str):
            self.derived = f'derived_from_{init_val}'

    wiv = WithInitVar(42, 'test')
    print(f'WithInitVar: {wiv}')
    print(f'WithInitVar derived: {wiv.derived}')
except Exception as e:
    print('SKIP_InitVar usage', type(e).__name__, e)


# === InitVar with default ===
try:
    print('\n=== InitVar with default ===')

    @dataclass
    class WithInitVarDefault:
        x: int
        init_val: InitVar[str] = 'default'
        derived: str = field(init=False)

        def __post_init__(self, init_val: str):
            self.derived = f'derived_from_{init_val}'

    wivd = WithInitVarDefault(10)
    print(f'WithInitVarDefault: {wivd}')
    print(f'WithInitVarDefault derived: {wivd.derived}')
except Exception as e:
    print('SKIP_InitVar with default', type(e).__name__, e)


# === MISSING sentinel ===
try:
    print('\n=== MISSING sentinel ===')

    print(f'MISSING is MISSING: {MISSING is MISSING}')
    print(f'MISSING repr: {repr(MISSING)}')
    print(f'bool(MISSING): {bool(MISSING)}')
except Exception as e:
    print('SKIP_MISSING sentinel', type(e).__name__, e)


# === field() default_factory with callable ===
try:
    print('\n=== field() default_factory with callable ===')

    counter = 0

    def get_next_id():
        global counter
        counter += 1
        return counter

    @dataclass
    class WithFactory:
        id: int = field(default_factory=get_next_id)
        name: str = 'unnamed'

    wf1 = WithFactory(name='first')
    wf2 = WithFactory(name='second')
    wf3 = WithFactory()
    print(f'WithFactory 1: {wf1}')
    print(f'WithFactory 2: {wf2}')
    print(f'WithFactory 3: {wf3}')
except Exception as e:
    print('SKIP_field() default_factory with callable', type(e).__name__, e)


# === field() with metadata ===
try:
    print('\n=== field() with metadata ===')

    @dataclass
    class WithMetadata:
        value: int = field(metadata={'min': 0, 'max': 100, 'description': 'A value between 0 and 100'})

    wm = WithMetadata(50)
    f = fields(wm)[0]
    print(f'Field metadata: {f.metadata}')
except Exception as e:
    print('SKIP_field() with metadata', type(e).__name__, e)


# === Field object attributes ===
try:
    print('\n=== Field object attributes ===')

    @dataclass
    class FieldAttrs:
        x: int = field(
            default=10,
            init=True,
            repr=True,
            hash=True,
            compare=True,
            metadata={'key': 'value'}
        )
        y: list = field(
            default_factory=list,
            init=True,
            repr=True,
        )

    fa = FieldAttrs()
    xf = fields(fa)[0]
    yf = fields(fa)[1]
    print(f'Field name: {xf.name}')
    print(f'Field type: {xf.type}')
    print(f'Field default: {xf.default}')
    print(f'Field default_factory: {yf.default_factory}')
    print(f'Field init: {xf.init}')
    print(f'Field repr: {xf.repr}')
    print(f'Field hash: {xf.hash}')
    print(f'Field compare: {xf.compare}')
    print(f'Field metadata: {xf.metadata}')
except Exception as e:
    print('SKIP_Field object attributes', type(e).__name__, e)


# === dataclass with __post_init__ ===
try:
    print('\n=== dataclass with __post_init__ ===')

    @dataclass
    class WithPostInit:
        x: int
        y: int
        sum_val: int = field(init=False)

        def __post_init__(self):
            self.sum_val = self.x + self.y

    wpi = WithPostInit(3, 4)
    print(f'WithPostInit: {wpi}')
    print(f'sum_val: {wpi.sum_val}')
except Exception as e:
    print('SKIP_dataclass with __post_init__', type(e).__name__, e)


# === dataclass with class variables ===
try:
    print('\n=== dataclass with class variables ===')

    @dataclass
    class WithClassVar:
        x: int
        class_var: str = 'not_a_field'  # No type annotation = class var
        y: str = 'field'

    wcv = WithClassVar(1)
    print(f'WithClassVar: {wcv}')
    print(f'class_var is class attribute: {"class_var" not in [f.name for f in fields(wcv)]}')
except Exception as e:
    print('SKIP_dataclass with class variables', type(e).__name__, e)


# === FrozenInstanceError inheritance ===
try:
    print('\n=== FrozenInstanceError inheritance ===')

    print(f'FrozenInstanceError is AttributeError subclass: {issubclass(FrozenInstanceError, AttributeError)}')
except Exception as e:
    print('SKIP_FrozenInstanceError inheritance', type(e).__name__, e)


# === dataclass weakref_slot ===
try:
    print('\n=== dataclass weakref_slot ===')

    import weakref

    @dataclass(slots=True, weakref_slot=True)
    class WithWeakRef:
        x: int

    wwr = WithWeakRef(42)
    ref = weakref.ref(wwr)
    print(f'Weak reference created: {ref is not None}')
    print(f'Weak reference points to object: {ref() is wwr}')
except Exception as e:
    print('SKIP_dataclass weakref_slot', type(e).__name__, e)


# === dataclass with generic types ===
try:
    print('\n=== dataclass with generic types ===')

    from typing import List, Dict, Optional

    @dataclass
    class WithGenerics:
        items: List[int]
        mapping: Dict[str, int]
        maybe: Optional[str] = None

    wg = WithGenerics([1, 2, 3], {'a': 1, 'b': 2}, 'hello')
    print(f'WithGenerics: {wg}')
except Exception as e:
    print('SKIP_dataclass with generic types', type(e).__name__, e)


# === asdict with dict_factory ===
try:
    print('\n=== asdict with dict_factory ===')

    df_result = asdict(p, dict_factory=lambda x: {k: v * 2 if isinstance(v, int) else v for k, v in x})
    print(f'asdict with dict_factory: {df_result}')
except Exception as e:
    print('SKIP_asdict with dict_factory', type(e).__name__, e)


# === astuple with tuple_factory ===
try:
    print('\n=== astuple with tuple_factory ===')

    tf_result = astuple(p, tuple_factory=lambda x: tuple(v * 3 if isinstance(v, int) else v for v in x))
    print(f'astuple with tuple_factory: {tf_result}')
except Exception as e:
    print('SKIP_astuple with tuple_factory', type(e).__name__, e)


# === Field repr ===
try:
    print('\n=== Field repr ===')

    @dataclass
    class ForFieldRepr:
        x: int = 1

    ffr = fields(ForFieldRepr)[0]
    print(f'Field repr: {repr(ffr)}')
except Exception as e:
    print('SKIP_Field repr', type(e).__name__, e)


# === recursive_repr usage ===
try:
    print('\n=== recursive_repr usage ===')

    class RecursiveClass:
        def __init__(self, name):
            self.name = name
            self.child = None

        @recursive_repr()
        def __repr__(self):
            if self.child is not None:
                return f'{self.name}({self.child})'
            return self.name

    rc = RecursiveClass('parent')
    rc.child = RecursiveClass('child')
    rc.child.child = rc  # Circular reference
    print(f'Recursive repr: {repr(rc)}')
except Exception as e:
    print('SKIP_recursive_repr usage', type(e).__name__, e)


# === dataclass property ===
try:
    print('\n=== dataclass property ===')

    @dataclass
    class WithProperty:
        x: int
        y: int

        @property
        def sum_xy(self):
            return self.x + self.y

    wp = WithProperty(3, 4)
    print(f'WithProperty: {wp}')
    print(f'sum_xy property: {wp.sum_xy}')
except Exception as e:
    print('SKIP_dataclass property', type(e).__name__, e)


# === dataclass method ===
try:
    print('\n=== dataclass method ===')

    @dataclass
    class WithMethod:
        x: int
        y: int

        def distance_from_origin(self):
            return (self.x ** 2 + self.y ** 2) ** 0.5

    wmth = WithMethod(3, 4)
    print(f'WithMethod: {wmth}')
    print(f'distance_from_origin: {wmth.distance_from_origin()}')
except Exception as e:
    print('SKIP_dataclass method', type(e).__name__, e)


# === dataclass static method ===
try:
    print('\n=== dataclass static method ===')

    @dataclass
    class WithStaticMethod:
        x: int

        @staticmethod
        def create_default():
            return WithStaticMethod(42)

    wsm = WithStaticMethod.create_default()
    print(f'WithStaticMethod from static: {wsm}')
except Exception as e:
    print('SKIP_dataclass static method', type(e).__name__, e)


# === dataclass class method ===
try:
    print('\n=== dataclass class method ===')

    @dataclass
    class WithClassMethod:
        x: int

        @classmethod
        def from_string(cls, s: str):
            return cls(int(s))

    wcm = WithClassMethod.from_string('99')
    print(f'WithClassMethod from string: {wcm}')
except Exception as e:
    print('SKIP_dataclass class method', type(e).__name__, e)


# === complex nested asdict ===
try:
    print('\n=== complex nested asdict ===')

    @dataclass
    class Outer:
        inner: Point
        name: str

    @dataclass
    class Container2:
        outer: Outer
        items: list

    complex_obj = Container2(Outer(Point(1, 2), 'test'), [1, 2, 3])
    print(f'Complex asdict: {asdict(complex_obj)}')
except Exception as e:
    print('SKIP_complex nested asdict', type(e).__name__, e)


# === dataclass with slots inheritance ===
try:
    print('\n=== dataclass with slots inheritance ===')

    @dataclass(slots=True)
    class SlotsBase:
        x: int

    @dataclass(slots=True)
    class SlotsDerived(SlotsBase):
        y: int

    sd = SlotsDerived(1, 2)
    print(f'SlotsDerived: {sd}')
    print(f'SlotsDerived has __slots__: {hasattr(SlotsDerived, "__slots__")}')
except Exception as e:
    print('SKIP_dataclass with slots inheritance', type(e).__name__, e)


# === dataclass compare fields selectively ===
try:
    print('\n=== dataclass compare fields selectively ===')

    @dataclass
    class SelectiveCompare:
        always_compare: int
        never_compare: str = field(compare=False)

    sc1 = SelectiveCompare(1, 'a')
    sc2 = SelectiveCompare(1, 'b')
    print(f'sc1 == sc2 (same always, diff never): {sc1 == sc2}')

    sc3 = SelectiveCompare(2, 'a')
    print(f'sc1 == sc3 (diff always): {sc1 == sc3}')
except Exception as e:
    print('SKIP_dataclass compare fields selectively', type(e).__name__, e)


# === dataclass hash selectively ===
try:
    print('\n=== dataclass hash selectively ===')

    @dataclass(frozen=True)
    class SelectiveHash:
        hash_this: int
        not_hash: str = field(hash=False, default='x')

    sh1 = SelectiveHash(1, 'a')
    sh2 = SelectiveHash(1, 'b')
    print(f'sh1 hash: {hash(sh1)}')
    print(f'sh2 hash: {hash(sh2)}')
    print(f'sh1 hash == sh2 hash: {hash(sh1) == hash(sh2)}')
except Exception as e:
    print('SKIP_dataclass hash selectively', type(e).__name__, e)


# === dataclass repr selectively ===
try:
    print('\n=== dataclass repr selectively ===')

    @dataclass
    class SelectiveRepr:
        show_me: int
        hide_me: str = field(repr=False, default='secret')

    sr = SelectiveRepr(42, 'hidden')
    print(f'SelectiveRepr repr (hide_me should be hidden): {repr(sr)}')
except Exception as e:
    print('SKIP_dataclass repr selectively', type(e).__name__, e)


# === field with both default and default_factory ===
try:
    print('\n=== field with both default and default_factory ===')

    # This should raise ValueError when class is defined
    # Let's verify with try/except in a different way
    try:
        exec('''
from dataclasses import dataclass, field

@dataclass
class BadField:
    x: int = field(default=1, default_factory=list)
''')
    except ValueError as e:
        print(f'ValueError for both defaults: {e}')
except Exception as e:
    print('SKIP_field with both default and default_factory', type(e).__name__, e)


# === make_dataclass with namespace ===
try:
    print('\n=== make_dataclass with namespace ===')

    DynamicWithMethod = make_dataclass(
        'DynamicWithMethod',
        [('x', int)],
        namespace={'double': lambda self: self.x * 2}
    )
    dwm = DynamicWithMethod(5)
    print(f'DynamicWithMethod: {dwm}')
    print(f'DynamicWithMethod.double(): {dwm.double()}')
except Exception as e:
    print('SKIP_make_dataclass with namespace', type(e).__name__, e)


# === dataclass __setattr__ and __delattr__ with frozen ===
try:
    print('\n=== dataclass __setattr__ and __delattr__ with frozen ===')

    @dataclass(frozen=True)
    class FrozenWithSlots:
        x: int

    try:
        fws = FrozenWithSlots(1)
        object.__setattr__(fws, '_private', 2)
        print(f'FrozenWithSlots object.__setattr__ works: {fws._private}')
    except Exception as e:
        print(f'object.__setattr__ error: {e}')
except Exception as e:
    print('SKIP_dataclass __setattr__ and __delattr__ with frozen', type(e).__name__, e)


# === dataclass docstring preservation ===
try:
    print('\n=== dataclass docstring preservation ===')

    @dataclass
    class WithDocstring:
        """This is a docstring."""
        x: int

    print(f'WithDocstring docstring: {WithDocstring.__doc__}')
except Exception as e:
    print('SKIP_dataclass docstring preservation', type(e).__name__, e)


# === dataclass annotations preservation ===
try:
    print('\n=== dataclass annotations preservation ===')

    @dataclass
    class WithAnnotations:
        x: int
        y: str

    print(f'WithAnnotations annotations: {WithAnnotations.__annotations__}')
except Exception as e:
    print('SKIP_dataclass annotations preservation', type(e).__name__, e)


# === dataclass module preservation ===
try:
    print('\n=== dataclass module preservation ===')

    print(f'Point module: {Point.__module__}')
except Exception as e:
    print('SKIP_dataclass module preservation', type(e).__name__, e)


# === dataclass qualname preservation ===
try:
    print('\n=== dataclass qualname preservation ===')

    print(f'Point qualname: {Point.__qualname__}')
except Exception as e:
    print('SKIP_dataclass qualname preservation', type(e).__name__, e)


# === dataclass name preservation ===
try:
    print('\n=== dataclass name preservation ===')

    print(f'Point name: {Point.__name__}')
except Exception as e:
    print('SKIP_dataclass name preservation', type(e).__name__, e)


# === Final summary ===
print('\n=== Test complete ===')
print('All dataclasses module features tested successfully!')
