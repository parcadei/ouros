#!/usr/bin/env python3
"""Cross-reference CPython stdlib API surface against Ouro.

This script captures API snapshots for selected stdlib modules from:
1. Host CPython (direct import + inspect)
2. Ouro (running introspection code inside a Ouro sandbox)

It then writes:
- `cpython_snapshot.json`
- `ouro_snapshot.json` (when ouro is available)
- `api_diff.json`
- `summary.txt`

The diff focuses on API surface parity:
- missing names in Ouro
- extra names in Ouro
- callable/type/text-signature mismatches
- class public-attribute surface mismatches
"""

from __future__ import annotations

import argparse
import importlib
import inspect
import json
import warnings
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

DEFAULT_MODULES = [
    'abc',
    'asyncio',
    'base64',
    'bisect',
    'collections',
    'contextlib',
    'copy',
    'csv',
    'dataclasses',
    'datetime',
    'decimal',
    'enum',
    'fractions',
    'functools',
    'hashlib',
    'heapq',
    'io',
    'itertools',
    'json',
    'math',
    'operator',
    'os.path',
    'pathlib',
    'pprint',
    'random',
    're',
    'statistics',
    'string',
    'struct',
    'sys',
    'textwrap',
    'time',
    'typing',
    'uuid',
    'weakref',
]


@dataclass
class SnapshotResult:
    """Module snapshot result including hard errors from collection."""

    module: str
    names: list[str]
    items: dict[str, dict[str, Any]]
    error: str | None = None

    def to_json(self) -> dict[str, Any]:
        """Convert to a JSON-serializable dictionary."""
        return {
            'module': self.module,
            'names': self.names,
            'items': self.items,
            'error': self.error,
        }


def _public_names(module: Any) -> list[str]:
    """Return public/exported names for a module.

    Uses `__all__` when available (source of truth for public API),
    otherwise falls back to non-underscore names from `dir(module)`.
    """
    all_names = getattr(module, '__all__', None)
    if isinstance(all_names, list | tuple):
        names = [name for name in all_names if isinstance(name, str)]
    else:
        names = [name for name in dir(module) if not name.startswith('_')]
    return sorted(set(names))


def _safe_signature(obj: Any) -> str | None:
    """Return string signature when available."""
    try:
        return str(inspect.signature(obj))
    except (TypeError, ValueError):
        return None


def _safe_text_signature(obj: Any) -> str | None:
    """Return `__text_signature__` when present and string."""
    value = getattr(obj, '__text_signature__', None)
    return value if isinstance(value, str) else None


def _class_public_attrs(cls: type[Any]) -> list[str]:
    """Return class attributes intended for public consumption."""
    return sorted(name for name in dir(cls) if not name.startswith('_'))


def collect_cpython_snapshot(module_name: str) -> SnapshotResult:
    """Collect a CPython API snapshot for one module."""
    try:
        module = importlib.import_module(module_name)
    except Exception as exc:  # pragma: no cover - defensive
        return SnapshotResult(module=module_name, names=[], items={}, error=f'{type(exc).__name__}: {exc}')

    names = _public_names(module)
    items: dict[str, dict[str, Any]] = {}

    for name in names:
        try:
            obj = getattr(module, name)
            entry: dict[str, Any] = {
                'python_type': type(obj).__name__,
                'callable': callable(obj),
                'is_class': inspect.isclass(obj),
                'module': getattr(obj, '__module__', None),
                'text_signature': _safe_text_signature(obj),
            }
            if callable(obj):
                entry['signature'] = _safe_signature(obj)
            if inspect.isclass(obj):
                entry['class_public_attrs'] = _class_public_attrs(obj)
            items[name] = entry
        except Exception as exc:  # pragma: no cover - defensive
            items[name] = {'error': f'{type(exc).__name__}: {exc}'}

    return SnapshotResult(module=module_name, names=names, items=items)


def _ouro_introspection_code(module_name: str) -> str:
    """Build Ouro code that snapshots one module's API metadata."""
    if not all(part.isidentifier() for part in module_name.split('.')):
        raise ValueError(f'Invalid module name for ouro introspection: {module_name!r}')
    literal = json.dumps(module_name)
    return f"""
import {module_name} as mod
if hasattr(mod, '__all__'):
    names = list(mod.__all__)
else:
    names = []
    for _name in dir(mod):
        if not _name.startswith('_'):
            names.append(_name)
names = sorted(set(names))

items = {{}}
for name in names:
    try:
        obj = getattr(mod, name)
        entry = {{}}
        entry['python_type'] = type(obj).__name__
        entry['callable'] = callable(obj)
        entry['is_class'] = type(obj) is type
        entry['module'] = obj.__module__ if hasattr(obj, '__module__') else None
        entry['text_signature'] = obj.__text_signature__ if hasattr(obj, '__text_signature__') else None
        if type(obj) is type:
            attrs = []
            for attr in dir(obj):
                if not attr.startswith('_'):
                    attrs.append(attr)
            attrs.sort()
            entry['class_public_attrs'] = attrs
        items[name] = entry
    except Exception as exc:
        items[name] = {{'error': type(exc).__name__ + ': ' + str(exc)}}

{{'module': {literal}, 'names': names, 'items': items}}
"""


