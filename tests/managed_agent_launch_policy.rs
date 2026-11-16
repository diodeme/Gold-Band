use gold_band::app::App;
use gold_band::config::{RuntimeConfig, SettingsConfig, catalog_agent_default_config};

#[test]
fn builtin_launch_config_follows_catalog_and_is_not_persisted() {
    let (settings, migrated) = SettingsConfig::from_json_value_with_migration(serde_json::json!({
        "settingsSchemaVersion": 10,
        "agents": {
            "claude-acp": {
                "adapter": { "command": "old-command", "args": ["old-version"], "displayName": "My Claude", "env": {"CUSTOM": "value"} },
                "primaryAgentDir": ".custom", "icon": "custom-icon"
            },
            "private-agent": {
                "adapter": { "command": "custom-command", "args": ["custom-version"], "displayName": "Private", "env": {} }
            }
        }
    })).unwrap();
    assert!(migrated);
    let agents = settings.agents.as_ref().unwrap();
    let builtin = &agents[&"claude-acp".parse().unwrap()];
    let expected = catalog_agent_default_config("claude-acp").unwrap();
    assert_eq!(builtin.adapter.command, expected.adapter.command);
    assert_eq!(builtin.adapter.args, expected.adapter.args);
    assert_eq!(builtin.adapter.display_name, "My Claude");
    assert_eq!(builtin.adapter.env["CUSTOM"], "value");
    assert_eq!(builtin.primary_agent_dir.as_deref(), Some(".custom"));
    assert_eq!(builtin.icon, "custom-icon");
    let persisted = serde_json::to_value(&settings).unwrap();
    let adapter = &persisted["agents"]["claude-acp"]["adapter"];
    assert!(adapter.get("command").is_none());
    assert!(adapter.get("args").is_none());
    assert_eq!(
        persisted["agents"]["private-agent"]["adapter"]["command"],
        "custom-command"
    );
    assert_eq!(
        persisted["agents"]["private-agent"]["adapter"]["args"][0],
        "custom-version"
    );
    let (reloaded, migrated) = SettingsConfig::from_json_value_with_migration(persisted).unwrap();
    assert!(!migrated);
    assert_eq!(
        reloaded.agents.unwrap()[&"claude-acp".parse().unwrap()]
            .adapter
            .args,
        expected.adapter.args
    );
}

#[test]
fn in_memory_settings_cannot_override_builtin_launch() {
    let id = "claude-acp".parse().unwrap();
    let mut agent = catalog_agent_default_config("claude-acp").unwrap();
    agent.adapter.command = "stale-command".into();
    agent.adapter.args = vec!["stale-version".into()];
    agent.adapter.display_name = "My Claude".into();
    let settings = SettingsConfig {
        agents: Some(std::collections::BTreeMap::from([(id, agent)])),
        ..SettingsConfig::default()
    };
    let runtime = RuntimeConfig::default().apply_settings(&settings);
    let resolved = &runtime.agents[&"claude-acp".parse().unwrap()];
    let expected = catalog_agent_default_config("claude-acp").unwrap();
    assert_eq!(resolved.adapter.command, expected.adapter.command);
    assert_eq!(resolved.adapter.args, expected.adapter.args);
    assert_eq!(resolved.adapter.display_name, "My Claude");
}

#[test]
fn missing_custom_launch_is_isolated_to_the_execution_boundary() {
    let (settings, migrated) = SettingsConfig::from_json_value_with_migration(serde_json::json!({
        "settingsSchemaVersion": 13,
        "agents": {
            "claude-acp": {
                "adapter": { "displayName": "Claude", "env": {} }
            },
            "registry-agent": {
                "adapter": { "displayName": "Registry Agent", "env": {} }
            }
        }
    }))
    .unwrap();
    assert!(!migrated);
    let agents = settings.agents.unwrap();
    let builtin = &agents[&"claude-acp".parse().unwrap()];
    let expected = catalog_agent_default_config("claude-acp").unwrap();
    assert_eq!(builtin.adapter.command, expected.adapter.command);
    let custom = &agents[&"registry-agent".parse().unwrap()];
    assert!(custom.adapter.command.is_empty());

    let error = custom
        .executable_adapter(&"registry-agent".parse().unwrap())
        .unwrap_err();
    let setup_error = error
        .downcast_ref::<gold_band::onboarding::model::SetupError>()
        .expect("missing launch must use the setup structured error");
    assert_eq!(setup_error.code, "agent.launch-command-missing");
}

#[test]
fn doctor_reports_missing_custom_launch_as_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let (settings, _) = SettingsConfig::from_json_value_with_migration(serde_json::json!({
        "settingsSchemaVersion": 13,
        "agents": {
            "registry-agent": {
                "adapter": { "displayName": "Registry Agent", "env": {} }
            }
        }
    }))
    .unwrap();
    let config = RuntimeConfig::default().apply_settings(&settings);
    let app = App::with_config(repo_root, config);

    let doctor = app.provider_doctor("registry-agent").unwrap();

    assert!(!doctor.available);
    assert_eq!(
        doctor
            .error
            .expect("missing launch must produce a diagnostic")
            .code,
        "agent.launch-command-missing"
    );
}
