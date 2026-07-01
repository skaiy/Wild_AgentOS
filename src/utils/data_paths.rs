use std::path::{Path, PathBuf};

pub const NEW_HOME_DIR: &str = ".wild-agent-os";
pub const LEGACY_HOME_DIR: &str = ".gliding_horse";

pub fn user_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Preferred user data root (writes always go here).
pub fn user_data_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("WILD_AGENT_OS_DATA") {
        return Some(PathBuf::from(dir));
    }
    if let Ok(dir) = std::env::var("GLIDING_HORSE_DATA") {
        return Some(PathBuf::from(dir));
    }
    user_home().map(|h| h.join(NEW_HOME_DIR))
}

pub fn legacy_user_data_root() -> Option<PathBuf> {
    user_home().map(|h| h.join(LEGACY_HOME_DIR))
}

/// Resolve an existing path under user data, preferring the new root.
pub fn resolve_user_subpath(subpath: &str) -> Option<PathBuf> {
    let sub = Path::new(subpath);
    if let Some(root) = user_data_root() {
        let candidate = root.join(sub);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    legacy_user_data_root()
        .map(|root| root.join(sub))
        .filter(|p| p.exists())
}

/// Target path under the new user data root (creates parent dirs if needed).
pub fn user_subpath(subpath: &str) -> Option<PathBuf> {
    let path = user_data_root()?.join(subpath);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Some(path)
}

/// Resolve project-local prompt/data path, preferring the new directory name.
pub fn resolve_project_subpath(subpath: &str) -> Option<PathBuf> {
    let sub = Path::new(subpath);
    for dir in [NEW_HOME_DIR, LEGACY_HOME_DIR] {
        let candidate = PathBuf::from(dir).join(sub);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Migrate `~/.gliding_horse` → `~/.wild-agent-os` when only the legacy dir exists.
pub fn migrate_legacy_home_data() -> std::io::Result<bool> {
    let (Some(new_root), Some(legacy_root)) =
        (user_data_root(), legacy_user_data_root())
    else {
        return Ok(false);
    };

    if new_root == legacy_root {
        return Ok(false);
    }

    if new_root.exists() || !legacy_root.exists() {
        return Ok(false);
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&legacy_root, &new_root)?;
    }

    #[cfg(not(unix))]
    {
        copy_dir_recursive(&legacy_root, &new_root)?;
    }

    tracing::info!(
        legacy = %legacy_root.display(),
        new = %new_root.display(),
        "migrated legacy user data directory"
    );
    Ok(true)
}

#[cfg(not(unix))]
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}