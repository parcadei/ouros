//! Generator type for Python generator functions.
//!
//! Generators are functions that use `yield` to suspend execution and return
//! values one at a time. Each call to `__next__()` resumes execution until
//! the next `yield` or until the function returns (raising `StopIteration`).

use crate::{
    heap::{DropWithHeap, Heap, HeapId},
    intern::FunctionId,
    resource::ResourceTracker,
    value::Value,
};

/// Generator execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum GeneratorState {
    /// Generator has been created but not yet started.
    /// The frame has not been pushed yet.
    New,
    /// Generator is currently executing.
    /// Prevents reentrant calls (sandbox safety).
    Running,
    /// Generator is suspended at a yield expression.
    /// The frame is saved and can be resumed.
    Suspended,
    /// Generator has finished execution (returned or raised).
    /// Further calls to `__next__()` will raise `StopIteration`.
    Finished,
}

/// A suspended generator function.
///
/// When a generator function is called, instead of executing its body,
/// a Generator is created on the heap. Each call to `__next__()` resumes
/// execution until the next yield or return.
///
/// # Namespace Layout
///
/// The `namespace` vector is pre-sized to match the function's namespace size and contains:
/// ```text
/// [params...][cell_vars...][free_vars...][locals...]
/// ```
/// - Parameter slots are filled with bound argument values at call time
/// - Cell/free var slots contain `Value::Ref` to captured cells
/// - Local slots start as `Value::Undefined`
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Generator {
    /// The generator function to execute.
    pub func_id: FunctionId,
    /// Pre-bound namespace values (sized to function namespace).
    /// Contains bound parameters, captured cells, and uninitialized locals.
    pub namespace: Vec<Value>,
    /// HeapIds of captured cells from enclosing scopes.
    /// These are passed to the frame when execution starts/resumes.
    pub frame_cells: Vec<HeapId>,
    /// Current execution state.
    pub state: GeneratorState,
    /// Saved instruction pointer (valid when state is Suspended).
    pub saved_ip: usize,
    /// Saved stack values (valid when state is Suspended).
    pub saved_stack: Vec<Value>,
    /// Source line of the most recent suspension point.
    ///
    /// This tracks `gi_frame.f_lineno` while the generator is suspended.
    /// `None` means no suspension line has been recorded yet.
    #[serde(default)]
    pub saved_lineno: Option<u16>,
}

impl Generator {
    /// Creates a new generator for a generator function call.
    ///
    /// # Arguments
    /// * `func_id` - The generator function to execute
    /// * `namespace` - Pre-bound namespace with parameters and captured variables
    /// * `frame_cells` - HeapIds of captured cells from enclosing scopes
    pub fn new(func_id: FunctionId, namespace: Vec<Value>, frame_cells: Vec<HeapId>) -> Self {
        Self {
            func_id,
            namespace,
            frame_cells,
            state: GeneratorState::New,
            saved_ip: 0,
            saved_stack: Vec::new(),
            saved_lineno: None,
        }
    }

    /// Returns true if the generator is finished.
    #[inline]
    #[expect(dead_code)]
    pub fn is_finished(&self) -> bool {
        matches!(self.state, GeneratorState::Finished)
    }
}

impl<T: ResourceTracker> DropWithHeap<T> for Generator {
    fn drop_with_heap(self, heap: &mut Heap<T>) {
        // Drop all namespace values
        for value in self.namespace {
            value.drop_with_heap(heap);
        }
        // Drop all saved stack values
        for value in self.saved_stack {
            value.drop_with_heap(heap);
        }
        // Drop all frame cells
        for cell_id in self.frame_cells {
            heap.dec_ref(cell_id);
        }
    }
}
