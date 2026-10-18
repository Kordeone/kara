// SPDX-License-Identifier: MPL-2.0
use crate::tasks::{self, Task};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const APP_ID: &str = "io.github.kordeone.Kara";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    tasks: Vec<Task>,
}

pub struct Store {
    path: PathBuf,
    _lock: File,
    snapshot: Option<Vec<u8>>,
    tasks: Vec<Task>,
}

pub fn data_path() -> Result<PathBuf, String> {
    data_path_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}
fn data_path_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    let base = xdg
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|p| Path::new(p).is_absolute())
                .map(|p| PathBuf::from(p).join(".local/share"))
        })
        .ok_or("Cannot find an absolute XDG data directory or HOME.")?;
    Ok(base.join(APP_ID).join("tasks.json"))
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
    }
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let dir = path.parent().ok_or("Invalid storage path.")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join("tasks.lock"))
            .map_err(|e| format!("Cannot open storage lock: {e}"))?;
        lock.try_lock().map_err(|e| {
            format!("Cannot lock task storage; another Kara instance may be using it: {e}")
        })?;
        let snapshot = read(&path)?;
        let tasks = match &snapshot {
            None => Vec::new(),
            Some(bytes) => {
                let doc: Document = serde_json::from_slice(bytes).map_err(|e| {
                    format!(
                        "Invalid task data in {}: {e}. The file has been preserved.",
                        path.display()
                    )
                })?;
                if doc.version != 1 {
                    return Err(format!(
                        "Unsupported task data version {}. The file has been preserved.",
                        doc.version
                    ));
                }
                tasks::validate(&doc.tasks)
                    .map_err(|e| format!("Invalid task data: {e} The file has been preserved."))?;
                doc.tasks
            }
        };
        Ok(Self {
            path,
            _lock: lock,
            snapshot,
            tasks,
        })
    }
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
    pub fn check_storage(&self) -> Result<(), String> {
        if read(&self.path)? != self.snapshot {
            return Err("Task storage changed outside Kara. Restart Kara to reload it; the file has been preserved.".into());
        }
        File::open(self.path.parent().ok_or("Invalid storage path.")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("Cannot sync task storage: {e}"))
    }
    /// Publish changes only after atomic replacement. A directory sync error is reported
    /// after publication because replacement has already succeeded at that point.
    pub fn replace(&mut self, tasks: Vec<Task>) -> Result<(), String> {
        tasks::validate(&tasks)?;
        if read(&self.path)? != self.snapshot {
            return Err("Task storage changed outside Kara. Changes were not saved. Restart Kara to reload the file.".into());
        }
        let bytes = serde_json::to_vec_pretty(&Document {
            version: 1,
            tasks: tasks.clone(),
        })
        .map_err(|e| e.to_string())?;
        let dir = self.path.parent().ok_or("Invalid storage path.")?;
        let mut temp = tempfile::NamedTempFile::new_in(dir)
            .map_err(|e| format!("Cannot create save file: {e}"))?;
        temp.write_all(&bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|e| format!("Cannot write task data: {e}"))?;
        // Check again after writing, before replacing the destination.
        if read(&self.path)? != self.snapshot {
            return Err("Task storage changed during save. Original data was preserved.".into());
        }
        temp.persist(&self.path)
            .map_err(|e| format!("Cannot replace task data: {e}"))?;
        self.tasks = tasks;
        self.snapshot = Some(bytes);
        File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("Tasks were written, but durability could not be confirmed: {e}"))
    }
    pub fn upsert(&mut self, task: Task) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        if let Some(old) = tasks.iter_mut().find(|old| old.id == task.id) {
            *old = task;
        } else {
            tasks.push(task);
        }
        self.replace(tasks)
    }
    pub fn toggle(&mut self, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        let task = tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Task no longer exists.")?;
        task.completed = !task.completed;
        self.replace(tasks)
    }
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        tasks.retain(|t| t.id != id);
        self.replace(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xdg_paths_use_absolute_data_home_and_standard_fallback() {
        assert_eq!(
            data_path_from(Some("/data".into()), Some("/home/user".into())).unwrap(),
            Path::new("/data").join(APP_ID).join("tasks.json")
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                data_path_from(xdg, Some("/home/user".into())).unwrap(),
                Path::new("/home/user/.local/share")
                    .join(APP_ID)
                    .join("tasks.json")
            );
        }
        assert!(data_path_from(None, None).is_err());
        assert!(data_path_from(None, Some("relative".into())).is_err());
    }
    #[test]
    fn atomic_replacement_keeps_old_open_file_intact_and_rejects_invalid_edits() {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let mut task = Task::new("Before").unwrap();
        store.upsert(task.clone()).unwrap();
        let before = fs::read(&path).unwrap();
        let mut original_handle = File::open(&path).unwrap();
        task.title = "After".into();
        store.upsert(task.clone()).unwrap();
        let mut original_bytes = Vec::new();
        original_handle.read_to_end(&mut original_bytes).unwrap();
        assert_eq!(original_bytes, before);
        let doc: Document = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc.tasks, vec![task.clone()]);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let saved = fs::read(&path).unwrap();
        task.title = "  ".into();
        assert!(store.upsert(task).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(store.tasks()[0].title, "After");
    }
    #[test]
    fn blocked_temporary_file_creation_preserves_previous_save() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let task = Task::new("Keep me").unwrap();
        store.upsert(task.clone()).unwrap();
        let before = fs::read(&path).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.delete(&task.id);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(store.tasks(), &[task]);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn crud_survives_restarts_and_keeps_identity_and_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let mut task = Task::new("Original").unwrap();
        let id = task.id.clone();
        store.upsert(task.clone()).unwrap();
        task.title = "Edited".into();
        task.due_date = Some("2026-10-17".into());
        task.notes = Some("Notes\nsecond line".into());
        task.priority = tasks::Priority::High;
        task.list_id = Some("future".into());
        store.upsert(task.clone()).unwrap();
        store.toggle(&id).unwrap();
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        task.completed = true;
        assert_eq!(store.tasks(), &[task.clone()]);
        store.toggle(&id).unwrap();
        assert!(!store.tasks()[0].completed);
        store.delete(&id).unwrap();
        drop(store);
        assert!(Store::open(path).unwrap().tasks().is_empty());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn corrupted_invalid_and_future_documents_are_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let task = Task::new("Valid").unwrap();
        let mut invalid = task.clone();
        invalid.due_date = Some("2026-02-30".into());
        for bytes in [
            b"{broken".to_vec(),
            Vec::new(),
            serde_json::to_vec(&Document {
                version: 99,
                tasks: vec![],
            })
            .unwrap(),
            serde_json::to_vec(&Document {
                version: 1,
                tasks: vec![task.clone(), task.clone()],
            })
            .unwrap(),
            serde_json::to_vec(&Document {
                version: 1,
                tasks: vec![invalid],
            })
            .unwrap(),
            br#"{"version":1,"tasks":[],"future_field":true}"#.to_vec(),
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(Store::open(path.clone()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn external_changes_lock_conflicts_and_failed_saves_preserve_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        assert!(Store::open(path.clone()).is_err());
        let task = Task::new("Saved").unwrap();
        store.upsert(task.clone()).unwrap();
        fs::write(&path, b"external data").unwrap();
        assert!(store.delete(&task.id).is_err());
        assert!(store.check_storage().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external data");
        assert_eq!(store.tasks(), &[task]);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(store.replace(vec![]).is_err());
        assert_eq!(store.tasks().len(), 1);
    }
}
