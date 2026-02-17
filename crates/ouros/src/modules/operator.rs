//! Implementation of the `operator` module.
//!
//! Provides function equivalents of Python's built-in operators:
//! - Arithmetic: `add`, `sub`, `mul`, `truediv`, `floordiv`, `mod`, `neg`, `abs`, `pow`, `pos`, `matmul`
//! - Comparison: `eq`, `ne`, `lt`, `le`, `gt`, `ge`
//! - Identity: `is_`, `is_not`, `is_none`, `is_not_none`
//! - Boolean: `not_`, `truth`
//! - Bitwise: `and_`, `or_`, `xor`, `inv`, `invert`, `lshift`, `rshift`
//! - Container: `getitem`, `setitem`, `delitem`, `contains`, `concat`, `countOf`, `indexOf`, `length_hint`
//! - Indexing: `index`
//! - In-place: `iadd`, `isub`, `imul`, `itruediv`, `ifloordiv`, `imod`, `iand`, `ior`, `ixor`, `ilshift`, `irshift`,
//!   `ipow`, `imatmul`, `iconcat`
//! - Callable factories: `itemgetter`, `attrgetter`, `methodcaller`
//!
//! These functions delegate to the same Value methods the VM uses internally.

use num_traits::Signed;

use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{AttrCallResult, AttrGetter, ItemGetter, LongInt, MethodCaller, Module, PyTrait},
    value::{BitwiseOp, Value},
};

/// Operator module functions.
///
/// Each variant maps to a Python `operator` module function.
/// These provide functional equivalents of Python's built-in operators,
/// enabling them to be used as first-class callables (e.g. passed to `map()` or `sorted()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum OperatorFunctions {
    Call,
    Add,
    Sub,
    Mul,
    Truediv,
    Floordiv,
    Mod,
    Neg,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    #[strum(serialize = "is_")]
    Is,
    #[strum(serialize = "is_not")]
    IsNot,
    #[strum(serialize = "is_none")]
    IsNone,
    #[strum(serialize = "is_not_none")]
    IsNotNone,
    #[strum(serialize = "not_")]
    Not,
    Truth,
    Abs,
    Index,
    Getitem,
    Setitem,
    Delitem,
    Contains,
    #[strum(serialize = "length_hint")]
    LengthHint,
    Itemgetter,
    Attrgetter,
    Methodcaller,
    Pow,
    Pos,
    #[strum(serialize = "and_")]
    And,
    #[strum(serialize = "or_")]
    Or,
    Xor,
    #[strum(serialize = "inv")]
    Inv,
    Invert,
    Matmul,
    Lshift,
    Rshift,
    Concat,
    Iconcat,
    #[strum(serialize = "countOf")]
    CountOf,
    #[strum(serialize = "indexOf")]
    IndexOf,
    Iadd,
    Isub,
    Imul,
    Itruediv,
    Ifloordiv,
    Imod,
    Iand,
    Ior,
    Ixor,
    Ilshift,
    Irshift,
    Ipow,
    Imatmul,
}

/// Creates the `operator` module and allocates it on the heap.
///
/// Sets up all operator functions that provide functional equivalents
/// to Python's built-in operators.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut module = Module::new(StaticStrings::Operator);

    macro_rules! reg {
        ($name:expr, $func:ident) => {
            module.set_attr(
                $name,
                Value::ModuleFunction(ModuleFunctions::Operator(OperatorFunctions::$func)),
                heap,
                interns,
            );
        };
    }

    // Arithmetic operations
    reg!(StaticStrings::OperatorCall, Call);
    reg!(StaticStrings::Add, Add);
    reg!(StaticStrings::ReSub, Sub);
    reg!(StaticStrings::OperatorMul, Mul);
    reg!(StaticStrings::OperatorTruediv, Truediv);
    reg!(StaticStrings::OperatorFloordiv, Floordiv);
    reg!(StaticStrings::OperatorMod, Mod);
    reg!(StaticStrings::OperatorNeg, Neg);
    reg!(StaticStrings::MathPow, Pow);
    reg!(StaticStrings::OperatorPos, Pos);
    reg!(StaticStrings::OperatorMatmul, Matmul);

    // Comparison operations
    reg!(StaticStrings::OperatorEq, Eq);
    reg!(StaticStrings::OperatorNe, Ne);
    reg!(StaticStrings::OperatorLt, Lt);
    reg!(StaticStrings::OperatorLe, Le);
    reg!(StaticStrings::OperatorGt, Gt);
    reg!(StaticStrings::OperatorGe, Ge);

    // Identity operations
    reg!(StaticStrings::OperatorIs, Is);
    reg!(StaticStrings::OperatorIsNot, IsNot);
    reg!(StaticStrings::OperatorIsNone, IsNone);
    reg!(StaticStrings::OperatorIsNotNone, IsNotNone);

    // Boolean operations
    reg!(StaticStrings::OperatorNot, Not);
    reg!(StaticStrings::OperatorTruth, Truth);

    // Bitwise operations
    reg!(StaticStrings::OperatorAnd, And);
    reg!(StaticStrings::OperatorOr, Or);
    reg!(StaticStrings::OperatorXor, Xor);
    reg!(StaticStrings::OperatorInv, Inv);
    reg!(StaticStrings::OperatorInvert, Invert);
    reg!(StaticStrings::OperatorLshift, Lshift);
    reg!(StaticStrings::OperatorRshift, Rshift);

    // Other operations
    reg!(StaticStrings::Abs, Abs);
    reg!(StaticStrings::Index, Index);
    reg!(StaticStrings::OperatorGetitem, Getitem);
    reg!(StaticStrings::OperatorSetitem, Setitem);
    reg!(StaticStrings::OperatorDelitem, Delitem);
    reg!(StaticStrings::OperatorContains, Contains);
    reg!(StaticStrings::OperatorConcat, Concat);
    reg!(StaticStrings::OperatorCountOf, CountOf);
    reg!(StaticStrings::OperatorIndexOf, IndexOf);
    reg!(StaticStrings::OperatorLengthHint, LengthHint);
    reg!(StaticStrings::OperatorItemgetter, Itemgetter);
    reg!(StaticStrings::OperatorAttrgetter, Attrgetter);
    reg!(StaticStrings::OperatorMethodcaller, Methodcaller);

    // In-place operations
    reg!(StaticStrings::OperatorIadd, Iadd);
    reg!(StaticStrings::OperatorIsub, Isub);
    reg!(StaticStrings::OperatorImul, Imul);
    reg!(StaticStrings::OperatorItruediv, Itruediv);
    reg!(StaticStrings::OperatorIfloordiv, Ifloordiv);
    reg!(StaticStrings::OperatorImod, Imod);
    reg!(StaticStrings::OperatorIand, Iand);
    reg!(StaticStrings::OperatorIor, Ior);
    reg!(StaticStrings::OperatorIxor, Ixor);
    reg!(StaticStrings::OperatorIlshift, Ilshift);
    reg!(StaticStrings::OperatorIrshift, Irshift);
    reg!(StaticStrings::OperatorIpow, Ipow);
    reg!(StaticStrings::OperatorImatmul, Imatmul);
    reg!(StaticStrings::OperatorIconcat, Iconcat);

    heap.allocate(crate::heap::HeapData::Module(module))
}

