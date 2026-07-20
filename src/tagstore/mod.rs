//! Native tag store engine (the tagdex format, version 1).
//!
//! A store is a directory tree whose root contains .tagstore/, with a single
//! JSON index mapping slash-separated root-relative paths to tag entries.
//! Content fingerprints let `repair` relink files renamed or moved outside
//! the tool; writes are atomic same-directory renames under a mkdir lock,
//! the two primitives that are reliable over SMB/CIFS (where user xattrs,
//! byte-range locks, and therefore SQLite are not).
//!
//! This module is the single writer for the format. It started as a
//! read-only view over the external Python `tagdex` CLI and absorbed it
//! wholesale so a public archnav is self-sufficient; docs/tagstore-format.md
//! is the format reference, and the fingerprint/index parity tests pin
//! compatibility with stores written by the Python implementation.

pub mod fingerprint;
pub mod index;
pub mod lock;
pub mod xattrs;

use std::collections::{BTreeMap, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use fingerprint::fingerprint_file;
use index::{Entry, Index};

pub const STORE_DIR: &str = ".tagstore";
const INDEX_FILE: &str = "index.json";

fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.mtime() * 1_000_000_000 + meta.mtime_nsec()
}

// ---------------------------------------------------------------------------
// Read caches (GUI hot path: per-row tag decoration on every keystroke)

