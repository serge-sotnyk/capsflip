//! The key state machine: turns key events into decisions.

use crate::key::{Key, KeyEvent, KeyKind, Modifiers};
use crate::rules::{Action, Rules, Trigger};

/// Identifies one press, so a late hold timer cannot fire a later press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HoldToken(u32);

/// What the shell must do with a key event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Let the event through untouched.
    Pass,
    /// Swallow the event.
    Swallow,
    /// Swallow the event and run the action.
    Run(Action),
    /// Swallow the event and call [`Engine::on_hold_timer`] with this token
    /// once the hold threshold has elapsed.
    ArmHold(HoldToken),
}

impl Decision {
    #[must_use]
    pub const fn swallows(self) -> bool {
        !matches!(self, Self::Pass)
    }
}

/// How the press in progress will end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Swallowed so far; a release makes it a tap, the timer makes it a hold.
    Undecided(HoldToken),
    /// Its action already ran; the rest of the press is swallowed.
    Spent,
    /// No rule applies; the whole press goes through.
    Passing,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    key: Key,
    phase: Phase,
}

/// Decides, event by event, what happens to the keys that have rules.
#[derive(Debug)]
pub struct Engine {
    rules: Rules,
    /// The keys with rules that are down, one entry per key. Several can be
    /// down at once: Win+X rolls into Win+V before X is released.
    presses: Vec<Press>,
    tokens: u32,
}

impl Engine {
    #[must_use]
    pub const fn new(rules: Rules) -> Self {
        Self {
            rules,
            presses: Vec::new(),
            tokens: 0,
        }
    }

    /// Handles one key event.
    ///
    /// `bypassed` tells whether rules are switched off right now (paused, or
    /// an excluded application is in the foreground). It is asked only for
    /// events that would be swallowed, so it may be expensive: a letter that
    /// some rule uses with Win is typed without it being asked.
    pub fn on_key(&mut self, event: KeyEvent, bypassed: impl FnOnce() -> bool) -> Decision {
        if event.injected {
            return Decision::Pass;
        }
        let decision = self.step(event);
        if decision.swallows() && bypassed() {
            self.forget(event.key);
            return Decision::Pass;
        }
        decision
    }

    /// The transition table: event kind against the press in progress.
    fn step(&mut self, event: KeyEvent) -> Decision {
        match (event.kind, self.phase_of(event.key)) {
            (KeyKind::Down, None) => self.begin(event),
            (KeyKind::Down, Some(Phase::Passing)) | (KeyKind::Up, None) => Decision::Pass,
            (KeyKind::Down, Some(_)) => Decision::Swallow,
            (KeyKind::Up, Some(phase)) => self.end(event.key, phase),
        }
    }

    /// Reports that the hold threshold of the press with this token elapsed.
    pub fn on_hold_timer(&mut self, token: HoldToken) -> Option<Action> {
        let press = self
            .presses
            .iter_mut()
            .find(|press| press.phase == Phase::Undecided(token))?;
        press.phase = Phase::Spent;
        self.rules.action(Trigger::Hold(press.key))
    }

    fn begin(&mut self, event: KeyEvent) -> Decision {
        let (phase, decision) = self.classify(event);
        self.presses.push(Press {
            key: event.key,
            phase,
        });
        decision
    }

    /// Decides how a new press will be handled, from the rules for its key.
    fn classify(&mut self, event: KeyEvent) -> (Phase, Decision) {
        if event.mods != Modifiers::NONE {
            let chord = self.rules.action(Trigger::Chord(event.mods, event.key));
            return chord.map_or((Phase::Passing, Decision::Pass), |action| {
                (Phase::Spent, Decision::Run(action))
            });
        }
        let has_hold = self.rules.action(Trigger::Hold(event.key)).is_some();
        let has_tap = self.rules.action(Trigger::Tap(event.key)).is_some();
        if !has_hold && !has_tap {
            return (Phase::Passing, Decision::Pass);
        }
        self.tokens = self.tokens.wrapping_add(1);
        let token = HoldToken(self.tokens);
        let decision = if has_hold {
            Decision::ArmHold(token)
        } else {
            Decision::Swallow
        };
        (Phase::Undecided(token), decision)
    }

