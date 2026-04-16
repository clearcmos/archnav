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
            let output = std::process::Command::new("zstd")
                .args(["-d", "-c", path])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .and_then(|child| {
                    let stdout = child.stdout.unwrap();
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
                    Ok(lines.join("\n"))
                });

            return match output {
                Ok(text) => text,
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
