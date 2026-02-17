//! Implementation of the `array` module.
//!
//! Provides a CPython-compatible `array.array` class with typed storage.

use std::{cmp::Ordering, convert::TryInto};

use num_bigint::BigInt;
use smallvec::smallvec;

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
        AttrCallResult, ClassObject, Dict, Instance, List, LongInt, Module, OurosIter, PyTrait, Slice, Str, Type,
        allocate_tuple, compute_c3_mro,
    },
    value::{EitherStr, Value},
};

const ARRAY_TYPECODES: &str = "bBuhHiIlLqQfd";
const ATTR_ARRAY_TYPECODE: &str = "_ouros_array_typecode";
const ATTR_ARRAY_ITEMS: &str = "_ouros_array_items";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
pub(crate) enum ArrayFunctions {
    #[strum(serialize = "__init__")]
    Init,
    #[strum(serialize = "append")]
    Append,
    #[strum(serialize = "extend")]
    Extend,
    #[strum(serialize = "insert")]
    Insert,
    #[strum(serialize = "pop")]
    Pop,
    #[strum(serialize = "remove")]
    Remove,
    #[strum(serialize = "__getitem__")]
    DunderGetitem,
    #[strum(serialize = "__setitem__")]
    DunderSetitem,
    #[strum(serialize = "__delitem__")]
    DunderDelitem,
    #[strum(serialize = "__len__")]
    DunderLen,
    #[strum(serialize = "__contains__")]
    DunderContains,
    #[strum(serialize = "__iter__")]
    DunderIter,
    #[strum(serialize = "__reversed__")]
    DunderReversed,
    #[strum(serialize = "__repr__")]
    DunderRepr,
    #[strum(serialize = "__eq__")]
    DunderEq,
    #[strum(serialize = "__ne__")]
    DunderNe,
    #[strum(serialize = "__lt__")]
    DunderLt,
    #[strum(serialize = "__le__")]
    DunderLe,
    #[strum(serialize = "__gt__")]
    DunderGt,
    #[strum(serialize = "__ge__")]
    DunderGe,
    #[strum(serialize = "index")]
    Index,
    #[strum(serialize = "count")]
    Count,
    #[strum(serialize = "reverse")]
    Reverse,
    #[strum(serialize = "buffer_info")]
    BufferInfo,
    #[strum(serialize = "byteswap")]
    Byteswap,
    #[strum(serialize = "tobytes")]
    Tobytes,
    #[strum(serialize = "frombytes")]
    Frombytes,
    #[strum(serialize = "tolist")]
    Tolist,
    #[strum(serialize = "fromlist")]
    Fromlist,
    #[strum(serialize = "__add__")]
    DunderAdd,
    #[strum(serialize = "__iadd__")]
    DunderIadd,
    #[strum(serialize = "__mul__")]
    DunderMul,
    #[strum(serialize = "__imul__")]
    DunderImul,
    #[strum(serialize = "__rmul__")]
    DunderRmul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeCode {
    B,
    UB,
    U,
    H,
    UH,
    I,
    UI,
    L,
    UL,
    Q,
    UQ,
    F,
    D,
}

impl TypeCode {
    fn from_char(value: char) -> Option<Self> {
        match value {
            'b' => Some(Self::B),
            'B' => Some(Self::UB),
            'u' => Some(Self::U),
            'h' => Some(Self::H),
            'H' => Some(Self::UH),
            'i' => Some(Self::I),
            'I' => Some(Self::UI),
            'l' => Some(Self::L),
            'L' => Some(Self::UL),
            'q' => Some(Self::Q),
            'Q' => Some(Self::UQ),
            'f' => Some(Self::F),
            'd' => Some(Self::D),
            _ => None,
        }
    }

    fn as_char(self) -> char {
        match self {
            Self::B => 'b',
            Self::UB => 'B',
            Self::U => 'u',
            Self::H => 'h',
            Self::UH => 'H',
            Self::I => 'i',
            Self::UI => 'I',
            Self::L => 'l',
            Self::UL => 'L',
            Self::Q => 'q',
            Self::UQ => 'Q',
            Self::F => 'f',
            Self::D => 'd',
        }
    }

    fn itemsize(self) -> usize {
        match self {
            Self::B | Self::UB => 1,
            Self::H | Self::UH => 2,
            Self::I | Self::UI | Self::F | Self::U => 4,
            Self::L | Self::UL | Self::Q | Self::UQ | Self::D => 8,
        }
    }
}

pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::ArrayMod);

    let array_class_id = create_array_class(heap, interns)?;
    let array_value = Value::Ref(array_class_id);
    module.set_attr_text("array", array_value.clone_with_heap(heap), heap, interns)?;
    module.set_attr_text("ArrayType", array_value.clone_with_heap(heap), heap, interns)?;
    array_value.drop_with_heap(heap);

    let typecodes_id = heap.allocate(HeapData::Str(Str::from(ARRAY_TYPECODES)))?;
    module.set_attr_text("typecodes", Value::Ref(typecodes_id), heap, interns)?;

    heap.allocate(HeapData::Module(module))
}

fn create_array_class(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut namespace = Dict::new();
    for (name, function) in [
        ("__init__", ArrayFunctions::Init),
        ("append", ArrayFunctions::Append),
        ("extend", ArrayFunctions::Extend),
        ("insert", ArrayFunctions::Insert),
        ("pop", ArrayFunctions::Pop),
        ("remove", ArrayFunctions::Remove),
        ("__getitem__", ArrayFunctions::DunderGetitem),
        ("__setitem__", ArrayFunctions::DunderSetitem),
        ("__delitem__", ArrayFunctions::DunderDelitem),
        ("__len__", ArrayFunctions::DunderLen),
        ("__contains__", ArrayFunctions::DunderContains),
        ("__iter__", ArrayFunctions::DunderIter),
        ("__reversed__", ArrayFunctions::DunderReversed),
        ("__repr__", ArrayFunctions::DunderRepr),
        ("__eq__", ArrayFunctions::DunderEq),
        ("__ne__", ArrayFunctions::DunderNe),
        ("__lt__", ArrayFunctions::DunderLt),
        ("__le__", ArrayFunctions::DunderLe),
        ("__gt__", ArrayFunctions::DunderGt),
        ("__ge__", ArrayFunctions::DunderGe),
        ("index", ArrayFunctions::Index),
        ("count", ArrayFunctions::Count),
        ("reverse", ArrayFunctions::Reverse),
        ("buffer_info", ArrayFunctions::BufferInfo),
        ("byteswap", ArrayFunctions::Byteswap),
        ("tobytes", ArrayFunctions::Tobytes),
        ("frombytes", ArrayFunctions::Frombytes),
        ("tolist", ArrayFunctions::Tolist),
        ("fromlist", ArrayFunctions::Fromlist),
        ("__add__", ArrayFunctions::DunderAdd),
        ("__iadd__", ArrayFunctions::DunderIadd),
        ("__mul__", ArrayFunctions::DunderMul),
        ("__imul__", ArrayFunctions::DunderImul),
        ("__rmul__", ArrayFunctions::DunderRmul),
    ] {
        dict_set_str_key(
            &mut namespace,
            name,
            Value::ModuleFunction(ModuleFunctions::ArrayMod(function)),
            heap,
            interns,
        )?;
    }

    let object_id = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_id);

    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        EitherStr::Heap("array.array".to_string()),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        namespace,
        vec![object_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[object_id], heap, interns).expect("array helper class should have valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    heap.with_entry_mut(object_id, |_, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("builtin object registry should be mutable");

    Ok(class_id)
}

