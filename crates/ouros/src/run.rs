//! Public interface for running Ouros code.
#[cfg(feature = "ref-count-return")]
use std::collections::HashSet;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    ExcType, Exception,
    asyncio::CallId,
    bytecode::{CachedVMBuffers, Code, Compiler, FrameExit, VM, VMSnapshot},
    exception_private::{RunError, RunResult},
    expressions::{Expr, Literal, Node, PreparedNode},
    heap::Heap,
    intern::{ExtFunctionId, Interns},
    io::{PrintWriter, StdPrint},
    modules::enum_mod,
    namespace::Namespaces,
    object::Object,
    os::OsFunction,
    parse::parse,
    prepare::prepare,
    resource::{NoLimitTracker, ResourceTracker},
    tracer::NoopTracer,
    value::Value,
};

/// Primary interface for running Ouros code.
///
/// `Runner` supports two execution modes:
/// - **Simple execution**: Use `run()` or `run_no_limits()` to run code to completion
/// - **Iterative execution**: Use `start()` to start execution which will pause at external function calls and
///   can be resumed later
///
/// # Example
/// ```
/// use ouros::{Runner, Object};
///
/// let runner = Runner::new("x + 1".to_owned(), "test.py", vec!["x".to_owned()], vec![]).unwrap();
/// let result = runner.run_no_limits(vec![Object::Int(41)]).unwrap();
/// assert_eq!(result, Object::Int(42));
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Runner {
    /// The underlying executor containing parsed AST and interns.
    executor: Executor,
}

impl Runner {
    /// Creates a new run snapshot by parsing the given code.
    ///
    /// This only parses and prepares the code - no heap or namespaces are created yet.
    /// Call `run_snapshot()` with inputs to start execution.
    ///
    /// # Arguments
    /// * `code` - The Python code to execute
    /// * `script_name` - The script name for error messages
    /// * `input_names` - Names of input variables
    ///
    /// # Errors
    /// Returns `Exception` if the code cannot be parsed.
    pub fn new(
        code: String,
        script_name: &str,
        input_names: Vec<String>,
        external_functions: Vec<String>,
    ) -> Result<Self, Exception> {
        Executor::new(code, script_name, input_names, external_functions).map(|executor| Self { executor })
    }

    /// Returns the code that was parsed to create this snapshot.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.executor.code
    }

    /// Executes the code and returns both the result and reference count data, used for testing only.
    #[cfg(feature = "ref-count-return")]
    pub fn run_ref_counts(&self, inputs: Vec<Object>) -> Result<RefCountOutput, Exception> {
        self.executor.run_ref_counts(inputs)
    }

    /// Executes the code to completion assuming not external functions or snapshotting.
    ///
    /// This is marginally faster than running with snapshotting enabled since we don't need
    /// to track the position in code, but does not allow calling of external functions.
    ///
    /// # Arguments
    /// * `inputs` - Values to fill the first N slots of the namespace
    /// * `resource_tracker` - Custom resource tracker implementation
    /// * `print` - print print implementation
    pub fn run(
        &self,
        inputs: Vec<Object>,
        resource_tracker: impl ResourceTracker,
        print: &mut impl PrintWriter,
    ) -> Result<Object, Exception> {
        self.executor.run(inputs, resource_tracker, print)
    }

    /// Executes the code to completion with no resource limits, printing to stdout/stderr.
    ///
    /// This uses an optimized path that caches and reuses the Heap and VM buffers
    /// across repeated invocations, avoiding the allocation/deallocation overhead
    /// that dominates execution time for short programs.
    pub fn run_no_limits(&self, inputs: Vec<Object>) -> Result<Object, Exception> {
        self.executor.run_no_limits_cached(inputs, &mut StdPrint)
    }

    /// Serializes the runner to a binary format.
    ///
    /// The serialized data can be stored and later restored with `load()`.
    /// This allows caching parsed code to avoid re-parsing on subsequent runs.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn dump(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserializes a runner from binary format.
    ///
    /// # Arguments
    /// * `bytes` - The serialized runner data from `dump()`
    ///
    /// # Errors
    /// Returns an error if deserialization fails.
    pub fn load(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }

    /// Starts execution with the given inputs and resource tracker, consuming self.
    ///
    /// Creates the heap and namespaces, then begins execution.
    ///
    /// For iterative execution, `start()` consumes self and returns a `RunProgress`:
    /// - `RunProgress::FunctionCall { ..., state }` - external function call, call `state.run(return_value)` to resume
    /// - `RunProgress::Complete(value)` - execution finished
    ///
    /// This enables snapshotting execution state and returning control to the host
    /// application during long-running computations.
    ///
    /// # Arguments
    /// * `inputs` - Initial input values (must match length of `input_names` from `new()`)
    /// * `resource_tracker` - Resource tracker for the execution
    /// * `print` - Writer for print output
    ///
    /// # Errors
    /// Returns `Exception` if:
    /// - The number of inputs doesn't match the expected count
    /// - An input value is invalid (e.g., `Object::Repr`)
    /// - A runtime error occurs during execution
    ///
    /// # Panics
    /// This method should not panic under normal operation. Internal assertions
    /// may panic if the VM reaches an inconsistent state (indicating a bug).
    pub fn start<T: ResourceTracker>(
        self,
        inputs: Vec<Object>,
        resource_tracker: T,
        print: &mut impl PrintWriter,
    ) -> Result<RunProgress<T>, Exception> {
        // Reset per-execution state (e.g. enum.auto() counter)
        enum_mod::reset_auto_counter();

        let executor = self.executor;

        // Create heap and prepare namespaces
        let mut heap = Heap::new(executor.namespace_size, resource_tracker);
        let mut namespaces = executor.prepare_namespaces(inputs, &mut heap)?;

        // Create and run VM - scope the VM borrow so we can move heap/namespaces after
        let mut vm = VM::new(&mut heap, &mut namespaces, &executor.interns, print, NoopTracer);

        // Start execution
        let vm_result = vm.run_module(&executor.module_code);

        let vm_state = vm.check_snapshot(&vm_result);

        // Handle the result using the destructured parts
        handle_vm_result(vm_result, vm_state, executor, heap, namespaces)
    }
}

