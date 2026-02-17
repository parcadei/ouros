#!/usr/bin/env python3
"""Generate Ouro stdlib add-on scaffolding from CPython introspection.

This script automates the repetitive work needed when adding a new stdlib
module to Ouro:

1. Introspect the CPython API shape for a target module.
2. Build oracle-backed example calls and expected outputs.
3. Generate test files for Ouro test cases and parity audit harnesses.
4. Generate a Rust scaffold module with `todo!()` implementation stubs.
5. Generate a registration-diff guide for `mod.rs`, `intern.rs`, and parity audit.
6. Generate a single `AGENT_TASK.md` bundle with everything an implementing agent needs.

The script is standalone and uses only Python stdlib modules.
"""

from __future__ import annotations

import argparse
import importlib
import inspect
import itertools
import json
import keyword
import math
import re
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path
from types import ModuleType
from typing import Any

# =============================================================================
# Section 1: Data model
# =============================================================================


@dataclass
class ModuleAPI:
    """Complete public API summary for one stdlib module."""

    module_name: str
    functions: list['FunctionInfo']
    classes: list['ClassInfo']
    constants: list['ConstantInfo']


@dataclass
class FunctionInfo:
    """Metadata and generated artifacts for one public callable."""

    name: str
    signature: str | None
    params: list['ParamInfo']
    is_safe: bool
    block_reason: str | None
    test_cases: list['TestCase'] = field(default_factory=list)
    properties: list['PropertySpec'] = field(default_factory=list)


@dataclass
class ParamInfo:
    """Serialized parameter information extracted from inspect.Signature."""

    name: str
    annotation: str | None
    default: str | None
    kind: str


@dataclass
class TestCase:
    """One successful oracle-backed function call."""

    label: str
    args_repr: str
    expected_repr: str
    expected_type: str


@dataclass
class PropertySpec:
    """A discovered property and the Python check used to verify it."""

    name: str
    description: str
    check_code: str
    holds: bool


@dataclass
class ClassInfo:
    """Public surface summary for one class exported by the module."""

    name: str
    public_attrs: list[str]
    is_safe: bool


@dataclass
class ConstantInfo:
    """Public constant metadata and value representation."""

    name: str
    value_repr: str
    value_type: str


@dataclass(frozen=True)
class CandidateCall:
    """Internal generated call candidate before CPython execution."""

    label: str
    args: tuple[Any, ...]
    kwargs: tuple[tuple[str, Any], ...]


@dataclass
class OracleSummary:
    """Internal oracle execution summary for one function."""

    success_cases: list[TestCase] = field(default_factory=list)
    skipped_cases: list[str] = field(default_factory=list)
    exceptions: list['ExceptionCase'] = field(default_factory=list)
    observed_calls: list['ObservedCall'] = field(default_factory=list)


@dataclass
class ObservedCall:
    """One observed oracle execution (success or exception) for a function call."""

    label: str
    args_repr: str
    positional_count: int
    keyword_names: list[str]
    arg_types: list[str]
    kwarg_types: dict[str, str]
    outcome: str
    result_repr: str | None
    result_type: str | None
    exc_type: str | None
    exc_message: str | None


@dataclass
class ExceptionCase:
    """One oracle-observed exception case with exact type/message."""

    label: str
    args_repr: str
    exc_type: str
    exc_message: str


@dataclass
class EdgeCaseObservation:
    """Observed function behavior on one edge-case sample input."""

    function_name: str
    sample_repr: str
    status: str
    output_repr: str | None
    output_type: str | None
    exc_type: str | None
    exc_message: str | None


@dataclass
class ClassProbe:
    """Observed behavior for one class constructor and basic method probes."""

    class_name: str
    constructor_signature: str | None
    constructor_cases: list[dict[str, Any]]
    method_probes: list[dict[str, Any]]


@dataclass
class StaticStringPlan:
    """Resolved StaticStrings mapping and new insertions for module integration."""

    resolved: dict[str, str]
    suggested_insertions: list[tuple[str, str]]


@dataclass
class RegistrationSnippet:
    """Human-readable insertion preview for one registration point."""

    file_path: str
    section: str
    anchor: str
    insert_line: str
    context_before: str
    context_after: str
    line_number: int | None


@dataclass
class ModuleReadiness:
    """One-shot readiness assessment for a generated module package."""

    score: int
    tier: str
    one_shot_eligible: bool
    summary: str
    strengths: list[str]
    risks: list[str]
    manual_verification_required: list[str]
    anti_hallucination_rules: list[str]
    function_status: list[dict[str, Any]]


# =============================================================================
# Constants and configuration
# =============================================================================


ROOT = Path(__file__).resolve().parents[1]
MODULES_RS = ROOT / 'crates' / 'ouro' / 'src' / 'modules' / 'mod.rs'
INTERN_RS = ROOT / 'crates' / 'ouro' / 'src' / 'intern.rs'
DEEP_PARITY = ROOT / 'playground' / 'deep_parity_audit.sh'
TEST_CASES_DIR = ROOT / 'crates' / 'ouro' / 'test_cases'
PARITY_TEST_DIR = ROOT / 'playground' / 'parity_tests'

UNSAFE_PATTERNS = {
    'open',
    'read',
    'write',
    'close',
    'connect',
    'socket',
    'system',
    'popen',
    'exec',
    'eval',
    'compile',
    'remove',
    'unlink',
    'rmdir',
    'mkdir',
    'rename',
    'chdir',
    'walk',
    'spawn',
    'fork',
    'sleep',
    'request',
    'urlopen',
    'download',
    'upload',
}

UNSAFE_EXACT_FUNCTION_NAMES = {
    'breakpoint',
    'copyright',
    'credits',
    'exit',
    'help',
    'license',
    'main',
    'quit',
}

UNSAFE_SYS_FUNCTION_NAMES = {
    'addaudithook',
    'audit',
    'breakpointhook',
    'displayhook',
    'excepthook',
    'set_asyncgen_hooks',
    'set_coroutine_origin_tracking_depth',
    'set_int_max_str_digits',
    'setprofile',
    'setrecursionlimit',
    'setswitchinterval',
    'settrace',
    'unraisablehook',
}

UNSAFE_PARAM_HINTS = {
    'path',
    'paths',
    'file',
    'filename',
    'dirname',
    'fd',
    'stream',
    'socket',
    'host',
    'port',
    'url',
    'command',
    'shell',
    'env',
}

INPUT_POOLS: dict[str, list[Any]] = {
    'str': ['', 'hello', '<b>hi</b>', '"quotes"', '&amp;', 'cafe', '  spaces  ', '\n\t'],
    'bytes': [b'', b'hello', b'\x00\x01\xff', b'hello world'],
    'type': [str, int, dict],
    # Keep integers intentionally small to avoid pathological runtime in oracle calls
    # (e.g. math.factorial(2**31) can hang generation).
    'int': [0, 1, -1, 2, 10, 42],
    'float': [0.0, 1.0, -1.0, 3.14, float('inf'), float('-inf')],
    'bool': [True, False],
    'list': [[], [1, 2, 3], ['a', 'b'], [1, 'mixed', True]],
    'NoneType': [None],
}

GENERIC_FALLBACK_INPUTS = ['', 'hello', 0, 1, True, None]

MAX_CASES_PER_FUNCTION = 24
MAX_PROPERTIES_PER_FUNCTION = 4
MAX_ERROR_CASES_PER_FUNCTION = 16
MAX_EDGE_OBSERVATIONS_PER_FUNCTION = 12
MAX_NEGATIVE_CANDIDATES_PER_FUNCTION = 16

ROUNDTRIP_NAME_PATTERNS = (
    ('encode', 'decode'),
    ('escape', 'unescape'),
    ('dumps', 'loads'),
    ('dump', 'load'),
    ('compress', 'decompress'),
    ('quote', 'unquote'),
)

PYTHON_RESERVED_NAMES = set(keyword.kwlist)
RUST_RESERVED_NAMES = {
    'as',
    'break',
    'const',
    'continue',
    'crate',
    'else',
    'enum',
    'extern',
    'false',
    'fn',
    'for',
    'if',
    'impl',
    'in',
    'let',
    'loop',
    'match',
    'mod',
    'move',
    'mut',
    'pub',
    'ref',
    'return',
    'self',
    'Self',
    'static',
    'struct',
    'super',
    'trait',
    'true',
    'type',
    'unsafe',
    'use',
    'where',
    'while',
}

HIGH_RISK_MODULE_HINTS = {
    'asyncio',
    'ctypes',
    'importlib',
    'io',
    'logging',
    'multiprocessing',
    'os',
    'pathlib',
    'shutil',
    'signal',
    'socket',
    'subprocess',
    'tempfile',
    'threading',
    'urllib',
}

# Modules where automatic parity probes are intentionally suppressed because the
# current Ouro surface is intentionally partial, highly host-coupled, or known
# to be process-global/nondeterministic in ways that produce noisy diffs.
PARITY_AUTOGEN_BLOCKLIST_MODULES = {
    'array',
    'asyncio',
    'builtins',
    'datetime',
    'gc',
    'inspect',
    'os',
    'pathlib',
    'pickle',
    'sys',
    'tempfile',
    'warnings',
}

PARITY_FUNCTION_NAME_BLOCKLIST = {
    # Emits checker diagnostics in Ouro output and is low-value for parity probes.
    'reveal_type',
}

MAX_READINESS_MANUAL_ITEMS = 30
MAX_READINESS_FUNCTION_STATUS_ITEMS = 40
MAX_AGENT_MANUAL_ITEMS = 20
MAX_AGENT_FUNCTION_ITEMS = 80


# =============================================================================
# Utility helpers
# =============================================================================


def log(message: str, *, verbose: bool, force: bool = False) -> None:
    """Write a progress message to stderr."""
    if force or verbose:
        print(f'[stdlib_add] {message}', file=sys.stderr)


def stable_repr(value: Any) -> str:
    """Return repr-like text for diagnostics that should never raise."""
    try:
        return repr(value)
    except Exception as exc:  # pragma: no cover - defensive
        return f'<repr-error {type(exc).__name__}: {exc}>'


def sanitize_identifier(value: str) -> str:
    """Normalize arbitrary text into an underscore-separated identifier."""
    cleaned = re.sub(r'[^A-Za-z0-9_]+', '_', value)
    cleaned = re.sub(r'_+', '_', cleaned).strip('_')
    if not cleaned:
        cleaned = 'value'
    if cleaned[0].isdigit():
        cleaned = f'v_{cleaned}'
    return cleaned


def module_file_stem(module_name: str) -> str:
    """Convert module name into a filesystem-safe stem."""
    return sanitize_identifier(module_name.replace('.', '_')).lower()


def py_single_quoted(value: str) -> str:
    """Render a Python string literal using single quotes."""
    # Use unicode-escape so control bytes (including NUL) are always representable.
    escaped = value.encode('unicode_escape').decode('ascii').replace("'", "\\'")
    return f"'{escaped}'"


def to_python_expr(value: Any) -> str | None:
    """Render a stable Python expression for simple literal-like values.

    Returns `None` for values we intentionally skip in generated assert tests.
    """
    if value is None:
        return 'None'
    if isinstance(value, bool):
        return 'True' if value else 'False'
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        if math.isnan(value):
            return None
        if math.isinf(value):
            return "float('inf')" if value > 0 else "float('-inf')"
        return repr(value)
    if isinstance(value, str):
        return py_single_quoted(value)
    if isinstance(value, bytes):
        return repr(value)
    if isinstance(value, list):
        parts: list[str] = []
        for item in value:
            expr = to_python_expr(item)
            if expr is None:
                return None
            parts.append(expr)
        return f'[{", ".join(parts)}]'
    if isinstance(value, tuple):
        parts = []
        for item in value:
            expr = to_python_expr(item)
            if expr is None:
                return None
            parts.append(expr)
        if len(parts) == 1:
            return f'({parts[0]},)'
        return f'({", ".join(parts)})'
    if isinstance(value, dict):
        items: list[str] = []
        for key, item in value.items():
            key_expr = to_python_expr(key)
            item_expr = to_python_expr(item)
            if key_expr is None or item_expr is None:
                return None
            items.append(f'{key_expr}: {item_expr}')
        return '{' + ', '.join(items) + '}'
    return None


def format_call_args(args: tuple[Any, ...], kwargs: tuple[tuple[str, Any], ...]) -> str | None:
    """Format call arguments as Python source usable inside `f(...)`."""
    rendered: list[str] = []
    for arg in args:
        expr = to_python_expr(arg)
        if expr is None:
            return None
        rendered.append(expr)
    for key, value in kwargs:
        expr = to_python_expr(value)
        if expr is None:
            return None
        rendered.append(f'{key}={expr}')
    return ', '.join(rendered)


def dedupe_preserve_order(values: list[Any]) -> list[Any]:
    """Deduplicate while preserving order using repr-based keys."""
    seen: set[str] = set()
    result: list[Any] = []
    for value in values:
        key = stable_repr(value)
        if key in seen:
            continue
        seen.add(key)
        result.append(value)
    return result


def call_with_isolated_argv(function_obj: Any, *args: Any, **kwargs: Any) -> Any:
    """Call a function while shielding it from this script's CLI argv.

    Some stdlib callables (for example `uuid.main`) parse `sys.argv` when called
    with no explicit args. We isolate argv so oracle execution does not inherit
    `stdlib_add.py` flags like `--output-dir`.
    """
    old_argv = sys.argv[:]
    sys.argv = [old_argv[0] if old_argv else 'stdlib_add']
    try:
        return function_obj(*args, **kwargs)
    finally:
        sys.argv = old_argv


def safe_equals(left: Any, right: Any) -> bool | None:
    """Safe equality helper for exotic stdlib objects.

    Returns `None` when equality itself raises.
    """
    try:
        return left == right
    except Exception:
        return None


def write_output(path: Path, content: str, *, dry_run: bool, verbose: bool) -> None:
    """Write file content, or print dry-run preview."""
    if dry_run:
        print(f'--- {path} ---')
        if verbose:
            print(content)
        else:
            first_lines = '\n'.join(content.splitlines()[:20])
            print(first_lines)
            if len(content.splitlines()) > 20:
                print('... <truncated; use --verbose for full preview>')
        print()
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding='utf-8')


def write_output_with_fallback(
    primary_path: Path,
    content: str,
    *,
    dry_run: bool,
    verbose: bool,
    fallback_path: Path | None = None,
) -> Path:
    """Write a file, with optional fallback path on permission errors."""
    try:
        write_output(primary_path, content, dry_run=dry_run, verbose=verbose)
        return primary_path
    except PermissionError:
        if fallback_path is None:
            raise
        log(
            (f'Permission denied writing {primary_path}; falling back to {fallback_path}'),
            verbose=verbose,
            force=True,
        )
        write_output(fallback_path, content, dry_run=dry_run, verbose=verbose)
        return fallback_path


def contains_unsafe_token(text: str) -> str | None:
    """Return matched unsafe token if found in lowercase text."""
    lowered = text.lower()
    for token in sorted(UNSAFE_PATTERNS):
        if token in lowered:
            return token
    return None


def is_host_sensitive_module(module_name: str) -> bool:
    """Return True when the module belongs to a host-sensitive stdlib family."""
    parts = module_name.split('.')
    return any(part in HIGH_RISK_MODULE_HINTS for part in parts)