fn dict_set_str_key(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ArrayFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        ArrayFunctions::Init => array_init(heap, interns, args)?,
        ArrayFunctions::Append => array_append(heap, interns, args)?,
        ArrayFunctions::Extend => array_extend(heap, interns, args)?,
        ArrayFunctions::Insert => array_insert(heap, interns, args)?,
        ArrayFunctions::Pop => array_pop(heap, args)?,
        ArrayFunctions::Remove => array_remove(heap, interns, args)?,
        ArrayFunctions::DunderGetitem => array_getitem(heap, interns, args)?,
        ArrayFunctions::DunderSetitem => array_setitem(heap, interns, args)?,
        ArrayFunctions::DunderDelitem => array_delitem(heap, args)?,
        ArrayFunctions::DunderLen => array_len(heap, args)?,
        ArrayFunctions::DunderContains => array_contains(heap, interns, args)?,
        ArrayFunctions::DunderIter => array_iter(heap, interns, args)?,
        ArrayFunctions::DunderReversed => array_reversed(heap, interns, args)?,
        ArrayFunctions::DunderRepr => array_repr(heap, interns, args)?,
        ArrayFunctions::DunderEq => array_eq(heap, interns, args)?,
        ArrayFunctions::DunderNe => array_ne(heap, interns, args)?,
        ArrayFunctions::DunderLt => array_lt(heap, interns, args)?,
        ArrayFunctions::DunderLe => array_le(heap, interns, args)?,
        ArrayFunctions::DunderGt => array_gt(heap, interns, args)?,
        ArrayFunctions::DunderGe => array_ge(heap, interns, args)?,
        ArrayFunctions::Index => array_index(heap, interns, args)?,
        ArrayFunctions::Count => array_count(heap, interns, args)?,
        ArrayFunctions::Reverse => array_reverse(heap, args)?,
        ArrayFunctions::BufferInfo => array_buffer_info(heap, args)?,
        ArrayFunctions::Byteswap => array_byteswap(heap, args)?,
        ArrayFunctions::Tobytes => array_tobytes(heap, interns, args)?,
        ArrayFunctions::Frombytes => array_frombytes(heap, interns, args)?,
        ArrayFunctions::Tolist => array_tolist(heap, args)?,
        ArrayFunctions::Fromlist => array_fromlist(heap, interns, args)?,
        ArrayFunctions::DunderAdd => array_add(heap, interns, args)?,
        ArrayFunctions::DunderIadd => array_iadd(heap, interns, args)?,
        ArrayFunctions::DunderMul => array_mul(heap, interns, args)?,
        ArrayFunctions::DunderImul => array_imul(heap, args)?,
        ArrayFunctions::DunderRmul => array_rmul(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

fn array_init(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("array.array"));
    }

    let Some(self_value) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("array.array", 2, 0));
    };
    defer_drop!(self_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.array")?;

    let Some(typecode_value) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("array.array", 2, 1));
    };

    let initializer = positional.next();
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        typecode_value.drop_with_heap(heap);
        initializer.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("array.array", 3, 4));
    }

    let typecode_char = parse_typecode_argument(typecode_value, heap, interns)?;
    let typecode = parse_typecode(typecode_char)?;

    set_array_typecode_and_items(self_id, typecode, Vec::new(), heap, interns)?;

    if let Some(initializer) = initializer {
        initialize_array(self_id, typecode, initializer, heap, interns)?;
    }

    Ok(Value::None)
}

fn initialize_array(
    instance_id: HeapId,
    typecode: TypeCode,
    initializer: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    if let Some(bytes) = value_as_bytes_slice(&initializer, heap, interns) {
        let bytes = bytes.to_vec();
        let values = decode_bytes(typecode, &bytes, heap)?;
        initializer.drop_with_heap(heap);
        append_many(instance_id, values, heap)?;
        return Ok(());
    }

    if matches!(initializer, Value::InternString(_) | Value::Ref(_)) {
        let maybe_string = initializer
            .as_either_str(heap)
            .map(|text| text.as_str(interns).to_string());
        if let Some(text) = maybe_string {
            if typecode == TypeCode::U {
                initializer.drop_with_heap(heap);
                let mut values = Vec::with_capacity(text.chars().count());
                for ch in text.chars() {
                    values.push(char_to_value(ch, heap)?);
                }
                append_many(instance_id, values, heap)?;
                return Ok(());
            }

            initializer.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "cannot use a str to initialize an array with typecode '{}'",
                typecode.as_char()
            )));
        }
    }

    extend_from_iterable(instance_id, typecode, initializer, heap, interns, false)
}

fn array_append(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, value) = args.get_two_args("array.append", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.append")?;
    let (typecode, items_id) = array_state(self_id, heap, interns)?;

    let normalized = normalize_array_value(value, typecode, heap, interns)?;
    with_list_mut(items_id, heap, |heap_inner, list| {
        list.append(heap_inner, normalized);
        Ok(())
    })?;

    Ok(Value::None)
}

fn array_extend(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, iterable) = args.get_two_args("array.extend", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.extend")?;
    let (typecode, _) = array_state(self_id, heap, interns)?;

    extend_from_iterable(self_id, typecode, iterable, heap, interns, true)?;
    Ok(Value::None)
}

fn array_insert(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, index_value, value) = args.get_three_args("array.insert", heap)?;
    defer_drop!(self_value, heap);
    defer_drop!(index_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.insert")?;
    let (typecode, items_id) = array_state(self_id, heap, interns)?;

    let index = value_to_i64(index_value, heap)?;
    let normalized = normalize_array_value(value, typecode, heap, interns)?;

    with_list_mut(items_id, heap, |heap_inner, list| {
        let len = i64::try_from(list.as_vec().len()).expect("list length fits i64");
        let mut normalized_index = if index < 0 { index + len } else { index };
        if normalized_index < 0 {
            normalized_index = 0;
        }
        if normalized_index > len {
            normalized_index = len;
        }
        let index_usize = usize::try_from(normalized_index).expect("normalized index is non-negative");
        list.insert(heap_inner, index_usize, normalized);
        Ok(())
    })?;

    Ok(Value::None)
}

fn array_pop(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (self_value, maybe_index) = args.get_one_two_args("array.pop", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.pop")?;
    let items_id = array_items_id(self_id, heap)?;

    let index = if let Some(index_value) = maybe_index {
        defer_drop!(index_value, heap);
        value_to_i64(index_value, heap)?
    } else {
        -1
    };

    let popped = with_list_mut(items_id, heap, |_, list| {
        if list.as_vec().is_empty() {
            return Err(SimpleException::new_msg(ExcType::IndexError, "pop from empty array").into());
        }

        let len = i64::try_from(list.as_vec().len()).expect("list length fits i64");
        let normalized = if index < 0 { index + len } else { index };
        if normalized < 0 || normalized >= len {
            return Err(SimpleException::new_msg(ExcType::IndexError, "pop index out of range").into());
        }

        let idx = usize::try_from(normalized).expect("normalized index is non-negative");
        Ok(list.as_vec_mut().remove(idx))
    })?;

    Ok(popped)
}

fn array_remove(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, target) = args.get_two_args("array.remove", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.remove")?;
    let items_id = array_items_id(self_id, heap)?;

    let removed = with_list_mut(items_id, heap, |heap_inner, list| {
        let Some(index) = list
            .as_vec()
            .iter()
            .position(|item| item.py_eq(&target, heap_inner, interns))
        else {
            return Err(SimpleException::new_msg(ExcType::ValueError, "array.remove(x): x not in array").into());
        };
        let removed = list.as_vec_mut().remove(index);
        removed.drop_with_heap(heap_inner);
        Ok(())
    });

    target.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    removed?;
    Ok(Value::None)
}

fn array_getitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, key) = args.get_two_args("array.__getitem__", heap)?;
    defer_drop!(self_value, heap);
    defer_drop!(key, heap);
    let self_id = expect_instance_id(self_value, heap, "array.__getitem__")?;
    let (typecode, items_id) = array_state(self_id, heap, interns)?;

    if let Some(slice) = value_as_slice(key, heap) {
        let class_id = instance_class_id(self_id, heap)?;
        let items = list_items_cloned(items_id, heap);
        let (start, stop, step) = slice
            .indices(items.len())
            .map_err(|()| ExcType::value_error_slice_step_zero())?;
        let sliced = get_slice_items(&items, start, stop, step, heap);
        items.drop_with_heap(heap);
        return create_array_instance(class_id, typecode, sliced, heap, interns);
    }

    let index = array_index_from_key(key, heap)?;
    let result = {
        let items = list_items_cloned(items_id, heap);
        let len = i64::try_from(items.len()).expect("list length fits i64");
        let normalized = if index < 0 { index + len } else { index };
        if normalized < 0 || normalized >= len {
            items.drop_with_heap(heap);
            return Err(SimpleException::new_msg(ExcType::IndexError, "array index out of range").into());
        }
        let idx = usize::try_from(normalized).expect("normalized index is non-negative");
        let value = items[idx].clone_with_heap(heap);
        items.drop_with_heap(heap);
        value
    };

    Ok(result)
}

