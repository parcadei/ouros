import operator

# === abs ===
try:
    print('abs_positive', operator.abs(5))
    print('abs_negative', operator.abs(-5))
    print('abs_zero', operator.abs(0))
    print('abs_float', operator.abs(-3.14))
except Exception as e:
    print('SKIP_abs', type(e).__name__, e)

# === add ===
try:
    print('add_int', operator.add(1, 2))
    print('add_float', operator.add(1.5, 2.5))
    print('add_string', operator.add('a', 'b'))
    print('add_list', operator.add([1], [2]))
except Exception as e:
    print('SKIP_add', type(e).__name__, e)

# === sub ===
try:
    print('sub_int', operator.sub(5, 3))
    print('sub_float', operator.sub(5.5, 2.2))
    print('sub_negative', operator.sub(3, 5))
except Exception as e:
    print('SKIP_sub', type(e).__name__, e)

# === mul ===
try:
    print('mul_int', operator.mul(3, 4))
    print('mul_float', operator.mul(2.5, 4.0))
    print('mul_seq', operator.mul([1, 2], 2))
    print('mul_str', operator.mul('a', 3))
except Exception as e:
    print('SKIP_mul', type(e).__name__, e)

# === matmul ===
try:
    class Mat:
        def __init__(self, val):
            self.val = val
        def __matmul__(self, other):
            # Simple matrix multiplication for 2x2 matrices represented as nested lists
            a, b, c, d = self.val[0][0], self.val[0][1], self.val[1][0], self.val[1][1]
            e, f, g, h = other.val[0][0], other.val[0][1], other.val[1][0], other.val[1][1]
            return Mat([[a*e + b*g, a*f + b*h], [c*e + d*g, c*f + d*h]])
        def __repr__(self):
            return f'Mat({self.val})'

    print('matmul', operator.matmul(Mat([[1, 2], [3, 4]]), Mat([[5, 6], [7, 8]])))
except Exception as e:
    print('SKIP_matmul', type(e).__name__, e)

# === truediv ===
try:
    print('truediv_int', operator.truediv(7, 2))
    print('truediv_float', operator.truediv(7.0, 2.0))
    print('truediv_result', operator.truediv(1, 2))
except Exception as e:
    print('SKIP_truediv', type(e).__name__, e)

# === floordiv ===
try:
    print('floordiv_int', operator.floordiv(7, 2))
    print('floordiv_negative', operator.floordiv(-7, 2))
    print('floordiv_float', operator.floordiv(7.0, 2.0))
except Exception as e:
    print('SKIP_floordiv', type(e).__name__, e)

# === mod ===
try:
    print('mod_int', operator.mod(7, 3))
    print('mod_float', operator.mod(7.5, 2.5))
    print('mod_negative', operator.mod(-7, 3))
except Exception as e:
    print('SKIP_mod', type(e).__name__, e)

# === pow ===
try:
    print('pow_int', operator.pow(2, 3))
    print('pow_zero', operator.pow(5, 0))
    print('pow_large', operator.pow(10, 3))
except Exception as e:
    print('SKIP_pow', type(e).__name__, e)

# === neg ===
try:
    print('neg_int', operator.neg(5))
    print('neg_negative', operator.neg(-5))
    print('neg_float', operator.neg(3.14))
except Exception as e:
    print('SKIP_neg', type(e).__name__, e)

# === pos ===
try:
    print('pos_int', operator.pos(5))
    print('pos_negative', operator.pos(-5))
    print('pos_float', operator.pos(3.14))
except Exception as e:
    print('SKIP_pos', type(e).__name__, e)

# === invert ===
try:
    print('invert', operator.invert(5))
    print('invert_zero', operator.invert(0))
    print('invert_neg', operator.invert(-1))
    print('inv_alias', operator.inv(5))
except Exception as e:
    print('SKIP_invert', type(e).__name__, e)

# === lshift ===
try:
    print('lshift', operator.lshift(1, 3))
    print('lshift_zero', operator.lshift(5, 0))
except Exception as e:
    print('SKIP_lshift', type(e).__name__, e)