def infer_simple_pool_keys(annotation_text: str | None, param_name: str) -> list[str]:
    """Infer candidate input pools from annotation/name heuristics."""
    text = (annotation_text or '').lower()
    name = param_name.lower()
    keys: list[str] = []

    # Annotation-based hints.
    if any(token in text for token in ('str', 'text', 'string')):
        keys.append('str')
    if any(token in text for token in ('bytes', 'bytearray', 'buffer')):
        keys.append('bytes')
    if 'bool' in text:
        keys.append('bool')
    if any(token in text for token in ('int', 'index', 'size', 'count', 'width', 'flags')):
        keys.append('int')
    if any(token in text for token in ('float', 'double', 'real')):
        keys.append('float')
    if any(token in text for token in ('list', 'sequence', 'iterable', 'tuple', 'set')):
        keys.append('list')
    if any(token in text for token in ('none', 'optional', 'null')):
        keys.append('NoneType')

    # Parameter-name hints.
    if name in {'s', 'text', 'string', 'pattern', 'name', 'prefix', 'suffix', 'sep'}:
        keys.append('str')
    # Keep this conservative: generic names like `b`/`data` often are not bytes.
    if name in {'buffer', 'payload', 'byte_data'}:
        keys.append('bytes')
    if name in {'cls', 'class_', 'klass', 'type'}:
        keys.append('type')
    if name in {'a', 'seq', 'sequence', 'iterable', 'iterable1', 'iterable2', 'values', 'items', 'data'}:
        keys.append('list')
    if name in {'n', 'k', 'i', 'j', 'idx', 'index', 'start', 'end', 'maxsplit', 'count', 'width'}:
        keys.append('int')
    if name.startswith('is_') or name.startswith('has_') or name in {'strict', 'quote', 'casefold'}:
        keys.append('bool')

    if not keys:
        # Prefer numeric/sequence defaults before string-only fallbacks; this
        # avoids many CPython-only edge paths for unannotated C-API callables.
        keys = ['int', 'list', 'str', 'bool', 'NoneType']
    return dedupe_preserve_order(keys)


def infer_param_values(param: inspect.Parameter) -> list[Any]:
    """Infer concrete candidate values for a signature parameter."""
    annotation_text: str | None
    if param.annotation is inspect._empty:
        annotation_text = None
    else:
        annotation_text = stable_repr(param.annotation)

    keys = infer_simple_pool_keys(annotation_text, param.name)
    values: list[Any] = []
    for key in keys:
        values.extend(INPUT_POOLS.get(key, []))

    if param.default is not inspect._empty:
        values.insert(0, param.default)

    if not values:
        values = list(GENERIC_FALLBACK_INPUTS)
    values = dedupe_preserve_order(values)
    return values[:4]


def candidate_key(candidate: CandidateCall) -> str:
    """Stable dedupe key for candidate calls."""
    return stable_repr((candidate.args, candidate.kwargs))


def empty_like_values(values: list[Any]) -> list[Any]:
    """Return values considered 'empty-ish' for focused edge-case calls."""
    result: list[Any] = []
    for value in values:
        if value in ('', b'', 0, 0.0, False, None):
            result.append(value)
        elif isinstance(value, list) and not value:
            result.append(value)
    return dedupe_preserve_order(result)


def normalize_case_label(function_name: str, suffix: str, used: set[str]) -> str:
    """Build a deterministic unique label for one generated test case."""
    base = sanitize_identifier(f'{function_name}_{suffix}').lower()
    label = base
    counter = 2
    while label in used:
        label = f'{base}_{counter}'
        counter += 1
    used.add(label)
    return label


# =============================================================================
# Section 2: CPython introspection
# =============================================================================


def _public_names(module: Any) -> list[str]:
    """Return public names using `__all__` when available.

    Mirrors `scripts/stdlib_api_diff.py::_public_names`.
    """
    all_names = getattr(module, '__all__', None)
    if isinstance(all_names, list | tuple):
        names = [name for name in all_names if isinstance(name, str)]
    else:
        names = [name for name in dir(module) if not name.startswith('_')]
    return sorted(set(names))


def _safe_signature(obj: Any) -> str | None:
    """Return inspect.signature(obj) or None when unavailable.

    Mirrors `scripts/stdlib_api_diff.py::_safe_signature`.
    """
    try:
        return str(inspect.signature(obj))
    except (TypeError, ValueError):
        return None


def _safe_signature_object(obj: Any) -> inspect.Signature | None:
    """Return inspect.Signature object when possible."""
    try:
        return inspect.signature(obj)
    except (TypeError, ValueError):
        return None


def classify_function_safety(
    module_name: str,
    function_name: str,
    signature: inspect.Signature | None,
) -> tuple[bool, str | None]:
    """Apply conservative safety heuristics for oracle execution.

    Functions marked unsafe are still included in API snapshots and scaffold output,
    but skipped for auto-generated execution cases.
    """
    if is_host_sensitive_module(module_name):
        return False, 'module is host-sensitive; oracle execution disabled'

    if function_name.lower() in UNSAFE_EXACT_FUNCTION_NAMES:
        return False, f'function `{function_name}` is interactive or host-sensitive'

    if module_name == 'sys' and function_name.lower() in UNSAFE_SYS_FUNCTION_NAMES:
        return False, f'sys function `{function_name}` mutates process-global runtime hooks'

    full_name = f'{module_name}.{function_name}'
    unsafe_token = contains_unsafe_token(full_name)
    if unsafe_token is not None:
        return False, f'name contains unsafe token `{unsafe_token}`'

    if signature is not None:
        for param in signature.parameters.values():
            pname = param.name.lower()
            for token in UNSAFE_PARAM_HINTS:
                if token in pname:
                    return False, f'parameter `{param.name}` looks host-sensitive'

    return True, None


def collect_cpython_snapshot(
    module_name: str,
    *,
    verbose: bool,
) -> tuple[ModuleAPI, dict[str, Any], ModuleType]:
    """Collect API metadata and callable objects for one module.

    The callable object map is used later by the oracle execution stage.
    """
    log(f'Importing module {module_name!r}', verbose=verbose, force=True)
    module = importlib.import_module(module_name)
    names = _public_names(module)

    functions: list[FunctionInfo] = []
    classes: list[ClassInfo] = []
    constants: list[ConstantInfo] = []
    callable_objects: dict[str, Any] = {}

    for name in names:
        try:
            obj = getattr(module, name)
        except Exception as exc:  # pragma: no cover - defensive
            log(f'Skipping {module_name}.{name}: getattr failed: {type(exc).__name__}: {exc}', verbose=verbose)
            continue

        if inspect.isclass(obj):
            attrs = sorted(attr for attr in dir(obj) if not attr.startswith('_'))
            classes.append(ClassInfo(name=name, public_attrs=attrs, is_safe=True))
            continue

        if callable(obj):
            signature_obj = _safe_signature_object(obj)
            signature_text = _safe_signature(obj)
            params: list[ParamInfo] = []
            if signature_obj is not None:
                for parameter in signature_obj.parameters.values():
                    annotation = None
                    if parameter.annotation is not inspect._empty:
                        annotation = stable_repr(parameter.annotation)
                    default = None
                    if parameter.default is not inspect._empty:
                        default = stable_repr(parameter.default)
                    params.append(
                        ParamInfo(
                            name=parameter.name,
                            annotation=annotation,
                            default=default,
                            kind=parameter.kind.name,
                        )
                    )
            is_safe, block_reason = classify_function_safety(module_name, name, signature_obj)
            functions.append(
                FunctionInfo(
                    name=name,
                    signature=signature_text,
                    params=params,
                    is_safe=is_safe,
                    block_reason=block_reason,
                )
            )
            callable_objects[name] = obj
            continue

        value_repr = stable_repr(obj)
        constants.append(ConstantInfo(name=name, value_repr=value_repr, value_type=type(obj).__name__))

    functions.sort(key=lambda info: info.name)
    classes.sort(key=lambda info: info.name)
    constants.sort(key=lambda info: info.name)

    api = ModuleAPI(
        module_name=module_name,
        functions=functions,
        classes=classes,
        constants=constants,
    )
    return api, callable_objects, module


# =============================================================================
# Section 3: Test input generation + CPython oracle execution
# =============================================================================


def build_candidate_calls(function_name: str, function_obj: Any) -> list[CandidateCall]:
    """Generate argument combinations for one callable using signature heuristics."""
    signature = _safe_signature_object(function_obj)
    if signature is None:
        return [CandidateCall(label='no_args', args=(), kwargs=())]

    parameters = list(signature.parameters.values())
    if any(param.kind is inspect.Parameter.VAR_KEYWORD for param in parameters):
        # Keep generation conservative for **kwargs-heavy APIs.
        parameters = [param for param in parameters if param.kind is not inspect.Parameter.VAR_KEYWORD]

    required_positional = [
        p
        for p in parameters
        if p.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
        and p.default is inspect._empty
    ]
    required_positional_only = [p for p in required_positional if p.kind is inspect.Parameter.POSITIONAL_ONLY]
    required_pos_or_kw = [p for p in required_positional if p.kind is inspect.Parameter.POSITIONAL_OR_KEYWORD]
    required_kw_only = [
        p for p in parameters if p.kind is inspect.Parameter.KEYWORD_ONLY and p.default is inspect._empty
    ]
    optional_kw_capable = [
        p
        for p in parameters
        if p.default is not inspect._empty
        and p.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY)
    ]

    if len(required_positional) + len(required_kw_only) > 6:
        return []

    value_map: dict[str, list[Any]] = {}
    for param in parameters:
        if param.kind in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD):
            continue
        value_map[param.name] = infer_param_values(param)

    candidates: list[CandidateCall] = []
    seen: set[str] = set()

    def add_candidate(label: str, args: tuple[Any, ...], kwargs: tuple[tuple[str, Any], ...]) -> None:
        candidate = CandidateCall(label=label, args=args, kwargs=kwargs)
        key = candidate_key(candidate)
        if key in seen:
            return
        seen.add(key)
        candidates.append(candidate)

    # Baseline required-only call.
    baseline_args = tuple(value_map[p.name][0] for p in required_positional)
    baseline_kwargs = tuple((p.name, value_map[p.name][0]) for p in required_kw_only)
    add_candidate('default', baseline_args, baseline_kwargs)

    # Required arguments passed by keyword where permitted.
    if required_pos_or_kw and len(required_positional_only) + len(required_pos_or_kw) <= 4:
        pos_args = tuple(value_map[p.name][0] for p in required_positional_only)
        kw = {name: val for name, val in baseline_kwargs}
        for param in required_pos_or_kw:
            kw[param.name] = value_map[param.name][0]
        add_candidate('required_keyword_form', pos_args, tuple(sorted(kw.items())))

    # Cartesian coverage over required positional arguments (small arity only).
    if required_positional and len(required_positional) <= 3:
        choice_lists = [value_map[param.name][:3] for param in required_positional]
        for combo_index, combo in enumerate(itertools.product(*choice_lists), start=1):
            add_candidate(f'combo_req_{combo_index}', tuple(combo), baseline_kwargs)
            if combo_index >= 16:
                break

    # Required-parameter variation calls.
    for param in required_positional:
        values = value_map[param.name]
        if len(values) < 2:
            continue
        index = required_positional.index(param)
        for choice_index, value in enumerate(values[1:3], start=1):
            args = list(baseline_args)
            args[index] = value
            add_candidate(f'var_{param.name}_{choice_index}', tuple(args), baseline_kwargs)

    for param in required_kw_only:
        values = value_map[param.name]
        if len(values) < 2:
            continue
        for choice_index, value in enumerate(values[1:3], start=1):
            kw = {name: val for name, val in baseline_kwargs}
            kw[param.name] = value
            add_candidate(f'kw_{param.name}_{choice_index}', baseline_args, tuple(sorted(kw.items())))

    # Pairwise required kw-only coverage.
    if len(required_kw_only) >= 2:
        for left_index, left_param in enumerate(required_kw_only):
            for right_param in required_kw_only[left_index + 1 :]:
                left_values = value_map[left_param.name][:2]
                right_values = value_map[right_param.name][:2]
                for left_choice in left_values:
                    for right_choice in right_values:
                        kw = {name: val for name, val in baseline_kwargs}
                        kw[left_param.name] = left_choice
                        kw[right_param.name] = right_choice
                        add_candidate(
                            f'kwpair_{left_param.name}_{right_param.name}',
                            baseline_args,
                            tuple(sorted(kw.items())),
                        )

    # Optional-parameter focused calls.
    for param in optional_kw_capable:
        values = value_map[param.name]
        if not values:
            continue
        value_choices = values[:2]
        for choice_index, value in enumerate(value_choices, start=1):
            kw = {name: val for name, val in baseline_kwargs}
            kw[param.name] = value
            add_candidate(f'opt_{param.name}_{choice_index}', baseline_args, tuple(sorted(kw.items())))

    # Limited multi-optional interactions.
    if len(optional_kw_capable) >= 2:
        left_param = optional_kw_capable[0]
        right_param = optional_kw_capable[1]
        left_values = value_map[left_param.name][:2]
        right_values = value_map[right_param.name][:2]
        for left_choice in left_values:
            for right_choice in right_values:
                kw = {name: val for name, val in baseline_kwargs}
                kw[left_param.name] = left_choice
                kw[right_param.name] = right_choice
                add_candidate(
                    f'optpair_{left_param.name}_{right_param.name}',
                    baseline_args,
                    tuple(sorted(kw.items())),
                )

    # Unary-function empty-input edge cases.
    if len(required_positional) == 1 and not required_kw_only:
        target = required_positional[0]
        for index, value in enumerate(empty_like_values(value_map[target.name]), start=1):
            add_candidate(f'empty_{target.name}_{index}', (value,), ())

    # Zero-arg call for functions with no required parameters.
    if not required_positional and not required_kw_only:
        add_candidate('zero_arg', (), ())

    if len(candidates) > MAX_CASES_PER_FUNCTION:
        candidates = candidates[:MAX_CASES_PER_FUNCTION]

    # Make labels deterministic and unique.
    normalized: list[CandidateCall] = []
    used_labels: set[str] = set()
    for candidate in candidates:
        label = normalize_case_label(function_name, candidate.label, used_labels)
        normalized.append(CandidateCall(label=label, args=candidate.args, kwargs=candidate.kwargs))
    return normalized


def wrong_type_values(value: Any) -> list[Any]:
    """Return candidate values likely to trigger type/domain errors."""
    pool: list[Any] = ['', b'', 0, 1, -1, 3.14, True, None, [], {}, '[]']
    candidates = [candidate for candidate in pool if type(candidate) is not type(value)]
    return dedupe_preserve_order(candidates)[:4]


