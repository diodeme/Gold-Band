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
