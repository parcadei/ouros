use std::{
    fmt,
    time::{Duration, Instant},
};

pub use NO_LIMIT_TRACKER as NoLimitTracker;

use crate::{
    ExcType, Exception,
    exception_private::{ExceptionRaise, RawStackFrame, RunError, SimpleException},
};

/// Threshold in bytes above which `check_large_result` is called.
///
/// Operations that may produce results larger than this threshold (100KB) should call
/// `check_large_result` before performing the operation. This prevents DoS attacks
/// where operations like `2 ** 10_000_000` allocate huge amounts of memory before
/// the allocation check can catch them.
pub const LARGE_RESULT_THRESHOLD: usize = 100_000;

/// Error returned when a resource limit is exceeded during execution.
///
/// This allows the sandbox to enforce strict limits on allocation count,
/// execution time, and memory usage.
#[derive(Debug, Clone)]
pub enum ResourceError {
    /// Maximum number of allocations exceeded.
    Allocation { limit: usize, count: usize },
    /// Maximum instruction operations exceeded.
    Operation { limit: usize, count: usize },
    /// Maximum execution time exceeded.
    Time { limit: Duration, elapsed: Duration },
    /// Maximum memory usage exceeded.
    Memory { limit: usize, used: usize },
    /// Maximum recursion depth exceeded.
    Recursion { limit: usize, depth: usize },
    /// Any other error, e.g. when propagating a python exception
    Exception(Exception),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation { limit, count } => {
                write!(f, "allocation limit exceeded: {count} > {limit}")
            }
            Self::Operation { limit, count } => {
                write!(f, "operation limit exceeded: {count} > {limit}")
            }
            Self::Time { limit, elapsed } => {
                write!(f, "time limit exceeded: {elapsed:?} > {limit:?}")
            }
            Self::Memory { limit, used } => {
                write!(f, "memory limit exceeded: {used} bytes > {limit} bytes")
            }
            Self::Recursion { .. } => {
                write!(f, "maximum recursion depth exceeded")
            }
            Self::Exception(exc) => {
                write!(f, "{exc}")
            }
        }
    }
}

impl std::error::Error for ResourceError {}

impl ResourceError {
    /// Converts this resource error to a Python exception with optional stack frame.
    ///
    /// Maps resource error types to Python exception types:
    /// - `Allocation` → `MemoryError`
    /// - `Memory` → `MemoryError`
    /// - `Operation` → `TimeoutError`
    /// - `Time` → `TimeoutError`
    /// - `Recursion` → `RecursionError`
    #[must_use]
    pub(crate) fn into_exception(self, frame: Option<RawStackFrame>) -> ExceptionRaise {
        let (exc_type, msg) = match self {
            Self::Allocation { limit, count } => (
                ExcType::MemoryError,
                Some(format!("allocation limit exceeded: {count} > {limit}")),
            ),
            Self::Operation { limit, count } => (
                ExcType::TimeoutError,
                Some(format!("operation limit exceeded: {count} > {limit}")),
            ),
            Self::Memory { limit, used } => (
                ExcType::MemoryError,
                Some(format!("memory limit exceeded: {used} bytes > {limit} bytes")),
            ),
            Self::Time { limit, elapsed } => (
                ExcType::TimeoutError,
                Some(format!("time limit exceeded: {elapsed:?} > {limit:?}")),
            ),
            Self::Recursion { .. } => (
                ExcType::RecursionError,
                Some("maximum recursion depth exceeded".to_string()),
            ),
            Self::Exception(exc) => (exc.exc_type(), exc.into_message()),
        };
        let exc = SimpleException::new(exc_type, msg);
        match frame {
            Some(f) => exc.with_frame(f),
            None => exc.into(),
        }
    }
}

impl From<ResourceError> for RunError {
    fn from(err: ResourceError) -> Self {
        // RecursionError is catchable in CPython (unlike MemoryError/TimeoutError
        // which are also catchable in CPython but uncatchable in Ouros for sandbox safety).
        // Making RecursionError catchable is important for CPython parity:
        // `try: f() except RecursionError: ...` must work.
        if matches!(err, ResourceError::Recursion { .. }) {
            Self::Exc(Box::new(err.into_exception(None)))
        } else {
            Self::UncatchableExc(Box::new(err.into_exception(None)))
        }
    }
}