/// Result of a single step of iterative execution.
///
/// This enum owns the execution state, ensuring type-safe state transitions.
/// - `FunctionCall` contains info about an external function call and state to resume
/// - `ResolveFutures` contains pending futures that need resolution before continuing
/// - `Complete` contains just the final value (execution is done)
///
/// # Type Parameters
/// * `T` - Resource tracker implementation (e.g., `NoLimitTracker` or `LimitedTracker`)
///
/// Serialization requires `T: Serialize + Deserialize`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: serde::de::DeserializeOwned"))]
pub enum RunProgress<T: ResourceTracker> {
    /// Execution paused at an external function call.
    ///
    /// The host can choose how to handle this:
    /// - **Sync resolution**: Call `state.run(return_value)` to push the result and continue
    /// - **Async resolution**: Call `state.run_pending()` to push an `ExternalFuture` and continue
    ///
    /// When using async resolution, the code continues and may `await` the future later.
    /// If the future isn't resolved when awaited, execution yields with `ResolveFutures`.
    FunctionCall {
        /// The name of the function being called.
        function_name: String,
        /// The positional arguments passed to the function.
        args: Vec<Object>,
        /// The keyword arguments passed to the function (key, value pairs).
        kwargs: Vec<(Object, Object)>,
        /// Unique identifier for this call (used for async correlation).
        call_id: u32,
        /// The execution state that can be resumed with a return value.
        state: Snapshot<T>,
    },
    /// Execution paused for an OS-level operation.
    ///
    /// The host should execute the OS operation (filesystem, network, etc.) and
    /// call `state.run(return_value)` to provide the result and continue.
    ///
    /// This enables sandboxed execution where the interpreter never directly performs I/O.
    OsCall {
        /// The OS function to execute.
        function: OsFunction,
        /// The positional arguments for the OS function.
        args: Vec<Object>,
        /// The keyword arguments passed to the function (key, value pairs).
        kwargs: Vec<(Object, Object)>,
        /// Unique identifier for this call (used for async correlation).
        call_id: u32,
        /// The execution state that can be resumed with a return value.
        state: Snapshot<T>,
    },
    /// All async tasks are blocked waiting for external futures to resolve.
    ///
    /// The host must resolve some or all of the pending calls before continuing.
    /// Use `state.resume(results)` to provide results for pending calls.
    ///
    /// access the pending call ids with `.pending_call_ids()`
    ResolveFutures(FutureSnapshot<T>),
    /// Execution completed with a final result.
    Complete(Object),
}

impl<T: ResourceTracker> RunProgress<T> {
    /// Consumes the `RunProgress` and returns external function call info and state.
    ///
    /// Returns (function_name, positional_args, keyword_args, call_id, state).
    #[must_use]
    #[expect(clippy::type_complexity)]
    pub fn into_function_call(self) -> Option<(String, Vec<Object>, Vec<(Object, Object)>, u32, Snapshot<T>)> {
        match self {
            Self::FunctionCall {
                function_name,
                args,
                kwargs,
                call_id,
                state,
            } => Some((function_name, args, kwargs, call_id, state)),
            _ => None,
        }
    }

    /// Consumes the `RunProgress` and returns the final value.
    #[must_use]
    pub fn into_complete(self) -> Option<Object> {
        match self {
            Self::Complete(value) => Some(value),
            _ => None,
        }
    }

    /// Consumes the `RunProgress` and returns pending futures info and state.
    ///
    /// Returns (pending_calls, state) if this is a ResolveFutures, None otherwise.
    #[must_use]
    pub fn into_resolve_futures(self) -> Option<FutureSnapshot<T>> {
        match self {
            Self::ResolveFutures(state) => Some(state),
            _ => None,
        }
    }
}

impl<T: ResourceTracker + serde::Serialize> RunProgress<T> {
    /// Serializes the execution state to a binary format.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn dump(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }
}

