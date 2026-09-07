//! 干预通知桌面端桥接：OS 原生通知 + 去重 + emit + dismiss。
//!
//! 本模块只含弹窗自身逻辑，不触碰主干运行时。生命周期见
//! `.claude/design/system-notification-intervention-reimpl-plan.md`：
//! Windows 横幅采用系统短时展示策略，超时后自动收起但仍保留在通知中心；
//! 无 resolved 闭环。发送流程为 dedup → OS 通知 → emit，
//! 失败一律 `tracing::warn!`，不静默吞错（方案 §6.3/§12）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::sync::Once;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use gold_band::app::{AcpTurnBatchProgress, InterventionNotification, RuntimeLifecycleEvent};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;

use crate::state::DesktopState;

#[cfg(windows)]
use std::process::Stdio;

/// Toast「查看详情」点击后，后端 emit 导航事件，前端 deep link 到节点。
pub const INTERVENTION_NAVIGATE_EVENT: &str = "gold-band://intervention-navigate";

/// 干预通知 OS 文案中的应用名（本次硬编码「码灵」，方案 §11）。
const APP_DISPLAY_NAME: &str = "码灵";
/// Windows AUMID（系统注册标识，与展示名是两回事），取自 tauri.conf.json identifier。
#[cfg(windows)]
const WINDOWS_AUMID: &str = "local.gold-band.desktop";

/// Windows 原生 Toast 只提供 Short（约 7 秒）/Long（约 25 秒）两档。
#[cfg(windows)]
const WINDOWS_TOAST_SHORT_DURATION_SECONDS: u64 = 7;
#[cfg(windows)]
const WINDOWS_TOAST_LONG_DURATION_SECONDS: u64 = 25;

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsToastDisplayPolicy {
    AutoDismissShort,
    AutoDismissLong,
}

#[cfg(windows)]
const fn windows_toast_display_policy(target_seconds: u64) -> WindowsToastDisplayPolicy {
    let short_distance = target_seconds.abs_diff(WINDOWS_TOAST_SHORT_DURATION_SECONDS);
    let long_distance = target_seconds.abs_diff(WINDOWS_TOAST_LONG_DURATION_SECONDS);
    if short_distance < long_distance {
        WindowsToastDisplayPolicy::AutoDismissShort
    } else {
        WindowsToastDisplayPolicy::AutoDismissLong
    }
}

#[cfg(windows)]
const fn expected_windows_toast_auto_dismiss_seconds(policy: WindowsToastDisplayPolicy) -> u64 {
    match policy {
        WindowsToastDisplayPolicy::AutoDismissShort => WINDOWS_TOAST_SHORT_DURATION_SECONDS,
        WindowsToastDisplayPolicy::AutoDismissLong => WINDOWS_TOAST_LONG_DURATION_SECONDS,
    }
}

/// 结构化 action 的「查看详情」前缀。
pub const ACTION_VIEW: &str = "view:";
/// 结构化 action 的「忽略」前缀。
pub const ACTION_DISMISS: &str = "dismiss:";

/// Toast「查看详情」按钮携带的完整定位字段（含 dedupKey，便于清后端去重）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewActionPayload {
    pub project_id: String,
    pub task_id: String,
    pub task_uuid: Option<String>,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub dedup_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledViewActionPayload {
    pub kind: String,
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub round_id: Option<String>,
    pub attempt_id: Option<String>,
    pub dedup_key: String,
}

/// A native notification can navigate either to an ordinary conversation
/// attempt or to scheduler-owned history/detail. Keep the ownership explicit
/// while preserving the payload fields consumed by the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "targetType", rename_all = "camelCase")]
pub enum InterventionNavigationTarget {
    Conversation(ViewActionPayload),
    Scheduled(ScheduledViewActionPayload),
}

