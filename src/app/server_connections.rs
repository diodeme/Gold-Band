//! User-level server connection configuration for the managed "Connections &
//! Terminal" module.
//!
//! Connections are a user-level (workspace-independent) entity persisted to a
//! standalone JSON file under `~/.gold-band/context`. Secrets never live in the
//! JSON config — they live in the OS credential store (see [`super::credentials`])
//! and are referenced only by the connection stable id. Connection id is the
//! canonical identity; `name` is display-only and only used for duplicate
//! validation.
//!
//! Reference integrity rules:
//! - `jumpRef` may not self-reference, may not form a cycle, and must point to an
//!   existing connection.
//! - Deleting a connection that is referenced as a jump gateway by other
//!   connections is rejected (`connection.jump-referenced`) so we never silently
//!   leave dangling references. The referencing connections must be re-pointed
//!   first.
//!
//! This mirrors the established `auto_template` persistence pattern (atomic JSON
//! write via `storage::write_json`, user-context dir, structured errors).

use std::collections::BTreeSet;

use anyhow::Result;

use crate::domain::VERSION;
use crate::storage::{read_json, write_json};

use super::App;
use super::credentials::CredentialStore;
use super::ids::{generate_uuid, now_rfc3339_like};
use super::ssh_sessions::{SshAuth, SshOpenRequest};

/// Upper bound for the recent-connection history. It is metadata only — it never
/// sits on a critical path.
const RECENT_CONNECTION_LIMIT: usize = 50;

const SERVER_CONNECTION_ID_PREFIX: &str = "conn-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionAuthMethod {
    Password,
    PublicKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionEnvironment {
    Test,
    Stage,
    Prod,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConnection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: ConnectionAuthMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump_ref: Option<String>,
    pub env: ConnectionEnvironment,
    #[serde(default, alias = "created_at")]
    pub created_at: String,
    #[serde(default, alias = "updated_at")]
    pub updated_at: String,
}

/// Non-secret, agent-visible spec for a server connection. This is what we hand
/// to the agent when a connection is attached to a conversation: the agent learns
/// WHAT/WHERE a host is and drives it through app-exposed tools, but never sees
/// `key_path` (a private-key path) nor the OS-keychain secret — those stay in the
/// application and are resolved at open time by the credential store.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSpec {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: ConnectionAuthMethod,
    pub env: ConnectionEnvironment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump_ref: Option<String>,
}

impl ServerConnection {
    /// Builds the agent-visible (non-secret) spec for this connection. Excludes
    /// `key_path` and any credential — the connection is referenced by `id` and
    /// the app resolves the secret at open time.
    pub fn spec(&self) -> ConnectionSpec {
        ConnectionSpec {
            id: self.id.clone(),
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            auth_method: self.auth_method,
            env: self.env,
            jump_ref: self.jump_ref.clone(),
        }
    }
}

