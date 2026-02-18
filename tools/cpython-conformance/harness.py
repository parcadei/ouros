#!/usr/bin/env python3
"""CPython Conformance Differential Test Harness.

Runs Python snippets through both CPython and ouros, compares their stdout
output and exit codes, and reports conformance. Designed for CI integration
with structured JSON and JUnit XML output.

Usage:
    python3 harness.py [--snippets-dir DIR] [--ouros-bin PATH] [--json] [--junit] [--verbose]

See --help for all options.
"""

import argparse
import difflib
import fnmatch
import json
import os
import re
import subprocess
import sys
import textwrap
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional
from xml.sax.saxutils import escape as xml_escape


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------

@dataclass
class CapturedResult:
    """Result of running a snippet through an interpreter."""

    stdout: str
    stderr: str
    exit_code: int
    duration_ms: float
    timed_out: bool


@dataclass
class ComparisonVerdict:
    """Verdict from comparing CPython and ouros output."""

    verdict: str  # PASS, FAIL, ERROR, SKIP, TIMEOUT, BAD_SNIPPET
    diff: Optional[str] = None
    message: Optional[str] = None


# ---------------------------------------------------------------------------
# Snippet metadata parsing
# ---------------------------------------------------------------------------

def parse_snippet_metadata(path: Path) -> dict:
    """Parse header comment metadata from a snippet file.

    Expected format:
        # conformance: <category>
        # description: <text>
        # expect: pass|error|skip
        # tags: tag1,tag2
        # ---

    Args:
        path: Path to the .py snippet file.

    Returns:
        Dictionary with keys: conformance, description, expect, tags.
    """
    meta = {
        "conformance": "unknown",
        "description": "",
        "expect": "pass",
        "tags": [],
    }

    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return meta

    for line in text.splitlines():
        stripped = line.strip()
        if stripped == "# ---":
            break
        if not stripped.startswith("#"):
            break

        # Parse "# key: value" format
        match = re.match(r"^#\s+(\w+):\s*(.+)$", stripped)
        if match:
            key, value = match.group(1), match.group(2).strip()
            if key == "conformance":
                meta["conformance"] = value
            elif key == "description":
                meta["description"] = value
            elif key == "expect":
                meta["expect"] = value
            elif key == "tags":
                meta["tags"] = [t.strip() for t in value.split(",") if t.strip()]

    return meta


# ---------------------------------------------------------------------------
# Output normalization
# ---------------------------------------------------------------------------

def normalize_output(text: str) -> str:
    """Normalize interpreter output for fair comparison.

    Applies these transformations:
    - Normalize CRLF to LF
    - Strip trailing whitespace from each line
    - Strip trailing empty lines
    - Normalize memory addresses (0x...) to 0xADDR
    - Normalize object id=N to id=ID
    - Normalize <class '__main__.Foo'> to <class 'Foo'>

    Args:
        text: Raw stdout from an interpreter.

    Returns:
        Normalized string for comparison.
    """
    # Normalize line endings
    text = text.replace("\r\n", "\n")

    # Filter ouros diagnostic lines that leak to stdout
    lines = text.split("\n")
    lines = [l for l in lines if not re.match(
        r"^(time taken to run typing:|Reading file:|type checking|success after:|error after:)",
        l,
    )]

    # Strip trailing whitespace per line
    lines = [line.rstrip() for line in lines]

    # Strip trailing empty lines
    while lines and not lines[-1]:
        lines.pop()

    text = "\n".join(lines)

    # Normalize memory addresses: 0x followed by hex digits
    text = re.sub(r"0x[0-9a-fA-F]+", "0xADDR", text)

    # Normalize object ids: id=<digits>
    text = re.sub(r"id=\d+", "id=ID", text)

    # Normalize class repr: <class '__main__.Foo'> -> <class 'Foo'>
    text = re.sub(r"<class '(?:__main__\.)([^']+)'>", r"<class '\1'>", text)

    return text


# ---------------------------------------------------------------------------
# Snippet discovery
# ---------------------------------------------------------------------------

