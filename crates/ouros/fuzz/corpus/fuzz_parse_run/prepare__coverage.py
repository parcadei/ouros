# === Augmented assignment to subscript (OpAssignSubscr) ===
# Exercises lines 442-458 in prepare.rs
data = [10, 20, 30]
data[0] += 5
assert data[0] == 15, 'augmented assign subscript +='

data[1] -= 3
assert data[1] == 17, 'augmented assign subscript -='

data[2] *= 2
assert data[2] == 60, 'augmented assign subscript *='

d = {'a': 10, 'b': 20}
d['a'] += 100
assert d['a'] == 110, 'augmented assign dict subscript +='

d['b'] //= 3
assert d['b'] == 6, 'augmented assign dict subscript //='

# Nested augmented subscript assignment
nested = [[1, 2], [3, 4]]
nested[0][1] += 10
assert nested[0][1] == 12, 'nested augmented subscript assign'


# === Delete subscript (DeleteSubscr) ===
# Exercises lines 493+ (DeleteSubscr path) in prepare.rs
d2 = {'key': 'value', 'other': 'data'}
del d2['key']
assert 'key' not in d2, 'dict key deleted'
assert d2 == {'other': 'data'}, 'dict after del subscript'


# === Delete attribute (DeleteAttr) ===
class DelAttrTest:
    pass


obj = DelAttrTest()
obj.x = 10
assert obj.x == 10, 'attr exists before del'
del obj.x
try:
    _ = obj.x
    assert False, 'should have raised AttributeError'
except AttributeError:
    pass


# === Global declaration read ===
# Exercises line 352 (ReturnNone) and global path in prepare.rs
g_val = 'global'


def fn_global_read():
    global g_val
    return g_val


assert fn_global_read() == 'global', 'global read in function'


# === Return None (ReturnNone) ===
# Exercises line 352 in prepare.rs
def returns_none():
    return


assert returns_none() is None, 'return without value returns None'


# === Nonlocal augmented assign ===
# Exercises lines 626-651 nonlocal path in prepare.rs
def outer_nonlocal():
    val = 10

    def inner():
        nonlocal val
        val += 5
        return val

    return inner()


assert outer_nonlocal() == 15, 'nonlocal augmented assign'


# === Starred unpack target in for loop ===
# Exercises lines 1112-1126 in prepare.rs
result = []
for first, *rest in [(1, 2, 3), (4, 5, 6)]:
    result.append((first, rest))
assert result == [(1, [2, 3]), (4, [5, 6])], 'starred unpack in for loop'


# === Tuple unpack target in comprehension ===
# Exercises lines 1127-1137 in prepare.rs
comp_result = [a + b for a, b in [(1, 2), (3, 4), (5, 6)]]
assert comp_result == [3, 7, 11], 'tuple unpack in comprehension'


# === Lambda closure capture ===
# Exercises lines 1662-1671 (lambda free var capture) in prepare.rs
def make_lambda_closure():
    x = 100

    def helper():
        return (lambda: x)()

    return helper()


assert make_lambda_closure() == 100, 'lambda captures enclosing variable'


# Lambda with default that references enclosing variable
def lambda_default_capture():
    val = 42
    f = lambda x=val: x
    return f()


assert lambda_default_capture() == 42, 'lambda default from enclosing scope'


# Lambda nested closure (exercises free_var pass-through)
def outer_lambda():
    x = 10

    def middle():
        return lambda: x

    return middle()()


assert outer_lambda() == 10, 'lambda pass-through closure capture'


# === Positional-only parameters with defaults ===
# Exercises line 1428 in prepare.rs
def pos_only_default(a, b=5, /):
    return a + b


assert pos_only_default(10) == 15, 'pos-only with default'
assert pos_only_default(10, 20) == 30, 'pos-only override default'


# === Lambda positional-only params with defaults ===
# Exercises lines 1751-1753 in prepare.rs
f_pos = lambda a, b=3, /: a * b
assert f_pos(4) == 12, 'lambda pos-only default'
assert f_pos(4, 5) == 20, 'lambda pos-only override'


