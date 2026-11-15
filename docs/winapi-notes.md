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

## Probe session still to run

The first session covered steps 1, 8, 9 and 10 below, in Notepad++. Steps 2
to 7 are still open, and so is step 8 in the legacy console.

This needs a person at the keyboard. Start the probe in a terminal:

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

## Questions the session answers

- Does swallowing Caps Lock in the hook leave the Caps Lock state unchanged?
- Which windows report their layout correctly through the foreground thread,
  and which need the focused child window's thread?
- Which window must receive `WM_INPUTLANGCHANGEREQUEST` in UWP apps?
- Does `HSHELL_LANGUAGE` arrive for manual switches, and what is in its
  `lParam`? Does it arrive for the legacy console?
- Is the polling timer needed at all, or do the two events cover everything?
