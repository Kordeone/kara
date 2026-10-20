// SPDX-License-Identifier: MPL-2.0
//! Portable JSON backups. SHA-256 detects damage, not authenticity/encryption.
use crate::tasks::{self, Task, TaskList, TaskSort};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const FORMAT: &str = "io.github.kordeone.Kara.backup";
const VERSION: u32 = 2;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    pub version: u32,
    pub tasks: Vec<Task>,
    pub lists: Vec<TaskList>,
    pub sort_mode: TaskSort,
    pub sort_ascending: bool,
    pub show_completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automatic_backups: Option<crate::automatic::Preferences>,
}
impl Data {
    fn validate(&self) -> Result<(), String> {
        if !matches!(
            (self.version, &self.automatic_backups),
            (6, None) | (7, Some(_))
        ) {
            return Err(format!("Unsupported backup task schema {}.", self.version));
        }
        if let Some(prefs) = &self.automatic_backups {
            prefs.validate()?;
        }
        tasks::validate_document(&self.tasks, &self.lists)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    backup_version: u32,
    sha256: String,
    data: serde_json::Value,
}

/// Only decoded, validated data reaches the restore confirmation/Store.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub(crate) data: Data,
}
impl Prepared {
    pub fn task_count(&self) -> usize {
        self.data.tasks.len()
    }
    pub fn list_count(&self) -> usize {
        self.data.lists.len()
    }
}
fn checksum(data: &Data) -> Result<String, String> {
    // Fixed struct field order, original vector order, UTF-8 JSON. Whitespace and
    // object key order in the portable envelope do not affect verification.
    let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
pub(crate) fn encode(mut data: Data) -> Result<Vec<u8>, String> {
    data.validate()?;
    if data.version == 6 {
        data.version = 7;
        data.automatic_backups = Some(crate::automatic::Preferences::default());
    }
    let bytes = serde_json::to_vec_pretty(&Envelope {
        format: FORMAT.into(),
        backup_version: VERSION,
        sha256: checksum(&data)?,
        data: serde_json::to_value(data).map_err(|e| e.to_string())?,
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Backup exceeds the 64 MiB limit.".into());
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Prepared, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Backup exceeds the 64 MiB limit.".into());
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid backup JSON: {e}"))?;
    if envelope.format != FORMAT || !matches!(envelope.backup_version, 1 | 2) {
        return Err("Unsupported Kara backup format or version.".into());
    }
    let mut data: Data = serde_json::from_value(envelope.data.clone())
        .map_err(|e| format!("Incomplete or invalid backup: {e}"))?;
    // Reject omitted fields that serde might otherwise default, and unknown data.
    if serde_json::to_value(&data).map_err(|e| e.to_string())? != envelope.data {
        return Err("Backup contains missing or noncanonical data fields.".into());
    }
    data.validate()?;
    if (envelope.backup_version == 1 && data.version != 6)
        || (envelope.backup_version == 2 && data.version != 7)
    {
        return Err("Backup version does not match its data schema.".into());
    }
    if envelope.sha256 != checksum(&data)? {
        return Err("Backup integrity check failed (SHA-256).".into());
    }
    // Upgrade only the validated in-memory import. Original input is untouched.
    if data.version == 6 {
        data.version = 7;
        data.automatic_backups = Some(crate::automatic::Preferences::default());
    }
    Ok(Prepared { data })
}
pub fn read(path: &Path) -> Result<Prepared, String> {
    if !std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("Select a regular Kara backup file.".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    decode(&bytes)
}
/// Export never overwrites any existing file, including another backup.
/// No unsafe in-place fallback if a portal/filesystem cannot atomically publish.
pub fn export(path: &Path, bytes: &[u8]) -> Result<(), String> {
    decode(bytes)?;
    if path.extension().is_none_or(|s| s != "kara-backup") {
        return Err("Choose a new filename ending in .kara-backup.".into());
    }
    let dir = path.parent().ok_or("Invalid backup path.")?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist_noclobber(path).map_err(|e| format!("Backup was not exported. Choose a new writable filename; existing files are never overwritten: {e}"))?;
    File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("Backup was written, but durability could not be confirmed: {e}"))
}

fn dialog_error(error: cosmic::dialog::file_chooser::Error) -> Result<Option<PathBuf>, String> {
    use cosmic::dialog::{ashpd, file_chooser::Error};
    match error {
        Error::Cancelled
        | Error::Save(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
        | Error::Open(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
        | Error::Response(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {
            Ok(None)
        }
        error => Err(format!("File dialog failed: {error}")),
    }
}

/// libcosmic's locked XDG portal API works with host and sandbox-granted paths.
pub async fn choose(exporting: bool) -> Result<Option<PathBuf>, String> {
    use cosmic::dialog::file_chooser::{self, FileFilter};
    let filter = FileFilter::new("Kara backup").glob("*.kara-backup");
    if exporting {
        match file_chooser::save::Dialog::new()
            .title("Export Kara backup".into())
            .file_name(format!("kara-{}.kara-backup", jiff::Zoned::now().date()))
            .filter(filter)
            .save_file()
            .await
        {
            Ok(response) => response
                .url()
                .map(|url| {
                    url.to_file_path()
                        .map_err(|()| "Select a local file location.".into())
                })
                .transpose(),
            Err(error) => dialog_error(error),
        }
    } else {
        match file_chooser::open::Dialog::new()
            .title("Restore Kara backup")
            .filter(filter)
            .open_file()
            .await
        {
            Ok(response) => response
                .url()
                .to_file_path()
                .map(Some)
                .map_err(|()| "Select a local backup file.".into()),
            Err(error) => dialog_error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locked_portal_open_and_save_cancellation_are_not_reported_as_errors() {
        use cosmic::dialog::{ashpd, file_chooser::Error};
        for error in [
            Error::Cancelled,
            Error::Save(ashpd::Error::Response(
                ashpd::desktop::ResponseError::Cancelled,
            )),
            Error::Open(ashpd::Error::Response(
                ashpd::desktop::ResponseError::Cancelled,
            )),
            Error::Response(ashpd::Error::Response(
                ashpd::desktop::ResponseError::Cancelled,
            )),
        ] {
            assert_eq!(dialog_error(error).unwrap(), None);
        }
        assert!(dialog_error(Error::UrlAbsolute).is_err());
    }
    pub const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
    #[test]
    fn version_two_backups_include_settings_and_reject_invalid_settings() {
        let mut data = decode(FIXTURE).unwrap().data;
        data.automatic_backups = Some(crate::automatic::Preferences {
            enabled: true,
            interval: "P1W".into(),
            last_success: Some(1000),
        });
        let bytes = encode(data.clone()).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["backup_version"], 2);
        assert_eq!(json["data"]["version"], 7);
        assert_eq!(decode(&bytes).unwrap().data, data);
        data.automatic_backups.as_mut().unwrap().interval = "P1M".into();
        assert!(encode(data).is_err());
        let mut bad = json;
        bad["data"]["automatic_backups"]["last_success"] = (-1).into();
        assert!(decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    #[test]
    fn independently_generated_v1_fixture_and_json_reformatting_are_compatible() {
        let prepared = decode(FIXTURE).unwrap();
        assert_eq!(prepared.task_count(), 2);
        assert_eq!(prepared.list_count(), 1);
        assert!(prepared.data.lists[0].pinned);
        assert!(prepared.data.tasks[1].completed);
        let json: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        assert_eq!(
            decode(&serde_json::to_vec(&json).unwrap()).unwrap().data,
            prepared.data
        );
        assert_eq!(
            decode(&encode(prepared.data.clone()).unwrap())
                .unwrap()
                .data,
            prepared.data
        );
    }
    #[test]
    fn rejects_truncated_tampered_incomplete_and_unsupported_backups() {
        for bytes in [
            b"".as_slice(),
            b"{}",
            b"null",
            &FIXTURE[..FIXTURE.len() / 2],
        ] {
            assert!(decode(bytes).is_err());
        }
        let original: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        for path in ["format", "backup_version", "sha256", "data"] {
            let mut json = original.clone();
            json.as_object_mut().unwrap().remove(path);
            assert!(
                decode(&serde_json::to_vec(&json).unwrap()).is_err(),
                "{path}"
            );
        }
        for field in [
            "tasks",
            "lists",
            "sort_mode",
            "sort_ascending",
            "show_completed",
        ] {
            let mut json = original.clone();
            json["data"].as_object_mut().unwrap().remove(field);
            assert!(
                decode(&serde_json::to_vec(&json).unwrap()).is_err(),
                "{field}"
            );
        }
        for field in ["due_date", "notes", "completed", "priority", "list_id"] {
            let mut json = original.clone();
            json["data"]["tasks"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(
                decode(&serde_json::to_vec(&json).unwrap()).is_err(),
                "{field}"
            );
        }
        let mut json = original.clone();
        json["data"]["tasks"][0]["title"] = "Tampered".into();
        assert!(
            decode(&serde_json::to_vec(&json).unwrap())
                .unwrap_err()
                .contains("integrity")
        );
        let mut json = original.clone();
        json["backup_version"] = 3.into();
        assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
        let mut json = original.clone();
        json["data"]["version"] = 8.into();
        assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
        let mut json = original.clone();
        json["unexpected"] = true.into();
        assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
        let mut json = original.clone();
        json["data"]["unexpected"] = true.into();
        assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
        let duplicate =
            String::from_utf8(FIXTURE.to_vec())
                .unwrap()
                .replacen('{', "{\"backup_version\":1,", 1);
        assert!(decode(duplicate.as_bytes()).is_err());
        assert!(decode(&serde_json::to_vec(&original["data"]).unwrap()).is_err());
    }
    #[test]
    fn rejects_invalid_priorities_duplicate_ids_and_broken_list_references() {
        let mut data = decode(FIXTURE).unwrap().data;
        data.tasks[1].id = data.tasks[0].id.clone();
        assert!(encode(data).is_err());
        let mut data = decode(FIXTURE).unwrap().data;
        data.lists.clear();
        assert!(encode(data).is_err());
        let mut data = decode(FIXTURE).unwrap().data;
        data.lists.push(data.lists[0].clone());
        assert!(encode(data).is_err());
        let mut json: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        json["data"]["tasks"][0]["priority"] = 4.into();
        assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
        json["data"]["tasks"][0]["priority"] = "High".into();
        assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
    }
    #[test]
    fn bounded_reads_reject_oversized_inputs_and_nonfiles() {
        assert!(
            decode(&vec![b' '; MAX_BYTES as usize + 1])
                .unwrap_err()
                .contains("64 MiB")
        );
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_err());
        let path = dir.path().join("too-large.kara-backup");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(read(&path).is_err());
    }
    #[test]
    fn export_is_portable_private_atomic_and_never_clobbers_existing_paths() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portable.kara-backup");
        export(&path, FIXTURE).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), FIXTURE);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read(&path).unwrap().data, decode(FIXTURE).unwrap().data);
        assert!(export(&path, FIXTURE).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), FIXTURE);
        let link = dir.path().join("link.kara-backup");
        symlink(&path, &link).unwrap();
        assert!(export(&link, FIXTURE).is_err());
        let folder = dir.path().join("folder.kara-backup");
        std::fs::create_dir(&folder).unwrap();
        assert!(export(&folder, FIXTURE).is_err());
        assert!(export(&dir.path().join("tasks.json"), FIXTURE).is_err());
        assert!(export(&dir.path().join("damaged.kara-backup"), b"{}").is_err());
        assert!(!dir.path().join("damaged.kara-backup").exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}
