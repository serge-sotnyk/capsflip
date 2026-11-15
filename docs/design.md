# capsflip design

capsflip brings Mac keyboard habits to Windows. This document records the
decisions behind v0.1 and the facts they rest on. Update it when a decision
changes; do not let it drift from the code.

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
| Real Caps Lock | Both a long press (threshold in config, default 400 ms) and Shift+Caps Lock toggle it |
| Long press timing | Fires when the threshold elapses while the key is still held, not on release |
| Last non-English layout | Tracked in the background, including manual switches (Ctrl+Shift, mouse, Win+Space). One global value, not per window |
| Value right after start | `default_secondary` from config, otherwise the first non-English layout installed |
| Phantom en-GB | Bypassed only: capsflip always activates one specific layout, and treats any English layout as "English" when reading the current one. It never removes layouts |
| Configured English missing | Warn, then fall back to any installed English layout |
| Settings | `%APPDATA%\capsflip\config.toml` is the single source of truth. Tray menu: pause, start at login, open config, reload config, quit |
| Indication | Static tray icon with two states: active and paused |
| Staying out of the way | Pause from the tray menu, plus a list of excluded executables in config |
| Elevated windows | Not supported. capsflip runs unelevated, so the hook does not see keys there and Caps Lock behaves natively |
| Console windows | Supported, including the legacy console host |
| Install | Portable exe. Autostart through `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. A named mutex prevents a second instance |
| Platforms | Windows 11, x64 and ARM64. ARM64 is built but not tested on hardware |
| Releases | GitHub Actions: checks on push, binaries attached to a GitHub Release on a `v*` tag. No code signing |

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
  what is active; `toggle_target(current, installed)` picks what to activate.
  All layout semantics live here.
- `config`: TOML parsed once into a validated `Config`.
- `exclusions`: matches an executable name against the excluded list.

### `crates/capsflip`

The Windows binary. It translates Win32 events into core types, and core
actions into Win32 calls. It contains no decisions of its own.

- `hook`: installs `WH_KEYBOARD_LL`; the callback asks the engine and posts
  the resulting actions to the message loop.
- `layouts`: lists layouts, reads the current one, activates one, and feeds
  observations to the tracker.
- `input`: `SendInput` for the real Caps Lock toggle.
- `foreground`: executable name and window class of the foreground window.
- `tray`, `autostart`, `single_instance`, `logging`, `main`.
- `examples/probe.rs`: a diagnostic that prints hook events, the current
  layout and the foreground window class. Used to verify Win32 behaviour.

## Win32 facts

Verified against documentation:

- A `WH_KEYBOARD_LL` callback that returns non-zero swallows the key before
  the Caps Lock toggle state changes.
- If the callback exceeds the hook timeout (at most 1000 ms), Windows removes
  the hook silently. The callback therefore only decides and posts a message;
  actions run after it returns.
- `GetKeyboardLayout` is unreliable for a legacy console window, because the
  thread reported for that window is not the thread that owns it. capsflip
  remembers the layout it last activated per console window instead.
- A layout is found by language identifier in `GetKeyboardLayoutList`, never
  by a hard-coded `HKL`: layout variants (for example Ukrainian Enhanced,
  `00020422`) have `HKL` values that differ between machines.

Still to verify with the probe (results go to `docs/winapi-notes.md`):

- which mechanism reports manual layout switches: `HSHELL_LANGUAGE`,
  `EVENT_SYSTEM_FOREGROUND`, or a timer as a fallback;
- which window must receive `WM_INPUTLANGCHANGEREQUEST` in UWP apps.

## Configuration

```toml
[layout]
english = "en-US"
default_secondary = "uk-UA"   # optional

[caps_lock]
hold_threshold_ms = 400

[exclusions]
apps = ["mstsc.exe", "vmware.exe"]
```

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
