//! The tray icon and its menu, drawn with Win32 from what the core decides.

use capsflip_core::icon;
use capsflip_core::tray::{Entry, Icon, Look, Notice, Severity};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFY_ICON_MESSAGE,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, DestroyIcon, DestroyMenu, GetSystemMetrics, HICON,
    HMENU, MF_CHECKED, MF_SEPARATOR, MF_STRING, PostMessageW, SM_CXSMICON, SetForegroundWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_NULL,
};
use windows::core::HSTRING;

/// Sent to the window for clicks and keys on the tray icon.
pub const WM_TRAY: u32 = WM_APP + 2;
/// Identifies the one icon among the icons of the window.
const ICON_ID: u32 = 1;

pub struct Tray {
    window: HWND,
    active: HICON,
    paused: HICON,
}

impl Tray {
    /// Draws both icons at the size of a small icon on this display.
    pub fn new(window: HWND) -> Self {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
        Self {
            window,
            active: create_icon(Icon::Active, size),
            paused: create_icon(Icon::Paused, size),
        }
    }

    /// Adds the icon to the notification area. This fails while there is
    /// no taskbar, for example on a build server.
    pub fn show(&self, look: Look) -> bool {
        let mut data = self.looking(look);
        data.uFlags |= NIF_MESSAGE;
        data.uCallbackMessage = WM_TRAY;
        let added = send(NIM_ADD, &data);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        send(NIM_SETVERSION, &data);
        added
    }

    pub fn update(&self, look: Look) {
        send(NIM_MODIFY, &self.looking(look));
    }

    /// Shows `notice` next to the icon. Nothing is shown without the icon.
    pub fn notify(&self, notice: &Notice) {
        let data = NOTIFYICONDATAW {
            uFlags: NIF_INFO,
            szInfoTitle: wide(notice.title),
            szInfo: wide(&notice.text),
            dwInfoFlags: info_flags(notice.severity),
            ..self.data()
        };
        send(NIM_MODIFY, &data);
    }

    pub fn remove(&self) {
        send(NIM_DELETE, &self.data());
    }

    fn looking(&self, look: Look) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            uFlags: NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            hIcon: match look.icon {
                Icon::Active => self.active,
                Icon::Paused => self.paused,
            },
            szTip: wide(look.tooltip),
            ..self.data()
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).unwrap_or_default(),
            hWnd: self.window,
            uID: ICON_ID,
            ..Default::default()
        }
    }
}

fn send(message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
    unsafe { Shell_NotifyIconW(message, data) }.as_bool()
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = unsafe { DestroyIcon(self.active) };
        let _ = unsafe { DestroyIcon(self.paused) };
    }
}

const fn info_flags(severity: Severity) -> NOTIFY_ICON_INFOTIP_FLAGS {
    match severity {
        Severity::Info => NIIF_INFO,
        Severity::Warning => NIIF_WARNING,
    }
}

/// Makes a `size` × `size` icon from the pixels the core draws.
fn create_icon(icon: Icon, size: i32) -> HICON {
    let width = usize::try_from(size).unwrap_or_default();
    let color: Vec<u8> = icon::pixels(icon, width)
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    // The mask has one bit per pixel in rows padded to 16 bits. All zero:
    // the alpha of the color pixels decides what shows.
    let mask = vec![0u8; width.div_ceil(16) * 2 * width];
    unsafe { CreateIcon(None, size, size, 1, 32, mask.as_ptr(), color.as_ptr()) }
        .unwrap_or_default()
}

/// `text` as a nul-terminated UTF-16 buffer, cut short when it is too long.
fn wide<const N: usize>(text: &str) -> [u16; N] {
    let mut buffer = [0u16; N];
    let room = buffer.len().saturating_sub(1);
    for (slot, unit) in buffer[..room].iter_mut().zip(text.encode_utf16()) {
        *slot = unit;
    }
    buffer
}

