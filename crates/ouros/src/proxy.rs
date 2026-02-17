/// Stable host-managed proxy identifier.
///
/// Proxy values are immediate VM values (not heap references) so host integrations
/// can exchange opaque handles without exposing host objects inside the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ProxyId(u32);

impl ProxyId {
    /// Creates a proxy ID from a raw integer.
    #[must_use]
    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw integer identifier.
    #[must_use]
    pub fn raw(self) -> u32 {
        self.0
    }
}
