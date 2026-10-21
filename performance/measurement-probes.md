# Stage 3B measurement methods and exact probes

Baseline source is `22d0b30`; audit documentation is `33239ee`. The original Matrix
baseline executable was the audit's unchanged release `engineering_probe`, copied
before any performance source was changed. The data and original probe are also
documented in `kara-audit/audit/measurement-harness.md`.

## Safe reproduction

Use dedicated baseline and feature checkouts; never reset or edit a protected
checkout. Verify the baseline remains 22d0b30. The exact probes below use the Debian
paths from this run; adapt their module paths to your dedicated copies on another
host. Never point baseline modules to an already-promoted main by accident.

Create temporary example files only if their paths do not exist. Build with
`CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<external-cache> cargo build --release --locked --examples`.
The example files are measurement-only and were removed before final checks;
do not treat their baseline source imports as permanent application dependencies.

For each process set TMPDIR and XDG_DATA_HOME/XDG_CONFIG_HOME/XDG_CACHE_HOME/
XDG_STATE_HOME to a fresh disposable root on a filesystem with sufficient space
(the run used /var/tmp). Run sequentially with a 240s timeout and capture JSON lines:

- Original audit engineering_probe: `1000 balanced`, `10000 balanced`, `10000 concentrated`.
- engineering_after: the same three arguments, three layout samples per dataset.
- responsiveness_before / responsiveness_after: `10000`, alternating three times.

All generated Store documents, imports, recovery snapshots and archives are owned
by TemporaryDirectory and removed after each process. Native headless layout uses
locked TinySkia at 1200x800. Three-sample medians are comparative measurements, not
rigorous percentiles. OS load, caches, font warmup and thermals were not controlled.

The responsiveness probes use a current-thread Tokio runtime and a 5ms timer.
The corrected BEFORE probe matches the old controller: file reading/validation
and export publication already use spawn_blocking, but export encoding and Add
preview planning run on the event-loop thread. Add/Replace/Clear/automatic backup
are synchronous. AFTER moves every measured stage to the single-owner worker,
while keeping an O(1) shared UI snapshot. Both use the same 10k records and operation
sequence. This measures backend event-loop availability, NOT a live COSMIC frame,
input dispatch, a native chooser, GPU rendering or the full application startup.

The probes call the worker's selected-path entry to avoid opening portals. The
application itself selects paths using native dialogs and serializes jobs through
Gate. Deterministic application tests separately verify Gate, iced task completion,
read-only snapshots, conflicting messages and deferred Quit.

The diagnostic `matrix_all_row_neighbors` loops over ALL tasks even after the fix.
The view no longer does that: it uses preordered adjacent refs for <=50 rows per
quadrant. Do not mistake that deliberately retained diagnostic for the new frame
construction cost. The first layout can include font/icon warmup; raw samples are
preserved in results.json. Memory includes probe fixtures/imports and allocator
caches and is not a full Kara GUI RSS measurement.

## engineering_after.rs

