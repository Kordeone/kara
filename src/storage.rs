// SPDX-License-Identifier: MPL-2.0
use crate::{
    matrix::{self, Direction},
    tasks::{self, Priority, Quadrant, Task, TaskList, TaskSort},
};
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
    #[serde(default)]
    lists: Vec<TaskList>,
    #[serde(default)]
    sort_mode: TaskSort,
    #[serde(default)]
    sort_ascending: Option<bool>,
    #[serde(default = "default_show_completed")]
    show_completed: bool,
}
fn default_show_completed() -> bool {
    true
}

// Legacy values are deliberately accepted only by the versioned migration.
#[derive(Deserialize)]
enum LegacyPriority {
    Low,
    Normal,
    High,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyTask {
    id: String,
    title: String,
    completed: bool,
    due_date: Option<String>,
    priority: LegacyPriority,
    notes: Option<String>,
    list_id: Option<String>,
    #[serde(default)]
    matrix_quadrant: Option<Quadrant>,
    #[serde(default)]
    matrix_order: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDocument {
    version: u32,
    tasks: Vec<LegacyTask>,
    #[serde(default)]
    lists: Vec<TaskList>,
}
fn decode_document(bytes: &[u8]) -> Result<Document, String> {
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    match header.version {
        4..=6 => serde_json::from_slice(bytes).map_err(|e| e.to_string()),
        1..=3 => {
            let old: LegacyDocument = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            let mut converted = Vec::with_capacity(old.tasks.len());
            for task in old.tasks {
                if task.matrix_quadrant.is_none() && task.matrix_order.is_some() {
                    return Err("A legacy Matrix order requires a quadrant.".into());
                }
                // Retired priority is validated, then preserved in the byte-exact
                // backup rather than guessed into a new importance/urgency value.
                let _retired_priority = task.priority;
                converted.push(Task {
                    id: task.id,
                    title: task.title,
                    completed: task.completed,
                    due_date: task.due_date,
                    notes: task.notes,
                    list_id: task.list_id,
                    created_at: None,
                    priority: task
                        .matrix_quadrant
                        .map_or(Priority::P0, Quadrant::priority),
                    matrix_order: task.matrix_order,
                });
            }
            Ok(Document {
                version: old.version,
                tasks: converted,
                lists: old.lists,
                sort_mode: TaskSort::default(),
                sort_ascending: None,
                show_completed: true,
            })
        }
        other => Err(format!("Unsupported task data version {other}.")),
    }
}

/// No-clobber publication, byte verification and directory sync must all finish
/// before the original legacy file can be atomically replaced.
fn ensure_migration_backup(path: &Path, original: &[u8], suffix: &str) -> Result<(), String> {
    let backup = path.with_extension(suffix);
    let dir = path.parent().ok_or("Invalid storage path.")?;
    let verify = || -> Result<(), String> {
        let metadata = fs::symlink_metadata(&backup).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file()
            || fs::read(&backup).map_err(|e| e.to_string())? != original
        {
            return Err(format!(
                "Migration backup {} differs from the original or is not a regular file. Preserve both files and resolve the backup conflict before retrying.",
                backup.display()
            ));
        }
        File::open(&backup)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    };
    if !backup.try_exists().map_err(|e| e.to_string())? {
        let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        temp.write_all(original)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        match temp.persist_noclobber(&backup) {
            Ok(_) => {}
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(format!(
                    "Cannot create migration backup {}: {e}",
                    backup.display()
                ));
            }
        }
    }
    verify()
        .map_err(|e| format!("Cannot verify migration backup; legacy data was not replaced: {e}"))
}

// A one-shot, thread-local fault injection covers rollback after publication
// without touching real user data or affecting concurrently running tests.
#[cfg(test)]
thread_local! {
    static FAIL_COMMIT_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn sync_committed_directory(dir: &Path) -> io::Result<()> {
    #[cfg(test)]
    if FAIL_COMMIT_SYNC.with(|flag| flag.replace(false)) {
        return Err(io::Error::other("injected directory sync failure"));
    }
    File::open(dir).and_then(|f| f.sync_all())
}

pub struct Store {
    path: PathBuf,
    _lock: File,
    snapshot: Option<Vec<u8>>,
    migration_version: Option<u32>,
    tasks: Vec<Task>,
    lists: Vec<TaskList>,
    sort_mode: TaskSort,
    sort_ascending: bool,
    show_completed: bool,
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
        let mut migration_version = None;
        let (tasks, lists, sort_mode, sort_ascending, show_completed) = match &snapshot {
            None => (
                Vec::new(),
                Vec::new(),
                TaskSort::default(),
                TaskSort::default().default_ascending(),
                true,
            ),
            Some(bytes) => {
                let mut doc = decode_document(bytes).map_err(|e| {
                    format!(
                        "Invalid task data in {}: {e}. The file has been preserved.",
                        path.display()
                    )
                })?;
                migration_version = (doc.version < 6).then_some(doc.version);
                if doc.version == 1 {
                    // Stage 1 allowed reserved list IDs without list definitions. Recover
                    // those definitions without changing a task or writing on load.
                    for task in &doc.tasks {
                        if let Some(id) = &task.list_id
                            && !doc.lists.iter().any(|list| &list.id == id)
                        {
                            let mut name = format!("Recovered list {id}");
                            while tasks::validate_list_name(&name, &doc.lists, None).is_err() {
                                name.push(' ');
                                name.push('•');
                            }
                            doc.lists.push(TaskList {
                                id: id.clone(),
                                name,
                                pinned: false,
                            });
                        }
                    }
                }
                tasks::validate_document(&doc.tasks, &doc.lists)
                    .map_err(|e| format!("Invalid task data: {e} The file has been preserved."))?;
                matrix::normalize(&mut doc.tasks);
                let ascending = doc
                    .sort_ascending
                    .unwrap_or_else(|| doc.sort_mode.default_ascending());
                (
                    doc.tasks,
                    doc.lists,
                    doc.sort_mode,
                    ascending,
                    doc.show_completed,
                )
            }
        };
        Ok(Self {
            path,
            _lock: lock,
            snapshot,
            migration_version,
            tasks,
            lists,
            sort_mode,
            sort_ascending,
            show_completed,
        })
    }
    fn backup_data(&self) -> crate::backup::Data {
        crate::backup::Data {
            version: 6,
            tasks: self.tasks.clone(),
            lists: self.lists.clone(),
            sort_mode: self.sort_mode,
            sort_ascending: self.sort_ascending,
            show_completed: self.show_completed,
        }
    }
    pub fn backup_bytes(&self) -> Result<Vec<u8>, String> {
        self.check_storage()?;
        crate::backup::encode(self.backup_data())
    }
    /// The recovery file is portable and verified/durable before any live write.
    /// Normal legacy migration still makes its own byte-exact original backup.
    pub fn restore_backup(&mut self, backup: &crate::backup::Prepared) -> Result<PathBuf, String> {
        crate::backup::encode(backup.data.clone())?;
        self.check_storage()?;
        let previous = self.backup_data();
        let before = self.snapshot.clone();
        let bytes = self.backup_bytes()?;
        let dir = self.path.parent().ok_or("Invalid storage path.")?;
        let mut recovery = tempfile::Builder::new()
            .prefix("kara-before-restore-")
            .suffix(".kara-backup")
            .tempfile_in(dir)
            .map_err(|e| format!("Cannot create recovery snapshot; nothing restored: {e}"))?;
        recovery
            .write_all(&bytes)
            .and_then(|()| recovery.as_file().sync_all())
            .map_err(|e| format!("Cannot write recovery snapshot; nothing restored: {e}"))?;
        let (file, path) = recovery.keep().map_err(|e| e.to_string())?;
        drop(file);
        if fs::read(&path).map_err(|e| e.to_string())? != bytes {
            return Err(format!(
                "Recovery snapshot verification failed: {}. Nothing restored.",
                path.display()
            ));
        }
        crate::backup::read(&path)?;
        File::open(dir).and_then(|f| f.sync_all()).map_err(|e| {
            format!(
                "Cannot sync recovery snapshot {}; nothing restored: {e}",
                path.display()
            )
        })?;
        let data = &backup.data;
        let result = self.commit_with_preferences(
            data.tasks.clone(),
            data.lists.clone(),
            data.sort_mode,
            data.sort_ascending,
            data.show_completed,
        );
        if let Err(error) = result {
            if self.snapshot != before {
                // Publication succeeded but directory durability failed. Roll back
                // through the same checked atomic path, never overwrite a later edit.
                let rollback = self.commit_with_preferences(
                    previous.tasks.clone(),
                    previous.lists.clone(),
                    previous.sort_mode,
                    previous.sort_ascending,
                    previous.show_completed,
                );
                let state = if self.backup_data() == previous {
                    "Previous data is active again."
                } else {
                    "Restored data is currently active; automatic rollback could not finish."
                };
                return Err(format!(
                    "Restore durability failure: {error}. {state} Rollback: {} Recovery snapshot: {}",
                    rollback.err().unwrap_or_else(|| "completed".into()),
                    path.display()
                ));
            }
            return Err(format!(
                "Restore failed before replacement: {error}. Previous data remains active. Recovery snapshot: {}",
                path.display()
            ));
        }
        Ok(path)
    }
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
    pub fn lists(&self) -> &[TaskList] {
        &self.lists
    }
    pub fn sort_mode(&self) -> TaskSort {
        self.sort_mode
    }
    pub fn sort_ascending(&self) -> bool {
        self.sort_ascending
    }
    pub fn show_completed(&self) -> bool {
        self.show_completed
    }
    pub fn set_preferences(
        &mut self,
        sort_mode: TaskSort,
        sort_ascending: bool,
        show_completed: bool,
    ) -> Result<(), String> {
        self.commit_with_preferences(
            self.tasks.clone(),
            self.lists.clone(),
            sort_mode,
            sort_ascending,
            show_completed,
        )
    }
    pub fn upsert_list(&mut self, mut list: TaskList) -> Result<(), String> {
        list.name = list.name.trim().into();
        let mut lists = self.lists.clone();
        if let Some(old) = lists.iter_mut().find(|old| old.id == list.id) {
            *old = list;
        } else {
            lists.push(list);
        }
        self.commit(self.tasks.clone(), lists)
    }
    pub fn delete_list(&mut self, id: &str) -> Result<(), String> {
        if !self.lists.iter().any(|list| list.id == id) {
            return Err("List no longer exists.".into());
        }
        let mut tasks = self.tasks.clone();
        matrix::move_list_to_inbox(&mut tasks, id);
        let lists = self
            .lists
            .iter()
            .filter(|list| list.id != id)
            .cloned()
            .collect();
        self.commit(tasks, lists)
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
        self.commit(tasks, self.lists.clone())
    }
    fn commit(&mut self, tasks: Vec<Task>, lists: Vec<TaskList>) -> Result<(), String> {
        self.commit_with_preferences(
            tasks,
            lists,
            self.sort_mode,
            self.sort_ascending,
            self.show_completed,
        )
    }
    fn commit_with_preferences(
        &mut self,
        mut tasks: Vec<Task>,
        lists: Vec<TaskList>,
        sort_mode: TaskSort,
        sort_ascending: bool,
        show_completed: bool,
    ) -> Result<(), String> {
        tasks::validate_document(&tasks, &lists)?;
        matrix::normalize(&mut tasks);
        if read(&self.path)? != self.snapshot {
            return Err("Task storage changed outside Kara. Changes were not saved. Restart Kara to reload the file.".into());
        }
        let bytes = serde_json::to_vec_pretty(&Document {
            version: 6,
            tasks: tasks.clone(),
            lists: lists.clone(),
            sort_mode,
            sort_ascending: Some(sort_ascending),
            show_completed,
        })
        .map_err(|e| e.to_string())?;
        let dir = self.path.parent().ok_or("Invalid storage path.")?;
        let mut temp = tempfile::NamedTempFile::new_in(dir)
            .map_err(|e| format!("Cannot create save file: {e}"))?;
        temp.write_all(&bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|e| format!("Cannot write task data: {e}"))?;
        if let Some(version) = self.migration_version {
            let backup_suffix = if version < 4 {
                "json.pre-priority-v4.bak"
            } else {
                "json.pre-tasks-v6.bak"
            };
            ensure_migration_backup(
                &self.path,
                self.snapshot.as_deref().ok_or("Missing legacy snapshot.")?,
                backup_suffix,
            )
            .map_err(|e| {
                format!("Cannot back up legacy task data. Migration changes were not saved: {e}")
            })?;
        }
        // Check again after writing/backup, before replacing the destination.
        if read(&self.path)? != self.snapshot {
            return Err("Task storage changed during save. Original data was preserved.".into());
        }
        temp.persist(&self.path)
            .map_err(|e| format!("Cannot replace task data: {e}"))?;
        self.tasks = tasks;
        self.lists = lists;
        self.sort_mode = sort_mode;
        self.sort_ascending = sort_ascending;
        self.show_completed = show_completed;
        self.snapshot = Some(bytes);
        self.migration_version = None;
        sync_committed_directory(dir)
            .map_err(|e| format!("Tasks were written, but durability could not be confirmed: {e}"))
    }
    pub fn upsert(&mut self, mut task: Task) -> Result<(), String> {
        tasks::validate(std::slice::from_ref(&task))?;
        let mut tasks = self.tasks.clone();
        // Ordinary editor saves cannot change ordering, even if a draft has a
        // missing/stale rank. Only scope changes append; reorder has its own API.
        task.matrix_order = tasks
            .iter()
            .find(|old| {
                old.id == task.id && old.list_id == task.list_id && old.priority == task.priority
            })
            .and_then(|old| old.matrix_order);
        if let Some(old) = tasks.iter_mut().find(|old| old.id == task.id) {
            task.created_at = old.created_at;
            *old = task;
        } else {
            task.created_at.get_or_insert_with(tasks::unix_timestamp);
            tasks.push(task);
        }
        self.replace(tasks)
    }
    pub fn classify(&mut self, id: &str, quadrant: Quadrant) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        matrix::classify(&mut tasks, id, quadrant)?;
        self.replace(tasks)
    }
    pub fn reorder(&mut self, id: &str, direction: Direction) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        matrix::reorder(&mut tasks, id, direction)?;
        self.replace(tasks)
    }
    pub fn move_matrix_task(
        &mut self,
        id: &str,
        placement: &matrix::Placement,
    ) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        if matrix::move_task(&mut tasks, id, placement)? {
            self.replace(tasks)?;
        }
        Ok(())
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
    const PORTABLE: &[u8] = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
    #[test]
    fn portable_restore_round_trips_all_fields_preferences_and_recovery_across_roots() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let path = destination.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store.upsert(Task::new("Before restore").unwrap()).unwrap();
        let original = store.backup_data();
        let prepared = crate::backup::decode(PORTABLE).unwrap();
        let recovery = store.restore_backup(&prepared).unwrap();
        assert_eq!(crate::backup::read(&recovery).unwrap().data, original);
        assert_eq!(store.backup_data(), prepared.data);
        let bytes = store.backup_bytes().unwrap();
        let export = source.path().join("portable.kara-backup");
        crate::backup::export(&export, &bytes).unwrap();
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains(destination.path().to_str().unwrap())
        );
        drop(store);
        let mut reopened = Store::open(path.clone()).unwrap();
        assert_eq!(reopened.backup_data(), prepared.data);
        reopened
            .restore_backup(&crate::backup::read(&recovery).unwrap())
            .unwrap();
        assert_eq!(reopened.backup_data(), original);
        // A different data root imports exactly the same record IDs, not copies.
        let mut other = Store::open(source.path().join("tasks.json")).unwrap();
        other
            .restore_backup(&crate::backup::read(&export).unwrap())
            .unwrap();
        let again = other.restore_backup(&prepared).unwrap();
        assert_eq!(other.backup_data(), prepared.data);
        assert_eq!(crate::backup::read(&again).unwrap().data, prepared.data);
    }
    #[test]
    fn legacy_export_does_not_migrate_and_restore_keeps_exact_migration_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        fs::write(&path, LEGACY_MATRIX).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        let exported = store.backup_bytes().unwrap();
        let old = crate::backup::decode(&exported).unwrap();
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        assert_eq!(old.data.tasks, store.tasks());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        let recovery = store
            .restore_backup(&crate::backup::decode(PORTABLE).unwrap())
            .unwrap();
        assert_eq!(
            fs::read(path.with_extension("json.pre-priority-v4.bak")).unwrap(),
            LEGACY_MATRIX
        );
        assert_eq!(crate::backup::read(&recovery).unwrap().data, old.data);
    }
    #[test]
    fn failed_migration_backup_blocks_restore_and_leaves_recoverable_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        fs::write(&path, LEGACY_MATRIX).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        let old = store.backup_data();
        fs::write(
            path.with_extension("json.pre-priority-v4.bak"),
            b"conflicting backup",
        )
        .unwrap();
        let error = store
            .restore_backup(&crate::backup::decode(PORTABLE).unwrap())
            .unwrap_err();
        assert!(error.contains("Previous data remains active"));
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        assert_eq!(store.backup_data(), old);
        let recovery = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "kara-backup"))
            .unwrap();
        assert_eq!(crate::backup::read(&recovery).unwrap().data, old);
        // Resolve only the deliberately created fixture conflict, then retry.
        fs::remove_file(path.with_extension("json.pre-priority-v4.bak")).unwrap();
        store
            .restore_backup(&crate::backup::decode(PORTABLE).unwrap())
            .unwrap();
        assert_eq!(store.tasks().len(), 2);
    }
    #[test]
    fn restore_sync_failure_rolls_back_live_disk_and_restart_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store.upsert(Task::new("Do not lose").unwrap()).unwrap();
        let old = store.backup_data();
        let bytes = fs::read(&path).unwrap();
        FAIL_COMMIT_SYNC.with(|flag| flag.set(true));
        let error = store
            .restore_backup(&crate::backup::decode(PORTABLE).unwrap())
            .unwrap_err();
        assert!(error.contains("Previous data is active again"));
        assert!(error.contains("Rollback: completed"));
        assert_eq!(store.backup_data(), old);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(store);
        assert_eq!(Store::open(path).unwrap().backup_data(), old);
    }
    #[test]
    fn missing_snapshot_permission_and_external_changes_block_restore_without_data_loss() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store.upsert(Task::new("Keep").unwrap()).unwrap();
        let old = store.backup_data();
        let bytes = fs::read(&path).unwrap();
        let prepared = crate::backup::decode(PORTABLE).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.restore_backup(&prepared);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(store.backup_data(), old);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        fs::write(&path, b"external damaged data").unwrap();
        assert!(store.restore_backup(&prepared).is_err());
        assert!(store.backup_bytes().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external damaged data");
        assert_eq!(store.backup_data(), old);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn first_run_empty_store_can_be_restored_and_recovered_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let old = store.backup_data();
        let recovery = store
            .restore_backup(&crate::backup::decode(PORTABLE).unwrap())
            .unwrap();
        store
            .restore_backup(&crate::backup::read(&recovery).unwrap())
            .unwrap();
        assert_eq!(store.backup_data(), old);
        assert!(store.tasks().is_empty());
        drop(store);
        assert!(Store::open(path).unwrap().tasks().is_empty());
    }
    const Q1: Quadrant = Quadrant::ImportantUrgent;
    const Q2: Quadrant = Quadrant::ImportantNotUrgent;
    fn matrix_task(title: &str, list: Option<&str>, quadrant: Quadrant) -> Task {
        let mut task = Task::new(title).unwrap();
        task.list_id = list.map(str::to_string);
        task.priority = quadrant.priority();
        task
    }
    fn matrix_titles(store: &Store, list: Option<&str>, quadrant: Quadrant) -> Vec<String> {
        matrix::ordered(store.tasks(), list, quadrant)
            .into_iter()
            .map(|task| task.title.clone())
            .collect()
    }
    const LEGACY_MATRIX: &[u8] = include_bytes!("../tests/fixtures/priority-v3.json");
    #[test]
    fn matrix_drop_position_priority_and_preferences_survive_restart_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let mut list = TaskList::new("Work").unwrap();
        list.pinned = true;
        store.upsert_list(list.clone()).unwrap();
        store
            .set_preferences(TaskSort::Priority, false, false)
            .unwrap();
        let mut moving = matrix_task("Moving", Some(&list.id), Q1);
        moving.notes = Some("Keep notes".into());
        moving.due_date = Some("2026-10-18".into());
        let id = moving.id.clone();
        let first = matrix_task("First", Some(&list.id), Q2);
        let last = matrix_task("Last", Some(&list.id), Q2);
        let last_id = last.id.clone();
        let mut done = matrix_task("Completed", Some(&list.id), Q1);
        done.completed = true;
        let done_id = done.id.clone();
        for task in [moving, first, last, done, Task::new("Inbox").unwrap()] {
            store.upsert(task).unwrap();
        }
        let document_ids = store
            .tasks()
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>();
        let original = store.tasks().iter().find(|t| t.id == id).unwrap().clone();
        let target = matrix::Placement {
            list_id: Some(list.id.clone()),
            quadrant: Q2,
            before: Some(last_id),
        };
        store.move_matrix_task(&id, &target).unwrap();
        assert_eq!(
            matrix_titles(&store, Some(&list.id), Q2),
            ["First", "Moving", "Last"]
        );
        let target = matrix::Placement {
            before: None,
            ..target
        };
        store.move_matrix_task(&id, &target).unwrap();
        store.move_matrix_task(&done_id, &target).unwrap();
        assert_eq!(
            matrix_titles(&store, Some(&list.id), Q2),
            ["First", "Last", "Moving", "Completed"]
        );
        let expected = store.tasks().to_vec();
        assert_eq!(
            expected.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
            document_ids
        );
        let updated = expected.iter().find(|t| t.id == id).unwrap();
        let mut original = original;
        original.priority = Q2.priority();
        original.matrix_order = updated.matrix_order;
        assert_eq!(updated, &original);
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(store.tasks(), expected);
        assert_eq!(store.lists(), &[list]);
        assert_eq!(store.sort_mode(), TaskSort::Priority);
        assert!(!store.sort_ascending());
        assert!(!store.show_completed());
        assert!(
            store
                .tasks()
                .iter()
                .find(|t| t.id == done_id)
                .unwrap()
                .completed
        );
    }
    #[test]
    fn invalid_noop_denied_and_external_drop_writes_preserve_live_and_disk_state() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let moving = matrix_task("Keep", None, Q1);
        let id = moving.id.clone();
        store.upsert(moving).unwrap();
        let before = fs::read(&path).unwrap();
        let tasks = store.tasks().to_vec();
        let inode = fs::metadata(&path).unwrap().ino();
        let no_op = matrix::Placement {
            list_id: None,
            quadrant: Q1,
            before: None,
        };
        store.move_matrix_task(&id, &no_op).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode); // no save for a no-op
        assert!(
            store
                .move_matrix_task(
                    &id,
                    &matrix::Placement {
                        before: Some("missing".into()),
                        ..no_op
                    }
                )
                .is_err()
        );
        let target = matrix::Placement {
            list_id: None,
            quadrant: Q2,
            before: None,
        };
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.move_matrix_task(&id, &target);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(store.tasks(), tasks);
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, b"external data").unwrap();
        assert!(store.move_matrix_task(&id, &target).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external data");
        assert_eq!(store.tasks(), tasks);
    }
    #[test]
    fn migration_preserves_properties_order_and_exact_backup_then_is_idempotent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let backup = path.with_extension("json.pre-priority-v4.bak");
        fs::write(&path, LEGACY_MATRIX).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        assert!(!backup.exists());
        assert_eq!(
            store.tasks().iter().map(|t| t.priority).collect::<Vec<_>>(),
            [
                Priority::P0,
                Priority::P0,
                Priority::P0,
                Priority::P0,
                Priority::P1,
                Priority::P2,
                Priority::P3,
                Priority::P3
            ]
        );
        assert_eq!(
            matrix_titles(&store, Some("work"), Q1),
            ["P3 rank two", "P3 rank seven"]
        );
        assert_eq!(
            matrix_titles(&store, None, Quadrant::NotImportantNotUrgent),
            ["P0 existing", "Old High", "Old Low", "Old Normal"]
        );
        let original: serde_json::Value = serde_json::from_slice(LEGACY_MATRIX).unwrap();
        let mut transformed = serde_json::to_value(store.tasks()).unwrap();
        let mut unrelated = original["tasks"].clone();
        for collection in [&mut transformed, &mut unrelated] {
            for task in collection.as_array_mut().unwrap() {
                for key in ["priority", "matrix_quadrant", "matrix_order"] {
                    task.as_object_mut().unwrap().remove(key);
                }
            }
        }
        assert_eq!(transformed, unrelated);
        let expected = store.tasks().to_vec();
        store.replace(expected.clone()).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), LEGACY_MATRIX);
        assert_eq!(
            fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let bytes = fs::read(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["version"], 6);
        assert_eq!(saved["lists"], original["lists"]);
        for task in saved["tasks"].as_array().unwrap() {
            assert!(task["priority"].as_u64().is_some_and(|p| p <= 3));
            assert!(task.get("matrix_quadrant").is_none());
        }
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks(), expected);
        store.replace(expected).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read(&backup).unwrap(), LEGACY_MATRIX);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3); // data, lock, backup
    }

    #[test]
    fn v5_upgrade_backs_up_exact_legacy_data_and_persists_preferences_safely() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let backup = path.with_extension("json.pre-tasks-v6.bak");
        let legacy = br#"{"version":5,"tasks":[{"id":"old-a","title":"Old A","completed":true,"due_date":null,"priority":3,"notes":null,"list_id":null,"matrix_order":0},{"id":"old-b","title":"Old B","completed":false,"due_date":null,"priority":3,"notes":null,"list_id":null,"matrix_order":1}],"lists":[],"sort_mode":"DueDate","show_completed":false}"#;
        fs::write(&path, legacy).unwrap();

        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.sort_mode(), TaskSort::DueDate);
        assert!(store.sort_ascending());
        assert!(!store.show_completed());
        assert!(store.tasks().iter().all(|task| task.created_at.is_none()));
        assert!(!backup.exists());
        store
            .set_preferences(TaskSort::Priority, true, false)
            .unwrap();
        assert_eq!(fs::read(&backup).unwrap(), legacy);
        assert_eq!(store.sort_mode(), TaskSort::Priority);
        assert!(store.sort_ascending());
        assert!(!store.show_completed());
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 6);
        assert_eq!(saved["sort_mode"], "Priority");
        assert_eq!(saved["sort_ascending"], true);
        assert_eq!(saved["show_completed"], false);
        assert!(
            saved["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|task| task.get("created_at").is_none())
        );
        assert_eq!(
            store
                .tasks()
                .iter()
                .map(|task| task.matrix_order)
                .collect::<Vec<_>>(),
            [Some(1), Some(0)]
        );
        drop(store);

        let restored = Store::open(path).unwrap();
        assert_eq!(restored.sort_mode(), TaskSort::Priority);
        assert!(restored.sort_ascending());
        assert!(!restored.show_completed());
        assert!(
            restored
                .tasks()
                .iter()
                .all(|task| task.created_at.is_none())
        );
        assert_eq!(fs::read(backup).unwrap(), legacy);
    }

    #[test]
    fn v4_upgrade_preserves_creation_default_and_recovers_exact_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let backup = path.with_extension("json.pre-tasks-v6.bak");
        let legacy = br#"{"version":4,"tasks":[],"lists":[]}"#;
        fs::write(&path, legacy).unwrap();

        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.sort_mode(), TaskSort::CreationDate);
        assert!(!store.sort_ascending());
        assert!(store.show_completed());
        store
            .set_preferences(
                TaskSort::CreationDate,
                TaskSort::CreationDate.default_ascending(),
                true,
            )
            .unwrap();
        assert_eq!(fs::read(&backup).unwrap(), legacy);
        drop(store);
        let restored = Store::open(path).unwrap();
        assert_eq!(restored.sort_mode(), TaskSort::CreationDate);
        assert!(!restored.sort_ascending());
        assert!(restored.show_completed());
    }

    #[test]
    fn new_task_creation_timestamp_survives_save_and_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let task = Task::new("Stamped").unwrap();
        let created_at = task.created_at;
        store.upsert(task.clone()).unwrap();
        let mut edited = task;
        edited.title = "Edited".into();
        edited.created_at = None;
        store.upsert(edited).unwrap();
        assert_eq!(store.tasks()[0].created_at, created_at);
        drop(store);
        let restored = Store::open(path).unwrap();
        assert_eq!(restored.tasks()[0].created_at, created_at);
    }
    #[test]
    fn legacy_versions_one_and_two_migrate_on_copies_with_verified_backups() {
        for version in [1, 2] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("tasks.json");
            let mut doc: serde_json::Value = serde_json::from_slice(LEGACY_MATRIX).unwrap();
            doc["version"] = serde_json::json!(version);
            for task in doc["tasks"].as_array_mut().unwrap() {
                task.as_object_mut().unwrap().remove("matrix_quadrant");
                task.as_object_mut().unwrap().remove("matrix_order");
            }
            let original = serde_json::to_vec(&doc).unwrap();
            fs::write(&path, &original).unwrap();
            let mut store = Store::open(path.clone()).unwrap();
            assert!(store.tasks().iter().all(|t| t.priority == Priority::P0));
            let expected = store.tasks().to_vec();
            store.replace(expected.clone()).unwrap();
            assert_eq!(
                fs::read(path.with_extension("json.pre-priority-v4.bak")).unwrap(),
                original
            );
            drop(store);
            assert_eq!(Store::open(path).unwrap().tasks(), expected);
        }
    }
    #[test]
    fn migration_recovers_after_backup_publication_without_rewriting_legacy_early() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        fs::write(&path, LEGACY_MATRIX).unwrap();
        // Model interruption after durable backup publication but before replacement.
        ensure_migration_backup(&path, LEGACY_MATRIX, "json.pre-priority-v4.bak").unwrap();
        ensure_migration_backup(&path, LEGACY_MATRIX, "json.pre-priority-v4.bak").unwrap();
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        let mut store = Store::open(path.clone()).unwrap();
        let expected = store.tasks().to_vec();
        store.replace(expected.clone()).unwrap();
        drop(store);
        assert_eq!(Store::open(path.clone()).unwrap().tasks(), expected);
        assert_eq!(
            fs::read(path.with_extension("json.pre-priority-v4.bak")).unwrap(),
            LEGACY_MATRIX
        );
    }
    #[test]
    fn conflicting_backup_blocks_migration_without_changing_live_or_disk_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let backup = path.with_extension("json.pre-priority-v4.bak");
        fs::write(&path, LEGACY_MATRIX).unwrap();
        fs::write(&backup, b"different existing backup").unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        let expected = store.tasks().to_vec();
        assert!(
            store
                .toggle("unclassified-high")
                .unwrap_err()
                .contains("backup")
        );
        assert_eq!(store.tasks(), expected);
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        assert_eq!(fs::read(&backup).unwrap(), b"different existing backup");
        fs::remove_file(&backup).unwrap(); // disposable fixture only
        std::os::unix::fs::symlink(&path, &backup).unwrap();
        assert!(store.replace(expected.clone()).is_err());
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        fs::remove_file(&backup).unwrap();
        store.replace(expected).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), LEGACY_MATRIX);
    }
    #[test]
    fn denied_backup_and_external_legacy_changes_never_publish_migration() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        fs::write(&path, LEGACY_MATRIX).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = ensure_migration_backup(&path, LEGACY_MATRIX, "json.pre-priority-v4.bak");
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), LEGACY_MATRIX);
        let mut store = Store::open(path.clone()).unwrap();
        let expected = store.tasks().to_vec();
        fs::write(&path, b"external corrupt data").unwrap();
        assert!(store.replace(expected.clone()).is_err());
        assert_eq!(store.tasks(), expected);
        assert_eq!(fs::read(&path).unwrap(), b"external corrupt data");
        assert!(!path.with_extension("json.pre-priority-v4.bak").exists());
    }
    #[test]
    fn malformed_legacy_priority_quadrant_and_rank_never_migrate_or_create_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let base: serde_json::Value = serde_json::from_slice(LEGACY_MATRIX).unwrap();
        let mut cases = vec![];
        for (key, value) in [
            ("priority", serde_json::json!(3)),
            ("priority", serde_json::json!("Unknown")),
            ("matrix_quadrant", serde_json::json!("Unknown")),
            ("matrix_quadrant", serde_json::json!(2)),
            ("matrix_order", serde_json::json!(-1)),
            ("matrix_order", serde_json::json!(1.5)),
            ("unknown_field", serde_json::json!(true)),
        ] {
            let mut doc = base.clone();
            doc["tasks"][0][key] = value;
            cases.push(doc);
        }
        for doc in cases {
            let bytes = serde_json::to_vec(&doc).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Store::open(path.clone()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert!(!path.with_extension("json.pre-priority-v4.bak").exists());
        }
    }
    #[test]
    fn editor_priority_changes_append_and_all_records_appear_in_one_quadrant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path).unwrap();
        let mut moving = Task::new("Moving").unwrap();
        let destination = matrix_task("Destination", None, Q1);
        let done = {
            let mut t = matrix_task("Done", None, Q1);
            t.completed = true;
            t
        };
        for task in [moving.clone(), destination, done] {
            store.upsert(task).unwrap();
        }
        moving.priority = Priority::P3;
        store.upsert(moving.clone()).unwrap();
        assert_eq!(
            matrix_titles(&store, None, Q1),
            ["Destination", "Moving", "Done"]
        );
        let ids: Vec<_> = Quadrant::ALL
            .into_iter()
            .flat_map(|q| matrix::ordered(store.tasks(), None, q))
            .map(|t| &t.id)
            .collect();
        assert_eq!(ids.len(), store.tasks().len());
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            ids.len()
        );
        store.toggle(&moving.id).unwrap();
        assert_eq!(
            store
                .tasks()
                .iter()
                .find(|t| t.id == moving.id)
                .unwrap()
                .priority,
            Priority::P3
        );
    }
    #[test]
    fn matrix_edits_and_list_moves_preserve_records_and_append_to_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let mut list = TaskList::new("Work").unwrap();
        list.pinned = true;
        store.upsert_list(list.clone()).unwrap();
        let inbox = matrix_task("Inbox existing", None, Q1);
        store.upsert(inbox).unwrap();
        let first = matrix_task("First", Some(&list.id), Q1);
        let mut second = matrix_task("Second", Some(&list.id), Q1);
        second.notes = Some("Preserve notes\nline 2".into());
        second.due_date = Some("2026-10-18".into());
        second.priority = tasks::Priority::P3;
        let id = second.id.clone();
        store.upsert(first).unwrap();
        store.upsert(second).unwrap();
        store.reorder(&id, Direction::Up).unwrap();
        assert_eq!(
            matrix_titles(&store, Some(&list.id), Q1),
            ["Second", "First"]
        );
        let mut edit = store
            .tasks()
            .iter()
            .find(|task| task.id == id)
            .unwrap()
            .clone();
        edit.title = "Edited".into();
        edit.matrix_order = None; // a stale/missing draft rank must not reorder an edit
        store.upsert(edit.clone()).unwrap();
        assert_eq!(
            matrix_titles(&store, Some(&list.id), Q1),
            ["Edited", "First"]
        );
        edit.list_id = None;
        store.upsert(edit).unwrap();
        assert_eq!(
            matrix_titles(&store, None, Q1),
            ["Inbox existing", "Edited"]
        );
        assert_eq!(matrix_titles(&store, Some(&list.id), Q1), ["First"]);
        let saved = store.tasks().to_vec();
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks(), saved);
        assert_eq!(store.tasks().len(), 3);
        assert!(store.lists()[0].pinned);
        let task = store.tasks().iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.priority.quadrant(), Q1);
        assert_eq!(task.notes.as_deref(), Some("Preserve notes\nline 2"));
        assert_eq!(task.priority, tasks::Priority::P3);
        store.toggle(&id).unwrap();
        let completed = store.tasks().iter().find(|task| task.id == id).unwrap();
        assert!(completed.completed);
        assert_eq!(completed.priority.quadrant(), Q1);
        let mut moving = completed.clone();
        moving.list_id = Some(list.id.clone());
        store.upsert(moving).unwrap();
        assert!(
            store
                .tasks()
                .iter()
                .find(|task| task.id == id)
                .unwrap()
                .completed
        );
        assert_eq!(
            matrix_titles(&store, Some(&list.id), Q1),
            ["First", "Edited"]
        );
        store.toggle(&id).unwrap();
        store.classify(&id, Q2).unwrap();
        assert_eq!(matrix_titles(&store, Some(&list.id), Q2), ["Edited"]);
        store
            .classify(&id, Quadrant::NotImportantNotUrgent)
            .unwrap();
        let task = store.tasks().iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.matrix_order, Some(0));
        assert_eq!(task.priority, Priority::P0);
        assert_eq!(task.list_id.as_deref(), Some(list.id.as_str()));
        drop(store);
        assert_eq!(Store::open(path).unwrap().tasks().len(), 3);
    }
    #[test]
    fn matrix_list_deletion_appends_in_prior_order_and_normalizes_after_task_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let list = TaskList::new("Work").unwrap();
        store.upsert_list(list.clone()).unwrap();
        let inbox = matrix_task("Inbox", None, Q1);
        let first = matrix_task("First", Some(&list.id), Q1);
        let second = matrix_task("Second", Some(&list.id), Q1);
        let mut done = matrix_task("Done", Some(&list.id), Q1);
        done.completed = true;
        let mut inbox_done = matrix_task("Inbox done", None, Q1);
        inbox_done.completed = true;
        let second_id = second.id.clone();
        let first_id = first.id.clone();
        for task in [inbox, first, second, done, inbox_done] {
            store.upsert(task).unwrap();
        }
        store.reorder(&second_id, Direction::Up).unwrap();
        store.delete_list(&list.id).unwrap();
        assert_eq!(
            matrix_titles(&store, None, Q1),
            ["Inbox", "Second", "First", "Inbox done", "Done"]
        );
        assert_eq!(store.tasks().len(), 5);
        assert!(
            store
                .tasks()
                .iter()
                .all(|task| task.list_id.is_none() && task.priority.quadrant() == Q1)
        );
        store.delete(&first_id).unwrap();
        let ranks: Vec<_> = matrix::ordered(store.tasks(), None, Q1)
            .iter()
            .map(|task| task.matrix_order.unwrap())
            .collect();
        assert_eq!(ranks, [0, 1, 2, 3]);
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(
            matrix_titles(&store, None, Q1),
            ["Inbox", "Second", "Inbox done", "Done"]
        );
        assert!(store.lists().is_empty());
    }
    #[test]
    fn legacy_polished_tasks_default_to_p0_and_pinned_lists_survive_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let bytes = br#"{"version":2,"tasks":[{"id":"original-id","title":"Legacy completed","completed":true,"due_date":"2026-10-18","priority":"High","notes":"Original notes","list_id":"work-id"}],"lists":[{"id":"work-id","name":"Work","pinned":true}]}"#;
        fs::write(&path, bytes).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks()[0].priority, Priority::P0);
        assert_eq!(store.tasks()[0].matrix_order, Some(0));
        assert!(store.lists()[0].pinned);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        store.classify("original-id", Q2).unwrap();
        drop(store);
        let store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks()[0].priority.quadrant(), Q2);
        assert!(store.tasks()[0].completed);
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 6);
        let mut legacy: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        saved["tasks"][0]
            .as_object_mut()
            .unwrap()
            .remove("matrix_order");
        for tasks in [&mut saved["tasks"], &mut legacy["tasks"]] {
            for task in tasks.as_array_mut().unwrap() {
                task.as_object_mut().unwrap().remove("priority");
                task.as_object_mut().unwrap().remove("matrix_order");
            }
        }
        assert_eq!(saved["tasks"], legacy["tasks"]);
        assert_eq!(saved["lists"], legacy["lists"]);
    }
    #[test]
    fn imported_matrix_ranks_normalize_stably_without_rewriting_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut tasks = vec![
            matrix_task("Missing", None, Q1),
            matrix_task("Duplicate1", None, Q1),
            matrix_task("Duplicate2", None, Q1),
            matrix_task("Other quadrant", None, Q2),
        ];
        tasks[1].matrix_order = Some(40);
        tasks[2].matrix_order = Some(40);
        tasks[3].matrix_order = Some(u64::MAX);
        let bytes = serde_json::to_vec(&Document {
            version: 4,
            tasks,
            lists: vec![],
            sort_mode: TaskSort::default(),
            sort_ascending: None,
            show_completed: true,
        })
        .unwrap();
        fs::write(&path, &bytes).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            matrix_titles(&store, None, Q1),
            ["Duplicate1", "Duplicate2", "Missing"]
        );
        let ranks: Vec<_> = matrix::ordered(store.tasks(), None, Q1)
            .iter()
            .map(|task| task.matrix_order)
            .collect();
        assert_eq!(ranks, [Some(0), Some(1), Some(2)]);
        store
            .reorder(&store.tasks()[0].id.clone(), Direction::Up)
            .unwrap();
        let expected = store.tasks().to_vec();
        drop(store);
        assert_eq!(Store::open(path).unwrap().tasks(), expected);
    }
    #[test]
    fn corrupt_priority_matrix_and_retired_fields_are_preserved_and_block_loading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let task = matrix_task("Bad data", None, Q1);
        let base = serde_json::json!({"version":4,"tasks":[task],"lists":[]});
        let mut cases = vec![];
        for value in [serde_json::json!("not-a-quadrant"), serde_json::json!(1)] {
            let mut doc = base.clone();
            doc["tasks"][0]["matrix_quadrant"] = value;
            cases.push(doc);
        }
        for value in [
            serde_json::json!(-1),
            serde_json::json!("zero"),
            serde_json::json!(0.5),
        ] {
            let mut doc = base.clone();
            doc["tasks"][0]["matrix_order"] = value;
            cases.push(doc);
        }
        for value in [
            serde_json::json!(-1),
            serde_json::json!(4),
            serde_json::json!(3.0),
            serde_json::json!("High"),
            serde_json::Value::Null,
        ] {
            let mut doc = base.clone();
            doc["tasks"][0]["priority"] = value;
            cases.push(doc);
        }
        let mut missing_priority = base.clone();
        missing_priority["tasks"][0]
            .as_object_mut()
            .unwrap()
            .remove("priority");
        cases.push(missing_priority);
        let mut retired_quadrant = base.clone();
        retired_quadrant["tasks"][0]["matrix_quadrant"] = serde_json::Value::Null;
        cases.push(retired_quadrant);
        let mut unknown = base;
        unknown["tasks"][0]["matrix_future_field"] = serde_json::json!(true);
        cases.push(unknown);
        for doc in cases {
            let bytes = serde_json::to_vec(&doc).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Store::open(path.clone()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn failed_matrix_writes_preserve_classification_ranks_and_data_then_retry() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let first = matrix_task("First", None, Q1);
        let second = matrix_task("Second", None, Q1);
        let id = first.id.clone();
        store.upsert(first).unwrap();
        store.upsert(second).unwrap();
        let before = fs::read(&path).unwrap();
        let tasks = store.tasks().to_vec();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let classify = store.classify(&id, Q2);
        let reorder = store.reorder(&id, Direction::Down);
        let complete = store.toggle(&id);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(classify.is_err());
        assert!(reorder.is_err());
        assert!(complete.is_err());
        assert_eq!(store.tasks(), tasks);
        assert_eq!(fs::read(&path).unwrap(), before);
        store.reorder(&id, Direction::Down).unwrap();
        store.classify(&id, Q2).unwrap();
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(matrix_titles(&store, None, Q2), ["First"]);
        fs::write(&path, b"external corruption").unwrap();
        assert!(store.classify(&id, Q1).is_err());
        assert!(store.reorder(&id, Direction::Up).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external corruption");
        assert_eq!(matrix_titles(&store, None, Q2), ["First"]);
    }
    #[test]
    fn lists_and_task_moves_round_trip_with_stable_ids_and_no_duplicate_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let mut first = TaskList::new("Work").unwrap();
        let mut second = TaskList::new("Home").unwrap();
        second.pinned = true;
        store.upsert_list(first.clone()).unwrap();
        store.upsert_list(second.clone()).unwrap();
        let mut task = Task::new("Move me").unwrap();
        task.matrix_order = Some(0);
        let task_id = task.id.clone();
        store.upsert(task.clone()).unwrap();
        for assignment in [
            Some(first.id.clone()),
            Some(second.id.clone()),
            None,
            Some(first.id.clone()),
        ] {
            task.list_id = assignment;
            store.upsert(task.clone()).unwrap();
            drop(store);
            store = Store::open(path.clone()).unwrap();
            assert_eq!(store.tasks(), std::slice::from_ref(&task));
            assert_eq!(store.tasks()[0].id, task_id);
        }
        store.toggle(&task_id).unwrap();
        assert_eq!(store.tasks()[0].list_id.as_deref(), Some(first.id.as_str()));
        first.name = "  Career  ".into();
        store.upsert_list(first.clone()).unwrap();
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(store.lists().len(), 2);
        assert_eq!(store.lists()[0].id, first.id);
        assert_eq!(store.lists()[0].name, "Career");
        assert!(store.lists()[1].pinned);
        assert_eq!(store.tasks()[0].list_id.as_deref(), Some(first.id.as_str()));
        assert!(store.tasks()[0].completed);
    }
    #[test]
    fn deleting_a_list_moves_all_its_tasks_to_inbox_in_one_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let first = TaskList::new("Delete this").unwrap();
        let second = TaskList::new("Keep this").unwrap();
        store.upsert_list(first.clone()).unwrap();
        store.upsert_list(second.clone()).unwrap();
        let mut active = Task::new("Active").unwrap();
        active.matrix_order = Some(0);
        active.list_id = Some(first.id.clone());
        active.notes = Some("Keep all fields".into());
        active.due_date = Some("2026-10-18".into());
        active.priority = tasks::Priority::P3;
        let mut completed = Task::new("Completed").unwrap();
        completed.matrix_order = Some(0);
        completed.list_id = Some(first.id.clone());
        completed.completed = true;
        let mut other = Task::new("Other").unwrap();
        other.matrix_order = Some(0);
        other.list_id = Some(second.id.clone());
        store
            .replace(vec![active.clone(), completed.clone(), other.clone()])
            .unwrap();
        let before = fs::read(&path).unwrap();
        let mut reader = File::open(&path).unwrap();
        store.delete_list(&first.id).unwrap();
        use std::io::Read;
        let mut old = Vec::new();
        reader.read_to_end(&mut old).unwrap();
        assert_eq!(old, before);
        active.list_id = None;
        completed.list_id = None;
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(store.tasks(), &[active, completed, other]);
        assert_eq!(store.lists(), &[second]);
        let today = tasks::parse_date("2026-10-18").unwrap();
        assert!(store.tasks()[0].visible(tasks::TaskView::Inbox, today));
        assert!(store.tasks()[1].visible(tasks::TaskView::Inbox, today));
        assert!(store.tasks()[1].completed);
    }
    #[test]
    fn stage1_files_load_without_rewriting_then_upgrade_without_losing_task_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        // Literal Stage 1 schema, not serialized with the Stage 2 document type.
        let bytes = br#"{"version":1,"tasks":[{"id":"stage1-id","title":"Legacy task","completed":true,"due_date":"2026-10-18","priority":"High","notes":"Notes\nsecond line","list_id":null}]}"#;
        fs::write(&path, bytes).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        let original = store.tasks().to_vec();
        assert!(store.lists().is_empty());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        store
            .upsert_list(TaskList::new("New list").unwrap())
            .unwrap();
        drop(store);
        let store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks(), original);
        assert_eq!(store.lists().len(), 1);
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 6);
        let mut legacy: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        for tasks in [&mut saved["tasks"], &mut legacy["tasks"]] {
            for task in tasks.as_array_mut().unwrap() {
                task.as_object_mut().unwrap().remove("priority");
                task.as_object_mut().unwrap().remove("matrix_order");
            }
        }
        assert_eq!(saved["tasks"], legacy["tasks"]);
    }
    #[test]
    fn stage1_reserved_list_references_are_recovered_without_changing_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut task = Task::new("Legacy assignment").unwrap();
        task.list_id = Some("reserved-id".into());
        task.created_at = None;
        let mut legacy = serde_json::to_value(&task).unwrap();
        legacy["priority"] = serde_json::json!("Normal");
        let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"tasks":[legacy]})).unwrap();
        task.matrix_order = Some(0);
        fs::write(&path, &bytes).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks(), std::slice::from_ref(&task));
        assert_eq!(store.lists()[0].id, "reserved-id");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        store.toggle(&task.id).unwrap();
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(store.lists()[0].id, "reserved-id");
        assert_eq!(store.tasks()[0].list_id, task.list_id);
    }
    #[test]
    fn invalid_list_documents_and_dangling_references_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let list = TaskList::new("Work").unwrap();
        let mut duplicate_name = TaskList::new(" work ").unwrap();
        duplicate_name.name = " work ".into();
        let mut duplicate_id = TaskList::new("Other").unwrap();
        duplicate_id.id = list.id.clone();
        let mut blank = list.clone();
        blank.name = "  ".into();
        let mut empty_id = list.clone();
        empty_id.id.clear();
        for lists in [
            vec![list.clone(), duplicate_name],
            vec![list.clone(), duplicate_id],
            vec![blank],
            vec![empty_id],
        ] {
            let bytes = serde_json::to_vec(&Document {
                version: 4,
                tasks: vec![],
                lists,
                sort_mode: TaskSort::default(),
                sort_ascending: None,
                show_completed: true,
            })
            .unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Store::open(path.clone()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let mut task = Task::new("Dangling").unwrap();
        task.list_id = Some("missing".into());
        let bytes = serde_json::to_vec(&Document {
            version: 4,
            tasks: vec![task],
            lists: vec![list],
            sort_mode: TaskSort::default(),
            sort_ascending: None,
            show_completed: true,
        })
        .unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(Store::open(path.clone()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn invalid_list_changes_are_rejected_without_touching_the_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let list = TaskList::new("Work").unwrap();
        store.upsert_list(list.clone()).unwrap();
        let other = TaskList::new("Home").unwrap();
        store.upsert_list(other.clone()).unwrap();
        let before = fs::read(&path).unwrap();
        let mut duplicate_rename = other.clone();
        duplicate_rename.name = "WORK".into();
        assert!(store.upsert_list(duplicate_rename).is_err());
        assert!(store.upsert_list(TaskList::new(" WORK ").unwrap()).is_err());
        let mut renamed = list.clone();
        renamed.name = " ".into();
        assert!(store.upsert_list(renamed).is_err());
        let mut task = Task::new("Dangling").unwrap();
        task.list_id = Some("missing".into());
        assert!(store.upsert(task).is_err());
        assert!(store.delete_list("missing").is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(store.lists(), &[list, other]);
        assert!(store.tasks().is_empty());
    }
    #[test]
    fn failed_list_writes_keep_both_assignments_and_definitions_then_retry() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let list = TaskList::new("Work").unwrap();
        store.upsert_list(list.clone()).unwrap();
        let mut task = Task::new("Keep association").unwrap();
        task.matrix_order = Some(0);
        task.list_id = Some(list.id.clone());
        store.upsert(task.clone()).unwrap();
        let before = fs::read(&path).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let deletion = store.delete_list(&list.id);
        let creation = store.upsert_list(TaskList::new("Unsaved").unwrap());
        let mut renamed = list.clone();
        renamed.name = "Unsaved rename".into();
        let rename = store.upsert_list(renamed);
        let mut moved = task.clone();
        moved.list_id = None;
        let movement = store.upsert(moved);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        for result in [deletion, creation, rename, movement] {
            assert!(result.is_err());
        }
        assert_eq!(store.tasks(), std::slice::from_ref(&task));
        assert_eq!(store.lists(), std::slice::from_ref(&list));
        assert_eq!(fs::read(&path).unwrap(), before);
        store.delete_list(&list.id).unwrap();
        assert!(store.lists().is_empty());
        assert_eq!(store.tasks()[0].list_id, None);
        drop(store);
        let store = Store::open(path).unwrap();
        assert_eq!(store.tasks().len(), 1);
        assert!(store.lists().is_empty());
    }
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
        task.matrix_order = Some(0);
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
        let mut task = Task::new("Keep me").unwrap();
        task.matrix_order = Some(0);
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
        task.matrix_order = Some(0);
        let id = task.id.clone();
        store.upsert(task.clone()).unwrap();
        task.title = "Edited".into();
        task.due_date = Some("2026-10-17".into());
        task.notes = Some("Notes\nsecond line".into());
        task.priority = tasks::Priority::P3;
        let list = TaskList::new("Future").unwrap();
        store.upsert_list(list.clone()).unwrap();
        task.list_id = Some(list.id);
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
                lists: vec![],
                tasks: vec![],
                sort_mode: TaskSort::default(),
                sort_ascending: None,
                show_completed: true,
            })
            .unwrap(),
            serde_json::to_vec(&Document {
                version: 4,
                lists: vec![],
                tasks: vec![task.clone(), task.clone()],
                sort_mode: TaskSort::default(),
                sort_ascending: None,
                show_completed: true,
            })
            .unwrap(),
            serde_json::to_vec(&Document {
                version: 4,
                lists: vec![],
                tasks: vec![invalid],
                sort_mode: TaskSort::default(),
                sort_ascending: None,
                show_completed: true,
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
        let mut task = Task::new("Saved").unwrap();
        task.matrix_order = Some(0);
        store.upsert(task.clone()).unwrap();
        fs::write(&path, b"external data").unwrap();
        assert!(
            store
                .upsert_list(TaskList::new("Not saved").unwrap())
                .is_err()
        );
        assert!(store.lists().is_empty());
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
