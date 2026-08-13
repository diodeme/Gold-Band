//! 工作流预设（DSL 层复用构造，开发设计 2.5）。
//!
//! 把「provider → 单节点 WorkflowDsl」这份构造从私有调用点上提到库层公开，
//! 供会话 VM（`build_direct_workflow`）与 multica（`create_conversation_run_vm` 经由的发送链）
//! 共用同一份 provider→WorkflowDsl 构造，杜绝重复造轮子。
//!
//! 仅上提 `direct_workflow`：它是 provider 先天绑定的单 Worker 节点工作流，会话 VM 与
//! multica 都是消费者。`auto_workflow` 的核心是 `AiDynamicAgentStrategy` 构造（VM 配置翻译），
//! 仅会话 VM 单一消费者、无第二消费方——上提只是搬迁不构成复用，故保留在 VM。

use std::collections::BTreeMap;

use crate::dsl::{
    END_NODE, EdgeDsl, EdgeOutcome, NodeDsl, PromptEnvelopeMode, WorkerNode, WorkflowControl,
    WorkflowDsl,
};

/// Direct 模式工作流预设：单个 raw-agent Worker 节点 → `$end`。
///
/// - `provider`：先天绑定进 `NodeDsl::Worker.provider`（不可变，multica 远程任务即此模式）。
/// - `model` / `permission_mode` / `config_options`：透传 Worker 节点（会话 VM 带值，multica 传 None/空）。
/// - `manual_check = false` + `PromptEnvelopeMode::RawAgent`：首轮 prompt 即 requirement，不经运行时封装。
///
/// 与原 `view_models_conversation::build_direct_workflow` 完全等价（该函数已改为委托此处）。
pub fn direct_workflow(
    provider: String,
    model: Option<String>,
    permission_mode: Option<String>,
    config_options: BTreeMap<String, String>,
) -> WorkflowDsl {
    WorkflowDsl {
        version: "0.1".to_string(),
        id: "direct-agent".to_string(),
        entry: "direct-agent".to_string(),
        control: WorkflowControl::default(),
        nodes: vec![NodeDsl::Worker(WorkerNode {
            id: "direct-agent".to_string(),
            provider: Some(provider),
            model,
            profile: None,
            goal: None,
            output: None,
            success_condition: None,
            permission_mode,
            config_options,
            manual_check: Some(false),
            prompt_envelope: PromptEnvelopeMode::RawAgent,
        })],
        edges: vec![EdgeDsl {
            from: "direct-agent".to_string(),
            to: END_NODE.to_string(),
            on: EdgeOutcome::Success,
            session: None,
            new_round_entry: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::NodeDsl;

    fn worker_of(workflow: &WorkflowDsl) -> &WorkerNode {
        match workflow.nodes.first() {
            Some(NodeDsl::Worker(w)) => w,
            _ => panic!("expected a Worker node"),
        }
    }

    #[test]
    fn direct_workflow_binds_provider_and_raw_envelope() {
        let wf = direct_workflow(
            "claude-acp".into(),
            Some("claude-sonnet-5".into()),
            Some("default".into()),
            BTreeMap::from([("k".into(), "v".into())]),
        );
        assert_eq!(wf.version, "0.1");
        assert_eq!(wf.id, "direct-agent");
        assert_eq!(wf.entry, "direct-agent");
        let w = worker_of(&wf);
        assert_eq!(w.provider.as_deref(), Some("claude-acp"));
        assert_eq!(w.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(w.permission_mode.as_deref(), Some("default"));
        assert_eq!(w.config_options.get("k").map(String::as_str), Some("v"));
        assert_eq!(w.manual_check, Some(false));
        assert_eq!(w.prompt_envelope, PromptEnvelopeMode::RawAgent);
        // 单边：direct-agent → $end（Success）。
        assert_eq!(wf.edges.len(), 1);
        let edge = &wf.edges[0];
        assert_eq!(edge.from, "direct-agent");
        assert_eq!(edge.to, END_NODE);
        assert_eq!(edge.on, EdgeOutcome::Success);
    }

    #[test]
    fn direct_workflow_allows_minimal_multica_binding() {
        // multica 远程任务：仅 provider 绑定，model/permission/options 全空。
        let wf = direct_workflow("claude-acp".into(), None, None, BTreeMap::new());
        let w = worker_of(&wf);
        assert_eq!(w.provider.as_deref(), Some("claude-acp"));
        assert!(w.model.is_none());
        assert!(w.permission_mode.is_none());
        assert!(w.config_options.is_empty());
    }
}