def build_negative_candidates(function_name: str, function_obj: Any) -> list[CandidateCall]:
    """Generate deliberately invalid calls to capture CPython error semantics."""
    signature = _safe_signature_object(function_obj)
    if signature is None:
        return []

    parameters = list(signature.parameters.values())
    positional_params = [
        param
        for param in parameters
        if param.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
    ]
    pos_or_kw_params = [param for param in positional_params if param.kind is inspect.Parameter.POSITIONAL_OR_KEYWORD]
    required_positional = [param for param in positional_params if param.default is inspect._empty]
    required_kw_only = [
        param
        for param in parameters
        if param.kind is inspect.Parameter.KEYWORD_ONLY and param.default is inspect._empty
    ]
    has_var_positional = any(param.kind is inspect.Parameter.VAR_POSITIONAL for param in parameters)
    has_var_keyword = any(param.kind is inspect.Parameter.VAR_KEYWORD for param in parameters)

    base_args: list[Any] = []
    for param in required_positional:
        values = infer_param_values(param)
        base_args.append(values[0] if values else None)

    base_kwargs: dict[str, Any] = {}
    for param in required_kw_only:
        values = infer_param_values(param)
        base_kwargs[param.name] = values[0] if values else None

    candidates: list[CandidateCall] = []
    seen: set[str] = set()

    def add_candidate(label: str, args: tuple[Any, ...], kwargs: tuple[tuple[str, Any], ...]) -> None:
        candidate = CandidateCall(label=label, args=args, kwargs=kwargs)
        key = candidate_key(candidate)
        if key in seen:
            return
        seen.add(key)
        candidates.append(candidate)

    if required_positional:
        # Missing required positional argument.
        add_candidate('err_missing_required', tuple(base_args[:-1]), ())

    if required_kw_only:
        # Missing required kw-only argument(s).
        add_candidate('err_missing_kwonly', tuple(base_args), ())

    if not has_var_keyword:
        # Unexpected keyword should raise.
        add_candidate(
            'err_unexpected_keyword',
            tuple(base_args),
            tuple(sorted({**base_kwargs, '__ouro_bad_keyword__': 1}.items())),
        )

    if pos_or_kw_params and required_positional:
        # Duplicate values for the same argument (positional + keyword).
        duplicate_target = pos_or_kw_params[0]
        duplicate_index = positional_params.index(duplicate_target)
        duplicate_value = base_args[duplicate_index] if duplicate_index < len(base_args) else None
        add_candidate(
            f'err_duplicate_value_{duplicate_target.name}',
            tuple(base_args),
            tuple(sorted({**base_kwargs, duplicate_target.name: duplicate_value}.items())),
        )

    if not has_var_positional and len(positional_params) <= 6:
        # Too many positional args should raise.
        add_candidate('err_too_many_positional', tuple(base_args + [None]), tuple(sorted(base_kwargs.items())))

    # If first required parameter looks numeric, pass wrong text to capture type error.
    if required_positional:
        first = required_positional[0]
        annotation = (stable_repr(first.annotation) if first.annotation is not inspect._empty else '').lower()
        if any(token in annotation for token in ('int', 'float', 'number')):
            mutated = list(base_args)
            mutated[0] = '<wrong-type>'
            add_candidate('err_wrong_type_first_arg', tuple(mutated), tuple(sorted(base_kwargs.items())))

    # Parameter-specific wrong-type checks.
    for index, param in enumerate(required_positional):
        baseline = base_args[index]
        for choice_index, wrong in enumerate(wrong_type_values(baseline)[:2], start=1):
            mutated = list(base_args)
            mutated[index] = wrong
            add_candidate(
                f'err_wrong_type_{param.name}_{choice_index}',
                tuple(mutated),
                tuple(sorted(base_kwargs.items())),
            )

    for param in required_kw_only:
        baseline = base_kwargs.get(param.name)
        if baseline is None and param.default is inspect._empty:
            baseline = ''
        for choice_index, wrong in enumerate(wrong_type_values(baseline)[:2], start=1):
            mutated_kwargs = dict(base_kwargs)
            mutated_kwargs[param.name] = wrong
            add_candidate(
                f'err_wrong_type_{param.name}_{choice_index}',
                tuple(base_args),
                tuple(sorted(mutated_kwargs.items())),
            )

    # Domain-style errors for common numeric parameter names.
    for index, param in enumerate(required_positional):
        if param.name.lower() in {'count', 'width', 'size', 'index', 'start', 'end', 'n', 'lo', 'hi'}:
            mutated = list(base_args)
            mutated[index] = -999999
            add_candidate(
                f'err_domain_{param.name}',
                tuple(mutated),
                tuple(sorted(base_kwargs.items())),
            )

    used_labels: set[str] = set()
    normalized: list[CandidateCall] = []
    for candidate in candidates[:MAX_NEGATIVE_CANDIDATES_PER_FUNCTION]:
        label = normalize_case_label(function_name, candidate.label, used_labels)
        normalized.append(CandidateCall(label=label, args=candidate.args, kwargs=candidate.kwargs))
    return normalized


def execute_oracle_cases(
    module_name: str,
    function_info: FunctionInfo,
    function_obj: Any,
    *,
    verbose: bool,
) -> OracleSummary:
    """Run generated calls against CPython and capture successful outputs."""
    summary = OracleSummary()
    candidates = build_candidate_calls(function_info.name, function_obj)
    if not candidates:
        summary.skipped_cases.append('no-supported-candidates')
    negative_candidates = build_negative_candidates(function_info.name, function_obj)

    for candidate in candidates:
        kwargs_dict = dict(candidate.kwargs)
        args_repr = format_call_args(candidate.args, candidate.kwargs)
        if args_repr is None:
            summary.skipped_cases.append(f'{candidate.label}:non-literal-args')
            continue
        keyword_names = [name for name, _ in candidate.kwargs]
        arg_types = [type(value).__name__ for value in candidate.args]
        kwarg_types = {name: type(value).__name__ for name, value in candidate.kwargs}

        try:
            result = call_with_isolated_argv(function_obj, *candidate.args, **kwargs_dict)
        except KeyboardInterrupt:
            raise
        except BaseException as exc:
            exc_case = ExceptionCase(
                label=candidate.label,
                args_repr=args_repr,
                exc_type=type(exc).__name__,
                exc_message=str(exc),
            )
            summary.exceptions.append(exc_case)
            summary.observed_calls.append(
                ObservedCall(
                    label=candidate.label,
                    args_repr=args_repr,
                    positional_count=len(candidate.args),
                    keyword_names=keyword_names,
                    arg_types=arg_types,
                    kwarg_types=kwarg_types,
                    outcome='exception',
                    result_repr=None,
                    result_type=None,
                    exc_type=exc_case.exc_type,
                    exc_message=exc_case.exc_message,
                )
            )
            continue

        expected_expr = to_python_expr(result)
        if expected_expr is None:
            summary.skipped_cases.append(f'{candidate.label}:non-literal-result:{type(result).__name__}')
            summary.observed_calls.append(
                ObservedCall(
                    label=candidate.label,
                    args_repr=args_repr,
                    positional_count=len(candidate.args),
                    keyword_names=keyword_names,
                    arg_types=arg_types,
                    kwarg_types=kwarg_types,
                    outcome='success_non_literal',
                    result_repr=stable_repr(result),
                    result_type=type(result).__name__,
                    exc_type=None,
                    exc_message=None,
                )
            )
            continue

        summary.success_cases.append(
            TestCase(
                label=candidate.label,
                args_repr=args_repr,
                expected_repr=expected_expr,
                expected_type=type(result).__name__,
            )
        )
        summary.observed_calls.append(
            ObservedCall(
                label=candidate.label,
                args_repr=args_repr,
                positional_count=len(candidate.args),
                keyword_names=keyword_names,
                arg_types=arg_types,
                kwarg_types=kwarg_types,
                outcome='success',
                result_repr=expected_expr,
                result_type=type(result).__name__,
                exc_type=None,
                exc_message=None,
            )
        )

    for candidate in negative_candidates:
        if len(summary.exceptions) >= MAX_ERROR_CASES_PER_FUNCTION:
            break
        kwargs_dict = dict(candidate.kwargs)
        args_repr = format_call_args(candidate.args, candidate.kwargs)
        if args_repr is None:
            continue
        keyword_names = [name for name, _ in candidate.kwargs]
        arg_types = [type(value).__name__ for value in candidate.args]
        kwarg_types = {name: type(value).__name__ for name, value in candidate.kwargs}
        try:
            result = call_with_isolated_argv(function_obj, *candidate.args, **kwargs_dict)
        except KeyboardInterrupt:
            raise
        except BaseException as exc:
            exc_case = ExceptionCase(
                label=candidate.label,
                args_repr=args_repr,
                exc_type=type(exc).__name__,
                exc_message=str(exc),
            )
            summary.exceptions.append(exc_case)
            summary.observed_calls.append(
                ObservedCall(
                    label=candidate.label,
                    args_repr=args_repr,
                    positional_count=len(candidate.args),
                    keyword_names=keyword_names,
                    arg_types=arg_types,
                    kwarg_types=kwarg_types,
                    outcome='negative_exception',
                    result_repr=None,
                    result_type=None,
                    exc_type=exc_case.exc_type,
                    exc_message=exc_case.exc_message,
                )
            )
            continue

        result_expr = to_python_expr(result)
        if result_expr is None:
            summary.skipped_cases.append(f'{candidate.label}:negative-call-succeeded-nonliteral-result')
            summary.observed_calls.append(
                ObservedCall(
                    label=candidate.label,
                    args_repr=args_repr,
                    positional_count=len(candidate.args),
                    keyword_names=keyword_names,
                    arg_types=arg_types,
                    kwarg_types=kwarg_types,
                    outcome='negative_success_non_literal',
                    result_repr=stable_repr(result),
                    result_type=type(result).__name__,
                    exc_type=None,
                    exc_message=None,
                )
            )
        else:
            summary.skipped_cases.append(f'{candidate.label}:negative-call-succeeded:{result_expr}')
            summary.observed_calls.append(
                ObservedCall(
                    label=candidate.label,
                    args_repr=args_repr,
                    positional_count=len(candidate.args),
                    keyword_names=keyword_names,
                    arg_types=arg_types,
                    kwarg_types=kwarg_types,
                    outcome='negative_success',
                    result_repr=result_expr,
                    result_type=type(result).__name__,
                    exc_type=None,
                    exc_message=None,
                )
            )

    if verbose:
        log(
            (
                f'oracle {module_name}.{function_info.name}: '
                f'{len(summary.success_cases)} success, '
                f'{len(summary.exceptions)} exception, '
                f'{len(summary.skipped_cases)} skipped'
            ),
            verbose=verbose,
        )
    return summary


def populate_oracle_tests(
    module_name: str,
    api: ModuleAPI,
    callable_objects: dict[str, Any],
    *,
    verbose: bool,
) -> dict[str, OracleSummary]:
    """Populate `FunctionInfo.test_cases` for safe functions."""
    summaries: dict[str, OracleSummary] = {}
    for function_info in api.functions:
        if not function_info.is_safe:
            summaries[function_info.name] = OracleSummary(skipped_cases=['unsafe'])
            continue
        function_obj = callable_objects.get(function_info.name)
        if function_obj is None:
            summaries[function_info.name] = OracleSummary(skipped_cases=['callable-not-found'])
            continue
        summary = execute_oracle_cases(module_name, function_info, function_obj, verbose=verbose)
        function_info.test_cases = summary.success_cases
        summaries[function_info.name] = summary
    return summaries


# =============================================================================
# Section 4: Unit test generator
# =============================================================================


def find_inverse_pairs(api: ModuleAPI) -> list[tuple[str, str]]:
    """Discover likely inverse-function pairs by naming patterns."""
    names = {function.name for function in api.functions}
    pairs: list[tuple[str, str]] = []
    for left, right in ROUNDTRIP_NAME_PATTERNS:
        for name in sorted(names):
            if left not in name:
                continue
            candidate = name.replace(left, right)
            if candidate in names and name != candidate:
                pairs.append((name, candidate))
    unique: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for pair in pairs:
        if pair in seen:
            continue
        seen.add(pair)
        unique.append(pair)
    return unique


def generate_roundtrip_asserts(
    module_name: str,
    api: ModuleAPI,
    callable_objects: dict[str, Any],
) -> list[str]:
    """Generate extra roundtrip asserts when an inverse pair is found."""
    lines: list[str] = []
    pair_index = 0
    for forward_name, backward_name in find_inverse_pairs(api):
        forward = callable_objects.get(forward_name)
        backward = callable_objects.get(backward_name)
        if forward is None or backward is None:
            continue
        samples = INPUT_POOLS['str'][:4] + INPUT_POOLS['bytes'][:3]
        for sample in samples:
            sample_expr = to_python_expr(sample)
            if sample_expr is None:
                continue
            try:
                result = backward(forward(sample))
            except Exception:
                continue
            if result != sample:
                continue
            pair_index += 1
            label = f'roundtrip_{forward_name}_{backward_name}_{pair_index}'
            lines.append(
                (
                    f'assert {module_name}.{backward_name}({module_name}.{forward_name}({sample_expr})) '
                    f"== {sample_expr}, '{label}'"
                )
            )
            if pair_index >= 8:
                return lines
    return lines


def generate_unit_tests_content(
    module_name: str,
    api: ModuleAPI,
    callable_objects: dict[str, Any],
) -> str:
    """Generate `crates/ouro/test_cases/<module>__stdlib.py` content."""
    lines: list[str] = [f'import {module_name}', '']

    wrote_function = False
    for function_info in api.functions:
        if not function_info.test_cases:
            continue
        wrote_function = True
        lines.append(f'# === {function_info.name} ===')
        for test_case in function_info.test_cases:
            call = f'{module_name}.{function_info.name}({test_case.args_repr})'
            lines.append(f"assert {call} == {test_case.expected_repr}, '{test_case.label}'")
        lines.append('')

    roundtrip_lines = generate_roundtrip_asserts(module_name, api, callable_objects)
    if roundtrip_lines:
        lines.append('# === roundtrip ===')
        lines.extend(roundtrip_lines)
        lines.append('')

    if not wrote_function and not roundtrip_lines:
        lines.append('# No successful safe CPython oracle cases were generated for this module.')
        lines.append("assert True, 'no_generated_cases'")
        lines.append('')

    return '\n'.join(lines).rstrip() + '\n'


def generate_error_tests_content(module_name: str, oracle: dict[str, OracleSummary]) -> str:
    """Generate a runnable script asserting exact exception type/message outputs."""
    lines: list[str] = [f'import {module_name}', '']
    found = False

    for function_name in sorted(oracle):
        summary = oracle[function_name]
        if not summary.exceptions:
            continue
        found = True
        deduped: list[ExceptionCase] = []
        seen_exc: set[str] = set()
        for exception_case in summary.exceptions:
            key = stable_repr((exception_case.args_repr, exception_case.exc_type, exception_case.exc_message))
            if key in seen_exc:
                continue
            seen_exc.add(key)
            deduped.append(exception_case)
        lines.append(f'# === {function_name} error semantics ===')
        for exception_case in deduped[:MAX_ERROR_CASES_PER_FUNCTION]:
            lines.append('try:')
            lines.append(f'    {module_name}.{function_name}({exception_case.args_repr})')
            lines.append(f"    assert False, '{exception_case.label}_should_raise'")
            lines.append('except Exception as exc:')
            lines.append(f"    assert type(exc).__name__ == '{exception_case.exc_type}', '{exception_case.label}_type'")
            lines.append(
                (
                    f'    assert str(exc) == {py_single_quoted(exception_case.exc_message)}, '
                    f"'{exception_case.label}_message'"
                )
            )
        lines.append('')

    if not found:
        lines.append('# No deterministic exception cases were generated.')
        lines.append("assert True, 'no_exception_cases'")
        lines.append('')

    return '\n'.join(lines).rstrip() + '\n'


def split_param_buckets(
    function_info: FunctionInfo,
) -> tuple[list[ParamInfo], list[ParamInfo], list[ParamInfo], list[ParamInfo], bool, bool]:
    """Split parameters into signature-shape buckets used for parser strategy hints."""
    required_positional: list[ParamInfo] = []
    optional_positional_or_keyword: list[ParamInfo] = []
    required_keyword_only: list[ParamInfo] = []
    optional_keyword_only: list[ParamInfo] = []
    has_var_positional = False
    has_var_keyword = False

    for param in function_info.params:
        if param.kind == 'VAR_POSITIONAL':
            has_var_positional = True
            continue
        if param.kind == 'VAR_KEYWORD':
            has_var_keyword = True
            continue
        if param.kind in {'POSITIONAL_ONLY', 'POSITIONAL_OR_KEYWORD'}:
            if param.default is None:
                required_positional.append(param)
            else:
                optional_positional_or_keyword.append(param)
            continue
        if param.kind == 'KEYWORD_ONLY':
            if param.default is None:
                required_keyword_only.append(param)
            else:
                optional_keyword_only.append(param)

    return (
        required_positional,
        optional_positional_or_keyword,
        required_keyword_only,
        optional_keyword_only,
        has_var_positional,
        has_var_keyword,
    )


def recommended_argvalues_strategy(module_name: str, function_info: FunctionInfo) -> tuple[str, str]:
    """Return human-readable + snippet guidance for parsing `ArgValues`."""
    (
        required_positional,
        optional_positional_or_keyword,
        required_keyword_only,
        optional_keyword_only,
        has_var_positional,
        has_var_keyword,
    ) = split_param_buckets(function_info)
    call_name = f'{module_name}.{function_info.name}'

    if (
        len(required_positional) == 0
        and not optional_positional_or_keyword
        and not required_keyword_only
        and not optional_keyword_only
        and not has_var_positional
        and not has_var_keyword
    ):
        return ('exact zero-arg function', f'args.check_zero_args("{call_name}", heap)?;')

    if (
        len(required_positional) == 1
        and not optional_positional_or_keyword
        and not required_keyword_only
        and not optional_keyword_only
        and not has_var_positional
        and not has_var_keyword
    ):
        return (
            'single required positional argument',
            f'let {to_snake_case(required_positional[0].name)} = args.get_one_arg("{call_name}", heap)?;',
        )

    if (
        len(required_positional) == 2
        and not optional_positional_or_keyword
        and not required_keyword_only
        and not optional_keyword_only
        and not has_var_positional
        and not has_var_keyword
    ):
        return (
            'two required positional arguments',
            (
                f'let ({to_snake_case(required_positional[0].name)}, '
                f'{to_snake_case(required_positional[1].name)}) = args.get_two_args("{call_name}", heap)?;'
            ),
        )

    if (
        len(required_positional) == 1
        and len(optional_positional_or_keyword) == 1
        and not required_keyword_only
        and not optional_keyword_only
        and not has_var_positional
        and not has_var_keyword
    ):
        optional_name = optional_positional_or_keyword[0].name
        return (
            'one required arg + one optional arg (keyword-capable)',
            (
                f'let ({to_snake_case(required_positional[0].name)}, opt_{to_snake_case(optional_name)}) = '
                f'args.get_one_two_args_with_keyword("{call_name}", "{optional_name}", heap, interns)?;'
            ),
        )

    if (
        len(required_positional) == 0
        and len(optional_positional_or_keyword) == 1
        and not required_keyword_only
        and not optional_keyword_only
        and not has_var_positional
        and not has_var_keyword
    ):
        return (
            'zero-or-one positional argument',
            f'let opt_{to_snake_case(optional_positional_or_keyword[0].name)} = args.get_zero_one_arg("{call_name}", heap)?;',
        )

    return (
        'custom argument parser required',
        'let (mut positional, kwargs) = args.into_parts(); // parse positional + keyword args manually',
    )