/// Trait for tracking resource usage and scheduling garbage collection.
///
/// Implementations can enforce limits on allocations, time, and memory,
/// as well as schedule periodic garbage collection.
///
/// All implementations should eventually trigger garbage collection to handle
/// reference cycles. The `should_gc` method controls *frequency*, not whether
/// GC runs at all.
pub trait ResourceTracker: fmt::Debug {
    /// Called before each heap allocation.
    ///
    /// Returns `Ok(())` if the allocation should proceed, or `Err(ResourceError)`
    /// if a limit would be exceeded.
    ///
    /// # Arguments
    /// * `size` - Approximate size in bytes of the allocation
    fn on_allocate(&mut self, get_size: impl FnOnce() -> usize) -> Result<(), ResourceError>;

    /// Called before inserting an item into an existing container.
    ///
    /// Unlike [`Self::on_allocate`], this does not represent creation of a new
    /// heap object. It is used for VM opcodes like `LIST_APPEND`, `SET_ADD`,
    /// and `DICT_SETITEM` where container growth should still count against the
    /// allocation budget (`max_allocations`) to avoid unbounded in-place growth.
    ///
    /// The default implementation routes through `on_allocate` with a zero-byte
    /// estimate for tracker implementations that don't need a dedicated fast path.
    fn on_container_insert(&mut self) -> Result<(), ResourceError> {
        self.on_allocate(|| 0)
    }

    /// Called when memory is freed (during dec_ref or garbage collection).
    ///
    /// # Arguments
    /// * `size` - Size in bytes of the freed allocation
    fn on_free(&mut self, get_size: impl FnOnce() -> usize);

    /// Called periodically (at statement boundaries) to check time limits.
    ///
    /// Returns `Ok(())` if within configured execution limits, or a
    /// `ResourceError` if a limit is exceeded (for example `Time` or `Operation`).
    fn check_time(&mut self) -> Result<(), ResourceError>;

    /// Called before pushing a new call frame to check recursion depth.
    ///
    /// Returns `Ok(())` if within recursion limit, or `Err(ResourceError::Recursion)`
    /// if the limit would be exceeded.
    ///
    /// # Arguments
    /// * `current_depth` - Current call stack depth (before the new frame is pushed)
    fn check_recursion_depth(&self, current_depth: usize) -> Result<(), ResourceError>;

    /// Called before operations that may produce large results (>100KB).
    ///
    /// This allows pre-emptive rejection of operations like `2 ** 10_000_000`
    /// before the memory is actually allocated. The check only happens for
    /// estimated result sizes above `LARGE_RESULT_THRESHOLD` to avoid overhead
    /// on small operations.
    ///
    /// # Arguments
    /// * `estimated_bytes` - Approximate size of the result in bytes
    ///
    /// Returns `Ok(())` to allow the operation, or `Err(ResourceError)` to reject.
    fn check_large_result(&self, estimated_bytes: usize) -> Result<(), ResourceError>;

    /// Returns the total number of allocations tracked, if this tracker records them.
    ///
    /// `LimitedTracker` returns `Some(count)`; `NoLimitTracker` returns `None`.
    fn allocation_count(&self) -> Option<usize> {
        None
    }

    /// Returns the current approximate memory usage in bytes, if tracked.
    ///
    /// `LimitedTracker` returns `Some(bytes)`; `NoLimitTracker` returns `None`.
    fn current_memory_bytes(&self) -> Option<usize> {
        None
    }
}

/// A resource tracker used for long-lived heaps with optional soft limits.
///
/// By default this behaves like an unrestricted tracker (except the default
/// recursion limit). REPL hosts can opt into limits via [`NoLimitTracker::with_limits`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct NoLimitTracker {
    /// Optional limits applied by long-lived REPL sessions.
    #[serde(default)]
    limits: ResourceLimits,
    /// Number of VM operations executed in the current REPL step.
    #[serde(default)]
    operation_count: usize,
    /// Total number of allocations made by the session heap.
    #[serde(default)]
    allocation_count: usize,
    /// Current approximate heap memory usage in bytes.
    #[serde(default)]
    current_memory: usize,
    /// Optional per-execution deadline used by REPL `execute_with_limits`.
    ///
    /// This field is intentionally excluded from serialization so deserialized
    /// trackers resume with no active deadline.
    #[serde(skip)]
    deadline: Option<Instant>,
    /// Original duration associated with the current deadline.
    ///
    /// Used to report meaningful `ResourceError::Time` details when the deadline
    /// is exceeded.
    #[serde(skip)]
    deadline_limit: Option<Duration>,
}

