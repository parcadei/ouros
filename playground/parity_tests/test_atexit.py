import atexit

# === register/unregister/run ===
events = []


def cb(name):
    events.append(name)


try:
    atexit._clear()
    print('ncallbacks_empty', atexit._ncallbacks())
    print('register_returns_fn', atexit.register(cb, 'a') is cb)
    print('register_returns_fn2', atexit.register(cb, 'b') is cb)
    print('ncallbacks_after_register', atexit._ncallbacks())
    while atexit._ncallbacks():
        atexit._run_exitfuncs()
    print('events_after_run', events)
    atexit.register(cb, 'x')
    atexit.register(cb, 'y')
    print('ncallbacks_before_unregister', atexit._ncallbacks())
    print('unregister_returns_none', atexit.unregister(cb))
    print('ncallbacks_after_unregister', atexit._ncallbacks())
except Exception as e:
    print('SKIP_atexit', type(e).__name__, e)
