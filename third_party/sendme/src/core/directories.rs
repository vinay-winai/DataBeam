//! Optional DataBeam metadata, carried as a normal collection blob. Other Sendme
//! clients can still receive every file; DataBeam additionally restores empty dirs.
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::atomic::AtomicBool};

pub const NAME: &str = ".databeam-directories-v1.json";
const KIND: &str = "databeam-empty-directories";
pub const MAX_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    kind: String,
    version: u8,
    directories: Vec<String>,
}

pub fn collect(paths: &[PathBuf], cancel: Option<&AtomicBool>) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    for source in paths {
        super::send::check_cancelled(cancel)?;
        let canonical = source.canonicalize()?;
        if !canonical.is_dir() {
            continue;
        }
        let parent = canonical
            .parent()
            .ok_or_else(|| anyhow::anyhow!("folder has no parent"))?;
        for entry in walkdir::WalkDir::new(&canonical) {
            super::send::check_cancelled(cancel)?;
            let entry = entry?;
            if entry.file_type().is_dir() && std::fs::read_dir(entry.path())?.next().is_none() {
                result.push(super::send::canonicalized_path_to_string(
                    entry.path().strip_prefix(parent)?,
                    true,
                )?);
            }
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

pub fn encode(directories: Vec<String>) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        directories.len() <= 65536,
        "too many empty directories in this share"
    );
    let bytes = serde_json::to_vec(&Manifest {
        kind: KIND.into(),
        version: 1,
        directories,
    })?;
    anyhow::ensure!(
        bytes.len() <= MAX_BYTES,
        "too many empty directories in this share"
    );
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(bytes.len() <= MAX_BYTES, "directory metadata is too large");
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    anyhow::ensure!(
        manifest.kind == KIND && manifest.version == 1,
        "unsupported directory metadata"
    );
    anyhow::ensure!(manifest.directories.len() <= 65536, "too many directories");
    for name in &manifest.directories {
        for component in name.split('/') {
            super::receive::validate_path_component(component)?;
        }
    }
    Ok(manifest.directories)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_untrusted_directory_paths() {
        for path in [
            "../escape",
            "/absolute",
            "a\\b",
            "C:/escape",
            "a//b",
            ".",
            "",
        ] {
            assert!(
                decode(&encode(vec![path.into()]).unwrap()).is_err(),
                "{path}"
            );
        }
        assert_eq!(
            decode(&encode(vec!["folder/empty".into()]).unwrap()).unwrap(),
            vec!["folder/empty"]
        );
    }
}
