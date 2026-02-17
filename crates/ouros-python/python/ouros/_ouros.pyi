from types import EllipsisType
from typing import Any, Callable, Literal, final, overload

from typing_extensions import Self

from . import ExternalResult, ResourceLimits
from .os_access import OsFunction

__all__ = [
    '__version__',
    'Sandbox',
    'Complete',
    'Snapshot',
    'FutureSnapshot',
    'SandboxError',
    'SandboxSyntaxError',
    'SandboxRuntimeError',
    'SandboxTypingError',
    'Frame',
    'SessionManager',
]
__version__: str

@final
class Sandbox:
    """
    A sandboxed Python interpreter instance.

    Parses and compiles Python code on initialization, then can be run
    multiple times with different input values. This separates the parsing
    cost from execution, making repeated runs more efficient.
    """

    def __new__(
        cls,
        code: str,
        *,
        script_name: str = 'main.py',
        inputs: list[str] | None = None,
        external_functions: list[str] | None = None,
        type_check: bool = False,
        type_check_stubs: str | None = None,
        dataclass_registry: list[type] | None = None,
    ) -> Self:
        """
        Create a new Sandbox interpreter by parsing the given code.

        Arguments:
            code: Python code to execute
            script_name: Name used in tracebacks and error messages
            inputs: List of input variable names available in the code
            external_functions: List of external function names the code can call
            type_check: Whether to perform type checking on the code (default: True)
            type_check_stubs: Optional code to prepend before type checking,
                e.g. with input variable declarations or external function signatures
            dataclass_registry: Optional list of dataclass types to register for proper
                isinstance() support on output, see `register_dataclass()` above.

        Raises:
            SandboxSyntaxError: If the code cannot be parsed
            SandboxTypingError: If type_check is True and type errors are found
        """

    def type_check(self, prefix_code: str | None = None) -> None:
        """
        Perform static type checking on the code.

        Analyzes the code for type errors without executing it. This uses
        a subset of Python's type system supported by Ouro.

        Arguments:
            prefix_code: Optional code to prepend before type checking,
                e.g. with input variable declarations or external function signatures.

        Raises:
            SandboxTypingError: If type errors are found. Use `.display(format, color)`
                on the exception to render the diagnostics in different formats.
            RuntimeError: If the type checking infrastructure fails internally.
        """

    def run(
        self,
        *,
        inputs: dict[str, Any] | None = None,
        limits: ResourceLimits | None = None,
        external_functions: dict[str, Callable[..., Any]] | None = None,
        print_callback: Callable[[Literal['stdout'], str], None] | None = None,
        os: Callable[[OsFunction, tuple[Any, ...]], Any] | None = None,
    ) -> Any:
        """
        Execute the code and return the result.

        The GIL is released allowing parallel execution.

        Arguments:
            inputs: Dict of input variable values (must match names from __init__)
            limits: Optional resource limits configuration
            external_functions: Dict of external function callbacks (must match names from __init__)
            print_callback: Optional callback for print output
            os: Optional callback for OS calls.
                Called with (function_name, args) where function_name is like 'Path.exists'
                and args is a tuple of arguments. Must return the appropriate value for the
                OS function (e.g., bool for exists(), stat_result for stat()).

        Returns:
            The result of the last expression in the code

        Raises:
            SandboxRuntimeError: If the code raises an exception during execution
        """

    def start(
        self,
        *,
        inputs: dict[str, Any] | None = None,
        limits: ResourceLimits | None = None,
        print_callback: Callable[[Literal['stdout'], str], None] | None = None,
    ) -> Snapshot | FutureSnapshot | Complete:
        """
        Start the code execution and return a progress object, or completion.

        This allows you to iteratively run code and parse/resume whenever an external function is called.

        The GIL is released allowing parallel execution.

        Arguments:
            inputs: Dict of input variable values (must match names from __init__)
            limits: Optional resource limits configuration
            print_callback: Optional callback for print output

        Returns:
            Snapshot if an external function call is pending,
            FutureSnapshot if futures need to be resolved,
            Complete if execution finished without external calls.

        Raises:
            SandboxRuntimeError: If the code raises an exception during execution
        """

    def dump(self) -> bytes:
        """
        Serialize the Sandbox instance to a binary format.

        The serialized data can be stored and later restored with `Sandbox.load()`.
        This allows caching parsed code to avoid re-parsing on subsequent runs.

        Returns:
            Bytes containing the serialized Sandbox instance.

        Raises:
            ValueError: If serialization fails.
        """

    @staticmethod
    def load(
        data: bytes,
        *,
        dataclass_registry: list[type] | None = None,
    ) -> 'Sandbox':
        """
        Deserialize a Sandbox instance from binary format.

        Arguments:
            data: The serialized Sandbox data from `dump()`
            dataclass_registry: Optional list of dataclass types to register for proper
                isinstance() support on output, see `register_dataclass()` above.

        Returns:
            A new Sandbox instance.

        Raises:
            ValueError: If deserialization fails.
        """

    def register_dataclass(self, cls: type) -> None:
        """
        Register a dataclass type for proper isinstance() support on output.

        When a dataclass passes through the sandbox and is returned, it normally becomes
        an `UnknownDataclass`. By registering the original type, we can use it to
        instantiate a real instance of that dataclass.

        Arguments:
            cls: The dataclass type to register.

        Raises:
            TypeError: If the argument is not a dataclass type.
        """

    def __repr__(self) -> str: ...

