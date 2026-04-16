use std::ffi::CString;
use std::sync::Mutex;

// FFI bindings to the C++ system tray handler
extern "C" {
    fn create_system_tray(
        toggle_cb: extern "C" fn(),
        exit_cb: extern "C" fn(),
    ) -> *mut std::ffi::c_void;
    fn system_tray_set_hotkey(tray: *mut std::ffi::c_void, hotkey: *const std::ffi::c_char);
    fn system_tray_set_window_visible(tray: *mut std::ffi::c_void, visible: bool);
    fn destroy_system_tray(tray: *mut std::ffi::c_void);
}

// Global storage for callbacks (needed because C callbacks can't capture state)
static TOGGLE_CALLBACK: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);
static EXIT_CALLBACK: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);

// C callback that routes to Rust closure
extern "C" fn toggle_callback_wrapper() {
    if let Ok(guard) = TOGGLE_CALLBACK.lock() {
        if let Some(ref cb) = *guard {
            cb();
        }
    }
}

extern "C" fn exit_callback_wrapper() {
    if let Ok(guard) = EXIT_CALLBACK.lock() {
        if let Some(ref cb) = *guard {
            cb();
        }
    }
}

/// Rust wrapper for the system tray
pub struct SystemTray {
    ptr: *mut std::ffi::c_void,
}

// Safe to send between threads as Qt handles thread safety
unsafe impl Send for SystemTray {}

impl SystemTray {
    /// Create a new system tray with toggle and exit callbacks.
    ///
    /// # Arguments
    /// * `on_toggle` - Called when tray is clicked or global hotkey is pressed
    /// * `on_exit` - Called when "Exit" menu item is selected
    pub fn new<F, G>(on_toggle: F, on_exit: G) -> Self
    where
        F: Fn() + Send + Sync + 'static,
        G: Fn() + Send + Sync + 'static,
    {
        // Store callbacks in global state
        {
            let mut guard = TOGGLE_CALLBACK.lock().unwrap();
            *guard = Some(Box::new(on_toggle));
        }
        {
            let mut guard = EXIT_CALLBACK.lock().unwrap();
            *guard = Some(Box::new(on_exit));
        }

        let ptr = unsafe { create_system_tray(toggle_callback_wrapper, exit_callback_wrapper) };

        Self { ptr }
    }

    /// Set the global hotkey (e.g., "Alt+`", "Ctrl+Space", "Meta+N").
    pub fn set_hotkey(&self, hotkey: &str) {
        if let Ok(c_hotkey) = CString::new(hotkey) {
            unsafe {
                system_tray_set_hotkey(self.ptr, c_hotkey.as_ptr());
            }
        }
    }

    /// Update the tray menu to reflect window visibility.
    #[allow(dead_code)]
    pub fn set_window_visible(&self, visible: bool) {
        unsafe {
            system_tray_set_window_visible(self.ptr, visible);
        }
    }
}

impl Drop for SystemTray {
    fn drop(&mut self) {
        unsafe {
            destroy_system_tray(self.ptr);
        }
        // Clear callbacks
        if let Ok(mut guard) = TOGGLE_CALLBACK.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = EXIT_CALLBACK.lock() {
            *guard = None;
        }
    }
}
