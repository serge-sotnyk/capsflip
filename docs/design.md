# capsflip design

capsflip brings Mac keyboard habits to Windows. This document records the
decisions behind each version and the facts they rest on. Update it when a
decision changes; do not let it drift from the code.

## Scope

| Version | Content |
|---|---|
| v0.1 | Caps Lock switches the keyboard layout, Mac style |
| v0.2 | Optional Win+C / Win+V / Win+X as copy / paste / cut |
| backlog | Other Mac shortcuts and conveniences (see GitHub milestones) |

## Decisions for v0.1

| Topic | Decision |
|---|---|
| Short Caps Lock press | Toggles between the configured English layout and the last used non-English layout |
| Real Caps Lock | Both a long press (threshold in config, default 500 ms) and Shift+Caps Lock toggle it. macOS does not document its threshold; 500 ms is when Windows starts repeating a held key |
| Long press timing | Fires when the threshold elapses while the key is still held, not on release |
| Last non-English layout | Tracked in the background, including manual switches (Ctrl+Shift, mouse, Win+Space). One global value, not per window. Any non-English layout seen in the focused window counts, so a window brought to the front with its own layout (Windows keeps one per window) makes that layout the pair of English. Revisit after daily use: #24 |
| Value right after start | `default_secondary` from config, otherwise the first non-English layout installed |
| Phantom en-GB | Bypassed only: capsflip always activates one specific layout, and treats any English layout as "English" when reading the current one. It never removes layouts |
| Configured English missing | Warn, then fall back to any installed English layout |
| Settings | `%APPDATA%\capsflip\config.toml` is the single source of truth. Tray menu: pause, start at login, open config, reload config, quit. A left click, a right click and Enter on the icon all open the menu |
| Rejected config | At start, capsflip runs with the built-in defaults and a notification says why; on reload, it keeps the settings in effect and says why. A typo never leaves the keyboard without capsflip |
| Open config | In the program registered for `.toml`, or Notepad when there is none |
| Indication | Static tray icon with two states: active (blue) and paused (grey). Drawn in code from ASCII art, so it is sharp at every display scale |
| Staying out of the way | Pause from the tray menu, plus a list of excluded executables in config. Pause is not remembered: capsflip always starts active |
| Second instance | Shows a message that capsflip is already running and exits |
| Log | `%LOCALAPPDATA%\capsflip\capsflip.log`, moved to `capsflip.old.log` at start once it passes 1 MiB |
| Elevated windows | Not supported. capsflip runs unelevated, so the hook does not see keys there and Caps Lock behaves natively |
| Console windows | Supported, including the legacy console host |
| Install | Portable exe. Autostart through `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. A named mutex prevents a second instance |
| Platforms | Windows 11, x64 and ARM64. ARM64 is built but not tested on hardware |
| Releases | GitHub Actions: checks on push, binaries attached to a GitHub Release on a `v*` tag. No code signing |

## Decisions for v0.2

Win+C / Win+V / Win+X as copy / paste / cut (#13). Decided by the owner on
2026-11-15 (#14, #15), except where marked as capsflip's default.

| Topic | Decision |
|---|---|
| Switching it on | Off by default; `[clipboard] win_shortcuts = true` in config, then Reload config. No tray item: the config stays the single source of truth (capsflip's default) |
| Keys sent | Ctrl+C / Ctrl+V / Ctrl+X |
| Terminals | In applications on the `terminals` list in config, copy and paste go as Ctrl+Insert / Shift+Insert, so Win+C never interrupts a program. Cut does nothing there, as Cmd+X in the Mac Terminal: Ctrl+X is "exit" in nano and a prefix in bash (capsflip's default) |
| Default terminal list | `WindowsTerminal.exe` and `conhost.exe`. A legacy console is named after `conhost.exe` whatever runs in it. The terminal inside VS Code cannot be told apart from the editor and gets Ctrl+C (capsflip's default) |
| Clipboard history | Moves to Win+Shift+V: capsflip sends the system the original Win+V |
| Quick link menu | No shortcut; it opens with a right click on Start. The owner chose Win+Shift+X, but a menu opened from injected keys does not get the keyboard focus and then does not close on Esc or a click elsewhere, even after `AllowSetForegroundWindow`; the owner agreed to drop it if it could not be fixed simply. Win+Shift+X is left alone |
| Holding the keys | One press, one action; holding Win+V does not repeat the paste |
| Pause, exclusions, elevated windows | As for Caps Lock: paused or in an excluded application the keys behave natively; elevated windows are out of reach |
| Win+Shift+C, other Mac shortcuts | Untouched; other shortcuts wait in #16 |

How the keys are sent is settled by a spike at the keyboard (#25) and
recorded in `docs/winapi-notes.md`. The hypothesis: while Win is still held,
inject a key that does nothing (the mask), release Win, send the combination,
press Win again, and inject the mask once more, so that neither the injected
release nor the physical one opens Start.

## Glossary

- **Layout**: one installed keyboard layout, identified by its `HKL`. The low
  word of an `HKL` is the language identifier.
- **English layout**: a layout whose primary language is English (`0x09`).
  en-US and en-GB are both English layouts.
- **Target English**: the one English layout named in config. It is the only
  English layout capsflip ever activates.
- **Secondary layout**: the last non-English layout that was active. This is
  what a tap switches to from English.
- **Phantom layout**: a layout Windows adds on its own, typically en-GB.
- **Tap**: Caps Lock pressed and released before the hold threshold, with no
  Shift held.
- **Hold**: Caps Lock kept down until the hold threshold elapses.
- **Rule**: one row of the table that maps a trigger (tap, hold, chord) to an
  action.
- **Action**: what the shell executes for a rule, for example toggle the
  layout or toggle Caps Lock.
- **Observation**: a report to the layout tracker that a given layout is now
  active, whatever caused it.
- **Excluded app**: an executable listed in config. While its window is in the
  foreground, no rule fires and keys pass through untouched.

## Architecture

A Cargo workspace with two crates. The boundary between them is the main
design decision: all logic lives in a pure crate, and Win32 lives in a thin
shell.

### `crates/capsflip-core`

No Win32, `#![forbid(unsafe_code)]`, written test-first.

