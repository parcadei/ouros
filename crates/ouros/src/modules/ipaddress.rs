//! Minimal implementation of the `ipaddress` module.
//!
//! This currently supports `v4_int_to_packed` and `v6_int_to_packed`.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Bytes, Module},
    value::Value,
};

/// `ipaddress` module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum IpaddressFunctions {
    V4IntToPacked,
    V6IntToPacked,
}

/// Creates the `ipaddress` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Ipaddress);
    module.set_attr_text(
        "v4_int_to_packed",
        Value::ModuleFunction(ModuleFunctions::Ipaddress(IpaddressFunctions::V4IntToPacked)),
        heap,
        interns,
    )?;
    module.set_attr_text(
        "v6_int_to_packed",
        Value::ModuleFunction(ModuleFunctions::Ipaddress(IpaddressFunctions::V6IntToPacked)),
        heap,
        interns,
    )?;
    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `ipaddress` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: IpaddressFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        IpaddressFunctions::V4IntToPacked => v4_int_to_packed(heap, interns, args)?,
        IpaddressFunctions::V6IntToPacked => v6_int_to_packed(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(result))
}

/// Implements `ipaddress.v4_int_to_packed(address)`.
fn v4_int_to_packed(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let n = parse_address_arg("v4_int_to_packed", args, heap, interns)?;
    let n_u32 = u32::try_from(n).map_err(|_| ExcType::type_error("address out of range"))?;
    let bytes = n_u32.to_be_bytes().to_vec();
    let id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(Value::Ref(id))
}

/// Implements `ipaddress.v6_int_to_packed(address)`.
fn v6_int_to_packed(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let n = parse_address_arg("v6_int_to_packed", args, heap, interns)?;
    let n_u128 = u128::try_from(n).map_err(|_| ExcType::type_error("address out of range"))?;
    let bytes = n_u128.to_be_bytes().to_vec();
    let id = heap.allocate(HeapData::Bytes(Bytes::new(bytes)))?;
    Ok(Value::Ref(id))
}

/// Parses the required `address` argument.
fn parse_address_arg(
    function_name: &str,
    args: ArgValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<i64> {
    let (positional, kwargs) = args.into_parts();
    let mut positional = positional.into_iter();
    let mut address = positional.next();
    if positional.next().is_some() {
        address.drop_with_heap(heap);
        return Err(ExcType::type_error_too_many_positional(function_name, 1, 2, 0));
    }

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            address.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        let key_name = keyword_name.as_str(interns).to_owned();
        key.drop_with_heap(heap);
        if key_name == "address" {
            if address.is_some() {
                value.drop_with_heap(heap);
                address.drop_with_heap(heap);
                return Err(ExcType::type_error_duplicate_arg(function_name, "address"));
            }
            address = Some(value);
            continue;
        }
        value.drop_with_heap(heap);
        address.drop_with_heap(heap);
        return Err(ExcType::type_error_unexpected_keyword(function_name, &key_name));
    }

    let Some(value) = address else {
        return Err(ExcType::type_error_missing_positional_with_names(
            function_name,
            &["address"],
        ));
    };
    let out = value.as_int(heap);
    value.drop_with_heap(heap);
    out
}
