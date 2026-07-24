use crate::domain::PauseReason;
use crate::observability::ProgressStage;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryMode {
    Auto,
    Manual,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeErrorDomain {
    RuntimeIo,
    RuntimeTransport,
    Provider,
    Config,
    Workspace,
    Workflow,
    Dynamic,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeErrorCode {
    pub domain: RuntimeErrorDomain,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: Vec<u64>,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: vec![1000, 3000, 10000],
            jitter: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeErrorInfo {
    pub code: RuntimeErrorCode,
    pub domain: RuntimeErrorDomain,
    pub recovery: RecoveryMode,
    pub retry_policy: Option<RetryPolicy>,
    pub params: serde_json::Value,
    pub diagnostic: String,
    pub raw: Option<serde_json::Value>,
}

impl RuntimeErrorInfo {
    pub fn new(
        domain: RuntimeErrorDomain,
        code: impl Into<String>,
        recovery: RecoveryMode,
        diagnostic: impl Into<String>,
        params: serde_json::Value,
        raw: Option<serde_json::Value>,
    ) -> Self {
        let retry_policy = (recovery == RecoveryMode::Auto).then(RetryPolicy::default);
        let code = RuntimeErrorCode {
            domain: domain.clone(),
            code: code.into(),
        };
        Self {
            code,
            domain,
            recovery,
            retry_policy,
            params,
            diagnostic: diagnostic.into(),
            raw,
        }
    }

    pub fn pause_reason_after_retry_boundary(&self) -> PauseReason {
        match self.recovery {
            RecoveryMode::Auto | RecoveryMode::Manual => PauseReason::RuntimeAbnormal,
            RecoveryMode::Blocked => PauseReason::ErrorBlocked,
        }
    }

    pub fn progress_stage_after_retry_boundary(&self) -> ProgressStage {
        match self.recovery {
            RecoveryMode::Auto | RecoveryMode::Manual => ProgressStage::Paused,
            RecoveryMode::Blocked => ProgressStage::Blocked,
        }
    }

    pub fn code_str(&self) -> &str {
        self.code.code.as_str()
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{diagnostic}")]
pub struct RuntimeError {
    pub info: RuntimeErrorInfo,
    diagnostic: String,
}

impl RuntimeError {
    pub fn new(info: RuntimeErrorInfo) -> Self {
        let diagnostic = info.diagnostic.clone();
        Self { info, diagnostic }
    }
}

pub fn runtime_error(info: RuntimeErrorInfo) -> anyhow::Error {
    anyhow!(RuntimeError::new(info))
}

pub fn blocked_runtime_error_info(
    domain: RuntimeErrorDomain,
    code: impl Into<String>,
    diagnostic: impl Into<String>,
    params: serde_json::Value,
) -> RuntimeErrorInfo {
    RuntimeErrorInfo::new(
        domain,
        code,
        RecoveryMode::Blocked,
        diagnostic,
        params,
        None,
    )
}

pub fn manual_runtime_error_info(
    domain: RuntimeErrorDomain,
    code: impl Into<String>,
    diagnostic: impl Into<String>,
    params: serde_json::Value,
) -> RuntimeErrorInfo {
    RuntimeErrorInfo::new(domain, code, RecoveryMode::Manual, diagnostic, params, None)
}

pub fn auto_runtime_error_info(
    domain: RuntimeErrorDomain,
    code: impl Into<String>,
    diagnostic: impl Into<String>,
    params: serde_json::Value,
) -> RuntimeErrorInfo {
    RuntimeErrorInfo::new(domain, code, RecoveryMode::Auto, diagnostic, params, None)
}

pub fn normalize_runtime_error(error: &anyhow::Error) -> RuntimeErrorInfo {
    let diagnostic = format!("{error:#}");
    if let Some(runtime_error) = error.downcast_ref::<RuntimeError>() {
        let mut info = runtime_error.info.clone();
        info.diagnostic = diagnostic.clone();
        return info;
    }
    if error.chain().any(|source| {
        source
            .downcast_ref::<std::io::Error>()
            .is_some_and(io_error_is_auto_recoverable)
    }) {
        return auto_runtime_error_info(
            RuntimeErrorDomain::RuntimeIo,
            "runtime.io.temporary-resource-unavailable",
            diagnostic.clone(),
            serde_json::json!({}),
        );
    }
    normalize_error_text(&diagnostic).unwrap_or_else(|| {
        blocked_runtime_error_info(
            RuntimeErrorDomain::Internal,
            "internal.unknown",
            diagnostic,
            serde_json::json!({}),
        )
    })
}

pub fn normalize_provider_failure(
    stop_reason: Option<&str>,
    diagnostic: impl Into<String>,
    raw: Option<serde_json::Value>,
) -> Option<RuntimeErrorInfo> {
    let diagnostic = diagnostic.into();
    let reason = stop_reason.unwrap_or_default();
    let text = if reason.is_empty() {
        diagnostic.clone()
    } else {
        format!("{reason}: {diagnostic}")
    };
    normalize_error_text(&text)
        .or_else(|| {
            reason.eq_ignore_ascii_case("error").then(|| {
                manual_runtime_error_info(
                    RuntimeErrorDomain::Provider,
                    "provider.execution-error",
                    diagnostic.clone(),
                    serde_json::json!({ "stopReason": reason }),
                )
            })
        })
        .map(|mut info| {
            info.raw = raw;
            info
        })
}

fn io_error_is_auto_recoverable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
    ) || error.raw_os_error() == Some(1450)
}

fn normalize_error_text(message: &str) -> Option<RuntimeErrorInfo> {
    let normalized = message
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(['`', '"', '\''], "");

    if contains_any(
        &normalized,
        &["auth-unavailable", "no auth available", "auth cooldown"],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.auth-unavailable",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "auth required",
            "authentication required",
            "not logged in",
            "missing api key",
            "no api key",
        ],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.auth-required",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "insufficient-quota",
            "quota exceeded",
            "balance",
            "credits",
            "credit",
        ],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.quota-insufficient",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "rate-limit",
            "rate limited",
            "429",
            "cooldown",
            "retry-after",
        ],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.rate-limited",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &["invalid model", "unsupported model", "model not found"],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.model-invalid",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "catalog missing",
            "model catalog missing",
            "doctor cache missing",
            "doctor diagnostic missing",
        ],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Config,
            "config.catalog-missing",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "provider missing",
            "missing resolved provider",
            "provider not configured",
            "agent not configured",
            "unsupported agent type",
            "is not supported yet",
        ],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Config,
            "config.provider-missing",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "permission mode",
            "config option invalid",
            "invalid params",
            "invalid configuration",
        ],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Config,
            "config.invalid",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &["non-git", "not a git", "no head", "worktree"],
    ) {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Workspace,
            "workspace.capability-missing",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "connection reset",
            "broken pipe",
            "timed out",
            "timeout",
            "connection aborted",
            "connection closed",
            "channel closed",
            "transport closed",
            "transport interrupted",
            "adapter transport interrupted",
            "stdout disconnected",
        ],
    ) {
        return Some(auto_runtime_error_info(
            RuntimeErrorDomain::RuntimeTransport,
            "runtime.transport-interrupted",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "os error 1450",
            "系统资源不足",
            "temporary local resource",
            "resource temporarily unavailable",
        ],
    ) {
        return Some(auto_runtime_error_info(
            RuntimeErrorDomain::RuntimeIo,
            "runtime.resource-unavailable",
            message,
            serde_json::json!({}),
        ));
    }
    if contains_any(
        &normalized,
        &[
            "502",
            "503",
            "504",
            "overloaded",
            "server-error",
            "server unavailable",
            "gateway unavailable",
            "usually temporary",
        ],
    ) {
        return Some(auto_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.server-unavailable",
            message,
            serde_json::json!({}),
        ));
    }
    if normalized.contains("acp ") || normalized.contains("acp-") {
        return Some(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.acp-error",
            message,
            serde_json::json!({}),
        ));
    }
    None
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn auth_unavailable_503_is_manual_not_auto() {
        let info = normalize_runtime_error(&anyhow!(
            "ACP `session/prompt` failed: 503 auth_unavailable: no auth available; usually temporary"
        ));
        assert_eq!(info.recovery, RecoveryMode::Manual);
        assert_eq!(info.code_str(), "provider.auth-unavailable");
        assert_eq!(
            info.pause_reason_after_retry_boundary(),
            PauseReason::RuntimeAbnormal
        );
    }

    #[test]
    fn server_503_without_auth_signal_is_auto() {
        let info = normalize_runtime_error(&anyhow!("503 server_error overloaded"));
        assert_eq!(info.recovery, RecoveryMode::Auto);
        assert_eq!(info.code_str(), "provider.server-unavailable");
        assert!(info.retry_policy.is_some());
    }

    #[test]
    fn quota_and_rate_limit_are_manual() {
        let quota = normalize_runtime_error(&anyhow!("insufficient_quota: balance too low"));
        assert_eq!(quota.recovery, RecoveryMode::Manual);
        assert_eq!(quota.code_str(), "provider.quota-insufficient");

        let rate = normalize_runtime_error(&anyhow!("429 rate_limit retry-after=60"));
        assert_eq!(rate.recovery, RecoveryMode::Manual);
        assert_eq!(rate.code_str(), "provider.rate-limited");
    }

    #[test]
    fn model_catalog_and_workspace_issues_are_manual() {
        let model = normalize_runtime_error(&anyhow!("unsupported model claude-x"));
        assert_eq!(model.code_str(), "provider.model-invalid");
        assert_eq!(model.recovery, RecoveryMode::Manual);

        let catalog = normalize_runtime_error(&anyhow!("model catalog missing"));
        assert_eq!(catalog.code_str(), "config.catalog-missing");
        assert_eq!(catalog.recovery, RecoveryMode::Manual);

        let workspace = normalize_runtime_error(&anyhow!("not a git workspace: no HEAD"));
        assert_eq!(workspace.code_str(), "workspace.capability-missing");
        assert_eq!(workspace.recovery, RecoveryMode::Manual);
    }

    #[test]
    fn transport_and_io_temporary_issues_are_auto() {
        let transport = normalize_runtime_error(&anyhow!("channel closed unexpectedly"));
        assert_eq!(transport.code_str(), "runtime.transport-interrupted");
        assert_eq!(transport.recovery, RecoveryMode::Auto);

        let io = normalize_runtime_error(
            &std::io::Error::new(std::io::ErrorKind::TimedOut, "slow disk").into(),
        );
        assert_eq!(io.code_str(), "runtime.io.temporary-resource-unavailable");
        assert_eq!(io.recovery, RecoveryMode::Auto);
    }

    #[test]
    fn unknown_internal_error_is_blocked() {
        let info = normalize_runtime_error(&anyhow!("dynamic graph invariant broken"));
        assert_eq!(info.recovery, RecoveryMode::Blocked);
        assert_eq!(
            info.pause_reason_after_retry_boundary(),
            PauseReason::ErrorBlocked
        );
    }

    #[test]
    fn provider_stop_reason_error_is_runtime_error_not_business_failure() {
        let info = normalize_provider_failure(
            Some("error"),
            "ACP provider reported error",
            Some(serde_json::json!({ "stopReason": "error" })),
        )
        .expect("provider error should normalize");
        assert_eq!(info.recovery, RecoveryMode::Manual);
        assert!(
            matches!(
                info.code_str(),
                "provider.execution-error" | "provider.acp-error"
            ),
            "unexpected code: {}",
            info.code_str()
        );

        assert!(normalize_provider_failure(Some("refusal"), "model refused", None).is_none());
    }

    #[test]
    fn structured_runtime_error_keeps_anyhow_context_chain() {
        let error = runtime_error(manual_runtime_error_info(
            RuntimeErrorDomain::Provider,
            "provider.acp-error",
            "session/set_config_option: failed to persist config.toml",
            serde_json::json!({}),
        ))
        .context("provider `codex-acp` failed to run `good-morning`");

        let info = normalize_runtime_error(&error);

        assert!(info.diagnostic.contains("provider `codex-acp`"));
        assert!(info.diagnostic.contains("session/set_config_option"));
    }
}