fn array_setitem(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, key, value) = args.get_three_args("array.__setitem__", heap)?;
    defer_drop!(self_value, heap);
    defer_drop!(key, heap);
    defer_drop!(value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.__setitem__")?;
    let (typecode, items_id) = array_state(self_id, heap, interns)?;

    if let Some(slice) = value_as_slice(key, heap) {
        let (other_typecode, replacement_id) = array_state_from_value(value, heap, interns).ok_or_else(|| {
            let value_type = value_type_name(value, heap);
            ExcType::type_error(format!("can only assign array (not \"{value_type}\") to array slice"))
        })?;

        if other_typecode != typecode {
            return Err(ExcType::type_error(
                "bad argument type for built-in operation".to_string(),
            ));
        }

        let replacement = list_items_cloned(replacement_id, heap);
        let (start, stop, step) = slice
            .indices(list_len(items_id, heap))
            .map_err(|()| ExcType::value_error_slice_step_zero())?;

        with_list_mut(items_id, heap, |heap_inner, list| {
            if step == 1 {
                let removed: Vec<Value> = list.as_vec_mut().splice(start..stop, replacement).collect();
                removed.drop_with_heap(heap_inner);
            } else {
                let indices = slice_assignment_indices(start, stop, step, list.as_vec().len());
                if replacement.len() != indices.len() {
                    return Err(SimpleException::new_msg(
                        ExcType::ValueError,
                        format!(
                            "attempt to assign array of size {} to extended slice of size {}",
                            replacement.len(),
                            indices.len()
                        ),
                    )
                    .into());
                }
                for (idx, new_value) in indices.into_iter().zip(replacement) {
                    let old = std::mem::replace(&mut list.as_vec_mut()[idx], new_value);
                    old.drop_with_heap(heap_inner);
                }
            }
            if list.as_vec().iter().any(|item| matches!(item, Value::Ref(_))) {
                list.set_contains_refs();
                heap_inner.mark_potential_cycle();
            }
            Ok(())
        })?;

        return Ok(Value::None);
    }

    let index = array_index_from_key(key, heap)?;
    let normalized = normalize_index_for_assignment(index, items_id, heap)?;
    let normalized_value = normalize_array_value(value.clone_with_heap(heap), typecode, heap, interns)?;
    with_list_mut(items_id, heap, |heap_inner, list| {
        let old = std::mem::replace(&mut list.as_vec_mut()[normalized], normalized_value);
        old.drop_with_heap(heap_inner);
        if list.as_vec().iter().any(|item| matches!(item, Value::Ref(_))) {
            list.set_contains_refs();
            heap_inner.mark_potential_cycle();
        }
        Ok(())
    })?;

    Ok(Value::None)
}

fn array_delitem(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (self_value, key) = args.get_two_args("array.__delitem__", heap)?;
    defer_drop!(self_value, heap);
    defer_drop!(key, heap);
    let self_id = expect_instance_id(self_value, heap, "array.__delitem__")?;
    let items_id = array_items_id(self_id, heap)?;

    if let Some(slice) = value_as_slice(key, heap) {
        let (start, stop, step) = slice
            .indices(list_len(items_id, heap))
            .map_err(|()| ExcType::value_error_slice_step_zero())?;
        with_list_mut(items_id, heap, |heap_inner, list| {
            if step == 1 {
                let removed: Vec<Value> = list.as_vec_mut().drain(start..stop).collect();
                removed.drop_with_heap(heap_inner);
            } else {
                let mut indices = slice_assignment_indices(start, stop, step, list.as_vec().len());
                indices.sort_unstable_by(|left, right| right.cmp(left));
                for idx in indices {
                    let removed = list.as_vec_mut().remove(idx);
                    removed.drop_with_heap(heap_inner);
                }
            }
            Ok(())
        })?;

        return Ok(Value::None);
    }

    let index = array_index_from_key(key, heap)?;
    with_list_mut(items_id, heap, |heap_inner, list| {
        let len = i64::try_from(list.as_vec().len()).expect("list length fits i64");
        let normalized = if index < 0 { index + len } else { index };
        if normalized < 0 || normalized >= len {
            return Err(SimpleException::new_msg(ExcType::IndexError, "array assignment index out of range").into());
        }
        let idx = usize::try_from(normalized).expect("normalized index is non-negative");
        let removed = list.as_vec_mut().remove(idx);
        removed.drop_with_heap(heap_inner);
        Ok(())
    })?;

    Ok(Value::None)
}

fn array_len(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.__len__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__len__")?;
    let items_id = array_items_id(self_id, heap)?;
    let len = list_len(items_id, heap);
    self_value.drop_with_heap(heap);
    Ok(Value::Int(i64::try_from(len).expect("len fits i64")))
}

fn array_contains(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, item) = args.get_two_args("array.__contains__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__contains__")?;
    let items_id = array_items_id(self_id, heap)?;

    let items = list_items_cloned(items_id, heap);
    let contains = items.iter().any(|value| value.py_eq(&item, heap, interns));
    items.drop_with_heap(heap);

    item.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::Bool(contains))
}

fn array_iter(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.__iter__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__iter__")?;
    let items_id = array_items_id(self_id, heap)?;

    heap.inc_ref(items_id);
    let iter_value = Value::Ref(items_id);
    let iter = OurosIter::new(iter_value, heap, interns)?;
    let iter_id = heap.allocate(HeapData::Iter(iter))?;

    self_value.drop_with_heap(heap);
    Ok(Value::Ref(iter_id))
}

fn array_reversed(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.__reversed__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__reversed__")?;
    let items_id = array_items_id(self_id, heap)?;

    let mut items = list_items_cloned(items_id, heap);
    items.reverse();
    let list_id = heap.allocate(HeapData::List(List::new(items)))?;
    let iter = OurosIter::new(Value::Ref(list_id), heap, interns)?;
    let iter_id = heap.allocate(HeapData::Iter(iter))?;

    self_value.drop_with_heap(heap);
    Ok(Value::Ref(iter_id))
}

fn array_repr(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.__repr__", heap)?;
    defer_drop!(self_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.__repr__")?;
    let (typecode, items_id) = array_state(self_id, heap, interns)?;

    let items = list_items_cloned(items_id, heap);
    defer_drop!(items, heap);
    let repr = if items.is_empty() {
        format!("array('{}')", typecode.as_char())
    } else if typecode == TypeCode::U {
        let mut text = String::new();
        for value in items {
            text.push_str(&value.py_str(heap, interns));
        }
        let text_id = heap.allocate(HeapData::Str(Str::from(text)))?;
        let text_value = Value::Ref(text_id);
        defer_drop!(text_value, heap);
        let text_repr = text_value.py_repr(heap, interns).into_owned();
        format!("array('u', {text_repr})")
    } else {
        let list_id = heap.allocate(HeapData::List(List::new(
            items.iter().map(|v| v.clone_with_heap(heap)).collect(),
        )))?;
        let list_value = Value::Ref(list_id);
        defer_drop!(list_value, heap);
        let list_repr = list_value.py_repr(heap, interns).into_owned();
        format!("array('{}', {list_repr})", typecode.as_char())
    };

    let repr_id = heap.allocate(HeapData::Str(Str::from(repr)))?;
    Ok(Value::Ref(repr_id))
}

fn array_eq(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, other) = args.get_two_args("array.__eq__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__eq__")?;
    let self_items = list_items_cloned(array_items_id(self_id, heap)?, heap);

    let result = if let Some((_, other_items_id)) = array_state_from_value(&other, heap, interns) {
        let other_items = list_items_cloned(other_items_id, heap);
        let equal = self_items.len() == other_items.len()
            && self_items
                .iter()
                .zip(&other_items)
                .all(|(left, right)| left.py_eq(right, heap, interns));
        other_items.drop_with_heap(heap);
        equal
    } else {
        false
    };

    self_items.drop_with_heap(heap);
    other.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::Bool(result))
}

fn array_ne(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let eq = array_eq(heap, interns, args)?;
    if let Value::Bool(value) = eq {
        Ok(Value::Bool(!value))
    } else {
        Ok(Value::Bool(true))
    }
}

fn array_lt(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    array_order_compare(heap, interns, args, Ordering::Less)
}

fn array_le(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    array_order_compare(heap, interns, args, Ordering::Equal)
}

fn array_gt(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    array_order_compare(heap, interns, args, Ordering::Greater)
}

fn array_ge(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, other) = args.get_two_args("array.__ge__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__ge__")?;

    let Some((_, other_items_id)) = array_state_from_value(&other, heap, interns) else {
        other.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Ok(Value::NotImplemented);
    };

    let self_items = list_items_cloned(array_items_id(self_id, heap)?, heap);
    let other_items = list_items_cloned(other_items_id, heap);
    let ordering = lexicographic_compare(&self_items, &other_items, heap, interns);
    self_items.drop_with_heap(heap);
    other_items.drop_with_heap(heap);
    other.drop_with_heap(heap);
    self_value.drop_with_heap(heap);

    let result = match ordering {
        Some(order) => matches!(order, Ordering::Greater | Ordering::Equal),
        None => false,
    };
    Ok(Value::Bool(result))
}

