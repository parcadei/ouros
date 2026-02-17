"""Tests for the SessionManager Python bindings.

Tests cover session lifecycle, code execution, variable operations,
fork independence, rewind/undo, save/load persistence, heap introspection,
cross-session transfer, and error handling.
"""

from __future__ import annotations

import tempfile

import pytest

from ouros.session import Session, SessionManager

# ---------------------------------------------------------------------------
# Session lifecycle
# ---------------------------------------------------------------------------


def test_create_session_returns_session():
    mgr = SessionManager()
    s = mgr.create_session('test-1')
    assert isinstance(s, Session)
    assert s.id == 'test-1'


def test_default_session_in_list():
    mgr = SessionManager()
    sessions = mgr.list_sessions()
    ids = [s['id'] for s in sessions]
    assert 'default' in ids


def test_create_and_list_sessions():
    mgr = SessionManager()
    mgr.create_session('alpha')
    mgr.create_session('beta')
    sessions = mgr.list_sessions()
    ids = sorted(s['id'] for s in sessions)
    # default + alpha + beta
    assert ids == ['alpha', 'beta', 'default']


def test_destroy_session():
    mgr = SessionManager()
    mgr.create_session('temp')
    mgr.destroy_session('temp')
    sessions = mgr.list_sessions()
    ids = [s['id'] for s in sessions]
    assert 'temp' not in ids


def test_destroy_default_session_raises():
    mgr = SessionManager()
    with pytest.raises(RuntimeError, match='cannot destroy the default session'):
        mgr.destroy_session('default')


def test_create_duplicate_session_raises():
    mgr = SessionManager()
    mgr.create_session('dup')
    with pytest.raises(RuntimeError, match='already exists'):
        mgr.create_session('dup')


def test_destroy_nonexistent_session_raises():
    mgr = SessionManager()
    with pytest.raises(RuntimeError, match='not found'):
        mgr.destroy_session('ghost')


# ---------------------------------------------------------------------------
# Execution
# ---------------------------------------------------------------------------


def test_execute_simple():
    mgr = SessionManager()
    result = mgr.execute('x = 42')
    assert result['stdout'] == ''


def test_execute_with_session_id():
    mgr = SessionManager()
    s = mgr.create_session('s1')
    result = s.execute('y = 10')
    assert result['stdout'] == ''


def test_execute_with_print():
    mgr = SessionManager()
    result = mgr.execute('print("hello")')
    assert result['stdout'] == 'hello\n'


def test_execute_nonexistent_session_raises():
    mgr = SessionManager()
    with pytest.raises(RuntimeError, match='not found'):
        mgr.execute('1+1', session_id='nope')


# ---------------------------------------------------------------------------
# Variable operations
# ---------------------------------------------------------------------------


def test_get_set_variable():
    mgr = SessionManager()
    mgr.execute('x = 42')
    var = mgr.get_variable('x')
    assert var['json_value'] == 42


def test_list_variables():
    mgr = SessionManager()
    mgr.execute('a = 1')
    mgr.execute('b = "hi"')
    variables = mgr.list_variables()
    names = sorted(v['name'] for v in variables)
    assert 'a' in names
    assert 'b' in names


def test_set_variable_expression():
    mgr = SessionManager()
    mgr.set_variable('x', '[1, 2, 3]')
    var = mgr.get_variable('x')
    assert var['json_value'] == [1, 2, 3]


def test_delete_variable():
    mgr = SessionManager()
    mgr.execute('z = 99')
    result = mgr.delete_variable('z')
    assert result is True


def test_delete_nonexistent_variable():
    mgr = SessionManager()
    result = mgr.delete_variable('nope')
    assert result is False


def test_eval_variable():
    mgr = SessionManager()
    mgr.execute('x = 10')
    result = mgr.eval_variable('x + 5')
    assert result['value']['json_value'] == 15


def test_session_object_get_variable():
    mgr = SessionManager()
    s = mgr.create_session('vars')
    s.execute('name = "ouros"')
    var = s.get_variable('name')
    assert var['json_value'] == 'ouros'


def test_session_object_set_variable():
    mgr = SessionManager()
    s = mgr.create_session('sv')
    s.set_variable('x', '42')
    var = s.get_variable('x')
    assert var['json_value'] == 42


# ---------------------------------------------------------------------------
# Fork independence
# ---------------------------------------------------------------------------


def test_fork_creates_independent_session():
    mgr = SessionManager()
    mgr.execute('x = 1')
    mgr.fork_session('default', 'fork1')
    # Modify forked session
    mgr.execute('x = 999', session_id='fork1')
    # Original should be unchanged
    original_var = mgr.get_variable('x')
    forked_var = mgr.get_variable('x', session_id='fork1')
    assert original_var['json_value'] == 1
    assert forked_var['json_value'] == 999


def test_session_fork_method():
    mgr = SessionManager()
    s = mgr.create_session('src')
    s.execute('v = 42')
    forked = s.fork('src-fork')
    assert isinstance(forked, Session)
    assert forked.id == 'src-fork'
    forked_var = forked.get_variable('v')
    assert forked_var['json_value'] == 42


# ---------------------------------------------------------------------------
# Rewind / undo
# ---------------------------------------------------------------------------