/// Dispatches a call to an operator module function.
///
/// All operator functions return immediate values (no host involvement needed).
/// Some functions (matmul, imatmul, index, length_hint) may return `AttrCallResult::CallFunction`
/// to support calling dunder methods that require VM execution.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: OperatorFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    if function == OperatorFunctions::Call {
        return op_call(heap, args);
    }

    // Handle functions that may need to call dunder methods via VM
    match function {
        OperatorFunctions::Matmul => return op_matmul_impl(heap, interns, args),
        OperatorFunctions::Imatmul => return op_imatmul_impl(heap, interns, args),
        OperatorFunctions::Index => return op_index_impl(heap, interns, args),
        OperatorFunctions::LengthHint => return op_length_hint_impl(heap, interns, args),
        _ => {}
    }

    let result = match function {
        OperatorFunctions::Call => unreachable!("handled above"),
        OperatorFunctions::Add => op_add(heap, interns, args),
        OperatorFunctions::Sub => op_sub(heap, interns, args),
        OperatorFunctions::Mul => op_mul(heap, interns, args),
        OperatorFunctions::Truediv => op_truediv(heap, interns, args),
        OperatorFunctions::Floordiv => op_floordiv(heap, interns, args),
        OperatorFunctions::Mod => op_mod(heap, interns, args),
        OperatorFunctions::Neg => op_neg(heap, interns, args),
        OperatorFunctions::Eq => op_eq(heap, interns, args),
        OperatorFunctions::Ne => op_ne(heap, interns, args),
        OperatorFunctions::Lt => op_lt(heap, interns, args),
        OperatorFunctions::Le => op_le(heap, interns, args),
        OperatorFunctions::Gt => op_gt(heap, interns, args),
        OperatorFunctions::Ge => op_ge(heap, interns, args),
        OperatorFunctions::Is => op_is(heap, interns, args),
        OperatorFunctions::IsNot => op_is_not(heap, interns, args),
        OperatorFunctions::IsNone => op_is_none(heap, interns, args),
        OperatorFunctions::IsNotNone => op_is_not_none(heap, interns, args),
        OperatorFunctions::Not => op_not(heap, interns, args),
        OperatorFunctions::Truth => op_truth(heap, interns, args),
        OperatorFunctions::Abs => op_abs(heap, interns, args),
        OperatorFunctions::Index => unreachable!("handled above"),
        OperatorFunctions::Getitem => op_getitem(heap, interns, args),
        OperatorFunctions::Setitem => op_setitem(heap, interns, args),
        OperatorFunctions::Delitem => op_delitem(heap, interns, args),
        OperatorFunctions::Contains => op_contains(heap, interns, args),
        OperatorFunctions::LengthHint => unreachable!("handled above"),
        OperatorFunctions::Itemgetter => op_itemgetter(heap, interns, args),
        OperatorFunctions::Attrgetter => op_attrgetter(heap, interns, args),
        OperatorFunctions::Methodcaller => op_methodcaller(heap, interns, args),
        OperatorFunctions::Pow => op_pow(heap, interns, args),
        OperatorFunctions::Pos => op_pos(heap, interns, args),
        OperatorFunctions::And => op_and(heap, interns, args),
        OperatorFunctions::Or => op_or(heap, interns, args),
        OperatorFunctions::Xor => op_xor(heap, interns, args),
        OperatorFunctions::Inv => op_invert(heap, interns, args),
        OperatorFunctions::Invert => op_invert(heap, interns, args),
        OperatorFunctions::Matmul => unreachable!("handled above"),
        OperatorFunctions::Lshift => op_lshift(heap, interns, args),
        OperatorFunctions::Rshift => op_rshift(heap, interns, args),
        OperatorFunctions::Concat => op_concat(heap, interns, args),
        OperatorFunctions::Iconcat => op_iconcat(heap, interns, args),
        OperatorFunctions::CountOf => op_count_of(heap, interns, args),
        OperatorFunctions::IndexOf => op_index_of(heap, interns, args),
        OperatorFunctions::Iadd => op_iadd(heap, interns, args),
        OperatorFunctions::Isub => op_isub(heap, interns, args),
        OperatorFunctions::Imul => op_imul(heap, interns, args),
        OperatorFunctions::Itruediv => op_itruediv(heap, interns, args),
        OperatorFunctions::Ifloordiv => op_ifloordiv(heap, interns, args),
        OperatorFunctions::Imod => op_imod(heap, interns, args),
        OperatorFunctions::Iand => op_iand(heap, interns, args),
        OperatorFunctions::Ior => op_ior(heap, interns, args),
        OperatorFunctions::Ixor => op_ixor(heap, interns, args),
        OperatorFunctions::Ilshift => op_ilshift(heap, interns, args),
        OperatorFunctions::Irshift => op_irshift(heap, interns, args),
        OperatorFunctions::Ipow => op_ipow(heap, interns, args),
        OperatorFunctions::Imatmul => unreachable!("handled above"),
    }?;
    Ok(AttrCallResult::Value(result))
}

