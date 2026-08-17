use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::config::ProviderDiagnosticSnapshot;
use gold_band::provider::{
    AcpLiveUpdate, AcpPromptAccepted, AcpSessionUpdate, ProviderAdapter, ProviderInfo,
    ProviderRunResult, ProviderRuntimePhaseUpdate, SessionRef, WorkerInvocation,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn with_available_claude_diagnostics(app: App) -> App {
    app.with_provider_diagnostics_source(Arc::new(|| {
        Ok(BTreeMap::from([(
            "claude-acp".to_string(),
            ProviderDiagnosticSnapshot {
                available: true,
                reason: None,
                checked_at: "2026-08-17T00:00:00Z".to_string(),
                capabilities: Some(serde_json::json!({
                    "configOptions": [
                        {
                            "id": "model",
                            "category": "model",
                            "type": "select",
                            "options": [
                                { "value": "deepseek" },
                                { "value": "gpt-5.4" }
                            ]
                        },
                        {
                            "id": "mode",
                            "category": "mode",
                            "type": "select",
                            "options": [
                                { "value": "ask" },
                                { "value": "bypassPermissions" }
                            ]
                        }
                    ]
                })),
            },
        )]))
    }))
}

struct PromptAcceptingProvider {
    inner: Box<dyn ProviderAdapter>,
}

impl ProviderAdapter for PromptAcceptingProvider {
    fn describe_provider(&self) -> ProviderInfo {
        self.inner.describe_provider()
    }

    fn doctor(&self) -> gold_band::provider::DoctorResult {
        self.inner.doctor()
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        self.inner.run_worker(req)
    }

    fn run_worker_with_runtime_callbacks(
        &self,
        req: WorkerInvocation,
        live_update: Option<AcpLiveUpdate<'_>>,
        session_update: Option<AcpSessionUpdate<'_>>,
        prompt_accepted: Option<AcpPromptAccepted<'_>>,
        runtime_phase_update: Option<ProviderRuntimePhaseUpdate<'_>>,
    ) -> anyhow::Result<ProviderRunResult> {
        if let Some(callback) = prompt_accepted {
            callback(req.resume_prompt_id.as_deref().unwrap_or("test-prompt"))?;
        }
        self.inner.run_worker_with_runtime_callbacks(
            req,
            live_update,
            session_update,
            None,
            runtime_phase_update,
        )
    }

    fn open_session(&self, worker_ref: &SessionRef) -> anyhow::Result<()> {
        self.inner.open_session(worker_ref)
    }

    fn build_continue_command(&self, worker_ref: &SessionRef) -> anyhow::Result<Option<String>> {
        self.inner.build_continue_command(worker_ref)
    }
}

pub fn app_with_available_claude_provider(
    repo_root: Utf8PathBuf,
    provider: Box<dyn ProviderAdapter>,
) -> App {
    with_available_claude_diagnostics(App::with_provider(
        repo_root,
        Box::new(PromptAcceptingProvider { inner: provider }),
    ))
}