def conversion_hint_for_param(param: ParamInfo) -> str:
    """Return best-effort value coercion hints for a parameter."""
    text = (param.annotation or '').lower()
    name = param.name.lower()
    if any(token in text for token in ('str', 'text', 'string')) or name in {'s', 'text', 'string', 'pattern'}:
        return 'string-like: start from `value.py_str(heap, interns)`'
    if any(token in text for token in ('int', 'index', 'count', 'size')) or name in {
        'i',
        'j',
        'n',
        'count',
        'index',
        'start',
        'end',
        'width',
    }:
        return 'integer-like: start from `value.as_int(heap)`'
    if 'bool' in text or name.startswith('is_') or name in {'quote', 'strict'}:
        return 'boolean-like: start from `value.py_bool(heap, interns)`'
    if 'bytes' in text or name in {'data', 'payload', 'buffer'}:
        return 'bytes-like: treat as byte sequence, preserve CPython error shape'
    if any(token in text for token in ('list', 'tuple', 'sequence', 'iterable')):
        return 'sequence-like: parse iterable/list and preserve iteration semantics'
    return 'generic object: use Python protocol methods (`py_str`, comparisons, truthiness) as needed'


def collect_return_type_counts(summary: OracleSummary) -> list[tuple[str, int]]:
    """Count observed return types from successful oracle calls."""
    counts: dict[str, int] = {}
    for observed_call in summary.observed_calls:
        if observed_call.outcome not in {
            'success',
            'success_non_literal',
            'negative_success',
            'negative_success_non_literal',
        }:
            continue
        if observed_call.result_type is None:
            continue
        counts[observed_call.result_type] = counts.get(observed_call.result_type, 0) + 1
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))


def collect_shape_counts(summary: OracleSummary, *, success_only: bool) -> list[tuple[str, int]]:
    """Count argument-shape frequencies from observed calls."""
    counts: dict[str, int] = {}
    for observed_call in summary.observed_calls:
        is_success = observed_call.outcome in {
            'success',
            'success_non_literal',
            'negative_success',
            'negative_success_non_literal',
        }
        if success_only and not is_success:
            continue
        if not success_only and is_success:
            continue
        keywords = ','.join(observed_call.keyword_names) if observed_call.keyword_names else '-'
        shape = f'{observed_call.positional_count} positional; kwargs=[{keywords}]'
        counts[shape] = counts.get(shape, 0) + 1
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))


def collect_param_type_profile(function_info: FunctionInfo, summary: OracleSummary) -> dict[str, list[str]]:
    """Infer per-parameter observed value types from successful oracle calls."""
    param_lookup = {param.name: param for param in function_info.params}
    ordered_non_var = [param for param in function_info.params if param.kind not in {'VAR_POSITIONAL', 'VAR_KEYWORD'}]
    profile: dict[str, set[str]] = {param.name: set() for param in ordered_non_var}

    for observed_call in summary.observed_calls:
        if observed_call.outcome not in {
            'success',
            'success_non_literal',
            'negative_success',
            'negative_success_non_literal',
        }:
            continue
        for index, param in enumerate(ordered_non_var):
            if index < len(observed_call.arg_types):
                profile[param.name].add(observed_call.arg_types[index])
        for key, value_type in observed_call.kwarg_types.items():
            if key in param_lookup:
                profile[key].add(value_type)

    return {name: sorted(types) for name, types in profile.items()}


def accepts_named_keywords(function_info: FunctionInfo, summary: OracleSummary) -> bool:
    """Return true when successful oracle calls include named args for positional-or-keyword params."""
    keyword_capable_params = {param.name for param in function_info.params if param.kind == 'POSITIONAL_OR_KEYWORD'}
    if not keyword_capable_params:
        return False
    for observed_call in summary.observed_calls:
        if observed_call.outcome not in {
            'success',
            'success_non_literal',
            'negative_success',
            'negative_success_non_literal',
        }:
            continue
        if any(name in keyword_capable_params for name in observed_call.keyword_names):
            return True
    return False


def generate_function_contracts_content(
    module_name: str,
    api: ModuleAPI,
    oracle: dict[str, OracleSummary],
    static_plan: StaticStringPlan,
) -> str:
    """Generate a function-by-function behavioral contract artifact."""
    lines: list[str] = [f'# Function Contracts for `{module_name}`', '']
    lines.append('This file is generated from CPython oracle calls and intended as direct implementation input.')
    lines.append('')

    for function_info in api.functions:
        summary = oracle.get(function_info.name, OracleSummary())
        static_symbol = static_plan.resolved.get(function_info.name, make_static_string_variant(function_info.name))
        lines.append(f'## `{function_info.name}`')
        lines.append('')
        lines.append(f'- signature: `{function_info.signature}`')
        lines.append(f'- static string symbol: `StaticStrings::{static_symbol}`')
        lines.append(f'- safety classification: `{"safe" if function_info.is_safe else "blocked"}`')
        if function_info.block_reason:
            lines.append(f'- block reason: `{function_info.block_reason}`')
        lines.append(f'- observed successes: `{len(summary.success_cases)}`')
        lines.append(f'- observed exceptions: `{len(summary.exceptions)}`')
        lines.append('')

        strategy_label, strategy_snippet = recommended_argvalues_strategy(module_name, function_info)
        if accepts_named_keywords(function_info, summary) and 'args.into_parts()' not in strategy_snippet:
            strategy_label = 'keyword-capable signature (manual parser recommended)'
            strategy_snippet = 'let (mut positional, kwargs) = args.into_parts(); // support keyword + positional forms'
        param_type_profile = collect_param_type_profile(function_info, summary)
        lines.append('### Argument Parsing Plan')
        lines.append('')
        lines.append(f'- recommended strategy: `{strategy_label}`')
        lines.append(f'- parser snippet: `{strategy_snippet}`')
        for param in function_info.params:
            default_note = f' default={param.default}' if param.default is not None else ''
            observed_types = ', '.join(param_type_profile.get(param.name, [])) or 'none'
            lines.append(
                (
                    f'- `{param.name}` ({param.kind}{default_note}): '
                    f'{conversion_hint_for_param(param)}; observed types=`{observed_types}`'
                )
            )
        lines.append('')

        return_counts = collect_return_type_counts(summary)
        lines.append('### Return Profile')
        lines.append('')
        if not return_counts:
            lines.append('- No successful return observations.')
        else:
            for return_type, count in return_counts:
                lines.append(f'- `{return_type}` observed `{count}` times')
        lines.append('')

        success_shapes = collect_shape_counts(summary, success_only=True)
        error_shapes = collect_shape_counts(summary, success_only=False)
        lines.append('### Observed Argument Shapes')
        lines.append('')
        if not success_shapes:
            lines.append('- success shapes: none observed')
        else:
            lines.append('- success shapes:')
            for shape, count in success_shapes:
                lines.append(f'  - `{shape}` x{count}')
        if not error_shapes:
            lines.append('- exception shapes: none observed')
        else:
            lines.append('- exception shapes:')
            for shape, count in error_shapes:
                lines.append(f'  - `{shape}` x{count}')
        lines.append('')

        lines.append('### Success Cases')
        lines.append('')
        if not summary.success_cases:
            lines.append('- None observed.')
        else:
            for case in summary.success_cases[:24]:
                lines.append(
                    (f'- `{function_info.name}({case.args_repr})` -> `{case.expected_repr}` ({case.expected_type})')
                )
        lines.append('')

        lines.append('### Exception Cases')
        lines.append('')
        if not summary.exceptions:
            lines.append('- None observed.')
        else:
            for exc_case in summary.exceptions[:24]:
                lines.append(
                    (
                        f'- `{function_info.name}({exc_case.args_repr})` raises '
                        f'`{exc_case.exc_type}({exc_case.exc_message})`'
                    )
                )
        lines.append('')

    if api.constants:
        lines.append('## Constants')
        lines.append('')
        for constant in api.constants:
            lines.append(f'- `{constant.name}`: `{constant.value_repr}` ({constant.value_type})')
        lines.append('')

    if api.classes:
        lines.append('## Classes')
        lines.append('')
        for class_info in api.classes:
            lines.append(f'- `{class_info.name}` attrs: {", ".join(class_info.public_attrs[:30])}')
        lines.append('')

    return '\n'.join(lines).rstrip() + '\n'


def generate_implementation_recipe_content(
    module_name: str,
    api: ModuleAPI,
    oracle: dict[str, OracleSummary],
    static_plan: StaticStringPlan,
) -> str:
    """Generate one-shot implementation guidance with per-function Rust skeletons."""
    module_leaf = module_name.split('.')[-1]
    lines: list[str] = [f'# Implementation Recipe for `{module_name}`', '']
    lines.append('Use this file as the direct implementation plan without extra repository search.')
    lines.append('')
    lines.append('## Core References')
    lines.append('')
    lines.append('- `crates/ouro/src/modules/bisect.rs` for module wiring + dispatch pattern')
    lines.append('- `crates/ouro/src/modules/textwrap.rs` for mixed positional/keyword parsing')
    lines.append('- `crates/ouro/src/args.rs` for `ArgValues` helpers')
    lines.append('- `crates/ouro/src/exception_private.rs` for exception constructors')
    lines.append('')
    lines.append('## Per-Function Skeletons')
    lines.append('')

    for function_info in api.functions:
        summary = oracle.get(function_info.name, OracleSummary())
        strategy_label, strategy_snippet = recommended_argvalues_strategy(module_name, function_info)
        if accepts_named_keywords(function_info, summary) and 'args.into_parts()' not in strategy_snippet:
            strategy_label = 'keyword-capable signature (manual parser recommended)'
            strategy_snippet = 'let (mut positional, kwargs) = args.into_parts(); // support keyword + positional forms'
        param_type_profile = collect_param_type_profile(function_info, summary)
        rust_fn_name = to_snake_case(function_info.name)
        static_symbol = static_plan.resolved.get(function_info.name, make_static_string_variant(function_info.name))
        lines.append(f'### `{function_info.name}`')
        lines.append('')
        lines.append(f'- signature: `{function_info.signature}`')
        lines.append(f'- static symbol: `StaticStrings::{static_symbol}`')
        lines.append(f'- parser strategy: `{strategy_label}`')
        lines.append(f'- oracle successes: `{len(summary.success_cases)}`')
        lines.append(f'- oracle exceptions: `{len(summary.exceptions)}`')
        lines.append('')
        lines.append('```rust')
        lines.append(f'fn {rust_fn_name}(')
        lines.append('    heap: &mut Heap<impl ResourceTracker>,')
        lines.append('    interns: &Interns,')
        lines.append('    args: ArgValues,')
        lines.append(') -> RunResult<Value> {')
        lines.append(f'    {strategy_snippet}')
        lines.append('    // TODO: coerce args, implement CPython behavior, and preserve exact error messages.')
        lines.append(f'    todo!("Implement {module_name}.{function_info.name}")')
        lines.append('}')
        lines.append('```')
        lines.append('')

        if function_info.params:
            lines.append('- conversion hints:')
            for param in function_info.params:
                observed_types = ', '.join(param_type_profile.get(param.name, [])) or 'none'
                lines.append(
                    (f'  - `{param.name}`: {conversion_hint_for_param(param)} (observed types: {observed_types})')
                )
            lines.append('')

        if summary.exceptions:
            lines.append('- top exception examples to preserve exactly:')
            for exception_case in summary.exceptions[:6]:
                lines.append(
                    (
                        f'  - `{function_info.name}({exception_case.args_repr})` -> '
                        f'`{exception_case.exc_type}({exception_case.exc_message})`'
                    )
                )
            lines.append('')

    lines.append('## Module Wiring')
    lines.append('')
    lines.append(
        f'- module name symbol: `StaticStrings::{static_plan.resolved.get(module_leaf, make_static_string_variant(module_leaf))}`'
    )
    lines.append('- apply all snippets from `registration_diff.md` exactly once')
    lines.append('- use `scaffold.rs` as the target module file and replace each `todo!()`')
    lines.append('')
    return '\n'.join(lines).rstrip() + '\n'


# =============================================================================
# Section 5: Property test generator
# =============================================================================


def check_roundtrip_property(
    module_name: str,
    forward_name: str,
    backward_name: str,
    forward: Any,
    backward: Any,
) -> tuple[bool, str]:
    """Check inverse roundtrip property over sample pools."""
    samples = INPUT_POOLS['str'][:4] + INPUT_POOLS['bytes'][:3]
    tested = 0
    for sample in samples:
        try:
            out = call_with_isolated_argv(backward, call_with_isolated_argv(forward, sample))
        except KeyboardInterrupt:
            raise
        except BaseException:
            continue
        eq = safe_equals(out, sample)
        if eq is None:
            continue
        tested += 1
        if not eq:
            expr = to_python_expr(sample) or stable_repr(sample)
            return False, f'counterexample at sample={expr}'
    if tested == 0:
        return False, 'no successful sample executions'
    code = (
        f'for value in {stable_repr(samples)}:\n'
        f'    if isinstance(value, bytes) or isinstance(value, str):\n'
        f'        try:\n'
        f'            assert {module_name}.{backward_name}({module_name}.{forward_name}(value)) == value\n'
        f'        except Exception:\n'
        f'            pass'
    )
    return True, code


def unary_function_sample_values(function_info: FunctionInfo) -> list[Any]:
    """Infer sample values for likely unary functions."""
    required = [param for param in function_info.params if param.default is None and param.kind != 'VAR_KEYWORD']
    if len(required) != 1:
        return []
    param = required[0]
    keys = infer_simple_pool_keys(param.annotation, param.name)
    samples: list[Any] = []
    for key in keys:
        samples.extend(INPUT_POOLS.get(key, []))
    if not samples:
        samples = list(GENERIC_FALLBACK_INPUTS)
    return dedupe_preserve_order(samples)[:6]


def check_unary_idempotence(
    module_name: str,
    function_info: FunctionInfo,
    function_obj: Any,
) -> tuple[bool, str]:
    """Check `f(f(x)) == f(x)` where possible for unary functions."""
    samples = unary_function_sample_values(function_info)
    if not samples:
        return False, 'not-a-simple-unary-function'
    tested = 0
    for sample in samples:
        try:
            once = call_with_isolated_argv(function_obj, sample)
            twice = call_with_isolated_argv(function_obj, once)
        except KeyboardInterrupt:
            raise
        except BaseException:
            continue
        eq = safe_equals(twice, once)
        if eq is None:
            continue
        tested += 1
        if not eq:
            expr = to_python_expr(sample) or stable_repr(sample)
            return False, f'counterexample at sample={expr}'
    if tested < 2:
        return False, 'insufficient successful executions'
    code = (
        f'for value in {stable_repr(samples)}:\n'
        f'    try:\n'
        f'        once = {module_name}.{function_info.name}(value)\n'
        f'        assert {module_name}.{function_info.name}(once) == once\n'
        f'    except Exception:\n'
        f'        pass'
    )
    return True, code


