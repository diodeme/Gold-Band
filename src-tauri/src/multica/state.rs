//! multica 运行期内存状态（开发设计 2.2.6 / 2.5）。
//!
//! **不进 `DesktopState`**：由 loop_/bridge 共享 `Arc<Mutex<MulticaRuntimeState>>`
//! （作为独立 tauri managed state，非 DesktopState 的 Mutex 池字段）。
//! `runtime_ids` 为缓存（register 幂等取回，丢失下次启动重建），M2 仅内存持有；
//! 待持久化的 pending_issues / task_conversations 在 M4 进库层 StateConfig。

// M4-c 的 start_multica_remote_task 才填充 active_runs；bridge 订阅器（M4-b）已消费。
// M5 完成后审查移除该 allow。
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 运行期内存状态容器。
#[derive(Default)]
pub struct MulticaRuntimeState {
    /// workspace_id → server 分配的 runtime_id（register 幂等取回，内存缓存）。
    pub runtime_ids: HashMap<String, String>,
    /// remote_task_id → 本地 task/run 映射（M4-c claim/start 后填充；bridge 归属用）。
    pub active_runs: HashMap<String, ActiveRemoteRun>,
}

/// 单个在飞 remote task 的本地映射。
///
/// `local_task_id`/`local_run_id` 为 display id（与 `RuntimeLifecycleEvent` 发出的 task_id/run_id
/// 同形，bridge 据此反向归属：本地 lifecycle 事件 → remote task）。
#[derive(Debug, Clone)]
pub struct ActiveRemoteRun {
    /// 该任务所属 runtime（心跳按它寻址）。
    pub runtime_id: String,
    /// 该任务所属 multica workspace（complete/fail 路径不需，但失败回显/重跑需）。
    pub workspace_id: String,
    /// 本地 task display id（事件归属键 = `RunCompleted.task_id`）。
    pub local_task_id: String,
    /// 本地 run display id（事件归属键 = `RunCompleted.run_id`，配 task_id 唯一定位）。
    pub local_run_id: String,
    pub issue_id: Option<String>,
    /// 行标签（claim 时的 thread_name，Issue 3C「最近完成」快照用，避免终态读盘）。
    pub title: Option<String>,
    pub started_at: String,
}

/// 共享句柄：loop 创建（managed），bridge（M4）取同一份。
pub type SharedMulticaState = Arc<Mutex<MulticaRuntimeState>>;

/// 构造共享运行期状态（main.rs setup 经 `.manage()` 注入）。
pub fn shared_state() -> SharedMulticaState {
    Arc::new(Mutex::new(MulticaRuntimeState::default()))
}

impl MulticaRuntimeState {
    /// 写入 workspace → runtime_id 映射（register 成功后调用）。
    pub fn set_runtime_id(&mut self, workspace_id: &str, runtime_id: &str) {
        self.runtime_ids
            .insert(workspace_id.to_string(), runtime_id.to_string());
    }

    /// 取某 workspace 的 runtime_id。
    pub fn runtime_id(&self, workspace_id: &str) -> Option<&str> {
        self.runtime_ids.get(workspace_id).map(String::as_str)
    }

    /// 所有已注册 runtime_id（recover-orphans 遍历用）。
    pub fn runtime_ids(&self) -> Vec<String> {
        self.runtime_ids.values().cloned().collect()
    }

    /// 清空 runtime_id 注册缓存（断开连接时调用）。
    ///
    /// 仅清 register 缓存——重连后 loop 全量/增量 register 会重建。**保留** `active_runs`
    /// （真实在飞本地 run 的 remote 映射；断开后 bridge 上报因无 PAT 失败但不影响本地 run，重连同账号仍有效）。
    pub fn clear_runtime_ids(&mut self) {
        self.runtime_ids.clear();
    }

