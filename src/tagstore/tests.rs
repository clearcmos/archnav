//! Behavior tests for the tag store engine, ported from the original
//! Python tagdex test suite so the Rust engine is held to the same
//! contract the format was designed under.

use super::*;

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let store = Store::init(&root).unwrap();
    Fixture {
        _tmp: tmp,
        root,
        store,
    }
}

impl Fixture {
    fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let p = self.root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, data).unwrap();
        p
    }

    fn tags_of(&self, rel: &str) -> Vec<String> {
        self.store.get(&self.root.join(rel)).unwrap()
    }
}

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|i| i.to_string()).collect()
}

// -- init / discover --------------------------------------------------------

#[test]
fn init_twice_fails() {
    let f = fixture();
    assert!(Store::init(&f.root)
        .unwrap_err()
        .contains("already initialized"));
}

#[test]
fn discover_walks_up_from_nested_file() {
    let f = fixture();
    let file = f.write("a/b/doc.txt", b"x");
    assert_eq!(Store::discover(&file).unwrap().root, f.root);
    assert_eq!(Store::discover(&f.root.join("a/b")).unwrap().root, f.root);
}

#[test]
fn discover_outside_fails() {
    let other = tempfile::tempdir().unwrap();
    assert!(Store::discover(other.path()).is_err());
}

// -- tag validation ---------------------------------------------------------

#[test]
fn validate_tag_rules() {
    assert_eq!(validate_tag("  invoice ").unwrap(), "invoice");
    assert_eq!(
        validate_tag("client:acmé corp").unwrap(),
        "client:acmé corp"
    );
    assert!(validate_tag("   ").is_err());
    assert!(validate_tag("a,b").unwrap_err().contains("comma"));
    assert!(validate_tag("a\nb").is_err());
}

// -- mutations ---------------------------------------------------------------

#[test]
fn add_get_merge_dedupe() {
    let f = fixture();
    let doc = f.write("doc.pdf", b"content");
    let result = f
        .store
        .add(std::slice::from_ref(&doc), &s(&["taxes", "invoice"]))
        .unwrap();
    assert_eq!(result["doc.pdf"], s(&["invoice", "taxes"]));
    f.store.add(&[doc], &s(&["b", "invoice"])).unwrap();
    assert_eq!(f.tags_of("doc.pdf"), s(&["b", "invoice", "taxes"]));
}

#[test]
fn remove_tag_and_drop_empty_entries() {
    let f = fixture();
    let doc = f.write("doc.pdf", b"content");
    f.store
        .add(std::slice::from_ref(&doc), &s(&["only"]))
        .unwrap();
    f.store.remove(&[doc], &s(&["only"]), false).unwrap();
    assert_eq!(f.store.find(&[], &[], &[]).unwrap(), vec![]);
}

#[test]
fn remove_all_and_set_replace() {
    let f = fixture();
    let doc = f.write("doc.pdf", b"content");
    f.store
        .add(std::slice::from_ref(&doc), &s(&["a", "b"]))
        .unwrap();
    f.store
        .set_tags(std::slice::from_ref(&doc), &s(&["new"]))
        .unwrap();
    assert_eq!(f.tags_of("doc.pdf"), s(&["new"]));
    f.store.remove(&[doc], &[], true).unwrap();
    assert_eq!(f.tags_of("doc.pdf"), Vec::<String>::new());
}

#[test]
fn rejects_bad_targets() {
    let f = fixture();
    let doc = f.write("doc.pdf", b"content");
    assert!(f.store.add(&[doc], &[]).is_err()); // no tags
    let d = f.root.join("subdir");
    std::fs::create_dir(&d).unwrap();
    assert!(f.store.add(&[d], &s(&["x"])).is_err()); // directory
    assert!(f
        .store
        .add(&[f.root.join("ghost.txt")], &s(&["x"]))
        .is_err()); // missing
    assert!(f
        .store
        .add(std::slice::from_ref(&f.store.index_path), &s(&["x"]))
        .is_err()); // in .tagstore
    let outside = tempfile::NamedTempFile::new().unwrap();
    assert!(f
        .store
        .add(&[outside.path().to_path_buf()], &s(&["x"]))
        .is_err());
}

#[test]
fn unicode_and_spaces_in_names() {
    let f = fixture();
    let doc = f.write("Top Picks/café menu.pdf", b"content");
    f.store.add(&[doc], &s(&["menu"])).unwrap();
    assert_eq!(
        f.store.find(&s(&["menu"]), &[], &[]).unwrap(),
        vec![("Top Picks/café menu.pdf".to_string(), s(&["menu"]))]
    );
}

// -- queries ------------------------------------------------------------------

fn query_fixture() -> Fixture {
    let f = fixture();
    f.store
        .add(&[f.write("a.txt", b"a")], &s(&["work", "invoice"]))
        .unwrap();
    f.store
        .add(&[f.write("b.txt", b"b")], &s(&["work", "report"]))
        .unwrap();
    f.store
        .add(&[f.write("c.txt", b"c")], &s(&["home"]))
        .unwrap();
    f
}