def discover_snippets(
    snippets_dir: Path,
    category: Optional[str] = None,
    pattern: Optional[str] = None,
) -> list[Path]:
    """Find all .py snippet files in the snippets directory.

    Args:
        snippets_dir: Root directory containing category subdirectories.
        category: If set, only return snippets from this category subdirectory.
        pattern: If set, only return snippets whose filename matches this glob.

    Returns:
        Sorted list of Path objects for each discovered snippet.
    """
    snippets = []

    if not snippets_dir.is_dir():
        return snippets

    for py_file in sorted(snippets_dir.rglob("*.py")):
        if not py_file.is_file():
            continue

        # Category filter: check if snippet is under the category subdir
        if category:
            rel = py_file.relative_to(snippets_dir)
            parts = rel.parts
            if len(parts) < 2 or parts[0] != category:
                continue

        # Pattern filter on filename
        if pattern and not fnmatch.fnmatch(py_file.name, pattern):
            continue

        snippets.append(py_file)

    return sorted(snippets)


# ---------------------------------------------------------------------------
# Interpreter execution
# ---------------------------------------------------------------------------

def run_interpreter(
    binary: str,
    script_path: Path,
    timeout: float,
) -> CapturedResult:
    """Run a Python file through an interpreter, capturing all output.

    Args:
        binary: Path to the interpreter binary (e.g. "python3" or "/path/to/ouros").
        script_path: Path to the .py file to execute.
        timeout: Maximum execution time in seconds.

    Returns:
        CapturedResult with stdout, stderr, exit code, duration, and timeout flag.
    """
    env = {**os.environ, "PYTHONHASHSEED": "0"}

    start = time.monotonic()
    try:
        result = subprocess.run(
            [binary, str(script_path)],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
        )
        elapsed = (time.monotonic() - start) * 1000
        return CapturedResult(
            stdout=result.stdout,
            stderr=result.stderr,
            exit_code=result.returncode,
            duration_ms=elapsed,
            timed_out=False,
        )
    except subprocess.TimeoutExpired:
        elapsed = (time.monotonic() - start) * 1000
        return CapturedResult(
            stdout="",
            stderr="",
            exit_code=-1,
            duration_ms=elapsed,
            timed_out=True,
        )
    except FileNotFoundError:
        elapsed = (time.monotonic() - start) * 1000
        return CapturedResult(
            stdout="",
            stderr=f"Binary not found: {binary}",
            exit_code=-1,
            duration_ms=elapsed,
            timed_out=False,
        )


# ---------------------------------------------------------------------------
# Comparison logic
# ---------------------------------------------------------------------------