fn array_order_compare(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
    order: Ordering,
) -> RunResult<Value> {
    let (self_value, other) = args.get_two_args("array comparison", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array comparison")?;

    let Some((_, other_items_id)) = array_state_from_value(&other, heap, interns) else {
        other.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Ok(Value::NotImplemented);
    };

    let self_items = list_items_cloned(array_items_id(self_id, heap)?, heap);
    let other_items = list_items_cloned(other_items_id, heap);
    let ordering = lexicographic_compare(&self_items, &other_items, heap, interns);
    self_items.drop_with_heap(heap);
    other_items.drop_with_heap(heap);
    other.drop_with_heap(heap);
    self_value.drop_with_heap(heap);

    let result = match ordering {
        Some(found) if order == Ordering::Equal => matches!(found, Ordering::Less | Ordering::Equal),
        Some(found) => found == order,
        None => false,
    };
    Ok(Value::Bool(result))
}

fn array_index(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_no_kwargs("array.index"));
    }

    let Some(self_value) = positional.next() else {
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("array.index", 2, 0));
    };
    let self_id = expect_instance_id(&self_value, heap, "array.index")?;
    let items_id = array_items_id(self_id, heap)?;

    let Some(target) = positional.next() else {
        self_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("array.index", 2, 1));
    };

    let len = list_len(items_id, heap);
    let start = if let Some(start_value) = positional.next() {
        let parsed = normalize_slice_bound(value_to_i64(&start_value, heap)?, len);
        start_value.drop_with_heap(heap);
        parsed
    } else {
        0
    };
    let end = if let Some(end_value) = positional.next() {
        let parsed = normalize_slice_bound(value_to_i64(&end_value, heap)?, len);
        end_value.drop_with_heap(heap);
        parsed
    } else {
        len
    };
    if let Some(extra) = positional.next() {
        extra.drop_with_heap(heap);
        target.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("array.index", 4, 5));
    }

    let end = end.max(start);
    let items = list_items_cloned(items_id, heap);
    for (offset, item) in items[start..end].iter().enumerate() {
        if target.py_eq(item, heap, interns) {
            let found = start + offset;
            target.drop_with_heap(heap);
            self_value.drop_with_heap(heap);
            items.drop_with_heap(heap);
            return Ok(Value::Int(i64::try_from(found).expect("index fits i64")));
        }
    }

    target.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    items.drop_with_heap(heap);
    Err(SimpleException::new_msg(ExcType::ValueError, "array.index(x): x not in array").into())
}

fn array_count(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, target) = args.get_two_args("array.count", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.count")?;
    let items_id = array_items_id(self_id, heap)?;

    let items = list_items_cloned(items_id, heap);
    let count = items.iter().filter(|value| value.py_eq(&target, heap, interns)).count();
    items.drop_with_heap(heap);

    target.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::Int(i64::try_from(count).expect("count fits i64")))
}

fn array_reverse(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.reverse", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.reverse")?;
    let items_id = array_items_id(self_id, heap)?;
    with_list_mut(items_id, heap, |_, list| {
        list.as_vec_mut().reverse();
        Ok(())
    })?;
    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

fn array_buffer_info(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.buffer_info", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.buffer_info")?;
    let items_id = array_items_id(self_id, heap)?;
    let length = i64::try_from(list_len(items_id, heap)).expect("len fits i64");
    let address = i64::try_from(self_value.public_id()).unwrap_or(i64::MAX);
    self_value.drop_with_heap(heap);
    Ok(allocate_tuple(
        smallvec![Value::Int(address), Value::Int(length)],
        heap,
    )?)
}

fn array_byteswap(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.byteswap", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.byteswap")?;
    let typecode = array_typecode(self_id, heap)?;
    let items_id = array_items_id(self_id, heap)?;

    if typecode.itemsize() == 1 {
        self_value.drop_with_heap(heap);
        return Ok(Value::None);
    }

    let old_items = list_items_cloned(items_id, heap);
    let mut swapped = Vec::with_capacity(old_items.len());
    for value in old_items {
        let mut bytes = encode_value(typecode, &value, heap, None)?;
        bytes.reverse();
        let mut decoded = decode_bytes(typecode, &bytes, heap)?;
        let decoded_value = decoded.pop().expect("decoded one element");
        swapped.push(decoded_value);
        value.drop_with_heap(heap);
    }

    with_list_mut(items_id, heap, |heap_inner, list| {
        let removed: Vec<Value> = list.as_vec_mut().splice(.., swapped).collect();
        removed.drop_with_heap(heap_inner);
        if list.as_vec().iter().any(|item| matches!(item, Value::Ref(_))) {
            list.set_contains_refs();
            heap_inner.mark_potential_cycle();
        }
        Ok(())
    })?;

    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

fn array_tobytes(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.tobytes", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.tobytes")?;
    let typecode = array_typecode(self_id, heap)?;
    let items_id = array_items_id(self_id, heap)?;

    let items = list_items_cloned(items_id, heap);
    let mut bytes = Vec::with_capacity(items.len().saturating_mul(typecode.itemsize()));
    for item in &items {
        bytes.extend_from_slice(&encode_value(typecode, item, heap, Some(interns))?);
    }
    items.drop_with_heap(heap);

    let bytes_id = heap.allocate(HeapData::Bytes(bytes.into()))?;
    self_value.drop_with_heap(heap);
    Ok(Value::Ref(bytes_id))
}

fn array_frombytes(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, bytes_value) = args.get_two_args("array.frombytes", heap)?;
    defer_drop!(self_value, heap);
    defer_drop!(bytes_value, heap);
    let self_id = expect_instance_id(self_value, heap, "array.frombytes")?;
    let typecode = array_typecode(self_id, heap)?;
    let items_id = array_items_id(self_id, heap)?;

    let Some(bytes) = value_as_bytes_slice(bytes_value, heap, interns) else {
        let type_name = value_type_name(bytes_value, heap);
        return Err(ExcType::type_error(format!(
            "a bytes-like object is required, not '{type_name}'"
        )));
    };

    let bytes = bytes.to_vec();
    let values = decode_bytes(typecode, &bytes, heap)?;
    append_many_to_items(items_id, values, heap)?;

    Ok(Value::None)
}

fn array_tolist(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let self_value = args.get_one_arg("array.tolist", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.tolist")?;
    let items_id = array_items_id(self_id, heap)?;

    let list = list_items_cloned(items_id, heap);
    let list_id = heap.allocate(HeapData::List(List::new(list)))?;

    self_value.drop_with_heap(heap);
    Ok(Value::Ref(list_id))
}

fn array_fromlist(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, list_value) = args.get_two_args("array.fromlist", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.fromlist")?;
    let typecode = array_typecode(self_id, heap)?;
    let items_id = array_items_id(self_id, heap)?;

    let input_items = if let Value::Ref(id) = &list_value {
        if let HeapData::List(list) = heap.get(*id) {
            list.as_vec()
                .iter()
                .map(|v| v.clone_with_heap(heap))
                .collect::<Vec<_>>()
        } else {
            list_value.drop_with_heap(heap);
            self_value.drop_with_heap(heap);
            return Err(ExcType::type_error("arg must be list".to_string()));
        }
    } else {
        list_value.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error("arg must be list".to_string()));
    };

    let mut normalized = Vec::with_capacity(input_items.len());
    for item in input_items {
        normalized.push(normalize_array_value(item, typecode, heap, interns)?);
    }

    append_many_to_items(items_id, normalized, heap)?;

    list_value.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    Ok(Value::None)
}

fn array_add(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, other) = args.get_two_args("array.__add__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__add__")?;
    let self_class_id = instance_class_id(self_id, heap)?;
    let (self_typecode, self_items_id) = array_state(self_id, heap, interns)?;

    let Some((other_typecode, other_items_id)) = array_state_from_value(&other, heap, interns) else {
        let other_type = value_type_name(&other, heap);
        other.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "can only append array (not \"{other_type}\") to array"
        )));
    };

    if self_typecode != other_typecode {
        other.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "bad argument type for built-in operation".to_string(),
        ));
    }

    let mut combined = list_items_cloned(self_items_id, heap);
    let other_items = list_items_cloned(other_items_id, heap);
    combined.extend(other_items.iter().map(|value| value.clone_with_heap(heap)));
    other_items.drop_with_heap(heap);

    let result = create_array_instance(self_class_id, self_typecode, combined, heap, interns);
    other.drop_with_heap(heap);
    self_value.drop_with_heap(heap);
    result
}

