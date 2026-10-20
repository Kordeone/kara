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
    pub amount: String,
    pub unit: crate::automatic::Unit,
    pub destination: Option<PathBuf>,
    pub destination_verified: bool,
    pub destination_error: Option<String>,
    pub interval_error: Option<String>,
    pub retention: String,
    pub retention_error: Option<String>,
    pub add_preview: Option<Result<crate::import::Summary, String>>,
    pub replace_confirm: bool,
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
    ChooseReplace,
    AddImport,
    Retention(String),
    AutoEnabled(bool),
    Amount(String),
    Unit(usize),
    ChooseFolder,
    FolderChosen(Result<Option<PathBuf>, String>),
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
            if !self.replace_confirm {
                let preview = match &self.add_preview {
                    Some(Ok(summary)) => summary.description(),
                    Some(Err(error)) => error.clone(),
                    None => "Choose Add to keep existing data, or Replace to restore the complete backup.".into(),
                };
                return Some(widget::dialog().title("Import Backup")
                    .body(format!("Backup: {} tasks, {} lists.\nAdd preview: {preview}{}\nA verified recovery backup is created before changes. Replace will also replace saved preferences.", backup.task_count(), backup.list_count(), self.notice.as_ref().map(|s| format!("\n{s}")).unwrap_or_default()))
                    .primary_action(widget::button::suggested("Add").on_press_maybe((available && matches!(self.add_preview,Some(Ok(_)))).then_some(message(Action::AddImport))))
                    .tertiary_action(widget::button::destructive("Replace").on_press_maybe(available.then_some(message(Action::ChooseReplace))))
                    .secondary_action(widget::button::standard("Cancel").on_press(message(Action::Close))).into());
            }
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
    pub fn can_apply(&self, available: bool) -> bool {
        available
            && self.auto_enabled
            && !self.busy
            && crate::automatic::configuration_interval(&self.amount, self.unit).is_ok()
            && crate::automatic::retention_count(&self.retention).is_ok()
            && self.destination.is_some()
            && self.destination_verified
    }
    pub fn settings_view(
        &self,
        available: bool,
        preferences: &crate::automatic::Preferences,
        automatic_error: Option<&str>,
    ) -> Element<'_, Message> {
        let message = |action| Message::Backup(action);
        let spacing = cosmic::theme::spacing();
        let enabled = available && self.auto_enabled && !self.busy;
        let valid = self.can_apply(available);
        let mut amount = widget::text_input("6", &self.amount)
            .label("Every")
            .width(cosmic::iced::Length::FillPortion(1));
        let mut retention = widget::text_input("10", &self.retention)
            .label("Keep last (1–100)")
            .width(cosmic::iced::Length::FillPortion(1));
        if enabled {
            amount = amount.on_input(|value| Message::Backup(Action::Amount(value)));
            retention = retention.on_input(|value| Message::Backup(Action::Retention(value)));
        }
        let unit: Element<'_, Message> = if enabled {
            widget::dropdown(
                &crate::automatic::Unit::LABELS[..],
                Some(self.unit.index()),
                |index| Message::Backup(Action::Unit(index)),
            )
            .width(cosmic::iced::Length::Fill)
            .into()
        } else {
            widget::button::standard(crate::automatic::Unit::LABELS[self.unit.index()])
                .width(cosmic::iced::Length::Fill)
                .into()
        };
        let fields = widget::row::with_capacity(2)
            .spacing(spacing.space_s)
            .push(amount)
            .push(
                widget::column::with_capacity(2)
                    .spacing(spacing.space_xxs)
                    .push(widget::text::caption("Unit"))
                    .push(unit)
                    .width(cosmic::iced::Length::FillPortion(2)),
            );
        let mut automatic = widget::column::with_capacity(12)
            .spacing(spacing.space_s)
            .push(widget::text::title4("Automatic backups"))
            .push(
                widget::toggler(self.auto_enabled)
                    .label(String::from("Enable automatic backups"))
                    .width(cosmic::iced::Length::Fill)
                    .on_toggle_maybe(
                        available.then_some(|value| Message::Backup(Action::AutoEnabled(value))),
                    ),
            )
            .push(fields)
            .push(widget::text::caption("Minimum 1 minute"))
            .push(retention)
            .push(widget::text::caption("Destination folder"))
            .push(widget::text::body(
                self.destination
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "No folder selected".into()),
            ))
            .push(
                widget::button::text("Choose folder…")
                    .on_press_maybe(enabled.then_some(message(Action::ChooseFolder))),
            );
        for error in [
            &self.interval_error,
            &self.retention_error,
            &self.destination_error,
        ]
        .into_iter()
        .flatten()
        {
            automatic = automatic.push(widget::text::body(error));
        }
        let last = preferences
            .last_success
            .and_then(|t| jiff::Timestamp::from_second(t).ok())
            .map(|t| {
                let local = t.to_zoned(jiff::tz::TimeZone::system());
                format!("{} {:02}:{:02}", local.date(), local.hour(), local.minute())
            })
            .unwrap_or_else(|| "Not yet".into());
        automatic = automatic
            .push(widget::text::caption(format!("Last backup: {last}")))
            .push(
                widget::button::text("Apply")
                    .on_press_maybe(valid.then_some(message(Action::SaveAutomatic))),
            );
        if let Some(error) = automatic_error {
            automatic = automatic.push(widget::text::body(error.to_string()));
        }
        let export = widget::button::text("Export Backup")
            .leading_icon(
                widget::icon::from_svg_bytes(include_bytes!(
                    "../assets/tabler/database-export.svg"
                ))
                .symbolic(true),
            )
            .on_press_maybe(available.then_some(message(Action::Export)));
        let import = widget::button::text("Import Backup")
            .leading_icon(
                widget::icon::from_svg_bytes(include_bytes!(
                    "../assets/tabler/database-import.svg"
                ))
                .symbolic(true),
            )
            .on_press_maybe(available.then_some(message(Action::Restore)));
        let content = widget::column::with_capacity(7).spacing(spacing.space_m)
            .push(widget::text::title4("Backup & Restore"))
            .push(widget::row::with_capacity(2).spacing(spacing.space_s)
                .push(export.width(cosmic::iced::Length::FillPortion(1)))
                .push(import.width(cosmic::iced::Length::FillPortion(1))))
            .push(widget::divider::horizontal::default())
            .push(automatic)
            .push(widget::divider::horizontal::default())
            .push(widget::column::with_capacity(3).spacing(spacing.space_s)
                .push(widget::text::title4("Clear All Data"))
                .push(widget::text::body("Reset tasks, lists and saved preferences. Backup archives will be preserved."))
                .push(widget::button::destructive("Clear all data…").on_press_maybe(available.then_some(message(Action::Clear)))));
        let mut status = widget::column::with_capacity(3).spacing(cosmic::theme::spacing().space_s);
        if self.busy {
            status = status.push(widget::text::body(
                "Waiting for the file dialog or file operation…",
            ));
        }
        if let Some(notice) = &self.notice {
            status = status.push(widget::text::body(notice));
        }
        let mut content = status.push(content);
        if !available && !self.busy {
            content = content.push(widget::text::body(
                "Close any task dialog and resolve storage errors to use backup actions.",
            ));
        }
        widget::scrollable(content.padding(spacing.space_xxs))
            .height(cosmic::iced::Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apply_requires_enabled_valid_form_and_verified_destination() {
        let mut state = State {
            amount: "6".into(),
            unit: crate::automatic::Unit::Hours,
            retention: "10".into(),
            destination: Some("/example".into()),
            destination_verified: true,
            ..State::default()
        };
        assert!(!state.can_apply(true));
        state.auto_enabled = true;
        assert!(state.can_apply(true));
        assert!(!state.can_apply(false));
        state.busy = true;
        assert!(!state.can_apply(true));
        state.busy = false;
        state.destination_verified = false;
        assert!(!state.can_apply(true));
        state.destination_verified = true;
        state.destination = None;
        assert!(!state.can_apply(true));
        state.destination = Some("/example".into());
        state.amount = "0".into();
        assert!(!state.can_apply(true));
        state.amount = "6".into();
        state.retention = "101".into();
        assert!(!state.can_apply(true));
    }
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
        for (width, height) in [(300., 480.), (400., 480.), (640., 560.), (1000., 700.)] {
            for state in [
                State {
                    open: true,
                    auto_enabled: true,
                    amount: "6".into(),
                    unit: crate::automatic::Unit::Months,
                    retention: "10".into(),
                    destination: Some(
                        "/run/user/1000/doc/long-document-portal-id/backup-folder/long-name".into(),
                    ),
                    destination_verified: true,
                    ..State::default()
                },
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
                    replace_confirm: true,
                    ..State::default()
                },
                State {
                    open: true,
                    pending: Some(prepared.clone()),
                    add_preview: Some(Err("Ambiguous list ID. ".repeat(40))),
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
