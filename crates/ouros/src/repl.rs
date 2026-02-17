//! Persistent REPL session support for Ouros.
//!
//! `ReplSession` keeps interpreter state across `execute()` calls so interactive
//! snippets can share variables, functions, classes, and heap objects.

use std::time::Instant;

use ahash::AHashMap;
use num_bigint::BigInt;

use crate::{
    asyncio::CallId,
    bytecode::{Code, Compiler, FrameExit, VM, VMSnapshot},
    capability::CapabilitySet,
    exception_private::ExcType,
    exception_public::Exception,
    function::Function,
    heap::HeapStats,
    intern::{ExtFunctionId, InternerBuilder, Interns},
    io::PrintWriter,
    namespace::{GLOBAL_NS_IDX, NamespaceId, Namespaces},
    object::{InvalidInputError, Object},
    parse,
    prepare::prepare_repl,
    repl_error::ReplError,
    resource::{LimitedTracker, NoLimitTracker, ResourceLimits},
    run::{ExternalResult, frame_exit_to_object},
    tracer::NoopTracer,
    types::PyTrait,
    value::Value,
};

/// Metadata for a single pending external future.
///
/// Carries the original function name and arguments alongside the call ID,
/// making `ResolveFutures` responses self-contained. Hosts (especially LLMs)
/// can correlate each `call_id` with the function that produced it without
/// maintaining their own mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingFutureInfo {
    /// Correlation ID for the pending external call.
    pub call_id: u32,
    /// Name of the external function that was called.
    pub function_name: String,
    /// Positional arguments that were passed to the function.
    pub args: Vec<Object>,
}

/// Result of interactive REPL execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplProgress {
    /// Execution paused at an external function call.
    ///
    /// The host should fulfill the call and continue by calling `ReplSession::resume`.
    FunctionCall {
        /// Name of the external function to call.
        function_name: String,
        /// Positional arguments passed by the Python code.
        args: Vec<Object>,
        /// Keyword arguments passed by the Python code.
        kwargs: Vec<(Object, Object)>,
        /// Correlation ID for the call.
        call_id: u32,
    },
    /// Execution paused at an operation against a host-managed proxy value.
    ///
    /// The host should resolve the operation and continue by calling
    /// `ReplSession::resume`.
    ProxyCall {
        /// Host-managed proxy identifier.
        proxy_id: u32,
        /// Accessed attribute/method name.
        method: String,
        /// Positional arguments passed to the proxy operation.
        args: Vec<Object>,
        /// Keyword arguments passed to the proxy operation.
        kwargs: Vec<(Object, Object)>,
        /// Correlation ID for the call.
        call_id: u32,
    },
    /// Execution paused with pending external futures that need resolution.
    ///
    /// Multiple external calls were made asynchronously (via `resume` with
    /// `ExternalResult::Future`). The host should resolve some or all pending
    /// calls and continue by calling `ReplSession::resume_futures`.
    ResolveFutures {
        /// Call IDs of the pending external futures (backward compat).
        pending_call_ids: Vec<u32>,
        /// Enriched metadata for each pending future, including the original
        /// function name and arguments. Same length as `pending_call_ids`.
        pending_futures: Vec<PendingFutureInfo>,
    },
    /// Execution completed with a result.
    Complete(Object),
}

/// State needed to resume a paused interactive REPL execution.
struct PendingResumeState {
    /// Module bytecode for the currently running snippet.
    module_code: Code,
    /// Source text of the running snippet, used for traceback formatting.
    source: String,
    /// Call ID that produced the current snapshot.
    call_id: u32,
    /// Name of the external function being called (stored for future metadata).
    function_name: String,
    /// Positional arguments passed to the external function (stored for future metadata).
    args: Vec<Object>,
}

/// State needed to resume after multiple async futures are pending.
struct PendingFuturesState {
    /// Module bytecode for the currently running snippet.
    module_code: Code,
    /// Source text of the running snippet.
    source: String,
    /// Pending call IDs that need resolution.
    pending_call_ids: Vec<u32>,
    /// Enriched metadata for each pending future (function name, args).
    /// Same length and order as `pending_call_ids`.
    pending_futures: Vec<PendingFutureInfo>,
}

/// Serializable representation of a full REPL session for disk persistence.
///
/// Packs all session components into a single struct. The heap and namespaces
/// are stored as postcard-encoded byte vectors (matching the `deep_clone()`
/// serialization format), while other fields are stored directly.
///
/// Pending interactive state (snapshots, resume state) is intentionally excluded --
/// sessions must be idle (not mid-yield) to be saved. Capabilities are also
/// excluded as a security measure -- the host re-applies them on load.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SessionSnapshot {
    /// Dynamically interned strings (excludes pre-interned base set).
    interner_strings: Vec<String>,
    /// Interned bytes literals.
    interner_bytes: Vec<Vec<u8>>,
    /// Interned big integer literals.
    interner_long_ints: Vec<BigInt>,
    /// Compiled function table.
    functions: Vec<Function>,
    /// External function names registered for this session.
    external_functions: Vec<String>,
    /// Postcard-serialized heap bytes.
    heap_bytes: Vec<u8>,
    /// Postcard-serialized namespace bytes.
    namespaces_bytes: Vec<u8>,
    /// Variable name to namespace slot mapping, flattened for serialization.
    name_map: Vec<(String, NamespaceId)>,
    /// Current global namespace size.
    namespace_size: usize,
    /// Number of external function slots at start of namespace.
    external_function_count: usize,
    /// Script name for error reporting.
    script_name: String,
}

