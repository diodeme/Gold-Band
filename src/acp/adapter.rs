use std::process::{Child, Stdio};

use anyhow::{Context, Result, ensure};

use crate::config::AcpAdapterConfig;
use crate::process::background_command;

#[derive(Debug, Clone)]
pub struct ResolvedAcpAdapter {
    pub adapter_id: String,
    pub display_name: String,
    pub command: String,
    pub args: Vec<String>,
}

pub fn resolve_adapter(config: &AcpAdapterConfig) -> Result<ResolvedAcpAdapter> {
    ensure!(
        !config.command.trim().is_empty(),
        "ACP adapter command cannot be empty"
    );
    Ok(ResolvedAcpAdapter {
        adapter_id: config.command.clone(),
        display_name: config.display_name.clone(),
        command: config.command.clone(),
        args: normalize_args(&config.args),
    })
}

pub fn spawn_adapter(
    config: &AcpAdapterConfig,
    cwd: &std::path::Path,
) -> Result<(ResolvedAcpAdapter, Child)> {
    let adapter = resolve_adapter(config)?;
    let executable = platform_adapter_command(&adapter.command);
    let mut command = background_command(&executable);
    command
        .args(&adapter.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &config.env {
        command.env(key, value);
    }
    let child = command
        .spawn()
        .with_context(|| format!("failed to start ACP adapter `{}`", executable))?;
    Ok((adapter, child))
}

fn normalize_args(args: &[String]) -> Vec<String> {
    args.iter()
        .flat_map(|arg| arg.split_whitespace().map(str::to_string))
        .collect()
}

#[cfg(windows)]
fn platform_adapter_command(command: &str) -> String {
    if command.eq_ignore_ascii_case("npx") {
        "npx.cmd".to_string()
    } else {
        command.to_string()
    }
}

#[cfg(not(windows))]
fn platform_adapter_command(command: &str) -> String {
    command.to_string()
}
