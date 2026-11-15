//! Diagnostic probe for the Win32 behaviour capsflip depends on.
//!
//! Run with `cargo run -p capsflip --example probe`, optionally followed by
//! `-- focus` to send layout requests to the focused child window instead of
//! the foreground window. Stop it with Ctrl+C in its console.
//!
//! While it runs, Caps Lock is swallowed:
//! - a Caps Lock tap toggles between en-US and the first non-English layout;
//! - Shift+Caps Lock injects a real Caps Lock press.
//!
//! Every line is tagged with the mechanism that produced it (`hook`, `shell`,
//! `winevent`, `poll`, `action`) and is written to the console and to
//! `target/probe.log`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

#[path = "shared/journal.rs"]
mod journal;

use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use journal::log;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Globalization::LCIDToLocaleName;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, GetKeyboardLayoutList, HKL, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CAPITAL,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetMessageW,
    GetWindowThreadProcessId, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP, MSG,
    PostMessageW, RegisterClassW, RegisterShellHookWindow, RegisterWindowMessageW, SetTimer,
    SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINEVENT_OUTOFCONTEXT,
    WM_APP, WM_INPUTLANGCHANGEREQUEST, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{PCWSTR, PWSTR, w};

const WM_TAP: u32 = WM_APP + 1;
const WM_REAL_CAPS: u32 = WM_APP + 2;
const POLL_MS: u32 = 250;
const HSHELL_LANGUAGE: usize = 8;
const LANG_EN_US: usize = 0x0409;
const PRIMARY_LANG_ENGLISH: usize = 0x09;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static SHELL_MSG: AtomicU32 = AtomicU32::new(0);
static CAPS_DOWN: AtomicBool = AtomicBool::new(false);
static SHIFT_AT_DOWN: AtomicBool = AtomicBool::new(false);
static POST_TO_FOCUS: AtomicBool = AtomicBool::new(false);
static LAST_POLL: Mutex<(isize, isize, isize)> = Mutex::new((0, 0, 0));

fn lang_of(hkl: HKL) -> usize {
    hkl.0 as usize & 0xFFFF
}

fn is_english(hkl: HKL) -> bool {
    lang_of(hkl) & 0x3FF == PRIMARY_LANG_ENGLISH
}

fn describe(hkl: HKL) -> String {
    let mut name = [0u16; 85];
    let len = unsafe { LCIDToLocaleName(lang_of(hkl) as u32, Some(&mut name), 0) };
    let locale = String::from_utf16_lossy(&name[..(len.max(1) - 1) as usize]);
    format!("{:#010x}({locale})", hkl.0 as usize)
}

fn installed_layouts() -> Vec<HKL> {
    let count = unsafe { GetKeyboardLayoutList(None) };
    let mut layouts = vec![HKL::default(); count as usize];
    let filled = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    layouts.truncate(filled as usize);
    layouts
}

struct Snapshot {
    foreground: HWND,
    focus: HWND,
    layout_of_foreground: HKL,
    layout_of_focus: HKL,
    class: String,
    exe: String,
}

fn snapshot() -> Snapshot {
    unsafe {
        let foreground = GetForegroundWindow();
        let mut pid = 0u32;
        let foreground_thread = GetWindowThreadProcessId(foreground, Some(&raw mut pid));
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let focus = if GetGUIThreadInfo(foreground_thread, &raw mut info).is_ok()
            && !info.hwndFocus.is_invalid()
        {
            info.hwndFocus
        } else {
            foreground
        };
        let focus_thread = GetWindowThreadProcessId(focus, None);
        let mut class = [0u16; 256];
        let class_len = GetClassNameW(foreground, &mut class);
        Snapshot {
            foreground,
            focus,
            layout_of_foreground: GetKeyboardLayout(foreground_thread),
            layout_of_focus: GetKeyboardLayout(focus_thread),
            class: String::from_utf16_lossy(&class[..class_len as usize]),
            exe: exe_name(pid),
        }
    }
}

fn exe_name(pid: u32) -> String {
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return "?".into();
        };
        let mut path = [0u16; 1024];
        let mut len = path.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &raw mut len,
        );
        let _ = CloseHandle(process);
        if result.is_err() {
            return "?".into();
        }
        let full = String::from_utf16_lossy(&path[..len as usize]);
        full.rsplit('\\').next().unwrap_or("?").to_string()
    }
}

fn describe_snapshot(s: &Snapshot) -> String {
    let mut text = format!(
        "fg={:#x} class={} exe={} layout(fg thread)={}",
        s.foreground.0 as usize,
        s.class,
        s.exe,
        describe(s.layout_of_foreground),
    );
    if s.focus != s.foreground {
        let _ = write!(
            text,
            " focus={:#x} layout(focus thread)={}",
            s.focus.0 as usize,
            describe(s.layout_of_focus)
        );
    }
    text
}

