// SPDX-License-Identifier: MPL-2.0
//! Native month grid and selected-day tasks; edits use the shared task dialog.
use crate::{
    app::Message,
    calendar_month::{self, Filter, State},
    storage::Store,
    tasks::Task,
};
use cosmic::{
    Element,
    iced::{Alignment, Length},
    widget::{self, icon},
};
use jiff::civil::Date;

#[derive(Debug, Clone)]
pub enum Action {
    Previous,
    Next,
    Today,
    Select(Date),
    Add,
}
const PREVIEW_LIMIT: usize = 2;
const MIN_DAY_HEIGHT: f32 = 90.;

pub fn view<'a>(
    store: Option<&'a Store>,
    state: &'a State,
    today: Date,
    filter: Filter<'a>,
    can_write: bool,
) -> Element<'a, Message> {
    let Some(store) = store else {
        return widget::text::body(
            "Calendar tasks are unavailable. Resolve the storage error and retry.",
        )
        .into();
    };
    let spacing = cosmic::theme::spacing();
    let scope = match filter {
        Filter::All => "All lists",
        Filter::Inbox => "Inbox",
        Filter::List(id) => store
            .lists()
            .iter()
            .find(|l| l.id == id)
            .map_or("Unavailable list", |l| l.name.as_str()),
    };
    let toolbar = widget::row::with_capacity(4)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(nav_button(
            "Previous month",
            icon::from_svg_bytes(include_bytes!("../assets/tabler/chevron-right.svg"))
                .symbolic(true),
            Action::Previous,
            state.adjacent(-1).is_some(),
            true,
        ))
        .push(widget::text::heading(state.label()).width(Length::Fill))
        .push(nav_button(
            "Next month",
            icon::from_svg_bytes(include_bytes!("../assets/tabler/chevron-right.svg"))
                .symbolic(true),
            Action::Next,
            state.adjacent(1).is_some(),
            false,
        ))
        .push(widget::button::standard("Today").on_press(Message::Month(Action::Today)));
    let body = widget::responsive(move |available| {
        let wide = available.width >= 950.;
        let cells = state.cells();
        let weeks = cells.len() / 7;
        let row_height = if wide {
            ((available.height - 28.) / weeks as f32).max(MIN_DAY_HEIGHT)
        } else {
            MIN_DAY_HEIGHT
        };
        let mut grid = widget::column::with_capacity(weeks + 1)
            .spacing(spacing.space_xxs)
            .width(Length::Fill);
        let mut weekdays = widget::row::with_capacity(7).spacing(spacing.space_xxs);
        for name in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
            weekdays = weekdays.push(
                widget::container(widget::text::caption(name))
                    .center_x(Length::Fill)
                    .width(Length::FillPortion(1)),
            );
        }
        grid = grid.push(weekdays);
        for week in cells.chunks(7) {
            let mut row = widget::row::with_capacity(7)
                .spacing(spacing.space_xxs)
                .height(row_height);
            for date in week {
                let cell: Element<'_, Message> = match date {
                    Some(date) => day_cell(store, *date, state, today, filter, row_height),
                    None => widget::container(widget::space::horizontal())
                        .width(Length::FillPortion(1))
                        .height(row_height)
                        .into(),
                };
                row = row.push(cell);
            }
            grid = grid.push(row);
        }
        let details = details(
            store,
            state,
            filter,
            can_write,
            if wide {
                available.height.max(320.)
            } else {
                300.
            },
        );
        let content: Element<'_, Message> = if wide {
            widget::row::with_capacity(2)
                .spacing(spacing.space_m)
                .push(grid)
                .push(widget::container(details).width(300))
                .into()
        } else {
            widget::column::with_capacity(2)
                .spacing(spacing.space_m)
                .push(grid)
                .push(details)
                .into()
        };
        widget::scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    });
    widget::column::with_capacity(3)
        .spacing(spacing.space_s)
        .push(toolbar)
        .push(widget::text::caption(format!(
            "{scope} · Weeks start on Monday"
        )))
        .push(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn nav_button(
    label: &'static str,
    handle: icon::Handle,
    action: Action,
    enabled: bool,
    previous: bool,
) -> Element<'static, Message> {
    // Reuse the bundled right chevron, rotated for the previous-month control.
    let glyph = widget::icon(handle)
        .size(16)
        .rotation((if previous { std::f32::consts::PI } else { 0. }).into());
    let button = widget::button::custom(
        widget::container(glyph)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(36)
    .height(36)
    .class(cosmic::theme::Button::Icon)
    .name(label)
    .on_press_maybe(enabled.then_some(Message::Month(action)));
    widget::tooltip(
        button,
        widget::text::body(label),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

fn task_title<'a>(
    task: &'a Task,
    size: u16,
) -> cosmic::iced::widget::text::Rich<'a, (), Message, cosmic::Theme, cosmic::Renderer> {
    let color: cosmic::iced::Color = cosmic::theme::active().cosmic().on_bg_color().into();
    cosmic::iced::widget::text::Rich::with_spans(vec![
        cosmic::iced::widget::text::Span::new(task.title.as_str())
            .strikethrough(task.completed)
            .color_maybe(task.completed.then_some(cosmic::iced::Color {
                a: color.a * 0.6,
                ..color
            })),
    ])
    .font(cosmic::font::default())
    .size(size)
    .width(Length::Fill)
}

fn day_cell<'a>(
    store: &'a Store,
    date: Date,
    state: &State,
    today: Date,
    filter: Filter<'_>,
    height: f32,
) -> Element<'a, Message> {
    let tasks = calendar_month::on_date(store.tasks(), date, filter);
    let count = tasks.len();
    let selected = date == state.selected;
    let current = date == today;
    let mut content = widget::column::with_capacity(4)
        .spacing(2)
        .width(Length::Fill)
        .push(
            // A short day label remains readable in narrow seven-column grids;
            // today's border and accessible name carry the extra state.
            widget::text::body(date.day().to_string()).height(20),
        );
    for task in tasks.into_iter().take(PREVIEW_LIMIT) {
        content = content.push(
            widget::row::with_capacity(2)
                .spacing(2)
                .align_y(Alignment::Center)
                .push(crate::priority_style::flag(task.priority))
                .push(
                    task_title(task, 12)
                        .height(16)
                        .wrapping(cosmic::iced::widget::text::Wrapping::None),
                ),
        );
    }
    if count > PREVIEW_LIMIT {
        content = content.push(widget::text::caption(format!(
            "+{} more",
            count - PREVIEW_LIMIT
        )));
    }
    let button = widget::button::custom(content)
        .padding(4)
        .width(Length::FillPortion(1))
        .height(height)
        .class(day_style(
            selected,
            current,
            date.month() == state.month.month(),
        ))
        .selected(selected)
        .name(format!(
            "{date}, {count} tasks{}{}",
            if current { ", today" } else { "" },
            if selected { ", selected" } else { "" }
        ))
        .on_press(Message::Month(Action::Select(date)));
    button.into()
}