/// A persistent REPL session that executes code against accumulated interpreter state.
///
/// The session owns a long-lived heap, namespace table, interner state, and compiled
/// function table. Each `execute()` call parses, prepares, compiles, and runs a new
/// snippet while preserving prior successful state.
///
/// # Known Limitations
/// - `del` for very high local slots (>255) still depends on `DeleteLocalW` support.
/// - `global x` inside a function does not dynamically discover globals defined in
///   later REPL lines (global snapshot behavior from prepare-time resolution).
pub struct ReplSession {
    /// Append-only interner state shared across REPL lines.
    interner: InternerBuilder,
    /// Accumulated compiled functions, indexed by `FunctionId`.
    functions: Vec<Function>,
    /// External function names registered for this session.
    external_functions: Vec<String>,
    /// Persistent heap backing all runtime objects.
    heap: crate::heap::Heap<NoLimitTracker>,
    /// Persistent namespace storage (global namespace at index 0).
    namespaces: Namespaces,
    /// Stable mapping from variable names to global namespace slots.
    name_map: AHashMap<String, NamespaceId>,
    /// Current size of the global namespace.
    namespace_size: usize,
    /// Number of external function slots at the start of the namespace.
    external_function_count: usize,
    /// Script name used in parse/runtime error reporting.
    script_name: String,
    /// VM snapshot waiting to be resumed after an interactive yield.
    pending_snapshot: Option<VMSnapshot>,
    /// Resume metadata paired with `pending_snapshot`.
    pending_resume_state: Option<PendingResumeState>,
    /// Futures resolution state paired with `pending_snapshot`.
    pending_futures_state: Option<PendingFuturesState>,
    /// Optional capability set controlling which external operations are permitted.
    ///
    /// When `None`, all operations are allowed (backwards-compatible default).
    /// When `Some`, external function calls and proxy operations are checked against
    /// the capability set at the yield boundary. Denied operations return a
    /// `PermissionError` to the Python code.
    capabilities: Option<CapabilitySet>,
    /// Accumulated metadata for pending external futures.
    ///
    /// Populated by `resume(ExternalResult::Future)` calls -- each one records
    /// the function name and args for the call that was deferred. Consumed when
    /// `ResolveFutures` is returned to the host, and stored in
    /// `PendingFuturesState` for incremental resolution.
    future_metadata: AHashMap<u32, PendingFutureInfo>,
}

impl ReplSession {
    /// Creates a new REPL session with optional external function bindings.
    ///
    /// External functions are pre-bound into the first N global namespace slots
    /// and remain available for all subsequent `execute()` calls.
    #[must_use]
    pub fn new(external_functions: Vec<String>, script_name: &str) -> Self {
        Self::new_with_resource_limits(external_functions, script_name, ResourceLimits::default())
    }

    /// Creates a new REPL session with persistent resource limits.
    ///
    /// The limits are enforced for every execution step (`execute`, interactive
    /// `execute`, and `resume`) while preserving persistent REPL state between
    /// calls.
    #[must_use]
    pub fn new_with_resource_limits(
        external_functions: Vec<String>,
        script_name: &str,
        resource_limits: ResourceLimits,
    ) -> Self {
        let external_function_ids: Vec<ExtFunctionId> = (0..external_functions.len()).map(ExtFunctionId::new).collect();
        let mut name_map = AHashMap::with_capacity(external_functions.len());
        let mut namespace_values = Vec::with_capacity(external_functions.len());

        for (index, function_name) in external_functions.iter().enumerate() {
            name_map.insert(function_name.clone(), NamespaceId::new(index));
            namespace_values.push(Value::ExtFunction(external_function_ids[index]));
        }

        let namespace_size = namespace_values.len();

        Self {
            interner: InternerBuilder::new(""),
            functions: Vec::new(),
            external_functions,
            heap: crate::heap::Heap::new(namespace_size.max(16), NoLimitTracker::with_limits(resource_limits)),
            namespaces: Namespaces::new(namespace_values),
            name_map,
            namespace_size,
            external_function_count: external_function_ids.len(),
            script_name: script_name.to_string(),
            pending_snapshot: None,
            pending_resume_state: None,
            pending_futures_state: None,
            capabilities: None,
            future_metadata: AHashMap::new(),
        }
    }

    /// Sets the capability set for this session.
    ///
    /// When set, external function calls and proxy operations are checked against
    /// the capability set at the yield boundary. Operations not in the set are
    /// denied with a `PermissionError`.
    ///
    /// Pass `None` to allow all operations (the default).
    pub fn set_capabilities(&mut self, capabilities: Option<CapabilitySet>) {
        self.capabilities = capabilities;
    }

    /// Returns the current capability set, if any.
    #[must_use]
    pub fn capabilities(&self) -> Option<&CapabilitySet> {
        self.capabilities.as_ref()
    }