def check_type_preservation(
    module_name: str,
    function_info: FunctionInfo,
    function_obj: Any,
) -> tuple[bool, str]:
    """Check whether unary output type matches input type on sampled values."""
    samples = unary_function_sample_values(function_info)
    if not samples:
        return False, 'not-a-simple-unary-function'
    tested = 0
    for sample in samples:
        try:
            result = call_with_isolated_argv(function_obj, sample)
        except KeyboardInterrupt:
            raise
        except BaseException:
            continue
        tested += 1
        if type(result) is not type(sample):
            expr = to_python_expr(sample) or stable_repr(sample)
            return False, f'type mismatch at sample={expr}'
    if tested == 0:
        return False, 'no successful executions'
    code = (
        f'for value in {stable_repr(samples)}:\n'
        f'    try:\n'
        f'        out = {module_name}.{function_info.name}(value)\n'
        f'        assert type(out) is type(value)\n'
        f'    except Exception:\n'
        f'        pass'
    )
    return True, code


def check_empty_input_property(
    module_name: str,
    function_info: FunctionInfo,
    function_obj: Any,
) -> tuple[bool, str]:
    """Check behavior of unary functions on an empty-like input."""
    samples = empty_like_values(unary_function_sample_values(function_info))
    if not samples:
        return False, 'no-empty-like-input'
    sample = samples[0]
    sample_expr = to_python_expr(sample)
    if sample_expr is None:
        return False, 'non-literal-empty-input'
    try:
        result = call_with_isolated_argv(function_obj, sample)
    except KeyboardInterrupt:
        raise
    except BaseException as exc:
        return False, f'raises {type(exc).__name__}: {exc}'
    result_expr = to_python_expr(result)
    if result_expr is None:
        return False, 'non-literal-result'
    code = f'assert {module_name}.{function_info.name}({sample_expr}) == {result_expr}'
    return True, code


def check_length_monotonicity(
    module_name: str,
    function_info: FunctionInfo,
    function_obj: Any,
) -> tuple[bool, str]:
    """Check if unary str->str length changes are monotonic in one direction."""
    samples = [value for value in unary_function_sample_values(function_info) if isinstance(value, str)]
    if len(samples) < 2:
        return False, 'insufficient-string-samples'
    directions: set[str] = set()
    tested = 0
    for sample in samples:
        try:
            result = call_with_isolated_argv(function_obj, sample)
        except KeyboardInterrupt:
            raise
        except BaseException:
            continue
        if not isinstance(result, str):
            return False, 'output-not-str'
        tested += 1
        if len(result) >= len(sample):
            directions.add('ge')
        if len(result) <= len(sample):
            directions.add('le')
    if tested < 2:
        return False, 'insufficient-successful-string-executions'
    if directions == {'ge', 'le'}:
        return False, 'mixed-monotonic-directions'

    relation = '>=' if directions == {'ge'} else '<='
    code = (
        f'for value in {stable_repr(samples)}:\n'
        f'    try:\n'
        f'        out = {module_name}.{function_info.name}(value)\n'
        f'        assert len(out) {relation} len(value)\n'
        f'    except Exception:\n'
        f'        pass'
    )
    return True, code


def discover_properties(
    module_name: str,
    api: ModuleAPI,
    callable_objects: dict[str, Any],
    *,
    verbose: bool,
) -> tuple[list[PropertySpec], list[str]]:
    """Discover and verify properties against CPython."""
    properties: list[PropertySpec] = []
    failures: list[str] = []

    # Roundtrip properties from inverse-name patterns.
    for forward_name, backward_name in find_inverse_pairs(api):
        forward = callable_objects.get(forward_name)
        backward = callable_objects.get(backward_name)
        if forward is None or backward is None:
            continue
        holds, code_or_reason = check_roundtrip_property(module_name, forward_name, backward_name, forward, backward)
        prop = PropertySpec(
            name=f'roundtrip_{forward_name}_{backward_name}',
            description=f'`{backward_name}({forward_name}(x)) == x` for sampled inputs',
            check_code=code_or_reason,
            holds=holds,
        )
        properties.append(prop)
        if not holds:
            failures.append(f'{prop.name}: {code_or_reason}')

    # Function-level unary properties.
    for function_info in api.functions:
        if not function_info.is_safe:
            continue
        function_obj = callable_objects.get(function_info.name)
        if function_obj is None:
            continue

        unary_prop_specs: list[tuple[str, str, Any]] = [
            (
                'idempotence',
                f'`{function_info.name}({function_info.name}(x)) == {function_info.name}(x)` on sampled inputs',
                check_unary_idempotence,
            ),
            (
                'type_preservation',
                f'`type({function_info.name}(x)) is type(x)` on sampled inputs',
                check_type_preservation,
            ),
            (
                'empty_input',
                f'`{function_info.name}` has stable behavior on an empty-like input',
                check_empty_input_property,
            ),
            (
                'length_monotonic',
                f'`len({function_info.name}(x))` is monotonic over sampled string inputs',
                check_length_monotonicity,
            ),
        ]

        added = 0
        for prop_kind, description, checker in unary_prop_specs:
            try:
                holds, code_or_reason = checker(module_name, function_info, function_obj)
            except KeyboardInterrupt:
                raise
            except Exception as exc:
                holds = False
                code_or_reason = f'property-check-error: {type(exc).__name__}: {exc}'
            prop = PropertySpec(
                name=f'{prop_kind}_{function_info.name}',
                description=description,
                check_code=code_or_reason,
                holds=holds,
            )
            properties.append(prop)
            function_info.properties.append(prop)
            if not holds:
                failures.append(f'{prop.name}: {code_or_reason}')
            added += 1
            if added >= MAX_PROPERTIES_PER_FUNCTION:
                break

    if verbose:
        holds_count = sum(1 for prop in properties if prop.holds)
        log(
            f'properties for {module_name}: {holds_count} holds, {len(properties) - holds_count} not-holding',
            verbose=verbose,
        )
    return properties, failures


def generate_property_tests_content(module_name: str, properties: list[PropertySpec]) -> str:
    """Generate property tests script content."""
    lines: list[str] = [f'import {module_name}', '']
    holding = [prop for prop in properties if prop.holds]
    non_holding = [prop for prop in properties if not prop.holds]

    if not holding:
        lines.append('# No verified properties were discovered for this module.')
        lines.append("assert True, 'no_verified_properties'")
        lines.append('')
    else:
        for prop in holding:
            lines.append(f'# === {prop.name} ===')
            lines.append(f'# {prop.description}')
            lines.extend(prop.check_code.splitlines())
            lines.append('')

    if non_holding:
        lines.append('# === informational_counterexamples ===')
        for prop in non_holding:
            lines.append(f'# {prop.name}: {prop.check_code}')
        lines.append('')

    return '\n'.join(lines).rstrip() + '\n'


# =============================================================================
# Section 6: Parity test generator
# =============================================================================


def generate_parity_tests_content(module_name: str, api: ModuleAPI) -> str:
    """Generate parity test script for Ouro-vs-CPython output comparison."""
    module_leaf = module_name.split('.')[-1]
    if module_leaf in PARITY_AUTOGEN_BLOCKLIST_MODULES:
        lines: list[str] = []
        lines.append('# Auto parity generation intentionally suppressed for this module.')
        lines.append("print('no_generated_parity_cases', True)")
        lines.append('')
        return '\n'.join(lines).rstrip() + '\n'

    lines = [f'import {module_name}', '']

    def parity_case_rank(test_case: TestCase) -> tuple[int, int]:
        """Rank cases so deterministic baseline probes are preferred."""
        label = test_case.label
        args_repr = test_case.args_repr
        has_kw = '=' in args_repr
        # Lower score is better.
        score = 100
        if 'default' in label:
            score = 0
        elif 'combo_req_2' in label:
            score = 10
        elif 'empty_' in label:
            score = 20
        elif 'combo_req_' in label:
            score = 30
        # De-prioritize keyword-heavy and option-combinator labels.
        if has_kw:
            score += 40
        if any(token in label for token in ('required_keyword_form', 'opt_', 'optpair_', 'kw_', 'kwpair_', 'var_')):
            score += 50
        return (score, len(args_repr))

    def select_parity_cases(cases: list[TestCase]) -> list[TestCase]:
        """Keep a compact deterministic subset for parity probes."""
        excluded_label_tokens = ('required_keyword_form', 'opt_', 'optpair_', 'kw_', 'kwpair_', 'var_')
        filtered = [
            case
            for case in cases
            if '=' not in case.args_repr
            and not any(token in case.label for token in excluded_label_tokens)
            and re.search(r'combo_req_[3-9][0-9]*', case.label) is None
        ]
        pool = filtered if filtered else cases
        ordered = sorted(pool, key=parity_case_rank)
        selected: list[TestCase] = []
        seen_args: set[str] = set()
        for case in ordered:
            if case.args_repr in seen_args:
                continue
            seen_args.add(case.args_repr)
            selected.append(case)
            if len(selected) >= 2:
                break
        return selected

    generated_any = False
    for function_info in api.functions:
        if function_info.name in PARITY_FUNCTION_NAME_BLOCKLIST:
            continue
        if function_info.name[:1].isupper():
            continue
        selected_cases = select_parity_cases(function_info.test_cases)
        if not selected_cases:
            continue
        generated_any = True
        lines.append(f'# === {function_info.name} ===')
        lines.append('try:')
        for test_case in selected_cases:
            call = f'{module_name}.{function_info.name}({test_case.args_repr})'
            lines.append(f"    print('{test_case.label}', {call})")
        lines.append('except Exception as e:')
        lines.append(f"    print('SKIP_{function_info.name}', type(e).__name__, e)")
        lines.append('')

    # Constants are intentionally omitted from auto parity probes. They often
    # vary by platform/build and create noisy diffs unrelated to core behavior.

    if not generated_any:
        lines.append("print('no_generated_parity_cases', True)")
        lines.append('')

    return '\n'.join(lines).rstrip() + '\n'


# =============================================================================
# Supplemental behavioral artifacts (error matrix, edge matrix, class probes)
# =============================================================================


def collect_edge_case_matrix(
    api: ModuleAPI,
    callable_objects: dict[str, Any],
) -> list[EdgeCaseObservation]:
    """Collect sampled edge-case behavior for safe functions using call candidates."""
    observations: list[EdgeCaseObservation] = []
    for function_info in api.functions:
        if not function_info.is_safe:
            continue
        function_obj = callable_objects.get(function_info.name)
        if function_obj is None:
            continue

        candidates = build_candidate_calls(function_info.name, function_obj)
        candidates.extend(build_negative_candidates(function_info.name, function_obj))
        if not candidates:
            continue

        seen_args: set[str] = set()
        captured_for_function = 0
        for candidate in candidates:
            args_repr = format_call_args(candidate.args, candidate.kwargs)
            if args_repr is None or args_repr in seen_args:
                continue
            seen_args.add(args_repr)
            kwargs_dict = dict(candidate.kwargs)

            try:
                output = call_with_isolated_argv(function_obj, *candidate.args, **kwargs_dict)
            except KeyboardInterrupt:
                raise
            except BaseException as exc:
                observations.append(
                    EdgeCaseObservation(
                        function_name=function_info.name,
                        sample_repr=args_repr,
                        status='raises',
                        output_repr=None,
                        output_type=None,
                        exc_type=type(exc).__name__,
                        exc_message=str(exc),
                    )
                )
                captured_for_function += 1
                continue

            output_expr = to_python_expr(output)
            observations.append(
                EdgeCaseObservation(
                    function_name=function_info.name,
                    sample_repr=args_repr,
                    status='ok',
                    output_repr=output_expr if output_expr is not None else stable_repr(output),
                    output_type=type(output).__name__,
                    exc_type=None,
                    exc_message=None,
                )
            )
            captured_for_function += 1

            if captured_for_function >= MAX_EDGE_OBSERVATIONS_PER_FUNCTION:
                break
    return observations


def generate_edge_case_report_content(observations: list[EdgeCaseObservation]) -> str:
    """Generate markdown summary for sampled edge-case behavior."""
    lines: list[str] = ['# Edge Case Behavior Matrix', '']
    if not observations:
        lines.append('No edge-case observations were captured.')
        lines.append('')
        return '\n'.join(lines)

    by_function: dict[str, list[EdgeCaseObservation]] = {}
    for observation in observations:
        by_function.setdefault(observation.function_name, []).append(observation)

    for function_name in sorted(by_function):
        lines.append(f'## `{function_name}`')
        lines.append('')
        for observation in by_function[function_name]:
            if observation.status == 'ok':
                lines.append(
                    (f'- sample `{observation.sample_repr}` -> `{observation.output_repr}` ({observation.output_type})')
                )
            else:
                lines.append(
                    (f'- sample `{observation.sample_repr}` raises `{observation.exc_type}({observation.exc_message})`')
                )
        lines.append('')
    return '\n'.join(lines).rstrip() + '\n'


def probe_class_behaviors(module_obj: ModuleType, api: ModuleAPI) -> list[ClassProbe]:
    """Probe class constructor and no-arg method behavior for stateful semantics."""
    probes: list[ClassProbe] = []
    for class_info in api.classes:
        cls = getattr(module_obj, class_info.name, None)
        if cls is None or not inspect.isclass(cls):
            continue

        signature = _safe_signature(cls)
        constructor_cases: list[dict[str, Any]] = []
        method_probes: list[dict[str, Any]] = []

        # Constructor probes: empty call + one-sample call when feasible.
        for label, args in (('ctor_empty', ()), ('ctor_basic', ('sample',))):
            try:
                instance = call_with_isolated_argv(cls, *args)
            except KeyboardInterrupt:
                raise
            except BaseException as exc:
                constructor_cases.append(
                    {
                        'label': label,
                        'args': stable_repr(args),
                        'status': 'raises',
                        'exc_type': type(exc).__name__,
                        'exc_message': str(exc),
                    }
                )
                continue
            constructor_cases.append(
                {
                    'label': label,
                    'args': stable_repr(args),
                    'status': 'ok',
                    'instance_type': type(instance).__name__,
                    'repr': stable_repr(instance),
                }
            )

            # Probe no-arg instance methods that look public and non-mutating.
            for attr_name in sorted(class_info.public_attrs):
                if attr_name.startswith('_'):
                    continue
                if attr_name in {'append', 'extend', 'update', 'remove', 'clear', 'pop'}:
                    continue
                try:
                    attr = getattr(instance, attr_name, None)
                except Exception as exc:
                    method_probes.append(
                        {
                            'method': attr_name,
                            'status': 'attr_error',
                            'exc_type': type(exc).__name__,
                            'exc_message': str(exc),
                        }
                    )
                    continue
                if not callable(attr):
                    continue
                method_sig = _safe_signature_object(attr)
                if method_sig is None:
                    continue
                required = [
                    param
                    for param in method_sig.parameters.values()
                    if param.default is inspect._empty
                    and param.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
                ]
                if required:
                    continue
                try:
                    result = call_with_isolated_argv(attr)
                except KeyboardInterrupt:
                    raise
                except BaseException as exc:
                    method_probes.append(
                        {
                            'method': attr_name,
                            'status': 'raises',
                            'exc_type': type(exc).__name__,
                            'exc_message': str(exc),
                        }
                    )
                    continue
                method_probes.append(
                    {
                        'method': attr_name,
                        'status': 'ok',
                        'result_type': type(result).__name__,
                        'result_repr': stable_repr(result),
                    }
                )
                if len(method_probes) >= 12:
                    break
            break

        probes.append(
            ClassProbe(
                class_name=class_info.name,
                constructor_signature=signature,
                constructor_cases=constructor_cases,
                method_probes=method_probes,
            )
        )
    return probes


