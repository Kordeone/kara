// SPDX-License-Identifier: MPL-2.0
//! Matrix-specific widgets; Tasks navigation and dialogs stay in the existing app.
use crate::{
    app::{Message, edit_task_icon, priority_button},
    matrix::Placement,
    matrix::{self, Direction},
    matrix_drag::{self, Payload},
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
    OpenPriority(String),
    ClosePriority,
    Add(Quadrant),
    Classify(String, Quadrant),
    Reorder(String, Direction),
    DragStart(Payload, Task),
    DragHover(Placement),
    DragOffered(Placement),
    DragLeave(Placement),
    DragCancel,
    DragEnd,
    Drop(
        Placement,
        String,
        Vec<u8>,
        cosmic::iced::clipboard::dnd::DndAction,
    ),
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

pub fn view<'a>(
    store: Option<&'a Store>,
    list: Option<&'a str>,
    can_write: bool,
    priority_task: Option<&'a str>,
    drag: &'a matrix_drag::State,
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
    let body = widget::responsive(move |available| {
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
                        .push(quadrant(
                            store,
                            list,
                            pair[0],
                            can_write,
                            priority_task,
                            drag,
                        ))
                        .push(quadrant(
                            store,
                            list,
                            pair[1],
                            can_write,
                            priority_task,
                            drag,
                        )),
                );
            }
        } else {
            for value in Quadrant::ALL {
                grid = grid.push(
                    widget::container(quadrant(store, list, value, can_write, priority_task, drag))
                        .height(panel_height),
                );
            }
        }
        if layout.scroll_grid {
            crate::matrix_clip::clip(
                widget::scrollable(grid)
                    .height(Length::Fill)
                    .width(Length::Fill),
            )
        } else {
            grid.height(Length::Fill).into()
        }
    });
    widget::column::with_capacity(2)
        .spacing(spacing.space_m)
        .push(widget::text::title1("Eisenhower Matrix"))
        .push(body)
        .height(Length::Fill)
        .into()
}

fn task_content<'a>(
    store: &'a Store,
    list: Option<&str>,
    quadrant: Quadrant,
    can_write: bool,
    priority_task: Option<&'a str>,
    drag: &'a matrix_drag::State,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut content = widget::column::with_capacity(6)
        .spacing(spacing.space_s)
        .width(Length::Fill);
    let tasks = matrix::ordered(store.tasks(), list, quadrant);
    let empty = tasks.is_empty();
    if empty {
        // The first task will replace the empty state at the top, not below it.
        content = content.push(drop_line(
            drag.hover.as_ref() == Some(&placement(list, quadrant, None)),
        ));
        content = content.push(widget::text::body("No tasks in this quadrant."));
    }
    let mut completed = false;
    for task in tasks {
        if task.completed && !completed {
            let end = placement(list, quadrant, None);
            content = content.push(drop_line(
                drag.hover.as_ref() == Some(&end) && drag.source().is_some_and(|t| !t.completed),
            ));
            content = content.push(widget::text::caption("Completed"));
            completed = true;
        }
        let target = placement(list, quadrant, Some(&task.id));
        let row = widget::column::with_capacity(2)
            .push(drop_line(drag.hover.as_ref() == Some(&target)))
            .push(task_row(store, task, can_write, priority_task, drag));
        content = content.push(drop_zone(row.into(), target, can_write, drag));
    }
    let end = placement(list, quadrant, None);
    if !empty {
        content = content.push(drop_line(
            drag.hover.as_ref() == Some(&end)
                && (drag.source().is_some_and(|t| t.completed) || !completed),
        ));
    }
    widget::container(content)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .into()
}

