//! Directory-based store lock.
//!
//! mkdir is atomic on local filesystems and over SMB, unlike O_EXCL opens
//! or byte-range locks, which are unreliable on some CIFS servers. The lock
//! is <root>/.tagstore/lock/; an owner.json inside records who holds it,
//! for diagnostics only. Staleness is judged from the lock directory mtime,
//! which the SMB server assigns, so it is consistent across client machines
//! even with minor clock skew; the 10 minute threshold dwarfs the
//! sub-second hold time of any tag operation.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const STALE_AFTER: Duration = Duration::from_secs(600);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// RAII guard: the lock directory is removed on drop.
#[derive(Debug)]
pub struct StoreLock {
    lock_dir: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.lock_dir);
    }
}

fn owner_info(lock_dir: &Path) -> String {
    match std::fs::read_to_string(lock_dir.join("owner.json")) {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => format!(
                "held by {} pid {} since {}",
                v.get("host").and_then(|x| x.as_str()).unwrap_or("?"),
                v.get("pid").and_then(|x| x.as_u64()).unwrap_or(0),
                v.get("acquired").and_then(|x| x.as_str()).unwrap_or("?"),
            ),
            Err(_) => "owner unknown".to_string(),
        },
        Err(_) => "owner unknown".to_string(),
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

pub fn acquire(
    store_dir: &Path,
    timeout: Duration,
    stale_after: Duration,
) -> Result<StoreLock, String> {
    let lock_dir = store_dir.join("lock");
    let deadline = Instant::now() + timeout;
    loop {
        match std::fs::create_dir(&lock_dir) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let age = std::fs::metadata(&lock_dir)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|mtime| SystemTime::now().duration_since(mtime).ok());
                match age {
                    None => continue, // released between mkdir and stat; retry immediately
                    Some(age) if age > stale_after => {
                        eprintln!(
                            "archnav: breaking stale tag store lock ({}, idle {}s)",
                            owner_info(&lock_dir),
                            age.as_secs()
                        );
                        let _ = std::fs::remove_dir_all(&lock_dir);
                        continue;
                    }
                    Some(_) => {
                        if Instant::now() >= deadline {
                            return Err(format!(
                                "store is locked ({}); retry, or remove {} if you are certain \
                                 no other tag writer is running",
                                owner_info(&lock_dir),
                                lock_dir.display()
                            ));
                        }
                        std::thread::sleep(POLL_INTERVAL);
                    }
                }
            }
            Err(e) => return Err(format!("cannot create {}: {}", lock_dir.display(), e)),
        }
    }
    let owner = serde_json::json!({
        "host": hostname(),
        "pid": std::process::id(),
        "acquired": format!("{:?}", SystemTime::now()),
    });
    let _ = std::fs::write(lock_dir.join("owner.json"), owner.to_string());
    Ok(StoreLock { lock_dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_dir = tmp.path().join("lock");
        {
            let _lock = acquire(tmp.path(), DEFAULT_TIMEOUT, STALE_AFTER).unwrap();
            assert!(lock_dir.is_dir());
            assert!(lock_dir.join("owner.json").is_file());
        }
        assert!(!lock_dir.exists()); // released on drop
    }

    #[test]
    fn contention_times_out() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("lock")).unwrap();
        let start = Instant::now();
        let err = acquire(tmp.path(), Duration::from_millis(300), STALE_AFTER).unwrap_err();
        assert!(err.contains("store is locked"));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn stale_lock_is_broken() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_dir = tmp.path().join("lock");
        std::fs::create_dir(&lock_dir).unwrap();
        // Backdate the lock dir mtime far past the stale threshold.
        let old = std::time::SystemTime::now() - Duration::from_secs(10_000);
        let times = libc_utime(&lock_dir, old);
        assert!(times.is_ok());
        let lock = acquire(tmp.path(), Duration::from_secs(2), STALE_AFTER);
        assert!(lock.is_ok());
    }

    fn libc_utime(path: &Path, t: SystemTime) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as libc::time_t;
        let times = [
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: secs,
                tv_nsec: 0,
            },
        ];
        let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let rc = unsafe { libc::utimensat(libc::AT_FDCWD, cpath.as_ptr(), times.as_ptr(), 0) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}
