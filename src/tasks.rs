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
    Completed,
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
            TaskView::Inbox => !self.completed && self.list_id.is_none(),
            TaskView::Completed => self.completed,
            TaskView::List(id) => !self.completed && self.list_id.as_deref() == Some(id),
            TaskView::Today => {
                !self.completed
                    && self
                        .due_date
                        .as_deref()
                        .and_then(|d| parse_date(d).ok())
                        .is_some_and(|d| d <= today)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskList {
    pub id: String,
    pub name: String,
}
impl TaskList {
    pub fn new(name: &str) -> Result<Self, String> {
        validate_list_name(name, &[], None)?;
        Ok(Self {
            id: new_id()?,
            name: name.trim().into(),
        })
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
    fn today_includes_overdue_and_named_lists_but_excludes_completed_and_future() {
        let today = parse_date("2026-10-17").unwrap();
        for (date, completed, expected) in [
            (None, false, false),
            (Some("2026-10-16"), false, true),
            (Some("2026-10-17"), false, true),
            (Some("2026-10-18"), false, false),
            (Some("2026-10-16"), true, false),
        ] {
            let mut task = Task::new("Task").unwrap();
            task.due_date = date.map(str::to_owned);
            task.completed = completed;
            task.list_id = Some("future-list".into());
            assert_eq!(task.visible(TaskView::Today, today), expected);
            assert!(!task.visible(TaskView::Inbox, today));
            assert_eq!(task.visible(TaskView::Completed, today), completed);
        }
        let task = Task::new("Inbox").unwrap();
        assert!(task.visible(TaskView::Inbox, today));
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
    fn list_views_exclude_other_lists_and_completed_tasks_while_global_views_include_them() {
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
        assert!(!first.visible(TaskView::List("first"), today));
        assert!(!first.visible(TaskView::Today, today));
        assert!(first.visible(TaskView::Completed, today));
        assert_eq!(first.list_id.as_deref(), Some("first"));
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
