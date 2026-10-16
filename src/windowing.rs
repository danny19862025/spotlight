use crate::{
    boosters::{self, Entry, Operation},
    core,
};
use gtk::{gio, glib, prelude::*};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};
#[derive(serde::Deserialize, Debug)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub app: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub minimized: bool,
}
type Replies = Mutex<HashMap<String, mpsc::Sender<String>>>;
static REPLIES: OnceLock<Replies> = OnceLock::new();
static BUS_NAME: OnceLock<String> = OnceLock::new();
pub struct Bridge {
    bus: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(id) = self.registration.take() {
            let _ = self.bus.unregister_object(id);
        }
    }
}
pub fn register() -> Result<Bridge, String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .map_err(|e| e.to_string())?;
    let info=gio::DBusNodeInfo::for_xml("<node><interface name='local.spotlight.Windows'><method name='Result'><arg type='s' direction='in'/><arg type='s' direction='in'/></method></interface></node>").map_err(|e|e.to_string())?.lookup_interface("local.spotlight.Windows").unwrap();
    let registration = bus
        .register_object("/local/spotlight/Windows", &info)
        .method_call(|_, _, _, _, _, params, invocation| {
            if let Some((token, payload)) = params.get::<(String, String)>()
                && payload.len() <= 262144
                && let Some(replies) = REPLIES.get()
                && let Ok(mut replies) = replies.lock()
                && let Some(tx) = replies.remove(&token)
            {
                let _ = tx.send(payload);
            }
            invocation.return_value(None);
        })
        .build()
        .map_err(|e| e.to_string())?;
    let _ = BUS_NAME.set(
        bus.unique_name()
            .ok_or("D-Bus connection has no unique name")?
            .into(),
    );
    REPLIES.get_or_init(Default::default);
    Ok(Bridge {
        bus,
        registration: Some(registration),
    })
}
fn call(
    bus: &gio::DBusConnection,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
) -> Result<glib::Variant, String> {
    bus.call_sync(
        Some("org.kde.KWin"),
        path,
        interface,
        method,
        args,
        None,
        gio::DBusCallFlags::NONE,
        1000,
        None::<&gio::Cancellable>,
    )
    .map_err(|e| format!("KWin: {e}"))
}
fn request(focus: Option<&str>) -> Result<String, String> {
    let name = BUS_NAME
        .get()
        .ok_or("Window switching requires a KDE KWin desktop session")?;
    let token = glib::uuid_string_random().to_string();
    let plugin = format!("spotlight-{token}");
    let directory = core::data_dir().join("runtime");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let path = directory.join(format!("{token}.js"));
    let script = script(name, &token, focus);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(script.as_bytes())
        .map_err(|e| e.to_string())?;
    drop(file);
    let (tx, rx) = mpsc::channel();
    REPLIES
        .get()
        .unwrap()
        .lock()
        .map_err(|_| "Window request lock failed")?
        .insert(token.clone(), tx);
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let response = call(
            &bus,
            "/Scripting",
            "org.kde.kwin.Scripting",
            "loadScript",
            Some(&(path.to_string_lossy().as_ref(), plugin.as_str()).to_variant()),
        )?;
        let (id,) = response
            .get::<(i32,)>()
            .ok_or("Unexpected KWin script response")?;
        if id < 0 {
            return Err("KWin could not load its temporary window script".into());
        }
        // Plasma 6 exposes each loaded JavaScript script under this path.
        call(
            &bus,
            &format!("/Scripting/Script{id}"),
            "org.kde.kwin.Script",
            "run",
            None,
        )?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| "KWin window request timed out".into())
    })();
    let _ = call(
        &bus,
        "/Scripting",
        "org.kde.kwin.Scripting",
        "unloadScript",
        Some(&(plugin,).to_variant()),
    );
    REPLIES
        .get()
        .unwrap()
        .lock()
        .map_err(|_| "Window request lock failed")?
        .remove(&token);
    let _ = fs::remove_file(path);
    result
}
fn script(service: &str, token: &str, focus: Option<&str>) -> String {
    let service = serde_json::to_string(service).unwrap();
    let token = serde_json::to_string(token).unwrap();
    let target = serde_json::to_string(&focus).unwrap();
    format!(
        r#"(function() {{
function reply(value) {{ callDBus({service}, '/local/spotlight/Windows', 'local.spotlight.Windows', 'Result', {token}, JSON.stringify(value)); }}
try {{
const windows = workspace.windowList(); const target = {target};
if (target !== null) {{
 const w = windows.find(w => String(w.internalId) === target && !w.deleted);
 if (!w) {{ reply({{error:'That window has closed'}}); return; }}
 if (w.activities && w.activities.length && w.activities.indexOf(workspace.currentActivity) < 0) workspace.currentActivity = w.activities[0];
 if (w.desktops && w.desktops.length && w.desktops.indexOf(workspace.currentDesktop) < 0) workspace.currentDesktop = w.desktops[0];
 w.minimized = false;
 if (typeof workspace.activateWindow === 'function') workspace.activateWindow(w); else workspace.activeWindow = w;
 reply({{focused: String(workspace.activeWindow && workspace.activeWindow.internalId) === target}}); return;
}}
reply(windows.filter(w => !w.deleted && !w.specialWindow && !w.skipTaskbar && w.normalWindow && String(w.resourceClass).toLowerCase() !== 'spotlight' && String(w.resourceClass).toLowerCase() !== 'local.spotlight.spotlight').slice(0,200).map(w => ({{id:String(w.internalId),title:String(w.caption),app:String(w.desktopFileName || w.resourceClass),active:!!w.active,minimized:!!w.minimized}})));
}} catch(e) {{ reply({{error:String(e)}}); }}
}})();"#
    )
}
pub fn list() -> Result<Vec<Window>, String> {
    let payload = request(None)?;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload)
        && let Some(error) = value.get("error").and_then(|s| s.as_str())
    {
        return Err(error.into());
    }
    serde_json::from_str(&payload).map_err(|_| "Invalid KWin window list".into())
}
pub fn fetch(query: &str) -> Result<Vec<Entry>, String> {
    let mut windows: Vec<_> = list()?
        .into_iter()
        .filter_map(|window| {
            core::score(query, &format!("{} {}", window.app, window.title))
                .map(|score| (score, window))
        })
        .collect();
    windows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.active.cmp(&a.1.active)));
    let mut rows: Vec<_> = windows
        .into_iter()
        .take(60)
        .map(|(_, w)| Entry {
            title: w.title,
            detail: format!(
                "{} · {} · Enter to switch",
                w.app,
                if w.minimized {
                    "Minimized"
                } else if w.active {
                    "Active"
                } else {
                    "Open"
                }
            ),
            badge: "Window",
            operation: Some(Operation::FocusWindow(w.id)),
        })
        .collect();
    if rows.is_empty() {
        rows.push(boosters::hint(
            "No matching open windows",
            "Type window chrome, window terminal, or window · KWin",
        ));
    }
    Ok(rows)
}
pub fn focus(id: &str) -> Result<String, String> {
    if id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || "{}-".contains(c))
    {
        return Err("Invalid window identity".into());
    }
    let response: serde_json::Value =
        serde_json::from_str(&request(Some(id))?).map_err(|_| "Invalid KWin focus response")?;
    if response["focused"] == true {
        Ok("Switched window".into())
    } else {
        Err(response["error"]
            .as_str()
            .unwrap_or("KWin could not activate that window")
            .into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_arguments_are_json_escaped() {
        let text = script(":1.1", "token", Some("a\"b"));
        assert!(text.contains(r#"a\"b"#));
        assert!(text.contains("workspace.windowList()"));
        assert!(text.contains("workspace.activeWindow = w"));
    }
}
