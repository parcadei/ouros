# === Basic generator function ===
try:
    def gen_basic():
        yield 1
        yield 2
        yield 3

    print('gen_basic', list(gen_basic()))
except Exception as e:
    print('SKIP_Basic_generator_function', type(e).__name__, e)

# === Generator with multiple yields ===
try:
    def gen_multi():
        yield 'a'
        yield 'b'
        yield 'c'
        yield 'd'

    print('gen_multi', list(gen_multi()))
except Exception as e:
    print('SKIP_Generator_with_multiple_yields', type(e).__name__, e)

# === Generator expression ===
try:
    print('gen_expr', list(x * 2 for x in range(5)))
except Exception as e:
    print('SKIP_Generator_expression', type(e).__name__, e)

# === Generator expression with condition ===
try:
    print('gen_expr_filter', list(x for x in range(10) if x % 2 == 0))
except Exception as e:
    print('SKIP_Generator_expression_with_condition', type(e).__name__, e)

# === Generator expression nested ===
try:
    print('gen_expr_nested', list((x, y) for x in range(2) for y in range(2)))
except Exception as e:
    print('SKIP_Generator_expression_nested', type(e).__name__, e)

# === Generator with send ===
try:
    def gen_send():
        received = yield 'first'
        yield f'got: {received}'

    g = gen_send()
    print('gen_send_start', next(g))
    print('gen_send_send', g.send('hello'))
except Exception as e:
    print('SKIP_Generator_with_send', type(e).__name__, e)

# === Generator send starting with None ===
try:
    def gen_send_none():
        received = yield 'ready'
        yield f'received: {received}'

    g = gen_send_none()
    print('gen_send_none_first', g.send(None))
    print('gen_send_none_second', g.send('value'))
except Exception as e:
    print('SKIP_Generator_send_starting_with_None', type(e).__name__, e)

# === Generator throw ===
try:
    def gen_throw():
        try:
            yield 1
            yield 2
        except ValueError:
            yield 'caught'

    g = gen_throw()
    print('gen_throw_first', next(g))
    print('gen_throw_caught', g.throw(ValueError('test error')))
except Exception as e:
    print('SKIP_Generator_throw', type(e).__name__, e)

# === Generator throw after exhausted ===
try:
    def gen_throw_exhausted():
        yield 1

    g = gen_throw_exhausted()
    print('gen_throw_exhausted', list(g))
    try:
        g.throw(ValueError('after close'))
    except ValueError as e:
        print('gen_throw_exhausted_error', type(e).__name__, str(e))
except Exception as e:
    print('SKIP_Generator_throw_after_exhausted', type(e).__name__, e)

# === Generator close ===
try:
    def gen_close():
        try:
            yield 1
            yield 2
        finally:
            print('gen_close_cleanup', 'running cleanup')

    g = gen_close()
    print('gen_close_first', next(g))
    g.close()
    print('gen_closed', 'closed')
except Exception as e:
    print('SKIP_Generator_close', type(e).__name__, e)

# === Generator close with cleanup ===
try:
    def gen_close_cleanup():
        try:
            yield 1
        except GeneratorExit:
            print('gen_close_cleanup_exit', 'GeneratorExit received')
            raise

    g = gen_close_cleanup()
    print('gen_close_cleanup_first', next(g))
    try:
        g.close()
        print('gen_close_cleanup_done', 'done')
    except RuntimeError as e:
        print('gen_close_cleanup_runtime', type(e).__name__, str(e))
except Exception as e:
    print('SKIP_Generator_close_with_cleanup', type(e).__name__, e)

# === yield from simple ===
try:
    def gen_inner():
        yield 1
        yield 2

    def gen_outer():
        yield 'start'
        yield from gen_inner()
        yield 'end'

    print('gen_yield_from', list(gen_outer()))
except Exception as e:
    print('SKIP_yield_from_simple', type(e).__name__, e)

# === yield from with send ===
try:
    def gen_inner_send():
        received = yield 'inner1'
        yield f'inner got: {received}'

    def gen_outer_send():
        yield 'outer1'
        result = yield from gen_inner_send()
        yield f'outer got: {result}'

    g = gen_outer_send()
    print('gen_yield_from_send_1', next(g))
    print('gen_yield_from_send_2', next(g))
    print('gen_yield_from_send_3', g.send('hello'))
except Exception as e:
    print('SKIP_yield_from_with_send', type(e).__name__, e)

