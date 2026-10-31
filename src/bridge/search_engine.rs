// Qt signals are camelCase by convention; cxx-qt mirrors them in generated methods.
#![allow(non_snake_case)]

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel;
    }

    // The results are the model behind the QML list, so the view creates rows
    // only for what is on screen however many paths matched
    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        #[qml_element]
        #[qproperty(QString, status_text)]
        #[qproperty(bool, engine_ready)]
        #[qproperty(i32, result_count)]
        #[qproperty(i32, total_indexed)]
        #[qproperty(i32, search_time_ms)]
        #[qproperty(i32, bookmark_count)]
        // Rows picked with Ctrl/Shift; none means just the current row.
        // The revision changes with every edit, for QML bindings to follow.
        #[qproperty(i32, selection_count)]
        #[qproperty(i32, selection_revision)]
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

        // True if launched with --hidden; QML reads this to decide whether to
        // show the window at startup or stay minimized in the tray.
        #[qinvokable]
        fn start_hidden(self: &SearchEngine) -> bool;

        // Search
        #[qinvokable]
        fn search(self: Pin<&mut SearchEngine>, query: QString, sort_index: i32);

        // Empty the list, dropping any search still running
        #[qinvokable]
        fn clear_results(self: Pin<&mut SearchEngine>);

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

        // The selected row's path and type, for opening and previewing it
        #[qinvokable]
        fn result_path_at(self: &SearchEngine, row: i32) -> QString;

        #[qinvokable]
        fn result_is_dir_at(self: &SearchEngine, row: i32) -> bool;

        // Selection of result rows
        #[qinvokable]
        fn clear_selection(self: Pin<&mut SearchEngine>);

        #[qinvokable]
        fn toggle_selected(self: Pin<&mut SearchEngine>, row: i32);

        // Select the rows from `from` to `to`, keeping the earlier selection
        // with `keep`
        #[qinvokable]
        fn select_range(self: Pin<&mut SearchEngine>, from: i32, to: i32, keep: bool);

        #[qinvokable]
        fn is_selected(self: &SearchEngine, row: i32) -> bool;

        // Paths of the selected rows, in list order
        #[qinvokable]
        fn selected_paths(self: &SearchEngine) -> QStringList;

        // Clipboard and drag
        #[qinvokable]
        fn copy_files(self: &SearchEngine, paths: QStringList);

        #[qinvokable]
        fn copy_locations(self: &SearchEngine, paths: QStringList);

        // Runs until the drop
        #[qinvokable]
        fn start_drag(self: &SearchEngine, paths: QStringList);

        // Results column layout as JSON ({"widths": {...}, "hidden": [...]})
        #[qinvokable]
        fn column_layout(self: &SearchEngine) -> QString;

        #[qinvokable]
        fn save_column_layout(self: &SearchEngine, layout: QString);

        // Saved window size as JSON ({"width": .., "height": ..}), or empty
        #[qinvokable]
        fn window_size(self: &SearchEngine) -> QString;

        #[qinvokable]
        fn save_window_size(self: &SearchEngine, width: i32, height: i32);

        // File operations
        #[qinvokable]
        fn open_file(self: &SearchEngine, path: QString);

        #[qinvokable]
        fn open_folder(self: &SearchEngine, path: QString);

        // Window state (QML reports visibility so the tray menu text tracks it)
        #[qinvokable]
        fn set_window_visible(self: &SearchEngine, visible: bool);

        // Context menu
        #[qinvokable]
        fn show_context_menu(self: &SearchEngine, paths: QStringList, x: i32, y: i32);
    }

    // QAbstractListModel
    unsafe extern "RustQt" {
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut SearchEngine>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut SearchEngine>);

        #[cxx_override]
        fn data(self: &SearchEngine, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &SearchEngine) -> QHash_i32_QByteArray;

        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &SearchEngine, parent: &QModelIndex) -> i32;
    }

    impl cxx_qt::Threading for SearchEngine {}
}

use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QStringList, QVariant,
};
use tracing::info;

