import atexit

# === register / unregister / run ===
events = []


def cb(name):
    events.append(name)


atexit._clear()
assert atexit._ncallbacks() == 0, 'ncallbacks_empty'
assert atexit.register(cb, 'first') is cb, 'register_returns_function'
assert atexit.register(cb, 'second') is cb, 'register_returns_function_second'
assert atexit._ncallbacks() == 2, 'ncallbacks_after_register'

while atexit._ncallbacks():
    atexit._run_exitfuncs()

assert events == ['second', 'first'], 'callbacks_run_lifo'
assert atexit._ncallbacks() == 0, 'ncallbacks_after_run'

# === unregister removes all matching callbacks ===
atexit.register(cb, 'x')
atexit.register(cb, 'y')
assert atexit._ncallbacks() == 2, 'ncallbacks_before_unregister'
assert atexit.unregister(cb) is None, 'unregister_returns_none'
assert atexit._ncallbacks() == 0, 'ncallbacks_after_unregister'

# === register argument validation ===
try:
    atexit.register()
    raise AssertionError('register_missing_expected_type_error')
except TypeError as exc:
    assert str(exc) == 'register() takes at least 1 argument (0 given)', 'register_missing_message'

try:
    atexit.register(123)
    raise AssertionError('register_non_callable_expected_type_error')
except TypeError as exc:
    assert str(exc) == 'the first argument must be callable', 'register_non_callable_message'

assert atexit._ncallbacks() == 0, 'ncallbacks_unchanged_after_register_errors'
