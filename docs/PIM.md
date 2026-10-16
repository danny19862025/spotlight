# Contacts and calendar search

Type `contacts name`, `contacts email`, or `calendar phrase` in Spotlight. Press Enter to read a contact or event inside Spotlight; Copy All copies the details and Open in App opens its source. No messages, invitations, or edits are sent.

For accounts synchronized by Akonadi, Spotlight uses the small native `spotlight-pim` bridge in `native/pim.cpp`. It reads through Akonadi's public collection/item API, including KAddressBook and KOrganizer resources. It never reads the Akonadi database directly, starts an Akonadi service, or requests remote synchronization. Open your PIM application to configure and synchronize accounts before searching.

The optional bridge requires a C++17 compiler, pkg-config, Qt 6 Core, KDE CoreAddons, and Akonadi 6 development headers/libraries. On Arch these headers ship with `akonadi`; other distributions may split development packages (for example `libkpim6akonadi-dev` and `qt6-base-dev`). Use your distribution's package manager. This does not install or configure KAddressBook, KOrganizer, or a mail/database server automatically.

Once those dependencies are installed:

```sh
sh native/build-pim.sh
install -m 755 target/native/spotlight-pim "$HOME/.local/lib/spotlight/spotlight-pim"
```

`install.sh` builds and installs the bridge automatically when the Akonadi 6 headers are present. Otherwise the main launcher and its other tools still install normally.

Without the bridge, Spotlight reads local `.vcf` and `.ics` resources under the XDG `contacts`, `kaddressbook`, `calendar`, and `korganizer` data directories, plus file/directory paths configured in Akonadi's local vCard/iCalendar resource configuration files. Remote DAV URLs are handled through the native bridge, not downloaded by Spotlight. You can also place exported vCards in `$XDG_DATA_HOME/spotlight/contacts` and calendars in `$XDG_DATA_HOME/spotlight/calendars` (normally `~/.local/share/spotlight/...`). A “Local resources only” row explains when the bridge is unavailable.

Search is bounded to 100 native collections and 2,000 cached native items (up to 500 per collection), or 2,000 local resource files, four directory levels, 2 MiB per file, 16 MiB total, and three seconds of local parsing. Native output is capped at 900 KiB. Individual text fields are limited to 4,096 characters. Results are cached in memory for 30 seconds; Ctrl+R refreshes them. Events are searchable by their stored title, start date, location, and description. Recurrence rules are displayed, but recurring instances are not expanded into an agenda. Modern UTF-8 vCard/iCalendar records and folded lines are supported; legacy vCard encodings and binary attachments are not indexed.
