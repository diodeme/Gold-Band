use std::sync::Mutex;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use super::{
    ClaimedImDelivery, ImChannelKind, ImDelivery, ImDeliveryBinding, ImDeliveryPayload,
    ImDeliveryState, ImDestination, ImInboundActionResult, ImInboundOutcome, ImNotificationKind,
    ImResolutionDispatch, MAX_IM_PRESENTATION_BYTES, resolution_event_id,
};

const IM_SCHEMA_VERSION: i64 = 5;
/// Schema version that introduced payload v2; older intervention payloads and
/// `desktop-resolved` records cannot be decoded and are removed on upgrade.
const IM_PAYLOAD_V2_SCHEMA_VERSION: i64 = 4;
const IM_RESOLUTION_CONTENT_SCHEMA_VERSION: i64 = 5;
pub const DEFAULT_IM_OUTBOX_ACTIVE_LIMIT: usize = 1_000;
pub const MAX_IM_DUE_BATCH: usize = 32;
pub const MAX_IM_CLEANUP_BATCH: usize = 200;
const IM_DELIVERY_DISPLAY_REF_MODULUS: i64 = 10_000;
/// Predicate on an `im_outbox` row: an intervention source whose per-target
/// resolution already exists. Such a source must never be (re)sent.
fn source_is_resolved_sql() -> String {
    // The suffix is a compile-time constant, not caller input.
    let suffix = resolution_event_id("");
    format!(
        "(payload_kind = 'intervention' AND EXISTS (
            SELECT 1 FROM im_outbox AS resolution
            WHERE resolution.channel_kind = im_outbox.channel_kind
              AND resolution.destination_id = im_outbox.destination_id
              AND resolution.notification_kind = im_outbox.notification_kind
              AND resolution.canonical_event_id = im_outbox.canonical_event_id || '{suffix}'))"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImDeliveryInsertResult {
    Inserted,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImDeliverySettlementResult {
    Applied,
    StaleClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImChannelCleanupPhase {
    Prepared,
    SettingsRemoved,
    ProjectionDrained,
    OutboxRemoved,
}

impl ImChannelCleanupPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::SettingsRemoved => "settings_removed",
            Self::ProjectionDrained => "projection_drained",
            Self::OutboxRemoved => "outbox_removed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "prepared" => Some(Self::Prepared),
            "settings_removed" => Some(Self::SettingsRemoved),
            "projection_drained" => Some(Self::ProjectionDrained),
            "outbox_removed" => Some(Self::OutboxRemoved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImChannelCleanupOperation {
    pub operation_id: String,
    pub channel: ImChannelKind,
    pub credential_ref: Option<String>,
    pub phase: ImChannelCleanupPhase,
    pub last_error_code: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImRepositoryError {
    #[error("IM repository I/O failed")]
    Io(#[from] std::io::Error),
    #[error("IM repository SQLite failed")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IM repository lock is unavailable")]
    LockUnavailable,
    #[error("IM repository schema {found} is newer than supported {supported}")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("IM outbox active capacity was reached")]
    Capacity { limit: usize },
    #[error("IM delivery payload is invalid")]
    InvalidPayload,
    #[error("IM delivery payload exceeds the size limit")]
    PayloadTooLarge { limit: usize },
    #[error("IM repository contains invalid typed data")]
    InvalidStoredData,
}

impl ImRepositoryError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) | Self::Sqlite(_) | Self::LockUnavailable => "IM_STORAGE_UNAVAILABLE",
            Self::SchemaTooNew { .. } => "IM_SCHEMA_TOO_NEW",
            Self::Capacity { .. } => "IM_OUTBOX_CAPACITY",
            Self::InvalidPayload | Self::InvalidStoredData => "IM_PAYLOAD_INVALID",
            Self::PayloadTooLarge { .. } => "IM_PAYLOAD_TOO_LARGE",
        }
    }
}

pub struct ImRepository {
    path: Utf8PathBuf,
    connection: Mutex<Option<Connection>>,
    active_limit: usize,
}

impl std::fmt::Debug for ImRepository {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImRepository")
            .field("path", &self.path)
            .field("active_limit", &self.active_limit)
            .finish_non_exhaustive()
    }
}

