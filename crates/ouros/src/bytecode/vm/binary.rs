//! Binary and in-place operation helpers for the VM.
//!
//! Binary operations follow the Python dunder protocol:
//! 1. Try the native `py_add`/`py_sub`/etc. on Value (fast path for builtins)
//! 2. If that returns `None`, check for instance dunder methods (`__add__`/`__radd__`/etc.)
//! 3. If the dunder returns `NotImplemented`, try the reflected dunder on the other operand
//!
//! The return type is `Result<CallResult, RunError>` because dunder methods may push
//! a new call frame (user-defined Python functions).

use super::{VM, call::CallResult};
use crate::{
    exception_private::{ExcType, RunError},
    heap::{HeapData, HeapGuard},
    intern::StaticStrings,
    io::PrintWriter,
    resource::ResourceTracker,
    tracer::VmTracer,
    types::{LongInt, PyTrait},
    value::{BitwiseOp, Value},
};

/// Helper macro for binary ops with dunder fallback.
///
/// Tries the native Value operation first, then falls back to dunder dispatch.
/// Returns `CallResult` to support frame-pushing dunder calls.
macro_rules! binary_op_with_dunder {
    ($self:expr, $py_op:ident, $op_str:expr, $dunder:expr, $reflected:expr $(, $extra_arg:expr)*) => {{
        let rhs = $self.pop();
        let lhs = $self.pop();

        // Fast path: try native operation
        match lhs.$py_op(&rhs, $self.heap $(, $extra_arg)*) {
            Ok(Some(v)) => {
                // Check for NotImplemented return from instance dunder that was called
                // inside py_op (shouldn't happen for native types, but be safe)
                lhs.drop_with_heap($self.heap);
                rhs.drop_with_heap($self.heap);
                Ok(CallResult::Push(v))
            }
            Ok(None) => {
                // Native op returned None - try dunder dispatch on instances
                let dunder_id: crate::intern::StringId = $dunder.into();
                let reflected_id: Option<crate::intern::StringId> = $reflected.map(|s: StaticStrings| s.into());
                let mut rhs_guard = HeapGuard::new(rhs, $self);
                let (rhs, this) = rhs_guard.as_parts();
                let mut lhs_guard = HeapGuard::new(lhs, this);
                let (lhs, this) = lhs_guard.as_parts();

                match this.try_binary_dunder(lhs, rhs, dunder_id, reflected_id)? {
                    Some(result) => Ok(result),
                    None => {
                        let lhs_type = lhs.py_type(this.heap);
                        let rhs_type = rhs.py_type(this.heap);
                        Err(ExcType::binary_type_error($op_str, lhs_type, rhs_type))
                    }
                }
            }
            Err(e) => {
                lhs.drop_with_heap($self.heap);
                rhs.drop_with_heap($self.heap);
                Err(e.into())
            }
        }
    }};
}

/// Helper macro for in-place ops with dunder fallback.
///
/// Tries the native in-place operation, then native binary, then dunder dispatch.
macro_rules! inplace_op_with_dunder {
    ($self:expr, $py_op:ident, $py_inplace:ident, $op_str:expr,
     $inplace_dunder:expr, $dunder:expr, $reflected:expr $(, $extra_arg:expr)*) => {{
        let rhs = $self.pop();
        let lhs = $self.pop();

        // Fast path: try native operation
        match lhs.$py_op(&rhs, $self.heap $(, $extra_arg)*) {
            Ok(Some(v)) => {
                lhs.drop_with_heap($self.heap);
                rhs.drop_with_heap($self.heap);
                Ok(CallResult::Push(v))
            }
            Ok(None) => {
                // Native op returned None - try dunder dispatch
                let inplace_id: crate::intern::StringId = $inplace_dunder.into();
                let dunder_id: crate::intern::StringId = $dunder.into();
                let reflected_id: Option<crate::intern::StringId> = $reflected.map(|s: StaticStrings| s.into());
                let mut rhs_guard = HeapGuard::new(rhs, $self);
                let (rhs, this) = rhs_guard.as_parts();
                let mut lhs_guard = HeapGuard::new(lhs, this);
                let (lhs, this) = lhs_guard.as_parts();

                match this.try_inplace_dunder(lhs, rhs, inplace_id, dunder_id, reflected_id)? {
                    Some(result) => Ok(result),
                    None => {
                        let lhs_type = lhs.py_type(this.heap);
                        let rhs_type = rhs.py_type(this.heap);
                        Err(ExcType::binary_type_error($op_str, lhs_type, rhs_type))
                    }
                }
            }
            Err(e) => {
                lhs.drop_with_heap($self.heap);
                rhs.drop_with_heap($self.heap);
                Err(e.into())
            }
        }
    }};
}

