// SPDX-License-Identifier: MPL-2.0

mod app;
mod date_entry;
mod i18n;
mod lifecycle;
mod matrix;
mod matrix_view;
mod priority_style;
mod storage;
mod tasks;
mod tray;

#[derive(Debug, Clone)]
pub struct Flags {
    token: Option<String>,
}
impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = &'static str;
    type Args = Vec<String>;
    fn action(&self) -> Option<&Self::SubCommand> {
        Some(&"open")
    }
    // libcosmic consumes the standard platform token before dbus_activation;
    // preserve it in the supported action arguments for a window reopened later.
    fn args(&self) -> Vec<&str> {
        self.token.as_deref().into_iter().collect()
    }
}

fn main() -> cosmic::iced::Result {
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Settings for configuring the application window and iced runtime.
    let settings = cosmic::app::Settings::default()
        .exit_on_close(false)
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(640.0)
                .min_height(560.0),
        );

    // Activate the existing process, or start a visible window and tray service.
    cosmic::app::run_single_instance::<app::AppModel>(
        settings,
        Flags {
            token: std::env::var("XDG_ACTIVATION_TOKEN").ok(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::app::CosmicFlags;
    #[test]
    fn repeated_launch_preserves_the_activation_token_in_action_arguments() {
        let flags = Flags {
            token: Some("launch-token".into()),
        };
        assert_eq!(flags.action(), Some(&"open"));
        assert_eq!(flags.args(), vec!["launch-token"]);
        assert!(Flags { token: None }.args().is_empty());
    }
}
