# Tests for sys module types

import sys

# === Verify type() returns _io.TextIOWrapper for stdout/stderr ===
assert str(type(sys.stdout)) == "<class '_io.TextIOWrapper'>", 'type(stdout) is _io.TextIOWrapper'
assert str(type(sys.stderr)) == "<class '_io.TextIOWrapper'>", 'type(stderr) is _io.TextIOWrapper'
assert type(sys.stdout).__name__ == 'TextIOWrapper', '__name__ for stdout type'
assert type(sys.stderr).__name__ == 'TextIOWrapper', '__name__ for stderr type'