impl NoLimitTracker {
    /// Creates a tracker with no active deadline.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            limits: ResourceLimits {
                max_operations: None,
                max_allocations: None,
                max_duration: None,
                max_memory: None,
                gc_interval: None,
                max_recursion_depth: None,
            },
            operation_count: 0,
            allocation_count: 0,
            current_memory: 0,
            deadline: None,
            deadline_limit: None,
        }
    }

    /// Creates a tracker with persistent resource limits for REPL execution.
    #[must_use]
    pub const fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            limits,
            operation_count: 0,
            allocation_count: 0,
            current_memory: 0,
            deadline: None,
            deadline_limit: None,
        }
    }

    /// Sets or clears the active execution deadline.
    ///
    /// REPL execution uses this to apply per-call timeouts while keeping a
    /// persistent `Heap<NoLimitTracker>` across lines.
    pub fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
        self.deadline_limit = deadline.map(|value| value.saturating_duration_since(Instant::now()));
    }

    /// Starts one bounded REPL execution step.
    ///
    /// This resets the per-step operation counter and configures either an
    /// explicit deadline or a deadline derived from `limits.max_duration`.
    pub fn begin_execution(&mut self, deadline: Option<Instant>) {
        self.operation_count = 0;
        if let Some(deadline) = deadline {
            self.set_deadline(Some(deadline));
            return;
        }
        if let Some(max_duration) = self.limits.max_duration {
            self.deadline = Some(Instant::now() + max_duration);
            self.deadline_limit = Some(max_duration);
        } else {
            self.set_deadline(None);
        }
    }
}

/// Shared value form of [`NoLimitTracker`] for expression-context compatibility.
///
/// Existing call sites and tests frequently pass `NoLimitTracker` as a value
/// expression. This constant preserves that ergonomic usage while the type now
/// carries internal deadline state.
pub const NO_LIMIT_TRACKER: NoLimitTracker = NoLimitTracker::new();

impl ResourceTracker for NoLimitTracker {
    #[inline]
    fn on_allocate(&mut self, get_size: impl FnOnce() -> usize) -> Result<(), ResourceError> {
        let tracks_allocations = self.limits.max_allocations.is_some();
        let tracks_memory = self.limits.max_memory.is_some();
        if !tracks_allocations && !tracks_memory {
            return Ok(());
        }

        if let Some(max) = self.limits.max_allocations
            && self.allocation_count >= max
        {
            return Err(ResourceError::Allocation {
                limit: max,
                count: self.allocation_count + 1,
            });
        }

        if let Some(max) = self.limits.max_memory {
            let size = get_size();
            let new_memory = self.current_memory + size;
            if new_memory > max {
                return Err(ResourceError::Memory {
                    limit: max,
                    used: new_memory,
                });
            }
            self.current_memory = new_memory;
        }

        if tracks_allocations {
            self.allocation_count += 1;
        }

        Ok(())
    }

    #[inline]
    fn on_container_insert(&mut self) -> Result<(), ResourceError> {
        if let Some(max) = self.limits.max_allocations
            && self.allocation_count >= max
        {
            return Err(ResourceError::Allocation {
                limit: max,
                count: self.allocation_count + 1,
            });
        }

        if self.limits.max_allocations.is_some() {
            self.allocation_count += 1;
        }
        Ok(())
    }

    #[inline]
    fn on_free(&mut self, get_size: impl FnOnce() -> usize) {
        if self.limits.max_memory.is_some() {
            self.current_memory = self.current_memory.saturating_sub(get_size());
        }
    }

    #[inline]
    fn check_time(&mut self) -> Result<(), ResourceError> {
        if let Some(max) = self.limits.max_operations {
            self.operation_count += 1;
            if self.operation_count > max {
                return Err(ResourceError::Operation {
                    limit: max,
                    count: self.operation_count,
                });
            }
        }

        if let Some(limit) = self.deadline {
            let now = Instant::now();
            if now >= limit {
                let configured_limit = self.deadline_limit.unwrap_or_default();
                return Err(ResourceError::Time {
                    limit: configured_limit,
                    elapsed: configured_limit.saturating_add(now.duration_since(limit)),
                });
            }
        }
        Ok(())
    }

    /// Enforces recursion depth using configured limits or the default depth.
    ///
    /// The default value of 1000 matches CPython behavior.
    #[inline]
    fn check_recursion_depth(&self, current_depth: usize) -> Result<(), ResourceError> {
        let max_recursion_limit = self.limits.max_recursion_depth.unwrap_or(DEFAULT_MAX_RECURSION_DEPTH);
        if current_depth >= max_recursion_limit {
            Err(ResourceError::Recursion {
                limit: max_recursion_limit,
                depth: current_depth + 1,
            })
        } else {
            Ok(())
        }
    }

