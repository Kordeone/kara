// SPDX-License-Identifier: MPL-2.0
//! Window transitions are explicit because Wayland windows must be destroyed and
//! recreated, and an open request can arrive while the old surface is closing.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Visible,
    Closing,
    Background,
    Opening,
    Quitting,
}
#[derive(Debug, Default)]
pub struct Lifecycle {
    pub state: State,
    pub tray_available: bool,
    reopen: bool,
    token: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Close,
    Open,
    Focus,
    Quit,
}
impl Lifecycle {
    pub fn close(&mut self) -> Effect {
        if matches!(
            self.state,
            State::Closing | State::Background | State::Quitting
        ) {
            return Effect::None;
        }
        if !self.tray_available {
            return self.quit();
        }
        self.state = State::Closing;
        Effect::Close
    }
    pub fn show(&mut self, token: Option<String>) -> Effect {
        if self.state == State::Quitting {
            return Effect::None;
        }
        if token.is_some() {
            self.token = token;
        }
        match self.state {
            State::Closing => {
                self.reopen = true;
                Effect::None
            }
            State::Background => {
                self.state = State::Opening;
                Effect::Open
            }
            State::Opening => Effect::None,
            State::Visible => Effect::Focus,
            State::Quitting => Effect::None,
        }
    }
    pub fn closed(&mut self) -> Effect {
        if self.state == State::Quitting {
            return Effect::None;
        }
        self.state = State::Background;
        if self.reopen || !self.tray_available {
            self.reopen = false;
            self.show(None)
        } else {
            Effect::None
        }
    }
    pub fn opened(&mut self) -> Effect {
        if self.state != State::Opening {
            return Effect::None;
        }
        self.state = State::Visible;
        Effect::Focus
    }
    pub fn availability(&mut self, ready: bool) -> Effect {
        self.tray_available = ready;
        if !ready && matches!(self.state, State::Background | State::Closing) {
            self.show(None)
        } else {
            Effect::None
        }
    }
    pub fn take_token(&mut self) -> Option<String> {
        self.token.take()
    }
    pub fn quit(&mut self) -> Effect {
        self.state = State::Quitting;
        self.token = None;
        Effect::Quit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_is_visible_and_close_without_a_tray_quits() {
        let mut life = Lifecycle::default();
        assert_eq!(life.state, State::Visible);
        assert_eq!(life.close(), Effect::Quit);
        assert_eq!(life.closed(), Effect::None);
    }
    #[test]
    fn close_then_activate_reopens_once_and_consumes_the_token_once() {
        let mut life = Lifecycle::default();
        life.availability(true);
        assert_eq!(life.close(), Effect::Close);
        assert_eq!(life.closed(), Effect::None);
        assert_eq!(life.state, State::Background);
        assert_eq!(life.show(Some("token".into())), Effect::Open);
        assert_eq!(life.show(None), Effect::None);
        assert_eq!(life.opened(), Effect::Focus);
        assert_eq!(life.take_token().as_deref(), Some("token"));
        assert_eq!(life.take_token(), None);
        assert_eq!(life.show(None), Effect::Focus);
    }
    #[test]
    fn activation_and_tray_loss_during_close_wait_for_the_old_window() {
        for lost in [false, true] {
            let mut life = Lifecycle::default();
            life.availability(true);
            life.close();
            assert_eq!(life.close(), Effect::None);
            if lost {
                assert_eq!(life.availability(false), Effect::None);
            } else {
                assert_eq!(life.show(Some("new-token".into())), Effect::None);
            }
            assert_eq!(life.closed(), Effect::Open);
            assert_eq!(life.opened(), Effect::Focus);
            assert_eq!(life.state, State::Visible);
        }
    }
    #[test]
    fn tray_loss_restores_a_background_window_and_recovery_allows_closing_again() {
        let mut life = Lifecycle::default();
        life.availability(true);
        life.close();
        life.closed();
        assert_eq!(life.availability(false), Effect::Open);
        assert_eq!(life.availability(false), Effect::None);
        life.opened();
        assert_eq!(life.availability(true), Effect::None);
        assert_eq!(life.close(), Effect::Close);
    }
    #[test]
    fn quit_is_final_from_every_window_state() {
        for state in [
            State::Visible,
            State::Closing,
            State::Background,
            State::Opening,
        ] {
            let mut life = Lifecycle {
                state,
                tray_available: true,
                ..Default::default()
            };
            assert_eq!(life.quit(), Effect::Quit);
            assert_eq!(life.availability(false), Effect::None);
            assert_eq!(life.show(Some("late".into())), Effect::None);
            assert_eq!(life.opened(), Effect::None);
            assert_eq!(life.closed(), Effect::None);
        }
    }
}