impl InterventionNavigationTarget {
    fn dedup_key(&self) -> &str {
        match self {
            Self::Conversation(payload) => &payload.dedup_key,
            Self::Scheduled(payload) => &payload.dedup_key,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledNativeNotificationInputVm {
    pub event_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: Option<String>,
    pub links: gold_band::scheduler::occurrence::OccurrenceLinks,
}

/// Toast「忽略」按钮只需清后端去重 key。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissActionPayload {
    pub dedup_key: String,
}

#[derive(Debug, Default)]
pub struct PendingInterventionNavigations {
    queue: Mutex<VecDeque<InterventionNavigationTarget>>,
}

impl PendingInterventionNavigations {
    fn push(&self, target: InterventionNavigationTarget) {
        if let Ok(mut queue) = self.queue.lock()
            && !queue
                .iter()
                .any(|pending| pending.dedup_key() == target.dedup_key())
        {
            queue.push_back(target);
        }
    }

    fn take_all(&self) -> Vec<InterventionNavigationTarget> {
        self.queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }
}

#[tauri::command]
pub fn take_pending_intervention_navigations(
    pending: tauri::State<'_, PendingInterventionNavigations>,
) -> Vec<InterventionNavigationTarget> {
    pending.take_all()
}

/// 编码 `view:` + base64(json(payload))，消除旧的 `view|a|b|c|d` 脆弱解析（方案 §9.1）。
pub fn encode_view_action(payload: &ViewActionPayload) -> String {
    let json = serde_json::to_string(payload).unwrap_or_default();
    format!("{ACTION_VIEW}{}", URL_SAFE_NO_PAD.encode(json.as_bytes()))
}

pub fn encode_scheduled_view_action(payload: &ScheduledViewActionPayload) -> String {
    let json = serde_json::to_string(payload).unwrap_or_default();
    format!("{ACTION_VIEW}{}", URL_SAFE_NO_PAD.encode(json.as_bytes()))
}

pub fn scheduled_notification_dedup_key(
    occurrence_id: Option<&str>,
    kind: &str,
    event_id: &str,
) -> String {
    format!("scheduled:{}:{kind}", occurrence_id.unwrap_or(event_id))
}

/// 编码 `dismiss:` + base64(json(payload))。
pub fn encode_dismiss_action(payload: &DismissActionPayload) -> String {
    let json = serde_json::to_string(payload).unwrap_or_default();
    format!(
        "{ACTION_DISMISS}{}",
        URL_SAFE_NO_PAD.encode(json.as_bytes())
    )
}

/// 解析结构化 action。返回 `(is_view, payload_json)`：`is_view=true` 表示查看详情，
/// `false` 表示忽略；无法识别时返回 `None`，安全降级不 panic（方案 §13.3）。
pub fn decode_action(raw: &str) -> Option<(bool, serde_json::Value)> {
    let (is_view, body) = if let Some(body) = raw.strip_prefix(ACTION_VIEW) {
        (true, body)
    } else if let Some(body) = raw.strip_prefix(ACTION_DISMISS) {
        (false, body)
    } else {
        return None;
    };
    let bytes = URL_SAFE_NO_PAD.decode(body).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some((is_view, value))
}

/// 处理 Toast 点击 action：查看详情 → 前置窗口 + emit 导航 + 清 dedup；忽略 → 清 dedup。
fn handle_toast_action(
    app_handle: &AppHandle,
    raw_action: Option<&str>,
    default_target: InterventionNavigationTarget,
) {
    let Some(state) = app_handle.try_state::<DesktopState>() else {
        warn!("DesktopState unavailable when handling toast action");
        return;
    };
    let dedup = state.notification_dedup();
    match raw_action {
        Some(action) => match decode_action(action) {
            Some((true, value)) => {
                if value.get("scheduledTaskId").is_some() {
                    let payload: ScheduledViewActionPayload = match serde_json::from_value(value) {
                        Ok(payload) => payload,
                        Err(error) => {
                            warn!(?error, "decode scheduled view action payload failed");
                            return;
                        }
                    };
                    route_intervention_navigation(
                        app_handle,
                        InterventionNavigationTarget::Scheduled(payload),
                    );
                    return;
                }
                let payload: ViewActionPayload = match serde_json::from_value(value) {
                    Ok(p) => p,
                    Err(error) => {
                        warn!(?error, "decode view action payload failed");
                        return;
                    }
                };
                route_intervention_navigation(
                    app_handle,
                    InterventionNavigationTarget::Conversation(payload),
                );
            }
            Some((false, value)) => {
                let payload: DismissActionPayload = match serde_json::from_value(value) {
                    Ok(p) => p,
                    Err(error) => {
                        warn!(?error, "decode dismiss action payload failed");
                        return;
                    }
                };
                dedup.clear_key(&payload.dedup_key);
            }
            None => warn!(action, "unrecognized toast action, ignored"),
        },
        None => route_intervention_navigation(app_handle, default_target),
    }
}

#[tauri::command]
pub fn send_scheduled_native_notification(
    app_handle: AppHandle,
    state: tauri::State<'_, DesktopState>,
    input: ScheduledNativeNotificationInputVm,
) -> crate::commands::CommandResult<()> {
    let dedup_key = scheduled_notification_dedup_key(
        input.occurrence_id.as_deref(),
        &input.kind,
        &input.event_id,
    );
    let dedup = state.notification_dedup();
    if !dedup.try_send(&dedup_key) {
        tracing::debug!(%dedup_key, "scheduled notification deduplicated");
        return Ok(());
    }
    let payload = ScheduledViewActionPayload {
        kind: input.kind,
        project_id: input.project_id,
        scheduled_task_id: input.scheduled_task_id,
        occurrence_id: input.occurrence_id,
        task_id: input.links.task_id,
        run_id: input.links.run_id,
        round_id: input.links.round_id,
        attempt_id: input.links.attempt_id,
        dedup_key: dedup_key.clone(),
    };
    send_scheduled_os_notification(
        &app_handle,
        10,
        &input.title,
        &input.body,
        &dedup_key,
        &payload,
    );
    Ok(())
}

fn clear_notification_dedup(app_handle: &AppHandle, dedup_key: &str) {
    if let Some(state) = app_handle.try_state::<DesktopState>() {
        state.notification_dedup().clear_key(dedup_key);
    }
}

fn route_intervention_navigation(app_handle: &AppHandle, target: InterventionNavigationTarget) {
    let Some(pending) = app_handle.try_state::<PendingInterventionNavigations>() else {
        warn!("pending intervention navigation state unavailable");
        return;
    };
    pending.push(target.clone());
    clear_notification_dedup(app_handle, target.dedup_key());
    if let Err(error) = crate::desktop_lifecycle::ensure_main_window(app_handle) {
        warn!(
            ?error,
            "failed to restore main window for intervention navigation"
        );
        return;
    }
    if let Err(error) = app_handle.emit(INTERVENTION_NAVIGATE_EVENT, ()) {
        warn!(?error, "emit intervention navigation availability failed");
    }
}

/// 发送一次干预通知：去重 → OS 通知 → emit。
///
/// 同一 dedup_key 在点掉前只发一次（去重器拦截重复信号）。OS 通知失败仅 warn，
/// 不影响 emit；emit 失败仅 warn，不影响流程（方案 §6.3）。
pub fn send_intervention_notification(
    app_handle: &AppHandle,
    dedup: &gold_band::app::NotificationDedup,
    auto_dismiss_target_secs: u64,
    notification: InterventionNotification,
) {
    if !dedup.try_send(&notification.dedup_key) {
        // 同节点同原因未点掉前不重复弹，记 debug 即可。
        tracing::debug!(
            dedup_key = %notification.dedup_key,
            "intervention notification deduplicated"
        );
        return;
    }
    send_os_notification(app_handle, auto_dismiss_target_secs, &notification);
}

/// 发送 OS 原生通知。Windows 走 Toast（含结构化 action），其余平台走 notify-rust。
fn send_os_notification(
    app_handle: &AppHandle,
    auto_dismiss_target_secs: u64,
    notification: &InterventionNotification,
) {
    #[cfg(windows)]
    {
        if let Err(error) = send_windows_toast(app_handle, auto_dismiss_target_secs, notification) {
            warn!(?error, dedup_key = %notification.dedup_key, "windows toast failed");
        }
        return;
    }
    #[cfg(not(windows))]
    {
        send_notify_rust(app_handle, auto_dismiss_target_secs, notification);
    }
}

fn send_scheduled_os_notification(
    app_handle: &AppHandle,
    auto_dismiss_target_secs: u64,
    title: &str,
    body: &str,
    dedup_key: &str,
    payload: &ScheduledViewActionPayload,
) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::{IconCrop, Toast};

        ensure_notification_registry();
        let view_action = encode_scheduled_view_action(payload);
        let dismiss_action = encode_dismiss_action(&DismissActionPayload {
            dedup_key: dedup_key.to_string(),
        });
        let handle = app_handle.clone();
        let default_target = InterventionNavigationTarget::Scheduled(payload.clone());
        let toast = Toast::new(WINDOWS_AUMID)
            .title(&format!("{} - {}", APP_DISPLAY_NAME, title))
            .text1(body)
            .add_button("查看详情", &view_action)
            .add_button("忽略", &dismiss_action)
            .on_activated(move |action: Option<String>| {
                handle_toast_action(&handle, action.as_deref(), default_target.clone());
                Ok(())
            });
        let mut toast = apply_windows_toast_display_policy(toast, auto_dismiss_target_secs);
        if let Some(icon_path) = resolve_app_icon_path(app_handle) {
            toast = toast.icon(&icon_path, IconCrop::Square, APP_DISPLAY_NAME);
        }
        if let Err(error) = toast.show() {
            warn!(?error, %dedup_key, "windows scheduled toast failed");
        }
        return;
    }
    #[cfg(not(windows))]
    {
        let _ = app_handle;
        let _ = payload;
        #[cfg(feature = "native-notification")]
        {
            use notify_rust::{Notification, Timeout};
            if let Err(error) = Notification::new()
                .appname(APP_DISPLAY_NAME)
                .summary(&format!("{} - {}", APP_DISPLAY_NAME, title))
                .body(body)
                .timeout(Timeout::from(std::time::Duration::from_secs(
                    auto_dismiss_target_secs,
                )))
                .show()
            {
                warn!(?error, %dedup_key, "scheduled notify-rust notification failed");
            }
        }
    }
}