    #[inline]
    fn check_large_result(&self, estimated_bytes: usize) -> Result<(), ResourceError> {
        if let Some(max) = self.limits.max_memory {
            let new_memory = self.current_memory.saturating_add(estimated_bytes);
            if new_memory > max {
                return Err(ResourceError::Memory {
                    limit: max,
                    used: new_memory,
                });
            }
        }
        Ok(())
    }
}

/// Configuration for resource limits.
///
/// All limits are optional - set to `None` to disable a specific limit.
/// Use `ResourceLimits::default()` for no limits, or build custom limits
/// with the builder pattern.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResourceLimits {
    /// Maximum number of VM operations (instructions) per execution step.
    pub max_operations: Option<usize>,
    /// Maximum number of heap allocations allowed.
    pub max_allocations: Option<usize>,
    /// Maximum execution time.
    pub max_duration: Option<Duration>,
    /// Maximum heap memory in bytes (approximate).
    pub max_memory: Option<usize>,
    /// Run garbage collection every N allocations.
    pub gc_interval: Option<usize>,
    /// Maximum recursion depth (function call stack depth).
    pub max_recursion_depth: Option<usize>,
}

/// Recommended maximum recursion depth if not otherwise specified.
pub const DEFAULT_MAX_RECURSION_DEPTH: usize = 1000;

/// Maximum recursion depth for data structure operations (repr, eq, hash, etc.).
///
/// Separate from the function call stack limit. This protects against stack overflow
/// when traversing deeply nested structures like `a = []; for _ in range(1000): a = [a]`.
///
/// Lower in debug mode to avoid stack overflow (debug builds use more stack space
/// per call frame).
#[cfg(debug_assertions)]
pub const MAX_DATA_RECURSION_DEPTH: u16 = 100;

/// Maximum recursion depth for data structure operations (repr, eq, hash, etc.).
///
/// Separate from the function call stack limit. This protects against stack overflow
/// when traversing deeply nested structures.
#[cfg(not(debug_assertions))]
pub const MAX_DATA_RECURSION_DEPTH: u16 = 500;

/// Maximum length of the Method Resolution Order (MRO) list for any class.
///
/// Limits the output of C3 linearization to prevent diamond-inheritance explosions
/// from consuming excessive memory or CPU. A limit of 2600 is generous enough for
/// any practical class hierarchy while still preventing adversarial abuse.
///
/// Consumed during Phase 2's C3 linearization implementation.
pub const MAX_MRO_LENGTH: usize = 2600;

/// Maximum depth of single-path inheritance chains.
///
/// Prevents deep inheritance hierarchies (e.g., 10000 levels) from causing stack
/// overflow during MRO computation or excessive memory use. A limit of 1000 matches
/// the default recursion limit and is sufficient for any practical class hierarchy.
///
/// Consumed during Phase 2's C3 linearization implementation.
pub const MAX_INHERITANCE_DEPTH: usize = 1000;

impl ResourceLimits {
    /// Creates a new ResourceLimits with all limits disabled, except max recursion which is set to 1000.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_recursion_depth: Some(1000),
            ..Default::default()
        }
    }

    /// Sets the maximum number of allocations.
    #[must_use]
    pub fn max_allocations(mut self, limit: usize) -> Self {
        self.max_allocations = Some(limit);
        self
    }

    /// Sets the maximum number of VM operations (instructions) per execution step.
    #[must_use]
    pub fn max_operations(mut self, limit: usize) -> Self {
        self.max_operations = Some(limit);
        self
    }

    /// Sets the maximum execution duration.
    #[must_use]
    pub fn max_duration(mut self, limit: Duration) -> Self {
        self.max_duration = Some(limit);
        self
    }

    /// Sets the maximum memory usage in bytes.
    #[must_use]
    pub fn max_memory(mut self, limit: usize) -> Self {
        self.max_memory = Some(limit);
        self
    }

    /// Sets the garbage collection interval (run GC every N allocations).
    #[must_use]
    pub fn gc_interval(mut self, interval: usize) -> Self {
        self.gc_interval = Some(interval);
        self
    }

    /// Sets the maximum recursion depth (function call stack depth).
    #[must_use]
    pub fn max_recursion_depth(mut self, limit: Option<usize>) -> Self {
        self.max_recursion_depth = limit;
        self
    }
}

