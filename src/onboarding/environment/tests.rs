use super::*;
use std::sync::Mutex;

struct FixtureRunner {
    results: Vec<(EnvironmentTool, Option<ProbeOutput>)>,
    failure: Option<(EnvironmentTool, EnvironmentStatus)>,
    minimum_timeouts: Vec<(EnvironmentTool, Duration)>,
    node_launcher: PathBuf,
    calls: Mutex<Vec<EnvironmentTool>>,
    timeouts: Mutex<Vec<(EnvironmentTool, Duration)>>,
}

impl FixtureRunner {
    fn healthy() -> Self {
        let output = |tool: EnvironmentTool| {
            let is_node = tool == EnvironmentTool::Node;
            let version = if is_node { "22.22.3" } else { "10.9.8" };
            ProbeOutput {
                stdout: version.into(),
                observations: vec![RuntimeObservation {
                    node_version: "22.22.3".into(),
                    node_path: PathBuf::from("node"),
                    script_path: if is_node {
                        None
                    } else {
                        Some(PathBuf::from(if tool == EnvironmentTool::Npm {
                            "npm-cli.js"
                        } else {
                            "npx-cli.js"
                        }))
                    },
                    npm: if is_node {
                        None
                    } else {
                        Some(NpmObservation {
                            root: PathBuf::from("npm-install"),
                            version: version.into(),
                            node_range: Some("^18.17.0 || >=20.5.0".into()),
                            node_compatible: true,
                        })
                    },
                }],
            }
        };
        Self {
            results: vec![
                (
                    EnvironmentTool::Git,
                    Some(ProbeOutput {
                        stdout: "git version 2.49.0.windows.1".into(),
                        observations: vec![],
                    }),
                ),
                (EnvironmentTool::Node, Some(output(EnvironmentTool::Node))),
                (EnvironmentTool::Npm, Some(output(EnvironmentTool::Npm))),
                (EnvironmentTool::Npx, Some(output(EnvironmentTool::Npx))),
            ],
            failure: None,
            minimum_timeouts: vec![],
            node_launcher: PathBuf::from("node"),
            calls: Mutex::new(vec![]),
            timeouts: Mutex::new(vec![]),
        }
    }
    fn output(&mut self, tool: EnvironmentTool) -> &mut ProbeOutput {
        self.results
            .iter_mut()
            .find(|(t, _)| *t == tool)
            .unwrap()
            .1
            .as_mut()
            .unwrap()
    }
    fn report(&self) -> EnvironmentReport {
        check_environment(self, environment_policy(), &AtomicBool::new(false), |_| {})
    }
}

