//! Implementation of the `pathlib` module.
//!
//! Provides a minimal implementation of Python's `pathlib` module with:
//! - `Path`: Concrete filesystem path operations
//! - `PurePath`: Pure lexical path operations (POSIX flavor in this runtime)
//! - `PurePosixPath`: POSIX pure path operations
//! - `PureWindowsPath`: Windows pure path operations
//!
//! `Path` supports both pure methods and filesystem methods (which yield external
//! function calls for host resolution). Pure classes expose only lexical operations.

use crate::{
    builtins::Builtins,
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    resource::{ResourceError, ResourceTracker},
    types::{Module, Type},
    value::Value,
};

/// Creates the `pathlib` module and allocates it on the heap.
///
/// Returns a HeapId pointing to the newly allocated module.
///
/// # Panics
///
/// Panics if the required strings have not been pre-interned during prepare phase.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Pathlib);

    // pathlib.Path - concrete path class with filesystem operations
    module.set_attr(
        StaticStrings::PathClass,
        Value::Builtin(Builtins::Type(Type::Path)),
        heap,
        interns,
    );
    // pathlib.PurePath - pure base path class (constructs pure POSIX values)
    module.set_attr(
        StaticStrings::PurePathClass,
        Value::Builtin(Builtins::Type(Type::PurePath)),
        heap,
        interns,
    );
    // pathlib.PurePosixPath - explicit pure POSIX path class
    module.set_attr(
        StaticStrings::PurePosixPathClass,
        Value::Builtin(Builtins::Type(Type::PurePosixPath)),
        heap,
        interns,
    );
    // pathlib.PureWindowsPath - explicit pure Windows path class
    module.set_attr(
        StaticStrings::PureWindowsPathClass,
        Value::Builtin(Builtins::Type(Type::PureWindowsPath)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}
