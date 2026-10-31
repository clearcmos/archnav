use std::ffi::{c_char, CString};

// FFI bindings to the C++ context menu, clipboard, and drag handlers
extern "C" {
    fn create_context_menu_handler() -> *mut std::ffi::c_void;
    fn show_context_menu(
        handler: *mut std::ffi::c_void,
        paths: *const *const c_char,
        count: usize,
        x: i32,
        y: i32,
    );
    fn destroy_context_menu_handler(handler: *mut std::ffi::c_void);
    fn copy_files_to_clipboard(paths: *const *const c_char, count: usize);
    fn copy_text_to_clipboard(text: *const c_char);
    fn start_file_drag(paths: *const *const c_char, count: usize);
}

/// Call `f` with `paths` as an array of C strings; a path holding a NUL byte
/// cannot exist on disk and is left out.
fn with_c_paths<R>(paths: &[String], f: impl FnOnce(*const *const c_char, usize) -> R) -> R {
    let owned: Vec<CString> = paths
        .iter()
        .filter_map(|p| CString::new(p.as_str()).ok())
        .collect();
    let pointers: Vec<*const c_char> = owned.iter().map(|p| p.as_ptr()).collect();
    f(pointers.as_ptr(), pointers.len())
}

/// Rust wrapper for the KDE context menu handler
pub struct ContextMenu {
    handler: *mut std::ffi::c_void,
}

// Safe to send between threads as the handler is only used from Qt main thread
unsafe impl Send for ContextMenu {}

impl ContextMenu {
    /// Create a new context menu handler
    pub fn new() -> Self {
        let handler = unsafe { create_context_menu_handler() };
        Self { handler }
    }

    /// Show a context menu for the given files at screen position (x, y)
    pub fn show(&self, paths: &[String], x: i32, y: i32) {
        with_c_paths(paths, |ptr, count| unsafe {
            show_context_menu(self.handler, ptr, count, x, y);
        });
    }
}

impl Drop for ContextMenu {
    fn drop(&mut self) {
        unsafe {
            destroy_context_menu_handler(self.handler);
        }
    }
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

/// Put the files on the clipboard, for pasting in Dolphin or elsewhere.
pub fn copy_files(paths: &[String]) {
    with_c_paths(paths, |ptr, count| unsafe {
        copy_files_to_clipboard(ptr, count)
    });
}

/// Put plain text on the clipboard.
pub fn copy_text(text: &str) {
    if let Ok(text) = CString::new(text) {
        unsafe { copy_text_to_clipboard(text.as_ptr()) };
    }
}

/// Drag the files out of the window; returns after the drop.
pub fn start_drag(paths: &[String]) {
    with_c_paths(paths, |ptr, count| unsafe { start_file_drag(ptr, count) });
}
