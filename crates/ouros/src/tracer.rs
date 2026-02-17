//! VM execution tracing infrastructure.
//!
//! Provides a trait-based tracing system for the bytecode VM with zero-cost abstraction.
//! When using [`NoopTracer`], all trace methods compile away entirely via monomorphization —
//! identical to how [`NoLimitTracker`](crate::resource::NoLimitTracker) eliminates resource
//! checking overhead in production.
//!
//! # Architecture
//!
//! The [`VmTracer`] trait defines hook points at key execution events (instruction dispatch,
//! function calls/returns, closure cell access, etc.). Concrete implementations collect
//! different kinds of data:
//!
//! | Tracer | Purpose |
//! |--------|---------|
//! | [`NoopTracer`] | Zero-cost no-op (production default) |
//! | [`StderrTracer`] | Human-readable execution log to stderr |
//! | [`ProfilingTracer`] | Opcode frequency counters and call depth tracking |
//! | [`CoverageTracer`] | Instruction pointer coverage (which bytecodes executed) |
//! | [`RecordingTracer`] | Full event recording for deterministic replay or post-mortem |
//!
//! # Usage
//!
//! The VM is parameterized as `VM<'a, T: ResourceTracker, P: PrintWriter, Tr: VmTracer>`.
//! Callers choose the tracer at construction time:
//!
//! ```ignore
//! // Production (zero overhead):
//! let mut vm = VM::new(&mut heap, &mut namespaces, &interns, &mut print, NoopTracer);
//!
//! // Debugging:
//! let mut vm = VM::new(&mut heap, &mut namespaces, &interns, &mut print, StderrTracer::new());
//!
//! // Profiling:
//! let mut tracer = ProfilingTracer::new();
//! let mut vm = VM::new(&mut heap, &mut namespaces, &interns, &mut print, tracer);
//! // ... run ...
//! let report = vm.tracer().report();
//! ```

use std::collections::HashMap;

use crate::bytecode::Opcode;

/// Trace event emitted during VM execution.
///
/// Used by [`RecordingTracer`] to capture a full execution trace for
/// deterministic replay or post-mortem analysis.
#[derive(Debug, Clone)]
pub enum TraceEvent {
    /// An opcode was dispatched at the given IP.
    Instruction {
        /// Instruction pointer (byte offset in the code object's bytecode).
        ip: usize,
        /// The opcode that was executed.
        opcode: Opcode,
        /// Stack depth at the time of dispatch (relative to frame base).
        stack_depth: usize,
    },
    /// A function call pushed a new frame.
    Call {
        /// Function name (if available from interns).
        func_name: Option<String>,
        /// Call stack depth after the push.
        depth: usize,
    },
    /// A function return popped a frame.
    Return {
        /// Call stack depth after the pop.
        depth: usize,
    },
    /// A closure cell was read (LoadCell opcode).
    CellLoad {
        /// Cell slot index in the frame's cells vector.
        slot: u16,
        /// Total number of cells in the current frame.
        cells_len: usize,
    },
    /// A closure cell was written (StoreCell opcode).
    CellStore {
        /// Cell slot index in the frame's cells vector.
        slot: u16,
        /// Total number of cells in the current frame.
        cells_len: usize,
    },
    /// A function object was created (MakeFunction / MakeClosure opcode).
    MakeFunction {
        /// Number of captured closure cells (0 for non-closure functions).
        cell_count: usize,
        /// Number of default argument values.
        defaults_count: usize,
    },
}

/// Trait for VM execution tracing.
///
/// All methods have default no-op implementations, so [`NoopTracer`] requires
/// zero lines of code and compiles to zero instructions. Implementations only
/// override the hooks they care about.
///
/// The trait is designed for monomorphization: the VM carries the tracer as a
/// type parameter `Tr: VmTracer`, so the compiler can inline and eliminate
/// no-op calls at compile time (identical to `ResourceTracker`).
pub trait VmTracer: std::fmt::Debug {
    /// Called before each opcode dispatch in the main execution loop.
    ///
    /// This is the hottest hook — called for every single bytecode instruction.
    /// Implementations should be as lightweight as possible.
    ///
    /// # Arguments
    /// * `ip` - Byte offset of the opcode in the code object's bytecode
    /// * `opcode` - The opcode about to be executed
    /// * `stack_depth` - Number of values on the operand stack (relative to frame base)
    /// * `frame_depth` - Number of frames on the call stack
    #[inline(always)]
    fn on_instruction(&mut self, _ip: usize, _opcode: Opcode, _stack_depth: usize, _frame_depth: usize) {}

