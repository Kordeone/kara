// SPDX-License-Identifier: MPL-2.0
//! Matrix ordering changes fields on shared records, never their document order.
use crate::tasks::{Quadrant, Task};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// Canonical zero-based ranks in each (list, quadrant), active tasks first.
/// Missing/duplicate/gapped ranks resolve stably using original document order.
pub fn normalize(tasks: &mut [Task]) {
    let mut groups: HashMap<(Option<String>, Quadrant), Vec<usize>> = HashMap::new();
    for (index, task) in tasks.iter().enumerate() {
        if let Some(quadrant) = task.matrix_quadrant {
            groups
                .entry((task.list_id.clone(), quadrant))
                .or_default()
                .push(index);
        }
    }
    for indices in groups.values_mut() {
        indices.sort_by_key(|&index| {
            (
                tasks[index].completed,
                tasks[index].matrix_order.unwrap_or(u64::MAX),
                index,
            )
        });
        for (rank, &index) in indices.iter().enumerate() {
            tasks[index].matrix_order = Some(rank as u64);
        }
    }
}

pub fn ordered<'a>(
    tasks: &'a [Task],
    list: Option<&str>,
    quadrant: Option<Quadrant>,
) -> Vec<&'a Task> {
    let mut visible: Vec<_> = tasks
        .iter()
        .filter(|task| task.list_id.as_deref() == list && task.matrix_quadrant == quadrant)
        .collect();
    visible.sort_by_key(|task| (task.completed, task.matrix_order.unwrap_or(u64::MAX)));
    visible
}

pub fn draft(list: Option<&str>, quadrant: Option<Quadrant>) -> Result<Task, String> {
    let mut task = Task::new("New task")?;
    task.title.clear();
    task.list_id = list.map(str::to_string);
    task.matrix_quadrant = quadrant;
    Ok(task)
}

pub fn classify(tasks: &mut [Task], id: &str, quadrant: Option<Quadrant>) -> Result<(), String> {
    let task = tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or("Task no longer exists.")?;
    if task.matrix_quadrant != quadrant {
        task.matrix_quadrant = quadrant;
        task.matrix_order = None; // append within its completion group at destination
    }
    normalize(tasks);
    Ok(())
}

pub fn neighbor<'a>(tasks: &'a [Task], id: &str, direction: Direction) -> Option<&'a str> {
    let task = tasks.iter().find(|task| task.id == id)?;
    task.matrix_quadrant?;
    let ordered = ordered(tasks, task.list_id.as_deref(), task.matrix_quadrant);
    let position = ordered.iter().position(|task| task.id == id)?;
    let index = match direction {
        Direction::Up => position.checked_sub(1)?,
        Direction::Down => position.checked_add(1)?,
    };
    ordered
        .get(index)
        .filter(|other| other.completed == task.completed)
        .map(|task| task.id.as_str())
}

pub fn reorder(tasks: &mut [Task], id: &str, direction: Direction) -> Result<(), String> {
    if !tasks.iter().any(|task| task.id == id) {
        return Err("Task no longer exists.".into());
    }
    normalize(tasks);
    if let Some(other) = neighbor(tasks, id, direction).map(str::to_string) {
        let first = tasks.iter().position(|task| task.id == id).unwrap();
        let second = tasks.iter().position(|task| task.id == other).unwrap();
        let old = tasks[first].matrix_order;
        tasks[first].matrix_order = tasks[second].matrix_order;
        tasks[second].matrix_order = old;
    }
    Ok(())
}