fn array_iadd(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, other) = args.get_two_args("array.__iadd__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__iadd__")?;
    let (self_typecode, self_items_id) = array_state(self_id, heap, interns)?;

    let Some((other_typecode, other_items_id)) = array_state_from_value(&other, heap, interns) else {
        let other_type = value_type_name(&other, heap);
        other.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "can only extend array with array (not \"{other_type}\")"
        )));
    };

    if self_typecode != other_typecode {
        other.drop_with_heap(heap);
        self_value.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "can only extend with array of same kind".to_string(),
        ));
    }

    let other_items = list_items_cloned(other_items_id, heap);
    append_many_to_items(self_items_id, other_items, heap)?;

    other.drop_with_heap(heap);
    let self_id_copy = self_id;
    self_value.drop_with_heap(heap);
    heap.inc_ref(self_id_copy);
    Ok(Value::Ref(self_id_copy))
}

fn array_mul(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, count_value) = args.get_two_args("array.__mul__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__mul__")?;
    let self_class_id = instance_class_id(self_id, heap)?;
    let typecode = array_typecode(self_id, heap)?;
    let items_id = array_items_id(self_id, heap)?;

    let count = repeat_count_from_value(&count_value, heap)?;
    count_value.drop_with_heap(heap);

    let source = list_items_cloned(items_id, heap);
    let mut repeated = Vec::with_capacity(source.len().saturating_mul(count));
    for _ in 0..count {
        repeated.extend(source.iter().map(|value| value.clone_with_heap(heap)));
    }
    source.drop_with_heap(heap);

    let result = create_array_instance(self_class_id, typecode, repeated, heap, interns);
    self_value.drop_with_heap(heap);
    result
}

fn array_imul(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (self_value, count_value) = args.get_two_args("array.__imul__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__imul__")?;
    let items_id = array_items_id(self_id, heap)?;

    let count = repeat_count_from_value(&count_value, heap)?;
    count_value.drop_with_heap(heap);

    let source = list_items_cloned(items_id, heap);
    let mut repeated = Vec::with_capacity(source.len().saturating_mul(count));
    for _ in 0..count {
        repeated.extend(source.iter().map(|value| value.clone_with_heap(heap)));
    }

    with_list_mut(items_id, heap, |heap_inner, list| {
        let removed: Vec<Value> = list.as_vec_mut().splice(.., repeated).collect();
        removed.drop_with_heap(heap_inner);
        if list.as_vec().iter().any(|item| matches!(item, Value::Ref(_))) {
            list.set_contains_refs();
            heap_inner.mark_potential_cycle();
        }
        Ok(())
    })?;

    source.drop_with_heap(heap);

    let self_id_copy = self_id;
    self_value.drop_with_heap(heap);
    heap.inc_ref(self_id_copy);
    Ok(Value::Ref(self_id_copy))
}

fn array_rmul(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (self_value, count_value) = args.get_two_args("array.__rmul__", heap)?;
    let self_id = expect_instance_id(&self_value, heap, "array.__rmul__")?;
    let self_class_id = instance_class_id(self_id, heap)?;
    let typecode = array_typecode(self_id, heap)?;
    let items_id = array_items_id(self_id, heap)?;

    let count = repeat_count_from_value(&count_value, heap)?;
    count_value.drop_with_heap(heap);

    let source = list_items_cloned(items_id, heap);
    let mut repeated = Vec::with_capacity(source.len().saturating_mul(count));
    for _ in 0..count {
        repeated.extend(source.iter().map(|value| value.clone_with_heap(heap)));
    }
    source.drop_with_heap(heap);

    let result = create_array_instance(self_class_id, typecode, repeated, heap, interns);
    self_value.drop_with_heap(heap);
    result
}

fn create_array_instance(
    class_id: HeapId,
    typecode: TypeCode,
    items: Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    let instance_id = allocate_instance_for_class(class_id, heap)?;
    set_array_typecode_and_items(instance_id, typecode, items, heap, interns)?;
    Ok(Value::Ref(instance_id))
}

fn allocate_instance_for_class(class_id: HeapId, heap: &mut Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let (slot_len, has_dict, _has_weakref) = match heap.get(class_id) {
        HeapData::ClassObject(cls) => (
            cls.slot_layout().len(),
            cls.instance_has_dict(),
            cls.instance_has_weakref(),
        ),
        _ => return Err(ExcType::type_error("object is not a class".to_string())),
    };

    heap.inc_ref(class_id);
    let attrs_id = if has_dict {
        Some(heap.allocate(HeapData::Dict(Dict::new()))?)
    } else {
        None
    };
    let mut slot_values = Vec::with_capacity(slot_len);
    slot_values.resize_with(slot_len, || Value::Undefined);

    let instance = Instance::new(class_id, attrs_id, slot_values, Vec::new());
    heap.allocate(HeapData::Instance(instance)).map_err(Into::into)
}

fn array_state(
    instance_id: HeapId,
    heap: &Heap<impl ResourceTracker>,
    _interns: &Interns,
) -> RunResult<(TypeCode, HeapId)> {
    if let Some(state) = array_state_from_instance(instance_id, heap) {
        Ok(state)
    } else {
        Err(ExcType::type_error("array helper expected instance".to_string()))
    }
}

fn array_state_from_value(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    _interns: &Interns,
) -> Option<(TypeCode, HeapId)> {
    let Value::Ref(instance_id) = value else {
        return None;
    };
    if !matches!(heap.get(*instance_id), HeapData::Instance(_)) {
        return None;
    }
    array_state_from_instance(*instance_id, heap)
}

fn array_state_from_instance(instance_id: HeapId, heap: &Heap<impl ResourceTracker>) -> Option<(TypeCode, HeapId)> {
    let typecode_value = get_instance_attr_by_name(instance_id, ATTR_ARRAY_TYPECODE, heap)?;
    let items_value = get_instance_attr_by_name(instance_id, ATTR_ARRAY_ITEMS, heap)?;
    let typecode = typecode_from_attr_value(typecode_value, heap)?;
    let items_id = match items_value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::List(_)) => *id,
        _ => return None,
    };
    Some((typecode, items_id))
}

fn array_typecode(instance_id: HeapId, heap: &Heap<impl ResourceTracker>) -> RunResult<TypeCode> {
    let Some(typecode_value) = get_instance_attr_by_name(instance_id, ATTR_ARRAY_TYPECODE, heap) else {
        return Err(ExcType::type_error("array helper expected instance".to_string()));
    };
    typecode_from_attr_value(typecode_value, heap)
        .ok_or_else(|| ExcType::type_error("array helper expected instance".to_string()))
}

fn array_items_id(instance_id: HeapId, heap: &Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    let Some(items_value) = get_instance_attr_by_name(instance_id, ATTR_ARRAY_ITEMS, heap) else {
        return Err(ExcType::type_error("array helper expected instance".to_string()));
    };
    let Value::Ref(items_id) = items_value else {
        return Err(ExcType::type_error("array helper expected instance".to_string()));
    };
    if !matches!(heap.get(*items_id), HeapData::List(_)) {
        return Err(ExcType::type_error("array helper expected instance".to_string()));
    }
    Ok(*items_id)
}

fn typecode_from_attr_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> Option<TypeCode> {
    match value {
        Value::Ref(typecode_id) => match heap.get(*typecode_id) {
            HeapData::Str(s) => s.as_str().chars().next().and_then(TypeCode::from_char),
            _ => None,
        },
        _ => None,
    }
}

fn set_array_typecode_and_items(
    instance_id: HeapId,
    typecode: TypeCode,
    items: Vec<Value>,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let typecode_id = heap.allocate(HeapData::Str(Str::from(typecode.as_char().to_string())))?;
    let itemsize = i64::try_from(typecode.itemsize()).expect("itemsize fits i64");
    let items_id = heap.allocate(HeapData::List(List::new(items)))?;

    set_instance_attr_by_name(instance_id, ATTR_ARRAY_TYPECODE, Value::Ref(typecode_id), heap, interns)?;
    set_instance_attr_by_name(instance_id, ATTR_ARRAY_ITEMS, Value::Ref(items_id), heap, interns)?;

    let public_typecode_id = heap.allocate(HeapData::Str(Str::from(typecode.as_char().to_string())))?;
    set_instance_attr_by_name(instance_id, "typecode", Value::Ref(public_typecode_id), heap, interns)?;
    set_instance_attr_by_name(instance_id, "itemsize", Value::Int(itemsize), heap, interns)?;
    Ok(())
}

