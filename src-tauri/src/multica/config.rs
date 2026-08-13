//! multica 配置层：VM 聚合 + pat/daemon_id getter + base_url 容错。
//!
//! 照搬 `metrics.rs` 的 channel-priority + normalize + 「永不回显明文 PAT」模式
//! （`metrics_settings` metrics.rs:130-167 / `normalize_metrics_base_url` metrics.rs:90-114）。

use gold_band::config::{
    MulticaAccountRef, MulticaWorkspaceRef, RuntimeConfig, SettingsConfig, StateConfig,
};
use serde::Serialize;
use url::Url;

use crate::channel::current_channel_config;

/// 前端 multica 设置 VM（开发设计 2.2.5）。
///
/// PAT 只暴露存在性（`pat_set`），**永不回显明文**。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MulticaSettingsVm {
    pub enabled: bool,
    pub toggle_locked: bool,
    pub multica_base_url: Option<String>,
    /// multica Web 前端地址（浏览器登录页），可能与 base_url 不同。
    pub multica_app_url: Option<String>,
    /// PAT 是否已设置（存在性，永不回显明文）。
    pub pat_set: bool,
    pub daemon_id_set: bool,
    pub workspaces: Vec<MulticaWorkspaceRef>,
    pub active_workspace_id: Option<String>,
    /// 添加 workspace 时的默认 provider 预选（claude-acp）。
    pub default_provider: String,
    /// 视为已连接：PAT 存在即乐观认为有效；实际失效时操作报 auth-failed 引导重连。
    pub connected: bool,
    /// 已连接 multica 账号身份（`/api/me`）；仅 UI 展示，让用户核对连了哪个账号。非凭证。
    pub connected_account: Option<MulticaAccountRef>,
}

/// 容错归一化 multica 根 URL（同 `normalize_metrics_base_url` 的 scheme/host 校验）。
///
/// multica 无固定 API 后缀需剥离，仅做 scheme(http/https)+host 校验 + 去尾斜杠/query/fragment。
pub fn normalize_multica_base_url(raw: &str) -> Option<String> {
    let value = raw.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return None;
    }
    let mut url = Url::parse(&value).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    let normalized = url.to_string();
    let normalized = normalized.trim_end_matches('/');
    if normalized.is_empty() {
        Some(value)
    } else {
        Some(normalized.to_string())
    }
}

/// multica API 根地址：用户配置优先，channel 编译期默认兜底（参考 `metrics_base_url`）。
pub fn multica_base_url(config: &RuntimeConfig) -> Option<String> {
    let channel = current_channel_config();
    config
        .desktop_multica_base_url
        .as_deref()
        .and_then(normalize_multica_base_url)
        .or_else(|| normalize_multica_base_url(channel.multica_base_url))
}

/// multica Web 前端地址（浏览器登录页），可能与 base_url 不同。
pub fn multica_app_url(config: &RuntimeConfig) -> Option<String> {
    let channel = current_channel_config();
    config
        .desktop_multica_app_url
        .as_deref()
        .and_then(normalize_multica_base_url)
        .or_else(|| normalize_multica_base_url(channel.multica_app_url))
}

/// PAT（用户配置；channel 无 PAT——PAT 登录后生成）。明文，调用方负责不回显。
pub fn get_pat(config: &RuntimeConfig) -> Option<String> {
    config
        .desktop_multica_pat
        .clone()
        .filter(|s| !s.trim().is_empty())
}

/// daemon_id（本机持久 UUID）。
pub fn get_daemon_id(config: &RuntimeConfig) -> Option<String> {
    config
        .desktop_multica_daemon_id
        .clone()
        .filter(|s| !s.is_empty())
}

/// 聚合 multica 设置 VM（照搬 `metrics_settings` 的 channel-priority + normalize）。
///
/// `enabled = config.desktop_multica_enabled || channel.multica_enabled`，且 base_url 必须有效。
pub fn multica_settings(config: &RuntimeConfig) -> MulticaSettingsVm {
    let channel = current_channel_config();
    let enabled_flag = config.desktop_multica_enabled || channel.multica_enabled;
    let base_url = multica_base_url(config);
    let app_url = multica_app_url(config);
    let pat_set = get_pat(config).is_some();
    MulticaSettingsVm {
        enabled: enabled_flag && base_url.is_some(),
        toggle_locked: channel.multica_toggle_locked,
        multica_base_url: base_url,
        multica_app_url: app_url,
        pat_set,
        daemon_id_set: get_daemon_id(config).is_some(),
        workspaces: config.desktop_multica_workspaces.clone(),
        active_workspace_id: config.desktop_multica_active_workspace_id.clone(),
        default_provider: config.desktop_multica_default_provider.clone(),
        connected: pat_set,
        connected_account: config.desktop_multica_account.clone(),
    }
}

