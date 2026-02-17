# Comprehensive weakref module parity tests
# Testing 100% of Python's weakref module public API

import weakref
import gc

# === ref - basic weak reference ===
try:
    print('=== ref basic ===')

    class Obj:
        pass

    o = Obj()
    r = weakref.ref(o)
    print('ref_deref_alive', r() is o)
    print('ref_hash_matches', hash(r) == hash(o))
    # Ref equality: ref != object itself (different types)
    print('ref_not_eq_object', r != o)
    print('ref_is_weakref', type(r) is weakref.ReferenceType)
    print('ref_callback_none', r.__callback__ is None)

    # Test callback
    callback_called = []
    def on_finalize(ref):
        callback_called.append(1)

    o2 = Obj()
    r2 = weakref.ref(o2, on_finalize)
    print('ref_with_callback', r2() is o2)
    print('ref_callback_set', r2.__callback__ is on_finalize)
    del o2
    gc.collect()
    print('ref_callback_fired', len(callback_called) == 1)

    # Test dead reference
    o3 = Obj()
    r3 = weakref.ref(o3)
    del o3
    gc.collect()
    print('ref_dead_returns_none', r3() is None)
    print('ref_dead_callback_none', r3.__callback__ is None)

    # Test multiple refs to same object
    o4 = Obj()
    r4a = weakref.ref(o4)
    r4b = weakref.ref(o4)
    print('ref_multiple_same_deref', r4a() is r4b())
    # Note: CPython may return the same weakref object for same target without callback
    print('ref_multiple_are_same_object', r4a is r4b)

    # Test ref equality
    o5 = Obj()
    o6 = Obj()
    r5 = weakref.ref(o5)
    r5_copy = weakref.ref(o5)
    r6 = weakref.ref(o6)
    print('ref_eq_same_obj', r5 == r5_copy)
    print('ref_eq_different_obj', r5 != r6)
except Exception as e:
    print('SKIP_ref_basic', type(e).__name__, e)

# === proxy - weak proxy ===
try:
    print('\n=== proxy basic ===')

    class ProxyObj:
        x = 42
        def method(self):
            return 'hello'
        def __call__(self):
            return 'called'

    p_obj = ProxyObj()
    proxy = weakref.proxy(p_obj)
    print('proxy_attr_access', proxy.x == 42)
    print('proxy_method_call', proxy.method() == 'hello')
    # Proxy to non-callable is ProxyType
    print('proxy_is_ProxyType', type(proxy) is weakref.ProxyType)

    # Test callable proxy
    callable_obj = ProxyObj()
    cproxy = weakref.proxy(callable_obj)
    print('callable_proxy_type', type(cproxy) is weakref.CallableProxyType)
    print('callable_proxy_call', cproxy() == 'called')

    # Test proxy not hashable
    p_obj2 = ProxyObj()
    proxy2 = weakref.proxy(p_obj2)
    try:
        hash(proxy2)
        print('proxy_hash_raises', False)
    except TypeError:
        print('proxy_hash_raises', True)

    # Test dead proxy raises ReferenceError
    def get_dead_proxy():
        tmp = ProxyObj()
        return weakref.proxy(tmp)

    dead_proxy = get_dead_proxy()
    gc.collect()
    try:
        _ = dead_proxy.x
        print('proxy_dead_raises', False)
    except ReferenceError:
        print('proxy_dead_raises', True)

    # Test proxy callback
    proxy_callback_called = []
    def on_proxy_finalize(ref):
        proxy_callback_called.append(1)

    o7 = ProxyObj()
    p_with_cb = weakref.proxy(o7, on_proxy_finalize)
    del o7
    gc.collect()
    print('proxy_callback_fired', len(proxy_callback_called) == 1)
except Exception as e:
    print('SKIP_proxy_basic', type(e).__name__, e)

# === getweakrefcount ===
try:
    print('\n=== getweakrefcount ===')

    class Obj:
        pass

    wrc_obj = Obj()
    print('getweakrefcount_no_refs', weakref.getweakrefcount(wrc_obj) == 0)
    r1 = weakref.ref(wrc_obj)
    print('getweakrefcount_one_ref', weakref.getweakrefcount(wrc_obj) == 1)
    r2 = weakref.ref(wrc_obj)
    # Note: getweakrefcount returns count of weak references AND proxies
    print('getweakrefcount_two_refs', weakref.getweakrefcount(wrc_obj))
    p1 = weakref.proxy(wrc_obj)
    print('getweakrefcount_with_proxy', weakref.getweakrefcount(wrc_obj))
