//! The controller: connects the engine and the layout tracker to the
//! operating system, which it reaches only through [`Platform`].

use crate::apps::Apps;
use crate::config::{HoldThreshold, Settings};
use crate::engine::{Decision, Engine, HoldToken};
use crate::key::{Key, KeyEvent, Modifiers};
use crate::layout::{LayoutId, LayoutTracker};
use crate::rules::{Action, Combo, Rules};
use crate::strokes::{self, MODIFIER_KEYS, Stroke};

/// Everything the controller needs from the operating system.
///
/// The shell implements it with straight Win32 calls; tests implement it
/// with a fake desktop.
pub trait Platform {
    /// The layout of the window that has the keyboard focus, or `None`
    /// when no window has it.
    fn foreground_layout(&self) -> Option<LayoutId>;
    /// The executable file name of the foreground window, e.g. `mstsc.exe`.
    fn foreground_executable(&self) -> String;
    fn installed_layouts(&self) -> Vec<LayoutId>;
    /// Asks the foreground window to switch to `layout`.
    fn activate_layout(&mut self, layout: LayoutId);
    /// Whether `key` is down right now.
    fn is_down(&self, key: Key) -> bool;
    /// Injects `strokes` in order, all at once.
    fn send_keys(&mut self, strokes: &[Stroke]);
    /// Arranges a call to [`Controller::on_hold_timer`] with `token` once
    /// `after` has elapsed.
    fn arm_hold_timer(&mut self, token: HoldToken, after: HoldThreshold);
}

/// Owns all state and decides what the shell does for each event.
#[derive(Debug)]
pub struct Controller {
    engine: Engine,
    tracker: LayoutTracker,
    hold_threshold: HoldThreshold,
    exclusions: Apps,
    /// Where copy and paste go as Ctrl+Insert and Shift+Insert.
    terminals: Apps,
    paused: bool,
    pending: Vec<Decision>,
}

impl Controller {
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        Self {
            engine: Engine::new(Rules::keyboard(settings.clipboard.win_shortcuts)),
            tracker: LayoutTracker::new(settings.layouts),
            hold_threshold: settings.hold_threshold,
            exclusions: settings.exclusions,
            terminals: settings.clipboard.terminals,
            paused: false,
            pending: Vec::new(),
        }
    }

    /// Applies settings read again from the config file. The layout learned
    /// so far is kept: it reflects what the user actually types in.
    pub fn reload(&mut self, settings: Settings) {
        self.engine = Engine::new(Rules::keyboard(settings.clipboard.win_shortcuts));
        self.tracker.set_prefs(settings.layouts);
        self.hold_threshold = settings.hold_threshold;
        self.exclusions = settings.exclusions;
        self.terminals = settings.clipboard.terminals;
    }

    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pauses or resumes every rule and returns whether it is paused now.
    /// Layouts chosen by hand are still remembered while paused.
    pub fn toggle_pause(&mut self) -> bool {
        self.paused = !self.paused;
        self.paused
    }

    /// Handles a key event inside the keyboard hook and returns whether to
    /// swallow it.
    ///
    /// It only decides. The work is queued for [`Self::run_pending`], which
    /// the shell must call once the hook has returned, because Windows
    /// removes a hook that is slow.
    pub fn on_key(&mut self, event: KeyEvent, platform: &impl Platform) -> bool {
        let (paused, exclusions) = (self.paused, &self.exclusions);
        let bypassed = || paused || exclusions.contains(&platform.foreground_executable());
        let decision = self.engine.on_key(event, bypassed);
        if decision.swallows() {
            self.pending.push(decision);
        }
        decision.swallows()
    }

    /// Carries out what the key events since the last call decided.
    pub fn run_pending(&mut self, platform: &mut impl Platform) {
        for decision in std::mem::take(&mut self.pending) {
            match decision {
                Decision::Run(action) => self.perform(action, platform),
                Decision::ArmHold(token) => platform.arm_hold_timer(token, self.hold_threshold),
                Decision::Pass | Decision::Swallow => {}
            }
        }
    }

    /// Reports that the hold timer armed with `token` has fired.
    pub fn on_hold_timer(&mut self, token: HoldToken, platform: &mut impl Platform) {
        if let Some(action) = self.engine.on_hold_timer(token) {
            self.perform(action, platform);
        }
    }

    /// Looks at the foreground layout, so that switches made by hand are
    /// remembered. Windows does not announce them; the shell calls this on a
    /// timer.
    pub fn on_poll(&mut self, platform: &impl Platform) {
        if let Some(layout) = platform.foreground_layout() {
            self.tracker.observe(layout);
        }
    }

    fn perform(&mut self, action: Action, platform: &mut impl Platform) {
        let combo = match action {
            Action::ToggleLayout => return self.toggle_layout(platform),
            Action::ToggleCapsLock => Some(Combo {
                mods: Modifiers::NONE,
                key: Key::CAPS_LOCK,
            }),
            Action::Clipboard(clip) => {
                clip.combo(self.terminals.contains(&platform.foreground_executable()))
            }
            Action::Send(combo) => Some(combo),
        };
        if let Some(combo) = combo {
            send(combo, platform);
        }
    }

    fn toggle_layout(&mut self, platform: &mut impl Platform) {
        let installed = platform.installed_layouts();
        let target = platform
            .foreground_layout()
            .and_then(|current| self.tracker.toggle(current, &installed));
        if let Some(target) = target {
            platform.activate_layout(target);
        }
    }
}

