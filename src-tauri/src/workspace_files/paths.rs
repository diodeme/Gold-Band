use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

use crate::commands::{CommandErrorVm, CommandResult};
use crate::conversation_workspace::workspace_entry_for_project;
use crate::state::DesktopState;

use super::models::{FileTargetLocationVm, WorkspaceFileLocatorVm};

#[derive(Clone)]
pub(crate) struct ResolvedWorkspaceRoot {
    pub project_id: String,
    pub path: PathBuf,
    pub config: gold_band::config::WorkspaceFilesConfig,
}

pub(crate) fn error(code: &str, params: serde_json::Value) -> CommandErrorVm {
    CommandErrorVm::new(code, params)
}

pub(crate) fn resolve_workspace_root(
    state: &DesktopState,
    project_id: &str,
) -> CommandResult<ResolvedWorkspaceRoot> {
    let context = state.context().map_err(|_| {
        error(
            "workspace-file.project-not-found",
            serde_json::json!({ "projectId": project_id }),
        )
    })?;
    let persisted = context.app().load_state().map_err(|_| {
        error(
            "workspace-file.project-not-found",
            serde_json::json!({ "projectId": project_id }),
        )
    })?;
    let (workspace_path, resolved_project_id) = workspace_entry_for_project(&persisted, project_id)
        .ok_or_else(|| {
            error(
                "workspace-file.project-not-found",
                serde_json::json!({ "projectId": project_id }),
            )
        })?;
    let canonical = std::fs::canonicalize(&workspace_path).map_err(|_| {
        error(
            "workspace-file.project-not-found",
            serde_json::json!({ "projectId": project_id }),
        )
    })?;
    Ok(ResolvedWorkspaceRoot {
        project_id: resolved_project_id,
        path: canonical,
        config: context.config.workspace_files,
    })
}

pub(crate) fn resolve_workspace_relative_path(
    root: &ResolvedWorkspaceRoot,
    relative_path: &str,
) -> CommandResult<PathBuf> {
    let relative = Path::new(relative_path);
    if relative_path.contains('\0')
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": relative_path }),
        ));
    }
    let candidate = if relative_path.trim().is_empty() {
        root.path.clone()
    } else {
        root.path.join(relative)
    };
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|io_error| io_path_error(io_error, &candidate, "read"))?;
    if !path_is_within(&canonical, &root.path) {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": relative_path }),
        ));
    }
    Ok(canonical)
}

pub(crate) fn canonicalize_file(path: &Path, operation: &str) -> CommandResult<PathBuf> {
    let canonical =
        std::fs::canonicalize(path).map_err(|io_error| io_path_error(io_error, path, operation))?;
    if !canonical.is_file() {
        return Err(error(
            "workspace-file.not-a-file",
            serde_json::json!({ "path": display_path(&canonical) }),
        ));
    }
    Ok(canonical)
}

pub(crate) fn locator_for_path(
    root: &ResolvedWorkspaceRoot,
    canonical_path: &Path,
) -> WorkspaceFileLocatorVm {
    let in_workspace = path_is_within(canonical_path, &root.path);
    WorkspaceFileLocatorVm {
        project_id: root.project_id.clone(),
        canonical_path: display_path(canonical_path),
        relative_path: in_workspace.then(|| relative_display(canonical_path, &root.path)),
        scope: if in_workspace {
            "workspace"
        } else {
            "external"
        }
        .to_string(),
    }
}

pub(crate) fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = comparable_path(path);
    let root = comparable_path(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn relative_display(path: &Path, root: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return slash_path(relative);
    }
    let root_components = root.components().count();
    let relative = path.components().skip(root_components).collect::<PathBuf>();
    slash_path(&relative)
}

pub(crate) fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(network_path) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{network_path}");
    }
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(&value)
        .to_string()
}

pub(crate) fn slash_path(path: &Path) -> String {
    display_path(path).replace('\\', "/")
}

