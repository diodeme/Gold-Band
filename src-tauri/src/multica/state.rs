//! multica 运行期内存状态（开发设计 2.2.6 / 2.5）。
//!
//! **不进 `DesktopState`**：由 loop_/bridge 共享 `Arc<Mutex<MulticaRuntimeState>>`
//! （作为独立 tauri managed state，非 DesktopState 的 Mutex 池字段）。
//! `runtime_ids` 为缓存（register 幂等取回，丢失下次启动重建），M2 仅内存持有；
//! 待持久化的 pending_issues / task_conversations 在 M4 进库层 StateConfig。

// M4-c 的 start_multica_conversation_run 才填充 active_runs；bridge 订阅器（M4-b）已消费。
// M5 完成后审查移除该 allow。
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 运行期内存状态容器。
#[derive(Default)]
pub struct MulticaRuntimeState {
    /// workspace_id → server 分配的 runtime_id（register 幂等取回，内存缓存）。
    pub runtime_ids: HashMap<String, String>,
    /// remote_task_id → 本地 task/run 映射（claim/start 后填充；bridge 归属用）。
    pub active_runs: HashMap<String, ActiveRemoteRun>,
    /// claim-at-click 后、start 前持有的 prepare lease（compose 期间循环续期，防 45s 回收）。
    pub prepare_leases: HashMap<String, PrepareLease>,
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

/// claim-at-click 后、start 前持有的 prepare lease（远程任务执行改 composer 复用）。
///
/// compose 期间心跳循环每 tick 调 `extend_prepare_lease(runtime_id, task_id)` 续期 45s 窗口，
/// 防 server 回收任务（与 multica daemon `startTaskPrepareLeaseExtender` 同构）。
///
/// 同时承载 claim 时捕获的任务身份（`issue_id` / `title`）—— start 命令无权再读 claim 响应，
/// 但 `register_active_run` 需要 issue_id（bridge 终态/重跑归属）与 title（thread_name，「最近完成」快照避免读盘），
/// 故 claim 时一并存入，start 时消费。start 消费后整个 lease 移除。
#[derive(Debug, Clone)]
pub struct PrepareLease {
    /// 该任务所属 runtime（续期请求按它寻址）。
    pub runtime_id: String,
    /// multica issue id（start 时进 active_run，bridge 终态/重跑归属用）。
    pub issue_id: Option<String>,
    /// thread_name（start 时进 active_run.title，Issue 3C「最近完成」快照避免读盘）。
    pub title: Option<String>,
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

    /// 清单个 workspace 的 runtime_id 缓存（心跳 404 runtime_not_found 时调用）。
    ///
    /// runtime 行已被服务端删除/失效时，旧 runtime_id 心跳永久 404；清掉后下个 tick
    /// `self_heal_registration` 会重注册取回新 runtime_id（自愈，开发设计 4.1）。
    pub fn clear_runtime_id(&mut self, workspace_id: &str) {
        self.runtime_ids.remove(workspace_id);
    }

