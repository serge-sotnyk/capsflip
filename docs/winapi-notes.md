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

## Probe session still to run

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
