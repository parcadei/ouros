# CPython Conformance Differential Test Harness

Created: 2026-02-18
Author: architect-agent

## Overview

A standalone differential testing tool that runs Python snippets through both ouros
and CPython, compares stdout/stderr/exit-code, and reports conformance. Unlike the
existing `datatest_runner.rs` (which embeds CPython via pyo3 in-process) and
`deep_parity_audit.sh` (which uses `cargo run` with heavy stderr filtering), this
harness runs both interpreters as **subprocesses** with clean capture, producing
structured JSON results suitable for CI dashboards.


## Erotetic Analysis

| Question | Answer |
|----------|--------|
| What exactly are we diffing? | stdout, stderr, and exit code from each interpreter |
| How does ouros run a `.py` file? | `cargo run -p ouros-cli --quiet -- <file.py>` produces the ouros binary `ouros` which reads a file and runs it. The CLI emits diagnostic noise on stderr (type-checking messages, timing). Only stdout carries Python `print()` output. |
| How does CPython run a `.py` file? | `python3 <file.py>` with `PYTHONHASHSEED=0` for determinism |
| What about ouros stderr noise? | The CLI writes `Reading file:`, `type checking ...`, `time taken ...`, `success after:` to stderr. The harness must either discard stderr entirely or strip known prefixes. |
| What about the existing `deep_parity_audit.sh`? | It already does ouros-vs-CPython comparison for 64 parity tests in `playground/parity_tests/`. The new harness complements it with: (a) structured output, (b) per-snippet granularity, (c) a snippet format that is self-contained and protocol-tagged, (d) JSON results for CI. |
| What about the `datatest_runner`? | It runs the 527 test fixtures in `test_cases/` through both ouros (via Rust API) and CPython (via pyo3). It uses expectation comments (`# Return=`, `# Raise=`, `TRACEBACK:`). The new harness is **complementary**: it tests raw subprocess behavior (matching what a real user would see) rather than in-process API behavior. |


## Requirements

- [x] Design: standalone script (Python, not Bash -- for structured output and better diff logic)
- [x] Design: takes a directory of `.py` test snippets
- [x] Design: runs each through `python3` and ouros as subprocesses, captures stdout + stderr + exit code
- [x] Design: diffs stdout (primary), stderr (informational), exit code
- [x] Design: reports PASS / FAIL / ERROR per snippet
- [x] Design: machine-readable JSON output for CI
- [x] Design: human-readable terminal summary
- [x] Design: snippet format with header comments for metadata
- [x] Design: protocol-based directory grouping


## Architecture

### Component Diagram

```
                   harness.py (orchestrator)
                       |
          +------------+------------+
          |                         |
    run_cpython()              run_ouros()
    python3 <file>         cargo run -p ouros-cli
          |              --quiet -- <file>
          v                         v
    CapturedResult             CapturedResult
    {stdout, stderr,           {stdout, stderr,
     exit_code}                 exit_code}
          |                         |
          +------------+------------+
                       |
                  compare()
                       |
                  TestVerdict
              {PASS|FAIL|ERROR|SKIP}
                       |
               +-------+-------+
               |               |
          json_report     terminal_summary
```

### How ouros Runs a .py File

The `ouros-cli` crate (at `crates/ouros-cli/src/main.rs`) defines a binary named
`ouros` that:

1. Reads `args[1]` as the file path (defaults to `example.py`)
2. Reads the file contents
3. Runs type checking (output to stderr)
4. Creates a `Runner::new(code, file_path, ...)` 
5. Calls `runner.run_no_limits(inputs)` which returns `Result<Object, Exception>`
6. On success, prints `success after: <duration>\n<value>` to stderr
7. On error, prints `error after: <duration>\n<exception>` to stderr
8. **Python `print()` calls go to real stdout** via `StdPrint` which buffers and
   flushes to `io::stdout()` on drop

**Key insight**: Python `print()` output goes to **stdout**. All ouros diagnostic
output goes to **stderr**. Therefore the harness should capture stdout for
comparison and stderr for diagnostics only.

**Build**: The binary must be pre-built via `cargo build -p ouros-cli`. It lives at
`target/debug/ouros` (or `target/release/ouros` for release builds). The harness
should accept a `--ouros-binary` flag to specify the path, defaulting to
`target/debug/ouros`.

