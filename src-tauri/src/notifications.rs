use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use gold_band::app::notification::InterventionNotification;
use tauri::{AppHandle, Emitter, Manager};

/// 通知去重器：确保同一暂停原因只发一次 OS 通知
pub struct NotificationDedup {
    sent: Mutex<HashSet<String>>,
}

impl NotificationDedup {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(HashSet::new()),
        }
    }

    pub fn try_send(&self, dedup_key: &str) -> bool {
        let mut sent = self.sent.lock().unwrap();
        if sent.contains(dedup_key) {
            return false;
        }
        sent.insert(dedup_key.to_string());
        true
    }

    pub fn clear_node(
        &self,
        run_id: &str,
        round_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) {
        let prefix = format!("{}:{}:{}:{}", run_id, round_id, node_id, attempt_id);
        self.sent
            .lock()
            .unwrap()
            .retain(|key| !key.starts_with(&prefix));
    }
}

/// 发送系统级 OS 通知并 emit 事件到前端
pub fn send_intervention_notification(
    app_handle: &AppHandle,
    dedup: &NotificationDedup,
    notification: &InterventionNotification,
) {
    if !dedup.try_send(&notification.dedup_key) {
        return;
    }

    send_os_notification(app_handle, notification);

    let _ = app_handle.emit("gold-band://intervention-required", notification);
}

// ── Windows 实现（使用 tauri-winrt-notification，支持按钮） ──

#[cfg(target_os = "windows")]
fn send_os_notification(app_handle: &AppHandle, notification: &InterventionNotification) {
    use std::path::Path;
    use tauri_winrt_notification::{Toast, Duration, Scenario, IconCrop};

    let app_id = aumid(app_handle);

    let view_action = format!(
        "view|{}|{}|{}|{}",
        notification.task_id, notification.run_id, notification.round_id, notification.node_id
    );
    let dismiss_action = "dismiss".to_string();

    // 编译时解析图标路径
    let icon_path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "\\icons\\icon.png"));

    let handle = app_handle.clone();
    let _ = Toast::new(&app_id)
        .title(&notification.title)
        .text1(&notification.body)
        .icon(icon_path, IconCrop::Square, "码灵")
        .duration(Duration::Long)
        .scenario(Scenario::Reminder)
        .add_button("查看详情", &view_action)
        .add_button("忽略", &dismiss_action)
        .on_activated(move |action: Option<String>| {
            if let Some(action) = action {
                if action == "dismiss" {
                    return Ok(());
                }
                if let Some(rest) = action.strip_prefix("view|") {
                    let parts: Vec<&str> = rest.splitn(4, '|').collect();
                    if parts.len() == 4 {
                        let task_id = parts[0].to_string();
                        let run_id = parts[1].to_string();
                        let round_id = parts[2].to_string();
                        if let Some(window) = handle.get_webview_window("main") {
                            let _ = window.set_focus();
                            let _ = window.unminimize();
                            let _ = window.show();
                        }
                        let _ = handle.emit("gold-band://intervention-navigate", serde_json::json!({
                            "taskId": task_id,
                            "runId": run_id,
                            "roundId": round_id,
                        }));
                    }
                }
            }
            Ok(())
        })
        .show();
}

/// 获取/注册 Windows AUMID
///
/// Windows Toast 要求应用在开始菜单中存在快捷方式且设置了 AppUserModelID。
/// 首次运行时自动创建快捷方式并写入注册表 DisplayName。
#[cfg(target_os = "windows")]
fn aumid(app_handle: &AppHandle) -> String {
    let aumid = app_handle.config().identifier.clone();
    ensure_toast_aumid_registered(&aumid);
    ensure_notification_registry(&aumid);
    aumid
}

/// 应用启动时调用：注册 AUMID 注册表和快捷方式（Windows）
#[cfg(target_os = "windows")]
pub fn init_notification_support(app_handle: &AppHandle) {
    let aumid = app_handle.config().identifier.clone();
    ensure_toast_aumid_registered(&aumid);
    ensure_notification_registry(&aumid);
}

#[cfg(not(target_os = "windows"))]
pub fn init_notification_support(_app_handle: &AppHandle) {}

