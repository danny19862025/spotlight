use crate::{Action, Item, Ui, core, item};
use gtk::{gio, glib, prelude::*};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, rc::Rc, sync::mpsc, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pinned {
    Text { id: String, value: String },
    Image { id: String, path: PathBuf },
}
impl Pinned {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Text { id, .. } | Self::Image { id, .. } => id,
        }
    }
}
pub fn filter(query: &str) -> (&str, &str) {
    let q = query.trim();
    for (prefix, kind) in [
        ("clip text", "text"),
        ("clip images", "images"),
        ("clip image", "images"),
        ("text", "text"),
        ("images", "images"),
        ("image", "images"),
        ("clip pinned", "pinned"),
        ("pinned", "pinned"),
    ] {
        if q == prefix {
            return (kind, "");
        }
        if let Some(rest) = q
            .strip_prefix(prefix)
            .and_then(|s| s.strip_prefix(char::is_whitespace))
        {
            return (kind, rest.trim());
        }
    }
    ("all", q)
}
pub fn command(query: &str) -> Option<&str> {
    let q = query.trim();
    let rest = q.strip_prefix("clip ")?.trim();
    let mode = rest.split_whitespace().next()?;
    matches!(mode, "text" | "image" | "images" | "pinned").then_some(rest)
}
pub fn is_pinned(id: &str) -> bool {
    id.starts_with("pinned:")
}
pub fn clear_history() -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .map_err(|e| e.to_string())?;
    for method in ["clearClipboardHistory", "clearClipboardContents"] {
        bus.call_sync(
            Some("org.kde.klipper"),
            "/klipper",
            "org.kde.klipper.klipper",
            method,
            None,
            None,
            gio::DBusCallFlags::NONE,
            3000,
            None::<&gio::Cancellable>,
        )
        .map_err(|e| format!("Could not clear Klipper: {e}"))?;
    }
    Ok(())
}
impl Ui {
    pub(super) fn pinned_clipboard_items(&self) -> Vec<Item> {
        self.store
            .borrow()
            .pinned_clips
            .iter()
            .enumerate()
            .filter_map(|(n, pin)| {
                let mut row = match pin {
                    Pinned::Text { id, value } => item(
                        &format!("pinned:{id}"),
                        &value
                            .lines()
                            .next()
                            .unwrap_or("Pinned text")
                            .chars()
                            .take(100)
                            .collect::<String>(),
                        "Pinned text · Enter copies · Ctrl+K to unpin",
                        "pin-symbolic",
                        "Pinned",
                        Action::Copy(value.clone()),
                    ),
                    Pinned::Image { id, path } => {
                        if !path.is_file() {
                            return None;
                        }
                        let mut row = item(
                            &format!("pinned:{id}"),
                            "Pinned clipboard image",
                            "Enter copies · Ctrl+K previews or unpins",
                            "image-x-generic-symbolic",
                            "Pinned",
                            Action::ClipboardImage(path.clone()),
                        );
                        row.thumbnail = Some(path.clone());
                        row
                    }
                };
                row.priority = 10_000 - n as i32;
                Some(row)
            })
            .collect()
    }
    pub(super) fn clipboard_actions(
        self: &Rc<Self>,
        menu: &gtk::Box,
        popup: &gtk::Popover,
        selected: Option<&Item>,
    ) {
        if *self.page.borrow() != crate::Page::Clipboard {
            return;
        }
        if let Some(selected) = selected
            && matches!(
                selected.action,
                Action::Copy(_) | Action::ClipboardImage(_) | Action::ClipboardTexture(_)
            )
        {
            let button = gtk::Button::with_label(if is_pinned(&selected.id) {
                "Unpin entry"
            } else {
                "Pin entry"
            });
            let ui = self.clone();
            let target = selected.clone();
            let pop = popup.clone();
            button.connect_clicked(move |_| {
                pop.popdown();
                ui.pin_clipboard(&target);
            });
            menu.append(&button);
        }
        for (label, query) in [
            ("Show text only", "clip text"),
            ("Show images only", "clip images"),
            ("Show pinned entries", "clip pinned"),
            ("Show all clipboard entries", ""),
        ] {
            let b = gtk::Button::with_label(label);
            let ui = self.clone();
            let pop = popup.clone();
            b.connect_clicked(move |_| {
                pop.popdown();
                ui.search.set_text(query);
            });
            menu.append(&b);
        }
        let clear = gtk::Button::with_label("Clear clipboard history…");
        let ui = self.clone();
        let pop = popup.clone();
        clear.connect_clicked(move |_| {
            pop.popdown();
            ui.clear_clipboard();
        });
        menu.append(&clear);
        menu.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    pub(super) fn pin_clipboard(self: &Rc<Self>, selected: &Item) {
        if !self.can_save {
            self.error("Saved data is unreadable; clipboard pins cannot be changed");
            return;
        }
        if let Some(id) = selected.id.strip_prefix("pinned:") {
            let mut store = self.store.borrow_mut();
            let Some(index) = store.pinned_clips.iter().position(|p| p.id() == id) else {
                return;
            };
            let pin = store.pinned_clips.remove(index);
            if let Err(error) = store.save() {
                store.pinned_clips.insert(index, pin);
                drop(store);
                self.error(&error);
                return;
            }
            if let Pinned::Image { path, .. } = pin {
                let directory = core::data_dir().join("clipboard-pins");
                if path.parent() == Some(directory.as_path()) {
                    let _ = fs::remove_file(path);
                }
            }
            drop(store);
            self.refresh();
            self.error("Entry unpinned");
            return;
        }
        if self.store.borrow().pinned_clips.len() >= 50 {
            self.error("Up to 50 entries can be pinned · unpin an entry first");
            return;
        }
        let result = (|| -> Result<Pinned, String> {
            match &selected.action {
                Action::Copy(value) => {
                    if value.len() > 65536 {
                        return Err("Pinned text is limited to 64 KiB".into());
                    }
                    let id = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, value)
                        .unwrap()
                        .to_string();
                    Ok(Pinned::Text {
                        id: format!("text-{id}"),
                        value: value.clone(),
                    })
                }
                Action::ClipboardTexture(texture) => {
                    if i64::from(texture.width()) * i64::from(texture.height()) > 16_000_000 {
                        return Err("Pinned images are limited to 16 million pixels".into());
                    }
                    pin_image(&texture.save_to_png_bytes())
                }
                Action::ClipboardImage(path) => {
                    crate::clipimage::load(path)?;
                    let bytes = fs::read(path).map_err(|e| e.to_string())?;
                    pin_image(&bytes)
                }
                _ => Err("Choose a clipboard entry to pin".into()),
            }
        })();
        match result {
            Ok(pin) => {
                let mut store = self.store.borrow_mut();
                if store.pinned_clips.iter().any(|p| p.id() == pin.id()) {
                    if let Pinned::Image { path, .. } = pin {
                        let _ = fs::remove_file(path);
                    }
                    drop(store);
                    self.error("That entry is already pinned");
                    return;
                }
                store.pinned_clips.insert(0, pin);
                if let Err(error) = store.save() {
                    let pin = store.pinned_clips.remove(0);
                    if let Pinned::Image { path, .. } = pin {
                        let _ = fs::remove_file(path);
                    }
                    drop(store);
                    self.error(&error);
                    return;
                }
                drop(store);
                self.refresh();
                self.error("Entry pinned in Spotlight");
            }
            Err(error) => self.error(&error),
        }
    }
    pub(super) fn clear_clipboard(self: &Rc<Self>) {
        let ui = self.clone();
        self.confirm("Clear clipboard history?","Clear Klipper history and the current clipboard. Entries pinned in Spotlight are kept.","Clear history",move||{
            let(tx,rx)=mpsc::channel();std::thread::spawn(move||{let _=tx.send(clear_history());});let weak=Rc::downgrade(&ui);
            glib::timeout_add_local(Duration::from_millis(80),move||{let Some(ui)=weak.upgrade()else{return glib::ControlFlow::Break;};match rx.try_recv(){Ok(Ok(()))=>{ui.clips.borrow_mut().clear();ui.clip_images.borrow_mut().clear();ui.clip_current.borrow_mut().take();ui.clip_generation.set(ui.clip_generation.get().wrapping_add(1));ui.refresh();ui.error("History cleared · pinned entries kept");glib::ControlFlow::Break},Ok(Err(error))=>{ui.error(&error);glib::ControlFlow::Break},Err(mpsc::TryRecvError::Empty)=>glib::ControlFlow::Continue,Err(_)=>{ui.error("Clipboard clearing interrupted");glib::ControlFlow::Break}}});
        });
    }
}
fn pin_image(bytes: &[u8]) -> Result<Pinned, String> {
    if bytes.len() > 16_777_216 {
        return Err("Pinned images are limited to 16 MiB".into());
    }
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let id = glib::compute_checksum_for_data(glib::ChecksumType::Sha256, bytes)
        .unwrap()
        .to_string();
    let dir = core::data_dir().join("clipboard-pins");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.png", glib::uuid_string_random()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if let Err(error) = file.write_all(bytes) {
        let _ = fs::remove_file(&path);
        return Err(error.to_string());
    }
    Ok(Pinned::Image {
        id: format!("image-{id}"),
        path,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_filters_and_old_saved_data() {
        assert_eq!(filter("clip text café"), ("text", "café"));
        assert_eq!(filter("images hello"), ("images", "hello"));
        assert_eq!(filter("clip pinned"), ("pinned", ""));
        assert_eq!(filter("textual note"), ("all", "textual note"));
        assert_eq!(command("clipboard"), None);
        assert_eq!(command("clip"), None);
        assert_eq!(command("clip text"), Some("text"));
        let store: core::Store = serde_json::from_str("{\"notes\":[]}").unwrap();
        assert!(store.pinned_clips.is_empty());
        let pin = Pinned::Text {
            id: "id".into(),
            value: "first\nsecond".into(),
        };
        let decoded: Pinned = serde_json::from_str(&serde_json::to_string(&pin).unwrap()).unwrap();
        assert!(matches!(decoded,Pinned::Text{value,..}if value=="first\nsecond"));
    }
}