fn set_instance_attr_by_name(
    instance_id: HeapId,
    name: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<()> {
    let key_id = heap.allocate(HeapData::Str(Str::from(name)))?;
    heap.with_entry_mut(instance_id, |heap_inner, data| -> RunResult<()> {
        let HeapData::Instance(instance) = data else {
            value.drop_with_heap(heap_inner);
            return Err(ExcType::type_error("array helper expected instance".to_string()));
        };
        if let Some(old) = instance.set_attr(Value::Ref(key_id), value, heap_inner, interns)? {
            old.drop_with_heap(heap_inner);
        }
        Ok(())
    })?;
    Ok(())
}

fn get_instance_attr_by_name<'a>(
    instance_id: HeapId,
    name: &str,
    heap: &'a Heap<impl ResourceTracker>,
) -> Option<&'a Value> {
    let HeapData::Instance(instance) = heap.get(instance_id) else {
        return None;
    };
    let attrs = instance.attrs(heap)?;
    for (key, value) in attrs {
        if let Value::Ref(id) = key
            && let HeapData::Str(s) = heap.get(*id)
            && s.as_str() == name
        {
            return Some(value);
        }
    }
    None
}

fn expect_instance_id(value: &Value, heap: &Heap<impl ResourceTracker>, method_name: &str) -> RunResult<HeapId> {
    match value {
        Value::Ref(id) if matches!(heap.get(*id), HeapData::Instance(_)) => Ok(*id),
        _ => Err(ExcType::type_error(format!("{method_name} expected instance"))),
    }
}

fn instance_class_id(instance_id: HeapId, heap: &Heap<impl ResourceTracker>) -> RunResult<HeapId> {
    match heap.get(instance_id) {
        HeapData::Instance(instance) => Ok(instance.class_id()),
        _ => Err(ExcType::type_error("array helper expected instance".to_string())),
    }
}

