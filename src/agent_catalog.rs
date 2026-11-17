use std::sync::OnceLock;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::platform::{DesktopPlatform, platform_excluded};

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
    pub official_registry: AgentCatalogOfficialRegistrySource,
    pub local_supplement: AgentCatalogLocalSupplementSource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogOfficialRegistrySource {
    pub url: String,
    pub registry_version: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogLocalSupplementSource {
    pub path: String,
    pub sha256: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogEntry {
    pub id: String,
    pub label: String,
    pub version: String,
    pub provenance: AgentCatalogProvenance,
    pub description: String,
    pub repository: Option<String>,
    pub website: Option<String>,
    pub icon_key: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Already installed ACP executable; its arguments exclude the npm launcher/package.
    pub local_launch: Option<LocalAgentLaunch>,
    /// Local Agent CLI whose ACP adapter may be prepared by the controlled setup probe.
    pub installed_command: Option<String>,
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
    #[serde(default)]
    pub excluded_platforms: Vec<DesktopPlatform>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentCatalogSourceKind {
    OfficialRegistry,
    LocalSupplement,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentIntegrationOrigin {
    Official,
    CommunityAdapter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogProvenance {
    pub source_kind: AgentCatalogSourceKind,
    pub integration_origin: AgentIntegrationOrigin,
    pub validated_version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAgentLaunch {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
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

impl AgentCatalogEntry {
    pub fn supports_platform(&self, platform: DesktopPlatform) -> bool {
        !platform_excluded(&self.excluded_platforms, platform)
    }
}

fn parse_agent_catalog(json: &str) -> Result<AgentCatalog> {
    let catalog: AgentCatalog = serde_json::from_str(json).context("invalid Agent catalog JSON")?;
    anyhow::ensure!(
        catalog.schema_version == 2,
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
        validate_entry(entry)?;
    }
    Ok(catalog)
}

fn validate_entry(entry: &AgentCatalogEntry) -> Result<()> {
    ensure!(!entry.version.is_empty(), "Agent catalog version is empty");
    ensure!(
        semver::Version::parse(&entry.version).is_ok(),
        "Agent `{}` has an invalid version",
        entry.id
    );
    ensure!(
        semver::Version::parse(&entry.provenance.validated_version).is_ok(),
        "Agent `{}` has an invalid validated version",
        entry.id
    );
    ensure!(
        !entry.icon_key.is_empty()
            && !entry.icon_key.contains(['/', '\\'])
            && entry.icon_key != "."
            && entry.icon_key != "..",
        "Agent `{}` has an invalid icon key",
        entry.id
    );
    ensure!(
        !entry.command.is_empty(),
        "Agent `{}` has an empty launch command",
        entry.id
    );
    for directory in entry
        .primary_agent_dir
        .iter()
        .chain(entry.project_primary_agent_dir.iter())
        .chain(entry.compatible_agent_dirs.iter())
    {
        let path = std::path::Path::new(directory);
        ensure!(
            !directory.trim().is_empty()
                && path.is_relative()
                && !path.components().any(|component| {
                    matches!(component.as_os_str().to_str(), Some("." | ".."))
                }),
            "Agent `{}` has an invalid Skill directory",
            entry.id
        );
    }
    let mut excluded = std::collections::BTreeSet::new();
    for platform in &entry.excluded_platforms {
        ensure!(
            excluded.insert(*platform),
            "Agent `{}` has duplicate platform exclusion `{}/{}`",
            entry.id,
            platform.os.as_str(),
            platform.arch.as_str()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_contains_the_curated_agents() {
        let catalog = builtin_agent_catalog();
        assert_eq!(catalog.agents.len(), 14);
        assert!(builtin_agent("amp-acp").is_some());
        let pi = builtin_agent("pi-acp").unwrap();
        assert_eq!(pi.command, "npx");
        assert!(!pi.version.is_empty());
        assert_eq!(
            pi.args,
            ["-y".to_string(), format!("pi-acp@{}", pi.version)]
        );
        assert_eq!(pi.primary_agent_dir.as_deref(), Some(".pi/agent"));
        assert_eq!(pi.project_primary_agent_dir.as_deref(), Some(".pi"));
        assert_eq!(pi.compatible_agent_dirs, [".agents"]);
        assert!(builtin_agent("glm-acp-agent").is_none());
        let kimi = builtin_agent("kimi").unwrap();
        assert_eq!(kimi.primary_agent_dir.as_deref(), Some(".kimi-code"));
        assert_eq!(kimi.compatible_agent_dirs, [".agents"]);
        let deepseek = builtin_agent("deepseek-harness").unwrap();
        assert_eq!(deepseek.command, "dsh");
        assert_eq!(deepseek.args, ["--profile".to_string(), "acp".to_string()]);
        assert_eq!(
            deepseek.provenance.source_kind,
            AgentCatalogSourceKind::LocalSupplement
        );
        assert_eq!(
            deepseek.provenance.integration_origin,
            AgentIntegrationOrigin::Official
        );
        assert_eq!(deepseek.provenance.validated_version, "0.2.1-rc.2");
        assert_eq!(deepseek.primary_agent_dir.as_deref(), Some(".dsh"));
        assert_eq!(deepseek.compatible_agent_dirs, [".agents"]);
        let qoder = builtin_agent("qoder").unwrap();
        assert_eq!(
            qoder.args,
            [
                "-y".to_string(),
                "@qoder-ai/qodercli@1.1.86".to_string(),
                "--acp".to_string()
            ]
        );
        assert_eq!(qoder.provenance.validated_version, "1.1.86");
        let qoder_local_launch = qoder.local_launch.as_ref().unwrap();
        assert_eq!(qoder_local_launch.command, "qodercli");
        assert_eq!(qoder_local_launch.args, ["--acp".to_string()]);
        assert_eq!(qoder.installed_command.as_deref(), Some("qodercli"));
        assert_eq!(qoder.primary_agent_dir.as_deref(), Some(".qoder"));
        assert_eq!(qoder.compatible_agent_dirs, [".agents"]);
        assert_eq!(
            qoder.excluded_platforms,
            [crate::platform::DesktopPlatform::new(
                crate::platform::DesktopOperatingSystem::Windows,
                crate::platform::DesktopArchitecture::Aarch64,
            )]
        );
        let zcode = builtin_agent("zcode-acp").unwrap();
        assert_eq!(zcode.command, "zcode-acp-server");
        assert!(zcode.args.is_empty());
        assert_eq!(
            zcode.provenance.source_kind,
            AgentCatalogSourceKind::LocalSupplement
        );
        assert_eq!(
            zcode.provenance.integration_origin,
            AgentIntegrationOrigin::CommunityAdapter
        );
        assert_eq!(zcode.provenance.validated_version, "0.84.1");
        assert_eq!(zcode.primary_agent_dir.as_deref(), Some(".zcode"));
        assert_eq!(zcode.project_primary_agent_dir.as_deref(), Some(".agents"));
        assert_eq!(zcode.compatible_agent_dirs, [".agents"]);
        assert!(catalog.agents.iter().all(|entry| {
            let local_supplement = entry.id == "deepseek-harness" || entry.id == "zcode-acp";
            let expected_source_kind = if local_supplement {
                AgentCatalogSourceKind::LocalSupplement
            } else {
                AgentCatalogSourceKind::OfficialRegistry
            };
            let expected_origin = if entry.id == "zcode-acp" {
                AgentIntegrationOrigin::CommunityAdapter
            } else {
                AgentIntegrationOrigin::Official
            };
            entry.provenance.source_kind == expected_source_kind
                && entry.provenance.integration_origin == expected_origin
                && !entry.provenance.validated_version.is_empty()
                && (entry.excluded_platforms.is_empty() || entry.id == "qoder")
        }));
    }

    #[test]
    fn platform_exclusions_are_typed_and_reject_duplicates() {
        let entry = serde_json::from_str::<AgentCatalogEntry>(
            r#"{
              "id": "qoder",
              "label": "Qoder",
              "version": "0.2.14",
              "provenance": {
                "sourceKind": "official-registry",
                "integrationOrigin": "official",
                "validatedVersion": "0.2.14"
              },
              "description": "fixture",
              "repository": null,
              "website": null,
              "iconKey": "qoder",
              "command": "npx",
              "args": ["-y", "@qoder-ai/qodercli@0.2.14", "--acp"],
              "excludedPlatforms": [
                {"os": "windows", "arch": "aarch64"},
                {"os": "windows", "arch": "aarch64"}
              ]
            }"#,
        )
        .unwrap();
        let error = validate_entry(&entry).unwrap_err().to_string();
        assert!(error.contains("duplicate platform exclusion"), "{error}");
    }
}