/// Implementation of `operator.call(obj, /, *args, **kwargs)`.
///
/// This defers invocation to VM call dispatch so user-defined functions,
/// frame-pushing callables, and external calls all keep normal semantics.
fn op_call(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<AttrCallResult> {
    let (mut positional, kwargs) = args.into_parts();
    let Some(callable) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error("call expected at least 1 argument, got 0"));
    };

    let forwarded_pos: Vec<Value> = positional.collect();
    let forwarded = if forwarded_pos.is_empty() {
        if kwargs.is_empty() {
            ArgValues::Empty
        } else {
            ArgValues::Kwargs(kwargs)
        }
    } else if kwargs.is_empty() {
        match forwarded_pos.len() {
            1 => ArgValues::One(forwarded_pos.into_iter().next().expect("length checked")),
            2 => {
                let mut iter = forwarded_pos.into_iter();
                ArgValues::Two(
                    iter.next().expect("length checked"),
                    iter.next().expect("length checked"),
                )
            }
            _ => ArgValues::ArgsKargs {
                args: forwarded_pos,
                kwargs: crate::args::KwargsValues::Empty,
            },
        }
    } else {
        ArgValues::ArgsKargs {
            args: forwarded_pos,
            kwargs,
        }
    };

    Ok(AttrCallResult::CallFunction(callable, forwarded))
}

// ===== Arithmetic operations =====

/// Implementation of `operator.add(a, b)`. Returns `a + b`.
fn op_add(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.add", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_add(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for +: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Implementation of `operator.sub(a, b)`. Returns `a - b`.
fn op_sub(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.sub", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_sub(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for -: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Implementation of `operator.mul(a, b)`. Returns `a * b`.
fn op_mul(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.mul", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_mult(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for *: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.truediv(a, b)`. Returns `a / b`.
fn op_truediv(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.truediv", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_div(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for /: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.floordiv(a, b)`. Returns `a // b`.
fn op_floordiv(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.floordiv", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_floordiv(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for //: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.mod(a, b)`. Returns `a % b`.
fn op_mod(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.mod", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_mod(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for %: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.pow(a, b)`. Returns `a ** b`.
fn op_pow(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.pow", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_pow(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for ** or pow(): '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.matmul(a, b)`. Returns `a @ b`.
///
/// Attempts to call `__matmul__` on the left operand, then `__rmatmul__` on the right operand.
/// Returns `AttrCallResult` to support method calls that may need VM execution.
fn op_matmul_impl(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (a, b) = args.get_two_args("operator.matmul", heap)?;

    // Try a.__matmul__(b)
    if let Value::Ref(_) = a {
        let dunder_id: StringId = StaticStrings::DunderMatmul.into();
        // Use Value::py_getattr to get proper method binding (BoundMethod for instances)
        let method_result = a.py_getattr(dunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                a.drop_with_heap(heap);
                // The callable is already a BoundMethod with self=a, just pass b
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::One(b)));
            }
            Ok(other) => {
                // Other cases like PropertyCall, DescriptorGet, etc.
                a.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, continue to try rmatmul
            }
        }
    }

    // Try b.__rmatmul__(a)
    if let Value::Ref(_) = b {
        let rdunder_id: StringId = StaticStrings::DunderRmatmul.into();
        // Use Value::py_getattr to get proper method binding
        let method_result = b.py_getattr(rdunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                b.drop_with_heap(heap);
                // The callable is already a BoundMethod with self=b, just pass a
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::One(a)));
            }
            Ok(other) => {
                b.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, fall through to error
            }
        }
    }

    defer_drop!(a, heap);
    defer_drop!(b, heap);
    Err(ExcType::binary_type_error("@", a.py_type(heap), b.py_type(heap)))
}

/// Implementation of `operator.neg(a)`. Returns `-a`.
fn op_neg(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.neg", heap)?;
    defer_drop!(a, heap);
    match a {
        Value::Int(i) => Ok(Value::Int(-i)),
        Value::Float(f) => Ok(Value::Float(-f)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                Ok(LongInt::new(-li.inner().clone()).into_value(heap)?)
            } else if let HeapData::Fraction(fraction) = heap.get(*id) {
                Ok((-fraction.clone()).to_value(heap)?)
            } else {
                Err(ExcType::type_error(format!(
                    "bad operand type for unary -: '{}'",
                    a.py_type(heap)
                )))
            }
        }
        _ => Err(ExcType::type_error(format!(
            "bad operand type for unary -: '{}'",
            a.py_type(heap)
        ))),
    }
}