fn caps_state() -> &'static str {
    if unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 == 1 {
        "ON"
    } else {
        "off"
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if info.vkCode == u32::from(VK_CAPITAL.0) {
            let injected = info.flags.contains(LLKHF_INJECTED);
            let up = info.flags.contains(LLKHF_UP);
            log(
                "hook",
                &format!(
                    "CapsLock {} injected={injected} time={}",
                    if up { "up" } else { "down" },
                    info.time
                ),
            );
            if !injected {
                if up {
                    CAPS_DOWN.store(false, Ordering::Relaxed);
                    let message = if SHIFT_AT_DOWN.load(Ordering::Relaxed) {
                        WM_REAL_CAPS
                    } else {
                        WM_TAP
                    };
                    let hwnd = HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut _);
                    let _ = unsafe { PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0)) };
                } else if !CAPS_DOWN.swap(true, Ordering::Relaxed) {
                    let shift = unsafe { GetAsyncKeyState(i32::from(VK_SHIFT.0)) } < 0;
                    SHIFT_AT_DOWN.store(shift, Ordering::Relaxed);
                }
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn foreground_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    log(
        "winevent",
        &format!("foreground changed: {}", describe_snapshot(&snapshot())),
    );
}

fn on_tap() {
    let s = snapshot();
    let current = s.layout_of_focus;
    let layouts = installed_layouts();
    let target = if is_english(current) {
        layouts.iter().copied().find(|l| !is_english(*l))
    } else {
        layouts.iter().copied().find(|l| lang_of(*l) == LANG_EN_US)
    };
    let Some(target) = target else {
        log("action", "tap: no target layout found");
        return;
    };
    let to_focus = POST_TO_FOCUS.load(Ordering::Relaxed);
    let receiver = if to_focus { s.focus } else { s.foreground };
    let result = unsafe {
        PostMessageW(
            Some(receiver),
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(0),
            LPARAM(target.0 as isize),
        )
    };
    log(
        "action",
        &format!(
            "tap: current={} request={} posted to {} {:#x} result={result:?} caps={}",
            describe(current),
            describe(target),
            if to_focus { "focus" } else { "foreground" },
            receiver.0 as usize,
            caps_state(),
        ),
    );
}

fn on_real_caps() {
    let before = caps_state();
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_CAPITAL,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    log(
        "action",
        &format!("Shift+Caps: injected {sent} events, caps before={before}"),
    );
}

fn on_poll() {
    let s = snapshot();
    let key = (
        s.foreground.0 as isize,
        s.layout_of_foreground.0 as isize,
        s.layout_of_focus.0 as isize,
    );
    let mut last = LAST_POLL.lock().unwrap();
    if *last != key {
        *last = key;
        log(
            "poll",
            &format!("{} caps={}", describe_snapshot(&s), caps_state()),
        );
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TAP => on_tap(),
        WM_REAL_CAPS => on_real_caps(),
        WM_TIMER => {
            on_poll();
            journal::flush("target/probe.log");
        }
        m if m != 0 && m == SHELL_MSG.load(Ordering::Relaxed) => {
            if wparam.0 == HSHELL_LANGUAGE {
                log(
                    "shell",
                    &format!(
                        "HSHELL_LANGUAGE lparam={:#x} now: {}",
                        lparam.0,
                        describe_snapshot(&snapshot())
                    ),
                );
            }
        }
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    LRESULT(0)
}

fn main() -> windows::core::Result<()> {
    journal::start();
    POST_TO_FOCUS.store(std::env::args().any(|a| a == "focus"), Ordering::Relaxed);

    log(
        "start",
        &format!(
            "post layout requests to focus window: {}",
            POST_TO_FOCUS.load(Ordering::Relaxed)
        ),
    );
    for layout in installed_layouts() {
        log("start", &format!("installed layout {}", describe(layout)));
    }

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("capsflip_probe"),
            ..Default::default()
        };
        RegisterClassW(&raw const class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("capsflip_probe"),
            PCWSTR::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

        SHELL_MSG.store(RegisterWindowMessageW(w!("SHELLHOOK")), Ordering::Relaxed);
        let shell = RegisterShellHookWindow(hwnd);
        log("start", &format!("RegisterShellHookWindow -> {shell:?}"));

        let winevent = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        log(
            "start",
            &format!("SetWinEventHook -> valid={}", !winevent.is_invalid()),
        );

        let _hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            Some(instance.into()),
            0,
        )?;
        log(
            "start",
            "keyboard hook installed; Caps Lock is now swallowed. Ctrl+C here to stop.",
        );

        SetTimer(Some(hwnd), 1, POLL_MS, None);

        let mut message = MSG::default();
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    Ok(())
}
