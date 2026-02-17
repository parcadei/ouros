//! Implementation of the isinstance() and issubclass() builtin functions.

use super::{Builtins, BuiltinsFunctions};
use crate::{
    args::ArgValues,
    defer_drop,
    exception_private::{ExcType, RunResult},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::{PyTrait, Type},
    value::{Marker, Value},
};

/// Implementation of the isinstance() builtin function.
///
/// Checks if an object is an instance of a class or a tuple of classes.
/// For user-defined class instances, checks the instance's class MRO
/// to support inheritance (isinstance(dog, Animal) == True when Dog(Animal)).
pub fn builtin_isinstance(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    interns: &Interns,
) -> RunResult<Value> {
    let (obj, classinfo) = args.get_two_args("isinstance", heap)?;
    defer_drop!(obj, heap);
    defer_drop!(classinfo, heap);

    // For user-defined class instances, extract the class_id and MRO for matching
    let instance_class_id = if let Value::Ref(heap_id) = &obj {
        if let HeapData::Instance(inst) = heap.get(*heap_id) {
            Some(inst.class_id())
        } else {
            None
        }
    } else {
        None
    };

    let obj_type = obj.py_type(heap);

    match isinstance_check(obj_type, instance_class_id, classinfo, heap, interns)? {
        Some(result) => Ok(Value::Bool(result)),
        None => Err(ExcType::isinstance_arg2_error()),
    }
}

/// Implementation of the issubclass() builtin function.
///
/// Checks if a class is a subclass of another class or tuple of classes.
/// Uses MRO for user-defined classes.
pub fn builtin_issubclass(
    heap: &mut Heap<impl ResourceTracker>,
    args: ArgValues,
    interns: &Interns,
) -> RunResult<Value> {
    let (cls_val, classinfo) = args.get_two_args("issubclass", heap)?;
    defer_drop!(cls_val, heap);
    defer_drop!(classinfo, heap);

    match issubclass_check(cls_val, classinfo, heap, interns)? {
        Some(result) => Ok(Value::Bool(result)),
        None => Err(ExcType::type_error(
            "issubclass() arg 2 must be a class, a tuple of classes, or a union".to_string(),
        )),
    }
}

