#[derive(Debug, Clone, Copy, Default)]
pub struct InhibitFlags {
    pub prevent_display_sleep: bool,
    pub prevent_idle_sleep: bool,
    pub prevent_system_sleep: bool,
    pub user_active: bool,
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod noop;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{activate, on_ac_power};
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub use noop::{activate, on_ac_power};
#[cfg(target_os = "windows")]
pub use windows::{activate, is_process_alive, on_ac_power};
