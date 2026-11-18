//! The readers of each operating system. Each module offers the same six
//! functions; the crate root orders, bounds, and names what they return.

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub(crate) mod netlink;
#[cfg(target_os = "linux")]
pub(crate) use linux as os;

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos as os;

#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
pub(crate) use windows as os;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) mod other;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) use other as os;
