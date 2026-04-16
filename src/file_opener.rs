use std::ffi::CString;

// FFI binding to the C++ KIO file opener
extern "C" {
    fn kio_open_file(path: *const std::ffi::c_char);
}

/// Open a file using KIO::OpenUrlJob with proper Wayland activation token.
/// This ensures the opened application receives focus (same as Dolphin).
pub fn open_file(path: &str) {
    if let Ok(c_path) = CString::new(path) {
        unsafe {
            kio_open_file(c_path.as_ptr());
        }
    }
}
