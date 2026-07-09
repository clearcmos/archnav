# Pending Security Dependency Upgrades

Two Aikido / RUSTSEC advisories on archnav are deliberately **deferred** because
fixing them requires a breaking major/minor upgrade of a parent crate plus source
changes, not a lockfile bump. Both are low/no severity on a local Linux/KDE
desktop app; neither is urgent. This file is the hand-off for an agent that wants
to clear them.

Context (as of 2026-07-09):
- Already fixed and pushed (commit `684be62`): `smallvec 1.15.2`, `tar 0.4.46`,
  `anyhow 1.0.103` (safe patch bumps).
- Both advisories below are currently marked **won't-fix / ignored** in Aikido
  (repo id `2497766`) with reasons, so the dashboard is quiet either way.
- This machine needs `CARGO_NET_GIT_FETCH_WITH_CLI=true` for cargo, and archnav
  needs Qt6 dev libs to build (`cxx-qt-lib` `qt_full`).

---

## 1. cxx 1.0.194 -> 1.0.196+  (Aikido AIKIDO-2026-359665, severity low)

**Why it is blocked.** `Cargo.toml` already allows it (`cxx = "1.0.95"`), but the
cxx version is effectively pinned by the **cxx-qt 0.8 family**: `cxx-qt-build`
generates the C++/Rust bridge with symbols stamped to cxx `1.0.194`. Bumping `cxx`
alone splits the version and the link fails with:

```
rust-lld: error: undefined symbol: cxxbridge1$194$PreviewBridge$...
```

(verified 2026-07-09 - `cargo update -p cxx` to 1.0.197 broke the build; reverted).

**Fix = upgrade the cxx-qt family 0.8 -> 0.9.1** (latest stable), which uses a cxx
>= 1.0.196.

Cargo.toml edits:
```toml
cxx-qt = "0.9"
cxx-qt-lib = { version = "0.9", features = ["qt_full"] }
cxx-qt-build = { version = "0.9", features = ["link_qt_object_files"] }
# leave: cxx = "1.0.95"  (range already permits 1.0.196+; cargo unifies to cxx-qt 0.9's cxx)
```
Then `cargo update`.

**Breaking-change scope (cxx-qt 0.8 -> 0.9 has API changes).** Update the bridge
modules:
- `src/bridge/search_engine.rs`
- `src/bridge/preview_bridge.rs`
- `src/toggle.rs`
- `src/main.rs` (QObject / QML registration)

Follow the cxx-qt 0.9 migration notes (https://kdab.github.io/cxx-qt/book/ +
the cxx-qt CHANGELOG). Typical 0.8->0.9 churn: `#[cxx_qt::bridge]` / qobject
attribute syntax, property / signal / invokable macros, QML registration in
`main.rs`, and the `cxx-qt-build` API in `build.rs`.

**Verify:** `CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --release`, launch the
app, confirm QML loads and the preview pane + search UI work. Advisory clears on
Aikido's next scan of `main`.

**Effort/risk:** moderate - a real cxx-qt migration, but self-contained to the 4
bridge files. Worth doing eventually to keep cxx-qt current.

---

## 2. mio 0.8.11 -> 1.2.1  (Aikido AIKIDO-2026-717871, severity low)

**Applicability first:** this is the RUSTSEC mio advisory about named-pipe token
delivery - it is **Windows-IOCP-specific**. archnav is Linux/KDE and uses inotify,
so **the advisory does not apply**. Recommended action: **accept / leave ignored**.
Only fix it if you want a literal zero on the dashboard.

**Why it is blocked.** `notify = "6"` -> mio `0.8`. mio `1.x` requires **notify 7/8**.

**Fix = bump notify 6 -> 8** (latest stable; uses mio 1.x - avoid the `9.0.0-rc`
pre-release):
```toml
notify = { version = "8", features = ["serde"] }
```
Then `cargo update`.

**Breaking-change scope (notify 6 -> 7 -> 8 API changes).** Update:
- `src/search/watcher.rs` (primary - `RecommendedWatcher`, `Config`, event loop)
- `src/search/{integrity,scanner,engine,database}.rs` (use notify types)

Check the notify CHANGELOG for 7.0 / 8.0: `Config` builder, `Event` / `EventKind`,
`RecommendedWatcher::new` signature, and whether debouncing moved to
`notify-debouncer-full` (if archnav debounces).

**Verify:** build + run, then create/modify/delete a file in a watched directory
and confirm the search index updates (that is the whole point of the watcher).

**Effort/risk:** moderate, and **low value** - the advisory is N/A on Linux. Do it
only opportunistically (e.g. if bumping notify for other reasons).

---

## When done

Push to `main`; Aikido rescans on push and the advisories flip to closed. Check
`https://app.aikido.dev/repositories/2497766`, or drive the dashboard with the
`~/git/aikido` tool. If you decide *not* to fix either, no action is needed -
they are already ignored with reasons recorded.
