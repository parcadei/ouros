//! Runtime implementation of Python's `numbers` module.
//!
//! This module builds the numeric ABC hierarchy (`Number`, `Complex`, `Real`,
//! `Rational`, `Integral`) as real class objects backed by Ouros's `abc`
//! machinery. The goal is CPython-compatible behavior for imports,
//! `issubclass`/`isinstance`, abstract-class instantiation checks, and core
//! mixin helpers used by stdlib code.

use smallvec::{SmallVec, smallvec};

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{
        AttrCallResult, ClassObject, Dict, List, Module, PyTrait, Type, UserProperty, allocate_tuple, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

/// `Complex.__abstractmethods__` sorted exactly as CPython reports them.
const COMPLEX_ABSTRACT_METHODS: &[&str] = &[
    "__abs__",
    "__add__",
    "__complex__",
    "__eq__",
    "__mul__",
    "__neg__",
    "__pos__",
    "__pow__",
    "__radd__",
    "__rmul__",
    "__rpow__",
    "__rtruediv__",
    "__truediv__",
    "conjugate",
    "imag",
    "real",
];

/// `Real.__abstractmethods__` sorted exactly as CPython reports them.
const REAL_ABSTRACT_METHODS: &[&str] = &[
    "__abs__",
    "__add__",
    "__ceil__",
    "__eq__",
    "__float__",
    "__floor__",
    "__floordiv__",
    "__le__",
    "__lt__",
    "__mod__",
    "__mul__",
    "__neg__",
    "__pos__",
    "__pow__",
    "__radd__",
    "__rfloordiv__",
    "__rmod__",
    "__rmul__",
    "__round__",
    "__rpow__",
    "__rtruediv__",
    "__truediv__",
    "__trunc__",
];

/// `Rational.__abstractmethods__` sorted exactly as CPython reports them.
const RATIONAL_ABSTRACT_METHODS: &[&str] = &[
    "__abs__",
    "__add__",
    "__ceil__",
    "__eq__",
    "__floor__",
    "__floordiv__",
    "__le__",
    "__lt__",
    "__mod__",
    "__mul__",
    "__neg__",
    "__pos__",
    "__pow__",
    "__radd__",
    "__rfloordiv__",
    "__rmod__",
    "__rmul__",
    "__round__",
    "__rpow__",
    "__rtruediv__",
    "__truediv__",
    "__trunc__",
    "denominator",
    "numerator",
];

/// `Integral.__abstractmethods__` sorted exactly as CPython reports them.
const INTEGRAL_ABSTRACT_METHODS: &[&str] = &[
    "__abs__",
    "__add__",
    "__and__",
    "__ceil__",
    "__eq__",
    "__floor__",
    "__floordiv__",
    "__int__",
    "__invert__",
    "__le__",
    "__lshift__",
    "__lt__",
    "__mod__",
    "__mul__",
    "__neg__",
    "__or__",
    "__pos__",
    "__pow__",
    "__radd__",
    "__rand__",
    "__rfloordiv__",
    "__rlshift__",
    "__rmod__",
    "__rmul__",
    "__ror__",
    "__round__",
    "__rpow__",
    "__rrshift__",
    "__rshift__",
    "__rtruediv__",
    "__rxor__",
    "__truediv__",
    "__trunc__",
    "__xor__",
];

/// `numbers` helper call targets used by class namespaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum NumbersFunctions {
    /// Shared body for abstract methods that raise `NotImplementedError`.
    AbstractMethod,
    /// Shared getter body for abstract properties.
    AbstractProperty,
    /// `Complex.__bool__` mixin.
    ComplexBool,
    /// `Complex.__sub__` mixin.
    ComplexSub,
    /// `Complex.__rsub__` mixin.
    ComplexRsub,
    /// `Real.__complex__` mixin.
    RealComplex,
    /// `Real.__divmod__` mixin.
    RealDivmod,
    /// `Real.__rdivmod__` mixin.
    RealRdivmod,
    /// `Real.real` concrete property getter.
    RealReal,
    /// `Real.imag` concrete property getter.
    RealImag,
    /// `Real.conjugate` mixin.
    RealConjugate,
    /// `Rational.__float__` mixin.
    RationalFloat,
    /// `Integral.__index__` mixin.
    IntegralIndex,
    /// `Integral.__float__` mixin.
    IntegralFloat,
    /// `Integral.numerator` concrete property getter.
    IntegralNumerator,
    /// `Integral.denominator` concrete property getter.
    IntegralDenominator,
}