```rust
#![allow(dead_code)]
// SPDX-License-Identifier: MPL-2.0

#[path = "../src/app.rs"]
mod app;
#[path = "../src/automatic.rs"]
mod automatic;
#[path = "../src/backup.rs"]
mod backup;
#[path = "../src/backup_ui.rs"]
mod backup_ui;
#[path = "../src/calendar_month.rs"]
mod calendar_month;
#[path = "../src/calendar_view.rs"]
mod calendar_view;
#[path = "../src/date_entry.rs"]
mod date_entry;
#[path = "../src/i18n.rs"]
mod i18n;
#[path = "../src/import.rs"]
mod import;
#[path = "../src/lifecycle.rs"]
mod lifecycle;
#[path = "../src/matrix.rs"]
mod matrix;
#[path = "../src/matrix_clip.rs"]
mod matrix_clip;
#[path = "../src/matrix_drag.rs"]
mod matrix_drag;
#[path = "../src/matrix_view.rs"]
mod matrix_view;
#[path = "../src/priority_style.rs"]
mod priority_style;
#[path = "../src/storage.rs"]
mod storage;
#[path = "../src/storage_job.rs"]
mod storage_job;
#[path = "../src/tasks.rs"]
mod tasks;
#[path = "../src/tray.rs"]
mod tray;

#[derive(Debug, Clone)]
pub struct Flags {
    token: Option<String>,
}
impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = &'static str;
    type Args = Vec<String>;
    fn action(&self) -> Option<&Self::SubCommand> {
        Some(&"open")
    }
    // libcosmic consumes the standard platform token before dbus_activation;
    // preserve it in the supported action arguments for a window reopened later.
    fn args(&self) -> Vec<&str> {
        self.token.as_deref().into_iter().collect()
    }
}

use std::{hint::black_box, time::Instant};
fn metric(name: &str, started: Instant) {
    println!(
        "{}",
        serde_json::json!({"metric":name,"ms":started.elapsed().as_secs_f64()*1000.})
    );
}
fn memory(name: &str) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    println!(
        "{}",
        serde_json::json!({"metric":name,"proc_status":s.lines().filter(|s|s.starts_with("VmRSS:")||s.starts_with("VmHWM:")).collect::<Vec<_>>()})
    );
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let n: usize = args[1].parse().unwrap();
    let concentrated = args.get(2).is_some_and(|s| s == "concentrated");
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    let lists: Vec<_> = (0..20)
        .map(|i| tasks::TaskList {
            id: format!("list-{i}"),
            name: format!("List {i}"),
            pinned: i % 3 == 0,
        })
        .collect();
    let mut records: Vec<_> = (0..n)
        .map(|i| tasks::Task {
            id: format!("audit-task-{i:08}"),
            title: format!("Task {:08} — representative audit record", n - i),
            completed: i % 7 == 0,
            due_date: (i % 5 != 0).then(|| format!("2026-03-{:02}", i % 31 + 1)),
            priority: if concentrated {
                tasks::Priority::P3
            } else {
                tasks::Priority::ALL[i % 4]
            },
            notes: Some("Disposable audit notes. ".repeat(8)),
            list_id: if concentrated || i % 5 == 0 {
                None
            } else {
                Some(format!("list-{}", i % 20))
            },
            created_at: Some(1_700_000_000_000_000_000 + i as i64),
            matrix_order: None,
        })
        .collect();
    let start = Instant::now();
    matrix::normalize(&mut records);
    metric("matrix_normalize", start);
    let data = backup::Data {
        version: 7,
        tasks: records,
        lists,
        sort_mode: tasks::TaskSort::CreationDate,
        sort_ascending: false,
        show_completed: true,
        automatic_backups: Some(automatic::Preferences::default()),
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    println!(
        "{}",
        serde_json::json!({"metric":"dataset","tasks":n,"lists":20,"concentrated":concentrated,"live_bytes":std::fs::metadata(&path).unwrap().len()})
    );
    let start = Instant::now();
    let mut store = storage::Store::open(path.clone()).unwrap();
    metric("store_open", start);
    memory("after_open_memory");
    let today = "2026-03-15".parse().unwrap();
    let start = Instant::now();
    black_box(tasks::sorted_visible_tasks(
        store.tasks(),
        tasks::TaskView::Inbox,
        today,
        tasks::TaskSort::Name,
        true,
        true,
    ));
    metric("inbox_name_sort", start);
    let state = calendar_month::State::new(today);
    let start = Instant::now();
    for date in state.cells().into_iter().flatten() {
        black_box(calendar_month::on_date(
            store.tasks(),
            date,
            calendar_month::Filter::All,
        ));
    }
    metric("calendar_all_cell_queries", start);
    let start = Instant::now();
    for task in store.tasks().iter().filter(|t| t.list_id.is_none()) {
        black_box(matrix::neighbor(
            store.tasks(),
            &task.id,
            matrix::Direction::Up,
        ));
        black_box(matrix::neighbor(
            store.tasks(),
            &task.id,
            matrix::Direction::Down,
        ));
    }
    metric("matrix_all_row_neighbors", start);
    let source = store.tasks().iter().find(|t| t.list_id.is_none()).unwrap();
    let placement = matrix::Placement {
        list_id: None,
        quadrant: source.priority.quadrant(),
        before: None,
    };
    let start = Instant::now();
    black_box(matrix::can_move(store.tasks(), &source.id, &placement));
    metric("one_drop_validation", start);
    let id = store.tasks()[0].id.clone();
    for _ in 0..3 {
        let start = Instant::now();
        store.toggle(&id).unwrap();
        metric("atomic_toggle_save", start);
    }
    let start = Instant::now();
    let bytes = store.backup_bytes().unwrap();
    metric("backup_encode", start);
    let start = Instant::now();
    let prepared = backup::decode(&bytes).unwrap();
    metric("backup_decode", start);
    let start = Instant::now();
    backup::export(&root.path().join("manual.kara-backup"), &bytes).unwrap();
    metric("backup_export_publish", start);
    let mut incoming = prepared.data.clone();
    for (i, t) in incoming.tasks.iter_mut().enumerate() {
        if i % 2 == 0 {
            t.id = format!("new-import-{}", t.id);
        }
    }
    let incoming = backup::decode(&backup::encode(incoming).unwrap()).unwrap();
    let start = Instant::now();
    black_box(store.preview_add(&incoming).unwrap());
    metric("add_preview_half_overlap", start);
    let start = Instant::now();
    store.add_backup(&incoming).unwrap();
    metric("add_import_half_overlap", start);
    let start = Instant::now();
    store.restore_backup(&prepared).unwrap();
    metric("replace_import", start);
    let archive = tempfile::tempdir().unwrap();
    store
        .set_automatic_configuration(true, "PT6H".into(), 10, Some(archive.path().into()))
        .unwrap();
    let start = Instant::now();
    store.automatic_backup(1_700_000_000).unwrap();
    metric("automatic_backup_checkpoint", start);
    memory("after_backups_memory");
    use cosmic::iced::{
        Size,
        advanced::{layout::Limits, renderer::Headless, widget::Tree},
    };
    let start = Instant::now();
    let renderer = cosmic::Renderer::new(cosmic::font::default(), 14.into(), Some("tiny-skia"))
        .await
        .unwrap();
    metric("headless_renderer_init", start);
    let drag = matrix_drag::State::default();
    for name in ["calendar_widget_build_layout", "matrix_widget_build_layout"] {
        for _ in 0..3 {
            let start = Instant::now();
            let mut element = if name.starts_with("calendar") {
                calendar_view::view(
                    Some(&store),
                    &state,
                    today,
                    calendar_month::Filter::All,
                    true,
                )
            } else {
                matrix_view::view(Some(&store), None, true, None, &drag)
            };
            let mut tree = Tree::new(&element);
            black_box(element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &Limits::new(Size::ZERO, Size::new(1200., 800.)),
            ));
            metric(name, start);
        }
        memory(name);
    }
    let start = Instant::now();
    store.clear_all_data().unwrap();
    metric("clear_with_recovery", start);
}
```