impl ImRepository {
    pub fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            connection: Mutex::new(None),
            active_limit: DEFAULT_IM_OUTBOX_ACTIVE_LIMIT,
        }
    }

    #[cfg(test)]
    fn with_active_limit(path: Utf8PathBuf, active_limit: usize) -> Self {
        Self {
            path,
            connection: Mutex::new(None),
            active_limit,
        }
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub fn has_pending_channel_cleanups_at(path: &Utf8Path) -> Result<bool, ImRepositoryError> {
        if !path.exists() {
            return Ok(false);
        }
        let connection = Connection::open_with_flags(
            path.as_std_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_millis(250))?;
        let table_exists = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'im_channel_cleanup_operations'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !table_exists {
            return Ok(false);
        }
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM im_channel_cleanup_operations LIMIT 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(Into::into)
    }

    pub fn enqueue(
        &self,
        delivery: &ImDelivery,
        now_ms: i64,
    ) -> Result<ImDeliveryInsertResult, ImRepositoryError> {
        validate_delivery(delivery)?;
        let payload_json = serde_json::to_string(&delivery.payload)
            .map_err(|_| ImRepositoryError::InvalidPayload)?;
        if payload_json.len() > MAX_IM_PRESENTATION_BYTES {
            return Err(ImRepositoryError::PayloadTooLarge {
                limit: MAX_IM_PRESENTATION_BYTES,
            });
        }
        let destination_json = serde_json::to_string(&delivery.destination)
            .map_err(|_| ImRepositoryError::InvalidPayload)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if matches!(&delivery.payload, ImDeliveryPayload::Intervention { .. }) {
                let terminal_event_id = resolution_event_id(&delivery.canonical_event_id);
                let terminal_delivery_id = ImDelivery::deterministic_id(
                    delivery.channel,
                    &delivery.destination.destination_id,
                    delivery.notification_kind,
                    &terminal_event_id,
                );
                let terminal_exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM im_outbox WHERE delivery_id = ?1)",
                    params![terminal_delivery_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if terminal_exists {
                    return Ok(ImDeliveryInsertResult::Duplicate);
                }
            }
            let exists = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM im_outbox
                    WHERE channel_kind = ?1 AND destination_id = ?2
                      AND notification_kind = ?3 AND canonical_event_id = ?4
                )",
                params![
                    delivery.channel.as_str(),
                    delivery.destination.destination_id,
                    delivery.notification_kind.as_str(),
                    delivery.canonical_event_id,
                ],
                |row| row.get::<_, bool>(0),
            )?;
            if exists {
                return Ok(ImDeliveryInsertResult::Duplicate);
            }
            let active = transaction.query_row(
                "SELECT COUNT(*) FROM im_outbox
                 WHERE state IN ('pending', 'sending')",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            if active >= self.active_limit as i64 {
                return Err(ImRepositoryError::Capacity {
                    limit: self.active_limit,
                });
            }
            transaction.execute(
                "INSERT INTO im_outbox (
                    delivery_id, channel_kind, destination_id, destination_json,
                    notification_kind, canonical_event_id, payload_kind, payload_json,
                    state, attempt_count, next_attempt_at_ms, expires_at_ms,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                           'pending', 0, ?9, ?10, ?9, ?9)",
                params![
                    delivery.delivery_id,
                    delivery.channel.as_str(),
                    delivery.destination.destination_id,
                    destination_json,
                    delivery.notification_kind.as_str(),
                    delivery.canonical_event_id,
                    delivery.payload.payload_kind(),
                    payload_json,
                    now_ms,
                    delivery.expires_at_ms,
                ],
            )?;
            transaction.commit()?;
            Ok(ImDeliveryInsertResult::Inserted)
        })
    }

    pub fn intervention_deliveries_for_canonical_event(
        &self,
        canonical_event_id: &str,
    ) -> Result<Vec<ImDelivery>, ImRepositoryError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT rowid, delivery_id, channel_kind, destination_json,
                        notification_kind, canonical_event_id, payload_json,
                        expires_at_ms, attempt_count
                 FROM im_outbox INDEXED BY idx_im_outbox_canonical_event
                 WHERE canonical_event_id = ?1 AND payload_kind = 'intervention'
                 ORDER BY channel_kind, destination_id, delivery_id",
            )?;
            let rows = statement.query_map(params![canonical_event_id], |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    StoredDeliveryRow {
                        row_id: row.get(0)?,
                        delivery_id: row.get(1)?,
                        destination_json: row.get(3)?,
                        notification_kind: row.get(4)?,
                        canonical_event_id: row.get(5)?,
                        payload_json: row.get(6)?,
                        expires_at_ms: row.get(7)?,
                        attempt_count: row.get(8)?,
                    },
                ))
            })?;
            rows.map(|row| {
                let (channel, row) = row?;
                let channel =
                    ImChannelKind::parse(&channel).ok_or(ImRepositoryError::InvalidStoredData)?;
                row.into_delivery(channel)
            })
            .collect()
        })
    }

    pub fn enqueue_resolution_and_expire_pending_source(
        &self,
        source_event_id: &str,
        resolution: &ImDelivery,
        dispatch: ImResolutionDispatch,
        now_ms: i64,
    ) -> Result<ImDeliveryInsertResult, ImRepositoryError> {
        validate_delivery(resolution)?;
        let ImDeliveryPayload::InterventionResolution {
            resolution: payload,
            ..
        } = &resolution.payload
        else {
            return Err(ImRepositoryError::InvalidPayload);
        };
        if source_event_id.trim().is_empty()
            || payload.source_event_id != source_event_id
            || payload.canonical_event_id != resolution.canonical_event_id
        {
            return Err(ImRepositoryError::InvalidPayload);
        }
        let payload_json = serde_json::to_string(&resolution.payload)
            .map_err(|_| ImRepositoryError::InvalidPayload)?;
        if payload_json.len() > MAX_IM_PRESENTATION_BYTES {
            return Err(ImRepositoryError::PayloadTooLarge {
                limit: MAX_IM_PRESENTATION_BYTES,
            });
        }
        let destination_json = serde_json::to_string(&resolution.destination)
            .map_err(|_| ImRepositoryError::InvalidPayload)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "UPDATE im_outbox SET state = 'expired', updated_at_ms = ?1
                 WHERE channel_kind = ?2 AND destination_id = ?3
                   AND notification_kind = ?4 AND canonical_event_id = ?5
                   AND payload_kind = 'intervention' AND state = 'pending'",
                params![
                    now_ms,
                    resolution.channel.as_str(),
                    resolution.destination.destination_id,
                    resolution.notification_kind.as_str(),
                    source_event_id,
                ],
            )?;
            let exists = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM im_outbox WHERE delivery_id = ?1)",
                params![resolution.delivery_id],
                |row| row.get::<_, bool>(0),
            )?;
            if exists {
                transaction.commit()?;
                return Ok(ImDeliveryInsertResult::Duplicate);
            }
            let state = match dispatch {
                ImResolutionDispatch::Send => {
                    let active = transaction.query_row(
                        "SELECT COUNT(*) FROM im_outbox WHERE state IN ('pending', 'sending')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?;
                    if active >= self.active_limit as i64 {
                        return Err(ImRepositoryError::Capacity {
                            limit: self.active_limit,
                        });
                    }
                    ImDeliveryState::Pending
                }
                ImResolutionDispatch::TombstoneOnly => ImDeliveryState::Expired,
            };
            transaction.execute(
                "INSERT INTO im_outbox (
                    delivery_id, channel_kind, destination_id, destination_json,
                    notification_kind, canonical_event_id, payload_kind, payload_json,
                    state, attempt_count, next_attempt_at_ms, expires_at_ms,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                           ?11, 0, ?9, ?10, ?9, ?9)",
                params![
                    resolution.delivery_id,
                    resolution.channel.as_str(),
                    resolution.destination.destination_id,
                    destination_json,
                    resolution.notification_kind.as_str(),
                    resolution.canonical_event_id,
                    resolution.payload.payload_kind(),
                    payload_json,
                    now_ms,
                    resolution.expires_at_ms,
                    state.as_str(),
                ],
            )?;
            transaction.commit()?;
            Ok(ImDeliveryInsertResult::Inserted)
        })
    }

    pub fn claim_due(
        &self,
        channel: ImChannelKind,
        now_ms: i64,
        lease_duration: Duration,
        requested_limit: usize,
    ) -> Result<Vec<ClaimedImDelivery>, ImRepositoryError> {
        let limit = requested_limit.clamp(1, MAX_IM_DUE_BATCH);
        let lease_ms = i64::try_from(lease_duration.as_millis())
            .unwrap_or(i64::MAX)
            .max(1);
        let lease_expires_at = now_ms.saturating_add(lease_ms);
        let has_due_or_expired = self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM im_outbox INDEXED BY idx_im_outbox_due
                        WHERE channel_kind = ?1 AND state = 'pending'
                          AND (next_attempt_at_ms <= ?2 OR expires_at_ms <= ?2)
                        LIMIT 1
                    )",
                    params![channel.as_str(), now_ms],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(Into::into)
        })?;
        if !has_due_or_expired {
            return Ok(Vec::new());
        }
        let rows = self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT rowid, delivery_id, destination_json, notification_kind,
                        canonical_event_id, payload_json, expires_at_ms, attempt_count
                 FROM im_outbox INDEXED BY idx_im_outbox_due
                 WHERE channel_kind = ?1 AND state = 'pending'
                   AND next_attempt_at_ms <= ?2 AND expires_at_ms > ?2
                 ORDER BY next_attempt_at_ms, created_at_ms, delivery_id
                 LIMIT ?3",
            )?;
            let mapped = statement.query_map(params![channel.as_str(), now_ms, limit], |row| {
                Ok(StoredDeliveryRow {
                    row_id: row.get(0)?,
                    delivery_id: row.get(1)?,
                    destination_json: row.get(2)?,
                    notification_kind: row.get(3)?,
                    canonical_event_id: row.get(4)?,
                    payload_json: row.get(5)?,
                    expires_at_ms: row.get(6)?,
                    attempt_count: row.get(7)?,
                })
            })?;
            Ok(mapped.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut valid = Vec::with_capacity(rows.len());
        let mut invalid = Vec::new();
        for row in rows {
            let row_id = row.row_id;
            let attempt_count = row.attempt_count;
            match row.into_claimed(channel) {
                Ok(claimed) => valid.push((row_id, attempt_count, claimed)),
                Err(error @ ImRepositoryError::InvalidPayload)
                | Err(error @ ImRepositoryError::InvalidStoredData)
                | Err(error @ ImRepositoryError::PayloadTooLarge { .. }) => {
                    invalid.push((row_id, attempt_count, error.code()));
                }
                Err(error) => return Err(error),
            }
        }
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "UPDATE im_outbox SET state = 'expired', updated_at_ms = ?1
                 WHERE channel_kind = ?2 AND state = 'pending' AND expires_at_ms <= ?1",
                params![now_ms, channel.as_str()],
            )?;
            let mut claimed = Vec::with_capacity(valid.len());
            for (row_id, attempt_count, delivery) in valid {
                let changed = transaction.execute(
                    "UPDATE im_outbox
                     SET state = 'sending', attempt_count = attempt_count + 1,
                         next_attempt_at_ms = ?1, updated_at_ms = ?2
                     WHERE rowid = ?3 AND channel_kind = ?4 AND state = 'pending'
                       AND next_attempt_at_ms <= ?2 AND expires_at_ms > ?2
                       AND attempt_count = ?5",
                    params![
                        lease_expires_at,
                        now_ms,
                        row_id,
                        channel.as_str(),
                        attempt_count
                    ],
                )?;
                if changed == 1 {
                    claimed.push(delivery);
                }
            }
            for (row_id, attempt_count, error_code) in invalid {
                transaction.execute(
                    "UPDATE im_outbox
                     SET state = 'dead_letter', next_attempt_at_ms = NULL,
                         last_error_code = ?1, updated_at_ms = ?2
                     WHERE rowid = ?3 AND channel_kind = ?4 AND state = 'pending'
                       AND next_attempt_at_ms <= ?2 AND expires_at_ms > ?2
                       AND attempt_count = ?5",
                    params![error_code, now_ms, row_id, channel.as_str(), attempt_count],
                )?;
            }
            transaction.commit()?;
            Ok(claimed)
        })
    }

    pub fn recover_expired_leases(
        &self,
        now_ms: i64,
        requested_limit: usize,
    ) -> Result<usize, ImRepositoryError> {
        let limit = requested_limit.clamp(1, MAX_IM_CLEANUP_BATCH);
        let resolved = source_is_resolved_sql();
        self.with_connection(|connection| {
            let changed = connection.execute(
                &format!(
                    "UPDATE im_outbox
                 SET state = CASE WHEN expires_at_ms <= ?1 OR {resolved}
                                  THEN 'expired' ELSE 'pending' END,
                     next_attempt_at_ms = CASE WHEN expires_at_ms <= ?1 OR {resolved}
                                  THEN NULL ELSE ?1 END,
                     updated_at_ms = ?1
                 WHERE delivery_id IN (
                    SELECT delivery_id FROM im_outbox INDEXED BY idx_im_outbox_due
                    WHERE state = 'sending' AND next_attempt_at_ms <= ?1
                    ORDER BY next_attempt_at_ms, delivery_id LIMIT ?2
                 )"
                ),
                params![now_ms, limit],
            )?;
            Ok(changed)
        })
    }

    pub fn mark_sent(
        &self,
        delivery_id: &str,
        channel: ImChannelKind,
        expected_attempt_count: u32,
        binding: Option<&ImDeliveryBinding>,
        now_ms: i64,
    ) -> Result<ImDeliverySettlementResult, ImRepositoryError> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = transaction.execute(
                "UPDATE im_outbox SET state = 'sent', next_attempt_at_ms = NULL,
                          last_error_code = NULL, updated_at_ms = ?1
                 WHERE delivery_id = ?2 AND channel_kind = ?3
                   AND state = 'sending' AND attempt_count = ?4",
                params![
                    now_ms,
                    delivery_id,
                    channel.as_str(),
                    expected_attempt_count
                ],
            )?;
            if changed == 1
                && let Some(binding) = binding
            {
                transaction.execute(
                    "INSERT INTO im_delivery_bindings (
                        delivery_id, channel_kind, platform_message_id,
                        platform_chat_id, update_token_ref, created_at_ms, updated_at_ms
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                     ON CONFLICT(delivery_id) DO UPDATE SET
                        platform_message_id = excluded.platform_message_id,
                        platform_chat_id = excluded.platform_chat_id,
                        update_token_ref = excluded.update_token_ref,
                        updated_at_ms = excluded.updated_at_ms",
                    params![
                        binding.delivery_id,
                        binding.channel.as_str(),
                        binding.platform_message_id,
                        binding.platform_chat_id,
                        binding.update_token_ref,
                        now_ms,
                    ],
                )?;
            }
            transaction.commit()?;
            Ok(if changed == 1 {
                ImDeliverySettlementResult::Applied
            } else {
                ImDeliverySettlementResult::StaleClaim
            })
        })
    }

    pub fn mark_failed(
        &self,
        delivery_id: &str,
        expected_attempt_count: u32,
        retry_at_ms: Option<i64>,
        error_code: &str,
        now_ms: i64,
    ) -> Result<ImDeliverySettlementResult, ImRepositoryError> {
        let state = if retry_at_ms.is_some() {
            ImDeliveryState::Pending
        } else {
            ImDeliveryState::DeadLetter
        };
        let resolved = source_is_resolved_sql();
        self.with_connection(|connection| {
            // A retry never resurrects a source whose resolution is already
            // recorded; it expires in the same statement instead.
            let changed = connection.execute(
                &format!(
                    "UPDATE im_outbox
                 SET state = CASE WHEN ?1 = 'pending' AND {resolved}
                                  THEN 'expired' ELSE ?1 END,
                     next_attempt_at_ms = CASE WHEN ?1 = 'pending' AND {resolved}
                                  THEN NULL ELSE ?2 END,
                     last_error_code = ?3, updated_at_ms = ?4
                 WHERE delivery_id = ?5 AND state = 'sending' AND attempt_count = ?6"
                ),
                params![
                    state.as_str(),
                    retry_at_ms,
                    error_code,
                    now_ms,
                    delivery_id,
                    expected_attempt_count
                ],
            )?;
            Ok(if changed == 1 {
                ImDeliverySettlementResult::Applied
            } else {
                ImDeliverySettlementResult::StaleClaim
            })
        })
    }

    pub fn binding(
        &self,
        delivery_id: &str,
    ) -> Result<Option<ImDeliveryBinding>, ImRepositoryError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT channel_kind, platform_message_id, platform_chat_id, update_token_ref
                     FROM im_delivery_bindings WHERE delivery_id = ?1",
                    params![delivery_id],
                    |row| {
                        let channel = row.get::<_, String>(0)?;
                        Ok((
                            channel,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .optional()?
                .map(
                    |(channel, platform_message_id, platform_chat_id, update_token_ref)| {
                        Ok(ImDeliveryBinding {
                            delivery_id: delivery_id.to_owned(),
                            channel: ImChannelKind::parse(&channel)
                                .ok_or(ImRepositoryError::InvalidStoredData)?,
                            platform_message_id,
                            platform_chat_id,
                            update_token_ref,
                        })
                    },
                )
                .transpose()
        })
    }

    /// Reschedules a claimed delivery whose prerequisite has not settled yet.
    /// This is not a connector failure and must not overwrite its error history.
    pub(crate) fn defer_delivery(
        &self,
        delivery_id: &str,
        expected_attempt_count: u32,
        retry_at_ms: i64,
        now_ms: i64,
    ) -> Result<ImDeliverySettlementResult, ImRepositoryError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "UPDATE im_outbox SET state = 'pending', next_attempt_at_ms = ?1, updated_at_ms = ?2
                 WHERE delivery_id = ?3 AND state = 'sending' AND attempt_count = ?4",
                params![retry_at_ms, now_ms, delivery_id, expected_attempt_count],
            )?;
            Ok(if changed == 1 {
                ImDeliverySettlementResult::Applied
            } else {
                ImDeliverySettlementResult::StaleClaim
            })
        })
    }

    pub(crate) fn delivery_state(
        &self,
        delivery_id: &str,
    ) -> Result<Option<ImDeliveryState>, ImRepositoryError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT state FROM im_outbox WHERE delivery_id = ?1",
                    params![delivery_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| {
                    ImDeliveryState::parse(&value).ok_or(ImRepositoryError::InvalidStoredData)
                })
                .transpose()
        })
    }

    pub fn record_inbound_result(
        &self,
        result: &ImInboundActionResult,
    ) -> Result<bool, ImRepositoryError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "INSERT OR IGNORE INTO im_inbound_actions (
                    action_id, channel_kind, canonical_event_id, actor_id_hash,
                    platform_event_id, result_code, result_revision,
                    received_at_ms, completed_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    result.action_id,
                    result.channel.as_str(),
                    result.canonical_event_id,
                    result.actor_id_hash,
                    result.platform_event_id,
                    result.outcome.as_str(),
                    result.result_revision,
                    result.received_at_ms,
                    result.completed_at_ms,
                ],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn inbound_result(
        &self,
        channel: ImChannelKind,
        platform_event_id: &str,
    ) -> Result<Option<ImInboundActionResult>, ImRepositoryError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT action_id, canonical_event_id, actor_id_hash, result_code,
                            result_revision, received_at_ms, completed_at_ms
                     FROM im_inbound_actions
                     WHERE channel_kind = ?1 AND platform_event_id = ?2",
                    params![channel.as_str(), platform_event_id],
                    |row| {
                        Ok((
                            ImInboundActionResult {
                                action_id: row.get(0)?,
                                channel,
                                canonical_event_id: row.get(1)?,
                                actor_id_hash: row.get(2)?,
                                platform_event_id: platform_event_id.to_string(),
                                outcome: ImInboundOutcome::Accepted,
                                result_revision: row.get(4)?,
                                received_at_ms: row.get(5)?,
                                completed_at_ms: row.get(6)?,
                            },
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?
                .map(with_stored_outcome)
                .transpose()
        })
    }

    pub fn successful_inbound_result_for_canonical_event(
        &self,
        channel: ImChannelKind,
        canonical_event_id: &str,
    ) -> Result<Option<ImInboundActionResult>, ImRepositoryError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT action_id, canonical_event_id, actor_id_hash, platform_event_id,
                            result_code, result_revision, received_at_ms, completed_at_ms
                     FROM im_inbound_actions INDEXED BY idx_im_inbound_actions_canonical_event
                     WHERE channel_kind = ?1
                       AND canonical_event_id = ?2
                       AND result_code IN (?3, ?4)
                     ORDER BY completed_at_ms DESC, action_id DESC
                     LIMIT 1",
                    params![
                        channel.as_str(),
                        canonical_event_id,
                        ImInboundOutcome::ACCEPTED,
                        ImInboundOutcome::ALREADY_APPLIED,
                    ],
                    |row| {
                        Ok((
                            ImInboundActionResult {
                                action_id: row.get(0)?,
                                channel,
                                canonical_event_id: row.get(1)?,
                                actor_id_hash: row.get(2)?,
                                platform_event_id: row.get(3)?,
                                outcome: ImInboundOutcome::Accepted,
                                result_revision: row.get(5)?,
                                received_at_ms: row.get(6)?,
                                completed_at_ms: row.get(7)?,
                            },
                            row.get::<_, String>(4)?,
                        ))
                    },
                )
                .optional()?
                .map(with_stored_outcome)
                .transpose()
        })
    }

    pub fn delivery(&self, delivery_id: &str) -> Result<Option<ImDelivery>, ImRepositoryError> {
        self.with_connection(|connection| {
            let row = connection
                .query_row(
                    "SELECT rowid, channel_kind, destination_json, notification_kind,
                            canonical_event_id, payload_json, expires_at_ms, attempt_count
                     FROM im_outbox WHERE delivery_id = ?1",
                    params![delivery_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(1)?,
                            StoredDeliveryRow {
                                row_id: row.get(0)?,
                                delivery_id: delivery_id.to_string(),
                                destination_json: row.get(2)?,
                                notification_kind: row.get(3)?,
                                canonical_event_id: row.get(4)?,
                                payload_json: row.get(5)?,
                                expires_at_ms: row.get(6)?,
                                attempt_count: row.get(7)?,
                            },
                        ))
                    },
                )
                .optional()?;
            row.map(|(channel, row)| {
                let channel =
                    ImChannelKind::parse(&channel).ok_or(ImRepositoryError::InvalidStoredData)?;
                row.into_delivery(channel)
            })
            .transpose()
        })
    }

    pub fn cleanup_retained(
        &self,
        outbox_before_ms: i64,
        inbound_before_ms: i64,
        requested_limit: usize,
    ) -> Result<(usize, usize), ImRepositoryError> {
        let limit = requested_limit.clamp(1, MAX_IM_CLEANUP_BATCH);
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let outbox = transaction.execute(
                "DELETE FROM im_outbox WHERE delivery_id IN (
                    SELECT delivery_id FROM im_outbox INDEXED BY idx_im_outbox_retention
                    WHERE state IN ('sent', 'expired', 'dead_letter') AND updated_at_ms < ?1
                    ORDER BY updated_at_ms, delivery_id LIMIT ?2
                 )",
                params![outbox_before_ms, limit],
            )?;
            let inbound = transaction.execute(
                "DELETE FROM im_inbound_actions WHERE action_id IN (
                    SELECT action_id FROM im_inbound_actions
                    INDEXED BY idx_im_inbound_actions_retention
                    WHERE completed_at_ms < ?1
                    ORDER BY completed_at_ms, action_id LIMIT ?2
                 )",
                params![inbound_before_ms, limit],
            )?;
            transaction.commit()?;
            Ok((outbox, inbound))
        })
    }

    pub fn delete_active_for_channel(
        &self,
        channel: ImChannelKind,
    ) -> Result<usize, ImRepositoryError> {
        self.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM im_outbox
                     WHERE channel_kind = ?1 AND state IN ('pending', 'sending')",
                    params![channel.as_str()],
                )
                .map_err(Into::into)
        })
    }

    pub fn create_channel_cleanup(
        &self,
        operation_id: &str,
        channel: ImChannelKind,
        credential_ref: Option<&str>,
        now_ms: i64,
    ) -> Result<(), ImRepositoryError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO im_channel_cleanup_operations (
                    operation_id, channel_kind, credential_ref, phase,
                    created_at_ms, updated_at_ms, last_error_code
                 ) VALUES (?1, ?2, ?3, 'prepared', ?4, ?4, NULL)",
                params![operation_id, channel.as_str(), credential_ref, now_ms],
            )?;
            Ok(())
        })
    }

    pub fn channel_cleanup(
        &self,
        channel: ImChannelKind,
    ) -> Result<Option<ImChannelCleanupOperation>, ImRepositoryError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT operation_id, channel_kind, credential_ref, phase, last_error_code
                     FROM im_channel_cleanup_operations WHERE channel_kind = ?1",
                    params![channel.as_str()],
                    cleanup_operation_from_row,
                )
                .optional()?
                .map(parse_cleanup_operation)
                .transpose()
        })
    }

    pub fn pending_channel_cleanups(
        &self,
    ) -> Result<Vec<ImChannelCleanupOperation>, ImRepositoryError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT operation_id, channel_kind, credential_ref, phase, last_error_code
                 FROM im_channel_cleanup_operations ORDER BY created_at_ms, operation_id",
            )?;
            statement
                .query_map([], cleanup_operation_from_row)?
                .map(|row| parse_cleanup_operation(row?))
                .collect()
        })
    }

    pub fn update_channel_cleanup(
        &self,
        operation_id: &str,
        phase: ImChannelCleanupPhase,
        last_error_code: Option<&str>,
        now_ms: i64,
    ) -> Result<bool, ImRepositoryError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "UPDATE im_channel_cleanup_operations
                 SET phase = ?1, last_error_code = ?2, updated_at_ms = ?3
                 WHERE operation_id = ?4",
                params![phase.as_str(), last_error_code, now_ms, operation_id],
            )? == 1)
        })
    }

    pub fn finish_channel_cleanup(&self, operation_id: &str) -> Result<bool, ImRepositoryError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "DELETE FROM im_channel_cleanup_operations WHERE operation_id = ?1",
                params![operation_id],
            )? == 1)
        })
    }

    #[cfg(test)]
    fn state(&self, delivery_id: &str) -> Result<ImDeliveryState, ImRepositoryError> {
        self.delivery_state(delivery_id)?
            .ok_or(ImRepositoryError::InvalidStoredData)
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, ImRepositoryError>,
    ) -> Result<T, ImRepositoryError> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| ImRepositoryError::LockUnavailable)?;
        if guard.is_none() {
            *guard = Some(open_connection(&self.path)?);
        }
        operation(
            guard
                .as_mut()
                .expect("IM repository connection initialized"),
        )
    }
}

