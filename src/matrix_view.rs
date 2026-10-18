// SPDX-License-Identifier: MPL-2.0
//! Matrix-specific widgets; Tasks navigation and dialogs stay in the existing app.
use crate::{
    app::Message,
    matrix::{self, Direction},
    storage::Store,
    tasks::{Quadrant, Task},
};
use cosmic::{
    Element,
    iced::{Alignment, Length},
    widget::{self, icon},
};

#[derive(Debug, Clone)]
pub enum Action {
    SelectList(usize),
    Add(Option<Quadrant>),
    Classify(String, Option<Quadrant>),
    Reorder(String, Direction),
}

pub fn view<'a>(
    store: Option<&'a Store>,
    list: Option<&'a str>,
    can_write: bool,
) -> Element<'a, Message> {
    let Some(store) = store else {
        return widget::text::body(
            "Matrix tasks are unavailable. Resolve the storage error and retry.",
        )
        .into();
    };
    let spacing = cosmic::theme::spacing();
    let mut options = vec!["Inbox".to_string()];
    options.extend(store.lists().iter().map(|list| list.name.clone()));
    let selected = list
        .and_then(|id| store.lists().iter().position(|list| list.id == id))
        .map_or(0, |i| i + 1);
    let selector = widget::column::with_capacity(2)
        .spacing(spacing.space_xxs)
        .push(widget::text::body("Task list"))
        .push(widget::dropdown(options, Some(selected), |index| {
            Message::Matrix(Action::SelectList(index))
        }));
    let matrix = widget::responsive(move |size| {
        let mut content = widget::column::with_capacity(5).spacing(spacing.space_m);
        if two_columns(size.width) {
            for pair in Quadrant::ALL.chunks(2) {
                content = content.push(
                    widget::row::with_capacity(2)
                        .spacing(spacing.space_m)
                        .push(quadrant(store, list, Some(pair[0]), can_write))
                        .push(quadrant(store, list, Some(pair[1]), can_write)),
                );
            }
        } else {
            for value in Quadrant::ALL {
                content = content.push(quadrant(store, list, Some(value), can_write));
            }
        }
        content = content.push(quadrant(store, list, None, can_write));
        widget::scrollable(content).height(Length::Fill).into()
    });
    widget::column::with_capacity(2)
        .spacing(spacing.space_m)
        .push(selector)
        .push(matrix)
        .height(Length::Fill)
        .into()
}
fn two_columns(width: f32) -> bool {
    width >= 720.0
}

fn quadrant<'a>(
    store: &'a Store,
    list: Option<&str>,
    quadrant: Option<Quadrant>,
    can_write: bool,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let label = quadrant.map_or("Unassigned", Quadrant::label);
    let header = widget::row::with_capacity(2)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(widget::text::heading(label).width(Length::Fill))
        .push(
            widget::button::icon(icon::from_name("list-add-symbolic"))
                .tooltip(format!("Add task to {label}"))
                .on_press_maybe(can_write.then_some(Message::Matrix(Action::Add(quadrant)))),
        );
    let mut content = widget::column::with_capacity(6)
        .spacing(spacing.space_s)
        .push(header);
    let tasks = matrix::ordered(store.tasks(), list, quadrant);
    if tasks.is_empty() {
        content = content.push(widget::text::body(if quadrant.is_some() {
            "No tasks in this quadrant."
        } else {
            "No unassigned tasks."
        }));
    }
    let mut completed = false;
    for task in tasks {
        if task.completed && !completed {
            content = content.push(widget::text::caption("Completed"));
            completed = true;
        }
        content = content.push(task_row(store, task, can_write));
    }
    widget::container(content)
        .class(cosmic::theme::Container::Card)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .into()
}

fn task_row<'a>(store: &'a Store, task: &'a Task, can_write: bool) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let id = task.id.clone();
    let check = widget::checkbox(task.completed)
        .on_toggle_maybe(can_write.then_some(move |_| Message::Toggle(id.clone())));
    let title_color = if task.completed {
        let active = cosmic::theme::active();
        let color: cosmic::iced::Color = active.cosmic().on_bg_color().into();
        Some(cosmic::iced::Color {
            a: color.a * 0.6,
            ..color
        })
    } else {
        None
    };
    let title = cosmic::iced::widget::text::Rich::<
        (),
        Message,
        cosmic::Theme,
        cosmic::Renderer,
    >::with_spans(vec![
        cosmic::iced::widget::text::Span::new(task.title.as_str())
            .strikethrough(task.completed)
            .color_maybe(title_color),
    ])
    .size(14)
    .font(cosmic::font::default())
    .width(Length::Fill);
    let header = widget::row::with_capacity(3)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(check)
        .push(title)
        .push(
            widget::button::icon(icon::from_name("document-edit-symbolic"))
                .tooltip("Edit task")
                .on_press_maybe(can_write.then(|| Message::Edit(task.id.clone()))),
        );
    let mut content = widget::column::with_capacity(3)
        .spacing(spacing.space_xxs)
        .push(header);
    let mut controls = widget::row::with_capacity(3)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center);
    if can_write {
        let id = task.id.clone();
        let options: Vec<_> = std::iter::once("Unassigned")
            .chain(Quadrant::ALL.map(Quadrant::label))
            .collect();
        let selected = task
            .matrix_quadrant
            .and_then(|quadrant| Quadrant::ALL.iter().position(|q| *q == quadrant))
            .map_or(0, |i| i + 1);
        controls = controls.push(
            widget::dropdown(options, Some(selected), move |index| {
                Message::Matrix(Action::Classify(
                    id.clone(),
                    index
                        .checked_sub(1)
                        .and_then(|i| Quadrant::ALL.get(i))
                        .copied(),
                ))
            })
            .width(Length::Fill),
        );
    } else {
        controls = controls.push(
            widget::text::caption(task.matrix_quadrant.map_or("Unassigned", Quadrant::label))
                .width(Length::Fill),
        );
    }
    if task.matrix_quadrant.is_some() {
        for (direction, icon_name, label) in [
            (Direction::Up, "go-up-symbolic", "Move task up"),
            (Direction::Down, "go-down-symbolic", "Move task down"),
        ] {
            controls = controls.push(
                widget::button::icon(icon::from_name(icon_name))
                    .tooltip(label)
                    .on_press_maybe(
                        (can_write
                            && matrix::neighbor(store.tasks(), &task.id, direction).is_some())
                        .then(|| Message::Matrix(Action::Reorder(task.id.clone(), direction))),
                    ),
            );
        }
    }
    content = content.push(controls);
    widget::container(content).padding(spacing.space_xxs).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrow_content_stacks_quadrants_in_row_major_order() {
        assert!(!two_columns(360.));
        assert!(!two_columns(719.));
        assert!(two_columns(720.));
        assert_eq!(
            Quadrant::ALL.map(Quadrant::label),
            [
                "Important & Urgent",
                "Important & Not Urgent",
                "Not Important & Urgent",
                "Not Important & Not Urgent"
            ]
        );
    }
}