/// Creates the `numbers` module and allocates it on the heap.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Numbers);

    // Build against a real abc module instance so exported names (`ABCMeta`,
    // `abstractmethod`) and virtual-subclass registration behavior align with
    // the same machinery used elsewhere.
    let abc_module_value = Value::Ref(super::abc::create_module(heap, interns)?);
    defer_drop!(abc_module_value, heap);
    let Value::Ref(abc_module_id) = abc_module_value else {
        unreachable!("abc::create_module must return a module heap reference");
    };

    let abc_meta_value = module_attr_copy(*abc_module_id, "ABCMeta", heap, interns)
        .expect("abc module must expose ABCMeta class object");
    let abc_meta_id = if let Value::Ref(id) = &abc_meta_value {
        *id
    } else {
        abc_meta_value.drop_with_heap(heap);
        panic!("abc module must expose ABCMeta class object");
    };

    let abstractmethod_value = module_attr_copy(*abc_module_id, "abstractmethod", heap, interns)
        .expect("abc module must expose abstractmethod");

    module.set_attr_text("ABCMeta", abc_meta_value, heap, interns)?;
    module.set_attr_text("abstractmethod", abstractmethod_value, heap, interns)?;

    let number_id = create_number_class(abc_meta_id, heap, interns)?;
    let complex_id = create_complex_class(abc_meta_id, number_id, heap, interns)?;
    let real_id = create_real_class(abc_meta_id, complex_id, heap, interns)?;
    let rational_id = create_rational_class(abc_meta_id, real_id, heap, interns)?;
    let integral_id = create_integral_class(abc_meta_id, rational_id, heap, interns)?;

    module.set_attr_text("Number", Value::Ref(number_id), heap, interns)?;
    module.set_attr_text("Complex", Value::Ref(complex_id), heap, interns)?;
    module.set_attr_text("Real", Value::Ref(real_id), heap, interns)?;
    module.set_attr_text("Rational", Value::Ref(rational_id), heap, interns)?;
    module.set_attr_text("Integral", Value::Ref(integral_id), heap, interns)?;

    module.set_attr_text("__all__", create_all_list(heap)?, heap, interns)?;

    register_builtin_virtual_subclass(complex_id, Type::Complex, heap, interns);
    register_builtin_virtual_subclass(complex_id, Type::Float, heap, interns);
    register_builtin_virtual_subclass(complex_id, Type::Int, heap, interns);

    register_builtin_virtual_subclass(real_id, Type::Float, heap, interns);
    register_builtin_virtual_subclass(real_id, Type::Int, heap, interns);

    register_builtin_virtual_subclass(rational_id, Type::Int, heap, interns);
    register_builtin_virtual_subclass(integral_id, Type::Int, heap, interns);

    register_builtin_virtual_subclass(number_id, Type::Complex, heap, interns);
    register_builtin_virtual_subclass(number_id, Type::Float, heap, interns);
    register_builtin_virtual_subclass(number_id, Type::Int, heap, interns);

    heap.allocate(HeapData::Module(module))
}

