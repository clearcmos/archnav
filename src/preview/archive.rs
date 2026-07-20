use std::io::Read;
use std::path::Path;

const MAX_ENTRIES: usize = 50;

/// Preview a ZIP archive using the zip crate.
pub fn preview_zip(path: &str) -> String {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return format!("Unable to open archive: {}", e),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return format!("Unable to read ZIP archive: {}", e),
    };

    let total = archive.len();
    let mut lines = Vec::with_capacity(MAX_ENTRIES + 2);
    lines.push(format!("ZIP Archive: {} entries\n", total));

    for i in 0..total.min(MAX_ENTRIES) {
        if let Ok(entry) = archive.by_index(i) {
            let size = super::format_size(entry.size());
            let name = entry.name();
            if entry.is_dir() {
                lines.push(format!("  [dir]     {}", name));
            } else {
                lines.push(format!("  {:>8}  {}", size, name));
            }
        }
    }

    if total > MAX_ENTRIES {
        lines.push(format!("\n... and {} more entries", total - MAX_ENTRIES));
    }

    lines.join("\n")
}

/// Preview a tar archive (optionally compressed with gz/bz2/xz).
pub fn preview_tar(path: &str, compression: Option<&str>) -> String {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return format!("Unable to open archive: {}", e),
    };

    let reader: Box<dyn Read> = match compression {
        Some("gz") => Box::new(flate2::read::GzDecoder::new(file)),
        Some("bz2") => {
            // bzip2 not available via flate2, use subprocess
            return preview_subprocess(path);
        }
        Some("xz") => {
            // xz not directly available, use subprocess
            return preview_subprocess(path);
        }
        _ => Box::new(file),
    };

    let mut archive = tar::Archive::new(reader);
    let entries = match archive.entries() {
        Ok(e) => e,
        Err(e) => return format!("Unable to read tar archive: {}", e),
    };

    let mut lines = Vec::new();
    let mut count = 0;

    let comp_label = match compression {
        Some("gz") => "tar.gz",
        Some("bz2") => "tar.bz2",
        Some("xz") => "tar.xz",
        _ => "tar",
    };
    lines.push(format!("{} Archive:\n", comp_label.to_uppercase()));

    for entry in entries.filter_map(|e| e.ok()) {
        if count >= MAX_ENTRIES {
            break;
        }

        let path_str = entry
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "???".to_string());

        let size = entry.size();
        let is_dir = entry.header().entry_type().is_dir();

        if is_dir {
            lines.push(format!("  [dir]     {}", path_str));
        } else {
            lines.push(format!("  {:>8}  {}", super::format_size(size), path_str));
        }

        count += 1;
    }

    if count == 0 {
        lines.push("  (empty archive)".to_string());
    } else if count >= MAX_ENTRIES {
        lines.push(format!("\n... showing first {} entries", MAX_ENTRIES));
    }

    lines.join("\n")
}

/// Preview archives that need external tools (7z, rar, zst).
pub fn preview_subprocess(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (cmd, args): (&str, Vec<&str>) = match ext.as_str() {
        "7z" => ("7z", vec!["l", path]),
        "rar" => ("unrar", vec!["l", path]),
        "zst" => {
            // For .tar.zst, try zstd + tar
            let child = std::process::Command::new("zstd")
                .args(["-d", "-c", path])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn();

            return match child {
                Ok(mut child) => {
                    let text = match child.stdout.take() {
                        Some(stdout) => {
                            let mut archive = tar::Archive::new(stdout);
                            let mut lines = vec!["ZSTD Archive:\n".to_string()];
                            let mut count = 0;
                            if let Ok(entries) = archive.entries() {
                                for entry in entries.filter_map(|e| e.ok()) {
                                    if count >= MAX_ENTRIES {
                                        break;
                                    }
                                    let p = entry
                                        .path()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_else(|_| "???".to_string());
                                    let size = entry.size();
                                    if entry.header().entry_type().is_dir() {
                                        lines.push(format!("  [dir]     {}", p));
                                    } else {
                                        lines.push(format!(
                                            "  {:>8}  {}",
                                            super::format_size(size),
                                            p
                                        ));
                                    }
                                    count += 1;
                                }
                            }
                            lines.join("\n")
                        }
                        None => "Unable to read zst archive output".to_string(),
                    };
                    // We stop reading after MAX_ENTRIES; reap the decompressor
                    // so it does not linger as a zombie process.
                    let _ = child.kill();
                    let _ = child.wait();
                    text
                }
                Err(e) => format!("Unable to read zst archive: {}", e),
            };
        }
        _ => return format!("Unsupported archive format: .{}", ext),
    };

    let output = std::process::Command::new(cmd).args(&args).output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            // Limit output
            let lines: Vec<&str> = text.lines().take(MAX_ENTRIES + 5).collect();
            lines.join("\n")
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            format!("Error listing archive: {}", stderr.trim())
        }
        Err(_) => format!(
            "{} not available. Install {} for {} archive previews.",
            cmd, cmd, ext
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_fixture(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("fixture.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions = Default::default();
        w.add_directory("sub/", opts).unwrap();
        w.start_file("hello.txt", opts).unwrap();
        w.write_all(b"hello world").unwrap();
        w.finish().unwrap();
        path
    }

    fn tar_bytes() -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        let data = b"tar content";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        b.append_data(&mut header, "inner/file.txt", data.as_slice())
            .unwrap();
        b.into_inner().unwrap()
    }

    #[test]
    fn zip_preview_lists_entries_and_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = zip_fixture(tmp.path());
        let out = preview_zip(path.to_str().unwrap());
        assert!(out.contains("ZIP Archive: 2 entries"));
        assert!(out.contains("[dir]     sub/"));
        assert!(out.contains("11 B") && out.contains("hello.txt"));
    }

    #[test]
    fn zip_preview_reports_open_and_format_errors() {
        assert!(preview_zip("/nonexistent/a.zip").starts_with("Unable to open archive:"));
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("bad.zip");
        std::fs::write(&bad, b"not a zip").unwrap();
        assert!(preview_zip(bad.to_str().unwrap()).starts_with("Unable to read ZIP archive:"));
    }

    #[test]
    fn tar_preview_lists_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fixture.tar");
        std::fs::write(&path, tar_bytes()).unwrap();
        let out = preview_tar(path.to_str().unwrap(), None);
        assert!(out.starts_with("TAR Archive:"));
        assert!(out.contains("11 B") && out.contains("inner/file.txt"));
    }

    #[test]
    fn tar_gz_preview_decompresses() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fixture.tar.gz");
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        enc.write_all(&tar_bytes()).unwrap();
        enc.finish().unwrap();
        let out = preview_tar(path.to_str().unwrap(), Some("gz"));
        assert!(out.starts_with("TAR.GZ Archive:"));
        assert!(out.contains("inner/file.txt"));
    }

    #[test]
    fn empty_tar_is_labeled_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.tar");
        let b = tar::Builder::new(Vec::new());
        std::fs::write(&path, b.into_inner().unwrap()).unwrap();
        let out = preview_tar(path.to_str().unwrap(), None);
        assert!(out.contains("(empty archive)"));
    }

    #[test]
    fn unsupported_subprocess_extension_is_reported() {
        assert_eq!(
            preview_subprocess("/tmp/whatever.foo"),
            "Unsupported archive format: .foo"
        );
    }
}