    /// Creates an independent deep copy of this session.
    ///
    /// The forked session has its own heap, interns, namespaces, and function table.
    /// Changes to either session do not affect the other. Useful for branching
    /// execution to try different code paths from the same state.
    ///
    /// Pending interactive state (`pending_snapshot` / `pending_resume_state` /
    /// `pending_futures_state`) is **not** cloned — the forked session starts in a
    /// clean, non-yielded execution state. If the original session is mid-yield, the fork can still execute new
    /// code independently.
    ///
    /// The fork inherits the same capability restrictions as the original. A forked
    /// session with `capabilities = None` (allow-all) is fine if the original also
    /// had `capabilities = None`.
    ///
    /// # Panics
    ///
    /// Panics if internal serialization round-trip fails, which should not happen
    /// for well-formed session state.
    #[must_use]
    pub fn fork(&self) -> Self {
        Self {
            interner: self.interner.clone(),
            functions: self.functions.clone(),
            external_functions: self.external_functions.clone(),
            heap: self.heap.deep_clone(),
            namespaces: self.namespaces.deep_clone(),
            name_map: self.name_map.clone(),
            namespace_size: self.namespace_size,
            external_function_count: self.external_function_count,
            script_name: self.script_name.clone(),
            pending_snapshot: None,
            pending_resume_state: None,
            pending_futures_state: None,
            capabilities: self.capabilities.clone(),
            future_metadata: AHashMap::new(),
        }
    }

    /// Serializes the full session state to bytes for disk persistence.
    ///
    /// Returns postcard-encoded bytes containing all session state: heap,
    /// namespaces, interner, compiled functions, and variable mappings.
    ///
    /// # Errors
    ///
    /// Returns an error if the session has pending interactive state (mid-yield).
    /// The caller should `resume` any pending calls before saving.
    pub fn save(&self) -> Result<Vec<u8>, String> {
        if self.pending_snapshot.is_some()
            || self.pending_resume_state.is_some()
            || self.pending_futures_state.is_some()
        {
            return Err("cannot save session with pending interactive state -- resume first".to_owned());
        }

        let heap_bytes = postcard::to_allocvec(&self.heap).map_err(|e| format!("heap serialization failed: {e}"))?;
        let namespaces_bytes =
            postcard::to_allocvec(&self.namespaces).map_err(|e| format!("namespace serialization failed: {e}"))?;

        let (interner_strings, interner_bytes, interner_long_ints) = self.interner.clone_data();

        let snapshot = SessionSnapshot {
            interner_strings,
            interner_bytes,
            interner_long_ints,
            functions: self.functions.clone(),
            external_functions: self.external_functions.clone(),
            heap_bytes,
            namespaces_bytes,
            name_map: self.name_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            namespace_size: self.namespace_size,
            external_function_count: self.external_function_count,
            script_name: self.script_name.clone(),
        };

        postcard::to_allocvec(&snapshot).map_err(|e| format!("session serialization failed: {e}"))
    }

    /// Restores a session from bytes previously produced by `save()`.
    ///
    /// Reconstructs the full session state including heap, namespaces, interner,
    /// and compiled functions. The restored session starts in an idle state with
    /// no pending interactive operations and no capability restrictions.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Postcard-encoded session snapshot bytes from `save()`.
    /// * `resource_limits` - Resource limits to apply to the restored session.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails (corrupt or incompatible data).
    pub fn load(bytes: &[u8], _resource_limits: ResourceLimits) -> Result<Self, String> {
        let snapshot: SessionSnapshot =
            postcard::from_bytes(bytes).map_err(|e| format!("session deserialization failed: {e}"))?;

        let heap: crate::heap::Heap<NoLimitTracker> =
            postcard::from_bytes(&snapshot.heap_bytes).map_err(|e| format!("heap deserialization failed: {e}"))?;
        let namespaces: Namespaces = postcard::from_bytes(&snapshot.namespaces_bytes)
            .map_err(|e| format!("namespace deserialization failed: {e}"))?;

        let interner = InternerBuilder::from_parts(
            snapshot.interner_strings,
            snapshot.interner_bytes,
            snapshot.interner_long_ints,
        );

        let name_map: AHashMap<String, NamespaceId> = snapshot.name_map.into_iter().collect();

        Ok(Self {
            interner,
            functions: snapshot.functions,
            external_functions: snapshot.external_functions,
            heap,
            namespaces,
            name_map,
            namespace_size: snapshot.namespace_size,
            external_function_count: snapshot.external_function_count,
            script_name: snapshot.script_name,
            pending_snapshot: None,
            pending_resume_state: None,
            pending_futures_state: None,
            capabilities: None,
            future_metadata: AHashMap::new(),
        })
    }

    /// Executes a snippet without additional resource limits.
    ///
    /// The snippet runs in the current session context and may mutate persistent
    /// state (global variables, heap objects, and function table).
    pub fn execute(&mut self, code: &str, print: &mut impl PrintWriter) -> Result<Object, ReplError> {
        self.ensure_not_waiting_for_resume()?;
        self.execute_inner(code, None, print)
    }

    /// Executes a snippet with a per-call time limit.
    ///
    /// The provided `LimitedTracker` is used only to extract `max_duration`; the
    /// session keeps using its persistent `Heap<NoLimitTracker>`.
    pub fn execute_with_limits(
        &mut self,
        code: &str,
        tracker: LimitedTracker,
        print: &mut impl PrintWriter,
    ) -> Result<Object, ReplError> {
        self.ensure_not_waiting_for_resume()?;
        let deadline = tracker.max_duration().map(|limit| Instant::now() + limit);
        self.execute_inner(code, deadline, print)
    }