@final
class Snapshot:
    """
    Represents a paused execution waiting for an external function call return value.

    Contains information about the pending external function call and allows
    resuming execution with the return value.
    """

    @property
    def script_name(self) -> str:
        """The name of the script being executed."""

    @property
    def is_os_function(self) -> bool:
        """Whether this snapshot is for an OS function call (e.g., Path.stat)."""

    @property
    def function_name(self) -> str | OsFunction:
        """The name of the function being called (external function or OS function like 'Path.stat').

        Will be a `OsFunction` if `is_os_function` is `True`.
        """

    @property
    def args(self) -> tuple[Any, ...]:
        """The positional arguments passed to the external function."""

    @property
    def kwargs(self) -> dict[str, Any]:
        """The keyword arguments passed to the external function."""

    @property
    def call_id(self) -> int:
        """The unique identifier for this external function call."""

    @overload
    def resume(self, *, return_value: Any) -> Snapshot | FutureSnapshot | Complete:
        """Resume execution with a return value from the external function.

        `resume` may only be called once on each Snapshot instance.

        The GIL is released allowing parallel execution.

        Arguments:
            return_value: The value to return from the external function call.
            exception: An exception to raise in the sandbox interpreter.
            future: A future to await in the sandbox interpreter.

        Returns:
            Snapshot if another external function call is pending,
            FutureSnapshot if futures need to be resolved,
            Complete if execution finished.

        Raises:
            TypeError: If both arguments are provided.
            RuntimeError: If execution has already completed.
            SandboxRuntimeError: If the code raises an exception during execution
        """

    @overload
    def resume(self, *, exception: BaseException) -> Snapshot | FutureSnapshot | Complete:
        """Resume execution by raising the exception in the sandbox interpreter.

        See docstring for the first overload for more information.
        """

    @overload
    def resume(self, *, future: EllipsisType) -> Snapshot | FutureSnapshot | Complete:
        """Resume execution by returning a pending future.

        No result is provided, we simply resume execution stating that a future is pending.

        See docstring for the first overload for more information.
        """

    def dump(self) -> bytes:
        """
        Serialize the Snapshot instance to a binary format.

        The serialized data can be stored and later restored with `Snapshot.load()`.
        This allows suspending execution and resuming later, potentially in a different process.

        Note: The `print_callback` is not serialized and must be re-provided via
        `set_print_callback()` after loading if print output is needed.

        Returns:
            Bytes containing the serialized Snapshot instance.

        Raises:
            ValueError: If serialization fails.
            RuntimeError: If the progress has already been resumed.
        """

    @staticmethod
    def load(
        data: bytes,
        *,
        print_callback: Callable[[Literal['stdout'], str], None] | None = None,
        dataclass_registry: list[type] | None = None,
    ) -> 'Snapshot':
        """
        Deserialize a Snapshot instance from binary format.

        Note: The `print_callback` is not preserved during serialization and must be
        re-provided as a keyword argument if print output is needed.

        Arguments:
            data: The serialized Snapshot data from `dump()`
            print_callback: Optional callback for print output
            dataclass_registry: Optional list of dataclass types to register for proper
                isinstance() support on output, see `register_dataclass()` above.

        Returns:
            A new Snapshot instance.

        Raises:
            ValueError: If deserialization fails.
        """

    def __repr__(self) -> str: ...

