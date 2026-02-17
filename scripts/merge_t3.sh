#!/usr/bin/env bash
set -eo pipefail

# T3 Stdlib Integration Script
# Merges 14 module worktrees into the main conty codebase
#
# What it does:
# 1. Copies 14 module .rs files + 14 test .py files
# 2. Appends StaticStrings variants to intern.rs
# 3. Wires all modules into mod.rs (6 insertion points)
# 4. Adds uuid + rand deps to Cargo.toml

CONTY="/Users/cosimo/.opc-dev/conty"
MODULES_DIR="$CONTY/crates/conty/src/modules"
TESTS_DIR="$CONTY/crates/conty/test_cases"
INTERN="$CONTY/crates/conty/src/intern.rs"
MOD_RS="$MODULES_DIR/mod.rs"
CARGO="$CONTY/crates/conty/Cargo.toml"

echo "=== Step 1: Copy module files ==="

copy_module() {
  local module="$1"
  local file="$2"
  local src="/tmp/conty-$module/crates/conty/src/modules/$file"
  if [ -f "$src" ]; then
    cp "$src" "$MODULES_DIR/$file"
    echo "  Copied $file"
  else
    echo "  ERROR: $src not found!"
    exit 1
  fi
  # Copy test file
  local test_src="/tmp/conty-$module/crates/conty/test_cases/import__${module}.py"
  if [ -f "$test_src" ]; then
    cp "$test_src" "$TESTS_DIR/import__${module}.py"
    echo "  Copied import__${module}.py"
  else
    echo "  ERROR: $test_src not found!"
    exit 1
  fi
}

copy_module abc abc.rs
copy_module hashlib hashlib.rs
copy_module contextlib contextlib.rs
copy_module statistics statistics.rs
copy_module string string_mod.rs
copy_module textwrap textwrap.rs
copy_module uuid uuid_mod.rs
copy_module base64 base64_mod.rs
copy_module random random_mod.rs
copy_module enum enum_mod.rs
copy_module csv csv_mod.rs
copy_module operator operator.rs
copy_module bisect bisect.rs
copy_module heapq heapq.rs

echo ""
echo "=== Step 3: Append StaticStrings to intern.rs ==="

# We insert before the closing `}` of the StaticStrings enum (line 809)
# Strategy: replace the closing `}` with new variants + `}`