def collect_ouro_snapshot(module_name: str, ouro_module: Any) -> SnapshotResult:
    """Collect a Ouro API snapshot for one module via sandboxed introspection."""
    code = _ouro_introspection_code(module_name)

    try:
        interpreter = ouro_module.Sandbox(code, script_name=f'api_snapshot_{module_name}.py')
        result = interpreter.run()
    except Exception as exc:
        return SnapshotResult(module=module_name, names=[], items={}, error=f'{type(exc).__name__}: {exc}')

    if not isinstance(result, dict):
        return SnapshotResult(
            module=module_name,
            names=[],
            items={},
            error=f'Unexpected ouro snapshot result type: {type(result).__name__}',
        )

    names = result.get('names', [])
    items = result.get('items', {})
    if not isinstance(names, list) or not isinstance(items, dict):
        return SnapshotResult(
            module=module_name,
            names=[],
            items={},
            error='Malformed ouro snapshot payload',
        )
    return SnapshotResult(module=module_name, names=[str(n) for n in names], items=items)


def diff_module(cpython: SnapshotResult, ouro: SnapshotResult) -> dict[str, Any]:
    """Diff two module snapshots."""
    if cpython.error or ouro.error:
        return {
            'module': cpython.module,
            'cpython_error': cpython.error,
            'ouro_error': ouro.error,
            'missing_in_ouro': [],
            'extra_in_ouro': [],
            'callable_mismatches': [],
            'type_mismatches': [],
            'text_signature_mismatches': [],
            'class_attr_diffs': [],
        }

    cp_names = set(cpython.names)
    mo_names = set(ouro.names)
    common_names = sorted(cp_names & mo_names)

    callable_mismatches: list[dict[str, Any]] = []
    type_mismatches: list[dict[str, Any]] = []
    text_signature_mismatches: list[dict[str, Any]] = []
    class_attr_diffs: list[dict[str, Any]] = []

    for name in common_names:
        cp = cpython.items.get(name, {})
        mo = ouro.items.get(name, {})

        if cp.get('error') or mo.get('error'):
            continue

        if cp.get('callable') != mo.get('callable'):
            callable_mismatches.append(
                {'name': name, 'cpython_callable': cp.get('callable'), 'ouro_callable': mo.get('callable')}
            )

        # Ignore concrete callable type name differences (e.g. function vs builtin_function_or_method),
        # since those are implementation details. Callable parity is tracked separately.
        if cp.get('python_type') != mo.get('python_type') and not (cp.get('callable') and mo.get('callable')):
            type_mismatches.append(
                {'name': name, 'cpython_type': cp.get('python_type'), 'ouro_type': mo.get('python_type')}
            )

        cp_text_sig = cp.get('text_signature')
        mo_text_sig = mo.get('text_signature')
        if isinstance(cp_text_sig, str) and isinstance(mo_text_sig, str) and cp_text_sig != mo_text_sig:
            text_signature_mismatches.append(
                {'name': name, 'cpython_text_signature': cp_text_sig, 'ouro_text_signature': mo_text_sig}
            )

        if cp.get('is_class') and mo.get('is_class'):
            cp_attrs = set(cp.get('class_public_attrs', []))
            mo_attrs = set(mo.get('class_public_attrs', []))
            missing_attrs = sorted(cp_attrs - mo_attrs)
            extra_attrs = sorted(mo_attrs - cp_attrs)
            if missing_attrs or extra_attrs:
                class_attr_diffs.append(
                    {'name': name, 'missing_attrs_in_ouro': missing_attrs, 'extra_attrs_in_ouro': extra_attrs}
                )

    return {
        'module': cpython.module,
        'cpython_error': None,
        'ouro_error': None,
        'missing_in_ouro': sorted(cp_names - mo_names),
        'extra_in_ouro': sorted(mo_names - cp_names),
        'callable_mismatches': callable_mismatches,
        'type_mismatches': type_mismatches,
        'text_signature_mismatches': text_signature_mismatches,
        'class_attr_diffs': class_attr_diffs,
    }