    /// Called when a new call frame is pushed (function call, class body, etc.).
    ///
    /// # Arguments
    /// * `func_name` - Function name if available (None for module-level or class bodies)
    /// * `depth` - Call stack depth after the push
    #[inline(always)]
    fn on_call(&mut self, _func_name: Option<&str>, _depth: usize) {}

    /// Called when a call frame is popped (function return).
    ///
    /// # Arguments
    /// * `depth` - Call stack depth after the pop
    #[inline(always)]
    fn on_return(&mut self, _depth: usize) {}

    /// Called when a closure cell is read (LoadCell opcode).
    ///
    /// Useful for debugging closure variable capture issues (e.g., generator
    /// expressions not seeing enclosing scope variables).
    ///
    /// # Arguments
    /// * `slot` - Cell slot index
    /// * `cells_len` - Total number of cells in the current frame
    #[inline(always)]
    fn on_cell_load(&mut self, _slot: u16, _cells_len: usize) {}

    /// Called when a closure cell is written (StoreCell opcode).
    ///
    /// # Arguments
    /// * `slot` - Cell slot index
    /// * `cells_len` - Total number of cells in the current frame
    #[inline(always)]
    fn on_cell_store(&mut self, _slot: u16, _cells_len: usize) {}

    /// Called when a function or closure object is created.
    ///
    /// # Arguments
    /// * `cell_count` - Number of captured closure cells (0 for MakeFunction)
    /// * `defaults_count` - Number of default argument values
    #[inline(always)]
    fn on_make_function(&mut self, _cell_count: usize, _defaults_count: usize) {}

    /// Called when a new exception handler is entered.
    ///
    /// # Arguments
    /// * `depth` - Exception stack depth after the push
    #[inline(always)]
    fn on_exception_push(&mut self, _depth: usize) {}

    /// Called when an exception handler is exited.
    ///
    /// # Arguments
    /// * `depth` - Exception stack depth after the pop
    #[inline(always)]
    fn on_exception_pop(&mut self, _depth: usize) {}
}

// ============================================================================
// NoopTracer — zero-cost production default
// ============================================================================

/// A tracer that does nothing.
///
/// All trait methods use the default no-op implementations. Because the VM
/// carries the tracer as a type parameter, the compiler monomorphizes
/// `VM<..., NoopTracer>` and inlines every hook to nothing — zero runtime cost.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTracer;

impl VmTracer for NoopTracer {}

// ============================================================================
// StderrTracer — human-readable execution log
// ============================================================================

/// Tracer that prints a human-readable execution log to stderr.
///
/// Output format:
/// ```text
/// [    0] LoadConst         stack=0  frames=1
/// [    3] StoreLocal        stack=1  frames=1
/// [    5] LoadConst         stack=0  frames=1
///   >>> CALL foo            depth=2
/// [    0] LoadLocal0        stack=0  frames=2
///   <<< RETURN              depth=1
/// ```
///
/// Useful for interactive debugging — pipe stderr to a file while stdout
/// shows normal program output.
#[derive(Debug)]
pub struct StderrTracer {
    /// Maximum number of instructions to trace before stopping (prevents
    /// runaway output on loops). None = unlimited.
    limit: Option<usize>,
    /// Number of instructions traced so far.
    count: usize,
    /// Whether we've stopped tracing (hit the limit).
    stopped: bool,
}

impl StderrTracer {
    /// Creates a new stderr tracer with no instruction limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            limit: None,
            count: 0,
            stopped: false,
        }
    }

    /// Creates a new stderr tracer that stops after `limit` instructions.
    ///
    /// After the limit is reached, no further output is produced. Useful for
    /// tracing just the beginning of execution without overwhelming output.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            count: 0,
            stopped: false,
        }
    }
}

impl Default for StderrTracer {
    fn default() -> Self {
        Self::new()
    }
}

impl VmTracer for StderrTracer {
    #[inline]
    fn on_instruction(&mut self, ip: usize, opcode: Opcode, stack_depth: usize, frame_depth: usize) {
        if self.stopped {
            return;
        }
        eprintln!("[{ip:>5}] {opcode:?}  stack={stack_depth}  frames={frame_depth}");
        self.count += 1;
        if let Some(limit) = self.limit
            && self.count >= limit
        {
            eprintln!("--- trace limit reached ({limit} instructions) ---");
            self.stopped = true;
        }
    }

    fn on_call(&mut self, func_name: Option<&str>, depth: usize) {
        if self.stopped {
            return;
        }
        let name = func_name.unwrap_or("<anonymous>");
        eprintln!("  >>> CALL {name:<20} depth={depth}");
    }

    fn on_return(&mut self, depth: usize) {
        if self.stopped {
            return;
        }
        eprintln!("  <<< RETURN              depth={depth}");
    }