/// Dispatches runtime calls for helper methods attached to `numbers` classes.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: NumbersFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        NumbersFunctions::AbstractMethod | NumbersFunctions::AbstractProperty => abstract_not_implemented(heap, args)?,
        NumbersFunctions::ComplexBool => complex_bool(heap, interns, args)?,
        NumbersFunctions::ComplexSub => complex_sub(heap, interns, args)?,
        NumbersFunctions::ComplexRsub => complex_rsub(heap, interns, args)?,
        NumbersFunctions::RealComplex => real_complex(heap, interns, args)?,
        NumbersFunctions::RealDivmod => real_divmod(heap, args)?,
        NumbersFunctions::RealRdivmod => real_rdivmod(heap, args)?,
        NumbersFunctions::RealReal => identity_getter(heap, args)?,
        NumbersFunctions::RealImag => real_imag(heap, args)?,
        NumbersFunctions::RealConjugate => identity_getter(heap, args)?,
        NumbersFunctions::RationalFloat => rational_float(heap, interns, args)?,
        NumbersFunctions::IntegralIndex => integral_index(heap, interns, args)?,
        NumbersFunctions::IntegralFloat => integral_float(heap, interns, args)?,
        NumbersFunctions::IntegralNumerator => identity_getter(heap, args)?,
        NumbersFunctions::IntegralDenominator => integral_denominator(heap, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Creates `numbers.Number`.
fn create_number_class(
    abc_meta_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    set_common_class_attrs(&mut namespace, heap, interns)?;
    dict_set_str_key(&mut namespace, "__hash__", Value::None, heap, interns)?;
    set_abstract_metadata(&mut namespace, &[], false, heap, interns)?;

    let object_id = heap.builtin_class_id(Type::Object)?;
    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("Number".to_string()),
        Value::Ref(abc_meta_id),
        &[object_id],
        namespace,
    )
}

/// Creates `numbers.Complex`.
fn create_complex_class(
    abc_meta_id: HeapId,
    number_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    set_common_class_attrs(&mut namespace, heap, interns)?;

    for &name in COMPLEX_ABSTRACT_METHODS {
        dict_set_str_key(
            &mut namespace,
            name,
            Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::AbstractMethod)),
            heap,
            interns,
        )?;
    }

    set_property(
        &mut namespace,
        "real",
        NumbersFunctions::AbstractProperty,
        heap,
        interns,
    )?;
    set_property(
        &mut namespace,
        "imag",
        NumbersFunctions::AbstractProperty,
        heap,
        interns,
    )?;

    dict_set_str_key(
        &mut namespace,
        "__bool__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::ComplexBool)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__sub__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::ComplexSub)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__rsub__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::ComplexRsub)),
        heap,
        interns,
    )?;

    set_abstract_metadata(&mut namespace, COMPLEX_ABSTRACT_METHODS, true, heap, interns)?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("Complex".to_string()),
        Value::Ref(abc_meta_id),
        &[number_id],
        namespace,
    )
}

/// Creates `numbers.Real`.
fn create_real_class(
    abc_meta_id: HeapId,
    complex_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    set_common_class_attrs(&mut namespace, heap, interns)?;

    for &name in REAL_ABSTRACT_METHODS {
        dict_set_str_key(
            &mut namespace,
            name,
            Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::AbstractMethod)),
            heap,
            interns,
        )?;
    }

    dict_set_str_key(
        &mut namespace,
        "__complex__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::RealComplex)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__divmod__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::RealDivmod)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__rdivmod__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::RealRdivmod)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "conjugate",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::RealConjugate)),
        heap,
        interns,
    )?;

    set_property(&mut namespace, "real", NumbersFunctions::RealReal, heap, interns)?;
    set_property(&mut namespace, "imag", NumbersFunctions::RealImag, heap, interns)?;

    set_abstract_metadata(&mut namespace, REAL_ABSTRACT_METHODS, true, heap, interns)?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("Real".to_string()),
        Value::Ref(abc_meta_id),
        &[complex_id],
        namespace,
    )
}

/// Creates `numbers.Rational`.
fn create_rational_class(
    abc_meta_id: HeapId,
    real_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    set_common_class_attrs(&mut namespace, heap, interns)?;

    for &name in RATIONAL_ABSTRACT_METHODS {
        dict_set_str_key(
            &mut namespace,
            name,
            Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::AbstractMethod)),
            heap,
            interns,
        )?;
    }

    dict_set_str_key(
        &mut namespace,
        "__float__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::RationalFloat)),
        heap,
        interns,
    )?;

    set_property(
        &mut namespace,
        "numerator",
        NumbersFunctions::AbstractProperty,
        heap,
        interns,
    )?;
    set_property(
        &mut namespace,
        "denominator",
        NumbersFunctions::AbstractProperty,
        heap,
        interns,
    )?;

    set_abstract_metadata(&mut namespace, RATIONAL_ABSTRACT_METHODS, true, heap, interns)?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("Rational".to_string()),
        Value::Ref(abc_meta_id),
        &[real_id],
        namespace,
    )
}

