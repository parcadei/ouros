//! Bytecode virtual machine for executing compiled Python code.
//!
//! The VM uses a stack-based execution model with an operand stack for computation
//! and a call stack for function frames. Each frame owns its instruction pointer (IP).

mod async_exec;
mod attr;
mod binary;
mod call;
mod collections;
mod compare;
mod exceptions;
mod format;
mod scheduler;

use ahash::{AHashMap, AHashSet};
use call::CallResult;
use scheduler::Scheduler;
use smallvec::SmallVec;

use crate::{
    Object,
    args::{ArgValues, KwargsValues},
    asyncio::{CallId, TaskId},
    bytecode::{code::Code, op::Opcode},
    exception_private::{ExcType, RawStackFrame, RunError, RunResult, SimpleException},
    heap::{ContainsHeap, DropWithHeap, Heap, HeapData, HeapGuard, HeapId},
    intern::{ExtFunctionId, FunctionId, Interns, StaticStrings, StringId},
    io::PrintWriter,
    modules::{
        BuiltinModule,
        statistics::{StatisticsFunctions, create_normaldist_value},
    },
    namespace::{GLOBAL_NS_IDX, NamespaceId, Namespaces},
    os::OsFunction,
    parse::CodeRange,
    proxy::ProxyId,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{ClassObject, Dict, ExitCallback, LongInt, OurosIter, PyTrait, StdlibObject, iter::advance_on_heap},
    value::{BitwiseOp, Value, extract_normaldist_params},
};

/// Information needed to finalize a class body frame.
///
/// When a `BuildClass` opcode runs, it pushes a class body frame with this info.
/// When that frame returns, the VM extracts the namespace into a Dict, creates a
/// `ClassObject`, and pushes it onto the parent frame's stack.
#[derive(Debug)]
pub(super) struct ClassBodyInfo {
    /// The class name (interned string id).
    name_id: StringId,
    /// The function ID of the class body function (for accessing local_names in Code).
    func_id: FunctionId,
    /// HeapIds of direct base classes (popped from stack during BuildClass).
    bases: Vec<HeapId>,
    /// The metaclass used to create this class.
    ///
    /// Stored as a Value so it can be either a builtin `type` or a user-defined class object.
    /// If this is a heap ref, its refcount is owned by the class object once finalization completes.
    metaclass: Value,
    /// Keyword arguments passed to the class definition (excluding `metaclass`).
    ///
    /// Stored as a heap Dict Value so we can pass kwargs to `__init_subclass__`.
    class_kwargs: Value,
    /// Optional namespace returned by `metaclass.__prepare__`.
    ///
    /// This is stored as an owned Value to support arbitrary mapping objects.
    /// When the prepared namespace is a dict, we clone it directly for the
    /// class namespace. For non-dict mappings, class body assignments are
    /// mirrored into the frame namespace so class finalization can still
    /// build a dict without calling user code.
    prepared_namespace: Option<Value>,
    /// Original bases tuple before applying `__mro_entries__`, if any.
    ///
    /// Stored so we can populate `__orig_bases__` in the class namespace for
    /// PEP 560/695 compatibility.
    orig_bases: Option<HeapId>,
    /// Namespace slot holding the `__class__` cell for zero-arg super(), if present.
    class_cell_slot: Option<NamespaceId>,
}

/// Result of executing Await opcode.
///
/// Indicates what the VM should do after awaiting a value:
/// - `ValueReady`: the awaited value resolved immediately, push it
/// - `FramePushed`: a new frame was pushed for coroutine execution
/// - `Yield`: all tasks blocked, yield to caller with pending futures
enum AwaitResult {
    /// The awaited value resolved immediately (e.g., resolved ExternalFuture).
    ValueReady(Value),
    /// A new frame was pushed to execute a coroutine.
    FramePushed,
    /// All tasks are blocked - yield to caller with pending futures.
    Yield(Vec<CallId>),
}

/// Tries an operation and handles exceptions, reloading cached frame state.
///
/// Use this in the main run loop where `cached_frame`
/// are used. After catching an exception, reloads the cache since the handler
/// may be in a different frame.
macro_rules! try_catch_sync {
    ($self:expr, $cached_frame:ident, $expr:expr) => {
        if let Err(e) = $expr {
            if let Some(result) = $self.handle_exception(e) {
                return Err(result);
            }
            // Exception was caught - handler may be in different frame, reload cache
            reload_cache!($self, $cached_frame);
        }
    };
}

/// Handles an exception and reloads cached frame state if caught.
///
/// Use this in the main run loop where `cached_frame`
/// are used. After catching an exception, reloads the cache since the handler
/// may be in a different frame.
///
/// Wrapped in a block to allow use in match arm expressions.
macro_rules! catch_sync {
    ($self:expr, $cached_frame:ident, $err:expr) => {{
        if let Some(result) = $self.handle_exception($err) {
            return Err(result);
        }
        // Exception was caught - handler may be in different frame, reload cache
        reload_cache!($self, $cached_frame);
    }};
}

/// Fetches a byte from bytecode using cached code/ip, advancing ip.
///
/// Used in the run loop for fast operand fetching without frame access.
macro_rules! fetch_byte {
    ($cached_frame:expr) => {{
        let byte = $cached_frame.code.bytecode()[$cached_frame.ip];
        $cached_frame.ip += 1;
        byte
    }};
}

/// Fetches a u8 operand using cached code/ip.
macro_rules! fetch_u8 {
    ($cached_frame:expr) => {
        fetch_byte!($cached_frame)
    };
}

/// Fetches an i8 operand using cached code/ip.
macro_rules! fetch_i8 {
    ($cached_frame:expr) => {{ i8::from_ne_bytes([fetch_byte!($cached_frame)]) }};
}

/// Fetches a u16 operand (little-endian) using cached code/ip.
macro_rules! fetch_u16 {
    ($cached_frame:expr) => {{
        let lo = $cached_frame.code.bytecode()[$cached_frame.ip];
        let hi = $cached_frame.code.bytecode()[$cached_frame.ip + 1];
        $cached_frame.ip += 2;
        u16::from_le_bytes([lo, hi])
    }};
}

/// Fetches an i16 operand (little-endian) using cached code/ip.
macro_rules! fetch_i16 {
    ($cached_frame:expr) => {{
        let lo = $cached_frame.code.bytecode()[$cached_frame.ip];
        let hi = $cached_frame.code.bytecode()[$cached_frame.ip + 1];
        $cached_frame.ip += 2;
        i16::from_le_bytes([lo, hi])
    }};
}

/// Reloads cached frame state from the current frame.
///
/// Call this after any operation that modifies the frame stack (calls, returns,
/// exception handling).
macro_rules! reload_cache {
    ($self:expr, $cached_frame:ident) => {{
        $cached_frame = $self.new_cached_frame();
    }};
}

/// Applies a relative jump offset to the cached IP.
///
/// Uses checked arithmetic to safely compute the new IP, panicking if the
/// jump would result in a negative or overflowing instruction pointer.
macro_rules! jump_relative {
    ($ip:expr, $offset:expr) => {{
        let ip_i64 = i64::try_from($ip).expect("instruction pointer exceeds i64");
        let new_ip = ip_i64 + i64::from($offset);
        $ip = usize::try_from(new_ip).expect("jump resulted in negative or overflowing IP");
    }};
}

/// Handles the result of a call operation that returns `CallResult`.
///
/// This macro eliminates the repetitive pattern of matching on `CallResult`
/// variants that appears in LoadAttr, CallFunction, CallFunctionKw, CallAttr,
/// CallAttrKw, and CallFunctionExtended opcodes.
///
/// Actions taken for each variant:
/// - `Push(value)`: Push the value onto the stack
/// - `FramePushed`: Reload the cached frame (a new frame was pushed)
/// - `External(ext_id, args)`: Return `FrameExit::ExternalCall` to yield to host
/// - `OsCall(func, args)`: Return `FrameExit::OsCall` to yield to host
/// - `Err(err)`: Handle the exception via `catch_sync!`
macro_rules! handle_call_result {
    ($self:expr, $cached_frame:ident, $result:expr) => {
        match $result {
            Ok(CallResult::Push(result)) => $self.push(result),
            Ok(CallResult::FramePushed) => reload_cache!($self, $cached_frame),
            Ok(CallResult::External(ext_id, args)) => {
                let call_id = $self.allocate_call_id();
                // Sync cached IP back to frame before snapshot for resume
                $self.current_frame_mut().ip = $cached_frame.ip;
                return Ok(FrameExit::ExternalCall {
                    ext_function_id: ext_id,
                    args,
                    call_id,
                });
            }
            Ok(CallResult::Proxy(proxy_id, method, args)) => {
                let call_id = $self.allocate_call_id();
                // Sync cached IP back to frame before snapshot for resume
                $self.current_frame_mut().ip = $cached_frame.ip;
                return Ok(FrameExit::ProxyCall {
                    proxy_id,
                    method,
                    args,
                    call_id,
                });
            }
            Ok(CallResult::OsCall(func, args)) => {
                let call_id = $self.allocate_call_id();
                // Sync cached IP back to frame before snapshot for resume
                $self.current_frame_mut().ip = $cached_frame.ip;
                return Ok(FrameExit::OsCall {
                    function: func,
                    args,
                    call_id,
                });
            }
            Err(err) => catch_sync!($self, $cached_frame, err),
        }
    };
}

/// Handles the result of a call operation that does not push a value.
///
/// Used for operations like class-body `__setitem__`/`__delitem__` that should
/// discard return values while still supporting frame pushes.
macro_rules! handle_void_call_result {
    ($self:expr, $cached_frame:ident, $result:expr, $context:expr) => {
        match $result {
            Ok(CallResult::Push(value)) => value.drop_with_heap($self.heap),
            Ok(CallResult::FramePushed) => {
                $self.pending_discard_return = true;
                reload_cache!($self, $cached_frame);
            }
            Ok(other) => catch_sync!(
                $self,
                $cached_frame,
                RunError::internal(format!("{} returned unsupported call result: {other:?}", $context))
            ),
            Err(err) => catch_sync!($self, $cached_frame, err),
        }
    };
}

/// Result of VM execution.
pub enum FrameExit {
    /// Execution completed successfully with a return value.
    Return(Value),

    /// Execution paused for an external function call.
    ///
    /// The caller should execute the external function and call `resume()`
    /// with the result. The `call_id` allows the host to use async resolution
    /// by calling `run_pending()` instead of `run(result)`.
    ExternalCall {
        /// ID of the external function to call.
        ext_function_id: ExtFunctionId,
        /// Arguments for the external function (includes both positional and keyword args).
        args: ArgValues,
        /// Unique ID for this call, used for async correlation.
        call_id: CallId,
    },

    /// Execution paused for a host-managed proxy operation.
    ///
    /// The host should perform the requested proxy operation and call `resume()`
    /// with the result.
    ProxyCall {
        /// ID of the proxy value.
        proxy_id: ProxyId,
        /// Attribute or method name being accessed.
        method: String,
        /// Arguments for the operation (includes both positional and keyword args).
        args: ArgValues,
        /// Unique ID for this call, used for async correlation.
        call_id: CallId,
    },

    /// Execution paused for an os function call.
    ///
    /// The caller should execute a function corresponding to the `os_call` and call `resume()`
    /// with the result. The `call_id` allows the host to use async resolution
    /// by calling `run_pending()` instead of `run(result)`.
    OsCall {
        /// ID of the os function to call.
        function: OsFunction,
        /// Arguments for the external function (includes both positional and keyword args).
        args: ArgValues,
        /// Unique ID for this call, used for async correlation.
        call_id: CallId,
    },

    /// All tasks are blocked waiting for external futures to resolve.
    ///
    /// The caller must resolve the pending CallIds before calling `resume()`.
    /// This happens when await is called on an ExternalFuture that hasn't
    /// been resolved yet, and there are no other ready tasks to switch to.
    ResolveFutures(Vec<CallId>),
}

/// A single function activation record.
///
/// Each frame represents one level in the call stack and owns its own
/// instruction pointer. This design avoids sync bugs on call/return.
#[derive(Debug)]
pub struct CallFrame<'code> {
    /// Bytecode being executed.
    code: &'code Code,

    /// Instruction pointer within this frame's bytecode.
    ip: usize,

    /// Base index into operand stack for this frame.
    ///
    /// Used to identify where this frame's stack region begins.
    stack_base: usize,

    /// Namespace index for this frame's locals.
    namespace_idx: NamespaceId,

    /// Function ID (for tracebacks). None for module-level code.
    function_id: Option<FunctionId>,

    /// Captured cells for closures.
    cells: Vec<HeapId>,

    /// Call site position (for tracebacks).
    call_position: Option<CodeRange>,

    /// If this frame is executing a class body, holds the info needed
    /// to create the ClassObject when the frame returns.
    class_body_info: Option<ClassBodyInfo>,

    /// If this frame is executing `__init__` for a class instantiation,
    /// holds the Instance value to push when the frame returns (instead of
    /// the `__init__` return value which is always None).
    init_instance: Option<Value>,

    /// If this frame is executing a generator, holds the HeapId of the Generator.
    generator_id: Option<HeapId>,
}

impl<'code> CallFrame<'code> {
    /// Creates a new call frame for module-level code.
    pub fn new_module(code: &'code Code, namespace_idx: NamespaceId) -> Self {
        Self {
            code,
            ip: 0,
            stack_base: 0,
            namespace_idx,
            function_id: None,
            cells: Vec::new(),
            call_position: None,
            class_body_info: None,
            init_instance: None,
            generator_id: None,
        }
    }

    /// Creates a new call frame for a function call.
    pub fn new_function(
        code: &'code Code,
        stack_base: usize,
        namespace_idx: NamespaceId,
        function_id: FunctionId,
        cells: Vec<HeapId>,
        call_position: Option<CodeRange>,
    ) -> Self {
        Self {
            code,
            ip: 0,
            stack_base,
            namespace_idx,
            function_id: Some(function_id),
            cells,
            call_position,
            class_body_info: None,
            init_instance: None,
            generator_id: None,
        }
    }

    /// Creates a new call frame for a simple function call (no closures/cells).
    ///
    /// This is a fast-path variant of `new_function` that avoids allocating an empty
    /// `Vec<HeapId>` for the cells field. Used by the inlined call fast path for
    /// simple sync functions where no cell variables exist.
    #[inline]
    pub fn new_simple_function(
        code: &'code Code,
        stack_base: usize,
        namespace_idx: NamespaceId,
        function_id: FunctionId,
        call_position: CodeRange,
    ) -> Self {
        Self {
            code,
            ip: 0,
            stack_base,
            namespace_idx,
            function_id: Some(function_id),
            cells: Vec::new(),
            call_position: Some(call_position),
            class_body_info: None,
            init_instance: None,
            generator_id: None,
        }
    }

    /// Creates a new call frame for a class body execution.
    ///
    /// The class body is executed as a function, and when it returns, the namespace
    /// is extracted into a Dict and used to create a ClassObject.
    pub fn new_class_body(
        code: &'code Code,
        stack_base: usize,
        namespace_idx: NamespaceId,
        function_id: FunctionId,
        cells: Vec<HeapId>,
        call_position: Option<CodeRange>,
        class_body_info: ClassBodyInfo,
    ) -> Self {
        Self {
            code,
            ip: 0,
            stack_base,
            namespace_idx,
            function_id: Some(function_id),
            cells,
            call_position,
            class_body_info: Some(class_body_info),
            init_instance: None,
            generator_id: None,
        }
    }
}

/// Cached state of the VM derived from the current frame as an optimization
#[derive(Debug, Copy, Clone)]
pub struct CachedFrame<'code> {
    /// Bytecode being executed.
    code: &'code Code,

    /// Instruction pointer within this frame's bytecode.
    ip: usize,

    /// Namespace index for this frame's locals.
    namespace_idx: NamespaceId,
}

impl<'code> From<&CallFrame<'code>> for CachedFrame<'code> {
    fn from(frame: &CallFrame<'code>) -> Self {
        Self {
            code: frame.code,
            ip: frame.ip,
            namespace_idx: frame.namespace_idx,
        }
    }
}

/// Specialized inline-cache targets for `CallAttr` opcode sites.
///
/// This enum models the monomorphic fast path currently supported by the
/// runtime call-site cache. Additional shapes can be added incrementally as
/// dedicated variants while keeping dispatch explicit and auditable.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum CallAttrInlineCacheKind {
    /// Exact `list.append(x)` call-site specialization.
    ListAppend,
    /// Exact `str.zfill(width)` call-site specialization on heap strings.
    StrZfill,
}

/// Monomorphic inline-cache entry for a single `CallAttr` opcode site.
///
/// The cache is keyed by code identity + opcode IP + static call signature
/// (`name_id`, `arg_count`). On a hit, the VM attempts the specialized fast
/// path directly and falls back safely if receiver shape changed.
#[derive(Debug, Copy, Clone)]
pub(super) struct CallAttrInlineCacheEntry {
    /// Stable identity for the code object containing the call site.
    code_identity: usize,
    /// Instruction pointer of the `CallAttr` opcode.
    opcode_ip: usize,
    /// Attribute name encoded in the opcode operand.
    name_id: StringId,
    /// Positional argument count encoded in the opcode operand.
    arg_count: u8,
    /// Specialized call shape to execute on cache hit.
    kind: CallAttrInlineCacheKind,
}

impl CallAttrInlineCacheEntry {
    /// Creates a cache entry for `list.append(x)` at a specific opcode site.
    pub(super) fn list_append_site(code_identity: usize, opcode_ip: usize, name_id: StringId) -> Self {
        Self {
            code_identity,
            opcode_ip,
            name_id,
            arg_count: 1,
            kind: CallAttrInlineCacheKind::ListAppend,
        }
    }

    /// Creates a cache entry for `str.zfill(width)` at a specific opcode site.
    pub(super) fn str_zfill_site(code_identity: usize, opcode_ip: usize, name_id: StringId) -> Self {
        Self {
            code_identity,
            opcode_ip,
            name_id,
            arg_count: 1,
            kind: CallAttrInlineCacheKind::StrZfill,
        }
    }

    /// Returns true when this entry matches the currently executing call site.
    pub(super) fn matches(self, code_identity: usize, opcode_ip: usize, name_id: StringId, arg_count: usize) -> bool {
        self.code_identity == code_identity
            && self.opcode_ip == opcode_ip
            && self.name_id == name_id
            && usize::from(self.arg_count) == arg_count
    }

    /// Returns the specialized cache kind for this site.
    pub(super) fn kind(self) -> CallAttrInlineCacheKind {
        self.kind
    }
}

/// Serializable representation of a call frame.
///
/// Cannot store `&Code` (a reference) - instead stores `FunctionId` to look up
/// the pre-compiled Code object on resume. Module-level code uses `None`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SerializedFrame {
    /// Which function's code this frame executes (None = module-level).
    function_id: Option<FunctionId>,

    /// Instruction pointer within this frame's bytecode.
    ip: usize,

    /// Base index into operand stack for this frame's locals.
    stack_base: usize,

    /// Namespace index for this frame's locals.
    namespace_idx: NamespaceId,

    /// Captured cells for closures (HeapIds remain valid after heap deserialization).
    cells: Vec<HeapId>,

    /// Call site position (for tracebacks).
    call_position: Option<CodeRange>,

    /// If this frame is executing a class body, info needed to finalize class creation.
    class_body_info: Option<SerializedClassBodyInfo>,

    /// If this frame is executing `__init__`, instance to return when the frame exits.
    init_instance: Option<Value>,

    /// If this frame is executing a generator, holds the HeapId of the Generator.
    generator_id: Option<HeapId>,
}

/// Serializable representation of [`ClassBodyInfo`] used in VM snapshots.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SerializedClassBodyInfo {
    name_id: StringId,
    func_id: FunctionId,
    bases: Vec<HeapId>,
    metaclass: Value,
    class_kwargs: Value,
    prepared_namespace: Option<Value>,
    orig_bases: Option<HeapId>,
    class_cell_slot: Option<NamespaceId>,
}

impl From<ClassBodyInfo> for SerializedClassBodyInfo {
    fn from(value: ClassBodyInfo) -> Self {
        Self {
            name_id: value.name_id,
            func_id: value.func_id,
            bases: value.bases,
            metaclass: value.metaclass,
            class_kwargs: value.class_kwargs,
            prepared_namespace: value.prepared_namespace,
            orig_bases: value.orig_bases,
            class_cell_slot: value.class_cell_slot,
        }
    }
}

impl From<SerializedClassBodyInfo> for ClassBodyInfo {
    fn from(value: SerializedClassBodyInfo) -> Self {
        Self {
            name_id: value.name_id,
            func_id: value.func_id,
            bases: value.bases,
            metaclass: value.metaclass,
            class_kwargs: value.class_kwargs,
            prepared_namespace: value.prepared_namespace,
            orig_bases: value.orig_bases,
            class_cell_slot: value.class_cell_slot,
        }
    }
}

impl CallFrame<'_> {
    /// Converts this frame to a serializable representation.
    fn serialize(self) -> SerializedFrame {
        SerializedFrame {
            function_id: self.function_id,
            ip: self.ip,
            stack_base: self.stack_base,
            namespace_idx: self.namespace_idx,
            cells: self.cells,
            call_position: self.call_position,
            class_body_info: self.class_body_info.map(Into::into),
            init_instance: self.init_instance,
            generator_id: self.generator_id,
        }
    }
}

/// VM state for pause/resume at external function calls.
///
/// **Ownership:** This struct OWNS the values (refcounts were already incremented).
/// Must be used with the serialized Heap - HeapId values are indices into that heap.
///
/// **Usage:** When the VM pauses for an external call, call `into_snapshot()` to
/// create this snapshot. The snapshot can be serialized and stored. On resume,
/// use `restore()` to reconstruct the VM and continue execution.
///
/// Note: This struct does not implement `Clone` because `Value` uses manual
/// reference counting. Snapshots transfer ownership - they are not copied.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VMSnapshot {
    /// Operand stack (may contain Value::Ref(HeapId) pointing to heap).
    stack: Vec<Value>,

    /// Call frames (serializable form - stores FunctionId, not &Code).
    frames: Vec<SerializedFrame>,

    /// Stack of exceptions being handled for nested except blocks.
    ///
    /// When entering an except handler, the exception is pushed onto this stack.
    /// When exiting via `ClearException`, the top is popped. This allows nested
    /// except handlers to restore the outer exception context.
    exception_stack: Vec<Value>,

    /// Operand-stack index for each entry in `exception_stack`.
    ///
    /// Used to discard stale exception contexts when stack unwinding bypasses
    /// `ClearException` (for example, an exception raised inside an `except`).
    #[serde(default)]
    exception_stack_positions: Vec<usize>,

    /// IP of the instruction that caused the pause (for exception handling).
    instruction_ip: usize,

    /// Counter for external call IDs when scheduler is not initialized.
    next_call_id: u32,

    /// Scheduler state for async execution (optional).
    ///
    /// Contains all task state, pending calls, and resolved futures.
    /// This enables async execution to be paused and resumed across host calls.
    /// None if no async operations have been performed yet.
    scheduler: Option<Scheduler>,
}

// ============================================================================
// Virtual Machine
// ============================================================================

/// The bytecode virtual machine.
///
/// Executes compiled bytecode using a stack-based execution model.
/// The instruction pointer (IP) lives in each `CallFrame`, not here,
/// to avoid sync bugs on call/return.
#[expect(clippy::struct_excessive_bools)]
pub struct VM<'a, T: ResourceTracker, P: PrintWriter, Tr: VmTracer = crate::tracer::NoopTracer> {
    /// Operand stack - values being computed.
    stack: Vec<Value>,

    /// Call stack - function frames (each frame has its own IP).
    frames: Vec<CallFrame<'a>>,

    /// Heap for reference-counted objects.
    heap: &'a mut Heap<T>,

    /// Namespace stack for variable storage.
    namespaces: &'a mut Namespaces,

    /// Interned strings/bytes.
    interns: &'a Interns,

    /// Print output writer.
    print_writer: &'a mut P,

    /// Stack of exceptions being handled for nested except blocks.
    ///
    /// Used by bare `raise` to re-raise the current exception.
    /// When entering an except handler, the exception is pushed onto this stack.
    /// When exiting via `ClearException`, the top is popped. This allows nested
    /// except handlers to restore the outer exception context.
    exception_stack: Vec<Value>,

    /// Operand-stack index for each entry in `exception_stack`.
    ///
    /// Entries are synchronized with `exception_stack` and let exception
    /// handling drop stale contexts when unwinding stack ranges.
    exception_stack_positions: Vec<usize>,

    /// IP of the instruction being executed (for exception table lookup).
    ///
    /// Updated at the start of each instruction before operands are fetched.
    /// This allows us to find the correct exception handler when an error occurs.
    instruction_ip: usize,

    /// Counter for external call IDs when scheduler is not initialized.
    ///
    /// Used by `allocate_call_id()` when no scheduler exists (sync code paths).
    /// When a scheduler is created, this counter is transferred to it.
    next_call_id: u32,

    /// Scheduler for async task management (lazy - only created when needed).
    ///
    /// Manages concurrent tasks, external call tracking, and task switching.
    /// Created lazily on first async operation to avoid allocations for sync code.
    scheduler: Option<Scheduler>,

    /// Module-level code (for restoring main task frames).
    ///
    /// Stored here because the main task's frames have `function_id: None` and
    /// need a reference to the module code when being restored after task switching.
    module_code: Option<&'a Code>,

    /// Pending hash target for instance __hash__ dunder dispatch in dict operations.
    ///
    /// When a dict operation (DictSetItem, BuildDict, StoreSubscr, BinarySubscr) encounters
    /// an instance key that needs a custom __hash__, the VM calls __hash__() which pushes a
    /// frame. This field records the instance HeapId so that when the frame returns, the VM
    /// can cache the hash result and re-execute the dict operation.
    ///
    /// The saved IP points to the start of the dict opcode (including its operand) so that
    /// after caching the hash, the opcode is re-executed and finds the cached hash.
    pending_hash_target: Option<HeapId>,
    /// When true, a pending hash dunder return should also push the hash result.
    ///
    /// Dict operations set `pending_hash_target` only to prime caches before re-executing
    /// the opcode, so they leave this false. `hash(instance)` sets this true so the return
    /// value is both cached and pushed.
    pending_hash_push_result: bool,

    /// Stack of pending string-conversion dunder returns.
    ///
    /// `str(instance)` and `repr(instance)` may call user-defined dunders that
    /// push frames. Each entry stores `(kind, frame_depth)` so nested calls can
    /// unwind exceptions without losing the outer pending conversion state.
    pending_stringify_return: Vec<(PendingStringifyReturn, usize)>,

    /// When true, the next frame return should drop the return value instead of pushing it.
    ///
    /// Used for dunder methods like __setitem__, __delitem__, __exit__ that return None
    /// but are called from opcodes that don't expect a return value on the stack.
    pending_discard_return: bool,

    /// When true, the next frame return should negate the bool return value.
    ///
    /// Used for `not in` operator when __contains__ pushes a frame. The result
    /// needs to be negated before being used.
    pending_negate_bool: bool,

    /// When true, the next frame return is from `__instancecheck__`.
    ///
    /// The return value should be coerced to bool before pushing.
    pending_instancecheck_return: bool,

    /// When true, the next frame return is from `__subclasscheck__`.
    ///
    /// The return value should be coerced to bool before pushing.
    pending_subclasscheck_return: bool,

    /// When true, the next frame return is from `__dir__` invoked by `dir(obj)`.
    ///
    /// The returned iterable is normalized and sorted before pushing.
    pending_dir_return: bool,

    /// Pending `__getattr__` fallbacks for `__getattribute__` calls that pushed a frame.
    ///
    /// When attribute access invokes `__getattribute__` and it pushes a frame,
    /// we keep a reference to the receiver and attribute name so that if the
    /// call raises AttributeError we can invoke `__getattr__` instead.
    pending_getattr_fallback: Vec<PendingGetAttr>,

    /// Pending binary dunder calls that pushed a frame.
    ///
    /// Binary ops (`+`, `-`, etc.) need special handling when `lhs.__op__(rhs)`
    /// returns `NotImplemented`: the VM must try `rhs.__rop__(lhs)`. When a dunder
    /// pushes a Python frame, this stack tracks the in-flight operation so ReturnValue
    /// can continue the protocol correctly.
    pending_binary_dunder: Vec<PendingBinaryDunder>,

    /// Stack of pending `ForIter` jump offsets.
    ///
    /// `ForIter` can resume nested generators (`for` inside generator pipelines), so
    /// pending `__next__` continuations must be tracked as a LIFO stack:
    /// - normal return/yield from `__next__`: pop one pending entry
    /// - `StopIteration` from `__next__`: pop one pending entry and jump to loop end
    pending_for_iter_jump: Vec<i16>,

    /// Pending `next(generator, default)` state while a generator frame executes.
    ///
    /// When set, the default should be returned if the target generator exhausts
    /// before producing a value.
    pending_next_default: Option<PendingNextDefault>,

    /// Pending `defaultdict[missing_key]` state while `default_factory()` executes.
    ///
    /// When `default_factory` is a user callable, `factory()` may push a frame.
    /// We stash the target defaultdict reference and missing key so the return
    /// value can be inserted and pushed when that frame completes.
    pending_defaultdict_missing: Option<PendingDefaultDictMissing>,

    /// When true, the next frame return is from a `default_factory()` call
    /// triggered by missing-key `defaultdict` access.
    pending_defaultdict_return: bool,

    /// Generator currently executing a `close()` request, if any.
    ///
    /// While set, an uncaught `GeneratorExit`/`StopIteration` from that generator is
    /// suppressed and `close()` returns `None`. If the generator yields instead, the
    /// VM raises `RuntimeError: generator ignored GeneratorExit`.
    pending_generator_close: Option<HeapId>,

    /// When set, a CompareModEq opcode pushed a __mod__/__rmod__ dunder frame.
    ///
    /// On normal return: compare the mod result with this value, push bool.
    /// Stores the constant k from `a % b == k`.
    pending_mod_eq_k: Option<Value>,

    /// When set, a CompareEqJumpIfFalse opcode pushed a `__eq__` dunder frame.
    ///
    /// On return, the dunder result is interpreted as the comparison truth value.
    /// If falsy, execution jumps by this offset in the caller frame.
    pending_compare_eq_jump: Option<i16>,

    /// Pending `__set_name__` calls to process after class body finalization.
    ///
    /// During `finalize_class_body`, descriptors that define `__set_name__` are
    /// collected here. They are then processed one-by-one in the main loop:
    /// the first call is initiated after the class value is pushed, and each
    /// subsequent call is initiated when the previous one's frame returns.
    ///
    /// Each entry is `(attr_name_id, descriptor_heap_id, class_heap_id)`.
    pending_set_name_calls: Vec<(StringId, HeapId, HeapId)>,

    /// When true, the next frame return is from a `__set_name__` call.
    ///
    /// The return value should be discarded and the next pending `__set_name__`
    /// call (if any) should be initiated.
    pending_set_name_return: bool,

    /// Pending `__new__` result: when set, the next frame return is from a `__new__` call.
    ///
    /// Contains `(class_heap_id, init_func_option, args_for_init)`.
    /// When `__new__` returns:
    /// - If result is an instance of the class AND init_func is Some, call `__init__`
    /// - Otherwise, push the result directly (skip `__init__`)
    pending_new_call: Option<PendingNewCall>,

    /// Pending `__init_subclass__` calls to process after class body finalization.
    ///
    /// Each entry is `(base_class_id, new_class_id)`: the base that defines
    /// `__init_subclass__` and the newly created subclass to pass as `cls`.
    pending_init_subclass_calls: Vec<(HeapId, HeapId, Value)>,

    /// When true, the next frame return is from an `__init_subclass__` call.
    pending_init_subclass_return: bool,

    /// Pending class build state when `__mro_entries__` or `__prepare__` pushed a frame.
    ///
    /// BuildClass can invoke user-defined callables while resolving bases or preparing
    /// the class namespace. When those calls push a frame, we stash all intermediate
    /// state here so the ReturnValue handler can resume class construction.
    pending_class_build: Option<PendingClassBuild>,

    /// Pending class finalization while waiting for prepared-namespace items.
    ///
    /// When `__prepare__` returns a non-dict mapping, we call `items()` to seed
    /// the class namespace. If that call pushes a frame, we stash the class
    /// body metadata here and resume finalization when the frame returns.
    pending_class_finalize: Option<PendingClassFinalize>,

    /// Stack of pending `list()` constructions from iterators.
    ///
    /// Nested list materializations are possible (e.g. recursive `str.join` over
    /// generators), so this is a stack rather than a single slot.
    pending_list_build: Vec<PendingListBuild>,

    /// When true, the next frame return is from a `__next__` call for list construction.
    ///
    /// The return value should be appended to `pending_list_build.items`,
    /// and the next `__next__()` call should be initiated.
    pending_list_build_return: bool,

    /// When true, the next frame return is from an `__iter__` call for list construction.
    ///
    /// The return value (the iterator) should be passed to `list_build_from_iterator`
    /// to continue the list construction process.
    pending_list_iter_return: bool,

    /// Pending unpack operation waiting for generator list materialization.
    ///
    /// `UNPACK_SEQUENCE` and `UNPACK_EX` are synchronous bytecode operations. For
    /// generator inputs we first materialize via `list_build_from_iterator`, which may
    /// suspend and resume frames. This field stores the unpack mode to apply once
    /// materialization finishes.
    pending_unpack: Option<PendingUnpack>,

    /// Pending `sum(generator[, start])` completion after list materialization.
    ///
    /// `sum()` is normally implemented as a synchronous builtin over `OurosIter`.
    /// For generator inputs we first materialize items via VM-driven generator
    /// iteration (`list_build_from_iterator`). If that iteration pushes frames,
    /// we stash the optional start value here and finalize `sum()` once the list
    /// is fully built.
    pending_sum_from_list: Option<PendingSumFromList>,

    /// Stack of pending builtins to run after generator list materialization.
    ///
    /// `OurosIter` cannot directly drive generator frames, so some builtins first
    /// materialize generator input and then run against the resulting list.
    /// Nested materializations are tracked independently.
    pending_builtin_from_list: Vec<PendingBuiltinFromList>,

    /// Pending `throw()`/`close()` action for a resumed generator frame.
    pending_generator_action: Option<PendingGeneratorAction>,

    /// Stack of pending yield-from delegations while waiting on pushed sub-iterator frames.
    pending_yield_from: Vec<PendingYieldFrom>,

    /// Pending `list.sort(key=...)` state while a user key callable frame executes.
    ///
    /// VM-level sort key evaluation can call lambdas/closures and therefore may push
    /// frames. When that happens, we stash item/key progress here and resume when the
    /// key frame returns.
    pending_list_sort: Option<PendingListSort>,

    /// When true, the next frame return is from a pending list-sort key call.
    pending_list_sort_return: bool,

    /// Pending `min`/`max` state when a `key=` callable pushed a frame.
    pending_min_max: Option<PendingMinMax>,

    /// When true, the next frame return is from a pending min/max key call.
    pending_min_max_return: bool,

    /// Pending `heapq.nsmallest/nlargest(..., key=...)` state while key calls run.
    pending_heapq_select: Option<PendingHeapqSelect>,

    /// When true, the next frame return is from a pending heapq key call.
    pending_heapq_select_return: bool,

    /// Pending `bisect`/`insort` state when a `key=` callable pushed a frame.
    pending_bisect: Option<PendingBisect>,

    /// When true, the next frame return is from a pending bisect key call.
    pending_bisect_return: bool,

    /// Pending `functools.reduce()` state when the user-defined function pushed a frame.
    ///
    /// When `reduce(func, iterable)` is called with a user-defined function (lambda,
    /// def function, closure), the VM calls the function for each pair of (accumulator, item).
    /// If the function pushes a frame, we stash the reduce state here and resume when
    /// the frame returns.
    pending_reduce: Option<PendingReduce>,

    /// When true, the next frame return is from the reduce function application.
    ///
    /// The return value becomes the new accumulator. The VM then continues
    /// processing remaining items in `pending_reduce`.
    pending_reduce_return: bool,

    /// Pending `sorted()` state when a user-defined key function pushed a frame.
    ///
    /// When `sorted(iterable, key=func)` is called with a user-defined key function,
    /// we call `list.sort()` which may push frames for key function calls. We store
    /// the list_id here so that when sort completes, we return the list instead of None.
    pending_sorted: Option<HeapId>,

    /// Pending `map()` state when the user-defined function pushed a frame.
    ///
    /// When `map(func, *iterables)` is called with a user-defined function (lambda,
    /// def function, closure), the VM calls the function for each set of items.
    /// If the function pushes a frame, we stash the map state here and resume when
    /// the frame returns.
    pending_map: Option<PendingMap>,

    /// When true, the next frame return is from the map function application.
    ///
    /// The return value is added to results. The VM then continues processing
    /// remaining items in `pending_map`.
    pending_map_return: bool,

    /// Pending `filter()` state when the user-defined function pushed a frame.
    ///
    /// When `filter(func, iterable)` is called with a user-defined function (lambda,
    /// def function, closure), the VM calls the function for each item.
    /// If the function pushes a frame, we stash the filter state here and resume when
    /// the frame returns.
    pending_filter: Option<PendingFilter>,

    /// When true, the next frame return is from the filter function application.
    ///
    /// The return value is checked for truthiness, and if true, the corresponding
    /// item is added to results. The VM then continues processing remaining items
    /// in `pending_filter`.
    pending_filter_return: bool,

    /// Pending `functools.lru_cache` write-backs after wrapped calls pushed frames.
    ///
    /// This is a stack because cached functions can recurse. Each frame-pushed
    /// cache miss pushes one pending write-back, and returns pop in LIFO order.
    pending_lru_cache: Vec<PendingLruCache>,

    /// When true, the next frame return should be cached into `pending_lru_cache`.
    pending_lru_cache_return: bool,

    /// Pending `itertools.groupby(..., key=...)` state when a key callable pushed a frame.
    pending_groupby: Option<PendingGroupBy>,

    /// When true, the next frame return is from a pending groupby key call.
    pending_groupby_return: bool,

    /// Pending `textwrap.indent(..., predicate=...)` state when predicate pushed a frame.
    pending_textwrap_indent: Option<PendingTextwrapIndent>,

    /// When true, the next frame return is from a textwrap indent predicate call.
    pending_textwrap_indent_return: bool,

    /// Pending `re.sub`/`re.subn` state when the callable replacement pushed a frame.
    pending_re_sub: Option<PendingReSub>,

    /// When true, the next frame return is from a re.sub callable replacement call.
    pending_re_sub_return: bool,

    /// Pending `contextlib` decorator wrapper state across frame boundaries.
    pending_context_decorator: Option<PendingContextDecorator>,

    /// When true, the next frame return is for a pending contextlib decorator stage.
    pending_context_decorator_return: bool,

    /// Pending `ExitStack` / `AsyncExitStack` callback unwind state across frames.
    pending_exit_stack: Option<PendingExitStack>,

    /// When true, the next frame return is from a callback invoked during exit-stack unwind.
    pending_exit_stack_return: bool,

    /// Pending `ExitStack.enter_context(...)` registration while `__enter__` frame runs.
    pending_exit_stack_enter: Option<PendingExitStackEnter>,

    /// When true, the next frame return is from a pending `enter_context` call.
    pending_exit_stack_enter_return: bool,

    /// Pending `cached_property` write-back after a getter call pushed a frame.
    ///
    /// Stores the target instance and cache attribute name so the return handler
    /// can persist the computed value into `instance.__dict__`.
    pending_cached_property: Option<PendingCachedProperty>,

    /// When true, the next frame return is from a cached_property getter call.
    pending_cached_property_return: bool,

    /// Monomorphic inline-cache entry for hot `CallAttr` sites.
    ///
    /// This is intentionally a tiny single-entry cache to keep lookup overhead
    /// below the cost of generic call dispatch on tight monomorphic loops.
    call_attr_inline_cache: Option<CallAttrInlineCacheEntry>,

    /// Execution tracer for debugging, profiling, and coverage.
    ///
    /// Receives callbacks at key execution points (instruction dispatch, function
    /// calls/returns, cell access). Uses monomorphization for zero-cost when
    /// [`NoopTracer`](crate::tracer::NoopTracer) is used.
    tracer: Tr,
}