fn add_button(quadrant: Quadrant, can_write: bool) -> Element<'static, Message> {
    widget::button::icon(icon::from_name("list-add-symbolic"))
        .tooltip(format!("Add task to {}", quadrant.label()))
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
    priority_task: Option<&'a str>,
    drag: &'a matrix_drag::State,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let header = widget::row::with_capacity(3)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(crate::priority_style::marker(quadrant.priority()))
        .push(widget::text::heading(quadrant.label()).width(Length::Fill))
        .push(add_button(quadrant, can_write));
    let header = widget::container(header)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(cosmic::theme::Container::Primary);
    let content = widget::column::with_capacity(2)
        .push(header)
        .push(crate::matrix_clip::clip(
            widget::scrollable(task_content(
                store,
                list,
                quadrant,
                can_write,
                priority_task,
                drag,
            ))
            .height(Length::Fill)
            .width(Length::Fill),
        ))
        .height(Length::Fill);
    let end = placement(list, quadrant, None);
    let highlighted = hovered_quadrant(drag, list, quadrant);
    let panel: Element<'a, Message> = widget::container(content)
        .class(feedback_style(
            if highlighted {
                Feedback::Destination
            } else {
                Feedback::None
            },
            true,
        ))
        .height(Length::Fill)
        .width(Length::Fill)
        .into();
    drop_zone(panel, end, can_write, drag)
}

fn task_row<'a>(
    store: &'a Store,
    task: &'a Task,
    can_write: bool,
    priority_task: Option<&str>,
    drag: &'a matrix_drag::State,
) -> Element<'a, Message> {
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
    let priority_button = widget::button::custom(crate::priority_style::flag(task.priority))
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .class(cosmic::theme::Button::Icon)
        .name(format!("Change priority: {}", task.priority.label()))
        .on_press_maybe(can_write.then(|| Message::Matrix(Action::OpenPriority(task.id.clone()))));
    let priority_button = widget::tooltip(
        priority_button,
        widget::text::body(format!("Change priority: {}", task.priority.label())),
        widget::tooltip::Position::Top,
    );
    let popover = widget::popover(priority_button)
        .position(widget::popover::Position::Bottom)
        .on_close(Message::Matrix(Action::ClosePriority));
    let priority_control = if can_write && priority_task == Some(task.id.as_str()) {
        popover.popup(priority_choices(task))
    } else {
        popover
    };
    let mut actions = widget::row::with_capacity(3)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .push(
            widget::button::icon(edit_task_icon())
                .width(Length::Fixed(36.0))
                .height(Length::Fixed(36.0))
                .name("Edit task")
                .tooltip("Edit task")
                .on_press_maybe(can_write.then(|| Message::Edit(task.id.clone()))),
        );
    let header = widget::row::with_capacity(6)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .push(drag_handle(task, can_write, drag))
        .push(check)
        .push(priority_control)
        .push(title);

    for (direction, icon_name, label) in [
        (Direction::Up, "go-up-symbolic", "Move task up"),
        (Direction::Down, "go-down-symbolic", "Move task down"),
    ] {
        actions = actions.push(
            widget::button::icon(icon::from_name(icon_name))
                .width(Length::Fixed(36.0))
                .height(Length::Fixed(36.0))
                .name(label)
                .tooltip(label)
                .on_press_maybe(
                    (can_write && matrix::neighbor(store.tasks(), &task.id, direction).is_some())
                        .then(|| Message::Matrix(Action::Reorder(task.id.clone(), direction))),
                ),
        );
    }
    let header = header.push(actions).width(Length::Fill);
    let mut content = widget::column::with_capacity(2)
        .spacing(spacing.space_xxs)
        .push(header);
    if let Some(metadata) = metadata(task) {
        content = content.push(widget::text::caption(metadata));
    }
    widget::container(content)
        .padding(spacing.space_xxs)
        .class(feedback_style(
            if drag.source().is_some_and(|source| source.id == task.id) {
                Feedback::Source
            } else {
                Feedback::None
            },
            false,
        ))
        .width(Length::Fill)
        .into()
}

fn placement(list: Option<&str>, quadrant: Quadrant, before: Option<&str>) -> Placement {
    Placement {
        list_id: list.map(str::to_string),
        quadrant,
        before: before.map(str::to_string),
    }
}

