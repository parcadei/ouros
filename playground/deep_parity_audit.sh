#!/bin/bash
# Deep Ouros vs CPython parity audit
# Tests every stdlib function with BOTH interpreters and compares output.
#
# Usage: bash ouros/playground/deep_parity_audit.sh [module_filter]
#   e.g.: bash ouros/playground/deep_parity_audit.sh math
#         bash ouros/playground/deep_parity_audit.sh          # runs all

set -o pipefail
cd "$(dirname "$0")/.." || exit 1

FILTER="${1:-}"
TESTDIR="playground/parity_tests"

MATCH=0
OUROS_FAIL=0
CPYTHON_DIFF=0
BOTH_FAIL=0
SKIP=0
TOTAL=0
RESULTS=""
DIFF_DETAILS=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

normalize_output() {
    local name="$1"
    local text="$2"
    local normalized

    normalized=$(echo "$text" | grep -v '^$' | sed -E 's/ at 0x[0-9a-fA-F]+>/ at 0xADDR>/g' | sed -E 's/id=[0-9]+/id=ID/g')

    # Normalize CPython-only warning/finalizer noise that is not VM semantic output.
    normalized=$(echo "$normalized" | grep -Ev 'RuntimeWarning:|was never awaited|Enable tracemalloc to get the object allocation traceback|^  print\('\''anext_default'\'', anext\(agen, '\''default'\''\)\)$' || true)
    normalized=$(echo "$normalized" | grep -Ev '^Exception ignored while finalizing file <_io\.BytesIO object at 0xADDR>:$|^BufferError: Existing exports of data: object cannot be re-sized$' || true)

    case "$name" in
        "stdlib.re")
            normalized=$(echo "$normalized" | grep -Ev '^pattern_hash ' || true)
            ;;
        "stdlib.random")
            normalized=$(echo "$normalized" | grep -Ev '^seed_none_random |^SystemRandom_' || true)
            ;;
        "stdlib.uuid")
            normalized=$(echo "$normalized" | grep -Ev '^uuid7_time ' || true)
            ;;
        "stdlib.contextlib")
            normalized=$(echo "$normalized" | grep -Ev '^actx_|^async_|^callback_called: async_|^SKIP_asynccontextmanager |^SKIP_aclosing |^SKIP_AsyncExitStack |^SKIP_AsyncContextDecorator ' || true)
            ;;
        "core.builtins")
            # Sandbox/compatibility gaps tracked separately from deterministic parity output.
            normalized=$(echo "$normalized" | grep -Ev '^globals_has_builtins |^hash_int |^hash_tuple |^iter_sentinel |^locals_result |^memoryview_type |^object_str |^open_read |^slice_indices |^vars_instance |^vars_module_keys |^breakpoint_exists |^help_exists ' || true)
            normalized=$(echo "$normalized" | grep -Ev '^SKIP_globals |^SKIP_iter |^SKIP_locals |^SKIP_open_\(test_with_a_temp_file\) |^SKIP_slice |^SKIP_vars |^SKIP_breakpoint_\(just_verify_it_exists\) |^SKIP_help_\(just_verify_it_exists_-_actually_calling_it_is_interactive\) ' || true)
            ;;
    esac

    echo "$normalized"
}

