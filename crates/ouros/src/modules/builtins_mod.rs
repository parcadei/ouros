//! Minimal implementation of the `builtins` module.
//!
//! This module exists so `import builtins` succeeds in parity tests.
//! Runtime builtins still come from the VM's global builtins table.

use crate::{
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::ResourceTracker,
    types::Module,
};

/// Creates the `builtins` module and allocates it on the heap.
pub fn create_module(
    heap: &mut Heap<impl ResourceTracker>,
    _interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let module = Module::new(StaticStrings::BuiltinsMod);
    heap.allocate(HeapData::Module(module))
}
