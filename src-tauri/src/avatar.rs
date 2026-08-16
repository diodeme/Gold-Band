use std::fs;

use base64::{Engine, engine::general_purpose::STANDARD};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use gold_band::config::{
    AvatarPreference, AvatarShapePreference, PersonalizationAvatarShape, PersonalizationPreference,
};
use gold_band::storage::{read_json, write_json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const AVATAR_STORE_VERSION: u32 = 2;
const PERSONALIZATION_AVATAR_STORE_VERSION: u32 = 2;
const MAX_RECENT_AVATARS: usize = 10;
const MAX_AVATAR_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AvatarShape {
    #[default]
    Circle,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AvatarKind {
    Agent,
    User,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDesktopAvatarInput {
    pub kind: AvatarKind,
    pub shape: AvatarShape,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarImageVm {
    pub id: String,
    pub data_url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarProfileVm {
    pub shape: AvatarShape,
    pub selected_avatar_id: Option<String>,
    pub recent_avatars: Vec<AvatarImageVm>,
}

impl Default for AvatarProfileVm {
    fn default() -> Self {
        Self {
            shape: AvatarShape::Circle,
            selected_avatar_id: None,
            recent_avatars: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPreferencesVm {
    pub agent: AvatarProfileVm,
    pub user: AvatarProfileVm,
}

#[derive(Debug, Clone)]
pub struct AvatarError {
    pub code: &'static str,
    pub params: Value,
}

impl AvatarError {
    fn new(code: &'static str, params: Value) -> Self {
        Self { code, params }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvatarRecord {
    id: String,
    file_name: String,
    mime_type: String,
    created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvatarProfileStore {
    #[serde(default)]
    shape: AvatarShape,
    selected_avatar_id: Option<String>,
    #[serde(default)]
    recent_avatars: Vec<AvatarRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvatarStore {
    version: u32,
    #[serde(default)]
    agent: AvatarProfileStore,
    #[serde(default)]
    user: AvatarProfileStore,
}

impl Default for AvatarStore {
    fn default() -> Self {
        Self {
            version: AVATAR_STORE_VERSION,
            agent: AvatarProfileStore::default(),
            user: AvatarProfileStore::default(),
        }
    }
}

impl AvatarStore {
    fn profile(&self, kind: AvatarKind) -> &AvatarProfileStore {
        match kind {
            AvatarKind::Agent => &self.agent,
            AvatarKind::User => &self.user,
        }
    }

    fn profile_mut(&mut self, kind: AvatarKind) -> &mut AvatarProfileStore {
        match kind {
            AvatarKind::Agent => &mut self.agent,
            AvatarKind::User => &mut self.user,
        }
    }
}

pub fn load_avatar_preferences(root: &Utf8Path) -> Result<AvatarPreferencesVm, AvatarError> {
    let store = load_store(root)?;
    Ok(avatar_preferences_vm(root, &store))
}

pub fn load_resolved_avatar_preferences(
    root: &Utf8Path,
    personalization: &PersonalizationPreference,
) -> Result<AvatarPreferencesVm, AvatarError> {
    let mut preferences = load_avatar_preferences(root)?;
    resolve_avatar_profile(&mut preferences.agent, &personalization.avatars.agent);
    resolve_avatar_profile(&mut preferences.user, &personalization.avatars.user);
    Ok(preferences)
}

pub fn legacy_avatar_personalization(
    root: &Utf8Path,
    personalization: &mut PersonalizationPreference,
) -> Result<bool, AvatarError> {
    let store = load_store(root)?;
    if store.version >= PERSONALIZATION_AVATAR_STORE_VERSION {
        return Ok(false);
    }
    migrate_legacy_avatar_profile(&store.agent, &mut personalization.avatars.agent);
    migrate_legacy_avatar_profile(&store.user, &mut personalization.avatars.user);
    Ok(true)
}

pub fn complete_legacy_avatar_personalization(root: &Utf8Path) -> Result<(), AvatarError> {
    let mut store = load_store(root)?;
    store.version = PERSONALIZATION_AVATAR_STORE_VERSION;
    persist_store(root, &store)
}

fn migrate_legacy_avatar_profile(
    profile: &AvatarProfileStore,
    personalization: &mut gold_band::config::AvatarPersonalization,
) {
    personalization.image =
        profile
            .selected_avatar_id
            .as_ref()
            .map_or(AvatarPreference::Theme, |asset_id| AvatarPreference::User {
                asset_id: asset_id.clone(),
            });
    personalization.shape = AvatarShapePreference::Custom {
        value: personalization_shape(profile.shape),
    };
}

fn resolve_avatar_profile(
    profile: &mut AvatarProfileVm,
    personalization: &gold_band::config::AvatarPersonalization,
) {
    profile.selected_avatar_id = match &personalization.image {
        AvatarPreference::Theme => None,
        AvatarPreference::User { asset_id } => profile
            .recent_avatars
            .iter()
            .any(|avatar| &avatar.id == asset_id)
            .then(|| asset_id.clone()),
    };
    profile.shape = match personalization.shape {
        AvatarShapePreference::Theme => AvatarShape::Circle,
        AvatarShapePreference::Custom { value } => match value {
            PersonalizationAvatarShape::Circle => AvatarShape::Circle,
            PersonalizationAvatarShape::Square => AvatarShape::Square,
        },
    };
}

fn personalization_shape(shape: AvatarShape) -> PersonalizationAvatarShape {
    match shape {
        AvatarShape::Circle => PersonalizationAvatarShape::Circle,
        AvatarShape::Square => PersonalizationAvatarShape::Square,
    }
}

pub fn save_avatar_image(
    root: &Utf8Path,
    input: SaveDesktopAvatarInput,
) -> Result<AvatarPreferencesVm, AvatarError> {
    let extension = supported_extension(&input.mime_type).ok_or_else(|| {
        AvatarError::new(
            "avatar.unsupported-image-type",
            serde_json::json!({ "mimeType": input.mime_type }),
        )
    })?;
    let bytes = STANDARD
        .decode(input.data_base64.trim())
        .map_err(|_| AvatarError::new("avatar.invalid-image-data", serde_json::json!({})))?;
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return Err(AvatarError::new(
            "avatar.image-too-large",
            serde_json::json!({ "maxBytes": MAX_AVATAR_BYTES }),
        ));
    }
    if !matches_mime_signature(&input.mime_type, &bytes) {
        return Err(AvatarError::new(
            "avatar.invalid-image-data",
            serde_json::json!({ "mimeType": input.mime_type }),
        ));
    }

    let mut store = load_store(root)?;
    let avatars_dir = avatars_dir(root);
    fs::create_dir_all(avatars_dir.as_std_path()).map_err(|error| {
        AvatarError::new(
            "avatar.save-failed",
            serde_json::json!({ "message": error.to_string() }),
        )
    })?;
    let id = Uuid::new_v4().to_string();
    let file_name = format!("{id}.{extension}");
    let image_path = avatars_dir.join(&file_name);
    fs::write(image_path.as_std_path(), &bytes).map_err(|error| {
        AvatarError::new(
            "avatar.save-failed",
            serde_json::json!({ "message": error.to_string() }),
        )
    })?;

    let profile = store.profile_mut(input.kind);
    profile.shape = input.shape;
    profile.selected_avatar_id = Some(id.clone());
    profile.recent_avatars.retain(|avatar| avatar.id != id);
    profile.recent_avatars.insert(
        0,
        AvatarRecord {
            id,
            file_name,
            mime_type: input.mime_type,
            created_at: Utc::now().to_rfc3339(),
        },
    );
    let removed = if profile.recent_avatars.len() > MAX_RECENT_AVATARS {
        profile.recent_avatars.split_off(MAX_RECENT_AVATARS)
    } else {
        Vec::new()
    };
    if let Err(error) = persist_store(root, &store) {
        let _ = fs::remove_file(image_path.as_std_path());
        return Err(error);
    }
    for avatar in removed {
        let _ = fs::remove_file(avatars_dir.join(avatar.file_name).as_std_path());
    }
    Ok(avatar_preferences_vm(root, &store))
}

pub fn select_recent_avatar(
    root: &Utf8Path,
    kind: AvatarKind,
    avatar_id: &str,
) -> Result<AvatarPreferencesVm, AvatarError> {
    let mut store = load_store(root)?;
    let profile = store.profile_mut(kind);
    let Some(index) = profile
        .recent_avatars
        .iter()
        .position(|avatar| avatar.id == avatar_id)
    else {
        return Err(AvatarError::new(
            "avatar.recent-not-found",
            serde_json::json!({ "avatarId": avatar_id }),
        ));
    };
    let selected = profile.recent_avatars.remove(index);
    profile.selected_avatar_id = Some(selected.id.clone());
    profile.recent_avatars.insert(0, selected);
    persist_store(root, &store)?;
    Ok(avatar_preferences_vm(root, &store))
}

pub fn save_avatar_shape(
    root: &Utf8Path,
    kind: AvatarKind,
    shape: AvatarShape,
) -> Result<AvatarPreferencesVm, AvatarError> {
    let mut store = load_store(root)?;
    store.profile_mut(kind).shape = shape;
    persist_store(root, &store)?;
    Ok(avatar_preferences_vm(root, &store))
}

pub fn clear_avatar(root: &Utf8Path, kind: AvatarKind) -> Result<AvatarPreferencesVm, AvatarError> {
    let mut store = load_store(root)?;
    store.profile_mut(kind).selected_avatar_id = None;
    persist_store(root, &store)?;
    Ok(avatar_preferences_vm(root, &store))
}

fn load_store(root: &Utf8Path) -> Result<AvatarStore, AvatarError> {
    let path = avatar_store_file(root);
    if !path.exists() {
        return Ok(AvatarStore::default());
    }
    read_json(&path).map_err(|error| {
        AvatarError::new(
            "avatar.load-failed",
            serde_json::json!({ "message": error.to_string() }),
        )
    })
}

fn persist_store(root: &Utf8Path, store: &AvatarStore) -> Result<(), AvatarError> {
    write_json(&avatar_store_file(root), store).map_err(|error| {
        AvatarError::new(
            "avatar.save-failed",
            serde_json::json!({ "message": error.to_string() }),
        )
    })
}

fn avatar_preferences_vm(root: &Utf8Path, store: &AvatarStore) -> AvatarPreferencesVm {
    AvatarPreferencesVm {
        agent: avatar_profile_vm(root, store.profile(AvatarKind::Agent)),
        user: avatar_profile_vm(root, store.profile(AvatarKind::User)),
    }
}

fn avatar_profile_vm(root: &Utf8Path, profile: &AvatarProfileStore) -> AvatarProfileVm {
    let recent_avatars = profile
        .recent_avatars
        .iter()
        .filter_map(|avatar| {
            let bytes = fs::read(avatars_dir(root).join(&avatar.file_name).as_std_path()).ok()?;
            Some(AvatarImageVm {
                id: avatar.id.clone(),
                data_url: format!(
                    "data:{};base64,{}",
                    avatar.mime_type,
                    STANDARD.encode(bytes)
                ),
                created_at: avatar.created_at.clone(),
            })
        })
        .collect::<Vec<_>>();
    let selected_avatar_id = profile.selected_avatar_id.as_ref().and_then(|selected| {
        recent_avatars
            .iter()
            .any(|avatar| &avatar.id == selected)
            .then(|| selected.clone())
    });
    AvatarProfileVm {
        shape: profile.shape,
        selected_avatar_id,
        recent_avatars,
    }
}

fn avatar_store_file(root: &Utf8Path) -> Utf8PathBuf {
    root.join("desktop/avatar-settings.json")
}

fn avatars_dir(root: &Utf8Path) -> Utf8PathBuf {
    root.join("desktop/avatars")
}

fn supported_extension(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "image/webp" => Some("webp"),
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        _ => None,
    }
}

fn matches_mime_signature(mime_type: &str, bytes: &[u8]) -> bool {
    match mime_type {
        "image/webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "image/png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn root(temp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap()
    }

    fn webp_payload(seed: u8) -> String {
        STANDARD.encode([
            b'R', b'I', b'F', b'F', seed, 0, 0, 0, b'W', b'E', b'B', b'P', seed,
        ])
    }

    #[test]
    fn avatar_preferences_default_to_unset_circle_profiles() {
        let temp = tempdir().unwrap();
        let preferences = load_avatar_preferences(&root(&temp)).unwrap();
        assert_eq!(preferences.agent.shape, AvatarShape::Circle);
        assert_eq!(preferences.user.shape, AvatarShape::Circle);
        assert!(preferences.agent.selected_avatar_id.is_none());
        assert!(preferences.user.recent_avatars.is_empty());
    }

    #[test]
    fn avatar_upload_select_clear_shape_and_recent_limit_are_persisted() {
        let temp = tempdir().unwrap();
        let root = root(&temp);
        let mut first_id = String::new();
        for seed in 0..11 {
            let preferences = save_avatar_image(
                &root,
                SaveDesktopAvatarInput {
                    kind: AvatarKind::Agent,
                    shape: AvatarShape::Square,
                    mime_type: "image/webp".to_string(),
                    data_base64: webp_payload(seed),
                },
            )
            .unwrap();
            if seed == 0 {
                first_id = preferences.agent.selected_avatar_id.unwrap();
            }
        }

        let preferences = load_avatar_preferences(&root).unwrap();
        assert_eq!(preferences.agent.shape, AvatarShape::Square);
        assert_eq!(preferences.agent.recent_avatars.len(), MAX_RECENT_AVATARS);
        assert!(
            !preferences
                .agent
                .recent_avatars
                .iter()
                .any(|avatar| avatar.id == first_id)
        );

        let selected_id = preferences.agent.recent_avatars[4].id.clone();
        let selected = select_recent_avatar(&root, AvatarKind::Agent, &selected_id).unwrap();
        assert_eq!(
            selected.agent.selected_avatar_id.as_deref(),
            Some(selected_id.as_str())
        );
        assert_eq!(selected.agent.recent_avatars[0].id, selected_id);

        let shaped = save_avatar_shape(&root, AvatarKind::Agent, AvatarShape::Circle).unwrap();
        assert_eq!(shaped.agent.shape, AvatarShape::Circle);
        assert_eq!(
            load_avatar_preferences(&root).unwrap().agent.shape,
            AvatarShape::Circle
        );

        let recent_count = shaped.agent.recent_avatars.len();
        let cleared = clear_avatar(&root, AvatarKind::Agent).unwrap();
        assert!(cleared.agent.selected_avatar_id.is_none());
        assert_eq!(cleared.agent.recent_avatars.len(), recent_count);
        assert!(
            load_avatar_preferences(&root)
                .unwrap()
                .agent
                .selected_avatar_id
                .is_none()
        );
    }

    #[test]
    fn avatar_upload_rejects_mismatched_image_data() {
        let temp = tempdir().unwrap();
        let error = save_avatar_image(
            &root(&temp),
            SaveDesktopAvatarInput {
                kind: AvatarKind::User,
                shape: AvatarShape::Circle,
                mime_type: "image/png".to_string(),
                data_base64: STANDARD.encode(b"not-a-png"),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "avatar.invalid-image-data");
    }
}
