// SPDX-License-Identifier: MPL-2.0
use crate::{
    matrix::{self, Direction},
    tasks::{self, Quadrant, Task, TaskList},
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
}

pub struct Store {
    path: PathBuf,
    _lock: File,
    snapshot: Option<Vec<u8>>,
    tasks: Vec<Task>,
    lists: Vec<TaskList>,
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
        let (tasks, lists) = match &snapshot {
            None => (Vec::new(), Vec::new()),
            Some(bytes) => {
                let mut doc: Document = serde_json::from_slice(bytes).map_err(|e| {
                    format!(
                        "Invalid task data in {}: {e}. The file has been preserved.",
                        path.display()
                    )
                })?;
                if ![1, 2, 3].contains(&doc.version) {
                    return Err(format!(
                        "Unsupported task data version {}. The file has been preserved.",
                        doc.version
                    ));
                }
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
                (doc.tasks, doc.lists)
            }
        };
        Ok(Self {
            path,
            _lock: lock,
            snapshot,
            tasks,
            lists,
        })
    }
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
    pub fn lists(&self) -> &[TaskList] {
        &self.lists
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
    fn commit(&mut self, mut tasks: Vec<Task>, lists: Vec<TaskList>) -> Result<(), String> {
        tasks::validate_document(&tasks, &lists)?;
        matrix::normalize(&mut tasks);
        if read(&self.path)? != self.snapshot {
            return Err("Task storage changed outside Kara. Changes were not saved. Restart Kara to reload the file.".into());
        }
        let bytes = serde_json::to_vec_pretty(&Document {
            version: 3,
            tasks: tasks.clone(),
            lists: lists.clone(),
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
        self.lists = lists;
        self.snapshot = Some(bytes);
        File::open(dir)
            .and_then(|f| f.sync_all())
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
                old.id == task.id
                    && old.list_id == task.list_id
                    && old.matrix_quadrant == task.matrix_quadrant
            })
            .and_then(|old| old.matrix_order);
        if let Some(old) = tasks.iter_mut().find(|old| old.id == task.id) {
            *old = task;
        } else {
            tasks.push(task);
        }
        self.replace(tasks)
    }
    pub fn classify(&mut self, id: &str, quadrant: Option<Quadrant>) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        matrix::classify(&mut tasks, id, quadrant)?;
        self.replace(tasks)
    }
    pub fn reorder(&mut self, id: &str, direction: Direction) -> Result<(), String> {
        let mut tasks = self.tasks.clone();
        matrix::reorder(&mut tasks, id, direction)?;
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
    const Q1: Quadrant = Quadrant::ImportantUrgent;
    const Q2: Quadrant = Quadrant::ImportantNotUrgent;
    fn matrix_task(title: &str, list: Option<&str>, quadrant: Quadrant) -> Task {
        let mut task = Task::new(title).unwrap();
        task.list_id = list.map(str::to_string);
        task.matrix_quadrant = Some(quadrant);
        task
    }
    fn matrix_titles(store: &Store, list: Option<&str>, quadrant: Quadrant) -> Vec<String> {
        matrix::ordered(store.tasks(), list, Some(quadrant))
            .into_iter()
            .map(|task| task.title.clone())
            .collect()
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
        second.priority = tasks::Priority::High;
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
        assert_eq!(task.matrix_quadrant, Some(Q1));
        assert_eq!(task.notes.as_deref(), Some("Preserve notes\nline 2"));
        assert_eq!(task.priority, tasks::Priority::High);
        store.toggle(&id).unwrap();
        let completed = store.tasks().iter().find(|task| task.id == id).unwrap();
        assert!(completed.completed);
        assert_eq!(completed.matrix_quadrant, Some(Q1));
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
        store.classify(&id, Some(Q2)).unwrap();
        assert_eq!(matrix_titles(&store, Some(&list.id), Q2), ["Edited"]);
        store.classify(&id, None).unwrap();
        let task = store.tasks().iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.matrix_order, None);
        assert_eq!(task.matrix_quadrant, None);
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
                .all(|task| task.list_id.is_none() && task.matrix_quadrant == Some(Q1))
        );
        store.delete(&first_id).unwrap();
        let ranks: Vec<_> = matrix::ordered(store.tasks(), None, Some(Q1))
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
    fn legacy_polished_tasks_default_to_unassigned_and_pinned_lists_survive_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let bytes = br#"{"version":2,"tasks":[{"id":"original-id","title":"Legacy completed","completed":true,"due_date":"2026-10-18","priority":"High","notes":"Original notes","list_id":"work-id"}],"lists":[{"id":"work-id","name":"Work","pinned":true}]}"#;
        fs::write(&path, bytes).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks()[0].matrix_quadrant, None);
        assert_eq!(store.tasks()[0].matrix_order, None);
        assert!(store.lists()[0].pinned);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        store.classify("original-id", Some(Q2)).unwrap();
        drop(store);
        let store = Store::open(path.clone()).unwrap();
        assert_eq!(store.tasks()[0].matrix_quadrant, Some(Q2));
        assert!(store.tasks()[0].completed);
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 3);
        let legacy: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        saved["tasks"][0]
            .as_object_mut()
            .unwrap()
            .remove("matrix_quadrant");
        saved["tasks"][0]
            .as_object_mut()
            .unwrap()
            .remove("matrix_order");
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
            version: 3,
            tasks,
            lists: vec![],
        })
        .unwrap();
        fs::write(&path, &bytes).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            matrix_titles(&store, None, Q1),
            ["Duplicate1", "Duplicate2", "Missing"]
        );
        let ranks: Vec<_> = matrix::ordered(store.tasks(), None, Some(Q1))
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
    fn corrupt_matrix_fields_are_preserved_and_block_loading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let task = matrix_task("Bad data", None, Q1);
        let base = serde_json::json!({"version":3,"tasks":[task],"lists":[]});
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
        let mut no_quadrant = base.clone();
        no_quadrant["tasks"][0]["matrix_quadrant"] = serde_json::Value::Null;
        no_quadrant["tasks"][0]["matrix_order"] = serde_json::json!(0);
        cases.push(no_quadrant);
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
        let classify = store.classify(&id, Some(Q2));
        let reorder = store.reorder(&id, Direction::Down);
        let complete = store.toggle(&id);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(classify.is_err());
        assert!(reorder.is_err());
        assert!(complete.is_err());
        assert_eq!(store.tasks(), tasks);
        assert_eq!(fs::read(&path).unwrap(), before);
        store.reorder(&id, Direction::Down).unwrap();
        store.classify(&id, Some(Q2)).unwrap();
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(matrix_titles(&store, None, Q2), ["First"]);
        fs::write(&path, b"external corruption").unwrap();
        assert!(store.classify(&id, Some(Q1)).is_err());
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
        active.list_id = Some(first.id.clone());
        active.notes = Some("Keep all fields".into());
        active.due_date = Some("2026-10-18".into());
        active.priority = tasks::Priority::High;
        let mut completed = Task::new("Completed").unwrap();
        completed.list_id = Some(first.id.clone());
        completed.completed = true;
        let mut other = Task::new("Other").unwrap();
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
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 3);
        let legacy: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(saved["tasks"], legacy["tasks"]);
    }
    #[test]
    fn stage1_reserved_list_references_are_recovered_without_changing_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut task = Task::new("Legacy assignment").unwrap();
        task.list_id = Some("reserved-id".into());
        let bytes =
            serde_json::to_vec(&serde_json::json!({"version":1,"tasks":[task.clone()]})).unwrap();
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
                version: 2,
                tasks: vec![],
                lists,
            })
            .unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Store::open(path.clone()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let mut task = Task::new("Dangling").unwrap();
        task.list_id = Some("missing".into());
        let bytes = serde_json::to_vec(&Document {
            version: 2,
            tasks: vec![task],
            lists: vec![list],
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
            })
            .unwrap(),
            serde_json::to_vec(&Document {
                version: 1,
                lists: vec![],
                tasks: vec![task.clone(), task.clone()],
            })
            .unwrap(),
            serde_json::to_vec(&Document {
                version: 1,
                lists: vec![],
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
