//! multica 错误类型与 `CommandErrorVm` 映射（开发设计 2.2.8 / 第 5 章码表）。
//!
//! 后端只返回 `{ code, params }`，**不含任何对客文案**；前端按 `code` 查 i18n 文案。
//! HTTP 错误按状态码映射（client.rs `map_status`）：401/403→auth-failed、
//! 404→task-not-found、409→claim-conflict、其余≥400→network-failed。

// 完整错误码表一次定义：注册/领取冲突/会话恢复等 variant 分 M2-M4 启用。
use serde_json::{Value, json};

/// multica 模块统一错误。
///
/// 命令层经 `command_error`（commands.rs）downcast 后映射为
/// `CommandErrorVm { code: "multica.<reason>", params }`。
#[derive(Debug, thiserror::Error)]
pub enum MulticaError {
    #[error("multica not configured")]
    NotConfigured,
    #[error("multica auth failed: {0}")]
    AuthFailed(String),
    #[error("multica network failed: {0}")]
    NetworkFailed(String),
    #[error("multica register failed: {0}")]
    RegisterFailed(String),
    #[error("multica claim conflict")]
    ClaimConflict,
    #[error("multica task not found")]
    TaskNotFound,
    #[error("multica runtime offline")]
    RuntimeOffline,
    // M4-d：保留在错误码表（multica.session-resume-failed），但断点续跑路径不 emit——任何 resume Err
    // 改走 silent fresh-fallback（更稳，无需 fragile 串匹配）。见开发设计 §2.2.8。
    #[allow(dead_code)]
    #[error("multica session resume failed, will rerun")]
    SessionResumeFailed,
}

impl MulticaError {
    /// 错误码（kebab-case，`multica.` 前缀），对齐第 5 章码表。
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured => "multica.not-configured",
            Self::AuthFailed(_) => "multica.auth-failed",
            Self::NetworkFailed(_) => "multica.network-failed",
            Self::RegisterFailed(_) => "multica.register-failed",
            Self::ClaimConflict => "multica.claim-conflict",
            Self::TaskNotFound => "multica.task-not-found",
            Self::RuntimeOffline => "multica.runtime-offline",
            Self::SessionResumeFailed => "multica.session-resume-failed",
        }
    }

    /// 错误参数（上下文，无对客文案）。
    pub fn params(&self) -> Value {
        match self {
            Self::AuthFailed(msg) | Self::NetworkFailed(msg) | Self::RegisterFailed(msg) => {
                json!({ "message": msg })
            }
            _ => json!({}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MulticaError;

    #[test]
    fn codes_use_multica_kebab_prefix() {
        assert_eq!(MulticaError::NotConfigured.code(), "multica.not-configured");
        assert_eq!(
            MulticaError::AuthFailed("401".into()).code(),
            "multica.auth-failed"
        );
        assert_eq!(
            MulticaError::NetworkFailed("timeout".into()).code(),
            "multica.network-failed"
        );
        assert_eq!(
            MulticaError::RegisterFailed("x".into()).code(),
            "multica.register-failed"
        );
        assert_eq!(MulticaError::ClaimConflict.code(), "multica.claim-conflict");
        assert_eq!(MulticaError::TaskNotFound.code(), "multica.task-not-found");
        assert_eq!(
            MulticaError::RuntimeOffline.code(),
            "multica.runtime-offline"
        );
        assert_eq!(
            MulticaError::SessionResumeFailed.code(),
            "multica.session-resume-failed"
        );
    }

    #[test]
    fn params_carry_message_context_without_user_copy() {
        assert_eq!(
            MulticaError::AuthFailed("jwt expired".into()).params(),
            serde_json::json!({ "message": "jwt expired" })
        );
        // 无上下文的变体返回空对象，绝不内嵌对客文案。
        assert_eq!(MulticaError::ClaimConflict.params(), serde_json::json!({}));
        assert_eq!(MulticaError::TaskNotFound.params(), serde_json::json!({}));
    }
}
