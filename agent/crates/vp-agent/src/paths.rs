use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Per-user data directory: %APPDATA%\Vision-Pro (overridable with VISION_PRO_HOME for tests).
pub fn data_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("VISION_PRO_HOME") {
        return PathBuf::from(custom);
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Vision-Pro")
}

pub fn ensure_data_dir() -> std::io::Result<PathBuf> {
    let dir = data_dir();
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(dir.join("logs"))?;
    Ok(dir)
}

/// Atomic write: write to a temp file in the same directory, flush, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}
