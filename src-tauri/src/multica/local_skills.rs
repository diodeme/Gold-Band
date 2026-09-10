//! multica 本地 skill 同步逻辑层（发现上报 / 导入打包 / 拉取组装）。
//!
//! 三块职责（设计文档 `.claude/design/multica_skill/`）：
//! - **发现映射**：`SkillManager::list()` 的全局库（`agent_source == ".gold-band"`，排除 agent 目录
//!   只读来源与 symlink 副本）→ multica `LocalSkillSummary`（key = 目录名，canonical 身份）。
//! - **导入打包**：按 skill_key 读目录 → SKILL.md 原文 + 支撑文件，预检 multica 尺寸约束
//!   （超限即 failed，不截断不部分上报）。
//! - **拉取组装**：远端 content 异构（可能含 frontmatter——multica DB 不强制 body-only），
//!   组装时判别剥离防双 frontmatter，附加字段原样保留；`multica_skill_dir_name` 清洗远端展示名
//!   为本地目录名（无映射表，确定性幂等）。
//!
//! wire 类型（report/bundle/summary）在 `client.rs`；本模块只做纯映射与文件系统读取，
//! 文件系统函数目录参数化（不依赖真实 home），全部可单测。

use std::fs;
use std::path::{Path, PathBuf};

use camino::Utf8Path;
use gold_band::app::App;
use gold_band::config::{SkillMeta, SkillSource, SKILL_FILE_NAME};
use gold_band::frontmatter::{
    parse_optional_frontmatter_document, render_frontmatter_document, FrontmatterUpdate,
};
use gold_band::skill::{parse_skill_md_public, skill_dir_name_from_str};
use gold_band::storage::GoldBandPaths;
use tauri::{AppHandle, Manager, Runtime};

use crate::multica::client::{
    LocalSkillBundle, LocalSkillFile, LocalSkillImportReport, LocalSkillListReport,
    LocalSkillSummary, MulticaClient,
};
use crate::state::DesktopState;

/// 码灵自有 skill 库的 agent_source 标识（`SkillManager::list()` 全局扫描的第一段来源）。
///
/// 硬编码字面量与 `src/skill/mod.rs` 的 `scan_skills_dir(..., ".gold-band")` 同源——
/// 即使 MALING 构建全局目录是 `~/.maling/skills`，该标识也恒为 `.gold-band`（渠道无关的库身份）。
const OWN_LIBRARY_AGENT_SOURCE: &str = ".gold-band";

/// multica 导入尺寸约束（对齐服务端，超限即 failed 上报；问题清单附录）。
pub const IMPORT_MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const IMPORT_MAX_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;
pub const IMPORT_MAX_FILES: usize = 256;
/// 相对路径深度上限（`a/b/c/d.md` = 4）。
pub const IMPORT_MAX_DEPTH: usize = 4;

/// 清洗远端展示名为本地 skill 目录名（设计 §5.2，无映射表方案）。
///
/// 最小清洗：文件系统非法字符（Windows `<>:"/\|?*`）与其余控制符删除、连续空白折叠为单个 `-`、
/// 首尾 `-` 去除、保留大小写与中文。清洗后为空回退 `skill-{id 前 8 位}`。
/// 纯函数、确定性、幂等（对已合规名无变化）。
///
/// **空白优先于控制符删除**：`\t`/`\n`/`\r` 等既是空白又是控制符，须按空白折叠为段落分隔符
/// （`a\tb` → `a-b`），否则「连续空白 → `-`」对这类字符永久失效、把两个词粘连成 `ab`
/// （设计 §5.2 两条规则的优先级在此显式化）。
pub fn multica_skill_dir_name(remote_name: &str, remote_id: &str) -> String {
    let mut out = String::with_capacity(remote_name.len());
    for c in remote_name.chars() {
        if c.is_whitespace() {
            // 连续空白折叠为单个 `-`（含与已有 `-` 相邻的空白）。
            if !out.ends_with('-') {
                out.push('-');
            }
        } else if c.is_control() || is_windows_illegal(c) {
            continue;
        } else {
            out.push(c);
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        // 清洗后为空（纯非法字符/纯空白）→ `skill-{id 前 8 位}`（id 为服务端字母数字，无需再清洗）。
        let suffix: String = remote_id.chars().take(8).collect();
        if suffix.is_empty() {
            "skill".to_string()
        } else {
            format!("skill-{suffix}")
        }
    } else {
        trimmed.to_string()
    }
}

/// Windows 文件名非法字符（`<>:"/\|?*`）。
fn is_windows_illegal(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

/// 全局库 SkillMeta 列表 → multica 发现上报的 summary 列表。
///
/// 仅上报码灵自有库（`agent_source == ".gold-band"`）；name/description 取 frontmatter
/// （`parse_skill_md` 已做目录名回退，无需二次兜底）；file_count 递归统计目录内全部文件
/// （含 SKILL.md）。key 为空（directory_path 无文件名段，理论不可达）的条目跳过。
pub fn own_global_skill_summaries(skills: &[SkillMeta], provider: &str) -> Vec<LocalSkillSummary> {
    skills
        .iter()
        .filter(|meta| meta.agent_source == OWN_LIBRARY_AGENT_SOURCE)
        .filter_map(|meta| {
            let key = skill_dir_name_from_str(&meta.directory_path)?.to_string();
            Some(LocalSkillSummary {
                file_count: count_skill_files(Path::new(&meta.directory_path)),
                key,
                name: meta.name.clone(),
                description: meta.description.clone(),
                source_path: meta.directory_path.clone(),
                provider: provider.to_string(),
                root: "provider",
            })
        })
        .collect()
}

/// 递归统计 skill 目录内文件数（含 SKILL.md；不跟随 symlink——与 `scan_skills_dir`
/// 顶层跳过 symlink 的口径一致，防 agent 同步副本重复计数与符号环）。
fn count_skill_files(dir: &Path) -> i64 {
    let mut count: i64 = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            count += count_skill_files(&path);
        } else if path.is_file() {
            count += 1;
        }
    }
    count
}

