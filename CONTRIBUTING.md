# Contributing to Ouros

Thank you for your interest in contributing to Ouros! This guide covers the development workflow, testing, and how to add new stdlib modules.

## Getting Started

### Prerequisites

- **Rust** (stable, latest) via [rustup](https://rustup.rs/)
- **Python 3.10+** (for Python tests and tooling)
- **Node.js 22+** (for JS bindings, optional)
- **uv** (Python package manager): `curl -LsSf https://astral.sh/uv/install.sh | sh`

### Setup

```bash
make install        # Install all dependencies (Rust, Python, JS)
make dev-py         # Build the Python package for development
```

## Development Workflow

### Building

Do **not** run `cargo build` or `cargo run` directly — it will fail due to Python binding issues. Use `make` commands instead:

```bash
make dev-py             # Build Python package (debug)
make dev-py-release     # Build Python package (release)
make dev-js             # Build JS package (debug)
```

### Running Code

```bash
cargo run -- path/to/script.py    # Run a Python file with Ouros
```

Use the `playground/` directory for experiments:

```bash
mkdir -p playground
python3 playground/test.py         # Run with CPython
cargo run -- playground/test.py    # Run with Ouros
```

### Testing

```bash
# Primary test commands
make test                    # Run all Rust tests
make test-ref-count-panic    # Run tests with ref-count checking (recommended)
make test-cases              # Run test_cases only (fastest for iteration)
make test-py                 # Build + run Python tests
make test-js                 # Build + run JS tests
make pytest                  # Run Python tests (assumes already built)

# Run a specific test
cargo test -p ouros --test datatest_runner --features ref-count-panic str__ops

# Linting
make lint-rs          # Clippy + import checks
make lint-py          # Ruff
make format-rs        # cargo fmt
```

### Test File Structure

Tests live in `crates/ouros/test_cases/`. Each file is a Python script that runs on both Ouros and CPython. Use `assert` statements with descriptive messages:

```python
# === String concatenation ===
assert 'hello' + ' world' == 'hello world', 'basic string concat'
assert '' + 'x' == 'x', 'empty + string'

# === Edge cases ===
x = 'a' * 100
assert len(x) == 100, 'string multiplication length'
```

**Consolidate related tests into single files** rather than creating many small files. Name files by feature: `str__ops.py`, `list__methods.py`, etc.

### Parity Tests

Parity tests compare Ouros output against CPython to verify behavioral correctness:

```bash
# Run all parity tests
bash playground/deep_parity_audit.sh

# Run a specific module
bash playground/deep_parity_audit.sh math
```

Results:
- **MATCH** - Identical output (goal)
- **MFAIL** - Ouros errors, CPython succeeds (fix the implementation)
- **DIFF** - Both succeed but output differs (fix the output)
- **BFAIL** - Both error (usually fine)

**Do not modify files in `playground/parity_tests/`.** These are ground truth. If a parity test fails, fix the Ouros implementation.

### Before Submitting

```bash
make format-rs                     # Format Rust code
make lint-rs                       # Check clippy
make lint-py                       # Check Python code
make test-ref-count-panic          # Run tests with refcount checking
bash playground/deep_parity_audit.sh  # Check parity hasn't regressed
```

## Adding Stdlib Modules

Ouros implements Python's standard library in Rust. Two tools help with this:

### 1. Check what's missing: `stdlib_api_diff.py`

Compare Ouros's implementation of a module against CPython:

```bash
python3 tools/stdlib_api_diff.py math
python3 tools/stdlib_api_diff.py --all          # Compare all modules
python3 tools/stdlib_api_diff.py --json math    # JSON output
```

This shows which functions, classes, and constants are missing or have different signatures.

### 2. Generate test scaffolding: `stdlib_add.py`

Auto-generate test cases by running functions against CPython and capturing expected outputs:

```bash
python3 tools/stdlib_add.py textwrap           # Generate tests for textwrap
python3 tools/stdlib_add.py textwrap --dry-run  # Preview without writing
```

This creates test files in `crates/ouros/test_cases/` with oracle-verified expected values.

### Module Implementation Workflow

1. **Identify the module** to implement or improve:
   ```bash
   python3 tools/stdlib_api_diff.py --all  # See coverage gaps
   ```

2. **Check CPython behavior** for the functions you'll implement:
   ```bash
   python3 tools/stdlib_api_diff.py <module>
   ```

3. **Write the Rust implementation** in `crates/ouros/src/stdlib/`

4. **Add tests** in `crates/ouros/test_cases/`:
   ```bash
   python3 tools/stdlib_add.py <module>  # Auto-generate test scaffolding
   ```

5. **Add a parity test** in `playground/parity_tests/test_<module>.py` that exercises the key functionality

6. **Verify**:
   ```bash
   make test-cases
   bash playground/deep_parity_audit.sh <module>
   ```

## Code Style

### Rust
- Use `impl Trait` syntax over `<T: Trait>` generics
- Import types directly (`use std::borrow::Cow;`) rather than using paths
- Use `#[expect(...)]` instead of `#[allow(...)]` for lint suppressions
- All structs, enums, and functions should have docstrings
- Follow "newspaper style": public functions at the top, helpers below
- Run `make format-rs && make lint-rs` before committing

### Python (tests)
- Prefer single quotes for strings
- Use `assert` with descriptive messages
- Don't use `# noqa` in test files (add exceptions to `pyproject.toml` instead)
- Don't mark tests as `xfail` — fix the behavior instead

## Benchmarks

```bash
make bench          # Run benchmarks (release profile)
make dev-bench      # Run benchmarks (dev profile, faster iteration)
```

## Architecture Overview

Ouros is a bytecode VM that:
1. Parses Python using [Ruff](https://github.com/astral-sh/ruff)'s parser
2. Compiles to a custom bytecode format
3. Executes in a sandboxed runtime with manual reference counting

Key crate structure:
- `crates/ouros/` - Core interpreter (parser, compiler, VM, stdlib)
- `crates/ouros-python/` - Python bindings (PyO3)
- `crates/ouros-js/` - JavaScript bindings (napi-rs)
- `crates/ouros-cli/` - CLI binary
- `crates/ouros-mcp/` - MCP server integration
- `crates/ouros-type-checking/` - Type checker (via Ruff's ty)

## Security

Ouros executes untrusted code. Any contribution must not introduce ways to:
- Access the host filesystem
- Execute system commands
- Access network resources
- Escape the sandbox in any way

See `CLAUDE.md` for the full security policy.
