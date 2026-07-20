//! `archnav tag` CLI: the terminal interface to the tag store engine.
//!
//! Runs before any Qt initialization (dispatched from main), so it works in
//! headless contexts. The UX mirrors the original tagdex CLI: positional
//! FILE TAG... or -t TAG FILE... forms, ls -lh style directory listings
//! with a tags column, and repair/check with meaningful exit codes.

use std::path::{Path, PathBuf};

use crate::tagstore::{ChildEntry, Store};

const USAGE: &str = "usage: archnav tag [--store DIR] <command> [args]

commands:
  init [DIR]                     create a tag store at a tree root
  add FILE TAG [TAG...]          add tags to a file
  add -t TAG [-t TAG...] FILE..  add tags to many files
  rm FILE TAG [TAG...]           remove tags (rm --all FILE.. clears)
  set FILE TAG [TAG...]          replace a file's tags
  ls [--plain] [PATH...]         list a directory with tags (default: cwd)
  find [TAG...] [--any TAG...] [--not TAG...] [--show-tags]
  tags                           list all tags with usage counts
  mv SRC DST                     move or rename a file and update the index
  repair [--prune] [--dry-run]   relink renamed/moved files by fingerprint
  check [--verify]               verify the index against the tree";

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok((code, lines)) => {
            for line in lines {
                println!("{}", line);
            }
            code
        }
        Err(err) => {
            eprintln!("archnav tag: {}", err);
            1
        }
    }
}

type CmdResult = Result<(i32, Vec<String>), String>;

fn dispatch(args: &[String]) -> CmdResult {
    let mut rest: Vec<String> = Vec::new();
    let mut store_override: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--store" {
            store_override = Some(it.next().ok_or("--store needs a directory")?.clone());
        } else {
            rest.push(a.clone());
        }
    }
    let Some(cmd) = rest.first().cloned() else {
        return Err(format!("missing command\n{}", USAGE));
    };
    let rest = &rest[1..];
    match cmd.as_str() {
        "init" => cmd_init(rest),
        "add" | "rm" | "set" => cmd_mutate(&cmd, rest, store_override.as_deref()),
        "ls" => cmd_ls(rest, store_override.as_deref()),
        "find" => cmd_find(rest, store_override.as_deref()),
        "tags" => cmd_tags(store_override.as_deref()),
        "mv" => cmd_mv(rest, store_override.as_deref()),
        "repair" => cmd_repair(rest, store_override.as_deref()),
        "check" => cmd_check(rest, store_override.as_deref()),
        "help" | "--help" | "-h" => Ok((0, vec![USAGE.to_string()])),
        other => Err(format!("unknown command {:?}\n{}", other, USAGE)),
    }
}

fn store_for(store_override: Option<&str>, anchor: Option<&Path>) -> Result<Store, String> {
    if let Some(dir) = store_override {
        return Store::open(Path::new(dir));
    }
    match anchor {
        Some(path) => Store::discover(path),
        None => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            Store::discover(&cwd)
        }
    }
}

// -- subcommands --------------------------------------------------------------

fn cmd_init(args: &[String]) -> CmdResult {
    let dir = args.first().map(String::as_str).unwrap_or(".");
    let store = Store::init(Path::new(dir))?;
    Ok((
        0,
        vec![format!("initialized tag store at {}", store.root.display())],
    ))
}

/// add/rm/set argument convention: with -t, every positional is a file;
/// without, the first positional is the file and the rest are tags.
fn cmd_mutate(cmd: &str, args: &[String], store_override: Option<&str>) -> CmdResult {
    let mut flag_tags: Vec<String> = Vec::new();
    let mut all = false;
    let mut positionals: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-t" | "--tag" => {
                flag_tags.push(it.next().ok_or("-t needs a tag")?.clone());
            }
            "--all" if cmd == "rm" => all = true,
            _ => positionals.push(a.clone()),
        }
    }

    let (files, tags): (Vec<PathBuf>, Vec<String>) = if all {
        (positionals.iter().map(PathBuf::from).collect(), Vec::new())
    } else if !flag_tags.is_empty() {
        (positionals.iter().map(PathBuf::from).collect(), flag_tags)
    } else {
        if positionals.len() < 2 {
            return Err("usage: FILE TAG [TAG...] or -t TAG [-t TAG...] FILE [FILE...]".into());
        }
        (
            vec![PathBuf::from(&positionals[0])],
            positionals[1..].to_vec(),
        )
    };
    if files.is_empty() {
        return Err("no files given".into());
    }

    let store = store_for(store_override, Some(&files[0]))?;
    let result = match cmd {
        "add" => store.add(&files, &tags)?,
        "rm" => store.remove(&files, &tags, all)?,
        "set" => store.set_tags(&files, &tags)?,
        _ => unreachable!(),
    };
    let lines = result
        .iter()
        .map(|(rel, tags)| format!("{}: {}", rel, tags.join(", ")))
        .collect();
    Ok((0, lines))
}

