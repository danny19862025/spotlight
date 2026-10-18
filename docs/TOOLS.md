# Expanded tools and desktop actions

## Local AI, availability, and search preferences

`ai summarize clipboard` prepares a local-only action. Enter reads text from the current clipboard (up to 64 KiB), verifies the local Ollama server and model, then generates a summary in a native window with Copy summary. Closing the window cancels the request. `ai models` lists installed local models; Settings → General → Search commands has an optional model name. Blank selects the first installed local model. Spotlight does not install, pull, or start models, save prompts/responses, or use a cloud fallback.

Install Ollama and a local text model yourself, then start the server with **`OLLAMA_NO_CLOUD=1`**. Spotlight requires `/api/status` to report `cloud.disabled = true`, rejects remote model metadata, and verifies local model weights before sending clipboard text. The client uses only `127.0.0.1:11434`, ignores proxy settings and `OLLAMA_HOST`, and refuses redirects. `SPOTLIGHT_OLLAMA_PORT` selects another numeric loopback port. Older servers without verifiable local-only status are refused. See [Ollama's local-only configuration](https://docs.ollama.com/faq#how-do-i-disable-ollamas-cloud-features).

`meet in 2h` finds a 30-minute slot at least two hours from now. `meet in 30m for 45m` selects another offset and duration. Compact `m`, `h`, and `d` durations are supported, with offsets up to seven days and meetings of 5 minutes to 8 hours. The initial search time rounds up to the next five minutes; availability covers all hours of the next seven days. Enter previews start/end times for copying and does not create an event.

Meeting availability reads **cached Akonadi calendar data**, not attendee availability or unsynchronized remote changes. Open KOrganizer to synchronize first. The optional public-API `spotlight-pim` bridge and `spotlight-freebusy` libical helper are required. Recurrence, EXDATE/EXRULE exclusions, moved/cancelled instances, transparency, floating/zoned times, and all-day dates are handled by libical and the bridge. Unsupported recurrence ranges, RDATE periods, unresolved timezones, malformed records, or data limits produce an error rather than a guessed free slot. See [calendar setup and bounds](PIM.md).

In **Settings → General → Search commands**, aliases use `name = target`, one per line:

```text
ff = firefox
g = web {query}
prs = gh pr list {query}
```

Aliases expand complete first words, retain the text in the search box, and resolve before command dispatch. `{query}` inserts the following text; without it, following text is appended. Targets are Spotlight searches, never shell snippets. Names are case-insensitive, with at most 64 aliases, eight expansion steps, and no cycles. The default `g` alias uses web search. Choose Google, DuckDuckGo, Bing, Brave, or Disabled as the fallback engine; the web result and Ctrl+Enter use that choice. Apply search settings validates and saves the draft.

Category checkboxes in the same panel remove unused tools from root search and gate typed lookups before backend dispatch. Docker/Podman commands, for example, do not start a CLI lookup while disabled. Disabled Fonts/Emoji skip constructing their main-search results. Changes clear cached command results and cancel active content/extension searches. App search, Settings, and the Guide remain available, and saved data is kept. These settings customize root search; a view opened explicitly from the Guide remains usable.

## Extension Store

`store` browses the local TOML catalog; `store text` filters it. Enter opens details showing version, author, source, artifact, sandbox capabilities, and SHA-256. **Install plugin** is the reviewed installation action. The initial catalog includes the bundled Text Tools SDK example, available offline. There is no invented list of third-party plugins or registry service.

The source catalog is [`community-plugins.toml`](community-plugins.toml), bundled into the app and also installed beside the binary. To customize it without rebuilding, copy it to `$XDG_DATA_HOME/spotlight/community-plugins.toml` (normally `~/.local/share/spotlight/community-plugins.toml`) and press Ctrl+R after editing. Add compatible community modules with an immutable `https://github.com/OWNER/REPO/releases/download/TAG/ASSET.wasm` artifact URL, a GitHub source URL, and the exact module SHA-256. The catalog supports at most 100 entries and 256 KiB; Python 3.11+ parses TOML locally.

No request is made when browsing the catalog. Release downloads occur after Install, use HTTPS GitHub release/asset hosts, and are capped at 4 MiB. Spotlight validates the digest, WASM structure, ABI exports, and imports before publishing a private plugin directory atomically. Modules may import only Spotlight's three WASM host functions. Native scripts/installers and archives are not accepted. Existing plugin IDs are never overwritten; this first Store does not perform upgrades or removals. Installed plugins appear in Script Commands and `plugin <id> <query>` and run inside the existing Wasmi capability boundary. See [WASM ABI and limits](WASM.md).

`python3 tests/productivity_smoke.py` checks local-only status, cloud/remote/redirect refusal before prompts, proxy bypass, native AI/settings/store flows, aliases and disabled dispatch, synthetic Akonadi cache handling, real libical recurrence/timezones, checksum failures, duplicate preservation, and execution of an installed example in the real WASM sandbox. AI and Akonadi use private fixtures; no model, account, or remote calendar is needed.

## Developer workflow

| Command | Behavior |
| --- | --- |
| `code myproject` | Reopen a recent local VS Code folder or `.code-workspace` |
| `jetbrains myproject`, `pycharm myproject` | Reopen a recent JetBrains project in its original IDE |
| `projects` | Browse recent projects from both editor families |
| `project ~/Projects/myproject` | Choose a folder as context for GitHub and dotenv commands |
| `ssh myserver` | Find a configured SSH alias and open Konsole running SSH |
| `env PATH`, `environment` | Search Spotlight's process environment by variable name; Enter inspects a value |
| `dotenv API` | Inspect matching variables from the current project's `.env` |
| `dotenv "/home/me/My Project/.env.local" TOKEN` | Inspect an explicit dotenv file with an optional name filter |
| `gh pr list review:required` | Search open pull requests in the current project |
| `gh issue bug`, `gh issue list --search "label:bug"` | Search open issues in the current project |
| `gh pr list --repo owner/repository` | Override context with a GitHub repository |
| `gh issue --repo "/home/me/My Project"` | Override context with a local Git repository |

Recent-project discovery uses **Python 3's standard library**. It reads `User/globalStorage/storage.json` and the relevant history keys in `state.vscdb` using a read-only SQLite connection for Code, Code Insiders, VSCodium, and Code OSS under `$XDG_CONFIG_HOME` (normally `~/.config`). JetBrains discovery reads `JetBrains/*/options/recentProjects.xml`, `recentSolutions.xml`, and legacy IDE configuration folders. Missing projects and remote workspace URIs are skipped. Opening a project uses its editor's command-line launcher, with an installed desktop entry as a fallback. See [VS Code's CLI documentation](https://code.visualstudio.com/docs/configure/command-line).

Opening an editor project updates the saved context. `project` displays that context and lets you choose another recent project; an absolute folder path also works without editor history. `SPOTLIGHT_PROJECT_DIR` in Spotlight's launch environment overrides saved context. The fallback is Spotlight's working directory. GitHub commands verify Git repository context and show the repository or folder with results. They require an installed, authenticated **GitHub CLI** and internet access. Listing uses structured JSON with a 40-result limit, and Enter opens the selected URL in the browser. Search text is passed as one argument to `--search`. See [gh pr list](https://cli.github.com/manual/gh_pr_list) and [gh issue list](https://cli.github.com/manual/gh_issue_list).

SSH search reads `~/.ssh/config` and bounded recursive `Include` files, supporting quoted paths and `*`/`?` globs. Only literal named Host aliases appear; wildcard and negated patterns are omitted. Discovery does not run config commands. Selection validates the alias again and runs `konsole --separate -e ssh -- ALIAS` with separate arguments. Konsole and OpenSSH must be installed. SSH handles authentication, host-key review, and the rest of your configuration in the terminal.

The environment inspector shows **variable names** in results and reads values on selection. Values are not placed in Spotlight's saved usage data or command cache. The environment belongs to the launcher process; another terminal's later `export` does not alter it. Dotenv inspection reads the file locally without sourcing or executing it, with a 1 MiB limit. It supports optional `export`, comments, empty values, quoted/multiline values, and last-key-wins duplicates. `${VARIABLE}` and `$(command)` remain literal. Parse errors identify the line without echoing its value. Dotenv inspection does not modify the file or process environment.

### Snippet placeholders

Snippet values accept `{cursor}` and `{clipboard}`. For example, `Hello {clipboard}, {cursor}!` inserts the current clipboard text and places the caret before `!` when the trigger expands on Space or Tab. The first cursor marker sets the position; further cursor markers are removed. Clipboard text is inserted literally, so placeholders inside it are not expanded again. Clipboard text is read at expansion time and limited to 1 MiB; edits made while that asynchronous read is pending cancel the insertion.

Selecting a snippet with Enter copies resolved text, with cursor markers removed. Trigger expansion controls the caret in Spotlight; a copied plain string carries no caret position. Desktop expansion exports Espanso's [cursor hints](https://espanso.org/docs/matches/basics/#cursor-hints) and [clipboard extension](https://espanso.org/docs/matches/extensions/#clipboard-extension), while preserving literal code braces through echo variables. Clipboard placeholders use text; an empty or image-only clipboard supplies empty text.

`python3 tests/devtools_smoke.py` checks real SQLite and XML editor fixtures, SSH Includes and stale aliases, exact editor/SSH/GitHub arguments, dotenv parsing, repository context, private inspector results, and native GTK snippet caret/clipboard behavior. External editor, SSH, and GitHub programs use fixtures; no connection or GitHub request is made.

## File and system tools

Select a file in **Search File** or **Recent**, then press **Ctrl+K** for Reveal in Dolphin, Copy Path, Move to Trash, Copy File, Open With, and Get Info. Trash requires confirmation. Copy File supplies the standard file-manager clipboard formats; paste it into Dolphin to copy the file. Get Info includes MIME type, size in bytes, and modification time. Directory size describes the directory entry rather than recursively adding its contents.

| Command | Behavior |
| --- | --- |
| `ip`, `my ip` | Local addresses and a public IP lookup through ipify |
| `ip local` | Local addresses only |
| `battery`, `disk`, `ram`, `uptime` | Copyable system snapshots; Ctrl+R refreshes |
| `qr hello`, `qr https://example.com` | Offline UTF-8 QR generation; Enter previews; Copy Image copies it |
| `generate qr` | Enter opens a text/URL input and Generate previews the QR |
| `ocr` | Choose clipboard-image or latest-screenshot recognition |
| `ocr latest`, `ocr /path/to/image.png` | Extract text and show a copyable preview |
| `lorem 5` | Generate five placeholder paragraphs, up to twenty |
| `case camel Hello world` | `helloWorld`; also snake, kebab, pascal, upper, lower, title |
| `json format {"ready":true}` | Validate JSON and open a formatted preview |
| `clip text`, `clip images`, `clip pinned` | Open filtered clipboard history |
| `speakers`, `speakers HDMI`, `audio outputs` | Select an audio output |
| `close window Firefox` | Request a normal close through KWin |
| `quit app Firefox` | Confirm closing all windows sharing the selected application process |
| `kill window Firefox` | Confirm force-quitting its process; unsaved work can be lost |
| `tabs chrome project` | Find titles and URLs in a connected local browser |
| `apt search firefox`, `pacman Ss firefox`, `flatpak search firefox` | Search and review a package installation |
| `packages firefox` | Select the available package manager automatically |
| `service status`, `service restart demo` | List/status or confirmed user service actions |
| `systemctl restart docker` | Review a system service restart |
| `systemctl --user stop demo` | Review stopping a user service |
| `bluetooth devices`, `bluetooth connect AirPods` | List known devices or select one to connect |
| `bluetooth disconnect Keyboard` | Select a known device to disconnect |

## Requirements

QR, case conversion, lorem generation, JSON formatting, RAM, uptime, disk, and battery discovery run locally. QR generation uses the bundled Rust `qrcodegen` dependency; no QR command-line utility is needed. IP discovery uses `ip`, and public IP uses `curl` and an internet connection.

OCR requires **Tesseract** and its installed language data. It uses the installation's default language. Inputs are limited to 16 MiB and 16 million pixels. Screenshot discovery checks `Pictures/Screenshots`, `Pictures/screenshots`, `~/Screenshots`, and screenshot/spectacle filenames in Pictures. An explicit path works for another folder. Clipboard OCR reads an existing image, and screenshot OCR reads an existing capture.

Audio outputs use `pactl` when available and move active streams to the selected sink. `wpctl` is the fallback for changing the default output for new streams. Devices are validated again before switching. Bluetooth device connections use `bluetoothctl`; pair devices in desktop Bluetooth settings before connecting them from Spotlight.

File Trash uses GIO's desktop Trash support, which may reject unsupported/internal mounts. Reveal prefers Dolphin and falls back to the desktop FileManager1 service. Open With uses the native GTK application chooser, compatible with GTK 4.10.

## Clipboard pins

In Clipboard History, **Ctrl+K** offers Pin entry, Unpin entry, filters, and Clear clipboard history. Text and image pins are saved under Spotlight's private data directory. Image pins are copies, so deleting the original Klipper history does not remove them. Unpinning an image removes Spotlight's copy. There is a fifty-entry limit; individual pinned text and images are limited to 64 KiB and 16 MiB respectively.

Clear history confirms before calling Klipper's supported clear-history and clear-contents methods. Spotlight's pins are retained. Spotlight does not edit Klipper's database directly.

## Browser tabs

Tab search uses Chrome DevTools' local HTTP discovery and activation endpoints. Configure a **loopback-only remote-debugging endpoint in a dedicated browser profile** using your browser's supported settings. Chrome's [remote-debugging changes](https://developer.chrome.com/blog/remote-debugging-port) describe the separate-profile requirement. A normal browser window without an endpoint cannot expose its tabs through KWin alone.

The default endpoint is `http://127.0.0.1:9222`. Set `SPOTLIGHT_CHROME_DEBUG_PORT` in Spotlight's launch environment for another local port. Search `tabs chrome` or append a title/URL fragment. Only page targets are listed. Activation targets the existing tab rather than opening another copy. See the [DevTools protocol documentation](https://chromedevtools.github.io/devtools-protocol/).

## Packages and services

Package searches read existing package catalogs and configured Flatpak remotes. Selecting a result opens a confirmation that names the package, manager, remote where relevant, and installation scope. Required dependencies can also be installed. apt/pacman invoke `pkexec` and the desktop authorization agent. Flatpak prefers a configured user remote of the selected name and otherwise uses the system installation with its normal authorization. Database refreshes, upgrades of the whole system, and adding remotes remain separate administrator tasks.

`service` and `services` default to **user** scope. `systemctl` defaults to **system** scope, matching the CLI; `--user` and `--system` select it explicitly. Status is read-only. Start, restart, and stop ask for confirmation and accept service units only. System actions use desktop authorization; user actions run through `systemctl --user`. The list includes loaded service units, including inactive loaded units. See [systemctl](https://www.freedesktop.org/software/systemd/man/latest/systemctl.html).

Normal window closing uses the [KWin scripting API](https://develop.kde.org/docs/plasma/kwin/api/). Force quit verifies the selected window and its process identity, then uses a Linux pidfd to avoid signaling a reused PID. Only a process owned by the current user can be force-quit.

## Verification

`cargo test --offline` checks parsers, text vectors, identity validation, and compatibility with older saved data. `python3 tests/features_smoke.py --ui` runs the fixture-backed backend and real GTK checks in a private D-Bus session. It independently decodes a generated Unicode QR code when `zbarimg` is installed and runs real Tesseract OCR. Package installs, service actions, audio switching, Bluetooth connections, and clipboard clearing use fixtures. Trash checks use disposable files and an isolated Trash directory.

On KDE, `python3 tests/window_controls_smoke.py` additionally creates three disposable GTK processes and verifies real KWin close, application quit, and force-quit behavior. It rejects a stale process identity before force-quitting the selected fixture. Existing applications are not targeted.