impl ConnectionSpec {
    /// Human-readable description rendered into the agent's hidden context and
    /// shown in the conversation reference card (hover shows the full spec).
    pub fn describe(&self) -> String {
        let auth = match self.auth_method {
            ConnectionAuthMethod::Password => "password",
            ConnectionAuthMethod::PublicKey => "public-key",
        };
        let env = match self.env {
            ConnectionEnvironment::Test => "test",
            ConnectionEnvironment::Stage => "stage",
            ConnectionEnvironment::Prod => "prod",
        };
        let mut parts = format!(
            "{} ({}:{}) user={} auth={} env={}",
            self.name, self.host, self.port, self.username, auth, env,
        );
        if let Some(ref jump) = self.jump_ref {
            parts.push_str(&format!(" via-jump={jump}"));
        }
        parts
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConnectionStore {
    pub version: String,
    pub connections: Vec<ServerConnection>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentConnection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_ref: Option<String>,
    pub host: String,
    pub username: String,
    pub env: ConnectionEnvironment,
    pub last_connected_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentConnectionStore {
    pub version: String,
    pub recent: Vec<RecentConnection>,
}

/// Write model for create / update. `secret` carries the credential to persist to
/// the OS store (when non-empty); the config itself never records the secret.
/// `clear_secret` removes a previously saved credential during an update.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConnectionInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: ConnectionAuthMethod,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub jump_ref: Option<String>,
    pub env: ConnectionEnvironment,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub clear_secret: bool,
}

/// Read model for the connection list. It only carries a summary plus the display
/// / integrity signals the list needs (saved-credential presence, jump label,
/// jump-missing flag). The full entity is available via `get_server_connection`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConnectionSummary {
    #[serde(flatten)]
    pub connection: ServerConnection,
    pub credential_ref: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump_label: Option<String>,
    #[serde(default)]
    pub jump_missing: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentConnectionSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_ref: Option<String>,
    pub host: String,
    pub username: String,
    pub env: ConnectionEnvironment,
    pub last_connected_at: String,
    /// False when the referenced connection has been deleted — the entry is kept
    /// for display but cannot be one-click reconnected.
    pub reconnectable: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerConnectionCommandError {
    #[error("connection.not-found")]
    NotFound,
    #[error("connection.name-conflict")]
    NameConflict,
    #[error("connection.invalid-input")]
    InvalidInput { reason: String },
    #[error("connection.jump-cycle")]
    JumpCycle { reason: String },
    #[error("connection.jump-missing")]
    JumpMissing { jump_ref: String },
    #[error("connection.jump-referenced")]
    JumpReferenced { referents: Vec<String> },
    #[error("credential.store-failed")]
    CredentialStoreFailed { reason: String },
    #[error("credential.missing")]
    CredentialMissing,
}

impl ServerConnectionCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "connection.not-found",
            Self::NameConflict => "connection.name-conflict",
            Self::InvalidInput { .. } => "connection.invalid-input",
            Self::JumpCycle { .. } => "connection.jump-cycle",
            Self::JumpMissing { .. } => "connection.jump-missing",
            Self::JumpReferenced { .. } => "connection.jump-referenced",
            Self::CredentialStoreFailed { .. } => "credential.store-failed",
            Self::CredentialMissing => "credential.missing",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::NotFound | Self::NameConflict | Self::CredentialMissing => serde_json::json!({}),
            Self::InvalidInput { reason } => serde_json::json!({ "reason": reason }),
            Self::JumpCycle { reason } => serde_json::json!({ "reason": reason }),
            Self::JumpMissing { jump_ref } => serde_json::json!({ "jumpRef": jump_ref }),
            Self::JumpReferenced { referents } => serde_json::json!({ "referents": referents }),
            Self::CredentialStoreFailed { reason } => serde_json::json!({ "reason": reason }),
        }
    }
}

fn validate_connection_input(
    input: &ServerConnectionInput,
) -> Result<(), ServerConnectionCommandError> {
    if input.name.trim().is_empty() {
        return Err(ServerConnectionCommandError::InvalidInput {
            reason: "name required".into(),
        });
    }
    if input.host.trim().is_empty() {
        return Err(ServerConnectionCommandError::InvalidInput {
            reason: "host required".into(),
        });
    }
    if input.port == 0 {
        return Err(ServerConnectionCommandError::InvalidInput {
            reason: "port must be nonzero".into(),
        });
    }
    if input.username.trim().is_empty() {
        return Err(ServerConnectionCommandError::InvalidInput {
            reason: "username required".into(),
        });
    }
    if input.auth_method == ConnectionAuthMethod::PublicKey
        && input.key_path.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err(ServerConnectionCommandError::InvalidInput {
            reason: "key path required for public key auth".into(),
        });
    }
    Ok(())
}

/// Validates the `jumpRef` chain for a connection being created (`self_id`
/// `None`) or updated (`self_id` `Some`). Rejects self-reference, cycles, and
/// references to non-existent connections.
fn resolve_jump_chain(
    store: &ServerConnectionStore,
    self_id: Option<&str>,
    jump_ref: Option<&str>,
) -> Result<(), ServerConnectionCommandError> {
    let Some(start) = jump_ref else {
        return Ok(());
    };
    let start = start.trim();
    if start.is_empty() {
        return Ok(());
    }
    if self_id == Some(start) {
        return Err(ServerConnectionCommandError::JumpCycle {
            reason: "self-reference".into(),
        });
    }

    let mut visited = BTreeSet::new();
    let mut current = start;
    loop {
        let connection = store
            .connections
            .iter()
            .find(|connection| connection.id == current)
            .ok_or_else(|| ServerConnectionCommandError::JumpMissing {
                jump_ref: current.to_string(),
            })?;

        if !visited.insert(current.to_string()) {
            return Err(ServerConnectionCommandError::JumpCycle {
                reason: format!("cycle at `{current}`").into(),
            });
        }

        match connection
            .jump_ref
            .as_deref()
            .map(str::trim)
            .filter(|next| !next.is_empty())
        {
            Some(next) => {
                if self_id == Some(next) {
                    return Err(ServerConnectionCommandError::JumpCycle {
                        reason: "self-reference".into(),
                    });
                }
                current = next;
            }
            None => return Ok(()),
        }
    }
}