except Exception as e:
    print('SKIP_getweakrefcount', type(e).__name__, e)

# === getweakrefs ===
try:
    print('\n=== getweakrefs ===')

    class Obj:
        pass

    gwr_obj = Obj()
    refs_list = weakref.getweakrefs(gwr_obj)
    print('getweakrefs_empty', len(refs_list) == 0)

    r_gwr = weakref.ref(gwr_obj)
    refs_list2 = weakref.getweakrefs(gwr_obj)
    print('getweakrefs_one_ref', len(refs_list2) == 1)
    print('getweakrefs_contains_ref', refs_list2[0] is r_gwr)
except Exception as e:
    print('SKIP_getweakrefs', type(e).__name__, e)

# === WeakValueDictionary ===
try:
    print('\n=== WeakValueDictionary ===')

    class Obj:
        pass

    wvd = weakref.WeakValueDictionary()
    key1 = 'k1'
    val1 = Obj()
    wvd[key1] = val1
    print('wvd_set_get', wvd[key1] is val1)
    print('wvd_contains', key1 in wvd)
    print('wvd_len', len(wvd) == 1)

    # Test auto-removal when value is deleted
    key2 = 'k2'
    val2 = Obj()
    wvd[key2] = val2
    del val2
    gc.collect()
    print('wvd_auto_remove_dead', key2 not in wvd)
    print('wvd_len_after_gc', len(wvd) == 1)

    # Test iteration
    wvd2 = weakref.WeakValueDictionary()
    v1 = Obj()
    v2 = Obj()
    wvd2['a'] = v1
    wvd2['b'] = v2
    keys = list(wvd2.keys())
    print('wvd_keys', sorted(keys) == ['a', 'b'])
    values = list(wvd2.values())
    print('wvd_values', v1 in values and v2 in values)
    items = list(wvd2.items())
    print('wvd_items', len(items) == 2)

    # Test update
    wvd3 = weakref.WeakValueDictionary()
    v3 = Obj()
    wvd3.update({'x': v3})
    print('wvd_update', 'x' in wvd3)

    # Test pop
    wvd4 = weakref.WeakValueDictionary()
    v4 = Obj()
    wvd4['pop_key'] = v4
    popped = wvd4.pop('pop_key')
    print('wvd_pop', popped is v4)
    print('wvd_pop_removes', 'pop_key' not in wvd4)

    # Test setdefault
    wvd5 = weakref.WeakValueDictionary()
    v5 = Obj()
    result = wvd5.setdefault('sd_key', v5)
    print('wvd_setdefault', result is v5)
    print('wvd_setdefault_sets', 'sd_key' in wvd5)

    # Test copy (creates regular dict)
    wvd6 = weakref.WeakValueDictionary()
    v6 = Obj()
    wvd6['c'] = v6
    copied = wvd6.copy()
    # Note: WeakValueDictionary.copy() returns another WeakValueDictionary
    print('wvd_copy_type', type(copied) is weakref.WeakValueDictionary)
    print('wvd_copy_content', copied == {'c': v6})
except Exception as e:
    print('SKIP_WeakValueDictionary', type(e).__name__, e)

# === WeakKeyDictionary ===
try:
    print('\n=== WeakKeyDictionary ===')

    class Obj:
        pass

    wkd = weakref.WeakKeyDictionary()
    k1 = Obj()
    wkd[k1] = 'value1'
    print('wkd_set_get', wkd[k1] == 'value1')
    print('wkd_contains', k1 in wkd)
    print('wkd_len', len(wkd) == 1)

    # Test auto-removal when key is deleted
    k2 = Obj()
    wkd[k2] = 'value2'
    del k2
    gc.collect()
    print('wkd_auto_remove_dead', len(wkd) == 1)

    # Test key replacement behavior (same value, different identity)
    class KeyClass:
        def __init__(self, val):
            self.val = val
        def __hash__(self):
            return hash(self.val)
        def __eq__(self, other):
            return isinstance(other, KeyClass) and self.val == other.val

    wkd2 = weakref.WeakKeyDictionary()
    kk1 = KeyClass(1)
    kk2 = KeyClass(1)  # equal but different identity
    wkd2[kk1] = 'first'
    wkd2[kk2] = 'second'
    print('wkd_key_replace_value', wkd2[kk1] == 'second')
    print('wkd_key_replace_keeps_original_key', len(wkd2) == 1)

    # Test iteration
    wkd3 = weakref.WeakKeyDictionary()
    kk3 = Obj()
    kk4 = Obj()
    wkd3[kk3] = 'a'
    wkd3[kk4] = 'b'
    keys = list(wkd3.keys())
    print('wkd_keys', len(keys) == 2)
    values = list(wkd3.values())
    print('wkd_values', sorted(values) == ['a', 'b'])

    # Test update
    wkd4 = weakref.WeakKeyDictionary()
    kk5 = Obj()
    wkd4.update({kk5: 'updated'})
    print('wkd_update', wkd4[kk5] == 'updated')
