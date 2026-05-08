use crate::parser::ParsedSource;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// In-memory parse cache for the daemon.
/// Stores parsed ASTs keyed by canonical file path.
pub struct ParseCache {
    entries: HashMap<PathBuf, ParsedSource>,
}

/// Serializable cache metrics returned by the `status` command.
#[derive(Debug, Clone, Serialize)]
pub struct CacheStatus {
    pub entries: usize,
}

impl Default for ParseCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ParseCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Look up a previously parsed file.
    pub fn get(&self, path: &Path) -> Option<&ParsedSource> {
        self.entries.get(path)
    }

    /// Store a parsed source, replacing any previous entry for the same path.
    pub fn insert(&mut self, path: PathBuf, parsed: ParsedSource) {
        self.entries.insert(path, parsed);
    }

    /// Remove a single entry from the cache.
    #[allow(dead_code)]
    pub fn remove(&mut self, path: &Path) -> Option<ParsedSource> {
        self.entries.remove(path)
    }

    /// Drop every cached entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Iterate over all cached (path, source) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &ParsedSource)> {
        self.entries.iter()
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return serializable status metrics.
    pub fn status(&self) -> CacheStatus {
        CacheStatus {
            entries: self.len(),
        }
    }
}
