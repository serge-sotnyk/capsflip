//! The capsflip Windows binary: a thin Win32 shell around `capsflip-core`.
//!
//! Everything runs on one thread: the keyboard hook, the timers, the tray
//! icon and the hidden window all call into the one [`App`] kept in a
//! thread local.

// A release build runs without a console. A debug build keeps one, so that
// `cargo run` shows the log as it is written.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod instance;
mod locale;
mod logging;
mod paths;
mod platform;
mod registry;
mod tray;

use std::cell::RefCell;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;

use capsflip_core::autostart;
use capsflip_core::config::{Config, ConfigError, Settings};
use capsflip_core::controller::Controller;
use capsflip_core::key::{Key, KeyEvent, KeyKind, Modifiers};
use capsflip_core::tray::{self as core_tray, Command, Entry, Notice};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{NIN_SELECT, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HC_ACTION,
    HHOOK, KBDLLHOOKSTRUCT, KillTimer, LLKHF_INJECTED, LLKHF_UP, MB_ICONERROR, MB_ICONINFORMATION,
    MB_OK, MESSAGEBOX_STYLE, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SW_SHOWNORMAL, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::instance::Instance;
use crate::logging::log;
use crate::platform::{HOLD_TIMER, Win32};
use crate::registry::RunKey;
use crate::tray::{Tray, WM_TRAY};

/// Posted to the window when the hook has queued work.
const WM_WAKE: u32 = WM_APP + 1;
const POLL_TIMER: usize = 2;
/// How often the foreground layout is read to notice manual switches.
const POLL_MS: u32 = 250;
/// `NIN_SELECT | NINF_KEY`: the tray icon chosen with Enter or Space.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
/// The tray icon events that open the menu: a right click, a left click
/// and the keyboard.
const MENU_EVENTS: [u32; 3] = [WM_CONTEXTMENU, NIN_SELECT, NIN_KEYSELECT];

struct App {
    controller: Controller,
    platform: Win32,
    tray: Tray,
    /// The config file, read again on reload.
    config: PathBuf,
    /// Where "Start at login" is recorded.
    run_key: RunKey,
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
    /// when the window receives `WM_WAKE`, because Windows removes a slow
    /// hook.
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

    /// Shows the tray icon, and the reason the config was not applied when
    /// it was not.
    fn start(&self, problem: Option<&ConfigError>) {
        self.show_icon();
        if let Some(notice) = core_tray::started(problem) {
            log(&format!("{}: {}", notice.title, notice.text));
            self.tray.notify(&notice);
        }
    }

    fn show_icon(&self) {
        let shown = self.tray.show(core_tray::look(self.controller.is_paused()));
        log(&format!("tray icon shown: {shown}"));
    }

    fn menu(&self) -> Vec<Entry> {
        core_tray::menu(self.controller.is_paused(), self.starts_at_login())
    }

    fn starts_at_login(&self) -> bool {
        autostart::starts(self.run_key.read().as_deref(), &current_exe())
    }

    fn on_command(&mut self, command: Command) {
        match command {
            Command::Pause => self.toggle_pause(),
            Command::StartAtLogin => self.toggle_start_at_login(),
            Command::OpenConfig => open(&self.config),
            Command::ReloadConfig => {
                let notice = self.reload();
                self.tray.notify(&notice);
            }
            Command::Quit => unsafe { PostQuitMessage(0) },
        }
    }

    fn toggle_pause(&mut self) {
        let paused = self.controller.toggle_pause();
        log(&format!("paused: {paused}"));
        self.tray.update(core_tray::look(paused));
    }

    fn toggle_start_at_login(&self) {
        let change = autostart::toggle(self.run_key.read().as_deref(), &current_exe());
        let applied = self.run_key.apply(&change);
        log(&format!("start at login: {change:?}: {applied:?}"));
    }

    /// Reads the config file again and applies it unless it is rejected.
    fn reload(&mut self) -> Notice {
        let loaded = Config::load(&self.config, locale::lang_of);
        let applied = loaded.map(|settings| self.controller.reload(settings));
        let notice = core_tray::reloaded(applied.as_ref().copied());
        log(&format!("{}: {}", notice.title, notice.text));
        notice
    }
}

fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// Opens `file` in the program registered for it, or in Notepad when no
/// program is.
fn open(file: &Path) {
    let file = HSTRING::from(file.as_os_str());
    if !shell_open(&file, &HSTRING::new()) {
        shell_open(&HSTRING::from("notepad.exe"), &file);
    }
}

fn shell_open(file: &HSTRING, parameters: &HSTRING) -> bool {
    let instance = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            file,
            parameters,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Success is any value above 32.
    let opened = instance.0.addr() > 32;
    log(&format!("open {file} {parameters}: {opened}"));
    opened
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
    match message {
        WM_WAKE => {
            with_app(|app| app.controller.run_pending(&mut app.platform));
        }
        WM_TRAY => on_tray(window, tray::event(lparam), tray::anchor(wparam)),
        // `taskkill` without `/f` asks the window to close: quit cleanly.
        WM_CLOSE => unsafe { PostQuitMessage(0) },
        _ if message == taskbar_created() => {
            with_app(|app| app.show_icon());
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}

/// Opens the menu after a click or key on the tray icon and carries out
/// the command chosen. The app is not borrowed while the menu is open: the
/// menu runs a message loop of its own, in which the hook and the timers
/// still need the app.
fn on_tray(window: HWND, event: u32, anchor: POINT) {
    if !MENU_EVENTS.contains(&event) {
        return;
    }
    let menu = with_app(|app| app.menu()).and_then(|entries| tray::popup(&entries).ok());
    let chosen = menu.map(|menu| tray::track(window, menu, anchor));
    if let Some(command) = chosen.and_then(Command::from_id) {
        with_app(|app| app.on_command(command));
    }
}

/// The message Explorer broadcasts when it creates the taskbar again, for
/// example after a restart of Explorer. The icon has to be added again.
fn taskbar_created() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe extern "system" fn hold_timer_proc(window: HWND, _message: u32, _id: usize, _time: u32) {
    let _ = unsafe { KillTimer(Some(window), HOLD_TIMER) };
    with_app(App::on_hold_timer);
}

unsafe extern "system" fn poll_timer_proc(_window: HWND, _message: u32, _id: usize, _time: u32) {
    with_app(|app| app.controller.on_poll(&app.platform));
}

/// A top-level window that is never shown. It owns the timers and the tray
/// icon; unlike a message-only window it receives the `TaskbarCreated`
/// broadcast and can bring the tray menu to the front.
fn create_window() -> windows::core::Result<HWND> {
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
            w!("capsflip"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

/// Creates the app with its window, poll timer and keyboard hook. The tray
/// icon is shown by [`App::start`].
fn install(settings: Settings, config: PathBuf, run_key: RunKey) -> windows::core::Result<HHOOK> {
    let window = create_window()?;
    let app = App {
        controller: Controller::new(settings),
        platform: Win32 {
            window,
            on_hold_timer: Some(hold_timer_proc),
            hold_token: None,
        },
        tray: Tray::new(window),
        config,
        run_key,
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
/// message loop is told to quit. A rejected config does not stop it: it
/// runs with the defaults and says why.
fn run(config: &Path) -> Result<(), Box<dyn Error>> {
    let (settings, problem) = Config::load_or_defaults(config, locale::lang_of);
    let hook = install(settings, config.to_path_buf(), RunKey::login())?;
    with_app(|app| app.start(problem.as_ref()));
    pump_messages();
    with_app(|app| app.tray.remove());
    let _ = unsafe { UnhookWindowsHookEx(hook) };
    Ok(())
}

fn main() -> ExitCode {
    logging::rotate();
    log("capsflip is starting");
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // Some programs that open the config file are reached through COM.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    Instance::claim(instance::NAME).map_or_else(already_running, |_instance| start())
}

fn already_running() -> ExitCode {
    log("capsflip is already running");
    alert(
        "capsflip is already running. Find it in the notification area.",
        MB_ICONINFORMATION,
    );
    ExitCode::SUCCESS
}

fn start() -> ExitCode {
    let started = paths::config()
        .ok_or_else(|| "APPDATA is not set".into())
        .and_then(|config| run(&config));
    started.map_or_else(|error| cannot_start(&*error), |()| ExitCode::SUCCESS)
}

fn cannot_start(error: &dyn Error) -> ExitCode {
    let message = format!("capsflip cannot start: {error}");
    log(&message);
    alert(&message, MB_ICONERROR);
    ExitCode::FAILURE
}

fn alert(text: &str, icon: MESSAGEBOX_STYLE) {
    let _ = unsafe { MessageBoxW(None, &HSTRING::from(text), w!("capsflip"), MB_OK | icon) };
}

#[cfg(test)]
mod tests {
    use capsflip_core::tray::Severity;
    use windows::Win32::UI::WindowsAndMessaging::{
        PM_REMOVE, PeekMessageW, SendMessageW, WM_GETTEXTLENGTH, WM_MOUSEMOVE, WM_QUIT,
    };

    use super::*;
    use crate::registry::scratch;

    /// A fresh, empty directory for one test.
    fn scratch_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("capsflip-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The app installed as `run` installs it, but with its config file in
    /// a scratch directory, its Run value under a scratch key and no icon
    /// in the notification area. Dropping it removes the hook and the key.
    struct Installed {
        hook: HHOOK,
        test: &'static str,
        config: PathBuf,
    }

    impl Installed {
        fn new(test: &'static str) -> Self {
            let config = scratch_dir(test).join("config.toml");
            std::fs::write(&config, "").unwrap();
            let hook = install(Settings::default(), config.clone(), scratch::run_key(test));
            Self {
                hook: hook.expect("the app installs"),
                test,
                config,
            }
        }
    }

    impl Drop for Installed {
        fn drop(&mut self) {
            let _ = unsafe { UnhookWindowsHookEx(self.hook) };
            scratch::remove(&scratch::run_key(self.test));
        }
    }

    fn checked(command: Command) -> bool {
        let menu = with_app(|app| app.menu()).expect("the app is installed");
        menu.contains(&Entry::Item {
            command,
            label: label_of(&menu, command),
            checked: true,
        })
    }

    fn label_of(menu: &[Entry], wanted: Command) -> &'static str {
        menu.iter()
            .find_map(|entry| match *entry {
                Entry::Item { command, label, .. } if command == wanted => Some(label),
                _ => None,
            })
            .expect("the command is in the menu")
    }

    /// Empties the queue of this thread and tells whether `WM_QUIT` was in
    /// it. Windows hands out `WM_QUIT` only once the queue is otherwise
    /// empty, so the messages before it are taken out too.
    fn quit_was_posted() -> bool {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == WM_QUIT {
                return true;
            }
        }
        false
    }

    #[test]
    fn it_starts_with_a_fresh_config_and_stops_when_asked_to_quit() {
        let config = scratch_dir("start").join("config.toml");
        unsafe { PostQuitMessage(0) };

        run(&config).expect("capsflip starts and stops");

        assert!(config.exists(), "the config template was written");
        assert!(with_app(|_| ()).is_some(), "the app is installed");
    }

    #[test]
    fn pause_from_the_menu_is_checked_and_a_second_click_resumes() {
        let _app = Installed::new("pause");
        assert!(!checked(Command::Pause));
        with_app(|app| app.on_command(Command::Pause));
        assert!(checked(Command::Pause));
        with_app(|app| app.on_command(Command::Pause));
        assert!(!checked(Command::Pause));
    }

    #[test]
    fn start_at_login_from_the_menu_registers_this_executable() {
        let _app = Installed::new("login");
        with_app(|app| app.on_command(Command::StartAtLogin));
        assert!(checked(Command::StartAtLogin));
        assert_eq!(
            scratch::run_key("login").read(),
            Some(autostart::command(&current_exe()))
        );
        with_app(|app| app.on_command(Command::StartAtLogin));
        assert!(!checked(Command::StartAtLogin));
        assert_eq!(scratch::run_key("login").read(), None);
    }

    #[test]
    fn reload_reports_whether_the_config_was_applied() {
        let app = Installed::new("reload");
        std::fs::write(&app.config, "[caps_lock]\nhold_threshold_ms = 250\n").unwrap();
        assert_eq!(with_app(App::reload).unwrap().severity, Severity::Info);
        std::fs::write(&app.config, "[caps_lock]\nhold_threshold_ms = 1\n").unwrap();
        assert_eq!(with_app(App::reload).unwrap().severity, Severity::Warning);
        // Without an icon in the notification area, the notice goes nowhere.
        with_app(|app| app.on_command(Command::ReloadConfig));
    }

    #[test]
    fn quit_from_the_menu_ends_the_message_loop() {
        let _app = Installed::new("quit");
        with_app(|app| app.on_command(Command::Quit));
        assert!(quit_was_posted());
    }

    #[test]
    fn the_window_quits_when_closed_and_leaves_other_messages_to_windows() {
        let _app = Installed::new("window");
        let window = with_app(|app| app.platform.window).unwrap();
        let send = |message: u32, lparam: isize| unsafe {
            SendMessageW(window, message, Some(WPARAM(0)), Some(LPARAM(lparam)))
        };
        send(WM_WAKE, 0);
        send(WM_TRAY, isize::try_from(WM_MOUSEMOVE).unwrap());
        send(taskbar_created(), 0);
        with_app(|app| app.tray.remove());
        assert_eq!(
            send(WM_GETTEXTLENGTH, 0),
            LRESULT(8),
            "the title, from Windows"
        );
        assert!(!quit_was_posted());

        send(WM_CLOSE, 0);
        assert!(quit_was_posted());
    }
}