impl EnvironmentRunner for FixtureRunner {
    fn resolve(&self, tool: EnvironmentTool) -> Option<PathBuf> {
        self.results
            .iter()
            .find(|(t, _)| *t == tool)
            .unwrap()
            .1
            .as_ref()
            .map(|_| {
                if tool == EnvironmentTool::Node {
                    self.node_launcher.clone()
                } else {
                    PathBuf::from(tool.command())
                }
            })
    }
    fn probe(
        &self,
        tool: EnvironmentTool,
        _: &Path,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<ProbeOutput, EnvironmentStatus> {
        self.calls.lock().unwrap().push(tool);
        self.timeouts.lock().unwrap().push((tool, timeout));
        if cancelled.load(Ordering::Acquire) {
            return Err(EnvironmentStatus::Cancelled);
        }
        if let Some((failed, status)) = self.failure
            && failed == tool
        {
            return Err(status);
        }
        if self
            .minimum_timeouts
            .iter()
            .any(|(candidate, minimum)| *candidate == tool && timeout < *minimum)
        {
            return Err(EnvironmentStatus::Timeout);
        }
        Ok(self
            .results
            .iter()
            .find(|(t, _)| *t == tool)
            .unwrap()
            .1
            .clone()
            .unwrap())
    }
}

/// Windows npm/npx launchers start node and resolve the npm package before printing a
/// version, so their cold start can exceed a native `--version`. Keep a separate,
/// larger but still bounded budget instead of relaxing the native tools.
#[test]
fn javascript_launchers_receive_a_cold_start_budget_separate_from_native_tools() {
    let mut runner = FixtureRunner::healthy();
    runner.minimum_timeouts = vec![
        (EnvironmentTool::Npm, Duration::from_secs(10)),
        (EnvironmentTool::Npx, Duration::from_secs(10)),
    ];

    let report = runner.report();

    assert!(report.runtime_ready(), "{:#?}", report.checks);
    let policy = environment_policy();
    assert!(
        policy.probe_timeouts.package_manager_command_ms > policy.probe_timeouts.native_command_ms
    );
    let timeouts = runner.timeouts.lock().unwrap();
    let timeout_of = |tool| {
        timeouts
            .iter()
            .find(|(candidate, _)| *candidate == tool)
            .unwrap()
            .1
    };
    for tool in [EnvironmentTool::Git, EnvironmentTool::Node] {
        assert_eq!(
            timeout_of(tool),
            Duration::from_millis(policy.probe_timeouts.native_command_ms),
            "{tool:?}"
        );
    }
    for tool in [EnvironmentTool::Npm, EnvironmentTool::Npx] {
        assert_eq!(
            timeout_of(tool),
            Duration::from_millis(policy.probe_timeouts.package_manager_command_ms),
            "{tool:?}"
        );
    }
}

#[test]
fn complete_local_toolchain_is_ready_without_repository_or_network() {
    let runner = FixtureRunner::healthy();
    let report = runner.report();
    assert!(report.runtime_ready());
    assert!(
        report
            .checks
            .iter()
            .all(|c| c.status == EnvironmentStatus::Ready)
    );
    assert_eq!(runner.calls.lock().unwrap().len(), 4);
}

#[test]
fn git_missing_does_not_bypass_or_block_the_mandatory_runtime_gate() {
    let mut runner = FixtureRunner::healthy();
    runner.results[0].1 = None;
    let report = runner.report();
    assert!(report.runtime_ready());
    assert_eq!(report.checks[0].status, EnvironmentStatus::Missing);
    for tool in [
        EnvironmentTool::Node,
        EnvironmentTool::Npm,
        EnvironmentTool::Npx,
    ] {
        let mut runner = FixtureRunner::healthy();
        runner
            .results
            .iter_mut()
            .find(|(t, _)| *t == tool)
            .unwrap()
            .1 = None;
        assert!(!runner.report().runtime_ready());
    }
}

#[test]
fn node_and_npm_boundaries_reject_unknown_majors_and_prereleases() {
    for (version, expected) in [
        ("22.19.9", false),
        ("22.20.0", true),
        ("23.0.0", false),
        ("24.0.0", true),
        ("25.0.0", false),
        ("24.0.0-rc.1", false),
    ] {
        assert_eq!(
            matches_policy(
                &Version::parse(version).unwrap(),
                &environment_policy().node.ranges
            ),
            expected,
            "{version}"
        );
    }
    for (version, expected) in [
        ("10.8.9", false),
        ("10.9.0", true),
        ("11.0.0", true),
        ("12.0.0", true),
        ("12.2.1", true),
        ("11.0.0-rc.1", false),
        ("12.0.0-rc.1", false),
        ("13.0.0", false),
    ] {
        assert_eq!(
            matches_policy(
                &Version::parse(version).unwrap(),
                &environment_policy().npm.ranges
            ),
            expected,
            "{version}"
        );
    }
}

#[test]
fn each_failed_tool_keeps_its_reason_without_masking_other_successes() {
    for status in [
        EnvironmentStatus::Failed,
        EnvironmentStatus::Timeout,
        EnvironmentStatus::Cancelled,
    ] {
        let mut runner = FixtureRunner::healthy();
        runner.failure = Some((EnvironmentTool::Npx, status));
        let report = runner.report();
        assert!(!report.runtime_ready());
        assert_eq!(report.checks[3].status, status);
        assert!(
            report.checks[..3]
                .iter()
                .all(|check| check.status == EnvironmentStatus::Ready)
        );
        assert_eq!(
            report.checks[3].error.as_ref().unwrap().params["tool"],
            "npx"
        );
    }
}

#[test]
fn newer_unverified_major_is_reported_separately_from_unsupported() {
    for (tool, newer, older) in [
        (EnvironmentTool::Node, "25.0.0", "20.19.0"),
        (EnvironmentTool::Npm, "13.0.0", "9.9.0"),
        (EnvironmentTool::Npx, "13.0.0", "9.9.0"),
    ] {
        let mut runner = FixtureRunner::healthy();
        runner.output(tool).stdout = newer.into();
        if tool == EnvironmentTool::Node {
            runner.output(tool).observations[0].node_version = newer.into();
        } else {
            runner.output(tool).observations[0]
                .npm
                .as_mut()
                .unwrap()
                .version = newer.into();
            if tool == EnvironmentTool::Npx {
                runner.output(EnvironmentTool::Npm).stdout = newer.into();
                runner.output(EnvironmentTool::Npm).observations[0]
                    .npm
                    .as_mut()
                    .unwrap()
                    .version = newer.into();
            }
        }
        let check = runner
            .report()
            .checks
            .into_iter()
            .find(|check| check.tool == tool)
            .unwrap();
        assert_eq!(
            check.status,
            EnvironmentStatus::Unverified,
            "{tool:?} {newer}"
        );
        assert_eq!(
            check.error.as_ref().unwrap().code,
            "setup.runtime-version-unverified"
        );
        assert_eq!(
            serde_json::to_value(&check).unwrap()["status"],
            "unverified"
        );

        let mut runner = FixtureRunner::healthy();
        runner.output(tool).stdout = older.into();
        if tool == EnvironmentTool::Node {
            runner.output(tool).observations[0].node_version = older.into();
        } else {
            runner.output(tool).observations[0]
                .npm
                .as_mut()
                .unwrap()
                .version = older.into();
            if tool == EnvironmentTool::Npx {
                runner.output(EnvironmentTool::Npm).stdout = older.into();
                runner.output(EnvironmentTool::Npm).observations[0]
                    .npm
                    .as_mut()
                    .unwrap()
                    .version = older.into();
            }
        }
        let check = runner
            .report()
            .checks
            .into_iter()
            .find(|check| check.tool == tool)
            .unwrap();
        assert_eq!(
            check.status,
            EnvironmentStatus::Unsupported,
            "{tool:?} {older}"
        );
        assert_eq!(
            check.error.as_ref().unwrap().code,
            "setup.runtime-version-unsupported"
        );
    }
}

#[test]
fn stable_unverified_node_can_continue_when_the_actual_npm_toolchain_is_compatible() {
    let mut runner = FixtureRunner::healthy();
    runner.output(EnvironmentTool::Node).stdout = "23.11.0".into();
    runner.output(EnvironmentTool::Node).observations[0].node_version = "23.11.0".into();
    for tool in [EnvironmentTool::Npm, EnvironmentTool::Npx] {
        runner.output(tool).observations[0].node_version = "23.11.0".into();
    }

    let report = runner.report();

    assert_eq!(
        report
            .checks
            .iter()
            .find(|check| check.tool == EnvironmentTool::Node)
            .unwrap()
            .status,
        EnvironmentStatus::Unverified
    );
    assert!(report.runtime_ready(), "{:#?}", report.checks);
}

#[test]
fn node_launcher_shim_may_resolve_to_a_different_actual_runtime_path() {
    let mut runner = FixtureRunner::healthy();
    runner.node_launcher = PathBuf::from(r"C:\Users\example\.local\bin\node.cmd");
    for tool in [
        EnvironmentTool::Node,
        EnvironmentTool::Npm,
        EnvironmentTool::Npx,
    ] {
        let observation = &mut runner.output(tool).observations[0];
        observation.node_version = "25.5.0".into();
        observation.node_path = PathBuf::from(r"D:\dev\node-v25.5.0-win-x64\node.exe");
    }
    runner.output(EnvironmentTool::Node).stdout = "25.5.0".into();

    let report = runner.report();
    let node = report
        .checks
        .iter()
        .find(|check| check.tool == EnvironmentTool::Node)
        .unwrap();

    assert_eq!(node.status, EnvironmentStatus::Unverified);
    assert_eq!(
        node.executable_path.as_deref(),
        Some(Path::new(r"C:\Users\example\.local\bin\node.cmd"))
    );
    assert_eq!(
        node.node_executable_path.as_deref(),
        Some(Path::new(r"D:\dev\node-v25.5.0-win-x64\node.exe"))
    );
    assert!(report.runtime_ready(), "{:#?}", report.checks);
}

#[test]
fn stable_unverified_npm_and_npx_can_continue_when_the_package_identity_matches() {
    let mut runner = FixtureRunner::healthy();
    for tool in [EnvironmentTool::Npm, EnvironmentTool::Npx] {
        runner.output(tool).stdout = "13.0.0".into();
        runner.output(tool).observations[0]
            .npm
            .as_mut()
            .unwrap()
            .version = "13.0.0".into();
    }

    let report = runner.report();

    for tool in [EnvironmentTool::Npm, EnvironmentTool::Npx] {
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.tool == tool)
                .unwrap()
                .status,
            EnvironmentStatus::Unverified,
            "{tool:?}"
        );
    }
    assert!(report.runtime_ready(), "{:#?}", report.checks);
}

