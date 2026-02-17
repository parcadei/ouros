use std::fmt;

use crate::{bytecode::CompileError, exception_public::Exception, parse::ParseError, resource::ResourceError};

/// Error type for REPL execution, separating failures by pipeline stage.
///
/// Keeping parse/compile/runtime/resource failures distinct lets callers handle
/// user feedback and recovery policies accurately without string matching.
#[derive(Debug, Clone)]
pub enum ReplError {
    /// Parsing failed before bytecode compilation.
    Parse(ParseError),
    /// Bytecode compilation failed after parsing/preparation succeeded.
    Compile(CompileError),
    /// Python runtime raised an exception while executing bytecode.
    Runtime(Exception),
    /// A resource limit was exceeded while executing bytecode.
    Resource(ResourceError),
}

impl fmt::Display for ReplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "parse error: {error:?}"),
            Self::Compile(error) => write!(f, "compile error: {error:?}"),
            Self::Runtime(error) => write!(f, "{error}"),
            Self::Resource(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReplError {}

impl From<ParseError> for ReplError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<CompileError> for ReplError {
    fn from(error: CompileError) -> Self {
        Self::Compile(error)
    }
}

impl From<Exception> for ReplError {
    fn from(error: Exception) -> Self {
        Self::Runtime(error)
    }
}

impl From<ResourceError> for ReplError {
    fn from(error: ResourceError) -> Self {
        Self::Resource(error)
    }
}
