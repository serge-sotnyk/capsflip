//! [`Platform`] implemented with Win32 calls. Nothing is decided here.

use capsflip_core::config::HoldThreshold;
use capsflip_core::controller::Platform;
use capsflip_core::engine::HoldToken;
use capsflip_core::key::{Key, KeyKind};
use capsflip_core::layout::LayoutId;
use capsflip_core::strokes::Stroke;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    PostMessageW, SetTimer, TIMERPROC, WM_INPUTLANGCHANGEREQUEST,
};
use windows::core::PWSTR;

use crate::logging::log;

/// Identifies the hold timer among the timers of the message window.
pub const HOLD_TIMER: usize = 1;

pub struct Win32 {
    /// The message window that owns the timers.
    pub window: HWND,
    /// Called by Windows when the hold timer fires.
    pub on_hold_timer: TIMERPROC,
    /// The token of the armed hold timer, handed back when it fires.
    pub hold_token: Option<HoldToken>,
}

impl Platform for Win32 {
    fn foreground_layout(&self) -> Option<LayoutId> {
        let thread = focus_thread()?;
        let layout = unsafe { GetKeyboardLayout(thread) };
        Some(LayoutId::from_raw(layout.0 as isize))
    }

    fn foreground_executable(&self) -> String {
        executable_of(input_window(unsafe { GetForegroundWindow() })).unwrap_or_default()
    }

    fn installed_layouts(&self) -> Vec<LayoutId> {
        let count = unsafe { GetKeyboardLayoutList(None) };
        let mut layouts = vec![HKL::default(); usize::try_from(count).unwrap_or_default()];
        let filled = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
        layouts.truncate(usize::try_from(filled).unwrap_or_default());
        layouts
            .into_iter()
            .map(|layout| LayoutId::from_raw(layout.0 as isize))
            .collect()
    }

    fn activate_layout(&mut self, layout: LayoutId) {
        let window = unsafe { GetForegroundWindow() };
        let request = (WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(layout.raw()));
        let posted = unsafe { PostMessageW(Some(window), request.0, request.1, request.2) };
        log(&format!(
            "layout {:#x} requested in {}: {posted:?}",
            layout.raw(),
            describe(window)
        ));
    }

    fn is_down(&self, key: Key) -> bool {
        let state = unsafe { GetAsyncKeyState(i32::from(key.vk())) };
        state < 0
    }

    fn send_keys(&mut self, strokes: &[Stroke]) {
        let inputs: Vec<INPUT> = strokes.iter().map(|stroke| input(*stroke)).collect();
        let size = i32::try_from(size_of::<INPUT>()).unwrap_or_default();
        let sent = unsafe { SendInput(&inputs, size) };
        let listed: Vec<String> = strokes.iter().map(|stroke| written(*stroke)).collect();
        log(&format!(
            "injected {sent} of {}: {}",
            inputs.len(),
            listed.join(" ")
        ));
    }

    fn arm_hold_timer(&mut self, token: HoldToken, after: HoldThreshold) {
        self.hold_token = Some(token);
        unsafe {
            SetTimer(
                Some(self.window),
                HOLD_TIMER,
                after.as_millis(),
                self.on_hold_timer,
            )
        };
    }
}

/// The thread that takes the keyboard input of the foreground window.
///
/// It is the focused child window when there is one: in UWP apps that child
/// lives on another thread than the frame window in the foreground.
fn focus_thread() -> Option<u32> {
    let foreground =
        Some(unsafe { GetForegroundWindow() }).filter(|window| !window.is_invalid())?;
    let thread = unsafe { GetWindowThreadProcessId(foreground, None) };
    let mut info = GUITHREADINFO {
        cbSize: u32::try_from(size_of::<GUITHREADINFO>()).unwrap_or_default(),
        ..Default::default()
    };
    let _ = unsafe { GetGUIThreadInfo(thread, &raw mut info) };
    let focus = Some(info.hwndFocus).filter(|window| !window.is_invalid());
    Some(input_thread(focus.unwrap_or(foreground)))
}

/// The thread that owns the default IME window of `window`, or the thread
/// of `window` itself when it has none.
///
/// For most windows both are the same thread. A legacy console window is
/// the exception: Windows names the console's client, such as `cmd.exe`, as
/// its owner, and the layout of that thread reads as zero. The IME window
/// belongs to the real thread in `conhost.exe`, whose layout follows every
/// switch (see `examples/console.rs`).
fn input_thread(window: HWND) -> u32 {
    unsafe { GetWindowThreadProcessId(input_window(window), None) }
}