fn drop_zone<'a>(
    content: Element<'a, Message>,
    target: Placement,
    enabled: bool,
    drag: &matrix_drag::State,
) -> Element<'a, Message> {
    use cosmic::iced::clipboard::dnd::DndAction;
    if !enabled {
        return content;
    }
    let enter = target.clone();
    let leave = target.clone();
    let offered = target.clone();
    let id = drag.widget_id(format!("matrix-drop:{target:?}"));
    widget::DndDestination::with_id(content, id, vec![matrix_drag::MIME.into()])
        .action(DndAction::Move)
        .preferred_action(DndAction::Move)
        .on_enter(move |_, _, _| Message::Matrix(Action::DragHover(enter.clone())))
        .on_leave(move || Message::Matrix(Action::DragLeave(leave.clone())))
        .on_drop(move |_, _| Message::Matrix(Action::DragOffered(offered.clone())))
        .on_finish(move |mime, bytes, action, _, _| {
            Message::Matrix(Action::Drop(target.clone(), mime, bytes, action))
        })
        .into()
}

fn drag_handle(task: &Task, enabled: bool, drag: &matrix_drag::State) -> Element<'static, Message> {
    let handle = widget::mouse_area(
        widget::container(widget::icon(icon::from_name("grip-lines-symbolic").into()).size(16))
            .padding(4),
    )
    .interaction(cosmic::iced::mouse::Interaction::Grab);
    let handle = widget::tooltip(
        handle,
        widget::text::body("Drag task; use the flag or arrows for keyboard movement"),
        widget::tooltip::Position::Top,
    );
    if !enabled {
        return handle.into();
    }
    let payload = Payload::new(&task.id);
    let start = Message::Matrix(Action::DragStart(payload.clone(), task.clone()));
    widget::DndSource::with_id(handle, drag.widget_id(format!("matrix-handle:{}", task.id)))
        .action(cosmic::iced::clipboard::dnd::DndAction::Move)
        .drag_content(move || payload.clone())
        .drag_icon(|offset| {
            // The locked source constrains its icon to the grip's 24px bounds.
            // A neutral task badge fits; a title/card preview would be clipped.
            let preview = drag_preview();
            let state = preview.as_widget().state();
            (preview, state, offset)
        })
        .on_start(Some(start))
        .on_cancel(Some(Message::Matrix(Action::DragCancel)))
        .on_finish(Some(Message::Matrix(Action::DragEnd)))
        .into()
}

// Keep the same slot even while idle: hover must not move drop hit boxes.
const INSERTION_HEIGHT: f32 = 8.0;
fn drop_line(active: bool) -> Element<'static, Message> {
    let cap = || {
        widget::container(widget::space::horizontal())
            .width(INSERTION_HEIGHT)
            .height(INSERTION_HEIGHT)
            .class(insertion_style(active))
    };
    widget::row::with_capacity(3)
        .align_y(Alignment::Center)
        .push(cap())
        .push(
            widget::container(widget::space::horizontal())
                .height(4)
                .width(Length::Fill)
                .class(insertion_style(active)),
        )
        .push(cap())
        .height(INSERTION_HEIGHT)
        .width(Length::Fill)
        .into()
}
fn insertion_style(active: bool) -> cosmic::theme::Container<'static> {
    cosmic::theme::Container::custom(move |theme| cosmic::widget::container::Style {
        background: active.then(|| theme.cosmic().accent_color().into()),
        ..Default::default()
    })
}

