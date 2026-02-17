//! Bytecode VM module for Ouros.
//!
//! This module contains the bytecode representation, compiler, and virtual machine
//! for executing Python code. The bytecode VM replaces the tree-walking interpreter
//! with a stack-based execution model.
//!
//! # Module Structure
//!
//! - `op` - Opcode enum definitions
//! - `code` - Code object containing bytecode and metadata
//! - `builder` - CodeBuilder for emitting bytecode during compilation
//! - `compiler` - AST to bytecode compiler
//! - `vm` - Virtual machine for bytecode execution

pub use code::Code;
pub use compiler::Compiler;
pub use op::Opcode;
pub use vm::{CachedVMBuffers, FrameExit, VM, VMSnapshot};
pub(crate) type CompileError = compiler::CompileError;

mod builder;
mod code;
mod compiler;
mod op;
mod vm;
