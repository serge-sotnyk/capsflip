//! Spike for Win+C / Win+V / Win+X as copy / paste / cut (#25).
//!
//! `cargo run -p capsflip --example winkeys` swallows Win+C, Win+V and Win+X
//! and sends copy, paste and cut instead; Win+Shift+V sends the original
//! Win+V, the clipboard history. In Windows Terminal and the legacy console,
//! copy and paste go as Ctrl+Insert and Shift+Insert, and cut does nothing.
//!
//! Win+Shift+X is left alone: the quick link menu opened from injected keys
//! does not get the keyboard focus and then does not close on Esc or a
//! click elsewhere, even after `AllowSetForegroundWindow(ASFW_ANY)`.
//!
//! While Win is held, a combination is sent as: the mask, Win up, the
//! combination, Win down, the mask. The mask is a key that does nothing
//! (`vkE8`); it keeps both the injected and the physical release of Win from
//! opening Start.
//!
//! Options after `--`:
//! - `no-extended`: send Insert and Win without the extended-key flag;
//! - `no-trailing-mask`: leave out the last mask, to see whether the
//!   physical release of Win opens Start without it.
//!
//! The log names only modifiers, C, V, X, Insert and the mask; every other
//! key is written as `other`, so nothing typed during the session is kept.
//! It goes to the console and to `target/winkeys.log`. Stop with Ctrl+C.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

#[path = "shared/journal.rs"]
mod journal;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use journal::log;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClassNameW,
    GetForegroundWindow, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP, MSG,
    PostMessageW, RegisterClassW, SetTimer, SetWindowsHookExW, WH_KEYBOARD_LL, WINDOW_EX_STYLE,
    WM_APP, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{PCWSTR, w};

const WM_JOB: u32 = WM_APP + 1;

const VK_C: u16 = 0x43;
const VK_V: u16 = 0x56;
const VK_X: u16 = 0x58;
const VK_INSERT: u16 = 0x2d;
const VK_LWIN: u16 = 0x5b;
const VK_RWIN: u16 = 0x5c;
const VK_LSHIFT: u16 = 0xa0;
const VK_RSHIFT: u16 = 0xa1;
const VK_LCONTROL: u16 = 0xa2;
const VK_RCONTROL: u16 = 0xa3;
const VK_LMENU: u16 = 0xa4;
const VK_RMENU: u16 = 0xa5;
const VK_CONTROL: u16 = 0x11;
const VK_SHIFT: u16 = 0x10;
/// Unassigned: a key that does nothing, used to mask the release of Win.
const VK_MASK: u16 = 0xe8;

#[derive(Clone, Copy, Debug)]
enum Job {
    Copy,
    Paste,
    Cut,
    /// The original Win+<key>, reached with Win+Shift+<key>.
    Native(u16),
}

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
/// Keys whose down was swallowed: their repeats and release are swallowed too.
static SWALLOWED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// Jobs with the hook time of the key that asked for them.
static JOBS: Mutex<Vec<(Job, u32)>> = Mutex::new(Vec::new());
static EXTENDED: AtomicBool = AtomicBool::new(true);
static TRAILING_MASK: AtomicBool = AtomicBool::new(true);

/// The name of a key the spike cares about; any other key is `other`.
fn name(vk: u32) -> &'static str {
    match vk as u16 {
        VK_LWIN => "LWin",
        VK_RWIN => "RWin",
        VK_LSHIFT | VK_SHIFT => "LShift",
        VK_RSHIFT => "RShift",
        VK_LCONTROL | VK_CONTROL => "LCtrl",
        VK_RCONTROL => "RCtrl",
        VK_LMENU => "LAlt",
        VK_RMENU => "RAlt",
        VK_C => "C",
        VK_V => "V",
        VK_X => "X",
        VK_INSERT => "Insert",
        VK_MASK => "mask",
        _ => "other",
    }
}