## responsiveness_before.rs

```rust
#![allow(dead_code)]
// SPDX-License-Identifier: MPL-2.0

#[path = "/home/rapper/projects/kara-main/src/app.rs"]
mod app;
#[path = "/home/rapper/projects/kara-main/src/automatic.rs"]
mod automatic;
#[path = "/home/rapper/projects/kara-main/src/backup.rs"]
mod backup;
#[path = "/home/rapper/projects/kara-main/src/backup_ui.rs"]
mod backup_ui;
#[path = "/home/rapper/projects/kara-main/src/calendar_month.rs"]
mod calendar_month;
#[path = "/home/rapper/projects/kara-main/src/calendar_view.rs"]
mod calendar_view;
#[path = "/home/rapper/projects/kara-main/src/date_entry.rs"]
mod date_entry;
#[path = "/home/rapper/projects/kara-main/src/i18n.rs"]
mod i18n;
#[path = "/home/rapper/projects/kara-main/src/import.rs"]
mod import;
#[path = "/home/rapper/projects/kara-main/src/lifecycle.rs"]
mod lifecycle;
#[path = "/home/rapper/projects/kara-main/src/matrix.rs"]
mod matrix;
#[path = "/home/rapper/projects/kara-main/src/matrix_clip.rs"]
mod matrix_clip;
#[path = "/home/rapper/projects/kara-main/src/matrix_drag.rs"]
mod matrix_drag;
#[path = "/home/rapper/projects/kara-main/src/matrix_view.rs"]
mod matrix_view;
#[path = "/home/rapper/projects/kara-main/src/priority_style.rs"]
mod priority_style;
#[path = "/home/rapper/projects/kara-main/src/storage.rs"]
mod storage;
#[path = "/home/rapper/projects/kara-main/src/tasks.rs"]
mod tasks;
#[path = "/home/rapper/projects/kara-main/src/tray.rs"]
mod tray;

#[derive(Debug, Clone)]
pub struct Flags {
    token: Option<String>,
}
impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = &'static str;
    type Args = Vec<String>;
    fn action(&self) -> Option<&Self::SubCommand> {
        Some(&"open")
    }
    // libcosmic consumes the standard platform token before dbus_activation;
    // preserve it in the supported action arguments for a window reopened later.
    fn args(&self) -> Vec<&str> {
        self.token.as_deref().into_iter().collect()
    }
}

#[path = "/home/rapper/projects/kara-performance/src/storage_job.rs"]
mod storage_job;

const USE_WORKER: bool = false;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
fn sync_operation(store: &mut storage::Store, job: storage_job::Job, path: &std::path::Path) {
    match job {
        storage_job::Job::Export => {
            let bytes = store.backup_bytes().unwrap();
            backup::export(path, &bytes).unwrap();
        }
        storage_job::Job::Import => {
            let p = backup::read(path).unwrap();
            store.preview_add(&p).unwrap();
        }
        storage_job::Job::Add(p) => {
            store.add_backup(&p).unwrap();
        }
        storage_job::Job::Replace(p) => {
            store.restore_backup(&p).unwrap();
        }
        storage_job::Job::Clear => {
            store.clear_all_data().unwrap();
        }
        storage_job::Job::Automatic(wall) => {
            store.automatic_backup(wall).unwrap();
        }
        _ => panic!("Unmeasured operation"),
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    let lists = (0..20)
        .map(|i| tasks::TaskList {
            id: format!("list-{i}"),
            name: format!("List {i}"),
            pinned: i % 3 == 0,
        })
        .collect();
    let mut tasks: Vec<_> = (0..n)
        .map(|i| tasks::Task {
            id: format!("audit-task-{i:08}"),
            title: format!("Task {:08} — representative audit record", n - i),
            completed: i % 7 == 0,
            due_date: (i % 5 != 0).then(|| format!("2026-03-{:02}", i % 31 + 1)),
            priority: tasks::Priority::ALL[i % 4],
            notes: Some("Disposable audit notes. ".repeat(8)),
            list_id: if i % 5 == 0 {
                None
            } else {
                Some(format!("list-{}", i % 20))
            },
            created_at: Some(1_700_000_000_000_000_000 + i as i64),
            matrix_order: None,
        })
        .collect();
    matrix::normalize(&mut tasks);
    let data = backup::Data {
        version: 7,
        tasks,
        lists,
        sort_mode: tasks::TaskSort::CreationDate,
        sort_ascending: false,
        show_completed: true,
        automatic_backups: Some(automatic::Preferences::default()),
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    let original = Arc::new(backup::decode(&backup::encode(data.clone()).unwrap()).unwrap());
    let mut incoming = data;
    for (i, t) in incoming.tasks.iter_mut().enumerate() {
        if i % 2 == 0 {
            t.id = format!("new-import-{}", t.id);
        }
    }
    let incoming = Arc::new(backup::decode(&backup::encode(incoming).unwrap()).unwrap());
    let mut store = storage::Store::open(path).unwrap();
    let export = root.path().join("manual.kara-backup");
    for (index, (label, job)) in [
        ("export", storage_job::Job::Export),
        ("import_preview", storage_job::Job::Import),
        ("add", storage_job::Job::Add(incoming)),
        ("replace", storage_job::Job::Replace(original)),
        ("automatic", storage_job::Job::Automatic(1_700_000_000)),
        ("clear", storage_job::Job::Clear),
    ]
    .into_iter()
    .enumerate()
    {
        let done = Arc::new(AtomicBool::new(false));
        let gaps = Arc::new(Mutex::new(Vec::new()));
        let stop = done.clone();
        let samples = gaps.clone();
        let ticker = tokio::spawn(async move {
            let mut previous = Instant::now();
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            while !stop.load(Ordering::Relaxed) {
                interval.tick().await;
                let now = Instant::now();
                samples
                    .lock()
                    .unwrap()
                    .push(now.duration_since(previous).as_secs_f64() * 1000.);
                previous = now;
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(12)).await;
        let start = Instant::now();
        let view: Option<()> = None;
        let snapshot_ms = start.elapsed().as_secs_f64() * 1000.;
        if USE_WORKER {
            let selection = matches!(job, storage_job::Job::Export | storage_job::Job::Import)
                .then(|| Ok(Some(export.clone())));
            let returned = storage_job::execute(index as u64, store, job, selection)
                .await
                .take()
                .unwrap()
                .unwrap();
            assert!(returned.storage_error.is_none());
            match returned.outcome {
                storage_job::Outcome::Export(Ok(_))
                | storage_job::Outcome::Import(Ok(_))
                | storage_job::Outcome::Add(Ok(_))
                | storage_job::Outcome::Replace(Ok(_))
                | storage_job::Outcome::Clear(Ok(_))
                | storage_job::Outcome::Automatic { result: Ok(()), .. } => (),
                other => panic!("{other:?}"),
            }
            store = returned.store;
        } else {
            match job {
                storage_job::Job::Export => {
                    let bytes = store.backup_bytes().unwrap();
                    let export = export.clone();
                    tokio::task::spawn_blocking(move || backup::export(&export, &bytes))
                        .await
                        .unwrap()
                        .unwrap();
                }
                storage_job::Job::Import => {
                    let export = export.clone();
                    let prepared = tokio::task::spawn_blocking(move || backup::read(&export))
                        .await
                        .unwrap()
                        .unwrap();
                    store.preview_add(&prepared).unwrap();
                }
                other => sync_operation(&mut store, other, &export),
            }
        }
        let operation_ms = start.elapsed().as_secs_f64() * 1000.;
        let _ = view;
        tokio::time::sleep(std::time::Duration::from_millis(12)).await;
        done.store(true, Ordering::Relaxed);
        ticker.await.unwrap();
        let mut samples = gaps.lock().unwrap().clone();
        samples.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"worker":USE_WORKER,"tasks":n,"operation":label,"operation_ms":operation_ms,"snapshot_ms":snapshot_ms,"max_timer_gap_ms":samples.last(),"p95_timer_gap_ms":samples[samples.len()*95/100],"timer_ticks":samples.len()})
        );
    }
}
```

