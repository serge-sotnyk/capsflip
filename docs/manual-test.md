# Manual test

The Win32 shell is verified by hand, because its tests cannot press keys or
click the tray icon. Run this checklist before tagging a release, on a
machine with en-US, Ukrainian and Russian layouts installed.

Start the build under test either way:

- `target\release\capsflip.exe` after `cargo build --release`: no console,
  as users run it;
- `cargo run -p capsflip`: a debug build that prints the log as it goes.

Every layout request and Caps Lock toggle is also written to
`%LOCALAPPDATA%\capsflip\capsflip.log`, with the window class and the
executable it went to. Look there first when something does not switch.

## Start

- [ ] The blue tray icon appears; its tooltip says `capsflip`.
- [ ] Starting a second copy shows "capsflip is already running" and exits.
- [ ] With a typo in `%APPDATA%\capsflip\config.toml`, capsflip still
      starts, and a notification says the defaults are used and why.

## Switching layouts

In each window: tap Caps Lock, type a few letters, tap again, type again.
The letters must change language each time.

- [ ] Notepad
- [ ] a browser (Chrome or Edge)
- [ ] VS Code
- [ ] Windows Terminal
- [ ] the legacy console: Win+R, `conhost.exe`
- [ ] Settings, in the search box (a UWP app)
- [ ] the Start menu search

## The pair of English

- [ ] Switch to Russian with Ctrl+Shift, then tap twice: Russian again.
- [ ] Switch to another window that is in English and tap: Russian.
- [ ] Switch to Ukrainian by hand and tap twice: Ukrainian again.
- [ ] With en-GB installed as well, no tap ever turns it on.

## Capitals

- [ ] Holding Caps Lock turns capitals on after half a second, while the key
      is still down. Holding it again turns them off.
- [ ] Shift+Caps Lock turns capitals on and off at once.
- [ ] A short tap does not make the Caps Lock indicator flicker.

## Tray menu

- [ ] A left click, a right click, and Enter on the icon (reach it with
      Win+B and the arrow keys) open the menu.
- [ ] Pause: the icon turns grey, the tooltip says paused, Caps Lock behaves
      as plain Caps Lock. Pause again: blue, switching works.
- [ ] Open config opens the config file.
- [ ] Set `hold_threshold_ms = 2000`, then Reload config: a notification
      confirms it, and a hold now takes two seconds.
- [ ] Make a typo, then Reload config: a warning says why, and the previous
      settings stay in effect.
- [ ] Set `apps = ["notepad.exe"]` under `[exclusions]` and reload: Caps Lock
      is plain Caps Lock in Notepad and switches layouts elsewhere.
- [ ] Start at login gets a check mark. After signing out and in again,
      capsflip is running. Clearing the check removes the value
      `capsflip` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- [ ] Quit: the icon disappears and the process ends.

## Staying alive

- [ ] Restart Explorer (`taskkill /f /im explorer.exe`, then
      `start explorer`): the icon comes back.
- [ ] Lock the screen with Win+L and unlock: a tap still switches.
- [ ] Put the machine to sleep and wake it: a tap still switches.

## Known limits

- [ ] In a window run as administrator, Caps Lock behaves as plain Caps
      Lock. This is by design: capsflip runs unelevated.