use crate::search::engine::{CoreEngine, Results};
use crate::search::highlight::Highlight;
use crate::search::query::{ParsedQuery, SortOrder};
use crate::search::selection::RowSelection;
use crate::search::trigram::Bookmark;

// Model roles, from Qt::UserRole, named for QML in role_names
const ROLE_PATH: i32 = 256;
const ROLE_FILENAME: i32 = 257;
const ROLE_IS_DIR: i32 = 258;
const ROLE_ICON_NAME: i32 = 259;
const ROLE_MTIME: i32 = 260;
const ROLE_FILE_SIZE: i32 = 261;
const ROLE_NAME_MARKUP: i32 = 262;

/// Rust backing struct for the SearchEngine QObject.
pub struct SearchEngineRust {
    // Qt properties
    status_text: QString,
    engine_ready: bool,
    result_count: i32,
    total_indexed: i32,
    search_time_ms: i32,
    bookmark_count: i32,
    selection_count: i32,
    selection_revision: i32,

    // Internal state (not exposed as Qt properties)
    inner: Option<Arc<CoreEngine>>,
    results: Results,
    /// Selected rows of `results`
    selection: RowSelection,
    /// Where the current query matched, for bold names
    highlight: Option<Arc<Highlight>>,
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
            selection_count: 0,
            selection_revision: 0,
            inner: None,
            results: Results::default(),
            selection: RowSelection::default(),
            highlight: None,
            search_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}