/// State for a pending `__new__` call awaiting frame return.
///
/// After `__new__` returns a value, the VM checks if it's an instance
/// of the target class. If so, `__init__` is called on the returned
/// instance. If not, the value is returned directly (no `__init__`).
pub(super) struct PendingNewCall {
    /// The class being instantiated.
    pub(super) class_heap_id: HeapId,
    /// The `__init__` method (if found), to call on the result.
    pub(super) init_func: Option<Value>,
    /// Original constructor args (to pass to `__init__`).
    pub(super) args: ArgValues,
}

/// Indicates whether a pending getattr fallback is for an instance or class object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingGetAttrKind {
    /// `__getattribute__` was called on an instance.
    Instance,
    /// `__getattribute__` was called on a class object (metaclass dispatch).
    Class,
}

/// Pending state for `__getattr__` fallback after `__getattribute__` pushed a frame.
///
/// Stores the receiver, attribute name, and frame depth so we can intercept an
/// AttributeError raised by `__getattribute__` and invoke `__getattr__` instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingGetAttr {
    /// Heap id of the receiver (instance or class object).
    obj_id: HeapId,
    /// Attribute name being looked up.
    name_id: StringId,
    /// Whether this is an instance or class attribute lookup.
    kind: PendingGetAttrKind,
    /// Frame depth of the `__getattribute__` call.
    frame_depth: usize,
}

/// Stage of a pending binary dunder dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingBinaryDunderStage {
    /// Waiting for the primary `lhs.__op__(rhs)` result.
    Primary,
    /// Waiting for the reflected `rhs.__rop__(lhs)` result.
    Reflected,
}

/// Pending state for frame-based binary dunder dispatch.
pub(super) struct PendingBinaryDunder {
    /// Left operand of the original binary operation.
    pub(super) lhs: Value,
    /// Right operand of the original binary operation.
    pub(super) rhs: Value,
    /// Primary dunder name (`__add__`, `__sub__`, ...), used for error formatting.
    pub(super) primary_dunder_id: StringId,
    /// Reflected dunder to try when primary returns `NotImplemented`.
    pub(super) reflected_dunder_id: Option<StringId>,
    /// Frame depth of the active dunder call.
    pub(super) frame_depth: usize,
    /// Which part of the protocol is currently in flight.
    pub(super) stage: PendingBinaryDunderStage,
}

/// Pending state for class construction across async frame boundaries.
///
/// This captures the intermediate class build data required to resume
/// after a user-defined `__mro_entries__` or `__prepare__` call returns.
pub(super) enum PendingClassBuild {
    /// Waiting for a `__mro_entries__` call to return.
    MroEntries {
        /// Class name being defined.
        name_id: StringId,
        /// Function id for the class body.
        func_id: FunctionId,
        /// Call site position for tracebacks.
        call_position: CodeRange,
        /// Remaining bases (not yet processed for __mro_entries__).
        remaining_bases: Vec<Value>,
        /// Bases resolved so far (after __mro_entries__ replacements).
        resolved_bases: Vec<Value>,
        /// Original bases tuple (pre-`__mro_entries__`) to pass to subsequent calls.
        orig_bases: HeapId,
        /// Class keyword arguments (excluding `metaclass`).
        class_kwargs: Value,
    },
    /// Waiting for a `metaclass.__prepare__` call to return.
    Prepare {
        /// Class name being defined.
        name_id: StringId,
        /// Function id for the class body.
        func_id: FunctionId,
        /// Call site position for tracebacks.
        call_position: CodeRange,
        /// Resolved base values (should all be classes).
        bases: Vec<Value>,
        /// Class keyword arguments (excluding `metaclass`).
        class_kwargs: Value,
        /// Selected metaclass value.
        metaclass: Value,
        /// Original bases tuple (pre-`__mro_entries__`) for `__orig_bases__`.
        orig_bases: Option<HeapId>,
    },
}

/// Result of invoking `metaclass.__prepare__` during class creation.
#[derive(Debug)]
enum PreparedNamespace {
    /// No `__prepare__` defined; use default namespace.
    None,
    /// Prepared namespace ready (mapping value).
    Ready(Value),
    /// `__prepare__` call pushed a frame; resume later.
    FramePushed,
}

/// Result of finalizing a class body frame.
#[derive(Debug)]
enum FinalizeClassResult {
    /// Class was created and is ready to push.
    Done(Value),
    /// A frame was pushed while extracting prepared namespace items.
    FramePushed,
}

/// Pending class finalization state while a helper call frame executes.
#[derive(Debug)]
enum PendingClassFinalize {
    /// Waiting for `prepared_namespace.items()` to return.
    PreparedItems {
        /// Class body metadata captured from the frame.
        class_info: ClassBodyInfo,
        /// Namespace index holding class-body locals.
        namespace_idx: NamespaceId,
        /// Call site position for the class statement.
        call_position: Option<CodeRange>,
        /// Frame depth of the helper call whose return resumes class finalization.
        pending_frame_depth: usize,
    },
    /// Waiting for a custom metaclass constructor call to return.
    MetaclassCall {
        /// Class body metadata captured from the frame.
        class_info: ClassBodyInfo,
        /// Namespace index holding class-body locals.
        namespace_idx: NamespaceId,
        /// Call site position for the class statement.
        call_position: Option<CodeRange>,
        /// The metaclass HeapId whose constructor call we are awaiting.
        metaclass_id: HeapId,
        /// Frame depth of the helper call whose return resumes class finalization.
        pending_frame_depth: usize,
    },
}

impl PendingClassFinalize {
    /// Returns the frame depth that must return before class finalization can resume.
    fn pending_frame_depth(&self) -> usize {
        match self {
            Self::PreparedItems {
                pending_frame_depth, ..
            }
            | Self::MetaclassCall {
                pending_frame_depth, ..
            } => *pending_frame_depth,
        }
    }
}

/// State for a pending `list()` construction from an instance with `__iter__`.
///
/// When `list(instance)` is called on an instance that defines `__iter__`,
/// we need to call `__iter__()` to get the iterator, then repeatedly call
/// `__next__()` until StopIteration. This struct tracks the intermediate state.
#[derive(Debug)]
pub(super) struct PendingListBuild {
    /// The iterator object returned by `__iter__()`.
    pub(super) iterator: Value,
    /// Collected items so far.
    pub(super) items: Vec<Value>,
}

/// Pending unpack mode to apply after generator list materialization completes.
#[derive(Debug, Clone, Copy)]
pub(super) enum PendingUnpack {
    /// Exact-count unpack (`a, b, c = iterable`).
    Sequence { count: usize },
    /// Star unpack (`a, *rest, b = iterable`).
    Extended { before: usize, after: usize },
}

/// Pending state for `next(generator, default)` while a generator frame executes.
#[derive(Debug)]
pub(super) struct PendingNextDefault {
    /// Generator being advanced by `next(...)`.
    pub(super) generator_id: HeapId,
    /// Default value to return when the generator exhausts.
    pub(super) default: Value,
}

/// Pending state for `defaultdict[missing_key]` while `default_factory()` runs.
#[derive(Debug)]
pub(super) struct PendingDefaultDictMissing {
    /// Target defaultdict object being populated.
    pub(super) defaultdict: Value,
    /// Missing key awaiting insertion.
    pub(super) key: Value,
}

/// Pending state for `sum(generator[, start])` while list materialization runs.
#[derive(Debug)]
pub(super) struct PendingSumFromList {
    /// Optional start argument passed to `sum()`.
    pub(super) start: Option<Value>,
}

/// Pending dunder return context for string conversions.
#[derive(Debug, Clone, Copy)]
pub(super) enum PendingStringifyReturn {
    /// Return came from `str(instance)` dispatch.
    Str,
    /// Return came from `repr(instance)` dispatch.
    Repr,
}

/// Builtins that can complete after generator list materialization.
#[derive(Debug)]
pub(super) enum PendingBuiltinFromListKind {
    /// `any(generator)`
    Any,
    /// `all(generator)`
    All,
    /// `tuple(generator)`
    Tuple,
    /// `dict(generator_of_pairs)`
    Dict,
    /// `set(generator)`
    Set,
    /// `min(generator)`
    Min,
    /// `max(generator)`
    Max,
    /// `'<sep>'.join(generator)` where separator is a concrete string value.
    Join(String),
    /// `enumerate(generator[, start])`.
    Enumerate { start: Option<Value> },
    /// `zip(...generators...)` in progress.
    ///
    /// `materialized` holds already-normalized (list or non-generator) arguments.
    /// `remaining` holds the arguments still to normalize.
    Zip {
        materialized: Vec<Value>,
        remaining: Vec<Value>,
    },
    /// `dict.update(generator, *rest, **kwargs)` after generator materialization.
    DictUpdate {
        dict_id: HeapId,
        remaining_positional: Vec<Value>,
        kwargs: crate::args::KwargsValues,
    },
    /// `heapq.merge(*iterables)` normalization in progress.
    ///
    /// `materialized` holds already-normalized positional iterables.
    /// `remaining` is the reverse-order worklist still to normalize.
    /// `kwargs` stores merge keyword arguments (`key`/`reverse`).
    HeapqMerge {
        materialized: Vec<Value>,
        remaining: Vec<Value>,
        kwargs: KwargsValues,
    },
    /// `statistics.mean/fmean/median(generator, ...)` after first-arg materialization.
    Statistics {
        function: StatisticsFunctions,
        positional_tail: Vec<Value>,
        kwargs: KwargsValues,
    },
    /// `sorted(generator, **kwargs)` after generator materialization.
    Sorted { kwargs: KwargsValues },
    /// `collections.Counter(generator, *tail, **kwargs)` after first-arg materialization.
    CollectionsCounter {
        positional_tail: Vec<Value>,
        kwargs: KwargsValues,
    },
}

/// Pending state for builtins that materialize generator input before evaluation.
#[derive(Debug)]
pub(super) struct PendingBuiltinFromList {
    /// Builtin to run after list materialization completes.
    pub(super) kind: PendingBuiltinFromListKind,
}

/// Action injected when resuming a suspended generator via `throw()`/`close()`.
#[derive(Debug)]
pub(super) enum GeneratorAction {
    /// Inject this exception value at the suspension point.
    Throw(Value),
    /// Inject `GeneratorExit` at the suspension point.
    Close,
}

/// Pending action that should be applied to the next instruction of a generator frame.
#[derive(Debug)]
pub(super) struct PendingGeneratorAction {
    /// Generator receiving the injected action.
    pub(super) generator_id: HeapId,
    /// Action to apply at the suspension point.
    pub(super) action: GeneratorAction,
}

/// How an in-flight yield-from sub-iterator call should resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum YieldFromMode {
    /// Normal delegation (`next`/`send`/`throw`) that may yield values.
    Normal,
    /// Delegation while servicing `close()`; completion should inject GeneratorExit.
    Close,
}

/// Pending state while `yield from` waits on a delegated iterator call frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct PendingYieldFrom {
    /// Outer generator executing the `yield from` expression.
    pub(super) outer_generator_id: HeapId,
    /// Bytecode IP of the `YieldFrom` opcode (used to re-execute on next/send).
    pub(super) opcode_ip: usize,
    /// Delegation mode that determines post-return behavior.
    pub(super) mode: YieldFromMode,
}

/// State for a pending `functools.reduce()` operation.
///
/// When `reduce(func, iterable)` is called with a user-defined function,
/// the VM needs to call the function for each step. If the function pushes
/// a frame (DefFunction, closure), we stash the iteration state here.
#[derive(Debug)]
pub(super) struct PendingReduce {
    /// The reduce function to apply at each step.
    pub(super) function: Value,
    /// The current accumulator value.
    pub(super) accumulator: Value,
    /// Remaining items to process (in order).
    pub(super) remaining_items: Vec<Value>,
}

/// State for a pending `map()` operation.
///
/// When `map(func, *iterables)` is called with a user-defined function,
/// the VM needs to call the function for each set of items from the iterators.
/// If the function pushes a frame, we stash the iteration state here.
#[derive(Debug)]
pub(super) struct PendingMap {
    /// The map function to apply to each set of items.
    pub(super) function: Value,
    /// Collected items from each iterator (each inner Vec is one iterator's items).
    pub(super) iterators: Vec<Vec<Value>>,
    /// Accumulated results so far.
    pub(super) results: Vec<Value>,
    /// Next item index to process.
    pub(super) current_idx: usize,
}

/// State for a pending `filter()` operation.
///
/// When `filter(func, iterable)` is called with a user-defined function,
/// the VM needs to call the function for each item and keep those where
/// the result is truthy. If the function pushes a frame, we stash the
/// iteration state here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingFilterMode {
    Filter,
    FilterFalse,
    TakeWhile,
    DropWhile,
}

#[derive(Debug)]
pub(super) struct PendingFilter {
    /// The filter function to apply to each item.
    pub(super) function: Value,
    /// Collected items from the iterable.
    pub(super) items: Vec<Value>,
    /// Accumulated results so far (items where function returned truthy).
    pub(super) results: Vec<Value>,
    /// Next item index to process.
    pub(super) current_idx: usize,
    /// Predicate consumption mode for this pending operation.
    pub(super) mode: PendingFilterMode,
    /// Whether `dropwhile` is still dropping leading truthy elements.
    pub(super) dropwhile_dropping: bool,
}

/// State for a pending `itertools.groupby(..., key=...)` operation.
///
/// Key evaluation may call user-defined functions that push frames. We stash
/// collected items, computed keys, and the next index to process across suspension.
#[derive(Debug)]
pub(super) struct PendingGroupBy {
    /// Key function to apply to each item.
    pub(super) function: Value,
    /// Original collected items from the iterable.
    pub(super) items: Vec<Value>,
    /// Computed keys so far (same order as processed items).
    pub(super) keys: Vec<Value>,
    /// Next item index to process.
    pub(super) current_idx: usize,
}

/// State for pending `textwrap.indent(..., predicate=...)` evaluation.
#[derive(Debug)]
pub(super) struct PendingTextwrapIndent {
    /// Predicate callable used to decide whether to prefix each line.
    pub(super) predicate: Value,
    /// Original lines with line endings preserved.
    pub(super) lines: Vec<String>,
    /// Prefix applied to lines when predicate is truthy.
    pub(super) prefix: String,
    /// Next line index awaiting predicate evaluation.
    pub(super) current_idx: usize,
    /// Output buffer accumulated so far.
    pub(super) output: String,
}

/// State for a pending `re.sub`/`re.subn` with callable replacement.
///
/// When the replacement function pushes a frame (user-defined function/lambda/closure),
/// we stash the remaining matches and partial result here. After each callback returns,
/// we incorporate its string return value and continue with the next match.
#[derive(Debug)]
pub(super) struct PendingReSub {
    /// The user's replacement callable.
    pub(super) function: Value,
    /// Pre-computed matches: (match_start, match_end, match_value).
    pub(super) matches: Vec<(usize, usize, Value)>,
    /// The original input string being substituted.
    pub(super) original_string: String,
    /// Whether the result should be bytes or str.
    pub(super) is_bytes: bool,
    /// If true, return (result, n_subs) tuple instead of just result.
    pub(super) return_count: bool,
    /// Replacement strings collected so far (one per processed match).
    pub(super) replacements: Vec<String>,
    /// Next match index to process.
    pub(super) current_idx: usize,
}

/// State for a pending call of a function wrapped by `@contextmanager` decorator usage.
#[derive(Debug)]
pub(super) struct PendingContextDecorator {
    /// Context manager object providing setup/cleanup.
    pub(super) generator: Value,
    /// Wrapped function callable.
    pub(super) wrapped: Value,
    /// Whether wrapped results should be awaited before cleanup.
    pub(super) async_mode: bool,
    /// Whether cleanup uses `__exit__`/`__aexit__` instead of generator close.
    pub(super) close_with_exit: bool,
    /// Arguments for the wrapped call while waiting for `__enter__` completion.
    pub(super) args: Option<ArgValues>,
    /// Return value from wrapped call while waiting for generator cleanup.
    pub(super) wrapped_result: Option<Value>,
    /// Current continuation stage.
    pub(super) stage: PendingContextDecoratorStage,
}

/// Continuation stages for pending contextlib decorator calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingContextDecoratorStage {
    /// Waiting for generator `__enter__` (`next()`) to return.
    Enter,
    /// Waiting for wrapped function call to return.
    Call,
    /// Waiting for generator `close()` to return.
    Close,
}

/// Callback kind currently awaiting frame return during `ExitStack` unwinding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingExitStackAwaiting {
    /// Callback came from `push()` / `enter_context()` and receives exception args.
    ExitLike,
    /// Callback came from `callback()` and ignores exception args.
    Callback,
}

/// Pending unwind state for `contextlib.ExitStack` / `AsyncExitStack`.
#[derive(Debug)]
pub(super) struct PendingExitStack {
    /// Registered callbacks to execute in LIFO order.
    pub(super) callbacks: Vec<ExitCallback>,
    /// Exception type currently being unwound (or `None`).
    pub(super) exc_type: Value,
    /// Exception value currently being unwound (or `None`).
    pub(super) exc_value: Value,
    /// Exception traceback currently being unwound (or `None`).
    pub(super) exc_tb: Value,
    /// Whether any exit-like callback requested suppression.
    pub(super) suppress: bool,
    /// Whether completion should return the suppression bool (`__exit__` / `__aexit__`).
    pub(super) return_suppress: bool,
    /// True when this unwind came from async entrypoints (`__aexit__`/`aclose`).
    pub(super) async_mode: bool,
    /// Callback kind waiting on a pushed frame return.
    pub(super) awaiting: Option<PendingExitStackAwaiting>,
    /// Callback currently in-flight while waiting for a frame return.
    pub(super) in_flight: Option<ExitCallback>,
}

/// Pending `ExitStack.enter_context(...)` registration across frame boundaries.
#[derive(Debug)]
pub(super) struct PendingExitStackEnter {
    /// Target exit stack object id.
    pub(super) stack_id: HeapId,
    /// Context manager whose `__enter__` is currently running.
    pub(super) manager: Value,
    /// Generator id for generator-backed context managers (yield-based enter path).
    pub(super) generator_id: Option<HeapId>,
}

/// State for a pending `list.sort(key=...)` operation.
///
/// When key evaluation invokes a user callable that pushes a frame, we keep the
/// partially computed key list and continue after the frame returns.
#[derive(Debug)]
pub(super) struct PendingListSort {
    /// Target list being sorted.
    pub(super) list_id: HeapId,
    /// True when this pending state owns an extra temporary ref to `list_id`.
    pub(super) holds_list_ref: bool,
    /// Key function callable.
    pub(super) key_fn: Value,
    /// Whether sorting order is reversed.
    pub(super) reverse: bool,
    /// Drained list items, restored after ordering is finalized.
    pub(super) items: Vec<Value>,
    /// Computed keys for `items[..next_index]`.
    pub(super) key_values: Vec<Value>,
    /// Next item index whose key still needs to be computed.
    pub(super) next_index: usize,
}

/// State for a pending `min(..., key=...)` / `max(..., key=...)` operation.
///
/// Key evaluation may call user functions that push frames. We keep the items,
/// current best value, and key-evaluation progress here across suspension.
#[derive(Debug)]
pub(super) struct PendingMinMax {
    /// Whether this operation is `min` (`true`) or `max` (`false`).
    pub(super) is_min: bool,
    /// Key callable supplied via `key=`.
    pub(super) key_fn: Value,
    /// Candidate values being compared.
    pub(super) items: Vec<Value>,
    /// Index of current best item in `items`.
    pub(super) best_index: Option<usize>,
    /// Current best key value.
    pub(super) best_key: Option<Value>,
    /// Next item index whose key still needs to be computed.
    pub(super) next_index: usize,
    /// Item index currently awaiting a key-call return.
    pub(super) awaiting_index: usize,
}

/// State for pending `heapq.nsmallest/nlargest(..., key=...)` evaluation.
///
/// Key evaluation may call user functions that push frames. This state tracks
/// item/key progress until all keys are available for final ordering.
#[derive(Debug)]
pub(super) struct PendingHeapqSelect {
    /// `true` for `nlargest`, `false` for `nsmallest`.
    pub(super) largest: bool,
    /// Number of items requested.
    pub(super) n: usize,
    /// Key callable supplied via `key=`.
    pub(super) key_fn: Value,
    /// Candidate values collected from the iterable.
    pub(super) items: Vec<Value>,
    /// Computed key values for `items[..next_index]`.
    pub(super) key_values: Vec<Value>,
    /// Next item index whose key still needs to be computed.
    pub(super) next_index: usize,
}

/// State for a pending `bisect(..., key=...)` operation.
///
/// Binary search with `key=` may call user functions on list elements. If a key
/// call pushes a frame, we stash search state and resume after return.
#[derive(Debug)]
pub(super) struct PendingBisect {
    /// `true` for bisect_left/insort_left, `false` for right variants.
    pub(super) left: bool,
    /// Whether this call should perform insertion (`insort_*`) instead of returning index.
    pub(super) insert: bool,
    /// Heap id of the target list.
    pub(super) list_id: HeapId,
    /// Owned reference to the target list to keep it alive across suspension.
    pub(super) list_value: Value,
    /// Target value `x` being located/inserted.
    pub(super) x: Value,
    /// Comparison target for binary search:
    /// - `x` for `bisect_left`/`bisect_right`
    /// - `key(x)` for `insort_left`/`insort_right` with `key=`
    pub(super) x_cmp: Value,
    /// Key callable supplied via `key=`.
    pub(super) key_fn: Value,
    /// Current binary-search lower bound (inclusive).
    pub(super) lo: usize,
    /// Current binary-search upper bound (exclusive).
    pub(super) hi: usize,
    /// Midpoint index currently awaiting a key-call return.
    pub(super) awaiting_mid: usize,
    /// Whether the next frame return should be captured as `x_cmp = key(x)`.
    pub(super) awaiting_x_key: bool,
}

/// Pending state for `cached_property` when the getter call pushed a frame.
#[derive(Debug)]
pub(super) struct PendingCachedProperty {
    /// Target instance that should receive cached value in `__dict__`.
    pub(super) instance_id: HeapId,
    /// Attribute name used as cache key.
    pub(super) attr_name: String,
}

/// Pending state for `functools.lru_cache` when the wrapped call pushed a frame.
#[derive(Debug)]
pub(super) struct PendingLruCache {
    /// Heap id of the cache wrapper object.
    pub(super) cache_id: HeapId,
    /// Cache key built from call arguments.
    pub(super) cache_key: Value,
}

/// Pre-allocated buffers that can be reused across VM executions.
///
/// After a VM is cleaned up, its operand stack and exception stack Vecs
/// are empty but retain their allocated capacity. By extracting and caching
/// these buffers, subsequent VM creations can reuse the memory instead of
/// allocating fresh Vecs. This eliminates the dominant allocation overhead
/// for short-lived executions (e.g., `1 + 2` where VM setup/teardown was 63%
/// of total time).
///
/// The frame Vec is NOT cached because `CallFrame<'a>` has a lifetime
/// parameter tied to borrowed Code references, making it impossible to store
/// across executions with different lifetimes.
#[derive(Debug)]
pub struct CachedVMBuffers {
    /// Pre-allocated operand stack (empty but retains capacity from previous run).
    stack: Vec<Value>,
    /// Pre-allocated exception stack (empty but retains capacity).
    exception_stack: Vec<Value>,
    /// Pre-allocated exception stack position metadata (empty but retains capacity).
    exception_stack_positions: Vec<usize>,
    /// Preserved monomorphic `CallAttr` inline-cache entry from the previous run.
    ///
    /// Keeping this across `run_no_limits_cached()` executions allows hot call
    /// sites that execute once per run to still benefit after warmup.
    call_attr_inline_cache: Option<CallAttrInlineCacheEntry>,
}

impl CachedVMBuffers {
    /// Creates empty cached buffers (no pre-allocation).
    ///
    /// Use this for the first execution; subsequent runs will benefit from
    /// the capacity established during the first run.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            exception_stack: Vec::new(),
            exception_stack_positions: Vec::new(),
            call_attr_inline_cache: None,
        }
    }
}

impl Default for CachedVMBuffers {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T: ResourceTracker, P: PrintWriter, Tr: VmTracer> VM<'a, T, P, Tr> {
    /// Creates a new VM with the given runtime context and execution tracer.
    ///
    /// The tracer receives callbacks at key execution points. Use [`NoopTracer`](crate::tracer::NoopTracer)
    /// for zero-overhead production execution, or one of the specialized tracers
    /// ([`StderrTracer`](crate::tracer::StderrTracer), [`ProfilingTracer`](crate::tracer::ProfilingTracer),
    /// etc.) for debugging and analysis.
    pub fn new(
        heap: &'a mut Heap<T>,
        namespaces: &'a mut Namespaces,
        interns: &'a Interns,
        print_writer: &'a mut P,
        tracer: Tr,
    ) -> Self {
        Self {
            stack: Vec::with_capacity(64),
            frames: Vec::with_capacity(16),
            heap,
            namespaces,
            interns,
            print_writer,
            exception_stack: Vec::new(),
            exception_stack_positions: Vec::new(),
            instruction_ip: 0,
            next_call_id: 0,
            scheduler: None, // Lazy - no allocation for sync code
            module_code: None,
            pending_hash_target: None,
            pending_hash_push_result: false,
            pending_stringify_return: Vec::new(),
            pending_discard_return: false,
            pending_negate_bool: false,
            pending_instancecheck_return: false,
            pending_subclasscheck_return: false,
            pending_dir_return: false,
            pending_getattr_fallback: Vec::new(),
            pending_binary_dunder: Vec::new(),
            pending_for_iter_jump: Vec::new(),
            pending_next_default: None,
            pending_defaultdict_missing: None,
            pending_defaultdict_return: false,
            pending_generator_close: None,
            pending_mod_eq_k: None,
            pending_compare_eq_jump: None,
            pending_set_name_calls: Vec::new(),
            pending_set_name_return: false,
            pending_new_call: None,
            pending_init_subclass_calls: Vec::new(),
            pending_init_subclass_return: false,
            pending_class_build: None,
            pending_class_finalize: None,
            pending_list_build: Vec::new(),
            pending_list_build_return: false,
            pending_list_iter_return: false,
            pending_unpack: None,
            pending_sum_from_list: None,
            pending_builtin_from_list: Vec::new(),
            pending_generator_action: None,
            pending_yield_from: Vec::new(),
            pending_list_sort: None,
            pending_list_sort_return: false,
            pending_min_max: None,
            pending_min_max_return: false,
            pending_heapq_select: None,
            pending_heapq_select_return: false,
            pending_bisect: None,
            pending_bisect_return: false,
            pending_reduce: None,
            pending_reduce_return: false,
            pending_map: None,
            pending_map_return: false,
            pending_filter: None,
            pending_filter_return: false,
            pending_lru_cache: Vec::new(),
            pending_lru_cache_return: false,
            pending_groupby: None,
            pending_groupby_return: false,
            pending_textwrap_indent: None,
            pending_textwrap_indent_return: false,
            pending_re_sub: None,
            pending_re_sub_return: false,
            pending_context_decorator: None,
            pending_context_decorator_return: false,
            pending_exit_stack: None,
            pending_exit_stack_return: false,
            pending_exit_stack_enter: None,
            pending_exit_stack_enter_return: false,
            pending_sorted: None,
            pending_cached_property: None,
            pending_cached_property_return: false,
            call_attr_inline_cache: None,
            tracer,
        }
    }

    /// Returns a reference to the tracer for inspecting collected data.
    ///
    /// Use this after execution to retrieve profiling reports, coverage data,
    /// or recorded events from the tracer.
    #[expect(dead_code)]
    pub fn tracer(&self) -> &Tr {
        &self.tracer
    }

    /// Returns a mutable reference to the tracer.
    #[expect(dead_code)]
    pub fn tracer_mut(&mut self) -> &mut Tr {
        &mut self.tracer
    }

    /// Creates a VM that reuses pre-allocated buffers from a previous execution.
    ///
    /// This avoids the cost of allocating fresh Vecs for the operand stack and
    /// exception stack. The cached buffers should come from a previous VM that
    /// was cleaned up via `cleanup()` followed by `take_buffers()`.
    ///
    /// For the frame Vec, a fresh allocation with capacity 1 is used since
    /// frames have a lifetime parameter that prevents caching across runs.
    /// Most simple programs only need 1 frame (the module frame); the Vec
    /// will grow on demand for programs that call functions.
    pub fn new_with_buffers(
        buffers: CachedVMBuffers,
        heap: &'a mut Heap<T>,
        namespaces: &'a mut Namespaces,
        interns: &'a Interns,
        print_writer: &'a mut P,
        tracer: Tr,
    ) -> Self {
        Self {
            stack: buffers.stack,
            frames: Vec::with_capacity(1),
            heap,
            namespaces,
            interns,
            print_writer,
            exception_stack: buffers.exception_stack,
            exception_stack_positions: buffers.exception_stack_positions,
            instruction_ip: 0,
            next_call_id: 0,
            scheduler: None,
            module_code: None,
            pending_hash_target: None,
            pending_hash_push_result: false,
            pending_stringify_return: Vec::new(),
            pending_discard_return: false,
            pending_negate_bool: false,
            pending_instancecheck_return: false,
            pending_subclasscheck_return: false,
            pending_dir_return: false,
            pending_getattr_fallback: Vec::new(),
            pending_binary_dunder: Vec::new(),
            pending_for_iter_jump: Vec::new(),
            pending_next_default: None,
            pending_defaultdict_missing: None,
            pending_defaultdict_return: false,
            pending_generator_close: None,
            pending_mod_eq_k: None,
            pending_compare_eq_jump: None,
            pending_set_name_calls: Vec::new(),
            pending_set_name_return: false,
            pending_new_call: None,
            pending_init_subclass_calls: Vec::new(),
            pending_init_subclass_return: false,
            pending_class_build: None,
            pending_class_finalize: None,
            pending_list_build: Vec::new(),
            pending_list_build_return: false,
            pending_list_iter_return: false,
            pending_unpack: None,
            pending_sum_from_list: None,
            pending_builtin_from_list: Vec::new(),
            pending_generator_action: None,
            pending_yield_from: Vec::new(),
            pending_list_sort: None,
            pending_list_sort_return: false,
            pending_min_max: None,
            pending_min_max_return: false,
            pending_heapq_select: None,
            pending_heapq_select_return: false,
            pending_bisect: None,
            pending_bisect_return: false,
            pending_reduce: None,
            pending_reduce_return: false,
            pending_map: None,
            pending_map_return: false,
            pending_filter: None,
            pending_filter_return: false,
            pending_lru_cache: Vec::new(),
            pending_lru_cache_return: false,
            pending_groupby: None,
            pending_groupby_return: false,
            pending_textwrap_indent: None,
            pending_textwrap_indent_return: false,
            pending_re_sub: None,
            pending_re_sub_return: false,
            pending_context_decorator: None,
            pending_context_decorator_return: false,
            pending_exit_stack: None,
            pending_exit_stack_return: false,
            pending_exit_stack_enter: None,
            pending_exit_stack_enter_return: false,
            pending_sorted: None,
            pending_cached_property: None,
            pending_cached_property_return: false,
            call_attr_inline_cache: buffers.call_attr_inline_cache,
            tracer,
        }
    }

    /// Extracts the reusable buffers from this VM after cleanup.
    ///
    /// Call this after `cleanup()` to reclaim the empty-but-allocated Vecs
    /// for reuse in a subsequent VM. The stack and exception_stack should be
    /// empty after cleanup (all values drained), but retain their allocated
    /// capacity from the previous run.
    ///
    /// # Panics
    /// Debug-asserts that the stack and exception_stack are empty (cleanup was called).
    pub fn take_buffers(&mut self) -> CachedVMBuffers {
        debug_assert!(
            self.stack.is_empty(),
            "take_buffers called before cleanup: stack not empty"
        );
        debug_assert!(
            self.exception_stack.is_empty(),
            "take_buffers called before cleanup: exception_stack not empty"
        );
        debug_assert!(
            self.exception_stack_positions.is_empty(),
            "take_buffers called before cleanup: exception_stack_positions not empty"
        );
        CachedVMBuffers {
            stack: std::mem::take(&mut self.stack),
            exception_stack: std::mem::take(&mut self.exception_stack),
            exception_stack_positions: std::mem::take(&mut self.exception_stack_positions),
            call_attr_inline_cache: std::mem::take(&mut self.call_attr_inline_cache),
        }
    }

    /// Reconstructs a VM from a snapshot.
    ///
    /// The heap and namespaces must already be deserialized. `FunctionId` values
    /// in frames are used to look up pre-compiled `Code` objects from the `Interns`.
    /// The `module_code` is used for frames with `function_id = None`.
    ///
    /// # Arguments
    /// * `snapshot` - The VM snapshot to restore
    /// * `module_code` - Compiled module code (for frames with function_id = None)
    /// * `heap` - The deserialized heap
    /// * `namespaces` - The deserialized namespaces
    /// * `interns` - Interns for looking up function code
    /// * `print_writer` - Writer for print output
    pub fn restore(
        snapshot: VMSnapshot,
        module_code: &'a Code,
        heap: &'a mut Heap<T>,
        namespaces: &'a mut Namespaces,
        interns: &'a Interns,
        print_writer: &'a mut P,
        tracer: Tr,
    ) -> Self {
        // Reconstruct call frames from serialized form
        let frames = snapshot
            .frames
            .into_iter()
            .map(|sf| {
                let code = match sf.function_id {
                    Some(func_id) => &interns.get_function(func_id).code,
                    None => module_code,
                };
                CallFrame {
                    code,
                    ip: sf.ip,
                    stack_base: sf.stack_base,
                    namespace_idx: sf.namespace_idx,
                    function_id: sf.function_id,
                    cells: sf.cells,
                    call_position: sf.call_position,
                    class_body_info: sf.class_body_info.map(Into::into),
                    init_instance: sf.init_instance,
                    generator_id: sf.generator_id,
                }
            })
            .collect();

        Self {
            stack: snapshot.stack,
            frames,
            heap,
            namespaces,
            interns,
            print_writer,
            exception_stack: snapshot.exception_stack,
            exception_stack_positions: snapshot.exception_stack_positions,
            instruction_ip: snapshot.instruction_ip,
            next_call_id: snapshot.next_call_id,
            scheduler: snapshot.scheduler,
            module_code: Some(module_code),
            pending_hash_target: None,
            pending_hash_push_result: false,
            pending_stringify_return: Vec::new(),
            pending_discard_return: false,
            pending_negate_bool: false,
            pending_instancecheck_return: false,
            pending_subclasscheck_return: false,
            pending_dir_return: false,
            pending_getattr_fallback: Vec::new(),
            pending_binary_dunder: Vec::new(),
            pending_for_iter_jump: Vec::new(),
            pending_next_default: None,
            pending_defaultdict_missing: None,
            pending_defaultdict_return: false,
            pending_generator_close: None,
            pending_mod_eq_k: None,
            pending_compare_eq_jump: None,
            pending_set_name_calls: Vec::new(),
            pending_set_name_return: false,
            pending_new_call: None,
            pending_init_subclass_calls: Vec::new(),
            pending_init_subclass_return: false,
            pending_class_build: None,
            pending_class_finalize: None,
            pending_list_build: Vec::new(),
            pending_list_build_return: false,
            pending_list_iter_return: false,
            pending_unpack: None,
            pending_sum_from_list: None,
            pending_builtin_from_list: Vec::new(),
            pending_generator_action: None,
            pending_yield_from: Vec::new(),
            pending_list_sort: None,
            pending_list_sort_return: false,
            pending_min_max: None,
            pending_min_max_return: false,
            pending_heapq_select: None,
            pending_heapq_select_return: false,
            pending_bisect: None,
            pending_bisect_return: false,
            pending_reduce: None,
            pending_reduce_return: false,
            pending_map: None,
            pending_map_return: false,
            pending_filter: None,
            pending_filter_return: false,
            pending_lru_cache: Vec::new(),
            pending_lru_cache_return: false,
            pending_groupby: None,
            pending_groupby_return: false,
            pending_textwrap_indent: None,
            pending_textwrap_indent_return: false,
            pending_re_sub: None,
            pending_re_sub_return: false,
            pending_context_decorator: None,
            pending_context_decorator_return: false,
            pending_exit_stack: None,
            pending_exit_stack_return: false,
            pending_exit_stack_enter: None,
            pending_exit_stack_enter_return: false,
            pending_sorted: None,
            pending_cached_property: None,
            pending_cached_property_return: false,
            call_attr_inline_cache: None,
            tracer,
        }
    }
    /// Consumes the VM and creates a snapshot for pause/resume if needed.
    pub fn check_snapshot(mut self, result: &RunResult<FrameExit>) -> Option<VMSnapshot> {
        if matches!(
            result,
            Ok(FrameExit::ExternalCall { .. }
                | FrameExit::ProxyCall { .. }
                | FrameExit::OsCall { .. }
                | FrameExit::ResolveFutures(_))
        ) {
            Some(self.snapshot())
        } else {
            self.cleanup();
            None
        }
    }

    /// Consumes the VM and creates a snapshot for pause/resume.
    ///
    /// **Ownership transfer:** This method takes `self` by value, consuming the VM.
    /// The snapshot owns all Values (refcounts already correct from the live VM).
    /// The heap and namespaces must be serialized alongside this snapshot.
    ///
    /// This is NOT a clone - it's a transfer. After calling this, the original VM
    /// is gone and only the snapshot (+ serialized heap/namespaces) represents the state.
    pub fn snapshot(self) -> VMSnapshot {
        VMSnapshot {
            // Move values directly - no clone, no refcount increment needed
            // (the VM owned them, now the snapshot owns them)
            stack: self.stack,
            frames: self.frames.into_iter().map(CallFrame::serialize).collect(),
            exception_stack: self.exception_stack,
            exception_stack_positions: self.exception_stack_positions,
            instruction_ip: self.instruction_ip,
            next_call_id: self.next_call_id,
            scheduler: self.scheduler,
        }
    }

    /// Pushes an initial frame for module-level code and runs the VM.
    pub fn run_module(&mut self, code: &'a Code) -> Result<FrameExit, RunError> {
        // Store module code for restoring main task frames during task switching
        self.module_code = Some(code);
        self.frames.push(CallFrame::new_module(code, GLOBAL_NS_IDX));
        self.tracer.on_call(Some("<module>"), self.frames.len());
        self.run()
    }

