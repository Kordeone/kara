// SPDX-License-Identifier: MPL-2.0
//! Conservative, deterministic Add planning over the existing shared records.
use crate::{
    backup::{Data, Prepared},
    matrix, tasks,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub tasks_added: usize,
    pub tasks_skipped: usize,
    pub lists_added: usize,
    pub lists_skipped: usize,
    pub conflicts: Vec<String>,
}
impl Summary {
    pub fn description(&self) -> String {
        let mut text = format!(
            "Tasks: {} added, {} skipped. Lists: {} added, {} skipped. Existing records and preferences win.",
            self.tasks_added, self.tasks_skipped, self.lists_added, self.lists_skipped
        );
        for conflict in self.conflicts.iter().take(8) {
            text.push('\n');
            text.push_str(conflict);
        }
        if self.conflicts.len() > 8 {
            text.push_str(&format!(
                "\n{} further ID matches keep existing values.",
                self.conflicts.len() - 8
            ));
        }
        text
    }
    pub fn has_changes(&self) -> bool {
        self.tasks_added != 0 || self.lists_added != 0
    }
}
pub(crate) fn plan(mut current: Data, incoming: &Prepared) -> Result<(Data, Summary), String> {
    tasks::validate_document(&incoming.data.tasks, &incoming.data.lists)?;
    let mut summary = Summary::default();
    for list in &incoming.data.lists {
        if let Some(existing) = current.lists.iter().find(|l| l.id == list.id) {
            if existing.name.trim().to_lowercase() != list.name.trim().to_lowercase() {
                return Err(format!(
                    "Add blocked: list ID {} means ‘{}’ here and ‘{}’ in the backup. No tasks will be reassigned. Use Replace or resolve the list conflict first.",
                    list.id, existing.name, list.name
                ));
            }
            summary.lists_skipped += 1;
            if existing != list {
                summary
                    .conflicts
                    .push(format!("List {}: existing name/pin retained.", list.id));
            }
        } else {
            tasks::validate_list_name(&list.name, &current.lists, None).map_err(|_| format!("Add blocked: list ‘{}’ has a different ID from an existing list. Rename one list or use Replace; no records changed.", list.name))?;
            current.lists.push(list.clone());
            summary.lists_added += 1;
        }
    }
    let old_ids: HashSet<_> = current.tasks.iter().map(|t| t.id.clone()).collect();
    let existing: HashMap<_, _> = current.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut new_tasks = Vec::new();
    for task in &incoming.data.tasks {
        if let Some(old) = existing.get(task.id.as_str()) {
            summary.tasks_skipped += 1;
            if *old != task {
                summary.conflicts.push(format!(
                    "Task {} (‘{}’): existing record retained.",
                    task.id, old.title
                ));
            }
        } else {
            new_tasks.push(task.clone());
            summary.tasks_added += 1;
        }
    }
    // Append incoming ranks within each (list, priority, completion group),
    // retaining existing relative order and imported peer order. Never sort the
    // document vector or change an existing record's non-ordering properties.
    let mut counts: HashMap<_, u64> = HashMap::new();
    for task in &current.tasks {
        *counts
            .entry((task.list_id.clone(), task.priority.quadrant()))
            .or_default() += 1;
    }
    let mut groups: HashMap<_, Vec<usize>> = HashMap::new();
    for (i, task) in new_tasks.iter().enumerate() {
        groups
            .entry((task.list_id.clone(), task.priority.quadrant()))
            .or_default()
            .push(i);
    }
    for (key, indices) in &mut groups {
        indices.sort_by_key(|&i| {
            (
                new_tasks[i].completed,
                new_tasks[i].matrix_order.is_none(),
                new_tasks[i].matrix_order.unwrap_or(0),
                i,
            )
        });
        let base = counts.get(key).copied().unwrap_or(0);
        for (rank, &i) in indices.iter().enumerate() {
            new_tasks[i].matrix_order = Some(base + rank as u64);
        }
    }
    current.tasks.extend(new_tasks);
    tasks::validate_document(&current.tasks, &current.lists)?;
    matrix::normalize(&mut current.tasks);
    debug_assert_eq!(old_ids.len() + summary.tasks_added, current.tasks.len());
    Ok((current, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{Priority, Quadrant, Task, TaskList, TaskSort};
    fn incoming() -> Prepared {
        crate::backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup")).unwrap()
    }
    #[test]
    fn nonoverlapping_records_append_and_existing_preferences_are_kept() {
        let source = incoming();
        let mut current = source.data.clone();
        current.tasks.clear();
        current.lists.clear();
        current.sort_mode = TaskSort::Priority;
        current.sort_ascending = true;
        let mut local = Task::new("Keep me").unwrap();
        local.matrix_order = Some(0);
        current.tasks.push(local.clone());
        let (merged, summary) = plan(current.clone(), &source).unwrap();
        assert_eq!(summary.tasks_added, 2);
        assert_eq!(summary.lists_added, 1);
        assert!(summary.conflicts.is_empty());
        assert_eq!(merged.tasks[0], local);
        assert_eq!(merged.sort_mode, current.sort_mode);
        assert_eq!(merged.sort_ascending, current.sort_ascending);
        assert_eq!(merged.automatic_backups, current.automatic_backups);
    }
    #[test]
    fn overlapping_ids_keep_existing_records_and_report_differences() {
        let source = incoming();
        let mut current = source.data.clone();
        current.tasks[0].title = "Existing edit".into();
        current.lists[0].pinned = false;
        let (merged, summary) = plan(current.clone(), &source).unwrap();
        assert_eq!(merged, current);
        assert_eq!(summary.tasks_skipped, 2);
        assert_eq!(summary.lists_skipped, 1);
        assert!(!summary.has_changes());
        assert_eq!(summary.conflicts.len(), 2);
    }
    #[test]
    fn ambiguous_list_ids_names_and_dangling_references_block_the_entire_plan() {
        let source = incoming();
        let mut current = source.data.clone();
        current.lists[0].name = "Different meaning".into();
        assert!(plan(current, &source).unwrap_err().contains("list ID"));
        let mut current = source.data.clone();
        current.tasks.clear();
        current.lists[0].id = "different-id".into();
        assert!(plan(current, &source).unwrap_err().contains("different ID"));
        let mut bad = incoming();
        bad.data.lists.clear();
        assert!(plan(source.data.clone(), &bad).is_err());
    }
    #[test]
    fn imported_matrix_peer_order_appends_without_reordering_existing_records() {
        let mut source = incoming();
        source.data.tasks.clear();
        source.data.lists.clear();
        let mut current = source.data.clone();
        fn task(title: &str, rank: u64, completed: bool) -> Task {
            let mut t = Task::new(title).unwrap();
            t.priority = Priority::P3;
            t.matrix_order = Some(rank);
            t.completed = completed;
            t
        }
        let local = task("Local", 0, false);
        let local_done = task("Local done", 1, true);
        current.tasks = vec![local.clone(), local_done.clone()];
        source.data.tasks = vec![
            task("Second", 1, false),
            task("First", 0, false),
            task("Incoming done", 2, true),
        ];
        let (merged, summary) = plan(current, &source).unwrap();
        assert_eq!(summary.tasks_added, 3);
        assert_eq!(merged.tasks[0], local);
        assert_eq!(merged.tasks[1].id, local_done.id);
        assert_eq!(
            matrix::ordered(&merged.tasks, None, Quadrant::ImportantUrgent)
                .iter()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            ["Local", "First", "Second", "Local done", "Incoming done"]
        );
        assert_eq!(merged.tasks[2].title, "Second");
        let mut local_done = local_done;
        local_done.matrix_order = merged.tasks[1].matrix_order;
        assert_eq!(merged.tasks[1], local_done);
    }
    #[test]
    fn incoming_ids_do_not_redirect_an_existing_tasks_list_assignment() {
        let source = incoming();
        let mut current = source.data.clone();
        current.tasks[0].list_id = None;
        let extra = TaskList::new("Other").unwrap();
        current.lists.push(extra);
        matrix::normalize(&mut current.tasks);
        let (merged, summary) = plan(current.clone(), &source).unwrap();
        assert_eq!(merged.tasks, current.tasks);
        assert!(merged.tasks[0].list_id.is_none());
        assert_eq!(summary.tasks_skipped, 2);
    }
}
