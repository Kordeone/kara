// SPDX-License-Identifier: MPL-2.0
//! Matrix-specific widgets; Tasks navigation and dialogs stay in the existing app.
use crate::{
    app::{Message, edit_task_icon},
    matrix::{self, Direction},
    storage::Store,
    tasks::{Priority, Quadrant, Task},
};
use cosmic::{
    Element,
    iced::{Alignment, Length},
    widget::{self, icon},
};

#[derive(Debug, Clone)]
pub enum Action {
    SelectList(usize),
    ToggleUnassigned,
    Add(Option<Quadrant>),
    Classify(String, Option<Quadrant>),
    Reorder(String, Direction),
}

// Minimum readable panel dimensions are fallbacks, not desktop quadrant sizes.
// The responsive widget supplies the actual remaining viewport after the toolbar.
const MIN_PANEL_HEIGHT: f32 = 200.0;
const MIN_COLUMN_WIDTH: f32 = 420.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    columns: bool,
    scroll_grid: bool,
}
impl Layout {
    fn for_viewport(width: f32, height: f32, gap: f32) -> Self {
        let columns = width >= MIN_COLUMN_WIDTH * 2.0 + gap;
        Self {
            columns,
            scroll_grid: !columns || height < MIN_PANEL_HEIGHT * 2.0 + gap,
        }
    }
}

fn unassigned_height(viewport_height: f32, task_count: usize) -> f32 {
    if task_count == 0 {
        0.0
    } else {
        (viewport_height * 0.25).clamp(0.0, 240.0)
    }
}

pub fn view<'a>(
    store: Option<&'a Store>,
    list: Option<&'a str>,
    can_write: bool,
    unassigned_expanded: bool,
) -> Element<'a, Message> {
    let Some(store) = store else {
        return widget::column::with_capacity(2)
            .spacing(cosmic::theme::spacing().space_m)
            .push(widget::text::title1("Eisenhower Matrix"))
            .push(widget::text::body(
                "Matrix tasks are unavailable. Resolve the storage error and retry.",
            ))
            .into();
    };
    let spacing = cosmic::theme::spacing();
    let mut options = vec!["Inbox".to_string()];
    options.extend(store.lists().iter().map(|list| list.name.clone()));
    let selected = list
        .and_then(|id| store.lists().iter().position(|list| list.id == id))
        .map_or(0, |i| i + 1);
    // A single responsive root lets the toolbar stack at narrow widths, while
    // the nested responsive body receives the space left after its real height.
    widget::responsive(move |size| {
        let selector = widget::column::with_capacity(2)
            .spacing(spacing.space_xxs)
            .push(widget::text::caption("Task list"))
            .push(
                widget::dropdown(options.clone(), Some(selected), |index| {
                    Message::Matrix(Action::SelectList(index))
                })
                .width(Length::Fill),
            );
        let selector = widget::container(selector)
            .max_width(280)
            .width(Length::Fill);
        let heading = widget::text::title1("Eisenhower Matrix");
        let toolbar: Element<'_, Message> = if size.width >= 720.0 {
            widget::row::with_capacity(2)
                .spacing(spacing.space_m)
                .align_y(Alignment::Center)
                .push(heading.width(Length::Fill))
                .push(selector)
                .into()
        } else {
            widget::column::with_capacity(2)
                .spacing(spacing.space_s)
                .push(heading)
                .push(selector)
                .into()
        };
        let body = widget::responsive(move |viewport| {
            let unassigned =
                unassigned(store, list, can_write, unassigned_expanded, viewport.height);
            // This responsive region receives the actual space remaining after
            // the disclosure and optional Unassigned scroller have been measured.
            let grid = widget::responsive(move |available| {
                let layout = Layout::for_viewport(
                    available.width,
                    available.height,
                    f32::from(spacing.space_m),
                );
                let panel_height = if layout.scroll_grid {
                    Length::Fixed(MIN_PANEL_HEIGHT)
                } else {
                    Length::Fill
                };
                let mut grid = widget::column::with_capacity(4)
                    .spacing(spacing.space_m)
                    .width(Length::Fill);
                if layout.columns {
                    for pair in Quadrant::ALL.chunks(2) {
                        grid = grid.push(
                            widget::row::with_capacity(2)
                                .spacing(spacing.space_m)
                                .height(panel_height)
                                .width(Length::Fill)
                                .push(quadrant(store, list, pair[0], can_write))
                                .push(quadrant(store, list, pair[1], can_write)),
                        );
                    }
                } else {
                    for value in Quadrant::ALL {
                        grid = grid.push(
                            widget::container(quadrant(store, list, value, can_write))
                                .height(panel_height),
                        );
                    }
                }
                if layout.scroll_grid {
                    widget::scrollable(grid)
                        .height(Length::Fill)
                        .width(Length::Fill)
                        .into()
                } else {
                    grid.height(Length::Fill).into()
                }
            });
            widget::column::with_capacity(2)
                .spacing(spacing.space_m)
                .push(grid)
                .push(unassigned)
                .height(Length::Fill)
                .into()
        });
        widget::column::with_capacity(2)
            .spacing(spacing.space_m)
            .push(toolbar)
            .push(body)
            .height(Length::Fill)
            .into()
    })
    .into()
}

