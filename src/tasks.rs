// SPDX-License-Identifier: MPL-2.0
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskSort {
    #[default]
    CreationDate,
    DueDate,
    Priority,
    Name,
}
impl TaskSort {
    pub const ALL: [Self; 4] = [
        Self::CreationDate,
        Self::DueDate,
        Self::Priority,
        Self::Name,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::CreationDate => "Creation date",
            Self::DueDate => "Due date",
            Self::Priority => "Priority",
            Self::Name => "Name",
        }
    }
    pub fn default_ascending(self) -> bool {
        matches!(self, Self::DueDate | Self::Name)
    }
}
pub fn sort_mode_at(index: usize) -> Option<TaskSort> {
    TaskSort::ALL.get(index).copied()
}
pub fn sort_order_label(ascending: bool) -> &'static str {
    if ascending { "Ascending" } else { "Descending" }
}
fn in_requested_direction(ordering: std::cmp::Ordering, ascending: bool) -> std::cmp::Ordering {
    if ascending {
        ordering
    } else {
        ordering.reverse()
    }
}
/// The sole persisted classification. Numeric values are part of schema v4.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
pub enum Priority {
    #[default]
    P0,
    P1,
    P2,
    P3,
}
impl Priority {
    pub const ALL: [Self; 4] = [Self::P0, Self::P1, Self::P2, Self::P3];
    pub fn label(self) -> &'static str {
        self.quadrant().label()
    }
    pub fn short_label(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::P3 => "P3",
        }
    }
    pub fn quadrant(self) -> Quadrant {
        match self {
            Self::P0 => Quadrant::NotImportantNotUrgent,
            Self::P1 => Quadrant::NotImportantUrgent,
            Self::P2 => Quadrant::ImportantNotUrgent,
            Self::P3 => Quadrant::ImportantUrgent,
        }
    }
}
impl From<Priority> for u8 {
    fn from(value: Priority) -> Self {
        match value {
            Priority::P0 => 0,
            Priority::P1 => 1,
            Priority::P2 => 2,
            Priority::P3 => 3,
        }
    }
}
impl TryFrom<u8> for Priority {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::ALL
            .get(usize::from(value))
            .copied()
            .ok_or("Priority must be an integer from 0 to 3.")
    }
}