#[cfg(windows)]
fn send_windows_toast(
    app_handle: &AppHandle,
    auto_dismiss_target_secs: u64,
    notification: &InterventionNotification,
) -> Result<(), tauri_winrt_notification::Error> {
    use tauri_winrt_notification::{IconCrop, Toast};

    ensure_notification_registry();

    let payload = ViewActionPayload {
        project_id: notification.project_id.clone(),
        task_id: notification.task_id.clone(),
        task_uuid: notification.task_uuid.clone(),
        run_id: notification.run_id.clone(),
        round_id: notification.round_id.clone(),
        node_id: notification.node_id.clone(),
        attempt_id: notification.attempt_id.clone(),
        outer_node_id: notification.outer_node_id.clone(),
        outer_attempt_id: notification.outer_attempt_id.clone(),
        dedup_key: notification.dedup_key.clone(),
    };
    let dismiss_payload = DismissActionPayload {
        dedup_key: notification.dedup_key.clone(),
    };
    let view_action = encode_view_action(&payload);
    let dismiss_action = encode_dismiss_action(&dismiss_payload);

    let handle = app_handle.clone();
    let default_target = InterventionNavigationTarget::Conversation(payload.clone());
    let toast = Toast::new(WINDOWS_AUMID)
        .title(&format!("{} - {}", APP_DISPLAY_NAME, notification.title))
        .text1(&notification.body)
        .add_button("查看详情", &view_action)
        .add_button("忽略", &dismiss_action)
        .on_activated(move |action: Option<String>| {
            handle_toast_action(&handle, action.as_deref(), default_target.clone());
            Ok(())
        });
    let mut toast = apply_windows_toast_display_policy(toast, auto_dismiss_target_secs);

    // 显式设置码灵 app 图标，避免落到默认/powershell 图标。
    if let Some(icon_path) = resolve_app_icon_path(app_handle) {
        toast = toast.icon(&icon_path, IconCrop::Square, APP_DISPLAY_NAME);
    }

    toast.show()
}

