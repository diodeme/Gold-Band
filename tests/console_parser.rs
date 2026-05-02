use gold_band::console::commands::{parse_console_command, suggest_console_commands, ConsoleLocalCommand, ParsedConsoleCommand};
use gold_band::command::{Command, RunCommand};
use gold_band::config::ConsoleThemeName;

#[test]
fn parses_run_start_command() {
    let parsed = parse_console_command("/run start task-001").unwrap();
    match parsed {
        ParsedConsoleCommand::Runtime(Command::Run(RunCommand::Start { task_id, workflow })) => {
            assert_eq!(task_id, "task-001");
            assert!(workflow.is_none());
        }
        _ => panic!("unexpected parse result"),
    }
}

#[test]
fn parses_help_as_local_command() {
    let parsed = parse_console_command("/help").unwrap();
    match parsed {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::Help) => {}
        _ => panic!("unexpected parse result"),
    }
}

#[test]
fn parses_local_commands() {
    let task = parse_console_command("/task").unwrap();
    let config = parse_console_command("/config").unwrap();
    let log = parse_console_command("/log").unwrap();
    let theme_show = parse_console_command("/theme").unwrap();
    let theme_set = parse_console_command("/theme nord").unwrap();
    let theme_set_cyber = parse_console_command("/theme cyber").unwrap();
    let theme_set_high_contrast = parse_console_command("/theme high-contrast").unwrap();
    let continue_cmd = parse_console_command("/continue").unwrap();
    match task {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::Task) => {}
        _ => panic!("unexpected task parse result"),
    }
    match config {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::Config) => {}
        _ => panic!("unexpected config parse result"),
    }
    match log {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::Log) => {}
        _ => panic!("unexpected log parse result"),
    }
    match theme_show {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::ThemeShow) => {}
        _ => panic!("unexpected theme show parse result"),
    }
    match theme_set {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::ThemeSet(ConsoleThemeName::Nord)) => {}
        _ => panic!("unexpected theme set parse result"),
    }
    match theme_set_cyber {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::ThemeSet(ConsoleThemeName::Cyber)) => {}
        _ => panic!("unexpected cyber theme parse result"),
    }
    match theme_set_high_contrast {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::ThemeSet(ConsoleThemeName::HighContrast)) => {}
        _ => panic!("unexpected high contrast theme parse result"),
    }
    match continue_cmd {
        ParsedConsoleCommand::Local(ConsoleLocalCommand::Continue) => {}
        _ => panic!("unexpected continue parse result"),
    }
    assert!(parse_console_command("/theme invalid-name").is_err());
}

#[test]
fn suggests_top_level_and_subcommands() {
    assert!(suggest_console_commands("/").contains(&"/run".to_string()));
    assert!(suggest_console_commands("/r").contains(&"/run".to_string()));
    assert!(suggest_console_commands("/th").contains(&"/theme".to_string()));
    assert!(suggest_console_commands("/theme ").contains(&"/theme nord".to_string()));
    assert!(suggest_console_commands("/theme ").contains(&"/theme cyber".to_string()));
    assert!(suggest_console_commands("/theme h").contains(&"/theme high-contrast".to_string()));
    assert!(suggest_console_commands("/theme d").contains(&"/theme dracula".to_string()));
    assert!(suggest_console_commands("/run ").contains(&"/run start".to_string()));
    assert!(suggest_console_commands("/run c").contains(&"/run continue".to_string()));
    assert!(suggest_console_commands("/artifact s").contains(&"/artifact show".to_string()));
}
