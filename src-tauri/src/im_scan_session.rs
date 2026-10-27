//! One in-flight scan authorization per channel. The slot owns the session
//! lifecycle shared by every scan flow: opening a session cancels the previous
//! one, the platform ticket attaches only while the session is still current,
//! a single poller may claim it, and configuration commit or cancellation clears it.

use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanSlotError {
    Cancelled,
    Expired,
    SessionConflict,
    ProtocolInvalid,
}

struct ActiveScanSession<T> {
    session_id: String,
    ticket: Option<T>,
    polling: bool,
    cancellation: CancellationToken,
}

pub(crate) struct ScanSessionSlot<T> {
    active: Mutex<Option<ActiveScanSession<T>>>,
}

impl<T> Default for ScanSessionSlot<T> {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }
}

impl<T: Clone> ScanSessionSlot<T> {
    /// Replaces any previous session and returns the new session's token.
    pub(crate) fn open(&self, session_id: &str) -> CancellationToken {
        let cancellation = CancellationToken::new();
        let mut active = self.lock();
        if let Some(previous) = active.take() {
            previous.cancellation.cancel();
        }
        *active = Some(ActiveScanSession {
            session_id: session_id.to_owned(),
            ticket: None,
            polling: false,
            cancellation: cancellation.clone(),
        });
        cancellation
    }

    /// Stores the platform ticket unless the session was replaced or cancelled
    /// while the ticket was being requested.
    pub(crate) fn attach(
        &self,
        session_id: &str,
        cancellation: &CancellationToken,
        ticket: T,
    ) -> Result<(), ScanSlotError> {
        let mut active = self.lock();
        let Some(current) = active.as_mut() else {
            return Err(ScanSlotError::Cancelled);
        };
        if current.session_id != session_id || cancellation.is_cancelled() {
            return Err(ScanSlotError::Cancelled);
        }
        current.ticket = Some(ticket);
        Ok(())
    }

    /// Hands the ticket to exactly one poller.
    pub(crate) fn claim(&self, session_id: &str) -> Result<(T, CancellationToken), ScanSlotError> {
        let mut active = self.lock();
        let current = active
            .as_mut()
            .filter(|current| current.session_id == session_id)
            .ok_or(ScanSlotError::Expired)?;
        if current.polling {
            return Err(ScanSlotError::SessionConflict);
        }
        current.polling = true;
        Ok((
            current
                .ticket
                .clone()
                .ok_or(ScanSlotError::ProtocolInvalid)?,
            current.cancellation.clone(),
        ))
    }

    /// Cancels the named session, or whichever session is active for `None`.
    pub(crate) fn cancel(&self, session_id: Option<&str>) {
        let mut active = self.lock();
        if active.as_ref().is_some_and(|current| {
            session_id.is_none_or(|session_id| current.session_id == session_id)
        }) && let Some(current) = active.take()
        {
            current.cancellation.cancel();
        }
    }

    /// Commits configuration only while this claimed session still owns the
    /// slot. The synchronous commit is atomic with respect to open/cancel;
    /// network and credential-store I/O must finish before entering it.
    pub(crate) fn commit<R>(
        &self,
        session_id: &str,
        commit: impl FnOnce() -> R,
    ) -> Result<R, ScanSlotError> {
        let mut active = self.lock();
        let current = active
            .as_ref()
            .filter(|current| {
                current.session_id == session_id && !current.cancellation.is_cancelled()
            })
            .ok_or(ScanSlotError::Cancelled)?;
        if !current.polling {
            return Err(ScanSlotError::ProtocolInvalid);
        }
        let result = commit();
        if let Some(current) = active.take() {
            current.cancellation.cancel();
        }
        Ok(result)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<ActiveScanSession<T>>> {
        self.active.lock().expect("IM scan session lock poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_session_cancels_the_previous_one_and_rejects_its_ticket() {
        let slot = ScanSessionSlot::<String>::default();
        let first = slot.open("a");
        let second = slot.open("b");
        assert!(first.is_cancelled() && !second.is_cancelled());
        assert_eq!(
            slot.attach("a", &first, "late".to_owned()),
            Err(ScanSlotError::Cancelled)
        );
        assert_eq!(slot.claim("a").unwrap_err(), ScanSlotError::Expired);
        slot.attach("b", &second, "ticket".to_owned()).unwrap();
        assert_eq!(slot.claim("b").unwrap().0, "ticket");
    }

    #[test]
    fn only_one_poller_claims_and_completion_clears_the_session() {
        let slot = ScanSessionSlot::<String>::default();
        let cancellation = slot.open("a");
        assert_eq!(slot.claim("a").unwrap_err(), ScanSlotError::ProtocolInvalid);
        let cancellation_b = slot.open("b");
        slot.attach("b", &cancellation_b, "t".to_owned()).unwrap();
        let (_, token) = slot.claim("b").unwrap();
        assert_eq!(slot.claim("b").unwrap_err(), ScanSlotError::SessionConflict);
        slot.cancel(Some("other"));
        assert!(!token.is_cancelled());
        slot.cancel(Some("b"));
        assert!(token.is_cancelled() && cancellation.is_cancelled());
        assert_eq!(slot.claim("b").unwrap_err(), ScanSlotError::Expired);
    }

    #[test]
    fn commit_serializes_with_replacement_and_only_retires_its_own_session() {
        let slot = ScanSessionSlot::<String>::default();
        let token = slot.open("a");
        slot.attach("a", &token, "ticket".into()).unwrap();
        slot.claim("a").unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let slot_for_commit = &slot;
            let committing = scope.spawn(move || {
                slot_for_commit
                    .commit("a", || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        "saved"
                    })
                    .unwrap()
            });
            entered_rx.recv().unwrap();
            // The same slot mutex used by open/cancel covers the commit.
            assert!(slot.active.try_lock().is_err());
            let replacing = scope.spawn(|| slot.open("b"));
            release_tx.send(()).unwrap();
            assert_eq!(committing.join().unwrap(), "saved");
            let replacement = replacing.join().unwrap();
            assert!(token.is_cancelled());
            assert!(!replacement.is_cancelled());
            slot.cancel(Some("a"));
            slot.attach("b", &replacement, "new ticket".into()).unwrap();
            assert_eq!(slot.claim("b").unwrap().0, "new ticket");
        });
    }

    #[test]
    fn cancelling_without_an_id_clears_any_session() {
        let slot = ScanSessionSlot::<String>::default();
        let token = slot.open("a");
        slot.cancel(None);
        assert!(token.is_cancelled());
        assert_eq!(
            slot.attach("a", &token, "t".to_owned()),
            Err(ScanSlotError::Cancelled)
        );
    }
}
