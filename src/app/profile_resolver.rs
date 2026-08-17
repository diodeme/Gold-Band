use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::config::{DesktopLanguage, ProfileSource, ResolvedProfileRef};
use crate::dsl::{NodeDsl, PromptEnvelopeMode, WorkflowDsl};
use crate::storage::GoldBandPaths;

use super::profiles::find_profile_by_id;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorkflowMetadata {
    pub profiles: Vec<ResolvedProfileRef>,
}

pub(crate) fn resolve_workflow_profiles(
    paths: &GoldBandPaths,
    workflow: &WorkflowDsl,
    language: DesktopLanguage,
) -> Result<ResolvedWorkflowMetadata> {
    let mut profiles = Vec::new();
    for node in &workflow.nodes {
        match node {
            NodeDsl::Worker(worker) if worker.prompt_envelope == PromptEnvelopeMode::RawAgent => {
                if worker
                    .profile
                    .as_deref()
                    .is_some_and(|profile| !profile.trim().is_empty())
                {
                    bail!(
                        "raw-agent node `{}` must not be associated with role",
                        node.id()
                    );
                }
            }
            NodeDsl::Worker(worker) => push_profile(
                paths,
                &mut profiles,
                node.id(),
                worker.profile.as_deref(),
                language,
            )?,
            NodeDsl::AiDynamic(_) => {}
        }
    }
    Ok(ResolvedWorkflowMetadata { profiles })
}

#[cfg(test)]
mod tests {
    use super::resolve_workflow_profiles;
    use crate::config::DesktopLanguage;
    use crate::dsl::{
        END_NODE, EdgeDsl, EdgeOutcome, NodeDsl, PromptEnvelopeMode, WorkerNode, WorkflowDsl,
    };
    use crate::storage::GoldBandPaths;
    use camino::Utf8PathBuf;

    fn worker(prompt_envelope: PromptEnvelopeMode, profile: Option<&str>) -> WorkflowDsl {
        WorkflowDsl {
            version: "0.1".to_string(),
            id: "profile-resolution".to_string(),
            entry: "worker".to_string(),
            control: Default::default(),
            nodes: vec![NodeDsl::Worker(WorkerNode {
                id: "worker".to_string(),
                execution_slot_id: None,
                provider: Some("claude-acp".to_string()),
                model: None,
                profile: profile.map(str::to_string),
                goal: None,
                output: None,
                success_condition: None,
                permission_mode: None,
                config_options: Default::default(),
                manual_check: None,
                prompt_envelope,
            })],
            edges: vec![EdgeDsl {
                from: "worker".to_string(),
                to: END_NODE.to_string(),
                on: EdgeOutcome::Success,
                session: None,
                new_round_entry: None,
            }],
        }
    }

    #[test]
    fn raw_agent_worker_does_not_require_role_resolution() {
        let paths = GoldBandPaths::new(Utf8PathBuf::from("profile-resolver-raw-agent"));
        let resolved = resolve_workflow_profiles(
            &paths,
            &worker(PromptEnvelopeMode::RawAgent, None),
            DesktopLanguage::ZhCn,
        )
        .unwrap();

        assert!(resolved.profiles.is_empty());
    }

    #[test]
    fn raw_agent_worker_rejects_role_association() {
        let paths = GoldBandPaths::new(Utf8PathBuf::from("profile-resolver-raw-agent-role"));
        let error = resolve_workflow_profiles(
            &paths,
            &worker(PromptEnvelopeMode::RawAgent, Some("pf-should-not-load")),
            DesktopLanguage::ZhCn,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must not be associated with role")
        );
    }
}

fn push_profile(
    paths: &GoldBandPaths,
    profiles: &mut Vec<ResolvedProfileRef>,
    node_id: &str,
    profile: Option<&str>,
    language: DesktopLanguage,
) -> Result<()> {
    let Some(profile) = profile else {
        bail!("node `{node_id}` is not associated with role");
    };
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        bail!("node `{node_id}` is not associated with role");
    }
    let resolved = resolve_profile(paths, node_id, trimmed, language)?;
    if profiles.iter().all(|existing: &ResolvedProfileRef| {
        existing.name != resolved.name || existing.path != resolved.path
    }) {
        profiles.push(resolved);
    }
    Ok(())
}

pub(crate) fn resolve_profile(
    paths: &GoldBandPaths,
    node_id: &str,
    profile_id: &str,
    language: DesktopLanguage,
) -> Result<ResolvedProfileRef> {
    let Some(profile) = find_profile_by_id(paths, profile_id, language)? else {
        return Err(anyhow!(
            "node `{node_id}` associated role no longer exists; reset it"
        ));
    };
    Ok(ResolvedProfileRef {
        name: profile.id.clone(),
        display_name: profile.name,
        source: match profile.scope {
            super::profiles::ProfileScope::BuiltIn => ProfileSource::BuiltIn,
            super::profiles::ProfileScope::User => ProfileSource::User,
        },
        path: profile.path,
    })
}

pub(crate) fn resolve_profile_for_node(
    metadata: &ResolvedWorkflowMetadata,
    profile_name: &str,
) -> Option<ResolvedProfileRef> {
    metadata
        .profiles
        .iter()
        .find(|profile| profile.name == profile_name)
        .cloned()
}
