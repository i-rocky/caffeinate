use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Power::{
    GetSystemPowerStatus, SetThreadExecutionState, ES_AWAYMODE_REQUIRED, ES_CONTINUOUS,
    ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SYSTEM_POWER_STATUS,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::InhibitFlags;

#[derive(Debug)]
pub struct Inhibitors {
    active: bool,
}

impl Drop for Inhibitors {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                let _ = SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }
}

pub fn activate(flags: InhibitFlags) -> Result<Inhibitors, String> {
    let state = desired_execution_state(flags);

    if state == ES_CONTINUOUS {
        return Ok(Inhibitors { active: false });
    }

    let result = unsafe { SetThreadExecutionState(state) };
    if result == 0 {
        return Err(format!(
            "SetThreadExecutionState failed: {}",
            io::Error::last_os_error()
        ));
    }

    Ok(Inhibitors { active: true })
}

pub fn is_process_alive(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }

    let alive = process_still_active(handle);
    unsafe {
        let _ = CloseHandle(handle);
    }
    alive
}

fn process_still_active(handle: HANDLE) -> bool {
    let mut code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code as *mut u32) };
    ok != 0 && code == STILL_ACTIVE
}
const STILL_ACTIVE: u32 = 259;

fn desired_execution_state(flags: InhibitFlags) -> u32 {
    let mut state = ES_CONTINUOUS;

    if flags.prevent_idle_sleep || flags.prevent_system_sleep || flags.user_active {
        state |= ES_SYSTEM_REQUIRED;
    }

    if flags.prevent_display_sleep || flags.user_active {
        state |= ES_DISPLAY_REQUIRED;
    }

    if flags.prevent_system_sleep {
        state |= ES_AWAYMODE_REQUIRED;
    }

    state
}

pub fn on_ac_power() -> Option<bool> {
    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: 255,
        BatteryFlag: 0,
        BatteryLifePercent: 255,
        SystemStatusFlag: 0,
        BatteryLifeTime: u32::MAX,
        BatteryFullLifeTime: u32::MAX,
    };

    let ok = unsafe { GetSystemPowerStatus(&mut status as *mut SYSTEM_POWER_STATUS) };
    if ok == 0 {
        return None;
    }

    match status.ACLineStatus {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_assertions_sets_continuous_only() {
        let state = desired_execution_state(InhibitFlags::default());
        assert_eq!(state, ES_CONTINUOUS);
    }

    #[test]
    fn idle_sets_system_required() {
        let state = desired_execution_state(InhibitFlags {
            prevent_idle_sleep: true,
            ..InhibitFlags::default()
        });
        assert_eq!(state, ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
    }

    #[test]
    fn display_sets_display_required() {
        let state = desired_execution_state(InhibitFlags {
            prevent_display_sleep: true,
            ..InhibitFlags::default()
        });
        assert_eq!(state, ES_CONTINUOUS | ES_DISPLAY_REQUIRED);
    }

    #[test]
    fn system_sets_system_and_away_mode() {
        let state = desired_execution_state(InhibitFlags {
            prevent_system_sleep: true,
            ..InhibitFlags::default()
        });
        assert_eq!(
            state,
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_AWAYMODE_REQUIRED
        );
    }

    #[test]
    fn user_activity_sets_system_and_display() {
        let state = desired_execution_state(InhibitFlags {
            user_active: true,
            ..InhibitFlags::default()
        });
        assert_eq!(
            state,
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        );
    }
}
