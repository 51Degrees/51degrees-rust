/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! The memory and disk space the machine has, read from the operating
//! system, so caches can size themselves with no setting. Native targets
//! only. `disk` comes with the `disk` feature.

#[cfg(feature = "disk")]
use std::path::Path;

/// The memory the machine has, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryInfo {
    /// All the physical memory.
    pub total: u64,
    /// The memory that can be given to a process without swapping, or `None`
    /// where the operating system does not say.
    pub available: Option<u64>,
}

/// The space on the volume that holds a path, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskInfo {
    /// The size of the volume.
    pub total: u64,
    /// The space this process can still write.
    pub available: u64,
}

/// The machine's memory, or `None` where it cannot be read.
pub fn memory() -> Option<MemoryInfo> {
    imp::memory()
}

/// The space on the volume that holds `path`, or `None` where it cannot be
/// read. `path` must exist.
#[cfg(feature = "disk")]
pub fn disk(path: &Path) -> Option<DiskInfo> {
    volume::disk(path)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    use super::MemoryInfo;

    pub(super) fn memory() -> Option<MemoryInfo> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let field = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
                .and_then(|rest| {
                    rest.trim()
                        .trim_end_matches("kB")
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
                .map(|kib| kib.saturating_mul(1024))
        };
        Some(MemoryInfo {
            total: field("MemTotal")?,
            available: field("MemAvailable"),
        })
    }
}

#[cfg(target_vendor = "apple")]
mod imp {
    use super::MemoryInfo;

    pub(super) fn memory() -> Option<MemoryInfo> {
        let mut total: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        // SAFETY: the name is a nul-terminated string, and `total` and `size`
        // are valid for writes of the sizes given.
        let status = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut total as *mut u64).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (status == 0 && total > 0).then_some(MemoryInfo {
            total,
            available: None,
        })
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_vendor = "apple"))
))]
mod imp {
    use super::MemoryInfo;

    pub(super) fn memory() -> Option<MemoryInfo> {
        None
    }
}

#[cfg(windows)]
mod imp {
    use super::MemoryInfo;

    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    pub(super) fn memory() -> Option<MemoryInfo> {
        let mut status = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            memory_load: 0,
            total_phys: 0,
            avail_phys: 0,
            total_page_file: 0,
            avail_page_file: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended_virtual: 0,
        };
        // SAFETY: `status` is valid for writes and its length is set, as the
        // function requires.
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return None;
        }
        Some(MemoryInfo {
            total: status.total_phys,
            available: Some(status.avail_phys),
        })
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::MemoryInfo;

    pub(super) fn memory() -> Option<MemoryInfo> {
        None
    }
}

// The space on a volume, read only with the disk feature.
#[cfg(feature = "disk")]
mod volume {
    use super::DiskInfo;
    use std::path::Path;

    // The statvfs field types differ between systems, so each is widened to
    // u64 through this, which needs no cast on any of them.
    #[cfg(unix)]
    fn widen<T: Into<u64>>(value: T) -> u64 {
        value.into()
    }

    #[cfg(unix)]
    pub(super) fn disk(path: &Path) -> Option<DiskInfo> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs is plain data, for which all zeroes is a valid value.
        let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: the path is nul-terminated and `stats` is valid for writes.
        if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
            return None;
        }
        let fragment = widen(stats.f_frsize).max(1);
        Some(DiskInfo {
            total: widen(stats.f_blocks).saturating_mul(fragment),
            available: widen(stats.f_bavail).saturating_mul(fragment),
        })
    }

    #[cfg(windows)]
    pub(super) fn disk(path: &Path) -> Option<DiskInfo> {
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                free_to_caller: *mut u64,
                total: *mut u64,
                total_free: *mut u64,
            ) -> i32;
        }

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: the path is nul-terminated and the outputs are valid for
        // writes.
        let ok =
            unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
        (ok != 0).then_some(DiskInfo { total, available })
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn disk(_path: &Path) -> Option<DiskInfo> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_machine_reports_memory_and_disk_where_the_system_says() {
        if cfg!(any(windows, target_os = "linux")) {
            let memory = memory().expect("memory");
            assert!(memory.total > 0);
            assert!(memory
                .available
                .is_some_and(|available| available <= memory.total));
            #[cfg(feature = "disk")]
            {
                let disk = disk(&std::env::temp_dir()).expect("disk");
                assert!(disk.total > 0 && disk.available <= disk.total);
            }
        }
    }
}