/// The default IME window of `window`, or `window` itself when it has none.
/// Its owner is the thread and the process that take the input of `window`;
/// for a legacy console that is `conhost.exe`, whatever runs in it.
fn input_window(window: HWND) -> HWND {
    let ime = Some(unsafe { ImmGetDefaultIMEWnd(window) }).filter(|ime| !ime.is_invalid());
    ime.unwrap_or(window)
}

/// The file name of the executable that owns `window`.
fn executable_of(window: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
    process_executable(pid)
}

/// The file name of the executable of the process `pid`.
fn process_executable(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut path = [0u16; 1024];
    let mut len = u32::try_from(path.len()).unwrap_or_default();
    let name = PWSTR(path.as_mut_ptr());
    let queried =
        unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, name, &raw mut len) };
    let _ = unsafe { CloseHandle(process) };
    let len = queried.ok().and(usize::try_from(len).ok())?;
    let full = String::from_utf16_lossy(path.get(..len)?);
    full.rsplit('\\').next().map(str::to_string)
}

/// Window class and executable, for the log.
fn describe(window: HWND) -> String {
    let mut class = [0u16; 256];
    let len = unsafe { GetClassNameW(window, &mut class) };
    let class = String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or_default()]);
    format!("{class} ({})", executable_of(window).unwrap_or_default())
}

/// One injected key event, with the extended-key flag where the key has it.
fn input(stroke: Stroke) -> INPUT {
    let up = (stroke.kind == KeyKind::Up).then_some(KEYEVENTF_KEYUP);
    let extended = stroke.key.extended().then_some(KEYEVENTF_EXTENDEDKEY);
    let flags = [up, extended].into_iter().flatten();
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(stroke.key.vk()),
                dwFlags: flags.fold(KEYBD_EVENT_FLAGS(0), |all, flag| all | flag),
                ..Default::default()
            },
        },
    }
}

/// A stroke for the log, such as `0x5b↑`. Only modifiers, C, V, X, Insert,
/// Caps Lock and the mask are ever injected, so nothing typed is logged.
fn written(stroke: Stroke) -> String {
    let arrow = match stroke.kind {
        KeyKind::Down => '↓',
        KeyKind::Up => '↑',
    };
    format!("{:#04x}{arrow}", stroke.key.vk())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_is_named_by_its_executable_file() {
        let this_test = std::env::current_exe().unwrap();
        let expected = this_test.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            process_executable(std::process::id()).as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn a_process_that_cannot_be_opened_has_no_name() {
        assert_eq!(process_executable(0), None);
    }

    #[test]
    fn a_window_takes_its_input_on_the_thread_that_made_it() {
        let window = crate::create_window().expect("a window");
        let this_thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
        assert_eq!(input_thread(window), this_thread);
    }

    #[test]
    fn a_window_is_named_after_the_process_that_takes_its_input() {
        let window = crate::create_window().expect("a window");
        let this_test = std::env::current_exe().unwrap();
        let expected = this_test.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            executable_of(input_window(window)).as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn an_injected_event_carries_the_release_and_extended_key_flags() {
        let event = |key, kind| unsafe { input(Stroke { key, kind }).Anonymous.ki };
        let insert_up = event(Key::INSERT, KeyKind::Up);
        assert_eq!(insert_up.wVk, VIRTUAL_KEY(0x2d));
        assert_eq!(insert_up.dwFlags, KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY);
        assert_eq!(
            event(Key::LWIN, KeyKind::Down).dwFlags,
            KEYEVENTF_EXTENDEDKEY
        );
        assert_eq!(event(Key::C, KeyKind::Up).dwFlags, KEYEVENTF_KEYUP);
        assert_eq!(event(Key::C, KeyKind::Down).dwFlags, KEYBD_EVENT_FLAGS(0));
    }

    #[test]
    fn strokes_are_logged_by_key_code_and_direction() {
        let up = Stroke {
            key: Key::LWIN,
            kind: KeyKind::Up,
        };
        let down = Stroke {
            key: Key::C,
            kind: KeyKind::Down,
        };
        assert_eq!(written(up), "0x5b↑");
        assert_eq!(written(down), "0x43↓");
    }

    #[test]
    fn the_mask_key_is_never_down() {
        let platform = Win32 {
            window: HWND::default(),
            on_hold_timer: None,
            hold_token: None,
        };
        assert!(!platform.is_down(Key::MASK));
    }

    #[test]
    fn a_window_that_does_not_exist_has_no_executable() {
        assert_eq!(executable_of(HWND::default()), None);
    }

    #[test]
    fn the_system_reports_at_least_one_installed_layout() {
        let platform = Win32 {
            window: HWND::default(),
            on_hold_timer: None,
            hold_token: None,
        };
        assert_ne!(platform.installed_layouts(), []);
    }
}