fn with_stored_outcome(
    (mut result, outcome): (ImInboundActionResult, String),
) -> Result<ImInboundActionResult, ImRepositoryError> {
    result.outcome =
        ImInboundOutcome::parse(&outcome).ok_or(ImRepositoryError::InvalidStoredData)?;
    Ok(result)
}

fn cleanup_operation_from_row(
    row: &rusqlite::Row<'_>,
) -> Result<(String, String, Option<String>, String, Option<String>), rusqlite::Error> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn parse_cleanup_operation(
    (operation_id, channel, credential_ref, phase, last_error_code): (
        String,
        String,
        Option<String>,
        String,
        Option<String>,
    ),
) -> Result<ImChannelCleanupOperation, ImRepositoryError> {
    Ok(ImChannelCleanupOperation {
        operation_id,
        channel: ImChannelKind::parse(&channel).ok_or(ImRepositoryError::InvalidStoredData)?,
        credential_ref,
        phase: ImChannelCleanupPhase::parse(&phase).ok_or(ImRepositoryError::InvalidStoredData)?,
        last_error_code,
    })
}

struct StoredDeliveryRow {
    row_id: i64,
    delivery_id: String,
    destination_json: String,
    notification_kind: String,
    canonical_event_id: String,
    payload_json: String,
    expires_at_ms: i64,
    attempt_count: u32,
}