## responsiveness_after.rs

```rust
#![allow(dead_code)]
// SPDX-License-Identifier: MPL-2.0

#[path = "/home/rapper/projects/kara-performance/src/app.rs"]
mod app;
#[path = "/home/rapper/projects/kara-performance/src/automatic.rs"]
mod automatic;
#[path = "/home/rapper/projects/kara-performance/src/backup.rs"]
mod backup;
#[path = "/home/rapper/projects/kara-performance/src/backup_ui.rs"]
mod backup_ui;
#[path = "/home/rapper/projects/kara-performance/src/calendar_month.rs"]
mod calendar_month;
#[path = "/home/rapper/projects/kara-performance/src/calendar_view.rs"]
mod calendar_view;
#[path = "/home/rapper/projects/kara-performance/src/date_entry.rs"]
mod date_entry;
#[path = "/home/rapper/projects/kara-performance/src/i18n.rs"]
mod i18n;
#[path = "/home/rapper/projects/kara-performance/src/import.rs"]
mod import;
#[path = "/home/rapper/projects/kara-performance/src/lifecycle.rs"]
mod lifecycle;
#[path = "/home/rapper/projects/kara-performance/src/matrix.rs"]
mod matrix;
#[path = "/home/rapper/projects/kara-performance/src/matrix_clip.rs"]
mod matrix_clip;
#[path = "/home/rapper/projects/kara-performance/src/matrix_drag.rs"]
mod matrix_drag;
#[path = "/home/rapper/projects/kara-performance/src/matrix_view.rs"]
mod matrix_view;
#[path = "/home/rapper/projects/kara-performance/src/priority_style.rs"]
mod priority_style;
#[path = "/home/rapper/projects/kara-performance/src/storage.rs"]
mod storage;
#[path = "/home/rapper/projects/kara-performance/src/storage_job.rs"]
mod storage_job;
#[path = "/home/rapper/projects/kara-performance/src/tasks.rs"]
mod tasks;
#[path = "/home/rapper/projects/kara-performance/src/tray.rs"]
mod tray;

#[derive(Debug, Clone)]
pub struct Flags {
    token: Option<String>,
}
impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = &'static str;
    type Args = Vec<String>;
    fn action(&self) -> Option<&Self::SubCommand> {
        Some(&"open")
    }
    // libcosmic consumes the standard platform token before dbus_activation;
    // preserve it in the supported action arguments for a window reopened later.
    fn args(&self) -> Vec<&str> {
        self.token.as_deref().into_iter().collect()
    }
}

const USE_WORKER: bool = true;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
fn sync_operation(store: &mut storage::Store, job: storage_job::Job, path: &std::path::Path) {
    match job {
        storage_job::Job::Export => {
            let bytes = store.backup_bytes().unwrap();
            backup::export(path, &bytes).unwrap();
        }
        storage_job::Job::Import => {
            let p = backup::read(path).unwrap();
            store.preview_add(&p).unwrap();
        }
        storage_job::Job::Add(p) => {
            store.add_backup(&p).unwrap();
        }
        storage_job::Job::Replace(p) => {
            store.restore_backup(&p).unwrap();
        }
        storage_job::Job::Clear => {
            store.clear_all_data().unwrap();
        }
        storage_job::Job::Automatic(wall) => {
            store.automatic_backup(wall).unwrap();
        }
        _ => panic!("Unmeasured operation"),
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    let lists = (0..20)
        .map(|i| tasks::TaskList {
            id: format!("list-{i}"),
            name: format!("List {i}"),
            pinned: i % 3 == 0,
        })
        .collect();
    let mut tasks: Vec<_> = (0..n)
        .map(|i| tasks::Task {
            id: format!("audit-task-{i:08}"),
            title: format!("Task {:08} — representative audit record", n - i),
            completed: i % 7 == 0,
            due_date: (i % 5 != 0).then(|| format!("2026-03-{:02}", i % 31 + 1)),
            priority: tasks::Priority::ALL[i % 4],
            notes: Some("Disposable audit notes. ".repeat(8)),
            list_id: if i % 5 == 0 {
                None
            } else {
                Some(format!("list-{}", i % 20))
            },
            created_at: Some(1_700_000_000_000_000_000 + i as i64),
            matrix_order: None,
        })
        .collect();
    matrix::normalize(&mut tasks);
    let data = backup::Data {
        version: 7,
        tasks,
        lists,
        sort_mode: tasks::TaskSort::CreationDate,
        sort_ascending: false,
        show_completed: true,
        automatic_backups: Some(automatic::Preferences::default()),
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    let original = Arc::new(backup::decode(&backup::encode(data.clone()).unwrap()).unwrap());
    let mut incoming = data;
    for (i, t) in incoming.tasks.iter_mut().enumerate() {
        if i % 2 == 0 {
            t.id = format!("new-import-{}", t.id);
        }
    }
    let incoming = Arc::new(backup::decode(&backup::encode(incoming).unwrap()).unwrap());
    let mut store = storage::Store::open(path).unwrap();
    let export = root.path().join("manual.kara-backup");
    for (index, (label, job)) in [
        ("export", storage_job::Job::Export),
        ("import_preview", storage_job::Job::Import),
        ("add", storage_job::Job::Add(incoming)),
        ("replace", storage_job::Job::Replace(original)),
        ("automatic", storage_job::Job::Automatic(1_700_000_000)),
        ("clear", storage_job::Job::Clear),
    ]
    .into_iter()
    .enumerate()
    {
        let done = Arc::new(AtomicBool::new(false));
        let gaps = Arc::new(Mutex::new(Vec::new()));
        let stop = done.clone();
        let samples = gaps.clone();
        let ticker = tokio::spawn(async move {
            let mut previous = Instant::now();
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            while !stop.load(Ordering::Relaxed) {
                interval.tick().await;
                let now = Instant::now();
                samples
                    .lock()
                    .unwrap()
                    .push(now.duration_since(previous).as_secs_f64() * 1000.);
                previous = now;
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(12)).await;
        let start = Instant::now();
        let view = Some(store.frozen());
        let snapshot_ms = start.elapsed().as_secs_f64() * 1000.;
        if USE_WORKER {
            let selection = matches!(job, storage_job::Job::Export | storage_job::Job::Import)
                .then(|| Ok(Some(export.clone())));
            let returned = storage_job::execute(index as u64, store, job, selection)
                .await
                .take()
                .unwrap()
                .unwrap();
            assert!(returned.storage_error.is_none());
            match returned.outcome {
                storage_job::Outcome::Export(Ok(_))
                | storage_job::Outcome::Import(Ok(_))
                | storage_job::Outcome::Add(Ok(_))
                | storage_job::Outcome::Replace(Ok(_))
                | storage_job::Outcome::Clear(Ok(_))
                | storage_job::Outcome::Automatic { result: Ok(()), .. } => (),
                other => panic!("{other:?}"),
            }
            store = returned.store;
        } else {
            sync_operation(&mut store, job, &export);
        }
        let operation_ms = start.elapsed().as_secs_f64() * 1000.;
        drop(view);
        tokio::time::sleep(std::time::Duration::from_millis(12)).await;
        done.store(true, Ordering::Relaxed);
        ticker.await.unwrap();
        let mut samples = gaps.lock().unwrap().clone();
        samples.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"worker":USE_WORKER,"tasks":n,"operation":label,"operation_ms":operation_ms,"snapshot_ms":snapshot_ms,"max_timer_gap_ms":samples.last(),"p95_timer_gap_ms":samples[samples.len()*95/100],"timer_ticks":samples.len()})
        );
    }
}
```

