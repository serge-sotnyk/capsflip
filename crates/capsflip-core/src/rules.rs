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
    /// Copy, paste or cut in the foreground window.
    Clipboard(Clip),
    /// Send exactly this combination to the system.
    Send(Combo),
}

/// A clipboard operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clip {
    Copy,
    Paste,
    Cut,
}

impl Clip {
    /// The keys that do it: Ctrl+C / V / X, or in a terminal, where Ctrl+C
    /// interrupts a program, Ctrl+Insert / Shift+Insert. A terminal has no
    /// cut: Ctrl+X means something else there.
    #[must_use]
    pub const fn combo(self, terminal: bool) -> Option<Combo> {
        let (mods, key) = match (self, terminal) {
            (Self::Copy, false) => (Modifiers::CTRL, Key::C),
            (Self::Paste, false) => (Modifiers::CTRL, Key::V),
            (Self::Cut, false) => (Modifiers::CTRL, Key::X),
            (Self::Copy, true) => (Modifiers::CTRL, Key::INSERT),
            (Self::Paste, true) => (Modifiers::SHIFT, Key::INSERT),
            (Self::Cut, true) => return None,
        };
        Some(Combo { mods, key })
    }
}

/// A key with exactly these modifiers, as the system should see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    pub mods: Modifiers,
    pub key: Key,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    pub trigger: Trigger,
    pub action: Action,
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

    /// The rules in effect: Caps Lock always, and Win+C / V / X with
    /// Win+Shift+V for the clipboard history when `win_shortcuts` is on.
    #[must_use]
    pub fn keyboard(win_shortcuts: bool) -> Self {
        let mut rules = Self::caps_lock();
        if win_shortcuts {
            let rule = |trigger, action| Rule { trigger, action };
            let win = |key| Trigger::Chord(Modifiers::WIN, key);
            let history = Combo {
                mods: Modifiers::WIN,
                key: Key::V,
            };
            rules.0.extend([
                rule(win(Key::C), Action::Clipboard(Clip::Copy)),
                rule(win(Key::V), Action::Clipboard(Clip::Paste)),
                rule(win(Key::X), Action::Clipboard(Clip::Cut)),
                rule(
                    Trigger::Chord(Modifiers::WIN_SHIFT, Key::V),
                    Action::Send(history),
                ),
            ]);
        }
        rules
    }

    /// The action of the first rule with exactly this trigger.
    pub(crate) fn action(&self, trigger: Trigger) -> Option<Action> {
        self.0
            .iter()
            .find_map(|rule| (rule.trigger == trigger).then_some(rule.action))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(key: Key) -> Trigger {
        Trigger::Chord(Modifiers::WIN, key)
    }

    #[test]
    fn the_clipboard_rules_are_there_only_when_switched_on() {
        let off = Rules::keyboard(false);
        assert_eq!(off.action(win(Key::C)), None);
        assert_eq!(off, Rules::caps_lock());

        let on = Rules::keyboard(true);
        assert_eq!(on.action(win(Key::C)), Some(Action::Clipboard(Clip::Copy)));
        assert_eq!(on.action(win(Key::V)), Some(Action::Clipboard(Clip::Paste)));
        assert_eq!(on.action(win(Key::X)), Some(Action::Clipboard(Clip::Cut)));
        assert_eq!(
            on.action(Trigger::Tap(Key::CAPS_LOCK)),
            Some(Action::ToggleLayout)
        );
    }

    #[test]
    fn win_shift_v_reaches_the_clipboard_history_and_win_shift_x_is_left_alone() {
        let on = Rules::keyboard(true);
        let win_v = Combo {
            mods: Modifiers::WIN,
            key: Key::V,
        };
        assert_eq!(
            on.action(Trigger::Chord(Modifiers::WIN_SHIFT, Key::V)),
            Some(Action::Send(win_v))
        );
        assert_eq!(
            on.action(Trigger::Chord(Modifiers::WIN_SHIFT, Key::X)),
            None
        );
    }

    const CTRL: Modifiers = Modifiers::CTRL;

    fn sends(clip: Clip, terminal: bool, mods: Modifiers, key: Key) -> bool {
        clip.combo(terminal) == Some(Combo { mods, key })
    }

    #[test]
    fn copy_paste_and_cut_are_ctrl_c_v_x() {
        assert!(sends(Clip::Copy, false, CTRL, Key::C));
        assert!(sends(Clip::Paste, false, CTRL, Key::V));
        assert!(sends(Clip::Cut, false, CTRL, Key::X));
    }

    #[test]
    fn in_a_terminal_copy_and_paste_use_insert_and_cut_does_nothing() {
        assert!(sends(Clip::Copy, true, CTRL, Key::INSERT));
        assert!(sends(Clip::Paste, true, Modifiers::SHIFT, Key::INSERT));
        assert_eq!(Clip::Cut.combo(true), None);
    }
}