#[cfg(windows)]
fn apply_windows_toast_display_policy(
    toast: tauri_winrt_notification::Toast,
    target_seconds: u64,
) -> tauri_winrt_notification::Toast {
    use tauri_winrt_notification::{Duration, Scenario};

    match windows_toast_display_policy(target_seconds) {
        WindowsToastDisplayPolicy::AutoDismissShort => {
            toast.duration(Duration::Short).scenario(Scenario::Default)
        }
        WindowsToastDisplayPolicy::AutoDismissLong => {
            toast.duration(Duration::Long).scenario(Scenario::Default)
        }
    }
}

/// 码灵 app 图标（编译期嵌入 `src-tauri/icons/icon.png`）。
///
/// 不走运行时 `BaseDirectory::Resource` 解析：dev/prod 资源目录解析易落空，导致
/// `toast.icon()` 被跳过、回退默认图标。嵌入字节后运行时写入 app local data 目录，
/// 生成真实存在的本地文件，满足 `tauri-winrt-notification` 的 `file:///` 渲染前提。
#[cfg(windows)]
const APP_ICON_BYTES: &[u8] = include_bytes!("../icons/icon.png");

/// Toast 图标文件名（写入 app local data 目录）。
#[cfg(windows)]
const TOAST_ICON_FILE_NAME: &str = "maling-toast-icon.png";

/// 解析码灵 Toast 图标路径：首次运行时把嵌入字节写入 app local data 目录，后续直接返回。
///
/// 写入用 `Once` 守护，幂等：已存在则跳过，避免每次弹窗都写盘。失败仅 warn，
/// 返回 `None` 时 `toast.icon()` 被跳过、回退默认图标（不阻断 Toast 主体）。
#[cfg(windows)]
fn resolve_app_icon_path(app_handle: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|error| {
            warn!(?error, "resolve app_local_data_dir failed for toast icon");
            error
        })
        .ok()?;
    let path = dir.join(TOAST_ICON_FILE_NAME);
    ensure_icon_file(&path);
    Some(path)
}

/// 幂等写入图标文件：不存在时写入嵌入字节。进程内只做一次实际写盘。
#[cfg(windows)]
fn ensure_icon_file(path: &std::path::Path) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if path.exists() {
            return;
        }
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                warn!(?error, path = %path.display(), "create toast icon dir failed");
                return;
            }
        }
        if let Err(error) = std::fs::write(path, APP_ICON_BYTES) {
            warn!(?error, path = %path.display(), "write toast icon file failed");
        }
    });
}

/// AUMID 注册与 Start Menu 快捷方式校验/重建：进程内只执行一次（方案 §9.2）。
#[cfg(windows)]
fn ensure_notification_registry() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if let Err(error) = register_aumid_and_shortcut() {
            warn!(
                ?error,
                "ensure_notification_registry failed; toast may not appear"
            );
        }
    });
}

/// 注册 AUMID（HKCU 注册表）并确保 Start Menu 快捷方式指向当前 exe。
///
/// 0.7.2 版 `tauri-winrt-notification` 不自带注册助手，故用 `reg` + PowerShell
/// `WScript.Shell` COM 自行完成（方案 §9.2：PowerShell + WScript.Shell COM，同步等待）。
#[cfg(windows)]
fn register_aumid_and_shortcut() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let exe_path = exe.to_string_lossy().to_string();

    // 1. 注册 AUMID 到 HKCU\Software\Classes\AppUserModelId\<AUMID>。
    //    Toast 需要 AUMID 关联 DisplayIcon 等元数据，否则通知无法正常显示。
    register_aumid_in_registry(&exe_path)?;

    // 2. 校验/重建 Start Menu 快捷方式，使其 TargetPath 指向当前 exe。
    let start_menu = start_menu_programs_dir()?;
    let lnk = start_menu.join(format!("{}.lnk", APP_DISPLAY_NAME));
    if !lnk.exists() || shortcut_target_mismatch(&lnk, &exe_path) {
        create_or_rebuild_shortcut(&lnk, &exe_path)?;
    }
    Ok(())
}

