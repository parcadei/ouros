# Adversarial Review: CPython Protocol Conformance Test Suite

Generated: 2026-02-18
Reviewer: architect-agent (Opus 4.6)

---

## Pass 1: Protocol Completeness

### Coverage Matrix

| Protocol | Happy Path | NotImplemented/Fallback | TypeError (both NI) | Subclass Priority | MRO Traversal | Verdict |
|----------|:----------:|:----------------------:|:-------------------:|:-----------------:|:-------------:|---------|
| Binary Ops (`__add__` etc.) | YES | YES | YES | YES | YES | **Good** |
| Inplace Ops (`__iadd__` etc.) | YES | YES | YES | PARTIAL | YES | **Good** |
| Comparison (`__eq__`, `__lt__` etc.) | YES | YES | YES | YES (eq only) | MISSING | **Gaps** |
| Unary Ops (`__neg__`, `__bool__` etc.) | YES | N/A | YES (missing) | MISSING | MISSING | **Gaps** |
| Hash | YES | N/A | N/A | N/A | YES | **Good** |
| Attribute Access | YES | YES | N/A | MISSING | MISSING | **Gaps** |
| Descriptor Protocol | YES | N/A | N/A | YES | YES | **OK** |
| Container Protocol | YES | PARTIAL | N/A | MISSING | MISSING | **Gaps** |
| Context Manager | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **ABSENT** |
| Callable Protocol | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **ABSENT** |
| String/Repr/Format | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **ABSENT** |
| Class Creation Hooks | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **ABSENT** |
| Object Lifecycle | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **MISSING** | **ABSENT** |

### Entirely Missing Protocol Categories (no snippets at all)

1. **Context Manager** (`__enter__`/`__exit__`, `__aenter__`/`__aexit__`) -- ZERO snippets
2. **Callable Protocol** (`__call__`, `__new__`/`__init__` interaction) -- ZERO snippets
3. **String/Repr/Format** (`__str__`, `__repr__`, `__format__`) -- ZERO snippets
4. **Class Creation Hooks** (`__init_subclass__`, `__set_name__`, `__prepare__`, `__mro_entries__`, `__instancecheck__`, `__subclasscheck__`) -- ZERO snippets (descriptor `__set_name__` exists in descriptor_protocol but is the only one)
5. **Object Lifecycle** (`__new__`, `__init__`, `__del__`) -- ZERO snippets

---

## Pass 2: Cross-Protocol Interactions

### Missing Cross-Protocol Tests

#### CP-01: `__eq__` affecting `__hash__` [TESTED]
Snippets `hash_eq_without_hash.py` and `hash_subclass_eq_no_hash.py` cover this.
**Gap**: No test for `__eq__` defined in a *mixin* class (diamond with one branch defining __eq__, other defining __hash__).

#### CP-02: `__iadd__` -> `__add__` -> `__radd__` full chain [TESTED]
Snippet `inplace_mixed_types_iadd_notimplemented.py` covers the full chain.
**Gap**: No test for exception raised inside `__iadd__` stopping the chain (analogous to `binary_add_exception_in_add.py`).

#### CP-03: `__bool__` -> `__len__` -> default True chain [TESTED]
Snippets `unary_bool_fallback_to_len.py` and `unary_bool_no_bool_no_len.py` cover this.
**Gap**: No test where `__bool__` raises an exception (should propagate, NOT fall back to `__len__`).

#### CP-04: `__ne__` -> negated `__eq__` fallback [TESTED]
Snippet `compare_ne_notimplemented_falls_to_eq.py` covers this.
**Gap**: No test for `__ne__` where `__eq__` returns a non-bool value -- the negation of a non-bool via `__ne__` fallback is subtle (CPython calls `not` on the `__eq__` result, which invokes `__bool__` on it).

#### CP-05: Descriptor protocol affecting attribute access [PARTIALLY TESTED]
`descriptor_data_vs_instance.py` and `descriptor_nondata_vs_instance.py` exist.
**Gap**: No test for data descriptor `__set__` interacting with `__setattr__`. No test for `__delete__` making something a data descriptor.