/// 读取 skill 目录打包为 multica 导入 bundle（设计 §4.4/§4.5）。
///
/// `dir` 为 skill 目录绝对路径（调用方经 [`resolve_own_skill_dir`] 从全局库根 + skill_key
/// 拼接并校验）。错误以纯文本返回（直接进 failed 上报的 error 字段，服务端原样透出到 Web）：
/// - 目录不存在 → `skill not found: <key>`；SKILL.md 缺失 → 对应原因
/// - 单文件 >1MiB / 整包 >8MiB / 文件数 >256 / 深度 >4 / 非 UTF-8 文件 → 对应具体原因
///   （multica 契约为文本 content 无二进制通道，非 UTF-8 **整包 failed** 并列明路径，不静默丢失）
///
/// content 为 SKILL.md **原文**（含 frontmatter 与附加字段——multica DB 不强制 body-only，
/// 2026-09-09 Q5 修正：原样透传即天然保真）；name/description 从 frontmatter 解析
/// （name 缺失回退目录名）。支撑文件路径为相对路径（`/` 分隔），按路径排序保证上报确定性。
pub fn read_local_skill_bundle(
    dir: &Utf8Path,
    skill_key: &str,
    provider: &str,
) -> Result<LocalSkillBundle, String> {
    if !dir.is_dir() {
        return Err(format!("skill not found: {skill_key}"));
    }
    let skill_md_path = dir.join(SKILL_FILE_NAME);
    if !skill_md_path.is_file() {
        return Err(format!("SKILL.md missing in skill: {skill_key}"));
    }
    let skill_md_len = file_len(skill_md_path.as_std_path())?;
    if skill_md_len > IMPORT_MAX_FILE_BYTES {
        return Err(format!("SKILL.md exceeds 1MiB limit in skill: {skill_key}"));
    }
    let content = fs::read_to_string(&skill_md_path)
        .map_err(|e| format!("SKILL.md not utf-8 readable in skill {skill_key}: {e}"))?;

    // 递归收集支撑文件（跳过根 SKILL.md；symlink 一律跳过——见 count_skill_files 口径）。
    let mut collected: Vec<(String, PathBuf, u64)> = Vec::new();
    collect_files_recursive(dir.as_std_path(), &mut Vec::new(), &mut collected)?;
    collected.sort_by(|a, b| a.0.cmp(&b.0));

    if 1 + collected.len() > IMPORT_MAX_FILES {
        return Err(format!(
            "skill bundle exceeds {IMPORT_MAX_FILES} file limit: {skill_key} ({} files)",
            1 + collected.len()
        ));
    }
    let total = skill_md_len + collected.iter().map(|(_, _, len)| *len).sum::<u64>();
    if total > IMPORT_MAX_BUNDLE_BYTES {
        return Err(format!("skill bundle exceeds 8MiB limit: {skill_key}"));
    }

    let mut files = Vec::with_capacity(collected.len());
    for (rel, path, _) in &collected {
        let text =
            fs::read_to_string(path).map_err(|_| format!("non-utf8 file (binary not supported): {rel}"))?;
        files.push(LocalSkillFile {
            path: rel.clone(),
            content: text,
        });
    }

    let dir_string = dir.as_str().to_string();
    let (meta, _body) = parse_skill_md_public(
        &content,
        skill_key,
        SkillSource::Global,
        &dir_string,
        OWN_LIBRARY_AGENT_SOURCE,
    );
    Ok(LocalSkillBundle {
        name: meta.name,
        description: meta.description,
        content,
        source_path: dir_string,
        provider: provider.to_string(),
        files,
    })
}