    fn on_cell_load(&mut self, slot: u16, cells_len: usize) {
        if self.stopped {
            return;
        }
        eprintln!("  ... CELL LOAD  slot={slot} of {cells_len}");
    }

    fn on_cell_store(&mut self, slot: u16, cells_len: usize) {
        if self.stopped {
            return;
        }
        eprintln!("  ... CELL STORE slot={slot} of {cells_len}");
    }

    fn on_make_function(&mut self, cell_count: usize, defaults_count: usize) {
        if self.stopped {
            return;
        }
        if cell_count > 0 {
            eprintln!("  +++ MAKE CLOSURE  cells={cell_count} defaults={defaults_count}");
        } else {
            eprintln!("  +++ MAKE FUNCTION defaults={defaults_count}");
        }
    }
}

// ============================================================================
// ProfilingTracer — opcode frequency and call depth tracking
// ============================================================================

/// Tracer that collects execution statistics for profiling.
///
/// Tracks:
/// - Per-opcode execution counts (which opcodes are hot)
/// - Total instruction count
/// - Maximum call stack depth reached
/// - Total number of function calls
///
/// Retrieve results via [`ProfilingTracer::report`] after execution.
#[derive(Debug)]
pub struct ProfilingTracer {
    /// Per-opcode execution counts.
    opcode_counts: HashMap<Opcode, u64>,
    /// Total instructions executed.
    total_instructions: u64,
    /// Maximum call stack depth observed.
    max_depth: usize,
    /// Total number of function calls.
    total_calls: u64,
    /// Total number of cell loads.
    total_cell_loads: u64,
    /// Total number of cell stores.
    total_cell_stores: u64,
}

/// Summary report from a profiling trace.
#[derive(Debug)]
pub struct ProfilingReport {
    /// Per-opcode execution counts, sorted by frequency (highest first).
    pub opcode_counts: Vec<(Opcode, u64)>,
    /// Total instructions executed.
    pub total_instructions: u64,
    /// Maximum call stack depth observed.
    pub max_depth: usize,
    /// Total number of function calls.
    pub total_calls: u64,
    /// Total number of cell loads.
    pub total_cell_loads: u64,
    /// Total number of cell stores.
    pub total_cell_stores: u64,
}

impl ProfilingTracer {
    /// Creates a new profiling tracer with zeroed counters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            opcode_counts: HashMap::new(),
            total_instructions: 0,
            max_depth: 0,
            total_calls: 0,
            total_cell_loads: 0,
            total_cell_stores: 0,
        }
    }

    /// Generates a profiling report from the collected data.
    ///
    /// Opcode counts are sorted by frequency (most executed first).
    #[must_use]
    pub fn report(&self) -> ProfilingReport {
        let mut opcode_counts: Vec<_> = self.opcode_counts.iter().map(|(&k, &v)| (k, v)).collect();
        opcode_counts.sort_by(|a, b| b.1.cmp(&a.1));
        ProfilingReport {
            opcode_counts,
            total_instructions: self.total_instructions,
            max_depth: self.max_depth,
            total_calls: self.total_calls,
            total_cell_loads: self.total_cell_loads,
            total_cell_stores: self.total_cell_stores,
        }
    }
}

impl Default for ProfilingTracer {
    fn default() -> Self {
        Self::new()
    }
}

impl VmTracer for ProfilingTracer {
    #[inline]
    fn on_instruction(&mut self, _ip: usize, opcode: Opcode, _stack_depth: usize, _frame_depth: usize) {
        *self.opcode_counts.entry(opcode).or_insert(0) += 1;
        self.total_instructions += 1;
    }

    #[inline]
    fn on_call(&mut self, _func_name: Option<&str>, depth: usize) {
        self.total_calls += 1;
        if depth > self.max_depth {
            self.max_depth = depth;
        }
    }

    fn on_cell_load(&mut self, _slot: u16, _cells_len: usize) {
        self.total_cell_loads += 1;
    }

    fn on_cell_store(&mut self, _slot: u16, _cells_len: usize) {
        self.total_cell_stores += 1;
    }
}

// ============================================================================
// CoverageTracer — instruction pointer coverage
// ============================================================================

/// Tracer that records which instruction offsets were executed.
///
/// Useful for bytecode-level coverage analysis — identifying dead code
/// paths within compiled functions. Uses a bitset-style approach with
/// `AHashSet` for efficient insertion and membership testing.
///
/// Retrieve results via [`CoverageTracer::covered_ips`] after execution.
#[derive(Debug)]
pub struct CoverageTracer {
    /// Set of instruction pointers that were executed.
    ips: ahash::AHashSet<usize>,
}

