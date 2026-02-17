//! Minimal `concurrent` package stub exposing `concurrent.futures`.

use crate::{
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::{ResourceError, ResourceTracker},
    types::Module,
    value::Value,
};

pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let module_name = interns
        .try_get_str_id("concurrent")
        .unwrap_or_else(|| StaticStrings::EmptyString.into());
    let mut module = Module::new(module_name);
    let futures_id = super::concurrent_futures::create_module(heap, interns)?;
    module.set_attr_text("futures", Value::Ref(futures_id), heap, interns)?;
    heap.allocate(HeapData::Module(module))
}
