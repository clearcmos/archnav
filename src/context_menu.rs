use std::ffi::CString;

// FFI bindings to the C++ context menu handler
extern "C" {
    fn create_context_menu_handler() -> *mut std::ffi::c_void;
    fn show_context_menu(
        handler: *mut std::ffi::c_void,
        path: *const std::ffi::c_char,
        x: i32,
        y: i32,
        window: *mut std::ffi::c_void,
    );
    fn destroy_context_menu_handler(handler: *mut std::ffi::c_void);
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

    /// Show a context menu for the given file path at screen position (x, y)
    pub fn show(&self, path: &str, x: i32, y: i32) {
        if let Ok(c_path) = CString::new(path) {
            unsafe {
                show_context_menu(
                    self.handler,
                    c_path.as_ptr(),
                    x,
                    y,
                    std::ptr::null_mut(),
                );
            }
        }
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