// Keep both axes explicit in each variant, including the repeated urgency suffix.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quadrant {
    ImportantUrgent,
    ImportantNotUrgent,
    NotImportantUrgent,
    NotImportantNotUrgent,
}
impl Quadrant {
    pub fn priority(self) -> Priority {
        match self {
            Self::NotImportantNotUrgent => Priority::P0,
            Self::NotImportantUrgent => Priority::P1,
            Self::ImportantNotUrgent => Priority::P2,
            Self::ImportantUrgent => Priority::P3,
        }
    }
    pub const ALL: [Self; 4] = [
        Self::ImportantUrgent,
        Self::ImportantNotUrgent,
        Self::NotImportantUrgent,
        Self::NotImportantNotUrgent,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ImportantUrgent => "Important & Urgent",
            Self::ImportantNotUrgent => "Important & Not Urgent",
            Self::NotImportantUrgent => "Not Important & Urgent",
            Self::NotImportantNotUrgent => "Not Important & Not Urgent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub completed: bool,
    pub due_date: Option<String>,
    pub priority: Priority,
    pub notes: Option<String>,
    pub list_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_order: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub enum TaskView<'a> {
    Inbox,
    Today,
    List(&'a str),
}

impl Task {
    pub fn new(title: &str) -> Result<Self, String> {
        let task = Self {
            id: new_id()?,
            title: title.trim().into(),
            completed: false,
            due_date: None,
            priority: Priority::P0,
            notes: None,
            list_id: None,
            created_at: Some(unix_timestamp()),
            matrix_order: None,
        };
        validate(std::slice::from_ref(&task))?;
        Ok(task)
    }
    pub fn visible(&self, view: TaskView<'_>, today: Date) -> bool {
        match view {
            TaskView::Inbox => self.list_id.is_none(),
            TaskView::List(id) => self.list_id.as_deref() == Some(id),
            TaskView::Today => self
                .due_date
                .as_deref()
                .and_then(|d| parse_date(d).ok())
                .is_some_and(|d| d <= today),
        }
    }
}

pub fn sorted_visible_tasks<'a>(
    tasks: &'a [Task],
    view: TaskView<'_>,
    today: Date,
    sort: TaskSort,
    ascending: bool,
    show_completed: bool,
) -> Vec<&'a Task> {
    let mut visible: Vec<_> = tasks
        .iter()
        .filter(|task| task.visible(view, today) && (show_completed || !task.completed))
        .collect();
    visible.sort_by(|a, b| match sort {
        TaskSort::CreationDate => match (a.created_at, b.created_at) {
            (Some(a), Some(b)) => in_requested_direction(a.cmp(&b), ascending),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
        TaskSort::DueDate => match (&a.due_date, &b.due_date) {
            (Some(a), Some(b)) => in_requested_direction(a.cmp(b), ascending),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
        TaskSort::Priority => in_requested_direction(a.priority.cmp(&b.priority), ascending),
        TaskSort::Name => in_requested_direction(
            a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            ascending,
        ),
    });
    visible
}

#[cfg(test)]
pub fn visible_tasks<'a>(tasks: &'a [Task], view: TaskView<'_>, today: Date) -> Vec<&'a Task> {
    sorted_visible_tasks(
        tasks,
        view,
        today,
        TaskSort::CreationDate,
        TaskSort::CreationDate.default_ascending(),
        true,
    )
}

pub(crate) fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskList {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub pinned: bool,
}
impl TaskList {
    pub fn new(name: &str) -> Result<Self, String> {
        validate_list_name(name, &[], None)?;
        Ok(Self {
            id: new_id()?,
            name: name.trim().into(),
            pinned: false,
        })
    }

    pub fn visible_in_sidebar(&self, expanded: bool) -> bool {
        expanded || self.pinned
    }
}
fn new_id() -> Result<String, String> {
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|e| format!("Cannot create unique ID: {e}"))?;
    Ok(format!("{:032x}", u128::from_be_bytes(random)))
}
pub fn validate_list_name(
    name: &str,
    lists: &[TaskList],
    except: Option<&str>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A list name is required.".into());
    }
    if lists.iter().any(|list| {
        Some(list.id.as_str()) != except && list.name.trim().to_lowercase() == name.to_lowercase()
    }) {
        return Err("A list with this name already exists.".into());
    }
    Ok(())
}
pub fn validate_document(tasks: &[Task], lists: &[TaskList]) -> Result<(), String> {
    validate(tasks)?;
    let mut ids = HashSet::new();
    for list in lists {
        if list.id.trim().is_empty() || !ids.insert(list.id.as_str()) {
            return Err("Missing or duplicate list ID.".into());
        }
        validate_list_name(&list.name, lists, Some(&list.id))?;
    }
    if tasks
        .iter()
        .any(|task| task.list_id.as_deref().is_some_and(|id| !ids.contains(id)))
    {
        return Err("A task refers to a list that does not exist.".into());
    }
    Ok(())
}

pub fn parse_date(value: &str) -> Result<Date, String> {
    if value.len() != 10
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || !value
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return Err("Enter a valid date as YYYY-MM-DD, or leave it empty.".into());
    }
    value
        .parse()
        .map_err(|_| "Enter a valid date as YYYY-MM-DD, or leave it empty.".into())
}