# === Implicit closure capture (no nonlocal keyword) ===
# Exercises lines 1956-1969 in prepare.rs
def implicit_closure():
    captured = 'value'

    def reader():
        return captured

    return reader()


assert implicit_closure() == 'value', 'implicit closure capture'


# === Scope info collection from various node types ===
# Exercises lines 2192-2265 in prepare.rs
# These exercise collect_scope_info_from_node for OpAssignSubscr, SubscriptAssign,
# AttrAssign, With, For, While


# OpAssignSubscr inside function with closure
def scope_opassign_subscr():
    data = [1, 2, 3]

    def inner():
        data[0] += 10
        return data

    return inner()


assert scope_opassign_subscr() == [11, 2, 3], 'scope info OpAssignSubscr in closure'


# SubscriptAssign inside function with closure
def scope_subscr_assign():
    d = {'x': 0}

    def inner():
        d['x'] = 42
        return d['x']

    return inner()


assert scope_subscr_assign() == 42, 'scope info SubscriptAssign in closure'


# AttrAssign inside function
def scope_attr_assign():
    class Container:
        pass

    c = Container()

    def inner():
        c.value = 99
        return c.value

    return inner()


assert scope_attr_assign() == 99, 'scope info AttrAssign in closure'


# While loop inside function (exercises scope collection for While)
def scope_while():
    result = 0
    i = 0
    while i < 3:
        result += i
        i += 1
    return result


assert scope_while() == 3, 'scope info While'


# For loop inside function with else
def scope_for_else():
    result = []
    for i in range(3):
        result.append(i)
    else:
        result.append('done')
    return result


assert scope_for_else() == [0, 1, 2, 'done'], 'scope info For with else'


# === Try/except handler name assignment (scope collection) ===
# Exercises line 2301 in prepare.rs
def scope_try_handler():
    try:
        raise ValueError('test')
    except ValueError as e:
        msg = str(e)
    return msg


assert scope_try_handler() == 'test', 'try/except handler name in scope'


# === Dict expression walrus scanning ===
# Exercises lines 2363-2365 in prepare.rs
def walrus_in_dict():
    d = {(k := 'a'): (v := 1)}
    return (k, v, d)


assert walrus_in_dict() == ('a', 1, {'a': 1}), 'walrus in dict inside function'


# === Comprehension walrus scanning ===
# Exercises lines 2413-2425 in prepare.rs
def walrus_in_comp():
    result = [(leak := x) for x in range(3)]
    return (result, leak)


assert walrus_in_comp() == ([0, 1, 2], 2), 'walrus in comprehension leaks to function scope'


# Dict comprehension walrus scanning
def walrus_in_dictcomp():
    result = {(kl := k): v for k, v in [(1, 2), (3, 4)]}
    return (result, kl)


assert walrus_in_dictcomp() == ({1: 2, 3: 4}, 3), 'walrus in dict comprehension'


# === FString walrus scanning ===
# Exercises lines 2427-2433 in prepare.rs
def walrus_in_fstring():
    msg = f'{(x := 42)} is the answer'
    return (msg, x)


assert walrus_in_fstring() == ('42 is the answer', 42), 'walrus in f-string'


# === Slice walrus scanning ===
# Exercises lines 2434-2444 in prepare.rs
def walrus_in_slice():
    data = [0, 1, 2, 3, 4]
    result = data[(start := 1) : (end := 3)]
    return (result, start, end)


assert walrus_in_slice() == ([1, 2], 1, 3), 'walrus in slice'


# === Cell var collection from While ===
# Exercises lines 2558-2564 in prepare.rs
def cell_var_while():
    x = 0

    def inner():
        return x

    while x < 3:
        x += 1
    return inner()


assert cell_var_while() == 3, 'cell var updated in while loop'


# === Cell var collection from OpAssignSubscr in closure ===
# Exercises lines 2610-2615 in prepare.rs
def cell_var_opassign_subscr():
    data = [1, 2, 3]

    def inner():
        return data

    data[0] += 100
    return inner()


assert cell_var_opassign_subscr() == [101, 2, 3], 'cell var with subscript augmented assign'