fn cmd_ls(args: &[String], store_override: Option<&str>) -> CmdResult {
    let plain = args.iter().any(|a| a == "--plain");
    let paths: Vec<PathBuf> = args
        .iter()
        .filter(|a| *a != "--plain")
        .map(PathBuf::from)
        .collect();
    let paths = if paths.is_empty() {
        vec![std::env::current_dir().map_err(|e| e.to_string())?]
    } else {
        paths
    };

    let store = store_for(store_override, Some(&paths[0]))?;
    let mut lines: Vec<String> = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if path.is_dir() {
            if paths.len() > 1 {
                if i > 0 {
                    lines.push(String::new());
                }
                lines.push(format!("{}:", path.display()));
            }
            let rows = store.list_dir(path)?;
            if plain {
                render_plain_listing(&rows, &mut lines);
            } else {
                render_long_listing(&rows, &mut lines);
            }
        } else {
            lines.push(format!(
                "{}: {}",
                store.rel(path)?,
                store.get(path)?.join(", ")
            ));
        }
    }
    Ok((0, lines))
}

fn cmd_find(args: &[String], store_override: Option<&str>) -> CmdResult {
    enum Bucket {
        Require,
        Any,
        Not,
    }
    let mut require: Vec<String> = Vec::new();
    let mut any_of: Vec<String> = Vec::new();
    let mut exclude: Vec<String> = Vec::new();
    let mut show_tags = false;
    let mut bucket = Bucket::Require;
    for a in args {
        match a.as_str() {
            "--any" => bucket = Bucket::Any,
            "--not" => bucket = Bucket::Not,
            "--show-tags" => show_tags = true,
            _ => match bucket {
                Bucket::Require => require.push(a.clone()),
                Bucket::Any => any_of.push(a.clone()),
                Bucket::Not => exclude.push(a.clone()),
            },
        }
    }
    let store = store_for(store_override, None)?;
    let results = store.find(&require, &any_of, &exclude)?;
    let lines = results
        .into_iter()
        .map(|(rel, tags)| {
            if show_tags {
                format!("{}: {}", rel, tags.join(", "))
            } else {
                rel
            }
        })
        .collect();
    Ok((0, lines))
}

fn cmd_tags(store_override: Option<&str>) -> CmdResult {
    let store = store_for(store_override, None)?;
    let counts = store.tag_counts()?;
    let width = counts
        .values()
        .max()
        .map(|m| m.to_string().len())
        .unwrap_or(1);
    let lines = counts
        .iter()
        .map(|(tag, n)| format!("{:>width$}  {}", n, tag, width = width))
        .collect();
    Ok((0, lines))
}

fn cmd_mv(args: &[String], store_override: Option<&str>) -> CmdResult {
    let [src, dst] = args else {
        return Err("usage: mv SRC DST".into());
    };
    let src = PathBuf::from(src);
    let store = store_for(store_override, Some(&src))?;
    let (from, to) = store.mv(&src, Path::new(dst))?;
    Ok((0, vec![format!("{} -> {}", from, to)]))
}