def build_summary(diff: dict[str, Any]) -> dict[str, Any]:
    """Build compact summary stats from full module diffs."""
    modules = diff['modules']
    summary = {
        'module_count': len(modules),
        'with_errors': 0,
        'with_name_diffs': 0,
        'with_shape_diffs': 0,
        'total_missing_names': 0,
        'total_extra_names': 0,
        'total_callable_mismatches': 0,
        'total_type_mismatches': 0,
        'total_text_signature_mismatches': 0,
        'total_class_attr_diff_entries': 0,
    }

    for module in modules:
        has_error = bool(module.get('cpython_error') or module.get('ouro_error'))
        if has_error:
            summary['with_errors'] += 1
            continue

        missing_count = len(module['missing_in_ouro'])
        extra_count = len(module['extra_in_ouro'])
        callable_count = len(module['callable_mismatches'])
        type_count = len(module['type_mismatches'])
        text_sig_count = len(module['text_signature_mismatches'])
        class_attr_count = len(module['class_attr_diffs'])

        summary['total_missing_names'] += missing_count
        summary['total_extra_names'] += extra_count
        summary['total_callable_mismatches'] += callable_count
        summary['total_type_mismatches'] += type_count
        summary['total_text_signature_mismatches'] += text_sig_count
        summary['total_class_attr_diff_entries'] += class_attr_count

        if missing_count or extra_count:
            summary['with_name_diffs'] += 1
        if callable_count or type_count or text_sig_count or class_attr_count:
            summary['with_shape_diffs'] += 1

    return summary


def build_grouped_markdown_report(
    diff: dict[str, Any],
    summary_lines: list[str],
    run_dir: Path,
) -> str:
    """Build a Markdown report grouped by stdlib module.

    The report is human-oriented and includes:
    - top-level summary copied from `summary.txt`
    - one section per module
    - per-module counts and detailed mismatches
    """
    lines: list[str] = []
    lines.append('# Stdlib API Diff Report (Grouped by Module)')
    lines.append('')
    lines.append(f'Run directory: `{run_dir}`')
    lines.append('')
    lines.append('## Summary')
    lines.append('')
    for line in summary_lines:
        if line:
            lines.append(f'- {line}')
    lines.append('')
    lines.append('## Modules')
    lines.append('')

    for module in sorted(diff['modules'], key=lambda module: module['module']):
        module_name = module['module']
        lines.append(f'### {module_name}')
        lines.append('')

        cpython_error = module.get('cpython_error')
        ouro_error = module.get('ouro_error')
        if cpython_error or ouro_error:
            lines.append(f'- CPython error: `{cpython_error}`')
            lines.append(f'- Ouro error: `{ouro_error}`')
            lines.append('')
            continue

        missing = module['missing_in_ouro']
        extra = module['extra_in_ouro']
        callable_mismatches = module['callable_mismatches']
        type_mismatches = module['type_mismatches']
        text_signature_mismatches = module['text_signature_mismatches']
        class_attr_diffs = module['class_attr_diffs']

        lines.append(f'- Missing in Ouro: {len(missing)}')
        if missing:
            lines.append(f"- Missing names: `{', '.join(missing)}`")

        lines.append(f'- Extra in Ouro: {len(extra)}')
        if extra:
            lines.append(f"- Extra names: `{', '.join(extra)}`")

        lines.append(f'- Callable mismatches: {len(callable_mismatches)}')
        for item in callable_mismatches:
            lines.append(
                f"- Callable mismatch `{item['name']}`: "
                f"CPython={item['cpython_callable']}, Ouro={item['ouro_callable']}"
            )

        lines.append(f'- Type mismatches: {len(type_mismatches)}')
        for item in type_mismatches:
            lines.append(
                f"- Type mismatch `{item['name']}`: "
                f"CPython=`{item['cpython_type']}`, Ouro=`{item['ouro_type']}`"
            )

        lines.append(f'- Text signature mismatches: {len(text_signature_mismatches)}')
        for item in text_signature_mismatches:
            lines.append(
                f"- Text signature mismatch `{item['name']}`: "
                f"CPython=`{item['cpython_text_signature']}`, Ouro=`{item['ouro_text_signature']}`"
            )

        lines.append(f'- Class attribute diff entries: {len(class_attr_diffs)}')
        for item in class_attr_diffs:
            missing_attrs = ', '.join(item['missing_attrs_in_ouro']) or '(none)'
            extra_attrs = ', '.join(item['extra_attrs_in_ouro']) or '(none)'
            lines.append(f"- Class attr diff `{item['name']}`: missing=[{missing_attrs}] extra=[{extra_attrs}]")

        lines.append('')

    return '\n'.join(lines) + '\n'


