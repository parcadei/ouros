# Ouros Dunder Dispatch Map

Generated: 2026-02-18

## Summary

The ouros VM implements Python's dunder dispatch system across six source files:
- `bytecode/vm/binary.rs` — Binary and in-place operator dispatch
- `bytecode/vm/compare.rs` — Comparison and membership dispatch
- `bytecode/vm/attr.rs` — Attribute access and descriptor protocol
- `bytecode/vm/call.rs` — Core dispatch primitives plus callable, hash, repr, format, iter, context manager
- `bytecode/vm/mod.rs` — Opcode-level dispatch for unary, subscript, hash, iter, for-loop
- `heap.rs` — Hash state and MRO-based unhashability detection

The central primitive is `lookup_type_dunder` (call.rs:10085) which looks up methods on the
**type** of an instance (not the instance itself), conforming to CPython's type-based dispatch.
Async variants (`__aenter__`/`__aexit__`) are handled alongside their sync counterparts.

---

## Protocol: Lookup Primitives

| Rust Function | File:Line | Role | Notes |
|---|---|---|---|
| `lookup_type_dunder` | call.rs:10085 | Core MRO lookup on instance type | Handles `__hash__` unhashability early; all other dunder lookups flow through here |
| `lookup_metaclass_dunder` | call.rs:10117 | Metaclass-level MRO lookup | Used for `__getattribute__`, `__setattr__`, `__delattr__`, `__call__` on class objects; filters out `object`-level fallbacks |
| `lookup_metaclass_namespace_dunder` | call.rs:10168 | Own-namespace-only metaclass lookup | No MRO walk; for immediate metaclass hooks |
| `call_dunder` | call.rs:10217 | Calls dunder with instance prepended as `self` | Handles ArgValues variants; increments instance refcount |
| `call_class_dunder` | call.rs:10252 | Calls dunder with class prepended as `self` | For metaclass hooks: `__prepare__`, `__mro_entries__`, `__instancecheck__`, `__subclasscheck__` |

---

## Protocol: Binary Operators

| Rust Function | File:Line | Python Protocol | Handles NotImplemented? | MRO-aware? |
|---|---|---|---|---|
| `binary_add` | binary.rs:117 | `__add__` / `__radd__` | Yes, via `try_binary_dunder` | Yes |
| `binary_sub` | binary.rs:151 | `__sub__` / `__rsub__` | Yes | Yes |
| `binary_mult` | binary.rs:220 | `__mul__` / `__rmul__` | Yes | Yes |
| `binary_div` | binary.rs:232 | `__truediv__` / `__rtruediv__` | Yes | Yes |
| `binary_floordiv` | binary.rs:244 | `__floordiv__` / `__rfloordiv__` | Yes | Yes |
| `binary_mod` | binary.rs:255 | `__mod__` / `__rmod__` | Yes | Yes |
| `binary_pow` | binary.rs:267 | `__pow__` / `__rpow__` | Yes | Yes |
| `binary_matmul` | binary.rs:278 | `__matmul__` / `__rmatmul__` | Yes, direct dunder path (no native op) | Yes |
| `binary_bitwise` | binary.rs:300 | `__and__`/`__rand__`, `__or__`/`__ror__`, `__xor__`/`__rxor__`, `__lshift__`/`__rlshift__`, `__rshift__`/`__rrshift__` | Yes — only on TypeError | Yes |
| `try_binary_dunder` | call.rs:10286 | Generic binary dispatch primitive | Yes — drops NotImplemented, tries reflected | Yes (via `lookup_type_dunder`) |
| `binary_result_if_implemented` | call.rs:10353 | NotImplemented filter | Consumes and drops NotImplemented | — |
| `binary_dunder_type_error` | call.rs:10364 | TypeError builder for binary ops | — | — |
| `compare_mod_eq` | compare.rs:330 | `__mod__` / `__rmod__` (for `a%b==k` pattern) | Yes, falls back to `try_binary_dunder` | Yes |

