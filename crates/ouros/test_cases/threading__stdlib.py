import threading


events = []


def worker(x):
    events.append(x)


t = threading.Thread(target=worker, args=(5,), daemon=False)
ret = t.start()
assert ret is None, 'thread_start_return_none'
t.join()
assert events == [5], 'thread_target_called'
assert t.is_alive() is False, 'thread_not_alive_after_start'
assert hasattr(t, 'name'), 'thread_has_name'
assert t.daemon is False, 'thread_daemon_false'
assert t.ident is not None, 'thread_ident_set'

main_t = threading.current_thread()
assert hasattr(main_t, 'name'), 'current_thread_has_name'
assert threading.main_thread().name == 'MainThread', 'main_thread_name'
assert threading.active_count() >= 1, 'active_count_at_least_one'
all_threads = threading.enumerate()
assert len(all_threads) >= 1, 'enumerate_nonempty'
assert any(th.name == 'MainThread' for th in all_threads), 'enumerate_contains_main_thread'

lock = threading.Lock()
assert lock.locked() is False, 'lock_initial_state'
assert lock.acquire() is True, 'lock_acquire_true'
assert lock.locked() is True, 'lock_locked_after_acquire'
lock.release()
assert lock.locked() is False, 'lock_unlocked_after_release'
with lock:
    assert lock.locked() is True, 'lock_context_enter'
assert lock.locked() is False, 'lock_context_exit'

rlock = threading.RLock()
assert rlock.locked() is False, 'rlock_initial_state'
rlock.acquire()
rlock.acquire()
assert rlock.locked() is True, 'rlock_locked'
rlock.release()
assert rlock.locked() is True, 'rlock_still_locked'
rlock.release()
assert rlock.locked() is False, 'rlock_unlocked'

flag = threading.Event()
assert flag.is_set() is False, 'event_initial_false'
assert flag.wait(timeout=0) is False, 'event_wait_false'
flag.set()
assert flag.is_set() is True, 'event_set_true'
assert flag.wait() is True, 'event_wait_true'
flag.clear()
assert flag.is_set() is False, 'event_clear_false'

local_data = threading.local()
local_data.value = 42
assert local_data.value == 42, 'thread_local_storage'
