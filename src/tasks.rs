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
pub enum TaskView {
    Inbox,
    Today,
    Completed,
}

impl Task {
    pub fn new(title: &str) -> Result<Self, String> {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|e| format!("Cannot create task ID: {e}"))?;
        let task = Self {
            id: format!("{:032x}", u128::from_be_bytes(random)),
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
    pub fn visible(&self, view: TaskView, today: Date) -> bool {
        match view {
            TaskView::Inbox => !self.completed && self.list_id.is_none(),
            TaskView::Completed => self.completed,
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