/// Creates `numbers.Integral`.
fn create_integral_class(
    abc_meta_id: HeapId,
    rational_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    set_common_class_attrs(&mut namespace, heap, interns)?;

    for &name in INTEGRAL_ABSTRACT_METHODS {
        dict_set_str_key(
            &mut namespace,
            name,
            Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::AbstractMethod)),
            heap,
            interns,
        )?;
    }

    dict_set_str_key(
        &mut namespace,
        "__index__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::IntegralIndex)),
        heap,
        interns,
    )?;
    dict_set_str_key(
        &mut namespace,
        "__float__",
        Value::ModuleFunction(ModuleFunctions::Numbers(NumbersFunctions::IntegralFloat)),
        heap,
        interns,
    )?;

    set_property(
        &mut namespace,
        "numerator",
        NumbersFunctions::IntegralNumerator,
        heap,
        interns,
    )?;
    set_property(
        &mut namespace,
        "denominator",
        NumbersFunctions::IntegralDenominator,
        heap,
        interns,
    )?;

    set_abstract_metadata(&mut namespace, INTEGRAL_ABSTRACT_METHODS, true, heap, interns)?;

    create_runtime_class(
        heap,
        interns,
        EitherStr::Heap("Integral".to_string()),
        Value::Ref(abc_meta_id),
        &[rational_id],
        namespace,
    )
}

/// Creates a class object with explicit metaclass, bases, and namespace.
fn create_runtime_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    name: EitherStr,
    metaclass: Value,
    bases: &[HeapId],
    namespace: Dict,
) -> Result<HeapId, ResourceError> {
    for &base_id in bases {
        heap.inc_ref(base_id);
    }
    if let Value::Ref(meta_id) = &metaclass {
        heap.inc_ref(*meta_id);
    }

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(name, class_uid, metaclass, namespace, bases.to_vec(), vec![]);
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, bases, heap, interns)
        .expect("numbers helper class hierarchy should always produce a valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }

    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    if bases.is_empty() {
        let object_id = heap.builtin_class_id(Type::Object)?;
        heap.with_entry_mut(object_id, |_, data| {
            let HeapData::ClassObject(cls) = data else {
                return Err(ExcType::type_error("builtin object is not a class".to_string()));
            };
            cls.register_subclass(class_id, class_uid);
            Ok(())
        })
        .expect("builtin object class registry should be mutable");
    } else {
        for &base_id in bases {
            heap.with_entry_mut(base_id, |_, data| {
                let HeapData::ClassObject(cls) = data else {
                    return Err(ExcType::type_error("base is not a class".to_string()));
                };
                cls.register_subclass(class_id, class_uid);
                Ok(())
            })
            .expect("numbers helper base should always be a class object");
        }
    }

    Ok(class_id)
}

/// Adds shared class metadata used by CPython's `numbers` classes.
fn set_common_class_attrs(
    namespace: &mut Dict,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    dict_set_str_key(
        namespace,
        "__module__",
        Value::InternString(StaticStrings::Numbers.into()),
        heap,
        interns,
    )?;
    let empty_tuple = allocate_tuple(SmallVec::new(), heap)?;
    dict_set_str_key(namespace, "__slots__", empty_tuple, heap, interns)
}

/// Writes `__abstractmethods__` and the internal abstract marker for a class.
fn set_abstract_metadata(
    namespace: &mut Dict,
    names: &[&str],
    is_abstract: bool,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let names_tuple = build_string_tuple(names, heap)?;
    dict_set_str_key(namespace, super::abc::ABSTRACT_METHODS_ATTR, names_tuple, heap, interns)?;
    dict_set_str_key(
        namespace,
        super::abc::ABC_IS_ABSTRACT_ATTR,
        Value::Bool(is_abstract),
        heap,
        interns,
    )
}