fn day_style(selected: bool, today: bool, in_month: bool) -> cosmic::theme::Button {
    let style = move |focused, theme: &cosmic::Theme, state| {
        use widget::button::Catalog;
        let class = cosmic::theme::Button::Standard;
        let mut result = match state {
            0 => theme.active(focused, false, &class),
            1 => theme.hovered(focused, false, &class),
            _ => theme.pressed(focused, false, &class),
        };
        result.border_radius = theme.cosmic().radius_s().into();
        if selected {
            result.background = Some(theme.cosmic().primary_component_color().into());
            result.border_width = 2.;
            result.border_color = theme.cosmic().accent_color().into();
        } else if today {
            result.border_width = 1.;
            result.border_color = theme.cosmic().accent_color().into();
        }
        if !in_month {
            let mut color: cosmic::iced::Color = theme.cosmic().on_bg_color().into();
            color.a *= 0.65;
            result.text_color = Some(color);
        }
        result
    };
    cosmic::theme::Button::Custom {
        active: Box::new(move |f, t| style(f, t, 0)),
        hovered: Box::new(move |f, t| style(f, t, 1)),
        pressed: Box::new(move |f, t| style(f, t, 2)),
        disabled: Box::new(move |t| style(false, t, 0)),
    }
}

fn details<'a>(
    store: &'a Store,
    state: &State,
    filter: Filter<'_>,
    can_write: bool,
    height: f32,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let tasks = calendar_month::on_date(store.tasks(), state.selected, filter);
    let header = widget::row::with_capacity(2)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(widget::text::heading(state.selected.to_string()).width(Length::Fill))
        .push(
            widget::button::standard("Add task")
                .on_press_maybe(can_write.then_some(Message::Month(Action::Add))),
        );
    let mut rows = widget::column::with_capacity(tasks.len().max(1))
        .spacing(spacing.space_s)
        .width(Length::Fill);
    if tasks.is_empty() {
        rows = rows.push(widget::text::body(
            "No tasks on this date in the selected list filter.",
        ));
    }
    for task in tasks {
        let id = task.id.clone();
        let list = task
            .list_id
            .as_deref()
            .and_then(|id| store.lists().iter().find(|l| l.id == id))
            .map_or("Inbox", |l| l.name.as_str());
        let row = widget::row::with_capacity(4)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .push(
                widget::checkbox(task.completed)
                    .on_toggle_maybe(can_write.then_some(move |_| Message::Toggle(id.clone()))),
            )
            .push(crate::priority_style::marker(task.priority))
            .push(
                widget::column::with_capacity(2)
                    .push(task_title(task, 14))
                    .push(widget::text::caption(list))
                    .width(Length::Fill),
            )
            .push(
                widget::button::icon(crate::app::edit_task_icon())
                    .name("Edit task")
                    .tooltip("Edit task")
                    .on_press_maybe(can_write.then(|| Message::Edit(task.id.clone()))),
            );
        rows = rows.push(
            widget::container(row)
                .padding(spacing.space_xxs)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        );
    }
    widget::container(
        widget::column::with_capacity(2)
            .spacing(spacing.space_s)
            .push(header)
            .push(
                widget::scrollable(rows)
                    .height(Length::Fill)
                    .width(Length::Fill),
            ),
    )
    .padding(spacing.space_s)
    .height(height)
    .width(Length::Fill)
    .class(cosmic::theme::Container::Card)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn populated_native_calendar_keeps_seven_cells_and_scrolls_in_small_windows() {
        use cosmic::iced::{
            Size,
            advanced::{
                layout::{Limits, Node},
                renderer::Headless,
                widget::Tree,
            },
        };
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("tasks.json")).unwrap();
        let date: Date = "2026-03-15".parse().unwrap();
        let state = State::new(date);
        for i in 0..40 {
            let mut task = calendar_month::draft(date, Filter::Inbox).unwrap();
            task.title = format!("A long calendar task title {i}");
            task.completed = i % 3 == 0;
            store.upsert(task).unwrap();
        }
        let renderer = cosmic::Renderer::new(cosmic::font::default(), 14.into(), Some("tiny-skia"))
            .await
            .unwrap();
        fn rows(node: &Node, count: &mut usize) {
            assert!(node.size().width.is_finite() && node.size().height.is_finite());
            if node.children().len() == 7
                && node.size().height >= MIN_DAY_HEIGHT
                && node.children().iter().all(|cell| {
                    (cell.size().height - node.size().height).abs() < 0.01
                        && cell.bounds().y.abs() < 0.01
                })
            {
                *count += 1;
                let cells = node.children();
                let width = cells[0].size().width;
                for cell in cells {
                    assert!((cell.size().width - width).abs() < 0.01);
                }
                let last = cells.last().unwrap();
                assert!((last.bounds().x + last.size().width - node.size().width).abs() < 0.01);
            }
            for child in node.children() {
                rows(child, count);
            }
        }
        for (width, height) in [(400., 350.), (680., 600.), (1200., 700.)] {
            let mut view = view(Some(&store), &state, date, Filter::All, true);
            let mut tree = Tree::new(&view);
            let layout = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &Limits::new(Size::ZERO, Size::new(width, height)),
            );
            assert_eq!(layout.size(), Size::new(width, height));
            let mut count = 0;
            rows(&layout, &mut count);
            assert_eq!(count, state.cells().len() / 7);
        }
        assert_eq!(
            calendar_month::on_date(store.tasks(), date, Filter::All).len(),
            40
        );
    }
}