# === Cell var collection from dict expression ===
# Exercises lines 2696-2698 in prepare.rs
def cell_var_dict():
    k = 'key'
    v = 'val'

    def inner():
        return {k: v}

    return inner()


assert cell_var_dict() == {'key': 'val'}, 'cell var in dict expression'


# === Cell var collection from lambda defaults ===
# Exercises lines 2650-2662 in prepare.rs
def cell_var_lambda_defaults():
    default_val = 10
    f = lambda x=default_val: x
    default_val = 999
    return f()


assert cell_var_lambda_defaults() == 10, 'lambda default captures at definition time'


# === Referenced names from Dict in closure ===
# Exercises lines 2985-2987 in prepare.rs
def referenced_dict():
    a = 'key'
    b = 'value'

    def inner():
        return {a: b}

    return inner()


assert referenced_dict() == {'key': 'value'}, 'referenced names from dict'


# === Referenced names from DictComp in closure ===
# Exercises lines 3045-3046 in prepare.rs
def referenced_dictcomp():
    items = [(1, 2), (3, 4)]

    def inner():
        return {k: v for k, v in items}

    return inner()


assert referenced_dictcomp() == {1: 2, 3: 4}, 'referenced names from dictcomp'


# === Referenced names from Slice in closure ===
# Exercises lines 3093-3102 in prepare.rs
def referenced_slice():
    data = [0, 1, 2, 3, 4, 5]
    start = 1
    end = 4
    step = 2

    def inner():
        return data[start:end:step]

    return inner()


assert referenced_slice() == [1, 3], 'referenced names from slice'


# === Referenced names from lambda body in closure ===
# Exercises lines 3048-3083 in prepare.rs
def referenced_lambda_body():
    x = 10

    def inner():
        f = lambda: x + 1
        return f()

    return inner()


assert referenced_lambda_body() == 11, 'referenced names from lambda body in closure'


# === Lambda that doesn't capture (parameter shadows outer) ===
# Exercises lambda parameter filtering in lines 3048-3065
def lambda_no_capture():
    x = 10

    def inner():
        f = lambda x: x + 1
        return f(5)

    return inner()


assert lambda_no_capture() == 6, 'lambda param shadows outer - no capture'


# === Comprehension referenced names ===
# Exercises lines 3145-3165 in prepare.rs
def comp_ref_names():
    items = [1, 2, 3]
    factor = 10

    def inner():
        return [x * factor for x in items]

    return inner()


assert comp_ref_names() == [10, 20, 30], 'comprehension referenced names in closure'


# Nested comprehension with second generator referencing first loop var
def nested_comp_ref():
    pairs = [[1, 2], [3, 4]]

    def inner():
        return [y for x in pairs for y in x]

    return inner()


assert nested_comp_ref() == [1, 2, 3, 4], 'nested comp second gen references first var'


# === With statement inside function (scope collection) ===
# Exercises lines 2225-2237 in prepare.rs
class SimpleCtx:
    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False


def scope_with():
    with SimpleCtx() as ctx:
        result = 'in context'
    return result


assert scope_with() == 'in context', 'with statement scope collection'


# === Cell var from fstring parts ===
# Exercises lines 2755-2761 in prepare.rs
def cell_var_fstring():
    name = 'world'

    def inner():
        return f'hello {name}'

    return inner()


assert cell_var_fstring() == 'hello world', 'cell var in fstring'


# === Cell var from comprehension conditions ===
# Exercises lines 2741-2753 in prepare.rs
def cell_var_comp():
    threshold = 2

    def inner():
        return [x for x in range(5) if x > threshold]

    return inner()


assert cell_var_comp() == [3, 4], 'cell var in comprehension condition'


# Dict comprehension cell var
def cell_var_dictcomp():
    prefix = 'item_'

    def inner():
        return {prefix + str(k): v for k, v in [(1, 'a'), (2, 'b')]}

    return inner()


assert cell_var_dictcomp() == {'item_1': 'a', 'item_2': 'b'}, 'cell var in dict comprehension'