fn cmd_repair(args: &[String], store_override: Option<&str>) -> CmdResult {
    let prune = args.iter().any(|a| a == "--prune");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let store = store_for(store_override, None)?;
    let report = store.repair(prune, dry_run)?;

    let mut lines: Vec<String> = Vec::new();
    for (old, new) in &report.relinked {
        lines.push(format!("relinked: {} -> {}", old, new));
    }
    for rel in &report.refreshed {
        lines.push(format!("refreshed fingerprint: {}", rel));
    }
    for (old, cands) in &report.ambiguous {
        lines.push(format!(
            "ambiguous: {} matches {}; resolve manually with `archnav tag mv`",
            old,
            cands.join(", ")
        ));
    }
    for rel in &report.missing {
        let suffix = if report.pruned.contains(rel) {
            " (pruned)"
        } else {
            " (rerun with --prune to drop)"
        };
        lines.push(format!("missing: {}{}", rel, suffix));
    }
    if report.clean() {
        lines.push("index is consistent with the tree".into());
    }
    if dry_run {
        lines.push("dry run, no changes written".into());
    }
    let unresolved = !report.ambiguous.is_empty() || (!report.missing.is_empty() && !prune);
    Ok((if unresolved { 1 } else { 0 }, lines))
}

fn cmd_check(args: &[String], store_override: Option<&str>) -> CmdResult {
    let verify = args.iter().any(|a| a == "--verify");
    let store = store_for(store_override, None)?;
    let report = store.check(verify)?;

    let mut lines = vec![format!(
        "entries: {}  untracked files: {}",
        report.entry_count, report.untracked_count
    )];
    for rel in &report.missing {
        lines.push(format!("missing: {}", rel));
    }
    for rel in &report.modified {
        lines.push(format!("modified since tagging: {}", rel));
    }
    for rel in &report.fp_mismatch {
        lines.push(format!("fingerprint mismatch: {}", rel));
    }
    if report.ok() {
        lines.push("ok".into());
        Ok((0, lines))
    } else {
        lines.push("problems found; `archnav tag repair` reconciles the index".into());
        Ok((1, lines))
    }
}

// -- ls rendering (mimics ls -lh --group-directories-first plus a tags column) --

fn render_plain_listing(rows: &[ChildEntry], lines: &mut Vec<String>) {
    if rows.is_empty() {
        return;
    }
    let width = rows
        .iter()
        .map(|r| r.name.chars().count() + usize::from(r.is_dir))
        .max()
        .unwrap_or(0);
    for r in rows {
        if r.is_dir {
            lines.push(format!("{}/", r.name));
        } else {
            let tags = if r.tags.is_empty() {
                "-".to_string()
            } else {
                r.tags.join(", ")
            };
            lines.push(format!("{:<width$}  {}", r.name, tags, width = width));
        }
    }
}

