use super::model::{GitDecision, OnboardingState, SetupError};
use crate::{
    config::SettingsConfig,
    storage::{
        GoldBandPaths, configuration::with_configuration, load_settings_file, save_settings_file,
    },
};
use anyhow::Result;

/// Called before language/profile/template seeding. Only known configuration locations are inspected.
pub fn initialize(paths: &GoldBandPaths) -> Result<SettingsConfig> {
    with_configuration(&paths.user_gold_band_dir(), || {
        let existing = paths.user_settings_file().exists()
            || paths.user_state_file().exists()
            || [
                paths.user_context_dir(),
                paths.user_profiles_dir(),
                paths.user_presets_dir(),
            ]
            .iter()
            .any(|path| std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some()));
        let mut settings = load_settings_file(&paths.user_settings_file())?;
        if settings.onboarding.is_none() {
            settings.onboarding = Some(if existing {
                OnboardingState::ExistingInstall
            } else {
                OnboardingState::Pending {
                    git_decision: GitDecision::Undecided,
                }
            });
            if !existing {
                settings.agents = Some(Default::default());
            }
            save_settings_file(&paths.user_settings_file(), &settings)?;
            settings = load_settings_file(&paths.user_settings_file())?;
        }
        Ok(settings)
    })
}

pub fn skip_git(paths: &GoldBandPaths) -> Result<SettingsConfig> {
    with_configuration(&paths.user_gold_band_dir(), || {
        let mut settings = load_settings_file(&paths.user_settings_file())?;
        match settings.onboarding.as_mut() {
            Some(OnboardingState::Pending { git_decision })
            | Some(OnboardingState::Deferred { git_decision }) => *git_decision = GitDecision::Skip,
            Some(OnboardingState::Completed { .. } | OnboardingState::ExistingInstall) => {
                return Ok(settings);
            }
            None => return Err(SetupError::new("setup.initialization-required").into()),
        }
        save_settings_file(&paths.user_settings_file(), &settings)?;
        load_settings_file(&paths.user_settings_file())
    })
}

pub fn defer(paths: &GoldBandPaths) -> Result<SettingsConfig> {
    with_configuration(&paths.user_gold_band_dir(), || {
        let mut settings = load_settings_file(&paths.user_settings_file())?;
        let git_decision = match &settings.onboarding {
            Some(OnboardingState::Pending { git_decision }) => *git_decision,
            Some(_) => return Ok(settings),
            None => return Err(SetupError::new("setup.initialization-required").into()),
        };
        settings.onboarding = Some(OnboardingState::Deferred { git_decision });
        save_settings_file(&paths.user_settings_file(), &settings)?;
        load_settings_file(&paths.user_settings_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{StoragePathConfig, write_json};

    fn paths(root: &tempfile::TempDir) -> GoldBandPaths {
        GoldBandPaths::new_with_path_config(
            camino::Utf8Path::from_path(root.path()).unwrap(),
            StoragePathConfig {
                app_key: "onboarding-fixture",
                config_dir_name: "onboarding-fixture",
                home_env_var: "GOLD_BAND_ONBOARDING_TEST_HOME",
            },
        )
    }

    #[test]
    fn onboarding_state_serialization_keeps_skip_separate_from_health() {
        let state = OnboardingState::Pending {
            git_decision: GitDecision::Skip,
        };
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"status":"pending", "gitDecision":"skip"})
        );
    }

    #[test]
    fn deferred_state_serializes_git_decision() {
        let value = serde_json::to_value(OnboardingState::Deferred {
            git_decision: GitDecision::Skip,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"status": "deferred", "gitDecision": "skip"})
        );
    }

    #[test]
    fn clean_install_is_recorded_before_seeding_and_pending_survives_restart() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(&temp);
        let first = initialize(&paths).unwrap();
        assert_eq!(
            first.onboarding,
            Some(OnboardingState::Pending {
                git_decision: GitDecision::Undecided
            })
        );
        assert!(first.agents.unwrap().is_empty());
        let skipped = skip_git(&paths).unwrap();
        assert_eq!(initialize(&paths).unwrap().onboarding, skipped.onboarding);
    }

    #[test]
    fn existing_configuration_is_not_forced_through_onboarding() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(&temp);
        write_json(&paths.user_settings_file(), &SettingsConfig::default()).unwrap();
        let settings = initialize(&paths).unwrap();
        assert_eq!(settings.onboarding, Some(OnboardingState::ExistingInstall));
        assert_eq!(skip_git(&paths).unwrap().onboarding, settings.onboarding);
    }

    #[test]
    fn profile_data_without_settings_is_an_existing_install() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(&temp);
        std::fs::create_dir_all(paths.user_profiles_dir()).unwrap();
        std::fs::write(paths.user_profiles_dir().join("user.md"), "profile").unwrap();
        assert_eq!(
            initialize(&paths).unwrap().onboarding,
            Some(OnboardingState::ExistingInstall)
        );
    }

    #[test]
    fn deferred_setup_survives_restart_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(&temp);
        assert_eq!(
            initialize(&paths).unwrap().onboarding,
            Some(OnboardingState::Pending {
                git_decision: GitDecision::Undecided
            })
        );
        assert_eq!(
            defer(&paths).unwrap().onboarding,
            Some(OnboardingState::Deferred {
                git_decision: GitDecision::Undecided
            })
        );
        assert_eq!(
            initialize(&paths).unwrap().onboarding,
            Some(OnboardingState::Deferred {
                git_decision: GitDecision::Undecided
            })
        );
        assert_eq!(
            defer(&paths).unwrap().onboarding,
            Some(OnboardingState::Deferred {
                git_decision: GitDecision::Undecided
            })
        );
        assert_eq!(
            skip_git(&paths).unwrap().onboarding,
            Some(OnboardingState::Deferred {
                git_decision: GitDecision::Skip
            })
        );
    }
}
