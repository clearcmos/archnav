//! Best-effort mirror of tags into the user.xdg.tags extended attribute.
//!
//! On filesystems that support user xattrs (ext4, btrfs, xfs) this lets KDE
//! Dolphin and Baloo see the tags natively. On filesystems that do not
//! (CIFS mounts return ENOTSUP) it is a silent no-op. The index remains the
//! source of truth either way; the xattr is written, never read back.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub const XATTR_NAME: &str = "user.xdg.tags";

/// Write tags to user.xdg.tags; remove the xattr when tags is empty.
/// Returns true when the xattr was updated, false when the filesystem does
/// not support it or permission was denied. Never errors.
pub fn mirror_tags(path: &Path, tags: &[String]) -> bool {
    let Ok(cpath) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let name = CString::new(XATTR_NAME).expect("no NUL in xattr name");

    if tags.is_empty() {
        let rc = unsafe { libc::removexattr(cpath.as_ptr(), name.as_ptr()) };
        if rc == 0 {
            return true;
        }
        // Absent xattr counts as success (nothing to remove).
        std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA)
    } else {
        let value = tags.join(",");
        let rc = unsafe {
            libc::setxattr(
                cpath.as_ptr(),
                name.as_ptr(),
                value.as_ptr() as *const libc::c_void,
                value.len(),
                0,
            )
        };
        rc == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xattrs_supported(dir: &Path) -> bool {
        let probe = dir.join(".xattr-probe");
        std::fs::write(&probe, b"").unwrap();
        let ok = mirror_tags(&probe, &["probe".into()]);
        let _ = std::fs::remove_file(&probe);
        ok
    }

    #[test]
    fn write_and_clear_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        if !xattrs_supported(tmp.path()) {
            eprintln!("skipping: no user xattr support under TMPDIR");
            return;
        }
        let f = tmp.path().join("doc.txt");
        std::fs::write(&f, b"content").unwrap();
        assert!(mirror_tags(&f, &["b".into(), "a".into()]));
        // Read back via getxattr to verify the comma join.
        let cpath = CString::new(f.as_os_str().as_bytes()).unwrap();
        let name = CString::new(XATTR_NAME).unwrap();
        let mut buf = [0u8; 64];
        let n = unsafe {
            libc::getxattr(
                cpath.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        assert_eq!(&buf[..n as usize], b"b,a");
        // Clearing removes it; clearing again is still success.
        assert!(mirror_tags(&f, &[]));
        assert!(mirror_tags(&f, &[]));
    }

    #[test]
    fn unsupported_path_returns_false() {
        // A nonexistent file gives ENOENT, which must map to false, not panic.
        assert!(!mirror_tags(Path::new("/nonexistent/x"), &["a".into()]));
    }
}