def compare_results(
    cpython: CapturedResult,
    ouros: CapturedResult,
    meta: dict,
) -> ComparisonVerdict:
    """Compare CPython and ouros results to determine verdict.

    Verdict logic:
    - SKIP if meta['expect'] == 'skip'
    - TIMEOUT if ouros timed out
    - BAD_SNIPPET if CPython failed (exit != 0) and expect != 'error'
    - ERROR if ouros crashed (signal-killed, negative exit code)
    - ERROR if ouros failed but CPython succeeded
    - For expect='error': PASS if both returned non-zero exit
    - For expect='pass': PASS if normalized stdout matches and exit codes match

    Args:
        cpython: Result from CPython.
        ouros: Result from ouros.
        meta: Snippet metadata dict with 'expect' key.

    Returns:
        ComparisonVerdict with verdict string and optional diff/message.
    """
    expect = meta.get("expect", "pass")

    # Skip
    if expect == "skip":
        return ComparisonVerdict(verdict="SKIP", message="Snippet marked as skip")

    # Timeout
    if ouros.timed_out:
        return ComparisonVerdict(verdict="TIMEOUT", message="ouros timed out")

    if cpython.timed_out:
        return ComparisonVerdict(
            verdict="TIMEOUT", message="CPython timed out"
        )

    # BAD_SNIPPET: CPython itself failed on a snippet not expected to error
    if cpython.exit_code != 0 and expect != "error":
        return ComparisonVerdict(
            verdict="BAD_SNIPPET",
            message=f"CPython exited with code {cpython.exit_code}",
        )

    # expect: error -- both should fail
    if expect == "error":
        if cpython.exit_code != 0 and ouros.exit_code != 0:
            return ComparisonVerdict(verdict="PASS", message="Both errored as expected")
        elif cpython.exit_code != 0 and ouros.exit_code == 0:
            return ComparisonVerdict(
                verdict="FAIL",
                message="CPython errored but ouros succeeded",
            )
        elif cpython.exit_code == 0 and ouros.exit_code != 0:
            return ComparisonVerdict(
                verdict="FAIL",
                message="ouros errored but CPython succeeded",
            )
        else:
            return ComparisonVerdict(
                verdict="FAIL",
                message="Neither interpreter errored (expected error)",
            )

    # expect: pass -- compare stdout and exit codes

    # ouros crashed (signal-killed)
    if ouros.exit_code < 0:
        sig = -ouros.exit_code
        return ComparisonVerdict(
            verdict="ERROR",
            message=f"ouros killed by signal {sig}",
        )

    # ouros failed but CPython succeeded
    if ouros.exit_code != 0 and cpython.exit_code == 0:
        return ComparisonVerdict(
            verdict="ERROR",
            message=f"ouros exited with code {ouros.exit_code}, CPython exited 0",
        )

    # Normalize and compare stdout
    cp_stdout = normalize_output(cpython.stdout)
    ou_stdout = normalize_output(ouros.stdout)

    stdout_match = cp_stdout == ou_stdout
    exit_match = cpython.exit_code == ouros.exit_code

    if stdout_match and exit_match:
        return ComparisonVerdict(verdict="PASS")

    # Generate diff for failure
    diff_text = generate_diff(cp_stdout, ou_stdout)
    message_parts = []
    if not stdout_match:
        message_parts.append("stdout differs")
    if not exit_match:
        message_parts.append(
            f"exit codes differ (cpython={cpython.exit_code}, ouros={ouros.exit_code})"
        )

    return ComparisonVerdict(
        verdict="FAIL",
        diff=diff_text if not stdout_match else None,
        message="; ".join(message_parts),
    )


# ---------------------------------------------------------------------------
# Diff generation
# ---------------------------------------------------------------------------

def generate_diff(cpython_output: str, ouros_output: str) -> str:
    """Generate a unified diff between CPython and ouros output.

    Args:
        cpython_output: Normalized CPython stdout.
        ouros_output: Normalized ouros stdout.

    Returns:
        Unified diff string.
    """
    cp_lines = cpython_output.splitlines(keepends=True)
    ou_lines = ouros_output.splitlines(keepends=True)
    diff = difflib.unified_diff(
        cp_lines,
        ou_lines,
        fromfile="cpython",
        tofile="ouros",
        lineterm="",
    )
    return "\n".join(diff)


# ---------------------------------------------------------------------------
# Category grouping
# ---------------------------------------------------------------------------

def group_by_category(results: list[dict]) -> dict:
    """Group test results by category with pass/fail counts.

    Args:
        results: List of result dicts, each with 'category' and 'verdict' keys.

    Returns:
        Dict mapping category name to {total, passed, failed, errors, skipped}.
    """
    groups: dict[str, dict] = {}

    for r in results:
        cat = r.get("category", "unknown")
        if cat not in groups:
            groups[cat] = {
                "total": 0,
                "passed": 0,
                "failed": 0,
                "errors": 0,
                "skipped": 0,
                "timeouts": 0,
                "bad_snippets": 0,
            }

        g = groups[cat]
        g["total"] += 1

        verdict = r.get("verdict", "")
        if verdict == "PASS":
            g["passed"] += 1
        elif verdict == "FAIL":
            g["failed"] += 1
        elif verdict == "ERROR":
            g["errors"] += 1
        elif verdict == "SKIP":
            g["skipped"] += 1
        elif verdict == "TIMEOUT":
            g["timeouts"] += 1
        elif verdict == "BAD_SNIPPET":
            g["bad_snippets"] += 1

    return groups


