// SPDX-License-Identifier: MPL-2.0
use crate::{
    automatic, backup, backup_ui, calendar_month, calendar_view, date_entry,
    lifecycle::{Effect, Lifecycle, State},
    matrix, matrix_drag, matrix_view,
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

const APP_ICON: &[u8] = include_bytes!("../resources/branding/kara-logo.png");
const TABLER_ICONS_URL: &str = "https://github.com/tabler/tabler-icons";
const EXTERNAL_LINK_ICON: &[u8] = include_bytes!("../assets/tabler/external-link.svg");
const EDIT_ICON: &[u8] = include_bytes!("../assets/tabler/pencil.svg");
const DELETE_ICON: &[u8] = include_bytes!("../assets/tabler/trash.svg");
const INFO_ICON: &[u8] = include_bytes!("../assets/tabler/info-circle.svg");
const CALENDAR_ICON: &[u8] = include_bytes!("../assets/tabler/calendar-month.svg");
const LIST_ICON: &[u8] = include_bytes!("../assets/tabler/list.svg");
const PIN_ICON: &[u8] = include_bytes!("../assets/tabler/pin.svg");
const PINNED_OFF_ICON: &[u8] = include_bytes!("../assets/tabler/pinned-off.svg");
const INBOX_ICON: &[u8] = include_bytes!("../assets/tabler/inbox.svg");
const TODAY_ICON: &[u8] = include_bytes!("../assets/tabler/calendar-check.svg");
const PINNED_LIST_ICON: &[u8] = include_bytes!("../assets/tabler/pinned.svg");
const SORT_CREATED_ICON: &[u8] = include_bytes!("../assets/tabler/calendar-plus.svg");
const SORT_DUE_ICON: &[u8] = include_bytes!("../assets/tabler/calendar-due.svg");
const PRIORITY_FLAG_ICON: &[u8] = include_bytes!("../assets/tabler/flag-3.svg");
const SORT_NAME_ICON: &[u8] = include_bytes!("../assets/tabler/sort-a-z.svg");
const SORT_ASCENDING_ICON: &[u8] = include_bytes!("../assets/tabler/arrow-up.svg");
const SORT_DESCENDING_ICON: &[u8] = include_bytes!("../assets/tabler/arrow-down.svg");
const MODULE_TASKS_ICON: &[u8] = include_bytes!("../assets/tabler/list-check.svg");
const MODULE_BOARD_ICON: &[u8] = include_bytes!("../assets/tabler/layout-kanban.svg");
const MODULE_MATRIX_ICON: &[u8] = include_bytes!("../assets/tabler/layout-grid.svg");
const MODULE_POMODORO_ICON: &[u8] = include_bytes!("../assets/tabler/stopwatch.svg");
const POMODORO_TIMER_ICON: &[u8] = include_bytes!("../assets/tabler/alarm.svg");
const POMODORO_HISTORY_ICON: &[u8] = include_bytes!("../assets/tabler/history.svg");
const POMODORO_STATISTICS_ICON: &[u8] = include_bytes!("../assets/tabler/chart-bar.svg");
const POMODORO_SETTINGS_ICON: &[u8] = include_bytes!("../assets/tabler/settings.svg");

const MODULE_COUNT: usize = 5;
const MODULES: [(Module, &str); MODULE_COUNT] = [
    (Module::Tasks, "Tasks"),
    (Module::Calendar, "Calendar"),
    (Module::Board, "Board View"),
    (Module::Matrix, "Eisenhower Matrix"),
    (Module::Pomodoro, "Pomodoro"),
];
const MODULE_CELL_HEIGHT: f32 = 36.;
const SIDEBAR_MAX_WIDTH: f32 = 280.;

// Retain the accepted sidebar minimum; five cells share the existing width.
// The spacer keeps native shrink layout measurable before the responsive row.
fn module_selector_min_width(gap: u16, padding: u16) -> f32 {
    4. * 40. + 3. * f32::from(gap) + 2. * f32::from(padding)
}

fn module_cell_width(allocated: f32, gap: u16) -> f32 {
    ((allocated - (MODULE_COUNT + 1) as f32 * f32::from(gap)).max(0.)) / MODULE_COUNT as f32
}

fn module_selector(
    allocated: f32,
    gap: u16,
    active: Module,
) -> Element<'static, cosmic::Action<Message>> {
    let cell_width = module_cell_width(allocated, gap);
    let mut row = widget::row::with_capacity(MODULE_COUNT)
        .width((allocated - 2. * f32::from(gap)).max(0.))
        .height(MODULE_CELL_HEIGHT)
        .spacing(gap);
    for (module, label) in MODULES {
        let selected = active == module;
        let cell = widget::button::custom(
            widget::container(widget::icon(module_icon(module)).size(16))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(cell_width)
        .height(MODULE_CELL_HEIGHT)
        .padding(0)
        .class(module_cell_style(selected))
        .selected(selected)
        .name(label)
        .on_press(cosmic::Action::App(Message::SelectModule(module)));
        row = row.push(widget::tooltip(
            cell,
            widget::text::body(label),
            widget::tooltip::Position::Bottom,
        ));
    }
    widget::container(row)
        .padding(gap)
        .width(allocated)
        .height(MODULE_CELL_HEIGHT + 2. * f32::from(gap))
        .into()
}

#[derive(Clone, Copy)]
enum ModuleCellState {
    Active,
    Hovered,
    Pressed,
}
fn module_cell_appearance(
    theme: &cosmic::Theme,
    selected: bool,
    focused: bool,
    state: ModuleCellState,
) -> widget::button::Style {
    use widget::button::Catalog;
    let class = cosmic::theme::Button::IconVertical;
    let mut style = match state {
        ModuleCellState::Active => theme.active(focused, selected, &class),
        ModuleCellState::Hovered => theme.hovered(focused, selected, &class),
        ModuleCellState::Pressed => theme.pressed(focused, selected, &class),
    };
    style.border_radius = theme.cosmic().radius_s().into();
    if selected {
        let base: cosmic::iced::Color = theme.cosmic().primary_container_color().into();
        let accent: cosmic::iced::Color = theme.cosmic().accent_color().into();
        let amount = match state {
            ModuleCellState::Active => 0.22,
            ModuleCellState::Hovered => 0.30,
            ModuleCellState::Pressed => 0.36,
        };
        style.background = Some(
            cosmic::iced::Color::from_rgb(
                base.r + (accent.r - base.r) * amount,
                base.g + (accent.g - base.g) * amount,
                base.b + (accent.b - base.b) * amount,
            )
            .into(),
        );
        style.overlay = None;
        style.icon_color = Some(theme.cosmic().on_bg_color().into());
        if !focused {
            style.border_width = 1.;
            style.border_color = accent;
        }
    }
    style
}
fn module_cell_style(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            module_cell_appearance(theme, selected, focused, ModuleCellState::Active)
        }),
        hovered: Box::new(move |focused, theme| {
            module_cell_appearance(theme, selected, focused, ModuleCellState::Hovered)
        }),
        pressed: Box::new(move |focused, theme| {
            module_cell_appearance(theme, selected, focused, ModuleCellState::Pressed)
        }),
        disabled: Box::new(|theme| {
            use widget::button::Catalog;
            theme.disabled(&cosmic::theme::Button::IconVertical)
        }),
    }
}

pub struct AppModel {
    core: cosmic::Core,
    lifecycle: Lifecycle,
    tray_connection: Option<zbus::Connection>,
    tray_error: Option<String>,
    nav: nav_bar::Model,
    navigation: ModuleNavigation,
    page: Page,
    lists_expanded: bool,
    matrix_lists_expanded: bool,
    matrix_list: Option<String>,
    matrix_priority_task: Option<String>,
    matrix_drag: matrix_drag::State,
    store: Option<Store>,
    storage_error: Option<String>,
    editor: Option<Editor>,
    delete_id: Option<String>,
    list_editor: Option<ListEditor>,
    delete_list_id: Option<String>,
    today: jiff::civil::Date,
    month_view: calendar_month::State,
    task_sort: tasks::TaskSort,
    task_sort_ascending: bool,
    show_completed: bool,
    backup: backup_ui::State,
    auto_scheduler: automatic::Scheduler,
    auto_error: Option<String>,
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
    Backup(backup_ui::Action),
    AutomaticTick,
    Tray(tray::Event),
    CloseMain,
    WindowCloseRequested(cosmic::iced::window::Id),
    WindowClosed(cosmic::iced::window::Id),
    WindowOpened(cosmic::iced::window::Id),
    FocusToken(cosmic::iced::window::Id, Option<String>),
    Exit,

