use serde::Deserialize;
use std::collections::BTreeMap;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltinMcpServerDef {
    id: String,
    name: String,
    #[serde(default = "default_enabled")]
    _enabled: bool,
    transport: BuiltinMcpTransportDef,
    #[serde(default)]
    help_message: Option<String>,
}

fn default_enabled() -> bool { true }

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum BuiltinMcpTransportDef {
    #[serde(rename_all = "camelCase")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    #[serde(rename_all = "camelCase")]
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    #[serde(rename_all = "camelCase")]
    Sse {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

impl BuiltinMcpServerDef {
    fn to_mcp_json(&self) -> String {
        let id = self.id.as_str();
        let name = self.name.as_str();
        let mut inner = match &self.transport {
            BuiltinMcpTransportDef::Stdio { command, args, env } => {
                serde_json::json!({
                    "command": command,
                    "args": args,
                    "env": env,
                    "name": name,
                })
            }
            BuiltinMcpTransportDef::Http { url, headers } => {
                serde_json::json!({
                    "type": "http",
                    "url": url,
                    "headers": headers,
                    "name": name,
                })
            }
            BuiltinMcpTransportDef::Sse { url, headers } => {
                serde_json::json!({
                    "type": "sse",
                    "url": url,
                    "headers": headers,
                    "name": name,
                })
            }
        };
        if let Some(ref msg) = self.help_message {
            inner["helpMessage"] = serde_json::json!(msg);
        }
        let mut map = serde_json::Map::new();
        map.insert(id.to_owned(), inner);
        serde_json::to_string(&map).unwrap()
    }
}

pub fn inject_builtin_mcp_servers(state: &crate::state::DesktopState) {
    let channel_config = crate::channel::current_channel_config();
    let builtin_servers: Vec<BuiltinMcpServerDef> =
        serde_json::from_str(channel_config.builtin_mcp_servers_json).unwrap_or_default();

    if builtin_servers.is_empty() {
        return;
    }

    let Ok(ctx) = state.context() else { return };
    let paths = gold_band::storage::GoldBandPaths::new(ctx.repo_root);
    let mcp_mgr = gold_band::mcp::McpManager::new(paths.user_settings_file());

    let Ok(existing) = mcp_mgr.list() else { return };
    let existing_managed: std::collections::HashSet<&str> = existing
        .iter()
        .filter(|s| s.config.managed)
        .map(|s| s.config.id.as_str())
        .collect();

    for server in &builtin_servers {
        let sid = server.id.as_str();
        let json = server.to_mcp_json();
        if existing_managed.contains(sid) {
            // Managed server already exists — update its config (name/transport may have changed)
            match mcp_mgr.add_managed(&json) {
                Ok(_) => info!(server_id = %sid, "synced builtin MCP server config"),
                Err(e) => warn!(server_id = %sid, error = %e, "failed to sync builtin MCP server"),
            }
        } else {
            // Not yet injected — add as new managed server
            match mcp_mgr.add_managed(&json) {
                Ok(_) => info!(server_id = %sid, "injected builtin MCP server"),
                Err(e) => warn!(server_id = %sid, error = %e, "failed to inject builtin MCP server"),
            }
        }
    }
}