    /// Executes code that may yield for external function or proxy calls.
    ///
    /// This mirrors `Runner::start`: when the VM needs host input, execution pauses
    /// and returns `ReplProgress::FunctionCall` or `ReplProgress::ProxyCall`. The host
    /// should call `resume()` with the resolved value or error.
    pub fn execute_interactive(&mut self, code: &str, print: &mut impl PrintWriter) -> Result<ReplProgress, ReplError> {
        self.ensure_not_waiting_for_resume()?;
        // Clear any leftover future metadata from prior interactive execution.
        self.future_metadata.clear();
        self.execute_inner_interactive(code, None, print)
    }

    /// Resumes a paused interactive execution after a host-resolved call.
    ///
    /// The supplied result is pushed into the paused VM. Execution may complete or
    /// yield again for another host call.
    pub fn resume(
        &mut self,
        result: impl Into<ExternalResult>,
        print: &mut impl PrintWriter,
    ) -> Result<ReplProgress, ReplError> {
        let Some(snapshot) = self.pending_snapshot.take() else {
            return Err(self.pending_resume_error());
        };
        let Some(pending_state) = self.pending_resume_state.take() else {
            return Err(self.pending_resume_error());
        };

        let runtime_interns =
            Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());
        let ext_result = result.into();
        self.heap.tracker_mut().begin_execution(None);

        let (frame_exit_result, vm_snapshot) = {
            let mut vm = VM::restore(
                snapshot,
                &pending_state.module_code,
                &mut self.heap,
                &mut self.namespaces,
                &runtime_interns,
                print,
                NoopTracer,
            );
            let vm_result = match ext_result {
                ExternalResult::Return(obj) => vm.resume(obj),
                ExternalResult::Error(exc) => vm.resume_with_exception(exc.into()),
                ExternalResult::Future => {
                    let call_id = CallId::new(pending_state.call_id);
                    vm.add_pending_call(call_id);
                    // Record metadata for this future so ResolveFutures can include it.
                    self.future_metadata.insert(
                        pending_state.call_id,
                        PendingFutureInfo {
                            call_id: pending_state.call_id,
                            function_name: pending_state.function_name.clone(),
                            args: pending_state.args.clone(),
                        },
                    );
                    vm.push(Value::ExternalFuture(call_id));
                    vm.run()
                }
            };
            let snapshot = vm.check_snapshot(&vm_result);
            (vm_result, snapshot)
        };

        let progress = self.handle_interactive_frame_exit(
            frame_exit_result,
            vm_snapshot,
            pending_state.module_code,
            pending_state.source,
            &runtime_interns,
        );