/// Implementation of `operator.pos(a)`. Returns `+a`.
///
/// For numeric types, this is an identity operation.
/// Booleans are converted to int, matching CPython's `__pos__` behavior.
fn op_pos(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.pos", heap)?;
    defer_drop!(a, heap);
    match a {
        Value::Int(_) | Value::Float(_) => Ok(a.clone_with_heap(heap)),
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Ref(id) => {
            if matches!(heap.get(*id), HeapData::LongInt(_)) {
                Ok(a.clone_with_heap(heap))
            } else {
                Err(ExcType::type_error(format!(
                    "bad operand type for unary +: '{}'",
                    a.py_type(heap)
                )))
            }
        }
        _ => Err(ExcType::type_error(format!(
            "bad operand type for unary +: '{}'",
            a.py_type(heap)
        ))),
    }
}

// ===== Comparison operations =====

/// Implementation of `operator.eq(a, b)`. Returns `a == b`.
fn op_eq(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.eq", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    Ok(Value::Bool(a.py_eq(b, heap, interns)))
}

/// Implementation of `operator.ne(a, b)`. Returns `a != b`.
fn op_ne(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ne", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    Ok(Value::Bool(!a.py_eq(b, heap, interns)))
}

/// Implementation of `operator.lt(a, b)`. Returns `a < b`.
fn op_lt(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.lt", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_cmp(b, heap, interns) {
        Some(ordering) => Ok(Value::Bool(ordering.is_lt())),
        None => Err(ExcType::type_error(format!(
            "'<' not supported between instances of '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
    }
}

/// Implementation of `operator.le(a, b)`. Returns `a <= b`.
fn op_le(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.le", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_cmp(b, heap, interns) {
        Some(ordering) => Ok(Value::Bool(ordering.is_le())),
        None => Err(ExcType::type_error(format!(
            "'<=' not supported between instances of '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
    }
}

/// Implementation of `operator.gt(a, b)`. Returns `a > b`.
fn op_gt(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.gt", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_cmp(b, heap, interns) {
        Some(ordering) => Ok(Value::Bool(ordering.is_gt())),
        None => Err(ExcType::type_error(format!(
            "'>' not supported between instances of '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
    }
}

/// Implementation of `operator.ge(a, b)`. Returns `a >= b`.
fn op_ge(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ge", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_cmp(b, heap, interns) {
        Some(ordering) => Ok(Value::Bool(ordering.is_ge())),
        None => Err(ExcType::type_error(format!(
            "'>=' not supported between instances of '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
    }
}

// ===== Identity operations =====

/// Implementation of `operator.is_(a, b)`. Returns `a is b`.
fn op_is(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.is_", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    Ok(Value::Bool(a.is(b)))
}

/// Implementation of `operator.is_not(a, b)`. Returns `a is not b`.
fn op_is_not(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.is_not", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    Ok(Value::Bool(!a.is(b)))
}

/// Implementation of `operator.is_none(a)`. Returns `a is None`.
fn op_is_none(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.is_none", heap)?;
    let is_none = matches!(a, Value::None);
    a.drop_with_heap(heap);
    Ok(Value::Bool(is_none))
}

/// Implementation of `operator.is_not_none(a)`. Returns `a is not None`.
fn op_is_not_none(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.is_not_none", heap)?;
    let is_not_none = !matches!(a, Value::None);
    a.drop_with_heap(heap);
    Ok(Value::Bool(is_not_none))
}

// ===== Boolean operations =====

/// Implementation of `operator.not_(a)`. Returns `not a`.
fn op_not(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.not_", heap)?;
    defer_drop!(a, heap);
    Ok(Value::Bool(!a.py_bool(heap, interns)))
}

/// Implementation of `operator.truth(a)`. Returns `bool(a)`.
fn op_truth(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.truth", heap)?;
    defer_drop!(a, heap);
    Ok(Value::Bool(a.py_bool(heap, interns)))
}

// ===== Bitwise operations =====

/// Implementation of `operator.and_(a, b)`. Returns `a & b` (bitwise AND).
fn op_and(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.and_", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::And, heap, interns)
}

/// Implementation of `operator.or_(a, b)`. Returns `a | b` (bitwise OR).
fn op_or(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.or_", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::Or, heap, interns)
}

/// Implementation of `operator.xor(a, b)`. Returns `a ^ b` (bitwise XOR).
fn op_xor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.xor", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::Xor, heap, interns)
}

/// Implementation of `operator.invert(a)`. Returns `~a` (bitwise NOT).
fn op_invert(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.invert", heap)?;
    defer_drop!(a, heap);
    match a {
        Value::Int(n) => Ok(Value::Int(!n)),
        Value::Bool(b) => Ok(Value::Int(!i64::from(*b))),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                Ok(LongInt::new(-(li.inner() + 1i32)).into_value(heap)?)
            } else {
                Err(ExcType::type_error(format!(
                    "bad operand type for unary ~: '{}'",
                    a.py_type(heap)
                )))
            }
        }
        _ => Err(ExcType::type_error(format!(
            "bad operand type for unary ~: '{}'",
            a.py_type(heap)
        ))),
    }
}

/// Implementation of `operator.lshift(a, b)`. Returns `a << b`.
fn op_lshift(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.lshift", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::LShift, heap, interns)
}

/// Implementation of `operator.rshift(a, b)`. Returns `a >> b`.
fn op_rshift(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.rshift", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::RShift, heap, interns)
}

// ===== Other operations =====

/// Implementation of `operator.abs(a)`. Returns `abs(a)`.
fn op_abs(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let a = args.get_one_arg("operator.abs", heap)?;
    defer_drop!(a, heap);
    match a {
        Value::Int(i) => Ok(Value::Int(i.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                Ok(LongInt::new(li.inner().abs().clone()).into_value(heap)?)
            } else if let HeapData::Fraction(fraction) = heap.get(*id) {
                Ok(fraction.abs().to_value(heap)?)
            } else {
                Err(ExcType::type_error(format!(
                    "bad operand type for abs(): '{}'",
                    a.py_type(heap)
                )))
            }
        }
        _ => Err(ExcType::type_error(format!(
            "bad operand type for abs(): '{}'",
            a.py_type(heap)
        ))),
    }
}

/// Implementation of `operator.index(a)`. Returns `a.__index__()`.
///
/// For int/bool/LongInt, returns the value directly.
/// For objects with `__index__` method, calls it and returns the result.
/// Returns `AttrCallResult` to support method calls that may need VM execution.
fn op_index_impl(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let a = args.get_one_arg("operator.index", heap)?;

    // Handle primitive types directly
    match &a {
        Value::Int(i) => {
            let result = Value::Int(*i);
            a.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(result));
        }
        Value::Bool(b) => {
            let result = Value::Int(i64::from(*b));
            a.drop_with_heap(heap);
            return Ok(AttrCallResult::Value(result));
        }
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                let result = Value::Ref(heap.allocate(HeapData::LongInt(LongInt::new(li.inner().clone())))?);
                a.drop_with_heap(heap);
                return Ok(AttrCallResult::Value(result));
            }
        }
        _ => {}
    }

    // Try calling __index__ method for objects that have it
    if let Value::Ref(_) = a {
        let dunder_id: StringId = StaticStrings::DunderIndex.into();
        // Use Value::py_getattr to get proper method binding
        let method_result = a.py_getattr(dunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                a.drop_with_heap(heap);
                // Call __index__() with no arguments (self is already bound)
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::Empty));
            }
            Ok(other) => {
                a.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, fall through to error
            }
        }
    }

    defer_drop!(a, heap);
    Err(ExcType::type_error(format!(
        "'{}' object cannot be interpreted as an integer",
        a.py_type(heap)
    )))
}