#### CP-06: `__contains__` -> `__iter__` -> `__getitem__` fallback chain [PARTIALLY TESTED]
`container_contains_fallback_iter.py` tests `__contains__` -> `__iter__`.
**Gap**: No test for the full chain ending at `__getitem__` (CPython falls back from `__iter__` to `__getitem__` with sequential integer indices starting at 0).

#### CP-07: `__index__` used in subscript calls [NOT TESTED]
ZERO snippets test `__index__`. The dunder-map documents `__index__` handling in `BinarySubscr` (mod.rs:3542).
This is a known bug-prone area per the map.

#### CP-08: `__init_subclass__` interaction with metaclasses [NOT TESTED]
ZERO snippets. Documented in dunder-map (mod.rs:7650).

#### CP-09: `__class_getitem__` [NOT TESTED]
Documented in dunder-map (mod.rs:3457) but no snippet.

#### CP-10: Context manager `__enter__`/`__exit__` with exceptions [NOT TESTED]
The entire context manager protocol is untested.

#### CP-11: `__str__` -> `__repr__` fallback [NOT TESTED]
Documented in dunder-map (call.rs:695) but no snippet.

#### CP-12: `__len__` return type validation [PARTIALLY TESTED]
`container_len_negative_raises.py` tests negative. No test for non-int return from `__len__`.

#### CP-13: `__bool__` return type validation [TESTED]
`unary_bool_returns_nonbool_error.py` exists.
**Gap**: In CPython 3.12+, `__bool__` returning int subclass is allowed (True/False are int subclasses). The test expects TypeError for int return, which may differ from CPython behavior for int 0/1.

#### CP-14: `__hash__` return type validation [NOT TESTED]
`hash_int_return.py` exists but does not test non-int return (should TypeError).

#### CP-15: Comparison subclass priority for ordering ops [PARTIALLY TESTED]
`compare_subclass_eq_priority.py` tests `__eq__` subclass priority.
**Gap**: No test for `__lt__`/`__gt__` subclass reflected priority (e.g., `base < sub` should try `sub.__gt__` first).

---

## Pass 3: Error and Exception Paths

### Missing Error Path Tests

#### EP-01: Dunder raises exception (not NotImplemented) [PARTIALLY TESTED]
`binary_add_exception_in_add.py` tests this for `__add__`.
**Gaps**:
- No test for exception in `__radd__` (should propagate, not retry)
- No test for exception in `__eq__` (should propagate)
- No test for exception in `__iadd__` (should propagate, not fall to `__add__`)
- No test for exception in `__contains__` (should propagate, not fall to `__iter__`)
- No test for exception in `__iter__` (should propagate)
- No test for exception in `__next__` (non-StopIteration should propagate)

#### EP-02: `__del__` during garbage collection [NOT TESTED]
No snippets at all.

#### EP-03: Recursive `__repr__` [NOT TESTED]
No snippet. CPython returns `...` for recursive containers.

#### EP-04: `__getattr__` raising non-AttributeError [NOT TESTED]
Critical: if `__getattr__` raises ValueError, it should propagate, not be swallowed.

#### EP-05: `__init__` returning non-None [NOT TESTED]
CPython raises TypeError. No snippet.

#### EP-06: `__new__` returning wrong type [NOT TESTED]
If `__new__` returns a non-instance, `__init__` should not be called. No snippet.

#### EP-07: `__bool__` raising an exception [NOT TESTED]
Should propagate, NOT fall back to `__len__`. This is a critical distinction.

#### EP-08: `__len__` returning non-int [NOT TESTED]
CPython raises TypeError. No snippet.

#### EP-09: `__hash__` returning non-int [NOT TESTED]
CPython raises TypeError. No snippet.

#### EP-10: `__next__` raising non-StopIteration [NOT TESTED]
Should propagate through for-loop. No snippet.

#### EP-11: `__exit__` return value suppressing exceptions [NOT TESTED]
Entire context manager protocol missing.

---

## Consolidated Missing Snippet List

### HIGH Priority (Known Bug-Prone, Cross-Protocol, or Entire Missing Protocols)

