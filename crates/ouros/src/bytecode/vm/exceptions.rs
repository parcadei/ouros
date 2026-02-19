//! Exception handling helpers for the VM.

use super::VM;
use crate::{
    args::ArgValues,
    builtins::Builtins,
    bytecode::vm::call::CallResult,
    exception_private::{ExcType, ExceptionRaise, RawStackFrame, RunError, SimpleException},
    heap::HeapData,
    intern::{StaticStrings, StringId},
    io::PrintWriter,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{PyTrait, Type},
    value::Value,
};

impl<T: ResourceTracker, P: PrintWriter, Tr: VmTracer> VM<'_, T, P, Tr> {
    /// Returns the current frame's name for traceback generation.
    ///
    /// Returns the function name for user-defined functions, or `<module>` for
    /// module-level code.
    fn current_frame_name(&self) -> StringId {
        let frame = self.current_frame();
        match frame.function_id {
            Some(func_id) => self.interns.get_function(func_id).name.name_id,
            None => StaticStrings::Module.into(),
        }
    }

    /// Creates a `RawStackFrame` for the current execution point.
    ///
    /// Used when raising exceptions to capture traceback information.
    fn make_stack_frame(&self) -> RawStackFrame {
        RawStackFrame::new(self.current_position(), self.current_frame_name(), None)
    }

    /// Attaches initial frame information to an error if it doesn't have any.
    ///
    /// Only sets the innermost frame if the exception doesn't already have one.
    /// Caller frames are added separately during exception propagation.
    ///
    /// Uses the `hide_caret` flag from `ExceptionRaise` to determine whether to show
    /// the caret marker in the traceback. This flag is set by error creators that know
    /// whether CPython would show a caret for this specific error type.
    fn attach_frame_to_error(&self, error: RunError) -> RunError {
        match error {
            RunError::Exc(mut exc) => {
                if exc.frame.is_none() {
                    let mut frame = self.make_stack_frame();
                    // Use the hide_caret flag from the error (set by error creators)
                    frame.hide_caret = exc.hide_caret;
                    exc.frame = Some(frame);
                }
                RunError::Exc(exc)
            }
            RunError::UncatchableExc(mut exc) => {
                if exc.frame.is_none() {
                    let mut frame = self.make_stack_frame();
                    frame.hide_caret = exc.hide_caret;
                    exc.frame = Some(frame);
                }
                RunError::UncatchableExc(exc)
            }
            RunError::Internal(_) => error,
        }
    }

    /// Creates a RunError from a Value that should be an exception.
    ///
    /// Takes ownership of the exception value and drops it properly.
    /// The `is_raise` flag indicates if this is from a `raise` statement (hide caret).
    pub(super) fn make_exception(&mut self, exc_value: Value, is_raise: bool, attach_context: bool) -> RunError {
        let (mut simple_exc, _valid_exception, original_value) =
            self.coerce_simple_exception(exc_value, "exceptions must derive from BaseException");
        if attach_context && let Some(context) = self.active_exception_context() {
            simple_exc.set_context(Some(context));
        }
        self.build_exception_error(simple_exc, is_raise, original_value)
    }

    /// Creates a RunError for `raise X from Y`.
    pub(super) fn make_exception_with_cause(
        &mut self,
        exc_value: Value,
        cause_value: Value,
        is_raise: bool,
    ) -> RunError {
        let (mut simple_exc, _valid_exception, original_value) =
            self.coerce_simple_exception(exc_value, "exceptions must derive from BaseException");
        if let Some(context) = self.active_exception_context() {
            simple_exc.set_context(Some(context));
        }

        let cause = if matches!(cause_value, Value::None) {
            None
        } else {
            let (cause_exc, valid_cause, _cause_original) =
                self.coerce_simple_exception(cause_value, "exception causes must derive from BaseException");
            if !valid_cause {
                let mut cause_exc = cause_exc;
                if let Some(context) = self.active_exception_context() {
                    cause_exc.set_context(Some(context));
                }
                return self.build_exception_error(cause_exc, is_raise, None);
            }
            Some(cause_exc)
        };

        simple_exc.set_cause(cause);
        simple_exc.set_suppress_context(true);
        self.build_exception_error(simple_exc, is_raise, original_value)
    }

    /// Converts an arbitrary value to a `SimpleException`.
    ///
    /// Returns `(exception, true, original_value)` for valid exception values and
    /// `(TypeError, false, None)` when the value does not derive from `BaseException`.
    ///
    /// For user-defined exception instances, `original_value` preserves the original
    /// Value::Ref so that exception identity is maintained through raise/except.
    fn coerce_simple_exception(
        &mut self,
        exc_value: Value,
        type_error_msg: &'static str,
    ) -> (SimpleException, bool, Option<Value>) {
        // Match by reference first so `Value::Ref` does not get implicitly dropped
        // by pattern matching under `ref-count-panic`.
        if let Value::Ref(heap_id) = &exc_value {
            let heap_id = *heap_id;
            // Check the heap data type first without borrowing
            let is_simple_exception = matches!(self.heap.get(heap_id), HeapData::Exception(_));
            let is_instance = matches!(self.heap.get(heap_id), HeapData::Instance(_));
            if is_simple_exception {
                // This is already a SimpleException (builtin exception),
                // no need to preserve original value
                let HeapData::Exception(exc) = self.heap.get(heap_id) else {
                    unreachable!()
                };
                let exc_clone = exc.clone();
                exc_value.drop_with_heap(self.heap);
                (exc_clone, true, None)
            } else if is_instance {
                if let Some(exc) = self.simple_exception_from_instance(heap_id) {
                    // For user-defined exception instances, preserve the original value
                    (exc, true, Some(exc_value))
                } else {
                    exc_value.drop_with_heap(self.heap);
                    (
                        SimpleException::new_msg(ExcType::TypeError, type_error_msg),
                        false,
                        None,
                    )
                }
            } else {
                exc_value.drop_with_heap(self.heap);
                (
                    SimpleException::new_msg(ExcType::TypeError, type_error_msg),
                    false,
                    None,
                )
            }
        } else {
            match exc_value {
                // Exception type (e.g., `raise ValueError` instead of `raise ValueError()`).
                Value::Builtin(Builtins::ExcType(exc_type)) => (SimpleException::new_none(exc_type), true, None),
                // Invalid exception value.
                other => {
                    other.drop_with_heap(self.heap);
                    (
                        SimpleException::new_msg(ExcType::TypeError, type_error_msg),
                        false,
                        None,
                    )
                }
            }
        }
    }

    /// Returns the active exception from the current `except` context, if any.
    fn active_exception_context(&self) -> Option<SimpleException> {
        let current = self.exception_stack.last()?;
        if let Value::Ref(exc_id) = current
            && let HeapData::Exception(exc) = self.heap.get(*exc_id)
        {
            return Some(exc.clone());
        }
        None
    }

    /// Drops the top exception context if it matches an unwound stack value.
    fn drop_matching_exception_context_for_value(&mut self, value: &Value) {
        if let (Some(Value::Ref(active_id)), Value::Ref(popped_id)) = (self.exception_stack.last(), value)
            && active_id == popped_id
            && let Some(exc) = self.pop_exception_context()
        {
            exc.drop_with_heap(self.heap);
        }
    }

    /// Unwinds the operand stack to `target_stack_depth`, syncing exception context.
    fn unwind_stack_to(&mut self, target_stack_depth: usize) {
        while self.stack.len() > target_stack_depth {
            let value = self.stack.pop().unwrap();
            self.drop_matching_exception_context_for_value(&value);
            value.drop_with_heap(self.heap);
        }
    }

    /// Creates the raised RunError and attaches the current frame metadata.
    fn build_exception_error(
        &self,
        simple_exc: SimpleException,
        is_raise: bool,
        original_value: Option<Value>,
    ) -> RunError {
        let frame = if is_raise {
            RawStackFrame::from_raise(self.current_position(), self.current_frame_name())
        } else {
            self.make_stack_frame()
        };

        RunError::Exc(Box::new(ExceptionRaise {
            exc: simple_exc,
            frame: Some(frame),
            hide_caret: false,
            original_value,
        }))
    }
    /// Converts a user-defined exception instance into a lightweight `SimpleException`.
    /// Returns `None` when the instance class does not inherit any builtin exception
    /// base and therefore cannot be raised.
    fn simple_exception_from_instance(&self, instance_id: crate::heap::HeapId) -> Option<SimpleException> {
        let HeapData::Instance(inst) = self.heap.get(instance_id) else {
            return None;
        };
        let class_id = inst.class_id();
        let HeapData::ClassObject(cls) = self.heap.get(class_id) else {
            return None;
        };
        let mro_ids = cls.mro().to_vec();
        let mut mro_names = Vec::with_capacity(mro_ids.len());
        let mut builtin_exc_type = None;
        for &mro_id in &mro_ids {
            let HeapData::ClassObject(mro_cls) = self.heap.get(mro_id) else {
                continue;
            };
            mro_names.push(mro_cls.name(self.interns).to_string());
            if builtin_exc_type.is_none()
                && let Some(Type::Exception(exc_type)) = self.heap.builtin_type_for_class_id(mro_id)
            {
                builtin_exc_type = Some(exc_type);
            }
        }
        let exc_type = builtin_exc_type?;

        let mut custom_attrs = Vec::new();
        let mut message: Option<String> = None;
        if let Some(attrs) = inst.attrs(self.heap) {
            for (key, value) in attrs {
                let key_name = match key {
                    Value::InternString(id) => self.interns.get_str(*id).to_string(),
                    Value::Ref(id) => match self.heap.get(*id) {
                        HeapData::Str(s) => s.as_str().to_string(),
                        _ => continue,
                    },
                    _ => continue,
                };
                if message.is_none() && key_name == "args" {
                    message = self.exception_message_from_args_value(value);
                }
                custom_attrs.push((key_name, value.py_str(self.heap, self.interns).into_owned()));
            }
        }

        Some(SimpleException::new(exc_type, message).with_custom_metadata(
            cls.name(self.interns).to_string(),
            mro_names,
            custom_attrs,
        ))
    }

    /// Extracts the display message from an exception `args` value.
    ///
    /// Mirrors CPython behavior for exception string conversion:
    /// - zero args -> empty string
    /// - one arg -> `str(arg0)`
    /// - multiple args -> `str(args)`
    fn exception_message_from_args_value(&self, value: &Value) -> Option<String> {
        let Value::Ref(args_id) = value else {
            return None;
        };
        match self.heap.get(*args_id) {
            HeapData::Tuple(tuple) => {
                let items = tuple.as_vec();
                match items.as_slice() {
                    [] => Some(String::new()),
                    [item] => Some(item.py_str(self.heap, self.interns).into_owned()),
                    _ => Some(value.py_str(self.heap, self.interns).into_owned()),
                }
            }
            HeapData::List(list) => {
                let items = list.as_vec();
                match items.as_slice() {
                    [] => Some(String::new()),
                    [item] => Some(item.py_str(self.heap, self.interns).into_owned()),
                    _ => Some(value.py_str(self.heap, self.interns).into_owned()),
                }
            }
            _ => None,
        }
    }

    /// Handles an exception by searching for a handler in the exception table.
    ///
    /// Returns:
    /// - `Some(VMResult)` if the exception was not caught (should return from run loop)
    /// - `None` if the exception was caught (continue execution)
    ///
    /// When an exception is caught:
    /// 1. Unwinds the stack to the handler's expected depth
    /// 2. Pushes the exception value onto the stack
    /// 3. Sets `current_exception` for bare `raise`
    /// 4. Jumps to the handler code
    pub(super) fn handle_exception(&mut self, mut error: RunError) -> Option<RunError> {
        // Ensure exception has initial frame info
        error = self.attach_frame_to_error(error);

        // `list.sort(key=...)` key calls run in child frames while sort state is
        // stored in `pending_list_sort`. If a key call raises, clear pending state
        // and restore list contents before normal exception dispatch.
        if self.pending_list_sort_return
            && let Err(e) = self.abort_pending_list_sort_on_exception()
        {
            return Some(e);
        }
        if self.pending_min_max_return {
            self.clear_pending_min_max();
        }
        if self.pending_bisect_return {
            self.clear_pending_bisect();
        }

        // For uncatchable exceptions (ResourceError like RecursionError),
        // we still need to unwind the stack to collect all frames for the traceback
        if matches!(error, RunError::UncatchableExc(_) | RunError::Internal(_)) {
            return Some(self.unwind_for_traceback(error));
        }

        // `next(generator, default)` keeps a pending default while the generator frame runs.
        // Any non-StopIteration error from that generator should drop the pending default.
        if !error.is_stop_iteration()
            && let Some(generator_id) = self.current_frame().generator_id
            && let Some(default_value) = self.take_pending_next_default_for(generator_id)
        {
            default_value.drop_with_heap(self.heap);
        }

        // Deferred unpacking only expects StopIteration as normal completion from
        // generator list materialization. Any other exception aborts pending unpack.
        if !error.is_stop_iteration() {
            self.pending_unpack = None;
        }

        // Check if this is StopIteration from a __next__ dunder called by ForIter.
        // ForIter sets pending_for_iter_jump when __next__ pushes a frame. If that frame
        // raises StopIteration, we intercept it here: pop the __next__ frame, pop the
        // iterator from the caller's stack, and jump to the end of the for-loop body.
        // This must happen before the normal exception handler search because ForIter
        // doesn't generate exception table entries -- it catches StopIteration internally.
        if let Some(&offset) = self.pending_for_iter_jump.last() {
            let for_iter_getitem = self.pending_for_iter_getitem.last().copied().unwrap_or(false);
            if error.is_stop_iteration() || (for_iter_getitem && error.is_exception_type(ExcType::IndexError)) {
                self.pending_for_iter_jump.pop();
                self.pending_for_iter_getitem.pop();
                // Pop the __next__ dunder frame
                self.pop_frame();
                // Now in the caller frame: pop the iterator from TOS
                let iter = self.pop();
                if let Value::Ref(iter_id) = iter {
                    self.getitem_for_iter_indices.remove(&iter_id);
                }
                iter.drop_with_heap(self.heap);
                // Jump forward by the ForIter offset (relative to caller's IP)
                let frame = self.current_frame_mut();
                let ip_i64 = i64::try_from(frame.ip).expect("IP exceeds i64");
                let new_ip = ip_i64 + i64::from(offset);
                frame.ip = usize::try_from(new_ip).expect("jump resulted in negative or overflowing IP");
                return None; // Exception caught - continue execution after for-loop
            }
        }

        // Check if this is StopIteration from a __next__ dunder called for list construction.
        // list(instance_with___iter__) calls __iter__() then repeatedly __next__() until
        // StopIteration. If __next__() pushes a frame and that frame raises StopIteration,
        // we finish the list construction and push the list.
        if self.pending_list_build_return
            && self.pending_list_build_generator_id().is_none()
            && (error.is_stop_iteration()
                || (self.pending_list_build_uses_getitem() && error.is_exception_type(ExcType::IndexError)))
        {
            self.pop_frame();
            let result = self.handle_list_build_stop_iteration();
            match result {
                Ok(CallResult::Push(list_value)) => {
                    match self.maybe_finish_sum_from_list_value(list_value) {
                        Ok(CallResult::Push(value)) => match self.maybe_finish_builtin_from_list_value(value) {
                            Ok(CallResult::Push(value)) => {
                                self.push(value);
                            }
                            Ok(other) => {
                                return Some(RunError::internal(format!(
                                    "maybe_finish_builtin_from_list_value returned unexpected result: {other:?}"
                                )));
                            }
                            Err(e) => return Some(e),
                        },
                        Ok(other) => {
                            return Some(RunError::internal(format!(
                                "maybe_finish_sum_from_list_value returned unexpected result: {other:?}"
                            )));
                        }
                        Err(e) => return Some(e),
                    }
                    return None; // Exception caught - continue execution
                }
                Ok(other) => {
                    // This shouldn't happen - handle_list_build_stop_iteration always returns Push
                    return Some(RunError::internal(format!(
                        "handle_list_build_stop_iteration returned unexpected result: {other:?}"
                    )));
                }
                Err(e) => return Some(e),
            }
        }

        // PEP 479: an uncaught StopIteration raised *inside* a generator body is
        // transformed into RuntimeError when it would otherwise escape the generator.
        if error.is_stop_iteration()
            && self.current_frame().generator_id.is_some()
            && self.pending_generator_close != self.current_frame().generator_id
            && self
                .pending_yield_from
                .last()
                .is_none_or(|pending| Some(pending.outer_generator_id) != self.current_frame().generator_id)
        {
            error = ExcType::generator_raised_stop_iteration();
        }

        // A delegated `yield from` call raised a non-StopIteration exception in the
        // outer generator frame. Delegation is over, so drop the saved iterator.
        if let Some(pending) = self.pending_yield_from.last().copied()
            && self.current_frame().generator_id == Some(pending.outer_generator_id)
            && !error.is_stop_iteration()
        {
            self.pending_yield_from.pop();
            if self.stack.len() > self.current_frame().stack_base && self.is_yield_from_iterator(self.peek()) {
                let iter = self.pop();
                iter.drop_with_heap(self.heap);
            }
        }

        // Not StopIteration - fall through to normal exception handling

        // Only catchable exceptions can be handled
        // Extract original_value from error, clone it for use, and drop the original properly.
        // This prevents refcount issues when error is dropped.
        let (exc_info, original_exc_value) = match &mut error {
            RunError::Exc(exc) => {
                // Take the original_value out to manage its lifecycle
                let taken_value = exc.original_value.take();
                let cloned = taken_value.as_ref().map(|v| v.clone_with_heap(self.heap));
                // Drop the taken value properly (decrements refcount)
                if let Some(v) = taken_value {
                    v.drop_with_heap(self.heap);
                }
                (exc.clone(), cloned)
            }
            RunError::UncatchableExc(_) | RunError::Internal(_) => unreachable!(),
        };

        // Create exception value to push on stack
        // Use original_exc_value if available to preserve exception identity
        let exc_value = if let Some(original) = original_exc_value {
            original
        } else {
            match self.create_exception_value(&exc_info) {
                Ok(v) => v,
                Err(e) => return Some(e),
            }
        };

        // Search for handler in current and outer frames
        loop {
            self.discard_stale_pending_class_finalize();
            let frame = self.current_frame();
            let ip = u32::try_from(self.instruction_ip).expect("instruction IP exceeds u32");

            // Search exception table for a handler covering this IP
            if let Some(entry) = frame.code.find_exception_handler(ip) {
                let handler_offset = usize::try_from(entry.handler()).expect("handler offset exceeds usize");
                let target_stack_depth = frame.stack_base + entry.stack_depth() as usize;
                // Found a handler! Unwind stack and jump to it.
                // Unwind stack to target depth (drop excess values)
                self.unwind_stack_to(target_stack_depth);

                // Push exception value onto stack (handler expects it)
                let exc_for_stack = exc_value.clone_with_heap(self.heap);
                self.push(exc_for_stack);

                // Push exception onto the exception_stack for bare raise
                // This allows nested except handlers to restore outer exception context
                self.push_exception_context(exc_value);

                // Jump to handler
                self.current_frame_mut().ip = handler_offset;

                return None; // Continue execution at handler
            }

            // No handler in this frame. If this is a pending __getattribute__ call that
            // raised AttributeError, attempt __getattr__ fallback before unwinding.
            if let Some(pending) = self.pending_getattr_fallback.last()
                && pending.frame_depth == self.frames.len()
                && matches!(error, RunError::Exc(ref exc) if exc.exc.exc_type() == ExcType::AttributeError)
            {
                let pending = *pending;
                let getattr_id: StringId = StaticStrings::DunderGetattr.into();
                let getattr = match pending.kind {
                    super::PendingGetAttrKind::Instance => self.lookup_type_dunder(pending.obj_id, getattr_id),
                    super::PendingGetAttrKind::Class => self.lookup_metaclass_dunder(pending.obj_id, getattr_id),
                };

                if let Some(getattr) = getattr {
                    // We will handle this AttributeError via __getattr__.
                    let pending = self
                        .pending_getattr_fallback
                        .pop()
                        .expect("pending getattr entry disappeared");
                    exc_value.drop_with_heap(self.heap);

                    // Pop the __getattribute__ frame before invoking __getattr__.
                    self.pop_frame();
                    self.instruction_ip = self.current_frame().ip;

                    let name_val = Value::InternString(pending.name_id);
                    let result = match pending.kind {
                        super::PendingGetAttrKind::Instance => {
                            self.call_dunder(pending.obj_id, getattr, ArgValues::One(name_val))
                        }
                        super::PendingGetAttrKind::Class => {
                            self.call_class_dunder(pending.obj_id, getattr, ArgValues::One(name_val))
                        }
                    };

                    // Drop the extra receiver ref now that __getattr__ has been invoked.
                    Value::Ref(pending.obj_id).drop_with_heap(self.heap);

                    return match result {
                        Ok(CallResult::Push(value)) => {
                            self.push(value);
                            None
                        }
                        Ok(CallResult::FramePushed) => None,
                        Ok(CallResult::External(_, _)) => Some(RunError::internal(
                            "__getattr__ cannot perform external calls during exception handling",
                        )),
                        Ok(CallResult::Proxy(_, _, _)) => Some(RunError::internal(
                            "__getattr__ cannot perform proxy calls during exception handling",
                        )),
                        Ok(CallResult::OsCall(_, _)) => Some(RunError::internal(
                            "__getattr__ cannot perform os calls during exception handling",
                        )),
                        Err(e) => self.handle_exception(e),
                    };
                }
            }

            // No handler in this frame. If this exception is escaping a generator
            // during close(), CPython suppresses GeneratorExit/StopIteration and
            // close() returns None. Any other exception must propagate.
            if let Some(closing_generator_id) = self.pending_generator_close
                && self.current_frame().generator_id == Some(closing_generator_id)
            {
                if matches!(
                    &error,
                    RunError::Exc(exc)
                        if matches!(exc.exc.exc_type(), ExcType::GeneratorExit | ExcType::StopIteration)
                ) {
                    self.pending_generator_close = None;
                    exc_value.drop_with_heap(self.heap);
                    self.unwind_stack_to(self.current_frame().stack_base);
                    self.cleanup_generator_frame(closing_generator_id);
                    self.push(Value::None);
                    return None;
                }
                self.pending_generator_close = None;
            }

            // No handler in this frame - pop frame and try outer
            if self.frames.len() <= 1 {
                // No more frames - exception is unhandled
                exc_value.drop_with_heap(self.heap);

                // For spawned tasks, fail the task instead of propagating
                if self.is_spawned_task() {
                    match self.handle_task_failure(error) {
                        Ok(()) => {
                            // Switched to next task - continue execution
                            return None;
                        }
                        Err(waiter_error) => {
                            // Switched to waiter - handle error in waiter's context
                            return self.handle_exception(waiter_error);
                        }
                    }
                }

                return Some(error);
            }

            // Get the call site position before popping frame
            // This is where the caller invoked the function that's failing
            let call_position = self.current_frame().call_position;
            let unwound_depth = self.frames.len();

            // Unwind this frame. Generator cleanup pops the frame itself to preserve
            // generator-specific state transitions; non-generator frames use pop_frame().
            self.unwind_stack_to(self.current_frame().stack_base);
            if let Some(generator_id) = self.current_frame().generator_id {
                self.cleanup_generator_frame(generator_id);
            } else {
                self.pop_frame();
            }

            while self
                .pending_stringify_return
                .last()
                .is_some_and(|(_, frame_depth)| *frame_depth == unwound_depth)
            {
                self.pending_stringify_return.pop();
            }

            // Clear any pending flags that were set for this frame's return handling.
            // If a property setter was called and raised an exception, we don't want
            // the discard_return flag to affect the next function return.
            self.pending_discard_return = false;

            // If there is no caller frame left, the exception is unhandled.
            if self.frames.is_empty() {
                exc_value.drop_with_heap(self.heap);
                return Some(error);
            }

            // Add caller frame info to traceback (if we have call position)
            if let Some(pos) = call_position {
                let frame_name = self.current_frame_name();
                match &mut error {
                    RunError::Exc(exc) => exc.add_caller_frame(pos, frame_name),
                    RunError::UncatchableExc(exc) => exc.add_caller_frame(pos, frame_name),
                    RunError::Internal(_) => {}
                }
            }

            // Update instruction_ip for exception handler lookup in the caller frame.
            // Use the caller frame's bytecode IP (which was synced before the call)
            // so find_exception_handler can locate the correct try/except block.
            // Previously this used call_position.start().line which is a SOURCE LINE
            // number, not a bytecode offset, causing exception handlers to be missed.
            self.instruction_ip = self.current_frame().ip;
        }
    }

    /// Unwinds the call stack to collect all frames for a traceback.
    ///
    /// Used for uncatchable exceptions (like RecursionError) that can't be handled
    /// but still need a complete traceback showing all active call frames.
    fn unwind_for_traceback(&mut self, mut error: RunError) -> RunError {
        // Pop frames and add caller frame info to the traceback
        while self.frames.len() > 1 {
            // Get the call site position before popping frame
            let call_position = self.current_frame().call_position;
            let unwound_depth = self.frames.len();

            // Pop this frame (cleans up namespace, etc.)
            self.unwind_stack_to(self.current_frame().stack_base);
            self.pop_frame();
            while self
                .pending_stringify_return
                .last()
                .is_some_and(|(_, frame_depth)| *frame_depth == unwound_depth)
            {
                self.pending_stringify_return.pop();
            }

            // Add caller frame info to traceback
            if let Some(pos) = call_position {
                let frame_name = self.current_frame_name();
                match &mut error {
                    RunError::Exc(exc) => exc.add_caller_frame(pos, frame_name),
                    RunError::UncatchableExc(exc) => exc.add_caller_frame(pos, frame_name),
                    RunError::Internal(_) => {}
                }
            }
        }
        error
    }

    /// Creates an exception Value from exception info.
    ///
    /// If `exc.original_value` is set (for user-defined exception instances),
    /// returns a clone of the original value to preserve exception identity.
    /// Otherwise, allocates a new Exception on the heap from the SimpleException.
    fn create_exception_value(&mut self, exc: &ExceptionRaise) -> Result<Value, RunError> {
        // For user-defined exception instances, use the original value to preserve
        // exception identity (ensures isinstance() and type() work correctly)
        if let Some(original) = &exc.original_value {
            return Ok(original.clone_with_heap(self.heap));
        }
        // For builtin exceptions, create a new value from the SimpleException
        let exception = exc.exc.clone();
        let heap_id = self.heap.allocate(HeapData::Exception(exception))?;
        Ok(Value::Ref(heap_id))
    }

    /// Checks if an exception matches an exception type for except clause matching.
    ///
    /// Validates that `exc_type` is a valid exception type (builtin exception,
    /// user-defined exception class, or tuple of those).
    /// Returns `Ok(true)` if exception matches, `Ok(false)` if not, or `Err` if exc_type is invalid.
    pub(super) fn check_exc_match(&self, exception: &Value, exc_type: &Value) -> Result<bool, RunError> {
        let exc_type_enum = exception.py_type(self.heap);
        self.check_exc_match_inner(exception, exc_type_enum, exc_type)
    }

    /// Inner recursive helper for check_exc_match that handles tuples and class handlers.
    fn check_exc_match_inner(
        &self,
        exception: &Value,
        exc_type_enum: Type,
        exc_type: &Value,
    ) -> Result<bool, RunError> {
        match exc_type {
            // Valid exception type
            Value::Builtin(Builtins::ExcType(handler_type)) => {
                // Check if exception is a builtin exception instance
                if matches!(exc_type_enum, Type::Exception(et) if et.is_subclass_of(*handler_type)) {
                    return Ok(true);
                }
                // Check if exception is a user-defined exception instance (preserved as Instance)
                // For example: a user class inheriting from Exception, caught via `except Exception`
                if let Value::Ref(exc_id) = exception
                    && let HeapData::Instance(inst) = self.heap.get(*exc_id)
                {
                    let inst_class_id = inst.class_id();
                    // Check the instance's class MRO for a builtin exception base
                    // that is a subclass of the handler type
                    if let HeapData::ClassObject(inst_cls) = self.heap.get(inst_class_id) {
                        for &mro_class_id in inst_cls.mro() {
                            if let Some(Type::Exception(exc_type)) = self.heap.builtin_type_for_class_id(mro_class_id)
                                && exc_type.is_subclass_of(*handler_type)
                            {
                                return Ok(true);
                            }
                        }
                    }
                }
                Ok(false)
            }
            // Tuple of exception types
            Value::Ref(id) => {
                match self.heap.get(*id) {
                    HeapData::Tuple(tuple) => {
                        for v in tuple.as_vec() {
                            if self.check_exc_match_inner(exception, exc_type_enum, v)? {
                                return Ok(true);
                            }
                        }
                        Ok(false)
                    }
                    HeapData::ClassObject(cls) => {
                        let Some(handler_exc_type) = self.class_builtin_exception_type(*id) else {
                            return Err(ExcType::except_invalid_type_error());
                        };
                        let handler_name = cls.name(self.interns);

                        // Check if exception is a SimpleException (builtin exception path)
                        if let Value::Ref(exc_id) = exception
                            && let HeapData::Exception(simple_exc) = self.heap.get(*exc_id)
                            && simple_exc.matches_custom_handler_name(handler_name)
                        {
                            return Ok(true);
                        }

                        // Check if exception is an Instance (user-defined exception instance path)
                        // When we preserve the original exception instance, we need to check
                        // if the instance's class is a subclass of the handler class
                        if let Value::Ref(exc_id) = exception
                            && let HeapData::Instance(inst) = self.heap.get(*exc_id)
                        {
                            let inst_class_id = inst.class_id();
                            // Check if the instance's class is the same as the handler class
                            if inst_class_id == *id {
                                return Ok(true);
                            }
                            // Check if the instance class has the handler class in its MRO
                            // (this handles subclass matching like `except Exception` catching ValidationError)
                            if let HeapData::ClassObject(inst_cls) = self.heap.get(inst_class_id)
                                && inst_cls.mro().contains(id)
                            {
                                return Ok(true);
                            }
                        }

                        Ok(matches!(
                            exc_type_enum,
                            Type::Exception(actual_exc_type) if actual_exc_type.is_subclass_of(handler_exc_type)
                        ))
                    }
                    // Not a tuple/class - invalid exception type
                    _ => Err(ExcType::except_invalid_type_error()),
                }
            }
            // Any other type is invalid for except clause
            _ => Err(ExcType::except_invalid_type_error()),
        }
    }

    /// Resolves the nearest builtin exception type in a class MRO.
    ///
    /// Returns `None` when the class does not inherit from any builtin exception
    /// base and therefore is invalid in an `except` clause.
    fn class_builtin_exception_type(&self, class_id: crate::heap::HeapId) -> Option<ExcType> {
        let HeapData::ClassObject(cls) = self.heap.get(class_id) else {
            return None;
        };
        for &mro_id in cls.mro() {
            if let Some(Type::Exception(exc_type)) = self.heap.builtin_type_for_class_id(mro_id) {
                return Some(exc_type);
            }
        }
        None
    }
}
