use anyhow::{Result, bail};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{name}.exe"));
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
            let candidate_cmd = dir.join(format!("{name}.cmd"));
            if candidate_cmd.is_file() {
                return Some(candidate_cmd);
            }
        }
    }
    None
}

pub fn background_command(program: impl AsRef<OsStr>) -> ProcessCommand {
    let mut command = ProcessCommand::new(program);
    apply_background_process_flags(&mut command);
    command
}

pub fn apply_background_process_flags(command: &mut ProcessCommand) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
}

pub fn kill_process_tree(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let status = background_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("failed to kill provider process tree for pid {pid}");
        }
    }
    #[cfg(not(windows))]
    {
        let status = background_command("kill")
            .args(["-TERM", &pid.to_string()])
            .status()?;
        if !status.success() {
            bail!("failed to kill provider process for pid {pid}");
        }
    }
    Ok(())
}
