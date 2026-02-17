//! Minimal `urllib` package stub.
//!
//! The package currently exposes a `parse` attribute that points to the
//! `urllib.parse` submodule, enabling `import urllib.parse` and
//! `from urllib import parse` patterns.

use crate::{
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::{ResourceError, ResourceTracker},
    types::Module,
    value::Value,
};

/// Creates the `urllib` package module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Urllib);
    let parse_module_id = super::urllib_parse::create_module(heap, interns)?;
    module.set_attr_text("parse", Value::Ref(parse_module_id), heap, interns)?;
    heap.allocate(HeapData::Module(module))
}