fn hovered_quadrant(drag: &matrix_drag::State, list: Option<&str>, quadrant: Quadrant) -> bool {
    // A before-row target belongs to the panel just as its append target does.
    drag.source().is_some()
        && drag
            .hover
            .as_ref()
            .is_some_and(|target| target.list_id.as_deref() == list && target.quadrant == quadrant)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feedback {
    None,
    Source,
    Destination,
}
fn feedback_style(feedback: Feedback, card: bool) -> cosmic::theme::Container<'static> {
    cosmic::theme::Container::custom(move |theme| feedback_appearance(theme, feedback, card))
}
fn feedback_appearance(
    theme: &cosmic::Theme,
    feedback: Feedback,
    card: bool,
) -> cosmic::widget::container::Style {
    use cosmic::widget::container::Catalog;
    let mut style = theme.style(&if card {
        cosmic::theme::Container::Card
    } else {
        cosmic::theme::Container::Transparent
    });
    if feedback != Feedback::None {
        let accent: cosmic::iced::Color = theme.cosmic().accent_color().into();
        let base: cosmic::iced::Color = theme.cosmic().primary_container_color().into();
        let amount = if feedback == Feedback::Source {
            0.18
        } else {
            0.07
        };
        // An opaque blend keeps text readable; never fade the completed title
        // or tint an entire quadrant with the task's semantic priority color.
        style.background = Some(
            cosmic::iced::Color::from_rgb(
                base.r + (accent.r - base.r) * amount,
                base.g + (accent.g - base.g) * amount,
                base.b + (accent.b - base.b) * amount,
            )
            .into(),
        );
        style.border.width = 2.;
        style.border.color = accent;
    }
    style
}

fn drag_preview_icon() -> icon::Icon {
    // Bundled symbolic task-list silhouette; no source or destination priority.
    widget::icon(
        icon::from_svg_bytes(include_bytes!(
            "../resources/icons/hicolor/scalable/actions/module-tasks-symbolic.svg"
        ))
        .symbolic(true),
    )
    .size(16)
}

fn drag_preview() -> Element<'static, ()> {
    // No second task record or interactive content in the compositor drag icon.
    widget::container(drag_preview_icon())
        .padding(4)
        .class(feedback_style(Feedback::Source, true))
        .into()
}

/// The same native selected flag buttons as the task editor; labels only in
/// tooltips/accessibility names, never repeated beneath the Matrix task title.
fn priority_choices(task: &Task) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut choices = widget::row::with_capacity(4).spacing(spacing.space_xxs);
    for priority in Priority::ALL {
        choices = choices.push(priority_button(
            task.priority,
            priority,
            classification_action(&task.id, priority),
        ));
    }
    widget::container(choices)
        .padding(spacing.space_xxs)
        .class(cosmic::theme::Container::Card)
        .into()
}

fn classification_action(id: &str, priority: Priority) -> Message {
    Message::Matrix(Action::Classify(id.to_string(), priority.quadrant()))
}

