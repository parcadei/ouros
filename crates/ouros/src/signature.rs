//! Function signature representation and argument binding.
//!
//! This module handles Python function signatures including all parameter types:
//! positional-only, positional-or-keyword, *args, keyword-only, and **kwargs.
//! It also handles default values and the argument binding algorithm.

use crate::{
    args::{ArgValues, KwargsValues},
    exception_private::{ExcType, RunResult, SimpleException},
    expressions::Identifier,
    heap::{Heap, HeapData},
    intern::{Interns, StringId},
    resource::ResourceTracker,
    types::{Dict, allocate_tuple},
    value::Value,
};

/// Represents a Python function signature with all parameter types.
///
/// A complete Python signature can include:
/// - Positional-only parameters (before `/`)
/// - Positional-or-keyword parameters (regular parameters)
/// - Variable positional parameter (`*args`)
/// - Keyword-only parameters (after `*` or `*args`)
/// - Variable keyword parameter (`**kwargs`)
///
/// # Default Values
///
/// Default values are tracked by count per parameter group. The `*_defaults_count` fields
/// indicate how many parameters (from the end of each group) have defaults. For example,
/// if `args = [a, b, c]` and `arg_defaults_count = 2`, then `b` and `c` have defaults.
///
/// Note: The actual default Values are evaluated at function definition time and stored
/// separately (in the heap as part of the function/closure object). This struct only
/// tracks the structure, not the values themselves.
///
/// # Namespace Layout
///
/// Parameters are laid out in the namespace in this order:
/// ```text
/// [pos_args][args][*args_slot?][kwargs][**kwargs_slot?]
/// ```
/// The `*args` slot is only present if `var_args` is Some.
/// The `**kwargs` slot is only present if `var_kwargs` is Some.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Signature {
    /// Positional-only parameters, e.g. `a, b` in `def f(a, b, /): ...`
    ///
    /// These can only be passed by position, not by keyword.
    pos_args: Option<Vec<StringId>>,

    /// Number of positional-only parameters with defaults (from the end).
    pos_defaults_count: usize,

    /// Positional-or-keyword parameters, e.g. `a, b` in `def f(a, b): ...`
    ///
    /// These can be passed either by position or by keyword.
    args: Option<Vec<StringId>>,

    /// Number of positional-or-keyword parameters with defaults (from the end).
    arg_defaults_count: usize,

    /// Variable positional parameter name, e.g. `args` in `def f(*args): ...`
    ///
    /// Collects excess positional arguments into a tuple.
    var_args: Option<StringId>,

    /// Keyword-only parameters, e.g. `c` in `def f(*, c): ...` or `def f(*args, c): ...`
    ///
    /// These can only be passed by keyword, not by position.
    kwargs: Option<Vec<StringId>>,

    /// Mapping from each keyword-only parameter to its default index (if any).
    ///
    /// Each entry corresponds to the same index in `kwargs`. A value of `Some(i)`
    /// points into the kwarg section of the defaults array, while `None` means
    /// the parameter is required.
    kwarg_default_map: Option<Vec<Option<usize>>>,

    /// Variable keyword parameter name, e.g. `kwargs` in `def f(**kwargs): ...`
    ///
    /// Collects excess keyword arguments into a dict.
    var_kwargs: Option<StringId>,

    /// How simple the signature is, used for fast path when binding
    bind_mode: BindMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum BindMode {
    /// If this is a simple signature (no defaults, no *args/**kwargs).
    ///
    /// Simple signatures can use a fast path for argument binding that avoids
    /// the full binding algorithm overhead. A simple signature has:
    /// - No positional-only parameters
    /// - No defaults for any parameters
    /// - No *args or **kwargs
    /// - No keyword-only parameters
    #[default]
    Simple,
    /// If this signature has only positional-or-keyword params with defaults.
    ///
    /// This identifies the common pattern `def f(a, b=1, c=2)` where:
    /// - No positional-only parameters
    /// - No *args or **kwargs
    /// - No keyword-only parameters
    /// - Has some default values
    ///
    /// These signatures can use a simplified binding that just fills positional
    /// args and applies defaults without the full algorithm overhead.
    SimpleWithDefaults,
    Complex,
}