fn render_long_listing(rows: &[ChildEntry], lines: &mut Vec<String>) {
    use std::os::unix::fs::MetadataExt;

    let ordered: Vec<&ChildEntry> = rows
        .iter()
        .filter(|r| r.is_dir)
        .chain(rows.iter().filter(|r| !r.is_dir))
        .collect();
    let blocks: u64 = ordered.iter().map(|r| r.meta.blocks()).sum();
    lines.push(format!("total {}", human_size(blocks * 512)));
    if ordered.is_empty() {
        return;
    }

    let now = now_unix();
    let cols: Vec<[String; 6]> = ordered
        .iter()
        .map(|r| {
            [
                mode_string(r.meta.mode()),
                r.meta.nlink().to_string(),
                user_name(r.meta.uid()),
                group_name(r.meta.gid()),
                human_size(r.meta.len()),
                format_mtime(r.meta.mtime(), now),
            ]
        })
        .collect();
    let width = |i: usize| cols.iter().map(|c| c[i].chars().count()).max().unwrap_or(0);
    let (w_nlink, w_user, w_group, w_size) = (width(1), width(2), width(3), width(4));
    let name_w = ordered
        .iter()
        .map(|r| r.name.chars().count() + usize::from(r.is_dir))
        .max()
        .unwrap_or(0);

    for (r, c) in ordered.iter().zip(&cols) {
        let left = format!(
            "{} {:>w_nlink$} {:<w_user$} {:<w_group$} {:>w_size$} {}",
            c[0], c[1], c[2], c[3], c[4], c[5]
        );
        if r.is_dir {
            lines.push(format!("{} {}/", left, r.name));
        } else {
            let tags = if r.tags.is_empty() {
                "-".to_string()
            } else {
                r.tags.join(", ")
            };
            lines.push(format!("{} {:<name_w$}  {}", left, r.name, tags));
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format a byte count like ls -h: powers of 1024, values rounded up.
fn human_size(size: u64) -> String {
    if size < 1024 {
        return size.to_string();
    }
    let mut value = size as f64;
    for unit in ["K", "M", "G", "T", "P", "E"] {
        value /= 1024.0;
        if value < 1024.0 || unit == "E" {
            if value < 10.0 {
                let tenths = (value * 10.0).ceil() / 10.0;
                if tenths < 10.0 {
                    return format!("{:.1}{}", tenths, unit);
                }
                return format!("{}{}", tenths.ceil() as u64, unit);
            }
            return format!("{}{}", value.ceil() as u64, unit);
        }
    }
    unreachable!()
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const SIX_MONTHS: i64 = (182.5 * 24.0 * 3600.0) as i64;

/// Format a timestamp like ls -l: time of day when recent, year otherwise.
fn format_mtime(mtime: i64, now: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t: libc::time_t = mtime;
    unsafe { libc::localtime_r(&t, &mut tm) };
    let month = MONTHS.get(tm.tm_mon as usize).unwrap_or(&"???");
    let day = format!("{} {:2}", month, tm.tm_mday);
    if now - SIX_MONTHS <= mtime && mtime <= now + 60 {
        format!("{} {:02}:{:02}", day, tm.tm_hour, tm.tm_min)
    } else {
        format!("{}  {}", day, tm.tm_year as i64 + 1900)
    }
}

/// Permission string like coreutils / Python's stat.filemode.
fn mode_string(mode: u32) -> String {
    let file_type = match mode & libc::S_IFMT {
        libc::S_IFDIR => 'd',
        libc::S_IFLNK => 'l',
        libc::S_IFREG => '-',
        libc::S_IFBLK => 'b',
        libc::S_IFCHR => 'c',
        libc::S_IFIFO => 'p',
        libc::S_IFSOCK => 's',
        _ => '?',
    };
    let mut s = String::with_capacity(10);
    s.push(file_type);
    let bit = |b: u32, c: char| if mode & b != 0 { c } else { '-' };
    s.push(bit(0o400, 'r'));
    s.push(bit(0o200, 'w'));
    s.push(match (mode & 0o100 != 0, mode & 0o4000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    s.push(bit(0o040, 'r'));
    s.push(bit(0o020, 'w'));
    s.push(match (mode & 0o010 != 0, mode & 0o2000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    s.push(bit(0o004, 'r'));
    s.push(bit(0o002, 'w'));
    s.push(match (mode & 0o001 != 0, mode & 0o1000 != 0) {
        (true, true) => 't',
        (false, true) => 'T',
        (true, false) => 'x',
        (false, false) => '-',
    });
    s
}

fn user_name(uid: u32) -> String {
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = [0i8; 2048];
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc == 0 && !result.is_null() {
            return std::ffi::CStr::from_ptr(pwd.pw_name)
                .to_string_lossy()
                .into_owned();
        }
    }
    uid.to_string()
}

fn group_name(gid: u32) -> String {
    unsafe {
        let mut grp: libc::group = std::mem::zeroed();
        let mut buf = [0i8; 2048];
        let mut result: *mut libc::group = std::ptr::null_mut();
        let rc = libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc == 0 && !result.is_null() {
            return std::ffi::CStr::from_ptr(grp.gr_name)
                .to_string_lossy()
                .into_owned();
        }
    }
    gid.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_fixture() -> (tempfile::TempDir, String) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        Store::init(&root).unwrap();
        let root_str = root.to_string_lossy().into_owned();
        (tmp, root_str)
    }

    fn a(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn write(root: &str, rel: &str, data: &[u8]) -> String {
        let p = Path::new(root).join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn add_positional_and_flag_forms() {
        let (_tmp, root) = store_fixture();
        let doc = write(&root, "doc.pdf", b"content");
        let (code, lines) = dispatch(&a(&["add", &doc, "invoice", "taxes"])).unwrap();
        assert_eq!(code, 0);
        assert_eq!(lines, vec!["doc.pdf: invoice, taxes"]);

        let f1 = write(&root, "a.txt", b"a");
        let f2 = write(&root, "b.txt", b"b");
        let (code, lines) = dispatch(&a(&["add", "-t", "bulk", &f1, &f2])).unwrap();
        assert_eq!(code, 0);
        assert_eq!(lines, vec!["a.txt: bulk", "b.txt: bulk"]);
    }

    #[test]
    fn rm_all_and_set() {
        let (_tmp, root) = store_fixture();
        let doc = write(&root, "doc.pdf", b"content");
        dispatch(&a(&["add", &doc, "a", "b"])).unwrap();
        let (_, lines) = dispatch(&a(&["set", &doc, "new"])).unwrap();
        assert_eq!(lines, vec!["doc.pdf: new"]);
        let (code, lines) = dispatch(&a(&["rm", "--all", &doc])).unwrap();
        assert_eq!(code, 0);
        assert_eq!(lines, vec!["doc.pdf: "]);
        let (_, lines) = dispatch(&a(&["find", "--store", &root])).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn find_flags_and_show_tags() {
        let (_tmp, root) = store_fixture();
        let f1 = write(&root, "a.txt", b"a");
        let f2 = write(&root, "b.txt", b"b");
        dispatch(&a(&["add", &f1, "work", "invoice"])).unwrap();
        dispatch(&a(&["add", &f2, "work", "report"])).unwrap();

        let (_, lines) = dispatch(&a(&["--store", &root, "find", "work"])).unwrap();
        assert_eq!(lines, vec!["a.txt", "b.txt"]);
        let (_, lines) =
            dispatch(&a(&["--store", &root, "find", "work", "--not", "invoice"])).unwrap();
        assert_eq!(lines, vec!["b.txt"]);
        let (_, lines) = dispatch(&a(&[
            "--store", &root, "find", "--any", "invoice", "report",
        ]))
        .unwrap();
        assert_eq!(lines, vec!["a.txt", "b.txt"]);
        let (_, lines) =
            dispatch(&a(&["--store", &root, "find", "invoice", "--show-tags"])).unwrap();
        assert_eq!(lines, vec!["a.txt: invoice, work"]);
    }

    #[test]
    fn tags_counts_aligned() {
        let (_tmp, root) = store_fixture();
        let f1 = write(&root, "a.txt", b"a");
        let f2 = write(&root, "b.txt", b"b");
        dispatch(&a(&["add", &f1, "work"])).unwrap();
        dispatch(&a(&["add", &f2, "work", "home"])).unwrap();
        let (_, lines) = dispatch(&a(&["--store", &root, "tags"])).unwrap();
        assert_eq!(lines, vec!["1  home", "2  work"]);
    }

    #[test]
    fn ls_plain_and_long() {
        let (_tmp, root) = store_fixture();
        let doc = write(&root, "beach.jpg", b"x");
        write(&root, "untagged.jpg", b"y");
        std::fs::create_dir(Path::new(&root).join("sub")).unwrap();
        dispatch(&a(&["add", &doc, "vacation"])).unwrap();

        let (_, lines) = dispatch(&a(&["ls", "--plain", &root])).unwrap();
        assert_eq!(
            lines,
            vec!["beach.jpg     vacation", "sub/", "untagged.jpg  -"]
        );

        let (_, lines) = dispatch(&a(&["ls", &root])).unwrap();
        assert!(lines[0].starts_with("total "));
        // directories first, like ls --group-directories-first
        assert!(
            lines[1].starts_with('d') && lines[1].ends_with(" sub/"),
            "got: {}",
            lines[1]
        );
        assert!(lines[2].contains("beach.jpg") && lines[2].ends_with("vacation"));
        assert!(lines[3].contains("untagged.jpg") && lines[3].ends_with('-'));
    }

    #[test]
    fn ls_single_file_shows_tags_line() {
        let (_tmp, root) = store_fixture();
        let doc = write(&root, "doc.pdf", b"x");
        dispatch(&a(&["add", &doc, "z"])).unwrap();
        let (_, lines) = dispatch(&a(&["ls", &doc])).unwrap();
        assert_eq!(lines, vec!["doc.pdf: z"]);
    }

    #[test]
    fn mv_and_repair_and_check_exit_codes() {
        let (_tmp, root) = store_fixture();
        let doc = write(&root, "old.txt", b"unique content");
        dispatch(&a(&["add", &doc, "keep"])).unwrap();

        let dst = Path::new(&root).join("new.txt");
        let (code, lines) = dispatch(&a(&["mv", &doc, &dst.to_string_lossy()])).unwrap();
        assert_eq!(code, 0);
        assert_eq!(lines, vec!["old.txt -> new.txt"]);

        // rename behind the tool's back; repair relinks, exit 0
        std::fs::rename(&dst, Path::new(&root).join("moved.txt")).unwrap();
        let (code, lines) = dispatch(&a(&["--store", &root, "repair"])).unwrap();
        assert_eq!(code, 0);
        assert_eq!(lines[0], "relinked: new.txt -> moved.txt");

        let (code, lines) = dispatch(&a(&["--store", &root, "check"])).unwrap();
        assert_eq!(code, 0);
        assert_eq!(lines.last().unwrap(), "ok");

        // delete the file: check exits 1; repair without --prune exits 1
        std::fs::remove_file(Path::new(&root).join("moved.txt")).unwrap();
        let (code, _) = dispatch(&a(&["--store", &root, "check"])).unwrap();
        assert_eq!(code, 1);
        let (code, lines) = dispatch(&a(&["--store", &root, "repair"])).unwrap();
        assert_eq!(code, 1);
        assert!(lines[0].starts_with("missing: moved.txt"));
        let (code, _) = dispatch(&a(&["--store", &root, "repair", "--prune"])).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn errors_are_reported() {
        let (_tmp, root) = store_fixture();
        assert!(dispatch(&a(&["bogus"])).is_err());
        assert!(dispatch(&[]).is_err());
        let doc = write(&root, "doc.pdf", b"x");
        assert!(dispatch(&a(&["add", &doc])).is_err()); // no tags
        assert!(dispatch(&a(&["add", &doc, "a,b"]))
            .unwrap_err()
            .contains("comma"));
        let outside = tempfile::tempdir().unwrap();
        let stray = outside.path().join("f.txt");
        std::fs::write(&stray, b"x").unwrap();
        let err = dispatch(&a(&["add", &stray.to_string_lossy(), "x"])).unwrap_err();
        assert!(err.contains("no .tagstore"), "got: {}", err);
    }

    #[test]
    fn init_reports_and_rejects_double_init() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_string_lossy().into_owned();
        let (code, lines) = dispatch(&a(&["init", &dir])).unwrap();
        assert_eq!(code, 0);
        assert!(lines[0].starts_with("initialized tag store at "));
        assert!(dispatch(&a(&["init", &dir]))
            .unwrap_err()
            .contains("already initialized"));
    }

    // -- formatting helpers, values cross-checked against coreutils ls -h ----

    #[test]
    fn human_size_matches_ls() {
        assert_eq!(human_size(0), "0");
        assert_eq!(human_size(287), "287");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1025), "1.1K"); // ls ceils, never truncates
        assert_eq!(human_size(583_680), "570K");
        assert_eq!(human_size(3_437_000), "3.3M");
        assert_eq!(human_size(25_000_000), "24M");
    }

    #[test]
    fn mode_string_common_cases() {
        assert_eq!(mode_string(libc::S_IFREG | 0o644), "-rw-r--r--");
        assert_eq!(mode_string(libc::S_IFDIR | 0o755), "drwxr-xr-x");
        assert_eq!(mode_string(libc::S_IFREG | 0o770), "-rwxrwx---");
        assert_eq!(mode_string(libc::S_IFREG | 0o4755), "-rwsr-xr-x");
        assert_eq!(mode_string(libc::S_IFDIR | 0o1777), "drwxrwxrwt");
    }

    #[test]
    fn format_mtime_recent_vs_old() {
        let now = now_unix();
        let recent = format_mtime(now - 3600, now);
        assert!(recent.contains(':'), "got: {}", recent);
        let old = format_mtime(now - 400 * 24 * 3600, now);
        assert!(!old.contains(':'), "got: {}", old);
        assert!(old.len() >= 11); // "Mon DD  YYYY"
    }
}