def _load_ouro_module() -> Any | None:
    """Import `ouro` if available."""
    try:
        import ouro
    except ImportError:
        return None
    return ouro


def _parse_args() -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(description='Diff CPython stdlib API shape against Ouro.')
    parser.add_argument(
        '--modules',
        nargs='*',
        default=DEFAULT_MODULES,
        help='Module names to audit (default: curated stdlib set used by parity tests).',
    )
    parser.add_argument(
        '--output-dir',
        default='tmp/api_diff',
        help='Directory where snapshots and diffs are written.',
    )
    parser.add_argument(
        '--skip-ouro',
        action='store_true',
        help='Collect only CPython snapshot (no Ouro import/execution).',
    )
    return parser.parse_args()


def main() -> int:
    """Entry point."""
    warnings.filterwarnings('ignore', category=DeprecationWarning)
    args = _parse_args()
    modules = list(dict.fromkeys(args.modules))
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    run_id = datetime.now(UTC).strftime('%Y%m%dT%H%M%SZ')
    run_dir = output_dir / run_id
    run_dir.mkdir(parents=True, exist_ok=True)

    cpython_snapshots = [collect_cpython_snapshot(module_name) for module_name in modules]
    cpython_payload = {'generated_at': run_id, 'runtime': 'cpython', 'modules': [s.to_json() for s in cpython_snapshots]}

    ouro_module = None if args.skip_ouro else _load_ouro_module()
    ouro_snapshots: list[SnapshotResult] = []
    if ouro_module is not None:
        ouro_snapshots = [collect_ouro_snapshot(module_name, ouro_module) for module_name in modules]
    else:
        ouro_snapshots = [SnapshotResult(module=module_name, names=[], items={}, error='ouro not available') for module_name in modules]

    ouro_payload = {'generated_at': run_id, 'runtime': 'ouro', 'modules': [s.to_json() for s in ouro_snapshots]}

    by_module_cpython = {s.module: s for s in cpython_snapshots}
    by_module_ouro = {s.module: s for s in ouro_snapshots}
    module_diffs = [diff_module(by_module_cpython[module_name], by_module_ouro[module_name]) for module_name in modules]
    diff_payload = {'generated_at': run_id, 'modules': module_diffs}
    diff_payload['summary'] = build_summary(diff_payload)

    cpython_path = run_dir / 'cpython_snapshot.json'
    ouro_path = run_dir / 'ouro_snapshot.json'
    diff_path = run_dir / 'api_diff.json'
    summary_path = run_dir / 'summary.txt'
    markdown_path = run_dir / 'api_diff_grouped_by_stdlib.md'

    cpython_path.write_text(json.dumps(cpython_payload, indent=2, sort_keys=True) + '\n')
    ouro_path.write_text(json.dumps(ouro_payload, indent=2, sort_keys=True) + '\n')
    diff_path.write_text(json.dumps(diff_payload, indent=2, sort_keys=True) + '\n')

    summary_lines = [
        f'Run ID: {run_id}',
        f'Modules: {len(modules)}',
        f'Errors: {diff_payload["summary"]["with_errors"]}',
        f'Modules with name diffs: {diff_payload["summary"]["with_name_diffs"]}',
        f'Modules with shape diffs: {diff_payload["summary"]["with_shape_diffs"]}',
        f'Total missing names: {diff_payload["summary"]["total_missing_names"]}',
        f'Total extra names: {diff_payload["summary"]["total_extra_names"]}',
        f'Total callable mismatches: {diff_payload["summary"]["total_callable_mismatches"]}',
        f'Total type mismatches: {diff_payload["summary"]["total_type_mismatches"]}',
        f'Total text-signature mismatches: {diff_payload["summary"]["total_text_signature_mismatches"]}',
        f'Total class attribute diff entries: {diff_payload["summary"]["total_class_attr_diff_entries"]}',
        '',
        f'CPython snapshot: {cpython_path}',
        f'Ouro snapshot:   {ouro_path}',
        f'Diff:             {diff_path}',
        f'Markdown report:  {markdown_path}',
    ]
    summary_path.write_text('\n'.join(summary_lines) + '\n')
    markdown_path.write_text(build_grouped_markdown_report(diff_payload, summary_lines, run_dir))

    print('\n'.join(summary_lines))
    if ouro_module is None and not args.skip_ouro:
        print('\nNOTE: ouro is not importable, so ouro snapshot contains per-module errors.')

    return 0


if __name__ == '__main__':
    raise SystemExit(main())
