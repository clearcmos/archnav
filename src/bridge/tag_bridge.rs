// Qt signals are camelCase by convention; cxx-qt mirrors them in generated methods.
#![allow(non_snake_case)]

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, tags)]
        #[qproperty(bool, has_store)]
        #[qproperty(bool, is_saving)]
        #[qproperty(QString, error_text)]
        type TagBridge = super::TagBridgeRust;

        #[qsignal]
        fn tagsSaved(self: Pin<&mut TagBridge>);

        #[qinvokable]
        fn load_tags(self: Pin<&mut TagBridge>, path: QString, is_dir: bool);

        #[qinvokable]
        fn tags_for(self: Pin<&mut TagBridge>, path: QString, is_dir: bool) -> QString;

        #[qinvokable]
        fn save_tags(self: Pin<&mut TagBridge>, path: QString, input: QString);

        #[qinvokable]
        fn clear(self: Pin<&mut TagBridge>);
    }

    impl cxx_qt::Threading for TagBridge {}
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::pin::Pin;

use crate::tagstore::{self, TagLookup};

/// Rust backing struct for the TagBridge QObject.
///
/// `tags` is the display string ("a, b, c"), empty when untagged or when the
/// file is outside any tag store. Reads parse the tagdex index directly;
/// writes go through the tagdex CLI (see src/tagstore.rs for the rationale).
pub struct TagBridgeRust {
    tags: QString,
    has_store: bool,
    is_saving: bool,
    error_text: QString,
    /// Sequence counter so a slow lookup (index read over a network mount)
    /// cannot overwrite the tags of a file selected later.
    tag_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Default for TagBridgeRust {
    fn default() -> Self {
        Self {
            tags: QString::default(),
            has_store: false,
            is_saving: false,
            error_text: QString::default(),
            tag_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}

impl qobject::TagBridge {
    /// Look up tags for the given path in a background thread (the index may
    /// live on a network mount; never block the UI on it).
    fn load_tags(mut self: Pin<&mut Self>, path: QString, is_dir: bool) {
        use std::sync::atomic::Ordering;

        let seq_counter = self.rust().tag_seq.clone();
        let my_seq = seq_counter.fetch_add(1, Ordering::SeqCst) + 1;

        let path_str = path.to_string();
        if is_dir || path_str.is_empty() {
            self.as_mut().set_tags(QString::default());
            self.as_mut().set_has_store(false);
            return;
        }

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let lookup = tagstore::read_tags(std::path::Path::new(&path_str));
            let _ = qt_thread.queue(move |mut qobj| {
                if my_seq < seq_counter.load(Ordering::SeqCst) {
                    return; // superseded by a newer selection
                }
                match lookup {
                    Ok(TagLookup::Tags(tags)) => {
                        qobj.as_mut().set_has_store(true);
                        qobj.as_mut().set_tags(QString::from(&*tags.join(", ")));
                    }
                    Ok(TagLookup::NoStore) => {
                        qobj.as_mut().set_has_store(false);
                        qobj.as_mut().set_tags(QString::default());
                    }
                    Err(err) => {
                        tracing::warn!("tag lookup failed: {}", err);
                        qobj.as_mut().set_has_store(false);
                        qobj.as_mut().set_tags(QString::default());
                    }
                }
            });
        });
    }

    /// Synchronous per-row tag lookup for the results model. Safe on the UI
    /// thread because both the store-root discovery and the parsed index are
    /// cached in tagstore; the first hit on a store reads its index once.
    fn tags_for(self: Pin<&mut Self>, path: QString, is_dir: bool) -> QString {
        if is_dir {
            return QString::default();
        }
        let path_str = path.to_string();
        match tagstore::read_tags(std::path::Path::new(&path_str)) {
            Ok(TagLookup::Tags(tags)) if !tags.is_empty() => QString::from(&*tags.join(", ")),
            _ => QString::default(),
        }
    }

    /// Replace the file's tags from the dialog's comma-separated input.
    /// Writes through the native tag store engine in a background thread
    /// (the index may live on a network mount); emits tagsSaved on success,
    /// sets error_text on failure.
    fn save_tags(mut self: Pin<&mut Self>, path: QString, input: QString) {
        let path_str = path.to_string();
        let tags = tagstore::parse_tag_input(&input.to_string());

        self.as_mut().set_is_saving(true);
        self.as_mut().set_error_text(QString::default());

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = tagstore::set_tags_for_file(std::path::Path::new(&path_str), &tags);
            let _ = qt_thread.queue(move |mut qobj| {
                qobj.as_mut().set_is_saving(false);
                match result {
                    Ok(final_tags) => {
                        qobj.as_mut().set_has_store(true);
                        qobj.as_mut()
                            .set_tags(QString::from(&*final_tags.join(", ")));
                        qobj.as_mut().tagsSaved();
                    }
                    Err(err) => {
                        qobj.as_mut().set_error_text(QString::from(&*err));
                    }
                }
            });
        });
    }

    fn clear(mut self: Pin<&mut Self>) {
        // Invalidate any in-flight lookup so it cannot resurrect content.
        self.rust()
            .tag_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.as_mut().set_tags(QString::default());
        self.as_mut().set_has_store(false);
        self.as_mut().set_is_saving(false);
        self.as_mut().set_error_text(QString::default());
    }
}