**Fast paths**: `binary_add` and `binary_sub` have `Int + Int` / `Int - Int` peek-ahead fast paths that bypass dunder lookup entirely. `binary_mult`, `binary_div`, etc. use `binary_op_with_dunder!` macro.

**Pattern for all binary ops**:
1. Try native `py_add`/`py_sub`/etc. (fast path for builtins)
2. If `None` returned: call `try_binary_dunder(lhs, rhs, __op__, __rop__)`
3. `try_binary_dunder` tries `lhs.__op__(rhs)`, if NotImplemented tries `rhs.__rop__(lhs)`
4. If nothing works: raise `TypeError: unsupported operand type(s)`

---

## Protocol: Inplace Operators

| Rust Function | File:Line | Python Protocol | Falls back to binary? | Frame-aware? |
|---|---|---|---|---|
| `inplace_add` | binary.rs:359 | `__iadd__` → `__add__` / `__radd__` | Yes | Yes — pending_binary_dunder push |
| `inplace_sub` | binary.rs:419 | `__isub__` → `__sub__` / `__rsub__` | Yes | Yes |
| `inplace_mul` | binary.rs:461 | `__imul__` → `__mul__` / `__rmul__` | Yes | Yes |
| `inplace_div` | binary.rs:475 | `__itruediv__` → `__truediv__` / `__rtruediv__` | Yes | Yes |
| `inplace_floordiv` | binary.rs:489 | `__ifloordiv__` → `__floordiv__` / `__rfloordiv__` | Yes | Yes |
| `inplace_mod` | binary.rs:502 | `__imod__` → `__mod__` / `__rmod__` | Yes | Yes |
| `inplace_pow` | binary.rs:515 | `__ipow__` → `__pow__` / `__rpow__` | Yes | Yes |
| `inplace_bitwise` | binary.rs:528 | `__iand__`/`__ior__`/`__ixor__`/`__ilshift__`/`__irshift__` → their binary counterparts | Yes | Yes |
| `try_inplace_dunder` | call.rs:10373 | Generic inplace dispatch primitive | Yes — NotImplemented falls through to `try_binary_dunder` | Yes |

**Fallback chain**: `__iop__` → if NotImplemented → `try_binary_dunder(__op__, __rop__)`.

**Stage tracking**: When `__iop__` or `__op__` is a user-defined function that pushes a frame,
a `PendingBinaryDunder` entry is pushed with `stage: Inplace|Primary|Reflected` to track which
leg of the chain is being awaited. Cleaned up by `drop_pending_binary_dunder_for_frame`
(mod.rs:7825) on frame pop.

---

## Protocol: Comparison / Rich Compare

| Rust Function | File:Line | Python Protocol | Handles NotImplemented? | Notes |
|---|---|---|---|---|
| `compare_eq` | compare.rs:24 | `__eq__` | Yes, via `try_instance_compare_dunder` | Fast path Int==Int; tries `lhs.__eq__(rhs)` then `rhs.__eq__(lhs)` |
| `compare_ne` | compare.rs:53 | `__ne__` → fallback to `!__eq__` | Yes | CPython fallback: if `__ne__` not implemented, negate `__eq__`; uses `pending_negate_bool` flag |
| `compare_lt` | compare.rs:99 | `__lt__` / reflected `__gt__` | Yes | |
| `compare_le` | compare.rs:107 | `__le__` / reflected `__ge__` | Yes | |
| `compare_gt` | compare.rs:115 | `__gt__` / reflected `__lt__` | Yes | |
| `compare_ge` | compare.rs:123 | `__ge__` / reflected `__le__` | Yes | |
| `compare_ord_dunder` | compare.rs:136 | Shared ordering helper | Yes | Fast path Int cmp Int; tries lhs dunder then reflected rhs dunder |
| `try_instance_compare_dunder` | compare.rs:185 | Generic comparison dispatch | Yes — drops NotImplemented | Tries `lhs.__op__(rhs)`, then `rhs.__rop__(lhs)` if provided |
| `comparison_result_if_implemented` | compare.rs:228 | NotImplemented filter | Consumes and drops NotImplemented | |
| `compare_is` | compare.rs:257 | `is` / `is not` | n/a — identity comparison, no dunder | |
| `compare_in` | compare.rs:267 | `__contains__` | n/a — truthy result | Handles FramePushed with `pending_negate_bool` for `not in` |