impl qobject::SearchEngine {
    /// Initialize the search engine: start toggle server immediately, then load
    /// DB and start watchers in a background thread so the window appears instantly.
    fn initialize(mut self: Pin<&mut Self>) {
        info!("Initializing search engine");

        self.as_mut()
            .set_status_text(QString::from("Loading index..."));

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

            let engine = Arc::new(CoreEngine::new(
                default_bookmarks,
                config.expanded_exclude_paths(),
                config.effective_max_results(),
            ));
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
                qobj.as_mut().set_status_text(QString::from(&format!(
                    "{} files indexed",
                    crate::search::engine::group_digits(file_count as usize)
                )));
            });
        });
    }

    /// Whether the app was launched with --hidden (start minimized to the tray
    /// instead of showing the window). Read by QML at startup. Backed by the
    /// ARCHNAV_START_HIDDEN env var set in main() before any threads spawn.
    fn start_hidden(&self) -> bool {
        std::env::var_os("ARCHNAV_START_HIDDEN").is_some()
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

        debug!(
            "[BRIDGE] search called: query='{}', seq={}",
            query_str, my_seq
        );

        std::thread::spawn(move || {
            let (results, total, elapsed) = inner.search(&query_str, sort_index);
            let highlight =
                Highlight::for_query(&ParsedQuery::parse(&query_str, SortOrder::MtimeDesc))
                    .map(Arc::new);
            let result_count = results.len() as i32;
            let status =
                crate::search::engine::results_status(results.len(), total, elapsed.as_millis());
            let search_time = elapsed.as_millis() as i32;
            let total_indexed = inner.file_count() as i32;

            debug!(
                "[BRIDGE] search complete: query='{}', seq={}, results={}",
                query_str, my_seq, result_count
            );

            let query_str_clone = query_str.clone();
            let _ = qt_thread.queue(move |mut qobj| {
                // Only update if this is still the most recent search
                let current_seq = seq_counter.load(Ordering::SeqCst);
                if my_seq < current_seq {
                    // A newer search was started, discard these stale results
                    debug!(
                        "[BRIDGE] discarding stale: query='{}', seq={} < current={}",
                        query_str_clone, my_seq, current_seq
                    );
                    return;
                }

                debug!(
                    "[BRIDGE] updating UI: query='{}', seq={}, results={}",
                    query_str_clone, my_seq, result_count
                );
                qobj.as_mut().rust_mut().highlight = highlight;
                qobj.as_mut().set_results(results);
                qobj.as_mut().set_result_count(result_count);
                qobj.as_mut().set_search_time_ms(search_time);
                qobj.as_mut().set_total_indexed(total_indexed);
                qobj.as_mut().set_status_text(QString::from(&status));
                qobj.as_mut().resultsReady();
            });
        });
    }

    fn clear_results(mut self: Pin<&mut Self>) {
        // A search still running would otherwise land after the clear
        self.rust()
            .search_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.as_mut().rust_mut().highlight = None;
        self.as_mut().set_results(Results::default());
        self.as_mut().set_result_count(0);
        // The status bar goes back to what it shows before a search
        if self.rust().engine_ready {
            let indexed = self.rust().total_indexed.max(0) as usize;
            self.as_mut().set_status_text(QString::from(&format!(
                "{} files indexed",
                crate::search::engine::group_digits(indexed)
            )));
        }
    }

    /// Swap in a new result set, telling attached views to reload.
    fn set_results(mut self: Pin<&mut Self>, results: Results) {
        // SAFETY: called on the Qt thread, and the reset brackets the only
        // change to what the model reports
        unsafe { self.as_mut().begin_reset_model() };
        let old = std::mem::replace(&mut self.as_mut().rust_mut().results, results);
        unsafe { self.as_mut().end_reset_model() };
        // Freeing a large result set here would stall the window
        crate::search::engine::drop_in_background(old);
        // Rows of the old results mean nothing in the new ones
        self.clear_selection();
    }

    /// Apply `change` to the selection, given the row count, and tell QML.
    fn change_selection(mut self: Pin<&mut Self>, change: impl FnOnce(&mut RowSelection, u32)) {
        let rows = u32::try_from(self.rust().results.len()).unwrap_or(u32::MAX);
        change(&mut self.as_mut().rust_mut().selection, rows);
        let count = i32::try_from(self.rust().selection.len()).unwrap_or(i32::MAX);
        let revision = self.rust().selection_revision.wrapping_add(1);
        self.as_mut().set_selection_count(count);
        self.as_mut().set_selection_revision(revision);
    }

    fn clear_selection(self: Pin<&mut Self>) {
        if !self.rust().selection.is_empty() {
            self.change_selection(|selection, _| selection.clear());
        }
    }

    fn toggle_selected(self: Pin<&mut Self>, row: i32) {
        self.change_selection(|selection, rows| selection.toggle(row, rows));
    }

    fn select_range(self: Pin<&mut Self>, from: i32, to: i32, keep: bool) {
        self.change_selection(|selection, rows| selection.select_range(from, to, keep, rows));
    }

    fn is_selected(&self, row: i32) -> bool {
        self.rust().selection.contains(row)
    }

    fn selected_paths(&self) -> QStringList {
        let results = &self.rust().results;
        self.rust()
            .selection
            .iter()
            .filter_map(|row| results.get(row))
            .map(|r| QString::from(&*r.path))
            .collect()
    }

    fn copy_files(&self, paths: QStringList) {
        crate::context_menu::copy_files(&strings(&paths));
    }

    fn copy_locations(&self, paths: QStringList) {
        crate::context_menu::copy_text(&strings(&paths).join("\n"));
    }

    fn start_drag(&self, paths: QStringList) {
        crate::context_menu::start_drag(&strings(&paths));
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(r) = self.rust().results.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            ROLE_PATH => QVariant::from(&QString::from(&*r.path)),
            ROLE_FILENAME => QVariant::from(&QString::from(file_name(&r.path))),
            ROLE_IS_DIR => QVariant::from(&r.is_dir),
            // Looked up per row as it scrolls into view, by name only
            ROLE_ICON_NAME => QVariant::from(&QString::from(&crate::mime_icon::icon_name_for(
                &r.path, r.is_dir,
            ))),
            ROLE_MTIME => QVariant::from(&r.mtime),
            ROLE_FILE_SIZE => QVariant::from(&(r.size as i64)),
            // Undefined in QML when nothing in the name matched
            ROLE_NAME_MARKUP => self
                .rust()
                .highlight
                .as_ref()
                .and_then(|h| h.markup(&r.path))
                .map(|markup| QVariant::from(&QString::from(&markup)))
                .unwrap_or_default(),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in [
            (ROLE_PATH, "path"),
            (ROLE_FILENAME, "filename"),
            (ROLE_IS_DIR, "isDir"),
            (ROLE_ICON_NAME, "iconName"),
            (ROLE_MTIME, "mtime"),
            (ROLE_FILE_SIZE, "fileSize"),
            (ROLE_NAME_MARKUP, "nameMarkup"),
        ] {
            roles.insert(role, QByteArray::from(name));
        }
        roles
    }

    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rust().results.len().try_into().unwrap_or(i32::MAX)
    }

    /// Add a bookmark and scan it in a background thread; scanning a large
    /// folder on the UI thread would freeze the window for its duration.
    fn add_bookmark(mut self: Pin<&mut Self>, name: QString, path: QString, is_network: bool) {
        let inner = match self.rust().inner.as_ref() {
            Some(inner) => inner.clone(),
            None => return,
        };

        let name = name.to_string();
        let path = path.to_string();
        let qt_thread = self.qt_thread();
        self.as_mut()
            .set_status_text(QString::from(&format!("Indexing {}...", path)));

        std::thread::spawn(move || {
            inner.add_bookmark(&name, &path, is_network);
            let count = inner.bookmarks().len() as i32;
            let file_count = inner.file_count() as i32;

            let _ = qt_thread.queue(move |mut qobj| {
                qobj.as_mut().set_bookmark_count(count);
                qobj.as_mut().set_total_indexed(file_count);
                qobj.as_mut().set_status_text(QString::from(&format!(
                    "{} files indexed",
                    crate::search::engine::group_digits(file_count as usize)
                )));
                qobj.as_mut().bookmarksChanged();
            });
        });
    }

    /// Remove a bookmark in a background thread (purging a large bookmark's
    /// in-memory entries is not instant).
    fn remove_bookmark(self: Pin<&mut Self>, name: QString) {
        let inner = match self.rust().inner.as_ref() {
            Some(inner) => inner.clone(),
            None => return,
        };

        let name = name.to_string();
        let qt_thread = self.qt_thread();

        std::thread::spawn(move || {
            inner.remove_bookmark(&name);
            let count = inner.bookmarks().len() as i32;
            let file_count = inner.file_count() as i32;

            let _ = qt_thread.queue(move |mut qobj| {
                qobj.as_mut().set_bookmark_count(count);
                qobj.as_mut().set_total_indexed(file_count);
                qobj.as_mut().bookmarksChanged();
            });
        });
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
                qobj.as_mut().set_status_text(QString::from(&format!(
                    "Rescan complete: {} files",
                    crate::search::engine::group_digits(file_count as usize)
                )));
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

    fn column_layout(&self) -> QString {
        let columns = crate::config::AppConfig::load().columns;
        QString::from(&serde_json::to_string(&columns).unwrap_or_default())
    }

    fn save_column_layout(&self, layout: QString) {
        match serde_json::from_str::<crate::config::ColumnLayout>(&layout.to_string()) {
            Ok(columns) => crate::config::AppConfig::save_columns(columns),
            Err(e) => tracing::warn!("Ignoring malformed column layout from QML: {}", e),
        }
    }

    fn window_size(&self) -> QString {
        crate::config::AppConfig::load()
            .window
            .and_then(|size| serde_json::to_string(&size).ok())
            .map(|json| QString::from(&json))
            .unwrap_or_default()
    }

    fn save_window_size(&self, width: i32, height: i32) {
        if width > 0 && height > 0 {
            crate::config::AppConfig::save_window(crate::config::WindowSize { width, height });
        }
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

    /// QML reports window visibility changes so the tray menu can flip
    /// between "Show archnav" and "Hide archnav".
    fn set_window_visible(&self, visible: bool) {
        crate::system_tray::set_global_window_visible(visible);
    }

    /// Show a KDE context menu for the given files.
    fn show_context_menu(&self, paths: QStringList, x: i32, y: i32) {
        // Context menu must be shown on the main thread, so we call it directly
        let menu = crate::context_menu::ContextMenu::new();
        menu.show(&strings(&paths), x, y);
    }
}

fn strings(list: &QStringList) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The last component of `path`, or all of it when it has none.
fn file_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}