# === Cell var from named expression (walrus) ===
# Exercises line 2762-2765 in prepare.rs
def cell_var_named():
    x = 0

    def inner():
        return (y := x + 1)

    return inner()


assert cell_var_named() == 1, 'cell var in named expression'


# === Args/Kwargs walrus scanning ===
# Exercises lines 2458-2496 in prepare.rs
def walrus_in_args():
    def identity(x):
        return x

    result = identity((w := 42))
    return (result, w)


assert walrus_in_args() == (42, 42), 'walrus in function arg'


def walrus_in_kwargs():
    def f(x=0, y=0):
        return x + y

    result = f(x=(a := 3), y=(b := 4))
    return (result, a, b)


assert walrus_in_kwargs() == (7, 3, 4), 'walrus in kwargs'


# === Referenced names from With ===
# Exercises lines 2888-2893 in prepare.rs
def ref_with():
    ctx = SimpleCtx()

    def inner():
        with ctx:
            return 'ok'

    return inner()


assert ref_with() == 'ok', 'referenced names from with in closure'


# === Referenced names from For ===
# Exercises lines 2894-2903 in prepare.rs
def ref_for():
    items = [1, 2, 3]

    def inner():
        total = 0
        for x in items:
            total += x
        return total

    return inner()


assert ref_for() == 6, 'referenced names from for in closure'


# === Referenced names from While ===
# Exercises lines 2905-2912 in prepare.rs
def ref_while():
    limit = 3

    def inner():
        i = 0
        while i < limit:
            i += 1
        return i

    return inner()


assert ref_while() == 3, 'referenced names from while in closure'


# === Referenced names from bare raise ===
# Exercises line 2837 in prepare.rs
def ref_bare_raise():
    try:
        raise ValueError('test')
    except ValueError:
        try:
            raise
        except ValueError as e:
            return str(e)


assert ref_bare_raise() == 'test', 'bare raise re-raises'


# === Referenced names from OpAssignSubscr ===
# Exercises lines 2860-2864 in prepare.rs
def ref_opassign_subscr():
    data = [1, 2, 3]

    def inner():
        data[0] += 100
        return data

    return inner()


assert ref_opassign_subscr() == [101, 2, 3], 'referenced names from OpAssignSubscr in closure'


# === Referenced names from SubscriptAssign ===
# Exercises lines 2866-2871 in prepare.rs
def ref_subscr_assign():
    d = {'a': 1}

    def inner():
        d['b'] = 2
        return d

    return inner()


assert ref_subscr_assign() == {'a': 1, 'b': 2}, 'referenced names from SubscriptAssign in closure'


# === Referenced names from AttrAssign ===
# Exercises lines 2873-2875 in prepare.rs
def ref_attr_assign():
    class Box:
        pass

    b = Box()

    def inner():
        b.val = 'hello'
        return b.val

    return inner()


assert ref_attr_assign() == 'hello', 'referenced names from AttrAssign in closure'


# === Referenced names from DeleteAttr ===
# Exercises lines 2881-2882 in prepare.rs
def ref_del_attr():
    class Box:
        pass

    b = Box()
    b.val = 42

    def inner():
        result = b.val
        del b.val
        return result

    return inner()


assert ref_del_attr() == 42, 'referenced names from DeleteAttr in closure'


# === Referenced names from DeleteSubscr ===
# Exercises lines 2884-2886 in prepare.rs
def ref_del_subscr():
    d = {'a': 1, 'b': 2}

    def inner():
        del d['a']
        return d

    return inner()


assert ref_del_subscr() == {'b': 2}, 'referenced names from DeleteSubscr in closure'


# === Global identifier resolution in non-module scope ===
# Exercises line 1897 in prepare.rs
counter_global = 0


def test_global_ident():
    global counter_global
    counter_global = counter_global + 1
    return counter_global


assert test_global_ident() == 1, 'global identifier resolution'
assert test_global_ident() == 2, 'global identifier resolution second call'