impl<T: ResourceTracker, P: PrintWriter, Tr: VmTracer> VM<'_, T, P, Tr> {
    /// Binary addition with dunder fallback.
    ///
    /// Includes a fast path for `Int + Int` that bypasses dunder dispatch and the
    /// `py_add` match entirely. Handles overflow by promoting to `LongInt`.
    pub(super) fn binary_add(&mut self) -> Result<CallResult, RunError> {
        // Fast path: peek at top two stack values for Int + Int.
        // This avoids py_add dispatch and dunder lookup in the common case
        // (e.g. `fib(n-1) + fib(n-2)` in numeric-heavy code).
        let len = self.stack.len();
        if len >= 2
            && let (Value::Int(a), Value::Int(b)) = (&self.stack[len - 2], &self.stack[len - 1])
        {
            let result = if let Some(v) = a.checked_add(*b) {
                Value::Int(v)
            } else {
                // Overflow: promote to LongInt
                let li = LongInt::from(*a) + LongInt::from(*b);
                li.into_value(self.heap)?
            };
            self.stack.truncate(len - 2);
            return Ok(CallResult::Push(result));
        }

        binary_op_with_dunder!(
            self,
            py_add,
            "+",
            StaticStrings::DunderAdd,
            Some(StaticStrings::DunderRadd),
            self.interns
        )
    }

    /// Binary subtraction with dunder fallback.
    ///
    /// Includes a fast path for `Int - Int` that bypasses dunder dispatch and the
    /// `py_sub` match entirely. Handles overflow by promoting to `LongInt`.
    /// Also handles set difference (`set - set`) which requires access to `interns`.
    pub(super) fn binary_sub(&mut self) -> Result<CallResult, RunError> {
        // Fast path: peek at top two stack values for Int - Int.
        // This avoids py_sub dispatch and dunder lookup in the common case
        // (e.g. `fib(n - 1)` in recursive numeric code).
        let len = self.stack.len();
        if len >= 2
            && let (Value::Int(a), Value::Int(b)) = (&self.stack[len - 2], &self.stack[len - 1])
        {
            let result = if let Some(v) = a.checked_sub(*b) {
                Value::Int(v)
            } else {
                // Overflow: promote to LongInt
                let li = LongInt::from(*a) - LongInt::from(*b);
                li.into_value(self.heap)?
            };
            self.stack.truncate(len - 2);
            return Ok(CallResult::Push(result));
        }

        // Check for set difference before falling back to py_sub.
        // Set difference requires interns for the contains check, which py_sub doesn't have.
        let rhs = self.pop();
        let lhs = self.pop();

        if let Some(result) = crate::value::py_set_difference(&lhs, &rhs, self.heap, self.interns)? {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(CallResult::Push(result));
        }

        if let Some(result) = crate::value::py_counter_subtract(&lhs, &rhs, self.heap, self.interns)? {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            return Ok(CallResult::Push(result));
        }

        // Fall back to py_sub for numeric types
        match lhs.py_sub(&rhs, self.heap) {
            Ok(Some(result)) => {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                Ok(CallResult::Push(result))
            }
            Ok(None) => {
                // Try dunder dispatch
                let dunder_id: crate::intern::StringId = StaticStrings::DunderSub.into();
                let reflected_id: Option<crate::intern::StringId> = Some(StaticStrings::DunderRsub.into());

                if let Some(result) = self.try_binary_dunder(&lhs, &rhs, dunder_id, reflected_id)? {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    Ok(result)
                } else {
                    let lhs_type = lhs.py_type(self.heap);
                    let rhs_type = rhs.py_type(self.heap);
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    Err(ExcType::binary_type_error("-", lhs_type, rhs_type))
                }
            }
            Err(e) => {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                Err(e.into())
            }
        }
    }

    /// Binary multiplication with dunder fallback.
    pub(super) fn binary_mult(&mut self) -> Result<CallResult, RunError> {
        binary_op_with_dunder!(
            self,
            py_mult,
            "*",
            StaticStrings::DunderMul,
            Some(StaticStrings::DunderRmul),
            self.interns
        )
    }

    /// Binary division with dunder fallback.
    pub(super) fn binary_div(&mut self) -> Result<CallResult, RunError> {
        binary_op_with_dunder!(
            self,
            py_div,
            "/",
            StaticStrings::DunderTruediv,
            Some(StaticStrings::DunderRtruediv),
            self.interns
        )
    }

    /// Binary floor division with dunder fallback.
    pub(super) fn binary_floordiv(&mut self) -> Result<CallResult, RunError> {
        binary_op_with_dunder!(
            self,
            py_floordiv,
            "//",
            StaticStrings::DunderFloordiv,
            Some(StaticStrings::DunderRfloordiv)
        )
    }

    /// Binary modulo with dunder fallback.
    pub(super) fn binary_mod(&mut self) -> Result<CallResult, RunError> {
        binary_op_with_dunder!(
            self,
            py_mod,
            "%",
            StaticStrings::DunderMod,
            Some(StaticStrings::DunderRmod)
        )
    }

    /// Binary power with dunder fallback.
    #[inline(never)]
    pub(super) fn binary_pow(&mut self) -> Result<CallResult, RunError> {
        binary_op_with_dunder!(
            self,
            py_pow,
            "** or pow()",
            StaticStrings::DunderPow,
            Some(StaticStrings::DunderRpow)
        )
    }

    /// Binary matmul (@) with dunder dispatch.
    pub(super) fn binary_matmul(&mut self) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        // No native py_matmul - go straight to dunder
        let dunder_id: crate::intern::StringId = StaticStrings::DunderMatmul.into();
        let reflected_id: Option<crate::intern::StringId> = Some(StaticStrings::DunderRmatmul.into());

        if let Some(result) = self.try_binary_dunder(&lhs, &rhs, dunder_id, reflected_id)? {
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            Ok(result)
        } else {
            let lhs_type = lhs.py_type(self.heap);
            let rhs_type = rhs.py_type(self.heap);
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
            Err(ExcType::binary_type_error("@", lhs_type, rhs_type))
        }
    }

    /// Binary bitwise operation with dunder fallback.
    pub(super) fn binary_bitwise(&mut self, op: BitwiseOp) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        match lhs.py_bitwise(&rhs, op, self.heap, self.interns) {
            Ok(v) => {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                Ok(CallResult::Push(v))
            }
            Err(e) => {
                // Only try dunder dispatch for TypeError (unsupported operand types).
                // Propagate all other errors (MemoryError, ValueError, etc.) immediately.
                let is_type_error = matches!(&e,
                    RunError::Exc(exc) if exc.exc.exc_type() == ExcType::TypeError
                );
                if !is_type_error {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    return Err(e);
                }

                // Try dunder dispatch for instances
                let (dunder, reflected) = match op {
                    BitwiseOp::And => (StaticStrings::DunderAnd, Some(StaticStrings::DunderRand)),
                    BitwiseOp::Or => (StaticStrings::DunderOr, Some(StaticStrings::DunderRor)),
                    BitwiseOp::Xor => (StaticStrings::DunderXor, Some(StaticStrings::DunderRxor)),
                    BitwiseOp::LShift => (StaticStrings::DunderLshift, Some(StaticStrings::DunderRlshift)),
                    BitwiseOp::RShift => (StaticStrings::DunderRshift, Some(StaticStrings::DunderRrshift)),
                };
                let dunder_id: crate::intern::StringId = dunder.into();
                let reflected_id: Option<crate::intern::StringId> = reflected.map(std::convert::Into::into);

                if let Some(result) = self.try_binary_dunder(&lhs, &rhs, dunder_id, reflected_id)? {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    Ok(result)
                } else {
                    let lhs_type = lhs.py_type(self.heap);
                    let rhs_type = rhs.py_type(self.heap);
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    let op_str = match op {
                        BitwiseOp::And => "&",
                        BitwiseOp::Or => "|",
                        BitwiseOp::Xor => "^",
                        BitwiseOp::LShift => "<<",
                        BitwiseOp::RShift => ">>",
                    };
                    Err(ExcType::binary_type_error(op_str, lhs_type, rhs_type))
                }
            }
        }
    }

    /// In-place addition with dunder fallback.
    ///
    /// Includes a fast path for `Int += Int` that bypasses `HeapGuard`, `py_iadd`,
    /// and dunder dispatch entirely. Handles overflow by promoting to `LongInt`.
    pub(super) fn inplace_add(&mut self) -> Result<CallResult, RunError> {
        // Fast path: Int += Int avoids HeapGuard, py_iadd dispatch, and cloning.
        // This is a hot path in loop counters (e.g. `i += 1`).
        let len = self.stack.len();
        if len >= 2
            && let (Value::Int(a), Value::Int(b)) = (&self.stack[len - 2], &self.stack[len - 1])
        {
            let result = if let Some(v) = a.checked_add(*b) {
                Value::Int(v)
            } else {
                let li = LongInt::from(*a) + LongInt::from(*b);
                li.into_value(self.heap)?
            };
            self.stack.truncate(len - 2);
            return Ok(CallResult::Push(result));
        }

        let rhs = self.pop();
        let mut lhs_guard = HeapGuard::new(self.pop(), self);
        let (lhs, this) = lhs_guard.as_parts_mut();

        // Try in-place operation first (for mutable types like lists)
        if lhs.py_iadd(rhs.clone_with_heap(this.heap), this.heap, lhs.ref_id(), this.interns)? {
            let (lhs, this) = lhs_guard.into_parts();
            rhs.drop_with_heap(this.heap);
            return Ok(CallResult::Push(lhs));
        }

        // Next try regular addition
        if let Some(v) = lhs.py_add(&rhs, this.heap, this.interns)? {
            rhs.drop_with_heap(this.heap);
            return Ok(CallResult::Push(v));
        }

        // Release the guard before calling dunder (needs &mut self)
        let (lhs, this) = lhs_guard.into_parts();

        // Try dunder dispatch
        let inplace_id: crate::intern::StringId = StaticStrings::DunderIadd.into();
        let dunder_id: crate::intern::StringId = StaticStrings::DunderAdd.into();
        let reflected_id: Option<crate::intern::StringId> = Some(StaticStrings::DunderRadd.into());

        let mut rhs_guard = HeapGuard::new(rhs, this);
        let (rhs, this) = rhs_guard.as_parts();
        let mut lhs_guard = HeapGuard::new(lhs, this);
        let (lhs, this) = lhs_guard.as_parts();

        if let Some(result) = this.try_inplace_dunder(lhs, rhs, inplace_id, dunder_id, reflected_id)? {
            Ok(result)
        } else {
            let lhs_type = lhs.py_type(this.heap);
            let rhs_type = rhs.py_type(this.heap);
            Err(ExcType::binary_type_error("+=", lhs_type, rhs_type))
        }
    }

    /// In-place subtraction with dunder fallback.
    ///
    /// Includes a fast path for `Int -= Int` that bypasses dunder dispatch and the
    /// `py_sub` match entirely. Handles overflow by promoting to `LongInt`.
    pub(super) fn inplace_sub(&mut self) -> Result<CallResult, RunError> {
        // Fast path: Int -= Int avoids py_sub dispatch and dunder lookup.
        let len = self.stack.len();
        if len >= 2
            && let (Value::Int(a), Value::Int(b)) = (&self.stack[len - 2], &self.stack[len - 1])
        {
            let result = if let Some(v) = a.checked_sub(*b) {
                Value::Int(v)
            } else {
                let li = LongInt::from(*a) - LongInt::from(*b);
                li.into_value(self.heap)?
            };
            self.stack.truncate(len - 2);
            return Ok(CallResult::Push(result));
        }

        // Counter -= Counter follows Counter subtraction semantics and returns a Counter.
        if len >= 2 {
            let lhs = self.stack[len - 2].clone_with_heap(self.heap);
            let rhs = self.stack[len - 1].clone_with_heap(self.heap);
            if let Some(result) = crate::value::py_counter_subtract(&lhs, &rhs, self.heap, self.interns)? {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                self.stack.truncate(len - 2);
                return Ok(CallResult::Push(result));
            }
            lhs.drop_with_heap(self.heap);
            rhs.drop_with_heap(self.heap);
        }

        inplace_op_with_dunder!(
            self,
            py_sub,
            py_sub,
            "-=",
            StaticStrings::DunderIsub,
            StaticStrings::DunderSub,
            Some(StaticStrings::DunderRsub)
        )
    }

    /// In-place multiplication with dunder fallback.
    pub(super) fn inplace_mul(&mut self) -> Result<CallResult, RunError> {
        inplace_op_with_dunder!(
            self,
            py_mult,
            py_mult,
            "*=",
            StaticStrings::DunderImul,
            StaticStrings::DunderMul,
            Some(StaticStrings::DunderRmul),
            self.interns
        )
    }

    /// In-place division with dunder fallback.
    pub(super) fn inplace_div(&mut self) -> Result<CallResult, RunError> {
        inplace_op_with_dunder!(
            self,
            py_div,
            py_div,
            "/=",
            StaticStrings::DunderItruediv,
            StaticStrings::DunderTruediv,
            Some(StaticStrings::DunderRtruediv),
            self.interns
        )
    }

    /// In-place floor division with dunder fallback.
    pub(super) fn inplace_floordiv(&mut self) -> Result<CallResult, RunError> {
        inplace_op_with_dunder!(
            self,
            py_floordiv,
            py_floordiv,
            "//=",
            StaticStrings::DunderIfloordiv,
            StaticStrings::DunderFloordiv,
            Some(StaticStrings::DunderRfloordiv)
        )
    }

    /// In-place modulo with dunder fallback.
    pub(super) fn inplace_mod(&mut self) -> Result<CallResult, RunError> {
        inplace_op_with_dunder!(
            self,
            py_mod,
            py_mod,
            "%=",
            StaticStrings::DunderImod,
            StaticStrings::DunderMod,
            Some(StaticStrings::DunderRmod)
        )
    }

    /// In-place power with dunder fallback.
    pub(super) fn inplace_pow(&mut self) -> Result<CallResult, RunError> {
        inplace_op_with_dunder!(
            self,
            py_pow,
            py_pow,
            "**=",
            StaticStrings::DunderIpow,
            StaticStrings::DunderPow,
            Some(StaticStrings::DunderRpow)
        )
    }

    /// In-place bitwise operation with dunder fallback.
    pub(super) fn inplace_bitwise(&mut self, op: BitwiseOp) -> Result<CallResult, RunError> {
        let rhs = self.pop();
        let lhs = self.pop();

        // Native dict in-place merge (`dict |= dict`) mutates lhs in place.
        if matches!(op, BitwiseOp::Or)
            && let (Value::Ref(lhs_id), Value::Ref(rhs_id)) = (&lhs, &rhs)
            && matches!(self.heap.get(*lhs_id), HeapData::Dict(_))
            && matches!(self.heap.get(*rhs_id), HeapData::Dict(_))
        {
            let interns = self.interns;
            let rhs_items = match self.heap.with_entry_mut(*rhs_id, |heap_inner, data| match data {
                HeapData::Dict(dict) => Ok(dict.items(heap_inner)),
                _ => Err(RunError::internal("inplace_bitwise: rhs expected dict")),
            }) {
                Ok(items) => items,
                Err(err) => {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    return Err(err);
                }
            };

            let replaced_values = match self.heap.with_entry_mut(*lhs_id, |heap_inner, data| match data {
                HeapData::Dict(dict) => {
                    let mut replaced = Vec::new();
                    let mut incoming = rhs_items.into_iter();
                    while let Some((key, value)) = incoming.next() {
                        match dict.set(key, value, heap_inner, interns) {
                            Ok(Some(old)) => replaced.push(old),
                            Ok(None) => {}
                            Err(err) => {
                                for old in replaced {
                                    old.drop_with_heap(heap_inner);
                                }
                                for (pending_key, pending_value) in incoming {
                                    pending_key.drop_with_heap(heap_inner);
                                    pending_value.drop_with_heap(heap_inner);
                                }
                                return Err(err);
                            }
                        }
                    }
                    Ok(replaced)
                }
                _ => Err(RunError::internal("inplace_bitwise: lhs expected dict")),
            }) {
                Ok(values) => values,
                Err(err) => {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    return Err(err);
                }
            };

            for old in replaced_values {
                old.drop_with_heap(self.heap);
            }
            rhs.drop_with_heap(self.heap);
            return Ok(CallResult::Push(lhs));
        }

        match lhs.py_bitwise(&rhs, op, self.heap, self.interns) {
            Ok(v) => {
                lhs.drop_with_heap(self.heap);
                rhs.drop_with_heap(self.heap);
                Ok(CallResult::Push(v))
            }
            Err(e) => {
                // Only try dunder dispatch for TypeError (unsupported operand types).
                // Propagate all other errors (MemoryError, ValueError, etc.) immediately.
                let is_type_error = matches!(&e,
                    RunError::Exc(exc) if exc.exc.exc_type() == ExcType::TypeError
                );
                if !is_type_error {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    return Err(e);
                }

                // Try dunder dispatch
                let (inplace_dunder, dunder, reflected) = match op {
                    BitwiseOp::And => (
                        StaticStrings::DunderIand,
                        StaticStrings::DunderAnd,
                        Some(StaticStrings::DunderRand),
                    ),
                    BitwiseOp::Or => (
                        StaticStrings::DunderIor,
                        StaticStrings::DunderOr,
                        Some(StaticStrings::DunderRor),
                    ),
                    BitwiseOp::Xor => (
                        StaticStrings::DunderIxor,
                        StaticStrings::DunderXor,
                        Some(StaticStrings::DunderRxor),
                    ),
                    BitwiseOp::LShift => (
                        StaticStrings::DunderIlshift,
                        StaticStrings::DunderLshift,
                        Some(StaticStrings::DunderRlshift),
                    ),
                    BitwiseOp::RShift => (
                        StaticStrings::DunderIrshift,
                        StaticStrings::DunderRshift,
                        Some(StaticStrings::DunderRrshift),
                    ),
                };
                let inplace_id: crate::intern::StringId = inplace_dunder.into();
                let dunder_id: crate::intern::StringId = dunder.into();
                let reflected_id: Option<crate::intern::StringId> = reflected.map(std::convert::Into::into);

                if let Some(result) = self.try_inplace_dunder(&lhs, &rhs, inplace_id, dunder_id, reflected_id)? {
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    Ok(result)
                } else {
                    let lhs_type = lhs.py_type(self.heap);
                    let rhs_type = rhs.py_type(self.heap);
                    lhs.drop_with_heap(self.heap);
                    rhs.drop_with_heap(self.heap);
                    let op_str = match op {
                        BitwiseOp::And => "&=",
                        BitwiseOp::Or => "|=",
                        BitwiseOp::Xor => "^=",
                        BitwiseOp::LShift => "<<=",
                        BitwiseOp::RShift => ">>=",
                    };
                    Err(ExcType::binary_type_error(op_str, lhs_type, rhs_type))
                }
            }
        }
    }
}