/// A resource tracker that enforces configurable limits.
///
/// Tracks allocation count, memory usage, and execution time, returning
/// errors when limits are exceeded. Also schedules garbage collection
/// at configurable intervals.
///
/// When serialized/deserialized, the `start_time` is reset to `Instant::now()`.
/// This means time limits restart from zero after deserialization.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LimitedTracker {
    limits: ResourceLimits,
    /// When execution started (for time limit checking).
    /// Reset to `Instant::now()` on deserialization.
    #[serde(skip, default = "Instant::now")]
    start_time: Instant,
    /// Total number of allocations made.
    allocation_count: usize,
    /// Number of VM operations executed.
    #[serde(default)]
    operation_count: usize,
    /// Current approximate memory usage in bytes.
    current_memory: usize,
}

impl LimitedTracker {
    /// Creates a new LimitedTracker with the given limits.
    ///
    /// The start time is recorded when the tracker is created, so create
    /// it immediately before starting execution.
    #[must_use]
    pub fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            start_time: Instant::now(),
            allocation_count: 0,
            operation_count: 0,
            current_memory: 0,
        }
    }

    /// Returns the current allocation count.
    #[must_use]
    pub fn allocation_count(&self) -> usize {
        self.allocation_count
    }

    /// Returns the current approximate memory usage.
    #[must_use]
    pub fn current_memory(&self) -> usize {
        self.current_memory
    }

    /// Returns the elapsed time since tracker creation.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Returns the configured maximum execution duration, if any.
    #[must_use]
    pub fn max_duration(&self) -> Option<Duration> {
        self.limits.max_duration
    }
}

impl ResourceTracker for LimitedTracker {
    fn on_allocate(&mut self, get_size: impl FnOnce() -> usize) -> Result<(), ResourceError> {
        // Check allocation count limit
        if let Some(max) = self.limits.max_allocations
            && self.allocation_count >= max
        {
            return Err(ResourceError::Allocation {
                limit: max,
                count: self.allocation_count + 1,
            });
        }

        let size = get_size();
        // Check memory limit
        if let Some(max) = self.limits.max_memory {
            let new_memory = self.current_memory + size;
            if new_memory > max {
                return Err(ResourceError::Memory {
                    limit: max,
                    used: new_memory,
                });
            }
        }

        // Update tracking state
        self.allocation_count += 1;
        self.current_memory += size;

        Ok(())
    }

    fn on_container_insert(&mut self) -> Result<(), ResourceError> {
        if let Some(max) = self.limits.max_allocations
            && self.allocation_count >= max
        {
            return Err(ResourceError::Allocation {
                limit: max,
                count: self.allocation_count + 1,
            });
        }

        self.allocation_count += 1;
        Ok(())
    }

    fn on_free(&mut self, get_size: impl FnOnce() -> usize) {
        self.current_memory = self.current_memory.saturating_sub(get_size());
    }

    fn check_time(&mut self) -> Result<(), ResourceError> {
        if let Some(max) = self.limits.max_operations {
            self.operation_count += 1;
            if self.operation_count > max {
                return Err(ResourceError::Operation {
                    limit: max,
                    count: self.operation_count,
                });
            }
        }

        if let Some(max) = self.limits.max_duration {
            let elapsed = self.start_time.elapsed();
            if elapsed > max {
                return Err(ResourceError::Time { limit: max, elapsed });
            }
        }
        Ok(())
    }

    fn check_recursion_depth(&self, current_depth: usize) -> Result<(), ResourceError> {
        if let Some(max) = self.limits.max_recursion_depth {
            // current_depth is before push, so new depth would be current_depth + 1
            if current_depth >= max {
                return Err(ResourceError::Recursion {
                    limit: max,
                    depth: current_depth + 1,
                });
            }
        }
        Ok(())
    }

    fn check_large_result(&self, estimated_bytes: usize) -> Result<(), ResourceError> {
        // Check if this would exceed memory limit
        if let Some(max) = self.limits.max_memory {
            let new_memory = self.current_memory.saturating_add(estimated_bytes);
            if new_memory > max {
                return Err(ResourceError::Memory {
                    limit: max,
                    used: new_memory,
                });
            }
        }
        Ok(())
    }

    fn allocation_count(&self) -> Option<usize> {
        Some(self.allocation_count)
    }

    fn current_memory_bytes(&self) -> Option<usize> {
        Some(self.current_memory)
    }
}