**Note**: `__ne__` falls back to negating `__eq__` result, matching CPython behavior. The async
`pending_negate_bool` flag is used for both `!=` fallback and `not in` with FramePushed.

---

## Protocol: Unary Operators

| Rust Function | File:Line | Python Protocol | Fast path? | Notes |
|---|---|---|---|---|
| `Opcode::UnaryNot` handler | mod.rs:2927 | `__bool__` → fallback `__len__` | Yes — skips dunder for non-instances | `pending_negate_bool = true` when FramePushed |
| `Opcode::UnaryNeg` handler | mod.rs:2997 | `__neg__` | No (checks instance first) | Uses `try_unary_dunder` |
| `Opcode::UnaryPos` handler | mod.rs:3089 | `__pos__` | No | Uses `try_unary_dunder` |
| `Opcode::UnaryInvert` handler | mod.rs:3151 | `__invert__` | No | Uses `try_unary_dunder` |
| `try_unary_dunder` | call.rs:10417 | Generic unary dispatch | — | Looks up on type; calls with ArgValues::Empty; no NotImplemented handling |

**`__bool__` fallback chain** (UnaryNot): tries `__bool__` first; if not found, tries `__len__` and treats nonzero as truthy. Matches CPython exactly.

**`__bool__` also invoked in truthiness tests**: `JumpIfFalse`, `JumpIfTrue` opcodes (mod.rs:3961, 4038, 4109, 4151) all dispatch `__bool__` then `__len__` for instance truthiness evaluation.

---

## Protocol: Hash

| Rust Function | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `exec_call_builtin_function` hash branch | call.rs:484-566 | `__hash__` via `hash()` builtin | Calls `lookup_type_dunder` + `call_dunder`; validates return is int; caches via `heap.set_cached_hash`; raises TypeError for `__eq__`-without-`__hash__` |
| `Opcode::BinarySubscr` hash pre-cache | mod.rs:3576 | `__hash__` for dict key pre-hashing | Calls `__hash__` on instance dict keys before lookup; uses `pending_hash_target` |
| `lookup_type_dunder` __hash__ guard | call.rs:10094 | `__hash__` unhashability | Returns `None` early if `is_unhashable_via_mro` is true |
| `is_unhashable_via_mro` | heap.rs:3932 | MRO unhashability detection | Returns true if `__hash__ = None` found in MRO OR `__eq__` defined but no `__hash__` in MRO |
| `compute_hash_inner` | heap.rs:3965 | Internal hash computation | Handles Cell (identity hash), Instance (MRO check), all other types |
| `compute_hash` (dataclass) | types/dataclass.rs:166 | Dataclass hash | Delegates to heap hash or tuple hash for frozen dataclasses |
| `compute_hash` (set element) | types/set.rs:1417 | Set element hash | |

**Hash protocol interaction with `__eq__`**: Both `lookup_type_dunder` (via `is_unhashable_via_mro`) and `exec_call_builtin_function` (line 550) check for `__eq__`-without-`__hash__`. `is_unhashable_via_mro` walks the full MRO; the builtin path checks with a separate `lookup_type_dunder` call.

---

## Protocol: Attribute Access