#[test]
fn find_semantics() {
    let f = query_fixture();
    let paths = |req: &[&str], any: &[&str], not: &[&str]| -> Vec<String> {
        f.store
            .find(&s(req), &s(any), &s(not))
            .unwrap()
            .into_iter()
            .map(|(rel, _)| rel)
            .collect()
    };
    assert_eq!(paths(&["work", "invoice"], &[], &[]), s(&["a.txt"])); // AND
    assert_eq!(
        paths(&[], &["invoice", "home"], &[]),
        s(&["a.txt", "c.txt"])
    ); // OR
    assert_eq!(paths(&["work"], &[], &["invoice"]), s(&["b.txt"])); // NOT
    assert_eq!(paths(&[], &[], &[]), s(&["a.txt", "b.txt", "c.txt"])); // all
}

#[test]
fn tag_counts() {
    let f = query_fixture();
    let counts = f.store.tag_counts().unwrap();
    assert_eq!(counts["work"], 2);
    assert_eq!(counts["home"], 1);
}

// -- list_dir -----------------------------------------------------------------

#[test]
fn list_dir_children_with_tags() {
    let f = fixture();
    f.store
        .add(&[f.write("b.txt", b"content")], &s(&["beta"]))
        .unwrap();
    f.write("a.txt", b"12345");
    std::fs::create_dir(f.root.join("sub")).unwrap();
    let rows = f.store.list_dir(&f.root).unwrap();
    let summary: Vec<(String, bool, Vec<String>)> = rows
        .iter()
        .map(|r| (r.name.clone(), r.is_dir, r.tags.clone()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a.txt".into(), false, vec![]),
            ("b.txt".into(), false, s(&["beta"])),
            ("sub".into(), true, vec![]),
        ]
    );
    assert_eq!(rows[0].meta.len(), 5); // stat carried for ls -l rendering
    assert!(!rows.iter().any(|r| r.name == STORE_DIR));
}

#[test]
fn list_dir_uses_relative_keys_in_subdir() {
    let f = fixture();
    f.store
        .add(&[f.write("sub/doc.txt", b"x")], &s(&["t"]))
        .unwrap();
    let rows = f.store.list_dir(&f.root.join("sub")).unwrap();
    assert_eq!(rows[0].tags, s(&["t"]));
}

// -- mv -----------------------------------------------------------------------

#[test]
fn mv_rekeys_entry_and_preserves_fingerprint() {
    let f = fixture();
    let src = f.write("old.txt", b"content");
    f.store
        .add(std::slice::from_ref(&src), &s(&["keep"]))
        .unwrap();
    let fp_before = f.store.load().unwrap().entries["old.txt"].fp.clone();
    std::fs::create_dir(f.root.join("sub")).unwrap();
    let (from, to) = f.store.mv(&src, &f.root.join("sub/new.txt")).unwrap();
    assert_eq!((from.as_str(), to.as_str()), ("old.txt", "sub/new.txt"));
    let entries = f.store.load().unwrap().entries;
    assert!(!entries.contains_key("old.txt"));
    assert_eq!(entries["sub/new.txt"].tags, s(&["keep"]));
    assert_eq!(entries["sub/new.txt"].fp, fp_before);
}

#[test]
fn mv_into_directory_and_refuses_overwrite() {
    let f = fixture();
    let src = f.write("doc.txt", b"content");
    std::fs::create_dir(f.root.join("dest")).unwrap();
    f.store.add(std::slice::from_ref(&src), &s(&["t"])).unwrap();
    let (_, to) = f.store.mv(&src, &f.root.join("dest")).unwrap();
    assert_eq!(to, "dest/doc.txt");

    let a = f.write("a.txt", b"a");
    f.write("b.txt", b"b");
    assert!(f
        .store
        .mv(&a, &f.root.join("b.txt"))
        .unwrap_err()
        .contains("already exists"));
    assert!(a.exists()); // nothing was renamed
}

// -- repair ---------------------------------------------------------------------

#[test]
fn repair_relinks_after_external_rename_and_move() {
    let f = fixture();
    let doc = f.write("old name.txt", b"unique content 1");
    f.store
        .add(std::slice::from_ref(&doc), &s(&["keep", "these"]))
        .unwrap();
    std::fs::create_dir(f.root.join("archive")).unwrap();
    std::fs::rename(&doc, f.root.join("archive/renamed.txt")).unwrap();
    let report = f.store.repair(false, false).unwrap();
    assert_eq!(
        report.relinked,
        vec![(
            "old name.txt".to_string(),
            "archive/renamed.txt".to_string()
        )]
    );
    assert_eq!(f.tags_of("archive/renamed.txt"), s(&["keep", "these"]));
}

#[test]
fn repair_reports_ambiguous_duplicates_without_guessing() {
    let f = fixture();
    let doc = f.write("orig.txt", b"same bytes");
    f.store.add(std::slice::from_ref(&doc), &s(&["t"])).unwrap();
    std::fs::remove_file(&doc).unwrap();
    f.write("copy1.txt", b"same bytes");
    f.write("copy2.txt", b"same bytes");
    let report = f.store.repair(false, false).unwrap();
    assert!(report.relinked.is_empty());
    assert_eq!(report.ambiguous.len(), 1);
    assert!(f.store.load().unwrap().entries.contains_key("orig.txt")); // kept for manual resolution
}