# === yield from with return value ===
try:
    def gen_inner_return():
        yield 1
        return 'returned'

    def gen_outer_return():
        result = yield from gen_inner_return()
        yield f'result: {result}'

    print('gen_yield_from_return', list(gen_outer_return()))
except Exception as e:
    print('SKIP_yield_from_with_return_value', type(e).__name__, e)

# === yield from with throw ===
try:
    def gen_inner_throw():
        try:
            yield 1
            yield 2
        except ValueError:
            yield 'inner caught'

    def gen_outer_throw():
        yield 'outer1'
        yield from gen_inner_throw()
        yield 'outer2'

    g = gen_outer_throw()
    print('gen_yield_from_throw_1', next(g))
    print('gen_yield_from_throw_2', next(g))
    print('gen_yield_from_throw_caught', g.throw(ValueError('test')))
except Exception as e:
    print('SKIP_yield_from_with_throw', type(e).__name__, e)

# === gi_frame attribute ===
try:
    def gen_frame():
        yield 1
        yield 2

    g = gen_frame()
    print('gen_gi_frame_exists', g.gi_frame is not None)
    print('gen_gi_frame_code_name', g.gi_frame.f_code.co_name)
    print('gen_gi_frame_lineno_before', g.gi_frame.f_lineno)
    next(g)
    print('gen_gi_frame_lineno_after', g.gi_frame.f_lineno)
except Exception as e:
    print('SKIP_gi_frame_attribute', type(e).__name__, e)

# === gi_running attribute ===
try:
    def gen_running_inner():
        yield 1

    running_check = None

    def gen_running():
        global running_check
        running_check = g.gi_running
        yield 1

    g = gen_running()
    print('gen_gi_running_before', g.gi_running)
    next(g)
    print('gen_gi_running_during', running_check)
    print('gen_gi_running_after', g.gi_running)
except Exception as e:
    print('SKIP_gi_running_attribute', type(e).__name__, e)

# === gi_code attribute ===
try:
    def gen_code():
        yield 1

    g = gen_code()
    print('gen_gi_code_exists', g.gi_code is not None)
    print('gen_gi_code_name', g.gi_code.co_name)
except Exception as e:
    print('SKIP_gi_code_attribute', type(e).__name__, e)

# === gi_suspended attribute (Python 3.11+) ===
try:
    def gen_suspended():
        yield 1
        yield 2

    g = gen_suspended()
    print('gen_gi_suspended_before', getattr(g, 'gi_suspended', 'N/A'))
    next(g)
    print('gen_gi_suspended_during', getattr(g, 'gi_suspended', 'N/A'))
except Exception as e:
    print('SKIP_gi_suspended_attribute_(Python_3.11+)', type(e).__name__, e)

# === Generator return value (StopIteration) ===
try:
    def gen_return_value():
        yield 1
        return 'done'

    g = gen_return_value()
    print('gen_return_val_1', next(g))
    try:
        next(g)
    except StopIteration as e:
        print('gen_return_val_stopiter', type(e).__name__, getattr(e, 'value', 'N/A'))
except Exception as e:
    print('SKIP_Generator_return_value_(StopIteration)', type(e).__name__, e)

# === Generator with multiple returns ===
try:
    def gen_multi_return(x):
        if x > 0:
            yield x
            return 'positive'
        else:
            return 'non-positive'

    for val in [5, -3]:
        g = gen_multi_return(val)
        results = []
        try:
            while True:
                results.append(next(g))
        except StopIteration as e:
            print(f'gen_multi_return_{val}', results, getattr(e, 'value', 'N/A'))
except Exception as e:
    print('SKIP_Generator_with_multiple_returns', type(e).__name__, e)

# === Generator with try-finally ===
try:
    def gen_finally():
        try:
            yield 1
            yield 2
        finally:
            print('gen_finally_cleanup', 'running finally')

    print('gen_finally', list(gen_finally()))
except Exception as e:
    print('SKIP_Generator_with_try-finally', type(e).__name__, e)

# === Generator with try-except-finally ===
try:
    def gen_except_finally():
        try:
            yield 1
            raise ValueError('test')
        except ValueError:
            yield 'caught'
        finally:
            print('gen_except_finally_cleanup', 'finally')

    print('gen_except_finally', list(gen_except_finally()))
except Exception as e:
    print('SKIP_Generator_with_try-except-finally', type(e).__name__, e)

# === Nested generator functions ===
try:
    def outer():
        def inner():
            yield 'inner'
        return inner()

    print('gen_nested_func', list(outer()))
except Exception as e:
    print('SKIP_Nested_generator_functions', type(e).__name__, e)