| Rust Function | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `load_attr` | attr.rs:52 | `__getattribute__`, `__getattr__`, descriptor `__get__` | Full Python attribute lookup: custom `__getattribute__` → AttributeError → `__getattr__` fallback |
| `store_attr` | attr.rs:985 | `__setattr__`, descriptor `__set__` | Custom `__setattr__` → descriptor setter (`UserProperty`/custom `__set__`) → direct store |
| `delete_attr` | attr.rs:1225 | `__delattr__`, descriptor `__delete__` | Custom `__delattr__` → descriptor deleter → direct delete |
| `call_descriptor_get` | attr.rs:229 | `__get__` | Handles SlotDescriptor, CachedProperty, SingleDispatchMethod, PartialMethod, custom `__get__` |
| `call_descriptor_get_with_owner` | attr.rs:785 | `__get__(obj, owner)` | For metaclass descriptor lookups with explicit owner |
| `load_class_attr_default` | attr.rs:819 | Metaclass `__getattribute__` + `type.__getattribute__` semantics | Metaclass data descriptors → class namespace/MRO → metaclass non-data descriptors/attrs → metaclass `__getattr__` |
| `find_descriptor_setter` | attr.rs:1102 | `__set__` discovery | Checks `UserProperty.fset`, SlotDescriptor, custom class with `__set__` in MRO |
| `find_descriptor_func` | attr.rs:1158 | `__set__` / `__delete__` discovery (generic) | |
| `call_slot_descriptor_get` | attr.rs:521 | `__slots__` member access | |
| `set_slot_descriptor` | attr.rs:604 | `__slots__` member set | |
| `delete_slot_descriptor` | attr.rs:702 | `__slots__` member delete | |
| `call_cached_property_get` | attr.rs:296 | `functools.cached_property` `__get__` | Caches result in `instance.__dict__` |
| `call_singledispatchmethod_get` | attr.rs:378 | `singledispatchmethod.__get__` | Returns bound partial on instance access |
| `call_partialmethod_get` | attr.rs:407 | `partialmethod.__get__` | |

**Priority order** for `load_attr` on instances:
1. Custom `__getattribute__` (type-level lookup)
2. On AttributeError from `__getattribute__`: try `__getattr__`
3. Default Python descriptor priority: data descriptors → instance attrs → non-data descriptors

**Priority order** for `store_attr` on instances:
1. Custom `__setattr__` (type-level lookup)
2. Data descriptor `__set__` in class MRO
3. Direct instance attribute store

**Metaclass variants**: `lookup_metaclass_dunder` is used for `__getattribute__`, `__setattr__`, `__delattr__` on class objects. The object-level builtins for these are filtered out (call.rs:10134-10158) so only custom metaclass overrides trigger.

---

## Protocol: Container

| Rust Function / Location | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `Opcode::BinarySubscr` instance branch | mod.rs:3430 | `__getitem__` | Calls `call_dunder(obj_id, method, ArgValues::One(idx))` |
| `Opcode::BinarySubscr` ClassObject branch | mod.rs:3457 | `__class_getitem__` | Used for `list[int]`-style PEP 585 generic aliases on custom classes |
| `Opcode::BinarySubscr` `__index__` branch | mod.rs:3542 | `__index__` | On instances used as subscript index; re-executes opcode with int result |
| `Opcode::StoreSubscr` instance branch | mod.rs:3757 | `__setitem__` | Calls `call_dunder(obj_id, method, ArgValues::Two(index, value))` |
| `Opcode::DeleteSubscr` instance branch | mod.rs:3818 | `__delitem__` | Calls `call_dunder(obj_id, method, ArgValues::One(index))` |
| `compare_in` | compare.rs:267 | `__contains__` | Calls `call_dunder(container_id, method, ArgValues::One(item_clone))` |
| `Opcode::GetIter` instance branch | mod.rs:4203 | `__iter__` | `lookup_type_dunder` + `call_dunder` + `normalize_iter_result` |
| `Opcode::GetIter` class branch | mod.rs:4236 | metaclass `__iter__` | `lookup_metaclass_dunder` + `call_class_dunder` |
| `Opcode::ForIter` instance branch | mod.rs:4305 | `__next__` | In for-loop iteration; stores jump offset in `pending_for_iter_jump` for FramePushed |
| `exec_call_builtin_function` len branch | call.rs:486 | `__len__` via `len()` | |
| `exec_call_builtin_function` next branch | call.rs:488 | `__next__` via `next()` | |