# ---------------------------------------------------------------------------
# Report generation: JSON
# ---------------------------------------------------------------------------

def build_json_report(
    results: list[dict],
    ouros_binary: str,
    cpython_binary: str,
) -> dict:
    """Build a JSON-serializable report from test results.

    Args:
        results: List of per-snippet result dicts.
        ouros_binary: Path to ouros binary used.
        cpython_binary: Path to CPython binary used.

    Returns:
        Complete report dict with metadata and results.
    """
    total = len(results)
    passed = sum(1 for r in results if r["verdict"] == "PASS")
    failed = sum(1 for r in results if r["verdict"] == "FAIL")
    errors = sum(1 for r in results if r["verdict"] == "ERROR")
    skipped = sum(1 for r in results if r["verdict"] == "SKIP")
    timeouts = sum(1 for r in results if r["verdict"] == "TIMEOUT")
    bad_snippets = sum(1 for r in results if r["verdict"] == "BAD_SNIPPET")

    testable = total - skipped
    pass_rate = passed / testable if testable > 0 else 0.0

    # Get CPython version
    cpython_version = ""
    try:
        cp = subprocess.run(
            [cpython_binary, "--version"],
            capture_output=True, text=True, timeout=5,
        )
        cpython_version = cp.stdout.strip().replace("Python ", "")
    except Exception:
        pass

    return {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "ouros_binary": ouros_binary,
        "cpython_binary": cpython_binary,
        "cpython_version": cpython_version,
        "total": total,
        "passed": passed,
        "failed": failed,
        "errors": errors,
        "skipped": skipped,
        "timeouts": timeouts,
        "bad_snippets": bad_snippets,
        "pass_rate": round(pass_rate, 4),
        "categories": group_by_category(results),
        "results": results,
    }


# ---------------------------------------------------------------------------
# Report generation: JUnit XML
# ---------------------------------------------------------------------------

def build_junit_xml(results: list[dict]) -> str:
    """Build a JUnit XML report from test results.

    Args:
        results: List of per-snippet result dicts.

    Returns:
        JUnit XML string.
    """
    total = len(results)
    failures = sum(1 for r in results if r["verdict"] == "FAIL")
    errors = sum(
        1 for r in results
        if r["verdict"] in ("ERROR", "TIMEOUT", "BAD_SNIPPET")
    )
    skipped = sum(1 for r in results if r["verdict"] == "SKIP")

    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<testsuite name="cpython-conformance" tests="{total}" '
        f'failures="{failures}" errors="{errors}" skipped="{skipped}">',
    ]

    for r in results:
        snippet = xml_escape(r.get("snippet", "unknown"))
        category = xml_escape(r.get("category", "unknown"))
        verdict = r.get("verdict", "UNKNOWN")
        duration_s = (r.get("duration_ouros_ms", 0) or 0) / 1000.0

        lines.append(
            f'  <testcase name="{snippet}" '
            f'classname="{category}" '
            f'time="{duration_s:.3f}">'
        )

        if verdict == "FAIL":
            diff = r.get("diff", {})
            diff_text = ""
            if isinstance(diff, dict):
                diff_text = diff.get("stdout", "")
            elif isinstance(diff, str):
                diff_text = diff
            message = r.get("message", "stdout mismatch")
            lines.append(
                f'    <failure message="{xml_escape(message)}">'
                f"{xml_escape(diff_text)}</failure>"
            )
        elif verdict in ("ERROR", "TIMEOUT", "BAD_SNIPPET"):
            message = r.get("message", verdict)
            lines.append(
                f'    <error message="{xml_escape(message)}"/>'
            )
        elif verdict == "SKIP":
            message = r.get("message", "skipped")
            lines.append(
                f'    <skipped message="{xml_escape(message)}"/>'
            )

        lines.append("  </testcase>")

    lines.append("</testsuite>")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Terminal output (colored)