fn task_content<'a>(
    store: &'a Store,
    list: Option<&str>,
    quadrant: Option<Quadrant>,
    can_write: bool,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut content = widget::column::with_capacity(6)
        .spacing(spacing.space_s)
        .width(Length::Fill);
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
        .padding(spacing.space_s)
        .width(Length::Fill)
        .into()
}

fn add_button(quadrant: Option<Quadrant>, can_write: bool) -> Element<'static, Message> {
    widget::button::icon(icon::from_name("list-add-symbolic"))
        .tooltip(format!(
            "Add task to {}",
            quadrant.map_or("Unassigned", Quadrant::label)
        ))
        .name("Add task")
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .on_press_maybe(can_write.then_some(Message::Matrix(Action::Add(quadrant))))
        .into()
}

fn quadrant<'a>(
    store: &'a Store,
    list: Option<&str>,
    quadrant: Quadrant,
    can_write: bool,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let header = widget::row::with_capacity(2)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(widget::text::heading(quadrant.label()).width(Length::Fill))
        .push(add_button(Some(quadrant), can_write));
    let header = widget::container(header)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(cosmic::theme::Container::Primary);
    let content = widget::column::with_capacity(2)
        .push(header)
        .push(
            widget::scrollable(task_content(store, list, Some(quadrant), can_write))
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .height(Length::Fill);
    widget::container(content)
        .class(cosmic::theme::Container::Card)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn unassigned<'a>(
    store: &'a Store,
    list: Option<&str>,
    can_write: bool,
    expanded: bool,
    viewport_height: f32,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let count = matrix::ordered(store.tasks(), list, None).len();
    let header = widget::row::with_capacity(3)
        .align_y(Alignment::Center)
        .spacing(spacing.space_s)
        .push(
            widget::button::icon(icon::from_name(if expanded {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            }))
            .tooltip(if expanded {
                "Collapse Unassigned"
            } else {
                "Expand Unassigned"
            })
            .on_press(Message::Matrix(Action::ToggleUnassigned)),
        )
        .push(
            widget::button::text(format!("Unassigned ({count})"))
                .on_press(Message::Matrix(Action::ToggleUnassigned))
                .width(Length::Fill),
        )
        .push(add_button(None, can_write));
    let mut content = widget::column::with_capacity(2).push(header);
    if expanded {
        let tasks = task_content(store, list, None, can_write);
        if count == 0 {
            content = content.push(tasks);
        } else {
            content = content.push(
                widget::scrollable(tasks)
                    .height(unassigned_height(viewport_height, count))
                    .width(Length::Fill),
            );
        }
    }
    widget::container(content)
        .padding(spacing.space_xxs)
        .class(cosmic::theme::Container::Card)
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
    let mut header = widget::row::with_capacity(5)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(check)
        .push(title)
        .push(
            widget::button::icon(edit_task_icon())
                .width(Length::Fixed(36.0))
                .height(Length::Fixed(36.0))
                .name("Edit task")
                .tooltip("Edit task")
                .on_press_maybe(can_write.then(|| Message::Edit(task.id.clone()))),
        );

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
            header = header.push(
                widget::button::icon(icon::from_name(icon_name))
                    .width(Length::Fixed(36.0))
                    .height(Length::Fixed(36.0))
                    .name(label)
                    .tooltip(label)
                    .on_press_maybe(
                        (can_write
                            && matrix::neighbor(store.tasks(), &task.id, direction).is_some())
                        .then(|| Message::Matrix(Action::Reorder(task.id.clone(), direction))),
                    ),
            );
        }
    }
    let mut content = widget::column::with_capacity(3)
        .spacing(spacing.space_xxs)
        .push(header);
    if let Some(metadata) = metadata(task) {
        content = content.push(widget::text::caption(metadata));
    }
    content = content.push(controls);
    widget::container(content)
        .padding(spacing.space_xxs)
        .max_width(640)
        .width(Length::Fill)
        .into()
}

