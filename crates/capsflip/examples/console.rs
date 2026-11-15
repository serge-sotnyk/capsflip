//! Checks how to read the keyboard layout of a legacy console window.
//!
//! `cargo run -p capsflip --example console` reads every visible
//! `ConsoleWindowClass` window two ways: through the thread that
//! `GetWindowThreadProcessId` names for the window, and through the thread
//! that owns the window's default IME window. With `-- <hkl in hex>` it then
//! asks each console to switch to that layout and reads again, to see which
//! reading follows. No key has to be pressed.

use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    WM_INPUTLANGCHANGEREQUEST,
};
use windows::core::BOOL;

fn main() {
    let request = std::env::args()
        .nth(1)
        .and_then(|hkl| u64::from_str_radix(hkl.trim_start_matches("0x"), 16).ok())
        .map(u64::cast_signed);
    let consoles = consoles();
    println!("{} visible console windows", consoles.len());
    for &window in &consoles {
        report("before", window);
    }
    let Some(hkl) = request else { return };
    for &window in &consoles {
        let posted = unsafe {
            PostMessageW(
                Some(window),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(isize::try_from(hkl).unwrap_or_default()),
            )
        };
        println!("request {hkl:#x} to {window:?}: {posted:?}");
    }
    std::thread::sleep(Duration::from_millis(500));
    for &window in &consoles {
        report("after", window);
    }
}

fn report(when: &str, window: HWND) {
    let (owner_pid, owner_thread) = owner(window);
    let ime = unsafe { ImmGetDefaultIMEWnd(window) };
    let (ime_pid, ime_thread) = owner(ime);
    println!(
        "{when} {window:?}: owner pid {owner_pid} thread {owner_thread} layout {:#x} | \
         ime {ime:?} pid {ime_pid} thread {ime_thread} layout {:#x}",
        layout(owner_thread),
        layout(ime_thread),
    );
}

fn owner(window: HWND) -> (u32, u32) {
    let mut pid = 0u32;
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
    (pid, thread)
}

fn layout(thread: u32) -> usize {
    unsafe { GetKeyboardLayout(thread) }.0.addr()
}

fn consoles() -> Vec<HWND> {
    let mut found: Vec<HWND> = Vec::new();
    let list = LPARAM((&raw mut found).addr().cast_signed());
    let _ = unsafe { EnumWindows(Some(collect), list) };
    found
}

unsafe extern "system" fn collect(window: HWND, list: LPARAM) -> BOOL {
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(window, &mut class) };
    let class = String::from_utf16_lossy(&class[..usize::try_from(len).unwrap_or_default()]);
    let visible = unsafe { IsWindowVisible(window) }.as_bool();
    if class == "ConsoleWindowClass" && visible {
        let found = unsafe { &mut *(list.0 as *mut Vec<HWND>) };
        found.push(window);
    }
    BOOL(1)
}