#[cfg(target_os = "windows")]
fn ensure_toast_aumid_registered(_aumid: &str) {
    use std::env;
    use std::fs;

    // 通过注册表设置 AUMID 的显示名称
    // HKCU\Software\Classes\AppUserModelId\{AUMID}
    //   DisplayName = "码灵"
    //   ShowInSettings = 0 (不显示在设置中)
    //
    // 同时创建开始菜单快捷方式（Windows Toast 要求快捷方式存在）
    let appdata = env::var("APPDATA").unwrap_or_default();
    let shortcut_dir = std::path::PathBuf::from(&appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("码灵");
    let shortcut_path = shortcut_dir.join("码灵.lnk");

    // 只在首次运行时创建
    if shortcut_path.exists() {
        return;
    }

    let exe_path = match env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    let _ = fs::create_dir_all(&shortcut_dir);

    // 使用 PowerShell 创建快捷方式并设置 AppUserModelID
    let ps_script = format!(
        r#"$dir = '{shortcut_dir}'
$lnk = '{shortcut_path}'
ni -ItemType Directory -Force -Path $dir | Out-Null
$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut($lnk)
$sc.TargetPath = '{exe}'
$sc.WorkingDirectory = '{workdir}'
$sc.Description = '码灵'
$sc.Save()
"#,
        shortcut_dir = shortcut_dir.to_string_lossy().replace('\\', "\\\\"),
        shortcut_path = shortcut_path.to_string_lossy().replace('\\', "\\\\"),
        exe = exe_path.to_string_lossy().replace('\\', "\\\\"),
        workdir = exe_path.parent()
            .map(|p| p.to_string_lossy().replace('\\', "\\\\"))
            .unwrap_or_default(),
    );

    let temp = env::temp_dir().join(format!("gold_band_toast_reg_{}.ps1", std::process::id()));
    let _ = fs::write(&temp, &ps_script);
    // 同步等待 PowerShell 完成：Windows Toast 的 on_activated 回调依赖
    // 开始菜单中存在带有正确 AppUserModelID 的快捷方式。
    // 如果 fire-and-forget，首个 Toast 弹出时快捷方式可能尚未创建完毕，
    // 导致"查看详情"按钮点击无效。
    let result = std::process::Command::new("powershell")
        .args(["-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
        .arg(&temp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(status) if status.success() => {
            let _ = fs::remove_file(&temp);
        }
        Ok(status) => {
            eprintln!(
                "[toast-init] PowerShell shortcut creation exited with {status}, keeping script at {}",
                temp.display()
            );
        }
        Err(error) => {
            eprintln!(
                "[toast-init] failed to run PowerShell for shortcut: {error}, keeping script at {}",
                temp.display()
            );
        }
    }
}

/// 首次运行时确保 Windows 通知注册表项存在（设置显示名称）
pub fn ensure_notification_registry(aumid: &str) {
    // 注册 DisplayName
    let _ = std::process::Command::new("reg")
        .args(["add", &format!(r"HKCU\Software\Classes\AppUserModelId\{aumid}"), "/v", "DisplayName", "/t", "REG_SZ", "/d", "码灵", "/f"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::process::Command::new("reg")
        .args(["add", &format!(r"HKCU\Software\Classes\AppUserModelId\{aumid}"), "/v", "ShowInSettings", "/t", "REG_DWORD", "/d", "0", "/f"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

// ── macOS / Linux 实现（fallback） ──

#[cfg(not(target_os = "windows"))]
fn send_os_notification(_app_handle: &AppHandle, notification: &InterventionNotification) {
    let _ = notify_rust::Notification::new()
        .appname("码灵")
        .summary(&notification.title)
        .body(&notification.body)
        .timeout(60_000)
        .show();
}

// ── 干预清除 ──

pub fn emit_intervention_resolved(
    app_handle: &AppHandle,
    dedup: &NotificationDedup,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
) {
    dedup.clear_node(run_id, round_id, node_id, attempt_id);
    let _ = app_handle.emit(
        "gold-band://intervention-resolved",
        serde_json::json!({
            "runId": run_id,
            "nodeId": node_id,
            "attemptId": attempt_id,
        }),
    );
}