#[cfg(windows)]
fn register_aumid_in_registry(exe_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let key = format!(r"HKCU\Software\Classes\AppUserModelId\{}", WINDOWS_AUMID);
    // 注册失败不致命（Toast 仍可能以默认行为显示），但记录 warn。
    let status = gold_band::process::background_command("reg")
        .args([
            "ADD",
            &key,
            "/v",
            "DisplayName",
            "/t",
            "REG_SZ",
            "/d",
            APP_DISPLAY_NAME,
            "/f",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(error) = status {
        warn!(?error, "reg add DisplayName failed");
    }
    let status = gold_band::process::background_command("reg")
        .args([
            "ADD", &key, "/v", "IconUri", "/t", "REG_SZ", "/d", exe_path, "/f",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(error) = status {
        warn!(?error, "reg add IconUri failed");
    }
    Ok(())
}

#[cfg(windows)]
fn start_menu_programs_dir() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::var("APPDATA")?;
    Ok(std::path::Path::new(&base)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs"))
}

#[cfg(windows)]
fn shortcut_target_mismatch(lnk: &std::path::Path, exe_path: &str) -> bool {
    let ps = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         Write-Output $ws.CreateShortcut('{}').TargetPath",
        lnk.to_string_lossy().replace('\'', "''"),
    );
    let output = match gold_band::process::background_command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(_) => return false, // 读取失败不阻断，留给重建路径处理。
    };
    let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
    !target.eq_ignore_ascii_case(exe_path)
}

#[cfg(windows)]
fn create_or_rebuild_shortcut(
    lnk: &std::path::Path,
    exe_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let ps = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $lnk = $ws.CreateShortcut('{}'); \
         $lnk.TargetPath = '{}'; \
         $lnk.IconLocation = '{},0'; \
         $lnk.Save()",
        lnk.to_string_lossy().replace('\'', "''"),
        exe_path.replace('\'', "''"),
        exe_path.replace('\'', "''"),
    );
    let status = gold_band::process::background_command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(format!("powershell shortcut creation failed with status {status}").into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_notify_rust(
    app_handle: &AppHandle,
    auto_dismiss_target_secs: u64,
    notification: &InterventionNotification,
) {
    #[cfg(feature = "native-notification")]
    {
        use notify_rust::{Notification, NotificationResponse, Timeout};
        let payload = ViewActionPayload {
            project_id: notification.project_id.clone(),
            task_id: notification.task_id.clone(),
            task_uuid: notification.task_uuid.clone(),
            run_id: notification.run_id.clone(),
            round_id: notification.round_id.clone(),
            node_id: notification.node_id.clone(),
            attempt_id: notification.attempt_id.clone(),
            outer_node_id: notification.outer_node_id.clone(),
            outer_attempt_id: notification.outer_attempt_id.clone(),
            dedup_key: notification.dedup_key.clone(),
        };
        let handle = match Notification::new()
            .appname(APP_DISPLAY_NAME)
            .summary(&format!("{} - {}", APP_DISPLAY_NAME, notification.title))
            .body(&notification.body)
            .action("view", "查看详情")
            .timeout(Timeout::from(std::time::Duration::from_secs(
                auto_dismiss_target_secs,
            )))
            .show()
        {
            Ok(handle) => handle,
            Err(error) => {
                warn!(?error, dedup_key = %notification.dedup_key, "notify-rust failed");
                clear_notification_dedup(app_handle, &notification.dedup_key);
                return;
            }
        };
        let handle_app = app_handle.clone();
        std::thread::spawn(move || {
            let response_app = handle_app.clone();
            let response_payload = payload.clone();
            if let Err(error) = handle.wait_for_response(move |response: &NotificationResponse| {
                match native_notification_response_disposition(response) {
                    NativeNotificationResponseDisposition::Navigate => {
                        route_intervention_navigation(
                            &response_app,
                            InterventionNavigationTarget::Conversation(response_payload),
                        )
                    }
                    NativeNotificationResponseDisposition::ClearDedup => {
                        clear_notification_dedup(&response_app, &response_payload.dedup_key)
                    }
                }
            }) {
                warn!(?error, "waiting for native notification response failed");
                clear_notification_dedup(&handle_app, &payload.dedup_key);
            }
        });
    }
    #[cfg(not(feature = "native-notification"))]
    {
        let _ = auto_dismiss_target_secs;
        let _ = notification;
        let _ = app_handle;
    }
}

#[cfg(all(feature = "native-notification", any(not(windows), test)))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeNotificationResponseDisposition {
    Navigate,
    ClearDedup,
}

#[cfg(all(feature = "native-notification", any(not(windows), test)))]
fn native_notification_response_disposition(
    response: &notify_rust::NotificationResponse,
) -> NativeNotificationResponseDisposition {
    match response {
        notify_rust::NotificationResponse::Default => {
            NativeNotificationResponseDisposition::Navigate
        }
        notify_rust::NotificationResponse::Action(action) if action == "view" => {
            NativeNotificationResponseDisposition::Navigate
        }
        notify_rust::NotificationResponse::Action(_)
        | notify_rust::NotificationResponse::Reply(_)
        | notify_rust::NotificationResponse::Closed(_) => {
            NativeNotificationResponseDisposition::ClearDedup
        }
    }
}

fn intervention_notification_for_event(
    event: &RuntimeLifecycleEvent,
) -> Option<InterventionNotification> {
    let scheduled_occurrence_id = match event {
        RuntimeLifecycleEvent::InterventionRequested {
            scheduled_occurrence_id,
            ..
        }
        | RuntimeLifecycleEvent::RunCompleted {
            scheduled_occurrence_id,
            ..
        }
        | RuntimeLifecycleEvent::AcpTurnFinished {
            scheduled_occurrence_id,
            ..
        } => scheduled_occurrence_id.as_deref(),
        _ => None,
    };
    if scheduled_occurrence_id.is_some() {
        return None;
    }

    match event {
        RuntimeLifecycleEvent::InterventionRequested {
            event_id,
            project_id,
            task_id,
            task_uuid,
            task_title,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id,
            outer_attempt_id,
            node_label,
            kind,
            ..
        } => Some(
            InterventionNotification::from_intervention_event(
                &event_id,
                &project_id,
                &task_id,
                task_title.as_deref(),
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
                &node_label,
                *kind,
            )
            .with_navigation_identity(
                task_uuid.as_deref(),
                outer_node_id.as_deref(),
                outer_attempt_id.as_deref(),
            ),
        ),
        RuntimeLifecycleEvent::RunCompleted {
            event_id,
            project_id,
            task_id,
            task_uuid,
            task_title,
            run_id,
            round_id,
            node_id,
            attempt_id,
            node_label,
            outcome,
            completion_agent_label,
            ..
        } => {
            if should_defer_direct_run_completion_notification(
                *outcome,
                completion_agent_label.as_deref(),
            ) {
                None
            } else {
                InterventionNotification::from_run_completion(
                    &event_id,
                    &project_id,
                    &task_id,
                    task_title.as_deref(),
                    &run_id,
                    &round_id,
                    &node_id,
                    &attempt_id,
                    &node_label,
                    *outcome,
                    completion_agent_label.as_deref(),
                )
                .map(|notification| {
                    notification.with_navigation_identity(task_uuid.as_deref(), None, None)
                })
            }
        }
        RuntimeLifecycleEvent::AcpTurnFinished {
            project_id,
            task_id,
            task_uuid,
            task_title,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id,
            outer_attempt_id,
            turn_id,
            agent_label,
            outcome,
            batch_progress,
            ..
        } => {
            if !should_send_acp_turn_notification(*outcome, *batch_progress) {
                None
            } else {
                InterventionNotification::agent_turn_finished(
                    &project_id,
                    &task_id,
                    task_title.as_deref(),
                    &run_id,
                    &round_id,
                    &node_id,
                    &attempt_id,
                    &turn_id,
                    &agent_label,
                    *outcome,
                    batch_progress.completed_reply_count,
                )
                .map(|notification| {
                    notification.with_navigation_identity(
                        task_uuid.as_deref(),
                        outer_node_id.as_deref(),
                        outer_attempt_id.as_deref(),
                    )
                })
            }
        }
        _ => None,
    }
}

pub fn create_intervention_notification_subscriber(
    app_handle: AppHandle,
    auto_dismiss_target_secs: u64,
) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
    Arc::new(move |event| {
        let Some(state) = app_handle.try_state::<DesktopState>() else {
            warn!("DesktopState unavailable; intervention notification dropped");
            return;
        };
        let notification = intervention_notification_for_event(&event);
        let Some(notification) = notification else {
            return;
        };
        let target = crate::state::NotificationAttentionTarget {
            project_id: &notification.project_id,
            task_id: &notification.task_id,
            run_id: &notification.run_id,
            round_id: &notification.round_id,
            node_id: &notification.node_id,
            attempt_id: &notification.attempt_id,
        };
        if !state.should_send_notification(&target, true) {
            tracing::debug!(
                dedup_key = %notification.dedup_key,
                "intervention notification suppressed while target is visible"
            );
            return;
        }
        let dedup = state.notification_dedup();
        send_intervention_notification(&app_handle, &dedup, auto_dismiss_target_secs, notification);
    })
}

fn should_send_acp_turn_notification(
    outcome: gold_band::app::AcpTurnOutcome,
    batch_progress: AcpTurnBatchProgress,
) -> bool {
    outcome != gold_band::app::AcpTurnOutcome::Completed || !batch_progress.continues
}

fn should_defer_direct_run_completion_notification(
    outcome: gold_band::domain::RunOutcome,
    completion_agent_label: Option<&str>,
) -> bool {
    outcome == gold_band::domain::RunOutcome::Success && completion_agent_label.is_some()
}

/// Tauri 命令占位已移除：应用内弹窗删除后，前端不再调用点掉命令。去重清理统一由
/// 后端 `handle_toast_action`（OS Toast「查看详情」/「忽略」点击时）完成。

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_view() -> ViewActionPayload {
        ViewActionPayload {
            project_id: "project-1".to_string(),
            task_id: "task-1".to_string(),
            task_uuid: Some("task-uuid-1".to_string()),
            run_id: "run-1".to_string(),
            round_id: "round-1".to_string(),
            node_id: "node-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            outer_node_id: Some("outer-node-1".to_string()),
            outer_attempt_id: Some("outer-attempt-1".to_string()),
            dedup_key: "project-1:run-1:round-1:node-1:attempt-1:waiting-for-user-input"
                .to_string(),
        }
    }

    // 13.3 action 编解码往返

    #[test]
    fn view_action_roundtrip() {
        let payload = sample_view();
        let encoded = encode_view_action(&payload);
        assert!(encoded.starts_with(ACTION_VIEW));
        let (is_view, value) = decode_action(&encoded).expect("decode view action");
        assert!(is_view);
        let decoded: ViewActionPayload = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.project_id, "project-1");
        assert_eq!(decoded.task_id, "task-1");
        assert_eq!(decoded.task_uuid.as_deref(), Some("task-uuid-1"));
        assert_eq!(decoded.dedup_key, payload.dedup_key);
        assert_eq!(decoded.node_id, "node-1");
        assert_eq!(decoded.attempt_id, "attempt-1");
        assert_eq!(decoded.outer_node_id.as_deref(), Some("outer-node-1"));
        assert_eq!(decoded.outer_attempt_id.as_deref(), Some("outer-attempt-1"));
    }

    #[test]
    fn view_action_from_an_older_notification_defaults_new_identity_fields() {
        let decoded: ViewActionPayload = serde_json::from_value(serde_json::json!({
            "projectId": "project-1",
            "taskId": "task-1",
            "runId": "run-1",
            "roundId": "round-1",
            "nodeId": "node-1",
            "attemptId": "attempt-1",
            "dedupKey": "old-notification"
        }))
        .expect("decode old notification payload");

        assert!(decoded.task_uuid.is_none());
        assert!(decoded.outer_node_id.is_none());
        assert!(decoded.outer_attempt_id.is_none());
    }

    #[test]
    fn dismiss_action_roundtrip() {
        let payload = DismissActionPayload {
            dedup_key: "project-1:run-1:round-1:node-1:attempt-1:permission-requested".to_string(),
        };
        let encoded = encode_dismiss_action(&payload);
        assert!(encoded.starts_with(ACTION_DISMISS));
        let (is_view, value) = decode_action(&encoded).expect("decode dismiss action");
        assert!(!is_view);
        let decoded: DismissActionPayload = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.dedup_key, payload.dedup_key);
    }

    #[test]
    fn decode_action_safe_on_garbage() {
        // 异常 action 安全降级，不 panic。
        assert!(decode_action("garbage").is_none());
        assert!(decode_action("view:!!!not-base64!!!").is_none());
        assert!(decode_action("view:").is_none());
        assert!(decode_action("dismiss:").is_none());
    }

    #[test]
    fn decode_distinguishes_view_and_dismiss() {
        let v = encode_view_action(&sample_view());
        let d = encode_dismiss_action(&DismissActionPayload {
            dedup_key: "k".to_string(),
        });
        assert!(decode_action(&v).unwrap().0);
        assert!(!decode_action(&d).unwrap().0);
    }

    #[test]
    fn scheduled_view_action_roundtrip_keeps_detail_and_run_links() {
        let payload = ScheduledViewActionPayload {
            kind: "failed".to_string(),
            project_id: "project-a".to_string(),
            scheduled_task_id: "scheduled-a".to_string(),
            occurrence_id: Some("occurrence-a".to_string()),
            task_id: Some("task-a".to_string()),
            run_id: Some("run-a".to_string()),
            round_id: Some("round-a".to_string()),
            attempt_id: Some("attempt-a".to_string()),
            dedup_key: scheduled_notification_dedup_key(Some("occurrence-a"), "failed", "event-a"),
        };

        let encoded = encode_scheduled_view_action(&payload);
        let (is_view, value) = decode_action(&encoded).unwrap();
        let decoded: ScheduledViewActionPayload = serde_json::from_value(value).unwrap();

        assert!(is_view);
        assert_eq!(decoded.project_id, "project-a");
        assert_eq!(decoded.scheduled_task_id, "scheduled-a");
        assert_eq!(decoded.run_id.as_deref(), Some("run-a"));
        assert_eq!(decoded.dedup_key, "scheduled:occurrence-a:failed");
    }

    fn run_completed_event(scheduled_occurrence_id: Option<&str>) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunCompleted {
            event_id: "run-completed-a".to_string(),
            occurred_at: "2026-08-09T08:00:00Z".to_string(),
            scheduled_occurrence_id: scheduled_occurrence_id.map(str::to_string),
            project_id: "project-a".to_string(),
            task_id: "task-a".to_string(),
            task_uuid: Some("task-uuid-a".to_string()),
            run_id: "run-a".to_string(),
            round_id: "round-a".to_string(),
            node_id: "node-a".to_string(),
            attempt_id: "attempt-a".to_string(),
            node_label: "Node A".to_string(),
            outcome: gold_band::domain::RunOutcome::Success,
            task_title: Some("Task A".to_string()),
            completion_agent_label: None,
        }
    }

    #[test]
    fn run_completion_notification_preserves_canonical_task_identity() {
        let notification = intervention_notification_for_event(&run_completed_event(None))
            .expect("run completion notification");
        let value = serde_json::to_value(notification).expect("serialize notification");

        assert_eq!(value["taskUuid"], "task-uuid-a");
    }

    #[test]
    fn intervention_notification_preserves_canonical_dynamic_attempt_identity() {
        let event = RuntimeLifecycleEvent::InterventionRequested {
            event_id: "permission-request-a".to_string(),
            occurred_at: "2026-09-04T10:00:00Z".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-a".to_string(),
            task_id: "task-a".to_string(),
            task_uuid: Some("task-uuid-a".to_string()),
            run_id: "run-a".to_string(),
            round_id: "round-a".to_string(),
            node_id: "worker-a".to_string(),
            attempt_id: "attempt-a".to_string(),
            outer_node_id: Some("ai-dynamic-a".to_string()),
            outer_attempt_id: Some("outer-attempt-a".to_string()),
            node_label: "Worker A".to_string(),
            kind: gold_band::app::RuntimeInterventionKind::PermissionRequested,
            task_title: Some("Task A".to_string()),
        };

        let notification =
            intervention_notification_for_event(&event).expect("permission notification");
        assert_eq!(notification.task_uuid.as_deref(), Some("task-uuid-a"));
        assert_eq!(notification.outer_node_id.as_deref(), Some("ai-dynamic-a"));
        assert_eq!(
            notification.outer_attempt_id.as_deref(),
            Some("outer-attempt-a")
        );
    }

    #[test]
    fn scheduled_lifecycle_completion_is_owned_by_scheduled_notification_policy() {
        let event = run_completed_event(Some("occurrence-a"));

        assert!(intervention_notification_for_event(&event).is_none());
    }

    #[test]
    fn ordinary_lifecycle_completion_still_creates_intervention_notification() {
        let event = run_completed_event(None);

        assert!(intervention_notification_for_event(&event).is_some());
    }

    #[test]
    fn pending_navigation_queue_is_ordered_and_deduplicated() {
        let pending = PendingInterventionNavigations::default();
        let first = InterventionNavigationTarget::Conversation(sample_view());
        let mut second = sample_view();
        second.dedup_key = "second".to_string();
        let second = InterventionNavigationTarget::Conversation(second);
        pending.push(first.clone());
        pending.push(first.clone());
        pending.push(second.clone());

        assert_eq!(pending.take_all(), vec![first, second]);
        assert!(pending.take_all().is_empty());
    }

    #[cfg(feature = "native-notification")]
    #[test]
    fn native_notification_response_contract_routes_only_default_and_view() {
        use notify_rust::{CloseReason, NotificationResponse, ResponseHandler};

        fn assert_response_handler_contract(_handler: impl ResponseHandler) {}
        assert_response_handler_contract(|response: &NotificationResponse| {
            let _ = native_notification_response_disposition(response);
        });

        assert_eq!(
            native_notification_response_disposition(&NotificationResponse::Default),
            NativeNotificationResponseDisposition::Navigate
        );
        assert_eq!(
            native_notification_response_disposition(&NotificationResponse::Action(
                "view".to_string()
            )),
            NativeNotificationResponseDisposition::Navigate
        );
        assert_eq!(
            native_notification_response_disposition(&NotificationResponse::Action(
                "dismiss".to_string()
            )),
            NativeNotificationResponseDisposition::ClearDedup
        );
        assert_eq!(
            native_notification_response_disposition(&NotificationResponse::Reply(
                "ignored".to_string()
            )),
            NativeNotificationResponseDisposition::ClearDedup
        );
        assert_eq!(
            native_notification_response_disposition(&NotificationResponse::Closed(
                CloseReason::Dismissed
            )),
            NativeNotificationResponseDisposition::ClearDedup
        );
    }

    #[test]
    fn automatic_prompt_queue_only_notifies_for_the_terminal_success() {
        use gold_band::app::AcpTurnOutcome;

        assert!(!should_send_acp_turn_notification(
            AcpTurnOutcome::Completed,
            AcpTurnBatchProgress {
                completed_reply_count: 2,
                continues: true,
            },
        ));
        assert!(should_send_acp_turn_notification(
            AcpTurnOutcome::Completed,
            AcpTurnBatchProgress::terminal(3),
        ));
        assert!(should_send_acp_turn_notification(
            AcpTurnOutcome::Failed,
            AcpTurnBatchProgress {
                completed_reply_count: 2,
                continues: true,
            },
        ));
    }

    #[test]
    fn successful_direct_run_defers_notification_to_prompt_queue_batch() {
        use gold_band::domain::RunOutcome;

        assert!(should_defer_direct_run_completion_notification(
            RunOutcome::Success,
            Some("Claude"),
        ));
        assert!(!should_defer_direct_run_completion_notification(
            RunOutcome::Failure,
            Some("Claude"),
        ));
        assert!(!should_defer_direct_run_completion_notification(
            RunOutcome::Success,
            None,
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_toast_policy_maps_target_to_nearest_native_duration() {
        let short_policy = windows_toast_display_policy(10);
        let long_policy = windows_toast_display_policy(20);

        assert_eq!(short_policy, WindowsToastDisplayPolicy::AutoDismissShort);
        assert_eq!(long_policy, WindowsToastDisplayPolicy::AutoDismissLong);
        assert_eq!(
            expected_windows_toast_auto_dismiss_seconds(short_policy),
            WINDOWS_TOAST_SHORT_DURATION_SECONDS
        );
        assert_eq!(
            expected_windows_toast_auto_dismiss_seconds(long_policy),
            WINDOWS_TOAST_LONG_DURATION_SECONDS
        );
        assert_eq!(WINDOWS_TOAST_SHORT_DURATION_SECONDS, 7);
        assert_eq!(WINDOWS_TOAST_LONG_DURATION_SECONDS, 25);
    }
}