@final
class FutureSnapshot:
    """
    Represents a paused execution waiting for multiple futures to be resolved.

    Contains information about the pending futures and allows resuming execution
    with the results.
    """

    @property
    def script_name(self) -> str:
        """The name of the script being executed."""

    @property
    def pending_call_ids(self) -> list[int]:
        """The call IDs of the pending futures.

        Raises an error if the snapshot has already been resumed.
        """

    def resume(
        self,
        results: dict[int, ExternalResult],
    ) -> Snapshot | FutureSnapshot | Complete:
        """Resume execution with results for one or more futures.

        `resume` may only be called once on each FutureSnapshot instance.

        The GIL is released allowing parallel execution.

        Arguments:
            results: Dict mapping call_id to result dict. Each result dict must have
                either 'return_value' or 'exception' key (not both).

        Returns:
            Snapshot if an external function call is pending,
            FutureSnapshot if more futures need to be resolved,
            Complete if execution finished.

        Raises:
            TypeError: If result dict has invalid keys.
            RuntimeError: If execution has already completed.
            SandboxRuntimeError: If the code raises an exception during execution
        """

    def dump(self) -> bytes:
        """
        Serialize the FutureSnapshot instance to a binary format.

        The serialized data can be stored and later restored with `FutureSnapshot.load()`.
        This allows suspending execution and resuming later, potentially in a different process.

        Note: The `print_callback` is not serialized and must be re-provided via
        `set_print_callback()` after loading if print output is needed.

        Returns:
            Bytes containing the serialized FutureSnapshot instance.

        Raises:
            ValueError: If serialization fails.
            RuntimeError: If the progress has already been resumed.
        """

    @staticmethod
    def load(
        data: bytes,
        *,
        print_callback: Callable[[Literal['stdout'], str], None] | None = None,
        dataclass_registry: list[type] | None = None,
    ) -> 'FutureSnapshot':
        """
        Deserialize a FutureSnapshot instance from binary format.

        Note: The `print_callback` is not preserved during serialization and must be
        re-provided as a keyword argument if print output is needed.

        Arguments:
            data: The serialized FutureSnapshot data from `dump()`
            print_callback: Optional callback for print output
            dataclass_registry: Optional list of dataclass types to register for proper
                isinstance() support on output, see `register_dataclass()` above.

        Returns:
            A new FutureSnapshot instance.

        Raises:
            ValueError: If deserialization fails.
        """

    def __repr__(self) -> str: ...

@final
class Complete:
    """The result of a completed code execution."""

    @property
    def output(self) -> Any:
        """The final output value from the executed code."""

    def __repr__(self) -> str: ...

class SandboxError(Exception):
    """Base exception for all sandbox interpreter errors.

    Catching `SandboxError` will catch syntax, runtime, and typing errors from the sandbox.
    This exception is raised internally and cannot be constructed directly.
    """

    def exception(self) -> BaseException:
        """Returns the inner exception as a Python exception object."""

    def __str__(self) -> str:
        """Returns the exception message."""

@final
class SandboxSyntaxError(SandboxError):
    """Raised when Python code has syntax errors or cannot be parsed.

    Inherits exception(), __str__() from SandboxError.
    """

    def display(self, format: Literal['type-msg', 'msg'] = 'msg') -> str:
        """Returns formatted exception string.

        Args:
            format: 'type-msg' - 'ExceptionType: message' format
                  'msg' - just the message
        """

@final
class SandboxTypingError(SandboxError):
    """Raised when type checking finds errors in the code.

    This exception is raised when static type analysis detects type errors
    before execution. Use `.display(format, color)` to render the diagnostics
    in different formats.

    Inherits exception(), __str__() from SandboxError.
    Cannot be constructed directly from Python.
    """

    def display(
        self,
        format: Literal[
            'full', 'concise', 'azure', 'json', 'jsonlines', 'rdjson', 'pylint', 'gitlab', 'github'
        ] = 'full',
        color: bool = False,
    ) -> str:
        """Renders the type error diagnostics with the specified format and color.

        Args:
            format: Output format for the diagnostics. Defaults to 'full'.
            color: Whether to include ANSI color codes. Defaults to False.
        """

