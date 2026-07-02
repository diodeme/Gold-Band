use std::fs;
use std::path::Path;
use std::process::Command;

use tracing::{debug, warn};

pub fn create_link(src: &Path, dst: &Path) {
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(src, dst) {
            Ok(()) => debug!("created symlink: {:?} -> {:?}", dst, src),
            Err(error) => warn!("failed to create symlink {:?} -> {:?}: {error}", dst, src),
        }
    }

    #[cfg(windows)]
    {
        match std::os::windows::fs::symlink_dir(src, dst) {
            Ok(()) => {
                debug!("created symlink: {:?} -> {:?}", dst, src);
                return;
            }
            Err(error) => debug!("symlink_dir failed (will try junction): {error}"),
        }

        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(dst.as_os_str())
            .arg(src.as_os_str())
            .output();

        match output {
            Ok(result) if result.status.success() => {
                debug!("created junction: {:?} -> {:?}", dst, src);
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                warn!(
                    "failed to create link {:?} -> {:?}: {}. Try enabling Developer Mode or run as Administrator.",
                    dst,
                    src,
                    stderr.trim()
                );
            }
            Err(error) => warn!("failed to create link {:?} -> {:?}: {error}", dst, src),
        }
    }
}

pub fn remove_link_if_points_to(link_path: &Path, expected_src: &Path) {
    if !link_path.exists() {
        return;
    }
    let is_link = link_path.is_symlink() || link_path.read_link().is_ok();
    if !is_link {
        return;
    }
    let target = match link_path.read_link() {
        Ok(target) => target,
        Err(_) => return,
    };
    let canonical_target = fs::canonicalize(&target).unwrap_or(target);
    let canonical_expected =
        fs::canonicalize(expected_src).unwrap_or_else(|_| expected_src.to_path_buf());
    if canonical_target != canonical_expected {
        return;
    }
    if fs::remove_file(link_path).is_err() {
        let _ = fs::remove_dir(link_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn remove_link_if_points_to_only_removes_matching_link() {
        let tmp = std::env::temp_dir().join(format!("gb-symlink-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let src_dir = tmp.join("source-skill");
        let other_dir = tmp.join("other-skill");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&other_dir).unwrap();
        fs::write(src_dir.join("SKILL.md"), "content").unwrap();
        fs::write(other_dir.join("SKILL.md"), "content").unwrap();

        let matching_link = tmp.join("matching-link");
        let other_link = tmp.join("other-link");
        create_link(&src_dir, &matching_link);
        create_link(&other_dir, &other_link);

        remove_link_if_points_to(&matching_link, &src_dir);
        remove_link_if_points_to(&other_link, &src_dir);

        assert!(!matching_link.exists());
        assert!(other_link.exists() || other_link.read_link().is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }
}