## Disposable graphical fixture generator

Fully Quit Kara first. Create a NEW empty directory with mktemp; pass that path and
`balanced` or `concentrated` to this script. Set XDG_DATA_HOME to that exact path
when launching the transferred feature build. Never pass a production data root.
This only seeds a disposable live schema-7 document; it is not a manual backup.
The application validates and normalizes its Matrix ranks on load.

```python
import json, pathlib, sys
root = pathlib.Path(sys.argv[1]).resolve()
mode = sys.argv[2]
assert root.is_dir() and not any(root.iterdir()), "Use a NEW empty disposable root"
assert mode in ("balanced", "concentrated")
directory = root / "io.github.kordeone.Kara"
directory.mkdir(mode=0o700)
tasks = []
for i in range(10000):
    tasks.append({"id": f"perf-{i:08}", "title": f"Task {10000-i:08} — disposable fixture",
        "completed": i % 7 == 0, "due_date": None if i % 5 == 0 else f"2026-03-{i%31+1:02}",
        "priority": 3 if mode == "concentrated" else i % 4,
        "notes": "Disposable audit notes. " * 8,
        "list_id": None if mode == "concentrated" or i % 5 == 0 else f"list-{i%20}",
        "created_at": 1700000000000000000+i, "matrix_order": i})
data = {"version": 7, "tasks": tasks,
    "lists": [{"id": f"list-{i}", "name": f"List {i}", "pinned": i%3 == 0} for i in range(20)],
    "sort_mode": "CreationDate", "sort_ascending": False, "show_completed": True,
    "automatic_backups": {"enabled": False, "interval": "PT6H", "last_success": None}}
path = directory / "tasks.json"
with path.open("x") as file:
    json.dump(data, file, ensure_ascii=False, indent=2)
path.chmod(0o600)
print(path)
```
