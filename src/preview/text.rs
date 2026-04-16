use super::format_size;
use pulldown_cmark::{html, Options, Parser};
use std::path::Path;

const MAX_PREVIEW_BYTES: usize = 50_000;

/// Preview a markdown file, converting to HTML for rich rendering.
pub fn preview_markdown(path: &str, content_width: u32) -> String {
    let content = preview_text(path);
    let base_dir = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    markdown_to_html(&content, &base_dir, content_width)
}

/// Convert markdown to HTML with image width set to fit container.
fn markdown_to_html(markdown: &str, base_dir: &str, content_width: u32) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Set image widths and resolve relative paths
    let html_output = process_images(&html_output, base_dir, content_width);

    format!(r#"<html><body style="font-family: sans-serif;">{}</body></html>"#, html_output)
}

/// Process images: set width and resolve relative paths.
fn process_images(html: &str, base_dir: &str, content_width: u32) -> String {
    let mut result = html.replace("<img ", &format!(r#"<img width="{}" "#, content_width));

    if !base_dir.is_empty() {
        let mut output = String::with_capacity(result.len());
        let mut remaining = result.as_str();

        while let Some(src_pos) = remaining.find("src=\"") {
            output.push_str(&remaining[..src_pos + 5]);
            remaining = &remaining[src_pos + 5..];

            if let Some(end_quote) = remaining.find('"') {
                let src_value = &remaining[..end_quote];
                if !src_value.starts_with("http://")
                    && !src_value.starts_with("https://")
                    && !src_value.starts_with('/')
                    && !src_value.starts_with("file:")
                {
                    output.push_str(&format!("{}/{}", base_dir, src_value));
                } else {
                    output.push_str(src_value);
                }
                remaining = &remaining[end_quote..];
            }
        }
        output.push_str(remaining);
        result = output;
    }

    result
}

/// Preview a text file: read first 50KB, detect binary content.
pub fn preview_text(path: &str) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            // Check if content looks binary (high ratio of non-printable bytes)
            let check_len = bytes.len().min(512);
            let non_printable = bytes[..check_len]
                .iter()
                .filter(|&&b| b == 0 || (b < 32 && b != b'\n' && b != b'\r' && b != b'\t'))
                .count();

            if check_len > 0 && non_printable * 10 > check_len {
                // Looks binary
                let size = format_size(bytes.len() as u64);
                return format!(
                    "Binary file ({})\n\nThis file appears to contain binary data.",
                    size
                );
            }

            let limit = MAX_PREVIEW_BYTES.min(bytes.len());
            let content = String::from_utf8_lossy(&bytes[..limit]);

            if bytes.len() > MAX_PREVIEW_BYTES {
                format!(
                    "{}\n\n--- Truncated (showing 50KB of {}) ---",
                    content,
                    format_size(bytes.len() as u64)
                )
            } else {
                content.to_string()
            }
        }
        Err(e) => format!("Unable to read file: {}", e),
    }
}