**Important**: `CLAUDE.md` says "DO NOT run `cargo build` or `cargo run`" because
of Python binding issues. However, `cargo build -p ouros-cli` builds only the CLI
crate which has no pyo3 dependency -- it depends only on `ouros` (the core lib) and
`ouros_type_checking`. The pyo3 issue affects `cargo test` (which builds the test
harness with pyo3 dev-dependency). Building just the CLI binary should be safe.
To be cautious, the harness should document that users should run `make dev-py`
first, or build with `cargo build -p ouros-cli`.


### Subprocess Execution Model

```python
def run_interpreter(binary: str, script_path: str, timeout: float) -> CapturedResult:
    """Run a Python file through an interpreter, capturing all output."""
    result = subprocess.run(
        [binary, str(script_path)],
        capture_output=True,
        text=True,
        timeout=timeout,
        env={**os.environ, "PYTHONHASHSEED": "0"},
    )
    return CapturedResult(
        stdout=result.stdout,
        stderr=result.stderr,
        exit_code=result.returncode,
    )
```

For CPython: `binary = "python3"`
For ouros: `binary = "target/debug/ouros"` (or path from `--ouros-binary`)


### Comparison Logic

```python
def compare(cpython: CapturedResult, ouros: CapturedResult, config: SnippetConfig) -> TestVerdict:
    # 1. If ouros crashed (segfault, panic) -> ERROR
    if ouros.exit_code < 0:  # signal-killed
        return TestVerdict.ERROR

    # 2. Normalize stdout (strip trailing whitespace, normalize line endings)
    cp_stdout = normalize(cpython.stdout)
    ou_stdout = normalize(ouros.stdout)

    # 3. Compare exit codes
    exit_match = cpython.exit_code == ouros.exit_code

    # 4. Compare stdout
    stdout_match = cp_stdout == ou_stdout

    # 5. Determine verdict
    if stdout_match and exit_match:
        return TestVerdict.PASS
    elif not exit_match and ouros.exit_code != 0 and cpython.exit_code == 0:
        return TestVerdict.ERROR  # ouros failed, cpython succeeded
    else:
        return TestVerdict.FAIL  # divergent output
```


## Directory Structure

```
tools/cpython-conformance/
├── harness-spec.md              # This specification
├── harness.py                   # Main runner script
├── snippets/                    # Test snippets organized by protocol
│   ├── arithmetic/
│   │   ├── int_add.py
│   │   ├── int_div_zero.py
│   │   ├── float_ops.py
│   │   └── ...
│   ├── strings/
│   │   ├── str_concat.py
│   │   ├── str_methods.py
│   │   ├── fstring_basic.py
│   │   └── ...
│   ├── collections/
│   │   ├── list_ops.py
│   │   ├── dict_ops.py
│   │   ├── set_ops.py
│   │   └── ...
│   ├── control_flow/
│   │   ├── if_else.py
│   │   ├── for_loop.py
│   │   ├── while_loop.py
│   │   ├── try_except.py
│   │   └── ...
│   ├── functions/
│   │   ├── def_basic.py
│   │   ├── closures.py
│   │   ├── decorators.py
│   │   ├── generators.py
│   │   └── ...
│   ├── classes/
│   │   ├── basic_class.py
│   │   ├── inheritance.py
│   │   ├── dunder_methods.py
│   │   └── ...
│   ├── builtins/
│   │   ├── print_basic.py
│   │   ├── type_conversions.py
│   │   ├── builtin_funcs.py
│   │   └── ...
│   ├── stdlib/
│   │   ├── math_basic.py
│   │   ├── json_roundtrip.py
│   │   ├── re_basic.py
│   │   └── ...
│   └── edge_cases/
│       ├── empty_script.py
│       ├── syntax_error.py
│       ├── recursion_limit.py
│       └── ...
└── results/                     # Output directory (gitignored)
    ├── latest.json              # Most recent run results
    └── YYYY-MM-DD_HHMMSS.json  # Timestamped archives
```


## Snippet Format Specification

Each `.py` file is self-contained and follows this structure:

```python
# conformance: <protocol-category>
# description: <what this snippet tests>
# expect: pass | error | skip
# tags: <comma-separated tags>
# ---
# Actual Python code below. Uses only print() for observable output.

print(1 + 2)
print("hello" + " " + "world")
```

### Header Comment Fields

| Field | Required | Description |
|-------|----------|-------------|
| `conformance` | Yes | Protocol category (e.g., `arithmetic`, `strings`, `classes`) |
| `description` | Yes | Human-readable description of what is tested |
| `expect` | No | Expected outcome: `pass` (default), `error` (both should error), `skip` (skip this snippet) |
| `tags` | No | Comma-separated tags for filtering (e.g., `int,operator,addition`) |
| `---` | Yes | Separator between metadata and code |