    SelectModule(Module),
    Matrix(matrix_view::Action),
    Month(calendar_view::Action),
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
    ToggleMatrixLists,
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
    SelectSortMode(usize),
    ShowCompleted(bool),
    About,
    LaunchUrl(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Inbox,
    Today,
    Calendar,
    CalendarInbox,
    CalendarList(String),
    Lists,
    List(String),
    Board,
    BoardLists,
    Matrix,
    MatrixLists,
    MatrixList(String),
    Pomodoro,
    PomodoroHistory,
    PomodoroStatistics,
    PomodoroSettings,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Tasks,
    Calendar,
    Board,
    Matrix,
    Pomodoro,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModuleNavigation {
    active: Module,
    tasks: Page,
    calendar: Page,
    board: Page,
    matrix: Page,
    pomodoro: Page,
}
impl Default for ModuleNavigation {
    fn default() -> Self {
        Self {
            active: Module::Tasks,
            tasks: Page::Inbox,
            calendar: Page::Calendar,
            board: Page::Board,
            matrix: Page::Matrix,
            pomodoro: Page::Pomodoro,
        }
    }
}
impl ModuleNavigation {
    fn remembered(&self, module: Module) -> &Page {
        match module {
            Module::Tasks => &self.tasks,
            Module::Calendar => &self.calendar,
            Module::Board => &self.board,
            Module::Matrix => &self.matrix,
            Module::Pomodoro => &self.pomodoro,
        }
    }

    fn remember(&mut self, page: Page) {
        match page.module() {
            Module::Tasks => self.tasks = page,
            Module::Calendar => self.calendar = page,
            Module::Board => self.board = page,
            Module::Matrix => self.matrix = page,
            Module::Pomodoro => self.pomodoro = page,
        }
    }

    fn switch(&mut self, module: Module, current: &Page) -> Page {
        self.remember(current.clone());
        self.active = module;
        self.remembered(module).clone()
    }
}
impl Page {
    fn module(&self) -> Module {
        match self {
            Self::Inbox | Self::Today | Self::Lists | Self::List(_) => Module::Tasks,
            Self::Calendar | Self::CalendarInbox | Self::CalendarList(_) => Module::Calendar,
            Self::Board | Self::BoardLists => Module::Board,
            Self::Matrix | Self::MatrixLists | Self::MatrixList(_) => Module::Matrix,
            Self::Pomodoro
            | Self::PomodoroHistory
            | Self::PomodoroStatistics
            | Self::PomodoroSettings => Module::Pomodoro,
        }
    }

    fn is_calendar(&self) -> bool {
        self.module() == Module::Calendar
    }
    fn calendar_filter(&self) -> calendar_month::Filter<'_> {
        match self {
            Self::CalendarInbox => calendar_month::Filter::Inbox,
            Self::CalendarList(id) => calendar_month::Filter::List(id),
            _ => calendar_month::Filter::All,
        }
    }

    fn is_matrix(&self) -> bool {
        self.module() == Module::Matrix
    }

    fn title(&self) -> &'static str {
        match self.module() {
            Module::Tasks => match self {
                Self::Inbox => "Inbox",
                Self::Today => "Today",
                Self::Lists => "Lists",
                Self::List(_) => "List",
                _ => unreachable!(),
            },
            Module::Calendar => "Calendar",
            Module::Board => "Board View",
            Module::Matrix => "Eisenhower Matrix",
            Module::Pomodoro => match self {
                Self::Pomodoro => "Focus Timer",
                Self::PomodoroHistory => "History",
                Self::PomodoroStatistics => "Statistics",
                Self::PomodoroSettings => "Settings",
                _ => unreachable!(),
            },
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
            Self::Inbox => Some(TaskView::Inbox),
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
        let navigation = ModuleNavigation::default();
        let page = Page::Inbox;
        let nav = build_nav(&[], &page, Module::Tasks, true, true);
        let about = About::default()
            .name("Kara")
            .icon(icon::from_raster_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .license(env!("CARGO_PKG_LICENSE"))
            .links([("Repository", env!("CARGO_PKG_REPOSITORY"))]);
        let mut app = Self {
            core,
            lifecycle: Lifecycle::default(),
            tray_connection: None,
            tray_error: None,
            nav,
            navigation,
            page,
            lists_expanded: true,
            matrix_lists_expanded: true,
            matrix_list: None,
            matrix_priority_task: None,
            matrix_drag: matrix_drag::State::default(),
            store: None,
            storage_error: None,
            editor: None,
            delete_id: None,
            list_editor: None,
            delete_list_id: None,
            today: jiff::Zoned::now().date(),
            month_view: calendar_month::State::new(jiff::Zoned::now().date()),
            task_sort: tasks::TaskSort::default(),
            task_sort_ascending: tasks::TaskSort::default().default_ascending(),
            show_completed: true,
            backup: backup_ui::State::default(),
            auto_scheduler: automatic::Scheduler::default(),
            auto_error: None,
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
            match page {
                Page::Lists => {
                    self.lists_expanded = !self.lists_expanded;
                }
                Page::MatrixLists => {
                    self.matrix_lists_expanded = !self.matrix_lists_expanded;
                }
                Page::Matrix | Page::MatrixList(_) => {
                    self.matrix_list = matrix_scope_after_page(&page, self.matrix_list.clone());
                }
                _ => {}
            }
            self.page = page;
            self.navigation.active = self.page.module();
            self.navigation.remember(self.page.clone());
        }
        self.matrix_priority_task = None;
        self.today = jiff::Zoned::now().date();
        self.rebuild_nav();
        self.update_title()
    }
    fn nav_bar(&self) -> Option<Element<'_, cosmic::Action<Message>>> {
        if !self.core.nav_bar_active() {
            return None;
        }
        let spacing = cosmic::theme::spacing();
        let active = self.navigation.active;
        let module_row = widget::responsive(move |allocated| {
            module_selector(allocated.width, spacing.space_xxs, active)
        })
        .height(MODULE_CELL_HEIGHT + 2. * f32::from(spacing.space_xxs));
        let nav = cosmic::widget::nav_bar(&self.nav, |id| {
            cosmic::Action::Cosmic(cosmic::app::Action::NavBar(id))
        })
        .on_context(|id| cosmic::Action::Cosmic(cosmic::app::Action::NavBarContext(id)))
        .context_menu(self.nav_context_menu())
        .into_container()
        .width(Length::Shrink)
        .height(Length::Fill);
        let panel = widget::container(
            widget::column::with_capacity(3)
                .push(module_row)
                .push(nav)
                .push(
                    widget::space::horizontal()
                        .width(module_selector_min_width(
                            spacing.space_xxs,
                            spacing.space_xxs,
                        ))
                        .height(Length::Shrink),
                )
                .width(Length::Shrink)
                .height(Length::Fill),
        )
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(
            cosmic::widget::nav_bar::nav_bar_style,
        ));
        let panel = if self.core.is_condensed() {
            panel
        } else {
            panel.max_width(SIDEBAR_MAX_WIDTH)
        };
        Some(panel.into())
    }
    fn nav_context_menu(&self) -> Option<Vec<cosmic::widget::menu::Tree<cosmic::Action<Message>>>> {
        let children = self
            .nav
            .iter()
            .map(|entity| match self.nav.data::<Page>(entity) {
                Some(Page::List(_)) | Some(Page::MatrixList(_)) => {
                    let id = entity;
                    let Some(page) = self.nav.data::<Page>(id) else {
                        return Vec::new();
                    };
                    let list_id = match page {
                        Page::List(list_id) | Page::MatrixList(list_id) => list_id,
                        _ => return Vec::new(),
                    };
                    let pinned = self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().iter().find(|list| &list.id == list_id))
                        .is_some_and(|list| list.pinned);
                    vec![widget::menu::Item::Button(
                        if pinned { "Unpin list" } else { "Pin list" },
                        Some(
                            icon::from_svg_bytes(if pinned { PINNED_OFF_ICON } else { PIN_ICON })
                                .symbolic(true),
                        ),
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
                Some(Page::MatrixLists) => vec![widget::menu::Item::Button(
                    if self.matrix_lists_expanded {
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
    fn header_start(&self) -> Vec<Element<'_, Message>> {
        // Native navigation toggle is inserted immediately before header_start.
        let button = widget::button::icon(
            icon::from_svg_bytes(include_bytes!("../assets/tabler/settings.svg")).symbolic(true),
        )
        .icon_size(16)
        .width(32)
        .height(32)
        .padding(8)
        .class(cosmic::theme::Button::Icon)
        .selected(self.backup.open)
        .name("Settings")
        .on_press_maybe(
            (self.editor.is_none()
                && self.list_editor.is_none()
                && self.delete_id.is_none()
                && self.delete_list_id.is_none()
                && !self.backup.busy)
                .then_some(Message::Backup(if self.backup.open {
                    backup_ui::Action::Close
                } else {
                    backup_ui::Action::Open
                })),
        );
        vec![
            widget::tooltip(
                button,
                widget::text::body("Settings"),
                widget::tooltip::Position::Bottom,
            )
            .gap(8)
            .into(),
        ]
    }
    fn header_end(&self) -> Vec<Element<'_, Message>> {
        vec![
            widget::button::icon(icon::from_svg_bytes(INFO_ICON).symbolic(true))
                .icon_size(16)
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .padding(8)
                .class(cosmic::theme::Button::Icon)
                .selected(self.core.window.show_context && !self.backup.open)
                .name("About Kara")
                .tooltip("About Kara")
                .on_press(Message::About)
                .into(),
        ]
    }
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Message>> {
        if self.backup.open {
            let prefs = self
                .store
                .as_ref()
                .map(|s| s.automatic_preferences())
                .cloned()
                .unwrap_or_default();
            return Some(
                context_drawer::context_drawer(
                    self.backup.settings_view(
                        self.backup_available(),
                        &prefs,
                        self.auto_error.as_deref(),
                    ),
                    Message::Backup(backup_ui::Action::Close),
                )
                .title("Settings"),
            );
        }
        self.core.window.show_context.then(|| {
            let mut drawer = context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::About,
            );
            let credit = widget::list::button(
                widget::row::with_capacity(2)
                    .push(
                        widget::column::with_capacity(2)
                            .push(widget::text::body("Tabler Icons"))
                            .push(widget::text::caption("Interface icons · License: MIT"))
                            .width(Length::Fill),
                    )
                    .push(
                        widget::icon(icon::from_svg_bytes(EXTERNAL_LINK_ICON).symbolic(true))
                            .size(16),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .on_press(Message::LaunchUrl(TABLER_ICONS_URL.into()));
            let credits = widget::settings::section()
                .title("Open Source Credits")
                .add(credit);
            drawer.content = widget::column::with_capacity(2)
                .push(drawer.content)
                .push(credits)
                .spacing(cosmic::theme::spacing().space_m)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into();
            drawer
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
        if let Some(error) = &self.auto_error {
            content = content.push(widget::text::body(error));
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
        if !page.is_matrix() {
            content = content.push(header);
        }
        if page.view().is_some() {
            let mut sort_controls = widget::row::with_capacity(4)
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center);
            for (index, sort) in tasks::TaskSort::ALL.into_iter().enumerate() {
                let selected = sort == self.task_sort;
                let icon = widget::container(widget::icon(task_sort_icon(sort)).size(20))
                    .width(Length::Fixed(24.0))
                    .height(Length::Fixed(24.0))
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center);
                let icon: cosmic::Element<'_, Message> = if selected {
                    let direction = widget::container(
                        widget::icon(sort_direction_icon(self.task_sort_ascending)).size(10),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::End)
                    .align_y(Alignment::Start);
                    cosmic::iced::widget::stack([icon.into(), direction.into()])
                        .width(Length::Fixed(24.0))
                        .height(Length::Fixed(24.0))
                        .into()
                } else {
                    icon.into()
                };
                let order = tasks::sort_order_label(self.task_sort_ascending);
                let label = if selected {
                    format!(
                        "Sort by {} ({order}; selected; click to reverse)",
                        sort.label()
                    )
                } else {
                    format!("Sort by {}", sort.label())
                };
                let button = widget::button::custom(icon)
                    .width(Length::Fixed(36.0))
                    .height(Length::Fixed(36.0))
                    .padding(0)
                    .class(cosmic::theme::Button::IconVertical)
                    .selected(selected)
                    .name(label.clone())
                    .on_press_maybe(self.can_write().then_some(Message::SelectSortMode(index)));
                sort_controls = sort_controls.push(widget::tooltip(
                    button,
                    widget::text::body(label),
                    widget::tooltip::Position::Top,
                ));
            }
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
                            widget::button::icon(icon::from_svg_bytes(DELETE_ICON).symbolic(true))
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
                            .padding([spacing.space_xs, spacing.space_s])
                            .width(Length::Fill)
                            .max_width(880)
                            .class(cosmic::theme::Container::Card),
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
        } else if page.is_calendar() {
            content = content.push(calendar_view::view(
                self.store.as_ref(),
                &self.month_view,
                self.today,
                self.page.calendar_filter(),
                self.can_write(),
            ));
        } else if page.is_matrix() {
            content = content.push(matrix_view::view(
                self.store.as_ref(),
                self.matrix_list.as_deref(),
                self.can_write(),
                self.matrix_priority_task.as_deref(),
                &self.matrix_drag,
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
                                    widget::button::icon(pin_action_icon(list.pinned))
                                        .tooltip(if list.pinned {
                                            "Unpin list"
                                        } else {
                                            "Pin list"
                                        })
                                        .name(if list.pinned {
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
                Page::Board | Page::BoardLists => "Board View is coming later.",
                Page::Pomodoro => "Focus Timer is coming later.",
                Page::PomodoroHistory => "Pomodoro history is coming later.",
                Page::PomodoroStatistics => "Pomodoro statistics are coming later.",
                Page::PomodoroSettings => "Pomodoro settings are coming later.",
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
        if self.backup.open && (self.backup.pending.is_some() || self.backup.clear_open) {
            let (tasks, lists) = self
                .store
                .as_ref()
                .map_or((0, 0), |s| (s.tasks().len(), s.lists().len()));
            return self.backup.view(self.backup_available(), tasks, lists);
        }
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
            let calendar_button = widget::button::icon(calendar_icon())
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
            cosmic::iced::time::every(Duration::from_secs(1)).map(|_| Message::AutomaticTick),
            cosmic::iced::window::close_requests().map(Message::WindowCloseRequested),
        ];
        if let Some(conn) = &self.tray_connection {
            subscriptions.push(tray::subscription(conn.clone()).map(Message::Tray));
        }
        if self.matrix_drag.pending() {
            subscriptions.push(cosmic::iced::event::listen_with(|event, _, _| {
                use cosmic::iced::clipboard::dnd::{DndEvent, SourceEvent};
                match event {
                    cosmic::iced::Event::Dnd(DndEvent::Source(SourceEvent::Cancelled)) => {
                        Some(Message::Matrix(matrix_view::Action::DragCancel))
                    }
                    cosmic::iced::Event::Dnd(DndEvent::Source(SourceEvent::Finished)) => {
                        Some(Message::Matrix(matrix_view::Action::DragEnd))
                    }
                    _ => None,
                }
            }));
        }
        if self.page.is_matrix() && self.matrix_priority_task.is_some() && self.editor.is_none() {
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
        if self.backup.open
            && (self.editor.is_some()
                || self.list_editor.is_some()
                || self.delete_id.is_some()
                || self.delete_list_id.is_some())
        {
            self.editor = None;
            self.list_editor = None;
            self.delete_id = None;
            self.delete_list_id = None;
            return Task::none();
        }
        if self.backup.open {
            if !self.backup.busy {
                self.backup = backup_ui::State::default();
                self.core.window.show_context = false;
            }
            return Task::none();
        }
        self.editor = None;
        self.delete_id = None;
        self.list_editor = None;
        self.delete_list_id = None;
        self.core.window.show_context = false;
        Task::none()
    }
    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::Backup(action) => return self.update_backup(action),
            Message::AutomaticTick => self.automatic_tick(),
            Message::Month(action) if self.page.is_calendar() => match action {
                calendar_view::Action::Previous => self.month_view.navigate(-1),
                calendar_view::Action::Next => self.month_view.navigate(1),
                calendar_view::Action::Select(date) => self.month_view.select(date),
                calendar_view::Action::Today => {
                    self.today = jiff::Zoned::now().date();
                    self.month_view.today(self.today);
                }
                calendar_view::Action::Add if self.can_write() => {
                    return self.update(Message::Add);
                }
                _ => {}
            },
            Message::SelectModule(module) => {
                self.page = self.navigation.switch(module, &self.page);
                self.matrix_priority_task = None;
                self.today = jiff::Zoned::now().date();
                self.rebuild_nav();
                return self.update_title();
            }
            Message::NavContext(NavMenuAction::ToggleLists(id)) => {
                match self.nav.data::<Page>(id) {
                    Some(Page::Lists) => return self.update(Message::ToggleLists),
                    Some(Page::MatrixLists) => return self.update(Message::ToggleMatrixLists),
                    _ => {}
                }
            }
            Message::NavContext(NavMenuAction::ToggleListPin(id)) => {
                match self.nav.data::<Page>(id).cloned() {
                    Some(Page::List(id)) | Some(Page::MatrixList(id)) => {
                        return self.update(Message::ToggleListPin(id));
                    }
                    _ => {}
                }
            }
            Message::Matrix(matrix_view::Action::DragStart(payload, task))
                if self.can_write() && self.page.is_matrix() && self.editor.is_none() =>
            {
                self.matrix_priority_task = None;
                if self
                    .store
                    .as_ref()
                    .is_some_and(|store| store.tasks().contains(&task))
                    && task.list_id == self.matrix_list
                {
                    self.matrix_drag.start(payload, task);
                } else {
                    self.matrix_drag.cancel();
                }
            }
            Message::Matrix(matrix_view::Action::DragHover(target)) => {
                if self.can_write()
                    && self.page.is_matrix()
                    && self.editor.is_none()
                    && self.matrix_drag.source().is_some()
                    && self.store.as_ref().is_some_and(|store| {
                        self.matrix_drag
                            .allows(store.tasks(), self.matrix_list.as_deref(), &target)
                    })
                {
                    self.matrix_drag.hover = Some(target);
                } else {
                    self.matrix_drag.hover = None;
                }
            }
            Message::Matrix(matrix_view::Action::DragOffered(target)) => {
                if self.can_write()
                    && self.page.is_matrix()
                    && self.editor.is_none()
                    && let Some(store) = &self.store
                {
                    self.matrix_drag
                        .offer(store.tasks(), self.matrix_list.as_deref(), target);
                }
            }
            Message::Matrix(matrix_view::Action::DragLeave(target)) => {
                if self.matrix_drag.hover.as_ref() == Some(&target) {
                    self.matrix_drag.hover = None;
                }
            }
            Message::Matrix(matrix_view::Action::DragCancel) => self.matrix_drag.cancel(),
            Message::Matrix(matrix_view::Action::DragEnd) => self.matrix_drag.end(),
            Message::Matrix(matrix_view::Action::Drop(target, mime, bytes, action)) => {
                if self.can_write()
                    && self.page.is_matrix()
                    && self.editor.is_none()
                    && let Some(store) = &mut self.store
                {
                    let payload = matrix_drag::Payload::decode(&mime, &bytes, action);
                    if let Some(id) = self.matrix_drag.request(
                        store.tasks(),
                        self.matrix_list.as_deref(),
                        &target,
                        payload,
                    ) {
                        self.storage_error = store.move_matrix_task(&id, &target).err();
                    }
                }
                self.matrix_drag.cancel();
            }
            Message::Matrix(matrix_view::Action::OpenPriority(id)) if self.can_write() => {
                self.matrix_priority_task = Some(id);
            }
            Message::Matrix(matrix_view::Action::ClosePriority) => {
                self.matrix_priority_task = None;
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
                        } else if self.page == Page::MatrixList(id.clone()) {
                            self.page = Page::Matrix;
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
                        .is_none_or(|list| !list.pinned)
                {
                    self.page = Page::Lists;
                }
                self.rebuild_nav();
                return self.update_title();
            }
            Message::ToggleMatrixLists => {
                self.matrix_lists_expanded = !self.matrix_lists_expanded;
                if !self.matrix_lists_expanded
                    && let Page::MatrixList(id) = &self.page
                    && self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().iter().find(|list| &list.id == id))
                        .is_none_or(|list| !list.pinned)
                {
                    self.page = Page::MatrixLists;
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
                        .is_none_or(|list| !list.pinned)
                {
                    self.page = Page::Lists;
                }
                if !self.matrix_lists_expanded
                    && self.page == Page::MatrixList(id.clone())
                    && self
                        .store
                        .as_ref()
                        .and_then(|store| store.lists().iter().find(|list| list.id == id))
                        .is_none_or(|list| !list.pinned)
                {
                    self.page = Page::MatrixLists;
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
            Message::Add if self.can_write() => match if self.page.is_calendar() {
                calendar_month::draft(self.month_view.selected, self.page.calendar_filter())
            } else {
                self.page.draft_task()
            } {
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
            Message::SelectSortMode(index) if self.can_write() => {
                if let Some(clicked) = tasks::sort_mode_at(index) {
                    let (sort, ascending) = tasks::sort_preferences_after_click(
                        self.task_sort,
                        self.task_sort_ascending,
                        clicked,
                    );
                    self.save_task_preferences(sort, ascending, self.show_completed);
                }
            }
            Message::ShowCompleted(show) if self.can_write() => {
                self.save_task_preferences(self.task_sort, self.task_sort_ascending, show);
            }
            Message::About if !self.backup.busy => {
                if self.backup.open {
                    self.backup = backup_ui::State::default();
                    self.core.window.show_context = true;
                } else {
                    self.core.window.show_context = !self.core.window.show_context;
                }
            }
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
    fn sync_automatic_form(&mut self) {
        if let Some(store) = &self.store {
            let prefs = store.automatic_preferences();
            self.backup.auto_enabled = prefs.enabled;
            self.backup.interval = prefs.interval.clone();
            match automatic::interval_controls(&prefs.interval) {
                Ok((amount, unit)) => {
                    self.backup.amount = amount;
                    self.backup.unit = unit;
                }
                Err(error) => self.backup.interval_error = Some(error),
            }
            let destination = self.store.as_ref().unwrap().automatic_directory();
            let verified = automatic::verify_destination(&destination);
            self.backup.destination_verified = verified.is_ok();
            self.backup.destination_error = verified.err();
            self.backup.destination = Some(destination);
            self.backup.interval_error =
                automatic::configuration_interval(&self.backup.amount, self.backup.unit).err();
            self.backup.retention = prefs.retention.to_string();
            self.backup.retention_error = None;
        }
    }
    fn refresh_after_replacement(&mut self) {
        if let Some(store) = &self.store {
            self.task_sort = store.sort_mode();
            self.task_sort_ascending = store.sort_ascending();
            self.show_completed = store.show_completed();
            self.storage_error = store.check_storage().err();
        }
        self.editor = None;
        self.list_editor = None;
        self.delete_id = None;
        self.delete_list_id = None;
        self.page = Page::Inbox;
        self.navigation = ModuleNavigation::default();
        self.matrix_list = None;
        self.matrix_drag = matrix_drag::State::default();
        self.matrix_priority_task = None;
        self.auto_scheduler = automatic::Scheduler::default();
        self.auto_error = None;
        self.sync_automatic_form();
        self.rebuild_nav();
    }
    fn automatic_tick(&mut self) {
        // Store operations are serialized on the app event loop. Never overlap a
        // manual operation or the confirmation modal with an automatic snapshot.
        if self.backup.busy
            || self.backup.pending.is_some()
            || self.backup.clear_open
            || self.storage_error.is_some()
        {
            return;
        }
        let Some(store) = &mut self.store else {
            return;
        };
        let prefs = store.automatic_preferences().clone();
        if !prefs.enabled {
            return;
        }
        let wall = jiff::Timestamp::now().as_second();
        let result = automatic::boot_seconds().and_then(|boot| {
            if self.auto_scheduler.due(&prefs, wall, boot)? {
                let result = store.automatic_backup(wall);
                self.auto_scheduler.finished(
                    automatic::schedule_seconds(&prefs.interval, wall)?,
                    automatic::boot_seconds()?,
                    result.is_ok() || store.automatic_preferences().last_success == Some(wall),
                );
                result?;
                self.auto_error = None;
            }
            Ok(())
        });
        if let Err(error) = result {
            self.auto_error = Some(format!("Automatic backup failed: {error}"));
        }
    }
    fn update_backup(&mut self, action: backup_ui::Action) -> Task<cosmic::Action<Message>> {
        use backup_ui::Action;
        match action {
            Action::Open
                if self.editor.is_none()
                    && self.list_editor.is_none()
                    && self.delete_id.is_none()
                    && self.delete_list_id.is_none()
                    && !self.backup.open =>
            {
                self.matrix_drag.cancel();
                self.matrix_priority_task = None;
                self.backup.open = true;
                self.core.window.show_context = true;
                self.sync_automatic_form();
            }
            Action::Close if !self.backup.busy => {
                self.backup = backup_ui::State::default();
                self.core.window.show_context = false;
            }
            Action::Export
                if self.backup.open && self.backup_available() && self.backup.pending.is_none() =>
            {
                let bytes = match self.store.as_ref().unwrap().backup_bytes() {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.backup.notice = Some(error);
                        return Task::none();
                    }
                };
                self.backup.busy = true;
                self.backup.notice = None;
                return Task::perform(
                    async move {
                        let Some(path) = backup::choose(true).await? else {
                            return Ok(None);
                        };
                        tokio::task::spawn_blocking(move || {
                            backup::export(&path, &bytes)?;
                            Ok(Some(path))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    |result| cosmic::Action::App(Message::Backup(Action::Exported(result))),
                );
            }
            Action::Restore
                if self.backup.open && self.backup_available() && self.backup.pending.is_none() =>
            {
                self.backup.busy = true;
                self.backup.notice = None;
                return Task::perform(
                    async {
                        let Some(path) = backup::choose(false).await? else {
                            return Ok(None);
                        };
                        tokio::task::spawn_blocking(move || {
                            backup::read(&path).map(|b| Some(std::sync::Arc::new(b)))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    |result| cosmic::Action::App(Message::Backup(Action::Loaded(result))),
                );
            }
            Action::Exported(result) if self.backup.busy && self.backup.open => {
                self.backup.busy = false;
                self.backup.notice = match result {
                    Ok(Some(path)) => Some(format!("Backup exported to {}", path.display())),
                    Ok(None) => None,
                    Err(error) => Some(error),
                };
            }
            Action::Loaded(result) if self.backup.busy && self.backup.open => {
                self.backup.busy = false;
                match result {
                    Ok(backup) => {
                        self.backup.add_preview = backup.as_ref().map(|b| {
                            self.store
                                .as_ref()
                                .ok_or_else(|| "Task storage unavailable.".to_string())
                                .and_then(|s| s.preview_add(b))
                        });
                        self.backup.replace_confirm = false;
                        self.backup.pending = backup;
                    }
                    Err(error) => self.backup.notice = Some(error),
                }
            }
            Action::AutoEnabled(value) if self.backup.open && !self.backup.busy => {
                // Turning off must work even if the staged form/destination is invalid.
                if !value && let Some(store) = &mut self.store {
                    let prefs = store.automatic_preferences().clone();
                    if let Err(error) = store.set_automatic_configuration(
                        false,
                        prefs.interval,
                        prefs.retention,
                        prefs.destination,
                    ) {
                        self.backup.notice = Some(error);
                        return Task::none();
                    }
                    self.auto_scheduler = automatic::Scheduler::default();
                }
                self.backup.auto_enabled = value;
            }
            Action::Amount(value)
                if self.backup.open && self.backup.auto_enabled && !self.backup.busy =>
            {
                self.backup.amount = value;
                self.backup.interval_error =
                    automatic::configuration_interval(&self.backup.amount, self.backup.unit).err();
            }
            Action::Unit(index)
                if self.backup.open && self.backup.auto_enabled && !self.backup.busy =>
            {
                if let Some(unit) = automatic::Unit::ALL.get(index) {
                    self.backup.unit = *unit;
                    self.backup.interval_error =
                        automatic::configuration_interval(&self.backup.amount, self.backup.unit)
                            .err();
                }
            }
            Action::ChooseFolder
                if self.backup.open && self.backup.auto_enabled && self.backup_available() =>
            {
                self.backup.busy = true;
                return Task::perform(
                    async {
                        let Some(path) = backup::choose_folder().await? else {
                            return Ok(None);
                        };
                        tokio::task::spawn_blocking(move || {
                            automatic::verify_destination(&path)?;
                            Ok(Some(path))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    |result| cosmic::Action::App(Message::Backup(Action::FolderChosen(result))),
                );
            }
            Action::FolderChosen(result) if self.backup.open && self.backup.busy => {
                self.backup.busy = false;
                match result {
                    Ok(Some(path)) => {
                        self.backup.destination = Some(path);
                        self.backup.destination_verified = true;
                        self.backup.destination_error = None;
                    }
                    Ok(None) => (),
                    Err(error) => {
                        self.backup.destination_verified = false;
                        self.backup.destination_error = Some(error);
                    }
                }
            }
            Action::Retention(value)
                if self.backup.open && self.backup.auto_enabled && !self.backup.busy =>
            {
                self.backup.retention_error = automatic::retention_count(&value).err();
                self.backup.retention = value;
            }
            Action::SaveAutomatic if self.backup.open && self.backup_available() => {
                if let Err(error) =
                    automatic::configuration_interval(&self.backup.amount, self.backup.unit)
                {
                    self.backup.interval_error = Some(error);
                } else if let Err(error) = automatic::retention_count(&self.backup.retention) {
                    self.backup.retention_error = Some(error);
                } else if !self.backup.auto_enabled || !self.backup.destination_verified {
                    return Task::none();
                } else if let Some(store) = &mut self.store {
                    let Some(destination) = self.backup.destination.clone() else {
                        return Task::none();
                    };
                    if let Err(error) = automatic::verify_destination(&destination) {
                        self.backup.destination_verified = false;
                        self.backup.destination_error = Some(error);
                        return Task::none();
                    }
                    match store.set_automatic_configuration(
                        self.backup.auto_enabled,
                        automatic::configuration_interval(&self.backup.amount, self.backup.unit)
                            .expect("validated interval"),
                        automatic::retention_count(&self.backup.retention)
                            .expect("validated retention"),
                        Some(destination),
                    ) {
                        Ok(()) => {
                            self.auto_scheduler = automatic::Scheduler::default();
                            self.auto_error = None;
                            match store.apply_retention() {
                                Ok(()) => {
                                    self.backup.notice =
                                        Some("Automatic backup settings saved.".into())
                                }
                                Err(error) => {
                                    self.auto_error =
                                        Some(format!("Retention cleanup failed: {error}"));
                                    self.backup.notice = Some("Settings saved; retention cleanup could not complete. Archives preserved.".into());
                                }
                            }
                        }
                        Err(error) => self.backup.notice = Some(error),
                    }
                }
            }
            Action::Clear
                if self.backup.open && self.backup_available() && self.backup.pending.is_none() =>
            {
                self.backup.clear_open = true;
                self.backup.clear_text.clear();
            }
            Action::ClearText(value) if self.backup.clear_open => self.backup.clear_text = value,
            Action::ConfirmClear
                if self.backup.open
                    && self.backup_available()
                    && self.backup.clear_open
                    && self.backup.clear_text == "CLEAR" =>
            {
                let result = self.store.as_mut().unwrap().clear_all_data();
                self.backup.clear_open = false;
                self.backup.clear_text.clear();
                self.backup.notice = Some(match result {
                    Ok(path) => format!("Data cleared. Recovery backup: {}", path.display()),
                    Err(error) => format!("Clear could not complete: {error}"),
                });
                self.month_view = calendar_month::State::new(jiff::Zoned::now().date());
                self.refresh_after_replacement();
                return self.update_title();
            }
            Action::ChooseReplace
                if self.backup.open && self.backup.pending.is_some() && self.backup_available() =>
            {
                self.backup.replace_confirm = true
            }
            Action::AddImport
                if self.backup.open && self.backup_available() && !self.backup.replace_confirm =>
            {
                if let Some(prepared) = &self.backup.pending {
                    match self.store.as_mut().unwrap().add_backup(prepared) {
                        Ok((recovery, summary)) => {
                            self.backup.pending = None;
                            self.backup.add_preview = None;
                            self.backup.notice = Some(format!(
                                "{}{}",
                                summary.description(),
                                recovery
                                    .map(|p| format!("\nRecovery backup: {}", p.display()))
                                    .unwrap_or_else(|| " No changes were needed.".into())
                            ));
                            // Add keeps current preferences, scheduler and navigation context.
                            self.rebuild_nav();
                            return self.update_title();
                        }
                        Err(error) => {
                            let store = self.store.as_ref().unwrap();
                            self.backup.add_preview = Some(store.preview_add(prepared));
                            self.storage_error = store.check_storage().err();
                            self.backup.notice = Some(format!("Add import failed: {error}"));
                        }
                    }
                }
            }
            Action::Confirm
                if self.backup.open && self.backup_available() && self.backup.replace_confirm =>
            {
                if let Some(prepared) = self.backup.pending.take() {
                    let store = self.store.as_mut().unwrap();
                    let result = store.restore_backup(&prepared);
                    self.backup.notice = Some(match result {
                        Ok(path) => format!(
                            "Backup restored. Previous data recovery file: {}",
                            path.display()
                        ),
                        Err(error) => error,
                    });
                    self.refresh_after_replacement();
                    return self.update_title();
                }
            }
            _ => {}
        }
        Task::none()
    }
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
        self.store.is_some()
            && self.storage_error.is_none()
            && !self.backup.busy
            && self.backup.pending.is_none()
            && !self.backup.clear_open
    }
    fn backup_available(&self) -> bool {
        self.store.is_some()
            && self.storage_error.is_none()
            && !self.backup.busy
            && self.editor.is_none()
            && self.list_editor.is_none()
            && self.delete_id.is_none()
            && self.delete_list_id.is_none()
    }
    fn rebuild_nav(&mut self) {
        if let Some(store) = &self.store {
            if matches!(&self.page, Page::CalendarList(id) if !store.lists().iter().any(|list| &list.id == id))
            {
                self.page = Page::Calendar;
            }
            if matches!(&self.navigation.calendar, Page::CalendarList(id) if !store.lists().iter().any(|list| &list.id == id))
            {
                self.navigation.calendar = Page::Calendar;
            }
        }
        self.navigation.active = self.page.module();
        self.navigation.remember(self.page.clone());
        self.nav = build_nav(
            self.store.as_ref().map_or(&[], Store::lists),
            &self.page,
            self.navigation.active,
            self.lists_expanded,
            self.matrix_lists_expanded,
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

fn pin_action_icon(is_pinned: bool) -> icon::Handle {
    icon::from_svg_bytes(if is_pinned { PINNED_OFF_ICON } else { PIN_ICON }).symbolic(true)
}

fn calendar_icon() -> icon::Handle {
    icon::from_svg_bytes(CALENDAR_ICON).symbolic(true)
}

fn list_icon() -> icon::Handle {
    icon::from_svg_bytes(LIST_ICON).symbolic(true)
}

fn module_icon(module: Module) -> icon::Handle {
    let asset = match module {
        Module::Tasks => MODULE_TASKS_ICON,
        Module::Calendar => CALENDAR_ICON,
        Module::Board => MODULE_BOARD_ICON,
        Module::Matrix => MODULE_MATRIX_ICON,
        Module::Pomodoro => MODULE_POMODORO_ICON,
    };
    icon::from_svg_bytes(asset).symbolic(true)
}

fn task_sort_icon(sort: tasks::TaskSort) -> icon::Handle {
    let asset = match sort {
        tasks::TaskSort::CreationDate => SORT_CREATED_ICON,
        tasks::TaskSort::DueDate => SORT_DUE_ICON,
        tasks::TaskSort::Priority => PRIORITY_FLAG_ICON,
        tasks::TaskSort::Name => SORT_NAME_ICON,
    };
    icon::from_svg_bytes(asset).symbolic(true)
}

fn sort_direction_icon(ascending: bool) -> icon::Handle {
    icon::from_svg_bytes(if ascending {
        SORT_ASCENDING_ICON
    } else {
        SORT_DESCENDING_ICON
    })
    .symbolic(true)
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

fn build_nav(
    lists: &[TaskList],
    active: &Page,
    module: Module,
    lists_expanded: bool,
    matrix_lists_expanded: bool,
) -> nav_bar::Model {
    let mut nav = nav_bar::Model::default();
    let add = |nav: &mut nav_bar::Model, page: Page, label: &'static str, nav_icon: icon::Icon| {
        nav.insert().text(label).data(page).icon(nav_icon).id()
    };
    match module {
        Module::Tasks => {
            for (page, label, nav_icon) in [
                (Page::Inbox, "Inbox", icon::icon(inbox_icon())),
                (Page::Today, "Today", icon::icon(today_icon())),
            ] {
                add(&mut nav, page, label, nav_icon);
            }
            let shortcut_ids = insert_pinned_shortcuts(&mut nav, lists, Module::Tasks);
            let lists_id = add(
                &mut nav,
                Page::Lists,
                "Lists",
                icon::from_name(if lists_expanded {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                })
                .icon(),
            );
            nav.divider_above_set(shortcut_ids.first().copied().unwrap_or(lists_id), true);
            nav.indent_set(lists_id, 0);
            if lists_expanded {
                insert_list_rows(&mut nav, lists, Module::Tasks, true);
            } else {
                insert_list_rows(&mut nav, lists, Module::Tasks, false);
            }
        }
        Module::Calendar => {
            add(
                &mut nav,
                Page::Calendar,
                "Month",
                icon::icon(calendar_icon()),
            );
            let heading = nav
                .insert()
                .text("Lists")
                .icon(icon::icon(list_icon()))
                .id();
            nav.enable(heading, false);
            let inbox = add(
                &mut nav,
                Page::CalendarInbox,
                "Inbox",
                icon::icon(inbox_icon()),
            );
            nav.indent_set(inbox, 1);
            let mut ordered: Vec<_> = lists.iter().collect();
            ordered.sort_by_key(|list| !list.pinned);
            for list in ordered {
                let row = nav
                    .insert()
                    .text(list.name.clone())
                    .data(Page::CalendarList(list.id.clone()))
                    .icon(icon::icon(if list.pinned {
                        pinned_list_icon()
                    } else {
                        list_icon()
                    }))
                    .id();
                nav.indent_set(row, 1);
            }
        }
        Module::Board => {
            add(&mut nav, Page::Board, "Inbox", icon::icon(inbox_icon()));
            add(&mut nav, Page::BoardLists, "Lists", icon::icon(list_icon()));
        }
        Module::Matrix => {
            add(&mut nav, Page::Matrix, "Inbox", icon::icon(inbox_icon()));
            insert_pinned_shortcuts(&mut nav, lists, Module::Matrix);
            let lists_id = add(
                &mut nav,
                Page::MatrixLists,
                "Lists",
                icon::from_name(if matrix_lists_expanded {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                })
                .icon(),
            );
            nav.indent_set(lists_id, 0);
            insert_list_rows(&mut nav, lists, Module::Matrix, matrix_lists_expanded);
        }
        Module::Pomodoro => {
            for (page, label, asset) in [
                (Page::Pomodoro, "Focus Timer", POMODORO_TIMER_ICON),
                (Page::PomodoroHistory, "History", POMODORO_HISTORY_ICON),
                (
                    Page::PomodoroStatistics,
                    "Statistics",
                    POMODORO_STATISTICS_ICON,
                ),
                (Page::PomodoroSettings, "Settings", POMODORO_SETTINGS_ICON),
            ] {
                add(
                    &mut nav,
                    page,
                    label,
                    icon::icon(icon::from_svg_bytes(asset).symbolic(true)),
                );
            }
        }
    }

    let selected = nav
        .iter()
        .find(|id| nav.data::<Page>(*id) == Some(active))
        .or_else(|| {
            matches!(active, Page::List(_) | Page::MatrixList(_))
                .then(|| {
                    nav.iter().find(|id| {
                        nav.data::<Page>(*id)
                            == Some(&match module {
                                Module::Tasks => Page::Lists,
                                Module::Matrix => Page::MatrixLists,
                                _ => return false,
                            })
                    })
                })
                .flatten()
        });
    if let Some(selected) = selected {
        nav.activate(selected);
    }
    nav
}

fn insert_pinned_shortcuts(
    nav: &mut nav_bar::Model,
    lists: &[TaskList],
    module: Module,
) -> Vec<nav_bar::Id> {
    lists
        .iter()
        .filter(|list| list.pinned)
        .filter_map(|list| {
            let page = match module {
                Module::Tasks => Page::List(list.id.clone()),
                Module::Matrix => Page::MatrixList(list.id.clone()),
                _ => return None,
            };
            let id = nav
                .insert()
                .text(list.name.clone())
                .data(page)
                .icon(icon::icon(pinned_list_icon()))
                .id();
            nav.indent_set(id, 0);
            Some(id)
        })
        .collect()
}

fn insert_list_rows(nav: &mut nav_bar::Model, lists: &[TaskList], module: Module, expanded: bool) {
    let mut visible: Vec<_> = lists
        .iter()
        .filter(|list| list.visible_in_sidebar(expanded))
        .collect();
    visible.sort_by_key(|list| !list.pinned);
    for list in visible {
        let page = match module {
            Module::Tasks => Page::List(list.id.clone()),
            Module::Matrix => Page::MatrixList(list.id.clone()),
            _ => continue,
        };
        let id = nav
            .insert()
            .text(list.name.clone())
            .data(page)
            .icon(if list.pinned {
                icon::icon(pinned_list_icon())
            } else {
                icon::icon(list_icon())
            })
            .id();
        nav.indent_set(id, 1);
    }
}

fn matrix_scope_after_page(page: &Page, current: Option<String>) -> Option<String> {
    match page {
        Page::Matrix => None,
        Page::MatrixList(id) => Some(id.clone()),
        _ => current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::Application;

    // No init/load: exercise the actual navigation tree without opening task data.
    fn sidebar_fixture() -> AppModel {
        AppModel {
            core: cosmic::Core::default(),
            lifecycle: Lifecycle::default(),
            tray_connection: None,
            tray_error: None,
            nav: build_nav(&[], &Page::Inbox, Module::Tasks, true, true),
            navigation: ModuleNavigation::default(),
            page: Page::Inbox,
            lists_expanded: true,
            matrix_lists_expanded: true,
            matrix_list: None,
            matrix_priority_task: None,
            matrix_drag: matrix_drag::State::default(),
            store: None,
            storage_error: None,
            editor: None,
            delete_id: None,
            list_editor: None,
            delete_list_id: None,
            today: "2026-10-19".parse().unwrap(),
            month_view: calendar_month::State::new("2026-10-19".parse().unwrap()),
            task_sort: tasks::TaskSort::default(),
            task_sort_ascending: tasks::TaskSort::default().default_ascending(),
            show_completed: true,
            backup: backup_ui::State::default(),
            auto_scheduler: automatic::Scheduler::default(),
            auto_error: None,
            about: About::default(),
            title_input: cosmic::iced::widget::Id::unique(),
        }
    }

    #[tokio::test]
    async fn actual_cosmic_sidebar_and_frame_fill_the_selector_bounds() {
        use cosmic::iced::{
            Size,
            advanced::{
                layout::{Limits, Node},
                renderer::Headless,
                widget::Tree,
            },
        };
        let renderer = cosmic::Renderer::new(cosmic::font::default(), 14.into(), Some("tiny-skia"))
            .await
            .expect("locked software renderer must work without a display/GPU");
        let gap = cosmic::theme::spacing().space_xxs;
        fn check(node: &Node, gap: u16) -> Option<f32> {
            // Locate the production panel: selector, native navigation, and
            // zero-height intrinsic spacer. Works inside the full COSMIC frame.
            let column = node.children().first()?;
            let children = column.children();
            if children.len() == 3
                && children[2].size().height == 0.
                && children[0].size().height == MODULE_CELL_HEIGHT + 2. * f32::from(gap)
            {
                let selector = &children[0];
                let container = &selector.children()[0];
                let row = &container.children()[0];
                let width = node.size().width;
                assert_eq!(column.size().width, width);
                assert_eq!(selector.size().width, width);
                assert_eq!(container.size().width, width);
                assert_eq!(row.size().width, width - 2. * f32::from(gap));
                let expected_cell = module_cell_width(width, gap);
                assert_eq!(row.children().len(), MODULE_COUNT);
                for (index, cell) in row.children().iter().enumerate() {
                    assert!((cell.size().width - expected_cell).abs() < 0.01);
                    assert_eq!(cell.size().height, MODULE_CELL_HEIGHT);
                    assert!(
                        (cell.bounds().x - index as f32 * (expected_cell + f32::from(gap))).abs()
                            < 0.01
                    );
                    let center = &cell.children()[0];
                    let icon = &center.children()[0];
                    assert_eq!(icon.size().width, 16.);
                    assert_eq!(icon.size().height, 16.);
                    assert!(
                        (center.bounds().x + icon.bounds().center_x() - expected_cell / 2.).abs()
                            < 0.01
                    );
                    assert!(
                        (center.bounds().y + icon.bounds().center_y() - MODULE_CELL_HEIGHT / 2.)
                            .abs()
                            < 0.01
                    );
                }
                let last = row.children().last().unwrap();
                assert!((last.bounds().x + last.size().width - row.size().width).abs() < 0.01);
                assert!(
                    (MODULE_COUNT as f32 * expected_cell
                        + (MODULE_COUNT - 1) as f32 * f32::from(gap)
                        - row.size().width)
                        .abs()
                        < 0.01
                );
                return Some(width);
            }
            None
        }
        fn find(node: &Node, gap: u16) -> Option<f32> {
            check(node, gap).or_else(|| node.children().iter().find_map(|child| find(child, gap)))
        }
        for module in [Module::Tasks, Module::Calendar, Module::Matrix] {
            let mut app = sidebar_fixture();
            app.navigation.active = module;
            app.page = if module == Module::Tasks {
                Page::Inbox
            } else if module == Module::Calendar {
                Page::Calendar
            } else {
                Page::Matrix
            };
            let long_list = TaskList::new(
                "A deliberately long navigation list label to exercise the sidebar cap",
            )
            .unwrap();
            app.nav = build_nav(&[long_list], &app.page, module, true, true);
            for available in [200_f32, 240., 280., 640., 1024.] {
                let mut nav = cosmic::Application::nav_bar(&app).unwrap();
                let mut tree = Tree::new(&nav);
                for _ in 0..2 {
                    let layout = nav.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &Limits::new(Size::ZERO, Size::new(available, 560.)),
                    );
                    let width = find(&layout, gap).expect("production sidebar must be measured");
                    assert_eq!(width, available.min(SIDEBAR_MAX_WIDTH));
                    println!(
                        "sidebar={width} padding={gap} inner={} cell={} gap={gap}",
                        width - 2. * f32::from(gap),
                        module_cell_width(width, gap)
                    );
                }
            }
            for viewport in [640_f32, 800., 1024., 1440.] {
                let mut frame = cosmic::app::ApplicationExt::view_main(&app);
                let mut tree = Tree::new(&frame);
                let layout = frame.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &Limits::new(Size::ZERO, Size::new(viewport, 768.)),
                );
                let width =
                    find(&layout, gap).expect("full native frame must contain the selector");
                assert_eq!(width, SIDEBAR_MAX_WIDTH);
                println!(
                    "frame={viewport} sidebar={width} inner={} five_cells_plus_gaps={}",
                    width - 2. * f32::from(gap),
                    MODULE_COUNT as f32 * module_cell_width(width, gap)
                        + (MODULE_COUNT - 1) as f32 * f32::from(gap)
                );
            }
        }
    }

    #[tokio::test]
    async fn settings_header_matches_about_glyph_and_button_dimensions() {
        use cosmic::iced::{
            Size,
            advanced::{layout::Limits, renderer::Headless, widget::Tree},
        };
        let app = sidebar_fixture();
        let renderer = cosmic::Renderer::new(cosmic::font::default(), 14.into(), Some("tiny-skia"))
            .await
            .unwrap();
        for mut element in [app.header_start().remove(0), app.header_end().remove(0)] {
            let mut tree = Tree::new(&element);
            let layout = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &Limits::new(Size::ZERO, Size::new(120., 60.)),
            );
            assert_eq!(layout.size(), Size::new(32., 32.));
            fn glyph(node: &cosmic::iced::advanced::layout::Node) -> bool {
                node.size() == Size::new(16., 16.) || node.children().iter().any(glyph)
            }
            assert!(glyph(&layout));
        }
    }
    #[test]
    fn add_choice_preserves_context_preferences_and_does_not_duplicate_on_repeat() {
        use backup_ui::Action;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store.upsert(KaraTask::new("Existing").unwrap()).unwrap();
        store
            .set_preferences(tasks::TaskSort::Priority, true, true)
            .unwrap();
        store
            .set_automatic_preferences(true, "PT6H".into(), 4)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        app.page = Page::Calendar;
        app.rebuild_nav();
        app.task_sort = tasks::TaskSort::Priority;
        app.auto_scheduler.finished(21600, 1000, true);
        let prepared = std::sync::Arc::new(
            backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup")).unwrap(),
        );
        let _ = app.update(Message::Backup(Action::Open));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(Ok(Some(prepared.clone())))));
        assert_eq!(
            app.backup
                .add_preview
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .tasks_added,
            2
        );
        let _ = app.update(Message::Backup(Action::Confirm)); // Replace requires choosing its mode first.
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = app.update(Message::Backup(Action::AddImport));
        assert!(app.backup.pending.is_none());
        assert_eq!(app.page, Page::Calendar);
        assert_eq!(app.task_sort, tasks::TaskSort::Priority);
        assert!(
            app.backup
                .notice
                .as_ref()
                .unwrap()
                .contains("Tasks: 2 added, 0 skipped")
        );
        let store = app.store.as_ref().unwrap();
        assert_eq!(store.tasks().len(), 3);
        assert_eq!(store.automatic_preferences().retention, 4);
        assert!(
            !app.auto_scheduler
                .due(store.automatic_preferences(), 0, 1001)
                .unwrap()
        );
        let added = std::fs::read(&path).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(Ok(Some(prepared)))));
        let _ = app.update(Message::Backup(Action::AddImport));
        assert!(
            app.backup
                .notice
                .as_ref()
                .unwrap()
                .contains("Tasks: 0 added, 2 skipped")
        );
        assert_eq!(std::fs::read(&path).unwrap(), added);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
    }
    #[test]
    fn blocked_add_and_cancel_are_safe_and_replace_requires_a_second_confirmation() {
        use backup_ui::Action;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let prepared = std::sync::Arc::new(
            backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup")).unwrap(),
        );
        let mut list = prepared.data.lists[0].clone();
        list.name = "Different list".into();
        store.upsert_list(list).unwrap();
        let before = std::fs::read(&path).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        let _ = app.update(Message::Backup(Action::Open));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(Ok(Some(prepared.clone())))));
        assert!(app.backup.add_preview.as_ref().unwrap().is_err());
        let _ = app.update(Message::Backup(Action::AddImport));
        assert!(
            app.backup
                .notice
                .as_ref()
                .unwrap()
                .contains("Add import failed")
        );
        assert!(app.dialog().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
        let _ = app.update(Message::Backup(Action::Close));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = app.update(Message::Backup(Action::Open));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(Ok(Some(prepared)))));
        let _ = app.update(Message::Backup(Action::ChooseReplace));
        assert!(app.backup.replace_confirm);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = app.update(Message::Backup(Action::Confirm));
        assert_eq!(app.store.as_ref().unwrap().tasks().len(), 2);
        assert_eq!(app.store.as_ref().unwrap().lists()[0].name, "Work");
    }
    #[test]
    fn schedule_folder_cancel_verification_reopen_disable_and_restart_are_safe() {
        use backup_ui::Action;
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.json");
        let mut app = sidebar_fixture();
        app.store = Some(Store::open(path.clone()).unwrap());
        let _ = app.update(Message::Backup(Action::Open));
        assert!(!app.backup.can_apply(true));
        let amount = app.backup.amount.clone();
        let _ = app.update(Message::Backup(Action::Amount("30".into())));
        assert_eq!(app.backup.amount, amount); // disabled input ignored
        let _ = app.update(Message::Backup(Action::AutoEnabled(true)));
        let _ = app.update(Message::Backup(Action::Unit(5)));
        let _ = app.update(Message::Backup(Action::Amount("1".into())));
        assert!(!app.backup.can_apply(true));
        app.backup.busy = true;
        let previous = app.backup.destination.clone();
        let _ = app.update(Message::Backup(Action::FolderChosen(Ok(None))));
        assert_eq!(app.backup.destination, previous);
        assert!(!app.backup.can_apply(true));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::FolderChosen(Err(
            "Permission denied".into(),
        ))));
        assert!(
            app.backup
                .destination_error
                .as_ref()
                .unwrap()
                .contains("Permission denied")
        );
        assert!(!app.backup.can_apply(true));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::FolderChosen(Ok(Some(
            destination.path().into(),
        )))));
        assert!(app.backup.can_apply(true));
        let _ = app.update(Message::Backup(Action::SaveAutomatic));
        assert_eq!(
            app.store.as_ref().unwrap().automatic_preferences().interval,
            "P1M"
        );
        let _ = app.update(Message::Backup(Action::Close));
        let _ = app.update(Message::Backup(Action::Open));
        assert_eq!(app.backup.amount, "1");
        assert_eq!(app.backup.unit, automatic::Unit::Months);
        assert_eq!(app.backup.destination.as_deref(), Some(destination.path()));
        assert!(app.backup.can_apply(true));
        // Recheck access at Apply, even if a formerly valid chooser result is cached.
        let moved = root.path().join("unavailable");
        std::fs::rename(destination.path(), &moved).unwrap();
        let before = std::fs::read(&path).unwrap();
        let _ = app.update(Message::Backup(Action::SaveAutomatic));
        assert!(!app.backup.destination_verified);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = app.update(Message::Backup(Action::Amount("0".into())));
        let _ = app.update(Message::Backup(Action::AutoEnabled(false)));
        assert!(!app.store.as_ref().unwrap().automatic_preferences().enabled);
        assert!(!app.backup.can_apply(true));
        drop(app.store.take());
        let reopened = Store::open(path).unwrap();
        assert!(!reopened.automatic_preferences().enabled);
        assert_eq!(reopened.automatic_preferences().interval, "P1M");
        assert_eq!(reopened.automatic_directory(), destination.path());
        std::fs::rename(moved, destination.path()).unwrap();
    }
    #[test]
    fn settings_header_and_interval_validation_preserve_saved_preferences() {
        use backup_ui::Action;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut app = sidebar_fixture();
        app.store = Some(Store::open(path.clone()).unwrap());
        assert_eq!(app.header_start().len(), 1);
        assert_eq!(app.header_end().len(), 1);
        let _ = app.update(Message::Backup(Action::Open));
        assert_eq!(app.backup.interval, "PT6H");
        assert!(app.context_drawer().is_some());
        assert!(app.dialog().is_none());
        let _ = app.update(Message::SelectModule(Module::Calendar));
        assert!(app.backup.open && app.core.window.show_context);
        assert_eq!(app.page.module(), Module::Calendar);
        let _ = app.update(Message::Backup(Action::AutoEnabled(true)));
        let _ = app.update(Message::Backup(Action::Unit(1)));
        let _ = app.update(Message::Backup(Action::Amount("30".into())));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::FolderChosen(Ok(Some(
            dir.path().to_path_buf(),
        )))));
        let _ = app.update(Message::Backup(Action::SaveAutomatic));
        assert!(app.store.as_ref().unwrap().automatic_preferences().enabled);
        let before = std::fs::read(&path).unwrap();
        let _ = app.update(Message::Backup(Action::Amount("0".into())));
        let _ = app.update(Message::Backup(Action::SaveAutomatic));
        assert!(app.backup.interval_error.is_some());
        assert_eq!(app.backup.amount, "0");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            app.store.as_ref().unwrap().automatic_preferences().interval,
            "PT30M"
        );
        let _ = app.update(Message::Backup(Action::Close));
        let _ = app.update(Message::Backup(Action::Open));
        assert_eq!(app.backup.interval, "PT30M");
        for value in ["0", "101", "bad"] {
            let _ = app.update(Message::Backup(Action::Retention(value.into())));
            let _ = app.update(Message::Backup(Action::SaveAutomatic));
            assert!(app.backup.retention_error.is_some());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        let _ = app.update(Message::Backup(Action::Retention("3".into())));
        let _ = app.update(Message::Backup(Action::SaveAutomatic));
        assert_eq!(
            app.store
                .as_ref()
                .unwrap()
                .automatic_preferences()
                .retention,
            3
        );
        let _ = app.update(Message::Backup(Action::Close));
        let _ = app.update(Message::Backup(Action::Open));
        assert_eq!(app.backup.retention, "3");
    }
    #[test]
    fn typed_clear_confirmation_cancel_and_stale_editor_messages_are_safe() {
        use backup_ui::Action;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store
            .restore_backup(
                &backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup")).unwrap(),
            )
            .unwrap();
        store
            .set_automatic_preferences(true, "P1D".into(), 10)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        let _ = app.update(Message::Backup(Action::Open));
        let _ = app.update(Message::Backup(Action::Clear));
        for value in ["", "clear", "CLEAR "] {
            let _ = app.update(Message::Backup(Action::ClearText(value.into())));
            let _ = app.update(Message::Backup(Action::ConfirmClear));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        let _ = app.on_escape();
        assert!(!app.backup.open);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
        let _ = app.update(Message::Backup(Action::Open));
        let _ = app.update(Message::Backup(Action::Clear));
        let _ = app.update(Message::Backup(Action::ClearText("CLEAR".into())));
        let _ = app.update(Message::Backup(Action::ConfirmClear));
        assert!(app.backup.notice.as_ref().unwrap().contains("Data cleared"));
        assert!(app.store.as_ref().unwrap().tasks().is_empty());
        assert!(app.store.as_ref().unwrap().lists().is_empty());
        assert!(!app.backup.auto_enabled);
        assert_eq!(app.backup.interval, "PT6H");
        assert_eq!(app.page, Page::Inbox);
        assert!(app.editor.is_none());
        let _ = app.update(Message::Backup(Action::Close));
        let _ = app.update(Message::Title("Stale edit".into()));
        let _ = app.update(Message::Save);
        assert!(app.store.as_ref().unwrap().tasks().is_empty());
        drop(app.store.take());
        assert!(Store::open(path).unwrap().tasks().is_empty());
    }
    #[test]
    fn automatic_tick_runs_without_a_window_and_avoids_overlapping_manual_operations() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("tasks.json")).unwrap();
        store
            .set_automatic_preferences(true, "PT30M".into(), 10)
            .unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        app.core.set_main_window_id(None);
        app.backup.open = true; // Ordinary Settings visibility must not pause automatic backups.
        app.backup.busy = true;
        app.automatic_tick();
        assert!(!dir.path().join("automatic-backups").exists());
        app.backup.busy = false;
        app.backup.clear_open = true;
        app.automatic_tick();
        assert!(!dir.path().join("automatic-backups").exists());
        app.backup.clear_open = false;
        app.automatic_tick();
        assert!(
            app.store
                .as_ref()
                .unwrap()
                .automatic_preferences()
                .last_success
                .is_some()
        );
        let count = std::fs::read_dir(dir.path().join("automatic-backups"))
            .unwrap()
            .count();
        app.automatic_tick();
        assert_eq!(
            std::fs::read_dir(dir.path().join("automatic-backups"))
                .unwrap()
                .count(),
            count
        );
    }
    #[test]
    fn backup_restore_requires_confirmation_and_cancel_never_changes_tasks() {
        use backup_ui::Action;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        store
            .upsert(KaraTask::new("Keep until confirmation").unwrap())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        let _ = app.update(Message::Backup(Action::Open));
        assert!(app.backup.open);
        assert!(app.can_write());
        let _ = app.update(Message::Backup(Action::ChooseReplace));
        let _ = app.update(Message::Backup(Action::Confirm));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = app.update(Message::Add);
        assert!(app.editor.is_some());
        assert!(app.dialog().is_some());
        let _ = app.on_escape();
        assert!(app.backup.open && app.core.window.show_context);
        let prepared = std::sync::Arc::new(
            backup::decode(include_bytes!("../tests/fixtures/backup-v1.kara-backup")).unwrap(),
        );
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(Ok(Some(prepared.clone())))));
        assert!(app.backup.pending.is_some());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(app.dialog().is_some());
        let _ = app.update(Message::Backup(Action::Close));
        assert!(!app.backup.open);
        assert!(app.backup.pending.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        // Only this explicit confirmation replaces the shared records/preferences.
        let _ = app.update(Message::Backup(Action::Open));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(Ok(Some(prepared.clone())))));
        let _ = app.update(Message::Backup(Action::ChooseReplace));
        let _ = app.update(Message::Backup(Action::Confirm));
        assert!(app.backup.pending.is_none());
        assert!(
            app.backup
                .notice
                .as_ref()
                .unwrap()
                .contains("Backup restored")
        );
        assert_eq!(app.task_sort, tasks::TaskSort::Name);
        assert!(!app.task_sort_ascending);
        assert!(!app.show_completed);
        assert_eq!(app.page, Page::Inbox);
        let store = app.store.as_ref().unwrap();
        assert_eq!(store.tasks(), prepared.data.tasks);
        assert_eq!(store.lists(), prepared.data.lists);
        assert_eq!(
            calendar_month::on_date(
                store.tasks(),
                "2024-02-29".parse().unwrap(),
                calendar_month::Filter::All
            )[0]
            .id,
            "portable-a"
        );
        assert_eq!(
            matrix::ordered(
                store.tasks(),
                Some("portable-list"),
                tasks::Quadrant::ImportantUrgent
            )
            .len(),
            2
        );
        let _ = app.update(Message::Backup(Action::Close));
        assert!(app.can_write());
        assert!(
            app.nav
                .iter()
                .any(|id| app.nav.data::<Page>(id) == Some(&Page::List("portable-list".into())))
        );
        let _ = app.update(Message::Backup(Action::ChooseReplace));
        let _ = app.update(Message::Backup(Action::Confirm));
        assert_eq!(app.store.as_ref().unwrap().tasks(), prepared.data.tasks);
    }

    #[test]
    fn backup_cancellation_errors_and_busy_state_preserve_editor_and_store() {
        use backup_ui::Action;
        let dir = tempfile::tempdir().unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(Store::open(dir.path().join("tasks.json")).unwrap());
        let _ = app.update(Message::Add);
        let _ = app.update(Message::Title("Unsaved draft".into()));
        let _ = app.update(Message::Backup(Action::Open));
        assert!(!app.backup.open);
        assert_eq!(app.editor.as_ref().unwrap().task.title, "Unsaved draft");
        let _ = app.update(Message::Cancel);
        let _ = app.update(Message::Backup(Action::Open));
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Close));
        let _ = app.on_escape();
        assert!(app.backup.open && app.backup.busy);
        let _ = app.update(Message::Backup(Action::Loaded(Ok(None))));
        assert!(!app.backup.busy);
        assert!(app.backup.pending.is_none());
        assert!(app.backup.notice.is_none());
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Loaded(
            Err("Invalid backup".into()),
        )));
        assert_eq!(app.backup.notice.as_deref(), Some("Invalid backup"));
        assert!(app.backup.pending.is_none());
        let _ = app.update(Message::Backup(Action::ChooseReplace));
        let _ = app.update(Message::Backup(Action::Confirm));
        assert!(app.store.as_ref().unwrap().tasks().is_empty());
        app.backup.busy = true;
        let _ = app.update(Message::Backup(Action::Exported(Ok(None))));
        assert!(app.backup.notice.is_none());
        let _ = app.on_escape();
        assert!(!app.backup.open);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1); // lock only
    }

    #[test]
    fn calendar_module_navigation_uses_shared_lists_and_leaves_tasks_without_calendar() {
        assert_eq!(
            MODULES.map(|(module, _)| module),
            [
                Module::Tasks,
                Module::Calendar,
                Module::Board,
                Module::Matrix,
                Module::Pomodoro
            ]
        );
        assert!(module_icon(Module::Calendar).symbolic);
        assert!(
            std::str::from_utf8(CALENDAR_ICON)
                .unwrap()
                .contains("icon-tabler-calendar-month")
        );
        let list = TaskList::new("Work").unwrap();
        let tasks = build_nav(
            std::slice::from_ref(&list),
            &Page::Inbox,
            Module::Tasks,
            true,
            true,
        );
        assert!(
            !tasks
                .iter()
                .any(|id| tasks.data::<Page>(id).is_some_and(Page::is_calendar))
        );
        let selected = Page::CalendarList(list.id.clone());
        let nav = build_nav(
            std::slice::from_ref(&list),
            &selected,
            Module::Calendar,
            true,
            true,
        );
        let pages: Vec<_> = nav
            .iter()
            .filter_map(|id| nav.data::<Page>(id).cloned())
            .collect();
        assert_eq!(
            pages,
            vec![Page::Calendar, Page::CalendarInbox, selected.clone()]
        );
        assert_eq!(nav.active_data::<Page>(), Some(&selected));
        let headings: Vec<_> = nav
            .iter()
            .filter(|id| nav.data::<Page>(*id).is_none())
            .collect();
        assert_eq!(headings.len(), 1);
        assert!(!nav.is_enabled(headings[0]));
        let mut navigation = ModuleNavigation::default();
        assert_eq!(
            navigation.switch(Module::Calendar, &Page::Today),
            Page::Calendar
        );
        navigation.remember(selected.clone());
        assert_eq!(navigation.switch(Module::Tasks, &selected), Page::Today);
        assert_eq!(navigation.switch(Module::Calendar, &Page::Today), selected);
    }

    #[test]
    fn calendar_editor_completion_and_restart_share_one_record_with_tasks_and_matrix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut store = Store::open(path.clone()).unwrap();
        let list = TaskList::new("Work").unwrap();
        store.upsert_list(list.clone()).unwrap();
        store
            .set_preferences(tasks::TaskSort::Name, false, false)
            .unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        app.page = Page::CalendarList(list.id.clone());
        app.rebuild_nav();
        let date: Date = "2024-02-29".parse().unwrap();
        let _ = app.update(Message::Month(calendar_view::Action::Select(date)));
        let _ = app.update(Message::Month(calendar_view::Action::Add));
        let editor = app.editor.as_ref().unwrap();
        let id = editor.task.id.clone();
        assert_eq!(editor.due, "2024-02-29");
        assert_eq!(editor.task.list_id.as_deref(), Some(list.id.as_str()));
        assert_eq!(editor.task.priority, Priority::P0);
        let _ = app.update(Message::Title("Created in Calendar".into()));
        let _ = app.update(Message::Save);
        assert!(app.editor.is_none());
        assert_eq!(app.store.as_ref().unwrap().tasks().len(), 1);
        let _ = app.update(Message::Edit(id.clone()));
        let _ = app.update(Message::Title("Edited shared task".into()));
        let _ = app.update(Message::Due("2024-03-01".into()));
        let _ = app.update(Message::Priority(3));
        let _ = app.update(Message::Save);
        let next: Date = "2024-03-01".parse().unwrap();
        let store = app.store.as_ref().unwrap();
        assert!(
            calendar_month::on_date(store.tasks(), date, calendar_month::Filter::All).is_empty()
        );
        assert_eq!(
            calendar_month::on_date(store.tasks(), next, calendar_month::Filter::List(&list.id))[0]
                .id,
            id
        );
        assert_eq!(
            matrix::ordered(
                store.tasks(),
                Some(&list.id),
                tasks::Quadrant::ImportantUrgent
            )[0]
            .id,
            id
        );
        assert_eq!(
            tasks::visible_tasks(store.tasks(), TaskView::List(&list.id), next)[0].title,
            "Edited shared task"
        );
        let _ = app.update(Message::Toggle(id.clone()));
        assert!(
            calendar_month::on_date(
                app.store.as_ref().unwrap().tasks(),
                next,
                calendar_month::Filter::All
            )[0]
            .completed
        );
        drop(app.store.take());
        let restored = Store::open(path.clone()).unwrap();
        assert_eq!(restored.tasks().len(), 1);
        assert_eq!(restored.tasks()[0].id, id);
        assert!(restored.tasks()[0].completed);
        assert_eq!(restored.sort_mode(), tasks::TaskSort::Name);
        assert!(!restored.sort_ascending());
        assert!(!restored.show_completed());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()["version"],
            7
        );
    }

    #[test]
    fn calendar_filters_recover_when_a_shared_list_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("tasks.json")).unwrap();
        let list = TaskList::new("Work").unwrap();
        store.upsert_list(list.clone()).unwrap();
        let mut task = calendar_month::draft(
            "2026-10-19".parse().unwrap(),
            calendar_month::Filter::List(&list.id),
        )
        .unwrap();
        task.title = "Keep me".into();
        let id = task.id.clone();
        store.upsert(task).unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        app.page = Page::CalendarList(list.id.clone());
        app.rebuild_nav();
        // Deletion through existing Tasks list management must also invalidate a remembered Calendar filter.
        let _ = app.update(Message::SelectModule(Module::Tasks));
        let _ = app.update(Message::DeleteList(list.id));
        let _ = app.update(Message::ConfirmDeleteList);
        let _ = app.update(Message::SelectModule(Module::Calendar));
        assert_eq!(app.page, Page::Calendar);
        let store = app.store.as_ref().unwrap();
        assert_eq!(store.tasks()[0].id, id);
        assert!(store.tasks()[0].list_id.is_none());
        assert_eq!(
            calendar_month::on_date(
                store.tasks(),
                "2026-10-19".parse().unwrap(),
                calendar_month::Filter::Inbox
            )
            .len(),
            1
        );
    }

    #[test]
    fn calendar_save_failure_keeps_the_editor_and_original_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let store = Store::open(path.clone()).unwrap();
        let mut app = sidebar_fixture();
        app.store = Some(store);
        app.page = Page::Calendar;
        let _ = app.update(Message::Month(calendar_view::Action::Add));
        let _ = app.update(Message::Title("Do not discard".into()));
        std::fs::write(&path, b"external damaged data").unwrap();
        let _ = app.update(Message::Save);
        assert!(app.storage_error.is_some());
        assert!(app.editor.is_some());
        assert!(app.store.as_ref().unwrap().tasks().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), b"external damaged data");
    }

    #[test]
    fn module_cells_fill_the_measured_sidebar_and_center_their_icons() {
        use cosmic::iced::{
            Element, Size,
            advanced::{layout::Limits, widget::Tree},
            widget::{Button, Column, Container, Row, Space, Tooltip},
        };
        for available in [240_f32, 280., 640., 1024.] {
            for nav_width in [120_f32, 176., 240., 360.] {
                for gap in [4_u16, 8] {
                    let padding = gap;
                    let mut row: Row<'_, (), cosmic::Theme, ()> = Row::new()
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .spacing(gap);
                    for _ in 0..MODULE_COUNT {
                        let cell = Button::new(
                            Container::new(Space::new().width(16).height(16))
                                .center_x(Length::Fill)
                                .center_y(Length::Fill),
                        )
                        .width(Length::FillPortion(1))
                        .height(MODULE_CELL_HEIGHT)
                        .padding(0);
                        row = row.push(Tooltip::new(
                            cell,
                            Space::new().width(16).height(16),
                            cosmic::iced::widget::tooltip::Position::Bottom,
                        ));
                    }
                    let selectors = Container::new(row)
                        .padding(padding)
                        .height(MODULE_CELL_HEIGHT + 2. * f32::from(padding))
                        .width(Length::Fill);
                    let nav = Container::new(Space::new().width(nav_width).height(Length::Fill))
                        .width(Length::Shrink)
                        .height(Length::Fill);
                    let column = Column::new()
                        .push(selectors)
                        .push(nav)
                        .push(
                            Space::new()
                                .width(module_selector_min_width(gap, padding))
                                .height(Length::Shrink),
                        )
                        .width(Length::Shrink)
                        .height(Length::Fill);
                    let mut panel: Element<'_, (), cosmic::Theme, ()> = Container::new(column)
                        .max_width(SIDEBAR_MAX_WIDTH)
                        .height(Length::Fill)
                        .into();
                    let mut tree = Tree::new(&panel);
                    let layout = panel.as_widget_mut().layout(
                        &mut tree,
                        &(),
                        &Limits::new(Size::ZERO, Size::new(available, 560.)),
                    );
                    let expected = nav_width
                        .max(module_selector_min_width(gap, padding))
                        .min(SIDEBAR_MAX_WIDTH)
                        .min(available);
                    assert_eq!(layout.size().width, expected);
                    let children = layout.children()[0].children();
                    assert_eq!(children.len(), 3);
                    assert_eq!(children[2].size().height, 0.);
                    let selector = &children[0];
                    let row = &selector.children()[0];
                    assert_eq!(row.size().width, expected - 2. * f32::from(padding));
                    let cell_width = (row.size().width
                        - (MODULE_COUNT - 1) as f32 * f32::from(gap))
                        / MODULE_COUNT as f32;
                    assert_eq!(row.children().len(), MODULE_COUNT);
                    for (index, cell) in row.children().iter().enumerate() {
                        assert!((cell.size().width - cell_width).abs() < 0.01);
                        assert!(
                            (cell.bounds().x - index as f32 * (cell_width + f32::from(gap))).abs()
                                < 0.01
                        );
                        assert_eq!(cell.size().height, MODULE_CELL_HEIGHT);
                        let centered = &cell.children()[0];
                        let icon = &centered.children()[0];
                        assert!(
                            (centered.bounds().x + icon.bounds().center_x() - cell_width / 2.)
                                .abs()
                                < 0.01
                        );
                        assert!(
                            (centered.bounds().y + icon.bounds().center_y()
                                - MODULE_CELL_HEIGHT / 2.)
                                .abs()
                                < 0.01
                        );
                    }
                    let last = row.children().last().unwrap();
                    assert!((last.bounds().x + last.size().width - row.size().width).abs() < 0.01);
                }
            }
        }
    }

    #[test]
    fn module_cell_selection_has_opaque_fill_and_distinct_hover_and_focus() {
        for theme in [
            cosmic::Theme::light(),
            cosmic::Theme::dark(),
            cosmic::Theme::light_hc(),
            cosmic::Theme::dark_hc(),
        ] {
            let idle = module_cell_appearance(&theme, false, false, ModuleCellState::Active);
            let selected = module_cell_appearance(&theme, true, false, ModuleCellState::Active);
            let hover = module_cell_appearance(&theme, true, false, ModuleCellState::Hovered);
            let pressed = module_cell_appearance(&theme, true, false, ModuleCellState::Pressed);
            let focused = module_cell_appearance(&theme, true, true, ModuleCellState::Active);
            assert_ne!(selected.background, idle.background);
            assert_ne!(hover.background, selected.background);
            assert_ne!(pressed.background, hover.background);
            assert_eq!(selected.border_width, 1.);
            assert_eq!(selected.border_radius, theme.cosmic().radius_s().into());
            assert!(focused.outline_width > selected.outline_width);
            for style in [selected, hover, pressed] {
                let Some(cosmic::iced::Background::Color(color)) = style.background else {
                    panic!("selected cells require an opaque rounded background");
                };
                assert_eq!(color.a, 1.);
                assert!(style.overlay.is_none());
                fn luminance(color: cosmic::iced::Color) -> f32 {
                    let linear = |v: f32| {
                        if v <= 0.04045 {
                            v / 12.92
                        } else {
                            ((v + 0.055) / 1.055).powf(2.4)
                        }
                    };
                    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
                }
                let foreground = luminance(style.icon_color.unwrap());
                let background = luminance(color);
                assert!(
                    (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
                        >= 4.5
                );
            }
        }
    }

    #[test]
    fn shrink_sidebar_retains_width_under_native_flex_compression() {
        use cosmic::iced::{
            Element, Size,
            advanced::{layout::Limits, widget::Tree},
            widget::{Button, Column, Container, Row, Space},
        };
        // Exercise the locked layout engine without GPU/font/display setup.
        // Fixed-size spaces stand in for icon/text contents; the four sizing
        // constraints and enclosing shrink/fill panel mirror nav_bar().
        fn width(available: f32, corrected: bool) -> f32 {
            let child_width = if corrected {
                Length::Shrink
            } else {
                Length::Fill
            };
            let button_width = if corrected {
                Length::Fixed(40.)
            } else {
                Length::FillPortion(1)
            };
            let mut modules: Row<'_, (), cosmic::Theme, ()> =
                Row::new().spacing(4).width(child_width);
            for _ in 0..4 {
                modules = modules.push(
                    Button::new(Space::new().width(16).height(16))
                        .width(button_width)
                        .height(36),
                );
            }
            let selectors = Container::new(modules).padding(4).width(child_width);
            let navigation = Container::new(Space::new().width(176).height(Length::Fill))
                .width(child_width)
                .height(Length::Fill);
            let column = Column::new()
                .push(selectors)
                .push(navigation)
                .width(Length::Shrink)
                .height(Length::Fill);
            let mut panel: Element<'_, (), cosmic::Theme, ()> = Container::new(column)
                .max_width(280)
                .height(Length::Fill)
                .into();
            let mut tree = Tree::new(&panel);
            panel
                .as_widget_mut()
                .layout(
                    &mut tree,
                    &(),
                    &Limits::new(Size::ZERO, Size::new(available, 560.)),
                )
                .size()
                .width
        }
        for available in [240., 280., 640., 1024.] {
            // The old all-fluid children supplied no intrinsic sidebar width.
            assert_eq!(width(available, false), 0.);
            let visible = width(available, true);
            assert!(visible >= 176. && visible <= available.min(280.));
        }
    }

    #[test]
    fn packaged_edit_action_icon_resolves_to_themable_symbolic_svg() {
        assert!(std::str::from_utf8(EDIT_ICON).unwrap().contains("<path"));

        let handle = edit_task_icon();
        assert!(handle.symbolic);
        assert!(matches!(&handle.data, icon::Data::Svg(_)));
    }

    #[test]
    fn official_application_logo_resolves_to_a_raster_icon() {
        assert!(APP_ICON.starts_with(b"\x89PNG\r\n\x1a\n"));
        let dimensions = (
            u32::from_be_bytes(APP_ICON[16..20].try_into().unwrap()),
            u32::from_be_bytes(APP_ICON[20..24].try_into().unwrap()),
        );
        assert_eq!(dimensions, (1254, 1254));
        let handle = icon::from_raster_bytes(APP_ICON);
        assert!(!handle.symbolic);
        assert!(matches!(handle.data, icon::Data::Image(_)));
    }

    #[test]
    fn tabler_credit_targets_the_official_collection() {
        assert_eq!(TABLER_ICONS_URL, "https://github.com/tabler/tabler-icons");
    }

    #[test]
    fn matrix_module_icon_is_the_bundled_four_quadrant_layout_grid() {
        let svg = std::str::from_utf8(MODULE_MATRIX_ICON).unwrap();
        assert!(svg.contains("icon-tabler-layout-grid"));
        assert!(svg.contains("viewBox=\"0 0 24 24\""));
        assert_eq!(svg.matches("<path d=").count(), 4);

        let handle = module_icon(Module::Matrix);
        assert!(handle.symbolic);
        assert!(matches!(handle.data, icon::Data::Svg(_)));
    }

    #[test]
    fn task_navigation_assets_are_bundled_symbolic_svgs() {
        for (asset, handle) in [
            (
                EXTERNAL_LINK_ICON,
                icon::from_svg_bytes(EXTERNAL_LINK_ICON).symbolic(true),
            ),
            (EDIT_ICON, edit_task_icon()),
            (
                DELETE_ICON,
                icon::from_svg_bytes(DELETE_ICON).symbolic(true),
            ),
            (INFO_ICON, icon::from_svg_bytes(INFO_ICON).symbolic(true)),
            (CALENDAR_ICON, calendar_icon()),
            (LIST_ICON, list_icon()),
            (PIN_ICON, pin_action_icon(false)),
            (PINNED_OFF_ICON, pin_action_icon(true)),
            (INBOX_ICON, inbox_icon()),
            (TODAY_ICON, today_icon()),
            (PINNED_LIST_ICON, pinned_list_icon()),
            (
                SORT_CREATED_ICON,
                task_sort_icon(tasks::TaskSort::CreationDate),
            ),
            (SORT_DUE_ICON, task_sort_icon(tasks::TaskSort::DueDate)),
            (
                PRIORITY_FLAG_ICON,
                task_sort_icon(tasks::TaskSort::Priority),
            ),
            (SORT_NAME_ICON, task_sort_icon(tasks::TaskSort::Name)),
            (SORT_ASCENDING_ICON, sort_direction_icon(true)),
            (SORT_DESCENDING_ICON, sort_direction_icon(false)),
            (MODULE_TASKS_ICON, module_icon(Module::Tasks)),
            (MODULE_BOARD_ICON, module_icon(Module::Board)),
            (MODULE_MATRIX_ICON, module_icon(Module::Matrix)),
            (MODULE_POMODORO_ICON, module_icon(Module::Pomodoro)),
            (
                POMODORO_TIMER_ICON,
                icon::from_svg_bytes(POMODORO_TIMER_ICON).symbolic(true),
            ),
            (
                POMODORO_HISTORY_ICON,
                icon::from_svg_bytes(POMODORO_HISTORY_ICON).symbolic(true),
            ),
            (
                POMODORO_STATISTICS_ICON,
                icon::from_svg_bytes(POMODORO_STATISTICS_ICON).symbolic(true),
            ),
            (
                POMODORO_SETTINGS_ICON,
                icon::from_svg_bytes(POMODORO_SETTINGS_ICON).symbolic(true),
            ),
        ] {
            let svg = std::str::from_utf8(asset).unwrap();
            assert!(svg.contains("<path"));
            assert!(svg.contains("stroke=\"currentColor\""));
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
        for page in [Page::Inbox, Page::Today, Page::Calendar, Page::Lists] {
            assert!(page.draft_task().unwrap().list_id.is_none());
        }
    }
    #[test]
    fn contextual_sidebar_builds_only_the_selected_module_navigation() {
        let unpinned_first = TaskList::new("Unpinned first").unwrap();
        let mut pinned_first = TaskList::new("Pinned first").unwrap();
        pinned_first.pinned = true;
        let unpinned_second = TaskList::new("Unpinned second").unwrap();
        let mut pinned_second = TaskList::new("Pinned second").unwrap();
        pinned_second.pinned = true;
        let lists = [
            unpinned_first.clone(),
            pinned_first.clone(),
            unpinned_second.clone(),
            pinned_second.clone(),
        ];
        let active = Page::List(pinned_first.id.clone());
        let expanded = build_nav(&lists, &active, Module::Tasks, true, true);
        let pages: Vec<_> = expanded
            .iter()
            .map(|id| expanded.data::<Page>(id).unwrap().clone())
            .collect();

        assert_eq!(
            pages,
            vec![
                Page::Inbox,
                Page::Today,
                Page::List(pinned_first.id.clone()),
                Page::List(pinned_second.id.clone()),
                Page::Lists,
                Page::List(pinned_first.id.clone()),
                Page::List(pinned_second.id.clone()),
                Page::List(unpinned_first.id.clone()),
                Page::List(unpinned_second.id.clone()),
            ]
        );
        let first_shortcut = expanded
            .iter()
            .find(|id| expanded.data::<Page>(*id) == Some(&active))
            .unwrap();
        assert_eq!(expanded.divider_above(first_shortcut), Some(true));
        assert_eq!(expanded.active_data::<Page>(), Some(&active));
        assert_eq!(expanded.indent(expanded.active()), Some(0));
        assert_eq!(
            pages.iter().filter(|page| **page == active).count(),
            2,
            "shortcut and expanded entry must target the same list"
        );

        let collapsed = build_nav(&lists, &Page::Lists, Module::Tasks, false, true);
        let collapsed_pages: Vec<_> = collapsed
            .iter()
            .map(|id| collapsed.data::<Page>(id).unwrap().clone())
            .collect();
        assert_eq!(
            collapsed_pages,
            vec![
                Page::Inbox,
                Page::Today,
                Page::List(pinned_first.id.clone()),
                Page::List(pinned_second.id.clone()),
                Page::Lists,
            ]
        );
        assert_eq!(collapsed.active_data::<Page>(), Some(&Page::Lists));
        let collapsed_pinned = build_nav(
            &lists,
            &Page::List(pinned_first.id.clone()),
            Module::Tasks,
            false,
            true,
        );
        assert_eq!(
            collapsed_pinned.active_data::<Page>(),
            Some(&Page::List(pinned_first.id.clone()))
        );
        let collapsed_unpinned = build_nav(
            &lists,
            &Page::List(unpinned_first.id.clone()),
            Module::Tasks,
            false,
            true,
        );
        assert_eq!(collapsed_unpinned.active_data::<Page>(), Some(&Page::Lists));

        let matrix = build_nav(
            &lists,
            &Page::MatrixList(pinned_first.id.clone()),
            Module::Matrix,
            true,
            true,
        );
        assert_eq!(
            matrix.active_data::<Page>(),
            Some(&Page::MatrixList(pinned_first.id.clone()))
        );
        assert!(
            !matrix
                .iter()
                .any(|id| matrix.data::<Page>(id) == Some(&Page::Today))
        );
        assert!(
            matrix
                .iter()
                .any(|id| matrix.data::<Page>(id) == Some(&Page::Matrix))
        );
        let matrix_pages: Vec<_> = matrix
            .iter()
            .filter_map(|id| matrix.data::<Page>(id))
            .cloned()
            .collect();
        assert_eq!(
            matrix_pages,
            vec![
                Page::Matrix,
                Page::MatrixList(pinned_first.id.clone()),
                Page::MatrixList(pinned_second.id.clone()),
                Page::MatrixLists,
                Page::MatrixList(pinned_first.id.clone()),
                Page::MatrixList(pinned_second.id.clone()),
                Page::MatrixList(unpinned_first.id.clone()),
                Page::MatrixList(unpinned_second.id.clone()),
            ]
        );
        let matrix_collapsed = build_nav(&lists, &Page::MatrixLists, Module::Matrix, true, false);
        let matrix_collapsed_pages: Vec<_> = matrix_collapsed
            .iter()
            .filter_map(|id| matrix_collapsed.data::<Page>(id))
            .cloned()
            .collect();
        assert_eq!(
            matrix_collapsed_pages,
            vec![
                Page::Matrix,
                Page::MatrixList(pinned_first.id.clone()),
                Page::MatrixList(pinned_second.id.clone()),
                Page::MatrixLists,
            ]
        );

        let board = build_nav(&lists, &Page::BoardLists, Module::Board, true, true);
        let board_pages: Vec<_> = board
            .iter()
            .map(|id| board.data::<Page>(id).unwrap().clone())
            .collect();
        assert_eq!(board_pages, vec![Page::Board, Page::BoardLists]);

        let pomodoro = build_nav(
            &lists,
            &Page::PomodoroSettings,
            Module::Pomodoro,
            true,
            true,
        );
        let pomodoro_pages: Vec<_> = pomodoro
            .iter()
            .map(|id| pomodoro.data::<Page>(id).unwrap().clone())
            .collect();
        assert_eq!(
            pomodoro_pages,
            vec![
                Page::Pomodoro,
                Page::PomodoroHistory,
                Page::PomodoroStatistics,
                Page::PomodoroSettings,
            ]
        );
        assert_eq!(
            pomodoro.active_data::<Page>(),
            Some(&Page::PomodoroSettings)
        );
    }

    #[test]
    fn module_navigation_remembers_each_modules_last_subsection() {
        let mut navigation = ModuleNavigation::default();
        assert_eq!(navigation.switch(Module::Board, &Page::Today), Page::Board);
        navigation.remember(Page::BoardLists);
        assert_eq!(
            navigation.switch(Module::Matrix, &Page::BoardLists),
            Page::Matrix
        );
        navigation.remember(Page::MatrixList("work".into()));
        assert_eq!(
            navigation.switch(Module::Pomodoro, &Page::MatrixList("work".into())),
            Page::Pomodoro
        );
        navigation.remember(Page::PomodoroSettings);
        assert_eq!(
            navigation.switch(Module::Tasks, &Page::PomodoroSettings),
            Page::Today
        );
        assert_eq!(
            navigation.switch(Module::Matrix, &Page::Today),
            Page::MatrixList("work".into())
        );
        assert_eq!(
            navigation.switch(Module::Board, &Page::MatrixList("work".into())),
            Page::BoardLists
        );
        assert_eq!(
            navigation.switch(Module::Pomodoro, &Page::BoardLists),
            Page::PomodoroSettings
        );
    }

    #[test]
    fn matrix_sidebar_scope_selection_preserves_existing_list_filter_semantics() {
        let current = Some("work".to_string());
        assert_eq!(
            matrix_scope_after_page(&Page::Matrix, current.clone()),
            None
        );
        assert_eq!(
            matrix_scope_after_page(&Page::MatrixList("home".into()), current.clone()),
            Some("home".into())
        );
        assert_eq!(
            matrix_scope_after_page(&Page::MatrixLists, current.clone()),
            current
        );
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