# === Generator with closure ===
try:
    def make_gen(multiplier):
        def gen():
            for i in range(3):
                yield i * multiplier
        return gen()

    print('gen_closure', list(make_gen(10)))
except Exception as e:
    print('SKIP_Generator_with_closure', type(e).__name__, e)

# === Generator with default arguments ===
try:
    def gen_defaults(a, b=2, c=3):
        yield a
        yield b
        yield c

    print('gen_defaults', list(gen_defaults(1)))
    print('gen_defaults_override', list(gen_defaults(1, 20, 30)))
except Exception as e:
    print('SKIP_Generator_with_default_arguments', type(e).__name__, e)

# === Generator with *args ===
try:
    def gen_args(*args):
        for arg in args:
            yield arg

    print('gen_args', list(gen_args(1, 2, 3)))
except Exception as e:
    print('SKIP_Generator_with_*args', type(e).__name__, e)

# === Generator with **kwargs ===
try:
    def gen_kwargs(**kwargs):
        for key in sorted(kwargs):
            yield (key, kwargs[key])

    print('gen_kwargs', list(gen_kwargs(a=1, b=2)))
except Exception as e:
    print('SKIP_Generator_with_**kwargs', type(e).__name__, e)

# === Generator with *args and **kwargs ===
try:
    def gen_args_kwargs(*args, **kwargs):
        for arg in args:
            yield ('arg', arg)
        for key in sorted(kwargs):
            yield ('kwarg', key, kwargs[key])

    print('gen_args_kwargs', list(gen_args_kwargs(1, 2, x=10, y=20)))
except Exception as e:
    print('SKIP_Generator_with_*args_and_**kwargs', type(e).__name__, e)

# === Infinite generator with islice pattern ===
try:
    def gen_infinite():
        n = 0
        while True:
            yield n
            n += 1

    g = gen_infinite()
    result = []
    for _ in range(5):
        result.append(next(g))
    print('gen_infinite_first5', result)
except Exception as e:
    print('SKIP_Infinite_generator_with_islice_pattern', type(e).__name__, e)

# === Generator that yields generators ===
try:
    def gen_of_gens():
        yield (x for x in range(2))
        yield (x for x in range(3, 5))

    g = gen_of_gens()
    print('gen_of_gens_1', list(next(g)))
    print('gen_of_gens_2', list(next(g)))
except Exception as e:
    print('SKIP_Generator_that_yields_generators', type(e).__name__, e)

# === Generator with recursive yield from ===
try:
    def gen_recursive(n):
        if n > 0:
            yield n
            yield from gen_recursive(n - 1)

    print('gen_recursive', list(gen_recursive(5)))
except Exception as e:
    print('SKIP_Generator_with_recursive_yield_from', type(e).__name__, e)

# === Generator used in for loop directly ===
try:
    def gen_for_loop():
        for i in range(3):
            yield i * 10

    result = []
    for item in gen_for_loop():
        result.append(item)
    print('gen_for_loop', result)
except Exception as e:
    print('SKIP_Generator_used_in_for_loop_directly', type(e).__name__, e)

# === Generator with next and default ===
try:
    def gen_with_default():
        yield 1

    g = gen_with_default()
    print('gen_next_default', next(g, 'default'))
    print('gen_next_default_exhausted', next(g, 'default'))
except Exception as e:
    print('SKIP_Generator_with_next_and_default', type(e).__name__, e)

# === Generator state after StopIteration ===
try:
    def gen_exhausted():
        yield 1

    g = gen_exhausted()
    print('gen_exhausted_1', next(g))
    try:
        next(g)
    except StopIteration:
        print('gen_exhausted_stop', 'stopped')
except Exception as e:
    print('SKIP_Generator_state_after_StopIteration', type(e).__name__, e)

# === Generator expression with tuple unpacking ===
try:
    data = [(1, 'a'), (2, 'b'), (3, 'c')]
    print('gen_expr_unpack', list(x for x, y in data))
except Exception as e:
    print('SKIP_Generator_expression_with_tuple_unpacking', type(e).__name__, e)

# === Generator expression as function argument ===
try:
    print('gen_expr_func_arg', sum(x for x in range(5)))
    print('gen_expr_func_arg_max', max(x * 2 for x in range(5)))
except Exception as e:
    print('SKIP_Generator_expression_as_function_argument', type(e).__name__, e)

# === Generator with del and reassignment ===
try:
    def gen_del():
        x = 1
        yield x
        del x
        x = 2
        yield x

    print('gen_del', list(gen_del()))
