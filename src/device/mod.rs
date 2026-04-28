mod protocol;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

pub use protocol::DeviceEvent;

#[cfg(target_os = "linux")]
pub use linux::Device;
#[cfg(target_os = "windows")]
pub use windows::Device;
