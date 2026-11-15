//! The capsflip Windows binary: a thin Win32 shell around `capsflip-core`.
//!
//! Everything runs on one thread: the keyboard hook, the timers and the
//! message window all call into the one [`App`] kept in a thread local.

mod locale;
mod logging;
mod paths;
mod platform;

use std::cell::RefCell;
use std::error::Error;
use std::path::Path;
use std::process::ExitCode;

use capsflip_core::config::{Config, Settings};
use capsflip_core::controller::Controller;
use capsflip_core::key::{Key, KeyEvent, KeyKind, Modifiers};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HC_ACTION,
    HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, KillTimer, LLKHF_INJECTED, LLKHF_UP, MSG, PostMessageW,
    RegisterClassW, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use crate::logging::log;
use crate::platform::{HOLD_TIMER, Win32};

/// Posted to the message window when the hook has queued work.
const WM_WAKE: u32 = WM_APP + 1;
const POLL_TIMER: usize = 2;
/// How often the foreground layout is read to notice manual switches.
const POLL_MS: u32 = 250;

struct App {
    controller: Controller,
    platform: Win32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app. Returns `None` before start-up has finished or when
/// the app is already in use further up the stack.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|app| app.try_borrow_mut().ok()?.as_mut().map(f))
}

impl App {
    /// Decides about a key inside the hook. The work itself is done later,
    /// when the message window receives `WM_WAKE`, because Windows removes
    /// a slow hook.
    fn on_key(&mut self, event: KeyEvent) -> bool {
        let swallowed = self.controller.on_key(event, &self.platform);
        if swallowed {
            let window = self.platform.window;
            let _ = unsafe { PostMessageW(Some(window), WM_WAKE, WPARAM(0), LPARAM(0)) };
        }
        swallowed
    }

    fn on_hold_timer(&mut self) {
        if let Some(token) = self.platform.hold_token.take() {
            self.controller.on_hold_timer(token, &mut self.platform);
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if swallow(code, lparam) {
        return LRESULT(1);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Whether the hook must swallow this event. Only `HC_ACTION` events carry
/// a key; the others must go to the next hook untouched.
fn swallow(code: i32, lparam: LPARAM) -> bool {
    let event = (u32::try_from(code) == Ok(HC_ACTION))
        .then(|| key_event(unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) }));
    event
        .and_then(|event| with_app(|app| app.on_key(event)))
        .unwrap_or(false)
}

fn key_event(info: &KBDLLHOOKSTRUCT) -> KeyEvent {
    let up = info.flags.contains(LLKHF_UP);
    KeyEvent {
        key: Key::from_vk(u16::try_from(info.vkCode).unwrap_or_default()),
        kind: if up { KeyKind::Up } else { KeyKind::Down },
        mods: Modifiers {
            shift: held(VK_SHIFT),
            ctrl: held(VK_CONTROL),
            alt: held(VK_MENU),
            win: held(VK_LWIN) | held(VK_RWIN),
        },
        injected: info.flags.contains(LLKHF_INJECTED),
    }
}

fn held(key: VIRTUAL_KEY) -> bool {
    let state = unsafe { GetAsyncKeyState(i32::from(key.0)) };
    state < 0
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message != WM_WAKE {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    with_app(|app| app.controller.run_pending(&mut app.platform));
    LRESULT(0)
}

unsafe extern "system" fn hold_timer_proc(window: HWND, _message: u32, _id: usize, _time: u32) {
    let _ = unsafe { KillTimer(Some(window), HOLD_TIMER) };
    with_app(App::on_hold_timer);
}

unsafe extern "system" fn poll_timer_proc(_window: HWND, _message: u32, _id: usize, _time: u32) {
    with_app(|app| app.controller.on_poll(&app.platform));
}

fn create_message_window() -> windows::core::Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None) }?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: w!("capsflip"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&raw const class) };
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("capsflip"),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )
    }
}

/// Creates the app with its message window, poll timer and keyboard hook.
fn install(settings: Settings) -> windows::core::Result<HHOOK> {
    let window = create_message_window()?;
    let app = App {
        controller: Controller::new(settings),
        platform: Win32 {
            window,
            on_hold_timer: Some(hold_timer_proc),
            hold_token: None,
        },
    };
    APP.with(|slot| slot.replace(Some(app)));
    unsafe { SetTimer(Some(window), POLL_TIMER, POLL_MS, Some(poll_timer_proc)) };

    let instance = unsafe { GetModuleHandleW(None) }?;
    let hook = (
        WH_KEYBOARD_LL,
        Some(keyboard_proc as _),
        Some(instance.into()),
    );
    unsafe { SetWindowsHookExW(hook.0, hook.1, hook.2, 0) }
}

/// Dispatches messages until `WM_QUIT` arrives.
fn pump_messages() {
    let mut message = MSG::default();
    while unsafe { GetMessageW(&raw mut message, None, 0, 0) }.as_bool() {
        unsafe { DispatchMessageW(&raw const message) };
    }
}

/// Starts capsflip with the config file at `config` and runs until the
/// message loop is told to quit.
fn run(config: &Path) -> Result<(), Box<dyn Error>> {
    let settings = Config::load(config, locale::lang_of)?;
    let hook = install(settings)?;
    pump_messages();
    let _ = unsafe { UnhookWindowsHookEx(hook) };
    Ok(())
}

fn main() -> ExitCode {
    log("capsflip is starting");
    let started = paths::config()
        .ok_or_else(|| "APPDATA is not set".into())
        .and_then(|config| run(&config));
    let Err(error) = started else {
        return ExitCode::SUCCESS;
    };
    log(&format!("capsflip cannot start: {error}"));
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;

    use super::*;

    /// A fresh, empty directory for one test.
    fn scratch(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("capsflip-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn it_starts_with_a_fresh_config_and_stops_when_asked_to_quit() {
        let config = scratch("start").join("config.toml");
        unsafe { PostQuitMessage(0) };

        run(&config).expect("capsflip starts and stops");

        assert!(config.exists(), "the config template was written");
        assert!(with_app(|_| ()).is_some(), "the app is installed");
    }

    #[test]
    fn it_refuses_to_start_with_a_broken_config() {
        let config = scratch("broken").join("config.toml");
        std::fs::write(&config, "[layout]\nenglish = 5\n").unwrap();

        let error = run(&config)
            .expect_err("a broken config is fatal")
            .to_string();

        assert!(error.contains("config.toml"), "{error}");
        assert!(with_app(|_| ()).is_none(), "nothing was installed");
    }
}
