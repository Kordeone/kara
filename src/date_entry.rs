// SPDX-License-Identifier: MPL-2.0
//! Parsing and non-destructive formatting for the task due-date field.

use jiff::{ToSpan, civil::Date};

const INVALID: &str = "Enter a valid date, or leave it empty.";

/// Normalize a complete date while leaving partial edits untouched.
///
/// Deferring normalization until all components are present avoids disrupting
/// cursor movement, deletion, selection, and replacement while a date is typed.
pub fn normalize_edit(value: &str, current_year: i16) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return value.to_string();
    }
    if trimmed.ends_with(['-', ' ']) || trimmed.starts_with(['-', ' ']) {
        return value.to_string();
    }

    let compact: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if compact.len() == 8 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}-{}-{}", &compact[..4], &compact[4..6], &compact[6..]);
    }

    let parts: Vec<_> = trimmed
        .split(['-', ' '])
        .filter(|s| !s.is_empty())
        .collect();
    let (year, month, day) = match parts.as_slice() {
        [year, _, _] if year.len() != 4 => return value.to_string(),
        [year, month, day] if year.len() == 4 => (*year, *month, *day),
        [year, _] if year.len() == 4 => return value.to_string(),
        [month, day] => ("", *month, *day),
        _ => return value.to_string(),
    };
    if month == "0" || day == "0" {
        return value.to_string();
    }
    if !month.bytes().all(|b| b.is_ascii_digit())
        || !day.bytes().all(|b| b.is_ascii_digit())
        || (!year.is_empty() && !year.bytes().all(|b| b.is_ascii_digit()))
        || month.len() > 2
        || day.len() > 2
    {
        return value.to_string();
    }
    let year = if year.is_empty() {
        current_year.to_string()
    } else {
        year.to_string()
    };
    let Ok(month_num) = month.parse::<u8>() else {
        return value.to_string();
    };
    let Ok(day_num) = day.parse::<u8>() else {
        return value.to_string();
    };
    format!("{year}-{month_num:02}-{day_num:02}")
}

/// Add separators to a complete uninterrupted digit entry without modifying
/// partial values while the text field still has focus.
pub fn normalize_continuous_input(value: &str) -> String {
    let compact: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if value.chars().all(|c| c.is_ascii_digit()) && compact.len() == 8 {
        format!("{}-{}-{}", &compact[..4], &compact[4..6], &compact[6..])
    } else {
        value.to_string()
    }
}

/// Parse an optional due date and return its canonical storage representation.
pub fn parse_optional(value: &str, current_year: i16) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let normalized = normalize_edit(value, current_year);
    let date: Date = normalized.parse().map_err(|_| INVALID.to_string())?;
    Ok(Some(date.to_string()))
}

/// Show an error only once the field contains a complete-looking date.
pub fn validation_error_for_edit(value: &str, current_year: i16) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.ends_with(['-', ' ']) {
        return None;
    }
    let parts: Vec<_> = value
        .split(['-', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() == 1
        || (parts.len() == 2 && parts[0].len() == 4)
        || parts.last().is_some_and(|day| *day == "0")
    {
        return None;
    }
    if parts.len() == 3 && parts[0].bytes().all(|b| b.is_ascii_digit()) && parts[0].len() < 4 {
        return None;
    }
    (parts.len() == 2 || parts.len() == 3)
        .then(|| parse_optional(value, current_year).err())
        .flatten()
}

/// Return a date for the picker, using today when the entry is empty/partial.
pub fn picker_date(value: &str, today: Date) -> Date {
    value.parse().unwrap_or(today)
}

/// Return a local-calendar day offset used by the due-date shortcuts.
pub fn relative_date(today: Date, days: i64) -> Date {
    today
        .checked_add(days.days())
        .expect("task date shortcut stays within the supported calendar range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_examples_normalize_to_iso_dates() {
        for input in ["19990203", "1999 2 3", "1999-2-3"] {
            assert_eq!(
                parse_optional(input, 2026).unwrap(),
                Some("1999-02-03".into())
            );
        }
        assert_eq!(
            parse_optional("02-03", 2026).unwrap(),
            Some("2026-02-03".into())
        );
    }

    #[test]
    fn empty_and_local_year_inference_are_supported() {
        assert_eq!(parse_optional("  ", 2026).unwrap(), None);
        assert_eq!(
            parse_optional("2 3", 2041).unwrap(),
            Some("2041-02-03".into())
        );
    }

    #[test]
    fn validates_leap_years_and_real_calendar_dates() {
        assert_eq!(
            parse_optional("2024-2-29", 2026).unwrap(),
            Some("2024-02-29".into())
        );
        assert!(parse_optional("2023-2-29", 2026).is_err());
        assert!(parse_optional("2024-4-31", 2026).is_err());
        assert!(parse_optional("2024-13-1", 2026).is_err());
    }

    #[test]
    fn formatting_leaves_partial_edits_untouched() {
        for input in [
            "1",
            "199",
            "1999-",
            "1999-2",
            "1999-02-",
            "1999-02-0",
            "1999 2",
        ] {
            assert_eq!(normalize_edit(input, 2026), input);
        }
        assert_eq!(validation_error_for_edit("1999-02-0", 2026), None);
        assert_eq!(normalize_continuous_input("1999020"), "1999020");
        assert_eq!(normalize_continuous_input("19990203"), "1999-02-03");
        assert_eq!(normalize_edit("19990203", 2026), "1999-02-03");
        assert_eq!(normalize_edit("02-03", 2026), "2026-02-03");
    }

    #[test]
    fn invalid_submission_keeps_value_and_correcting_clears_inline_error() {
        let typed = "2023-2-29";
        assert!(parse_optional(typed, 2026).is_err());
        assert_eq!(typed, "2023-2-29");
        assert!(validation_error_for_edit(typed, 2026).is_some());
        assert_eq!(validation_error_for_edit("2024-2-29", 2026), None);
        assert_eq!(
            parse_optional("2024-2-29", 2026).unwrap(),
            Some("2024-02-29".into())
        );
    }

    #[test]
    fn relative_shortcuts_cross_month_year_and_leap_day_boundaries() {
        let today: Date = "2023-12-29".parse().unwrap();
        assert_eq!(relative_date(today, 0).to_string(), "2023-12-29");
        assert_eq!(relative_date(today, 1).to_string(), "2023-12-30");
        assert_eq!(relative_date(today, 7).to_string(), "2024-01-05");

        let leap_eve: Date = "2024-02-28".parse().unwrap();
        assert_eq!(relative_date(leap_eve, 1).to_string(), "2024-02-29");
        assert_eq!(relative_date(leap_eve, 7).to_string(), "2024-03-06");
    }
}