/// Creates and installs a `property` descriptor backed by a `numbers` helper.
fn set_property(
    namespace: &mut Dict,
    name: &str,
    getter: NumbersFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let prop = UserProperty::new(Some(Value::ModuleFunction(ModuleFunctions::Numbers(getter))));
    let prop_id = heap.allocate(HeapData::UserProperty(prop))?;
    dict_set_str_key(namespace, name, Value::Ref(prop_id), heap, interns)
}

/// Creates `numbers.__all__` exactly as CPython defines it.
fn create_all_list(heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
    let items = vec![
        heap_string_value("Number", heap)?,
        heap_string_value("Complex", heap)?,
        heap_string_value("Real", heap)?,
        heap_string_value("Rational", heap)?,
        heap_string_value("Integral", heap)?,
    ];
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    Ok(Value::Ref(list_id))
}

/// Builds a tuple of Python string values from Rust string slices.
fn build_string_tuple(names: &[&str], heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
    let mut values: SmallVec<[Value; 3]> = SmallVec::with_capacity(names.len());
    for &name in names {
        values.push(heap_string_value(name, heap)?);
    }
    allocate_tuple(values, heap)
}

/// Allocates a heap-backed Python string value.
fn heap_string_value(value: &str, heap: &mut Heap<impl ResourceTracker>) -> Result<Value, ResourceError> {
    let id = heap.allocate(HeapData::Str(crate::types::Str::from(value)))?;
    Ok(Value::Ref(id))
}

