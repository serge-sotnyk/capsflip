//! Keyboard layouts and the choice of which one a tap activates.

/// A Windows language identifier: the low word of a layout handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LangId(u16);

impl LangId {
    const PRIMARY_MASK: u16 = 0x03ff;
    const PRIMARY_ENGLISH: u16 = 0x09;

    #[must_use]
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    /// True for every English sublanguage, so en-GB counts as English too.
    #[must_use]
    pub const fn is_english(self) -> bool {
        self.0 & Self::PRIMARY_MASK == Self::PRIMARY_ENGLISH
    }
}

/// One installed keyboard layout, identified by its `HKL` bits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LayoutId(isize);

impl LayoutId {
    #[must_use]
    pub const fn from_raw(hkl: isize) -> Self {
        Self(hkl)
    }

    #[must_use]
    pub const fn raw(self) -> isize {
        self.0
    }

    #[must_use]
    pub const fn lang(self) -> LangId {
        let bytes = self.0.to_le_bytes();
        LangId(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
}

/// Which languages the user wants a tap to switch between.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayoutPrefs {
    /// The only English layout a tap ever activates.
    pub english: LangId,
    /// The non-English language to start with, before any was observed.
    pub default_secondary: Option<LangId>,
}

/// Remembers the last non-English layout and picks the target of a tap.
#[derive(Debug)]
pub struct LayoutTracker {
    prefs: LayoutPrefs,
    secondary: Option<LayoutId>,
}

impl LayoutTracker {
    #[must_use]
    pub const fn new(prefs: LayoutPrefs) -> Self {
        Self {
            prefs,
            secondary: None,
        }
    }

    /// Replaces the preferences and keeps the remembered layout.
    pub const fn set_prefs(&mut self, prefs: LayoutPrefs) {
        self.prefs = prefs;
    }

    /// Records that `layout` is active now, whatever caused it.
    pub fn observe(&mut self, layout: LayoutId) {
        if !layout.lang().is_english() {
            self.secondary = Some(layout);
        }
    }

    /// Picks the layout a tap should activate when `current` is active.
    ///
    /// Returns `None` when `installed` offers nothing to switch to.
    pub fn toggle(&mut self, current: LayoutId, installed: &[LayoutId]) -> Option<LayoutId> {
        self.observe(current);
        if current.lang().is_english() {
            self.secondary_among(installed)
        } else {
            self.english_among(installed)
        }
    }

    fn secondary_among(&self, installed: &[LayoutId]) -> Option<LayoutId> {
        let remembered = self.secondary.filter(|layout| installed.contains(layout));
        let default = || find(installed, |lang| Some(lang) == self.prefs.default_secondary);
        let any = || find(installed, |lang| !lang.is_english());
        remembered.or_else(default).or_else(any)
    }