---

## Protocol: String / Repr / Format

| Rust Function / Location | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `exec_call_builtin_function` repr branch | call.rs:484 | `__repr__` via `repr()` | Calls `call_dunder`; result post-processed by `handle_stringify_call_result` |
| `exec_call_builtin_type` str branch | call.rs:695 | `__str__` → fallback `__repr__` | `str(x)` tries `__str__`; falls back to `__repr__` if not found |
| `exec_call_builtin_type` int/float/complex/bool branch | call.rs:696-699 | `__int__`, `__float__`, `__complex__`, `__bool__`/`__len__` | Type conversion dunders |
| format builtin dispatch | call.rs:8134 | `__format__` | `format(obj, spec)` calls `obj.__format__(spec)` on instances |
| `format_value` | format.rs:54 | f-string formatting | Uses `py_str`/`py_repr` for non-instances (no dunder dispatch at f-string level for native types) |

---

## Protocol: Callable

| Rust Function / Location | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `call_function` (Instance branch) | call.rs:6621 | `__call__` | `lookup_type_dunder` then `call_dunder(heap_id, method, args)` |
| `call_function` (ClassObject branch) | call.rs:6441 | metaclass `__call__` | `lookup_metaclass_dunder`; if not found falls through to `call_class_instantiate` |
| `call_class_instantiate` | call.rs:7341 | `__new__` + `__init__` | MRO lookup for both; `__new__` called first, creates instance; `__init__` called on result |
| super `__call__` | call.rs:9879 | super().__call__ | Falls through to `call_class_instantiate` for metaclass methods |

**Class instantiation** (`call_class_instantiate` flow):
1. MRO lookup for `__new__` and `__init__`
2. Check for abstract classes (raises TypeError if abstract methods present)
3. If metaclass subclass with no custom `__new__`/`__init__`: delegate to `type.__new__`
4. Call `__new__` → allocates instance
5. Call `__init__` on result

---

## Protocol: Context Manager

| Rust Function / Location | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `call_function` `__enter__`/`__exit__` detection | call.rs:6628-6650 | `__enter__` + `__exit__` (context decorator) | Detects sync decorator protocol (has both); also checks async (`__aenter__`/`__aexit__`) |
| `call_exit_stack_enter_context` | call.rs:2213 | `__enter__` | ExitStack.enter_context path |
| `call_exit_stack_enter_async_context` | call.rs:2250 | `__aenter__` | AsyncExitStack path |
| `call_context_decorator_with_instance` | call.rs:1850 | `__enter__` / `__aenter__` + `__exit__` / `__aexit__` | Instance-based context decorator |
| `context_decorator_close` | call.rs:2007 | `__exit__` / `__aexit__` | Called after wrapped function completes |
| `exit_stack_continue_unwind` | call.rs:2344 | `__exit__` / `__aexit__` | ExitStack callback unwind |
| `call.rs` with-statement enter (BeforeWithBlock opcode) | call.rs:2215 | `__enter__` | |
| `call.rs` with-statement exit | call.rs:2076, 2376 | `__exit__` / `__aexit__` | `(exc_type, exc_value, tb)` args; suppress semantics |

**Async variants**: `__aenter__`/`__aexit__` are dispatched via `call_attr` (not `call_dunder` directly) to match the attribute-call pattern Python uses. The result is passed through `exec_get_awaitable()`.

---

## Protocol: Descriptor Protocol

| Rust Function | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `call_descriptor_get` | attr.rs:229 | `__get__(self, obj, type)` | Instance access: `(instance, type(instance))`; class access: `(None, owner)` |
| `call_descriptor_get_with_owner` | attr.rs:785 | `__get__(self, obj, owner)` | Explicit owner for metaclass descriptor chains |
| `store_attr` custom descriptor | attr.rs:1027 | `__set__(self, instance, value)` | `lookup_type_dunder(desc_id, __set__)` then `call_function(method, [desc, obj, value])` |
| `delete_attr` custom descriptor | attr.rs:1270 | `__delete__(self, instance)` | |
| `load_attr_default` (class access) | attr.rs (within `load_attr`) | `__get__` on class-level access | Checks `mro_has_attr("__get__")` and calls `call_descriptor_get(value, obj)` |