    /// 当前有在飞任务的 runtime_id 集合（去重 + 排序，心跳遍历用）。
    pub fn active_runtime_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .active_runs
            .values()
            .map(|run| run.runtime_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// 登记 remote task 的本地映射（M4-c claim+start 后调用）。
    pub fn register_active_run(&mut self, remote_task_id: &str, run: ActiveRemoteRun) {
        self.active_runs
            .insert(remote_task_id.to_string(), run);
    }

    /// 移除 remote task 映射（终态/取消后调用），返回被移除项供副作用使用。
    pub fn drop_active_run(&mut self, remote_task_id: &str) -> Option<ActiveRemoteRun> {
        self.active_runs.remove(remote_task_id)
    }

    /// 按 (local_task_id, local_run_id) 反查在飞 remote task（bridge 事件归属键）。
    ///
    /// 返回 `(remote_task_id, run 克隆)`（锁内 clone 以便释放锁后再做 async HTTP）。
    pub fn find_active_run_by_local(
        &self,
        local_task_id: &str,
        local_run_id: &str,
    ) -> Option<(String, ActiveRemoteRun)> {
        self.active_runs
            .iter()
            .find(|(_, r)| r.local_task_id == local_task_id && r.local_run_id == local_run_id)
            .map(|(rid, r)| (rid.clone(), r.clone()))
    }

    /// 按 remote_task_id 取在飞映射（cancel 命令用，键即 remote_task_id）。
    pub fn active_run(&self, remote_task_id: &str) -> Option<ActiveRemoteRun> {
        self.active_runs.get(remote_task_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_run(runtime_id: &str, local: &str) -> ActiveRemoteRun {
        ActiveRemoteRun {
            runtime_id: runtime_id.into(),
            workspace_id: "ws-1".into(),
            local_task_id: format!("task-{local}"),
            local_run_id: format!("run-{local}"),
            issue_id: Some(format!("issue-{local}")),
            title: Some(format!("title-{local}")),
            started_at: "2026-08-05T00:00:00".into(),
        }
    }

    #[test]
    fn set_runtime_id_is_overwritable_and_queryable() {
        let mut state = MulticaRuntimeState::default();
        assert!(state.runtime_id("ws-1").is_none());
        state.set_runtime_id("ws-1", "rt-a");
        assert_eq!(state.runtime_id("ws-1"), Some("rt-a"));
        // 幂等重注册覆盖为同值（server 稳定分配）。
        state.set_runtime_id("ws-1", "rt-a");
        assert_eq!(state.runtime_ids().as_slice(), &["rt-a".to_string()]);
    }

    #[test]
    fn clear_runtime_ids_empties_cache_but_keeps_active_runs() {
        // 断开连接：清 register 缓存，但保留在飞本地 run 的 remote 映射。
        let mut state = MulticaRuntimeState::default();
        state.set_runtime_id("ws-1", "rt-a");
        state.set_runtime_id("ws-2", "rt-b");
        state.register_active_run("remote-1", sample_run("rt-a", "1"));

        state.clear_runtime_ids();

        assert!(state.runtime_ids().is_empty());
        assert!(state.runtime_id("ws-1").is_none());
        // active_runs 保留（断开不改在飞本地 run 的归属映射）。
        assert_eq!(state.active_runtime_ids(), vec!["rt-a".to_string()]);
    }

    #[test]
    fn active_runtime_ids_dedups_across_runs() {
        let mut state = MulticaRuntimeState::default();
        // 两个在飞任务同属一个 runtime → 心跳只发一次（去重）。
        state.register_active_run("remote-1", sample_run("rt-a", "1"));
        state.register_active_run("remote-2", sample_run("rt-a", "2"));
        state.register_active_run(
            "remote-3",
            ActiveRemoteRun {
                runtime_id: "rt-b".into(),
                workspace_id: "ws-2".into(),
                local_task_id: "task-3".into(),
                local_run_id: "run-3".into(),
                issue_id: None,
                title: None,
                started_at: "2026-08-05T00:00:02".into(),
            },
        );
        assert_eq!(
            state.active_runtime_ids(),
            vec!["rt-a".to_string(), "rt-b".to_string()]
        );
    }

    #[test]
    fn find_active_run_by_local_matches_and_misses() {
        let mut state = MulticaRuntimeState::default();
        state.register_active_run("remote-9", sample_run("rt-a", "9"));
        // 命中：local_task_id + local_run_id 双键匹配。
        let found = state.find_active_run_by_local("task-9", "run-9");
        assert_eq!(found.as_ref().map(|(r, _)| r.as_str()), Some("remote-9"));
        // 串台防护：仅 task_id 匹配但 run_id 不同 → 不命中（多 workspace/多 run 不串台）。
        assert!(state.find_active_run_by_local("task-9", "run-other").is_none());
        // 完全不命中。
        assert!(state.find_active_run_by_local("task-x", "run-x").is_none());
    }

    #[test]
    fn drop_active_run_returns_and_removes() {
        let mut state = MulticaRuntimeState::default();
        state.register_active_run("remote-9", sample_run("rt-a", "9"));
        let dropped = state.drop_active_run("remote-9");
        assert!(dropped.is_some());
        assert_eq!(dropped.unwrap().local_task_id, "task-9");
        // 已移除 → 再 drop 返回 None。
        assert!(state.drop_active_run("remote-9").is_none());
        assert!(state.active_runtime_ids().is_empty());
    }

    #[test]
    fn active_run_looks_up_by_remote_id() {
        // cancel 命令按 remote_task_id 直查（键即 remote id）。
        let mut state = MulticaRuntimeState::default();
        state.register_active_run("remote-9", sample_run("rt-a", "9"));
        let found = state.active_run("remote-9").expect("已登记应命中");
        assert_eq!(found.local_task_id, "task-9");
        assert_eq!(found.local_run_id, "run-9");
        assert_eq!(found.workspace_id, "ws-1");
        assert!(state.active_run("remote-x").is_none());
    }
}