# ---------------------------------------------------------------------------

# ANSI color codes
_COLORS = {
    "green": "\033[32m",
    "red": "\033[31m",
    "yellow": "\033[33m",
    "cyan": "\033[36m",
    "dim": "\033[2m",
    "bold": "\033[1m",
    "reset": "\033[0m",
}


def _color(text: str, color: str) -> str:
    """Wrap text in ANSI color codes if stdout is a TTY."""
    if not sys.stdout.isatty():
        return text
    return f"{_COLORS.get(color, '')}{text}{_COLORS['reset']}"


def _verdict_colored(verdict: str) -> str:
    """Color-code a verdict string."""
    colors = {
        "PASS": "green",
        "FAIL": "red",
        "ERROR": "red",
        "SKIP": "yellow",
        "TIMEOUT": "yellow",
        "BAD_SNIPPET": "yellow",
    }
    return _color(f"{verdict:>11}", colors.get(verdict, "dim"))


def print_terminal_summary(
    results: list[dict],
    verbose: bool = False,
) -> None:
    """Print a human-readable terminal summary.

    Args:
        results: List of per-snippet result dicts.
        verbose: If True, show full diff on failures.
    """
    width = 60
    print()
    print("=" * width)
    print(_color(" CPYTHON CONFORMANCE RESULTS", "bold"))
    print("=" * width)
    print()

    for r in results:
        snippet = r.get("snippet", "unknown")
        verdict = r.get("verdict", "UNKNOWN")
        line = f"  {_verdict_colored(verdict)}  {snippet}"
        print(line)

        if verdict in ("FAIL", "ERROR", "TIMEOUT", "BAD_SNIPPET"):
            message = r.get("message")
            if message:
                print(f"             {_color(message, 'dim')}")

            if verbose and verdict == "FAIL":
                diff = r.get("diff")
                if isinstance(diff, dict):
                    diff_text = diff.get("stdout", "")
                elif isinstance(diff, str):
                    diff_text = diff
                else:
                    diff_text = ""
                if diff_text:
                    for dl in diff_text.splitlines():
                        if dl.startswith("+"):
                            print(f"             {_color(dl, 'green')}")
                        elif dl.startswith("-"):
                            print(f"             {_color(dl, 'red')}")
                        else:
                            print(f"             {dl}")

    # Category summary
    groups = group_by_category(results)
    if groups:
        print()
        print(_color("  Per-category:", "bold"))
        for cat in sorted(groups):
            g = groups[cat]
            rate = g["passed"] / (g["total"] - g.get("skipped", 0)) * 100 \
                if (g["total"] - g.get("skipped", 0)) > 0 else 0
            print(
                f"    {cat:<25} "
                f"{_color(str(g['passed']), 'green')}/{g['total']} "
                f"({rate:.0f}%)"
            )

    # Overall summary
    total = len(results)
    passed = sum(1 for r in results if r["verdict"] == "PASS")
    failed = sum(1 for r in results if r["verdict"] == "FAIL")
    errors = sum(1 for r in results if r["verdict"] == "ERROR")
    skipped = sum(1 for r in results if r["verdict"] == "SKIP")
    timeouts = sum(1 for r in results if r["verdict"] == "TIMEOUT")

    testable = total - skipped
    pass_rate = (passed / testable * 100) if testable > 0 else 0

    print()
    print("-" * width)
    parts = [
        f" {_color('PASS:', 'green')} {passed}",
        f" {_color('FAIL:', 'red')} {failed}",
        f" ERROR: {errors}",
    ]
    if skipped:
        parts.append(f" SKIP: {skipped}")
    if timeouts:
        parts.append(f" TIMEOUT: {timeouts}")
    parts.append(f" Total: {total}")
    print(" ".join(parts))

    rate_color = "green" if pass_rate >= 80 else "yellow" if pass_rate >= 50 else "red"
    print(
        f" Conformance: {_color(f'{pass_rate:.1f}%', rate_color)} "
        f"({passed}/{testable})"
    )
    print("-" * width)
    print()


