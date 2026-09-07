use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::{
    ImChannelCapabilities, ImChannelKind, ImConnectionIdentity, ImConnectorEvent, ImErrorCode,
    ImIntegrationError, ImObservedBinding,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImConnectionState {
    Disabled,
    Connecting,
    Reconnecting,
    Connected,
    AuthenticationRequired,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImChannelSnapshot {
    pub kind: ImChannelKind,
    pub enabled: bool,
    pub generation: u64,
    pub state: ImConnectionState,
    pub capabilities: ImChannelCapabilities,
    pub identity: Option<ImConnectionIdentity>,
    pub binding: Option<ImObservedBinding>,
    pub last_connected_at_ms: Option<i64>,
    pub last_error_code: Option<ImErrorCode>,
}

#[derive(Debug)]
pub struct ImConnectionStart {
    pub generation: u64,
    pub cancellation: CancellationToken,
}

#[derive(Debug)]
struct ChannelSlot {
    snapshot: ImChannelSnapshot,
    cancellation: CancellationToken,
}

#[derive(Debug, Default)]
pub struct ImConnectionManager {
    slots: Mutex<HashMap<ImChannelKind, ChannelSlot>>,
}

impl ImConnectionManager {
    pub fn replace(
        &self,
        kind: ImChannelKind,
        enabled: bool,
        capabilities: ImChannelCapabilities,
    ) -> ImConnectionStart {
        self.replace_with_binding(kind, enabled, capabilities, None)
    }

    pub fn replace_with_binding(
        &self,
        kind: ImChannelKind,
        enabled: bool,
        capabilities: ImChannelCapabilities,
        binding: Option<ImObservedBinding>,
    ) -> ImConnectionStart {
        let mut slots = self
            .slots
            .lock()
            .expect("IM connection manager lock poisoned");
        let generation = slots
            .get(&kind)
            .map(|slot| slot.snapshot.generation.saturating_add(1))
            .unwrap_or(1);
        if let Some(previous) = slots.get(&kind) {
            previous.cancellation.cancel();
        }
        let cancellation = CancellationToken::new();
        slots.insert(
            kind,
            ChannelSlot {
                snapshot: ImChannelSnapshot {
                    kind,
                    enabled,
                    generation,
                    state: if enabled {
                        ImConnectionState::Connecting
                    } else {
                        ImConnectionState::Disabled
                    },
                    capabilities,
                    identity: None,
                    binding,
                    last_connected_at_ms: None,
                    last_error_code: None,
                },
                cancellation: cancellation.clone(),
            },
        );
        ImConnectionStart {
            generation,
            cancellation,
        }
    }

    pub fn apply_event(&self, kind: ImChannelKind, event: ImConnectorEvent, now_ms: i64) -> bool {
        if let ImConnectorEvent::BindingObserved {
            generation,
            binding,
        } = &event
        {
            if self.prepare_binding(kind, *generation, binding) {
                return self.commit_binding(kind, *generation, binding.clone());
            }
            return self
                .snapshot(kind)
                .is_some_and(|snapshot| snapshot.generation == *generation);
        }
        let mut slots = self
            .slots
            .lock()
            .expect("IM connection manager lock poisoned");
        let Some(slot) = slots.get_mut(&kind) else {
            return false;
        };
        if event.generation() != slot.snapshot.generation {
            return false;
        }
        match event {
            ImConnectorEvent::Connected { identity, .. } => {
                slot.snapshot.state = ImConnectionState::Connected;
                slot.snapshot.identity = Some(identity);
                slot.snapshot.last_connected_at_ms = Some(now_ms);
                slot.snapshot.last_error_code = None;
            }
            ImConnectorEvent::ReconnectScheduled { error, .. } => {
                slot.snapshot.state = ImConnectionState::Reconnecting;
                slot.snapshot.last_error_code = Some(error.code);
            }
            ImConnectorEvent::ConnectionFailed { error, .. } => {
                slot.snapshot.state = connection_state_for_error(&error);
                slot.snapshot.last_error_code = Some(error.code);
            }
            ImConnectorEvent::BindingObserved { .. } => unreachable!("binding events return above"),
            ImConnectorEvent::InboundAction { .. } => {}
        }
        true
    }

    pub fn prepare_binding(
        &self,
        kind: ImChannelKind,
        generation: u64,
        binding: &ImObservedBinding,
    ) -> bool {
        let mut slots = self
            .slots
            .lock()
            .expect("IM connection manager lock poisoned");
        let Some(slot) = slots.get_mut(&kind) else {
            return false;
        };
        if generation != slot.snapshot.generation {
            return false;
        }
        if !binding.is_private {
            slot.snapshot.state = ImConnectionState::Error;
            slot.snapshot.last_error_code = Some(ImErrorCode::PrivateBindingRequired);
            return false;
        }
        if slot.snapshot.binding.as_ref().is_some_and(|current| {
            current.actor_id != binding.actor_id
                || current.conversation_id != binding.conversation_id
                || current.destination_id != binding.destination_id
        }) {
            slot.snapshot.state = ImConnectionState::Error;
            slot.snapshot.last_error_code = Some(
                if slot
                    .snapshot
                    .binding
                    .as_ref()
                    .is_some_and(|current| current.actor_id != binding.actor_id)
                {
                    ImErrorCode::ActorMismatch
                } else {
                    ImErrorCode::ConversationMismatch
                },
            );
            return false;
        }
        true
    }

    pub fn commit_binding(
        &self,
        kind: ImChannelKind,
        generation: u64,
        binding: ImObservedBinding,
    ) -> bool {
        let mut slots = self
            .slots
            .lock()
            .expect("IM connection manager lock poisoned");
        let Some(slot) = slots.get_mut(&kind) else {
            return false;
        };
        if generation != slot.snapshot.generation {
            return false;
        }
        slot.snapshot.binding = Some(binding);
        slot.snapshot.last_error_code = None;
        true
    }

    pub fn mark_binding_persistence_failed(&self, kind: ImChannelKind, generation: u64) -> bool {
        let mut slots = self
            .slots
            .lock()
            .expect("IM connection manager lock poisoned");
        let Some(slot) = slots.get_mut(&kind) else {
            return false;
        };
        if generation != slot.snapshot.generation {
            return false;
        }
        slot.snapshot.last_error_code = Some(ImErrorCode::StorageUnavailable);
        true
    }

    pub fn snapshot(&self, kind: ImChannelKind) -> Option<ImChannelSnapshot> {
        self.slots
            .lock()
            .expect("IM connection manager lock poisoned")
            .get(&kind)
            .map(|slot| slot.snapshot.clone())
    }
}

fn connection_state_for_error(error: &ImIntegrationError) -> ImConnectionState {
    if error.code == ImErrorCode::AuthenticationRequired {
        ImConnectionState::AuthenticationRequired
    } else {
        ImConnectionState::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_old_generation_events_cannot_replace_current_state() {
        let manager = ImConnectionManager::default();
        let capabilities = ImChannelCapabilities {
            proactive_delivery: true,
            card_actions: true,
            message_update: true,
            private_chat: true,
        };
        let first = manager.replace(ImChannelKind::WeCom, true, capabilities);
        let second = manager.replace(ImChannelKind::WeCom, true, capabilities);
        assert!(first.cancellation.is_cancelled());
        assert!(!manager.apply_event(
            ImChannelKind::WeCom,
            ImConnectorEvent::Connected {
                generation: first.generation,
                identity: ImConnectionIdentity {
                    bot_id: "old".into(),
                    display_name: "old".into(),
                },
            },
            10,
        ));
        assert!(manager.apply_event(
            ImChannelKind::WeCom,
            ImConnectorEvent::Connected {
                generation: second.generation,
                identity: ImConnectionIdentity {
                    bot_id: "new".into(),
                    display_name: "new".into(),
                },
            },
            20,
        ));
        let snapshot = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(snapshot.generation, second.generation);
        assert_eq!(snapshot.identity.unwrap().bot_id, "new");
    }

    #[test]
    fn retry_progress_and_terminal_failure_have_distinct_states() {
        let manager = ImConnectionManager::default();
        let start = manager.replace(ImChannelKind::WeCom, true, ImChannelCapabilities::default());
        let network_error = ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None);
        assert!(manager.apply_event(
            ImChannelKind::WeCom,
            ImConnectorEvent::ReconnectScheduled {
                generation: start.generation,
                error: network_error,
            },
            10,
        ));
        let reconnecting = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(reconnecting.state, ImConnectionState::Reconnecting);
        assert_eq!(
            reconnecting.last_error_code,
            Some(ImErrorCode::NetworkUnavailable)
        );

        assert!(manager.apply_event(
            ImChannelKind::WeCom,
            ImConnectorEvent::ConnectionFailed {
                generation: start.generation,
                error: ImIntegrationError::permanent(ImErrorCode::ConnectionConflict),
            },
            20,
        ));
        let failed = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(failed.state, ImConnectionState::Error);
        assert_eq!(
            failed.last_error_code,
            Some(ImErrorCode::ConnectionConflict)
        );
    }

    #[test]
    fn group_binding_is_rejected_at_the_manager_boundary() {
        let manager = ImConnectionManager::default();
        let start = manager.replace(ImChannelKind::WeCom, true, ImChannelCapabilities::default());
        assert!(manager.apply_event(
            ImChannelKind::WeCom,
            ImConnectorEvent::BindingObserved {
                generation: start.generation,
                binding: ImObservedBinding {
                    destination_id: "group".into(),
                    conversation_id: "group-chat".into(),
                    actor_id: "actor".into(),
                    is_private: false,
                },
            },
            0,
        ));
        let snapshot = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(snapshot.state, ImConnectionState::Error);
        assert_eq!(
            snapshot.last_error_code,
            Some(ImErrorCode::PrivateBindingRequired)
        );
        assert!(snapshot.binding.is_none());
    }

    #[test]
    fn persisted_private_binding_cannot_be_replaced_by_another_actor() {
        let manager = ImConnectionManager::default();
        let start = manager.replace_with_binding(
            ImChannelKind::WeCom,
            true,
            ImChannelCapabilities::default(),
            Some(ImObservedBinding {
                destination_id: "user-1".into(),
                conversation_id: "chat-1".into(),
                actor_id: "user-1".into(),
                is_private: true,
            }),
        );
        assert!(manager.apply_event(
            ImChannelKind::WeCom,
            ImConnectorEvent::BindingObserved {
                generation: start.generation,
                binding: ImObservedBinding {
                    destination_id: "user-2".into(),
                    conversation_id: "chat-2".into(),
                    actor_id: "user-2".into(),
                    is_private: true,
                },
            },
            0,
        ));
        let snapshot = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(snapshot.last_error_code, Some(ImErrorCode::ActorMismatch));
        assert_eq!(snapshot.binding.unwrap().actor_id, "user-1");
    }

    #[test]
    fn binding_can_be_validated_without_becoming_public() {
        let manager = ImConnectionManager::default();
        let start = manager.replace(ImChannelKind::WeCom, true, ImChannelCapabilities::default());
        let binding = ImObservedBinding {
            destination_id: "user-1".into(),
            conversation_id: "chat-1".into(),
            actor_id: "user-1".into(),
            is_private: true,
        };

        assert!(manager.prepare_binding(ImChannelKind::WeCom, start.generation, &binding));
        assert!(
            manager
                .snapshot(ImChannelKind::WeCom)
                .unwrap()
                .binding
                .is_none()
        );
        assert!(manager.mark_binding_persistence_failed(ImChannelKind::WeCom, start.generation));
        let failed = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert!(failed.binding.is_none());
        assert_eq!(
            failed.last_error_code,
            Some(ImErrorCode::StorageUnavailable)
        );

        assert!(manager.commit_binding(ImChannelKind::WeCom, start.generation, binding.clone()));
        let committed = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(committed.binding, Some(binding));
        assert_eq!(committed.last_error_code, None);
    }

    #[test]
    fn stale_generation_cannot_commit_a_persisted_binding() {
        let manager = ImConnectionManager::default();
        let first = manager.replace(ImChannelKind::WeCom, true, ImChannelCapabilities::default());
        let binding = ImObservedBinding {
            destination_id: "user-1".into(),
            conversation_id: "chat-1".into(),
            actor_id: "user-1".into(),
            is_private: true,
        };
        assert!(manager.prepare_binding(ImChannelKind::WeCom, first.generation, &binding));
        let second = manager.replace(ImChannelKind::WeCom, true, ImChannelCapabilities::default());

        assert!(!manager.commit_binding(ImChannelKind::WeCom, first.generation, binding));
        let snapshot = manager.snapshot(ImChannelKind::WeCom).unwrap();
        assert_eq!(snapshot.generation, second.generation);
        assert!(snapshot.binding.is_none());
    }
}
