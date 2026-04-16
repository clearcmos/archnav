const MAX_ENTRIES: usize = 80;

/// Preview a directory: list contents, folders first, sorted.
pub fn preview_directory(path: &str) -> String {
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut dirs = Vec::new();
            let mut files = Vec::new();

            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let size = if is_dir {
                    0
                } else {
                    entry.metadata().map(|m| m.len()).unwrap_or(0)
                };

                if is_dir {
                    dirs.push(format!("[dir]  {}/", name));
                } else {
                    let size_str = super::format_size(size);
                    files.push(format!("{:>8}  {}", size_str, name));
                }
            }

            dirs.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
            files.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

            let total = dirs.len() + files.len();
            let mut lines: Vec<String> = Vec::with_capacity(total.min(MAX_ENTRIES) + 2);

            lines.push(format!("{} items\n", total));

            for d in dirs.iter().take(MAX_ENTRIES) {
                lines.push(d.clone());
            }
            let remaining = MAX_ENTRIES.saturating_sub(dirs.len());
            for f in files.iter().take(remaining) {
                lines.push(f.clone());
            }

            if total > MAX_ENTRIES {
                lines.push(format!("\n... and {} more items", total - MAX_ENTRIES));
            }

            lines.join("\n")
        }
        Err(e) => format!("Unable to read directory: {}", e),
    }
}