# Build the new variants block
INTERN_ADDITIONS=$(cat <<'VARIANTS'

    // ==========================
    // abc module strings
    #[strum(serialize = "abc")]
    Abc,
    #[strum(serialize = "ABC")]
    AbcABC,
    #[strum(serialize = "abstractmethod")]
    AbcAbstractmethod,

    // ==========================
    // hashlib module strings
    #[strum(serialize = "hashlib")]
    Hashlib,
    #[strum(serialize = "md5")]
    HlMd5,
    #[strum(serialize = "sha256")]
    HlSha256,

    // ==========================
    // contextlib module strings
    #[strum(serialize = "contextlib")]
    Contextlib,
    #[strum(serialize = "suppress")]
    ClSuppress,
    #[strum(serialize = "contextmanager")]
    ClContextmanager,

    // ==========================
    // statistics module strings
    #[strum(serialize = "statistics")]
    Statistics,
    #[strum(serialize = "mean")]
    StatMean,
    #[strum(serialize = "median")]
    StatMedian,
    #[strum(serialize = "mode")]
    StatMode,
    #[strum(serialize = "stdev")]
    StatStdev,
    #[strum(serialize = "variance")]
    StatVariance,

    // ==========================
    // string module strings
    #[strum(serialize = "string")]
    StringMod,
    #[strum(serialize = "ascii_lowercase")]
    StrAsciiLowercase,
    #[strum(serialize = "ascii_uppercase")]
    StrAsciiUppercase,
    #[strum(serialize = "ascii_letters")]
    StrAsciiLetters,
    #[strum(serialize = "digits")]
    StrDigits,
    #[strum(serialize = "hexdigits")]
    StrHexdigits,
    #[strum(serialize = "octdigits")]
    StrOctdigits,
    #[strum(serialize = "punctuation")]
    StrPunctuation,
    #[strum(serialize = "whitespace")]
    StrWhitespace,
    #[strum(serialize = "printable")]
    StrPrintable,

    // ==========================
    // textwrap module strings
    #[strum(serialize = "textwrap")]
    Textwrap,
    #[strum(serialize = "dedent")]
    TwDedent,
    #[strum(serialize = "indent")]
    TwIndent,
    #[strum(serialize = "fill")]
    TwFill,
    #[strum(serialize = "wrap")]
    TwWrap,

    // ==========================
    // uuid module strings
    #[strum(serialize = "uuid")]
    Uuid,
    #[strum(serialize = "uuid4")]
    UuidUuid4,

    // ==========================
    // base64 module strings
    #[strum(serialize = "base64")]
    Base64,
    #[strum(serialize = "b64encode")]
    B64Encode,
    #[strum(serialize = "b64decode")]
    B64Decode,

    // ==========================
    // random module strings
    #[strum(serialize = "random")]
    Random,
    #[strum(serialize = "randint")]
    RdRandint,
    #[strum(serialize = "choice")]
    RdChoice,
    #[strum(serialize = "shuffle")]
    RdShuffle,
    #[strum(serialize = "seed")]
    RdSeed,

    // ==========================
    // enum module strings
    #[strum(serialize = "enum")]
    EnumMod,
    #[strum(serialize = "Enum")]
    EnEnum,
    #[strum(serialize = "IntEnum")]
    EnIntEnum,

    // ==========================
    // csv module strings
    #[strum(serialize = "csv")]
    Csv,
    #[strum(serialize = "reader")]
    CsvReader,

    // ==========================
    // operator module strings
    // NOTE: We reuse existing variants for "add", "sub", "abs", "index" since they have
    // the same serialize strings. The operator module uses the same StaticStrings variants
    // for these function names.
    #[strum(serialize = "abs")]
    Abs,
    #[strum(serialize = "operator")]
    Operator,
    #[strum(serialize = "mul")]
    OperatorMul,
    #[strum(serialize = "truediv")]
    OperatorTruediv,
    #[strum(serialize = "floordiv")]
    OperatorFloordiv,
    #[strum(serialize = "mod")]
    OperatorMod,
    #[strum(serialize = "neg")]
    OperatorNeg,
    #[strum(serialize = "eq")]
    OperatorEq,
    #[strum(serialize = "ne")]
    OperatorNe,
    #[strum(serialize = "lt")]
    OperatorLt,
    #[strum(serialize = "le")]
    OperatorLe,
    #[strum(serialize = "gt")]
    OperatorGt,
    #[strum(serialize = "ge")]
    OperatorGe,
    #[strum(serialize = "not_")]
    OperatorNot,
    #[strum(serialize = "truth")]
    OperatorTruth,
    #[strum(serialize = "getitem")]
    OperatorGetitem,
    #[strum(serialize = "contains")]
    OperatorContains,

    // ==========================
    // bisect module strings
    #[strum(serialize = "bisect")]
    Bisect,
    #[strum(serialize = "bisect_left")]
    BisectLeft,
    #[strum(serialize = "bisect_right")]
    BisectRight,
    #[strum(serialize = "insort_left")]
    InsortLeft,
    #[strum(serialize = "insort_right")]
    InsortRight,
    #[strum(serialize = "insort")]
    Insort,

    // ==========================
    // heapq module strings
    #[strum(serialize = "heapq")]
    Heapq,
    #[strum(serialize = "heappush")]
    HqHeappush,
    #[strum(serialize = "heappop")]
    HqHeappop,
    #[strum(serialize = "heapify")]
    HqHeapify,
    #[strum(serialize = "nlargest")]
    HqNlargest,
    #[strum(serialize = "nsmallest")]
    HqNsmallest,
VARIANTS
)

# Find the closing brace of StaticStrings enum and insert before it
# Line 809 is `}` closing the enum. We insert before it.
python3 -c "
import sys
lines = open('$INTERN').readlines()
# Find the enum closing brace (line 809, 0-indexed 808)
# Look for pattern: line after DcField, that is just '}'
insert_idx = None
for i in range(len(lines)-1, -1, -1):
    if lines[i].strip() == '}' and i > 700:
        # Check if previous non-empty line is a variant
        for j in range(i-1, -1, -1):
            if lines[j].strip():
                if lines[j].strip().endswith(',') or 'DcField' in lines[j]:
                    insert_idx = i
                break
        if insert_idx:
            break

if insert_idx is None:
    print('ERROR: Could not find StaticStrings enum closing brace', file=sys.stderr)
    sys.exit(1)

additions = '''$INTERN_ADDITIONS'''
lines.insert(insert_idx, additions + '\n')
with open('$INTERN', 'w') as f:
    f.writelines(lines)
print(f'  Inserted T3 variants before line {insert_idx + 1}')
"

echo ""
echo "=== Step 4: Wire modules into mod.rs ==="

# Build the new mod.rs from scratch based on current + additions
python3 << 'PYEOF'
lines = open("/Users/cosimo/.opc-dev/conty/crates/conty/src/modules/mod.rs").readlines()

# Modules WITH Functions types (need all 6 insertion points)
fn_modules = [
    ("abc", "Abc", "abc", "AbcFunctions", "Abc"),
    ("hashlib", "Hashlib", "hashlib", "HashlibFunctions", "Hashlib"),
    ("contextlib", "Contextlib", "contextlib", "ContextlibFunctions", "Contextlib"),
    ("statistics", "Statistics", "statistics", "StatisticsFunctions", "Statistics"),
    ("textwrap", "Textwrap", "textwrap", "TextwrapFunctions", "Textwrap"),
    ("uuid_mod", "Uuid", "uuid_mod", "UuidFunctions", "Uuid"),
    ("base64_mod", "Base64", "base64_mod", "Base64Functions", "Base64"),
    ("random_mod", "Random", "random_mod", "RandomFunctions", "Random"),
    ("csv_mod", "Csv", "csv_mod", "CsvFunctions", "Csv"),
    ("operator", "Operator", "operator", "OperatorFunctions", "Operator"),
    ("bisect", "Bisect", "bisect", "BisectFunctions", "Bisect"),
    ("heapq", "Heapq", "heapq", "HeapqFunctions", "Heapq"),
]

