//! Comparison operation helpers for the VM.
//!
//! Comparisons support dunder protocols: when comparing instances, the VM looks
//! up `__eq__`/`__ne__`/`__lt__`/`__le__`/`__gt__`/`__ge__` on the type.

use super::{VM, call::CallResult};
use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunError},
    heap::HeapData,
    intern::{StaticStrings, StringId},
    io::PrintWriter,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{LongInt, PyTrait},
    value::Value,
};

impl<T: ResourceTracker, P: PrintWriter, Tr: VmTracer> VM<'_, T, P, Tr> {
    /// Equality comparison with dunder support.
    ///
    /// Includes a fast path for `Int == Int` that bypasses dunder dispatch entirely,
    /// since integers are immediate values with no custom `__eq__`.
    pub(super) fn compare_eq(&mut self) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        // Fast path: Int == Int avoids dunder lookup and py_eq overhead
        if let (Value::Int(a), Value::Int(b)) = (&lhs, &rhs) {
            return Ok(CallResult::Push(Value::Bool(*a == *b)));
        }

        // Try rich comparison dunder dispatch first: lhs.__eq__(rhs), then rhs.__eq__(lhs).
        if let Some(result) =
            self.try_instance_compare_dunder(&lhs, &rhs, StaticStrings::DunderEq, Some(StaticStrings::DunderEq))?
        {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(result);
        }

        // Fast path: native comparison
        let result = lhs.py_eq(&rhs, self.heap, self.interns);
        lhs.drop_with_heap(self.heap);
        rhs.drop_with_heap(self.heap);
        Ok(CallResult::Push(Value::Bool(result)))
    }

    /// Inequality comparison with dunder support.
    ///
    /// Includes a fast path for `Int != Int` that bypasses dunder dispatch entirely,
    /// since integers are immediate values with no custom `__ne__`.
    pub(super) fn compare_ne(&mut self) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        // Fast path: Int != Int avoids dunder lookup and py_eq overhead
        if let (Value::Int(a), Value::Int(b)) = (&lhs, &rhs) {
            return Ok(CallResult::Push(Value::Bool(*a != *b)));
        }

        // Try rich comparison dunder dispatch first: lhs.__ne__(rhs), then rhs.__ne__(lhs).
        if let Some(result) =
            self.try_instance_compare_dunder(&lhs, &rhs, StaticStrings::DunderNe, Some(StaticStrings::DunderNe))?
        {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(result);
        }

        // CPython fallback for `!=`: if __ne__ is not implemented, negate __eq__.
        if let Some(result) =
            self.try_instance_compare_dunder(&lhs, &rhs, StaticStrings::DunderEq, Some(StaticStrings::DunderEq))?
        {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return match result {
                CallResult::Push(v) => {
                    let bool_val = v.py_bool(self.heap, self.interns);
                    v.drop_with_heap(self.heap);
                    Ok(CallResult::Push(Value::Bool(!bool_val)))
                }
                CallResult::FramePushed => {
                    self.pending_negate_bool = true;
                    Ok(CallResult::FramePushed)
                }
                other => Ok(other),
            };
        }

        // Fast path: native comparison
        let result = !lhs.py_eq(&rhs, self.heap, self.interns);
        lhs.drop_with_heap(self.heap);
        rhs.drop_with_heap(self.heap);
        Ok(CallResult::Push(Value::Bool(result)))
    }

    /// Ordering comparison with dunder support.
    pub(super) fn compare_lt(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            StaticStrings::DunderLt,
            Some(StaticStrings::DunderGt),
            std::cmp::Ordering::is_lt,
        )
    }

    pub(super) fn compare_le(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            StaticStrings::DunderLe,
            Some(StaticStrings::DunderGe),
            std::cmp::Ordering::is_le,
        )
    }

    pub(super) fn compare_gt(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            StaticStrings::DunderGt,
            Some(StaticStrings::DunderLt),
            std::cmp::Ordering::is_gt,
        )
    }

    pub(super) fn compare_ge(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            StaticStrings::DunderGe,
            Some(StaticStrings::DunderLe),
            std::cmp::Ordering::is_ge,
        )
    }

    /// Ordering comparison helper with dunder fallback.
    ///
    /// Includes a fast path for `Int cmp Int` that bypasses dunder lookup and `py_cmp`
    /// entirely, since integers are immediate values with well-defined ordering and no
    /// custom comparison dunders.
    fn compare_ord_dunder(
        &mut self,
        lhs_dunder: StaticStrings,
        rhs_dunder: Option<StaticStrings>,
        check: fn(std::cmp::Ordering) -> bool,
    ) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        // Fast path: Int cmp Int avoids dunder lookup and py_cmp overhead.
        // This is the hot path for loops with integer counters (e.g. `while i <= n`).
        if let (Value::Int(a), Value::Int(b)) = (&lhs, &rhs) {
            let result = check(a.cmp(b));
            return Ok(CallResult::Push(Value::Bool(result)));
        }

        // Try rich comparison dunder dispatch first:
        // lhs.__op__(rhs), then rhs.__rop__(lhs).
        if let Some(result) = self.try_instance_compare_dunder(&lhs, &rhs, lhs_dunder, rhs_dunder)? {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(result);
        }

        // Slow path: native ordering via py_cmp.
        // If no native ordering exists, CPython raises TypeError.
        let result = lhs.py_cmp(&rhs, self.heap, self.interns);
        if let Some(ordering) = result {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(CallResult::Push(Value::Bool(check(ordering))));
        }
        let lhs_type = lhs.py_type(self.heap);
        let rhs_type = rhs.py_type(self.heap);
        lhs.drop_with_heap(self.heap);
        rhs.drop_with_heap(self.heap);
        Err(Self::compare_type_error(
            Self::compare_symbol(lhs_dunder),
            lhs_type,
            rhs_type,
        ))
    }

    /// Try to dispatch a comparison via instance dunders.
    ///
    /// The VM tries `lhs_dunder` on the left operand first. If the result is
    /// `NotImplemented`, it tries `rhs_dunder` on the right operand (when provided).
    /// Returns `None` when no applicable dunder exists or all attempted dunders
    /// return `NotImplemented`.
    fn try_instance_compare_dunder(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        lhs_dunder: StaticStrings,
        rhs_dunder: Option<StaticStrings>,
    ) -> Result<Option<CallResult>, RunError> {
        let lhs_dunder_id = lhs_dunder.into();

        // Try lhs.__op__(rhs).
        if let Value::Ref(lhs_id) = lhs
            && matches!(self.heap.get(*lhs_id), HeapData::Instance(_))
            && let Some(method) = self.lookup_type_dunder(*lhs_id, lhs_dunder_id)
        {
            let rhs_clone = rhs.clone_with_heap(self.heap);
            let result = self.call_dunder(*lhs_id, method, ArgValues::One(rhs_clone))?;
            if let Some(result) = self.comparison_result_if_implemented(result) {
                return Ok(Some(result));
            }
        }

        // Try rhs.__rop__(lhs) when provided.
        if let Some(rhs_dunder) = rhs_dunder {
            let rhs_dunder_id = rhs_dunder.into();
            if let Value::Ref(rhs_id) = rhs
                && matches!(self.heap.get(*rhs_id), HeapData::Instance(_))
                && let Some(method) = self.lookup_type_dunder(*rhs_id, rhs_dunder_id)
            {
                let lhs_clone = lhs.clone_with_heap(self.heap);
                let result = self.call_dunder(*rhs_id, method, ArgValues::One(lhs_clone))?;
                if let Some(result) = self.comparison_result_if_implemented(result) {
                    return Ok(Some(result));
                }
            }
        }

        Ok(None)
    }

    /// Returns `None` when a comparison dunder returned `NotImplemented`.
    ///
    /// `NotImplemented` must be consumed and dropped so the caller can attempt
    /// reflected dispatch or fallback comparison semantics.
    fn comparison_result_if_implemented(&mut self, result: CallResult) -> Option<CallResult> {
        match result {
            CallResult::Push(v) if matches!(v, Value::NotImplemented) => {
                v.drop_with_heap(self.heap);
                None
            }
            other => Some(other),
        }
    }

    /// Returns the display symbol for a comparison dunder.
    fn compare_symbol(dunder: StaticStrings) -> &'static str {
        match dunder {
            StaticStrings::DunderLt => "<",
            StaticStrings::DunderLe => "<=",
            StaticStrings::DunderGt => ">",
            StaticStrings::DunderGe => ">=",
            _ => "<",
        }
    }

    /// Builds CPython-compatible unsupported-ordering TypeErrors.
    fn compare_type_error(op: &str, lhs_type: crate::types::Type, rhs_type: crate::types::Type) -> RunError {
        ExcType::type_error(format!(
            "'{op}' not supported between instances of '{lhs_type}' and '{rhs_type}'"
        ))
    }

    /// Identity comparison (is/is not).
    pub(super) fn compare_is(&mut self, negate: bool) {
        let rhs = self.pop();
        let lhs = self.pop();
        let result = lhs.is(&rhs);
        lhs.drop_with_heap(self.heap);
        rhs.drop_with_heap(self.heap);
        self.push(Value::Bool(if negate { !result } else { result }));
    }

    /// Membership test (in/not in) with dunder support.
    pub(super) fn compare_in(&mut self, negate: bool) -> Result<CallResult, RunError> {
        let container = self.pop();
        let item = self.pop();

        // Try __contains__ dunder on instance
        if let Value::Ref(container_id) = &container
            && matches!(self.heap.get(*container_id), HeapData::Instance(_))
        {
            let dunder_id = StaticStrings::DunderContains.into();
            if let Some(method) = self.lookup_type_dunder(*container_id, dunder_id) {
                let item_clone = item.clone_with_heap(self.heap);
                // __contains__ takes (self, item), returns bool
                // We need to negate if this is 'not in'
                // Store the negate flag so the caller can handle it
                // Actually we can't easily negate after FramePushed.
                // For now, call the dunder and let the result be post-processed.
                // The issue: if it returns FramePushed, we can't negate.
                // Solution: we handle 'not in' by wrapping the result in a separate step.
                // For 'in', just return the result.
                // For 'not in', we need to negate after the frame returns.
                // This is complex, so let's handle the sync case directly.
                let result = self.call_dunder(*container_id, method, ArgValues::One(item_clone))?;
                item.drop_with_heap(self.heap);
                container.drop_with_heap(self.heap);

                if negate {
                    match result {
                        CallResult::Push(v) => {
                            let bool_val = v.py_bool(self.heap, self.interns);
                            v.drop_with_heap(self.heap);
                            return Ok(CallResult::Push(Value::Bool(!bool_val)));
                        }
                        CallResult::FramePushed => {
                            // Set flag to negate the return value when frame returns
                            self.pending_negate_bool = true;
                            return Ok(CallResult::FramePushed);
                        }
                        other => return Ok(other),
                    }
                }

                return Ok(result);
            }
        }

        // Native containment check
        let result = container.py_contains(&item, self.heap, self.interns);
        item.drop_with_heap(self.heap);
        container.drop_with_heap(self.heap);

        let contained = result?;
        Ok(CallResult::Push(Value::Bool(if negate {
            !contained
        } else {
            contained
        })))
    }

    /// Modulo equality comparison: a % b == k
    ///
    /// Returns `CallResult` to support dunder dispatch. When both operands are
    /// native types, returns `Push(Bool)`. When an instance dunder pushes a frame,
    /// the caller must handle the FramePushed + pending_mod_eq_k flow.
    pub(super) fn compare_mod_eq(&mut self, k: &Value) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        // Fastest path for the hot `Int % Int == Int` shape used in tight loops.
        // This avoids the generic `py_mod_eq` dispatch and preserves Python's
        // modulo sign semantics (remainder has divisor sign).
        if let Value::Int(k_val) = k
            && let (Value::Int(v1), Value::Int(v2)) = (&lhs, &rhs)
        {
            if *v2 == 0 {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                return Err(ExcType::zero_division().into());
            }
            let r = *v1 % *v2;
            let result = if r != 0 && (*v1 < 0) != (*v2 < 0) { r + *v2 } else { r };
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(CallResult::Push(Value::Bool(result == *k_val)));
        }

        // Try fast path for remaining Int/Float native types.
        let mod_result = match k {
            Value::Int(k_val) => lhs.py_mod_eq(&rhs, *k_val),
            _ => None,
        };

        if let Some(is_equal) = mod_result {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(CallResult::Push(Value::Bool(is_equal)));
        }

        let mod_value = lhs.py_mod(&rhs, self.heap);

        match mod_value {
            Ok(Some(v)) => {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                let (k_value, k_needs_drop) = if let Value::InternLongInt(id) = k {
                    let bi = self.interns.get_long_int(*id).clone();
                    (LongInt::new(bi).into_value(self.heap)?, true)
                } else {
                    (k.copy_for_extend(), false)
                };

                let is_equal = v.py_eq(&k_value, self.heap, self.interns);
                v.drop_with_heap(self.heap);
                if k_needs_drop {
                    k_value.drop_with_heap(self.heap);
                }
                Ok(CallResult::Push(Value::Bool(is_equal)))
            }
            Ok(None) => {
                // Native mod returned None - try __mod__/__rmod__ dunder dispatch
                let dunder_id: StringId = StaticStrings::DunderMod.into();
                let reflected_id: Option<StringId> = Some(StaticStrings::DunderRmod.into());

                if let Some(result) = self.try_binary_dunder(&lhs, &rhs, dunder_id, reflected_id)? {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    // If result is Push, do the == k comparison inline.
                    // If FramePushed, caller must set pending_mod_eq_k.
                    match result {
                        CallResult::Push(mod_val) => {
                            let (k_value, k_needs_drop) = if let Value::InternLongInt(id) = k {
                                let bi = self.interns.get_long_int(*id).clone();
                                (LongInt::new(bi).into_value(self.heap)?, true)
                            } else {
                                (k.copy_for_extend(), false)
                            };
                            let is_equal = mod_val.py_eq(&k_value, self.heap, self.interns);
                            mod_val.drop_with_heap(self.heap);
                            if k_needs_drop {
                                k_value.drop_with_heap(self.heap);
                            }
                            Ok(CallResult::Push(Value::Bool(is_equal)))
                        }
                        CallResult::FramePushed => Ok(CallResult::FramePushed),
                        other => Ok(other),
                    }
                } else {
                    let lhs_type = lhs.py_type(self.heap);
                    let rhs_type = rhs.py_type(self.heap);
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    Err(ExcType::binary_type_error("%", lhs_type, rhs_type))
                }
            }
            Err(e) => {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                Err(e)
            }
        }
    }
}