- `key`: `Key`, `KeyEvent { key, kind, time_ms, injected }`, modifiers.
- `rules`: the rule table, `Trigger -> Action`. Triggers are `Tap(key)`,
  `Hold(key)` and `Chord(mods, key)`. v0.1 ships three rules:
  `Tap(Caps) -> ToggleLayout`, `Hold(Caps) -> ToggleCapsLock`,
  `Chord(Shift, Caps) -> ToggleCapsLock`. Later features add rows.
- `engine`: the state machine. `on_key(event)` returns whether to swallow the
  event and which actions to run; `on_timer(now)` fires holds. Injected events
  always pass through.
- `layout`: `LayoutId`, `LangId`, `LayoutTracker`. `observe(layout)` records
  what is active; `toggle(current, installed)` picks what to activate. All
  layout semantics live here.
- `config`: TOML parsed once into a validated `Config`, then resolved into
  `Settings` once the system has turned locale names into languages. Also
  reads the config file and writes the template when there is none.
- `exclusions`: matches an executable name against the excluded list.
- `controller`: owns the engine, the tracker and the settings, and reaches
  the system only through the `Platform` trait. It decides inside the hook
  (`on_key`), acts after the hook has returned (`run_pending`), handles the
  hold timer and learns the foreground layout from polling (`on_poll`).
- `logfile`: appends a line to the log file and moves a large one aside.
- `tray`: the menu commands and check marks, the icon and tooltip for each
  state, the text of the notifications.
- `icon`: the pixels of the tray icon at any size.
- `autostart`: the `Run` value for this executable and whether the one in
  the registry is it.

### `crates/capsflip`

The Windows binary. It implements `Platform` with Win32 calls and forwards
the hook, the timers and the wake message to the controller. It contains no
decisions of its own.

