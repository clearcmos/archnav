pub mod database;
pub mod engine;
pub mod highlight;
pub mod integrity;
pub mod query;
pub mod scanner;
pub mod selection;
pub mod trigram;
pub mod watcher;

pub use engine::CoreEngine;
pub use query::{FileTypeMode, ParsedQuery, QueryMode, SortOrder};
pub use trigram::{Bookmark, FileEntry, SearchResult, TrigramIndex};