    fn end(&mut self, key: Key, phase: Phase) -> Decision {
        self.forget(key);
        match phase {
            Phase::Passing => Decision::Pass,
            Phase::Spent => Decision::Swallow,
            Phase::Undecided(_) => self
                .rules
                .action(Trigger::Tap(key))
                .map_or(Decision::Swallow, Decision::Run),
        }
    }

    fn phase_of(&self, key: Key) -> Option<Phase> {
        self.presses
            .iter()
            .find(|press| press.key == key)
            .map(|press| press.phase)
    }

    fn forget(&mut self, key: Key) {
        self.presses.retain(|press| press.key != key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Key, KeyEvent, KeyKind, Modifiers};
    use crate::rules::{Action, Clip, Rule, Rules, Trigger};

    const CAPS: Key = Key::CAPS_LOCK;
    const LETTER_A: Key = Key::from_vk(0x41);
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };

    fn event(key: Key, kind: KeyKind, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            kind,
            mods,
            injected: false,
        }
    }

    fn down(key: Key) -> KeyEvent {
        event(key, KeyKind::Down, Modifiers::NONE)
    }

    fn up(key: Key) -> KeyEvent {
        event(key, KeyKind::Up, Modifiers::NONE)
    }

    fn injected(event: KeyEvent) -> KeyEvent {
        KeyEvent {
            injected: true,
            ..event
        }
    }

    /// An engine with the v0.1 rules that is never bypassed.
    struct Active(Engine);

    impl Active {
        fn new() -> Self {
            Self(Engine::new(Rules::caps_lock()))
        }

        fn key(&mut self, event: KeyEvent) -> Decision {
            self.0.on_key(event, || false)
        }

        /// Presses Caps Lock and returns the token of its hold timer.
        fn press_caps(&mut self) -> HoldToken {
            match self.key(down(CAPS)) {
                Decision::ArmHold(token) => token,
                other => panic!("expected ArmHold, got {other:?}"),
            }
        }
    }

    #[test]
    fn keys_without_rules_pass_through() {
        let mut e = Active::new();
        assert_eq!(e.key(down(LETTER_A)), Decision::Pass);
        assert_eq!(e.key(up(LETTER_A)), Decision::Pass);
    }

    #[test]
    fn a_tap_toggles_the_layout_on_release() {
        let mut e = Active::new();
        assert!(matches!(e.key(down(CAPS)), Decision::ArmHold(_)));
        assert_eq!(e.key(up(CAPS)), Decision::Run(Action::ToggleLayout));
    }

    #[test]
    fn auto_repeat_is_swallowed_and_the_tap_fires_once() {
        let mut e = Active::new();
        e.press_caps();
        assert_eq!(e.key(down(CAPS)), Decision::Swallow);
        assert_eq!(e.key(down(CAPS)), Decision::Swallow);
        assert_eq!(e.key(up(CAPS)), Decision::Run(Action::ToggleLayout));
        assert_eq!(e.key(up(CAPS)), Decision::Pass);
    }

    #[test]
    fn a_hold_toggles_caps_lock_while_the_key_is_still_down() {
        let mut e = Active::new();
        let token = e.press_caps();
        assert_eq!(e.0.on_hold_timer(token), Some(Action::ToggleCapsLock));
    }

    #[test]
    fn after_a_hold_fired_the_rest_of_the_press_does_nothing() {
        let mut e = Active::new();
        let token = e.press_caps();
        e.0.on_hold_timer(token);
        assert_eq!(e.0.on_hold_timer(token), None);
        assert_eq!(e.key(down(CAPS)), Decision::Swallow);
        assert_eq!(e.key(up(CAPS)), Decision::Swallow);
    }

    #[test]
    fn a_timer_that_fires_after_release_does_nothing() {
        let mut e = Active::new();
        let token = e.press_caps();
        e.key(up(CAPS));
        assert_eq!(e.0.on_hold_timer(token), None);
    }

    #[test]
    fn a_timer_from_an_earlier_press_does_not_fire_the_current_one() {
        let mut e = Active::new();
        let stale = e.press_caps();
        e.key(up(CAPS));
        let current = e.press_caps();
        assert_ne!(stale, current);
        assert_eq!(e.0.on_hold_timer(stale), None);
        assert_eq!(e.key(up(CAPS)), Decision::Run(Action::ToggleLayout));
    }

    #[test]
    fn shift_caps_toggles_caps_lock_at_once() {
        let mut e = Active::new();
        let shift_caps = event(CAPS, KeyKind::Down, Modifiers::SHIFT);
        assert_eq!(e.key(shift_caps), Decision::Run(Action::ToggleCapsLock));
        assert_eq!(e.key(shift_caps), Decision::Swallow);
        assert_eq!(e.key(up(CAPS)), Decision::Swallow);
    }

    #[test]
    fn tap_or_chord_is_decided_when_the_key_goes_down() {
        let mut e = Active::new();
        e.press_caps();
        let release_with_shift = event(CAPS, KeyKind::Up, Modifiers::SHIFT);
        assert_eq!(
            e.key(release_with_shift),
            Decision::Run(Action::ToggleLayout)
        );
    }

    #[test]
    fn a_modifier_combination_without_a_rule_passes_for_the_whole_press() {
        let mut e = Active::new();
        assert_eq!(e.key(event(CAPS, KeyKind::Down, CTRL)), Decision::Pass);
        assert_eq!(
            e.key(down(CAPS)),
            Decision::Pass,
            "repeat after Ctrl was released"
        );
        assert_eq!(e.key(up(CAPS)), Decision::Pass);
    }

    #[test]
    fn injected_events_pass_and_do_not_disturb_a_press() {
        let mut e = Active::new();
        e.press_caps();
        assert_eq!(e.key(injected(down(CAPS))), Decision::Pass);
        assert_eq!(e.key(injected(up(CAPS))), Decision::Pass);
        assert_eq!(e.key(up(CAPS)), Decision::Run(Action::ToggleLayout));
    }

    #[test]
    fn a_release_without_a_press_passes() {
        let mut e = Active::new();
        assert_eq!(e.key(up(CAPS)), Decision::Pass);
    }

    #[test]
    fn while_bypassed_everything_passes() {
        let mut e = Engine::new(Rules::caps_lock());
        assert_eq!(e.on_key(down(CAPS), || true), Decision::Pass);
        assert_eq!(e.on_key(up(CAPS), || true), Decision::Pass);
    }

    #[test]
    fn a_bypass_that_starts_mid_press_cancels_the_press() {
        let mut e = Active::new();
        let token = e.press_caps();
        assert_eq!(e.0.on_key(up(CAPS), || true), Decision::Pass);
        assert_eq!(e.0.on_hold_timer(token), None);
        assert_eq!(e.key(up(CAPS)), Decision::Pass);
    }

    #[test]
    fn the_bypass_question_is_asked_only_for_keys_with_rules() {
        let mut e = Engine::new(Rules::caps_lock());
        let never = || panic!("asked whether an unrelated or injected key is bypassed");
        assert_eq!(e.on_key(down(LETTER_A), never), Decision::Pass);
        assert_eq!(e.on_key(injected(down(CAPS)), never), Decision::Pass);
    }

    fn win(key: Key, kind: KeyKind) -> KeyEvent {
        event(key, kind, Modifiers::WIN)
    }

    fn clipboard() -> Engine {
        Engine::new(Rules::keyboard(true))
    }

    #[test]
    fn a_roll_from_one_chord_to_the_next_swallows_both_releases() {
        let mut e = clipboard();
        let mut key = |event| e.on_key(event, || false);
        assert_eq!(
            key(win(Key::X, KeyKind::Down)),
            Decision::Run(Action::Clipboard(Clip::Cut))
        );
        assert_eq!(
            key(win(Key::V, KeyKind::Down)),
            Decision::Run(Action::Clipboard(Clip::Paste))
        );
        assert_eq!(key(win(Key::X, KeyKind::Up)), Decision::Swallow);
        assert_eq!(key(win(Key::V, KeyKind::Up)), Decision::Swallow);
    }

    #[test]
    fn a_caps_lock_press_outlives_a_chord_made_during_it() {
        let mut e = Active(clipboard());
        let token = e.press_caps();
        assert_eq!(
            e.key(win(Key::C, KeyKind::Down)),
            Decision::Run(Action::Clipboard(Clip::Copy))
        );
        assert_eq!(e.key(win(Key::C, KeyKind::Up)), Decision::Swallow);
        assert_eq!(e.0.on_hold_timer(token), Some(Action::ToggleCapsLock));
        assert_eq!(e.key(up(CAPS)), Decision::Swallow);
    }

    #[test]
    fn holding_a_chord_runs_its_action_once() {
        let mut e = Active(clipboard());
        assert!(matches!(
            e.key(win(Key::V, KeyKind::Down)),
            Decision::Run(_)
        ));
        assert_eq!(e.key(win(Key::V, KeyKind::Down)), Decision::Swallow);
        assert_eq!(e.key(win(Key::V, KeyKind::Down)), Decision::Swallow);
        assert_eq!(e.key(win(Key::V, KeyKind::Up)), Decision::Swallow);
    }

    #[test]
    fn the_bypass_question_is_asked_only_for_events_that_would_be_swallowed() {
        let mut e = clipboard();
        let never = || panic!("asked whether a plain letter is bypassed");
        assert_eq!(e.on_key(down(Key::C), never), Decision::Pass);
        assert_eq!(e.on_key(up(Key::C), never), Decision::Pass);
        let shift_c = event(Key::C, KeyKind::Down, Modifiers::SHIFT);
        assert_eq!(e.on_key(shift_c, never), Decision::Pass);
    }

    #[test]
    fn while_bypassed_a_chord_passes_whole() {
        let mut e = clipboard();
        assert_eq!(
            e.on_key(win(Key::C, KeyKind::Down), || true),
            Decision::Pass
        );
        assert_eq!(
            e.on_key(win(Key::C, KeyKind::Down), || true),
            Decision::Pass
        );
        assert_eq!(e.on_key(win(Key::C, KeyKind::Up), || true), Decision::Pass);
        assert_eq!(
            e.on_key(win(Key::C, KeyKind::Down), || false),
            Decision::Run(Action::Clipboard(Clip::Copy)),
            "a new press once the bypass is over"
        );
    }

    #[test]
    fn a_key_with_only_a_tap_rule_needs_no_hold_timer() {
        let tap_only = Rules::new(vec![Rule {
            trigger: Trigger::Tap(CAPS),
            action: Action::ToggleLayout,
        }]);
        let mut e = Engine::new(tap_only);
        assert_eq!(e.on_key(down(CAPS), || false), Decision::Swallow);
        assert_eq!(
            e.on_key(up(CAPS), || false),
            Decision::Run(Action::ToggleLayout)
        );
    }

    #[test]
    fn a_key_with_only_a_hold_rule_does_nothing_on_a_tap() {
        let hold_only = Rules::new(vec![Rule {
            trigger: Trigger::Hold(CAPS),
            action: Action::ToggleCapsLock,
        }]);
        let mut e = Engine::new(hold_only);
        assert!(matches!(
            e.on_key(down(CAPS), || false),
            Decision::ArmHold(_)
        ));
        assert_eq!(e.on_key(up(CAPS), || false), Decision::Swallow);
    }

    #[test]
    fn a_key_with_only_chord_rules_passes_when_pressed_alone() {
        let chord_only = Rules::new(vec![Rule {
            trigger: Trigger::Chord(Modifiers::SHIFT, CAPS),
            action: Action::ToggleCapsLock,
        }]);
        let mut e = Engine::new(chord_only);
        assert_eq!(e.on_key(down(CAPS), || false), Decision::Pass);
        assert_eq!(e.on_key(up(CAPS), || false), Decision::Pass);
    }

    #[test]
    fn only_pass_lets_the_event_through() {
        assert!(!Decision::Pass.swallows());
        assert!(Decision::Swallow.swallows());
        assert!(Decision::Run(Action::ToggleLayout).swallows());
        let mut e = Active::new();
        assert!(Decision::ArmHold(e.press_caps()).swallows());
    }
}