#[test]
fn repair_missing_prune_and_dry_run() {
    let f = fixture();
    let doc = f.write("gone.txt", b"unique content 2");
    f.store.add(std::slice::from_ref(&doc), &s(&["t"])).unwrap();
    std::fs::remove_file(&doc).unwrap();

    let report = f.store.repair(false, false).unwrap();
    assert_eq!(report.missing, s(&["gone.txt"]));
    assert!(f.store.load().unwrap().entries.contains_key("gone.txt"));

    let report = f.store.repair(true, true).unwrap(); // dry run: nothing written
    assert_eq!(report.pruned, s(&["gone.txt"]));
    assert!(f.store.load().unwrap().entries.contains_key("gone.txt"));

    let report = f.store.repair(true, false).unwrap();
    assert_eq!(report.pruned, s(&["gone.txt"]));
    assert!(!f.store.load().unwrap().entries.contains_key("gone.txt"));
}

#[test]
fn repair_refreshes_modified_files() {
    let f = fixture();
    let doc = f.write("doc.txt", b"version 1");
    f.store.add(std::slice::from_ref(&doc), &s(&["t"])).unwrap();
    let fp_before = f.store.load().unwrap().entries["doc.txt"].fp.clone();
    std::fs::write(&doc, b"version 2, longer").unwrap();
    let report = f.store.repair(false, false).unwrap();
    assert_eq!(report.refreshed, s(&["doc.txt"]));
    let entry = &f.store.load().unwrap().entries["doc.txt"];
    assert_ne!(entry.fp, fp_before);
    assert_eq!(entry.tags, s(&["t"]));
}

// -- check -----------------------------------------------------------------------

#[test]
fn check_reports_missing_modified_and_verify_mismatch() {
    let f = fixture();
    let doc = f.write("doc.txt", b"aaaa");
    f.store.add(std::slice::from_ref(&doc), &s(&["t"])).unwrap();
    assert!(f.store.check(false).unwrap().ok());

    // Same size, mtime restored: only --verify's re-fingerprint can see it.
    let meta = std::fs::metadata(&doc).unwrap();
    std::fs::write(&doc, b"bbbb").unwrap();
    restore_mtime(&doc, &meta);
    assert!(f.store.check(false).unwrap().ok());
    assert_eq!(f.store.check(true).unwrap().fp_mismatch, s(&["doc.txt"]));

    std::fs::remove_file(&doc).unwrap();
    let report = f.store.check(false).unwrap();
    assert_eq!(report.missing, s(&["doc.txt"]));
    assert!(!report.ok());
}

fn restore_mtime(path: &Path, meta: &std::fs::Metadata) {
    use std::os::unix::ffi::OsStrExt;
    let times = [
        libc::timespec {
            tv_sec: meta.atime(),
            tv_nsec: meta.atime_nsec(),
        },
        libc::timespec {
            tv_sec: meta.mtime(),
            tv_nsec: meta.mtime_nsec(),
        },
    ];
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, cpath.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0);
}

// -- GUI read path -----------------------------------------------------------------

#[test]
fn read_tags_no_store_vs_untagged_vs_tagged() {
    let outside = tempfile::tempdir().unwrap();
    let stray = outside.path().join("f.txt");
    std::fs::write(&stray, b"x").unwrap();
    assert!(matches!(read_tags(&stray).unwrap(), TagLookup::NoStore));

    let f = fixture();
    let doc = f.write("doc.txt", b"x");
    match read_tags(&doc).unwrap() {
        TagLookup::Tags(t) => assert!(t.is_empty()),
        TagLookup::NoStore => panic!("expected empty tags, not NoStore"),
    }
    f.store.add(std::slice::from_ref(&doc), &s(&["a"])).unwrap();
    match read_tags(&doc).unwrap() {
        TagLookup::Tags(t) => assert_eq!(t, s(&["a"])),
        TagLookup::NoStore => panic!("expected tags"),
    }
}

#[test]
fn write_invalidates_read_cache() {
    let f = fixture();
    let doc = f.write("doc.txt", b"x");
    // Prime the cache, then write through the GUI entry point.
    assert!(matches!(read_tags(&doc).unwrap(), TagLookup::Tags(_)));
    let tags = set_tags_for_file(&doc, &s(&["fresh"])).unwrap();
    assert_eq!(tags, s(&["fresh"]));
    match read_tags(&doc).unwrap() {
        TagLookup::Tags(t) => assert_eq!(t, s(&["fresh"])),
        TagLookup::NoStore => panic!(),
    }
    // Clearing through the same entry point works too.
    assert_eq!(set_tags_for_file(&doc, &[]).unwrap(), Vec::<String>::new());
}

#[test]
fn parse_tag_input_splits_and_trims() {
    assert_eq!(parse_tag_input(" a, b tag ,, c "), s(&["a", "b tag", "c"]));
    assert!(parse_tag_input("  ").is_empty());
}
