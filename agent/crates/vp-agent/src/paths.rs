use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Per-user data directory: ~/Library/Application Support/Vision-Pro
/// (overridable with VISION_PRO_HOME for tests and side-by-side development installs).
pub fn data_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("VISION_PRO_HOME") {
        return PathBuf::from(custom);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join("Library")
        .join("Application Support")
        .join("Vision-Pro")
}

/// Creates the data directory (and `logs/`) readable only by the current user.
pub fn ensure_data_dir() -> std::io::Result<PathBuf> {
    let dir = data_dir();
    fs::create_dir_all(dir.join("logs"))?;
    for path in [dir.clone(), dir.join("logs")] {
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

/// Atomic write: write to a private temp file in the same directory, flush, then rename over the
/// target. Files are created with mode 0600.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}