/// 首次生成 daemon_id（UUID v4 simple）并写回 SettingsConfig。
///
/// 返回是否新生成（true → 调用方需 `save_settings` 落盘）。
/// 参考 `runtime/mod.rs:120-129` 的 uuid 惯例。
pub fn ensure_daemon_id(settings: &mut SettingsConfig) -> bool {
    if settings
        .desktop_multica_daemon_id
        .as_deref()
        .map_or(false, |s| !s.is_empty())
    {
        return false;
    }
    settings.desktop_multica_daemon_id = Some(uuid::Uuid::new_v4().simple().to_string());
    true
}

/// 清 workspace 绑定（Settings 侧账号作用域）：`workspaces` + `active_workspace_id`。
///
/// 仅清绑定，**保留** PAT / 账号身份 / daemon_id。换号重连专用：旧账号 PAT 发现的 `workspace_id`
/// 对新账号无意义（且新账号可能根本不是其成员），换号即作废；但 PAT / 账号身份紧接着由新登录覆写，
/// daemon_id 是本机持久标识，均不在此清。运行期 register 缓存（`MulticaRuntimeState::runtime_ids`，
/// workspace→runtime_id）由命令层另行清。
pub fn clear_multica_workspace_bindings(settings: &mut SettingsConfig) {
    settings.desktop_multica_workspaces = None;
    settings.desktop_multica_active_workspace_id = None;
}

/// 清任务/会话本地索引（State 侧账号作用域）：`task_conversations` + `completed_tasks`。
///
/// 两者均以当前账号的 remote task id 为键，换号/断开后对新账号无意义且会跨账号泄漏。凭证变更时与
/// [`clear_multica_workspace_bindings`] 配套调用，构成「作废账号作用域状态」的完整覆盖（Settings 绑定 +
/// State 索引）。`multica_runtime_ids` 是死字段（仅声明、从不读写，真缓存在内存 `MulticaRuntimeState`），
/// 不在此处理。
pub fn clear_multica_state_indices(state: &mut StateConfig) {
    state.multica_task_conversations = None;
    state.multica_completed_tasks.clear();
}

/// 判定重连是否发生账号切换：以 email 为稳定标识。
///
/// 旧账号身份与新登录 email 不同 → 换号 → 账号作用域状态（workspace 绑定 + 任务/会话索引）需作废。
/// 任一方 email 缺失（旧 server 未返回 / 首次连接 / 断开后重连无旧身份）→ 无法判定 → 视为「未切换」：
/// 同账号重连是主流派，保留绑定；若确属脏绑定，register 时被服务端 404 自愈。
pub fn multica_account_changed(
    existing: Option<&MulticaAccountRef>,
    new_email: Option<&str>,
) -> bool {
    let old_email = existing.and_then(|a| a.email.as_deref());
    matches!((old_email, new_email), (Some(old), Some(new)) if old != new)
}

/// 清除 multica 登录态（与 [`connect_multica`](crate::commands::connect_multica) 对称的断开）。
///
/// 账号作用域的状态在此一并清空：PAT（`connected` 判定依据）、账号身份（`/api/me`）、
/// workspace 绑定与 active workspace——它们的 `workspace_id` 都由当前账号 PAT 发现、仅在登录态下有效，
/// 断开/换号后残留即脏数据，故与登录态同生共灭（杜绝「断开后设置页仍展示上个账号绑定的工作空间」，
/// 与左侧远程任务列表 `connected=false` 空态对齐）。**保留** daemon_id（本机持久标识，换账号/重连不变）；
/// 断开后回到干净入口，重连同账号需重新绑定 workspace。State 侧任务/会话索引（`task_conversations`/
/// `completed_tasks`）由命令层在断开时另行调 [`clear_multica_state_indices`] 清（同样账号作用域，换号/断开须一并作废）。
/// 运行期 register 缓存由命令层另行清 `MulticaRuntimeState`。
pub fn clear_multica_session(settings: &mut SettingsConfig) {
    settings.desktop_multica_pat = None;
    // 账号身份与登录态同生命周期：断开即清，杜绝展示「已连接」却已无凭证的错位状态。
    settings.desktop_multica_account = None;
    // workspace 绑定同样账号作用域（workspace_id 由当前 PAT 发现）：随登录态一并清空。
    clear_multica_workspace_bindings(settings);
}