/// Builds the popup menu for `entries`.
pub fn popup(entries: &[Entry]) -> windows::core::Result<HMENU> {
    let menu = unsafe { CreatePopupMenu() }?;
    for entry in entries {
        let (flags, id, label) = match *entry {
            Entry::Item {
                command,
                label,
                checked,
            } => (
                if checked { MF_CHECKED } else { MF_STRING },
                command.id(),
                label,
            ),
            Entry::Separator => (MF_SEPARATOR, 0, ""),
        };
        let id = usize::try_from(id).unwrap_or_default();
        unsafe { AppendMenuW(menu, flags, id, &HSTRING::from(label)) }?;
    }
    Ok(menu)
}

/// Shows `menu` at `anchor`, waits for a choice and destroys the menu.
/// Returns the identifier of the item chosen, or zero.
pub fn track(window: HWND, menu: HMENU, anchor: POINT) -> u32 {
    // Without this the menu does not close when the user clicks elsewhere.
    let _ = unsafe { SetForegroundWindow(window) };
    let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON;
    let chosen = unsafe { TrackPopupMenuEx(menu, flags.0, anchor.x, anchor.y, window, None) };
    let _ = unsafe { PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0)) };
    let _ = unsafe { DestroyMenu(menu) };
    u32::try_from(chosen.0).unwrap_or_default()
}

/// The event in a [`WM_TRAY`] message: the low word of `lparam`.
pub fn event(lparam: LPARAM) -> u32 {
    let [low, high, ..] = lparam.0.to_le_bytes();
    u32::from(u16::from_le_bytes([low, high]))
}

/// Where the menu opens: the signed screen point packed into `wparam`.
pub fn anchor(wparam: WPARAM) -> POINT {
    let [x_low, x_high, y_low, y_high, ..] = wparam.0.to_le_bytes();
    POINT {
        x: i32::from(i16::from_le_bytes([x_low, x_high])),
        y: i32::from(i16::from_le_bytes([y_low, y_high])),
    }
}

#[cfg(test)]
mod tests {
    use capsflip_core::tray::{Command, menu};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMenuItemCount, GetMenuState, MF_BYCOMMAND, MF_BYPOSITION,
    };

    use super::*;

    #[test]
    fn text_is_cut_to_fit_and_always_ends_with_nul() {
        assert_eq!(wide::<4>("ab"), [0x61, 0x62, 0, 0]);
        assert_eq!(wide::<4>("abcdef"), [0x61, 0x62, 0x63, 0]);
        assert_eq!(wide::<3>("ї"), [0x457, 0, 0]);
    }

    #[test]
    fn the_event_is_the_low_word_and_the_anchor_two_signed_words() {
        assert_eq!(event(LPARAM(0x0001_0400)), 0x400);
        let packed = (300usize << 16) | usize::from(0xfffbu16);
        assert_eq!(anchor(WPARAM(packed)), POINT { x: -5, y: 300 });
    }

    #[test]
    fn an_icon_is_made_from_the_pixels_of_the_core() {
        let icon = create_icon(Icon::Paused, 20);
        assert!(!icon.is_invalid());
        unsafe { DestroyIcon(icon) }.unwrap();
    }

    #[test]
    fn the_menu_has_every_entry_and_its_check_marks() {
        let menu = popup(&menu(true, false)).unwrap();
        let state = |id: u32| unsafe { GetMenuState(menu, id, MF_BYCOMMAND) };
        assert_eq!(unsafe { GetMenuItemCount(Some(menu)) }, 7);
        assert_ne!(state(Command::Pause.id()) & MF_CHECKED.0, 0);
        assert_eq!(state(Command::StartAtLogin.id()) & MF_CHECKED.0, 0);
        let separator = unsafe { GetMenuState(menu, 2, MF_BYPOSITION) };
        assert_ne!(separator & MF_SEPARATOR.0, 0);
        unsafe { DestroyMenu(menu) }.unwrap();
    }
}
