//! The key events to inject so that the system sees a combination, given
//! the keys the user is holding right now.

use crate::key::{Key, KeyKind, Modifiers};
use crate::rules::Combo;

/// One injected key event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stroke {
    pub key: Key,
    pub kind: KeyKind,
}

/// Every modifier key, both sides, in the order they are released.
pub const MODIFIER_KEYS: [Key; 8] = [
    Key::LWIN,
    Key::RWIN,
    Key::LSHIFT,
    Key::RSHIFT,
    Key::LCONTROL,
    Key::RCONTROL,
    Key::LMENU,
    Key::RMENU,
];

/// The events that make the system see exactly `combo` while the keys in
/// `held` are down.
///
/// A held modifier the combination does not use is released before the key
/// and pressed again after it; a modifier it needs and is not held is
/// pressed (the left one) and released. When Win is released, the mask goes
/// before the release and after the new press, so that neither opens Start
/// (measured in the spike, `docs/winapi-notes.md`).
#[must_use]
pub fn sequence(combo: Combo, held: &[Key]) -> Vec<Stroke> {
    let (release, press) = changes(combo.mods, held);
    let masked = release
        .iter()
        .any(|key| [Key::LWIN, Key::RWIN].contains(key));
    let mask = if masked { tap(Key::MASK) } else { Vec::new() };
    let mut strokes = mask.clone();
    strokes.extend(each(release.iter(), KeyKind::Up));
    strokes.extend(each(press.iter(), KeyKind::Down));
    strokes.extend(tap(combo.key));
    strokes.extend(each(press.iter().rev(), KeyKind::Up));
    strokes.extend(each(release.iter().rev(), KeyKind::Down));
    strokes.extend(mask);
    strokes
}

/// The held keys to release and the keys to press for `mods`.
fn changes(mods: Modifiers, held: &[Key]) -> (Vec<Key>, Vec<Key>) {
    let mut release = Vec::new();
    let mut press = Vec::new();
    let pairs = [
        (mods.win, [Key::LWIN, Key::RWIN]),
        (mods.shift, [Key::LSHIFT, Key::RSHIFT]),
        (mods.ctrl, [Key::LCONTROL, Key::RCONTROL]),
        (mods.alt, [Key::LMENU, Key::RMENU]),
    ];
    for (wanted, sides) in pairs {
        let down: Vec<Key> = sides.into_iter().filter(|key| held.contains(key)).collect();
        match (wanted, down.is_empty()) {
            (true, true) => press.push(sides[0]),
            (false, false) => release.extend(down),
            _ => {}
        }
    }
    (release, press)
}

/// The same event for each of `keys`.
fn each<'a>(keys: impl Iterator<Item = &'a Key>, kind: KeyKind) -> impl Iterator<Item = Stroke> {
    keys.map(move |key| Stroke { key: *key, kind })
}

fn tap(key: Key) -> Vec<Stroke> {
    let key = [key];
    each(key.iter(), KeyKind::Down)
        .chain(each(key.iter(), KeyKind::Up))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strokes written as the spike logged them, e.g. `LWin↑ LCtrl↓`.
    fn strokes(text: &str) -> Vec<Stroke> {
        text.split_whitespace()
            .map(|word| {
                let (name, kind) = match word.strip_suffix('↓') {
                    Some(name) => (name, KeyKind::Down),
                    None => (word.strip_suffix('↑').expect("↑ or ↓"), KeyKind::Up),
                };
                Stroke {
                    key: key(name),
                    kind,
                }
            })
            .collect()
    }

    fn key(name: &str) -> Key {
        match name {
            "mask" => Key::MASK,
            "LWin" => Key::LWIN,
            "RWin" => Key::RWIN,
            "LShift" => Key::LSHIFT,
            "RShift" => Key::RSHIFT,
            "LCtrl" => Key::LCONTROL,
            "RCtrl" => Key::RCONTROL,
            "LAlt" => Key::LMENU,
            "C" => Key::C,
            "V" => Key::V,
            "Insert" => Key::INSERT,
            "Caps" => Key::CAPS_LOCK,
            _ => panic!("unknown key {name}"),
        }
    }

    fn combo(mods: Modifiers, key: Key) -> Combo {
        Combo { mods, key }
    }

    const CTRL_C: Combo = Combo {
        mods: Modifiers::CTRL,
        key: Key::C,
    };

    #[test]
    fn win_is_released_around_the_combination_and_masked_both_times() {
        assert_eq!(
            sequence(CTRL_C, &[Key::LWIN]),
            strokes("mask↓ mask↑ LWin↑ LCtrl↓ C↓ C↑ LCtrl↑ LWin↓ mask↓ mask↑")
        );
    }

    #[test]
    fn the_win_key_that_is_held_is_the_one_released() {
        let paste = combo(Modifiers::SHIFT, Key::INSERT);
        assert_eq!(
            sequence(paste, &[Key::RWIN]),
            strokes("mask↓ mask↑ RWin↑ LShift↓ Insert↓ Insert↑ LShift↑ RWin↓ mask↓ mask↑")
        );
    }

    #[test]
    fn win_shift_v_becomes_win_v_by_lifting_shift_only() {
        let history = combo(Modifiers::WIN, Key::V);
        assert_eq!(
            sequence(history, &[Key::LWIN, Key::RSHIFT]),
            strokes("RShift↑ V↓ V↑ RShift↓")
        );
    }

    #[test]
    fn every_unwanted_modifier_is_released_and_pressed_again_in_reverse() {
        assert_eq!(
            sequence(CTRL_C, &[Key::LWIN, Key::LSHIFT, Key::LMENU]),
            strokes(
                "mask↓ mask↑ LWin↑ LShift↑ LAlt↑ LCtrl↓ C↓ C↑ LCtrl↑ LAlt↓ LShift↓ LWin↓ mask↓ mask↑"
            )
        );
    }

    #[test]
    fn a_wanted_modifier_that_is_already_held_is_left_alone() {
        assert_eq!(
            sequence(CTRL_C, &[Key::LWIN, Key::RCONTROL]),
            strokes("mask↓ mask↑ LWin↑ C↓ C↑ LWin↓ mask↓ mask↑")
        );
    }

    #[test]
    fn without_win_held_there_is_no_mask() {
        assert_eq!(sequence(CTRL_C, &[]), strokes("LCtrl↓ C↓ C↑ LCtrl↑"));
        let caps = combo(Modifiers::NONE, Key::CAPS_LOCK);
        assert_eq!(sequence(caps, &[]), strokes("Caps↓ Caps↑"));
        assert_eq!(
            sequence(caps, &[Key::LSHIFT]),
            strokes("LShift↑ Caps↓ Caps↑ LShift↓")
        );
    }

    #[test]
    fn the_modifier_keys_cover_both_sides_of_all_four() {
        assert_eq!(MODIFIER_KEYS.len(), 8);
        for key in [Key::LWIN, Key::RWIN, Key::RSHIFT, Key::RCONTROL, Key::LMENU] {
            assert!(MODIFIER_KEYS.contains(&key), "{key:?}");
        }
    }
}