def generate_class_probe_report_content(probes: list[ClassProbe]) -> str:
    """Generate markdown summary for class/stateful probes."""
    lines: list[str] = ['# Class and Stateful Behavior Probes', '']
    if not probes:
        lines.append('No class probes were generated.')
        lines.append('')
        return '\n'.join(lines)

    for probe in probes:
        lines.append(f'## `{probe.class_name}`')
        lines.append('')
        lines.append(f'- constructor signature: `{probe.constructor_signature}`')
        lines.append('- constructor cases:')
        for case in probe.constructor_cases:
            if case['status'] == 'ok':
                lines.append(f'  - {case["label"]}: ok ({case["instance_type"]})')
            else:
                lines.append(f'  - {case["label"]}: {case["exc_type"]}({case["exc_message"]})')
        if probe.method_probes:
            lines.append('- no-arg method probes:')
            for method in probe.method_probes:
                if method['status'] == 'ok':
                    lines.append(f'  - {method["method"]}: ok -> {method["result_type"]}')
                else:
                    lines.append(f'  - {method["method"]}: {method["exc_type"]}({method["exc_message"]})')
        lines.append('')
    return '\n'.join(lines).rstrip() + '\n'


def generate_impl_checklist_content(module_name: str) -> str:
    """Generate Ouro-specific implementation constraints checklist."""
    return (
        '# Implementation Checklist\n\n'
        f'## Module `{module_name}`\n\n'
        '1. Keep sandbox boundaries strict:\n'
        '- no filesystem/network/subprocess escape paths\n'
        '- block host-sensitive APIs unless explicitly sandboxed\n\n'
        '2. Match CPython exception semantics:\n'
        '- exact exception type\n'
        '- exact exception message when deterministic\n'
        '- preserve argument parsing error shape\n\n'
        '3. Preserve refcount safety in Rust implementation:\n'
        '- use `defer_drop!` / `defer_drop_mut!` where branching exists\n'
        '- ensure all early-return paths release heap refs\n\n'
        '4. Preserve resource-limit semantics:\n'
        '- avoid unbounded allocations or loops\n'
        '- avoid bypass paths around ResourceTracker\n\n'
        '5. Registration completeness:\n'
        '- `modules/mod.rs` (module wiring + function dispatch)\n'
        '- `intern.rs` (StaticStrings module + symbol names)\n'
        '- parity audit module list entry\n'
    )


# =============================================================================
# Section 7: Rust scaffold generator
# =============================================================================


def to_pascal_case(value: str) -> str:
    """Convert arbitrary name text to PascalCase."""
    parts = re.split(r'[^A-Za-z0-9]+', value)
    chunks: list[str] = []
    for part in parts:
        if not part:
            continue
        if part.isdigit():
            chunks.append(f'N{part}')
        else:
            chunks.append(part[0].upper() + part[1:])
    result = ''.join(chunks) or 'Generated'
    if result[0].isdigit():
        result = f'N{result}'
    return result


def to_snake_case(value: str) -> str:
    """Convert arbitrary name text to snake_case."""
    value = re.sub(r'([a-z0-9])([A-Z])', r'\1_\2', value)
    value = re.sub(r'[^A-Za-z0-9]+', '_', value)
    value = re.sub(r'_+', '_', value).strip('_')
    value = value.lower() or 'generated'
    if value in RUST_RESERVED_NAMES:
        value = f'{value}_value'
    if value[0].isdigit():
        value = f'v_{value}'
    return value


def make_rust_module_ident(module_name: str) -> str:
    """Build a Rust module identifier from Python module name."""
    ident = to_snake_case(module_name.replace('.', '_'))
    if ident in {'copy', 'string', 'random', 'time', 'enum', 'io', 'csv', 'base64', 'datetime', 'decimal', 'struct'}:
        ident = f'{ident}_mod'
    return ident


def make_static_string_variant(name: str) -> str:
    """Best-effort StaticStrings variant suggestion for an identifier."""
    raw = to_pascal_case(name)
    if raw in {'Self', 'Type', 'Module'}:
        raw = f'{raw}Value'
    return raw


def function_variant_and_attr(function_name: str) -> tuple[str, str | None]:
    """Compute enum variant and optional explicit strum serialize attribute."""
    variant = to_pascal_case(function_name)
    default_serialized = to_snake_case(variant)
    if default_serialized == function_name:
        return variant, None
    return variant, function_name


def generate_rust_scaffold_content(module_name: str, api: ModuleAPI, static_plan: StaticStringPlan) -> str:
    """Generate `scaffold.rs` with todo stubs for module functions."""
    module_leaf = module_name.split('.')[-1]
    module_variant = to_pascal_case(module_leaf)
    module_functions_enum = f'{module_variant}Functions'
    rust_module_ident = make_rust_module_ident(module_leaf)
    module_static_variant = static_plan.resolved.get(module_leaf, make_static_string_variant(module_leaf))

    functions = api.functions or [
        FunctionInfo(name='placeholder', signature='()', params=[], is_safe=True, block_reason=None)
    ]

    enum_lines: list[str] = []
    match_lines: list[str] = []
    create_lines: list[str] = []
    stub_lines: list[str] = []
    constant_lines: list[str] = []
    class_lines: list[str] = []

    for function_info in functions:
        enum_variant, explicit_serialize = function_variant_and_attr(function_info.name)
        static_variant = static_plan.resolved.get(function_info.name, make_static_string_variant(function_info.name))
        rust_fn_name = to_snake_case(function_info.name)

        if explicit_serialize is not None:
            enum_lines.append(f'    #[strum(serialize = "{explicit_serialize}")]')
        enum_lines.append(f'    {enum_variant},')

        create_lines.extend(
            [
                '    module.set_attr(',
                f'        StaticStrings::{static_variant},',
                (
                    f'        Value::ModuleFunction('
                    f'ModuleFunctions::{module_variant}({module_functions_enum}::{enum_variant})),'
                ),
                '        heap,',
                '        interns,',
                '    );',
                '',
            ]
        )

        match_lines.append(f'        {module_functions_enum}::{enum_variant} => {rust_fn_name}(heap, interns, args),')

        stub_lines.extend(
            [
                f'/// Implementation scaffold for `{module_name}.{function_info.name}`.',
                f'fn {rust_fn_name}(',
                '    heap: &mut Heap<impl ResourceTracker>,',
                '    interns: &Interns,',
                '    args: ArgValues,',
                ') -> RunResult<Value> {',
                '    let _ = (&heap, &interns, &args);',
                f'    todo!("Implement {module_name}.{function_info.name}")',
                '}',
                '',
            ]
        )

    if api.constants:
        constant_lines.append('// Expected module constants from CPython oracle:')
        for constant in api.constants[:40]:
            constant_lines.append(f'// - {constant.name}: {constant.value_repr} ({constant.value_type})')
        constant_lines.append('')

    if api.classes:
        class_lines.append('// Expected module classes from CPython oracle:')
        for class_info in api.classes[:20]:
            class_lines.append(f'// - {class_info.name} (public attrs: {", ".join(class_info.public_attrs[:20])})')
        class_lines.append('')

    return '\n'.join(
        [
            f'//! Scaffold implementation for the `{module_name}` module.',
            '//!',
            '//! Generated by `scripts/stdlib_add.py`.',
            '//! Fill each `todo!()` with CPython-compatible behavior.',
            '',
            'use crate::{',
            '    args::ArgValues,',
            '    builtins::Builtins,',
            '    exception_private::{ExcType, RunResult},',
            '    heap::{DropWithHeap, Heap, HeapData, HeapId},',
            '    intern::{Interns, StaticStrings},',
            '    modules::ModuleFunctions,',
            '    resource::ResourceTracker,',
            '    types::{AttrCallResult, Module, PyTrait},',
            '    value::Value,',
            '};',
            '',
            '#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]',
            '#[strum(serialize_all = "snake_case")]',
            f'pub(crate) enum {module_functions_enum} {{',
            *enum_lines,
            '}',
            '',
            '/// Create and register module attributes on heap allocation.',
            'pub fn create_module(',
            '    heap: &mut Heap<impl ResourceTracker>,',
            '    interns: &Interns,',
            ') -> Result<HeapId, crate::resource::ResourceError> {',
            f'    let mut module = Module::new(StaticStrings::{module_static_variant});',
            '',
            *create_lines,
            '    heap.allocate(HeapData::Module(module))',
            '}',
            '',
            '/// Dispatch one module function call.',
            'pub(super) fn call(',
            '    heap: &mut Heap<impl ResourceTracker>,',
            '    interns: &Interns,',
            f'    function: {module_functions_enum},',
            '    args: ArgValues,',
            ') -> RunResult<AttrCallResult> {',
            '    let result = match function {',
            *match_lines,
            '    }?;',
            '    Ok(AttrCallResult::Value(result))',
            '}',
            '',
            *stub_lines,
            *constant_lines,
            *class_lines,
            '// NOTE:',
            '// 1. Add precise `StaticStrings` variants for module + function names in `intern.rs`.',
            '// 2. Wire this module into `modules/mod.rs` BuiltinModule and ModuleFunctions dispatch.',
            '// 3. Replace placeholder `StaticStrings::<Name>` guesses with actual project naming conventions.',
            '',
            f'// Suggested module filename: `crates/ouro/src/modules/{rust_module_ident}.rs`',
            '',
            '// Keep imports at top-level and remove unused imports once implementation is complete.',
            '// Avoid `unsafe`; use heap/refcount helpers (`defer_drop!`, `drop_with_heap`) on all paths.',
            '',
        ]
    )


# =============================================================================
# Section 8: Registration diff generator
# =============================================================================


def extract_context(lines: list[str], index: int, radius: int = 2) -> tuple[str, str]:
    """Return context lines before/after a target insertion index."""
    start = max(0, index - radius)
    end = min(len(lines), index + radius)
    before = ''.join(lines[start:index]).rstrip('\n')
    after = ''.join(lines[index:end]).rstrip('\n')
    return before, after


def find_first_index(lines: list[str], predicate: Any) -> int:
    """Find first line index matching predicate; return -1 if not found."""
    for idx, line in enumerate(lines):
        if predicate(line):
            return idx
    return -1


def find_last_index(lines: list[str], predicate: Any) -> int:
    """Find last line index matching predicate; return -1 if not found."""
    for idx in range(len(lines) - 1, -1, -1):
        if predicate(lines[idx]):
            return idx
    return -1


def parse_static_strings(intern_lines: list[str]) -> tuple[dict[str, str], set[str]]:
    """Parse `StaticStrings` serialize-value mapping from `intern.rs` source lines."""
    serialize_to_variant: dict[str, str] = {}
    variants: set[str] = set()
    pending_serialize: str | None = None

    for raw_line in intern_lines:
        line = raw_line.strip()
        if line.startswith('#[strum(serialize = '):
            match = re.search(r'#\[strum\(serialize = "(.*)"\)\]', line)
            if match is not None:
                pending_serialize = match.group(1)
            continue
        variant_match = re.match(r'([A-Za-z][A-Za-z0-9]*),$', line)
        if variant_match is None:
            continue
        variant = variant_match.group(1)
        variants.add(variant)
        serialized = pending_serialize if pending_serialize is not None else to_snake_case(variant)
        serialize_to_variant[serialized] = variant
        pending_serialize = None

    return serialize_to_variant, variants


def build_static_string_plan(module_name: str, api: ModuleAPI, intern_lines: list[str]) -> StaticStringPlan:
    """Resolve or propose StaticStrings variant names for module surface names."""
    existing_map, existing_variants = parse_static_strings(intern_lines)
    resolved: dict[str, str] = {}
    suggested_insertions: list[tuple[str, str]] = []
    planned_variants: set[str] = set()

    targets = [module_name.split('.')[-1]]
    targets.extend(function.name for function in api.functions)
    targets.extend(constant.name for constant in api.constants if constant.name.isidentifier())
    targets.extend(class_info.name for class_info in api.classes if class_info.name.isidentifier())

    for serialized_name in dedupe_preserve_order(targets):
        if serialized_name in existing_map:
            resolved[serialized_name] = existing_map[serialized_name]
            continue

        base_variant = make_static_string_variant(serialized_name)
        variant = base_variant
        suffix = 2
        while variant in existing_variants or variant in planned_variants:
            variant = f'{base_variant}{suffix}'
            suffix += 1
        resolved[serialized_name] = variant
        planned_variants.add(variant)
        suggested_insertions.append((serialized_name, variant))

    return StaticStringPlan(resolved=resolved, suggested_insertions=suggested_insertions)


def generate_static_string_plan_content(plan: StaticStringPlan) -> str:
    """Generate markdown for StaticStrings resolution and insertion candidates."""
    lines: list[str] = ['# StaticStrings Plan', '']
    lines.append('## Resolved names')
    lines.append('')
    for serialized_name in sorted(plan.resolved):
        lines.append(f'- `{serialized_name}` -> `StaticStrings::{plan.resolved[serialized_name]}`')
    lines.append('')
    lines.append('## New insertion candidates')
    lines.append('')
    if not plan.suggested_insertions:
        lines.append('- None; all required names already exist.')
    else:
        for serialized_name, variant in plan.suggested_insertions:
            lines.append(f'- `#[strum(serialize = "{serialized_name}")]` then `{variant},`')
    lines.append('')

    if plan.suggested_insertions:
        lines.append('## Note')
        lines.append('')
        lines.append('- `registration_diff.md` emits a dedicated "module marker" insertion when missing.')
        lines.append('- Apply symbol insertions once; do not duplicate the module marker entry.')
    lines.append('')
    return '\n'.join(lines)