def test_rewind_restores_state():
    mgr = SessionManager()
    mgr.execute('x = 1')
    mgr.execute('x = 2')
    mgr.execute('x = 3')
    result = mgr.rewind(steps=2)
    assert result['steps_rewound'] == 2
    var = mgr.get_variable('x')
    assert var['json_value'] == 1


def test_history_returns_depth():
    mgr = SessionManager()
    mgr.execute('a = 1')
    mgr.execute('b = 2')
    current, max_depth = mgr.history()
    assert current == 2
    assert max_depth == 20  # default


def test_set_history_depth():
    mgr = SessionManager()
    for i in range(5):
        mgr.execute(f'x = {i}')
    trimmed = mgr.set_history_depth(2)
    # 5 entries trimmed to 2 means 3 dropped
    assert trimmed == 3
    current, max_depth = mgr.history()
    assert current == 2
    assert max_depth == 2


def test_session_rewind():
    mgr = SessionManager()
    s = mgr.create_session('rw')
    s.execute('x = 10')
    s.execute('x = 20')
    result = s.rewind(steps=1)
    assert result['steps_rewound'] == 1
    var = s.get_variable('x')
    assert var['json_value'] == 10


# ---------------------------------------------------------------------------
# Transfer variable
# ---------------------------------------------------------------------------


def test_transfer_variable():
    mgr = SessionManager()
    mgr.execute('data = [1, 2, 3]')
    mgr.create_session('target')
    mgr.transfer_variable('default', 'target', 'data')
    var = mgr.get_variable('data', session_id='target')
    assert var['json_value'] == [1, 2, 3]


def test_transfer_variable_with_rename():
    mgr = SessionManager()
    mgr.execute('src_val = 42')
    mgr.create_session('dest')
    mgr.transfer_variable('default', 'dest', 'src_val', target_name='dst_val')
    var = mgr.get_variable('dst_val', session_id='dest')
    assert var['json_value'] == 42


# ---------------------------------------------------------------------------
# Heap introspection
# ---------------------------------------------------------------------------


def test_heap_stats():
    mgr = SessionManager()
    mgr.execute('x = [1, 2, 3]')
    stats = mgr.heap_stats()
    assert 'live_objects' in stats
    assert isinstance(stats['live_objects'], int)
    assert stats['live_objects'] > 0


def test_snapshot_and_diff_heap():
    mgr = SessionManager()
    mgr.execute('a = 1')
    mgr.snapshot_heap('before')
    mgr.execute('b = [1, 2, 3, 4, 5]')
    mgr.snapshot_heap('after')
    diff = mgr.diff_heap('before', 'after')
    assert 'heap_diff' in diff
    assert 'variable_diff' in diff


# ---------------------------------------------------------------------------
# Save / load persistence
# ---------------------------------------------------------------------------


def test_save_and_load_session():
    with tempfile.TemporaryDirectory() as tmpdir:
        mgr = SessionManager()
        mgr.set_storage_dir(tmpdir)
        mgr.execute('x = 42')
        save_result = mgr.save_session(name='snap1')
        assert save_result['name'] == 'snap1'
        assert save_result['size_bytes'] > 0

        # Load into a new session
        loaded_id = mgr.load_session('snap1', session_id='loaded')
        assert loaded_id == 'loaded'
        var = mgr.get_variable('x', session_id='loaded')
        assert var['json_value'] == 42


def test_list_saved_sessions():
    with tempfile.TemporaryDirectory() as tmpdir:
        mgr = SessionManager()
        mgr.set_storage_dir(tmpdir)
        mgr.execute('a = 1')
        mgr.save_session(name='snap_a')
        mgr.execute('b = 2')
        mgr.save_session(name='snap_b')
        saved = mgr.list_saved_sessions()
        names = sorted(s['name'] for s in saved)
        assert names == ['snap_a', 'snap_b']


# ---------------------------------------------------------------------------
# Reset
# ---------------------------------------------------------------------------


def test_reset_clears_session():
    mgr = SessionManager()
    mgr.execute('x = 42')
    mgr.reset()
    variables = mgr.list_variables()
    names = [v['name'] for v in variables]
    assert 'x' not in names


# ---------------------------------------------------------------------------
# Cross-session pipeline
# ---------------------------------------------------------------------------


def test_call_session():
    mgr = SessionManager()
    mgr.execute('x = 10')
    mgr.create_session('receiver')
    result = mgr.call_session(
        target='receiver',
        code='x * 2',
        target_variable='result',
    )
    assert result['stdout'] == ''
    var = mgr.get_variable('result', session_id='receiver')
    assert var['json_value'] == 20


# ---------------------------------------------------------------------------
# Error handling
# ---------------------------------------------------------------------------


def test_get_variable_not_found():
    mgr = SessionManager()
    with pytest.raises(RuntimeError, match='not found'):
        mgr.get_variable('nonexistent')


def test_rewind_zero_steps_raises():
    mgr = SessionManager()
    with pytest.raises(RuntimeError, match='at least 1'):
        mgr.rewind(steps=0)


def test_rewind_too_many_steps_raises():
    mgr = SessionManager()
    mgr.execute('x = 1')
    with pytest.raises(RuntimeError, match='cannot rewind'):
        mgr.rewind(steps=100)


def test_save_without_storage_dir_raises():
    mgr = SessionManager()
    with pytest.raises(RuntimeError, match='storage not configured'):
        mgr.save_session(name='test')
