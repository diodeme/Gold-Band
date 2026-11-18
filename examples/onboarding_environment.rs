//! Read-only P0 probe; no Agent installation, authentication or inference.
use gold_band::onboarding::environment::{
    LocalEnvironmentRunner, check_environment, environment_policy,
};
use std::sync::atomic::AtomicBool;

fn main() {
    let report = check_environment(
        &LocalEnvironmentRunner::default(),
        environment_policy(),
        &AtomicBool::new(false),
        |_| {},
    );
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if !report.npx_launchable {
        std::process::exit(1);
    }
}
