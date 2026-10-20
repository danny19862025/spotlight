use crate::{Action, Item, Ui, core, item, pastequeue, todos, windowing};
use gtk::{gio, glib, prelude::*};
use std::{rc::Rc, sync::mpsc, time::Duration};

#[derive(Clone)]
pub enum Operation {
    AddTodo(String),
    ToggleTodo(usize, String),
    EditTodos,
    SaveLayout(String),
    RestoreLayout(String),
    DeleteLayout(String),
    Queue,
    StopQueue,
}
impl Operation {
    pub fn category(&self) -> &'static str {
        match self {
            Self::AddTodo(_) | Self::ToggleTodo(..) | Self::EditTodos => "saved",
            Self::SaveLayout(_) | Self::RestoreLayout(_) | Self::DeleteLayout(_) => "windows",
            Self::Queue | Self::StopQueue => "clipboard",
        }
    }
}
fn row(id: &str, title: &str, subtitle: &str, op: Operation) -> Item {
    item(
        id,
        title,
        subtitle,
        "view-list-symbolic",
        "Command",
        Action::Workflow(op),
    )
}
impl Ui {
    pub(super) fn workflow_rows(&self, query: &str) -> Vec<Item> {
        let mut rows = Vec::new();
        if self.store.borrow().commands.enabled("saved") {
            rows.extend(todos::rows(query));
        }
        if self.store.borrow().commands.enabled("windows")
            && (query == "layout" || query.starts_with("layout "))
        {
            let layouts = &self.store.borrow().window_layouts;
            let rest = query.strip_prefix("layout").unwrap().trim();
            let (verb, name) = rest.split_once(' ').unwrap_or(("", rest));
            let name = name.trim();
            if !name.is_empty() && name.chars().count() <= 80 {
                match verb {
                    "save" => rows.push(row(
                        "layout-save",
                        &format!("Save window layout · {name}"),
                        if layouts.contains_key(name) {
                            "Replace this saved layout with current KWin window positions"
                        } else {
                            "Save current KWin window positions"
                        },
                        Operation::SaveLayout(name.into()),
                    )),
                    "delete" if layouts.contains_key(name) => rows.push(row(
                        "layout-delete",
                        &format!("Delete layout · {name}"),
                        "Remove saved positions",
                        Operation::DeleteLayout(name.into()),
                    )),
                    "restore" | "" if layouts.contains_key(name) => rows.push(row(
                        "layout-restore",
                        &format!("Restore layout · {name}"),
                        "Arrange currently open windows · missing apps are skipped",
                        Operation::RestoreLayout(name.into()),
                    )),
                    _ => {}
                }
            }
            if rest.is_empty() {
                let mut names: Vec<_> = layouts.keys().collect();
                names.sort();
                for name in names {
                    rows.push(row(
                        &format!("layout:{name}"),
                        &format!("Restore layout · {name}"),
                        "Arrange currently open windows",
                        Operation::RestoreLayout(name.clone()),
                    ));
                }
            }
            if rows.iter().all(|r| {
                !matches!(
                    r.action,
                    Action::Workflow(
                        Operation::SaveLayout(_)
                            | Operation::RestoreLayout(_)
                            | Operation::DeleteLayout(_)
                    )
                )
            }) {
                rows.push(item(
                    "layout-help",
                    "Window layouts",
                    "layout save work · layout work · layout delete work",
                    "view-grid-symbolic",
                    "Help",
                    Action::Noop,
                ));
            }
        }
        if self.store.borrow().commands.enabled("clipboard") && query.starts_with("clip queue") {
            rows.push(row(
                "clip-queue",
                "Clipboard paste queue",
                "Review and reorder pins, then paste each with Ctrl+V",
                Operation::Queue,
            ));
            if query == "clip queue stop"
                || self
                    .paste_queue
                    .borrow()
                    .as_ref()
                    .is_some_and(|q| q.active.get())
            {
                rows.insert(
                    0,
                    row(
                        "clip-queue-stop",
                        "Stop clipboard queue",
                        "Leave the current entry on the clipboard",
                        Operation::StopQueue,
                    ),
                );
            }
        }
        rows
    }
    pub(super) fn workflow(self: &Rc<Self>, operation: Operation) {
        let path = core::data_dir().join("todos.md");
        match operation {
            Operation::AddTodo(text) => match todos::add(&path, &text) {
                Ok(()) => {
                    self.search.set_text("todo");
                    self.refresh();
                    self.error("Todo saved");
                }
                Err(e) => self.error(&e),
            },
            Operation::ToggleTodo(line, expected) => match todos::toggle(&path, line, &expected) {
                Ok(()) => {
                    self.refresh();
                    self.error("Todo updated");
                }
                Err(e) => self.error(&e),
            },
            Operation::EditTodos => {
                let result = todos::read(&path).and_then(|text| {
                    if !path.exists() {
                        todos::write(&path, "", &text)?;
                    }
                    gio::AppInfo::launch_default_for_uri(
                        &gio::File::for_path(&path).uri(),
                        None::<&gio::AppLaunchContext>,
                    )
                    .map_err(|e| e.to_string())
                });
                if let Err(e) = result {
                    self.error(&e);
                }
            }
            Operation::SaveLayout(name) => self.layout_request(name, true),
            Operation::RestoreLayout(name) => self.layout_request(name, false),
            Operation::DeleteLayout(name) => {
                if !self.can_save {
                    self.error("Saved data is unreadable");
                    return;
                }
                let mut store = self.store.borrow_mut();
                let previous = store.window_layouts.remove(&name);
                let result = store.save();
                if result.is_err()
                    && let Some(p) = previous
                {
                    store.window_layouts.insert(name, p);
                }
                drop(store);
                self.refresh();
                self.error(
                    result
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("Layout deleted"),
                );
            }
            Operation::Queue => self.queue_editor(),
            Operation::StopQueue => {
                if let Some(q) = self.paste_queue.borrow().as_ref() {
                    q.stop();
                }
                self.refresh();
                self.error("Clipboard queue stopped");
            }
        }
    }
    fn layout_request(self: &Rc<Self>, name: String, save: bool) {
        if save && !self.can_save {
            self.error("Saved data is unreadable");
            return;
        }
        if self.layout_busy.replace(true) {
            self.error("A layout operation is already running");
            return;
        }
        let placements = self.store.borrow().window_layouts.get(&name).cloned();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = if save {
                windowing::capture_layout().map(|p| (Some(p), String::new()))
            } else {
                placements
                    .ok_or("Layout no longer exists".into())
                    .and_then(|p| windowing::restore_layout(&p))
                    .map(|message| (None, message))
            };
            let _ = tx.send(result);
        });
        self.error(if save {
            "Saving window positions…"
        } else {
            "Restoring window positions…"
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(_) => ui.error("Window layout worker stopped"),
                Ok(Err(e)) => ui.error(&e),
                Ok(Ok((placements, message))) => {
                    if let Some(placements) = placements {
                        let count = placements.len();
                        let mut store = ui.store.borrow_mut();
                        let previous = store.window_layouts.insert(name.clone(), placements);
                        let result = store.save();
                        if result.is_err() {
                            match previous {
                                Some(p) => {
                                    store.window_layouts.insert(name.clone(), p);
                                }
                                None => {
                                    store.window_layouts.remove(&name);
                                }
                            }
                        }
                        drop(store);
                        ui.refresh();
                        match result {
                            Ok(()) => ui.error(&format!(
                                "Saved {name} · {count} windows · restore with layout {name}"
                            )),
                            Err(e) => ui.error(&e),
                        }
                    } else {
                        ui.error(&message);
                    }
                }
            }
            ui.layout_busy.set(false);
            glib::ControlFlow::Break
        });
    }
    pub(super) fn sync_pomodoro(self: &Rc<Self>) {
        let now = glib::DateTime::now_utc()
            .map(|d| d.to_unix())
            .unwrap_or(i64::MAX);
        let running = self
            .store
            .borrow()
            .reminders
            .iter()
            .any(|r| r.pomodoro && r.due > now);
        let mut state = self.pomodoro_awake.borrow_mut();
        if running && state.is_none() {
            match crate::caffeinate::Inhibitor::start() {
                Ok(inhibitor) => *state = Some(inhibitor),
                Err(e) => self.error(&format!("Pomodoro could not keep awake: {e}")),
            }
        } else if !running {
            state.take();
        }
        self.awake_status
            .set_visible(state.is_some() || self.caffeinate.borrow().is_some());
    }
    fn queue_editor(self: &Rc<Self>) {
        let dialog = gtk::Window::builder()
            .title("Clipboard paste queue")
            .application(&self.window.application().unwrap())
            .transient_for(&self.window)
            .modal(true)
            .default_width(560)
            .default_height(480)
            .build();
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel.add_css_class("editor");
        let info = gtk::Label::new(Some(
            "Pins paste from top to bottom. Use ↑ / ↓ to reorder; uncheck to skip.",
        ));
        info.set_wrap(true);
        panel.append(&info);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        for pin in &self.store.borrow().pinned_clips {
            let row = gtk::ListBoxRow::new();
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label: String = match pin {
                crate::clipboard::Pinned::Text { value, .. } => value
                    .lines()
                    .next()
                    .unwrap_or("Empty text")
                    .chars()
                    .take(65)
                    .collect(),
                crate::clipboard::Pinned::Image { .. } => "Pinned image".into(),
            };
            let check = gtk::CheckButton::with_label(&label);
            check.set_active(true);
            check.set_hexpand(true);
            line.append(&check);
            for (label, delta) in [("↑", -1), ("↓", 1)] {
                let button = gtk::Button::with_label(label);
                let l = list.clone();
                let r = row.clone();
                button.connect_clicked(move |_| {
                    let at = r.index();
                    let next = at + delta;
                    if next >= 0 && l.row_at_index(next).is_some() {
                        l.remove(&r);
                        l.insert(&r, next);
                    }
                });
                line.append(&button);
            }
            // A stable pin ID on the row keeps reorder independent of the persistent pin order.
            row.set_widget_name(pin.id());
            row.set_child(Some(&line));
            list.append(&row);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&list)
            .build();
        panel.append(&scroll);
        let status = gtk::Label::new(Some(
            "Start hides Spotlight. Paste normally in your app; copying new content stops the queue.",
        ));
        status.set_wrap(true);
        panel.append(&status);
        let start = gtk::Button::with_label("Start paste queue");
        start.add_css_class("accent");
        panel.append(&start);
        let ui = self.clone();
        let d = dialog.clone();
        start.connect_clicked(move |_| {
            let mut pins = Vec::new();
            let mut index = 0;
            while let Some(row) = list.row_at_index(index) {
                let line = row.child().unwrap().downcast::<gtk::Box>().unwrap();
                if line
                    .first_child()
                    .unwrap()
                    .downcast::<gtk::CheckButton>()
                    .unwrap()
                    .is_active()
                    && let Some(pin) = ui
                        .store
                        .borrow()
                        .pinned_clips
                        .iter()
                        .find(|p| p.id() == row.widget_name())
                {
                    pins.push(pin.clone());
                }
                index += 1;
            }
            match pastequeue::Queue::new(ui.window.clipboard(), &pins) {
                Ok(q) => {
                    if let Some(old) = ui.paste_queue.borrow_mut().take() {
                        old.stop();
                    }
                    *ui.paste_queue.borrow_mut() = Some(q.clone());
                    q.publish();
                    status.set_text("Preparing background clipboard…");
                    let d = d.clone();
                    let ui = ui.clone();
                    let status = status.clone();
                    glib::timeout_add_local(Duration::from_millis(25), move || {
                        if let Some(error) = q.error.borrow().as_ref() {
                            status.set_text(error);
                            return glib::ControlFlow::Break;
                        }
                        if q.ready.get() {
                            d.close();
                            ui.window.set_visible(false);
                            return glib::ControlFlow::Break;
                        }
                        glib::ControlFlow::Continue
                    });
                }
                Err(e) => status.set_text(&e),
            }
        });
        dialog.set_child(Some(&panel));
        dialog.present();
    }
}
