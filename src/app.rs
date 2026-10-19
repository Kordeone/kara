// SPDX-License-Identifier: MPL-2.0
use crate::{
    date_entry,
    lifecycle::{Effect, Lifecycle, State},
    matrix, matrix_view,
    storage::{self, Store},
    tasks::{self, Priority, Task as KaraTask, TaskList, TaskView},
    tray,
};
use cosmic::{
    app::context_drawer,
    iced::{Alignment, Length, Subscription},
    prelude::*,
    widget::{self, about::About, icon, nav_bar},
};
use jiff::civil::{Date, Weekday};
use std::time::Duration;

const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");
const EDIT_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/edit-symbolic.svg");
const INBOX_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/inbox-tray-symbolic.svg");
const TODAY_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/calendar-check-symbolic.svg");
const PINNED_LIST_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/list-pinned-symbolic.svg");
const SORT_MODE_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/sort-mode-symbolic.svg");
const SORT_ASCENDING_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/sort-ascending-symbolic.svg");
const SORT_DESCENDING_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/sort-descending-symbolic.svg");
const SORT_SELECTED_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/actions/sort-selected-symbolic.svg");

pub struct AppModel {
    core: cosmic::Core,
    lifecycle: Lifecycle,
    tray_connection: Option<zbus::Connection>,
    tray_error: Option<String>,
    nav: nav_bar::Model,
    page: Page,
    lists_expanded: bool,
    matrix_list: Option<String>,
    matrix_priority_task: Option<String>,
    store: Option<Store>,
    storage_error: Option<String>,
    editor: Option<Editor>,
    delete_id: Option<String>,
    list_editor: Option<ListEditor>,
    delete_list_id: Option<String>,
    today: jiff::civil::Date,
    task_sort: tasks::TaskSort,
    task_sort_ascending: bool,
    show_completed: bool,
    sort_menu_open: bool,
    about: About,
    title_input: cosmic::iced::widget::Id,
}
struct Editor {
    task: KaraTask,
    due: String,
    notes: widget::text_editor::Content,
    error: Option<String>,
    title_error: Option<String>,
    due_error: Option<String>,
    calendar: cosmic::widget::calendar::CalendarModel,
    calendar_open: bool,
    is_new: bool,
}
struct ListEditor {
    id: Option<String>,
    is_new: bool,
    name: String,
    error: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Message {
    Tray(tray::Event),
    CloseMain,
    WindowCloseRequested(cosmic::iced::window::Id),
    WindowClosed(cosmic::iced::window::Id),
    WindowOpened(cosmic::iced::window::Id),
    FocusToken(cosmic::iced::window::Id, Option<String>),
    Exit,

    Matrix(matrix_view::Action),
    NavContext(NavMenuAction),
    Add,
    AddList,
    RenameList(String),
    DeleteList(String),
    ConfirmDeleteList,
    ListName(String),
    SaveList,
    OpenList(String),
    ToggleLists,
    ToggleListPin(String),
    AssignList(usize),
    Edit(String),
    Toggle(String),
    Delete(String),
    ConfirmDelete,
    Cancel,
    Title(String),
    Due(String),
    DueBlur,
    DateShortcut(i64),
    Priority(usize),
    ToggleCalendar,
    CloseCalendar,
    CalendarPrev,
    CalendarNext,
    CalendarSelect(Date),
    Notes(widget::text_editor::Action),
    Save,
    RetryStorage,
    DateTick,
    ToggleSortMenu,
    CloseSortMenu,
    SelectSortMode(usize),
    ToggleSortDirection,
    ShowCompleted(bool),
    About,
    LaunchUrl(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Tasks,
    Inbox,
    Today,
    Calendar,
    Lists,
    List(String),
    Board,
    Matrix,
    Pomodoro,
}
impl Page {
    fn title(&self) -> &'static str {
        match self {
            Self::Tasks => "Tasks",
            Self::Inbox => "Inbox",
            Self::Today => "Today",
            Self::Calendar => "Calendar",
            Self::Lists => "Lists",
            Self::List(_) => "List",
            Self::Board => "Board View",
            Self::Matrix => "Eisenhower Matrix",
            Self::Pomodoro => "Pomodoro",
        }
    }
    fn draft_task(&self) -> Result<KaraTask, String> {
        let mut task = KaraTask::new("New task")?;
        task.title.clear();
        if let Self::List(id) = self {
            task.list_id = Some(id.clone());
        }
        Ok(task)
    }
    fn view(&self) -> Option<TaskView<'_>> {
        match self {
            Self::Tasks | Self::Inbox => Some(TaskView::Inbox),
            Self::Today => Some(TaskView::Today),
            Self::List(id) => Some(TaskView::List(id)),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavMenuAction {
    ToggleListPin(nav_bar::Id),
    ToggleLists(nav_bar::Id),
}
impl widget::menu::Action for NavMenuAction {
    type Message = cosmic::Action<Message>;

    fn message(&self) -> Self::Message {
        cosmic::Action::App(Message::NavContext(*self))
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = crate::Flags;
    type Message = Message;
    const APP_ID: &'static str = storage::APP_ID;
    fn core(&self) -> &cosmic::Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }
    fn init(core: cosmic::Core, _: Self::Flags) -> (Self, Task<cosmic::Action<Message>>) {
        let nav = build_nav(&[], &Page::Inbox, true);
        let about = About::default()
            .name("Kara")
            .icon(icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .license(env!("CARGO_PKG_LICENSE"))
            .links([("Repository", env!("CARGO_PKG_REPOSITORY"))]);
        let mut app = Self {
            core,
            lifecycle: Lifecycle::default(),
            tray_connection: None,
            tray_error: None,
            nav,
            page: Page::Inbox,
            lists_expanded: true,
            matrix_list: None,
            matrix_priority_task: None,
            store: None,
            storage_error: None,
            editor: None,
            delete_id: None,
            list_editor: None,
            delete_list_id: None,
            today: jiff::Zoned::now().date(),
            task_sort: tasks::TaskSort::default(),
            task_sort_ascending: tasks::TaskSort::default().default_ascending(),
            show_completed: true,
            sort_menu_open: false,
            about,
            title_input: cosmic::iced::widget::Id::unique(),
        };
        app.load();
        let command = app.update_title();
        (app, command)
    }
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Message>> {
        self.nav.activate(id);
        if let Some(page) = self.nav.data::<Page>(id).cloned() {
            self.page = page;
            if self.page == Page::Lists {
                self.lists_expanded = !self.lists_expanded;
            }
        }
        self.today = jiff::Zoned::now().date();
        self.rebuild_nav();
        self.update_title()
    }
    fn nav_context_menu(&self) -> Option<Vec<cosmic::widget::menu::Tree<cosmic::Action<Message>>>> {
        let children = self
            .nav
            .iter()
            .map(|entity| match self.nav.data::<Page>(entity) {
                Some(Page::List(_)) => {
                    let id = entity;
                    let Some(Page::List(list_id)) = self.nav.data::<Page>(id) else {
                        return Vec::new();
                    };
                    let pinned = self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().iter().find(|list| &list.id == list_id))
                        .is_some_and(|list| list.pinned);
                    vec![widget::menu::Item::Button(
                        if pinned { "Unpin list" } else { "Pin list" },
                        Some(icon::from_name("view-pin-symbolic").into()),
                        NavMenuAction::ToggleListPin(id),
                    )]
                }
                Some(Page::Lists) => vec![widget::menu::Item::Button(
                    if self.lists_expanded {
                        "Collapse lists"
                    } else {
                        "Expand lists"
                    },
                    None,
                    NavMenuAction::ToggleLists(entity),
                )],
                _ => Vec::new(),
            })
            .collect();
        Some(widget::menu::nav_context(
            &std::collections::HashMap::new(),
            children,
        ))
    }
    fn header_end(&self) -> Vec<Element<'_, Message>> {
        vec![
            widget::button::icon(icon::from_name("dialog-information-symbolic"))
                .tooltip("About Kara")
                .on_press(Message::About)
                .into(),
        ]
    }
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Message>> {
        self.core.window.show_context.then(|| {
            context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::About,
            )
        })
    }
    fn view(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let page = self.page.clone();
        let mut content = widget::column::with_capacity(5).spacing(spacing.space_m);
        if let Some(error) = &self.tray_error {
            content = content.push(widget::text::body(format!(
                "System tray unavailable: {error}. Closing the window will quit Kara."
            )));
        }
        if let Some(error) = &self.storage_error {
            content = content.push(
                widget::settings::section()
                    .title("Task storage error")
                    .add(widget::text::body(error))
                    .add(widget::button::standard("Retry storage").on_press(Message::RetryStorage)),
            );
        }
        let mut header = widget::row::with_capacity(3)
            .align_y(Alignment::Center)
            .spacing(spacing.space_s)
            .push(widget::text::title1(self.page_title()))
            .push(widget::space::horizontal());
        if page.view().is_some() {
            header = header.push(
                widget::button::suggested("Add task")
                    .on_press_maybe(self.can_write().then_some(Message::Add)),
            );
        }
        if page == Page::Lists {
            header = header.push(
                widget::button::suggested("Create list")
                    .on_press_maybe(self.can_write().then_some(Message::AddList)),
            );
        }
        if page != Page::Matrix {
            content = content.push(header);
        }
        if page.view().is_some() {
            let mut options = widget::column::with_capacity(4).spacing(spacing.space_xxs);
            for (index, sort) in tasks::TaskSort::ALL.into_iter().enumerate() {
                let option = widget::button::text(sort.label()).width(Length::Fill);
                let option = if sort == self.task_sort {
                    option.leading_icon(sort_selected_icon())
                } else {
                    option
                };
                options = options.push(
                    option
                        .name(format!("Sort by {}", sort.label()))
                        .on_press_maybe(self.can_write().then_some(Message::SelectSortMode(index))),
                );
            }
            let sort_mode_button = widget::button::icon(sort_mode_icon())
                .width(Length::Fixed(36.0))
                .height(Length::Fixed(36.0))
                .name(format!("Sort mode: {}", self.task_sort.label()))
                .tooltip("Choose task sort mode")
                .on_press_maybe(self.can_write().then_some(Message::ToggleSortMenu));
            let sort_mode_button = widget::popover(sort_mode_button)
                .position(widget::popover::Position::Bottom)
                .on_close(Message::CloseSortMenu);
            let sort_mode_button = if self.sort_menu_open {
                sort_mode_button.popup(
                    widget::container(options)
                        .width(Length::Fixed(176.0))
                        .padding(spacing.space_xxs)
                        .class(cosmic::theme::Container::Card),
                )
            } else {
                sort_mode_button
            };
            let direction_label = tasks::sort_order_label(self.task_sort_ascending);
            let direction_tooltip = if self.task_sort_ascending {
                "Ascending order; click to switch to descending"
            } else {
                "Descending order; click to switch to ascending"
            };
            let sort_controls = widget::row::with_capacity(2)
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center)
                .push(sort_mode_button)
                .push(
                    widget::button::icon(sort_direction_icon(self.task_sort_ascending))
                        .width(Length::Fixed(36.0))
                        .height(Length::Fixed(36.0))
                        .name(format!("Sort direction: {direction_label}"))
                        .tooltip(direction_tooltip)
                        .on_press_maybe(self.can_write().then_some(Message::ToggleSortDirection)),
                );
            let toolbar = widget::row::with_capacity(3)
                .spacing(spacing.space_s)
                .align_y(Alignment::Center)
                .push(sort_controls)
                .push(widget::space::horizontal())
                .push(
                    widget::checkbox(self.show_completed)
                        .label("Show Completed")
                        .on_toggle_maybe(self.can_write().then_some(Message::ShowCompleted)),
                );
            content = content.push(toolbar);
        }
        if let Page::List(id) = &page {
            content =
                content.push(
                    widget::row::with_capacity(2)
                        .spacing(spacing.space_s)
                        .push(widget::button::standard("Rename list").on_press_maybe(
                            self.can_write().then(|| Message::RenameList(id.clone())),
                        ))
                        .push(widget::button::standard("Delete list").on_press_maybe(
                            self.can_write().then(|| Message::DeleteList(id.clone())),
                        )),
                );
        }
        if let Some(view) = page.view() {
            let mut rows = widget::column::with_capacity(10)
                .spacing(spacing.space_s)
                .width(Length::Fill)
                .align_x(Alignment::Center);
            let mut count = 0;
            if let Some(store) = &self.store {
                let visible = tasks::sorted_visible_tasks(
                    store.tasks(),
                    view,
                    self.today,
                    self.task_sort,
                    self.task_sort_ascending,
                    self.show_completed,
                );
                for task in visible {
                    count += 1;
                    let id = task.id.clone();
                    let check = widget::checkbox(task.completed).on_toggle_maybe(
                        self.can_write()
                            .then_some(move |_| Message::Toggle(id.clone())),
                    );
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
                    let detail = task
                        .due_date
                        .as_ref()
                        .map_or(String::new(), |d| format!("Due {d}"));
                    let task_details = widget::column::with_capacity(2)
                        .spacing(spacing.space_xxs)
                        .push(title)
                        .push(
                            widget::row::with_capacity(2)
                                .spacing(spacing.space_xs)
                                .push(crate::priority_style::marker(task.priority))
                                .push(widget::text::caption(detail)),
                        )
                        .width(Length::Fill);
                    let actions = widget::row::with_capacity(2)
                        .spacing(spacing.space_xxs)
                        .align_y(Alignment::Center)
                        .push(
                            widget::button::icon(edit_task_icon())
                                .width(Length::Fixed(36.0))
                                .height(Length::Fixed(36.0))
                                .tooltip("Edit task")
                                .name("Edit task")
                                .on_press_maybe(
                                    self.can_write().then(|| Message::Edit(task.id.clone())),
                                ),
                        )
                        .push(
                            widget::button::icon(icon::from_name("user-trash-symbolic"))
                                .width(Length::Fixed(36.0))
                                .height(Length::Fixed(36.0))
                                .tooltip("Delete task")
                                .name("Delete task")
                                .on_press_maybe(
                                    self.can_write().then(|| Message::Delete(task.id.clone())),
                                ),
                        );
                    let row = widget::row::with_capacity(3)
                        .spacing(spacing.space_s)
                        .align_y(Alignment::Center)
                        .push(check)
                        .push(task_details)
                        .push(actions);
                    rows = rows.push(
                        widget::container(row)
                            .padding(spacing.space_xxs)
                            .width(Length::Fill)
                            .max_width(880),
                    );
                }
            }
            if count == 0 {
                let empty = if self.store.is_none() {
                    "Tasks are unavailable. Resolve the storage error and retry."
                } else {
                    match view {
                        _ if !self.show_completed => {
                            "No active tasks in this view. Enable Show Completed to see completed tasks."
                        }
                        TaskView::Inbox => "Your inbox is empty. Add a task to get started.",
                        TaskView::Today => "No tasks due today or overdue.",
                        TaskView::List(_) => "No tasks in this list. Add a task to get started.",
                    }
                };
                rows = rows.push(
                    widget::container(widget::text::body(empty))
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                );
            }
            content = content.push(
                widget::scrollable(rows)
                    .height(Length::Fill)
                    .width(Length::Fill),
            );
        } else if page == Page::Matrix {
            content = content.push(matrix_view::view(
                self.store.as_ref(),
                self.matrix_list.as_deref(),
                self.can_write(),
                self.matrix_priority_task.as_deref(),
            ));
        } else if page == Page::Lists {
            let mut lists = widget::column::with_capacity(8).spacing(spacing.space_s);
            match &self.store {
                None => {
                    lists = lists.push(widget::text::body(
                        "Lists are unavailable. Resolve the storage error and retry.",
                    ))
                }
                Some(store) if store.lists().is_empty() => {
                    lists = lists.push(widget::text::body(
                        "No lists yet. Create a list to organize your tasks.",
                    ))
                }
                Some(store) => {
                    for list in store.lists() {
                        lists = lists.push(
                            widget::row::with_capacity(5)
                                .spacing(spacing.space_s)
                                .align_y(Alignment::Center)
                                .push(
                                    widget::button::text(list.name.clone())
                                        .on_press(Message::OpenList(list.id.clone())),
                                )
                                .push(widget::space::horizontal())
                                .push(
                                    widget::button::icon(icon::from_name("view-pin-symbolic"))
                                        .tooltip(if list.pinned {
                                            "Unpin list"
                                        } else {
                                            "Pin list"
                                        })
                                        .selected(list.pinned)
                                        .on_press_maybe(
                                            self.can_write()
                                                .then(|| Message::ToggleListPin(list.id.clone())),
                                        ),
                                )
                                .push(
                                    widget::button::text("Rename").on_press_maybe(
                                        self.can_write()
                                            .then(|| Message::RenameList(list.id.clone())),
                                    ),
                                )
                                .push(
                                    widget::button::text("Delete").on_press_maybe(
                                        self.can_write()
                                            .then(|| Message::DeleteList(list.id.clone())),
                                    ),
                                ),
                        );
                    }
                }
            }
            content = content.push(widget::scrollable(lists).height(Length::Fill));
        } else {
            content = content.push(widget::text::body(match page {
                Page::Calendar => "Calendar view is coming later.",
                _ => "Coming later.",
            }));
        }
        let content = if page.view().is_some() || page == Page::Lists {
            widget::container(content)
                .max_width(960)
                .width(Length::Fill)
                .height(Length::Fill)
        } else {
            widget::container(content)
                .width(Length::Fill)
                .height(Length::Fill)
        };
        widget::container(content)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .into()
    }
    fn dialog(&self) -> Option<Element<'_, Message>> {
        if let Some(editor) = &self.list_editor {
            let mut fields = widget::column::with_capacity(2)
                .spacing(cosmic::theme::spacing().space_s)
                .push(
                    widget::text_input("List name", &editor.name)
                        .id(self.title_input.clone())
                        .on_input(Message::ListName)
                        .on_submit(|_| Message::SaveList),
                );
            if let Some(error) = &editor.error {
                fields = fields.push(widget::text::body(error));
            }
            let mut dialog = widget::dialog()
                .title(if editor.is_new {
                    "Create list"
                } else {
                    "Rename list"
                })
                .control(fields)
                .primary_action(
                    widget::button::suggested("Save")
                        .on_press_maybe(self.can_write().then_some(Message::SaveList)),
                )
                .secondary_action(widget::button::standard("Cancel").on_press(Message::Cancel));
            if self.storage_error.is_some() {
                dialog = dialog.tertiary_action(
                    widget::button::standard("Retry storage").on_press(Message::RetryStorage),
                );
            }
            return Some(dialog.into());
        }
        if let Some(id) = &self.delete_list_id {
            let list = self
                .store
                .as_ref()?
                .lists()
                .iter()
                .find(|list| &list.id == id)?;
            let mut body = format!(
                "Delete “{}”? Its tasks will be unassigned. Incomplete tasks will appear in Inbox; completed tasks will remain in Completed. No tasks will be deleted.",
                list.name
            );
            if let Some(error) = &self.storage_error {
                body.push_str(&format!("\n\n{error}"));
            }
            return Some(
                widget::dialog()
                    .title("Delete list?")
                    .body(body)
                    .primary_action(
                        widget::button::destructive("Delete list")
                            .on_press_maybe(self.can_write().then_some(Message::ConfirmDeleteList)),
                    )
                    .secondary_action(widget::button::standard("Cancel").on_press(Message::Cancel))
                    .into(),
            );
        }
        if let Some(editor) = &self.editor {
            let spacing = cosmic::theme::spacing();
            let calendar_button =
                widget::button::icon(icon::from_name("x-office-calendar-symbolic"))
                    .tooltip("Choose date")
                    .name("Choose due date")
                    .on_press(Message::ToggleCalendar);
            let calendar_button = widget::popover(calendar_button)
                .position(widget::popover::Position::Bottom)
                .on_close(Message::CloseCalendar);
            let calendar_button = if editor.calendar_open {
                calendar_button.popup(
                    widget::container(widget::calendar(
                        &editor.calendar,
                        Message::CalendarSelect,
                        || Message::CalendarPrev,
                        || Message::CalendarNext,
                        Weekday::Sunday,
                    ))
                    .class(cosmic::theme::Container::Card)
                    .padding(spacing.space_xxs),
                )
            } else {
                calendar_button
            };
            let due_input = widget::text_input("YYYY-MM-DD", &editor.due)
                .style(if editor.due_error.is_some() {
                    invalid_text_input_style()
                } else {
                    cosmic::theme::TextInput::Default
                })
                .on_input(Message::Due)
                .on_submit(|_| Message::Save)
                .on_unfocus(Message::DueBlur)
                .width(Length::Fill);
            let due_entry = widget::row::with_capacity(2)
                .align_y(Alignment::Center)
                .spacing(spacing.space_xxs)
                .push(due_input)
                .push(calendar_button);

            let mut priority_buttons = widget::row::with_capacity(4).spacing(spacing.space_xxs);
            for (index, priority) in Priority::ALL.into_iter().enumerate() {
                priority_buttons = priority_buttons.push(priority_button(
                    editor.task.priority,
                    priority,
                    Message::Priority(index),
                ));
            }
            let mut options = vec!["Inbox".to_string()];
            let mut selected = Some(0);
            if let Some(store) = &self.store {
                options.extend(store.lists().iter().map(|list| list.name.clone()));
                if let Some(id) = &editor.task.list_id {
                    selected = store
                        .lists()
                        .iter()
                        .position(|list| &list.id == id)
                        .map(|i| i + 1);
                }
            }
            let list_selector =
                widget::dropdown(options, selected, Message::AssignList).width(Length::Fill);
            let title_field = widget::column::with_capacity(3)
                .spacing(spacing.space_xxs)
                .push(widget::text::body("Title"))
                .push(
                    widget::text_input("Task title", &editor.task.title)
                        .style(if editor.title_error.is_some() {
                            invalid_text_input_style()
                        } else {
                            cosmic::theme::TextInput::Default
                        })
                        .id(self.title_input.clone())
                        .on_input(Message::Title)
                        .on_submit(|_| Message::Save)
                        .width(Length::Fill),
                )
                .push_maybe(editor.title_error.as_ref().map(widget::text::body));
            let shortcuts = widget::row::with_capacity(3)
                .spacing(spacing.space_xxs)
                .push(widget::button::text("Today").on_press(Message::DateShortcut(0)))
                .push(widget::button::text("Tomorrow").on_press(Message::DateShortcut(1)))
                .push(widget::button::text("Next week").on_press(Message::DateShortcut(7)));
            let due_field = widget::column::with_capacity(4)
                .spacing(spacing.space_xxs)
                .push(widget::text::body("Due date"))
                .push(due_entry)
                .push(shortcuts)
                .push_maybe(editor.due_error.as_ref().map(widget::text::body));
            let metadata = widget::row::with_capacity(2)
                .align_y(Alignment::Start)
                .spacing(spacing.space_s)
                .push(
                    widget::column::with_capacity(2)
                        .width(Length::FillPortion(1))
                        .spacing(spacing.space_xxs)
                        .push(widget::text::body("Priority"))
                        .push(priority_buttons),
                )
                .push(
                    widget::column::with_capacity(2)
                        .width(Length::FillPortion(1))
                        .spacing(spacing.space_xxs)
                        .push(widget::text::body("List"))
                        .push(list_selector),
                );
            let fields = widget::column::with_capacity(6)
                .spacing(spacing.space_s)
                .push(title_field)
                .push_maybe(editor.error.as_ref().map(widget::text::body))
                .push(due_field)
                .push(metadata)
                .push(widget::text::body("Notes"))
                .push(
                    widget::text_editor::text_editor(&editor.notes)
                        .height(76)
                        .on_action(Message::Notes),
                );
            let mut dialog = widget::dialog()
                .title(if editor.is_new {
                    "Add Task"
                } else {
                    "Edit Task"
                })
                .control(fields)
                .max_width(700)
                .primary_action(
                    widget::button::suggested("Save")
                        .on_press_maybe(self.can_write().then_some(Message::Save)),
                )
                .secondary_action(widget::button::standard("Cancel").on_press(Message::Cancel));
            if self.storage_error.is_some() {
                dialog = dialog.tertiary_action(
                    widget::button::standard("Retry storage").on_press(Message::RetryStorage),
                );
            }
            return Some(dialog.into());
        }
        let id = self.delete_id.as_ref()?;
        let task = self.store.as_ref()?.tasks().iter().find(|t| &t.id == id)?;
        Some(
            widget::dialog()
                .title("Delete task?")
                .body(match &self.storage_error {
                    Some(error) => {
                        format!("Delete “{}”? This cannot be undone.\n\n{error}", task.title)
                    }
                    None => format!("Delete “{}”? This cannot be undone.", task.title),
                })
                .primary_action(
                    widget::button::destructive("Delete")
                        .on_press_maybe(self.can_write().then_some(Message::ConfirmDelete)),
                )
                .secondary_action(widget::button::standard("Cancel").on_press(Message::Cancel))
                .into(),
        )
    }
    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            cosmic::iced::time::every(Duration::from_secs(30)).map(|_| Message::DateTick),
            cosmic::iced::window::close_requests().map(Message::WindowCloseRequested),
        ];
        if let Some(conn) = &self.tray_connection {
            subscriptions.push(tray::subscription(conn.clone()).map(Message::Tray));
        }
        if self.page == Page::Matrix && self.matrix_priority_task.is_some() && self.editor.is_none()
        {
            subscriptions.push(cosmic::iced::event::listen_with(
                |event, _, _| match event {
                    cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                        key:
                            cosmic::iced::keyboard::Key::Named(
                                cosmic::iced::keyboard::key::Named::Escape,
                            ),
                        ..
                    }) => Some(Message::Matrix(matrix_view::Action::ClosePriority)),
                    _ => None,
                },
            ));
        }
        if self.sort_menu_open && self.editor.is_none() {
            subscriptions.push(cosmic::iced::event::listen_with(
                |event, _, _| match event {
                    cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                        key:
                            cosmic::iced::keyboard::Key::Named(
                                cosmic::iced::keyboard::key::Named::Escape,
                            ),
                        ..
                    }) => Some(Message::CloseSortMenu),
                    _ => None,
                },
            ));
        }
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.calendar_open)
        {
            subscriptions.push(cosmic::iced::event::listen_with(
                |event, _, _| match event {
                    cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                        key:
                            cosmic::iced::keyboard::Key::Named(
                                cosmic::iced::keyboard::key::Named::Escape,
                            ),
                        ..
                    }) => Some(Message::CloseCalendar),
                    _ => None,
                },
            ));
        }
        Subscription::batch(subscriptions)
    }
    fn on_app_exit(&mut self) -> Option<Message> {
        Some(Message::CloseMain)
    }
    fn on_close_requested(&self, id: cosmic::iced::window::Id) -> Option<Message> {
        (self.core.main_window_id() == Some(id)).then_some(Message::WindowClosed(id))
    }
    fn dbus_connection(&mut self, conn: zbus::Connection) -> Task<cosmic::Action<Message>> {
        self.tray_connection = Some(conn);
        Task::none()
    }
    fn dbus_activation(
        &mut self,
        message: cosmic::dbus_activation::Message,
    ) -> Task<cosmic::Action<Message>> {
        let token = match message.msg {
            cosmic::dbus_activation::Details::ActivateAction { action, args }
                if action == "open" =>
            {
                args.into_iter().next()
            }
            _ => message.activation_token,
        };
        self.today = jiff::Zoned::now().date();
        let effect = self.lifecycle.show(token);
        self.window_effect(effect)
    }
    fn on_escape(&mut self) -> Task<cosmic::Action<Message>> {
        self.sort_menu_open = false;
        self.editor = None;
        self.delete_id = None;
        self.list_editor = None;
        self.delete_list_id = None;
        self.core.window.show_context = false;
        Task::none()
    }
    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::NavContext(NavMenuAction::ToggleLists(_)) => {
                return self.update(Message::ToggleLists);
            }
            Message::NavContext(NavMenuAction::ToggleListPin(id)) => {
                if let Some(Page::List(id)) = self.nav.data::<Page>(id).cloned() {
                    return self.update(Message::ToggleListPin(id));
                }
            }
            Message::Matrix(matrix_view::Action::OpenPriority(id)) if self.can_write() => {
                self.matrix_priority_task = Some(id);
            }
            Message::Matrix(matrix_view::Action::ClosePriority) => {
                self.matrix_priority_task = None;
            }
            Message::Matrix(matrix_view::Action::SelectList(index)) => {
                self.matrix_priority_task = None;
                if index == 0 {
                    self.matrix_list = None;
                } else if let Some(list) = self
                    .store
                    .as_ref()
                    .and_then(|store| store.lists().get(index - 1))
                {
                    self.matrix_list = Some(list.id.clone());
                }
            }
            Message::Matrix(matrix_view::Action::Add(quadrant)) if self.can_write() => {
                self.matrix_priority_task = None;
                match matrix::draft(self.matrix_list.as_deref(), quadrant) {
                    Ok(task) => {
                        self.open_editor(task, true);
                        return widget::text_input::focus(self.title_input.clone());
                    }
                    Err(error) => self.storage_error = Some(error),
                }
            }
            Message::Matrix(matrix_view::Action::Classify(id, quadrant)) if self.can_write() => {
                self.matrix_priority_task = None;
                if let Some(store) = &mut self.store {
                    self.storage_error = store.classify(&id, quadrant).err();
                }
            }
            Message::Matrix(matrix_view::Action::Reorder(id, direction)) if self.can_write() => {
                if let Some(store) = &mut self.store {
                    self.storage_error = store.reorder(&id, direction).err();
                }
            }
            Message::Tray(tray::Event::Availability(result)) => {
                self.tray_error = result.as_ref().err().cloned();
                let effect = self.lifecycle.availability(result.is_ok());
                return self.window_effect(effect);
            }
            Message::Tray(tray::Event::Open(token)) => {
                self.today = jiff::Zoned::now().date();
                let effect = self.lifecycle.show(token);
                return self.window_effect(effect);
            }
            Message::Tray(tray::Event::Quit) => {
                let effect = self.lifecycle.quit();
                return self.window_effect(effect);
            }
            Message::CloseMain => {
                let effect = self.lifecycle.close();
                return self.window_effect(effect);
            }
            Message::WindowCloseRequested(id) if self.core.main_window_id() == Some(id) => {
                let effect = self.lifecycle.close();
                return self.window_effect(effect);
            }
            Message::WindowClosed(id) if self.core.main_window_id() == Some(id) => {
                self.core.set_main_window_id(None);
                let effect = self.lifecycle.closed();
                return self.window_effect(effect);
            }
            Message::WindowOpened(id) if self.core.main_window_id() == Some(id) => {
                let effect = self.lifecycle.opened();
                return self.window_effect(effect);
            }
            Message::FocusToken(id, token)
                if self.core.main_window_id() == Some(id)
                    && self.lifecycle.state != State::Quitting =>
            {
                if let Some(token) = token {
                    return cosmic::iced::platform_specific::shell::commands::activation::activate(
                        id, token,
                    );
                }
                return Task::batch([
                    cosmic::iced::window::gain_focus(id),
                    cosmic::iced::window::request_user_attention(
                        id,
                        Some(cosmic::iced::window::UserAttention::Informational),
                    ),
                ]);
            }
            Message::Exit => return cosmic::iced::exit(),
            Message::AddList if self.can_write() => {
                self.list_editor = Some(ListEditor {
                    id: None,
                    is_new: true,
                    name: String::new(),
                    error: None,
                });
                return widget::text_input::focus(self.title_input.clone());
            }
            Message::RenameList(id) if self.can_write() => {
                if let Some(list) = self
                    .store
                    .as_ref()
                    .and_then(|s| s.lists().iter().find(|l| l.id == id))
                {
                    self.list_editor = Some(ListEditor {
                        id: Some(id),
                        is_new: false,
                        name: list.name.clone(),
                        error: None,
                    });
                    return widget::text_input::focus(self.title_input.clone());
                }
            }
            Message::ListName(name) => {
                if let Some(editor) = &mut self.list_editor {
                    editor.name = name;
                }
            }
            Message::SaveList if self.can_write() => {
                if let (Some(editor), Some(store)) = (&mut self.list_editor, &mut self.store) {
                    if let Err(error) =
                        tasks::validate_list_name(&editor.name, store.lists(), editor.id.as_deref())
                    {
                        editor.error = Some(error);
                        return Task::none();
                    }
                    let list = match &editor.id {
                        Some(id) => Ok(store
                            .lists()
                            .iter()
                            .find(|list| &list.id == id)
                            .map(|list| TaskList {
                                id: id.clone(),
                                name: editor.name.trim().into(),
                                pinned: list.pinned,
                            })
                            .unwrap_or_else(|| TaskList {
                                id: id.clone(),
                                name: editor.name.trim().into(),
                                pinned: false,
                            })),
                        None => TaskList::new(&editor.name),
                    };
                    let result = list.and_then(|list| {
                        // Retain the generated ID in the draft, including when a
                        // save fails or directory syncing fails after publication.
                        editor.id = Some(list.id.clone());
                        store.upsert_list(list)
                    });
                    match result {
                        Ok(()) => self.list_editor = None,
                        Err(error) => {
                            editor.error = Some(error.clone());
                            self.storage_error = Some(error);
                        }
                    }
                }
                self.rebuild_nav();
                return self.update_title();
            }
            Message::DeleteList(id) if self.can_write() => self.delete_list_id = Some(id),
            Message::ConfirmDeleteList if self.can_write() => {
                if let (Some(store), Some(id)) = (&mut self.store, &self.delete_list_id) {
                    self.storage_error = store.delete_list(id).err();
                    if !store.lists().iter().any(|list| &list.id == id) {
                        if self.page == Page::List(id.clone()) {
                            self.page = Page::Lists;
                        }
                        self.delete_list_id = None;
                    }
                    if self
                        .matrix_list
                        .as_ref()
                        .is_some_and(|id| !store.lists().iter().any(|list| &list.id == id))
                    {
                        self.matrix_list = None;
                    }
                }
                self.rebuild_nav();
                return self.update_title();
            }
            Message::OpenList(id) => {
                if self
                    .store
                    .as_ref()
                    .is_some_and(|store| store.lists().iter().any(|list| list.id == id))
                {
                    self.page = Page::List(id);
                    self.rebuild_nav();
                    return self.update_title();
                }
            }
            Message::ToggleLists => {
                self.lists_expanded = !self.lists_expanded;
                if !self.lists_expanded
                    && let Page::List(id) = &self.page
                    && self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().iter().find(|list| &list.id == id))
                        .is_none_or(|list| !list.visible_in_sidebar(false))
                {
                    self.page = Page::Lists;
                }
                self.rebuild_nav();
                return self.update_title();
            }
            Message::ToggleListPin(id) if self.can_write() => {
                if let Some(store) = &mut self.store
                    && let Some(mut list) = store.lists().iter().find(|list| list.id == id).cloned()
                {
                    list.pinned = !list.pinned;
                    self.storage_error = store.upsert_list(list).err();
                }
                if !self.lists_expanded
                    && self.page == Page::List(id.clone())
                    && self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().iter().find(|list| list.id == id))
                        .is_none_or(|list| !list.visible_in_sidebar(false))
                {
                    self.page = Page::Lists;
                }
                self.rebuild_nav();
                return self.update_title();
            }
            Message::AssignList(index) => {
                if let Some(editor) = &mut self.editor {
                    if index == 0 {
                        editor.task.list_id = None;
                    } else if let Some(list) = self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().get(index - 1))
                    {
                        editor.task.list_id = Some(list.id.clone());
                    }
                }
            }
            Message::Add if self.can_write() => match self.page.draft_task() {
                Ok(task) => {
                    self.open_editor(task, true);
                    return widget::text_input::focus(self.title_input.clone());
                }
                Err(error) => self.storage_error = Some(error),
            },
            Message::Edit(id) if self.can_write() => {
                self.matrix_priority_task = None;
                if let Some(task) = self
                    .store
                    .as_ref()
                    .and_then(|s| s.tasks().iter().find(|t| t.id == id))
                    .cloned()
                {
                    self.open_editor(task, false);
                    return widget::text_input::focus(self.title_input.clone());
                }
            }
            Message::Title(title) => {
                if let Some(e) = &mut self.editor {
                    e.task.title = title;
                    e.title_error = title_validation_error(&e.task.title).map(str::to_string);
                    e.error = None;
                }
            }
            Message::Due(due) => {
                if let Some(e) = &mut self.editor {
                    let year = jiff::Zoned::now().year();
                    e.due = date_entry::normalize_continuous_input(&due);
                    e.due_error = date_entry::validation_error_for_edit(&e.due, year);
                }
            }
            Message::DueBlur => {
                if let Some(e) = &mut self.editor {
                    let year = jiff::Zoned::now().year();
                    e.due = date_entry::normalize_edit(&e.due, year);
                    e.due_error = date_entry::validation_error_for_edit(&e.due, year);
                }
            }
            Message::DateShortcut(days) => {
                if let Some(e) = &mut self.editor {
                    e.due = date_entry::relative_date(jiff::Zoned::now().date(), days).to_string();
                    e.due_error = None;
                }
            }
            Message::Priority(index) => {
                if let (Some(e), Some(p)) = (&mut self.editor, priority_at(index)) {
                    e.task.priority = p;
                }
            }
            Message::ToggleCalendar => {
                if let Some(e) = &mut self.editor {
                    e.calendar_open = !e.calendar_open;
                    if e.calendar_open {
                        e.calendar = cosmic::widget::calendar::CalendarModel::new(
                            date_entry::picker_date(&e.due, jiff::Zoned::now().date()),
                            date_entry::picker_date(&e.due, jiff::Zoned::now().date()),
                        );
                    }
                }
            }
            Message::CloseCalendar => {
                if let Some(e) = &mut self.editor {
                    e.calendar_open = false;
                }
            }
            Message::CalendarPrev => {
                if let Some(e) = &mut self.editor {
                    e.calendar.show_prev_month();
                }
            }
            Message::CalendarNext => {
                if let Some(e) = &mut self.editor {
                    e.calendar.show_next_month();
                }
            }
            Message::CalendarSelect(date) => {
                if let Some(e) = &mut self.editor {
                    e.due = date.to_string();
                    e.due_error = None;
                    e.calendar.set_selected_visible(date);
                    e.calendar_open = false;
                }
            }
            Message::Notes(action) => {
                if let Some(e) = &mut self.editor {
                    e.notes.perform(action);
                }
            }
            Message::Save if self.can_write() => {
                if let Some(e) = &mut self.editor {
                    e.title_error = title_validation_error(&e.task.title).map(str::to_string);
                    if e.title_error.is_some() {
                        return Task::none();
                    }
                    let due = match date_entry::parse_optional(&e.due, jiff::Zoned::now().year()) {
                        Ok(due) => {
                            e.due_error = None;
                            due
                        }
                        Err(error) => {
                            e.due_error = Some(error);
                            return Task::none();
                        }
                    };
                    let notes = e.notes.text();
                    let task = apply_editor_fields(e.task.clone(), &e.task.title, due, notes);
                    if let Err(error) = tasks::validate(std::slice::from_ref(&task)) {
                        e.error = Some(error);
                        return Task::none();
                    }
                    if let Some(store) = &mut self.store {
                        match store.upsert(task) {
                            Ok(()) => self.editor = None,
                            Err(error) => {
                                e.error = Some(error.clone());
                                self.storage_error = Some(error);
                            }
                        }
                    }
                }
            }
            Message::Toggle(id) if self.can_write() => {
                if let Some(store) = &mut self.store {
                    self.storage_error = store.toggle(&id).err();
                }
            }
            Message::Delete(id) if self.can_write() => self.delete_id = Some(id),
            Message::ConfirmDelete if self.can_write() => {
                if let (Some(store), Some(id)) = (&mut self.store, &self.delete_id) {
                    self.storage_error = store.delete(id).err();
                    if !store.tasks().iter().any(|t| &t.id == id) {
                        self.delete_id = None;
                    }
                }
            }
            Message::Cancel => {
                self.editor = None;
                self.delete_id = None;
                self.list_editor = None;
                self.delete_list_id = None;
            }
            Message::RetryStorage => {
                // Retain a loaded store and its expected disk snapshot after save errors.
                // Reopening it here could discard edits or authorize overwriting external changes.
                if self.store.is_none() {
                    self.load();
                } else if let Some(store) = &self.store {
                    self.storage_error = store.check_storage().err();
                    if self.storage_error.is_none()
                        && let Some(editor) = &mut self.editor
                    {
                        editor.error = None;
                    }
                    if self.storage_error.is_none()
                        && let Some(editor) = &mut self.list_editor
                    {
                        editor.error = None;
                    }
                }
                return self.update_title();
            }
            Message::DateTick => self.today = jiff::Zoned::now().date(),
            Message::ToggleSortMenu if self.can_write() => {
                self.sort_menu_open = !self.sort_menu_open;
            }
            Message::CloseSortMenu => self.sort_menu_open = false,
            Message::SelectSortMode(index) if self.can_write() => {
                if let Some(sort) = tasks::sort_mode_at(index) {
                    let ascending = if sort == self.task_sort {
                        self.task_sort_ascending
                    } else {
                        sort.default_ascending()
                    };
                    self.save_task_preferences(sort, ascending, self.show_completed);
                }
                self.sort_menu_open = false;
            }
            Message::ToggleSortDirection if self.can_write() => {
                self.save_task_preferences(
                    self.task_sort,
                    !self.task_sort_ascending,
                    self.show_completed,
                );
            }
            Message::ShowCompleted(show) if self.can_write() => {
                self.save_task_preferences(self.task_sort, self.task_sort_ascending, show);
            }
            Message::About => self.core.window.show_context = !self.core.window.show_context,
            Message::LaunchUrl(url) => {
                if let Err(e) = open::that_detached(url) {
                    eprintln!("Cannot open repository link: {e}");
                }
            }
            _ => {}
        }
        Task::none()
    }
}
impl AppModel {
    fn window_effect(&mut self, effect: Effect) -> Task<cosmic::Action<Message>> {
        use cosmic::iced::{Size, window};
        match effect {
            Effect::None => Task::none(),
            Effect::Quit => {
                let conn = self.tray_connection.take();
                Task::perform(
                    async move {
                        if let Some(conn) = conn {
                            let _ =
                                tokio::time::timeout(Duration::from_secs(1), conn.close()).await;
                        }
                    },
                    |_| cosmic::Action::App(Message::Exit),
                )
            }
            Effect::Close => self
                .core
                .main_window_id()
                .map_or_else(Task::none, window::close),
            Effect::Open => {
                let (id, open) = window::open(window::Settings {
                    min_size: Some(Size::new(640., 560.)),
                    decorations: false,
                    transparent: true,
                    exit_on_close_request: false,
                    platform_specific: window::settings::PlatformSpecific {
                        application_id: storage::APP_ID.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                self.core.set_main_window_id(Some(id));
                Task::batch([
                    self.update_title(),
                    open.map(|id| cosmic::Action::App(Message::WindowOpened(id))),
                ])
            }
            Effect::Focus => {
                let Some(id) = self.core.main_window_id() else {
                    return Task::none();
                };
                let token = self.lifecycle.take_token();
                let focus = if token.is_some() {
                    Task::done(cosmic::Action::App(Message::FocusToken(id, token)))
                } else {
                    cosmic::iced::platform_specific::shell::commands::activation::request_token(
                        Some(storage::APP_ID.into()),
                        Some(id),
                    )
                    .map(move |token| cosmic::Action::App(Message::FocusToken(id, token)))
                };
                window::minimize(id, false).chain(focus)
            }
        }
    }
    fn page_title(&self) -> String {
        if let Page::List(id) = &self.page
            && let Some(list) = self
                .store
                .as_ref()
                .and_then(|store| store.lists().iter().find(|list| &list.id == id))
        {
            return list.name.clone();
        }
        self.page.title().into()
    }
    fn can_write(&self) -> bool {
        self.store.is_some() && self.storage_error.is_none()
    }
    fn rebuild_nav(&mut self) {
        self.nav = build_nav(
            self.store.as_ref().map_or(&[], Store::lists),
            &self.page,
            self.lists_expanded,
        );
    }
    fn load(&mut self) {
        match storage::data_path().and_then(Store::open) {
            Ok(store) => {
                self.task_sort = store.sort_mode();
                self.task_sort_ascending = store.sort_ascending();
                self.show_completed = store.show_completed();
                self.store = Some(store);
                self.storage_error = None;
                self.rebuild_nav();
            }
            Err(error) => {
                self.storage_error = Some(error);
            }
        }
    }
    fn save_task_preferences(
        &mut self,
        sort: tasks::TaskSort,
        ascending: bool,
        show_completed: bool,
    ) {
        if let Some(store) = &mut self.store {
            match store.set_preferences(sort, ascending, show_completed) {
                Ok(()) => {
                    self.task_sort = sort;
                    self.task_sort_ascending = ascending;
                    self.show_completed = show_completed;
                    self.storage_error = None;
                }
                Err(error) => self.storage_error = Some(error),
            }
        }
    }
    fn open_editor(&mut self, task: KaraTask, is_new: bool) {
        let picker_date = date_entry::picker_date(
            task.due_date.as_deref().unwrap_or_default(),
            jiff::Zoned::now().date(),
        );
        self.editor = Some(Editor {
            due: task.due_date.clone().unwrap_or_default(),
            notes: widget::text_editor::Content::with_text(
                task.notes.as_deref().unwrap_or_default(),
            ),
            task,
            error: None,
            title_error: None,
            due_error: None,
            calendar: cosmic::widget::calendar::CalendarModel::new(picker_date, picker_date),
            calendar_open: false,
            is_new,
        });
    }
    fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let title = format!("KARA — {}", self.page_title());
        self.set_header_title("KARA".to_string());
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            Task::none()
        }
    }
}

pub(crate) fn priority_button<M: Clone + 'static>(
    selected: Priority,
    priority: Priority,
    on_press: M,
) -> cosmic::Element<'static, M> {
    let label = priority_accessible_name(selected, priority);
    let button = widget::button::custom(
        widget::container(crate::priority_style::flag(priority))
            .width(Length::Fixed(36.0))
            .height(Length::Fixed(36.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(36.0))
    .class(cosmic::theme::Button::IconVertical)
    .selected(priority_selected(selected, priority))
    .name(label.clone())
    .on_press(on_press);
    widget::tooltip(
        button,
        widget::text::body(priority.label()),
        widget::tooltip::Position::Top,
    )
    .into()
}

pub(crate) fn edit_task_icon() -> icon::Handle {
    icon::from_svg_bytes(EDIT_ICON).symbolic(true)
}

fn inbox_icon() -> icon::Handle {
    icon::from_svg_bytes(INBOX_ICON).symbolic(true)
}

fn today_icon() -> icon::Handle {
    icon::from_svg_bytes(TODAY_ICON).symbolic(true)
}

fn pinned_list_icon() -> icon::Handle {
    icon::from_svg_bytes(PINNED_LIST_ICON).symbolic(true)
}

fn sort_mode_icon() -> icon::Handle {
    icon::from_svg_bytes(SORT_MODE_ICON).symbolic(true)
}

fn sort_direction_icon(ascending: bool) -> icon::Handle {
    icon::from_svg_bytes(if ascending {
        SORT_ASCENDING_ICON
    } else {
        SORT_DESCENDING_ICON
    })
    .symbolic(true)
}

fn sort_selected_icon() -> icon::Handle {
    icon::from_svg_bytes(SORT_SELECTED_ICON).symbolic(true)
}

fn priority_accessible_name(selected: Priority, priority: Priority) -> String {
    format!(
        "{} — {}{}",
        priority.short_label(),
        priority.label(),
        if priority_selected(selected, priority) {
            " (selected)"
        } else {
            ""
        }
    )
}

fn priority_selected(selected: Priority, option: Priority) -> bool {
    selected == option
}

fn priority_at(index: usize) -> Option<Priority> {
    Priority::ALL.get(index).copied()
}

fn title_validation_error(title: &str) -> Option<&'static str> {
    title
        .trim()
        .is_empty()
        .then_some("A task title is required.")
}