fn metadata(task: &Task) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(due) = &task.due_date {
        parts.push(format!("Due {due}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_drag_badge_is_bundled_symbolic_and_not_a_priority_flag() {
        let neutral = drag_preview_icon().into_svg_handle().unwrap();
        for priority in Priority::ALL {
            assert_ne!(
                neutral.id(),
                crate::priority_style::flag(priority)
                    .into_svg_handle()
                    .unwrap()
                    .id()
            );
        }
        match neutral.data() {
            cosmic::iced::advanced::svg::Data::Bytes(bytes) => assert_eq!(
                bytes.as_ref(),
                include_bytes!(
                    "../resources/icons/hicolor/scalable/actions/module-tasks-symbolic.svg"
                )
            ),
            _ => panic!("Native drag badge must use the bundled task symbol"),
        }
    }

    #[test]
    fn destination_feedback_covers_row_and_append_targets_and_clears_on_cancel() {
        let task = Task::new("Original source").unwrap();
        let original = task.clone();
        let mut drag = matrix_drag::State::default();
        drag.start(Payload::new(&task.id), task);
        for before in [None, Some("destination-row")] {
            drag.hover = Some(placement(None, Quadrant::ImportantUrgent, before));
            assert!(hovered_quadrant(&drag, None, Quadrant::ImportantUrgent));
            assert!(!hovered_quadrant(&drag, None, Quadrant::NotImportantUrgent));
            assert!(!hovered_quadrant(
                &drag,
                Some("work"),
                Quadrant::ImportantUrgent
            ));
        }
        assert_eq!(drag.source(), Some(&original));
        drag.cancel();
        assert!(drag.source().is_none());
        assert!(drag.hover.is_none());
        assert!(!hovered_quadrant(&drag, None, Quadrant::ImportantUrgent));
        // A stale visual target alone must never highlight a destination.
        drag.hover = Some(placement(None, Quadrant::ImportantUrgent, None));
        assert!(!hovered_quadrant(&drag, None, Quadrant::ImportantUrgent));
    }

    #[test]
    fn feedback_uses_theme_surfaces_and_restores_native_style() {
        use cosmic::widget::container::Catalog;
        for theme in [
            cosmic::Theme::light(),
            cosmic::Theme::dark(),
            cosmic::Theme::light_hc(),
            cosmic::Theme::dark_hc(),
        ] {
            let normal = feedback_appearance(&theme, Feedback::None, true);
            let native = theme.style(&cosmic::theme::Container::Card);
            assert_eq!(normal.background, native.background);
            assert_eq!(normal.border, native.border);
            let source = feedback_appearance(&theme, Feedback::Source, false);
            let destination = feedback_appearance(&theme, Feedback::Destination, true);
            assert_ne!(source.background, destination.background);
            assert_ne!(destination.background, normal.background);
            assert_eq!(source.border.width, 2.);
            assert_eq!(destination.border.width, 2.);
            let accent: cosmic::iced::Color = theme.cosmic().accent_color().into();
            assert_eq!(source.border.color, accent);
            assert_eq!(destination.border.color, accent);
        }
    }

    #[test]
    fn insertion_slot_does_not_resize_when_hover_changes() {
        let idle = drop_line(false);
        let active = drop_line(true);
        assert_eq!(idle.as_widget().size(), active.as_widget().size());
        assert_eq!(
            active.as_widget().size().height,
            Length::Fixed(INSERTION_HEIGHT)
        );
        assert_eq!(active.as_widget().size().width, Length::Fill);
    }

    #[test]
    fn native_preview_state_survives_the_locked_source_container_wrapper() {
        use cosmic::iced::advanced::widget::Tree;
        for _ in Priority::ALL {
            let preview = drag_preview();
            let mut state = Tree::new(&preview);
            // DndSource wraps the returned icon in a grip-sized container.
            // Container forwards state/tag/children to its content in this lock.
            let mut wrapped: Element<'static, ()> =
                widget::container(preview).width(24).height(24).into();
            assert_eq!(state.tag, wrapped.as_widget().tag());
            wrapped.as_widget_mut().diff(&mut state);
        }
    }

    #[test]
    fn compact_flag_choices_update_the_same_task_in_each_quadrant() {
        let mut task = Task::new("Keep title").unwrap();
        task.list_id = Some("work".into());
        task.completed = true;
        task.due_date = Some("2026-10-18".into());
        let original = task.clone();
        let mut tasks = vec![task];
        for priority in Priority::ALL {
            let Message::Matrix(Action::Classify(id, quadrant)) =
                classification_action(&original.id, priority)
            else {
                panic!("Flag choices must keep the existing classification action");
            };
            matrix::classify(&mut tasks, &id, quadrant).unwrap();
            assert_eq!(tasks[0].priority, priority);
            assert_eq!(tasks[0].id, original.id);
            assert_eq!(tasks[0].list_id, original.list_id);
            assert_eq!(tasks[0].due_date, original.due_date);
            assert_eq!(tasks[0].completed, original.completed);
            assert_eq!(matrix::ordered(&tasks, Some("work"), quadrant).len(), 1);
        }
    }
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
        task.priority = crate::tasks::Priority::P3;
        let original = task.clone();
        assert_eq!(metadata(&task).as_deref(), Some("Due 2026-10-18"));
        assert_eq!(task, original);
    }
}
