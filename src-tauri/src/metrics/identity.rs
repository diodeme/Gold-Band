//! Cross-platform user identity (userId) and OS detection for heartbeat reporting.
//!
//! Uses the `whoami` crate for robust username retrieval on Windows, macOS, and Linux
//! without maintaining three separate FFI bindings. Falls back gracefully when the
//! username is unavailable rather than sending `"unknown"`.

use serde::Serialize;

/// Compile-time OS classification sent in heartbeat payloads.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClientOs {
    Windows,
    Macos,
    Linux,
}

/// Abstraction over OS username retrieval so tests can inject deterministic values.
pub trait UserIdProvider: Send + Sync {
    fn username(&self) -> Option<String>;
}

/// Production provider backed by `whoami::username()`.
pub struct WhoamiUserIdProvider;

impl UserIdProvider for WhoamiUserIdProvider {
    fn username(&self) -> Option<String> {
        let raw = whoami::username();
        normalize_user_id(&raw)
    }
}

/// A test/mock provider that returns a fixed value (or `None`).
#[cfg(test)]
pub struct FixedUserIdProvider {
    pub value: Option<String>,
}

#[cfg(test)]
impl UserIdProvider for FixedUserIdProvider {
    fn username(&self) -> Option<String> {
        self.value.clone()
    }
}

/// Trim, reject empty/`unknown`, and lowercase the username.
pub fn normalize_user_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed.eq_ignore_ascii_case("unknown") {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

/// Map the compile target to the three-valued `ClientOs` enum.
pub fn detect_os() -> ClientOs {
    #[cfg(target_os = "windows")]
    {
        ClientOs::Windows
    }
    #[cfg(target_os = "macos")]
    {
        ClientOs::Macos
    }
    #[cfg(target_os = "linux")]
    {
        ClientOs::Linux
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(
            normalize_user_id("  JohnDoe  "),
            Some("johndoe".to_string())
        );
        assert_eq!(normalize_user_id("Alice"), Some("alice".to_string()));
    }

    #[test]
    fn normalize_rejects_empty_and_unknown() {
        assert_eq!(normalize_user_id(""), None);
        assert_eq!(normalize_user_id("   "), None);
        assert_eq!(normalize_user_id("unknown"), None);
        assert_eq!(normalize_user_id("UNKNOWN"), None);
        assert_eq!(normalize_user_id(&"a".repeat(129)), None);
    }

    #[test]
    fn fixed_provider_returns_configured_value() {
        let provider = FixedUserIdProvider {
            value: Some("testuser".to_string()),
        };
        assert_eq!(provider.username().as_deref(), Some("testuser"));

        let empty = FixedUserIdProvider { value: None };
        assert!(empty.username().is_none());
    }

    #[test]
    fn detect_os_matches_compiled_target() {
        let os = detect_os();
        #[cfg(target_os = "windows")]
        assert_eq!(os, ClientOs::Windows);
        #[cfg(target_os = "macos")]
        assert_eq!(os, ClientOs::Macos);
        #[cfg(target_os = "linux")]
        assert_eq!(os, ClientOs::Linux);
    }

    #[test]
    fn client_os_serializes_to_lowercase_string() {
        let json = serde_json::to_string(&ClientOs::Windows).unwrap();
        assert_eq!(json, "\"windows\"");
        let json = serde_json::to_string(&ClientOs::Macos).unwrap();
        assert_eq!(json, "\"macos\"");
        let json = serde_json::to_string(&ClientOs::Linux).unwrap();
        assert_eq!(json, "\"linux\"");
    }
}