pub(crate) fn parse_file_link(
    root: &ResolvedWorkspaceRoot,
    raw_href: &str,
) -> CommandResult<(PathBuf, Option<FileTargetLocationVm>)> {
    let trimmed = raw_href
        .trim()
        .trim_matches(|character| character == '<' || character == '>');
    if trimmed.is_empty() {
        return Err(error(
            "workspace-file.path-invalid",
            serde_json::json!({ "path": raw_href }),
        ));
    }

    let (without_fragment, fragment_target) = split_line_fragment(trimmed);
    let (path_part, suffix_target) = split_line_suffix(without_fragment);
    let target = fragment_target.or(suffix_target);

    let decoded = if looks_like_windows_absolute(path_part) {
        percent_decode(path_part)?
    } else if let Ok(url) = Url::parse(path_part) {
        if url.scheme() != "file" {
            return Err(error(
                "workspace-file.path-invalid",
                serde_json::json!({ "path": raw_href }),
            ));
        }
        url.to_file_path()
            .map_err(|_| {
                error(
                    "workspace-file.path-invalid",
                    serde_json::json!({ "path": raw_href }),
                )
            })?
            .to_string_lossy()
            .into_owned()
    } else {
        percent_decode(path_part)?
    };

    let path = PathBuf::from(decoded);
    if !path.is_absolute()
        && path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error(
            "workspace-file.path-outside-workspace",
            serde_json::json!({ "path": raw_href }),
        ));
    }
    let candidate = if path.is_absolute() {
        path
    } else {
        root.path.join(path)
    };
    let canonical = canonicalize_file(&candidate, "read")?;
    Ok((canonical, target))
}

fn split_line_fragment(input: &str) -> (&str, Option<FileTargetLocationVm>) {
    let Some((path, fragment)) = input.rsplit_once('#') else {
        return (input, None);
    };
    let Some(rest) = fragment
        .strip_prefix('L')
        .or_else(|| fragment.strip_prefix('l'))
    else {
        return (input, None);
    };
    let (line, end_line) = rest
        .split_once('-')
        .map(|(line, end)| (line, end.trim_start_matches(['L', 'l'])))
        .unwrap_or((rest, ""));
    let Ok(line) = line.parse::<u32>() else {
        return (input, None);
    };
    if line == 0 {
        return (input, None);
    }
    let end_line = end_line.parse::<u32>().ok().filter(|value| *value >= line);
    (
        path,
        Some(FileTargetLocationVm {
            line: Some(line),
            column: None,
            end_line,
        }),
    )
}

fn split_line_suffix(input: &str) -> (&str, Option<FileTargetLocationVm>) {
    let Some((without_last, last)) = trailing_number(input) else {
        return (input, None);
    };
    if let Some((without_line, line)) = trailing_number(without_last) {
        return (
            without_line,
            Some(FileTargetLocationVm {
                line: Some(line),
                column: Some(last),
                end_line: None,
            }),
        );
    }
    (
        without_last,
        Some(FileTargetLocationVm {
            line: Some(last),
            column: None,
            end_line: None,
        }),
    )
}

fn trailing_number(input: &str) -> Option<(&str, u32)> {
    let (prefix, suffix) = input.rsplit_once(':')?;
    let value = suffix.parse::<u32>().ok()?;
    (value > 0).then_some((prefix, value))
}

fn looks_like_windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || value.starts_with("\\\\")
}

fn percent_decode(value: &str) -> CommandResult<String> {
    percent_decode_str(value)
        .decode_utf8()
        .map(Cow::into_owned)
        .map_err(|_| {
            error(
                "workspace-file.path-invalid",
                serde_json::json!({ "path": value }),
            )
        })
}