fn held(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

/// What the hook does with a key that goes down while nothing is swallowed.
fn decide(vk: u32) -> Option<Job> {
    let win = held(VK_LWIN) || held(VK_RWIN);
    let shift = held(VK_LSHIFT) || held(VK_RSHIFT);
    let other = held(VK_LCONTROL) || held(VK_RCONTROL) || held(VK_LMENU) || held(VK_RMENU);
    if !win || other {
        return None;
    }
    match (vk as u16, shift) {
        (VK_C, false) => Some(Job::Copy),
        (VK_V, false) => Some(Job::Paste),
        (VK_X, false) => Some(Job::Cut),
        (VK_V, true) => Some(Job::Native(VK_V)),
        _ => None,
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if hook(info) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Logs the event and tells whether to swallow it.
fn hook(info: &KBDLLHOOKSTRUCT) -> bool {
    let injected = info.flags.contains(LLKHF_INJECTED);
    let up = info.flags.contains(LLKHF_UP);
    let vk = info.vkCode;
    let what = format!(
        "{} {} injected={injected} time={}",
        name(vk),
        if up { "up" } else { "down" },
        info.time
    );
    if injected {
        log("hook", &what);
        return false;
    }
    let mut swallowed = SWALLOWED.lock().unwrap();
    let known = swallowed.contains(&vk);
    if up {
        swallowed.retain(|key| *key != vk);
        log("hook", &format!("{what} swallowed={known}"));
        return known;
    }
    if known {
        log("hook", &format!("{what} repeat swallowed"));
        return true;
    }
    let Some(job) = decide(vk) else {
        log("hook", &what);
        return false;
    };
    swallowed.push(vk);
    JOBS.lock().unwrap().push((job, info.time));
    let window = HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut _);
    let _ = unsafe { PostMessageW(Some(window), WM_JOB, WPARAM(0), LPARAM(0)) };
    log("hook", &format!("{what} swallowed, job {job:?}"));
    true
}

fn input(vk: u16, up: bool) -> INPUT {
    let extended = EXTENDED.load(Ordering::Relaxed)
        && matches!(vk, VK_INSERT | VK_LWIN | VK_RWIN | VK_RCONTROL | VK_RMENU);
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn foreground_class() -> String {
    let mut class = [0u16; 256];
    let len = unsafe { GetClassNameW(GetForegroundWindow(), &mut class) };
    String::from_utf16_lossy(&class[..len as usize])
}

/// The keys to send for `job` in the foreground window, as (modifier, key).
fn combination(job: Job, terminal: bool) -> Option<(u16, u16)> {
    match (job, terminal) {
        (Job::Copy, false) => Some((VK_LCONTROL, VK_C)),
        (Job::Paste, false) => Some((VK_LCONTROL, VK_V)),
        (Job::Cut, false) => Some((VK_LCONTROL, VK_X)),
        (Job::Copy, true) => Some((VK_LCONTROL, VK_INSERT)),
        (Job::Paste, true) => Some((VK_LSHIFT, VK_INSERT)),
        (Job::Cut, true) => None,
        (Job::Native(key), _) => Some((0, key)),
    }
}

/// The events that send `modifier`+`key` from the keys held right now.
fn strokes(modifier: u16, key: u16) -> Vec<(u16, bool)> {
    let win = [VK_LWIN, VK_RWIN].into_iter().find(|vk| held(*vk));
    let shifts: Vec<u16> = [VK_LSHIFT, VK_RSHIFT]
        .into_iter()
        .filter(|vk| held(*vk))
        .collect();
    let mut keys = Vec::new();
    if modifier == 0 {
        // The original Win+<key>: Win stays down, Shift goes up for the key.
        keys.extend(shifts.iter().map(|vk| (*vk, true)));
        keys.extend([(key, false), (key, true)]);
        keys.extend(shifts.iter().rev().map(|vk| (*vk, false)));
        return keys;
    }
    if let Some(win) = win {
        keys.extend([(VK_MASK, false), (VK_MASK, true), (win, true)]);
    }
    let release_shift = modifier != VK_LSHIFT;
    if release_shift {
        keys.extend(shifts.iter().map(|vk| (*vk, true)));
    }
    keys.extend([
        (modifier, false),
        (key, false),
        (key, true),
        (modifier, true),
    ]);
    if release_shift {
        keys.extend(shifts.iter().rev().map(|vk| (*vk, false)));
    }
    if let Some(win) = win {
        keys.push((win, false));
        if TRAILING_MASK.load(Ordering::Relaxed) {
            keys.extend([(VK_MASK, false), (VK_MASK, true)]);
        }
    }
    keys
}

fn run(job: Job, asked_at: u32) {
    let class = foreground_class();
    let terminal = class == "CASCADIA_HOSTING_WINDOW_CLASS" || class == "ConsoleWindowClass";
    let late = unsafe { GetTickCount() }.wrapping_sub(asked_at);
    let win_held = held(VK_LWIN) || held(VK_RWIN);
    let Some((modifier, key)) = combination(job, terminal) else {
        log(
            "job",
            &format!("{job:?} in {class}: nothing to send ({late} ms after the key)"),
        );
        return;
    };
    let keys = strokes(modifier, key);
    let inputs: Vec<INPUT> = keys.iter().map(|(vk, up)| input(*vk, *up)).collect();
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    let listed: Vec<String> = keys
        .iter()
        .map(|(vk, up)| format!("{}{}", name(u32::from(*vk)), if *up { "↑" } else { "↓" }))
        .collect();
    log(
        "job",
        &format!(
            "{job:?} in {class}: {} ms after the key, Win held={win_held}, sent {sent}/{}: {}",
            late,
            inputs.len(),
            listed.join(" ")
        ),
    );
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_JOB => {
            let jobs = std::mem::take(&mut *JOBS.lock().unwrap());
            for (job, asked_at) in jobs {
                run(job, asked_at);
            }
        }
        WM_TIMER => journal::flush("target/winkeys.log"),
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    LRESULT(0)
}

fn main() -> windows::core::Result<()> {
    journal::start();
    EXTENDED.store(
        !std::env::args().any(|a| a == "no-extended"),
        Ordering::Relaxed,
    );
    TRAILING_MASK.store(
        !std::env::args().any(|a| a == "no-trailing-mask"),
        Ordering::Relaxed,
    );
    log(
        "start",
        &format!(
            "extended flag: {}, trailing mask: {}",
            EXTENDED.load(Ordering::Relaxed),
            TRAILING_MASK.load(Ordering::Relaxed)
        ),
    );

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("capsflip_winkeys"),
            ..Default::default()
        };
        RegisterClassW(&raw const class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("capsflip_winkeys"),
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
        let _hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            Some(instance.into()),
            0,
        )?;
        log(
            "start",
            "hook installed; Win+C/V/X are now copy/paste/cut. Ctrl+C here to stop.",
        );
        SetTimer(Some(hwnd), 1, 200, None);

        let mut message = MSG::default();
        while GetMessageW(&raw mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&raw const message);
        }
    }
    Ok(())
}
