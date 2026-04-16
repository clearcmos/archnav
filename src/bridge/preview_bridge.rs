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
        fn request_preview(self: Pin<&mut PreviewBridge>, path: QString, is_dir: bool, content_width: i32);

        #[qinvokable]
        fn clear_preview(self: Pin<&mut PreviewBridge>);
    }

    impl cxx_qt::Threading for PreviewBridge {}
}

use std::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;

/// Rust backing struct for the PreviewBridge QObject.
pub struct PreviewBridgeRust {
    preview_type: QString,
    preview_text: QString,
    image_path: QString,
    file_path: QString,
    is_loading: bool,
}

impl Default for PreviewBridgeRust {
    fn default() -> Self {
        Self {
            preview_type: QString::from("none"),
            preview_text: QString::default(),
            image_path: QString::default(),
            file_path: QString::default(),
            is_loading: false,
        }
    }
}

impl qobject::PreviewBridge {
    /// Request a preview for the given path. Runs in background thread,
    /// emits preview_ready when done.
    fn request_preview(mut self: Pin<&mut Self>, path: QString, is_dir: bool, content_width: i32) {
        let path_str = path.to_string();
        self.as_mut().set_file_path(QString::from(&*path_str));
        self.as_mut().set_is_loading(true);

        let qt_thread = self.qt_thread();
        let width = content_width.max(100) as u32;  // Minimum 100px

        std::thread::spawn(move || {
            let result = crate::preview::generate_preview(&path_str, is_dir, width);

            let _ = qt_thread.queue(move |mut qobj| {
                qobj.as_mut()
                    .set_preview_type(QString::from(&*result.preview_type));
                qobj.as_mut()
                    .set_preview_text(QString::from(&*result.text));
                qobj.as_mut()
                    .set_image_path(QString::from(&*result.image_path));
                qobj.as_mut().set_is_loading(false);
                qobj.as_mut().previewReady();
            });
        });
    }

    fn clear_preview(mut self: Pin<&mut Self>) {
        self.as_mut().set_preview_type(QString::from("none"));
        self.as_mut().set_preview_text(QString::default());
        self.as_mut().set_image_path(QString::default());
        self.as_mut().set_file_path(QString::default());
        self.as_mut().set_is_loading(false);
    }
}