impl StoredDeliveryRow {
    fn into_delivery(self, channel: ImChannelKind) -> Result<ImDelivery, ImRepositoryError> {
        let destination: ImDestination = serde_json::from_str(&self.destination_json)
            .map_err(|_| ImRepositoryError::InvalidStoredData)?;
        let notification_kind = ImNotificationKind::parse(&self.notification_kind)
            .ok_or(ImRepositoryError::InvalidStoredData)?;
        let payload: ImDeliveryPayload = serde_json::from_str(&self.payload_json)
            .map_err(|_| ImRepositoryError::InvalidStoredData)?;
        let delivery = ImDelivery {
            delivery_id: self.delivery_id,
            channel,
            destination,
            notification_kind,
            canonical_event_id: self.canonical_event_id,
            payload,
            expires_at_ms: self.expires_at_ms,
            display_ref: Some(display_ref_from_row_id(self.row_id)),
        };
        validate_delivery(&delivery)?;
        Ok(delivery)
    }

    fn into_claimed(self, channel: ImChannelKind) -> Result<ClaimedImDelivery, ImRepositoryError> {
        let attempt_count = self.attempt_count;
        let delivery = self.into_delivery(channel)?;
        Ok(ClaimedImDelivery {
            delivery,
            attempt_count: attempt_count.saturating_add(1),
        })
    }
}

fn validate_delivery(delivery: &ImDelivery) -> Result<(), ImRepositoryError> {
    let expected_id = ImDelivery::deterministic_id(
        delivery.channel,
        &delivery.destination.destination_id,
        delivery.notification_kind,
        &delivery.canonical_event_id,
    );
    if delivery.delivery_id != expected_id
        || delivery.destination.destination_id.trim().is_empty()
        || delivery.destination.conversation_id.trim().is_empty()
        || delivery.destination.authorized_actor_id.trim().is_empty()
        || delivery.canonical_event_id.trim().is_empty()
        || !delivery.payload.validate_for(delivery.notification_kind)
    {
        return Err(ImRepositoryError::InvalidPayload);
    }
    Ok(())
}

fn display_ref_from_row_id(row_id: i64) -> u16 {
    (row_id.rem_euclid(IM_DELIVERY_DISPLAY_REF_MODULUS)) as u16
}

fn open_connection(path: &Utf8Path) -> Result<Connection, ImRepositoryError> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut connection = Connection::open(path.as_std_path())?;
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = FULL;",
    )?;
    ensure_schema(&mut connection)?;
    Ok(connection)
}