# === rshift ===
try:
    print('rshift', operator.rshift(8, 2))
    print('rshift_zero', operator.rshift(5, 0))
except Exception as e:
    print('SKIP_rshift', type(e).__name__, e)

# === and_ ===
try:
    print('and_', operator.and_(0b1100, 0b1010))
    print('and__zero', operator.and_(0b1111, 0))
    print('and__same', operator.and_(0b1111, 0b1111))
except Exception as e:
    print('SKIP_and_', type(e).__name__, e)

# === or_ ===
try:
    print('or_', operator.or_(0b1100, 0b1010))
    print('or__zero', operator.or_(0b1010, 0))
    print('or__same', operator.or_(0b1010, 0b0101))
except Exception as e:
    print('SKIP_or_', type(e).__name__, e)

# === xor ===
try:
    print('xor', operator.xor(0b1100, 0b1010))
    print('xor_zero', operator.xor(0b1111, 0))
    print('xor_same', operator.xor(0b1111, 0b1111))
except Exception as e:
    print('SKIP_xor', type(e).__name__, e)

# === eq ===
try:
    print('eq_true', operator.eq(1, 1))
    print('eq_false', operator.eq(1, 2))
    print('eq_str', operator.eq('a', 'a'))
except Exception as e:
    print('SKIP_eq', type(e).__name__, e)

# === ne ===
try:
    print('ne_true', operator.ne(1, 2))
    print('ne_false', operator.ne(1, 1))
    print('ne_str', operator.ne('a', 'b'))
except Exception as e:
    print('SKIP_ne', type(e).__name__, e)

# === lt ===
try:
    print('lt_true', operator.lt(1, 2))
    print('lt_false', operator.lt(2, 1))
    print('lt_equal', operator.lt(1, 1))
except Exception as e:
    print('SKIP_lt', type(e).__name__, e)

# === le ===
try:
    print('le_true', operator.le(1, 2))
    print('le_equal', operator.le(1, 1))
    print('le_false', operator.le(2, 1))
except Exception as e:
    print('SKIP_le', type(e).__name__, e)

# === gt ===
try:
    print('gt_true', operator.gt(2, 1))
    print('gt_false', operator.gt(1, 2))
    print('gt_equal', operator.gt(1, 1))
except Exception as e:
    print('SKIP_gt', type(e).__name__, e)

# === ge ===
try:
    print('ge_true', operator.ge(2, 1))
    print('ge_equal', operator.ge(1, 1))
    print('ge_false', operator.ge(1, 2))
except Exception as e:
    print('SKIP_ge', type(e).__name__, e)

# === concat ===
try:
    print('concat_list', operator.concat([1, 2], [3, 4]))
    print('concat_tuple', operator.concat((1, 2), (3, 4)))
    print('concat_str', operator.concat('ab', 'cd'))
except Exception as e:
    print('SKIP_concat', type(e).__name__, e)

# === contains ===
try:
    print('contains_true', operator.contains([1, 2, 3], 2))
    print('contains_false', operator.contains([1, 2, 3], 4))
    print('contains_str', operator.contains('hello', 'ell'))
    print('contains_dict', operator.contains({1: 'a', 2: 'b'}, 1))
except Exception as e:
    print('SKIP_contains', type(e).__name__, e)

# === countOf ===
try:
    print('countOf_list', operator.countOf([1, 2, 2, 3, 2], 2))
    print('countOf_zero', operator.countOf([1, 2, 3], 4))
    print('countOf_str', operator.countOf('banana', 'a'))
    print('countOf_tuple', operator.countOf((1, 1, 1, 2), 1))
except Exception as e:
    print('SKIP_countOf', type(e).__name__, e)

# === indexOf ===
try:
    print('indexOf_list', operator.indexOf([1, 2, 3, 2], 2))
    print('indexOf_first', operator.indexOf([1, 2, 3], 3))
    print('indexOf_str', operator.indexOf('hello', 'l'))
    print('indexOf_tuple', operator.indexOf((1, 2, 3), 2))
except Exception as e:
    print('SKIP_indexOf', type(e).__name__, e)