/// 递归收集**支撑文件**：`(相对路径 "/" 分隔, 绝对路径, 字节数)`。
///
/// 根 `SKILL.md` 不是支撑文件——它由 [`LocalSkillBundle::content`] 承载，故在根层跳过：
/// 重复收集会把正文再上报一次，并让同一个文件同时占掉「1（content）+ 1（支撑）」两份
/// 文件数与字节限额，使恰好 256 文件的 skill 被误判超限（子目录内的 `SKILL.md` 仍是支撑文件）。
fn collect_files_recursive(
    dir: &Path,
    rel_prefix: &mut Vec<String>,
    out: &mut Vec<(String, PathBuf, u64)>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read dir failed: {e}"))?;
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if rel_prefix.is_empty() && name == SKILL_FILE_NAME {
            continue;
        }
        if path.is_dir() {
            if rel_prefix.len() + 1 > IMPORT_MAX_DEPTH {
                let rel = rel_prefix.join("/");
                return Err(format!(
                    "skill file depth exceeds {IMPORT_MAX_DEPTH}: {rel}/{name}"
                ));
            }
            rel_prefix.push(name);
            collect_files_recursive(&path, rel_prefix, out)?;
            rel_prefix.pop();
        } else if path.is_file() {
            if rel_prefix.len() + 1 > IMPORT_MAX_DEPTH {
                let rel = rel_prefix.join("/");
                return Err(format!(
                    "skill file depth exceeds {IMPORT_MAX_DEPTH}: {rel}/{name}"
                ));
            }
            let len = file_len(&path)?;
            if len > IMPORT_MAX_FILE_BYTES {
                let rel = format!("{}/{name}", rel_prefix.join("/"));
                return Err(format!("file exceeds 1MiB limit: {rel}"));
            }
            let rel = if rel_prefix.is_empty() {
                name
            } else {
                format!("{}/{name}", rel_prefix.join("/"))
            };
            out.push((rel, path, len));
        }
    }
    Ok(())
}

fn file_len(path: &Path) -> Result<u64, String> {
    fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| format!("read metadata failed for {}: {e}", path.display()))
}

/// 把服务端回传的 skill_key 解析为全局库内的 skill 目录。
///
/// skill_key 是**服务端控制的文件系统路径输入**（码灵上报的 key 恒为裸目录名，但异常/恶意回传
/// `../`、分隔符、`.` 时不得触达库外路径）：分隔符与 `.`/`..` 整体拒绝（防路径穿越），
/// 再要求目录真实存在于全局库内，否则按 `skill not found` 语义上报。
pub fn resolve_own_skill_dir(skill_key: &str) -> Result<camino::Utf8PathBuf, String> {
    if skill_key.is_empty()
        || skill_key.contains('/')
        || skill_key.contains('\\')
        || skill_key == "."
        || skill_key == ".."
    {
        return Err(format!("skill not found: {skill_key}"));
    }
    let dir = GoldBandPaths::global_skills_dir().join(skill_key);
    if !dir.is_dir() {
        return Err(format!("skill not found: {skill_key}"));
    }
    Ok(dir)
}

