mod color;
mod core;
mod currency;
mod emoji;
mod fonts;
mod quicknote;
mod reminder;
mod shortcut;
use core::{Store, TextItem};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Root,
    Files,
    Clipboard,
    Snippets,
    Links,
    Notes,
    Scripts,
    Emoji,
    Fonts,
    Colors,
    Windows,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Root => "Suggestions",
            Self::Files => "Files · home folder",
            Self::Clipboard => "Clipboard history · KDE Klipper",
            Self::Snippets => "Snippets",
            Self::Links => "Quicklinks",
            Self::Notes => "Notes",
            Self::Scripts => "Script commands",
            Self::Emoji => "Everyday emoji & symbols",
            Self::Fonts => "Installed Fonts",
            Self::Colors => "Color Formats",
            Self::Windows => "Window Manager · active window",
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Self::Root => "Search apps, files, emoji, or the web…",
            Self::Files => "Search your files…",
            Self::Clipboard => "Search clipboard history…",
            Self::Snippets => "Search snippets…",
            Self::Links => "Search quicklinks…",
            Self::Notes => "Search notes…",
            Self::Scripts => "Search script commands…",
            Self::Emoji => "Search everyday emoji…",
            Self::Fonts => "Search installed fonts…",
            Self::Colors => "Search HEX, RGB, HSL, HSV…",
            Self::Windows => "Search window actions…",
        }
    }
}
#[derive(Clone)]
enum Action {
    App(gio::AppInfo),
    Page(Page),
    Open(String),
    Copy(String),
    Edit(Page, usize),
    Script(PathBuf),
    System(Vec<String>),
    Window(&'static str),
    New(Page),
    Settings,
    Quit,
    MoreResults,
    FontPreview(String),
    ResetFont,
    Noop,
    PickColor,
    AlarmSetup,
    TimerSetup,
    DismissAlarms,
    QuickNote(String),
    CaptureNote(String),
    Schedule(reminder::Draft),
    CancelReminder(String),
}
#[derive(Clone)]
struct Item {
    id: String,
    title: String,
    subtitle: String,
    badge: String,
    icon: String,
    gicon: Option<gio::Icon>,
    keywords: String,
    action: Action,
}
fn item(id: &str, title: &str, subtitle: &str, icon: &str, badge: &str, action: Action) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        badge: badge.into(),
        icon: icon.into(),
        gicon: None,
        keywords: String::new(),
        action,
    }
}
struct Ui {
    window: gtk::ApplicationWindow,
    search: gtk::Entry,
    list: gtk::ListBox,
    section: gtk::Label,
    notice: gtk::Label,
    hint: gtk::Label,
    add: gtk::Button,
    back: gtk::Button,
    store: RefCell<Store>,
    can_save: bool,
    page: RefCell<Page>,
    apps: RefCell<Vec<Item>>,
    fonts: Vec<String>,
    font_css: gtk::CssProvider,
    visible: RefCell<Vec<Item>>,
    files: RefCell<Vec<PathBuf>>,
    file_loading: RefCell<bool>,
    files_ready: RefCell<bool>,
    clips: RefCell<Vec<String>>,
    result_limit: Cell<usize>,
    currency: RefCell<currency::State>,
    picker: RefCell<Option<(String, gio::SignalSubscription)>>,
    picking_color: Cell<bool>,
    picked_color: RefCell<Option<String>>,
    reminder_clock: RefCell<Option<glib::SourceId>>,
    note_editor: RefCell<Option<gtk::Window>>,
    note_draft: RefCell<String>,
    alarm_window: RefCell<Option<gtk::Window>>,
    alarm_repeat: RefCell<Option<glib::SourceId>>,
    alarm_label: RefCell<Option<gtk::Label>>,
}
impl Ui {
    fn error(&self, text: &str) {
        self.notice.set_text(text);
        self.notice.set_visible(!text.is_empty());
    }
    fn save(&self) {
        if !self.can_save {
            self.error("Saved data is unreadable; saving is disabled to preserve data.json.");
            return;
        }
        if let Err(e) = self.store.borrow().save() {
            self.error(&format!("Could not save: {e}"));
        }
    }
    fn selected(&self) -> Option<Item> {
        let index = self.list.selected_row()?.index() as usize;
        self.visible.borrow().get(index).cloned()
    }
    fn page(self: &Rc<Self>, page: Page) {
        *self.page.borrow_mut() = page;
        self.result_limit.set(60);
        self.error("");
        self.search.set_text("");
        self.search.set_placeholder_text(Some(page.placeholder()));
        self.back.set_visible(page != Page::Root);
        self.add
            .set_visible(matches!(page, Page::Snippets | Page::Links | Page::Notes));
        self.search.grab_focus();
        if page == Page::Files {
            self.load_files();
        }
        if page == Page::Clipboard {
            self.load_clipboard();
        }
        self.refresh();
    }
    fn load_files(self: &Rc<Self>) {
        if *self.files_ready.borrow() || *self.file_loading.borrow() {
            return;
        }
        *self.file_loading.borrow_mut() = true;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(core::index_files());
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(80), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(files) => {
                    *ui.files.borrow_mut() = files;
                    *ui.file_loading.borrow_mut() = false;
                    *ui.files_ready.borrow_mut() = true;
                    ui.refresh();
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => {
                    *ui.file_loading.borrow_mut() = false;
                    ui.error("File indexing failed");
                    glib::ControlFlow::Break
                }
            }
        });
    }
    fn file_items(&self, query: &str) -> Vec<Item> {
        let files = self.files.borrow();
        let mut matches: Vec<_> = files
            .iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy();
                let score = if query.contains('/') {
                    core::score(query, &path.to_string_lossy())
                } else {
                    core::score(query, &name)
                }?;
                Some((score, path))
            })
            .collect();
        matches.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        matches
            .into_iter()
            .take(60)
            .filter_map(|(_, path)| {
                let name = path.file_name()?.to_string_lossy();
                let full_path = path.to_string_lossy();
                Some(item(
                    &full_path,
                    &name,
                    &full_path,
                    if path.is_dir() {
                        "folder-symbolic"
                    } else {
                        "text-x-generic-symbolic"
                    },
                    "File",
                    Action::Open(gio::File::for_path(path).uri().to_string()),
                ))
            })
            .collect()
    }
    fn load_clipboard(self: &Rc<Self>) {
        self.clips.borrow_mut().clear();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> Result<Vec<String>, String> {
                let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
                    .map_err(|e| e.to_string())?;
                let call = |method: &str, args: Option<&glib::Variant>| {
                    bus.call_sync(
                        Some("org.kde.klipper"),
                        "/klipper",
                        "org.kde.klipper.klipper",
                        method,
                        args,
                        None,
                        gio::DBusCallFlags::NONE,
                        800,
                        None::<&gio::Cancellable>,
                    )
                    .map_err(|e| e.to_string())
                };
                let menu = call("getClipboardHistoryMenu", None)?
                    .get::<(Vec<String>,)>()
                    .ok_or("Unexpected clipboard response")?
                    .0;
                let mut texts = Vec::new();
                for i in 0..menu.len().min(60) {
                    let response =
                        call("getClipboardHistoryItem", Some(&(i as i32,).to_variant()))?;
                    if let Some((s,)) = response.get::<(String,)>()
                        && !s.is_empty()
                        && s.len() < 65536
                    {
                        texts.push(s);
                    }
                }
                Ok(texts)
            })();
            let _ = tx.send(result);
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(80), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(result) => {
                    match result{Ok(texts)=>*ui.clips.borrow_mut()=texts,Err(_)=>ui.error("Clipboard history requires KDE Klipper. Enable it in your system tray.")};
                    ui.refresh();
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });
    }
    fn color_items(&self) -> Vec<Item> {
        let picked = self.picked_color.borrow();
        let Some(hex) = picked.as_ref() else {
            return vec![item(
                "pick-color-first",
                "Pick a Color",
                "Choose a screen pixel to convert into different formats",
                "color-select-symbolic",
                "Pick",
                Action::PickColor,
            )];
        };
        color::formats(hex)
            .unwrap_or_default()
            .into_iter()
            .map(|format| {
                let subtitle = if format.name.contains("alpha")
                    || format.name == "RGBA"
                    || format.name == "HSLA"
                {
                    format!("{} · opaque alpha · Enter to copy", format.name)
                } else {
                    format!("{} · picked color · Enter to copy", format.name)
                };
                item(
                    &format!("color-format:{}", format.name),
                    &format.value,
                    &subtitle,
                    "●",
                    format.name,
                    Action::Copy(format.value.clone()),
                )
            })
            .collect()
    }
    fn font_items(&self) -> Vec<Item> {
        self.fonts
            .iter()
            .map(|family| {
                item(
                    &format!("font:{family}"),
                    family,
                    "The quick brown fox · Aa Bb 123 · Enter to preview",
                    "preferences-desktop-font-symbolic",
                    "Font",
                    Action::FontPreview(family.clone()),
                )
            })
            .collect()
    }
    fn set_font(self: &Rc<Self>, family: Option<&str>) -> Result<(), String> {
        if !self.can_save {
            return Err("Saved data is unreadable; cannot change the font".into());
        }
        if family.is_some_and(|name| !self.fonts.iter().any(|font| font == name)) {
            return Err("That font is not installed".into());
        }
        let mut store = self.store.borrow_mut();
        let previous = store.selected_font.clone();
        store.selected_font = family.map(str::to_string);
        if let Err(error) = store.save() {
            store.selected_font = previous;
            return Err(error);
        }
        drop(store);
        self.font_css
            .load_from_data(&family.map(fonts::css).unwrap_or_default());
        self.refresh();
        self.error(
            &family
                .map(|name| format!("Spotlight is now using {name}"))
                .unwrap_or_else(|| "Default font restored".into()),
        );
        Ok(())
    }
    fn font_preview(self: &Rc<Self>, family: &str) {
        let dialog = gtk::Window::builder()
            .title(format!("Font Preview · {family}"))
            .application(&self.window.application().unwrap())
            .transient_for(&self.window)
            .modal(true)
            .default_width(620)
            .default_height(330)
            .build();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 14);
        panel.add_css_class("editor");
        let title = gtk::Label::new(Some(family));
        title.set_xalign(0.);
        title.add_css_class("item-title");
        panel.append(&title);
        let input = gtk::Entry::builder()
            .placeholder_text("Type text to preview")
            .text("The quick brown fox jumps over the lazy dog. 0123456789")
            .build();
        panel.append(&input);
        let sample = gtk::Label::new(Some(&input.text()));
        sample.set_wrap(true);
        sample.set_selectable(true);
        sample.set_xalign(0.);
        sample.set_vexpand(true);
        let mut desc = gtk::pango::FontDescription::new();
        desc.set_family(family);
        desc.set_size(24 * gtk::pango::SCALE);
        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrFontDesc::new(&desc));
        sample.set_attributes(Some(&attrs));
        panel.append(&sample);
        input.connect_changed(move |entry| sample.set_text(&entry.text()));
        let status = gtk::Label::new(Some(
            "Use in Spotlight changes this launcher. Copy Font Name lets you choose it in another app.",
        ));
        status.set_wrap(true);
        status.set_xalign(0.);
        panel.append(&status);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let close = gtk::Button::with_label("Close");
        let copy = gtk::Button::with_label("Copy Font Name");
        let use_font = gtk::Button::with_label("Use in Spotlight");
        use_font.add_css_class("accent");
        buttons.append(&close);
        buttons.append(&copy);
        buttons.append(&use_font);
        panel.append(&buttons);
        dialog.set_child(Some(&panel));
        let d = dialog.downgrade();
        close.connect_clicked(move |_| {
            if let Some(dialog) = d.upgrade() {
                dialog.close();
            }
        });
        let weak = Rc::downgrade(self);
        let name = family.to_string();
        let message = status.clone();
        copy.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.window.clipboard().set_text(&name);
                message.set_text(&format!("Copied {name}"));
            }
        });
        let weak = Rc::downgrade(self);
        let name = family.to_string();
        use_font.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                match ui.set_font(Some(&name)) {
                    Ok(()) => status.set_text(&format!("Spotlight is now using {name}")),
                    Err(error) => status.set_text(&error),
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        let d = dialog.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(dialog) = d.upgrade() {
                    dialog.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        dialog.add_controller(keys);
        dialog.present();
    }
    fn note_destination(&self) -> String {
        match self.store.borrow().obsidian_vault.as_ref() {
            Some(path) => format!(
                "Obsidian · {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            None => "Spotlight local notes · connect Obsidian in Settings".into(),
        }
    }
    fn capture_note(self: &Rc<Self>, text: &str) -> Result<(quicknote::SavedNote, String), String> {
        if !self.can_save {
            return Err("Saved data is unreadable; fix it before saving notes".into());
        }
        let vault = self.store.borrow().obsidian_vault.clone();
        let note = quicknote::save(text, vault.as_deref(), &core::data_dir())?;
        let mut store = self.store.borrow_mut();
        store.quick_notes.insert(0, note.clone());
        store.quick_notes.truncate(100);
        let metadata_error = store.save().err();
        drop(store);
        if *self.files_ready.borrow() && self.files.borrow().len() < 25000 {
            self.files.borrow_mut().push(note.path.clone());
        }
        let message = match metadata_error {
            Some(error) => format!(
                "Saved {} · could not save recent list: {error}",
                note.path.display()
            ),
            None => format!("Saved to {} · {}", self.note_destination(), note.title),
        };
        Ok((note, message))
    }
    fn quick_note(self: &Rc<Self>, initial: &str) {
        if let Some(dialog) = self.note_editor.borrow().as_ref() {
            dialog.present();
            return;
        }
        if !initial.is_empty() {
            *self.note_draft.borrow_mut() = initial.to_string();
        }
        let dialog = gtk::Window::builder()
            .title("Spotlight Quick Note")
            .transient_for(&self.window)
            .modal(true)
            .default_width(560)
            .default_height(360)
            .build();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
        panel.add_css_class("editor");
        let title = gtk::Label::new(Some("Quick Note"));
        title.set_xalign(0.);
        title.add_css_class("item-title");
        panel.append(&title);
        let target = gtk::Label::new(Some(&self.note_destination()));
        target.set_xalign(0.);
        target.set_wrap(true);
        target.add_css_class("subtitle");
        panel.append(&target);
        let text = gtk::TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.buffer().set_text(&self.note_draft.borrow());
        let scroll = gtk::ScrolledWindow::builder()
            .child(&text)
            .vexpand(true)
            .min_content_height(180)
            .build();
        panel.append(&scroll);
        let status = gtk::Label::new(Some(
            "Ctrl+Enter to save · Escape closes and keeps your draft",
        ));
        status.set_xalign(0.);
        status.set_wrap(true);
        status.add_css_class("subtitle");
        panel.append(&status);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Close");
        let save = gtk::Button::with_label("Save Note");
        save.add_css_class("accent");
        let open = gtk::Button::with_label("Save & Open");
        buttons.append(&cancel);
        buttons.append(&open);
        buttons.append(&save);
        panel.append(&buttons);
        dialog.set_child(Some(&panel));
        let weak = Rc::downgrade(self);
        text.buffer().connect_changed(move |buffer| {
            if let Some(ui) = weak.upgrade() {
                *ui.note_draft.borrow_mut() = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
            }
        });
        for (button, should_open) in [(&save, false), (&open, true)] {
            let weak = Rc::downgrade(self);
            let buffer = text.buffer();
            let status = status.clone();
            let d = dialog.downgrade();
            button.connect_clicked(move |_| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let content = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                match ui.capture_note(&content) {
                    Ok((note, message)) => {
                        ui.note_draft.borrow_mut().clear();
                        if let Some(dialog) = d.upgrade() {
                            dialog.close();
                        }
                        ui.search.set_text("");
                        ui.refresh();
                        ui.error(&message);
                        if should_open {
                            match gio::AppInfo::launch_default_for_uri(
                                &quicknote::open_uri(&note),
                                None::<&gio::AppLaunchContext>,
                            ) {
                                Ok(()) => ui.window.set_visible(false),
                                Err(error) => {
                                    ui.error(&format!("Note saved; could not open it: {error}"))
                                }
                            }
                        }
                    }
                    Err(error) => status.set_text(&error),
                }
            });
        }
        let d = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = d.upgrade() {
                dialog.close();
            }
        });
        let weak = Rc::downgrade(self);
        dialog.connect_close_request(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.note_editor.borrow_mut().take();
            }
            glib::Propagation::Proceed
        });
        let keys = gtk::EventControllerKey::new();
        let save_button = save.clone();
        let d = dialog.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gdk::Key::Return && modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                save_button.emit_clicked();
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Escape {
                if let Some(dialog) = d.upgrade() {
                    dialog.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        dialog.add_controller(keys);
        *self.note_editor.borrow_mut() = Some(dialog.clone());
        dialog.present();
        text.grab_focus();
    }
    fn timer_setup(self: &Rc<Self>) {
        let dialog = gtk::Window::builder()
            .title("Set Timer")
            .application(&self.window.application().unwrap())
            .transient_for(&self.window)
            .modal(true)
            .default_width(440)
            .build();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 14);
        panel.add_css_class("editor");
        panel.append(&gtk::Label::new(Some("Choose a timer duration")));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_halign(gtk::Align::Center);
        let hours = gtk::SpinButton::with_range(0., 8760., 1.);
        let minutes = gtk::SpinButton::with_range(0., 59., 1.);
        minutes.set_value(5.);
        let seconds = gtk::SpinButton::with_range(0., 59., 1.);
        for (label, name, spin) in [
            ("Hours", "timer-hours", &hours),
            ("Minutes", "timer-minutes", &minutes),
            ("Seconds", "timer-seconds", &seconds),
        ] {
            spin.set_numeric(true);
            spin.set_widget_name(name);
            let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
            column.append(&gtk::Label::new(Some(label)));
            column.append(spin);
            row.append(&column);
        }
        panel.append(&row);
        panel.append(&gtk::Label::new(Some(
            "The timer keeps running when Spotlight is hidden.",
        )));
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        panel.append(&status);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        let start = gtk::Button::with_label("Start Timer");
        start.add_css_class("accent");
        buttons.append(&cancel);
        buttons.append(&start);
        panel.append(&buttons);
        dialog.set_child(Some(&panel));
        dialog.set_default_widget(Some(&start));
        let d = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = d.upgrade() {
                dialog.close();
            }
        });
        let weak = Rc::downgrade(self);
        let d = dialog.downgrade();
        let minute_entry = minutes.clone();
        start.connect_clicked(move |_| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            hours.update();
            minutes.update();
            seconds.update();
            let duration = hours.value_as_int() as u64 * 3600
                + minutes.value_as_int() as u64 * 60
                + seconds.value_as_int() as u64;
            if duration == 0 {
                status.set_text("Choose a duration greater than zero.");
                return;
            }
            if duration > 31_536_000 {
                status.set_text("Timers can run for up to 365 days.");
                return;
            }
            let before = ui.store.borrow().reminders.len();
            ui.schedule(reminder::Draft::Timer(duration));
            if ui.store.borrow().reminders.len() > before {
                if let Some(dialog) = d.upgrade() {
                    dialog.close();
                }
            } else {
                status.set_text(&ui.notice.text());
            }
        });
        let keys = gtk::EventControllerKey::new();
        let d = dialog.downgrade();
        let button = start.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(dialog) = d.upgrade() {
                    dialog.close();
                }
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Return || key == gdk::Key::KP_Enter {
                button.emit_clicked();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        dialog.add_controller(keys);
        dialog.present();
        minute_entry.grab_focus();
        minute_entry.select_region(0, -1);
    }
    fn alarm_setup(self: &Rc<Self>) {
        let Ok(now) = glib::DateTime::now_local() else {
            return;
        };
        let next = now.add_minutes(1).unwrap_or(now);
        let dialog = gtk::Window::builder()
            .title("Set Alarm")
            .application(&self.window.application().unwrap())
            .transient_for(&self.window)
            .modal(true)
            .default_width(420)
            .build();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 14);
        panel.add_css_class("editor");
        let label = gtk::Label::new(Some("Set an alarm · local time (24-hour clock)"));
        panel.append(&label);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_halign(gtk::Align::Center);
        let hour = gtk::SpinButton::with_range(0., 23., 1.);
        hour.set_numeric(true);
        hour.set_value(next.hour() as f64);
        let minute = gtk::SpinButton::with_range(0., 59., 1.);
        minute.set_numeric(true);
        minute.set_value(next.minute() as f64);
        row.append(&hour);
        row.append(&gtk::Label::new(Some(":")));
        row.append(&minute);
        panel.append(&row);
        panel.append(&gtk::Label::new(Some(
            "An earlier time is scheduled for tomorrow.",
        )));
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        panel.append(&status);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let test = gtk::Button::with_label("Test sound");
        let cancel = gtk::Button::with_label("Cancel");
        let set = gtk::Button::with_label("Set Alarm");
        set.add_css_class("accent");
        buttons.append(&test);
        buttons.append(&cancel);
        buttons.append(&set);
        panel.append(&buttons);
        dialog.set_child(Some(&panel));
        test.connect_clicked(|_| play_alarm_sound());
        let d = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = d.upgrade() {
                dialog.close();
            }
        });
        let weak = Rc::downgrade(self);
        let d = dialog.downgrade();
        set.connect_clicked(move |_| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let before = ui.store.borrow().reminders.len();
            hour.update();
            minute.update();
            ui.schedule(reminder::Draft::Alarm {
                hour: hour.value_as_int(),
                minute: minute.value_as_int(),
            });
            if ui.store.borrow().reminders.len() > before {
                if let Some(dialog) = d.upgrade() {
                    dialog.close();
                }
            } else {
                status.set_text(&ui.notice.text());
            }
        });
        dialog.present();
    }
    fn dismiss_alarms(self: &Rc<Self>) {
        let ids: Vec<_> = self
            .store
            .borrow()
            .ringing_reminders
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
        self.store.borrow_mut().ringing_reminders.clear();
        self.save();
        if let Some(source) = self.alarm_repeat.borrow_mut().take() {
            source.remove();
        }
        if let Some(app) = self.window.application() {
            for id in ids {
                app.withdraw_notification(&format!("reminder-{id}"));
            }
        }
        if let Some(window) = self.alarm_window.borrow_mut().take() {
            window.destroy();
        }
        self.alarm_label.borrow_mut().take();
        self.refresh();
    }
    fn show_alarm(self: &Rc<Self>) {
        if self.store.borrow().ringing_reminders.is_empty() {
            return;
        }
        if let Some(window) = self.alarm_window.borrow().as_ref() {
            if let Some(label) = self.alarm_label.borrow().as_ref() {
                label.set_text(
                    &self
                        .store
                        .borrow()
                        .ringing_reminders
                        .iter()
                        .map(|entry| entry.title.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            window.present();
            return;
        }
        let window: gtk::Window = gtk::ApplicationWindow::builder()
            .application(&self.window.application().unwrap())
            .title("Spotlight Alarm")
            .default_width(440)
            .resizable(false)
            .build()
            .upcast();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 16);
        panel.add_css_class("editor");
        let heading = gtk::Label::new(Some("Alarm · time is up"));
        heading.add_css_class("item-title");
        panel.append(&heading);
        let text = self
            .store
            .borrow()
            .ringing_reminders
            .iter()
            .map(|entry| entry.title.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let label = gtk::Label::new(Some(&text));
        label.set_wrap(true);
        *self.alarm_label.borrow_mut() = Some(label.clone());
        panel.append(&label);
        let dismiss = gtk::Button::with_label("Dismiss Alarm");
        dismiss.add_css_class("accent");
        panel.append(&dismiss);
        window.set_child(Some(&panel));
        let weak = Rc::downgrade(self);
        dismiss.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.dismiss_alarms();
            }
        });
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.dismiss_alarms();
            }
            glib::Propagation::Stop
        });
        *self.alarm_window.borrow_mut() = Some(window.clone());
        window.present();
        play_alarm_sound();
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_seconds_local(5, move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if ui.store.borrow().ringing_reminders.is_empty() {
                ui.alarm_repeat.borrow_mut().take();
                return glib::ControlFlow::Break;
            }
            play_alarm_sound();
            glib::ControlFlow::Continue
        });
        *self.alarm_repeat.borrow_mut() = Some(source);
    }
    fn schedule(self: &Rc<Self>, draft: reminder::Draft) {
        if !self.can_save {
            self.error("Cannot save timers while saved data is unreadable");
            return;
        }
        let result = (|| -> Result<String, String> {
            let now = glib::DateTime::now_local().map_err(|e| e.to_string())?;
            let entry = draft.create(&now)?;
            let due = glib::DateTime::from_unix_local(entry.due)
                .ok()
                .and_then(|date| date.format("%a %d %b %H:%M:%S %Z").ok())
                .map(|date| date.to_string())
                .unwrap_or_default();
            let label = format!("{} · due {due}", entry.title);
            let mut store = self.store.borrow_mut();
            if store.reminders.len() >= 100 {
                return Err("You can set up to 100 timers and alarms".into());
            }
            let id = entry.id.clone();
            store.reminders.push(entry);
            if let Err(error) = store.save() {
                store.reminders.retain(|entry| entry.id != id);
                return Err(error);
            }
            Ok(label)
        })();
        match result {
            Ok(label) => {
                self.search.set_text("");
                self.refresh();
                self.error(&format!("{label} set · search timer or alarm to cancel"));
                self.arm_reminders();
            }
            Err(error) => self.error(&format!("Could not set reminder: {error}")),
        }
    }
    fn cancel_reminder(self: &Rc<Self>, id: &str) {
        if !self.can_save {
            self.error("Saved data cannot be changed");
            return;
        }
        let mut store = self.store.borrow_mut();
        let previous = store.reminders.clone();
        store.reminders.retain(|entry| entry.id != id);
        if let Err(error) = store.save() {
            store.reminders = previous;
            drop(store);
            self.error(&format!("Could not cancel reminder: {error}"));
            return;
        }
        let empty = store.reminders.is_empty();
        drop(store);
        if empty && let Some(source) = self.reminder_clock.borrow_mut().take() {
            source.remove();
        }
        self.refresh();
        self.error("Reminder cancelled");
    }
    fn check_reminders(self: &Rc<Self>) {
        let Ok(now) = glib::DateTime::now_local() else {
            return;
        };
        let mut store = self.store.borrow_mut();
        let due = reminder::take_due(&mut store.reminders, now.to_unix());
        if due.is_empty() {
            let ringing = !store.ringing_reminders.is_empty();
            drop(store);
            if ringing && self.alarm_window.borrow().is_none() {
                self.show_alarm();
            }
            return;
        }
        store.ringing_reminders.extend(due.iter().cloned());
        let saved = store.save();
        drop(store);
        for entry in &due {
            if let Some(app) = self.window.application() {
                let title = if entry.alarm {
                    "Spotlight alarm"
                } else {
                    "Timer finished"
                };
                let notification = gio::Notification::new(title);
                notification.set_body(Some(&entry.title));
                notification.set_priority(gio::NotificationPriority::Urgent);
                notification.set_default_action("app.show-alarm");
                app.send_notification(Some(&format!("reminder-{}", entry.id)), &notification);
            }
        }
        self.show_alarm();
        if self.window.is_visible() {
            self.refresh();
            self.error(&format!(
                "{} finished",
                due.iter()
                    .map(|entry| entry.title.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Err(error) = saved {
            self.error(&format!("Could not save completed reminders: {error}"));
        }
    }
    fn arm_reminders(self: &Rc<Self>) {
        self.check_reminders();
        if self.reminder_clock.borrow().is_some() || self.store.borrow().reminders.is_empty() {
            return;
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_seconds_local(1, move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            ui.check_reminders();
            if ui.store.borrow().reminders.is_empty() {
                ui.reminder_clock.borrow_mut().take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        *self.reminder_clock.borrow_mut() = Some(source);
    }
    fn finish_color(self: &Rc<Self>, result: Result<Option<String>, String>) {
        self.picker.borrow_mut().take();
        self.picking_color.set(false);
        match result {
            Ok(Some(hex)) => {
                self.window.clipboard().set_text(&hex);
                *self.picked_color.borrow_mut() = Some(hex.clone());
                self.page(Page::Colors);
                self.error(&format!(
                    "Copied {hex} · select another format and press Enter to copy"
                ));
            }
            Ok(None) => self.error("Color selection cancelled"),
            Err(message) => self.error(&message),
        }
        self.window.present();
        self.search.grab_focus();
    }
    fn pick_color(self: &Rc<Self>) {
        if self.picking_color.replace(true) {
            return;
        }
        self.window.set_visible(false);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let bus = match gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) {
                Ok(bus) => bus,
                Err(error) => {
                    ui.finish_color(Err(format!(
                        "Could not connect to the desktop color picker: {error}"
                    )));
                    return;
                }
            };
            let token = format!("spotlight_{}", glib::uuid_string_random().replace('-', ""));
            let sender = bus
                .unique_name()
                .unwrap()
                .trim_start_matches(':')
                .replace('.', "_");
            let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
            let weak = Rc::downgrade(&ui);
            let subscription = bus.subscribe_to_signal(
                Some("org.freedesktop.portal.Desktop"),
                Some("org.freedesktop.portal.Request"),
                Some("Response"),
                Some(&path),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let Some(ui) = weak.upgrade() {
                        ui.finish_color(color::response(signal.parameters));
                        ui.refresh();
                    }
                },
            );
            *ui.picker.borrow_mut() = Some((path.clone(), subscription));
            let options = std::collections::HashMap::from([("handle_token", token.to_variant())]);
            let weak = Rc::downgrade(&ui);
            bus.call(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Screenshot",
                "PickColor",
                Some(&("", options).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                10000,
                None::<&gio::Cancellable>,
                move |result| {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(reply) => {
                            let returned = reply.child_value(0);
                            if returned.str() != Some(path.as_str()) && ui.picker.borrow().is_some()
                            {
                                ui.finish_color(Err(
                                    "Unexpected response from the desktop color picker".into(),
                                ));
                            }
                        }
                        Err(error) => ui.finish_color(Err(format!(
                            "Could not open the desktop color picker: {error}"
                        ))),
                    }
                },
            );
        });
    }
    fn root(&self) -> Vec<Item> {
        let mut out = self.apps.borrow().clone();
        if self.store.borrow().selected_font.is_some() {
            out.push(item(
                "reset-font",
                "Reset Spotlight Font",
                "Restore Spotlight's default font",
                "preferences-desktop-font-symbolic",
                "Font",
                Action::ResetFont,
            ));
        }
        if !self.store.borrow().ringing_reminders.is_empty() {
            out.push(item(
                "dismiss-alarm",
                "Dismiss Alarm",
                "Stop the alarm sound and close the alert",
                "alarm-symbolic",
                "Dismiss",
                Action::DismissAlarms,
            ));
        }
        out.push(item(
            "quick-note",
            "Quick Note",
            &format!("{} · multiline capture", self.note_destination()),
            "document-edit-symbolic",
            "Note",
            Action::QuickNote(String::new()),
        ));
        let store = self.store.borrow();
        if let Some(vault) = &store.obsidian_vault {
            out.push(item(
                "obsidian-vault",
                "Open Obsidian Vault",
                &vault.to_string_lossy(),
                "document-open-symbolic",
                "Obsidian",
                Action::Open(format!(
                    "obsidian://open?vault={}",
                    core::encode(&vault.file_name().unwrap_or_default().to_string_lossy())
                )),
            ));
        }
        for note in &store.quick_notes {
            if note.path.is_file() {
                out.push(item(
                    &format!("quick-note:{}", note.path.display()),
                    &note.title,
                    if note.obsidian {
                        "Quick note · Open in Obsidian"
                    } else {
                        "Quick note · Open Markdown file"
                    },
                    "document-edit-symbolic",
                    "Note",
                    Action::Open(quicknote::open_uri(note)),
                ));
            }
        }
        drop(store);
        out.push(item(
            "set-timer",
            "Set Timer",
            "Type timer 5m or timer 1h30m · runs while Spotlight is hidden",
            "alarm-symbolic",
            "Command",
            Action::TimerSetup,
        ));
        out.push(item(
            "set-alarm",
            "Set Alarm",
            "Type alarm 7:30 am or alarm 18:45 · local time",
            "alarm-symbolic",
            "Command",
            Action::AlarmSetup,
        ));
        for entry in &self.store.borrow().reminders {
            let date = glib::DateTime::from_unix_local(entry.due)
                .ok()
                .and_then(|date| date.format("%a %d %b · %H:%M:%S").ok())
                .map(|date| date.to_string())
                .unwrap_or_default();
            out.push(item(
                &format!("reminder:{}", entry.id),
                &format!("Cancel {}", entry.title),
                &format!("Due {date} · Enter to cancel"),
                "alarm-symbolic",
                "Active",
                Action::CancelReminder(entry.id.clone()),
            ));
        }
        let mut picker = item(
            "color-picker",
            "Color Picker",
            "Pick a screen pixel · choose HEX, RGB, HSL, HSV · Escape cancels",
            "color-select-symbolic",
            "Command",
            Action::PickColor,
        );
        picker.keywords = "color colour picker pick eyedropper pipette screen pixel".into();
        out.push(picker);
        out.push(item(
            "color-formats",
            "Color Formats",
            "Convert the picked color · HEX, RGB, HSL, HSV",
            "color-select-symbolic",
            "Colors",
            Action::Page(Page::Colors),
        ));
        if let Some(hex) = self.picked_color.borrow().as_ref() {
            let mut picked = item(
                "picked-color",
                hex,
                "Last picked color · Enter to choose a copy format",
                "color-select-symbolic",
                "Color",
                Action::Page(Page::Colors),
            );
            picked.keywords = "color colour picked formats hex rgb rgba hsl hsla hsv hsb".into();
            out.push(picked);
        }
        for (p, name, desc, icon) in [
            (
                Page::Windows,
                "Window Manager",
                "Tile, maximize, restore, or move your active window · KDE",
                "view-grid-symbolic",
            ),
            (
                Page::Files,
                "Search Files",
                "Find files and folders in your home directory",
                "folder-symbolic",
            ),
            (
                Page::Clipboard,
                "Clipboard History",
                "Search and copy previous clipboard entries",
                "edit-paste-symbolic",
            ),
            (
                Page::Snippets,
                "Snippets",
                "Keep text you use often within reach",
                "insert-text-symbolic",
            ),
            (
                Page::Links,
                "Quicklinks",
                "Your favorite websites, one keystroke away",
                "web-browser-symbolic",
            ),
            (
                Page::Notes,
                "Notes",
                "Capture an idea without leaving your flow",
                "document-edit-symbolic",
            ),
            (
                Page::Scripts,
                "Script Commands",
                "Run your own executable scripts",
                "utilities-terminal-symbolic",
            ),
            (
                Page::Fonts,
                "Search Fonts",
                "Preview installed fonts · use in Spotlight or copy the name",
                "preferences-desktop-font-symbolic",
            ),
            (
                Page::Emoji,
                "Emoji & Symbols",
                "Everyday emoji · your most-used picks first",
                "face-smile-symbolic",
            ),
        ] {
            out.push(item(name, name, desc, icon, "Command", Action::Page(p)));
        }
        for (name, icon, args) in [
            (
                "Lock Screen",
                "system-lock-screen-symbolic",
                vec!["loginctl", "lock-session"],
            ),
            (
                "Take Screenshot",
                "camera-photo-symbolic",
                vec!["spectacle"],
            ),
            (
                "System Settings",
                "preferences-system-symbolic",
                vec!["systemsettings"],
            ),
        ] {
            out.push(item(
                name,
                name,
                "System action",
                icon,
                "System",
                Action::System(args.into_iter().map(str::to_string).collect()),
            ));
        }
        out.push(item(
            "settings",
            "Spotlight Settings",
            "Manage your launcher and keyboard shortcut",
            "emblem-system-symbolic",
            "Command",
            Action::Settings,
        ));
        out.push(item(
            "quit",
            "Quit Spotlight",
            "Stop the background launcher",
            "application-exit-symbolic",
            "Command",
            Action::Quit,
        ));
        let store = self.store.borrow();
        for (i, link) in store.links.iter().enumerate() {
            out.push(item(
                &format!("link:{i}"),
                &link.name,
                &link.value,
                "web-browser-symbolic",
                "Quicklink",
                Action::Open(link.value.clone()),
            ));
        }
        out
    }
    fn currency_item(self: &Rc<Self>, query: currency::Query) -> Item {
        let (source, target) = &query.pair;
        if source == target {
            let value = currency::convert(&query, 1.).unwrap();
            return item(
                "currency",
                &value,
                "Same currency · Enter to copy",
                "accessories-calculator-symbolic",
                "Currency",
                Action::Copy(value.clone()),
            );
        }
        if let Some((fetched, result)) = self.currency.borrow().cache.get(&query.pair) {
            let ttl = if result.is_ok() { 900 } else { 30 };
            if fetched.elapsed().as_secs() < ttl {
                return match result {
                    Ok(rate) => match currency::convert(&query, rate.rate) {
                        Some(value) => item(
                            "currency",
                            &value,
                            &format!(
                                "Frankfurter · rate date {} · 1 {source} = {} {target} · Enter to copy",
                                rate.date, rate.rate
                            ),
                            "accessories-calculator-symbolic",
                            "Currency",
                            Action::Copy(value.clone()),
                        ),
                        None => item(
                            "currency",
                            "Amount is too large",
                            "Try a smaller amount",
                            "dialog-warning-symbolic",
                            "Currency",
                            Action::Noop,
                        ),
                    },
                    Err(message) => item(
                        "currency",
                        "Currency conversion unavailable",
                        message,
                        "network-offline-symbolic",
                        "Currency",
                        Action::Noop,
                    ),
                };
            }
        }
        if self
            .currency
            .borrow_mut()
            .pending
            .insert(query.pair.clone())
        {
            let weak = Rc::downgrade(self);
            let pair = query.pair.clone();
            glib::timeout_add_local_once(Duration::from_millis(400), move || {
                let Some(ui) = weak.upgrade() else { return };
                let current = currency::parse(&ui.search.text());
                if *ui.page.borrow() != Page::Root || !current.is_some_and(|q| q.pair == pair) {
                    ui.currency.borrow_mut().pending.remove(&pair);
                    return;
                }
                let (tx, rx) = mpsc::channel();
                let request_pair = pair.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(currency::fetch(&request_pair));
                });
                let weak = Rc::downgrade(&ui);
                glib::timeout_add_local(Duration::from_millis(80), move || {
                    let Some(ui) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    let result = match rx.try_recv() {
                        Ok(result) => result,
                        Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                        Err(_) => Err("Currency request interrupted".into()),
                    };
                    let mut state = ui.currency.borrow_mut();
                    state.pending.remove(&pair);
                    if state.cache.len() >= 64 {
                        state.cache.clear();
                    }
                    state
                        .cache
                        .insert(pair.clone(), (std::time::Instant::now(), result));
                    drop(state);
                    if *ui.page.borrow() == Page::Root
                        && currency::parse(&ui.search.text()).is_some_and(|q| q.pair == pair)
                    {
                        ui.refresh();
                    }
                    glib::ControlFlow::Break
                });
            });
        }
        item(
            "currency",
            "Fetching exchange rate…",
            &format!("{source} → {target} · Frankfurter"),
            "network-transmit-receive-symbolic",
            "Currency",
            Action::Noop,
        )
    }
    fn refresh(self: &Rc<Self>) {
        let query = self.search.text().to_string();
        let q = query.trim();
        let page = *self.page.borrow();
        self.section.set_text(page.title());
        if page == Page::Root && !q.is_empty() {
            self.load_files();
            if *self.file_loading.borrow() {
                self.section.set_text("Searching · indexing your files…");
            }
        }
        let mut items = match page {
            Page::Root => self.root(),
            Page::Windows => window_items(),
            Page::Files => {
                if *self.file_loading.borrow() {
                    self.section.set_text("Indexing your home folder…");
                }
                self.file_items(q)
            }
            Page::Clipboard => self
                .clips
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    item(
                        &format!("clip:{i}"),
                        &s.lines()
                            .next()
                            .unwrap_or("Text")
                            .chars()
                            .take(100)
                            .collect::<String>(),
                        &format!("{} characters · Enter to copy", s.chars().count()),
                        "edit-paste-symbolic",
                        "Text",
                        Action::Copy(s.clone()),
                    )
                })
                .collect(),
            Page::Snippets | Page::Links | Page::Notes => {
                let store = self.store.borrow();
                let source = match page {
                    Page::Snippets => &store.snippets,
                    Page::Links => &store.links,
                    _ => &store.notes,
                };
                let mut v: Vec<_> = source
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        item(
                            &format!("text:{i}"),
                            &s.name,
                            &s.value
                                .lines()
                                .next()
                                .unwrap_or("")
                                .chars()
                                .take(100)
                                .collect::<String>(),
                            if page == Page::Links {
                                "web-browser-symbolic"
                            } else {
                                "document-edit-symbolic"
                            },
                            "⌃E Edit",
                            if page == Page::Links {
                                Action::Open(s.value.clone())
                            } else if page == Page::Notes {
                                Action::Edit(page, i)
                            } else {
                                Action::Copy(s.value.clone())
                            },
                        )
                    })
                    .collect();
                if source.is_empty() {
                    v.push(item(
                        "new",
                        "Create your first item",
                        "Press Enter to get started",
                        "list-add-symbolic",
                        "Create",
                        Action::New(page),
                    ));
                }
                v
            }
            Page::Scripts => {
                use std::os::unix::fs::PermissionsExt;
                let dir = core::data_dir().join("scripts");
                std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| {
                        e.file_type().is_ok_and(|t| t.is_file())
                            && e.metadata()
                                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
                    })
                    .map(|e| {
                        let path = e.path();
                        let name = e.file_name().to_string_lossy().replace(['_', '-'], " ");
                        item(
                            &path.to_string_lossy(),
                            &name,
                            "Run executable · Ctrl+K for actions",
                            "utilities-terminal-symbolic",
                            "Script",
                            Action::Script(path.clone()),
                        )
                    })
                    .collect()
            }
            Page::Emoji => emoji_items(),
            Page::Fonts => self.font_items(),
            Page::Colors => self.color_items(),
        };
        if page == Page::Root && !q.is_empty() {
            items.extend(emoji_items());
            items.extend(self.file_items(q));
            items.extend(self.font_items());
            if self.picked_color.borrow().is_some() {
                items.extend(self.color_items());
            }
        }
        let store = self.store.borrow();
        let mut ranked: Vec<_> = items
            .drain(..)
            .filter_map(|i| {
                let s = if matches!(i.action, Action::FontPreview(_)) {
                    let explicit = page == Page::Fonts || fonts::query(q).is_some();
                    let score = core::score(fonts::query(q).unwrap_or(q), &i.title);
                    score.filter(|score| explicit || *score >= 500)
                } else if !i.keywords.is_empty() {
                    core::score(q, &i.title).max(core::score(q, &i.keywords).map(|s| s - 100))
                } else if page == Page::Clipboard {
                    if let Action::Copy(ref text) = i.action {
                        core::score(q, text)
                    } else {
                        None
                    }
                } else {
                    core::score(q, &i.title)
                        .or_else(|| core::score(q, &i.subtitle).map(|s| s - 100))
                }?;
                let bonus = if page == Page::Root {
                    (if store.favorites.contains(&i.id) {
                        300
                    } else {
                        0
                    }) + store.usage.get(&i.id).copied().unwrap_or(0).min(100) as i32
                        + if q.is_empty() && matches!(i.action, Action::Page(_)) {
                            25
                        } else {
                            0
                        }
                } else {
                    0
                };
                Some((s + bonus, i))
            })
            .collect();
        if page == Page::Emoji && q.is_empty() {
            // Stable sorting keeps the curated order when personal usage ties.
            ranked.sort_by(|a, b| {
                store
                    .favorites
                    .contains(&b.1.id)
                    .cmp(&store.favorites.contains(&a.1.id))
                    .then_with(|| {
                        store
                            .usage
                            .get(&b.1.id)
                            .copied()
                            .unwrap_or(0)
                            .cmp(&store.usage.get(&a.1.id).copied().unwrap_or(0))
                    })
            });
        } else if page != Page::Colors || !q.is_empty() {
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        }
        let limit = if matches!(page, Page::Emoji | Page::Fonts) {
            self.result_limit.get()
        } else {
            60
        };
        let total = ranked.len();
        let mut results: Vec<Item> = ranked.into_iter().take(limit).map(|(_, i)| i).collect();
        if matches!(page, Page::Emoji | Page::Fonts) {
            self.section
                .set_text(&format!("{} · {} of {total}", page.title(), results.len()));
            if total > limit {
                results.push(item(
                    "results:more",
                    if page == Page::Fonts {
                        "Show more fonts"
                    } else {
                        "Show more emoji"
                    },
                    "Load the next 60 results",
                    "list-add-symbolic",
                    "Enter",
                    Action::MoreResults,
                ));
            }
        }
        drop(store);
        if page == Page::Root && !q.is_empty() {
            if fonts::query(q) == Some("") {
                results.retain(|entry| !matches!(entry.action, Action::Page(Page::Fonts)));
                results.insert(
                    0,
                    item(
                        "font-browser",
                        "Search Fonts",
                        "Browse, preview, and use installed fonts",
                        "preferences-desktop-font-symbolic",
                        "Fonts",
                        Action::Page(Page::Fonts),
                    ),
                );
            }
            if let Some(content) = quicknote::query(q) {
                let action = if content.is_empty() {
                    Action::QuickNote(String::new())
                } else {
                    Action::CaptureNote(content.to_string())
                };
                results.insert(
                    0,
                    item(
                        "capture-note",
                        if content.is_empty() {
                            "Quick Note"
                        } else {
                            "Save Quick Note"
                        },
                        &format!(
                            "{} · Enter to {}",
                            self.note_destination(),
                            if content.is_empty() {
                                "write a note"
                            } else {
                                "save"
                            }
                        ),
                        "document-edit-symbolic",
                        "Note",
                        action,
                    ),
                );
            }
            if let Some(parsed) = reminder::parse(q) {
                let entry = match parsed {
                    Ok(draft) => item(
                        "schedule-reminder",
                        &format!("Set {}", draft.label()),
                        "Enter to start · search timer or alarm to cancel",
                        "alarm-symbolic",
                        "Set",
                        Action::Schedule(draft),
                    ),
                    Err(hint) => item(
                        "reminder-help",
                        "Set a timer or alarm",
                        hint,
                        "alarm-symbolic",
                        "Set",
                        if ["alarm", "set alarm", "set an alarm"]
                            .contains(&q.to_ascii_lowercase().as_str())
                        {
                            Action::AlarmSetup
                        } else if ["timer", "set timer", "set a timer"]
                            .contains(&q.to_ascii_lowercase().as_str())
                        {
                            Action::TimerSetup
                        } else {
                            Action::Noop
                        },
                    ),
                };
                results.insert(0, entry);
            }
            if let Some(answer) = core::calculate(q) {
                results.insert(
                    0,
                    item(
                        "calculator",
                        &answer,
                        q,
                        "accessories-calculator-symbolic",
                        "Copy result",
                        Action::Copy(answer.clone()),
                    ),
                );
            }
            if core::calculate(q).is_none() {
                if let Some(query) = currency::parse(q) {
                    results.insert(0, self.currency_item(query));
                }
            }
            if q.starts_with("https://") || q.starts_with("http://") {
                results.insert(
                    0,
                    item(
                        "url",
                        &format!("Open {q}"),
                        "Open in your default browser",
                        "web-browser-symbolic",
                        "Open",
                        Action::Open(q.into()),
                    ),
                );
            }
            results.push(item(
                "web",
                &format!("Search Google for “{q}”"),
                "Google · Ctrl+Enter · default browser",
                "system-search-symbolic",
                "Search",
                Action::Open(format!(
                    "https://www.google.com/search?q={}",
                    core::encode(q)
                )),
            ));
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for i in &results {
            self.list.append(&self.row(i));
        }
        if results.is_empty() {
            self.section.set_text(if page == Page::Scripts {
                "No scripts yet · add executable files in Settings → Scripts folder"
            } else {
                "No results · try another search"
            });
        }
        *self.visible.borrow_mut() = results;
        if let Some(row) = self.list.row_at_index(0) {
            self.list.select_row(Some(&row));
        }
        self.hint.set_text(
            if matches!(page, Page::Snippets | Page::Links | Page::Notes) {
                "⌃N New    ⌃E Edit    ⌃D Delete"
            } else if page == Page::Root && !q.is_empty() {
                "↑ ↓ Navigate    ⌃Enter Google"
            } else {
                "↑ ↓ Navigate"
            },
        );
    }
    fn row(&self, i: &Item) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        if let Some(icon) = &i.gicon {
            let image = gtk::Image::from_gicon(icon);
            image.set_pixel_size(28);
            image.add_css_class("tile");
            line.append(&image);
        } else if i.icon.ends_with("symbolic") {
            let image = gtk::Image::from_icon_name(&i.icon);
            image.set_pixel_size(23);
            image.add_css_class("tile");
            line.append(&image);
        } else {
            let label = gtk::Label::new(Some(&i.icon));
            if i.id.starts_with("color-format:") {
                if let Some(rgb) = self
                    .picked_color
                    .borrow()
                    .as_ref()
                    .and_then(|hex| color::rgb(hex).ok())
                {
                    let attrs = gtk::pango::AttrList::new();
                    attrs.insert(gtk::pango::AttrColor::new_foreground(
                        rgb[0] as u16 * 257,
                        rgb[1] as u16 * 257,
                        rgb[2] as u16 * 257,
                    ));
                    label.set_attributes(Some(&attrs));
                }
            }
            label.add_css_class("tile");
            line.append(&label);
        }
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 3);
        texts.set_hexpand(true);
        let title = gtk::Label::new(Some(&i.title));
        title.set_xalign(0.);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("item-title");
        texts.append(&title);
        if !i.subtitle.is_empty() {
            let sub = gtk::Label::new(Some(&i.subtitle));
            sub.set_xalign(0.);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            sub.add_css_class("subtitle");
            if let Action::FontPreview(family) = &i.action {
                let mut desc = gtk::pango::FontDescription::new();
                desc.set_family(family);
                let attrs = gtk::pango::AttrList::new();
                attrs.insert(gtk::pango::AttrFontDesc::new(&desc));
                sub.set_attributes(Some(&attrs));
            }
            texts.append(&sub);
        }
        line.append(&texts);
        let fav = self.store.borrow().favorites.contains(&i.id);
        let badge = gtk::Label::new(Some(if fav { "★ Favorite" } else { &i.badge }));
        badge.add_css_class("badge");
        line.append(&badge);
        row.set_child(Some(&line));
        row
    }
    fn launch(self: &Rc<Self>, i: Item) {
        {
            let mut store = self.store.borrow_mut();
            let n = store.usage.entry(i.id.clone()).or_default();
            *n = n.saturating_add(1);
        }
        self.save();
        let result: Result<(), String> = match i.action {
            Action::App(app) => app
                .launch(&[], None::<&gio::AppLaunchContext>)
                .map_err(|e| e.to_string()),
            Action::Open(uri) => {
                gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
                    .map_err(|e| e.to_string())
            }
            Action::QuickNote(initial) => {
                self.quick_note(&initial);
                return;
            }
            Action::CaptureNote(content) => {
                match self.capture_note(&content) {
                    Ok((_, message)) => {
                        self.search.set_text("");
                        self.refresh();
                        self.error(&message);
                    }
                    Err(error) => self.error(&error),
                }
                return;
            }

            Action::Schedule(draft) => {
                self.schedule(draft);
                return;
            }
            Action::CancelReminder(id) => {
                self.cancel_reminder(&id);
                return;
            }
            Action::DismissAlarms => {
                self.dismiss_alarms();
                return;
            }
            Action::FontPreview(family) => {
                self.font_preview(&family);
                return;
            }
            Action::ResetFont => {
                if let Err(error) = self.set_font(None) {
                    self.error(&error);
                }
                return;
            }
            Action::TimerSetup => {
                self.timer_setup();
                return;
            }
            Action::AlarmSetup => {
                self.alarm_setup();
                return;
            }
            Action::PickColor => {
                self.pick_color();
                return;
            }
            Action::Noop => return,
            Action::MoreResults => {
                let next = self.result_limit.get();
                self.result_limit.set(next + 60);
                self.refresh();
                if let Some(row) = self.list.row_at_index(next as i32) {
                    self.list.select_row(Some(&row));
                    row.grab_focus();
                    self.search.grab_focus();
                }
                return;
            }
            Action::Copy(text) => {
                self.window.clipboard().set_text(&text);
                self.error("Copied to clipboard");
                return;
            }
            Action::Page(p) => {
                self.page(p);
                return;
            }
            Action::New(p) => {
                self.editor(p, None);
                return;
            }
            Action::Edit(p, n) => {
                self.editor(p, Some(n));
                return;
            }
            Action::System(args) => Command::new(&args[0])
                .args(&args[1..])
                .spawn()
                .map(|mut child| {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                })
                .map_err(|e| e.to_string()),
            Action::Window(shortcut) => {
                self.window_action(shortcut);
                return;
            }
            Action::Script(path) => {
                self.run_script(path);
                return;
            }
            Action::Settings => {
                self.settings();
                return;
            }
            Action::Quit => {
                if let Some(app) = self.window.application() {
                    app.quit();
                }
                return;
            }
        };
        match result {
            Ok(()) => self.window.set_visible(false),
            Err(e) => self.error(&format!("Could not open: {e}")),
        }
    }
    fn window_action(self: &Rc<Self>, shortcut: &'static str) {
        // Let KWin return focus to the previous app before invoking its action.
        self.window.set_visible(false);
        let ui = Rc::clone(self);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            let result = (|| -> Result<(), String> {
                if ui.window.is_visible() || ui.window.is_active() {
                    return Err("Spotlight is still focused. Try the window action again.".into());
                }
                let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
                    .map_err(|e| e.to_string())?;
                bus.call_sync(
                    Some("org.kde.kglobalaccel"),
                    "/component/kwin",
                    "org.kde.kglobalaccel.Component",
                    "invokeShortcut",
                    Some(&(shortcut,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    1000,
                    None::<&gio::Cancellable>,
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })();
            if let Err(e) = result {
                ui.window.present();
                ui.error(&format!("Window actions require KDE KWin: {e}"));
            }
        });
    }
    fn activate(self: &Rc<Self>) {
        if let Some(i) = self.selected() {
            self.launch(i);
        }
    }
    fn favorite(self: &Rc<Self>) {
        if let Some(i) = self.selected() {
            let mut store = self.store.borrow_mut();
            if store.favorites.contains(&i.id) {
                store.favorites.retain(|x| x != &i.id);
            } else {
                store.favorites.push(i.id);
            }
            drop(store);
            self.save();
            self.refresh();
        }
    }
    fn edit_selected(self: &Rc<Self>) {
        let p = *self.page.borrow();
        if matches!(p, Page::Snippets | Page::Links | Page::Notes)
            && let Some(i) = self.selected()
            && let Some(n) = i.id.strip_prefix("text:").and_then(|s| s.parse().ok())
        {
            self.editor(p, Some(n));
        }
    }
    fn delete_selected(self: &Rc<Self>) {
        let p = *self.page.borrow();
        if !matches!(p, Page::Snippets | Page::Links | Page::Notes) {
            return;
        }
        if let Some(n) = self.selected().and_then(|i| {
            i.id.strip_prefix("text:")
                .and_then(|s| s.parse::<usize>().ok())
        }) {
            let mut s = self.store.borrow_mut();
            let v = match p {
                Page::Snippets => &mut s.snippets,
                Page::Links => &mut s.links,
                _ => &mut s.notes,
            };
            if n < v.len() {
                v.remove(n);
            }
            drop(s);
            self.save();
            self.refresh();
        }
    }
    fn editor(self: &Rc<Self>, page: Page, index: Option<usize>) {
        let old = {
            let s = self.store.borrow();
            let v = match page {
                Page::Snippets => &s.snippets,
                Page::Links => &s.links,
                _ => &s.notes,
            };
            index.and_then(|n| v.get(n).cloned())
        };
        let dialog = gtk::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title(if old.is_some() {
                "Edit item"
            } else {
                "Create item"
            })
            .default_width(500)
            .default_height(350)
            .build();
        let container = gtk::Box::new(gtk::Orientation::Vertical, 12);
        container.add_css_class("editor");
        let title = gtk::Label::new(Some(match page {
            Page::Links => "Quicklink",
            Page::Snippets => "Snippet",
            _ => "Note",
        }));
        title.set_xalign(0.);
        title.add_css_class("item-title");
        container.append(&title);
        let name = gtk::Entry::builder().placeholder_text("Name").build();
        if let Some(s) = &old {
            name.set_text(&s.name);
        }
        container.append(&name);
        let value = gtk::TextView::new();
        value.set_wrap_mode(gtk::WrapMode::WordChar);
        if let Some(s) = &old {
            value.buffer().set_text(&s.value);
        }
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&value)
            .vexpand(true)
            .min_content_height(160)
            .build();
        container.append(&scrolled);
        if page == Page::Links {
            container.append(&gtk::Label::new(Some(
                "Use a full URL, for example https://github.com",
            )));
        }
        let status = gtk::Label::new(None);
        status.add_css_class("notice");
        container.append(&status);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::with_label("Save");
        save.add_css_class("accent");
        buttons.append(&cancel);
        buttons.append(&save);
        container.append(&buttons);
        dialog.set_child(Some(&container));
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
        let ui = Rc::clone(self);
        let d = dialog.clone();
        save.connect_clicked(move |_| {
            let n = name.text().trim().to_string();
            let b = value.buffer();
            let v = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
            if n.is_empty() || v.trim().is_empty() {
                status.set_text("Add a name and content.");
                return;
            }
            if page == Page::Links && !v.starts_with("https://") && !v.starts_with("http://") {
                status.set_text("Quicklinks must start with https:// or http://");
                return;
            }
            let mut s = ui.store.borrow_mut();
            let list = match page {
                Page::Snippets => &mut s.snippets,
                Page::Links => &mut s.links,
                _ => &mut s.notes,
            };
            let entry = TextItem { name: n, value: v };
            if let Some(index) = index {
                if let Some(i) = list.get_mut(index) {
                    *i = entry;
                }
            } else {
                list.push(entry);
            }
            drop(s);
            ui.save();
            ui.refresh();
            d.close();
        });
        dialog.present();
    }
    fn settings(self: &Rc<Self>) {
        let dialog = gtk::Window::builder()
            .title("Spotlight Settings")
            .transient_for(&self.window)
            .modal(true)
            .default_width(560)
            .default_height(660)
            .build();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 16);
        panel.add_css_class("editor");
        for text in [
            "Spotlight · native Rust launcher",
            "Open launcher: Meta + Space (KDE)",
            "Enter opens · Esc goes back or hides · Ctrl+K shows actions",
            "Ctrl+F toggles favorite · Ctrl+N creates an item",
            "Clipboard history comes from KDE Klipper; Spotlight does not record it.",
            "File search scans your home on demand, up to 25,000 entries.",
            "Hidden folders, build output, and symbolic links are skipped.",
        ] {
            let l = gtk::Label::new(Some(text));
            l.set_wrap(true);
            l.set_xalign(0.);
            panel.append(&l);
        }
        let font_label = gtk::Label::new(Some(&format!(
            "Spotlight font: {}",
            self.store
                .borrow()
                .selected_font
                .as_deref()
                .unwrap_or("Default")
        )));
        font_label.set_xalign(0.);
        font_label.set_wrap(true);
        panel.append(&font_label);
        let font_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let search_fonts = gtk::Button::with_label("Search Fonts");
        let reset_font = gtk::Button::with_label("Reset Font");
        font_buttons.append(&search_fonts);
        font_buttons.append(&reset_font);
        panel.append(&font_buttons);
        let weak = Rc::downgrade(self);
        let parent = dialog.downgrade();
        search_fonts.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                if let Some(dialog) = parent.upgrade() {
                    dialog.close();
                }
                ui.page(Page::Fonts);
            }
        });
        let weak = Rc::downgrade(self);
        reset_font.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                match ui.set_font(None) {
                    Ok(()) => font_label.set_text("Spotlight font: Default"),
                    Err(error) => font_label.set_text(&error),
                }
            }
        });
        let vaults = quicknote::discover();
        let destination =
            gtk::Label::new(Some(&format!("Quick notes: {}", self.note_destination())));
        destination.set_xalign(0.);
        destination.set_wrap(true);
        panel.append(&destination);
        if !vaults.is_empty() {
            let found = gtk::Label::new(Some(&format!(
                "Detected vaults: {}",
                vaults
                    .iter()
                    .map(|vault| vault.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
            found.set_xalign(0.);
            found.set_wrap(true);
            found.add_css_class("subtitle");
            panel.append(&found);
        }
        let vault_path = gtk::Entry::builder()
            .placeholder_text("Obsidian vault folder")
            .build();
        if let Some(path) = self
            .store
            .borrow()
            .obsidian_vault
            .as_ref()
            .or_else(|| vaults.first().map(|vault| &vault.path))
        {
            vault_path.set_text(&path.to_string_lossy());
        }
        panel.append(&vault_path);
        let vault_status = gtk::Label::new(None);
        vault_status.set_wrap(true);
        vault_status.set_xalign(0.);
        panel.append(&vault_status);
        let vault_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let choose_vault = gtk::Button::with_label("Choose folder");
        let connect_vault = gtk::Button::with_label("Connect Obsidian");
        let disconnect_vault = gtk::Button::with_label("Use local notes");
        vault_buttons.append(&choose_vault);
        vault_buttons.append(&connect_vault);
        vault_buttons.append(&disconnect_vault);
        panel.append(&vault_buttons);
        let entry = vault_path.clone();
        let parent = dialog.downgrade();
        choose_vault.connect_clicked(move |_| {
            let chooser = gtk::FileDialog::builder()
                .title("Choose an Obsidian vault folder")
                .build();
            let entry = entry.clone();
            chooser.select_folder(
                parent.upgrade().as_ref(),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Ok(folder) = result
                        && let Some(path) = folder.path()
                    {
                        entry.set_text(&path.to_string_lossy());
                    }
                },
            );
        });
        let weak = Rc::downgrade(self);
        let status = vault_status.clone();
        let label = destination.clone();
        connect_vault.connect_clicked(move |_| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if !ui.can_save {
                status.set_text("Saved data is unreadable; cannot change the connection");
                return;
            }
            let path =
                match quicknote::validate_vault(std::path::Path::new(vault_path.text().trim())) {
                    Ok(path) => path,
                    Err(error) => {
                        status.set_text(&error);
                        return;
                    }
                };
            let mut store = ui.store.borrow_mut();
            let old = (store.obsidian_vault.clone(), store.obsidian_disabled);
            store.obsidian_vault = Some(path);
            store.obsidian_disabled = false;
            if let Err(error) = store.save() {
                store.obsidian_vault = old.0;
                store.obsidian_disabled = old.1;
                status.set_text(&error);
                return;
            }
            drop(store);
            label.set_text(&format!("Quick notes: {}", ui.note_destination()));
            status.set_text(
                "Connected · quick notes will save in Spotlight Quick Notes inside your vault",
            );
            ui.refresh();
        });
        let weak = Rc::downgrade(self);
        let status = vault_status;
        let label = destination;
        disconnect_vault.connect_clicked(move |_| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if !ui.can_save {
                status.set_text("Saved data is unreadable; cannot change the connection");
                return;
            }
            let mut store = ui.store.borrow_mut();
            let old = (store.obsidian_vault.take(), store.obsidian_disabled);
            store.obsidian_disabled = true;
            if let Err(error) = store.save() {
                store.obsidian_vault = old.0;
                store.obsidian_disabled = old.1;
                status.set_text(&error);
                return;
            }
            drop(store);
            label.set_text(&format!("Quick notes: {}", ui.note_destination()));
            status.set_text("Future notes will save locally; existing vault notes are kept");
            ui.refresh();
        });
        let folder = gtk::Button::with_label("Open scripts folder");
        let ui = Rc::clone(self);
        folder.connect_clicked(move |_| {
            let p = core::data_dir().join("scripts");
            if let Err(e) = std::fs::create_dir_all(&p) {
                ui.error(&e.to_string());
                return;
            }
            let _ = gio::AppInfo::launch_default_for_uri(
                &gio::File::for_path(p).uri(),
                None::<&gio::AppLaunchContext>,
            );
        });
        panel.append(&folder);
        let refresh = gtk::Button::with_label("Refresh apps and file index");
        let ui = Rc::clone(self);
        refresh.connect_clicked(move |_| {
            *ui.apps.borrow_mut() = applications();
            ui.files.borrow_mut().clear();
            *ui.files_ready.borrow_mut() = false;
            ui.refresh();
            ui.error("Application list refreshed. Reopen file search to rebuild the index.");
        });
        panel.append(&refresh);
        let close = gtk::Button::with_label("Done");
        let d = dialog.clone();
        close.connect_clicked(move |_| d.close());
        panel.append(&close);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&panel)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        dialog.set_child(Some(&scroll));
        dialog.present();
    }
    fn actions(self: &Rc<Self>, button: &gtk::Button) {
        let popup = gtk::Popover::new();
        popup.set_parent(button);
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 6);
        for (name, action) in [
            ("Open / run                     ↵", 0),
            ("Toggle favorite              Ctrl+F", 1),
            ("Copy selected text", 2),
            ("Edit item                          Ctrl+E", 3),
            ("Delete item                      Ctrl+D", 4),
            ("Settings", 5),
        ] {
            let b = gtk::Button::with_label(name);
            let ui = Rc::clone(self);
            let pop = popup.clone();
            b.connect_clicked(move |_| {
                pop.popdown();
                match action {
                    0 => ui.activate(),
                    1 => ui.favorite(),
                    2 => {
                        if let Some(i) = ui.selected() {
                            let text = match i.action {
                                Action::Copy(s) | Action::Open(s) => s,
                                _ => i.title,
                            };
                            ui.window.clipboard().set_text(&text);
                            ui.error("Copied to clipboard");
                        }
                    }
                    3 => ui.edit_selected(),
                    4 => ui.delete_selected(),
                    _ => ui.settings(),
                }
            });
            menu.append(&b);
        }
        popup.set_child(Some(&menu));
        popup.connect_closed(|p| p.unparent());
        popup.popup();
    }
    fn run_script(self: &Rc<Self>, path: PathBuf) {
        self.error("Running script…");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            use std::io::Read;
            let result = (|| -> Result<String, String> {
                let mut child = Command::new(path)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                let mut pipe = child.stdout.take().ok_or("Missing stdout")?.take(65536);
                pipe.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                drop(pipe);
                let truncated = bytes.len() == 65536;
                if truncated {
                    let _ = child.kill();
                }
                let status = child.wait().map_err(|e| e.to_string())?;
                Ok(format!(
                    "{}\n\n{}",
                    String::from_utf8_lossy(&bytes),
                    if truncated {
                        "Output capped at 64 KiB; script stopped.".to_string()
                    } else {
                        format!("Exit status: {status}")
                    }
                ))
            })();
            let _ = tx.send(result);
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(r) => {
                    ui.error("");
                    match r {
                        Ok(s) => {
                            let d = gtk::Window::builder()
                                .title("Script output")
                                .transient_for(&ui.window)
                                .default_width(650)
                                .default_height(400)
                                .build();
                            let text = gtk::TextView::new();
                            text.set_editable(false);
                            text.set_monospace(true);
                            text.set_wrap_mode(gtk::WrapMode::WordChar);
                            text.buffer().set_text(&s);
                            d.set_child(Some(&gtk::ScrolledWindow::builder().child(&text).build()));
                            d.present();
                        }
                        Err(e) => ui.error(&e),
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });
    }
}
fn applications() -> Vec<Item> {
    gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .map(|a| {
            let name = a.display_name().to_string();
            let id = a.id().map(|s| s.to_string()).unwrap_or(name.clone());
            let mut i = item(
                &id,
                &name,
                "",
                "application-x-executable-symbolic",
                "Application",
                Action::App(a.clone()),
            );
            i.gicon = a.icon();
            i
        })
        .collect()
}
fn window_items() -> Vec<Item> {
    [
        (
            "Left Half",
            "Window Quick Tile Left",
            "Tile to the left half",
        ),
        (
            "Right Half",
            "Window Quick Tile Right",
            "Tile to the right half",
        ),
        ("Top Half", "Window Quick Tile Top", "Tile to the top half"),
        (
            "Bottom Half",
            "Window Quick Tile Bottom",
            "Tile to the bottom half",
        ),
        (
            "Top Left Quarter",
            "Window Quick Tile Top Left",
            "Tile to the top left corner",
        ),
        (
            "Top Right Quarter",
            "Window Quick Tile Top Right",
            "Tile to the top right corner",
        ),
        (
            "Bottom Left Quarter",
            "Window Quick Tile Bottom Left",
            "Tile to the bottom left corner",
        ),
        (
            "Bottom Right Quarter",
            "Window Quick Tile Bottom Right",
            "Tile to the bottom right corner",
        ),
        ("Maximize", "Window Maximize", "Toggle maximized size"),
        (
            "Restore",
            "Window Restore",
            "Restore the window's previous size",
        ),
        (
            "Minimize",
            "Window Minimize",
            "Hide the window in the taskbar",
        ),
        (
            "Next Screen",
            "Window to Next Screen",
            "Move to the next monitor",
        ),
    ]
    .into_iter()
    .map(|(title, shortcut, description)| {
        item(
            &format!("window:{shortcut}"),
            title,
            description,
            "view-grid-symbolic",
            "Window",
            Action::Window(shortcut),
        )
    })
    .collect()
}

