// SPDX-License-Identifier: MPL-2.0
//! Civil-date month calculations and views over the shared task records.
use crate::tasks::{Task, parse_date};
use jiff::{Span, civil::Date};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter<'a> {
    All,
    Inbox,
    List(&'a str),
}
impl Filter<'_> {
    pub fn includes(self, task: &Task) -> bool {
        match self {
            Self::All => true,
            Self::Inbox => task.list_id.is_none(),
            Self::List(id) => task.list_id.as_deref() == Some(id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub month: Date,
    pub selected: Date,
}
impl State {
    pub fn new(today: Date) -> Self {
        Self {
            month: today.first_of_month(),
            selected: today,
        }
    }
    pub fn adjacent(&self, delta: i64) -> Option<Date> {
        self.month.checked_add(Span::new().months(delta)).ok()
    }
    pub fn navigate(&mut self, delta: i64) {
        if let Some(month) = self.adjacent(delta) {
            self.selected = Date::new(
                month.year(),
                month.month(),
                self.selected.day().min(month.days_in_month()),
            )
            .expect("a clamped day is valid in the destination month");
            self.month = month;
        }
    }
    pub fn select(&mut self, date: Date) {
        self.month = date.first_of_month();
        self.selected = date;
    }
    pub fn today(&mut self, today: Date) {
        self.select(today);
    }
    pub fn label(&self) -> String {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        format!(
            "{} {}",
            MONTHS[(self.month.month() - 1) as usize],
            self.month.year()
        )
    }
    /// Complete Monday-through-Sunday weeks, including adjacent-month dates.
    /// Only the unrepresentable days beyond Date::MIN/MAX are blank.
    pub fn cells(&self) -> Vec<Option<Date>> {
        let offset = i64::from(self.month.weekday().to_monday_zero_offset());
        let count = ((offset + i64::from(self.month.days_in_month()) + 6) / 7) * 7;
        (0..count)
            .map(|i| self.month.checked_add(Span::new().days(i - offset)).ok())
            .collect()
    }
}

pub fn on_date<'a>(tasks: &'a [Task], date: Date, filter: Filter<'_>) -> Vec<&'a Task> {
    let mut visible: Vec<_> = tasks
        .iter()
        .filter(|task| {
            filter.includes(task)
                && task.due_date.as_deref().and_then(|d| parse_date(d).ok()) == Some(date)
        })
        .collect();
    // Stable within each completion group; never rearranges the stored vector.
    visible.sort_by_key(|task| task.completed);
    visible
}

pub fn draft(date: Date, filter: Filter<'_>) -> Result<Task, String> {
    let mut task = Task::new("New task")?;
    task.title.clear();
    task.due_date = Some(date.to_string());
    task.list_id = match filter {
        Filter::List(id) => Some(id.to_string()),
        _ => None,
    };
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn date(s: &str) -> Date {
        s.parse().unwrap()
    }
    #[test]
    fn month_navigation_clamps_days_and_crosses_years() {
        let mut state = State::new(date("2024-01-31"));
        state.navigate(1);
        assert_eq!(state.selected, date("2024-02-29"));
        assert_eq!(state.label(), "February 2024");
        state.select(date("2023-01-31"));
        state.navigate(1);
        assert_eq!(state.selected, date("2023-02-28"));
        state.select(date("2024-12-15"));
        state.navigate(1);
        assert_eq!(state.month, date("2025-01-01"));
        state.navigate(-1);
        assert_eq!(state.selected, date("2024-12-15"));
        state.today(date("2026-10-19"));
        assert_eq!(state, State::new(date("2026-10-19")));
    }
    #[test]
    fn complete_weeks_cover_months_and_leap_centuries() {
        for (value, days, cells) in [
            ("2021-02-01", 28, 28),
            ("2024-02-01", 29, 35),
            ("2026-03-01", 31, 42),
            ("1900-02-01", 28, 35),
            ("2000-02-01", 29, 35),
        ] {
            let state = State::new(date(value));
            let grid = state.cells();
            assert_eq!(grid.len(), cells);
            assert_eq!(grid[0].unwrap().weekday(), jiff::civil::Weekday::Monday);
            assert_eq!(
                grid.last().unwrap().unwrap().weekday(),
                jiff::civil::Weekday::Sunday
            );
            assert_eq!(
                grid.iter()
                    .flatten()
                    .filter(|d| d.month() == state.month.month())
                    .count(),
                days
            );
            for pair in grid.windows(2) {
                assert_eq!(pair[0].unwrap().tomorrow().unwrap(), pair[1].unwrap());
            }
        }
    }
    #[test]
    fn representable_limits_do_not_panic_or_wrap() {
        for (date, delta) in [(Date::MIN, -1), (Date::MAX, 1)] {
            let mut state = State::new(date);
            let before = state.clone();
            state.navigate(delta);
            assert_eq!(state, before);
            assert!(state.adjacent(delta).is_none());
            assert_eq!(state.cells().len() % 7, 0);
        }
    }
    #[test]
    fn date_placement_filters_shared_records_without_utc_shifts_or_duplication() {
        let d = date("2024-02-29");
        let mut inbox = Task::new("Inbox").unwrap();
        inbox.due_date = Some(d.to_string());
        let mut named = inbox.clone();
        named.id = "named".into();
        named.list_id = Some("work".into());
        named.completed = true;
        let mut other = inbox.clone();
        other.id = "other".into();
        other.due_date = Some("2024-03-01".into());
        let undated = Task::new("No date").unwrap();
        let tasks = vec![named.clone(), inbox.clone(), other, undated];
        let before = tasks.clone();
        assert_eq!(
            on_date(&tasks, d, Filter::All)
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec![inbox.id.as_str(), named.id.as_str()]
        );
        assert_eq!(on_date(&tasks, d, Filter::Inbox)[0].id, inbox.id);
        assert_eq!(on_date(&tasks, d, Filter::List("work"))[0].id, named.id);
        assert!(on_date(&tasks, d, Filter::List("missing")).is_empty());
        assert_eq!(tasks, before);
    }
    #[test]
    fn creation_preselects_civil_date_and_shared_list_with_normal_defaults() {
        for filter in [Filter::All, Filter::Inbox, Filter::List("work")] {
            let task = draft(date("2024-02-29"), filter).unwrap();
            assert_eq!(task.due_date.as_deref(), Some("2024-02-29"));
            assert_eq!(
                task.list_id.as_deref(),
                if filter == Filter::List("work") {
                    Some("work")
                } else {
                    None
                }
            );
            assert_eq!(task.priority, crate::tasks::Priority::P0);
            assert!(!task.completed);
            assert!(task.title.is_empty());
        }
    }
}