### Rules for Snippets

1. **Self-contained**: No imports of test infrastructure. Only stdlib imports.
2. **Output via `print()` only**: All observable behavior must go through `print()`.
   This is what gets compared between interpreters.
3. **Deterministic**: No randomness, no timestamps, no memory addresses, no object
   ids in output. Use `PYTHONHASHSEED=0` (set by harness) for dict ordering.
4. **No file I/O**: Snippets must not read/write files (ouros is sandboxed).
5. **No network**: No socket/HTTP operations.
6. **Quick**: Each snippet should complete in under 5 seconds.
7. **Error tests**: For snippets that test error behavior, wrap in try/except and
   print the exception: `except Exception as e: print(type(e).__name__, e)`

### Example Snippets

**`snippets/arithmetic/int_add.py`**:
```python
# conformance: arithmetic
# description: Integer addition with various operand types
# tags: int,operator,addition
# ---
print(1 + 2)
print(0 + 0)
print(-1 + 1)
print(1000000000 + 1000000000)
print(2**62 + 2**62)  # large ints
```

**`snippets/strings/fstring_basic.py`**:
```python
# conformance: strings
# description: Basic f-string formatting
# tags: fstring,format
# ---
x = 42
print(f'{x}')
print(f'hello {x} world')
print(f'{x!r}')
print(f'{x:>10}')
print(f'{"nested"}')
```

**`snippets/edge_cases/syntax_error.py`**:
```python
# conformance: edge_cases
# description: Syntax error should produce non-zero exit code from both interpreters
# expect: error
# tags: syntax,error
# ---
def f(
    # missing closing paren
```


## How to Invoke ouros for Each Snippet

### Pre-built Binary (Recommended)

```bash
# Build once before running the harness
cargo build -p ouros-cli

# The binary is at:
target/debug/ouros
```

### Invocation

```bash
# ouros: captures stdout (print output) and stderr (diagnostics)
target/debug/ouros snippets/arithmetic/int_add.py

# CPython: captures stdout and stderr
PYTHONHASHSEED=0 python3 snippets/arithmetic/int_add.py
```

### Handling ouros stderr noise

The ouros CLI writes diagnostic information to stderr:
- `Reading file: <path>`
- `type checking succeeded` or `type checking failed:\n<details>`
- `time taken to run typing: <duration>`
- `success after: <duration>\n<value>` (on success)
- `error after: <duration>\n<exception>` (on error)

The harness **ignores stderr for comparison purposes**. Only stdout is compared.
Stderr is captured and stored in results for debugging.


## Result Format

### JSON Output

```json
{
  "timestamp": "2026-02-18T12:00:00Z",
  "ouros_binary": "target/debug/ouros",
  "cpython_binary": "python3",
  "cpython_version": "3.14.0",
  "ouros_version": "0.0.4",
  "total": 42,
  "passed": 38,
  "failed": 3,
  "errors": 1,
  "skipped": 0,
  "pass_rate": 0.904,
  "results": [
    {
      "snippet": "snippets/arithmetic/int_add.py",
      "category": "arithmetic",
      "description": "Integer addition with various operand types",
      "verdict": "PASS",
      "duration_cpython_ms": 45,
      "duration_ouros_ms": 120,
      "cpython_exit": 0,
      "ouros_exit": 0
    },
    {
      "snippet": "snippets/strings/fstring_basic.py",
      "category": "strings",
      "description": "Basic f-string formatting",
      "verdict": "FAIL",
      "duration_cpython_ms": 38,
      "duration_ouros_ms": 95,
      "cpython_exit": 0,
      "ouros_exit": 0,
      "diff": {
        "stdout": "--- cpython\n+++ ouros\n@@ -3 +3 @@\n-'42'\n+42"
      }
    },
    {
      "snippet": "snippets/edge_cases/recursion_limit.py",
      "category": "edge_cases",
      "description": "Recursion limit should produce error",
      "verdict": "ERROR",
      "duration_cpython_ms": 50,
      "duration_ouros_ms": 0,
      "cpython_exit": 1,
      "ouros_exit": -11,
      "error": "ouros killed by signal 11 (SIGSEGV)"
    }
  ]
}
```

### Terminal Summary

```
============================================
 CPYTHON CONFORMANCE RESULTS
============================================

  PASS  arithmetic/int_add.py
  PASS  arithmetic/float_ops.py
  FAIL  strings/fstring_basic.py
        - cpython: '42'
        + ouros:   42
  PASS  strings/str_concat.py
  ERROR edge_cases/recursion_limit.py
        ouros killed by signal 11

--------------------------------------------
 PASS: 38  FAIL: 3  ERROR: 1  SKIP: 0  Total: 42
 Conformance: 90.4% (38/42)
--------------------------------------------
```