fn metadata(task: &Task) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(due) = &task.due_date {
        parts.push(format!("Due {due}"));
    }
    if task.priority != Priority::Normal {
        parts.push(format!("{} priority", task.priority.label()));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_grid_fills_viewport_and_short_or_narrow_views_scroll() {
        let wide = Layout::for_viewport(1100., 800., 16.);
        assert!(wide.columns);
        assert!(!wide.scroll_grid);
        let narrow = Layout::for_viewport(600., 800., 16.);
        assert!(!narrow.columns);
        assert!(narrow.scroll_grid);
        let short = Layout::for_viewport(1100., 300., 16.);
        assert!(short.columns);
        assert!(short.scroll_grid);
        // Two equal flex rows need exactly two readable panels plus the gap.
        assert!(!Layout::for_viewport(1100., 416., 16.).scroll_grid);
        assert!(Layout::for_viewport(1100., 415., 16.).scroll_grid);
    }
    #[test]
    fn unassigned_is_bounded_by_the_actual_viewport() {
        for height in [0., 100., 400., 800., 2000.] {
            let expanded = unassigned_height(height, 10);
            assert!(expanded <= height * 0.25);
            assert!((0. ..=240.).contains(&expanded));
        }
        assert_eq!(unassigned_height(400., 1), 100.);
        assert_eq!(unassigned_height(2000., 1), 240.);
        assert_eq!(unassigned_height(2000., 0), 0.);
    }
    #[test]
    fn column_breakpoint_accounts_for_native_spacing_and_retains_label_order() {
        assert!(!Layout::for_viewport(850., 900., 16.).columns);
        assert!(Layout::for_viewport(856., 900., 16.).columns);
        assert!(!Layout::for_viewport(856., 900., 24.).columns);
        assert_eq!(
            Quadrant::ALL.map(Quadrant::label),
            [
                "Important & Urgent",
                "Important & Not Urgent",
                "Not Important & Urgent",
                "Not Important & Not Urgent",
            ]
        );
    }
    #[test]
    fn secondary_metadata_is_optional_and_does_not_change_task_fields() {
        let mut task = Task::new("Title").unwrap();
        assert_eq!(metadata(&task), None);
        task.due_date = Some("2026-10-18".into());
        task.priority = Priority::High;
        let original = task.clone();
        assert_eq!(
            metadata(&task).as_deref(),
            Some("Due 2026-10-18 · High priority")
        );
        assert_eq!(task, original);
    }
}
