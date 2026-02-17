"""Multi-session interpreter management for Ouros.

Provides :class:`SessionManager` and :class:`Session` as ergonomic Python
wrappers around the native Rust ``_ouros.SessionManager`` class.

:class:`Session` is a thin convenience handle that binds a ``session_id`` to
a manager, letting callers write ``session.execute(code)`` instead of
``manager.execute(code, session_id='...')``.

Example::

    from ouros.session import SessionManager

    mgr = SessionManager()
    s = mgr.create_session('sandbox')
    s.execute('x = 42')
    print(s.get_variable('x'))  # {'json_value': 42, 'repr': '42'}
"""

from __future__ import annotations

from typing import Any

from ._ouros import SessionManager as _NativeSessionManager


class Session:
    """A single interpreter session bound to a :class:`SessionManager`.

    All operations delegate to the underlying native manager, passing
    ``session_id`` automatically. Do not construct directly -- use
    :meth:`SessionManager.create_session` instead.
    """

    __slots__ = ('_manager', '_id')

    def __init__(self, manager: SessionManager, session_id: str) -> None:
        self._manager = manager
        self._id = session_id

    @property
    def id(self) -> str:
        """The session ID string."""
        return self._id

    # -- Execution -------------------------------------------------------------

    def execute(self, code: str) -> dict[str, Any]:
        """Execute Python code in this session."""
        return self._manager._native.execute(code, session_id=self._id)

    def resume(self, call_id: int, value: Any) -> dict[str, Any]:
        """Resume execution after an external function call."""
        return self._manager._native.resume(call_id, value, session_id=self._id)

    def resume_as_pending(self, call_id: int) -> dict[str, Any]:
        """Resume an external call as a pending async future."""
        return self._manager._native.resume_as_pending(call_id, session_id=self._id)

    def resume_futures(self, results: dict[int, Any]) -> dict[str, Any]:
        """Resume execution with results for pending futures."""
        return self._manager._native.resume_futures(results, session_id=self._id)

    # -- Variables -------------------------------------------------------------

    def list_variables(self) -> list[dict[str, Any]]:
        """List defined global variables and their types."""
        return self._manager._native.list_variables(session_id=self._id)

    def get_variable(self, name: str) -> dict[str, Any]:
        """Get one variable's value."""
        return self._manager._native.get_variable(name, session_id=self._id)

    def set_variable(self, name: str, value_expr: str) -> None:
        """Set or create a global variable via a Python expression string."""
        self._manager._native.set_variable(name, value_expr, session_id=self._id)

    def delete_variable(self, name: str) -> bool:
        """Delete a global variable. Returns True if it existed."""
        return self._manager._native.delete_variable(name, session_id=self._id)

    def eval_variable(self, expression: str) -> dict[str, Any]:
        """Evaluate a Python expression without modifying session state."""
        return self._manager._native.eval_variable(expression, session_id=self._id)

    # -- Fork ------------------------------------------------------------------

    def fork(self, new_id: str) -> Session:
        """Fork this session into a new independent copy."""
        self._manager._native.fork_session(self._id, new_id)
        return Session(self._manager, new_id)

    # -- History / rewind ------------------------------------------------------

    def rewind(self, *, steps: int = 1) -> dict[str, Any]:
        """Rewind this session by N steps."""
        return self._manager._native.rewind(steps=steps, session_id=self._id)

    def history(self) -> tuple[int, int]:
        """Returns (current_depth, max_depth) for session history."""
        return self._manager._native.history(session_id=self._id)

    def set_history_depth(self, max_depth: int) -> int:
        """Configure the maximum undo history depth. Returns entries trimmed."""
        return self._manager._native.set_history_depth(max_depth, session_id=self._id)

    # -- Heap ------------------------------------------------------------------

    def heap_stats(self) -> dict[str, Any]:
        """Return heap statistics for this session."""
        return self._manager._native.heap_stats(session_id=self._id)

    def snapshot_heap(self, name: str) -> None:
        """Save the current heap stats as a named snapshot."""
        self._manager._native.snapshot_heap(name, session_id=self._id)

    # -- Persistence -----------------------------------------------------------

    def save(self, *, name: str | None = None) -> dict[str, Any]:
        """Save this session to disk."""
        return self._manager._native.save_session(session_id=self._id, name=name)

    # -- Reset -----------------------------------------------------------------

    def reset(self, *, external_functions: list[str] | None = None) -> None:
        """Reset this session to a fresh state."""
        self._manager._native.reset(session_id=self._id, external_functions=external_functions)

    def __repr__(self) -> str:
        return f'Session(id={self._id!r})'


