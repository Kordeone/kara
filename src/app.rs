// SPDX-License-Identifier: MPL-2.0
use crate::{
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
use std::time::Duration;

const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

pub struct AppModel {
    core: cosmic::Core,
    lifecycle: Lifecycle,
    tray_connection: Option<zbus::Connection>,
    tray_error: Option<String>,
    nav: nav_bar::Model,
    page: Page,
    lists_expanded: bool,
    matrix_list: Option<String>,
    store: Option<Store>,
    storage_error: Option<String>,
    editor: Option<Editor>,
    delete_id: Option<String>,
    list_editor: Option<ListEditor>,
    delete_list_id: Option<String>,
    today: jiff::civil::Date,
    about: About,
    title_input: cosmic::iced::widget::Id,
}
struct Editor {
    task: KaraTask,
    due: String,
    notes: widget::text_editor::Content,
    error: Option<String>,
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
    Priority(usize),
    Notes(widget::text_editor::Action),
    Save,
    RetryStorage,
    DateTick,
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
            store: None,
            storage_error: None,
            editor: None,
            delete_id: None,
            list_editor: None,
            delete_list_id: None,
            today: jiff::Zoned::now().date(),
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
        content = content.push(header);
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
                let visible = tasks::visible_tasks(store.tasks(), view, self.today);
                let completed_start = visible.iter().position(|task| task.completed);
                for (index, task) in visible.iter().enumerate() {
                    if completed_start == Some(index) {
                        rows = rows.push(widget::text::caption("Completed"));
                    }
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
                    let detail = match &task.due_date {
                        Some(d) => format!("Due {d} · {} priority", task.priority.label()),
                        None => format!("{} priority", task.priority.label()),
                    };
                    let task_details = widget::column::with_capacity(2)
                        .spacing(spacing.space_xxs)
                        .push(title)
                        .push(widget::text::caption(detail))
                        .width(Length::Fill);
                    let actions = widget::row::with_capacity(2)
                        .spacing(spacing.space_xxs)
                        .align_y(Alignment::Center)
                        .push(
                            widget::button::icon(icon::from_name("document-edit-symbolic"))
                                .tooltip("Edit task")
                                .on_press_maybe(
                                    self.can_write().then(|| Message::Edit(task.id.clone())),
                                ),
                        )
                        .push(
                            widget::button::icon(icon::from_name("user-trash-symbolic"))
                                .tooltip("Delete task")
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
            let mut fields = widget::column::with_capacity(12)
                .spacing(spacing.space_s)
                .push(widget::text::heading("Task details"));
            if let Some(error) = &editor.error {
                fields = fields.push(widget::text::body(error));
            }
            fields = fields
                .push(widget::text::body("Title"))
                .push(
                    widget::text_input("Task title", &editor.task.title)
                        .id(self.title_input.clone())
                        .on_input(Message::Title)
                        .on_submit(|_| Message::Save)
                        .width(Length::Fill),
                )
                .push(widget::text::body("Due date"))
                .push(
                    widget::text_input("YYYY-MM-DD (optional)", &editor.due)
                        .on_input(Message::Due)
                        .on_submit(|_| Message::Save)
                        .width(Length::Fill),
                )
                .push(widget::text::body("Priority"))
                .push(
                    widget::dropdown(
                        vec!["Low", "Normal", "High"],
                        Priority::ALL
                            .iter()
                            .position(|p| *p == editor.task.priority),
                        Message::Priority,
                    )
                    .width(Length::Fill),
                )
                .push(widget::text::heading("Organization"))
                .push(widget::text::body("Task list"));
            let mut options = vec!["Inbox (no list)".to_string()];
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
            fields = fields
                .push(widget::dropdown(options, selected, Message::AssignList).width(Length::Fill));
            fields = fields.push(widget::text::heading("Notes")).push(
                widget::text_editor::text_editor(&editor.notes)
                    .height(110)
                    .on_action(Message::Notes),
            );
            let mut dialog = widget::dialog()
                .title(if editor.is_new {
                    "Add task"
                } else {
                    "Edit task"
                })
                .control(widget::container(widget::scrollable(fields)).max_height(400))
                .max_width(560)
                .max_height(520)
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
            Message::Matrix(matrix_view::Action::SelectList(index)) => {
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
                match matrix::draft(self.matrix_list.as_deref(), quadrant) {
                    Ok(task) => {
                        self.open_editor(task, true);
                        return widget::text_input::focus(self.title_input.clone());
                    }
                    Err(error) => self.storage_error = Some(error),
                }
            }
            Message::Matrix(matrix_view::Action::Classify(id, quadrant)) if self.can_write() => {
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
                }
            }
            Message::Due(due) => {
                if let Some(e) = &mut self.editor {
                    e.due = due;
                }
            }
            Message::Priority(index) => {
                if let (Some(e), Some(p)) = (&mut self.editor, Priority::ALL.get(index)) {
                    e.task.priority = *p;
                }
            }
            Message::Notes(action) => {
                if let Some(e) = &mut self.editor {
                    e.notes.perform(action);
                }
            }
            Message::Save if self.can_write() => {
                if let Some(e) = &mut self.editor {
                    let mut task = e.task.clone();
                    task.title = task.title.trim().into();
                    let due = e.due.trim();
                    if !due.is_empty()
                        && let Err(error) = tasks::parse_date(due)
                    {
                        e.error = Some(error);
                        return Task::none();
                    }
                    task.due_date = (!due.is_empty()).then(|| due.to_string());
                    let notes = e.notes.text();
                    task.notes = (!notes.trim().is_empty()).then_some(notes);
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
                self.store = Some(store);
                self.storage_error = None;
                self.rebuild_nav();
            }
            Err(error) => {
                self.storage_error = Some(error);
            }
        }
    }
    fn open_editor(&mut self, task: KaraTask, is_new: bool) {
        self.editor = Some(Editor {
            due: task.due_date.clone().unwrap_or_default(),
            notes: widget::text_editor::Content::with_text(
                task.notes.as_deref().unwrap_or_default(),
            ),
            task,
            error: None,
            is_new,
        });
    }
    fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let title = format!("Kara — {}", self.page_title());
        self.set_header_title(self.page_title());
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            Task::none()
        }
    }
}

fn build_nav(lists: &[TaskList], active: &Page, lists_expanded: bool) -> nav_bar::Model {
    let mut nav = nav_bar::Model::default();
    nav.insert()
        .text("Tasks")
        .data(Page::Tasks)
        .icon(icon::from_name("view-list-symbolic"));

    for (page, icon_name) in [
        (Page::Inbox, "mail-inbox-symbolic"),
        (Page::Today, "view-calendar-symbolic"),
        (Page::Calendar, "x-office-calendar-symbolic"),
        (
            Page::Lists,
            if lists_expanded {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            },
        ),
    ] {
        let id = nav
            .insert()
            .text(page.title())
            .data(page.clone())
            .icon(icon::from_name(icon_name))
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
                    .icon(icon::from_name(if list.pinned {
                        "view-pin-symbolic"
                    } else {
                        "view-list-symbolic"
                    }))
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
    fn task_creation_uses_the_selected_list_and_otherwise_inbox() {
        let task = Page::List("selected-list".into()).draft_task().unwrap();
        assert_eq!(task.list_id.as_deref(), Some("selected-list"));
        assert!(!task.completed);
        assert!(task.due_date.is_none());
        assert_eq!(task.priority, Priority::Normal);
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
}