impl CoverageTracer {
    /// Creates a new coverage tracer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ips: ahash::AHashSet::new(),
        }
    }

    /// Returns the set of instruction pointers that were executed.
    #[must_use]
    pub fn covered_ips(&self) -> &ahash::AHashSet<usize> {
        &self.ips
    }

    /// Returns the number of unique instruction offsets executed.
    #[must_use]
    pub fn coverage_count(&self) -> usize {
        self.ips.len()
    }
}

impl Default for CoverageTracer {
    fn default() -> Self {
        Self::new()
    }
}

impl VmTracer for CoverageTracer {
    #[inline]
    fn on_instruction(&mut self, ip: usize, _opcode: Opcode, _stack_depth: usize, _frame_depth: usize) {
        self.ips.insert(ip);
    }
}

// ============================================================================
// RecordingTracer — full event recording for replay
// ============================================================================

/// Tracer that records all events for deterministic replay or post-mortem analysis.
///
/// Captures every trace event into a `Vec<TraceEvent>`. This is the most
/// expensive tracer (allocates per event), so use it only for debugging
/// specific issues or recording short executions.
///
/// # Post-mortem analysis
///
/// After execution, iterate `events()` to reconstruct the full execution
/// history. Combined with the bytecode disassembly, this gives complete
/// visibility into what the VM did and why.
///
/// # Deterministic replay
///
/// The event stream can be compared between two runs to find divergence
/// points — useful for debugging non-determinism or comparing Ouros vs
/// CPython execution traces.
#[derive(Debug)]
pub struct RecordingTracer {
    /// All recorded events in chronological order.
    events: Vec<TraceEvent>,
    /// Optional limit on number of events recorded.
    limit: Option<usize>,
}

impl RecordingTracer {
    /// Creates a new recording tracer with no event limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            limit: None,
        }
    }

    /// Creates a new recording tracer that stops recording after `limit` events.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            events: Vec::with_capacity(limit.min(1024)),
            limit: Some(limit),
        }
    }

    /// Returns the recorded events.
    #[must_use]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Consumes the tracer and returns the recorded events.
    #[must_use]
    pub fn into_events(self) -> Vec<TraceEvent> {
        self.events
    }

    /// Returns the number of events recorded.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Returns true if the event limit has been reached.
    fn at_limit(&self) -> bool {
        self.limit.is_some_and(|l| self.events.len() >= l)
    }
}

impl Default for RecordingTracer {
    fn default() -> Self {
        Self::new()
    }
}

impl VmTracer for RecordingTracer {
    #[inline]
    fn on_instruction(&mut self, ip: usize, opcode: Opcode, stack_depth: usize, _frame_depth: usize) {
        if self.at_limit() {
            return;
        }
        self.events.push(TraceEvent::Instruction {
            ip,
            opcode,
            stack_depth,
        });
    }

    fn on_call(&mut self, func_name: Option<&str>, depth: usize) {
        if self.at_limit() {
            return;
        }
        self.events.push(TraceEvent::Call {
            func_name: func_name.map(String::from),
            depth,
        });
    }

    fn on_return(&mut self, depth: usize) {
        if self.at_limit() {
            return;
        }
        self.events.push(TraceEvent::Return { depth });
    }

    fn on_cell_load(&mut self, slot: u16, cells_len: usize) {
        if self.at_limit() {
            return;
        }
        self.events.push(TraceEvent::CellLoad { slot, cells_len });
    }

    fn on_cell_store(&mut self, slot: u16, cells_len: usize) {
        if self.at_limit() {
            return;
        }
        self.events.push(TraceEvent::CellStore { slot, cells_len });
    }

    fn on_make_function(&mut self, cell_count: usize, defaults_count: usize) {
        if self.at_limit() {
            return;
        }
        self.events.push(TraceEvent::MakeFunction {
            cell_count,
            defaults_count,
        });
    }
}

impl std::fmt::Display for ProfilingReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== VM Profiling Report ===")?;
        writeln!(f, "Total instructions: {}", self.total_instructions)?;
        writeln!(f, "Total calls:        {}", self.total_calls)?;
        writeln!(f, "Max call depth:     {}", self.max_depth)?;
        writeln!(f, "Cell loads:         {}", self.total_cell_loads)?;
        writeln!(f, "Cell stores:        {}", self.total_cell_stores)?;
        writeln!(f)?;
        writeln!(f, "--- Opcode Frequency ---")?;
        for (opcode, count) in &self.opcode_counts {
            let pct = (*count as f64 / self.total_instructions as f64) * 100.0;
            writeln!(f, "  {opcode:<20?} {count:>10}  ({pct:>5.1}%)")?;
        }
        Ok(())
    }
}