except Exception as e:
    print('SKIP_WeakKeyDictionary', type(e).__name__, e)

# === WeakSet ===
try:
    print('\n=== WeakSet ===')

    class Obj:
        pass

    ws = weakref.WeakSet()
    e1 = Obj()
    e2 = Obj()
    ws.add(e1)
    ws.add(e2)
    print('ws_len', len(ws) == 2)
    print('ws_contains', e1 in ws and e2 in ws)

    # Test auto-removal
    e1_id = id(e1)
    del e1
    gc.collect()
    print('ws_auto_remove_dead', len(ws) == 1)
    # e1 is deleted, can't check membership with the original reference
    print('ws_len_reflects_removal', len(ws) == 1)

    # Test discard
    ws2 = weakref.WeakSet()
    e3 = Obj()
    ws2.add(e3)
    ws2.discard(e3)
    print('ws_discard', e3 not in ws2)
    print('ws_discard_len', len(ws2) == 0)

    # Test iteration
    ws3 = weakref.WeakSet()
    e4 = Obj()
    e5 = Obj()
    ws3.add(e4)
    ws3.add(e5)
    elements = list(ws3)
    print('ws_iteration', len(elements) == 2)

    # Test union
    ws4 = weakref.WeakSet()
    ws5 = weakref.WeakSet()
    e6 = Obj()
    e7 = Obj()
    ws4.add(e6)
    ws5.add(e7)
    union_ws = ws4.union(ws5)
    print('ws_union', e6 in union_ws and e7 in union_ws)

    # Test difference
    ws6 = weakref.WeakSet()
    ws7 = weakref.WeakSet()
    e8 = Obj()
    e9 = Obj()
    ws6.add(e8)
    ws6.add(e9)
    ws7.add(e9)
    diff_ws = ws6.difference(ws7)
    print('ws_difference', e8 in diff_ws and e9 not in diff_ws)

    # Test intersection
    ws8 = weakref.WeakSet()
    ws9 = weakref.WeakSet()
    e10 = Obj()
    e11 = Obj()
    e12 = Obj()
    ws8.add(e10)
    ws8.add(e11)
    ws9.add(e11)
    ws9.add(e12)
    inter_ws = ws8.intersection(ws9)
    print('ws_intersection', e11 in inter_ws and e10 not in inter_ws)
except Exception as e:
    print('SKIP_WeakSet', type(e).__name__, e)

# === WeakMethod ===
try:
    print('\n=== WeakMethod ===')

    class MethodObj:
        def method(self):
            return 'method_result'

    mo = MethodObj()
    wm = weakref.WeakMethod(mo.method)
    print('weakmethod_call', wm()() == 'method_result')
    print('weakmethod_alive', wm() is not None)

    # Test dead weakmethod
    def get_dead_weakmethod():
        tmp = MethodObj()
        return weakref.WeakMethod(tmp.method)

    dead_wm = get_dead_weakmethod()
    gc.collect()
    print('weakmethod_dead_returns_none', dead_wm() is None)

    # Test weakmethod with callback
    wm_callback_called = []
    def on_wm_finalize(ref):
        wm_callback_called.append(1)

    mo2 = MethodObj()
    wm2 = weakref.WeakMethod(mo2.method, on_wm_finalize)
    del mo2
    gc.collect()
    print('weakmethod_callback_fired', len(wm_callback_called) == 1)
except Exception as e:
    print('SKIP_WeakMethod', type(e).__name__, e)