/// Guards a delete: a connection that other connections reference as a jump
/// gateway cannot be removed until those references are re-pointed.
fn ensure_jump_not_referenced(
    store: &ServerConnectionStore,
    id: &str,
) -> Result<(), ServerConnectionCommandError> {
    let referents: Vec<String> = store
        .connections
        .iter()
        .filter(|connection| connection.jump_ref.as_deref() == Some(id))
        .map(|connection| connection.name.clone())
        .collect();
    if referents.is_empty() {
        return Ok(());
    }
    Err(ServerConnectionCommandError::JumpReferenced { referents })
}

fn next_server_connection_id(store: &ServerConnectionStore) -> String {
    loop {
        let candidate = format!("{SERVER_CONNECTION_ID_PREFIX}{}", generate_uuid());
        if !store
            .connections
            .iter()
            .any(|connection| connection.id == candidate)
        {
            return candidate;
        }
    }
}

fn connection_credential_ref(credential_store: &dyn CredentialStore, id: &str) -> bool {
    match credential_store.has_secret(id) {
        Ok(present) => present,
        Err(error) => {
            tracing::warn!(connection_id = id, error = %error, "failed to resolve credential status");
            false
        }
    }
}

fn build_summary(
    connection: &ServerConnection,
    store: &ServerConnectionStore,
    credential_store: &dyn CredentialStore,
) -> ServerConnectionSummary {
    let (jump_label, jump_missing) = match connection.jump_ref.as_deref() {
        Some(id) => match store.connections.iter().find(|item| item.id == id) {
            Some(gateway) => (Some(gateway.name.clone()), false),
            None => (None, true),
        },
        None => (None, false),
    };
    ServerConnectionSummary {
        connection: connection.clone(),
        credential_ref: connection_credential_ref(credential_store, &connection.id),
        jump_label,
        jump_missing,
    }
}

fn build_summaries(
    store: &ServerConnectionStore,
    credential_store: &dyn CredentialStore,
) -> Vec<ServerConnectionSummary> {
    store
        .connections
        .iter()
        .map(|connection| build_summary(connection, store, credential_store))
        .collect()
}

