use super::InhibitFlags;

#[derive(Debug)]
pub struct Inhibitors;

// Failing loudly beats pretending: a silent no-op would let this binary
// shadow a working caffeinate (e.g. /usr/bin/caffeinate on macOS) while
// the machine still goes to sleep.
pub fn activate(_flags: InhibitFlags) -> Result<Inhibitors, String> {
    #[cfg(target_os = "macos")]
    {
        Err(
            "sleep inhibition is not implemented in this build; use the system /usr/bin/caffeinate"
                .to_string(),
        )
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("sleep inhibition is not supported on this platform".to_string())
    }
}

pub fn on_ac_power() -> Option<bool> {
    None
}