fn ensure_schema(connection: &mut Connection) -> Result<(), ImRepositoryError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS core_schema (
            component TEXT PRIMARY KEY NOT NULL,
            version INTEGER NOT NULL
         );",
    )?;
    let version = connection
        .query_row(
            "SELECT version FROM core_schema WHERE component = 'im_remote_intervention'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if version.is_some_and(|found| found > IM_SCHEMA_VERSION) {
        return Err(ImRepositoryError::SchemaTooNew {
            found: version.unwrap_or_default(),
            supported: IM_SCHEMA_VERSION,
        });
    }
    if version == Some(IM_SCHEMA_VERSION) {
        return Ok(());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS im_outbox (
            delivery_id TEXT PRIMARY KEY NOT NULL,
            channel_kind TEXT NOT NULL,
            destination_id TEXT NOT NULL,
            destination_json TEXT NOT NULL,
            notification_kind TEXT NOT NULL,
            canonical_event_id TEXT NOT NULL,
            payload_kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            state TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER,
            expires_at_ms INTEGER NOT NULL,
            last_error_code TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            CHECK (payload_kind IN ('intervention', 'information')),
            CHECK (state IN ('pending', 'sending', 'sent', 'expired', 'dead_letter')),
            UNIQUE(channel_kind, destination_id, notification_kind, canonical_event_id)
         );
         CREATE INDEX IF NOT EXISTS idx_im_outbox_due
         ON im_outbox(channel_kind, state, next_attempt_at_ms, created_at_ms);
         CREATE INDEX IF NOT EXISTS idx_im_outbox_active
         ON im_outbox(state) WHERE state IN ('pending', 'sending');
         CREATE INDEX IF NOT EXISTS idx_im_outbox_retention
         ON im_outbox(state, updated_at_ms);
         CREATE INDEX IF NOT EXISTS idx_im_outbox_canonical_event
         ON im_outbox(canonical_event_id, notification_kind, channel_kind);

         CREATE TABLE IF NOT EXISTS im_delivery_bindings (
            delivery_id TEXT PRIMARY KEY NOT NULL REFERENCES im_outbox(delivery_id) ON DELETE CASCADE,
            channel_kind TEXT NOT NULL,
            platform_message_id TEXT NOT NULL,
            platform_chat_id TEXT NOT NULL,
            update_token_ref TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE(channel_kind, platform_message_id)
         );

         CREATE TABLE IF NOT EXISTS im_inbound_actions (
            action_id TEXT PRIMARY KEY NOT NULL,
            channel_kind TEXT NOT NULL,
            canonical_event_id TEXT,
            actor_id_hash TEXT NOT NULL,
            platform_event_id TEXT NOT NULL,
            result_code TEXT NOT NULL,
            result_revision INTEGER,
            received_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER NOT NULL,
            UNIQUE(channel_kind, platform_event_id)
         );
         CREATE INDEX IF NOT EXISTS idx_im_inbound_actions_retention
         ON im_inbound_actions(completed_at_ms);
         CREATE INDEX IF NOT EXISTS idx_im_inbound_actions_canonical_event
         ON im_inbound_actions(channel_kind, canonical_event_id, completed_at_ms);

         CREATE TABLE IF NOT EXISTS im_channel_cleanup_operations (
            operation_id TEXT PRIMARY KEY NOT NULL,
            channel_kind TEXT NOT NULL UNIQUE,
            credential_ref TEXT,
            phase TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_error_code TEXT,
            CHECK (phase IN ('prepared', 'settings_removed', 'projection_drained', 'outbox_removed'))
         );",
    )?;
    if version.is_some_and(|found| found < IM_PAYLOAD_V2_SCHEMA_VERSION) {
        transaction.execute_batch(
            "DELETE FROM im_outbox WHERE payload_kind = 'intervention';
             DELETE FROM im_outbox WHERE canonical_event_id LIKE '%:desktop-resolved';",
        )?;
    }
    if version.is_some_and(|found| found < IM_RESOLUTION_CONTENT_SCHEMA_VERSION) {
        // Keep identity, bindings and tombstones: dropping a resolution here
        // would allow a delayed source projection to resurrect a handled card.
        // Old rows never stored answers; make that absence explicit instead of
        // inventing an empty submitted form or retaining a legacy decoder.
        transaction.execute_batch(
            "UPDATE im_outbox
             SET payload_json = json_set(payload_json,
                 '$.data.resolution.outcome', json('{\"kind\":\"resultUnavailable\"}'))
             WHERE json_valid(payload_json)
               AND json_extract(payload_json, '$.kind') = 'interventionResolution'
               AND json_extract(payload_json, '$.data.version') = 2
               AND json_extract(payload_json, '$.data.resolution.outcome.kind') = 'elicitationSubmitted';
             UPDATE im_outbox
             SET payload_json = json_set(payload_json, '$.data.version', 3)
             WHERE json_valid(payload_json)
               AND json_extract(payload_json, '$.data.version') IN (1, 2);",
        )?;
    }
    transaction.execute(
        "INSERT INTO core_schema(component, version)
         VALUES ('im_remote_intervention', ?1)
         ON CONFLICT(component) DO UPDATE SET version = excluded.version",
        params![IM_SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::app::intervention::{
        InterventionAllowedAction, InterventionLocator, InterventionRequestIdentity,
    };
    use crate::im::{
        IM_PAYLOAD_VERSION, ImNavigationLocator, ImResolutionOutcome, ImResolvedBy,
        InformationalNotification, InterventionPresentation, InterventionRef,
        InterventionResolutionNotification,
    };

    fn repository(limit: usize) -> (TempDir, ImRepository) {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        (temp, ImRepository::with_active_limit(path, limit))
    }

    #[test]
    fn empty_due_probe_does_not_request_a_sqlite_writer_lock() {
        let (_temp, repository) = repository(DEFAULT_IM_OUTBOX_ACTIVE_LIMIT);
        assert!(
            repository
                .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 1)
                .unwrap()
                .is_empty()
        );

        let writer = Connection::open(repository.path().as_std_path()).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let result = repository.claim_due(ImChannelKind::WeCom, 101, Duration::from_secs(30), 1);
        writer.execute_batch("ROLLBACK").unwrap();

        assert!(
            matches!(result, Ok(ref deliveries) if deliveries.is_empty()),
            "an empty due probe must remain read-only: {result:?}"
        );
    }

    #[test]
    fn pending_cleanup_probe_does_not_create_an_empty_database() {
        let temp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        assert!(!ImRepository::has_pending_channel_cleanups_at(&path).unwrap());
        assert!(!path.exists());

        let repository = ImRepository::new(path.clone());
        repository
            .create_channel_cleanup("cleanup-1", ImChannelKind::WeCom, None, 100)
            .unwrap();
        assert!(ImRepository::has_pending_channel_cleanups_at(&path).unwrap());
    }

    fn information_delivery(
        channel: ImChannelKind,
        destination_id: &str,
        event_id: &str,
    ) -> ImDelivery {
        let destination = ImDestination {
            destination_id: destination_id.into(),
            conversation_id: format!("chat-{destination_id}"),
            authorized_actor_id: format!("actor-{destination_id}"),
        };
        let kind = ImNotificationKind::RunFailure;
        ImDelivery {
            delivery_id: ImDelivery::deterministic_id(channel, destination_id, kind, event_id),
            channel,
            destination,
            notification_kind: kind,
            canonical_event_id: event_id.into(),
            payload: ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: event_id.into(),
                    notification_kind: kind,
                    locator: ImNavigationLocator {
                        project_id: "project-1".into(),
                        task_id: Some("task-1".into()),
                        run_id: Some("run-1".into()),
                        round_id: Some("round-1".into()),
                        node_id: Some("node-1".into()),
                        attempt_id: Some("attempt-1".into()),
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    outcome: "failure".into(),
                    summary_key: "im.notification.runFailure.summary".into(),
                    parameters: Default::default(),
                    missed_count: None,
                },
            },
            expires_at_ms: 10_000,
            display_ref: None,
        }
    }

    fn intervention_delivery(
        channel: ImChannelKind,
        destination_id: &str,
        event_id: &str,
    ) -> ImDelivery {
        let destination = ImDestination {
            destination_id: destination_id.into(),
            conversation_id: format!("chat-{destination_id}"),
            authorized_actor_id: format!("actor-{destination_id}"),
        };
        let kind = ImNotificationKind::Permission;
        ImDelivery {
            delivery_id: ImDelivery::deterministic_id(channel, destination_id, kind, event_id),
            channel,
            destination,
            notification_kind: kind,
            canonical_event_id: event_id.into(),
            payload: ImDeliveryPayload::Intervention {
                version: IM_PAYLOAD_VERSION,
                reference: InterventionRef {
                    locator: InterventionLocator {
                        project_id: "project-1".into(),
                        task_id: "task-1".into(),
                        run_id: "run-1".into(),
                        round_id: "round-1".into(),
                        node_id: "node-1".into(),
                        attempt_id: "attempt-1".into(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    request: InterventionRequestIdentity::Permission {
                        request_id: "permission-1".into(),
                    },
                    expected_state: "state-1".into(),
                    allowed_actions: vec![InterventionAllowedAction::PermissionOption {
                        option_id: "allow_once".into(),
                        name: "Allow once".into(),
                        permission_kind: crate::app::intervention::PermissionActionKind::AllowOnce,
                    }],
                    expires_at_ms: None,
                },
                presentation: InterventionPresentation {
                    title_key: "im.notification.permission.title".into(),
                    summary_key: "im.notification.permission.summary".into(),
                    title: None,
                    summary: None,
                    body: None,
                    context: None,
                    fields: BTreeMap::new(),
                    questions: Vec::new(),
                },
            },
            expires_at_ms: 10_000,
            display_ref: None,
        }
    }

    fn resolution_delivery(source: &ImDelivery) -> ImDelivery {
        let event_id = resolution_event_id(&source.canonical_event_id);
        ImDelivery {
            delivery_id: ImDelivery::deterministic_id(
                source.channel,
                &source.destination.destination_id,
                source.notification_kind,
                &event_id,
            ),
            channel: source.channel,
            destination: source.destination.clone(),
            notification_kind: source.notification_kind,
            canonical_event_id: event_id.clone(),
            payload: ImDeliveryPayload::InterventionResolution {
                version: IM_PAYLOAD_VERSION,
                resolution: InterventionResolutionNotification {
                    canonical_event_id: event_id,
                    source_event_id: source.canonical_event_id.clone(),
                    notification_kind: source.notification_kind,
                    locator: ImNavigationLocator {
                        project_id: "project-1".into(),
                        task_id: Some("task-1".into()),
                        run_id: Some("run-1".into()),
                        round_id: Some("round-1".into()),
                        node_id: Some("node-1".into()),
                        attempt_id: Some("attempt-1".into()),
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    display_ref: source.display_ref,
                    resolved_by: ImResolvedBy::Desktop,
                    outcome: ImResolutionOutcome::PermissionOption {
                        option_id: "allow".into(),
                    },
                },
            },
            expires_at_ms: 20_000,
            display_ref: source.display_ref,
        }
    }

    #[test]
    fn a_resolved_source_is_never_requeued_by_failure_or_lease_recovery() {
        let (_temp, repository) = repository(10);
        let failed = intervention_delivery(ImChannelKind::Feishu, "user-1", "event-failed");
        let leased = intervention_delivery(ImChannelKind::Feishu, "user-1", "event-leased");
        let unresolved = intervention_delivery(ImChannelKind::Feishu, "user-1", "event-open");
        for delivery in [&failed, &leased, &unresolved] {
            repository.enqueue(delivery, 100).unwrap();
        }
        let claimed = repository
            .claim_due(ImChannelKind::Feishu, 100, Duration::from_millis(50), 10)
            .unwrap();
        assert_eq!(claimed.len(), 3);
        // The intervention resolves while its source card is still being sent.
        for source in [&failed, &leased] {
            repository
                .enqueue_resolution_and_expire_pending_source(
                    &source.canonical_event_id,
                    &resolution_delivery(source),
                    ImResolutionDispatch::Send,
                    120,
                )
                .unwrap();
        }

        repository
            .mark_failed(&failed.delivery_id, 1, Some(130), "IM_NETWORK", 125)
            .unwrap();
        repository
            .mark_failed(&unresolved.delivery_id, 1, Some(130), "IM_NETWORK", 125)
            .unwrap();
        assert_eq!(repository.recover_expired_leases(150, 10).unwrap(), 1);

        for (source, expected) in [
            (&failed, ImDeliveryState::Expired),
            (&leased, ImDeliveryState::Expired),
            (&unresolved, ImDeliveryState::Pending),
        ] {
            assert_eq!(
                repository.delivery_state(&source.delivery_id).unwrap(),
                Some(expected),
                "{}",
                source.canonical_event_id
            );
        }
        let resent = repository
            .claim_due(ImChannelKind::Feishu, 200, Duration::from_millis(50), 10)
            .unwrap()
            .into_iter()
            .map(|claimed| claimed.delivery.canonical_event_id)
            .collect::<Vec<_>>();
        assert!(!resent.contains(&failed.canonical_event_id));
        assert!(!resent.contains(&leased.canonical_event_id));
        assert!(resent.contains(&unresolved.canonical_event_id));
    }

    #[test]
    fn semantic_replay_is_deduplicated_per_target_but_distinct_events_are_not() {
        let (_temp, repository) = repository(10);
        let first = information_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        assert_eq!(
            repository.enqueue(&first, 100).unwrap(),
            ImDeliveryInsertResult::Inserted
        );
        assert_eq!(
            repository.enqueue(&first, 101).unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
        let second = information_delivery(ImChannelKind::WeCom, "user-1", "event-2");
        assert_eq!(
            repository.enqueue(&second, 102).unwrap(),
            ImDeliveryInsertResult::Inserted
        );
        let other_target = information_delivery(ImChannelKind::WeCom, "user-2", "event-1");
        assert_eq!(
            repository.enqueue(&other_target, 103).unwrap(),
            ImDeliveryInsertResult::Inserted
        );
    }

    #[test]
    fn send_resolution_atomically_supersedes_pending_source_and_is_idempotent() {
        let (_temp, repository) = repository(1);
        let source = intervention_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        repository.enqueue(&source, 100).unwrap();
        let indexed = repository
            .intervention_deliveries_for_canonical_event("event-1")
            .unwrap();
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].display_ref, Some(1));

        let resolution = resolution_delivery(&indexed[0]);
        assert_eq!(
            repository
                .enqueue_resolution_and_expire_pending_source(
                    "event-1",
                    &resolution,
                    ImResolutionDispatch::Send,
                    200,
                )
                .unwrap(),
            ImDeliveryInsertResult::Inserted
        );
        assert_eq!(
            repository.state(&source.delivery_id).unwrap(),
            ImDeliveryState::Expired
        );
        assert_eq!(
            repository.state(&resolution.delivery_id).unwrap(),
            ImDeliveryState::Pending
        );
        assert_eq!(
            repository
                .enqueue_resolution_and_expire_pending_source(
                    "event-1",
                    &resolution,
                    ImResolutionDispatch::Send,
                    201,
                )
                .unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
    }

    #[test]
    fn send_resolution_preserves_a_source_that_was_already_sent() {
        let (_temp, repository) = repository(2);
        let source = intervention_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        repository.enqueue(&source, 100).unwrap();
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 1)
            .unwrap();
        repository
            .mark_sent(
                &source.delivery_id,
                source.channel,
                claimed[0].attempt_count,
                None,
                110,
            )
            .unwrap();
        let resolution = resolution_delivery(&claimed[0].delivery);
        repository
            .enqueue_resolution_and_expire_pending_source(
                "event-1",
                &resolution,
                ImResolutionDispatch::Send,
                200,
            )
            .unwrap();
        assert_eq!(
            repository.state(&source.delivery_id).unwrap(),
            ImDeliveryState::Sent
        );
        assert_eq!(
            repository.state(&resolution.delivery_id).unwrap(),
            ImDeliveryState::Pending
        );
    }

    #[test]
    fn tombstone_resolution_is_expired_uncounted_and_blocks_a_late_source() {
        let (_temp, repository) = repository(1);
        let busy = information_delivery(ImChannelKind::WeCom, "user-2", "event-busy");
        repository.enqueue(&busy, 100).unwrap();
        let source = intervention_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        let tombstone = resolution_delivery(&source);

        assert_eq!(
            repository
                .enqueue_resolution_and_expire_pending_source(
                    "event-1",
                    &tombstone,
                    ImResolutionDispatch::TombstoneOnly,
                    200,
                )
                .unwrap(),
            ImDeliveryInsertResult::Inserted
        );
        assert_eq!(
            repository.state(&tombstone.delivery_id).unwrap(),
            ImDeliveryState::Expired
        );
        assert_eq!(
            repository.enqueue(&source, 300).unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
        assert!(repository.delivery(&source.delivery_id).unwrap().is_none());
        let due = repository
            .claim_due(ImChannelKind::WeCom, 300, Duration::from_secs(30), 10)
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].delivery.delivery_id, busy.delivery_id);
        assert_eq!(
            repository
                .enqueue_resolution_and_expire_pending_source(
                    "event-1",
                    &tombstone,
                    ImResolutionDispatch::TombstoneOnly,
                    301,
                )
                .unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
    }

    #[test]
    fn capacity_due_batch_and_lease_recovery_are_bounded() {
        let (_temp, repository) = repository(2);
        let first = information_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        let second = information_delivery(ImChannelKind::WeCom, "user-1", "event-2");
        repository.enqueue(&first, 100).unwrap();
        repository.enqueue(&second, 100).unwrap();
        let third = information_delivery(ImChannelKind::WeCom, "user-1", "event-3");
        assert!(matches!(
            repository.enqueue(&third, 100),
            Err(ImRepositoryError::Capacity { limit: 2 })
        ));

        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_millis(50), 200)
            .unwrap();
        assert_eq!(claimed.len(), 2);
        assert_eq!(
            repository.state(&first.delivery_id).unwrap(),
            ImDeliveryState::Sending
        );
        assert_eq!(repository.recover_expired_leases(149, 200).unwrap(), 0);
        assert_eq!(repository.recover_expired_leases(150, 1).unwrap(), 1);
    }

    #[test]
    fn corrupt_due_row_is_dead_lettered_without_blocking_healthy_rows() {
        let (_temp, repository) = repository(10);
        let corrupt = information_delivery(ImChannelKind::WeCom, "user-1", "event-corrupt");
        let healthy = information_delivery(ImChannelKind::WeCom, "user-1", "event-healthy");
        repository.enqueue(&corrupt, 100).unwrap();
        repository.enqueue(&healthy, 101).unwrap();
        repository
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE im_outbox SET payload_json = '{' WHERE delivery_id = ?1",
                    params![corrupt.delivery_id],
                )?;
                Ok(())
            })
            .unwrap();

        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 101, Duration::from_secs(30), 2)
            .unwrap();

        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].delivery.delivery_id, healthy.delivery_id);
        assert_eq!(
            repository.state(&corrupt.delivery_id).unwrap(),
            ImDeliveryState::DeadLetter
        );
        let error_code = repository
            .with_connection(|connection| {
                Ok(connection.query_row(
                    "SELECT last_error_code FROM im_outbox WHERE delivery_id = ?1",
                    params![corrupt.delivery_id],
                    |row| row.get::<_, Option<String>>(0),
                )?)
            })
            .unwrap();
        assert_eq!(error_code.as_deref(), Some("IM_PAYLOAD_INVALID"));
        assert_eq!(repository.recover_expired_leases(200, 10).unwrap(), 0);
    }

    #[test]
    fn lease_recovery_expires_invalid_delivery_and_fences_late_settlement() {
        let (_temp, repository) = repository(10);
        let valid = information_delivery(ImChannelKind::WeCom, "user-1", "event-valid");
        let mut expired = information_delivery(ImChannelKind::WeCom, "user-1", "event-expired");
        expired.expires_at_ms = 120;
        repository.enqueue(&valid, 100).unwrap();
        repository.enqueue(&expired, 100).unwrap();
        let first_claim = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_millis(50), 2)
            .unwrap();
        assert_eq!(repository.recover_expired_leases(150, 2).unwrap(), 2);
        assert_eq!(
            repository.state(&expired.delivery_id).unwrap(),
            ImDeliveryState::Expired
        );

        let second_claim = repository
            .claim_due(ImChannelKind::WeCom, 150, Duration::from_millis(50), 2)
            .unwrap();
        assert_eq!(second_claim.len(), 1);
        assert_eq!(second_claim[0].delivery.delivery_id, valid.delivery_id);
        assert_eq!(second_claim[0].attempt_count, 2);
        let stale_attempt = first_claim
            .iter()
            .find(|claim| claim.delivery.delivery_id == valid.delivery_id)
            .unwrap();
        assert_eq!(
            repository
                .mark_sent(
                    &valid.delivery_id,
                    valid.channel,
                    stale_attempt.attempt_count,
                    None,
                    151,
                )
                .unwrap(),
            ImDeliverySettlementResult::StaleClaim
        );
        assert_eq!(
            repository.state(&valid.delivery_id).unwrap(),
            ImDeliveryState::Sending
        );
        assert_eq!(
            repository
                .mark_failed(
                    &valid.delivery_id,
                    second_claim[0].attempt_count,
                    None,
                    "IM_PROTOCOL_INVALID",
                    152,
                )
                .unwrap(),
            ImDeliverySettlementResult::Applied
        );
        assert_eq!(
            repository.state(&valid.delivery_id).unwrap(),
            ImDeliveryState::DeadLetter
        );
    }

    #[test]
    fn channel_cleanup_journal_is_durable_and_idempotently_advanced() {
        let (temp, repository) = repository(10);
        repository
            .create_channel_cleanup(
                "operation-1",
                ImChannelKind::WeCom,
                Some("credential-1"),
                100,
            )
            .unwrap();
        for (index, phase) in [
            ImChannelCleanupPhase::SettingsRemoved,
            ImChannelCleanupPhase::ProjectionDrained,
            ImChannelCleanupPhase::OutboxRemoved,
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                repository
                    .update_channel_cleanup(
                        "operation-1",
                        phase,
                        Some("IM_STORAGE_UNAVAILABLE"),
                        101 + index as i64,
                    )
                    .unwrap()
            );
            assert_eq!(
                repository
                    .channel_cleanup(ImChannelKind::WeCom)
                    .unwrap()
                    .unwrap()
                    .phase,
                phase
            );
        }
        drop(repository);

        let reopened =
            ImRepository::new(Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap());
        let operation = reopened
            .channel_cleanup(ImChannelKind::WeCom)
            .unwrap()
            .unwrap();
        assert_eq!(operation.operation_id, "operation-1");
        assert_eq!(operation.credential_ref.as_deref(), Some("credential-1"));
        assert_eq!(operation.phase, ImChannelCleanupPhase::OutboxRemoved);
        assert_eq!(
            operation.last_error_code.as_deref(),
            Some("IM_STORAGE_UNAVAILABLE")
        );
        assert_eq!(
            reopened.pending_channel_cleanups().unwrap(),
            vec![operation]
        );
        assert!(reopened.finish_channel_cleanup("operation-1").unwrap());
        assert!(!reopened.finish_channel_cleanup("operation-1").unwrap());
    }

    #[test]
    fn channel_cleanup_removes_active_deliveries_but_preserves_sent_audit() {
        let (_temp, repository) = repository(10);
        let sent = information_delivery(ImChannelKind::WeCom, "user-1", "event-sent");
        let active = information_delivery(ImChannelKind::WeCom, "user-1", "event-active");
        repository.enqueue(&sent, 100).unwrap();
        repository.enqueue(&active, 101).unwrap();
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 101, Duration::from_secs(30), 2)
            .unwrap();
        let sent_claim = claimed
            .iter()
            .find(|claim| claim.delivery.delivery_id == sent.delivery_id)
            .unwrap();
        repository
            .mark_sent(
                &sent.delivery_id,
                sent.channel,
                sent_claim.attempt_count,
                None,
                102,
            )
            .unwrap();

        assert_eq!(
            repository
                .delete_active_for_channel(ImChannelKind::WeCom)
                .unwrap(),
            1
        );
        assert!(repository.delivery(&active.delivery_id).unwrap().is_none());
        assert!(repository.delivery(&sent.delivery_id).unwrap().is_some());
    }

    #[test]
    fn recent_delivery_display_refs_are_distinct_and_stable_across_retries() {
        let (_temp, repository) = repository(10);
        let first = information_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        let second = information_delivery(ImChannelKind::WeCom, "user-1", "event-2");
        repository.enqueue(&first, 100).unwrap();
        repository.enqueue(&second, 100).unwrap();

        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 2)
            .unwrap();
        assert_eq!(claimed[0].delivery.display_ref, Some(1));
        assert_eq!(claimed[1].delivery.display_ref, Some(2));

        repository.recover_expired_leases(131, 10).unwrap();
        let reclaimed = repository
            .claim_due(ImChannelKind::WeCom, 132, Duration::from_secs(30), 2)
            .unwrap();
        for row in reclaimed {
            let expected = if row.delivery.delivery_id == first.delivery_id {
                Some(1)
            } else {
                Some(2)
            };
            assert_eq!(row.delivery.display_ref, expected);
        }
    }

    #[test]
    fn sent_binding_inbound_idempotency_and_retention_are_durable() {
        let (temp, repository) = repository(10);
        let delivery = information_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        repository.enqueue(&delivery, 100).unwrap();
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 1)
            .unwrap();
        let binding = ImDeliveryBinding {
            delivery_id: delivery.delivery_id.clone(),
            channel: ImChannelKind::WeCom,
            platform_message_id: "message-1".into(),
            platform_chat_id: "chat-1".into(),
            update_token_ref: None,
        };
        repository
            .mark_sent(
                &delivery.delivery_id,
                delivery.channel,
                claimed[0].attempt_count,
                Some(&binding),
                200,
            )
            .unwrap();
        assert_eq!(
            repository.state(&delivery.delivery_id).unwrap(),
            ImDeliveryState::Sent
        );
        let result = ImInboundActionResult {
            action_id: "action-1".into(),
            channel: ImChannelKind::WeCom,
            canonical_event_id: Some("event-1".into()),
            actor_id_hash: "hash".into(),
            platform_event_id: "callback-1".into(),
            outcome: ImInboundOutcome::Accepted,
            result_revision: Some(1),
            received_at_ms: 100,
            completed_at_ms: 200,
        };
        assert!(repository.record_inbound_result(&result).unwrap());
        assert!(!repository.record_inbound_result(&result).unwrap());
        drop(repository);

        let reopened =
            ImRepository::new(Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap());
        assert_eq!(
            reopened
                .inbound_result(ImChannelKind::WeCom, "callback-1")
                .unwrap(),
            Some(result)
        );
        assert_eq!(
            reopened
                .successful_inbound_result_for_canonical_event(ImChannelKind::WeCom, "event-1")
                .unwrap()
                .map(|result| result.platform_event_id),
            Some("callback-1".to_owned())
        );
        assert_eq!(reopened.cleanup_retained(201, 201, 1).unwrap(), (1, 1));
    }

    #[test]
    fn successful_ack_without_platform_message_id_marks_sent_without_binding() {
        let (_temp, repository) = repository(10);
        let delivery = information_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        repository.enqueue(&delivery, 100).unwrap();
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 1)
            .unwrap();

        repository
            .mark_sent(
                &delivery.delivery_id,
                delivery.channel,
                claimed[0].attempt_count,
                None,
                200,
            )
            .unwrap();

        assert_eq!(
            repository.state(&delivery.delivery_id).unwrap(),
            ImDeliveryState::Sent
        );
        assert_eq!(repository.binding(&delivery.delivery_id).unwrap(), None);
    }

    #[test]
    fn current_schema_is_added_to_existing_schema_v1() {
        let (temp, repository) = repository(10);
        repository.with_connection(|_| Ok(())).unwrap();
        drop(repository);
        let path = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "DROP INDEX IF EXISTS idx_im_inbound_actions_canonical_event;
                     UPDATE core_schema SET version = 1 WHERE component = 'im_remote_intervention';",
                )
                .unwrap();
        }

        let reopened = ImRepository::new(path.clone());
        reopened
            .with_connection(|connection| {
                let version = connection.query_row(
                    "SELECT version FROM core_schema WHERE component = 'im_remote_intervention'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(version, IM_SCHEMA_VERSION);
                let index_count = connection.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_im_inbound_actions_canonical_event'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(index_count, 1);
                let cleanup_table_count = connection.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'im_channel_cleanup_operations'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                assert_eq!(cleanup_table_count, 1);
                Ok(())
            })
            .unwrap();
    }

    fn stored_schema_version(path: &Utf8Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT version FROM core_schema WHERE component = 'im_remote_intervention'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    }

    fn set_stored_schema_version(path: &Utf8Path, version: i64) {
        Connection::open(path)
            .unwrap()
            .execute(
                "UPDATE core_schema SET version = ?1 WHERE component = 'im_remote_intervention'",
                params![version],
            )
            .unwrap();
    }

    fn outbox_count(path: &Utf8Path, table: &str) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    }

    /// Seeds a v3 database with a sent intervention (with binding), an old
    /// `desktop-resolved` record, an unrelated information delivery and an
    /// inbound audit row.
    fn seed_schema_v3(temp: &TempDir) -> (Utf8PathBuf, ImDelivery, ImDelivery) {
        let path = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        let repository = ImRepository::new(path.clone());
        let intervention = intervention_delivery(ImChannelKind::WeCom, "user-1", "event-1");
        repository.enqueue(&intervention, 100).unwrap();
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 1)
            .unwrap();
        repository
            .mark_sent(
                &intervention.delivery_id,
                intervention.channel,
                claimed[0].attempt_count,
                Some(&ImDeliveryBinding {
                    delivery_id: intervention.delivery_id.clone(),
                    channel: ImChannelKind::WeCom,
                    platform_message_id: "message-1".into(),
                    platform_chat_id: "chat-1".into(),
                    update_token_ref: None,
                }),
                110,
            )
            .unwrap();
        let old_resolution =
            information_delivery(ImChannelKind::WeCom, "user-1", "event-1:desktop-resolved");
        repository.enqueue(&old_resolution, 120).unwrap();
        let information = information_delivery(ImChannelKind::WeCom, "user-1", "event-2");
        repository.enqueue(&information, 130).unwrap();
        repository
            .record_inbound_result(&ImInboundActionResult {
                action_id: "action-1".into(),
                channel: ImChannelKind::WeCom,
                canonical_event_id: Some("event-1".into()),
                actor_id_hash: "hash".into(),
                platform_event_id: "callback-1".into(),
                outcome: ImInboundOutcome::Accepted,
                result_revision: None,
                received_at_ms: 140,
                completed_at_ms: 140,
            })
            .unwrap();
        drop(repository);
        set_stored_schema_version(&path, 3);
        (path, intervention, information)
    }

    #[test]
    fn schema_v4_upgrade_removes_v1_intervention_records_and_cascades_bindings() {
        let temp = tempfile::tempdir().unwrap();
        let (path, intervention, information) = seed_schema_v3(&temp);
        assert_eq!(outbox_count(&path, "im_delivery_bindings"), 1);

        let reopened = ImRepository::new(path.clone());
        reopened
            .with_connection(|connection| {
                let foreign_keys =
                    connection.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?;
                assert_eq!(foreign_keys, 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(stored_schema_version(&path), IM_SCHEMA_VERSION);
        assert!(
            reopened
                .delivery(&intervention.delivery_id)
                .unwrap()
                .is_none()
        );
        assert!(
            reopened
                .intervention_deliveries_for_canonical_event("event-1")
                .unwrap()
                .is_empty()
        );
        assert!(
            reopened
                .delivery(&information.delivery_id)
                .unwrap()
                .is_some()
        );
        assert_eq!(outbox_count(&path, "im_outbox"), 1);
        assert_eq!(outbox_count(&path, "im_delivery_bindings"), 0);
        assert!(
            reopened
                .inbound_result(ImChannelKind::WeCom, "callback-1")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn failed_schema_upgrade_rolls_back_and_retries_on_next_open() {
        let temp = tempfile::tempdir().unwrap();
        let (path, _, _) = seed_schema_v3(&temp);
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER block_upgrade BEFORE DELETE ON im_outbox
                 BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
            )
            .unwrap();

        let blocked = ImRepository::new(path.clone());
        assert!(matches!(
            blocked.with_connection(|_| Ok(())),
            Err(ImRepositoryError::Sqlite(_))
        ));
        drop(blocked);
        assert_eq!(stored_schema_version(&path), 3);
        assert_eq!(outbox_count(&path, "im_outbox"), 3);
        assert_eq!(outbox_count(&path, "im_delivery_bindings"), 1);

        Connection::open(&path)
            .unwrap()
            .execute_batch("DROP TRIGGER block_upgrade;")
            .unwrap();
        let retried = ImRepository::new(path.clone());
        retried.with_connection(|_| Ok(())).unwrap();
        assert_eq!(stored_schema_version(&path), IM_SCHEMA_VERSION);
        assert_eq!(outbox_count(&path, "im_outbox"), 1);
    }

    #[test]
    fn schema_v5_preserves_sources_bindings_and_tombstones_without_inventing_answers() {
        let temp = tempfile::tempdir().unwrap();
        let (path, source, delayed_source, resolved) = seed_schema_v4(&temp);
        let source_ref = source.display_ref;
        let reopened = ImRepository::new(path.clone());
        assert_eq!(
            reopened
                .delivery(&source.delivery_id)
                .unwrap()
                .unwrap()
                .display_ref,
            source_ref
        );
        assert_eq!(stored_schema_version(&path), IM_SCHEMA_VERSION);
        assert!(reopened.binding(&source.delivery_id).unwrap().is_some());
        let payload = reopened
            .delivery(&resolved.delivery_id)
            .unwrap()
            .unwrap()
            .payload;
        let ImDeliveryPayload::InterventionResolution {
            resolution,
            version,
        } = payload
        else {
            unreachable!()
        };
        assert_eq!(version, IM_PAYLOAD_VERSION);
        assert_eq!(resolution.outcome, ImResolutionOutcome::ResultUnavailable);
        assert_eq!(resolution.resolved_by, ImResolvedBy::Desktop);
        assert_eq!(
            reopened.enqueue(&delayed_source, 200).unwrap(),
            ImDeliveryInsertResult::Duplicate
        );
        assert_eq!(outbox_count(&path, "im_outbox"), 3);
        assert_eq!(outbox_count(&path, "im_delivery_bindings"), 1);
        assert!(
            reopened
                .inbound_result(source.channel, "v4-action")
                .unwrap()
                .is_some()
        );
    }

    fn seed_schema_v4(temp: &TempDir) -> (Utf8PathBuf, ImDelivery, ImDelivery, ImDelivery) {
        let path = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        let repository = ImRepository::new(path.clone());
        let mut source = intervention_delivery(ImChannelKind::Feishu, "user-1", "answered");
        source.notification_kind = ImNotificationKind::Elicitation;
        if let ImDeliveryPayload::Intervention {
            reference,
            presentation,
            ..
        } = &mut source.payload
        {
            reference.request = InterventionRequestIdentity::Elicitation {
                elicitation_id: "elicitation-1".into(),
            };
            reference.allowed_actions = vec![InterventionAllowedAction::ElicitationAccept];
            presentation.title_key = "im.notification.elicitation.title".into();
            presentation.summary_key = "im.notification.elicitation.summary".into();
        }
        source.refresh_id();
        repository.enqueue(&source, 100).unwrap();
        let source = repository.delivery(&source.delivery_id).unwrap().unwrap();
        let claimed = repository
            .claim_due(source.channel, 100, Duration::from_secs(30), 1)
            .unwrap();
        repository
            .mark_sent(
                &source.delivery_id,
                source.channel,
                claimed[0].attempt_count,
                Some(&ImDeliveryBinding {
                    delivery_id: source.delivery_id.clone(),
                    channel: source.channel,
                    platform_message_id: "v4-message".into(),
                    platform_chat_id: "v4-chat".into(),
                    update_token_ref: None,
                }),
                101,
            )
            .unwrap();
        let mut resolved = resolution_delivery(&source);
        if let ImDeliveryPayload::InterventionResolution { resolution, .. } = &mut resolved.payload
        {
            resolution.outcome = ImResolutionOutcome::ElicitationSubmitted {
                content: serde_json::json!({"answer": "not stored in v2"}),
            };
        }
        repository
            .enqueue_resolution_and_expire_pending_source(
                &source.canonical_event_id,
                &resolved,
                ImResolutionDispatch::Send,
                102,
            )
            .unwrap();
        let delayed_source = intervention_delivery(ImChannelKind::Feishu, "user-1", "late-source");
        repository
            .enqueue_resolution_and_expire_pending_source(
                &delayed_source.canonical_event_id,
                &resolution_delivery(&delayed_source),
                ImResolutionDispatch::TombstoneOnly,
                103,
            )
            .unwrap();
        repository
            .record_inbound_result(&ImInboundActionResult {
                action_id: "v4-action".into(),
                channel: source.channel,
                canonical_event_id: Some(source.canonical_event_id.clone()),
                actor_id_hash: "hash".into(),
                platform_event_id: "v4-action".into(),
                outcome: ImInboundOutcome::Accepted,
                result_revision: None,
                received_at_ms: 100,
                completed_at_ms: 101,
            })
            .unwrap();
        drop(repository);
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("UPDATE im_outbox SET payload_json = json_set(payload_json, '$.data.version', 2);
            UPDATE im_outbox SET payload_json = json_set(payload_json, '$.data.resolution.outcome', json('{\"kind\":\"elicitationSubmitted\"}'))
            WHERE canonical_event_id = 'answered:resolved';
            UPDATE core_schema SET version = 4 WHERE component = 'im_remote_intervention';").unwrap();
        (path, source, delayed_source, resolved)
    }

    #[test]
    fn schema_v5_update_failure_rolls_back_payloads_and_version() {
        let temp = tempfile::tempdir().unwrap();
        let (path, source, _, resolved) = seed_schema_v4(&temp);
        let connection = Connection::open(&path).unwrap();
        // Fail the second migration statement, after the outcome conversion,
        // to prove that partial changes also roll back in the same transaction.
        connection
            .execute_batch(
                "CREATE TRIGGER block_v5 BEFORE UPDATE ON im_outbox
            WHEN json_extract(NEW.payload_json, '$.data.version') = 3
            BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
            )
            .unwrap();
        assert!(
            ImRepository::new(path.clone())
                .delivery(&source.delivery_id)
                .is_err()
        );
        assert_eq!(stored_schema_version(&path), 4);
        let version: i64 = connection
            .query_row(
                "SELECT json_extract(payload_json, '$.data.version') FROM im_outbox LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 2);
        let outcome: String = connection.query_row(
            "SELECT json_extract(payload_json, '$.data.resolution.outcome.kind') FROM im_outbox WHERE delivery_id = ?1",
            [&resolved.delivery_id], |row| row.get(0),
        ).unwrap();
        assert_eq!(outcome, "elicitationSubmitted");
        assert_eq!(outbox_count(&path, "im_delivery_bindings"), 1);
        connection.execute_batch("DROP TRIGGER block_v5;").unwrap();
        assert!(
            ImRepository::new(path.clone())
                .delivery(&source.delivery_id)
                .unwrap()
                .is_some()
        );
        assert_eq!(stored_schema_version(&path), IM_SCHEMA_VERSION);
    }

    #[test]
    fn newer_schema_is_rejected_without_changes() {
        let temp = tempfile::tempdir().unwrap();
        let (path, _, _) = seed_schema_v3(&temp);
        set_stored_schema_version(&path, IM_SCHEMA_VERSION + 1);
        let repository = ImRepository::new(path.clone());
        let error = repository.with_connection(|_| Ok(())).unwrap_err();
        assert!(matches!(
            error,
            ImRepositoryError::SchemaTooNew { found, supported }
                if found == IM_SCHEMA_VERSION + 1 && supported == IM_SCHEMA_VERSION
        ));
        assert_eq!(error.code(), "IM_SCHEMA_TOO_NEW");
        assert_eq!(outbox_count(&path, "im_outbox"), 3);
    }

    #[test]
    fn due_and_retention_queries_use_their_indexes() {
        let (_temp, repository) = repository(10);
        repository
            .with_connection(|connection| {
                for (sql, expected) in [
                    (
                        "EXPLAIN QUERY PLAN SELECT delivery_id FROM im_outbox INDEXED BY idx_im_outbox_due WHERE channel_kind = 'wecom' AND state = 'pending' AND next_attempt_at_ms <= 100 ORDER BY next_attempt_at_ms, created_at_ms, delivery_id LIMIT 32",
                        "idx_im_outbox_due",
                    ),
                    (
                        "EXPLAIN QUERY PLAN SELECT delivery_id FROM im_outbox INDEXED BY idx_im_outbox_retention WHERE state IN ('sent', 'expired', 'dead_letter') AND updated_at_ms < 100 ORDER BY updated_at_ms, delivery_id LIMIT 200",
                        "idx_im_outbox_retention",
                    ),
                    (
                        "EXPLAIN QUERY PLAN SELECT action_id FROM im_inbound_actions INDEXED BY idx_im_inbound_actions_canonical_event WHERE channel_kind = 'wecom' AND canonical_event_id = 'event-1' AND result_code IN ('ACCEPTED', 'ALREADY_APPLIED') ORDER BY completed_at_ms DESC, action_id DESC LIMIT 1",
                        "idx_im_inbound_actions_canonical_event",
                    ),
                    (
                        "EXPLAIN QUERY PLAN SELECT delivery_id FROM im_outbox INDEXED BY idx_im_outbox_canonical_event WHERE canonical_event_id = 'event-1' AND payload_kind = 'intervention' ORDER BY notification_kind, channel_kind",
                        "idx_im_outbox_canonical_event",
                    ),
                ] {
                    let details = connection
                        .prepare(sql)?
                        .query_map([], |row| row.get::<_, String>(3))?
                        .collect::<Result<Vec<_>, _>>()?;
                    assert!(details.iter().any(|detail| detail.contains(expected)), "{details:?}");
                }
                Ok(())
            })
            .unwrap();
        let _ = json!({ "verified": true });
    }
}
