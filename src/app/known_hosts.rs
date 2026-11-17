//! Host-key trust store for the managed "Connections & Terminal" module.
//!
//! Policy (design): a host key is silently accepted and recorded on first
//! contact; on reuse the recorded key must match, otherwise the connection is
//! rejected with a structured `terminal.host-key-mismatch`. This mirrors the
//! OpenSSH `known_hosts` format but lives under the Gold Band user-context dir
//! (`~/.gold-band/context/known_hosts`) so the module never reads or mutates the
//! user's real `~/.ssh/known_hosts`.
//!
//! Only the public key is ever stored — no secret material touches this file.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use ssh_key::PublicKey;

/// Outcome of a host-key check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyTrust {
    /// The key matches the recorded entry.
    Known,
    /// No entry existed; it was recorded for future checks.
    Recorded,
}

#[derive(Debug, thiserror::Error)]
pub enum KnownHostsError {
    #[error("host-key changed for `{host}`")]
    KeyChanged { host: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
}

/// Record of the trusted host keys. Backed by a single file in the
/// OpenSSH-compatible `known_hosts` format, keyed by `[host]:port`.
#[derive(Debug, Clone)]
pub struct KnownHostsStore {
    path: PathBuf,
}

impl KnownHostsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Checks the host key against the store, recording it on first contact.
    /// - `Known`    → a matching entry already exists (accept).
    /// - `Recorded` → the key was new and has been persisted (accept).
    /// - `Err`      → an entry exists but the key differs (reject, mismatch).
    pub fn check_or_record(
        &self,
        host: &str,
        port: u16,
        pubkey: &PublicKey,
    ) -> Result<HostKeyTrust, KnownHostsError> {
        let matcher = host_matcher(host, port);
        let mut matched = false;

        if let Ok(lines) = std::fs::read_to_string(&self.path) {
            for raw in lines.lines() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((hosts, rest)) = line.split_once(char::is_whitespace) else {
                    continue;
                };
                let hosts_match = hosts.split(',').any(|candidate| candidate == matcher);
                if !hosts_match {
                    continue;
                }
                match parse_public_key(rest) {
                    Some(recorded)
                        if recorded.algorithm() == pubkey.algorithm() && recorded == *pubkey =>
                    {
                        matched = true;
                    }
                    Some(_) => {
                        // Same host, different key: reject.
                        return Err(KnownHostsError::KeyChanged {
                            host: host.to_string(),
                        });
                    }
                    None => continue,
                }
            }
        }

        if matched {
            return Ok(HostKeyTrust::Known);
        }

        // First contact (no matching entry): record and accept.
        let recorded_line = format!(
            "{matcher} {}",
            pubkey
                .to_openssh()
                .map_err(|error| KnownHostsError::Parse(error.to_string()))?
        );
        append_line(&self.path, &recorded_line)?;
        Ok(HostKeyTrust::Recorded)
    }
}

/// The select host depends on whether the connection uses the default port.
/// Non-22 ports use `[host]:port` (OpenSSH convention).
fn host_matcher(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Parses `keytype base64 [comment]` into an OpenSSH [`PublicKey`]. Returns
/// `None` when the token does not parse as a key (e.g. a hashed-host alias).
fn parse_public_key(rest: &str) -> Option<PublicKey> {
    let joined = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    PublicKey::from_openssh(&joined).ok()
}

fn append_line(path: &Path, line: &str) -> Result<(), KnownHostsError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(KnownHostsError::Io)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(KnownHostsError::Io)?;
    writeln!(file, "{line}").map_err(KnownHostsError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};

    fn temp_store() -> (KnownHostsStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = KnownHostsStore::new(dir.path().join("known_hosts"));
        (store, dir)
    }

    fn ed25519() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("generate key")
            .public_key()
            .clone()
    }

    #[test]
    fn first_contact_records_and_accepts() {
        let (store, _dir) = temp_store();
        let key = ed25519();
        assert_eq!(
            store.check_or_record("host-a", 22, &key).unwrap(),
            HostKeyTrust::Recorded
        );
        // Re-contact with the same key is now known.
        assert_eq!(
            store.check_or_record("host-a", 22, &key).unwrap(),
            HostKeyTrust::Known
        );
    }

    #[test]
    fn mismatched_key_is_rejected() {
        let (store, _dir) = temp_store();
        let first = ed25519();
        let second = ed25519();
        store.check_or_record("host-b", 22, &first).unwrap();
        let error = store.check_or_record("host-b", 22, &second).unwrap_err();
        assert!(matches!(error, KnownHostsError::KeyChanged { .. }));
    }

    #[test]
    fn different_host_does_not_conflict() {
        let (store, _dir) = temp_store();
        let key = ed25519();
        store.check_or_record("host-c", 22, &key).unwrap();
        // Same key on a different host is recorded independently.
        assert_eq!(
            store.check_or_record("host-d", 22, &key).unwrap(),
            HostKeyTrust::Recorded
        );
    }

    #[test]
    fn non_default_port_uses_bracketed_host() {
        let (store, _dir) = temp_store();
        let key = ed25519();
        store.check_or_record("host-e", 2222, &key).unwrap();
        // An entry for the default port does not match the bracketed host.
        assert_eq!(
            store.check_or_record("host-e", 22, &key).unwrap(),
            HostKeyTrust::Recorded
        );
    }

    #[test]
    fn hashed_and_comment_lines_are_skipped() {
        let (store, dir) = temp_store();
        let key = ed25519();
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, "# a comment\n|1|xxxx|hashed-host\n").unwrap();
        assert_eq!(
            store.check_or_record("host-f", 22, &key).unwrap(),
            HostKeyTrust::Recorded
        );
    }
}