fn normalized_name(value: &str) -> String {
    value.trim().to_string()
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn empty_server_connection_store() -> ServerConnectionStore {
    ServerConnectionStore {
        version: VERSION.to_string(),
        connections: Vec::new(),
    }
}

fn empty_recent_connection_store() -> RecentConnectionStore {
    RecentConnectionStore {
        version: VERSION.to_string(),
        recent: Vec::new(),
    }
}

impl App {
    /// Returns connection summaries (list view). Secrets are never returned.
    pub fn server_connections(&self) -> Result<Vec<ServerConnectionSummary>> {
        let store = self.load_server_connection_store()?;
        Ok(build_summaries(&store, self.credential_store.as_ref()))
    }

    /// Returns a single full connection by id.
    pub fn get_server_connection(&self, id: &str) -> Result<ServerConnection> {
        let store = self.load_server_connection_store()?;
        store
            .connections
            .iter()
            .find(|connection| connection.id == id)
            .cloned()
            .ok_or_else(|| ServerConnectionCommandError::NotFound.into())
    }

    /// Resolves a connection's auth material into an [`SshOpenRequest`] for opening
    /// a remote terminal. Secrets are pulled from the OS credential store (never the
    /// connections config). A stored jump reference is resolved recursively into a
    /// nested gateway request (阶段 D standard SSH gateway path); the store's
    /// reference-integrity rules already reject missing / cyclic jump chains.
    pub fn resolve_ssh_request(&self, id: &str) -> Result<SshOpenRequest> {
        self.resolve_ssh_request_inner(id, &mut Vec::new())
    }

    fn resolve_ssh_request_inner(
        &self,
        id: &str,
        chain: &mut Vec<String>,
    ) -> Result<SshOpenRequest> {
        if chain.iter().any(|visited| visited == id) {
            return Err(ServerConnectionCommandError::JumpCycle {
                reason: format!("connection `{id}` appears twice in the jump chain"),
            }
            .into());
        }
        // The gateway path implements a single hop (target + one bastion), per
        // the connection→terminal design scope. A stored jump-of-jump would
        // otherwise silently skip the outer hop, so reject it explicitly.
        if chain.len() >= 2 {
            return Err(ServerConnectionCommandError::InvalidInput {
                reason: "multi-level jump chains are not supported (target + one bastion)"
                    .to_string(),
            }
            .into());
        }
        chain.push(id.to_string());
        let connection = self.get_server_connection(id)?;
        let auth = match connection.auth_method {
            // No stored secret is a valid state: a dynamic/OTP password (e.g. a
            // rotating bastion policy) cannot be fixed at save time. Proceed with
            // an empty password rather than rejecting up front — the SSH layer's
            // password attempt will fail fast and fall back to keyboard-interactive,
            // which prompts the user live via `AuthPrompt` (never persisted).
            ConnectionAuthMethod::Password => {
                let password = self
                    .credential_store
                    .get_secret(&connection.id)?
                    .unwrap_or_default();
                SshAuth::Password { password }
            }
            ConnectionAuthMethod::PublicKey => {
                let key_path = connection.key_path.clone().unwrap_or_default();
                let passphrase = self.credential_store.get_secret(&connection.id)?;
                SshAuth::PublicKey {
                    key_path,
                    passphrase,
                }
            }
        };
        // Resolve the stored jump reference into a fully-resolved nested gateway
        // request (credentials included) so the SSH layer never needs connection
        // identity. The reference-integrity rules on write keep the chain valid;
        // the visited-set here is a defensive cycle guard for concurrent edits.
        let jump = match normalized_optional(connection.jump_ref.clone()) {
            Some(jump_id) => Some(Box::new(self.resolve_ssh_request_inner(&jump_id, chain)?)),
            None => None,
        };
        Ok(SshOpenRequest {
            connection_id: connection.id.clone(),
            connection_name: connection.name.clone(),
            host: connection.host.clone(),
            port: connection.port,
            username: connection.username.clone(),
            auth,
            jump,
            // 手动打开（终端面板 / 连接页）没有对话来源；连接 MCP 工具路径在
            // 打开前用 `with_origin` 打上 run 定位。
            origin: None,
        })
    }

    /// Binds a connection to a conversation run, persisting the run's non-secret
    /// `connection_specs` (the value the agent's hidden context and MCP server
    /// tool scope read from). Secrets never land here — only the spec. Idempotent
    /// by stable connection id: re-binding an already-bound connection refreshes
    /// its spec (a renamed/rehosted connection must not keep a stale spec).
    pub fn bind_run_connection(
        &self,
        task_id: &str,
        run_id: &str,
        connection_id: &str,
    ) -> Result<ConnectionSpec> {
        let spec = self.get_server_connection(connection_id)?.spec();
        let path = self.paths.run_file(task_id, run_id);
        let bound = spec.clone();
        crate::storage::with_file_lock(&path, || {
            let mut run: crate::runtime::RunState = read_json(&path)?;
            run.connection_specs
                .retain(|existing| existing.id != bound.id);
            run.connection_specs.push(bound.clone());
            write_json(&path, &run)
        })?;
        Ok(spec)
    }

    /// Removes a connection from a run's bound set. Returns whether it was
    /// bound (idempotent: unbinding an unbound connection is a no-op, not an
    /// error). After removal the agent's next turn omits it from both the hidden
    /// context and the connection MCP tool scope.
    pub fn unbind_run_connection(
        &self,
        task_id: &str,
        run_id: &str,
        connection_id: &str,
    ) -> Result<bool> {
        let path = self.paths.run_file(task_id, run_id);
        crate::storage::with_file_lock(&path, || {
            let mut run: crate::runtime::RunState = read_json(&path)?;
            let before = run.connection_specs.len();
            run.connection_specs
                .retain(|existing| existing.id != connection_id);
            let removed = run.connection_specs.len() != before;
            if removed {
                write_json(&path, &run)?;
            }
            Ok(removed)
        })
    }

    /// The non-secret connection specs bound to a run (conversation view model
    /// source for the "connection reference" card).
    pub fn run_connection_specs(&self, task_id: &str, run_id: &str) -> Result<Vec<ConnectionSpec>> {
        let run: crate::runtime::RunState = read_json(&self.paths.run_file(task_id, run_id))?;
        Ok(run.connection_specs)
    }

    pub fn create_server_connection(
        &self,
        input: ServerConnectionInput,
    ) -> Result<ServerConnection> {
        validate_connection_input(&input)?;
        let mut store = self.load_server_connection_store()?;
        let name = normalized_name(&input.name);
        if store
            .connections
            .iter()
            .any(|connection| connection.name == name)
        {
            return Err(ServerConnectionCommandError::NameConflict.into());
        }
        resolve_jump_chain(&store, None, input.jump_ref.as_deref())?;

        let id = next_server_connection_id(&store);
        let now = now_rfc3339_like();
        let connection = ServerConnection {
            id: id.clone(),
            name,
            host: normalized_name(&input.host),
            port: input.port,
            username: normalized_name(&input.username),
            auth_method: input.auth_method,
            key_path: normalized_optional(input.key_path),
            jump_ref: normalized_optional(input.jump_ref),
            env: input.env,
            created_at: now.clone(),
            updated_at: now,
        };

        if let Some(secret) = input.secret.as_deref().filter(|secret| !secret.is_empty()) {
            self.credential_store
                .store_secret(&id, secret)
                .map_err(
                    |error| ServerConnectionCommandError::CredentialStoreFailed {
                        reason: error.to_string(),
                    },
                )?;
        }

        store.connections.push(connection.clone());
        if let Err(error) = self.save_server_connection_store(&store) {
            // Roll back the credential so we don't leave an orphan secret for a
            // connection that was never persisted.
            let _ = self.credential_store.delete_secret(&id);
            return Err(error);
        }
        Ok(connection)
    }

    pub fn update_server_connection(
        &self,
        id: &str,
        input: ServerConnectionInput,
    ) -> Result<ServerConnection> {
        validate_connection_input(&input)?;
        let mut store = self.load_server_connection_store()?;
        let index = store
            .connections
            .iter()
            .position(|connection| connection.id == id)
            .ok_or_else(|| ServerConnectionCommandError::NotFound)?;
        let name = normalized_name(&input.name);
        if store
            .connections
            .iter()
            .any(|connection| connection.id != id && connection.name == name)
        {
            return Err(ServerConnectionCommandError::NameConflict.into());
        }
        resolve_jump_chain(&store, Some(id), input.jump_ref.as_deref())?;

        let mut connection = store.connections[index].clone();
        connection.name = name;
        connection.host = normalized_name(&input.host);
        connection.port = input.port;
        connection.username = normalized_name(&input.username);
        connection.auth_method = input.auth_method;
        connection.key_path = normalized_optional(input.key_path);
        connection.jump_ref = normalized_optional(input.jump_ref);
        connection.env = input.env;
        connection.updated_at = now_rfc3339_like();

        if input.clear_secret {
            self.credential_store.delete_secret(id)?;
        } else if let Some(secret) = input.secret.as_deref().filter(|secret| !secret.is_empty()) {
            self.credential_store
                .store_secret(id, secret)
                .map_err(
                    |error| ServerConnectionCommandError::CredentialStoreFailed {
                        reason: error.to_string(),
                    },
                )?;
        }

        store.connections[index] = connection.clone();
        self.save_server_connection_store(&store)?;
        Ok(connection)
    }

    pub fn delete_server_connection(&self, id: &str) -> Result<ServerConnectionStore> {
        if id.trim().is_empty() {
            return Err(ServerConnectionCommandError::NotFound.into());
        }
        let mut store = self.load_server_connection_store()?;
        ensure_jump_not_referenced(&store, id)?;
        let original_len = store.connections.len();
        store.connections.retain(|connection| connection.id != id);
        if store.connections.len() == original_len {
            return Err(ServerConnectionCommandError::NotFound.into());
        }
        // Deleting a connection removes its saved credential too.
        self.credential_store.delete_secret(id)?;
        self.save_server_connection_store(&store)?;
        Ok(store)
    }

    /// Returns recent-connection history, de-duplicated and time-descending.
    /// Entries whose referenced connection was deleted are retained for display
    /// but marked non-reconnectable.
    pub fn recent_connections(&self) -> Result<Vec<RecentConnectionSummary>> {
        let store = self.load_recent_connection_store()?;
        let connections = self.load_server_connection_store()?;
        Ok(store
            .recent
            .into_iter()
            .map(|recent| {
                let reconnectable = recent
                    .connection_ref
                    .as_deref()
                    .is_some_and(|id| connections.connections.iter().any(|item| item.id == id));
                RecentConnectionSummary {
                    connection_ref: recent.connection_ref,
                    host: recent.host,
                    username: recent.username,
                    env: recent.env,
                    last_connected_at: recent.last_connected_at,
                    reconnectable,
                }
            })
            .collect())
    }

    /// Records a connection attempt as the most recent history entry. Called by
    /// the session layer when a terminal session is opened. De-duplicated by
    /// `connection_ref` (when present) or by host+username+env.
    pub fn record_recent_connection(
        &self,
        connection_ref: Option<&str>,
        host: &str,
        username: &str,
        env: ConnectionEnvironment,
    ) -> Result<()> {
        let mut store = self.load_recent_connection_store()?;
        store.recent.retain(
            |recent| match (recent.connection_ref.as_deref(), connection_ref) {
                (Some(existing), Some(current)) => existing != current,
                _ => !(recent.host == host && recent.username == username && recent.env == env),
            },
        );
        store.recent.insert(
            0,
            RecentConnection {
                connection_ref: connection_ref.map(str::to_string),
                host: host.to_string(),
                username: username.to_string(),
                env,
                last_connected_at: now_rfc3339_like(),
            },
        );
        if store.recent.len() > RECENT_CONNECTION_LIMIT {
            store.recent.truncate(RECENT_CONNECTION_LIMIT);
        }
        self.save_recent_connection_store(&store)
    }

    fn load_server_connection_store(&self) -> Result<ServerConnectionStore> {
        let path = self.paths.server_connections_file();
        if path.exists() {
            return read_json(&path);
        }
        let store = empty_server_connection_store();
        self.save_server_connection_store(&store)?;
        Ok(store)
    }

    fn save_server_connection_store(&self, store: &ServerConnectionStore) -> Result<()> {
        write_json(&self.paths.server_connections_file(), store)
    }

    fn load_recent_connection_store(&self) -> Result<RecentConnectionStore> {
        let path = self.paths.recent_connections_file();
        if path.exists() {
            return read_json(&path);
        }
        let store = empty_recent_connection_store();
        self.save_recent_connection_store(&store)?;
        Ok(store)
    }

    fn save_recent_connection_store(&self, store: &RecentConnectionStore) -> Result<()> {
        write_json(&self.paths.recent_connections_file(), store)
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    use super::*;
    use crate::app::credentials::test_in_memory_store;
    use crate::config::RuntimeConfig;
    use crate::storage::StoragePathConfig;

    /// The home resolver reads its env var by name each call, and `home_env_var`
    /// is a `&'static str`, so each test needs its own distinct name — reusing one
    /// name across tests races (one test's `TempDir` can be dropped out from under
    /// another's still-running assertions once cargo runs this module's tests
    /// concurrently, which it does by default). Leak a unique name per call so the
    /// env var is truly test-local instead of shared process-wide state.
    fn unique_home_env_var() -> &'static str {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Box::leak(format!("GOLD_BAND_CONN_TEST_HOME_{id}").into_boxed_str())
    }

    fn connection_app() -> (App, TempDir) {
        let temp = TempDir::new().expect("create temp dir");
        let path_config = StoragePathConfig {
            app_key: "gold-band",
            config_dir_name: ".gold-band",
            home_env_var: unique_home_env_var(),
        };
        // SAFETY: this test's env var name is unique (see `unique_home_env_var`),
        // so setting it cannot race with any other test's home dir resolution.
        unsafe {
            std::env::set_var(path_config.home_env_var, temp.path().join("home"));
        }
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap();
        let app =
            App::with_config_and_path_config(repo_root, RuntimeConfig::default(), path_config);
        (
            App::with_credential_store(app, test_in_memory_store()),
            temp,
        )
    }

    fn input(name: &str, host: &str) -> ServerConnectionInput {
        ServerConnectionInput {
            name: name.to_string(),
            host: host.to_string(),
            port: 22,
            username: "ops".to_string(),
            auth_method: ConnectionAuthMethod::Password,
            key_path: None,
            jump_ref: None,
            env: ConnectionEnvironment::Test,
            secret: Some("s3cret".to_string()),
            clear_secret: false,
        }
    }

    /// Writes a minimal running run so the bind/unbind methods have something to
    /// mutate.
    fn write_binding_test_run(app: &App) {
        let run = crate::runtime::RunState {
            version: VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: crate::domain::RunStatus::Running,
            outcome: None,
            started_at: "2026-09-09T00:00:00Z".to_string(),
            updated_at: "2026-09-09T00:00:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
            connection_specs: Vec::new(),
        };
        crate::storage::write_json(&app.paths.run_file("task-001", "run-001"), &run).unwrap();
    }

    #[test]
    fn run_connection_binding_is_idempotent_and_reversible() {
        let (app, _temp) = connection_app();
        write_binding_test_run(&app);
        let connection = app
            .create_server_connection(input("wb-prod", "10.0.0.5"))
            .unwrap();

        // Bind persists the non-secret spec onto the run.
        let spec = app
            .bind_run_connection("task-001", "run-001", &connection.id)
            .unwrap();
        assert_eq!(spec.name, "wb-prod");
        assert_eq!(spec.host, "10.0.0.5");
        let bound = app.run_connection_specs("task-001", "run-001").unwrap();
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].id, connection.id);

        // Re-binding the same connection refreshes rather than duplicating.
        app.bind_run_connection("task-001", "run-001", &connection.id)
            .unwrap();
        assert_eq!(
            app.run_connection_specs("task-001", "run-001")
                .unwrap()
                .len(),
            1,
            "re-binding the same connection must not duplicate its spec"
        );

        // Unbind removes it; a second unbind is a no-op, not an error.
        assert!(
            app.unbind_run_connection("task-001", "run-001", &connection.id)
                .unwrap()
        );
        assert!(
            app.run_connection_specs("task-001", "run-001")
                .unwrap()
                .is_empty()
        );
        assert!(
            !app.unbind_run_connection("task-001", "run-001", &connection.id)
                .unwrap()
        );
    }

    #[test]
    fn bind_run_connection_rejects_an_unknown_connection() {
        let (app, _temp) = connection_app();
        write_binding_test_run(&app);
        let error = app
            .bind_run_connection("task-001", "run-001", "conn-missing")
            .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.not-found"
        );
    }

    #[test]
    fn resolve_ssh_request_rejects_a_multi_level_jump_chain() {
        // The gateway path implements target + one bastion; a stored
        // jump-of-jump must be rejected with a structured error instead of
        // silently skipping the outer hop when opening.
        let (app, _temp) = connection_app();
        let outer = app
            .create_server_connection(input("gateway-outer", "10.0.0.1"))
            .unwrap();
        let mut inner_input = input("gateway-inner", "10.0.0.2");
        inner_input.jump_ref = Some(outer.id.clone());
        let inner = app.create_server_connection(inner_input).unwrap();
        let mut target_input = input("target", "10.0.0.3");
        target_input.jump_ref = Some(inner.id.clone());
        let target = app.create_server_connection(target_input).unwrap();

        let error = app.resolve_ssh_request(&target.id).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.invalid-input"
        );
    }

    #[test]
    fn run_connection_specs_never_carry_the_private_key_path_or_secret() {
        let (app, _temp) = connection_app();
        write_binding_test_run(&app);
        let mut key_input = input("wb-key", "10.0.0.9");
        key_input.auth_method = ConnectionAuthMethod::PublicKey;
        key_input.key_path = Some("/home/ops/.ssh/id_ed25519".to_string());
        let connection = app.create_server_connection(key_input).unwrap();

        let spec = app
            .bind_run_connection("task-001", "run-001", &connection.id)
            .unwrap();
        let serialized = serde_json::to_string(&spec).unwrap();
        assert!(
            !serialized.contains("id_ed25519") && !serialized.contains("s3cret"),
            "the bound spec must never expose a key path or credential: {serialized}"
        );
    }

    #[test]
    fn server_connections_crud_credential_and_reference_integrity() {
        let (app, _temp) = connection_app();

        // Empty list on first load.
        assert!(app.server_connections().unwrap().is_empty());

        // Create two connections, one a jump gateway for the other.
        let gateway = app
            .create_server_connection(input("gateway", "10.0.0.1"))
            .unwrap();
        assert!(
            gateway.id.starts_with("conn-"),
            "stable connector id prefix"
        );

        let mut target_input = input("target", "10.0.0.2");
        target_input.secret = None;
        target_input.jump_ref = Some(gateway.id.clone());
        let target = app.create_server_connection(target_input).unwrap();
        assert_eq!(target.jump_ref.as_deref(), Some(gateway.id.as_str()));

        // Credential presence is reflected in the list summary; target has none.
        let list = app.server_connections().unwrap();
        let gateway_summary = list.iter().find(|s| s.connection.id == gateway.id).unwrap();
        assert!(gateway_summary.credential_ref, "saved credential reflected");
        let target_summary = list.iter().find(|s| s.connection.id == target.id).unwrap();
        assert!(!target_summary.credential_ref);
        assert_eq!(target_summary.jump_label.as_deref(), Some("gateway"));

        // Duplicate name rejected.
        let err = app
            .create_server_connection(input("gateway", "10.0.0.3"))
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.name-conflict"
        );

        // Referencing a non-existent jump gateway is rejected as jump-missing.
        let mut missing = input("missing-gw", "10.0.0.4");
        missing.jump_ref = Some("conn-does-not-exist".to_string());
        assert_eq!(
            app.create_server_connection(missing)
                .unwrap_err()
                .downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.jump-missing"
        );

        // Self-reference via update is rejected as a cycle.
        let self_conn = app
            .create_server_connection(input("still", "10.0.0.5"))
            .unwrap();
        let mut self_update = input("still", "10.0.0.5");
        self_update.jump_ref = Some(self_conn.id.clone());
        assert_eq!(
            app.update_server_connection(&self_conn.id, self_update)
                .unwrap_err()
                .downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.jump-cycle"
        );

        // Cycle: repoint the gateway at the target while the target points at the
        // gateway is rejected.
        let mut gateway_update = input("gateway", "10.0.0.1");
        gateway_update.jump_ref = Some(target.id.clone());
        assert_eq!(
            app.update_server_connection(&gateway.id, gateway_update)
                .unwrap_err()
                .downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.jump-cycle"
        );

        // Deleting the gateway is rejected while the target references it.
        let err = app.delete_server_connection(&gateway.id).unwrap_err();
        let err = err.downcast_ref::<ServerConnectionCommandError>().unwrap();
        assert_eq!(err.code(), "connection.jump-referenced");
        assert_eq!(err.params()["referents"][0], "target");

        // Re-point the target to direct, then delete the gateway: credential removed.
        app.update_server_connection(&target.id, input("target", "10.0.0.2"))
            .unwrap();
        app.delete_server_connection(&gateway.id).unwrap();
        let list = app.server_connections().unwrap();
        assert!(list.iter().all(|s| s.connection.id != gateway.id));
        assert!(
            !app.credential_store.has_secret(&gateway.id).unwrap(),
            "credential removed on delete"
        );

        // Recent history: a live ref is reconnectable, a deleted ref is kept but
        // marked non-reconnectable.
        app.record_recent_connection(
            Some(&target.id),
            "10.0.0.2",
            "ops",
            ConnectionEnvironment::Test,
        )
        .unwrap();
        assert!(app.recent_connections().unwrap()[0].reconnectable);
        app.delete_server_connection(&target.id).unwrap();
        let recent = app.recent_connections().unwrap();
        assert!(
            !recent
                .iter()
                .find(|r| r.connection_ref.is_some())
                .unwrap()
                .reconnectable
        );

        // Update clears the saved credential via clearSecret.
        let mut clear_input = input("still", "10.0.0.5");
        clear_input.secret = None;
        clear_input.clear_secret = true;
        app.update_server_connection(&self_conn.id, clear_input)
            .unwrap();
        assert!(!app.credential_store.has_secret(&self_conn.id).unwrap());

        // Detail view resolves by id and not-found is structured.
        assert_eq!(
            app.get_server_connection(&self_conn.id).unwrap().name,
            "still"
        );
        assert_eq!(
            app.get_server_connection("nope")
                .unwrap_err()
                .downcast_ref::<ServerConnectionCommandError>()
                .unwrap()
                .code(),
            "connection.not-found"
        );
    }

    /// A password-auth connection with no stored secret (a dynamic/OTP password
    /// that cannot be fixed at save time — e.g. a bastion policy that rotates it)
    /// must still resolve to an open attempt so the SSH layer's keyboard-interactive
    /// fallback can prompt live via `AuthPrompt`. Rejecting up front with
    /// `credential.missing` before ever reaching the SSH layer defeats that flow
    /// entirely: `resolve_ssh_request` is where this must be fixed, not by forcing
    /// a secret to exist.
    #[test]
    fn resolve_ssh_request_allows_password_auth_without_a_stored_secret() {
        let (app, _temp) = connection_app();
        let mut dynamic_input = input("dynamic-otp", "10.0.0.9");
        dynamic_input.secret = None;
        let connection = app.create_server_connection(dynamic_input).unwrap();

        let request = app.resolve_ssh_request(&connection.id).unwrap();
        match request.auth {
            SshAuth::Password { password } => assert!(password.is_empty()),
            SshAuth::PublicKey { .. } => panic!("expected password auth"),
        }
    }

    /// The agent-visible connection spec must NEVER expose the private-key path
    /// or any secret — only the non-secret identity a tool can drive by id.
    #[test]
    fn connection_spec_is_non_secret_and_describes_conn_target() {
        let mut input = ServerConnectionInput {
            name: "edge".to_string(),
            host: "10.0.0.9".to_string(),
            port: 22,
            username: "ops".to_string(),
            auth_method: ConnectionAuthMethod::PublicKey,
            key_path: Some("/home/ops/.ssh/id_pkey".to_string()),
            jump_ref: None,
            env: ConnectionEnvironment::Test,
            secret: None,
            clear_secret: false,
        };
        input.name = "edge".to_string();
        let conn = ServerConnection {
            id: "conn-abc".to_string(),
            name: input.name,
            host: input.host,
            port: input.port,
            username: input.username,
            auth_method: input.auth_method,
            key_path: input.key_path,
            jump_ref: input.jump_ref,
            env: input.env,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };

        let spec = conn.spec();
        // key_path must not appear on the spec (private-key path never to agent).
        let serialized = serde_json::to_string(&spec).unwrap();
        assert!(
            !serialized.contains("keyPath") && !serialized.contains("id_pkey"),
            "spec must not expose key_path"
        );
        assert_eq!(spec.host, "10.0.0.9");
        assert_eq!(spec.auth_method, ConnectionAuthMethod::PublicKey);
        assert!(spec.describe().contains("edge (10.0.0.9:22) user=ops"));
    }
}
