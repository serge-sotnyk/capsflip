//! The rule table: which trigger runs which action.

use crate::key::{Key, Modifiers};

/// What the user does to fire a rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// The key alone, pressed and released before the hold threshold.
    Tap(Key),
    /// The key alone, kept down until the hold threshold elapses.
    Hold(Key),
    /// The key pressed while exactly these modifiers are held.
    Chord(Modifiers, Key),
}

/// What the shell does when a rule fires.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Switch between the English layout and the secondary layout.
    ToggleLayout,
    /// Toggle the real Caps Lock state.
    ToggleCapsLock,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    pub trigger: Trigger,
    pub action: Action,
}

impl Trigger {
    const fn key(self) -> Key {
        match self {
            Self::Tap(key) | Self::Hold(key) | Self::Chord(_, key) => key,
        }
    }
}

/// The rule table. A new shortcut is a new row here, not new engine code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rules(Vec<Rule>);

impl Rules {
    #[must_use]
    pub const fn new(rules: Vec<Rule>) -> Self {
        Self(rules)
    }

    /// The Caps Lock rules of v0.1.
    #[must_use]
    pub fn caps_lock() -> Self {
        let caps = Key::CAPS_LOCK;
        let rule = |trigger, action| Rule { trigger, action };
        Self(vec![
            rule(Trigger::Tap(caps), Action::ToggleLayout),
            rule(Trigger::Hold(caps), Action::ToggleCapsLock),
            rule(
                Trigger::Chord(Modifiers::SHIFT, caps),
                Action::ToggleCapsLock,
            ),
        ])
    }

    /// The action of the first rule with exactly this trigger.
    pub(crate) fn action(&self, trigger: Trigger) -> Option<Action> {
        self.0
            .iter()
            .find(|rule| rule.trigger == trigger)
            .map(|rule| rule.action)
    }

    /// True when at least one rule is about `key`.
    pub(crate) fn concern(&self, key: Key) -> bool {
        self.0.iter().any(|rule| rule.trigger.key() == key)
    }
}