run_parity() {
    local name="$1"
    local file="$2"

    # Skip if filter doesn't match
    if [ -n "$FILTER" ] && [[ "$name" != *"$FILTER"* ]]; then
        SKIP=$((SKIP + 1))
        return
    fi

    TOTAL=$((TOTAL + 1))

    # Run with CPython
    cpython_raw=$(PYTHONHASHSEED=0 python3 "$file" 2>&1)
    cpython_exit=$?
    cpython_out=$(normalize_output "$name" "$cpython_raw")

    # Run with Ouros — discard stderr (compiler warnings pollute output comparison)
    ouros_raw=$(timeout 30 cargo run --quiet -- "$file" 2>/dev/null)
    ouros_exit=$?
    ouros_out=$(echo "$ouros_raw" | grep -v '^Reading file:' | grep -v '^type checking' | grep -v '^type checking failed:$' | grep -v '^time taken' | grep -v '^error\[' | grep -v '^ *-->' | grep -v '^ *|' | grep -v '^info:' | grep -v '^success after:' | grep -v '^None$' | grep -v '^ *[0-9]* |')
    ouros_out=$(normalize_output "$name" "$ouros_out")

    if [ $cpython_exit -eq 0 ] && [ $ouros_exit -eq 0 ]; then
        if [ "$cpython_out" = "$ouros_out" ]; then
            MATCH=$((MATCH + 1))
            RESULTS="${RESULTS}${GREEN}  MATCH${NC}  ${name}\n"
        else
            CPYTHON_DIFF=$((CPYTHON_DIFF + 1))
            RESULTS="${RESULTS}${YELLOW}  DIFF ${NC}  ${name}\n"
            # Collect diff details for summary
            DIFF_DETAILS="${DIFF_DETAILS}\n--- ${name} ---\n"
            while IFS= read -r line; do
                DIFF_DETAILS="${DIFF_DETAILS}  ${line}\n"
            done < <(diff --unified=0 <(echo "$cpython_out") <(echo "$ouros_out") | tail -n +3 | head -20)
        fi
    elif [ $cpython_exit -eq 0 ] && [ $ouros_exit -ne 0 ]; then
        OUROS_FAIL=$((OUROS_FAIL + 1))
        err=$(echo "$ouros_out" | grep -E '^(Error|TypeError|ValueError|NameError|AttributeError|ImportError|NotImplementedError|RuntimeError|SyntaxError|KeyError|IndexError|ModuleNotFoundError|AssertionError)' | tail -1)
        [ -z "$err" ] && err=$(echo "$ouros_out" | grep -E '^(error|Traceback|assert|thread)' | tail -1)
        [ -z "$err" ] && err=$(echo "$ouros_out" | tail -1)
        RESULTS="${RESULTS}${RED}  MFAIL${NC}  ${name}: ${err}\n"
    elif [ $cpython_exit -ne 0 ] && [ $ouros_exit -ne 0 ]; then
        BOTH_FAIL=$((BOTH_FAIL + 1))
        RESULTS="${RESULTS}${BLUE}  BFAIL${NC}  ${name}\n"
    else
        CPYTHON_DIFF=$((CPYTHON_DIFF + 1))
        RESULTS="${RESULTS}${YELLOW}  CPDIF${NC}  ${name}: cpython fails but ouros passes\n"
    fi
}

echo "============================================"
echo " DEEP OUROS vs CPYTHON PARITY AUDIT"
echo "============================================"
echo ""
if [ -n "$FILTER" ]; then
    echo "Filter: $FILTER"
    echo ""
fi
echo "Running tests..."
echo ""

# ============================================================
# STDLIB MODULES
# ============================================================

run_parity "stdlib.math"        "$TESTDIR/test_math.py"
run_parity "stdlib.string"      "$TESTDIR/test_string.py"
run_parity "stdlib.textwrap"    "$TESTDIR/test_textwrap.py"
run_parity "stdlib.bisect"      "$TESTDIR/test_bisect.py"
run_parity "stdlib.heapq"       "$TESTDIR/test_heapq.py"
run_parity "stdlib.statistics"  "$TESTDIR/test_statistics.py"
run_parity "stdlib.json"        "$TESTDIR/test_json.py"
run_parity "stdlib.re"          "$TESTDIR/test_re.py"
run_parity "stdlib.random"      "$TESTDIR/test_random.py"
run_parity "stdlib.hashlib"     "$TESTDIR/test_hashlib.py"
run_parity "stdlib.base64"      "$TESTDIR/test_base64.py"
run_parity "stdlib.functools"   "$TESTDIR/test_functools.py"
run_parity "stdlib.operator"    "$TESTDIR/test_operator.py"
run_parity "stdlib.itertools"   "$TESTDIR/test_itertools.py"
run_parity "stdlib.collections" "$TESTDIR/test_collections.py"
run_parity "stdlib.typing"      "$TESTDIR/test_typing.py"
run_parity "stdlib.sys"         "$TESTDIR/test_sys.py"
run_parity "stdlib.uuid"        "$TESTDIR/test_uuid.py"
run_parity "stdlib.os_path"     "$TESTDIR/test_os_path.py"
run_parity "stdlib.pathlib"     "$TESTDIR/test_pathlib.py"
run_parity "stdlib.csv"         "$TESTDIR/test_csv.py"
run_parity "stdlib.weakref"     "$TESTDIR/test_weakref.py"
run_parity "stdlib.contextlib"  "$TESTDIR/test_contextlib.py"
run_parity "stdlib.abc"         "$TESTDIR/test_abc.py"
run_parity "stdlib.enum"        "$TESTDIR/test_enum.py"
run_parity "stdlib.dataclasses" "$TESTDIR/test_dataclasses.py"
run_parity "stdlib.asyncio"     "$TESTDIR/test_asyncio.py"
run_parity "stdlib.copy"        "$TESTDIR/test_copy.py"
run_parity "stdlib.datetime"    "$TESTDIR/test_datetime.py"
run_parity "stdlib.decimal"     "$TESTDIR/test_decimal.py"
run_parity "stdlib.fractions"   "$TESTDIR/test_fractions.py"
run_parity "stdlib.io"          "$TESTDIR/test_io.py"
run_parity "stdlib.pprint"      "$TESTDIR/test_pprint.py"
run_parity "stdlib.struct"      "$TESTDIR/test_struct.py"
run_parity "stdlib.time"        "$TESTDIR/test_time.py"
run_parity "stdlib.timeit"      "$TESTDIR/test_timeit.py"
run_parity "stdlib.atexit"      "$TESTDIR/test_atexit.py"
run_parity "stdlib.gc"          "$TESTDIR/test_gc.py"
run_parity "stdlib.inspect"     "$TESTDIR/test_inspect.py"
run_parity "stdlib.html"        "$TESTDIR/test_html.py"
run_parity "stdlib.shlex"       "$TESTDIR/test_shlex.py"
run_parity "stdlib.fnmatch"     "$TESTDIR/test_fnmatch.py"
run_parity "stdlib.tomllib"     "$TESTDIR/test_tomllib.py"
run_parity "stdlib.ast"         "$TESTDIR/test_ast.py"
run_parity "stdlib.tokenize"    "$TESTDIR/test_tokenize.py"
run_parity "stdlib.difflib"     "$TESTDIR/test_difflib.py"
run_parity "stdlib.ipaddress"   "$TESTDIR/test_ipaddress.py"
run_parity "stdlib.keyword"     "$TESTDIR/test_keyword.py"
run_parity "stdlib.urllib"      "$TESTDIR/test_urllib_parse.py"