/// Implementation of `operator.getitem(obj, key)`. Returns `obj[key]`.
fn op_getitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (obj, key) = args.get_two_args("operator.getitem", heap)?;
    let mut obj = obj;
    let result = obj.py_getitem(&key, heap, interns);
    obj.drop_with_heap(heap);
    key.drop_with_heap(heap);
    result
}

/// Implementation of `operator.setitem(obj, key, value)`. Performs `obj[key] = value`.
fn op_setitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (obj, key, value) = args.get_three_args("operator.setitem", heap)?;
    let mut obj = obj;
    let result = obj.py_setitem(key, value, heap, interns);
    obj.drop_with_heap(heap);
    result?;
    Ok(Value::None)
}

/// Implementation of `operator.delitem(obj, key)`. Performs `del obj[key]`.
fn op_delitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (obj, key) = args.get_two_args("operator.delitem", heap)?;
    let mut obj = obj;
    let result = obj.py_delitem(key, heap, interns);
    obj.drop_with_heap(heap);
    result?;
    Ok(Value::None)
}

/// Implementation of `operator.contains(a, b)`. Returns `b in a`.
fn op_contains(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.contains", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    let result = a.py_contains(b, heap, interns)?;
    Ok(Value::Bool(result))
}

/// Implementation of `operator.length_hint(obj, default=0)`.
///
/// Returns `len(obj)` when available, else tries `obj.__length_hint__()`,
/// and finally falls back to `default`.
/// Returns `AttrCallResult` to support method calls that may need VM execution.
fn op_length_hint_impl(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (obj, default) = args.get_one_two_args("operator.length_hint", heap)?;
    let fallback = if let Some(default) = default {
        let value = default.as_int(heap)?;
        default.drop_with_heap(heap);
        value
    } else {
        0
    };

    // First try len(obj)
    let len = obj.py_len(heap, interns);
    if let Some(len) = len {
        obj.drop_with_heap(heap);
        let length = i64::try_from(len).unwrap_or(i64::MAX);
        return Ok(AttrCallResult::Value(Value::Int(length)));
    }

    // Try obj.__length_hint__() for objects that have it
    if let Value::Ref(_) = obj {
        let dunder_id: StringId = StaticStrings::DunderLengthHint.into();
        // Use Value::py_getattr to get proper method binding
        let method_result = obj.py_getattr(dunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                obj.drop_with_heap(heap);
                // Call __length_hint__() with no arguments (self is already bound)
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::Empty));
            }
            Ok(other) => {
                obj.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, fall through to default
            }
        }
    }

    obj.drop_with_heap(heap);
    Ok(AttrCallResult::Value(Value::Int(fallback)))
}

