# Contacts and calendars

## Create calendar events

Type `event Birthday on July 13 2027`, `event on July 13 2027`, or `add event Dinner on July 13 2027 at 6pm`. Press Enter to review the event. You can change the title, date, all-day setting, local start time, duration in minutes, location, and destination calendar. A title is required. Supported date forms include `July 13 2027`, `13 July 2027`, and `2027-07-13`; times accept 12-hour and 24-hour notation. A date without a time is an all-day event. **Add Event** or Ctrl+Enter saves; Cancel or Escape discards the form. No calendar write happens while typing or previewing.

Spotlight discovers the enabled GNOME Calendar/Evolution calendars through the public Evolution Data Server API and selects the configured default calendar where available. Choose a different destination in the form. Read-only calendars reject writes. Saving to an account calendar uses its normal backend; remote synchronization is managed by that account's calendar service. Spotlight does not add attendees or send invitations.

The bridge is bundled with Spotlight and uses `/usr/bin/python3`, PyGObject, and the `ECal-2.0` / `EDataServer-1.2` introspection libraries. If these are unavailable, the form offers **Spotlight (local only)**. Local events are private `.ics` files under `$XDG_DATA_HOME/spotlight/calendars` and are searchable in Spotlight; they are not automatically imported into GNOME Calendar or synchronized to an account. Choosing an account never silently falls back to local storage on a save error. If a save times out, check that calendar before retrying.

Timed events are interpreted in the system's local time zone and stored as UTC. All-day events use date values and an exclusive next-day end date. iCalendar text is escaped, UTF-8 lines are folded, and local files use unique names and mode 0600. Event contents are excluded from Spotlight's saved usage history.

## Search

Type `contacts name`, `contacts email`, or `calendar phrase` in Spotlight. Press Enter to read a contact or event inside Spotlight; Copy All copies the details and Open in App opens supported file/Akonadi sources. Search does not edit records. GNOME Calendar/Evolution searches use the same calendar service as event creation. Account setup remains in your calendar app.

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