/// List deletion is a batch move: append each former quadrant in its prior
/// relative order, after the existing Inbox tasks of the same completion state.
pub fn move_list_to_inbox(tasks: &mut [Task], list_id: &str) {
    normalize(tasks);
    for quadrant in Quadrant::ALL {
        let moving: Vec<_> = ordered(tasks, Some(list_id), Some(quadrant))
            .into_iter()
            .map(|task| task.id.clone())
            .collect();
        let start = tasks
            .iter()
            .filter(|task| task.list_id.is_none() && task.matrix_quadrant == Some(quadrant))
            .count() as u64;
        for (offset, id) in moving.iter().enumerate() {
            let task = tasks.iter_mut().find(|task| &task.id == id).unwrap();
            task.list_id = None;
            task.matrix_order = Some(start + offset as u64);
        }
    }
    for task in tasks
        .iter_mut()
        .filter(|task| task.list_id.as_deref() == Some(list_id))
    {
        task.list_id = None;
    }
    normalize(tasks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{Priority, TaskView, parse_date, visible_tasks};
    const A: Quadrant = Quadrant::ImportantUrgent;
    const B: Quadrant = Quadrant::ImportantNotUrgent;
    fn task(
        id: &str,
        list: Option<&str>,
        quadrant: Option<Quadrant>,
        order: Option<u64>,
        completed: bool,
    ) -> Task {
        let mut task = Task::new(id).unwrap();
        task.id = id.into();
        task.list_id = list.map(str::to_string);
        task.matrix_quadrant = quadrant;
        task.matrix_order = order;
        task.completed = completed;
        task
    }
    fn ids(tasks: &[Task], list: Option<&str>, quadrant: Option<Quadrant>) -> Vec<String> {
        ordered(tasks, list, quadrant)
            .into_iter()
            .map(|task| task.id.clone())
            .collect()
    }
    #[test]
    fn classification_is_explicit_and_drafts_default_to_the_requested_context() {
        let mut urgent = Task::new("Overdue high priority").unwrap();
        urgent.priority = Priority::High;
        urgent.due_date = Some("2000-01-01".into());
        assert_eq!(urgent.matrix_quadrant, None);
        assert_eq!(urgent.matrix_order, None);
        let id = urgent.id.clone();
        let mut tasks = vec![urgent];
        classify(&mut tasks, &id, Some(Quadrant::NotImportantNotUrgent)).unwrap();
        assert_eq!(tasks[0].priority, Priority::High);
        assert_eq!(tasks[0].due_date.as_deref(), Some("2000-01-01"));
        assert_eq!(
            tasks[0].matrix_quadrant,
            Some(Quadrant::NotImportantNotUrgent)
        );
        for quadrant in Quadrant::ALL {
            let draft = draft(Some("work"), Some(quadrant)).unwrap();
            assert_eq!(draft.matrix_quadrant, Some(quadrant));
            assert_eq!(draft.list_id.as_deref(), Some("work"));
            assert!(!draft.completed);
            assert_eq!(draft.matrix_order, None);
            assert!(draft.title.is_empty());
        }
        let unassigned = draft(None, None).unwrap();
        assert!(unassigned.list_id.is_none());
        assert!(unassigned.matrix_quadrant.is_none());
    }
    #[test]
    fn normalization_is_scoped_stable_idempotent_and_never_reorders_records() {
        let mut tasks = vec![
            task("a", None, Some(A), Some(7), false),
            task("b", None, Some(A), Some(7), false),
            task("other", None, Some(B), Some(u64::MAX), false),
            task("list", Some("work"), Some(A), None, false),
            task("unassigned", None, None, None, false),
            task("missing", None, Some(A), None, false),
        ];
        let original_ids: Vec<_> = tasks.iter().map(|task| task.id.clone()).collect();
        normalize(&mut tasks);
        assert_eq!(ids(&tasks, None, Some(A)), ["a", "b", "missing"]);
        assert_eq!(
            tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>(),
            original_ids
        );
        for (id, rank) in [
            ("a", Some(0)),
            ("b", Some(1)),
            ("other", Some(0)),
            ("list", Some(0)),
            ("unassigned", None),
            ("missing", Some(2)),
        ] {
            assert_eq!(
                tasks
                    .iter()
                    .find(|task| task.id == id)
                    .unwrap()
                    .matrix_order,
                rank
            );
        }
        let normalized = tasks.clone();
        normalize(&mut tasks);
        assert_eq!(tasks, normalized);
    }
    #[test]
    fn quadrant_changes_append_and_unassigning_clears_the_rank() {
        let mut tasks = vec![
            task("moving", None, Some(A), Some(0), false),
            task("source", None, Some(A), Some(1), false),
            task("destination", None, Some(B), Some(0), false),
            task("done", None, Some(B), Some(1), true),
        ];
        classify(&mut tasks, "moving", Some(B)).unwrap();
        assert_eq!(ids(&tasks, None, Some(A)), ["source"]);
        assert_eq!(
            ids(&tasks, None, Some(B)),
            ["destination", "moving", "done"]
        );
        assert_eq!(tasks[1].matrix_order, Some(0));
        let before = tasks.clone();
        classify(&mut tasks, "moving", Some(B)).unwrap();
        assert_eq!(tasks, before);
        classify(&mut tasks, "moving", None).unwrap();
        assert_eq!(tasks[0].matrix_quadrant, None);
        assert_eq!(tasks[0].matrix_order, None);
        assert_eq!(ids(&tasks, None, Some(B)), ["destination", "done"]);
        assert!(classify(&mut tasks, "missing", Some(A)).is_err());
    }
    #[test]
    fn reordering_respects_list_quadrant_and_completed_boundaries() {
        let mut tasks = vec![
            task("first", None, Some(A), Some(0), false),
            task("second", None, Some(A), Some(1), false),
            task("third", None, Some(A), Some(2), false),
            task("done1", None, Some(A), Some(3), true),
            task("done2", None, Some(A), Some(4), true),
            task("other", Some("work"), Some(A), Some(0), false),
            task("other-quadrant", None, Some(B), Some(0), false),
        ];
        assert!(neighbor(&tasks, "first", Direction::Up).is_none());
        assert!(neighbor(&tasks, "third", Direction::Down).is_none());
        assert!(neighbor(&tasks, "done1", Direction::Up).is_none());
        reorder(&mut tasks, "second", Direction::Up).unwrap();
        reorder(&mut tasks, "done1", Direction::Down).unwrap();
        assert_eq!(
            ids(&tasks, None, Some(A)),
            ["second", "first", "third", "done2", "done1"]
        );
        assert_eq!(tasks[5].matrix_order, Some(0));
        assert_eq!(tasks[6].matrix_order, Some(0));
        let before = tasks.clone();
        reorder(&mut tasks, "third", Direction::Down).unwrap();
        assert_eq!(tasks, before);
        assert!(reorder(&mut tasks, "missing", Direction::Up).is_err());
    }
    #[test]
    fn completion_retains_context_and_can_be_reversed_without_duplicate_tasks() {
        let mut tasks = vec![
            task("first", Some("work"), Some(A), Some(0), false),
            task("second", Some("work"), Some(A), Some(1), false),
        ];
        tasks[0].due_date = Some("2026-10-18".into());
        tasks[0].completed = true;
        normalize(&mut tasks);
        assert_eq!(ids(&tasks, Some("work"), Some(A)), ["second", "first"]);
        assert_eq!(tasks[0].list_id.as_deref(), Some("work"));
        assert_eq!(tasks[0].matrix_quadrant, Some(A));
        let today = parse_date("2026-10-18").unwrap();
        assert_eq!(visible_tasks(&tasks, TaskView::Today, today)[0].id, "first");
        assert_eq!(
            visible_tasks(&tasks, TaskView::List("work"), today).len(),
            2
        );
        tasks[0].completed = false;
        normalize(&mut tasks);
        assert_eq!(ids(&tasks, Some("work"), Some(A)), ["second", "first"]);
        assert_eq!(tasks.len(), 2);
        assert!(ordered(&tasks, None, Some(A)).is_empty());
    }
}
