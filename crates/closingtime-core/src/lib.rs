//! Offline ownership records. Integrators must obtain review/approval before apply_plan.
//! Tags and ancestry express provenance, not an adversarial security boundary.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("Closingtime currently supports Linux and macOS only");
mod engine;
pub mod model;
pub mod platform;
pub mod store;
pub use engine::{Engine, ReviewedApproval};
pub use model::*;
pub use platform::{Backend, NativeBackend};
pub use store::{Store, default_state_dir};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn new_id() -> Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