## Normalization Rules

Before comparing stdout, apply these normalizations:

| Rule | Rationale |
|------|-----------|
| Strip trailing whitespace from each line | Minor formatting differences |
| Strip trailing empty lines | Some interpreters add trailing newlines |
| Normalize `\r\n` to `\n` | Cross-platform consistency |
| Replace `0x[0-9a-fA-F]+` in `<object at 0x...>` with `0xADDR` | Memory addresses are non-deterministic |
| Replace `id=[0-9]+` with `id=ID` | Object IDs are non-deterministic |

These match the normalizations already used in `deep_parity_audit.sh`.


## Integration with CI

### GitHub Actions Workflow

```yaml
name: CPython Conformance
on: [push, pull_request]
jobs:
  conformance:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - uses: actions/setup-python@v5
        with:
          python-version: '3.14'

      - name: Build ouros CLI
        run: cargo build -p ouros-cli

      - name: Run conformance harness
        run: |
          python3 tools/cpython-conformance/harness.py \
            --ouros-binary target/debug/ouros \
            --snippets-dir tools/cpython-conformance/snippets \
            --output tools/cpython-conformance/results/latest.json \
            --junit tools/cpython-conformance/results/junit.xml

      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: conformance-results
          path: tools/cpython-conformance/results/

      - name: Check pass rate
        run: |
          RATE=$(python3 -c "import json; d=json.load(open('tools/cpython-conformance/results/latest.json')); print(d['pass_rate'])")
          echo "Pass rate: $RATE"
          python3 -c "import json,sys; d=json.load(open('tools/cpython-conformance/results/latest.json')); sys.exit(0 if d['pass_rate'] >= 0.80 else 1)"
```

### Integration with Existing Tests

The harness is **complementary** to existing infrastructure, not a replacement:

| Tool | Scope | How It Tests | When to Use |
|------|-------|-------------|-------------|
| `datatest_runner.rs` | 527 fixtures in `test_cases/` | In-process via Rust API + pyo3 | Core VM behavior, return values, exceptions |
| `deep_parity_audit.sh` | 64 parity tests in `playground/parity_tests/` | Subprocess (`cargo run` + `python3`) | Stdlib function coverage |
| **This harness** | Snippets in `tools/cpython-conformance/snippets/` | Subprocess (clean capture, structured output) | Protocol-level conformance, CI dashboards |

The harness can optionally be wired into `make`:
```makefile
.PHONY: conformance
conformance: ## Run CPython conformance harness
	cargo build -p ouros-cli
	python3 tools/cpython-conformance/harness.py \
		--ouros-binary target/debug/ouros \
		--snippets-dir tools/cpython-conformance/snippets
```


## CLI Interface

```
usage: harness.py [-h] [--ouros-binary PATH] [--cpython-binary PATH]
                  [--snippets-dir DIR] [--output JSON_PATH]
                  [--junit XML_PATH] [--timeout SECONDS]
                  [--filter PATTERN] [--category CATEGORY]
                  [--verbose] [--fail-fast]

CPython Conformance Differential Test Harness

options:
  --ouros-binary PATH    Path to ouros binary (default: target/debug/ouros)
  --cpython-binary PATH  Path to CPython binary (default: python3)
  --snippets-dir DIR     Directory containing test snippets (default: snippets/)
  --output JSON_PATH     Write JSON results to file
  --junit XML_PATH       Write JUnit XML results (for CI)
  --timeout SECONDS      Per-snippet timeout (default: 10)
  --filter PATTERN       Only run snippets matching glob pattern
  --category CATEGORY    Only run snippets in this category
  --verbose              Show full stdout/stderr on failures
  --fail-fast            Stop on first failure
```


## Implementation Phases

### Phase 1: Foundation
**Files to create:**
- `tools/cpython-conformance/harness.py` -- Main runner (argparse, subprocess, comparison, reporting)

**Acceptance:**
- [ ] Can run a single snippet through both interpreters
- [ ] Produces correct PASS/FAIL/ERROR verdict
- [ ] Prints terminal summary

**Estimated effort:** Small (single file, ~300 lines)

### Phase 2: Snippet Library
**Files to create:**
- `tools/cpython-conformance/snippets/arithmetic/*.py` (5-10 snippets)
- `tools/cpython-conformance/snippets/strings/*.py` (5-10 snippets)
- `tools/cpython-conformance/snippets/collections/*.py` (5-10 snippets)
- `tools/cpython-conformance/snippets/builtins/*.py` (5-10 snippets)
- `tools/cpython-conformance/snippets/control_flow/*.py` (5-10 snippets)

