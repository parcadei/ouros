use crate::{
    exception_private::ExceptionRaise,
    heap::{Heap, HeapId},
    parse::CodeRange,
    resource::{ResourceError, ResourceTracker},
    value::Value,
};

/// Unique identifier for values stored inside the namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(crate) struct NamespaceId(u32);

impl NamespaceId {
    pub fn new(index: usize) -> Self {
        Self(index.try_into().expect("Invalid namespace id"))
    }

    /// Returns the raw index value.
    ///
    /// Used by the bytecode compiler to emit slot indices for variable access.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index for the global (module-level) namespace in Namespaces.
/// At module level, local_idx == GLOBAL_NS_IDX (same namespace).
pub(crate) const GLOBAL_NS_IDX: NamespaceId = NamespaceId(0);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Namespace(Vec<Value>);

impl Namespace {
    fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    pub fn get(&self, index: NamespaceId) -> &Value {
        &self.0[index.index()]
    }

    #[cfg(feature = "ref-count-return")]
    pub fn get_opt(&self, index: NamespaceId) -> Option<&Value> {
        self.0.get(index.index())
    }

    pub fn get_mut(&mut self, index: NamespaceId) -> &mut Value {
        &mut self.0[index.index()]
    }

    pub fn mut_vec(&mut self) -> &mut Vec<Value> {
        &mut self.0
    }
}

impl IntoIterator for Namespace {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Storage for all namespaces during execution.
///
/// This struct owns all namespace data, allowing safe mutable access through indices.
/// Index 0 is always the global (module-level) namespace.
///
/// # Design Rationale
///
/// Instead of using raw pointers to share namespace access between frames,
/// we use indices into this central namespaces. Since variable scope (Local vs Global)
/// is known at compile time, we only ever need one mutable reference at a time.
///
/// # Closure Support
///
/// Variables captured by closures are stored in cells on the heap, not in namespaces.
/// The `get_var_value` method handles both namespace-based and cell-based variable access.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Namespaces {
    stack: Vec<Namespace>,
    /// if we have an old namespace to reuse, trace its id
    reuse_ids: Vec<NamespaceId>,
    /// Return values from external function calls or functions that completed after internal external calls.
    ///
    /// Each entry is `(call_position, value)`:
    /// - `call_position` is `None` for direct external function calls (any matching position works)
    /// - `call_position` is `Some(pos)` for function return values (only match at that exact call site)
    ///
    /// This distinction is necessary because during argument re-evaluation, we might have multiple
    /// function calls. Only the correct call should receive the cached return value.
    ext_return_values: Vec<(Option<CodeRange>, Value)>,
    /// Index of the next return value to be used.
    ///
    /// Since we can have multiple external function calls within a single statement (e.g. `foo() + bar()`),
    /// we need to keep track of which functions we've already called to continue execution.
    ///
    /// This is somewhat similar to temporal style durable execution, but just within a single statement.
    next_ext_return_value: usize,
    /// Pending exception from an external function call.
    ///
    /// When set, the next call to `take_ext_return_value` will return this error,
    /// allowing it to propagate through try/except blocks.
    ext_exception: Option<ExceptionRaise>,
}

impl Namespaces {
    /// Creates namespaces with the global namespace initialized.
    ///
    /// The global namespace is always at index 0.
    pub fn new(namespace: Vec<Value>) -> Self {
        Self {
            stack: vec![Namespace(namespace)],
            reuse_ids: vec![],
            ext_return_values: vec![],
            next_ext_return_value: 0,
            ext_exception: None,
        }
    }

    /// Creates an independent deep copy of the namespaces via serialization round-trip.
    ///
    /// The cloned namespaces are a self-consistent snapshot: every namespace slot,
    /// transient external-return state, and reuse-ID list is duplicated. Namespace
    /// indices remain valid in the clone because the stack layout is preserved.
    ///
    /// Used by `ReplSession::fork()` to branch execution. Transient external-call
    /// state (`ext_return_values`, `ext_exception`) is included in the clone so the
    /// snapshot is bit-for-bit faithful, though `fork()` itself clears them.
    ///
    /// # Panics
    ///
    /// Panics if serialization or deserialization fails, which should not happen
    /// for well-formed namespaces.
    pub fn deep_clone(&self) -> Self {
        let bytes = postcard::to_allocvec(self).expect("namespace serialization should not fail");
        postcard::from_bytes(&bytes).expect("namespace deserialization should not fail")
    }

