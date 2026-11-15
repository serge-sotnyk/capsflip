//! One capsflip per user session, enforced with a named mutex.

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::HSTRING;

/// The mutex that marks a running capsflip in this session.
pub const NAME: &str = r"Local\capsflip-single-instance";

/// Held for as long as this process is the one running capsflip.
pub struct Instance(HANDLE);

impl Instance {
    /// Claims the mutex `name`. Returns `None` when another process, or an
    /// earlier claim in this one, already holds it.
    pub fn claim(name: &str) -> Option<Self> {
        let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(name)) }.ok()?;
        let taken = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let instance = Self(handle);
        (!taken).then_some(instance)
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_claim_holds_until_it_is_released() {
        let name = format!(r"Local\capsflip-test-{}", std::process::id());
        let first = Instance::claim(&name);
        assert!(first.is_some());
        assert!(Instance::claim(&name).is_none(), "a second instance");
        drop(first);
        assert!(Instance::claim(&name).is_some(), "free again");
    }
}