    /// 所有已注册 `(workspace_id, runtime_id)` 对（常驻心跳遍历用--需 workspace_id 才能在
    /// 心跳 404 时按 workspace 清缓存触发自愈重注册）。
    pub fn runtime_id_pairs(&self) -> Vec<(String, String)> {
        self.runtime_ids
            .iter()
            .map(|(ws, rid)| (ws.clone(), rid.clone()))
            .collect()
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

    /// 登记 prepare lease（claim-at-click 后调用；心跳循环据此续期，防 compose 期间 45s 回收）。
    ///
    /// 同时捕获 `issue_id` / `title`（claim 响应里的任务身份），start 命令消费时进 `active_run`。
    pub fn register_prepare_lease(
        &mut self,
        remote_task_id: &str,
        runtime_id: String,
        issue_id: Option<String>,
        title: Option<String>,
    ) {
        self.prepare_leases.insert(
            remote_task_id.to_string(),
            PrepareLease {
                runtime_id,
                issue_id,
                title,
            },
        );
    }

    /// 移除 prepare lease（start 消费 / 放弃 compose 时调用），返回被移除项。
    pub fn drop_prepare_lease(&mut self, remote_task_id: &str) -> Option<PrepareLease> {
        self.prepare_leases.remove(remote_task_id)
    }

    /// 所有 prepare lease 快照 `(remote_task_id, runtime_id)`，心跳循环续期遍历用。
    pub fn prepare_leases_snapshot(&self) -> Vec<(String, String)> {
        self.prepare_leases
            .iter()
            .map(|(remote, lease)| (remote.clone(), lease.runtime_id.clone()))
            .collect()
    }

    /// 取单条 prepare lease 克隆（start 命令读取身份用，**不移除**）。
    ///
    /// 成功建 run 后才 [`drop_prepare_lease`]——失败路径（校验/HTTP）下 lease 留存，心跳继续续期，
    /// 任务保持已领取态可重试发送（而非 45s 后被回收回 pending）。
    pub fn prepare_lease(&self, remote_task_id: &str) -> Option<PrepareLease> {
        self.prepare_leases.get(remote_task_id).cloned()
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
        assert!(state.active_run("remote-1").is_some());
    }

    #[test]
    fn runtime_ids_returns_all_registered_workspaces() {
        // 常驻心跳源：所有已连接工作空间（与在飞任务解耦，无任务也在线）。
        let mut state = MulticaRuntimeState::default();
        state.set_runtime_id("ws-1", "rt-a");
        state.set_runtime_id("ws-2", "rt-b");
        let mut ids = state.runtime_ids();
        ids.sort();
        assert_eq!(ids, vec!["rt-a".to_string(), "rt-b".to_string()]);

        // 无 active_runs 也照常返回（连接后即持续在线）。
        assert!(state.active_runs.is_empty());
    }

    #[test]
    fn runtime_id_pairs_carries_workspace_for_self_heal() {
        // 心跳遍历需 workspace_id：runtime 行失效 404 时才能按 workspace 清缓存，下 tick 自愈重注册。
        let mut state = MulticaRuntimeState::default();
        state.set_runtime_id("ws-1", "rt-a");
        state.set_runtime_id("ws-2", "rt-b");
        let mut pairs = state.runtime_id_pairs();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("ws-1".into(), "rt-a".into()),
                ("ws-2".into(), "rt-b".into()),
            ]
        );
    }

    #[test]
    fn clear_runtime_id_singular_drops_one_keeps_rest() {
        // 心跳 404 runtime_not_found：仅清失效那个 workspace 的缓存，其余保留。
        let mut state = MulticaRuntimeState::default();
        state.set_runtime_id("ws-1", "rt-a");
        state.set_runtime_id("ws-2", "rt-b");

        state.clear_runtime_id("ws-1");

        assert!(state.runtime_id("ws-1").is_none(), "失效 workspace 应清掉");
        assert_eq!(state.runtime_id("ws-2"), Some("rt-b"), "其余 workspace 不受影响");
        // 再清不存在的 -> 无副作用。
        state.clear_runtime_id("ws-x");
        assert_eq!(state.runtime_id("ws-2"), Some("rt-b"));
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
        assert!(state.active_runs.is_empty());
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

    #[test]
    fn prepare_lease_register_snapshot_and_drop() {
        // claim-at-click 后登记 lease（带任务身份）→ 循环按 snapshot 续期 → start 消费时 drop。
        let mut state = MulticaRuntimeState::default();
        assert!(state.prepare_leases_snapshot().is_empty());

        state.register_prepare_lease(
            "remote-1",
            "rt-a".into(),
            Some("issue-1".into()),
            Some("Fix login".into()),
        );
        let snapshot = state.prepare_leases_snapshot();
        assert_eq!(snapshot.as_slice(), &[("remote-1".into(), "rt-a".into())]);

        // start 消费 → 移除并返回（携带 claim 时捕获的身份），续期随之停止。
        let dropped = state.drop_prepare_lease("remote-1").expect("已登记应命中");
        assert_eq!(dropped.runtime_id, "rt-a");
        assert_eq!(dropped.issue_id.as_deref(), Some("issue-1"));
        assert_eq!(dropped.title.as_deref(), Some("Fix login"));
        assert!(state.prepare_leases_snapshot().is_empty());
        // 放弃 compose 再 drop → None（45s 自然过期兜底）。
        assert!(state.drop_prepare_lease("remote-1").is_none());
    }

    #[test]
    fn prepare_lease_snapshot_carries_multiple_runtimes() {
        // 多 runtime 各持一条 lease → snapshot 逐条续期（不同 workspace 并发 compose）。
        let mut state = MulticaRuntimeState::default();
        state.register_prepare_lease("remote-a", "rt-1".into(), None, None);
        state.register_prepare_lease("remote-b", "rt-2".into(), None, None);
        let mut snapshot = state.prepare_leases_snapshot();
        snapshot.sort();
        assert_eq!(
            snapshot,
            vec![
                ("remote-a".into(), "rt-1".into()),
                ("remote-b".into(), "rt-2".into()),
            ]
        );
    }
}