pub fn validate(tasks: &[Task]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for task in tasks {
        if task.id.is_empty() || !ids.insert(&task.id) {
            return Err("Missing or duplicate task ID.".into());
        }
        if task.title.trim().is_empty() {
            return Err("A task title is required.".into());
        }
        if let Some(date) = &task.due_date {
            parse_date(date)?;
        }
        if task.list_id.as_ref().is_some_and(|id| id.trim().is_empty()) {
            return Err("A list ID cannot be empty.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unified_priority_defaults_range_and_quadrant_mapping() {
        assert_eq!(Task::new("New").unwrap().priority, Priority::P0);
        for (value, quadrant) in [
            (0, Quadrant::NotImportantNotUrgent),
            (1, Quadrant::NotImportantUrgent),
            (2, Quadrant::ImportantNotUrgent),
            (3, Quadrant::ImportantUrgent),
        ] {
            let priority = Priority::try_from(value).unwrap();
            assert_eq!(priority.quadrant(), quadrant);
            assert_eq!(quadrant.priority(), priority);
            assert_eq!(
                serde_json::to_value(priority).unwrap(),
                serde_json::json!(value)
            );
            assert_eq!(
                serde_json::from_value::<Priority>(serde_json::json!(value)).unwrap(),
                priority
            );
        }
        for value in [
            serde_json::json!(-1),
            serde_json::json!(4),
            serde_json::json!(256),
            serde_json::json!(1.5),
            serde_json::json!("High"),
            serde_json::Value::Null,
        ] {
            assert!(serde_json::from_value::<Priority>(value).is_err());
        }
    }
    #[test]
    fn task_contexts_keep_completed_tasks_and_today_excludes_future_tasks() {
        let today = parse_date("2026-10-17").unwrap();
        for (date, completed, expected) in [
            (None, false, false),
            (Some("2026-10-16"), false, true),
            (Some("2026-10-17"), false, true),
            (Some("2026-10-18"), false, false),
            (Some("2026-10-16"), true, true),
            (Some("2026-10-18"), true, false),
        ] {
            let mut task = Task::new("Task").unwrap();
            task.due_date = date.map(str::to_owned);
            task.completed = completed;
            task.list_id = Some("future-list".into());
            assert_eq!(task.visible(TaskView::Today, today), expected);
            assert!(!task.visible(TaskView::Inbox, today));
            assert!(task.visible(TaskView::List("future-list"), today));
        }
        let task = Task::new("Inbox").unwrap();
        assert!(task.visible(TaskView::Inbox, today));
        let mut completed_inbox = Task::new("Done in inbox").unwrap();
        completed_inbox.completed = true;
        assert!(completed_inbox.visible(TaskView::Inbox, today));
        let inbox_tasks = [completed_inbox.clone(), task.clone()];
        assert_eq!(
            sorted_visible_tasks(
                &inbox_tasks,
                TaskView::Inbox,
                today,
                TaskSort::CreationDate,
                TaskSort::CreationDate.default_ascending(),
                true,
            ),
            vec![&completed_inbox, &task]
        );
    }
    #[test]
    fn list_names_ids_and_references_are_validated() {
        for name in ["", " ", "\t\n"] {
            assert!(TaskList::new(name).is_err());
        }
        let list = TaskList::new("  École  ").unwrap();
        assert_eq!(list.name, "École");
        assert!(validate_list_name(" école ", std::slice::from_ref(&list), None).is_err());
        assert!(validate_list_name("ÉCOLE", std::slice::from_ref(&list), Some(&list.id)).is_ok());
        let mut duplicate = TaskList::new("école").unwrap();
        assert!(validate_document(&[], &[list.clone(), duplicate.clone()]).is_err());
        duplicate.id = list.id.clone();
        duplicate.name = "Other".into();
        assert!(validate_document(&[], &[list.clone(), duplicate]).is_err());
        let mut task = Task::new("Task").unwrap();
        task.list_id = Some("missing".into());
        assert!(
            validate_document(std::slice::from_ref(&task), std::slice::from_ref(&list)).is_err()
        );
        task.list_id = Some(list.id.clone());
        assert!(validate_document(&[task], &[list]).is_ok());
    }
    #[test]
    fn list_and_today_views_keep_completed_tasks_in_context_and_filter_is_non_destructive() {
        let today = parse_date("2026-10-18").unwrap();
        let mut first = Task::new("First").unwrap();
        first.list_id = Some("first".into());
        first.due_date = Some("2026-10-18".into());
        let mut second = first.clone();
        second.id = "other-task".into();
        second.list_id = Some("second".into());
        assert!(first.visible(TaskView::List("first"), today));
        assert!(!second.visible(TaskView::List("first"), today));
        assert!(!first.visible(TaskView::Inbox, today));
        for task in [&first, &second] {
            assert!(task.visible(TaskView::Today, today));
        }
        first.completed = true;
        assert!(first.visible(TaskView::List("first"), today));
        assert!(first.visible(TaskView::Today, today));
        assert_eq!(first.list_id.as_deref(), Some("first"));

        let mut active = first.clone();
        active.id = "active-task".into();
        active.completed = false;
        let all = [first.clone(), active.clone()];
        let visible = sorted_visible_tasks(
            &all,
            TaskView::List("first"),
            today,
            TaskSort::CreationDate,
            TaskSort::CreationDate.default_ascending(),
            true,
        );
        assert_eq!(visible, vec![&first, &active]);
        let hidden = sorted_visible_tasks(
            &all,
            TaskView::List("first"),
            today,
            TaskSort::CreationDate,
            TaskSort::CreationDate.default_ascending(),
            false,
        );
        assert_eq!(hidden, vec![&active]);
        assert!(first.completed);
    }

    #[test]
    fn all_task_sort_modes_are_stable_and_never_change_matrix_ranks() {
        let create = |id: &str,
                      title: &str,
                      created_at: Option<i64>,
                      due_date: Option<&str>,
                      priority: Priority,
                      completed: bool,
                      matrix_order: Option<u64>| {
            let mut task = Task::new(title).unwrap();
            task.id = id.into();
            task.created_at = created_at;
            task.due_date = due_date.map(str::to_owned);
            task.priority = priority;
            task.completed = completed;
            task.matrix_order = matrix_order;
            task
        };
        let tasks = vec![
            create(
                "old",
                "bravo",
                Some(10),
                Some("2026-10-20"),
                Priority::P0,
                false,
                Some(9),
            ),
            create("new", "Alpha", Some(30), None, Priority::P3, true, Some(2)),
            create(
                "same-time-first",
                "alpha",
                Some(30),
                Some("2026-10-18"),
                Priority::P1,
                false,
                Some(7),
            ),
            create(
                "unknown-first",
                "charlie",
                None,
                Some("2026-10-18"),
                Priority::P2,
                false,
                Some(4),
            ),
            create(
                "unknown-second",
                "delta",
                None,
                None,
                Priority::P2,
                true,
                Some(3),
            ),
        ];
        let ranks: Vec<_> = tasks.iter().map(|task| task.matrix_order).collect();
        let today = parse_date("2026-10-18").unwrap();
        let titles = |sort| {
            sorted_visible_tasks(
                &tasks,
                TaskView::Inbox,
                today,
                sort,
                sort.default_ascending(),
                true,
            )
            .into_iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>()
        };

        assert_eq!(
            titles(TaskSort::CreationDate),
            [
                "new",
                "same-time-first",
                "old",
                "unknown-first",
                "unknown-second"
            ]
        );
        assert_eq!(
            titles(TaskSort::DueDate),
            [
                "same-time-first",
                "unknown-first",
                "old",
                "new",
                "unknown-second"
            ]
        );
        assert_eq!(
            titles(TaskSort::Priority),
            [
                "new",
                "unknown-first",
                "unknown-second",
                "same-time-first",
                "old"
            ]
        );
        assert_eq!(
            titles(TaskSort::Name),
            [
                "new",
                "same-time-first",
                "old",
                "unknown-first",
                "unknown-second"
            ]
        );
        let ids_in_direction = |sort, ascending| {
            sorted_visible_tasks(&tasks, TaskView::Inbox, today, sort, ascending, true)
                .into_iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids_in_direction(TaskSort::CreationDate, true),
            [
                "old",
                "new",
                "same-time-first",
                "unknown-first",
                "unknown-second"
            ]
        );
        assert_eq!(
            ids_in_direction(TaskSort::DueDate, false),
            [
                "old",
                "same-time-first",
                "unknown-first",
                "new",
                "unknown-second"
            ]
        );
        assert_eq!(
            ids_in_direction(TaskSort::Priority, true),
            [
                "old",
                "same-time-first",
                "unknown-first",
                "unknown-second",
                "new"
            ]
        );
        assert_eq!(
            ids_in_direction(TaskSort::Name, false),
            [
                "unknown-second",
                "unknown-first",
                "old",
                "new",
                "same-time-first"
            ]
        );
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.matrix_order)
                .collect::<Vec<_>>(),
            ranks
        );

        let active_only = sorted_visible_tasks(
            &tasks,
            TaskView::Inbox,
            today,
            TaskSort::Name,
            TaskSort::Name.default_ascending(),
            false,
        );
        assert_eq!(
            active_only
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            ["same-time-first", "old", "unknown-first"]
        );
        assert!(tasks[1].completed && tasks[4].completed);
    }

    #[test]
    fn sort_mode_choices_and_default_directions_match_the_task_toolbar() {
        assert_eq!(sort_mode_at(0), Some(TaskSort::CreationDate));
        assert_eq!(sort_mode_at(1), Some(TaskSort::DueDate));
        assert_eq!(sort_mode_at(2), Some(TaskSort::Priority));
        assert_eq!(sort_mode_at(3), Some(TaskSort::Name));
        assert_eq!(sort_mode_at(4), None);
        assert!(!TaskSort::CreationDate.default_ascending());
        assert!(TaskSort::DueDate.default_ascending());
        assert!(!TaskSort::Priority.default_ascending());
        assert!(TaskSort::Name.default_ascending());
        assert_eq!(sort_order_label(true), "Ascending");
        assert_eq!(sort_order_label(false), "Descending");
    }

    #[test]
    fn legacy_tasks_keep_unknown_creation_time_without_synthesizing_history() {
        let legacy = serde_json::json!({
            "id":"legacy", "title":"Old", "completed":false, "due_date":null,
            "priority":0, "notes":null, "list_id":null, "matrix_order":0
        });
        let task: Task = serde_json::from_value(legacy).unwrap();
        assert_eq!(task.created_at, None);

        let created = Task::new("New").unwrap();
        assert!(created.created_at.is_some());
    }

    #[test]
    fn collapsed_lists_show_only_pinned_lists() {
        let mut pinned = TaskList::new("Pinned").unwrap();
        let unpinned = TaskList::new("Unpinned").unwrap();
        assert!(!pinned.pinned);
        assert!(pinned.visible_in_sidebar(true));
        assert!(!pinned.visible_in_sidebar(false));
        pinned.pinned = true;
        assert!(pinned.visible_in_sidebar(false));
        assert!(unpinned.visible_in_sidebar(true));
        assert!(!unpinned.visible_in_sidebar(false));

        let serialized = serde_json::to_value(&unpinned).unwrap();
        let mut legacy = serialized.as_object().unwrap().clone();
        legacy.remove("pinned");
        let restored: TaskList = serde_json::from_value(legacy.into()).unwrap();
        assert!(!restored.pinned);
    }
    #[test]
    fn dates_and_titles_are_validated() {
        for date in [
            "2026-02-29",
            "2026-13-01",
            "2026-1-01",
            "tomorrow",
            "2026-10-17T12:00",
        ] {
            assert!(parse_date(date).is_err());
        }
        assert!(parse_date("2028-02-29").is_ok());
        assert!(Task::new("  ").is_err());
        assert_ne!(Task::new("one").unwrap().id, Task::new("two").unwrap().id);
    }
}
