use std::time::Duration;

use gold_band::acp::client::AcpRuntimePolicy;
use gold_band::config::RuntimeConfig;

#[test]
fn prompt_terminal_route_timeout_defaults_to_ten_seconds() {
    assert_eq!(
        AcpRuntimePolicy::default().prompt_terminal_route_timeout,
        Duration::from_secs(10)
    );
    assert_eq!(
        RuntimeConfig::default().acp_prompt_terminal_route_timeout_ms,
        10_000
    );
}