impl<T: ResourceTracker + serde::de::DeserializeOwned> RunProgress<T> {
    /// Deserializes execution state from binary format.
    ///
    /// # Errors
    /// Returns an error if deserialization fails.
    pub fn load(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

/// Execution state that can be resumed after an external function call.
///
/// This struct owns all runtime state and provides methods to continue execution:
/// - `run(result)`: Resume with the external function's return value (sync pattern)
/// - `run_pending()`: Resume with an `ExternalFuture` that can be awaited later (async pattern)
///
/// External function calls occur when calling a function that is not a builtin,
/// exception, or user-defined function.
///
/// # Type Parameters
/// * `T` - Resource tracker implementation
///
/// Serialization requires `T: Serialize + Deserialize`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: serde::de::DeserializeOwned"))]
pub struct Snapshot<T: ResourceTracker> {
    /// The executor containing compiled code and interns.
    executor: Executor,
    /// The VM state containing stack, frames, and exception state.
    vm_state: VMSnapshot,
    /// The heap containing all allocated objects.
    heap: Heap<T>,
    /// The namespaces containing all variable bindings.
    namespaces: Namespaces,
    /// The call_id from the most recent FunctionCall that created this Snapshot.
    /// Used by `run_pending()` to push the correct `ExternalFuture`.
    pending_call_id: u32,
}

#[derive(Debug)]
pub struct OurosFuture;

/// Return value or exception from an external function.
#[derive(Debug)]
pub enum ExternalResult {
    /// Continues execution with the return value from the external function.
    Return(Object),
    /// Continues execution with the exception raised by the external function.
    Error(Exception),
    /// Pending future - when the external function is a coroutine.
    Future,
}

impl From<Object> for ExternalResult {
    fn from(value: Object) -> Self {
        Self::Return(value)
    }
}

impl From<Exception> for ExternalResult {
    fn from(exception: Exception) -> Self {
        Self::Error(exception)
    }
}

impl From<OurosFuture> for ExternalResult {
    fn from(_: OurosFuture) -> Self {
        Self::Future
    }
}

impl<T: ResourceTracker> Snapshot<T> {
    /// Continues execution with the return value or exception from the external function.
    ///
    /// Consumes self and returns the next execution progress.
    ///
    /// # Arguments
    /// * `result` - The return value or exception from the external function
    /// * `print` - The print writer to use for output
    ///
    /// # Panics
    /// This method should not panic under normal operation. Internal assertions
    /// may panic if the VM reaches an inconsistent state (indicating a bug).
    pub fn run(
        mut self,
        result: impl Into<ExternalResult>,
        print: &mut impl PrintWriter,
    ) -> Result<RunProgress<T>, Exception> {
        let ext_result = result.into();

        // Restore the VM from the snapshot
        let mut vm = VM::restore(
            self.vm_state,
            &self.executor.module_code,
            &mut self.heap,
            &mut self.namespaces,
            &self.executor.interns,
            print,
            NoopTracer,
        );

        // Convert return value or exception before creating VM (to avoid borrow conflicts)
        let vm_result = match ext_result {
            ExternalResult::Return(obj) => vm.resume(obj),
            ExternalResult::Error(exc) => vm.resume_with_exception(exc.into()),
            ExternalResult::Future => {
                // Get the call_id and ext_function_id that were stored when this Snapshot was created
                let call_id = CallId::new(self.pending_call_id);

                // Store pending call data in the scheduler so we can track the creator task
                // and ignore results if the task is cancelled
                vm.add_pending_call(call_id);

                // Push the ExternalFuture value onto the stack
                // This allows the code to continue and potentially await this future later
                vm.push(Value::ExternalFuture(call_id));

                // Continue execution
                vm.run()
            }
        };

        let vm_state = vm.check_snapshot(&vm_result);

        // Handle the result using the destructured parts
        handle_vm_result(vm_result, vm_state, self.executor, self.heap, self.namespaces)
    }

    /// Continues execution by pushing an ExternalFuture instead of a concrete value.
    ///
    /// This is the async resolution pattern: instead of providing the result immediately,
    /// the host calls this method to continue execution with a pending future. The code
    /// can then `await` this future later.
    ///
    /// If the code awaits the future before it's resolved, execution will yield with
    /// `RunProgress::ResolveFutures`. The host can then provide the result via
    /// `FutureSnapshot::resume()`.
    ///
    /// # Arguments
    /// * `print` - Writer for print output
    ///
    /// # Returns
    /// The next execution progress - may be another `FunctionCall`, `ResolveFutures`, or `Complete`.
    ///
    /// # Panics
    /// Panics if the VM reaches an inconsistent state (indicating a bug in the interpreter).
    pub fn run_pending(self, print: &mut impl PrintWriter) -> Result<RunProgress<T>, Exception> {
        self.run(OurosFuture, print)
    }
}

/// Execution state paused while waiting for external future results.
///
/// Unlike `Snapshot` (used for sync external calls), `FutureSnapshot` supports
/// incremental resolution - you can provide partial results and Ouros will
/// continue running until all tasks are blocked again.
///
/// # Type Parameters
/// * `T` - Resource tracker implementation
///
/// Serialization requires `T: Serialize + Deserialize`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: serde::de::DeserializeOwned"))]
pub struct FutureSnapshot<T: ResourceTracker> {
    /// The executor containing compiled code and interns.
    executor: Executor,
    /// The VM state containing stack, frames, and exception state.
    vm_state: VMSnapshot,
    /// The heap containing all allocated objects.
    heap: Heap<T>,
    /// The namespaces containing all variable bindings.
    namespaces: Namespaces,
    /// The pending call_ids that this snapshot is waiting on.
    /// Used to validate that resume() only receives known call_ids.
    pending_call_ids: Vec<u32>,
}

impl<T: ResourceTracker> FutureSnapshot<T> {
    pub fn pending_call_ids(&self) -> &[u32] {
        &self.pending_call_ids
    }

    /// Resumes execution with results for some or all pending futures.
    ///
    /// **Incremental resolution**: You don't need to provide all results at once.
    /// If you provide a partial list, Ouros will:
    /// 1. Mark those futures as resolved
    /// 2. Unblock any tasks waiting on those futures
    /// 3. Continue running until all tasks are blocked again
    /// 4. Return `ResolveFutures` with the remaining pending calls
    ///
    /// This allows the host to resolve futures as they complete, rather than
    /// waiting for all of them.
    ///
    /// # Arguments
    /// * `results` - List of (call_id, result) pairs. Can be a subset of pending calls.
    /// * `print` - Writer for print output
    ///
    /// # Returns
    /// * `RunProgress::ResolveFutures` - More futures need resolution
    /// * `RunProgress::FunctionCall` - VM hit another external call
    /// * `RunProgress::Complete` - All tasks completed successfully
    /// * `Err(Exception)` - An unhandled exception occurred
    ///
    /// # Errors
    /// Returns `Err(Exception)` if any call_id in `results` is not in the pending set.
    ///
    /// # Panics
    /// Panics if the VM state cannot be snapshotted (internal error).
    pub fn resume(
        self,
        results: Vec<(u32, ExternalResult)>,
        print: &mut impl PrintWriter,
    ) -> Result<RunProgress<T>, Exception> {
        // Destructure self to avoid partial move issues
        let Self {
            executor,
            vm_state,
            mut heap,
            mut namespaces,
            pending_call_ids,
        } = self;

        // Validate that all provided call_ids are in the pending set before restoring VM
        let invalid_call_id = results
            .iter()
            .find(|(call_id, _)| !pending_call_ids.contains(call_id))
            .map(|(call_id, _)| *call_id);

        // Restore the VM from the snapshot (must happen before any error return to clean up properly)
        let mut vm = VM::restore(
            vm_state,
            &executor.module_code,
            &mut heap,
            &mut namespaces,
            &executor.interns,
            print,
            NoopTracer,
        );

        // Now check for invalid call_ids after VM is restored
        if let Some(call_id) = invalid_call_id {
            vm.cleanup();
            #[cfg(feature = "ref-count-panic")]
            namespaces.drop_global_with_heap(&mut heap);
            return Err(Exception::runtime_error(format!(
                "unknown call_id {call_id}, expected one of: {pending_call_ids:?}"
            )));
        }

        for (call_id, ext_result) in results {
            match ext_result {
                // Resolve successful futures in the scheduler
                ExternalResult::Return(obj) => vm
                    .resolve_future(call_id, obj)
                    .map_err(|e| Exception::runtime_error(format!("Invalid return type for call {call_id}: {e}")))?,
                // Fail futures that returned errors
                ExternalResult::Error(exc) => vm.fail_future(call_id, RunError::from(exc)),
                // do nothing, same as not returning this id
                ExternalResult::Future => {}
            }
        }

        // Check for failed tasks, but do not return immediately. The error needs to
        // flow through VM exception handling first so sandbox try/except blocks can
        // catch it. If uncaught, vm.resume_with_exception will return Err(RunError).
        let failed_task_error = vm.take_failed_task_error();

        // Push resolved value for main task if it was blocked.
        // Returns true if the main task was unblocked and a value was pushed.
        let main_task_ready = if failed_task_error.is_none() {
            vm.prepare_main_task_after_resolve()
        } else {
            // The current task failed and its frames are already restored into the VM,
            // so treat it as ready to execute exception handling.
            true
        };

        // Load a ready task if frames are empty (e.g., gather completed while
        // tasks were running and we yielded with no frames)
        let loaded_task = match vm.load_ready_task_if_needed() {
            Ok(loaded) => loaded,
            Err(e) => {
                vm.cleanup();
                #[cfg(feature = "ref-count-panic")]
                namespaces.drop_global_with_heap(&mut heap);
                return Err(e.into_python_exception(&executor.interns, &executor.code));
            }
        };

        // Check if we can continue execution.
        // If the main task wasn't unblocked, no task was loaded, and there are still frames
        // (meaning the main task is still blocked waiting for futures), we need to return
        // ResolveFutures without calling vm.run().
        if !main_task_ready && !loaded_task {
            let pending_call_ids = vm.get_pending_call_ids();
            if !pending_call_ids.is_empty() {
                let vm_state = vm.snapshot();
                let pending_call_ids: Vec<u32> = pending_call_ids.iter().map(|id| id.raw()).collect();
                return Ok(RunProgress::ResolveFutures(Self {
                    executor,
                    vm_state,
                    heap,
                    namespaces,
                    pending_call_ids,
                }));
            }
        }

        // Continue execution. If a task failed, resume via the exception path so
        // Python-level handlers can intercept it.
        let result = if let Some(error) = failed_task_error {
            vm.resume_with_exception(error)
        } else {
            vm.run()
        };

        let vm_state = vm.check_snapshot(&result);

        // Handle the result using the destructured parts
        handle_vm_result(result, vm_state, executor, heap, namespaces)
    }
}

/// Handles a FrameExit result and converts it to RunProgress for FutureSnapshot.
///
/// This is a standalone function to avoid partial move issues when destructuring FutureSnapshot.
#[cfg_attr(not(feature = "ref-count-panic"), expect(unused_mut))]
fn handle_vm_result<T: ResourceTracker>(
    result: RunResult<FrameExit>,
    vm_state: Option<VMSnapshot>,
    executor: Executor,
    mut heap: Heap<T>,
    mut namespaces: Namespaces,
) -> Result<RunProgress<T>, Exception> {
    macro_rules! new_snapshot {
        ($call_id: expr) => {
            Snapshot {
                executor,
                vm_state: vm_state.expect("snapshot should exist for ExternalCall"),
                heap,
                namespaces,
                pending_call_id: $call_id.raw(),
            }
        };
    }

    match result {
        Ok(FrameExit::Return(value)) => {
            #[cfg(feature = "ref-count-panic")]
            namespaces.drop_global_with_heap(&mut heap);

            let obj = Object::new(value, &mut heap, &executor.interns);
            Ok(RunProgress::Complete(obj))
        }
        Ok(FrameExit::ExternalCall {
            ext_function_id,
            args,
            call_id,
        }) => {
            let function_name = executor.interns.get_external_function_name(ext_function_id);
            let (args_py, kwargs_py) = args.into_py_objects(&mut heap, &executor.interns);

            Ok(RunProgress::FunctionCall {
                function_name,
                args: args_py,
                kwargs: kwargs_py,
                call_id: call_id.raw(),
                state: new_snapshot!(call_id),
            })
        }
        Ok(FrameExit::ProxyCall {
            proxy_id,
            method,
            args,
            call_id,
        }) => {
            let (args_py, kwargs_py) = args.into_py_objects(&mut heap, &executor.interns);
            Ok(RunProgress::FunctionCall {
                function_name: format!("<proxy #{}>.{method}", proxy_id.raw()),
                args: args_py,
                kwargs: kwargs_py,
                call_id: call_id.raw(),
                state: new_snapshot!(call_id),
            })
        }
        Ok(FrameExit::OsCall {
            function,
            args,
            call_id,
        }) => {
            let (args_py, kwargs_py) = args.into_py_objects(&mut heap, &executor.interns);

            Ok(RunProgress::OsCall {
                function,
                args: args_py,
                kwargs: kwargs_py,
                call_id: call_id.raw(),
                state: new_snapshot!(call_id),
            })
        }
        Ok(FrameExit::ResolveFutures(pending_call_ids)) => {
            let pending_call_ids: Vec<u32> = pending_call_ids.iter().map(|id| id.raw()).collect();
            Ok(RunProgress::ResolveFutures(FutureSnapshot {
                executor,
                vm_state: vm_state.expect("snapshot should exist for ResolveFutures"),
                heap,
                namespaces,
                pending_call_ids,
            }))
        }
        Err(err) => {
            #[cfg(feature = "ref-count-panic")]
            namespaces.drop_global_with_heap(&mut heap);

            Err(err.into_python_exception(&executor.interns, &executor.code))
        }
    }
}

/// Cached runtime state from a previous execution for reuse.
///
/// Instead of allocating a fresh Heap, Namespaces, and VM buffers for every
/// execution, we cache them after cleanup. On the next run:
/// - `Heap::reset()` clears entries but retains Vec capacity
/// - `Namespaces::reset()` clears namespace state but retains stack Vec capacity
/// - VM buffers (operand stack, exception stack) are reused directly
///
/// For short-lived programs like `1 + 2`, this eliminates ~60% of execution
/// time that was spent on allocation and deallocation overhead.
struct CachedRunState<T: ResourceTracker> {
    /// Heap with cleared state but retained allocation capacity.
    heap: Heap<T>,
    /// Namespaces with cleared state but retained stack Vec capacity.
    namespaces: Namespaces,
    /// VM operand/exception stack buffers with retained capacity.
    vm_buffers: CachedVMBuffers,
}

impl<T: ResourceTracker> std::fmt::Debug for CachedRunState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedRunState")
            .field("vm_buffers", &self.vm_buffers)
            .finish_non_exhaustive()
    }
}