/// 拉取组装：远端 skill content → 落库用 SKILL.md 全文（设计 §5.3）。
///
/// 远端 content 异构，须先判别（multica DB 不强制 body-only）：
/// - 以 `---` 开头且能解析出闭合 frontmatter 与至少一个有效字段 → 剥出 body 与字段 map，
///   **附加字段（`allowed-tools` 等）原样保留**（无有效字段则视为正文误以 `---` 开头，整段作 body，
///   避免把水平分割线误判成 frontmatter）
/// - 其余 → body = content 整段，字段 map 为空
///
/// 最终 frontmatter：`name: <远端展示名>`（固定以远端 name 列为准——Web 改名后列名为权威展示名）
/// + `description: <远端 description 列，空则回退 content 内 description，均无则省略>`
/// + 保留的附加字段。保证「码灵导出 → 导入 → 拉回」附加字段零丢失且不产生双 frontmatter。
pub fn assemble_pulled_skill_md(
    remote_name: &str,
    remote_description: &str,
    content: &str,
) -> String {
    let doc = parse_optional_frontmatter_document(content)
        .ok()
        .filter(|doc| !doc.fields.is_empty());
    let (body, fields) = match doc {
        Some(doc) => (doc.body, doc.fields),
        None => (content.to_string(), Default::default()),
    };
    let content_description = fields
        .get("description")
        .filter(|d| !d.trim().is_empty())
        .map(|d| d.trim().to_string());
    let description = if !remote_description.trim().is_empty() {
        Some(remote_description.trim().to_string())
    } else {
        content_description
    };

    let mut updates: Vec<FrontmatterUpdate<'_>> = vec![FrontmatterUpdate {
        key: "name",
        value: remote_name,
        source: None,
    }];
    if let Some(desc) = description.as_deref() {
        updates.push(FrontmatterUpdate {
            key: "description",
            value: desc,
            source: None,
        });
    }
    for (key, value) in &fields {
        if key != "name" && key != "description" {
            updates.push(FrontmatterUpdate {
                key,
                value,
                source: None,
            });
        }
    }
    render_frontmatter_document(&updates, &body)
}

/// 拉取「已存在」判定 + 覆盖定位：全局自有库（`agent_source == ".gold-band"`）中存在同名目录时，
/// 返回该 skill 的 `directory_path`（**绝对路径**）。
///
/// 返回值直接作为 `write_instance` 的 `current_directory_path`——该参数是路径而非目录名
/// （`save_target_dir` 原样取其作目标目录、`write_instance` 校验其存在），传裸目录名会被当作
/// 相对路径按进程 CWD 解析而报 `SKILL dir not found`。同时该目录也是覆盖时读取旧内容
/// （保留本地未知 frontmatter 字段）的基准。
pub fn own_global_skill_dir(skills: &[SkillMeta], dir_name: &str) -> Option<String> {
    skills
        .iter()
        .find(|meta| {
            meta.agent_source == OWN_LIBRARY_AGENT_SOURCE
                && skill_dir_name_from_str(&meta.directory_path) == Some(dir_name)
        })
        .map(|meta| meta.directory_path.clone())
}

// ── 心跳 ack 待办处理（loop_.rs spawn 调用；绝不阻塞心跳 tick）──────────────────────

/// 处理发现待办：扫描全局库 → 上报结果（completed/failed）。
///
/// 无状态消费：requestId 不持久化，上报失败经 client 内 3 次网络重试后放弃并记日志，
/// 服务端 60s running 超时终态化兜底（multica 对迟到/重复上报幂等返回 200）。
pub(crate) async fn handle_local_skill_discovery<R: Runtime>(
    app: AppHandle<R>,
    client: MulticaClient,
    runtime_id: String,
    provider: String,
    request_id: String,
) {
    let report = match home_app(&app) {
        None => failed_list_report("app state unavailable".to_string()),
        Some(app) => {
            // 目录扫描是阻塞 FS（百级目录 <1s），放 spawn_blocking 不占 async worker。
            let scan =
                tauri::async_runtime::spawn_blocking(move || app.skill_manager().list()).await;
            match scan {
                Ok(Ok(list)) => LocalSkillListReport {
                    status: "completed",
                    skills: own_global_skill_summaries(&list.global, &provider),
                    supported: true,
                    mcp_supported: false,
                    error: None,
                },
                Ok(Err(error)) => failed_list_report(format!("skill scan failed: {error}")),
                Err(error) => failed_list_report(format!("skill scan task failed: {error}")),
            }
        }
    };
    if let Err(error) = client
        .report_local_skill_list_result(&runtime_id, &request_id, &report)
        .await
    {
        tracing::warn!(
            runtime_id = %runtime_id,
            request_id = %request_id,
            %error,
            "multica local skill list report failed after retries (server timeout will finalize)"
        );
    }
}

