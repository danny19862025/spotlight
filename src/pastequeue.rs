use crate::{clipboard::Pinned, core};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

pub struct Queue {
    directory: PathBuf,
    helper: PathBuf,
    child: RefCell<Option<Child>>,
    pub active: Cell<bool>,
    pub ready: Cell<bool>,
    pub error: RefCell<Option<String>>,
}
impl Queue {
    pub fn new(_: gdk::Clipboard, pins: &[Pinned]) -> Result<Rc<Self>, String> {
        if pins.is_empty() {
            return Err("Pin clipboard entries first, then select at least one".into());
        }
        let installed = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("spotlight-pastequeue")));
        let helper=[std::env::var_os("SPOTLIGHT_PASTEQUEUE_HELPER").map(PathBuf::from),installed,Some(core::home().join(".local/lib/spotlight/spotlight-pastequeue")),Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/native/spotlight-pastequeue"))].into_iter().flatten().find(|p|p.is_file()).ok_or("Missing paste queue helper · build with sh native/build-pastequeue.sh and install it beside Spotlight")?;
        let directory = core::data_dir()
            .join("runtime")
            .join(format!("pastequeue-{}", glib::uuid_string_random()));
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let prepare = (|| {
            let mut manifest = Vec::new();
            let mut total = 0;
            for (index, pin) in pins.iter().enumerate() {
                let (mime, bytes) = match pin {
                    Pinned::Text { value, .. } => {
                        ("text/plain;charset=utf-8", value.as_bytes().to_vec())
                    }
                    Pinned::Image { path, .. } => {
                        let texture = gdk::Texture::from_file(&gio::File::for_path(path))
                            .map_err(|e| format!("Cannot load pinned image: {e}"))?;
                        ("image/png", texture.save_to_png_bytes().as_ref().to_vec())
                    }
                };
                total += bytes.len();
                if total > 67_108_864 {
                    return Err("Queue exceeds 64 MiB; select fewer pins".into());
                }
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(directory.join(index.to_string()))
                    .map_err(|e| e.to_string())?;
                file.write_all(&bytes).map_err(|e| e.to_string())?;
                manifest.push(mime);
            }
            if manifest.len() > 50 {
                return Err("Choose at most 50 pins".into());
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(directory.join("queue.json"))
                .map_err(|e| e.to_string())?;
            file.write_all(&serde_json::to_vec(&manifest).unwrap())
                .map_err(|e| e.to_string())
        })();
        if let Err(e) = prepare {
            let _ = fs::remove_dir_all(&directory);
            return Err(e);
        }
        Ok(Rc::new(Self {
            directory,
            helper,
            child: RefCell::new(None),
            active: Cell::new(true),
            ready: Cell::new(false),
            error: RefCell::new(None),
        }))
    }
    pub fn publish(self: &Rc<Self>) {
        if self.child.borrow().is_some() {
            return;
        }
        let result = Command::new(&self.helper)
            .arg(self.directory.join("queue.json"))
            .env(
                "QT_QPA_PLATFORM",
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    "wayland"
                } else {
                    "xcb"
                },
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                self.active.set(false);
                *self.error.borrow_mut() = Some(format!("Cannot start paste queue: {e}"));
                return;
            }
        };
        let output = child.stdout.take().unwrap();
        *self.child.borrow_mut() = Some(child);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let weak = Rc::downgrade(self);
        let started = std::time::Instant::now();
        glib::timeout_add_local(Duration::from_millis(25), move || {
            let Some(queue) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            loop {
                match rx.try_recv() {
                    Ok(line) if line == "ready" => queue.ready.set(true),
                    Ok(line) if line == "done" || line == "stopped" => {
                        queue.active.set(false);
                        return glib::ControlFlow::Break;
                    }
                    Ok(line) if line == "cancelled" => {
                        queue.active.set(false);
                        if !queue.ready.get() {
                            *queue.error.borrow_mut() =
                                Some("Could not acquire the background clipboard".into());
                        }
                        return glib::ControlFlow::Break;
                    }
                    Ok(line) if line.starts_with("error ") => {
                        queue.active.set(false);
                        *queue.error.borrow_mut() = Some(line[6..].into());
                        return glib::ControlFlow::Break;
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(_) => {
                        queue.active.set(false);
                        if !queue.ready.get() {
                            *queue.error.borrow_mut() =
                                Some("KDE background clipboard access is unavailable".into());
                        }
                        return glib::ControlFlow::Break;
                    }
                }
            }
            if !queue.ready.get() && started.elapsed() > Duration::from_secs(5) {
                queue.active.set(false);
                *queue.error.borrow_mut() = Some("Paste queue helper timed out".into());
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }
    pub fn stop(&self) {
        self.active.set(false);
        if let Some(child) = self.child.borrow_mut().as_mut()
            && let Some(input) = child.stdin.as_mut()
        {
            let _ = input.write_all(b"stop\n");
        }
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}
