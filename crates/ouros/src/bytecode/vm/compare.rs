//! Comparison operation helpers for the VM.
//!
//! Comparisons support dunder protocols: when comparing instances, the VM looks
//! up `__eq__`/`__ne__`/`__lt__`/`__le__`/`__gt__`/`__ge__` on the type.

use super::{
    PendingCompareDispatch, PendingCompareDunder, PendingCompareKind, PendingCompareSide, PendingCompareStep,
    PendingTruthinessKind, VM, call::CallResult,
};
use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunError},
    heap::{HeapData, HeapId},
    intern::{StaticStrings, StringId},
    io::PrintWriter,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{LongInt, PyTrait},
    value::Value,
};

#[derive(Clone, Copy)]
struct CompareDispatchCandidate {
    step: PendingCompareStep,
    class_id: HeapId,
    owner_id: HeapId,
}

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

        // Try rich comparison dunder dispatch first.
        if let Some(result) = self.try_instance_compare_dunder(
            &lhs,
            &rhs,
            PendingCompareKind::Eq,
            StaticStrings::DunderEq,
            Some(StaticStrings::DunderEq),
        )? {
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

        // Try __ne__ first.
        if let Some(result) = self.try_instance_compare_dunder(
            &lhs,
            &rhs,
            PendingCompareKind::NePrimary,
            StaticStrings::DunderNe,
            Some(StaticStrings::DunderNe),
        )? {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(result);
        }

        // CPython fallback for `!=`: if __ne__ is not implemented, negate __eq__.
        if let Some(result) = self.try_instance_compare_dunder(
            &lhs,
            &rhs,
            PendingCompareKind::NeEqFallback,
            StaticStrings::DunderEq,
            Some(StaticStrings::DunderEq),
        )? {
            let final_result = self.finish_compare_result(PendingCompareKind::NeEqFallback, result)?;
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(final_result);
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
            PendingCompareKind::Lt,
            StaticStrings::DunderLt,
            Some(StaticStrings::DunderGt),
            std::cmp::Ordering::is_lt,
        )
    }

    pub(super) fn compare_le(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            PendingCompareKind::Le,
            StaticStrings::DunderLe,
            Some(StaticStrings::DunderGe),
            std::cmp::Ordering::is_le,
        )
    }

    pub(super) fn compare_gt(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            PendingCompareKind::Gt,
            StaticStrings::DunderGt,
            Some(StaticStrings::DunderLt),
            std::cmp::Ordering::is_gt,
        )
    }

    pub(super) fn compare_ge(&mut self) -> Result<CallResult, RunError> {
        self.compare_ord_dunder(
            PendingCompareKind::Ge,
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
        kind: PendingCompareKind,
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
        if let Some(result) = self.try_instance_compare_dunder(&lhs, &rhs, kind, lhs_dunder, rhs_dunder)? {
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
        kind: PendingCompareKind,
        lhs_dunder: StaticStrings,
        rhs_dunder: Option<StaticStrings>,
    ) -> Result<Option<CallResult>, RunError> {
        let Some(mut dispatch) = self.build_compare_dispatch(lhs, rhs, kind, lhs_dunder, rhs_dunder) else {
            return Ok(None);
        };

        let first_step = dispatch.first.expect("compare dispatch missing first step");
        let first_result = self.call_compare_step(lhs, rhs, first_step)?;
        match first_result {
            CallResult::FramePushed => {
                dispatch.next_step = 1;
                self.pending_compare_dunder.push(PendingCompareDunder {
                    lhs: lhs.clone_with_heap(self.heap),
                    rhs: rhs.clone_with_heap(self.heap),
                    dispatch,
                    frame_depth: self.frames.len(),
                });
                Ok(Some(CallResult::FramePushed))
            }
            other => {
                if let Some(result) = self.comparison_result_if_implemented(other) {
                    return Ok(Some(result));
                }
                if let Some(second_step) = dispatch.second {
                    let second_result = self.call_compare_step(lhs, rhs, second_step)?;
                    return match second_result {
                        CallResult::FramePushed => {
                            dispatch.next_step = 2;
                            self.pending_compare_dunder.push(PendingCompareDunder {
                                lhs: lhs.clone_with_heap(self.heap),
                                rhs: rhs.clone_with_heap(self.heap),
                                dispatch,
                                frame_depth: self.frames.len(),
                            });
                            Ok(Some(CallResult::FramePushed))
                        }
                        other => Ok(self.comparison_result_if_implemented(other)),
                    };
                }
                Ok(None)
            }
        }
    }

    /// Resumes comparison protocol after a frame-pushed dunder returned.
    pub(super) fn resume_pending_compare_dunder(
        &mut self,
        mut pending: PendingCompareDunder,
        value: Value,
    ) -> Result<CallResult, RunError> {
        let mut result = if matches!(value, Value::NotImplemented) {
            value.drop_with_heap(self.heap);
            None
        } else {
            Some(CallResult::Push(value))
        };

        if result.is_none()
            && pending.dispatch.next_step == 1
            && let Some(second_step) = pending.dispatch.second
        {
            let second_result = self.call_compare_step(&pending.lhs, &pending.rhs, second_step)?;
            match second_result {
                CallResult::FramePushed => {
                    pending.dispatch.next_step = 2;
                    pending.frame_depth = self.frames.len();
                    self.pending_compare_dunder.push(pending);
                    return Ok(CallResult::FramePushed);
                }
                other => {
                    result = self.comparison_result_if_implemented(other);
                }
            }
        }

        let final_result = if let Some(result) = result {
            self.finish_compare_result(pending.dispatch.kind, result)?
        } else {
            self.compare_notimplemented_fallback(pending.dispatch.kind, &pending.lhs, &pending.rhs)?
        };

        pending.lhs.drop_with_heap(self.heap);
        pending.rhs.drop_with_heap(self.heap);
        Ok(final_result)
    }

    fn compare_notimplemented_fallback(
        &mut self,
        kind: PendingCompareKind,
        lhs: &Value,
        rhs: &Value,
    ) -> Result<CallResult, RunError> {
        match kind {
            PendingCompareKind::Eq => Ok(CallResult::Push(Value::Bool(lhs.py_eq(rhs, self.heap, self.interns)))),
            PendingCompareKind::NePrimary => {
                if let Some(result) = self.try_instance_compare_dunder(
                    lhs,
                    rhs,
                    PendingCompareKind::NeEqFallback,
                    StaticStrings::DunderEq,
                    Some(StaticStrings::DunderEq),
                )? {
                    return self.finish_compare_result(PendingCompareKind::NeEqFallback, result);
                }
                Ok(CallResult::Push(Value::Bool(!lhs.py_eq(rhs, self.heap, self.interns))))
            }
            PendingCompareKind::NeEqFallback => {
                Ok(CallResult::Push(Value::Bool(!lhs.py_eq(rhs, self.heap, self.interns))))
            }
            PendingCompareKind::Lt | PendingCompareKind::Le | PendingCompareKind::Gt | PendingCompareKind::Ge => {
                Err(Self::compare_type_error(
                    Self::compare_symbol_for_kind(kind),
                    lhs.py_type(self.heap),
                    rhs.py_type(self.heap),
                ))
            }
        }
    }

    fn finish_compare_result(&mut self, kind: PendingCompareKind, result: CallResult) -> Result<CallResult, RunError> {
        match kind {
            PendingCompareKind::NeEqFallback => self.negate_compare_result(result),
            _ => Ok(result),
        }
    }

    fn negate_compare_result(&mut self, result: CallResult) -> Result<CallResult, RunError> {
        match result {
            CallResult::Push(value) => self.negate_truthiness_value(value),
            other => Ok(other),
        }
    }

    fn negate_truthiness_value(&mut self, value: Value) -> Result<CallResult, RunError> {
        if let Value::Ref(id) = &value
            && matches!(self.heap.get(*id), HeapData::Instance(_))
        {
            let id = *id;
            let bool_id: StringId = StaticStrings::DunderBool.into();
            if let Some(method) = self.lookup_type_dunder(id, bool_id) {
                let result = self.call_dunder(id, method, ArgValues::Empty)?;
                value.drop_with_heap(self.heap);
                return match result {
                    CallResult::Push(bool_value) => {
                        let truthy = self.truthiness_from_bool_dunder_return(&bool_value)?;
                        bool_value.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(!truthy)))
                    }
                    CallResult::FramePushed => {
                        self.push_pending_truthiness_return(PendingTruthinessKind::Bool, true);
                        Ok(CallResult::FramePushed)
                    }
                    other => Ok(other),
                };
            }

            let len_id: StringId = StaticStrings::DunderLen.into();
            if let Some(method) = self.lookup_type_dunder(id, len_id) {
                let result = self.call_dunder(id, method, ArgValues::Empty)?;
                value.drop_with_heap(self.heap);
                return match result {
                    CallResult::Push(len_value) => {
                        let truthy = self.truthiness_from_len_dunder_return(&len_value)?;
                        len_value.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(!truthy)))
                    }
                    CallResult::FramePushed => {
                        self.push_pending_truthiness_return(PendingTruthinessKind::Len, true);
                        Ok(CallResult::FramePushed)
                    }
                    other => Ok(other),
                };
            }
        }

        let truthy = value.py_bool(self.heap, self.interns);
        value.drop_with_heap(self.heap);
        Ok(CallResult::Push(Value::Bool(!truthy)))
    }

    fn build_compare_dispatch(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        kind: PendingCompareKind,
        lhs_dunder: StaticStrings,
        rhs_dunder: Option<StaticStrings>,
    ) -> Option<PendingCompareDispatch> {
        let lhs_candidate = self.lookup_compare_dispatch_candidate(lhs, PendingCompareSide::Lhs, lhs_dunder);
        let rhs_candidate =
            rhs_dunder.and_then(|dunder| self.lookup_compare_dispatch_candidate(rhs, PendingCompareSide::Rhs, dunder));

        let (first, second) = match (lhs_candidate, rhs_candidate) {
            (None, None) => return None,
            (Some(lhs_cand), None) => (Some(lhs_cand.step), None),
            (None, Some(rhs_cand)) => (Some(rhs_cand.step), None),
            (Some(lhs_cand), Some(rhs_cand)) => {
                let rhs_subclass_priority = self.is_strict_subclass(rhs_cand.class_id, lhs_cand.class_id)
                    && rhs_cand.owner_id != lhs_cand.owner_id;
                if rhs_subclass_priority {
                    (Some(rhs_cand.step), Some(lhs_cand.step))
                } else if lhs_cand.class_id == rhs_cand.class_id
                    && matches!(
                        kind,
                        PendingCompareKind::Eq | PendingCompareKind::NePrimary | PendingCompareKind::NeEqFallback
                    )
                {
                    (Some(lhs_cand.step), None)
                } else {
                    (Some(lhs_cand.step), Some(rhs_cand.step))
                }
            }
        };

        Some(PendingCompareDispatch {
            kind,
            first,
            second,
            next_step: 0,
        })
    }

    fn lookup_compare_dispatch_candidate(
        &mut self,
        operand: &Value,
        side: PendingCompareSide,
        dunder: StaticStrings,
    ) -> Option<CompareDispatchCandidate> {
        let Value::Ref(instance_id) = operand else {
            return None;
        };
        let HeapData::Instance(instance) = self.heap.get(*instance_id) else {
            return None;
        };
        let class_id = instance.class_id();
        let HeapData::ClassObject(class_obj) = self.heap.get(class_id) else {
            return None;
        };
        let dunder_name_id: StringId = dunder.into();
        let dunder_name = self.interns.get_str(dunder_name_id);
        let (method, owner_id) = class_obj.mro_lookup_attr(dunder_name, class_id, self.heap, self.interns)?;
        method.drop_with_heap(self.heap);

        Some(CompareDispatchCandidate {
            step: PendingCompareStep { side, dunder },
            class_id,
            owner_id,
        })
    }

    fn call_compare_step(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        step: PendingCompareStep,
    ) -> Result<CallResult, RunError> {
        match step.side {
            PendingCompareSide::Lhs => {
                let Value::Ref(lhs_id) = lhs else {
                    return Ok(CallResult::Push(Value::NotImplemented));
                };
                if !matches!(self.heap.get(*lhs_id), HeapData::Instance(_)) {
                    return Ok(CallResult::Push(Value::NotImplemented));
                }
                let dunder_id: StringId = step.dunder.into();
                let Some(method) = self.lookup_type_dunder(*lhs_id, dunder_id) else {
                    return Ok(CallResult::Push(Value::NotImplemented));
                };
                let rhs_clone = rhs.clone_with_heap(self.heap);
                self.call_dunder(*lhs_id, method, ArgValues::One(rhs_clone))
            }
            PendingCompareSide::Rhs => {
                let Value::Ref(rhs_id) = rhs else {
                    return Ok(CallResult::Push(Value::NotImplemented));
                };
                if !matches!(self.heap.get(*rhs_id), HeapData::Instance(_)) {
                    return Ok(CallResult::Push(Value::NotImplemented));
                }
                let dunder_id: StringId = step.dunder.into();
                let Some(method) = self.lookup_type_dunder(*rhs_id, dunder_id) else {
                    return Ok(CallResult::Push(Value::NotImplemented));
                };
                let lhs_clone = lhs.clone_with_heap(self.heap);
                self.call_dunder(*rhs_id, method, ArgValues::One(lhs_clone))
            }
        }
    }

    pub(super) fn is_strict_subclass(&self, candidate_subclass: HeapId, candidate_base: HeapId) -> bool {
        if candidate_subclass == candidate_base {
            return false;
        }
        match self.heap.get(candidate_subclass) {
            HeapData::ClassObject(cls) => cls.is_subclass_of(candidate_subclass, candidate_base),
            _ => false,
        }
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

    fn compare_symbol_for_kind(kind: PendingCompareKind) -> &'static str {
        match kind {
            PendingCompareKind::Lt => "<",
            PendingCompareKind::Le => "<=",
            PendingCompareKind::Gt => ">",
            PendingCompareKind::Ge => ">=",
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

        // CPython fallback for instances: __iter__ first, then __getitem__ sequence protocol.
        if let Value::Ref(container_id) = &container
            && matches!(self.heap.get(*container_id), HeapData::Instance(_))
        {
            let iter_id: StringId = StaticStrings::DunderIter.into();
            if let Some(iter_method) = self.lookup_type_dunder(*container_id, iter_id) {
                let list_result = match self.call_dunder(*container_id, iter_method, ArgValues::Empty) {
                    Ok(CallResult::Push(iterator)) => self.list_build_from_iterator(iterator),
                    Ok(CallResult::FramePushed) => {
                        self.pending_list_iter_return = true;
                        Ok(CallResult::FramePushed)
                    }
                    Ok(other) => Ok(other),
                    Err(e) => Err(e),
                };
                return match list_result {
                    Ok(CallResult::Push(materialized)) => {
                        let contained = materialized.py_contains(&item, self.heap, self.interns)?;
                        materialized.drop_with_heap(self.heap);
                        item.drop_with_heap(self.heap);
                        container.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(if negate {
                            !contained
                        } else {
                            contained
                        })))
                    }
                    Ok(CallResult::FramePushed) => {
                        container.drop_with_heap(self.heap);
                        self.pending_builtin_from_list.push(super::PendingBuiltinFromList {
                            kind: super::PendingBuiltinFromListKind::Contains { needle: item, negate },
                        });
                        Ok(CallResult::FramePushed)
                    }
                    Ok(other) => {
                        item.drop_with_heap(self.heap);
                        container.drop_with_heap(self.heap);
                        Ok(other)
                    }
                    Err(e) => {
                        item.drop_with_heap(self.heap);
                        container.drop_with_heap(self.heap);
                        Err(e)
                    }
                };
            }

            let getitem_id: StringId = StaticStrings::DunderGetitem.into();
            let getitem_name = self.interns.get_str(getitem_id);
            if self.type_mro_has_attr(*container_id, getitem_name) {
                let list_result = self.list_build_from_iterator(container.clone_with_heap(self.heap));
                return match list_result {
                    Ok(CallResult::Push(materialized)) => {
                        let contained = materialized.py_contains(&item, self.heap, self.interns)?;
                        materialized.drop_with_heap(self.heap);
                        item.drop_with_heap(self.heap);
                        container.drop_with_heap(self.heap);
                        Ok(CallResult::Push(Value::Bool(if negate {
                            !contained
                        } else {
                            contained
                        })))
                    }
                    Ok(CallResult::FramePushed) => {
                        container.drop_with_heap(self.heap);
                        self.pending_builtin_from_list.push(super::PendingBuiltinFromList {
                            kind: super::PendingBuiltinFromListKind::Contains { needle: item, negate },
                        });
                        Ok(CallResult::FramePushed)
                    }
                    Ok(other) => {
                        item.drop_with_heap(self.heap);
                        container.drop_with_heap(self.heap);
                        Ok(other)
                    }
                    Err(e) => {
                        item.drop_with_heap(self.heap);
                        container.drop_with_heap(self.heap);
                        Err(e)
                    }
                };
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
