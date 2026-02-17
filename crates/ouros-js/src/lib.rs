// napi macros generate code that triggers some clippy lints
#![allow(clippy::needless_pass_by_value)]

//! Node.js/TypeScript bindings for the Ouros sandboxed Python interpreter.
//!
//! This module provides a JavaScript/TypeScript interface to Ouros via napi-rs,
//! allowing execution of sandboxed Python code from Node.js with configurable
//! inputs, resource limits, and external function callbacks.
//!
//! ## Quick Start
//!
//! ```typescript
//! import { Sandbox } from 'ouros';
//!
//! // Simple execution
//! const m = new Sandbox('1 + 2');
//! const result = m.run(); // returns 3
//!
//! // With inputs
//! const m2 = new Sandbox('x + y', { inputs: ['x', 'y'] });
//! const result2 = m2.run({ inputs: { x: 10, y: 20 } }); // returns 30
//!
//! // Iterative execution with external functions
//! const m3 = new Sandbox('external_func()', { externalFunctions: ['external_func'] });
//! let progress = m3.start();
//! if (progress instanceof Snapshot) {
//!     progress = progress.resume({ returnValue: 42 });
//! }
//! ```

mod convert;
mod exceptions;
mod limits;
mod ouros_cls;
mod session_manager;

pub use exceptions::{ExceptionInfo, Frame, JsOurosException, OurosTypingError};
pub use limits::JsResourceLimits;
pub use ouros_cls::{
    ExceptionInput, FutureResultInput, Ouros, OurosComplete, OurosFutureSnapshot, OurosOptions, OurosSnapshot,
    ResumeOptions, RunOptions, SnapshotLoadOptions, StartOptions,
};
pub use session_manager::NapiSessionManager;