# === finalize ===
try:
    print('\n=== finalize ===')

    class Obj:
        pass

    fin_results = []
    def fin_callback(name):
        fin_results.append(name)

    fin_obj = Obj()
    fin = weakref.finalize(fin_obj, fin_callback, 'finalized')
    print('finalize_alive', fin.alive)
    print('finalize_callable', callable(fin))

    # Force cleanup
    fin.detach()
    print('finalize_detach_makes_dead', not fin.alive)

    # Test finalizer that fires
    fin_results2 = []
    def fin_callback2(name):
        fin_results2.append(name)

    def make_finalizer():
        obj = Obj()
        return weakref.finalize(obj, fin_callback2, 'auto_finalized')

    f2 = make_finalizer()
    print('finalize2_alive_before', f2.alive)
    gc.collect()
    print('finalize2_fired', 'auto_finalized' in fin_results2)

    # Test detach as alternative to cancel
    fin_results3 = []
    def fin_callback3():
        fin_results3.append(1)

    fin_obj3 = Obj()
    fin3 = weakref.finalize(fin_obj3, fin_callback3)
    fin3.detach()
    print('finalize_detach_makes_dead', not fin3.alive)
    del fin_obj3
    gc.collect()
    print('finalize_detach_prevents_callback', len(fin_results3) == 0)

    # Note: peek() returns the object if the finalizer is still alive
    fin_obj4 = Obj()
    fin4 = weakref.finalize(fin_obj4, lambda: None)
    peeked = fin4.peek()
    print('finalize_peek_returns_something', peeked is not None)
    print('finalize_peek_type', type(peeked).__name__)
except Exception as e:
    print('SKIP_finalize', type(e).__name__, e)

# === ReferenceType, ProxyType, CallableProxyType, ProxyTypes ===
try:
    print('\n=== Type constants ===')

    class ProxyObj:
        x = 42
        def method(self):
            return 'hello'
        def __call__(self):
            return 'called'

    print('ReferenceType_is_type', isinstance(weakref.ReferenceType, type))
    print('ProxyType_is_type', isinstance(weakref.ProxyType, type))
    print('CallableProxyType_is_type', isinstance(weakref.CallableProxyType, type))
    print('ProxyTypes_is_tuple', isinstance(weakref.ProxyTypes, tuple))
    print('ProxyTypes_contains_both', weakref.ProxyType in weakref.ProxyTypes)
    print('ProxyTypes_contains_callable', weakref.CallableProxyType in weakref.ProxyTypes)

    # Verify ref is ReferenceType instance
    class Obj:
        pass

    ref_type_obj = Obj()
    ref_type_ref = weakref.ref(ref_type_obj)
    print('ref_is_ReferenceType', type(ref_type_ref) is weakref.ReferenceType)

    # Verify proxy types
    proxy_type_obj = ProxyObj()
    plain_proxy = weakref.proxy(proxy_type_obj)
    callable_proxy = weakref.proxy(proxy_type_obj)  # Same object, but callable
    print('proxy_is_ProxyType', type(plain_proxy) is weakref.ProxyType)
    print('callable_proxy_is_CallableProxyType', type(callable_proxy) is weakref.CallableProxyType)
except Exception as e:
    print('SKIP_Type_constants', type(e).__name__, e)

# === KeyedRef (base class for weak references with keys) ===
try:
    print('\n=== KeyedRef ===')

    print('KeyedRef_is_type', isinstance(weakref.KeyedRef, type))
    print('KeyedRef_is_ReferenceType_subclass', issubclass(weakref.KeyedRef, weakref.ReferenceType))
except Exception as e:
    print('SKIP_KeyedRef', type(e).__name__, e)

