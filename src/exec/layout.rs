use camino::{Utf8Path, Utf8PathBuf};

pub(crate) fn commands_dir(attempt_dir: &Utf8Path) -> Utf8PathBuf {
    attempt_dir.join("commands")
}

pub(crate) fn command_dir(attempt_dir: &Utf8Path, index: usize, command_id: &str) -> Utf8PathBuf {
    commands_dir(attempt_dir).join(format!("{:02}-{}", index + 1, command_id))
}

pub(crate) fn command_json_path(attempt_dir: &Utf8Path, index: usize, command_id: &str) -> Utf8PathBuf {
    command_dir(attempt_dir, index, command_id).join("command.json")
}

pub(crate) fn stdout_log_path(attempt_dir: &Utf8Path, index: usize, command_id: &str) -> Utf8PathBuf {
    command_dir(attempt_dir, index, command_id).join("stdout.log")
}

pub(crate) fn stderr_log_path(attempt_dir: &Utf8Path, index: usize, command_id: &str) -> Utf8PathBuf {
    command_dir(attempt_dir, index, command_id).join("stderr.log")
}

pub(crate) fn stdout_rel_path(index: usize, command_id: &str) -> String {
    format!("commands/{:02}-{}/stdout.log", index + 1, command_id)
}

pub(crate) fn stderr_rel_path(index: usize, command_id: &str) -> String {
    format!("commands/{:02}-{}/stderr.log", index + 1, command_id)
}
