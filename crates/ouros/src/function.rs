use std::fmt::Write;

use crate::{
    bytecode::Code,
    expressions::Identifier,
    intern::{Interns, StringId},
    namespace::NamespaceId,
    signature::Signature,
    value::EitherStr,
};

/// A defined function once compiled and ready for execution.
///
/// This is created during the compilation phase from a `PreparedFunctionDef`.
/// Contains everything needed to execute a user-defined function: compiled bytecode,
/// metadata, and closure information. Functions are stored on the heap and
/// referenced via HeapId.
///
/// # Namespace Layout
///
/// The namespace has a predictable layout that allows sequential construction:
/// ```text
/// [params...][cell_vars...][free_vars...][locals...]
/// ```
/// - Slots 0..signature.param_count(): function parameters (see `Signature` for layout)
/// - Slots after params: cell refs for variables captured by nested functions
/// - Slots after cell_vars: free_var refs (captured from enclosing scope)
/// - Remaining slots: local variables
///
/// # Closure Support
///
/// - `free_var_enclosing_slots`: Enclosing namespace slots for captured variables.
///   At definition time, cells are captured from these slots and stored in a Closure.
///   At call time, they're pushed sequentially after cell_vars.
/// - `cell_var_count`: Number of cells to create for variables captured by nested functions.
///   At call time, cells are created and pushed sequentially after params.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Function {
    /// The function name (used for error messages and repr).
    pub name: Identifier,
    /// Qualified name (e.g., `Outer.<locals>.inner` or `Class.method`).
    pub qualname: EitherStr,
    /// Module name where this function was defined.
    pub module_name: StringId,
    /// Type parameters declared with PEP 695 syntax (`def f[T]`).
    ///
    /// Stored as interned names; runtime semantics are provided via `__type_params__`.
    pub type_params: Vec<StringId>,
    /// The function signature.
    pub signature: Signature,
    /// Size of the initial namespace (number of local variable slots).
    pub namespace_size: usize,
    /// Enclosing namespace slots for variables captured from enclosing scopes.
    ///
    /// At definition time: look up cell HeapId from enclosing namespace at each slot.
    /// At call time: captured cells are pushed sequentially (our slots are implicit).
    pub free_var_enclosing_slots: Vec<NamespaceId>,
    /// Number of cell variables (captured by nested functions).
    ///
    /// At call time, this many cells are created and pushed right after params.
    /// Their slots are implicitly params.len()..params.len()+cell_var_count.
    pub cell_var_count: usize,
    /// Maps cell variable indices to their corresponding parameter indices, if any.
    ///
    /// When a parameter is also captured by nested functions (cell variable), its value
    /// must be copied into the cell after binding. Each entry corresponds to a cell
    /// (index 0..cell_var_count), and contains `Some(param_index)` if that cell is for
    /// a parameter, or `None` otherwise.
    pub cell_param_indices: Vec<Option<usize>>,
    /// Namespace slot reserved for the `__class__` cell in class body functions.
    ///
    /// This enables zero-argument `super()` and `__class__` references in methods
    /// by providing a cell that is set to the final class object during creation.
    #[serde(default)]
    pub class_cell_slot: Option<NamespaceId>,
    /// Target namespace slots for class-body closure cells captured from enclosing scopes.
    ///
    /// This is used only for class body execution, where captured cells may need
    /// to be installed into non-contiguous slots before methods are created.
    #[serde(default)]
    pub class_free_var_target_slots: Vec<NamespaceId>,
    /// Number of default parameter values.
    ///
    /// At function definition time, this many default values are evaluated and stored
    /// in a separate defaults array. The signature indicates how these map to parameters.
    pub defaults_count: usize,
    /// Whether this is an async function (`async def`).
    ///
    /// When true, calling this function creates a `Coroutine` object instead of
    /// immediately pushing a frame. The coroutine captures the bound arguments
    /// and starts execution only when awaited.
    pub is_async: bool,
    /// Whether this is a generator function (contains `yield` or `yield from`).
    ///
    /// When true, calling this function creates a `Generator` object instead of
    /// immediately pushing a frame. The generator captures the bound arguments
    /// and starts execution only when `__next__()` is called.
    pub is_generator: bool,
    /// Compiled bytecode for this function body.
    pub code: Code,
}

