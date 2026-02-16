use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedFd;

use super::InhibitFlags;

const APP_ID: &str = "caffeinate";

#[derive(Debug)]
pub struct Inhibitors {
    _login1_fds: Vec<OwnedFd>,
    _screensaver_guards: Vec<ScreenSaverInhibitor>,
}

#[derive(Debug)]
struct ScreenSaverInhibitor {
    connection: Connection,
    cookie: u32,
}

impl Drop for ScreenSaverInhibitor {
    fn drop(&mut self) {
        if let Ok(proxy) = Proxy::new(
            &self.connection,
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
        ) {
            let _: Result<(), _> = proxy.call("UnInhibit", &(self.cookie));
        }
    }
}

pub fn activate(flags: InhibitFlags) -> Result<Inhibitors, String> {
    let need_idle = flags.prevent_idle_sleep || flags.prevent_display_sleep || flags.user_active;
    let need_system = flags.prevent_system_sleep;
    let need_screensaver = flags.prevent_display_sleep || flags.user_active;

    if !need_idle && !need_system && !need_screensaver {
        return Ok(Inhibitors {
            _login1_fds: Vec::new(),
            _screensaver_guards: Vec::new(),
        });
    }

    let mut errors = Vec::new();
    let mut login1_fds = Vec::new();
    let mut screensaver_guards = Vec::new();

    if need_idle || need_system {
        match Connection::system() {
            Ok(conn) => {
                if need_idle {
                    match inhibit_login1(&conn, "idle", "Preventing idle sleep") {
                        Ok(fd) => login1_fds.push(fd),
                        Err(e) => errors.push(e),
                    }
                }
                if need_system {
                    match inhibit_login1(&conn, "sleep", "Preventing system sleep") {
                        Ok(fd) => login1_fds.push(fd),
                        Err(e) => errors.push(e),
                    }
                }
            }
            Err(e) => errors.push(format!("failed to connect to system bus: {e}")),
        }
    }

    if need_screensaver {
        match Connection::session() {
            Ok(conn) => match inhibit_screensaver(&conn, "Preventing display sleep") {
                Ok(cookie) => screensaver_guards.push(ScreenSaverInhibitor {
                    connection: conn,
                    cookie,
                }),
                Err(e) => errors.push(e),
            },
            Err(e) => errors.push(format!("failed to connect to session bus: {e}")),
        }
    }

    if login1_fds.is_empty() && screensaver_guards.is_empty() {
        let joined = errors.join("; ");
        return Err(format!(
            "failed to acquire any sleep inhibitor on Linux: {joined}"
        ));
    }

    for err in errors {
        eprintln!("caffeinate: warning: {err}");
    }

    Ok(Inhibitors {
        _login1_fds: login1_fds,
        _screensaver_guards: screensaver_guards,
    })
}

pub fn on_ac_power() -> Option<bool> {
    let base = std::path::Path::new("/sys/class/power_supply");
    let entries = std::fs::read_dir(base).ok()?;

    let mut saw_mains = false;
    let mut any_online = false;

    for entry in entries.flatten() {
        let p = entry.path();
        let Ok(ty) = std::fs::read_to_string(p.join("type")) else {
            continue;
        };
        let ty = ty.trim();
        if ty != "Mains" && ty != "AC" {
            continue;
        }
        saw_mains = true;

        let online = std::fs::read_to_string(p.join("online"))
            .ok()
            .map(|s| s.trim() == "1")
            .unwrap_or(false);
        if online {
            any_online = true;
        }
    }

    if saw_mains {
        Some(any_online)
    } else {
        None
    }
}

fn inhibit_login1(connection: &Connection, what: &str, reason: &str) -> Result<OwnedFd, String> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(|e| format!("failed to bind login1 proxy: {e}"))?;

    proxy
        .call::<_, _, OwnedFd>("Inhibit", &(what, APP_ID, reason, "block"))
        .map_err(|e| format!("login1 inhibit({what}) failed: {e}"))
}

fn inhibit_screensaver(connection: &Connection, reason: &str) -> Result<u32, String> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
    )
    .map_err(|e| format!("failed to bind screensaver proxy: {e}"))?;

    proxy
        .call::<_, _, u32>("Inhibit", &(APP_ID, reason))
        .map_err(|e| format!("screensaver inhibit failed: {e}"))
}
