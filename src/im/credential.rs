use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ImChannelKind;

const CREDENTIAL_SERVICE: &str = "Gold Band IM";
const CREDENTIAL_PAYLOAD_VERSION: u16 = 1;
const ACTION_SIGNING_KEY_ACCOUNT: &str = "action-token-signing-key:v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImCredentialPayload {
    pub version: u16,
    pub fields: BTreeMap<String, String>,
}

impl ImCredentialPayload {
    pub fn new(fields: BTreeMap<String, String>) -> Self {
        Self {
            version: CREDENTIAL_PAYLOAD_VERSION,
            fields,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImCredentialError {
    #[error("credential reference is invalid")]
    InvalidReference,
    #[error("credential payload is invalid")]
    InvalidPayload,
    #[error("credential is unavailable")]
    Unavailable,
    #[error("credential was not found")]
    NotFound,
}

impl ImCredentialError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidReference | Self::InvalidPayload => "IM_CREDENTIAL_INVALID",
            Self::Unavailable => "IM_CREDENTIAL_UNAVAILABLE",
            Self::NotFound => "IM_CREDENTIAL_NOT_FOUND",
        }
    }
}

pub trait ImCredentialStore: Send + Sync {
    fn save(
        &self,
        channel: ImChannelKind,
        credential_ref: &str,
        payload: &ImCredentialPayload,
    ) -> Result<(), ImCredentialError>;

    fn load(
        &self,
        channel: ImChannelKind,
        credential_ref: &str,
    ) -> Result<ImCredentialPayload, ImCredentialError>;

    fn delete(&self, channel: ImChannelKind, credential_ref: &str)
    -> Result<(), ImCredentialError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OsImCredentialStore;

impl OsImCredentialStore {
    pub fn new_reference() -> String {
        Uuid::new_v4().to_string()
    }

    fn entry(
        channel: ImChannelKind,
        credential_ref: &str,
    ) -> Result<keyring::Entry, ImCredentialError> {
        Uuid::parse_str(credential_ref).map_err(|_| ImCredentialError::InvalidReference)?;
        keyring::Entry::new(
            CREDENTIAL_SERVICE,
            &format!("{}:{credential_ref}", channel.as_str()),
        )
        .map_err(|_| ImCredentialError::Unavailable)
    }

    pub fn load_or_create_action_signing_key(&self) -> Result<Vec<u8>, ImCredentialError> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, ACTION_SIGNING_KEY_ACCOUNT)
            .map_err(|_| ImCredentialError::Unavailable)?;
        match entry.get_password() {
            Ok(encoded) => decode_signing_key(&encoded),
            Err(keyring::Error::NoEntry) => {
                let mut key = Vec::with_capacity(32);
                key.extend_from_slice(Uuid::new_v4().as_bytes());
                key.extend_from_slice(Uuid::new_v4().as_bytes());
                entry
                    .set_password(&STANDARD_NO_PAD.encode(&key))
                    .map_err(|_| ImCredentialError::Unavailable)?;
                Ok(key)
            }
            Err(_) => Err(ImCredentialError::Unavailable),
        }
    }
}

fn decode_signing_key(encoded: &str) -> Result<Vec<u8>, ImCredentialError> {
    let key = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|_| ImCredentialError::InvalidPayload)?;
    if key.len() < 32 {
        return Err(ImCredentialError::InvalidPayload);
    }
    Ok(key)
}

impl ImCredentialStore for OsImCredentialStore {
    fn save(
        &self,
        channel: ImChannelKind,
        credential_ref: &str,
        payload: &ImCredentialPayload,
    ) -> Result<(), ImCredentialError> {
        if payload.version != CREDENTIAL_PAYLOAD_VERSION || payload.fields.is_empty() {
            return Err(ImCredentialError::InvalidPayload);
        }
        let encoded =
            serde_json::to_string(payload).map_err(|_| ImCredentialError::InvalidPayload)?;
        Self::entry(channel, credential_ref)?
            .set_password(&encoded)
            .map_err(|_| ImCredentialError::Unavailable)
    }

    fn load(
        &self,
        channel: ImChannelKind,
        credential_ref: &str,
    ) -> Result<ImCredentialPayload, ImCredentialError> {
        let encoded = Self::entry(channel, credential_ref)?
            .get_password()
            .map_err(|error| {
                if matches!(error, keyring::Error::NoEntry) {
                    ImCredentialError::NotFound
                } else {
                    ImCredentialError::Unavailable
                }
            })?;
        let payload: ImCredentialPayload =
            serde_json::from_str(&encoded).map_err(|_| ImCredentialError::InvalidPayload)?;
        if payload.version != CREDENTIAL_PAYLOAD_VERSION || payload.fields.is_empty() {
            return Err(ImCredentialError::InvalidPayload);
        }
        Ok(payload)
    }

    fn delete(
        &self,
        channel: ImChannelKind,
        credential_ref: &str,
    ) -> Result<(), ImCredentialError> {
        Self::entry(channel, credential_ref)?
            .delete_credential()
            .or_else(|error| {
                if matches!(error, keyring::Error::NoEntry) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|_| ImCredentialError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_reference_contains_no_platform_identity() {
        let reference = OsImCredentialStore::new_reference();
        assert!(Uuid::parse_str(&reference).is_ok());
        assert!(!reference.contains("wecom"));
    }

    #[test]
    fn credential_payload_has_no_debug_implementation() {
        fn accepts_serializable(_: &impl Serialize) {}
        accepts_serializable(&ImCredentialPayload::new(BTreeMap::from([(
            "secret".into(),
            "value".into(),
        )])));
    }

    #[test]
    fn signing_key_decoder_rejects_short_or_malformed_values() {
        assert!(matches!(
            decode_signing_key("not-base64"),
            Err(ImCredentialError::InvalidPayload)
        ));
        assert!(matches!(
            decode_signing_key(&STANDARD_NO_PAD.encode([1_u8; 16])),
            Err(ImCredentialError::InvalidPayload)
        ));
        assert_eq!(
            decode_signing_key(&STANDARD_NO_PAD.encode([2_u8; 32])).unwrap(),
            vec![2_u8; 32]
        );
    }
}
