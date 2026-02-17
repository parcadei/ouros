//! Capability-based permission system for sandboxed execution.
//!
//! Capabilities control what external functions the VM is allowed to call and with
//! what arguments. They are checked at the yield boundary — the point where the VM
//! pauses and asks the host to execute an external function or proxy operation.
//!
//! Without capabilities, the yield boundary is purely architectural. With capabilities,
//! it becomes a security boundary: the VM can request any operation, but the host only
//! fulfills requests that match the session's capability set.
//!
//! # Usage
//!
//! ```
//! use ouros::capability::{Capability, CapabilitySet};
//!
//! let caps = CapabilitySet::new(vec![
//!     Capability::CallFunction("read_file".into()),
//!     Capability::CallFunction("fetch".into()),
//! ]);
//!
//! assert!(caps.allows_function("read_file"));
//! assert!(!caps.allows_function("exec_command"));
//! ```

use std::fmt;

/// A single permission grant.
///
/// Each variant represents a class of operation the sandbox is allowed to perform.
/// The capability is checked against the function name and arguments at the yield
/// boundary before the host executes the operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Permission to call a specific external function by name.
    ///
    /// This is the most common capability — it allows the VM to invoke a named
    /// external function that was registered at session creation time.
    CallFunction(String),

    /// Permission to call any external function (wildcard).
    ///
    /// Use with caution — this bypasses per-function checks. Appropriate for
    /// trusted development environments.
    CallAnyFunction,

    /// Permission to perform operations on proxy objects.
    ///
    /// Proxy calls are method invocations on host-managed opaque objects.
    /// Without this capability, proxy method calls are denied at the yield boundary.
    ProxyAccess,

    /// Custom capability identified by a string key.
    ///
    /// For domain-specific permissions that don't fit the built-in categories.
    /// The host is responsible for interpreting these during external call handling.
    Custom(String),
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CallFunction(name) => write!(f, "call:{name}"),
            Self::CallAnyFunction => f.write_str("call:*"),
            Self::ProxyAccess => f.write_str("proxy:*"),
            Self::Custom(key) => write!(f, "custom:{key}"),
        }
    }
}

/// Error returned when an operation is denied by the capability set.
#[derive(Debug, Clone)]
pub struct PermissionDenied {
    /// Human-readable description of the denied operation.
    pub operation: String,
    /// Capability that would have been required.
    pub required: String,
}

impl fmt::Display for PermissionDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PermissionError: {} denied (requires capability '{}')",
            self.operation, self.required
        )
    }
}

impl std::error::Error for PermissionDenied {}

/// A set of granted capabilities for a sandbox session.
///
/// The capability set is immutable once created — capabilities cannot be escalated
/// during execution. A forked session inherits its parent's capabilities or a subset.
///
/// An empty capability set (`CapabilitySet::none()`) denies all external operations,
/// making the sandbox a pure computation environment with no host interaction.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    capabilities: Vec<Capability>,
}

impl CapabilitySet {
    /// Creates a new capability set with the given permissions.
    #[must_use]
    pub fn new(capabilities: Vec<Capability>) -> Self {
        Self { capabilities }
    }

    /// Creates an empty capability set that denies everything.
    ///
    /// This is the most restrictive profile — the VM can compute but cannot
    /// call any external functions or access proxy objects.
    #[must_use]
    pub fn none() -> Self {
        Self {
            capabilities: Vec::new(),
        }
    }

    /// Creates a capability set that allows all operations.
    ///
    /// Equivalent to running without capabilities — for trusted environments.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            capabilities: vec![Capability::CallAnyFunction, Capability::ProxyAccess],
        }
    }

    /// Checks whether a specific external function call is allowed.
    ///
    /// Returns `Ok(())` if the function name matches a `CallFunction` capability
    /// or if `CallAnyFunction` is granted. Returns `Err(PermissionDenied)` otherwise.
    pub fn check_function_call(&self, function_name: &str) -> Result<(), PermissionDenied> {
        for cap in &self.capabilities {
            match cap {
                Capability::CallAnyFunction => return Ok(()),
                Capability::CallFunction(name) if name == function_name => return Ok(()),
                _ => {}
            }
        }
        Err(PermissionDenied {
            operation: format!("call to external function '{function_name}'"),
            required: format!("call:{function_name}"),
        })
    }

    /// Checks whether proxy object access is allowed.
    ///
    /// Returns `Ok(())` if `ProxyAccess` is granted. Returns `Err(PermissionDenied)`
    /// otherwise.
    pub fn check_proxy_access(&self, method: &str) -> Result<(), PermissionDenied> {
        for cap in &self.capabilities {
            if matches!(cap, Capability::ProxyAccess) {
                return Ok(());
            }
        }
        Err(PermissionDenied {
            operation: format!("proxy method call '{method}'"),
            required: "proxy:*".into(),
        })
    }

    /// Returns `true` if the given function name is allowed.
    #[must_use]
    pub fn allows_function(&self, function_name: &str) -> bool {
        self.check_function_call(function_name).is_ok()
    }

    /// Returns `true` if proxy access is allowed.
    #[must_use]
    pub fn allows_proxy(&self) -> bool {
        self.check_proxy_access("").is_ok()
    }

    /// Creates a subset of this capability set, retaining only capabilities
    /// that also appear in `restrict`.
    ///
    /// Used when forking sessions to narrow permissions:
    /// ```
    /// # use ouros::capability::{Capability, CapabilitySet};
    /// let parent = CapabilitySet::new(vec![
    ///     Capability::CallFunction("read".into()),
    ///     Capability::CallFunction("write".into()),
    /// ]);
    /// let child = parent.subset(&[Capability::CallFunction("read".into())]);
    /// assert!(child.allows_function("read"));
    /// assert!(!child.allows_function("write"));
    /// ```
    #[must_use]
    pub fn subset(&self, restrict: &[Capability]) -> Self {
        let capabilities = self
            .capabilities
            .iter()
            .filter(|cap| restrict.contains(cap))
            .cloned()
            .collect();
        Self { capabilities }
    }

    /// Returns the capabilities as a slice for inspection.
    #[must_use]
    pub fn as_slice(&self) -> &[Capability] {
        &self.capabilities
    }
}

impl fmt::Display for CapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.capabilities.is_empty() {
            return f.write_str("CapabilitySet(none)");
        }
        f.write_str("CapabilitySet(")?;
        for (i, cap) in self.capabilities.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{cap}")?;
        }
        f.write_str(")")
    }
}