/// Lower level interface to parse code and run it to completion.
///
/// This is an internal type used by [`Runner`]. It stores the compiled bytecode and source code
/// for error reporting.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Executor {
    /// Number of slots needed in the global namespace.
    namespace_size: usize,
    /// Maps variable names to their indices in the namespace. Used for ref-count testing.
    name_map: ahash::AHashMap<String, crate::namespace::NamespaceId>,
    /// Compiled bytecode for the module.
    module_code: Code,
    /// Interned strings used for looking up names and filenames during execution.
    interns: Interns,
    /// IDs to create values to inject into the the namespace to represent external functions.
    external_function_ids: Vec<ExtFunctionId>,
    /// Cached literal return for trivial modules (e.g. `1 + 2` folded to `return 3`).
    ///
    /// Used to short-circuit VM setup in `run()`/`run_no_limits_cached()` when:
    /// - there are no namespace slots,
    /// - there are no external functions,
    /// - and no runtime inputs are provided.
    constant_return: Option<Object>,
    /// Source code for error reporting (extracting preview lines for tracebacks).
    code: String,
    /// Estimated heap capacity for pre-allocation on subsequent runs.
    /// Uses AtomicUsize for thread-safety (required by PyO3's Sync bound).
    heap_capacity: AtomicUsize,
    /// Cached runtime state from the previous `run()` invocation.
    ///
    /// When present, the next `run()` call reuses the heap (via `reset()`) and VM
    /// buffers instead of allocating fresh ones. The Mutex provides interior
    /// mutability so `run()` can work with `&self`.
    ///
    /// Skipped during serialization since cached buffers are an optimization detail
    /// and cannot be meaningfully serialized (they reference freed heap memory).
    #[serde(skip)]
    cached_state: Mutex<Option<CachedRunState<NoLimitTracker>>>,
}