        self.finish_interactive_step(progress)
    }

    /// Resumes execution with results for some or all pending async futures.
    ///
    /// Accepts a list of `(call_id, ExternalResult)` pairs. Supports incremental
    /// resolution -- provide a subset and Ouros will continue until blocked again,
    /// returning `ResolveFutures` with the remaining pending calls.
    ///
    /// # Errors
    ///
    /// Returns `ReplError` if:
    /// - No futures are pending (no prior `ResolveFutures` progress).
    /// - A provided `call_id` is not in the pending set.
    /// - A runtime error occurs during continued execution.
    pub fn resume_futures(
        &mut self,
        results: Vec<(u32, ExternalResult)>,
        print: &mut impl PrintWriter,
    ) -> Result<ReplProgress, ReplError> {
        let Some(snapshot) = self.pending_snapshot.take() else {
            return Err(self.pending_resume_error());
        };
        let Some(futures_state) = self.pending_futures_state.take() else {
            return Err(self.pending_resume_error());
        };

        let runtime_interns =
            Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());
        self.heap.tracker_mut().begin_execution(None);

        let (frame_exit_result, vm_snapshot) = {
            let mut vm = VM::restore(
                snapshot,
                &futures_state.module_code,
                &mut self.heap,
                &mut self.namespaces,
                &runtime_interns,
                print,
                NoopTracer,
            );

            // Validate all call_ids before resolving
            let invalid_call_id = results
                .iter()
                .find(|(call_id, _)| !futures_state.pending_call_ids.contains(call_id))
                .map(|(call_id, _)| *call_id);

            if let Some(invalid_id) = invalid_call_id {
                vm.cleanup();
                return Err(ReplError::Runtime(Exception::runtime_error(format!(
                    "unknown call_id {invalid_id}, expected one of: {:?}",
                    futures_state.pending_call_ids
                ))));
            }

            // Resolve each future in the VM
            for (call_id, ext_result) in results {
                match ext_result {
                    ExternalResult::Return(obj) => {
                        if let Err(e) = vm.resolve_future(call_id, obj) {
                            vm.cleanup();
                            return Err(ReplError::Runtime(Exception::runtime_error(format!(
                                "Invalid return type for call {call_id}: {e}"
                            ))));
                        }
                    }
                    ExternalResult::Error(exc) => {
                        vm.fail_future(call_id, crate::exception_private::RunError::from(exc));
                    }
                    ExternalResult::Future => {}
                }
            }

            // Check for failed tasks -- take the error but don't return it yet.
            // The error needs to flow through the VM's exception handling so that
            // Python try/except blocks can catch it. If uncaught, vm.run() will
            // propagate it as Err(RunError) which we convert to ReplError::Runtime.
            let failed_task_error = vm.take_failed_task_error();

            // Push resolved value for main task if blocked (only meaningful when
            // there's no failure -- a failed task won't have a resolved value).
            let main_task_ready = if failed_task_error.is_none() {
                vm.prepare_main_task_after_resolve()
            } else {
                // The main task failed; its frames are already in the VM from
                // VM::restore, so we treat it as "ready" for execution purposes.
                true
            };

            // Load a ready task if needed
            let loaded_task = match vm.load_ready_task_if_needed() {
                Ok(loaded) => loaded,
                Err(e) => {
                    vm.cleanup();
                    return Err(ReplError::Runtime(
                        e.into_python_exception(&runtime_interns, futures_state.source.as_str()),
                    ));
                }
            };

            // If still blocked, return ResolveFutures without running
            if !main_task_ready && !loaded_task {
                let pending = vm.get_pending_call_ids();
                if !pending.is_empty() {
                    let vm_state = vm.snapshot();
                    let pending_call_ids: Vec<u32> = pending.iter().map(|id| id.raw()).collect();
                    let pending_futures = self.build_pending_futures(&pending_call_ids);
                    self.pending_snapshot = Some(vm_state);
                    self.pending_futures_state = Some(PendingFuturesState {
                        module_code: futures_state.module_code,
                        source: futures_state.source,
                        pending_call_ids: pending_call_ids.clone(),
                        pending_futures: pending_futures.clone(),
                    });
                    self.heap.tracker_mut().set_deadline(None);
                    return Ok(ReplProgress::ResolveFutures {
                        pending_call_ids,
                        pending_futures,
                    });
                }
            }

            // Continue execution -- if a task failed, raise the exception through
            // the VM's normal exception handling (which checks try/except handlers).
            // If uncaught, it propagates as Err(RunError).
            let result = if let Some(error) = failed_task_error {
                vm.resume_with_exception(error)
            } else {
                vm.run()
            };
            let snapshot = vm.check_snapshot(&result);
            (result, snapshot)
        };

        let progress = self.handle_interactive_frame_exit(
            frame_exit_result,
            vm_snapshot,
            futures_state.module_code,
            futures_state.source,
            &runtime_interns,
        );

        self.finish_interactive_step(progress)
    }

    /// Returns the script name configured for this session.
    #[must_use]
    pub fn script_name(&self) -> &str {
        &self.script_name
    }

    /// Returns a snapshot of the current heap state.
    ///
    /// The snapshot includes live object counts, free slot counts, per-type
    /// breakdowns, and interned string counts. Useful for monitoring heap
    /// growth across REPL interactions and comparing states via diffs.
    #[must_use]
    pub fn heap_stats(&self) -> HeapStats {
        self.heap.heap_stats(self.interner.interned_string_count())
    }

    /// Lists defined global variables and their type names.
    ///
    /// Undefined slots and external-function slots are excluded.
    #[must_use]
    pub fn list_variables(&self) -> Vec<(String, String)> {
        let global = self.namespaces.get(GLOBAL_NS_IDX);
        let mut vars = Vec::new();

        for (name, &slot) in &self.name_map {
            if slot.index() < self.external_function_count {
                continue;
            }
            let value = global.get(slot);
            if matches!(value, Value::Undefined) {
                continue;
            }
            vars.push((name.clone(), value.py_type(&self.heap).to_string()));
        }

        vars.sort_unstable_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
        vars
    }

    /// Returns the current value of a named global variable.
    ///
    /// Returns `None` when the name is unknown or currently undefined.
    #[must_use]
    pub fn get_variable(&self, name: &str) -> Option<Object> {
        let &slot = self.name_map.get(name)?;
        if slot.index() < self.external_function_count {
            return None;
        }

        let value = self.namespaces.get(GLOBAL_NS_IDX).get(slot);
        if matches!(value, Value::Undefined) {
            return None;
        }

        let interns = Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());
        Some(Object::from_borrowed_value(value, &self.heap, &interns))
    }

    /// Returns the Python repr string for a named global variable.
    ///
    /// This method properly invokes custom `__repr__` methods on class instances
    /// by executing `repr(varname)` as Python code. For variables without custom
    /// repr, it falls back to the default `py_repr_fmt` behavior.
    ///
    /// Returns `None` when the name is unknown or currently undefined.
    ///
    /// Note: This method clones the session to avoid modifying the original state.
    #[must_use]
    pub fn get_variable_repr(&self, name: &str) -> Option<String> {
        use crate::io::NoPrint;

        // First check if the variable exists
        let &slot = self.name_map.get(name)?;
        if slot.index() < self.external_function_count {
            return None;
        }

        let value = self.namespaces.get(GLOBAL_NS_IDX).get(slot);
        if matches!(value, Value::Undefined) {
            return None;
        }

        // Clone the session to execute repr() without modifying original state
        let mut cloned = self.fork();

        // Execute repr(varname) to get the Python-level repr
        let code = format!("repr({name})");
        let mut print = NoPrint;
        if let Ok(result) = cloned.execute(&code, &mut print) {
            // Extract the string from the Object
            if let crate::object::Object::String(s) = result {
                Some(s)
            } else {
                // Unexpected: repr() should always return a string
                // Fall back to default formatting
                let interns =
                    Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());
                let mut repr_string = String::new();
                let mut heap_ids = ahash::AHashSet::new();
                value
                    .py_repr_fmt(&mut repr_string, &self.heap, &mut heap_ids, &interns)
                    .ok()?;
                Some(repr_string)
            }
        } else {
            // If repr() fails, fall back to default formatting
            let interns =
                Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());
            let mut repr_string = String::new();
            let mut heap_ids = ahash::AHashSet::new();
            value
                .py_repr_fmt(&mut repr_string, &self.heap, &mut heap_ids, &interns)
                .ok()?;
            Some(repr_string)
        }
    }

    /// Injects or overwrites a named global variable.
    ///
    /// Converts the given `Object` into a heap-allocated `Value` and stores
    /// it in the global namespace. If a variable with the same name already exists,
    /// the old value is properly dropped (ref-count decremented). If the name is
    /// new, a fresh namespace slot is allocated.
    ///
    /// Returns an error if the session has a pending `resume()` call, if the name
    /// collides with a registered external function, or if the `Object`
    /// cannot be converted to a runtime value (e.g., output-only types like `Repr`).
    ///
    /// # Errors
    ///
    /// - `InvalidInputError::InvalidType("session has pending resume")` when the
    ///   session is paused mid-execution waiting for `resume()`.
    /// - `InvalidInputError::InvalidType("cannot overwrite external function '...'")`
    ///   when `name` matches a registered external function name.
    /// - Any `InvalidInputError` propagated from `Object::to_value` (unsupported
    ///   types or resource exhaustion).
    pub fn set_variable(&mut self, name: &str, value: Object) -> Result<(), InvalidInputError> {
        self.ensure_not_waiting_for_resume()
            .map_err(|_| InvalidInputError::invalid_type("session has pending resume"))?;

        let interns = Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());

        let new_value = value.to_value(&mut self.heap, &interns)?;

        if let Some(&existing_slot) = self.name_map.get(name) {
            if existing_slot.index() < self.external_function_count {
                new_value.drop_with_heap(&mut self.heap);
                return Err(InvalidInputError::invalid_type("cannot overwrite external function"));
            }
            let old = std::mem::replace(self.namespaces.get_mut(GLOBAL_NS_IDX).get_mut(existing_slot), new_value);
            old.drop_with_heap(&mut self.heap);
        } else {
            let slot = NamespaceId::new(self.namespace_size);
            self.namespace_size += 1;
            self.namespaces.grow_global(self.namespace_size);
            self.name_map.insert(name.to_string(), slot);
            *self.namespaces.get_mut(GLOBAL_NS_IDX).get_mut(slot) = new_value;
        }

        Ok(())
    }

    /// Removes a named global variable from the session.
    ///
    /// The variable's value is properly dropped (ref-count decremented) and its
    /// namespace slot is set to `Undefined`. The slot is not reclaimed -- it remains
    /// allocated but invisible to `get_variable` and `list_variables`.
    ///
    /// Returns `true` if the variable existed and was removed, `false` if the name
    /// was unknown or already undefined.
    ///
    /// Returns an error if the session has a pending `resume()` call or if the
    /// name matches a registered external function.
    ///
    /// # Errors
    ///
    /// - `InvalidInputError::InvalidType("session has pending resume")` when the
    ///   session is paused mid-execution waiting for `resume()`.
    /// - `InvalidInputError::InvalidType("cannot delete external function")` when
    ///   `name` matches a registered external function name.
    pub fn delete_variable(&mut self, name: &str) -> Result<bool, InvalidInputError> {
        self.ensure_not_waiting_for_resume()
            .map_err(|_| InvalidInputError::invalid_type("session has pending resume"))?;

        let Some(&slot) = self.name_map.get(name) else {
            return Ok(false);
        };

        if slot.index() < self.external_function_count {
            return Err(InvalidInputError::invalid_type("cannot delete external function"));
        }

        let old = std::mem::replace(self.namespaces.get_mut(GLOBAL_NS_IDX).get_mut(slot), Value::Undefined);

        if matches!(old, Value::Undefined) {
            return Ok(false);
        }

        old.drop_with_heap(&mut self.heap);
        Ok(true)
    }

    /// Parses, prepares, compiles, and executes one REPL snippet.
    ///
    /// On parse/prepare/compile failure, persistent session state is left unchanged.
    /// Once compilation succeeds, interner/name/function/namespace metadata for the
    /// new snippet is committed before runtime execution.
    fn execute_inner(
        &mut self,
        code: &str,
        deadline: Option<Instant>,
        print: &mut impl PrintWriter,
    ) -> Result<Object, ReplError> {
        let parse_result =
            parse::parse_with_interner(code, &self.script_name, self.interner.clone()).map_err(ReplError::Parse)?;
        let prepared = prepare_repl(
            parse_result,
            self.name_map.clone(),
            self.namespace_size,
            &self.external_functions,
        )
        .map_err(ReplError::Parse)?;

        let namespace_size =
            u16::try_from(prepared.namespace_size).expect("module namespace size exceeds u16 in REPL session");

        let compile_interns = Interns::new_for_repl(&prepared.interner, Vec::new(), self.external_functions.clone());
        let existing_function_count = self.functions.len();
        let compile_result = Compiler::compile_module_repl(
            &prepared.nodes,
            &compile_interns,
            namespace_size,
            self.functions.clone(),
        )
        .map_err(ReplError::Compile)?;

        let mut compiled_functions = compile_result.functions;
        let module_code = compile_result.code;
        let new_functions = compiled_functions.split_off(existing_function_count);

        self.interner = prepared.interner;
        self.name_map = prepared.name_map;
        let old_namespace_size = self.namespace_size;
        self.namespace_size = prepared.namespace_size;
        self.functions.extend(new_functions);

        if self.namespace_size > old_namespace_size {
            self.namespaces.grow_global(self.namespace_size);
        }

        self.namespaces.clear_transient_state(&mut self.heap);
        self.heap.tracker_mut().begin_execution(deadline);

        let runtime_interns =
            Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());

        let frame_exit_result = {
            let mut vm = VM::new(
                &mut self.heap,
                &mut self.namespaces,
                &runtime_interns,
                print,
                NoopTracer,
            );
            let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.run_module(&module_code)));
            vm.cleanup();
            match run_result {
                Ok(result) => result,
                Err(panic) => {
                    self.heap.tracker_mut().set_deadline(None);
                    self.namespaces.cleanup_non_global(&mut self.heap);
                    std::panic::resume_unwind(panic);
                }
            }
        };

        self.heap.tracker_mut().set_deadline(None);
        self.namespaces.cleanup_non_global(&mut self.heap);

        frame_exit_to_object(frame_exit_result, &mut self.heap, &runtime_interns)
            .map_err(|error| ReplError::Runtime(error.into_python_exception(&runtime_interns, code)))
    }

    /// Parses, compiles, and executes one snippet in interactive mode.
    ///
    /// Unlike `execute_inner`, this preserves VM state when execution yields so the
    /// host can resume the exact same snippet.
    fn execute_inner_interactive(
        &mut self,
        code: &str,
        deadline: Option<Instant>,
        print: &mut impl PrintWriter,
    ) -> Result<ReplProgress, ReplError> {
        let parse_result =
            parse::parse_with_interner(code, &self.script_name, self.interner.clone()).map_err(ReplError::Parse)?;
        let prepared = prepare_repl(
            parse_result,
            self.name_map.clone(),
            self.namespace_size,
            &self.external_functions,
        )
        .map_err(ReplError::Parse)?;

        let namespace_size =
            u16::try_from(prepared.namespace_size).expect("module namespace size exceeds u16 in REPL session");

        let compile_interns = Interns::new_for_repl(&prepared.interner, Vec::new(), self.external_functions.clone());
        let existing_function_count = self.functions.len();
        let compile_result = Compiler::compile_module_repl(
            &prepared.nodes,
            &compile_interns,
            namespace_size,
            self.functions.clone(),
        )
        .map_err(ReplError::Compile)?;

        let mut compiled_functions = compile_result.functions;
        let module_code = compile_result.code;
        let new_functions = compiled_functions.split_off(existing_function_count);

        self.interner = prepared.interner;
        self.name_map = prepared.name_map;
        let old_namespace_size = self.namespace_size;
        self.namespace_size = prepared.namespace_size;
        self.functions.extend(new_functions);

        if self.namespace_size > old_namespace_size {
            self.namespaces.grow_global(self.namespace_size);
        }

        self.namespaces.clear_transient_state(&mut self.heap);
        self.heap.tracker_mut().begin_execution(deadline);

        let runtime_interns =
            Interns::new_for_repl(&self.interner, self.functions.clone(), self.external_functions.clone());
        let source = code.to_owned();

        let (frame_exit_result, vm_snapshot) = {
            let mut vm = VM::new(
                &mut self.heap,
                &mut self.namespaces,
                &runtime_interns,
                print,
                NoopTracer,
            );
            let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.run_module(&module_code)));
            match run_result {
                Ok(result) => {
                    let snapshot = vm.check_snapshot(&result);
                    (result, snapshot)
                }
                Err(panic) => {
                    self.heap.tracker_mut().set_deadline(None);
                    self.namespaces.cleanup_non_global(&mut self.heap);
                    std::panic::resume_unwind(panic);
                }
            }
        };

        let progress =
            self.handle_interactive_frame_exit(frame_exit_result, vm_snapshot, module_code, source, &runtime_interns);

        self.finish_interactive_step(progress)
    }

    /// Converts a VM frame-exit into REPL interactive progress.
    ///
    /// This stores a pending snapshot on yield and converts VM values to public
    /// `Object` values for host handling.
    fn handle_interactive_frame_exit(
        &mut self,
        frame_exit_result: crate::exception_private::RunResult<FrameExit>,
        vm_snapshot: Option<VMSnapshot>,
        module_code: Code,
        source: String,
        runtime_interns: &Interns,
    ) -> Result<ReplProgress, ReplError> {
        let frame_exit = frame_exit_result
            .map_err(|error| ReplError::Runtime(error.into_python_exception(runtime_interns, source.as_str())))?;

        match frame_exit {
            FrameExit::Return(value) => Ok(ReplProgress::Complete(Object::new(
                value,
                &mut self.heap,
                runtime_interns,
            ))),
            FrameExit::ExternalCall {
                ext_function_id,
                args,
                call_id,
            } => {
                let function_name = runtime_interns.get_external_function_name(ext_function_id);

                // Capability check: deny external function calls not in the capability set.
                if let Some(caps) = &self.capabilities
                    && let Err(denied) = caps.check_function_call(&function_name)
                {
                    return Err(ReplError::Runtime(
                        crate::exception_private::RunError::from(ExcType::permission_error(denied.to_string()))
                            .into_python_exception(runtime_interns, source.as_str()),
                    ));
                }

                let (args_py, kwargs_py) = args.into_py_objects(&mut self.heap, runtime_interns);
                self.pending_snapshot = Some(vm_snapshot.expect("interactive external call must have snapshot"));
                self.pending_resume_state = Some(PendingResumeState {
                    module_code,
                    source,
                    call_id: call_id.raw(),
                    function_name: function_name.clone(),
                    args: args_py.clone(),
                });
                Ok(ReplProgress::FunctionCall {
                    function_name,
                    args: args_py,
                    kwargs: kwargs_py,
                    call_id: call_id.raw(),
                })
            }
            FrameExit::ProxyCall {
                proxy_id,
                method,
                args,
                call_id,
            } => {
                // Capability check: deny proxy operations not in the capability set.
                if let Some(caps) = &self.capabilities
                    && let Err(denied) = caps.check_proxy_access(&method)
                {
                    return Err(ReplError::Runtime(
                        crate::exception_private::RunError::from(ExcType::permission_error(denied.to_string()))
                            .into_python_exception(runtime_interns, source.as_str()),
                    ));
                }

                let (args_py, kwargs_py) = args.into_py_objects(&mut self.heap, runtime_interns);
                self.pending_snapshot = Some(vm_snapshot.expect("interactive proxy call must have snapshot"));
                self.pending_resume_state = Some(PendingResumeState {
                    module_code,
                    source,
                    call_id: call_id.raw(),
                    function_name: format!("proxy:{method}"),
                    args: args_py.clone(),
                });
                Ok(ReplProgress::ProxyCall {
                    proxy_id: proxy_id.raw(),
                    method,
                    args: args_py,
                    kwargs: kwargs_py,
                    call_id: call_id.raw(),
                })
            }
            FrameExit::OsCall { function, .. } => Err(ReplError::Runtime(
                crate::exception_private::RunError::from(ExcType::not_implemented(format!(
                    "OS function '{function}' not implemented with REPL interactive execution"
                )))
                .into_python_exception(runtime_interns, source.as_str()),
            )),
            FrameExit::ResolveFutures(call_ids) => {
                let pending_call_ids: Vec<u32> = call_ids.iter().map(|id| id.raw()).collect();
                let pending_futures = self.build_pending_futures(&pending_call_ids);
                self.pending_snapshot = Some(vm_snapshot.expect("interactive ResolveFutures must have snapshot"));
                self.pending_futures_state = Some(PendingFuturesState {
                    module_code,
                    source,
                    pending_call_ids: pending_call_ids.clone(),
                    pending_futures: pending_futures.clone(),
                });
                Ok(ReplProgress::ResolveFutures {
                    pending_call_ids,
                    pending_futures,
                })
            }
        }
    }

    /// Finalizes a single interactive step by clearing deadlines and transient state.
    ///
    /// Yielding states keep VM data alive; complete/error states clean transient
    /// namespaces and future metadata.
    fn finish_interactive_step(
        &mut self,
        progress: Result<ReplProgress, ReplError>,
    ) -> Result<ReplProgress, ReplError> {
        self.heap.tracker_mut().set_deadline(None);
        match &progress {
            Ok(
                ReplProgress::FunctionCall { .. }
                | ReplProgress::ProxyCall { .. }
                | ReplProgress::ResolveFutures { .. },
            ) => {}
            Ok(ReplProgress::Complete(_)) | Err(_) => {
                self.namespaces.cleanup_non_global(&mut self.heap);
                self.future_metadata.clear();
            }
        }
        progress
    }

    /// Builds `PendingFutureInfo` entries for the given call IDs from accumulated metadata.
    ///
    /// Each entry carries the original function name and arguments. Call IDs without
    /// stored metadata (e.g. from proxy calls or edge cases) get a placeholder entry
    /// with an empty function name and no args.
    fn build_pending_futures(&self, pending_call_ids: &[u32]) -> Vec<PendingFutureInfo> {
        pending_call_ids
            .iter()
            .map(|&call_id| {
                self.future_metadata
                    .get(&call_id)
                    .cloned()
                    .unwrap_or(PendingFutureInfo {
                        call_id,
                        function_name: String::new(),
                        args: Vec::new(),
                    })
            })
            .collect()
    }

    /// Returns an error when API usage attempts to execute while a resume is pending.
    fn ensure_not_waiting_for_resume(&self) -> Result<(), ReplError> {
        if self.pending_snapshot.is_none() {
            return Ok(());
        }
        Err(self.pending_resume_error())
    }

    /// Builds the runtime error used when `resume()` ordering is violated.
    fn pending_resume_error(&self) -> ReplError {
        ReplError::Runtime(Exception::new(
            ExcType::RuntimeError,
            Some("repl session has a pending call; call resume() first".to_owned()),
        ))
    }
}

impl Drop for ReplSession {
    /// Cleans up persistent namespace values when ref-count assertions are enabled.
    fn drop(&mut self) {
        #[cfg(feature = "ref-count-panic")]
        {
            self.namespaces.clear_transient_state(&mut self.heap);
            self.namespaces.cleanup_non_global(&mut self.heap);
            self.namespaces.drop_global_with_heap(&mut self.heap);
        }
    }
}