except Exception as e:
    print('SKIP_Generator_with_del_and_reassignment', type(e).__name__, e)

# === Generator with global ===
try:
    counter = 0

    def gen_global():
        global counter
        counter += 1
        yield counter
        counter += 1
        yield counter

    print('gen_global', list(gen_global()))
    print('gen_global_counter', counter)
except Exception as e:
    print('SKIP_Generator_with_global', type(e).__name__, e)

# === Generator with nonlocal ===
try:
    def make_gen_nonlocal():
        count = 0
        def gen():
            nonlocal count
            count += 1
            yield count
            count += 1
            yield count
        return gen()

    print('gen_nonlocal', list(make_gen_nonlocal()))
except Exception as e:
    print('SKIP_Generator_with_nonlocal', type(e).__name__, e)

# === Generator with yield in conditional ===
try:
    def gen_conditional(x):
        if x > 0:
            yield 'positive'
        elif x < 0:
            yield 'negative'
        else:
            yield 'zero'

    print('gen_conditional_pos', list(gen_conditional(5)))
    print('gen_conditional_neg', list(gen_conditional(-3)))
    print('gen_conditional_zero', list(gen_conditional(0)))
except Exception as e:
    print('SKIP_Generator_with_yield_in_conditional', type(e).__name__, e)

# === Generator with yield in loop ===
try:
    def gen_loop():
        for i in range(3):
            if i == 1:
                yield f'special {i}'
            else:
                yield i

    print('gen_loop_conditional', list(gen_loop()))
except Exception as e:
    print('SKIP_Generator_with_yield_in_loop', type(e).__name__, e)

# === Generator with while loop ===
try:
    def gen_while(n):
        while n > 0:
            yield n
            n -= 1

    print('gen_while', list(gen_while(5)))
except Exception as e:
    print('SKIP_Generator_with_while_loop', type(e).__name__, e)

# === Generator with break ===
try:
    def gen_break():
        for i in range(10):
            if i == 3:
                break
            yield i

    print('gen_break', list(gen_break()))
except Exception as e:
    print('SKIP_Generator_with_break', type(e).__name__, e)

# === Generator with continue ===
try:
    def gen_continue():
        for i in range(5):
            if i == 2:
                continue
            yield i

    print('gen_continue', list(gen_continue()))
except Exception as e:
    print('SKIP_Generator_with_continue', type(e).__name__, e)

# === Generator with nested loops ===
try:
    def gen_nested_loops():
        for i in range(2):
            for j in range(2):
                yield (i, j)

    print('gen_nested_loops', list(gen_nested_loops()))
except Exception as e:
    print('SKIP_Generator_with_nested_loops', type(e).__name__, e)

# === Generator with enumerate ===
try:
    def gen_enumerate():
        for i, val in enumerate(['a', 'b', 'c']):
            yield (i, val)

    print('gen_enumerate', list(gen_enumerate()))
except Exception as e:
    print('SKIP_Generator_with_enumerate', type(e).__name__, e)

# === Generator with zip ===
try:
    def gen_zip():
        for a, b in zip([1, 2, 3], ['a', 'b', 'c']):
            yield (a, b)

    print('gen_zip', list(gen_zip()))
except Exception as e:
    print('SKIP_Generator_with_zip', type(e).__name__, e)

# === Generator raising exception ===
try:
    def gen_raises():
        yield 1
        raise ValueError('error in gen')
        yield 2  # noqa

    g = gen_raises()
    print('gen_raises_1', next(g))
    try:
        next(g)
    except ValueError as e:
        print('gen_raises_error', type(e).__name__, str(e))
except Exception as e:
    print('SKIP_Generator_raising_exception', type(e).__name__, e)

# === Generator catching exception from send ===
try:
    def gen_catch_send_error():
        try:
            yield 'ready'
        except ValueError:
            yield 'caught in gen'

    g = gen_catch_send_error()
    print('gen_catch_send_1', next(g))
    print('gen_catch_send_2', g.throw(ValueError('thrown')))
except Exception as e:
    print('SKIP_Generator_catching_exception_from_send', type(e).__name__, e)

# === Generator with complex send sequence ===
try:
    def gen_complex_send():
        x = yield 'start'
        y = yield f'got x: {x}'
        z = yield f'got y: {y}'
        return f'done with {z}'

    g = gen_complex_send()
    print('gen_complex_1', g.send(None))
    print('gen_complex_2', g.send('a'))
    print('gen_complex_3', g.send('b'))
    try:
        g.send('c')
    except StopIteration as e:
        print('gen_complex_return', getattr(e, 'value', 'N/A'))