fn emoji_items() -> Vec<Item> {
    emoji::catalog()
        .iter()
        .map(|e| {
            let mut entry = item(
                &format!("emoji:{}", e.symbol),
                e.name,
                &format!("{} · Enter to copy", e.category),
                e.symbol,
                "Emoji",
                Action::Copy(e.symbol.to_string()),
            );
            entry.keywords = e.keywords.clone();
            entry
        })
        .collect()
}

fn build(app: &gtk::Application) -> Rc<Ui> {
    let css = gtk::CssProvider::new();
    css.load_from_data(include_str!("../assets/style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("Display"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Spotlight")
        .default_width(760)
        .default_height(530)
        .decorated(false)
        .resizable(false)
        .build();
    let font_css = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("Display"),
        &font_css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    let mut installed_fonts: Vec<String> = window
        .pango_context()
        .list_families()
        .into_iter()
        .map(|family| family.name().to_string())
        .collect();
    installed_fonts.sort_by_key(|name| name.to_lowercase());
    installed_fonts.dedup();
    let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shell.add_css_class("shell");
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    top.add_css_class("top");
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("flat");
    back.set_visible(false);
    top.append(&back);
    let brand = gtk::Label::new(Some("╱"));
    brand.add_css_class("brand");
    top.append(&brand);
    let search = gtk::Entry::builder()
        .placeholder_text(Page::Root.placeholder())
        .hexpand(true)
        .build();
    search.add_css_class("search");
    top.append(&search);
    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some("Create item · Ctrl+N"));
    add.set_visible(false);
    top.append(&add);
    let windows = gtk::Button::from_icon_name("view-grid-symbolic");
    windows.add_css_class("flat");
    windows.set_tooltip_text(Some("Window Manager"));
    top.append(&windows);
    let settings = gtk::Button::from_icon_name("emblem-system-symbolic");
    settings.add_css_class("flat");
    settings.set_tooltip_text(Some("Settings"));
    top.append(&settings);
    shell.append(&top);
    let section = gtk::Label::new(Some("Suggestions"));
    section.set_xalign(0.);
    section.add_css_class("section");
    shell.append(&section);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.set_activate_on_single_click(false);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .min_content_height(365)
        .build();
    shell.append(&scroll);
    let notice = gtk::Label::new(None);
    notice.set_xalign(0.);
    notice.set_wrap(true);
    notice.add_css_class("notice");
    notice.set_visible(false);
    shell.append(&notice);
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("footer");
    let label = gtk::Label::new(Some("╱  Spotlight"));
    footer.append(&label);
    let hint = gtk::Label::new(Some("↑ ↓ Navigate"));
    hint.set_hexpand(true);
    hint.set_xalign(0.);
    hint.set_margin_start(16);
    footer.append(&hint);
    let open = gtk::Button::with_label("Open  ↵");
    open.add_css_class("flat");
    footer.append(&open);
    let actions = gtk::Button::with_label("Actions  ⌃K");
    actions.add_css_class("flat");
    footer.append(&actions);
    shell.append(&footer);
    window.set_child(Some(&shell));
    let (mut store, load_error) = match Store::load() {
        Ok(s) => (s, None),
        Err(e) => (Store::default(), Some(e)),
    };
    if load_error.is_none()
        && store.obsidian_vault.is_none()
        && !store.obsidian_disabled
        && !std::env::args().any(|arg| arg == "--smoke-test")
    {
        let vaults = quicknote::discover();
        if vaults.len() == 1 {
            store.obsidian_vault = Some(vaults[0].path.clone());
            if let Err(error) = store.save() {
                eprintln!("Could not save the Obsidian connection: {error}");
            }
        }
    }
    if let Some(family) = store
        .selected_font
        .as_ref()
        .filter(|family| installed_fonts.contains(family))
    {
        font_css.load_from_data(&fonts::css(family));
    }
    let ui = Rc::new(Ui {
        window,
        search,
        list,
        section,
        notice,
        hint,
        add,
        back,
        store: RefCell::new(store),
        can_save: load_error.is_none(),
        page: RefCell::new(Page::Root),
        apps: RefCell::new(applications()),
        fonts: installed_fonts,
        font_css,
        visible: RefCell::new(Vec::new()),
        files: RefCell::new(Vec::new()),
        file_loading: RefCell::new(false),
        files_ready: RefCell::new(false),
        clips: RefCell::new(Vec::new()),
        result_limit: Cell::new(60),
        currency: RefCell::new(currency::State::default()),
        picker: RefCell::new(None),
        picking_color: Cell::new(false),
        picked_color: RefCell::new(None),
        reminder_clock: RefCell::new(None),
        note_editor: RefCell::new(None),
        note_draft: RefCell::new(String::new()),
        alarm_window: RefCell::new(None),
        alarm_repeat: RefCell::new(None),
        alarm_label: RefCell::new(None),
    });
    if let Some(e) = load_error {
        ui.error(&format!(
            "Saved data could not be read: {e}. See data.json before saving new items."
        ));
    }
    let u = Rc::clone(&ui);
    ui.search.connect_changed(move |_| {
        u.result_limit.set(60);
        u.refresh();
    });
    let u = Rc::clone(&ui);
    ui.search.connect_activate(move |_| u.activate());
    let u = Rc::clone(&ui);
    ui.list.connect_row_activated(move |_, row| {
        let selected = u.visible.borrow().get(row.index() as usize).cloned();
        if let Some(i) = selected {
            u.launch(i);
        }
    });
    let u = Rc::clone(&ui);
    ui.back.connect_clicked(move |_| u.page(Page::Root));
    let u = Rc::clone(&ui);
    ui.add
        .connect_clicked(move |_| u.editor(*u.page.borrow(), None));
    let u = Rc::clone(&ui);
    settings.connect_clicked(move |_| u.settings());
    let u = Rc::clone(&ui);
    windows.connect_clicked(move |_| u.page(Page::Windows));
    let u = Rc::clone(&ui);
    open.connect_clicked(move |_| u.activate());
    let u = Rc::clone(&ui);
    actions.connect_clicked(move |b| u.actions(b));
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let u = Rc::clone(&ui);
    let action_button = actions.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        if key == gdk::Key::Escape {
            if *u.page.borrow() != Page::Root {
                u.page(Page::Root);
            } else {
                u.window.set_visible(false);
            }
            return glib::Propagation::Stop;
        }
        if ctrl {
            match key {
                gdk::Key::Return | gdk::Key::KP_Enter if *u.page.borrow() == Page::Root => {
                    let web = u
                        .visible
                        .borrow()
                        .iter()
                        .find(|entry| entry.id == "web")
                        .cloned();
                    if let Some(web) = web {
                        u.launch(web);
                    }
                }
                gdk::Key::k | gdk::Key::K => u.actions(&action_button),
                gdk::Key::f | gdk::Key::F => u.favorite(),
                gdk::Key::n | gdk::Key::N => {
                    let p = *u.page.borrow();
                    if matches!(p, Page::Snippets | Page::Links | Page::Notes) {
                        u.editor(p, None);
                    }
                }
                gdk::Key::e | gdk::Key::E => u.edit_selected(),
                gdk::Key::d | gdk::Key::D => u.delete_selected(),
                gdk::Key::comma => u.settings(),
                _ => return glib::Propagation::Proceed,
            };
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Down || key == gdk::Key::Up {
            let total = u.visible.borrow().len() as i32;
            if total > 0 {
                let at = u.list.selected_row().map(|r| r.index()).unwrap_or(0);
                let next = if key == gdk::Key::Down {
                    (at + 1).min(total - 1)
                } else {
                    (at - 1).max(0)
                };
                if let Some(row) = u.list.row_at_index(next) {
                    u.list.select_row(Some(&row));
                    row.grab_focus();
                    u.search.grab_focus();
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    ui.window.add_controller(keys);
    ui.window.connect_close_request(|w| {
        w.set_visible(false);
        glib::Propagation::Stop
    });
    ui.refresh();
    ui.window.present();
    ui.search.grab_focus();
    if !std::env::args().any(|a| a == "--smoke-test") {
        ui.arm_reminders();
    }
    ui
}
fn smoke_currency(ui: Rc<Ui>) {
    ui.search.set_text("100 USD to AUD");
    assert_eq!(ui.visible.borrow()[0].title, "Fetching exchange rate…");
    let started = std::time::Instant::now();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let pair = ("USD".into(), "AUD".into());
        if let Some((_, result)) = ui.currency.borrow().cache.get(&pair) {
            let rate = result.as_ref().expect("Live currency API request");
            let query = currency::parse("100 USD to AUD").unwrap();
            assert_eq!(
                ui.visible.borrow()[0].title,
                currency::convert(&query, rate.rate).unwrap()
            );
            assert!(ui.visible.borrow()[0].subtitle.contains(&rate.date));
            println!(
                "Live currency UI passed: {} · rate date {}",
                ui.visible.borrow()[0].title,
                rate.date
            );
            if std::env::args().any(|a| a == "--currency-screenshot") {
                capture_smoke(Rc::clone(&ui));
            } else {
                smoke(Rc::clone(&ui));
            }
            return glib::ControlFlow::Break;
        }
        assert!(
            started.elapsed().as_secs() < 16,
            "Live currency request timed out"
        );
        glib::ControlFlow::Continue
    });
}
fn play_alarm_sound() {
    std::thread::spawn(|| {
        let played = Command::new("paplay")
            .arg("/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga")
            .status()
            .is_ok_and(|status| status.success());
        if !played {
            let result = Command::new("canberra-gtk-play")
                .args([
                    "--id",
                    "alarm-clock-elapsed",
                    "--description",
                    "Spotlight alarm",
                ])
                .status();
            if !result.is_ok_and(|status| status.success()) {
                eprintln!("Spotlight could not play the alarm sound");
            }
        }
    });
}

fn test_widget(
    widget: &gtk::Widget,
    predicate: &dyn Fn(&gtk::Widget) -> bool,
) -> Option<gtk::Widget> {
    if predicate(widget) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(found) = test_widget(&widget, predicate) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}
fn smoke_notes(ui: Rc<Ui>) {
    glib::timeout_add_local_once(Duration::from_millis(500), move || {
        assert!(
            ui.store.borrow().quick_notes.is_empty(),
            "Use an isolated XDG_DATA_HOME for note tests"
        );
        let vault = core::data_dir().join("Test vault");
        std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
        ui.store.borrow_mut().obsidian_vault = Some(vault.clone());
        ui.store.borrow().save().unwrap();
        ui.search.set_text("note One-line capture café 🎉");
        assert!(matches!(
            ui.visible.borrow()[0].action,
            Action::CaptureNote(_)
        ));
        ui.activate();
        let saved = Store::load().unwrap();
        assert_eq!(saved.quick_notes.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&saved.quick_notes[0].path).unwrap(),
            "One-line capture café 🎉\n"
        );
        assert!(
            saved.quick_notes[0]
                .path
                .starts_with(vault.canonicalize().unwrap())
        );
        ui.search.set_text("One-line capture");
        assert!(ui.visible.borrow().iter().any(|entry| matches!(&entry.action, Action::Open(uri) if uri.starts_with("obsidian://open?path="))), "Saved notes open in Obsidian");
        ui.window
            .application()
            .unwrap()
            .activate_action("quick-note", None);
        let dialog = ui.note_editor.borrow().as_ref().unwrap().clone();
        let text: gtk::TextView =
            test_widget(dialog.upcast_ref(), &|widget| widget.is::<gtk::TextView>())
                .unwrap()
                .downcast()
                .unwrap();
        let body = "Multiline capture\n\n    indented code\n- a list\n";
        text.buffer().set_text(body);
        dialog.close();
        assert!(ui.note_editor.borrow().is_none());
        ui.quick_note("");
        let dialog = ui.note_editor.borrow().as_ref().unwrap().clone();
        let text: gtk::TextView =
            test_widget(dialog.upcast_ref(), &|widget| widget.is::<gtk::TextView>())
                .unwrap()
                .downcast()
                .unwrap();
        assert_eq!(
            text.buffer()
                .text(
                    &text.buffer().start_iter(),
                    &text.buffer().end_iter(),
                    false
                )
                .as_str(),
            body,
            "Draft survives closing the note box"
        );
        let save: gtk::Button = test_widget(dialog.upcast_ref(), &|widget| {
            widget
                .clone()
                .downcast::<gtk::Button>()
                .is_ok_and(|button| button.label().as_deref() == Some("Save Note"))
        })
        .unwrap()
        .downcast()
        .unwrap();
        save.emit_clicked();
        assert!(ui.note_editor.borrow().is_none());
        let saved = Store::load().unwrap();
        assert_eq!(saved.quick_notes.len(), 2);
        assert_eq!(
            std::fs::read_to_string(&saved.quick_notes[0].path).unwrap(),
            body
        );
        assert!(ui.note_draft.borrow().is_empty());
        assert_eq!(saved.obsidian_vault, Some(vault));
        println!(
            "Quick-note UI passed: search capture, Markdown content, vault connection, recent notes, Obsidian URI, multiline editor, draft retention, saved state"
        );
        ui.search.set_text("");
        smoke(Rc::clone(&ui));
    });
}
fn smoke_alarm(ui: Rc<Ui>) {
    assert!(
        ui.store.borrow().reminders.is_empty() && ui.store.borrow().ringing_reminders.is_empty(),
        "Use isolated empty data for alarm tests"
    );
    ui.search.set_text("alarm");
    assert!(
        matches!(ui.visible.borrow()[0].action, Action::AlarmSetup),
        "Bare alarm opens setup"
    );
    ui.activate();
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        let dialog = ui
            .window
            .application()
            .unwrap()
            .windows()
            .into_iter()
            .find(|window| window.title().as_deref() == Some("Set Alarm"))
            .expect("Alarm setup window");
        let set: gtk::Button = test_widget(dialog.upcast_ref(), &|widget| {
            widget
                .clone()
                .downcast::<gtk::Button>()
                .is_ok_and(|button| button.label().as_deref() == Some("Set Alarm"))
        })
        .unwrap()
        .downcast()
        .unwrap();
        set.emit_clicked();
        let alarm = Store::load().unwrap().reminders[0].clone();
        assert!(alarm.alarm);
        let now = glib::DateTime::now_local().unwrap().to_unix();
        assert!(
            alarm.due > now && alarm.due <= now + 60,
            "Next-minute local alarm"
        );
        println!(
            "Real clock alarm set; waiting {} seconds for expiry",
            alarm.due - now
        );
        ui.window.set_visible(false);
        let id = alarm.id.clone();
        let started = std::time::Instant::now();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            assert!(started.elapsed().as_secs() < 70, "Alarm did not fire");
            if ui
                .store
                .borrow()
                .ringing_reminders
                .iter()
                .any(|entry| entry.id == id)
            {
                assert!(
                    ui.alarm_window.borrow().as_ref().unwrap().is_visible(),
                    "Alarm alert visible"
                );
                assert!(
                    ui.alarm_repeat.borrow().is_some(),
                    "Repeating alarm sound scheduled"
                );
                assert!(
                    Store::load()
                        .unwrap()
                        .ringing_reminders
                        .iter()
                        .any(|entry| entry.id == id),
                    "Ringing state survives restart"
                );
                let window = ui.alarm_window.borrow().as_ref().unwrap().clone();
                let dismiss: gtk::Button = test_widget(window.upcast_ref(), &|widget| {
                    widget
                        .clone()
                        .downcast::<gtk::Button>()
                        .is_ok_and(|button| button.label().as_deref() == Some("Dismiss Alarm"))
                })
                .unwrap()
                .downcast()
                .unwrap();
                dismiss.emit_clicked();
                assert!(ui.alarm_window.borrow().is_none());
                assert!(ui.alarm_repeat.borrow().is_none());
                assert!(Store::load().unwrap().ringing_reminders.is_empty());
                println!(
                    "Alarm UI passed: setup form, real clock deadline, visible alert, repeating sound source, persisted ringing state, dismissal"
                );
                ui.window.application().unwrap().quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    });
}
fn smoke_colors(ui: Rc<Ui>) {
    ui.page(Page::Colors);
    assert!(
        matches!(ui.visible.borrow()[0].action, Action::PickColor),
        "Empty converter offers color picking"
    );
    ui.finish_color(Ok(Some("#0080FF".into())));
    assert!(*ui.page.borrow() == Page::Colors);
    assert_eq!(ui.visible.borrow().len(), 7, "All color formats offered");
    ui.search.set_text("rgba");
    let expected = "rgba(0, 128, 255, 1)";
    let index = ui
        .visible
        .borrow()
        .iter()
        .position(|entry| matches!(&entry.action, Action::Copy(value) if value == expected))
        .unwrap();
    ui.list
        .select_row(ui.list.row_at_index(index as i32).as_ref());
    ui.activate();
    ui.window.clipboard().read_text_async(None::<&gio::Cancellable>, move |result| {
        assert_eq!(result.unwrap().as_deref(), Some("rgba(0, 128, 255, 1)"), "Chosen format reaches the clipboard");
        ui.page(Page::Root); ui.search.set_text("hsl");
        assert!(ui.visible.borrow().iter().any(|entry| matches!(&entry.action, Action::Copy(value) if value == "hsl(209.882, 100%, 50%)")), "HSL searchable from main bar");
        ui.finish_color(Ok(None));
        assert_eq!(ui.picked_color.borrow().as_deref(), Some("#0080FF"), "Cancellation keeps the selected color");
        ui.search.set_text("color formats");
        assert!(matches!(ui.visible.borrow()[0].action, Action::Page(Page::Colors)));
        ui.activate();
        assert_eq!(ui.visible.borrow().len(), 7, "Formats can be reopened");
        println!("Color format UI passed: picker handoff, seven formats, filtering, actual RGBA clipboard contents, main search, cancellation, reopening");
        ui.window.application().unwrap().quit();
    });
}
fn smoke_fonts(ui: Rc<Ui>) {
    assert!(!ui.fonts.is_empty(), "Installed fonts discovered");
    assert!(
        ui.store.borrow().selected_font.is_none(),
        "Use isolated data for font tests"
    );
    ui.search.set_text("fonts");
    assert!(
        matches!(ui.visible.borrow()[0].action, Action::Page(Page::Fonts)),
        "Fonts command is discoverable"
    );
    ui.activate();
    assert_eq!(*ui.page.borrow() as u8, Page::Fonts as u8);
    if ui.fonts.len() > 60 {
        assert_eq!(ui.visible.borrow().len(), 61);
        ui.list.select_row(ui.list.row_at_index(60).as_ref());
        ui.activate();
        assert!(
            ui.visible.borrow().len() > 61,
            "Browse more installed fonts"
        );
    }
    let family = ui
        .fonts
        .iter()
        .find(|name| name.eq_ignore_ascii_case("DejaVu Sans Mono"))
        .or_else(|| {
            ui.fonts
                .iter()
                .find(|name| name.eq_ignore_ascii_case("Monospace"))
        })
        .unwrap_or(&ui.fonts[0])
        .clone();
    ui.page(Page::Root);
    ui.search.set_text(&format!("font {family}"));
    assert!(matches!(&ui.visible.borrow()[0].action, Action::FontPreview(name) if name == &family));
    ui.activate();
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        let title = format!("Font Preview · {family}");
        let dialog = ui
            .window
            .application()
            .unwrap()
            .windows()
            .into_iter()
            .find(|window| window.title().as_deref() == Some(title.as_str()))
            .unwrap();
        let input: gtk::Entry =
            test_widget(dialog.upcast_ref(), &|widget| widget.is::<gtk::Entry>())
                .unwrap()
                .downcast()
                .unwrap();
        input.set_text("Custom font preview 123");
        assert!(
            test_widget(dialog.upcast_ref(), &|widget| widget
                .clone()
                .downcast::<gtk::Label>()
                .is_ok_and(|label| label.text() == "Custom font preview 123"))
            .is_some(),
            "Editable preview"
        );
        let button = |label: &str| -> gtk::Button {
            test_widget(dialog.upcast_ref(), &|widget| {
                widget
                    .clone()
                    .downcast::<gtk::Button>()
                    .is_ok_and(|button| button.label().as_deref() == Some(label))
            })
            .unwrap()
            .downcast()
            .unwrap()
        };
        button("Copy Font Name").emit_clicked();
        assert!(
            ui.store.borrow().selected_font.is_none(),
            "Copying does not change the font"
        );
        button("Use in Spotlight").emit_clicked();
        assert_eq!(
            Store::load().unwrap().selected_font.as_deref(),
            Some(family.as_str()),
            "Selected font saved"
        );
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            let applied = ui
                .search
                .pango_context()
                .font_description()
                .unwrap()
                .family()
                .unwrap()
                .to_string();
            assert!(
                applied.contains(&family),
                "Actual rendered font family: {applied}"
            );
            dialog.close();
            ui.page(Page::Root);
            ui.search.set_text("reset spotlight font");
            assert!(matches!(ui.visible.borrow()[0].action, Action::ResetFont));
            ui.activate();
            assert!(
                Store::load().unwrap().selected_font.is_none(),
                "Default font restored and saved"
            );
            println!(
                "Font UI passed: discovery, pagination, main search, editable preview, copy, apply, actual rendering, saved preference, reset"
            );
            ui.window.application().unwrap().quit();
        });
    });
}
fn smoke_timer_setup(ui: Rc<Ui>) {
    assert!(
        ui.store.borrow().reminders.is_empty() && ui.store.borrow().ringing_reminders.is_empty(),
        "Use isolated empty data for timer setup tests"
    );
    ui.search.set_text("set timer for 5 minutes");
    assert!(matches!(
        ui.visible.borrow()[0].action,
        Action::Schedule(reminder::Draft::Timer(300))
    ));
    ui.activate();
    assert_eq!(Store::load().unwrap().reminders.len(), 1);
    ui.search.set_text("cancel timer");
    ui.activate();
    assert!(Store::load().unwrap().reminders.is_empty());
    for query in ["timer", "set timer", "set a timer"] {
        ui.search.set_text(query);
        assert!(
            matches!(ui.visible.borrow()[0].action, Action::TimerSetup),
            "Timer setup command: {query}"
        );
    }
    ui.activate();
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        let dialog = ui
            .window
            .application()
            .unwrap()
            .windows()
            .into_iter()
            .find(|window| window.title().as_deref() == Some("Set Timer"))
            .unwrap();
        let spinner = |name: &str| -> gtk::SpinButton {
            test_widget(dialog.upcast_ref(), &|widget| widget.widget_name() == name)
                .unwrap()
                .downcast()
                .unwrap()
        };
        let hours = spinner("timer-hours");
        let minutes = spinner("timer-minutes");
        let seconds = spinner("timer-seconds");
        let start: gtk::Button = test_widget(dialog.upcast_ref(), &|widget| {
            widget
                .clone()
                .downcast::<gtk::Button>()
                .is_ok_and(|button| button.label().as_deref() == Some("Start Timer"))
        })
        .unwrap()
        .downcast()
        .unwrap();
        hours.set_text("0");
        minutes.set_text("0");
        seconds.set_text("0");
        start.emit_clicked();
        assert!(
            ui.store.borrow().reminders.is_empty(),
            "Rejects zero duration"
        );
        assert!(dialog.is_visible());
        seconds.set_text("2");
        start.emit_clicked();
        let saved = Store::load().unwrap();
        assert_eq!(saved.reminders.len(), 1);
        assert!(!saved.reminders[0].alarm);
        assert_eq!(
            saved.reminders[0].title, "Timer · 2s",
            "Uses typed spin-button values"
        );
        ui.window.set_visible(false);
        glib::timeout_add_local_once(Duration::from_secs(4), move || {
            assert!(ui.store.borrow().reminders.is_empty());
            assert_eq!(Store::load().unwrap().ringing_reminders.len(), 1);
            assert!(
                ui.alarm_window.borrow().as_ref().unwrap().is_visible(),
                "Finished timer shows its alert"
            );
            assert!(ui.alarm_repeat.borrow().is_some());
            ui.dismiss_alarms();
            assert!(Store::load().unwrap().ringing_reminders.is_empty());
            assert!(ui.alarm_repeat.borrow().is_none());
            println!(
                "Timer setup UI passed: command aliases, text timers, cancellation, duration picker, zero-duration validation, typed values, hidden expiry, alert, dismissal"
            );
            ui.window.application().unwrap().quit();
        });
    });
}
fn smoke_reminders(ui: Rc<Ui>) {
    assert!(
        ui.store.borrow().reminders.is_empty(),
        "Use an isolated XDG_DATA_HOME for reminder tests"
    );
    ui.search.set_text("timer 2 seconds");
    assert!(matches!(
        ui.visible.borrow()[0].action,
        Action::Schedule(reminder::Draft::Timer(2))
    ));
    ui.activate();
    assert_eq!(Store::load().unwrap().reminders.len(), 1, "Timer saved");
    ui.window.set_visible(false);
    glib::timeout_add_local_once(Duration::from_secs(4), move || {
        assert!(
            ui.store.borrow().reminders.is_empty(),
            "Timer fired while hidden"
        );
        assert!(
            Store::load().unwrap().reminders.is_empty(),
            "Completed timer removed from saved state"
        );
        assert!(
            ui.reminder_clock.borrow().is_none(),
            "No idle timer polling"
        );
        assert_eq!(
            Store::load().unwrap().ringing_reminders.len(),
            1,
            "Unacknowledged alert saved"
        );
        assert!(
            ui.alarm_window.borrow().as_ref().unwrap().is_visible(),
            "Visible timer alert"
        );
        ui.dismiss_alarms();
        assert!(Store::load().unwrap().ringing_reminders.is_empty());
        assert!(ui.alarm_repeat.borrow().is_none());
        ui.window.present();
        ui.search.set_text("alarm 23:59");
        assert!(matches!(
            ui.visible.borrow()[0].action,
            Action::Schedule(reminder::Draft::Alarm {
                hour: 23,
                minute: 59
            })
        ));
        ui.activate();
        let stored = Store::load().unwrap();
        assert_eq!(stored.reminders.len(), 1, "Alarm saved");
        assert!(stored.reminders[0].alarm);
        ui.search.set_text("cancel alarm");
        assert!(matches!(
            ui.visible.borrow()[0].action,
            Action::CancelReminder(_)
        ));
        ui.activate();
        assert!(
            Store::load().unwrap().reminders.is_empty(),
            "Alarm cancellation saved"
        );
        println!(
            "Reminder UI passed: hidden timer expiry, notification, saved state, alarm creation and cancellation, idle scheduler cleanup"
        );
        smoke(Rc::clone(&ui));
    });
}
fn smoke(ui: Rc<Ui>) {
    glib::timeout_add_local_once(Duration::from_millis(500), move || {
        assert!(!ui.apps.borrow().is_empty(), "Application discovery");
        ui.search.set_text("coffee & café");
        let web = ui
            .visible
            .borrow()
            .iter()
            .find(|entry| entry.id == "web")
            .unwrap()
            .clone();
        assert!(web.title.starts_with("Search Google"));
        assert!(
            matches!(&web.action, Action::Open(uri) if uri == "https://www.google.com/search?q=coffee%20%26%20caf%C3%A9")
        );
        assert!(ui.hint.text().contains("Google"));
        for query in ["color picker", "colour", "eyedropper"] {
            ui.search.set_text(query);
            assert!(
                ui.visible
                    .borrow()
                    .iter()
                    .any(|entry| matches!(entry.action, Action::PickColor)),
                "Color picker search: {query}"
            );
        }
        ui.finish_color(Ok(Some("#0080FF".into())));
        ui.refresh();
        assert_eq!(ui.picked_color.borrow().as_deref(), Some("#0080FF"));
        assert!(
            *ui.page.borrow() == Page::Colors,
            "Picked color opens format choices"
        );
        assert_eq!(ui.visible.borrow().len(), 7);
        ui.search.set_text("hex");
        assert!(
            ui.visible
                .borrow()
                .iter()
                .any(|entry| matches!(&entry.action, Action::Copy(value) if value == "#0080FF")),
            "Picked hex available to copy"
        );
        ui.finish_color(Ok(None));
        assert_eq!(
            ui.picked_color.borrow().as_deref(),
            Some("#0080FF"),
            "Cancellation preserves the last color"
        );
        ui.page(Page::Root);
        ui.search.set_text("hsl");
        assert!(ui.visible.borrow().iter().any(|entry| matches!(&entry.action, Action::Copy(value) if value == "hsl(209.882, 100%, 50%)")), "Picked color formats searchable from main bar");
        ui.error("");
        ui.currency.borrow_mut().cache.insert(
            ("USD".into(), "AUD".into()),
            (
                std::time::Instant::now(),
                Ok(currency::Rate {
                    date: "2026-10-15".into(),
                    base: "USD".into(),
                    quote: "AUD".into(),
                    rate: 1.5,
                }),
            ),
        );
        ui.search.set_text("100 USD to AUD");
        assert_eq!(ui.visible.borrow()[0].title, "150.00 AUD");
        assert!(ui.visible.borrow()[0].subtitle.contains("2026-10-15"));
        ui.search.set_text("200 usd in aud");
        assert_eq!(
            ui.visible.borrow()[0].title,
            "300.00 AUD",
            "Reuses rate for a new amount"
        );
        ui.currency.borrow_mut().cache.insert(
            ("AAA".into(), "BBB".into()),
            (
                std::time::Instant::now(),
                Err("Test connection failure".into()),
            ),
        );
        ui.search.set_text("100 AAA to BBB");
        assert_eq!(
            ui.visible.borrow()[0].title,
            "Currency conversion unavailable"
        );
        assert!(matches!(ui.visible.borrow()[0].action, Action::Noop));
        ui.search.set_text("2+3*4");
        assert_eq!(ui.visible.borrow()[0].title, "14");
        ui.search.set_text("10 km to mi");
        assert!(ui.visible.borrow()[0].title.starts_with("6.2137"));
        for (query, expected) in [
            ("smile", "😀"),
            ("penguin", "🐧"),
            ("flag: Australia", "🇦🇺"),
            ("thumbs up: dark skin tone", "👍🏿"),
            ("woman technologist", "👩‍💻"),
            ("coffee", "☕"),
        ] {
            ui.search.set_text(query);
            assert!(
                ui.visible
                    .borrow()
                    .iter()
                    .any(|entry| matches!(&entry.action, Action::Copy(value) if value == expected)),
                "Main search finds emoji: {query}"
            );
        }
        ui.search.set_text("grinning face");
        let emoji_index = ui
            .visible
            .borrow()
            .iter()
            .position(|entry| matches!(&entry.action, Action::Copy(value) if value == "😀"))
            .unwrap();
        ui.list
            .select_row(ui.list.row_at_index(emoji_index as i32).as_ref());
        ui.activate();
        assert_eq!(ui.notice.text(), "Copied to clipboard");
        let previous_usage = ui.store.borrow().usage.clone();
        let previous_favorites = ui.store.borrow().favorites.clone();
        ui.store.borrow_mut().usage.clear();
        ui.store.borrow_mut().favorites.clear();
        ui.store.borrow_mut().usage.insert("emoji:👍".into(), 5);
        ui.page(Page::Emoji);
        assert!(
            matches!(&ui.visible.borrow()[0].action, Action::Copy(value) if value == "👍"),
            "Most-used emoji first"
        );
        ui.store.borrow_mut().favorites.push("emoji:❤️".into());
        ui.refresh();
        assert!(
            matches!(&ui.visible.borrow()[0].action, Action::Copy(value) if value == "❤️"),
            "Favorite emoji first"
        );
        ui.store.borrow_mut().usage = previous_usage;
        ui.store.borrow_mut().favorites = previous_favorites;
        ui.refresh();
        assert_eq!(
            ui.visible.borrow().len(),
            61,
            "First emoji batch plus Show more"
        );
        assert!(
            ui.section
                .text()
                .contains(&emoji::catalog().len().to_string())
        );
        ui.list.select_row(ui.list.row_at_index(60).as_ref());
        ui.activate();
        assert_eq!(
            ui.visible.borrow().len(),
            emoji::catalog().len(),
            "Everyday selection fully browsable"
        );
        for (query, expected) in [
            ("penguin", "🐧"),
            ("flag: Australia", "🇦🇺"),
            ("thumbs up: dark skin tone", "👍🏿"),
            ("woman technologist", "👩‍💻"),
            ("red heart", "❤️"),
            ("coffee", "☕"),
        ] {
            ui.search.set_text(query);
            assert!(
                matches!(&ui.visible.borrow()[0].action, Action::Copy(value) if value == expected),
                "Emoji search: {query}"
            );
        }
        ui.search.set_text("eggplant");
        assert!(
            !ui.visible
                .borrow()
                .iter()
                .any(|entry| matches!(&entry.action, Action::Copy(value) if value == "🍆")),
            "Unwanted emoji excluded"
        );
        ui.search.set_text("");
        assert_eq!(
            ui.visible.borrow().len(),
            61,
            "Search resets browsing limit"
        );
        ui.page(Page::Windows);
        assert_eq!(ui.visible.borrow().len(), 12, "Window actions available");
        ui.search.set_text("left half");
        assert_eq!(ui.visible.borrow()[0].title, "Left Half");
        ui.page(Page::Files);
        ui.page(Page::Clipboard);
        let checked = Rc::clone(&ui);
        glib::timeout_add_local_once(Duration::from_millis(1000), move || {
            assert!(*checked.files_ready.borrow(), "File index completed");
            assert!(!checked.files.borrow().is_empty(), "Files discovered");
            assert!(
                checked.files.borrow().len() <= 25000,
                "File index stays bounded"
            );
            assert!(
                !checked.notice.is_visible(),
                "Klipper integration did not report an error"
            );
            checked.page(Page::Files);
            let name = checked.files.borrow()[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            checked.search.set_text(&name);
            assert_eq!(
                checked.visible.borrow()[0].title,
                name,
                "Exact file search wins"
            );
            checked.page(Page::Root);
            let indexed_path = checked.files.borrow()[0].clone();
            let indexed_uri = gio::File::for_path(&indexed_path).uri().to_string();
            checked.search.set_text(&name);
            assert!(
                checked
                    .visible
                    .borrow()
                    .iter()
                    .any(|entry| entry.badge == "File"
                        && matches!(&entry.action, Action::Open(uri) if uri == &indexed_uri)),
                "Main search finds indexed files with their open action"
            );
            checked.search.set_text(&indexed_path.to_string_lossy());
            assert!(
                checked
                    .visible
                    .borrow()
                    .iter()
                    .any(|entry| matches!(&entry.action, Action::Open(uri) if uri == &indexed_uri)),
                "Main search accepts file paths"
            );
            checked.search.set_text("");
            assert!(
                !checked
                    .visible
                    .borrow()
                    .iter()
                    .any(|entry| entry.badge == "File"),
                "Empty main search keeps suggestions uncluttered"
            );
            smoke_window_action(checked);
        });
    });
}
fn smoke_window_action(ui: Rc<Ui>) {
    let target = gtk::ApplicationWindow::builder()
        .application(&ui.window.application().unwrap())
        .title("Spotlight window action check")
        .default_width(320)
        .default_height(200)
        .build();
    target.present();
    glib::timeout_add_local_once(Duration::from_millis(400), move || {
        ui.window.present();
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            assert!(
                target.is_active() || ui.window.is_active(),
                "A test window has focus"
            );
            ui.window_action("Window Maximize");
            glib::timeout_add_local_once(Duration::from_millis(1000), move || {
                assert!(!ui.window.is_visible(), "Launcher hides for window action");
                assert!(target.is_maximized(), "KWin maximized the previous window");
                target.destroy();
                ui.window.present();
                capture_smoke(ui);
            });
        });
    });
}
fn capture_smoke(ui: Rc<Ui>) {
    let currency_screenshot = std::env::args().any(|a| a == "--currency-screenshot");
    if !currency_screenshot {
        ui.page(Page::Root);
        ui.search.set_text("");
    }
    let ui2 = Rc::clone(&ui);
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        let paint = gtk::WidgetPaintable::new(Some(&ui2.window));
        let snapshot = gtk::Snapshot::new();
        paint.snapshot(
            &snapshot,
            ui2.window.width() as f64,
            ui2.window.height() as f64,
        );
        if let Some(node) = snapshot.to_node()
            && let Some(renderer) = ui2.window.renderer()
        {
            let texture = renderer.render_texture(&node, None);
            texture
                .save_to_png(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    if currency_screenshot {
                        "currency-preview.png"
                    } else {
                        "preview.png"
                    },
                ))
                .expect("Save preview");
        }
        if currency_screenshot {
            println!("Saved currency-preview.png");
        } else {
            println!(
                "GUI smoke checks passed: app discovery, calculation, conversion, emoji search, bounded file index, file search, Klipper integration, window action search, KWin maximize, navigation, rendering."
            );
        }
        ui2.window.application().unwrap().quit();
    });
}
fn main() -> glib::ExitCode {
    if std::env::args().any(|a| a == "--install-shortcut") {
        return match shortcut::install() {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                glib::ExitCode::FAILURE
            }
        };
    }
    if let Some(action) = std::env::args().find_map(|a| match a.as_str() {
        "--quit" => Some("quit"),
        "--diagnostics" => Some("diagnostics"),
        "--quick-note" => Some("quick-note"),
        _ => None,
    }) {
        let result = (|| -> Result<(), glib::Error> {
            let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
            let args = (
                action,
                Vec::<glib::Variant>::new(),
                std::collections::HashMap::<String, glib::Variant>::new(),
            )
                .to_variant();
            bus.call_sync(
                Some("local.spotlight.Spotlight"),
                "/local/spotlight/Spotlight",
                "org.gtk.Actions",
                "Activate",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                2000,
                None::<&gio::Cancellable>,
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => return glib::ExitCode::SUCCESS,
            Err(_) if action == "quick-note" => {}
            Err(e) => {
                eprintln!("Spotlight is not running: {e}");
                return glib::ExitCode::FAILURE;
            }
        };
    }
    let test = std::env::args().any(|a| a == "--smoke-test");
    let app = gtk::Application::builder()
        .application_id(if test {
            "local.spotlight.Spotlight.Test"
        } else {
            "local.spotlight.Spotlight"
        })
        .build();
    let ui: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    let quit = gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);
    let show = gio::SimpleAction::new("show", None);
    let state = Rc::clone(&ui);
    show.connect_activate(move |_, _| {
        if let Some(ui) = state.borrow().as_ref() {
            ui.page(Page::Root);
            ui.window.present();
            ui.search.grab_focus();
        }
    });
    app.add_action(&show);
    let show_alarm = gio::SimpleAction::new("show-alarm", None);
    let state = Rc::clone(&ui);
    show_alarm.connect_activate(move |_, _| {
        if let Some(ui) = state.borrow().as_ref() {
            ui.show_alarm();
        }
    });
    app.add_action(&show_alarm);
    let quick_note = gio::SimpleAction::new("quick-note", None);
    let state = Rc::clone(&ui);
    quick_note.connect_activate(move |_, _| {
        if let Some(ui) = state.borrow().as_ref() {
            ui.window.present();
            ui.quick_note("");
        }
    });
    app.add_action(&quick_note);
    let diagnostics = gio::SimpleAction::new("diagnostics", None);
    let state = Rc::clone(&ui);
    diagnostics.connect_activate(move |_, _| {
        if let Ok(stats) = std::fs::read_to_string("/proc/self/smaps_rollup") {
            for line in stats.lines().filter(|s| {
                s.starts_with("Rss:")
                    || s.starts_with("Pss:")
                    || s.starts_with("Private_Clean:")
                    || s.starts_with("Private_Dirty:")
            }) {
                println!("{line}");
            }
        }
        if let Some(ui) = state.borrow().as_ref() {
            println!(
                "Panel visible: {} · indexed files: {}",
                ui.window.is_visible(),
                ui.files.borrow().len()
            );
        }
    });
    app.add_action(&diagnostics);
    app.connect_activate(move |app| {
        if let Some(ui) = ui.borrow().as_ref() {
            if ui.window.is_visible() {
                ui.window.set_visible(false);
            } else {
                ui.page(Page::Root);
                ui.window.present();
                ui.search.grab_focus();
            }
            return;
        }
        let built = build(app);
        if test {
            if std::env::args().any(|a| a == "--colors-smoke-test") {
                smoke_colors(Rc::clone(&built));
            } else if std::env::args().any(|a| a == "--fonts-smoke-test") {
                smoke_fonts(Rc::clone(&built));
            } else if std::env::args().any(|a| a == "--timer-setup-smoke-test") {
                smoke_timer_setup(Rc::clone(&built));
            } else if std::env::args().any(|a| a == "--alarm-smoke-test") {
                smoke_alarm(Rc::clone(&built));
            } else if std::env::args().any(|a| a == "--notes-smoke-test") {
                smoke_notes(Rc::clone(&built));
            } else if std::env::args().any(|a| a == "--reminder-smoke-test") {
                smoke_reminders(Rc::clone(&built));
            } else if std::env::args().any(|a| a == "--live-currency-test") {
                smoke_currency(Rc::clone(&built));
            } else {
                smoke(Rc::clone(&built));
            }
        }
        if std::env::args().any(|arg| arg == "--quick-note") {
            built.quick_note("");
        }
        *ui.borrow_mut() = Some(built);
    });
    let lifetime: Rc<RefCell<Option<gio::ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));
    let primary_lifetime = Rc::clone(&lifetime);
    app.connect_startup(move |app| {
        *primary_lifetime.borrow_mut() = Some(app.hold());
    });
    let result = app.run_with_args(&["spotlight"]);
    lifetime.borrow_mut().take();
    result
}