#[test]
fn policy_verified_majors_cover_every_range_lower_bound() {
    let policy = environment_policy();
    for (ranges, verified_majors) in [
        (&policy.node.ranges, &policy.node.verified_majors),
        (&policy.npm.ranges, &policy.npm.verified_majors),
    ] {
        for range in ranges {
            let lower = range
                .split_whitespace()
                .find(|part| part.starts_with(">="))
                .expect("policy range has an inclusive lower bound");
            let version = Version::parse(lower.trim_start_matches(">=")).unwrap();
            assert!(
                verified_majors.contains(&version.major),
                "{range} lower-bound major must be listed in verifiedMajors"
            );
        }
    }
}

#[test]
fn independently_valid_versions_do_not_hide_different_node_installations() {
    let mut runner = FixtureRunner::healthy();
    runner.output(EnvironmentTool::Npm).observations[0].node_path = "second-node".into();
    let report = runner.report();
    assert!(!report.runtime_ready());
    assert_eq!(report.checks[2].status, EnvironmentStatus::Incompatible);
    let error = report.checks[2].error.as_ref().unwrap();
    assert_eq!(error.params["reason"], "node-mismatch");
    assert_eq!(error.params["expectedNodePath"], "node");
    assert_eq!(error.params["actualNodePath"], "second-node");
}