# === getitem ===
try:
    print('getitem_list', operator.getitem([1, 2, 3], 1))
    print('getitem_dict', operator.getitem({'a': 1, 'b': 2}, 'a'))
    print('getitem_str', operator.getitem('hello', 1))
    print('getitem_slice', operator.getitem([1, 2, 3, 4], slice(1, 3)))
except Exception as e:
    print('SKIP_getitem', type(e).__name__, e)

# === setitem ===
try:
    lst = [1, 2, 3]
    operator.setitem(lst, 1, 10)
    print('setitem_list', lst)
    d = {'a': 1}
    operator.setitem(d, 'b', 2)
    print('setitem_dict', d)
    lst2 = [[1, 2], [3, 4]]
    operator.setitem(lst2, 0, [5, 6])
    print('setitem_nested', lst2)
except Exception as e:
    print('SKIP_setitem', type(e).__name__, e)

# === delitem ===
try:
    lst = [1, 2, 3, 4]
    operator.delitem(lst, 1)
    print('delitem_list', lst)
    d = {'a': 1, 'b': 2}
    operator.delitem(d, 'a')
    print('delitem_dict', d)
except Exception as e:
    print('SKIP_delitem', type(e).__name__, e)

# === not_ ===
try:
    print('not_true', operator.not_(True))
    print('not_false', operator.not_(False))
    print('not_zero', operator.not_(0))
    print('not_one', operator.not_(1))
    print('not_empty', operator.not_(''))
except Exception as e:
    print('SKIP_not_', type(e).__name__, e)

# === truth ===
try:
    print('truth_true', operator.truth(True))
    print('truth_false', operator.truth(False))
    print('truth_zero', operator.truth(0))
    print('truth_one', operator.truth(1))
    print('truth_empty', operator.truth(''))
except Exception as e:
    print('SKIP_truth', type(e).__name__, e)

# === is_ ===
try:
    a = [1, 2]
    b = a
    c = [1, 2]
    print('is_true', operator.is_(a, b))
    print('is_false', operator.is_(a, c))
    print('is_none_true', operator.is_(None, None))
except Exception as e:
    print('SKIP_is_', type(e).__name__, e)

# === is_not ===
try:
    print('is_not_true', operator.is_not(a, c))
    print('is_not_false', operator.is_not(a, b))
    print('is_not_none_true', operator.is_not(1, None))
except Exception as e:
    print('SKIP_is_not', type(e).__name__, e)

# === is_none ===
try:
    print('is_none_yes', operator.is_none(None))
    print('is_none_no', operator.is_none(0))
    print('is_none_empty', operator.is_none(''))
except Exception as e:
    print('SKIP_is_none', type(e).__name__, e)

# === is_not_none ===
try:
    print('is_not_none_yes', operator.is_not_none(0))
    print('is_not_none_no', operator.is_not_none(None))
    print('is_not_none_empty', operator.is_not_none(''))
except Exception as e:
    print('SKIP_is_not_none', type(e).__name__, e)

# === index ===
try:
    class MyIndex:
        def __index__(self):
            return 42
    print('index', operator.index(MyIndex()))
    print('index_int', operator.index(5))
    print('index_bool', operator.index(True))
except Exception as e:
    print('SKIP_index', type(e).__name__, e)

# === length_hint ===
try:
    print('length_hint_list', operator.length_hint([1, 2, 3]))
    print('length_hint_empty', operator.length_hint([]))
    print('length_hint_default', operator.length_hint(object(), 10))
except Exception as e:
    print('SKIP_length_hint', type(e).__name__, e)

# === call ===
try:
    def func(a, b, c=0):
        return a + b + c
    print('call', operator.call(func, 1, 2, c=3))
    print('call_no_kw', operator.call(func, 1, 2))
    print('call_lambda', operator.call(lambda x: x * 2, 5))
except Exception as e:
    print('SKIP_call', type(e).__name__, e)

# === itemgetter ===
try:
    data = [('a', 1), ('b', 2), ('c', 3)]
    get_first = operator.itemgetter(0)
    print('itemgetter_single', get_first(data[0]))
    get_both = operator.itemgetter(0, 1)
    print('itemgetter_multi', get_both(data[0]))
    get_slice = operator.itemgetter(slice(1, 3))
    print('itemgetter_slice', get_slice([1, 2, 3, 4, 5]))