| ID | File | Description | Why HIGH |
|----|------|-------------|----------|
| H01 | `cross_protocol/eq_affects_hash_mixin.py` | `__eq__` in mixin makes class unhashable even when another parent has `__hash__` | Bug #5 in dunder-map |
| H02 | `cross_protocol/ne_fallback_nonbool_eq.py` | `__ne__` falls back to `not __eq__()` when `__eq__` returns non-bool | Known subtle semantics |
| H03 | `cross_protocol/contains_iter_getitem_chain.py` | Full `__contains__` -> `__iter__` -> `__getitem__` fallback | Documented but untested chain |
| H04 | `cross_protocol/index_in_subscript.py` | `__index__` used to convert custom int for `[]` | Bug-prone area per dunder-map |
| H05 | `cross_protocol/bool_exception_no_len_fallback.py` | `__bool__` raises -> NO fallback to `__len__` | Critical error path |
| H06 | `cross_protocol/iadd_exception_no_add_fallback.py` | `__iadd__` raises -> NO fallback to `__add__` | Critical error path (Bug #7) |
| H07 | `cross_protocol/str_repr_fallback.py` | `str()` falls back to `__repr__` when no `__str__` | Missing entire protocol |
| H08 | `context_manager/basic_with.py` | Basic `with` statement dispatching `__enter__`/`__exit__` | Missing entire protocol |
| H09 | `context_manager/exit_suppresses_exception.py` | `__exit__` returning True suppresses exception | Missing critical path |
| H10 | `context_manager/exit_exception_args.py` | `__exit__` receives (type, value, traceback) on exception | Missing critical path |
| H11 | `callable_protocol/call_dunder.py` | `__call__` makes instances callable | Missing entire protocol |
| H12 | `callable_protocol/new_init_interaction.py` | `__new__` then `__init__` called during instantiation | Missing entire protocol |
| H13 | `callable_protocol/init_returns_non_none.py` | `__init__` returning non-None raises TypeError | Missing error path |
| H14 | `callable_protocol/new_returns_wrong_type.py` | `__new__` returning other type skips `__init__` | Missing error path |
| H15 | `string_repr_format/repr_basic.py` | `repr()` calls `__repr__` | Missing entire protocol |
| H16 | `string_repr_format/format_basic.py` | `format()` calls `__format__` | Missing entire protocol |
| H17 | `class_creation/init_subclass.py` | `__init_subclass__` called on subclass creation | Missing entire protocol |
| H18 | `class_creation/class_getitem.py` | `MyClass[int]` calls `__class_getitem__` | Missing entire protocol |
| H19 | `cross_protocol/getattr_raises_non_attributeerror.py` | `__getattr__` raising ValueError propagates | Critical error path |
| H20 | `cross_protocol/radd_exception_propagates.py` | Exception in `__radd__` propagates (not swallowed) | Binary dispatch error path |
| H21 | `cross_protocol/contains_exception_propagates.py` | Exception in `__contains__` propagates (no iter fallback) | Container error path |
| H22 | `cross_protocol/next_non_stopiteration_propagates.py` | Non-StopIteration in `__next__` propagates through for loop | Iterator error path |
| H23 | `container_protocol/container_getitem_iteration_fallback.py` | `for x in obj` falls back to `__getitem__(0), (1), ...` when no `__iter__` | Old-style iteration protocol |
| H24 | `hash_protocol/hash_non_int_return_typeerror.py` | `__hash__` returning non-int raises TypeError | Return type validation |
| H25 | `container_protocol/container_len_non_int_typeerror.py` | `__len__` returning non-int raises TypeError | Return type validation |
| H26 | `comparison/compare_lt_subclass_reflected_priority.py` | `base < sub` tries `sub.__gt__` first (subclass priority for ordering) | Subclass priority gap |

### MEDIUM Priority (Protocol Gaps)

| ID | File | Description |
|----|------|-------------|
| M01 | `string_repr_format/str_fallback_repr.py` | `str(x)` with only `__repr__` defined |
| M02 | `class_creation/instancecheck.py` | `isinstance()` with custom `__instancecheck__` |
| M03 | `class_creation/subclasscheck.py` | `issubclass()` with custom `__subclasscheck__` |
| M04 | `class_creation/prepare.py` | `__prepare__` provides class namespace |
| M05 | `object_lifecycle/del_invocation.py` | `__del__` called on `del` / garbage collection |
| M06 | `context_manager/nested_with.py` | Nested `with` statements |
| M07 | `context_manager/exit_reraises.py` | `__exit__` returning False does NOT suppress |
| M08 | `descriptor_protocol/descriptor_delete_makes_data.py` | `__delete__` presence makes descriptor "data" |
| M09 | `comparison/compare_ordering_mro.py` | Ordering comparison with MRO traversal |
| M10 | `unary_ops/unary_neg_mro.py` | `__neg__` inherited through MRO |
| M11 | `binary_ops/binary_add_mro_inherited.py` | `__add__` found via MRO (not own class) |
| M12 | `cross_protocol/eq_notimplemented_identity_fallback.py` | Both `__eq__` return NI -> identity comparison |

### LOW Priority (Edge Cases)

| ID | File | Description |
|----|------|-------------|
| L01 | `string_repr_format/repr_recursive.py` | Self-referencing object in `repr()` |
| L02 | `callable_protocol/call_nested.py` | Nested `__call__` (callable returns callable) |
| L03 | `cross_protocol/hash_cached_consistency.py` | Hash value cached, must not change |
| L04 | `binary_ops/binary_add_different_notimplemented_paths.py` | `__add__` absent, only `__radd__` on other side |
| L05 | `class_creation/mro_entries.py` | `__mro_entries__` expands virtual bases |
| L06 | `context_manager/async_with_basic.py` | Async context manager protocol |
| L07 | `descriptor_protocol/descriptor_slots.py` | `__slots__` interaction with descriptors |

---

## Snippet Code for All HIGH-Priority Items

All snippets below follow the established format with conformance/description/tags headers.
They have been written as standalone `.py` files in the `snippets/` directory.

See the individual files in:
- `snippets/cross_protocol/` (H01-H07, H19-H22)
- `snippets/context_manager/` (H08-H10)
- `snippets/callable_protocol/` (H11-H14)
- `snippets/string_repr_format/` (H15-H16)
- `snippets/class_creation/` (H17-H18)
- `snippets/container_protocol/` (H23, H25)
- `snippets/hash_protocol/` (H24)
- `snippets/comparison/` (H26)

---

## Summary Statistics

| Category | Existing | Missing HIGH | Missing MEDIUM | Missing LOW |
|----------|:--------:|:------------:|:--------------:|:-----------:|
| Binary Ops | 29 | 1 | 1 | 1 |
| Inplace Ops | 21 | 1 | 0 | 0 |
| Comparison | 16 | 1 | 2 | 0 |
| Unary Ops | 16 | 1 | 1 | 0 |
| Hash Protocol | 12 | 1 | 0 | 1 |
| Attribute Access | 8 | 1 | 0 | 0 |
| Descriptor Protocol | 9 | 0 | 1 | 1 |
| Container Protocol | 14 | 2 | 0 | 0 |
| Context Manager | **0** | 3 | 2 | 1 |
| Callable Protocol | **0** | 4 | 0 | 1 |
| String/Repr/Format | **0** | 2 | 1 | 1 |
| Class Creation | **0** | 2 | 3 | 1 |
| Object Lifecycle | **0** | 0 | 1 | 0 |
| Cross-Protocol | **0** | 7 | 1 | 0 |
| **TOTAL** | **125** | **26** | **13** | **7** |

**Overall gap rate**: 26 HIGH-priority tests missing, 5 entire protocol categories with ZERO coverage.

The most dangerous gaps are the cross-protocol interaction tests (H01-H07, H19-H22) because
these test the boundaries between dispatch systems where the VM must correctly chain, fallback,
or propagate errors. The entirely missing protocol categories (context manager, callable, str/repr,
class creation) mean 5 of the 13 documented protocol families have zero conformance testing.