#[test]
fn actual_npm_engines_npx_package_identity_and_observation_are_required() {
    for scenario in 0..5 {
        let mut runner = FixtureRunner::healthy();
        let output = runner.output(EnvironmentTool::Npx);
        match scenario {
            0 => output.observations[0].npm.as_mut().unwrap().node_compatible = false,
            1 => output.observations[0].npm.as_mut().unwrap().root = "other-npm".into(),
            2 => output.observations[0].npm.as_mut().unwrap().version = "10.9.9".into(),
            3 => output.observations.clear(),
            _ => output.observations[0].node_version = "22.20.0".into(),
        }
        let report = runner.report();
        assert!(!report.runtime_ready(), "scenario {scenario}");
        assert_eq!(report.checks[3].status, EnvironmentStatus::Incompatible);
        assert_eq!(
            report.checks[3].error.as_ref().unwrap().params["reason"],
            [
                "npm-engines-incompatible",
                "npm-package-mismatch",
                "npm-version-mismatch",
                "probe-unavailable",
                "node-version-mismatch",
            ][scenario]
        );
    }
}

#[test]
fn git_vendor_suffix_and_minimum_use_the_existing_git_parser() {
    for (version, status) in [
        (
            "git version 2.35.9.windows.1",
            EnvironmentStatus::Unsupported,
        ),
        ("git version 2.36.0.windows.1", EnvironmentStatus::Ready),
        ("git version 2.49.0-rc1", EnvironmentStatus::Unsupported),
        ("unknown", EnvironmentStatus::Failed),
    ] {
        let mut runner = FixtureRunner::healthy();
        runner.output(EnvironmentTool::Git).stdout = version.into();
        assert_eq!(runner.report().checks[0].status, status);
    }
}

#[test]
fn cancellation_is_distinct_and_all_tool_results_are_published_once() {
    let calls = Mutex::new(Vec::new());
    let report = check_environment(
        &FixtureRunner::healthy(),
        environment_policy(),
        &AtomicBool::new(true),
        |check| calls.lock().unwrap().push(check.tool),
    );
    assert!(!report.runtime_ready());
    assert!(
        report
            .checks
            .iter()
            .all(|check| check.status == EnvironmentStatus::Cancelled)
    );
    assert_eq!(calls.lock().unwrap().len(), 4);
}

