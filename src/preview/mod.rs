pub mod archive;
pub mod directory;
pub mod media;
pub mod text;

use std::path::Path;

/// Result of generating a preview for a file.
pub struct PreviewResult {
    /// Type of preview: "text", "directory", "image", "pdf", "audio", "video", "archive", "binary", "none"
    pub preview_type: String,
    /// Text content for text/directory/archive/media previews
    pub text: String,
    /// Path to image for image previews (or album art)
    pub image_path: String,
}

/// Generate a preview for the given path.
/// `content_width` is used for sizing images in markdown.
pub fn generate_preview(path: &str, is_dir: bool, content_width: u32) -> PreviewResult {
    if is_dir {
        return PreviewResult {
            preview_type: "directory".to_string(),
            text: directory::preview_directory(path),
            image_path: String::new(),
        };
    }

    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Images — handled by QML directly
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tiff" | "ico" => PreviewResult {
            preview_type: "image".to_string(),
            text: image_info(path),
            image_path: path.to_string(),
        },

        // PDF — render pages with pdftoppm
        "pdf" => {
            let text = pdf_info(path);
            PreviewResult {
                preview_type: "pdf".to_string(),
                text,
                image_path: String::new(),
            }
        }

        // Audio
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "wma" => {
            let (text, album_art) = media::preview_audio(path);
            PreviewResult {
                preview_type: "audio".to_string(),
                text,
                image_path: album_art,
            }
        }

        // Video
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" => {
            let text = media::preview_video(path);
            PreviewResult {
                preview_type: "video".to_string(),
                text,
                image_path: String::new(),
            }
        }

        // Archives
        "zip" | "jar" | "war" | "apk" => PreviewResult {
            preview_type: "archive".to_string(),
            text: archive::preview_zip(path),
            image_path: String::new(),
        },
        "tar" => PreviewResult {
            preview_type: "archive".to_string(),
            text: archive::preview_tar(path, None),
            image_path: String::new(),
        },
        "gz" | "tgz" => PreviewResult {
            preview_type: "archive".to_string(),
            text: archive::preview_tar(path, Some("gz")),
            image_path: String::new(),
        },
        "bz2" => PreviewResult {
            preview_type: "archive".to_string(),
            text: archive::preview_tar(path, Some("bz2")),
            image_path: String::new(),
        },
        "xz" => PreviewResult {
            preview_type: "archive".to_string(),
            text: archive::preview_tar(path, Some("xz")),
            image_path: String::new(),
        },
        "7z" | "rar" | "zst" => PreviewResult {
            preview_type: "archive".to_string(),
            text: archive::preview_subprocess(path),
            image_path: String::new(),
        },

        // Binary types — show file info
        "exe" | "dll" | "so" | "dylib" | "o" | "obj" | "a" | "bin" | "dat" | "db" | "sqlite"
        | "sqlite3" | "class" | "pyc" | "pyo" | "whl" | "ttf" | "otf" | "woff" | "woff2" => {
            PreviewResult {
                preview_type: "binary".to_string(),
                text: binary_info(path),
                image_path: String::new(),
            }
        }

        // Markdown
        "md" | "markdown" => PreviewResult {
            preview_type: "markdown".to_string(),
            text: text::preview_markdown(path, content_width),
            image_path: String::new(),
        },

        // Default — try as text
        _ => PreviewResult {
            preview_type: "text".to_string(),
            text: text::preview_text(path),
            image_path: String::new(),
        },
    }
}

fn image_info(path: &str) -> String {
    let meta = std::fs::metadata(path);
    let size_str = meta
        .as_ref()
        .map(|m| format_size(m.len()))
        .unwrap_or_else(|_| "unknown".to_string());

    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);

    format!("{}\nSize: {}", filename, size_str)
}

fn pdf_info(path: &str) -> String {
    // Use pdfinfo if available for page count
    let output = std::process::Command::new("pdfinfo").arg(path).output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => {
            let meta = std::fs::metadata(path);
            let size = meta.map(|m| format_size(m.len())).unwrap_or_default();
            format!("PDF Document\nSize: {}", size)
        }
    }
}

fn binary_info(path: &str) -> String {
    let meta = std::fs::metadata(path);
    let size_str = meta
        .as_ref()
        .map(|m| format_size(m.len()))
        .unwrap_or_else(|_| "unknown".to_string());

    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);

    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown");

    format!(
        "Binary File\n\nName: {}\nType: {}\nSize: {}",
        filename, ext, size_str
    )
}

pub fn format_size(bytes: u64) -> String {
    // Binary units, labeled as such (matches Style.formatSize in QML).
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    // Dispatch tests cover the extension routing; media and pdf types are
    // exercised elsewhere (parse tests) because they shell out to
    // ffprobe/pdfinfo, which CI does not install.

    #[test]
    fn dispatches_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let r = generate_preview(tmp.path().to_str().unwrap(), true, 400);
        assert_eq!(r.preview_type, "directory");
        assert!(r.text.contains("0 items"));
    }

    #[test]
    fn dispatches_image_with_path_for_qml() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("pic.png");
        std::fs::write(&p, b"fake").unwrap();
        let r = generate_preview(p.to_str().unwrap(), false, 400);
        assert_eq!(r.preview_type, "image");
        assert_eq!(r.image_path, p.to_str().unwrap());
        assert!(r.text.contains("pic.png"));
    }

    #[test]
    fn dispatches_binary_info() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("lib.so");
        std::fs::write(&p, b"\x7fELF").unwrap();
        let r = generate_preview(p.to_str().unwrap(), false, 400);
        assert_eq!(r.preview_type, "binary");
        assert!(r.text.contains("Binary File"));
        assert!(r.text.contains("Type: so"));
    }

    #[test]
    fn dispatches_unknown_extension_as_text() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("notes.conf");
        std::fs::write(&p, b"key=value\n").unwrap();
        let r = generate_preview(p.to_str().unwrap(), false, 400);
        assert_eq!(r.preview_type, "text");
        assert!(r.text.contains("key=value"));
    }

    #[test]
    fn dispatches_zip_as_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.zip");
        std::fs::write(&p, b"not really").unwrap();
        let r = generate_preview(p.to_str().unwrap(), false, 400);
        assert_eq!(r.preview_type, "archive");
    }
}