except Exception as e:
    print('SKIP_Generator_with_complex_send_sequence', type(e).__name__, e)

# === Generator with yield from and send to inner ===
try:
    def gen_inner_bidi():
        a = yield 'inner1'
        b = yield f'inner2: {a}'
        return f'inner done: {b}'

    def gen_outer_bidi():
        result = yield from gen_inner_bidi()
        yield f'outer: {result}'

    g = gen_outer_bidi()
    print('gen_bidi_1', next(g))
    print('gen_bidi_2', g.send('x'))
    print('gen_bidi_3', g.send('y'))
except Exception as e:
    print('SKIP_Generator_with_yield_from_and_send_to_inner', type(e).__name__, e)

# === Empty generator ===
try:
    def gen_empty():
        return
        yield  # noqa

    print('gen_empty', list(gen_empty()))
except Exception as e:
    print('SKIP_Empty_generator', type(e).__name__, e)

# === Generator with only return ===
try:
    def gen_return_only():
        return 'value'
        yield  # noqa

    g = gen_return_only()
    try:
        next(g)
    except StopIteration as e:
        print('gen_return_only', getattr(e, 'value', 'N/A'))
except Exception as e:
    print('SKIP_Generator_with_only_return', type(e).__name__, e)

# === Generator yielding None explicitly ===
try:
    def gen_yield_none():
        yield None
        yield 1
        yield None

    print('gen_yield_none', list(gen_yield_none()))
except Exception as e:
    print('SKIP_Generator_yielding_None_explicitly', type(e).__name__, e)

# === Generator yielding various types ===
try:
    def gen_mixed_types():
        yield 1
        yield 'string'
        yield [1, 2, 3]
        yield {'key': 'value'}
        yield (1, 2)
        yield {1, 2, 3}

    print('gen_mixed_types', list(gen_mixed_types()))
except Exception as e:
    print('SKIP_Generator_yielding_various_types', type(e).__name__, e)

# === Generator identity ===
try:
    def gen_identity():
        yield 1

    g1 = gen_identity()
    g2 = gen_identity()
    print('gen_identity_diff', g1 is not g2)
except Exception as e:
    print('SKIP_Generator_identity', type(e).__name__, e)

# === Generator iter() ===
try:
    def gen_iter():
        yield 1
        yield 2

    g = gen_iter()
    print('gen_iter_same', iter(g) is g)
except Exception as e:
    print('SKIP_Generator_iter()', type(e).__name__, e)

# === Generator __iter__ and __next__ ===
try:
    def gen_dunder():
        yield 1
        yield 2

    g = gen_dunder()
    print('gen_dunder_next', g.__next__())
    print('gen_dunder_iter', g.__iter__() is g)
except Exception as e:
    print('SKIP_Generator___iter___and___next__', type(e).__name__, e)

# === Generator with class method ===
try:
    class GenClass:
        def __init__(self, n):
            self.n = n
        
        def gen_method(self):
            for i in range(self.n):
                yield i

    obj = GenClass(3)
    print('gen_class_method', list(obj.gen_method()))
except Exception as e:
    print('SKIP_Generator_with_class_method', type(e).__name__, e)

# === Generator with static method ===
try:
    class GenStatic:
        @staticmethod
        def gen_static():
            yield 1
            yield 2

    print('gen_static_method', list(GenStatic.gen_static()))
except Exception as e:
    print('SKIP_Generator_with_static_method', type(e).__name__, e)

# === Generator with class method decorator ===
try:
    class GenClassMethod:
        _count = 0
        
        @classmethod
        def gen_class(cls):
            cls._count += 1
            yield cls._count

    print('gen_class_method', list(GenClassMethod.gen_class()))
except Exception as e:
    print('SKIP_Generator_with_class_method_decorator', type(e).__name__, e)

# === Generator slicing equivalent ===
try:
    def gen_slice():
        for i in range(10):
            yield i

    g = gen_slice()
    result = []
    for _ in range(3):
        result.append(next(g))
    print('gen_slice_first3', result)
except Exception as e:
    print('SKIP_Generator_slicing_equivalent', type(e).__name__, e)

# === Generator with list comprehension inside ===
try:
    def gen_list_inside():
        data = [x for x in range(3)]
        for item in data:
            yield item * 2

    print('gen_list_inside', list(gen_list_inside()))