    /// Cleans up VM state before the VM is dropped.
    ///
    /// This method must be called before the VM goes out of scope to ensure
    /// proper reference counting cleanup for any exception values and scheduler state.
    pub fn cleanup(&mut self) {
        // Drop all exceptions in the exception stack
        while let Some(exc) = self.pop_exception_context() {
            exc.drop_with_heap(self.heap);
        }
        // Stack should be empty, but clean up just in case
        for value in self.stack.drain(..) {
            value.drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_class_finalize.take() {
            match pending {
                PendingClassFinalize::PreparedItems {
                    class_info,
                    namespace_idx,
                    ..
                }
                | PendingClassFinalize::MetaclassCall {
                    class_info,
                    namespace_idx,
                    ..
                } => {
                    self.namespaces.drop_with_heap(namespace_idx, self.heap);
                    self.drop_class_body_info(class_info);
                }
            }
        }
        if let Some(pending) = self.pending_reduce.take() {
            pending.function.drop_with_heap(self.heap);
            pending.accumulator.drop_with_heap(self.heap);
            for item in pending.remaining_items {
                item.drop_with_heap(self.heap);
            }
        }
        if let Some(pending) = self.pending_map.take() {
            pending.function.drop_with_heap(self.heap);
            for iter in pending.iterators {
                for item in iter {
                    item.drop_with_heap(self.heap);
                }
            }
            for result in pending.results {
                result.drop_with_heap(self.heap);
            }
        }
        if let Some(pending) = self.pending_filter.take() {
            pending.function.drop_with_heap(self.heap);
            for item in pending.items {
                item.drop_with_heap(self.heap);
            }
            for result in pending.results {
                result.drop_with_heap(self.heap);
            }
        }
        for pending in self.pending_lru_cache.drain(..) {
            pending.cache_key.drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_groupby.take() {
            pending.function.drop_with_heap(self.heap);
            for item in pending.items {
                item.drop_with_heap(self.heap);
            }
            for key in pending.keys {
                key.drop_with_heap(self.heap);
            }
        }
        if let Some(pending) = self.pending_textwrap_indent.take() {
            pending.predicate.drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_re_sub.take() {
            pending.function.drop_with_heap(self.heap);
            for (_start, _end, match_val) in pending.matches {
                match_val.drop_with_heap(self.heap);
            }
        }
        if let Some(mut pending) = self.pending_context_decorator.take() {
            pending.generator.drop_with_heap(self.heap);
            pending.wrapped.drop_with_heap(self.heap);
            if let Some(args) = pending.args.take() {
                args.drop_with_heap(self.heap);
            }
            if let Some(result) = pending.wrapped_result.take() {
                result.drop_with_heap(self.heap);
            }
        }
        if let Some(mut pending) = self.pending_exit_stack.take() {
            for callback in pending.callbacks.drain(..) {
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
            pending.exc_type.drop_with_heap(self.heap);
            pending.exc_value.drop_with_heap(self.heap);
            pending.exc_tb.drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_exit_stack_enter.take() {
            self.heap.dec_ref(pending.stack_id);
            pending.manager.drop_with_heap(self.heap);
        }
        if let Some(list_id) = self.pending_sorted.take() {
            Value::Ref(list_id).drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_min_max.take() {
            pending.key_fn.drop_with_heap(self.heap);
            for item in pending.items {
                item.drop_with_heap(self.heap);
            }
            if let Some(best_key) = pending.best_key {
                best_key.drop_with_heap(self.heap);
            }
        }
        if let Some(pending) = self.pending_heapq_select.take() {
            pending.key_fn.drop_with_heap(self.heap);
            for item in pending.items {
                item.drop_with_heap(self.heap);
            }
            for key_value in pending.key_values {
                key_value.drop_with_heap(self.heap);
            }
        }
        if let Some(pending) = self.pending_bisect.take() {
            pending.list_value.drop_with_heap(self.heap);
            pending.x.drop_with_heap(self.heap);
            pending.key_fn.drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_defaultdict_missing.take() {
            pending.defaultdict.drop_with_heap(self.heap);
            pending.key.drop_with_heap(self.heap);
        }
        if let Some(pending) = self.pending_sum_from_list.take()
            && let Some(start) = pending.start
        {
            start.drop_with_heap(self.heap);
        }
        self.clear_pending_list_build();
        self.pending_unpack = None;
        self.clear_pending_builtin_from_list();
        self.pending_stringify_return.clear();
        self.pending_for_iter_jump.clear();
        self.clear_pending_next_default();
        self.clear_pending_generator_action();
        self.pending_yield_from.clear();
        self.clear_pending_list_sort();
        self.clear_pending_min_max();
        self.clear_pending_heapq_select();
        self.clear_pending_bisect();
        self.pending_lru_cache_return = false;
        self.pending_cached_property = None;
        self.pending_cached_property_return = false;
        self.pending_textwrap_indent_return = false;
        self.pending_context_decorator_return = false;
        self.pending_exit_stack_return = false;
        self.pending_exit_stack_enter_return = false;
        self.pending_defaultdict_return = false;
        self.pending_generator_close = None;
        // Clean up current frames (main module frame after return, or any remaining frames)
        self.cleanup_current_frames();
        // Clean up task frame namespaces (scheduler doesn't have access to namespaces)
        self.cleanup_all_task_frames();
        // Clean up scheduler state (task stacks, pending calls, resolved values)
        if let Some(scheduler) = &mut self.scheduler {
            scheduler.cleanup(self.heap);
        }
    }

    /// Cleans up frames stored in all scheduler tasks.
    ///
    /// Task frames reference namespaces and cells that need to be cleaned up
    /// before the VM is dropped. This is separate from `scheduler.cleanup()`
    /// because the scheduler doesn't have access to the VM's namespaces.
    fn cleanup_all_task_frames(&mut self) {
        let Some(scheduler) = &mut self.scheduler else {
            return;
        };
        // Clean up each task's saved frames
        for task_idx in 0..scheduler.task_count() {
            let task_id = TaskId::new(u32::try_from(task_idx).expect("task_idx exceeds u32"));
            let task = scheduler.get_task_mut(task_id);
            for frame in std::mem::take(&mut task.frames) {
                // Clean up cell references
                for cell_id in frame.cells {
                    self.heap.dec_ref(cell_id);
                }
                // Clean up the namespace (but not the global namespace)
                if frame.namespace_idx != GLOBAL_NS_IDX {
                    self.namespaces.drop_with_heap(frame.namespace_idx, self.heap);
                }
            }
        }
    }

    /// Allocates a new `CallId` for an external function call.
    ///
    /// Works with or without a scheduler. If a scheduler exists, delegates to it.
    /// Otherwise, uses the VM's `next_call_id` counter directly, avoiding
    /// scheduler creation overhead for synchronous external calls.
    fn allocate_call_id(&mut self) -> CallId {
        if let Some(scheduler) = &mut self.scheduler {
            scheduler.allocate_call_id()
        } else {
            let id = CallId::new(self.next_call_id);
            self.next_call_id += 1;
            id
        }
    }

    /// Returns true if we're on the main task (or no async at all).
    ///
    /// This is used to determine whether a `ReturnValue` at the last frame means
    /// module-level completion (return to host) or spawned task completion
    /// (handle task completion and switch).
    fn is_main_task(&self) -> bool {
        self.scheduler
            .as_ref()
            .is_none_or(|s| s.current_task_id().is_none_or(TaskId::is_main))
    }

    /// Main execution loop.
    ///
    /// Fetches opcodes from the current frame's bytecode and executes them.
    /// Returns when execution completes, an error occurs, or an external
    /// call is needed.
    ///
    /// Uses locally cached `code` and `ip` variables to avoid repeated
    /// `frames.last_mut().expect()` calls during operand fetching. The cache
    /// is reloaded after any operation that modifies the frame stack.
    #[expect(unused_assignments)]
    pub fn run(&mut self) -> Result<FrameExit, RunError> {
        // Cache frame state locally to avoid repeated frames.last_mut() calls.
        // The Code reference has lifetime 'a (lives in Interns), independent of frame borrow.
        let mut cached_frame: CachedFrame<'a> = self.new_cached_frame();

        loop {
            // Check time limit and trigger GC if needed at each instruction.
            // For NoLimitTracker, these are inlined no-ops that compile away.
            self.heap.tracker_mut().check_time()?;

            if self.heap.should_gc() {
                // Sync IP before GC for safety
                self.current_frame_mut().ip = cached_frame.ip;
                self.run_gc();
            }

            // Track instruction IP for exception table lookup
            self.instruction_ip = cached_frame.ip;

            // Fetch opcode using cached values (no frame access)
            let opcode = {
                let byte = cached_frame.code.bytecode()[cached_frame.ip];
                cached_frame.ip += 1;
                Opcode::try_from(byte).expect("invalid opcode in bytecode")
            };

            // Trace hook — compiles to nothing for NoopTracer via monomorphization.
            self.tracer.on_instruction(
                cached_frame.ip - 1,
                opcode,
                self.stack.len().saturating_sub(self.current_frame().stack_base),
                self.frames.len(),
            );

            match opcode {
                // ============================================================
                // Stack Operations
                // ============================================================
                Opcode::Pop => {
                    let value = self.pop();
                    value.drop_with_heap(self.heap);
                }
                Opcode::Dup => {
                    // Copy without incrementing refcount first (avoids borrow conflict)
                    let value = self.peek().copy_for_extend();
                    // Now we can safely increment refcount and push
                    if let Value::Ref(id) = &value {
                        self.heap.inc_ref(*id);
                    }
                    self.push(value);
                }
                Opcode::Rot2 => {
                    // Swap top two: [a, b] → [b, a]
                    let len = self.stack.len();
                    self.stack.swap(len - 1, len - 2);
                }
                Opcode::Rot3 => {
                    // Rotate top three: [a, b, c] → [c, a, b]
                    // Uses in-place rotation without cloning
                    let len = self.stack.len();
                    // Move c out, then shift a→b→c, then put c at a's position
                    // Equivalent to: [..rest, a, b, c] → [..rest, c, a, b]
                    self.stack[len - 3..].rotate_right(1);
                }
                // Constants & Literals
                Opcode::LoadConst => {
                    let idx = fetch_u16!(cached_frame);
                    // Copy without incrementing refcount first (avoids borrow conflict)
                    let value = cached_frame.code.constants().get(idx).copy_for_extend();
                    // Handle InternLongInt specially - convert to heap-allocated LongInt
                    if let Value::InternLongInt(long_int_id) = value {
                        let bi = self.interns.get_long_int(long_int_id).clone();
                        match LongInt::new(bi).into_value(self.heap) {
                            Ok(v) => self.push(v),
                            Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                        }
                    } else {
                        // Now we can safely increment refcount for Ref values
                        if let Value::Ref(id) = &value {
                            self.heap.inc_ref(*id);
                        }
                        self.push(value);
                    }
                }
                Opcode::LoadNone => self.push(Value::None),
                Opcode::LoadTrue => self.push(Value::Bool(true)),
                Opcode::LoadFalse => self.push(Value::Bool(false)),
                Opcode::LoadSmallInt => {
                    let n = fetch_i8!(cached_frame);
                    self.push(Value::Int(i64::from(n)));
                }
                // Variables - Specialized Local Loads (no operand)
                Opcode::LoadLocal0 => handle_call_result!(self, cached_frame, self.load_local(&cached_frame, 0)),
                Opcode::LoadLocal1 => handle_call_result!(self, cached_frame, self.load_local(&cached_frame, 1)),
                Opcode::LoadLocal2 => handle_call_result!(self, cached_frame, self.load_local(&cached_frame, 2)),
                Opcode::LoadLocal3 => handle_call_result!(self, cached_frame, self.load_local(&cached_frame, 3)),
                // Variables - General Local Operations
                Opcode::LoadLocal => {
                    let slot = u16::from(fetch_u8!(cached_frame));
                    handle_call_result!(self, cached_frame, self.load_local(&cached_frame, slot));
                }
                Opcode::LoadLocalW => {
                    let slot = fetch_u16!(cached_frame);
                    handle_call_result!(self, cached_frame, self.load_local(&cached_frame, slot));
                }
                Opcode::StoreLocal => {
                    let slot = u16::from(fetch_u8!(cached_frame));
                    handle_void_call_result!(self, cached_frame, self.store_local(&cached_frame, slot), "StoreLocal");
                }
                Opcode::StoreLocalW => {
                    let slot = fetch_u16!(cached_frame);
                    handle_void_call_result!(self, cached_frame, self.store_local(&cached_frame, slot), "StoreLocalW");
                }
                // Variables - Specialized Local Stores (no operand)
                Opcode::StoreLocal0 => {
                    handle_void_call_result!(self, cached_frame, self.store_local(&cached_frame, 0), "StoreLocal0");
                }
                Opcode::StoreLocal1 => {
                    handle_void_call_result!(self, cached_frame, self.store_local(&cached_frame, 1), "StoreLocal1");
                }
                Opcode::StoreLocal2 => {
                    handle_void_call_result!(self, cached_frame, self.store_local(&cached_frame, 2), "StoreLocal2");
                }
                Opcode::StoreLocal3 => {
                    handle_void_call_result!(self, cached_frame, self.store_local(&cached_frame, 3), "StoreLocal3");
                }
                // Superinstruction: LoadSmallInt + StoreLocal fused
                Opcode::StoreLocalSmallInt => {
                    let slot = u16::from(fetch_u8!(cached_frame));
                    let value = fetch_i8!(cached_frame);
                    self.push(Value::Int(i64::from(value)));
                    handle_void_call_result!(
                        self,
                        cached_frame,
                        self.store_local(&cached_frame, slot),
                        "StoreLocalSmallInt"
                    );
                }
                Opcode::DeleteLocal => {
                    let slot = u16::from(fetch_u8!(cached_frame));
                    handle_void_call_result!(
                        self,
                        cached_frame,
                        self.delete_local(&cached_frame, slot),
                        "DeleteLocal"
                    );
                }
                // Variables - Global Operations
                Opcode::LoadGlobal => {
                    let slot = fetch_u16!(cached_frame);
                    try_catch_sync!(self, cached_frame, self.load_global(slot));
                }
                Opcode::StoreGlobal => {
                    let slot = fetch_u16!(cached_frame);
                    self.store_global(slot);
                }
                // Variables - Cell Operations (closures)
                Opcode::LoadCell => {
                    let slot = fetch_u16!(cached_frame);
                    try_catch_sync!(self, cached_frame, self.load_cell(slot));
                }
                Opcode::StoreCell => {
                    let slot = fetch_u16!(cached_frame);
                    try_catch_sync!(self, cached_frame, self.store_cell(slot));
                }
                // Binary Operations - use handle_call_result for dunder support
                Opcode::BinaryAdd => {
                    // Hot-path for integer addition avoids IP sync + call-result dispatch.
                    // This is the dominant case in arithmetic-heavy micro-benchmarks.
                    let len = self.stack.len();
                    if len >= 2
                        && let (Value::Int(a), Value::Int(b)) = (&self.stack[len - 2], &self.stack[len - 1])
                    {
                        let result = if let Some(v) = a.checked_add(*b) {
                            Value::Int(v)
                        } else {
                            // Overflow: promote to LongInt to preserve Python semantics.
                            let li = LongInt::from(*a) + LongInt::from(*b);
                            match li.into_value(self.heap) {
                                Ok(v) => v,
                                Err(e) => {
                                    catch_sync!(self, cached_frame, RunError::from(e));
                                    continue;
                                }
                            }
                        };
                        self.stack.truncate(len - 2);
                        self.push(result);
                        continue;
                    }
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_add());
                }
                Opcode::BinarySub => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_sub());
                }
                Opcode::BinaryMul => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_mult());
                }
                Opcode::BinaryDiv => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_div());
                }
                Opcode::BinaryFloorDiv => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_floordiv());
                }
                Opcode::BinaryMod => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_mod());
                }
                Opcode::BinaryPow => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_pow());
                }
                // Bitwise operations - with dunder fallback for instances
                Opcode::BinaryAnd => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_bitwise(BitwiseOp::And));
                }
                Opcode::BinaryOr => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_bitwise(BitwiseOp::Or));
                }
                Opcode::BinaryXor => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_bitwise(BitwiseOp::Xor));
                }
                Opcode::BinaryLShift => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_bitwise(BitwiseOp::LShift));
                }
                Opcode::BinaryRShift => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_bitwise(BitwiseOp::RShift));
                }
                Opcode::BinaryMatMul => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.binary_matmul());
                }
                // Comparison Operations - with dunder support
                Opcode::CompareEq => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_eq());
                }
                Opcode::CompareEqJumpIfFalse => {
                    let offset = fetch_i16!(cached_frame);
                    self.current_frame_mut().ip = cached_frame.ip;

                    match self.compare_eq() {
                        Ok(CallResult::Push(compare_result)) => {
                            let is_equal = compare_result.py_bool(self.heap, self.interns);
                            compare_result.drop_with_heap(self.heap);
                            if !is_equal {
                                jump_relative!(cached_frame.ip, offset);
                            }
                        }
                        Ok(CallResult::FramePushed) => {
                            // __eq__ pushed a frame. The return value is consumed by this
                            // fused opcode, so defer jump resolution to ReturnValue.
                            self.pending_compare_eq_jump = Some(offset);
                            reload_cache!(self, cached_frame);
                        }
                        Ok(CallResult::External(ext_id, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::ExternalCall {
                                ext_function_id: ext_id,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::Proxy(proxy_id, method, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::ProxyCall {
                                proxy_id,
                                method,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::OsCall(func, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::OsCall {
                                function: func,
                                args,
                                call_id,
                            });
                        }
                        Err(err) => catch_sync!(self, cached_frame, err),
                    }
                }
                Opcode::CompareNe => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_ne());
                }
                Opcode::CompareLt => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_lt());
                }
                Opcode::CompareLe => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_le());
                }
                Opcode::CompareGt => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_gt());
                }
                Opcode::CompareGe => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_ge());
                }
                Opcode::CompareIs => self.compare_is(false),
                Opcode::CompareIsNot => self.compare_is(true),
                Opcode::CompareIn => {
                    // Pre-hash instance items for hash-based native containment
                    // (`item in dict/set/frozenset`) so lookups use Python-level __hash__.
                    let stack_len = self.stack.len();
                    if stack_len >= 2 {
                        let item_heap_id = match &self.stack[stack_len - 2] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };
                        let container_heap_id = match &self.stack[stack_len - 1] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };

                        if let (Some(item_id), Some(container_id)) = (item_heap_id, container_heap_id)
                            && matches!(
                                self.heap.get(container_id),
                                HeapData::Dict(_) | HeapData::Set(_) | HeapData::FrozenSet(_)
                            )
                            && matches!(self.heap.get(item_id), HeapData::Instance(_))
                            && !self.heap.has_cached_hash(item_id)
                        {
                            let dunder_id: StringId = StaticStrings::DunderHash.into();
                            if let Some(method) = self.lookup_type_dunder(item_id, dunder_id) {
                                self.pending_hash_target = Some(item_id);
                                self.current_frame_mut().ip = self.instruction_ip;
                                cached_frame.ip = self.instruction_ip;
                                match self.call_dunder(item_id, method, ArgValues::Empty) {
                                    Ok(CallResult::FramePushed) => {
                                        reload_cache!(self, cached_frame);
                                    }
                                    Ok(CallResult::Push(hash_val)) => {
                                        self.pending_hash_target = None;
                                        #[expect(clippy::cast_sign_loss)]
                                        let hash = match &hash_val {
                                            Value::Int(i) => *i as u64,
                                            Value::Bool(b) => u64::from(*b),
                                            _ => {
                                                hash_val.drop_with_heap(self.heap);
                                                catch_sync!(
                                                    self,
                                                    cached_frame,
                                                    ExcType::type_error("__hash__ method should return an integer")
                                                );
                                                continue;
                                            }
                                        };
                                        hash_val.drop_with_heap(self.heap);
                                        self.heap.set_cached_hash(item_id, hash);
                                    }
                                    Ok(_) => {
                                        self.pending_hash_target = None;
                                    }
                                    Err(e) => {
                                        self.pending_hash_target = None;
                                        catch_sync!(self, cached_frame, e);
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_in(false));
                }
                Opcode::CompareNotIn => {
                    // Same pre-hash flow as CompareIn for hash-based native containers.
                    let stack_len = self.stack.len();
                    if stack_len >= 2 {
                        let item_heap_id = match &self.stack[stack_len - 2] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };
                        let container_heap_id = match &self.stack[stack_len - 1] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };

                        if let (Some(item_id), Some(container_id)) = (item_heap_id, container_heap_id)
                            && matches!(
                                self.heap.get(container_id),
                                HeapData::Dict(_) | HeapData::Set(_) | HeapData::FrozenSet(_)
                            )
                            && matches!(self.heap.get(item_id), HeapData::Instance(_))
                            && !self.heap.has_cached_hash(item_id)
                        {
                            let dunder_id: StringId = StaticStrings::DunderHash.into();
                            if let Some(method) = self.lookup_type_dunder(item_id, dunder_id) {
                                self.pending_hash_target = Some(item_id);
                                self.current_frame_mut().ip = self.instruction_ip;
                                cached_frame.ip = self.instruction_ip;
                                match self.call_dunder(item_id, method, ArgValues::Empty) {
                                    Ok(CallResult::FramePushed) => {
                                        reload_cache!(self, cached_frame);
                                    }
                                    Ok(CallResult::Push(hash_val)) => {
                                        self.pending_hash_target = None;
                                        #[expect(clippy::cast_sign_loss)]
                                        let hash = match &hash_val {
                                            Value::Int(i) => *i as u64,
                                            Value::Bool(b) => u64::from(*b),
                                            _ => {
                                                hash_val.drop_with_heap(self.heap);
                                                catch_sync!(
                                                    self,
                                                    cached_frame,
                                                    ExcType::type_error("__hash__ method should return an integer")
                                                );
                                                continue;
                                            }
                                        };
                                        hash_val.drop_with_heap(self.heap);
                                        self.heap.set_cached_hash(item_id, hash);
                                    }
                                    Ok(_) => {
                                        self.pending_hash_target = None;
                                    }
                                    Err(e) => {
                                        self.pending_hash_target = None;
                                        catch_sync!(self, cached_frame, e);
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.compare_in(true));
                }
                Opcode::CompareModEq => {
                    let const_idx = fetch_u16!(cached_frame);
                    let k = cached_frame.code.constants().get(const_idx);
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.compare_mod_eq(k) {
                        Ok(CallResult::Push(result)) => self.push(result),
                        Ok(CallResult::FramePushed) => {
                            // __mod__/__rmod__ dunder pushed a frame.
                            // Store k for post-return comparison.
                            self.pending_mod_eq_k = Some(k.copy_for_extend());
                            reload_cache!(self, cached_frame);
                        }
                        Ok(_) => {}
                        Err(err) => catch_sync!(self, cached_frame, err),
                    }
                }
                // Unary Operations
                Opcode::UnaryNot => {
                    // UnaryNot with __bool__ dunder support
                    let value = self.pop();
                    // For instances, try __bool__ dunder
                    if let Value::Ref(id) = &value
                        && matches!(self.heap.get(*id), HeapData::Instance(_))
                    {
                        let dunder_id = StaticStrings::DunderBool.into();
                        if let Some(method) = self.lookup_type_dunder(*id, dunder_id) {
                            // Call __bool__, negate the result
                            // For FramePushed, we'll get the result later and
                            // the UnaryNot is already implicit at opcode level
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(*id, method, ArgValues::Empty) {
                                Ok(CallResult::Push(bool_val)) => {
                                    let b = bool_val.py_bool(self.heap, self.interns);
                                    bool_val.drop_with_heap(self.heap);
                                    value.drop_with_heap(self.heap);
                                    self.push(Value::Bool(!b));
                                }
                                Ok(CallResult::FramePushed) => {
                                    // __bool__ pushed a frame. Use pending_negate_bool to
                                    // negate the return value when the frame returns.
                                    value.drop_with_heap(self.heap);
                                    self.pending_negate_bool = true;
                                    reload_cache!(self, cached_frame);
                                }
                                Err(e) => {
                                    value.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    value.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                        // Try __len__ as bool fallback
                        let len_id = StaticStrings::DunderLen.into();
                        if let Some(method) = self.lookup_type_dunder(*id, len_id) {
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(*id, method, ArgValues::Empty) {
                                Ok(CallResult::Push(len_val)) => {
                                    let b = match &len_val {
                                        Value::Int(n) => *n != 0,
                                        _ => len_val.py_bool(self.heap, self.interns),
                                    };
                                    len_val.drop_with_heap(self.heap);
                                    value.drop_with_heap(self.heap);
                                    self.push(Value::Bool(!b));
                                }
                                Ok(CallResult::FramePushed) => {
                                    value.drop_with_heap(self.heap);
                                    reload_cache!(self, cached_frame);
                                }
                                Err(e) => {
                                    value.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    value.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                    }
                    let result = !value.py_bool(self.heap, self.interns);
                    value.drop_with_heap(self.heap);
                    self.push(Value::Bool(result));
                }
                Opcode::UnaryNeg => {
                    // Unary minus - negate numeric value, with __neg__ dunder
                    let value = self.pop();
                    // Try __neg__ dunder for instances
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.try_unary_dunder(&value, StaticStrings::DunderNeg.into()) {
                        Ok(Some(result)) => {
                            value.drop_with_heap(self.heap);
                            match result {
                                CallResult::Push(v) => self.push(v),
                                CallResult::FramePushed => reload_cache!(self, cached_frame),
                                _ => {}
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                            continue;
                        }
                    }
                    match crate::value::py_counter_unary_neg(&value, self.heap, self.interns) {
                        Ok(Some(result)) => {
                            value.drop_with_heap(self.heap);
                            self.push(result);
                            continue;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                            continue;
                        }
                    }
                    match value {
                        Value::Int(n) => {
                            if let Some(negated) = n.checked_neg() {
                                self.push(Value::Int(negated));
                            } else {
                                let li = -LongInt::from(n);
                                match li.into_value(self.heap) {
                                    Ok(v) => self.push(v),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            }
                        }
                        Value::Float(f) => self.push(Value::Float(-f)),
                        Value::Bool(b) => self.push(Value::Int(if b { -1 } else { 0 })),
                        Value::Ref(id) => {
                            if let HeapData::StdlibObject(StdlibObject::Complex { real, imag }) = self.heap.get(id) {
                                let result = self
                                    .heap
                                    .allocate(HeapData::StdlibObject(StdlibObject::new_complex(-*real, -*imag)));
                                value.drop_with_heap(self.heap);
                                match result {
                                    Ok(id) => self.push(Value::Ref(id)),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            } else if let HeapData::LongInt(li) = self.heap.get(id) {
                                let negated = -LongInt::new(li.inner().clone());
                                value.drop_with_heap(self.heap);
                                match negated.into_value(self.heap) {
                                    Ok(v) => self.push(v),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            } else if let HeapData::Fraction(fraction) = self.heap.get(id) {
                                let negated = -fraction.clone();
                                value.drop_with_heap(self.heap);
                                match negated.to_value(self.heap) {
                                    Ok(v) => self.push(v),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            } else if let Some((mu, sigma)) = extract_normaldist_params(&value, self.heap) {
                                value.drop_with_heap(self.heap);
                                match create_normaldist_value(self.heap, -mu, sigma) {
                                    Ok(v) => self.push(v),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            } else {
                                let value_type = value.py_type(self.heap);
                                value.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, ExcType::unary_type_error("-", value_type));
                            }
                        }
                        _ => {
                            let value_type = value.py_type(self.heap);
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, ExcType::unary_type_error("-", value_type));
                        }
                    }
                }
                Opcode::UnaryPos => {
                    // Unary plus with __pos__ dunder
                    let value = self.pop();
                    // Try __pos__ dunder for instances
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.try_unary_dunder(&value, StaticStrings::DunderPos.into()) {
                        Ok(Some(result)) => {
                            value.drop_with_heap(self.heap);
                            match result {
                                CallResult::Push(v) => self.push(v),
                                CallResult::FramePushed => reload_cache!(self, cached_frame),
                                _ => {}
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                            continue;
                        }
                    }
                    match crate::value::py_counter_unary_pos(&value, self.heap, self.interns) {
                        Ok(Some(result)) => {
                            value.drop_with_heap(self.heap);
                            self.push(result);
                            continue;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                            continue;
                        }
                    }
                    match value {
                        Value::Int(_) | Value::Float(_) => self.push(value),
                        Value::Bool(b) => self.push(Value::Int(i64::from(b))),
                        Value::Ref(id) => {
                            if matches!(self.heap.get(id), HeapData::LongInt(_))
                                || matches!(self.heap.get(id), HeapData::StdlibObject(StdlibObject::Complex { .. }))
                            {
                                self.push(value);
                            } else if let Some((mu, sigma)) = extract_normaldist_params(&value, self.heap) {
                                value.drop_with_heap(self.heap);
                                match create_normaldist_value(self.heap, mu, sigma) {
                                    Ok(v) => self.push(v),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            } else {
                                let value_type = value.py_type(self.heap);
                                value.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, ExcType::unary_type_error("+", value_type));
                            }
                        }
                        _ => {
                            let value_type = value.py_type(self.heap);
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, ExcType::unary_type_error("+", value_type));
                        }
                    }
                }
                Opcode::UnaryInvert => {
                    // Bitwise NOT with __invert__ dunder
                    let value = self.pop();
                    // Try __invert__ dunder for instances
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.try_unary_dunder(&value, StaticStrings::DunderInvert.into()) {
                        Ok(Some(result)) => {
                            value.drop_with_heap(self.heap);
                            match result {
                                CallResult::Push(v) => self.push(v),
                                CallResult::FramePushed => reload_cache!(self, cached_frame),
                                _ => {}
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                            continue;
                        }
                    }
                    match value {
                        Value::Int(n) => self.push(Value::Int(!n)),
                        Value::Bool(b) => self.push(Value::Int(!i64::from(b))),
                        Value::Ref(id) => {
                            if let HeapData::LongInt(li) = self.heap.get(id) {
                                let inverted = -(li.inner() + 1i32);
                                value.drop_with_heap(self.heap);
                                match LongInt::new(inverted).into_value(self.heap) {
                                    Ok(v) => self.push(v),
                                    Err(e) => catch_sync!(self, cached_frame, RunError::from(e)),
                                }
                            } else {
                                let value_type = value.py_type(self.heap);
                                value.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, ExcType::unary_type_error("~", value_type));
                            }
                        }
                        _ => {
                            let value_type = value.py_type(self.heap);
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, ExcType::unary_type_error("~", value_type));
                        }
                    }
                }
                // In-place Operations - use handle_call_result for dunder support
                Opcode::InplaceAdd => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_add());
                }
                Opcode::InplaceSub => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_sub());
                }
                Opcode::InplaceMul => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_mul());
                }
                Opcode::InplaceDiv => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_div());
                }
                Opcode::InplaceFloorDiv => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_floordiv());
                }
                Opcode::InplaceMod => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_mod());
                }
                Opcode::InplacePow => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_pow());
                }
                Opcode::InplaceAnd => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_bitwise(BitwiseOp::And));
                }
                Opcode::InplaceOr => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_bitwise(BitwiseOp::Or));
                }
                Opcode::InplaceXor => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_bitwise(BitwiseOp::Xor));
                }
                Opcode::InplaceLShift => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_bitwise(BitwiseOp::LShift));
                }
                Opcode::InplaceRShift => {
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.inplace_bitwise(BitwiseOp::RShift));
                }
                // Collection Building - route through exception handling
                Opcode::BuildList => {
                    let count = fetch_u16!(cached_frame) as usize;
                    try_catch_sync!(self, cached_frame, self.build_list(count));
                }
                Opcode::BuildListHint => {
                    let const_idx = fetch_u16!(cached_frame);
                    let hint = cached_frame.code.constants().get(const_idx);
                    try_catch_sync!(self, cached_frame, self.build_list_with_hint(hint));
                }
                Opcode::BuildTuple => {
                    let count = fetch_u16!(cached_frame) as usize;
                    try_catch_sync!(self, cached_frame, self.build_tuple(count));
                }
                Opcode::BuildDict => {
                    let count = fetch_u16!(cached_frame) as usize;
                    // Pre-hash any instance keys before building the dict.
                    // Scan stack for instance keys that need __hash__ dispatch.
                    let stack_len = self.stack.len();
                    let mut needs_hash = None;
                    for i in 0..count {
                        let key_pos = stack_len - count * 2 + i * 2;
                        if let Value::Ref(key_id) = &self.stack[key_pos] {
                            let key_id = *key_id;
                            if matches!(self.heap.get(key_id), HeapData::Instance(_))
                                && !self.heap.has_cached_hash(key_id)
                            {
                                let dunder_id: StringId = StaticStrings::DunderHash.into();
                                if let Some(method) = self.lookup_type_dunder(key_id, dunder_id) {
                                    needs_hash = Some((key_id, method));
                                    break;
                                }
                            }
                        }
                    }
                    if let Some((key_id, method)) = needs_hash {
                        self.pending_hash_target = Some(key_id);
                        self.current_frame_mut().ip = self.instruction_ip;
                        cached_frame.ip = self.instruction_ip;
                        match self.call_dunder(key_id, method, ArgValues::Empty) {
                            Ok(CallResult::FramePushed) => {
                                reload_cache!(self, cached_frame);
                            }
                            Ok(CallResult::Push(hash_val)) => {
                                self.pending_hash_target = None;
                                #[expect(clippy::cast_sign_loss)]
                                let hash = if let Value::Int(i) = &hash_val {
                                    *i as u64
                                } else {
                                    hash_val.drop_with_heap(self.heap);
                                    catch_sync!(
                                        self,
                                        cached_frame,
                                        ExcType::type_error("__hash__ method should return an integer")
                                    );
                                    continue;
                                };
                                hash_val.drop_with_heap(self.heap);
                                self.heap.set_cached_hash(key_id, hash);
                            }
                            Ok(_) => {
                                self.pending_hash_target = None;
                            }
                            Err(e) => {
                                self.pending_hash_target = None;
                                catch_sync!(self, cached_frame, e);
                            }
                        }
                        continue;
                    }
                    try_catch_sync!(self, cached_frame, self.build_dict(count));
                }
                Opcode::BuildSet => {
                    let count = fetch_u16!(cached_frame) as usize;
                    try_catch_sync!(self, cached_frame, self.build_set(count));
                }
                Opcode::FormatValue => {
                    let flags = fetch_u8!(cached_frame);
                    try_catch_sync!(self, cached_frame, self.format_value(flags));
                }
                Opcode::BuildFString => {
                    let count = fetch_u16!(cached_frame) as usize;
                    try_catch_sync!(self, cached_frame, self.build_fstring(count));
                }
                Opcode::BuildSlice => {
                    try_catch_sync!(self, cached_frame, self.build_slice());
                }
                Opcode::ListExtend => {
                    try_catch_sync!(self, cached_frame, self.list_extend());
                }
                Opcode::ListToTuple => {
                    try_catch_sync!(self, cached_frame, self.list_to_tuple());
                }
                Opcode::DictMerge => {
                    let func_name_id = fetch_u16!(cached_frame);
                    try_catch_sync!(self, cached_frame, self.dict_merge(func_name_id));
                }
                Opcode::DictUpdate => {
                    try_catch_sync!(self, cached_frame, self.dict_update());
                }
                // Comprehension Building - append/add/set items during iteration
                Opcode::ListAppend => {
                    let depth = fetch_u8!(cached_frame) as usize;
                    try_catch_sync!(self, cached_frame, self.list_append(depth));
                }
                Opcode::SetAdd => {
                    let depth = fetch_u8!(cached_frame) as usize;
                    try_catch_sync!(self, cached_frame, self.set_add(depth));
                }
                Opcode::DictSetItem => {
                    let depth = fetch_u8!(cached_frame) as usize;
                    // Pre-hash instance keys: if the key is an instance with __hash__
                    // and no cached hash yet, call __hash__() first. When it returns,
                    // the hash will be cached and we re-execute DictSetItem.
                    let stack_len = self.stack.len();
                    if stack_len >= 2 {
                        let key = &self.stack[stack_len - 2]; // key is below value
                        if let Value::Ref(key_id) = key {
                            let key_id = *key_id;
                            if matches!(self.heap.get(key_id), HeapData::Instance(_))
                                && !self.heap.has_cached_hash(key_id)
                            {
                                let dunder_id: StringId = StaticStrings::DunderHash.into();
                                if let Some(method) = self.lookup_type_dunder(key_id, dunder_id) {
                                    // Save IP to re-execute this opcode after __hash__ returns
                                    self.pending_hash_target = Some(key_id);
                                    self.current_frame_mut().ip = self.instruction_ip;
                                    cached_frame.ip = self.instruction_ip;
                                    match self.call_dunder(key_id, method, ArgValues::Empty) {
                                        Ok(CallResult::FramePushed) => {
                                            reload_cache!(self, cached_frame);
                                        }
                                        Ok(CallResult::Push(hash_val)) => {
                                            // Synchronous return (unlikely for closures)
                                            self.pending_hash_target = None;
                                            #[expect(clippy::cast_sign_loss)]
                                            let hash = if let Value::Int(i) = &hash_val {
                                                *i as u64
                                            } else {
                                                hash_val.drop_with_heap(self.heap);
                                                catch_sync!(
                                                    self,
                                                    cached_frame,
                                                    ExcType::type_error("__hash__ method should return an integer")
                                                );
                                                continue;
                                            };
                                            hash_val.drop_with_heap(self.heap);
                                            self.heap.set_cached_hash(key_id, hash);
                                            // Fall through - IP has been reset, will re-execute DictSetItem
                                        }
                                        Ok(_) => {
                                            self.pending_hash_target = None;
                                        }
                                        Err(e) => {
                                            self.pending_hash_target = None;
                                            catch_sync!(self, cached_frame, e);
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                    try_catch_sync!(self, cached_frame, self.dict_set_item(depth));
                }
                // Subscript & Attribute - route through exception handling
                Opcode::BinarySubscr => {
                    let stack_len = self.stack.len();
                    if stack_len >= 2 {
                        // Extract heap IDs from stack without holding references to self.stack
                        let obj_heap_id = match &self.stack[stack_len - 2] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };
                        let idx_heap_id = match &self.stack[stack_len - 1] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };

                        // Check if obj is an instance with __getitem__
                        if let Some(obj_id) = obj_heap_id
                            && matches!(self.heap.get(obj_id), HeapData::Instance(_))
                        {
                            let dunder_id: StringId = StaticStrings::DunderGetitem.into();
                            if let Some(method) = self.lookup_type_dunder(obj_id, dunder_id) {
                                let idx = self.pop();
                                let obj_val = self.pop();
                                self.current_frame_mut().ip = cached_frame.ip;
                                match self.call_dunder(obj_id, method, ArgValues::One(idx)) {
                                    Ok(result) => {
                                        obj_val.drop_with_heap(self.heap);
                                        match result {
                                            CallResult::Push(v) => self.push(v),
                                            CallResult::FramePushed => reload_cache!(self, cached_frame),
                                            _ => {}
                                        }
                                    }
                                    Err(e) => {
                                        obj_val.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                }
                                continue;
                            }
                        }

                        // Check if obj is a ClassObject with __class_getitem__
                        if let Some(obj_id) = obj_heap_id
                            && matches!(self.heap.get(obj_id), HeapData::ClassObject(_))
                        {
                            let cgi_id: StringId = StaticStrings::DunderClassGetitem.into();
                            // Look up __class_getitem__ in the class's namespace via MRO
                            let mut method = match self.heap.get(obj_id) {
                                HeapData::ClassObject(cls) => {
                                    let cgi_str = self.interns.get_str(cgi_id);
                                    cls.mro_lookup_attr(cgi_str, obj_id, self.heap, self.interns)
                                        .map(|(v, _)| v)
                                }
                                _ => None,
                            };
                            if method.is_none() {
                                self.heap.inc_ref(obj_id);
                                let cgi_id = self
                                    .heap
                                    .allocate(HeapData::ClassGetItem(crate::types::ClassGetItem::new(obj_id)))?;
                                method = Some(Value::Ref(cgi_id));
                            }
                            if let Some(method_val) = method {
                                let idx = self.pop();
                                let obj_val = self.pop();
                                self.current_frame_mut().ip = cached_frame.ip;
                                // Call __class_getitem__ with descriptor-aware binding.
                                let (callable, args) = match method_val {
                                    Value::Ref(ref_id) => match self.heap.get(ref_id) {
                                        HeapData::ClassMethod(cm) => {
                                            let func = cm.func().clone_with_heap(self.heap);
                                            Value::Ref(ref_id).drop_with_heap(self.heap);
                                            self.heap.inc_ref(obj_id);
                                            let bound_id = self.heap.allocate(HeapData::BoundMethod(
                                                crate::types::BoundMethod::new(func, Value::Ref(obj_id)),
                                            ))?;
                                            (Value::Ref(bound_id), ArgValues::One(idx))
                                        }
                                        HeapData::StaticMethod(sm) => {
                                            let func = sm.func().clone_with_heap(self.heap);
                                            Value::Ref(ref_id).drop_with_heap(self.heap);
                                            (func, ArgValues::One(idx))
                                        }
                                        _ => {
                                            self.heap.inc_ref(obj_id);
                                            (Value::Ref(ref_id), ArgValues::Two(Value::Ref(obj_id), idx))
                                        }
                                    },
                                    other => {
                                        self.heap.inc_ref(obj_id);
                                        (other, ArgValues::Two(Value::Ref(obj_id), idx))
                                    }
                                };
                                match self.call_function(callable, args) {
                                    Ok(CallResult::Push(v)) => {
                                        obj_val.drop_with_heap(self.heap);
                                        self.push(v);
                                    }
                                    Ok(CallResult::FramePushed) => {
                                        obj_val.drop_with_heap(self.heap);
                                        reload_cache!(self, cached_frame);
                                    }
                                    Err(e) => {
                                        obj_val.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                    _ => {
                                        obj_val.drop_with_heap(self.heap);
                                    }
                                }
                                continue;
                            }
                        }

                        // Check for built-in type subscripting (PEP 585).
                        if let Value::Builtin(crate::builtins::Builtins::Type(_)) = &self.stack[stack_len - 2] {
                            let idx = self.pop();
                            let obj_val = self.pop();
                            self.current_frame_mut().ip = cached_frame.ip;
                            match crate::types::make_generic_alias(obj_val, idx, self.heap, self.interns) {
                                Ok(alias) => self.push(alias),
                                Err(e) => catch_sync!(self, cached_frame, e),
                            }
                            continue;
                        }

                        // Check if index is an instance with __index__ (for list/tuple indexing)
                        if let Some(idx_id) = idx_heap_id
                            && matches!(self.heap.get(idx_id), HeapData::Instance(_))
                        {
                            let dunder_id: StringId = StaticStrings::DunderIndex.into();
                            if let Some(method) = self.lookup_type_dunder(idx_id, dunder_id) {
                                // Call __index__() to get an int, replace index on stack
                                // Back up IP so BinarySubscr re-executes with the int index
                                self.current_frame_mut().ip = self.instruction_ip;
                                cached_frame.ip = self.instruction_ip;
                                let idx_val = self.pop(); // pop instance index
                                match self.call_dunder(idx_id, method, ArgValues::Empty) {
                                    Ok(CallResult::Push(int_val)) => {
                                        idx_val.drop_with_heap(self.heap);
                                        self.push(int_val); // push int result as new index
                                        // IP backed up, so BinarySubscr will re-execute
                                    }
                                    Ok(CallResult::FramePushed) => {
                                        idx_val.drop_with_heap(self.heap);
                                        // __index__ frame will push result, then re-execute
                                        reload_cache!(self, cached_frame);
                                    }
                                    Err(e) => {
                                        idx_val.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                    _ => {
                                        idx_val.drop_with_heap(self.heap);
                                    }
                                }
                                continue;
                            }

                            // Pre-hash instance keys for dict lookup
                            if let Some(obj_id) = obj_heap_id
                                && matches!(self.heap.get(obj_id), HeapData::Dict(_))
                                && !self.heap.has_cached_hash(idx_id)
                            {
                                let hash_dunder_id: StringId = StaticStrings::DunderHash.into();
                                if let Some(method) = self.lookup_type_dunder(idx_id, hash_dunder_id) {
                                    self.pending_hash_target = Some(idx_id);
                                    self.current_frame_mut().ip = self.instruction_ip;
                                    cached_frame.ip = self.instruction_ip;
                                    match self.call_dunder(idx_id, method, ArgValues::Empty) {
                                        Ok(CallResult::FramePushed) => {
                                            reload_cache!(self, cached_frame);
                                        }
                                        Ok(CallResult::Push(hash_val)) => {
                                            self.pending_hash_target = None;
                                            #[expect(clippy::cast_sign_loss)]
                                            let hash = if let Value::Int(i) = &hash_val {
                                                *i as u64
                                            } else {
                                                hash_val.drop_with_heap(self.heap);
                                                catch_sync!(
                                                    self,
                                                    cached_frame,
                                                    ExcType::type_error("__hash__ method should return an integer")
                                                );
                                                continue;
                                            };
                                            hash_val.drop_with_heap(self.heap);
                                            self.heap.set_cached_hash(idx_id, hash);
                                        }
                                        Ok(_) => {
                                            self.pending_hash_target = None;
                                        }
                                        Err(e) => {
                                            self.pending_hash_target = None;
                                            catch_sync!(self, cached_frame, e);
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                    let index = self.pop();
                    let mut obj = self.pop();

                    if let Value::Ref(defaultdict_id) = &obj
                        && matches!(self.heap.get(*defaultdict_id), HeapData::DefaultDict(_))
                    {
                        let existing = self
                            .heap
                            .with_entry_mut(*defaultdict_id, |heap_inner, data| match data {
                                HeapData::DefaultDict(default_dict) => {
                                    default_dict.get_existing(&index, heap_inner, self.interns)
                                }
                                _ => Err(RunError::internal("defaultdict access target was not DefaultDict")),
                            });
                        match existing {
                            Ok(Some(value)) => {
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                self.push(value);
                                continue;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, err);
                                continue;
                            }
                        }

                        let factory = self
                            .heap
                            .with_entry_mut(*defaultdict_id, |heap_inner, data| match data {
                                HeapData::DefaultDict(default_dict) => {
                                    Ok(default_dict.default_factory_cloned(heap_inner))
                                }
                                _ => Err(RunError::internal("defaultdict access target was not DefaultDict")),
                            });
                        let factory = match factory {
                            Ok(factory) => factory,
                            Err(err) => {
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, err);
                                continue;
                            }
                        };

                        let Some(factory) = factory else {
                            let err = ExcType::key_error(&index, self.heap, self.interns);
                            obj.drop_with_heap(self.heap);
                            index.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, err);
                            continue;
                        };

                        self.current_frame_mut().ip = cached_frame.ip;
                        match self.call_function(factory, ArgValues::Empty) {
                            Ok(CallResult::Push(default_value)) => {
                                let key_for_insert = index.clone_with_heap(self.heap);
                                let value_for_insert = default_value.clone_with_heap(self.heap);
                                let insert_result = self.heap.with_entry_mut(
                                    *defaultdict_id,
                                    |heap_inner, data| -> Result<(), RunError> {
                                        let HeapData::DefaultDict(default_dict) = data else {
                                            key_for_insert.drop_with_heap(heap_inner);
                                            value_for_insert.drop_with_heap(heap_inner);
                                            return Err(RunError::internal(
                                                "defaultdict access target was not DefaultDict",
                                            ));
                                        };
                                        default_dict.insert_default(
                                            key_for_insert,
                                            value_for_insert,
                                            heap_inner,
                                            self.interns,
                                        )
                                    },
                                );
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                match insert_result {
                                    Ok(()) => self.push(default_value),
                                    Err(err) => {
                                        default_value.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, err);
                                    }
                                }
                                continue;
                            }
                            Ok(CallResult::FramePushed) => {
                                self.pending_defaultdict_missing = Some(PendingDefaultDictMissing {
                                    defaultdict: obj.clone_with_heap(self.heap),
                                    key: index.clone_with_heap(self.heap),
                                });
                                self.pending_defaultdict_return = true;
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                reload_cache!(self, cached_frame);
                                continue;
                            }
                            Ok(CallResult::External(_, _) | CallResult::Proxy(_, _, _) | CallResult::OsCall(_, _)) => {
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                catch_sync!(
                                    self,
                                    cached_frame,
                                    ExcType::type_error(
                                        "defaultdict default_factory cannot be external in subscript access",
                                    )
                                );
                                continue;
                            }
                            Err(err) => {
                                obj.drop_with_heap(self.heap);
                                index.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, err);
                                continue;
                            }
                        }
                    }

                    let result = obj.py_getitem(&index, self.heap, self.interns);
                    obj.drop_with_heap(self.heap);
                    index.drop_with_heap(self.heap);
                    match result {
                        Ok(v) => self.push(v),
                        Err(e) => catch_sync!(self, cached_frame, e),
                    }
                }
                Opcode::StoreSubscr => {
                    // Stack order: value, obj, index (TOS)
                    let stack_len = self.stack.len();
                    if stack_len >= 3 {
                        let obj_heap_id = match &self.stack[stack_len - 2] {
                            Value::Ref(id) => Some(*id),
                            _ => None,
                        };
                        // Check if obj is an instance with __setitem__
                        if let Some(obj_id) = obj_heap_id
                            && matches!(self.heap.get(obj_id), HeapData::Instance(_))
                        {
                            let dunder_id: StringId = StaticStrings::DunderSetitem.into();
                            if let Some(method) = self.lookup_type_dunder(obj_id, dunder_id) {
                                let index = self.pop();
                                let obj_val = self.pop();
                                let value = self.pop();
                                self.current_frame_mut().ip = cached_frame.ip;
                                match self.call_dunder(obj_id, method, ArgValues::Two(index, value)) {
                                    Ok(result) => {
                                        obj_val.drop_with_heap(self.heap);
                                        match result {
                                            CallResult::Push(v) => {
                                                v.drop_with_heap(self.heap); // __setitem__ returns None
                                            }
                                            CallResult::FramePushed => {
                                                self.pending_discard_return = true;
                                                reload_cache!(self, cached_frame);
                                            }
                                            _ => {}
                                        }
                                    }
                                    Err(e) => {
                                        obj_val.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                }
                                continue;
                            }
                        }
                        // WeakKeyDictionary store: preserve original key object on equality match.
                        if let Some(obj_id) = obj_heap_id
                            && self.heap.is_weak_key_dict(obj_id)
                        {
                            let index = self.pop();
                            let obj_val = self.pop();
                            let value = self.pop();
                            let result = self.heap.set_weak_key_dict_item(obj_id, index, value, self.interns);
                            obj_val.drop_with_heap(self.heap);
                            if let Err(e) = result {
                                catch_sync!(self, cached_frame, e);
                            }
                            continue;
                        }
                    }
                    let index = self.pop();
                    let mut obj = self.pop();
                    let value = self.pop();
                    let result = obj.py_setitem(index, value, self.heap, self.interns);
                    obj.drop_with_heap(self.heap);
                    if let Err(e) = result {
                        catch_sync!(self, cached_frame, e);
                    }
                }
                Opcode::DeleteSubscr => {
                    // del obj[key] -> pop key, pop obj, call py_delitem
                    // Check for instance __delitem__
                    let stack_len = self.stack.len();
                    if stack_len >= 2 {
                        let obj = &self.stack[stack_len - 2]; // obj is below key
                        if let Value::Ref(obj_id) = obj {
                            let obj_id = *obj_id;
                            if matches!(self.heap.get(obj_id), HeapData::Instance(_)) {
                                let dunder_id: StringId = StaticStrings::DunderDelitem.into();
                                if let Some(method) = self.lookup_type_dunder(obj_id, dunder_id) {
                                    let index = self.pop();
                                    let obj_val = self.pop();
                                    self.current_frame_mut().ip = cached_frame.ip;
                                    match self.call_dunder(obj_id, method, ArgValues::One(index)) {
                                        Ok(result) => {
                                            obj_val.drop_with_heap(self.heap);
                                            match result {
                                                CallResult::Push(v) => {
                                                    v.drop_with_heap(self.heap);
                                                }
                                                CallResult::FramePushed => {
                                                    self.pending_discard_return = true;
                                                    reload_cache!(self, cached_frame);
                                                }
                                                _ => {}
                                            }
                                        }
                                        Err(e) => {
                                            obj_val.drop_with_heap(self.heap);
                                            catch_sync!(self, cached_frame, e);
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                    let index = self.pop();
                    let mut obj = self.pop();
                    let result = obj.py_delitem(index, self.heap, self.interns);
                    obj.drop_with_heap(self.heap);
                    try_catch_sync!(self, cached_frame, result);
                }
                Opcode::LoadAttr => {
                    let name_idx = fetch_u16!(cached_frame);
                    let name_id = StringId::from_index(name_idx);
                    // Sync IP before call - property getters may push a frame,
                    // and the caller frame needs the correct IP for when it resumes.
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.load_attr(name_id));
                }
                Opcode::LoadAttrImport => {
                    let name_idx = fetch_u16!(cached_frame);
                    let name_id = StringId::from_index(name_idx);
                    handle_call_result!(self, cached_frame, self.load_attr_import(name_id));
                }
                Opcode::StoreAttr => {
                    let name_idx = fetch_u16!(cached_frame);
                    let name_id = StringId::from_index(name_idx);
                    // Sync IP before call - property setters may push a frame.
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.store_attr(name_id) {
                        Ok(CallResult::Push(_)) => {
                            // Normal store completed - don't push anything
                        }
                        Ok(CallResult::FramePushed) => {
                            // Property setter pushed a frame
                            reload_cache!(self, cached_frame);
                        }
                        Ok(CallResult::External(ext_id, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::ExternalCall {
                                ext_function_id: ext_id,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::Proxy(proxy_id, method, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::ProxyCall {
                                proxy_id,
                                method,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::OsCall(func, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::OsCall {
                                function: func,
                                args,
                                call_id,
                            });
                        }
                        Err(err) => catch_sync!(self, cached_frame, err),
                    }
                }
                Opcode::DeleteAttr => {
                    let name_idx = fetch_u16!(cached_frame);
                    let name_id = StringId::from_index(name_idx);
                    // Sync IP before call - property deleters may push a frame.
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.delete_attr(name_id) {
                        Ok(CallResult::Push(_)) => {
                            // Normal delete completed - don't push anything
                        }
                        Ok(CallResult::FramePushed) => {
                            // Property deleter pushed a frame
                            reload_cache!(self, cached_frame);
                        }
                        Ok(CallResult::External(ext_id, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::ExternalCall {
                                ext_function_id: ext_id,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::Proxy(proxy_id, method, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::ProxyCall {
                                proxy_id,
                                method,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::OsCall(func, args)) => {
                            let call_id = self.allocate_call_id();
                            return Ok(FrameExit::OsCall {
                                function: func,
                                args,
                                call_id,
                            });
                        }
                        Err(err) => catch_sync!(self, cached_frame, err),
                    }
                }
                // Control Flow - use cached_frame.ip directly for jumps
                Opcode::Jump => {
                    let offset = fetch_i16!(cached_frame);
                    jump_relative!(cached_frame.ip, offset);
                }
                Opcode::JumpIfTrue => {
                    let offset = fetch_i16!(cached_frame);
                    // Check for instance with __bool__ dunder
                    let cond = self.pop();
                    if let Value::Ref(id) = &cond
                        && matches!(self.heap.get(*id), HeapData::Instance(_))
                    {
                        let id = *id;
                        let dunder_id: StringId = StaticStrings::DunderBool.into();
                        if let Some(method) = self.lookup_type_dunder(id, dunder_id) {
                            // Back up IP and call __bool__. When it returns, the bool
                            // result will be on stack, and the jump re-executes.
                            self.current_frame_mut().ip = self.instruction_ip;
                            cached_frame.ip = self.instruction_ip;
                            match self.call_dunder(id, method, ArgValues::Empty) {
                                Ok(CallResult::FramePushed) => {
                                    cond.drop_with_heap(self.heap);
                                    reload_cache!(self, cached_frame);
                                }
                                Ok(CallResult::Push(bool_val)) => {
                                    let b = bool_val.py_bool(self.heap, self.interns);
                                    bool_val.drop_with_heap(self.heap);
                                    cond.drop_with_heap(self.heap);
                                    // Reset IP past the opcode+operand for this jump
                                    cached_frame.ip = self.instruction_ip + 3; // 1 opcode + 2 offset bytes
                                    if b {
                                        jump_relative!(cached_frame.ip, offset);
                                    }
                                }
                                Err(e) => {
                                    cond.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    cond.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                        // Try __len__ as fallback
                        let len_id: StringId = StaticStrings::DunderLen.into();
                        if let Some(method) = self.lookup_type_dunder(id, len_id) {
                            self.current_frame_mut().ip = self.instruction_ip;
                            cached_frame.ip = self.instruction_ip;
                            match self.call_dunder(id, method, ArgValues::Empty) {
                                Ok(CallResult::FramePushed) => {
                                    cond.drop_with_heap(self.heap);
                                    reload_cache!(self, cached_frame);
                                }
                                Ok(CallResult::Push(len_val)) => {
                                    let b = match &len_val {
                                        Value::Int(i) => *i != 0,
                                        _ => true,
                                    };
                                    len_val.drop_with_heap(self.heap);
                                    cond.drop_with_heap(self.heap);
                                    cached_frame.ip = self.instruction_ip + 3;
                                    if b {
                                        jump_relative!(cached_frame.ip, offset);
                                    }
                                }
                                Err(e) => {
                                    cond.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    cond.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                    }
                    if cond.py_bool(self.heap, self.interns) {
                        jump_relative!(cached_frame.ip, offset);
                    }
                    cond.drop_with_heap(self.heap);
                }
                Opcode::JumpIfFalse => {
                    let offset = fetch_i16!(cached_frame);
                    // Check for instance with __bool__ dunder
                    let cond = self.pop();
                    if let Value::Ref(id) = &cond
                        && matches!(self.heap.get(*id), HeapData::Instance(_))
                    {
                        let id = *id;
                        let dunder_id: StringId = StaticStrings::DunderBool.into();
                        if let Some(method) = self.lookup_type_dunder(id, dunder_id) {
                            self.current_frame_mut().ip = self.instruction_ip;
                            cached_frame.ip = self.instruction_ip;
                            match self.call_dunder(id, method, ArgValues::Empty) {
                                Ok(CallResult::FramePushed) => {
                                    cond.drop_with_heap(self.heap);
                                    reload_cache!(self, cached_frame);
                                }
                                Ok(CallResult::Push(bool_val)) => {
                                    let b = bool_val.py_bool(self.heap, self.interns);
                                    bool_val.drop_with_heap(self.heap);
                                    cond.drop_with_heap(self.heap);
                                    cached_frame.ip = self.instruction_ip + 3;
                                    if !b {
                                        jump_relative!(cached_frame.ip, offset);
                                    }
                                }
                                Err(e) => {
                                    cond.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    cond.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                        let len_id: StringId = StaticStrings::DunderLen.into();
                        if let Some(method) = self.lookup_type_dunder(id, len_id) {
                            self.current_frame_mut().ip = self.instruction_ip;
                            cached_frame.ip = self.instruction_ip;
                            match self.call_dunder(id, method, ArgValues::Empty) {
                                Ok(CallResult::FramePushed) => {
                                    cond.drop_with_heap(self.heap);
                                    reload_cache!(self, cached_frame);
                                }
                                Ok(CallResult::Push(len_val)) => {
                                    let b = match &len_val {
                                        Value::Int(i) => *i != 0,
                                        _ => true,
                                    };
                                    len_val.drop_with_heap(self.heap);
                                    cond.drop_with_heap(self.heap);
                                    cached_frame.ip = self.instruction_ip + 3;
                                    if !b {
                                        jump_relative!(cached_frame.ip, offset);
                                    }
                                }
                                Err(e) => {
                                    cond.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    cond.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                    }
                    if !cond.py_bool(self.heap, self.interns) {
                        jump_relative!(cached_frame.ip, offset);
                    }
                    cond.drop_with_heap(self.heap);
                }
                Opcode::JumpIfTrueOrPop => {
                    let offset = fetch_i16!(cached_frame);
                    // For instances with __bool__, dispatch dunder then re-execute
                    if let Value::Ref(id) = self.peek() {
                        let id = *id;
                        if matches!(self.heap.get(id), HeapData::Instance(_)) {
                            let dunder_id: StringId = StaticStrings::DunderBool.into();
                            if let Some(method) = self.lookup_type_dunder(id, dunder_id) {
                                let cond = self.pop();
                                self.current_frame_mut().ip = self.instruction_ip;
                                cached_frame.ip = self.instruction_ip;
                                match self.call_dunder(id, method, ArgValues::Empty) {
                                    Ok(CallResult::FramePushed) => {
                                        cond.drop_with_heap(self.heap);
                                        reload_cache!(self, cached_frame);
                                    }
                                    Ok(CallResult::Push(bool_val)) => {
                                        let b = bool_val.py_bool(self.heap, self.interns);
                                        bool_val.drop_with_heap(self.heap);
                                        cond.drop_with_heap(self.heap);
                                        // Push a Bool to replace the instance for re-execution
                                        self.push(Value::Bool(b));
                                    }
                                    Err(e) => {
                                        cond.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                    _ => {
                                        cond.drop_with_heap(self.heap);
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    if self.peek().py_bool(self.heap, self.interns) {
                        jump_relative!(cached_frame.ip, offset);
                    } else {
                        let value = self.pop();
                        value.drop_with_heap(self.heap);
                    }
                }
                Opcode::JumpIfFalseOrPop => {
                    let offset = fetch_i16!(cached_frame);
                    // For instances with __bool__, dispatch dunder then re-execute
                    if let Value::Ref(id) = self.peek() {
                        let id = *id;
                        if matches!(self.heap.get(id), HeapData::Instance(_)) {
                            let dunder_id: StringId = StaticStrings::DunderBool.into();
                            if let Some(method) = self.lookup_type_dunder(id, dunder_id) {
                                let cond = self.pop();
                                self.current_frame_mut().ip = self.instruction_ip;
                                cached_frame.ip = self.instruction_ip;
                                match self.call_dunder(id, method, ArgValues::Empty) {
                                    Ok(CallResult::FramePushed) => {
                                        cond.drop_with_heap(self.heap);
                                        reload_cache!(self, cached_frame);
                                    }
                                    Ok(CallResult::Push(bool_val)) => {
                                        let b = bool_val.py_bool(self.heap, self.interns);
                                        bool_val.drop_with_heap(self.heap);
                                        cond.drop_with_heap(self.heap);
                                        self.push(Value::Bool(b));
                                    }
                                    Err(e) => {
                                        cond.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                    _ => {
                                        cond.drop_with_heap(self.heap);
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    if self.peek().py_bool(self.heap, self.interns) {
                        let value = self.pop();
                        value.drop_with_heap(self.heap);
                    } else {
                        jump_relative!(cached_frame.ip, offset);
                    }
                }
                // Iteration - route through exception handling
                Opcode::GetIter => {
                    let value = self.pop();
                    // Check if value is already an iterator object - iterators are their own
                    // iterables and must return themselves from iter().
                    if let Value::Ref(id) = &value
                        && matches!(self.heap.get(*id), HeapData::Generator(_) | HeapData::Iter(_))
                    {
                        // Iterator implements __iter__ returning self, just push it back.
                        self.push(value);
                        continue;
                    }
                    // Check if value is an instance with __iter__
                    if let Value::Ref(id) = &value
                        && matches!(self.heap.get(*id), HeapData::Instance(_))
                    {
                        let id = *id;
                        let dunder_id: StringId = StaticStrings::DunderIter.into();
                        if let Some(method) = self.lookup_type_dunder(id, dunder_id) {
                            // Call __iter__() - result replaces value on stack
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(id, method, ArgValues::Empty) {
                                Ok(CallResult::Push(iter_val)) => {
                                    value.drop_with_heap(self.heap);
                                    match self.normalize_iter_result(iter_val) {
                                        Ok(iter_obj) => self.push(iter_obj),
                                        Err(e) => catch_sync!(self, cached_frame, e),
                                    }
                                }
                                Ok(CallResult::FramePushed) => {
                                    value.drop_with_heap(self.heap);
                                    // __iter__ result will be pushed when frame returns
                                    reload_cache!(self, cached_frame);
                                }
                                Err(e) => {
                                    value.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    value.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                    }
                    // Check if value is a class object with metaclass __iter__
                    if let Value::Ref(id) = &value
                        && matches!(self.heap.get(*id), HeapData::ClassObject(_))
                    {
                        let id = *id;
                        let dunder_id: StringId = StaticStrings::DunderIter.into();
                        if let Some(method) = self.lookup_metaclass_dunder(id, dunder_id) {
                            // Call metaclass __iter__() - result replaces value on stack
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_class_dunder(id, method, ArgValues::Empty) {
                                Ok(CallResult::Push(iter_val)) => {
                                    value.drop_with_heap(self.heap);
                                    match self.normalize_iter_result(iter_val) {
                                        Ok(iter_obj) => self.push(iter_obj),
                                        Err(e) => catch_sync!(self, cached_frame, e),
                                    }
                                }
                                Ok(CallResult::FramePushed) => {
                                    value.drop_with_heap(self.heap);
                                    // __iter__ result will be pushed when frame returns
                                    reload_cache!(self, cached_frame);
                                }
                                Err(e) => {
                                    value.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, e);
                                }
                                _ => {
                                    value.drop_with_heap(self.heap);
                                }
                            }
                            continue;
                        }
                    }
                    // Create a OurosIter from the value and store on heap
                    match OurosIter::new(value, self.heap, self.interns) {
                        Ok(iter) => match self.heap.allocate(HeapData::Iter(iter)) {
                            Ok(heap_id) => self.push(Value::Ref(heap_id)),
                            Err(e) => catch_sync!(self, cached_frame, e.into()),
                        },
                        Err(e) => catch_sync!(self, cached_frame, e),
                    }
                }
                Opcode::ForIter => {
                    let offset = fetch_i16!(cached_frame);
                    // Peek at the iterator on TOS and extract heap_id
                    let Value::Ref(heap_id) = *self.peek() else {
                        return Err(RunError::internal("ForIter: expected iterator ref on stack"));
                    };

                    // Check if the iterator is a generator
                    if matches!(self.heap.get(heap_id), HeapData::Generator(_)) {
                        self.current_frame_mut().ip = cached_frame.ip;
                        match self.generator_next(heap_id) {
                            Ok(CallResult::FramePushed) => {
                                self.pending_for_iter_jump.push(offset);
                                reload_cache!(self, cached_frame);
                            }
                            Err(e) if e.is_stop_iteration() => {
                                let iter = self.pop();
                                iter.drop_with_heap(self.heap);
                                jump_relative!(cached_frame.ip, offset);
                            }
                            Err(e) => {
                                let iter = self.pop();
                                iter.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, e);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Check if the iterator is an instance with __next__
                    if matches!(self.heap.get(heap_id), HeapData::Instance(_)) {
                        let dunder_id: StringId = StaticStrings::DunderNext.into();
                        if let Some(method) = self.lookup_type_dunder(heap_id, dunder_id) {
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(heap_id, method, ArgValues::Empty) {
                                Ok(CallResult::Push(next_val)) => {
                                    self.push(next_val);
                                }
                                Ok(CallResult::FramePushed) => {
                                    // __next__ pushed a frame. Store the jump offset so that:
                                    // - On normal return: push the value (next item)
                                    // - On StopIteration: pop iterator and jump to end
                                    self.pending_for_iter_jump.push(offset);
                                    reload_cache!(self, cached_frame);
                                }
                                Err(e) => {
                                    // Check if it's StopIteration
                                    if e.is_stop_iteration() {
                                        let iter = self.pop();
                                        iter.drop_with_heap(self.heap);
                                        jump_relative!(cached_frame.ip, offset);
                                    } else {
                                        let iter = self.pop();
                                        iter.drop_with_heap(self.heap);
                                        catch_sync!(self, cached_frame, e);
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }
                    }

                    // Use advance_iterator which avoids std::mem::replace overhead
                    // by using a two-phase approach: read state, get value, update index
                    match advance_on_heap(self.heap, heap_id, self.interns) {
                        Ok(Some(value)) => self.push(value),
                        Ok(None) => {
                            // Iterator exhausted - pop it and jump to end
                            let iter = self.pop();
                            iter.drop_with_heap(self.heap);
                            jump_relative!(cached_frame.ip, offset);
                        }
                        Err(e) => {
                            // Error during iteration (e.g., dict size changed)
                            let iter = self.pop();
                            iter.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                        }
                    }
                }
                // Function Calls - sync IP before call, reload cache after frame changes
                Opcode::CallFunction => {
                    let arg_count = fetch_u8!(cached_frame) as usize;

                    // Sync IP before call (call_function may access frame for traceback)
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(self, cached_frame, self.exec_call_function(arg_count));
                }
                Opcode::CallBuiltinFunction => {
                    // Fetch operands: builtin_id (u8) + arg_count (u8)
                    let builtin_id = fetch_u8!(cached_frame);
                    let arg_count = fetch_u8!(cached_frame) as usize;

                    // Sync IP before call (may push frame for dunder dispatch)
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(
                        self,
                        cached_frame,
                        self.exec_call_builtin_function(builtin_id, arg_count)
                    );
                }
                Opcode::CallBuiltinType => {
                    // Fetch operands: type_id (u8) + arg_count (u8)
                    let type_id = fetch_u8!(cached_frame);
                    let arg_count = fetch_u8!(cached_frame) as usize;

                    // Sync IP before call (may push frame for dunder dispatch)
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(self, cached_frame, self.exec_call_builtin_type(type_id, arg_count));
                }
                Opcode::CallFunctionKw => {
                    // Fetch operands: pos_count, kw_count, then kw_count name indices
                    let pos_count = fetch_u8!(cached_frame) as usize;
                    let kw_count = fetch_u8!(cached_frame) as usize;

                    // Read keyword names with inline storage for common tiny kwargs arity.
                    let mut kwname_ids: SmallVec<[StringId; 4]> = SmallVec::with_capacity(kw_count);
                    for _ in 0..kw_count {
                        kwname_ids.push(StringId::from_index(fetch_u16!(cached_frame)));
                    }

                    // Sync IP before call (call_function may access frame for traceback)
                    self.current_frame_mut().ip = cached_frame.ip;
                    handle_call_result!(self, cached_frame, self.exec_call_function_kw(pos_count, kwname_ids));
                }
                Opcode::CallAttr => {
                    // CallAttr: u16 name_id, u8 arg_count
                    // Stack: [obj, arg1, arg2, ..., argN] -> [result]
                    let name_idx = fetch_u16!(cached_frame);
                    let arg_count = fetch_u8!(cached_frame) as usize;
                    let name_id = StringId::from_index(name_idx);

                    // Sync IP before call (may yield to host for OS/external calls)
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(self, cached_frame, self.exec_call_attr(name_id, arg_count));
                }
                Opcode::CallAttrKw => {
                    // CallAttrKw: u16 name_id, u8 pos_count, u8 kw_count, then kw_count u16 name indices
                    // Stack: [obj, pos_args..., kw_values...] -> [result]
                    let name_idx = fetch_u16!(cached_frame);
                    let pos_count = fetch_u8!(cached_frame) as usize;
                    let kw_count = fetch_u8!(cached_frame) as usize;
                    let name_id = StringId::from_index(name_idx);

                    // Read keyword names with inline storage for common tiny kwargs arity.
                    let mut kwname_ids: SmallVec<[StringId; 4]> = SmallVec::with_capacity(kw_count);
                    for _ in 0..kw_count {
                        kwname_ids.push(StringId::from_index(fetch_u16!(cached_frame)));
                    }

                    // Sync IP before call (may yield to host for OS/external calls)
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(
                        self,
                        cached_frame,
                        self.exec_call_attr_kw(name_id, pos_count, kwname_ids)
                    );
                }
                Opcode::CallFunctionExtended => {
                    let flags = fetch_u8!(cached_frame);
                    let has_kwargs = (flags & 0x01) != 0;

                    // Sync IP before call
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(self, cached_frame, self.exec_call_function_extended(has_kwargs));
                }
                Opcode::CallAttrExtended => {
                    let name_idx = fetch_u16!(cached_frame);
                    let flags = fetch_u8!(cached_frame);
                    let name_id = StringId::from_index(name_idx);
                    let has_kwargs = (flags & 0x01) != 0;

                    // Sync IP before call (may yield to host for OS/external calls)
                    self.current_frame_mut().ip = cached_frame.ip;

                    handle_call_result!(self, cached_frame, self.exec_call_attr_extended(name_id, has_kwargs));
                }
                // Function Definition
                Opcode::MakeFunction => {
                    let func_idx = fetch_u16!(cached_frame);
                    let defaults_count = fetch_u8!(cached_frame) as usize;
                    let func_id = FunctionId::from_index(func_idx);
                    self.tracer.on_make_function(0, defaults_count);
                    if defaults_count == 0 {
                        self.push(Value::DefFunction(func_id));
                    } else {
                        // Pop default values from stack (drain maintains order: first pushed = first in vec)
                        let defaults = self.pop_n(defaults_count);
                        let id = self.heap.allocate(HeapData::FunctionDefaults(func_id, defaults))?;
                        self.push(Value::Ref(id));
                    }
                }
                Opcode::MakeClosure => {
                    let func_idx = fetch_u16!(cached_frame);
                    let defaults_count = fetch_u8!(cached_frame) as usize;
                    let cell_count = fetch_u8!(cached_frame) as usize;
                    let func_id = FunctionId::from_index(func_idx);
                    self.tracer.on_make_function(cell_count, defaults_count);

                    // Pop cells from stack (pushed after defaults, so on top)
                    // Cells are Value::Ref pointing to HeapData::Cell
                    // We use individual pops which reverses order, so we need to reverse back
                    let mut cells = Vec::with_capacity(cell_count);
                    for _ in 0..cell_count {
                        // mut needed for dec_ref_forget when ref-count-panic feature is enabled
                        #[cfg_attr(not(feature = "ref-count-panic"), expect(unused_mut))]
                        let mut cell_val = self.pop();
                        match &cell_val {
                            Value::Ref(heap_id) => {
                                // Keep the reference - the Closure will own the HeapId
                                cells.push(*heap_id);
                                // Mark the Value as dereferenced since Closure takes ownership
                                // of the reference count (we don't call drop_with_heap because
                                // we're not decrementing the refcount, just transferring it)
                                #[cfg(feature = "ref-count-panic")]
                                cell_val.dec_ref_forget();
                            }
                            _ => {
                                return Err(RunError::internal("MakeClosure: expected cell reference on stack"));
                            }
                        }
                    }
                    // Reverse to get original order (individual pops reverse the order)
                    cells.reverse();

                    // Pop default values from stack (drain maintains order: first pushed = first in vec)
                    let defaults = self.pop_n(defaults_count);

                    // Create Closure on heap and push reference
                    let heap_id = self.heap.allocate(HeapData::Closure(func_id, cells, defaults))?;
                    self.push(Value::Ref(heap_id));
                }
                // Class Definition
                Opcode::BuildClass => {
                    let func_idx = fetch_u16!(cached_frame);
                    let name_idx = fetch_u16!(cached_frame);
                    let base_count = fetch_u8!(cached_frame) as usize;
                    let func_id = FunctionId::from_index(func_idx);
                    let name_id = StringId::from_index(name_idx);

                    // Pop base class expressions (pushed in order by compiler)
                    let base_values = self.pop_n(base_count);

                    // Pop class kwargs dict (always pushed by compiler)
                    let class_kwargs = self.pop();
                    let class_kwargs = match class_kwargs {
                        value @ Value::Ref(id) => {
                            if matches!(self.heap.get(id), HeapData::Dict(_)) {
                                value
                            } else {
                                value.drop_with_heap(self.heap);
                                for v in base_values {
                                    v.drop_with_heap(self.heap);
                                }
                                return Err(ExcType::type_error("class kwargs must be a dict".to_string()));
                            }
                        }
                        other => {
                            other.drop_with_heap(self.heap);
                            for v in base_values {
                                v.drop_with_heap(self.heap);
                            }
                            return Err(ExcType::type_error("class kwargs must be a dict".to_string()));
                        }
                    };

                    // Sync IP before any calls (we may push frames)
                    self.current_frame_mut().ip = cached_frame.ip;
                    let call_position = self.current_position();

                    // Begin class construction (may push a frame for __mro_entries__ or __prepare__)
                    let frame_pushed = self.run_class_build(
                        name_id,
                        func_id,
                        base_values,
                        Vec::new(),
                        class_kwargs,
                        call_position,
                        None,
                    )?;

                    if frame_pushed {
                        reload_cache!(self, cached_frame);
                    }
                }
                // Exception Handling
                Opcode::Raise => {
                    let exc = self.pop();
                    let error = self.make_exception(exc, true, true); // is_raise=true, hide caret
                    catch_sync!(self, cached_frame, error);
                }
                Opcode::RaiseFrom => {
                    let cause = self.pop();
                    let exc = self.pop();
                    let error = self.make_exception_with_cause(exc, cause, true);
                    catch_sync!(self, cached_frame, error);
                }
                Opcode::Reraise => {
                    // Pop the current exception from the stack to re-raise it
                    // If caught, handle_exception will push it back
                    let error = if let Some(exc) = self.pop_exception_context() {
                        self.make_exception(exc, true, false) // is_raise=true for reraise
                    } else {
                        // No active exception - create a RuntimeError
                        SimpleException::new_msg(ExcType::RuntimeError, "No active exception to reraise").into()
                    };
                    catch_sync!(self, cached_frame, error);
                }
                Opcode::ClearException => {
                    // Pop the current exception from the stack
                    // This restores the previous exception context (if any)
                    if let Some(exc) = self.pop_exception_context() {
                        exc.drop_with_heap(self.heap);
                    }
                }
                Opcode::WithExceptSetup => {
                    // Stack: [..., exception] -> [..., exc_type, exc_val, None]
                    // Used in with-statement exception handler to set up __exit__ args
                    let exc_val = self.pop();
                    // Extract the ExcType from the exception value
                    if let Value::Ref(exc_id) = &exc_val {
                        if let HeapData::Exception(exc) = self.heap.get(*exc_id) {
                            let exc_type = exc.exc_type();
                            // Push exc_type as a builtin value
                            self.push(Value::Builtin(crate::builtins::Builtins::ExcType(exc_type)));
                            // Push the exception value itself (as exc_val)
                            self.push(exc_val);
                            // Push None for traceback
                            self.push(Value::None);
                        } else {
                            // Not an exception - shouldn't happen, but handle gracefully
                            exc_val.drop_with_heap(self.heap);
                            self.push(Value::None);
                            self.push(Value::None);
                            self.push(Value::None);
                        }
                    } else {
                        // Not a ref - shouldn't happen
                        exc_val.drop_with_heap(self.heap);
                        self.push(Value::None);
                        self.push(Value::None);
                        self.push(Value::None);
                    }
                }
                Opcode::CheckExcMatch => {
                    // Stack: [exception, exc_type] -> [exception, bool]
                    let exc_type = self.pop();
                    let exception = self.peek();
                    let result = self.check_exc_match(exception, &exc_type);
                    exc_type.drop_with_heap(self.heap);
                    let result = result?;
                    self.push(Value::Bool(result));
                }
                // Return - reload cache after popping frame
                Opcode::ReturnValue => {
                    let value = self.pop();
                    if self.frames.len() == 1 {
                        // Last frame - check if this is main task or spawned task
                        let is_main_task = self.is_main_task();

                        if is_main_task {
                            // Module-level return - we're done
                            return Ok(FrameExit::Return(value));
                        }

                        // Spawned task completed - handle task completion
                        let result = self.handle_task_completion(value);
                        match result {
                            Ok(AwaitResult::ValueReady(v)) => {
                                self.push(v);
                            }
                            Ok(AwaitResult::FramePushed) => {
                                // Switched to another task - reload cache
                                reload_cache!(self, cached_frame);
                            }
                            Ok(AwaitResult::Yield(pending)) => {
                                // All tasks blocked - return to host
                                return Ok(FrameExit::ResolveFutures(pending));
                            }
                            Err(e) => {
                                catch_sync!(self, cached_frame, e);
                            }
                        }
                        continue;
                    }
                    // Check if this is a generator frame
                    if let Some(generator_id) = self.current_frame().generator_id {
                        let closing_generator = self.pending_generator_close == Some(generator_id);
                        if closing_generator {
                            self.pending_generator_close = None;
                        }

                        // Return from a delegated `yield from` sub-generator call.
                        // The caller generator keeps the delegated iterator on its stack.
                        // When the sub-generator returns, we consume that iterator and:
                        // - in normal mode: push the sub-generator return value as the
                        //   value of the `yield from` expression.
                        // - in close mode: continue outer close() by injecting GeneratorExit.
                        let delegated_return = self.pending_yield_from.last().copied().filter(|pending| {
                            self.frames.len() >= 2
                                && self.frames[self.frames.len() - 2].generator_id == Some(pending.outer_generator_id)
                        });
                        if let Some(pending) = delegated_return {
                            self.pending_yield_from.pop();
                            self.finish_generator_frame(generator_id);
                            let iter = self.pop();
                            iter.drop_with_heap(self.heap);

                            if pending.mode == YieldFromMode::Close {
                                value.drop_with_heap(self.heap);
                                self.pending_generator_close = Some(pending.outer_generator_id);
                                catch_sync!(
                                    self,
                                    cached_frame,
                                    SimpleException::new_none(ExcType::GeneratorExit).into()
                                );
                                continue;
                            }

                            self.push(value);
                            reload_cache!(self, cached_frame);
                            continue;
                        }

                        // Generator return: close() suppresses StopIteration, normal completion raises it.
                        let stop_iter = if closing_generator {
                            value.drop_with_heap(self.heap);
                            None
                        } else {
                            Some(self.stop_iteration_for_generator_return(value))
                        };
                        self.finish_generator_frame(generator_id);

                        if closing_generator {
                            if let Some(default_value) = self.take_pending_next_default_for(generator_id) {
                                default_value.drop_with_heap(self.heap);
                            }
                            self.push(Value::None);
                            reload_cache!(self, cached_frame);
                            continue;
                        }

                        // ForIter continuation for generator.__next__(): iterator is still on the
                        // caller stack. Pop it and jump to the end of the loop body.
                        if let Some(offset) = self.pending_for_iter_jump.pop() {
                            let iter = self.pop();
                            iter.drop_with_heap(self.heap);
                            let frame = self.current_frame_mut();
                            jump_relative!(frame.ip, offset);
                            reload_cache!(self, cached_frame);
                            continue;
                        }

                        // list(generator): StopIteration finalizes list construction.
                        if self.pending_list_build_return {
                            let list_build_result = self.handle_list_build_stop_iteration();
                            if self.pending_unpack.is_some() {
                                match list_build_result {
                                    Ok(CallResult::Push(list_value)) => {
                                        if let Err(err) = self.resume_pending_unpack(list_value) {
                                            catch_sync!(self, cached_frame, err);
                                            continue;
                                        }
                                        reload_cache!(self, cached_frame);
                                        continue;
                                    }
                                    Ok(other) => {
                                        catch_sync!(
                                            self,
                                            cached_frame,
                                            RunError::internal(format!(
                                                "pending unpack expected list-build push, got {other:?}"
                                            ))
                                        );
                                        continue;
                                    }
                                    Err(err) => {
                                        catch_sync!(self, cached_frame, err);
                                        continue;
                                    }
                                }
                            }
                            let result = self.maybe_finish_sum_from_list_result(list_build_result);
                            let result = self.maybe_finish_builtin_from_list_result(result);
                            handle_call_result!(self, cached_frame, result);
                            reload_cache!(self, cached_frame);
                            continue;
                        }

                        if self
                            .pending_next_default
                            .as_ref()
                            .is_some_and(|pending| pending.generator_id == generator_id)
                        {
                            let default_value = self
                                .take_pending_next_default_for(generator_id)
                                .expect("pending next default should exist");
                            let _ = stop_iter.as_ref().expect("closing generators returned above");
                            self.push(default_value);
                            reload_cache!(self, cached_frame);
                            continue;
                        }

                        let stop_iter = stop_iter.expect("closing generators returned above");

                        // There is no in-VM caller frame when generator methods are called from host.
                        // In that case propagate StopIteration directly instead of entering handler search.
                        let Some(frame) = self.frames.last() else {
                            return Err(stop_iter);
                        };
                        // Exception handlers live in the caller frame. Use the caller's bytecode IP
                        // before handler lookup so try/except around next(...) can catch StopIteration.
                        self.instruction_ip = frame.ip;
                        if let Some(result) = self.handle_exception(stop_iter) {
                            return Err(result);
                        }
                        // Exception was caught - reload cache and continue
                        reload_cache!(self, cached_frame);
                        continue;
                    }
                    // Check if this is a class body frame
                    if self.current_frame().class_body_info.is_some() {
                        // Class body finished - extract namespace into a ClassObject.
                        // Drop the return value (always None for class bodies).
                        value.drop_with_heap(self.heap);

                        match self.finalize_class_body() {
                            Ok(FinalizeClassResult::Done(class_value)) => {
                                self.push(class_value);
                                // Process any pending __set_name__ calls collected during finalization
                                match self.process_next_set_name_call() {
                                    Ok(true) => {
                                        // A __set_name__ frame was pushed - continue the main loop
                                        // to execute it. When it returns, the pending_set_name handler
                                        // below will process the next one.
                                        reload_cache!(self, cached_frame);
                                    }
                                    Ok(false) => {
                                        // All __set_name__ calls done - now process __init_subclass__
                                        match self.process_next_init_subclass_call() {
                                            Ok(_) => reload_cache!(self, cached_frame),
                                            Err(e) => catch_sync!(self, cached_frame, e),
                                        }
                                    }
                                    Err(e) => catch_sync!(self, cached_frame, e),
                                }
                            }
                            Ok(FinalizeClassResult::FramePushed) => {
                                reload_cache!(self, cached_frame);
                            }
                            Err(e) => {
                                self.instruction_ip = self.current_frame().ip;
                                catch_sync!(self, cached_frame, e);
                            }
                        }
                    } else if self.pending_class_build.is_some() {
                        // Returning from __mro_entries__ or __prepare__ during class construction.
                        let pending = self.pending_class_build.take().expect("checked above");
                        self.pop_frame();
                        match self.resume_class_build(pending, value) {
                            Ok(true) => reload_cache!(self, cached_frame),
                            Ok(false) => reload_cache!(self, cached_frame),
                            Err(e) => {
                                self.instruction_ip = self.current_frame().ip;
                                catch_sync!(self, cached_frame, e);
                            }
                        }
                    } else if self
                        .pending_class_finalize
                        .as_ref()
                        .is_some_and(|pending| pending.pending_frame_depth() == self.frames.len())
                        && self.pending_new_call.is_none()
                        && self.current_frame().init_instance.is_none()
                    {
                        // Returning from prepared-namespace items() during class finalization.
                        let pending = self
                            .pending_class_finalize
                            .take()
                            .expect("checked pending class finalize");
                        self.pop_frame();
                        match self.resume_class_finalize(pending, value) {
                            Ok(class_value) => {
                                self.push(class_value);
                                match self.process_next_set_name_call() {
                                    Ok(true) => {
                                        reload_cache!(self, cached_frame);
                                    }
                                    Ok(false) => match self.process_next_init_subclass_call() {
                                        Ok(_) => reload_cache!(self, cached_frame),
                                        Err(e) => catch_sync!(self, cached_frame, e),
                                    },
                                    Err(e) => catch_sync!(self, cached_frame, e),
                                }
                            }
                            Err(e) => catch_sync!(self, cached_frame, e),
                        }
                    } else if self.pending_new_call.is_some() {
                        // __new__ call returned - handle the result (maybe call __init__)
                        let pending = self.pending_new_call.take().expect("checked above");
                        let pending_class_heap_id = pending.class_heap_id;
                        self.pop_frame();
                        match self.handle_new_result(value, pending.class_heap_id, pending.init_func, pending.args) {
                            Ok(call::CallResult::Push(v)) => {
                                let should_resume_metaclass_finalize =
                                    self.pending_class_finalize.as_ref().is_some_and(|pending_finalize| {
                                        matches!(
                                            pending_finalize,
                                            PendingClassFinalize::MetaclassCall { metaclass_id, .. }
                                                if *metaclass_id == pending_class_heap_id
                                        )
                                    });
                                if should_resume_metaclass_finalize {
                                    let pending_finalize = self
                                        .pending_class_finalize
                                        .take()
                                        .expect("pending class finalize should exist");
                                    match self.resume_class_finalize(pending_finalize, v) {
                                        Ok(class_value) => {
                                            self.push(class_value);
                                            match self.process_next_set_name_call() {
                                                Ok(true) => {
                                                    reload_cache!(self, cached_frame);
                                                }
                                                Ok(false) => match self.process_next_init_subclass_call() {
                                                    Ok(_) => reload_cache!(self, cached_frame),
                                                    Err(e) => catch_sync!(self, cached_frame, e),
                                                },
                                                Err(e) => catch_sync!(self, cached_frame, e),
                                            }
                                        }
                                        Err(e) => catch_sync!(self, cached_frame, e),
                                    }
                                } else {
                                    self.push(v);
                                    reload_cache!(self, cached_frame);
                                }
                            }
                            Ok(call::CallResult::FramePushed) => {
                                // __init__ was called, frame pushed.
                                // The init_instance field was already set by handle_new_result.
                                reload_cache!(self, cached_frame);
                            }
                            Err(e) => catch_sync!(self, cached_frame, e),
                            _ => {
                                reload_cache!(self, cached_frame);
                            }
                        }
                    } else if self.current_frame().init_instance.is_some() {
                        // __init__ call returning - drop None return value, push the instance.
                        value.drop_with_heap(self.heap);
                        let frame_depth = self.frames.len();
                        self.drop_pending_getattr_for_frame(frame_depth);
                        self.drop_pending_binary_dunder_for_frame(frame_depth);
                        let frame = self.frames.pop().expect("no frame to pop");
                        // Clean up frame's stack region
                        while self.stack.len() > frame.stack_base {
                            let v = self.stack.pop().unwrap();
                            v.drop_with_heap(self.heap);
                        }
                        // Clean up the namespace
                        if frame.namespace_idx != GLOBAL_NS_IDX {
                            self.namespaces.drop_with_heap(frame.namespace_idx, self.heap);
                        }
                        // Push the instance that was stashed in the frame
                        let instance = frame.init_instance.expect("checked above");
                        let should_resume_metaclass_finalize =
                            self.pending_class_finalize.as_ref().is_some_and(|pending_finalize| {
                                let PendingClassFinalize::MetaclassCall { metaclass_id, .. } = pending_finalize else {
                                    return false;
                                };
                                let Value::Ref(class_id) = &instance else {
                                    return false;
                                };
                                match self.heap.get(*class_id) {
                                    HeapData::ClassObject(cls) => {
                                        matches!(cls.metaclass(), Value::Ref(id) if *id == *metaclass_id)
                                    }
                                    _ => false,
                                }
                            });
                        if should_resume_metaclass_finalize {
                            let pending_finalize = self
                                .pending_class_finalize
                                .take()
                                .expect("pending class finalize should exist");
                            match self.resume_class_finalize(pending_finalize, instance) {
                                Ok(class_value) => {
                                    self.push(class_value);
                                    match self.process_next_set_name_call() {
                                        Ok(true) => {
                                            reload_cache!(self, cached_frame);
                                        }
                                        Ok(false) => match self.process_next_init_subclass_call() {
                                            Ok(_) => reload_cache!(self, cached_frame),
                                            Err(e) => catch_sync!(self, cached_frame, e),
                                        },
                                        Err(e) => catch_sync!(self, cached_frame, e),
                                    }
                                }
                                Err(e) => catch_sync!(self, cached_frame, e),
                            }
                        } else {
                            self.push(instance);
                            reload_cache!(self, cached_frame);
                        }
                    } else if self.pending_set_name_return {
                        // __set_name__ call returned - discard the return value,
                        // pop the frame, and process the next pending call (if any).
                        self.pending_set_name_return = false;
                        value.drop_with_heap(self.heap);
                        self.pop_frame();
                        match self.process_next_set_name_call() {
                            Ok(true) => {
                                // Another __set_name__ frame pushed
                                reload_cache!(self, cached_frame);
                            }
                            Ok(false) => {
                                // All __set_name__ done - chain into __init_subclass__
                                match self.process_next_init_subclass_call() {
                                    Ok(_) => reload_cache!(self, cached_frame),
                                    Err(e) => catch_sync!(self, cached_frame, e),
                                }
                            }
                            Err(e) => catch_sync!(self, cached_frame, e),
                        }
                    } else if self.pending_init_subclass_return {
                        // __init_subclass__ call returned - discard, pop, process next
                        self.pending_init_subclass_return = false;
                        value.drop_with_heap(self.heap);
                        self.pop_frame();
                        match self.process_next_init_subclass_call() {
                            Ok(_) => reload_cache!(self, cached_frame),
                            Err(e) => catch_sync!(self, cached_frame, e),
                        }
                    } else if self.pending_discard_return {
                        // Dunder like __setitem__/__delitem__ returned - discard the value
                        self.pending_discard_return = false;
                        value.drop_with_heap(self.heap);
                        self.pop_frame();
                        reload_cache!(self, cached_frame);
                    } else if self.pending_defaultdict_return {
                        // default_factory() returned for defaultdict missing-key access.
                        self.pending_defaultdict_return = false;
                        self.pop_frame();
                        let result = self.handle_defaultdict_missing_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_instancecheck_return {
                        // __instancecheck__ returned - coerce to bool
                        self.pending_instancecheck_return = false;
                        let b = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        self.pop_frame();
                        self.push(Value::Bool(b));
                        reload_cache!(self, cached_frame);
                    } else if self.pending_subclasscheck_return {
                        // __subclasscheck__ returned - coerce to bool
                        self.pending_subclasscheck_return = false;
                        let b = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        self.pop_frame();
                        self.push(Value::Bool(b));
                        reload_cache!(self, cached_frame);
                    } else if self.pending_dir_return {
                        // __dir__ returned for dir(obj) - normalize and sort.
                        self.pending_dir_return = false;
                        self.pop_frame();
                        match self.normalize_dir_result(value) {
                            Ok(sorted) => {
                                self.push(sorted);
                                reload_cache!(self, cached_frame);
                            }
                            Err(e) => catch_sync!(self, cached_frame, e),
                        }
                    } else if self.pending_negate_bool {
                        // __contains__ or __bool__ returned for 'not in' - negate the result
                        self.pending_negate_bool = false;
                        let b = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        self.pop_frame();
                        self.push(Value::Bool(!b));
                        reload_cache!(self, cached_frame);
                    } else if !self.pending_for_iter_jump.is_empty() {
                        // __next__ dunder returned normally - push the value (next item)
                        self.pending_for_iter_jump.pop();
                        self.pop_frame();
                        self.push(value);
                        reload_cache!(self, cached_frame);
                    } else if let Some(k) = self.pending_mod_eq_k.take() {
                        // __mod__/__rmod__ dunder returned for CompareModEq.
                        // Compare the mod result with k and push bool.
                        self.pop_frame();
                        let is_equal = value.py_eq(&k, self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        k.drop_with_heap(self.heap);
                        self.push(Value::Bool(is_equal));
                        reload_cache!(self, cached_frame);
                    } else if let Some(offset) = self.pending_compare_eq_jump.take() {
                        // __eq__ dunder returned for CompareEqJumpIfFalse.
                        // Consume the result and jump if the comparison is falsy.
                        self.pop_frame();
                        let is_equal = value.py_bool(self.heap, self.interns);
                        value.drop_with_heap(self.heap);
                        if !is_equal {
                            let frame = self.current_frame_mut();
                            jump_relative!(frame.ip, offset);
                        }
                        reload_cache!(self, cached_frame);
                    } else if self
                        .pending_binary_dunder
                        .last()
                        .is_some_and(|pending| pending.frame_depth == self.frames.len())
                    {
                        // Binary dunder returned from a frame-pushed call.
                        // If primary returned NotImplemented, invoke reflected dunder.
                        let mut pending = self
                            .pending_binary_dunder
                            .pop()
                            .expect("pending binary dunder entry disappeared");
                        self.pop_frame();

                        if matches!(value, Value::NotImplemented) {
                            value.drop_with_heap(self.heap);
                            match pending.stage {
                                PendingBinaryDunderStage::Primary => {
                                    let reflected_result = if let Some(reflected_id) = pending.reflected_dunder_id
                                        && let Value::Ref(rhs_id) = &pending.rhs
                                        && matches!(self.heap.get(*rhs_id), HeapData::Instance(_))
                                        && let Some(method) = self.lookup_type_dunder(*rhs_id, reflected_id)
                                    {
                                        let lhs_arg = pending.lhs.clone_with_heap(self.heap);
                                        Some(self.call_dunder(*rhs_id, method, ArgValues::One(lhs_arg)))
                                    } else {
                                        None
                                    };

                                    match reflected_result {
                                        Some(Ok(CallResult::Push(reflected_value))) => {
                                            if matches!(reflected_value, Value::NotImplemented) {
                                                reflected_value.drop_with_heap(self.heap);
                                                let err = self.binary_dunder_type_error(
                                                    &pending.lhs,
                                                    &pending.rhs,
                                                    pending.primary_dunder_id,
                                                );
                                                pending.lhs.drop_with_heap(self.heap);
                                                pending.rhs.drop_with_heap(self.heap);
                                                catch_sync!(self, cached_frame, err);
                                            } else {
                                                pending.lhs.drop_with_heap(self.heap);
                                                pending.rhs.drop_with_heap(self.heap);
                                                self.push(reflected_value);
                                                reload_cache!(self, cached_frame);
                                            }
                                        }
                                        Some(Ok(CallResult::FramePushed)) => {
                                            pending.stage = PendingBinaryDunderStage::Reflected;
                                            pending.frame_depth = self.frames.len();
                                            self.pending_binary_dunder.push(pending);
                                            reload_cache!(self, cached_frame);
                                        }
                                        Some(Ok(CallResult::External(ext_id, args))) => {
                                            pending.lhs.drop_with_heap(self.heap);
                                            pending.rhs.drop_with_heap(self.heap);
                                            let call_id = self.allocate_call_id();
                                            return Ok(FrameExit::ExternalCall {
                                                ext_function_id: ext_id,
                                                args,
                                                call_id,
                                            });
                                        }
                                        Some(Ok(CallResult::Proxy(proxy_id, method, args))) => {
                                            pending.lhs.drop_with_heap(self.heap);
                                            pending.rhs.drop_with_heap(self.heap);
                                            let call_id = self.allocate_call_id();
                                            return Ok(FrameExit::ProxyCall {
                                                proxy_id,
                                                method,
                                                args,
                                                call_id,
                                            });
                                        }
                                        Some(Ok(CallResult::OsCall(function, args))) => {
                                            pending.lhs.drop_with_heap(self.heap);
                                            pending.rhs.drop_with_heap(self.heap);
                                            let call_id = self.allocate_call_id();
                                            return Ok(FrameExit::OsCall {
                                                function,
                                                args,
                                                call_id,
                                            });
                                        }
                                        Some(Err(err)) => {
                                            pending.lhs.drop_with_heap(self.heap);
                                            pending.rhs.drop_with_heap(self.heap);
                                            catch_sync!(self, cached_frame, err);
                                        }
                                        None => {
                                            let err = self.binary_dunder_type_error(
                                                &pending.lhs,
                                                &pending.rhs,
                                                pending.primary_dunder_id,
                                            );
                                            pending.lhs.drop_with_heap(self.heap);
                                            pending.rhs.drop_with_heap(self.heap);
                                            catch_sync!(self, cached_frame, err);
                                        }
                                    }
                                }
                                PendingBinaryDunderStage::Reflected => {
                                    let err = self.binary_dunder_type_error(
                                        &pending.lhs,
                                        &pending.rhs,
                                        pending.primary_dunder_id,
                                    );
                                    pending.lhs.drop_with_heap(self.heap);
                                    pending.rhs.drop_with_heap(self.heap);
                                    catch_sync!(self, cached_frame, err);
                                }
                            }
                        } else {
                            pending.lhs.drop_with_heap(self.heap);
                            pending.rhs.drop_with_heap(self.heap);
                            self.push(value);
                            reload_cache!(self, cached_frame);
                        }
                    } else if let Some(target_id) = self.pending_hash_target.take() {
                        // __hash__ dunder returned for dict operation - cache the hash result
                        // and optionally push it if this came from hash(instance).
                        #[expect(clippy::cast_sign_loss)]
                        let hash_value = match &value {
                            Value::Int(i) => *i as u64,
                            Value::Bool(b) => u64::from(*b),
                            _ => {
                                self.pending_hash_push_result = false;
                                value.drop_with_heap(self.heap);
                                self.pop_frame();
                                reload_cache!(self, cached_frame);
                                catch_sync!(
                                    self,
                                    cached_frame,
                                    ExcType::type_error("__hash__ method should return an integer")
                                );
                                continue;
                            }
                        };
                        value.drop_with_heap(self.heap);
                        self.heap.set_cached_hash(target_id, hash_value);
                        self.pop_frame();
                        if self.pending_hash_push_result {
                            self.pending_hash_push_result = false;
                            let hash_i64 = i64::from_ne_bytes(hash_value.to_ne_bytes());
                            self.push(Value::Int(hash_i64));
                        }
                        // For dict operations, DON'T push value - the calling opcode will re-execute.
                        reload_cache!(self, cached_frame);
                    } else if self
                        .pending_stringify_return
                        .last()
                        .is_some_and(|(_, frame_depth)| *frame_depth == self.frames.len())
                    {
                        // __str__/__repr__ dunder returned for str()/repr() dispatch.
                        let (kind, _) = self
                            .pending_stringify_return
                            .pop()
                            .expect("pending stringify return disappeared");
                        self.pop_frame();
                        match self.validate_stringify_result(value, kind) {
                            Ok(string_value) => {
                                self.push(string_value);
                                reload_cache!(self, cached_frame);
                            }
                            Err(err) => {
                                self.instruction_ip = self.current_frame().ip;
                                catch_sync!(self, cached_frame, err);
                            }
                        }
                    } else if self.pending_list_sort_return {
                        // User key callable returned for list.sort(key=...) - record key
                        // and continue sorting until all keys are computed.
                        self.pending_list_sort_return = false;
                        self.pop_frame();
                        let mut result = self.handle_list_sort_return(value);
                        // If this is a sorted() call (not list.sort()), return the list instead of None
                        if let Ok(CallResult::Push(returned_value)) = &result
                            && matches!(returned_value, Value::None)
                            && self.pending_sorted.is_some()
                        {
                            let list_id = self.pending_sorted.take().unwrap();
                            result = Ok(CallResult::Push(Value::Ref(list_id)));
                        }
                        if result.is_err()
                            && let Some(list_id) = self.pending_sorted.take()
                        {
                            Value::Ref(list_id).drop_with_heap(self.heap);
                        }
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_min_max_return {
                        self.pending_min_max_return = false;
                        self.pop_frame();
                        let result = self.handle_min_max_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_heapq_select_return {
                        self.pending_heapq_select_return = false;
                        self.pop_frame();
                        let result = self.handle_heapq_select_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_bisect_return {
                        self.pending_bisect_return = false;
                        self.pop_frame();
                        let result = self.handle_bisect_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_reduce_return {
                        // User function returned for functools.reduce() - use return value
                        // as new accumulator and continue processing remaining items
                        self.pending_reduce_return = false;
                        self.pop_frame();
                        let result = self.handle_reduce_return(value);
                        handle_call_result!(self, cached_frame, result);
                        // Reload cache to continue with parent frame
                        reload_cache!(self, cached_frame);
                    } else if self.pending_map_return {
                        // User function returned for map() - add return value to results
                        // and continue processing remaining items
                        self.pending_map_return = false;
                        self.pop_frame();
                        let result = self.handle_map_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_filter_return {
                        // User function returned for filter() - check truthiness and add
                        // corresponding item to results if truthy, then continue processing
                        self.pending_filter_return = false;
                        self.pop_frame();
                        let result = self.handle_filter_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_lru_cache_return {
                        self.pop_frame();
                        let Some(pending) = self.pending_lru_cache.pop() else {
                            value.drop_with_heap(self.heap);
                            catch_sync!(
                                self,
                                cached_frame,
                                RunError::internal("pending lru_cache return missing state")
                            );
                            continue;
                        };

                        self.pending_lru_cache_return = !self.pending_lru_cache.is_empty();

                        if let Err(err) = self.store_lru_cache_value(pending.cache_id, pending.cache_key, &value) {
                            value.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, err);
                            continue;
                        }
                        self.push(value);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_groupby_return {
                        // User key callable returned for itertools.groupby(..., key=...)
                        // - record key and continue evaluating remaining items.
                        self.pending_groupby_return = false;
                        self.pop_frame();
                        let result = self.handle_groupby_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_textwrap_indent_return {
                        self.pending_textwrap_indent_return = false;
                        self.pop_frame();
                        let result = self.handle_textwrap_indent_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_re_sub_return {
                        // User callback returned for re.sub() — extract replacement
                        // string and continue processing remaining matches.
                        self.pending_re_sub_return = false;
                        self.pop_frame();
                        let result = self.handle_re_sub_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_context_decorator_return {
                        self.pending_context_decorator_return = false;
                        self.pop_frame();
                        let result = self.handle_context_decorator_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_exit_stack_enter_return {
                        self.pending_exit_stack_enter_return = false;
                        self.pop_frame();
                        let result = self.handle_exit_stack_enter_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_exit_stack_return {
                        self.pending_exit_stack_return = false;
                        self.pop_frame();
                        let result = self.handle_exit_stack_return(value);
                        handle_call_result!(self, cached_frame, result);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_cached_property_return {
                        self.pending_cached_property_return = false;
                        self.pop_frame();
                        if let Some(pending) = self.pending_cached_property.take() {
                            let cache_value = value.clone_with_heap(self.heap);
                            let cache_name = pending.attr_name;
                            let cache_result = self.heap.with_entry_mut(pending.instance_id, |heap, data| {
                                let HeapData::Instance(inst) = data else {
                                    cache_value.drop_with_heap(heap);
                                    return Err(RunError::internal(
                                        "cached_property target was not an instance on return",
                                    ));
                                };

                                let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(cache_name)))?;
                                let name_value = Value::Ref(key_id);
                                if let Some(old) = inst.set_attr(name_value, cache_value, heap, self.interns)? {
                                    old.drop_with_heap(heap);
                                }
                                Ok(())
                            });
                            if let Err(err) = cache_result {
                                value.drop_with_heap(self.heap);
                                catch_sync!(self, cached_frame, err);
                                continue;
                            }
                        }
                        self.push(value);
                        reload_cache!(self, cached_frame);
                    } else if self.pending_list_build_return && self.pending_list_build_generator_id().is_none() {
                        // __next__ dunder returned for list construction - continue iteration
                        self.pop_frame();
                        let list_build_result = self.handle_list_build_return(value);
                        let result = self.maybe_finish_sum_from_list_result(list_build_result);
                        let result = self.maybe_finish_builtin_from_list_result(result);
                        handle_call_result!(self, cached_frame, result);
                    } else if self.pending_list_iter_return {
                        // __iter__ dunder returned for list construction - continue with list_build_from_iterator
                        self.pending_list_iter_return = false;
                        self.pop_frame();
                        let result = self.list_build_from_iterator(value);
                        handle_call_result!(self, cached_frame, result);
                    } else {
                        // Normal function return - pop frame and push return value
                        self.pop_frame();
                        self.push(value);
                        // Reload cache from parent frame
                        reload_cache!(self, cached_frame);
                    }
                }
                // Async/Await
                Opcode::Await => {
                    // Sync IP before exec (may push new frame for coroutine)
                    self.current_frame_mut().ip = cached_frame.ip;
                    let result = self.exec_get_awaitable();
                    match result {
                        Ok(AwaitResult::ValueReady(value)) => {
                            self.push(value);
                        }
                        Ok(AwaitResult::FramePushed) => {
                            // Reload cache after pushing a new frame
                            reload_cache!(self, cached_frame);
                        }
                        Ok(AwaitResult::Yield(pending_calls)) => {
                            // All tasks are blocked - return control to host
                            return Ok(FrameExit::ResolveFutures(pending_calls));
                        }
                        Err(e) => {
                            catch_sync!(self, cached_frame, e);
                        }
                    }
                }
                // Unpacking - route through exception handling
                Opcode::UnpackSequence => {
                    let count = fetch_u8!(cached_frame) as usize;
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.unpack_sequence(count) {
                        Ok(frame_pushed) => {
                            if frame_pushed {
                                reload_cache!(self, cached_frame);
                            }
                        }
                        Err(e) => catch_sync!(self, cached_frame, e),
                    }
                }
                Opcode::UnpackEx => {
                    let before = fetch_u8!(cached_frame) as usize;
                    let after = fetch_u8!(cached_frame) as usize;
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.unpack_ex(before, after) {
                        Ok(frame_pushed) => {
                            if frame_pushed {
                                reload_cache!(self, cached_frame);
                            }
                        }
                        Err(e) => catch_sync!(self, cached_frame, e),
                    }
                }
                // Special
                Opcode::Nop => {
                    // No operation
                }
                // Module Operations
                Opcode::LoadModule => {
                    let module_id = fetch_u8!(cached_frame);
                    try_catch_sync!(self, cached_frame, self.load_module(module_id));
                }
                Opcode::RaiseImportError => {
                    // Fetch the module name from the constant pool and raise ModuleNotFoundError
                    let const_idx = fetch_u16!(cached_frame);
                    let module_name = cached_frame.code.constants().get(const_idx);
                    // The constant should be an InternString from compile_import/compile_import_from
                    let name_str = match module_name {
                        Value::InternString(id) => self.interns.get_str(*id),
                        _ => "<unknown>",
                    };
                    let error = ExcType::module_not_found_error(name_str);
                    catch_sync!(self, cached_frame, error);
                }
                Opcode::YieldFrom => {
                    // Delegate one iteration step to a sub-iterator for `yield from`.
                    let Some(outer_generator_id) = self.current_frame().generator_id else {
                        return Err(RunError::internal("YieldFrom opcode outside generator frame"));
                    };
                    let opcode_ip = self.instruction_ip;
                    let frame_stack_base = self.current_frame().stack_base;
                    if self.stack.len() <= frame_stack_base {
                        return Err(RunError::internal("YieldFrom expected iterator on stack"));
                    }

                    let mut sent_value = None;
                    if !self.is_yield_from_iterator(self.peek()) {
                        if self.stack.len() <= frame_stack_base + 1 {
                            return Err(RunError::internal("YieldFrom expected iterator below sent value"));
                        }
                        sent_value = Some(self.pop());
                        if !self.is_yield_from_iterator(self.peek()) {
                            if let Some(value) = sent_value {
                                value.drop_with_heap(self.heap);
                            }
                            return Err(RunError::internal("YieldFrom expected iterator below sent value"));
                        }
                    }

                    let iterator = self.peek().clone_with_heap(self.heap);
                    let action = self.take_pending_generator_action(outer_generator_id);
                    let mode = if matches!(action, Some(GeneratorAction::Close)) {
                        YieldFromMode::Close
                    } else {
                        YieldFromMode::Normal
                    };
                    if action.is_some()
                        && let Some(value) = sent_value.take()
                    {
                        value.drop_with_heap(self.heap);
                    }

                    // Persist caller IP (next instruction) before delegation calls.
                    self.current_frame_mut().ip = cached_frame.ip;
                    match self.yield_from_step(iterator, sent_value, action) {
                        Ok(CallResult::Push(value)) => {
                            if mode == YieldFromMode::Close {
                                value.drop_with_heap(self.heap);
                                let iter = self.pop();
                                iter.drop_with_heap(self.heap);
                                self.pending_generator_close = Some(outer_generator_id);
                                catch_sync!(
                                    self,
                                    cached_frame,
                                    SimpleException::new_none(ExcType::GeneratorExit).into()
                                );
                                continue;
                            }

                            // Yield delegated value and suspend at this opcode so send/throw/close
                            // resumes delegation state.
                            self.current_frame_mut().ip = opcode_ip;
                            let delegated_line = self
                                .current_frame()
                                .code
                                .location_for_offset(opcode_ip)
                                .map(|entry| entry.range().start().line)
                                .unwrap_or_default();
                            self.suspend_generator_frame(outer_generator_id, value, delegated_line);

                            while let Some(pending) = self.pending_yield_from.last().copied() {
                                if self.current_frame().generator_id != Some(pending.outer_generator_id) {
                                    break;
                                }
                                self.pending_yield_from.pop();
                                let delegated_value = self.pop();
                                self.current_frame_mut().ip = pending.opcode_ip;
                                let delegated_line = self
                                    .current_frame()
                                    .code
                                    .location_for_offset(pending.opcode_ip)
                                    .map(|entry| entry.range().start().line)
                                    .unwrap_or_default();
                                self.suspend_generator_frame(
                                    pending.outer_generator_id,
                                    delegated_value,
                                    delegated_line,
                                );
                            }

                            if self.frames.is_empty() {
                                return Ok(FrameExit::Return(self.pop()));
                            }
                            if !self.pending_for_iter_jump.is_empty() {
                                self.pending_for_iter_jump.pop();
                                reload_cache!(self, cached_frame);
                                continue;
                            }
                            if self.pending_list_build_return {
                                let yielded_item = self.pop();
                                let list_build_result = self.handle_list_build_return(yielded_item);
                                let result = self.maybe_finish_sum_from_list_result(list_build_result);
                                let result = self.maybe_finish_builtin_from_list_result(result);
                                handle_call_result!(self, cached_frame, result);
                                continue;
                            }
                            reload_cache!(self, cached_frame);
                        }
                        Ok(CallResult::FramePushed) => {
                            self.pending_yield_from.push(PendingYieldFrom {
                                outer_generator_id,
                                opcode_ip,
                                mode,
                            });
                            reload_cache!(self, cached_frame);
                        }
                        Ok(CallResult::External(ext_id, args)) => {
                            let call_id = self.allocate_call_id();
                            self.current_frame_mut().ip = cached_frame.ip;
                            return Ok(FrameExit::ExternalCall {
                                ext_function_id: ext_id,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::Proxy(proxy_id, method, args)) => {
                            let call_id = self.allocate_call_id();
                            self.current_frame_mut().ip = cached_frame.ip;
                            return Ok(FrameExit::ProxyCall {
                                proxy_id,
                                method,
                                args,
                                call_id,
                            });
                        }
                        Ok(CallResult::OsCall(func, args)) => {
                            let call_id = self.allocate_call_id();
                            self.current_frame_mut().ip = cached_frame.ip;
                            return Ok(FrameExit::OsCall {
                                function: func,
                                args,
                                call_id,
                            });
                        }
                        Err(e) if e.is_stop_iteration() => {
                            if self.current_frame().generator_id != Some(outer_generator_id) {
                                self.pending_yield_from.push(PendingYieldFrom {
                                    outer_generator_id,
                                    opcode_ip,
                                    mode,
                                });
                                catch_sync!(self, cached_frame, e);
                                continue;
                            }
                            let iter = self.pop();
                            iter.drop_with_heap(self.heap);
                            if mode == YieldFromMode::Close {
                                self.pending_generator_close = Some(outer_generator_id);
                                catch_sync!(
                                    self,
                                    cached_frame,
                                    SimpleException::new_none(ExcType::GeneratorExit).into()
                                );
                                continue;
                            }
                            // Non-generator iterators don't carry a return value.
                            self.push(Value::None);
                        }
                        Err(e) => {
                            if self.current_frame().generator_id != Some(outer_generator_id) {
                                self.pending_yield_from.push(PendingYieldFrom {
                                    outer_generator_id,
                                    opcode_ip,
                                    mode,
                                });
                                catch_sync!(self, cached_frame, e);
                                continue;
                            }
                            let iter = self.pop();
                            iter.drop_with_heap(self.heap);
                            catch_sync!(self, cached_frame, e);
                        }
                    }
                }
                Opcode::Yield => {
                    // Yield opcode: pop the yielded value, save state, and suspend
                    let yielded_value = self.pop();

                    // Get the generator_id from the current frame
                    let Some(generator_id) = self.current_frame().generator_id else {
                        return Err(RunError::internal("Yield opcode outside generator frame"));
                    };

                    // Sync IP to frame before suspending
                    self.current_frame_mut().ip = cached_frame.ip;

                    // close() injected GeneratorExit into this generator but it yielded again:
                    // CPython turns that into RuntimeError and closes the generator.
                    if self.pending_generator_close == Some(generator_id) {
                        yielded_value.drop_with_heap(self.heap);
                        self.pending_generator_close = None;
                        self.finish_generator_frame(generator_id);
                        let err = ExcType::generator_ignored_exit();
                        // Route the RuntimeError through the caller frame so surrounding
                        // try/except around close() can catch it.
                        let Some(frame) = self.frames.last() else {
                            return Err(err);
                        };
                        self.instruction_ip = frame.ip;
                        if let Some(result) = self.handle_exception(err) {
                            return Err(result);
                        }
                        reload_cache!(self, cached_frame);
                        continue;
                    }

                    // Suspend the generator frame (this pops the frame, saves state, and pushes yielded value)
                    let yielded_line = self.current_position().start().line;
                    self.suspend_generator_frame(generator_id, yielded_value, yielded_line);

                    // If a generator was resumed directly (no caller frame remains), return
                    // the yielded value to the host. Otherwise keep executing in-VM caller code.
                    if self.frames.is_empty() {
                        return Ok(FrameExit::Return(self.pop()));
                    }

                    // A delegated sub-generator yielded during `yield from`.
                    // Re-yield through the outer generator while preserving the delegated iterator.
                    if let Some(pending) = self.pending_yield_from.last().copied()
                        && self.current_frame().generator_id == Some(pending.outer_generator_id)
                    {
                        self.pending_yield_from.pop();
                        let delegated_value = self.pop();
                        self.current_frame_mut().ip = pending.opcode_ip;
                        let delegated_line = self
                            .current_frame()
                            .code
                            .location_for_offset(pending.opcode_ip)
                            .map(|entry| entry.range().start().line)
                            .unwrap_or_default();
                        self.suspend_generator_frame(pending.outer_generator_id, delegated_value, delegated_line);

                        while let Some(next_pending) = self.pending_yield_from.last().copied() {
                            if self.current_frame().generator_id != Some(next_pending.outer_generator_id) {
                                break;
                            }
                            self.pending_yield_from.pop();
                            let next_value = self.pop();
                            self.current_frame_mut().ip = next_pending.opcode_ip;
                            let delegated_line = self
                                .current_frame()
                                .code
                                .location_for_offset(next_pending.opcode_ip)
                                .map(|entry| entry.range().start().line)
                                .unwrap_or_default();
                            self.suspend_generator_frame(next_pending.outer_generator_id, next_value, delegated_line);
                        }

                        if self.frames.is_empty() {
                            return Ok(FrameExit::Return(self.pop()));
                        }
                        if !self.pending_for_iter_jump.is_empty() {
                            self.pending_for_iter_jump.pop();
                            reload_cache!(self, cached_frame);
                            continue;
                        }
                        if self.pending_list_build_return {
                            let yielded_item = self.pop();
                            let list_build_result = self.handle_list_build_return(yielded_item);
                            let result = self.maybe_finish_sum_from_list_result(list_build_result);
                            let result = self.maybe_finish_builtin_from_list_result(result);
                            handle_call_result!(self, cached_frame, result);
                            continue;
                        }
                        reload_cache!(self, cached_frame);
                        continue;
                    }

                    // ForIter resumed a generator via generator_next(). Yield should behave
                    // like a normal __next__ frame return: keep the yielded item on stack,
                    // clear pending state, and continue in the caller frame.
                    if !self.pending_for_iter_jump.is_empty() {
                        self.pending_for_iter_jump.pop();
                        reload_cache!(self, cached_frame);
                        continue;
                    }

                    // list(instance_with___iter__) may iterate via __next__ calls that push
                    // frames. Mirror ReturnValue continuation handling when a generator yields.
                    if self.pending_list_build_return {
                        let yielded_item = self.pop();
                        let list_build_result = self.handle_list_build_return(yielded_item);
                        let result = self.maybe_finish_sum_from_list_result(list_build_result);
                        let result = self.maybe_finish_builtin_from_list_result(result);
                        handle_call_result!(self, cached_frame, result);
                        continue;
                    }

                    // contextmanager decorator wrappers resume after the setup
                    // generator yields once. This continuation mirrors the
                    // ReturnValue-based pending handlers but hooks Yield.
                    if self.pending_context_decorator_return
                        && self.pending_context_decorator.as_ref().is_some_and(|pending| {
                            pending.stage == PendingContextDecoratorStage::Enter
                                && matches!(pending.generator, Value::Ref(id) if id == generator_id)
                        })
                    {
                        self.pending_context_decorator_return = false;
                        let yielded_item = self.pop();
                        let result = self.handle_context_decorator_return(yielded_item);
                        handle_call_result!(self, cached_frame, result);
                        continue;
                    }

                    // ExitStack.enter_context / enter_async_context may resume from
                    // generator-backed __enter__/__aenter__ calls that yield.
                    if self.pending_exit_stack_enter_return
                        && self.pending_exit_stack_enter.as_ref().is_some_and(|pending| {
                            pending
                                .generator_id
                                .is_some_and(|pending_id| pending_id == generator_id)
                        })
                    {
                        self.pending_exit_stack_enter_return = false;
                        let yielded_item = self.pop();
                        let result = self.handle_exit_stack_enter_return(yielded_item);
                        handle_call_result!(self, cached_frame, result);
                        continue;
                    }

                    // Default in-VM continuation: yielded value is already on the caller stack.
                    reload_cache!(self, cached_frame);
                }
            }
        }
    }

    /// Continues class construction after `BuildClass` or a pending `__mro_entries__` call.
    ///
    /// Returns `true` if a new frame was pushed (caller should reload cached frame),
    /// `false` if the class build completed without pushing a frame.
    #[expect(clippy::too_many_arguments)]
    fn run_class_build(
        &mut self,
        name_id: StringId,
        func_id: FunctionId,
        mut remaining_bases: Vec<Value>,
        mut resolved_bases: Vec<Value>,
        class_kwargs: Value,
        call_position: CodeRange,
        orig_bases: Option<HeapId>,
    ) -> RunResult<bool> {
        // Ensure class_kwargs is a dict Value
        let kwargs_id = match &class_kwargs {
            Value::Ref(id) if matches!(self.heap.get(*id), HeapData::Dict(_)) => *id,
            _ => return Err(ExcType::type_error("class kwargs must be a dict".to_string())),
        };

        // Compute original bases tuple if needed (used by __mro_entries__ and __orig_bases__).
        let orig_bases_id = if let Some(id) = orig_bases {
            id
        } else {
            let mut base_values: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
            base_values.extend(remaining_bases.iter().map(|v| v.clone_with_heap(self.heap)));
            let tuple_val = crate::types::allocate_tuple(base_values, self.heap)?;
            let orig_id = match &tuple_val {
                Value::Ref(id) => *id,
                _ => return Err(RunError::internal("BuildClass: expected tuple for orig_bases")),
            };
            #[cfg(feature = "ref-count-panic")]
            {
                std::mem::forget(tuple_val);
            }
            orig_id
        };

        // Remove explicit metaclass from kwargs if present.
        let explicit_metaclass = self.heap.with_entry_mut(kwargs_id, |heap, data| {
            let HeapData::Dict(dict) = data else {
                return Err(ExcType::type_error("class kwargs must be a dict".to_string()));
            };
            let metaclass = dict.pop_by_str("metaclass", heap, self.interns);
            if let Some((key, value)) = metaclass {
                key.drop_with_heap(heap);
                Ok(Some(value))
            } else {
                Ok(None)
            }
        })?;

        // Resolve __mro_entries__ for each base in order.
        let mro_entries_id: StringId = StaticStrings::DunderMroEntries.into();
        while !remaining_bases.is_empty() {
            let base = remaining_bases.remove(0);
            // Attempt to call base.__mro_entries__(orig_bases) on a cloned base value.
            // If missing, treat as no-op (base remains as-is).
            self.heap.inc_ref(orig_bases_id);
            let orig_bases_val = Value::Ref(orig_bases_id);
            let base_clone = base.clone_with_heap(self.heap);
            match self.call_attr(base_clone, mro_entries_id, ArgValues::One(orig_bases_val)) {
                Ok(CallResult::Push(value)) => {
                    // __mro_entries__ returned synchronously; expect a tuple of bases.
                    base.drop_with_heap(self.heap);
                    let new_bases = self.unpack_mro_entries_result(value)?;
                    resolved_bases.extend(new_bases);
                }
                Ok(CallResult::FramePushed) => {
                    // __mro_entries__ pushed a frame; stash state and resume on return.
                    base.drop_with_heap(self.heap);
                    self.pending_class_build = Some(PendingClassBuild::MroEntries {
                        name_id,
                        func_id,
                        call_position,
                        remaining_bases,
                        resolved_bases,
                        orig_bases: orig_bases_id,
                        class_kwargs,
                    });
                    return Ok(true);
                }
                Ok(other) => {
                    base.drop_with_heap(self.heap);
                    return Err(RunError::internal(format!(
                        "__mro_entries__ returned unsupported call result: {other:?}"
                    )));
                }
                Err(RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::AttributeError => {
                    // No __mro_entries__ defined - keep base as-is.
                    resolved_bases.push(base);
                }
                Err(e) => {
                    base.drop_with_heap(self.heap);
                    return Err(e);
                }
            }
        }

        // Select the metaclass.
        let metaclass = self.select_metaclass(&resolved_bases, explicit_metaclass)?;

        // Call metaclass.__prepare__ if defined.
        let prepared_namespace = self.call_metaclass_prepare(&metaclass, name_id, &resolved_bases, &class_kwargs)?;
        if matches!(prepared_namespace, PreparedNamespace::FramePushed) {
            self.pending_class_build = Some(PendingClassBuild::Prepare {
                name_id,
                func_id,
                call_position,
                bases: resolved_bases,
                class_kwargs,
                metaclass,
                orig_bases: Some(orig_bases_id),
            });
            return Ok(true);
        }

        let prepared_namespace = match prepared_namespace {
            PreparedNamespace::None => None,
            PreparedNamespace::Ready(value) => Some(value),
            PreparedNamespace::FramePushed => unreachable!("handled above"),
        };

        // Validate bases and convert to HeapIds.
        let bases = self.extract_base_ids(resolved_bases)?;

        // Push class body frame.
        self.push_class_body_frame(
            name_id,
            func_id,
            bases,
            metaclass,
            class_kwargs,
            prepared_namespace,
            Some(orig_bases_id),
            call_position,
        )?;

        Ok(true)
    }

    /// Resumes class construction after a pending `__mro_entries__` or `__prepare__` call.
    ///
    /// Returns `true` if a new frame was pushed.
    fn resume_class_build(&mut self, pending: PendingClassBuild, value: Value) -> RunResult<bool> {
        match pending {
            PendingClassBuild::MroEntries {
                name_id,
                func_id,
                call_position,
                remaining_bases,
                mut resolved_bases,
                orig_bases,
                class_kwargs,
            } => {
                let new_bases = self.unpack_mro_entries_result(value)?;
                resolved_bases.extend(new_bases);
                self.run_class_build(
                    name_id,
                    func_id,
                    remaining_bases,
                    resolved_bases,
                    class_kwargs,
                    call_position,
                    Some(orig_bases),
                )
            }
            PendingClassBuild::Prepare {
                name_id,
                func_id,
                call_position,
                bases,
                class_kwargs,
                metaclass,
                orig_bases,
            } => {
                let prepared_namespace = if self.is_prepared_namespace_mapping(&value) {
                    Some(value)
                } else {
                    value.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("__prepare__ must return a mapping".to_string()));
                };

                let bases_ids = self.extract_base_ids(bases)?;
                self.push_class_body_frame(
                    name_id,
                    func_id,
                    bases_ids,
                    metaclass,
                    class_kwargs,
                    prepared_namespace,
                    orig_bases,
                    call_position,
                )?;
                Ok(true)
            }
        }
    }

    /// Unpacks the result of `__mro_entries__`, ensuring it's a tuple of bases.
    ///
    /// Returns owned base values with proper refcount handling.
    fn unpack_mro_entries_result(&mut self, value: Value) -> RunResult<Vec<Value>> {
        match value {
            Value::Ref(id) => {
                if let HeapData::Tuple(tuple) = self.heap.get(id) {
                    let mut items = Vec::with_capacity(tuple.as_vec().len());
                    for item in tuple.as_vec().as_ref() {
                        items.push(item.clone_with_heap(self.heap));
                    }
                    Value::Ref(id).drop_with_heap(self.heap);
                    Ok(items)
                } else {
                    Value::Ref(id).drop_with_heap(self.heap);
                    Err(ExcType::type_error("__mro_entries__ must return a tuple".to_string()))
                }
            }
            other => {
                other.drop_with_heap(self.heap);
                Err(ExcType::type_error("__mro_entries__ must return a tuple".to_string()))
            }
        }
    }

    /// Selects the appropriate metaclass from explicit kwargs or base classes.
    ///
    /// Mirrors CPython's "most derived metaclass" rule to avoid conflicts.
    fn select_metaclass(&mut self, bases: &[Value], explicit: Option<Value>) -> RunResult<Value> {
        if let Some(meta) = explicit {
            if !self.is_valid_metaclass(&meta) {
                meta.drop_with_heap(self.heap);
                return Err(ExcType::type_error("metaclass must be a class".to_string()));
            }
            for base in bases {
                let base_meta = self.metaclass_for_base(base)?;
                if !self.metaclass_is_subclass(&meta, &base_meta) {
                    base_meta.drop_with_heap(self.heap);
                    meta.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("metaclass conflict".to_string()));
                }
                base_meta.drop_with_heap(self.heap);
            }
            return Ok(meta);
        }

        let mut winner = Value::Builtin(crate::builtins::Builtins::Type(crate::types::Type::Type));
        for base in bases {
            let base_meta = self.metaclass_for_base(base)?;
            if self.metaclass_is_subclass(&winner, &base_meta) {
                // Keep current winner.
                base_meta.drop_with_heap(self.heap);
            } else if self.metaclass_is_subclass(&base_meta, &winner) {
                winner.drop_with_heap(self.heap);
                winner = base_meta;
            } else {
                winner.drop_with_heap(self.heap);
                base_meta.drop_with_heap(self.heap);
                return Err(ExcType::type_error("metaclass conflict".to_string()));
            }
        }

        Ok(winner)
    }

    /// Returns true if the value is a valid metaclass (class object or builtin type).
    fn is_valid_metaclass(&mut self, metaclass: &Value) -> bool {
        match metaclass {
            Value::Ref(id) => matches!(self.heap.get(*id), HeapData::ClassObject(_)),
            Value::Builtin(crate::builtins::Builtins::Type(crate::types::Type::Type)) => true,
            _ => false,
        }
    }

    /// Returns the metaclass for a given base value.
    fn metaclass_for_base(&mut self, base: &Value) -> RunResult<Value> {
        match base {
            Value::Ref(id) => {
                if let HeapData::ClassObject(cls) = self.heap.get(*id) {
                    Ok(cls.metaclass().clone_with_heap(self.heap))
                } else {
                    Err(ExcType::type_error("bases must be classes".to_string()))
                }
            }
            Value::Builtin(
                crate::builtins::Builtins::Type(_)
                | crate::builtins::Builtins::ExcType(_)
                | crate::builtins::Builtins::Function(crate::builtins::BuiltinsFunctions::Type),
            )
            | Value::Marker(crate::value::Marker(StaticStrings::TypingNamedTuple)) => Ok(Value::Builtin(
                crate::builtins::Builtins::Type(crate::types::Type::Type),
            )),
            _ => Err(ExcType::type_error("bases must be classes".to_string())),
        }
    }

    /// Returns true if `candidate` is a subclass of `base` in metaclass terms.
    fn metaclass_is_subclass(&mut self, candidate: &Value, base: &Value) -> bool {
        match (candidate, base) {
            (
                Value::Builtin(crate::builtins::Builtins::Type(ct)),
                Value::Builtin(crate::builtins::Builtins::Type(bt)),
            ) => ct == bt,
            (Value::Ref(_), Value::Builtin(crate::builtins::Builtins::Type(crate::types::Type::Type))) => true,
            (Value::Builtin(crate::builtins::Builtins::Type(crate::types::Type::Type)), Value::Ref(_)) => false,
            (Value::Ref(cand_id), Value::Ref(base_id)) => {
                if let HeapData::ClassObject(cand_cls) = self.heap.get(*cand_id) {
                    cand_cls.is_subclass_of(*cand_id, *base_id)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Calls `metaclass.__prepare__` if defined, returning the prepared namespace.
    fn call_metaclass_prepare(
        &mut self,
        metaclass: &Value,
        name_id: StringId,
        bases: &[Value],
        class_kwargs: &Value,
    ) -> RunResult<PreparedNamespace> {
        let meta_id = match metaclass {
            Value::Ref(id) => *id,
            _ => return Ok(PreparedNamespace::None),
        };

        let HeapData::ClassObject(meta_cls) = self.heap.get(meta_id) else {
            return Ok(PreparedNamespace::None);
        };

        let prepare_name: StringId = StaticStrings::DunderPrepare.into();
        let prepare_name_str = self.interns.get_str(prepare_name);
        let Some((method, _)) = meta_cls.mro_lookup_attr(prepare_name_str, meta_id, self.heap, self.interns) else {
            return Ok(PreparedNamespace::None);
        };

        let mut base_values: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
        base_values.extend(bases.iter().map(|v| v.clone_with_heap(self.heap)));
        let bases_tuple = crate::types::allocate_tuple(base_values, self.heap)?;

        let kwargs = self.clone_kwargs_from_value(class_kwargs)?;
        let args = ArgValues::ArgsKargs {
            args: vec![Value::InternString(name_id), bases_tuple],
            kwargs,
        };

        match self.call_class_dunder(meta_id, method, args)? {
            CallResult::Push(value) => {
                if self.is_prepared_namespace_mapping(&value) {
                    Ok(PreparedNamespace::Ready(value))
                } else {
                    value.drop_with_heap(self.heap);
                    Err(ExcType::type_error("__prepare__ must return a mapping".to_string()))
                }
            }
            CallResult::FramePushed => Ok(PreparedNamespace::FramePushed),
            other => Err(RunError::internal(format!(
                "__prepare__ returned unsupported call result: {other:?}"
            ))),
        }
    }

    /// Returns true if a `__prepare__` value is a valid namespace mapping.
    fn is_prepared_namespace_mapping(&mut self, value: &Value) -> bool {
        match value {
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::Dict(_) => true,
                HeapData::Instance(inst) => {
                    let class_id = inst.class_id();
                    let HeapData::ClassObject(cls) = self.heap.get(class_id) else {
                        return false;
                    };
                    let has_getitem = cls.mro_has_attr("__getitem__", class_id, self.heap, self.interns);
                    let has_setitem = cls.mro_has_attr("__setitem__", class_id, self.heap, self.interns);
                    has_getitem && has_setitem
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Clones class keyword arguments into a `KwargsValues` for calling user functions.
    fn clone_kwargs_from_value(&mut self, kwargs: &Value) -> RunResult<crate::args::KwargsValues> {
        match kwargs {
            Value::Ref(id) => {
                let dict = self.heap.with_entry_mut(*id, |heap, data| {
                    let HeapData::Dict(dict) = data else {
                        return Err(ExcType::type_error("class kwargs must be a dict".to_string()));
                    };
                    dict.clone_with_heap(heap, self.interns)
                })?;
                Ok(crate::args::KwargsValues::Dict(dict))
            }
            Value::None => Ok(crate::args::KwargsValues::Empty),
            _ => Err(ExcType::type_error("class kwargs must be a dict".to_string())),
        }
    }

    /// Validates base values and converts them to HeapIds.
    fn extract_base_ids(&mut self, bases: Vec<Value>) -> RunResult<Vec<HeapId>> {
        let mut base_ids = Vec::with_capacity(bases.len());
        for base in bases {
            match base {
                value @ Value::Ref(id) => {
                    if matches!(self.heap.get(id), HeapData::ClassObject(_)) {
                        self.heap.inc_ref(id);
                        base_ids.push(id);
                        value.drop_with_heap(self.heap);
                    } else {
                        value.drop_with_heap(self.heap);
                        return Err(ExcType::type_error("bases must be classes".to_string()));
                    }
                }
                Value::Builtin(crate::builtins::Builtins::Type(t)) => {
                    let class_id = self.heap.builtin_class_id(t)?;
                    self.heap.inc_ref(class_id);
                    base_ids.push(class_id);
                }
                Value::Builtin(crate::builtins::Builtins::ExcType(exc_type)) => {
                    let class_id = self.heap.builtin_class_id(crate::types::Type::Exception(exc_type))?;
                    self.heap.inc_ref(class_id);
                    base_ids.push(class_id);
                }
                Value::Builtin(crate::builtins::Builtins::Function(crate::builtins::BuiltinsFunctions::Type)) => {
                    let class_id = self.heap.builtin_class_id(crate::types::Type::Type)?;
                    self.heap.inc_ref(class_id);
                    base_ids.push(class_id);
                }
                Value::Marker(crate::value::Marker(StaticStrings::TypingNamedTuple)) => {
                    // `typing.NamedTuple` class syntax is represented as a marker base.
                    // Lower it to `tuple` as the concrete runtime base.
                    let class_id = self.heap.builtin_class_id(crate::types::Type::Tuple)?;
                    self.heap.inc_ref(class_id);
                    base_ids.push(class_id);
                }
                other => {
                    other.drop_with_heap(self.heap);
                    return Err(ExcType::type_error("bases must be classes".to_string()));
                }
            }
        }
        Ok(base_ids)
    }

    /// Pushes a class body frame with the provided class metadata.
    #[expect(clippy::too_many_arguments)]
    fn push_class_body_frame(
        &mut self,
        name_id: StringId,
        func_id: FunctionId,
        bases: Vec<HeapId>,
        metaclass: Value,
        class_kwargs: Value,
        prepared_namespace: Option<Value>,
        orig_bases: Option<HeapId>,
        call_position: CodeRange,
    ) -> RunResult<()> {
        let func = self.interns.get_function(func_id);
        if func.class_free_var_target_slots.len() != func.free_var_enclosing_slots.len() {
            return Err(RunError::internal(
                "class body free-var metadata length mismatch".to_string(),
            ));
        }

        let captured_free_var_cells: Vec<(usize, HeapId)> = if func.class_free_var_target_slots.is_empty() {
            Vec::new()
        } else {
            let enclosing_namespace_idx = self.current_frame().namespace_idx;
            let enclosing_namespace = self.namespaces.get(enclosing_namespace_idx);
            func.class_free_var_target_slots
                .iter()
                .zip(&func.free_var_enclosing_slots)
                .map(|(target_slot, enclosing_slot)| {
                    let value = enclosing_namespace.get(*enclosing_slot);
                    let Value::Ref(cell_id) = value else {
                        return Err(RunError::internal(
                            "class body expected captured cell reference in enclosing namespace".to_string(),
                        ));
                    };
                    if !matches!(self.heap.get(*cell_id), HeapData::Cell(_)) {
                        return Err(RunError::internal(
                            "class body expected captured HeapData::Cell in enclosing namespace".to_string(),
                        ));
                    }
                    Ok((target_slot.index(), *cell_id))
                })
                .collect::<RunResult<Vec<_>>>()?
        };

        let namespace_idx = self.namespaces.new_namespace(func.namespace_size, self.heap)?;

        let namespace = self.namespaces.get_mut(namespace_idx).mut_vec();
        namespace.resize_with(func.namespace_size, || Value::Undefined);

        let mut frame_cells = Vec::new();
        for (slot_idx, cell_id) in captured_free_var_cells {
            self.heap.inc_ref(cell_id);
            frame_cells.push(cell_id);
            if namespace.len() <= slot_idx {
                namespace.resize_with(slot_idx + 1, || Value::Undefined);
            }
            namespace[slot_idx] = Value::Ref(cell_id);
        }
        if let Some(cell_slot) = func.class_cell_slot {
            let cell_id = self.heap.allocate(HeapData::Cell(Value::Undefined))?;
            frame_cells.push(cell_id);
            let slot_idx = cell_slot.index();
            if namespace.len() <= slot_idx {
                namespace.resize_with(slot_idx + 1, || Value::Undefined);
            }
            namespace[slot_idx] = Value::Ref(cell_id);
        }

        let code = &func.code;
        self.frames.push(CallFrame::new_class_body(
            code,
            self.stack.len(),
            namespace_idx,
            func_id,
            frame_cells,
            Some(call_position),
            ClassBodyInfo {
                name_id,
                func_id,
                bases,
                metaclass,
                class_kwargs,
                prepared_namespace,
                orig_bases,
                class_cell_slot: func.class_cell_slot,
            },
        ));
        self.tracer
            .on_call(Some(self.interns.get_str(name_id)), self.frames.len());

        Ok(())
    }

    /// Finalizes a class body frame by extracting the namespace into a ClassObject.
    ///
    /// Called from the `ReturnValue` handler when the current frame is a class body.
    /// Extracts named local variables from the namespace, builds a Dict, creates a
    /// `ClassObject`, and returns its Value. The frame and namespace are cleaned up.
    ///
    /// After creating the class, calls `__set_name__` on any descriptors in the
    /// class namespace that define it.
    fn finalize_class_body(&mut self) -> RunResult<FinalizeClassResult> {
        let frame_depth = self.frames.len();
        self.drop_pending_getattr_for_frame(frame_depth);
        self.drop_pending_binary_dunder_for_frame(frame_depth);
        let frame = self.frames.pop().expect("no frame to pop");
        let mut class_info = Some(frame.class_body_info.expect("not a class body frame"));
        let namespace_idx = frame.namespace_idx;
        let call_position = frame.call_position;
        let class_position = self.extend_class_def_position(
            call_position,
            class_info.as_ref().expect("class body info should be present").func_id,
        );

        // Clean up frame's stack region
        while self.stack.len() > frame.stack_base {
            let value = self.stack.pop().unwrap();
            value.drop_with_heap(self.heap);
        }

        // Build a Dict from the class body namespace.
        //
        // If `__prepare__` returned a dict, clone it to preserve any pre-populated
        // entries. For non-dict mappings, call `items()` and seed the class dict
        // from those pairs.
        let class_dict = if let Some(prepared_value) = class_info
            .as_ref()
            .expect("class body info should be present")
            .prepared_namespace
            .as_ref()
        {
            match prepared_value {
                Value::Ref(prepared_id) if matches!(self.heap.get(*prepared_id), HeapData::Dict(_)) => {
                    match self.heap.with_entry_mut(*prepared_id, |heap, data| {
                        let HeapData::Dict(dict) = data else {
                            return Err(ExcType::type_error("__prepare__ must return a mapping".to_string()));
                        };
                        dict.clone_with_heap(heap, self.interns)
                    }) {
                        Ok(dict) => dict,
                        Err(err) => {
                            self.namespaces.drop_with_heap(namespace_idx, self.heap);
                            self.drop_class_body_info(class_info.take().expect("class body info should be present"));
                            return Err(err);
                        }
                    }
                }
                Value::Ref(_) => {
                    let items_name: StringId = StaticStrings::Items.into();
                    let mapping = prepared_value.clone_with_heap(self.heap);
                    match self.call_attr(mapping, items_name, ArgValues::Empty) {
                        Ok(CallResult::Push(items_value)) => match self.dict_from_items_value(items_value) {
                            Ok(dict) => dict,
                            Err(err) => {
                                self.namespaces.drop_with_heap(namespace_idx, self.heap);
                                self.drop_class_body_info(
                                    class_info.take().expect("class body info should be present"),
                                );
                                return Err(err);
                            }
                        },
                        Ok(CallResult::FramePushed) => {
                            self.pending_class_finalize = Some(PendingClassFinalize::PreparedItems {
                                class_info: class_info.take().expect("class body info should be present"),
                                namespace_idx,
                                call_position: class_position,
                                pending_frame_depth: self.frames.len(),
                            });
                            return Ok(FinalizeClassResult::FramePushed);
                        }
                        Ok(other) => {
                            self.namespaces.drop_with_heap(namespace_idx, self.heap);
                            self.drop_class_body_info(class_info.take().expect("class body info should be present"));
                            return Err(RunError::internal(format!(
                                "__prepare__ items() returned unsupported call result: {other:?}"
                            )));
                        }
                        Err(err) => {
                            self.namespaces.drop_with_heap(namespace_idx, self.heap);
                            self.drop_class_body_info(class_info.take().expect("class body info should be present"));
                            return Err(err);
                        }
                    }
                }
                _ => {
                    self.namespaces.drop_with_heap(namespace_idx, self.heap);
                    self.drop_class_body_info(class_info.take().expect("class body info should be present"));
                    return Err(RunError::internal("__prepare__ returned invalid mapping"));
                }
            }
        } else {
            Dict::new()
        };

        let mut class_dict_guard = HeapGuard::new(class_dict, self);
        {
            let (class_dict, this) = class_dict_guard.as_parts_mut();
            if let Err(err) = this.merge_named_namespace_slots_into_class_dict(
                class_info.as_ref().expect("class body info should be present").func_id,
                namespace_idx,
                class_dict,
            ) {
                this.namespaces.drop_with_heap(namespace_idx, this.heap);
                this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                return Err(err);
            }

            if this.should_invoke_custom_metaclass_constructor(
                &class_info
                    .as_ref()
                    .expect("class body info should be present")
                    .metaclass,
            ) {
                let call_result = match this.call_custom_metaclass_constructor(
                    class_info.as_ref().expect("class body info should be present"),
                    class_dict,
                ) {
                    Ok(result) => result,
                    Err(err) => {
                        this.namespaces.drop_with_heap(namespace_idx, this.heap);
                        this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                        return Err(err);
                    }
                };
                match call_result {
                    CallResult::Push(class_value) => {
                        let class_cell_id = match this.class_cell_id_from_namespace(
                            class_info.as_ref().expect("class body info should be present"),
                            namespace_idx,
                        ) {
                            Ok(cell_id) => cell_id,
                            Err(err) => {
                                this.namespaces.drop_with_heap(namespace_idx, this.heap);
                                this.drop_class_body_info(
                                    class_info.take().expect("class body info should be present"),
                                );
                                return Err(Self::add_class_def_position(err, class_position));
                            }
                        };
                        if let Err(err) = this.bind_class_cell_from_class_value(class_cell_id, &class_value) {
                            this.namespaces.drop_with_heap(namespace_idx, this.heap);
                            this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                            return Err(Self::add_class_def_position(err, class_position));
                        }
                        if let Err(err) = this.postprocess_custom_metaclass_result(
                            &class_value,
                            class_info.as_ref().expect("class body info should be present"),
                        ) {
                            this.namespaces.drop_with_heap(namespace_idx, this.heap);
                            this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                            return Err(Self::add_class_def_position(err, class_position));
                        }
                        this.namespaces.drop_with_heap(namespace_idx, this.heap);
                        this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                        return Ok(FinalizeClassResult::Done(class_value));
                    }
                    CallResult::FramePushed => {
                        let metaclass_id = if let Value::Ref(id) = &class_info
                            .as_ref()
                            .expect("class body info should be present")
                            .metaclass
                        {
                            *id
                        } else {
                            this.namespaces.drop_with_heap(namespace_idx, this.heap);
                            this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                            return Err(RunError::internal(
                                "custom metaclass frame push requires heap metaclass id",
                            ));
                        };
                        this.pending_class_finalize = Some(PendingClassFinalize::MetaclassCall {
                            class_info: class_info.take().expect("class body info should be present"),
                            namespace_idx,
                            call_position: class_position,
                            metaclass_id,
                            pending_frame_depth: this.frames.len(),
                        });
                        return Ok(FinalizeClassResult::FramePushed);
                    }
                    other => {
                        this.namespaces.drop_with_heap(namespace_idx, this.heap);
                        this.drop_class_body_info(class_info.take().expect("class body info should be present"));
                        return Err(RunError::internal(format!(
                            "custom metaclass call returned unsupported call result: {other:?}"
                        )));
                    }
                }
            }
        }

        let class_dict = class_dict_guard.into_inner();
        let class_info = class_info.take().expect("class body info should be present");
        let class_value = match self.finish_class_body_from_dict(class_info, namespace_idx, class_dict, class_position)
        {
            Ok(value) => value,
            Err(err) => return Err(Self::add_class_def_position(err, class_position)),
        };
        Ok(FinalizeClassResult::Done(class_value))
    }

    /// Resumes class finalization after a prepared-namespace `items()` call returns.
    fn resume_class_finalize(&mut self, pending: PendingClassFinalize, value: Value) -> RunResult<Value> {
        match pending {
            PendingClassFinalize::PreparedItems {
                class_info,
                namespace_idx,
                call_position,
                pending_frame_depth: _,
            } => {
                let class_dict = match self.dict_from_items_value(value) {
                    Ok(dict) => dict,
                    Err(err) => {
                        self.namespaces.drop_with_heap(namespace_idx, self.heap);
                        self.drop_class_body_info(class_info);
                        return Err(err);
                    }
                };

                match self.finish_class_body_from_dict(class_info, namespace_idx, class_dict, call_position) {
                    Ok(class_value) => Ok(class_value),
                    Err(err) => Err(Self::add_class_def_position(err, call_position)),
                }
            }
            PendingClassFinalize::MetaclassCall {
                class_info,
                namespace_idx,
                call_position,
                metaclass_id: _,
                pending_frame_depth: _,
            } => {
                let class_cell_id = match self.class_cell_id_from_namespace(&class_info, namespace_idx) {
                    Ok(cell_id) => cell_id,
                    Err(err) => {
                        self.namespaces.drop_with_heap(namespace_idx, self.heap);
                        self.drop_class_body_info(class_info);
                        return Err(Self::add_class_def_position(err, call_position));
                    }
                };
                if let Err(err) = self.bind_class_cell_from_class_value(class_cell_id, &value) {
                    self.namespaces.drop_with_heap(namespace_idx, self.heap);
                    self.drop_class_body_info(class_info);
                    return Err(Self::add_class_def_position(err, call_position));
                }
                if let Err(err) = self.postprocess_custom_metaclass_result(&value, &class_info) {
                    self.namespaces.drop_with_heap(namespace_idx, self.heap);
                    self.drop_class_body_info(class_info);
                    return Err(Self::add_class_def_position(err, call_position));
                }
                self.namespaces.drop_with_heap(namespace_idx, self.heap);
                self.drop_class_body_info(class_info);
                Ok(value)
            }
        }
    }

    /// Drops pending class-finalization state after unwinding past its helper frame.
    ///
    /// During exception unwinding a metaclass/helper call frame may be popped without
    /// returning through `ReturnValue`. In that case pending state must be dropped so
    /// later unrelated returns do not resume stale class finalization.
    fn discard_stale_pending_class_finalize(&mut self) {
        let Some(pending) = self.pending_class_finalize.take() else {
            return;
        };
        if pending.pending_frame_depth() <= self.frames.len() {
            self.pending_class_finalize = Some(pending);
            return;
        }
        match pending {
            PendingClassFinalize::PreparedItems {
                class_info,
                namespace_idx,
                ..
            }
            | PendingClassFinalize::MetaclassCall {
                class_info,
                namespace_idx,
                ..
            } => {
                self.namespaces.drop_with_heap(namespace_idx, self.heap);
                self.drop_class_body_info(class_info);
            }
        }
    }

    /// Returns whether class finalization should invoke a custom metaclass.
    ///
    /// Builtin `type` uses Ouros's direct class-construction path.
    fn should_invoke_custom_metaclass_constructor(&self, metaclass: &Value) -> bool {
        matches!(metaclass, Value::Ref(_))
    }

    /// Calls a custom metaclass constructor with `(name, bases, namespace, **kwargs)`.
    fn call_custom_metaclass_constructor(
        &mut self,
        class_info: &ClassBodyInfo,
        class_dict: &Dict,
    ) -> RunResult<CallResult> {
        let metaclass = class_info.metaclass.clone_with_heap(self.heap);

        let mut base_values: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
        for &base_id in &class_info.bases {
            if let Some(ty) = self.heap.builtin_type_for_class_id(base_id) {
                base_values.push(Value::Builtin(crate::builtins::Builtins::Type(ty)));
            } else {
                self.heap.inc_ref(base_id);
                base_values.push(Value::Ref(base_id));
            }
        }
        let bases_tuple = crate::types::allocate_tuple(base_values, self.heap)?;

        let namespace_dict = class_dict.clone_with_heap(self.heap, self.interns)?;
        let namespace_id = self.heap.allocate(HeapData::Dict(namespace_dict))?;
        let namespace_value = Value::Ref(namespace_id);

        let kwargs = self.clone_kwargs_from_value(&class_info.class_kwargs)?;
        let args = ArgValues::ArgsKargs {
            args: vec![Value::InternString(class_info.name_id), bases_tuple, namespace_value],
            kwargs,
        };

        self.call_function(metaclass, args)
    }

    /// Runs post-class-creation hooks after a custom metaclass call returns.
    ///
    /// Custom metaclass construction bypasses `finish_class_body_from_dict`, so we
    /// must still schedule descriptor `__set_name__` and base `__init_subclass__`
    /// calls (and ABC abstract metadata refresh) for class parity with CPython.
    fn postprocess_custom_metaclass_result(
        &mut self,
        class_value: &Value,
        class_info: &ClassBodyInfo,
    ) -> RunResult<()> {
        let Value::Ref(class_id) = class_value else {
            return Ok(());
        };
        if !matches!(self.heap.get(*class_id), HeapData::ClassObject(_)) {
            return Ok(());
        }

        crate::modules::abc::maybe_recompute_abstracts_for_class(*class_id, self.heap, self.interns)?;
        self.collect_set_name_descriptors(*class_id);

        let kwargs = class_info.class_kwargs.clone_with_heap(self.heap);
        self.collect_init_subclass_calls(*class_id, kwargs);
        Ok(())
    }

    /// Builds a Dict from an iterable of `(key, value)` pairs.
    ///
    /// Mirrors `dict.update()` sequence handling to ensure error parity.
    fn dict_from_items_value(&mut self, items_value: Value) -> RunResult<Dict> {
        let mut dict = Dict::new();
        let mut iter = OurosIter::new(items_value, self.heap, self.interns)?;

        loop {
            let item = match iter.for_next(self.heap, self.interns) {
                Ok(Some(i)) => i,
                Ok(None) => break,
                Err(e) => {
                    iter.drop_with_heap(self.heap);
                    return Err(e);
                }
            };

            let mut pair_iter = match OurosIter::new(item, self.heap, self.interns) {
                Ok(pi) => pi,
                Err(e) => {
                    iter.drop_with_heap(self.heap);
                    return Err(e);
                }
            };

            let key = match pair_iter.for_next(self.heap, self.interns) {
                Ok(Some(k)) => k,
                Ok(None) => {
                    pair_iter.drop_with_heap(self.heap);
                    iter.drop_with_heap(self.heap);
                    return Err(ExcType::type_error(
                        "dictionary update sequence element has length 0; 2 is required",
                    ));
                }
                Err(e) => {
                    pair_iter.drop_with_heap(self.heap);
                    iter.drop_with_heap(self.heap);
                    return Err(e);
                }
            };

            let value = match pair_iter.for_next(self.heap, self.interns) {
                Ok(Some(v)) => v,
                Ok(None) => {
                    key.drop_with_heap(self.heap);
                    pair_iter.drop_with_heap(self.heap);
                    iter.drop_with_heap(self.heap);
                    return Err(ExcType::type_error(
                        "dictionary update sequence element has length 1; 2 is required",
                    ));
                }
                Err(e) => {
                    key.drop_with_heap(self.heap);
                    pair_iter.drop_with_heap(self.heap);
                    iter.drop_with_heap(self.heap);
                    return Err(e);
                }
            };

            match pair_iter.for_next(self.heap, self.interns) {
                Ok(Some(first_extra)) => {
                    first_extra.drop_with_heap(self.heap);
                    key.drop_with_heap(self.heap);
                    value.drop_with_heap(self.heap);
                    loop {
                        match pair_iter.for_next(self.heap, self.interns) {
                            Ok(Some(extra)) => extra.drop_with_heap(self.heap),
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                    pair_iter.drop_with_heap(self.heap);
                    iter.drop_with_heap(self.heap);
                    return Err(ExcType::type_error(
                        "dictionary update sequence element has length > 2; 2 is required",
                    ));
                }
                Ok(None) => {}
                Err(e) => {
                    key.drop_with_heap(self.heap);
                    value.drop_with_heap(self.heap);
                    pair_iter.drop_with_heap(self.heap);
                    iter.drop_with_heap(self.heap);
                    return Err(e);
                }
            }
            pair_iter.drop_with_heap(self.heap);

            match dict.set(key, value, self.heap, self.interns) {
                Ok(Some(old_value)) => old_value.drop_with_heap(self.heap),
                Ok(None) => {}
                Err(e) => {
                    iter.drop_with_heap(self.heap);
                    return Err(e);
                }
            }
        }

        iter.drop_with_heap(self.heap);
        Ok(dict)
    }

    /// Resolves the class-body `__class__` cell object from a namespace slot.
    ///
    /// Returns an owned reference to the cell object so callers can safely keep it
    /// alive after dropping the class-body namespace.
    fn class_cell_id_from_namespace(
        &mut self,
        class_info: &ClassBodyInfo,
        namespace_idx: NamespaceId,
    ) -> RunResult<Option<HeapId>> {
        let Some(cell_slot) = class_info.class_cell_slot else {
            return Ok(None);
        };
        let namespace = self.namespaces.get(namespace_idx);
        #[cfg_attr(not(feature = "ref-count-panic"), expect(unused_mut))]
        let mut cell_val = namespace.get(cell_slot).copy_for_extend();
        let cell_id = match &cell_val {
            Value::Ref(id) => {
                self.heap.inc_ref(*id);
                Some(*id)
            }
            Value::Undefined => None,
            _ => return Err(RunError::internal("class __class__ cell slot is not a cell")),
        };
        #[cfg(feature = "ref-count-panic")]
        if matches!(cell_val, Value::Ref(_)) {
            cell_val.dec_ref_forget();
        }
        Ok(cell_id)
    }

    /// Writes the created class into the captured `__class__` cell when present.
    ///
    /// Zero-argument `super()` in class methods depends on this cell being set to the
    /// final class object. The owned cell reference is always released.
    fn bind_class_cell_from_class_value(
        &mut self,
        class_cell_id: Option<HeapId>,
        class_value: &Value,
    ) -> RunResult<()> {
        let Some(cell_id) = class_cell_id else {
            return Ok(());
        };
        if let Value::Ref(class_id) = class_value
            && matches!(self.heap.get(*class_id), HeapData::ClassObject(_))
        {
            self.heap.inc_ref(*class_id);
            self.heap.set_cell_value(cell_id, Value::Ref(*class_id))?;
        }
        Value::Ref(cell_id).drop_with_heap(self.heap);
        Ok(())
    }

    /// Completes class creation once a class dict has been built.
    fn finish_class_body_from_dict(
        &mut self,
        class_info: ClassBodyInfo,
        namespace_idx: NamespaceId,
        class_dict: Dict,
        call_position: Option<CodeRange>,
    ) -> RunResult<Value> {
        let this = self;
        let mut class_dict_guard = HeapGuard::new(class_dict, this);
        let class_cell_id = {
            let (class_dict, this) = class_dict_guard.as_parts_mut();
            this.merge_named_namespace_slots_into_class_dict(class_info.func_id, namespace_idx, class_dict)?;
            let func = this.interns.get_function(class_info.func_id);
            let class_cell_id = this.class_cell_id_from_namespace(&class_info, namespace_idx)?;

            // Clean up the namespace (decrements all refcounts for values in it)
            this.namespaces.drop_with_heap(namespace_idx, this.heap);

            // Drop the prepared namespace ref now that we've cloned it.
            if let Some(prepared_value) = class_info.prepared_namespace {
                prepared_value.drop_with_heap(this.heap);
            }

            // Populate standard class metadata if missing.
            let module_key: StringId = StaticStrings::DunderModule.into();
            if class_dict.get_by_str("__module__", this.heap, this.interns).is_none() {
                let module_val = Value::InternString(StaticStrings::MainModule.into());
                if let Some(old) =
                    class_dict.set(Value::InternString(module_key), module_val, this.heap, this.interns)?
                {
                    old.drop_with_heap(this.heap);
                }
            }

            let qualname_key: StringId = StaticStrings::DunderQualname.into();
            if class_dict.get_by_str("__qualname__", this.heap, this.interns).is_none() {
                let qualname_val = match &func.qualname {
                    crate::value::EitherStr::Interned(id) => Value::InternString(*id),
                    crate::value::EitherStr::Heap(s) => {
                        let id = this.heap.allocate(HeapData::Str(crate::types::Str::from(s.as_str())))?;
                        Value::Ref(id)
                    }
                };
                if let Some(old) =
                    class_dict.set(Value::InternString(qualname_key), qualname_val, this.heap, this.interns)?
                {
                    old.drop_with_heap(this.heap);
                }
            }

            let annotations_key: StringId = StaticStrings::DunderAnnotations.into();
            if class_dict
                .get_by_str("__annotations__", this.heap, this.interns)
                .is_none()
            {
                let ann_dict = Dict::new();
                let ann_id = this.heap.allocate(HeapData::Dict(ann_dict))?;
                if let Some(old) = class_dict.set(
                    Value::InternString(annotations_key),
                    Value::Ref(ann_id),
                    this.heap,
                    this.interns,
                )? {
                    old.drop_with_heap(this.heap);
                }
            }

            let type_params_key: StringId = StaticStrings::DunderTypeParams.into();
            if class_dict
                .get_by_str("__type_params__", this.heap, this.interns)
                .is_none()
                && !func.type_params.is_empty()
            {
                let mut type_params: smallvec::SmallVec<[Value; 3]> = smallvec::SmallVec::new();
                for name_id in &func.type_params {
                    type_params.push(Value::InternString(*name_id));
                }
                let tuple_val = crate::types::allocate_tuple(type_params, this.heap)?;
                if let Some(old) =
                    class_dict.set(Value::InternString(type_params_key), tuple_val, this.heap, this.interns)?
                {
                    old.drop_with_heap(this.heap);
                }
            }

            if this.class_has_typing_namedtuple_base(class_info.orig_bases) {
                let field_names = this.namedtuple_fields_from_annotations(class_dict);
                let class_name = this.interns.get_str(class_info.name_id).to_string();
                let factory = crate::types::NamedTupleFactory::new(class_name, field_names);
                let factory_id = this.heap.allocate(HeapData::NamedTupleFactory(factory))?;

                if let Some(orig_id) = class_info.orig_bases {
                    Value::Ref(orig_id).drop_with_heap(this.heap);
                }
                for base_id in class_info.bases {
                    this.heap.dec_ref(base_id);
                }
                class_info.metaclass.drop_with_heap(this.heap);
                class_info.class_kwargs.drop_with_heap(this.heap);
                if let Some(cell_id) = class_cell_id {
                    Value::Ref(cell_id).drop_with_heap(this.heap);
                }
                return Ok(Value::Ref(factory_id));
            }

            if let Some(orig_id) = class_info.orig_bases {
                let orig_key: StringId = StaticStrings::DunderOrigBases.into();
                if class_dict
                    .get_by_str("__orig_bases__", this.heap, this.interns)
                    .is_none()
                {
                    this.heap.inc_ref(orig_id);
                    if let Some(old) = class_dict.set(
                        Value::InternString(orig_key),
                        Value::Ref(orig_id),
                        this.heap,
                        this.interns,
                    )? {
                        old.drop_with_heap(this.heap);
                    }
                }
                Value::Ref(orig_id).drop_with_heap(this.heap);
            }
            class_cell_id
        };
        let class_dict = class_dict_guard.into_inner();

        // Create the ClassObject with bases.
        // We need to compute the MRO after allocation (because MRO includes self_id).
        let class_name = crate::value::EitherStr::Interned(class_info.name_id);
        let bases = class_info.bases;
        let metaclass = class_info.metaclass;
        let class_uid = this.heap.next_class_uid();
        // Allocate with empty MRO first, then compute and set it.
        let class_obj = ClassObject::new(class_name, class_uid, metaclass, class_dict, bases.clone(), vec![]);
        let heap_id = this.heap.allocate(HeapData::ClassObject(class_obj))?;

        let class_kwargs = class_info.class_kwargs;

        // Compute C3 MRO now that we have the HeapId for self.
        let mro = match crate::types::compute_c3_mro(heap_id, &bases, this.heap, this.interns) {
            Ok(mro) => mro,
            Err(err) => {
                if let Some(cell_id) = class_cell_id {
                    Value::Ref(cell_id).drop_with_heap(this.heap);
                }
                class_kwargs.drop_with_heap(this.heap);
                this.heap.dec_ref(heap_id);
                return Err(Self::add_class_def_position(err, call_position));
            }
        };

        // Inc_ref for each entry in the MRO (the class holds these references).
        for &mro_id in &mro {
            this.heap.inc_ref(mro_id);
        }

        // Set the MRO on the class object.
        if let HeapData::ClassObject(cls) = this.heap.get_mut(heap_id) {
            cls.set_mro(mro);
        }

        // Register direct subclasses for `type.__subclasses__()`.
        if bases.is_empty() {
            let object_id = this.heap.builtin_class_id(crate::types::Type::Object)?;
            this.heap.with_entry_mut(object_id, |_, data| {
                let HeapData::ClassObject(cls) = data else {
                    return Err(RunError::internal("builtin object is not a class object"));
                };
                cls.register_subclass(heap_id, class_uid);
                Ok(())
            })?;
        } else {
            for &base_id in &bases {
                this.heap.with_entry_mut(base_id, |_, data| {
                    let HeapData::ClassObject(cls) = data else {
                        return Err(RunError::internal("base is not a class object"));
                    };
                    cls.register_subclass(heap_id, class_uid);
                    Ok(())
                })?;
            }
        }

        // Extract __slots__ if defined in the class namespace.
        // __slots__ is typically a tuple of strings naming allowed instance attributes.
        if let Err(err) = this.extract_slots(heap_id) {
            if let Some(cell_id) = class_cell_id {
                Value::Ref(cell_id).drop_with_heap(this.heap);
            }
            class_kwargs.drop_with_heap(this.heap);
            this.heap.dec_ref(heap_id);
            return Err(err);
        }

        // ABC classes created with `metaclass=ABCMeta` must compute abstract
        // metadata at class-finalization time (even when not inheriting `ABC`).
        if let Err(err) = crate::modules::abc::maybe_recompute_abstracts_for_class(heap_id, this.heap, this.interns) {
            if let Some(cell_id) = class_cell_id {
                Value::Ref(cell_id).drop_with_heap(this.heap);
            }
            class_kwargs.drop_with_heap(this.heap);
            this.heap.dec_ref(heap_id);
            return Err(err);
        }

        // Collect __set_name__ calls for descriptors in the class namespace.
        // These will be processed in the main loop after the class value is pushed,
        // to avoid re-entrancy issues with self.run().
        this.collect_set_name_descriptors(heap_id);

        // Collect __init_subclass__ calls for base classes that define it.
        // __init_subclass__ is called on each base class with cls=new_class.
        this.collect_init_subclass_calls(heap_id, class_kwargs);

        if let Some(cell_id) = class_cell_id {
            this.heap.inc_ref(heap_id);
            if let Err(err) = this.heap.set_cell_value(cell_id, Value::Ref(heap_id)) {
                Value::Ref(cell_id).drop_with_heap(this.heap);
                return Err(err);
            }
            Value::Ref(cell_id).drop_with_heap(this.heap);
        }

        Ok(Value::Ref(heap_id))
    }

    /// Returns whether original class bases included `typing.NamedTuple`.
    fn class_has_typing_namedtuple_base(&self, orig_bases: Option<HeapId>) -> bool {
        let Some(orig_id) = orig_bases else {
            return false;
        };
        let HeapData::Tuple(tuple) = self.heap.get(orig_id) else {
            return false;
        };
        tuple.as_vec().iter().any(|value| {
            matches!(
                value,
                Value::Marker(crate::value::Marker(StaticStrings::TypingNamedTuple))
            )
        })
    }

    /// Extracts namedtuple field names from class `__annotations__` in declaration order.
    fn namedtuple_fields_from_annotations(&mut self, class_dict: &Dict) -> Vec<crate::value::EitherStr> {
        let Some(annotations) = class_dict.get_by_str("__annotations__", self.heap, self.interns) else {
            return Vec::new();
        };
        let Value::Ref(annotations_id) = annotations else {
            return Vec::new();
        };
        let HeapData::Dict(dict) = self.heap.get(*annotations_id) else {
            return Vec::new();
        };

        let mut field_names = Vec::new();
        for (key, _) in dict {
            if let Some(text) = key.as_either_str(self.heap) {
                field_names.push(crate::value::EitherStr::Heap(text.as_str(self.interns).to_string()));
            }
        }
        field_names
    }

    /// Merges named class-body namespace slots into a class dictionary.
    ///
    /// Class bodies execute in VM local slots. This helper copies all named,
    /// non-`Undefined` locals into `class_dict` so both builtin and custom
    /// metaclass construction paths receive the full class namespace.
    fn merge_named_namespace_slots_into_class_dict(
        &mut self,
        func_id: FunctionId,
        namespace_idx: NamespaceId,
        class_dict: &mut Dict,
    ) -> RunResult<()> {
        let func = self.interns.get_function(func_id);
        let code = &func.code;
        let namespace = self.namespaces.get(namespace_idx);

        for slot_idx in 0..func.namespace_size {
            #[expect(clippy::cast_possible_truncation)]
            if let Some(name_id) = code.local_name(slot_idx as u16) {
                if name_id == StringId::default() {
                    continue;
                }
                let value = namespace.get(NamespaceId::new(slot_idx));
                if matches!(value, Value::Undefined) {
                    continue;
                }
                let key = Value::InternString(name_id);
                let cloned_value = value.clone_with_heap(self.heap);
                if let Some(old) = class_dict.set(key, cloned_value, self.heap, self.interns)? {
                    old.drop_with_heap(self.heap);
                }
            }
        }
        Ok(())
    }

    /// Extends a class definition position to include the first class-body line.
    ///
    /// This matches CPython's traceback formatting for class definition errors,
    /// which prints the `class` line followed by the first body line.
    fn extend_class_def_position(&self, call_position: Option<CodeRange>, func_id: FunctionId) -> Option<CodeRange> {
        let position = call_position?;
        let func = self.interns.get_function(func_id);
        let body_line = func
            .code
            .first_location_after_line(position.start().line)
            .map_or_else(|| position.start().line.saturating_add(1), |range| range.start().line);
        if body_line > position.start().line {
            let end = crate::exception_public::CodeLoc {
                line: body_line,
                column: 1,
            };
            Some(position.with_end(end))
        } else {
            Some(position)
        }
    }

    /// Attaches the class definition position to errors that lack traceback data.
    ///
    /// Used for errors raised during class finalization, which may occur outside
    /// an executing frame and otherwise show an empty traceback.
    fn add_class_def_position(error: RunError, call_position: Option<CodeRange>) -> RunError {
        let Some(position) = call_position else {
            return error;
        };
        match error {
            RunError::Exc(mut exc) => {
                if exc.frame.is_none() {
                    let mut frame = RawStackFrame::from_position(position);
                    frame.hide_caret = true;
                    exc.frame = Some(frame);
                }
                RunError::Exc(exc)
            }
            RunError::UncatchableExc(mut exc) => {
                if exc.frame.is_none() {
                    let mut frame = RawStackFrame::from_position(position);
                    frame.hide_caret = true;
                    exc.frame = Some(frame);
                }
                RunError::UncatchableExc(exc)
            }
            RunError::Internal(_) => error,
        }
    }

    /// Extracts and finalizes `__slots__` for a class.
    ///
    /// This computes the slot layout, instance `__dict__`/`__weakref__` flags,
    /// and installs slot descriptors into the class namespace. Raises a
    /// `ValueError` for slot/class-variable conflicts and a `TypeError` for
    /// invalid `__slots__` declarations.
    fn extract_slots(&mut self, class_heap_id: HeapId) -> RunResult<()> {
        let slots_id: StringId = StaticStrings::DunderSlots.into();
        let slots_str = self.interns.get_str(slots_id);

        let (slots_value, class_name, mro) = match self.heap.get(class_heap_id) {
            HeapData::ClassObject(cls) => (
                cls.namespace()
                    .get_by_str(slots_str, self.heap, self.interns)
                    .map(|v| v.clone_with_heap(self.heap)),
                cls.name(self.interns).to_string(),
                cls.mro().to_vec(),
            ),
            _ => return Err(RunError::internal("extract_slots: not a class object")),
        };

        let class_defines_slots = slots_value.is_some();
        let raw_slots = if let Some(value) = slots_value.as_ref() {
            self.parse_slots_value(value)?
        } else {
            Vec::new()
        };
        if let Some(value) = slots_value {
            value.drop_with_heap(self.heap);
        }

        let mut direct_slots: Vec<(String, crate::types::SlotDescriptorKind)> = Vec::new();
        let mut seen_direct = AHashSet::new();
        let mut dict_slot = false;
        let mut weakref_slot = false;

        for raw in raw_slots {
            let mangled = Self::mangle_slot_name(&class_name, &raw);
            if !seen_direct.insert(mangled.clone()) {
                continue;
            }
            let kind = if mangled == "__dict__" {
                dict_slot = true;
                crate::types::SlotDescriptorKind::Dict
            } else if mangled == "__weakref__" {
                weakref_slot = true;
                crate::types::SlotDescriptorKind::Weakref
            } else {
                crate::types::SlotDescriptorKind::Member
            };
            direct_slots.push((mangled, kind));
        }

        if let HeapData::ClassObject(cls) = self.heap.get(class_heap_id) {
            for (name, kind) in &direct_slots {
                if matches!(kind, crate::types::SlotDescriptorKind::Member)
                    && cls.namespace().get_by_str(name, self.heap, self.interns).is_some()
                {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        format!("'{name}' in __slots__ conflicts with class variable"),
                    )
                    .into());
                }
            }
        }

        let mut slot_layout: Vec<String> = Vec::new();
        let mut slot_indices: AHashMap<String, usize> = AHashMap::new();
        let mut seen_layout = AHashSet::new();
        let mut base_has_dict = false;
        let mut base_has_weakref = false;

        for &base_id in mro.iter().skip(1) {
            if let HeapData::ClassObject(base_cls) = self.heap.get(base_id) {
                if base_cls.instance_has_dict() {
                    base_has_dict = true;
                }
                if base_cls.instance_has_weakref() {
                    base_has_weakref = true;
                }
                for name in base_cls.slot_layout() {
                    if seen_layout.insert(name.clone()) {
                        slot_layout.push(name.clone());
                    }
                }
            }
        }

        for (name, kind) in &direct_slots {
            if matches!(kind, crate::types::SlotDescriptorKind::Member) && seen_layout.insert(name.clone()) {
                slot_layout.push(name.clone());
            }
        }

        for (idx, name) in slot_layout.iter().enumerate() {
            slot_indices.insert(name.clone(), idx);
        }

        let (instance_has_dict, instance_has_weakref) = if class_defines_slots {
            (base_has_dict || dict_slot, base_has_weakref || weakref_slot)
        } else {
            (true, true)
        };

        self.heap.with_entry_mut(class_heap_id, |heap, data| {
            let HeapData::ClassObject(cls) = data else {
                return Err(RunError::internal("extract_slots: not a class object"));
            };

            if class_defines_slots {
                let slot_names: Vec<String> = direct_slots.iter().map(|(name, _)| name.clone()).collect();
                cls.set_slots(slot_names);
            }

            cls.set_slot_layout(
                slot_layout.clone(),
                slot_indices.clone(),
                instance_has_dict,
                instance_has_weakref,
            );

            for (name, kind) in &direct_slots {
                let should_insert = match kind {
                    crate::types::SlotDescriptorKind::Member => true,
                    crate::types::SlotDescriptorKind::Dict | crate::types::SlotDescriptorKind::Weakref => {
                        cls.namespace().get_by_str(name, heap, self.interns).is_none()
                    }
                };

                if !should_insert {
                    continue;
                }

                let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(name.clone())))?;
                let key = Value::Ref(key_id);
                let desc_id = heap.allocate(HeapData::SlotDescriptor(crate::types::SlotDescriptor::new(
                    name.clone(),
                    *kind,
                )))?;
                let value = Value::Ref(desc_id);
                if let Some(old) = cls.set_attr(key, value, heap, self.interns)? {
                    old.drop_with_heap(heap);
                }
            }

            Ok(())
        })?;

        Ok(())
    }

    /// Parses a `__slots__` value into a list of raw slot names.
    fn parse_slots_value(&mut self, slots_val: &Value) -> RunResult<Vec<String>> {
        match slots_val {
            Value::InternString(id) => Ok(vec![self.interns.get_str(*id).to_string()]),
            Value::Ref(id) if matches!(self.heap.get(*id), HeapData::Str(_)) => {
                let HeapData::Str(s) = self.heap.get(*id) else {
                    unreachable!();
                };
                Ok(vec![s.as_str().to_string()])
            }
            _ => {
                let iterable = slots_val.clone_with_heap(self.heap);
                let mut iter = OurosIter::new(iterable, self.heap, self.interns)?;
                let mut out = Vec::new();

                loop {
                    let item = match iter.for_next(self.heap, self.interns) {
                        Ok(Some(item)) => item,
                        Ok(None) => break,
                        Err(e) => {
                            iter.drop_with_heap(self.heap);
                            return Err(e);
                        }
                    };

                    let slot_name = match self.slot_item_to_string(&item) {
                        Ok(name) => {
                            item.drop_with_heap(self.heap);
                            name
                        }
                        Err(e) => {
                            item.drop_with_heap(self.heap);
                            iter.drop_with_heap(self.heap);
                            return Err(e);
                        }
                    };
                    out.push(slot_name);
                }

                iter.drop_with_heap(self.heap);
                Ok(out)
            }
        }
    }

    /// Converts a `__slots__` item to a string or raises a `TypeError`.
    fn slot_item_to_string(&mut self, item: &Value) -> RunResult<String> {
        match item {
            Value::InternString(id) => Ok(self.interns.get_str(*id).to_string()),
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::Str(s) => Ok(s.as_str().to_string()),
                other => Err(ExcType::type_error(format!(
                    "__slots__ items must be strings, not '{}'",
                    other.py_type(self.heap)
                ))),
            },
            other => Err(ExcType::type_error(format!(
                "__slots__ items must be strings, not '{}'",
                other.py_type(self.heap)
            ))),
        }
    }

    /// Applies Python's class name mangling rules to slot names.
    fn mangle_slot_name(class_name: &str, name: &str) -> String {
        if !name.starts_with("__") || name.ends_with("__") {
            return name.to_string();
        }
        let stripped = class_name.trim_start_matches('_');
        if stripped.is_empty() {
            return name.to_string();
        }
        let mut mangled = String::with_capacity(1 + stripped.len() + name.len());
        mangled.push('_');
        mangled.push_str(stripped);
        mangled.push_str(name);
        mangled
    }

    /// Collects descriptors in a class namespace that define `__set_name__`.
    ///
    /// Populates `self.pending_set_name_calls` with `(attr_name, descriptor_id, class_id)`
    /// tuples. These are processed in the main loop after the class value is pushed,
    /// avoiding re-entrancy issues that would occur if we called `self.run()` here.
    fn collect_set_name_descriptors(&mut self, class_heap_id: HeapId) {
        let set_name_id: StringId = StaticStrings::DunderSetName.into();

        if let HeapData::ClassObject(cls) = self.heap.get(class_heap_id) {
            for (key, value) in cls.namespace() {
                if let Value::Ref(desc_id) = value {
                    let desc_id = *desc_id;
                    if let HeapData::Instance(inst) = self.heap.get(desc_id) {
                        let desc_class_id = inst.class_id();
                        let set_name_str = self.interns.get_str(set_name_id);
                        if let HeapData::ClassObject(desc_cls) = self.heap.get(desc_class_id)
                            && desc_cls.mro_has_attr(set_name_str, desc_class_id, self.heap, self.interns)
                            && let Value::InternString(name_id) = key
                        {
                            self.pending_set_name_calls.push((*name_id, desc_id, class_heap_id));
                        }
                    }
                }
            }
        }
    }

    /// Initiates the next pending `__set_name__` call, if any.
    ///
    /// Pops the first entry from `pending_set_name_calls` and calls
    /// `descriptor.__set_name__(owner_class, attr_name)`. If the call pushes
    /// a frame, returns `true` so the main loop knows to wait for the frame to
    /// return. If the call completes synchronously, keeps processing until
    /// all pending calls are done (or one pushes a frame).
    ///
    /// Returns `true` if a frame was pushed (caller should `continue` the main loop),
    /// `false` if all pending calls are done.
    fn process_next_set_name_call(&mut self) -> RunResult<bool> {
        let set_name_id: StringId = StaticStrings::DunderSetName.into();

        while let Some((attr_name_id, desc_id, class_id)) = self.pending_set_name_calls.pop() {
            if let Some(method) = self.lookup_type_dunder(desc_id, set_name_id) {
                // Call __set_name__(self=descriptor, owner=class, name=attr_name)
                self.heap.inc_ref(desc_id);
                self.heap.inc_ref(class_id);
                let name_val = Value::InternString(attr_name_id);
                let args = ArgValues::ArgsKargs {
                    args: vec![Value::Ref(desc_id), Value::Ref(class_id), name_val],
                    kwargs: crate::args::KwargsValues::Empty,
                };
                let result = self.call_function(method, args)?;
                match result {
                    call::CallResult::Push(ret) => {
                        // Synchronous completion -- discard return value, continue to next
                        ret.drop_with_heap(self.heap);
                    }
                    call::CallResult::FramePushed => {
                        // Frame pushed -- main loop will handle the return.
                        // Set flag so the return handler knows to discard the
                        // result and continue with the next __set_name__ call.
                        self.pending_set_name_return = true;
                        return Ok(true);
                    }
                    _ => {} // External/OS calls not expected for __set_name__
                }
            }
        }

        Ok(false)
    }

    /// Collects `__init_subclass__` calls for a newly created class.
    ///
    /// Walks the MRO (skipping the new class itself) to find base classes that
    /// define `__init_subclass__`. Each such base gets a pending call with
    /// `cls=new_class`.
    fn collect_init_subclass_calls(&mut self, new_class_id: HeapId, class_kwargs: Value) {
        let init_subclass_str = "__init_subclass__";

        // Get the bases of the new class
        let bases: Vec<HeapId> = match self.heap.get(new_class_id) {
            HeapData::ClassObject(cls) => cls.bases().to_vec(),
            _ => return,
        };

        // For each direct base, check if it defines __init_subclass__
        for &base_id in &bases {
            let has_init_subclass = match self.heap.get(base_id) {
                HeapData::ClassObject(base_cls) => {
                    base_cls.mro_has_attr(init_subclass_str, base_id, self.heap, self.interns)
                }
                _ => false,
            };
            if has_init_subclass {
                let kwargs_clone = class_kwargs.clone_with_heap(self.heap);
                self.pending_init_subclass_calls
                    .push((base_id, new_class_id, kwargs_clone));
            }
        }

        // Drop the original kwargs now that we've cloned it for pending calls.
        class_kwargs.drop_with_heap(self.heap);
    }

    /// Processes the next pending `__init_subclass__` call.
    ///
    /// Returns `true` if a frame was pushed (caller should `continue` the main loop),
    /// `false` if all pending calls are done.
    fn process_next_init_subclass_call(&mut self) -> RunResult<bool> {
        let init_subclass_id: StringId = StaticStrings::DunderInitSubclass.into();

        while let Some((base_id, new_class_id, kwargs_val)) = self.pending_init_subclass_calls.pop() {
            if let Some(method) = {
                match self.heap.get(base_id) {
                    HeapData::ClassObject(cls) => {
                        let name = self.interns.get_str(init_subclass_id);
                        cls.mro_lookup_attr(name, base_id, self.heap, self.interns)
                            .map(|(v, _)| v)
                    }
                    _ => None,
                }
            } {
                // Call __init_subclass__(cls=new_class)
                self.heap.inc_ref(new_class_id);
                let kwargs = self.clone_kwargs_from_value(&kwargs_val)?;
                let args = ArgValues::ArgsKargs {
                    args: vec![Value::Ref(new_class_id)],
                    kwargs,
                };
                let result = self.call_function(method, args)?;
                kwargs_val.drop_with_heap(self.heap);
                match result {
                    call::CallResult::Push(ret) => {
                        ret.drop_with_heap(self.heap);
                    }
                    call::CallResult::FramePushed => {
                        self.pending_init_subclass_return = true;
                        return Ok(true);
                    }
                    _ => {}
                }
            } else {
                kwargs_val.drop_with_heap(self.heap);
            }
        }

        Ok(false)
    }

    /// Loads a built-in module and pushes it onto the stack.
    fn load_module(&mut self, module_id: u8) -> RunResult<()> {
        let module = BuiltinModule::from_repr(module_id).expect("unknown module id");

        // Create the module on the heap using pre-interned strings
        let heap_id = module.create(self.heap, self.interns)?;
        self.push(Value::Ref(heap_id));
        Ok(())
    }

    /// Resumes execution after an external call completes.
    ///
    /// Pushes the return value onto the stack and continues execution.
    pub fn resume(&mut self, obj: Object) -> Result<FrameExit, RunError> {
        let value = obj
            .to_value(self.heap, self.interns)
            .map_err(|e| SimpleException::new(ExcType::RuntimeError, Some(format!("invalid return type: {e}"))))?;
        self.push(value);
        self.run()
    }

    /// Resumes execution after an external call raised an exception.
    ///
    /// Uses the exception handling mechanism to try to catch the exception.
    /// If caught, continues execution at the handler. If not, propagates the error.
    pub fn resume_with_exception(&mut self, error: RunError) -> Result<FrameExit, RunError> {
        // Use the normal exception handling mechanism
        // handle_exception returns None if caught, Some(error) if not caught
        if let Some(uncaught_error) = self.handle_exception(error) {
            return Err(uncaught_error);
        }
        // Exception was caught, continue execution
        self.run()
    }

    // ========================================================================
    // Stack Operations
    // ========================================================================

    /// Pushes a value onto the operand stack.
    #[inline]
    pub(crate) fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    /// Pops a value from the operand stack.
    #[inline]
    pub(super) fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    /// Peeks at the top of the operand stack without removing it.
    #[inline]
    pub(super) fn peek(&self) -> &Value {
        self.stack.last().expect("stack underflow")
    }

    /// Peeks at a value at the given depth below the top of the operand stack.
    ///
    /// Depth 0 is TOS (equivalent to `peek()`), depth 1 is one below TOS, etc.
    /// For `CallFunction` with N args, the callable sits at depth N (below all args).
    #[inline]
    pub(super) fn peek_at_depth(&self, depth: usize) -> &Value {
        let idx = self.stack.len() - 1 - depth;
        &self.stack[idx]
    }

    /// Pops n values from the stack in reverse order (first popped is last in vec).
    pub(super) fn pop_n(&mut self, n: usize) -> Vec<Value> {
        let start = self.stack.len() - n;
        self.stack.split_off(start)
    }

    // ========================================================================
    // Frame Operations
    // ========================================================================

    /// Returns a reference to the current (topmost) call frame.
    #[inline]
    pub(super) fn current_frame(&self) -> &CallFrame<'a> {
        self.frames.last().expect("no active frame")
    }

    /// Creates a new cached frame from the current frame.
    #[inline]
    pub(super) fn new_cached_frame(&self) -> CachedFrame<'a> {
        self.current_frame().into()
    }

    /// Returns a mutable reference to the current call frame.
    #[inline]
    pub(super) fn current_frame_mut(&mut self) -> &mut CallFrame<'a> {
        self.frames.last_mut().expect("no active frame")
    }

    /// Records a pending `__getattr__` fallback for a `__getattribute__` call.
    ///
    /// Keeps an extra reference to the receiver so it stays alive if we need
    /// to invoke `__getattr__` after an AttributeError.
    fn push_pending_getattr_fallback(&mut self, obj_id: HeapId, name_id: StringId, kind: PendingGetAttrKind) {
        self.heap.inc_ref(obj_id);
        self.pending_getattr_fallback.push(PendingGetAttr {
            obj_id,
            name_id,
            kind,
            frame_depth: self.frames.len(),
        });
    }

    /// Drops a pending `__getattr__` fallback if it matches the given frame depth.
    ///
    /// This releases the extra reference held for the receiver when the
    /// `__getattribute__` frame completes normally.
    fn drop_pending_getattr_for_frame(&mut self, frame_depth: usize) {
        if let Some(pending) = self.pending_getattr_fallback.last()
            && pending.frame_depth == frame_depth
        {
            let pending = self.pending_getattr_fallback.pop().expect("checked pending entry");
            Value::Ref(pending.obj_id).drop_with_heap(self.heap);
        }
    }

    /// Clears all pending `__getattr__` fallbacks, dropping held references.
    ///
    /// Used when tearing down a task to avoid leaking receiver references.
    fn clear_pending_getattr_fallbacks(&mut self) {
        for pending in self.pending_getattr_fallback.drain(..) {
            Value::Ref(pending.obj_id).drop_with_heap(self.heap);
        }
    }

    /// Drops pending binary dunder state for a frame being popped.
    ///
    /// This releases held operand references when a dunder frame exits via
    /// exception unwinding before the protocol completes.
    fn drop_pending_binary_dunder_for_frame(&mut self, frame_depth: usize) {
        if let Some(pending) = self.pending_binary_dunder.last()
            && pending.frame_depth == frame_depth
        {
            let pending = self.pending_binary_dunder.pop().expect("checked pending binary dunder");
            pending.lhs.drop_with_heap(self.heap);
            pending.rhs.drop_with_heap(self.heap);
        }
    }

    /// Clears all pending binary dunder state, dropping held operand references.
    fn clear_pending_binary_dunder(&mut self) {
        for pending in self.pending_binary_dunder.drain(..) {
            pending.lhs.drop_with_heap(self.heap);
            pending.rhs.drop_with_heap(self.heap);
        }
    }

    /// Drops class-body frame metadata with proper refcount handling.
    ///
    /// This is used when a class body frame is being cleaned up early (e.g.,
    /// task switching or exception unwinding) and the class has not been
    /// finalized yet.
    fn drop_class_body_info(&mut self, class_body_info: ClassBodyInfo) {
        for base_id in class_body_info.bases {
            self.heap.dec_ref(base_id);
        }
        class_body_info.metaclass.drop_with_heap(self.heap);
        class_body_info.class_kwargs.drop_with_heap(self.heap);
        if let Some(prepared_value) = class_body_info.prepared_namespace {
            prepared_value.drop_with_heap(self.heap);
        }
        if let Some(orig_id) = class_body_info.orig_bases {
            Value::Ref(orig_id).drop_with_heap(self.heap);
        }
    }

    /// Pops the current frame from the call stack.
    ///
    /// Cleans up the frame's stack region and namespace (except for global namespace).
    pub(super) fn pop_frame(&mut self) {
        let frame_depth = self.frames.len();
        self.drop_pending_getattr_for_frame(frame_depth);
        self.drop_pending_binary_dunder_for_frame(frame_depth);
        self.tracer.on_return(frame_depth.saturating_sub(1));
        let frame = self.frames.pop().expect("no frame to pop");
        if let Some(init_instance) = frame.init_instance {
            init_instance.drop_with_heap(self.heap);
        }
        if let Some(class_body_info) = frame.class_body_info {
            self.drop_class_body_info(class_body_info);
        }
        // Clean up frame's stack region
        while self.stack.len() > frame.stack_base {
            let value = self.stack.pop().unwrap();
            value.drop_with_heap(self.heap);
        }
        // Clean up the namespace (but not the global namespace)
        if frame.namespace_idx != GLOBAL_NS_IDX {
            self.namespaces.drop_with_heap(frame.namespace_idx, self.heap);
        }
    }

    /// Pushes an active exception context for a newly entered `except` handler.
    ///
    /// Callers must push the corresponding exception value onto `self.stack` first.
    pub(super) fn push_exception_context(&mut self, exc_value: Value) {
        let stack_pos = self
            .stack
            .len()
            .checked_sub(1)
            .expect("exception handler entered without stack exception value");
        self.exception_stack.push(exc_value);
        self.exception_stack_positions.push(stack_pos);
    }

    /// Pops the current active exception context, if any.
    pub(super) fn pop_exception_context(&mut self) -> Option<Value> {
        let exc = self.exception_stack.pop()?;
        let _ = self.exception_stack_positions.pop();
        Some(exc)
    }

    /// Cleans up all frames for the current task before switching tasks.
    ///
    /// Used when a task completes or fails and we need to switch to another task.
    /// Properly cleans up each frame's namespace and cell references.
    pub(super) fn cleanup_current_frames(&mut self) {
        self.clear_pending_getattr_fallbacks();
        self.clear_pending_binary_dunder();
        self.pending_stringify_return.clear();
        self.clear_pending_generator_action();
        self.pending_yield_from.clear();
        self.clear_pending_list_build();
        self.clear_pending_list_sort();
        self.clear_pending_min_max();
        self.clear_pending_heapq_select();
        self.clear_pending_bisect();
        self.clear_pending_defaultdict_missing();
        let frames = std::mem::take(&mut self.frames);
        for frame in frames {
            if let Some(init_instance) = frame.init_instance {
                init_instance.drop_with_heap(self.heap);
            }
            if let Some(class_body_info) = frame.class_body_info {
                self.drop_class_body_info(class_body_info);
            }
            // Clean up cell references
            for cell_id in frame.cells {
                self.heap.dec_ref(cell_id);
            }
            // Clean up the namespace (but not the global namespace)
            if frame.namespace_idx != GLOBAL_NS_IDX {
                self.namespaces.drop_with_heap(frame.namespace_idx, self.heap);
            }
        }
    }

    /// Runs garbage collection with proper GC roots.
    ///
    /// GC roots include values in namespaces, the operand stack, and exception stack.
    fn run_gc(&mut self) {
        // Collect roots from all reachable values
        let stack_roots = self.stack.iter().filter_map(Value::ref_id);
        let exc_roots = self.exception_stack.iter().filter_map(Value::ref_id);
        let ns_roots = self.namespaces.iter_heap_ids();

        // Collect all roots into a vec to avoid lifetime issues
        let roots: Vec<HeapId> = stack_roots.chain(exc_roots).chain(ns_roots).collect();

        self.heap.collect_garbage(roots);
    }

    /// Returns the current source position for traceback generation.
    ///
    /// Uses `instruction_ip` which is set at the start of each instruction in the run loop,
    /// ensuring accurate position tracking even when using cached IP for bytecode fetching.
    pub(super) fn current_position(&self) -> CodeRange {
        let frame = self.current_frame();
        // Use instruction_ip which points to the start of the current instruction
        // (set at the beginning of each loop iteration in run())
        frame
            .code
            .location_for_offset(self.instruction_ip)
            .map(crate::bytecode::code::LocationEntry::range)
            .unwrap_or_default()
    }

    // ========================================================================
    // Generator Operations
    // ========================================================================

    /// Clears a pending `next(generator, default)` state, dropping the default value.
    fn clear_pending_next_default(&mut self) {
        if let Some(pending) = self.pending_next_default.take() {
            pending.default.drop_with_heap(self.heap);
        }
    }

    /// Clears pending `defaultdict` missing-key state.
    fn clear_pending_defaultdict_missing(&mut self) {
        if let Some(pending) = self.pending_defaultdict_missing.take() {
            pending.defaultdict.drop_with_heap(self.heap);
            pending.key.drop_with_heap(self.heap);
        }
        self.pending_defaultdict_return = false;
    }

    /// Handles a `default_factory()` return value for `defaultdict[missing_key]`.
    fn handle_defaultdict_missing_return(&mut self, value: Value) -> Result<CallResult, RunError> {
        let Some(pending) = self.pending_defaultdict_missing.take() else {
            value.drop_with_heap(self.heap);
            return Err(RunError::internal("defaultdict return handling missing pending state"));
        };

        let dict_id = match pending.defaultdict {
            Value::Ref(id) => id,
            other => {
                other.drop_with_heap(self.heap);
                pending.key.drop_with_heap(self.heap);
                value.drop_with_heap(self.heap);
                return Err(RunError::internal(
                    "defaultdict pending target was not a heap reference",
                ));
            }
        };

        let key_for_insert = pending.key.clone_with_heap(self.heap);
        let value_for_insert = value.clone_with_heap(self.heap);
        let insert_result = self
            .heap
            .with_entry_mut(dict_id, |heap_inner, data| -> Result<(), RunError> {
                let HeapData::DefaultDict(default_dict) = data else {
                    key_for_insert.drop_with_heap(heap_inner);
                    value_for_insert.drop_with_heap(heap_inner);
                    return Err(RunError::internal("defaultdict pending target was not DefaultDict"));
                };
                default_dict.insert_default(key_for_insert, value_for_insert, heap_inner, self.interns)
            });

        pending.key.drop_with_heap(self.heap);
        Value::Ref(dict_id).drop_with_heap(self.heap);

        if let Err(err) = insert_result {
            value.drop_with_heap(self.heap);
            return Err(err);
        }

        Ok(CallResult::Push(value))
    }

    /// Takes the pending next-default value when it targets `generator_id`.
    fn take_pending_next_default_for(&mut self, generator_id: HeapId) -> Option<Value> {
        if self
            .pending_next_default
            .as_ref()
            .is_some_and(|pending| pending.generator_id == generator_id)
        {
            return self.pending_next_default.take().map(|pending| pending.default);
        }
        None
    }

    /// Clears any pending injected generator action, dropping held exception values.
    fn clear_pending_generator_action(&mut self) {
        if let Some(pending) = self.pending_generator_action.take()
            && let GeneratorAction::Throw(value) = pending.action
        {
            value.drop_with_heap(self.heap);
        }
    }

    /// Clears a pending injected action only when it targets the given generator.
    fn clear_pending_generator_action_for(&mut self, generator_id: HeapId) {
        if self
            .pending_generator_action
            .as_ref()
            .is_some_and(|pending| pending.generator_id == generator_id)
        {
            self.clear_pending_generator_action();
        }
    }

    /// Stores a pending injected action for a resumed generator, replacing any previous action.
    fn set_pending_generator_action(&mut self, generator_id: HeapId, action: GeneratorAction) {
        self.clear_pending_generator_action();
        self.pending_generator_action = Some(PendingGeneratorAction { generator_id, action });
    }

    /// Takes and returns a pending injected action for the current generator, if any.
    fn take_pending_generator_action(&mut self, generator_id: HeapId) -> Option<GeneratorAction> {
        let pending = self.pending_generator_action.take()?;
        if pending.generator_id == generator_id {
            Some(pending.action)
        } else {
            self.pending_generator_action = Some(pending);
            None
        }
    }

    /// Returns true if this value is a delegated iterator shape handled by `yield from`.
    fn is_yield_from_iterator(&self, value: &Value) -> bool {
        matches!(
            value,
            Value::Ref(id) if matches!(self.heap.get(*id), HeapData::Iter(_) | HeapData::Generator(_) | HeapData::Instance(_))
        )
    }

    /// Returns the pending list-build iterator id when it is a generator.
    ///
    /// `pending_list_build_return` is set both for regular `__next__` frames and
    /// VM-managed generator resumes. Nested function returns inside a running
    /// generator must not be mistaken for iterator `__next__` returns.
    fn pending_list_build_generator_id(&self) -> Option<HeapId> {
        let pending = self.pending_list_build.last()?;
        let Value::Ref(iter_id) = pending.iterator else {
            return None;
        };
        if matches!(self.heap.get(iter_id), HeapData::Generator(_)) {
            Some(iter_id)
        } else {
            None
        }
    }

    /// Returns whether a suspended generator should resume by re-executing `YieldFrom`.
    ///
    /// This is used to decide whether `throw()`/`close()` should be injected as
    /// normal exceptions, or delegated to the current `yield from` sub-iterator first.
    fn generator_suspended_at_yield_from(&self, generator_id: HeapId) -> Result<bool, RunError> {
        use crate::types::Generator;

        match self.heap.get(generator_id) {
            HeapData::Generator(Generator { saved_ip, func_id, .. }) => {
                let code = self.interns.get_function(*func_id).code.bytecode();
                if *saved_ip >= code.len() {
                    return Ok(false);
                }
                let opcode = Opcode::try_from(code[*saved_ip])
                    .map_err(|err| RunError::internal(format!("invalid opcode in generator frame: {err}")))?;
                Ok(matches!(opcode, Opcode::YieldFrom))
            }
            _ => Err(RunError::internal("generator method called on non-generator")),
        }
    }

    /// Executes one delegation step for the current `yield from` iterator.
    ///
    /// The iterator value is a cloned handle to the delegated iterator object; it is
    /// dropped before returning on all direct paths (except when ownership is moved into
    /// `call_attr` for generic iterator objects).
    fn yield_from_step(
        &mut self,
        iterator: Value,
        sent_value: Option<Value>,
        action: Option<GeneratorAction>,
    ) -> Result<CallResult, RunError> {
        let Value::Ref(iterator_id) = iterator else {
            if let Some(value) = sent_value {
                value.drop_with_heap(self.heap);
            }
            if let Some(GeneratorAction::Throw(exc_value)) = action {
                return Err(self.make_exception(exc_value, false, false));
            }
            let type_name = iterator.py_type(self.heap);
            iterator.drop_with_heap(self.heap);
            return Err(ExcType::type_error_not_iterable(type_name));
        };

        if matches!(self.heap.get(iterator_id), HeapData::Iter(_)) {
            match action {
                Some(GeneratorAction::Throw(exc_value)) => {
                    if let Some(value) = sent_value {
                        value.drop_with_heap(self.heap);
                    }
                    iterator.drop_with_heap(self.heap);
                    return Err(self.make_exception(exc_value, false, false));
                }
                Some(GeneratorAction::Close) => {
                    if let Some(value) = sent_value {
                        value.drop_with_heap(self.heap);
                    }
                    iterator.drop_with_heap(self.heap);
                    return Ok(CallResult::Push(Value::None));
                }
                None => {
                    if let Some(value) = sent_value {
                        if !matches!(value, Value::None) {
                            value.drop_with_heap(self.heap);
                            iterator.drop_with_heap(self.heap);
                            let type_name = self.heap.get(iterator_id).py_type(self.heap);
                            return Err(ExcType::attribute_error(type_name, "send"));
                        }
                        value.drop_with_heap(self.heap);
                    }
                    let result = match advance_on_heap(self.heap, iterator_id, self.interns)? {
                        Some(value) => Ok(CallResult::Push(value)),
                        None => Err(ExcType::stop_iteration()),
                    };
                    iterator.drop_with_heap(self.heap);
                    return result;
                }
            }
        }

        if matches!(self.heap.get(iterator_id), HeapData::Generator(_)) {
            let result = match action {
                Some(GeneratorAction::Throw(exc_value)) => {
                    if let Some(value) = sent_value {
                        value.drop_with_heap(self.heap);
                    }
                    self.generator_throw(iterator_id, exc_value)
                }
                Some(GeneratorAction::Close) => {
                    if let Some(value) = sent_value {
                        value.drop_with_heap(self.heap);
                    }
                    self.generator_close(iterator_id)
                }
                None => {
                    if let Some(value) = sent_value {
                        if matches!(value, Value::None) {
                            value.drop_with_heap(self.heap);
                            self.generator_next(iterator_id)
                        } else {
                            self.generator_send(iterator_id, value)
                        }
                    } else {
                        self.generator_next(iterator_id)
                    }
                }
            };
            iterator.drop_with_heap(self.heap);
            return result;
        }

        // Generic iterators: delegate through protocol methods.
        match action {
            Some(GeneratorAction::Throw(exc_value)) => {
                if let Some(value) = sent_value {
                    value.drop_with_heap(self.heap);
                }
                iterator.drop_with_heap(self.heap);
                Err(self.make_exception(exc_value, false, false))
            }
            Some(GeneratorAction::Close) => {
                if let Some(value) = sent_value {
                    value.drop_with_heap(self.heap);
                }
                let close_id: StringId = StaticStrings::Close.into();
                match self.call_attr(iterator, close_id, ArgValues::Empty) {
                    Err(RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::AttributeError => {
                        Ok(CallResult::Push(Value::None))
                    }
                    other => other,
                }
            }
            None => {
                let dunder_next: StringId = StaticStrings::DunderNext.into();
                let send_id: StringId = StaticStrings::Send.into();
                if let Some(value) = sent_value {
                    if matches!(value, Value::None) {
                        value.drop_with_heap(self.heap);
                        self.call_attr(iterator, dunder_next, ArgValues::Empty)
                    } else {
                        self.call_attr(iterator, send_id, ArgValues::One(value))
                    }
                } else {
                    self.call_attr(iterator, dunder_next, ArgValues::Empty)
                }
            }
        }
    }

    /// Resumes a generator by pushing its frame onto the call stack.
    ///
    /// This is the core of generator iteration - called by `__next__()` to get the next value.
    /// Handles all generator states:
    /// - `New`: First call - push frame with IP=0
    /// - `Suspended`: Resume - restore saved IP and stack
    /// - `Running`: Error - generator already executing
    /// - `Finished`: Error - raise StopIteration
    pub(in crate::bytecode::vm) fn generator_next(&mut self, generator_id: HeapId) -> Result<CallResult, RunError> {
        use crate::types::{Generator, GeneratorState};

        let state = self.generator_state(generator_id)?;

        match state {
            GeneratorState::Finished => Err(ExcType::stop_iteration()),
            GeneratorState::Running => Err(ExcType::generator_already_executing()),
            GeneratorState::New => {
                self.push_generator_frame(generator_id, GeneratorState::New, None)?;
                Ok(CallResult::FramePushed)
            }
            GeneratorState::Suspended => {
                let resume_past_end = match self.heap.get(generator_id) {
                    HeapData::Generator(Generator { saved_ip, func_id, .. }) => {
                        let code_len = self.interns.get_function(*func_id).code.bytecode().len();
                        *saved_ip >= code_len
                    }
                    _ => return Err(RunError::internal("generator_next called on non-generator")),
                };
                if resume_past_end {
                    if let HeapData::Generator(g) = self.heap.get_mut(generator_id) {
                        g.state = GeneratorState::Finished;
                    }
                    return Err(ExcType::stop_iteration());
                }
                self.push_generator_frame(generator_id, GeneratorState::Suspended, Some(Value::None))?;
                Ok(CallResult::FramePushed)
            }
        }
    }

    /// Resumes a generator with a sent value.
    ///
    /// This powers `gen.send(value)`:
    /// - New + `None`: starts the generator (equivalent to `next(gen)`)
    /// - New + non-`None`: raises `TypeError`
    /// - Suspended: resumes with `value` as the result of the paused `yield` expression
    /// - Running/Finished: raises the standard generator state error
    fn generator_send(&mut self, generator_id: HeapId, value: Value) -> Result<CallResult, RunError> {
        use crate::types::GeneratorState;

        let state = self.generator_state(generator_id)?;
        match state {
            GeneratorState::Finished => {
                value.drop_with_heap(self.heap);
                Err(ExcType::stop_iteration())
            }
            GeneratorState::Running => {
                value.drop_with_heap(self.heap);
                Err(ExcType::generator_already_executing())
            }
            GeneratorState::New => {
                if !matches!(value, Value::None) {
                    value.drop_with_heap(self.heap);
                    return Err(ExcType::generator_send_not_started());
                }
                value.drop_with_heap(self.heap);
                self.push_generator_frame(generator_id, GeneratorState::New, None)?;
                Ok(CallResult::FramePushed)
            }
            GeneratorState::Suspended => {
                self.push_generator_frame(generator_id, GeneratorState::Suspended, Some(value))?;
                Ok(CallResult::FramePushed)
            }
        }
    }

    /// Injects an exception into a generator at its suspension point.
    ///
    /// This powers `gen.throw(exc)`:
    /// - New/Finished: marks the generator finished and raises the exception immediately
    /// - Suspended: resumes the frame and routes the exception through normal handler lookup
    /// - Running: raises `ValueError: generator already executing`
    fn generator_throw(&mut self, generator_id: HeapId, exc_value: Value) -> Result<CallResult, RunError> {
        use crate::types::GeneratorState;

        let state = self.generator_state(generator_id)?;
        match state {
            GeneratorState::Running => {
                exc_value.drop_with_heap(self.heap);
                Err(ExcType::generator_already_executing())
            }
            GeneratorState::New | GeneratorState::Finished => {
                self.mark_generator_finished(generator_id)?;
                Err(self.make_exception(exc_value, false, false))
            }
            GeneratorState::Suspended => {
                if self.generator_suspended_at_yield_from(generator_id)? {
                    self.push_generator_frame(generator_id, GeneratorState::Suspended, None)?;
                    self.set_pending_generator_action(generator_id, GeneratorAction::Throw(exc_value));
                    return Ok(CallResult::FramePushed);
                }
                self.push_generator_frame(generator_id, GeneratorState::Suspended, None)?;
                // Route injected exception through handlers in the resumed generator frame.
                self.instruction_ip = self.current_frame().ip;
                Err(self.make_exception(exc_value, false, false))
            }
        }
    }

    /// Closes a generator by injecting `GeneratorExit`.
    ///
    /// Semantics match CPython:
    /// - New/Finished: mark finished and return `None`
    /// - Running: raise `ValueError: generator already executing`
    /// - Suspended: inject `GeneratorExit` and set close-tracking state
    fn generator_close(&mut self, generator_id: HeapId) -> Result<CallResult, RunError> {
        use crate::types::GeneratorState;

        let state = self.generator_state(generator_id)?;
        match state {
            GeneratorState::New | GeneratorState::Finished => {
                self.mark_generator_finished(generator_id)?;
                Ok(CallResult::Push(Value::None))
            }
            GeneratorState::Running => Err(ExcType::generator_already_executing()),
            GeneratorState::Suspended => {
                if self.generator_suspended_at_yield_from(generator_id)? {
                    self.pending_generator_close = Some(generator_id);
                    self.push_generator_frame(generator_id, GeneratorState::Suspended, None)?;
                    self.set_pending_generator_action(generator_id, GeneratorAction::Close);
                    return Ok(CallResult::FramePushed);
                }
                self.pending_generator_close = Some(generator_id);
                self.push_generator_frame(generator_id, GeneratorState::Suspended, None)?;
                // GeneratorExit should start exception handling from the resumed generator frame.
                self.instruction_ip = self.current_frame().ip;
                Err(SimpleException::new_none(ExcType::GeneratorExit).into())
            }
        }
    }

    /// Returns the current execution state for a heap generator.
    fn generator_state(&self, generator_id: HeapId) -> Result<crate::types::GeneratorState, RunError> {
        use crate::types::Generator;

        match self.heap.get(generator_id) {
            HeapData::Generator(Generator { state, .. }) => Ok(*state),
            _ => Err(RunError::internal("generator method called on non-generator")),
        }
    }

    /// Marks a heap generator as finished.
    fn mark_generator_finished(&mut self, generator_id: HeapId) -> Result<(), RunError> {
        use crate::types::GeneratorState;

        match self.heap.get_mut(generator_id) {
            HeapData::Generator(generator) => {
                generator.state = GeneratorState::Finished;
                Ok(())
            }
            _ => Err(RunError::internal("generator method called on non-generator")),
        }
    }

    /// Converts a generator return value into the `StopIteration` exception raised to callers.
    ///
    /// We store a typed encoding for `.value` while preserving CPython-style `str(exc)`.
    fn stop_iteration_for_generator_return(&mut self, value: Value) -> RunError {
        if matches!(value, Value::None) {
            value.drop_with_heap(self.heap);
            return ExcType::stop_iteration();
        }

        let display = value.py_str(self.heap, self.interns).into_owned();
        let encoded_value = match &value {
            Value::Bool(v) => format!("b:{v}"),
            Value::Int(v) => format!("i:{v}"),
            Value::Float(v) => format!("f:{v}"),
            Value::InternString(id) => format!("s:{}", self.interns.get_str(*id)),
            Value::Ref(id) => match self.heap.get(*id) {
                HeapData::Str(s) => format!("s:{}", s.as_str()),
                _ => format!("s:{display}"),
            },
            _ => format!("s:{display}"),
        };

        value.drop_with_heap(self.heap);
        ExcType::stop_iteration_with_value(display, encoded_value)
    }

    /// Pushes a generator frame onto the call stack.
    ///
    /// **New state (first call):**
    /// 1. Set generator state to `Running`
    /// 2. Move namespace values from Generator into registered Namespace
    /// 3. Push `CallFrame` with `ip: 0`, `stack_base: self.stack.len()`, `generator_id: Some(heap_id)`
    ///
    /// **Suspended state (resume):**
    /// 1. Set generator state to `Running`
    /// 2. Move saved namespace values into registered Namespace
    /// 3. Restore saved stack values onto `self.stack`
    /// 4. Optionally push resume value onto stack (yield expression result for `send()`)
    /// 5. Push `CallFrame` with `ip: saved_ip`, `generator_id: Some(heap_id)`
    fn push_generator_frame(
        &mut self,
        generator_id: HeapId,
        prev_state: crate::types::GeneratorState,
        resume_value: Option<Value>,
    ) -> Result<(), RunError> {
        use crate::types::GeneratorState;

        // Extract data from the generator (we'll modify it later)
        let (func_id, namespace_values, frame_cells, saved_ip, saved_stack) = {
            let HeapData::Generator(generator) = self.heap.get_mut(generator_id) else {
                return Err(RunError::internal("push_generator_frame called on non-generator"));
            };

            // Set state to Running immediately
            generator.state = GeneratorState::Running;

            // Take ownership of the data we need
            let namespace = std::mem::take(&mut generator.namespace);
            let saved_stack = std::mem::take(&mut generator.saved_stack);
            let func_id = generator.func_id;
            let frame_cells = std::mem::take(&mut generator.frame_cells);
            let saved_ip = generator.saved_ip;

            (func_id, namespace, frame_cells, saved_ip, saved_stack)
        };
        // Register the namespace with the Namespaces system
        let namespace_idx = self.namespaces.register_values(namespace_values, self.heap)?;

        // Get function info for the code reference
        let func = self.interns.get_function(func_id);
        let code = &func.code;

        // Determine IP and stack_base based on state
        let (ip, stack_base) = match prev_state {
            GeneratorState::New => {
                // New generators should not carry suspended stack data.
                saved_stack.drop_with_heap(self.heap);
                if let Some(value) = resume_value {
                    value.drop_with_heap(self.heap);
                    return Err(RunError::internal("new generators cannot resume with a value"));
                }
                // First call: start at IP 0, stack base is current stack length
                (0, self.stack.len())
            }
            GeneratorState::Suspended => {
                // Resume: restore saved IP and stack
                // First, restore the saved stack values
                let saved_stack_len = saved_stack.len();
                for value in saved_stack {
                    self.stack.push(value);
                }
                let has_resume_value = resume_value.is_some();
                if let Some(value) = resume_value {
                    self.stack.push(value);
                }
                let stack_len = self.stack.len();
                (saved_ip, stack_len - saved_stack_len - usize::from(has_resume_value))
            }
            _ => {
                saved_stack.drop_with_heap(self.heap);
                resume_value.drop_with_heap(self.heap);
                return Err(RunError::internal("push_generator_frame called with invalid state"));
            }
        };

        // Create and push the frame
        let frame = CallFrame {
            code,
            ip,
            stack_base,
            namespace_idx,
            function_id: Some(func_id),
            cells: frame_cells,
            call_position: None, // Generators don't have a single call position
            class_body_info: None,
            init_instance: None,
            generator_id: Some(generator_id),
        };
        // Hold one strong reference for the lifetime of the active generator frame.
        // Callers may drop their last generator handle immediately after `__next__`/`send`,
        // but suspension/finish paths still need to mutate the heap generator object.
        self.heap.inc_ref(generator_id);
        self.frames.push(frame);
        self.tracer.on_call(
            Some(self.interns.get_str(self.interns.get_function(func_id).name.name_id)),
            self.frames.len(),
        );

        Ok(())
    }

    /// Suspends a generator frame when yield is executed.
    ///
    /// 1. Pop the current frame (the generator frame)
    /// 2. Save the current IP and stack values to the generator
    /// 3. Move namespace values back to the generator
    /// 4. Mark the generator as Suspended and store the suspension line number
    /// 5. Push the yielded value onto the stack for the caller
    fn suspend_generator_frame(&mut self, generator_id: HeapId, yielded_value: Value, suspended_lineno: u16) {
        // Pop the generator frame
        let frame = self.frames.pop().expect("no generator frame to suspend");
        let saved_ip = frame.ip;
        let namespace_idx = frame.namespace_idx;
        let frame_cells = frame.cells;

        // Save any stack values (temporary values during expression evaluation)
        // These are values below the yielded value that was already popped by the Yield opcode
        let saved_stack: Vec<Value> = if self.stack.len() > frame.stack_base {
            self.stack.drain(frame.stack_base..).collect()
        } else {
            Vec::new()
        };

        // Move namespace values back to the generator
        let namespace_values = self.namespaces.take_values(namespace_idx);
        self.namespaces.release_without_drop(namespace_idx);

        // Update the generator state
        if let HeapData::Generator(generator) = self.heap.get_mut(generator_id) {
            generator.saved_ip = saved_ip;
            generator.saved_stack = saved_stack;
            generator.namespace = namespace_values;
            generator.frame_cells = frame_cells;
            generator.saved_lineno = (suspended_lineno != 0).then_some(suspended_lineno);
            generator.state = crate::types::GeneratorState::Suspended;
        }

        if let Some(default_value) = self.take_pending_next_default_for(generator_id) {
            default_value.drop_with_heap(self.heap);
        }

        // Release the frame-owned generator reference acquired in `push_generator_frame`.
        self.heap.dec_ref(generator_id);

        // Push the yielded value onto the stack for the caller
        self.stack.push(yielded_value);
    }

    /// Cleans up a generator frame when an exception propagates out of it.
    ///
    /// Called from exception handling when unwinding through a generator frame.
    /// Unlike `finish_generator_frame` which handles normal returns, this
    /// preserves the exception being propagated while still cleaning up the frame.
    fn cleanup_generator_frame(&mut self, generator_id: HeapId) {
        use crate::types::GeneratorState;

        self.clear_pending_generator_action_for(generator_id);
        self.pending_yield_from
            .retain(|pending| pending.outer_generator_id != generator_id);

        // Pop the generator frame
        let frame = self.frames.pop().expect("no generator frame to cleanup");

        // Drain the frame's stack portion
        while self.stack.len() > frame.stack_base {
            let value = self.stack.pop().unwrap();
            value.drop_with_heap(self.heap);
        }

        // Release the namespace
        if frame.namespace_idx != GLOBAL_NS_IDX {
            self.namespaces.drop_with_heap(frame.namespace_idx, self.heap);
        }
        for cell_id in frame.cells {
            self.heap.dec_ref(cell_id);
        }

        // Mark generator as finished
        if let HeapData::Generator(generator) = self.heap.get_mut(generator_id) {
            generator.state = GeneratorState::Finished;
        }
        // Release the frame-owned generator reference acquired in `push_generator_frame`.
        self.heap.dec_ref(generator_id);
    }

    /// Finishes a generator frame when it returns normally.
    ///
    /// Called when a generator's frame reaches a return statement or falls off the end.
    /// Cleans up the frame, drains its stack portion, releases the namespace,
    /// and sets the generator state to Finished.
    fn finish_generator_frame(&mut self, generator_id: HeapId) {
        use crate::types::GeneratorState;

        self.clear_pending_generator_action_for(generator_id);
        self.pending_yield_from
            .retain(|pending| pending.outer_generator_id != generator_id);

        // Pop the generator frame
        let frame = self.frames.pop().expect("no generator frame to finish");

        // Drain the frame's stack portion
        while self.stack.len() > frame.stack_base {
            let value = self.stack.pop().unwrap();
            value.drop_with_heap(self.heap);
        }

        // Release the namespace (the generator doesn't need namespace values on normal return)
        if frame.namespace_idx != GLOBAL_NS_IDX {
            self.namespaces.drop_with_heap(frame.namespace_idx, self.heap);
        }
        for cell_id in frame.cells {
            self.heap.dec_ref(cell_id);
        }

        // Mark generator as finished
        if let HeapData::Generator(generator) = self.heap.get_mut(generator_id) {
            generator.state = GeneratorState::Finished;
        }
        // Release the frame-owned generator reference acquired in `push_generator_frame`.
        self.heap.dec_ref(generator_id);
    }

    // ========================================================================
    // Variable Operations
    // ========================================================================

    /// Returns a prepared namespace mapping and name for a class body local slot.
    ///
    /// Only returns Some when the current frame is a class body, a prepared
    /// namespace exists, and the slot corresponds to a named local (not an
    /// internal slot or the `__class__` cell).
    fn class_body_prepared_local(&mut self, cached_frame: &CachedFrame<'a>, slot: u16) -> Option<(Value, StringId)> {
        let (prepared_value, name_id, class_cell_slot) = {
            let frame = self.current_frame();
            let class_info = frame.class_body_info.as_ref()?;
            let prepared = class_info.prepared_namespace.as_ref()?;
            let name_id = cached_frame.code.local_name(slot)?;
            if name_id == StringId::default() {
                return None;
            }
            (prepared.copy_for_extend(), name_id, class_info.class_cell_slot)
        };

        if class_cell_slot == Some(NamespaceId::new(slot as usize)) {
            return None;
        }

        if let Value::Ref(id) = prepared_value {
            self.heap.inc_ref(id);
        }

        Some((prepared_value, name_id))
    }

    /// Loads a local variable and returns a CallResult with the value.
    ///
    /// Returns `UnboundLocalError` if this is a true local (assigned somewhere in the function)
    /// or `NameError` if the name doesn't exist in any scope.
    fn load_local(&mut self, cached_frame: &CachedFrame<'a>, slot: u16) -> RunResult<CallResult> {
        if let Some((prepared, name_id)) = self.class_body_prepared_local(cached_frame, slot) {
            let name_str = self.interns.get_str(name_id);
            let result = match prepared {
                Value::Ref(prepared_id) => match self.heap.get(prepared_id) {
                    HeapData::Dict(_) => {
                        let value = self.heap.with_entry_mut(prepared_id, |heap, data| {
                            let HeapData::Dict(dict) = data else {
                                return Err(ExcType::type_error("__prepare__ must return a mapping".to_string()));
                            };
                            Ok(dict
                                .get_by_str(name_str, heap, self.interns)
                                .map(|value| value.clone_with_heap(heap)))
                        })?;
                        match value {
                            Some(value) => Ok(CallResult::Push(value)),
                            None => Err(self.name_error_for_local(slot, Some(name_id))),
                        }
                    }
                    HeapData::Instance(_) => {
                        let dunder_id: StringId = StaticStrings::DunderGetitem.into();
                        if let Some(method) = self.lookup_type_dunder(prepared_id, dunder_id) {
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(prepared_id, method, ArgValues::One(Value::InternString(name_id))) {
                                Ok(result) => Ok(result),
                                Err(RunError::Exc(exc)) if exc.exc.exc_type() == ExcType::KeyError => {
                                    Err(self.name_error_for_local(slot, Some(name_id)))
                                }
                                Err(err) => Err(err),
                            }
                        } else {
                            Err(ExcType::type_error(format!(
                                "'{}' object is not subscriptable",
                                self.heap.get(prepared_id).py_type(self.heap)
                            )))
                        }
                    }
                    _ => Err(RunError::internal("__prepare__ returned invalid mapping")),
                },
                _ => Err(RunError::internal("__prepare__ returned invalid mapping")),
            };

            prepared.drop_with_heap(self.heap);
            return result;
        }

        let namespace = self.namespaces.get(cached_frame.namespace_idx);
        // Copy without incrementing refcount first (avoids borrow conflict)
        let value = namespace.get(NamespaceId::new(slot as usize)).copy_for_extend();

        // Check for undefined value - raise appropriate error based on whether
        // this is a true local (assigned somewhere) or an undefined reference
        if matches!(value, Value::Undefined) {
            let name = cached_frame.code.local_name(slot);
            let err = if cached_frame.code.is_assigned_local(slot) {
                // True local accessed before assignment
                self.unbound_local_error(slot, name)
            } else {
                // Name doesn't exist in any scope
                self.name_error_for_local(slot, name)
            };
            return Err(err);
        }

        // Now we can safely increment refcount and return
        if let Value::Ref(id) = &value {
            self.heap.inc_ref(*id);
        }
        Ok(CallResult::Push(value))
    }

    /// Creates an UnboundLocalError for a local variable accessed before assignment.
    fn unbound_local_error(&self, slot: u16, name: Option<StringId>) -> RunError {
        let name_str = match name {
            Some(id) => self.interns.get_str(id).to_string(),
            None => format!("<local {slot}>"),
        };
        ExcType::unbound_local_error(&name_str).into()
    }

    /// Creates a NameError for an undefined global variable.
    fn name_error(&self, slot: u16, name: Option<StringId>) -> RunError {
        let name_str = match name {
            Some(id) => self.interns.get_str(id).to_string(),
            None => format!("<global {slot}>"),
        };
        ExcType::name_error(&name_str).into()
    }

    /// Creates a NameError for an undefined local variable.
    fn name_error_for_local(&self, slot: u16, name: Option<StringId>) -> RunError {
        let name_str = match name {
            Some(id) => self.interns.get_str(id).to_string(),
            None => format!("<local {slot}>"),
        };
        ExcType::name_error(&name_str).into()
    }

    /// Pops the top of stack and stores it in a local variable.
    fn store_local(&mut self, cached_frame: &CachedFrame<'a>, slot: u16) -> RunResult<CallResult> {
        if let Some((prepared, name_id)) = self.class_body_prepared_local(cached_frame, slot) {
            let value = self.pop();
            let result = match prepared {
                Value::Ref(prepared_id) => match self.heap.get(prepared_id) {
                    HeapData::Dict(_) => {
                        let old = self.heap.with_entry_mut(prepared_id, |heap, data| {
                            let HeapData::Dict(dict) = data else {
                                return Err(ExcType::type_error("__prepare__ must return a mapping".to_string()));
                            };
                            dict.set(Value::InternString(name_id), value, heap, self.interns)
                        })?;
                        if let Some(old) = old {
                            old.drop_with_heap(self.heap);
                        }
                        Ok(CallResult::Push(Value::None))
                    }
                    HeapData::Instance(_) => {
                        let value_for_namespace = value.clone_with_heap(self.heap);
                        let dunder_id: StringId = StaticStrings::DunderSetitem.into();
                        if let Some(method) = self.lookup_type_dunder(prepared_id, dunder_id) {
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(
                                prepared_id,
                                method,
                                ArgValues::Two(Value::InternString(name_id), value),
                            ) {
                                Ok(result) => {
                                    // Mirror into the frame namespace so class finalization can
                                    // still build a dict without calling user code.
                                    let namespace = self.namespaces.get_mut(cached_frame.namespace_idx);
                                    let ns_slot = NamespaceId::new(slot as usize);
                                    let old_value = std::mem::replace(namespace.get_mut(ns_slot), value_for_namespace);
                                    old_value.drop_with_heap(self.heap);
                                    Ok(result)
                                }
                                Err(err) => {
                                    value_for_namespace.drop_with_heap(self.heap);
                                    Err(err)
                                }
                            }
                        } else {
                            value.drop_with_heap(self.heap);
                            value_for_namespace.drop_with_heap(self.heap);
                            Err(ExcType::type_error(format!(
                                "'{}' object does not support item assignment",
                                self.heap.get(prepared_id).py_type(self.heap)
                            )))
                        }
                    }
                    _ => Err(RunError::internal("__prepare__ returned invalid mapping")),
                },
                _ => Err(RunError::internal("__prepare__ returned invalid mapping")),
            };

            prepared.drop_with_heap(self.heap);
            return result;
        }

        let value = self.pop();
        let namespace = self.namespaces.get_mut(cached_frame.namespace_idx);
        let ns_slot = NamespaceId::new(slot as usize);
        let old_value = std::mem::replace(namespace.get_mut(ns_slot), value);
        if let Some(result) = self.maybe_call_destructor_on_last_local_ref(&old_value, cached_frame.ip)? {
            match result {
                CallResult::Push(ret) => {
                    ret.drop_with_heap(self.heap);
                    old_value.drop_with_heap(self.heap);
                    return Ok(CallResult::Push(Value::None));
                }
                CallResult::FramePushed => {
                    old_value.drop_with_heap(self.heap);
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    old_value.drop_with_heap(self.heap);
                    return Err(RunError::internal(format!(
                        "__del__ returned unsupported call result: {other:?}"
                    )));
                }
            }
        }
        old_value.drop_with_heap(self.heap);
        Ok(CallResult::Push(Value::None))
    }

    /// Deletes a local variable (sets it to Undefined).
    fn delete_local(&mut self, cached_frame: &CachedFrame<'a>, slot: u16) -> RunResult<CallResult> {
        if let Some((prepared, name_id)) = self.class_body_prepared_local(cached_frame, slot) {
            let result = match prepared {
                Value::Ref(prepared_id) => match self.heap.get(prepared_id) {
                    HeapData::Dict(_) => {
                        let removed = self.heap.with_entry_mut(prepared_id, |heap, data| {
                            let HeapData::Dict(dict) = data else {
                                return Err(ExcType::type_error("__prepare__ must return a mapping".to_string()));
                            };
                            dict.pop(&Value::InternString(name_id), heap, self.interns)
                        })?;
                        if let Some((old_key, old_value)) = removed {
                            old_key.drop_with_heap(self.heap);
                            old_value.drop_with_heap(self.heap);
                        }
                        Ok(CallResult::Push(Value::None))
                    }
                    HeapData::Instance(_) => {
                        let dunder_id: StringId = StaticStrings::DunderDelitem.into();
                        if let Some(method) = self.lookup_type_dunder(prepared_id, dunder_id) {
                            self.current_frame_mut().ip = cached_frame.ip;
                            match self.call_dunder(prepared_id, method, ArgValues::One(Value::InternString(name_id))) {
                                Ok(result) => {
                                    // Mirror into the frame namespace to keep it in sync.
                                    let namespace = self.namespaces.get_mut(cached_frame.namespace_idx);
                                    let ns_slot = NamespaceId::new(slot as usize);
                                    let old_value = std::mem::replace(namespace.get_mut(ns_slot), Value::Undefined);
                                    old_value.drop_with_heap(self.heap);

                                    Ok(result)
                                }
                                Err(err) => Err(err),
                            }
                        } else {
                            Err(ExcType::type_error(format!(
                                "'{}' object does not support item deletion",
                                self.heap.get(prepared_id).py_type(self.heap)
                            )))
                        }
                    }
                    _ => Err(RunError::internal("__prepare__ returned invalid mapping")),
                },
                _ => Err(RunError::internal("__prepare__ returned invalid mapping")),
            };

            prepared.drop_with_heap(self.heap);
            return result;
        }

        let namespace = self.namespaces.get_mut(cached_frame.namespace_idx);
        let ns_slot = NamespaceId::new(slot as usize);
        let old_value = std::mem::replace(namespace.get_mut(ns_slot), Value::Undefined);
        if let Some(result) = self.maybe_call_destructor_on_last_local_ref(&old_value, cached_frame.ip)? {
            match result {
                CallResult::Push(ret) => {
                    ret.drop_with_heap(self.heap);
                    old_value.drop_with_heap(self.heap);
                    return Ok(CallResult::Push(Value::None));
                }
                CallResult::FramePushed => {
                    old_value.drop_with_heap(self.heap);
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    old_value.drop_with_heap(self.heap);
                    return Err(RunError::internal(format!(
                        "__del__ returned unsupported call result: {other:?}"
                    )));
                }
            }
        }
        old_value.drop_with_heap(self.heap);
        Ok(CallResult::Push(Value::None))
    }

    /// Calls `__del__` for an instance when deleting/rebinding the last local reference.
    ///
    /// This mirrors CPython's common case where `del x` immediately triggers finalization
    /// when `x` was the final strong reference. Errors in `__del__` are intentionally
    /// ignored to match Python's "exception ignored in destructor" behavior.
    fn maybe_call_destructor_on_last_local_ref(
        &mut self,
        value: &Value,
        instruction_ip: usize,
    ) -> RunResult<Option<CallResult>> {
        let Value::Ref(instance_id) = value else {
            return Ok(None);
        };
        if !matches!(self.heap.get(*instance_id), HeapData::Instance(_)) {
            return Ok(None);
        }
        if self.heap.get_refcount(*instance_id) != 1 {
            return Ok(None);
        }

        let class_id = match self.heap.get(*instance_id) {
            HeapData::Instance(inst) => inst.class_id(),
            _ => return Ok(None),
        };
        let Some(method) = (match self.heap.get(class_id) {
            HeapData::ClassObject(cls) => cls
                .mro_lookup_attr("__del__", class_id, self.heap, self.interns)
                .map(|(v, _)| v),
            _ => None,
        }) else {
            return Ok(None);
        };

        self.current_frame_mut().ip = instruction_ip;
        match self.call_dunder(*instance_id, method, ArgValues::Empty) {
            Ok(result) => Ok(Some(result)),
            Err(_) => Ok(None),
        }
    }

    /// Normalizes a `__iter__` return value into an iterator object.
    ///
    /// Python requires `__iter__` to return an iterator. Ouros accepts three
    /// forms here:
    /// - existing iterator/generator objects
    /// - instances implementing `__next__`
    /// - plain iterables, which are wrapped via `OurosIter::new`
    fn normalize_iter_result(&mut self, iter_value: Value) -> RunResult<Value> {
        if let Value::Ref(id) = &iter_value {
            if matches!(self.heap.get(*id), HeapData::Generator(_) | HeapData::Iter(_)) {
                return Ok(iter_value);
            }
            if matches!(self.heap.get(*id), HeapData::Instance(_)) {
                let next_id: StringId = StaticStrings::DunderNext.into();
                if let Some(next_method) = self.lookup_type_dunder(*id, next_id) {
                    next_method.drop_with_heap(self.heap);
                    return Ok(iter_value);
                }
            }
        }

        let iter = OurosIter::new(iter_value, self.heap, self.interns)?;
        let iter_id = self.heap.allocate(HeapData::Iter(iter))?;
        Ok(Value::Ref(iter_id))
    }

    /// Loads a global variable and pushes it onto the stack.
    ///
    /// Returns a NameError if the variable is undefined.
    fn load_global(&mut self, slot: u16) -> RunResult<()> {
        let namespace = self.namespaces.get(GLOBAL_NS_IDX);
        // Copy without incrementing refcount first (avoids borrow conflict)
        let value = namespace
            .get(NamespaceId::new(slot as usize))
            .clone_with_heap(self.heap);

        // Check for undefined value - raise NameError if so
        if matches!(value, Value::Undefined) {
            // For globals, we'd need a global_names table too, but for now use a placeholder
            let name = self.current_frame().code.local_name(slot);
            Err(self.name_error(slot, name))
        } else {
            self.push(value);
            Ok(())
        }
    }

    /// Pops the top of stack and stores it in a global variable.
    fn store_global(&mut self, slot: u16) {
        let value = self.pop();
        let namespace = self.namespaces.get_mut(GLOBAL_NS_IDX);
        let ns_slot = NamespaceId::new(slot as usize);
        let old_value = std::mem::replace(namespace.get_mut(ns_slot), value);
        old_value.drop_with_heap(self.heap);
    }

    /// Loads from a closure cell and pushes onto the stack.
    ///
    /// Returns a NameError if the cell value is undefined (free variable not bound).
    fn load_cell(&mut self, slot: u16) -> RunResult<()> {
        self.tracer.on_cell_load(slot, self.current_frame().cells.len());
        let Some(&cell_id) = self.current_frame().cells.get(slot as usize) else {
            let function_name = self.current_frame().function_id.map_or_else(
                || "<module>".to_string(),
                |id| {
                    self.interns
                        .get_str(self.interns.get_function(id).name.name_id)
                        .to_string()
                },
            );
            return Err(RunError::internal(format!(
                "LoadCell slot {slot} out of bounds for frame '{function_name}' with {} cells",
                self.current_frame().cells.len()
            )));
        };
        // get_cell_value already clones with proper refcount via clone_with_heap
        let value = self.heap.get_cell_value(cell_id)?;

        // Check for undefined value - raise NameError for unbound free variable
        if matches!(value, Value::Undefined) {
            let name = self.current_frame().code.local_name(slot);
            return Err(self.free_var_error(name));
        }

        self.push(value);
        Ok(())
    }

    /// Creates a NameError for an unbound free variable.
    fn free_var_error(&self, name: Option<StringId>) -> RunError {
        let name_str = match name {
            Some(id) => self.interns.get_str(id).to_string(),
            None => "<free var>".to_string(),
        };
        ExcType::name_error_free_variable(&name_str).into()
    }

    /// Pops the top of stack and stores it in a closure cell.
    fn store_cell(&mut self, slot: u16) -> RunResult<()> {
        self.tracer.on_cell_store(slot, self.current_frame().cells.len());
        let value = self.pop();
        let cell_id = self.current_frame().cells[slot as usize];
        self.heap.set_cell_value(cell_id, value)
    }
}

// `heap` is not a public field on VM, so this implementation needs to go here rather than in `heap.rs`
impl<T: ResourceTracker, P: PrintWriter, Tr: VmTracer> ContainsHeap<T> for VM<'_, T, P, Tr> {
    fn heap_mut(&mut self) -> &mut Heap<T> {
        self.heap
    }
}
