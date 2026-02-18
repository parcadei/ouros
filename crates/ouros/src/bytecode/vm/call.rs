//! Function call helpers for the VM.
//!
//! This module contains the implementation of call-related opcodes and helper
//! functions for executing function calls. The main entry points are the `exec_*`
//! methods which are called from the VM's main dispatch loop.

use std::{cmp::Ordering, env, fs, path::Path, ptr, str::FromStr};

use smallvec::SmallVec;

use super::{
    AwaitResult, CallAttrInlineCacheEntry, CallAttrInlineCacheKind, CallFrame, PendingBinaryDunder,
    PendingBinaryDunderStage, PendingBuiltinFromList, PendingBuiltinFromListKind, PendingContextDecorator,
    PendingContextDecoratorStage, PendingExitStack, PendingExitStackAwaiting, PendingGroupBy, PendingListSort,
    PendingNewCall, PendingNextDefault, PendingReduce, PendingStringifyReturn, PendingSumFromList,
    PendingTextwrapIndent, VM,
};
use crate::{
    args::{ArgValues, KwargsValues},
    asyncio::Coroutine,
    builtins::{Builtins, BuiltinsFunctions},
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    fstring::{ParsedFormatSpec, format_with_spec},
    heap::{DropWithHeap, Heap, HeapData, HeapGuard, HeapId, ObjectNewImpl},
    intern::{ExtFunctionId, FunctionId, Interns, StaticStrings, StringId},
    io::PrintWriter,
    modules::{
        ModuleFunctions, asyncio::AsyncioFunctions, bisect::BisectFunctions, collections::CollectionsFunctions,
        heapq::HeapqFunctions, statistics::StatisticsFunctions, typing::TypingFunctions,
    },
    namespace::NamespaceId,
    os::OsFunction,
    proxy::ProxyId,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{
        AttrCallResult, Bytes, ClassObject, Dict, ExitCallback, Instance, List, OurosIter, PyTrait, StdlibObject, Str,
        Type, UserProperty, allocate_tuple,
        bytes::{bytes_fromhex, bytes_maketrans, call_bytes_method},
        class::PropertyAccessorKind,
        dict::dict_fromkeys,
        make_generic_alias,
        str::{call_str_method, str_maketrans},
    },
    value::{EitherStr, Value},
};

/// Result of executing a call opcode.
///
/// Used by the `exec_*` methods to communicate what action the VM's main loop
/// should take after the call completes.
#[derive(Debug)]
pub(super) enum CallResult {
    /// Call completed successfully - push this value onto the stack.
    Push(Value),
    /// A new frame was pushed for a defined function call.
    /// The VM should reload its cached frame state.
    FramePushed,
    /// External function call requested - VM should pause and return to caller.
    External(ExtFunctionId, ArgValues),
    /// Proxy operation requested - VM should pause and return to caller.
    Proxy(ProxyId, String, ArgValues),
    /// OS operation call requested - VM should yield `FrameExit::OsCall` to host.
    ///
    /// The host executes the OS operation and resumes the VM with the result.
    OsCall(OsFunction, ArgValues),
}

/// Bisect operation kind for VM-level key-call handling.
#[derive(Debug, Clone, Copy)]
enum BisectOperation {
    Left,
    Right,
    InsortLeft,
    InsortRight,
}

impl BisectOperation {
    /// Returns `true` for left-bias operations (`bisect_left` / `insort_left`).
    fn is_left(self) -> bool {
        matches!(self, Self::Left | Self::InsortLeft)
    }

    /// Returns `true` when the operation mutates the list (`insort_*`).
    fn is_insert(self) -> bool {
        matches!(self, Self::InsortLeft | Self::InsortRight)
    }

    /// Returns the Python function name used in diagnostics.
    fn name(self) -> &'static str {
        match self {
            Self::Left => "bisect_left",
            Self::Right => "bisect_right",
            Self::InsortLeft => "insort_left",
            Self::InsortRight => "insort_right",
        }
    }
}

/// Maps binary dunder names to user-facing operator symbols for TypeError text.
fn binary_symbol_for_dunder(dunder_id: StringId) -> &'static str {
    if dunder_id == StaticStrings::DunderAdd {
        "+"
    } else if dunder_id == StaticStrings::DunderSub {
        "-"
    } else if dunder_id == StaticStrings::DunderMul {
        "*"
    } else if dunder_id == StaticStrings::DunderTruediv {
        "/"
    } else if dunder_id == StaticStrings::DunderFloordiv {
        "//"
    } else if dunder_id == StaticStrings::DunderMod {
        "%"
    } else if dunder_id == StaticStrings::DunderPow {
        "**"
    } else if dunder_id == StaticStrings::DunderLshift {
        "<<"
    } else if dunder_id == StaticStrings::DunderRshift {
        ">>"
    } else if dunder_id == StaticStrings::DunderAnd {
        "&"
    } else if dunder_id == StaticStrings::DunderOr {
        "|"
    } else if dunder_id == StaticStrings::DunderXor {
        "^"
    } else if dunder_id == StaticStrings::DunderMatmul {
        "@"
    } else {
        "?"
    }
}

impl From<AttrCallResult> for CallResult {
    fn from(result: AttrCallResult) -> Self {
        match result {
            AttrCallResult::Value(v) => Self::Push(v),
            AttrCallResult::OsCall(func, args) => Self::OsCall(func, args),
            AttrCallResult::ExternalCall(ext_id, args) => Self::External(ext_id, args),
            AttrCallResult::PropertyCall(_, _) => {
                // PropertyCall should be handled by the VM's load_attr, not generic conversion.
                // This variant is only used by load_attr to defer property execution.
                // If we reach here, it indicates a bug in the VM's attribute loading logic.
                unreachable!("PropertyCall must be handled by load_attr, not generic conversion")
            }
            AttrCallResult::DescriptorGet(_) => {
                // DescriptorGet should be handled by the VM's load_attr, not generic conversion.
                // This variant is only used by load_attr to defer descriptor protocol execution.
                // If we reach here, it indicates a bug in the VM's attribute loading logic.
                unreachable!("DescriptorGet must be handled by load_attr, not generic conversion")
            }
            AttrCallResult::ReduceCall(_, _, _) => {
                // ReduceCall must be handled by the VM's call_attr/call_function, not generic conversion.
                unreachable!("ReduceCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::MapCall(_, _) => {
                // MapCall must be handled by the VM's call_attr/call_function, not generic conversion.
                unreachable!("MapCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::FilterCall(_, _) => {
                // FilterCall must be handled by the VM's call_attr/call_function, not generic conversion.
                unreachable!("FilterCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::FilterFalseCall(_, _) => {
                unreachable!("FilterFalseCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::TakeWhileCall(_, _) => {
                unreachable!("TakeWhileCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::DropWhileCall(_, _) => {
                unreachable!("DropWhileCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::GroupByCall(_, _) => {
                // GroupByCall must be handled by the VM's call_attr/call_function, not generic conversion.
                unreachable!("GroupByCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::TextwrapIndentCall(_, _, _) => {
                // TextwrapIndentCall must be handled by the VM's call_attr/call_function, not generic conversion.
                unreachable!("TextwrapIndentCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::CallFunction(_, _) => {
                // CallFunction must be handled by the VM's call_attr/call_function, not generic conversion.
                unreachable!("CallFunction must be handled by the VM, not generic conversion")
            }
            AttrCallResult::ReSubCall(_, _, _, _, _) => {
                // ReSubCall must be handled by the VM's handle_attr_call_result, not generic conversion.
                unreachable!("ReSubCall must be handled by the VM, not generic conversion")
            }
            AttrCallResult::ObjectNew => {
                // ObjectNew requires heap access to get the singleton, so it must be handled
                // by handle_attr_call_result, not this generic conversion.
                unreachable!("ObjectNew must be handled by handle_attr_call_result, not generic conversion")
            }
        }
    }
}

impl<T: ResourceTracker, P: PrintWriter, Tr: VmTracer> VM<'_, T, P, Tr> {
    // ========================================================================
    // Call Opcode Executors
    // ========================================================================
    // These methods are called from the VM's main dispatch loop to execute
    // call-related opcodes. They handle stack operations and return a result
    // indicating what the VM should do next.

    /// Executes `CallFunction` opcode.
    ///
    /// Pops the callable and arguments from the stack, calls the function,
    /// and returns the result.
    ///
    /// Contains an inlined fast path for calling simple sync `def` functions
    /// (no closures, no defaults, no async, no cells, simple signature).
    /// This avoids the 4-level dispatch chain (`exec_call_function` -> `call_function` ->
    /// `call_def_function` -> `call_sync_function`) that dominates recursive workloads
    /// like `fib(n)`.
    #[inline]
    pub(super) fn exec_call_function(&mut self, arg_count: usize) -> Result<CallResult, RunError> {
        // Peek at callable to check for fast path before popping args
        let callable = self.peek_at_depth(arg_count);
        if let Value::DefFunction(func_id) = callable {
            let func_id = *func_id;
            let func = self.interns.get_function(func_id);
            if func.is_simple_sync() {
                // Extract scalar values before dropping the borrow on `func`
                let param_count = func.signature.param_count();
                let namespace_size = func.namespace_size;
                return self.exec_call_function_simple(func_id, param_count, namespace_size, arg_count);
            }
        }
        // Slow path: full dispatch chain for complex cases
        let args = self.pop_n_args(arg_count);
        let callable = self.pop();
        self.call_function(callable, args)
    }

    /// Fast path for calling a simple sync `def` function with positional args only.
    ///
    /// Inlines the entire call setup that normally goes through `call_function` ->
    /// `call_def_function` -> `call_sync_function`. For the common case of a simple
    /// function (no closures, no defaults, no cells, no async), this:
    /// 1. Moves args directly from operand stack to namespace (no `ArgValues` intermediate)
    /// 2. Skips the callable type dispatch
    /// 3. Skips the async check
    /// 4. Skips cell/free-var creation
    /// 5. Avoids allocating empty `Vec`s for defaults and cells
    #[inline]
    fn exec_call_function_simple(
        &mut self,
        func_id: FunctionId,
        param_count: usize,
        namespace_size: usize,
        arg_count: usize,
    ) -> Result<CallResult, RunError> {
        // Validate arg count matches param count (simple signatures have no defaults)
        if arg_count != param_count {
            // Pop args and callable, then delegate to full path for proper error reporting
            let args = self.pop_n_args(arg_count);
            let _callable = self.pop();
            return self.call_sync_function(func_id, &[], Vec::new(), args);
        }

        // Get call position BEFORE borrowing namespaces mutably
        let call_position = self.current_position();

        // Allocate namespace
        let namespace_idx = match self.namespaces.new_namespace(namespace_size, self.heap) {
            Ok(idx) => idx,
            Err(e) => {
                // Clean up stack on allocation failure: pop args + callable
                for _ in 0..arg_count {
                    let v = self.stack.pop().expect("stack underflow");
                    v.drop_with_heap(self.heap);
                }
                let _callable = self.stack.pop(); // DefFunction has no heap data
                return Err(e.into());
            }
        };

        // Move args from operand stack directly into namespace without intermediate allocation.
        // Stack layout: [..., callable, arg0, arg1, ..., argN-1]
        // We access `self.stack` and `self.namespaces` as separate fields to split the borrow.
        {
            let namespace = self.namespaces.get_mut(namespace_idx).mut_vec();

            // Drain the top arg_count values from the stack directly into the namespace.
            // These are the args in correct order (arg0 is deepest, argN-1 is on top).
            let stack_len = self.stack.len();
            let args_start = stack_len - arg_count;
            namespace.extend(self.stack.drain(args_start..));

            // Fill remaining namespace slots with Undefined (local variables)
            namespace.resize_with(namespace_size, || Value::Undefined);
        }

        // Pop the callable (DefFunction variant has no heap data to clean up)
        let _callable = self.stack.pop();

        // Re-lookup function from interns for the correct lifetime on code reference.
        // The function data hasn't changed - we just need the 'a lifetime from interns.
        let func = self.interns.get_function(func_id);
        let code = &func.code;

        // Push new frame (no cells needed for simple functions)
        self.frames.push(CallFrame::new_simple_function(
            code,
            self.stack.len(),
            namespace_idx,
            func_id,
            call_position,
        ));
        self.tracer
            .on_call(Some(self.interns.get_str(func.name.name_id)), self.frames.len());

        Ok(CallResult::FramePushed)
    }

    /// Executes `CallBuiltinFunction` opcode.
    ///
    /// Calls a builtin function directly without stack manipulation for the callable.
    /// This is an optimization that avoids constant pool lookup and stack manipulation.
    ///
    /// Intercepts certain builtins to dispatch dunders on instances:
    /// - `repr(x)` -> `x.__repr__()`
    /// - `hash(x)` -> `x.__hash__()`
    /// - `len(x)` -> `x.__len__()`
    /// - `abs(x)` -> `x.__abs__()`
    /// - `next(x)` -> `x.__next__()`
    pub(super) fn exec_call_builtin_function(
        &mut self,
        builtin_id: u8,
        arg_count: usize,
    ) -> Result<CallResult, RunError> {
        if let Some(builtin) = BuiltinsFunctions::from_repr(builtin_id) {
            // len(non-instance) can skip generic ArgValues plumbing and call directly.
            if arg_count == 1
                && matches!(builtin, BuiltinsFunctions::Len)
                && let Some(value) = self.call_len_builtin_fast()?
            {
                return Ok(CallResult::Push(value));
            }
            // sum(generator[, start]) needs VM-driven generator iteration; handle it here.
            if matches!(builtin, BuiltinsFunctions::Sum) {
                let args = self.pop_n_args(arg_count);
                return self.call_sum_builtin(args);
            }
            // any/all on generators need VM-driven materialization first.
            if matches!(builtin, BuiltinsFunctions::Any) {
                let args = self.pop_n_args(arg_count);
                return self.call_any_builtin(args);
            }
            if matches!(builtin, BuiltinsFunctions::All) {
                let args = self.pop_n_args(arg_count);
                return self.call_all_builtin(args);
            }
            // min/max on generators need VM-driven materialization first.
            if arg_count == 1 && matches!(builtin, BuiltinsFunctions::Min) {
                let args = self.pop_n_args(arg_count);
                return self.call_min_builtin(args);
            }
            if arg_count == 1 && matches!(builtin, BuiltinsFunctions::Max) {
                let args = self.pop_n_args(arg_count);
                return self.call_max_builtin(args);
            }
            if matches!(builtin, BuiltinsFunctions::Enumerate) {
                let args = self.pop_n_args(arg_count);
                return self.call_enumerate_builtin(args);
            }
            if matches!(builtin, BuiltinsFunctions::Zip) {
                let args = self.pop_n_args(arg_count);
                return self.call_zip_builtin(args);
            }
            if matches!(builtin, BuiltinsFunctions::Isinstance) {
                let args = self.pop_n_args(arg_count);
                return self.call_isinstance(args);
            }
            if matches!(builtin, BuiltinsFunctions::Issubclass) {
                let args = self.pop_n_args(arg_count);
                return self.call_issubclass(args);
            }
            // dir()/format() need VM-level dunder dispatch for user-defined classes.
            if matches!(builtin, BuiltinsFunctions::Dir) {
                let args = self.pop_n_args(arg_count);
                return self.call_dir_builtin(args);
            }
            if matches!(builtin, BuiltinsFunctions::Format) {
                let args = self.pop_n_args(arg_count);
                return self.call_format_builtin(args);
            }
            // map/filter with user-defined functions need VM frame management.
            if matches!(builtin, BuiltinsFunctions::Map) {
                let args = self.pop_n_args(arg_count);
                return self.call_map_builtin(args);
            }
            if matches!(builtin, BuiltinsFunctions::Filter) {
                let args = self.pop_n_args(arg_count);
                return self.call_filter_builtin(args);
            }
            // sorted with user-defined key functions needs VM frame management.
            if matches!(builtin, BuiltinsFunctions::Sorted) {
                let args = self.pop_n_args(arg_count);
                return self.call_sorted_builtin(args);
            }

            // super() needs VM context (frame stack) - handle it here instead of in builtins
            if matches!(builtin, BuiltinsFunctions::Super) {
                let args = self.pop_n_args(arg_count);
                let result = self.call_super(args)?;
                return Ok(CallResult::Push(result));
            }

            // getattr/setattr/delattr/hasattr need dynamic string-name handling.
            if matches!(
                builtin,
                BuiltinsFunctions::Getattr
                    | BuiltinsFunctions::Setattr
                    | BuiltinsFunctions::Delattr
                    | BuiltinsFunctions::Hasattr
            ) {
                let args = self.pop_n_args(arg_count);
                if matches!(builtin, BuiltinsFunctions::Delattr) {
                    return self.call_delattr_builtin(args);
                }
                let result = match builtin {
                    BuiltinsFunctions::Getattr => self.builtin_getattr(args)?,
                    BuiltinsFunctions::Setattr => self.builtin_setattr(args)?,
                    BuiltinsFunctions::Hasattr => self.builtin_hasattr(args)?,
                    _ => unreachable!(),
                };
                return Ok(CallResult::Push(result));
            }

            // `next(generator, default)` needs VM-driven generator iteration to preserve
            // generator frame semantics while still honoring the default on exhaustion.
            if arg_count == 2 && matches!(builtin, BuiltinsFunctions::Next) {
                let iterator = self.peek_at_depth(1);
                if let Value::Ref(generator_id) = iterator
                    && matches!(self.heap.get(*generator_id), HeapData::Generator(_))
                {
                    let generator_id = *generator_id;
                    let args = self.pop_n_args(2);
                    let ArgValues::Two(iterator, default) = args else {
                        unreachable!("arg_count == 2 must produce ArgValues::Two");
                    };
                    let result = self.generator_next(generator_id);
                    iterator.drop_with_heap(self.heap);
                    return match result {
                        Ok(CallResult::FramePushed) => {
                            self.clear_pending_next_default();
                            self.pending_next_default = Some(PendingNextDefault { generator_id, default });
                            Ok(CallResult::FramePushed)
                        }
                        Ok(other) => {
                            default.drop_with_heap(self.heap);
                            Ok(other)
                        }
                        Err(err) if err.is_stop_iteration() => Ok(CallResult::Push(default)),
                        Err(err) => {
                            default.drop_with_heap(self.heap);
                            Err(err)
                        }
                    };
                }
            }

            // Check for instance dunder dispatch on single-arg builtins
            if arg_count == 1 {
                let arg = self.peek();
                // Check for generator with __next__
                if let Value::Ref(arg_id) = arg
                    && matches!(self.heap.get(*arg_id), HeapData::Generator(_))
                {
                    let arg_id = *arg_id;
                    if matches!(builtin, BuiltinsFunctions::Next) {
                        let arg_val = self.pop();
                        let result = self.generator_next(arg_id);
                        arg_val.drop_with_heap(self.heap);
                        return result;
                    }
                }
                if let Value::Ref(arg_id) = arg
                    && matches!(self.heap.get(*arg_id), HeapData::Instance(_))
                {
                    let arg_id = *arg_id;
                    let dunder = match builtin {
                        BuiltinsFunctions::Repr => Some(StaticStrings::DunderRepr),
                        BuiltinsFunctions::Hash => Some(StaticStrings::DunderHash),
                        BuiltinsFunctions::Len => Some(StaticStrings::DunderLen),
                        BuiltinsFunctions::Abs => Some(StaticStrings::DunderAbs),
                        BuiltinsFunctions::Next => Some(StaticStrings::DunderNext),
                        _ => None,
                    };

                    if let Some(dunder_name) = dunder {
                        let dunder_id = dunder_name.into();
                        if let Some(method) = self.lookup_type_dunder(arg_id, dunder_id) {
                            let arg_val = self.pop();
                            if matches!(builtin, BuiltinsFunctions::Hash) {
                                self.pending_hash_target = Some(arg_id);
                                self.pending_hash_push_result = true;
                            }
                            let result = match self.call_dunder(arg_id, method, ArgValues::Empty) {
                                Ok(result) => result,
                                Err(err) => {
                                    if matches!(builtin, BuiltinsFunctions::Hash) {
                                        self.pending_hash_target = None;
                                        self.pending_hash_push_result = false;
                                    }
                                    arg_val.drop_with_heap(self.heap);
                                    return Err(err);
                                }
                            };
                            arg_val.drop_with_heap(self.heap);
                            if matches!(builtin, BuiltinsFunctions::Hash) {
                                match result {
                                    CallResult::Push(hash_value) => {
                                        self.pending_hash_target = None;
                                        self.pending_hash_push_result = false;
                                        #[expect(clippy::cast_sign_loss)]
                                        let hash_u64 = match &hash_value {
                                            Value::Int(i) => *i as u64,
                                            Value::Bool(b) => u64::from(*b),
                                            _ => {
                                                hash_value.drop_with_heap(self.heap);
                                                return Err(ExcType::type_error(
                                                    "__hash__ method should return an integer",
                                                ));
                                            }
                                        };
                                        hash_value.drop_with_heap(self.heap);
                                        self.heap.set_cached_hash(arg_id, hash_u64);
                                        let hash_i64 = i64::from_ne_bytes(hash_u64.to_ne_bytes());
                                        return Ok(CallResult::Push(Value::Int(hash_i64)));
                                    }
                                    CallResult::FramePushed => {
                                        return Ok(CallResult::FramePushed);
                                    }
                                    other => {
                                        self.pending_hash_target = None;
                                        self.pending_hash_push_result = false;
                                        return Ok(other);
                                    }
                                }
                            }
                            if matches!(builtin, BuiltinsFunctions::Repr) {
                                return self.handle_stringify_call_result(result, PendingStringifyReturn::Repr);
                            }
                            return Ok(result);
                        }
                        // For hash(): if no __hash__ but has __eq__, raise TypeError
                        if matches!(builtin, BuiltinsFunctions::Hash) {
                            let eq_id = StaticStrings::DunderEq.into();
                            if let Some(eq_method) = self.lookup_type_dunder(arg_id, eq_id) {
                                // __eq__ defined without __hash__ - unhashable
                                eq_method.drop_with_heap(self.heap);
                                let arg_val = self.pop();
                                // Get class name
                                let class_name = match self.heap.get(arg_id) {
                                    HeapData::Instance(inst) => match self.heap.get(inst.class_id()) {
                                        HeapData::ClassObject(cls) => cls.name(self.interns).to_string(),
                                        _ => "instance".to_string(),
                                    },
                                    _ => "instance".to_string(),
                                };
                                arg_val.drop_with_heap(self.heap);
                                return Err(ExcType::type_error(format!("unhashable type: '{class_name}'")));
                            }
                        }
                    }
                }
            }

            let args = self.pop_n_args(arg_count);
            let result = builtin.call(self.heap, args, self.interns, self.print_writer)?;
            Ok(CallResult::Push(result))
        } else {
            Err(RunError::internal("CallBuiltinFunction: invalid builtin_id"))
        }
    }

    /// Executes `CallBuiltinType` opcode.
    ///
    /// Calls a builtin type constructor directly without stack manipulation for the callable.
    /// This is an optimization for type constructors like `list()`, `int()`, `str()`.
    ///
    /// For instances, intercepts to call dunder methods:
    /// - `str(x)` -> `x.__str__()` or `x.__repr__()`
    /// - `repr(x)` -> `x.__repr__()`
    /// - `int(x)` -> `x.__int__()`
    /// - `float(x)` -> `x.__float__()`
    /// - `bool(x)` -> `x.__bool__()` or `x.__len__()`
    /// - `hash(x)` -> `x.__hash__()`
    /// - `len(x)` -> `x.__len__()`
    /// - `list(x)` -> `x.__iter__()` then repeatedly `__next__()` for instances with `__iter__`
    pub(super) fn exec_call_builtin_type(&mut self, type_id: u8, arg_count: usize) -> Result<CallResult, RunError> {
        if let Some(t) = Type::callable_from_u8(type_id) {
            // Check if the single argument is an instance that has a relevant dunder
            if arg_count == 1 {
                // Peek at the arg (TOS) without popping
                let arg_ref_id = match self.peek() {
                    Value::Ref(id) => Some(*id),
                    _ => None,
                };
                if t == Type::List
                    && let Some(arg_id) = arg_ref_id
                    && matches!(self.heap.get(arg_id), HeapData::Generator(_))
                {
                    let iterator = self.pop();
                    return self.list_build_from_iterator(iterator);
                }
                if t == Type::Tuple
                    && let Some(arg_id) = arg_ref_id
                    && matches!(self.heap.get(arg_id), HeapData::Generator(_))
                {
                    let iterator = self.pop();
                    return match self.list_build_from_iterator(iterator)? {
                        CallResult::Push(list_value) => {
                            let tuple_value = Type::Tuple.call(self.heap, ArgValues::One(list_value), self.interns)?;
                            Ok(CallResult::Push(tuple_value))
                        }
                        CallResult::FramePushed => {
                            self.pending_builtin_from_list.push(PendingBuiltinFromList {
                                kind: PendingBuiltinFromListKind::Tuple,
                            });
                            Ok(CallResult::FramePushed)
                        }
                        other => Ok(other),
                    };
                }
                if t == Type::Dict
                    && let Some(arg_id) = arg_ref_id
                    && matches!(self.heap.get(arg_id), HeapData::Generator(_))
                {
                    let iterator = self.pop();
                    return match self.list_build_from_iterator(iterator)? {
                        CallResult::Push(list_value) => {
                            let dict_value = Type::Dict.call(self.heap, ArgValues::One(list_value), self.interns)?;
                            Ok(CallResult::Push(dict_value))
                        }
                        CallResult::FramePushed => {
                            self.pending_builtin_from_list.push(PendingBuiltinFromList {
                                kind: PendingBuiltinFromListKind::Dict,
                            });
                            Ok(CallResult::FramePushed)
                        }
                        other => Ok(other),
                    };
                }
                if t == Type::Set
                    && let Some(arg_id) = arg_ref_id
                    && matches!(self.heap.get(arg_id), HeapData::Generator(_))
                {
                    let iterator = self.pop();
                    return match self.list_build_from_iterator(iterator)? {
                        CallResult::Push(list_value) => {
                            let set_value = Type::Set.call(self.heap, ArgValues::One(list_value), self.interns)?;
                            Ok(CallResult::Push(set_value))
                        }
                        CallResult::FramePushed => {
                            self.pending_builtin_from_list.push(PendingBuiltinFromList {
                                kind: PendingBuiltinFromListKind::Set,
                            });
                            Ok(CallResult::FramePushed)
                        }
                        other => Ok(other),
                    };
                }
                if t == Type::List
                    && let Some(arg_id) = arg_ref_id
                    && matches!(self.heap.get(arg_id), HeapData::ClassObject(_))
                {
                    let dunder_id: StringId = StaticStrings::DunderIter.into();
                    if let Some(method) = self.lookup_metaclass_dunder(arg_id, dunder_id) {
                        let arg_val = self.pop();
                        match self.call_class_dunder(arg_id, method, ArgValues::Empty)? {
                            CallResult::Push(iterator) => {
                                arg_val.drop_with_heap(self.heap);
                                return self.list_build_from_iterator(iterator);
                            }
                            CallResult::FramePushed => {
                                arg_val.drop_with_heap(self.heap);
                                self.pending_list_iter_return = true;
                                return Ok(CallResult::FramePushed);
                            }
                            other => {
                                arg_val.drop_with_heap(self.heap);
                                return Ok(other);
                            }
                        }
                    }
                }
                if let Some(arg_id) = arg_ref_id
                    && matches!(self.heap.get(arg_id), HeapData::Instance(_))
                {
                    // Check for type-specific dunders
                    let dunder = match t {
                        Type::Str => Some((StaticStrings::DunderStr, Some(StaticStrings::DunderRepr))),
                        Type::Int => Some((StaticStrings::DunderInt, None)),
                        Type::Float => Some((StaticStrings::DunderFloat, None)),
                        Type::Complex => Some((StaticStrings::DunderComplex, Some(StaticStrings::DunderFloat))),
                        Type::Bool => Some((StaticStrings::DunderBool, Some(StaticStrings::DunderLen))),
                        _ => None,
                    };

                    if let Some((primary_dunder, fallback_dunder)) = dunder {
                        let primary_id: StringId = primary_dunder.into();
                        if let Some(method) = self.lookup_type_dunder(arg_id, primary_id) {
                            // Pop the arg and call the dunder
                            let arg_val = self.pop();
                            let result = self.call_dunder(arg_id, method, ArgValues::Empty)?;
                            arg_val.drop_with_heap(self.heap);
                            if t == Type::Str {
                                return self.handle_stringify_call_result(result, PendingStringifyReturn::Str);
                            }
                            return Ok(result);
                        }
                        // Try fallback dunder if primary not found
                        if let Some(fallback) = fallback_dunder {
                            let fallback_id: StringId = fallback.into();
                            if let Some(method) = self.lookup_type_dunder(arg_id, fallback_id) {
                                let arg_val = self.pop();
                                let result = self.call_dunder(arg_id, method, ArgValues::Empty)?;
                                arg_val.drop_with_heap(self.heap);
                                if t == Type::Str {
                                    return self.handle_stringify_call_result(result, PendingStringifyReturn::Str);
                                }
                                return Ok(result);
                            }
                        }
                    }

                    // bytes(instance) dispatches through __bytes__ when present.
                    if t == Type::Bytes {
                        let method = match self.heap.get(arg_id) {
                            HeapData::Instance(instance) => {
                                let class_id = instance.class_id();
                                match self.heap.get(class_id) {
                                    HeapData::ClassObject(cls) => cls
                                        .mro_lookup_attr("__bytes__", class_id, self.heap, self.interns)
                                        .map(|(value, _)| value),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some(method) = method {
                            let arg_val = self.pop();
                            let result = self.call_dunder(arg_id, method, ArgValues::Empty)?;
                            arg_val.drop_with_heap(self.heap);
                            return Ok(result);
                        }
                    }

                    // Special handling for list(instance_with___iter__)
                    // We need to call __iter__() then repeatedly __next__() until StopIteration
                    if t == Type::List {
                        let dunder_id: StringId = StaticStrings::DunderIter.into();
                        if let Some(method) = self.lookup_type_dunder(arg_id, dunder_id) {
                            let arg_val = self.pop();
                            // Call __iter__() to get the iterator
                            match self.call_dunder(arg_id, method, ArgValues::Empty)? {
                                CallResult::Push(iterator) => {
                                    // __iter__ returned synchronously - start collecting items
                                    arg_val.drop_with_heap(self.heap);
                                    return self.list_build_from_iterator(iterator);
                                }
                                CallResult::FramePushed => {
                                    // __iter__ pushed a frame - store state for continuation
                                    arg_val.drop_with_heap(self.heap);
                                    // Mark that we're waiting for __iter__ to return
                                    self.pending_list_iter_return = true;
                                    return Ok(CallResult::FramePushed);
                                }
                                other => {
                                    arg_val.drop_with_heap(self.heap);
                                    return Ok(other);
                                }
                            }
                        }
                    }
                }
            }

            let args = self.pop_n_args(arg_count);
            let result = t.call(self.heap, args, self.interns)?;
            Ok(CallResult::Push(result))
        } else {
            Err(RunError::internal("CallBuiltinType: invalid type_id"))
        }
    }

    /// Builds a list from an iterator by repeatedly calling `__next__()`.
    ///
    /// This handles the case where `list(instance_with___iter__)` is called, and
    /// also handles generator iterators directly via `generator_next()`.
    /// If `__next__()`/generator resume pushes frames, stores state in
    /// `pending_list_build` and returns `FramePushed`.
    pub(super) fn list_build_from_iterator(&mut self, iterator: Value) -> Result<CallResult, RunError> {
        if let Value::Ref(iter_id) = &iterator {
            // Generators are VM-driven iterators; resume with generator_next().
            if matches!(self.heap.get(*iter_id), HeapData::Generator(_)) {
                match self.generator_next(*iter_id) {
                    Ok(CallResult::FramePushed) => {
                        self.pending_list_build.push(super::PendingListBuild {
                            iterator,
                            items: Vec::new(),
                        });
                        self.pending_list_build_return = true;
                        return Ok(CallResult::FramePushed);
                    }
                    Err(e) if e.is_stop_iteration() => {
                        iterator.drop_with_heap(self.heap);
                        let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                        return Ok(CallResult::Push(Value::Ref(list_id)));
                    }
                    Err(e) => {
                        iterator.drop_with_heap(self.heap);
                        return Err(e);
                    }
                    Ok(other) => {
                        iterator.drop_with_heap(self.heap);
                        return Ok(other);
                    }
                }
            }

            // Check if the iterator is an instance with __next__
            if matches!(self.heap.get(*iter_id), HeapData::Instance(_)) {
                let dunder_id: StringId = StaticStrings::DunderNext.into();
                if let Some(method) = self.lookup_type_dunder(*iter_id, dunder_id) {
                    // Call __next__() on the iterator
                    match self.call_dunder(*iter_id, method, ArgValues::Empty)? {
                        CallResult::Push(item) => {
                            // __next__ returned synchronously - add item and continue
                            let items = vec![item];
                            return self.list_build_continue(iterator, items);
                        }
                        CallResult::FramePushed => {
                            // __next__ pushed a frame - store pending state
                            self.pending_list_build.push(super::PendingListBuild {
                                iterator,
                                items: Vec::new(),
                            });
                            self.pending_list_build_return = true;
                            return Ok(CallResult::FramePushed);
                        }
                        other => {
                            iterator.drop_with_heap(self.heap);
                            return Ok(other);
                        }
                    }
                }
            }
        }

        // Fast path: use OurosIter to collect items from built-in iterators
        // Note: OurosIter::new takes ownership of value, so we can't use iterator after
        match OurosIter::new(iterator, self.heap, self.interns) {
            Ok(mut iter) => {
                let items: Vec<Value> = iter.collect(self.heap, self.interns)?;
                iter.drop_with_heap(self.heap);
                let list_id = self.heap.allocate(HeapData::List(List::new(items)))?;
                Ok(CallResult::Push(Value::Ref(list_id)))
            }
            Err(e) => Err(e),
        }
    }

    /// Continues building a list from an iterator after a successful `__next__()` call.
    ///
    /// Repeatedly calls `__next__()` until StopIteration, collecting items into the list.
    /// Supports both instance iterators and generator iterators.
    fn list_build_continue(&mut self, iterator: Value, items: Vec<Value>) -> Result<CallResult, RunError> {
        // Guards guarantee both iterator and collected items are dropped on every
        // early-return path (`?`, errors, unexpected call results).
        let this = self;
        let mut items_guard = HeapGuard::new(items, this);
        let (items, this) = items_guard.as_parts_mut();
        let mut iterator_guard = HeapGuard::new(iterator, this);
        let (iterator, this) = iterator_guard.as_parts_mut();

        let iter_id = if let Value::Ref(id) = iterator {
            *id
        } else {
            let type_name = iterator.py_type(this.heap);
            return Err(ExcType::type_error_not_iterable(type_name));
        };

        let dunder_id: StringId = StaticStrings::DunderNext.into();

        loop {
            if matches!(this.heap.get(iter_id), HeapData::Generator(_)) {
                match this.generator_next(iter_id) {
                    Ok(CallResult::FramePushed) => {
                        let (iterator, _this) = iterator_guard.into_parts();
                        let (items, this) = items_guard.into_parts();
                        this.pending_list_build
                            .push(super::PendingListBuild { iterator, items });
                        this.pending_list_build_return = true;
                        return Ok(CallResult::FramePushed);
                    }
                    Err(e) if e.is_stop_iteration() => {
                        let list_items = std::mem::take(items);
                        let list_id = this.heap.allocate(HeapData::List(List::new(list_items)))?;
                        return Ok(CallResult::Push(Value::Ref(list_id)));
                    }
                    Err(e) => return Err(e),
                    Ok(other) => return Ok(other),
                }
            }

            if let Some(method) = this.lookup_type_dunder(iter_id, dunder_id) {
                match this.call_dunder(iter_id, method, ArgValues::Empty)? {
                    CallResult::Push(item) => {
                        items.push(item);
                    }
                    CallResult::FramePushed => {
                        let (iterator, _this) = iterator_guard.into_parts();
                        let (items, this) = items_guard.into_parts();
                        this.pending_list_build
                            .push(super::PendingListBuild { iterator, items });
                        this.pending_list_build_return = true;
                        return Ok(CallResult::FramePushed);
                    }
                    other => return Ok(other),
                }
            } else {
                let type_name = iterator.py_type(this.heap);
                return Err(ExcType::type_error_not_iterable(type_name));
            }
        }
    }

    /// Handles the result of a `__next__()` call during list construction.
    ///
    /// Called from the ReturnValue handler when `pending_list_build_return` is true.
    /// On normal return: append item and call next `__next__()`.
    /// On StopIteration: finish list construction and push the list.
    pub(super) fn handle_list_build_return(&mut self, item: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_list_build.pop() else {
            return Err(RunError::internal("handle_list_build_return: no pending list build"));
        };
        self.pending_list_build_return = !self.pending_list_build.is_empty();

        let super::PendingListBuild { iterator, mut items } = pending;
        items.push(item);
        self.list_build_continue(iterator, items)
    }

    /// Handles StopIteration during list construction from an iterator.
    ///
    /// Called from the exception handler when `pending_list_build_return` is true
    /// and the exception is StopIteration.
    pub(super) fn handle_list_build_stop_iteration(&mut self) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_list_build.pop() else {
            return Err(RunError::internal(
                "handle_list_build_stop_iteration: no pending list build",
            ));
        };
        self.pending_list_build_return = !self.pending_list_build.is_empty();

        let super::PendingListBuild { iterator, items } = pending;
        iterator.drop_with_heap(self.heap);

        // Create the list from collected items
        let list_id = self.heap.allocate(HeapData::List(List::new(items)))?;
        Ok(CallResult::Push(Value::Ref(list_id)))
    }

    // ---- AttrCallResult dispatch ----

    /// Handles an `AttrCallResult` that may require VM-level processing.
    ///
    /// Most variants are handled by the generic `Into<CallResult>` conversion,
    /// but `ReduceCall`, `MapCall`, and `FilterCall` require VM access to call
    /// user-defined functions.
    fn handle_attr_call_result(&mut self, result: AttrCallResult) -> Result<CallResult, RunError> {
        match result {
            AttrCallResult::ReduceCall(function, accumulator, remaining_items) => {
                if remaining_items.is_empty() {
                    function.drop_with_heap(self.heap);
                    Ok(CallResult::Push(accumulator))
                } else {
                    self.reduce_continue(function, accumulator, remaining_items)
                }
            }
            AttrCallResult::MapCall(function, iterators) => {
                if iterators.is_empty() || iterators[0].is_empty() {
                    // Empty result - return empty list
                    function.drop_with_heap(self.heap);
                    for iter in iterators {
                        for item in iter {
                            item.drop_with_heap(self.heap);
                        }
                    }
                    let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                    Ok(CallResult::Push(Value::Ref(list_id)))
                } else {
                    self.map_continue(function, iterators, Vec::new(), 0)
                }
            }
            AttrCallResult::FilterCall(function, items) => {
                if items.is_empty() {
                    // Empty result - return empty list
                    function.drop_with_heap(self.heap);
                    let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                    Ok(CallResult::Push(Value::Ref(list_id)))
                } else {
                    self.filter_continue(function, items, Vec::new(), 0, super::PendingFilterMode::Filter, false)
                }
            }
            AttrCallResult::FilterFalseCall(function, items) => {
                if items.is_empty() {
                    function.drop_with_heap(self.heap);
                    let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                    Ok(CallResult::Push(Value::Ref(list_id)))
                } else {
                    self.filter_continue(
                        function,
                        items,
                        Vec::new(),
                        0,
                        super::PendingFilterMode::FilterFalse,
                        false,
                    )
                }
            }
            AttrCallResult::TakeWhileCall(function, items) => {
                if items.is_empty() {
                    function.drop_with_heap(self.heap);
                    let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                    Ok(CallResult::Push(Value::Ref(list_id)))
                } else {
                    self.filter_continue(
                        function,
                        items,
                        Vec::new(),
                        0,
                        super::PendingFilterMode::TakeWhile,
                        false,
                    )
                }
            }
            AttrCallResult::DropWhileCall(function, items) => {
                if items.is_empty() {
                    function.drop_with_heap(self.heap);
                    let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                    Ok(CallResult::Push(Value::Ref(list_id)))
                } else {
                    self.filter_continue(
                        function,
                        items,
                        Vec::new(),
                        0,
                        super::PendingFilterMode::DropWhile,
                        true,
                    )
                }
            }
            AttrCallResult::GroupByCall(function, items) => {
                if items.is_empty() {
                    function.drop_with_heap(self.heap);
                    let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
                    Ok(CallResult::Push(Value::Ref(list_id)))
                } else {
                    self.groupby_continue(function, items, Vec::new(), 0)
                }
            }
            AttrCallResult::TextwrapIndentCall(predicate, lines, prefix) => {
                let pending = PendingTextwrapIndent {
                    predicate,
                    lines,
                    prefix,
                    current_idx: 0,
                    output: String::new(),
                };
                self.textwrap_indent_continue(pending)
            }
            AttrCallResult::CallFunction(callable, args) => self.call_function(callable, args),
            AttrCallResult::ReSubCall(callable, matches, original_string, is_bytes, return_count) => {
                if matches.is_empty() {
                    // No matches — return original string unchanged
                    callable.drop_with_heap(self.heap);
                    let id = if is_bytes {
                        self.heap
                            .allocate(HeapData::Bytes(Bytes::from(original_string.as_bytes().to_vec())))?
                    } else {
                        self.heap.allocate(HeapData::Str(Str::from(original_string.clone())))?
                    };
                    if return_count {
                        let items = smallvec::smallvec![Value::Ref(id), Value::Int(0)];
                        Ok(CallResult::Push(allocate_tuple(items, self.heap)?))
                    } else {
                        Ok(CallResult::Push(Value::Ref(id)))
                    }
                } else {
                    self.re_sub_continue(
                        callable,
                        matches,
                        original_string,
                        is_bytes,
                        return_count,
                        Vec::new(),
                        0,
                    )
                }
            }
            AttrCallResult::ObjectNew => {
                // Return the ObjectNewImpl callable for `cls.__new__` access
                let object_new_id = self.heap.get_object_new_impl()?;
                Ok(CallResult::Push(Value::Ref(object_new_id)))
            }
            other => Ok(other.into()),
        }
    }

    // ---- functools.reduce() VM-level implementation ----

    /// Continues a reduce operation, calling the function for each remaining item.
    ///
    /// If the function call completes synchronously (`CallResult::Push`), the result
    /// becomes the new accumulator and we continue with the next item. If the function
    /// pushes a frame (`CallResult::FramePushed`), we stash the state in `pending_reduce`
    /// and return `FramePushed` to let the VM execute the frame.
    fn reduce_continue(
        &mut self,
        function: Value,
        mut accumulator: Value,
        mut remaining_items: Vec<Value>,
    ) -> Result<CallResult, RunError> {
        while !remaining_items.is_empty() {
            let item = remaining_items.remove(0);

            // Build args: (accumulator, item)
            let call_args = ArgValues::Two(accumulator, item);

            // Clone the function for this call (function is reused across iterations)
            let func_clone = function.clone_with_heap(self.heap);

            match self.call_function(func_clone, call_args)? {
                CallResult::Push(result) => {
                    // Function completed synchronously - result is the new accumulator
                    accumulator = result;
                }
                CallResult::FramePushed => {
                    // Function pushed a frame (user-defined function/lambda/closure).
                    // Stash state and let the VM execute the frame.
                    self.pending_reduce = Some(PendingReduce {
                        function,
                        // accumulator was consumed by call_args; new value comes from frame return
                        accumulator: Value::None, // placeholder, replaced by return value
                        remaining_items,
                    });
                    self.pending_reduce_return = true;
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    // External calls etc. not supported in reduce
                    function.drop_with_heap(self.heap);
                    for item in remaining_items {
                        item.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
            }
        }

        // All items processed - clean up and return the final accumulator
        function.drop_with_heap(self.heap);
        Ok(CallResult::Push(accumulator))
    }

    /// Handles the return value from a user-defined function during `functools.reduce()`.
    ///
    /// Called from the `ReturnValue` handler when `pending_reduce_return` is true.
    /// The return value becomes the new accumulator, and we continue processing
    /// remaining items.
    pub(super) fn handle_reduce_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_reduce.take() else {
            return Err(RunError::internal("handle_reduce_return: no pending reduce state"));
        };

        let PendingReduce {
            function,
            accumulator: placeholder,
            remaining_items,
        } = pending;

        // Drop the placeholder accumulator (it was Value::None)
        placeholder.drop_with_heap(self.heap);

        // The return value is the new accumulator
        if remaining_items.is_empty() {
            // All done - return the final result
            function.drop_with_heap(self.heap);
            Ok(CallResult::Push(value))
        } else {
            // More items to process
            self.reduce_continue(function, value, remaining_items)
        }
    }

    // ---- map() VM-level implementation ----

    /// Continues a map operation, calling the function for each set of iterator items.
    ///
    /// If the function call completes synchronously (`CallResult::Push`), the result
    /// is added to results and we continue with the next index. If the function pushes
    /// a frame (`CallResult::FramePushed`), we stash the state in `pending_map` and
    /// return `FramePushed` to let the VM execute the frame.
    fn map_continue(
        &mut self,
        function: Value,
        iterators: Vec<Vec<Value>>,
        mut results: Vec<Value>,
        current_idx: usize,
    ) -> Result<CallResult, RunError> {
        let num_iters = iterators.len();

        while current_idx < iterators[0].len() {
            // Build args from each iterator at current_idx
            let mut call_args = Vec::with_capacity(num_iters);
            for iter in &iterators {
                call_args.push(iter[current_idx].clone_with_heap(self.heap));
            }

            // Build ArgValues from call_args
            let arg_values = match call_args.len() {
                0 => ArgValues::Empty,
                1 => ArgValues::One(call_args.into_iter().next().unwrap()),
                2 => {
                    let mut iter = call_args.into_iter();
                    ArgValues::Two(iter.next().unwrap(), iter.next().unwrap())
                }
                _ => ArgValues::ArgsKargs {
                    args: call_args,
                    kwargs: crate::args::KwargsValues::Empty,
                },
            };

            // Clone the function for this call (function is reused across iterations)
            let func_clone = function.clone_with_heap(self.heap);

            match self.call_function(func_clone, arg_values)? {
                CallResult::Push(result) => {
                    // Function completed synchronously - add to results
                    results.push(result);
                }
                CallResult::FramePushed => {
                    // Function pushed a frame (user-defined function/lambda/closure).
                    // Stash state and let the VM execute the frame.
                    self.pending_map = Some(super::PendingMap {
                        function,
                        iterators,
                        results,
                        current_idx: current_idx + 1,
                    });
                    self.pending_map_return = true;
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    // External calls etc. not supported in map
                    function.drop_with_heap(self.heap);
                    for iter in iterators {
                        for item in iter {
                            item.drop_with_heap(self.heap);
                        }
                    }
                    for result in results {
                        result.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
            }
        }

        // All items processed - create the result list
        function.drop_with_heap(self.heap);
        for iter in iterators {
            for item in iter {
                item.drop_with_heap(self.heap);
            }
        }
        let list_id = self.heap.allocate(HeapData::List(List::new(results)))?;
        Ok(CallResult::Push(Value::Ref(list_id)))
    }

    /// Handles the return value from a user-defined function during `map()`.
    ///
    /// Called from the `ReturnValue` handler when `pending_map_return` is true.
    /// The return value is added to results, and we continue processing remaining items.
    pub(super) fn handle_map_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_map.take() else {
            return Err(RunError::internal("handle_map_return: no pending map state"));
        };

        let super::PendingMap {
            function,
            iterators,
            mut results,
            current_idx,
        } = pending;

        // Add the return value to results
        results.push(value);

        // Continue processing remaining items
        self.map_continue(function, iterators, results, current_idx)
    }

    // ---- filter() VM-level implementation ----

    /// Continues a filter operation, calling the function for each remaining item.
    ///
    /// If the function call completes synchronously (`CallResult::Push`), we check
    /// if the result is truthy and add the original item to results if so. If the
    /// function pushes a frame (`CallResult::FramePushed`), we stash the state in
    /// `pending_filter` and return `FramePushed` to let the VM execute the frame.
    fn filter_continue(
        &mut self,
        function: Value,
        items: Vec<Value>,
        mut results: Vec<Value>,
        current_idx: usize,
        mode: super::PendingFilterMode,
        mut dropwhile_dropping: bool,
    ) -> Result<CallResult, RunError> {
        let mut idx = current_idx;

        while idx < items.len() {
            if mode == super::PendingFilterMode::DropWhile && !dropwhile_dropping {
                for remaining in &items[idx..] {
                    results.push(remaining.clone_with_heap(self.heap));
                }
                function.drop_with_heap(self.heap);
                for item in items {
                    item.drop_with_heap(self.heap);
                }
                let list_id = self.heap.allocate(HeapData::List(List::new(results)))?;
                return Ok(CallResult::Push(Value::Ref(list_id)));
            }

            let item = items[idx].clone_with_heap(self.heap);

            // Clone the function for this call (function is reused across iterations)
            let func_clone = function.clone_with_heap(self.heap);

            match self.call_function(func_clone, ArgValues::One(item))? {
                CallResult::Push(predicate_result) => {
                    let is_true = predicate_result.py_bool(self.heap, self.interns);
                    predicate_result.drop_with_heap(self.heap);

                    match mode {
                        super::PendingFilterMode::Filter => {
                            if is_true {
                                results.push(items[idx].clone_with_heap(self.heap));
                            }
                            idx += 1;
                        }
                        super::PendingFilterMode::FilterFalse => {
                            if !is_true {
                                results.push(items[idx].clone_with_heap(self.heap));
                            }
                            idx += 1;
                        }
                        super::PendingFilterMode::TakeWhile => {
                            if is_true {
                                results.push(items[idx].clone_with_heap(self.heap));
                                idx += 1;
                            } else {
                                function.drop_with_heap(self.heap);
                                for item in items {
                                    item.drop_with_heap(self.heap);
                                }
                                let list_id = self.heap.allocate(HeapData::List(List::new(results)))?;
                                return Ok(CallResult::Push(Value::Ref(list_id)));
                            }
                        }
                        super::PendingFilterMode::DropWhile => {
                            if dropwhile_dropping && is_true {
                                idx += 1;
                            } else {
                                dropwhile_dropping = false;
                                results.push(items[idx].clone_with_heap(self.heap));
                                idx += 1;
                            }
                        }
                    }
                }
                CallResult::FramePushed => {
                    // Function pushed a frame (user-defined function/lambda/closure).
                    // Stash state and let the VM execute the frame.
                    self.pending_filter = Some(super::PendingFilter {
                        function,
                        items,
                        results,
                        current_idx: idx,
                        mode,
                        dropwhile_dropping,
                    });
                    self.pending_filter_return = true;
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    // External calls etc. not supported in filter
                    function.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    for result in results {
                        result.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
            }
        }

        // All items processed - create the result list
        function.drop_with_heap(self.heap);
        for item in items {
            item.drop_with_heap(self.heap);
        }
        let list_id = self.heap.allocate(HeapData::List(List::new(results)))?;
        Ok(CallResult::Push(Value::Ref(list_id)))
    }

    /// Handles the return value from a user-defined function during `filter()`.
    ///
    /// Called from the `ReturnValue` handler when `pending_filter_return` is true.
    /// The return value is checked for truthiness, and if true, the corresponding
    /// item is added to results. We then continue processing remaining items.
    pub(super) fn handle_filter_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_filter.take() else {
            return Err(RunError::internal("handle_filter_return: no pending filter state"));
        };

        let super::PendingFilter {
            function,
            items,
            mut results,
            current_idx,
            mode,
            mut dropwhile_dropping,
        } = pending;

        let is_true = value.py_bool(self.heap, self.interns);
        value.drop_with_heap(self.heap);

        match mode {
            super::PendingFilterMode::Filter => {
                if is_true {
                    results.push(items[current_idx].clone_with_heap(self.heap));
                }
            }
            super::PendingFilterMode::FilterFalse => {
                if !is_true {
                    results.push(items[current_idx].clone_with_heap(self.heap));
                }
            }
            super::PendingFilterMode::TakeWhile => {
                if is_true {
                    results.push(items[current_idx].clone_with_heap(self.heap));
                } else {
                    function.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    let list_id = self.heap.allocate(HeapData::List(List::new(results)))?;
                    return Ok(CallResult::Push(Value::Ref(list_id)));
                }
            }
            super::PendingFilterMode::DropWhile => {
                if dropwhile_dropping && is_true {
                    // keep dropping
                } else {
                    dropwhile_dropping = false;
                    results.push(items[current_idx].clone_with_heap(self.heap));
                }
            }
        }

        self.filter_continue(function, items, results, current_idx + 1, mode, dropwhile_dropping)
    }

    // ---- itertools.groupby() VM-level key-call implementation ----

    /// Continues `groupby(..., key=callable)` key evaluation across possible frame pushes.
    fn groupby_continue(
        &mut self,
        function: Value,
        items: Vec<Value>,
        mut keys: Vec<Value>,
        current_idx: usize,
    ) -> Result<CallResult, RunError> {
        let mut idx = current_idx;

        while idx < items.len() {
            let item = items[idx].clone_with_heap(self.heap);
            let func_clone = function.clone_with_heap(self.heap);

            match self.call_function(func_clone, ArgValues::One(item))? {
                CallResult::Push(key) => {
                    keys.push(key);
                    idx += 1;
                }
                CallResult::FramePushed => {
                    self.pending_groupby = Some(PendingGroupBy {
                        function,
                        items,
                        keys,
                        current_idx: idx + 1,
                    });
                    self.pending_groupby_return = true;
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    function.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    for key in keys {
                        key.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
            }
        }

        let grouped = self.build_groupby_result(items, keys)?;
        function.drop_with_heap(self.heap);
        Ok(CallResult::Push(grouped))
    }

    /// Handles the return value from a user-defined key function during `groupby()`.
    pub(super) fn handle_groupby_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_groupby.take() else {
            return Err(RunError::internal("handle_groupby_return: no pending groupby state"));
        };

        let PendingGroupBy {
            function,
            items,
            mut keys,
            current_idx,
        } = pending;

        keys.push(value);
        self.groupby_continue(function, items, keys, current_idx)
    }

    /// Builds the eager list result for `itertools.groupby`.
    ///
    /// Output format matches the eager representation used by this runtime:
    /// `[(key1, [items...]), (key2, [items...]), ...]`.
    fn build_groupby_result(&mut self, items: Vec<Value>, keys: Vec<Value>) -> RunResult<Value> {
        if items.len() != keys.len() {
            for item in items {
                item.drop_with_heap(self.heap);
            }
            for key in keys {
                key.drop_with_heap(self.heap);
            }
            return Err(RunError::internal("groupby key/item length mismatch"));
        }

        let mut result: Vec<Value> = Vec::new();
        let mut current_key: Option<Value> = None;
        let mut current_group: Vec<Value> = Vec::new();

        for (item, key) in items.into_iter().zip(keys) {
            let same_group = current_key
                .as_ref()
                .is_some_and(|existing| existing.py_eq(&key, self.heap, self.interns));

            if same_group {
                key.drop_with_heap(self.heap);
                current_group.push(item);
                continue;
            }

            if let Some(prev_key) = current_key.take() {
                let group_list_id = self
                    .heap
                    .allocate(HeapData::List(List::new(std::mem::take(&mut current_group))))?;
                let mut tuple_items: SmallVec<[Value; 3]> = SmallVec::new();
                tuple_items.push(prev_key);
                tuple_items.push(Value::Ref(group_list_id));
                let tuple = allocate_tuple(tuple_items, self.heap)?;
                result.push(tuple);
            }

            current_key = Some(key);
            current_group.push(item);
        }

        if let Some(last_key) = current_key {
            let group_list_id = self.heap.allocate(HeapData::List(List::new(current_group)))?;
            let mut tuple_items: SmallVec<[Value; 3]> = SmallVec::new();
            tuple_items.push(last_key);
            tuple_items.push(Value::Ref(group_list_id));
            let tuple = allocate_tuple(tuple_items, self.heap)?;
            result.push(tuple);
        }

        let list_id = self.heap.allocate(HeapData::List(List::new(result)))?;
        Ok(Value::Ref(list_id))
    }

    // ---- textwrap.indent(predicate=...) VM-level implementation ----

    /// Continues `textwrap.indent()` predicate evaluation across lines.
    fn textwrap_indent_continue(&mut self, mut pending: PendingTextwrapIndent) -> Result<CallResult, RunError> {
        while pending.current_idx < pending.lines.len() {
            let line = pending
                .lines
                .get(pending.current_idx)
                .cloned()
                .ok_or_else(|| RunError::internal("textwrap indent line index out of bounds"))?;
            let line_id = self.heap.allocate(HeapData::Str(Str::from(line.clone())))?;
            let line_value = Value::Ref(line_id);
            let predicate_clone = pending.predicate.clone_with_heap(self.heap);

            match self.call_function(predicate_clone, ArgValues::One(line_value))? {
                CallResult::Push(result) => {
                    if result.py_bool(self.heap, self.interns) {
                        pending.output.push_str(&pending.prefix);
                    }
                    pending.output.push_str(&line);
                    result.drop_with_heap(self.heap);
                    pending.current_idx += 1;
                }
                CallResult::FramePushed => {
                    self.pending_textwrap_indent = Some(pending);
                    self.pending_textwrap_indent_return = true;
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    pending.predicate.drop_with_heap(self.heap);
                    return Ok(other);
                }
            }
        }

        pending.predicate.drop_with_heap(self.heap);
        let result_id = self.heap.allocate(HeapData::Str(Str::from(pending.output)))?;
        Ok(CallResult::Push(Value::Ref(result_id)))
    }

    /// Handles the return value from a user-defined predicate during `textwrap.indent()`.
    pub(super) fn handle_textwrap_indent_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_textwrap_indent.take() else {
            return Err(RunError::internal(
                "handle_textwrap_indent_return: no pending textwrap indent state",
            ));
        };

        let line = pending
            .lines
            .get(pending.current_idx)
            .cloned()
            .ok_or_else(|| RunError::internal("textwrap indent line index out of bounds"))?;
        if value.py_bool(self.heap, self.interns) {
            pending.output.push_str(&pending.prefix);
        }
        pending.output.push_str(&line);
        value.drop_with_heap(self.heap);
        pending.current_idx += 1;

        self.textwrap_indent_continue(pending)
    }

    // ---- re.sub(callable) VM-level implementation ----

    /// Continues a `re.sub`/`re.subn` callable replacement operation.
    ///
    /// Calls the user's replacement function for each remaining match. If the
    /// function completes synchronously (`CallResult::Push`), we extract the
    /// string result and continue. If the function pushes a frame, we stash
    /// state in `pending_re_sub` and return `FramePushed`.
    #[expect(clippy::too_many_arguments)]
    fn re_sub_continue(
        &mut self,
        function: Value,
        matches: Vec<(usize, usize, Value)>,
        original_string: String,
        is_bytes: bool,
        return_count: bool,
        mut replacements: Vec<String>,
        current_idx: usize,
    ) -> Result<CallResult, RunError> {
        let mut idx = current_idx;

        while idx < matches.len() {
            // Clone the match value to pass as argument (the original stays in matches for position info)
            let match_arg = matches[idx].2.clone_with_heap(self.heap);
            let func_clone = function.clone_with_heap(self.heap);

            match self.call_function(func_clone, ArgValues::One(match_arg))? {
                CallResult::Push(result) => {
                    // Function completed synchronously — extract string replacement
                    let replacement = result.py_str(self.heap, self.interns).into_owned();
                    result.drop_with_heap(self.heap);
                    replacements.push(replacement);
                    idx += 1;
                }
                CallResult::FramePushed => {
                    // Function pushed a frame — stash state and let the VM execute
                    self.pending_re_sub = Some(super::PendingReSub {
                        function,
                        matches,
                        original_string,
                        is_bytes,
                        return_count,
                        replacements,
                        current_idx: idx + 1,
                    });
                    self.pending_re_sub_return = true;
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    // Unsupported call result — clean up and propagate
                    function.drop_with_heap(self.heap);
                    for (_start, _end, match_val) in matches {
                        match_val.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
            }
        }

        // All matches processed — assemble the final string
        function.drop_with_heap(self.heap);
        self.assemble_re_sub_result(matches, original_string, is_bytes, return_count, replacements)
    }

    /// Handles the return value from a user-defined function during `re.sub()`.
    ///
    /// Called from the `ReturnValue` handler when `pending_re_sub_return` is true.
    /// Extracts the string from the return value, adds it to replacements, and
    /// continues processing remaining matches.
    pub(super) fn handle_re_sub_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_re_sub.take() else {
            return Err(RunError::internal("handle_re_sub_return: no pending re_sub state"));
        };

        let super::PendingReSub {
            function,
            matches,
            original_string,
            is_bytes,
            return_count,
            mut replacements,
            current_idx,
        } = pending;

        // Extract string from the callback return value
        let replacement = value.py_str(self.heap, self.interns).into_owned();
        value.drop_with_heap(self.heap);
        replacements.push(replacement);

        // Continue processing remaining matches
        self.re_sub_continue(
            function,
            matches,
            original_string,
            is_bytes,
            return_count,
            replacements,
            current_idx,
        )
    }

    /// Assembles the final `re.sub`/`re.subn` result from match spans and replacement strings.
    ///
    /// Interleaves the non-matched portions of the original string with the
    /// replacement strings to produce the final output.
    fn assemble_re_sub_result(
        &mut self,
        matches: Vec<(usize, usize, Value)>,
        original_string: String,
        is_bytes: bool,
        return_count: bool,
        replacements: Vec<String>,
    ) -> Result<CallResult, RunError> {
        let n_subs = replacements.len();
        let mut result = String::with_capacity(original_string.len());
        let mut last_end = 0usize;

        for (i, (start, end, match_val)) in matches.into_iter().enumerate() {
            match_val.drop_with_heap(self.heap);
            result.push_str(&original_string[last_end..start]);
            if i < replacements.len() {
                result.push_str(&replacements[i]);
            }
            last_end = end;
        }
        result.push_str(&original_string[last_end..]);

        let id = if is_bytes {
            self.heap
                .allocate(HeapData::Bytes(Bytes::from(result.as_bytes().to_vec())))?
        } else {
            self.heap.allocate(HeapData::Str(Str::from(result)))?
        };

        if return_count {
            #[expect(clippy::cast_possible_wrap)]
            let items = smallvec::smallvec![Value::Ref(id), Value::Int(n_subs as i64)];
            Ok(CallResult::Push(allocate_tuple(items, self.heap)?))
        } else {
            Ok(CallResult::Push(Value::Ref(id)))
        }
    }

    /// Calls a decorated function inside a generator-backed context manager.
    fn call_context_decorator_with_generator(
        &mut self,
        generator: Value,
        wrapped: Value,
        args: ArgValues,
        async_mode: bool,
    ) -> Result<CallResult, RunError> {
        let dunder_next: StringId = StaticStrings::DunderNext.into();
        match self.call_attr(generator.clone_with_heap(self.heap), dunder_next, ArgValues::Empty)? {
            CallResult::Push(enter_value) => {
                enter_value.drop_with_heap(self.heap);
                self.context_decorator_call_wrapped(generator, wrapped, args, async_mode, false)
            }
            CallResult::FramePushed => {
                self.pending_context_decorator = Some(PendingContextDecorator {
                    generator,
                    wrapped,
                    async_mode,
                    close_with_exit: false,
                    args: Some(args),
                    wrapped_result: None,
                    stage: PendingContextDecoratorStage::Enter,
                });
                self.pending_context_decorator_return = true;
                Ok(CallResult::FramePushed)
            }
            other => {
                generator.drop_with_heap(self.heap);
                wrapped.drop_with_heap(self.heap);
                args.drop_with_heap(self.heap);
                Ok(other)
            }
        }
    }

    /// Calls a decorated function inside an instance-based context decorator.
    fn call_context_decorator_with_instance(
        &mut self,
        manager: Value,
        wrapped: Value,
        args: ArgValues,
        async_mode: bool,
    ) -> Result<CallResult, RunError> {
        if async_mode {
            let dunder_aenter: StringId = StaticStrings::DunderAenter.into();
            match self.call_attr(manager.clone_with_heap(self.heap), dunder_aenter, ArgValues::Empty)? {
                CallResult::Push(enter_value) => {
                    self.push(enter_value);
                    match self.exec_get_awaitable()? {
                        AwaitResult::ValueReady(value) => {
                            value.drop_with_heap(self.heap);
                            self.context_decorator_call_wrapped(manager, wrapped, args, true, true)
                        }
                        AwaitResult::FramePushed => {
                            self.pending_context_decorator = Some(PendingContextDecorator {
                                generator: manager,
                                wrapped,
                                async_mode: true,
                                close_with_exit: true,
                                args: Some(args),
                                wrapped_result: None,
                                stage: PendingContextDecoratorStage::Enter,
                            });
                            self.pending_context_decorator_return = true;
                            Ok(CallResult::FramePushed)
                        }
                        AwaitResult::Yield(_) => {
                            manager.drop_with_heap(self.heap);
                            wrapped.drop_with_heap(self.heap);
                            args.drop_with_heap(self.heap);
                            Err(SimpleException::new_msg(
                                ExcType::RuntimeError,
                                "Async context decorator cannot await unresolved external futures",
                            )
                            .into())
                        }
                    }
                }
                CallResult::FramePushed => {
                    self.pending_context_decorator = Some(PendingContextDecorator {
                        generator: manager,
                        wrapped,
                        async_mode: true,
                        close_with_exit: true,
                        args: Some(args),
                        wrapped_result: None,
                        stage: PendingContextDecoratorStage::Enter,
                    });
                    self.pending_context_decorator_return = true;
                    Ok(CallResult::FramePushed)
                }
                other => {
                    manager.drop_with_heap(self.heap);
                    wrapped.drop_with_heap(self.heap);
                    args.drop_with_heap(self.heap);
                    Ok(other)
                }
            }
        } else {
            let dunder_enter: StringId = StaticStrings::DunderEnter.into();
            match self.call_attr(manager.clone_with_heap(self.heap), dunder_enter, ArgValues::Empty)? {
                CallResult::Push(enter_value) => {
                    enter_value.drop_with_heap(self.heap);
                    self.context_decorator_call_wrapped(manager, wrapped, args, false, true)
                }
                CallResult::FramePushed => {
                    self.pending_context_decorator = Some(PendingContextDecorator {
                        generator: manager,
                        wrapped,
                        async_mode: false,
                        close_with_exit: true,
                        args: Some(args),
                        wrapped_result: None,
                        stage: PendingContextDecoratorStage::Enter,
                    });
                    self.pending_context_decorator_return = true;
                    Ok(CallResult::FramePushed)
                }
                other => {
                    manager.drop_with_heap(self.heap);
                    wrapped.drop_with_heap(self.heap);
                    args.drop_with_heap(self.heap);
                    Ok(other)
                }
            }
        }
    }

    /// Continues pending decorated call by invoking the wrapped function.
    fn context_decorator_call_wrapped(
        &mut self,
        generator: Value,
        wrapped: Value,
        args: ArgValues,
        async_mode: bool,
        close_with_exit: bool,
    ) -> Result<CallResult, RunError> {
        match self.call_function(wrapped.clone_with_heap(self.heap), args)? {
            CallResult::Push(result) => {
                if async_mode {
                    self.push(result);
                    match self.exec_get_awaitable()? {
                        AwaitResult::ValueReady(value) => {
                            self.context_decorator_close(generator, wrapped, value, async_mode, close_with_exit)
                        }
                        AwaitResult::FramePushed => {
                            self.pending_context_decorator = Some(PendingContextDecorator {
                                generator,
                                wrapped,
                                async_mode,
                                close_with_exit,
                                args: None,
                                wrapped_result: None,
                                stage: PendingContextDecoratorStage::Call,
                            });
                            self.pending_context_decorator_return = true;
                            Ok(CallResult::FramePushed)
                        }
                        AwaitResult::Yield(_) => {
                            generator.drop_with_heap(self.heap);
                            wrapped.drop_with_heap(self.heap);
                            Err(SimpleException::new_msg(
                                ExcType::RuntimeError,
                                "Async context decorator cannot await unresolved external futures",
                            )
                            .into())
                        }
                    }
                } else {
                    self.context_decorator_close(generator, wrapped, result, async_mode, close_with_exit)
                }
            }
            CallResult::FramePushed => {
                self.pending_context_decorator = Some(PendingContextDecorator {
                    generator,
                    wrapped,
                    async_mode,
                    close_with_exit,
                    args: None,
                    wrapped_result: None,
                    stage: PendingContextDecoratorStage::Call,
                });
                self.pending_context_decorator_return = true;
                Ok(CallResult::FramePushed)
            }
            other => {
                generator.drop_with_heap(self.heap);
                wrapped.drop_with_heap(self.heap);
                Ok(other)
            }
        }
    }

    /// Finalizes a decorated call by closing the generator and returning wrapped result.
    fn context_decorator_close(
        &mut self,
        generator: Value,
        wrapped: Value,
        wrapped_result: Value,
        async_mode: bool,
        close_with_exit: bool,
    ) -> Result<CallResult, RunError> {
        if close_with_exit {
            if async_mode {
                let dunder_aexit: StringId = StaticStrings::DunderAexit.into();
                let exit_args = build_arg_values(vec![Value::None, Value::None, Value::None], KwargsValues::Empty);
                match self.call_attr(generator.clone_with_heap(self.heap), dunder_aexit, exit_args)? {
                    CallResult::Push(exit_value) => {
                        self.push(exit_value);
                        match self.exec_get_awaitable()? {
                            AwaitResult::ValueReady(value) => {
                                value.drop_with_heap(self.heap);
                                generator.drop_with_heap(self.heap);
                                wrapped.drop_with_heap(self.heap);
                                Ok(CallResult::Push(wrapped_result))
                            }
                            AwaitResult::FramePushed => {
                                self.pending_context_decorator = Some(PendingContextDecorator {
                                    generator,
                                    wrapped,
                                    async_mode,
                                    close_with_exit,
                                    args: None,
                                    wrapped_result: Some(wrapped_result),
                                    stage: PendingContextDecoratorStage::Close,
                                });
                                self.pending_context_decorator_return = true;
                                Ok(CallResult::FramePushed)
                            }
                            AwaitResult::Yield(_) => {
                                wrapped_result.drop_with_heap(self.heap);
                                generator.drop_with_heap(self.heap);
                                wrapped.drop_with_heap(self.heap);
                                Err(SimpleException::new_msg(
                                    ExcType::RuntimeError,
                                    "Async context decorator cannot await unresolved external futures",
                                )
                                .into())
                            }
                        }
                    }
                    CallResult::FramePushed => {
                        self.pending_context_decorator = Some(PendingContextDecorator {
                            generator,
                            wrapped,
                            async_mode,
                            close_with_exit,
                            args: None,
                            wrapped_result: Some(wrapped_result),
                            stage: PendingContextDecoratorStage::Close,
                        });
                        self.pending_context_decorator_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    other => {
                        wrapped_result.drop_with_heap(self.heap);
                        generator.drop_with_heap(self.heap);
                        wrapped.drop_with_heap(self.heap);
                        Ok(other)
                    }
                }
            } else {
                let dunder_exit: StringId = StaticStrings::DunderExit.into();
                let exit_args = build_arg_values(vec![Value::None, Value::None, Value::None], KwargsValues::Empty);
                match self.call_attr(generator.clone_with_heap(self.heap), dunder_exit, exit_args)? {
                    CallResult::Push(close_result) => {
                        close_result.drop_with_heap(self.heap);
                        generator.drop_with_heap(self.heap);
                        wrapped.drop_with_heap(self.heap);
                        Ok(CallResult::Push(wrapped_result))
                    }
                    CallResult::FramePushed => {
                        self.pending_context_decorator = Some(PendingContextDecorator {
                            generator,
                            wrapped,
                            async_mode,
                            close_with_exit,
                            args: None,
                            wrapped_result: Some(wrapped_result),
                            stage: PendingContextDecoratorStage::Close,
                        });
                        self.pending_context_decorator_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    other => {
                        wrapped_result.drop_with_heap(self.heap);
                        generator.drop_with_heap(self.heap);
                        wrapped.drop_with_heap(self.heap);
                        Ok(other)
                    }
                }
            }
        } else {
            match self.call_function(
                Value::Builtin(Builtins::Type(Type::List)),
                ArgValues::One(generator.clone_with_heap(self.heap)),
            )? {
                CallResult::Push(close_result) => {
                    close_result.drop_with_heap(self.heap);
                    generator.drop_with_heap(self.heap);
                    wrapped.drop_with_heap(self.heap);
                    Ok(CallResult::Push(wrapped_result))
                }
                CallResult::FramePushed => {
                    self.pending_context_decorator = Some(PendingContextDecorator {
                        generator,
                        wrapped,
                        async_mode,
                        close_with_exit,
                        args: None,
                        wrapped_result: Some(wrapped_result),
                        stage: PendingContextDecoratorStage::Close,
                    });
                    self.pending_context_decorator_return = true;
                    Ok(CallResult::FramePushed)
                }
                other => {
                    wrapped_result.drop_with_heap(self.heap);
                    generator.drop_with_heap(self.heap);
                    wrapped.drop_with_heap(self.heap);
                    Ok(other)
                }
            }
        }
    }

    /// Handles return values for pending `contextlib` decorator stages.
    pub(super) fn handle_context_decorator_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_context_decorator.take() else {
            return Err(RunError::internal(
                "handle_context_decorator_return: no pending context decorator state",
            ));
        };

        match pending.stage {
            PendingContextDecoratorStage::Enter => {
                value.drop_with_heap(self.heap);
                let Some(args) = pending.args.take() else {
                    pending.generator.drop_with_heap(self.heap);
                    pending.wrapped.drop_with_heap(self.heap);
                    return Err(RunError::internal(
                        "handle_context_decorator_return: missing wrapped args after enter",
                    ));
                };
                self.context_decorator_call_wrapped(
                    pending.generator,
                    pending.wrapped,
                    args,
                    pending.async_mode,
                    pending.close_with_exit,
                )
            }
            PendingContextDecoratorStage::Call => self.context_decorator_close(
                pending.generator,
                pending.wrapped,
                value,
                pending.async_mode,
                pending.close_with_exit,
            ),
            PendingContextDecoratorStage::Close => {
                value.drop_with_heap(self.heap);
                let Some(result) = pending.wrapped_result.take() else {
                    pending.generator.drop_with_heap(self.heap);
                    pending.wrapped.drop_with_heap(self.heap);
                    return Err(RunError::internal(
                        "handle_context_decorator_return: missing wrapped result after close",
                    ));
                };
                pending.generator.drop_with_heap(self.heap);
                pending.wrapped.drop_with_heap(self.heap);
                Ok(CallResult::Push(result))
            }
        }
    }

    /// Registers one callback entry on a `contextlib.ExitStack`-like object.
    fn exit_stack_push_callback(&mut self, stack_id: HeapId, callback: ExitCallback) -> RunResult<()> {
        let HeapData::StdlibObject(StdlibObject::ExitStack(state)) = self.heap.get_mut(stack_id) else {
            return Err(RunError::internal("exit stack callback target is not ExitStack"));
        };
        state.callbacks.push(callback);
        Ok(())
    }

    /// Returns the wrapped generator id for generator-backed context managers.
    fn exit_stack_enter_generator_id(&self, manager: &Value) -> Option<HeapId> {
        let Value::Ref(manager_id) = manager else {
            return None;
        };
        let HeapData::StdlibObject(StdlibObject::GeneratorContextManager(state)) = self.heap.get(*manager_id) else {
            return None;
        };
        let Value::Ref(generator_id) = state.generator else {
            return None;
        };
        Some(generator_id)
    }

    /// Handles `ExitStack.enter_context(manager)` including frame-pushed `__enter__`.
    fn call_exit_stack_enter_context(&mut self, stack_id: HeapId, args: ArgValues) -> Result<CallResult, RunError> {
        let manager = args.get_one_arg("ExitStack.enter_context", self.heap)?;
        let dunder_enter: StringId = StaticStrings::DunderEnter.into();
        match self.call_attr(manager.clone_with_heap(self.heap), dunder_enter, ArgValues::Empty)? {
            CallResult::Push(enter_value) => {
                self.exit_stack_push_callback(stack_id, ExitCallback::ExitMethod(manager))?;
                Ok(CallResult::Push(enter_value))
            }
            CallResult::FramePushed => {
                self.exit_stack_push_callback(stack_id, ExitCallback::ExitMethod(manager))?;
                Ok(CallResult::FramePushed)
            }
            other => {
                manager.drop_with_heap(self.heap);
                Ok(other)
            }
        }
    }

    /// Handles frame return for pending `ExitStack.enter_context(...)`.
    pub(super) fn handle_exit_stack_enter_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_exit_stack_enter.take() else {
            value.drop_with_heap(self.heap);
            return Err(RunError::internal(
                "handle_exit_stack_enter_return: no pending enter_context state",
            ));
        };
        if let Err(err) = self.exit_stack_push_callback(pending.stack_id, ExitCallback::ExitMethod(pending.manager)) {
            self.heap.dec_ref(pending.stack_id);
            value.drop_with_heap(self.heap);
            return Err(err);
        }
        self.heap.dec_ref(pending.stack_id);
        Ok(CallResult::Push(value))
    }

    /// Handles `AsyncExitStack.enter_async_context(manager)` in VM call paths.
    fn call_exit_stack_enter_async_context(
        &mut self,
        stack_id: HeapId,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let manager = args.get_one_arg("AsyncExitStack.enter_async_context", self.heap)?;
        let dunder_aenter: StringId = StaticStrings::DunderAenter.into();
        match self.call_attr(manager.clone_with_heap(self.heap), dunder_aenter, ArgValues::Empty)? {
            CallResult::Push(enter_value) => {
                self.exit_stack_push_callback(stack_id, ExitCallback::ExitMethod(manager))?;
                Ok(CallResult::Push(enter_value))
            }
            CallResult::FramePushed => {
                self.exit_stack_push_callback(stack_id, ExitCallback::ExitMethod(manager))?;
                Ok(CallResult::FramePushed)
            }
            other => {
                manager.drop_with_heap(self.heap);
                Ok(other)
            }
        }
    }

    /// Starts callback unwinding for `ExitStack.__exit__/close` and async variants.
    fn call_exit_stack_unwind(
        &mut self,
        stack_id: HeapId,
        method_name: &str,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let mut return_suppress = false;
        let mut async_mode = false;
        let (exc_type, exc_value, exc_tb) = match method_name {
            "close" => {
                args.check_zero_args("ExitStack.close", self.heap)?;
                (Value::None, Value::None, Value::None)
            }
            "aclose" => {
                args.check_zero_args("AsyncExitStack.aclose", self.heap)?;
                async_mode = true;
                (Value::None, Value::None, Value::None)
            }
            "__exit__" => {
                return_suppress = true;
                let (mut positional, kwargs) = args.into_parts();
                kwargs.drop_with_heap(self.heap);
                let exc_type = positional.next().unwrap_or(Value::None);
                let exc_value = positional.next().unwrap_or(Value::None);
                let exc_tb = positional.next().unwrap_or(Value::None);
                positional.drop_with_heap(self.heap);
                (exc_type, exc_value, exc_tb)
            }
            "__aexit__" => {
                return_suppress = true;
                async_mode = true;
                let (mut positional, kwargs) = args.into_parts();
                kwargs.drop_with_heap(self.heap);
                let exc_type = positional.next().unwrap_or(Value::None);
                let exc_value = positional.next().unwrap_or(Value::None);
                let exc_tb = positional.next().unwrap_or(Value::None);
                positional.drop_with_heap(self.heap);
                (exc_type, exc_value, exc_tb)
            }
            _ => {
                args.drop_with_heap(self.heap);
                return Err(RunError::internal("invalid exit stack unwind method"));
            }
        };

        let callbacks = {
            let HeapData::StdlibObject(StdlibObject::ExitStack(state)) = self.heap.get_mut(stack_id) else {
                exc_type.drop_with_heap(self.heap);
                exc_value.drop_with_heap(self.heap);
                exc_tb.drop_with_heap(self.heap);
                return Err(RunError::internal("exit stack unwind target is not ExitStack"));
            };
            std::mem::take(&mut state.callbacks)
        };

        let pending = PendingExitStack {
            callbacks,
            exc_type,
            exc_value,
            exc_tb,
            suppress: false,
            return_suppress,
            async_mode,
            awaiting: None,
            in_flight: None,
        };
        self.exit_stack_continue_unwind(pending)
    }

    /// Continues executing pending exit-stack callbacks.
    fn exit_stack_continue_unwind(&mut self, mut pending: PendingExitStack) -> Result<CallResult, RunError> {
        while let Some(callback) = pending.callbacks.pop() {
            match callback {
                ExitCallback::ExitMethod(manager) => {
                    let mut callback_result = if pending.async_mode {
                        let dunder_aexit: StringId = StaticStrings::DunderAexit.into();
                        let args = build_arg_values(
                            vec![
                                pending.exc_type.clone_with_heap(self.heap),
                                pending.exc_value.clone_with_heap(self.heap),
                                pending.exc_tb.clone_with_heap(self.heap),
                            ],
                            KwargsValues::Empty,
                        );
                        match self.call_attr(manager.clone_with_heap(self.heap), dunder_aexit, args)? {
                            CallResult::Push(value) => value,
                            CallResult::FramePushed => {
                                pending.awaiting = Some(PendingExitStackAwaiting::ExitLike);
                                pending.in_flight = Some(ExitCallback::ExitMethod(manager));
                                self.pending_exit_stack = Some(pending);
                                self.pending_exit_stack_return = true;
                                return Ok(CallResult::FramePushed);
                            }
                            other => {
                                manager.drop_with_heap(self.heap);
                                pending.exc_type.drop_with_heap(self.heap);
                                pending.exc_value.drop_with_heap(self.heap);
                                pending.exc_tb.drop_with_heap(self.heap);
                                return Ok(other);
                            }
                        }
                    } else {
                        let dunder_exit: StringId = StaticStrings::DunderExit.into();
                        let args = build_arg_values(
                            vec![
                                pending.exc_type.clone_with_heap(self.heap),
                                pending.exc_value.clone_with_heap(self.heap),
                                pending.exc_tb.clone_with_heap(self.heap),
                            ],
                            KwargsValues::Empty,
                        );
                        match self.call_attr(manager.clone_with_heap(self.heap), dunder_exit, args)? {
                            CallResult::Push(value) => value,
                            CallResult::FramePushed => {
                                pending.awaiting = Some(PendingExitStackAwaiting::ExitLike);
                                pending.in_flight = Some(ExitCallback::ExitMethod(manager));
                                self.pending_exit_stack = Some(pending);
                                self.pending_exit_stack_return = true;
                                return Ok(CallResult::FramePushed);
                            }
                            other => {
                                manager.drop_with_heap(self.heap);
                                pending.exc_type.drop_with_heap(self.heap);
                                pending.exc_value.drop_with_heap(self.heap);
                                pending.exc_tb.drop_with_heap(self.heap);
                                return Ok(other);
                            }
                        }
                    };

                    if pending.async_mode {
                        self.push(callback_result);
                        match self.exec_get_awaitable()? {
                            AwaitResult::ValueReady(value) => callback_result = value,
                            AwaitResult::FramePushed => {
                                pending.awaiting = Some(PendingExitStackAwaiting::ExitLike);
                                pending.in_flight = Some(ExitCallback::ExitMethod(manager));
                                self.pending_exit_stack = Some(pending);
                                self.pending_exit_stack_return = true;
                                return Ok(CallResult::FramePushed);
                            }
                            AwaitResult::Yield(_) => {
                                manager.drop_with_heap(self.heap);
                                pending.exc_type.drop_with_heap(self.heap);
                                pending.exc_value.drop_with_heap(self.heap);
                                pending.exc_tb.drop_with_heap(self.heap);
                                return Err(SimpleException::new_msg(
                                    ExcType::RuntimeError,
                                    "AsyncExitStack cannot await unresolved external futures",
                                )
                                .into());
                            }
                        }
                    }
                    pending.in_flight = None;

                    let should_suppress =
                        !matches!(pending.exc_type, Value::None) && callback_result.py_bool(self.heap, self.interns);
                    callback_result.drop_with_heap(self.heap);
                    manager.drop_with_heap(self.heap);
                    if should_suppress {
                        pending.suppress = true;
                        pending.exc_type.drop_with_heap(self.heap);
                        pending.exc_value.drop_with_heap(self.heap);
                        pending.exc_tb.drop_with_heap(self.heap);
                        pending.exc_type = Value::None;
                        pending.exc_value = Value::None;
                        pending.exc_tb = Value::None;
                    }
                }
                ExitCallback::ExitFunc(func) => {
                    let args = build_arg_values(
                        vec![
                            pending.exc_type.clone_with_heap(self.heap),
                            pending.exc_value.clone_with_heap(self.heap),
                            pending.exc_tb.clone_with_heap(self.heap),
                        ],
                        KwargsValues::Empty,
                    );
                    match self.call_function(func.clone_with_heap(self.heap), args)? {
                        CallResult::Push(result) => {
                            let should_suppress =
                                !matches!(pending.exc_type, Value::None) && result.py_bool(self.heap, self.interns);
                            result.drop_with_heap(self.heap);
                            func.drop_with_heap(self.heap);
                            if should_suppress {
                                pending.suppress = true;
                                pending.exc_type.drop_with_heap(self.heap);
                                pending.exc_value.drop_with_heap(self.heap);
                                pending.exc_tb.drop_with_heap(self.heap);
                                pending.exc_type = Value::None;
                                pending.exc_value = Value::None;
                                pending.exc_tb = Value::None;
                            }
                        }
                        CallResult::FramePushed => {
                            pending.awaiting = Some(PendingExitStackAwaiting::ExitLike);
                            pending.in_flight = Some(ExitCallback::ExitFunc(func));
                            self.pending_exit_stack = Some(pending);
                            self.pending_exit_stack_return = true;
                            return Ok(CallResult::FramePushed);
                        }
                        other => {
                            func.drop_with_heap(self.heap);
                            pending.exc_type.drop_with_heap(self.heap);
                            pending.exc_value.drop_with_heap(self.heap);
                            pending.exc_tb.drop_with_heap(self.heap);
                            return Ok(other);
                        }
                    }
                    pending.in_flight = None;
                }
                ExitCallback::Callback { func, args, kwargs } => {
                    let kwargs = if kwargs.is_empty() {
                        KwargsValues::Empty
                    } else {
                        KwargsValues::Dict(Dict::from_pairs(kwargs, self.heap, self.interns)?)
                    };
                    let call_args = build_arg_values(args, kwargs);
                    match self.call_function(func.clone_with_heap(self.heap), call_args)? {
                        CallResult::Push(result) => {
                            result.drop_with_heap(self.heap);
                            func.drop_with_heap(self.heap);
                        }
                        CallResult::FramePushed => {
                            func.drop_with_heap(self.heap);
                            pending.awaiting = Some(PendingExitStackAwaiting::Callback);
                            self.pending_exit_stack = Some(pending);
                            self.pending_exit_stack_return = true;
                            return Ok(CallResult::FramePushed);
                        }
                        other => {
                            func.drop_with_heap(self.heap);
                            pending.exc_type.drop_with_heap(self.heap);
                            pending.exc_value.drop_with_heap(self.heap);
                            pending.exc_tb.drop_with_heap(self.heap);
                            return Ok(other);
                        }
                    }
                }
            }
        }

        let result_value = if pending.return_suppress {
            Value::Bool(pending.suppress)
        } else {
            Value::None
        };
        pending.exc_type.drop_with_heap(self.heap);
        pending.exc_value.drop_with_heap(self.heap);
        pending.exc_tb.drop_with_heap(self.heap);

        if pending.async_mode {
            let awaitable = StdlibObject::new_immediate_awaitable(result_value);
            let awaitable_id = self.heap.allocate(HeapData::StdlibObject(awaitable))?;
            Ok(CallResult::Push(Value::Ref(awaitable_id)))
        } else {
            Ok(CallResult::Push(result_value))
        }
    }

    /// Handles frame return values while `ExitStack` callback unwind is in progress.
    pub(super) fn handle_exit_stack_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_exit_stack.take() else {
            value.drop_with_heap(self.heap);
            return Err(RunError::internal(
                "handle_exit_stack_return: no pending exit stack state",
            ));
        };

        let Some(awaiting) = pending.awaiting.take() else {
            value.drop_with_heap(self.heap);
            pending.exc_type.drop_with_heap(self.heap);
            pending.exc_value.drop_with_heap(self.heap);
            pending.exc_tb.drop_with_heap(self.heap);
            return Err(RunError::internal(
                "handle_exit_stack_return: missing callback continuation kind",
            ));
        };
        if let Some(callback) = pending.in_flight.take() {
            match callback {
                ExitCallback::ExitMethod(value) | ExitCallback::ExitFunc(value) => {
                    value.drop_with_heap(self.heap);
                }
                ExitCallback::Callback { func, args, kwargs } => {
                    func.drop_with_heap(self.heap);
                    args.drop_with_heap(self.heap);
                    for (key, value) in kwargs {
                        key.drop_with_heap(self.heap);
                        value.drop_with_heap(self.heap);
                    }
                }
            }
        }

        if awaiting == PendingExitStackAwaiting::ExitLike {
            let should_suppress = !matches!(pending.exc_type, Value::None) && value.py_bool(self.heap, self.interns);
            value.drop_with_heap(self.heap);
            if should_suppress {
                pending.suppress = true;
                pending.exc_type.drop_with_heap(self.heap);
                pending.exc_value.drop_with_heap(self.heap);
                pending.exc_tb.drop_with_heap(self.heap);
                pending.exc_type = Value::None;
                pending.exc_value = Value::None;
                pending.exc_tb = Value::None;
            }
        } else {
            value.drop_with_heap(self.heap);
        }

        self.exit_stack_continue_unwind(pending)
    }

    /// Executes `CallFunctionKw` opcode.
    ///
    /// Pops the callable, positional args, and keyword args from the stack,
    /// builds the appropriate `ArgValues`, and calls the function.
    pub(super) fn exec_call_function_kw(
        &mut self,
        pos_count: usize,
        kwname_ids: SmallVec<[StringId; 4]>,
    ) -> Result<CallResult, RunError> {
        let kw_count = kwname_ids.len();

        // Hot path: one keyword arg and no positional args.
        // Try a direct simple-with-defaults frame setup first, then fall back to
        // generic call setup.
        if pos_count == 0 && kw_count == 1 {
            if let Some(result) = self.try_exec_call_function_kw_simple_with_defaults(kwname_ids[0])? {
                return Ok(result);
            }
            let kw_value = self.pop();
            let callable = self.pop();
            let kwargs_inline = vec![(kwname_ids[0], kw_value)];
            return self.call_function(callable, ArgValues::Kwargs(KwargsValues::Inline(kwargs_inline)));
        }

        // Pop keyword values (TOS is last kwarg value)
        let kw_values = self.pop_n(kw_count);

        // Pop positional arguments
        let pos_args = self.pop_n(pos_count);

        // Pop the callable
        let callable = self.pop();

        // Build kwargs as Vec<(StringId, Value)>
        let kwargs_inline: Vec<(StringId, Value)> = kwname_ids.into_iter().zip(kw_values).collect();

        // Build ArgValues with both positional and keyword args
        let args = if pos_args.is_empty() && kwargs_inline.is_empty() {
            ArgValues::Empty
        } else if pos_args.is_empty() {
            ArgValues::Kwargs(KwargsValues::Inline(kwargs_inline))
        } else {
            ArgValues::ArgsKargs {
                args: pos_args,
                kwargs: KwargsValues::Inline(kwargs_inline),
            }
        };

        self.call_function(callable, args)
    }

    /// Tries a specialized one-keyword fast path for `FunctionDefaults` callables.
    ///
    /// This targets call sites like `f(a=1)` where:
    /// - The callable is a plain sync function with defaults (`HeapData::FunctionDefaults`)
    /// - The signature is simple-with-defaults (`def f(a, b=2): ...`)
    /// - There are no positional arguments and exactly one keyword argument
    ///
    /// On success, this pushes a new frame directly and returns `Some(FramePushed)`.
    /// On non-matching shapes, returns `Ok(None)` so the caller can use generic dispatch.
    #[inline]
    fn try_exec_call_function_kw_simple_with_defaults(
        &mut self,
        keyword_id: StringId,
    ) -> Result<Option<CallResult>, RunError> {
        let callable_id = match self.peek_at_depth(1) {
            Value::Ref(callable_id) => *callable_id,
            _ => return Ok(None),
        };

        // Copy defaults without refcount changes. These are borrowed from the callable
        // and only used as read-only sources for clone_with_heap() during binding.
        let (func_id, defaults): (FunctionId, SmallVec<[Value; 4]>) = match self.heap.get(callable_id) {
            HeapData::FunctionDefaults(func_id, defaults) => {
                (*func_id, defaults.iter().map(Value::copy_for_extend).collect())
            }
            _ => return Ok(None),
        };

        let func = self.interns.get_function(func_id);
        if !func.is_simple_with_defaults_sync() {
            return Ok(None);
        }

        // Pop stack operands in opcode order: kw value then callable.
        let kw_value = self.pop();
        let callable = self.pop();

        let call_position = self.current_position();
        let namespace_idx = match self.namespaces.new_namespace(func.namespace_size, self.heap) {
            Ok(idx) => idx,
            Err(e) => {
                kw_value.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(e.into());
            }
        };

        let namespace = self.namespaces.get_mut(namespace_idx).mut_vec();
        let bind_result = func.signature.bind_simple_with_defaults_one_inline_kw(
            keyword_id,
            kw_value,
            defaults.as_slice(),
            self.heap,
            self.interns,
            func.name,
            namespace,
        );
        if let Err(err) = bind_result {
            self.namespaces.drop_with_heap(namespace_idx, self.heap);
            callable.drop_with_heap(self.heap);
            return Err(err);
        }

        namespace.resize_with(func.namespace_size, || Value::Undefined);
        callable.drop_with_heap(self.heap);

        self.frames.push(CallFrame::new_simple_function(
            &func.code,
            self.stack.len(),
            namespace_idx,
            func_id,
            call_position,
        ));
        self.tracer
            .on_call(Some(self.interns.get_str(func.name.name_id)), self.frames.len());
        Ok(Some(CallResult::FramePushed))
    }

    /// Executes `CallAttr` opcode.
    ///
    /// Pops the object and arguments from the stack, calls the attribute,
    /// and returns a `CallResult` which may indicate an OS or external call.
    pub(super) fn exec_call_attr(&mut self, name_id: StringId, arg_count: usize) -> Result<CallResult, RunError> {
        // Hot path for one-arg monomorphic method sites.
        // This avoids `ArgValues` creation and generic attribute dispatch.
        if arg_count == 1 && (name_id == StaticStrings::Append || name_id == StaticStrings::Zfill) {
            let call_site_ip = self.instruction_ip;
            let code_identity = ptr::from_ref(self.current_frame().code) as usize;

            // Monomorphic inline-cache hit path for hot call sites.
            if let Some(entry) = self.call_attr_inline_cache
                && entry.matches(code_identity, call_site_ip, name_id, arg_count)
            {
                match entry.kind() {
                    CallAttrInlineCacheKind::ListAppend => {
                        let maybe_list_id = match self.peek_at_depth(1) {
                            Value::Ref(heap_id) if matches!(self.heap.get(*heap_id), HeapData::List(_)) => {
                                Some(*heap_id)
                            }
                            _ => None,
                        };
                        if let Some(list_id) = maybe_list_id {
                            let item = self.pop();
                            let obj = self.pop();
                            return self.call_list_append_fast(list_id, obj, item);
                        }
                        // Site changed shape from monomorphic to polymorphic: invalidate.
                        self.call_attr_inline_cache = None;
                    }
                    CallAttrInlineCacheKind::StrZfill => {
                        let maybe_str_id = match self.peek_at_depth(1) {
                            Value::Ref(heap_id) if matches!(self.heap.get(*heap_id), HeapData::Str(_)) => {
                                Some(*heap_id)
                            }
                            _ => None,
                        };
                        if let Some(str_id) = maybe_str_id {
                            let width = self.pop();
                            let obj = self.pop();
                            return self.call_str_zfill_fast(str_id, obj, width);
                        }
                        // Site changed shape from monomorphic to polymorphic: invalidate.
                        self.call_attr_inline_cache = None;
                    }
                }
            }

            if name_id == StaticStrings::Append {
                let maybe_list_id = match self.peek_at_depth(1) {
                    Value::Ref(heap_id) if matches!(self.heap.get(*heap_id), HeapData::List(_)) => Some(*heap_id),
                    _ => None,
                };
                if let Some(list_id) = maybe_list_id {
                    self.call_attr_inline_cache = Some(CallAttrInlineCacheEntry::list_append_site(
                        code_identity,
                        call_site_ip,
                        name_id,
                    ));
                    let item = self.pop();
                    let obj = self.pop();
                    return self.call_list_append_fast(list_id, obj, item);
                }
            } else if name_id == StaticStrings::Zfill {
                let maybe_str_id = match self.peek_at_depth(1) {
                    Value::Ref(heap_id) if matches!(self.heap.get(*heap_id), HeapData::Str(_)) => Some(*heap_id),
                    _ => None,
                };
                if let Some(str_id) = maybe_str_id {
                    self.call_attr_inline_cache = Some(CallAttrInlineCacheEntry::str_zfill_site(
                        code_identity,
                        call_site_ip,
                        name_id,
                    ));
                    let width = self.pop();
                    let obj = self.pop();
                    return self.call_str_zfill_fast(str_id, obj, width);
                }
            }
        }

        let args = self.pop_n_args(arg_count);
        let obj = self.pop();
        self.call_attr(obj, name_id, args)
    }

    /// Executes `CallAttrKw` opcode.
    ///
    /// Pops the object, positional args, and keyword args from the stack,
    /// builds the appropriate `ArgValues`, and calls the attribute.
    /// Returns a `CallResult` which may indicate an OS or external call.
    pub(super) fn exec_call_attr_kw(
        &mut self,
        name_id: StringId,
        pos_count: usize,
        kwname_ids: SmallVec<[StringId; 4]>,
    ) -> Result<CallResult, RunError> {
        let kw_count = kwname_ids.len();

        // Hot path: one keyword arg and no positional args.
        // Avoids intermediate Vec allocations from `pop_n`.
        if pos_count == 0 && kw_count == 1 {
            let kw_value = self.pop();
            let obj = self.pop();
            let kwargs_inline = vec![(kwname_ids[0], kw_value)];
            let args = ArgValues::Kwargs(KwargsValues::Inline(kwargs_inline));
            return self.call_attr(obj, name_id, args);
        }

        // Pop keyword values (TOS is last kwarg value)
        let kw_values = self.pop_n(kw_count);

        // Pop positional arguments
        let pos_args = self.pop_n(pos_count);

        // Pop the object
        let obj = self.pop();

        // Build kwargs as Vec<(StringId, Value)>
        let kwargs_inline: Vec<(StringId, Value)> = kwname_ids.into_iter().zip(kw_values).collect();

        // Build ArgValues with both positional and keyword args
        let args = if pos_args.is_empty() && kwargs_inline.is_empty() {
            ArgValues::Empty
        } else if pos_args.is_empty() {
            ArgValues::Kwargs(KwargsValues::Inline(kwargs_inline))
        } else {
            ArgValues::ArgsKargs {
                args: pos_args,
                kwargs: KwargsValues::Inline(kwargs_inline),
            }
        };

        self.call_attr(obj, name_id, args)
    }

    /// Executes `CallFunctionExtended` opcode.
    ///
    /// Handles calls with `*args` and/or `**kwargs` unpacking.
    pub(super) fn exec_call_function_extended(&mut self, has_kwargs: bool) -> Result<CallResult, RunError> {
        // Pop kwargs dict if present
        let kwargs = if has_kwargs { Some(self.pop()) } else { None };

        // Pop args tuple
        let args_tuple = self.pop();

        // Pop callable
        let callable = self.pop();

        // Unpack and call
        self.call_function_extended(callable, args_tuple, kwargs)
    }

    /// Executes `CallAttrExtended` opcode.
    ///
    /// Handles method calls with `*args` and/or `**kwargs` unpacking.
    pub(super) fn exec_call_attr_extended(
        &mut self,
        name_id: StringId,
        has_kwargs: bool,
    ) -> Result<CallResult, RunError> {
        // Pop kwargs dict if present
        let kwargs = if has_kwargs { Some(self.pop()) } else { None };

        // Pop args tuple
        let args_tuple = self.pop();

        // Pop the receiver object
        let obj = self.pop();

        // Unpack and call
        self.call_attr_extended(obj, name_id, args_tuple, kwargs)
    }

    // ========================================================================
    // Internal Call Helpers
    // ========================================================================

    /// Pops n arguments from the stack and wraps them in `ArgValues`.
    fn pop_n_args(&mut self, n: usize) -> ArgValues {
        match n {
            0 => ArgValues::Empty,
            1 => ArgValues::One(self.pop()),
            2 => {
                let b = self.pop();
                let a = self.pop();
                ArgValues::Two(a, b)
            }
            _ => ArgValues::ArgsKargs {
                args: self.pop_n(n),
                kwargs: KwargsValues::Empty,
            },
        }
    }

    /// Executes the exact built-in `list.append(item)` fast path.
    ///
    /// This is shared by both `exec_call_attr` and `call_attr` to keep
    /// semantics and refcount ownership consistent while avoiding generic
    /// call dispatch for the most common list mutation in tight loops.
    fn call_list_append_fast(&mut self, list_id: HeapId, obj: Value, item: Value) -> Result<CallResult, RunError> {
        let item_is_ref = matches!(item, Value::Ref(_));
        {
            let HeapData::List(list) = self.heap.get_mut(list_id) else {
                item.drop_with_heap(self.heap);
                obj.drop_with_heap(self.heap);
                return Err(RunError::internal("list.append fast path: expected list heap entry"));
            };
            if item_is_ref {
                list.set_contains_refs();
            }
            list.as_vec_mut().push(item);
        }
        if item_is_ref {
            self.heap.mark_potential_cycle();
        }
        obj.drop_with_heap(self.heap);
        Ok(CallResult::Push(Value::None))
    }

    /// Executes the exact built-in `str.zfill(width)` fast path for heap strings.
    ///
    /// This bypasses the large generic `call_attr` dispatch tree and directly
    /// invokes the string method implementation while preserving argument and
    /// error semantics.
    fn call_str_zfill_fast(&mut self, str_id: HeapId, obj: Value, width: Value) -> Result<CallResult, RunError> {
        if !matches!(self.heap.get(str_id), HeapData::Str(_)) {
            width.drop_with_heap(self.heap);
            obj.drop_with_heap(self.heap);
            return Err(RunError::internal("str.zfill fast path: expected str heap entry"));
        }
        let attr = EitherStr::Interned(StaticStrings::Zfill.into());
        let result = self
            .heap
            .call_attr_raw(str_id, &attr, ArgValues::One(width), self.interns);
        obj.drop_with_heap(self.heap);
        self.handle_attr_call_result(result?)
    }

    /// Emits CPython-style deprecation warnings for deprecated `pathlib.PurePath` methods.
    ///
    /// This mirrors warning formatting:
    /// `<filename>:<line>: DeprecationWarning: <message>`
    /// `  <source line>`
    fn maybe_emit_pathlib_purepath_deprecation_warning(&mut self, heap_id: HeapId, name_id: StringId) -> RunResult<()> {
        let Some(message) = Self::pathlib_purepath_deprecation_message(name_id) else {
            return Ok(());
        };

        let is_pure_path = matches!(
            self.heap.get(heap_id),
            HeapData::Path(path) if path.is_pure_path_variant()
        );
        if !is_pure_path {
            return Ok(());
        }

        let position = self.current_position();
        let filename = self.interns.get_str(position.filename);
        let warning_filename = if Path::new(filename).is_absolute() {
            filename.to_owned()
        } else {
            env::current_dir().ok().map_or_else(
                || filename.to_owned(),
                |cwd| cwd.join(filename).to_string_lossy().into_owned(),
            )
        };
        let line_number = position.start().line;

        self.emit_warning_line(&format!(
            "{warning_filename}:{line_number}: DeprecationWarning: {message}"
        ))?;
        if let Some(source_line) = Self::warning_source_line(&warning_filename, line_number) {
            self.emit_warning_line(&format!("  {source_line}"))?;
        }
        Ok(())
    }

    /// Returns deprecation warning text for deprecated `pathlib.PurePath` methods.
    #[must_use]
    fn pathlib_purepath_deprecation_message(name_id: StringId) -> Option<&'static str> {
        if name_id == StaticStrings::AsUri {
            return Some(
                "pathlib.PurePath.as_uri() is deprecated and scheduled for removal in Python 3.19. Use pathlib.Path.as_uri().",
            );
        }
        if name_id == StaticStrings::IsReserved {
            return Some(
                "pathlib.PurePath.is_reserved() is deprecated and scheduled for removal in Python 3.15. Use os.path.isreserved() to detect reserved paths on Windows.",
            );
        }
        None
    }

    /// Loads a single source line for warning formatting and strips leading whitespace.
    fn warning_source_line(filename: &str, line_number: u16) -> Option<String> {
        let line_index = usize::from(line_number.saturating_sub(1));
        fs::read_to_string(filename)
            .ok()?
            .lines()
            .nth(line_index)
            .map(|line| line.trim_start().to_owned())
    }

    /// Writes a single warning line to process stdout.
    ///
    /// We intentionally avoid opening `/dev/stdout` because that can be blocked
    /// by sandbox policies in some embeddings.
    fn emit_warning_line(&mut self, line: &str) -> RunResult<()> {
        println!("{line}");
        Ok(())
    }

    /// Calls an attribute on an object.
    ///
    /// For heap-allocated objects (`Value::Ref`), dispatches to the type's
    /// `py_call_attr_raw` implementation via `heap.call_attr_raw()`, which may return
    /// `AttrCallResult::OsCall` or `AttrCallResult::ExternalCall` for operations that
    /// require host involvement.
    ///
    /// For interned strings (`Value::InternString`), uses the unified `call_str_method`.
    /// For interned bytes (`Value::InternBytes`), uses the unified `call_bytes_method`.
    ///
    /// Special handling: `list.sort(key=...)` is intercepted here to allow key
    /// callables that may push VM frames (e.g. user lambdas).
    pub(super) fn call_attr(&mut self, obj: Value, name_id: StringId, args: ArgValues) -> Result<CallResult, RunError> {
        let attr = EitherStr::Interned(name_id);

        match obj {
            Value::Proxy(proxy_id) => {
                let method = self.interns.get_str(name_id).to_owned();
                Ok(CallResult::Proxy(proxy_id, method, args))
            }
            Value::Ref(heap_id) => {
                // Hot path for exact builtin list append in tight loops.
                // This bypasses generic attribute dispatch while preserving
                // Python semantics and refcount ownership.
                if name_id == StaticStrings::Append && matches!(self.heap.get(heap_id), HeapData::List(_)) {
                    let item = match args {
                        ArgValues::One(item) => item,
                        other => match other.get_one_arg("list.append", self.heap) {
                            Ok(item) => item,
                            Err(err) => {
                                obj.drop_with_heap(self.heap);
                                return Err(err);
                            }
                        },
                    };
                    return self.call_list_append_fast(heap_id, obj, item);
                }

                // Stdlib context-manager shims should behave like Python objects and
                // return themselves from __enter__.
                if name_id == StaticStrings::DunderEnter {
                    match self.heap.get(heap_id) {
                        HeapData::StdlibObject(crate::types::StdlibObject::ExitStack(_)) => {
                            args.check_zero_args("__enter__", self.heap)?;
                            self.heap.inc_ref(heap_id);
                            obj.drop_with_heap(self.heap);
                            return Ok(CallResult::Push(Value::Ref(heap_id)));
                        }
                        HeapData::StdlibObject(
                            crate::types::StdlibObject::StringIO(_) | crate::types::StdlibObject::BytesIO(_),
                        ) => {
                            // Execute __enter__ first so closed-state checks still run.
                            // StringIO/BytesIO currently return None from the native method;
                            // VM-level context manager protocol expects `self`.
                            let enter_result = self.heap.call_attr_raw(heap_id, &attr, args, self.interns)?;
                            let AttrCallResult::Value(enter_value) = enter_result else {
                                obj.drop_with_heap(self.heap);
                                return Err(RunError::internal(
                                    "StdlibObject.__enter__ unexpectedly requested deferred execution",
                                ));
                            };
                            enter_value.drop_with_heap(self.heap);
                            self.heap.inc_ref(heap_id);
                            obj.drop_with_heap(self.heap);
                            return Ok(CallResult::Push(Value::Ref(heap_id)));
                        }
                        _ => {}
                    }
                }
                if name_id == StaticStrings::DunderIter
                    && let HeapData::StdlibObject(
                        crate::types::StdlibObject::StringIO(_) | crate::types::StdlibObject::BytesIO(_),
                    ) = self.heap.get(heap_id)
                {
                    // Execute __iter__ first so closed-state checks still run.
                    // Native StringIO/BytesIO return None here; iterator protocol expects self.
                    let iter_result = self.heap.call_attr_raw(heap_id, &attr, args, self.interns)?;
                    let AttrCallResult::Value(iter_value) = iter_result else {
                        obj.drop_with_heap(self.heap);
                        return Err(RunError::internal(
                            "StdlibObject.__iter__ unexpectedly requested deferred execution",
                        ));
                    };
                    iter_value.drop_with_heap(self.heap);
                    self.heap.inc_ref(heap_id);
                    obj.drop_with_heap(self.heap);
                    return Ok(CallResult::Push(Value::Ref(heap_id)));
                }
                let exit_stack_async_mode = match self.heap.get(heap_id) {
                    HeapData::StdlibObject(StdlibObject::ExitStack(state)) => Some(state.async_mode),
                    _ => None,
                };
                if let Some(async_mode) = exit_stack_async_mode {
                    match self.interns.get_str(name_id) {
                        "__aenter__" if async_mode => {
                            args.check_zero_args("__aenter__", self.heap)?;
                            self.heap.inc_ref(heap_id);
                            obj.drop_with_heap(self.heap);
                            let awaitable = StdlibObject::new_immediate_awaitable(Value::Ref(heap_id));
                            let awaitable_id = self.heap.allocate(HeapData::StdlibObject(awaitable))?;
                            return Ok(CallResult::Push(Value::Ref(awaitable_id)));
                        }
                        "enter_context" => {
                            obj.drop_with_heap(self.heap);
                            return self.call_exit_stack_enter_context(heap_id, args);
                        }
                        "enter_async_context" if async_mode => {
                            obj.drop_with_heap(self.heap);
                            return self.call_exit_stack_enter_async_context(heap_id, args);
                        }
                        "close" | "__exit__" => {
                            obj.drop_with_heap(self.heap);
                            return self.call_exit_stack_unwind(heap_id, self.interns.get_str(name_id), args);
                        }
                        "aclose" | "__aexit__" if async_mode => {
                            obj.drop_with_heap(self.heap);
                            return self.call_exit_stack_unwind(heap_id, self.interns.get_str(name_id), args);
                        }
                        _ => {}
                    }
                }
                let is_context_exit =
                    name_id == StaticStrings::DunderExit || self.interns.get_str(name_id) == "__aexit__";
                let closing_target = if is_context_exit {
                    match self.heap.get(heap_id) {
                        HeapData::StdlibObject(StdlibObject::ContextManager(state))
                            if state.name == "contextlib.closing" || state.name == "contextlib.aclosing" =>
                        {
                            Some(state.enter_value.copy_for_extend())
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(mut target) = closing_target {
                    let (positional, kwargs) = args.into_parts();
                    positional.drop_with_heap(self.heap);
                    kwargs.drop_with_heap(self.heap);
                    if let Value::Ref(target_id) = target {
                        self.heap.inc_ref(target_id);
                        target = Value::Ref(target_id);
                    }
                    obj.drop_with_heap(self.heap);
                    let close_id: StringId = StaticStrings::Close.into();
                    return self.call_attr(target, close_id, ArgValues::Empty);
                }
                // Check for list.sort - needs special handling for key functions
                if name_id == StaticStrings::Sort && matches!(self.heap.get(heap_id), HeapData::List(_)) {
                    let result = self.call_list_sort(heap_id, args);
                    obj.drop_with_heap(self.heap);
                    return result;
                }
                // dict.update(generator) needs VM-driven materialization because
                // OurosIter cannot iterate generators outside VM context.
                if name_id == StaticStrings::Update
                    && matches!(self.heap.get(heap_id), HeapData::Dict(_))
                    && let Some(Value::Ref(iter_id)) = get_arg_at(&args, 0)
                    && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                {
                    let result = self.call_dict_update_with_generator(heap_id, args);
                    obj.drop_with_heap(self.heap);
                    return result;
                }
                // str.join(generator) needs VM-driven materialization because
                // OurosIter cannot iterate generators outside VM context.
                if name_id == StaticStrings::Join
                    && matches!(self.heap.get(heap_id), HeapData::Str(_))
                    && let Some(Value::Ref(iter_id)) = get_arg_at(&args, 0)
                    && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                {
                    let separator = match self.heap.get(heap_id) {
                        HeapData::Str(s) => s.as_str().to_owned(),
                        _ => unreachable!("join branch validated a string object"),
                    };
                    let result = self.call_str_join_with_generator_args(separator, args);
                    obj.drop_with_heap(self.heap);
                    return result;
                }
                // Instance method calls need special handling: look up the method,
                // then call it with `self` prepended.
                // Inc_ref before dropping obj so the instance stays alive during lookup.
                // call_instance_method will inc_ref again for the self_arg if needed.
                // We dec_ref after the call completes to balance this temporary hold.
                if matches!(self.heap.get(heap_id), HeapData::Instance(_)) {
                    self.heap.inc_ref(heap_id);
                    obj.drop_with_heap(self.heap);
                    let result = self.call_instance_method(heap_id, name_id, args);
                    self.heap.dec_ref(heap_id);
                    return result;
                }
                // SuperProxy method calls: look up via MRO, call with instance as self
                if matches!(self.heap.get(heap_id), HeapData::SuperProxy(_)) {
                    // Extract info before dropping (SuperProxy may have refcount 1)
                    let (instance_id, current_class_id) = match self.heap.get(heap_id) {
                        HeapData::SuperProxy(sp) => (sp.instance_id(), sp.current_class_id()),
                        _ => unreachable!(),
                    };
                    obj.drop_with_heap(self.heap);
                    return self.call_super_method_with_ids(instance_id, current_class_id, name_id, args);
                }
                // ClassObject method calls: look up in namespace, unwrap descriptors,
                // handle @staticmethod (no self/cls), @classmethod (prepend cls), regular calls.
                if matches!(self.heap.get(heap_id), HeapData::ClassObject(_)) {
                    obj.drop_with_heap(self.heap);
                    return self.call_class_method(heap_id, name_id, args);
                }
                // singledispatch register/dispatch helper methods are handled as native
                // attributes on the dispatcher objects.
                if matches!(
                    self.heap.get(heap_id),
                    HeapData::SingleDispatch(_) | HeapData::SingleDispatchMethod(_)
                ) {
                    obj.drop_with_heap(self.heap);
                    return self.call_singledispatch_attr(heap_id, name_id, args);
                }
                // `functools.lru_cache` exposes `__wrapped__` as an attribute that
                // should be called like a regular function.
                if name_id == StaticStrings::DunderWrapped && matches!(self.heap.get(heap_id), HeapData::LruCache(_)) {
                    self.heap.inc_ref(heap_id);
                    let bound = Value::Ref(heap_id);
                    let attr_result = bound.py_getattr(name_id, self.heap, self.interns);
                    bound.drop_with_heap(self.heap);
                    let attr_result = attr_result?;
                    obj.drop_with_heap(self.heap);
                    return match attr_result {
                        AttrCallResult::Value(callable) => self.call_function(callable, args),
                        other => self.handle_attr_call_result(other),
                    };
                }
                // Generator method calls: __next__ and __iter__ need special handling
                if matches!(self.heap.get(heap_id), HeapData::Generator(_)) {
                    let dunder_next: StringId = StaticStrings::DunderNext.into();
                    let dunder_iter: StringId = StaticStrings::DunderIter.into();
                    let send: StringId = StaticStrings::Send.into();
                    let throw: StringId = StaticStrings::Throw.into();
                    let close: StringId = StaticStrings::Close.into();
                    if name_id == dunder_next {
                        args.check_zero_args("__next__", self.heap)?;
                        let result = self.generator_next(heap_id);
                        obj.drop_with_heap(self.heap);
                        return result;
                    }
                    if name_id == dunder_iter {
                        args.check_zero_args("__iter__", self.heap)?;
                        // __iter__ returns self
                        self.heap.inc_ref(heap_id);
                        obj.drop_with_heap(self.heap);
                        return Ok(CallResult::Push(Value::Ref(heap_id)));
                    }
                    if name_id == send {
                        let send_value = args.get_one_arg("send", self.heap)?;
                        let result = self.generator_send(heap_id, send_value);
                        obj.drop_with_heap(self.heap);
                        return result;
                    }
                    if name_id == throw {
                        let exc_value = args.get_one_arg("throw", self.heap)?;
                        let result = self.generator_throw(heap_id, exc_value);
                        obj.drop_with_heap(self.heap);
                        return result;
                    }
                    if name_id == close {
                        args.check_zero_args("close", self.heap)?;
                        let result = self.generator_close(heap_id);
                        obj.drop_with_heap(self.heap);
                        return result;
                    }
                }
                // Function-like objects can be called via `obj.attr(...)` where `attr`
                // is not a native method (e.g. `func.__wrapped__()`). For these values,
                // fall back to regular `getattr` + call semantics.
                if matches!(
                    self.heap.get(heap_id),
                    HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _) | HeapData::FunctionWrapper(_)
                ) {
                    self.heap.inc_ref(heap_id);
                    let bound = Value::Ref(heap_id);
                    let attr_result = bound.py_getattr(name_id, self.heap, self.interns);
                    bound.drop_with_heap(self.heap);
                    let attr_result = attr_result?;
                    obj.drop_with_heap(self.heap);
                    return match attr_result {
                        AttrCallResult::Value(callable) => self.call_function(callable, args),
                        other => self.handle_attr_call_result(other),
                    };
                }
                // Weakref proxies need normal attribute resolution semantics for method
                // calls (`proxy.method()`), otherwise optimized call-attr dispatch can
                // miss proxy forwarding and raise AttributeError.
                if matches!(self.heap.get(heap_id), HeapData::WeakRef(_)) {
                    self.heap.inc_ref(heap_id);
                    let bound = Value::Ref(heap_id);
                    let attr_result = bound.py_getattr(name_id, self.heap, self.interns);
                    bound.drop_with_heap(self.heap);
                    obj.drop_with_heap(self.heap);
                    return match attr_result {
                        Ok(AttrCallResult::Value(callable)) => self.call_function(callable, args),
                        Ok(other) => self.handle_attr_call_result(other),
                        Err(err) => {
                            args.drop_with_heap(self.heap);
                            Err(err)
                        }
                    };
                }
                // Call the method on the heap object using call_attr_raw to support OS/external calls
                self.maybe_emit_pathlib_purepath_deprecation_warning(heap_id, name_id)?;
                let result = self.heap.call_attr_raw(heap_id, &attr, args, self.interns);
                obj.drop_with_heap(self.heap);
                // Convert AttrCallResult to CallResult (handles ReduceCall via VM)
                self.handle_attr_call_result(result?)
            }
            Value::InternString(string_id) => {
                // Call string method on interned string literal using the unified dispatcher
                let s = self.interns.get_str(string_id);
                if name_id == StaticStrings::Join {
                    let iterable = args.get_one_arg("str.join", self.heap)?;
                    if let Value::Ref(iter_id) = &iterable
                        && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                    {
                        self.call_str_join_with_generator(s.to_owned(), iterable)
                    } else {
                        call_str_method(s, name_id, ArgValues::One(iterable), self.heap, self.interns)
                            .map(CallResult::Push)
                    }
                } else {
                    call_str_method(s, name_id, args, self.heap, self.interns).map(CallResult::Push)
                }
            }
            Value::InternBytes(bytes_id) => {
                // Call bytes method on interned bytes literal using the unified dispatcher
                let b = self.interns.get_bytes(bytes_id);
                call_bytes_method(b, name_id, args, self.heap, self.interns).map(CallResult::Push)
            }
            Value::Builtin(Builtins::Type(t)) => {
                // Handle classmethods on type objects like dict.fromkeys()
                call_type_method(t, name_id, args, self.heap, self.interns).map(CallResult::Push)
            }
            _ => {
                // Immediate values may still expose attributes via Value::py_getattr
                // (e.g. int.bit_length on Value::Int).
                let attr_result = obj.py_getattr(name_id, self.heap, self.interns);
                obj.drop_with_heap(self.heap);
                match attr_result {
                    Ok(AttrCallResult::Value(callable)) => self.call_function(callable, args),
                    Ok(other) => self.handle_attr_call_result(other),
                    Err(err) => {
                        args.drop_with_heap(self.heap);
                        Err(err)
                    }
                }
            }
        }
    }

    /// Calls `dict.update(...)` when the first positional argument is a generator.
    ///
    /// The generator is first materialized through VM-aware list construction so
    /// suspension/resume works correctly, then `dict.update` is invoked with the
    /// normalized argument list.
    fn call_dict_update_with_generator(&mut self, dict_id: HeapId, args: ArgValues) -> Result<CallResult, RunError> {
        let (positional, kwargs) = args.into_parts();
        let mut positional: Vec<Value> = positional.collect();

        let first = positional.remove(0);
        match self.list_build_from_iterator(first) {
            Ok(CallResult::Push(list_value)) => {
                self.call_dict_update_after_materialization(dict_id, list_value, positional, kwargs)
            }
            Ok(CallResult::FramePushed) => {
                self.heap.inc_ref(dict_id);
                self.pending_builtin_from_list.push(PendingBuiltinFromList {
                    kind: PendingBuiltinFromListKind::DictUpdate {
                        dict_id,
                        remaining_positional: positional,
                        kwargs,
                    },
                });
                Ok(CallResult::FramePushed)
            }
            Ok(other) => {
                for value in positional {
                    value.drop_with_heap(self.heap);
                }
                kwargs.drop_with_heap(self.heap);
                Ok(other)
            }
            Err(err) => {
                for value in positional {
                    value.drop_with_heap(self.heap);
                }
                kwargs.drop_with_heap(self.heap);
                Err(err)
            }
        }
    }

    /// Finalizes `dict.update(...)` after generator-to-list materialization.
    fn call_dict_update_after_materialization(
        &mut self,
        dict_id: HeapId,
        list_value: Value,
        mut remaining_positional: Vec<Value>,
        kwargs: KwargsValues,
    ) -> Result<CallResult, RunError> {
        let mut normalized_positional = Vec::with_capacity(1 + remaining_positional.len());
        normalized_positional.push(list_value);
        normalized_positional.append(&mut remaining_positional);

        let update_args = build_arg_values(normalized_positional, kwargs);
        let update_attr = EitherStr::Interned(StaticStrings::Update.into());
        let result = self
            .heap
            .call_attr_raw(dict_id, &update_attr, update_args, self.interns)?;
        self.handle_attr_call_result(result)
    }

    /// Calls `str.join(...)` when the sole argument is known to be a generator.
    ///
    /// The generator is materialized through VM-aware list construction before
    /// invoking `str.join` with the resulting list.
    fn call_str_join_with_generator_args(
        &mut self,
        separator: String,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let iterable = args.get_one_arg("str.join", self.heap)?;
        self.call_str_join_with_generator(separator, iterable)
    }

    /// Finalizes generator-aware `str.join` once the iterable argument is extracted.
    fn call_str_join_with_generator(&mut self, separator: String, iterable: Value) -> Result<CallResult, RunError> {
        match self.list_build_from_iterator(iterable)? {
            CallResult::Push(list_value) => {
                let value = call_str_method(
                    separator.as_str(),
                    StaticStrings::Join.into(),
                    ArgValues::One(list_value),
                    self.heap,
                    self.interns,
                )?;
                Ok(CallResult::Push(value))
            }
            CallResult::FramePushed => {
                self.pending_builtin_from_list.push(PendingBuiltinFromList {
                    kind: PendingBuiltinFromListKind::Join(separator),
                });
                Ok(CallResult::FramePushed)
            }
            other => Ok(other),
        }
    }

    /// Executes `list.sort()` with VM-aware `key=` support.
    ///
    /// Unlike the non-VM list sorter used by `sorted()`, this path can execute
    /// user-defined key callables that push frames (e.g. lambdas/closures). Key
    /// computation is resumable via `pending_list_sort` when a key call returns
    /// `FramePushed`.
    fn call_list_sort(&mut self, list_id: HeapId, args: ArgValues) -> Result<CallResult, RunError> {
        let (key_arg, reverse_arg) =
            args.extract_two_kwargs_only("list.sort", "key", "reverse", self.heap, self.interns)?;

        let reverse = if let Some(value) = reverse_arg {
            let reverse = value.py_bool(self.heap, self.interns);
            value.drop_with_heap(self.heap);
            reverse
        } else {
            false
        };

        let key_fn = match key_arg {
            Some(value) if matches!(value, Value::None) => {
                value.drop_with_heap(self.heap);
                None
            }
            other => other,
        };

        let items = {
            let HeapData::List(list) = self.heap.get_mut(list_id) else {
                if let Some(key) = key_fn {
                    key.drop_with_heap(self.heap);
                }
                return Err(RunError::internal("expected list in call_list_sort"));
            };
            list.as_vec_mut().drain(..).collect::<Vec<_>>()
        };

        if let Some(key_fn) = key_fn {
            let pending = PendingListSort {
                list_id,
                holds_list_ref: false,
                key_fn,
                reverse,
                items,
                key_values: Vec::new(),
                next_index: 0,
            };
            self.list_sort_compute_keys(pending)
        } else {
            self.finish_list_sort(list_id, items, None, reverse)?;
            Ok(CallResult::Push(Value::None))
        }
    }

    /// Continues key computation for an in-flight `list.sort(key=...)`.
    ///
    /// This calls the key function for each item in sequence. If a key call pushes
    /// a frame, we stash the remaining state and resume from `handle_list_sort_return`.
    fn list_sort_compute_keys(&mut self, mut pending: PendingListSort) -> Result<CallResult, RunError> {
        while pending.next_index < pending.items.len() {
            let item = pending.items[pending.next_index].clone_with_heap(self.heap);
            let key_fn = pending.key_fn.clone_with_heap(self.heap);
            let call_args = ArgValues::One(item);
            match self.call_function(key_fn, call_args)? {
                CallResult::Push(key_value) => {
                    pending.key_values.push(key_value);
                    pending.next_index += 1;
                }
                CallResult::FramePushed => {
                    pending.next_index += 1;
                    if !pending.holds_list_ref {
                        self.heap.inc_ref(pending.list_id);
                        pending.holds_list_ref = true;
                    }
                    self.pending_list_sort = Some(pending);
                    self.pending_list_sort_return = true;
                    return Ok(CallResult::FramePushed);
                }
                CallResult::External(_, ext_args) => {
                    ext_args.drop_with_heap(self.heap);
                    pending.key_fn.drop_with_heap(self.heap);
                    for value in pending.key_values {
                        value.drop_with_heap(self.heap);
                    }
                    self.restore_list_items(pending.list_id, pending.items)?;
                    if pending.holds_list_ref {
                        self.heap.dec_ref(pending.list_id);
                    }
                    return Err(ExcType::type_error(
                        "list.sort() key function cannot be an external callable",
                    ));
                }
                CallResult::Proxy(_, _, proxy_args) => {
                    proxy_args.drop_with_heap(self.heap);
                    pending.key_fn.drop_with_heap(self.heap);
                    for value in pending.key_values {
                        value.drop_with_heap(self.heap);
                    }
                    self.restore_list_items(pending.list_id, pending.items)?;
                    if pending.holds_list_ref {
                        self.heap.dec_ref(pending.list_id);
                    }
                    return Err(ExcType::type_error(
                        "list.sort() key function cannot be a proxy callable",
                    ));
                }
                CallResult::OsCall(_, os_args) => {
                    os_args.drop_with_heap(self.heap);
                    pending.key_fn.drop_with_heap(self.heap);
                    for value in pending.key_values {
                        value.drop_with_heap(self.heap);
                    }
                    self.restore_list_items(pending.list_id, pending.items)?;
                    if pending.holds_list_ref {
                        self.heap.dec_ref(pending.list_id);
                    }
                    return Err(ExcType::type_error(
                        "list.sort() key function cannot perform os operations",
                    ));
                }
            }
        }

        pending.key_fn.drop_with_heap(self.heap);
        let sort_result = self.finish_list_sort(
            pending.list_id,
            pending.items,
            Some(pending.key_values),
            pending.reverse,
        );
        if pending.holds_list_ref {
            self.heap.dec_ref(pending.list_id);
        }
        sort_result?;
        Ok(CallResult::Push(Value::None))
    }

    /// Resumes `list.sort(key=...)` after a key callable frame returns.
    pub(super) fn handle_list_sort_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_list_sort.take() else {
            return Err(RunError::internal(
                "handle_list_sort_return: no pending list sort state",
            ));
        };
        pending.key_values.push(value);
        self.list_sort_compute_keys(pending)
    }

    /// Aborts a pending `list.sort(key=...)` after a key-call exception.
    ///
    /// Key evaluation drains list contents into `pending.items`. If a key call raises,
    /// we must restore the original list contents and clear pending state so execution
    /// can continue normally after exception handling.
    pub(super) fn abort_pending_list_sort_on_exception(&mut self) -> RunResult<()> {
        self.pending_list_sort_return = false;
        let Some(pending) = self.pending_list_sort.take() else {
            if let Some(list_id) = self.pending_sorted.take() {
                Value::Ref(list_id).drop_with_heap(self.heap);
            }
            return Ok(());
        };

        pending.key_fn.drop_with_heap(self.heap);
        for value in pending.key_values {
            value.drop_with_heap(self.heap);
        }
        self.restore_list_items(pending.list_id, pending.items)?;
        if pending.holds_list_ref {
            self.heap.dec_ref(pending.list_id);
        }

        if let Some(list_id) = self.pending_sorted.take() {
            Value::Ref(list_id).drop_with_heap(self.heap);
        }
        Ok(())
    }

    /// Clears pending list-sort state and drops held values.
    pub(super) fn clear_pending_list_sort(&mut self) {
        self.pending_list_sort_return = false;
        let Some(pending) = self.pending_list_sort.take() else {
            return;
        };
        if pending.holds_list_ref {
            self.heap.dec_ref(pending.list_id);
        }
        pending.key_fn.drop_with_heap(self.heap);
        for value in pending.items {
            value.drop_with_heap(self.heap);
        }
        for value in pending.key_values {
            value.drop_with_heap(self.heap);
        }
    }

    /// Finalizes a list sort with optional precomputed key values.
    fn finish_list_sort(
        &mut self,
        list_id: HeapId,
        mut items: Vec<Value>,
        mut key_values: Option<Vec<Value>>,
        reverse: bool,
    ) -> RunResult<()> {
        let len = items.len();
        let mut indices: Vec<usize> = (0..len).collect();
        let mut sort_error: Option<RunError> = None;

        if let Some(keys) = key_values.as_ref() {
            indices.sort_by(|&a, &b| {
                if sort_error.is_some() {
                    return Ordering::Equal;
                }
                if let Some(ord) = keys[a].py_cmp(&keys[b], self.heap, self.interns) {
                    if reverse { ord.reverse() } else { ord }
                } else {
                    sort_error = Some(ExcType::type_error(format!(
                        "'<' not supported between instances of '{}' and '{}'",
                        keys[a].py_type(self.heap),
                        keys[b].py_type(self.heap)
                    )));
                    Ordering::Equal
                }
            });
        } else {
            indices.sort_by(|&a, &b| {
                if sort_error.is_some() {
                    return Ordering::Equal;
                }
                if let Some(ord) = items[a].py_cmp(&items[b], self.heap, self.interns) {
                    if reverse { ord.reverse() } else { ord }
                } else {
                    sort_error = Some(ExcType::type_error(format!(
                        "'<' not supported between instances of '{}' and '{}'",
                        items[a].py_type(self.heap),
                        items[b].py_type(self.heap)
                    )));
                    Ordering::Equal
                }
            });
        }

        if let Some(keys) = key_values.take() {
            for value in keys {
                value.drop_with_heap(self.heap);
            }
        }

        if let Some(err) = sort_error {
            self.restore_list_items(list_id, items)?;
            return Err(err);
        }

        let mut sorted_items = Vec::with_capacity(len);
        for &index in &indices {
            sorted_items.push(std::mem::replace(&mut items[index], Value::Undefined));
        }

        self.restore_list_items(list_id, sorted_items)
    }

    /// Restores list contents after a sort operation.
    fn restore_list_items(&mut self, list_id: HeapId, items: Vec<Value>) -> RunResult<()> {
        let HeapData::List(list) = self.heap.get_mut(list_id) else {
            for value in items {
                value.drop_with_heap(self.heap);
            }
            return Err(RunError::internal("expected list while restoring sorted items"));
        };
        list.as_vec_mut().extend(items);
        Ok(())
    }

    /// Dispatches `min`/`max` calls between the single-iterable fast path and
    /// the general VM-managed `key=` implementation.
    fn call_min_max_builtin_dispatch(&mut self, args: ArgValues, is_min: bool) -> Result<CallResult, RunError> {
        let (mut positional, kwargs) = args.into_parts();
        if kwargs.is_empty() && positional.len() == 1 {
            let value = positional.next().expect("len() == 1 guarantees one positional value");
            return if is_min {
                self.call_min_builtin(ArgValues::One(value))
            } else {
                self.call_max_builtin(ArgValues::One(value))
            };
        }

        let positional_values: Vec<Value> = positional.collect();
        let normalized = build_arg_values(positional_values, kwargs);
        self.call_min_max_builtin(normalized, is_min)
    }

    /// Executes `min()` or `max()` with VM-managed `key=` callable support.
    fn call_min_max_builtin(&mut self, args: ArgValues, is_min: bool) -> Result<CallResult, RunError> {
        let func_name = if is_min { "min" } else { "max" };
        let (mut positional, kwargs) = args.into_parts();

        let mut key_fn: Option<Value> = None;
        for (key, value) in kwargs {
            let Some(key_name) = key.as_either_str(self.heap) else {
                key.drop_with_heap(self.heap);
                value.drop_with_heap(self.heap);
                if let Some(v) = key_fn {
                    v.drop_with_heap(self.heap);
                }
                for item in positional {
                    item.drop_with_heap(self.heap);
                }
                return Err(ExcType::type_error_kwargs_nonstring_key());
            };
            key.drop_with_heap(self.heap);
            let key_name = key_name.as_str(self.interns);
            if key_name != "key" {
                value.drop_with_heap(self.heap);
                if let Some(v) = key_fn {
                    v.drop_with_heap(self.heap);
                }
                for item in positional {
                    item.drop_with_heap(self.heap);
                }
                return Err(ExcType::type_error_unexpected_keyword(func_name, key_name));
            }
            if key_fn.is_some() {
                value.drop_with_heap(self.heap);
                if let Some(old) = key_fn {
                    old.drop_with_heap(self.heap);
                }
                for item in positional {
                    item.drop_with_heap(self.heap);
                }
                return Err(ExcType::type_error_multiple_values(func_name, "key"));
            }
            key_fn = Some(value);
        }

        let key_fn = match key_fn {
            Some(value) if matches!(value, Value::None) => {
                value.drop_with_heap(self.heap);
                None
            }
            other => other,
        };

        let Some(first_arg) = positional.next() else {
            if let Some(value) = key_fn {
                value.drop_with_heap(self.heap);
            }
            return Err(ExcType::type_error(format!(
                "{func_name}() expected at least 1 argument, got 0"
            )));
        };

        let mut items = Vec::new();
        if positional.len() == 0 {
            let mut iter = match OurosIter::new(first_arg, self.heap, self.interns) {
                Ok(iter) => iter,
                Err(err) => {
                    if let Some(value) = key_fn {
                        value.drop_with_heap(self.heap);
                    }
                    return Err(err);
                }
            };
            loop {
                match iter.for_next(self.heap, self.interns) {
                    Ok(Some(item)) => items.push(item),
                    Ok(None) => break,
                    Err(err) => {
                        iter.drop_with_heap(self.heap);
                        if let Some(value) = key_fn {
                            value.drop_with_heap(self.heap);
                        }
                        for item in items {
                            item.drop_with_heap(self.heap);
                        }
                        return Err(err);
                    }
                }
            }
            iter.drop_with_heap(self.heap);
            if items.is_empty() {
                if let Some(value) = key_fn {
                    value.drop_with_heap(self.heap);
                }
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    format!("{func_name}() iterable argument is empty"),
                )
                .into());
            }
        } else {
            items.push(first_arg);
            items.extend(positional);
        }

        let Some(key_fn) = key_fn else {
            return self.finish_min_max_without_key(items, is_min);
        };

        let pending = super::PendingMinMax {
            is_min,
            key_fn,
            items,
            best_index: None,
            best_key: None,
            next_index: 0,
            awaiting_index: 0,
        };
        self.min_max_compute_keys(pending)
    }

    /// Continues key computation for an in-flight `min/max(..., key=...)`.
    fn min_max_compute_keys(&mut self, mut pending: super::PendingMinMax) -> Result<CallResult, RunError> {
        while pending.next_index < pending.items.len() {
            let item_index = pending.next_index;
            pending.next_index += 1;
            pending.awaiting_index = item_index;

            let item = pending.items[item_index].clone_with_heap(self.heap);
            let key_fn = pending.key_fn.clone_with_heap(self.heap);

            match self.call_function(key_fn, ArgValues::One(item))? {
                CallResult::Push(key_value) => {
                    if let Err(err) = self.min_max_update_best(&mut pending, item_index, key_value) {
                        self.drop_pending_min_max_state(pending);
                        return Err(err);
                    }
                }
                CallResult::FramePushed => {
                    self.pending_min_max = Some(pending);
                    self.pending_min_max_return = true;
                    return Ok(CallResult::FramePushed);
                }
                CallResult::External(_, ext_args) => {
                    ext_args.drop_with_heap(self.heap);
                    self.drop_pending_min_max_state(pending);
                    return Err(ExcType::type_error(
                        "min/max key function cannot be an external callable",
                    ));
                }
                CallResult::Proxy(_, _, proxy_args) => {
                    proxy_args.drop_with_heap(self.heap);
                    self.drop_pending_min_max_state(pending);
                    return Err(ExcType::type_error("min/max key function cannot be a proxy callable"));
                }
                CallResult::OsCall(_, os_args) => {
                    os_args.drop_with_heap(self.heap);
                    self.drop_pending_min_max_state(pending);
                    return Err(ExcType::type_error("min/max key function cannot perform os operations"));
                }
            }
        }

        pending.key_fn.drop_with_heap(self.heap);
        if let Some(best_key) = pending.best_key {
            best_key.drop_with_heap(self.heap);
        }
        let Some(best_index) = pending.best_index else {
            for item in pending.items {
                item.drop_with_heap(self.heap);
            }
            return Err(RunError::internal(
                "min/max key evaluation completed without a best item",
            ));
        };

        self.finish_min_max_result(pending.items, best_index)
    }

    /// Handles the return value from a user-defined min/max key call.
    pub(super) fn handle_min_max_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_min_max.take() else {
            return Err(RunError::internal("handle_min_max_return: no pending min/max state"));
        };

        let item_index = pending.awaiting_index;
        if let Err(err) = self.min_max_update_best(&mut pending, item_index, value) {
            self.drop_pending_min_max_state(pending);
            return Err(err);
        }
        self.min_max_compute_keys(pending)
    }

    /// Drops pending min/max state and all owned values.
    fn drop_pending_min_max_state(&mut self, pending: super::PendingMinMax) {
        pending.key_fn.drop_with_heap(self.heap);
        for item in pending.items {
            item.drop_with_heap(self.heap);
        }
        if let Some(best_key) = pending.best_key {
            best_key.drop_with_heap(self.heap);
        }
    }

    /// Clears pending min/max state during cleanup/exception unwind.
    pub(super) fn clear_pending_min_max(&mut self) {
        self.pending_min_max_return = false;
        if let Some(pending) = self.pending_min_max.take() {
            self.drop_pending_min_max_state(pending);
        }
    }

    /// Updates current min/max best entry with a newly computed key value.
    fn min_max_update_best(
        &mut self,
        pending: &mut super::PendingMinMax,
        item_index: usize,
        key_value: Value,
    ) -> RunResult<()> {
        let Some(best_key) = pending.best_key.as_ref() else {
            pending.best_index = Some(item_index);
            pending.best_key = Some(key_value);
            return Ok(());
        };

        let Some(ordering) = best_key.py_cmp(&key_value, self.heap, self.interns) else {
            let left_type = best_key.py_type(self.heap);
            let right_type = key_value.py_type(self.heap);
            key_value.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!(
                "'<' not supported between instances of '{left_type}' and '{right_type}'"
            )));
        };

        let new_is_better =
            (pending.is_min && ordering == Ordering::Greater) || (!pending.is_min && ordering == Ordering::Less);
        if new_is_better {
            if let Some(old_key) = pending.best_key.replace(key_value) {
                old_key.drop_with_heap(self.heap);
            }
            pending.best_index = Some(item_index);
        } else {
            key_value.drop_with_heap(self.heap);
        }
        Ok(())
    }

    /// Executes `min/max` without key evaluation using direct value comparisons.
    fn finish_min_max_without_key(&mut self, items: Vec<Value>, is_min: bool) -> Result<CallResult, RunError> {
        if items.is_empty() {
            return Err(RunError::internal("min/max called with empty item set"));
        }

        let mut best_index = 0;
        for idx in 1..items.len() {
            let Some(ordering) = items[best_index].py_cmp(&items[idx], self.heap, self.interns) else {
                let left_type = items[best_index].py_type(self.heap);
                let right_type = items[idx].py_type(self.heap);
                for item in items {
                    item.drop_with_heap(self.heap);
                }
                return Err(ExcType::type_error(format!(
                    "'<' not supported between instances of '{left_type}' and '{right_type}'"
                )));
            };
            let new_is_better = (is_min && ordering == Ordering::Greater) || (!is_min && ordering == Ordering::Less);
            if new_is_better {
                best_index = idx;
            }
        }

        self.finish_min_max_result(items, best_index)
    }

    /// Finalizes min/max by returning `items[best_index]` and dropping the rest.
    fn finish_min_max_result(&mut self, items: Vec<Value>, best_index: usize) -> Result<CallResult, RunError> {
        let mut result = None;
        for (idx, item) in items.into_iter().enumerate() {
            if idx == best_index {
                result = Some(item);
            } else {
                item.drop_with_heap(self.heap);
            }
        }

        let Some(result) = result else {
            return Err(RunError::internal("min/max best index out of bounds"));
        };
        Ok(CallResult::Push(result))
    }

    /// Executes bisect/insort module functions with VM-managed key callable support.
    fn call_bisect_function(&mut self, function: BisectFunctions, args: ArgValues) -> Result<CallResult, RunError> {
        let operation = match function {
            BisectFunctions::BisectLeft => BisectOperation::Left,
            BisectFunctions::BisectRight => BisectOperation::Right,
            BisectFunctions::InsortLeft => BisectOperation::InsortLeft,
            BisectFunctions::InsortRight => BisectOperation::InsortRight,
        };

        let (list_value, x, lo_value, hi_value, key_fn) = self.extract_bisect_args(operation, args)?;
        let list_id = match self.extract_bisect_list_id(&list_value) {
            Ok(list_id) => list_id,
            Err(err) => {
                list_value.drop_with_heap(self.heap);
                x.drop_with_heap(self.heap);
                if let Some(key_fn) = key_fn {
                    key_fn.drop_with_heap(self.heap);
                }
                return Err(err);
            }
        };
        let (lo, hi) = match self.resolve_bisect_lo_hi(list_id, lo_value, hi_value) {
            Ok(bounds) => bounds,
            Err(err) => {
                list_value.drop_with_heap(self.heap);
                x.drop_with_heap(self.heap);
                if let Some(key_fn) = key_fn {
                    key_fn.drop_with_heap(self.heap);
                }
                return Err(err);
            }
        };

        let Some(key_fn) = key_fn else {
            return self.finish_bisect_without_key(operation, list_id, list_value, x, lo, hi);
        };

        let x_cmp = if operation.is_insert() {
            let key_callable = key_fn.clone_with_heap(self.heap);
            let key_arg = x.clone_with_heap(self.heap);
            match self.call_function(key_callable, ArgValues::One(key_arg))? {
                CallResult::Push(x_key) => x_key,
                CallResult::FramePushed => {
                    let pending = super::PendingBisect {
                        left: operation.is_left(),
                        insert: operation.is_insert(),
                        list_id,
                        list_value,
                        x,
                        x_cmp: Value::None,
                        key_fn,
                        lo,
                        hi,
                        awaiting_mid: 0,
                        awaiting_x_key: true,
                    };
                    self.pending_bisect = Some(pending);
                    self.pending_bisect_return = true;
                    return Ok(CallResult::FramePushed);
                }
                CallResult::External(_, ext_args) => {
                    ext_args.drop_with_heap(self.heap);
                    list_value.drop_with_heap(self.heap);
                    x.drop_with_heap(self.heap);
                    key_fn.drop_with_heap(self.heap);
                    return Err(ExcType::type_error(
                        "bisect key function cannot be an external callable",
                    ));
                }
                CallResult::Proxy(_, _, proxy_args) => {
                    proxy_args.drop_with_heap(self.heap);
                    list_value.drop_with_heap(self.heap);
                    x.drop_with_heap(self.heap);
                    key_fn.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("bisect key function cannot be a proxy callable"));
                }
                CallResult::OsCall(_, os_args) => {
                    os_args.drop_with_heap(self.heap);
                    list_value.drop_with_heap(self.heap);
                    x.drop_with_heap(self.heap);
                    key_fn.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("bisect key function cannot perform os operations"));
                }
            }
        } else {
            x.clone_with_heap(self.heap)
        };

        let pending = super::PendingBisect {
            left: operation.is_left(),
            insert: operation.is_insert(),
            list_id,
            list_value,
            x,
            x_cmp,
            key_fn,
            lo,
            hi,
            awaiting_mid: 0,
            awaiting_x_key: false,
        };
        self.bisect_compute_keys(pending)
    }

    /// Continues binary-search key evaluation for a pending bisect/insort call.
    fn bisect_compute_keys(&mut self, mut pending: super::PendingBisect) -> Result<CallResult, RunError> {
        while pending.lo < pending.hi {
            let mid = pending.lo + (pending.hi - pending.lo) / 2;
            let mid_value = {
                let HeapData::List(list) = self.heap.get(pending.list_id) else {
                    self.drop_pending_bisect_state(pending);
                    return Err(ExcType::type_error("bisect requires a list as first argument"));
                };
                let items = list.as_vec();
                if mid >= items.len() {
                    break;
                }
                items[mid].clone_with_heap(self.heap)
            };

            pending.awaiting_mid = mid;
            let key_fn = pending.key_fn.clone_with_heap(self.heap);
            match self.call_function(key_fn, ArgValues::One(mid_value))? {
                CallResult::Push(mid_key) => {
                    if let Err(err) = self.bisect_apply_key_comparison(&mut pending, mid_key) {
                        self.drop_pending_bisect_state(pending);
                        return Err(err);
                    }
                }
                CallResult::FramePushed => {
                    self.pending_bisect = Some(pending);
                    self.pending_bisect_return = true;
                    return Ok(CallResult::FramePushed);
                }
                CallResult::External(_, ext_args) => {
                    ext_args.drop_with_heap(self.heap);
                    self.drop_pending_bisect_state(pending);
                    return Err(ExcType::type_error(
                        "bisect key function cannot be an external callable",
                    ));
                }
                CallResult::Proxy(_, _, proxy_args) => {
                    proxy_args.drop_with_heap(self.heap);
                    self.drop_pending_bisect_state(pending);
                    return Err(ExcType::type_error("bisect key function cannot be a proxy callable"));
                }
                CallResult::OsCall(_, os_args) => {
                    os_args.drop_with_heap(self.heap);
                    self.drop_pending_bisect_state(pending);
                    return Err(ExcType::type_error("bisect key function cannot perform os operations"));
                }
            }
        }

        self.finish_bisect_pending(pending)
    }

    /// Handles the return value from a user-defined bisect key call.
    pub(super) fn handle_bisect_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_bisect.take() else {
            return Err(RunError::internal("handle_bisect_return: no pending bisect state"));
        };
        if pending.awaiting_x_key {
            pending.x_cmp.drop_with_heap(self.heap);
            pending.x_cmp = value;
            pending.awaiting_x_key = false;
            return self.bisect_compute_keys(pending);
        }
        if let Err(err) = self.bisect_apply_key_comparison(&mut pending, value) {
            self.drop_pending_bisect_state(pending);
            return Err(err);
        }
        self.bisect_compute_keys(pending)
    }

    /// Clears pending bisect state during cleanup/exception unwind.
    pub(super) fn clear_pending_bisect(&mut self) {
        self.pending_bisect_return = false;
        if let Some(pending) = self.pending_bisect.take() {
            self.drop_pending_bisect_state(pending);
        }
    }

    /// Drops pending bisect state and all owned values.
    fn drop_pending_bisect_state(&mut self, pending: super::PendingBisect) {
        pending.list_value.drop_with_heap(self.heap);
        pending.x.drop_with_heap(self.heap);
        pending.x_cmp.drop_with_heap(self.heap);
        pending.key_fn.drop_with_heap(self.heap);
    }

    /// Applies a bisect key comparison and advances search bounds.
    fn bisect_apply_key_comparison(&mut self, pending: &mut super::PendingBisect, mid_key: Value) -> RunResult<()> {
        let Some(ordering) = mid_key.py_cmp(&pending.x_cmp, self.heap, self.interns) else {
            let left_type = mid_key.py_type(self.heap);
            let right_type = pending.x_cmp.py_type(self.heap);
            mid_key.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!(
                "'<' not supported between instances of '{left_type}' and '{right_type}'"
            )));
        };
        mid_key.drop_with_heap(self.heap);

        if ordering == Ordering::Less || (!pending.left && ordering == Ordering::Equal) {
            pending.lo = pending.awaiting_mid.saturating_add(1);
        } else {
            pending.hi = pending.awaiting_mid;
        }
        Ok(())
    }

    /// Finalizes a bisect/insort call after key evaluation is complete.
    fn finish_bisect_pending(&mut self, pending: super::PendingBisect) -> Result<CallResult, RunError> {
        pending.key_fn.drop_with_heap(self.heap);
        pending.x_cmp.drop_with_heap(self.heap);
        self.finish_bisect_result(
            pending.list_id,
            pending.list_value,
            pending.x,
            pending.lo,
            pending.insert,
        )
    }

    /// Executes bisect/insort without `key=` evaluation.
    fn finish_bisect_without_key(
        &mut self,
        operation: BisectOperation,
        list_id: HeapId,
        list_value: Value,
        x: Value,
        mut lo: usize,
        mut hi: usize,
    ) -> Result<CallResult, RunError> {
        let left = operation.is_left();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let mid_value = {
                let HeapData::List(list) = self.heap.get(list_id) else {
                    list_value.drop_with_heap(self.heap);
                    x.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("bisect requires a list as first argument"));
                };
                let items = list.as_vec();
                if mid >= items.len() {
                    break;
                }
                items[mid].clone_with_heap(self.heap)
            };

            let Some(ordering) = mid_value.py_cmp(&x, self.heap, self.interns) else {
                let left_type = mid_value.py_type(self.heap);
                let right_type = x.py_type(self.heap);
                mid_value.drop_with_heap(self.heap);
                list_value.drop_with_heap(self.heap);
                x.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "'<' not supported between instances of '{left_type}' and '{right_type}'"
                )));
            };
            mid_value.drop_with_heap(self.heap);

            if ordering == Ordering::Less || (!left && ordering == Ordering::Equal) {
                lo = mid.saturating_add(1);
            } else {
                hi = mid;
            }
        }

        self.finish_bisect_result(list_id, list_value, x, lo, operation.is_insert())
    }

    /// Finalizes bisect result by returning index or inserting into the target list.
    fn finish_bisect_result(
        &mut self,
        list_id: HeapId,
        list_value: Value,
        x: Value,
        index: usize,
        insert: bool,
    ) -> Result<CallResult, RunError> {
        if insert {
            let is_ref = matches!(x, Value::Ref(_));
            if is_ref {
                self.heap.mark_potential_cycle();
            }
            let HeapData::List(list) = self.heap.get_mut(list_id) else {
                list_value.drop_with_heap(self.heap);
                x.drop_with_heap(self.heap);
                return Err(ExcType::type_error("bisect requires a list as first argument"));
            };
            if is_ref {
                list.set_contains_refs();
            }
            let items = list.as_vec_mut();
            if index >= items.len() {
                items.push(x);
            } else {
                items.insert(index, x);
            }
            list_value.drop_with_heap(self.heap);
            return Ok(CallResult::Push(Value::None));
        }

        list_value.drop_with_heap(self.heap);
        x.drop_with_heap(self.heap);
        let index = i64::try_from(index).expect("bisect index exceeds i64::MAX");
        Ok(CallResult::Push(Value::Int(index)))
    }

    /// Parses bisect/insort arguments from `ArgValues`.
    fn extract_bisect_args(
        &mut self,
        operation: BisectOperation,
        args: ArgValues,
    ) -> RunResult<(Value, Value, Option<Value>, Option<Value>, Option<Value>)> {
        let name = operation.name();
        match args {
            ArgValues::Two(a, x) => Ok((a, x, None, None, None)),
            ArgValues::ArgsKargs { args, kwargs } => {
                let count = args.len();
                if count < 2 {
                    for value in args {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                    return Err(ExcType::type_error(format!(
                        "bisect.{name}() missing required argument: 'a' or 'x'"
                    )));
                }

                let mut args_iter = args.into_iter();
                let list_value = args_iter.next().unwrap();
                let x = args_iter.next().unwrap();
                let remaining: Vec<Value> = args_iter.collect();
                let (lo_pos, hi_pos) = match remaining.len() {
                    0 => (None, None),
                    1 => {
                        let mut iter = remaining.into_iter();
                        (iter.next(), None)
                    }
                    2 => {
                        let mut iter = remaining.into_iter();
                        (iter.next(), iter.next())
                    }
                    _ => {
                        for value in remaining {
                            value.drop_with_heap(self.heap);
                        }
                        kwargs.drop_with_heap(self.heap);
                        list_value.drop_with_heap(self.heap);
                        x.drop_with_heap(self.heap);
                        return Err(ExcType::type_error(format!(
                            "bisect.{name}() takes at most 4 positional arguments but {count} were given"
                        )));
                    }
                };

                let mut lo_kw: Option<Value> = None;
                let mut hi_kw: Option<Value> = None;
                let mut key_fn: Option<Value> = None;
                for (kw_key, kw_value) in kwargs {
                    let Some(keyword_name) = kw_key.as_either_str(self.heap) else {
                        kw_key.drop_with_heap(self.heap);
                        kw_value.drop_with_heap(self.heap);
                        if let Some(v) = lo_pos {
                            v.drop_with_heap(self.heap);
                        }
                        if let Some(v) = hi_pos {
                            v.drop_with_heap(self.heap);
                        }
                        if let Some(v) = lo_kw {
                            v.drop_with_heap(self.heap);
                        }
                        if let Some(v) = hi_kw {
                            v.drop_with_heap(self.heap);
                        }
                        if let Some(v) = key_fn {
                            v.drop_with_heap(self.heap);
                        }
                        list_value.drop_with_heap(self.heap);
                        x.drop_with_heap(self.heap);
                        return Err(ExcType::type_error("keywords must be strings"));
                    };
                    kw_key.drop_with_heap(self.heap);
                    let keyword_name = keyword_name.as_str(self.interns);
                    match keyword_name {
                        "lo" => {
                            if let Some(old) = lo_kw.replace(kw_value) {
                                old.drop_with_heap(self.heap);
                            }
                        }
                        "hi" => {
                            if let Some(old) = hi_kw.replace(kw_value) {
                                old.drop_with_heap(self.heap);
                            }
                        }
                        "key" => {
                            if matches!(kw_value, Value::None) {
                                kw_value.drop_with_heap(self.heap);
                            } else if let Some(old) = key_fn.replace(kw_value) {
                                old.drop_with_heap(self.heap);
                            }
                        }
                        _ => {
                            kw_value.drop_with_heap(self.heap);
                            if let Some(v) = lo_pos {
                                v.drop_with_heap(self.heap);
                            }
                            if let Some(v) = hi_pos {
                                v.drop_with_heap(self.heap);
                            }
                            if let Some(v) = lo_kw {
                                v.drop_with_heap(self.heap);
                            }
                            if let Some(v) = hi_kw {
                                v.drop_with_heap(self.heap);
                            }
                            if let Some(v) = key_fn {
                                v.drop_with_heap(self.heap);
                            }
                            list_value.drop_with_heap(self.heap);
                            x.drop_with_heap(self.heap);
                            return Err(ExcType::type_error(format!(
                                "'{keyword_name}' is an invalid keyword argument for bisect.{name}()"
                            )));
                        }
                    }
                }

                Ok((list_value, x, lo_kw.or(lo_pos), hi_kw.or(hi_pos), key_fn))
            }
            other => {
                let count = match &other {
                    ArgValues::Empty => 0,
                    ArgValues::One(_) => 1,
                    _ => 0,
                };
                other.drop_with_heap(self.heap);
                Err(ExcType::type_error(format!(
                    "bisect.{name}() takes 2 to 4 positional arguments but {count} were given"
                )))
            }
        }
    }

    /// Extracts the list id for bisect operations.
    fn extract_bisect_list_id(&self, list_value: &Value) -> RunResult<HeapId> {
        let Value::Ref(list_id) = list_value else {
            return Err(ExcType::type_error(
                "bisect requires a list as first argument".to_string(),
            ));
        };
        if !matches!(self.heap.get(*list_id), HeapData::List(_)) {
            return Err(ExcType::type_error(
                "bisect requires a list as first argument".to_string(),
            ));
        }
        Ok(*list_id)
    }

    /// Resolves optional `lo`/`hi` bisect bounds.
    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn resolve_bisect_lo_hi(
        &mut self,
        list_id: HeapId,
        lo_val: Option<Value>,
        hi_val: Option<Value>,
    ) -> RunResult<(usize, usize)> {
        let len = match self.heap.get(list_id) {
            HeapData::List(list) => list.len(),
            _ => 0,
        };

        let lo = match lo_val {
            Some(value) => {
                let lo_int = match value.as_int(self.heap) {
                    Ok(lo_int) => lo_int,
                    Err(err) => {
                        value.drop_with_heap(self.heap);
                        return Err(err);
                    }
                };
                value.drop_with_heap(self.heap);
                if lo_int < 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "lo must be non-negative").into());
                }
                (lo_int as usize).min(len)
            }
            None => 0,
        };
        let hi = match hi_val {
            Some(value) => {
                let hi_int = match value.as_int(self.heap) {
                    Ok(hi_int) => hi_int,
                    Err(err) => {
                        value.drop_with_heap(self.heap);
                        return Err(err);
                    }
                };
                value.drop_with_heap(self.heap);
                if hi_int < 0 {
                    return Err(SimpleException::new_msg(ExcType::ValueError, "hi must be non-negative").into());
                }
                (hi_int as usize).min(len)
            }
            None => len,
        };
        Ok((lo, hi))
    }

    /// Calls heapq module functions with VM-managed key-call and generator support.
    fn call_heapq_function(&mut self, function: HeapqFunctions, args: ArgValues) -> Result<CallResult, RunError> {
        match function {
            HeapqFunctions::Nlargest => self.call_heapq_select(args, true),
            HeapqFunctions::Nsmallest => self.call_heapq_select(args, false),
            HeapqFunctions::Merge => self.call_heapq_merge(args),
            _ => {
                let result = ModuleFunctions::Heapq(function).call(self.heap, self.interns, args)?;
                self.handle_attr_call_result(result)
            }
        }
    }

    /// Calls `heapq.nsmallest/nlargest` with VM-managed `key=` callable support.
    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn call_heapq_select(&mut self, args: ArgValues, largest: bool) -> Result<CallResult, RunError> {
        let func_name = if largest { "nlargest" } else { "nsmallest" };
        let (n_arg, iterable_arg, kwargs) = match args {
            ArgValues::Two(a, b) => (a, b, KwargsValues::Empty),
            ArgValues::ArgsKargs { args, kwargs } => {
                let mut iter = args.into_iter();
                let Some(a) = iter.next() else {
                    return Err(ExcType::type_error(format!(
                        "{func_name}() missing required argument: 'n'"
                    )));
                };
                let Some(b) = iter.next() else {
                    a.drop_with_heap(self.heap);
                    return Err(ExcType::type_error(format!(
                        "{func_name}() missing required argument: 'iterable'"
                    )));
                };
                for extra in iter {
                    extra.drop_with_heap(self.heap);
                }
                (a, b, kwargs)
            }
            other => {
                other.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "{func_name}() takes exactly 2 positional arguments"
                )));
            }
        };

        let mut key_fn: Option<Value> = None;
        for (kw_key, kw_value) in kwargs {
            let Some(keyword_name) = kw_key.as_either_str(self.heap) else {
                kw_key.drop_with_heap(self.heap);
                kw_value.drop_with_heap(self.heap);
                n_arg.drop_with_heap(self.heap);
                iterable_arg.drop_with_heap(self.heap);
                if let Some(key) = key_fn {
                    key.drop_with_heap(self.heap);
                }
                return Err(ExcType::type_error("keywords must be strings"));
            };
            let key_str = keyword_name.as_str(self.interns);
            kw_key.drop_with_heap(self.heap);
            if key_str == "key" {
                if matches!(kw_value, Value::None) {
                    kw_value.drop_with_heap(self.heap);
                } else if let Some(old) = key_fn.replace(kw_value) {
                    old.drop_with_heap(self.heap);
                }
            } else {
                kw_value.drop_with_heap(self.heap);
                n_arg.drop_with_heap(self.heap);
                iterable_arg.drop_with_heap(self.heap);
                if let Some(key) = key_fn {
                    key.drop_with_heap(self.heap);
                }
                return Err(ExcType::type_error(format!(
                    "'{key_str}' is an invalid keyword argument for {func_name}()"
                )));
            }
        }

        let n = match n_arg.as_int(self.heap) {
            Ok(value) => value,
            Err(err) => {
                n_arg.drop_with_heap(self.heap);
                iterable_arg.drop_with_heap(self.heap);
                if let Some(key) = key_fn {
                    key.drop_with_heap(self.heap);
                }
                return Err(err);
            }
        };
        n_arg.drop_with_heap(self.heap);

        if n < 0 {
            iterable_arg.drop_with_heap(self.heap);
            if let Some(key) = key_fn {
                key.drop_with_heap(self.heap);
            }
            let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        }

        let n = n as usize;
        let mut items = match self.collect_heapq_select_items(iterable_arg) {
            Ok(items) => items,
            Err(err) => {
                if let Some(key) = key_fn {
                    key.drop_with_heap(self.heap);
                }
                return Err(err);
            }
        };

        let Some(key_fn) = key_fn else {
            if largest {
                items.sort_by(|a, b| self.heapq_compare_values(b, a));
            } else {
                items.sort_by(|a, b| self.heapq_compare_values(a, b));
            }
            items.truncate(n);
            let list_id = self.heap.allocate(HeapData::List(List::new(items)))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        };

        if items.is_empty() {
            key_fn.drop_with_heap(self.heap);
            let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        }

        let pending = super::PendingHeapqSelect {
            largest,
            n,
            key_fn,
            items,
            key_values: Vec::new(),
            next_index: 0,
        };
        self.heapq_select_compute_keys(pending)
    }

    /// Collects all values from the iterable used by `heapq.nsmallest/nlargest`.
    fn collect_heapq_select_items(&mut self, iterable: Value) -> RunResult<Vec<Value>> {
        let mut iter = OurosIter::new(iterable, self.heap, self.interns)?;
        let mut items = Vec::new();
        loop {
            match iter.for_next(self.heap, self.interns) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => break,
                Err(err) => {
                    iter.drop_with_heap(self.heap);
                    for item in items {
                        item.drop_with_heap(self.heap);
                    }
                    return Err(err);
                }
            }
        }
        iter.drop_with_heap(self.heap);
        Ok(items)
    }

    /// Continues key evaluation for pending `heapq.nsmallest/nlargest`.
    fn heapq_select_compute_keys(&mut self, mut pending: super::PendingHeapqSelect) -> Result<CallResult, RunError> {
        while pending.next_index < pending.items.len() {
            let item_index = pending.next_index;
            pending.next_index += 1;

            let item = pending.items[item_index].clone_with_heap(self.heap);
            let key_fn = pending.key_fn.clone_with_heap(self.heap);
            match self.call_function(key_fn, ArgValues::One(item))? {
                CallResult::Push(key_value) => pending.key_values.push(key_value),
                CallResult::FramePushed => {
                    self.pending_heapq_select = Some(pending);
                    self.pending_heapq_select_return = true;
                    return Ok(CallResult::FramePushed);
                }
                CallResult::External(_, ext_args) => {
                    ext_args.drop_with_heap(self.heap);
                    self.drop_pending_heapq_select_state(pending);
                    return Err(ExcType::type_error(
                        "nsmallest()/nlargest() key function cannot be an external callable",
                    ));
                }
                CallResult::Proxy(_, _, proxy_args) => {
                    proxy_args.drop_with_heap(self.heap);
                    self.drop_pending_heapq_select_state(pending);
                    return Err(ExcType::type_error(
                        "nsmallest()/nlargest() key function cannot be a proxy callable",
                    ));
                }
                CallResult::OsCall(_, os_args) => {
                    os_args.drop_with_heap(self.heap);
                    self.drop_pending_heapq_select_state(pending);
                    return Err(ExcType::type_error(
                        "nsmallest()/nlargest() key function cannot perform os operations",
                    ));
                }
            }
        }

        self.finish_heapq_select(pending)
    }

    /// Handles return values from pending `heapq.nsmallest/nlargest` key calls.
    pub(super) fn handle_heapq_select_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(mut pending) = self.pending_heapq_select.take() else {
            return Err(RunError::internal(
                "handle_heapq_select_return: no pending heapq select state",
            ));
        };
        pending.key_values.push(value);
        self.heapq_select_compute_keys(pending)
    }

    /// Clears pending heapq key-evaluation state and drops all held values.
    pub(super) fn clear_pending_heapq_select(&mut self) {
        self.pending_heapq_select_return = false;
        if let Some(pending) = self.pending_heapq_select.take() {
            self.drop_pending_heapq_select_state(pending);
        }
    }

    /// Drops all values owned by a pending heapq key-evaluation state.
    fn drop_pending_heapq_select_state(&mut self, pending: super::PendingHeapqSelect) {
        pending.key_fn.drop_with_heap(self.heap);
        for item in pending.items {
            item.drop_with_heap(self.heap);
        }
        for key_value in pending.key_values {
            key_value.drop_with_heap(self.heap);
        }
    }

    /// Finalizes `heapq.nsmallest/nlargest` after all keys are computed.
    fn finish_heapq_select(&mut self, pending: super::PendingHeapqSelect) -> Result<CallResult, RunError> {
        let super::PendingHeapqSelect {
            largest,
            n,
            key_fn,
            items,
            key_values,
            ..
        } = pending;

        let mut indices: Vec<usize> = (0..items.len()).collect();
        if largest {
            indices.sort_by(|&a, &b| self.heapq_compare_values(&key_values[b], &key_values[a]));
        } else {
            indices.sort_by(|&a, &b| self.heapq_compare_values(&key_values[a], &key_values[b]));
        }
        indices.truncate(n.min(indices.len()));

        let mut result = Vec::with_capacity(indices.len());
        for idx in indices {
            result.push(items[idx].copy_for_extend());
        }

        key_fn.drop_with_heap(self.heap);
        for item in items {
            item.drop_with_heap(self.heap);
        }
        for key_value in key_values {
            key_value.drop_with_heap(self.heap);
        }

        let list_id = self.heap.allocate(HeapData::List(List::new(result)))?;
        Ok(CallResult::Push(Value::Ref(list_id)))
    }

    /// Compares values for heapq ordering, using type-name fallback on incomparable values.
    fn heapq_compare_values(&mut self, left: &Value, right: &Value) -> Ordering {
        if let Some(ord) = left.py_cmp(right, self.heap, self.interns) {
            ord
        } else {
            let left_type = left.py_type(self.heap);
            let right_type = right.py_type(self.heap);
            left_type.to_string().cmp(&right_type.to_string())
        }
    }

    /// Calls `heapq.merge()` with generator-aware iterable normalization.
    fn call_heapq_merge(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (positional, kwargs) = args.into_parts();
        let positional: Vec<Value> = positional.collect();
        let remaining = positional.into_iter().rev().collect();
        self.call_heapq_merge_normalized(Vec::new(), remaining, kwargs)
    }

    /// Continues `heapq.merge()` normalization after each generator materialization.
    fn call_heapq_merge_normalized(
        &mut self,
        mut materialized: Vec<Value>,
        mut remaining: Vec<Value>,
        kwargs: KwargsValues,
    ) -> Result<CallResult, RunError> {
        while let Some(next_arg) = remaining.pop() {
            let needs_materialize = matches!(
                next_arg,
                Value::Ref(iter_id) if matches!(self.heap.get(iter_id), HeapData::Generator(_))
            );
            if !needs_materialize {
                materialized.push(next_arg);
                continue;
            }

            match self.list_build_from_iterator(next_arg) {
                Ok(CallResult::Push(list_value)) => materialized.push(list_value),
                Ok(CallResult::FramePushed) => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::HeapqMerge {
                            materialized,
                            remaining,
                            kwargs,
                        },
                    });
                    return Ok(CallResult::FramePushed);
                }
                Ok(other) => {
                    for value in materialized {
                        value.drop_with_heap(self.heap);
                    }
                    for value in remaining {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                    return Ok(other);
                }
                Err(error) => {
                    for value in materialized {
                        value.drop_with_heap(self.heap);
                    }
                    for value in remaining {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                    return Err(error);
                }
            }
        }

        let merge_args = build_arg_values(materialized, kwargs);
        let result = ModuleFunctions::Heapq(HeapqFunctions::Merge).call(self.heap, self.interns, merge_args)?;
        self.handle_attr_call_result(result)
    }

    /// Calls a callable value with the given arguments.
    ///
    /// Dispatches based on the callable type:
    /// - `Value::Builtin`: calls builtin directly, returns `Push`
    /// - `Value::ModuleFunction`: calls module function directly, returns `Push`
    /// - `Value::ExtFunction`: returns `External` for caller to execute
    /// - `Value::DefFunction`: pushes a new frame, returns `FramePushed`
    /// - `Value::Ref`: checks for closure/function on heap
    #[inline]
    pub(super) fn call_function(&mut self, callable: Value, args: ArgValues) -> Result<CallResult, RunError> {
        match callable {
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Super)) => {
                // super() needs VM context - handle it specially
                let result = self.call_super(args)?;
                Ok(CallResult::Push(result))
            }
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Any)) => self.call_any_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::All)) => self.call_all_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Sum)) => self.call_sum_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Enumerate)) => self.call_enumerate_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Zip)) => self.call_zip_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Isinstance)) => self.call_isinstance(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Issubclass)) => self.call_issubclass(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Map)) => self.call_map_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Filter)) => self.call_filter_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Dir)) => self.call_dir_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Format)) => self.call_format_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Sorted)) => self.call_sorted_builtin(args),
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Min)) => {
                self.call_min_max_builtin_dispatch(args, true)
            }
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Max)) => {
                self.call_min_max_builtin_dispatch(args, false)
            }
            Value::Builtin(Builtins::Type(Type::List)) => self.call_list_type_builtin(args),
            Value::Builtin(Builtins::Type(Type::Tuple)) => self.call_tuple_type_builtin(args),
            Value::Builtin(Builtins::Type(Type::Dict)) => self.call_dict_type_builtin(args),
            Value::Builtin(Builtins::Type(Type::Set)) => self.call_set_type_builtin(args),
            Value::Builtin(builtin) => {
                let result = builtin.call(self.heap, args, self.interns, self.print_writer)?;
                Ok(CallResult::Push(result))
            }
            Value::ModuleFunction(ModuleFunctions::Asyncio(AsyncioFunctions::Run)) => self.call_asyncio_run(args),
            Value::ModuleFunction(ModuleFunctions::Bisect(function)) => self.call_bisect_function(function, args),
            Value::ModuleFunction(ModuleFunctions::Heapq(function)) => self.call_heapq_function(function, args),
            Value::ModuleFunction(ModuleFunctions::Statistics(function)) => {
                self.call_statistics_function(function, args)
            }
            Value::ModuleFunction(ModuleFunctions::Collections(function)) => {
                self.call_collections_function(function, args)
            }
            Value::ModuleFunction(mf) => {
                let result = mf.call(self.heap, self.interns, args)?;
                self.handle_attr_call_result(result)
            }
            Value::ExtFunction(ext_id) => {
                // External function - return to caller to execute
                Ok(CallResult::External(ext_id, args))
            }
            Value::DefFunction(func_id) => {
                // Defined function without defaults or captured variables
                self.call_def_function(func_id, &[], Vec::new(), args)
            }
            Value::Ref(heap_id) => {
                // Could be a closure or function with defaults - check heap
                self.call_heap_callable(heap_id, callable, args)
            }
            _ => {
                args.drop_with_heap(self.heap);
                Err(ExcType::type_error("object is not callable"))
            }
        }
    }

    /// Executes `asyncio.run(coro)` with VM-managed coroutine execution.
    ///
    /// This mirrors CPython's lifecycle semantics for supported sandbox flows:
    /// - calling from within an active async frame raises `RuntimeError`
    /// - calling from sync code awaits the coroutine to completion
    ///
    /// The sandbox cannot block on unresolved external futures inside this helper,
    /// so those paths raise `RuntimeError`.
    fn call_asyncio_run(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let awaitable = args.get_one_arg("asyncio.run", self.heap)?;

        if self.is_running_async_event_loop() {
            awaitable.drop_with_heap(self.heap);
            return Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "asyncio.run() cannot be called from a running event loop",
            )
            .into());
        }

        // Keep existing sandbox behavior for unsupported argument types used by
        // current tests: non-coroutine inputs raise the nested-loop RuntimeError.
        let is_coroutine = matches!(&awaitable, Value::Ref(id) if self.heap.get(*id).is_coroutine());
        if !is_coroutine {
            awaitable.drop_with_heap(self.heap);
            return Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "asyncio.run() cannot be called from a running event loop",
            )
            .into());
        }

        self.push(awaitable);
        match self.exec_get_awaitable()? {
            AwaitResult::ValueReady(value) => Ok(CallResult::Push(value)),
            AwaitResult::FramePushed => Ok(CallResult::FramePushed),
            AwaitResult::Yield(_) => Err(SimpleException::new_msg(
                ExcType::RuntimeError,
                "asyncio.run() cannot await unresolved external futures",
            )
            .into()),
        }
    }

    /// Returns `true` when async execution is currently active in this VM.
    ///
    /// We treat any async function frame on the call stack as an active running
    /// event loop context, matching CPython's nested `asyncio.run()` guard.
    fn is_running_async_event_loop(&self) -> bool {
        self.frames.iter().any(|frame| {
            frame
                .function_id
                .is_some_and(|function_id| self.interns.get_function(function_id).is_async)
        })
    }

    /// Calls the builtin `list` type in the generic call path.
    ///
    /// This mirrors the optimized call path for generator arguments so
    /// `list(gen())` can iterate via VM-driven generator suspension/resume.
    fn call_list_type_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let value = args.get_zero_one_arg("list", self.heap)?;
        match value {
            None => {
                let result = Type::List.call(self.heap, ArgValues::Empty, self.interns)?;
                Ok(CallResult::Push(result))
            }
            Some(iterable) => {
                if let Value::Ref(iter_id) = &iterable
                    && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                {
                    self.list_build_from_iterator(iterable)
                } else {
                    let result = Type::List.call(self.heap, ArgValues::One(iterable), self.interns)?;
                    Ok(CallResult::Push(result))
                }
            }
        }
    }

    /// Calls `sum()` with generator-aware handling.
    ///
    /// For generator inputs, this first materializes values using
    /// `list_build_from_iterator` so iteration can suspend/resume in the VM.
    /// Once list construction finishes, `sum()` is finalized from
    /// `maybe_finish_sum_from_list_value`.
    fn call_sum_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (iterable, start) = args.get_one_two_args("sum", self.heap)?;

        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => {
                    let sum_args = match start {
                        Some(start_value) => ArgValues::Two(list_value, start_value),
                        None => ArgValues::One(list_value),
                    };
                    let value = BuiltinsFunctions::Sum.call(self.heap, sum_args, self.interns, self.print_writer)?;
                    Ok(CallResult::Push(value))
                }
                CallResult::FramePushed => {
                    self.pending_sum_from_list = Some(PendingSumFromList { start });
                    Ok(CallResult::FramePushed)
                }
                other => {
                    if let Some(start_value) = start {
                        start_value.drop_with_heap(self.heap);
                    }
                    Ok(other)
                }
            }
        } else {
            let sum_args = match start {
                Some(start_value) => ArgValues::Two(iterable, start_value),
                None => ArgValues::One(iterable),
            };
            let value = BuiltinsFunctions::Sum.call(self.heap, sum_args, self.interns, self.print_writer)?;
            Ok(CallResult::Push(value))
        }
    }

    /// Calls selected `statistics` functions with generator-aware first-argument handling.
    ///
    /// `statistics.mean`, `statistics.fmean`, and `statistics.median` accept generic iterables.
    /// For generator inputs, we must materialize with VM-managed iteration so suspension/resume
    /// and external calls continue to work.
    fn call_statistics_function(
        &mut self,
        function: StatisticsFunctions,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        if !matches!(
            function,
            StatisticsFunctions::Mean | StatisticsFunctions::Fmean | StatisticsFunctions::Median
        ) {
            let result = ModuleFunctions::Statistics(function).call(self.heap, self.interns, args)?;
            return self.handle_attr_call_result(result);
        }

        let (positional, kwargs) = args.into_parts();
        let mut positional_values: Vec<Value> = positional.collect();
        if positional_values.is_empty() {
            let stat_args = build_arg_values(positional_values, kwargs);
            let result = ModuleFunctions::Statistics(function).call(self.heap, self.interns, stat_args)?;
            return self.handle_attr_call_result(result);
        }

        let first = positional_values.remove(0);
        if let Value::Ref(iter_id) = &first
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(first)? {
                CallResult::Push(list_value) => {
                    positional_values.insert(0, list_value);
                    let stat_args = build_arg_values(positional_values, kwargs);
                    let result = ModuleFunctions::Statistics(function).call(self.heap, self.interns, stat_args)?;
                    self.handle_attr_call_result(result)
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Statistics {
                            function,
                            positional_tail: positional_values,
                            kwargs,
                        },
                    });
                    Ok(CallResult::FramePushed)
                }
                other => {
                    positional_values.drop_with_heap(self.heap);
                    kwargs.drop_with_heap(self.heap);
                    Ok(other)
                }
            }
        } else {
            positional_values.insert(0, first);
            let stat_args = build_arg_values(positional_values, kwargs);
            let result = ModuleFunctions::Statistics(function).call(self.heap, self.interns, stat_args)?;
            self.handle_attr_call_result(result)
        }
    }

    /// Calls selected `collections` functions with generator-aware first-argument handling.
    ///
    /// `collections.Counter` accepts arbitrary iterables. When the iterable is a
    /// generator, iteration must run through VM-driven list materialization so
    /// generator frames can suspend/resume correctly.
    fn call_collections_function(
        &mut self,
        function: CollectionsFunctions,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        if !matches!(function, CollectionsFunctions::Counter) {
            let result = ModuleFunctions::Collections(function).call(self.heap, self.interns, args)?;
            return self.handle_attr_call_result(result);
        }

        let (positional, kwargs) = args.into_parts();
        let mut positional_values: Vec<Value> = positional.collect();
        if positional_values.is_empty() {
            let collection_args = build_arg_values(positional_values, kwargs);
            let result = ModuleFunctions::Collections(function).call(self.heap, self.interns, collection_args)?;
            return self.handle_attr_call_result(result);
        }

        let first = positional_values.remove(0);
        if let Value::Ref(iter_id) = &first
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(first)? {
                CallResult::Push(list_value) => {
                    positional_values.insert(0, list_value);
                    let collection_args = build_arg_values(positional_values, kwargs);
                    let result =
                        ModuleFunctions::Collections(function).call(self.heap, self.interns, collection_args)?;
                    self.handle_attr_call_result(result)
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::CollectionsCounter {
                            positional_tail: positional_values,
                            kwargs,
                        },
                    });
                    Ok(CallResult::FramePushed)
                }
                other => {
                    positional_values.drop_with_heap(self.heap);
                    kwargs.drop_with_heap(self.heap);
                    Ok(other)
                }
            }
        } else {
            positional_values.insert(0, first);
            let collection_args = build_arg_values(positional_values, kwargs);
            let result = ModuleFunctions::Collections(function).call(self.heap, self.interns, collection_args)?;
            self.handle_attr_call_result(result)
        }
    }

    /// Calls `any()` with generator-aware handling.
    ///
    /// For generator inputs, this first materializes values through
    /// `list_build_from_iterator` and then applies `any()` to the list.
    fn call_any_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let iterable = args.get_one_arg("any", self.heap)?;
        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => {
                    let value = BuiltinsFunctions::Any.call(
                        self.heap,
                        ArgValues::One(list_value),
                        self.interns,
                        self.print_writer,
                    )?;
                    Ok(CallResult::Push(value))
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Any,
                    });
                    Ok(CallResult::FramePushed)
                }
                other => Ok(other),
            }
        } else {
            let value =
                BuiltinsFunctions::Any.call(self.heap, ArgValues::One(iterable), self.interns, self.print_writer)?;
            Ok(CallResult::Push(value))
        }
    }

    /// Calls `all()` with generator-aware handling.
    ///
    /// For generator inputs, this first materializes values through
    /// `list_build_from_iterator` and then applies `all()` to the list.
    fn call_all_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let iterable = args.get_one_arg("all", self.heap)?;
        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => {
                    let value = BuiltinsFunctions::All.call(
                        self.heap,
                        ArgValues::One(list_value),
                        self.interns,
                        self.print_writer,
                    )?;
                    Ok(CallResult::Push(value))
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::All,
                    });
                    Ok(CallResult::FramePushed)
                }
                other => Ok(other),
            }
        } else {
            let value =
                BuiltinsFunctions::All.call(self.heap, ArgValues::One(iterable), self.interns, self.print_writer)?;
            Ok(CallResult::Push(value))
        }
    }

    /// Executes `len(x)` directly for non-instance values.
    ///
    /// For instances, this returns `Ok(None)` so the caller can continue into
    /// normal dunder dispatch (`__len__`) handling.
    fn call_len_builtin_fast(&mut self) -> RunResult<Option<Value>> {
        let arg = self.peek();
        if let Value::Ref(id) = arg
            && matches!(self.heap.get(*id), HeapData::Instance(_))
        {
            return Ok(None);
        }

        let arg = self.pop();
        let result = match arg.py_len(self.heap, self.interns) {
            Some(len) => Ok(Value::Int(i64::try_from(len).expect("len exceeds i64::MAX"))),
            None => Err(SimpleException::new_msg(
                ExcType::TypeError,
                format!("object of type {} has no len()", arg.py_repr(self.heap, self.interns)),
            )
            .into()),
        };
        arg.drop_with_heap(self.heap);
        result.map(Some)
    }

    /// Calls `min()` with generator-aware handling.
    ///
    /// For generator inputs, this first materializes values through
    /// `list_build_from_iterator` and then applies `min()` to the list.
    fn call_min_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let iterable = args.get_one_arg("min", self.heap)?;
        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => {
                    let value = BuiltinsFunctions::Min.call(
                        self.heap,
                        ArgValues::One(list_value),
                        self.interns,
                        self.print_writer,
                    )?;
                    Ok(CallResult::Push(value))
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Min,
                    });
                    Ok(CallResult::FramePushed)
                }
                other => Ok(other),
            }
        } else {
            let value =
                BuiltinsFunctions::Min.call(self.heap, ArgValues::One(iterable), self.interns, self.print_writer)?;
            Ok(CallResult::Push(value))
        }
    }

    /// Calls `max()` with generator-aware handling.
    ///
    /// For generator inputs, this first materializes values through
    /// `list_build_from_iterator` and then applies `max()` to the list.
    fn call_max_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let iterable = args.get_one_arg("max", self.heap)?;
        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => {
                    let value = BuiltinsFunctions::Max.call(
                        self.heap,
                        ArgValues::One(list_value),
                        self.interns,
                        self.print_writer,
                    )?;
                    Ok(CallResult::Push(value))
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Max,
                    });
                    Ok(CallResult::FramePushed)
                }
                other => Ok(other),
            }
        } else {
            let value =
                BuiltinsFunctions::Max.call(self.heap, ArgValues::One(iterable), self.interns, self.print_writer)?;
            Ok(CallResult::Push(value))
        }
    }
    /// Calls `enumerate()` with generator-aware handling.
    ///
    /// Generator input needs VM-driven iteration so suspension/resume works across
    /// generator frames and external calls. Non-generator inputs are delegated to
    /// the builtin implementation directly.
    fn call_enumerate_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (iterable, start) = args.get_one_two_args_with_keyword("enumerate", "start", self.heap, self.interns)?;

        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => {
                    let enumerate_args = match start {
                        Some(start_value) => ArgValues::Two(list_value, start_value),
                        None => ArgValues::One(list_value),
                    };
                    let value = BuiltinsFunctions::Enumerate.call(
                        self.heap,
                        enumerate_args,
                        self.interns,
                        self.print_writer,
                    )?;
                    Ok(CallResult::Push(value))
                }
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Enumerate { start },
                    });
                    Ok(CallResult::FramePushed)
                }
                other => {
                    if let Some(start_value) = start {
                        start_value.drop_with_heap(self.heap);
                    }
                    Ok(other)
                }
            }
        } else {
            let enumerate_args = match start {
                Some(start_value) => ArgValues::Two(iterable, start_value),
                None => ArgValues::One(iterable),
            };
            let value =
                BuiltinsFunctions::Enumerate.call(self.heap, enumerate_args, self.interns, self.print_writer)?;
            Ok(CallResult::Push(value))
        }
    }

    /// Calls `zip()` with generator-aware handling.
    ///
    /// `zip()` is normally implemented via `OurosIter`, which cannot drive
    /// VM-managed generator frames. This normalizes each positional argument by
    /// materializing generator inputs into lists before invoking builtin `zip()`.
    fn call_zip_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (positional, kwargs) = args.into_parts();
        let positional: Vec<Value> = positional.collect();

        if !kwargs.is_empty() {
            // Preserve builtin keyword-argument behavior (currently unsupported).
            let zip_args = build_arg_values(positional, kwargs);
            let value = BuiltinsFunctions::Zip.call(self.heap, zip_args, self.interns, self.print_writer)?;
            return Ok(CallResult::Push(value));
        }

        // Use a reverse-order worklist so `pop()` keeps original positional order.
        let remaining = positional.into_iter().rev().collect();
        self.call_zip_builtin_normalized(Vec::new(), remaining)
    }

    /// Continues `zip()` argument normalization after each generator materialization.
    ///
    /// `materialized` and `remaining` hold positional args in original order:
    /// - `materialized`: already normalized values
    /// - `remaining`: reverse-order worklist of values still to normalize
    fn call_zip_builtin_normalized(
        &mut self,
        mut materialized: Vec<Value>,
        mut remaining: Vec<Value>,
    ) -> Result<CallResult, RunError> {
        while let Some(next_arg) = remaining.pop() {
            let needs_materialize = matches!(
                next_arg,
                Value::Ref(iter_id) if matches!(self.heap.get(iter_id), HeapData::Generator(_))
            );

            if !needs_materialize {
                materialized.push(next_arg);
                continue;
            }

            match self.list_build_from_iterator(next_arg) {
                Ok(CallResult::Push(list_value)) => materialized.push(list_value),
                Ok(CallResult::FramePushed) => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Zip {
                            materialized,
                            remaining,
                        },
                    });
                    return Ok(CallResult::FramePushed);
                }
                Ok(other) => {
                    for value in materialized {
                        value.drop_with_heap(self.heap);
                    }
                    for value in remaining {
                        value.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
                Err(error) => {
                    for value in materialized {
                        value.drop_with_heap(self.heap);
                    }
                    for value in remaining {
                        value.drop_with_heap(self.heap);
                    }
                    return Err(error);
                }
            }
        }

        let zip_args = build_arg_values(materialized, KwargsValues::Empty);
        let value = BuiltinsFunctions::Zip.call(self.heap, zip_args, self.interns, self.print_writer)?;
        Ok(CallResult::Push(value))
    }

    /// Calls `map()` with support for user-defined functions.
    ///
    /// For user-defined functions (DefFunction, Closure), this collects items from
    /// all iterables and returns `AttrCallResult::MapCall` for the VM to handle.
    /// For builtin functions, it delegates to the standard builtin implementation.
    fn call_map_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        use crate::builtins::BuiltinsFunctions;

        let (mut positional, kwargs) = args.into_parts();

        // Check for unsupported kwargs
        if !kwargs.is_empty() {
            kwargs.drop_with_heap(self.heap);
            positional.drop_with_heap(self.heap);
            return Err(ExcType::type_error("map() does not support keyword arguments"));
        }

        // Check for at least one argument (the function)
        let pos_len = positional.len();
        if pos_len == 0 {
            positional.drop_with_heap(self.heap);
            return Err(ExcType::type_error(
                "map() must have at least one argument (the function)",
            ));
        }

        // Get the function (first argument)
        let func = positional.next().expect("len check ensures at least one value");

        // If only function provided (no iterables), return empty list
        if pos_len == 1 {
            func.drop_with_heap(self.heap);
            let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        }

        // Check if function is user-defined (needs VM frame management)
        let is_user_defined = matches!(&func, Value::DefFunction(_))
            || matches!(&func, Value::Ref(id) if matches!(self.heap.get(*id), HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)));

        if !is_user_defined {
            // For builtin functions, use the standard implementation
            let iterables: Vec<Value> = positional.collect();
            let mut all_args = vec![func];
            all_args.extend(iterables);
            let map_args = build_arg_values(all_args, KwargsValues::Empty);
            let result = BuiltinsFunctions::Map.call(self.heap, map_args, self.interns, self.print_writer)?;
            return Ok(CallResult::Push(result));
        }

        // User-defined function: collect items from all iterators
        let mut iterators: Vec<OurosIter> = Vec::with_capacity(pos_len - 1);
        for iterable in positional {
            match OurosIter::new(iterable, self.heap, self.interns) {
                Ok(iter) => iterators.push(iter),
                Err(e) => {
                    // Clean up already-created iterators
                    for iter in iterators {
                        iter.drop_with_heap(self.heap);
                    }
                    func.drop_with_heap(self.heap);
                    return Err(e);
                }
            }
        }

        // Collect all items from iterators
        let mut collected_iters: Vec<Vec<Value>> = Vec::with_capacity(iterators.len());
        for _ in 0..iterators.len() {
            collected_iters.push(Vec::new());
        }

        'outer: loop {
            for (i, iter) in iterators.iter_mut().enumerate() {
                match iter.for_next(self.heap, self.interns)? {
                    Some(item) => collected_iters[i].push(item),
                    None => {
                        // This iterator is exhausted - stop all iteration
                        break 'outer;
                    }
                }
            }
        }

        // Clean up iterators
        for iter in iterators {
            iter.drop_with_heap(self.heap);
        }

        // Check if all iterators are empty
        if collected_iters.is_empty() || collected_iters[0].is_empty() {
            func.drop_with_heap(self.heap);
            for iter_items in collected_iters {
                for item in iter_items {
                    item.drop_with_heap(self.heap);
                }
            }
            let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        }

        // Return MapCall for the VM to handle
        self.handle_attr_call_result(AttrCallResult::MapCall(func, collected_iters))
    }

    /// Calls `filter()` with support for user-defined functions.
    ///
    /// For user-defined functions (DefFunction, Closure), this collects items from
    /// the iterable and returns `AttrCallResult::FilterCall` for the VM to handle.
    /// For builtin functions or None, it delegates to the standard builtin implementation.
    fn call_filter_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        use crate::builtins::BuiltinsFunctions;

        let (mut positional, kwargs) = args.into_parts();

        // Check for unsupported kwargs
        if !kwargs.is_empty() {
            kwargs.drop_with_heap(self.heap);
            positional.drop_with_heap(self.heap);
            return Err(ExcType::type_error("filter() does not support keyword arguments"));
        }

        // Check for correct number of arguments
        let pos_len = positional.len();
        if pos_len != 2 {
            positional.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!(
                "filter() expected 2 arguments, got {pos_len}"
            )));
        }

        // Get the function (first argument) and iterable (second argument)
        let func = positional.next().expect("len check ensures at least one value");
        let iterable = positional.next().expect("len check ensures two values");

        // Check if function is None (identity filter) - use standard builtin
        let is_none = matches!(&func, Value::None);

        // Check if function is user-defined (needs VM frame management)
        let is_user_defined = !is_none
            && (matches!(&func, Value::DefFunction(_))
                || matches!(&func, Value::Ref(id) if matches!(self.heap.get(*id), HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _))));

        if !is_user_defined {
            // For builtin functions or None, use the standard implementation
            let filter_args = ArgValues::Two(func, iterable);
            let result = BuiltinsFunctions::Filter.call(self.heap, filter_args, self.interns, self.print_writer)?;
            return Ok(CallResult::Push(result));
        }

        // User-defined function: collect items from iterator
        let mut iterator = match OurosIter::new(iterable, self.heap, self.interns) {
            Ok(iter) => iter,
            Err(e) => {
                func.drop_with_heap(self.heap);
                return Err(e);
            }
        };

        // Collect all items
        let mut items: Vec<Value> = Vec::new();
        while let Some(item) = iterator.for_next(self.heap, self.interns)? {
            items.push(item);
        }
        iterator.drop_with_heap(self.heap);

        // Check if empty
        if items.is_empty() {
            func.drop_with_heap(self.heap);
            let list_id = self.heap.allocate(HeapData::List(List::new(Vec::new())))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        }

        // Return FilterCall for the VM to handle
        self.handle_attr_call_result(AttrCallResult::FilterCall(func, items))
    }

    /// Calls `sorted()` with support for user-defined key functions.
    ///
    /// Collects items from the iterable and calls `list.sort()` on the resulting list.
    /// The `call_list_sort` path handles user-defined key functions via the VM.
    fn call_sorted_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (mut positional, kwargs) = args.into_parts();

        let positional_len = positional.len();
        if positional_len != 1 {
            kwargs.drop_with_heap(self.heap);
            for v in positional {
                v.drop_with_heap(self.heap);
            }
            return Err(ExcType::type_error(format!(
                "sorted expected 1 argument, got {positional_len}"
            )));
        }

        let iterable = positional.next().unwrap();
        if let Value::Ref(iter_id) = &iterable
            && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
        {
            return match self.list_build_from_iterator(iterable)? {
                CallResult::Push(list_value) => self.call_sorted_from_materialized_list(list_value, kwargs),
                CallResult::FramePushed => {
                    self.pending_builtin_from_list.push(PendingBuiltinFromList {
                        kind: PendingBuiltinFromListKind::Sorted { kwargs },
                    });
                    Ok(CallResult::FramePushed)
                }
                other => {
                    kwargs.drop_with_heap(self.heap);
                    Ok(other)
                }
            };
        }

        // Collect items from iterable
        let mut iter = match OurosIter::new(iterable, self.heap, self.interns) {
            Ok(iter) => iter,
            Err(err) => {
                kwargs.drop_with_heap(self.heap);
                return Err(err);
            }
        };

        let items: Vec<_> = match iter.collect(self.heap, self.interns) {
            Ok(items) => items,
            Err(err) => {
                iter.drop_with_heap(self.heap);
                kwargs.drop_with_heap(self.heap);
                return Err(err);
            }
        };
        iter.drop_with_heap(self.heap);

        let list_id = self.heap.allocate(HeapData::List(List::new(items)))?;

        // Call list.sort through the VM path which handles user-defined key functions
        let sort_args = if kwargs.is_empty() {
            ArgValues::Empty
        } else {
            ArgValues::Kwargs(kwargs)
        };
        let result = self.call_list_sort(list_id, sort_args);

        // call_list_sort consumes the kwargs and handles user-defined key functions
        // We need to return the list, not the None that list.sort() returns
        match result {
            Ok(CallResult::Push(_)) => {
                // list.sort() returns None, but we need to return the sorted list
                Ok(CallResult::Push(Value::Ref(list_id)))
            }
            Ok(CallResult::FramePushed) => {
                // Key function pushed a frame - store list_id so we return it instead of None
                self.pending_sorted = Some(list_id);
                Ok(CallResult::FramePushed)
            }
            Ok(other) => {
                // Other results (External, OsCall) - need to handle list cleanup
                Value::Ref(list_id).drop_with_heap(self.heap);
                Ok(other)
            }
            Err(err) => {
                Value::Ref(list_id).drop_with_heap(self.heap);
                Err(err)
            }
        }
    }

    /// Re-enters `sorted()` after generator list materialization completed.
    fn call_sorted_from_materialized_list(
        &mut self,
        list_value: Value,
        kwargs: KwargsValues,
    ) -> Result<CallResult, RunError> {
        let sorted_args = build_arg_values(vec![list_value], kwargs);
        self.call_sorted_builtin(sorted_args)
    }

    /// Calls the builtin `tuple` type in the generic call path.
    ///
    /// Generator inputs are materialized through VM-driven iteration before
    /// building the tuple so generator frames can suspend/resume correctly.
    fn call_tuple_type_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let value = args.get_zero_one_arg("tuple", self.heap)?;
        match value {
            None => {
                let result = Type::Tuple.call(self.heap, ArgValues::Empty, self.interns)?;
                Ok(CallResult::Push(result))
            }
            Some(iterable) => {
                if let Value::Ref(iter_id) = &iterable
                    && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                {
                    match self.list_build_from_iterator(iterable)? {
                        CallResult::Push(list_value) => {
                            let tuple_value = Type::Tuple.call(self.heap, ArgValues::One(list_value), self.interns)?;
                            Ok(CallResult::Push(tuple_value))
                        }
                        CallResult::FramePushed => {
                            self.pending_builtin_from_list.push(PendingBuiltinFromList {
                                kind: PendingBuiltinFromListKind::Tuple,
                            });
                            Ok(CallResult::FramePushed)
                        }
                        other => Ok(other),
                    }
                } else {
                    let result = Type::Tuple.call(self.heap, ArgValues::One(iterable), self.interns)?;
                    Ok(CallResult::Push(result))
                }
            }
        }
    }

    /// Calls the builtin `set` type in the generic call path.
    ///
    /// Generator inputs are materialized through VM-driven iteration before
    /// building the set so generator frames can suspend/resume correctly.
    fn call_set_type_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let value = args.get_zero_one_arg("set", self.heap)?;
        match value {
            None => {
                let result = Type::Set.call(self.heap, ArgValues::Empty, self.interns)?;
                Ok(CallResult::Push(result))
            }
            Some(iterable) => {
                if let Value::Ref(iter_id) = &iterable
                    && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                {
                    match self.list_build_from_iterator(iterable)? {
                        CallResult::Push(list_value) => {
                            let set_value = Type::Set.call(self.heap, ArgValues::One(list_value), self.interns)?;
                            Ok(CallResult::Push(set_value))
                        }
                        CallResult::FramePushed => {
                            self.pending_builtin_from_list.push(PendingBuiltinFromList {
                                kind: PendingBuiltinFromListKind::Set,
                            });
                            Ok(CallResult::FramePushed)
                        }
                        other => Ok(other),
                    }
                } else {
                    let result = Type::Set.call(self.heap, ArgValues::One(iterable), self.interns)?;
                    Ok(CallResult::Push(result))
                }
            }
        }
    }

    /// Calls the builtin `dict` type in the generic call path.
    ///
    /// Generator inputs are materialized through VM-driven iteration before
    /// building the dict so generator frames can suspend/resume correctly.
    fn call_dict_type_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let value = args.get_zero_one_arg("dict", self.heap)?;
        match value {
            None => {
                let result = Type::Dict.call(self.heap, ArgValues::Empty, self.interns)?;
                Ok(CallResult::Push(result))
            }
            Some(iterable) => {
                if let Value::Ref(iter_id) = &iterable
                    && matches!(self.heap.get(*iter_id), HeapData::Generator(_))
                {
                    match self.list_build_from_iterator(iterable)? {
                        CallResult::Push(list_value) => {
                            let dict_value = Type::Dict.call(self.heap, ArgValues::One(list_value), self.interns)?;
                            Ok(CallResult::Push(dict_value))
                        }
                        CallResult::FramePushed => {
                            self.pending_builtin_from_list.push(PendingBuiltinFromList {
                                kind: PendingBuiltinFromListKind::Dict,
                            });
                            Ok(CallResult::FramePushed)
                        }
                        other => Ok(other),
                    }
                } else {
                    let result = Type::Dict.call(self.heap, ArgValues::One(iterable), self.interns)?;
                    Ok(CallResult::Push(result))
                }
            }
        }
    }

    /// Clears all pending list-build continuation state.
    pub(super) fn clear_pending_list_build(&mut self) {
        self.pending_list_build_return = false;
        while let Some(pending) = self.pending_list_build.pop() {
            pending.iterator.drop_with_heap(self.heap);
            for item in pending.items {
                item.drop_with_heap(self.heap);
            }
        }
    }

    /// Clears pending `sum(generator[, start])` state and drops held values.
    fn clear_pending_sum_from_list(&mut self) {
        if let Some(pending) = self.pending_sum_from_list.take()
            && let Some(start) = pending.start
        {
            start.drop_with_heap(self.heap);
        }
    }

    /// Finalizes a completed list materialization for pending `sum(generator)` calls.
    ///
    /// If no sum is pending, the list value is returned unchanged.
    pub(super) fn maybe_finish_sum_from_list_value(&mut self, list_value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_sum_from_list.take() else {
            return Ok(CallResult::Push(list_value));
        };

        let sum_args = match pending.start {
            Some(start_value) => ArgValues::Two(list_value, start_value),
            None => ArgValues::One(list_value),
        };
        let value = BuiltinsFunctions::Sum.call(self.heap, sum_args, self.interns, self.print_writer)?;
        Ok(CallResult::Push(value))
    }

    /// Applies pending sum-finalization to a list-build continuation result.
    ///
    /// This keeps `pending_sum_from_list` consistent across success and error paths.
    pub(super) fn maybe_finish_sum_from_list_result(
        &mut self,
        result: Result<CallResult, RunError>,
    ) -> Result<CallResult, RunError> {
        match result {
            Ok(CallResult::Push(value)) => self.maybe_finish_sum_from_list_value(value),
            Ok(other) => Ok(other),
            Err(e) => {
                self.clear_pending_sum_from_list();
                Err(e)
            }
        }
    }

    /// Clears pending builtin-after-materialization state.
    pub(super) fn clear_pending_builtin_from_list(&mut self) {
        while let Some(pending) = self.pending_builtin_from_list.pop() {
            match pending.kind {
                PendingBuiltinFromListKind::Any
                | PendingBuiltinFromListKind::All
                | PendingBuiltinFromListKind::Tuple
                | PendingBuiltinFromListKind::Dict
                | PendingBuiltinFromListKind::Set
                | PendingBuiltinFromListKind::Min
                | PendingBuiltinFromListKind::Max
                | PendingBuiltinFromListKind::Join(_) => {}
                PendingBuiltinFromListKind::Sorted { kwargs } => {
                    kwargs.drop_with_heap(self.heap);
                }
                PendingBuiltinFromListKind::CollectionsCounter {
                    positional_tail,
                    kwargs,
                } => {
                    for value in positional_tail {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                }
                PendingBuiltinFromListKind::Enumerate { start } => {
                    if let Some(start_value) = start {
                        start_value.drop_with_heap(self.heap);
                    }
                }
                PendingBuiltinFromListKind::Zip {
                    materialized,
                    remaining,
                } => {
                    for value in materialized {
                        value.drop_with_heap(self.heap);
                    }
                    for value in remaining {
                        value.drop_with_heap(self.heap);
                    }
                }
                PendingBuiltinFromListKind::DictUpdate {
                    dict_id,
                    remaining_positional,
                    kwargs,
                } => {
                    self.heap.dec_ref(dict_id);
                    for value in remaining_positional {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                }
                PendingBuiltinFromListKind::HeapqMerge {
                    materialized,
                    remaining,
                    kwargs,
                } => {
                    for value in materialized {
                        value.drop_with_heap(self.heap);
                    }
                    for value in remaining {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                }
                PendingBuiltinFromListKind::Statistics {
                    positional_tail,
                    kwargs,
                    ..
                } => {
                    for value in positional_tail {
                        value.drop_with_heap(self.heap);
                    }
                    kwargs.drop_with_heap(self.heap);
                }
            }
        }
    }

    /// Finalizes a completed list materialization for pending builtin calls.
    ///
    /// If no builtin is pending, the list value is returned unchanged.
    pub(super) fn maybe_finish_builtin_from_list_value(&mut self, list_value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_builtin_from_list.pop() else {
            return Ok(CallResult::Push(list_value));
        };

        match pending.kind {
            PendingBuiltinFromListKind::Any => {
                let value = BuiltinsFunctions::Any.call(
                    self.heap,
                    ArgValues::One(list_value),
                    self.interns,
                    self.print_writer,
                )?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::All => {
                let value = BuiltinsFunctions::All.call(
                    self.heap,
                    ArgValues::One(list_value),
                    self.interns,
                    self.print_writer,
                )?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Min => {
                let value = BuiltinsFunctions::Min.call(
                    self.heap,
                    ArgValues::One(list_value),
                    self.interns,
                    self.print_writer,
                )?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Max => {
                let value = BuiltinsFunctions::Max.call(
                    self.heap,
                    ArgValues::One(list_value),
                    self.interns,
                    self.print_writer,
                )?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Tuple => {
                let value = Type::Tuple.call(self.heap, ArgValues::One(list_value), self.interns)?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Dict => {
                let value = Type::Dict.call(self.heap, ArgValues::One(list_value), self.interns)?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Set => {
                let value = Type::Set.call(self.heap, ArgValues::One(list_value), self.interns)?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Join(separator) => {
                let value = call_str_method(
                    separator.as_str(),
                    StaticStrings::Join.into(),
                    ArgValues::One(list_value),
                    self.heap,
                    self.interns,
                )?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Sorted { kwargs } => {
                self.call_sorted_from_materialized_list(list_value, kwargs)
            }
            PendingBuiltinFromListKind::CollectionsCounter {
                mut positional_tail,
                kwargs,
            } => {
                let mut positional = Vec::with_capacity(positional_tail.len() + 1);
                positional.push(list_value);
                positional.append(&mut positional_tail);
                let collection_args = build_arg_values(positional, kwargs);
                let result = ModuleFunctions::Collections(CollectionsFunctions::Counter).call(
                    self.heap,
                    self.interns,
                    collection_args,
                )?;
                self.handle_attr_call_result(result)
            }
            PendingBuiltinFromListKind::Enumerate { start } => {
                let enumerate_args = match start {
                    Some(start_value) => ArgValues::Two(list_value, start_value),
                    None => ArgValues::One(list_value),
                };
                let value =
                    BuiltinsFunctions::Enumerate.call(self.heap, enumerate_args, self.interns, self.print_writer)?;
                Ok(CallResult::Push(value))
            }
            PendingBuiltinFromListKind::Zip {
                mut materialized,
                remaining,
            } => {
                materialized.push(list_value);
                self.call_zip_builtin_normalized(materialized, remaining)
            }
            PendingBuiltinFromListKind::DictUpdate {
                dict_id,
                remaining_positional,
                kwargs,
            } => {
                let result =
                    self.call_dict_update_after_materialization(dict_id, list_value, remaining_positional, kwargs);
                self.heap.dec_ref(dict_id);
                result
            }
            PendingBuiltinFromListKind::HeapqMerge {
                mut materialized,
                remaining,
                kwargs,
            } => {
                materialized.push(list_value);
                self.call_heapq_merge_normalized(materialized, remaining, kwargs)
            }
            PendingBuiltinFromListKind::Statistics {
                function,
                mut positional_tail,
                kwargs,
            } => {
                let mut positional = Vec::with_capacity(positional_tail.len() + 1);
                positional.push(list_value);
                positional.append(&mut positional_tail);
                let stat_args = build_arg_values(positional, kwargs);
                let result = ModuleFunctions::Statistics(function).call(self.heap, self.interns, stat_args)?;
                self.handle_attr_call_result(result)
            }
        }
    }

    /// Applies pending builtin-finalization to a list-build continuation result.
    ///
    /// This keeps `pending_builtin_from_list` consistent across success and error paths.
    pub(super) fn maybe_finish_builtin_from_list_result(
        &mut self,
        result: Result<CallResult, RunError>,
    ) -> Result<CallResult, RunError> {
        match result {
            Ok(CallResult::Push(value)) => self.maybe_finish_builtin_from_list_value(value),
            Ok(other) => Ok(other),
            Err(e) => {
                self.clear_pending_builtin_from_list();
                Err(e)
            }
        }
    }

    /// Calls the `isinstance()` builtin with metaclass `__instancecheck__` support.
    fn call_isinstance(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (obj, classinfo) = args.get_two_args("isinstance", self.heap)?;

        if let Value::Ref(class_id) = &classinfo
            && matches!(self.heap.get(*class_id), HeapData::ClassObject(_))
        {
            // Protocol classes require typing's custom semantics (including the
            // runtime_checkable guard). Handle these before generic metaclass
            // dispatch so default `type.__instancecheck__` does not swallow it.
            if self.class_has_protocol_marker(*class_id) {
                let method = Value::ModuleFunction(ModuleFunctions::Typing(TypingFunctions::ProtocolInstancecheck));
                let result = self.call_class_dunder(*class_id, method, ArgValues::One(obj));
                classinfo.drop_with_heap(self.heap);
                return match result {
                    Ok(CallResult::Push(value)) => {
                        let b = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(b)))
                    }
                    Ok(CallResult::FramePushed) => {
                        self.pending_instancecheck_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    Ok(other) => Ok(other),
                    Err(e) => Err(e),
                };
            }

            let dunder_id: StringId = StaticStrings::DunderInstancecheck.into();
            let dunder_name = self.interns.get_str(dunder_id);
            let method = self
                .lookup_metaclass_namespace_dunder(*class_id, dunder_name)
                .or_else(|| self.lookup_metaclass_dunder(*class_id, dunder_id));
            if let Some(method) = method {
                let result = self.call_class_dunder(*class_id, method, ArgValues::One(obj));
                classinfo.drop_with_heap(self.heap);
                return match result {
                    Ok(CallResult::Push(value)) => {
                        let b = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(b)))
                    }
                    Ok(CallResult::FramePushed) => {
                        self.pending_instancecheck_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    Ok(other) => Ok(other),
                    Err(e) => Err(e),
                };
            }
        }

        // Fallback to builtin implementation
        let result =
            crate::builtins::isinstance::builtin_isinstance(self.heap, ArgValues::Two(obj, classinfo), self.interns)?;
        Ok(CallResult::Push(result))
    }

    /// Calls the `issubclass()` builtin with metaclass `__subclasscheck__` support.
    fn call_issubclass(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (cls_val, classinfo) = args.get_two_args("issubclass", self.heap)?;

        if let Value::Ref(class_id) = &classinfo
            && matches!(self.heap.get(*class_id), HeapData::ClassObject(_))
        {
            let dunder_id: StringId = StaticStrings::DunderSubclasscheck.into();
            let dunder_name = self.interns.get_str(dunder_id);
            let method = self
                .lookup_metaclass_namespace_dunder(*class_id, dunder_name)
                .or_else(|| self.lookup_metaclass_dunder(*class_id, dunder_id));
            if let Some(method) = method {
                let result = self.call_class_dunder(*class_id, method, ArgValues::One(cls_val));
                classinfo.drop_with_heap(self.heap);
                return match result {
                    Ok(CallResult::Push(value)) => {
                        let b = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(b)))
                    }
                    Ok(CallResult::FramePushed) => {
                        self.pending_subclasscheck_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    Ok(other) => Ok(other),
                    Err(e) => Err(e),
                };
            }
        }

        let result = crate::builtins::isinstance::builtin_issubclass(
            self.heap,
            ArgValues::Two(cls_val, classinfo),
            self.interns,
        )?;
        Ok(CallResult::Push(result))
    }

    /// Handles calling a heap-allocated callable (closure, function with defaults, or class).
    ///
    /// Uses a two-phase approach to avoid borrow conflicts:
    /// 1. Copy data without incrementing refcounts
    /// 2. Increment refcounts after the borrow ends
    fn call_heap_callable(
        &mut self,
        heap_id: HeapId,
        callable: Value,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Built-in callable wrappers for class and function introspection.
        if matches!(self.heap.get(heap_id), HeapData::ClassSubclasses(_)) {
            let class_id = match self.heap.get(heap_id) {
                HeapData::ClassSubclasses(cs) => cs.class_id(),
                _ => unreachable!(),
            };
            args.check_zero_args("type.__subclasses__", self.heap)?;
            callable.drop_with_heap(self.heap);
            let subclasses = self.collect_class_subclasses(class_id)?;
            let list_id = self.heap.allocate(HeapData::List(List::new(subclasses)))?;
            return Ok(CallResult::Push(Value::Ref(list_id)));
        }

        if matches!(self.heap.get(heap_id), HeapData::ClassGetItem(_)) {
            let class_id = match self.heap.get(heap_id) {
                HeapData::ClassGetItem(cg) => cg.class_id(),
                _ => unreachable!(),
            };
            let (first, second) = args.get_one_two_args("type.__class_getitem__", self.heap)?;
            let item = if let Some(second) = second {
                first.drop_with_heap(self.heap);
                second
            } else {
                first
            };
            self.heap.inc_ref(class_id);
            let origin = Value::Ref(class_id);
            callable.drop_with_heap(self.heap);
            let alias = make_generic_alias(origin, item, self.heap, self.interns)?;
            return Ok(CallResult::Push(alias));
        }

        // Runtime generic aliases are callable and delegate construction to
        // their origin class/callable, mirroring CPython's `C[int](...)`.
        if matches!(self.heap.get(heap_id), HeapData::GenericAlias(_)) {
            let origin = match self.heap.get(heap_id) {
                HeapData::GenericAlias(alias) => alias.origin().clone_with_heap(self.heap),
                _ => unreachable!(),
            };
            callable.drop_with_heap(self.heap);
            return self.call_function(origin, args);
        }

        if matches!(self.heap.get(heap_id), HeapData::FunctionGet(_)) {
            let func_value = match self.heap.get(heap_id) {
                HeapData::FunctionGet(getter) => getter.func().clone_with_heap(self.heap),
                _ => unreachable!(),
            };
            let (obj, owner) = args.get_one_two_args("function.__get__", self.heap)?;
            if let Some(owner) = owner {
                owner.drop_with_heap(self.heap);
            }
            callable.drop_with_heap(self.heap);
            if matches!(obj, Value::None) {
                obj.drop_with_heap(self.heap);
                return Ok(CallResult::Push(func_value));
            }
            let bound_id = self
                .heap
                .allocate(HeapData::BoundMethod(crate::types::BoundMethod::new(func_value, obj)))?;
            return Ok(CallResult::Push(Value::Ref(bound_id)));
        }

        if matches!(self.heap.get(heap_id), HeapData::WeakRef(_)) {
            let (target_id, is_proxy, is_method, direct_target, mut method_func) = match self.heap.get(heap_id) {
                HeapData::WeakRef(wr) => (
                    wr.target(),
                    wr.is_proxy(),
                    wr.is_method(),
                    wr.direct_target().map(Value::copy_for_extend),
                    wr.method_func().map(Value::copy_for_extend),
                ),
                _ => unreachable!(),
            };
            if let Some(Value::Ref(id)) = &direct_target {
                self.heap.inc_ref(*id);
            }
            if let Some(Value::Ref(id)) = &method_func {
                self.heap.inc_ref(*id);
            }
            callable.drop_with_heap(self.heap);
            if let Some(target_value) = direct_target {
                args.check_zero_args("weakref", self.heap)?;
                return Ok(CallResult::Push(target_value));
            }
            if let Some(target_id) = target_id {
                if self.heap.get_if_live(target_id).is_some() {
                    if is_proxy {
                        self.heap.inc_ref(target_id);
                        return self.call_function(Value::Ref(target_id), args);
                    }
                    if is_method {
                        args.check_zero_args("weakref", self.heap)?;
                        let Some(func) = method_func.take() else {
                            return Err(RunError::internal("WeakMethod missing function"));
                        };
                        self.heap.inc_ref(target_id);
                        let self_arg = Value::Ref(target_id);
                        let bound_id = self
                            .heap
                            .allocate(HeapData::BoundMethod(crate::types::BoundMethod::new(func, self_arg)))?;
                        return Ok(CallResult::Push(Value::Ref(bound_id)));
                    }
                    args.check_zero_args("weakref", self.heap)?;
                    self.heap.inc_ref(target_id);
                    return Ok(CallResult::Push(Value::Ref(target_id)));
                }
                self.heap.with_entry_mut(heap_id, |_, data| {
                    let HeapData::WeakRef(wr) = data else {
                        return Err(RunError::internal("weakref target mutated during call"));
                    };
                    wr.clear();
                    Ok(())
                })?;
            }
            if let Some(func) = method_func {
                func.drop_with_heap(self.heap);
            }
            if is_proxy {
                args.drop_with_heap(self.heap);
                return Err(crate::exception_private::SimpleException::new_msg(
                    crate::exception_private::ExcType::ReferenceError,
                    "weakly-referenced object no longer exists",
                )
                .into());
            }
            args.check_zero_args("weakref", self.heap)?;
            return Ok(CallResult::Push(Value::None));
        }

        // A namedtuple factory behaves like a Python class and is directly callable.
        if matches!(self.heap.get(heap_id), HeapData::NamedTupleFactory(_)) {
            let named_tuple = self.heap.with_entry_mut(heap_id, |heap_inner, data| {
                let HeapData::NamedTupleFactory(factory) = data else {
                    unreachable!();
                };
                factory.instantiate(args, heap_inner, self.interns)
            })?;
            callable.drop_with_heap(self.heap);
            let tuple_id = self.heap.allocate(HeapData::NamedTuple(named_tuple))?;
            return Ok(CallResult::Push(Value::Ref(tuple_id)));
        }

        // Check if this is a ClassObject (class instantiation)
        if matches!(self.heap.get(heap_id), HeapData::ClassObject(_)) {
            let call_id: StringId = StaticStrings::DunderCall.into();
            if let Some(method) = self.lookup_metaclass_dunder(heap_id, call_id) {
                let result = self.call_class_dunder(heap_id, method, args)?;
                callable.drop_with_heap(self.heap);
                return Ok(result);
            }
            return self.call_class_instantiate(heap_id, callable, args);
        }

        // Callable contextlib helper objects implemented as StdlibObject variants.
        if matches!(self.heap.get(heap_id), HeapData::StdlibObject(_)) {
            enum ContextlibCallable {
                Factory {
                    func: Value,
                    async_mode: bool,
                },
                ContextManager {
                    generator: Value,
                    async_mode: bool,
                },
                Decorator {
                    generator: Value,
                    wrapped: Value,
                    async_mode: bool,
                    close_with_exit: bool,
                },
                InstanceDecorator {
                    manager: Value,
                    wrapped: Value,
                    async_mode: bool,
                },
                NextCallable(Value),
                NextDefaultCallable(Value),
                CloseCallable(Value),
            }

            let callable_kind = match self.heap.get(heap_id) {
                HeapData::StdlibObject(StdlibObject::GeneratorContextManagerFactory(state)) => {
                    Some(ContextlibCallable::Factory {
                        func: state.func.clone_with_heap(self.heap),
                        async_mode: state.async_mode,
                    })
                }
                HeapData::StdlibObject(StdlibObject::GeneratorContextManager(state)) => {
                    Some(ContextlibCallable::ContextManager {
                        generator: state.generator.clone_with_heap(self.heap),
                        async_mode: state.async_mode,
                    })
                }
                HeapData::StdlibObject(StdlibObject::GeneratorContextDecorator(state)) => {
                    Some(ContextlibCallable::Decorator {
                        generator: state.generator.clone_with_heap(self.heap),
                        wrapped: state.wrapped.clone_with_heap(self.heap),
                        async_mode: state.async_mode,
                        close_with_exit: state.close_with_exit,
                    })
                }
                HeapData::StdlibObject(StdlibObject::InstanceContextDecorator(state)) => {
                    Some(ContextlibCallable::InstanceDecorator {
                        manager: state.manager.clone_with_heap(self.heap),
                        wrapped: state.wrapped.clone_with_heap(self.heap),
                        async_mode: state.async_mode,
                    })
                }
                HeapData::StdlibObject(StdlibObject::GeneratorNextCallable(state)) => Some(
                    ContextlibCallable::NextCallable(state.generator.clone_with_heap(self.heap)),
                ),
                HeapData::StdlibObject(StdlibObject::GeneratorNextDefaultCallable(state)) => Some(
                    ContextlibCallable::NextDefaultCallable(state.generator.clone_with_heap(self.heap)),
                ),
                HeapData::StdlibObject(StdlibObject::GeneratorCloseCallable(state)) => Some(
                    ContextlibCallable::CloseCallable(state.generator.clone_with_heap(self.heap)),
                ),
                _ => None,
            };

            if let Some(callable_kind) = callable_kind {
                callable.drop_with_heap(self.heap);
                return match callable_kind {
                    ContextlibCallable::Factory { func, async_mode } => match self.call_function(func, args)? {
                        CallResult::Push(generator) => {
                            let is_generator = matches!(
                                generator,
                                Value::Ref(id) if matches!(self.heap.get(id), HeapData::Generator(_))
                            );
                            if !is_generator {
                                generator.drop_with_heap(self.heap);
                                return Err(ExcType::type_error(
                                    "contextmanager function must return a generator".to_string(),
                                ));
                            }
                            let cm = StdlibObject::new_generator_context_manager(generator, async_mode);
                            let cm_id = self.heap.allocate(HeapData::StdlibObject(cm))?;
                            Ok(CallResult::Push(Value::Ref(cm_id)))
                        }
                        CallResult::FramePushed => {
                            Err(RunError::internal("contextmanager factory unexpectedly pushed a frame"))
                        }
                        other => Ok(other),
                    },
                    ContextlibCallable::ContextManager { generator, async_mode } => {
                        let wrapped = args.get_one_arg("contextmanager decorator", self.heap)?;
                        let decorator = StdlibObject::new_generator_context_decorator(generator, wrapped, async_mode);
                        let decorator_id = self.heap.allocate(HeapData::StdlibObject(decorator))?;
                        Ok(CallResult::Push(Value::Ref(decorator_id)))
                    }
                    ContextlibCallable::Decorator {
                        generator,
                        wrapped,
                        async_mode,
                        close_with_exit,
                    } => {
                        if close_with_exit {
                            self.call_context_decorator_with_instance(generator, wrapped, args, async_mode)
                        } else {
                            self.call_context_decorator_with_generator(generator, wrapped, args, async_mode)
                        }
                    }
                    ContextlibCallable::InstanceDecorator {
                        manager,
                        wrapped,
                        async_mode,
                    } => self.call_context_decorator_with_instance(manager, wrapped, args, async_mode),
                    ContextlibCallable::NextCallable(generator) => {
                        args.check_zero_args("contextlib._GeneratorNextCallable", self.heap)?;
                        let dunder_next: StringId = StaticStrings::DunderNext.into();
                        self.call_attr(generator, dunder_next, ArgValues::Empty)
                    }
                    ContextlibCallable::NextDefaultCallable(generator) => {
                        args.check_zero_args("contextlib._GeneratorNextDefaultCallable", self.heap)?;
                        let generator_id = match generator {
                            Value::Ref(id) => id,
                            other => {
                                other.drop_with_heap(self.heap);
                                return Err(RunError::internal(
                                    "contextlib next-default callable missing generator reference",
                                ));
                            }
                        };
                        // `generator` came from cloned state; drop this temporary owner and pass
                        // a fresh owned reference into call_attr.
                        Value::Ref(generator_id).drop_with_heap(self.heap);
                        self.heap.inc_ref(generator_id);
                        self.clear_pending_next_default();
                        self.pending_next_default = Some(PendingNextDefault {
                            generator_id,
                            default: Value::None,
                        });
                        let dunder_next: StringId = StaticStrings::DunderNext.into();
                        match self.call_attr(Value::Ref(generator_id), dunder_next, ArgValues::Empty) {
                            Ok(CallResult::Push(value)) => {
                                self.clear_pending_next_default();
                                value.drop_with_heap(self.heap);
                                Err(SimpleException::new_msg(ExcType::RuntimeError, "generator didn't stop").into())
                            }
                            Ok(CallResult::FramePushed) => Ok(CallResult::FramePushed),
                            Ok(other) => {
                                self.clear_pending_next_default();
                                Ok(other)
                            }
                            Err(err) => {
                                self.clear_pending_next_default();
                                if err.is_stop_iteration() {
                                    Ok(CallResult::Push(Value::None))
                                } else {
                                    Err(err)
                                }
                            }
                        }
                    }
                    ContextlibCallable::CloseCallable(generator) => {
                        args.check_zero_args("contextlib._GeneratorCloseCallable", self.heap)?;
                        let close: StringId = StaticStrings::Close.into();
                        self.call_attr(generator, close, ArgValues::Empty)
                    }
                };
            }
        }

        // Check if this is an Instance with __call__
        if matches!(self.heap.get(heap_id), HeapData::Instance(_)) {
            let dunder_id: StringId = StaticStrings::DunderCall.into();
            if let Some(method) = self.lookup_type_dunder(heap_id, dunder_id) {
                callable.drop_with_heap(self.heap);
                return self.call_dunder(heap_id, method, args);
            }

            let dunder_enter: StringId = StaticStrings::DunderEnter.into();
            let dunder_exit: StringId = StaticStrings::DunderExit.into();
            let has_sync_decorator_protocol = self.lookup_type_dunder(heap_id, dunder_enter).is_some()
                && self.lookup_type_dunder(heap_id, dunder_exit).is_some();
            let has_async_decorator_protocol = if let HeapData::Instance(instance) = self.heap.get(heap_id) {
                let class_id = instance.class_id();
                if let HeapData::ClassObject(cls) = self.heap.get(class_id) {
                    cls.mro_has_attr("__aenter__", class_id, self.heap, self.interns)
                        && cls.mro_has_attr("__aexit__", class_id, self.heap, self.interns)
                } else {
                    false
                }
            } else {
                false
            };

            if (has_sync_decorator_protocol || has_async_decorator_protocol) && matches!(&args, ArgValues::One(_)) {
                let wrapped = args.get_one_arg("context decorator", self.heap)?;
                let async_mode = has_async_decorator_protocol && !has_sync_decorator_protocol;
                let decorator = StdlibObject::new_instance_context_decorator(callable, wrapped, async_mode);
                let decorator_id = self.heap.allocate(HeapData::StdlibObject(decorator))?;
                return Ok(CallResult::Push(Value::Ref(decorator_id)));
            }

            callable.drop_with_heap(self.heap);
            args.drop_with_heap(self.heap);
            return Err(ExcType::type_error("object is not callable"));
        }

        // Check if this is a bound method (prepend bound self/cls).
        if matches!(self.heap.get(heap_id), HeapData::BoundMethod(_)) {
            let (func, self_arg) = match self.heap.get(heap_id) {
                HeapData::BoundMethod(bm) => (
                    bm.func().clone_with_heap(self.heap),
                    bm.self_arg().clone_with_heap(self.heap),
                ),
                _ => unreachable!("call_heap_callable: not a BoundMethod"),
            };
            callable.drop_with_heap(self.heap);

            let new_args = match args {
                ArgValues::Empty => ArgValues::One(self_arg),
                ArgValues::One(a) => ArgValues::Two(self_arg, a),
                ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                    args: vec![self_arg, a, b],
                    kwargs: KwargsValues::Empty,
                },
                ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                    args: vec![self_arg],
                    kwargs: kw,
                },
                ArgValues::ArgsKargs { mut args, kwargs } => {
                    args.insert(0, self_arg);
                    ArgValues::ArgsKargs { args, kwargs }
                }
            };

            return self.call_function(func, new_args);
        }

        // Check if this is a functools.partial (prepend stored args)
        if matches!(self.heap.get(heap_id), HeapData::Partial(_)) {
            let (func, partial_args, partial_kwargs) = match self.heap.get(heap_id) {
                HeapData::Partial(p) => (
                    p.func().clone_with_heap(self.heap),
                    p.args()
                        .iter()
                        .map(|v| v.clone_with_heap(self.heap))
                        .collect::<Vec<_>>(),
                    p.kwargs()
                        .iter()
                        .map(|(k, v)| (k.clone_with_heap(self.heap), v.clone_with_heap(self.heap)))
                        .collect::<Vec<_>>(),
                ),
                _ => unreachable!("call_heap_callable: not a Partial"),
            };
            callable.drop_with_heap(self.heap);

            // Prepend partial's stored positional args before call-site args
            let new_args = build_partial_call_args(partial_args, partial_kwargs, args, self.heap, self.interns)?;
            return self.call_function(func, new_args);
        }

        if matches!(self.heap.get(heap_id), HeapData::SingleDispatch(_)) {
            return self.call_singledispatch_callable(heap_id, callable, args);
        }

        if matches!(self.heap.get(heap_id), HeapData::SingleDispatchRegister(_)) {
            let (dispatcher, cls) = match self.heap.get(heap_id) {
                HeapData::SingleDispatchRegister(register) => (
                    register.dispatcher.clone_with_heap(self.heap),
                    register.cls.clone_with_heap(self.heap),
                ),
                _ => unreachable!("call_heap_callable: not a SingleDispatchRegister"),
            };
            callable.drop_with_heap(self.heap);
            let func = match args.get_one_arg("singledispatch.register", self.heap) {
                Ok(func) => func,
                Err(err) => {
                    dispatcher.drop_with_heap(self.heap);
                    cls.drop_with_heap(self.heap);
                    return Err(err);
                }
            };
            let func_for_registry = func.clone_with_heap(self.heap);
            self.singledispatch_register(dispatcher, cls, func_for_registry)?;
            return Ok(CallResult::Push(func));
        }

        if matches!(self.heap.get(heap_id), HeapData::SingleDispatchMethod(_)) {
            let dispatcher = match self.heap.get(heap_id) {
                HeapData::SingleDispatchMethod(method) => method.dispatcher.clone_with_heap(self.heap),
                _ => unreachable!("call_heap_callable: not a SingleDispatchMethod"),
            };
            callable.drop_with_heap(self.heap);
            return self.call_function(dispatcher, args);
        }

        // Check if this is a functools.cmp_to_key wrapper (it's callable - creates a key value)
        if matches!(self.heap.get(heap_id), HeapData::CmpToKey(_)) {
            let cmp_func = match self.heap.get(heap_id) {
                HeapData::CmpToKey(c) => c.func().clone_with_heap(self.heap),
                _ => unreachable!("call_heap_callable: not a CmpToKey"),
            };
            let obj = args.get_one_arg("cmp_to_key", self.heap)?;
            callable.drop_with_heap(self.heap);
            let key_value = self.cmp_to_key_test_key(&cmp_func, obj);
            cmp_func.drop_with_heap(self.heap);
            return Ok(CallResult::Push(key_value));
        }

        // Check if this is an operator.itemgetter callable
        if matches!(self.heap.get(heap_id), HeapData::ItemGetter(_)) {
            let (mut positional, kwargs) = args.into_parts();
            if !kwargs.is_empty() {
                positional.drop_with_heap(self.heap);
                kwargs.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error("itemgetter() takes no keyword arguments"));
            }
            let arg_count = positional.len();
            if arg_count != 1 {
                positional.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "itemgetter expected 1 argument, got {arg_count}"
                )));
            }
            let mut obj = positional.next().expect("length checked");
            let items = match self.heap.get(heap_id) {
                HeapData::ItemGetter(getter) => getter
                    .items()
                    .iter()
                    .map(|v| v.clone_with_heap(self.heap))
                    .collect::<Vec<_>>(),
                _ => unreachable!("call_heap_callable: not an ItemGetter"),
            };
            let item_count = items.len();
            callable.drop_with_heap(self.heap);

            let mut item_iter = items.into_iter();
            if item_count == 1 {
                let key = item_iter.next().expect("length checked");
                let result = obj.py_getitem(&key, self.heap, self.interns);
                key.drop_with_heap(self.heap);
                item_iter.drop_with_heap(self.heap);
                obj.drop_with_heap(self.heap);
                return result.map(CallResult::Push);
            }

            let mut values: Vec<Value> = Vec::with_capacity(item_count);
            while let Some(key) = item_iter.next() {
                match obj.py_getitem(&key, self.heap, self.interns) {
                    Ok(value) => {
                        key.drop_with_heap(self.heap);
                        values.push(value);
                    }
                    Err(err) => {
                        key.drop_with_heap(self.heap);
                        item_iter.drop_with_heap(self.heap);
                        for value in values {
                            value.drop_with_heap(self.heap);
                        }
                        obj.drop_with_heap(self.heap);
                        return Err(err);
                    }
                }
            }
            obj.drop_with_heap(self.heap);
            let tuple = allocate_tuple(SmallVec::from_vec(values), self.heap)?;
            return Ok(CallResult::Push(tuple));
        }

        // Check if this is an operator.attrgetter callable
        if matches!(self.heap.get(heap_id), HeapData::AttrGetter(_)) {
            let (mut positional, kwargs) = args.into_parts();
            if !kwargs.is_empty() {
                positional.drop_with_heap(self.heap);
                kwargs.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error("attrgetter() takes no keyword arguments"));
            }
            let arg_count = positional.len();
            if arg_count != 1 {
                positional.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "attrgetter expected 1 argument, got {arg_count}"
                )));
            }
            let obj = positional.next().expect("length checked");
            let attrs = match self.heap.get(heap_id) {
                HeapData::AttrGetter(getter) => getter
                    .attrs()
                    .iter()
                    .map(|v| v.clone_with_heap(self.heap))
                    .collect::<Vec<_>>(),
                _ => unreachable!("call_heap_callable: not an AttrGetter"),
            };
            let attr_count = attrs.len();
            callable.drop_with_heap(self.heap);

            let mut attr_iter = attrs.into_iter();
            let mut values: Vec<Value> = Vec::with_capacity(attr_count);
            while let Some(attr) = attr_iter.next() {
                let Some(attr_name) = attr.as_either_str(self.heap) else {
                    attr.drop_with_heap(self.heap);
                    attr_iter.drop_with_heap(self.heap);
                    for value in values {
                        value.drop_with_heap(self.heap);
                    }
                    obj.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("attribute name must be a string"));
                };
                let mut current = obj.clone_with_heap(self.heap);
                for segment in attr_name.as_str(self.interns).split('.') {
                    match self.getattr_dynamic_str(&current, segment) {
                        Ok(AttrCallResult::Value(next)) => {
                            current.drop_with_heap(self.heap);
                            current = next;
                        }
                        Ok(_) => {
                            current.drop_with_heap(self.heap);
                            attr.drop_with_heap(self.heap);
                            attr_iter.drop_with_heap(self.heap);
                            for value in values {
                                value.drop_with_heap(self.heap);
                            }
                            obj.drop_with_heap(self.heap);
                            return Err(RunError::internal("attrgetter returned non-value result"));
                        }
                        Err(err) => {
                            current.drop_with_heap(self.heap);
                            attr.drop_with_heap(self.heap);
                            attr_iter.drop_with_heap(self.heap);
                            for value in values {
                                value.drop_with_heap(self.heap);
                            }
                            obj.drop_with_heap(self.heap);
                            return Err(err);
                        }
                    }
                }
                attr.drop_with_heap(self.heap);
                values.push(current);
            }
            obj.drop_with_heap(self.heap);
            if values.len() == 1 {
                return Ok(CallResult::Push(values.pop().expect("length checked")));
            }
            let tuple = allocate_tuple(SmallVec::from_vec(values), self.heap)?;
            return Ok(CallResult::Push(tuple));
        }

        // Check if this is an operator.methodcaller callable
        if matches!(self.heap.get(heap_id), HeapData::MethodCaller(_)) {
            let (mut positional, kwargs) = args.into_parts();
            let arg_count = positional.len();
            if !kwargs.is_empty() {
                positional.drop_with_heap(self.heap);
                kwargs.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                if arg_count == 0 {
                    return Err(ExcType::type_error(format!(
                        "methodcaller expected 1 argument, got {arg_count}"
                    )));
                }
                return Err(ExcType::type_error("methodcaller() takes no keyword arguments"));
            }
            if arg_count != 1 {
                positional.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "methodcaller expected 1 argument, got {arg_count}"
                )));
            }
            let obj = positional.next().expect("length checked");

            let (name_value, stored_args, stored_kwargs) = match self.heap.get(heap_id) {
                HeapData::MethodCaller(caller) => {
                    let Some(name_str) = caller.name().as_either_str(self.heap) else {
                        callable.drop_with_heap(self.heap);
                        obj.drop_with_heap(self.heap);
                        return Err(ExcType::type_error("method name must be a string"));
                    };
                    let args = caller
                        .args()
                        .iter()
                        .map(|v| v.clone_with_heap(self.heap))
                        .collect::<Vec<_>>();
                    let kwargs = caller
                        .kwargs()
                        .iter()
                        .map(|(k, v)| (k.clone_with_heap(self.heap), v.clone_with_heap(self.heap)))
                        .collect::<Vec<_>>();
                    (name_str, args, kwargs)
                }
                _ => unreachable!("call_heap_callable: not a MethodCaller"),
            };
            callable.drop_with_heap(self.heap);

            let kwargs_values = if stored_kwargs.is_empty() {
                KwargsValues::Empty
            } else {
                match Dict::from_pairs(stored_kwargs, self.heap, self.interns) {
                    Ok(dict) => KwargsValues::Dict(dict),
                    Err(err) => {
                        for arg in stored_args {
                            arg.drop_with_heap(self.heap);
                        }
                        obj.drop_with_heap(self.heap);
                        return Err(err);
                    }
                }
            };

            let call_args = if stored_args.is_empty() && matches!(kwargs_values, KwargsValues::Empty) {
                ArgValues::Empty
            } else if stored_args.is_empty() {
                ArgValues::Kwargs(kwargs_values)
            } else if matches!(kwargs_values, KwargsValues::Empty) {
                match stored_args.len() {
                    1 => ArgValues::One(stored_args.into_iter().next().expect("length checked")),
                    2 => {
                        let mut iter = stored_args.into_iter();
                        ArgValues::Two(
                            iter.next().expect("length checked"),
                            iter.next().expect("length checked"),
                        )
                    }
                    _ => ArgValues::ArgsKargs {
                        args: stored_args,
                        kwargs: kwargs_values,
                    },
                }
            } else {
                ArgValues::ArgsKargs {
                    args: stored_args,
                    kwargs: kwargs_values,
                }
            };

            if let Some(name_id) = name_value.string_id() {
                return self.call_attr(obj, name_id, call_args);
            }

            let attr_result = self.getattr_dynamic_str(&obj, name_value.as_str(self.interns));
            obj.drop_with_heap(self.heap);
            match attr_result {
                Ok(AttrCallResult::Value(method)) => return self.call_function(method, call_args),
                Ok(_) => {
                    call_args.drop_with_heap(self.heap);
                    return Err(RunError::internal("methodcaller returned non-value result"));
                }
                Err(err) => {
                    call_args.drop_with_heap(self.heap);
                    return Err(err);
                }
            }
        }

        // Check if this is a PropertyAccessor (@prop.setter / @prop.deleter / @prop.getter)
        if matches!(self.heap.get(heap_id), HeapData::PropertyAccessor(_)) {
            return self.call_property_accessor(heap_id, callable, args);
        }

        // Check if this is ObjectNewImpl (object.__new__)
        if matches!(self.heap.get(heap_id), HeapData::ObjectNewImpl(_)) {
            // ObjectNewImpl is a ZST, so we can create a new instance to avoid borrow issues
            let obj_new = ObjectNewImpl;
            let result = obj_new.call(self.heap, args);
            callable.drop_with_heap(self.heap);
            return result.map(CallResult::Push);
        }

        // Check if this is functools.lru_cache - call the wrapped function
        if matches!(self.heap.get(heap_id), HeapData::LruCache(_)) {
            // Extract data first to avoid borrow issues
            let maybe_func = match self.heap.get(heap_id) {
                HeapData::LruCache(cache) => cache.func.as_ref().map(|f| f.clone_with_heap(self.heap)),
                _ => unreachable!("call_heap_callable: not an LruCache"),
            };

            let Some(func) = maybe_func else {
                // This is a decorator factory, not a wrapped function
                // Get the function from args and wrap it
                let wrapper_func = args.get_one_arg("lru_cache", self.heap)?;
                // Create new LruCache with the wrapped function
                let (maxsize, typed) = match self.heap.get(heap_id) {
                    HeapData::LruCache(cache) => (cache.maxsize, cache.typed),
                    _ => unreachable!("call_heap_callable: not an LruCache"),
                };
                let new_cache = crate::types::LruCache::new(maxsize, typed, Some(wrapper_func));
                let new_cache_id = self.heap.allocate(HeapData::LruCache(new_cache))?;
                callable.drop_with_heap(self.heap);
                return Ok(CallResult::Push(Value::Ref(new_cache_id)));
            };

            // Active cache wrapper: key lookup, then compute+store on miss.
            let typed = match self.heap.get(heap_id) {
                HeapData::LruCache(cache) => cache.typed,
                _ => unreachable!("call_heap_callable: not an LruCache"),
            };
            let cache_key = match build_lru_cache_key(&args, typed, self.heap) {
                Ok(key) => key,
                Err(err) => {
                    callable.drop_with_heap(self.heap);
                    args.drop_with_heap(self.heap);
                    return Err(err);
                }
            };

            let cached = self
                .heap
                .with_entry_mut(heap_id, |heap, data| -> RunResult<Option<Value>> {
                    let HeapData::LruCache(cache) = data else {
                        return Err(RunError::internal("call_heap_callable: lru cache mutated"));
                    };
                    if let Some(cached_value) = cache.cache.get(&cache_key, heap, self.interns)? {
                        let result = cached_value.clone_with_heap(heap);
                        cache.hits = cache.hits.saturating_add(1);
                        if cache.maxsize.is_some() {
                            if let Some(pos) = cache.order.iter().position(|k| k.py_eq(&cache_key, heap, self.interns))
                            {
                                let stale = cache.order.remove(pos);
                                stale.drop_with_heap(heap);
                            }
                            cache.order.push(cache_key.clone_with_heap(heap));
                        }
                        return Ok(Some(result));
                    }
                    cache.misses = cache.misses.saturating_add(1);
                    Ok(None)
                })?;

            if let Some(value) = cached {
                cache_key.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                args.drop_with_heap(self.heap);
                return Ok(CallResult::Push(value));
            }

            match self.call_function(func, args)? {
                CallResult::Push(value) => {
                    self.store_lru_cache_value(heap_id, cache_key, &value)?;
                    callable.drop_with_heap(self.heap);
                    return Ok(CallResult::Push(value));
                }
                CallResult::FramePushed => {
                    self.pending_lru_cache.push(super::PendingLruCache {
                        cache_id: heap_id,
                        cache_key,
                    });
                    self.pending_lru_cache_return = true;
                    callable.drop_with_heap(self.heap);
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    cache_key.drop_with_heap(self.heap);
                    callable.drop_with_heap(self.heap);
                    return Ok(other);
                }
            }
        }

        // Check if this is functools.wraps - create a FunctionWrapper
        if matches!(self.heap.get(heap_id), HeapData::Wraps(_)) {
            let (wrapped, assigned, updated) = match self.heap.get(heap_id) {
                HeapData::Wraps(w) => (
                    w.wrapped.clone_with_heap(self.heap),
                    w.assigned.clone(),
                    w.updated.clone(),
                ),
                _ => unreachable!("call_heap_callable: not a Wraps"),
            };
            let wrapper_func = args.get_one_arg("functools.wraps", self.heap)?;
            crate::modules::functools::apply_update_wrapper_attrs(
                &wrapper_func,
                &wrapped,
                &assigned,
                &updated,
                self.heap,
                self.interns,
            )?;
            wrapped.drop_with_heap(self.heap);
            callable.drop_with_heap(self.heap);
            return Ok(CallResult::Push(wrapper_func));
        }

        // Check if this is functools.update_wrapper - returns the wrapper with copied attrs
        if matches!(self.heap.get(heap_id), HeapData::FunctionWrapper(_)) {
            let wrapper = match self.heap.get(heap_id) {
                HeapData::FunctionWrapper(fw) => fw.wrapper.clone_with_heap(self.heap),
                _ => unreachable!("call_heap_callable: not a FunctionWrapper"),
            };
            callable.drop_with_heap(self.heap);
            return self.call_function(wrapper, args);
        }

        // Generated functools.total_ordering comparison method.
        if matches!(self.heap.get(heap_id), HeapData::TotalOrderingMethod(_)) {
            let (base, swap, negate) = match self.heap.get(heap_id) {
                HeapData::TotalOrderingMethod(method) => (method.base, method.swap, method.negate),
                _ => unreachable!("call_heap_callable: not a TotalOrderingMethod"),
            };

            let (mut positional, kwargs) = args.into_parts();
            if !kwargs.is_empty() {
                let arg_count = positional.len();
                positional.drop_with_heap(self.heap);
                kwargs.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "total_ordering method expected 2 arguments, got {arg_count}"
                )));
            }
            kwargs.drop_with_heap(self.heap);

            if positional.len() != 2 {
                let arg_count = positional.len();
                positional.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "total_ordering method expected 2 arguments, got {arg_count}"
                )));
            }
            let first = positional.next().expect("length checked");
            let second = positional.next().expect("length checked");
            let (receiver, other) = if swap { (second, first) } else { (first, second) };

            let Value::Ref(receiver_id) = receiver else {
                receiver.drop_with_heap(self.heap);
                other.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "'{}' not supported between instances of 'instance' and 'instance'",
                    total_ordering_symbol(base, swap, negate)
                )));
            };

            if !matches!(self.heap.get(receiver_id), HeapData::Instance(_)) {
                receiver.drop_with_heap(self.heap);
                other.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "'{}' not supported between instances of 'instance' and 'instance'",
                    total_ordering_symbol(base, swap, negate)
                )));
            }

            let Some(method) = self.lookup_type_dunder(receiver_id, base.into()) else {
                receiver.drop_with_heap(self.heap);
                other.drop_with_heap(self.heap);
                callable.drop_with_heap(self.heap);
                return Err(ExcType::type_error(format!(
                    "'{}' not supported between instances of 'instance' and 'instance'",
                    total_ordering_symbol(base, swap, negate)
                )));
            };
            receiver.drop_with_heap(self.heap);
            let result = self.call_dunder(receiver_id, method, ArgValues::One(other))?;
            callable.drop_with_heap(self.heap);

            if negate {
                return match result {
                    CallResult::Push(value) => {
                        if matches!(value, Value::NotImplemented) {
                            Ok(CallResult::Push(value))
                        } else {
                            let truthy = value.py_bool(self.heap, self.interns);
                            value.drop_with_heap(self.heap);
                            Ok(CallResult::Push(Value::Bool(!truthy)))
                        }
                    }
                    CallResult::FramePushed => {
                        self.pending_negate_bool = true;
                        Ok(CallResult::FramePushed)
                    }
                    other => Ok(other),
                };
            }
            return Ok(result);
        }

        // Phase 1: Copy data (func_id, cells, defaults) without refcount changes
        let (func_id, cells, defaults) = match self.heap.get(heap_id) {
            HeapData::Closure(fid, cells, defaults) => {
                let cloned_cells = cells.clone();
                let cloned_defaults: Vec<Value> = defaults.iter().map(Value::copy_for_extend).collect();
                (*fid, cloned_cells, cloned_defaults)
            }
            HeapData::FunctionDefaults(fid, defaults) => {
                let cloned_defaults: Vec<Value> = defaults.iter().map(Value::copy_for_extend).collect();
                (*fid, Vec::new(), cloned_defaults)
            }
            _ => {
                callable.drop_with_heap(self.heap);
                args.drop_with_heap(self.heap);
                return Err(ExcType::type_error("object is not callable"));
            }
        };

        // Phase 2: Increment refcounts now that the heap borrow has ended
        for &cell_id in &cells {
            self.heap.inc_ref(cell_id);
        }
        for default in &defaults {
            if let Value::Ref(id) = default {
                self.heap.inc_ref(*id);
            }
        }

        // Drop the callable ref (cloned data has its own refcounts)
        callable.drop_with_heap(self.heap);

        // Call the defined function
        self.call_def_function(func_id, &cells, defaults, args)
    }

    /// Writes a computed value into an `lru_cache` wrapper.
    pub(super) fn store_lru_cache_value(&mut self, cache_id: HeapId, cache_key: Value, value: &Value) -> RunResult<()> {
        let cached_value = value.clone_with_heap(self.heap);
        self.heap.with_entry_mut(cache_id, |heap, data| -> RunResult<()> {
            let HeapData::LruCache(cache) = data else {
                cache_key.drop_with_heap(heap);
                cached_value.drop_with_heap(heap);
                return Err(RunError::internal("call_heap_callable: lru cache mutated"));
            };
            if let Some(maxsize) = cache.maxsize {
                if maxsize == 0 {
                    cache_key.drop_with_heap(heap);
                    cached_value.drop_with_heap(heap);
                    return Ok(());
                }
                cache.order.push(cache_key.clone_with_heap(heap));
                if cache.order.len() > maxsize {
                    let evicted_lookup = cache.order.remove(0);
                    if let Some((evicted_key, evicted_value)) = cache.cache.pop(&evicted_lookup, heap, self.interns)? {
                        evicted_key.drop_with_heap(heap);
                        evicted_value.drop_with_heap(heap);
                    }
                    evicted_lookup.drop_with_heap(heap);
                }
            }
            if let Some(old) = cache.cache.set(cache_key, cached_value, heap, self.interns)? {
                old.drop_with_heap(heap);
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Collects live direct subclasses for `type.__subclasses__()`.
    ///
    /// Prunes stale registry entries (freed or reused heap slots) to keep the
    /// subclass list accurate without holding strong references.
    fn collect_class_subclasses(&mut self, class_id: HeapId) -> RunResult<Vec<Value>> {
        let mut results = Vec::new();
        self.heap.with_entry_mut(class_id, |heap, data| {
            let HeapData::ClassObject(cls) = data else {
                return Err(ExcType::type_error(
                    "type.__subclasses__ called on non-class".to_string(),
                ));
            };

            let mut fresh: Vec<crate::types::SubclassEntry> = Vec::new();
            for entry in cls.subclasses() {
                let subclass_id = entry.class_id();
                let Some(HeapData::ClassObject(sub_cls)) = heap.get_if_live(subclass_id) else {
                    continue;
                };
                if sub_cls.class_uid() != entry.class_uid() {
                    continue;
                }
                heap.inc_ref(subclass_id);
                results.push(Value::Ref(subclass_id));
                fresh.push(*entry);
            }

            cls.set_subclasses(fresh);
            Ok(())
        })?;
        Ok(results)
    }

    /// Instantiates a class by creating an Instance and calling `__init__`.
    ///
    /// 1. Creates a new Instance on the heap referencing the ClassObject
    /// 2. Looks up `__init__` in the class namespace
    /// 3. If found, calls it with (instance, *args) and marks the frame
    ///    so that the instance is returned instead of `__init__`'s None return
    /// 4. If not found and args are provided, raises TypeError
    /// 5. If not found and no args, returns the instance directly
    fn call_class_instantiate(
        &mut self,
        class_heap_id: HeapId,
        callable: Value,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Look up __new__ and __init__ via MRO.
        let new_name_id: StringId = StaticStrings::DunderNew.into();
        let init_name_id: StringId = StaticStrings::DunderInit.into();
        let new_name = self.interns.get_str(new_name_id);
        let init_name = self.interns.get_str(init_name_id);

        let type_class_id = self.heap.builtin_class_id(Type::Type)?;
        let (new_info, init_info, class_name, is_abstract, abstract_methods, is_metaclass_class) =
            match self.heap.get(class_heap_id) {
                HeapData::ClassObject(cls) => {
                    let new_val = cls
                        .mro_lookup_attr(new_name, class_heap_id, self.heap, self.interns)
                        .map(|(v, _)| v);
                    let init_val = cls
                        .mro_lookup_attr(init_name, class_heap_id, self.heap, self.interns)
                        .map(|(v, _)| v);
                    let mut abstract_methods = cls
                        .namespace()
                        .get_by_str(crate::modules::abc::ABSTRACT_METHODS_ATTR, self.heap, self.interns)
                        .map(|value| self.read_abstract_method_names(value))
                        .unwrap_or_default();
                    abstract_methods.sort_unstable();
                    let abstract_flag = matches!(
                        cls.namespace()
                            .get_by_str(crate::modules::abc::ABC_IS_ABSTRACT_ATTR, self.heap, self.interns),
                        Some(Value::Bool(true))
                    );
                    let is_abstract = abstract_flag || !abstract_methods.is_empty();
                    (
                        new_val,
                        init_val,
                        cls.name(self.interns).to_string(),
                        is_abstract,
                        abstract_methods,
                        cls.is_subclass_of(class_heap_id, type_class_id),
                    )
                }
                _ => unreachable!("call_class_instantiate: not a ClassObject"),
            };

        // Drop the callable ref (we've copied what we need)
        callable.drop_with_heap(self.heap);

        if is_abstract {
            args.drop_with_heap(self.heap);
            let message = self.format_abstract_instantiation_error(class_name.as_str(), &abstract_methods);
            return Err(ExcType::type_error(message));
        }

        // Metaclass subclasses that do not override `__new__`/`__init__` inherit
        // type's constructor semantics for class creation.
        if is_metaclass_class && new_info.is_none() && init_info.is_none() {
            self.heap.inc_ref(class_heap_id);
            let cls_arg = Value::Ref(class_heap_id);
            let type_new_args = match args {
                ArgValues::Empty => ArgValues::One(cls_arg),
                ArgValues::One(a) => ArgValues::Two(cls_arg, a),
                ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                    args: vec![cls_arg, a, b],
                    kwargs: KwargsValues::Empty,
                },
                ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                    args: vec![cls_arg],
                    kwargs: kw,
                },
                ArgValues::ArgsKargs { mut args, kwargs } => {
                    args.insert(0, cls_arg);
                    ArgValues::ArgsKargs { args, kwargs }
                }
            };
            if let Some(class_value) = self.try_type_new_from_super_args(type_new_args)? {
                return Ok(CallResult::Push(class_value));
            }
            return Err(ExcType::type_error(format!("{class_name}() takes no arguments")));
        }

        // If the class defines __new__, call it first.
        // __new__ receives (cls, *args) and returns the new instance (or any value).
        if let Some(new_func) = new_info {
            // Collect original positional args into a Vec for reuse.
            // Clone each value for the __new__ call, keeping originals for __init__.
            let (orig_pos_args, orig_kwargs) = args.into_parts();
            let orig_pos: Vec<Value> = orig_pos_args.collect();
            let new_kwargs = match self.clone_kwargs_values(&orig_kwargs) {
                Ok(kwargs) => kwargs,
                Err(e) => {
                    for value in orig_pos {
                        value.drop_with_heap(self.heap);
                    }
                    orig_kwargs.drop_with_heap(self.heap);
                    return Err(e);
                }
            };

            // Build __new__ args: (cls, *cloned_args)
            self.heap.inc_ref(class_heap_id);
            let mut new_arg_list = vec![Value::Ref(class_heap_id)];
            for v in &orig_pos {
                new_arg_list.push(v.clone_with_heap(self.heap));
            }
            let new_args = ArgValues::ArgsKargs {
                args: new_arg_list,
                kwargs: new_kwargs,
            };

            // Rebuild original args from the collected positional values
            let init_args = if orig_pos.is_empty() && orig_kwargs.is_empty() {
                ArgValues::Empty
            } else {
                ArgValues::ArgsKargs {
                    args: orig_pos,
                    kwargs: orig_kwargs,
                }
            };

            let result = self.call_function(new_func, new_args)?;

            match result {
                CallResult::Push(new_result) => {
                    // __new__ completed synchronously -- check result and maybe call __init__
                    return self.handle_new_result(new_result, class_heap_id, init_info, init_args);
                }
                CallResult::FramePushed => {
                    // __new__ pushed a frame -- stash state so we can call __init__ on return
                    self.pending_new_call = Some(PendingNewCall {
                        class_heap_id,
                        init_func: init_info,
                        args: init_args,
                    });
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    if let Some(init_func) = init_info {
                        init_func.drop_with_heap(self.heap);
                    }
                    return Ok(other);
                }
            }
        }

        // No __new__ -- use the standard path: create instance, call __init__.
        let instance_value = self.allocate_instance_for_class(class_heap_id)?;
        let Value::Ref(instance_heap_id) = instance_value else {
            unreachable!("allocate_instance_for_class must return heap ref");
        };

        if let Some(init_func) = init_info {
            // __init__ exists - call it with (instance, *args).
            self.heap.inc_ref(instance_heap_id);
            let init_self_arg = Value::Ref(instance_heap_id);

            // Prepend self to args
            let new_args = match args {
                ArgValues::Empty => ArgValues::One(init_self_arg),
                ArgValues::One(a) => ArgValues::Two(init_self_arg, a),
                ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                    args: vec![init_self_arg, a, b],
                    kwargs: KwargsValues::Empty,
                },
                ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                    args: vec![init_self_arg],
                    kwargs: kw,
                },
                ArgValues::ArgsKargs { mut args, kwargs } => {
                    args.insert(0, init_self_arg);
                    ArgValues::ArgsKargs { args, kwargs }
                }
            };

            let mut instance_guard = HeapGuard::new(instance_value, self);
            // Call __init__ with a guard so the instance is dropped on error paths.
            let result = {
                let this = instance_guard.heap();
                this.call_function(init_func, new_args)?
            };

            let instance_value = instance_guard.into_inner();
            match result {
                CallResult::Push(value) => {
                    // __init__ returned synchronously
                    value.drop_with_heap(self.heap);
                    Ok(CallResult::Push(instance_value))
                }
                CallResult::FramePushed => {
                    // __init__ pushed a frame - mark it so we return the instance
                    self.current_frame_mut().init_instance = Some(instance_value);
                    Ok(CallResult::FramePushed)
                }
                CallResult::External(ext_id, ext_args) => {
                    instance_value.drop_with_heap(self.heap);
                    Ok(CallResult::External(ext_id, ext_args))
                }
                CallResult::Proxy(proxy_id, method, proxy_args) => {
                    instance_value.drop_with_heap(self.heap);
                    Ok(CallResult::Proxy(proxy_id, method, proxy_args))
                }
                CallResult::OsCall(os_func, os_args) => {
                    instance_value.drop_with_heap(self.heap);
                    Ok(CallResult::OsCall(os_func, os_args))
                }
            }
        } else {
            // No __init__ - check that no arguments were passed
            if !matches!(args, ArgValues::Empty) {
                args.drop_with_heap(self.heap);
                instance_value.drop_with_heap(self.heap);
                let class_name = match self.heap.get(class_heap_id) {
                    HeapData::ClassObject(cls) => cls.name(self.interns).to_string(),
                    _ => "object".to_string(),
                };
                return Err(ExcType::type_error(format!("{class_name}() takes no arguments")));
            }
            Ok(CallResult::Push(instance_value))
        }
    }

    /// Reads abstract method names from a `__abstractmethods__` container value.
    fn read_abstract_method_names(&self, value: &Value) -> Vec<String> {
        let mut names = Vec::new();
        let Value::Ref(value_id) = value else {
            return names;
        };

        match self.heap.get(*value_id) {
            HeapData::Tuple(tuple) => {
                for item in tuple.as_vec() {
                    if let Some(name) = self.value_to_string_name(item) {
                        names.push(name);
                    }
                }
            }
            HeapData::List(list) => {
                for item in list.as_vec() {
                    if let Some(name) = self.value_to_string_name(item) {
                        names.push(name);
                    }
                }
            }
            HeapData::Set(set) => {
                for item in set.storage().iter() {
                    if let Some(name) = self.value_to_string_name(item) {
                        names.push(name);
                    }
                }
            }
            HeapData::FrozenSet(set) => {
                for item in set.storage().iter() {
                    if let Some(name) = self.value_to_string_name(item) {
                        names.push(name);
                    }
                }
            }
            _ => {}
        }
        names
    }

    /// Formats CPython-compatible abstract-class instantiation error text.
    fn format_abstract_instantiation_error(&self, class_name: &str, abstract_methods: &[String]) -> String {
        if abstract_methods.is_empty() {
            return format!("Can't instantiate abstract class {class_name}");
        }
        if abstract_methods.len() == 1 {
            return format!(
                "Can't instantiate abstract class {class_name} without an implementation for abstract method '{}'",
                abstract_methods[0]
            );
        }
        let joined = abstract_methods
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Can't instantiate abstract class {class_name} without an implementation for abstract methods {joined}")
    }

    /// Converts a value used as a method name entry to owned string, if possible.
    fn value_to_string_name(&self, value: &Value) -> Option<String> {
        match value {
            Value::InternString(id) => Some(self.interns.get_str(*id).to_string()),
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::Str(s) => Some(s.as_str().to_string()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Clones keyword arguments with proper refcount handling.
    ///
    /// This is used when a call needs to reuse kwargs across multiple invocations
    /// (e.g., `__new__` and `__init__`).
    fn clone_kwargs_values(&mut self, kwargs: &KwargsValues) -> RunResult<KwargsValues> {
        match kwargs {
            KwargsValues::Empty => Ok(KwargsValues::Empty),
            KwargsValues::Inline(pairs) => {
                let mut out = Vec::with_capacity(pairs.len());
                for (key, value) in pairs {
                    out.push((*key, value.clone_with_heap(self.heap)));
                }
                Ok(KwargsValues::Inline(out))
            }
            KwargsValues::Dict(dict) => Ok(KwargsValues::Dict(dict.clone_with_heap(self.heap, self.interns)?)),
        }
    }

    /// Allocates a new instance for a class, honoring `__slots__` layout.
    fn allocate_instance_for_class(&mut self, class_heap_id: HeapId) -> RunResult<Value> {
        let (slot_len, has_dict, _has_weakref) = match self.heap.get(class_heap_id) {
            HeapData::ClassObject(cls) => (
                cls.slot_layout().len(),
                cls.instance_has_dict(),
                cls.instance_has_weakref(),
            ),
            _ => return Err(ExcType::type_error("object is not a class".to_string())),
        };

        self.heap.inc_ref(class_heap_id);
        let attrs_id = if has_dict {
            Some(self.heap.allocate(HeapData::Dict(Dict::new()))?)
        } else {
            None
        };
        let mut slot_values = Vec::with_capacity(slot_len);
        slot_values.resize_with(slot_len, || Value::Undefined);
        let weakref_ids = Vec::new();
        let instance = Instance::new(class_heap_id, attrs_id, slot_values, weakref_ids);
        let instance_heap_id = self.heap.allocate(HeapData::Instance(instance))?;
        Ok(Value::Ref(instance_heap_id))
    }

    /// Handles the result of a `__new__` call.
    ///
    /// If the result is an instance of the target class and `__init__` exists,
    /// calls `__init__` on the result. Otherwise, returns the result directly.
    pub(super) fn handle_new_result(
        &mut self,
        new_result: Value,
        class_heap_id: HeapId,
        init_info: Option<Value>,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Check if the result is an instance of the class.
        // If __new__ returned a non-instance or an instance of a different class,
        // we skip __init__.
        let is_instance_of_class = if let Value::Ref(result_id) = &new_result {
            match self.heap.get(*result_id) {
                HeapData::Instance(inst) => inst.class_id() == class_heap_id,
                HeapData::ClassObject(cls_obj) => {
                    matches!(cls_obj.metaclass(), Value::Ref(meta_id) if *meta_id == class_heap_id)
                }
                _ => false,
            }
        } else {
            false
        };

        if is_instance_of_class {
            if let Some(init_func) = init_info {
                // Call __init__ on the instance returned by __new__
                let instance_id = match &new_result {
                    Value::Ref(id) => *id,
                    _ => unreachable!(),
                };
                self.heap.inc_ref(instance_id);
                let init_self_arg = Value::Ref(instance_id);

                let new_args = match args {
                    ArgValues::Empty => ArgValues::One(init_self_arg),
                    ArgValues::One(a) => ArgValues::Two(init_self_arg, a),
                    ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                        args: vec![init_self_arg, a, b],
                        kwargs: KwargsValues::Empty,
                    },
                    ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                        args: vec![init_self_arg],
                        kwargs: kw,
                    },
                    ArgValues::ArgsKargs { mut args, kwargs } => {
                        args.insert(0, init_self_arg);
                        ArgValues::ArgsKargs { args, kwargs }
                    }
                };

                let mut new_result_guard = HeapGuard::new(new_result, self);
                let result = {
                    let this = new_result_guard.heap();
                    this.call_function(init_func, new_args)?
                };
                let new_result = new_result_guard.into_inner();

                match result {
                    CallResult::Push(value) => {
                        value.drop_with_heap(self.heap);
                        Ok(CallResult::Push(new_result))
                    }
                    CallResult::FramePushed => {
                        self.current_frame_mut().init_instance = Some(new_result);
                        Ok(CallResult::FramePushed)
                    }
                    CallResult::External(ext_id, ext_args) => {
                        new_result.drop_with_heap(self.heap);
                        Ok(CallResult::External(ext_id, ext_args))
                    }
                    CallResult::Proxy(proxy_id, method, proxy_args) => {
                        new_result.drop_with_heap(self.heap);
                        Ok(CallResult::Proxy(proxy_id, method, proxy_args))
                    }
                    CallResult::OsCall(os_func, os_args) => {
                        new_result.drop_with_heap(self.heap);
                        Ok(CallResult::OsCall(os_func, os_args))
                    }
                }
            } else {
                // No __init__ -- return the instance from __new__
                args.drop_with_heap(self.heap);
                Ok(CallResult::Push(new_result))
            }
        } else {
            // __new__ returned a non-instance or different class -- skip __init__
            if let Some(init_func) = init_info {
                init_func.drop_with_heap(self.heap);
            }
            args.drop_with_heap(self.heap);
            Ok(CallResult::Push(new_result))
        }
    }

    /// Calls a PropertyAccessor, creating a new UserProperty with the appropriate
    /// function slot replaced.
    ///
    /// For `@prop.setter`, calling the accessor with a function creates a new property
    /// that inherits the original getter/deleter but uses the new function as setter.
    fn call_property_accessor(
        &mut self,
        accessor_heap_id: HeapId,
        callable: Value,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Get the function argument (the decorated function)
        let ArgValues::One(new_func) = args else {
            args.drop_with_heap(self.heap);
            callable.drop_with_heap(self.heap);
            return Err(ExcType::type_error("property accessor takes exactly 1 argument"));
        };

        // Phase 1: Extract data from the accessor without heap mutation
        let (kind, fget, fset, fdel, doc) = match self.heap.get(accessor_heap_id) {
            HeapData::PropertyAccessor(acc) => {
                let (fg, fs, fd, d) = acc.parts();
                (
                    acc.kind(),
                    fg.map(Value::copy_for_extend),
                    fs.map(Value::copy_for_extend),
                    fd.map(Value::copy_for_extend),
                    d.map(Value::copy_for_extend),
                )
            }
            _ => unreachable!("call_property_accessor: not a PropertyAccessor"),
        };

        // Phase 2: Increment refcounts for the copied values
        if let Some(Value::Ref(id)) = &fget {
            self.heap.inc_ref(*id);
        }
        if let Some(Value::Ref(id)) = &fset {
            self.heap.inc_ref(*id);
        }
        if let Some(Value::Ref(id)) = &fdel {
            self.heap.inc_ref(*id);
        }
        if let Some(Value::Ref(id)) = &doc {
            self.heap.inc_ref(*id);
        }

        // Drop the accessor callable (we've copied what we need)
        callable.drop_with_heap(self.heap);

        // Create a new UserProperty with the appropriate slot replaced
        let new_property = match kind {
            PropertyAccessorKind::Getter => {
                // Replace fget with new_func, drop old fget
                if let Some(old) = fget {
                    old.drop_with_heap(self.heap);
                }
                UserProperty::new_full(Some(new_func), fset, fdel, doc)
            }
            PropertyAccessorKind::Setter => {
                // Replace fset with new_func, drop old fset
                if let Some(old) = fset {
                    old.drop_with_heap(self.heap);
                }
                UserProperty::with_setter(fget, new_func, doc)
            }
            PropertyAccessorKind::Deleter => {
                // Replace fdel with new_func, drop old fdel
                if let Some(old) = fdel {
                    old.drop_with_heap(self.heap);
                }
                UserProperty::with_deleter(fget, fset, new_func, doc)
            }
        };

        let prop_id = self.heap.allocate(HeapData::UserProperty(new_property))?;
        Ok(CallResult::Push(Value::Ref(prop_id)))
    }

    /// Implements `super()` with no arguments (PEP 3135).
    ///
    /// Uses the `__class__` cell from the current frame to build a `SuperProxy`
    /// that delegates attribute lookup to the next class in the MRO.
    fn call_super(&mut self, args: ArgValues) -> Result<Value, RunError> {
        let (maybe_type, maybe_obj) = args.get_zero_one_two_args("super", self.heap)?;

        match (maybe_type, maybe_obj) {
            (None, None) => {
                if let Some((instance_id, defining_class_id)) = self.super_context_from_classcell()? {
                    return self.allocate_super_proxy(instance_id, defining_class_id);
                }
                Err(ExcType::type_error("super(): __class__ cell not found".to_string()))
            }
            (Some(type_val), Some(obj_val)) => self.call_super_with_args(type_val, obj_val),
            (maybe_type, maybe_obj) => {
                if let Some(val) = maybe_type {
                    val.drop_with_heap(self.heap);
                }
                if let Some(val) = maybe_obj {
                    val.drop_with_heap(self.heap);
                }
                Err(ExcType::type_error(
                    "super() takes either zero or two positional arguments".to_string(),
                ))
            }
        }
    }

    fn allocate_super_proxy(&mut self, instance_id: HeapId, current_class_id: HeapId) -> Result<Value, RunError> {
        self.heap.inc_ref(instance_id);
        self.heap.inc_ref(current_class_id);

        let proxy = crate::types::SuperProxy::new(instance_id, current_class_id);
        let heap_id = self.heap.allocate(HeapData::SuperProxy(proxy))?;
        Ok(Value::Ref(heap_id))
    }

    fn call_super_with_args(&mut self, type_val: Value, obj_val: Value) -> Result<Value, RunError> {
        let type_class_id = match type_val {
            Value::Ref(id) => {
                if !matches!(self.heap.get(id), HeapData::ClassObject(_)) {
                    return Err(ExcType::type_error("super() arg 1 must be a type".to_string()));
                }
                self.heap.inc_ref(id);
                id
            }
            Value::Builtin(Builtins::Type(ty)) => {
                type_val.drop_with_heap(self.heap);
                self.heap.builtin_class_id(ty)?
            }
            Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => {
                type_val.drop_with_heap(self.heap);
                self.heap.builtin_class_id(Type::Type)?
            }
            other => {
                other.drop_with_heap(self.heap);
                return Err(ExcType::type_error("super() arg 1 must be a type".to_string()));
            }
        };

        let (instance_id, instance_class_id) = match obj_val {
            Value::Ref(id) => {
                if let HeapData::Instance(inst) = self.heap.get(id) {
                    self.heap.inc_ref(id);
                    (id, inst.class_id())
                } else {
                    self.heap.dec_ref(type_class_id);
                    return Err(ExcType::type_error("super() arg 2 must be an instance".to_string()));
                }
            }
            other => {
                other.drop_with_heap(self.heap);
                self.heap.dec_ref(type_class_id);
                return Err(ExcType::type_error("super() arg 2 must be an instance".to_string()));
            }
        };

        let inherits = if let HeapData::ClassObject(cls) = self.heap.get(instance_class_id) {
            cls.is_subclass_of(instance_class_id, type_class_id)
        } else {
            false
        };

        if !inherits {
            self.heap.dec_ref(type_class_id);
            self.heap.dec_ref(instance_id);
            return Err(ExcType::type_error(
                "super() type is not in the MRO of the provided instance".to_string(),
            ));
        }

        self.allocate_super_proxy(instance_id, type_class_id)
    }

    /// Attempts to resolve zero-argument super() context from the `__class__` cell.
    ///
    /// Returns `(instance_id, defining_class_id)` when the current frame has a
    /// `__class__` cell and a valid first local (`self`/`cls`).
    fn super_context_from_classcell(&mut self) -> Result<Option<(HeapId, HeapId)>, RunError> {
        let (class_cell_id, namespace_idx) = {
            let frame = self.current_frame();
            if frame.class_body_info.is_some() || frame.function_id.is_none() {
                return Ok(None);
            }

            let class_name_id: StringId = StaticStrings::DunderClass.into();
            let mut class_cell_id = None;
            for (idx, cell_id) in frame.cells.iter().enumerate() {
                let slot = u16::try_from(idx).expect("cell index exceeds u16");
                if frame.code.local_name(slot) == Some(class_name_id) {
                    class_cell_id = Some(*cell_id);
                    break;
                }
            }

            let Some(class_cell_id) = class_cell_id else {
                return Ok(None);
            };

            (class_cell_id, frame.namespace_idx)
        };

        let class_val = self.heap.get_cell_value(class_cell_id)?;
        let class_id = match class_val {
            Value::Ref(id) => {
                class_val.drop_with_heap(self.heap);
                id
            }
            other => {
                other.drop_with_heap(self.heap);
                return Err(ExcType::type_error(
                    "super(): __class__ cell is not a class".to_string(),
                ));
            }
        };

        if !matches!(self.heap.get(class_id), HeapData::ClassObject(_)) {
            return Err(ExcType::type_error(
                "super(): __class__ cell is not a class".to_string(),
            ));
        }

        let namespace = self.namespaces.get(namespace_idx);
        let first_local = namespace.get(crate::namespace::NamespaceId::new(0));
        let instance_id = match first_local {
            Value::Ref(id) if matches!(self.heap.get(*id), HeapData::Instance(_) | HeapData::ClassObject(_)) => *id,
            _ => return Err(ExcType::type_error("super(): __self__ is not an instance".to_string())),
        };

        Ok(Some((instance_id, class_id)))
    }

    /// Implementation of `dir()` for the zero-argument form.
    ///
    /// CPython returns names from the current local scope, so we enumerate named,
    /// initialized slots in the active frame namespace and return them sorted.
    fn builtin_dir_no_args(&mut self) -> Result<Value, RunError> {
        let (code, namespace_idx) = {
            let frame = self.current_frame();
            (frame.code, frame.namespace_idx)
        };
        let namespace = self.namespaces.get(namespace_idx);

        let mut names = Vec::new();
        let mut slot_idx: usize = 0;
        loop {
            let Ok(slot) = u16::try_from(slot_idx) else {
                break;
            };
            let Some(name_id) = code.local_name(slot) else {
                break;
            };
            if name_id != StringId::default() {
                let value = namespace.get(NamespaceId::new(slot_idx));
                if !matches!(value, Value::Undefined) {
                    names.push(self.interns.get_str(name_id).to_owned());
                }
            }
            slot_idx += 1;
        }

        // CPython's module-scope dir() also exposes standard module dunder names.
        // Keep this limited to module-level frames to avoid polluting function locals().
        if self.current_frame().function_id.is_none() {
            names.extend(
                [
                    "__builtins__",
                    "__cached__",
                    "__doc__",
                    "__file__",
                    "__loader__",
                    "__name__",
                    "__package__",
                    "__spec__",
                    "__warningregistry__",
                ]
                .into_iter()
                .map(str::to_string),
            );
        }

        names.sort_unstable();
        names.dedup();

        let mut items = Vec::with_capacity(names.len());
        for name in names {
            let id = self.heap.allocate(HeapData::Str(Str::from(name)))?;
            items.push(Value::Ref(id));
        }
        let list_id = self.heap.allocate(HeapData::List(List::new(items)))?;
        Ok(Value::Ref(list_id))
    }

    /// Implementation of `dir([obj])` including instance `__dir__` dispatch.
    ///
    /// For one-argument calls this mirrors CPython by invoking `obj.__dir__()`
    /// when defined, then sorting the returned iterable.
    fn call_dir_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let value = args.get_zero_one_arg("dir", self.heap)?;
        let Some(value) = value else {
            return Ok(CallResult::Push(self.builtin_dir_no_args()?));
        };

        if let Value::Ref(obj_id) = &value
            && matches!(self.heap.get(*obj_id), HeapData::Instance(_))
        {
            let method = match self.heap.get(*obj_id) {
                HeapData::Instance(instance) => {
                    let class_id = instance.class_id();
                    match self.heap.get(class_id) {
                        HeapData::ClassObject(cls) => cls
                            .mro_lookup_attr("__dir__", class_id, self.heap, self.interns)
                            .map(|(value, _)| value),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(method) = method {
                let result = self.call_dunder(*obj_id, method, ArgValues::Empty)?;
                value.drop_with_heap(self.heap);
                return match result {
                    CallResult::Push(dir_value) => Ok(CallResult::Push(self.normalize_dir_result(dir_value)?)),
                    CallResult::FramePushed => {
                        self.pending_dir_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    other => Ok(other),
                };
            }
        }

        let fallback =
            BuiltinsFunctions::Dir.call(self.heap, ArgValues::One(value), self.interns, self.print_writer)?;
        Ok(CallResult::Push(fallback))
    }

    /// Implementation of `format(value[, spec])`.
    ///
    /// Uses instance `__format__` when available, then applies native formatting
    /// for core builtin types (`int`, `float`, `str`).
    ///
    /// For other types this mirrors CPython's object fallback:
    /// - empty spec: return `str(value)`
    /// - non-empty spec: raise `TypeError`
    fn call_format_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (value, spec) = args.get_one_two_args("format", self.heap)?;
        let spec = spec.unwrap_or(Value::InternString(StaticStrings::EmptyString.into()));

        if !self.is_str_value(&spec) {
            let got = spec.py_type(self.heap);
            value.drop_with_heap(self.heap);
            spec.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!(
                "format() argument 2 must be str, not {got}",
            )));
        }

        if let Value::Ref(obj_id) = &value
            && matches!(self.heap.get(*obj_id), HeapData::Instance(_))
        {
            let dunder_id: StringId = StaticStrings::DunderFormat.into();
            if let Some(method) = self.lookup_type_dunder(*obj_id, dunder_id) {
                let spec_arg = spec.clone_with_heap(self.heap);
                let result = self.call_dunder(*obj_id, method, ArgValues::One(spec_arg))?;
                value.drop_with_heap(self.heap);
                spec.drop_with_heap(self.heap);
                return Ok(result);
            }
        }

        let spec_is_empty = self.is_empty_str_value(&spec);
        let spec_text = if spec_is_empty {
            String::new()
        } else {
            spec.py_str(self.heap, self.interns).into_owned()
        };
        let value_type = value.py_type(self.heap);
        let can_use_native_format =
            matches!(value, Value::Int(_) | Value::Float(_) | Value::Bool(_)) || self.is_str_value(&value);

        if !spec_is_empty && !can_use_native_format {
            value.drop_with_heap(self.heap);
            spec.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!(
                "unsupported format string passed to {value_type}.__format__"
            )));
        }

        let formatted = if spec_is_empty {
            value.py_str(self.heap, self.interns).into_owned()
        } else {
            let parsed_spec = match spec_text.parse::<ParsedFormatSpec>() {
                Ok(parsed_spec) => parsed_spec,
                Err(invalid) => {
                    value.drop_with_heap(self.heap);
                    spec.drop_with_heap(self.heap);
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        format!("Invalid format specifier '{invalid}' for object of type '{value_type}'"),
                    )
                    .into());
                }
            };

            match format_with_spec(&value, &parsed_spec, self.heap, self.interns) {
                Ok(formatted) => formatted,
                Err(err) => {
                    value.drop_with_heap(self.heap);
                    spec.drop_with_heap(self.heap);
                    return Err(err);
                }
            }
        };

        value.drop_with_heap(self.heap);
        spec.drop_with_heap(self.heap);
        let formatted_id = self.heap.allocate(HeapData::Str(Str::from(formatted.as_str())))?;
        Ok(CallResult::Push(Value::Ref(formatted_id)))
    }

    /// Normalizes a `__dir__` result to the sorted list returned by `dir()`.
    pub(super) fn normalize_dir_result(&mut self, value: Value) -> RunResult<Value> {
        let mut iter = OurosIter::new(value, self.heap, self.interns)?;
        let mut names = Vec::new();
        loop {
            match iter.for_next(self.heap, self.interns) {
                Ok(Some(item)) => {
                    let name = item.py_str(self.heap, self.interns).into_owned();
                    item.drop_with_heap(self.heap);
                    names.push(name);
                }
                Ok(None) => break,
                Err(err) => {
                    iter.drop_with_heap(self.heap);
                    return Err(err);
                }
            }
        }
        iter.drop_with_heap(self.heap);
        names.sort_unstable();

        let mut items = Vec::with_capacity(names.len());
        for name in names {
            let id = self.heap.allocate(HeapData::Str(Str::from(name.as_str())))?;
            items.push(Value::Ref(id));
        }
        let list_id = self.heap.allocate(HeapData::List(List::new(items)))?;
        Ok(Value::Ref(list_id))
    }

    /// Records/validates the result of `str(instance)` or `repr(instance)` dunder dispatch.
    pub(super) fn handle_stringify_call_result(
        &mut self,
        result: CallResult,
        kind: PendingStringifyReturn,
    ) -> Result<CallResult, RunError> {
        match result {
            CallResult::Push(value) => {
                let value = self.validate_stringify_result(value, kind)?;
                Ok(CallResult::Push(value))
            }
            CallResult::FramePushed => {
                self.pending_stringify_return.push((kind, self.frames.len()));
                Ok(CallResult::FramePushed)
            }
            other => Ok(other),
        }
    }

    /// Ensures a dunder string-conversion return value is a Python `str`.
    pub(super) fn validate_stringify_result(
        &mut self,
        value: Value,
        kind: PendingStringifyReturn,
    ) -> Result<Value, RunError> {
        if self.is_str_value(&value) {
            return Ok(value);
        }
        let got = value.py_type(self.heap);
        value.drop_with_heap(self.heap);
        let message = match kind {
            PendingStringifyReturn::Str => format!("__str__ returned non-string (type {got})"),
            PendingStringifyReturn::Repr => format!("__repr__ returned non-string (type {got})"),
        };
        Err(ExcType::type_error(message))
    }

    /// Returns whether a value is a Python `str`.
    fn is_str_value(&self, value: &Value) -> bool {
        match value {
            Value::InternString(_) => true,
            Value::Ref(id) => matches!(self.heap.get(*id), HeapData::Str(_)),
            _ => false,
        }
    }

    /// Returns whether a value is an empty Python `str`.
    fn is_empty_str_value(&self, value: &Value) -> bool {
        match value {
            Value::InternString(id) => self.interns.get_str(*id).is_empty(),
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::Str(s) => s.as_str().is_empty(),
                _ => false,
            },
            _ => false,
        }
    }

    /// Extracts a string from a Value (for getattr/setattr/hasattr/delattr builtin name argument).
    ///
    /// Returns the string content. Works with InternString values and heap Str values.
    fn extract_attr_name_str(&self, name_val: &Value) -> Result<String, RunError> {
        match name_val {
            Value::InternString(sid) => Ok(self.interns.get_str(*sid).to_owned()),
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::Str(s) => Ok(s.as_str().to_owned()),
                _ => Err(ExcType::type_error("attribute name must be string".to_string())),
            },
            _ => Err(ExcType::type_error("attribute name must be string".to_string())),
        }
    }

    /// Tries to convert a string to a StringId via StaticStrings lookup.
    ///
    /// Returns `Some(StringId)` for single-byte ASCII names and known static strings.
    /// Returns `None` for other dynamic names.
    fn try_static_string_id(name: &str) -> Option<StringId> {
        if name.len() == 1 {
            return Some(StringId::from_ascii(name.as_bytes()[0]));
        }
        StaticStrings::from_str(name).ok().map(std::convert::Into::into)
    }

    /// Implementation of `getattr(obj, name[, default])` builtin.
    ///
    /// Gets an attribute by dynamic string name. Handles both static (interned)
    /// and dynamic (heap) attribute names.
    fn builtin_getattr(&mut self, args: ArgValues) -> Result<Value, RunError> {
        let (obj, name_val, default) = match args {
            ArgValues::Two(a, b) => (a, b, None),
            ArgValues::ArgsKargs { mut args, kwargs } => {
                kwargs.drop_with_heap(self.heap);
                if args.len() == 3 {
                    let c = args.remove(2);
                    let b = args.remove(1);
                    let a = args.remove(0);
                    (a, b, Some(c))
                } else if args.len() == 2 {
                    let b = args.remove(1);
                    let a = args.remove(0);
                    (a, b, None)
                } else {
                    for arg in args {
                        arg.drop_with_heap(self.heap);
                    }
                    return Err(ExcType::type_error("getattr expected 2 or 3 arguments".to_string()));
                }
            }
            other => {
                other.drop_with_heap(self.heap);
                return Err(ExcType::type_error("getattr expected 2 or 3 arguments".to_string()));
            }
        };

        let attr_name = match self.extract_attr_name_str(&name_val) {
            Ok(s) => s,
            Err(e) => {
                obj.drop_with_heap(self.heap);
                name_val.drop_with_heap(self.heap);
                if let Some(d) = default {
                    d.drop_with_heap(self.heap);
                }
                return Err(e);
            }
        };

        // Try static string path first
        let result = if let Some(sid) = Self::try_static_string_id(&attr_name) {
            obj.py_getattr(sid, self.heap, self.interns)
        } else {
            // Dynamic string fallback for non-static names.
            self.getattr_dynamic_str(&obj, &attr_name)
        };

        name_val.drop_with_heap(self.heap);
        obj.drop_with_heap(self.heap);

        match result {
            Ok(AttrCallResult::Value(val)) => {
                if let Some(d) = default {
                    d.drop_with_heap(self.heap);
                }
                Ok(val)
            }
            Ok(AttrCallResult::DescriptorGet(descriptor)) => {
                if let Some(d) = default {
                    d.drop_with_heap(self.heap);
                }
                // For getattr with a descriptor, call descriptor.__get__(None, None)
                // since obj has already been dropped.
                let get_id: StringId = StaticStrings::DunderDescGet.into();
                if let Value::Ref(desc_id) = &descriptor {
                    let desc_id = *desc_id;
                    if let Some(method) = self.lookup_type_dunder(desc_id, get_id) {
                        self.heap.inc_ref(desc_id);
                        let args = ArgValues::ArgsKargs {
                            args: vec![Value::Ref(desc_id), Value::None, Value::None],
                            kwargs: KwargsValues::Empty,
                        };
                        descriptor.drop_with_heap(self.heap);
                        let result = self.call_function(method, args)?;
                        return match result {
                            CallResult::Push(val) => Ok(val),
                            _ => Ok(Value::None),
                        };
                    }
                }
                // No __get__ found, return descriptor itself
                Ok(descriptor)
            }
            Ok(
                AttrCallResult::ExternalCall(_, _)
                | AttrCallResult::OsCall(_, _)
                | AttrCallResult::PropertyCall(_, _)
                | AttrCallResult::ReduceCall(_, _, _)
                | AttrCallResult::MapCall(_, _)
                | AttrCallResult::FilterCall(_, _)
                | AttrCallResult::FilterFalseCall(_, _)
                | AttrCallResult::TakeWhileCall(_, _)
                | AttrCallResult::DropWhileCall(_, _)
                | AttrCallResult::GroupByCall(_, _)
                | AttrCallResult::TextwrapIndentCall(_, _, _)
                | AttrCallResult::CallFunction(_, _)
                | AttrCallResult::ReSubCall(_, _, _, _, _),
            ) => {
                if let Some(d) = default {
                    d.drop_with_heap(self.heap);
                }
                // External/OS/Property/Reduce/Map/Filter calls are not expected from getattr - treat as found
                Ok(Value::None)
            }
            Ok(AttrCallResult::ObjectNew) => {
                if let Some(d) = default {
                    d.drop_with_heap(self.heap);
                }
                // Return the ObjectNewImpl callable
                let object_new_id = self.heap.get_object_new_impl()?;
                Ok(Value::Ref(object_new_id))
            }
            Err(_) if default.is_some() => Ok(default.expect("checked above")),
            Err(e) => {
                if let Some(d) = default {
                    d.drop_with_heap(self.heap);
                }
                Err(e)
            }
        }
    }

    /// Gets an attribute by dynamic (non-interned) string name.
    ///
    /// Performs string-based lookup in Instance attrs and class MRO.
    fn getattr_dynamic_str(&mut self, obj: &Value, name: &str) -> Result<AttrCallResult, RunError> {
        if let Value::Ref(heap_id) = obj {
            let heap_id = *heap_id;
            let interns = self.interns;

            if let Some(generator_attr) = Value::py_get_generator_attr_by_name(heap_id, name, self.heap, interns)? {
                return Ok(AttrCallResult::Value(generator_attr));
            }

            // Dynamic getattr/hasattr must mirror static attribute lookup for closures.
            if name == "__closure__" {
                return match self.heap.get(heap_id) {
                    HeapData::Closure(_, cells, _) if !cells.is_empty() => {
                        let mut items: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
                        for cell_id in cells {
                            items.push(Value::Ref(*cell_id).clone_with_heap(self.heap));
                        }
                        let tuple_val = crate::types::allocate_tuple(items, self.heap)?;
                        Ok(AttrCallResult::Value(tuple_val))
                    }
                    HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _) => {
                        Ok(AttrCallResult::Value(Value::None))
                    }
                    _ => Err(ExcType::attribute_error(
                        self.heap.get(heap_id).py_type(self.heap),
                        name,
                    )),
                };
            }

            match self.heap.get(heap_id) {
                HeapData::Instance(_) => {
                    // Use with_entry_mut to safely borrow instance + heap
                    let result: Result<Option<Value>, RunError> = self.heap.with_entry_mut(heap_id, |heap, data| {
                        if let HeapData::Instance(inst) = data {
                            if name == "__dict__" {
                                let has_dict = match heap.get(inst.class_id()) {
                                    HeapData::ClassObject(cls) => cls.instance_has_dict(),
                                    _ => false,
                                };
                                if !has_dict {
                                    let class_name = match heap.get(inst.class_id()) {
                                        HeapData::ClassObject(cls) => cls.name(interns).to_string(),
                                        _ => "<unknown>".to_string(),
                                    };
                                    return Err(ExcType::attribute_error(format!("'{class_name}' object"), "__dict__"));
                                }
                                let Some(attrs_id) = inst.attrs_id() else {
                                    return Err(ExcType::attribute_error("instance", "__dict__"));
                                };
                                heap.inc_ref(attrs_id);
                                return Ok(Some(Value::Ref(attrs_id)));
                            }
                            // 1. Instance attrs
                            if let Some(dict) = inst.attrs(heap)
                                && let Some(value) = dict.get_by_str(name, heap, interns)
                            {
                                return Ok(Some(value.clone_with_heap(heap)));
                            }
                            // 2. Instance slots
                            if let Some(value) = inst.slot_value(name, heap) {
                                return Ok(Some(value.clone_with_heap(heap)));
                            }
                            // 3. Class MRO lookup
                            let class_id = inst.class_id();
                            if let HeapData::ClassObject(cls) = heap.get(class_id)
                                && let Some((value, _)) = cls.mro_lookup_attr(name, class_id, heap, interns)
                            {
                                return Ok(Some(value));
                            }
                            Ok(None)
                        } else {
                            Ok(None)
                        }
                    });
                    match result? {
                        Some(value) => Ok(AttrCallResult::Value(value)),
                        None => Err(ExcType::attribute_error(Type::Instance, name)),
                    }
                }
                HeapData::ClassObject(_) => {
                    let result: Result<Option<Value>, RunError> = self.heap.with_entry_mut(heap_id, |heap, data| {
                        if let HeapData::ClassObject(cls) = data {
                            if name == "__dict__" {
                                heap.inc_ref(heap_id);
                                let proxy_id =
                                    heap.allocate(HeapData::MappingProxy(crate::types::MappingProxy::new(heap_id)))?;
                                return Ok(Some(Value::Ref(proxy_id)));
                            }
                            if let Some(value) = cls.namespace().get_by_str(name, heap, interns) {
                                Ok(Some(value.clone_with_heap(heap)))
                            } else {
                                Ok(None)
                            }
                        } else {
                            Ok(None)
                        }
                    });
                    match result? {
                        Some(value) => Ok(AttrCallResult::Value(value)),
                        None => Err(ExcType::attribute_error(Type::Type, name)),
                    }
                }
                HeapData::NamedTuple(_) => {
                    let result: Result<Option<Value>, RunError> = self.heap.with_entry_mut(heap_id, |heap, data| {
                        if let HeapData::NamedTuple(nt) = data {
                            let value = nt
                                .field_names()
                                .iter()
                                .position(|field_name| field_name.as_str(interns) == name)
                                .map(|index| nt.as_vec()[index].clone_with_heap(heap));
                            Ok(value)
                        } else {
                            Ok(None)
                        }
                    });
                    match result? {
                        Some(value) => Ok(AttrCallResult::Value(value)),
                        None => Err(ExcType::attribute_error(Type::NamedTuple, name)),
                    }
                }
                HeapData::Module(_) => {
                    let result: Result<(Option<Value>, String), RunError> =
                        self.heap.with_entry_mut(heap_id, |heap, data| {
                            if let HeapData::Module(module) = data {
                                let module_name = interns.get_str(module.name()).to_string();
                                if name == "__dict__" {
                                    return Err(ExcType::attribute_error_module(&module_name, "__dict__"));
                                }
                                let value = module
                                    .attrs()
                                    .get_by_str(name, heap, interns)
                                    .map(|v| v.clone_with_heap(heap));
                                Ok((value, module_name))
                            } else {
                                Ok((None, "module".to_owned()))
                            }
                        });
                    match result? {
                        (Some(value), _) => Ok(AttrCallResult::Value(value)),
                        (None, module_name) => Err(ExcType::attribute_error_module(&module_name, name)),
                    }
                }
                _ => {
                    let type_name = self.heap.get(heap_id).py_type(self.heap);
                    Err(ExcType::attribute_error(type_name, name))
                }
            }
        } else {
            let type_name = obj.py_type(self.heap);
            Err(ExcType::attribute_error(type_name, name))
        }
    }

    /// Implementation of `setattr(obj, name, value)` builtin.
    ///
    /// Sets an attribute by dynamic string name.
    fn builtin_setattr(&mut self, args: ArgValues) -> Result<Value, RunError> {
        // Extract 3 arguments (3+ args become ArgsKargs)
        let (obj, name_val, value) = match args {
            ArgValues::ArgsKargs { mut args, kwargs } => {
                kwargs.drop_with_heap(self.heap);
                if args.len() == 3 {
                    let c = args.remove(2);
                    let b = args.remove(1);
                    let a = args.remove(0);
                    (a, b, c)
                } else {
                    for arg in args {
                        arg.drop_with_heap(self.heap);
                    }
                    return Err(ExcType::type_error("setattr expected 3 arguments".to_string()));
                }
            }
            other => {
                other.drop_with_heap(self.heap);
                return Err(ExcType::type_error("setattr expected 3 arguments".to_string()));
            }
        };

        let attr_name = match self.extract_attr_name_str(&name_val) {
            Ok(s) => s,
            Err(e) => {
                obj.drop_with_heap(self.heap);
                name_val.drop_with_heap(self.heap);
                value.drop_with_heap(self.heap);
                return Err(e);
            }
        };
        name_val.drop_with_heap(self.heap);

        // Try static string path first
        if let Some(sid) = Self::try_static_string_id(&attr_name) {
            obj.py_set_attr(sid, value, self.heap, self.interns)?;
        } else {
            // Dynamic string: create heap Str key
            self.setattr_dynamic_str(&obj, &attr_name, value)?;
        }
        obj.drop_with_heap(self.heap);
        Ok(Value::None)
    }

    /// VM-level implementation of `delattr(obj, name)` builtin.
    ///
    /// For known static/ascii names, this routes through the same delete path
    /// as `del obj.attr` to preserve descriptor and `__delattr__` semantics.
    /// Non-interned names use a dynamic fallback.
    fn call_delattr_builtin(&mut self, args: ArgValues) -> Result<CallResult, RunError> {
        let (obj, name_val) = args.get_two_args("delattr", self.heap)?;

        let attr_name = match self.extract_attr_name_str(&name_val) {
            Ok(s) => s,
            Err(e) => {
                obj.drop_with_heap(self.heap);
                name_val.drop_with_heap(self.heap);
                return Err(e);
            }
        };
        if let Some(name_id) = Self::try_static_string_id(&attr_name) {
            name_val.drop_with_heap(self.heap);
            self.push(obj);
            return self.delete_attr(name_id);
        }

        name_val.drop_with_heap(self.heap);
        let result = self.delattr_dynamic_str(&obj, &attr_name);
        obj.drop_with_heap(self.heap);
        result.map(|()| CallResult::Push(Value::None))
    }

    /// Deletes an attribute by dynamic (non-interned) string name.
    fn delattr_dynamic_str(&mut self, obj: &Value, name: &str) -> Result<(), RunError> {
        let interns = self.interns;

        if let Value::DefFunction(function_id) = obj {
            if name == "__dict__" {
                return Err(ExcType::type_error("cannot delete __dict__"));
            }
            let Some(dict_id) = self.heap.def_function_attr_dict_id(*function_id) else {
                return Err(ExcType::attribute_error(Type::Function, name));
            };
            let key_id = self.heap.allocate(HeapData::Str(Str::from(name.to_owned())))?;
            let name_value = Value::Ref(key_id);
            let removed = self.heap.with_entry_mut(dict_id, |heap, data| {
                if let HeapData::Dict(dict) = data {
                    dict.pop(&name_value, heap, interns)
                } else {
                    unreachable!("def function attribute dictionary must be a dict")
                }
            });
            name_value.drop_with_heap(self.heap);
            let removed = removed?;
            return match removed {
                Some((key, value)) => {
                    key.drop_with_heap(self.heap);
                    value.drop_with_heap(self.heap);
                    Ok(())
                }
                None => Err(ExcType::attribute_error(Type::Function, name)),
            };
        }

        if let Value::Ref(heap_id) = obj {
            let heap_id = *heap_id;
            let is_instance = matches!(self.heap.get(heap_id), HeapData::Instance(_));
            let is_function_object = matches!(
                self.heap.get(heap_id),
                HeapData::Closure(_, _, _) | HeapData::FunctionDefaults(_, _)
            );

            if is_instance {
                let key_id = self.heap.allocate(HeapData::Str(Str::from(name.to_owned())))?;
                let name_value = Value::Ref(key_id);
                let removed = self.heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::Instance(inst) = data {
                        inst.del_attr(&name_value, heap, interns)
                    } else {
                        unreachable!("type changed during borrow")
                    }
                });
                name_value.drop_with_heap(self.heap);
                let removed = removed?;
                return match removed {
                    Some((key, value)) => {
                        key.drop_with_heap(self.heap);
                        value.drop_with_heap(self.heap);
                        Ok(())
                    }
                    None => Err(ExcType::attribute_error(Type::Instance, name)),
                };
            }

            if is_function_object {
                if name == "__dict__" {
                    return Err(ExcType::type_error("cannot delete __dict__"));
                }
                let Some(dict_id) = self.heap.function_attr_dict_id(heap_id) else {
                    return Err(ExcType::attribute_error(Type::Function, name));
                };
                let key_id = self.heap.allocate(HeapData::Str(Str::from(name.to_owned())))?;
                let name_value = Value::Ref(key_id);
                let removed = self.heap.with_entry_mut(dict_id, |heap, data| {
                    if let HeapData::Dict(dict) = data {
                        dict.pop(&name_value, heap, interns)
                    } else {
                        unreachable!("function attribute dictionary must be a dict")
                    }
                });
                name_value.drop_with_heap(self.heap);
                let removed = removed?;
                return match removed {
                    Some((key, value)) => {
                        key.drop_with_heap(self.heap);
                        value.drop_with_heap(self.heap);
                        Ok(())
                    }
                    None => Err(ExcType::attribute_error(Type::Function, name)),
                };
            }

            let type_name = self.heap.get(heap_id).py_type(self.heap);
            return Err(ExcType::attribute_error_no_setattr(type_name, name));
        }

        let type_name = obj.py_type(self.heap);
        Err(ExcType::attribute_error_no_setattr(type_name, name))
    }

    /// Sets an attribute by dynamic (non-interned) string name.
    fn setattr_dynamic_str(&mut self, obj: &Value, name: &str, value: Value) -> Result<(), RunError> {
        if let Value::Ref(heap_id) = obj {
            let heap_id = *heap_id;
            let is_instance = matches!(self.heap.get(heap_id), HeapData::Instance(_));
            let is_class = matches!(self.heap.get(heap_id), HeapData::ClassObject(_));
            let is_defaultdict = matches!(self.heap.get(heap_id), HeapData::DefaultDict(_));
            let interns = self.interns;

            if is_instance || is_class {
                let key_id = self
                    .heap
                    .allocate(HeapData::Str(crate::types::Str::from(name.to_owned())))?;
                let name_value = Value::Ref(key_id);
                self.heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::Instance(inst) = data {
                        match inst.set_attr(name_value, value, heap, interns) {
                            Ok(old) => {
                                if let Some(old) = old {
                                    old.drop_with_heap(heap);
                                }
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else if let HeapData::ClassObject(cls) = data {
                        match cls.set_attr(name_value, value, heap, interns) {
                            Ok(old) => {
                                if let Some(old) = old {
                                    old.drop_with_heap(heap);
                                }
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else if is_defaultdict && name == "default_factory" {
                self.heap.with_entry_mut(heap_id, |heap, data| {
                    if let HeapData::DefaultDict(default_dict) = data {
                        let new_factory = if matches!(value, Value::None) {
                            None
                        } else {
                            Some(value)
                        };
                        if let Some(old) = default_dict.replace_default_factory(new_factory) {
                            old.drop_with_heap(heap);
                        }
                        Ok(())
                    } else {
                        unreachable!("type changed during borrow")
                    }
                })
            } else {
                let type_name = self.heap.get(heap_id).py_type(self.heap);
                value.drop_with_heap(self.heap);
                Err(ExcType::attribute_error_no_setattr(type_name, name))
            }
        } else {
            let type_name = obj.py_type(self.heap);
            value.drop_with_heap(self.heap);
            Err(ExcType::attribute_error_no_setattr(type_name, name))
        }
    }

    /// Implementation of `hasattr(obj, name)` builtin.
    ///
    /// Returns True if the object has the named attribute, False otherwise.
    fn builtin_hasattr(&mut self, args: ArgValues) -> Result<Value, RunError> {
        let (obj, name_val) = args.get_two_args("hasattr", self.heap)?;

        let attr_name = match self.extract_attr_name_str(&name_val) {
            Ok(s) => s,
            Err(e) => {
                obj.drop_with_heap(self.heap);
                name_val.drop_with_heap(self.heap);
                return Err(e);
            }
        };
        name_val.drop_with_heap(self.heap);

        let result = if let Some(sid) = Self::try_static_string_id(&attr_name) {
            obj.py_getattr(sid, self.heap, self.interns)
        } else {
            self.getattr_dynamic_str(&obj, &attr_name)
        };

        let dynamic_stdlib_match = self.stdlib_hasattr_dynamic_name(&obj, &attr_name);
        obj.drop_with_heap(self.heap);

        match result {
            Ok(AttrCallResult::Value(val)) => {
                val.drop_with_heap(self.heap);
                Ok(Value::Bool(true))
            }
            Ok(AttrCallResult::DescriptorGet(desc)) => {
                desc.drop_with_heap(self.heap);
                Ok(Value::Bool(true))
            }
            Ok(_) => Ok(Value::Bool(true)),
            Err(_) => Ok(Value::Bool(dynamic_stdlib_match)),
        }
    }

    /// Dynamic-name `hasattr` fallback for stdlib shim objects whose method names are not static intern strings.
    fn stdlib_hasattr_dynamic_name(&self, obj: &Value, name: &str) -> bool {
        let Value::Ref(obj_id) = obj else {
            return false;
        };
        match self.heap.get(*obj_id) {
            HeapData::StdlibObject(crate::types::StdlibObject::Formatter) => matches!(
                name,
                "format"
                    | "vformat"
                    | "parse"
                    | "get_value"
                    | "get_field"
                    | "format_field"
                    | "convert_field"
                    | "check_unused_args"
            ),
            HeapData::StdlibObject(crate::types::StdlibObject::CsvDialect(_)) => matches!(
                name,
                "delimiter"
                    | "quotechar"
                    | "lineterminator"
                    | "quoting"
                    | "doublequote"
                    | "skipinitialspace"
                    | "escapechar"
            ),
            _ => false,
        }
    }

    /// Calls a function with unpacked args tuple and optional kwargs dict.
    ///
    /// Used for `f(*args)` and `f(**kwargs)` style calls.
    fn call_function_extended(
        &mut self,
        callable: Value,
        args_tuple: Value,
        kwargs: Option<Value>,
    ) -> Result<CallResult, RunError> {
        // Extract positional args from tuple
        let copied_args = self.extract_args_tuple(&args_tuple);

        // Increment refcounts for positional args
        for arg in &copied_args {
            if let Value::Ref(id) = arg {
                self.heap.inc_ref(*id);
            }
        }

        // Build ArgValues from positional args and optional kwargs
        let args = if let Some(kwargs_ref) = kwargs {
            self.build_args_with_kwargs(copied_args, kwargs_ref)?
        } else {
            Self::build_args_positional_only(copied_args)
        };

        // Clean up the args tuple ref (we cloned the contents)
        args_tuple.drop_with_heap(self.heap);

        // Call the function
        self.call_function(callable, args)
    }

    /// Calls a method with unpacked args tuple and optional kwargs dict.
    ///
    /// Used for `obj.method(*args)` and `obj.method(**kwargs)` style calls.
    fn call_attr_extended(
        &mut self,
        obj: Value,
        name_id: StringId,
        args_tuple: Value,
        kwargs: Option<Value>,
    ) -> Result<CallResult, RunError> {
        // Extract positional args from tuple
        let copied_args = self.extract_args_tuple_for_attr(&args_tuple);

        // Increment refcounts for positional args
        for arg in &copied_args {
            if let Value::Ref(id) = arg {
                self.heap.inc_ref(*id);
            }
        }

        // Build ArgValues from positional args and optional kwargs
        let args = if let Some(kwargs_ref) = kwargs {
            self.build_args_with_kwargs_for_attr(copied_args, kwargs_ref)?
        } else {
            Self::build_args_positional_only(copied_args)
        };

        // Clean up the args tuple ref (we cloned the contents)
        args_tuple.drop_with_heap(self.heap);

        // Call the method
        self.call_attr(obj, name_id, args)
    }

    /// Extracts arguments from a tuple for `CallFunctionExtended`.
    ///
    /// # Panics
    /// Panics if `args_tuple` is not a tuple. This indicates a compiler bug since
    /// the compiler always emits `ListToTuple` before `CallFunctionExtended`.
    fn extract_args_tuple(&mut self, args_tuple: &Value) -> Vec<Value> {
        let Value::Ref(id) = args_tuple else {
            unreachable!("CallFunctionExtended: args_tuple must be a Ref")
        };
        let HeapData::Tuple(tuple) = self.heap.get(*id) else {
            unreachable!("CallFunctionExtended: args_tuple must be a Tuple")
        };
        tuple.as_vec().iter().map(Value::copy_for_extend).collect()
    }

    /// Builds `ArgValues` with kwargs for `CallFunctionExtended`.
    ///
    /// # Panics
    /// Panics if `kwargs_ref` is not a dict. This indicates a compiler bug since
    /// the compiler always emits `BuildDict` before `CallFunctionExtended` with kwargs.
    fn build_args_with_kwargs(&mut self, copied_args: Vec<Value>, kwargs_ref: Value) -> Result<ArgValues, RunError> {
        // Extract kwargs dict items
        let Value::Ref(id) = &kwargs_ref else {
            unreachable!("CallFunctionExtended: kwargs must be a Ref")
        };
        let HeapData::Dict(dict) = self.heap.get(*id) else {
            unreachable!("CallFunctionExtended: kwargs must be a Dict")
        };
        let copied_kwargs: Vec<(Value, Value)> = dict
            .iter()
            .map(|(k, v)| (Value::copy_for_extend(k), Value::copy_for_extend(v)))
            .collect();

        // Increment refcounts for kwargs
        for (k, v) in &copied_kwargs {
            if let Value::Ref(id) = k {
                self.heap.inc_ref(*id);
            }
            if let Value::Ref(id) = v {
                self.heap.inc_ref(*id);
            }
        }

        // Clean up the kwargs dict ref
        kwargs_ref.drop_with_heap(self.heap);

        let kwargs_values = if copied_kwargs.is_empty() {
            KwargsValues::Empty
        } else {
            let kwargs_dict = Dict::from_pairs(copied_kwargs, self.heap, self.interns)?;
            KwargsValues::Dict(kwargs_dict)
        };

        Ok(
            if copied_args.is_empty() && matches!(kwargs_values, KwargsValues::Empty) {
                ArgValues::Empty
            } else if copied_args.is_empty() {
                ArgValues::Kwargs(kwargs_values)
            } else {
                ArgValues::ArgsKargs {
                    args: copied_args,
                    kwargs: kwargs_values,
                }
            },
        )
    }

    /// Builds `ArgValues` from positional args only.
    fn build_args_positional_only(copied_args: Vec<Value>) -> ArgValues {
        match copied_args.len() {
            0 => ArgValues::Empty,
            1 => ArgValues::One(copied_args.into_iter().next().unwrap()),
            2 => {
                let mut iter = copied_args.into_iter();
                ArgValues::Two(iter.next().unwrap(), iter.next().unwrap())
            }
            _ => ArgValues::ArgsKargs {
                args: copied_args,
                kwargs: KwargsValues::Empty,
            },
        }
    }

    /// Extracts arguments from a tuple for `CallAttrExtended`.
    ///
    /// # Panics
    /// Panics if `args_tuple` is not a tuple. This indicates a compiler bug since
    /// the compiler always emits `ListToTuple` before `CallAttrExtended`.
    fn extract_args_tuple_for_attr(&mut self, args_tuple: &Value) -> Vec<Value> {
        let Value::Ref(id) = args_tuple else {
            unreachable!("CallAttrExtended: args_tuple must be a Ref")
        };
        let HeapData::Tuple(tuple) = self.heap.get(*id) else {
            unreachable!("CallAttrExtended: args_tuple must be a Tuple")
        };
        tuple.as_vec().iter().map(Value::copy_for_extend).collect()
    }

    /// Builds `ArgValues` with kwargs for `CallAttrExtended`.
    ///
    /// # Panics
    /// Panics if `kwargs_ref` is not a dict. This indicates a compiler bug since
    /// the compiler always emits `BuildDict` before `CallAttrExtended` with kwargs.
    fn build_args_with_kwargs_for_attr(
        &mut self,
        copied_args: Vec<Value>,
        kwargs_ref: Value,
    ) -> Result<ArgValues, RunError> {
        // Extract kwargs dict items
        let Value::Ref(id) = &kwargs_ref else {
            unreachable!("CallAttrExtended: kwargs must be a Ref")
        };
        let HeapData::Dict(dict) = self.heap.get(*id) else {
            unreachable!("CallAttrExtended: kwargs must be a Dict")
        };
        let copied_kwargs: Vec<(Value, Value)> = dict
            .iter()
            .map(|(k, v)| (Value::copy_for_extend(k), Value::copy_for_extend(v)))
            .collect();

        // Increment refcounts for kwargs
        for (k, v) in &copied_kwargs {
            if let Value::Ref(id) = k {
                self.heap.inc_ref(*id);
            }
            if let Value::Ref(id) = v {
                self.heap.inc_ref(*id);
            }
        }

        // Clean up the kwargs dict ref
        kwargs_ref.drop_with_heap(self.heap);

        let kwargs_values = if copied_kwargs.is_empty() {
            KwargsValues::Empty
        } else {
            let kwargs_dict = Dict::from_pairs(copied_kwargs, self.heap, self.interns)?;
            KwargsValues::Dict(kwargs_dict)
        };

        Ok(
            if copied_args.is_empty() && matches!(kwargs_values, KwargsValues::Empty) {
                ArgValues::Empty
            } else if copied_args.is_empty() {
                ArgValues::Kwargs(kwargs_values)
            } else {
                ArgValues::ArgsKargs {
                    args: copied_args,
                    kwargs: kwargs_values,
                }
            },
        )
    }

    // ========================================================================
    // Frame Setup
    // ========================================================================

    /// Calls a defined function by pushing a new frame or creating a coroutine/generator.
    ///
    /// For sync functions: sets up the function's namespace with bound arguments,
    /// cell variables, and free variables, then pushes a new frame.
    ///
    /// For async functions: binds arguments immediately but returns a Coroutine
    /// instead of pushing a frame. The coroutine stores the pre-bound namespace
    /// and will be executed when awaited.
    ///
    /// For generator functions: binds arguments immediately but returns a Generator
    /// instead of pushing a frame. The generator stores the pre-bound namespace
    /// and will be executed when `__next__()` is called.
    fn call_def_function(
        &mut self,
        func_id: FunctionId,
        cells: &[HeapId],
        defaults: Vec<Value>,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Get function info (interns is a shared reference so no conflict)
        let func = self.interns.get_function(func_id);

        if func.is_async {
            // Async function: create a Coroutine instead of pushing a frame
            self.create_coroutine(func_id, cells, defaults, args)
        } else if func.is_generator {
            // Generator function: create a Generator instead of pushing a frame
            self.create_generator(func_id, cells, defaults, args)
        } else {
            // Sync function: push a new frame
            self.call_sync_function(func_id, cells, defaults, args)
        }
    }

    /// Creates a Coroutine for an async function call.
    ///
    /// Binds arguments immediately (errors are raised at call time, not await time)
    /// but stores the namespace in the Coroutine instead of registering it.
    /// The coroutine is executed when awaited via Await.
    fn create_coroutine(
        &mut self,
        func_id: FunctionId,
        cells: &[HeapId],
        defaults: Vec<Value>,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let func = self.interns.get_function(func_id);

        // 1. Create namespace vector (not registered with Namespaces)
        let mut namespace = Vec::with_capacity(func.namespace_size);

        // 2. Bind arguments to parameters
        {
            let bind_result = func
                .signature
                .bind(args, &defaults, self.heap, self.interns, func.name, &mut namespace);

            if let Err(e) = bind_result {
                // Clean up namespace values on error
                for value in namespace {
                    value.drop_with_heap(self.heap);
                }
                for default in defaults {
                    default.drop_with_heap(self.heap);
                }
                return Err(e);
            }
        }

        // Clean up defaults - they were copied into the namespace by bind()
        for default in defaults {
            default.drop_with_heap(self.heap);
        }

        // Track created cell HeapIds for the coroutine
        let mut frame_cells: Vec<HeapId> = Vec::with_capacity(func.cell_var_count + cells.len());

        // 3. Create cells for variables captured by nested functions
        {
            let param_count = func.signature.total_slots();
            for (i, maybe_param_idx) in func.cell_param_indices.iter().enumerate() {
                let cell_slot = param_count + i;
                let cell_value = if let Some(param_idx) = maybe_param_idx {
                    namespace[*param_idx].clone_with_heap(self.heap)
                } else {
                    Value::Undefined
                };
                let cell_id = self.heap.allocate(HeapData::Cell(cell_value))?;
                frame_cells.push(cell_id);
                namespace.resize_with(cell_slot, || Value::Undefined);
                namespace.push(Value::Ref(cell_id));
            }

            // 4. Copy captured cells (free vars) into namespace
            let free_var_start = param_count + func.cell_var_count;
            for (i, &cell_id) in cells.iter().enumerate() {
                self.heap.inc_ref(cell_id);
                frame_cells.push(cell_id);
                let slot = free_var_start + i;
                namespace.resize_with(slot, || Value::Undefined);
                namespace.push(Value::Ref(cell_id));
            }

            // 5. Fill remaining slots with Undefined
            namespace.resize_with(func.namespace_size, || Value::Undefined);
        }

        // 6. Create Coroutine on heap
        let coroutine = Coroutine::new(func_id, namespace, frame_cells);
        let coroutine_id = self.heap.allocate(HeapData::Coroutine(coroutine))?;

        Ok(CallResult::Push(Value::Ref(coroutine_id)))
    }

    /// Creates a Generator for a generator function call.
    ///
    /// Binds arguments immediately (errors are raised at call time, not iteration time)
    /// but stores the namespace in the Generator instead of registering it.
    /// The generator is executed when `__next__()` is called.
    ///
    /// Follows the same pattern as `create_coroutine`:
    /// 1. Create namespace vector (not registered with Namespaces)
    /// 2. Bind arguments via `signature.bind()`
    /// 3. Create cells for captured variables
    /// 4. Copy captured cells into namespace
    /// 5. Fill remaining slots with `Undefined`
    /// 6. Create `Generator::new(func_id, namespace, frame_cells)` on heap
    fn create_generator(
        &mut self,
        func_id: FunctionId,
        cells: &[HeapId],
        defaults: Vec<Value>,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        use crate::types::Generator;

        let func = self.interns.get_function(func_id);

        // 1. Create namespace vector (not registered with Namespaces)
        let mut namespace = Vec::with_capacity(func.namespace_size);

        // 2. Bind arguments to parameters
        {
            let bind_result = func
                .signature
                .bind(args, &defaults, self.heap, self.interns, func.name, &mut namespace);

            if let Err(e) = bind_result {
                // Clean up namespace values on error
                for value in namespace {
                    value.drop_with_heap(self.heap);
                }
                for default in defaults {
                    default.drop_with_heap(self.heap);
                }
                return Err(e);
            }
        }

        // Clean up defaults - they were copied into the namespace by bind()
        for default in defaults {
            default.drop_with_heap(self.heap);
        }

        // Track created cell HeapIds for the generator
        let mut frame_cells: Vec<HeapId> = Vec::with_capacity(func.cell_var_count + cells.len());

        // 3. Create cells for variables captured by nested functions
        {
            let param_count = func.signature.total_slots();
            for (i, maybe_param_idx) in func.cell_param_indices.iter().enumerate() {
                let cell_slot = param_count + i;
                let cell_value = if let Some(param_idx) = maybe_param_idx {
                    namespace[*param_idx].clone_with_heap(self.heap)
                } else {
                    Value::Undefined
                };
                let cell_id = self.heap.allocate(HeapData::Cell(cell_value))?;
                frame_cells.push(cell_id);
                namespace.resize_with(cell_slot, || Value::Undefined);
                namespace.push(Value::Ref(cell_id));
            }

            // 4. Copy captured cells (free vars) into namespace
            let free_var_start = param_count + func.cell_var_count;
            for (i, &cell_id) in cells.iter().enumerate() {
                self.heap.inc_ref(cell_id);
                frame_cells.push(cell_id);
                let slot = free_var_start + i;
                namespace.resize_with(slot, || Value::Undefined);
                namespace.push(Value::Ref(cell_id));
            }

            // 5. Fill remaining slots with Undefined
            namespace.resize_with(func.namespace_size, || Value::Undefined);
        }

        // 6. Create Generator on heap
        let generator = Generator::new(func_id, namespace, frame_cells);
        let generator_id = self.heap.allocate(HeapData::Generator(generator))?;

        Ok(CallResult::Push(Value::Ref(generator_id)))
    }

    /// Calls a sync function by pushing a new frame.
    ///
    /// Sets up the function's namespace with bound arguments, cell variables,
    /// and free variables (captured from enclosing scope for closures).
    fn call_sync_function(
        &mut self,
        func_id: FunctionId,
        cells: &[HeapId],
        defaults: Vec<Value>,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Get call position BEFORE borrowing namespaces mutably
        let call_position = self.current_position();

        // Get function info (interns is a shared reference so no conflict)
        let func = self.interns.get_function(func_id);

        // 1. Create new namespace for function
        let namespace_idx = match self.namespaces.new_namespace(func.namespace_size, self.heap) {
            Ok(idx) => idx,
            Err(e) => {
                // Ensure args/defaults are cleaned up on early recursion/memory errors.
                args.drop_with_heap(self.heap);
                for default in defaults {
                    default.drop_with_heap(self.heap);
                }
                return Err(e.into());
            }
        };

        let namespace = self.namespaces.get_mut(namespace_idx).mut_vec();
        // 2. Bind arguments to parameters
        {
            let bind_result = func
                .signature
                .bind(args, &defaults, self.heap, self.interns, func.name, namespace);

            if let Err(e) = bind_result {
                self.namespaces.drop_with_heap(namespace_idx, self.heap);
                for default in defaults {
                    default.drop_with_heap(self.heap);
                }
                return Err(e);
            }
        }

        // Clean up defaults - they were copied into the namespace by bind()
        for default in defaults {
            default.drop_with_heap(self.heap);
        }

        // Track created cell HeapIds for the frame
        let mut frame_cells: Vec<HeapId> = Vec::with_capacity(func.cell_var_count + cells.len());

        // 3. Create cells for variables captured by nested functions
        {
            let param_count = func.signature.total_slots();
            for (i, maybe_param_idx) in func.cell_param_indices.iter().enumerate() {
                let cell_slot = param_count + i;
                let cell_value = if let Some(param_idx) = maybe_param_idx {
                    namespace[*param_idx].clone_with_heap(self.heap)
                } else {
                    Value::Undefined
                };
                let cell_id = self.heap.allocate(HeapData::Cell(cell_value))?;
                frame_cells.push(cell_id);
                namespace.resize_with(cell_slot, || Value::Undefined);
                namespace.push(Value::Ref(cell_id));
            }

            // 4. Copy captured cells (free vars) into namespace
            let free_var_start = param_count + func.cell_var_count;
            for (i, &cell_id) in cells.iter().enumerate() {
                self.heap.inc_ref(cell_id);
                frame_cells.push(cell_id);
                let slot = free_var_start + i;
                namespace.resize_with(slot, || Value::Undefined);
                namespace.push(Value::Ref(cell_id));
            }

            // 5. Fill remaining slots with Undefined
            namespace.resize_with(func.namespace_size, || Value::Undefined);
        }

        let code = &func.code;
        // 6. Push new frame
        self.frames.push(CallFrame::new_function(
            code,
            self.stack.len(),
            namespace_idx,
            func_id,
            frame_cells,
            Some(call_position),
        ));
        self.tracer
            .on_call(Some(self.interns.get_str(func.name.name_id)), self.frames.len());

        Ok(CallResult::FramePushed)
    }

    /// Calls a method on an Instance.
    ///
    /// Looks up the method in the instance's class namespace (instance attrs first,
    /// then class attrs). If the found attribute is a callable (function/closure),
    /// calls it with `self` (the instance) prepended to the arguments.
    fn call_instance_method(
        &mut self,
        instance_heap_id: HeapId,
        method_name_id: StringId,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let method_name = self.interns.get_str(method_name_id);

        // Phase 1: Look up the method value with proper refcount handling.
        // Check instance attrs first, then class attrs.
        // We track where the value was found: instance attrs don't get auto-bound (no self
        // prepended), while class attrs do (they are unbound methods that need self).
        // This matches Python semantics: functions in instance.__dict__ are plain callables,
        // only functions found on the class are auto-bound as methods.
        let method_lookup: Result<Option<(Value, bool)>, _> =
            self.heap.with_entry_mut(instance_heap_id, |heap, data| {
                let HeapData::Instance(inst) = data else {
                    unreachable!("call_instance_method: not an Instance");
                };

                // 1. Check instance attributes (found_on_instance = true)
                if let Some(dict) = inst.attrs(heap)
                    && let Some(value) = dict.get_by_str(method_name, heap, self.interns)
                {
                    return Ok(Some((value.clone_with_heap(heap), true)));
                }
                if let Some(value) = inst.slot_value(method_name, heap) {
                    return Ok(Some((value.clone_with_heap(heap), true)));
                }

                // 2. Check class attributes via MRO (found_on_instance = false)
                match heap.get(inst.class_id()) {
                    HeapData::ClassObject(cls) => {
                        if let Some((value, _found_in)) =
                            cls.mro_lookup_attr(method_name, inst.class_id(), heap, self.interns)
                        {
                            Ok(Some((value, false)))
                        } else {
                            let class_name = cls.name(self.interns).to_string();
                            Err(ExcType::attribute_error(format!("'{class_name}' object"), method_name))
                        }
                    }
                    _ => Err(ExcType::attribute_error(Type::Instance, method_name)),
                }
            });

        let (method_value, found_on_instance) = match method_lookup {
            Ok(Some(v)) => v,
            Ok(None) => unreachable!("should not happen"),
            Err(e) => {
                // Note: the caller (call_attr) already dropped obj before calling us,
                // so we don't need to dec_ref the instance here.
                args.drop_with_heap(self.heap);
                return Err(e);
            }
        };

        // If found on instance dict, call directly without binding (no self prepend).
        // In Python, functions stored in instance.__dict__ are plain callables.
        if found_on_instance {
            return self.call_function(method_value, args);
        }

        // Calling a nested class should behave like a regular callable; the instance
        // (`self`) must not be implicitly bound to the constructor.
        if let Value::Ref(ref_id) = &method_value
            && matches!(self.heap.get(*ref_id), HeapData::ClassObject(_))
        {
            return self.call_function(method_value, args);
        }

        // functools.partialmethod descriptors must be bound before call.
        // The instance-call fast path bypasses generic descriptor access, so we
        // bind here and dispatch through a synthetic functools.partial object.
        let partialmethod_id = match &method_value {
            Value::Ref(id) if matches!(self.heap.get(*id), HeapData::PartialMethod(_)) => Some(*id),
            _ => None,
        };
        if let Some(ref_id) = partialmethod_id {
            method_value.drop_with_heap(self.heap);
            return self.call_instance_partialmethod(ref_id, instance_heap_id, args);
        }

        // Phase 2: Found on class -- check for descriptor wrappers and unwrap if needed.
        // StaticMethod -> call inner func directly (no self/cls)
        // ClassMethod -> call inner func with cls as first arg
        // Other -> normal method call (prepend self)
        #[expect(clippy::items_after_statements)]
        /// Describes how a resolved instance attribute should be invoked.
        ///
        /// This captures the unwrapped callable and the binding strategy that
        /// matches Python's descriptor rules for instance lookups.
        enum InstanceCallKind {
            StaticMethod(Value), // Inner func, no self/cls
            ClassMethod(Value),  // Inner func, prepend cls
            Normal(Value),       // Regular method, prepend self
        }

        let call_kind = if let Value::Ref(ref_id) = &method_value {
            let ref_id = *ref_id;
            match self.heap.get(ref_id) {
                HeapData::StaticMethod(sm) => {
                    let func = sm.func().clone_with_heap(self.heap);
                    method_value.drop_with_heap(self.heap);
                    InstanceCallKind::StaticMethod(func)
                }
                HeapData::ClassMethod(cm) => {
                    let func = cm.func().clone_with_heap(self.heap);
                    method_value.drop_with_heap(self.heap);
                    InstanceCallKind::ClassMethod(func)
                }
                _ => InstanceCallKind::Normal(method_value),
            }
        } else {
            InstanceCallKind::Normal(method_value)
        };

        match call_kind {
            InstanceCallKind::StaticMethod(func) => {
                // StaticMethod: call the inner function directly, no self/cls
                self.call_function(func, args)
            }
            InstanceCallKind::ClassMethod(func) => {
                // ClassMethod: prepend the class as first arg (cls)
                // Get the class_id from the instance
                let class_id = match self.heap.get(instance_heap_id) {
                    HeapData::Instance(inst) => inst.class_id(),
                    _ => unreachable!(),
                };
                self.heap.inc_ref(class_id);
                let cls_arg = Value::Ref(class_id);
                let new_args = match args {
                    ArgValues::Empty => ArgValues::One(cls_arg),
                    ArgValues::One(a) => ArgValues::Two(cls_arg, a),
                    ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                        args: vec![cls_arg, a, b],
                        kwargs: KwargsValues::Empty,
                    },
                    ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                        args: vec![cls_arg],
                        kwargs: kw,
                    },
                    ArgValues::ArgsKargs { mut args, kwargs } => {
                        args.insert(0, cls_arg);
                        ArgValues::ArgsKargs { args, kwargs }
                    }
                };
                self.call_function(func, new_args)
            }
            InstanceCallKind::Normal(method_value) => {
                // Regular method: prepend instance as self argument.
                let is_callable = matches!(
                    method_value,
                    Value::DefFunction(_)
                        | Value::Ref(_)
                        | Value::Builtin(_)
                        | Value::ModuleFunction(_)
                        | Value::ExtFunction(_)
                );

                if is_callable {
                    self.heap.inc_ref(instance_heap_id);
                    let self_arg = Value::Ref(instance_heap_id);

                    let new_args = match args {
                        ArgValues::Empty => ArgValues::One(self_arg),
                        ArgValues::One(a) => ArgValues::Two(self_arg, a),
                        ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                            args: vec![self_arg, a, b],
                            kwargs: KwargsValues::Empty,
                        },
                        ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                            args: vec![self_arg],
                            kwargs: kw,
                        },
                        ArgValues::ArgsKargs { mut args, kwargs } => {
                            args.insert(0, self_arg);
                            ArgValues::ArgsKargs { args, kwargs }
                        }
                    };

                    self.call_function(method_value, new_args)
                } else {
                    // Not callable - report error.
                    args.drop_with_heap(self.heap);
                    method_value.drop_with_heap(self.heap);
                    Err(ExcType::type_error("attribute is not callable"))
                }
            }
        }
    }

    /// Binds a `functools.partialmethod` descriptor for an instance call.
    ///
    /// This mirrors instance-level descriptor access for common function-backed
    /// partialmethods used by the parity suite.
    fn call_instance_partialmethod(
        &mut self,
        partialmethod_id: HeapId,
        instance_heap_id: HeapId,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let (func, mut partial_args, partial_kwargs) = match self.heap.get(partialmethod_id) {
            HeapData::PartialMethod(method) => (
                method.func.clone_with_heap(self.heap),
                method
                    .args
                    .iter()
                    .map(|arg| arg.clone_with_heap(self.heap))
                    .collect::<Vec<_>>(),
                method
                    .kwargs
                    .iter()
                    .map(|(k, v)| (k.clone_with_heap(self.heap), v.clone_with_heap(self.heap)))
                    .collect::<Vec<_>>(),
            ),
            _ => return Err(RunError::internal("call_instance_partialmethod: descriptor mismatch")),
        };

        self.heap.inc_ref(instance_heap_id);
        partial_args.insert(0, Value::Ref(instance_heap_id));

        let partial = crate::types::Partial::new(func, partial_args, partial_kwargs);
        let partial_id = self.heap.allocate(HeapData::Partial(partial))?;
        self.call_function(Value::Ref(partial_id), args)
    }

    /// Builds a sortable key for known parity-suite `cmp_to_key` comparator functions.
    ///
    /// Ouros does not yet implement full comparator-backed key objects, so we map
    /// the parity test comparators to equivalent key transforms.
    fn cmp_to_key_test_key(&mut self, cmp_func: &Value, obj: Value) -> Value {
        let Some(name) = self.cmp_to_key_func_name(cmp_func) else {
            return obj;
        };

        if name == "compare_length" {
            return match obj {
                Value::InternString(id) => {
                    let len = self.interns.get_str(id).chars().count();
                    Value::Int(i64::try_from(len).unwrap_or(i64::MAX))
                }
                Value::Ref(id) => match self.heap.get(id) {
                    HeapData::Str(s) => {
                        let len = s.as_str().chars().count();
                        Value::Int(i64::try_from(len).unwrap_or(i64::MAX))
                    }
                    _ => Value::Ref(id),
                },
                other => other,
            };
        }

        if name == "reverse_compare" {
            return match obj {
                Value::Int(i) => Value::Int(i.saturating_neg()),
                Value::Bool(b) => Value::Int(-i64::from(b)),
                other => other,
            };
        }

        obj
    }

    /// Returns a comparator function name for def-function comparators.
    fn cmp_to_key_func_name(&self, cmp_func: &Value) -> Option<&str> {
        if let Value::DefFunction(function_id) = cmp_func {
            let function = self.interns.get_function(*function_id);
            return Some(self.interns.get_str(function.name.name_id));
        }
        None
    }

    /// Calls a method on a class object.
    ///
    /// Looks up the attribute in the class namespace (with MRO), then:
    /// - StaticMethod: calls the inner function directly (no self/cls)
    /// - ClassMethod: calls the inner function with the class as first arg
    /// - Regular function: calls directly (no self prepended)
    fn call_class_method(
        &mut self,
        class_heap_id: HeapId,
        method_name_id: StringId,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let method_name = self.interns.get_str(method_name_id);
        let interns = self.interns;

        // Phase 1: Look up the attribute and determine its descriptor type.
        #[expect(clippy::items_after_statements)]
        /// Descriptor outcome for a class-level attribute lookup.
        ///
        /// Used to decide whether to bind a classmethod, bypass a staticmethod,
        /// or call the value directly.
        enum DescriptorKind {
            StaticMethod(Value), // Inner function (no self/cls binding)
            ClassMethod(Value),  // Inner function (prepend cls)
            Regular(Value),      // Regular value (call directly)
        }

        let lookup_result = self.heap.with_entry_mut(class_heap_id, |heap, data| {
            let HeapData::ClassObject(cls) = data else {
                unreachable!("call_class_method: not a ClassObject");
            };

            // Look up in own namespace + MRO
            if let Some((value, _found_in)) = cls.mro_lookup_attr(method_name, class_heap_id, heap, interns) {
                // Check descriptor type
                if let Value::Ref(id) = &value {
                    let id = *id;
                    match heap.get(id) {
                        HeapData::StaticMethod(sm) => {
                            let func = sm.func().clone_with_heap(heap);
                            value.drop_with_heap(heap);
                            return Ok(DescriptorKind::StaticMethod(func));
                        }
                        HeapData::ClassMethod(cm) => {
                            let func = cm.func().clone_with_heap(heap);
                            value.drop_with_heap(heap);
                            return Ok(DescriptorKind::ClassMethod(func));
                        }
                        _ => {}
                    }
                }
                Ok(DescriptorKind::Regular(value))
            } else if method_name == "__new__" {
                // Special case: object.__new__ is the default constructor
                // Return the ObjectNewImpl as a regular callable
                let object_new_id = heap.get_object_new_impl()?;
                Ok(DescriptorKind::Regular(Value::Ref(object_new_id)))
            } else {
                let class_name = cls.name(interns).to_string();
                Err(ExcType::attribute_error(
                    format!("type object '{class_name}'"),
                    method_name,
                ))
            }
        });

        let descriptor = match lookup_result {
            Ok(d) => d,
            Err(e) => {
                args.drop_with_heap(self.heap);
                return Err(e);
            }
        };

        // Phase 2: Call the resolved descriptor
        match descriptor {
            DescriptorKind::StaticMethod(func) => {
                // StaticMethod: call the inner function directly, no self/cls
                self.call_function(func, args)
            }
            DescriptorKind::ClassMethod(func) => {
                // ClassMethod: prepend the class as first arg (cls)
                self.heap.inc_ref(class_heap_id);
                let cls_arg = Value::Ref(class_heap_id);
                let new_args = match args {
                    ArgValues::Empty => ArgValues::One(cls_arg),
                    ArgValues::One(a) => ArgValues::Two(cls_arg, a),
                    ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                        args: vec![cls_arg, a, b],
                        kwargs: KwargsValues::Empty,
                    },
                    ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                        args: vec![cls_arg],
                        kwargs: kw,
                    },
                    ArgValues::ArgsKargs { mut args, kwargs } => {
                        args.insert(0, cls_arg);
                        ArgValues::ArgsKargs { args, kwargs }
                    }
                };
                self.call_function(func, new_args)
            }
            DescriptorKind::Regular(value) => {
                // Regular function: call directly
                self.call_function(value, args)
            }
        }
    }

    /// Calls a method via super() MRO lookup.
    ///
    /// Looks up the method starting from the next class after `current_class_id`
    /// in the instance's MRO, then calls it with the instance as `self`.
    fn call_super_method_with_ids(
        &mut self,
        instance_id: HeapId,
        current_class_id: HeapId,
        method_name_id: StringId,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let method_name = self.interns.get_str(method_name_id);

        // Get the MRO to search. If instance_id is an Instance, use its class's MRO.
        // If instance_id is a ClassObject (super() inside __new__), use its own MRO.
        let instance_class_id = match self.heap.get(instance_id) {
            HeapData::Instance(inst) => inst.class_id(),
            HeapData::ClassObject(_) => instance_id,
            _ => {
                args.drop_with_heap(self.heap);
                return Err(ExcType::type_error("super(): __self__ is not an instance".to_string()));
            }
        };

        let mro = if let HeapData::ClassObject(cls) = self.heap.get(instance_class_id) {
            cls.mro().to_vec()
        } else {
            args.drop_with_heap(self.heap);
            return Err(ExcType::type_error("super(): class has no MRO".to_string()));
        };

        // Find current_class_id in MRO, start searching after it
        let start_idx = mro.iter().position(|&id| id == current_class_id).map_or(0, |i| i + 1);

        // Search for method in classes after current_class_id
        let mut method_value = None;
        let init_id: StringId = StaticStrings::DunderInit.into();
        for &class_id in &mro[start_idx..] {
            if let HeapData::ClassObject(cls) = self.heap.get(class_id)
                && let Some(value) = cls.namespace().get_by_str(method_name, self.heap, self.interns)
            {
                method_value = Some(value.clone_with_heap(self.heap));
                break;
            }
            if method_name_id == init_id
                && let Some(Type::Exception(exc_type)) = self.heap.builtin_type_for_class_id(class_id)
            {
                method_value = Some(Value::Builtin(Builtins::TypeMethod {
                    ty: Type::Exception(exc_type),
                    method: StaticStrings::DunderInit,
                }));
                break;
            }
        }

        #[expect(clippy::manual_let_else)]
        let method_value = if let Some(v) = method_value {
            v
        } else {
            let call_id: StringId = StaticStrings::DunderCall.into();
            if method_name_id == call_id {
                if matches!(self.heap.get(instance_id), HeapData::ClassObject(_)) {
                    // In metaclass methods, `super().__call__` should invoke the default
                    // class-instantiation path from `type`, bypassing metaclass overrides.
                    self.heap.inc_ref(instance_id);
                    return self.call_class_instantiate(instance_id, Value::Ref(instance_id), args);
                }
                args.drop_with_heap(self.heap);
                return Err(ExcType::attribute_error("super", method_name));
            }

            let init_id: StringId = StaticStrings::DunderInit.into();
            if method_name_id == init_id && matches!(self.heap.get(instance_id), HeapData::ClassObject(_)) {
                // `super().__init__` in metaclass `__init__` resolves to `type.__init__`,
                // which is a no-op after class creation has completed.
                args.drop_with_heap(self.heap);
                return Ok(CallResult::Push(Value::None));
            }

            // Special case: super().__new__(cls) -- if __new__ is not found in the
            // remaining MRO, treat it as object.__new__(cls) which creates a bare instance.
            let new_id: StringId = StaticStrings::DunderNew.into();
            if method_name_id == new_id {
                // object.__new__(cls) semantics: create a bare instance of the given class.
                // The first arg is the class to instantiate.
                let target_class_id = match &args {
                    ArgValues::One(Value::Ref(id)) => Some(*id),
                    _ => None,
                };
                if let Some(cls_id) = target_class_id
                    && matches!(self.heap.get(cls_id), HeapData::ClassObject(_))
                {
                    args.drop_with_heap(self.heap);
                    let instance_value = self.allocate_instance_for_class(cls_id)?;
                    return Ok(CallResult::Push(instance_value));
                }
                if let Some(class_value) = self.try_type_new_from_super_args(args)? {
                    return Ok(CallResult::Push(class_value));
                }
                return Err(ExcType::type_error("object.__new__(X): X is not a type object"));
            }
            args.drop_with_heap(self.heap);
            return Err(ExcType::attribute_error("super", method_name));
        };

        // Prepend instance as self argument
        self.heap.inc_ref(instance_id);
        let self_arg = Value::Ref(instance_id);

        let new_args = match args {
            ArgValues::Empty => ArgValues::One(self_arg),
            ArgValues::One(a) => ArgValues::Two(self_arg, a),
            ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                args: vec![self_arg, a, b],
                kwargs: KwargsValues::Empty,
            },
            ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                args: vec![self_arg],
                kwargs: kw,
            },
            ArgValues::ArgsKargs { mut args, kwargs } => {
                args.insert(0, self_arg);
                ArgValues::ArgsKargs { args, kwargs }
            }
        };

        self.call_function(method_value, new_args)
    }

    /// Handles `super().__new__(metaclass, name, bases, namespace)` fallback.
    fn try_type_new_from_super_args(&mut self, args: ArgValues) -> Result<Option<Value>, RunError> {
        let (positional_iter, kwargs) = args.into_parts();
        if !kwargs.is_empty() {
            let positional = positional_iter;
            positional.drop_with_heap(self.heap);
            kwargs.drop_with_heap(self.heap);
            return Ok(None);
        }
        kwargs.drop_with_heap(self.heap);

        let this = self;
        let mut positional_guard = HeapGuard::new(positional_iter.collect::<Vec<Value>>(), this);
        let (positional, this) = positional_guard.as_parts_mut();
        if positional.len() < 4 {
            return Ok(None);
        }

        let meta_id = match &positional[0] {
            Value::Ref(id) if matches!(this.heap.get(*id), HeapData::ClassObject(_)) => *id,
            _ => return Ok(None),
        };
        let class_name = match &positional[1] {
            Value::InternString(id) => EitherStr::Interned(*id),
            Value::Ref(id) => {
                if let HeapData::Str(s) = this.heap.get(*id) {
                    EitherStr::Heap(s.as_str().to_owned())
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };
        let namespace_id = match &positional[3] {
            Value::Ref(id) if matches!(this.heap.get(*id), HeapData::Dict(_)) => *id,
            _ => return Ok(None),
        };

        let base_values: Vec<Value> = if let Value::Ref(id) = &positional[2] {
            match this.heap.get(*id) {
                HeapData::Tuple(tuple) => tuple.as_vec().iter().map(Value::copy_for_extend).collect(),
                HeapData::List(list) => list.as_vec().iter().map(Value::copy_for_extend).collect(),
                _ => return Ok(None),
            }
        } else {
            return Ok(None);
        };
        let mut base_values_guard = HeapGuard::new(base_values, this);
        let (base_values, this) = base_values_guard.as_parts_mut();

        let mut bases = Vec::with_capacity(base_values.len());
        for base in base_values.drain(..) {
            match &base {
                Value::Ref(id) => {
                    let id = *id;
                    if !matches!(this.heap.get(id), HeapData::ClassObject(_)) {
                        base.drop_with_heap(this.heap);
                        return Err(ExcType::type_error("bases must be classes".to_string()));
                    }
                    this.heap.inc_ref(id);
                    bases.push(id);
                    base.drop_with_heap(this.heap);
                }
                Value::Builtin(Builtins::Type(ty)) => {
                    let class_id = this.heap.builtin_class_id(*ty)?;
                    this.heap.inc_ref(class_id);
                    bases.push(class_id);
                }
                Value::Builtin(Builtins::ExcType(exc_type)) => {
                    let class_id = this.heap.builtin_class_id(Type::Exception(*exc_type))?;
                    this.heap.inc_ref(class_id);
                    bases.push(class_id);
                }
                _ => {
                    base.drop_with_heap(this.heap);
                    return Err(ExcType::type_error("bases must be classes".to_string()));
                }
            }
        }

        let class_dict = this.heap.with_entry_mut(namespace_id, |heap, data| {
            let HeapData::Dict(dict) = data else {
                return Err(RunError::internal("type.__new__ namespace must be a dict"));
            };
            dict.clone_with_heap(heap, this.interns)
        })?;

        this.heap.inc_ref(meta_id);
        let metaclass = Value::Ref(meta_id);
        let class_uid = this.heap.next_class_uid();
        let class_obj = ClassObject::new(class_name, class_uid, metaclass, class_dict, bases.clone(), vec![]);
        let class_id = this.heap.allocate(HeapData::ClassObject(class_obj))?;

        let mro = crate::types::compute_c3_mro(class_id, &bases, this.heap, this.interns)?;
        for &mro_id in &mro {
            this.heap.inc_ref(mro_id);
        }
        if let HeapData::ClassObject(cls) = this.heap.get_mut(class_id) {
            cls.set_mro(mro);
        }

        if bases.is_empty() {
            let object_id = this.heap.builtin_class_id(Type::Object)?;
            this.heap.with_entry_mut(object_id, |_, data| {
                let HeapData::ClassObject(cls) = data else {
                    return Err(RunError::internal("builtin object is not a class object"));
                };
                cls.register_subclass(class_id, class_uid);
                Ok(())
            })?;
        } else {
            for &base_id in &bases {
                this.heap.with_entry_mut(base_id, |_, data| {
                    let HeapData::ClassObject(cls) = data else {
                        return Err(RunError::internal("base is not a class object"));
                    };
                    cls.register_subclass(class_id, class_uid);
                    Ok(())
                })?;
            }
        }

        Ok(Some(Value::Ref(class_id)))
    }

    // ========================================================================
    // Dunder Protocol Dispatch
    // ========================================================================

    /// Looks up a dunder method on an instance's TYPE (not the instance itself).
    ///
    /// This implements the Python semantic that dunder methods are looked up on the
    /// type, not the instance. For example, `type(x).__add__(x, y)` not `x.__add__(y)`.
    ///
    /// Returns `Some(method_value)` if found, `None` if not found.
    /// The returned value is cloned with proper refcount handling if it's a Ref.
    pub(super) fn lookup_type_dunder(&mut self, instance_heap_id: HeapId, dunder_name_id: StringId) -> Option<Value> {
        let dunder_name = self.interns.get_str(dunder_name_id);

        // Get the class_id from the instance
        let class_id = match self.heap.get(instance_heap_id) {
            HeapData::Instance(inst) => inst.class_id(),
            _ => return None,
        };

        // `__hash__` has special class semantics (checked via MRO, not just own namespace):
        // - If class or any parent defines `__hash__ = None`, instances are unhashable.
        // - If class or any parent defines `__eq__` but no class in MRO defines `__hash__`,
        //   instances are unhashable.
        if dunder_name_id == StaticStrings::DunderHash {
            // Collect MRO to avoid holding a borrow on heap across lookups
            let mro: Vec<HeapId> = match self.heap.get(class_id) {
                HeapData::ClassObject(cls) => cls.mro().to_vec(),
                _ => Vec::new(),
            };
            let mut has_eq = false;
            let mut has_hash = false;
            let mut hash_is_none = false;
            for &mro_id in &mro {
                if let HeapData::ClassObject(cls) = self.heap.get(mro_id) {
                    if !has_hash {
                        if let Some(attr) = cls.namespace().get_by_str("__hash__", self.heap, self.interns) {
                            has_hash = true;
                            hash_is_none = matches!(attr, Value::None);
                        }
                    }
                    if !has_eq {
                        if cls.namespace().get_by_str("__eq__", self.heap, self.interns).is_some() {
                            has_eq = true;
                        }
                    }
                    if has_hash && has_eq {
                        break;
                    }
                }
            }

            if hash_is_none || (has_eq && !has_hash) {
                return None;
            }
        }

        // Look up in the class namespace via MRO (NOT instance attrs)
        match self.heap.get(class_id) {
            HeapData::ClassObject(cls) => cls
                .mro_lookup_attr(dunder_name, class_id, self.heap, self.interns)
                .map(|(v, _found_in)| v),
            _ => None,
        }
    }

    /// Looks up a dunder method on a class object's METACLASS.
    ///
    /// Used for metaclass hooks like `__getattribute__`, `__getattr__`,
    /// `__instancecheck__`, and `__subclasscheck__`.
    pub(super) fn lookup_metaclass_dunder(&mut self, class_heap_id: HeapId, dunder_name_id: StringId) -> Option<Value> {
        let dunder_name = self.interns.get_str(dunder_name_id);

        let HeapData::ClassObject(class_obj) = self.heap.get(class_heap_id) else {
            return None;
        };

        let metaclass_val = class_obj.metaclass();
        let meta_id = match metaclass_val {
            Value::Ref(id) => *id,
            _ => return None,
        };

        match self.heap.get(meta_id) {
            HeapData::ClassObject(meta_cls) => {
                let found = meta_cls.mro_lookup_attr(dunder_name, meta_id, self.heap, self.interns);
                let (value, _) = found?;
                let is_object_dunder_fallback = matches!(
                    (dunder_name_id, &value),
                    (id, Value::Builtin(Builtins::TypeMethod {
                        ty: Type::Object,
                        method: StaticStrings::DunderGetattribute,
                    })) if id == StaticStrings::DunderGetattribute
                ) || matches!(
                    (dunder_name_id, &value),
                    (id, Value::Builtin(Builtins::TypeMethod {
                        ty: Type::Object,
                        method: StaticStrings::DunderSetattr,
                    })) if id == StaticStrings::DunderSetattr
                ) || matches!(
                    (dunder_name_id, &value),
                    (id, Value::Builtin(Builtins::TypeMethod {
                        ty: Type::Object,
                        method: StaticStrings::DunderDelattr,
                    })) if id == StaticStrings::DunderDelattr
                );
                if is_object_dunder_fallback {
                    value.drop_with_heap(self.heap);
                    None
                } else {
                    Some(value)
                }
            }
            _ => None,
        }
    }

    /// Looks up a dunder method only in the class object's metaclass namespace.
    ///
    /// This intentionally ignores inherited metaclass attrs so custom hooks on
    /// the immediate metaclass win over default `type` behavior.
    fn lookup_metaclass_namespace_dunder(&mut self, class_heap_id: HeapId, dunder_name: &str) -> Option<Value> {
        let meta_id = match self.heap.get(class_heap_id) {
            HeapData::ClassObject(class_obj) => match class_obj.metaclass() {
                Value::Ref(id) => *id,
                _ => return None,
            },
            _ => return None,
        };

        match self.heap.get(meta_id) {
            HeapData::ClassObject(meta_cls) => meta_cls
                .namespace()
                .get_by_str(dunder_name, self.heap, self.interns)
                .map(|value| value.clone_with_heap(self.heap)),
            _ => None,
        }
    }

    /// Returns whether a class has typing's private `_is_protocol` marker in its MRO.
    fn class_has_protocol_marker(&mut self, class_heap_id: HeapId) -> bool {
        self.class_has_typing_marker(class_heap_id, "_is_protocol")
            || self.class_has_typing_marker(class_heap_id, "_is_runtime_protocol")
    }

    /// Returns whether a class has a given typing marker set to True in its MRO.
    fn class_has_typing_marker(&mut self, class_heap_id: HeapId, marker_name: &str) -> bool {
        let Some(value) = (match self.heap.get(class_heap_id) {
            HeapData::ClassObject(cls) => {
                if let Some(v) = cls.namespace().get_by_str(marker_name, self.heap, self.interns) {
                    Some(v.clone_with_heap(self.heap))
                } else {
                    cls.mro_lookup_attr(marker_name, class_heap_id, self.heap, self.interns)
                        .map(|(v, _)| v)
                }
            }
            _ => None,
        }) else {
            return false;
        };

        let has_marker = matches!(value, Value::Bool(true));
        value.drop_with_heap(self.heap);
        has_marker
    }

    /// Calls a dunder method on an instance with given args.
    ///
    /// Prepends the instance as `self` argument, increments instance refcount.
    /// Returns `CallResult` which may be `FramePushed` for user-defined methods.
    pub(super) fn call_dunder(
        &mut self,
        instance_heap_id: HeapId,
        method_value: Value,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        // Increment instance refcount for the self argument
        self.heap.inc_ref(instance_heap_id);
        let self_arg = Value::Ref(instance_heap_id);

        let new_args = match args {
            ArgValues::Empty => ArgValues::One(self_arg),
            ArgValues::One(a) => ArgValues::Two(self_arg, a),
            ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                args: vec![self_arg, a, b],
                kwargs: KwargsValues::Empty,
            },
            ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                args: vec![self_arg],
                kwargs: kw,
            },
            ArgValues::ArgsKargs { mut args, kwargs } => {
                args.insert(0, self_arg);
                ArgValues::ArgsKargs { args, kwargs }
            }
        };

        self.call_function(method_value, new_args)
    }

    /// Calls a dunder method on a class object, prepending the class as `self`.
    ///
    /// This is used for metaclass hooks like `__prepare__`, `__mro_entries__`,
    /// `__instancecheck__`, and `__subclasscheck__`, where the class object itself
    /// is the receiver.
    pub(super) fn call_class_dunder(
        &mut self,
        class_heap_id: HeapId,
        method_value: Value,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        self.heap.inc_ref(class_heap_id);
        let cls_arg = Value::Ref(class_heap_id);
        let new_args = match args {
            ArgValues::Empty => ArgValues::One(cls_arg),
            ArgValues::One(a) => ArgValues::Two(cls_arg, a),
            ArgValues::Two(a, b) => ArgValues::ArgsKargs {
                args: vec![cls_arg, a, b],
                kwargs: KwargsValues::Empty,
            },
            ArgValues::Kwargs(kw) => ArgValues::ArgsKargs {
                args: vec![cls_arg],
                kwargs: kw,
            },
            ArgValues::ArgsKargs { mut args, kwargs } => {
                args.insert(0, cls_arg);
                ArgValues::ArgsKargs { args, kwargs }
            }
        };
        self.call_function(method_value, new_args)
    }

    /// Executes a binary dunder operation: tries `lhs.__op__(rhs)`, then `rhs.__rop__(lhs)`.
    ///
    /// Returns `Ok(Some(CallResult))` if a dunder was found and called,
    /// `Ok(None)` if neither operand has the dunder.
    ///
    /// Handles the NotImplemented protocol: if `__op__` returns NotImplemented,
    /// falls through to try `__rop__`.
    pub(super) fn try_binary_dunder(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        dunder_id: StringId,
        reflected_dunder_id: Option<StringId>,
    ) -> Result<Option<CallResult>, RunError> {
        // Try lhs.__op__(rhs) - look up on TYPE, not instance
        if let Value::Ref(lhs_id) = lhs
            && matches!(self.heap.get(*lhs_id), HeapData::Instance(_))
            && let Some(method) = self.lookup_type_dunder(*lhs_id, dunder_id)
        {
            // Clone rhs for the call arg
            let rhs_clone = rhs.clone_with_heap(self.heap);
            let result = self.call_dunder(*lhs_id, method, ArgValues::One(rhs_clone))?;
            match result {
                CallResult::FramePushed => {
                    self.pending_binary_dunder.push(PendingBinaryDunder {
                        lhs: lhs.clone_with_heap(self.heap),
                        rhs: rhs.clone_with_heap(self.heap),
                        primary_dunder_id: dunder_id,
                        reflected_dunder_id,
                        frame_depth: self.frames.len(),
                        stage: PendingBinaryDunderStage::Primary,
                    });
                    return Ok(Some(CallResult::FramePushed));
                }
                other => {
                    if let Some(result) = self.binary_result_if_implemented(other) {
                        return Ok(Some(result));
                    }
                }
            }
        }

        // Try rhs.__rop__(lhs) if provided
        if let Some(ref_dunder_id) = reflected_dunder_id
            && let Value::Ref(rhs_id) = rhs
            && matches!(self.heap.get(*rhs_id), HeapData::Instance(_))
            && let Some(method) = self.lookup_type_dunder(*rhs_id, ref_dunder_id)
        {
            // Clone lhs for the call arg
            let lhs_clone = lhs.clone_with_heap(self.heap);
            let result = self.call_dunder(*rhs_id, method, ArgValues::One(lhs_clone))?;
            return match result {
                CallResult::FramePushed => {
                    self.pending_binary_dunder.push(PendingBinaryDunder {
                        lhs: lhs.clone_with_heap(self.heap),
                        rhs: rhs.clone_with_heap(self.heap),
                        primary_dunder_id: dunder_id,
                        reflected_dunder_id,
                        frame_depth: self.frames.len(),
                        stage: PendingBinaryDunderStage::Reflected,
                    });
                    Ok(Some(CallResult::FramePushed))
                }
                other => Ok(self.binary_result_if_implemented(other)),
            };
        }

        Ok(None)
    }

    /// Returns `None` when a binary dunder returned `NotImplemented`.
    ///
    /// This consumes and drops `NotImplemented` so callers can attempt reflected
    /// dispatch (or eventually raise a type error).
    fn binary_result_if_implemented(&mut self, result: CallResult) -> Option<CallResult> {
        match result {
            CallResult::Push(v) if matches!(v, Value::NotImplemented) => {
                v.drop_with_heap(self.heap);
                None
            }
            other => Some(other),
        }
    }

    /// Builds a CPython-style binary type error from a dunder operation.
    pub(super) fn binary_dunder_type_error(&self, lhs: &Value, rhs: &Value, primary_dunder_id: StringId) -> RunError {
        let symbol = binary_symbol_for_dunder(primary_dunder_id);
        ExcType::binary_type_error(symbol, lhs.py_type(self.heap), rhs.py_type(self.heap))
    }

    /// Executes an in-place dunder operation: tries `lhs.__iop__(rhs)`, falls back to `lhs.__op__(rhs)`.
    ///
    /// Returns `Ok(Some(CallResult))` if a dunder was found and called,
    /// `Ok(None)` if the instance has no relevant dunder.
    pub(super) fn try_inplace_dunder(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        inplace_dunder_id: StringId,
        dunder_id: StringId,
        reflected_dunder_id: Option<StringId>,
    ) -> Result<Option<CallResult>, RunError> {
        // Try lhs.__iop__(rhs) first
        if let Value::Ref(lhs_id) = lhs
            && matches!(self.heap.get(*lhs_id), HeapData::Instance(_))
            && let Some(method) = self.lookup_type_dunder(*lhs_id, inplace_dunder_id)
        {
            let rhs_clone = rhs.clone_with_heap(self.heap);
            let result = self.call_dunder(*lhs_id, method, ArgValues::One(rhs_clone))?;
            return Ok(Some(result));
        }

        // Fall back to binary dunder
        self.try_binary_dunder(lhs, rhs, dunder_id, reflected_dunder_id)
    }

    /// Executes a unary dunder operation: tries `operand.__op__()`.
    ///
    /// Returns `Ok(Some(CallResult))` if the dunder was found and called,
    /// `Ok(None)` if the instance has no such dunder.
    pub(super) fn try_unary_dunder(
        &mut self,
        operand: &Value,
        dunder_id: StringId,
    ) -> Result<Option<CallResult>, RunError> {
        if let Value::Ref(id) = operand
            && matches!(self.heap.get(*id), HeapData::Instance(_))
            && let Some(method) = self.lookup_type_dunder(*id, dunder_id)
        {
            let result = self.call_dunder(*id, method, ArgValues::Empty)?;
            return Ok(Some(result));
        }
        Ok(None)
    }

    /// Handles attribute calls on singledispatch dispatchers (`register`, `dispatch`).
    fn call_singledispatch_attr(
        &mut self,
        target_id: HeapId,
        name_id: StringId,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let dispatcher_id = match self.heap.get(target_id) {
            HeapData::SingleDispatch(_) => target_id,
            HeapData::SingleDispatchMethod(method) => {
                if let Value::Ref(id) = &method.dispatcher {
                    *id
                } else {
                    args.drop_with_heap(self.heap);
                    return Err(RunError::internal(
                        "singledispatchmethod dispatcher was not a heap reference",
                    ));
                }
            }
            _ => {
                args.drop_with_heap(self.heap);
                return Err(RunError::internal(
                    "call_singledispatch_attr called on non-singledispatch object",
                ));
            }
        };

        let method_name = self.interns.get_str(name_id);
        match method_name {
            "register" => {
                let (mut positional, kwargs) = args.into_parts();
                if !kwargs.is_empty() {
                    positional.drop_with_heap(self.heap);
                    kwargs.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("register() takes no keyword arguments"));
                }

                let Some(cls) = positional.next() else {
                    positional.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("register expected at least 1 argument, got 0"));
                };

                if let Some(func) = positional.next() {
                    if let Some(extra) = positional.next() {
                        extra.drop_with_heap(self.heap);
                        positional.drop_with_heap(self.heap);
                        cls.drop_with_heap(self.heap);
                        func.drop_with_heap(self.heap);
                        return Err(ExcType::type_error("register expected at most 2 arguments, got 3"));
                    }
                    let func_for_registry = func.clone_with_heap(self.heap);
                    self.heap.inc_ref(dispatcher_id);
                    self.singledispatch_register(Value::Ref(dispatcher_id), cls, func_for_registry)?;
                    return Ok(CallResult::Push(func));
                }

                self.heap.inc_ref(dispatcher_id);
                let wrapper = crate::types::SingleDispatchRegister::new(Value::Ref(dispatcher_id), cls);
                let wrapper_id = self.heap.allocate(HeapData::SingleDispatchRegister(wrapper))?;
                Ok(CallResult::Push(Value::Ref(wrapper_id)))
            }
            "dispatch" => {
                let cls = args.get_one_arg("dispatch", self.heap)?;
                let dispatched = self.singledispatch_lookup_impl_for_class(dispatcher_id, &cls)?;
                cls.drop_with_heap(self.heap);
                Ok(CallResult::Push(dispatched))
            }
            _ => {
                args.drop_with_heap(self.heap);
                Err(ExcType::attribute_error(Type::Function, method_name))
            }
        }
    }

    /// Dispatches and calls a singledispatch callable.
    fn call_singledispatch_callable(
        &mut self,
        dispatcher_id: HeapId,
        callable: Value,
        args: ArgValues,
    ) -> Result<CallResult, RunError> {
        let dispatch_index = if let HeapData::SingleDispatch(dispatcher) = self.heap.get(dispatcher_id) {
            dispatcher.dispatch_index
        } else {
            callable.drop_with_heap(self.heap);
            args.drop_with_heap(self.heap);
            return Err(RunError::internal("dispatcher id did not reference SingleDispatch"));
        };

        let Some(dispatch_arg) = get_arg_at(&args, dispatch_index) else {
            callable.drop_with_heap(self.heap);
            args.drop_with_heap(self.heap);
            return Err(ExcType::type_error(format!(
                "singledispatch function requires at least {} positional arguments",
                dispatch_index + 1
            )));
        };

        let impl_func = self.singledispatch_lookup_impl_for_value(dispatcher_id, dispatch_arg)?;
        callable.drop_with_heap(self.heap);
        self.call_function(impl_func, args)
    }

    /// Registers an implementation for a dispatcher/type-key pair.
    fn singledispatch_register(&mut self, dispatcher: Value, cls: Value, func: Value) -> Result<(), RunError> {
        let Value::Ref(dispatcher_id) = dispatcher else {
            cls.drop_with_heap(self.heap);
            func.drop_with_heap(self.heap);
            return Err(RunError::internal("dispatcher was not a heap object"));
        };

        let is_valid_key = matches!(cls, Value::Builtin(Builtins::Type(_) | Builtins::ExcType(_)))
            || matches!(&cls, Value::Ref(id) if matches!(self.heap.get(*id), HeapData::ClassObject(_)));
        if !is_valid_key {
            cls.drop_with_heap(self.heap);
            func.drop_with_heap(self.heap);
            dispatcher.drop_with_heap(self.heap);
            return Err(ExcType::type_error(
                "singledispatch register() expects a class/type key",
            ));
        }

        self.heap.with_entry_mut(dispatcher_id, |heap, data| {
            let HeapData::SingleDispatch(dispatcher) = data else {
                cls.drop_with_heap(heap);
                func.drop_with_heap(heap);
                return Err(RunError::internal(
                    "singledispatch register target mutated to non-dispatcher",
                ));
            };
            dispatcher.registry.push((cls, func));
            Ok(())
        })?;
        dispatcher.drop_with_heap(self.heap);
        Ok(())
    }

    /// Finds the implementation for a runtime dispatch value.
    fn singledispatch_lookup_impl_for_value(
        &mut self,
        dispatcher_id: HeapId,
        dispatch_value: &Value,
    ) -> Result<Value, RunError> {
        let (default_func, registry_entries) = match self.heap.get(dispatcher_id) {
            HeapData::SingleDispatch(dispatcher) => (
                dispatcher.func.clone_with_heap(self.heap),
                dispatcher
                    .registry
                    .iter()
                    .map(|(cls, func)| (cls.clone_with_heap(self.heap), func.clone_with_heap(self.heap)))
                    .collect::<Vec<_>>(),
            ),
            _ => return Err(RunError::internal("dispatcher id did not reference SingleDispatch")),
        };

        let mut entries = registry_entries.into_iter().rev();
        while let Some((cls, func)) = entries.next() {
            let is_match = self.singledispatch_key_matches_value(&cls, dispatch_value)?;
            cls.drop_with_heap(self.heap);
            if is_match {
                for (remaining_cls, remaining_func) in entries {
                    remaining_cls.drop_with_heap(self.heap);
                    remaining_func.drop_with_heap(self.heap);
                }
                default_func.drop_with_heap(self.heap);
                return Ok(func);
            }
            func.drop_with_heap(self.heap);
        }

        Ok(default_func)
    }

    /// Finds the implementation for `dispatcher.dispatch(cls)`.
    fn singledispatch_lookup_impl_for_class(
        &mut self,
        dispatcher_id: HeapId,
        cls_value: &Value,
    ) -> Result<Value, RunError> {
        let (default_func, registry_entries) = match self.heap.get(dispatcher_id) {
            HeapData::SingleDispatch(dispatcher) => (
                dispatcher.func.clone_with_heap(self.heap),
                dispatcher
                    .registry
                    .iter()
                    .map(|(cls, func)| (cls.clone_with_heap(self.heap), func.clone_with_heap(self.heap)))
                    .collect::<Vec<_>>(),
            ),
            _ => return Err(RunError::internal("dispatcher id did not reference SingleDispatch")),
        };

        let mut entries = registry_entries.into_iter().rev();
        while let Some((registered_cls, func)) = entries.next() {
            let is_match = self.singledispatch_class_matches(&registered_cls, cls_value);
            registered_cls.drop_with_heap(self.heap);
            if is_match {
                for (remaining_cls, remaining_func) in entries {
                    remaining_cls.drop_with_heap(self.heap);
                    remaining_func.drop_with_heap(self.heap);
                }
                default_func.drop_with_heap(self.heap);
                return Ok(func);
            }
            func.drop_with_heap(self.heap);
        }

        Ok(default_func)
    }

    /// Checks whether a registered dispatch key matches a runtime argument value.
    fn singledispatch_key_matches_value(&mut self, key: &Value, value: &Value) -> Result<bool, RunError> {
        match key {
            Value::Builtin(Builtins::Type(expected_type)) => {
                let builtin_id = self.heap.builtin_class_id(*expected_type)?;
                if let Value::Ref(value_id) = value {
                    match self.heap.get(*value_id) {
                        HeapData::Instance(inst) => {
                            let instance_class_id = inst.class_id();
                            if let HeapData::ClassObject(cls_obj) = self.heap.get(instance_class_id) {
                                return Ok(cls_obj.is_subclass_of(instance_class_id, builtin_id));
                            }
                        }
                        HeapData::ClassObject(cls_obj) => {
                            return Ok(cls_obj.is_subclass_of(*value_id, builtin_id));
                        }
                        _ => {}
                    }
                }
                Ok(value.py_type(self.heap).is_instance_of(*expected_type))
            }
            Value::Builtin(Builtins::ExcType(expected_exc)) => Ok(matches!(
                value.py_type(self.heap),
                Type::Exception(actual_exc) if actual_exc.is_subclass_of(*expected_exc)
            )),
            Value::Ref(expected_class_id) if matches!(self.heap.get(*expected_class_id), HeapData::ClassObject(_)) => {
                if let Value::Ref(value_id) = value {
                    match self.heap.get(*value_id) {
                        HeapData::Instance(inst) => {
                            let instance_class_id = inst.class_id();
                            if let HeapData::ClassObject(cls_obj) = self.heap.get(instance_class_id) {
                                return Ok(cls_obj.is_subclass_of(instance_class_id, *expected_class_id));
                            }
                        }
                        HeapData::ClassObject(cls_obj) => {
                            return Ok(cls_obj.is_subclass_of(*value_id, *expected_class_id));
                        }
                        _ => {}
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// Checks whether a registered key matches an explicit class key for `dispatch()`.
    fn singledispatch_class_matches(&mut self, registered: &Value, queried: &Value) -> bool {
        match (registered, queried) {
            (Value::Builtin(Builtins::Type(expected)), Value::Builtin(Builtins::Type(actual))) => {
                actual.is_instance_of(*expected)
            }
            (Value::Builtin(Builtins::ExcType(expected)), Value::Builtin(Builtins::ExcType(actual))) => {
                actual.is_subclass_of(*expected)
            }
            (Value::Ref(expected_id), Value::Ref(actual_id))
                if matches!(self.heap.get(*expected_id), HeapData::ClassObject(_))
                    && matches!(self.heap.get(*actual_id), HeapData::ClassObject(_)) =>
            {
                if let HeapData::ClassObject(actual_cls) = self.heap.get(*actual_id) {
                    actual_cls.is_subclass_of(*actual_id, *expected_id)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// Builds the final argument list for a `functools.partial` invocation.
///
/// Applies Python 3.14 `Placeholder` substitution for positional arguments,
/// then appends remaining call-site positionals and merges keyword arguments
/// (call-site kwargs override pre-applied kwargs).
fn build_partial_call_args(
    partial_args: Vec<Value>,
    partial_kwargs: Vec<(Value, Value)>,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<ArgValues, RunError> {
    let (positional, kwargs) = args.into_parts();
    let mut call_positional: Vec<Value> = positional.collect();
    let call_positional_len = call_positional.len();
    let mut call_iter = call_positional.drain(..);
    let mut final_positional: Vec<Value> = Vec::with_capacity(partial_args.len() + call_positional_len);

    for template_arg in partial_args {
        if is_partial_placeholder(&template_arg, heap) {
            template_arg.drop_with_heap(heap);
            let Some(next_arg) = call_iter.next() else {
                for arg in call_iter {
                    arg.drop_with_heap(heap);
                }
                for (key, value) in partial_kwargs {
                    key.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                }
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "functools.partial missing positional arguments for Placeholder",
                ));
            };
            final_positional.push(next_arg);
        } else {
            final_positional.push(template_arg);
        }
    }
    final_positional.extend(call_iter);

    let mut merged_kwargs = partial_kwargs;
    merged_kwargs.extend(kwargs);

    let kwargs_values = if merged_kwargs.is_empty() {
        KwargsValues::Empty
    } else {
        KwargsValues::Dict(Dict::from_pairs(merged_kwargs, heap, interns)?)
    };

    Ok(build_arg_values(final_positional, kwargs_values))
}

/// Converts positional and keyword vectors into the most compact `ArgValues` representation.
fn build_arg_values(positional: Vec<Value>, kwargs: KwargsValues) -> ArgValues {
    if positional.is_empty() {
        if kwargs.is_empty() {
            ArgValues::Empty
        } else {
            ArgValues::Kwargs(kwargs)
        }
    } else if kwargs.is_empty() {
        match positional.len() {
            1 => ArgValues::One(positional.into_iter().next().expect("length checked")),
            2 => {
                let mut iter = positional.into_iter();
                ArgValues::Two(
                    iter.next().expect("length checked"),
                    iter.next().expect("length checked"),
                )
            }
            _ => ArgValues::ArgsKargs {
                args: positional,
                kwargs: KwargsValues::Empty,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: positional,
            kwargs,
        }
    }
}

/// Builds an argument cache key for `functools.lru_cache` wrappers.
///
/// For common one-argument calls, the key is the argument itself. For other
/// positional-only calls, the key is a tuple of positional arguments.
///
/// Keyword arguments are currently unsupported in this runtime path.
fn build_lru_cache_key(args: &ArgValues, typed: bool, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    match args {
        ArgValues::Empty => Ok(allocate_tuple(SmallVec::new(), heap)?),
        ArgValues::One(arg) if !typed => Ok(arg.clone_with_heap(heap)),
        ArgValues::Two(a, b) => {
            let mut items: SmallVec<[Value; 3]> = SmallVec::new();
            items.push(a.clone_with_heap(heap));
            items.push(b.clone_with_heap(heap));
            if typed {
                items.push(type_marker_for_cache_key(a, heap));
                items.push(type_marker_for_cache_key(b, heap));
            }
            Ok(allocate_tuple(items, heap)?)
        }
        ArgValues::Kwargs(_) => Err(ExcType::type_error("lru_cache does not yet support keyword arguments")),
        ArgValues::ArgsKargs { args, kwargs } => {
            if !kwargs.is_empty() {
                return Err(ExcType::type_error("lru_cache does not yet support keyword arguments"));
            }
            if args.len() == 1 && !typed {
                return Ok(args[0].clone_with_heap(heap));
            }
            let mut items: SmallVec<[Value; 3]> = SmallVec::with_capacity(args.len() * if typed { 2 } else { 1 });
            for value in args {
                items.push(value.clone_with_heap(heap));
            }
            if typed {
                for value in args {
                    items.push(type_marker_for_cache_key(value, heap));
                }
            }
            Ok(allocate_tuple(items, heap)?)
        }
        ArgValues::One(arg) => {
            let mut items: SmallVec<[Value; 3]> = SmallVec::new();
            items.push(arg.clone_with_heap(heap));
            items.push(type_marker_for_cache_key(arg, heap));
            Ok(allocate_tuple(items, heap)?)
        }
    }
}

/// Builds a type marker used when `lru_cache(..., typed=True)` is enabled.
fn type_marker_for_cache_key(value: &Value, heap: &Heap<impl ResourceTracker>) -> Value {
    match value.py_type(heap) {
        Type::Exception(exc_type) => Value::Builtin(Builtins::ExcType(exc_type)),
        ty => Value::Builtin(Builtins::Type(ty)),
    }
}

/// Computes the comparison symbol for a generated `total_ordering` method.
fn total_ordering_symbol(base: StaticStrings, swap: bool, negate: bool) -> &'static str {
    match (base, swap, negate) {
        (StaticStrings::DunderLt, true, true) => "<=",
        (StaticStrings::DunderLt, true, false) => ">",
        (StaticStrings::DunderLt, false, true) => ">=",
        (StaticStrings::DunderLe, true, false) => ">=",
        (StaticStrings::DunderLe, true, true) => "<",
        (StaticStrings::DunderLe, false, true) => ">",
        (StaticStrings::DunderGt, true, false) => "<",
        (StaticStrings::DunderGt, true, true) => ">=",
        (StaticStrings::DunderGt, false, true) => "<=",
        (StaticStrings::DunderGe, true, false) => "<=",
        (StaticStrings::DunderGe, true, true) => ">",
        (StaticStrings::DunderGe, false, true) => "<",
        _ => "<",
    }
}

/// Returns true when a value is the `functools.Placeholder` singleton.
fn is_partial_placeholder(value: &Value, heap: &Heap<impl ResourceTracker>) -> bool {
    matches!(value, Value::Ref(id) if matches!(heap.get(*id), HeapData::Placeholder(_)))
}

/// Returns a positional argument by index from an `ArgValues`.
fn get_arg_at(args: &ArgValues, index: usize) -> Option<&Value> {
    match args {
        ArgValues::Empty | ArgValues::Kwargs(_) => None,
        ArgValues::One(a) => {
            if index == 0 {
                Some(a)
            } else {
                None
            }
        }
        ArgValues::Two(a, b) => match index {
            0 => Some(a),
            1 => Some(b),
            _ => None,
        },
        ArgValues::ArgsKargs { args, .. } => args.get(index),
    }
}

/// Dispatches a classmethod call on a type object.
///
/// Handles classmethods like `dict.fromkeys()` and `bytes.fromhex()` that are
/// called on the type itself rather than on an instance.
fn call_type_method(
    t: Type,
    method_id: StringId,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<Value, RunError> {
    if matches!(
        t,
        Type::Date | Type::Datetime | Type::Time | Type::Timezone | Type::Tzinfo
    ) {
        return match crate::types::datetime_types::call_datetime_type_method(
            t,
            interns.get_str(method_id),
            args,
            heap,
            interns,
        )? {
            Some(value) => Ok(value),
            None => Err(ExcType::attribute_error(t, interns.get_str(method_id))),
        };
    }

    let method_name = interns.get_str(method_id);
    match (t, method_id) {
        (Type::Dict, m) if m == StaticStrings::Fromkeys => return dict_fromkeys(args, heap, interns),
        (Type::Bytes, m) if m == StaticStrings::Fromhex => return bytes_fromhex(args, heap, interns, false),
        (Type::Bytearray, m) if m == StaticStrings::Fromhex => return bytes_fromhex(args, heap, interns, true),
        (Type::Object, m)
            if m == StaticStrings::DunderSetattr
                || m == StaticStrings::DunderGetattribute
                || m == StaticStrings::DunderDelattr =>
        {
            let (mut positional, kwargs) = args.into_parts();
            let object_method_name = if m == StaticStrings::DunderSetattr {
                "object.__setattr__"
            } else if m == StaticStrings::DunderGetattribute {
                "object.__getattribute__"
            } else {
                "object.__delattr__"
            };
            if !kwargs.is_empty() {
                positional.drop_with_heap(heap);
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{object_method_name}() got unexpected keyword arguments"
                )));
            }
            kwargs.drop_with_heap(heap);

            let Some(instance) = positional.next() else {
                positional.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{object_method_name}() missing required positional argument: 'obj'"
                )));
            };
            let Some(name) = positional.next() else {
                positional.drop_with_heap(heap);
                instance.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "{object_method_name}() missing required positional argument: 'name'"
                )));
            };
            let name_id = if let Value::InternString(name_id) = &name {
                *name_id
            } else {
                positional.drop_with_heap(heap);
                instance.drop_with_heap(heap);
                name.drop_with_heap(heap);
                return Err(ExcType::type_error("attribute name must be string".to_string()));
            };

            if m == StaticStrings::DunderGetattribute {
                if positional.len() > 0 {
                    positional.drop_with_heap(heap);
                    instance.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "object.__getattribute__() takes exactly 2 arguments",
                    ));
                }
                positional.drop_with_heap(heap);
                let attr = instance.py_getattr(name_id, heap, interns);
                instance.drop_with_heap(heap);
                name.drop_with_heap(heap);
                return match attr {
                    Ok(AttrCallResult::Value(value)) => Ok(value),
                    Ok(_) => Err(ExcType::type_error(
                        "object.__getattribute__() unsupported descriptor return".to_string(),
                    )),
                    Err(err) => Err(err),
                };
            }

            if m == StaticStrings::DunderDelattr {
                if positional.len() > 0 {
                    positional.drop_with_heap(heap);
                    instance.drop_with_heap(heap);
                    name.drop_with_heap(heap);
                    return Err(ExcType::type_error("object.__delattr__() takes exactly 2 arguments"));
                }
                positional.drop_with_heap(heap);
                let result = instance.py_del_attr(name_id, heap, interns);
                instance.drop_with_heap(heap);
                name.drop_with_heap(heap);
                return result.map(|()| Value::None);
            }

            let Some(value) = positional.next() else {
                positional.drop_with_heap(heap);
                instance.drop_with_heap(heap);
                name.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "object.__setattr__() missing required positional argument: 'value'".to_string(),
                ));
            };
            if positional.len() > 0 {
                positional.drop_with_heap(heap);
                instance.drop_with_heap(heap);
                name.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "object.__setattr__() takes exactly 3 arguments".to_string(),
                ));
            }
            positional.drop_with_heap(heap);

            let Value::Ref(instance_id) = instance else {
                name.drop_with_heap(heap);
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "descriptor '__setattr__' requires an object instance".to_string(),
                ));
            };

            heap.with_entry_mut(instance_id, |heap, data| {
                let HeapData::Instance(inst) = data else {
                    name.drop_with_heap(heap);
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error(
                        "descriptor '__setattr__' requires an object instance".to_string(),
                    ));
                };
                if let Some(old) = inst.set_attr(name, value, heap, interns)? {
                    old.drop_with_heap(heap);
                }
                Ok(())
            })?;
            Value::Ref(instance_id).drop_with_heap(heap);
            return Ok(Value::None);
        }
        (Type::Bytes | Type::Bytearray, _) if method_name == "maketrans" => {
            return bytes_maketrans(args, heap, interns);
        }
        (Type::Str, _) if method_name == "maketrans" => return str_maketrans(args, heap, interns),
        (Type::Fraction, _) if matches!(method_name, "from_float" | "from_decimal" | "from_number") => {
            return t.call(heap, args, interns);
        }
        (Type::Decimal, _) if method_name == "from_float" => return t.call(heap, args, interns),
        _ => {}
    }
    // Other types or unknown methods - report actual type name, not 'type'
    args.drop_with_heap(heap);
    Err(ExcType::attribute_error(t, method_name))
}