/// Wrapper for op_length_hint_impl that returns Value for the call() function.
fn op_length_hint(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    match op_length_hint_impl(heap, interns, args)? {
        AttrCallResult::Value(v) => Ok(v),
        _ => Err(ExcType::type_error("length_hint operation not supported".to_string())),
    }
}

/// Implementation of `operator.concat(a, b)`. Returns `a + b` for sequences.
fn op_concat(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.concat", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_add(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for +: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Implementation of `operator.iconcat(a, b)`. Returns `a += b` for sequences.
fn op_iconcat(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.iconcat", heap)?;
    defer_drop!(b, heap);
    let mut a = a;
    if a.py_iadd(b.clone_with_heap(heap), heap, a.ref_id(), interns)? {
        return Ok(a);
    }
    defer_drop!(a, heap);
    match a.py_add(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for +: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Implementation of `operator.countOf(a, b)`. Returns the number of occurrences of `b` in `a`.
fn op_count_of(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.countOf", heap)?;
    let count = count_occurrences(&a, &b, heap, interns)?;
    a.drop_with_heap(heap);
    b.drop_with_heap(heap);
    Ok(Value::Int(count))
}

/// Counts occurrences of `needle` in a container `haystack`.
///
/// For sequences (list, tuple), counts matching elements.
/// For strings, counts occurrences of substring.
/// Collects items first to avoid borrow conflicts between `heap.get()` and `py_eq()`.
fn count_occurrences(
    haystack: &Value,
    needle: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    // Handle string substring counting (for both interned and heap strings)
    let haystack_str = match haystack {
        Value::InternString(sid) => Some(interns.get_str(*sid)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    };

    if let Some(s) = haystack_str {
        // Extract needle string directly to avoid lifetime issues
        let needle_str = match needle {
            Value::InternString(sid) => interns.get_str(*sid),
            Value::Ref(nid) => match heap.get(*nid) {
                HeapData::Str(ns) => ns.as_str(),
                _ => {
                    return Err(ExcType::type_error(
                        "can't count occurrences of non-string in string".to_string(),
                    ));
                }
            },
            _ => {
                return Err(ExcType::type_error(
                    "can't count occurrences of non-string in string".to_string(),
                ));
            }
        };
        let count = s.matches(needle_str).count();
        return Ok(i64::try_from(count).unwrap_or(i64::MAX));
    }

    // Handle list and tuple
    let Value::Ref(id) = haystack else {
        return Err(ExcType::type_error(
            "operator.countOf requires an iterable as first argument".to_string(),
        ));
    };

    let items: Vec<Value> = match heap.get(*id) {
        HeapData::List(list) => list.as_vec().iter().map(Value::copy_for_extend).collect(),
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(Value::copy_for_extend).collect(),
        _ => {
            return Err(ExcType::type_error(
                "operator.countOf requires an iterable as first argument".to_string(),
            ));
        }
    };
    let count = items.iter().filter(|item| item.py_eq(needle, heap, interns)).count();
    Ok(i64::try_from(count).unwrap_or(i64::MAX))
}

/// Implementation of `operator.indexOf(a, b)`. Returns the index of first occurrence of `b` in `a`.
fn op_index_of(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.indexOf", heap)?;
    let index = find_index(&a, &b, heap, interns)?;
    a.drop_with_heap(heap);
    b.drop_with_heap(heap);
    Ok(Value::Int(index))
}

// ===== Callable factories =====

/// Implementation of `operator.itemgetter(*items)`.
///
/// Returns a callable that indexes its argument with the stored items.
fn op_itemgetter(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();

    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "itemgetter() takes no keyword arguments").into());
    }

    if positional.len() == 0 {
        positional.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "itemgetter expected 1 argument, got 0").into());
    }

    let items: Vec<Value> = positional.collect();
    let getter = ItemGetter::new(items);
    let id = heap.allocate(HeapData::ItemGetter(getter))?;
    Ok(Value::Ref(id))
}

