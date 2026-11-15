//! How Windows names languages.

use capsflip_core::layout::LangId;
use windows::Win32::Globalization::LocaleNameToLCID;
use windows::core::PCWSTR;

/// What `LocaleNameToLCID` returns for a well-formed name it has no data for.
const LOCALE_CUSTOM_UNSPECIFIED: u32 = 0x1000;

/// Asks Windows for the language of a locale name such as `en-US`.
pub fn lang_of(tag: &str) -> Option<LangId> {
    let name: Vec<u16> = tag.encode_utf16().chain(std::iter::once(0)).collect();
    let lcid = unsafe { LocaleNameToLCID(PCWSTR(name.as_ptr()), 0) };
    u16::try_from(lcid & 0xffff)
        .ok()
        .filter(|_| is_known(lcid))
        .map(LangId::new)
}

fn is_known(lcid: u32) -> bool {
    lcid != 0 && lcid != LOCALE_CUSTOM_UNSPECIFIED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_resolves_the_locales_capsflip_was_written_for() {
        assert_eq!(lang_of("en-US"), Some(LangId::new(0x0409)));
        assert_eq!(lang_of("en-GB"), Some(LangId::new(0x0809)));
        assert_eq!(lang_of("uk-UA"), Some(LangId::new(0x0422)));
        assert_eq!(lang_of("ru-RU"), Some(LangId::new(0x0419)));
    }

    #[test]
    fn a_name_windows_does_not_know_has_no_language() {
        assert_eq!(lang_of("xx-XX"), None);
        assert_eq!(lang_of("not a locale"), None);
    }
}
