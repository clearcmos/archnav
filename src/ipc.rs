use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::{fs, thread};
use tracing::{info, warn};

use crate::search::engine::CoreEngine;
use crate::search::query::{FileTypeMode, ParsedQuery, SortOrder};
use crate::search::trigram::{Bookmark, SearchResult};

/// IPC request types for backward compatibility with the old daemon protocol.
#[derive(serde::Deserialize)]
struct SearchRequest {
    bookmark_path: String,
    #[serde(default)]
    mode: String,
    query: String,
    extension: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(serde::Deserialize)]
struct SearchAllRequest {
    #[serde(default)]
    bookmark_paths: Vec<String>,
    query: String,
    extension: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

/// A SEARCH_ALL result: the match and the name of the bookmark it is under.
#[derive(serde::Serialize)]
struct SearchAllEntry<'a> {
    #[serde(flatten)]
    result: &'a SearchResult,
    bookmark: &'a str,
}

/// Results per response when a request names no `limit`; 0 asks for all.
fn default_limit() -> usize {
    500
}

fn socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string())
        + "/archnav-daemon.sock"
}

/// Start the IPC server thread. Maintains the same protocol as the old daemon.
pub fn start_ipc_server(engine: Arc<CoreEngine>) {
    thread::spawn(move || {
        let path = socket_path();
        let _ = fs::remove_file(&path);

        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to bind IPC socket {}: {}", path, e);
                return;
            }
        };

        info!("IPC server listening on {}", path);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let engine = engine.clone();
                    thread::spawn(move || {
                        handle_client(stream, &engine);
                    });
                }
                Err(e) => {
                    warn!("IPC accept error: {}", e);
                }
            }
        }
    });
}

fn handle_client(stream: std::os::unix::net::UnixStream, engine: &CoreEngine) {
    let reader = BufReader::new(&stream);
    let mut writer = &stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let response = process_command(&line, engine);
        if writeln!(writer, "{}", response).is_err() {
            break;
        }
    }
}

fn process_command(line: &str, engine: &CoreEngine) -> String {
    if let Some(rest) = line.strip_prefix("SEARCH_ALL ") {
        handle_search_all(rest, engine)
    } else if let Some(rest) = line.strip_prefix("SEARCH ") {
        handle_search(rest, engine)
    } else if let Some(rest) = line.strip_prefix("ADD_BOOKMARK ") {
        handle_add_bookmark(rest, engine)
    } else if let Some(rest) = line.strip_prefix("RESCAN ") {
        handle_rescan(rest, engine)
    } else if line == "STATS" {
        handle_stats(engine)
    } else if line == "PING" {
        r#"{"status": "pong"}"#.to_string()
    } else {
        r#"{"error": "unknown command"}"#.to_string()
    }
}

fn handle_search(json: &str, engine: &CoreEngine) -> String {
    let req: SearchRequest = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(e) => return format!(r#"{{"error": "{}"}}"#, e),
    };

    let idx = engine.index.read().unwrap();

    let file_type_mode = match req.mode.as_str() {
        "edit" => FileTypeMode::Edit,
        "gotofile" => FileTypeMode::GotoFile,
        "gotodir" => FileTypeMode::GotoDir,
        _ => FileTypeMode::All,
    };

    let mut query = ParsedQuery::parse(&req.query, SortOrder::MtimeDesc);
    // An explicit "extension" field wins; otherwise keep any *.ext in the query
    if req.extension.is_some() {
        query.extension_filter = req.extension;
    }
    query.file_type_mode = file_type_mode;

    let start = std::time::Instant::now();
    let results = idx.search(&query, &req.bookmark_path, req.limit);
    let elapsed = start.elapsed().as_millis() as u64;
    let total = idx.file_count();

    let resp = serde_json::json!({
        "results": results,
        "total_indexed": total,
        "search_time_ms": elapsed,
    });

    serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
}

fn handle_search_all(json: &str, engine: &CoreEngine) -> String {
    let req: SearchAllRequest = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(e) => return format!(r#"{{"error": "{}"}}"#, e),
    };

    let idx = engine.index.read().unwrap();

    let mut query = ParsedQuery::parse(&req.query, SortOrder::MtimeDesc);
    // An explicit "extension" field wins; otherwise keep any *.ext in the query
    if req.extension.is_some() {
        query.extension_filter = req.extension;
    }

    let start = std::time::Instant::now();
    let (results, total_matches) = idx.search_all_counted(&query, &req.bookmark_paths, req.limit);
    let elapsed = start.elapsed().as_millis() as u64;
    let results: Vec<SearchAllEntry> = results
        .iter()
        .map(|result| SearchAllEntry {
            result,
            bookmark: idx
                .bookmark_name_of(&result.path, &req.bookmark_paths)
                .unwrap_or_default(),
        })
        .collect();
    let total = idx.file_count();

    let resp = serde_json::json!({
        "results": results,
        "total_matches": total_matches,
        "total_indexed": total,
        "search_time_ms": elapsed,
    });

    serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
}

fn handle_add_bookmark(json: &str, engine: &CoreEngine) -> String {
    let bookmark: Bookmark = match serde_json::from_str(json) {
        Ok(b) => b,
        Err(e) => return format!(r#"{{"error": "{}"}}"#, e),
    };

    engine.add_bookmark(&bookmark.name, &bookmark.path, bookmark.is_network);

    let count = engine.file_count();
    format!(r#"{{"status": "ok", "indexed": {}}}"#, count)
}

fn handle_rescan(path_str: &str, engine: &CoreEngine) -> String {
    let path = path_str.trim();
    let count = engine.rescan_path(path);
    format!(r#"{{"status": "ok", "indexed": {}}}"#, count)
}

fn handle_stats(engine: &CoreEngine) -> String {
    let idx = engine.index.read().unwrap();
    format!(
        r#"{{"files": {}, "trigrams": {}, "bookmarks": {}}}"#,
        idx.files.len(),
        idx.trigrams.len(),
        idx.bookmarks.len()
    )
}