#[test]
fn npx_launchability_only_requires_npx_to_start() {
    let healthy = FixtureRunner::healthy().report();
    assert!(healthy.npx_launchable);

    let mut npm_missing = FixtureRunner::healthy();
    npm_missing.failure = Some((EnvironmentTool::Npm, EnvironmentStatus::Missing));
    let report = npm_missing.report();
    assert!(report.npx_launchable);
    assert!(!report.runtime_ready());

    let mut node_missing = FixtureRunner::healthy();
    node_missing.failure = Some((EnvironmentTool::Node, EnvironmentStatus::Missing));
    let report = node_missing.report();
    assert!(report.npx_launchable, "{:#?}", report.checks);
    assert!(!report.runtime_ready());

    let mut old_npx = FixtureRunner::healthy();
    old_npx.output(EnvironmentTool::Npx).stdout = "9.0.0".into();
    let report = old_npx.report();
    let npx = report
        .checks
        .iter()
        .find(|check| check.tool == EnvironmentTool::Npx)
        .unwrap();
    assert_eq!(npx.status, EnvironmentStatus::Unsupported);
    assert!(report.npx_launchable);

    for status in [
        EnvironmentStatus::Missing,
        EnvironmentStatus::Failed,
        EnvironmentStatus::Timeout,
    ] {
        let mut runner = FixtureRunner::healthy();
        runner.failure = Some((EnvironmentTool::Npx, status));
        assert!(!runner.report().npx_launchable, "{status:?}");
    }
}

#[test]
fn npm_and_npx_report_the_node_version_their_process_actually_used() {
    let mut runner = FixtureRunner::healthy();
    let observation = &mut runner.output(EnvironmentTool::Npx).observations[0];
    observation.node_version = "22.10.0".into();

    let report = runner.report();
    let npx = report
        .checks
        .iter()
        .find(|check| check.tool == EnvironmentTool::Npx)
        .unwrap();

    assert_eq!(npx.node_version.as_deref(), Some("22.10.0"));
    assert_eq!(npx.status, EnvironmentStatus::Incompatible);
    assert!(report.npx_launchable);
}

#[cfg(windows)]
#[test]
fn windows_path_refresh_and_npm_shims_preserve_chinese_and_space_paths() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("旧 Node");
    let new = root.path().join("新 Node");
    for directory in [&old, &new] {
        fs::create_dir(directory).unwrap();
        for file in ["node.exe", "npm.cmd", "npx.cmd"] {
            fs::write(directory.join(file), "fixture").unwrap();
        }
        fs::write(directory.join("npm"), "#!/bin/sh").unwrap();
    }
    let first = LocalEnvironmentRunner::with_path(std::env::join_paths([&old]).unwrap());
    let refreshed = LocalEnvironmentRunner::with_path(std::env::join_paths([&new, &old]).unwrap());
    assert_eq!(
        first.resolve(EnvironmentTool::Node),
        Some(old.join("node.exe"))
    );
    assert_eq!(
        refreshed.resolve(EnvironmentTool::Node),
        Some(new.join("node.exe"))
    );
    assert_eq!(
        refreshed.resolve(EnvironmentTool::Npm),
        Some(new.join("npm.cmd"))
    );
    assert_eq!(
        refreshed.resolve(EnvironmentTool::Npx),
        Some(new.join("npx.cmd"))
    );
}

#[cfg(windows)]
#[test]
fn hidden_process_probe_handles_space_paths_timeout_and_cancel() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("环境 fixture");
    fs::create_dir(&directory).unwrap();
    let script = directory.join("git.cmd");
    fs::write(
        &script,
        "@echo off\r\necho git version 2.49.0.windows.1\r\n",
    )
    .unwrap();
    let runner = LocalEnvironmentRunner::with_path(std::env::join_paths([&directory]).unwrap());
    let output = runner
        .probe(
            EnvironmentTool::Git,
            &script,
            Duration::from_secs(3),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(output.stdout.contains("git version 2.49.0"));
    fs::write(&script, "@echo off\r\n:again\r\ngoto again\r\n").unwrap();
    assert_eq!(
        runner
            .probe(
                EnvironmentTool::Git,
                &script,
                Duration::from_millis(80),
                &AtomicBool::new(false)
            )
            .unwrap_err(),
        EnvironmentStatus::Timeout
    );
    assert_eq!(
        runner
            .probe(
                EnvironmentTool::Git,
                &script,
                Duration::from_secs(3),
                &AtomicBool::new(true)
            )
            .unwrap_err(),
        EnvironmentStatus::Cancelled
    );
}