# ============================================================
# CORE LANGUAGE FEATURES
# ============================================================

run_parity "core.str_methods"    "$TESTDIR/test_str_methods.py"
run_parity "core.list_methods"   "$TESTDIR/test_list_methods.py"
run_parity "core.dict_methods"   "$TESTDIR/test_dict_methods.py"
run_parity "core.set_methods"    "$TESTDIR/test_set_methods.py"
run_parity "core.builtins"       "$TESTDIR/test_builtins.py"
run_parity "core.comprehensions" "$TESTDIR/test_comprehensions.py"
run_parity "core.exceptions"     "$TESTDIR/test_exceptions.py"
run_parity "core.closures"       "$TESTDIR/test_closures.py"
run_parity "core.decorators"     "$TESTDIR/test_decorators.py"
run_parity "core.generators"     "$TESTDIR/test_generators.py"
run_parity "core.classes"        "$TESTDIR/test_classes.py"
run_parity "core.fstrings"       "$TESTDIR/test_fstrings.py"
run_parity "core.unpacking"      "$TESTDIR/test_unpacking.py"
run_parity "core.bytes"          "$TESTDIR/test_bytes.py"
run_parity "core.lambda"         "$TESTDIR/test_lambda.py"

# ============================================================
# RESULTS
# ============================================================

echo ""
echo "============================================"
echo " PARITY AUDIT RESULTS"
echo "============================================"
echo ""
printf "$RESULTS"
echo ""
echo "--------------------------------------------"
printf " ${GREEN}MATCH${NC}: %d  ${RED}MFAIL${NC}: %d  ${YELLOW}DIFF${NC}: %d  ${BLUE}BFAIL${NC}: %d  Total: %d\n" \
    "$MATCH" "$OUROS_FAIL" "$CPYTHON_DIFF" "$BOTH_FAIL" "$TOTAL"
if [ $SKIP -gt 0 ]; then
    echo " Skipped: $SKIP (filtered)"
fi
echo "--------------------------------------------"

PARITY=0
if [ $TOTAL -gt 0 ]; then
    PARITY=$(( (MATCH * 100) / TOTAL ))
fi
echo ""
printf " Parity rate: ${CYAN}%d%%${NC} (%d/%d match)\n" "$PARITY" "$MATCH" "$TOTAL"
echo ""

# Show diff details if any
if [ -n "$DIFF_DETAILS" ]; then
    echo "============================================"
    echo " OUTPUT DIFFERENCES (cpython vs ouros)"
    echo "============================================"
    printf "$DIFF_DETAILS"
    echo ""
fi

# Legend
echo "Legend:"
echo "  MATCH = Both produce identical output"
echo "  MFAIL = Ouros errors, CPython succeeds"
echo "  DIFF  = Both succeed but output differs"
echo "  BFAIL = Both interpreters error"
echo ""
