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
        if let Some(backup) = &self.pending {
            let mut dialog = widget::dialog()
                .title("Replace Kara data?")
                .body(format!("Replace all {tasks} current tasks, {lists} lists and saved sorting/display preferences with {} tasks and {} lists from this backup. This replaces data; it does not merge it. A verified recovery backup will be saved in Kara's data directory before replacement.", backup.task_count(), backup.list_count()))
                .primary_action(widget::button::destructive("Replace data").on_press_maybe(available.then_some(message(Action::Confirm))))
                .secondary_action(widget::button::standard("Cancel").on_press(message(Action::Close)));
            if let Some(notice) = &self.notice {
                dialog = dialog.control(widget::text::body(notice));
            }
            return Some(dialog.into());
        }
        let mut content = widget::column::with_capacity(4)
            .spacing(cosmic::theme::spacing().space_s)
            .push(widget::text::body("Export or restore tasks, lists, Matrix ordering and saved sorting/display preferences. Backups are local files, without encryption. Choose a new .kara-backup filename; existing files are never overwritten."))
            .push(widget::button::standard("Export backup…").on_press_maybe(available.then_some(message(Action::Export))))
            .push(widget::button::standard("Restore backup…").on_press_maybe(available.then_some(message(Action::Restore))));
        if self.busy {
            content = content.push(widget::text::body(
                "Waiting for the file dialog or file operation…",
            ));
        }
        if let Some(notice) = &self.notice {
            content = content.push(widget::text::body(notice));
        }
        if !available && !self.busy {
            content = content.push(widget::text::body(
                "Resolve the task storage error before exporting or restoring.",
            ));
        }
        Some(
            widget::dialog()
                .title("Backup and Restore")
                .control(widget::container(widget::scrollable(content)).max_height(300.))
                .secondary_action(
                    widget::button::standard("Close")
                        .on_press_maybe((!self.busy).then_some(message(Action::Close))),
                )
                .into(),
        )
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
                    pending: Some(prepared.clone()),
                    ..State::default()
                },
            ] {
                let mut panel = state.view(!state.busy, 500, 20).unwrap();
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