#[cfg(test)]
mod tests {
    use super::{
        clear_multica_session, clear_multica_state_indices, clear_multica_workspace_bindings,
        ensure_daemon_id, multica_account_changed, multica_settings, normalize_multica_base_url,
    };
    use gold_band::config::{
        MulticaAccountRef, MulticaCompletedTask, MulticaTaskConversation, MulticaWorkspaceRef,
        RuntimeConfig, StateConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn normalizes_multica_base_url_strips_trailing_slash_and_query() {
        assert_eq!(
            normalize_multica_base_url(" http://maling.weoa.com/ ").as_deref(),
            Some("http://maling.weoa.com")
        );
        // query/fragment 去除，path 保留
        assert_eq!(
            normalize_multica_base_url("https://maling.weoa.com/app?x=1#frag").as_deref(),
            Some("https://maling.weoa.com/app")
        );
        // 非 http(s) / 无 host → None
        assert_eq!(normalize_multica_base_url("ftp://maling.weoa.com"), None);
        assert_eq!(normalize_multica_base_url("not-a-url"), None);
        assert_eq!(normalize_multica_base_url(""), None);
    }

    #[test]
    fn multica_settings_fresh_config_resolves_channel_default_and_hides_pat() {
        // default 频道预填了 multica URL + enabled（本地联调零配置，见 configs/channels/default.json）：
        // fresh RuntimeConfig 经频道回退即可解析 base_url 并 enabled，但仍未连接（无 PAT）、不回显明文 PAT。
        // 注：enabled/base_url 由编译期频道决定（default/wb 均预填），故此处断言频道回退结果。
        let config = RuntimeConfig::default();
        let vm = multica_settings(&config);
        assert!(
            vm.multica_base_url.is_some(),
            "频道默认应回退出 base_url（零配置直连前提）"
        );
        assert!(vm.enabled, "default 频道 enabled + URL → fresh config 即 enabled");
        assert!(!vm.pat_set);
        assert!(!vm.connected);
        assert_eq!(vm.default_provider, "claude-acp");
        // VM 结构体本身无明文 PAT 字段（编译期保证永不回显）。
    }

    #[test]
    fn ensure_daemon_id_generates_once_then_idempotent() {
        let mut settings = gold_band::config::SettingsConfig::default();
        assert!(ensure_daemon_id(&mut settings), "首次应生成 daemon_id");
        let first = settings.desktop_multica_daemon_id.clone().unwrap();
        assert!(!first.is_empty());
        assert!(
            !ensure_daemon_id(&mut settings),
            "已存在不应重复生成"
        );
        assert_eq!(
            settings.desktop_multica_daemon_id.as_deref(),
            Some(first.as_str())
        );
    }

    #[test]
    fn clear_multica_session_clears_account_scoped_state_but_keeps_daemon_id() {
        // 登录态：PAT + daemon_id + workspace 绑定 + active + 账号身份齐全（connect 后的典型态）。
        let mut settings = gold_band::config::SettingsConfig::default();
        settings.desktop_multica_pat = Some("secret-token".into());
        ensure_daemon_id(&mut settings);
        settings.desktop_multica_workspaces = Some(vec![multica_ref("ws-1", "claude-acp")]);
        settings.desktop_multica_active_workspace_id = Some("ws-1".into());
        settings.desktop_multica_account = Some(gold_band::config::MulticaAccountRef {
            name: Some("Demo".into()),
            email: Some("demo@maling.local".into()),
        });

        clear_multica_session(&mut settings);

        // 账号作用域状态一并清空：PAT（connected 判定转 false）+ 账号身份 + workspace 绑定 + active。
        // workspace_id 由当前账号 PAT 发现、仅登录态下有效，断开即脏数据，随登录态清空。
        assert!(settings.desktop_multica_pat.is_none());
        assert!(settings.desktop_multica_account.is_none());
        assert!(
            settings.desktop_multica_workspaces.is_none(),
            "workspace 绑定属账号作用域：断开即清"
        );
        assert!(
            settings.desktop_multica_active_workspace_id.is_none(),
            "active workspace 随绑定一并清空，避免悬空引用"
        );
        // daemon_id 是本机持久标识（换账号/重连不变）：保留。
        assert!(settings.desktop_multica_daemon_id.is_some());
    }

    fn populated_state() -> StateConfig {
        // 两个账号作用域索引均非空：模拟旧账号登录期产生的本地簿记。
        let mut convs = HashMap::new();
        convs.insert(
            "remote-1".to_string(),
            MulticaTaskConversation {
                local_task_id: "task-1".into(),
                local_run_id: "run-1".into(),
                session_id: Some("acp-1".into()),
                work_dir: Some("/repo/a".into()),
            },
        );
        StateConfig {
            multica_task_conversations: Some(convs),
            multica_completed_tasks: vec![MulticaCompletedTask {
                remote_task_id: "remote-1".into(),
                local_task_id: "task-1".into(),
                local_run_id: "run-1".into(),
                workspace_id: "ws-1".into(),
                local_project_id: "proj-1".into(),
                issue_id: Some("issue-1".into()),
                status: "completed".into(),
                title: "T1".into(),
                completed_at: "2026-08-11T00:00:00".into(),
            }],
            ..StateConfig::default()
        }
    }

    #[test]
    fn clear_multica_state_indices_empties_both_account_scoped_indices() {
        // 换号/断开：State 侧两索引（续跑索引 + 完成历史）均账号作用域，一并作废。
        let mut state = populated_state();
        assert!(state.multica_task_conversations.is_some());
        assert!(!state.multica_completed_tasks.is_empty());

        clear_multica_state_indices(&mut state);

        assert!(
            state.multica_task_conversations.is_none(),
            "续跑索引清空（旧 remote id 对新账号无意义）"
        );
        assert!(
            state.multica_completed_tasks.is_empty(),
            "完成历史清空（不再串号到新账号）"
        );
    }

    #[test]
    fn clear_multica_workspace_bindings_clears_bindings_keeps_credentials_and_daemon_id() {
        // 换号重连：仅清 workspace 绑定，PAT/账号身份/daemon_id 保留
        // （PAT/账号由新登录紧接着覆写，daemon_id 本机持久不变）。
        let mut settings = gold_band::config::SettingsConfig::default();
        settings.desktop_multica_pat = Some("secret-token".into());
        ensure_daemon_id(&mut settings);
        settings.desktop_multica_account = Some(MulticaAccountRef {
            name: Some("Demo".into()),
            email: Some("demo@maling.local".into()),
        });
        settings.desktop_multica_workspaces = Some(vec![multica_ref("ws-1", "claude-acp")]);
        settings.desktop_multica_active_workspace_id = Some("ws-1".into());

        clear_multica_workspace_bindings(&mut settings);

        assert!(
            settings.desktop_multica_workspaces.is_none(),
            "workspace 绑定属账号作用域：换号即清"
        );
        assert!(
            settings.desktop_multica_active_workspace_id.is_none(),
            "active 随绑定清空，避免悬空引用"
        );
        assert!(settings.desktop_multica_pat.is_some(), "换号路径 PAT 由新登录覆写，不在此清");
        assert!(settings.desktop_multica_account.is_some(), "账号身份同理保留，由新登录覆写");
        assert!(settings.desktop_multica_daemon_id.is_some(), "daemon_id 本机持久，不变");
    }

    #[test]
    fn multica_account_changed_judges_by_email_with_safe_default() {
        let acc = |email: Option<&str>| MulticaAccountRef {
            name: Some("N".into()),
            email: email.map(Into::into),
        };
        // 同 email → 未切换（同账号重连主流派，保留绑定）。
        assert!(!multica_account_changed(
            Some(&acc(Some("a@maling.local"))),
            Some("a@maling.local"),
        ));
        // 不同 email → 切换。
        assert!(multica_account_changed(
            Some(&acc(Some("a@maling.local"))),
            Some("b@maling.local"),
        ));
        // 任一 email 缺失 → 无法判定 → false（脏绑定由 register 404 自愈）。
        assert!(!multica_account_changed(Some(&acc(None)), Some("a@maling.local")));
        assert!(!multica_account_changed(
            Some(&acc(Some("a@maling.local"))),
            None,
        ));
        assert!(!multica_account_changed(None, Some("a@maling.local")));
        assert!(!multica_account_changed(None, None));
    }

    fn multica_ref(id: &str, provider: &str) -> MulticaWorkspaceRef {
        MulticaWorkspaceRef {
            id: id.into(),
            name: format!("ws {id}"),
            slug: id.into(),
            provider: provider.into(),
        }
    }

    #[test]
    fn multica_settings_exposes_bound_workspaces_and_active() {
        // add_multica_workspace 落绑后，VM 应回显 workspaces 列表 + activeWorkspaceId（M5-c 设置页契约）。
        let mut config = RuntimeConfig::default();
        config.desktop_multica_workspaces.push(multica_ref("ws-1", "claude-acp"));
        config.desktop_multica_workspaces.push(multica_ref("ws-2", "claude-acp"));
        config.desktop_multica_active_workspace_id = Some("ws-2".into());
        let vm = multica_settings(&config);
        assert_eq!(vm.workspaces.len(), 2);
        assert_eq!(vm.workspaces[0].id, "ws-1");
        assert_eq!(vm.workspaces[1].id, "ws-2");
        assert_eq!(vm.active_workspace_id.as_deref(), Some("ws-2"));
        // slug=id 兜底（add_multica_workspace 路径，server list_workspaces 不含 slug）。
        assert_eq!(vm.workspaces[0].slug, "ws-1");
    }
}