**Data descriptor priority**: `is_data_descriptor` (types/class.rs) determines if a value has `__set__` or `__delete__`, which grants priority over instance attributes.

---

## Protocol: Class Creation Hooks

| Rust Function / Location | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `__mro_entries__` | mod.rs:5958 | `__mro_entries__` | Called on bases during class creation to expand virtual bases |
| `__prepare__` | mod.rs:6237 | `__prepare__` | Metaclass namespace preparation |
| `__init_subclass__` | mod.rs:7650 | `__init_subclass__` | Called after class body executes |
| `__set_name__` | mod.rs:7547, 7579 | `__set_name__` | Called on descriptors when assigned in class body |
| `__instancecheck__` | call.rs:6216 | `__instancecheck__` | `isinstance()` with custom class |
| `__subclasscheck__` | call.rs:6253 | `__subclasscheck__` | `issubclass()` with custom class |

---

## Protocol: Object Lifecycle

| Rust Function / Location | File:Line | Python Protocol | Notes |
|---|---|---|---|
| `call_class_instantiate` | call.rs:7341 | `__new__` | MRO lookup; `object.__new__` fallback creates bare instance |
| `call_class_instantiate` | call.rs:7349 | `__init__` | MRO lookup; called with args on new instance |
| `pending_init_instance` frame flag | mod.rs (frame data) | `__init__` post-processing | Return value of `__init__` is discarded (must return None) |

---

## Pending State Structures (Frame-Aware Dispatch)

When a dunder method is a user-defined function, it pushes a new call frame. The VM tracks
continuation state via pending flags/stacks:

| Field | File | Tracks |
|---|---|---|
| `pending_binary_dunder: Vec<PendingBinaryDunder>` | mod.rs | Binary/inplace dunder in-progress; stage = Primary/Reflected/Inplace |
| `pending_negate_bool: bool` | mod.rs | `__ne__` fallback and `not in` negate-after-return |
| `pending_for_iter_jump: Vec<i16>` | mod.rs | `ForIter` jump offset for `__next__` FramePushed |
| `pending_hash_target: Option<HeapId>` | mod.rs | `__hash__` return → cache in heap entry |
| `pending_hash_push_result: bool` | mod.rs | For `hash()` builtin: push formatted i64 result |
| `pending_cached_property: Option<PendingCachedProperty>` | mod.rs | Cache result from `cached_property.__get__` |
| `pending_discard_return: bool` | mod.rs | `__setitem__`/`__delitem__`/setter discard return |
| `pending_getattr_fallback: Vec<PendingGetAttrInfo>` | mod.rs | `__getattr__` fallback after `__getattribute__` raises AttributeError |

**Cleanup**: `drop_pending_binary_dunder_for_frame` (mod.rs:7825) and `clear_pending_binary_dunder` (mod.rs:7836) release operand refcounts when a dunder frame exits via exception unwinding.

---

## Cross-Protocol Interactions

- **`__eq__` → `__hash__` interaction**: If a class defines `__eq__` but not `__hash__`, instances are unhashable. Detected by `is_unhashable_via_mro` (heap.rs:3932). This walks the full MRO so inherited `__hash__ = None` is found. Both `lookup_type_dunder` (for any `__hash__` lookup) and `exec_call_builtin_function` (the `hash()` builtin) independently guard this.

- **`__ne__` → `__eq__` fallback**: `compare_ne` (compare.rs:53) tries `__ne__` first; if NotImplemented/absent, falls back to calling `__eq__` and negating. The negation flag `pending_negate_bool` handles the FramePushed case.

- **`__bool__` → `__len__` fallback**: `UnaryNot` (mod.rs:2927), `JumpIfFalse`/`JumpIfTrue` (mod.rs:3961, 4038, 4109) all implement the CPython fallback: try `__bool__`; if missing, try `__len__`; if missing, default True.