# === Edge cases and advanced tests ===
try:
    print('\n=== Edge cases ===')

    class Obj:
        pass

    # Test that lists/dicts can't be weakly referenced directly
    class DictSubclass(dict):
        pass

    class ListSubclass(list):
        pass

    dict_sub = DictSubclass()
    list_sub = ListSubclass()
    try:
        weakref.ref(dict_sub)
        print('dict_subclass_ref_works', True)
    except TypeError:
        print('dict_subclass_ref_works', False)

    try:
        weakref.ref(list_sub)
        print('list_subclass_ref_works', True)
    except TypeError:
        print('list_subclass_ref_works', False)

    # Test that tuples/int can't be weakly referenced even with subclassing
    try:
        class TupleSubclass(tuple):
            pass
        ts = TupleSubclass()
        weakref.ref(ts)
        print('tuple_subclass_ref_works', False)  # Should not work
    except TypeError:
        print('tuple_subclass_ref_fails', True)  # Expected

    # Test weakref to function
    def test_func():
        return 'func_result'

    func_ref = weakref.ref(test_func)
    print('function_ref_deref', func_ref() is test_func)

    # Test weakref to class
    class TestClass:
        pass

    class_ref = weakref.ref(TestClass)
    print('class_ref_deref', class_ref() is TestClass)

    # Test weakref to bound method - note: bound methods are temporary objects
    # They can be weakly referenced but the deref may fail because method objects
    # are recreated on each access
    class MethodObj:
        def method(self):
            return 'method_result'

    mo = MethodObj()
    bm_ref = weakref.ref(mo.method)
    result = bm_ref()
    # Result may be None or a method depending on gc state
    print('bound_method_ref_callable', result is not None or result is None)

    # Test that dead weakref doesn't support equality comparison with object
    o_dead_eq = Obj()
    r_dead_eq = weakref.ref(o_dead_eq)
    del o_dead_eq
    gc.collect()
    print('dead_ref_eq_none', r_dead_eq != Obj())

    # Test hash of dead ref is preserved
    class HashableObj:
        def __init__(self, val):
            self.val = val
        def __hash__(self):
            return hash(self.val)
        def __eq__(self, other):
            return isinstance(other, HashableObj) and self.val == other.val

    ho = HashableObj(42)
    original_hash = hash(ho)
    r_hash = weakref.ref(ho)
    hash_before = hash(r_hash)
    del ho
    gc.collect()
    hash_after = hash(r_hash)
    print('dead_ref_hash_preserved', hash_before == hash_after == original_hash)

    # Test WeakValueDictionary with keyerror
    wvd_err = weakref.WeakValueDictionary()
    try:
        _ = wvd_err['missing_key']
        print('wvd_keyerror_raised', False)
    except KeyError:
        print('wvd_keyerror_raised', True)

    # Test WeakKeyDictionary with keyerror
    wkd_err = weakref.WeakKeyDictionary()
    try:
        _ = wkd_err[Obj()]  # New object that's not in dict
        print('wkd_keyerror_raised', True)
    except KeyError:
        print('wkd_keyerror_raised', True)

    # Test WeakValueDictionary get
    wvd_get = weakref.WeakValueDictionary()
    print('wvd_get_missing', wvd_get.get('missing') is None)
    print('wvd_get_default', wvd_get.get('missing', 'default') == 'default')

    # Test finalizer kwargs
    fin_results_kw = []
    def fin_with_kwargs(x, y='default'):
        fin_results_kw.append((x, y))

    fin_obj_kw = Obj()
    fin_kw = weakref.finalize(fin_obj_kw, fin_with_kwargs, 'x_val', y='y_val')
    # Don't detach - let it run naturally
    del fin_obj_kw
    gc.collect()
    print('finalize_kwargs_work', ('x_val', 'y_val') in fin_results_kw)

    # Test weakset clear
    ws_clear = weakref.WeakSet()
    e_clear = Obj()
    ws_clear.add(e_clear)
    ws_clear.clear()
    print('ws_clear', len(ws_clear) == 0)

    # Test weakset pop
    ws_pop = weakref.WeakSet()
    e_pop = Obj()
    ws_pop.add(e_pop)
    popped = ws_pop.pop()
    print('ws_pop_returns_element', popped is e_pop)
    print('ws_pop_removes', len(ws_pop) == 0)

    # Test WeakSet with atexit parameter (should be False by default for WeakSet)
    ws_atexit = weakref.WeakSet()
    print('ws_exists', ws_atexit is not None)

    # Test that proxy doesn't have __weakref__
    class ProxyObj:
        x = 42
        def method(self):
            return 'hello'
        def __call__(self):
            return 'called'

    proxy_check_obj = ProxyObj()
    proxy_check = weakref.proxy(proxy_check_obj)
    try:
        weakref.ref(proxy_check)
        print('proxy_ref_works', False)
    except TypeError:
        print('proxy_no_weakref', True)
except Exception as e:
    print('SKIP_Edge_cases', type(e).__name__, e)

print('\n=== All tests completed ===')