fn parse_typecode_argument(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<char> {
    let type_error = |msg: String| -> RunResult<char> { Err(ExcType::type_error(msg)) };

    let Some(text) = value.as_either_str(heap) else {
        let type_name = type_name_for_error(&value, heap);
        value.drop_with_heap(heap);
        return type_error(format!(
            "array() argument 1 must be a unicode character, not {type_name}"
        ));
    };

    let typecode = text.as_str(interns).to_string();
    value.drop_with_heap(heap);
    if typecode.chars().count() != 1 {
        return type_error(format!(
            "array() argument 1 must be a unicode character, not a string of length {}",
            typecode.chars().count()
        ));
    }

    Ok(typecode.chars().next().expect("checked length"))
}

fn parse_typecode(value: char) -> RunResult<TypeCode> {
    TypeCode::from_char(value).ok_or_else(|| {
        SimpleException::new_msg(
            ExcType::ValueError,
            "bad typecode (must be b, B, u, h, H, i, I, l, L, q, Q, f or d)",
        )
        .into()
    })
}

fn extend_from_iterable(
    instance_id: HeapId,
    typecode: TypeCode,
    iterable: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    strict_array_only: bool,
) -> RunResult<()> {
    if let Some((other_typecode, other_items_id)) = array_state_from_value(&iterable, heap, interns) {
        if strict_array_only {
            if other_typecode != typecode {
                iterable.drop_with_heap(heap);
                return Err(ExcType::type_error(
                    "can only extend with array of same kind".to_string(),
                ));
            }
            let values = list_items_cloned(other_items_id, heap);
            append_many_to_items(array_items_id(instance_id, heap)?, values, heap)?;
            iterable.drop_with_heap(heap);
            return Ok(());
        }

        let values = list_items_cloned(other_items_id, heap);
        let mut normalized = Vec::with_capacity(values.len());
        for value in values {
            normalized.push(normalize_array_value(value, typecode, heap, interns)?);
        }
        append_many_to_items(array_items_id(instance_id, heap)?, normalized, heap)?;
        iterable.drop_with_heap(heap);
        return Ok(());
    }

    let items_id = array_items_id(instance_id, heap)?;
    let mut iter = OurosIter::new(iterable, heap, interns)?;
    loop {
        match iter.for_next(heap, interns) {
            Ok(Some(item)) => {
                let normalized = normalize_array_value(item, typecode, heap, interns)?;
                with_list_mut(items_id, heap, |heap_inner, list| {
                    list.append(heap_inner, normalized);
                    Ok(())
                })?;
            }
            Ok(None) => break,
            Err(err) => {
                iter.drop_with_heap(heap);
                return Err(err);
            }
        }
    }
    iter.drop_with_heap(heap);
    Ok(())
}

fn append_many(instance_id: HeapId, values: Vec<Value>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<()> {
    let items_id = array_items_id(instance_id, heap)?;
    append_many_to_items(items_id, values, heap)
}

fn append_many_to_items(items_id: HeapId, values: Vec<Value>, heap: &mut Heap<impl ResourceTracker>) -> RunResult<()> {
    with_list_mut(items_id, heap, |heap_inner, list| {
        for value in values {
            list.append(heap_inner, value);
        }
        Ok(())
    })
}

fn normalize_array_value(
    value: Value,
    typecode: TypeCode,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Value> {
    match typecode {
        TypeCode::B => normalize_signed(value, i64::from(i8::MIN), i64::from(i8::MAX), "signed char", heap),
        TypeCode::UB => normalize_unsigned(value, 0, u64::from(u8::MAX), "unsigned byte integer", heap),
        TypeCode::H => normalize_signed(
            value,
            i64::from(i16::MIN),
            i64::from(i16::MAX),
            "signed short integer",
            heap,
        ),
        TypeCode::UH => normalize_unsigned(value, 0, u64::from(u16::MAX), "unsigned short", heap),
        TypeCode::I => normalize_signed(value, i64::from(i32::MIN), i64::from(i32::MAX), "signed integer", heap),
        TypeCode::UI => normalize_unsigned_i(value, u64::from(u32::MAX), heap),
        TypeCode::L => normalize_c_long(value, heap),
        TypeCode::UL => normalize_c_ulong(value, heap),
        TypeCode::Q => normalize_q(value, heap),
        TypeCode::UQ => normalize_uq(value, heap),
        TypeCode::F => normalize_float(value, true, heap),
        TypeCode::D => normalize_float(value, false, heap),
        TypeCode::U => normalize_unicode(value, heap, interns),
    }
}

fn normalize_signed(
    value: Value,
    min: i64,
    max: i64,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let parsed = value_to_i64(&value, heap)?;
    value.drop_with_heap(heap);
    if parsed < min {
        return Err(SimpleException::new_msg(ExcType::OverflowError, format!("{name} is less than minimum")).into());
    }
    if parsed > max {
        return Err(SimpleException::new_msg(ExcType::OverflowError, format!("{name} is greater than maximum")).into());
    }
    Ok(Value::Int(parsed))
}

fn normalize_unsigned(
    value: Value,
    min: u64,
    max: u64,
    name: &str,
    heap: &mut Heap<impl ResourceTracker>,
) -> RunResult<Value> {
    let parsed = value_to_u64(&value, heap).map_err(|err| match err {
        IntParseError::Negative => {
            SimpleException::new_msg(ExcType::OverflowError, format!("{name} is less than minimum")).into()
        }
        IntParseError::TooLarge => {
            SimpleException::new_msg(ExcType::OverflowError, format!("{name} is greater than maximum")).into()
        }
        IntParseError::TypeError => int_type_error(&value, heap),
    })?;
    value.drop_with_heap(heap);
    if parsed < min {
        return Err(SimpleException::new_msg(ExcType::OverflowError, format!("{name} is less than minimum")).into());
    }
    if parsed > max {
        return Err(SimpleException::new_msg(ExcType::OverflowError, format!("{name} is greater than maximum")).into());
    }
    u64_to_value(parsed, heap)
}

fn normalize_unsigned_i(value: Value, max: u64, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let parsed = value_to_u64(&value, heap).map_err(|err| match err {
        IntParseError::Negative => {
            SimpleException::new_msg(ExcType::OverflowError, "can't convert negative value to unsigned int").into()
        }
        IntParseError::TooLarge => {
            SimpleException::new_msg(ExcType::OverflowError, "unsigned int is greater than maximum").into()
        }
        IntParseError::TypeError => int_type_error(&value, heap),
    })?;
    value.drop_with_heap(heap);
    if parsed > max {
        return Err(SimpleException::new_msg(ExcType::OverflowError, "unsigned int is greater than maximum").into());
    }
    u64_to_value(parsed, heap)
}

fn normalize_c_long(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let parsed = value_to_i64(&value, heap);
    value.drop_with_heap(heap);
    parsed.map(Value::Int).map_err(|_| {
        SimpleException::new_msg(ExcType::OverflowError, "Python int too large to convert to C long").into()
    })
}

fn normalize_c_ulong(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let parsed = value_to_u64(&value, heap).map_err(|err| match err {
        IntParseError::Negative => {
            SimpleException::new_msg(ExcType::OverflowError, "can't convert negative value to unsigned int").into()
        }
        IntParseError::TooLarge => SimpleException::new_msg(
            ExcType::OverflowError,
            "Python int too large to convert to C unsigned long",
        )
        .into(),
        IntParseError::TypeError => int_type_error(&value, heap),
    })?;
    value.drop_with_heap(heap);
    u64_to_value(parsed, heap)
}

fn normalize_q(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let parsed = value_to_i64(&value, heap)
        .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "int too big to convert"))?;
    value.drop_with_heap(heap);
    Ok(Value::Int(parsed))
}

fn normalize_uq(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let parsed = value_to_u64(&value, heap).map_err(|err| match err {
        IntParseError::Negative => {
            SimpleException::new_msg(ExcType::OverflowError, "can't convert negative int to unsigned").into()
        }
        IntParseError::TooLarge => SimpleException::new_msg(ExcType::OverflowError, "int too big to convert").into(),
        IntParseError::TypeError => int_type_error(&value, heap),
    })?;
    value.drop_with_heap(heap);
    u64_to_value(parsed, heap)
}

fn normalize_float(value: Value, narrow: bool, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let parsed = match value {
        Value::Float(f) => f,
        Value::Int(i) => i as f64,
        Value::Bool(b) => i64::from(b) as f64,
        Value::Ref(id) => {
            if let HeapData::LongInt(long_int) = heap.get(id) {
                long_int.to_f64().ok_or_else(|| {
                    SimpleException::new_msg(ExcType::OverflowError, "int too large to convert to float")
                })?
            } else {
                let err = int_type_error(&value, heap);
                value.drop_with_heap(heap);
                return Err(err);
            }
        }
        _ => {
            let err = int_type_error(&value, heap);
            value.drop_with_heap(heap);
            return Err(err);
        }
    };
    value.drop_with_heap(heap);

    if narrow {
        let narrowed = parsed as f32;
        Ok(Value::Float(f64::from(narrowed)))
    } else {
        Ok(Value::Float(parsed))
    }
}

fn normalize_unicode(value: Value, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Value> {
    let Some(text) = value.as_either_str(heap).map(|s| s.as_str(interns).to_string()) else {
        let type_name = value_type_name(&value, heap);
        value.drop_with_heap(heap);
        return Err(ExcType::type_error(format!(
            "array item must be a unicode character, not {type_name}"
        )));
    };
    value.drop_with_heap(heap);

    if text.chars().count() != 1 {
        return Err(ExcType::type_error(format!(
            "array item must be a unicode character, not a string of length {}",
            text.chars().count()
        )));
    }

    let str_id = heap.allocate(HeapData::Str(Str::from(text)))?;
    Ok(Value::Ref(str_id))
}

fn encode_value(
    typecode: TypeCode,
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: Option<&Interns>,
) -> RunResult<Vec<u8>> {
    let bytes = match typecode {
        TypeCode::B => {
            let i = value_to_i64(value, heap)?;
            (i as i8).to_ne_bytes().to_vec()
        }
        TypeCode::UB => {
            let u = value_to_u64(value, heap).map_err(|_| ExcType::overflow_shift_count())?;
            (u as u8).to_ne_bytes().to_vec()
        }
        TypeCode::H => {
            let i = value_to_i64(value, heap)?;
            (i as i16).to_ne_bytes().to_vec()
        }
        TypeCode::UH => {
            let u = value_to_u64(value, heap).map_err(|_| ExcType::overflow_shift_count())?;
            (u as u16).to_ne_bytes().to_vec()
        }
        TypeCode::I => {
            let i = value_to_i64(value, heap)?;
            (i as i32).to_ne_bytes().to_vec()
        }
        TypeCode::UI => {
            let u = value_to_u64(value, heap).map_err(|_| ExcType::overflow_shift_count())?;
            (u as u32).to_ne_bytes().to_vec()
        }
        TypeCode::L | TypeCode::Q => {
            let i = value_to_i64(value, heap)?;
            i.to_ne_bytes().to_vec()
        }
        TypeCode::UL | TypeCode::UQ => {
            let u = value_to_u64(value, heap).map_err(|_| ExcType::overflow_shift_count())?;
            u.to_ne_bytes().to_vec()
        }
        TypeCode::F => {
            let f = value_to_f64(value, heap)? as f32;
            f.to_ne_bytes().to_vec()
        }
        TypeCode::D => {
            let f = value_to_f64(value, heap)?;
            f.to_ne_bytes().to_vec()
        }
        TypeCode::U => {
            let interns = interns.ok_or_else(|| ExcType::type_error("missing interns".to_string()))?;
            let text = value.py_str(heap, interns).to_string();
            let mut chars = text.chars();
            let ch = chars.next().ok_or_else(|| {
                SimpleException::new_msg(ExcType::TypeError, "array item must be a unicode character")
            })?;
            let code = ch as u32;
            code.to_ne_bytes().to_vec()
        }
    };
    Ok(bytes)
}

fn decode_bytes(typecode: TypeCode, bytes: &[u8], heap: &mut Heap<impl ResourceTracker>) -> RunResult<Vec<Value>> {
    if !bytes.len().is_multiple_of(typecode.itemsize()) {
        return Err(SimpleException::new_msg(ExcType::ValueError, "bytes length not a multiple of item size").into());
    }

    let mut out = Vec::with_capacity(bytes.len() / typecode.itemsize());
    for chunk in bytes.chunks_exact(typecode.itemsize()) {
        let value = match typecode {
            TypeCode::B => {
                let arr: [u8; 1] = chunk.try_into().expect("chunk size checked");
                Value::Int(i64::from(i8::from_ne_bytes(arr)))
            }
            TypeCode::UB => {
                let arr: [u8; 1] = chunk.try_into().expect("chunk size checked");
                Value::Int(i64::from(u8::from_ne_bytes(arr)))
            }
            TypeCode::H => {
                let arr: [u8; 2] = chunk.try_into().expect("chunk size checked");
                Value::Int(i64::from(i16::from_ne_bytes(arr)))
            }
            TypeCode::UH => {
                let arr: [u8; 2] = chunk.try_into().expect("chunk size checked");
                Value::Int(i64::from(u16::from_ne_bytes(arr)))
            }
            TypeCode::I => {
                let arr: [u8; 4] = chunk.try_into().expect("chunk size checked");
                Value::Int(i64::from(i32::from_ne_bytes(arr)))
            }
            TypeCode::UI => {
                let arr: [u8; 4] = chunk.try_into().expect("chunk size checked");
                u64_to_value(u64::from(u32::from_ne_bytes(arr)), heap)?
            }
            TypeCode::L | TypeCode::Q => {
                let arr: [u8; 8] = chunk.try_into().expect("chunk size checked");
                Value::Int(i64::from_ne_bytes(arr))
            }
            TypeCode::UL | TypeCode::UQ => {
                let arr: [u8; 8] = chunk.try_into().expect("chunk size checked");
                u64_to_value(u64::from_ne_bytes(arr), heap)?
            }
            TypeCode::F => {
                let arr: [u8; 4] = chunk.try_into().expect("chunk size checked");
                Value::Float(f64::from(f32::from_ne_bytes(arr)))
            }
            TypeCode::D => {
                let arr: [u8; 8] = chunk.try_into().expect("chunk size checked");
                Value::Float(f64::from_ne_bytes(arr))
            }
            TypeCode::U => {
                let arr: [u8; 4] = chunk.try_into().expect("chunk size checked");
                let code = u32::from_ne_bytes(arr);
                let Some(ch) = char::from_u32(code) else {
                    return Err(
                        SimpleException::new_msg(ExcType::ValueError, "chr() arg not in range(0x110000)").into(),
                    );
                };
                char_to_value(ch, heap)?
            }
        };
        out.push(value);
    }

    Ok(out)
}

fn value_to_i64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => long_int.to_i64().ok_or_else(ExcType::index_error_int_too_large),
            _ => Err(int_type_error(value, heap)),
        },
        _ => Err(int_type_error(value, heap)),
    }
}

