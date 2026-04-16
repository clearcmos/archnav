use std::process::Command;

/// Preview audio file: ffprobe metadata + album art extraction.
/// Returns (text_info, album_art_path).
pub fn preview_audio(path: &str) -> (String, String) {
    let text = ffprobe_info(path);
    let album_art = extract_album_art(path);
    (text, album_art)
}

/// Preview video file: ffprobe metadata.
pub fn preview_video(path: &str) -> String {
    ffprobe_info(path)
}

/// Run ffprobe and format output as human-readable metadata.
fn ffprobe_info(path: &str) -> String {
    let output = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            path,
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let json_str = String::from_utf8_lossy(&out.stdout);
            format_ffprobe_json(&json_str, path)
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            format!("ffprobe error: {}", stderr.trim())
        }
        Err(_) => "ffprobe not available. Install ffmpeg for media previews.".to_string(),
    }
}

/// Parse ffprobe JSON and format as readable text.
fn format_ffprobe_json(json: &str, path: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json);
    let v = match parsed {
        Ok(v) => v,
        Err(_) => return format!("Media file: {}", path),
    };

    let mut lines = Vec::new();

    // Format info
    if let Some(format) = v.get("format") {
        if let Some(filename) = format.get("filename").and_then(|v| v.as_str()) {
            let name = std::path::Path::new(filename)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(filename);
            lines.push(format!("File: {}", name));
        }

        if let Some(format_name) = format.get("format_long_name").and_then(|v| v.as_str()) {
            lines.push(format!("Format: {}", format_name));
        }

        if let Some(duration) = format.get("duration").and_then(|v| v.as_str()) {
            if let Ok(secs) = duration.parse::<f64>() {
                let mins = (secs / 60.0).floor() as u64;
                let remaining = (secs % 60.0).floor() as u64;
                lines.push(format!("Duration: {}:{:02}", mins, remaining));
            }
        }

        if let Some(size) = format.get("size").and_then(|v| v.as_str()) {
            if let Ok(bytes) = size.parse::<u64>() {
                lines.push(format!("Size: {}", super::format_size(bytes)));
            }
        }

        if let Some(bit_rate) = format.get("bit_rate").and_then(|v| v.as_str()) {
            if let Ok(bps) = bit_rate.parse::<u64>() {
                lines.push(format!("Bitrate: {} kbps", bps / 1000));
            }
        }

        // Tags (title, artist, album, etc.)
        if let Some(tags) = format.get("tags").and_then(|v| v.as_object()) {
            lines.push(String::new());
            for key in ["title", "artist", "album", "genre", "date", "track", "comment"] {
                // Try both lowercase and uppercase versions
                let val = tags
                    .get(key)
                    .or_else(|| tags.get(&key.to_uppercase()))
                    .and_then(|v| v.as_str());
                if let Some(val) = val {
                    lines.push(format!(
                        "{}: {}",
                        key.chars().next().unwrap().to_uppercase().to_string() + &key[1..],
                        val
                    ));
                }
            }
        }
    }

    // Stream info
    if let Some(streams) = v.get("streams").and_then(|v| v.as_array()) {
        lines.push(String::new());
        lines.push("Streams:".to_string());

        for stream in streams {
            let codec_type = stream
                .get("codec_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let codec_name = stream
                .get("codec_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            match codec_type {
                "audio" => {
                    let sample_rate = stream
                        .get("sample_rate")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let channels = stream
                        .get("channels")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    lines.push(format!(
                        "  Audio: {} ({}Hz, {} ch)",
                        codec_name, sample_rate, channels
                    ));
                }
                "video" => {
                    let width = stream
                        .get("width")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let height = stream
                        .get("height")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let fps = stream
                        .get("r_frame_rate")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    lines.push(format!(
                        "  Video: {} ({}x{}, {})",
                        codec_name, width, height, fps
                    ));
                }
                "subtitle" => {
                    let lang = stream
                        .get("tags")
                        .and_then(|t| t.get("language"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    lines.push(format!("  Subtitle: {} ({})", codec_name, lang));
                }
                _ => {}
            }
        }
    }

    if lines.is_empty() {
        format!("Media file: {}", path)
    } else {
        lines.join("\n")
    }
}

/// Extract album art from audio file to a temp file. Returns path or empty string.
fn extract_album_art(path: &str) -> String {
    let temp_dir = std::env::temp_dir().join("archnav-art");
    std::fs::create_dir_all(&temp_dir).ok();

    let output_path = temp_dir.join("album_art.jpg");

    let result = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            path,
            "-an",
            "-vcodec",
            "mjpeg",
            "-frames:v",
            "1",
            output_path.to_str().unwrap_or(""),
        ])
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status();

    match result {
        Ok(status) if status.success() && output_path.exists() => {
            output_path.to_string_lossy().to_string()
        }
        _ => String::new(),
    }
}
