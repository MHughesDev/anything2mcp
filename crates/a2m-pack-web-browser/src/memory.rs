//! How much memory one process of a browser holds, as its operating system
//! counts what the process costs: on Linux its proportional set size
//! (`Pss` of `/proc/<id>/smaps_rollup`), which shares each page among the
//! processes that map it, so that the processes of one browser add up to
//! what the browser holds; on macOS its physical footprint
//! (`ri_phys_footprint` of `proc_pid_rusage`), the figure the system's own
//! activity monitor shows; on Windows its private commit (`PrivateUsage`
//! of `GetProcessMemoryInfo`), which counts no page another process shares.
//!
//! Which processes are the browser's is the browser's own word, asked
//! over its pipe (`SystemInfo.getProcessInfo`); this module only measures
//! one process by its number. A number that names no process holds
//! nothing, since a process may end between the browser's list and the
//! measurement. Any other answer is an error, and the caller fails closed
//! on it.

/// The memory the process `id` holds, in bytes; `None` when there is no
/// such process. Blocking: it reads the system's own tables.
///
/// # Errors
///
/// A text that says why the process could not be measured, such as a
/// refused read.
pub(crate) fn of_process(id: u32) -> Result<Option<u64>, String> {
    imp::of_process(id)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io::ErrorKind;

    pub(super) fn of_process(id: u32) -> Result<Option<u64>, String> {
        let path = format!("/proc/{id}/smaps_rollup");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            // A process that ended while its file was read.
            Err(error) if error.raw_os_error() == Some(3) => return Ok(None),
            Err(error) => return Err(format!("`{path}` could not be read: {error}")),
        };
        // A process that has ended and is not yet reaped maps nothing, and
        // its file names no line.
        let Some(line) = text.lines().find(|line| line.starts_with("Pss:")) else {
            return Ok(None);
        };
        let mut words = line.split_whitespace().skip(1);
        let kilobytes = words
            .next()
            .and_then(|number| number.parse::<u64>().ok())
            .filter(|_| words.next() == Some("kB"))
            .ok_or_else(|| format!("`{path}` gives its `Pss` in a form no rule covers"))?;
        Ok(Some(kilobytes.saturating_mul(1024)))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    #[allow(unsafe_code)] // one call into the operating system, explained below
    pub(super) fn of_process(id: u32) -> Result<Option<u64>, String> {
        let pid = libc::c_int::try_from(id).map_err(|_| format!("{id} is not a process number"))?;
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: the flavour `RUSAGE_INFO_V2` tells the call to write one
        // `rusage_info_v2`, and `info` is that struct, zeroed, writable, and
        // alive for the whole call. The call reads nothing of ours.
        let done = unsafe {
            libc::proc_pid_rusage(
                pid,
                libc::RUSAGE_INFO_V2,
                info.as_mut_ptr().cast::<libc::rusage_info_t>(),
            )
        };
        if done != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(None);
            }
            return Err(format!("process {id} could not be measured: {error}"));
        }
        // SAFETY: the call succeeded, so it wrote the whole struct; and a
        // zeroed `rusage_info_v2`, all integers, is a valid one anyway.
        let info = unsafe { info.assume_init() };
        Ok(Some(info.ri_phys_footprint))
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER};
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    #[allow(unsafe_code)] // three calls into the operating system, each explained where it is made
    pub(super) fn of_process(id: u32) -> Result<Option<u64>, String> {
        // SAFETY: the call takes plain numbers and touches no memory of
        // ours; it returns a handle we own, or null.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id) };
        if process.is_null() {
            let error = std::io::Error::last_os_error();
            // The system answers so for a number that names no process.
            if error.raw_os_error() == i32::try_from(ERROR_INVALID_PARAMETER).ok() {
                return Ok(None);
            }
            return Err(format!("process {id} could not be opened: {error}"));
        }
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>())
            .map_err(|_| "the size of the counters is not a number the system takes".to_owned())?;
        counters.cb = size;
        // SAFETY: `process` is the open handle above; `counters` is a
        // `PROCESS_MEMORY_COUNTERS_EX`, writable and alive for the call, and
        // `size` is its size, so the call writes inside it. Its first part is
        // a `PROCESS_MEMORY_COUNTERS`, as the call's signature names it.
        let ok = unsafe {
            GetProcessMemoryInfo(
                process,
                std::ptr::from_mut(&mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
                size,
            )
        };
        let error = std::io::Error::last_os_error();
        // SAFETY: the handle is ours, open, and closed once, here.
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return Err(format!("process {id} could not be measured: {error}"));
        }
        Ok(Some(
            u64::try_from(counters.PrivateUsage).unwrap_or(u64::MAX),
        ))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub(super) fn of_process(_id: u32) -> Result<Option<u64>, String> {
        Err("the memory of a process is not measured on this operating system".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::of_process;

    #[test]
    fn this_process_holds_memory_and_a_number_of_no_process_holds_none() {
        let held = of_process(std::process::id()).unwrap();
        assert!(held.is_some_and(|bytes| bytes > 1024 * 1024), "{held:?}");
        // A number no system hands out today: above Linux's largest, and
        // macOS's, and on Windows not a multiple of four.
        let none = of_process(999_999_997);
        assert!(!matches!(none, Ok(Some(_))), "{none:?}");
        if cfg!(target_os = "linux") {
            assert_eq!(none, Ok(None));
        }
    }
}
