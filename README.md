# Spotlight

A native Linux launcher written in Rust and GTK 4, with a dark Raycast-style interface. Designed for KDE Plasma on Wayland. No browser, Electron, Python runtime, account, or background clipboard recorder.

## Build and install

Requires Rust, GTK 4.10+ development libraries, and pkg-config. Run `cargo fetch`, then `sh install.sh`. Launch `~/.local/bin/spotlight` or find **Spotlight** in the applications menu. On KDE, run `~/.local/bin/spotlight --install-shortcut` to register **Meta+Space**. Existing shortcut conflicts are preserved.

The first launch stays running; launching it again toggles the panel. Escape hides it. Use **Quit Spotlight** to exit. No automatic login startup is installed.

## Features

- Fuzzy application search with desktop icons, favorites, and usage ranking.
- File search directly in the main search bar by filename or path, with full paths shown under results. Indexes in the background when you start typing; the Search Files command also remains available. Bounded to 25,000 entries, six directory levels, excluding hidden files, symlinks, and build output. Names are searched, not file contents. Open files with their default application.
- Calculator: arithmetic, parentheses, powers, percentages, `pi`, `e`, `sqrt`, trigonometry (radians), `ln`, `log`, `abs`, and `round`.
- Unit conversion: `10 km to mi`, `32 f to c`, `2 kg to lb`, `1 gib to mib`. Supports common unit names, volume, area, speed, and temperature symbols. Natural-language date calculations are not implemented.
- Currency conversion in the main search: `100 USD to AUD` or `50 eur in gbp`. Fetches the latest daily rates from [Frankfurter](https://frankfurter.dev/), shows the rate date, and copies with Enter. Requires internet and `curl`; rates are cached in memory for 15 minutes. These are reference rates, not tick-by-tick prices.
- Clipboard history through existing KDE Klipper. Text only, newest 60 entries, maximum 64 KiB each. Enter copies; paste with Ctrl+V in your target app.
- Quick note capture from any app: press Meta+Space, type `note your text`, and press Enter. Search `Quick Note` for a multiline box; Ctrl+Enter saves, Escape keeps the draft in memory until Spotlight quits. “Save & Open” opens the saved note. Spotlight automatically connects when exactly one existing Obsidian vault is detected. Markdown notes save inside `Spotlight Quick Notes` in that vault, appear in main search, and open in Obsidian. Manage the connection or choose local-only capture in Settings. Vault notes are written directly as files, so Obsidian does not need to be running. `spotlight --quick-note` opens the capture box directly.
- Snippets, quicklinks, and editable notes stored locally. Snippets copy on demand; system-wide automatic text expansion is not implemented.
- A compact everyday emoji selection (93 emoji plus five text symbols), with favorites and your most-used emoji first when browsing. Eggplant and the rest of the full archive are excluded from search. Search directly in the main launcher using names such as `smile`, categories, or pasted emoji; Enter copies the complete sequence. “Show more emoji” browses the remaining entries.
- Installed font search and preview: type a font family name, `font JetBrains Mono`, or `fonts` to browse. Enter opens an editable text preview. “Use in Spotlight” applies the family to the launcher and remembers it across restarts; “Copy Font Name” copies it for other apps. “Reset Spotlight Font” or Settings → Reset Font restores the default. Font sizes and system-wide font settings stay as configured.
- Web search uses Google by default. Type a query in the main search bar and press Ctrl+Enter, or select “Search Google” and press Enter. Results open in your default browser. Full HTTP/HTTPS URLs open directly.
- Script commands: place executable files with a valid shebang in `~/.local/share/spotlight/scripts/`. They run with your normal user privileges and display stdout (limited to 64 KiB). stderr goes to the launch terminal. Run only scripts you trust.
- Window Manager icon beside Settings: tile into halves or quarters, maximize, restore, minimize, and move to the next monitor. Actions use KDE KWin and apply to the window focused before Spotlight opened. Search “Window Manager” to open it from the keyboard.
- Color Picker: search `color picker`, `colour`, or `eyedropper`, press Enter, and click a screen pixel. Spotlight hides during selection, copies `#RRGGBB`, and opens Color Formats. Choose HEX, HEX with alpha, RGB, RGBA, HSL, HSLA, or HSV/HSB coordinates and press Enter to copy. Screen colors use opaque alpha. Search `Color Formats` to reopen the choices, or search `rgb` / `hsl` directly after picking. Escape cancels. Uses the desktop Screenshot portal and works on KDE Wayland.
- Timers and alarms in the main search: `timer 5m`, `set timer for 30 seconds`, `timer 1h30m`, `alarm 7:30 am`, `alarm 18:45`, or `alarm in 5 minutes`. Type `alarm` and press Enter for a time picker with a Test Sound button. Type `timer` or `set timer` and press Enter for an hours/minutes/seconds picker, then choose Start Timer. Press Enter to set a typed duration. Alarms use the next occurrence in your local time. Search `cancel timer` or `cancel alarm`, select the active reminder, and press Enter to cancel. Saved reminders survive restarts and fire once if overdue when Spotlight next runs. Fired reminders open an alert and repeat their sound every five seconds until dismissed; unacknowledged alerts are saved across restarts. Desktop notifications and the system alarm sound work while the panel is hidden; Spotlight must remain running. This does not wake a suspended or powered-off computer.
- Lock screen, screenshot, and KDE settings commands.

| Key | Action |
| --- | --- |
| Meta+Space | Toggle launcher (KDE shortcut) |
| Up / Down | Select result |
| Enter | Open, copy, or run |
| Ctrl+Enter | Search Google from the main search bar |
| Escape | Back to search, then hide |
| Ctrl+K | Actions menu |
| Ctrl+F | Toggle favorite |
| Ctrl+N | Create snippet, note, or quicklink |
| Ctrl+E | Edit selected item |
| Ctrl+D | Delete selected item |
| Ctrl+, | Settings |

## Data and resource use

Data uses `$XDG_DATA_HOME/spotlight` or `~/.local/share/spotlight`, with private directory and file permissions. New installations start with empty notes, snippets, quicklinks, favorites, and usage history. No personal data is bundled or migrated from earlier app names. Runtime data, local scripts, private configuration, and generated desktop screenshots are excluded from Git. File indexing is lazy and runs in a worker thread; hidden panels do not continually poll for files. The reminder clock runs once per second only while timers or alarms are active. The installed launcher uses GTK's Cairo renderer to avoid allocating a GPU context. Clipboard history stays in Klipper. Currency queries send only the source and target currency codes to Frankfurter over HTTPS, after a short typing delay. Amounts stay local. Other calculations and unit conversions work offline; opening links or web search delegates to your browser.

This is an independent implementation of the launcher workflow, not a Raycast port. Raycast's extension API/store, AI services, cloud sync, and full feature parity are not implemented.

Emoji data is bundled from [Unicode Emoji 17.0](https://unicode.org/Public/17.0.0/emoji/emoji-test.txt) under the [Unicode License V3](assets/UNICODE-LICENSE.txt). The everyday selection uses this bundled metadata and needs no downloads at runtime.

## Verification

`cargo test --offline` tests expression parsing, rejected expressions, conversions, Unicode fuzzy matching, and URL encoding. `spotlight --smoke-test` exercises the real GTK search UI, file indexing and search, and Klipper integration, and writes `preview.png` in this project directory. Use a KDE desktop session for the latter. Add `--live-currency-test` to `--smoke-test` to verify a real exchange-rate request and its UI result. With an empty, temporary `XDG_DATA_HOME`, add `--reminder-smoke-test` to check a real two-second timer notification, hidden-window expiry, persistence, alarm setup, and cancellation. `spotlight --diagnostics` prints memory statistics in the running launcher's terminal; `spotlight --quit` stops it.

With an empty temporary `XDG_DATA_HOME`, `spotlight --smoke-test --notes-smoke-test` verifies note capture and the editor against a temporary vault.

## Uninstall

Remove `~/.local/bin/spotlight`, `~/.local/lib/spotlight`, `~/.local/share/applications/spotlight.desktop`, `~/.local/share/kglobalaccel/spotlight.desktop`, and `~/.local/share/icons/hicolor/scalable/apps/spotlight.svg`. Remove its shortcut in KDE System Settings → Keyboard → Shortcuts. Your data is retained under `~/.local/share/spotlight` until you remove it yourself.