    /// Resets namespaces for reuse, returning a mutable reference to the global
    /// namespace's value Vec for in-place refilling.
    ///
    /// This avoids deallocating and reallocating ANY Vecs: the outer `stack` Vec,
    /// the global namespace's inner Vec, and auxiliary Vecs all retain their
    /// allocated capacity. The caller should clear and refill the returned Vec
    /// (via `clear()` + `push()`/`extend()`) to set up the new global namespace.
    ///
    /// The caller must ensure all namespace values have been properly cleaned up
    /// (ref counts decremented) before calling reset.
    pub fn reset_global(&mut self) -> &mut Vec<Value> {
        // Truncate to just the global namespace slot, keeping any extra capacity.
        // This drops any additional namespace entries (which should already be
        // cleaned up via drop_with_heap).
        self.stack.truncate(1);
        // Clear the global namespace's values but keep the Vec's capacity
        self.stack[0].0.clear();
        // Clear auxiliary state
        self.reuse_ids.clear();
        self.ext_return_values.clear();
        self.next_ext_return_value = 0;
        self.ext_exception = None;
        // Return the cleared global namespace Vec for refilling
        &mut self.stack[0].0
    }

    /// Clears transient external-call state without touching namespace values.
    ///
    /// This is primarily used by REPL execution, where the global namespace must
    /// persist across lines but external-call bookkeeping (`ext_return_values`,
    /// `next_ext_return_value`, and `ext_exception`) must not leak between runs.
    ///
    /// Any cached return values are dropped with heap-aware cleanup so `Value::Ref`
    /// entries correctly decrement their refcounts on all paths.
    pub fn clear_transient_state(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        for (_, value) in std::mem::take(&mut self.ext_return_values) {
            value.drop_with_heap(heap);
        }
        self.next_ext_return_value = 0;
        self.ext_exception = None;
    }

    /// Drops all non-global namespace values and marks their slots as reusable.
    ///
    /// This is used after aborted execution paths (for example, exception unwind in
    /// a persistent REPL) to ensure function-local namespaces do not keep live
    /// references across runs. The global namespace at index 0 is intentionally
    /// preserved.
    ///
    /// After cleanup, every non-global namespace slot is empty and present in
    /// `reuse_ids`, allowing future function calls to reuse indices without growth.
    pub fn cleanup_non_global(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        let mut reuse_ids = Vec::with_capacity(self.stack.len().saturating_sub(1));
        for index in 1..self.stack.len() {
            let namespace = &mut self.stack[index];
            for value in namespace.0.drain(..) {
                value.drop_with_heap(heap);
            }
            reuse_ids.push(NamespaceId::new(index));
        }
        self.reuse_ids = reuse_ids;
    }

    /// Grows the global namespace to the requested size.
    ///
    /// Existing global values are preserved and any newly added slots are filled
    /// with `Value::Undefined`. The global namespace is append-only for REPL use,
    /// so shrinking is treated as a logic error and will panic.
    ///
    /// # Panics
    /// Panics when `new_size` is smaller than the current global namespace size.
    pub fn grow_global(&mut self, new_size: usize) {
        let global = &mut self.stack[GLOBAL_NS_IDX.index()];
        assert!(
            new_size >= global.0.len(),
            "namespace must never shrink: new {new_size} < current {}",
            global.0.len()
        );
        global.0.resize_with(new_size, || Value::Undefined);
    }

    /// Gets an immutable slice reference to a namespace by index.
    ///
    /// Used for reading from the enclosing namespace when defining closures,
    /// without requiring mutable access.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds.
    pub fn get(&self, idx: NamespaceId) -> &Namespace {
        &self.stack[idx.index()]
    }

    /// Gets a mutable slice reference to a namespace by index.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds.
    pub fn get_mut(&mut self, idx: NamespaceId) -> &mut Namespace {
        &mut self.stack[idx.index()]
    }