# ---------------------------------------------------------------------------
# Main orchestration
# ---------------------------------------------------------------------------

def find_ouros_binary() -> Optional[str]:
    """Auto-detect the ouros binary from common locations.

    Returns:
        Path to ouros binary, or None if not found.
    """
    candidates = [
        "/tmp/ouro-fresh/target/release/ouros",
        "/tmp/ouro-fresh/target/debug/ouros",
        "target/release/ouros",
        "target/debug/ouros",
    ]
    for c in candidates:
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return c
    return None


def run_single_snippet(
    snippet_path: Path,
    snippets_dir: Path,
    ouros_binary: str,
    cpython_binary: str,
    timeout: float,
    verbose: bool = False,
) -> dict:
    """Run a single snippet through both interpreters and compare.

    Args:
        snippet_path: Path to the .py snippet.
        snippets_dir: Root snippets directory (for relative path display).
        ouros_binary: Path to ouros binary.
        cpython_binary: Path to CPython binary.
        timeout: Per-snippet timeout in seconds.
        verbose: Whether to show extra diagnostic info.

    Returns:
        Result dict with all fields needed for reporting.
    """
    meta = parse_snippet_metadata(snippet_path)

    # Compute display path relative to snippets dir
    try:
        rel_path = snippet_path.relative_to(snippets_dir)
    except ValueError:
        rel_path = snippet_path

    # Determine category from parent directory
    parts = rel_path.parts
    category = parts[0] if len(parts) > 1 else meta.get("conformance", "unknown")

    result = {
        "snippet": str(rel_path),
        "category": category,
        "description": meta.get("description", ""),
        "tags": meta.get("tags", []),
        "expect": meta.get("expect", "pass"),
    }

    # Skip early
    if meta.get("expect") == "skip":
        result["verdict"] = "SKIP"
        result["message"] = "Snippet marked as skip"
        result["duration_cpython_ms"] = 0
        result["duration_ouros_ms"] = 0
        result["cpython_exit"] = 0
        result["ouros_exit"] = 0
        return result

    # Run CPython
    cpython_result = run_interpreter(cpython_binary, snippet_path, timeout)
    result["duration_cpython_ms"] = round(cpython_result.duration_ms, 1)
    result["cpython_exit"] = cpython_result.exit_code

    # Run ouros
    ouros_result = run_interpreter(ouros_binary, snippet_path, timeout)
    result["duration_ouros_ms"] = round(ouros_result.duration_ms, 1)
    result["ouros_exit"] = ouros_result.exit_code

    # Compare
    verdict = compare_results(cpython_result, ouros_result, meta)
    result["verdict"] = verdict.verdict
    if verdict.message:
        result["message"] = verdict.message
    if verdict.diff:
        result["diff"] = {"stdout": verdict.diff}

    # Store raw output for verbose/debug
    if verbose:
        result["cpython_stdout"] = cpython_result.stdout
        result["cpython_stderr"] = cpython_result.stderr
        result["ouros_stdout"] = ouros_result.stdout
        result["ouros_stderr"] = ouros_result.stderr

    return result


