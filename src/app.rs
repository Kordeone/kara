// SPDX-License-Identifier: MPL-2.0
use crate::{
    storage::{self, Store},
    tasks::{self, Priority, Task as KaraTask, TaskView},
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
    nav: nav_bar::Model,
    store: Option<Store>,
    storage_error: Option<String>,
    editor: Option<Editor>,
    delete_id: Option<String>,
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
#[derive(Debug, Clone)]
pub enum Message {
    Add,
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
#[derive(Clone, Copy)]
enum Page {
    Tasks,
    Inbox,
    Today,
    Completed,
    MyLists,
    Board,
    Matrix,
    Pomodoro,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Tasks | Self::Inbox => "Inbox",
            Self::Today => "Today",
            Self::Completed => "Completed",
            Self::MyLists => "My Lists",
            Self::Board => "Board View",
            Self::Matrix => "Eisenhower Matrix",
            Self::Pomodoro => "Pomodoro",
        }
    }
    fn view(self) -> Option<TaskView> {
        match self {
            Self::Tasks | Self::Inbox => Some(TaskView::Inbox),
            Self::Today => Some(TaskView::Today),
            Self::Completed => Some(TaskView::Completed),
            _ => None,
        }
    }
}
impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = storage::APP_ID;
    fn core(&self) -> &cosmic::Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }
    fn init(core: cosmic::Core, _: ()) -> (Self, Task<cosmic::Action<Message>>) {
        let mut nav = nav_bar::Model::default();
        nav.insert()
            .text("Tasks")
            .data(Page::Tasks)
            .icon(icon::from_name("view-list-symbolic"))
            .activate();
        for page in [Page::Inbox, Page::Today, Page::Completed, Page::MyLists] {
            let id = nav.insert().text(page.title()).data(page).id();
            nav.indent_set(id, 1);
        }
        for (page, name) in [
            (Page::Board, "view-grid-symbolic"),
            (Page::Matrix, "view-app-grid-symbolic"),
            (Page::Pomodoro, "alarm-symbolic"),
        ] {
            nav.insert()
                .text(page.title())
                .data(page)
                .icon(icon::from_name(name));
        }
        let about = About::default()
            .name("Kara")
            .icon(icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .license(env!("CARGO_PKG_LICENSE"))
            .links([("Repository", env!("CARGO_PKG_REPOSITORY"))]);
        let mut app = Self {
            core,
            nav,
            store: None,
            storage_error: None,
            editor: None,
            delete_id: None,
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
    fn header_end(&self) -> Vec<Element<'_, Message>> {
        vec![
            widget::button::text("About Kara")
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
        let page = self.page();
        let mut content = widget::column::with_capacity(5).spacing(spacing.space_m);
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
            .push(widget::text::title1(page.title()))
            .push(widget::space::horizontal());
        if page.view().is_some() {
            header = header.push(
                widget::button::suggested("Add task")
                    .on_press_maybe(self.can_write().then_some(Message::Add)),
            );
        }
        content = content.push(header);
        if let Some(view) = page.view() {
            let mut rows = widget::column::with_capacity(10).spacing(spacing.space_s);
            let mut count = 0;
            if let Some(store) = &self.store {
                for task in store.tasks().iter().filter(|t| t.visible(view, self.today)) {
                    count += 1;
                    let id = task.id.clone();
                    let check = widget::checkbox(task.completed)
                        .label(&task.title)
                        .on_toggle_maybe(
                            self.can_write()
                                .then_some(move |_| Message::Toggle(id.clone())),
                        );
                    let detail = match &task.due_date {
                        Some(d) => format!("Due {d} · {} priority", task.priority.label()),
                        None => format!("{} priority", task.priority.label()),
                    };
                    let label = widget::column::with_capacity(2)
                        .spacing(spacing.space_xxs)
                        .push(check)
                        .push(widget::text::caption(detail))
                        .width(Length::Fill);
                    let row = widget::row::with_capacity(3)
                        .spacing(spacing.space_s)
                        .align_y(Alignment::Center)
                        .push(label)
                        .push(widget::button::text("Edit").on_press_maybe(
                            self.can_write().then(|| Message::Edit(task.id.clone())),
                        ))
                        .push(widget::button::text("Delete").on_press_maybe(
                            self.can_write().then(|| Message::Delete(task.id.clone())),
                        ));
                    rows = rows.push(widget::container(row).padding(spacing.space_s));
                }
            }
            if count == 0 {
                let empty = if self.store.is_none() {
                    "Tasks are unavailable. Resolve the storage error and retry."
                } else {
                    match view {
                        TaskView::Inbox => "Your inbox is empty. Add a task to get started.",
                        TaskView::Today => "No tasks due today or overdue.",
                        TaskView::Completed => "No completed tasks yet.",
                    }
                };
                rows = rows.push(widget::text::body(empty));
            }
            content = content.push(widget::scrollable(rows).height(Length::Fill));
        } else {
            content = content.push(widget::text::body(if matches!(page, Page::MyLists) {
                "Coming in Stage 2."
            } else {
                "Coming later."
            }));
        }
        widget::container(content)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    fn dialog(&self) -> Option<Element<'_, Message>> {
        if let Some(editor) = &self.editor {
            let spacing = cosmic::theme::spacing();
            let mut fields = widget::column::with_capacity(9)
                .spacing(spacing.space_s)
                .push(widget::text::body("Title"))
                .push(
                    widget::text_input("Task title", &editor.task.title)
                        .id(self.title_input.clone())
                        .on_input(Message::Title)
                        .on_submit(|_| Message::Save),
                )
                .push(widget::text::body("Due date (optional)"))
                .push(
                    widget::text_input("YYYY-MM-DD", &editor.due)
                        .on_input(Message::Due)
                        .on_submit(|_| Message::Save),
                )
                .push(widget::text::body("Priority"))
                .push(widget::dropdown(
                    vec!["Low", "Normal", "High"],
                    Priority::ALL
                        .iter()
                        .position(|p| *p == editor.task.priority),
                    Message::Priority,
                ))
                .push(widget::text::body("Notes (optional)"))
                .push(
                    widget::text_editor::text_editor(&editor.notes)
                        .height(110)
                        .on_action(Message::Notes),
                );
            if let Some(error) = &editor.error {
                fields = fields.push(widget::text::body(error));
            }
            let mut dialog = widget::dialog()
                .title(if editor.is_new {
                    "Add task"
                } else {
                    "Edit task"
                })
                .control(widget::container(widget::scrollable(fields)).max_height(430))
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
        cosmic::iced::time::every(Duration::from_secs(30)).map(|_| Message::DateTick)
    }
    fn on_escape(&mut self) -> Task<cosmic::Action<Message>> {
        self.editor = None;
        self.delete_id = None;
        self.core.window.show_context = false;
        Task::none()
    }
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Message>> {
        self.nav.activate(id);
        self.today = jiff::Zoned::now().date();
        self.update_title()
    }
    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::Add if self.can_write() => match KaraTask::new("New task") {
                Ok(mut task) => {
                    task.title.clear();
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
                }
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
    fn page(&self) -> Page {
        self.nav
            .active_data::<Page>()
            .copied()
            .unwrap_or(Page::Tasks)
    }
    fn can_write(&self) -> bool {
        self.store.is_some() && self.storage_error.is_none()
    }
    fn load(&mut self) {
        match storage::data_path().and_then(Store::open) {
            Ok(store) => {
                self.store = Some(store);
                self.storage_error = None;
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
        let title = format!("Kara — {}", self.page().title());
        self.set_header_title(self.page().title().to_string());
        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            Task::none()
        }
    }
}