# === Cell var from OpAssignAttr ===
# Exercises lines 2605-2607 in prepare.rs
def cell_var_opassign_attr():
    class Box:
        def __init__(self, v):
            self.v = v

    b = Box(10)

    def inner():
        return b.v

    b.v = 20
    return inner()


assert cell_var_opassign_attr() == 20, 'cell var with attr assignment'


# === Cell var from SubscriptAssign ===
# Exercises lines 2616-2622 in prepare.rs
def cell_var_subscr_assign():
    data = {'x': 1}

    def inner():
        return data['x']

    data['x'] = 999
    return inner()


assert cell_var_subscr_assign() == 999, 'cell var with subscript assign'


# === Cell var from AttrAssign ===
# Exercises lines 2623-2624 in prepare.rs
def cell_var_attr_assign():
    class Box:
        pass

    b = Box()
    b.v = 10

    def inner():
        return b.v

    b.v = 42
    return inner()


assert cell_var_attr_assign() == 42, 'cell var with attr assign'


# === IfElse walrus scanning ===
# Exercises lines 2402-2405 in prepare.rs
def walrus_in_ifelse():
    result = (x := 5) if True else 10
    return (result, x)


assert walrus_in_ifelse() == (5, 5), 'walrus in if-else expression'


# === Cell var from IfElse expression ===
# Exercises lines 2731-2734 in prepare.rs
def cell_var_ifelse():
    flag = True
    a = 10
    b = 20

    def inner():
        return a if flag else b

    return inner()


assert cell_var_ifelse() == 10, 'cell var in if-else expression'


# === Set comprehension walrus scanning ===
# Exercises lines 2408-2415 (SetComp branch) in prepare.rs
def walrus_in_setcomp():
    result = {(leak := x) for x in [1, 2, 3]}
    return leak


assert walrus_in_setcomp() == 3, 'walrus in set comprehension leaks'


# === Cell var collection from SetComp ===
# Exercises lines 2736-2743 in prepare.rs
def cell_var_setcomp():
    threshold = 2

    def inner():
        return {x for x in range(5) if x > threshold}

    return inner()


assert cell_var_setcomp() == {3, 4}, 'cell var in set comprehension'


# === Walrus in two-arg function call ===
# Exercises ArgExprs::Two path in lines 2461-2463 in prepare.rs
def walrus_two_args():
    def add(a, b):
        return a + b

    result = add((x := 3), (y := 4))
    return (result, x, y)


assert walrus_two_args() == (7, 3, 4), 'walrus in two-arg call'


# === Cell var from ChainCmp ===
# Exercises lines 2704-2706 in prepare.rs
def cell_var_chaincmp():
    lo = 0
    hi = 10

    def inner():
        return lo < 5 < hi

    return inner()


assert cell_var_chaincmp() is True, 'cell var in chain comparison'


# === Cell var from Not expression ===
# Exercises lines 2709-2711 in prepare.rs
def cell_var_not():
    flag = False

    def inner():
        return not flag

    return inner()


assert cell_var_not() is True, 'cell var in not expression'


# === Referenced names from OpAssignAttr ===
# Exercises lines 2855-2857 in prepare.rs
def ref_opassign_attr():
    class Box:
        def __init__(self, v):
            self.v = v

    b = Box(10)

    def inner():
        b.v = b.v + 5
        return b.v

    return inner()


assert ref_opassign_attr() == 15, 'referenced names from OpAssignAttr in closure'


# === Cell var from Await (exercises line 2766-2768) ===
# Skipped: requires # run-async marker


# === Walrus in var_args / var_kwargs ===
# Exercises lines 2491-2496 in prepare.rs
def walrus_in_star_call():
    def f(*args, **kwargs):
        return (args, kwargs)

    items = [(w := 1), 2, 3]
    result = f(*items)
    return (result, w)


assert walrus_in_star_call() == (((1, 2, 3), {}), 1), 'walrus in star call'


# === Referenced names from fstring in closure ===
# Exercises lines 3002-3003 in prepare.rs
def ref_fstring():
    name = 'world'

    def inner():
        return f'hello {name}'

    return inner()


assert ref_fstring() == 'hello world', 'referenced names from fstring in closure'