/// 处理导入待办：按 skill_key 读目录打包 → 上报结果（completed/failed）。
///
/// skill 在发现后被本地删除/破坏 → failed 上报（Web 端可见原因），不静默吞掉。
pub(crate) async fn handle_local_skill_import(
    client: MulticaClient,
    runtime_id: String,
    provider: String,
    request_id: String,
    skill_key: String,
) {
    let report = match resolve_own_skill_dir(&skill_key) {
        Err(error) => LocalSkillImportReport {
            status: "failed",
            skill: None,
            error: Some(error),
        },
        Ok(dir) => {
            let key = skill_key.clone();
            let bundle = tauri::async_runtime::spawn_blocking(move || {
                read_local_skill_bundle(&dir, &key, &provider)
            })
            .await;
            match bundle {
                Ok(Ok(bundle)) => LocalSkillImportReport {
                    status: "completed",
                    skill: Some(bundle),
                    error: None,
                },
                Ok(Err(error)) => LocalSkillImportReport {
                    status: "failed",
                    skill: None,
                    error: Some(error),
                },
                Err(error) => LocalSkillImportReport {
                    status: "failed",
                    skill: None,
                    error: Some(format!("read skill task failed: {error}")),
                },
            }
        }
    };
    if let Err(error) = client
        .report_local_skill_import_result(&runtime_id, &request_id, &report)
        .await
    {
        tracing::warn!(
            runtime_id = %runtime_id,
            request_id = %request_id,
            skill_key = %skill_key,
            %error,
            "multica local skill import report failed after retries (server timeout will finalize)"
        );
    }
}

/// 全局 skill 库是 home 作用域（渠道目录，非 workspace 目录），取 home App 做 `SkillManager`
/// 扫描入口（与 `invalidate_remote_task` 的 `context.app()` 同一取法）。
fn home_app<R: Runtime>(app: &AppHandle<R>) -> Option<App> {
    let desktop = app.try_state::<DesktopState>()?;
    let context = desktop.context().ok()?;
    Some(context.app())
}