- **`__iadd__` → `__add__` → `__radd__`**: `try_inplace_dunder` (call.rs:10373) tries `__iop__`; on NotImplemented falls through to `try_binary_dunder` which tries `__op__` then `__rop__`.

- **`__getattribute__` → `__getattr__`**: `load_attr` (attr.rs:52) calls custom `__getattribute__`; on AttributeError, tries `__getattr__` as fallback. Both instance and class paths implement this (class path uses metaclass lookup).

- **`__get__` and data descriptors**: `find_descriptor_setter` (attr.rs:1102) checks `__set__`/`__delete__` presence to decide data-vs-non-data descriptor priority. `store_attr` calls `__setattr__` before checking descriptors.

- **`__iter__` → `__next__`**: `GetIter` opcode calls `__iter__` to obtain an iterator, then `ForIter` repeatedly calls `__next__`. Both dunder steps are frame-aware.

---

## Known Bug-Prone Areas (from Bugbot history)

### Bug #3: Binary dunder cleanup on frame pop
- **Location**: `drop_pending_binary_dunder_for_frame` (mod.rs:7825), called from `pop_frame` (mod.rs:7868)
- **Risk**: If a dunder call frame is popped during exception unwinding before the protocol completes (e.g., `__add__` raises but `__radd__` was queued), operand refcounts must be released. The `frame_depth` field in `PendingBinaryDunder` matches against current depth.
- **Test surface**: Raise exceptions inside `__add__` when `__radd__` exists on other operand.

### Bug #5: MRO-based `__hash__ = None` detection
- **Location**: `is_unhashable_via_mro` (heap.rs:3932), called from `lookup_type_dunder` (call.rs:10098)
- **Risk**: Walk must find `__hash__ = None` even when inherited through deep MRO. Also must detect `__eq__`-without-`__hash__` across the full chain. Two separate flags (`hash_is_none`, `has_eq`/`has_hash`) break early once both found.
- **Test surface**: Subclass of class with `__hash__ = None`; class with `__eq__` but inherited `__hash__`; diamond inheritance with mixed hash state.

### Bug #7: Inplace fallback to binary when NotImplemented
- **Location**: `try_inplace_dunder` (call.rs:10373) line ~10404: comment "fall through to binary dunder"
- **Risk**: When `__iop__` returns NotImplemented (as a synchronous `CallResult::Push(NotImplemented)`), it falls through to `try_binary_dunder`. If `__iop__` pushes a frame (FramePushed), the stage is recorded as `Inplace`; the frame return handler must then check the return value and still fall through if NotImplemented. This async NotImplemented path needs careful testing.
- **Test surface**: `__iadd__` returning `NotImplemented` (sync); `__iadd__` returning `NotImplemented` from a user-defined function (async/FramePushed path).

### Additional risk areas (not in Bugbot history):

### `__ne__` fallback negate with FramePushed
- **Location**: `compare_ne` (compare.rs:82-88), `pending_negate_bool = true`
- **Risk**: When `__ne__` is absent and `__eq__` pushes a frame, `pending_negate_bool` is set. If the return value from `__eq__` is not a bool (e.g., an instance), the negation in `py_bool` + `!` may not match CPython semantics exactly.

### `__index__` re-execution with `BinarySubscr`
- **Location**: mod.rs:3542-3568
- **Risk**: The opcode IP is backed up to `self.instruction_ip` (pre-opcode) so `BinarySubscr` re-executes with the int result. If the `__index__` call itself uses subscripting, this re-execution could create unexpected state.

### `__contains__` with `not in` and FramePushed
- **Location**: `compare_in` (compare.rs:267), `pending_negate_bool = true` (line 301)
- **Risk**: Same `pending_negate_bool` flag shared between `__ne__` fallback and `not in`. If both are somehow nested, state could corrupt. In practice, frames are sequential, but worth auditing.