**Acceptance:**
- [ ] At least 30 snippets covering core protocols
- [ ] All snippets follow the format specification
- [ ] All snippets are deterministic

**Estimated effort:** Medium (curating good test cases)

### Phase 3: Structured Output
**Files to modify:**
- `tools/cpython-conformance/harness.py` -- Add JSON output, JUnit XML

**Acceptance:**
- [ ] JSON output validates against schema
- [ ] JUnit XML parseable by CI tools

**Estimated effort:** Small

### Phase 4: CI Integration
**Files to create/modify:**
- `.github/workflows/conformance.yml` (or add job to existing workflow)
- `Makefile` -- Add `conformance` target

**Acceptance:**
- [ ] CI runs on push/PR
- [ ] Pass rate threshold enforced
- [ ] Results uploaded as artifacts

**Estimated effort:** Small

### Phase 5: Expansion
**Ongoing:**
- Add more snippets as ouros gains features
- Add categories: `classes/`, `stdlib/`, `async/`, `edge_cases/`
- Extract failing snippets into ouros issue tracker

**Estimated effort:** Ongoing


## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| ouros CLI stderr noise pollutes comparison | FAIL false positives | Compare only stdout; stderr is diagnostic |
| ouros binary not built before harness runs | All tests ERROR | Harness checks binary exists, prints helpful error |
| Non-deterministic output (hash, addresses) | Flaky tests | Normalization rules + `PYTHONHASHSEED=0` |
| ouros CLI prints return value to stderr | Confusion about what is "output" | Document clearly: stdout = print(), stderr = diagnostics |
| `cargo build` fails due to pyo3 | Cannot build binary | `cargo build -p ouros-cli` avoids pyo3 (it is only a dev-dependency of ouros, not a dependency of ouros-cli) |
| Snippet tests duplicate existing test_cases/ | Wasted effort | Snippets focus on print()-based observable behavior; test_cases focus on return values/exceptions |
| Timeout too short for ouros (includes type-checking) | False ERROR | Default 10s timeout, configurable via flag |


## Open Questions

- [ ] Should the harness also compare stderr for error cases (exception messages)? Initial design says no -- only stdout. But for snippets that `except` and `print()` the error, this is captured in stdout naturally.
- [ ] Should we auto-generate some snippets from existing `test_cases/` fixtures? Many test_cases already use `assert` which produces no stdout. Converting them would require wrapping in `print()`.
- [ ] Should there be a `# timeout: N` header for individual snippets that need more time?


## Success Criteria

1. Harness script runs to completion and produces a correct terminal summary
2. JSON output contains all snippet results with accurate verdicts
3. At least 30 initial snippets covering arithmetic, strings, collections, builtins, and control flow
4. All snippets produce identical output on CPython 3.13+ and CPython 3.14
5. Pass rate against ouros is measurable and trackable over time
6. CI integration gates on a configurable pass-rate threshold


## Relationship to Existing Infrastructure

```
                    ┌─────────────────────────────────┐
                    │      ouros test infrastructure   │
                    │                                  │
                    │  datatest_runner.rs              │
                    │  ├── 527 fixtures in test_cases/ │
                    │  ├── In-process (Rust API + pyo3)│
                    │  ├── Tests: return values,       │
                    │  │   exceptions, tracebacks,     │
                    │  │   ref counts                  │
                    │  └── Verdicts via assertion       │
                    │                                  │
                    │  deep_parity_audit.sh            │
                    │  ├── 64 tests in parity_tests/   │
                    │  ├── Subprocess (cargo run)       │
                    │  ├── Tests: stdlib print output   │
                    │  └── Verdicts: MATCH/DIFF/MFAIL  │
                    │                                  │
                    │  cpython-conformance/harness.py  │  <-- NEW
                    │  ├── N snippets in snippets/     │
                    │  ├── Subprocess (clean binary)    │
                    │  ├── Tests: print()-observable    │
                    │  │   behavior by protocol         │
                    │  ├── Structured JSON + JUnit      │
                    │  └── Verdicts: PASS/FAIL/ERROR   │
                    └─────────────────────────────────┘
```

The new harness fills the gap between:
- `datatest_runner` (precise but in-process, not testing the real CLI binary)
- `deep_parity_audit.sh` (tests the CLI but unstructured output, no CI integration, heavy stderr filtering)