/// Injects `combo`, working around the modifiers the user holds now.
fn send(combo: Combo, platform: &mut impl Platform) {
    let held: Vec<Key> = MODIFIER_KEYS
        .into_iter()
        .filter(|key| platform.is_down(*key))
        .collect();
    platform.send_keys(&strokes::sequence(combo, &held));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, HoldThreshold};
    use crate::key::{Key, KeyEvent, KeyKind, Modifiers};
    use crate::layout::{LangId, LayoutId};
    use crate::rules::Combo;
    use crate::strokes::{self, Stroke};

    const EN_US: LayoutId = LayoutId::from_raw(0x0409_0409);
    const EN_GB: LayoutId = LayoutId::from_raw(0x0809_0809);
    const RU: LayoutId = LayoutId::from_raw(0x0419_0419);
    const UK: LayoutId = LayoutId::from_raw(0x0422_0422);

    /// A desktop with one focused window that switches layout when asked.
    struct Desktop {
        layout: LayoutId,
        focused: bool,
        executable: &'static str,
        installed: Vec<LayoutId>,
        activated: Vec<LayoutId>,
        /// The keys the user holds.
        down: Vec<Key>,
        /// Every injected sequence.
        sent: Vec<Vec<Stroke>>,
        armed: Vec<(HoldToken, HoldThreshold)>,
    }

    impl Desktop {
        fn typing_in(layout: LayoutId) -> Self {
            Self {
                layout,
                focused: true,
                executable: "notepad.exe",
                installed: vec![EN_GB, EN_US, RU, UK],
                activated: Vec::new(),
                down: Vec::new(),
                sent: Vec::new(),
                armed: Vec::new(),
            }
        }

        /// How many injected sequences toggled Caps Lock.
        fn caps_lock_toggles(&self) -> usize {
            self.sent
                .iter()
                .filter(|strokes| strokes.iter().any(|s| s.key == Key::CAPS_LOCK))
                .count()
        }
    }

    impl Platform for Desktop {
        fn foreground_layout(&self) -> Option<LayoutId> {
            self.focused.then_some(self.layout)
        }

        fn foreground_executable(&self) -> String {
            self.executable.to_string()
        }

        fn installed_layouts(&self) -> Vec<LayoutId> {
            self.installed.clone()
        }

        fn activate_layout(&mut self, layout: LayoutId) {
            self.layout = layout;
            self.activated.push(layout);
        }

        fn is_down(&self, key: Key) -> bool {
            self.down.contains(&key)
        }

        fn send_keys(&mut self, strokes: &[Stroke]) {
            self.sent.push(strokes.to_vec());
        }

        fn arm_hold_timer(&mut self, token: HoldToken, after: HoldThreshold) {
            self.armed.push((token, after));
        }
    }

    fn lang_of(tag: &str) -> Option<LangId> {
        match tag {
            "en-US" => Some(EN_US.lang()),
            "en-GB" => Some(EN_GB.lang()),
            "uk-UA" => Some(UK.lang()),
            _ => None,
        }
    }

    fn settings(config: &str) -> Settings {
        Config::parse(config).unwrap().resolve(lang_of).unwrap()
    }

    fn controller(config: &str) -> Controller {
        Controller::new(settings(config))
    }

    fn caps(kind: KeyKind, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            key: Key::CAPS_LOCK,
            kind,
            mods,
            injected: false,
        }
    }

    /// Presses and releases Caps Lock, then lets the message loop run.
    fn tap(controller: &mut Controller, desktop: &mut Desktop) {
        controller.on_key(caps(KeyKind::Down, Modifiers::NONE), desktop);
        controller.on_key(caps(KeyKind::Up, Modifiers::NONE), desktop);
        controller.run_pending(desktop);
    }

    #[test]
    fn a_tap_is_swallowed_and_acts_only_after_the_hook_returned() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        assert!(c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop));
        assert!(c.on_key(caps(KeyKind::Up, Modifiers::NONE), &desktop));
        assert_eq!(desktop.activated, []);
        c.run_pending(&mut desktop);
        assert_eq!(desktop.activated, [RU]);
        c.run_pending(&mut desktop);
        assert_eq!(desktop.activated, [RU], "pending work runs once");
    }

    #[test]
    fn taps_alternate_between_english_and_the_configured_secondary() {
        let mut c = controller("[layout]\ndefault_secondary = \"uk-UA\"\n");
        let mut desktop = Desktop::typing_in(EN_US);
        tap(&mut c, &mut desktop);
        tap(&mut c, &mut desktop);
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.activated, [UK, EN_US, UK]);
    }

    #[test]
    fn a_layout_chosen_by_hand_replaces_the_secondary() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.layout, RU);
        desktop.layout = UK; // Ctrl+Shift
        tap(&mut c, &mut desktop);
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.layout, UK);
    }

    #[test]
    fn polling_notices_a_manual_switch_made_in_another_window() {
        let mut c = controller("[layout]\ndefault_secondary = \"uk-UA\"\n");
        let mut desktop = Desktop::typing_in(RU);
        c.on_poll(&desktop);
        desktop.layout = EN_US; // another window gets the focus
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.activated, [RU]);
    }

    #[test]
    fn a_hold_arms_the_timer_and_toggles_caps_lock_when_it_fires() {
        let mut c = controller("[caps_lock]\nhold_threshold_ms = 700\n");
        let mut desktop = Desktop::typing_in(EN_US);
        c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop);
        c.run_pending(&mut desktop);
        let (token, after) = desktop.armed[0];
        assert_eq!(after.as_millis(), 700);

        c.on_hold_timer(token, &mut desktop);
        assert_eq!(desktop.caps_lock_toggles(), 1);

        assert!(c.on_key(caps(KeyKind::Up, Modifiers::NONE), &desktop));
        c.run_pending(&mut desktop);
        assert_eq!(desktop.activated, []);
        assert_eq!(desktop.caps_lock_toggles(), 1);
    }

    #[test]
    fn a_timer_that_fires_after_a_tap_does_nothing() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop);
        c.run_pending(&mut desktop);
        let (token, _) = desktop.armed[0];
        c.on_key(caps(KeyKind::Up, Modifiers::NONE), &desktop);
        c.run_pending(&mut desktop);
        c.on_hold_timer(token, &mut desktop);
        assert_eq!(desktop.caps_lock_toggles(), 0);
    }

    #[test]
    fn shift_caps_toggles_caps_lock_and_leaves_the_layout_alone() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        assert!(c.on_key(caps(KeyKind::Down, Modifiers::SHIFT), &desktop));
        c.on_key(caps(KeyKind::Up, Modifiers::SHIFT), &desktop);
        c.run_pending(&mut desktop);
        assert_eq!(desktop.caps_lock_toggles(), 1);
        assert_eq!(desktop.activated, []);
    }

    const WIN_SHORTCUTS: &str = "[clipboard]\nwin_shortcuts = true\n";

    /// Presses `key` with Win (and Shift when asked) held, then lets the
    /// message loop run.
    fn win(c: &mut Controller, desktop: &mut Desktop, key: Key, mods: Modifiers) -> bool {
        let press = KeyEvent {
            key,
            kind: KeyKind::Down,
            mods,
            injected: false,
        };
        let swallowed = c.on_key(press, desktop);
        c.on_key(
            KeyEvent {
                kind: KeyKind::Up,
                ..press
            },
            desktop,
        );
        c.run_pending(desktop);
        swallowed
    }

    fn sequence(mods: Modifiers, key: Key, held: &[Key]) -> Vec<Stroke> {
        strokes::sequence(Combo { mods, key }, held)
    }

    #[test]
    fn win_c_v_x_send_ctrl_c_v_x_with_win_let_go_around_them() {
        let mut c = controller(WIN_SHORTCUTS);
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.down = vec![Key::LWIN];
        for key in [Key::C, Key::V, Key::X] {
            assert!(win(&mut c, &mut desktop, key, Modifiers::WIN));
        }
        let held = [Key::LWIN];
        assert_eq!(
            desktop.sent,
            [
                sequence(Modifiers::CTRL, Key::C, &held),
                sequence(Modifiers::CTRL, Key::V, &held),
                sequence(Modifiers::CTRL, Key::X, &held),
            ]
        );
    }

    #[test]
    fn in_a_terminal_copy_and_paste_use_insert_and_cut_sends_nothing() {
        let mut c = controller(WIN_SHORTCUTS);
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.executable = "WindowsTerminal.exe";
        desktop.down = vec![Key::RWIN];
        for key in [Key::C, Key::V, Key::X] {
            assert!(win(&mut c, &mut desktop, key, Modifiers::WIN), "swallowed");
        }
        let held = [Key::RWIN];
        assert_eq!(
            desktop.sent,
            [
                sequence(Modifiers::CTRL, Key::INSERT, &held),
                sequence(Modifiers::SHIFT, Key::INSERT, &held),
            ]
        );
    }

    #[test]
    fn win_shift_v_opens_the_clipboard_history() {
        let mut c = controller(WIN_SHORTCUTS);
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.down = vec![Key::LWIN, Key::LSHIFT];
        assert!(win(&mut c, &mut desktop, Key::V, Modifiers::WIN_SHIFT));
        assert_eq!(
            desktop.sent,
            [sequence(Modifiers::WIN, Key::V, &desktop.down)]
        );
    }

    #[test]
    fn win_shortcuts_are_off_until_switched_on_in_config() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        assert!(!win(&mut c, &mut desktop, Key::C, Modifiers::WIN));
        assert_eq!(desktop.sent, Vec::<Vec<Stroke>>::new());

        c.reload(settings(WIN_SHORTCUTS));
        assert!(win(&mut c, &mut desktop, Key::C, Modifiers::WIN));
    }

    #[test]
    fn reload_can_switch_the_win_shortcuts_off_and_change_the_terminals() {
        let mut c = controller(WIN_SHORTCUTS);
        let mut desktop = Desktop::typing_in(EN_US);
        c.reload(settings(
            "[clipboard]\nwin_shortcuts = true\nterminals = [\"notepad.exe\"]\n",
        ));
        win(&mut c, &mut desktop, Key::C, Modifiers::WIN);
        assert_eq!(desktop.sent, [sequence(Modifiers::CTRL, Key::INSERT, &[])]);

        c.reload(settings(""));
        assert!(!win(&mut c, &mut desktop, Key::C, Modifiers::WIN));
    }

    #[test]
    fn in_an_excluded_application_win_c_is_left_to_windows() {
        let mut c =
            controller("[clipboard]\nwin_shortcuts = true\n[exclusions]\napps = [\"mstsc.exe\"]\n");
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.executable = "mstsc.exe";
        assert!(!win(&mut c, &mut desktop, Key::C, Modifiers::WIN));
        assert_eq!(desktop.sent, Vec::<Vec<Stroke>>::new());
    }

    #[test]
    fn caps_lock_is_toggled_with_shift_let_go_first() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.down = vec![Key::LSHIFT];
        win(&mut c, &mut desktop, Key::CAPS_LOCK, Modifiers::SHIFT);
        assert_eq!(
            desktop.sent,
            [sequence(Modifiers::NONE, Key::CAPS_LOCK, &[Key::LSHIFT])]
        );
    }

    #[test]
    fn in_an_excluded_application_caps_lock_is_left_alone() {
        let mut c = controller("[exclusions]\napps = [\"mstsc.exe\"]\n");
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.executable = "MSTSC.EXE";
        assert!(!c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop));
        assert!(!c.on_key(caps(KeyKind::Up, Modifiers::NONE), &desktop));
        c.run_pending(&mut desktop);
        assert_eq!(desktop.activated, []);
        assert_eq!(desktop.armed, []);
    }

    #[test]
    fn while_paused_caps_lock_is_left_alone() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        assert!(!c.is_paused());
        assert!(c.toggle_pause(), "now paused");
        assert!(c.is_paused());
        assert!(!c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop));
        assert!(!c.on_key(caps(KeyKind::Up, Modifiers::NONE), &desktop));
        c.run_pending(&mut desktop);
        assert_eq!(desktop.activated, []);
        assert_eq!(desktop.armed, []);

        assert!(!c.toggle_pause(), "running again");
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.activated, [RU]);
    }

    #[test]
    fn while_paused_manual_switches_are_still_noticed() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(UK);
        c.toggle_pause();
        c.on_poll(&desktop);
        c.toggle_pause();
        desktop.layout = EN_US;
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.activated, [UK]);
    }

    #[test]
    fn reloading_applies_the_new_settings_and_keeps_the_learned_layout() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(UK);
        c.on_poll(&desktop);
        c.reload(settings(
            "[layout]\nenglish = \"en-GB\"\n[caps_lock]\nhold_threshold_ms = 700\n\
             [exclusions]\napps = [\"mstsc.exe\"]\n",
        ));

        desktop.executable = "mstsc.exe";
        assert!(!c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop));
        assert!(!c.on_key(caps(KeyKind::Up, Modifiers::NONE), &desktop));
        desktop.executable = "notepad.exe";

        tap(&mut c, &mut desktop);
        tap(&mut c, &mut desktop);
        assert_eq!(
            desktop.activated,
            [EN_GB, UK],
            "Ukrainian was learned before"
        );

        c.on_key(caps(KeyKind::Down, Modifiers::NONE), &desktop);
        c.run_pending(&mut desktop);
        assert_eq!(
            desktop.armed.last().map(|(_, after)| after.as_millis()),
            Some(700)
        );
    }

    #[test]
    fn other_keys_are_not_swallowed() {
        let mut c = controller("");
        let desktop = Desktop::typing_in(EN_US);
        let letter = KeyEvent {
            key: Key::from_vk(0x41),
            ..caps(KeyKind::Down, Modifiers::NONE)
        };
        assert!(!c.on_key(letter, &desktop));
    }

    #[test]
    fn without_a_foreground_window_nothing_is_learned_or_switched() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(UK);
        desktop.focused = false;
        c.on_poll(&desktop);
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.activated, []);

        desktop.focused = true;
        desktop.layout = EN_US;
        tap(&mut c, &mut desktop);
        assert_eq!(
            desktop.activated,
            [RU],
            "Ukrainian was never seen in a window"
        );
    }

    #[test]
    fn with_no_layout_to_switch_to_a_tap_does_nothing() {
        let mut c = controller("");
        let mut desktop = Desktop::typing_in(EN_US);
        desktop.installed = vec![EN_US, EN_GB];
        tap(&mut c, &mut desktop);
        assert_eq!(desktop.activated, []);
    }
}
