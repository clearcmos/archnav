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
        #[qproperty(QString, preview_type)]
        #[qproperty(QString, preview_text)]
        #[qproperty(QString, image_path)]
        #[qproperty(QString, file_path)]
        #[qproperty(bool, is_loading)]
        type PreviewBridge = super::PreviewBridgeRust;

        #[qsignal]
        fn previewReady(self: Pin<&mut PreviewBridge>);

        #[qinvokable]
        fn request_preview(
            self: Pin<&mut PreviewBridge>,
            path: QString,
            is_dir: bool,
            content_width: i32,
        );

        #[qinvokable]
        fn clear_preview(self: Pin<&mut PreviewBridge>);
    }

    impl cxx_qt::Threading for PreviewBridge {}
}

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::pin::Pin;

/// Rust backing struct for the PreviewBridge QObject.
pub struct PreviewBridgeRust {
    preview_type: QString,
    preview_text: QString,
    image_path: QString,
    file_path: QString,
    is_loading: bool,
    /// Sequence counter so a slow preview (e.g. ffprobe over a network mount)
    /// cannot overwrite the preview of a file selected later.
    preview_seq: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Default for PreviewBridgeRust {
    fn default() -> Self {
        Self {
            preview_type: QString::from("none"),
            preview_text: QString::default(),
            image_path: QString::default(),
            file_path: QString::default(),
            is_loading: false,
            preview_seq: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}

impl qobject::PreviewBridge {
    /// Request a preview for the given path. Runs in background thread,
    /// emits preview_ready when done. Stale results (a newer request or a
    /// clear happened meanwhile) are discarded.
    fn request_preview(mut self: Pin<&mut Self>, path: QString, is_dir: bool, content_width: i32) {
        use std::sync::atomic::Ordering;

        let path_str = path.to_string();
        self.as_mut().set_file_path(QString::from(&*path_str));
        self.as_mut().set_is_loading(true);

        let qt_thread = self.qt_thread();
        let width = content_width.max(100) as u32; // Minimum 100px

        let seq_counter = self.rust().preview_seq.clone();
        let my_seq = seq_counter.fetch_add(1, Ordering::SeqCst) + 1;

        std::thread::spawn(move || {
            let result = crate::preview::generate_preview(&path_str, is_dir, width);

            let _ = qt_thread.queue(move |mut qobj| {
                if my_seq < seq_counter.load(Ordering::SeqCst) {
                    return; // superseded by a newer request or a clear
                }
                qobj.as_mut()
                    .set_preview_type(QString::from(&*result.preview_type));
                qobj.as_mut().set_preview_text(QString::from(&*result.text));
                qobj.as_mut()
                    .set_image_path(QString::from(&*result.image_path));
                qobj.as_mut().set_is_loading(false);
                qobj.as_mut().previewReady();
            });
        });
    }

    fn clear_preview(mut self: Pin<&mut Self>) {
        // Invalidate any in-flight preview so it cannot resurrect content.
        self.rust()
            .preview_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.as_mut().set_preview_type(QString::from("none"));
        self.as_mut().set_preview_text(QString::default());
        self.as_mut().set_image_path(QString::default());
        self.as_mut().set_file_path(QString::default());
        self.as_mut().set_is_loading(false);
    }
}