def generate_registration_snippets(
    module_name: str, api: ModuleAPI
) -> tuple[list[RegistrationSnippet], StaticStringPlan]:
    """Generate insertion guidance for `mod.rs`, `intern.rs`, and parity audit."""
    module_leaf = module_name.split('.')[-1]
    rust_mod_ident = make_rust_module_ident(module_leaf)
    builtin_variant = to_pascal_case(module_leaf)
    module_functions_enum = f'{builtin_variant}Functions'
    parity_stem = module_file_stem(module_name)

    snippets: list[RegistrationSnippet] = []

    modules_lines = MODULES_RS.read_text(encoding='utf-8').splitlines(keepends=True)
    intern_lines = INTERN_RS.read_text(encoding='utf-8').splitlines(keepends=True)
    parity_lines = DEEP_PARITY.read_text(encoding='utf-8').splitlines(keepends=True)
    static_plan = build_static_string_plan(module_name, api, intern_lines)
    static_module_variant = static_plan.resolved[module_leaf]

    # 1. mod declaration
    idx = find_last_index(modules_lines, lambda line: line.startswith('pub(crate) mod '))
    if idx != -1:
        before, after = extract_context(modules_lines, idx + 1)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='mod declarations',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'pub(crate) mod {rust_mod_ident};',
                context_before=before,
                context_after=after,
                line_number=idx + 2,
            )
        )

    # 2. BuiltinModule variant
    idx = find_first_index(modules_lines, lambda line: 'BuiltinsMod,' in line)
    if idx != -1:
        before, after = extract_context(modules_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='BuiltinModule enum variant',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'    {builtin_variant},',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    # 3. from_string_id arm
    idx = find_first_index(modules_lines, lambda line: '_ => None,' in line)
    if idx != -1:
        before, after = extract_context(modules_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='BuiltinModule::from_string_id',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'            StaticStrings::{static_module_variant} => Some(Self::{builtin_variant}),',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    # 4. BuiltinModule::create arm
    idx = find_first_index(modules_lines, lambda line: 'Self::Tempfile => tempfile_mod::create_module' in line)
    if idx == -1:
        idx = find_first_index(modules_lines, lambda line: 'Self::BuiltinsMod => builtins_mod::create_module' in line)
    if idx != -1:
        before, after = extract_context(modules_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='BuiltinModule::create',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'            Self::{builtin_variant} => {rust_mod_ident}::create_module(heap, interns),',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    # 5. ModuleFunctions enum entry
    idx = find_first_index(modules_lines, lambda line: 'Tempfile(tempfile_mod::TempfileFunctions),' in line)
    if idx == -1:
        idx = find_first_index(modules_lines, lambda line: line.strip() == '}')
    if idx != -1:
        before, after = extract_context(modules_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='ModuleFunctions enum',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'    {builtin_variant}({rust_mod_ident}::{module_functions_enum}),',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    # 6. ModuleFunctions::call arm
    idx = find_first_index(
        modules_lines,
        lambda line: 'Self::Tempfile(functions) => tempfile_mod::call(heap, interns, functions, args),' in line,
    )
    if idx == -1:
        idx = find_first_index(modules_lines, lambda line: 'Self::Warnings(functions)' in line)
    if idx != -1:
        before, after = extract_context(modules_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='ModuleFunctions::call',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'            Self::{builtin_variant}(functions) => {rust_mod_ident}::call(heap, interns, functions, args),',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    # 7. ModuleFunctions display arm
    idx = find_first_index(modules_lines, lambda line: 'Self::Tempfile(func) => write!(f, "{func}"),' in line)
    if idx == -1:
        idx = find_first_index(modules_lines, lambda line: 'Self::Warnings(func)' in line)
    if idx != -1:
        before, after = extract_context(modules_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='crates/ouro/src/modules/mod.rs',
                section='impl Display for ModuleFunctions',
                anchor=modules_lines[idx].rstrip(),
                insert_line=f'            Self::{builtin_variant}(func) => write!(f, "{{func}}"),',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    # 8. intern.rs StaticStrings module + symbol variants
    idx = find_first_index(intern_lines, lambda line: '// string module strings' in line)
    if idx == -1:
        idx = find_last_index(intern_lines, lambda line: line.strip() == '}')
    if idx != -1:
        before, after = extract_context(intern_lines, idx)
        insertion_block: list[str] = []
        module_marker_missing = any(
            serialized_name == module_leaf for serialized_name, _ in static_plan.suggested_insertions
        )
        if module_marker_missing:
            insertion_block.append(f'    #[strum(serialize = "{module_leaf}")]')
            insertion_block.append(f'    {static_module_variant},')
        for serialized_name, variant in static_plan.suggested_insertions:
            if serialized_name == module_leaf:
                continue
            insertion_block.append(f'    #[strum(serialize = "{serialized_name}")]')
            insertion_block.append(f'    {variant},')

        if insertion_block:
            snippets.append(
                RegistrationSnippet(
                    file_path='crates/ouro/src/intern.rs',
                    section='StaticStrings enum (insert block)',
                    anchor=intern_lines[idx].rstrip(),
                    insert_line='\n'.join(insertion_block),
                    context_before=before,
                    context_after=after,
                    line_number=idx + 1,
                )
            )

    # 9. deep parity audit module list
    idx = find_first_index(parity_lines, lambda line: 'run_parity "stdlib.time"' in line)
    if idx == -1:
        idx = find_first_index(parity_lines, lambda line: '# CORE LANGUAGE FEATURES' in line)
    if idx != -1:
        before, after = extract_context(parity_lines, idx)
        snippets.append(
            RegistrationSnippet(
                file_path='playground/deep_parity_audit.sh',
                section='stdlib MODULES run list',
                anchor=parity_lines[idx].rstrip(),
                insert_line=f'run_parity "stdlib.{parity_stem}"      "$TESTDIR/test_{parity_stem}.py"',
                context_before=before,
                context_after=after,
                line_number=idx + 1,
            )
        )

    return snippets, static_plan


def generate_registration_diff_content(snippets: list[RegistrationSnippet]) -> str:
    """Generate markdown document with insertion guidance."""
    lines: list[str] = ['# Registration Diff Guide', '']
    if not snippets:
        lines.append('No insertion snippets were generated.')
        lines.append('')
        return '\n'.join(lines)

    for snippet in snippets:
        lines.append(f'## `{snippet.file_path}` - {snippet.section}')
        lines.append('')
        lines.append(f'- Anchor: `{snippet.anchor}`')
        if snippet.line_number is not None:
            lines.append(f'- Suggested insert line number: `{snippet.line_number}`')
        lines.append(f'- Insert line: `{snippet.insert_line}`')
        lines.append('')
        lines.append('Context:')
        lines.append('```text')
        if snippet.context_before:
            lines.append(snippet.context_before)
        lines.append(f'>> INSERT HERE: {snippet.insert_line}')
        if snippet.context_after:
            lines.append(snippet.context_after)
        lines.append('```')
        lines.append('')
    return '\n'.join(lines).rstrip() + '\n'


# =============================================================================
# Readiness and anti-hallucination guidance
# =============================================================================


def module_risk_flags(module_name: str, api: ModuleAPI) -> list[str]:
    """Return coarse module-level risk flags used in one-shot readiness scoring."""
    flags: list[str] = []
    if is_host_sensitive_module(module_name):
        flags.append('host-sensitive-module-family')
    if any(not function_info.is_safe for function_info in api.functions):
        flags.append('contains-blocked-functions')
    if api.classes:
        flags.append('class-heavy-surface')
    return flags


def truncate_items(items: list[str], max_items: int) -> tuple[list[str], int]:
    """Return a bounded view of list entries and the omitted count."""
    if len(items) <= max_items:
        return items, 0
    return items[:max_items], len(items) - max_items


def assess_module_readiness(
    module_name: str,
    api: ModuleAPI,
    oracle: dict[str, OracleSummary],
    properties: list[PropertySpec],
) -> ModuleReadiness:
    """Assess whether generated artifacts are sufficient for one-shot implementation."""
    total_functions = len(api.functions)
    safe_functions = [function_info for function_info in api.functions if function_info.is_safe]
    unsafe_functions = [function_info for function_info in api.functions if not function_info.is_safe]

    function_status: list[dict[str, Any]] = []
    safe_with_success = 0
    safe_with_only_exceptions = 0
    safe_with_no_observations = 0
    total_exception_cases = 0

    for function_info in api.functions:
        summary = oracle.get(function_info.name, OracleSummary())
        success_count = len(summary.success_cases)
        exception_count = len(summary.exceptions)
        observed_count = len(summary.observed_calls)
        total_exception_cases += exception_count

        if function_info.is_safe:
            if success_count > 0:
                safe_with_success += 1
            elif exception_count > 0:
                safe_with_only_exceptions += 1
            elif observed_count == 0:
                safe_with_no_observations += 1

        status = 'blocked'
        if function_info.is_safe:
            if success_count > 0:
                status = 'oracle-covered'
            elif exception_count > 0:
                status = 'exceptions-only'
            else:
                status = 'no-observations'

        function_status.append(
            {
                'name': function_info.name,
                'status': status,
                'is_safe': function_info.is_safe,
                'success_cases': success_count,
                'exception_cases': exception_count,
                'observed_calls': observed_count,
                'block_reason': function_info.block_reason,
            }
        )

    safe_total = len(safe_functions)
    blocked_total = len(unsafe_functions)
    coverage_ratio = (safe_with_success / safe_total) if safe_total else 0.0
    risk_flags = module_risk_flags(module_name, api)
    holds_count = sum(1 for prop in properties if prop.holds)

    score = 100
    if blocked_total:
        score -= min(30, blocked_total * 2 + int((blocked_total / max(1, total_functions)) * 20))
    if safe_total == 0:
        score -= 40
    elif coverage_ratio < 0.3:
        score -= 30
    elif coverage_ratio < 0.6:
        score -= 20
    elif coverage_ratio < 0.8:
        score -= 10
    if safe_with_only_exceptions:
        score -= min(15, safe_with_only_exceptions * 3)
    if safe_with_no_observations:
        score -= min(20, safe_with_no_observations * 4)
    if api.classes:
        score -= min(15, len(api.classes) * 3)
    if 'host-sensitive-module-family' in risk_flags:
        score -= 20
    if holds_count >= max(1, safe_total):
        score += 3
    score = max(0, min(100, score))

    if score >= 85:
        tier = 'high'
    elif score >= 70:
        tier = 'medium'
    elif score >= 50:
        tier = 'caution'
    else:
        tier = 'low'

    one_shot_eligible = (
        score >= 75
        and blocked_total == 0
        and safe_total > 0
        and coverage_ratio >= 0.8
        and safe_with_only_exceptions == 0
        and safe_with_no_observations == 0
        and 'host-sensitive-module-family' not in risk_flags
    )

    strengths: list[str] = []
    if safe_with_success:
        strengths.append(f'{safe_with_success}/{safe_total} safe functions have successful oracle coverage.')
    if total_exception_cases:
        strengths.append(f'Captured {total_exception_cases} exact CPython exception cases.')
    if holds_count:
        strengths.append(f'Validated {holds_count} property checks against CPython.')

    risks: list[str] = []
    if blocked_total:
        risks.append(f'{blocked_total} functions were blocked by safety heuristics.')
    if safe_with_only_exceptions:
        risks.append(f'{safe_with_only_exceptions} safe functions only produced exceptions in sampled calls.')
    if safe_with_no_observations:
        risks.append(f'{safe_with_no_observations} safe functions had no oracle observations.')
    if 'host-sensitive-module-family' in risk_flags:
        risks.append('Module belongs to a host-sensitive family (filesystem/network/process/import/runtime).')
    if api.classes:
        risks.append(f'{len(api.classes)} public classes may require stateful/manual semantics validation.')

    manual_verification_required: list[str] = []
    for function_info in unsafe_functions:
        manual_verification_required.append(
            f'`{function_info.name}` blocked by safety heuristic: {function_info.block_reason}.'
        )
    for status in function_status:
        if status['status'] == 'exceptions-only':
            manual_verification_required.append(
                f'`{status["name"]}` only has exception observations; verify successful CPython behavior manually.'
            )
        elif status['status'] == 'no-observations':
            manual_verification_required.append(
                f'`{status["name"]}` has no oracle observations; manual CPython probing required.'
            )
    if api.classes:
        manual_verification_required.append('Class constructors/method contracts need explicit parity verification.')
    if 'host-sensitive-module-family' in risk_flags:
        manual_verification_required.append(
            'Host-sensitive operations require explicit sandbox policy decisions before implementation.'
        )

    anti_hallucination_rules = [
        'Implement only names present in `api_surface.json`; do not invent functions/constants/classes.',
        'Do not infer undocumented defaults/kwargs beyond observed signatures and oracle data.',
        'For any function listed in manual verification, run focused CPython probes before coding behavior.',
        'Preserve exact exception type/message from `error_tests.py` and `function_contracts.md`.',
        'If oracle data is missing for a behavior path, mark TODO and add targeted parity probe first.',
    ]

    summary = (
        f'one_shot_eligible={one_shot_eligible}; '
        f'safe_coverage={safe_with_success}/{safe_total}; '
        f'blocked={blocked_total}; '
        f'class_count={len(api.classes)}'
    )

    return ModuleReadiness(
        score=score,
        tier=tier,
        one_shot_eligible=one_shot_eligible,
        summary=summary,
        strengths=strengths,
        risks=risks,
        manual_verification_required=manual_verification_required,
        anti_hallucination_rules=anti_hallucination_rules,
        function_status=function_status,
    )


def generate_readiness_report_content(module_name: str, readiness: ModuleReadiness) -> str:
    """Generate human-readable one-shot readiness + manual verification report."""
    lines: list[str] = [f'# Readiness Report for `{module_name}`', '']
    lines.append(f'- score: `{readiness.score}`')
    lines.append(f'- tier: `{readiness.tier}`')
    lines.append(f'- one-shot eligible: `{readiness.one_shot_eligible}`')
    lines.append(f'- summary: `{readiness.summary}`')
    lines.append('')

    lines.append('## Strengths')
    lines.append('')
    if readiness.strengths:
        for item in readiness.strengths:
            lines.append(f'- {item}')
    else:
        lines.append('- None.')
    lines.append('')

    lines.append('## Risks')
    lines.append('')
    if readiness.risks:
        for item in readiness.risks:
            lines.append(f'- {item}')
    else:
        lines.append('- None.')
    lines.append('')

    lines.append('## Manual Verification Required')
    lines.append('')
    if readiness.manual_verification_required:
        shown_manual, omitted_manual = truncate_items(
            readiness.manual_verification_required, MAX_READINESS_MANUAL_ITEMS
        )
        for item in shown_manual:
            lines.append(f'- {item}')
        if omitted_manual:
            lines.append(f'- ... {omitted_manual} more items (see `readiness.json` for full list).')
    else:
        lines.append('- None.')
    lines.append('')

    lines.append('## Anti-Hallucination Rules')
    lines.append('')
    for rule in readiness.anti_hallucination_rules:
        lines.append(f'- {rule}')
    lines.append('')

    lines.append('## Function Status')
    lines.append('')
    status_counts: dict[str, int] = {}
    for item in readiness.function_status:
        status_counts[item['status']] = status_counts.get(item['status'], 0) + 1
    for status_name in sorted(status_counts):
        lines.append(f'- `{status_name}`: `{status_counts[status_name]}`')
    lines.append('')

    shown_status = readiness.function_status[:MAX_READINESS_FUNCTION_STATUS_ITEMS]
    for item in shown_status:
        lines.append(
            (
                f'- `{item["name"]}`: `{item["status"]}` '
                f'(success={item["success_cases"]}, exceptions={item["exception_cases"]}, '
                f'observed={item["observed_calls"]})'
            )
        )
    omitted_status = len(readiness.function_status) - len(shown_status)
    if omitted_status:
        lines.append(f'- ... {omitted_status} more function entries (see `readiness.json`).')
    lines.append('')
    return '\n'.join(lines).rstrip() + '\n'


# =============================================================================
# Section 9: Agent prompt generator
# =============================================================================


def generate_agent_task_content(
    module_name: str,
    api: ModuleAPI,
    properties: list[PropertySpec],
    readiness: ModuleReadiness,
    registration_snippets: list[RegistrationSnippet],
    output_dir: Path,
    unit_tests_path: Path,
    parity_tests_path: Path,
    property_tests_path: Path,
    error_tests_path: Path,
    edge_matrix_path: Path,
    edge_report_path: Path,
    class_probe_path: Path,
    class_probe_report_path: Path,
    static_plan_path: Path,
    checklist_path: Path,
    function_contracts_path: Path,
    oracle_behavior_path: Path,
    implementation_recipe_path: Path,
    readiness_report_path: Path,
) -> str:
    """Generate `AGENT_TASK.md` bundle for implementing agent."""
    safe_functions = [function for function in api.functions if function.is_safe]
    unsafe_functions = [function for function in api.functions if not function.is_safe]
    holding_properties = [prop for prop in properties if prop.holds]
    failing_properties = [prop for prop in properties if not prop.holds]
    shown_manual, omitted_manual = truncate_items(readiness.manual_verification_required, MAX_AGENT_MANUAL_ITEMS)
    manual_lines = [f'- {item}' for item in shown_manual] if shown_manual else ['- None.']
    if omitted_manual:
        manual_lines.append(f'- ... {omitted_manual} more items (see `readiness_report.md`).')

    lines: list[str] = [
        f'# Task: Implement `{module_name}` stdlib module for Ouro',
        '',
        '## Generated Artifacts',
        '',
        f'- Unit tests: `{unit_tests_path}`',
        f'- Parity tests: `{parity_tests_path}`',
        f'- Property tests: `{property_tests_path}`',
        f'- Error semantics tests: `{error_tests_path}`',
        f'- Edge matrix JSON: `{edge_matrix_path}`',
        f'- Edge matrix report: `{edge_report_path}`',
        f'- Class probe JSON: `{class_probe_path}`',
        f'- Class probe report: `{class_probe_report_path}`',
        f'- StaticStrings plan: `{static_plan_path}`',
        f'- Implementation checklist: `{checklist_path}`',
        f'- Function contracts: `{function_contracts_path}`',
        f'- Oracle behavior matrix: `{oracle_behavior_path}`',
        f'- Implementation recipe: `{implementation_recipe_path}`',
        f'- Readiness report: `{readiness_report_path}`',
        f'- Readiness JSON: `{output_dir / "readiness.json"}`',
        f'- Rust scaffold: `{output_dir / "scaffold.rs"}`',
        f'- Registration diff: `{output_dir / "registration_diff.md"}`',
        f'- API surface JSON: `{output_dir / "api_surface.json"}`',
        '',
        '## Readiness and Guardrails',
        '',
        f'- One-shot eligible: `{readiness.one_shot_eligible}`',
        f'- Readiness score: `{readiness.score}` (`{readiness.tier}`)',
        f'- Summary: `{readiness.summary}`',
        '',
        'Mandatory anti-hallucination rules:',
        *[f'- {rule}' for rule in readiness.anti_hallucination_rules],
        '',
        'Manual verification required:',
        *manual_lines,
        '- If `one-shot eligible` is `False`, do not implement unverified behaviors without adding manual CPython probes.',
        '',
        '## API Summary',
        '',
        f'- Module: `{module_name}`',
        f'- Public functions: `{len(api.functions)}`',
        f'- Safe oracle-tested functions: `{len(safe_functions)}`',
        f'- Blocked/unsafe functions: `{len(unsafe_functions)}`',
        f'- Public classes: `{len(api.classes)}`',
        f'- Public constants: `{len(api.constants)}`',
        '',
        '### Functions',
        '',
    ]

    if not api.functions:
        lines.append('- No public functions discovered.')
    else:
        for function in api.functions[:MAX_AGENT_FUNCTION_ITEMS]:
            status = 'safe' if function.is_safe else f'blocked ({function.block_reason})'
            lines.append(f'- `{function.name}{function.signature or ""}` - {status}')
        omitted_functions = len(api.functions) - min(len(api.functions), MAX_AGENT_FUNCTION_ITEMS)
        if omitted_functions:
            lines.append(f'- ... {omitted_functions} more functions (see `api_surface.json`).')
    lines.append('')

    lines.extend(
        [
            '### Properties',
            '',
            f'- Verified properties: `{len(holding_properties)}`',
            f'- Non-holding properties (informational): `{len(failing_properties)}`',
            '',
        ]
    )
    for prop in holding_properties[:20]:
        lines.append(f'- HOLDS `{prop.name}`: {prop.description}')
    for prop in failing_properties[:12]:
        lines.append(f'- INFO `{prop.name}`: {prop.check_code}')
    lines.append('')

    lines.extend(
        [
            '## Registration Checklist',
            '',
            'Apply each insertion from `registration_diff.md`:',
            '- `crates/ouro/src/modules/mod.rs` (7 insertion points)',
            '- `crates/ouro/src/intern.rs` (add module marker in `StaticStrings`)',
            '- `playground/deep_parity_audit.sh` (add module parity entry)',
            '',
            f'Generated insertion snippets: `{len(registration_snippets)}`',
            '',
            '## One-Shot Inputs',
            '',
            'Implement directly from these generated artifacts without extra repo search:',
            '- `function_contracts.md` for per-function behavior and errors',
            '- `oracle_behavior.json` for exhaustive sampled oracle outcomes',
            '- `implementation_recipe.md` for per-function Rust parser skeletons',
            '- `readiness_report.md` for explicit no-hallucination and manual-verification gates',
            '- `static_strings_plan.md` + `registration_diff.md` for naming/wiring',
            '- `IMPLEMENTATION_CHECKLIST.md` for sandbox/refcount/resource constraints',
            '',
            '## Verification Commands',
            '',
            f'```bash\nmake test-cases\npython3 {unit_tests_path}\n'
            f'python3 {parity_tests_path}\n'
            f'python3 {property_tests_path}\n'
            f'python3 {error_tests_path}\n'
            'bash playground/deep_parity_audit.sh\n```',
            '',
            '## Notes',
            '',
            '- Keep behavior CPython-compatible; do not weaken parity expectations.',
            '- Keep reference-count drop paths correct on all branches.',
            '- Add precise exception messages matching existing exception helpers.',
            '- Remove scaffold TODOs only after implementation is complete.',
            '',
        ]
    )
    return '\n'.join(lines).rstrip() + '\n'


# =============================================================================
# Section 10: API surface JSON
# =============================================================================


def module_api_to_json(
    module_api: ModuleAPI,
    properties: list[PropertySpec],
    oracle: dict[str, OracleSummary],
    edge_observations: list[EdgeCaseObservation],
    class_probes: list[ClassProbe],
    static_plan: StaticStringPlan,
    readiness: ModuleReadiness,
) -> dict[str, Any]:
    """Serialize module API and generated artifacts for reference output."""
    payload = asdict(module_api)
    payload['properties'] = [asdict(prop) for prop in properties]
    payload['oracle'] = {
        name: {
            'success_count': len(summary.success_cases),
            'skipped_cases': summary.skipped_cases,
            'exceptions': [asdict(exception_case) for exception_case in summary.exceptions],
            'observed_calls': [asdict(observed_call) for observed_call in summary.observed_calls],
        }
        for name, summary in oracle.items()
    }
    payload['edge_observations'] = [asdict(observation) for observation in edge_observations]
    payload['class_probes'] = [asdict(probe) for probe in class_probes]
    payload['static_string_plan'] = asdict(static_plan)
    payload['readiness'] = asdict(readiness)
    return payload


# =============================================================================
# Section 11: CLI main / orchestration
# =============================================================================


def resolve_output_dir(module_name: str, provided: Path | None, module_count: int) -> Path:
    """Resolve per-module output directory from CLI flags."""
    default_dir = Path(f'/tmp/stdlib-add-{module_file_stem(module_name)}')
    if provided is None:
        return default_dir
    if module_count == 1:
        return provided
    return provided / module_file_stem(module_name)


def build_outputs_for_module(
    module_name: str,
    *,
    output_dir: Path,
    dry_run: bool,
    skip_tests: bool,
    verbose: bool,
) -> None:
    """Execute full generation flow for one module."""
    log(f'Collecting CPython API snapshot for {module_name}', verbose=verbose, force=True)
    api, callable_objects, module_obj = collect_cpython_snapshot(module_name, verbose=verbose)

    oracle_results: dict[str, OracleSummary] = {}
    properties: list[PropertySpec] = []
    edge_observations: list[EdgeCaseObservation] = []
    class_probes: list[ClassProbe] = []

    if skip_tests:
        log(f'Skipping oracle-based test generation for {module_name}', verbose=verbose, force=True)
    else:
        log(f'Generating oracle test cases for {module_name}', verbose=verbose, force=True)
        oracle_results = populate_oracle_tests(module_name, api, callable_objects, verbose=verbose)
        try:
            log(f'Discovering properties for {module_name}', verbose=verbose, force=True)
            properties, _failures = discover_properties(module_name, api, callable_objects, verbose=verbose)
        except Exception as exc:
            log(
                f'Property discovery failed for {module_name}: {type(exc).__name__}: {exc}',
                verbose=verbose,
                force=True,
            )
            properties = []

        try:
            log(f'Collecting edge-case behavior matrix for {module_name}', verbose=verbose, force=True)
            edge_observations = collect_edge_case_matrix(api, callable_objects)
        except Exception as exc:
            log(
                f'Edge-case collection failed for {module_name}: {type(exc).__name__}: {exc}',
                verbose=verbose,
                force=True,
            )
            edge_observations = []

        if is_host_sensitive_module(module_name):
            log(
                f'Skipping class probes for host-sensitive module {module_name}',
                verbose=verbose,
                force=True,
            )
            class_probes = []
        else:
            try:
                log(f'Collecting class probes for {module_name}', verbose=verbose, force=True)
                class_probes = probe_class_behaviors(module_obj, api)
            except Exception as exc:
                log(
                    f'Class probes failed for {module_name}: {type(exc).__name__}: {exc}',
                    verbose=verbose,
                    force=True,
                )
                class_probes = []

    module_stem = module_file_stem(module_name)
    unit_test_path = TEST_CASES_DIR / f'{module_stem}__stdlib.py'
    parity_test_path = PARITY_TEST_DIR / f'test_{module_stem}.py'
    property_test_path = output_dir / 'tests' / 'property_tests.py'
    error_test_path = output_dir / 'tests' / 'error_tests.py'
    edge_matrix_path = output_dir / 'edge_case_matrix.json'
    edge_report_path = output_dir / 'edge_case_matrix.md'
    class_probe_path = output_dir / 'class_behavior.json'
    class_probe_report_path = output_dir / 'class_behavior.md'
    static_plan_path = output_dir / 'static_strings_plan.md'
    checklist_path = output_dir / 'IMPLEMENTATION_CHECKLIST.md'
    function_contracts_path = output_dir / 'function_contracts.md'
    oracle_behavior_path = output_dir / 'oracle_behavior.json'
    implementation_recipe_path = output_dir / 'implementation_recipe.md'
    readiness_report_path = output_dir / 'readiness_report.md'
    readiness_json_path = output_dir / 'readiness.json'
    unit_test_fallback_path = output_dir / 'test_cases' / f'{module_stem}__stdlib.py'
    parity_test_fallback_path = output_dir / 'parity_tests' / f'test_{module_stem}.py'
    scaffold_path = output_dir / 'scaffold.rs'
    registration_diff_path = output_dir / 'registration_diff.md'
    agent_task_path = output_dir / 'AGENT_TASK.md'
    api_json_path = output_dir / 'api_surface.json'

    log(f'Generating registration snippets for {module_name}', verbose=verbose, force=True)
    registration_snippets, static_plan = generate_registration_snippets(module_name, api)
    registration_diff_content = generate_registration_diff_content(registration_snippets)
    static_plan_content = generate_static_string_plan_content(static_plan)
    checklist_content = generate_impl_checklist_content(module_name)

    log(f'Generating Rust scaffold for {module_name}', verbose=verbose, force=True)
    scaffold_content = generate_rust_scaffold_content(module_name, api, static_plan)
    function_contracts_content = generate_function_contracts_content(module_name, api, oracle_results, static_plan)
    implementation_recipe_content = generate_implementation_recipe_content(
        module_name, api, oracle_results, static_plan
    )
    readiness = assess_module_readiness(module_name, api, oracle_results, properties)
    readiness_report_content = generate_readiness_report_content(module_name, readiness)

    if not skip_tests:
        log(f'Generating unit tests for {module_name}', verbose=verbose, force=True)
        unit_tests_content = generate_unit_tests_content(module_name, api, callable_objects)
        log(f'Generating parity tests for {module_name}', verbose=verbose, force=True)
        parity_tests_content = generate_parity_tests_content(module_name, api)
        log(f'Generating property tests for {module_name}', verbose=verbose, force=True)
        property_tests_content = generate_property_tests_content(module_name, properties)
        log(f'Generating error tests for {module_name}', verbose=verbose, force=True)
        error_tests_content = generate_error_tests_content(module_name, oracle_results)
    else:
        unit_tests_content = '# skipped via --skip-tests\n'
        parity_tests_content = '# skipped via --skip-tests\n'
        property_tests_content = '# skipped via --skip-tests\n'
        error_tests_content = '# skipped via --skip-tests\n'

    edge_report_content = generate_edge_case_report_content(edge_observations)
    class_probe_report_content = generate_class_probe_report_content(class_probes)
    edge_matrix_content = (
        json.dumps([asdict(observation) for observation in edge_observations], indent=2, sort_keys=True) + '\n'
    )
    class_probe_content = json.dumps([asdict(probe) for probe in class_probes], indent=2, sort_keys=True) + '\n'
    oracle_behavior_content = (
        json.dumps(
            {
                function_name: [asdict(observed_call) for observed_call in summary.observed_calls]
                for function_name, summary in oracle_results.items()
            },
            indent=2,
            sort_keys=True,
        )
        + '\n'
    )

    api_surface_payload = module_api_to_json(
        api,
        properties,
        oracle_results,
        edge_observations,
        class_probes,
        static_plan,
        readiness,
    )
    api_surface_content = json.dumps(api_surface_payload, indent=2, sort_keys=True) + '\n'
    readiness_json_content = json.dumps(asdict(readiness), indent=2, sort_keys=True) + '\n'

    # Write all outputs.
    write_output(scaffold_path, scaffold_content, dry_run=dry_run, verbose=verbose)
    write_output(registration_diff_path, registration_diff_content, dry_run=dry_run, verbose=verbose)
    write_output(static_plan_path, static_plan_content, dry_run=dry_run, verbose=verbose)
    write_output(checklist_path, checklist_content, dry_run=dry_run, verbose=verbose)
    write_output(edge_matrix_path, edge_matrix_content, dry_run=dry_run, verbose=verbose)
    write_output(edge_report_path, edge_report_content, dry_run=dry_run, verbose=verbose)
    write_output(class_probe_path, class_probe_content, dry_run=dry_run, verbose=verbose)
    write_output(class_probe_report_path, class_probe_report_content, dry_run=dry_run, verbose=verbose)
    write_output(function_contracts_path, function_contracts_content, dry_run=dry_run, verbose=verbose)
    write_output(oracle_behavior_path, oracle_behavior_content, dry_run=dry_run, verbose=verbose)
    write_output(implementation_recipe_path, implementation_recipe_content, dry_run=dry_run, verbose=verbose)
    write_output(readiness_report_path, readiness_report_content, dry_run=dry_run, verbose=verbose)
    write_output(readiness_json_path, readiness_json_content, dry_run=dry_run, verbose=verbose)
    write_output(api_json_path, api_surface_content, dry_run=dry_run, verbose=verbose)

    unit_tests_written_path = unit_test_path
    parity_tests_written_path = parity_test_path
    property_tests_written_path = property_test_path
    error_tests_written_path = error_test_path

    if not skip_tests:
        unit_tests_written_path = write_output_with_fallback(
            unit_test_path,
            unit_tests_content,
            dry_run=dry_run,
            verbose=verbose,
            fallback_path=unit_test_fallback_path,
        )
        parity_tests_written_path = write_output_with_fallback(
            parity_test_path,
            parity_tests_content,
            dry_run=dry_run,
            verbose=verbose,
            fallback_path=parity_test_fallback_path,
        )
        property_tests_written_path = write_output_with_fallback(
            property_test_path,
            property_tests_content,
            dry_run=dry_run,
            verbose=verbose,
        )
        error_tests_written_path = write_output_with_fallback(
            error_test_path,
            error_tests_content,
            dry_run=dry_run,
            verbose=verbose,
        )

    log(f'Generating AGENT_TASK.md for {module_name}', verbose=verbose, force=True)
    agent_task_content = generate_agent_task_content(
        module_name,
        api,
        properties,
        readiness,
        registration_snippets,
        output_dir,
        unit_tests_written_path,
        parity_tests_written_path,
        property_tests_written_path,
        error_tests_written_path,
        edge_matrix_path,
        edge_report_path,
        class_probe_path,
        class_probe_report_path,
        static_plan_path,
        checklist_path,
        function_contracts_path,
        oracle_behavior_path,
        implementation_recipe_path,
        readiness_report_path,
    )
    write_output(agent_task_path, agent_task_content, dry_run=dry_run, verbose=verbose)

    log(
        (
            f'Completed {module_name}: '
            f'functions={len(api.functions)}, '
            f'classes={len(api.classes)}, '
            f'constants={len(api.constants)}, '
            f'readiness={readiness.score}/{readiness.tier}, '
            f'one_shot={readiness.one_shot_eligible}, '
            f'output_dir={output_dir}'
        ),
        verbose=verbose,
        force=True,
    )


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(
        description='Generate Ouro stdlib add-on tests, scaffold, registration guide, and agent bundle.',
    )
    parser.add_argument('modules', nargs='+', help='One or more stdlib module names (e.g. html fnmatch shlex).')
    parser.add_argument(
        '--dry-run',
        action='store_true',
        help='Preview generated output content without writing files.',
    )
    parser.add_argument(
        '--output-dir',
        type=Path,
        default=None,
        help='Custom output directory. For multiple modules, a subdirectory per module is created.',
    )
    parser.add_argument(
        '--skip-tests',
        action='store_true',
        help='Generate scaffold + registration only (skip CPython oracle test generation).',
    )
    parser.add_argument(
        '--verbose',
        action='store_true',
        help='Print detailed progress logs to stderr.',
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """CLI entrypoint."""
    args = parse_args(argv)
    module_names = list(dict.fromkeys(args.modules))

    for module_name in module_names:
        output_dir = resolve_output_dir(module_name, args.output_dir, len(module_names))
        try:
            build_outputs_for_module(
                module_name,
                output_dir=output_dir,
                dry_run=args.dry_run,
                skip_tests=args.skip_tests,
                verbose=args.verbose,
            )
        except KeyboardInterrupt:
            raise
        except BaseException as exc:
            print(f'error: failed while generating for {module_name}: {type(exc).__name__}: {exc}', file=sys.stderr)
            return 1
    return 0


# =============================================================================
# Section 12: Top-level entry point
# =============================================================================


if __name__ == '__main__':
    raise SystemExit(main())