impl Function {
    /// Create a new compiled function.
    ///
    /// This is typically called by the bytecode compiler after compiling a `PreparedFunctionDef`.
    ///
    /// # Arguments
    /// * `name` - The function name identifier
    /// * `signature` - The function signature with parameter names and defaults
    /// * `namespace_size` - Number of local variable slots needed
    /// * `free_var_enclosing_slots` - Enclosing namespace slots for captured variables
    /// * `cell_var_count` - Number of cells to create for variables captured by nested functions
    /// * `cell_param_indices` - Maps cell indices to parameter indices for captured parameters
    /// * `defaults_count` - Number of default parameter values
    /// * `is_async` - Whether this is an async function
    /// * `is_generator` - Whether this is a generator function
    /// * `code` - The compiled bytecode for the function body
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        name: Identifier,
        qualname: EitherStr,
        module_name: StringId,
        type_params: Vec<StringId>,
        signature: Signature,
        namespace_size: usize,
        free_var_enclosing_slots: Vec<NamespaceId>,
        cell_var_count: usize,
        cell_param_indices: Vec<Option<usize>>,
        defaults_count: usize,
        is_async: bool,
        is_generator: bool,
        code: Code,
    ) -> Self {
        Self {
            name,
            qualname,
            module_name,
            type_params,
            signature,
            namespace_size,
            free_var_enclosing_slots,
            cell_var_count,
            cell_param_indices,
            class_cell_slot: None,
            class_free_var_target_slots: Vec::new(),
            defaults_count,
            is_async,
            is_generator,
            code,
        }
    }

    /// Creates a Function for a class body.
    ///
    /// Class body functions have no parameters, no defaults, and are not async.
    /// They may reserve a single `__class__` cell to support zero-arg `super()`.
    /// They are executed by the `BuildClass` opcode to populate the class namespace.
    pub fn new_class_body(
        name: Identifier,
        qualname: EitherStr,
        module_name: StringId,
        type_params: Vec<StringId>,
        namespace_size: usize,
        class_cell_slot: Option<NamespaceId>,
        free_var_enclosing_slots: Vec<NamespaceId>,
        class_free_var_target_slots: Vec<NamespaceId>,
        code: Code,
    ) -> Self {
        debug_assert_eq!(
            free_var_enclosing_slots.len(),
            class_free_var_target_slots.len(),
            "class-body free var metadata lengths must match"
        );
        let cell_var_count = usize::from(class_cell_slot.is_some());
        Self {
            name,
            qualname,
            module_name,
            type_params,
            signature: Signature::default(),
            namespace_size,
            free_var_enclosing_slots,
            cell_var_count,
            cell_param_indices: vec![None; cell_var_count],
            class_cell_slot,
            class_free_var_target_slots,
            defaults_count: 0,
            is_async: false,
            is_generator: false,
            code,
        }
    }

    /// Returns whether this function qualifies for the fast call path.
    ///
    /// A function is "simple sync" when it can be called without the full dispatch chain:
    /// - Not async (sync only)
    /// - Not a generator
    /// - No cell variables (not captured by nested functions)
    /// - Simple signature (no defaults, no *args/**kwargs, no keyword-only params)
    ///
    /// This is the hot path for recursive functions like `fib(n)` where the overhead
    /// of the 4-level dispatch chain (`exec_call_function` -> `call_function` ->
    /// `call_def_function` -> `call_sync_function`) dominates execution time.
    #[inline]
    pub fn is_simple_sync(&self) -> bool {
        !self.is_async && !self.is_generator && self.cell_var_count == 0 && self.signature.is_simple()
    }

    /// Returns whether this function qualifies for the simple-with-defaults fast path.
    ///
    /// This is similar to `is_simple_sync()`, but allows positional-or-keyword
    /// signatures with defaults. It is used by the VM's keyword-call fast path to
    /// avoid generic callable dispatch for common shapes like:
    /// `def f(a, b=1): ...; f(a=2)`.
    #[inline]
    pub fn is_simple_with_defaults_sync(&self) -> bool {
        !self.is_async && !self.is_generator && self.cell_var_count == 0 && self.signature.is_simple_with_defaults()
    }

    /// Writes the Python repr() string for this function to a formatter.
    pub fn py_repr_fmt<W: Write>(&self, f: &mut W, interns: &Interns, py_id: usize) -> std::fmt::Result {
        write!(f, "<function {} at 0x{py_id:x}>", self.qualname.as_str(interns))
    }
}
