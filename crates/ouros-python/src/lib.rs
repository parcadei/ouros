//! Python bindings for the Ouros sandboxed Python interpreter.
//!
//! This module provides a Python interface to Ouros, allowing execution of
//! sandboxed Python code with configurable resource limits and external
//! function callbacks.

mod convert;
mod dataclass;
mod exceptions;
mod external;
mod limits;
mod ouros_cls;
mod session_manager;

use std::sync::OnceLock;

// Use `::ouros` to refer to the external crate (not the pymodule)
pub use exceptions::{OurosError, OurosRuntimeError, OurosSyntaxError, OurosTypingError, PyFrame};
pub use ouros_cls::{PyOuros, PyOurosComplete, PyOurosFutureSnapshot, PyOurosSnapshot};
use pyo3::prelude::*;
pub use session_manager::PySessionManager;

/// Returns the package version, converting Cargo's format to Python's PEP 440.
fn get_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();

    VERSION.get_or_init(|| {
        let version = env!("CARGO_PKG_VERSION");
        // cargo uses "1.0-alpha1" etc. while python uses "1.0.0a1", this is not full compatibility,
        // but it's good enough for now
        // see https://docs.rs/semver/1.0.9/semver/struct.Version.html#method.parse for rust spec
        // see https://peps.python.org/pep-0440/ for python spec
        // it seems the dot after "alpha/beta" e.g. "-alpha.1" is not necessary, hence why this works
        version.replace("-alpha", "a").replace("-beta", "b")
    })
}

/// Ouros - A sandboxed Python interpreter written in Rust.
#[pymodule]
mod _ouros {
    use pyo3::prelude::*;

    #[pymodule_export]
    use super::OurosError as SandboxError;
    #[pymodule_export]
    use super::OurosRuntimeError as SandboxRuntimeError;
    #[pymodule_export]
    use super::OurosSyntaxError as SandboxSyntaxError;
    #[pymodule_export]
    use super::OurosTypingError as SandboxTypingError;
    #[pymodule_export]
    use super::PyFrame as Frame;
    #[pymodule_export]
    use super::PyOuros as Sandbox;
    #[pymodule_export]
    use super::PyOurosComplete as Complete;
    #[pymodule_export]
    use super::PyOurosFutureSnapshot as FutureSnapshot;
    #[pymodule_export]
    use super::PyOurosSnapshot as Snapshot;
    #[pymodule_export]
    use super::PySessionManager as SessionManager;
    use super::get_version;

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", get_version())?;
        Ok(())
    }
}
