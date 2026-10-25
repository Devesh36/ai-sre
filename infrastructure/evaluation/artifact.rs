use anyhow::Result;
use serde::Serialize;
use std::path::Path;

/// Shared by harness and replay: serialize fully before atomically replacing a report.
pub fn write_json(value: &impl Serialize, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