def main(argv: Optional[list[str]] = None) -> int:
    """Main entry point for the harness.

    Args:
        argv: Command-line arguments (defaults to sys.argv[1:]).

    Returns:
        Exit code: 0 if pass rate meets threshold, 1 otherwise.
    """
    # Determine the default snippets dir relative to this script
    script_dir = Path(__file__).resolve().parent
    default_snippets = script_dir / "snippets"

    parser = argparse.ArgumentParser(
        description="CPython Conformance Differential Test Harness",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent("""\
            Examples:
              python3 harness.py
              python3 harness.py --ouros-bin /path/to/ouros --verbose
              python3 harness.py --json > results.json
              python3 harness.py --junit > results.xml
        """),
    )
    parser.add_argument(
        "--ouros-bin", "--ouros-binary",
        default=None,
        help="Path to ouros binary (auto-detects from target/release or target/debug)",
    )
    parser.add_argument(
        "--cpython-bin", "--cpython-binary",
        default="python3",
        help="Path to CPython binary (default: python3)",
    )
    parser.add_argument(
        "--snippets-dir",
        default=str(default_snippets),
        help=f"Directory containing test snippets (default: {default_snippets})",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Write JSON results to this file path",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print JSON results to stdout",
    )
    parser.add_argument(
        "--junit",
        default=None,
        help="Write JUnit XML results to this file path",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=10.0,
        help="Per-snippet timeout in seconds (default: 10)",
    )
    parser.add_argument(
        "--filter",
        default=None,
        help="Only run snippets matching this glob pattern",
    )
    parser.add_argument(
        "--category",
        default=None,
        help="Only run snippets in this category subdirectory",
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Show full stdout/stderr on failures",
    )
    parser.add_argument(
        "--fail-fast",
        action="store_true",
        help="Stop on first failure",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=0.0,
        help="Minimum pass rate (0.0-1.0) to exit with code 0 (default: 0.0)",
    )

    args = parser.parse_args(argv)

    # Find ouros binary
    ouros_bin = args.ouros_bin or find_ouros_binary()
    if not ouros_bin:
        print(
            "ERROR: ouros binary not found.\n"
            "\n"
            "Build it with:\n"
            "  cargo build -p ouros-cli --release\n"
            "\n"
            "Or specify the path:\n"
            "  python3 harness.py --ouros-bin /path/to/ouros\n",
            file=sys.stderr,
        )
        return 1

    if not os.path.isfile(ouros_bin):
        print(f"ERROR: ouros binary not found at: {ouros_bin}", file=sys.stderr)
        return 1

    if not os.access(ouros_bin, os.X_OK):
        print(f"ERROR: ouros binary is not executable: {ouros_bin}", file=sys.stderr)
        return 1

    # Find snippets
    snippets_dir = Path(args.snippets_dir)
    if not snippets_dir.is_dir():
        print(f"ERROR: snippets directory not found: {snippets_dir}", file=sys.stderr)
        return 1

    snippets = discover_snippets(
        snippets_dir,
        category=args.category,
        pattern=args.filter,
    )

    if not snippets:
        print(f"No snippets found in {snippets_dir}", file=sys.stderr)
        return 1

    # Run all snippets
    results = []
    for snippet_path in snippets:
        result = run_single_snippet(
            snippet_path=snippet_path,
            snippets_dir=snippets_dir,
            ouros_binary=ouros_bin,
            cpython_binary=args.cpython_bin,
            timeout=args.timeout,
            verbose=args.verbose,
        )
        results.append(result)

        # Fail-fast
        if args.fail_fast and result["verdict"] in ("FAIL", "ERROR"):
            break

    # Output: JSON to stdout
    if args.json:
        report = build_json_report(results, ouros_bin, args.cpython_bin)
        print(json.dumps(report, indent=2))
    else:
        # Terminal summary
        print_terminal_summary(results, verbose=args.verbose)

    # Output: JSON to file
    if args.output:
        report = build_json_report(results, ouros_bin, args.cpython_bin)
        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(json.dumps(report, indent=2) + "\n")
        print(f"JSON results written to: {args.output}", file=sys.stderr)

    # Output: JUnit XML
    if args.junit:
        xml = build_junit_xml(results)
        junit_path = Path(args.junit)
        junit_path.parent.mkdir(parents=True, exist_ok=True)
        junit_path.write_text(xml + "\n")
        print(f"JUnit XML written to: {args.junit}", file=sys.stderr)

    # Check threshold
    total = len(results)
    skipped = sum(1 for r in results if r["verdict"] == "SKIP")
    passed = sum(1 for r in results if r["verdict"] == "PASS")
    testable = total - skipped
    pass_rate = passed / testable if testable > 0 else 0.0

    if args.threshold > 0 and pass_rate < args.threshold:
        print(
            f"FAIL: Pass rate {pass_rate:.1%} is below threshold {args.threshold:.1%}",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