impl Clone for Executor {
    fn clone(&self) -> Self {
        Self {
            namespace_size: self.namespace_size,
            name_map: self.name_map.clone(),
            module_code: self.module_code.clone(),
            interns: self.interns.clone(),
            external_function_ids: self.external_function_ids.clone(),
            constant_return: self.constant_return.clone(),
            code: self.code.clone(),
            heap_capacity: AtomicUsize::new(self.heap_capacity.load(Ordering::Relaxed)),
            // Don't clone cached state - each clone starts fresh
            cached_state: Mutex::new(None),
        }
    }
}

impl Executor {
    /// Creates a new executor with the given code, filename, input names, and external functions.
    fn new(
        code: String,
        script_name: &str,
        input_names: Vec<String>,
        external_functions: Vec<String>,
    ) -> Result<Self, Exception> {
        let parse_result = parse(&code, script_name).map_err(|e| e.into_python_exc(script_name, &code))?;
        let prepared = prepare(parse_result, input_names, &external_functions)
            .map_err(|e| e.into_python_exc(script_name, &code))?;
        let constant_return = detect_constant_return(&prepared.nodes);

        // Incrementing order matches the indexes used in intern::Interns::get_external_function_name
        let external_function_ids = (0..external_functions.len()).map(ExtFunctionId::new).collect();

        // Create interns with empty functions (functions will be set after compilation)
        let mut interns = Interns::new(prepared.interner, Vec::new(), external_functions);

        // Compile the module to bytecode, which also compiles all nested functions
        let namespace_size_u16 = u16::try_from(prepared.namespace_size).expect("module namespace size exceeds u16");
        let compile_result = Compiler::compile_module(&prepared.nodes, &interns, namespace_size_u16)
            .map_err(|e| e.into_python_exc(script_name, &code))?;

        // Set the compiled functions in the interns
        interns.set_functions(compile_result.functions);

        Ok(Self {
            namespace_size: prepared.namespace_size,
            name_map: prepared.name_map,
            module_code: compile_result.code,
            interns,
            external_function_ids,
            constant_return,
            code,
            heap_capacity: AtomicUsize::new(prepared.namespace_size),
            cached_state: Mutex::new(None),
        })
    }

