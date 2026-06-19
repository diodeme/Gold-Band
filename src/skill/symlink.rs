// ── SKILL Symlink 映射 ──
// 将 Gold-Band 管理的 .agents/skills/ 映射到 .claude/skills/
// 供 Claude Code 等外部 Agent 自动发现 SKILL

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, warn};

use crate::config::SkillMeta;

/// 增量同步所有 SKILL symlink（保存时 + 启动时调用）
///
/// - 全局 SKILL → ~/.claude/skills/
/// - 项目 SKILL → <workspace>/.claude/skills/
///
/// 同名时 Project > Global（项目覆盖全局）
pub fn sync_all(workspace: &Path, global_skills: &[SkillMeta], project_skills: &[SkillMeta]) {
    debug!(
        "syncing skill symlinks: {} global, {} project → workspace {:?}",
        global_skills.len(),
        project_skills.len(),
        workspace
    );

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let global_target = home.join(".claude").join("skills");
    sync_to_target(&global_target, global_skills, &[]);

    // 项目 target 仅在有实际 workspace 路径时处理
    if !workspace.as_os_str().is_empty() {
        let project_target = workspace.join(".claude").join("skills");
        sync_to_target(&project_target, global_skills, project_skills);
    }
}

fn sync_to_target(target_dir: &Path, global: &[SkillMeta], project: &[SkillMeta]) {
    if fs::create_dir_all(target_dir).is_err() {
        debug!("failed to create target dir: {:?}", target_dir);
        return;
    }

    // 1. 构建期望映射: name → source_dir（Project 覆盖 Global）
    let mut desired: BTreeMap<&str, &Path> = BTreeMap::new();
    for s in global {
        desired.insert(&s.name, Path::new(&s.directory_path));
    }
    for s in project {
        desired.insert(&s.name, Path::new(&s.directory_path));
    }

    // 2. 扫描现有条目 — 删除过时的链接，保留正确的（即使 desired 为空也要清理）
    let mut existing: HashSet<String> = HashSet::new();
    if let Ok(entries) = fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };

            let is_correct_link = desired
                .get(name.as_str())
                .map(|&expected_src| {
                    path.read_link()
                        .ok()
                        .as_ref()
                        .map(|p| p.as_path() == expected_src)
                        .unwrap_or(false)
                })
                .unwrap_or(false);

            if is_correct_link {
                existing.insert(name);
                continue;
            }

            // 不在期望列表中 或 指向错误 → 删除
            let is_link = path.is_symlink() || path.read_link().is_ok();
            let is_stale_dir = path.is_dir() && !is_link && !desired.contains_key(name.as_str());
            if is_link || is_stale_dir {
                debug!("removing stale link: {:?}", path);
                if fs::remove_file(&path).is_err() {
                    let _ = fs::remove_dir(&path);
                }
            } else if desired.contains_key(name.as_str()) {
                debug!("skipping non-link entry: {:?}", path);
            }
        }
    }

    // 3. 创建缺失的 symlink（跳过此步如果没有期望的 SKILL）
    if desired.is_empty() {
        return;
    }
    for (name, source) in &desired {
        if existing.contains(*name) {
            continue;
        }
        let target = target_dir.join(name);
        if target.exists() {
            continue;
        }
        create_link(source, &target);
    }
}

/// 跨平台创建目录链接：Unix → symlink, Windows → symlink(需要DevMode) → mklink /J 回退
fn create_link(src: &Path, dst: &Path) {
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(src, dst) {
            Ok(()) => debug!("created symlink: {:?} → {:?}", dst, src),
            Err(e) => warn!("failed to create symlink {:?} → {:?}: {e}", dst, src),
        }
    }

    #[cfg(windows)]
    {
        // 方式 1: symlink_dir（需要 Win10 1703+ 且开启开发者模式）
        match std::os::windows::fs::symlink_dir(src, dst) {
            Ok(()) => {
                debug!("created symlink: {:?} → {:?}", dst, src);
                return;
            }
            Err(e) => debug!("symlink_dir failed (will try junction): {e}"),
        }

        // 方式 2: mklink /J junction（不需要特殊权限）
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(dst.as_os_str())
            .arg(src.as_os_str())
            .output();

        match output {
            Ok(o) if o.status.success() => {
                debug!("created junction: {:?} → {:?}", dst, src);
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!(
                    "failed to create link {:?} → {:?}: {}. Try enabling Developer Mode or run as Administrator.",
                    dst, src, stderr.trim()
                );
            }
            Err(e) => {
                warn!("failed to create link {:?} → {:?}: {e}", dst, src);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SkillMeta, SkillSource};
    use std::fs;

    #[test]
    fn sync_creates_and_removes_links() {
        let tmp = std::env::temp_dir().join("gb-symlink-test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let src_dir = tmp.join(".agents").join("skills").join("test-skill");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("SKILL.md"), "---\nname: test-skill\ndescription: test\n---\nhello").unwrap();

        let meta = SkillMeta {
            name: "test-skill".into(),
            description: "test".into(),
            source: SkillSource::Project,
            directory_path: src_dir.to_string_lossy().to_string(),
            disable_model_invocation: false,
            load_warnings: vec![],
        };

        sync_all(&tmp, &[], &[meta]);

        let link = tmp.join(".claude").join("skills").join("test-skill");
        println!("link exists: {}, is_symlink: {}", link.exists(), link.is_symlink());
        if let Ok(t) = link.read_link() {
            println!("link target: {:?}", t);
        }

        assert!(link.exists(), "link was not created: {:?}", link);

        // simulate delete: sync with empty list
        fs::remove_dir_all(&src_dir).unwrap();
        sync_all(&tmp, &[], &[]);

        assert!(!link.exists(), "stale link was not cleaned up");

        let _ = fs::remove_dir_all(&tmp);
    }
}
