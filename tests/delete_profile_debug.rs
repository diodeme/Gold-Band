use camino::Utf8PathBuf;
use gold_band::app::{App, ProfileInput, ProfileScope};

#[test]
fn delete_profile_smoke_test_uses_force_flag_signature() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let created = app
        .create_profile(ProfileInput {
            scope: ProfileScope::User,
            name: "删除冒烟角色".to_string(),
            summary: "用于删除冒烟测试".to_string(),
            content: "临时内容".to_string(),
        })
        .unwrap();

    let list = app.delete_profile(&created.id, false).unwrap();
    assert!(list.profiles.iter().all(|profile| profile.id != created.id));
}
