//! The value in the user's registry that starts capsflip at login.

use capsflip_core::autostart::Change;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::HSTRING;

/// A string value under `HKEY_CURRENT_USER`.
pub struct RunKey {
    key: HSTRING,
    name: HSTRING,
}

impl RunKey {
    /// The value Windows reads at login.
    pub fn login() -> Self {
        Self {
            key: r"Software\Microsoft\Windows\CurrentVersion\Run".into(),
            name: "capsflip".into(),
        }
    }

    /// The text of the value, or `None` when there is none.
    pub fn read(&self) -> Option<String> {
        let mut text = [0u16; 1024];
        let mut bytes = u32::try_from(size_of_val(&text)).unwrap_or_default();
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &self.key,
                &self.name,
                RRF_RT_REG_SZ,
                None,
                Some(text.as_mut_ptr().cast()),
                Some(&raw mut bytes),
            )
        };
        status.ok().ok()?;
        let units = usize::try_from(bytes / 2).ok()?.saturating_sub(1);
        Some(String::from_utf16_lossy(text.get(..units)?))
    }

    /// Writes or deletes the value.
    pub fn apply(&self, change: &Change) -> windows::core::Result<()> {
        match change {
            Change::Register(command) => self.write(command),
            Change::Unregister => {
                unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &self.key, &self.name) }.ok()
            }
        }
    }

    fn write(&self, text: &str) -> windows::core::Result<()> {
        let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let bytes = u32::try_from(size_of_val(text.as_slice())).unwrap_or_default();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &self.key,
                &self.name,
                REG_SZ.0,
                Some(text.as_ptr().cast()),
                bytes,
            )
        }
        .ok()
    }
}

#[cfg(test)]
pub mod scratch {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};

    use super::RunKey;

    /// A value under a key of its own, so that tests never touch the real
    /// `Run` key. Call [`remove`] at the end of the test.
    pub fn run_key(test: &str) -> RunKey {
        RunKey {
            key: format!(r"Software\capsflip-tests-{}-{test}", std::process::id()).into(),
            name: "capsflip".into(),
        }
    }

    pub fn remove(run_key: &RunKey) {
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &run_key.key) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_written_read_back_and_deleted() {
        let key = scratch::run_key("autostart");
        assert_eq!(key.read(), None);

        let command = r#""C:\Програми\capsflip.exe""#;
        key.apply(&Change::Register(command.to_string())).unwrap();
        assert_eq!(key.read().as_deref(), Some(command));

        key.apply(&Change::Unregister).unwrap();
        assert_eq!(key.read(), None);
        scratch::remove(&key);
    }

    #[test]
    fn deleting_a_value_that_is_not_there_is_an_error() {
        let key = scratch::run_key("missing");
        assert!(key.apply(&Change::Unregister).is_err());
        scratch::remove(&key);
    }
}
