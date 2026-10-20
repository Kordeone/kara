// SPDX-License-Identifier: MPL-2.0
//! Compact native backup panel and explicit replacement confirmation.
use crate::{app::Message, backup::Prepared};
use cosmic::{Element, widget};
use std::{path::PathBuf, sync::Arc};

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub busy: bool,
    pub pending: Option<Arc<Prepared>>,
    pub notice: Option<String>,
    pub auto_enabled: bool,
    pub interval: String,
    pub interval_error: Option<String>,
    pub clear_open: bool,
    pub clear_text: String,
}
#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Close,
    Export,
    Restore,
    Exported(Result<Option<PathBuf>, String>),
    Loaded(Result<Option<Arc<Prepared>>, String>),
    Confirm,
    AutoEnabled(bool),
    Interval(String),
    SaveAutomatic,
    Clear,
    ClearText(String),
    ConfirmClear,
}
impl State {
    pub fn view(
        &self,
        available: bool,
        tasks: usize,
        lists: usize,
    ) -> Option<Element<'_, Message>> {
        if !self.open {
            return None;
        }
        let message = |action| Message::Backup(action);
        if self.clear_open {
            return Some(widget::dialog().title("Clear All Data?")
                .body(format!("Clear all {tasks} tasks, {lists} lists and Kara's saved preferences, including automatic backup settings. A verified recovery backup is required first. Manual backups, automatic archives and recovery files will remain. Type CLEAR to confirm."))
                .control(widget::text_input("Type CLEAR", &self.clear_text).on_input(|s| Message::Backup(Action::ClearText(s))))
                .primary_action(widget::button::destructive("Clear all data").on_press_maybe((available && self.clear_text == "CLEAR").then_some(message(Action::ConfirmClear))))
                .secondary_action(widget::button::standard("Cancel").on_press(message(Action::Close))).into());
        }
        if let Some(backup) = &self.pending {
            let mut dialog = widget::dialog()
                .title("Replace Kara data?")
                .body(format!("Replace all {tasks} current tasks, {lists} lists and saved preferences (including automatic backup settings) with {} tasks and {} lists from this backup. This replaces data; it does not merge it. A verified recovery backup will be saved in Kara's data directory before replacement.", backup.task_count(), backup.list_count()))
                .primary_action(widget::button::destructive("Replace data").on_press_maybe(available.then_some(message(Action::Confirm))))
                .secondary_action(widget::button::standard("Cancel").on_press(message(Action::Close)));
            if let Some(notice) = &self.notice {
                dialog = dialog.control(widget::text::body(notice));
            }
            return Some(dialog.into());
        }
        None
    }
    pub fn settings_view(
        &self,
        available: bool,
        preferences: &crate::automatic::Preferences,
        automatic_error: Option<&str>,
    ) -> Element<'_, Message> {
        let message = |action| Message::Backup(action);
        let mut automatic = widget::settings::section()
            .title("Automatic backups")
            .add(widget::toggler(self.auto_enabled).label(String::from("Enable automatic backups")).on_toggle_maybe(available.then_some(|value| Message::Backup(Action::AutoEnabled(value)))))
            .add(widget::text_input("PT6H", &self.interval).label("ISO 8601 interval").on_input(|value| Message::Backup(Action::Interval(value))))
            .add(widget::text::caption("PT30M · PT6H · P1D · P1W. Fixed weeks/days, hours, minutes and whole seconds; no months or years. Apply to save changes. Keep the latest 10 successful automatic archives."));
        if let Some(error) = &self.interval_error {
            automatic = automatic.add(widget::text::body(error.to_string()));
        }
        let last = preferences
            .last_success
            .and_then(|t| jiff::Timestamp::from_second(t).ok())
            .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).to_string())
            .unwrap_or_else(|| "None yet".into());
        automatic = automatic
            .add(widget::text::body(format!(
                "Last successful automatic backup: {last}"
            )))
            .add(
                widget::button::suggested("Apply automatic backup settings").on_press_maybe(
                    (available && crate::automatic::interval_seconds(&self.interval).is_ok())
                        .then_some(message(Action::SaveAutomatic)),
                ),
            );
        if let Some(error) = automatic_error {
            automatic = automatic.add(widget::text::body(error.to_string()));
        }
        let mut content = widget::column::with_capacity(6)
            .spacing(cosmic::theme::spacing().space_s)
            .push(widget::settings::section().title("Backup & Restore")
                .add(widget::text::body("Export or restore tasks, lists and preferences. Choose a new .kara-backup filename; existing files are never overwritten. Backups are local and unencrypted."))
                .add(widget::button::standard("Export backup…").on_press_maybe(available.then_some(message(Action::Export))))
                .add(widget::button::standard("Restore backup…").on_press_maybe(available.then_some(message(Action::Restore)))))
            .push(automatic)
            .push(widget::settings::section().title("Clear All Data")
                .add(widget::text::body("Reset tasks, lists and saved preferences. Backup archives will be preserved."))
                .add(widget::button::destructive("Clear all data…").on_press_maybe(available.then_some(message(Action::Clear)))));
        let mut status = widget::column::with_capacity(3).spacing(cosmic::theme::spacing().space_s);
        if self.busy {
            status = status.push(widget::text::body(
                "Waiting for the file dialog or file operation…",
            ));
        }
        if let Some(notice) = &self.notice {
            status = status.push(widget::text::body(notice));
        }
        content = status.push(content);
        if !available && !self.busy {
            content = content.push(widget::text::body(
                "Close any task dialog and resolve storage errors to use backup actions.",
            ));
        }
        widget::scrollable(content)
            .height(cosmic::iced::Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_panels_keep_footer_actions_inside_small_viewports() {
        use cosmic::iced::{
            Size,
            advanced::{layout::Limits, renderer::Headless, widget::Tree},
        };
        let renderer = cosmic::Renderer::new(cosmic::font::default(), 14.into(), Some("tiny-skia"))
            .await
            .unwrap();
        let prepared = Arc::new(
            crate::backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup"))
                .unwrap(),
        );
        for (width, height) in [(400., 480.), (640., 560.), (1000., 700.)] {
            for state in [
                State {
                    open: true,
                    ..State::default()
                },
                State {
                    open: true,
                    busy: true,
                    ..State::default()
                },
                State {
                    open: true,
                    notice: Some("A long recoverable file operation error. ".repeat(50)),
                    ..State::default()
                },
                State {
                    open: true,
                    clear_open: true,
                    clear_text: "CLEAR".into(),
                    ..State::default()
                },
                State {
                    open: true,
                    interval: "bad interval".into(),
                    interval_error: Some("Invalid duration".into()),
                    ..State::default()
                },
                State {
                    open: true,
                    pending: Some(prepared.clone()),
                    ..State::default()
                },
            ] {
                let Some(mut panel) = state.view(!state.busy, 500, 20) else {
                    let mut settings = state.settings_view(
                        !state.busy,
                        &crate::automatic::Preferences::default(),
                        None,
                    );
                    let mut tree = Tree::new(&settings);
                    let layout = settings.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &Limits::new(Size::ZERO, Size::new(width, height)),
                    );
                    assert!(layout.size().width <= width && layout.size().height <= height);
                    continue;
                };
                let mut tree = Tree::new(&panel);
                let layout = panel.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &Limits::new(Size::ZERO, Size::new(width, height)),
                );
                assert!(layout.size().width <= width && layout.size().height <= height);
                let column = &layout.children()[0];
                let footer = column.children().last().unwrap();
                assert!(footer.bounds().y + footer.size().height <= column.size().height + 0.01);
                assert!(footer.size().height >= 30.);
            }
        }
    }
}