enum IntParseError {
    Negative,
    TooLarge,
    TypeError,
}

fn value_to_u64(value: &Value, heap: &Heap<impl ResourceTracker>) -> Result<u64, IntParseError> {
    match value {
        Value::Int(i) => {
            if *i < 0 {
                Err(IntParseError::Negative)
            } else {
                Ok(*i as u64)
            }
        }
        Value::Bool(b) => Ok(i64::from(*b) as u64),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => {
                if long_int.is_negative() {
                    Err(IntParseError::Negative)
                } else {
                    long_int.to_u64().ok_or(IntParseError::TooLarge)
                }
            }
            _ => Err(IntParseError::TypeError),
        },
        _ => Err(IntParseError::TypeError),
    }
}

fn value_to_f64(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<f64> {
    match value {
        Value::Float(f) => Ok(*f),
        Value::Int(i) => Ok(*i as f64),
        Value::Bool(b) => Ok(i64::from(*b) as f64),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => long_int.to_f64().ok_or_else(|| {
                SimpleException::new_msg(ExcType::OverflowError, "int too large to convert to float").into()
            }),
            _ => Err(int_type_error(value, heap)),
        },
        _ => Err(int_type_error(value, heap)),
    }
}

fn int_type_error(value: &Value, heap: &Heap<impl ResourceTracker>) -> crate::exception_private::RunError {
    let type_name = value.py_type(heap);
    SimpleException::new_msg(
        ExcType::TypeError,
        format!("'{type_name}' object cannot be interpreted as an integer"),
    )
    .into()
}

fn value_as_slice<'a>(value: &Value, heap: &'a Heap<impl ResourceTracker>) -> Option<&'a Slice> {
    let Value::Ref(id) = value else {
        return None;
    };
    let HeapData::Slice(slice) = heap.get(*id) else {
        return None;
    };
    Some(slice)
}

fn normalize_index_for_assignment(index: i64, items_id: HeapId, heap: &Heap<impl ResourceTracker>) -> RunResult<usize> {
    let len = i64::try_from(list_len(items_id, heap)).expect("len fits i64");
    let normalized = if index < 0 { index + len } else { index };
    if normalized < 0 || normalized >= len {
        return Err(SimpleException::new_msg(ExcType::IndexError, "array assignment index out of range").into());
    }
    Ok(usize::try_from(normalized).expect("normalized index is non-negative"))
}

fn array_index_from_key(key: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<i64> {
    match key {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => long_int.to_i64().ok_or_else(ExcType::index_error_int_too_large),
            _ => Err(SimpleException::new_msg(ExcType::TypeError, "array indices must be integers").into()),
        },
        _ => Err(SimpleException::new_msg(ExcType::TypeError, "array indices must be integers").into()),
    }
}

fn value_as_bytes_slice<'a>(
    value: &'a Value,
    heap: &'a Heap<impl ResourceTracker>,
    interns: &'a Interns,
) -> Option<&'a [u8]> {
    match value {
        Value::InternBytes(bytes_id) => Some(interns.get_bytes(*bytes_id)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) | HeapData::Bytearray(bytes) => Some(bytes.as_slice()),
            _ => None,
        },
        _ => None,
    }
}

fn list_len(items_id: HeapId, heap: &Heap<impl ResourceTracker>) -> usize {
    match heap.get(items_id) {
        HeapData::List(list) => list.as_vec().len(),
        _ => 0,
    }
}

fn list_items_cloned(items_id: HeapId, heap: &Heap<impl ResourceTracker>) -> Vec<Value> {
    match heap.get(items_id) {
        HeapData::List(list) => list.as_vec().iter().map(|value| value.clone_with_heap(heap)).collect(),
        _ => Vec::new(),
    }
}

fn with_list_mut<T, Tracker, F>(items_id: HeapId, heap: &mut Heap<Tracker>, f: F) -> RunResult<T>
where
    Tracker: ResourceTracker,
    F: FnOnce(&mut Heap<Tracker>, &mut List) -> RunResult<T>,
{
    heap.with_entry_mut(items_id, |heap_inner, data| {
        let HeapData::List(list) = data else {
            return Err(ExcType::type_error("array helper expected list".to_string()));
        };
        f(heap_inner, list)
    })
}

fn repeat_count_from_value(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<usize> {
    match value {
        Value::Int(i) => {
            if *i <= 0 {
                Ok(0)
            } else {
                usize::try_from(*i).map_err(|_| ExcType::overflow_repeat_count().into())
            }
        }
        Value::Bool(b) => Ok(usize::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(long_int) => {
                if long_int.is_negative() {
                    Ok(0)
                } else {
                    long_int
                        .to_usize()
                        .ok_or_else(|| ExcType::overflow_repeat_count().into())
                }
            }
            _ => Err(ExcType::type_error(format!(
                "can't multiply sequence by non-int-int type '{}'",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "can't multiply sequence by non-int-int type '{}'",
            value.py_type(heap)
        ))),
    }
}

fn u64_to_value(value: u64, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    if let Ok(i) = i64::try_from(value) {
        Ok(Value::Int(i))
    } else {
        LongInt::new(BigInt::from(value)).into_value(heap).map_err(Into::into)
    }
}

fn char_to_value(ch: char, heap: &mut Heap<impl ResourceTracker>) -> RunResult<Value> {
    let string_id = heap.allocate(HeapData::Str(Str::from(ch.to_string())))?;
    Ok(Value::Ref(string_id))
}

fn type_name_for_error(value: &Value, heap: &Heap<impl ResourceTracker>) -> String {
    match value {
        Value::None => "None".to_string(),
        _ => value.py_type(heap).to_string(),
    }
}

fn value_type_name(value: &Value, heap: &Heap<impl ResourceTracker>) -> String {
    match value {
        Value::None => "NoneType".to_string(),
        _ => value.py_type(heap).to_string(),
    }
}

fn normalize_slice_bound(index: i64, len: usize) -> usize {
    if index < 0 {
        let abs = usize::try_from(-index).unwrap_or(usize::MAX);
        len.saturating_sub(abs)
    } else {
        usize::try_from(index).unwrap_or(len).min(len)
    }
}

fn lexicographic_compare(
    left: &[Value],
    right: &[Value],
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Option<Ordering> {
    for (left_item, right_item) in left.iter().zip(right.iter()) {
        if left_item.py_eq(right_item, heap, interns) {
            continue;
        }
        return left_item.py_cmp(right_item, heap, interns);
    }
    Some(left.len().cmp(&right.len()))
}

fn get_slice_items(
    items: &[Value],
    start: usize,
    stop: usize,
    step: i64,
    heap: &mut Heap<impl ResourceTracker>,
) -> Vec<Value> {
    let mut result = Vec::new();
    if let Ok(step_usize) = usize::try_from(step) {
        let mut i = start;
        while i < stop && i < items.len() {
            result.push(items[i].clone_with_heap(heap));
            i += step_usize;
        }
    } else {
        let step_abs = usize::try_from(-step).expect("negative step to positive");
        let step_abs_i64 = i64::try_from(step_abs).expect("step fits i64");
        let mut i = i64::try_from(start).expect("start fits i64");
        let stop_i64 = if stop == items.len() + 1 {
            -1
        } else {
            i64::try_from(stop).expect("stop fits i64")
        };
        while i > stop_i64 {
            result.push(items[usize::try_from(i).expect("i non-negative")].clone_with_heap(heap));
            i -= step_abs_i64;
        }
    }
    result
}

fn slice_assignment_indices(start: usize, stop: usize, step: i64, len: usize) -> Vec<usize> {
    let mut indices = Vec::new();
    if step > 0 {
        let step_usize = usize::try_from(step).expect("positive step");
        let mut i = start;
        while i < stop {
            indices.push(i);
            i += step_usize;
        }
    } else {
        let step_abs = usize::try_from(-step).expect("negative step");
        let step_abs_i64 = i64::try_from(step_abs).expect("step fits i64");
        let mut i = i64::try_from(start).expect("start fits i64");
        let stop_i64 = if stop == len + 1 {
            -1
        } else {
            i64::try_from(stop).expect("stop fits i64")
        };
        while i > stop_i64 {
            indices.push(usize::try_from(i).expect("i non-negative"));
            i -= step_abs_i64;
        }
    }
    indices
}
