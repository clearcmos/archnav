pub mod bridge;
pub mod config;
pub mod context_menu;
pub mod file_opener;
pub mod ipc;
pub mod preview;
pub mod search;
pub mod system_tray;
pub mod toggle;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};

extern "C" {
    fn install_qt_debug_handler();
    fn create_qapplication();
    fn run_qapplication() -> i32;
    fn destroy_qapplication();
    fn quit_qapplication();
}

fn main() {
    // Install custom Qt message handler FIRST (before anything else)
    unsafe { install_qt_debug_handler(); }

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("archnav=info")),
        )
        .init();

    // Handle --toggle flag before creating Qt app
    if std::env::args().any(|a| a == "--toggle") {
        if toggle::send_toggle() {
            // Successfully toggled existing instance
            std::process::exit(0);
        }
        // No existing instance — fall through to start new one
        tracing::info!("No existing instance found, starting new one");
    }

    // Handle --test-search flag for CLI testing
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--test-search") {
        let queries: Vec<&str> = args.iter().skip(pos + 1).map(|s| s.as_str()).collect();
        if queries.is_empty() {
            eprintln!("Usage: archnav --test-search <query1> [query2] ...");
            std::process::exit(1);
        }

        // Load config and create engine
        let cfg = config::AppConfig::load();
        let bookmarks = cfg.to_bookmarks();
        println!("Loading search index...");
        let engine = search::engine::CoreEngine::new(bookmarks);
        println!("Indexed {} files\n", engine.file_count());

        // Run each query
        for query in queries {
            engine.test_search(query);
            println!();
        }
        std::process::exit(0);
    }

    // Handle --benchmark flag for search performance benchmarking
    if args.iter().any(|a| a == "--benchmark") {
        let iterations: usize = args.iter()
            .position(|a| a == "--iterations")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let cfg = config::AppConfig::load();
        let bookmarks = cfg.to_bookmarks();
        println!("Loading search index...");
        let load_start = std::time::Instant::now();
        let engine = search::engine::CoreEngine::new(bookmarks);
        let load_time = load_start.elapsed();
        let file_count = engine.file_count();
        println!("Loaded {} files in {:.2?}\n", file_count, load_time);

        let queries = vec![
            // Short queries (common trigram hits)
            "config",
            "main",
            "test",
            // Medium queries (more selective)
            "archnav",
            "PKGBUILD",
            "package.json",
            // Long/specific queries
            "pacman.conf",
            "screenshot",
            // Extension filter
            "*.rs",
            "*.py main",
            // Uncommon (few or no results)
            "xyzzyplugh",
            // 2-char (below trigram threshold, falls back to scan)
            "ab",
        ];

        println!("╔══════════════════════════════════════════════════════════════════════╗");
        println!("║  ArchNav Search Benchmark                                           ║");
        println!("║  Files indexed: {:>10}                                           ║", file_count);
        println!("║  Iterations:    {:>10}                                           ║", iterations);
        println!("╚══════════════════════════════════════════════════════════════════════╝");
        println!();
        println!("{:<25} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "Query", "Min", "Median", "P95", "P99", "Max", "Results");
        println!("{}", "─".repeat(83));

        for query in &queries {
            // Warm-up: 3 iterations (discarded)
            for _ in 0..3 {
                let _ = engine.search(query, 0);
            }

            // Timed iterations
            let mut times: Vec<std::time::Duration> = Vec::with_capacity(iterations);
            let mut result_count = 0;
            for _ in 0..iterations {
                // Clear search cache between runs for honest measurement
                engine.clear_search_cache();
                let (results, elapsed) = engine.search(query, 0);
                times.push(elapsed);
                result_count = results.len();
            }

            times.sort();
            let min = times[0];
            let median = times[times.len() / 2];
            let p95 = times[(times.len() as f64 * 0.95) as usize];
            let p99 = times[(times.len() as f64 * 0.99) as usize];
            let max = times[times.len() - 1];

            fn fmt_dur(d: std::time::Duration) -> String {
                let us = d.as_micros();
                if us < 1000 {
                    format!("{}µs", us)
                } else {
                    format!("{:.2}ms", us as f64 / 1000.0)
                }
            }

            println!("{:<25} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
                format!("\"{}\"", query),
                fmt_dur(min), fmt_dur(median), fmt_dur(p95), fmt_dur(p99), fmt_dur(max),
                result_count);
        }

        println!("\n{} iterations per query, 3 warm-up runs excluded.", iterations);
        println!("Cache cleared between iterations for honest cold-search measurement.");
        std::process::exit(0);
    }

    tracing::info!("ArchNav v0.1.0 starting");

    // Create Qt application (QApplication for QtWidgets support - needed for context menus)
    unsafe { create_qapplication(); }

    // Load config for hotkey setting
    let cfg = config::AppConfig::load();
    let hotkey = cfg.toggle_hotkey.clone();

    // Create system tray with toggle and exit callbacks
    let _tray = system_tray::SystemTray::new(
        // Toggle callback: send toggle via Unix socket to trigger window show/hide
        || {
            // Send toggle command to ourselves via the socket
            // This reuses the existing toggle infrastructure
            let _ = toggle::send_toggle();
        },
        // Exit callback: quit the application
        || {
            unsafe { quit_qapplication(); }
        },
    );

    // Register global hotkey
    _tray.set_hotkey(&hotkey);
    tracing::info!("System tray created with hotkey: {}", hotkey);

    let mut engine = QQmlApplicationEngine::new();

    if let Some(mut engine) = engine.as_mut() {
        // Load QML from embedded QRC resources
        let url_str = "qrc:/qt/qml/org/archnav/app/qml/Main.qml";
        tracing::info!("Loading QML from: {}", url_str);
        engine.load(&QUrl::from(url_str));
        tracing::info!("QML loaded");
    } else {
        tracing::error!("QQmlApplicationEngine is null!");
    }

    // Run the Qt event loop
    let exit_code = unsafe { run_qapplication() };

    // Cleanup
    unsafe { destroy_qapplication(); }
    let _ = std::fs::remove_file(toggle::socket_path());

    std::process::exit(exit_code);
}