except Exception as e:
    print('SKIP_itemgetter', type(e).__name__, e)

# === attrgetter ===
try:
    class Obj:
        def __init__(self):
            self.x = 1
            self.y = 2
            self.name = 'test'

    obj = Obj()
    get_x = operator.attrgetter('x')
    print('attrgetter_single', get_x(obj))
    get_xy = operator.attrgetter('x', 'y')
    print('attrgetter_multi', get_xy(obj))

    # Nested attrgetter
    class Container:
        def __init__(self):
            self.inner = obj

    container = Container()
    get_inner_x = operator.attrgetter('inner.x')
    print('attrgetter_nested', get_inner_x(container))
except Exception as e:
    print('SKIP_attrgetter', type(e).__name__, e)

# === methodcaller ===
try:
    class Counter:
        def __init__(self):
            self.count = 0
        def increment(self, n=1):
            self.count += n
            return self.count
        def reset(self):
            self.count = 0
            return self.count

    counter = Counter()
    call_inc = operator.methodcaller('increment')
    print('methodcaller_noargs', call_inc(counter))
    call_inc2 = operator.methodcaller('increment', 5)
    print('methodcaller_withargs', call_inc2(counter))
    call_reset = operator.methodcaller('reset')
    print('methodcaller_reset', call_reset(counter))
except Exception as e:
    print('SKIP_methodcaller', type(e).__name__, e)

# === in-place operators ===
# iadd
try:
    lst = [1, 2]
    print('iadd_list', operator.iadd(lst, [3, 4]), lst)
    n = 5
    print('iadd_int', operator.iadd(n, 3))
except Exception as e:
    print('SKIP_iadd', type(e).__name__, e)

# iand
try:
    x = 0b1111
    print('iand', operator.iand(x, 0b1010))
except Exception as e:
    print('SKIP_iand', type(e).__name__, e)

# iconcat
try:
    lst = [1, 2]
    print('iconcat', operator.iconcat(lst, [3, 4]), lst)
except Exception as e:
    print('SKIP_iconcat', type(e).__name__, e)

# ifloordiv
try:
    print('ifloordiv', operator.ifloordiv(7, 2))
except Exception as e:
    print('SKIP_ifloordiv', type(e).__name__, e)

# ilshift
try:
    print('ilshift', operator.ilshift(1, 3))
except Exception as e:
    print('SKIP_ilshift', type(e).__name__, e)

# imatmul
try:
    print('imatmul', operator.imatmul(Mat([[1, 2], [3, 4]]), Mat([[5, 6], [7, 8]])))
except Exception as e:
    print('SKIP_imatmul', type(e).__name__, e)

# imod
try:
    print('imod', operator.imod(7, 3))
except Exception as e:
    print('SKIP_imod', type(e).__name__, e)

# imul
try:
    lst = [1, 2]
    print('imul_list', operator.imul(lst, 2), lst)
    print('imul_int', operator.imul(3, 4))
except Exception as e:
    print('SKIP_imul', type(e).__name__, e)

# ior
try:
    print('ior', operator.ior(0b1010, 0b0101))
except Exception as e:
    print('SKIP_ior', type(e).__name__, e)

# ipow
try:
    print('ipow', operator.ipow(2, 3))
except Exception as e:
    print('SKIP_ipow', type(e).__name__, e)

# irshift
try:
    print('irshift', operator.irshift(8, 2))
except Exception as e:
    print('SKIP_irshift', type(e).__name__, e)

# isub
try:
    print('isub', operator.isub(5, 3))
except Exception as e:
    print('SKIP_isub', type(e).__name__, e)

# itruediv
try:
    print('itruediv', operator.itruediv(7, 2))
except Exception as e:
    print('SKIP_itruediv', type(e).__name__, e)

# ixor
try:
    print('ixor', operator.ixor(0b1111, 0b1010))
except Exception as e:
    print('SKIP_ixor', type(e).__name__, e)
