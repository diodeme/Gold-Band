//! Hosts the user trusts to serve remote Markdown images.
//!
//! Rendering a remote `<img>` sends a request the document author controls, which can leak the
//! reader's IP or, via a prompt-injected URL, data an Agent was tricked into embedding. Remote
//! images therefore load only from hosts listed here; the user adds a host from the image itself.

use serde::{Deserialize, Serialize};
use url::Url;

pub const CURRENT_REMOTE_IMAGE_TRUST_SCHEMA_VERSION: u32 = 1;
pub const REMOTE_IMAGE_HOST_INVALID: &str = "remote-image.host-invalid";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImageTrust {
    #[serde(default = "remote_image_trust_schema_version")]
    pub schema_version: u32,
    /// Normalized hosts (see [`normalize_remote_image_host`]), sorted and unique.
    #[serde(default)]
    pub trusted_hosts: Vec<String>,
}

impl Default for RemoteImageTrust {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_REMOTE_IMAGE_TRUST_SCHEMA_VERSION,
            trusted_hosts: Vec::new(),
        }
    }
}

impl RemoteImageTrust {
    pub fn is_empty(&self) -> bool {
        self.trusted_hosts.is_empty()
    }

    /// Adds hosts; returns whether the set changed. Input must already be normalized.
    pub fn trust(&mut self, hosts: impl IntoIterator<Item = String>) -> bool {
        let before = self.trusted_hosts.len();
        self.trusted_hosts.extend(hosts);
        self.trusted_hosts.sort();
        self.trusted_hosts.dedup();
        self.trusted_hosts.len() != before
    }

    /// Removes a host; returns whether the set changed. Input must already be normalized.
    pub fn revoke(&mut self, host: &str) -> bool {
        let before = self.trusted_hosts.len();
        self.trusted_hosts.retain(|candidate| candidate != host);
        self.trusted_hosts.len() != before
    }
}

fn remote_image_trust_schema_version() -> u32 {
    CURRENT_REMOTE_IMAGE_TRUST_SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {input}")]
pub struct RemoteImageHostError {
    pub code: &'static str,
    pub input: String,
}

/// Normalizes a bare host (`Example.COM`, `例子.测试`, `example.com:8443`) the same way the
/// WHATWG URL parser does, so the frontend's `new URL(src).host` matches what is stored.
/// Default ports are dropped; non-default ports stay part of the identity.
pub fn normalize_remote_image_host(input: &str) -> Result<String, RemoteImageHostError> {
    let invalid = || RemoteImageHostError {
        code: REMOTE_IMAGE_HOST_INVALID,
        input: input.to_string(),
    };
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.contains(['/', '?', '#', '@', '\\']) {
        return Err(invalid());
    }
    let url = Url::parse(&format!("https://{trimmed}/")).map_err(|_| invalid())?;
    let host = url.host_str().ok_or_else(invalid)?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_idn_and_ports() {
        assert_eq!(
            normalize_remote_image_host(" Static.Dion.BLUE ").unwrap(),
            "static.dion.blue"
        );
        assert_eq!(
            normalize_remote_image_host("例子.测试").unwrap(),
            "xn--fsqu00a.xn--0zwm56d"
        );
        assert_eq!(
            normalize_remote_image_host("example.com:8443").unwrap(),
            "example.com:8443"
        );
        assert_eq!(
            normalize_remote_image_host("example.com:443").unwrap(),
            "example.com"
        );
        assert_eq!(
            normalize_remote_image_host("[::1]:8080").unwrap(),
            "[::1]:8080"
        );
    }

    #[test]
    fn rejects_inputs_that_are_not_bare_hosts() {
        for input in [
            "",
            "  ",
            "https://example.com",
            "example.com/a.png",
            "user@example.com",
            "a b.com",
            "example.com:99999",
        ] {
            let error = normalize_remote_image_host(input).unwrap_err();
            assert_eq!(error.code, REMOTE_IMAGE_HOST_INVALID, "{input}");
            assert_eq!(error.input, input);
        }
    }

    #[test]
    fn trust_and_revoke_are_idempotent_and_keep_hosts_sorted_unique() {
        let mut trust = RemoteImageTrust::default();
        assert!(trust.trust([
            "b.com".to_string(),
            "a.com".to_string(),
            "b.com".to_string()
        ]));
        assert_eq!(trust.trusted_hosts, ["a.com", "b.com"]);
        assert!(!trust.trust(["a.com".to_string()]));
        assert!(trust.revoke("a.com"));
        assert!(!trust.revoke("a.com"));
        assert_eq!(trust.trusted_hosts, ["b.com"]);
    }

    #[test]
    fn deserializes_missing_fields_with_defaults() {
        let trust: RemoteImageTrust = serde_json::from_str("{}").unwrap();
        assert_eq!(trust, RemoteImageTrust::default());
    }
}
