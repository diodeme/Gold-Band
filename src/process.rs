use anyhow::{Result, bail};
use std::process::{Command as ProcessCommand, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn kill_process_tree(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let status = ProcessCommand::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("failed to kill provider process tree for pid {pid}");
        }
    }
    #[cfg(not(windows))]
    {
        let status = ProcessCommand::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()?;
        if !status.success() {
            bail!("failed to kill provider process for pid {pid}");
        }
    }
    Ok(())
}
