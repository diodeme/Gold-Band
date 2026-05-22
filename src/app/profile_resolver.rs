use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::config::{ProfileSource, ResolvedProfileRef};
use crate::dsl::{NodeDsl, WorkflowDsl};
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
) -> Result<ResolvedWorkflowMetadata> {
    let mut profiles = Vec::new();
    for node in &workflow.nodes {
        let profile = match node {
            NodeDsl::Worker(worker) => worker.profile.as_deref(),
        };
        let Some(profile) = profile else {
            bail!("node `{}` is not associated with role", node.id());
        };
        let trimmed = profile.trim();
        if trimmed.is_empty() {
            bail!("node `{}` is not associated with role", node.id());
        }
        let resolved = resolve_profile(paths, node.id(), trimmed)?;
        if profiles.iter().all(|existing: &ResolvedProfileRef| {
            existing.name != resolved.name || existing.path != resolved.path
        }) {
            profiles.push(resolved);
        }
    }
    Ok(ResolvedWorkflowMetadata { profiles })
}

pub(crate) fn resolve_profile(
    paths: &GoldBandPaths,
    node_id: &str,
    profile_id: &str,
) -> Result<ResolvedProfileRef> {
    let Some(profile) = find_profile_by_id(paths, profile_id)? else {
        return Err(anyhow!(
            "node `{node_id}` associated role visibility changed; reset it"
        ));
    };
    Ok(ResolvedProfileRef {
        name: profile.id,
        source: match profile.scope {
            super::profiles::ProfileScope::Project => ProfileSource::Project,
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