/// Implementation of `operator.attrgetter(*attrs)`.
///
/// Returns a callable that retrieves the stored attribute names from its argument.
fn op_attrgetter(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();

    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "attrgetter() takes no keyword arguments").into());
    }

    if positional.len() == 0 {
        positional.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "attrgetter expected 1 argument, got 0").into());
    }

    let mut attrs: Vec<Value> = Vec::with_capacity(positional.len());
    while let Some(attr) = positional.next() {
        if attr.as_either_str(heap).is_none() {
            for value in attrs {
                value.drop_with_heap(heap);
            }
            attr.drop_with_heap(heap);
            positional.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::TypeError, "attribute name must be a string").into());
        }
        attrs.push(attr);
    }

    let getter = AttrGetter::new(attrs);
    let id = heap.allocate(HeapData::AttrGetter(getter))?;
    Ok(Value::Ref(id))
}

/// Implementation of `operator.methodcaller(name, *args, **kwargs)`.
///
/// Returns a callable that invokes the named method on its argument.
fn op_methodcaller(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();

    let Some(name) = positional.next() else {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            "methodcaller needs at least one argument, the method name",
        )
        .into());
    };

    if name.as_either_str(heap).is_none() {
        name.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(SimpleException::new_msg(ExcType::TypeError, "method name must be a string").into());
    }

    let call_args: Vec<Value> = positional.collect();
    let call_kwargs: Vec<(Value, Value)> = kwargs.into_iter().collect();

    let caller = MethodCaller::new(name, call_args, call_kwargs);
    let id = heap.allocate(HeapData::MethodCaller(caller))?;
    Ok(Value::Ref(id))
}

