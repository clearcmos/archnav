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

            dirs.sort_by_key(|a| a.to_lowercase());
            files.sort_by_key(|a| a.to_lowercase());

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_dirs_first_with_sizes_and_count() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("zsub")).unwrap();
        std::fs::write(tmp.path().join("afile.txt"), b"12345").unwrap();
        let out = preview_directory(tmp.path().to_str().unwrap());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "2 items");
        // Directories come first even when files sort earlier alphabetically
        assert!(lines[2].contains("[dir]  zsub/"), "got: {}", lines[2]);
        assert!(lines[3].contains("5 B") && lines[3].contains("afile.txt"));
    }

    #[test]
    fn truncates_past_max_entries() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..(MAX_ENTRIES + 5) {
            std::fs::write(tmp.path().join(format!("f{:03}.txt", i)), b"x").unwrap();
        }
        let out = preview_directory(tmp.path().to_str().unwrap());
        assert!(out.contains(&format!("{} items", MAX_ENTRIES + 5)));
        assert!(out.contains("... and 5 more items"));
    }

    #[test]
    fn unreadable_directory_reports_error() {
        let out = preview_directory("/nonexistent/archnav-test-dir");
        assert!(out.starts_with("Unable to read directory:"));
    }
}
