use serde::Deserialize;
use std::collections::BTreeMap;
use tracing::{info, warn};

use gold_band::config::{McpServerConfig, McpTransportConfig};
use gold_band::mcp::ManagedMcpReconcile;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltinMcpServerDef {
    id: String,
    name: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    transport: BuiltinMcpTransportDef,
    #[serde(default)]
    help_message: Option<String>,
}

fn default_enabled() -> bool {
    true
}

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
    fn to_config(&self) -> McpServerConfig {
        let transport = match &self.transport {
            BuiltinMcpTransportDef::Stdio { command, args, env } => McpTransportConfig::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
            },
            BuiltinMcpTransportDef::Http { url, headers } => McpTransportConfig::Http {
                url: url.clone(),
                headers: headers.clone(),
                oauth: None,
            },
            BuiltinMcpTransportDef::Sse { url, headers } => McpTransportConfig::Sse {
                url: url.clone(),
                headers: headers.clone(),
            },
        };
        McpServerConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            enabled: self.enabled,
            transport,
            managed: true,
            help_message: self.help_message.clone(),
        }
    }
}

pub fn inject_builtin_mcp_servers(state: &crate::state::DesktopState) {
    let channel_config = crate::channel::current_channel_config();
    let channel_servers: Vec<BuiltinMcpServerDef> =
        serde_json::from_str(channel_config.builtin_mcp_servers_json).unwrap_or_default();

    let Ok(ctx) = state.context() else { return };
    let paths = gold_band::storage::GoldBandPaths::new(ctx.repo_root);
    let mcp_mgr = gold_band::mcp::McpManager::new(paths.user_settings_file());
    let mut builtin_servers = channel_servers
        .iter()
        .map(|server| (server.to_config(), server.enabled))
        .collect::<Vec<_>>();
    let executable = match std::env::current_exe() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => {
            warn!(%error, "failed to resolve executable for builtin memory MCP");
            return;
        }
    };
    builtin_servers.push((
        gold_band::memory::mcp::managed_server_config(executable),
        true,
    ));

    for (server, default_enabled) in builtin_servers {
        let sid = server.id.clone();
        match mcp_mgr.reconcile_managed_config(server, default_enabled) {
            Ok(ManagedMcpReconcile::Inserted) => {
                info!(server_id = %sid, "injected builtin MCP server")
            }
            Ok(ManagedMcpReconcile::Updated) => {
                info!(server_id = %sid, "updated builtin MCP server config")
            }
            Ok(ManagedMcpReconcile::Unchanged) => {}
            Err(error) => {
                warn!(server_id = %sid, %error, "failed to reconcile builtin MCP server")
            }
        }
    }
}