/// Reads and clones a module attribute value by string key.
fn module_attr_copy(
    module_id: HeapId,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Value> {
    let HeapData::Module(module) = heap.get(module_id) else {
        return None;
    };
    module
        .attrs()
        .get_by_str(name, heap, interns)
        .map(|value| value.clone_with_heap(heap))
}

/// Sets a string-keyed value into a dict, dropping replaced values.
fn dict_set_str_key(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(crate::types::Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Registers a builtin type as a virtual subclass of a `numbers` ABC.
fn register_builtin_virtual_subclass(
    class_id: HeapId,
    builtin_type: Type,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) {
    heap.inc_ref(class_id);
    let args = ArgValues::Two(Value::Ref(class_id), Value::Builtin(Builtins::Type(builtin_type)));
    let result = super::abc::call(heap, interns, super::abc::AbcFunctions::Register, args)
        .expect("numbers virtual subclass registration should succeed");
    if let AttrCallResult::Value(value) = result {
        value.drop_with_heap(heap);
    }
}

/// Shared implementation for abstract method/property bodies.
fn abstract_not_implemented(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.drop_with_heap(heap);
    Err(SimpleException::new(ExcType::NotImplementedError, None).into())
}

/// `Complex.__bool__`: true iff `self != 0`.
fn complex_bool(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("__bool__", heap)?;
    defer_drop!(value, heap);
    Ok(Value::Bool(!value.py_eq(&Value::Int(0), heap, interns)))
}

/// `Complex.__sub__`: `self + (-other)`.
fn complex_sub(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (lhs, rhs) = args.get_two_args("__sub__", heap)?;
    defer_drop!(lhs, heap);
    defer_drop!(rhs, heap);

    let neg_rhs = negate_value(rhs, heap, interns)?;
    defer_drop!(neg_rhs, heap);

    let Some(result) = lhs.py_add(neg_rhs, heap, interns)? else {
        return Err(binary_op_type_error("-", lhs, rhs, heap));
    };
    Ok(result)
}

/// `Complex.__rsub__`: `(-self) + other`.
fn complex_rsub(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (lhs, rhs) = args.get_two_args("__rsub__", heap)?;
    defer_drop!(lhs, heap);
    defer_drop!(rhs, heap);

    let neg_lhs = negate_value(lhs, heap, interns)?;
    defer_drop!(neg_lhs, heap);

    let Some(result) = neg_lhs.py_add(rhs, heap, interns)? else {
        return Err(binary_op_type_error("-", rhs, lhs, heap));
    };
    Ok(result)
}

/// `Real.__complex__`: `complex(float(self), 0)`.
fn real_complex(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("__complex__", heap)?;
    let real = Type::Float.call(heap, ArgValues::One(value), interns)?;
    Type::Complex.call(heap, ArgValues::Two(real, Value::Float(0.0)), interns)
}

/// `Real.__divmod__`: `(self // other, self % other)`.
fn real_divmod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (lhs, rhs) = args.get_two_args("__divmod__", heap)?;
    defer_drop!(lhs, heap);
    defer_drop!(rhs, heap);

    let Some(floor) = lhs.py_floordiv(rhs, heap)? else {
        return Err(divmod_type_error(lhs, rhs, heap));
    };
    let Some(rem) = lhs.py_mod(rhs, heap)? else {
        floor.drop_with_heap(heap);
        return Err(divmod_type_error(lhs, rhs, heap));
    };

    Ok(allocate_tuple(smallvec![floor, rem], heap)?)
}

/// `Real.__rdivmod__`: `(other // self, other % self)`.
fn real_rdivmod(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (lhs, rhs) = args.get_two_args("__rdivmod__", heap)?;
    defer_drop!(lhs, heap);
    defer_drop!(rhs, heap);

    let Some(floor) = rhs.py_floordiv(lhs, heap)? else {
        return Err(divmod_type_error(rhs, lhs, heap));
    };
    let Some(rem) = rhs.py_mod(lhs, heap)? else {
        floor.drop_with_heap(heap);
        return Err(divmod_type_error(rhs, lhs, heap));
    };

    Ok(allocate_tuple(smallvec![floor, rem], heap)?)
}

/// Identity property/method helper (`return +self` equivalent for numeric values).
fn identity_getter(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.get_one_arg("identity", heap)
}

/// `Real.imag`: real numbers have no imaginary component.
fn real_imag(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("imag", heap)?;
    value.drop_with_heap(heap);
    Ok(Value::Int(0))
}

/// `Rational.__float__` compatibility helper.
fn rational_float(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("__float__", heap)?;
    let int_value = Type::Int.call(heap, ArgValues::One(value), interns)?;
    Type::Float.call(heap, ArgValues::One(int_value), interns)
}

/// `Integral.__index__` delegates to `int(self)`.
fn integral_index(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("__index__", heap)?;
    Type::Int.call(heap, ArgValues::One(value), interns)
}

/// `Integral.__float__`: `float(int(self))`.
fn integral_float(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("__float__", heap)?;
    let int_value = Type::Int.call(heap, ArgValues::One(value), interns)?;
    Type::Float.call(heap, ArgValues::One(int_value), interns)
}

/// `Integral.denominator`: integers have denominator `1`.
fn integral_denominator(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let value = args.get_one_arg("denominator", heap)?;
    value.drop_with_heap(heap);
    Ok(Value::Int(1))
}

/// Computes `-value` using numeric subtraction from zero.
fn negate_value(value: &Value, heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> RunResult<Value> {
    let Some(negated) = Value::Int(0).py_sub(value, heap)? else {
        return Err(unary_op_type_error("-", value, heap));
    };
    Ok(negated)
}

/// Builds a CPython-style binary operator type error.
fn binary_op_type_error(
    op: &str,
    lhs: &Value,
    rhs: &Value,
    heap: &Heap<impl ResourceTracker>,
) -> crate::exception_private::RunError {
    ExcType::type_error(format!(
        "unsupported operand type(s) for {op}: '{}' and '{}'",
        lhs.py_type(heap),
        rhs.py_type(heap)
    ))
}

/// Builds a CPython-style unary operator type error.
fn unary_op_type_error(
    op: &str,
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
) -> crate::exception_private::RunError {
    ExcType::type_error(format!("bad operand type for unary {op}: '{}'", value.py_type(heap)))
}

/// Builds the `divmod` unsupported operand type error.
fn divmod_type_error(
    lhs: &Value,
    rhs: &Value,
    heap: &Heap<impl ResourceTracker>,
) -> crate::exception_private::RunError {
    ExcType::type_error(format!(
        "unsupported operand type(s) for divmod(): '{}' and '{}'",
        lhs.py_type(heap),
        rhs.py_type(heap)
    ))
}
