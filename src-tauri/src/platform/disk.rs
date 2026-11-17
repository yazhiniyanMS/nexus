//! Free and total disk space.

use std::path::Path;

/// Free and total bytes of the filesystem holding `path` (or its nearest existing parent).
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let existing = path.ancestors().find(|p| p.exists())?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs only writes into the zeroed struct it is given.
        let stat = unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
                return None;
            }
            stat
        };
        #[allow(clippy::unnecessary_cast)]
        let block = stat.f_frsize as u64;
        #[allow(clippy::unnecessary_cast)]
        Some(((stat.f_bavail as u64).saturating_mul(block), (stat.f_blocks as u64).saturating_mul(block)))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: the path is NUL-terminated and the out-pointers are valid for the call.
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut())
        };
        (ok != 0).then_some((free, total))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_space_for_existing_and_not_yet_existing_folders() {
        let dir = std::env::temp_dir();
        let (free, total) = super::space(&dir).expect("the temp folder is on a real disk");
        assert!(total > 0 && free <= total);
        assert!(super::space(&dir.join("not-created-yet").join("deeper")).is_some(), "uses the nearest existing parent");
    }
}
