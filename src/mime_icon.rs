use std::ffi::CString;

// FFI binding to the C++ MIME icon lookup
extern "C" {
    fn mime_icon_name(
        file_name: *const std::ffi::c_char,
        out: *mut std::ffi::c_char,
        out_len: usize,
    ) -> usize;
}

/// Theme icon name for a search result: "folder" for directories, otherwise
/// the icon of the MIME type its file name suggests. Needs the Qt application
/// to exist, since the icon theme is consulted.
pub fn icon_name_for(path: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".to_string();
    }
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let Ok(c_name) = CString::new(name) else {
        return "unknown".to_string();
    };
    let mut buf = [0u8; 256];
    let len = unsafe { mime_icon_name(c_name.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    if len == 0 {
        return "unknown".to_string();
    }
    String::from_utf8_lossy(&buf[..len]).into_owned()
}