/// Finds the index of the first occurrence of `needle` in `haystack`.
///
/// For sequences (list, tuple), finds matching element index.
/// For strings, finds substring index.
/// Returns `ValueError` if the element is not found.
/// Collects items first to avoid borrow conflicts between `heap.get()` and `py_eq()`.
fn find_index(
    haystack: &Value,
    needle: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    // Handle string substring finding (for both interned and heap strings)
    let haystack_str = match haystack {
        Value::InternString(sid) => Some(interns.get_str(*sid)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    };

    if let Some(s) = haystack_str {
        // Extract needle string directly to avoid lifetime issues
        let needle_str = match needle {
            Value::InternString(sid) => interns.get_str(*sid),
            Value::Ref(nid) => match heap.get(*nid) {
                HeapData::Str(ns) => ns.as_str(),
                _ => {
                    return Err(ExcType::type_error(
                        "can't find index of non-string in string".to_string(),
                    ));
                }
            },
            _ => {
                return Err(ExcType::type_error(
                    "can't find index of non-string in string".to_string(),
                ));
            }
        };
        match s.find(needle_str) {
            Some(pos) => return Ok(i64::try_from(pos).unwrap_or(i64::MAX)),
            None => return Err(SimpleException::new_msg(ExcType::ValueError, "substring not found").into()),
        }
    }

    // Handle list and tuple
    let Value::Ref(id) = haystack else {
        return Err(ExcType::type_error(
            "operator.indexOf requires an iterable as first argument".to_string(),
        ));
    };

    let items: Vec<Value> = match heap.get(*id) {
        HeapData::List(list) => list.as_vec().iter().map(Value::copy_for_extend).collect(),
        HeapData::Tuple(tuple) => tuple.as_vec().iter().map(Value::copy_for_extend).collect(),
        _ => {
            return Err(ExcType::type_error(
                "operator.indexOf requires an iterable as first argument".to_string(),
            ));
        }
    };
    for (i, item) in items.iter().enumerate() {
        if item.py_eq(needle, heap, interns) {
            return Ok(i64::try_from(i).unwrap_or(i64::MAX));
        }
    }
    Err(SimpleException::new_msg(ExcType::ValueError, "sequence.index(x): x not in sequence").into())
}

// ===== In-place operations =====

/// Implementation of `operator.iadd(a, b)`. Returns `a += b`.
///
/// For mutable sequences, extends in-place. For immutable types, returns `a + b`.
fn op_iadd(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.iadd", heap)?;
    defer_drop!(b, heap);
    let mut a = a;
    if a.py_iadd(b.clone_with_heap(heap), heap, a.ref_id(), interns)? {
        return Ok(a);
    }
    defer_drop!(a, heap);
    match a.py_add(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for +=: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Implementation of `operator.isub(a, b)`. Returns `a -= b`.
fn op_isub(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.isub", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_sub(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for -=: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Implementation of `operator.imul(a, b)`. Returns `a *= b`.
fn op_imul(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.imul", heap)?;
    defer_drop!(b, heap);
    let a_type = a.py_type(heap);
    let b_type = b.py_type(heap);

    let mut a = a;
    if try_imul_list_in_place(&mut a, b, heap, interns)? {
        return Ok(a);
    }

    defer_drop!(a, heap);
    match a.py_mult(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for *=: '{a_type}' and '{b_type}'"
        ))),
        Err(e) => Err(e),
    }
}

/// Tries to apply list `*=` semantics in place.
///
/// CPython mutates lists for `*=`, preserving identity. `py_mult` returns a new
/// list value, so this helper materializes that repeated content and writes it
/// back into the original list object.
fn try_imul_list_in_place(
    a: &mut Value,
    b: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<bool> {
    let list_id = match a {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::List(_)) => *id,
        _ => return Ok(false),
    };

    let Some(multiplied) = a.py_mult(b, heap, interns)? else {
        return Ok(false);
    };
    let multiplied_id = if let Value::Ref(id) = &multiplied {
        *id
    } else {
        multiplied.drop_with_heap(heap);
        return Ok(false);
    };

    let replacement_items = heap.with_entry_mut(multiplied_id, |heap, data| -> RunResult<Option<Vec<Value>>> {
        let HeapData::List(multiplied_list) = data else {
            return Ok(None);
        };
        let items = multiplied_list
            .as_vec()
            .iter()
            .map(|item| item.clone_with_heap(heap))
            .collect::<Vec<_>>();
        Ok(Some(items))
    })?;
    let Some(replacement_items) = replacement_items else {
        multiplied.drop_with_heap(heap);
        return Ok(false);
    };

    let has_refs = replacement_items.iter().any(|item| matches!(item, Value::Ref(_)));
    heap.with_entry_mut(list_id, |heap, data| -> RunResult<()> {
        let HeapData::List(target_list) = data else {
            unreachable!("list type changed during operator.imul");
        };
        for old in target_list.as_vec_mut().drain(..) {
            old.drop_with_heap(heap);
        }
        target_list.as_vec_mut().extend(replacement_items);
        if has_refs {
            target_list.set_contains_refs();
            heap.mark_potential_cycle();
        }
        Ok(())
    })?;

    multiplied.drop_with_heap(heap);
    Ok(true)
}

/// Implementation of `operator.itruediv(a, b)`. Returns `a /= b`.
fn op_itruediv(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.itruediv", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_div(b, heap, interns) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for /=: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.ifloordiv(a, b)`. Returns `a //= b`.
fn op_ifloordiv(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ifloordiv", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_floordiv(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for //=: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.imod(a, b)`. Returns `a %= b`.
fn op_imod(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.imod", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_mod(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for %=: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.iand(a, b)`. Returns `a &= b`.
fn op_iand(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.iand", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::And, heap, interns)
}

/// Implementation of `operator.ior(a, b)`. Returns `a |= b`.
fn op_ior(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ior", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::Or, heap, interns)
}

/// Implementation of `operator.ixor(a, b)`. Returns `a ^= b`.
fn op_ixor(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ixor", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::Xor, heap, interns)
}

/// Implementation of `operator.ilshift(a, b)`. Returns `a <<= b`.
fn op_ilshift(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ilshift", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::LShift, heap, interns)
}

/// Implementation of `operator.irshift(a, b)`. Returns `a >>= b`.
fn op_irshift(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.irshift", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    a.py_bitwise(b, BitwiseOp::RShift, heap, interns)
}

/// Implementation of `operator.ipow(a, b)`. Returns `a **= b`.
fn op_ipow(heap: &mut Heap<impl ResourceTracker>, _interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (a, b) = args.get_two_args("operator.ipow", heap)?;
    defer_drop!(a, heap);
    defer_drop!(b, heap);
    match a.py_pow(b, heap) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(ExcType::type_error(format!(
            "unsupported operand type(s) for **=: '{}' and '{}'",
            a.py_type(heap),
            b.py_type(heap)
        ))),
        Err(e) => Err(e),
    }
}

/// Implementation of `operator.imatmul(a, b)`. Returns `a @= b`.
///
/// Attempts `a.__imatmul__(b)` first. If that attribute is missing, falls back
/// to binary matmul dispatch (`__matmul__` then `__rmatmul__`).
fn op_imatmul_impl(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let (a, b) = args.get_two_args("operator.imatmul", heap)?;

    // Try a.__imatmul__(b)
    if let Value::Ref(_) = a {
        let dunder_id: StringId = StaticStrings::DunderImatmul.into();
        let method_result = a.py_getattr(dunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                a.drop_with_heap(heap);
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::One(b)));
            }
            Ok(other) => {
                a.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, continue to __matmul__/__rmatmul__ fallback.
            }
        }
    }

    // Try a.__matmul__(b)
    if let Value::Ref(_) = a {
        let dunder_id: StringId = StaticStrings::DunderMatmul.into();
        let method_result = a.py_getattr(dunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                a.drop_with_heap(heap);
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::One(b)));
            }
            Ok(other) => {
                a.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, continue to try __rmatmul__.
            }
        }
    }

    // Try b.__rmatmul__(a)
    if let Value::Ref(_) = b {
        let rdunder_id: StringId = StaticStrings::DunderRmatmul.into();
        let method_result = b.py_getattr(rdunder_id, heap, interns);
        match method_result {
            Ok(AttrCallResult::Value(callable)) => {
                b.drop_with_heap(heap);
                return Ok(AttrCallResult::CallFunction(callable, ArgValues::One(a)));
            }
            Ok(other) => {
                b.drop_with_heap(heap);
                return Ok(other);
            }
            Err(_) => {
                // Attribute not found, fall through to error.
            }
        }
    }

    defer_drop!(a, heap);
    defer_drop!(b, heap);
    Err(ExcType::binary_type_error("@=", a.py_type(heap), b.py_type(heap)))
}