/// Recursively checks if obj_type matches classinfo for isinstance().
///
/// Returns `Ok(true)` if the type matches, `Ok(false)` if it doesn't,
/// or `Err(())` if classinfo is invalid (not a type or tuple of types).
///
/// Supports:
/// - Single types: `isinstance(x, int)`
/// - Exception types: `isinstance(err, ValueError)` or `isinstance(err, LookupError)`
/// - User-defined classes with MRO: `isinstance(dog, Animal)` when Dog inherits from Animal
/// - Nested tuples: `isinstance(x, (int, (str, bytes)))`
/// - `object` type: all instances are instances of `object`
fn isinstance_check(
    obj_type: Type,
    instance_class_id: Option<HeapId>,
    classinfo: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<bool>> {
    if let Some(items) = union_classinfo_items(classinfo, heap) {
        return tuple_any(items, heap, |value, heap| {
            isinstance_check(obj_type, instance_class_id, value, heap, interns)
        });
    }
    if matches!(classinfo, Value::Marker(Marker(StaticStrings::Generic))) {
        return Ok(Some(true));
    }

    match classinfo {
        // isinstance(x, type)
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => Ok(Some(obj_type.is_instance_of(Type::Type))),

        // Single builtin type: isinstance(x, int)
        Value::Builtin(Builtins::Type(t)) => {
            // Special case: isinstance(instance, object) is always True for user class instances
            if *t == Type::Object && instance_class_id.is_some() {
                return Ok(Some(true));
            }
            if let Some(inst_cls_id) = instance_class_id {
                let builtin_id = heap.builtin_class_id(*t)?;
                if let HeapData::ClassObject(inst_cls) = heap.get(inst_cls_id) {
                    return Ok(Some(inst_cls.is_subclass_of(inst_cls_id, builtin_id)));
                }
                return Ok(Some(false));
            }
            Ok(Some(obj_type.is_instance_of(*t)))
        }
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => {
            if let Some(inst_cls_id) = instance_class_id {
                let builtin_id = heap.builtin_class_id(Type::Type)?;
                if let HeapData::ClassObject(inst_cls) = heap.get(inst_cls_id) {
                    return Ok(Some(inst_cls.is_subclass_of(inst_cls_id, builtin_id)));
                }
                return Ok(Some(false));
            }
            Ok(Some(obj_type.is_instance_of(Type::Type)))
        }

        // Exception type: isinstance(err, ValueError) or isinstance(err, LookupError)
        Value::Builtin(Builtins::ExcType(handler_type)) => {
            // Check exception hierarchy using is_subclass_of
            if matches!(obj_type, Type::Exception(exc_type) if exc_type.is_subclass_of(*handler_type)) {
                return Ok(Some(true));
            }
            // For user-defined exception instances (preserved as Instance), check via MRO
            if let Some(inst_cls_id) = instance_class_id {
                // First get the builtin exception class ID (this may mutate heap)
                let builtin_exc_id_result = heap.builtin_class_id(Type::Exception(*handler_type));
                if let Ok(builtin_exc_id) = builtin_exc_id_result {
                    // Now check subclass relationship
                    if let HeapData::ClassObject(inst_cls) = heap.get(inst_cls_id) {
                        return Ok(Some(inst_cls.is_subclass_of(inst_cls_id, builtin_exc_id)));
                    }
                }
            }
            Ok(Some(false))
        }

        // Ref could be a ClassObject (user class) or a Tuple
        Value::Ref(id) => {
            match heap.get(*id) {
                // User-defined class: isinstance(obj, MyClass)
                HeapData::ClassObject(_) => {
                    // Check if the instance's class is this class or a subclass (via MRO)
                    if let Some(inst_cls_id) = instance_class_id {
                        Ok(Some(class_matches_classinfo(inst_cls_id, *id, heap, interns)))
                    } else {
                        Ok(Some(false))
                    }
                }
                // Tuple of types (possibly nested): isinstance(x, (int, (str, bytes)))
                HeapData::Tuple(tuple) => {
                    let items: Vec<Value> = tuple.as_vec().iter().map(Value::copy_for_extend).collect();
                    tuple_any(items, heap, |value, heap| {
                        isinstance_check(obj_type, instance_class_id, value, heap, interns)
                    })
                }
                _ => Ok(None), // Not a class or tuple - invalid
            }
        }
        _ => Ok(None), // Invalid classinfo
    }
}

/// Checks if cls is a subclass of classinfo for issubclass().
///
/// Supports user-defined classes with MRO, builtin types, and tuples.
fn issubclass_check(
    cls: &Value,
    classinfo: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<bool>> {
    if let Some(items) = union_classinfo_items(classinfo, heap) {
        return tuple_any(items, heap, |value, heap| issubclass_check(cls, value, heap, interns));
    }
    if matches!(classinfo, Value::Marker(Marker(StaticStrings::Generic))) {
        return Ok(Some(true));
    }

    // Get the class HeapId (cls must be a class)
    let cls_id = match cls {
        Value::Ref(id) => {
            if matches!(heap.get(*id), HeapData::ClassObject(_)) {
                Some(*id)
            } else {
                return Ok(None);
            }
        }
        Value::Builtin(Builtins::Type(t)) => {
            // Builtin type: issubclass(int, object) etc
            return issubclass_builtin_check(*t, classinfo, heap, interns);
        }
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => {
            return issubclass_builtin_check(Type::Type, classinfo, heap, interns);
        }
        Value::Builtin(Builtins::ExcType(exc_type)) => {
            // Builtin exception class: issubclass(ValueError, Exception) etc.
            return issubclass_exception_check(*exc_type, classinfo, heap, interns);
        }
        _ => return Ok(None),
    };
    let Some(cls_id) = cls_id else {
        return Ok(None);
    };

    match classinfo {
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => {
            let builtin_id = heap.builtin_class_id(Type::Type)?;
            if let HeapData::ClassObject(cls_obj) = heap.get(cls_id) {
                Ok(Some(cls_obj.is_subclass_of(cls_id, builtin_id)))
            } else {
                Ok(Some(false))
            }
        }
        // issubclass(MyClass, object)
        Value::Builtin(Builtins::Type(info_t)) => {
            let builtin_id = heap.builtin_class_id(*info_t)?;
            if let HeapData::ClassObject(cls_obj) = heap.get(cls_id) {
                Ok(Some(cls_obj.is_subclass_of(cls_id, builtin_id)))
            } else {
                Ok(Some(false))
            }
        }
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => {
            let builtin_id = heap.builtin_class_id(Type::Type)?;
            if let HeapData::ClassObject(cls_obj) = heap.get(cls_id) {
                Ok(Some(cls_obj.is_subclass_of(cls_id, builtin_id)))
            } else {
                Ok(Some(false))
            }
        }
        // issubclass(MyError, Exception)
        Value::Builtin(Builtins::ExcType(info_exc_type)) => {
            let builtin_exc_id = heap.builtin_class_id(Type::Exception(*info_exc_type))?;
            if let HeapData::ClassObject(cls_obj) = heap.get(cls_id) {
                Ok(Some(cls_obj.is_subclass_of(cls_id, builtin_exc_id)))
            } else {
                Ok(Some(false))
            }
        }

        Value::Ref(info_id) => match heap.get(*info_id) {
            HeapData::ClassObject(_) => Ok(Some(class_matches_classinfo(cls_id, *info_id, heap, interns))),
            HeapData::Tuple(tuple) => {
                let items: Vec<Value> = tuple.as_vec().iter().map(Value::copy_for_extend).collect();
                tuple_any(items, heap, |value, heap| issubclass_check(cls, value, heap, interns))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

/// Checks issubclass for builtin types (e.g., issubclass(bool, int)).
fn issubclass_builtin_check(
    t: Type,
    classinfo: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<bool>> {
    if let Some(items) = union_classinfo_items(classinfo, heap) {
        return tuple_any(items, heap, |value, heap| {
            issubclass_builtin_check(t, value, heap, interns)
        });
    }
    if matches!(classinfo, Value::Marker(Marker(StaticStrings::Generic))) {
        return Ok(Some(true));
    }

    match classinfo {
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => Ok(Some(t.is_instance_of(Type::Type))),
        Value::Builtin(Builtins::Type(info_t)) => Ok(Some(t.is_instance_of(*info_t))),
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => Ok(Some(t.is_instance_of(Type::Type))),
        Value::Builtin(Builtins::ExcType(_)) => Ok(Some(false)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => {
                let items: Vec<Value> = tuple.as_vec().iter().map(Value::copy_for_extend).collect();
                tuple_any(items, heap, |value, heap| {
                    issubclass_builtin_check(t, value, heap, interns)
                })
            }
            HeapData::ClassObject(_) => {
                let cls_value = Value::Builtin(Builtins::Type(t));
                let result = crate::modules::abc::is_virtual_subclass_registered(*id, &cls_value, heap)
                    || crate::modules::abc::subclasshook_len_matches(*id, &cls_value, heap, interns);
                Ok(Some(result))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

/// Checks issubclass for builtin exception classes (e.g., issubclass(ValueError, Exception)).
fn issubclass_exception_check(
    exc_type: crate::exception_private::ExcType,
    classinfo: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<bool>> {
    if let Some(items) = union_classinfo_items(classinfo, heap) {
        return tuple_any(items, heap, |value, heap| {
            issubclass_exception_check(exc_type, value, heap, interns)
        });
    }

    match classinfo {
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => Ok(Some(false)),
        Value::Builtin(Builtins::ExcType(info_exc_type)) => Ok(Some(exc_type.is_subclass_of(*info_exc_type))),
        Value::Builtin(Builtins::Type(info_t)) => Ok(Some(*info_t == Type::Object)),
        Value::Builtin(Builtins::Function(BuiltinsFunctions::Type)) => Ok(Some(false)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Tuple(tuple) => {
                let items: Vec<Value> = tuple.as_vec().iter().map(Value::copy_for_extend).collect();
                tuple_any(items, heap, |value, heap| {
                    issubclass_exception_check(exc_type, value, heap, interns)
                })
            }
            HeapData::ClassObject(_) => {
                let cls_value = Value::Builtin(Builtins::ExcType(exc_type));
                let result = crate::modules::abc::is_virtual_subclass_registered(*id, &cls_value, heap)
                    || crate::modules::abc::subclasshook_len_matches(*id, &cls_value, heap, interns);
                Ok(Some(result))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

/// Returns whether `cls_id` matches user class `classinfo_id` via nominal ABC checks.
fn class_matches_classinfo(
    cls_id: HeapId,
    classinfo_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> bool {
    if let HeapData::ClassObject(cls_obj) = heap.get(cls_id)
        && cls_obj.is_subclass_of(cls_id, classinfo_id)
    {
        return true;
    }

    heap.inc_ref(cls_id);
    let cls_value = Value::Ref(cls_id);
    let result = crate::modules::abc::is_virtual_subclass_registered(classinfo_id, &cls_value, heap)
        || crate::modules::abc::subclasshook_len_matches(classinfo_id, &cls_value, heap, interns);
    cls_value.drop_with_heap(heap);
    result
}

/// Returns union arguments when classinfo is a `typing.Union[...]`-style alias.
///
/// `issubclass()` and `isinstance()` both accept unions as arg2, so we normalize
/// those to the same recursive tuple-processing path used for tuples.
fn union_classinfo_items(classinfo: &Value, heap: &Heap<impl ResourceTracker>) -> Option<Vec<Value>> {
    let Value::Ref(id) = classinfo else {
        return None;
    };
    let HeapData::GenericAlias(alias) = heap.get(*id) else {
        return None;
    };
    if !matches!(alias.origin(), Value::Marker(Marker(StaticStrings::UnionType))) {
        return None;
    }
    Some(alias.args().iter().map(Value::copy_for_extend).collect())
}

/// Processes tuple classinfo entries, ensuring temporary values are refcount-safe.
///
/// Copies values with `copy_for_extend()`, increments refs before use, then
/// drops them with the heap regardless of early returns.
fn tuple_any<T: ResourceTracker, F>(items: Vec<Value>, heap: &mut Heap<T>, mut check: F) -> RunResult<Option<bool>>
where
    F: FnMut(&Value, &mut Heap<T>) -> RunResult<Option<bool>>,
{
    let mut iter = items.into_iter();
    while let Some(item) = iter.next() {
        if let Value::Ref(id) = &item {
            heap.inc_ref(*id);
        }
        let result = match check(&item, heap) {
            Ok(value) => value,
            Err(err) => {
                item.drop_with_heap(heap);
                drop_copied_values(iter, heap);
                return Err(err);
            }
        };
        item.drop_with_heap(heap);
        match result {
            Some(true) => {
                drop_copied_values(iter, heap);
                return Ok(Some(true));
            }
            None => {
                drop_copied_values(iter, heap);
                return Ok(None);
            }
            Some(false) => {}
        }
    }
    Ok(Some(false))
}

/// Drops copied tuple values safely by balancing temporary refcounts.
fn drop_copied_values<T: ResourceTracker, I: Iterator<Item = Value>>(iter: I, heap: &mut Heap<T>) {
    for value in iter {
        if let Value::Ref(id) = &value {
            heap.inc_ref(*id);
        }
        value.drop_with_heap(heap);
    }
}