    /// Executes the code with a custom resource tracker.
    ///
    /// This provides full control over resource tracking and garbage collection
    /// scheduling. The tracker is called on each allocation and periodically
    /// during execution to check time limits and trigger GC.
    ///
    /// # Arguments
    /// * `inputs` - Values to fill the first N slots of the namespace
    /// * `resource_tracker` - Custom resource tracker implementation
    /// * `print` - Print implementation for print() output
    fn run(
        &self,
        inputs: Vec<Object>,
        resource_tracker: impl ResourceTracker,
        print: &mut impl PrintWriter,
    ) -> Result<Object, Exception> {
        if self.namespace_size == 0
            && self.external_function_ids.is_empty()
            && inputs.is_empty()
            && let Some(result) = &self.constant_return
        {
            return Ok(result.clone());
        }

        // Reset per-execution state (e.g. enum.auto() counter)
        enum_mod::reset_auto_counter();

        let heap_capacity = self.heap_capacity.load(Ordering::Relaxed);
        let mut heap = Heap::new(heap_capacity, resource_tracker);
        let mut namespaces = self.prepare_namespaces(inputs, &mut heap)?;

        // Create and run VM
        let mut vm = VM::new(&mut heap, &mut namespaces, &self.interns, print, NoopTracer);
        #[cfg(feature = "ref-count-panic")]
        let frame_exit_result = {
            let frame_exit_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.run_module(&self.module_code)));
            // Clean up VM state before it goes out of scope
            vm.cleanup();
            match frame_exit_result {
                Ok(result) => result,
                Err(panic) => {
                    namespaces.drop_global_with_heap(&mut heap);
                    heap.debug_dump_remaining();
                    std::panic::resume_unwind(panic);
                }
            }
        };
        #[cfg(not(feature = "ref-count-panic"))]
        let frame_exit_result = {
            let frame_exit_result = vm.run_module(&self.module_code);
            // Clean up VM state before it goes out of scope
            vm.cleanup();
            frame_exit_result
        };

        if heap.size() > heap_capacity {
            self.heap_capacity.store(heap.size(), Ordering::Relaxed);
        }

        // Clean up the global namespace before returning (only needed with ref-count-panic)
        #[cfg(feature = "ref-count-panic")]
        namespaces.drop_global_with_heap(&mut heap);

        frame_exit_to_object(frame_exit_result, &mut heap, &self.interns)
            .map_err(|e| e.into_python_exception(&self.interns, &self.code))
    }

    /// Executes the code without resource limits, reusing cached buffers when available.
    ///
    /// This is the fast path for `Runner::run_no_limits()`. On the first call, it
    /// behaves identically to `run()` with `NoLimitTracker`. On subsequent calls, it
    /// reuses the Heap (via `reset()`), Namespaces (via `reset()`), and VM buffers
    /// from the previous execution, avoiding the dominant allocation/deallocation overhead.
    ///
    /// Profiling showed that for simple programs like `1 + 2`, VM setup/teardown
    /// consumed ~63% of execution time. Buffer reuse eliminates most of that overhead.
    fn run_no_limits_cached(&self, inputs: Vec<Object>, print: &mut impl PrintWriter) -> Result<Object, Exception> {
        if self.namespace_size == 0
            && self.external_function_ids.is_empty()
            && inputs.is_empty()
            && let Some(result) = &self.constant_return
        {
            return Ok(result.clone());
        }

        // Reset per-execution state (e.g. enum.auto() counter)
        enum_mod::reset_auto_counter();

        // Try to take cached state from the previous run using try_lock (non-blocking).
        // If another thread holds the lock, we skip caching and allocate fresh.
        // try_lock is much cheaper than lock (~5ns vs ~25ns on macOS) for the
        // uncontended case, which matters when total execution is ~100ns.
        let cached = self.cached_state.try_lock().ok().and_then(|mut guard| guard.take());

        let (mut heap, cached_namespaces, vm_buffers) = if let Some(mut state) = cached {
            // Reuse the heap by resetting it (clears entries but retains Vec capacity)
            state.heap.reset(NoLimitTracker);
            (state.heap, Some(state.namespaces), Some(state.vm_buffers))
        } else {
            // First run or cache miss: allocate fresh
            let heap_capacity = self.heap_capacity.load(Ordering::Relaxed);
            (Heap::new(heap_capacity, NoLimitTracker), None, None)
        };

        // Build namespaces, reusing the inner Vec when cached to avoid allocation
        let mut namespaces = if let Some(mut ns) = cached_namespaces {
            // Reuse the cached Namespaces: reset clears state and returns the
            // global namespace's inner Vec (cleared but capacity retained).
            let ns_vec = ns.reset_global();
            self.fill_namespace_values(inputs, &mut heap, ns_vec)?;
            ns
        } else {
            let namespace_values = self.prepare_namespace_values(inputs, &mut heap)?;
            Namespaces::new(namespace_values)
        };

        // Create VM, reusing buffers if available
        let mut vm = if let Some(buffers) = vm_buffers {
            VM::new_with_buffers(buffers, &mut heap, &mut namespaces, &self.interns, print, NoopTracer)
        } else {
            VM::new(&mut heap, &mut namespaces, &self.interns, print, NoopTracer)
        };

        #[cfg(feature = "ref-count-panic")]
        let (frame_exit_result, vm_buffers) = {
            let frame_exit_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.run_module(&self.module_code)));

            // Clean up VM state
            vm.cleanup();

            // Extract reusable buffers from the cleaned-up VM
            let vm_buffers = vm.take_buffers();
            let frame_exit_result = match frame_exit_result {
                Ok(result) => result,
                Err(panic) => {
                    namespaces.drop_global_with_heap(&mut heap);
                    heap.debug_dump_remaining();
                    std::panic::resume_unwind(panic);
                }
            };
            (frame_exit_result, vm_buffers)
        };
        #[cfg(not(feature = "ref-count-panic"))]
        let (frame_exit_result, vm_buffers) = {
            let frame_exit_result = vm.run_module(&self.module_code);

            // Clean up VM state
            vm.cleanup();

            // Extract reusable buffers from the cleaned-up VM
            let vm_buffers = vm.take_buffers();
            (frame_exit_result, vm_buffers)
        };

        // Update heap capacity hint for future runs
        let heap_capacity = self.heap_capacity.load(Ordering::Relaxed);
        if heap.size() > heap_capacity {
            self.heap_capacity.store(heap.size(), Ordering::Relaxed);
        }

        // Clean up the global namespace before returning (only needed with ref-count-panic)
        #[cfg(feature = "ref-count-panic")]
        namespaces.drop_global_with_heap(&mut heap);

        let result = frame_exit_to_object(frame_exit_result, &mut heap, &self.interns)
            .map_err(|e| e.into_python_exception(&self.interns, &self.code));

        // Cache the heap, namespaces, and VM buffers for the next run.
        // We do this after frame_exit_to_object because that function may need the heap
        // for Object conversion, but before returning so the cache is available.
        if let Ok(mut guard) = self.cached_state.try_lock() {
            *guard = Some(CachedRunState {
                heap,
                namespaces,
                vm_buffers,
            });
        }

        result
    }

    /// Executes the code and returns both the result and reference count data, used for testing only.
    ///
    /// This is used for testing reference counting behavior. Returns:
    /// - The execution result (`Exit`)
    /// - Reference count data as a tuple of:
    ///   - A map from variable names to their reference counts (only for heap-allocated values)
    ///   - The number of unique heap value IDs referenced by variables
    ///   - The total number of live heap values
    ///
    /// For strict matching validation, compare unique_refs_count with heap_entry_count.
    /// If they're equal, all heap values are accounted for by named variables.
    ///
    /// Only available when the `ref-count-return` feature is enabled.
    #[cfg(feature = "ref-count-return")]
    fn run_ref_counts(&self, inputs: Vec<Object>) -> Result<RefCountOutput, Exception> {
        // Reset per-execution state (e.g. enum.auto() counter)
        enum_mod::reset_auto_counter();

        let mut heap = Heap::new(self.namespace_size, NoLimitTracker);
        let mut namespaces = self.prepare_namespaces(inputs, &mut heap)?;

        // Create and run VM with StdPrint for output
        let mut print = StdPrint;
        let mut vm = VM::new(&mut heap, &mut namespaces, &self.interns, &mut print, NoopTracer);
        let frame_exit_result = vm.run_module(&self.module_code);

        // Compute ref counts before consuming the heap - return value is still alive
        let final_namespace = namespaces.into_global();
        let mut counts = ahash::AHashMap::new();
        let mut unique_ids = HashSet::new();

        for (name, &namespace_id) in &self.name_map {
            if let Some(Value::Ref(id)) = final_namespace.get_opt(namespace_id) {
                counts.insert(name.clone(), heap.get_refcount(*id));
                unique_ids.insert(*id);
            }
        }
        let unique_refs = unique_ids.len();
        let heap_count = heap.entry_count();

        // Clean up the namespace after reading ref counts but before moving the heap
        for obj in final_namespace {
            obj.drop_with_heap(&mut heap);
        }

        // Now convert the return value to Object (this drops the Value, decrementing refcount)
        let py_object = frame_exit_to_object(frame_exit_result, &mut heap, &self.interns)
            .map_err(|e| e.into_python_exception(&self.interns, &self.code))?;

        let allocations_since_gc = heap.get_allocations_since_gc();

        Ok(RefCountOutput {
            py_object,
            counts,
            unique_refs,
            heap_count,
            allocations_since_gc,
        })
    }

    /// Prepares the namespace namespaces for execution.
    ///
    /// Converts each `Object` input to a `Value`, allocating on the heap if needed.
    /// Returns the prepared Namespaces or an error if there are too many inputs or invalid input types.
    fn prepare_namespaces(
        &self,
        inputs: Vec<Object>,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Namespaces, Exception> {
        let namespace = self.prepare_namespace_values(inputs, heap)?;
        Ok(Namespaces::new(namespace))
    }

    /// Builds the global namespace values vector without wrapping in Namespaces.
    ///
    /// This is the core logic shared between `prepare_namespaces` (which creates fresh
    /// Namespaces) and the cached path (which reuses existing Namespaces via `reset()`).
    ///
    /// Fills the namespace with external function stubs, converted input values,
    /// and `Undefined` padding to reach the expected namespace size.
    fn prepare_namespace_values(
        &self,
        inputs: Vec<Object>,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<Vec<Value>, Exception> {
        let Some(extra) = self
            .namespace_size
            .checked_sub(self.external_function_ids.len() + inputs.len())
        else {
            return Err(Exception::runtime_error("too many inputs for namespace"));
        };
        // register external functions in the namespace first, matching the logic in prepare
        let mut namespace: Vec<Value> = Vec::with_capacity(self.namespace_size);
        for f_id in &self.external_function_ids {
            namespace.push(Value::ExtFunction(*f_id));
        }
        // Convert each Object to a Value, propagating any invalid input errors
        for input in inputs {
            namespace.push(
                input
                    .to_value(heap, &self.interns)
                    .map_err(|e| Exception::runtime_error(format!("invalid input type: {e}")))?,
            );
        }
        if extra > 0 {
            namespace.extend((0..extra).map(|_| Value::Undefined));
        }
        Ok(namespace)
    }

    /// Fills an existing namespace Vec in-place with the global namespace values.
    ///
    /// This is the zero-allocation variant of `prepare_namespace_values`, used when
    /// reusing a cached Namespaces. The Vec should be pre-cleared by the caller
    /// (via `Namespaces::reset_global()`), and its existing capacity is reused.
    fn fill_namespace_values(
        &self,
        inputs: Vec<Object>,
        heap: &mut Heap<impl ResourceTracker>,
        namespace: &mut Vec<Value>,
    ) -> Result<(), Exception> {
        let Some(extra) = self
            .namespace_size
            .checked_sub(self.external_function_ids.len() + inputs.len())
        else {
            return Err(Exception::runtime_error("too many inputs for namespace"));
        };
        for f_id in &self.external_function_ids {
            namespace.push(Value::ExtFunction(*f_id));
        }
        for input in inputs {
            namespace.push(
                input
                    .to_value(heap, &self.interns)
                    .map_err(|e| Exception::runtime_error(format!("invalid input type: {e}")))?,
            );
        }
        if extra > 0 {
            namespace.extend((0..extra).map(|_| Value::Undefined));
        }
        Ok(())
    }
}

pub(crate) fn frame_exit_to_object(
    frame_exit_result: RunResult<FrameExit>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Object> {
    match frame_exit_result? {
        FrameExit::Return(return_value) => Ok(Object::new(return_value, heap, interns)),
        FrameExit::ExternalCall { ext_function_id, .. } => {
            let function_name = interns.get_external_function_name(ext_function_id);
            Err(ExcType::not_implemented(format!(
                "External function '{function_name}' not implemented with standard execution"
            ))
            .into())
        }
        FrameExit::ProxyCall { proxy_id, method, .. } => Err(ExcType::not_implemented(format!(
            "Proxy call on '<proxy #{}>.{}' not implemented with standard execution",
            proxy_id.raw(),
            method
        ))
        .into()),
        FrameExit::OsCall { function, .. } => Err(ExcType::not_implemented(format!(
            "OS function '{function}' not implemented with standard execution"
        ))
        .into()),
        FrameExit::ResolveFutures(_) => {
            Err(ExcType::not_implemented("async futures not supported by standard execution.").into())
        }
    }
}

/// Detects modules that are a single literal return and can bypass VM execution.
///
/// This only recognizes side-effect-free literal returns from the prepared module
/// body. More complex programs continue through the normal VM path.
fn detect_constant_return(nodes: &[PreparedNode]) -> Option<Object> {
    let [Node::Return(expr_loc)] = nodes else {
        return None;
    };
    let Expr::Literal(literal) = &expr_loc.expr else {
        return None;
    };
    match literal {
        Literal::Ellipsis => Some(Object::Ellipsis),
        Literal::None => Some(Object::None),
        Literal::Bool(value) => Some(Object::Bool(*value)),
        Literal::Int(value) => Some(Object::Int(*value)),
        Literal::Float(value) => Some(Object::Float(*value)),
        _ => None,
    }
}

/// Output from `run_ref_counts` containing reference count and heap information.
///
/// Used for testing GC behavior and reference counting correctness.
#[cfg(feature = "ref-count-return")]
#[derive(Debug)]
pub struct RefCountOutput {
    pub py_object: Object,
    pub counts: ahash::AHashMap<String, usize>,
    pub unique_refs: usize,
    pub heap_count: usize,
    /// Number of GC-tracked allocations since the last garbage collection.
    ///
    /// If GC ran during execution, this will be lower than the total number of
    /// allocations. Compare this against expected allocation count to verify GC ran.
    pub allocations_since_gc: u32,
}