# Modules WITHOUT Functions types (only pub mod + BuiltinModule + from_string_id + create)
const_modules = [
    ("string_mod", "StringMod", "string_mod", "StringMod"),
    ("enum_mod", "Enum", "enum_mod", "EnumMod"),
]

result = []
for line in lines:
    stripped = line.rstrip()

    # 1. pub(crate) mod declarations — insert after "pub(crate) mod weakref;"
    if stripped == "pub(crate) mod weakref;":
        result.append(line)
        for m in fn_modules:
            result.append(f"pub(crate) mod {m[0]};\n")
        for m in const_modules:
            result.append(f"pub(crate) mod {m[0]};\n")
        continue

    # 2. BuiltinModule enum — insert before closing "}"
    #    Detect: line after "Dataclasses,"
    if "Dataclasses," in stripped and "///" in lines[lines.index(line)-1]:
        # This is the doc comment + variant. Just append it and add after
        result.append(line)
        continue
    if stripped == "Dataclasses,":
        result.append(line)
        for m in fn_modules:
            result.append(f"    /// The `{m[2]}` module.\n")
            result.append(f"    {m[1]},\n")
        for m in const_modules:
            result.append(f"    /// The `{m[2]}` module.\n")
            result.append(f"    {m[1]},\n")
        continue

    # 3. from_string_id — insert before "_ => None,"
    if stripped == "_ => None,":
        for m in fn_modules:
            result.append(f"            StaticStrings::{m[4]} => Some(Self::{m[1]}),\n")
        for m in const_modules:
            result.append(f"            StaticStrings::{m[3]} => Some(Self::{m[1]}),\n")
        result.append(line)
        continue

    # 4. create_module — insert before the closing "}" of the match in create()
    #    Detect: "Self::Dataclasses => dataclasses::create_module(heap, interns),"
    if "Self::Dataclasses => dataclasses::create_module" in stripped:
        result.append(line)
        for m in fn_modules:
            result.append(f"            Self::{m[1]} => {m[2]}::create_module(heap, interns),\n")
        for m in const_modules:
            result.append(f"            Self::{m[1]} => {m[2]}::create_module(heap, interns),\n")
        continue

    # 5. ModuleFunctions enum — insert before closing "}"
    #    Detect: "Weakref(weakref::WeakrefFunctions),"
    if "Weakref(weakref::WeakrefFunctions)," in stripped:
        result.append(line)
        for m in fn_modules:
            result.append(f"    {m[1]}({m[2]}::{m[3]}),\n")
        continue

    # 6. Display impl — insert before closing "}"
    #    Detect: 'Self::Weakref(func) => write!(f, "{func}"),'
    if 'Self::Weakref(func) => write!(f, "{func}"),' in stripped:
        result.append(line)
        for m in fn_modules:
            result.append(f'            Self::{m[1]}(func) => write!(f, "{{func}}"),\n')
        continue

    # 7. call() match — insert before closing "}"
    #    Detect: "Self::Weakref(functions) => weakref::call(heap, interns, functions, args),"
    if "Self::Weakref(functions) => weakref::call" in stripped:
        result.append(line)
        for m in fn_modules:
            result.append(f"            Self::{m[1]}(functions) => {m[2]}::call(heap, interns, functions, args),\n")
        continue

    result.append(line)

with open("/Users/cosimo/.opc-dev/conty/crates/conty/src/modules/mod.rs", "w") as f:
    f.writelines(result)

print(f"  Wired {len(fn_modules)} function modules + {len(const_modules)} constant modules into mod.rs")
PYEOF

echo ""
echo "=== Step 5: Add Cargo.toml dependencies ==="

# Add uuid and rand deps after the last dependency line
python3 << 'PYEOF'
lines = open("/Users/cosimo/.opc-dev/conty/crates/conty/Cargo.toml").readlines()
result = []
added = False
for line in lines:
    result.append(line)
    # Insert after smallvec line (last dep)
    if 'smallvec' in line and not added:
        result.append('uuid = { version = "1", features = ["v4"] }\n')
        result.append('rand = "0.8"\n')
        added = True
        print("  Added uuid and rand dependencies")

with open("/Users/cosimo/.opc-dev/conty/crates/conty/Cargo.toml", "w") as f:
    f.writelines(result)
PYEOF

echo ""
echo "=== Step 6: Summary ==="
echo "  Module files: 14 copied"
echo "  Test files: 14 copied"
echo "  intern.rs: T3 StaticStrings appended"
echo "  mod.rs: 14 modules wired"
echo "  Cargo.toml: uuid + rand added"
echo ""
echo "Next steps:"
echo "  cd $CONTY && make format-rs && make lint-rs"
echo "  cargo test -p conty --test datatest_runner --features ref-count-panic"