fn failed_list_report(error: String) -> LocalSkillListReport {
    LocalSkillListReport {
        status: "failed",
        skills: Vec::new(),
        supported: true,
        mcp_supported: false,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::skill::SkillManager;

    #[test]
    fn dir_name_is_idempotent_for_regular_names() {
        assert_eq!(multica_skill_dir_name("pr-review", "sk-1"), "pr-review");
        assert_eq!(multica_skill_dir_name("PR review", "sk-1"), "PR-review");
        // 中文与大小写保留。
        assert_eq!(
            multica_skill_dir_name("代码审查 Skill", "sk-1"),
            "代码审查-Skill"
        );
        // 幂等：清洗结果再清洗不变。
        let once = multica_skill_dir_name("My Cool Skill", "sk-1");
        assert_eq!(multica_skill_dir_name(&once, "sk-1"), once);
    }

    #[test]
    fn dir_name_strips_illegal_and_collapses_whitespace() {
        // Windows 非法字符删除、控制符删除。
        assert_eq!(multica_skill_dir_name("a<b>c:\"d/e|f?g*h", "sk-1"), "abcdefgh");
        assert_eq!(multica_skill_dir_name("a\tb\nc", "sk-1"), "a-b-c");
        // 连续空白折叠为单个 -，首尾 - 去除。
        assert_eq!(
            multica_skill_dir_name("  spaced   out  ", "sk-1"),
            "spaced-out"
        );
        assert_eq!(multica_skill_dir_name("---trim---", "sk-1"), "trim");
    }

    #[test]
    fn dir_name_falls_back_to_skill_id_prefix_when_empty() {
        assert_eq!(
            multica_skill_dir_name("???***", "sk-abcdef12345"),
            "skill-sk-abcde"
        );
        assert_eq!(multica_skill_dir_name("   ", "0123456789"), "skill-01234567");
        assert_eq!(multica_skill_dir_name("", "short"), "skill-short");
        assert_eq!(multica_skill_dir_name("**", ""), "skill");
    }

    #[test]
    fn assemble_strips_remote_frontmatter_and_keeps_extra_fields() {
        // 远端 content 含 frontmatter + 附加字段：剥离重组，name 固定远端展示名列，附加字段保留。
        // 渲染走 canonical `render_frontmatter_document`（与编辑器写盘同源）：值含非
        // `[A-Za-z0-9._-]` 字符时按 YAML 规则加引号，`allowed-tools: Read` 为纯字母故不加。
        let content = "---\nname: old-name\nallowed-tools: Read\n---\n\nBody here\n";
        let out = assemble_pulled_skill_md("PR review", "Review PRs", content);
        assert_eq!(
            out,
            "---\nname: \"PR review\"\ndescription: \"Review PRs\"\nallowed-tools: Read\n---\n\nBody here\n"
        );
    }

    #[test]
    fn assemble_body_only_content_gets_fresh_frontmatter() {
        let out = assemble_pulled_skill_md("PR review", "Review PRs", "Just body\n");
        assert_eq!(
            out,
            "---\nname: \"PR review\"\ndescription: \"Review PRs\"\n---\nJust body\n"
        );
    }

    #[test]
    fn assemble_description_falls_back_to_content_then_omits() {
        // 远端 description 列为空 → 回退 content 内 description（含空格的值得加引号）。
        let with_content_desc = "---\nname: x\ndescription: from content\n---\nbody";
        let out = assemble_pulled_skill_md("N", "", with_content_desc);
        assert_eq!(out, "---\nname: N\ndescription: \"from content\"\n---\nbody");

        // 两侧均无 → 省略 description。
        let out = assemble_pulled_skill_md("N", "  ", "plain body");
        assert_eq!(out, "---\nname: N\n---\nplain body");
    }

    #[test]
    fn assemble_treats_horizontal_rule_as_body() {
        // 正文以 ---（水平分割线）开头且无闭合 frontmatter → 整段作 body，不误剥。
        let hr_body = "---\nline under rule\n";
        let out = assemble_pulled_skill_md("N", "", hr_body);
        assert_eq!(out, "---\nname: N\n---\n---\nline under rule\n");

        // 闭合但零有效字段（---\n---\n）→ 同样整段作 body（防双 frontmatter 由组装端保证）。
        let empty_fm = "---\n---\nreal body";
        let out = assemble_pulled_skill_md("N", "", empty_fm);
        assert_eq!(out, "---\nname: N\n---\n---\n---\nreal body");
    }

    #[test]
    fn assemble_prefers_remote_description_column_over_content() {
        // 远端 description 列非空时优先（Web 改名/改描述后列名为权威）。
        let content = "---\nname: x\ndescription: from content\n---\nbody";
        let out = assemble_pulled_skill_md("N", "from column", content);
        assert!(out.contains("description: \"from column\"\n"));
        assert!(!out.contains("from content"));
    }

    fn meta(dir_path: &str, agent_source: &str) -> SkillMeta {
        SkillMeta {
            name: format!("name-{dir_path}"),
            description: "d".into(),
            source: SkillSource::Global,
            directory_path: dir_path.into(),
            agent_source: agent_source.into(),
            load_warnings: vec![],
            synced_agent_types: Vec::new(),
        }
    }

    #[test]
    fn discovery_summary_filters_own_library_and_maps_fields() {
        let skills = vec![
            meta("C:/home/.gold-band/skills/pr-review", ".gold-band"),
            meta("C:/home/.claude/skills/pr-review", ".claude"),
            meta("C:/home/.gold-band/skills/other", ".gold-band"),
        ];
        let out = own_global_skill_summaries(&skills, "claude-acp");
        // agent_source 过滤：.claude 来源排除（agent 只读来源/symlink 副本，防重复上报）。
        assert_eq!(out.len(), 2);
        let first = out.iter().find(|s| s.key == "pr-review").unwrap();
        assert_eq!(first.name, "name-C:/home/.gold-band/skills/pr-review");
        assert_eq!(first.provider, "claude-acp");
        assert_eq!(first.root, "provider");
        assert_eq!(first.source_path, "C:/home/.gold-band/skills/pr-review");
    }

    #[test]
    fn own_global_skill_dir_returns_absolute_dir_of_matching_own_library_skill() {
        let skills = vec![
            meta("C:/home/.gold-band/skills/to-spec", ".gold-band"),
            meta("C:/home/.gold-band/skills/pr-review", ".gold-band"),
            meta("C:/home/.claude/skills/other", ".claude"),
        ];
        // 命中自有库同名目录 → 返回既有 skill 的 directory_path（绝对路径，覆盖写入定位用）。
        assert_eq!(
            own_global_skill_dir(&skills, "to-spec").as_deref(),
            Some("C:/home/.gold-band/skills/to-spec")
        );
        // 仅 agent 来源命中（.claude）不算存在——那是外部库，覆盖会误伤。
        assert_eq!(own_global_skill_dir(&skills, "other"), None);
        assert_eq!(own_global_skill_dir(&skills, "absent"), None);
    }

    /// 回归用户实测症状：覆盖拉取必须把**既有目录绝对路径**交给 `write_instance`，
    /// 传裸目录名会被当作相对路径解析，报 `SKILL dir not found: "<name>"`。
    #[test]
    fn overwrite_requires_absolute_existing_dir_not_bare_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_skill::<&str, &str>(
            tmp.path().join("to-spec").as_path(),
            "---\nname: to-spec\n---\nold body",
            &[],
        );
        let bare_name = format!("to-spec-bare-{}", std::process::id());
        let manager = SkillManager::new(
            GoldBandPaths::new(camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap()),
            std::collections::BTreeMap::new(),
        );

        // 裸目录名（相对路径）→ 既有目录校验失败，即用户所见错误。
        let err = manager
            .write_instance(
                "to-spec",
                SkillSource::Global,
                "---\nname: to-spec\n---\nnew body",
                None,
                None,
                Some(&bare_name),
                None,
            )
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("SKILL dir not found"),
            "{err:#}"
        );

        // 既有目录绝对路径 → 覆盖成功（发现端返回的 directory_path 语义）。
        manager
            .write_instance(
                "to-spec",
                SkillSource::Global,
                "---\nname: to-spec\n---\nnew body",
                None,
                None,
                Some(dir.as_str()),
                None,
            )
            .unwrap();
        let written = fs::read_to_string(dir.join("SKILL.md").as_std_path()).unwrap();
        assert!(written.contains("new body"), "{written}");
        assert!(!written.contains("old body"), "{written}");
        // 覆盖写在既有目录内（本地身份/目录名不变）——不是新建到别处。
        assert!(dir.join("SKILL.md").is_file());
    }

    fn write_skill<S: AsRef<str>, C: AsRef<str>>(
        dir: &Path,
        skill_md: &str,
        files: &[(S, C)],
    ) -> camino::Utf8PathBuf {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), skill_md).unwrap();
        for (rel, content) in files {
            let path = dir.join(rel.as_ref().replace('/', std::path::MAIN_SEPARATOR_STR));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content.as_ref()).unwrap();
        }
        camino::Utf8PathBuf::from_path_buf(dir.to_path_buf()).unwrap()
    }

    #[test]
    fn bundle_reads_skill_md_verbatim_and_support_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_skill(
            tmp.path().join("pr-review").as_path(),
            "---\nname: PR review\nallowed-tools: Read\n---\n\nBody\n",
            &[("assets/template.md", "# tpl"), ("nested/deep/ref.md", "ref")],
        );
        let bundle = read_local_skill_bundle(&dir, "pr-review", "claude-acp").unwrap();
        // content = SKILL.md 原文（含 frontmatter 与附加字段，Q5 修正：原样透传）。
        assert_eq!(
            bundle.content,
            "---\nname: PR review\nallowed-tools: Read\n---\n\nBody\n"
        );
        assert_eq!(bundle.name, "PR review");
        // 支撑文件按路径排序、相对路径 / 分隔；根 SKILL.md 由 content 承载，不得混入支撑文件。
        assert_eq!(bundle.files.len(), 2);
        assert_eq!(bundle.files[0].path, "assets/template.md");
        assert_eq!(bundle.files[0].content, "# tpl");
        assert_eq!(bundle.files[1].path, "nested/deep/ref.md");
        assert!(!bundle.files.iter().any(|file| file.path == "SKILL.md"));
        assert_eq!(bundle.provider, "claude-acp");
        assert_eq!(bundle.source_path, dir.as_str());
    }

    #[test]
    fn bundle_does_not_count_root_skill_md_toward_file_limit() {
        // 限额口径是「SKILL.md + 支撑文件」总数 256：255 个支撑文件恰好到顶，必须通过。
        // 根 SKILL.md 重复计入支撑文件会让第 256 个文件被误判超限（且把正文重复上报为支撑文件）。
        let tmp = tempfile::tempdir().unwrap();
        let files: Vec<(String, &str)> = (0..IMPORT_MAX_FILES - 1)
            .map(|i| (format!("f{i:03}.md"), "x"))
            .collect();
        let dir = write_skill(
            tmp.path().join("edge-files").as_path(),
            "---\nname: e\n---\nbody",
            &files,
        );
        let bundle = read_local_skill_bundle(&dir, "edge-files", "claude-acp").unwrap();
        assert_eq!(bundle.files.len(), IMPORT_MAX_FILES - 1);
    }

    #[test]
    fn bundle_fails_for_missing_dir_and_missing_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let absent =
            camino::Utf8PathBuf::from_path_buf(tmp.path().join("ghost").to_path_buf()).unwrap();
        assert_eq!(
            read_local_skill_bundle(&absent, "ghost", "claude-acp").unwrap_err(),
            "skill not found: ghost"
        );

        let no_md =
            camino::Utf8PathBuf::from_path_buf(tmp.path().join("no-skill-md").to_path_buf()).unwrap();
        fs::create_dir_all(no_md.as_std_path()).unwrap();
        assert!(
            read_local_skill_bundle(&no_md, "no-skill-md", "claude-acp")
                .unwrap_err()
                .contains("SKILL.md missing")
        );
    }

    #[test]
    fn bundle_enforces_file_count_depth_and_size_limits() {
        // 文件数超限：SKILL.md + 256 支撑文件 = 257。
        let tmp = tempfile::tempdir().unwrap();
        let many: Vec<(String, &str)> = (0..IMPORT_MAX_FILES)
            .map(|i| (format!("f{i:03}.md"), "x"))
            .collect();
        let dir = write_skill(
            tmp.path().join("many").as_path(),
            "---\nname: many\n---\nbody",
            &many,
        );
        let err = read_local_skill_bundle(&dir, "many", "claude-acp").unwrap_err();
        assert!(err.contains("256 file limit"), "{err}");

        // 深度超限：a/b/c/d/e.md = 5 层。
        let tmp2 = tempfile::tempdir().unwrap();
        let deep = write_skill(
            tmp2.path().join("deep").as_path(),
            "---\nname: deep\n---\nbody",
            &[("a/b/c/d/e.md", "x")],
        );
        let err = read_local_skill_bundle(&deep, "deep", "claude-acp").unwrap_err();
        assert!(err.contains("depth exceeds 4"), "{err}");

        // 单文件超限：2MiB 支撑文件。
        let tmp3 = tempfile::tempdir().unwrap();
        let big = vec![0u8; (IMPORT_MAX_FILE_BYTES + 1) as usize];
        let dir3 = tmp3.path().join("bigfile");
        fs::create_dir_all(&dir3).unwrap();
        fs::write(dir3.join("SKILL.md"), "---\nname: b\n---\nx").unwrap();
        fs::write(dir3.join("big.bin"), &big).unwrap();
        let dir3 = camino::Utf8PathBuf::from_path_buf(dir3).unwrap();
        let err = read_local_skill_bundle(&dir3, "bigfile", "claude-acp").unwrap_err();
        assert!(err.contains("1MiB limit"), "{err}");

        // 深度 4 合法边界：a/b/c/d.md 恰好 4 层，通过。
        let tmp4 = tempfile::tempdir().unwrap();
        let ok = write_skill(
            tmp4.path().join("edge").as_path(),
            "---\nname: e\n---\nbody",
            &[("a/b/c/d.md", "x")],
        );
        assert!(read_local_skill_bundle(&ok, "edge", "claude-acp").is_ok());
    }

    #[test]
    fn bundle_fails_on_non_utf8_support_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("binary");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nname: b\n---\nx").unwrap();
        fs::write(
            dir.join("icon.png"),
            [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        )
        .unwrap();
        let dir = camino::Utf8PathBuf::from_path_buf(dir).unwrap();
        // 二进制文件无法经文本契约透传 → 整包 failed 并列明路径（不静默丢失）。
        let err = read_local_skill_bundle(&dir, "binary", "claude-acp").unwrap_err();
        assert!(err.contains("non-utf8"), "{err}");
        assert!(err.contains("icon.png"), "{err}");
    }

    #[test]
    fn skill_key_validation_rejects_traversal() {
        // 服务端回传的 skill_key 是路径输入：分隔符 / `..` / `.` / 空一律拒绝（not found 语义）。
        for bad in ["", "../escape", "a/b", "a\\b", "..", "."] {
            let err = resolve_own_skill_dir(bad).unwrap_err();
            assert!(err.contains("skill not found"), "{bad}: {err}");
        }
        // 库内不存在的目录名同样按 not found 拒绝。
        assert!(resolve_own_skill_dir("definitely-not-exist-dir").is_err());
    }

    #[test]
    fn count_skill_files_counts_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("s");
        write_skill(&dir, "x", &[("a/b.md", "y"), ("c.md", "z")]);
        assert_eq!(count_skill_files(&dir), 3); // SKILL.md + 2 支撑文件
        assert_eq!(count_skill_files(tmp.path().join("absent").as_path()), 0);
    }
}
