#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, status_text)]
        #[qproperty(bool, engine_ready)]
        #[qproperty(i32, result_count)]
        #[qproperty(i32, total_indexed)]
        #[qproperty(i32, search_time_ms)]
        #[qproperty(i32, bookmark_count)]
        type SearchEngine = super::SearchEngineRust;

        // Signals
        #[qsignal]
        fn resultsReady(self: Pin<&mut SearchEngine>);

        #[qsignal]
        fn bookmarksChanged(self: Pin<&mut SearchEngine>);

        #[qsignal]
        fn rescanComplete(self: Pin<&mut SearchEngine>);

        #[qsignal]
        fn toggleRequested(self: Pin<&mut SearchEngine>);

        #[qsignal]
        fn exitRequested(self: Pin<&mut SearchEngine>);

        // Engine lifecycle
        #[qinvokable]
        fn initialize(self: Pin<&mut SearchEngine>);

        // Search
        #[qinvokable]
        fn search(self: Pin<&mut SearchEngine>, query: QString, sort_index: i32);

        // Bookmark management
        #[qinvokable]
        fn add_bookmark(
            self: Pin<&mut SearchEngine>,
            name: QString,
            path: QString,
            is_network: bool,
        );

        #[qinvokable]
        fn remove_bookmark(self: Pin<&mut SearchEngine>, name: QString);

        #[qinvokable]
        fn rename_bookmark(self: Pin<&mut SearchEngine>, old_name: QString, new_name: QString);

        #[qinvokable]
        fn rescan_all(self: Pin<&mut SearchEngine>);

        // Bookmark accessors
        #[qinvokable]
        fn bookmark_name_at(self: &SearchEngine, index: i32) -> QString;

        #[qinvokable]
        fn bookmark_path_at(self: &SearchEngine, index: i32) -> QString;

        // Result accessors (QML reads these after results_ready signal)
        #[qinvokable]
        fn result_path_at(self: &SearchEngine, row: i32) -> QString;

        #[qinvokable]
        fn result_is_dir_at(self: &SearchEngine, row: i32) -> bool;

        #[qinvokable]
        fn result_mtime_at(self: &SearchEngine, row: i32) -> i64;

        #[qinvokable]
        fn result_size_at(self: &SearchEngine, row: i32) -> i64;

        #[qinvokable]
        fn result_bookmark_at(self: &SearchEngine, row: i32) -> QString;

        #[qinvokable]
        fn result_filename_at(self: &SearchEngine, row: i32) -> QString;

        // File operations
        #[qinvokable]
        fn open_file(self: &SearchEngine, path: QString);

        #[qinvokable]
        fn open_folder(self: &SearchEngine, path: QString);

        // Context menu
        #[qinvokable]
        fn show_context_menu(self: &SearchEngine, path: QString, x: i32, y: i32);
    }

    impl cxx_qt::Threading for SearchEngine {}
}

use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use tracing::info;

use crate::search::engine::CoreEngine;
use crate::search::trigram::{Bookmark, SearchAllResult};

/// Rust backing struct for the SearchEngine QObject.
pub struct SearchEngineRust {
    // Qt properties
    status_text: QString,
    engine_ready: bool,
    result_count: i32,
    total_indexed: i32,
    search_time_ms: i32,
    bookmark_count: i32,

