// SPDX-License-Identifier: MPL-2.0
//! Session validation for native Matrix DnD. No records move before a valid drop.
use crate::{
    matrix::{self, Placement},
    tasks::Task,
};
use cosmic::iced::clipboard::{dnd::DndAction, mime::AsMimeTypes};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

pub const MIME: &str = "application/x-kara-matrix-task-v1";
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub id: String,
    token: u64,
}
impl Payload {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            token: NEXT_TOKEN.fetch_add(1, Ordering::Relaxed),
        }
    }
    pub fn decode(mime: &str, bytes: &[u8], action: DndAction) -> Option<Self> {
        if mime != MIME || action != DndAction::Move || bytes.len() > 4096 {
            return None;
        }
        serde_json::from_slice(bytes).ok()
    }
}
impl AsMimeTypes for Payload {
    fn available(&self) -> Cow<'static, [String]> {
        Cow::Owned(vec![MIME.into()])
    }
    fn as_bytes(&self, mime: &str) -> Option<Cow<'static, [u8]>> {
        (mime == MIME)
            .then(|| Cow::Owned(serde_json::to_vec(self).expect("DnD payload serialization")))
    }
}

#[derive(Debug, Default)]
pub struct State {
    session: Option<(Payload, Task)>,
    pub hover: Option<Placement>,
    ended: bool,
    offered: Option<Placement>,
    ids: RefCell<HashMap<String, cosmic::widget::Id>>,
    pages: HashMap<(Option<String>, crate::tasks::Quadrant), usize>,
}
impl State {
    pub fn page(&self, list: Option<&str>, quadrant: crate::tasks::Quadrant) -> usize {
        self.pages
            .get(&(list.map(str::to_owned), quadrant))
            .copied()
            .unwrap_or(0)
    }
    pub fn set_page(&mut self, list: Option<&str>, quadrant: crate::tasks::Quadrant, page: usize) {
        if !self.pending() {
            self.pages.insert((list.map(str::to_owned), quadrant), page);
        }
    }
    pub fn focus_task(&mut self, tasks: &[Task], id: &str) {
        if let Some(task) = tasks.iter().find(|t| t.id == id) {
            let rows = matrix::ordered(tasks, task.list_id.as_deref(), task.priority.quadrant());
            if let Some(index) = rows.iter().position(|t| t.id == id) {
                self.pages.insert(
                    (task.list_id.clone(), task.priority.quadrant()),
                    index / matrix::PAGE_SIZE,
                );
            }
        }
    }
    pub fn start(&mut self, payload: Payload, task: Task) {
        if payload.id == task.id {
            self.session = Some((payload, task));
        } else {
            self.session = None;
        }
        self.hover = None;
        self.ended = false;
        self.offered = None;
    }
    pub fn cancel(&mut self) {
        self.session = None;
        self.hover = None;
        self.ended = false;
        self.offered = None;
    }
    #[cfg(test)]
    pub(crate) fn cached_handle_count(&self) -> usize {
        self.ids
            .borrow()
            .keys()
            .filter(|key| key.starts_with("matrix-handle:"))
            .count()
    }
    pub fn widget_id(&self, key: String) -> cosmic::widget::Id {
        // Custom named IDs compare equal by name, but DndDestination routes by
        // their changing numeric allocation. Retain the actual ID across views.
        self.ids
            .borrow_mut()
            .entry(key)
            .or_insert_with(cosmic::widget::Id::unique)
            .clone()
    }
    pub fn end(&mut self) {
        // Source-finished may precede destination data. Retain only an already
        // observed physical drop; an ended drag without a drop cannot be replayed.
        if self.offered.is_some() {
            self.ended = true;
            self.hover = None;
        } else {
            self.cancel();
        }
    }
    pub fn source(&self) -> Option<&Task> {
        self.session
            .as_ref()
            .filter(|_| !self.ended)
            .map(|(_, task)| task)
    }
    pub fn pending(&self) -> bool {
        self.session.is_some()
    }
    pub fn allows(&self, tasks: &[Task], list: Option<&str>, placement: &Placement) -> bool {
        let Some((_, original)) = &self.session else {
            return false;
        };
        original.list_id.as_deref() == list
            && placement.list_id.as_deref() == list
            && tasks.iter().any(|task| task == original)
            && placement.before.as_deref() != Some(original.id.as_str())
            && matrix::can_move(tasks, &original.id, placement)
    }
    pub fn offer(&mut self, tasks: &[Task], list: Option<&str>, placement: Placement) {
        self.offered = self.allows(tasks, list, &placement).then_some(placement);
        self.hover = None;
    }
    pub fn request(
        &mut self,
        tasks: &[Task],
        list: Option<&str>,
        placement: &Placement,
        payload: Option<Payload>,
    ) -> Option<String> {
        let valid = self.offered.as_ref() == Some(placement)
            && self
                .session
                .as_ref()
                .is_some_and(|(expected, _)| Some(expected) == payload.as_ref())
            && self.allows(tasks, list, placement);
        let id = valid.then(|| self.session.as_ref().unwrap().0.id.clone());
        self.cancel(); // consume once; duplicate/late data cannot apply twice
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::Quadrant;
    fn fixture() -> (Task, Payload, Placement) {
        let task = Task::new("Drag this").unwrap();
        let payload = Payload::new(&task.id);
        let target = Placement {
            list_id: None,
            quadrant: Quadrant::ImportantUrgent,
            before: None,
        };
        (task, payload, target)
    }
    #[test]
    fn numeric_drag_routing_ids_survive_redraws_and_session_cleanup() {
        let mut state = State::default();
        let first = state.widget_id("target:before-a".into());
        let numeric = cosmic::widget::DndDestination::<()>::with_id(
            cosmic::widget::space::horizontal(),
            first,
            vec![MIME.into()],
        )
        .get_drag_id();
        state.cancel();
        let second = state.widget_id("target:before-a".into());
        let target = cosmic::widget::DndDestination::<()>::with_id(
            cosmic::widget::space::horizontal(),
            second,
            vec![MIME.into()],
        );
        assert_eq!(target.get_drag_id(), numeric);
        let different = state.widget_id("target:before-b".into());
        assert_ne!(
            cosmic::widget::DndDestination::<()>::with_id(
                cosmic::widget::space::horizontal(),
                different,
                vec![MIME.into()]
            )
            .get_drag_id(),
            numeric
        );
    }
    #[test]
    fn mime_payload_requires_exact_type_move_action_and_valid_bounded_data() {
        let (_, payload, _) = fixture();
        let bytes = payload.as_bytes(MIME).unwrap();
        assert_eq!(
            Payload::decode(MIME, &bytes, DndAction::Move),
            Some(payload.clone())
        );
        assert_eq!(payload.available().as_ref(), [MIME.to_string()]);
        assert!(payload.as_bytes("text/plain").is_none());
        for (mime, bytes, action) in [
            ("text/plain", bytes.to_vec(), DndAction::Move),
            (MIME, bytes.to_vec(), DndAction::Copy),
            (MIME, b"{broken".to_vec(), DndAction::Move),
            (MIME, vec![b' '; 4097], DndAction::Move),
        ] {
            assert!(Payload::decode(mime, &bytes, action).is_none());
        }
    }
    #[test]
    fn cancel_no_drop_wrong_session_or_repeated_data_never_authorize_a_move() {
        let (task, payload, target) = fixture();
        let tasks = vec![task.clone()];
        let mut state = State::default();
        state.start(payload.clone(), task.clone());
        assert!(
            state
                .request(&tasks, None, &target, Some(payload.clone()))
                .is_none()
        );
        state.start(payload.clone(), task.clone());
        state.offer(&tasks, None, target.clone());
        state.cancel();
        assert!(
            state
                .request(&tasks, None, &target, Some(payload.clone()))
                .is_none()
        );
        state.start(payload.clone(), task.clone());
        state.offer(&tasks, None, target.clone());
        assert!(
            state
                .request(&tasks, None, &target, Some(Payload::new(&task.id)))
                .is_none()
        );
        state.start(payload.clone(), task);
        state.offer(&tasks, None, target.clone());
        assert!(
            state
                .request(&tasks, None, &target, Some(payload.clone()))
                .is_some()
        );
        assert!(
            state
                .request(&tasks, None, &target, Some(payload))
                .is_none()
        );
        assert_eq!(tasks[0].priority, crate::tasks::Priority::P0);
    }
    #[test]
    fn source_finish_can_precede_drop_data_but_cannot_authorize_without_a_drop() {
        let (task, payload, target) = fixture();
        let tasks = vec![task.clone()];
        let mut state = State::default();
        state.start(payload.clone(), task.clone());
        state.end();
        assert!(!state.pending());
        assert!(
            state
                .request(&tasks, None, &target, Some(payload.clone()))
                .is_none()
        );
        state.start(payload.clone(), task.clone());
        state.offer(&tasks, None, target.clone());
        state.end();
        assert!(state.source().is_none());
        assert_eq!(
            state.request(&tasks, None, &target, Some(payload)),
            Some(task.id)
        );
    }
    #[test]
    fn stale_task_changed_list_or_changed_target_invalidates_the_drop() {
        let (task, payload, target) = fixture();
        for case in 0..3 {
            let mut state = State::default();
            let mut tasks = vec![task.clone()];
            state.start(payload.clone(), task.clone());
            state.offer(&tasks, None, target.clone());
            let list = if case == 1 { Some("other-list") } else { None };
            if case == 0 {
                tasks[0].completed = true;
            }
            let mut requested = target.clone();
            if case == 2 {
                requested.quadrant = Quadrant::ImportantNotUrgent;
            }
            assert!(
                state
                    .request(&tasks, list, &requested, Some(payload.clone()))
                    .is_none()
            );
        }
    }
    #[test]
    fn native_finish_callback_is_only_emitted_for_data_after_a_physical_drop() {
        use cosmic::widget::dnd_destination::State as NativeState;
        let mut state: NativeState<()> = NativeState::new();
        state.on_enter(1., 2., vec![MIME.into()], Some(|_, _, _| false), ());
        let data_callback: Option<fn(String, Vec<u8>) -> bool> = None;
        let (message, _) = state.on_data_received(
            MIME.into(),
            vec![],
            data_callback,
            Some(|_, _, _, _, _| true),
        );
        assert!(message.is_none());
        state.on_action_selected(DndAction::Move, None::<fn(DndAction) -> bool>);
        state.on_drop(None::<fn(f64, f64) -> bool>);
        let (message, _) = state.on_data_received(
            MIME.into(),
            vec![],
            data_callback,
            Some(|_, _, _, _, _| true),
        );
        assert_eq!(message, Some(true));
    }
}
