# capsflip

Mac keyboard habits for Windows, written in Rust.

- **Caps Lock switches the keyboard layout**, as on a Mac. A short press
  switches between English and the last non-English layout you used,
  whether you reached it with capsflip, Ctrl+Shift, Win+Space or the mouse.
  Holding Caps Lock for half a second turns capitals on or off, as does
  Shift+Caps Lock. Phantom layouts that Windows adds on its own, such as
  English (United Kingdom), are never switched to.
- **Win+C, Win+V and Win+X copy, paste and cut**, as Cmd+C/V/X on a Mac.
  Off until you switch it on in the config, because it replaces what
  Windows does with these keys.

Other ideas wait in the
[issues](https://github.com/serge-sotnyk/capsflip/issues).

## Install

1. Download `capsflip-x64.exe` (or `capsflip-arm64.exe` for ARM) from the
   [latest release](https://github.com/serge-sotnyk/capsflip/releases/latest)
   and put it in a folder of your own, for example
   `%LOCALAPPDATA%\Programs\capsflip`.
2. Run it. The executable is not signed, so Windows SmartScreen may warn
   that it is from an unknown publisher: choose *More info*, then *Run
   anyway*. `SHA256SUMS.txt` in the release lets you check the download.
3. A blue icon appears in the notification area. Its menu has *Start at
   login*.

capsflip is portable: it installs nothing. To remove it, clear *Start at
login*, quit it, and delete the executable, `%APPDATA%\capsflip` and
`%LOCALAPPDATA%\capsflip`.

## Use

| Press | Does |
|---|---|
| Caps Lock | switches between English and the last non-English layout |
| Caps Lock, held for half a second | turns capitals on or off |
| Shift+Caps Lock | turns capitals on or off |

With `win_shortcuts = true` in the config:

| Press | Does | In a terminal |
|---|---|---|
| Win+C | copies (Ctrl+C) | copies (Ctrl+Insert), never interrupts a program |
| Win+V | pastes (Ctrl+V) | pastes (Shift+Insert) |
| Win+X | cuts (Ctrl+X) | nothing |
| Win+Shift+V | opens the clipboard history, which Win+V used to open | the same |

Copilot no longer opens on Win+C. The quick link menu that Win+X used to
open is still there with a right click on Start.

The tray menu has *Pause*, *Start at login*, *Open config*, *Reload config*
and *Quit*. While paused the icon is grey and Caps Lock is plain Caps Lock.

## Configure

The config file is `%APPDATA%\capsflip\config.toml`. It is written on first
start with every setting commented out; remove the `#` in front of a setting
to change it, then choose *Reload config*.

```toml
[layout]
# The English layout that Caps Lock switches to.
english = "en-US"
# The layout to pair with English before any other one was used.
default_secondary = "uk-UA"

[caps_lock]
# How long Caps Lock must be held to toggle capitals, in milliseconds.
hold_threshold_ms = 500

[exclusions]
# Programs in which capsflip leaves every key alone.
apps = ["mstsc.exe", "vmware.exe"]

[clipboard]
# Win+C, Win+V and Win+X copy, paste and cut.
win_shortcuts = true
# Terminals: copy and paste go as Ctrl+Insert and Shift+Insert there.
terminals = ["WindowsTerminal.exe", "conhost.exe"]
```

Programs are named by their executable file. A legacy console window is
`conhost.exe`, whatever runs in it.

A config with a mistake is never applied: on reload the previous settings
stay, at start the defaults are used, and a notification says what is
wrong. The log is `%LOCALAPPDATA%\capsflip\capsflip.log`.

## Limits

- Windows 11 only. The ARM64 build has not been tried on ARM hardware.
- Windows run as administrator are out of reach: there Caps Lock is plain
  Caps Lock and Win+C/V/X do what Windows does. capsflip runs unelevated on
  purpose.
- The terminal inside VS Code cannot be told apart from the editor, so
  Win+C there sends Ctrl+C: it copies a selection, and without one it
  interrupts the running program.
- capsflip watches the keyboard with a low-level hook and injects key
  presses. Anti-cheat software in games and some antivirus products may
  object to that; pause capsflip or add the game to `exclusions`.

## Build

```
cargo build --release
```

The Rust version is pinned in `rust-toolchain.toml`. Contributors: read
[AGENTS.md](AGENTS.md) and [docs/design.md](docs/design.md) first;
`pwsh scripts/check.ps1` runs every quality gate.

## License

[MIT](LICENSE)
