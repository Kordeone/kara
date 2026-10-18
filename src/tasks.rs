// SPDX-License-Identifier: MPL-2.0
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}
impl Priority {
    pub const ALL: [Self; 3] = [Self::Low, Self::Normal, Self::High];
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
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
            priority: Priority::Normal,
            notes: None,
            list_id: None,
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

pub fn visible_tasks<'a>(tasks: &'a [Task], view: TaskView<'_>, today: Date) -> Vec<&'a Task> {
    let mut visible: Vec<_> = tasks
        .iter()
        .filter(|task| task.visible(view, today))
        .collect();
    visible.sort_by_key(|task| task.completed);
    visible
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
            visible_tasks(&inbox_tasks, TaskView::Inbox, today),
            vec![&task, &completed_inbox]
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
    fn list_and_today_views_keep_completed_tasks_in_context_after_active_tasks() {
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
        let visible = visible_tasks(&all, TaskView::List("first"), today);
        assert_eq!(visible, vec![&active, &first]);
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
