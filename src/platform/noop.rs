use super::InhibitFlags;

#[derive(Debug)]
pub struct Inhibitors;

pub fn activate(_flags: InhibitFlags) -> Result<Inhibitors, String> {
    Ok(Inhibitors)
}

pub fn on_ac_power() -> Option<bool> {
    None
}
