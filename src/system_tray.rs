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

/// Global tray handle so window-visibility changes (reported by QML through
/// the bridge) can update the tray menu text. Stored as usize because raw
/// pointers are not Send; only ever dereferenced on the Qt main thread.
static TRAY_PTR: Mutex<Option<usize>> = Mutex::new(None);

/// Update the tray menu ("Show archnav" / "Hide archnav") to match the
/// window's visibility. No-op if the tray does not exist.
pub fn set_global_window_visible(visible: bool) {
    if let Ok(guard) = TRAY_PTR.lock() {
        if let Some(ptr) = *guard {
            unsafe {
                system_tray_set_window_visible(ptr as *mut std::ffi::c_void, visible);
            }
        }
    }
}

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

        if let Ok(mut guard) = TRAY_PTR.lock() {
            *guard = Some(ptr as usize);
        }

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
}

impl Drop for SystemTray {
    fn drop(&mut self) {
        if let Ok(mut guard) = TRAY_PTR.lock() {
            *guard = None;
        }
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