except Exception as e:
    print('SKIP_Generator_with_list_comprehension_inside', type(e).__name__, e)

# === Generator with generator inside ===
try:
    def gen_gen_inside():
        inner = (x * 2 for x in range(3))
        for item in inner:
            yield item

    print('gen_gen_inside', list(gen_gen_inside()))
except Exception as e:
    print('SKIP_Generator_with_generator_inside', type(e).__name__, e)

# === Generator with multiple yield from ===
try:
    def gen_multi_yield_from():
        yield from [1, 2]
        yield from (x for x in range(3, 5))
        yield from (5, 6)

    print('gen_multi_yield_from', list(gen_multi_yield_from()))
except Exception as e:
    print('SKIP_Generator_with_multiple_yield_from', type(e).__name__, e)

# === yield from string ===
try:
    def gen_yield_from_str():
        yield from 'ab'

    print('gen_yield_from_str', list(gen_yield_from_str()))
except Exception as e:
    print('SKIP_yield_from_string', type(e).__name__, e)

# === yield from dict keys ===
try:
    def gen_yield_from_dict():
        yield from {'a': 1, 'b': 2}

    print('gen_yield_from_dict', list(gen_yield_from_dict()))
except Exception as e:
    print('SKIP_yield_from_dict_keys', type(e).__name__, e)

# === Generator with send and complex expression ===
try:
    def gen_send_expr():
        x = yield 'start'
        y = (yield 'middle') or 'default'
        yield f'x={x}, y={y}'

    g = gen_send_expr()
    print('gen_send_expr_1', next(g))
    print('gen_send_expr_2', g.send('a'))
    print('gen_send_expr_3', next(g))  # Send None
except Exception as e:
    print('SKIP_Generator_with_send_and_complex_expression', type(e).__name__, e)

# === Generator with yield in try-else ===
try:
    def gen_try_else():
        try:
            yield 'try'
        except ValueError:
            yield 'except'
        else:
            yield 'else'

    print('gen_try_else', list(gen_try_else()))
except Exception as e:
    print('SKIP_Generator_with_yield_in_try-else', type(e).__name__, e)

# === Generator with yield in try-except that doesn't catch ===
try:
    def gen_try_no_catch():
        try:
            yield 'try'
            raise TypeError('test')
        except ValueError:
            yield 'except'

    g = gen_try_no_catch()
    print('gen_try_no_catch_1', next(g))
    try:
        next(g)
    except TypeError as e:
        print('gen_try_no_catch_error', type(e).__name__, str(e))
except Exception as e:
    print('SKIP_Generator_with_yield_in_try-except_that_doesnt_catch', type(e).__name__, e)

# === Generator state: gi_frame after exhaustion ===
try:
    def gen_frame_exhausted():
        yield 1

    g = gen_frame_exhausted()
    print('gen_frame_before', g.gi_frame is not None)
    list(g)
    print('gen_frame_after', g.gi_frame is None)
except Exception as e:
    print('SKIP_Generator_state:_gi_frame_after_exhaustion', type(e).__name__, e)

# === Generator expression scope isolation ===
try:
    x = 100
    print('gen_expr_scope', list(x for x in range(3)), x)
except Exception as e:
    print('SKIP_Generator_expression_scope_isolation', type(e).__name__, e)

# === Generator with walrus operator ===
try:
    def gen_walrus():
        n = yield 'ready'
        while n is not None:
            n = yield f'got: {n}'

    g = gen_walrus()
    print('gen_walrus_1', next(g))
    print('gen_walrus_2', g.send(5))
    print('gen_walrus_3', g.send(10))
except Exception as e:
    print('SKIP_Generator_with_walrus_operator', type(e).__name__, e)

# === Generator with f-string yield ===
try:
    def gen_fstring():
        for i in range(3):
            yield f'value: {i}'

    print('gen_fstring', list(gen_fstring()))
except Exception as e:
    print('SKIP_Generator_with_f-string_yield', type(e).__name__, e)

# === Generator with formatted yield ===
try:
    def gen_format():
        for i in range(3):
            yield 'value: {}'.format(i)

    print('gen_format', list(gen_format()))
except Exception as e:
    print('SKIP_Generator_with_formatted_yield', type(e).__name__, e)

# === Generator with docstring ===
try:
    def gen_docstring():
        '''This is a generator function.'''
        yield 1

    print('gen_docstring', gen_docstring.__doc__)
    print('gen_docstring_list', list(gen_docstring()))
except Exception as e:
    print('SKIP_Generator_with_docstring', type(e).__name__, e)
