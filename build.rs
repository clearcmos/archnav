use cxx_qt_build::{CxxQtBuilder, QmlModule, QmlFile};
use std::process::Command;
use std::path::PathBuf;

fn main() {
    // Run MOC on system_tray.cpp to generate system_tray.moc
    // This is needed because it contains a Q_OBJECT class (SystemTrayHandler)
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let moc_output = out_dir.join("system_tray.moc");

    // Find MOC binary
    let moc_path = if let Ok(output) = Command::new("pkg-config")
        .args(["--variable=libexecdir", "Qt6Core"])
        .output()
    {
        let libexec = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !libexec.is_empty() {
            format!("{}/moc", libexec)
        } else {
            "moc".to_string()
        }
    } else {
        "moc".to_string()
    };

    // Get Qt include paths for MOC
    let qt_include = Command::new("pkg-config")
        .args(["--cflags", "Qt6Core", "Qt6Widgets", "Qt6DBus"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // Run MOC on the source file
    let moc_args: Vec<&str> = qt_include.split_whitespace().collect();
    let mut moc_cmd = Command::new(&moc_path);
    for arg in &moc_args {
        moc_cmd.arg(arg);
    }
    moc_cmd
        .arg("-I").arg("src")
        .arg("src/system_tray.cpp")
        .arg("-o").arg(&moc_output);

    let moc_status = moc_cmd.status();
    match moc_status {
        Ok(status) if status.success() => {
            println!("cargo:rerun-if-changed=src/system_tray.cpp");
            println!("cargo:rerun-if-changed=src/system_tray.h");
        }
        Ok(status) => {
            eprintln!("MOC failed with status: {}", status);
        }
        Err(e) => {
            eprintln!("Failed to run MOC: {}", e);
        }
    }

    unsafe {
        CxxQtBuilder::new_qml_module(
            QmlModule::new("org.archnav.app")
                .qml_file("qml/Main.qml")
                .qml_file("qml/SearchBar.qml")
                .qml_file("qml/ResultsList.qml")
                .qml_file("qml/PreviewPanel.qml")
                .qml_file("qml/BookmarkDialog.qml")
                .qml_file(QmlFile::from("qml/Style.qml").singleton(true)),
        )
        .qt_module("Qml")
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .qt_module("Widgets") // Needed for QMenu context menus
        .qt_module("DBus") // Needed for FileManager1 properties dialog
        .files([
            "src/bridge/search_engine.rs",
            "src/bridge/preview_bridge.rs",
        ])
        .cc_builder(|cc| {
            // Qt debug handler
            cc.file("src/qt_debug_handler.cpp");

            // QApplication wrapper (needed for QMenu/QtWidgets)
            cc.file("src/qt_app.cpp");

            // Context menu handler with KDE integration
            cc.file("src/context_menu.cpp");

            // File opener using KIO::OpenUrlJob for proper Wayland focus
            cc.file("src/file_opener.cpp");

            // System tray with GlobalShortcuts portal
            cc.file("src/system_tray.cpp");

            // Add src directory for headers
            cc.include("src");

            // Add OUT_DIR for MOC-generated files
            cc.include(&out_dir);

            // Add QtQmlIntegration include path for MOC
            if let Ok(output) = Command::new("pkg-config")
                .args(["--variable=includedir", "Qt6QmlIntegration"])
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    cc.include(&path);
                    cc.include(format!("{}/QtQmlIntegration", path));
                }
            }

            // KDE Frameworks 6 include paths (standard system locations on Arch)
            let kf6_include = "/usr/include/KF6";
            cc.include(kf6_include);
            for subdir in [
                "KService", "KIOCore", "KIOGui", "KIO",
                "KCoreAddons", "KConfig", "KConfigCore",
            ] {
                let path = format!("{}/{}", kf6_include, subdir);
                if std::path::Path::new(&path).is_dir() {
                    cc.include(&path);
                }
            }

            cc.flag("-fPIC");
        })
        .build();
    }

    // Link KDE Frameworks 6 libraries (standard system location on Arch)
    println!("cargo:rustc-link-lib=KF6Service");
    println!("cargo:rustc-link-lib=KF6KIOCore");
    println!("cargo:rustc-link-lib=KF6KIOGui");
    println!("cargo:rustc-link-lib=KF6CoreAddons");
}
