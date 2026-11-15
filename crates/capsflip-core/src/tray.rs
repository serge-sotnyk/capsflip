//! What the tray icon shows and offers. The shell draws it with Win32.

use crate::config::ConfigError;

/// A command from the tray menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Pause,
    StartAtLogin,
    OpenConfig,
    ReloadConfig,
    Quit,
}

impl Command {
    pub const ALL: [Self; 5] = [
        Self::Pause,
        Self::StartAtLogin,
        Self::OpenConfig,
        Self::ReloadConfig,
        Self::Quit,
    ];

    /// The identifier of the menu item. It is never zero, because Windows
    /// reports a menu closed without a choice as zero.
    #[must_use]
    pub const fn id(self) -> u32 {
        self as u32 + 1
    }

    #[must_use]
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }
}

/// One line of the tray menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    Item {
        command: Command,
        label: &'static str,
        checked: bool,
    },
    Separator,
}

/// The tray menu, with the check marks for the current state.
#[must_use]
pub fn menu(paused: bool, starts_at_login: bool) -> Vec<Entry> {
    let item = |command, label, checked| Entry::Item {
        command,
        label,
        checked,
    };
    vec![
        item(Command::Pause, "Pause", paused),
        item(Command::StartAtLogin, "Start at login", starts_at_login),
        Entry::Separator,
        item(Command::OpenConfig, "Open config", false),
        item(Command::ReloadConfig, "Reload config", false),
        Entry::Separator,
        item(Command::Quit, "Quit", false),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Active,
    Paused,
}

/// How the tray icon looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Look {
    pub icon: Icon,
    pub tooltip: &'static str,
}

#[must_use]
pub const fn look(paused: bool) -> Look {
    if paused {
        Look {
            icon: Icon::Paused,
            tooltip: "capsflip (paused)",
        }
    } else {
        Look {
            icon: Icon::Active,
            tooltip: "capsflip",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Info,
    Warning,
}

/// A notification shown next to the tray icon.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Notice {
    pub title: &'static str,
    pub text: String,
    pub severity: Severity,
}

/// What to tell the user once capsflip has started, if anything.
#[must_use]
pub fn started(problem: Option<&ConfigError>) -> Option<Notice> {
    problem.map(|problem| Notice {
        title: "Config not applied; using the defaults",
        text: problem.to_string(),
        severity: Severity::Warning,
    })
}

/// What to tell the user after the config was read again.
#[must_use]
pub fn reloaded(result: Result<(), &ConfigError>) -> Notice {
    match result {
        Ok(()) => Notice {
            title: "Config reloaded",
            text: "The new settings are in effect.".to_string(),
            severity: Severity::Info,
        },
        Err(problem) => Notice {
            title: "Config not applied; previous settings kept",
            text: problem.to_string(),
            severity: Severity::Warning,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn items(entries: &[Entry]) -> Vec<(Command, &'static str, bool)> {
        entries
            .iter()
            .filter_map(|entry| match *entry {
                Entry::Item {
                    command,
                    label,
                    checked,
                } => Some((command, label, checked)),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_offers_every_command_in_order() {
        let menu = menu(false, false);
        let commands: Vec<Command> = items(&menu).into_iter().map(|item| item.0).collect();
        assert_eq!(
            commands,
            [
                Command::Pause,
                Command::StartAtLogin,
                Command::OpenConfig,
                Command::ReloadConfig,
                Command::Quit
            ]
        );
        assert_eq!(menu.iter().filter(|e| **e == Entry::Separator).count(), 2);
    }

    #[test]
    fn pause_and_start_at_login_are_checked_when_they_are_on() {
        let checked = |paused, at_login| {
            items(&menu(paused, at_login))
                .into_iter()
                .filter(|item| item.2)
                .map(|item| item.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(checked(false, false), []);
        assert_eq!(checked(true, false), [Command::Pause]);
        assert_eq!(checked(false, true), [Command::StartAtLogin]);
    }

    #[test]
    fn every_command_has_a_label() {
        for (_, label, _) in items(&menu(false, false)) {
            assert_ne!(label, "");
        }
    }

    #[test]
    fn a_command_is_found_by_its_identifier_and_zero_means_none() {
        for command in Command::ALL {
            assert_ne!(command.id(), 0);
            assert_eq!(Command::from_id(command.id()), Some(command));
        }
        assert_eq!(Command::from_id(0), None);
        assert_eq!(Command::from_id(99), None);
    }

    #[test]
    fn the_icon_and_tooltip_show_whether_capsflip_is_paused() {
        assert_eq!(look(false).icon, Icon::Active);
        assert_eq!(look(true).icon, Icon::Paused);
        assert_eq!(look(false).tooltip, "capsflip");
        assert!(look(true).tooltip.contains("paused"));
    }

    fn rejected() -> crate::config::ConfigError {
        Config::parse("[layout]\nenglish = 5\n").unwrap_err()
    }

    #[test]
    fn a_clean_start_shows_nothing() {
        assert_eq!(started(None), None);
    }

    #[test]
    fn a_start_with_a_rejected_config_says_why_and_that_defaults_are_used() {
        let problem = rejected();
        let notice = started(Some(&problem)).expect("a notice");
        assert_eq!(notice.severity, Severity::Warning);
        assert!(notice.title.contains("defaults"), "{}", notice.title);
        assert_eq!(notice.text, problem.to_string());
    }

    #[test]
    fn a_reload_confirms_success() {
        let notice = reloaded(Ok(()));
        assert_eq!(notice.severity, Severity::Info);
        assert!(notice.title.contains("reloaded"), "{}", notice.title);
    }

    #[test]
    fn a_rejected_reload_says_why_and_that_the_previous_settings_stay() {
        let problem = rejected();
        let notice = reloaded(Err(&problem));
        assert_eq!(notice.severity, Severity::Warning);
        assert!(notice.title.contains("previous"), "{}", notice.title);
        assert_eq!(notice.text, problem.to_string());
    }

    #[test]
    fn titles_fit_in_a_windows_notification() {
        let problem = rejected();
        let titles = [
            started(Some(&problem)).unwrap().title,
            reloaded(Ok(())).title,
            reloaded(Err(&problem)).title,
        ];
        for title in titles {
            assert!(title.len() < 64, "{title}");
        }
    }
}