impl Signature {
    /// Creates a full signature with all parameter types.
    ///
    /// # Arguments
    /// * `pos_args` - Positional-only parameter names
    /// * `pos_defaults_count` - Number of pos_args with defaults (from end)
    /// * `args` - Positional-or-keyword parameter names
    /// * `arg_defaults_count` - Number of args with defaults (from end)
    /// * `var_args` - Variable positional parameter name (*args)
    /// * `kwargs` - Keyword-only parameter names
    /// * `kwarg_default_map` - Mapping of kw-only parameters to default indices
    /// * `var_kwargs` - Variable keyword parameter name (**kwargs)
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        pos_args: Vec<StringId>,
        pos_defaults_count: usize,
        args: Vec<StringId>,
        arg_defaults_count: usize,
        var_args: Option<StringId>,
        kwargs: Vec<StringId>,
        kwarg_default_map: Vec<Option<usize>>,
        var_kwargs: Option<StringId>,
    ) -> Self {
        let pos_args = if pos_args.is_empty() { None } else { Some(pos_args) };
        let has_kwonly = !kwargs.is_empty();
        let kwargs = if has_kwonly { Some(kwargs) } else { None };

        let bind_mode = if pos_args.is_none()
            && pos_defaults_count == 0
            && arg_defaults_count == 0
            && var_args.is_none()
            && kwargs.is_none()
            && var_kwargs.is_none()
        {
            BindMode::Simple
        } else if pos_args.is_none()
            && var_args.is_none()
            && kwargs.is_none()
            && var_kwargs.is_none()
            && arg_defaults_count > 0
        {
            BindMode::SimpleWithDefaults
        } else {
            BindMode::Complex
        };

        Self {
            pos_args,
            pos_defaults_count,
            args: if args.is_empty() { None } else { Some(args) },
            arg_defaults_count,
            var_args,
            kwargs,
            kwarg_default_map: if has_kwonly { Some(kwarg_default_map) } else { None },
            var_kwargs,
            bind_mode,
        }
    }

    /// Binds arguments to parameters according to Python's calling conventions.
    ///
    /// This implements the full argument binding algorithm:
    /// 1. Bind positional args to pos_args, then args (in order)
    /// 2. Bind keyword args to args and kwargs (NOT pos_args - positional-only)
    /// 3. Collect excess positional args into *args tuple
    /// 4. Collect excess keyword args into **kwargs dict
    /// 5. Apply defaults for missing parameters
    ///
    /// Returns a Vec<Value> ready to be injected into the namespace, laid out as:
    /// `[pos_args][args][*args_slot?][kwargs][**kwargs_slot?]`
    ///
    /// # Arguments
    /// * `args` - The arguments from the call site
    /// * `defaults` - Evaluated default values (layout: pos_defaults, arg_defaults, kwarg_defaults)
    /// * `heap` - The heap for allocating *args tuple and **kwargs dict
    /// * `interns` - For looking up parameter names in error messages
    /// * `func_name` - Function name for error messages
    /// * `namespace_size` - The size of the namespace to allocate
    ///
    /// # Errors
    /// Returns an error if:
    /// - Too few or too many positional arguments
    /// - Missing required keyword-only arguments
    /// - Unexpected keyword argument
    /// - Positional-only parameter passed as keyword
    /// - Same argument passed both positionally and by keyword
    pub fn bind(
        &self,
        mut args: ArgValues,
        defaults: &[Value],
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        func_name: Identifier,
        namespace: &mut Vec<Value>,
    ) -> RunResult<()> {
        // Fast path for simple signatures (no defaults, no special params) and
        // signatures with only positional-or-keyword params and defaults.
        // This avoids the full binding algorithm overhead for common cases.

        if matches!(self.bind_mode, BindMode::Simple | BindMode::SimpleWithDefaults) {
            // Try to consume args directly into namespace without the full algorithm.
            // Returns Some(args) if kwargs were passed (need full algorithm).
            let opt_args = match args {
                ArgValues::Empty => None,
                ArgValues::One(a) => {
                    namespace.push(a);
                    None
                }
                ArgValues::Two(a1, a2) => {
                    namespace.push(a1);
                    namespace.push(a2);
                    None
                }
                ArgValues::ArgsKargs {
                    args,
                    kwargs: KwargsValues::Empty,
                } => {
                    namespace.extend(args);
                    None
                }
                // Fast path for explicit keyword calls on simple-with-defaults signatures.
                // This handles common shapes like `def f(a, b=1); f(a=2)` without the
                // full generic binder.
                ArgValues::Kwargs(KwargsValues::Inline(kwargs)) if self.bind_mode == BindMode::SimpleWithDefaults => {
                    return self.bind_simple_with_defaults_inline_kwargs(
                        kwargs, defaults, heap, interns, func_name, namespace,
                    );
                }
                ArgValues::ArgsKargs {
                    args,
                    kwargs: KwargsValues::Inline(kwargs),
                } if self.bind_mode == BindMode::SimpleWithDefaults && args.is_empty() => {
                    return self.bind_simple_with_defaults_inline_kwargs(
                        kwargs, defaults, heap, interns, func_name, namespace,
                    );
                }
                args => Some(args),
            };

            if let Some(continue_args) = opt_args {
                // Kwargs were passed - need full algorithm
                args = continue_args;
            } else {
                let actual_count = namespace.len();
                let param_count = self.param_count();

                if actual_count == param_count {
                    // Exact match - no defaults needed
                    return Ok(());
                } else if self.bind_mode == BindMode::SimpleWithDefaults {
                    let required = self.required_positional_count();
                    if actual_count >= required && actual_count < param_count {
                        // Apply defaults for remaining parameters
                        // Defaults are stored at the end of the defaults array for pos-or-kw params
                        let defaults_needed = param_count - actual_count;
                        let defaults_start = self.arg_defaults_count - defaults_needed;
                        for i in 0..defaults_needed {
                            namespace.push(defaults[defaults_start + i].clone_with_heap(heap));
                        }
                        return Ok(());
                    }
                }

                // Wrong number of arguments - clean up and return error
                for val in namespace.drain(..) {
                    val.drop_with_heap(heap);
                }
                return self.wrong_arg_count_error(actual_count, interns, func_name);
            }
        }
        // Full binding algorithm for complex signatures or kwargs

        // Split args into positional iterator and keyword components without allocating
        let (mut pos_iter, keyword_args) = args.into_parts();

        // Calculate how many positional params we have
        let pos_param_count = self.pos_arg_count();
        let arg_param_count = self.arg_count();
        let total_positional_params = pos_param_count + arg_param_count;

        // Check positional argument count against maximum
        let positional_count = pos_iter.len();
        let kwonly_given = keyword_args.len();
        if let Some(max) = self.max_positional_count()
            && positional_count > max
        {
            let func = interns.get_str(func_name.name_id);
            // Must clean up iterator and kwargs before returning error
            for value in pos_iter {
                value.drop_with_heap(heap);
            }
            keyword_args.drop_with_heap(heap);
            return Err(ExcType::type_error_too_many_positional(
                func,
                max,
                positional_count,
                kwonly_given,
            ));
        }

        // Initialize result namespace with Undefined values for all slots
        // Layout: [pos_args][args][*args?][kwargs][**kwargs?]
        let var_args_offset = usize::from(self.var_args.is_some());
        for _ in 0..self.total_slots() {
            namespace.push(Value::Undefined);
        }

        // Track which parameters have been bound (for duplicate detection)
        // Uses a u64 bitmap - supports up to 64 named parameters which is sufficient
        // for any reasonable Python function (Python itself has practical limits).
        // Note: this tracks only named params, not *args/**kwargs slots
        let mut bound_params: u64 = 0;

        // 1. Bind positional args to pos_args, then args

        // Bind to pos_args
        for (i, slot) in namespace.iter_mut().enumerate().take(pos_param_count) {
            if let Some(val) = pos_iter.next() {
                *slot = val;
                bound_params |= 1 << i;
            }
        }

        // Bind to args
        for (i, slot) in namespace
            .iter_mut()
            .enumerate()
            .take(total_positional_params)
            .skip(pos_param_count)
        {
            if let Some(val) = pos_iter.next() {
                *slot = val;
                bound_params |= 1 << i;
            }
        }

        // 2. Collect excess positional args into *args tuple
        let excess_positional = pos_iter.collect();
        let var_args_value = if self.var_args.is_some() {
            // Create tuple from excess args
            Some(allocate_tuple(excess_positional, heap)?)
        } else {
            None
        };
        // If no *args, excess was already checked above via max_positional_count

        // 3. Bind keyword args
        // Bind keywords to args and kwargs (not pos_args - those are positional-only)
        let mut excess_kwargs = Dict::new();

        for (key, value) in keyword_args {
            let Some(keyword_name) = key.as_either_str(heap) else {
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
                return Err(ExcType::type_error("keywords must be strings"));
            };

            // Check if this keyword matches a positional-only param (error)
            if let Some(pos_args) = &self.pos_args
                && let Some(&param_id) = pos_args
                    .iter()
                    .find(|&&param_id| keyword_name.matches(param_id, interns))
            {
                let func = interns.get_str(func_name.name_id);
                let param = interns.get_str(param_id);
                key.drop_with_heap(heap);
                value.drop_with_heap(heap);
                cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
                return Err(ExcType::type_error_positional_only(func, param));
            }

            // Use Option to track the value as we try to bind it
            let mut remaining_value = Some(value);
            let mut key_value = Some(key);

            // Try to bind to an args param
            if let Some(ref args) = self.args {
                let matching_param = args
                    .iter()
                    .enumerate()
                    .find(|&(_, param_id)| keyword_name.matches(*param_id, interns));
                if let Some((i, &param_id)) = matching_param {
                    let idx = pos_param_count + i;
                    if (bound_params & (1 << idx)) != 0 {
                        let func = interns.get_str(func_name.name_id);
                        let param = interns.get_str(param_id);
                        if let Some(v) = remaining_value.take() {
                            v.drop_with_heap(heap);
                        }
                        if let Some(dup_key) = key_value.take() {
                            dup_key.drop_with_heap(heap);
                        }
                        cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
                        return Err(ExcType::type_error_duplicate_arg(func, param));
                    }
                    if let Some(v) = remaining_value.take() {
                        namespace[idx] = v;
                    }
                    bound_params |= 1 << idx;
                    if let Some(key) = key_value.take() {
                        key.drop_with_heap(heap);
                    }
                }
            }

            // Try to bind to a kwargs param (keyword-only)
            if remaining_value.is_some()
                && let Some(ref kwargs) = self.kwargs
            {
                for (i, &param_id) in kwargs.iter().enumerate() {
                    if keyword_name.matches(param_id, interns) {
                        // Skip past *args slot if present
                        let ns_idx = total_positional_params + var_args_offset + i;
                        let idx = total_positional_params + i;
                        if (bound_params & (1 << idx)) != 0 {
                            let func = interns.get_str(func_name.name_id);
                            let param = interns.get_str(param_id);
                            if let Some(v) = remaining_value.take() {
                                v.drop_with_heap(heap);
                            }
                            if let Some(dup_key) = key_value.take() {
                                dup_key.drop_with_heap(heap);
                            }
                            cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
                            return Err(ExcType::type_error_duplicate_arg(func, param));
                        }
                        // Store the value for this keyword-only param
                        if let Some(v) = remaining_value.take() {
                            namespace[ns_idx] = v;
                        }
                        bound_params |= 1 << idx;
                        if let Some(bound_key) = key_value.take() {
                            bound_key.drop_with_heap(heap);
                        }
                        break;
                    }
                }
            }

            // If still not bound, handle as excess or error
            if let Some(v) = remaining_value {
                if self.var_kwargs.is_some() {
                    // Collect into **kwargs
                    let key_for_kwargs = key_value.take().expect("keyword key available for **kwargs");
                    excess_kwargs.set(key_for_kwargs, v, heap, interns)?;
                } else {
                    let func = interns.get_str(func_name.name_id);
                    let key_str = keyword_name.as_str(interns);
                    v.drop_with_heap(heap);
                    if let Some(unused_key) = key_value.take() {
                        unused_key.drop_with_heap(heap);
                    }
                    cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
                    return Err(ExcType::type_error_unexpected_keyword(func, key_str));
                }
            }

            if let Some(unused_key) = key_value {
                unused_key.drop_with_heap(heap);
            }
        }

        // 3.5. Apply default values to unbound optional parameters
        // Defaults layout: [pos_defaults...][arg_defaults...][kwarg_defaults...]
        // Each section only contains defaults for params that have them.
        let mut default_idx = 0;

        // Apply pos_args defaults (optional params at the end of pos_args)
        if self.pos_defaults_count > 0 {
            let first_optional = pos_param_count - self.pos_defaults_count;
            for i in first_optional..pos_param_count {
                if (bound_params & (1 << i)) == 0 {
                    namespace[i] = defaults[default_idx + (i - first_optional)].clone_with_heap(heap);
                    bound_params |= 1 << i;
                }
            }
        }
        default_idx += self.pos_defaults_count;

        // Apply args defaults (optional params at the end of args)
        if self.arg_defaults_count > 0 {
            let first_optional = arg_param_count - self.arg_defaults_count;
            for i in first_optional..arg_param_count {
                let ns_idx = pos_param_count + i;
                if (bound_params & (1 << ns_idx)) == 0 {
                    namespace[ns_idx] = defaults[default_idx + (i - first_optional)].clone_with_heap(heap);
                    bound_params |= 1 << ns_idx;
                }
            }
        }
        default_idx += self.arg_defaults_count;

        // Apply kwargs defaults using the explicit default map
        if let Some(ref default_map) = self.kwarg_default_map {
            for (i, default_slot) in default_map.iter().enumerate() {
                if let Some(slot_idx) = default_slot {
                    let bound_idx = total_positional_params + i;
                    // Skip past *args slot if present
                    let ns_idx = total_positional_params + var_args_offset + i;
                    if (bound_params & (1 << bound_idx)) == 0 {
                        namespace[ns_idx] = defaults[default_idx + slot_idx].clone_with_heap(heap);
                        bound_params |= 1 << bound_idx;
                    }
                }
            }
        }

        // 4. Check that all required params are bound BEFORE building final namespace.
        // This ensures we can clean up properly on error without leaking heap values.
        let func = interns.get_str(func_name.name_id);

        // Check required positional params (pos_args + required args)
        let mut missing_positional: Vec<&str> = Vec::new();

        // Check pos_args
        if let Some(ref pos_args) = self.pos_args {
            let required_pos_only = pos_args.len().saturating_sub(self.pos_defaults_count);
            for (i, &param_id) in pos_args.iter().enumerate() {
                if i < required_pos_only && (bound_params & (1 << i)) == 0 {
                    missing_positional.push(interns.get_str(param_id));
                }
            }
        }

        // Check args (positional-or-keyword)
        if let Some(ref args_params) = self.args {
            let required_args = args_params.len().saturating_sub(self.arg_defaults_count);
            for (i, &param_id) in args_params.iter().enumerate() {
                if i < required_args && (bound_params & (1 << (pos_param_count + i))) == 0 {
                    missing_positional.push(interns.get_str(param_id));
                }
            }
        }

        if !missing_positional.is_empty() {
            // Clean up bound values before returning error
            cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
            return Err(ExcType::type_error_missing_positional_with_names(
                func,
                &missing_positional,
            ));
        }

        // Check required keyword-only args
        let mut missing_kwonly: Vec<&str> = Vec::new();
        if let Some(ref kwargs_params) = self.kwargs {
            let default_map = self.kwarg_default_map.as_ref();
            for (i, &param_id) in kwargs_params.iter().enumerate() {
                let has_default = default_map.and_then(|map| map.get(i)).is_some_and(Option::is_some);
                if !has_default && (bound_params & (1 << (total_positional_params + i))) == 0 {
                    missing_kwonly.push(interns.get_str(param_id));
                }
            }
        }

        if !missing_kwonly.is_empty() {
            // Clean up bound values before returning error
            cleanup_on_error(namespace, var_args_value, excess_kwargs, heap);
            return Err(ExcType::type_error_missing_kwonly_with_names(func, &missing_kwonly));
        }

        // 5. Fill in *args and **kwargs slots directly
        // Namespace layout: [pos_args][args][*args?][kwargs][**kwargs?]

        // Insert *args tuple if present
        if let Some(var_args_val) = var_args_value {
            namespace[total_positional_params] = var_args_val;
        }

        // Insert **kwargs dict if present (at the last slot)
        if self.var_kwargs.is_some() {
            let dict_id = heap.allocate(HeapData::Dict(excess_kwargs))?;
            let last_slot = namespace.len() - 1;
            namespace[last_slot] = Value::Ref(dict_id);
        }

        Ok(())
    }

    /// Returns the total number of named parameters (excluding *args/**kwargs slots).
    ///
    /// This is `pos_args.len() + args.len() + kwargs.len()`.
    pub fn param_count(&self) -> usize {
        self.pos_arg_count() + self.arg_count() + self.kwarg_count()
    }

    /// Returns the total number of namespace slots needed for parameters.
    ///
    /// This includes slots for:
    /// - All named parameters (pos_args + args + kwargs)
    /// - The *args tuple (if var_args is Some)
    /// - The **kwargs dict (if var_kwargs is Some)
    pub fn total_slots(&self) -> usize {
        let mut slots = self.param_count();
        if self.var_args.is_some() {
            slots += 1;
        }
        if self.var_kwargs.is_some() {
            slots += 1;
        }
        slots
    }

    /// Returns whether this signature uses simple binding mode (no defaults, no *args/**kwargs,
    /// no keyword-only params, no positional-only params).
    ///
    /// Simple signatures support fast-path argument binding where positional args
    /// are pushed directly into the namespace without the full binding algorithm.
    #[inline]
    pub fn is_simple(&self) -> bool {
        self.bind_mode == BindMode::Simple
    }

    /// Returns whether this signature uses simple-with-defaults binding mode.
    ///
    /// This mode covers signatures with only positional-or-keyword parameters
    /// and at least one default value (no positional-only params, no *args,
    /// no **kwargs, no keyword-only params).
    #[inline]
    pub fn is_simple_with_defaults(&self) -> bool {
        self.bind_mode == BindMode::SimpleWithDefaults
    }

    /// Returns the total number of default values across all parameter groups.
    pub fn total_defaults_count(&self) -> usize {
        self.pos_defaults_count + self.arg_defaults_count + self.kwarg_defaults_count()
    }

    /// Returns the minimum number of positional arguments required.
    ///
    /// This is the total positional param count minus the number of defaults.
    /// For a signature like `def f(a, b, c=1)`, this returns 2 (a and b are required).
    #[inline]
    fn required_positional_count(&self) -> usize {
        self.pos_arg_count() + self.arg_count() - self.pos_defaults_count - self.arg_defaults_count
    }

    fn kwarg_defaults_count(&self) -> usize {
        self.kwarg_default_map
            .as_deref()
            .map(|v| v.iter().filter(|&x| x.is_some()).count())
            .unwrap_or_default()
    }

    /// Returns the number of positional-only parameters.
    fn pos_arg_count(&self) -> usize {
        self.pos_args.as_ref().map_or(0, Vec::len)
    }

    /// Returns the number of positional-or-keyword parameters.
    fn arg_count(&self) -> usize {
        self.args.as_ref().map_or(0, Vec::len)
    }

    /// Returns the number of keyword-only parameters.
    fn kwarg_count(&self) -> usize {
        self.kwargs.as_ref().map_or(0, Vec::len)
    }

    /// Returns an iterator over all parameter names in namespace slot order.
    ///
    /// Order: pos_args, args, var_args (if present), kwargs, var_kwargs (if present)
    pub(crate) fn param_names(&self) -> impl Iterator<Item = StringId> + '_ {
        let pos_args = self.pos_args.iter().flat_map(|v| v.iter().copied());
        let args = self.args.iter().flat_map(|v| v.iter().copied());
        let var_args = self.var_args.iter().copied();
        let kwargs = self.kwargs.iter().flat_map(|v| v.iter().copied());
        let var_kwargs = self.var_kwargs.iter().copied();

        pos_args.chain(args).chain(var_args).chain(kwargs).chain(var_kwargs)
    }

    /// Returns the maximum number of positional arguments accepted.
    ///
    /// Returns None if *args is present (unlimited positional args).
    fn max_positional_count(&self) -> Option<usize> {
        if self.var_args.is_some() {
            None
        } else {
            Some(self.pos_arg_count() + self.arg_count())
        }
    }

    /// Binds inline keyword arguments for `SimpleWithDefaults` signatures.
    ///
    /// This avoids the full generic binder for the common case of explicit keyword
    /// calls to plain functions with positional-or-keyword parameters and defaults
    /// (e.g. `def f(a, b=1); f(a=2)`).
    ///
    /// The helper intentionally handles only inline keyword inputs generated by
    /// normal call opcodes. All other argument shapes continue through `bind()`.
    fn bind_simple_with_defaults_inline_kwargs(
        &self,
        kwargs: Vec<(StringId, Value)>,
        defaults: &[Value],
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        func_name: Identifier,
        namespace: &mut Vec<Value>,
    ) -> RunResult<()> {
        debug_assert_eq!(self.bind_mode, BindMode::SimpleWithDefaults);

        if kwargs.len() == 1 {
            let mut kwargs_iter = kwargs.into_iter();
            let (keyword_id, value) = kwargs_iter.next().expect("length checked");
            return self.bind_simple_with_defaults_one_inline_kw(
                keyword_id, value, defaults, heap, interns, func_name, namespace,
            );
        }

        let args_params = self
            .args
            .as_ref()
            .expect("SimpleWithDefaults signatures must define positional-or-keyword parameters");
        let param_count = args_params.len();
        namespace.resize_with(param_count, || Value::Undefined);

        let func = interns.get_str(func_name.name_id);
        let mut kwargs_iter = kwargs.into_iter();
        while let Some((keyword_id, value)) = kwargs_iter.next() {
            let Some(param_idx) = args_params.iter().position(|&param_id| param_id == keyword_id) else {
                value.drop_with_heap(heap);
                for (_, remaining) in kwargs_iter {
                    remaining.drop_with_heap(heap);
                }
                for bound in namespace.drain(..) {
                    bound.drop_with_heap(heap);
                }
                return Err(ExcType::type_error_unexpected_keyword(
                    func,
                    interns.get_str(keyword_id),
                ));
            };

            if !matches!(namespace[param_idx], Value::Undefined) {
                value.drop_with_heap(heap);
                for (_, remaining) in kwargs_iter {
                    remaining.drop_with_heap(heap);
                }
                for bound in namespace.drain(..) {
                    bound.drop_with_heap(heap);
                }
                return Err(ExcType::type_error_duplicate_arg(
                    func,
                    interns.get_str(args_params[param_idx]),
                ));
            }

            namespace[param_idx] = value;
        }

        // Apply defaults for optional parameters that are still unbound.
        let first_optional = param_count.saturating_sub(self.arg_defaults_count);
        for slot in first_optional..param_count {
            if matches!(namespace[slot], Value::Undefined) {
                namespace[slot] = defaults[slot - first_optional].clone_with_heap(heap);
            }
        }

        // Validate required parameters after defaults are applied.
        let required = self.required_positional_count();
        let mut missing_positional: Vec<&str> = Vec::new();
        for slot in 0..required {
            if matches!(namespace[slot], Value::Undefined) {
                missing_positional.push(interns.get_str(args_params[slot]));
            }
        }

        if !missing_positional.is_empty() {
            for bound in namespace.drain(..) {
                bound.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_missing_positional_with_names(
                func,
                &missing_positional,
            ));
        }

        Ok(())
    }

    /// Binds exactly one inline keyword argument for `SimpleWithDefaults`.
    ///
    /// This specializes the hot call shape used by bytecode `CallFunctionKw`
    /// when `pos_count == 0 && kw_count == 1` (for example `f(a=1)` where
    /// `f` is `def f(a, b=2): ...`). It avoids building a temporary kwargs
    /// vector and the generic binding loop.
    pub(crate) fn bind_simple_with_defaults_one_inline_kw(
        &self,
        keyword_id: StringId,
        value: Value,
        defaults: &[Value],
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
        func_name: Identifier,
        namespace: &mut Vec<Value>,
    ) -> RunResult<()> {
        debug_assert_eq!(self.bind_mode, BindMode::SimpleWithDefaults);

        let args_params = self
            .args
            .as_ref()
            .expect("SimpleWithDefaults signatures must define positional-or-keyword parameters");

        // Hot path for the common shape:
        //   def f(a, b=...)
        //   f(a=...)
        // This avoids namespace prefill and the generic required/default scans.
        if args_params.len() == 2 && self.arg_defaults_count == 1 && keyword_id == args_params[0] {
            namespace.push(value);
            namespace.push(defaults[0].clone_with_heap(heap));
            return Ok(());
        }

        let param_count = args_params.len();
        namespace.resize_with(param_count, || Value::Undefined);

        let func = interns.get_str(func_name.name_id);
        let Some(param_idx) = args_params.iter().position(|&param_id| param_id == keyword_id) else {
            value.drop_with_heap(heap);
            for bound in namespace.drain(..) {
                bound.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_unexpected_keyword(
                func,
                interns.get_str(keyword_id),
            ));
        };

        namespace[param_idx] = value;

        // Apply defaults for optional parameters that are still unbound.
        let first_optional = param_count.saturating_sub(self.arg_defaults_count);
        for slot in first_optional..param_count {
            if matches!(namespace[slot], Value::Undefined) {
                namespace[slot] = defaults[slot - first_optional].clone_with_heap(heap);
            }
        }

        // Validate required parameters after defaults are applied.
        let required = self.required_positional_count();
        let mut missing_positional: Vec<&str> = Vec::new();
        for slot in 0..required {
            if matches!(namespace[slot], Value::Undefined) {
                missing_positional.push(interns.get_str(args_params[slot]));
            }
        }
        if !missing_positional.is_empty() {
            for bound in namespace.drain(..) {
                bound.drop_with_heap(heap);
            }
            return Err(ExcType::type_error_missing_positional_with_names(
                func,
                &missing_positional,
            ));
        }

        Ok(())
    }

    /// Creates an error for wrong number of arguments.
    ///
    /// Handles both "missing required positional arguments" and "too many arguments" cases,
    /// formatting the error message to match CPython's style.
    ///
    /// # Arguments
    /// * `actual_count` - Number of arguments actually provided
    /// * `interns` - String storage for looking up interned names
    fn wrong_arg_count_error<T>(&self, actual_count: usize, interns: &Interns, func_name: Identifier) -> RunResult<T> {
        let name_str = interns.get_str(func_name.name_id);
        let param_count = self.param_count();
        let msg = if let Some(missing_count) = param_count.checked_sub(actual_count) {
            // Missing arguments - show actual parameter names
            let mut msg = format!(
                "{}() missing {} required positional argument{}: ",
                name_str,
                missing_count,
                if missing_count == 1 { "" } else { "s" }
            );
            // Collect parameter names, skipping the ones already provided
            let mut missing_names: Vec<_> = self
                .param_names()
                .skip(actual_count)
                .map(|string_id| format!("'{}'", interns.get_str(string_id)))
                .collect();
            let last = missing_names.pop().unwrap();
            if !missing_names.is_empty() {
                msg.push_str(&missing_names.join(", "));
                msg.push_str(", and ");
            }
            msg.push_str(&last);
            msg
        } else {
            // Too many arguments
            format!(
                "{}() takes {} positional argument{} but {} {} given",
                name_str,
                param_count,
                if param_count == 1 { "" } else { "s" },
                actual_count,
                if actual_count == 1 { "was" } else { "were" }
            )
        };
        Err(SimpleException::new_msg(ExcType::TypeError, msg)
            .with_position(func_name.position)
            .into())
    }
}

/// Cleans up bound values when returning an error from `bind()`.
///
/// This function properly decrements reference counts for all heap-allocated
/// values that were bound during argument processing but need to be discarded
/// due to an error (e.g., missing required argument).
fn cleanup_on_error(
    namespace: &mut [Value],
    var_args_value: Option<Value>,
    excess_kwargs: Dict,
    heap: &mut Heap<impl ResourceTracker>,
) {
    // Clean up values in namespace
    for slot in namespace.iter_mut() {
        let value = std::mem::replace(slot, Value::Undefined);
        value.drop_with_heap(heap);
    }
    // Clean up *args tuple if allocated
    if let Some(val) = var_args_value {
        val.drop_with_heap(heap);
    }
    // Clean up excess kwargs dict contents (keys and values)
    for (key, value) in excess_kwargs {
        key.drop_with_heap(heap);
        value.drop_with_heap(heap);
    }
}