@final
class SandboxRuntimeError(SandboxError):
    """Raised when sandbox code fails during execution.

    Inherits exception(), __str__() from SandboxError.
    Additionally provides traceback() and display() methods.
    """

    def traceback(self) -> list[Frame]:
        """Returns the traceback as a list of Frame objects."""

    def display(self, format: Literal['traceback', 'type-msg', 'msg'] = 'traceback') -> str:
        """Returns formatted exception string.

        Args:
            format: 'traceback' - full traceback with exception
                  'type-msg' - 'ExceptionType: message' format
                  'msg' - just the message
        """

@final
class Frame:
    """A single frame in a traceback."""

    @property
    def filename(self) -> str:
        """The filename where the code is located."""

    @property
    def line(self) -> int:
        """Line number (1-based)."""

    @property
    def column(self) -> int:
        """Column number (1-based)."""

    @property
    def end_line(self) -> int:
        """End line number (1-based)."""

    @property
    def end_column(self) -> int:
        """End column number (1-based)."""

    @property
    def function_name(self) -> str | None:
        """The name of the function, or None for module-level code."""

    @property
    def source_line(self) -> str | None:
        """The source code line for preview in the traceback."""

    def dict(self) -> dict[str, int | str | None]:
        """dict of attributes."""

@final
class SessionManager:
    """Multi-session manager for the Ouro interpreter.

    Wraps the Rust SessionManager and delegates all operations, converting
    Rust result types to Python dicts.
    """

    def __new__(cls, *, script_name: str = 'session.py') -> Self: ...
    def execute(self, code: str, *, session_id: str | None = None) -> dict[str, Any]: ...
    def resume(self, call_id: int, value: Any, *, session_id: str | None = None) -> dict[str, Any]: ...
    def resume_as_pending(self, call_id: int, *, session_id: str | None = None) -> dict[str, Any]: ...
    def resume_futures(self, results: dict[int, Any], *, session_id: str | None = None) -> dict[str, Any]: ...
    def list_variables(self, *, session_id: str | None = None) -> list[dict[str, Any]]: ...
    def get_variable(self, name: str, *, session_id: str | None = None) -> dict[str, Any]: ...
    def set_variable(self, name: str, value_expr: str, *, session_id: str | None = None) -> None: ...
    def delete_variable(self, name: str, *, session_id: str | None = None) -> bool: ...
    def transfer_variable(self, source: str, target: str, name: str, *, target_name: str | None = None) -> None: ...
    def eval_variable(self, expression: str, *, session_id: str | None = None) -> dict[str, Any]: ...
    def create_session(self, id: str, *, external_functions: list[str] | None = None) -> str: ...
    def destroy_session(self, id: str) -> None: ...
    def list_sessions(self) -> list[dict[str, Any]]: ...
    def fork_session(self, source: str, new_id: str) -> None: ...
    def reset(self, *, session_id: str | None = None, external_functions: list[str] | None = None) -> None: ...
    def set_storage_dir(self, dir: str) -> None: ...
    def save_session(self, *, session_id: str | None = None, name: str | None = None) -> dict[str, Any]: ...
    def load_session(self, name: str, *, session_id: str | None = None) -> str: ...
    def list_saved_sessions(self) -> list[dict[str, Any]]: ...
    def rewind(self, *, steps: int = 1, session_id: str | None = None) -> dict[str, Any]: ...
    def history(self, *, session_id: str | None = None) -> tuple[int, int]: ...
    def set_history_depth(self, max_depth: int, *, session_id: str | None = None) -> int: ...
    def heap_stats(self, *, session_id: str | None = None) -> dict[str, Any]: ...
    def snapshot_heap(self, name: str, *, session_id: str | None = None) -> None: ...
    def diff_heap(self, before: str, after: str) -> dict[str, Any]: ...
    def call_session(
        self, *, target: str, code: str, target_variable: str, source: str | None = None
    ) -> dict[str, Any]: ...