    // Internal state (not exposed as Qt properties)
    inner: Option<Arc<CoreEngine>>,
    results: Vec<SearchAllResult>,
    /// Sequence counter to discard stale search results
    search_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Default for SearchEngineRust {
    fn default() -> Self {
        Self {
            status_text: QString::from("Initializing..."),
            engine_ready: false,
            result_count: 0,
            total_indexed: 0,
            search_time_ms: 0,
            bookmark_count: 0,
            inner: None,
            results: Vec::new(),
            search_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}

impl qobject::SearchEngine {
    /// Initialize the search engine: start toggle server immediately, then load
    /// DB and start watchers in a background thread so the window appears instantly.
    fn initialize(mut self: Pin<&mut Self>) {
        info!("Initializing search engine");

        self.as_mut().set_status_text(QString::from("Loading index..."));

        // Start toggle socket server immediately (needs to work before engine loads)
        let qt_thread = self.qt_thread();
        crate::toggle::start_toggle_server(qt_thread.clone());

        // Move heavy initialization to background thread
        std::thread::spawn(move || {
            // Load bookmarks from config
            let config = crate::config::AppConfig::load();
            let bookmarks = config.to_bookmarks();
            let default_bookmarks = if bookmarks.is_empty() {
                if let Some(home) = dirs::home_dir() {
                    vec![Bookmark {
                        name: "home".to_string(),
                        path: home.to_string_lossy().to_string(),
                        is_network: false,
                    }]
                } else {
                    Vec::new()
                }
            } else {
                bookmarks
            };

            let engine = Arc::new(CoreEngine::new(default_bookmarks));
            let file_count = engine.file_count() as i32;
            let bookmark_count = engine.bookmarks().len() as i32;

            // Start IPC server for external tools
            crate::ipc::start_ipc_server(engine.clone());

            info!("Search engine initialized with {} files", file_count);

            // Queue results back to Qt main thread
            let _ = qt_thread.queue(move |mut qobj| {
                qobj.as_mut().rust_mut().inner = Some(engine);
                qobj.as_mut().set_total_indexed(file_count);
                qobj.as_mut().set_bookmark_count(bookmark_count);
                qobj.as_mut().set_engine_ready(true);
                qobj.as_mut().set_status_text(QString::from(
                    &format!("{} files indexed", file_count),
                ));
            });
        });
    }

    /// Perform a search in a background thread. Emits results_ready when done.
    fn search(self: Pin<&mut Self>, query: QString, sort_index: i32) {
        use std::sync::atomic::Ordering;
        use tracing::debug;

        let inner = match self.rust().inner.as_ref() {
            Some(inner) => inner.clone(),
            None => return,
        };

        let query_str = query.to_string();
        let qt_thread = self.qt_thread();

        // Increment sequence counter and capture it for this search
        let seq_counter = self.rust().search_seq.clone();
        let my_seq = seq_counter.fetch_add(1, Ordering::SeqCst) + 1;

        debug!("[BRIDGE] search called: query='{}', seq={}", query_str, my_seq);

        std::thread::spawn(move || {
            let (results, elapsed) = inner.search(&query_str, sort_index);
            let result_count = results.len() as i32;
            let search_time = elapsed.as_millis() as i32;
            let total_indexed = inner.file_count() as i32;

            debug!("[BRIDGE] search complete: query='{}', seq={}, results={}", query_str, my_seq, result_count);

            let query_str_clone = query_str.clone();
            let _ = qt_thread.queue(move |mut qobj| {
                // Only update if this is still the most recent search
                let current_seq = seq_counter.load(Ordering::SeqCst);
                if my_seq < current_seq {
                    // A newer search was started, discard these stale results
                    debug!("[BRIDGE] discarding stale: query='{}', seq={} < current={}", query_str_clone, my_seq, current_seq);
                    return;
                }

                debug!("[BRIDGE] updating UI: query='{}', seq={}, results={}", query_str_clone, my_seq, result_count);
                qobj.as_mut().rust_mut().results = results;
                qobj.as_mut().set_result_count(result_count);
                qobj.as_mut().set_search_time_ms(search_time);
                qobj.as_mut().set_total_indexed(total_indexed);
                qobj.as_mut().set_status_text(QString::from(
                    &format!("{} results in {}ms", result_count, search_time),
                ));
                qobj.as_mut().resultsReady();
            });
        });
    }

    fn add_bookmark(mut self: Pin<&mut Self>, name: QString, path: QString, is_network: bool) {
        let inner = match self.rust().inner.as_ref() {
            Some(inner) => inner.clone(),
            None => return,
        };

        inner.add_bookmark(&name.to_string(), &path.to_string(), is_network);
        let count = inner.bookmarks().len() as i32;
        self.as_mut().set_bookmark_count(count);
        self.as_mut().bookmarksChanged();
    }

    fn remove_bookmark(mut self: Pin<&mut Self>, name: QString) {
        let inner = match self.rust().inner.as_ref() {
            Some(inner) => inner.clone(),
            None => return,
        };

        inner.remove_bookmark(&name.to_string());
        let count = inner.bookmarks().len() as i32;
        self.as_mut().set_bookmark_count(count);
        self.as_mut().bookmarksChanged();
    }

    fn rename_bookmark(mut self: Pin<&mut Self>, old_name: QString, new_name: QString) {
        if let Some(inner) = self.rust().inner.as_ref() {
            inner.rename_bookmark(&old_name.to_string(), &new_name.to_string());
        }
        self.as_mut().bookmarksChanged();
    }

    /// Rescan all bookmarks in background thread. Emits rescan_complete when done.
    fn rescan_all(self: Pin<&mut Self>) {
        let inner = match self.rust().inner.as_ref() {
            Some(inner) => inner.clone(),
            None => return,
        };

        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            inner.rescan_all();
            let file_count = inner.file_count() as i32;

            let _ = qt_thread.queue(move |mut qobj| {
                qobj.as_mut().set_total_indexed(file_count);
                qobj.as_mut().set_status_text(QString::from(
                    &format!("Rescan complete: {} files", file_count),
                ));
                qobj.as_mut().rescanComplete();
            });
        });
    }

    fn bookmark_name_at(&self, index: i32) -> QString {
        self.rust()
            .inner
            .as_ref()
            .and_then(|e| {
                e.bookmarks()
                    .get(index as usize)
                    .map(|b| QString::from(&*b.name))
            })
            .unwrap_or_default()
    }

    fn bookmark_path_at(&self, index: i32) -> QString {
        self.rust()
            .inner
            .as_ref()
            .and_then(|e| {
                e.bookmarks()
                    .get(index as usize)
                    .map(|b| QString::from(&*b.path))
            })
            .unwrap_or_default()
    }

    fn result_path_at(&self, row: i32) -> QString {
        self.rust()
            .results
            .get(row as usize)
            .map(|r| QString::from(&*r.path))
            .unwrap_or_default()
    }

    fn result_is_dir_at(&self, row: i32) -> bool {
        self.rust()
            .results
            .get(row as usize)
            .map(|r| r.is_dir)
            .unwrap_or(false)
    }

    fn result_mtime_at(&self, row: i32) -> i64 {
        self.rust()
            .results
            .get(row as usize)
            .map(|r| r.mtime)
            .unwrap_or(0)
    }

    fn result_size_at(&self, row: i32) -> i64 {
        self.rust()
            .results
            .get(row as usize)
            .map(|r| r.size as i64)
            .unwrap_or(0)
    }

    fn result_bookmark_at(&self, row: i32) -> QString {
        self.rust()
            .results
            .get(row as usize)
            .map(|r| QString::from(&*r.bookmark))
            .unwrap_or_default()
    }

    fn result_filename_at(&self, row: i32) -> QString {
        self.rust()
            .results
            .get(row as usize)
            .map(|r| {
                let path = std::path::Path::new(&r.path);
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&r.path);
                QString::from(name)
            })
            .unwrap_or_default()
    }

    /// Open a file using KIO::OpenUrlJob for proper Wayland focus handling.
    /// This uses the same mechanism as Dolphin for XDG activation tokens.
    fn open_file(&self, path: QString) {
        let path_str = path.to_string();

        // Record file open for frecency tracking
        if let Some(inner) = self.rust().inner.as_ref() {
            inner.record_file_open(&path_str);
        }

        // Use KIO::OpenUrlJob which handles XDG activation tokens for Wayland focus
        crate::file_opener::open_file(&path_str);
    }

    /// Open the containing folder in the default file manager.
    /// Uses KIO::OpenUrlJob for proper Wayland focus handling.
    fn open_folder(&self, path: QString) {
        let path_str = path.to_string();
        let parent = std::path::Path::new(&path_str)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path_str);
        // Use KIO::OpenUrlJob which handles XDG activation tokens for Wayland focus
        crate::file_opener::open_file(&parent);
    }

    /// Show a KDE context menu for the given file path.
    fn show_context_menu(&self, path: QString, x: i32, y: i32) {
        let path_str = path.to_string();
        // Context menu must be shown on the main thread, so we call it directly
        let menu = crate::context_menu::ContextMenu::new();
        menu.show(&path_str, x, y);
    }
}