fn apply_editor_fields(
    mut task: KaraTask,
    title: &str,
    due: Option<String>,
    notes: String,
) -> KaraTask {
    task.title = title.trim().into();
    task.due_date = due;
    task.notes = (!notes.trim().is_empty()).then_some(notes);
    task
}

fn invalid_text_input_style() -> cosmic::theme::TextInput {
    use cosmic::widget::text_input::StyleSheet;
    let appearance = |theme: &cosmic::Theme| {
        let mut appearance = theme.active(&cosmic::theme::TextInput::Default);
        appearance.border_color = theme.cosmic().palette.bright_red.into();
        appearance
    };
    cosmic::theme::TextInput::Custom {
        active: Box::new(appearance),
        error: Box::new(appearance),
        hovered: Box::new(appearance),
        focused: Box::new(appearance),
        disabled: Box::new(appearance),
    }
}

fn build_nav(lists: &[TaskList], active: &Page, lists_expanded: bool) -> nav_bar::Model {
    let mut nav = nav_bar::Model::default();
    nav.insert()
        .text("Tasks")
        .data(Page::Tasks)
        .icon(icon::from_name("view-list-symbolic"));

    for page in [Page::Inbox, Page::Today, Page::Calendar, Page::Lists] {
        let nav_icon = match page {
            Page::Inbox => icon::icon(inbox_icon()),
            Page::Today => icon::icon(today_icon()),
            Page::Calendar => icon::from_name("x-office-calendar-symbolic").icon(),
            Page::Lists => icon::from_name(if lists_expanded {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            })
            .icon(),
            _ => unreachable!(),
        };
        let id = nav
            .insert()
            .text(page.title())
            .data(page.clone())
            .icon(nav_icon)
            .id();
        nav.indent_set(id, 1);
        if page == Page::Lists {
            for list in lists
                .iter()
                .filter(|list| list.visible_in_sidebar(lists_expanded))
            {
                let id = nav
                    .insert()
                    .text(list.name.clone())
                    .data(Page::List(list.id.clone()))
                    .icon(if list.pinned {
                        icon::icon(pinned_list_icon())
                    } else {
                        icon::from_name("view-list-symbolic").icon()
                    })
                    .id();
                nav.indent_set(id, 2);
            }
        }
    }

    for (page, icon_name) in [
        (Page::Board, "view-grid-symbolic"),
        (Page::Matrix, "view-app-grid-symbolic"),
        (Page::Pomodoro, "alarm-symbolic"),
    ] {
        nav.insert()
            .text(page.title())
            .data(page)
            .icon(icon::from_name(icon_name));
    }

    let selected = nav
        .iter()
        .find(|id| nav.data::<Page>(*id) == Some(active))
        .or_else(|| {
            matches!(active, Page::List(_))
                .then(|| {
                    nav.iter()
                        .find(|id| nav.data::<Page>(*id) == Some(&Page::Lists))
                })
                .flatten()
        });
    if let Some(selected) = selected {
        nav.activate(selected);
    }
    nav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_edit_action_icon_resolves_to_themable_symbolic_svg() {
        assert!(std::str::from_utf8(EDIT_ICON).unwrap().contains("<path"));

        let handle = edit_task_icon();
        assert!(handle.symbolic);
        assert!(matches!(&handle.data, icon::Data::Svg(_)));
    }

    #[test]
    fn task_navigation_assets_are_bundled_symbolic_svgs() {
        for (asset, handle) in [
            (INBOX_ICON, inbox_icon()),
            (TODAY_ICON, today_icon()),
            (PINNED_LIST_ICON, pinned_list_icon()),
            (SORT_MODE_ICON, sort_mode_icon()),
            (SORT_ASCENDING_ICON, sort_direction_icon(true)),
            (SORT_DESCENDING_ICON, sort_direction_icon(false)),
            (SORT_SELECTED_ICON, sort_selected_icon()),
        ] {
            assert!(std::str::from_utf8(asset).unwrap().contains("<path"));
            assert!(handle.symbolic);
            assert!(matches!(&handle.data, icon::Data::Svg(_)));
        }
    }

    #[test]
    fn task_creation_uses_the_selected_list_and_otherwise_inbox() {
        let task = Page::List("selected-list".into()).draft_task().unwrap();
        assert_eq!(task.list_id.as_deref(), Some("selected-list"));
        assert!(!task.completed);
        assert!(task.due_date.is_none());
        assert_eq!(task.priority, Priority::P0);
        for page in [
            Page::Tasks,
            Page::Inbox,
            Page::Today,
            Page::Calendar,
            Page::Lists,
        ] {
            assert!(page.draft_task().unwrap().list_id.is_none());
        }
    }
    #[test]
    fn native_sidebar_keeps_task_navigation_and_matrix_as_top_level_sections() {
        let mut pinned = TaskList::new("Pinned").unwrap();
        pinned.pinned = true;
        let unpinned = TaskList::new("Unpinned").unwrap();
        let lists = [pinned.clone(), unpinned.clone()];
        let active = Page::List(pinned.id.clone());
        let expanded = build_nav(&lists, &active, true);
        let pages: Vec<_> = expanded
            .iter()
            .map(|id| expanded.data::<Page>(id).unwrap().clone())
            .collect();

        assert_eq!(
            pages,
            vec![
                Page::Tasks,
                Page::Inbox,
                Page::Today,
                Page::Calendar,
                Page::Lists,
                Page::List(pinned.id.clone()),
                Page::List(unpinned.id.clone()),
                Page::Board,
                Page::Matrix,
                Page::Pomodoro,
            ]
        );
        assert_eq!(expanded.active_data::<Page>(), Some(&active));
        assert_eq!(expanded.indent(expanded.active()), Some(2));

        let collapsed = build_nav(&lists, &Page::Lists, false);
        let collapsed_pages: Vec<_> = collapsed
            .iter()
            .map(|id| collapsed.data::<Page>(id).unwrap().clone())
            .collect();
        assert!(collapsed_pages.contains(&Page::List(pinned.id)));
        assert!(!collapsed_pages.contains(&Page::List(unpinned.id)));
        assert_eq!(collapsed.active_data::<Page>(), Some(&Page::Lists));
        assert!(collapsed_pages.contains(&Page::Matrix));
    }

    #[test]
    fn priority_accessible_names_include_meaning_and_exactly_one_selected_choice() {
        for selected in Priority::ALL {
            let names = Priority::ALL.map(|p| priority_accessible_name(selected, p));
            assert_eq!(
                names
                    .iter()
                    .filter(|name| name.contains("(selected)"))
                    .count(),
                1
            );
            for (priority, name) in Priority::ALL.into_iter().zip(names) {
                assert!(name.contains(priority.label()));
                assert_eq!(name.contains("(selected)"), priority == selected);
            }
        }
    }

    #[test]
    fn priority_selector_indices_cover_all_existing_priorities() {
        assert_eq!(Priority::default(), Priority::P0);
        assert_eq!(priority_at(0), Some(Priority::P0));
        assert_eq!(priority_at(1), Some(Priority::P1));
        assert_eq!(priority_at(2), Some(Priority::P2));
        assert_eq!(priority_at(3), Some(Priority::P3));
        assert_eq!(priority_at(4), None);
        for selected in Priority::ALL {
            assert!(priority_selected(selected, selected));
            assert_eq!(
                Priority::ALL
                    .iter()
                    .filter(|option| priority_selected(selected, **option))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn title_validation_rejects_empty_or_whitespace_and_recovers_on_valid_text() {
        assert_eq!(
            title_validation_error(""),
            Some("A task title is required.")
        );
        assert_eq!(
            title_validation_error(" \t "),
            Some("A task title is required.")
        );
        assert_eq!(title_validation_error(" Write report "), None);
    }

    #[test]
    fn shared_editor_fields_preserve_matrix_and_task_assignment_data() {
        let mut existing = matrix::draft(Some("work"), tasks::Quadrant::ImportantUrgent).unwrap();
        existing.title = "Old title".into();
        existing.priority = Priority::P3;
        existing.matrix_order = Some(4);
        let edited = apply_editor_fields(
            existing,
            "  Revised title  ",
            Some("2026-02-03".into()),
            " notes ".into(),
        );
        assert_eq!(edited.title, "Revised title");
        assert_eq!(edited.due_date.as_deref(), Some("2026-02-03"));
        assert_eq!(edited.notes.as_deref(), Some(" notes "));
        assert_eq!(edited.list_id.as_deref(), Some("work"));
        assert_eq!(edited.priority, Priority::P3);
        assert_eq!(edited.priority.quadrant(), tasks::Quadrant::ImportantUrgent);
        assert_eq!(edited.matrix_order, Some(4));

        let draft = matrix::draft(None, tasks::Quadrant::NotImportantUrgent).unwrap();
        let created = apply_editor_fields(draft, "Matrix task", None, String::new());
        assert_eq!(
            created.priority.quadrant(),
            tasks::Quadrant::NotImportantUrgent
        );
        assert!(created.matrix_order.is_none());
    }
}
