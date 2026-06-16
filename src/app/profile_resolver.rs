use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::config::{DesktopLanguage, ProfileSource, ResolvedProfileRef};
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
    language: DesktopLanguage,
) -> Result<ResolvedWorkflowMetadata> {
    let mut profiles = Vec::new();
    for node in &workflow.nodes {
        match node {
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
            "node `{node_id}` associated role visibility changed; reset it"
        ));
    };
    Ok(ResolvedProfileRef {
        name: profile.id.clone(),
        display_name: profile.name,
        source: match profile.scope {
            super::profiles::ProfileScope::BuiltIn => ProfileSource::BuiltIn,
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
