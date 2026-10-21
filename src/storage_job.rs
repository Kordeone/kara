// SPDX-License-Identifier: MPL-2.0
//! Exactly one owned Store moves through a blocking worker. UI snapshots never write.
use crate::{automatic, backup, import, storage::Store};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone)]
pub enum Job {
    Export,
    Import,
    Folder,
    Verify(PathBuf),
    Add(Arc<backup::Prepared>),
    Replace(Arc<backup::Prepared>),
    Clear,
    Configure {
        enabled: bool,
        interval: String,
        retention: u16,
        destination: Option<PathBuf>,
    },
    Automatic(i64),
    #[cfg(test)]
    Pause(
        Arc<tokio::sync::Notify>,
        Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
    ),
    #[cfg(test)]
    Panic,
    #[cfg(test)]
    FailReplace(Arc<backup::Prepared>),
}
impl Job {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Export => "Exporting backup…",
            Self::Import => "Reading and validating backup…",
            Self::Folder => "Choosing and checking backup folder…",
            Self::Verify(_) => "Checking backup folder…",
            Self::Add(_) => "Adding backup records…",
            Self::Replace(_) => "Restoring backup…",
            Self::Clear => "Creating recovery backup and clearing data…",
            Self::Configure { .. } => "Saving backup settings…",
            Self::Automatic(_) => "Creating automatic backup…",
            #[cfg(test)]
            Self::Pause(..) | Self::Panic | Self::FailReplace(_) => "Testing background work…",
        }
    }
}
pub type ImportPreview = (Arc<backup::Prepared>, Result<import::Summary, String>);
pub type AddResult = Result<(Option<PathBuf>, import::Summary), String>;
#[derive(Debug)]
pub enum Outcome {
    Export(Result<Option<PathBuf>, String>),
    Import(Result<Option<ImportPreview>, String>),
    Folder(Result<Option<PathBuf>, String>),
    Verify(Result<(), String>),
    Add(AddResult),
    Replace(Result<PathBuf, String>),
    Clear(Result<PathBuf, String>),
    Configure {
        result: Result<(), String>,
        committed: bool,
    },
    Automatic {
        result: Result<(), String>,
        checkpointed: bool,
        delay: Result<u64, String>,
        boot: Result<u64, String>,
    },
}
pub struct Returned {
    pub store: Store,
    pub outcome: Outcome,
    pub storage_error: Option<String>,
}
// This mutex only consumes an already-completed result once across cloned iced
// messages. It is never acquired by the worker or held during any transaction.
#[derive(Clone)]
pub struct Finished {
    pub id: u64,
    value: Arc<Mutex<Option<Result<Returned, String>>>>,
}
impl std::fmt::Debug for Finished {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageJobFinished")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
impl Finished {
    pub fn take(&self) -> Option<Result<Returned, String>> {
        self.value.lock().ok()?.take()
    }
}
#[derive(Default)]
pub struct Gate {
    next: u64,
    pub active: Option<(u64, &'static str)>,
    pub quit: bool,
}
impl Gate {
    pub fn begin(&mut self, job: &Job) -> Option<u64> {
        if self.active.is_some() || self.quit {
            return None;
        }
        self.next = self.next.checked_add(1)?;
        self.active = Some((self.next, job.label()));
        Some(self.next)
    }
    pub fn accept(&mut self, finished: &Finished) -> Option<Result<Returned, String>> {
        if self.active.as_ref().map(|a| a.0) != Some(finished.id) {
            return None;
        }
        let result = finished.take()?;
        self.active = None;
        Some(result)
    }
}
/// Native chooser awaits on the async runtime, not inside a blocking closure.
/// Once a transaction starts it has no cancel handle; Quit is deferred by Gate.
pub async fn run(id: u64, store: Store, job: Job) -> Finished {
    let choice = match &job {
        Job::Export => Some(backup::choose(true).await),
        Job::Import => Some(backup::choose(false).await),
        Job::Folder => Some(backup::choose_folder().await),
        _ => None,
    };
    execute(id, store, job, choice).await
}
pub type Selection = Result<Option<PathBuf>, String>;
pub async fn execute(id: u64, store: Store, job: Job, choice: Option<Selection>) -> Finished {
    let result = tokio::task::spawn_blocking(move || perform(store, job, choice))
        .await
        .map_err(|e| format!("Background storage operation failed: {e}. Retry storage to reload the atomic on-disk state."));
    Finished {
        id,
        value: Arc::new(Mutex::new(Some(result))),
    }
}
fn perform(mut store: Store, job: Job, choice: Option<Selection>) -> Returned {
    let selection = choice.unwrap_or_else(|| Err("No location was selected.".into()));
    let outcome = match job {
        Job::Export => Outcome::Export(selection.and_then(|path| {
            if let Some(path) = path {
                let bytes = store.backup_bytes()?;
                backup::export(&path, &bytes)?;
                Ok(Some(path))
            } else {
                Ok(None)
            }
        })),
        Job::Import => Outcome::Import(selection.and_then(|path| {
            path.map(|path| {
                let prepared = Arc::new(backup::read(&path)?);
                let preview = store.preview_add(&prepared);
                Ok((prepared, preview))
            })
            .transpose()
        })),
        Job::Folder => Outcome::Folder(selection.and_then(|path| {
            if let Some(path) = &path {
                automatic::verify_destination(path)?;
            }
            Ok(path)
        })),
        Job::Verify(path) => Outcome::Verify(automatic::verify_destination(&path)),
        Job::Add(prepared) => Outcome::Add(store.add_backup(&prepared)),
        Job::Replace(prepared) => Outcome::Replace(store.restore_backup(&prepared)),
        Job::Clear => Outcome::Clear(store.clear_all_data()),
        Job::Configure {
            enabled,
            interval,
            retention,
            destination,
        } => {
            let before = store.automatic_preferences().clone();
            let mut committed = false;
            let result = (|| {
                if enabled {
                    automatic::verify_destination(
                        destination
                            .as_deref()
                            .ok_or("Choose a backup destination.")?,
                    )?;
                }
                store.set_automatic_configuration(enabled, interval, retention, destination)?;
                committed = true;
                if enabled {
                    store.apply_retention()?;
                }
                Ok(())
            })();
            committed |= store.automatic_preferences() != &before;
            Outcome::Configure { result, committed }
        }
        Job::Automatic(wall) => {
            let result = store.automatic_backup(wall);
            Outcome::Automatic {
                checkpointed: store.automatic_preferences().last_success == Some(wall),
                result,
                delay: automatic::schedule_seconds(&store.automatic_preferences().interval, wall),
                boot: automatic::boot_seconds(),
            }
        }
        #[cfg(test)]
        Job::Pause(ready, release) => {
            ready.notify_one();
            release.lock().unwrap().recv().unwrap();
            Outcome::Verify(Ok(()))
        }
        #[cfg(test)]
        Job::Panic => panic!("injected background worker failure"),
        #[cfg(test)]
        Job::FailReplace(prepared) => {
            crate::storage::fail_next_commit_sync();
            Outcome::Replace(store.restore_backup(&prepared))
        }
    };
    let storage_error = store.check_storage().err();
    Returned {
        store,
        outcome,
        storage_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepared() -> Arc<backup::Prepared> {
        Arc::new(backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup")).unwrap())
    }
    fn returned(done: Finished) -> Returned {
        done.take().unwrap().unwrap()
    }
    #[tokio::test(flavor = "current_thread")]
    async fn export_import_cancel_corruption_and_existing_paths_preserve_data() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store
            .upsert(crate::tasks::Task::new("Local").unwrap())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let done = returned(execute(1, store, Job::Export, Some(Ok(None))).await);
        assert!(matches!(done.outcome, Outcome::Export(Ok(None))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let destination = root.path().join("manual.kara-backup");
        let done = returned(
            execute(
                2,
                done.store,
                Job::Export,
                Some(Ok(Some(destination.clone()))),
            )
            .await,
        );
        assert!(matches!(done.outcome, Outcome::Export(Ok(Some(_)))));
        assert_eq!(backup::read(&destination).unwrap().task_count(), 1);
        let original = std::fs::read(&destination).unwrap();
        let done = returned(
            execute(
                3,
                done.store,
                Job::Export,
                Some(Ok(Some(destination.clone()))),
            )
            .await,
        );
        assert!(matches!(done.outcome, Outcome::Export(Err(_))));
        assert_eq!(std::fs::read(&destination).unwrap(), original);
        let bad = root.path().join("broken.kara-backup");
        std::fs::write(&bad, b"{broken").unwrap();
        let done = returned(execute(4, done.store, Job::Import, Some(Ok(Some(bad)))).await);
        assert!(matches!(done.outcome, Outcome::Import(Err(_))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let done = returned(execute(5, done.store, Job::Import, Some(Ok(None))).await);
        assert!(matches!(done.outcome, Outcome::Import(Ok(None))));
        let done = returned(execute(6, done.store, Job::Import, Some(Ok(Some(destination)))).await);
        let Outcome::Import(Ok(Some((backup, preview)))) = done.outcome else {
            panic!("Missing preview")
        };
        assert_eq!(preview.unwrap().tasks_skipped, 1);
        let done = returned(execute(7, done.store, Job::Add(backup), None).await);
        assert!(matches!(done.outcome, Outcome::Add(Ok((None, _)))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[tokio::test(flavor = "current_thread")]
    async fn add_replace_and_clear_use_verified_recovery_and_survive_restart() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store
            .upsert(crate::tasks::Task::new("Local").unwrap())
            .unwrap();
        let done = returned(execute(1, store, Job::Add(prepared()), None).await);
        let Outcome::Add(Ok((Some(snapshot), summary))) = done.outcome else {
            panic!("Add failed")
        };
        assert_eq!(summary.tasks_added, 2);
        assert_eq!(done.store.tasks().len(), 3);
        assert_eq!(backup::read(&snapshot).unwrap().task_count(), 1);
        let done = returned(execute(2, done.store, Job::Replace(prepared()), None).await);
        assert!(matches!(done.outcome, Outcome::Replace(Ok(_))));
        assert_eq!(done.store.tasks().len(), 2);
        let done = returned(execute(3, done.store, Job::Clear, None).await);
        let Outcome::Clear(Ok(recovery)) = done.outcome else {
            panic!("Clear failed")
        };
        assert!(done.store.tasks().is_empty());
        assert_eq!(backup::read(&recovery).unwrap().task_count(), 2);
        drop(done.store);
        assert!(Store::open(path.clone()).unwrap().tasks().is_empty());
        let store = Store::open(path.clone()).unwrap();
        let done = returned(
            execute(
                4,
                store,
                Job::Replace(Arc::new(backup::read(&recovery).unwrap())),
                None,
            )
            .await,
        );
        assert_eq!(done.store.tasks().len(), 2);
        drop(done.store);
        assert_eq!(Store::open(path).unwrap().tasks().len(), 2);
    }
    #[tokio::test(flavor = "current_thread")]
    async fn write_failures_rollback_on_the_worker_and_keep_usable_recovery() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store
            .upsert(crate::tasks::Task::new("Original").unwrap())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let done = returned(execute(1, store, Job::FailReplace(prepared()), None).await);
        assert!(matches!(done.outcome,Outcome::Replace(Err(ref e)) if e.contains("Rollback")));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(done.store.tasks()[0].title, "Original");
        let recovery = std::fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.path().extension().is_some_and(|s| s == "kara-backup"))
            .unwrap()
            .path();
        assert_eq!(backup::read(&recovery).unwrap().task_count(), 1);
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let done = returned(execute(2, done.store, Job::Clear, None).await);
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(done.outcome, Outcome::Clear(Err(_))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(done.store);
        assert_eq!(Store::open(path).unwrap().tasks()[0].title, "Original");
    }
    #[tokio::test(flavor = "current_thread")]
    async fn background_panic_releases_lock_without_authorizing_frozen_view_writes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store
            .upsert(crate::tasks::Task::new("Keep").unwrap())
            .unwrap();
        let mut view = store.frozen();
        let before = std::fs::read(&path).unwrap();
        let id = view.tasks()[0].id.clone();
        let mut gate = Gate::default();
        let id_job = gate.begin(&Job::Panic).unwrap();
        let finished = run(id_job, store, Job::Panic).await;
        assert!(gate.accept(&finished).unwrap().is_err());
        assert!(gate.active.is_none());
        assert!(view.toggle(&id).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(Store::open(path).unwrap().tasks().len(), 1);
    }
    #[tokio::test(flavor = "current_thread")]
    async fn automatic_jobs_keep_destination_retention_and_failure_checkpoint_rules() {
        let root = tempfile::tempdir().unwrap();
        let archives = tempfile::tempdir().unwrap();
        let store = Store::open(root.path().join("tasks.json")).unwrap();
        let done = returned(
            run(
                1,
                store,
                Job::Configure {
                    enabled: true,
                    interval: "PT6H".into(),
                    retention: 1,
                    destination: Some(archives.path().into()),
                },
            )
            .await,
        );
        assert!(matches!(
            done.outcome,
            Outcome::Configure {
                result: Ok(()),
                committed: true
            }
        ));
        let manual = archives.path().join("manual.kara-backup");
        backup::export(&manual, &done.store.backup_bytes().unwrap()).unwrap();
        let mut store = done.store;
        for wall in 1000..1003 {
            let done = returned(run(wall as u64, store, Job::Automatic(wall)).await);
            assert!(matches!(
                done.outcome,
                Outcome::Automatic {
                    result: Ok(()),
                    checkpointed: true,
                    ..
                }
            ));
            store = done.store;
        }
        assert_eq!(std::fs::read_dir(archives.path()).unwrap().count(), 3);
        assert!(manual.exists());
        let gone = root.path().join("gone");
        std::fs::rename(archives.path(), &gone).unwrap();
        let done = returned(run(2000, store, Job::Automatic(2000)).await);
        assert!(matches!(
            done.outcome,
            Outcome::Automatic {
                result: Err(_),
                checkpointed: false,
                ..
            }
        ));
        assert_eq!(done.store.automatic_preferences().last_success, Some(1002));
        assert!(!archives.path().exists());
        std::fs::rename(gone, archives.path()).unwrap();
    }
}