    /// Creates a new namespace for a function call with memory and recursion tracking.
    ///
    /// This method:
    /// 1. Checks recursion depth limit (fails fast before allocating)
    /// 2. Tracks namespace memory usage through the heap's `ResourceTracker`
    ///
    /// # Arguments
    /// * `namespace_size` - Expected number of values in the namespace
    /// * `heap` - The heap, used to access the resource tracker for memory accounting
    ///
    /// # Returns
    /// * `Ok(NamespaceId)` - Index of the new namespace
    /// * `Err(ResourceError::Recursion)` - If adding this namespace would exceed recursion limit
    /// * `Err(ResourceError::Memory)` - If adding this namespace would exceed memory limits
    pub fn new_namespace(
        &mut self,
        namespace_size: usize,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<NamespaceId, ResourceError> {
        // Check recursion depth BEFORE memory allocation (fail fast)
        // Depth = active namespaces only (total - freed - global).
        // We subtract reuse_ids.len() because those slots were freed when frames were popped,
        // and subtract 1 for the global namespace (stack[0]).
        // Without this correction, after catching RecursionError and unwinding 1000 frames,
        // the stack.len() stays at 1000+ (freed slots stay in the vec) and subsequent
        // calls would immediately fail even though the actual depth is back to normal.
        let current_depth = self.stack.len().saturating_sub(1 + self.reuse_ids.len());
        heap.tracker().check_recursion_depth(current_depth)?;

        // Track the memory used by this namespace's slots
        let size = namespace_size * std::mem::size_of::<Value>();
        heap.tracker_mut().on_allocate(|| size)?;

        if let Some(reuse_id) = self.reuse_ids.pop() {
            Ok(reuse_id)
        } else {
            let idx = NamespaceId::new(self.stack.len());
            self.stack.push(Namespace::with_capacity(namespace_size));
            Ok(idx)
        }
    }

    /// Registers a pre-built namespace (e.g., from a coroutine) with memory and recursion tracking.
    ///
    /// This is similar to `new_namespace` but takes an already-populated `Vec<Value>` instead
    /// of creating an empty one. Used when starting execution of a coroutine whose namespace
    /// was pre-bound at call time.
    ///
    /// # Arguments
    /// * `namespace` - The pre-built namespace values
    /// * `heap` - The heap, used to access the resource tracker for memory accounting
    ///
    /// # Returns
    /// * `Ok(NamespaceId)` - Index of the registered namespace
    /// * `Err(ResourceError::Recursion)` - If adding this namespace would exceed recursion limit
    /// * `Err(ResourceError::Memory)` - If adding this namespace would exceed memory limits
    pub fn register_prebuilt(
        &mut self,
        namespace: Vec<Value>,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<NamespaceId, ResourceError> {
        // Check recursion depth BEFORE memory allocation (fail fast)
        // Use active depth (total - freed - global) for accurate count after unwinding.
        let current_depth = self.stack.len().saturating_sub(1 + self.reuse_ids.len());
        heap.tracker().check_recursion_depth(current_depth)?;

        // Track the memory used by this namespace's slots
        let size = namespace.len() * std::mem::size_of::<Value>();
        heap.tracker_mut().on_allocate(|| size)?;

        // Try to reuse an existing slot, or push a new one
        if let Some(reuse_id) = self.reuse_ids.pop() {
            // Replace the old namespace with the new one
            self.stack[reuse_id.index()] = Namespace(namespace);
            Ok(reuse_id)
        } else {
            let idx = NamespaceId::new(self.stack.len());
            self.stack.push(Namespace(namespace));
            Ok(idx)
        }
    }

    /// Voids the most recently added namespace (after function returns),
    /// properly cleaning up any heap-allocated values.
    ///
    /// This method:
    /// 1. Tracks the freed memory through the heap's `ResourceTracker`
    /// 2. Decrements reference counts for any `Value::Ref` entries in the namespace
    ///
    /// # Panics
    /// Panics if attempting to pop the global namespace (index 0).
    pub fn drop_with_heap(&mut self, namespace_id: NamespaceId, heap: &mut Heap<impl ResourceTracker>) {
        let namespace = &mut self.stack[namespace_id.index()];
        // Track the freed memory for this namespace
        let size = namespace.0.len() * std::mem::size_of::<Value>();
        heap.tracker_mut().on_free(|| size);

        for value in namespace.0.drain(..) {
            value.drop_with_heap(heap);
        }
        self.reuse_ids.push(namespace_id);
    }

    /// Cleans up the global namespace by dropping all values with proper ref counting.
    ///
    /// Call this before the namespaces is dropped to properly decrement reference counts
    /// for any `Value::Ref` entries in the global namespace and return values.
    ///
    /// Only needed when `ref-count-panic` is enabled, since the Drop impl panics on unfreed Refs.
    #[cfg(feature = "ref-count-panic")]
    pub fn drop_global_with_heap(&mut self, heap: &mut Heap<impl ResourceTracker>) {
        // Clean up global namespace
        let global = self.get_mut(GLOBAL_NS_IDX);
        for value in &mut global.0 {
            let v = std::mem::replace(value, Value::Undefined);
            v.drop_with_heap(heap);
        }
        // Clean up any remaining return values from external function calls
        for (_, value) in std::mem::take(&mut self.ext_return_values) {
            value.drop_with_heap(heap);
        }
        // Clear any pending exception
        self.ext_exception = None;
    }

    /// Returns the global namespace for final inspection (e.g., ref-count testing).
    ///
    /// Consumes the namespaces since the namespace Vec is moved out.
    ///
    /// Only available when the `ref-count-return` feature is enabled.
    #[cfg(feature = "ref-count-return")]
    pub fn into_global(mut self) -> Namespace {
        self.stack.swap_remove(GLOBAL_NS_IDX.index())
    }

    /// Returns an iterator over all HeapIds referenced by values in all namespaces.
    ///
    /// This is used by garbage collection to find all root references. Any heap
    /// object reachable from these roots should not be collected.
    pub fn iter_heap_ids(&self) -> impl Iterator<Item = HeapId> + '_ {
        self.stack
            .iter()
            .flat_map(|namespace| namespace.0.iter().filter_map(Value::ref_id))
    }

    /// Extracts all values from a namespace, leaving the slot empty.
    ///
    /// This is used when moving namespace values to/from a generator's saved state.
    /// The namespace is cleared but not released - use `release_without_drop` afterwards.
    pub fn take_values(&mut self, idx: NamespaceId) -> Vec<Value> {
        std::mem::take(&mut self.stack[idx.index()].0)
    }

    /// Releases a namespace slot without dropping the values.
    ///
    /// This is used when namespace values have been moved elsewhere (e.g., to a generator).
    /// The slot is marked as free without running destructors on the values.
    pub fn release_without_drop(&mut self, idx: NamespaceId) {
        self.reuse_ids.push(idx);
    }

    /// Registers a pre-populated namespace vector and returns its ID.
    ///
    /// This is used when restoring a generator's namespace - the values are already
    /// populated and just need to be registered with the namespace system.
    ///
    /// # Arguments
    /// * `values` - The pre-populated namespace values
    /// * `heap` - The heap for resource tracking
    ///
    /// # Returns
    /// * `Ok(NamespaceId)` - The ID of the registered namespace
    /// * `Err(ResourceError::Recursion)` - If recursion limit would be exceeded
    /// * `Err(ResourceError::Memory)` - If memory limit would be exceeded
    pub fn register_values(
        &mut self,
        values: Vec<Value>,
        heap: &mut Heap<impl ResourceTracker>,
    ) -> Result<NamespaceId, ResourceError> {
        // Check recursion depth BEFORE memory allocation (fail fast)
        let current_depth = self.stack.len().saturating_sub(1 + self.reuse_ids.len());
        heap.tracker().check_recursion_depth(current_depth)?;

        // Track the memory used by this namespace's slots
        let size = values.len() * std::mem::size_of::<Value>();
        heap.tracker_mut().on_allocate(|| size)?;

        // Try to reuse an existing slot, or push a new one
        if let Some(reuse_id) = self.reuse_ids.pop() {
            self.stack[reuse_id.index()] = Namespace(values);
            Ok(reuse_id)
        } else {
            let idx = NamespaceId::new(self.stack.len());
            self.stack.push(Namespace(values));
            Ok(idx)
        }
    }
}
