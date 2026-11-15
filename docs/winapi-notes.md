# Win32 notes

What was measured on a real machine, as opposed to read in documentation.
The tool is `crates/capsflip/examples/probe.rs`.

## Measured so far

Windows 11 Pro 10.0.26100, x64, per-window layouts enabled.

- `GetKeyboardLayoutList` returned five layouts although three are configured:

  | HKL | Language | Note |
  |---|---|---|
  | `0x04090409` | en-US | configured |
  | `0x04190419` | ru-RU | configured |
  | `0xfffffffff0a80422` | uk-UA | configured, Ukrainian Enhanced |
  | `0x08090809` | en-GB | phantom |
  | `0xfffffffff0010809` | en-GB | phantom |

- Handles of layout variants are sign-extended to 64 bits. The language is
  the low word in every case.
- The first non-English layout in the list is Russian, not Ukrainian. Without
  `default_secondary` in config, a fresh start pairs English with Russian.
- `RegisterShellHookWindow` and `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`
  both succeed from an unelevated process with a hidden top-level window.

## First probe session

Run in Notepad++ (a classic Win32 app) and in the Claude desktop app
(Electron, window class `Chrome_WidgetWin_1`).

- **Swallowing works.** With Caps Lock swallowed in the hook, the Caps Lock
  state stayed off through every tap and through holds of 1.5 s and 2.7 s.
- **Auto-repeat reaches the hook.** A held key repeats `down` after about
  515 ms and then every 31 ms or so, with a single `up` at the end.
- **Injected input is flagged and toggles the real state.** The two events
  sent with `SendInput` came back through the hook with `LLKHF_INJECTED`,
  and the Caps Lock state went off, on, off across two injections.
- **`WM_INPUTLANGCHANGEREQUEST` posted to the foreground window works** in
  both apps. The new layout was readable 60 to 240 ms later.
- **The foreground thread reports the right layout** in both apps. In
  Notepad++ the focused child window is a different window on the same
  thread, with the same layout.
- **`HSHELL_LANGUAGE` never arrived.** Four manual switches with Ctrl+Shift
  produced no shell hook message; only the polling timer saw them. Manual
  switches therefore have to be observed by polling.
- **`EVENT_SYSTEM_FOREGROUND` arrives** on every window change, but it says
  nothing about the layout of the window that was left, so it cannot replace
  polling either.

Decision: the shell polls the foreground layout on a timer and feeds each
reading to the tracker. The shell hook and the foreground event are not used.

The probe itself always pairs English with the first non-English layout and
has no hold logic. Both are deliberate simplifications of the probe; the real
behaviour lives in `capsflip-core`.

## Second session: the real capsflip, 2026-11-15

The remaining windows were tried with capsflip itself rather than the
probe; its log names the window class and executable of every request.

| Window | Class (executable) | Result |
|---|---|---|
| Notepad++ | `Notepad++` (`notepad++.exe`) | works |
| Chrome | `Chrome_WidgetWin_1` (`chrome.exe`) | works |
| Claude desktop (Electron) | `Chrome_WidgetWin_1` (`claude.exe`) | works |
| Windows Terminal | `CASCADIA_HOSTING_WINDOW_CLASS` (`WindowsTerminal.exe`) | works |
| New Notepad | `Notepad` (`Notepad.exe`) | works |
| Settings, search box (UWP) | `ApplicationFrameWindow` (`ApplicationFrameHost.exe`) | works |
| Start menu search | `Windows.UI.Core.CoreWindow` (`SearchHost.exe`) | works |
| Legacy console | `ConsoleWindowClass` (`cmd.exe`) | every tap asked for English |
| Terminal run as administrator | none | Caps Lock native, as designed |

In UWP apps the request posted to the foreground window is enough; the
focused child window only matters for reading the layout.

### The legacy console

The log showed every tap in the legacy console asking for en-US. The cause,
measured with `cargo run -p capsflip --example console`, which needs no key
presses:

- `GetWindowThreadProcessId` on the console window names `cmd.exe` and one
  of its threads. `GetKeyboardLayout` of that thread returns 0, which is not
  English, so every tap "switched back" to English.
- The console's default IME window (`ImmGetDefaultIMEWnd`) belongs to the
  real `conhost.exe` process. Its thread reported en-US, then Ukrainian
  Enhanced after the example posted `WM_INPUTLANGCHANGEREQUEST` for it to
  the console window, then en-US again after the request for en-US.

Decision: read the layout of the thread that owns the IME window of the
focused window, falling back to the window's own thread. For other windows
it is the same thread, so there is no special case for consoles, and the
per-window memory planned for them is not needed.

## Running the probe

The probe answered the questions below in the first session. It stays as a
tool for the next unknown window. Start it in a terminal:

```
cargo run -p capsflip --example probe
```

While it runs, Caps Lock is swallowed: a tap asks the foreground window to
switch between en-US and the first non-English layout, and Shift+Caps Lock
injects a real Caps Lock. Everything is logged to `target/probe.log`.

In each of these windows, tap Caps Lock, type a few letters, tap again and
type again. Note any window where the letters did not change language:

1. Notepad
2. a browser
3. VS Code
4. Windows Terminal
5. the legacy console: start it with `conhost.exe cmd`
6. Settings (a UWP app), in its search box
7. the Start menu search box

Then, in Notepad:

8. switch to Russian by hand (Ctrl+Shift or Win+Space), wait a second, and
   switch back the same way;
9. press Shift+Caps Lock, type, press it again;
10. hold Caps Lock for two seconds and release it.

Repeat step 8 in the legacy console. Stop the probe with Ctrl+C.

If a window in steps 1 to 7 did not switch, run the probe again with
`-- focus` appended and retry that window: the request then goes to the
focused child window instead of the foreground window.

## Questions and answers

- Does swallowing Caps Lock in the hook leave the Caps Lock state unchanged?
  Yes.
- Which windows report their layout correctly through the foreground thread,
  and which need the focused child window's thread? UWP apps need the
  focused child; the legacy console needs the thread of its IME window.
- Which window must receive `WM_INPUTLANGCHANGEREQUEST` in UWP apps? The
  foreground window.
- Does `HSHELL_LANGUAGE` arrive for manual switches? No, never.
- Is the polling timer needed at all? Yes, it is the only way to see manual
  switches.
