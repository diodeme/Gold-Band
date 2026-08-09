use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalog {
    pub schema_version: u32,
    pub source: AgentCatalogSource,
    pub agents: Vec<AgentCatalogEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogSource {
    pub url: String,
    pub registry_version: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogEntry {
    pub id: String,
    pub label: String,
    pub version: String,
    pub description: String,
    pub repository: Option<String>,
    pub website: Option<String>,
    pub icon_key: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    pub primary_agent_dir: Option<String>,
    #[serde(default)]
    pub project_primary_agent_dir: Option<String>,
    #[serde(default)]
    pub compatible_agent_dirs: Vec<String>,
    #[serde(default)]
    pub supports_system_prompt: bool,
    #[serde(default)]
    pub supports_external_session_sync: bool,
}

pub fn builtin_agent_catalog() -> &'static AgentCatalog {
    static CATALOG: OnceLock<AgentCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        parse_agent_catalog(include_str!("../resources/agent-catalog.json"))
            .expect("embedded agent-catalog.json is valid")
    })
}

pub fn builtin_agent(id: &str) -> Option<&'static AgentCatalogEntry> {
    builtin_agent_catalog()
        .agents
        .iter()
        .find(|entry| entry.id == id)
}

fn parse_agent_catalog(json: &str) -> Result<AgentCatalog> {
    let catalog: AgentCatalog = serde_json::from_str(json).context("invalid Agent catalog JSON")?;
    anyhow::ensure!(
        catalog.schema_version == 1,
        "unsupported Agent catalog schema"
    );
    anyhow::ensure!(!catalog.agents.is_empty(), "Agent catalog is empty");
    let mut ids = std::collections::BTreeSet::new();
    for entry in &catalog.agents {
        anyhow::ensure!(
            !entry.id.trim().is_empty(),
            "Agent catalog contains an empty id"
        );
        anyhow::ensure!(
            ids.insert(entry.id.as_str()),
            "duplicate Agent catalog id `{}`",
            entry.id
        );
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_contains_the_curated_agents() {
        let catalog = builtin_agent_catalog();
        assert_eq!(catalog.agents.len(), 11);
        assert!(builtin_agent("amp-acp").is_some());
        let pi = builtin_agent("pi-acp").unwrap();
        assert_eq!(pi.command, "npx");
        assert_eq!(pi.args, ["-y", "pi-acp@0.0.33"]);
        assert_eq!(pi.primary_agent_dir.as_deref(), Some(".pi/agent"));
        assert_eq!(pi.project_primary_agent_dir.as_deref(), Some(".pi"));
        assert_eq!(pi.compatible_agent_dirs, [".agents"]);
        assert!(builtin_agent("glm-acp-agent").is_none());
        let kimi = builtin_agent("kimi").unwrap();
        assert_eq!(kimi.primary_agent_dir.as_deref(), Some(".kimi-code"));
        assert_eq!(kimi.compatible_agent_dirs, [".agents"]);
    }
}