fn comparable_path(path: &Path) -> String {
    let normalized = slash_path(path).trim_end_matches('/').to_string();
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

pub(crate) fn io_path_error(
    io_error: std::io::Error,
    path: &Path,
    operation: &str,
) -> CommandErrorVm {
    let code = match io_error.kind() {
        std::io::ErrorKind::NotFound => "workspace-file.not-found",
        std::io::ErrorKind::PermissionDenied => "workspace-file.permission-denied",
        _ if operation == "write" => "workspace-file.write-failed",
        _ => "workspace-file.read-failed",
    };
    error(
        code,
        serde_json::json!({
            "path": display_path(path),
            "operation": operation,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn root(path: &Path) -> ResolvedWorkspaceRoot {
        ResolvedWorkspaceRoot {
            project_id: "project-1".to_string(),
            path: std::fs::canonicalize(path).unwrap(),
            config: gold_band::config::WorkspaceFilesConfig::default(),
        }
    }

    #[test]
    fn parses_line_and_column_suffix_without_consuming_windows_drive() {
        let (path, target) = split_line_suffix("D:/repo/src/client.rs:2727:8");
        assert_eq!(path, "D:/repo/src/client.rs");
        let target = target.unwrap();
        assert_eq!(target.line, Some(2727));
        assert_eq!(target.column, Some(8));
    }

    #[test]
    fn parses_line_fragment_range() {
        let (path, target) = split_line_fragment("file:///D:/repo/client.rs#L10-L20");
        assert_eq!(path, "file:///D:/repo/client.rs");
        let target = target.unwrap();
        assert_eq!(target.line, Some(10));
        assert_eq!(target.end_line, Some(20));
    }

    #[test]
    fn relative_path_uses_forward_slashes() {
        let root = Path::new("root");
        let path = root.join("src").join("main.rs");
        assert_eq!(relative_display(&path, root), "src/main.rs");
    }

    #[test]
    fn display_path_removes_windows_extended_length_prefixes() {
        assert_eq!(
            display_path(Path::new(r"\\?\D:\repo\README.md")),
            r"D:\repo\README.md"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\README.md")),
            r"\\server\share\README.md"
        );
    }

    #[test]
    fn parses_url_encoded_relative_file_and_target() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("space dir")).unwrap();
        let path = dir.path().join("space dir").join("你好.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        let workspace = root(dir.path());

        let (resolved, target) =
            parse_file_link(&workspace, "space%20dir/%E4%BD%A0%E5%A5%BD.rs:12:4").unwrap();
        assert_eq!(resolved, std::fs::canonicalize(path).unwrap());
        let target = target.unwrap();
        assert_eq!(target.line, Some(12));
        assert_eq!(target.column, Some(4));
    }

    #[test]
    fn rejects_relative_parent_traversal() {
        let workspace_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let outside = outside_dir.path().join("outside-workspace-file.txt");
        std::fs::write(&outside, "outside").unwrap();
        let workspace = root(workspace_dir.path());

        let outside_name = outside_dir.path().file_name().unwrap().to_string_lossy();
        let href = format!("../{outside_name}/outside-workspace-file.txt");
        let result = parse_file_link(&workspace, &href);
        assert_eq!(
            result.unwrap_err().code,
            "workspace-file.path-outside-workspace"
        );
    }

    #[test]
    fn locator_classifies_an_absolute_file_outside_the_workspace() {
        let workspace_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let path = outside_dir.path().join("outside.txt");
        std::fs::write(&path, "outside").unwrap();
        let workspace = root(workspace_dir.path());
        let canonical = std::fs::canonicalize(path).unwrap();

        let locator = locator_for_path(&workspace, &canonical);
        assert_eq!(locator.scope, "external");
        assert_eq!(locator.relative_path, None);
    }

    #[test]
    fn parses_file_url_with_line_fragment() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("client.rs");
        std::fs::write(&path, "fn main() {}").unwrap();
        let workspace = root(dir.path());
        let href = format!("{}#L10-L20", Url::from_file_path(&path).unwrap());

        let (resolved, target) = parse_file_link(&workspace, &href).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(path).unwrap());
        let target = target.unwrap();
        assert_eq!(target.line, Some(10));
        assert_eq!(target.end_line, Some(20));
    }
}