- `main`: the app in a thread local, the keyboard hook, a hidden top-level
  window, the hold and poll timers, the menu commands, start-up. The window
  is top-level rather than message-only because only top-level windows get
  the `TaskbarCreated` broadcast (Explorer restarted, add the icon again)
  and can bring the tray menu to the front.
- `platform`: `Platform` for Win32: read the layout of the focused window,
  list layouts, post `WM_INPUTLANGCHANGEREQUEST`, inject Caps Lock, name the
  foreground executable.
- `tray`: `Shell_NotifyIconW`, the icon from the core's pixels, the popup
  menu.
- `registry`: reads and writes the `Run` value.
- `instance`: the named mutex that keeps one capsflip per session.
- `locale`: locale name to language identifier.
- `paths`, `logging`: where the config and the log live; timestamped lines.

A release build has no console (`windows_subsystem = "windows"`); a debug
build keeps one, so `cargo run` shows the log as it is written.
- `examples/probe.rs`: a diagnostic that prints hook events, the current
  layout and the foreground window class. Used to verify Win32 behaviour.
- `examples/console.rs`: reads the layout of every legacy console window two
  ways and can ask them to switch, with no key pressed.
- `examples/winkeys.rs`: the spike for Win+C / V / X (#25): swallows the
  combinations, injects copy, paste and cut with the menu mask, and logs
  every event and how late the injection ran.
- `examples/shared/journal.rs`: the queued log both diagnostics write.

Its tests run against the real system instead of a mocked Win32: they start
and stop the app with a real hook, send messages to its real window, carry
out menu commands, build a real menu and icon, claim a real mutex, resolve
real locales and name the test process. They keep out of the user's way:
the `Run` value goes under a scratch registry key, the log goes to the
temporary directory, and the tray icon is shown only for an instant, never
with a notification. What needs a person at the keyboard is in
`docs/manual-test.md`.

## Win32 facts

Verified against documentation:

- A `WH_KEYBOARD_LL` callback that returns non-zero swallows the key before
  the Caps Lock toggle state changes.
- If the callback exceeds the hook timeout (at most 1000 ms), Windows removes
  the hook silently. The callback therefore only decides and posts a message;
  actions run after it returns.
- For a legacy console window, `GetWindowThreadProcessId` names the
  console's client (for example `cmd.exe`), not the `conhost.exe` thread
  that owns the window, so `GetKeyboardLayout` of that thread is useless.
- A layout is found by language identifier in `GetKeyboardLayoutList`, never
  by a hard-coded `HKL`: layout variants (for example Ukrainian Enhanced,
  `00020422`) have `HKL` values that differ between machines.

Measured with the probe (details in `docs/winapi-notes.md`):

- Windows does not announce manual layout switches: `HSHELL_LANGUAGE` never
  arrived. The shell therefore reads the foreground layout every 250 ms.
- `WM_INPUTLANGCHANGEREQUEST` posted to the foreground window switches the
  layout everywhere capsflip was tried: classic Win32, Chrome and Electron,
  Windows Terminal, the new Notepad, Settings (UWP), the Start menu search
  and the legacy console.
- In a legacy console the layout of the client's thread reads as zero. The
  default IME window of the console belongs to the real `conhost.exe`
  thread, and that thread's layout follows every switch. capsflip therefore
  reads the layout of the thread that owns the IME window of the focused
  window, for every window; for all others it is the window's own thread.
  No per-window memory is needed.

## Configuration

```toml
[layout]
english = "en-US"
default_secondary = "uk-UA"   # optional

[caps_lock]
hold_threshold_ms = 500

[exclusions]
apps = ["mstsc.exe", "vmware.exe"]
```

The template written on first start has every setting commented out, so the
defaults stay in the code and a better default in a later version reaches
users who never changed the setting.

An invalid config is never applied: capsflip keeps the previous one and
reports the error.

## Risks

- Windows may remove the hook silently. The callback stays minimal; if the
  manual checklist shows the hook dying after sleep, reinstall it on session
  unlock.
- Anti-cheat and antivirus software may object to a keyboard hook combined
  with injected input. An unsigned exe triggers SmartScreen. Both are
  documented in the README.
- The ARM64 build is not tested on hardware.
