use gold_band::scheduler::occurrence::ScheduledError;
use gold_band::scheduler::occurrence::ScheduledErrorCode;

pub trait SystemSleepInhibitor: Send {
    fn acquire(&mut self, reason: &str) -> Result<(), ScheduledError>;
    fn release(&mut self);
}

#[derive(Debug, Clone)]
pub struct PowerStatus {
    pub effective: bool,
    pub error: Option<ScheduledError>,
}

#[derive(Debug, Clone)]
pub struct ScheduledPowerStatus {
    pub effective: bool,
    pub enabled_job_count: usize,
    pub error: Option<ScheduledError>,
}

pub struct PowerController<I> {
    inhibitor: I,
    effective: bool,
}

pub struct ScheduledPowerManager<I> {
    controller: PowerController<I>,
    status: ScheduledPowerStatus,
}

impl<I> ScheduledPowerManager<I>
where
    I: SystemSleepInhibitor,
{
    pub fn new(inhibitor: I) -> Self {
        Self {
            controller: PowerController::new(inhibitor),
            status: ScheduledPowerStatus {
                effective: false,
                enabled_job_count: 0,
                error: None,
            },
        }
    }

    pub fn reconcile(
        &mut self,
        keep_awake_enabled: bool,
        enabled_job_count: usize,
        app_is_running: bool,
    ) -> ScheduledPowerStatus {
        let controller_status =
            self.controller
                .update(keep_awake_enabled, enabled_job_count, app_is_running);
        self.status = ScheduledPowerStatus {
            effective: controller_status.effective,
            enabled_job_count,
            error: controller_status.error,
        };
        self.status.clone()
    }

    pub fn status(&self) -> ScheduledPowerStatus {
        self.status.clone()
    }
}

#[derive(Default)]
pub struct PlatformSleepInhibitor {
    guard: Option<keepawake::KeepAwake>,
}

impl SystemSleepInhibitor for PlatformSleepInhibitor {
    fn acquire(&mut self, reason: &str) -> Result<(), ScheduledError> {
        let guard = keepawake::Builder::default()
            .display(false)
            .idle(true)
            .sleep(false)
            .reason(reason)
            .app_name("Gold Band")
            .app_reverse_domain("local.gold-band.desktop")
            .create()
            .map_err(|error| {
                ScheduledError::with_params(
                    ScheduledErrorCode::PowerInhibitorFailed,
                    serde_json::json!({ "reason": error.to_string() }),
                )
            })?;
        self.guard = Some(guard);
        Ok(())
    }

    fn release(&mut self) {
        self.guard.take();
    }
}

impl<I> PowerController<I>
where
    I: SystemSleepInhibitor,
{
    pub fn new(inhibitor: I) -> Self {
        Self {
            inhibitor,
            effective: false,
        }
    }

    pub fn update(
        &mut self,
        keep_awake_enabled: bool,
        enabled_job_count: usize,
        app_is_running: bool,
    ) -> PowerStatus {
        let should_be_effective = keep_awake_enabled && enabled_job_count > 0 && app_is_running;
        if should_be_effective && !self.effective {
            if let Err(error) = self
                .inhibitor
                .acquire("Gold Band scheduled tasks are enabled")
            {
                return PowerStatus {
                    effective: false,
                    error: Some(error),
                };
            }
            self.effective = true;
        } else if !should_be_effective && self.effective {
            self.inhibitor.release();
            self.effective = false;
        }

        PowerStatus {
            effective: self.effective,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gold_band::scheduler::occurrence::{ScheduledError, ScheduledErrorCode};

    use super::{PowerController, ScheduledPowerManager, SystemSleepInhibitor};

    #[derive(Debug, Default)]
    struct FakeState {
        acquire_count: usize,
        release_count: usize,
        fail_acquire: bool,
    }

    struct FakeInhibitor {
        state: Arc<Mutex<FakeState>>,
    }

    impl SystemSleepInhibitor for FakeInhibitor {
        fn acquire(&mut self, _reason: &str) -> Result<(), ScheduledError> {
            let mut state = self.state.lock().unwrap();
            state.acquire_count += 1;
            if state.fail_acquire {
                return Err(ScheduledError::new(
                    ScheduledErrorCode::PowerInhibitorFailed,
                ));
            }
            Ok(())
        }

        fn release(&mut self) {
            self.state.lock().unwrap().release_count += 1;
        }
    }

    fn controller() -> (PowerController<FakeInhibitor>, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState::default()));
        (
            PowerController::new(FakeInhibitor {
                state: state.clone(),
            }),
            state,
        )
    }

    #[test]
    fn acquires_once_only_when_setting_jobs_and_app_are_active() {
        let (mut controller, state) = controller();

        assert!(!controller.update(false, 1, true).effective);
        assert!(!controller.update(true, 0, true).effective);
        assert!(!controller.update(true, 1, false).effective);
        assert!(controller.update(true, 1, true).effective);
        assert!(controller.update(true, 2, true).effective);

        let state = state.lock().unwrap();
        assert_eq!(state.acquire_count, 1);
        assert_eq!(state.release_count, 0);
    }

    #[test]
    fn releases_once_when_the_last_activation_condition_disappears() {
        let (mut controller, state) = controller();

        assert!(controller.update(true, 2, true).effective);
        assert!(!controller.update(true, 0, true).effective);
        assert!(!controller.update(false, 0, true).effective);

        let state = state.lock().unwrap();
        assert_eq!(state.acquire_count, 1);
        assert_eq!(state.release_count, 1);
    }

    #[test]
    fn acquire_failure_is_reported_without_becoming_effective() {
        let (mut controller, state) = controller();
        state.lock().unwrap().fail_acquire = true;

        let status = controller.update(true, 1, true);

        assert!(!status.effective);
        assert_eq!(
            status.error.map(|error| error.code),
            Some(ScheduledErrorCode::PowerInhibitorFailed)
        );
        assert_eq!(state.lock().unwrap().acquire_count, 1);
    }

    #[test]
    fn manager_tracks_enabled_jobs_and_releases_on_shutdown() {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let mut manager = ScheduledPowerManager::new(FakeInhibitor {
            state: state.clone(),
        });

        let active = manager.reconcile(true, 2, true);
        assert!(active.effective);
        assert_eq!(active.enabled_job_count, 2);

        let shutdown = manager.reconcile(true, 2, false);
        assert!(!shutdown.effective);
        assert_eq!(shutdown.enabled_job_count, 2);
        assert_eq!(state.lock().unwrap().release_count, 1);
    }
}
