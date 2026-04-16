use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::{fs, thread};
use tracing::{info, warn};

use cxx_qt::CxxQtThread;

use crate::bridge::search_engine::qobject::SearchEngine;

/// Socket path for single-instance toggle.
pub fn socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }))
        + "/archnav.sock"
}

/// Try to send a toggle command to an existing instance.
/// Returns true if successful (existing instance found).
pub fn send_toggle() -> bool {
    let path = socket_path();

    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => {
            // No existing instance or stale socket
            let _ = fs::remove_file(&path);
            return false;
        }
    };

    use std::io::Write;
    match stream.write_all(b"toggle") {
        Ok(_) => {
            info!("Toggle sent to existing instance");
            true
        }
        Err(_) => {
            let _ = fs::remove_file(&path);
            false
        }
    }
}

/// Start the toggle socket server in a background thread.
/// When "toggle" is received, emits toggle_requested on the Qt thread.
pub fn start_toggle_server(qt_thread: CxxQtThread<SearchEngine>) {
    thread::spawn(move || {
        let path = socket_path();
        let _ = fs::remove_file(&path);

        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to bind toggle socket {}: {}", path, e);
                return;
            }
        };

        info!("Toggle server listening on {}", path);

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let mut buf = [0u8; 64];
                    if let Ok(n) = stream.read(&mut buf) {
                        let msg = String::from_utf8_lossy(&buf[..n]);
                        if msg.trim() == "toggle" {
                            let _ = qt_thread.queue(|mut qobj| {
                                qobj.as_mut().toggleRequested();
                            });
                        }
                    }
                }
                Err(e) => {
                    warn!("Toggle accept error: {}", e);
                }
            }
        }
    });
}