struct CachedIndex {
    mtime: SystemTime,
    size: u64,
    tags: Arc<HashMap<String, Vec<String>>>,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CachedIndex>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedIndex>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn invalidate_cache(root: &Path) {
    cache().lock().unwrap().remove(root);
}

/// Store-root discovery cache, keyed by the file's parent directory. The
/// results list decorates every row with tags on each keystroke, and the
/// walk-up stats behind discovery are expensive on CIFS mounts. The short
/// TTL also bounds how stale a freshly created (or deleted) .tagstore can
/// look inside a running archnav.
const DIR_ROOT_TTL: Duration = Duration::from_secs(5);

type DirRootCache = HashMap<PathBuf, (Instant, Option<PathBuf>)>;

fn dir_root_cache() -> &'static Mutex<DirRootCache> {
    static CACHE: OnceLock<Mutex<DirRootCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Walk upward from the file's directory looking for a .tagstore dir.
pub fn find_store_root(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    if let Some((at, root)) = dir_root_cache().lock().unwrap().get(dir) {
        if at.elapsed() < DIR_ROOT_TTL {
            return root.clone();
        }
    }
    let root = discover_root(dir);
    dir_root_cache()
        .lock()
        .unwrap()
        .insert(dir.to_path_buf(), (Instant::now(), root.clone()));
    root
}

fn discover_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start;
    loop {
        if cur.join(STORE_DIR).is_dir() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

/// Cached read of a store root's tag map (rel path -> tags), used by the
/// results-list decoration and the t: search filter.
pub fn entries_for_root(root: &Path) -> Result<Arc<HashMap<String, Vec<String>>>, String> {
    let index_path = root.join(STORE_DIR).join(INDEX_FILE);
    let meta = std::fs::metadata(&index_path)
        .map_err(|e| format!("cannot stat {}: {}", index_path.display(), e))?;
    let mtime = meta.modified().map_err(|e| e.to_string())?;
    let size = meta.len();

    if let Some(cached) = cache().lock().unwrap().get(root) {
        if cached.mtime == mtime && cached.size == size {
            return Ok(cached.tags.clone());
        }
    }

    let idx = Index::load(&index_path)?;
    let tags: Arc<HashMap<String, Vec<String>>> = Arc::new(
        idx.entries
            .into_iter()
            .map(|(rel, e)| (rel, e.tags))
            .collect(),
    );
    cache().lock().unwrap().insert(
        root.to_path_buf(),
        CachedIndex {
            mtime,
            size,
            tags: tags.clone(),
        },
    );
    Ok(tags)
}

/// Result of looking up a file's tags.
#[derive(Debug)]
pub enum TagLookup {
    /// No .tagstore directory exists above the file.
    NoStore,
    /// The file is inside a store; an empty vec means untagged.
    Tags(Vec<String>),
}

/// Look up the tags of an absolute file path.
pub fn read_tags(file: &Path) -> Result<TagLookup, String> {
    let Some(root) = find_store_root(file) else {
        return Ok(TagLookup::NoStore);
    };
    let tags = entries_for_root(&root)?;
    let rel = file
        .strip_prefix(&root)
        .map_err(|_| format!("{} is not under {}", file.display(), root.display()))?
        .to_string_lossy()
        .into_owned();
    Ok(TagLookup::Tags(
        tags.get(rel.as_str()).cloned().unwrap_or_default(),
    ))
}

/// Parse the tag dialog's comma-separated input into clean tags.
/// Commas are invalid inside tags, so they are a safe separator.
pub fn parse_tag_input(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Replace a file's full tag set (empty clears). The write entry point used
/// by the GUI; discovers the store from the file's location.
pub fn set_tags_for_file(file: &Path, tags: &[String]) -> Result<Vec<String>, String> {
    let store = Store::discover(file)?;
    let result = if tags.is_empty() {
        store.remove(&[file.to_path_buf()], &[], true)?
    } else {
        store.set_tags(&[file.to_path_buf()], tags)?
    };
    Ok(result.into_values().next().unwrap_or_default())
}

pub fn validate_tag(tag: &str) -> Result<String, String> {
    let cleaned = tag.trim();
    if cleaned.is_empty() {
        return Err("empty tag".to_string());
    }
    if cleaned.contains(',') {
        return Err(format!(
            "tag {:?} contains a comma (reserved as the tag separator)",
            cleaned
        ));
    }
    if cleaned.chars().any(|c| (c as u32) < 32) {
        return Err(format!("tag {:?} contains control characters", tag));
    }
    Ok(cleaned.to_string())
}

// ---------------------------------------------------------------------------
// Reports

#[derive(Debug, Default)]
pub struct RepairReport {
    pub relinked: Vec<(String, String)>,
    pub refreshed: Vec<String>,
    pub missing: Vec<String>,
    pub ambiguous: Vec<(String, Vec<String>)>,
    pub pruned: Vec<String>,
}

impl RepairReport {
    pub fn clean(&self) -> bool {
        self.relinked.is_empty()
            && self.refreshed.is_empty()
            && self.missing.is_empty()
            && self.ambiguous.is_empty()
            && self.pruned.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct CheckReport {
    pub missing: Vec<String>,
    pub modified: Vec<String>,
    pub fp_mismatch: Vec<String>,
    pub entry_count: usize,
    pub untracked_count: usize,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.modified.is_empty() && self.fp_mismatch.is_empty()
    }
}

/// One immediate child of a listed directory (for `tag ls`).
pub struct ChildEntry {
    pub name: String,
    pub is_dir: bool,
    pub tags: Vec<String>,
    pub meta: std::fs::Metadata,
}

// ---------------------------------------------------------------------------
// Store

#[derive(Debug)]
pub struct Store {
    pub root: PathBuf,
    pub store_dir: PathBuf,
    pub index_path: PathBuf,
}

impl Store {
    fn at(root: PathBuf) -> Store {
        let store_dir = root.join(STORE_DIR);
        let index_path = store_dir.join(INDEX_FILE);
        Store {
            root,
            store_dir,
            index_path,
        }
    }

    /// Create a store at a tree root.
    pub fn init(root: &Path) -> Result<Store, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {}", root.display(), e))?;
        if !root.is_dir() {
            return Err(format!("{} is not a directory", root.display()));
        }
        let store = Store::at(root);
        if store.store_dir.exists() {
            return Err(format!(
                "store already initialized at {}",
                store.root.display()
            ));
        }
        std::fs::create_dir(&store.store_dir)
            .map_err(|e| format!("cannot create {}: {}", store.store_dir.display(), e))?;
        Index::default().save(&store.index_path)?;
        Ok(store)
    }

    /// Open an explicit store root (must contain .tagstore).
    pub fn open(root: &Path) -> Result<Store, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {}", root.display(), e))?;
        if !root.join(STORE_DIR).is_dir() {
            return Err(format!("{} has no {}", root.display(), STORE_DIR));
        }
        Ok(Store::at(root))
    }

    /// Walk upward from the anchor (file or directory) to find a store.
    pub fn discover(anchor: &Path) -> Result<Store, String> {
        let start = absolutize(anchor)?;
        let start_dir = if start.is_dir() {
            start.clone()
        } else {
            start
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(start.clone())
        };
        match discover_root(&start_dir) {
            Some(root) => Ok(Store::at(root)),
            None => Err(format!(
                "no {} found from {} upward; run `archnav tag init` at the tree root",
                STORE_DIR,
                anchor.display()
            )),
        }
    }

    /// The slash-separated root-relative index key for a path.
    pub fn rel(&self, path: &Path) -> Result<String, String> {
        let resolved = absolutize(path)?;
        let r = resolved.strip_prefix(&self.root).map_err(|_| {
            format!(
                "{} is outside the store root {}",
                path.display(),
                self.root.display()
            )
        })?;
        if r.as_os_str().is_empty() {
            return Err(format!("{} is the store root, not a file", path.display()));
        }
        if r.components().any(|c| c.as_os_str() == STORE_DIR) {
            return Err(format!("{} is inside {}", path.display(), STORE_DIR));
        }
        Ok(r.to_string_lossy().into_owned())
    }

    pub fn abspath(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn load(&self) -> Result<Index, String> {
        if !self.index_path.is_file() {
            return Err(format!(
                "missing {}; store is corrupt or not initialized",
                self.index_path.display()
            ));
        }
        Index::load(&self.index_path)
    }

    fn require_regular_file(&self, path: &Path) -> Result<std::fs::Metadata, String> {
        let meta = std::fs::symlink_metadata(path)
            .map_err(|_| format!("{}: no such file", path.display()))?;
        if !meta.is_file() {
            return Err(format!(
                "{}: not a regular file (directories and symlinks cannot be tagged)",
                path.display()
            ));
        }
        Ok(meta)
    }

    fn entry_for(
        &self,
        path: &Path,
        tags: Vec<String>,
        prev: Option<&Entry>,
    ) -> Result<Entry, String> {
        let meta = self.require_regular_file(path)?;
        let (size, mtime) = (meta.len(), mtime_ns(&meta));
        let fp = match prev {
            // Stat unchanged since last fingerprint: skip re-reading (matters on NAS).
            Some(p) if p.size == size && p.mtime_ns == mtime => p.fp.clone(),
            _ => fingerprint_file(path)?,
        };
        Ok(Entry {
            fp,
            mtime_ns: mtime,
            size,
            tags,
        })
    }

    /// Apply a transform to each path's tag list; returns final tags per rel path.
    fn mutate(
        &self,
        paths: &[PathBuf],
        transform: &dyn Fn(Vec<String>) -> Vec<String>,
    ) -> Result<BTreeMap<String, Vec<String>>, String> {
        if paths.is_empty() {
            return Err("no files given".to_string());
        }
        let rels: Vec<(PathBuf, String)> = paths
            .iter()
            .map(|p| Ok((p.clone(), self.rel(p)?)))
            .collect::<Result<_, String>>()?;

        let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
        {
            let _lock = lock::acquire(&self.store_dir, lock::DEFAULT_TIMEOUT, lock::STALE_AFTER)?;
            let mut idx = self.load()?;
            for (path, rel) in &rels {
                let prev = idx.entries.get(rel).cloned();
                let mut new_tags =
                    transform(prev.as_ref().map(|e| e.tags.clone()).unwrap_or_default());
                new_tags.sort();
                new_tags.dedup();
                if new_tags.is_empty() {
                    idx.entries.remove(rel);
                } else {
                    let entry = self.entry_for(path, new_tags.clone(), prev.as_ref())?;
                    idx.entries.insert(rel.clone(), entry);
                }
                result.insert(rel.clone(), new_tags);
            }
            idx.save(&self.index_path)?;
        }
        invalidate_cache(&self.root);
        for (rel, tags) in &result {
            xattrs::mirror_tags(&self.abspath(rel), tags);
        }
        Ok(result)
    }

    pub fn add(
        &self,
        paths: &[PathBuf],
        tags: &[String],
    ) -> Result<BTreeMap<String, Vec<String>>, String> {
        let vt = validate_all(tags)?;
        if vt.is_empty() {
            return Err("no tags given".to_string());
        }
        self.mutate(paths, &move |mut cur| {
            cur.extend(vt.iter().cloned());
            cur
        })
    }

    pub fn remove(
        &self,
        paths: &[PathBuf],
        tags: &[String],
        all_tags: bool,
    ) -> Result<BTreeMap<String, Vec<String>>, String> {
        if all_tags {
            return self.mutate(paths, &|_| Vec::new());
        }
        let vt = validate_all(tags)?;
        if vt.is_empty() {
            return Err("no tags given (use --all to clear every tag)".to_string());
        }
        self.mutate(paths, &move |cur| {
            cur.into_iter().filter(|t| !vt.contains(t)).collect()
        })
    }

    pub fn set_tags(
        &self,
        paths: &[PathBuf],
        tags: &[String],
    ) -> Result<BTreeMap<String, Vec<String>>, String> {
        let vt = validate_all(tags)?;
        if vt.is_empty() {
            return Err("no tags given".to_string());
        }
        self.mutate(paths, &move |_| vt.clone())
    }

    pub fn get(&self, path: &Path) -> Result<Vec<String>, String> {
        let rel = self.rel(path)?;
        Ok(self
            .load()?
            .entries
            .get(&rel)
            .map(|e| e.tags.clone())
            .unwrap_or_default())
    }

    /// Query the index: all `require` tags present, at least one of `any_of`
    /// (when non-empty), none of `exclude`. No constraints lists everything.
    pub fn find(
        &self,
        require: &[String],
        any_of: &[String],
        exclude: &[String],
    ) -> Result<Vec<(String, Vec<String>)>, String> {
        let idx = self.load()?;
        let mut out = Vec::new();
        for (rel, entry) in &idx.entries {
            let has = |t: &String| entry.tags.contains(t);
            if !require.iter().all(has) {
                continue;
            }
            if !any_of.is_empty() && !any_of.iter().any(has) {
                continue;
            }
            if exclude.iter().any(has) {
                continue;
            }
            out.push((rel.clone(), entry.tags.clone()));
        }
        Ok(out)
    }

    pub fn tag_counts(&self) -> Result<BTreeMap<String, usize>, String> {
        let idx = self.load()?;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for entry in idx.entries.values() {
            for tag in &entry.tags {
                *counts.entry(tag.clone()).or_default() += 1;
            }
        }
        Ok(counts)
    }

    /// Immediate children of a directory with their tags, sorted by name.
    pub fn list_dir(&self, directory: &Path) -> Result<Vec<ChildEntry>, String> {
        let resolved = absolutize(directory)?;
        if !resolved.is_dir() {
            return Err(format!("{} is not a directory", directory.display()));
        }
        let prefix = if resolved == self.root {
            String::new()
        } else {
            format!("{}/", self.rel(&resolved)?)
        };
        let idx = self.load()?;
        let mut rows: Vec<ChildEntry> = Vec::new();
        let dir_iter = std::fs::read_dir(&resolved)
            .map_err(|e| format!("cannot read {}: {}", resolved.display(), e))?;
        for entry in dir_iter.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == STORE_DIR {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let tags = if meta.is_dir() {
                Vec::new()
            } else {
                idx.entries
                    .get(&format!("{}{}", prefix, name))
                    .map(|e| e.tags.clone())
                    .unwrap_or_default()
            };
            rows.push(ChildEntry {
                name,
                is_dir: meta.is_dir(),
                tags,
                meta,
            });
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    }

    /// Move or rename a file and update the index in one step.
    pub fn mv(&self, src: &Path, dst: &Path) -> Result<(String, String), String> {
        let src_rel = self.rel(src)?;
        self.require_regular_file(src)?;
        let dst_target = if dst.is_dir() {
            dst.join(src.file_name().ok_or("source has no file name")?)
        } else {
            dst.to_path_buf()
        };
        let dst_rel = self.rel(&dst_target)?;
        let mut moved_tags: Vec<String> = Vec::new();
        {
            let _lock = lock::acquire(&self.store_dir, lock::DEFAULT_TIMEOUT, lock::STALE_AFTER)?;
            let mut idx = self.load()?;
            if dst_target.exists() {
                return Err(format!("{} already exists", dst_target.display()));
            }
            if idx.entries.contains_key(&dst_rel) {
                return Err(format!("{} is already tracked in the index", dst_rel));
            }
            std::fs::rename(src, &dst_target).map_err(|e| format!("rename failed: {}", e))?;
            if let Some(entry) = idx.entries.remove(&src_rel) {
                let meta = std::fs::symlink_metadata(&dst_target)
                    .map_err(|e| format!("cannot stat {}: {}", dst_target.display(), e))?;
                moved_tags = entry.tags.clone();
                idx.entries.insert(
                    dst_rel.clone(),
                    Entry {
                        fp: entry.fp,
                        mtime_ns: mtime_ns(&meta),
                        size: meta.len(),
                        tags: entry.tags,
                    },
                );
                idx.save(&self.index_path)?;
            }
        }
        invalidate_cache(&self.root);
        if !moved_tags.is_empty() {
            xattrs::mirror_tags(&dst_target, &moved_tags);
        }
        Ok((src_rel, dst_rel))
    }

    /// Reconcile the index with the tree: relink renamed/moved files by
    /// fingerprint, refresh entries modified in place, optionally prune
    /// entries whose file is gone.
    pub fn repair(&self, prune: bool, dry_run: bool) -> Result<RepairReport, String> {
        let mut report = RepairReport::default();
        {
            let _lock = lock::acquire(&self.store_dir, lock::DEFAULT_TIMEOUT, lock::STALE_AFTER)?;
            let mut idx = self.load()?;
            let disk = self.scan()?;

            let mut by_size: BTreeMap<u64, Vec<String>> = BTreeMap::new();
            for (rel, meta) in &disk {
                if !idx.entries.contains_key(rel) {
                    by_size.entry(meta.len()).or_default().push(rel.clone());
                }
            }
            let mut fp_cache: HashMap<String, Option<String>> = HashMap::new();

            let orphans: Vec<String> = idx
                .entries
                .keys()
                .filter(|k| !disk.contains_key(*k))
                .cloned()
                .collect();
            let mut used: std::collections::HashSet<String> = Default::default();
            for old in orphans {
                let e = idx.entries[&old].clone();
                let matches: Vec<String> = by_size
                    .get(&e.size)
                    .map(|cands| {
                        cands
                            .iter()
                            .filter(|c| !used.contains(*c))
                            .filter(|c| {
                                fp_cache
                                    .entry((*c).clone())
                                    .or_insert_with(|| fingerprint_file(&self.abspath(c)).ok())
                                    .as_deref()
                                    == Some(e.fp.as_str())
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                match matches.len() {
                    1 => {
                        let new = matches.into_iter().next().unwrap();
                        used.insert(new.clone());
                        let meta = &disk[&new];
                        idx.entries.insert(
                            new.clone(),
                            Entry {
                                fp: e.fp.clone(),
                                mtime_ns: mtime_ns(meta),
                                size: meta.len(),
                                tags: e.tags.clone(),
                            },
                        );
                        idx.entries.remove(&old);
                        report.relinked.push((old, new));
                    }
                    0 => {
                        report.missing.push(old.clone());
                        if prune {
                            idx.entries.remove(&old);
                            report.pruned.push(old);
                        }
                    }
                    _ => report.ambiguous.push((old, matches)),
                }
            }

            let rels: Vec<String> = idx.entries.keys().cloned().collect();
            for rel in rels {
                let Some(meta) = disk.get(&rel) else { continue }; // unresolved orphan
                let e = &idx.entries[&rel];
                if meta.len() != e.size || mtime_ns(meta) != e.mtime_ns {
                    let new_fp = fingerprint_file(&self.abspath(&rel))?;
                    let tags = e.tags.clone();
                    idx.entries.insert(
                        rel.clone(),
                        Entry {
                            fp: new_fp,
                            mtime_ns: mtime_ns(meta),
                            size: meta.len(),
                            tags,
                        },
                    );
                    report.refreshed.push(rel);
                }
            }

            let changed = !report.relinked.is_empty()
                || !report.refreshed.is_empty()
                || !report.pruned.is_empty();
            if changed && !dry_run {
                idx.save(&self.index_path)?;
            }
            if !dry_run {
                for (_old, new) in &report.relinked {
                    let tags = idx
                        .entries
                        .get(new)
                        .map(|e| e.tags.clone())
                        .unwrap_or_default();
                    xattrs::mirror_tags(&self.abspath(new), &tags);
                }
            }
        }
        invalidate_cache(&self.root);
        Ok(report)
    }

    /// Read-only verification of the index against the tree.
    pub fn check(&self, verify: bool) -> Result<CheckReport, String> {
        let idx = self.load()?;
        let disk = self.scan()?;
        let mut report = CheckReport {
            entry_count: idx.entries.len(),
            untracked_count: disk
                .keys()
                .filter(|r| !idx.entries.contains_key(*r))
                .count(),
            ..Default::default()
        };
        for (rel, e) in &idx.entries {
            match disk.get(rel) {
                None => report.missing.push(rel.clone()),
                Some(meta) if meta.len() != e.size || mtime_ns(meta) != e.mtime_ns => {
                    report.modified.push(rel.clone());
                }
                Some(_) if verify => {
                    if fingerprint_file(&self.abspath(rel))? != e.fp {
                        report.fp_mismatch.push(rel.clone());
                    }
                }
                Some(_) => {}
            }
        }
        Ok(report)
    }

    /// Map every regular file under the root (except .tagstore) to its metadata.
    fn scan(&self) -> Result<BTreeMap<String, std::fs::Metadata>, String> {
        let mut out = BTreeMap::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let iter = std::fs::read_dir(&dir)
                .map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
            for entry in iter.filter_map(|e| e.ok()) {
                if entry.file_name() == STORE_DIR {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(entry.path());
                } else if meta.is_file() {
                    let rel = entry
                        .path()
                        .strip_prefix(&self.root)
                        .map(|r| r.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if !rel.is_empty() {
                        out.insert(rel, meta);
                    }
                }
            }
        }
        Ok(out)
    }
}

fn validate_all(tags: &[String]) -> Result<Vec<String>, String> {
    tags.iter().map(|t| validate_tag(t)).collect()
}

/// Resolve to an absolute, symlink-free path. Unlike canonicalize, tolerates
/// a non-existent final component (needed for mv destinations) by resolving
/// the parent instead.
fn absolutize(path: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = path.canonicalize() {
        return Ok(p);
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let file_name = path.file_name();
    match (parent, file_name) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|p| p.join(name))
            .map_err(|e| format!("{}: {}", path.display(), e)),
        _ => Err(format!("{}: cannot resolve path", path.display())),
    }
}

#[cfg(test)]
mod tests;