    fn english_among(&self, installed: &[LayoutId]) -> Option<LayoutId> {
        find(installed, |lang| lang == self.prefs.english)
            .or_else(|| find(installed, LangId::is_english))
    }
}

fn find(installed: &[LayoutId], wanted: impl Fn(LangId) -> bool) -> Option<LayoutId> {
    installed
        .iter()
        .copied()
        .find(|layout| wanted(layout.lang()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a layout the way Windows hands it out: a 32-bit `HKL` value
    /// sign-extended to pointer size.
    fn hkl(bits: u32) -> LayoutId {
        LayoutId::from_raw(bits.cast_signed() as isize)
    }

    const EN_US: u32 = 0x0409_0409;
    const EN_GB: u32 = 0x0809_0809;
    const EN_GB_VARIANT: u32 = 0xf001_0809;
    const RU: u32 = 0x0419_0419;
    const UK_ENHANCED: u32 = 0xf0a8_0422;

    #[test]
    fn language_is_the_low_word_even_when_the_handle_is_sign_extended() {
        assert_eq!(hkl(UK_ENHANCED).lang(), LangId::new(0x0422));
        assert_eq!(hkl(EN_US).lang(), LangId::new(0x0409));
    }

    #[test]
    fn every_english_sublanguage_counts_as_english() {
        assert!(hkl(EN_US).lang().is_english());
        assert!(hkl(EN_GB).lang().is_english());
        assert!(hkl(EN_GB_VARIANT).lang().is_english());
    }

    #[test]
    fn other_languages_are_not_english() {
        assert!(!hkl(RU).lang().is_english());
        assert!(!hkl(UK_ENHANCED).lang().is_english());
    }

    #[test]
    fn raw_handle_round_trips() {
        assert_eq!(hkl(EN_US).raw(), 0x0409_0409);
        assert_eq!(LayoutId::from_raw(hkl(UK_ENHANCED).raw()), hkl(UK_ENHANCED));
    }

    const LANG_EN_US: LangId = LangId::new(0x0409);
    const LANG_UK: LangId = LangId::new(0x0422);

    /// The layout list of the machine capsflip was first written for,
    /// phantoms included.
    fn installed() -> Vec<LayoutId> {
        [EN_GB, EN_US, RU, UK_ENHANCED, EN_GB_VARIANT]
            .map(hkl)
            .to_vec()
    }

    fn tracker(default_secondary: Option<LangId>) -> LayoutTracker {
        LayoutTracker::new(LayoutPrefs {
            english: LANG_EN_US,
            default_secondary,
        })
    }

    #[test]
    fn from_english_a_fresh_tracker_goes_to_the_default_secondary() {
        let mut t = tracker(Some(LANG_UK));
        assert_eq!(t.toggle(hkl(EN_US), &installed()), Some(hkl(UK_ENHANCED)));
    }

    #[test]
    fn without_a_default_the_first_non_english_layout_is_used() {
        let mut t = tracker(None);
        assert_eq!(t.toggle(hkl(EN_US), &installed()), Some(hkl(RU)));
    }

    #[test]
    fn a_default_that_is_not_installed_falls_back_to_the_first_non_english() {
        let mut t = tracker(Some(LangId::new(0x0407)));
        assert_eq!(t.toggle(hkl(EN_US), &installed()), Some(hkl(RU)));
    }

    #[test]
    fn from_non_english_it_goes_to_the_configured_english_not_a_phantom() {
        let mut t = tracker(Some(LANG_UK));
        assert_eq!(t.toggle(hkl(UK_ENHANCED), &installed()), Some(hkl(EN_US)));
    }

    #[test]
    fn a_phantom_english_layout_is_left_like_any_english_layout() {
        let mut t = tracker(Some(LANG_UK));
        assert_eq!(t.toggle(hkl(EN_GB), &installed()), Some(hkl(UK_ENHANCED)));
    }

    #[test]
    fn an_observed_manual_switch_becomes_the_secondary_layout() {
        let mut t = tracker(Some(LANG_UK));
        t.observe(hkl(RU));
        assert_eq!(t.toggle(hkl(EN_US), &installed()), Some(hkl(RU)));
    }

    #[test]
    fn observing_english_does_not_forget_the_secondary_layout() {
        let mut t = tracker(Some(LANG_UK));
        t.observe(hkl(RU));
        t.observe(hkl(EN_GB));
        assert_eq!(t.toggle(hkl(EN_US), &installed()), Some(hkl(RU)));
    }

    #[test]
    fn toggling_away_from_a_layout_remembers_it() {
        let mut t = tracker(Some(LANG_UK));
        assert_eq!(t.toggle(hkl(RU), &installed()), Some(hkl(EN_US)));
        assert_eq!(t.toggle(hkl(EN_US), &installed()), Some(hkl(RU)));
    }

    #[test]
    fn a_remembered_layout_that_was_uninstalled_is_not_used() {
        let mut t = tracker(Some(LANG_UK));
        t.observe(hkl(RU));
        let without_ru = [EN_US, UK_ENHANCED].map(hkl);
        assert_eq!(t.toggle(hkl(EN_US), &without_ru), Some(hkl(UK_ENHANCED)));
    }

    #[test]
    fn without_the_configured_english_any_english_layout_is_used() {
        let mut t = tracker(Some(LANG_UK));
        let without_en_us = [RU, EN_GB, UK_ENHANCED].map(hkl);
        assert_eq!(t.toggle(hkl(RU), &without_en_us), Some(hkl(EN_GB)));
    }

    #[test]
    fn with_nothing_to_switch_to_there_is_no_target() {
        let mut t = tracker(Some(LANG_UK));
        assert_eq!(t.toggle(hkl(EN_US), &[EN_US, EN_GB].map(hkl)), None);
        assert_eq!(t.toggle(hkl(RU), &[RU, UK_ENHANCED].map(hkl)), None);
    }
}