class SessionManager:
    """Multi-session manager for the Ouros interpreter.

    Wraps the native Rust ``SessionManager`` and provides a Pythonic API.
    A "default" session is always present and is used when no ``session_id``
    is specified.
    """

    __slots__ = ('_native',)

    def __init__(self, *, script_name: str = 'session.py') -> None:
        self._native = _NativeSessionManager(script_name=script_name)

    # -- Session lifecycle -----------------------------------------------------

    def create_session(
        self,
        session_id: str,
        *,
        external_functions: list[str] | None = None,
    ) -> Session:
        """Create a new named session and return a Session handle."""
        self._native.create_session(session_id, external_functions=external_functions)
        return Session(self, session_id)

    def destroy_session(self, session_id: str) -> None:
        """Destroy a named session."""
        self._native.destroy_session(session_id)

    def list_sessions(self) -> list[dict[str, Any]]:
        """List all active sessions with their variable counts."""
        return self._native.list_sessions()

    def fork_session(self, source: str, new_id: str) -> Session:
        """Fork an existing session into a new independent copy."""
        self._native.fork_session(source, new_id)
        return Session(self, new_id)

    # -- Execution (delegate to default session) -------------------------------

    def execute(self, code: str, *, session_id: str | None = None) -> dict[str, Any]:
        """Execute Python code in the default or specified session."""
        return self._native.execute(code, session_id=session_id)

    # -- Variables (delegate to default session) -------------------------------

    def list_variables(self, *, session_id: str | None = None) -> list[dict[str, Any]]:
        """List defined global variables."""
        return self._native.list_variables(session_id=session_id)

    def get_variable(self, name: str, *, session_id: str | None = None) -> dict[str, Any]:
        """Get one variable's value."""
        return self._native.get_variable(name, session_id=session_id)

    def set_variable(self, name: str, value_expr: str, *, session_id: str | None = None) -> None:
        """Set or create a global variable via a Python expression string."""
        self._native.set_variable(name, value_expr, session_id=session_id)

    def delete_variable(self, name: str, *, session_id: str | None = None) -> bool:
        """Delete a global variable. Returns True if it existed."""
        return self._native.delete_variable(name, session_id=session_id)

    def eval_variable(self, expression: str, *, session_id: str | None = None) -> dict[str, Any]:
        """Evaluate a Python expression without modifying session state."""
        return self._native.eval_variable(expression, session_id=session_id)

    def transfer_variable(
        self,
        source: str,
        target: str,
        name: str,
        *,
        target_name: str | None = None,
    ) -> None:
        """Transfer a variable from one session to another."""
        self._native.transfer_variable(source, target, name, target_name=target_name)

    # -- History / rewind (delegate to default session) ------------------------

    def rewind(self, *, steps: int = 1, session_id: str | None = None) -> dict[str, Any]:
        """Rewind a session by N steps."""
        return self._native.rewind(steps=steps, session_id=session_id)

    def history(self, *, session_id: str | None = None) -> tuple[int, int]:
        """Returns (current_depth, max_depth) for session history."""
        return self._native.history(session_id=session_id)

    def set_history_depth(self, max_depth: int, *, session_id: str | None = None) -> int:
        """Configure the maximum undo history depth. Returns entries trimmed."""
        return self._native.set_history_depth(max_depth, session_id=session_id)

    # -- Heap introspection (delegate to default session) ----------------------

    def heap_stats(self, *, session_id: str | None = None) -> dict[str, Any]:
        """Return heap statistics for a session."""
        return self._native.heap_stats(session_id=session_id)

    def snapshot_heap(self, name: str, *, session_id: str | None = None) -> None:
        """Save the current heap stats as a named snapshot."""
        self._native.snapshot_heap(name, session_id=session_id)

    def diff_heap(self, before: str, after: str) -> dict[str, Any]:
        """Compare two named heap snapshots and return the diff."""
        return self._native.diff_heap(before, after)

    # -- Persistence -----------------------------------------------------------

    def set_storage_dir(self, directory: str) -> None:
        """Configure the directory for session persistence."""
        self._native.set_storage_dir(directory)

    def save_session(
        self,
        *,
        session_id: str | None = None,
        name: str | None = None,
    ) -> dict[str, Any]:
        """Save a session to disk as a named snapshot."""
        return self._native.save_session(session_id=session_id, name=name)

    def load_session(self, name: str, *, session_id: str | None = None) -> str:
        """Load a previously saved session from disk. Returns the session ID."""
        return self._native.load_session(name, session_id=session_id)

    def list_saved_sessions(self) -> list[dict[str, Any]]:
        """List all saved session snapshots on disk."""
        return self._native.list_saved_sessions()

    # -- Reset -----------------------------------------------------------------

    def reset(self, *, session_id: str | None = None, external_functions: list[str] | None = None) -> None:
        """Reset a session to a fresh state."""
        self._native.reset(session_id=session_id, external_functions=external_functions)

    # -- Cross-session pipeline ------------------------------------------------

    def call_session(
        self,
        *,
        target: str,
        code: str,
        target_variable: str,
        source: str | None = None,
    ) -> dict[str, Any]:
        """Execute code in source session and store result in target session."""
        return self._native.call_session(
            target=target,
            code=code,
            target_variable=target_variable,
            source=source,
        )

    def __repr__(self) -> str:
        return 'SessionManager()'
